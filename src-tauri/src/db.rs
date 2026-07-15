use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use uuid::Uuid;

use crate::models::{
    ClientPhoto,
    Appointment, AppointmentUpsertInput, AppointmentsListFilters, CashEntry, CashEntryUpsertInput,
    CashListFilters, Client, ClientUpsertInput, Doctor, DoctorUpsertInput, Payment,
    PaymentUpsertInput, PaymentsListFilters, Sale, SaleUpsertInput, SalesListFilters, Service,
    ServiceUpsertInput, SyncQueueItem, Visit, VisitItem, VisitItemUpsertInput, DoctorAccount,
    VisitItemsListFilters, VisitUpsertInput, VisitsListFilters,
};
use crate::util::now_iso;

const MIGRATION_SQL: &str = r#"
PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;

CREATE TABLE IF NOT EXISTS clients (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  phone TEXT,
  email TEXT,
  notes TEXT,
  first_name TEXT,
  last_name TEXT,
  parent_name TEXT,
  dob TEXT,
  gender TEXT,
  city TEXT,
  address TEXT,
  allergies TEXT,
  weight_kg REAL,
  height_cm REAL,
  patient_code TEXT,
  created_at TEXT,
  updated_at TEXT,
  deleted INTEGER DEFAULT 0
);

CREATE TABLE IF NOT EXISTS sales (
  id TEXT PRIMARY KEY,
  client_id TEXT NOT NULL,
  date TEXT,
  total REAL NOT NULL,
  notes TEXT,
  fiscalized INTEGER NOT NULL DEFAULT 0,
  fiscalized_at TEXT,
  created_at TEXT,
  updated_at TEXT,
  deleted INTEGER DEFAULT 0
);

CREATE TABLE IF NOT EXISTS payments (
  id TEXT PRIMARY KEY,
  client_id TEXT NOT NULL,
  sale_id TEXT,
  date TEXT,
  amount REAL NOT NULL,
  method TEXT,
  notes TEXT,
  created_at TEXT,
  updated_at TEXT,
  deleted INTEGER DEFAULT 0
);

CREATE TABLE IF NOT EXISTS sync_queue (
  id TEXT PRIMARY KEY,
  table_name TEXT NOT NULL,
  row_id TEXT NOT NULL,
  op TEXT NOT NULL,
  payload TEXT NOT NULL,
  created_at TEXT,
  status TEXT NOT NULL DEFAULT 'pending',
  last_error TEXT
);

CREATE TABLE IF NOT EXISTS app_settings (
  key TEXT PRIMARY KEY,
  value TEXT
);

CREATE TABLE IF NOT EXISTS doctors (
  id TEXT PRIMARY KEY,
  code TEXT,
  name TEXT NOT NULL,
  title TEXT,
  specialty TEXT,
  phone TEXT,
  email TEXT,
  notes TEXT,
  created_at TEXT,
  updated_at TEXT,
  deleted INTEGER DEFAULT 0
);

-- Local-only credentials for doctor logins (NOT synced to Supabase).
CREATE TABLE IF NOT EXISTS doctor_accounts (
  doctor_id TEXT PRIMARY KEY,
  salt TEXT NOT NULL,
  password_hash TEXT NOT NULL,
  is_admin INTEGER NOT NULL DEFAULT 0,
  created_at TEXT,
  updated_at TEXT
);

CREATE TABLE IF NOT EXISTS services (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  default_price REAL NOT NULL,
  vat_code TEXT NOT NULL DEFAULT 'C',
  notes TEXT,
  created_at TEXT,
  updated_at TEXT,
  deleted INTEGER DEFAULT 0
);

CREATE TABLE IF NOT EXISTS appointments (
  id TEXT PRIMARY KEY,
  client_id TEXT NOT NULL,
  doctor_id TEXT,
  start_at TEXT NOT NULL,
  end_at TEXT,
  status TEXT NOT NULL,
  notes TEXT,
  created_at TEXT,
  updated_at TEXT,
  deleted INTEGER DEFAULT 0
);

CREATE TABLE IF NOT EXISTS visits (
  id TEXT PRIMARY KEY,
  client_id TEXT NOT NULL,
  doctor_id TEXT,
  date TEXT,
  visit_time TEXT,
  status TEXT NOT NULL,
  notes TEXT,
  body_weight TEXT,
  body_weight_unit TEXT,
  body_height TEXT,
  body_height_unit TEXT,
  head_circumference TEXT,
  head_circumference_unit TEXT,
  body_temperature TEXT,
  body_temperature_unit TEXT,
  blood_oxygen TEXT,
  blood_oxygen_unit TEXT,
  glycemia TEXT,
  glycemia_unit TEXT,
  pulse TEXT,
  pulse_unit TEXT,
  bmi TEXT,
  blood_pressure_systolic TEXT,
  blood_pressure_diastolic TEXT,
  blood_pressure_unit TEXT,
  complaints TEXT,
  additional_notes TEXT,
  controls TEXT,
  remarks TEXT,
  analyses TEXT,
  advice TEXT,
  therapies TEXT,
  diagnosis TEXT,
  examinations TEXT,
  created_at TEXT,
  updated_at TEXT,
  deleted INTEGER DEFAULT 0
);

CREATE TABLE IF NOT EXISTS visit_items (
  id TEXT PRIMARY KEY,
  visit_id TEXT NOT NULL,
  client_id TEXT NOT NULL,
  tooth TEXT,
  title TEXT NOT NULL,
  qty REAL NOT NULL,
  unit_price REAL NOT NULL,
  fiscal INTEGER NOT NULL DEFAULT 1,
  vat_code TEXT NOT NULL DEFAULT 'C',
  fiscalized INTEGER NOT NULL DEFAULT 0,
  fiscalized_at TEXT,
  notes TEXT,
  created_at TEXT,
  updated_at TEXT,
  deleted INTEGER DEFAULT 0
);

CREATE TABLE IF NOT EXISTS cash_ledger (
  id TEXT PRIMARY KEY,
  type TEXT NOT NULL,
  date TEXT,
  amount REAL NOT NULL,
  category TEXT,
  notes TEXT,
  created_at TEXT,
  updated_at TEXT,
  deleted INTEGER DEFAULT 0
);

