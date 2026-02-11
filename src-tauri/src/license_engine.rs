use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context};
use serde::Deserialize;

use crate::db::Db;
use crate::util::{is_network_error, now_iso, parse_rfc3339_to_utc};

const KEY_SUPABASE_URL: &str = "supabase_url";
const KEY_SUPABASE_ANON_KEY: &str = "supabase_anon_key";
const KEY_SUPABASE_API_KEY: &str = "supabase_api_key";

const KEY_CLINIC_ID: &str = "clinic_id";
const KEY_CLINIC_NAME: &str = "clinic_name";

const KEY_ERROR_LOG_PATH: &str = "error_log_path";

const KEY_LICENSE_ACTIVE_UNTIL: &str = "license_active_until";
const KEY_LICENSE_DISABLED: &str = "license_disabled";
const KEY_LICENSE_APPROVED: &str = "license_approved";
const KEY_LICENSE_ENFORCE_IP: &str = "license_enforce_ip";
const KEY_LICENSE_ALLOWED_IP_LIST: &str = "license_allowed_ip_list";
const KEY_LICENSE_LAST_PUBLIC_IP: &str = "license_last_public_ip";
const KEY_LICENSE_LAST_CHECKED_AT: &str = "license_last_checked_at";

const GRACE_DAYS: i64 = 7;

