use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::db::Db;
use crate::models::{Client, Payment, Sale, SyncQueueItem};
use crate::util::{is_network_error, now_iso, parse_rfc3339_to_utc};

const KEY_SUPABASE_URL: &str = "supabase_url";
const KEY_SUPABASE_ANON_KEY: &str = "supabase_anon_key";
const KEY_SUPABASE_API_KEY: &str = "supabase_api_key";

fn looks_like_jwt(token: &str) -> bool {
  let t = token.trim();
  // Supabase legacy anon keys are JWTs that typically start with `eyJ` and have 3 segments.
  t.starts_with("eyJ") && t.matches('.').count() >= 2
}

fn with_supabase_auth(req: reqwest::RequestBuilder, api_key: &str) -> reqwest::RequestBuilder {
  let req = req.header("apikey", api_key);
  // Only send Authorization when the key is JWT-like; sending a non-JWT publishable key can cause 401.
  if looks_like_jwt(api_key) {
    req.bearer_auth(api_key)
  } else {
    req
  }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SyncPublicStatus {
  pub sync_status: String, // synced | pending | error
  pub last_sync_error: Option<String>,
}

pub struct SyncEngine {
  db: Arc<Db>,
  client: reqwest::Client,
  lock: tokio::sync::Mutex<()>,
  status: tokio::sync::RwLock<SyncPublicStatus>,
}

impl SyncEngine {
  pub fn new(db: Arc<Db>) -> anyhow::Result<Self> {
    let client = reqwest::Client::builder()
      .timeout(Duration::from_secs(20))
      .build()
      .context("build http client")?;
    Ok(Self {
      db,
      client,
      lock: tokio::sync::Mutex::new(()),
      status: tokio::sync::RwLock::new(SyncPublicStatus {
        sync_status: "pending".to_string(),
        last_sync_error: None,
      }),
    })
  }

  pub async fn get_status(&self) -> SyncPublicStatus {
    self.status.read().await.clone()
  }

  pub fn spawn_background(self: Arc<Self>) {
    tauri::async_runtime::spawn(async move {
      // Startup sync.
      let _ = self.sync_now().await;

      let mut interval = tokio::time::interval(Duration::from_secs(120));
      loop {
        interval.tick().await;
        let _ = self.sync_now().await;
      }
    });
  }

  pub async fn sync_now(&self) -> anyhow::Result<()> {
    let _guard = self.lock.lock().await;

    let supabase_url = self.db.setting_get(KEY_SUPABASE_URL)?;
    let api_key = self
      .db
      .setting_get(KEY_SUPABASE_API_KEY)?
      .or_else(|| self.db.setting_get(KEY_SUPABASE_ANON_KEY).ok().flatten());

    if supabase_url.as_deref().unwrap_or("").trim().is_empty() || api_key.as_deref().unwrap_or("").trim().is_empty() {
      self.set_status("pending", None).await;
      return Ok(());
    }

    let supabase_url = supabase_url.unwrap();
    let api_key = api_key.unwrap();

    let last_sync_time = self
      .db
      .get_last_sync_time()?
      .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string());
    let sync_started_at = now_iso();

    // Pull first (to avoid overwriting newer remote updates).
    if let Err(e) = self.pull_updates(&supabase_url, &api_key, &last_sync_time).await {
      if contains_network_error(&e) {
        self.set_status("pending", Some("offline".to_string())).await;
        return Ok(());
      }
      self.set_status("error", Some(e.to_string())).await;
      return Err(e);
    }

    // Push queue.
    if let Err(e) = self.push_queue(&supabase_url, &api_key).await {
      if contains_network_error(&e) {
        self.set_status("pending", Some("offline".to_string())).await;
        return Ok(());
      }
      self.set_status("error", Some(e.to_string())).await;
      return Err(e);
    }

    // Mark successful sync time.
    self.db.set_last_sync_time(&sync_started_at)?;

    let pending = self.db.sync_queue_count_pending()?;
    if pending == 0 {
      self.set_status("synced", None).await;
    } else {
      self.set_status("pending", None).await;
    }
    Ok(())
  }

  async fn set_status(&self, status: &str, err: Option<String>) {
    let mut s = self.status.write().await;
    s.sync_status = status.to_string();
    s.last_sync_error = err;
  }

  async fn pull_updates(&self, supabase_url: &str, api_key: &str, last_sync_time: &str) -> anyhow::Result<()> {
    let base = supabase_url.trim_end_matches('/');
    let clients: Vec<Client> = self
      .fetch_table(base, api_key, "clients", last_sync_time)
      .await
      .context("pull clients")?;
    for c in clients {
      self.apply_remote_row_clients(&c)?;
    }

    let sales: Vec<Sale> = self
      .fetch_table(base, api_key, "sales", last_sync_time)
      .await
      .context("pull sales")?;
    for s in sales {
      self.apply_remote_row_sales(&s)?;
    }

    let payments: Vec<Payment> = self
      .fetch_table(base, api_key, "payments", last_sync_time)
      .await
      .context("pull payments")?;
    for p in payments {
      self.apply_remote_row_payments(&p)?;
    }

    Ok(())
  }

  fn apply_remote_row_clients(&self, remote: &Client) -> anyhow::Result<()> {
    let local = self.db.clients_updated_at(&remote.id)?;
    if let Some(local_ts) = local {
      if newer_or_equal(&local_ts, &remote.updated_at)? {
        return Ok(());
      }
    }
    // Remote wins: drop pending local edits for this row (LWW).
    self.db.sync_queue_drop_pending_for_row("clients", &remote.id)?;
    self.db.apply_remote_client(remote)?;
    Ok(())
  }

  fn apply_remote_row_sales(&self, remote: &Sale) -> anyhow::Result<()> {
    let local = self.db.sales_updated_at(&remote.id)?;
    if let Some(local_ts) = local {
      if newer_or_equal(&local_ts, &remote.updated_at)? {
        return Ok(());
      }
    }
    self.db.sync_queue_drop_pending_for_row("sales", &remote.id)?;
    self.db.apply_remote_sale(remote)?;
    Ok(())
  }

  fn apply_remote_row_payments(&self, remote: &Payment) -> anyhow::Result<()> {
    let local = self.db.payments_updated_at(&remote.id)?;
    if let Some(local_ts) = local {
      if newer_or_equal(&local_ts, &remote.updated_at)? {
        return Ok(());
      }
    }
    self.db.sync_queue_drop_pending_for_row("payments", &remote.id)?;
    self.db.apply_remote_payment(remote)?;
    Ok(())
  }

  async fn fetch_table<T: DeserializeOwned>(
    &self,
    base: &str,
    api_key: &str,
    table: &str,
    last_sync_time: &str,
  ) -> anyhow::Result<Vec<T>> {
    let url = format!("{base}/rest/v1/{table}");
    let resp = with_supabase_auth(self.client.get(&url), api_key)
      .query(&[
        ("select", "*"),
        ("updated_at", &format!("gt.{last_sync_time}")),
        ("order", "updated_at.asc"),
      ])
      .send()
      .await?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
      bail!("supabase pull failed {table}: {status} {body}");
    }
    let rows: Vec<T> = serde_json::from_str(&body).context("decode json")?;
    Ok(rows)
  }

  async fn push_queue(&self, supabase_url: &str, api_key: &str) -> anyhow::Result<()> {
    let items = self.db.sync_queue_list_pending(200)?;
    if items.is_empty() {
      return Ok(());
    }

    let base = supabase_url.trim_end_matches('/');
    let mut upserts: HashMap<String, Vec<(String, Value)>> = HashMap::new();
    let mut deletes: Vec<(SyncQueueItem, Value)> = Vec::new();

    for it in items {
      let v: Value = serde_json::from_str(&it.payload).context("parse sync payload")?;
      match it.op.as_str() {
        "delete" => deletes.push((it, v)),
        _ => {
          upserts
            .entry(it.table_name.clone())
            .or_default()
            .push((it.id.clone(), v));
        }
      }
    }

    // Upserts in batches per table.
    for (table, mut rows) in upserts {
      while !rows.is_empty() {
        let batch: Vec<(String, Value)> = rows.drain(0..rows.len().min(50)).collect();
        let payload: Vec<Value> = batch.iter().map(|(_, v)| v.clone()).collect();
        let url = format!("{base}/rest/v1/{table}?on_conflict=id");
        let resp = with_supabase_auth(self.client.post(&url), api_key)
          .header("Prefer", "resolution=merge-duplicates,return=minimal")
          .json(&payload)
          .send()
          .await?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
          let msg = format!("supabase upsert failed {table}: {status} {body}");
          for (qid, _) in batch {
            self.db.sync_queue_mark_failed(&qid, &msg)?;
          }
          bail!("{msg}");
        }
        for (qid, _) in batch {
          self.db.sync_queue_mark_sent(&qid)?;
        }
      }
    }

    // Deletes individually (PATCH deleted=1).
    for (it, v) in deletes {
      let id = v
        .get("id")
        .and_then(|x| x.as_str())
        .ok_or_else(|| anyhow!("delete payload missing id"))?
        .to_string();
      let updated_at = v
        .get("updated_at")
        .and_then(|x| x.as_str())
        .ok_or_else(|| anyhow!("delete payload missing updated_at"))?
        .to_string();

      let url = format!("{base}/rest/v1/{}?id=eq.{}", it.table_name, urlencoding::encode(&id));
      let body = serde_json::json!({ "deleted": 1, "updated_at": updated_at });
      let resp = with_supabase_auth(self.client.patch(&url), api_key)
        .header("Prefer", "return=minimal")
        .json(&body)
        .send()
        .await?;
      let status = resp.status();
      let text = resp.text().await.unwrap_or_default();
      if !status.is_success() {
        let msg = format!("supabase delete failed {}: {status} {text}", it.table_name);
        self.db.sync_queue_mark_failed(&it.id, &msg)?;
        bail!("{msg}");
      }
      self.db.sync_queue_mark_sent(&it.id)?;
    }

    Ok(())
  }
}

fn newer_or_equal(local_ts: &str, remote_ts: &str) -> anyhow::Result<bool> {
  let l = parse_rfc3339_to_utc(local_ts)?;
  let r = parse_rfc3339_to_utc(remote_ts)?;
  Ok(l >= r)
}

fn contains_network_error(e: &anyhow::Error) -> bool {
  e.chain().any(|c| {
    c.downcast_ref::<reqwest::Error>()
      .map(is_network_error)
      .unwrap_or(false)
  })
}
