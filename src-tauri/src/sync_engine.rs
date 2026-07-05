use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::db::Db;
use crate::models::{
    Appointment, CashEntry, Client, Doctor, FiscalJob, Offer, OfferItem, Payment, Prescription, Sale, Service, StockItem, StockMovement, StockSupplier, SyncQueueItem, Visit, VisitItem, DoctorAccount,
};
use crate::util::{is_network_error, now_iso, parse_rfc3339_to_utc};

const KEY_SUPABASE_URL: &str = "supabase_url";
const KEY_SUPABASE_ANON_KEY: &str = "supabase_anon_key";
const KEY_SUPABASE_API_KEY: &str = "supabase_api_key";
const KEY_ERROR_LOG_PATH: &str = "error_log_path";

fn append_sync_error(db: &Db, source: &str, message: &str) {
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

            let mut interval = tokio::time::interval(Duration::from_secs(30));
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
        let clinic_id = self.db.setting_get("clinic_id")?.unwrap_or_default();

        if supabase_url.as_deref().unwrap_or("").trim().is_empty()
            || api_key.as_deref().unwrap_or("").trim().is_empty()
            || clinic_id.trim().is_empty()
        {
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
        if let Err(e) = self
            .pull_updates(&supabase_url, &api_key, &last_sync_time, &clinic_id)
            .await
        {
            if contains_network_error(&e) {
                append_sync_error(self.db.as_ref(), "sync_pull", &format!("offline: {e}"));
                self.set_status("pending", Some("offline".to_string()))
                    .await;
                return Ok(());
            }
            append_sync_error(self.db.as_ref(), "sync_pull", &e.to_string());
            self.set_status("error", Some(e.to_string())).await;
            return Err(e);
        }

        // Push queue.
        if let Err(e) = self.push_queue(&supabase_url, &api_key, &clinic_id).await {
            if contains_network_error(&e) {
                append_sync_error(self.db.as_ref(), "sync_push", &format!("offline: {e}"));
                self.set_status("pending", Some("offline".to_string()))
                    .await;
                return Ok(());
            }
            append_sync_error(self.db.as_ref(), "sync_push", &e.to_string());
            self.set_status("error", Some(e.to_string())).await;
            return Err(e);
        }

        // Sync clinic-level settings (bank account) — non-fatal.
        let _ = self.sync_clinic_settings(&supabase_url, &api_key, &clinic_id).await;

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

    async fn pull_updates(
        &self,
        supabase_url: &str,
        api_key: &str,
        last_sync_time: &str,
        clinic_id: &str,
    ) -> anyhow::Result<()> {
        let base = supabase_url.trim_end_matches('/');

        let doctors: Vec<Doctor> = self
            .fetch_table(base, api_key, "doctors", last_sync_time, clinic_id)
            .await
            .context("pull doctors")?;
        for d in doctors {
            self.apply_remote_row_doctors(&d)?;
        }

        let doctor_accounts: Vec<DoctorAccount> = self
            .fetch_table(base, api_key, "doctor_accounts", last_sync_time, clinic_id)
            .await
            .context("pull doctor_accounts")?;
        for da in doctor_accounts {
            self.apply_remote_row_doctor_accounts(&da)?;
        }

        let services: Vec<Service> = self
            .fetch_table(base, api_key, "services", last_sync_time, clinic_id)
            .await
            .context("pull services")?;
        for s in services {
            self.apply_remote_row_services(&s)?;
        }

        let appointments: Vec<Appointment> = self
            .fetch_table(base, api_key, "appointments", last_sync_time, clinic_id)
            .await
            .context("pull appointments")?;
        for a in appointments {
            self.apply_remote_row_appointments(&a)?;
        }

        let clients: Vec<Client> = self
            .fetch_table(base, api_key, "clients", last_sync_time, clinic_id)
            .await
            .context("pull clients")?;
        for c in clients {
            self.apply_remote_row_clients(&c)?;
        }

        let sales: Vec<Sale> = self
            .fetch_table(base, api_key, "sales", last_sync_time, clinic_id)
            .await
            .context("pull sales")?;
        for s in sales {
            self.apply_remote_row_sales(&s)?;
        }

        let payments: Vec<Payment> = self
            .fetch_table(base, api_key, "payments", last_sync_time, clinic_id)
            .await
            .context("pull payments")?;
        for p in payments {
            self.apply_remote_row_payments(&p)?;
        }

        let visits: Vec<Visit> = self
            .fetch_table(base, api_key, "visits", last_sync_time, clinic_id)
            .await
            .context("pull visits")?;
        for v in visits {
            self.apply_remote_row_visits(&v)?;
        }

        let visit_items: Vec<VisitItem> = self
            .fetch_table(base, api_key, "visit_items", last_sync_time, clinic_id)
            .await
            .context("pull visit_items")?;
        for it in visit_items {
            self.apply_remote_row_visit_items(&it)?;
        }

        let cash: Vec<CashEntry> = self
            .fetch_table(base, api_key, "cash_ledger", last_sync_time, clinic_id)
            .await
            .context("pull cash_ledger")?;
        for c in cash {
            self.apply_remote_row_cash(&c)?;
        }

        let offers: Vec<Offer> = self
            .fetch_table(base, api_key, "offers", last_sync_time, clinic_id)
            .await
            .context("pull offers")?;
        for o in offers {
            self.apply_remote_row_offers(&o)?;
        }

        let offer_items: Vec<OfferItem> = self
            .fetch_table(base, api_key, "offer_items", last_sync_time, clinic_id)
            .await
            .context("pull offer_items")?;
        for it in offer_items {
            self.apply_remote_row_offer_items(&it)?;
        }

        let fjobs: Vec<FiscalJob> = self
            .fetch_table(base, api_key, "fiscal_jobs", last_sync_time, clinic_id)
            .await
            .context("pull fiscal_jobs")?;
        for r in fjobs {
            if let Some(local_ts) = self.db.row_updated_at("fiscal_jobs", &r.id)? {
                if newer_or_equal(&local_ts, &r.updated_at)? {
                    continue;
                }
            }
            self.db.sync_queue_drop_pending_for_row("fiscal_jobs", &r.id)?;
            self.db.apply_remote_fiscal_job(&r)?;
        }

        let prescriptions: Vec<Prescription> = self
            .fetch_table(base, api_key, "prescriptions", last_sync_time, clinic_id)
            .await
            .context("pull prescriptions")?;
        for r in prescriptions {
            if let Some(local_ts) = self.db.row_updated_at("prescriptions", &r.id)? {
                if newer_or_equal(&local_ts, &r.updated_at)? {
                    continue;
                }
            }
            self.db.sync_queue_drop_pending_for_row("prescriptions", &r.id)?;
            self.db.apply_remote_prescription(&r)?;
        }

        let suppliers: Vec<StockSupplier> = self
            .fetch_table(base, api_key, "stock_suppliers", last_sync_time, clinic_id)
            .await
            .context("pull stock_suppliers")?;
        for r in suppliers {
            if let Some(local_ts) = self.db.row_updated_at("stock_suppliers", &r.id)? {
                if newer_or_equal(&local_ts, &r.updated_at)? {
                    continue;
                }
            }
            self.db.sync_queue_drop_pending_for_row("stock_suppliers", &r.id)?;
            self.db.apply_remote_stock_supplier(&r)?;
        }

        let stock_items: Vec<StockItem> = self
            .fetch_table(base, api_key, "stock_items", last_sync_time, clinic_id)
            .await
            .context("pull stock_items")?;
        for r in stock_items {
            if let Some(local_ts) = self.db.row_updated_at("stock_items", &r.id)? {
                if newer_or_equal(&local_ts, &r.updated_at)? {
                    continue;
                }
            }
            self.db.sync_queue_drop_pending_for_row("stock_items", &r.id)?;
            self.db.apply_remote_stock_item(&r)?;
        }

        let movements: Vec<StockMovement> = self
            .fetch_table(base, api_key, "stock_movements", last_sync_time, clinic_id)
            .await
            .context("pull stock_movements")?;
        for r in movements {
            // Levizjet jane te pandryshueshme — ON CONFLICT DO NOTHING lokalisht.
            self.db.apply_remote_stock_movement(&r)?;
        }

        Ok(())
    }

    fn apply_remote_row_doctors(&self, remote: &Doctor) -> anyhow::Result<()> {
        let local = self.db.doctors_updated_at(&remote.id)?;
        if let Some(local_ts) = local {
            if newer_or_equal(&local_ts, &remote.updated_at)? {
                return Ok(());
            }
        }
        self.db
            .sync_queue_drop_pending_for_row("doctors", &remote.id)?;
        self.db.apply_remote_doctor(remote)?;
        Ok(())
    }

    fn apply_remote_row_doctor_accounts(&self, remote: &DoctorAccount) -> anyhow::Result<()> {
        let local = self.db.doctor_accounts_updated_at(&remote.doctor_id)?;
        if let Some(local_ts) = local {
            if newer_or_equal(&local_ts, &remote.updated_at)? {
                return Ok(());
            }
        }
        self.db.sync_queue_drop_pending_for_row("doctor_accounts", &remote.doctor_id)?;
        self.db.apply_remote_doctor_account(remote)?;
        Ok(())
    }

    fn apply_remote_row_services(&self, remote: &Service) -> anyhow::Result<()> {
        let local = self.db.services_updated_at(&remote.id)?;
        if let Some(local_ts) = local {
            if newer_or_equal(&local_ts, &remote.updated_at)? {
                return Ok(());
            }
        }
        self.db
            .sync_queue_drop_pending_for_row("services", &remote.id)?;
        self.db.apply_remote_service(remote)?;
        Ok(())
    }

    fn apply_remote_row_appointments(&self, remote: &Appointment) -> anyhow::Result<()> {
        let local = self.db.appointments_updated_at(&remote.id)?;
        if let Some(local_ts) = local {
            if newer_or_equal(&local_ts, &remote.updated_at)? {
                return Ok(());
            }
        }
        self.db
            .sync_queue_drop_pending_for_row("appointments", &remote.id)?;
        self.db.apply_remote_appointment(remote)?;
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
        self.db
            .sync_queue_drop_pending_for_row("clients", &remote.id)?;
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
        self.db
            .sync_queue_drop_pending_for_row("sales", &remote.id)?;
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
        self.db
            .sync_queue_drop_pending_for_row("payments", &remote.id)?;
        self.db.apply_remote_payment(remote)?;
        Ok(())
    }

    fn apply_remote_row_visits(&self, remote: &Visit) -> anyhow::Result<()> {
        let local = self.db.visits_updated_at(&remote.id)?;
        if let Some(local_ts) = local {
            if newer_or_equal(&local_ts, &remote.updated_at)? {
                return Ok(());
            }
        }
        self.db
            .sync_queue_drop_pending_for_row("visits", &remote.id)?;
        self.db.apply_remote_visit(remote)?;
        Ok(())
    }

    fn apply_remote_row_visit_items(&self, remote: &VisitItem) -> anyhow::Result<()> {
        let local = self.db.visit_items_updated_at(&remote.id)?;
        if let Some(local_ts) = local {
            if newer_or_equal(&local_ts, &remote.updated_at)? {
                return Ok(());
            }
        }
        self.db
            .sync_queue_drop_pending_for_row("visit_items", &remote.id)?;
        self.db.apply_remote_visit_item(remote)?;
        Ok(())
    }

    fn apply_remote_row_cash(&self, remote: &CashEntry) -> anyhow::Result<()> {
        let local = self.db.cash_updated_at(&remote.id)?;
        if let Some(local_ts) = local {
            if newer_or_equal(&local_ts, &remote.updated_at)? {
                return Ok(());
            }
        }
        self.db
            .sync_queue_drop_pending_for_row("cash_ledger", &remote.id)?;
        self.db.apply_remote_cash_entry(remote)?;
        Ok(())
    }

    fn apply_remote_row_offers(&self, remote: &Offer) -> anyhow::Result<()> {
        let local = self.db.offer_updated_at(&remote.id)?;
        if let Some(local_ts) = local {
            if newer_or_equal(&local_ts, &remote.updated_at)? {
                return Ok(());
            }
        }
        self.db.sync_queue_drop_pending_for_row("offers", &remote.id)?;
        self.db.apply_remote_offer(remote)?;
        Ok(())
    }

    fn apply_remote_row_offer_items(&self, remote: &OfferItem) -> anyhow::Result<()> {
        let local = self.db.offer_items_updated_at(&remote.id)?;
        if let Some(local_ts) = local {
            if newer_or_equal(&local_ts, &remote.updated_at)? {
                return Ok(());
            }
        }
        self.db.sync_queue_drop_pending_for_row("offer_items", &remote.id)?;
        self.db.apply_remote_offer_item(remote)?;
        Ok(())
    }

    async fn fetch_table<T: DeserializeOwned>(
        &self,
        base: &str,
        api_key: &str,
        table: &str,
        last_sync_time: &str,
        clinic_id: &str,
    ) -> anyhow::Result<Vec<T>> {
        let url = format!("{base}/rest/v1/{table}");
        let resp = with_supabase_auth(self.client.get(&url), api_key)
            .header("clinic_id", clinic_id)
            .query(&[
                ("select", "*"),
                ("clinic_id", &format!("eq.{}", clinic_id)),
                ("updated_at", &format!("gt.{last_sync_time}")),
                ("order", "updated_at.asc"),
                ("limit", "10000"),
            ])
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            let msg = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(|s| s.to_string()))
                .unwrap_or_else(|| body.clone());
            let err_msg = format!("supabase pull failed {table}: {status} {msg}");
            append_sync_error(
                self.db.as_ref(),
                "sync_fetch_table",
                &err_msg,
            );
            bail!("{err_msg}");
        }
        let rows: Vec<T> = serde_json::from_str(&body).context("decode json")?;
        Ok(rows)
    }

    async fn push_queue(&self, supabase_url: &str, api_key: &str, clinic_id: &str) -> anyhow::Result<()> {
        let items = self.db.sync_queue_list_pending(200)?;
        if items.is_empty() {
            return Ok(());
        }

        let base = supabase_url.trim_end_matches('/');
        let mut upserts: HashMap<String, Vec<(String, Value)>> = HashMap::new();
        let mut deletes: Vec<(SyncQueueItem, Value)> = Vec::new();

        for it in items {
            let mut v: Value = serde_json::from_str(&it.payload).context("parse sync payload")?;
            if it.table_name == "visit_items" && it.op != "delete" {
                // Legacy safety: prefer the current local row over stale queue payloads.
                // Older payloads could carry null vat_code/fiscalized which Supabase rejects.
                let local_id = v
                    .get("id")
                    .and_then(|x| x.as_str())
                    .map(str::trim)
                    .filter(|x| !x.is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| it.row_id.clone());
                if !local_id.trim().is_empty() {
                    if let Ok(Some(local_row)) = self.db.visit_items_get(&local_id) {
                        if let Ok(local_value) = serde_json::to_value(local_row) {
                            v = local_value;
                        }
                    }
                }
            }

            // Inject clinic_id into the payload for all upserts.
            if let Some(obj) = v.as_object_mut() {
                obj.insert("clinic_id".to_string(), Value::String(clinic_id.to_string()));
            }

            normalize_sync_row_for_table(&it.table_name, &mut v);
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
                let mut payload: Vec<Value> = batch.iter().map(|(_, v)| v.clone()).collect();
                normalize_object_batch_keys(&mut payload);
                normalize_table_batch_values(&table, &mut payload);
                
                let conflict_col = if table == "doctor_accounts" { "doctor_id" } else { "id" };
                let url = format!("{base}/rest/v1/{table}?on_conflict={conflict_col}");
                
                let resp = with_supabase_auth(self.client.post(&url), api_key)
                    .header("Prefer", "resolution=merge-duplicates,return=minimal")
                    .header("clinic_id", clinic_id)
                    .json(&payload)
                    .send()
                    .await?;
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                if !status.is_success() {
                    let json_msg = serde_json::from_str::<Value>(&body)
                        .ok()
                        .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(|s| s.to_string()))
                        .unwrap_or_else(|| body.clone());
                    let msg = format!("supabase upsert failed {table}: {status} {json_msg}");
                    append_sync_error(self.db.as_ref(), "sync_upsert", &msg);
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
            let pk_col = if it.table_name == "doctor_accounts" { "doctor_id" } else { "id" };
            let id = v
                .get(pk_col)
                .and_then(|x| x.as_str())
                .ok_or_else(|| anyhow!("delete payload missing pk"))?
                .to_string();
            let updated_at = v
                .get("updated_at")
                .and_then(|x| x.as_str())
                .ok_or_else(|| anyhow!("delete payload missing updated_at"))?
                .to_string();

            let url = format!(
                "{base}/rest/v1/{}?{}=eq.{}",
                it.table_name,
                pk_col,
                urlencoding::encode(&id)
            );
            let body = serde_json::json!({ "deleted": 1, "updated_at": updated_at });
            let resp = with_supabase_auth(self.client.patch(&url), api_key)
                .header("Prefer", "return=minimal")
                .header("clinic_id", clinic_id)
                .json(&body)
                .send()
                .await?;
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            if !status.is_success() {
                let msg = format!("supabase delete failed {}: {status} {text}", it.table_name);
                append_sync_error(self.db.as_ref(), "sync_delete", &msg);
                self.db.sync_queue_mark_failed(&it.id, &msg)?;
                bail!("{msg}");
            }
            self.db.sync_queue_mark_sent(&it.id)?;
        }

        Ok(())
    }

    async fn sync_clinic_settings(&self, supabase_url: &str, api_key: &str, clinic_id: &str) {
        let local_bank = self
            .db
            .setting_get("clinic_bank_account")
            .unwrap_or(None)
            .unwrap_or_default();

        let base = supabase_url.trim_end_matches('/');

        // Push local value to clinic_registry if set.
        if !local_bank.trim().is_empty() {
            let url = format!("{base}/rest/v1/clinic_registry?on_conflict=clinic_id");
            let body = serde_json::json!([{ "clinic_id": clinic_id, "bank_account": local_bank.trim() }]);
            let _ = with_supabase_auth(self.client.post(&url), api_key)
                .header("Prefer", "resolution=merge-duplicates,return=minimal")
                .header("clinic_id", clinic_id)
                .json(&body)
                .send()
                .await;
        }

        // Pull remote value; update local if remote differs and local is empty.
        let url = format!("{base}/rest/v1/clinic_registry?clinic_id=eq.{clinic_id}&select=bank_account");
        if let Ok(resp) = with_supabase_auth(self.client.get(&url), api_key)
            .header("clinic_id", clinic_id)
            .send()
            .await
        {
            if resp.status().is_success() {
                if let Ok(rows) = resp.json::<Vec<serde_json::Value>>().await {
                    if let Some(remote_ba) = rows
                        .first()
                        .and_then(|r| r.get("bank_account"))
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                    {
                        if local_bank.trim().is_empty() {
                            let _ = self.db.setting_set("clinic_bank_account", remote_ba);
                        }
                    }
                }
            }
        }
    }
}

