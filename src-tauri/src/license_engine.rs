use std::sync::Arc;
use std::time::Duration;
use std::path::PathBuf;

use anyhow::{bail, Context};
use serde::Deserialize;

use crate::db::Db;
use crate::util::{is_network_error, now_iso, parse_rfc3339_to_utc};

const KEY_SUPABASE_URL: &str = "supabase_url";
const KEY_SUPABASE_ANON_KEY: &str = "supabase_anon_key";
const KEY_SUPABASE_API_KEY: &str = "supabase_api_key";
const KEY_ERROR_LOG_PATH: &str = "error_log_path";

const KEY_LICENSE_ACTIVE_UNTIL: &str = "license_active_until";
const KEY_LICENSE_DISABLED: &str = "license_disabled";
const KEY_LICENSE_LAST_CHECKED_AT: &str = "license_last_checked_at";

const GRACE_DAYS: i64 = 7;

fn append_license_error(db: &Db, source: &str, message: &str) {
  let path: Option<PathBuf> = db
    .setting_get(KEY_ERROR_LOG_PATH)
    .ok()
    .flatten()
    .map(PathBuf::from);
  if let Some(path) = path {
    let _ = crate::error_logs::append(&path, source, message);
  }
}

fn looks_like_jwt(token: &str) -> bool {
  let t = token.trim();
  t.starts_with("eyJ") && t.matches('.').count() >= 2
}