CREATE TABLE IF NOT EXISTS regular_invoices (
  id TEXT PRIMARY KEY,
  sale_id TEXT NOT NULL,
  invoice_number TEXT,
  client_id TEXT,
  client_name TEXT,
  date TEXT,
  total REAL NOT NULL DEFAULT 0,
  pdf_filename TEXT,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS stock_suppliers (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  phone TEXT,
  email TEXT,
  address TEXT,
  notes TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted INTEGER DEFAULT 0
);

CREATE TABLE IF NOT EXISTS stock_items (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  unit TEXT,
  category TEXT,
  supplier_id TEXT,
  min_quantity REAL NOT NULL DEFAULT 0,
  notes TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted INTEGER DEFAULT 0
);

CREATE TABLE IF NOT EXISTS stock_movements (
  id TEXT PRIMARY KEY,
  item_id TEXT NOT NULL,
  movement_type TEXT NOT NULL,
  quantity REAL NOT NULL,
  price_per_unit REAL,
  notes TEXT,
  date TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sale_items (
  id TEXT PRIMARY KEY,
  sale_id TEXT NOT NULL,
  item_type TEXT NOT NULL,
  ref_id TEXT,
  title TEXT NOT NULL,
  qty REAL NOT NULL,
  unit_price REAL NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS offers (
  id TEXT PRIMARY KEY,
  clinic_id TEXT NOT NULL DEFAULT '',
  client_id TEXT NOT NULL DEFAULT '',
  offer_number TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'draft',
  valid_until TEXT,
  notes TEXT,
  vat_pct REAL NOT NULL DEFAULT 18,
  subtotal REAL NOT NULL DEFAULT 0,
  vat_amount REAL NOT NULL DEFAULT 0,
  total REAL NOT NULL DEFAULT 0,
  invoice_id TEXT,
  source_offer_id TEXT,
  created_at TEXT NOT NULL DEFAULT '',
  updated_at TEXT NOT NULL DEFAULT '',
  deleted_at TEXT
);

CREATE TABLE IF NOT EXISTS fiscal_jobs (
  id TEXT PRIMARY KEY,
  sale_id TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pending',
  requested_by TEXT NOT NULL DEFAULT '',
  error TEXT,
  processed_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS prescriptions (
  id TEXT PRIMARY KEY,
  visit_id TEXT,
  client_id TEXT,
  doctor_id TEXT,
  kind TEXT NOT NULL DEFAULT 'recete',
  title TEXT NOT NULL DEFAULT '',
  content TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS client_photos (
  id TEXT PRIMARY KEY,
  client_id TEXT NOT NULL,
  stage TEXT NOT NULL DEFAULT 'before',
  label TEXT NOT NULL DEFAULT '',
  file_path TEXT NOT NULL,
  taken_at TEXT,
  created_at TEXT NOT NULL,
  deleted INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS offer_items (
  id TEXT PRIMARY KEY,
  offer_id TEXT NOT NULL DEFAULT '',
  clinic_id TEXT NOT NULL DEFAULT '',
  description TEXT NOT NULL DEFAULT '',
  qty REAL NOT NULL DEFAULT 1,
  unit_price REAL NOT NULL DEFAULT 0,
  discount_pct REAL NOT NULL DEFAULT 0,
  line_total REAL NOT NULL DEFAULT 0,
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT '',
  updated_at TEXT NOT NULL DEFAULT '',
  deleted_at TEXT
);

-- Rezultate te ardhura nga analizuesi laboratorik i lidhur me pajisjen (RS232/ASTM).
-- Vetem lokale (jo e sinkronizuar) - eshte specifike per hardware-in e lidhur
-- ne kete PC. Kur nje rezultat perputhet/caktohet ne nje vizite, teksti i
-- formatuar shtohet te visits.analyses (ai fushe sinkronizohet normalisht).
CREATE TABLE IF NOT EXISTS lab_inbox (
  id TEXT PRIMARY KEY,
  profile_id TEXT NOT NULL DEFAULT '',
  patient_ref_raw TEXT NOT NULL DEFAULT '',
  formatted_text TEXT NOT NULL DEFAULT '',
  raw_message TEXT NOT NULL DEFAULT '',
  matched_client_id TEXT,
  matched_visit_id TEXT,
  status TEXT NOT NULL DEFAULT 'unmatched',
  received_at TEXT NOT NULL DEFAULT ''
);
"#;

pub struct Db {
    db_path: PathBuf,
    conn: Mutex<Connection>,
}

impl Db {
    pub fn new(db_path: PathBuf) -> anyhow::Result<Self> {
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create db dir: {}", parent.display()))?;
        }

        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_FULL_MUTEX;
        let conn = Connection::open_with_flags(&db_path, flags)
            .with_context(|| format!("open sqlite: {}", db_path.display()))?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute_batch(MIGRATION_SQL)?;
        Self::run_migrations(&conn)?;

        Ok(Self {
            db_path,
            conn: Mutex::new(conn),
        })
    }

    /// Baze SQLite vetem ne memorie - perdoret nga "Demo Mode" (mobile-only):
    /// e njejta skeme/logjike si Db::new, por asgje s'shkruhet ne disk dhe
    /// gjithcka zhduket kur mbyllet procesi. `db_path()` mbetet bosh - askush
    /// s'duhet ta perdore per demo mode (sync/backup logic e injoron).
    pub fn new_in_memory() -> anyhow::Result<Self> {
        let conn = Connection::open_in_memory().context("open in-memory sqlite")?;
        conn.execute_batch(MIGRATION_SQL)?;
        Self::run_migrations(&conn)?;

        Ok(Self {
            db_path: PathBuf::new(),
            conn: Mutex::new(conn),
        })
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    fn conn(&self) -> anyhow::Result<std::sync::MutexGuard<'_, Connection>> {
        self.conn.lock().map_err(|_| anyhow!("db mutex poisoned"))
    }

    fn has_column(conn: &Connection, table: &str, column: &str) -> anyhow::Result<bool> {
        let n: i64 = conn.query_row(
            "SELECT COUNT(1) FROM pragma_table_info(?1) WHERE name=?2",
            params![table, column],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    fn add_column_if_missing(
        conn: &Connection,
        table: &str,
        column: &str,
        ddl: &str,
    ) -> anyhow::Result<()> {
        if Self::has_column(conn, table, column)? {
            return Ok(());
        }
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {ddl}"),
            [],
        )?;
        Ok(())
    }

    fn run_migrations(conn: &Connection) -> anyhow::Result<()> {
        // Additive migrations for older local DBs. Keep these idempotent.
        // clients
        for (col, ddl) in [
            ("first_name", "TEXT"),
            ("last_name", "TEXT"),
            ("parent_name", "TEXT"),
            ("dob", "TEXT"),
            ("gender", "TEXT"),
            ("city", "TEXT"),
            ("address", "TEXT"),
            ("allergies", "TEXT"),
            ("weight_kg", "REAL"),
            ("height_cm", "REAL"),
            ("patient_code", "TEXT"),
        ] {
            Self::add_column_if_missing(conn, "clients", col, ddl)?;
        }

        // doctors
        for (col, ddl) in [("code", "TEXT"), ("title", "TEXT"), ("specialty", "TEXT")] {
            Self::add_column_if_missing(conn, "doctors", col, ddl)?;
        }

        // doctor_accounts (make syncable)
        for (col, ddl) in [("clinic_id", "TEXT"), ("deleted", "INTEGER DEFAULT 0")] {
            Self::add_column_if_missing(conn, "doctor_accounts", col, ddl)?;
        }

        // services
        Self::add_column_if_missing(conn, "services", "vat_code", "TEXT NOT NULL DEFAULT 'C'")?;
        conn.execute(
            "UPDATE services SET vat_code='C' WHERE vat_code IS NULL OR TRIM(vat_code)=''",
            [],
        )?;

        // sales
        Self::add_column_if_missing(conn, "sales", "fiscalized", "INTEGER NOT NULL DEFAULT 0")?;
        Self::add_column_if_missing(conn, "sales", "fiscalized_at", "TEXT")?;
        conn.execute("UPDATE sales SET fiscalized=0 WHERE fiscalized IS NULL", [])?;

        // visits
        for (col, ddl) in [
            ("visit_time", "TEXT"),
            ("body_weight", "TEXT"),
            ("body_weight_unit", "TEXT"),
            ("body_height", "TEXT"),
            ("body_height_unit", "TEXT"),
            ("head_circumference", "TEXT"),
            ("head_circumference_unit", "TEXT"),
            ("body_temperature", "TEXT"),
            ("body_temperature_unit", "TEXT"),
            ("blood_oxygen", "TEXT"),
            ("blood_oxygen_unit", "TEXT"),
            ("glycemia", "TEXT"),
            ("glycemia_unit", "TEXT"),
            ("pulse", "TEXT"),
            ("pulse_unit", "TEXT"),
            ("bmi", "TEXT"),
            ("blood_pressure_systolic", "TEXT"),
            ("blood_pressure_diastolic", "TEXT"),
            ("blood_pressure_unit", "TEXT"),
            ("complaints", "TEXT"),
            ("additional_notes", "TEXT"),
            ("controls", "TEXT"),
            ("remarks", "TEXT"),
            ("analyses", "TEXT"),
            ("advice", "TEXT"),
            ("therapies", "TEXT"),
            ("diagnosis", "TEXT"),
            ("examinations", "TEXT"),
            ("specialty_report", "TEXT"),
        ] {
            Self::add_column_if_missing(conn, "visits", col, ddl)?;
        }

        // stock_items
        Self::add_column_if_missing(conn, "stock_items", "sale_price", "REAL NOT NULL DEFAULT 0")?;

        // visit_items
        Self::add_column_if_missing(conn, "visit_items", "vat_code", "TEXT NOT NULL DEFAULT 'C'")?;
        Self::add_column_if_missing(
            conn,
            "visit_items",
            "fiscalized",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        Self::add_column_if_missing(conn, "visit_items", "fiscalized_at", "TEXT")?;
        conn.execute(
            "UPDATE visit_items SET vat_code='C' WHERE vat_code IS NULL OR TRIM(vat_code)=''",
            [],
        )?;
        conn.execute(
            "UPDATE visit_items SET fiscalized=0 WHERE fiscalized IS NULL",
            [],
        )?;
        conn.execute("UPDATE visit_items SET fiscal=1 WHERE fiscal IS NULL", [])?;

        // Ensure new indexes exist.
        // Note: indexes are safe to re-run with IF NOT EXISTS.
        conn.execute_batch(
      r#"
      CREATE INDEX IF NOT EXISTS idx_clients_updated_at ON clients(updated_at);
      CREATE INDEX IF NOT EXISTS idx_clients_patient_code ON clients(patient_code);

      CREATE INDEX IF NOT EXISTS idx_sales_client_id ON sales(client_id);
      CREATE INDEX IF NOT EXISTS idx_sales_date ON sales(date);
      CREATE INDEX IF NOT EXISTS idx_sales_updated_at ON sales(updated_at);
      CREATE INDEX IF NOT EXISTS idx_sales_fiscalized ON sales(fiscalized);

      CREATE INDEX IF NOT EXISTS idx_payments_client_id ON payments(client_id);
      CREATE INDEX IF NOT EXISTS idx_payments_sale_id ON payments(sale_id);
      CREATE INDEX IF NOT EXISTS idx_payments_date ON payments(date);
      CREATE INDEX IF NOT EXISTS idx_payments_updated_at ON payments(updated_at);

      CREATE INDEX IF NOT EXISTS idx_sync_queue_status ON sync_queue(status);
      CREATE INDEX IF NOT EXISTS idx_sync_queue_created_at ON sync_queue(created_at);
      CREATE INDEX IF NOT EXISTS idx_sync_queue_table_row ON sync_queue(table_name, row_id);

      CREATE INDEX IF NOT EXISTS idx_doctors_updated_at ON doctors(updated_at);
      CREATE INDEX IF NOT EXISTS idx_doctors_code ON doctors(code);
      CREATE UNIQUE INDEX IF NOT EXISTS idx_doctors_code_unique ON doctors(code) WHERE code IS NOT NULL AND code <> '' AND deleted = 0;

      CREATE INDEX IF NOT EXISTS idx_doctor_accounts_is_admin ON doctor_accounts(is_admin);
      CREATE INDEX IF NOT EXISTS idx_doctor_accounts_updated_at ON doctor_accounts(updated_at);

      CREATE INDEX IF NOT EXISTS idx_services_updated_at ON services(updated_at);
      CREATE INDEX IF NOT EXISTS idx_services_vat_code ON services(vat_code);

      CREATE INDEX IF NOT EXISTS idx_appointments_client_id ON appointments(client_id);
      CREATE INDEX IF NOT EXISTS idx_appointments_doctor_id ON appointments(doctor_id);
      CREATE INDEX IF NOT EXISTS idx_appointments_start_at ON appointments(start_at);
      CREATE INDEX IF NOT EXISTS idx_appointments_status ON appointments(status);
      CREATE INDEX IF NOT EXISTS idx_appointments_updated_at ON appointments(updated_at);

      CREATE INDEX IF NOT EXISTS idx_visits_client_id ON visits(client_id);
      CREATE INDEX IF NOT EXISTS idx_visits_doctor_id ON visits(doctor_id);
      CREATE INDEX IF NOT EXISTS idx_visits_date ON visits(date);
      CREATE INDEX IF NOT EXISTS idx_visits_status ON visits(status);
      CREATE INDEX IF NOT EXISTS idx_visits_updated_at ON visits(updated_at);

      CREATE INDEX IF NOT EXISTS idx_visit_items_visit_id ON visit_items(visit_id);
      CREATE INDEX IF NOT EXISTS idx_visit_items_client_id ON visit_items(client_id);
      CREATE INDEX IF NOT EXISTS idx_visit_items_updated_at ON visit_items(updated_at);
      CREATE INDEX IF NOT EXISTS idx_visit_items_fiscalized ON visit_items(fiscalized);

      CREATE INDEX IF NOT EXISTS idx_cash_ledger_type ON cash_ledger(type);
      CREATE INDEX IF NOT EXISTS idx_cash_ledger_date ON cash_ledger(date);
      CREATE INDEX IF NOT EXISTS idx_cash_ledger_updated_at ON cash_ledger(updated_at);

      CREATE INDEX IF NOT EXISTS idx_sale_items_sale_id ON sale_items(sale_id);

      CREATE INDEX IF NOT EXISTS idx_offers_clinic_id ON offers(clinic_id);
      CREATE INDEX IF NOT EXISTS idx_offers_client_id ON offers(client_id);
      CREATE INDEX IF NOT EXISTS idx_offers_updated_at ON offers(updated_at);
      CREATE INDEX IF NOT EXISTS idx_offers_status ON offers(status);

      CREATE INDEX IF NOT EXISTS idx_offer_items_offer_id ON offer_items(offer_id);
      CREATE INDEX IF NOT EXISTS idx_offer_items_clinic_id ON offer_items(clinic_id);
      CREATE INDEX IF NOT EXISTS idx_offer_items_updated_at ON offer_items(updated_at);
      "#,
    )?;

        // Legacy recovery: older builds may have queued visit_items payloads with missing/null vat_code.
        Self::normalize_legacy_sync_payloads(conn)?;

        Ok(())
    }

    fn normalize_legacy_sync_payloads(conn: &Connection) -> anyhow::Result<()> {
        let mut stmt = conn.prepare(
            "SELECT id, payload
       FROM sync_queue
       WHERE table_name='visit_items' AND status IN ('pending','failed')",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        for row in rows {
            let (id, payload) = row?;
            let mut v: serde_json::Value = match serde_json::from_str(&payload) {
                Ok(x) => x,
                Err(_) => continue,
            };
            let mut changed = false;
            if let Some(obj) = v.as_object_mut() {
                let vat = obj
                    .get("vat_code")
                    .and_then(|x| x.as_str())
                    .map(str::trim)
                    .unwrap_or("");
                if vat.is_empty() {
                    obj.insert(
                        "vat_code".to_string(),
                        serde_json::Value::String("C".to_string()),
                    );
                    changed = true;
                }

                let fiscalized = obj.get("fiscalized").and_then(|x| x.as_i64());
                if fiscalized.is_none() {
                    obj.insert("fiscalized".to_string(), serde_json::Value::from(0_i64));
                    changed = true;
                }

                let fiscal = obj.get("fiscal").and_then(|x| x.as_i64());
                if fiscal.is_none() {
                    obj.insert("fiscal".to_string(), serde_json::Value::from(1_i64));
                    changed = true;
                }
            }

            if changed {
                let next_payload = serde_json::to_string(&v)?;
                conn.execute(
                    "UPDATE sync_queue SET payload=?2 WHERE id=?1",
                    params![id, next_payload],
                )?;
            }
        }
        Ok(())
    }

    pub fn settings_get_all(&self) -> anyhow::Result<HashMap<String, String>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT key, value FROM app_settings ORDER BY key ASC")?;
        let mut rows = stmt.query([])?;
        let mut out = HashMap::new();
        while let Some(row) = rows.next()? {
            let k: String = row.get(0)?;
            let v: Option<String> = row.get(1)?;
            if let Some(v) = v {
                out.insert(k, v);
            }
        }
        Ok(out)
    }

    pub fn setting_get(&self, key: &str) -> anyhow::Result<Option<String>> {
        let conn = self.conn()?;
        Ok(conn
            .query_row(
                "SELECT value FROM app_settings WHERE key = ?1",
                params![key],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten())
    }

    pub fn setting_set(&self, key: &str, value: &str) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
       ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn sync_queue_count_pending(&self) -> anyhow::Result<i64> {
        let conn = self.conn()?;
        let n: i64 = conn.query_row(
            "SELECT COUNT(1) FROM sync_queue WHERE status = 'pending'",
            [],
            |row| row.get(0),
        )?;
        Ok(n)
    }

    pub fn sync_queue_list_pending(&self, limit: usize) -> anyhow::Result<Vec<SyncQueueItem>> {
        let conn = self.conn()?;
        // Defensive: legacy queue payloads may still contain null/missing visit_items fields.
        // Normalize before each read so sync always sends valid rows.
        Self::normalize_legacy_sync_payloads(&conn)?;
        let mut stmt = conn.prepare(
            "SELECT id, table_name, row_id, op, payload, created_at, status, last_error
       FROM sync_queue
       WHERE status IN ('pending','failed')
       ORDER BY created_at ASC
       LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok(SyncQueueItem {
                id: row.get(0)?,
                table_name: row.get(1)?,
                row_id: row.get(2)?,
                op: row.get(3)?,
                payload: row.get(4)?,
                created_at: row.get(5)?,
                status: row.get(6)?,
                last_error: row.get(7)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn sync_queue_mark_sent(&self, id: &str) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE sync_queue SET status='sent', last_error=NULL WHERE id=?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn sync_queue_mark_failed(&self, id: &str, err: &str) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE sync_queue SET status='failed', last_error=?2 WHERE id=?1",
            params![id, err],
        )?;
        Ok(())
    }

    pub fn row_updated_at(&self, table: &str, id: &str) -> anyhow::Result<Option<String>> {
        // Only whitelisted tables — table name is never user input.
        let sql = match table {
            "prescriptions" => "SELECT updated_at FROM prescriptions WHERE id=?1",
            "fiscal_jobs" => "SELECT updated_at FROM fiscal_jobs WHERE id=?1",
            "stock_suppliers" => "SELECT updated_at FROM stock_suppliers WHERE id=?1",
            "stock_items" => "SELECT updated_at FROM stock_items WHERE id=?1",
            _ => return Ok(None),
        };
        let conn = self.conn()?;
        let v: Option<String> = conn.query_row(sql, params![id], |r| r.get(0)).optional()?;
        Ok(v)
    }

    pub fn sync_queue_drop_pending_for_row(&self, table: &str, row_id: &str) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
      "DELETE FROM sync_queue WHERE table_name=?1 AND row_id=?2 AND status IN ('pending','failed')",
      params![table, row_id],
    )?;
        Ok(())
    }

    fn queue_replace_pending_conn(
        conn: &Connection,
        table: &str,
        row_id: &str,
        op: &str,
        payload_json: &str,
        created_at: &str,
    ) -> anyhow::Result<()> {
        conn.execute(
            "DELETE FROM sync_queue WHERE table_name=?1 AND row_id=?2 AND status IN ('pending','failed')",
            params![table, row_id],
        )?;
        conn.execute(
            "INSERT INTO sync_queue (id, table_name, row_id, op, payload, created_at, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending')",
            params![Uuid::new_v4().to_string(), table, row_id, op, payload_json, created_at],
        )?;
        Ok(())
    }

    fn queue_replace_pending_tx(
        tx: &rusqlite::Transaction<'_>,
        table: &str,
        row_id: &str,
        op: &str,
        payload_json: &str,
        created_at: &str,
    ) -> anyhow::Result<()> {
        tx.execute(
      "DELETE FROM sync_queue WHERE table_name=?1 AND row_id=?2 AND status IN ('pending','failed')",
      params![table, row_id],
    )?;
        tx.execute(
            "INSERT INTO sync_queue (id, table_name, row_id, op, payload, created_at, status)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending')",
            params![
                Uuid::new_v4().to_string(),
                table,
                row_id,
                op,
                payload_json,
                created_at
            ],
        )?;
        Ok(())
    }

    pub fn clients_list(&self, search: Option<String>) -> anyhow::Result<Vec<Client>> {
        let conn = self.conn()?;
        let mut out = Vec::new();

        if let Some(s) = search.filter(|x| !x.trim().is_empty()) {
            let like = format!("%{}%", s.trim());
            let mut stmt = conn.prepare(
        "SELECT id, name, phone, email, notes,
                first_name, last_name, parent_name, dob, gender, city, address, allergies, weight_kg, height_cm, patient_code,
                created_at, updated_at, deleted
         FROM clients
         WHERE deleted = 0 AND (
           name LIKE ?1 OR phone LIKE ?1 OR email LIKE ?1 OR COALESCE(patient_code,'') LIKE ?1
         )
         ORDER BY updated_at DESC
         LIMIT 1000",
      )?;
            let rows = stmt.query_map(params![like], |row| {
                Ok(Client {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    phone: row.get(2)?,
                    email: row.get(3)?,
                    notes: row.get(4)?,
                    first_name: row.get(5)?,
                    last_name: row.get(6)?,
                    parent_name: row.get(7)?,
                    dob: row.get(8)?,
                    gender: row.get(9)?,
                    city: row.get(10)?,
                    address: row.get(11)?,
                    allergies: row.get(12)?,
                    weight_kg: row.get(13)?,
                    height_cm: row.get(14)?,
                    patient_code: row.get(15)?,
                    created_at: row.get(16)?,
                    updated_at: row.get(17)?,
                    deleted: row.get(18)?,
                })
            })?;
            for r in rows {
                out.push(r?);
            }
            return Ok(out);
        }

        let mut stmt = conn.prepare(
      "SELECT id, name, phone, email, notes,
              first_name, last_name, parent_name, dob, gender, city, address, allergies, weight_kg, height_cm, patient_code,
              created_at, updated_at, deleted
       FROM clients
       WHERE deleted = 0
       ORDER BY updated_at DESC
       LIMIT 1000",
    )?;
        let rows = stmt.query_map([], |row| {
            Ok(Client {
                id: row.get(0)?,
                name: row.get(1)?,
                phone: row.get(2)?,
                email: row.get(3)?,
                notes: row.get(4)?,
                first_name: row.get(5)?,
                last_name: row.get(6)?,
                parent_name: row.get(7)?,
                dob: row.get(8)?,
                gender: row.get(9)?,
                city: row.get(10)?,
                address: row.get(11)?,
                allergies: row.get(12)?,
                weight_kg: row.get(13)?,
                height_cm: row.get(14)?,
                patient_code: row.get(15)?,
                created_at: row.get(16)?,
                updated_at: row.get(17)?,
                deleted: row.get(18)?,
            })
        })?;
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn clients_get(&self, id: &str) -> anyhow::Result<Option<Client>> {
        let conn = self.conn()?;
        Ok(
      conn
        .query_row(
          "SELECT id, name, phone, email, notes,
                  first_name, last_name, parent_name, dob, gender, city, address, allergies, weight_kg, height_cm, patient_code,
                  created_at, updated_at, deleted
           FROM clients WHERE id=?1",
          params![id],
          |row| {
            Ok(Client {
              id: row.get(0)?,
              name: row.get(1)?,
              phone: row.get(2)?,
              email: row.get(3)?,
              notes: row.get(4)?,
              first_name: row.get(5)?,
              last_name: row.get(6)?,
              parent_name: row.get(7)?,
              dob: row.get(8)?,
              gender: row.get(9)?,
              city: row.get(10)?,
              address: row.get(11)?,
              allergies: row.get(12)?,
              weight_kg: row.get(13)?,
              height_cm: row.get(14)?,
              patient_code: row.get(15)?,
              created_at: row.get(16)?,
              updated_at: row.get(17)?,
              deleted: row.get(18)?,
            })
          },
        )
        .optional()?,
    )
    }

    pub fn clients_upsert(&self, input: ClientUpsertInput) -> anyhow::Result<Client> {
        let name = input.name.trim();
        if name.is_empty() {
            bail!("client name is required");
        }
        let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let phone = input.phone;
        let email = input.email;
        let notes = input.notes;
        let first_name = input.first_name;
        let last_name = input.last_name;
        let parent_name = input.parent_name;
        let dob = input.dob;
        let gender = input.gender;
        let city = input.city;
        let address = input.address;
        let allergies = input.allergies;
        let weight_kg = input.weight_kg;
        let height_cm = input.height_cm;
        let patient_code = input.patient_code;
        let now = now_iso();

        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        let existing_created_at: Option<String> = tx
            .query_row(
                "SELECT created_at FROM clients WHERE id=?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?;
        let created_at = existing_created_at.unwrap_or_else(|| now.clone());

        tx.execute(
      "INSERT INTO clients (id, name, phone, email, notes,
                            first_name, last_name, parent_name, dob, gender, city, address, allergies, weight_kg, height_cm, patient_code,
                            created_at, updated_at, deleted)
       VALUES (?1, ?2, ?3, ?4, ?5,
               ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
               ?17, ?18, 0)
       ON CONFLICT(id) DO UPDATE SET
         name=excluded.name,
         phone=excluded.phone,
         email=excluded.email,
         notes=excluded.notes,
         first_name=excluded.first_name,
         last_name=excluded.last_name,
         parent_name=excluded.parent_name,
         dob=excluded.dob,
         gender=excluded.gender,
         city=excluded.city,
         address=excluded.address,
         allergies=excluded.allergies,
         weight_kg=excluded.weight_kg,
         height_cm=excluded.height_cm,
         patient_code=excluded.patient_code,
         updated_at=excluded.updated_at,
         deleted=excluded.deleted",
      params![
        id,
        name,
        &phone,
        &email,
        &notes,
        &first_name,
        &last_name,
        &parent_name,
        &dob,
        &gender,
        &city,
        &address,
        &allergies,
        &weight_kg,
        &height_cm,
        &patient_code,
        &created_at,
        &now
      ],
    )?;
        let row = Client {
            id: id.clone(),
            name: name.to_string(),
            phone,
            email,
            notes,
            first_name,
            last_name,
            parent_name,
            dob,
            gender,
            city,
            address,
            allergies,
            weight_kg,
            height_cm,
            patient_code,
            created_at: created_at.clone(),
            updated_at: now.clone(),
            deleted: 0,
        };
        let payload = serde_json::to_string(&row)?;
        Self::queue_replace_pending_tx(&tx, "clients", &row.id, "upsert", &payload, &now)?;
        tx.commit()?;
        Ok(row)
    }

    pub fn clients_delete(&self, id: &str) -> anyhow::Result<()> {
        if id.trim().is_empty() {
            bail!("client id is required");
        }
        let now = now_iso();
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE clients SET deleted=1, updated_at=?2 WHERE id=?1",
            params![id, now],
        )?;
        let row = tx
      .query_row(
        "SELECT id, name, phone, email, notes,
                first_name, last_name, parent_name, dob, gender, city, address, allergies, weight_kg, height_cm, patient_code,
                created_at, updated_at, deleted
         FROM clients WHERE id=?1",
        params![id],
        |r| {
          Ok(Client {
            id: r.get(0)?,
            name: r.get(1)?,
            phone: r.get(2)?,
            email: r.get(3)?,
            notes: r.get(4)?,
            first_name: r.get(5)?,
            last_name: r.get(6)?,
            parent_name: r.get(7)?,
            dob: r.get(8)?,
            gender: r.get(9)?,
            city: r.get(10)?,
            address: r.get(11)?,
            allergies: r.get(12)?,
            weight_kg: r.get(13)?,
            height_cm: r.get(14)?,
            patient_code: r.get(15)?,
            created_at: r.get(16)?,
            updated_at: r.get(17)?,
            deleted: r.get(18)?,
          })
        },
      )
      .optional()?
      .ok_or_else(|| anyhow!("client not found"))?;
        let payload = serde_json::to_string(&row)?;
        Self::queue_replace_pending_tx(&tx, "clients", &row.id, "delete", &payload, &now)?;
        tx.commit()?;
        Ok(())
    }

    pub fn sales_list(&self, filters: Option<SalesListFilters>) -> anyhow::Result<Vec<Sale>> {
        let f = filters.unwrap_or_default();
        let include_deleted = f.include_deleted.unwrap_or(false);

        let mut sql = String::from(
      "SELECT id, client_id, date, total, notes, fiscalized, fiscalized_at, created_at, updated_at, deleted
       FROM sales WHERE 1=1",
    );
        let mut args: Vec<rusqlite::types::Value> = Vec::new();

        if !include_deleted {
            sql.push_str(" AND deleted = 0");
        }
        if let Some(cid) = f.client_id.filter(|x| !x.trim().is_empty()) {
            sql.push_str(" AND client_id = ?1");
            args.push(cid.into());
        }
        if let Some(d) = f.date_from.filter(|x| !x.trim().is_empty()) {
            sql.push_str(&format!(" AND date >= ?{}", args.len() + 1));
            args.push(d.into());
        }
        if let Some(d) = f.date_to.filter(|x| !x.trim().is_empty()) {
            sql.push_str(&format!(" AND date <= ?{}", args.len() + 1));
            args.push(d.into());
        }

        sql.push_str(" ORDER BY date DESC, updated_at DESC LIMIT 2000");

        let conn = self.conn()?;
        let mut stmt = conn.prepare(&sql)?;
        let mut out = Vec::new();
        let rows = stmt.query_map(rusqlite::params_from_iter(args.iter()), |row| {
            Ok(Sale {
                id: row.get(0)?,
                client_id: row.get(1)?,
                date: row.get(2)?,
                total: row.get(3)?,
                notes: row.get(4)?,
                fiscalized: row.get(5)?,
                fiscalized_at: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
                deleted: row.get(9)?,
            })
        })?;
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    // Cashier view: only show fiscal part of a sale (hide non-fiscal-only sales).
    // For sales without item lines, we assume they are fiscal (sale.total).
    pub fn sales_list_fiscal_only(
        &self,
        filters: Option<SalesListFilters>,
    ) -> anyhow::Result<Vec<Sale>> {
        let f = filters.unwrap_or_default();
        let include_deleted = f.include_deleted.unwrap_or(false);

        let mut sql = String::from(
      "SELECT s.id, s.client_id, s.date,
              CASE
                WHEN COALESCE(SUM(CASE WHEN vi.deleted = 0 THEN 1 ELSE 0 END), 0) > 0
                THEN COALESCE(SUM(CASE WHEN vi.deleted = 0 AND vi.fiscal = 1 THEN (vi.qty * vi.unit_price) ELSE 0 END), 0)
                ELSE s.total
              END AS total_effective,
              s.notes, s.fiscalized, s.fiscalized_at, s.created_at, s.updated_at, s.deleted
       FROM sales s
       LEFT JOIN visit_items vi ON vi.visit_id = s.id
       WHERE 1=1",
    );
        let mut args: Vec<rusqlite::types::Value> = Vec::new();

        if !include_deleted {
            sql.push_str(" AND s.deleted = 0");
        }
        if let Some(cid) = f.client_id.filter(|x| !x.trim().is_empty()) {
            sql.push_str(&format!(" AND s.client_id = ?{}", args.len() + 1));
            args.push(cid.into());
        }
        if let Some(d) = f.date_from.filter(|x| !x.trim().is_empty()) {
            sql.push_str(&format!(" AND s.date >= ?{}", args.len() + 1));
            args.push(d.into());
        }
        if let Some(d) = f.date_to.filter(|x| !x.trim().is_empty()) {
            sql.push_str(&format!(" AND s.date <= ?{}", args.len() + 1));
            args.push(d.into());
        }

        sql.push_str(" GROUP BY s.id");
        sql.push_str(
      " HAVING
          COALESCE(SUM(CASE WHEN vi.deleted = 0 THEN 1 ELSE 0 END), 0) = 0
          OR COALESCE(SUM(CASE WHEN vi.deleted = 0 AND vi.fiscal = 1 THEN (vi.qty * vi.unit_price) ELSE 0 END), 0) > 0",
    );
        sql.push_str(" ORDER BY s.date DESC, s.updated_at DESC LIMIT 2000");

        let conn = self.conn()?;
        let mut stmt = conn.prepare(&sql)?;
        let mut out = Vec::new();
        let rows = stmt.query_map(rusqlite::params_from_iter(args.iter()), |row| {
            Ok(Sale {
                id: row.get(0)?,
                client_id: row.get(1)?,
                date: row.get(2)?,
                total: row.get(3)?,
                notes: row.get(4)?,
                fiscalized: row.get(5)?,
                fiscalized_at: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
                deleted: row.get(9)?,
            })
        })?;
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn sales_daily_report(
        &self,
        date: &str,
    ) -> anyhow::Result<crate::models::DailySalesReport> {
        let date = date.trim();
        if date.is_empty() {
            bail!("date eshte i detyrueshem");
        }

        let conn = self.conn()?;
        let mut stmt = conn.prepare(
      "SELECT s.id, s.client_id, COALESCE(c.name, '') AS client_name, s.date, s.total, s.notes, s.updated_at,
              COALESCE(SUM(CASE WHEN vi.deleted = 0 AND vi.fiscal = 1 THEN (vi.qty * vi.unit_price) ELSE 0 END), 0) AS fiscal_sum,
              COALESCE(SUM(CASE WHEN vi.deleted = 0 AND vi.fiscal = 0 THEN (vi.qty * vi.unit_price) ELSE 0 END), 0) AS non_fiscal_sum,
              COALESCE(SUM(CASE WHEN vi.deleted = 0 THEN 1 ELSE 0 END), 0) AS item_count
       FROM sales s
       LEFT JOIN clients c ON c.id = s.client_id
       LEFT JOIN visit_items vi ON vi.visit_id = s.id
       WHERE s.deleted = 0 AND s.date = ?1
       GROUP BY s.id
       ORDER BY s.updated_at DESC",
    )?;

        let rows = stmt.query_map(params![date], |row| {
            let sale_id: String = row.get(0)?;
            let client_id: String = row.get(1)?;
            let client_name: String = row.get(2)?;
            let sale_date: Option<String> = row.get(3)?;
            let sale_total: f64 = row.get(4)?;
            let notes: Option<String> = row.get(5)?;
            let updated_at: String = row.get(6)?;
            let fiscal_sum: f64 = row.get(7)?;
            let non_fiscal_sum: f64 = row.get(8)?;
            let item_count: i64 = row.get(9)?;

            // If the sale has item lines, we trust them (they support mixed fiscal/non-fiscal).
            // Otherwise, fallback to the sale.total (assume fiscal).
            let (fiscal_total, non_fiscal_total, total) = if item_count > 0 {
                let total = fiscal_sum + non_fiscal_sum;
                (fiscal_sum, non_fiscal_sum, total)
            } else {
                (sale_total, 0.0, sale_total)
            };

            let classification = if fiscal_total > 0.0 && non_fiscal_total > 0.0 {
                "mixed"
            } else if non_fiscal_total > 0.0 {
                "non_fiscal"
            } else {
                "fiscal"
            };

            Ok(crate::models::DailySaleRow {
                sale_id,
                client_id,
                client_name,
                date: sale_date,
                total,
                fiscal_total,
                non_fiscal_total,
                notes,
                updated_at,
                classification: classification.to_string(),
            })
        })?;

        let mut out_rows: Vec<crate::models::DailySaleRow> = Vec::new();
        for r in rows {
            out_rows.push(r?);
        }

        let mut fiscal_total = 0.0_f64;
        let mut non_fiscal_total = 0.0_f64;
        let mut count_fiscal_only = 0_i64;
        let mut count_non_fiscal_only = 0_i64;
        let mut count_mixed = 0_i64;

        for r in &out_rows {
            fiscal_total += r.fiscal_total;
            non_fiscal_total += r.non_fiscal_total;
            match r.classification.as_str() {
                "mixed" => count_mixed += 1,
                "non_fiscal" => count_non_fiscal_only += 1,
                _ => count_fiscal_only += 1,
            }
        }

        Ok(crate::models::DailySalesReport {
            date: date.to_string(),
            total: fiscal_total + non_fiscal_total,
            fiscal_total,
            non_fiscal_total,
            count_sales: out_rows.len() as i64,
            count_fiscal_only,
            count_non_fiscal_only,
            count_mixed,
            rows: out_rows,
        })
    }

    pub fn sales_daily_report_for_doctor(
        &self,
        date: &str,
        doctor_id: &str,
    ) -> anyhow::Result<crate::models::DailySalesReport> {
        let date = date.trim();
        if date.is_empty() {
            bail!("date eshte i detyrueshem");
        }
        let doctor_id = doctor_id.trim();
        if doctor_id.is_empty() {
            bail!("doctor_id eshte i detyrueshem");
        }

        let conn = self.conn()?;
        let mut stmt = conn.prepare(
      "SELECT s.id, s.client_id, COALESCE(c.name, '') AS client_name, s.date, s.total, s.notes, s.updated_at,
              COALESCE(SUM(CASE WHEN vi.deleted = 0 AND vi.fiscal = 1 THEN (vi.qty * vi.unit_price) ELSE 0 END), 0) AS fiscal_sum,
              COALESCE(SUM(CASE WHEN vi.deleted = 0 AND vi.fiscal = 0 THEN (vi.qty * vi.unit_price) ELSE 0 END), 0) AS non_fiscal_sum,
              COALESCE(SUM(CASE WHEN vi.deleted = 0 THEN 1 ELSE 0 END), 0) AS item_count
       FROM sales s
       INNER JOIN visits v ON v.id = s.id
       LEFT JOIN clients c ON c.id = s.client_id
       LEFT JOIN visit_items vi ON vi.visit_id = s.id
       WHERE s.deleted = 0 AND v.deleted = 0 AND s.date = ?1 AND v.doctor_id = ?2
       GROUP BY s.id
       ORDER BY s.updated_at DESC",
    )?;

        let rows = stmt.query_map(params![date, doctor_id], |row| {
            let sale_id: String = row.get(0)?;
            let client_id: String = row.get(1)?;
            let client_name: String = row.get(2)?;
            let sale_date: Option<String> = row.get(3)?;
            let sale_total: f64 = row.get(4)?;
            let notes: Option<String> = row.get(5)?;
            let updated_at: String = row.get(6)?;
            let fiscal_sum: f64 = row.get(7)?;
            let non_fiscal_sum: f64 = row.get(8)?;
            let item_count: i64 = row.get(9)?;

            let (fiscal_total, non_fiscal_total, total) = if item_count > 0 {
                let total = fiscal_sum + non_fiscal_sum;
                (fiscal_sum, non_fiscal_sum, total)
            } else {
                (sale_total, 0.0, sale_total)
            };

            let classification = if fiscal_total > 0.0 && non_fiscal_total > 0.0 {
                "mixed"
            } else if non_fiscal_total > 0.0 {
                "non_fiscal"
            } else {
                "fiscal"
            };

            Ok(crate::models::DailySaleRow {
                sale_id,
                client_id,
                client_name,
                date: sale_date,
                total,
                fiscal_total,
                non_fiscal_total,
                notes,
                updated_at,
                classification: classification.to_string(),
            })
        })?;

        let mut out_rows: Vec<crate::models::DailySaleRow> = Vec::new();
        for r in rows {
            out_rows.push(r?);
        }

        let mut fiscal_total = 0.0_f64;
        let mut non_fiscal_total = 0.0_f64;
        let mut count_fiscal_only = 0_i64;
        let mut count_non_fiscal_only = 0_i64;
        let mut count_mixed = 0_i64;

        for r in &out_rows {
            fiscal_total += r.fiscal_total;
            non_fiscal_total += r.non_fiscal_total;
            match r.classification.as_str() {
                "mixed" => count_mixed += 1,
                "non_fiscal" => count_non_fiscal_only += 1,
                _ => count_fiscal_only += 1,
            }
        }

        Ok(crate::models::DailySalesReport {
            date: date.to_string(),
            total: fiscal_total + non_fiscal_total,
            fiscal_total,
            non_fiscal_total,
            count_sales: out_rows.len() as i64,
            count_fiscal_only,
            count_non_fiscal_only,
            count_mixed,
            rows: out_rows,
        })
    }

    pub fn sales_get(&self, id: &str) -> anyhow::Result<Option<Sale>> {
        let conn = self.conn()?;
        Ok(
      conn
        .query_row(
          "SELECT id, client_id, date, total, notes, fiscalized, fiscalized_at, created_at, updated_at, deleted
           FROM sales WHERE id=?1",
          params![id],
          |row| {
            Ok(Sale {
              id: row.get(0)?,
              client_id: row.get(1)?,
              date: row.get(2)?,
              total: row.get(3)?,
              notes: row.get(4)?,
              fiscalized: row.get(5)?,
              fiscalized_at: row.get(6)?,
              created_at: row.get(7)?,
              updated_at: row.get(8)?,
              deleted: row.get(9)?,
            })
          },
        )
        .optional()?,
    )
    }

    pub fn sales_invoice_number(&self, sale_id: &str) -> anyhow::Result<String> {
        let sale_id = sale_id.trim();
        if sale_id.is_empty() {
            bail!("sale_id eshte i detyrueshem");
        }

        let conn = self.conn()?;
        let (sale_date, created_at): (Option<String>, String) = conn
            .query_row(
                "SELECT date, created_at FROM sales WHERE id=?1",
                params![sale_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| anyhow!("fatura nuk u gjet"))?;

        let ref_date = sale_date
            .as_deref()
            .filter(|d| d.len() >= 10)
            .map(|d| d[0..10].to_string())
            .or_else(|| {
                if created_at.len() >= 10 {
                    Some(created_at[0..10].to_string())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "1970-01-01".to_string());

        let period = if ref_date.len() >= 7 {
            ref_date[0..7].to_string()
        } else {
            "1970-01".to_string()
        };
        let year = period.get(0..4).unwrap_or("1970");
        let month = period.get(5..7).unwrap_or("01");

        let seq: i64 = conn.query_row(
            "SELECT COUNT(1)
       FROM sales
       WHERE substr(COALESCE(date, substr(created_at,1,10)),1,7)=?1
         AND (
           COALESCE(date, substr(created_at,1,10)) < ?2
           OR (COALESCE(date, substr(created_at,1,10)) = ?2 AND created_at <= ?3)
         )",
            params![period, ref_date, created_at],
            |row| row.get(0),
        )?;

        let nr = if seq <= 0 { 1 } else { seq };
        Ok(format!("{}/{}/{:04}", year, month, nr))
    }

    pub fn sales_upsert(&self, input: SaleUpsertInput) -> anyhow::Result<Sale> {
        if input.client_id.trim().is_empty() {
            bail!("client_id is required");
        }
        if !input.total.is_finite() || input.total < 0.0 {
            bail!("total must be a finite number >= 0");
        }
        let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let client_id = input.client_id;
        let date = input.date;
        let notes = input.notes;
        let total = input.total;
        let now = now_iso();

        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        let existing: Option<(String, i64, Option<String>)> = tx
            .query_row(
                "SELECT created_at, fiscalized, fiscalized_at FROM sales WHERE id=?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let created_at = existing
            .as_ref()
            .map(|x| x.0.clone())
            .unwrap_or_else(|| now.clone());
        let fiscalized = existing.as_ref().map(|x| x.1).unwrap_or(0);
        let fiscalized_at: Option<String> = existing.and_then(|x| x.2);

        tx.execute(
      "INSERT INTO sales (id, client_id, date, total, notes, fiscalized, fiscalized_at, created_at, updated_at, deleted)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0)
       ON CONFLICT(id) DO UPDATE SET
         client_id=excluded.client_id,
         date=excluded.date,
         total=excluded.total,
         notes=excluded.notes,
         fiscalized=excluded.fiscalized,
         fiscalized_at=excluded.fiscalized_at,
         updated_at=excluded.updated_at,
         deleted=excluded.deleted",
      params![id, &client_id, &date, total, &notes, fiscalized, &fiscalized_at, &created_at, &now],
    )?;
        let row = Sale {
            id: id.clone(),
            client_id,
            date,
            total,
            notes,
            fiscalized,
            fiscalized_at,
            created_at: created_at.clone(),
            updated_at: now.clone(),
            deleted: 0,
        };
        let payload = serde_json::to_string(&row)?;
        Self::queue_replace_pending_tx(&tx, "sales", &row.id, "upsert", &payload, &now)?;
        tx.commit()?;
        Ok(row)
    }

    pub fn sales_delete(&self, id: &str) -> anyhow::Result<()> {
        if id.trim().is_empty() {
            bail!("sale id is required");
        }
        let now = now_iso();
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE sales SET deleted=1, updated_at=?2 WHERE id=?1",
            params![id, now],
        )?;
        let row = tx
      .query_row(
        "SELECT id, client_id, date, total, notes, fiscalized, fiscalized_at, created_at, updated_at, deleted FROM sales WHERE id=?1",
        params![id],
        |r| {
          Ok(Sale {
            id: r.get(0)?,
            client_id: r.get(1)?,
            date: r.get(2)?,
            total: r.get(3)?,
            notes: r.get(4)?,
            fiscalized: r.get(5)?,
            fiscalized_at: r.get(6)?,
            created_at: r.get(7)?,
            updated_at: r.get(8)?,
            deleted: r.get(9)?,
          })
        },
      )
      .optional()?
      .ok_or_else(|| anyhow!("sale not found"))?;
        let payload = serde_json::to_string(&row)?;
        Self::queue_replace_pending_tx(&tx, "sales", &row.id, "delete", &payload, &now)?;

        // Keep history consistent: when a sale is removed, remove linked payments too.
        let mut linked_payments: Vec<Payment> = Vec::new();
        {
            let mut stmt = tx.prepare(
        "SELECT id, client_id, sale_id, date, amount, method, notes, created_at, updated_at, deleted
         FROM payments
         WHERE sale_id = ?1 AND deleted = 0",
      )?;
            let rows = stmt.query_map(params![id], |r| {
                Ok(Payment {
                    id: r.get(0)?,
                    client_id: r.get(1)?,
                    sale_id: r.get(2)?,
                    date: r.get(3)?,
                    amount: r.get(4)?,
                    method: r
                        .get::<_, Option<String>>(5)?
                        .unwrap_or_else(|| "cash".to_string()),
                    notes: r.get(6)?,
                    created_at: r.get(7)?,
                    updated_at: r.get(8)?,
                    deleted: r.get(9)?,
                })
            })?;
            for r in rows {
                linked_payments.push(r?);
            }
        }
        for p in linked_payments {
            tx.execute(
                "UPDATE payments SET deleted=1, updated_at=?2 WHERE id=?1",
                params![&p.id, &now],
            )?;
            let mut del = p.clone();
            del.deleted = 1;
            del.updated_at = now.clone();
            let p_payload = serde_json::to_string(&del)?;
            Self::queue_replace_pending_tx(&tx, "payments", &del.id, "delete", &p_payload, &now)?;
        }

        tx.commit()?;
        Ok(())
    }

    fn fiscal_vat_group_for_code(vat_code: &str, vat_e_group: i64) -> i64 {
        match vat_code.trim().to_uppercase().as_str() {
            "A" => 1,
            "C" => 3,
            "D" => 4,
            "E" => vat_e_group,
            _ => 3, // Default to C
        }
    }

    fn fiscal_qty_str(qty: f64) -> String {
        if (qty.round() - qty).abs() < 0.000_001 {
            format!("{}", qty.round() as i64)
        } else {
            format!("{qty:.3}")
        }
    }

    fn fiscal_sanitize_text(v: &str) -> String {
        let mut s = v.replace('\r', " ").replace('\n', " ").replace(';', ",");
        s = s.trim().to_string();
        if s.is_empty() {
            return "Sherbim".to_string();
        }
        if s.len() > 120 {
            s.truncate(120);
        }
        s
    }

    fn fiscal_write_inp_atomic(path: &Path, body: &str) -> anyhow::Result<()> {
        let tmp_name = format!(
            "{}.tmp-{}",
            path.file_name()
                .and_then(|x| x.to_str())
                .unwrap_or("fiscal.inp"),
            &Uuid::new_v4().to_string()[..8]
        );
        let tmp_path = path.with_file_name(tmp_name);

        {
            let mut f = fs::File::create(&tmp_path)
                .with_context(|| format!("create tmp inp: {}", tmp_path.display()))?;
            f.write_all(body.as_bytes())
                .with_context(|| format!("write tmp inp: {}", tmp_path.display()))?;
            f.flush()
                .with_context(|| format!("flush tmp inp: {}", tmp_path.display()))?;
            let _ = f.sync_all();
        }

        fs::rename(&tmp_path, path).with_context(|| {
            format!(
                "rename tmp->inp: {} -> {}",
                tmp_path.display(),
                path.display()
            )
        })?;
        Ok(())
    }

    fn fiscal_clear_article_path() -> PathBuf {
        #[cfg(target_os = "windows")]
        {
            PathBuf::from(r"C:\Temp\ClearArticle.inp")
        }
        #[cfg(not(target_os = "windows"))]
        {
            std::env::temp_dir().join("ClearArticle.inp")
        }
    }

    fn fiscal_emit_clear_article_command() -> anyhow::Result<PathBuf> {
        let path = Self::fiscal_clear_article_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create clear-article dir: {}", parent.display()))?;
        }
        Self::fiscal_write_inp_atomic(&path, "O,1,______,_,__;ALL\n")?;
        Ok(path)
    }

    fn fiscal_emit_clear_article_command_in_dir(
        output_dir: &Path,
        label: &str,
    ) -> anyhow::Result<PathBuf> {
        let path = output_dir.join(format!(
            "clear-{}-{}.inp",
            label,
            &Uuid::new_v4().to_string()[..8]
        ));
        Self::fiscal_write_inp_atomic(&path, "O,1,______,_,__;ALL\n")?;
        Ok(path)
    }

    fn fiscal_emit_clear_article_twice(output_dir: &Path, label: &str) -> anyhow::Result<()> {
        // 1) Legacy fixed file (compatibility with existing fiscal setup/scripts).
        let _ = Self::fiscal_emit_clear_article_command()?;
        // 2) Dedicated unique command file (prevents overwrite/race issues).
        let _ = Self::fiscal_emit_clear_article_command_in_dir(output_dir, label)?;
        Ok(())
    }

    pub fn fiscal_clear_article_on_app_open(&self) -> anyhow::Result<()> {
        #[cfg(target_os = "windows")]
        let output_dir = PathBuf::from(r"C:\Temp");
        #[cfg(not(target_os = "windows"))]
        let output_dir = std::env::temp_dir();
        fs::create_dir_all(&output_dir)
            .with_context(|| format!("create startup clear dir: {}", output_dir.display()))?;
        // Run clear command twice on app open.
        Self::fiscal_emit_clear_article_twice(&output_dir, "startup")
    }

    fn fiscal_wait_out_text(inp_path: &Path, timeout: Duration) -> Option<String> {
        let out_path = inp_path.with_extension("out");
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            match fs::read_to_string(&out_path) {
                Ok(x) => return Some(x),
                Err(e) => {
                    if e.kind() != std::io::ErrorKind::NotFound {
                        return Some(format!("read_out_error: {}", e));
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        None
    }

    fn fiscal_out_has_error(raw: &str) -> bool {
        let s = raw.to_ascii_lowercase();
        s.contains("error")
            || s.contains("invalid")
            || s.contains("syntax")
            || s.contains("failed")
            || s.contains("gabim")
            || s.contains("not allowed")
    }

    fn fiscal_out_program_article_error(raw: &str) -> bool {
        let s = raw.to_ascii_lowercase();
        s.contains("cannot program or change article")
            || s.contains("progr_article")
            || s.contains("error #11")
    }

    fn fiscal_out_note_status_2(raw: &str) -> bool {
        raw.to_ascii_lowercase().contains("notestatus;2")
    }

    fn fiscal_print_errors_path(inp_path: &Path) -> Option<PathBuf> {
        let parent = inp_path.parent()?;
        let name = inp_path.file_name()?;
        Some(parent.join("PrintErrors").join(name))
    }

    fn fiscal_wait_moved_to_print_errors(inp_path: &Path, timeout: Duration) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if let Some(p) = Self::fiscal_print_errors_path(inp_path) {
                if p.exists() {
                    return true;
                }
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        false
    }

    fn fiscal_plu_counter_key(clinic_id: &str) -> String {
        let mut suffix = clinic_id
            .trim()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect::<String>();
        if suffix.is_empty() {
            suffix = "default".to_string();
        }
        format!("fiscal_plu_counter_{}", suffix)
    }

    pub fn fiscal_receipt_generate_inp(
        &self,
        sale_id: &str,
        output_dir: &Path,
        allow_any_items_if_no_fiscal: bool,
    ) -> anyhow::Result<PathBuf> {
        let sale_id = sale_id.trim();
        if sale_id.is_empty() {
            bail!("sale_id eshte i detyrueshem");
        }

        fs::create_dir_all(output_dir)
            .with_context(|| format!("create fiscal dir: {}", output_dir.display()))?;

        let vat_e_group: i64 = self
            .setting_get("fiscal_vat_e_group")?
            .and_then(|v| v.trim().parse::<i64>().ok())
            .filter(|v| *v == 2 || *v == 4 || *v == 5)
            .unwrap_or(5);

        let sale: Sale;
        let mut items: Vec<VisitItem> = Vec::new();
        let mut used_non_fiscal_fallback = false;
        {
            let conn = self.conn()?;
            sale = conn
        .query_row(
          "SELECT id, client_id, date, total, notes, fiscalized, fiscalized_at, created_at, updated_at, deleted
           FROM sales WHERE id=?1",
          params![sale_id],
          |row| {
            Ok(Sale {
              id: row.get(0)?,
              client_id: row.get(1)?,
              date: row.get(2)?,
              total: row.get(3)?,
              notes: row.get(4)?,
              fiscalized: row.get(5)?,
              fiscalized_at: row.get(6)?,
              created_at: row.get(7)?,
              updated_at: row.get(8)?,
              deleted: row.get(9)?,
            })
          },
        )
        .optional()?
        .ok_or_else(|| anyhow!("fatura nuk u gjet"))?;
            if sale.deleted != 0 {
                bail!("fatura eshte e fshire");
            }
            if sale.fiscalized != 0 {
                bail!("fatura eshte fiskalizuar tashme");
            }

            let client_exists: Option<String> = conn
                .query_row(
                    "SELECT id FROM clients WHERE id=?1",
                    params![&sale.client_id],
                    |row| row.get(0),
                )
                .optional()?;
            if client_exists.is_none() {
                bail!("pacienti nuk u gjet");
            }

            let mut stmt = conn.prepare(
        "SELECT id, visit_id, client_id, tooth, title, qty, unit_price, fiscal, vat_code, fiscalized, fiscalized_at, notes, created_at, updated_at, deleted
         FROM visit_items
         WHERE visit_id=?1 AND deleted=0 AND fiscal=1 AND fiscalized=0
         ORDER BY created_at ASC",
      )?;
            let rows = stmt.query_map(params![sale_id], |row| {
                Ok(VisitItem {
                    id: row.get(0)?,
                    visit_id: row.get(1)?,
                    client_id: row.get(2)?,
                    tooth: row.get(3)?,
                    title: row.get(4)?,
                    qty: row.get(5)?,
                    unit_price: row.get(6)?,
                    fiscal: row.get(7)?,
                    vat_code: row.get(8)?,
                    fiscalized: row.get(9)?,
                    fiscalized_at: row.get(10)?,
                    notes: row.get(11)?,
                    created_at: row.get(12)?,
                    updated_at: row.get(13)?,
                    deleted: row.get(14)?,
                })
            })?;
            for r in rows {
                items.push(r?);
            }

            if items.is_empty() && allow_any_items_if_no_fiscal {
                let mut stmt_any = conn.prepare(
          "SELECT id, visit_id, client_id, tooth, title, qty, unit_price, fiscal, vat_code, fiscalized, fiscalized_at, notes, created_at, updated_at, deleted
           FROM visit_items
           WHERE visit_id=?1 AND deleted=0 AND fiscalized=0
           ORDER BY created_at ASC",
        )?;
                let rows_any = stmt_any.query_map(params![sale_id], |row| {
                    Ok(VisitItem {
                        id: row.get(0)?,
                        visit_id: row.get(1)?,
                        client_id: row.get(2)?,
                        tooth: row.get(3)?,
                        title: row.get(4)?,
                        qty: row.get(5)?,
                        unit_price: row.get(6)?,
                        fiscal: row.get(7)?,
                        vat_code: row.get(8)?,
                        fiscalized: row.get(9)?,
                        fiscalized_at: row.get(10)?,
                        notes: row.get(11)?,
                        created_at: row.get(12)?,
                        updated_at: row.get(13)?,
                        deleted: row.get(14)?,
                    })
                })?;
                for r in rows_any {
                    items.push(r?);
                }
                if !items.is_empty() {
                    used_non_fiscal_fallback = true;
                }
            }
        }
        if items.is_empty() {
            bail!("nuk ka rreshta fiskal pa fiskalizuar");
        }

        // Required by fiscal flow: clear article list once before printing receipt.
        Self::fiscal_emit_clear_article_twice(output_dir, "before")?;

        // Step A: check if there is an open note. If NoteStatus=2, close with N before real sale.
        let g_path = output_dir.join(format!(
            "status-{}-{}.inp",
            sale.id,
            &Uuid::new_v4().to_string()[..8]
        ));
        Self::fiscal_write_inp_atomic(&g_path, "G,1,______,_,__;NoteStatus\n")?;
        if let Some(out) = Self::fiscal_wait_out_text(&g_path, Duration::from_secs(4)) {
            if Self::fiscal_out_note_status_2(&out) {
                let n_path = output_dir.join(format!(
                    "cancel-open-{}-{}.inp",
                    sale.id,
                    &Uuid::new_v4().to_string()[..8]
                ));
                Self::fiscal_write_inp_atomic(&n_path, "N,1,______,_,__;\n")?;
                let _ = Self::fiscal_wait_out_text(&n_path, Duration::from_secs(3));
            }
        }

        // Real sale lines: only actual sale items (no test rows).
        let mut sale_body = String::new();
        let mut lines_for_fallback: Vec<(String, f64, f64, i64, i64)> = Vec::new(); // desc, unit_price, qty, vat_group, plu
        let mut total = 0.0_f64;
        let clinic_id = self.setting_get("clinic_id")?.unwrap_or_default();
        let plu_counter_key = Self::fiscal_plu_counter_key(&clinic_id);
        let mut last_plu = self
            .setting_get(&plu_counter_key)?
            .and_then(|v| v.trim().parse::<i64>().ok())
            .unwrap_or(15005);
        if last_plu < 15005 {
            last_plu = 15005;
            self.setting_set(&plu_counter_key, &last_plu.to_string())?;
        }

        for it in &items {
            let qty = if it.qty.is_finite() && it.qty > 0.0 {
                it.qty
            } else {
                1.0
            };
            let unit_price = if it.unit_price.is_finite() && it.unit_price >= 0.0 {
                it.unit_price
            } else {
                0.0
            };
            let sub = qty * unit_price;
            total += sub;

            let desc = Self::fiscal_sanitize_text(&it.title);
            let vat_group = Self::fiscal_vat_group_for_code(&it.vat_code, vat_e_group);

            // Per-clinic PLU sequence:
            // default last value starts at 15005, then each product consumes next PLU.
            last_plu += 1;
            let item_code = last_plu;
            self.setting_set(&plu_counter_key, &last_plu.to_string())?;

            sale_body.push_str(&format!(
                "S,1,______,_,__;{};{:.2};{};1;1;{};0;{};0;0\n",
                desc,
                unit_price,
                Self::fiscal_qty_str(qty),
                vat_group,
                item_code
            ));
            lines_for_fallback.push((desc, unit_price, qty, vat_group, item_code));
        }
        sale_body.push_str("T,1,______,_,__;\n");

        let primary_path = output_dir.join(format!(
            "kupon-{}-{}.inp",
            sale.id,
            &Uuid::new_v4().to_string()[..8]
        ));
        Self::fiscal_write_inp_atomic(&primary_path, &sale_body)?;
        let primary_out = Self::fiscal_wait_out_text(&primary_path, Duration::from_secs(8));
        let primary_print_error =
            Self::fiscal_wait_moved_to_print_errors(&primary_path, Duration::from_secs(8));

        // Step B: if S/T failed, fallback:
        // - Program-article fallback (N + U + real S/T) for Error #11 / article mode issues.
        // - Otherwise keep previous K + real S/T fallback.
        let mut final_path = primary_path.clone();
        let mut failed_after_fallback = false;
        let primary_out_has_error = primary_out
            .as_deref()
            .map(Self::fiscal_out_has_error)
            .unwrap_or(false);
        if primary_out_has_error || primary_print_error {
            let needs_program_fallback = primary_print_error
                || primary_out
                    .as_deref()
                    .map(Self::fiscal_out_program_article_error)
                    .unwrap_or(false);

            if needs_program_fallback {
                let mut u_body = String::from("N,1,______,_,__;\n");
                for (desc, unit_price, _qty, vat_group, item_code) in &lines_for_fallback {
                    u_body.push_str(&format!(
                        "U,1,______,_,__;{};{:.2};0;1;1;{};0;{};;;\n",
                        desc, unit_price, vat_group, item_code
                    ));
                }
                u_body.push_str(&sale_body);

                let u_path = output_dir.join(format!(
                    "kupon-u-{}-{}.inp",
                    sale.id,
                    &Uuid::new_v4().to_string()[..8]
                ));
                Self::fiscal_write_inp_atomic(&u_path, &u_body)?;
                final_path = u_path.clone();
                let u_out = Self::fiscal_wait_out_text(&u_path, Duration::from_secs(8));
                let u_print_error =
                    Self::fiscal_wait_moved_to_print_errors(&u_path, Duration::from_secs(8));
                let u_failed = u_print_error
                    || u_out
                        .as_deref()
                        .map(Self::fiscal_out_has_error)
                        .unwrap_or(false);
                if u_failed {
                    failed_after_fallback = true;
                }
            } else {
                let mut k_body = String::from("K,1,______,_,__;;1;0000;;;;;;1;\n");
                k_body.push_str(&sale_body);
                let k_path = output_dir.join(format!(
                    "kupon-k-{}-{}.inp",
                    sale.id,
                    &Uuid::new_v4().to_string()[..8]
                ));
                Self::fiscal_write_inp_atomic(&k_path, &k_body)?;
                final_path = k_path.clone();
                let k_out = Self::fiscal_wait_out_text(&k_path, Duration::from_secs(8));
                let k_print_error =
                    Self::fiscal_wait_moved_to_print_errors(&k_path, Duration::from_secs(8));
                let k_failed = k_print_error
                    || k_out
                        .as_deref()
                        .map(Self::fiscal_out_has_error)
                        .unwrap_or(false);
                if k_failed {
                    failed_after_fallback = true;
                }
            }
        }

        // Step C: if still failing, try forced close T(cash) and then N.
        if failed_after_fallback {
            let close_body = format!("T,1,______,_,__;0;{:.2};;;;\nN,1,______,_,__;\n", total);
            let close_path = output_dir.join(format!(
                "kupon-close-{}-{}.inp",
                sale.id,
                &Uuid::new_v4().to_string()[..8]
            ));
            Self::fiscal_write_inp_atomic(&close_path, &close_body)?;
            let _ = Self::fiscal_wait_out_text(&close_path, Duration::from_secs(6));
            let _ = Self::fiscal_emit_clear_article_twice(output_dir, "failed-after");
            bail!("fiskalizimi deshtoi edhe pas fallback (K dhe mbyllja T/N).");
        }

        // Required by fiscal flow: clear article list twice after printing receipt.
        Self::fiscal_emit_clear_article_twice(output_dir, "after-1")?;
        Self::fiscal_emit_clear_article_twice(output_dir, "after-2")?;

        // Mark items as fiscalized and queue them for sync.
        let now = now_iso();
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        for it in &mut items {
            let updated_fiscal_flag = if used_non_fiscal_fallback {
                1
            } else {
                it.fiscal
            };
            tx.execute(
        "UPDATE visit_items SET fiscal=?3, fiscalized=1, fiscalized_at=?2, updated_at=?2 WHERE id=?1",
        params![&it.id, &now, updated_fiscal_flag],
      )?;
            it.fiscal = updated_fiscal_flag;
            it.fiscalized = 1;
            it.fiscalized_at = Some(now.clone());
            it.updated_at = now.clone();
            let payload = serde_json::to_string(&it)?;
            Self::queue_replace_pending_tx(&tx, "visit_items", &it.id, "upsert", &payload, &now)?;
        }

        tx.execute(
            "UPDATE sales SET fiscalized=1, fiscalized_at=?2, updated_at=?2 WHERE id=?1",
            params![sale_id, &now],
        )?;
        let updated_sale = Sale {
            fiscalized: 1,
            fiscalized_at: Some(now.clone()),
            updated_at: now.clone(),
            ..sale
        };
        let payload = serde_json::to_string(&updated_sale)?;
        Self::queue_replace_pending_tx(&tx, "sales", &updated_sale.id, "upsert", &payload, &now)?;

        // Auto-save fiscalized value as a payment entry (cash) for this sale.
        // Avoid duplicates when manual payments already exist.
        let auto_note = "[AUTO-FISCAL] Kupon fiskal";
        let existing_auto_payment: Option<(String, String)> = tx
            .query_row(
                "SELECT id, created_at
         FROM payments
         WHERE sale_id=?1 AND deleted=0 AND COALESCE(notes,'')=?2
         LIMIT 1",
                params![&updated_sale.id, auto_note],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let has_any_payment = tx
            .query_row(
                "SELECT 1 FROM payments WHERE sale_id=?1 AND deleted=0 LIMIT 1",
                params![&updated_sale.id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .is_some();

        if existing_auto_payment.is_some() || !has_any_payment {
            // Use a deterministic id (same as sale id) for the auto fiscal payment.
            // This keeps the operation idempotent and avoids duplicate rows on rapid repeated calls.
            let (payment_id, payment_created_at) =
                existing_auto_payment.unwrap_or_else(|| (updated_sale.id.clone(), now.clone()));
            let payment_date = updated_sale
                .date
                .clone()
                .or_else(|| now.get(0..10).map(|x| x.to_string()));

            tx.execute(
        "INSERT INTO payments (id, client_id, sale_id, date, amount, method, notes, created_at, updated_at, deleted)
         VALUES (?1, ?2, ?3, ?4, ?5, 'cash', ?6, ?7, ?8, 0)
         ON CONFLICT(id) DO UPDATE SET
           client_id=excluded.client_id,
           sale_id=excluded.sale_id,
           date=excluded.date,
           amount=excluded.amount,
           method=excluded.method,
           notes=excluded.notes,
           updated_at=excluded.updated_at,
           deleted=excluded.deleted",
        params![
          &payment_id,
          &updated_sale.client_id,
          &updated_sale.id,
          &payment_date,
          total,
          auto_note,
          &payment_created_at,
          &now
        ],
      )?;

            let auto_payment = Payment {
                id: payment_id.clone(),
                client_id: updated_sale.client_id.clone(),
                sale_id: Some(updated_sale.id.clone()),
                date: payment_date,
                amount: total,
                method: "cash".to_string(),
                notes: Some(auto_note.to_string()),
                created_at: payment_created_at,
                updated_at: now.clone(),
                deleted: 0,
            };
            let payment_payload = serde_json::to_string(&auto_payment)?;
            Self::queue_replace_pending_tx(
                &tx,
                "payments",
                &auto_payment.id,
                "upsert",
                &payment_payload,
                &now,
            )?;
        }

        tx.commit()?;
        Ok(final_path)
    }

    pub fn sales_mark_fiscalized_manual(
        &self,
        sale_id: &str,
        reason: Option<&str>,
    ) -> anyhow::Result<()> {
        let sale_id = sale_id.trim();
        if sale_id.is_empty() {
            bail!("sale_id eshte i detyrueshem");
        }

        let now = now_iso();
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;

        let sale = tx
      .query_row(
        "SELECT id, client_id, date, total, notes, fiscalized, fiscalized_at, created_at, updated_at, deleted
         FROM sales WHERE id=?1",
        params![sale_id],
        |r| {
          Ok(Sale {
            id: r.get(0)?,
            client_id: r.get(1)?,
            date: r.get(2)?,
            total: r.get(3)?,
            notes: r.get(4)?,
            fiscalized: r.get(5)?,
            fiscalized_at: r.get(6)?,
            created_at: r.get(7)?,
            updated_at: r.get(8)?,
            deleted: r.get(9)?,
          })
        },
      )
      .optional()?
      .ok_or_else(|| anyhow!("fatura nuk u gjet"))?;

        if sale.deleted != 0 {
            bail!("fatura eshte e fshire");
        }
        if sale.fiscalized == 1 {
            return Ok(());
        }

        let mut items: Vec<VisitItem> = Vec::new();
        {
            let mut stmt = tx.prepare(
        "SELECT id, visit_id, client_id, tooth, title, qty, unit_price, fiscal, vat_code, fiscalized, fiscalized_at, notes, created_at, updated_at, deleted
         FROM visit_items
         WHERE visit_id=?1 AND deleted=0 AND fiscalized=0
         ORDER BY created_at ASC",
      )?;
            let rows = stmt.query_map(params![sale_id], |row| {
                Ok(VisitItem {
                    id: row.get(0)?,
                    visit_id: row.get(1)?,
                    client_id: row.get(2)?,
                    tooth: row.get(3)?,
                    title: row.get(4)?,
                    qty: row.get(5)?,
                    unit_price: row.get(6)?,
                    fiscal: row.get(7)?,
                    vat_code: row.get(8)?,
                    fiscalized: row.get(9)?,
                    fiscalized_at: row.get(10)?,
                    notes: row.get(11)?,
                    created_at: row.get(12)?,
                    updated_at: row.get(13)?,
                    deleted: row.get(14)?,
                })
            })?;
            for r in rows {
                items.push(r?);
            }
        }

        for it in &mut items {
            tx.execute(
        "UPDATE visit_items SET fiscal=1, fiscalized=1, fiscalized_at=?2, updated_at=?2 WHERE id=?1",
        params![&it.id, &now],
      )?;
            it.fiscal = 1;
            it.fiscalized = 1;
            it.fiscalized_at = Some(now.clone());
            it.updated_at = now.clone();
            let payload = serde_json::to_string(&it)?;
            Self::queue_replace_pending_tx(&tx, "visit_items", &it.id, "upsert", &payload, &now)?;
        }

        tx.execute(
            "UPDATE sales SET fiscalized=1, fiscalized_at=?2, updated_at=?2 WHERE id=?1",
            params![sale_id, &now],
        )?;

        let updated_sale = Sale {
            fiscalized: 1,
            fiscalized_at: Some(now.clone()),
            updated_at: now.clone(),
            ..sale
        };
        let payload = serde_json::to_string(&updated_sale)?;
        Self::queue_replace_pending_tx(&tx, "sales", &updated_sale.id, "upsert", &payload, &now)?;

        let auto_note = "[AUTO-FISCAL] Kupon fiskal";
        let note = if let Some(r) = reason {
            let rr = r.trim();
            if rr.is_empty() {
                auto_note.to_string()
            } else {
                format!("{auto_note} [MANUAL] {rr}")
            }
        } else {
            auto_note.to_string()
        };
        let payment_exists = tx
            .query_row(
                "SELECT 1 FROM payments WHERE sale_id=?1 AND deleted=0 LIMIT 1",
                params![&updated_sale.id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .is_some();
        if !payment_exists {
            let payment_date = updated_sale
                .date
                .clone()
                .or_else(|| now.get(0..10).map(|x| x.to_string()));
            let payment = Payment {
                id: updated_sale.id.clone(),
                client_id: updated_sale.client_id.clone(),
                sale_id: Some(updated_sale.id.clone()),
                date: payment_date,
                amount: updated_sale.total,
                method: "cash".to_string(),
                notes: Some(note),
                created_at: now.clone(),
                updated_at: now.clone(),
                deleted: 0,
            };
            tx.execute(
        "INSERT INTO payments (id, client_id, sale_id, date, amount, method, notes, created_at, updated_at, deleted)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0)
         ON CONFLICT(id) DO UPDATE SET
           client_id=excluded.client_id,
           sale_id=excluded.sale_id,
           date=excluded.date,
           amount=excluded.amount,
           method=excluded.method,
           notes=excluded.notes,
           updated_at=excluded.updated_at,
           deleted=excluded.deleted",
        params![
          &payment.id,
          &payment.client_id,
          &payment.sale_id,
          &payment.date,
          payment.amount,
          &payment.method,
          &payment.notes,
          &payment.created_at,
          &payment.updated_at
        ],
      )?;
            let payment_payload = serde_json::to_string(&payment)?;
            Self::queue_replace_pending_tx(
                &tx,
                "payments",
                &payment.id,
                "upsert",
                &payment_payload,
                &now,
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    pub fn sales_mark_non_fiscal_manual(
        &self,
        sale_id: &str,
        reason: Option<&str>,
    ) -> anyhow::Result<()> {
        let sale_id = sale_id.trim();
        if sale_id.is_empty() {
            bail!("sale_id eshte i detyrueshem");
        }

        let now = now_iso();
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;

        let sale = tx
      .query_row(
        "SELECT id, client_id, date, total, notes, fiscalized, fiscalized_at, created_at, updated_at, deleted
         FROM sales WHERE id=?1",
        params![sale_id],
        |r| {
          Ok(Sale {
            id: r.get(0)?,
            client_id: r.get(1)?,
            date: r.get(2)?,
            total: r.get(3)?,
            notes: r.get(4)?,
            fiscalized: r.get(5)?,
            fiscalized_at: r.get(6)?,
            created_at: r.get(7)?,
            updated_at: r.get(8)?,
            deleted: r.get(9)?,
          })
        },
      )
      .optional()?
      .ok_or_else(|| anyhow!("fatura nuk u gjet"))?;

        if sale.deleted != 0 {
            bail!("fatura eshte e fshire");
        }
        if sale.fiscalized == 1 {
            bail!("fatura eshte fiskalizuar tashme");
        }

        let mut items: Vec<VisitItem> = Vec::new();
        {
            let mut stmt = tx.prepare(
        "SELECT id, visit_id, client_id, tooth, title, qty, unit_price, fiscal, vat_code, fiscalized, fiscalized_at, notes, created_at, updated_at, deleted
         FROM visit_items
         WHERE visit_id=?1 AND deleted=0 AND fiscalized=0
         ORDER BY created_at ASC",
      )?;
            let rows = stmt.query_map(params![sale_id], |row| {
                Ok(VisitItem {
                    id: row.get(0)?,
                    visit_id: row.get(1)?,
                    client_id: row.get(2)?,
                    tooth: row.get(3)?,
                    title: row.get(4)?,
                    qty: row.get(5)?,
                    unit_price: row.get(6)?,
                    fiscal: row.get(7)?,
                    vat_code: row.get(8)?,
                    fiscalized: row.get(9)?,
                    fiscalized_at: row.get(10)?,
                    notes: row.get(11)?,
                    created_at: row.get(12)?,
                    updated_at: row.get(13)?,
                    deleted: row.get(14)?,
                })
            })?;
            for r in rows {
                items.push(r?);
            }
        }

        for it in &mut items {
            tx.execute(
                "UPDATE visit_items SET fiscal=0, updated_at=?2 WHERE id=?1",
                params![&it.id, &now],
            )?;
            it.fiscal = 0;
            it.updated_at = now.clone();
            let payload = serde_json::to_string(&it)?;
            Self::queue_replace_pending_tx(&tx, "visit_items", &it.id, "upsert", &payload, &now)?;
        }

        let mut updated_sale = sale.clone();
        if let Some(r) = reason {
            let rr = r.trim();
            if !rr.is_empty() {
                let suffix = format!("[NON-FISCAL-MANUAL] {}", rr);
                let merged = match updated_sale
                    .notes
                    .as_deref()
                    .map(str::trim)
                    .filter(|x| !x.is_empty())
                {
                    Some(base) => format!("{base}\n{suffix}"),
                    None => suffix,
                };
                tx.execute(
                    "UPDATE sales SET notes=?2, updated_at=?3 WHERE id=?1",
                    params![&updated_sale.id, &merged, &now],
                )?;
                updated_sale.notes = Some(merged);
            } else {
                tx.execute(
                    "UPDATE sales SET updated_at=?2 WHERE id=?1",
                    params![&updated_sale.id, &now],
                )?;
            }
        } else {
            tx.execute(
                "UPDATE sales SET updated_at=?2 WHERE id=?1",
                params![&updated_sale.id, &now],
            )?;
        }
        updated_sale.updated_at = now.clone();

        let payload = serde_json::to_string(&updated_sale)?;
        Self::queue_replace_pending_tx(&tx, "sales", &updated_sale.id, "upsert", &payload, &now)?;

        tx.commit()?;
        Ok(())
    }

    pub fn payments_list(
        &self,
        filters: Option<PaymentsListFilters>,
    ) -> anyhow::Result<Vec<Payment>> {
        let f = filters.unwrap_or_default();
        let include_deleted = f.include_deleted.unwrap_or(false);

        let mut sql = String::from(
      "SELECT id, client_id, sale_id, date, amount, method, notes, created_at, updated_at, deleted
       FROM payments WHERE 1=1",
    );
        let mut args: Vec<rusqlite::types::Value> = Vec::new();

        if !include_deleted {
            sql.push_str(" AND deleted = 0");
        }
        if let Some(cid) = f.client_id.filter(|x| !x.trim().is_empty()) {
            sql.push_str(" AND client_id = ?1");
            args.push(cid.into());
        }
        if let Some(sid) = f.sale_id.filter(|x| !x.trim().is_empty()) {
            sql.push_str(&format!(" AND sale_id = ?{}", args.len() + 1));
            args.push(sid.into());
        }
        if let Some(d) = f.date_from.filter(|x| !x.trim().is_empty()) {
            sql.push_str(&format!(" AND date >= ?{}", args.len() + 1));
            args.push(d.into());
        }
        if let Some(d) = f.date_to.filter(|x| !x.trim().is_empty()) {
            sql.push_str(&format!(" AND date <= ?{}", args.len() + 1));
            args.push(d.into());
        }

        sql.push_str(" ORDER BY date DESC, updated_at DESC LIMIT 2000");

        let conn = self.conn()?;
        let mut stmt = conn.prepare(&sql)?;
        let mut out = Vec::new();
        let rows = stmt.query_map(rusqlite::params_from_iter(args.iter()), |row| {
            Ok(Payment {
                id: row.get(0)?,
                client_id: row.get(1)?,
                sale_id: row.get(2)?,
                date: row.get(3)?,
                amount: row.get(4)?,
                method: row
                    .get::<_, Option<String>>(5)?
                    .unwrap_or_else(|| "cash".to_string()),
                notes: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
                deleted: row.get(9)?,
            })
        })?;
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn payments_get(&self, id: &str) -> anyhow::Result<Option<Payment>> {
        let conn = self.conn()?;
        Ok(
      conn
        .query_row(
          "SELECT id, client_id, sale_id, date, amount, method, notes, created_at, updated_at, deleted FROM payments WHERE id=?1",
          params![id],
          |row| {
            Ok(Payment {
              id: row.get(0)?,
              client_id: row.get(1)?,
              sale_id: row.get(2)?,
              date: row.get(3)?,
              amount: row.get(4)?,
              method: row.get::<_, Option<String>>(5)?.unwrap_or_else(|| "cash".to_string()),
              notes: row.get(6)?,
              created_at: row.get(7)?,
              updated_at: row.get(8)?,
              deleted: row.get(9)?,
            })
          },
        )
        .optional()?,
    )
    }

    pub fn payments_upsert(&self, input: PaymentUpsertInput) -> anyhow::Result<Payment> {
        if input.client_id.trim().is_empty() {
            bail!("client_id is required");
        }
        if !input.amount.is_finite() || input.amount < 0.0 {
            bail!("amount must be a finite number >= 0");
        }
        let method = input.method.trim().to_lowercase();
        if method != "cash" && method != "card" && method != "bank" && method != "other" {
            bail!("method must be one of: cash, card, bank, other");
        }

        let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let client_id = input.client_id;
        let sale_id = input.sale_id;
        let date = input.date;
        let amount = input.amount;
        let notes = input.notes;
        let now = now_iso();

        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        let existing_created_at: Option<String> = tx
            .query_row(
                "SELECT created_at FROM payments WHERE id=?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?;
        let created_at = existing_created_at.unwrap_or_else(|| now.clone());

        tx.execute(
      "INSERT INTO payments (id, client_id, sale_id, date, amount, method, notes, created_at, updated_at, deleted)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0)
       ON CONFLICT(id) DO UPDATE SET
         client_id=excluded.client_id,
         sale_id=excluded.sale_id,
         date=excluded.date,
         amount=excluded.amount,
         method=excluded.method,
         notes=excluded.notes,
         updated_at=excluded.updated_at,
         deleted=excluded.deleted",
      params![
        id,
        &client_id,
        &sale_id,
        &date,
        amount,
        &method,
        &notes,
        &created_at,
        &now
      ],
    )?;
        let row = Payment {
            id: id.clone(),
            client_id,
            sale_id,
            date,
            amount,
            method,
            notes,
            created_at: created_at.clone(),
            updated_at: now.clone(),
            deleted: 0,
        };
        let payload = serde_json::to_string(&row)?;
        Self::queue_replace_pending_tx(&tx, "payments", &row.id, "upsert", &payload, &now)?;
        tx.commit()?;
        Ok(row)
    }

    pub fn payments_delete(&self, id: &str) -> anyhow::Result<()> {
        if id.trim().is_empty() {
            bail!("payment id is required");
        }
        let now = now_iso();
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE payments SET deleted=1, updated_at=?2 WHERE id=?1",
            params![id, now],
        )?;
        let row = tx
      .query_row(
        "SELECT id, client_id, sale_id, date, amount, method, notes, created_at, updated_at, deleted FROM payments WHERE id=?1",
        params![id],
        |r| {
          Ok(Payment {
            id: r.get(0)?,
            client_id: r.get(1)?,
            sale_id: r.get(2)?,
            date: r.get(3)?,
            amount: r.get(4)?,
            method: r.get::<_, Option<String>>(5)?.unwrap_or_else(|| "cash".to_string()),
            notes: r.get(6)?,
            created_at: r.get(7)?,
            updated_at: r.get(8)?,
            deleted: r.get(9)?,
          })
        },
      )
      .optional()?
      .ok_or_else(|| anyhow!("payment not found"))?;
        let payload = serde_json::to_string(&row)?;
        Self::queue_replace_pending_tx(&tx, "payments", &row.id, "delete", &payload, &now)?;
        tx.commit()?;
        Ok(())
    }

    pub fn doctors_get(&self, id: &str) -> anyhow::Result<Option<Doctor>> {
        let conn = self.conn()?;
        Ok(
      conn
        .query_row(
          "SELECT id, code, name, title, specialty, phone, email, notes, created_at, updated_at, deleted FROM doctors WHERE id=?1",
          params![id],
          |row| {
            Ok(Doctor {
              id: row.get(0)?,
              code: row.get(1)?,
              name: row.get(2)?,
              title: row.get(3)?,
              specialty: row.get(4)?,
              phone: row.get(5)?,
              email: row.get(6)?,
              notes: row.get(7)?,
              created_at: row.get(8)?,
              updated_at: row.get(9)?,
              deleted: row.get(10)?,
            })
          },
        )
        .optional()?,
    )
    }

    pub fn doctors_get_by_code(&self, code: &str) -> anyhow::Result<Option<Doctor>> {
        let code = code.trim();
        if code.is_empty() {
            return Ok(None);
        }
        let conn = self.conn()?;
        Ok(
      conn
        .query_row(
          "SELECT id, code, name, title, specialty, phone, email, notes, created_at, updated_at, deleted
           FROM doctors
           WHERE deleted = 0 AND LOWER(COALESCE(code,'')) = LOWER(?1)
           LIMIT 1",
          params![code],
          |row| {
            Ok(Doctor {
              id: row.get(0)?,
              code: row.get(1)?,
              name: row.get(2)?,
              title: row.get(3)?,
              specialty: row.get(4)?,
              phone: row.get(5)?,
              email: row.get(6)?,
              notes: row.get(7)?,
              created_at: row.get(8)?,
              updated_at: row.get(9)?,
              deleted: row.get(10)?,
            })
          },
        )
        .optional()?,
    )
    }

    pub fn doctor_id_from_code_or_id(&self, code_or_id: &str) -> anyhow::Result<Option<String>> {
        let s = code_or_id.trim();
        if s.is_empty() {
            return Ok(None);
        }
        if let Some(d) = self.doctors_get(s)?.filter(|d| d.deleted == 0) {
            return Ok(Some(d.id));
        }
        Ok(self.doctors_get_by_code(s)?.map(|d| d.id))
    }

    pub fn doctor_account_get(
        &self,
        doctor_id: &str,
    ) -> anyhow::Result<Option<(String, String, bool)>> {
        let conn = self.conn()?;
        Ok(conn
            .query_row(
                "SELECT salt, password_hash, is_admin FROM doctor_accounts WHERE doctor_id=?1",
                params![doctor_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)? == 1,
                    ))
                },
            )
            .optional()?)
    }

    pub fn doctor_account_set(
        &self,
        doctor_id: &str,
        salt: &str,
        password_hash: &str,
        is_admin: bool,
        now: &str,
    ) -> anyhow::Result<()> {
        let clinic_id = self.setting_get("clinic_id")?.unwrap_or_default();
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        tx.execute(
      "INSERT INTO doctor_accounts (doctor_id, clinic_id, salt, password_hash, is_admin, created_at, updated_at, deleted)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)
       ON CONFLICT(doctor_id) DO UPDATE SET
         clinic_id=excluded.clinic_id,
         salt=excluded.salt,
         password_hash=excluded.password_hash,
         is_admin=excluded.is_admin,
         updated_at=excluded.updated_at,
         deleted=0",
      params![doctor_id, &clinic_id, salt, password_hash, if is_admin { 1 } else { 0 }, now, now],
    )?;
        let row = DoctorAccount {
            doctor_id: doctor_id.to_string(),
            clinic_id: Some(clinic_id),
            salt: salt.to_string(),
            password_hash: password_hash.to_string(),
            is_admin: if is_admin { 1 } else { 0 },
            created_at: now.to_string(),
            updated_at: now.to_string(),
            deleted: 0,
        };
        let payload = serde_json::to_string(&row)?;
        Self::queue_replace_pending_tx(&tx, "doctor_accounts", &row.doctor_id, "upsert", &payload, now)?;
        tx.commit()?;
        Ok(())
    }

    pub fn doctor_account_delete(&self, doctor_id: &str) -> anyhow::Result<()> {
        let now = now_iso();
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE doctor_accounts SET deleted=1, updated_at=?2 WHERE doctor_id=?1",
            params![doctor_id, now],
        )?;
        // Queue delete for sync
        let row = tx.query_row(
            "SELECT doctor_id, clinic_id, salt, password_hash, is_admin, created_at, updated_at, deleted FROM doctor_accounts WHERE doctor_id=?1",
            params![doctor_id],
            |r| Ok(DoctorAccount {
                doctor_id: r.get(0)?,
                clinic_id: r.get(1)?,
                salt: r.get(2)?,
                password_hash: r.get(3)?,
                is_admin: r.get(4)?,
                created_at: r.get(5)?,
                updated_at: r.get(6)?,
                deleted: r.get(7)?,
            })
        ).optional()?.ok_or_else(|| anyhow!("doctor account not found"))?;
        
        let payload = serde_json::to_string(&row)?;
        Self::queue_replace_pending_tx(&tx, "doctor_accounts", &row.doctor_id, "delete", &payload, &now)?;
        tx.commit()?;
        Ok(())
    }

    pub fn doctors_list(&self, search: Option<String>) -> anyhow::Result<Vec<Doctor>> {
        let conn = self.conn()?;
        let mut out = Vec::new();

        if let Some(s) = search.filter(|x| !x.trim().is_empty()) {
            let like = format!("%{}%", s.trim());
            let mut stmt = conn.prepare(
        "SELECT id, code, name, title, specialty, phone, email, notes, created_at, updated_at, deleted
         FROM doctors
         WHERE deleted = 0 AND (name LIKE ?1 OR phone LIKE ?1 OR email LIKE ?1 OR COALESCE(code,'') LIKE ?1)
         ORDER BY updated_at DESC
         LIMIT 1000",
      )?;
            let rows = stmt.query_map(params![like], |row| {
                Ok(Doctor {
                    id: row.get(0)?,
                    code: row.get(1)?,
                    name: row.get(2)?,
                    title: row.get(3)?,
                    specialty: row.get(4)?,
                    phone: row.get(5)?,
                    email: row.get(6)?,
                    notes: row.get(7)?,
                    created_at: row.get(8)?,
                    updated_at: row.get(9)?,
                    deleted: row.get(10)?,
                })
            })?;
            for r in rows {
                out.push(r?);
            }
            return Ok(out);
        }

        let mut stmt = conn.prepare(
      "SELECT id, code, name, title, specialty, phone, email, notes, created_at, updated_at, deleted
       FROM doctors
       WHERE deleted = 0
       ORDER BY updated_at DESC
       LIMIT 1000",
    )?;
        let rows = stmt.query_map([], |row| {
            Ok(Doctor {
                id: row.get(0)?,
                code: row.get(1)?,
                name: row.get(2)?,
                title: row.get(3)?,
                specialty: row.get(4)?,
                phone: row.get(5)?,
                email: row.get(6)?,
                notes: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
                deleted: row.get(10)?,
            })
        })?;
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn doctors_login_options(&self) -> anyhow::Result<Vec<crate::models::DoctorLoginOption>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT d.id, d.code, d.name, d.title, d.specialty,
              CASE WHEN a.doctor_id IS NULL THEN 0 ELSE 1 END AS has_account,
              COALESCE(a.is_admin, 0) AS is_admin
       FROM doctors d
       LEFT JOIN doctor_accounts a ON a.doctor_id = d.id
       WHERE d.deleted = 0
       ORDER BY d.name ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(crate::models::DoctorLoginOption {
                id: row.get(0)?,
                code: row.get(1)?,
                name: row.get(2)?,
                title: row.get(3)?,
                specialty: row.get(4)?,
                has_account: row.get::<_, i64>(5)? == 1,
                is_admin: row.get::<_, i64>(6)? == 1,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn doctors_upsert(&self, input: DoctorUpsertInput) -> anyhow::Result<Doctor> {
        let name = input.name.trim();
        if name.is_empty() {
            bail!("doctor name is required");
        }
        let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let code = input
            .code
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty());
        let title = input
            .title
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty());
        let specialty = input
            .specialty
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty());
        let phone = input.phone;
        let email = input.email;
        let notes = input.notes;
        let now = now_iso();

        let mut conn = self.conn()?;
        let tx = conn.transaction()?;

        // Ensure code uniqueness among non-deleted doctors.
        if let Some(c) = code.as_deref() {
            let existing: Option<String> = tx
        .query_row(
          "SELECT id FROM doctors WHERE deleted = 0 AND LOWER(COALESCE(code,'')) = LOWER(?1) AND id <> ?2 LIMIT 1",
          params![c, &id],
          |r| r.get(0),
        )
        .optional()?;
            if existing.is_some() {
                bail!("kodi i mjekut ekziston. perdor nje kod tjeter");
            }
        }

        let existing_created_at: Option<String> = tx
            .query_row(
                "SELECT created_at FROM doctors WHERE id=?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?;
        let created_at = existing_created_at.unwrap_or_else(|| now.clone());

        tx.execute(
      "INSERT INTO doctors (id, code, name, title, specialty, phone, email, notes, created_at, updated_at, deleted)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0)
       ON CONFLICT(id) DO UPDATE SET
         code=excluded.code,
         name=excluded.name,
         title=excluded.title,
         specialty=excluded.specialty,
         phone=excluded.phone,
         email=excluded.email,
         notes=excluded.notes,
         updated_at=excluded.updated_at,
         deleted=excluded.deleted",
      params![
        id,
        &code,
        name,
        &title,
        &specialty,
        &phone,
        &email,
        &notes,
        &created_at,
        &now
      ],
    )?;
        let row = Doctor {
            id: id.clone(),
            code,
            name: name.to_string(),
            title,
            specialty,
            phone,
            email,
            notes,
            created_at: created_at.clone(),
            updated_at: now.clone(),
            deleted: 0,
        };
        let payload = serde_json::to_string(&row)?;
        Self::queue_replace_pending_tx(&tx, "doctors", &row.id, "upsert", &payload, &now)?;
        tx.commit()?;
        Ok(row)
    }

    pub fn doctors_delete(&self, id: &str) -> anyhow::Result<()> {
        if id.trim().is_empty() {
            bail!("doctor id is required");
        }
        let now = now_iso();
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE doctors SET deleted=1, updated_at=?2 WHERE id=?1",
            params![id, now],
        )?;
        // Also soft-delete the account if it exists AND queue for sync
        if let Some(mut row) = tx.query_row(
            "SELECT doctor_id, clinic_id, salt, password_hash, is_admin, created_at, updated_at, deleted FROM doctor_accounts WHERE doctor_id=?1 AND deleted = 0",
            params![id],
            |r| Ok(DoctorAccount {
                doctor_id: r.get(0)?,
                clinic_id: r.get(1)?,
                salt: r.get(2)?,
                password_hash: r.get(3)?,
                is_admin: r.get(4)?,
                created_at: r.get(5)?,
                updated_at: r.get(6)?,
                deleted: r.get(7)?,
            })
        ).optional()? {
            tx.execute("UPDATE doctor_accounts SET deleted=1, updated_at=?2 WHERE doctor_id=?1", params![id, now])?;
            row.deleted = 1;
            row.updated_at = now.to_string();
            let payload = serde_json::to_string(&row)?;
            Self::queue_replace_pending_tx(&tx, "doctor_accounts", &row.doctor_id, "delete", &payload, &now)?;
        }
        let row = tx
      .query_row(
        "SELECT id, code, name, title, specialty, phone, email, notes, created_at, updated_at, deleted FROM doctors WHERE id=?1",
        params![id],
        |r| {
          Ok(Doctor {
            id: r.get(0)?,
            code: r.get(1)?,
            name: r.get(2)?,
            title: r.get(3)?,
            specialty: r.get(4)?,
            phone: r.get(5)?,
            email: r.get(6)?,
            notes: r.get(7)?,
            created_at: r.get(8)?,
            updated_at: r.get(9)?,
            deleted: r.get(10)?,
          })
        },
      )
      .optional()?
      .ok_or_else(|| anyhow!("doctor not found"))?;
        let payload = serde_json::to_string(&row)?;
        Self::queue_replace_pending_tx(&tx, "doctors", &row.id, "delete", &payload, &now)?;
        tx.commit()?;
        Ok(())
    }

    pub fn services_list(&self, search: Option<String>) -> anyhow::Result<Vec<Service>> {
        let conn = self.conn()?;
        let mut out = Vec::new();

        if let Some(s) = search.filter(|x| !x.trim().is_empty()) {
            let like = format!("%{}%", s.trim());
            let mut stmt = conn.prepare(
                "SELECT id, title, default_price, vat_code, notes, created_at, updated_at, deleted
         FROM services
         WHERE deleted = 0 AND (title LIKE ?1 OR notes LIKE ?1)
         ORDER BY updated_at DESC
         LIMIT 2000",
            )?;
            let rows = stmt.query_map(params![like], |row| {
                Ok(Service {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    default_price: row.get(2)?,
                    vat_code: row.get(3)?,
                    notes: row.get(4)?,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                    deleted: row.get(7)?,
                })
            })?;
            for r in rows {
                out.push(r?);
            }
            return Ok(out);
        }

        let mut stmt = conn.prepare(
            "SELECT id, title, default_price, vat_code, notes, created_at, updated_at, deleted
       FROM services
       WHERE deleted = 0
       ORDER BY updated_at DESC
       LIMIT 2000",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Service {
                id: row.get(0)?,
                title: row.get(1)?,
                default_price: row.get(2)?,
                vat_code: row.get(3)?,
                notes: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
                deleted: row.get(7)?,
            })
        })?;
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn services_upsert(&self, input: ServiceUpsertInput) -> anyhow::Result<Service> {
        let title = input.title.trim();
        if title.is_empty() {
            bail!("service title is required");
        }
        if !input.default_price.is_finite() || input.default_price < 0.0 {
            bail!("default_price must be a finite number >= 0");
        }
        let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let default_price = input.default_price;
        let vat_registered = self
            .setting_get("clinic_vat_registered")
            .unwrap_or(None)
            .unwrap_or_default()
            .trim()
            == "1";

        let mut vat_code = input
            .vat_code
            .unwrap_or_else(|| "".to_string())
            .trim()
            .to_uppercase();
        if vat_registered {
            // VAT registered: A (0% exempt), D (8%), E (18%).
            if vat_code.is_empty() {
                vat_code = "A".to_string();
            }
            if vat_code == "C" {
                // Convert legacy "not VAT" into 0% exempt.
                vat_code = "A".to_string();
            }
            match vat_code.as_str() {
                "A" | "D" | "E" => {}
                _ => bail!("vat_code duhet te jete A, D ose E (kur biznesi eshte ne TVSH)"),
            }
        } else {
            // Not VAT registered: always "C" (0% - not in VAT).
            vat_code = "C".to_string();
        }
        let notes = input.notes;
        let now = now_iso();

        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        let existing_created_at: Option<String> = tx
            .query_row(
                "SELECT created_at FROM services WHERE id=?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?;
        let created_at = existing_created_at.unwrap_or_else(|| now.clone());

        tx.execute(
      "INSERT INTO services (id, title, default_price, vat_code, notes, created_at, updated_at, deleted)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)
       ON CONFLICT(id) DO UPDATE SET
         title=excluded.title,
         default_price=excluded.default_price,
         vat_code=excluded.vat_code,
         notes=excluded.notes,
         updated_at=excluded.updated_at,
         deleted=excluded.deleted",
      params![id, title, default_price, &vat_code, &notes, &created_at, &now],
    )?;
        let row = Service {
            id: id.clone(),
            title: title.to_string(),
            default_price,
            vat_code,
            notes,
            created_at: created_at.clone(),
            updated_at: now.clone(),
            deleted: 0,
        };
        let payload = serde_json::to_string(&row)?;
        Self::queue_replace_pending_tx(&tx, "services", &row.id, "upsert", &payload, &now)?;
        tx.commit()?;
        Ok(row)
    }

    pub fn services_delete(&self, id: &str) -> anyhow::Result<()> {
        if id.trim().is_empty() {
            bail!("service id is required");
        }
        let now = now_iso();
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE services SET deleted=1, updated_at=?2 WHERE id=?1",
            params![id, now],
        )?;
        let row = tx
      .query_row(
        "SELECT id, title, default_price, vat_code, notes, created_at, updated_at, deleted FROM services WHERE id=?1",
        params![id],
        |r| {
          Ok(Service {
            id: r.get(0)?,
            title: r.get(1)?,
            default_price: r.get(2)?,
            vat_code: r.get(3)?,
            notes: r.get(4)?,
            created_at: r.get(5)?,
            updated_at: r.get(6)?,
            deleted: r.get(7)?,
          })
        },
      )
      .optional()?
      .ok_or_else(|| anyhow!("service not found"))?;
        let payload = serde_json::to_string(&row)?;
        Self::queue_replace_pending_tx(&tx, "services", &row.id, "delete", &payload, &now)?;
        tx.commit()?;
        Ok(())
    }

    pub fn appointments_list(
        &self,
        filters: Option<AppointmentsListFilters>,
    ) -> anyhow::Result<Vec<Appointment>> {
        let f = filters.unwrap_or_default();
        let include_deleted = f.include_deleted.unwrap_or(false);

        let mut sql = String::from(
      "SELECT id, client_id, doctor_id, start_at, end_at, status, notes, created_at, updated_at, deleted
       FROM appointments WHERE 1=1",
    );
        let mut args: Vec<rusqlite::types::Value> = Vec::new();

        if !include_deleted {
            sql.push_str(" AND deleted = 0");
        }
        if let Some(cid) = f.client_id.filter(|x| !x.trim().is_empty()) {
            sql.push_str(&format!(" AND client_id = ?{}", args.len() + 1));
            args.push(cid.into());
        }
        if let Some(did) = f.doctor_id.filter(|x| !x.trim().is_empty()) {
            sql.push_str(&format!(" AND doctor_id = ?{}", args.len() + 1));
            args.push(did.into());
        }
        if let Some(s) = f.status.filter(|x| !x.trim().is_empty()) {
            sql.push_str(&format!(" AND status = ?{}", args.len() + 1));
            args.push(s.into());
        }
        if let Some(d) = f.start_from.filter(|x| !x.trim().is_empty()) {
            sql.push_str(&format!(" AND start_at >= ?{}", args.len() + 1));
            args.push(d.into());
        }
        if let Some(d) = f.start_to.filter(|x| !x.trim().is_empty()) {
            sql.push_str(&format!(" AND start_at <= ?{}", args.len() + 1));
            args.push(d.into());
        }

        sql.push_str(" ORDER BY start_at ASC, updated_at DESC LIMIT 5000");

        let conn = self.conn()?;
        let mut stmt = conn.prepare(&sql)?;
        let mut out = Vec::new();
        let rows = stmt.query_map(rusqlite::params_from_iter(args.iter()), |row| {
            Ok(Appointment {
                id: row.get(0)?,
                client_id: row.get(1)?,
                doctor_id: row.get(2)?,
                start_at: row.get(3)?,
                end_at: row.get(4)?,
                status: row.get(5)?,
                notes: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
                deleted: row.get(9)?,
            })
        })?;
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn appointments_get(&self, id: &str) -> anyhow::Result<Option<Appointment>> {
        let conn = self.conn()?;
        Ok(
      conn
        .query_row(
          "SELECT id, client_id, doctor_id, start_at, end_at, status, notes, created_at, updated_at, deleted
           FROM appointments WHERE id=?1",
          params![id],
          |row| {
            Ok(Appointment {
              id: row.get(0)?,
              client_id: row.get(1)?,
              doctor_id: row.get(2)?,
              start_at: row.get(3)?,
              end_at: row.get(4)?,
              status: row.get(5)?,
              notes: row.get(6)?,
              created_at: row.get(7)?,
              updated_at: row.get(8)?,
              deleted: row.get(9)?,
            })
          },
        )
        .optional()?,
    )
    }

    pub fn appointments_upsert(
        &self,
        input: AppointmentUpsertInput,
    ) -> anyhow::Result<Appointment> {
        if input.client_id.trim().is_empty() {
            bail!("client_id is required");
        }
        if input.start_at.trim().is_empty() {
            bail!("start_at is required");
        }
        let status = input.status.trim().to_lowercase();
        if status != "scheduled" && status != "done" && status != "cancelled" {
            bail!("status must be one of: scheduled, done, cancelled");
        }
        let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let client_id = input.client_id;
        let doctor_id = input.doctor_id;
        let start_at = input.start_at;
        let end_at = input.end_at;
        let notes = input.notes;
        let now = now_iso();

        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        let existing_created_at: Option<String> = tx
            .query_row(
                "SELECT created_at FROM appointments WHERE id=?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?;
        let created_at = existing_created_at.unwrap_or_else(|| now.clone());

        tx.execute(
      "INSERT INTO appointments (id, client_id, doctor_id, start_at, end_at, status, notes, created_at, updated_at, deleted)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0)
       ON CONFLICT(id) DO UPDATE SET
         client_id=excluded.client_id,
         doctor_id=excluded.doctor_id,
         start_at=excluded.start_at,
         end_at=excluded.end_at,
         status=excluded.status,
         notes=excluded.notes,
         updated_at=excluded.updated_at,
         deleted=excluded.deleted",
      params![id, &client_id, &doctor_id, &start_at, &end_at, &status, &notes, &created_at, &now],
    )?;
        let row = Appointment {
            id: id.clone(),
            client_id,
            doctor_id,
            start_at,
            end_at,
            status,
            notes,
            created_at: created_at.clone(),
            updated_at: now.clone(),
            deleted: 0,
        };
        let payload = serde_json::to_string(&row)?;
        Self::queue_replace_pending_tx(&tx, "appointments", &row.id, "upsert", &payload, &now)?;
        tx.commit()?;
        Ok(row)
    }

    pub fn appointments_delete(&self, id: &str) -> anyhow::Result<()> {
        if id.trim().is_empty() {
            bail!("appointment id is required");
        }
        let now = now_iso();
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE appointments SET deleted=1, updated_at=?2 WHERE id=?1",
            params![id, now],
        )?;
        let row = tx
      .query_row(
        "SELECT id, client_id, doctor_id, start_at, end_at, status, notes, created_at, updated_at, deleted
         FROM appointments WHERE id=?1",
        params![id],
        |r| {
          Ok(Appointment {
            id: r.get(0)?,
            client_id: r.get(1)?,
            doctor_id: r.get(2)?,
            start_at: r.get(3)?,
            end_at: r.get(4)?,
            status: r.get(5)?,
            notes: r.get(6)?,
            created_at: r.get(7)?,
            updated_at: r.get(8)?,
            deleted: r.get(9)?,
          })
        },
      )
      .optional()?
      .ok_or_else(|| anyhow!("appointment not found"))?;
        let payload = serde_json::to_string(&row)?;
        Self::queue_replace_pending_tx(&tx, "appointments", &row.id, "delete", &payload, &now)?;
        tx.commit()?;
        Ok(())
    }

    pub fn visits_list(&self, filters: Option<VisitsListFilters>) -> anyhow::Result<Vec<Visit>> {
        let f = filters.unwrap_or_default();
        let include_deleted = f.include_deleted.unwrap_or(false);

        let mut sql = String::from(
      "SELECT id, client_id, doctor_id, date, visit_time, status, notes, body_weight, body_weight_unit, body_height, body_height_unit, head_circumference, head_circumference_unit, body_temperature, body_temperature_unit, blood_oxygen, blood_oxygen_unit, glycemia, glycemia_unit, pulse, pulse_unit, bmi, blood_pressure_systolic, blood_pressure_diastolic, blood_pressure_unit, complaints, additional_notes, controls, remarks, analyses, advice, therapies, diagnosis, examinations, created_at, updated_at, deleted, specialty_report
       FROM visits WHERE 1=1",
    );
        let mut args: Vec<rusqlite::types::Value> = Vec::new();

        if !include_deleted {
            sql.push_str(" AND deleted = 0");
        }
        if let Some(cid) = f.client_id.filter(|x| !x.trim().is_empty()) {
            sql.push_str(&format!(" AND client_id = ?{}", args.len() + 1));
            args.push(cid.into());
        }
        if let Some(did) = f.doctor_id.filter(|x| !x.trim().is_empty()) {
            sql.push_str(&format!(" AND doctor_id = ?{}", args.len() + 1));
            args.push(did.into());
        }
        if let Some(s) = f.status.filter(|x| !x.trim().is_empty()) {
            sql.push_str(&format!(" AND status = ?{}", args.len() + 1));
            args.push(s.into());
        }
        if let Some(d) = f.date_from.filter(|x| !x.trim().is_empty()) {
            sql.push_str(&format!(" AND date >= ?{}", args.len() + 1));
            args.push(d.into());
        }
        if let Some(d) = f.date_to.filter(|x| !x.trim().is_empty()) {
            sql.push_str(&format!(" AND date <= ?{}", args.len() + 1));
            args.push(d.into());
        }

        sql.push_str(" ORDER BY date DESC, updated_at DESC LIMIT 2000");

        let conn = self.conn()?;
        let mut stmt = conn.prepare(&sql)?;
        let mut out = Vec::new();
        let rows = stmt.query_map(rusqlite::params_from_iter(args.iter()), |row| {
            Ok(Visit {
                id: row.get(0)?,
                client_id: row.get(1)?,
                doctor_id: row.get(2)?,
                date: row.get(3)?,
                visit_time: row.get(4)?,
                status: row.get(5)?,
                notes: row.get(6)?,
                body_weight: row.get(7)?,
                body_weight_unit: row.get(8)?,
                body_height: row.get(9)?,
                body_height_unit: row.get(10)?,
                head_circumference: row.get(11)?,
                head_circumference_unit: row.get(12)?,
                body_temperature: row.get(13)?,
                body_temperature_unit: row.get(14)?,
                blood_oxygen: row.get(15)?,
                blood_oxygen_unit: row.get(16)?,
                glycemia: row.get(17)?,
                glycemia_unit: row.get(18)?,
                pulse: row.get(19)?,
                pulse_unit: row.get(20)?,
                bmi: row.get(21)?,
                blood_pressure_systolic: row.get(22)?,
                blood_pressure_diastolic: row.get(23)?,
                blood_pressure_unit: row.get(24)?,
                complaints: row.get(25)?,
                additional_notes: row.get(26)?,
                controls: row.get(27)?,
                remarks: row.get(28)?,
                analyses: row.get(29)?,
                advice: row.get(30)?,
                therapies: row.get(31)?,
                diagnosis: row.get(32)?,
                examinations: row.get(33)?,
                specialty_report: row.get(37)?,
                created_at: row.get(34)?,
                updated_at: row.get(35)?,
                deleted: row.get(36)?,
            })
        })?;
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn visits_get(&self, id: &str) -> anyhow::Result<Option<Visit>> {
        let conn = self.conn()?;
        Ok(
      conn
        .query_row(
          "SELECT id, client_id, doctor_id, date, visit_time, status, notes, body_weight, body_weight_unit, body_height, body_height_unit, head_circumference, head_circumference_unit, body_temperature, body_temperature_unit, blood_oxygen, blood_oxygen_unit, glycemia, glycemia_unit, pulse, pulse_unit, bmi, blood_pressure_systolic, blood_pressure_diastolic, blood_pressure_unit, complaints, additional_notes, controls, remarks, analyses, advice, therapies, diagnosis, examinations, created_at, updated_at, deleted, specialty_report
           FROM visits WHERE id=?1",
          params![id],
          |row| {
            Ok(Visit {
              id: row.get(0)?,
              client_id: row.get(1)?,
              doctor_id: row.get(2)?,
              date: row.get(3)?,
              visit_time: row.get(4)?,
              status: row.get(5)?,
              notes: row.get(6)?,
              body_weight: row.get(7)?,
              body_weight_unit: row.get(8)?,
              body_height: row.get(9)?,
              body_height_unit: row.get(10)?,
              head_circumference: row.get(11)?,
              head_circumference_unit: row.get(12)?,
              body_temperature: row.get(13)?,
              body_temperature_unit: row.get(14)?,
              blood_oxygen: row.get(15)?,
              blood_oxygen_unit: row.get(16)?,
              glycemia: row.get(17)?,
              glycemia_unit: row.get(18)?,
              pulse: row.get(19)?,
              pulse_unit: row.get(20)?,
              bmi: row.get(21)?,
              blood_pressure_systolic: row.get(22)?,
              blood_pressure_diastolic: row.get(23)?,
              blood_pressure_unit: row.get(24)?,
              complaints: row.get(25)?,
              additional_notes: row.get(26)?,
              controls: row.get(27)?,
              remarks: row.get(28)?,
              analyses: row.get(29)?,
              advice: row.get(30)?,
              therapies: row.get(31)?,
              diagnosis: row.get(32)?,
              examinations: row.get(33)?,
                specialty_report: row.get(37)?,
              created_at: row.get(34)?,
              updated_at: row.get(35)?,
              deleted: row.get(36)?,
            })
          },
        )
        .optional()?,
    )
    }

    pub fn visits_upsert(&self, input: VisitUpsertInput) -> anyhow::Result<Visit> {
        if input.client_id.trim().is_empty() {
            bail!("client_id is required");
        }
        let status = input.status.trim().to_lowercase();
        if status != "draft" && status != "final" {
            bail!("status must be one of: draft, final");
        }
        let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let client_id = input.client_id;
        let doctor_id = input.doctor_id;
        let date = input.date;
        let visit_time = input.visit_time;
        let notes = input.notes;
        let body_weight = input.body_weight;
        let body_weight_unit = input.body_weight_unit;
        let body_height = input.body_height;
        let body_height_unit = input.body_height_unit;
        let head_circumference = input.head_circumference;
        let head_circumference_unit = input.head_circumference_unit;
        let body_temperature = input.body_temperature;
        let body_temperature_unit = input.body_temperature_unit;
        let blood_oxygen = input.blood_oxygen;
        let blood_oxygen_unit = input.blood_oxygen_unit;
        let glycemia = input.glycemia;
        let glycemia_unit = input.glycemia_unit;
        let pulse = input.pulse;
        let pulse_unit = input.pulse_unit;
        let bmi = input.bmi;
        let blood_pressure_systolic = input.blood_pressure_systolic;
        let blood_pressure_diastolic = input.blood_pressure_diastolic;
        let blood_pressure_unit = input.blood_pressure_unit;
        let complaints = input.complaints;
        let additional_notes = input.additional_notes;
        let controls = input.controls;
        let remarks = input.remarks;
        let analyses = input.analyses;
        let advice = input.advice;
        let therapies = input.therapies;
        let diagnosis = input.diagnosis;
        let examinations = input.examinations;
        let specialty_report = input.specialty_report;
        let now = now_iso();

        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        let existing_created_at: Option<String> = tx
            .query_row(
                "SELECT created_at FROM visits WHERE id=?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?;
        let created_at = existing_created_at.unwrap_or_else(|| now.clone());

        tx.execute(
      "INSERT INTO visits (
         id, client_id, doctor_id, date, visit_time, status, notes,
         body_weight, body_weight_unit, body_height, body_height_unit,
         head_circumference, head_circumference_unit,
         body_temperature, body_temperature_unit,
         blood_oxygen, blood_oxygen_unit,
         glycemia, glycemia_unit,
         pulse, pulse_unit,
         bmi,
         blood_pressure_systolic, blood_pressure_diastolic, blood_pressure_unit,
         complaints, additional_notes, controls, remarks, analyses, advice, therapies, diagnosis, examinations,
         created_at, updated_at, deleted, specialty_report
       )
       VALUES (
         ?1, ?2, ?3, ?4, ?5, ?6, ?7,
         ?8, ?9, ?10, ?11,
         ?12, ?13,
         ?14, ?15,
         ?16, ?17,
         ?18, ?19,
         ?20, ?21,
         ?22,
         ?23, ?24, ?25,
         ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34,
         ?35, ?36, 0, ?37
       )
       ON CONFLICT(id) DO UPDATE SET
         client_id=excluded.client_id,
         doctor_id=excluded.doctor_id,
         date=excluded.date,
         visit_time=excluded.visit_time,
         status=excluded.status,
         notes=excluded.notes,
         body_weight=excluded.body_weight,
         body_weight_unit=excluded.body_weight_unit,
         body_height=excluded.body_height,
         body_height_unit=excluded.body_height_unit,
         head_circumference=excluded.head_circumference,
         head_circumference_unit=excluded.head_circumference_unit,
         body_temperature=excluded.body_temperature,
         body_temperature_unit=excluded.body_temperature_unit,
         blood_oxygen=excluded.blood_oxygen,
         blood_oxygen_unit=excluded.blood_oxygen_unit,
         glycemia=excluded.glycemia,
         glycemia_unit=excluded.glycemia_unit,
         pulse=excluded.pulse,
         pulse_unit=excluded.pulse_unit,
         bmi=excluded.bmi,
         blood_pressure_systolic=excluded.blood_pressure_systolic,
         blood_pressure_diastolic=excluded.blood_pressure_diastolic,
         blood_pressure_unit=excluded.blood_pressure_unit,
         complaints=excluded.complaints,
         additional_notes=excluded.additional_notes,
         controls=excluded.controls,
         remarks=excluded.remarks,
         analyses=excluded.analyses,
         advice=excluded.advice,
         therapies=excluded.therapies,
         diagnosis=excluded.diagnosis,
         examinations=excluded.examinations,
         specialty_report=excluded.specialty_report,
         updated_at=excluded.updated_at,
         deleted=excluded.deleted",
      params![
        id,
        &client_id,
        &doctor_id,
        &date,
        &visit_time,
        &status,
        &notes,
        &body_weight,
        &body_weight_unit,
        &body_height,
        &body_height_unit,
        &head_circumference,
        &head_circumference_unit,
        &body_temperature,
        &body_temperature_unit,
        &blood_oxygen,
        &blood_oxygen_unit,
        &glycemia,
        &glycemia_unit,
        &pulse,
        &pulse_unit,
        &bmi,
        &blood_pressure_systolic,
        &blood_pressure_diastolic,
        &blood_pressure_unit,
        &complaints,
        &additional_notes,
        &controls,
        &remarks,
        &analyses,
        &advice,
        &therapies,
        &diagnosis,
        &examinations,
        &created_at,
        &now,
        &specialty_report
      ],
    )?;
        let row = Visit {
            id: id.clone(),
            client_id,
            doctor_id,
            date,
            visit_time,
            status,
            notes,
            body_weight,
            body_weight_unit,
            body_height,
            body_height_unit,
            head_circumference,
            head_circumference_unit,
            body_temperature,
            body_temperature_unit,
            blood_oxygen,
            blood_oxygen_unit,
            glycemia,
            glycemia_unit,
            pulse,
            pulse_unit,
            bmi,
            blood_pressure_systolic,
            blood_pressure_diastolic,
            blood_pressure_unit,
            complaints,
            additional_notes,
            controls,
            remarks,
            analyses,
            advice,
            therapies,
            diagnosis,
            examinations,
            specialty_report,
            created_at: created_at.clone(),
            updated_at: now.clone(),
            deleted: 0,
        };
        let payload = serde_json::to_string(&row)?;
        Self::queue_replace_pending_tx(&tx, "visits", &row.id, "upsert", &payload, &now)?;
        tx.commit()?;
        Ok(row)
    }

    pub fn client_photos_list(&self, client_id: &str) -> anyhow::Result<Vec<ClientPhoto>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, client_id, stage, label, file_path, taken_at, created_at, deleted
             FROM client_photos WHERE client_id=?1 AND deleted=0 ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![client_id], |r| {
            Ok(ClientPhoto {
                id: r.get(0)?,
                client_id: r.get(1)?,
                stage: r.get(2)?,
                label: r.get(3)?,
                file_path: r.get(4)?,
                taken_at: r.get(5)?,
                created_at: r.get(6)?,
                deleted: r.get(7)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn client_photo_add(&self, row: &ClientPhoto) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO client_photos (id, client_id, stage, label, file_path, taken_at, created_at, deleted)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)",
            params![row.id, row.client_id, row.stage, row.label, row.file_path, row.taken_at, row.created_at],
        )?;
        Ok(())
    }

    pub fn client_photo_get(&self, id: &str) -> anyhow::Result<Option<ClientPhoto>> {
        let conn = self.conn()?;
        let r = conn
            .query_row(
                "SELECT id, client_id, stage, label, file_path, taken_at, created_at, deleted
                 FROM client_photos WHERE id=?1",
                params![id],
                |r| {
                    Ok(ClientPhoto {
                        id: r.get(0)?,
                        client_id: r.get(1)?,
                        stage: r.get(2)?,
                        label: r.get(3)?,
                        file_path: r.get(4)?,
                        taken_at: r.get(5)?,
                        created_at: r.get(6)?,
                        deleted: r.get(7)?,
                    })
                },
            )
            .optional()?;
        Ok(r)
    }

    pub fn client_photo_delete(&self, id: &str) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute("UPDATE client_photos SET deleted=1 WHERE id=?1", params![id])?;
        Ok(())
    }

    pub fn visits_delete(&self, id: &str) -> anyhow::Result<()> {
        if id.trim().is_empty() {
            bail!("visit id is required");
        }
        let now = now_iso();
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE visits SET deleted=1, updated_at=?2 WHERE id=?1",
            params![id, now],
        )?;
        let row = tx
      .query_row(
        "SELECT id, client_id, doctor_id, date, visit_time, status, notes, body_weight, body_weight_unit, body_height, body_height_unit, head_circumference, head_circumference_unit, body_temperature, body_temperature_unit, blood_oxygen, blood_oxygen_unit, glycemia, glycemia_unit, pulse, pulse_unit, bmi, blood_pressure_systolic, blood_pressure_diastolic, blood_pressure_unit, complaints, additional_notes, controls, remarks, analyses, advice, therapies, diagnosis, examinations, created_at, updated_at, deleted, specialty_report
         FROM visits WHERE id=?1",
        params![id],
        |r| {
          Ok(Visit {
            id: r.get(0)?,
            client_id: r.get(1)?,
            doctor_id: r.get(2)?,
            date: r.get(3)?,
            visit_time: r.get(4)?,
            status: r.get(5)?,
            notes: r.get(6)?,
            body_weight: r.get(7)?,
            body_weight_unit: r.get(8)?,
            body_height: r.get(9)?,
            body_height_unit: r.get(10)?,
            head_circumference: r.get(11)?,
            head_circumference_unit: r.get(12)?,
            body_temperature: r.get(13)?,
            body_temperature_unit: r.get(14)?,
            blood_oxygen: r.get(15)?,
            blood_oxygen_unit: r.get(16)?,
            glycemia: r.get(17)?,
            glycemia_unit: r.get(18)?,
            pulse: r.get(19)?,
            pulse_unit: r.get(20)?,
            bmi: r.get(21)?,
            blood_pressure_systolic: r.get(22)?,
            blood_pressure_diastolic: r.get(23)?,
            blood_pressure_unit: r.get(24)?,
            complaints: r.get(25)?,
            additional_notes: r.get(26)?,
            controls: r.get(27)?,
            remarks: r.get(28)?,
            analyses: r.get(29)?,
            advice: r.get(30)?,
            therapies: r.get(31)?,
            diagnosis: r.get(32)?,
            examinations: r.get(33)?,
            specialty_report: r.get(37)?,
            created_at: r.get(34)?,
            updated_at: r.get(35)?,
            deleted: r.get(36)?,
          })
        },
      )
      .optional()?
      .ok_or_else(|| anyhow!("visit not found"))?;
        let payload = serde_json::to_string(&row)?;
        Self::queue_replace_pending_tx(&tx, "visits", &row.id, "delete", &payload, &now)?;

        // Keep history consistent: removing a visit also removes its procedures.
        let mut linked_items: Vec<VisitItem> = Vec::new();
        {
            let mut stmt = tx.prepare(
        "SELECT id, visit_id, client_id, tooth, title, qty, unit_price, fiscal, vat_code, fiscalized, fiscalized_at, notes, created_at, updated_at, deleted
         FROM visit_items
         WHERE visit_id = ?1 AND deleted = 0",
      )?;
            let rows = stmt.query_map(params![id], |r| {
                Ok(VisitItem {
                    id: r.get(0)?,
                    visit_id: r.get(1)?,
                    client_id: r.get(2)?,
                    tooth: r.get(3)?,
                    title: r.get(4)?,
                    qty: r.get(5)?,
                    unit_price: r.get(6)?,
                    fiscal: r.get(7)?,
                    vat_code: r.get(8)?,
                    fiscalized: r.get(9)?,
                    fiscalized_at: r.get(10)?,
                    notes: r.get(11)?,
                    created_at: r.get(12)?,
                    updated_at: r.get(13)?,
                    deleted: r.get(14)?,
                })
            })?;
            for r in rows {
                linked_items.push(r?);
            }
        }
        for it in linked_items {
            tx.execute(
                "UPDATE visit_items SET deleted=1, updated_at=?2 WHERE id=?1",
                params![&it.id, &now],
            )?;
            let mut del = it.clone();
            del.deleted = 1;
            del.updated_at = now.clone();
            let it_payload = serde_json::to_string(&del)?;
            Self::queue_replace_pending_tx(
                &tx,
                "visit_items",
                &del.id,
                "delete",
                &it_payload,
                &now,
            )?;
        }

        // If visit has an invoice (same id), remove sale + linked payments as well.
        let linked_sale = tx
      .query_row(
        "SELECT id, client_id, date, total, notes, fiscalized, fiscalized_at, created_at, updated_at, deleted
         FROM sales
         WHERE id = ?1 AND deleted = 0
         LIMIT 1",
        params![id],
        |r| {
          Ok(Sale {
            id: r.get(0)?,
            client_id: r.get(1)?,
            date: r.get(2)?,
            total: r.get(3)?,
            notes: r.get(4)?,
            fiscalized: r.get(5)?,
            fiscalized_at: r.get(6)?,
            created_at: r.get(7)?,
            updated_at: r.get(8)?,
            deleted: r.get(9)?,
          })
        },
      )
      .optional()?;
        if let Some(s) = linked_sale {
            tx.execute(
                "UPDATE sales SET deleted=1, updated_at=?2 WHERE id=?1",
                params![&s.id, &now],
            )?;
            let mut s_del = s.clone();
            s_del.deleted = 1;
            s_del.updated_at = now.clone();
            let s_payload = serde_json::to_string(&s_del)?;
            Self::queue_replace_pending_tx(&tx, "sales", &s_del.id, "delete", &s_payload, &now)?;
        }

        let mut linked_payments: Vec<Payment> = Vec::new();
        {
            let mut stmt = tx.prepare(
        "SELECT id, client_id, sale_id, date, amount, method, notes, created_at, updated_at, deleted
         FROM payments
         WHERE sale_id = ?1 AND deleted = 0",
      )?;
            let rows = stmt.query_map(params![id], |r| {
                Ok(Payment {
                    id: r.get(0)?,
                    client_id: r.get(1)?,
                    sale_id: r.get(2)?,
                    date: r.get(3)?,
                    amount: r.get(4)?,
                    method: r
                        .get::<_, Option<String>>(5)?
                        .unwrap_or_else(|| "cash".to_string()),
                    notes: r.get(6)?,
                    created_at: r.get(7)?,
                    updated_at: r.get(8)?,
                    deleted: r.get(9)?,
                })
            })?;
            for r in rows {
                linked_payments.push(r?);
            }
        }
        for p in linked_payments {
            tx.execute(
                "UPDATE payments SET deleted=1, updated_at=?2 WHERE id=?1",
                params![&p.id, &now],
            )?;
            let mut del = p.clone();
            del.deleted = 1;
            del.updated_at = now.clone();
            let p_payload = serde_json::to_string(&del)?;
            Self::queue_replace_pending_tx(&tx, "payments", &del.id, "delete", &p_payload, &now)?;
        }

        tx.commit()?;
        Ok(())
    }

    pub fn history_reset_all(&self) -> anyhow::Result<()> {
        // Reset all history modules (offline-first): mark local rows deleted and queue deletes for cloud sync.
        let visit_ids: Vec<String> = self
            .visits_list(Some(VisitsListFilters {
                include_deleted: Some(false),
                ..Default::default()
            }))?
            .into_iter()
            .map(|x| x.id)
            .collect();
        for id in visit_ids {
            self.visits_delete(&id)?;
        }

        let sale_ids: Vec<String> = self
            .sales_list(Some(SalesListFilters {
                client_id: None,
                date_from: None,
                date_to: None,
                include_deleted: Some(false),
            }))?
            .into_iter()
            .map(|x| x.id)
            .collect();
        for id in sale_ids {
            self.sales_delete(&id)?;
        }

        let payment_ids: Vec<String> = self
            .payments_list(Some(PaymentsListFilters {
                include_deleted: Some(false),
                ..Default::default()
            }))?
            .into_iter()
            .map(|x| x.id)
            .collect();
        for id in payment_ids {
            self.payments_delete(&id)?;
        }

        let appointment_ids: Vec<String> = self
            .appointments_list(Some(AppointmentsListFilters {
                client_id: None,
                doctor_id: None,
                start_from: None,
                start_to: None,
                status: None,
                include_deleted: Some(false),
            }))?
            .into_iter()
            .map(|x| x.id)
            .collect();
        for id in appointment_ids {
            self.appointments_delete(&id)?;
        }

        // Any orphan procedure rows not covered by visit cascade.
        let orphan_item_ids: Vec<String> = self
            .visit_items_list(Some(VisitItemsListFilters {
                visit_id: None,
                client_id: None,
                include_deleted: Some(false),
            }))?
            .into_iter()
            .map(|x| x.id)
            .collect();
        for id in orphan_item_ids {
            self.visit_items_delete(&id)?;
        }

        Ok(())
    }

    pub fn visit_items_list(
        &self,
        filters: Option<VisitItemsListFilters>,
    ) -> anyhow::Result<Vec<VisitItem>> {
        let f = filters.unwrap_or_default();
        let include_deleted = f.include_deleted.unwrap_or(false);

        let mut sql = String::from(
      "SELECT id, visit_id, client_id, tooth, title, qty, unit_price, fiscal, vat_code, fiscalized, fiscalized_at, notes, created_at, updated_at, deleted
       FROM visit_items WHERE 1=1",
    );
        let mut args: Vec<rusqlite::types::Value> = Vec::new();

        if !include_deleted {
            sql.push_str(" AND deleted = 0");
        }
        if let Some(vid) = f.visit_id.filter(|x| !x.trim().is_empty()) {
            sql.push_str(&format!(" AND visit_id = ?{}", args.len() + 1));
            args.push(vid.into());
        }
        if let Some(cid) = f.client_id.filter(|x| !x.trim().is_empty()) {
            sql.push_str(&format!(" AND client_id = ?{}", args.len() + 1));
            args.push(cid.into());
        }

        sql.push_str(" ORDER BY updated_at DESC LIMIT 5000");

        let conn = self.conn()?;
        let mut stmt = conn.prepare(&sql)?;
        let mut out = Vec::new();
        let rows = stmt.query_map(rusqlite::params_from_iter(args.iter()), |row| {
            Ok(VisitItem {
                id: row.get(0)?,
                visit_id: row.get(1)?,
                client_id: row.get(2)?,
                tooth: row.get(3)?,
                title: row.get(4)?,
                qty: row.get(5)?,
                unit_price: row.get(6)?,
                fiscal: row.get(7)?,
                vat_code: row.get(8)?,
                fiscalized: row.get(9)?,
                fiscalized_at: row.get(10)?,
                notes: row.get(11)?,
                created_at: row.get(12)?,
                updated_at: row.get(13)?,
                deleted: row.get(14)?,
            })
        })?;
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn visit_items_get(&self, id: &str) -> anyhow::Result<Option<VisitItem>> {
        let conn = self.conn()?;
        Ok(
      conn
        .query_row(
          "SELECT id, visit_id, client_id, tooth, title, qty, unit_price, fiscal, vat_code, fiscalized, fiscalized_at, notes, created_at, updated_at, deleted
           FROM visit_items WHERE id=?1",
          params![id],
          |row| {
            Ok(VisitItem {
              id: row.get(0)?,
              visit_id: row.get(1)?,
              client_id: row.get(2)?,
              tooth: row.get(3)?,
              title: row.get(4)?,
              qty: row.get(5)?,
              unit_price: row.get(6)?,
              fiscal: row.get(7)?,
              vat_code: row.get(8)?,
              fiscalized: row.get(9)?,
              fiscalized_at: row.get(10)?,
              notes: row.get(11)?,
              created_at: row.get(12)?,
              updated_at: row.get(13)?,
              deleted: row.get(14)?,
            })
          },
        )
        .optional()?,
    )
    }

    pub fn visit_items_upsert(&self, input: VisitItemUpsertInput) -> anyhow::Result<VisitItem> {
        if input.visit_id.trim().is_empty() {
            bail!("visit_id is required");
        }
        if input.client_id.trim().is_empty() {
            bail!("client_id is required");
        }
        let title = input.title.trim();
        if title.is_empty() {
            bail!("title is required");
        }
        if !input.qty.is_finite() || input.qty <= 0.0 {
            bail!("qty must be a finite number > 0");
        }
        if !input.unit_price.is_finite() || input.unit_price < 0.0 {
            bail!("unit_price must be a finite number >= 0");
        }

        let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let visit_id = input.visit_id;
        let client_id = input.client_id;
        let tooth = input.tooth;
        let qty = input.qty;
        let unit_price = input.unit_price;
        let fiscal = if input.fiscal.unwrap_or(true) { 1 } else { 0 };
        let vat_registered = self
            .setting_get("clinic_vat_registered")
            .unwrap_or(None)
            .unwrap_or_default()
            .trim()
            == "1";

        let mut vat_code = input
            .vat_code
            .unwrap_or_else(|| "".to_string())
            .trim()
            .to_uppercase();
        if vat_registered {
            if vat_code.is_empty() {
                vat_code = "A".to_string();
            }
            if vat_code == "C" {
                vat_code = "A".to_string();
            }
            match vat_code.as_str() {
                "A" | "D" | "E" => {}
                _ => bail!("vat_code duhet te jete A, D ose E (kur biznesi eshte ne TVSH)"),
            }
        } else {
            vat_code = "C".to_string();
        }
        let notes = input.notes;
        let now = now_iso();

        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        let existing: Option<(String, i64, Option<String>, String, f64, f64, String)> = tx
      .query_row(
        "SELECT created_at, fiscalized, fiscalized_at, title, qty, unit_price, vat_code FROM visit_items WHERE id=?1",
        params![id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?)),
      )
      .optional()?;
        let created_at = existing
            .as_ref()
            .map(|x| x.0.clone())
            .unwrap_or_else(|| now.clone());

        // Preserve fiscalization only when the fiscal line hasn't changed.
        let (fiscalized, fiscalized_at) = if fiscal == 1 {
            if let Some((_, ex_fisc, ex_at, ex_title, ex_qty, ex_price, ex_vat)) = existing.as_ref()
            {
                if *ex_fisc == 1
                    && ex_title.trim() == title
                    && (*ex_qty - qty).abs() < 0.000_000_1
                    && (*ex_price - unit_price).abs() < 0.000_000_1
                    && ex_vat.trim().eq_ignore_ascii_case(&vat_code)
                {
                    (*ex_fisc, ex_at.clone())
                } else {
                    (0, None)
                }
            } else {
                (0, None)
            }
        } else {
            (0, None)
        };

        tx.execute(
      "INSERT INTO visit_items (id, visit_id, client_id, tooth, title, qty, unit_price, fiscal, vat_code, fiscalized, fiscalized_at, notes, created_at, updated_at, deleted)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, 0)
       ON CONFLICT(id) DO UPDATE SET
         visit_id=excluded.visit_id,
         client_id=excluded.client_id,
         tooth=excluded.tooth,
         title=excluded.title,
         qty=excluded.qty,
         unit_price=excluded.unit_price,
         fiscal=excluded.fiscal,
         vat_code=excluded.vat_code,
         fiscalized=excluded.fiscalized,
         fiscalized_at=excluded.fiscalized_at,
         notes=excluded.notes,
         updated_at=excluded.updated_at,
         deleted=excluded.deleted",
      params![
        id,
        &visit_id,
        &client_id,
        &tooth,
        title,
        qty,
        unit_price,
        fiscal,
        &vat_code,
        fiscalized,
        &fiscalized_at,
        &notes,
        &created_at,
        &now
      ],
    )?;
        let row = VisitItem {
            id: id.clone(),
            visit_id,
            client_id,
            tooth,
            title: title.to_string(),
            qty,
            unit_price,
            fiscal,
            vat_code,
            fiscalized,
            fiscalized_at,
            notes,
            created_at: created_at.clone(),
            updated_at: now.clone(),
            deleted: 0,
        };
        let payload = serde_json::to_string(&row)?;
        Self::queue_replace_pending_tx(&tx, "visit_items", &row.id, "upsert", &payload, &now)?;
        tx.commit()?;
        Ok(row)
    }

    pub fn visit_items_delete(&self, id: &str) -> anyhow::Result<()> {
        if id.trim().is_empty() {
            bail!("visit_item id is required");
        }
        let now = now_iso();
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE visit_items SET deleted=1, updated_at=?2 WHERE id=?1",
            params![id, now],
        )?;
        let row = tx
      .query_row(
        "SELECT id, visit_id, client_id, tooth, title, qty, unit_price, fiscal, vat_code, fiscalized, fiscalized_at, notes, created_at, updated_at, deleted
         FROM visit_items WHERE id=?1",
        params![id],
        |r| {
          Ok(VisitItem {
            id: r.get(0)?,
            visit_id: r.get(1)?,
            client_id: r.get(2)?,
            tooth: r.get(3)?,
            title: r.get(4)?,
            qty: r.get(5)?,
            unit_price: r.get(6)?,
            fiscal: r.get(7)?,
            vat_code: r.get(8)?,
            fiscalized: r.get(9)?,
            fiscalized_at: r.get(10)?,
            notes: r.get(11)?,
            created_at: r.get(12)?,
            updated_at: r.get(13)?,
            deleted: r.get(14)?,
          })
        },
      )
      .optional()?
      .ok_or_else(|| anyhow!("visit_item not found"))?;
        let payload = serde_json::to_string(&row)?;
        Self::queue_replace_pending_tx(&tx, "visit_items", &row.id, "delete", &payload, &now)?;
        tx.commit()?;
        Ok(())
    }

    pub fn cash_list(&self, filters: Option<CashListFilters>) -> anyhow::Result<Vec<CashEntry>> {
        let f = filters.unwrap_or_default();
        let include_deleted = f.include_deleted.unwrap_or(false);

        let mut sql = String::from(
            "SELECT id, type, date, amount, category, notes, created_at, updated_at, deleted
       FROM cash_ledger WHERE 1=1",
        );
        let mut args: Vec<rusqlite::types::Value> = Vec::new();

        if !include_deleted {
            sql.push_str(" AND deleted = 0");
        }
        if let Some(t) = f.r#type.filter(|x| !x.trim().is_empty()) {
            sql.push_str(&format!(" AND type = ?{}", args.len() + 1));
            args.push(t.into());
        }
        if let Some(d) = f.date_from.filter(|x| !x.trim().is_empty()) {
            sql.push_str(&format!(" AND date >= ?{}", args.len() + 1));
            args.push(d.into());
        }
        if let Some(d) = f.date_to.filter(|x| !x.trim().is_empty()) {
            sql.push_str(&format!(" AND date <= ?{}", args.len() + 1));
            args.push(d.into());
        }

        sql.push_str(" ORDER BY date DESC, updated_at DESC LIMIT 5000");

        let conn = self.conn()?;
        let mut stmt = conn.prepare(&sql)?;
        let mut out = Vec::new();
        let rows = stmt.query_map(rusqlite::params_from_iter(args.iter()), |row| {
            Ok(CashEntry {
                id: row.get(0)?,
                r#type: row.get(1)?,
                date: row.get(2)?,
                amount: row.get(3)?,
                category: row.get(4)?,
                notes: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
                deleted: row.get(8)?,
            })
        })?;
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn cash_upsert(&self, input: CashEntryUpsertInput) -> anyhow::Result<CashEntry> {
        let t = input.r#type.trim().to_lowercase();
        if t != "income" && t != "expense" {
            bail!("type must be one of: income, expense");
        }
        if !input.amount.is_finite() || input.amount < 0.0 {
            bail!("amount must be a finite number >= 0");
        }

        let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let date = input.date;
        let amount = input.amount;
        let category = input.category;
        let notes = input.notes;
        let now = now_iso();

        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        let existing_created_at: Option<String> = tx
            .query_row(
                "SELECT created_at FROM cash_ledger WHERE id=?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?;
        let created_at = existing_created_at.unwrap_or_else(|| now.clone());

        tx.execute(
      "INSERT INTO cash_ledger (id, type, date, amount, category, notes, created_at, updated_at, deleted)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0)
       ON CONFLICT(id) DO UPDATE SET
         type=excluded.type,
         date=excluded.date,
         amount=excluded.amount,
         category=excluded.category,
         notes=excluded.notes,
         updated_at=excluded.updated_at,
         deleted=excluded.deleted",
      params![id, &t, &date, amount, &category, &notes, &created_at, &now],
    )?;
        let row = CashEntry {
            id: id.clone(),
            r#type: t,
            date,
            amount,
            category,
            notes,
            created_at: created_at.clone(),
            updated_at: now.clone(),
            deleted: 0,
        };
        let payload = serde_json::to_string(&row)?;
        Self::queue_replace_pending_tx(&tx, "cash_ledger", &row.id, "upsert", &payload, &now)?;
        tx.commit()?;
        Ok(row)
    }

    pub fn cash_delete(&self, id: &str) -> anyhow::Result<()> {
        if id.trim().is_empty() {
            bail!("cash entry id is required");
        }
        let now = now_iso();
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE cash_ledger SET deleted=1, updated_at=?2 WHERE id=?1",
            params![id, now],
        )?;
        let row = tx
            .query_row(
                "SELECT id, type, date, amount, category, notes, created_at, updated_at, deleted
         FROM cash_ledger WHERE id=?1",
                params![id],
                |r| {
                    Ok(CashEntry {
                        id: r.get(0)?,
                        r#type: r.get(1)?,
                        date: r.get(2)?,
                        amount: r.get(3)?,
                        category: r.get(4)?,
                        notes: r.get(5)?,
                        created_at: r.get(6)?,
                        updated_at: r.get(7)?,
                        deleted: r.get(8)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| anyhow!("cash entry not found"))?;
        let payload = serde_json::to_string(&row)?;
        Self::queue_replace_pending_tx(&tx, "cash_ledger", &row.id, "delete", &payload, &now)?;
        tx.commit()?;
        Ok(())
    }

    pub fn get_last_sync_time(&self) -> anyhow::Result<Option<String>> {
        self.setting_get("last_sync_time")
    }

    pub fn set_last_sync_time(&self, ts: &str) -> anyhow::Result<()> {
        self.setting_set("last_sync_time", ts)
    }

    pub fn clients_updated_at(&self, id: &str) -> anyhow::Result<Option<String>> {
        let conn = self.conn()?;
        Ok(conn
            .query_row(
                "SELECT updated_at FROM clients WHERE id=?1",
                params![id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten())
    }

    pub fn sales_updated_at(&self, id: &str) -> anyhow::Result<Option<String>> {
        let conn = self.conn()?;
        Ok(conn
            .query_row(
                "SELECT updated_at FROM sales WHERE id=?1",
                params![id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten())
    }

    pub fn payments_updated_at(&self, id: &str) -> anyhow::Result<Option<String>> {
        let conn = self.conn()?;
        Ok(conn
            .query_row(
                "SELECT updated_at FROM payments WHERE id=?1",
                params![id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten())
    }

    pub fn doctors_updated_at(&self, id: &str) -> anyhow::Result<Option<String>> {
        let conn = self.conn()?;
        Ok(conn
            .query_row(
                "SELECT updated_at FROM doctors WHERE id=?1",
                params![id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten())
    }

    pub fn doctor_accounts_updated_at(&self, doctor_id: &str) -> anyhow::Result<Option<String>> {
        let conn = self.conn()?;
        Ok(conn
            .query_row(
                "SELECT updated_at FROM doctor_accounts WHERE doctor_id=?1",
                params![doctor_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten())
    }

    pub fn services_updated_at(&self, id: &str) -> anyhow::Result<Option<String>> {
        let conn = self.conn()?;
        Ok(conn
            .query_row(
                "SELECT updated_at FROM services WHERE id=?1",
                params![id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten())
    }

    pub fn appointments_updated_at(&self, id: &str) -> anyhow::Result<Option<String>> {
        let conn = self.conn()?;
        Ok(conn
            .query_row(
                "SELECT updated_at FROM appointments WHERE id=?1",
                params![id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten())
    }

    pub fn visits_updated_at(&self, id: &str) -> anyhow::Result<Option<String>> {
        let conn = self.conn()?;
        Ok(conn
            .query_row(
                "SELECT updated_at FROM visits WHERE id=?1",
                params![id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten())
    }

    pub fn visit_items_updated_at(&self, id: &str) -> anyhow::Result<Option<String>> {
        let conn = self.conn()?;
        Ok(conn
            .query_row(
                "SELECT updated_at FROM visit_items WHERE id=?1",
                params![id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten())
    }

    pub fn cash_updated_at(&self, id: &str) -> anyhow::Result<Option<String>> {
        let conn = self.conn()?;
        Ok(conn
            .query_row(
                "SELECT updated_at FROM cash_ledger WHERE id=?1",
                params![id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten())
    }

    pub fn apply_remote_client(&self, row: &Client) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
      "INSERT INTO clients (id, name, phone, email, notes,
                            first_name, last_name, parent_name, dob, gender, city, address, allergies, weight_kg, height_cm, patient_code,
                            created_at, updated_at, deleted)
       VALUES (?1, ?2, ?3, ?4, ?5,
               ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
               ?17, ?18, ?19)
       ON CONFLICT(id) DO UPDATE SET
         name=excluded.name,
         phone=excluded.phone,
         email=excluded.email,
         notes=excluded.notes,
         first_name=excluded.first_name,
         last_name=excluded.last_name,
         parent_name=excluded.parent_name,
         dob=excluded.dob,
         gender=excluded.gender,
         city=excluded.city,
         address=excluded.address,
         allergies=excluded.allergies,
         weight_kg=excluded.weight_kg,
         height_cm=excluded.height_cm,
         patient_code=excluded.patient_code,
         created_at=excluded.created_at,
         updated_at=excluded.updated_at,
         deleted=excluded.deleted",
      params![
        row.id,
        row.name,
        row.phone,
        row.email,
        row.notes,
        row.first_name,
        row.last_name,
        row.parent_name,
        row.dob,
        row.gender,
        row.city,
        row.address,
        row.allergies,
        row.weight_kg,
        row.height_cm,
        row.patient_code,
        row.created_at,
        row.updated_at,
        row.deleted
      ],
    )?;
        Ok(())
    }

    pub fn apply_remote_sale(&self, row: &Sale) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
      "INSERT INTO sales (id, client_id, date, total, notes, fiscalized, fiscalized_at, created_at, updated_at, deleted)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
       ON CONFLICT(id) DO UPDATE SET
         client_id=excluded.client_id,
         date=excluded.date,
         total=excluded.total,
         notes=excluded.notes,
         fiscalized=excluded.fiscalized,
         fiscalized_at=excluded.fiscalized_at,
         created_at=excluded.created_at,
         updated_at=excluded.updated_at,
         deleted=excluded.deleted",
      params![
        row.id,
        row.client_id,
        row.date,
        row.total,
        row.notes,
        row.fiscalized,
        row.fiscalized_at,
        row.created_at,
        row.updated_at,
        row.deleted
      ],
    )?;
        Ok(())
    }

    pub fn apply_remote_payment(&self, row: &Payment) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
      "INSERT INTO payments (id, client_id, sale_id, date, amount, method, notes, created_at, updated_at, deleted)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
       ON CONFLICT(id) DO UPDATE SET
         client_id=excluded.client_id,
         sale_id=excluded.sale_id,
         date=excluded.date,
         amount=excluded.amount,
         method=excluded.method,
         notes=excluded.notes,
         created_at=excluded.created_at,
         updated_at=excluded.updated_at,
         deleted=excluded.deleted",
      params![
        row.id,
        row.client_id,
        row.sale_id,
        row.date,
        row.amount,
        row.method,
        row.notes,
        row.created_at,
        row.updated_at,
        row.deleted
      ],
    )?;
        Ok(())
    }

    pub fn apply_remote_doctor_account(&self, row: &DoctorAccount) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
      "INSERT INTO doctor_accounts (doctor_id, clinic_id, salt, password_hash, is_admin, created_at, updated_at, deleted)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
       ON CONFLICT(doctor_id) DO UPDATE SET
         clinic_id=excluded.clinic_id,
         salt=excluded.salt,
         password_hash=excluded.password_hash,
         is_admin=excluded.is_admin,
         created_at=excluded.created_at,
         updated_at=excluded.updated_at,
         deleted=excluded.deleted",
      params![
        row.doctor_id,
        row.clinic_id,
        row.salt,
        row.password_hash,
        row.is_admin,
        row.created_at,
        row.updated_at,
        row.deleted
      ],
    )?;
        Ok(())
    }

    pub fn apply_remote_doctor(&self, row: &Doctor) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
      "INSERT INTO doctors (id, code, name, title, specialty, phone, email, notes, created_at, updated_at, deleted)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
       ON CONFLICT(id) DO UPDATE SET
         code=excluded.code,
         name=excluded.name,
         title=excluded.title,
         specialty=excluded.specialty,
         phone=excluded.phone,
         email=excluded.email,
         notes=excluded.notes,
         created_at=excluded.created_at,
         updated_at=excluded.updated_at,
         deleted=excluded.deleted",
      params![
        row.id,
        row.code,
        row.name,
        row.title,
        row.specialty,
        row.phone,
        row.email,
        row.notes,
        row.created_at,
        row.updated_at,
        row.deleted
      ],
    )?;
        Ok(())
    }

    pub fn apply_remote_service(&self, row: &Service) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
      "INSERT INTO services (id, title, default_price, vat_code, notes, created_at, updated_at, deleted)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
       ON CONFLICT(id) DO UPDATE SET
         title=excluded.title,
         default_price=excluded.default_price,
         vat_code=excluded.vat_code,
         notes=excluded.notes,
         created_at=excluded.created_at,
         updated_at=excluded.updated_at,
         deleted=excluded.deleted",
      params![
        row.id,
        row.title,
        row.default_price,
        row.vat_code,
        row.notes,
        row.created_at,
        row.updated_at,
        row.deleted
      ],
    )?;
        Ok(())
    }

    pub fn apply_remote_appointment(&self, row: &Appointment) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
      "INSERT INTO appointments (id, client_id, doctor_id, start_at, end_at, status, notes, created_at, updated_at, deleted)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
       ON CONFLICT(id) DO UPDATE SET
         client_id=excluded.client_id,
         doctor_id=excluded.doctor_id,
         start_at=excluded.start_at,
         end_at=excluded.end_at,
         status=excluded.status,
         notes=excluded.notes,
         created_at=excluded.created_at,
         updated_at=excluded.updated_at,
         deleted=excluded.deleted",
      params![
        row.id,
        row.client_id,
        row.doctor_id,
        row.start_at,
        row.end_at,
        row.status,
        row.notes,
        row.created_at,
        row.updated_at,
        row.deleted
      ],
    )?;
        Ok(())
    }

    pub fn apply_remote_visit(&self, row: &Visit) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
      "INSERT INTO visits (
         id, client_id, doctor_id, date, visit_time, status, notes,
         body_weight, body_weight_unit, body_height, body_height_unit,
         head_circumference, head_circumference_unit,
         body_temperature, body_temperature_unit,
         blood_oxygen, blood_oxygen_unit,
         glycemia, glycemia_unit,
         pulse, pulse_unit,
         bmi,
         blood_pressure_systolic, blood_pressure_diastolic, blood_pressure_unit,
         complaints, additional_notes, controls, remarks, analyses, advice, therapies, diagnosis, examinations,
         created_at, updated_at, deleted, specialty_report
       )
       VALUES (
         ?1, ?2, ?3, ?4, ?5, ?6, ?7,
         ?8, ?9, ?10, ?11,
         ?12, ?13,
         ?14, ?15,
         ?16, ?17,
         ?18, ?19,
         ?20, ?21,
         ?22,
         ?23, ?24, ?25,
         ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34,
         ?35, ?36, ?37, ?38
       )
       ON CONFLICT(id) DO UPDATE SET
         client_id=excluded.client_id,
         doctor_id=excluded.doctor_id,
         date=excluded.date,
         visit_time=excluded.visit_time,
         status=excluded.status,
         notes=excluded.notes,
         body_weight=excluded.body_weight,
         body_weight_unit=excluded.body_weight_unit,
         body_height=excluded.body_height,
         body_height_unit=excluded.body_height_unit,
         head_circumference=excluded.head_circumference,
         head_circumference_unit=excluded.head_circumference_unit,
         body_temperature=excluded.body_temperature,
         body_temperature_unit=excluded.body_temperature_unit,
         blood_oxygen=excluded.blood_oxygen,
         blood_oxygen_unit=excluded.blood_oxygen_unit,
         glycemia=excluded.glycemia,
         glycemia_unit=excluded.glycemia_unit,
         pulse=excluded.pulse,
         pulse_unit=excluded.pulse_unit,
         bmi=excluded.bmi,
         blood_pressure_systolic=excluded.blood_pressure_systolic,
         blood_pressure_diastolic=excluded.blood_pressure_diastolic,
         blood_pressure_unit=excluded.blood_pressure_unit,
         complaints=excluded.complaints,
         additional_notes=excluded.additional_notes,
         controls=excluded.controls,
         remarks=excluded.remarks,
         analyses=excluded.analyses,
         advice=excluded.advice,
         therapies=excluded.therapies,
         diagnosis=excluded.diagnosis,
         examinations=excluded.examinations,
         specialty_report=excluded.specialty_report,
         created_at=excluded.created_at,
         updated_at=excluded.updated_at,
         deleted=excluded.deleted",
      params![
        row.id,
        row.client_id,
        row.doctor_id,
        row.date,
        row.visit_time,
        row.status,
        row.notes,
        row.body_weight,
        row.body_weight_unit,
        row.body_height,
        row.body_height_unit,
        row.head_circumference,
        row.head_circumference_unit,
        row.body_temperature,
        row.body_temperature_unit,
        row.blood_oxygen,
        row.blood_oxygen_unit,
        row.glycemia,
        row.glycemia_unit,
        row.pulse,
        row.pulse_unit,
        row.bmi,
        row.blood_pressure_systolic,
        row.blood_pressure_diastolic,
        row.blood_pressure_unit,
        row.complaints,
        row.additional_notes,
        row.controls,
        row.remarks,
        row.analyses,
        row.advice,
        row.therapies,
        row.diagnosis,
        row.examinations,
        row.created_at,
        row.updated_at,
        row.deleted,
        row.specialty_report
      ],
    )?;
        Ok(())
    }

    pub fn apply_remote_visit_item(&self, row: &VisitItem) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
      "INSERT INTO visit_items (id, visit_id, client_id, tooth, title, qty, unit_price, fiscal, vat_code, fiscalized, fiscalized_at, notes, created_at, updated_at, deleted)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
       ON CONFLICT(id) DO UPDATE SET
         visit_id=excluded.visit_id,
         client_id=excluded.client_id,
         tooth=excluded.tooth,
         title=excluded.title,
         qty=excluded.qty,
         unit_price=excluded.unit_price,
         fiscal=excluded.fiscal,
         vat_code=excluded.vat_code,
         fiscalized=excluded.fiscalized,
         fiscalized_at=excluded.fiscalized_at,
         notes=excluded.notes,
         created_at=excluded.created_at,
         updated_at=excluded.updated_at,
         deleted=excluded.deleted",
      params![
        row.id,
        row.visit_id,
        row.client_id,
        row.tooth,
        row.title,
        row.qty,
        row.unit_price,
        row.fiscal,
        row.vat_code,
        row.fiscalized,
        row.fiscalized_at,
        row.notes,
        row.created_at,
        row.updated_at,
        row.deleted
      ],
    )?;
        Ok(())
    }

    pub fn apply_remote_cash_entry(&self, row: &CashEntry) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
      "INSERT INTO cash_ledger (id, type, date, amount, category, notes, created_at, updated_at, deleted)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
       ON CONFLICT(id) DO UPDATE SET
         type=excluded.type,
         date=excluded.date,
         amount=excluded.amount,
         category=excluded.category,
         notes=excluded.notes,
         created_at=excluded.created_at,
         updated_at=excluded.updated_at,
         deleted=excluded.deleted",
      params![
        row.id,
        row.r#type,
        row.date,
        row.amount,
        row.category,
        row.notes,
        row.created_at,
        row.updated_at,
        row.deleted
      ],
    )?;
        Ok(())
    }

    pub fn regular_invoice_insert(
        &self,
        id: &str,
        sale_id: &str,
        invoice_number: Option<&str>,
        client_id: Option<&str>,
        client_name: Option<&str>,
        date: Option<&str>,
        total: f64,
        pdf_filename: Option<&str>,
        created_at: &str,
    ) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR IGNORE INTO regular_invoices
             (id, sale_id, invoice_number, client_id, client_name, date, total, pdf_filename, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![id, sale_id, invoice_number, client_id, client_name, date, total, pdf_filename, created_at],
        )?;
        Ok(())
    }

    pub fn regular_invoices_list(
        &self,
        date_from: Option<&str>,
        date_to: Option<&str>,
    ) -> anyhow::Result<Vec<crate::models::RegularInvoice>> {
        let conn = self.conn()?;
        let mut sql = String::from(
            "SELECT id, sale_id, invoice_number, client_id, client_name, date, total, pdf_filename, created_at
             FROM regular_invoices WHERE 1=1",
        );
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        if let Some(d) = date_from.filter(|x| !x.trim().is_empty()) {
            sql.push_str(&format!(" AND date >= ?{}", args.len() + 1));
            args.push(d.to_string().into());
        }
        if let Some(d) = date_to.filter(|x| !x.trim().is_empty()) {
            sql.push_str(&format!(" AND date <= ?{}", args.len() + 1));
            args.push(d.to_string().into());
        }
        sql.push_str(" ORDER BY date DESC, created_at DESC");
        let conn_ref = &conn;
        let mut stmt = conn_ref.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(args), |row| {
            Ok(crate::models::RegularInvoice {
                id: row.get(0)?,
                sale_id: row.get(1)?,
                invoice_number: row.get(2)?,
                client_id: row.get(3)?,
                client_name: row.get(4)?,
                date: row.get(5)?,
                total: row.get(6)?,
                pdf_filename: row.get(7)?,
                created_at: row.get(8)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn sales_monthly_report(&self, year: i32) -> anyhow::Result<Vec<crate::models::MonthlyReportRow>> {
        let conn = self.conn.lock().unwrap();
        let year_str = year.to_string();
        let mut stmt = conn.prepare(
            "SELECT strftime('%m', date) as month, SUM(total) as total, COUNT(*) as count
             FROM sales
             WHERE deleted = 0 AND date IS NOT NULL AND strftime('%Y', date) = ?1
             GROUP BY strftime('%m', date)
             ORDER BY month"
        )?;
        let rows = stmt.query_map([&year_str], |row| {
            Ok(crate::models::MonthlyReportRow {
                month: row.get::<_, String>(0)?,
                total: row.get::<_, f64>(1)?,
                count: row.get::<_, i64>(2)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn stock_suppliers_list(&self) -> anyhow::Result<Vec<crate::models::StockSupplier>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, phone, email, address, notes, created_at, updated_at FROM stock_suppliers WHERE deleted = 0 ORDER BY name"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(crate::models::StockSupplier {
                deleted: 0,
                id: row.get(0)?, name: row.get(1)?, phone: row.get(2)?, email: row.get(3)?,
                address: row.get(4)?, notes: row.get(5)?, created_at: row.get(6)?, updated_at: row.get(7)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn stock_supplier_upsert(&self, s: &crate::models::StockSupplier) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO stock_suppliers (id, name, phone, email, address, notes, created_at, updated_at, deleted)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,0)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name, phone=excluded.phone, email=excluded.email,
               address=excluded.address, notes=excluded.notes, updated_at=excluded.updated_at",
            rusqlite::params![s.id, s.name, s.phone, s.email, s.address, s.notes, s.created_at, s.updated_at],
        )?;
        let payload = serde_json::to_string(s)?;
        Self::queue_replace_pending_conn(&conn, "stock_suppliers", &s.id, "upsert", &payload, &now_iso())?;
        Ok(())
    }

    pub fn stock_supplier_delete(&self, id: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = now_iso();
        conn.execute("UPDATE stock_suppliers SET deleted=1, updated_at=?2 WHERE id=?1", params![id, now])?;
        let payload = serde_json::json!({ "id": id, "updated_at": now }).to_string();
        Self::queue_replace_pending_conn(&conn, "stock_suppliers", id, "delete", &payload, &now)?;
        Ok(())
    }

    pub fn stock_items_list(&self) -> anyhow::Result<Vec<crate::models::StockItem>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT i.id, i.name, i.unit, i.category, i.supplier_id, i.min_quantity, COALESCE(i.sale_price, 0), i.notes, i.created_at, i.updated_at,
                    s.name as supplier_name,
                    COALESCE((SELECT SUM(CASE WHEN m.movement_type='in' THEN m.quantity WHEN m.movement_type='out' THEN -m.quantity ELSE m.quantity END)
                              FROM stock_movements m WHERE m.item_id = i.id), 0) as current_quantity
             FROM stock_items i
             LEFT JOIN stock_suppliers s ON s.id = i.supplier_id
             WHERE i.deleted = 0 ORDER BY i.name"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(crate::models::StockItem {
                deleted: 0,
                id: row.get(0)?, name: row.get(1)?, unit: row.get(2)?, category: row.get(3)?,
                supplier_id: row.get(4)?, min_quantity: row.get(5)?, sale_price: row.get(6)?,
                notes: row.get(7)?, created_at: row.get(8)?, updated_at: row.get(9)?,
                supplier_name: row.get(10)?, current_quantity: row.get(11)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn stock_item_upsert(&self, item: &crate::models::StockItem) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO stock_items (id, name, unit, category, supplier_id, min_quantity, sale_price, notes, created_at, updated_at, deleted)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,0)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name, unit=excluded.unit, category=excluded.category,
               supplier_id=excluded.supplier_id, min_quantity=excluded.min_quantity,
               sale_price=excluded.sale_price, notes=excluded.notes, updated_at=excluded.updated_at",
            rusqlite::params![item.id, item.name, item.unit, item.category, item.supplier_id, item.min_quantity, item.sale_price, item.notes, item.created_at, item.updated_at],
        )?;
        let payload = serde_json::to_string(item)?;
        Self::queue_replace_pending_conn(&conn, "stock_items", &item.id, "upsert", &payload, &now_iso())?;
        Ok(())
    }

    pub fn stock_item_delete(&self, id: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = now_iso();
        conn.execute("UPDATE stock_items SET deleted=1, updated_at=?2 WHERE id=?1", params![id, now])?;
        let payload = serde_json::json!({ "id": id, "updated_at": now }).to_string();
        Self::queue_replace_pending_conn(&conn, "stock_items", id, "delete", &payload, &now)?;
        Ok(())
    }

    pub fn stock_movements_list(&self, item_id: &str) -> anyhow::Result<Vec<crate::models::StockMovement>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, item_id, movement_type, quantity, price_per_unit, notes, date, created_at
             FROM stock_movements WHERE item_id = ?1 ORDER BY date DESC, created_at DESC LIMIT 200"
        )?;
        let rows = stmt.query_map([item_id], |row| {
            Ok(crate::models::StockMovement {
                deleted: 0,
                id: row.get(0)?, item_id: row.get(1)?, movement_type: row.get(2)?,
                quantity: row.get(3)?, price_per_unit: row.get(4)?, notes: row.get(5)?,
                date: row.get(6)?, created_at: row.get(7)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn stock_movement_add(&self, m: &crate::models::StockMovement) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO stock_movements (id, item_id, movement_type, quantity, price_per_unit, notes, date, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            rusqlite::params![m.id, m.item_id, m.movement_type, m.quantity, m.price_per_unit, m.notes, m.date, m.created_at],
        )?;
        let payload = serde_json::to_string(m)?;
        Self::queue_replace_pending_conn(&conn, "stock_movements", &m.id, "upsert", &payload, &now_iso())?;
        Ok(())
    }

    pub fn fiscal_jobs_list(&self, status: Option<String>) -> anyhow::Result<Vec<crate::models::FiscalJob>> {
        let conn = self.conn()?;
        let mut sql = String::from(
            "SELECT id, sale_id, status, requested_by, error, processed_at, created_at, updated_at, deleted
             FROM fiscal_jobs WHERE deleted=0",
        );
        let mut args: Vec<String> = Vec::new();
        if let Some(st) = status.as_deref().map(str::trim).filter(|x| !x.is_empty()) {
            args.push(st.to_string());
            sql.push_str(" AND status=?1");
        }
        sql.push_str(" ORDER BY created_at ASC LIMIT 300");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(args.iter()), |r| {
            Ok(crate::models::FiscalJob {
                id: r.get(0)?, sale_id: r.get(1)?, status: r.get(2)?, requested_by: r.get(3)?,
                error: r.get(4)?, processed_at: r.get(5)?, created_at: r.get(6)?,
                updated_at: r.get(7)?, deleted: r.get(8)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn fiscal_job_upsert(&self, row: &crate::models::FiscalJob) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO fiscal_jobs (id, sale_id, status, requested_by, error, processed_at, created_at, updated_at, deleted)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(id) DO UPDATE SET sale_id=excluded.sale_id, status=excluded.status,
               requested_by=excluded.requested_by, error=excluded.error, processed_at=excluded.processed_at,
               updated_at=excluded.updated_at, deleted=excluded.deleted",
            params![row.id, row.sale_id, row.status, row.requested_by, row.error, row.processed_at, row.created_at, row.updated_at, row.deleted],
        )?;
        let payload = serde_json::to_string(row)?;
        Self::queue_replace_pending_conn(&conn, "fiscal_jobs", &row.id, "upsert", &payload, &now_iso())?;
        Ok(())
    }

    pub fn fiscal_job_get(&self, id: &str) -> anyhow::Result<Option<crate::models::FiscalJob>> {
        let conn = self.conn()?;
        let r = conn.query_row(
            "SELECT id, sale_id, status, requested_by, error, processed_at, created_at, updated_at, deleted
             FROM fiscal_jobs WHERE id=?1",
            params![id],
            |r| Ok(crate::models::FiscalJob {
                id: r.get(0)?, sale_id: r.get(1)?, status: r.get(2)?, requested_by: r.get(3)?,
                error: r.get(4)?, processed_at: r.get(5)?, created_at: r.get(6)?,
                updated_at: r.get(7)?, deleted: r.get(8)?,
            }),
        ).optional()?;
        Ok(r)
    }

    pub fn apply_remote_fiscal_job(&self, r: &crate::models::FiscalJob) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO fiscal_jobs (id, sale_id, status, requested_by, error, processed_at, created_at, updated_at, deleted)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(id) DO UPDATE SET sale_id=excluded.sale_id, status=excluded.status,
               requested_by=excluded.requested_by, error=excluded.error, processed_at=excluded.processed_at,
               created_at=excluded.created_at, updated_at=excluded.updated_at, deleted=excluded.deleted",
            params![r.id, r.sale_id, r.status, r.requested_by, r.error, r.processed_at, r.created_at, r.updated_at, r.deleted],
        )?;
        Ok(())
    }

    pub fn prescriptions_list(&self, kind: Option<String>, client_id: Option<String>) -> anyhow::Result<Vec<crate::models::Prescription>> {
        let conn = self.conn()?;
        let mut sql = String::from(
            "SELECT id, visit_id, client_id, doctor_id, kind, title, content, created_at, updated_at, deleted
             FROM prescriptions WHERE deleted=0",
        );
        let mut args: Vec<String> = Vec::new();
        if let Some(k) = kind.as_deref().map(str::trim).filter(|x| !x.is_empty()) {
            args.push(k.to_string());
            sql.push_str(&format!(" AND kind=?{}", args.len()));
        }
        if let Some(c) = client_id.as_deref().map(str::trim).filter(|x| !x.is_empty()) {
            args.push(c.to_string());
            sql.push_str(&format!(" AND client_id=?{}", args.len()));
        }
        sql.push_str(" ORDER BY created_at DESC LIMIT 500");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(args.iter()), |r| {
            Ok(crate::models::Prescription {
                id: r.get(0)?, visit_id: r.get(1)?, client_id: r.get(2)?, doctor_id: r.get(3)?,
                kind: r.get(4)?, title: r.get(5)?, content: r.get(6)?,
                created_at: r.get(7)?, updated_at: r.get(8)?, deleted: r.get(9)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn prescriptions_upsert(&self, row: &crate::models::Prescription) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO prescriptions (id, visit_id, client_id, doctor_id, kind, title, content, created_at, updated_at, deleted)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,0)
             ON CONFLICT(id) DO UPDATE SET visit_id=excluded.visit_id, client_id=excluded.client_id,
               doctor_id=excluded.doctor_id, kind=excluded.kind, title=excluded.title,
               content=excluded.content, updated_at=excluded.updated_at",
            params![row.id, row.visit_id, row.client_id, row.doctor_id, row.kind, row.title, row.content, row.created_at, row.updated_at],
        )?;
        let payload = serde_json::to_string(row)?;
        Self::queue_replace_pending_conn(&conn, "prescriptions", &row.id, "upsert", &payload, &now_iso())?;
        Ok(())
    }

    pub fn prescriptions_delete(&self, id: &str) -> anyhow::Result<()> {
        let conn = self.conn()?;
        let now = now_iso();
        conn.execute("UPDATE prescriptions SET deleted=1, updated_at=?2 WHERE id=?1", params![id, now])?;
        let payload = serde_json::json!({ "id": id, "updated_at": now }).to_string();
        Self::queue_replace_pending_conn(&conn, "prescriptions", id, "delete", &payload, &now)?;
        Ok(())
    }

    pub fn apply_remote_prescription(&self, r: &crate::models::Prescription) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO prescriptions (id, visit_id, client_id, doctor_id, kind, title, content, created_at, updated_at, deleted)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
             ON CONFLICT(id) DO UPDATE SET visit_id=excluded.visit_id, client_id=excluded.client_id,
               doctor_id=excluded.doctor_id, kind=excluded.kind, title=excluded.title, content=excluded.content,
               created_at=excluded.created_at, updated_at=excluded.updated_at, deleted=excluded.deleted",
            params![r.id, r.visit_id, r.client_id, r.doctor_id, r.kind, r.title, r.content, r.created_at, r.updated_at, r.deleted],
        )?;
        Ok(())
    }

    pub fn apply_remote_stock_supplier(&self, r: &crate::models::StockSupplier) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO stock_suppliers (id, name, phone, email, address, notes, created_at, updated_at, deleted)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name, phone=excluded.phone, email=excluded.email,
               address=excluded.address, notes=excluded.notes, created_at=excluded.created_at,
               updated_at=excluded.updated_at, deleted=excluded.deleted",
            params![r.id, r.name, r.phone, r.email, r.address, r.notes, r.created_at, r.updated_at, r.deleted],
        )?;
        Ok(())
    }

    pub fn apply_remote_stock_item(&self, r: &crate::models::StockItem) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO stock_items (id, name, unit, category, supplier_id, min_quantity, sale_price, notes, created_at, updated_at, deleted)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name, unit=excluded.unit, category=excluded.category,
               supplier_id=excluded.supplier_id, min_quantity=excluded.min_quantity, sale_price=excluded.sale_price,
               notes=excluded.notes, created_at=excluded.created_at, updated_at=excluded.updated_at, deleted=excluded.deleted",
            params![r.id, r.name, r.unit, r.category, r.supplier_id, r.min_quantity, r.sale_price, r.notes, r.created_at, r.updated_at, r.deleted],
        )?;
        Ok(())
    }

    pub fn apply_remote_stock_movement(&self, r: &crate::models::StockMovement) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO stock_movements (id, item_id, movement_type, quantity, price_per_unit, notes, date, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
             ON CONFLICT(id) DO NOTHING",
            params![r.id, r.item_id, r.movement_type, r.quantity, r.price_per_unit, r.notes, r.date, r.created_at],
        )?;
        Ok(())
    }

    pub fn sale_items_list(&self, sale_id: &str) -> anyhow::Result<Vec<crate::models::SaleItem>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, sale_id, item_type, ref_id, title, qty, unit_price, created_at
             FROM sale_items WHERE sale_id = ?1 ORDER BY created_at"
        )?;
        let rows = stmt.query_map([sale_id], |row| {
            Ok(crate::models::SaleItem {
                id: row.get(0)?, sale_id: row.get(1)?, item_type: row.get(2)?, ref_id: row.get(3)?,
                title: row.get(4)?, qty: row.get(5)?, unit_price: row.get(6)?, created_at: row.get(7)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn sale_items_replace(&self, sale_id: &str, items: &[crate::models::SaleItemInput]) -> anyhow::Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        // Return stock for old product line-items being replaced.
        {
            let mut stmt = tx.prepare(
                "SELECT ref_id, qty FROM sale_items WHERE sale_id = ?1 AND item_type = 'product' AND ref_id IS NOT NULL"
            )?;
            let old_products: Vec<(String, f64)> = stmt
                .query_map([sale_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            for (ref_id, qty) in old_products {
                let now = chrono::Utc::now().to_rfc3339();
                let date = now.split('T').next().unwrap_or(&now).to_string();
                tx.execute(
                    "INSERT INTO stock_movements (id, item_id, movement_type, quantity, price_per_unit, notes, date, created_at)
                     VALUES (?1,?2,'in',?3,NULL,?4,?5,?6)",
                    rusqlite::params![Uuid::new_v4().to_string(), ref_id, qty, "Rikthim - ndryshim shitje", date, now],
                )?;
            }
        }
        tx.execute("DELETE FROM sale_items WHERE sale_id = ?1", [sale_id])?;
        for item in items {
            let now = chrono::Utc::now().to_rfc3339();
            let date = now.split('T').next().unwrap_or(&now).to_string();
            let id = Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO sale_items (id, sale_id, item_type, ref_id, title, qty, unit_price, created_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                rusqlite::params![id, sale_id, item.item_type, item.ref_id, item.title, item.qty, item.unit_price, now],
            )?;
            if item.item_type == "product" {
                if let Some(ref_id) = &item.ref_id {
                    tx.execute(
                        "INSERT INTO stock_movements (id, item_id, movement_type, quantity, price_per_unit, notes, date, created_at)
                         VALUES (?1,?2,'out',?3,?4,?5,?6,?7)",
                        rusqlite::params![Uuid::new_v4().to_string(), ref_id, item.qty, item.unit_price, "Shitje", date, now],
                    )?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    // ─── Offers ───────────────────────────────────────────────────────────────

    pub fn next_offer_number(&self, month: u32, year: i32) -> anyhow::Result<String> {
        let conn = self.conn.lock().unwrap();
        let pattern = format!("%/{:02}/{}", month, year);
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM offers WHERE offer_number LIKE ?1 AND deleted_at IS NULL",
            rusqlite::params![pattern],
            |r| r.get(0),
        ).unwrap_or(0);
        Ok(format!("{:03}/{:02}/{}", count + 1, month, year))
    }

    pub fn offers_list(&self, clinic_id: &str) -> anyhow::Result<Vec<crate::models::Offer>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, clinic_id, client_id, offer_number, status, valid_until, notes,
                    vat_pct, subtotal, vat_amount, total, invoice_id, source_offer_id,
                    created_at, updated_at, deleted_at
             FROM offers WHERE clinic_id = ?1 AND deleted_at IS NULL
             ORDER BY created_at DESC"
        )?;
        let rows = stmt.query_map(rusqlite::params![clinic_id], |r| {
            Ok(crate::models::Offer {
                id: r.get(0)?, clinic_id: r.get(1)?, client_id: r.get(2)?,
                offer_number: r.get(3)?, status: r.get(4)?, valid_until: r.get(5)?,
                notes: r.get(6)?, vat_pct: r.get(7)?, subtotal: r.get(8)?,
                vat_amount: r.get(9)?, total: r.get(10)?, invoice_id: r.get(11)?,
                source_offer_id: r.get(12)?, created_at: r.get(13)?,
                updated_at: r.get(14)?, deleted_at: r.get(15)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn offer_get(&self, id: &str) -> anyhow::Result<Option<crate::models::Offer>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, clinic_id, client_id, offer_number, status, valid_until, notes,
                    vat_pct, subtotal, vat_amount, total, invoice_id, source_offer_id,
                    created_at, updated_at, deleted_at
             FROM offers WHERE id = ?1",
            rusqlite::params![id],
            |r| Ok(crate::models::Offer {
                id: r.get(0)?, clinic_id: r.get(1)?, client_id: r.get(2)?,
                offer_number: r.get(3)?, status: r.get(4)?, valid_until: r.get(5)?,
                notes: r.get(6)?, vat_pct: r.get(7)?, subtotal: r.get(8)?,
                vat_amount: r.get(9)?, total: r.get(10)?, invoice_id: r.get(11)?,
                source_offer_id: r.get(12)?, created_at: r.get(13)?,
                updated_at: r.get(14)?, deleted_at: r.get(15)?,
            }),
        ).optional().map_err(|e| anyhow::anyhow!(e))
    }

    pub fn offer_upsert(&self, o: &crate::models::Offer) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO offers (id, clinic_id, client_id, offer_number, status, valid_until, notes,
                                 vat_pct, subtotal, vat_amount, total, invoice_id, source_offer_id,
                                 created_at, updated_at, deleted_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)
             ON CONFLICT(id) DO UPDATE SET
               client_id=excluded.client_id, status=excluded.status,
               valid_until=excluded.valid_until, notes=excluded.notes,
               vat_pct=excluded.vat_pct, subtotal=excluded.subtotal,
               vat_amount=excluded.vat_amount, total=excluded.total,
               invoice_id=excluded.invoice_id, source_offer_id=excluded.source_offer_id,
               updated_at=excluded.updated_at, deleted_at=excluded.deleted_at",
            rusqlite::params![
                o.id, o.clinic_id, o.client_id, o.offer_number, o.status,
                o.valid_until, o.notes, o.vat_pct, o.subtotal, o.vat_amount,
                o.total, o.invoice_id, o.source_offer_id, o.created_at, o.updated_at, o.deleted_at
            ],
        )?;
        Ok(())
    }

    pub fn offer_soft_delete(&self, id: &str, now: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE offers SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2",
            rusqlite::params![now, id],
        )?;
        Ok(())
    }

    pub fn offer_items_list(&self, offer_id: &str) -> anyhow::Result<Vec<crate::models::OfferItem>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, offer_id, clinic_id, description, qty, unit_price, discount_pct,
                    line_total, sort_order, created_at, updated_at, deleted_at
             FROM offer_items WHERE offer_id = ?1 AND deleted_at IS NULL
             ORDER BY sort_order ASC, created_at ASC"
        )?;
        let rows = stmt.query_map(rusqlite::params![offer_id], |r| {
            Ok(crate::models::OfferItem {
                id: r.get(0)?, offer_id: r.get(1)?, clinic_id: r.get(2)?,
                description: r.get(3)?, qty: r.get(4)?, unit_price: r.get(5)?,
                discount_pct: r.get(6)?, line_total: r.get(7)?, sort_order: r.get(8)?,
                created_at: r.get(9)?, updated_at: r.get(10)?, deleted_at: r.get(11)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn offer_items_replace(
        &self,
        offer_id: &str,
        clinic_id: &str,
        items: &[crate::models::OfferItemInput],
        now: &str,
    ) -> anyhow::Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE offer_items SET deleted_at = ?1, updated_at = ?1 WHERE offer_id = ?2 AND deleted_at IS NULL",
            rusqlite::params![now, offer_id],
        )?;
        for (i, item) in items.iter().enumerate() {
            let id = item.id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            let line_total = item.qty * item.unit_price * (1.0 - item.discount_pct / 100.0);
            tx.execute(
                "INSERT INTO offer_items (id, offer_id, clinic_id, description, qty, unit_price, discount_pct, line_total, sort_order, created_at, updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?10)
                 ON CONFLICT(id) DO UPDATE SET description=excluded.description, qty=excluded.qty, unit_price=excluded.unit_price, discount_pct=excluded.discount_pct, line_total=excluded.line_total, sort_order=excluded.sort_order, updated_at=excluded.updated_at, deleted_at=NULL",
                rusqlite::params![id, offer_id, clinic_id, item.description, item.qty, item.unit_price, item.discount_pct, line_total, i as i64, now],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn offer_updated_at(&self, id: &str) -> anyhow::Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT updated_at FROM offers WHERE id = ?1",
            rusqlite::params![id],
            |r| r.get(0),
        ).optional().map_err(|e| anyhow::anyhow!(e))
    }

    pub fn offer_items_updated_at(&self, id: &str) -> anyhow::Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT updated_at FROM offer_items WHERE id = ?1",
            rusqlite::params![id],
            |r| r.get(0),
        ).optional().map_err(|e| anyhow::anyhow!(e))
    }

    pub fn apply_remote_offer(&self, o: &crate::models::Offer) -> anyhow::Result<()> {
        self.offer_upsert(o)
    }

    pub fn apply_remote_offer_item(&self, item: &crate::models::OfferItem) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO offer_items (id, offer_id, clinic_id, description, qty, unit_price, discount_pct, line_total, sort_order, created_at, updated_at, deleted_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
             ON CONFLICT(id) DO UPDATE SET offer_id=excluded.offer_id, description=excluded.description, qty=excluded.qty, unit_price=excluded.unit_price, discount_pct=excluded.discount_pct, line_total=excluded.line_total, sort_order=excluded.sort_order, updated_at=excluded.updated_at, deleted_at=excluded.deleted_at",
            rusqlite::params![item.id, item.offer_id, item.clinic_id, item.description, item.qty, item.unit_price, item.discount_pct, item.line_total, item.sort_order, item.created_at, item.updated_at, item.deleted_at],
        )?;
        Ok(())
    }

    pub fn offer_set_status(&self, id: &str, status: &str, invoice_id: Option<&str>, now: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE offers SET status=?1, invoice_id=COALESCE(?2, invoice_id), updated_at=?3 WHERE id=?4",
            rusqlite::params![status, invoice_id, now, id],
        )?;
        Ok(())
    }

    pub fn offer_queue_upsert(&self, o: &crate::models::Offer) -> anyhow::Result<()> {
        let payload = serde_json::to_string(o).map_err(|e| anyhow::anyhow!(e))?;
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        Self::queue_replace_pending_tx(&tx, "offers", &o.id, "upsert", &payload, &o.updated_at)?;
        tx.commit()?;
        Ok(())
    }

    pub fn offer_item_queue_upsert(&self, item: &crate::models::OfferItem) -> anyhow::Result<()> {
        let payload = serde_json::to_string(item).map_err(|e| anyhow::anyhow!(e))?;
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        Self::queue_replace_pending_tx(&tx, "offer_items", &item.id, "upsert", &payload, &item.updated_at)?;
        tx.commit()?;
        Ok(())
    }

    pub fn lab_inbox_insert(
        &self,
        profile_id: &str,
        patient_ref_raw: &str,
        formatted_text: &str,
        raw_message: &str,
    ) -> anyhow::Result<crate::models::LabInboxItem> {
        let conn = self.conn()?;
        let id = Uuid::new_v4().to_string();
        let received_at = now_iso();
        conn.execute(
            "INSERT INTO lab_inbox (id, profile_id, patient_ref_raw, formatted_text, raw_message, status, received_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'unmatched', ?6)",
            params![id, profile_id, patient_ref_raw, formatted_text, raw_message, received_at],
        )?;
        Ok(crate::models::LabInboxItem {
            id,
            profile_id: profile_id.to_string(),
            patient_ref_raw: patient_ref_raw.to_string(),
            formatted_text: formatted_text.to_string(),
            matched_client_id: None,
            matched_visit_id: None,
            status: "unmatched".to_string(),
            received_at,
        })
    }

    pub fn lab_inbox_list(&self, only_unmatched: bool) -> anyhow::Result<Vec<crate::models::LabInboxItem>> {
        let conn = self.conn()?;
        let sql = if only_unmatched {
            "SELECT id, profile_id, patient_ref_raw, formatted_text, matched_client_id, matched_visit_id, status, received_at
             FROM lab_inbox WHERE status = 'unmatched' ORDER BY received_at DESC"
        } else {
            "SELECT id, profile_id, patient_ref_raw, formatted_text, matched_client_id, matched_visit_id, status, received_at
             FROM lab_inbox ORDER BY received_at DESC"
        };
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([], |row| {
            Ok(crate::models::LabInboxItem {
                id: row.get(0)?,
                profile_id: row.get(1)?,
                patient_ref_raw: row.get(2)?,
                formatted_text: row.get(3)?,
                matched_client_id: row.get(4)?,
                matched_visit_id: row.get(5)?,
                status: row.get(6)?,
                received_at: row.get(7)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Bashkangjit tekstin e formatuar te fusha `analyses` e vizites se dhene
    /// (permes visits_upsert ekzistues, qe kujdeset vete per sync_queue), dhe
    /// e shenon rezultatin si 'assigned' ne lab_inbox.
    pub fn lab_inbox_assign_to_visit(&self, inbox_id: &str, visit_id: &str) -> anyhow::Result<()> {
        let item = {
            let conn = self.conn()?;
            conn.query_row(
                "SELECT profile_id, patient_ref_raw, formatted_text, matched_client_id, matched_visit_id, status, received_at
                 FROM lab_inbox WHERE id = ?1",
                params![inbox_id],
                |row| {
                    Ok(crate::models::LabInboxItem {
                        id: inbox_id.to_string(),
                        profile_id: row.get(0)?,
                        patient_ref_raw: row.get(1)?,
                        formatted_text: row.get(2)?,
                        matched_client_id: row.get(3)?,
                        matched_visit_id: row.get(4)?,
                        status: row.get(5)?,
                        received_at: row.get(6)?,
                    })
                },
            )
            .optional()?
        };
        let item = item.ok_or_else(|| anyhow!("lab_inbox item not found: {inbox_id}"))?;

        let visit = self
            .visits_get(visit_id)?
            .ok_or_else(|| anyhow!("visit not found: {visit_id}"))?;

        let merged_analyses = match &visit.analyses {
            Some(existing) if !existing.trim().is_empty() => {
                format!("{existing}\n\n{}", item.formatted_text)
            }
            _ => item.formatted_text.clone(),
        };

        let matched_client_id = visit.client_id.clone();
        let input = crate::models::VisitUpsertInput {
            id: Some(visit.id.clone()),
            client_id: visit.client_id,
            doctor_id: visit.doctor_id,
            date: visit.date,
            visit_time: visit.visit_time,
            status: visit.status,
            notes: visit.notes,
            body_weight: visit.body_weight,
            body_weight_unit: visit.body_weight_unit,
            body_height: visit.body_height,
            body_height_unit: visit.body_height_unit,
            head_circumference: visit.head_circumference,
            head_circumference_unit: visit.head_circumference_unit,
            body_temperature: visit.body_temperature,
            body_temperature_unit: visit.body_temperature_unit,
            blood_oxygen: visit.blood_oxygen,
            blood_oxygen_unit: visit.blood_oxygen_unit,
            glycemia: visit.glycemia,
            glycemia_unit: visit.glycemia_unit,
            pulse: visit.pulse,
            pulse_unit: visit.pulse_unit,
            bmi: visit.bmi,
            blood_pressure_systolic: visit.blood_pressure_systolic,
            blood_pressure_diastolic: visit.blood_pressure_diastolic,
            blood_pressure_unit: visit.blood_pressure_unit,
            complaints: visit.complaints,
            additional_notes: visit.additional_notes,
            controls: visit.controls,
            remarks: visit.remarks,
            analyses: Some(merged_analyses),
            advice: visit.advice,
            therapies: visit.therapies,
            diagnosis: visit.diagnosis,
            examinations: visit.examinations,
            specialty_report: visit.specialty_report,
        };
        self.visits_upsert(input)?;

        let conn = self.conn()?;
        conn.execute(
            "UPDATE lab_inbox SET status = 'assigned', matched_visit_id = ?2, matched_client_id = ?3 WHERE id = ?1",
            params![inbox_id, visit_id, matched_client_id],
        )?;
        Ok(())
    }
}