fn normalize_object_batch_keys(payload: &mut [Value]) {
    let mut key_union: BTreeSet<String> = BTreeSet::new();
    let mut all_objects = true;

    for row in payload.iter() {
        match row.as_object() {
            Some(obj) => {
                for k in obj.keys() {
                    key_union.insert(k.clone());
                }
            }
            None => {
                all_objects = false;
                break;
            }
        }
    }

    if !all_objects || key_union.is_empty() {
        return;
    }

    for row in payload.iter_mut() {
        if let Some(obj) = row.as_object_mut() {
            for k in &key_union {
                if !obj.contains_key(k) {
                    obj.insert(k.clone(), Value::Null);
                }
            }
        }
    }
}

fn normalize_sync_row_for_table(table: &str, row: &mut Value) {
    if let Some(obj) = row.as_object_mut() {
        match table {
            "visit_items" => {
                normalize_string_key(obj, "vat_code", "C", true);
                normalize_i64_key(obj, "fiscalized", 0);
                normalize_i64_key(obj, "fiscal", 1);
            }
            "services" => {
                normalize_string_key(obj, "vat_code", "C", true);
            }
            "sales" => {
                normalize_i64_key(obj, "fiscalized", 0);
            }
            _ => {}
        }
    }
}

fn normalize_table_batch_values(table: &str, payload: &mut [Value]) {
    for row in payload.iter_mut() {
        normalize_sync_row_for_table(table, row);
    }
}

fn normalize_string_key(
    obj: &mut Map<String, Value>,
    key: &str,
    default_value: &str,
    uppercase: bool,
) {
    let mut out = obj
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or("")
        .to_string();

    if out.is_empty() {
        out = default_value.to_string();
    }
    if uppercase {
        out = out.to_uppercase();
    }
    obj.insert(key.to_string(), Value::String(out));
}

fn normalize_i64_key(obj: &mut Map<String, Value>, key: &str, default_value: i64) {
    let parsed = match obj.get(key) {
        Some(v) => {
            if let Some(n) = v.as_i64() {
                Some(n)
            } else if let Some(b) = v.as_bool() {
                Some(if b { 1 } else { 0 })
            } else if let Some(s) = v.as_str() {
                s.trim().parse::<i64>().ok()
            } else {
                None
            }
        }
        None => None,
    };

    obj.insert(
        key.to_string(),
        Value::from(parsed.unwrap_or(default_value)),
    );
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