fn with_supabase_auth(req: reqwest::RequestBuilder, api_key: &str) -> reqwest::RequestBuilder {
  let req = req.header("apikey", api_key);
  if looks_like_jwt(api_key) {
    req.bearer_auth(api_key)
  } else {
    req
  }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LicensePublicState {
  pub ok: bool,
  pub status: String, // ok | expired | disabled | offline_grace | unconfigured | error
  pub active_until: Option<String>,
  pub last_checked_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct LicenseRow {
  pub active_until: Option<String>,
  pub disabled: Option<bool>,
  pub updated_at: Option<String>,
}

pub struct LicenseEngine {
  db: Arc<Db>,
  client: reqwest::Client,
  state: tokio::sync::RwLock<LicensePublicState>,
  lock: tokio::sync::Mutex<()>,
}

impl LicenseEngine {
  pub fn new(db: Arc<Db>) -> anyhow::Result<Self> {
    let client = reqwest::Client::builder()
      .timeout(Duration::from_secs(15))
      .build()
      .context("build http client")?;

    // Compute initial state from stored settings (offline-safe).
    let initial = Self::compute_offline_state(db.as_ref())?;
    Ok(Self {
      db,
      client,
      state: tokio::sync::RwLock::new(initial),
      lock: tokio::sync::Mutex::new(()),
    })
  }

  pub async fn get_state(&self) -> LicensePublicState {
    self.state.read().await.clone()
  }

  pub async fn is_ok(&self) -> bool {
    self.state.read().await.ok
  }

  pub fn spawn_background(self: Arc<Self>) {
    tauri::async_runtime::spawn(async move {
      let _ = self.check_now().await;
      let mut interval = tokio::time::interval(Duration::from_secs(6 * 60 * 60));
      loop {
        interval.tick().await;
        let _ = self.check_now().await;
      }
    });
  }

  pub async fn check_now(&self) -> anyhow::Result<()> {
    let _guard = self.lock.lock().await;

    // If Supabase isn't configured yet, allow usage (dev/offline local-only mode).
    let supabase_url = self.db.setting_get(KEY_SUPABASE_URL)?;
    let api_key = self
      .db
      .setting_get(KEY_SUPABASE_API_KEY)?
      .or_else(|| self.db.setting_get(KEY_SUPABASE_ANON_KEY).ok().flatten());

    if supabase_url.as_deref().unwrap_or("").trim().is_empty() || api_key.as_deref().unwrap_or("").trim().is_empty() {
      let st = LicensePublicState {
        ok: true,
        status: "unconfigured".to_string(),
        active_until: None,
        last_checked_at: None,
      };
      self.set_state(st).await;
      return Ok(());
    }

    let base = supabase_url.unwrap().trim_end_matches('/').to_string();
    let api_key = api_key.unwrap();

    let url = format!("{base}/rest/v1/app_license");
    let resp = with_supabase_auth(self.client.get(&url), &api_key)
      .query(&[
        ("select", "active_until,disabled,updated_at"),
        ("order", "updated_at.desc"),
        ("limit", "1"),
      ])
      .send()
      .await;

    let resp = match resp {
      Ok(r) => r,
      Err(e) => {
        if is_network_error(&e) {
          append_license_error(self.db.as_ref(), "license_check", &format!("offline: {e}"));
          let st = Self::compute_offline_state(self.db.as_ref())?;
          self.set_state(st).await;
          return Ok(());
        }
        append_license_error(self.db.as_ref(), "license_check", &e.to_string());
        bail!("license check request failed: {e}");
      }
    };

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
      // Treat auth issues as "error" but still allow grace if previously valid.
      append_license_error(self.db.as_ref(), "license_check", &format!("license check failed: {status} {body}"));
      let mut st = Self::compute_offline_state(self.db.as_ref())?;
      st.ok = st.ok && st.status == "offline_grace";
      st.status = "error".to_string();
      self.set_state(st).await;
      bail!("license check failed: {status} {body}");
    }

    let rows: Vec<LicenseRow> = serde_json::from_str(&body).context("decode license json")?;
    let row = rows.into_iter().next();
    let now = now_iso();

    let mut disabled = false;
    let mut active_until: Option<String> = None;
    if let Some(r) = row {
      disabled = r.disabled.unwrap_or(false);
      active_until = r.active_until;
    }

    // Store for offline grace evaluation.
    self
      .db
      .setting_set(KEY_LICENSE_DISABLED, if disabled { "1" } else { "0" })?;
    if let Some(u) = active_until.as_deref() {
      self.db.setting_set(KEY_LICENSE_ACTIVE_UNTIL, u)?;
    } else {
      self.db.setting_set(KEY_LICENSE_ACTIVE_UNTIL, "")?;
    }
    self.db.setting_set(KEY_LICENSE_LAST_CHECKED_AT, &now)?;

    let ok = if disabled {
      false
    } else if let Some(u) = active_until.as_deref().filter(|x| !x.trim().is_empty()) {
      let until = parse_rfc3339_to_utc(u)?;
      let now_dt = parse_rfc3339_to_utc(&now)?;
      now_dt <= until
    } else {
      false
    };

    let st = LicensePublicState {
      ok,
      status: if disabled {
        "disabled".to_string()
      } else if ok {
        "ok".to_string()
      } else {
        "expired".to_string()
      },
      active_until,
      last_checked_at: Some(now),
    };
    self.set_state(st).await;
    Ok(())
  }

  async fn set_state(&self, st: LicensePublicState) {
    let mut w = self.state.write().await;
    *w = st;
  }

  fn compute_offline_state(db: &Db) -> anyhow::Result<LicensePublicState> {
    // If no Supabase config, treat as unconfigured (allowed).
    let supabase_url = db.setting_get(KEY_SUPABASE_URL)?;
    let api_key = db
      .setting_get(KEY_SUPABASE_API_KEY)?
      .or_else(|| db.setting_get(KEY_SUPABASE_ANON_KEY).ok().flatten());
    if supabase_url.as_deref().unwrap_or("").trim().is_empty() || api_key.as_deref().unwrap_or("").trim().is_empty() {
      return Ok(LicensePublicState {
        ok: true,
        status: "unconfigured".to_string(),
        active_until: None,
        last_checked_at: None,
      });
    }

    let disabled = db.setting_get(KEY_LICENSE_DISABLED)?.unwrap_or_default().trim() == "1";
    let active_until = db.setting_get(KEY_LICENSE_ACTIVE_UNTIL)?.and_then(|x| {
      let t = x.trim().to_string();
      if t.is_empty() { None } else { Some(t) }
    });
    let last_checked_at = db.setting_get(KEY_LICENSE_LAST_CHECKED_AT)?.and_then(|x| {
      let t = x.trim().to_string();
      if t.is_empty() { None } else { Some(t) }
    });

    if disabled {
      return Ok(LicensePublicState {
        ok: false,
        status: "disabled".to_string(),
        active_until,
        last_checked_at,
      });
    }

    // If we have an active_until and it's expired, block regardless of grace.
    if let Some(u) = active_until.as_deref() {
      if let (Ok(until), Ok(now)) = (parse_rfc3339_to_utc(u), parse_rfc3339_to_utc(&now_iso())) {
        if now > until {
          return Ok(LicensePublicState {
            ok: false,
            status: "expired".to_string(),
            active_until,
            last_checked_at,
          });
        }
      }
    }

    // Allow within grace days since last successful check.
    if let Some(ts) = last_checked_at.as_deref() {
      if let (Ok(last), Ok(now)) = (parse_rfc3339_to_utc(ts), parse_rfc3339_to_utc(&now_iso())) {
        let days = (now - last).num_days();
        if days <= GRACE_DAYS {
          return Ok(LicensePublicState {
            ok: true,
            status: "offline_grace".to_string(),
            active_until,
            last_checked_at,
          });
        }
      }
    }

    Ok(LicensePublicState {
      ok: false,
      status: "expired".to_string(),
      active_until,
      last_checked_at,
    })
  }
}