fn append_license_error(db: &Db, source: &str, message: &str) {
  let path: Option<PathBuf> = db.setting_get(KEY_ERROR_LOG_PATH).ok().flatten().map(PathBuf::from);
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

fn ip_allowed(ip: &str, allowed_list: &str) -> bool {
  let ip = ip.trim();
  if ip.is_empty() {
    return false;
  }
  for item in allowed_list.split(&[',', ';', '\n', '\r'][..]) {
    let item = item.trim();
    if item.is_empty() {
      continue;
    }
    if item == ip {
      return true;
    }
  }
  false
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LicensePublicState {
  pub ok: bool,
  // ok | expired | disabled | pending_approval | ip_blocked | offline_grace | unconfigured | error
  pub status: String,
  pub active_until: Option<String>,
  pub last_checked_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ClinicRegistryRow {
  pub clinic_id: Option<String>,
  pub clinic_name: Option<String>,
  pub approved: Option<bool>,
  pub disabled: Option<bool>,
  pub active_until: Option<String>,
  pub enforce_ip: Option<bool>,
  pub allowed_ip_list: Option<String>,
  pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct PublicIpRow {
  pub ip: String,
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

  async fn fetch_public_ip(&self) -> Option<String> {
    let resp = self
      .client
      .get("https://api.ipify.org")
      .query(&[("format", "json")])
      .send()
      .await
      .ok()?;
    if !resp.status().is_success() {
      return None;
    }
    let body = resp.text().await.ok()?;
    let parsed: PublicIpRow = serde_json::from_str(&body).ok()?;
    let ip = parsed.ip.trim().to_string();
    if ip.is_empty() {
      None
    } else {
      Some(ip)
    }
  }

  pub async fn check_now(&self) -> anyhow::Result<()> {
    let _guard = self.lock.lock().await;

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

    let clinic_id = self.db.setting_get(KEY_CLINIC_ID)?.unwrap_or_default();
    if clinic_id.trim().is_empty() {
      let st = LicensePublicState {
        ok: true,
        status: "unconfigured".to_string(),
        active_until: None,
        last_checked_at: None,
      };
      self.set_state(st).await;
      return Ok(());
    }
    let clinic_name = self.db.setting_get(KEY_CLINIC_NAME)?.unwrap_or_default();

    let base = supabase_url.unwrap().trim_end_matches('/').to_string();
    let api_key = api_key.unwrap();
    let now = now_iso();
    let public_ip = self.fetch_public_ip().await;

    let url = format!("{base}/rest/v1/clinic_registry");
    let resp = with_supabase_auth(self.client.get(&url), &api_key)
      .query(&[
        (
          "select",
          "clinic_id,clinic_name,approved,disabled,active_until,enforce_ip,allowed_ip_list,updated_at",
        ),
        ("clinic_id", &format!("eq.{}", clinic_id.trim())),
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
      append_license_error(self.db.as_ref(), "license_check", &format!("license check failed: {status} {body}"));
      let mut st = Self::compute_offline_state(self.db.as_ref())?;
      st.ok = st.ok && st.status == "offline_grace";
      st.status = "error".to_string();
      self.set_state(st).await;
      bail!("license check failed: {status} {body}");
    }

    let mut rows: Vec<ClinicRegistryRow> = serde_json::from_str(&body).context("decode clinic_registry json")?;
    if rows.is_empty() {
      let create_payload = serde_json::json!({
        "clinic_id": clinic_id.trim(),
        "clinic_name": clinic_name.trim(),
        "approved": false,
        "disabled": false,
        "active_until": serde_json::Value::Null,
        "enforce_ip": false,
        "allowed_ip_list": "",
        "last_seen_at": now,
        "last_seen_ip": public_ip.clone().unwrap_or_default(),
        "created_at": now,
        "updated_at": now
      });
      let r = with_supabase_auth(self.client.post(&url), &api_key)
        .header("Prefer", "resolution=merge-duplicates,return=minimal")
        .json(&create_payload)
        .send()
        .await;
      match r {
        Ok(resp) if resp.status().is_success() => {}
        Ok(resp) => {
          let st = resp.status();
          let txt = resp.text().await.unwrap_or_default();
          append_license_error(
            self.db.as_ref(),
            "license_register",
            &format!("failed to register clinic: {} {}", st, txt),
          );
        }
        Err(e) => {
          append_license_error(self.db.as_ref(), "license_register", &e.to_string());
        }
      }

      self.db.setting_set(KEY_LICENSE_DISABLED, "0")?;
      self.db.setting_set(KEY_LICENSE_APPROVED, "0")?;
      self.db.setting_set(KEY_LICENSE_ENFORCE_IP, "0")?;
      self.db.setting_set(KEY_LICENSE_ALLOWED_IP_LIST, "")?;
      self.db.setting_set(KEY_LICENSE_ACTIVE_UNTIL, "")?;
      self.db.setting_set(KEY_LICENSE_LAST_PUBLIC_IP, &public_ip.clone().unwrap_or_default())?;
      self.db.setting_set(KEY_LICENSE_LAST_CHECKED_AT, &now)?;

      let st = LicensePublicState {
        ok: false,
        status: "pending_approval".to_string(),
        active_until: None,
        last_checked_at: Some(now),
      };
      self.set_state(st).await;
      return Ok(());
    }

    let row = rows.remove(0);
    let approved = row.approved.unwrap_or(false);
    let disabled = row.disabled.unwrap_or(false);
    let active_until = row.active_until.and_then(|x| {
      let t = x.trim().to_string();
      if t.is_empty() {
        None
      } else {
        Some(t)
      }
    });
    let enforce_ip = row.enforce_ip.unwrap_or(false);
    let allowed_ip_list = row.allowed_ip_list.unwrap_or_default();

    // Heartbeat: update seen info; ignore errors.
    let heartbeat_payload = serde_json::json!({
      "clinic_name": clinic_name.trim(),
      "last_seen_at": now,
      "last_seen_ip": public_ip.clone().unwrap_or_default(),
      "updated_at": now
    });
    let heartbeat = with_supabase_auth(
      self
        .client
        .patch(format!("{url}?clinic_id=eq.{}", urlencoding::encode(clinic_id.trim()))),
      &api_key,
    )
    .header("Prefer", "return=minimal")
    .json(&heartbeat_payload)
    .send()
    .await;
    if let Ok(resp) = heartbeat {
      if !resp.status().is_success() {
        let st = resp.status();
        let txt = resp.text().await.unwrap_or_default();
        append_license_error(
          self.db.as_ref(),
          "license_heartbeat",
          &format!("heartbeat failed: {} {}", st, txt),
        );
      }
    }

    self
      .db
      .setting_set(KEY_LICENSE_DISABLED, if disabled { "1" } else { "0" })?;
    self
      .db
      .setting_set(KEY_LICENSE_APPROVED, if approved { "1" } else { "0" })?;
    self
      .db
      .setting_set(KEY_LICENSE_ENFORCE_IP, if enforce_ip { "1" } else { "0" })?;
    self.db.setting_set(KEY_LICENSE_ALLOWED_IP_LIST, &allowed_ip_list)?;
    self.db.setting_set(KEY_LICENSE_LAST_PUBLIC_IP, &public_ip.clone().unwrap_or_default())?;
    self.db.setting_set(
      KEY_LICENSE_ACTIVE_UNTIL,
      active_until.as_deref().unwrap_or(""),
    )?;
    self.db.setting_set(KEY_LICENSE_LAST_CHECKED_AT, &now)?;

    let mut ok = true;
    let mut st = "ok".to_string();
    if disabled {
      ok = false;
      st = "disabled".to_string();
    } else if !approved {
      ok = false;
      st = "pending_approval".to_string();
    }

    if ok && enforce_ip {
      let ip_to_check = public_ip
        .clone()
        .or_else(|| self.db.setting_get(KEY_LICENSE_LAST_PUBLIC_IP).ok().flatten());
      if let Some(ip) = ip_to_check {
        if !ip_allowed(&ip, &allowed_ip_list) {
          ok = false;
          st = "ip_blocked".to_string();
        }
      } else if !allowed_ip_list.trim().is_empty() {
        ok = false;
        st = "ip_blocked".to_string();
      }
    }

    if ok {
      if let Some(u) = active_until.as_deref().filter(|x| !x.trim().is_empty()) {
        let until = parse_rfc3339_to_utc(u)?;
        let now_dt = parse_rfc3339_to_utc(&now)?;
        if now_dt > until {
          ok = false;
          st = "expired".to_string();
        }
      } else {
        ok = false;
        st = "expired".to_string();
      }
    }

    let out = LicensePublicState {
      ok,
      status: st,
      active_until,
      last_checked_at: Some(now),
    };
    self.set_state(out).await;
    Ok(())
  }

  async fn set_state(&self, st: LicensePublicState) {
    let mut w = self.state.write().await;
    *w = st;
  }

  fn compute_offline_state(db: &Db) -> anyhow::Result<LicensePublicState> {
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
    let approved = db.setting_get(KEY_LICENSE_APPROVED)?.unwrap_or_default().trim() == "1";
    let enforce_ip = db.setting_get(KEY_LICENSE_ENFORCE_IP)?.unwrap_or_default().trim() == "1";
    let allowed_ip_list = db.setting_get(KEY_LICENSE_ALLOWED_IP_LIST)?.unwrap_or_default();
    let last_public_ip = db.setting_get(KEY_LICENSE_LAST_PUBLIC_IP)?.unwrap_or_default();

    let active_until = db.setting_get(KEY_LICENSE_ACTIVE_UNTIL)?.and_then(|x| {
      let t = x.trim().to_string();
      if t.is_empty() {
        None
      } else {
        Some(t)
      }
    });
    let last_checked_at = db.setting_get(KEY_LICENSE_LAST_CHECKED_AT)?.and_then(|x| {
      let t = x.trim().to_string();
      if t.is_empty() {
        None
      } else {
        Some(t)
      }
    });

    if disabled {
      return Ok(LicensePublicState {
        ok: false,
        status: "disabled".to_string(),
        active_until,
        last_checked_at,
      });
    }

    if !approved {
      return Ok(LicensePublicState {
        ok: false,
        status: "pending_approval".to_string(),
        active_until,
        last_checked_at,
      });
    }

    if enforce_ip && !allowed_ip_list.trim().is_empty() && !ip_allowed(&last_public_ip, &allowed_ip_list) {
      return Ok(LicensePublicState {
        ok: false,
        status: "ip_blocked".to_string(),
        active_until,
        last_checked_at,
      });
    }

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
    } else {
      return Ok(LicensePublicState {
        ok: false,
        status: "expired".to_string(),
        active_until,
        last_checked_at,
      });
    }

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
