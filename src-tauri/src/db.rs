use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{anyhow, bail, Context};
use rusqlite::{params, Connection, OptionalExtension, OpenFlags};
use uuid::Uuid;

use crate::models::{
  Appointment, AppointmentUpsertInput, AppointmentsListFilters, CashEntry, CashEntryUpsertInput, CashListFilters,
  Client, ClientUpsertInput, Doctor, DoctorUpsertInput, Payment, PaymentUpsertInput, PaymentsListFilters, Sale,
  SaleUpsertInput, SalesListFilters, Service, ServiceUpsertInput, SyncQueueItem, Visit, VisitItem,
  VisitItemUpsertInput, VisitItemsListFilters, VisitUpsertInput, VisitsListFilters,
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
  status TEXT NOT NULL,
  notes TEXT,
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
"#;

pub struct Db {
  db_path: PathBuf,
  conn: Mutex<Connection>,
}

impl Db {
  pub fn new(db_path: PathBuf) -> anyhow::Result<Self> {
    if let Some(parent) = db_path.parent() {
      fs::create_dir_all(parent).with_context(|| format!("create db dir: {}", parent.display()))?;
    }

    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_FULL_MUTEX;
    let conn = Connection::open_with_flags(&db_path, flags).with_context(|| format!("open sqlite: {}", db_path.display()))?;
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.execute_batch(MIGRATION_SQL)?;
    Self::run_migrations(&conn)?;

    Ok(Self {
      db_path,
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

  fn add_column_if_missing(conn: &Connection, table: &str, column: &str, ddl: &str) -> anyhow::Result<()> {
    if Self::has_column(conn, table, column)? {
      return Ok(());
    }
    conn.execute(&format!("ALTER TABLE {table} ADD COLUMN {column} {ddl}"), [])?;
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

    // services
    Self::add_column_if_missing(conn, "services", "vat_code", "TEXT NOT NULL DEFAULT 'C'")?;

    // sales
    Self::add_column_if_missing(conn, "sales", "fiscalized", "INTEGER NOT NULL DEFAULT 0")?;
    Self::add_column_if_missing(conn, "sales", "fiscalized_at", "TEXT")?;

    // visits
    for (col, ddl) in [
      ("complaints", "TEXT"),
      ("additional_notes", "TEXT"),
      ("controls", "TEXT"),
      ("remarks", "TEXT"),
      ("analyses", "TEXT"),
      ("advice", "TEXT"),
      ("therapies", "TEXT"),
      ("diagnosis", "TEXT"),
      ("examinations", "TEXT"),
    ] {
      Self::add_column_if_missing(conn, "visits", col, ddl)?;
    }

    // visit_items
    Self::add_column_if_missing(conn, "visit_items", "vat_code", "TEXT NOT NULL DEFAULT 'C'")?;
    Self::add_column_if_missing(conn, "visit_items", "fiscalized", "INTEGER NOT NULL DEFAULT 0")?;
    Self::add_column_if_missing(conn, "visit_items", "fiscalized_at", "TEXT")?;

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
      "#,
    )?;

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
    Ok(
      conn
        .query_row("SELECT value FROM app_settings WHERE key = ?1", params![key], |row| row.get::<_, Option<String>>(0))
        .optional()?
        .flatten(),
    )
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
    let n: i64 = conn.query_row("SELECT COUNT(1) FROM sync_queue WHERE status = 'pending'", [], |row| row.get(0))?;
    Ok(n)
  }

  pub fn sync_queue_list_pending(&self, limit: usize) -> anyhow::Result<Vec<SyncQueueItem>> {
    let conn = self.conn()?;
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
    conn.execute("UPDATE sync_queue SET status='sent', last_error=NULL WHERE id=?1", params![id])?;
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

  pub fn sync_queue_drop_pending_for_row(&self, table: &str, row_id: &str) -> anyhow::Result<()> {
    let conn = self.conn()?;
    conn.execute(
      "DELETE FROM sync_queue WHERE table_name=?1 AND row_id=?2 AND status IN ('pending','failed')",
      params![table, row_id],
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
      params![Uuid::new_v4().to_string(), table, row_id, op, payload_json, created_at],
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
      .query_row("SELECT created_at FROM clients WHERE id=?1", params![id], |row| row.get(0))
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
    tx.execute("UPDATE clients SET deleted=1, updated_at=?2 WHERE id=?1", params![id, now])?;
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
  pub fn sales_list_fiscal_only(&self, filters: Option<SalesListFilters>) -> anyhow::Result<Vec<Sale>> {
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

  pub fn sales_daily_report(&self, date: &str) -> anyhow::Result<crate::models::DailySalesReport> {
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

  pub fn sales_daily_report_for_doctor(&self, date: &str, doctor_id: &str) -> anyhow::Result<crate::models::DailySalesReport> {
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
    let created_at = existing.as_ref().map(|x| x.0.clone()).unwrap_or_else(|| now.clone());
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
    tx.execute("UPDATE sales SET deleted=1, updated_at=?2 WHERE id=?1", params![id, now])?;
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
      path.file_name().and_then(|x| x.to_str()).unwrap_or("fiscal.inp"),
      &Uuid::new_v4().to_string()[..8]
    );
    let tmp_path = path.with_file_name(tmp_name);

    {
      let mut f = fs::File::create(&tmp_path).with_context(|| format!("create tmp inp: {}", tmp_path.display()))?;
      f.write_all(body.as_bytes())
        .with_context(|| format!("write tmp inp: {}", tmp_path.display()))?;
      f.flush().with_context(|| format!("flush tmp inp: {}", tmp_path.display()))?;
      let _ = f.sync_all();
    }

    fs::rename(&tmp_path, path).with_context(|| format!("rename tmp->inp: {} -> {}", tmp_path.display(), path.display()))?;
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
      fs::create_dir_all(parent).with_context(|| format!("create clear-article dir: {}", parent.display()))?;
    }
    Self::fiscal_write_inp_atomic(&path, "O,1,______,_,__;ALL\n")?;
    Ok(path)
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

  fn fiscal_out_note_status_2(raw: &str) -> bool {
    raw.to_ascii_lowercase().contains("notestatus;2")
  }

  pub fn fiscal_receipt_generate_inp(&self, sale_id: &str, output_dir: &Path) -> anyhow::Result<PathBuf> {
    let sale_id = sale_id.trim();
    if sale_id.is_empty() {
      bail!("sale_id eshte i detyrueshem");
    }

    fs::create_dir_all(output_dir).with_context(|| format!("create fiscal dir: {}", output_dir.display()))?;

    let vat_e_group: i64 = self
      .setting_get("fiscal_vat_e_group")?
      .and_then(|v| v.trim().parse::<i64>().ok())
      .filter(|v| *v == 2 || *v == 4 || *v == 5)
      .unwrap_or(5);

    let sale: Sale;
    let mut items: Vec<VisitItem> = Vec::new();
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

      let client_exists: Option<String> = conn
        .query_row("SELECT id FROM clients WHERE id=?1", params![&sale.client_id], |row| row.get(0))
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
    }
    if items.is_empty() {
      bail!("nuk ka rreshta fiskal pa fiskalizuar");
    }

    // Required by fiscal flow: clear article list before printing receipt.
    Self::fiscal_emit_clear_article_command()?;

    // Step A: check if there is an open note. If NoteStatus=2, close with N before real sale.
    let g_path = output_dir.join(format!("status-{}-{}.inp", sale.id, &Uuid::new_v4().to_string()[..8]));
    Self::fiscal_write_inp_atomic(&g_path, "G,1,______,_,__;NoteStatus\n")?;
    if let Some(out) = Self::fiscal_wait_out_text(&g_path, Duration::from_secs(4)) {
      if Self::fiscal_out_note_status_2(&out) {
        let n_path = output_dir.join(format!("cancel-open-{}-{}.inp", sale.id, &Uuid::new_v4().to_string()[..8]));
        Self::fiscal_write_inp_atomic(&n_path, "N,1,______,_,__;\n")?;
        let _ = Self::fiscal_wait_out_text(&n_path, Duration::from_secs(3));
      }
    }

    // Real sale lines: only actual sale items (no test rows).
    let mut sale_body = String::new();
    let mut total = 0.0_f64;
    for (idx, it) in items.iter().enumerate() {
      let qty = if it.qty.is_finite() && it.qty > 0.0 { it.qty } else { 1.0 };
      let unit_price = if it.unit_price.is_finite() && it.unit_price >= 0.0 {
        it.unit_price
      } else {
        0.0
      };
      let sub = qty * unit_price;
      total += sub;

      let desc = Self::fiscal_sanitize_text(&it.title);
      let vat_group = Self::fiscal_vat_group_for_code(&it.vat_code, vat_e_group);
      let item_code = 15001_i64 + idx as i64;
      sale_body.push_str(&format!(
        "S,1,______,_,__;{};{:.2};{};1;1;{};0;{};0;0\n",
        desc,
        unit_price,
        Self::fiscal_qty_str(qty),
        vat_group,
        item_code
      ));
    }
    sale_body.push_str("T,1,______,_,__;\n");

    let primary_path = output_dir.join(format!("kupon-{}-{}.inp", sale.id, &Uuid::new_v4().to_string()[..8]));
    Self::fiscal_write_inp_atomic(&primary_path, &sale_body)?;
    let primary_out = Self::fiscal_wait_out_text(&primary_path, Duration::from_secs(8));

    // Step B: if S/T failed, fallback with K + real S/T.
    let mut final_path = primary_path.clone();
    let mut failed_after_fallback = false;
    if let Some(out) = primary_out.as_deref() {
      if Self::fiscal_out_has_error(out) {
        let mut k_body = String::from("K,1,______,_,__;;1;0000;;;;;;1;\n");
        k_body.push_str(&sale_body);
        let k_path = output_dir.join(format!("kupon-k-{}-{}.inp", sale.id, &Uuid::new_v4().to_string()[..8]));
        Self::fiscal_write_inp_atomic(&k_path, &k_body)?;
        final_path = k_path.clone();
        let k_out = Self::fiscal_wait_out_text(&k_path, Duration::from_secs(8));
        if let Some(k_out) = k_out.as_deref() {
          if Self::fiscal_out_has_error(k_out) {
            failed_after_fallback = true;
          }
        }
      }
    }

    // Step C: if still failing, try forced close T(cash) and then N.
    if failed_after_fallback {
      let close_body = format!("T,1,______,_,__;0;{:.2};;;;\nN,1,______,_,__;\n", total);
      let close_path = output_dir.join(format!("kupon-close-{}-{}.inp", sale.id, &Uuid::new_v4().to_string()[..8]));
      Self::fiscal_write_inp_atomic(&close_path, &close_body)?;
      let _ = Self::fiscal_wait_out_text(&close_path, Duration::from_secs(6));
      let _ = Self::fiscal_emit_clear_article_command();
      bail!("fiskalizimi deshtoi edhe pas fallback (K dhe mbyllja T/N).");
    }

    // Required by fiscal flow: clear article list after printing receipt.
    Self::fiscal_emit_clear_article_command()?;

    // Mark items as fiscalized and queue them for sync.
    let now = now_iso();
    let mut conn = self.conn()?;
    let tx = conn.transaction()?;
    for it in &mut items {
      tx.execute(
        "UPDATE visit_items SET fiscalized=1, fiscalized_at=?2, updated_at=?2 WHERE id=?1",
        params![&it.id, &now],
      )?;
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

    tx.commit()?;
    Ok(final_path)
  }

  pub fn payments_list(&self, filters: Option<PaymentsListFilters>) -> anyhow::Result<Vec<Payment>> {
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
        method: row.get::<_, Option<String>>(5)?.unwrap_or_else(|| "cash".to_string()),
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
      .query_row("SELECT created_at FROM payments WHERE id=?1", params![id], |row| row.get(0))
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
    tx.execute("UPDATE payments SET deleted=1, updated_at=?2 WHERE id=?1", params![id, now])?;
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

  pub fn doctor_account_get(&self, doctor_id: &str) -> anyhow::Result<Option<(String, String, bool)>> {
    let conn = self.conn()?;
    Ok(
      conn
        .query_row(
          "SELECT salt, password_hash, is_admin FROM doctor_accounts WHERE doctor_id=?1",
          params![doctor_id],
          |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)? == 1)),
        )
        .optional()?,
    )
  }

  pub fn doctor_account_set(
    &self,
    doctor_id: &str,
    salt: &str,
    password_hash: &str,
    is_admin: bool,
    now: &str,
  ) -> anyhow::Result<()> {
    let conn = self.conn()?;
    conn.execute(
      "INSERT INTO doctor_accounts (doctor_id, salt, password_hash, is_admin, created_at, updated_at)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6)
       ON CONFLICT(doctor_id) DO UPDATE SET
         salt=excluded.salt,
         password_hash=excluded.password_hash,
         is_admin=excluded.is_admin,
         updated_at=excluded.updated_at",
      params![doctor_id, salt, password_hash, if is_admin { 1 } else { 0 }, now, now],
    )?;
    Ok(())
  }

  pub fn doctor_account_delete(&self, doctor_id: &str) -> anyhow::Result<()> {
    let conn = self.conn()?;
    conn.execute("DELETE FROM doctor_accounts WHERE doctor_id=?1", params![doctor_id])?;
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
    let code = input.code.map(|x| x.trim().to_string()).filter(|x| !x.is_empty());
    let title = input.title.map(|x| x.trim().to_string()).filter(|x| !x.is_empty());
    let specialty = input.specialty.map(|x| x.trim().to_string()).filter(|x| !x.is_empty());
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
      .query_row("SELECT created_at FROM doctors WHERE id=?1", params![id], |row| row.get(0))
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
    tx.execute("UPDATE doctors SET deleted=1, updated_at=?2 WHERE id=?1", params![id, now])?;
    // Local-only: remove login credentials when a doctor is deleted.
    let _ = tx.execute("DELETE FROM doctor_accounts WHERE doctor_id=?1", params![id]);
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

    let mut vat_code = input.vat_code.unwrap_or_else(|| "".to_string()).trim().to_uppercase();
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
      .query_row("SELECT created_at FROM services WHERE id=?1", params![id], |row| row.get(0))
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
    tx.execute("UPDATE services SET deleted=1, updated_at=?2 WHERE id=?1", params![id, now])?;
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

  pub fn appointments_list(&self, filters: Option<AppointmentsListFilters>) -> anyhow::Result<Vec<Appointment>> {
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

  pub fn appointments_upsert(&self, input: AppointmentUpsertInput) -> anyhow::Result<Appointment> {
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
      .query_row("SELECT created_at FROM appointments WHERE id=?1", params![id], |row| row.get(0))
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
    tx.execute("UPDATE appointments SET deleted=1, updated_at=?2 WHERE id=?1", params![id, now])?;
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
      "SELECT id, client_id, doctor_id, date, status, notes, complaints, additional_notes, controls, remarks, analyses, advice, therapies, diagnosis, examinations, created_at, updated_at, deleted
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
        status: row.get(4)?,
        notes: row.get(5)?,
        complaints: row.get(6)?,
        additional_notes: row.get(7)?,
        controls: row.get(8)?,
        remarks: row.get(9)?,
        analyses: row.get(10)?,
        advice: row.get(11)?,
        therapies: row.get(12)?,
        diagnosis: row.get(13)?,
        examinations: row.get(14)?,
        created_at: row.get(15)?,
        updated_at: row.get(16)?,
        deleted: row.get(17)?,
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
          "SELECT id, client_id, doctor_id, date, status, notes, complaints, additional_notes, controls, remarks, analyses, advice, therapies, diagnosis, examinations, created_at, updated_at, deleted
           FROM visits WHERE id=?1",
          params![id],
          |row| {
            Ok(Visit {
              id: row.get(0)?,
              client_id: row.get(1)?,
              doctor_id: row.get(2)?,
              date: row.get(3)?,
              status: row.get(4)?,
              notes: row.get(5)?,
              complaints: row.get(6)?,
              additional_notes: row.get(7)?,
              controls: row.get(8)?,
              remarks: row.get(9)?,
              analyses: row.get(10)?,
              advice: row.get(11)?,
              therapies: row.get(12)?,
              diagnosis: row.get(13)?,
              examinations: row.get(14)?,
              created_at: row.get(15)?,
              updated_at: row.get(16)?,
              deleted: row.get(17)?,
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
    let notes = input.notes;
    let complaints = input.complaints;
    let additional_notes = input.additional_notes;
    let controls = input.controls;
    let remarks = input.remarks;
    let analyses = input.analyses;
    let advice = input.advice;
    let therapies = input.therapies;
    let diagnosis = input.diagnosis;
    let examinations = input.examinations;
    let now = now_iso();

    let mut conn = self.conn()?;
    let tx = conn.transaction()?;
    let existing_created_at: Option<String> = tx
      .query_row("SELECT created_at FROM visits WHERE id=?1", params![id], |row| row.get(0))
      .optional()?;
    let created_at = existing_created_at.unwrap_or_else(|| now.clone());

    tx.execute(
      "INSERT INTO visits (id, client_id, doctor_id, date, status, notes, complaints, additional_notes, controls, remarks, analyses, advice, therapies, diagnosis, examinations, created_at, updated_at, deleted)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, 0)
       ON CONFLICT(id) DO UPDATE SET
         client_id=excluded.client_id,
         doctor_id=excluded.doctor_id,
         date=excluded.date,
         status=excluded.status,
         notes=excluded.notes,
         complaints=excluded.complaints,
         additional_notes=excluded.additional_notes,
         controls=excluded.controls,
         remarks=excluded.remarks,
         analyses=excluded.analyses,
         advice=excluded.advice,
         therapies=excluded.therapies,
         diagnosis=excluded.diagnosis,
         examinations=excluded.examinations,
         updated_at=excluded.updated_at,
         deleted=excluded.deleted",
      params![
        id,
        &client_id,
        &doctor_id,
        &date,
        &status,
        &notes,
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
        &now
      ],
    )?;
    let row = Visit {
      id: id.clone(),
      client_id,
      doctor_id,
      date,
      status,
      notes,
      complaints,
      additional_notes,
      controls,
      remarks,
      analyses,
      advice,
      therapies,
      diagnosis,
      examinations,
      created_at: created_at.clone(),
      updated_at: now.clone(),
      deleted: 0,
    };
    let payload = serde_json::to_string(&row)?;
    Self::queue_replace_pending_tx(&tx, "visits", &row.id, "upsert", &payload, &now)?;
    tx.commit()?;
    Ok(row)
  }

  pub fn visits_delete(&self, id: &str) -> anyhow::Result<()> {
    if id.trim().is_empty() {
      bail!("visit id is required");
    }
    let now = now_iso();
    let mut conn = self.conn()?;
    let tx = conn.transaction()?;
    tx.execute("UPDATE visits SET deleted=1, updated_at=?2 WHERE id=?1", params![id, now])?;
    let row = tx
      .query_row(
        "SELECT id, client_id, doctor_id, date, status, notes, complaints, additional_notes, controls, remarks, analyses, advice, therapies, diagnosis, examinations, created_at, updated_at, deleted FROM visits WHERE id=?1",
        params![id],
        |r| {
          Ok(Visit {
            id: r.get(0)?,
            client_id: r.get(1)?,
            doctor_id: r.get(2)?,
            date: r.get(3)?,
            status: r.get(4)?,
            notes: r.get(5)?,
            complaints: r.get(6)?,
            additional_notes: r.get(7)?,
            controls: r.get(8)?,
            remarks: r.get(9)?,
            analyses: r.get(10)?,
            advice: r.get(11)?,
            therapies: r.get(12)?,
            diagnosis: r.get(13)?,
            examinations: r.get(14)?,
            created_at: r.get(15)?,
            updated_at: r.get(16)?,
            deleted: r.get(17)?,
          })
        },
      )
      .optional()?
      .ok_or_else(|| anyhow!("visit not found"))?;
    let payload = serde_json::to_string(&row)?;
    Self::queue_replace_pending_tx(&tx, "visits", &row.id, "delete", &payload, &now)?;
    tx.commit()?;
    Ok(())
  }

  pub fn visit_items_list(&self, filters: Option<VisitItemsListFilters>) -> anyhow::Result<Vec<VisitItem>> {
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

    let mut vat_code = input.vat_code.unwrap_or_else(|| "".to_string()).trim().to_uppercase();
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
    let created_at = existing.as_ref().map(|x| x.0.clone()).unwrap_or_else(|| now.clone());

    // Preserve fiscalization only when the fiscal line hasn't changed.
    let (fiscalized, fiscalized_at) = if fiscal == 1 {
      if let Some((_, ex_fisc, ex_at, ex_title, ex_qty, ex_price, ex_vat)) = existing.as_ref() {
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
    tx.execute("UPDATE visit_items SET deleted=1, updated_at=?2 WHERE id=?1", params![id, now])?;
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
      .query_row("SELECT created_at FROM cash_ledger WHERE id=?1", params![id], |row| row.get(0))
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
    tx.execute("UPDATE cash_ledger SET deleted=1, updated_at=?2 WHERE id=?1", params![id, now])?;
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
    Ok(
      conn
        .query_row("SELECT updated_at FROM clients WHERE id=?1", params![id], |row| row.get::<_, Option<String>>(0))
        .optional()?
        .flatten(),
    )
  }

  pub fn sales_updated_at(&self, id: &str) -> anyhow::Result<Option<String>> {
    let conn = self.conn()?;
    Ok(
      conn
        .query_row("SELECT updated_at FROM sales WHERE id=?1", params![id], |row| row.get::<_, Option<String>>(0))
        .optional()?
        .flatten(),
    )
  }

  pub fn payments_updated_at(&self, id: &str) -> anyhow::Result<Option<String>> {
    let conn = self.conn()?;
    Ok(
      conn
        .query_row("SELECT updated_at FROM payments WHERE id=?1", params![id], |row| row.get::<_, Option<String>>(0))
        .optional()?
        .flatten(),
    )
  }

  pub fn doctors_updated_at(&self, id: &str) -> anyhow::Result<Option<String>> {
    let conn = self.conn()?;
    Ok(
      conn
        .query_row("SELECT updated_at FROM doctors WHERE id=?1", params![id], |row| row.get::<_, Option<String>>(0))
        .optional()?
        .flatten(),
    )
  }

  pub fn services_updated_at(&self, id: &str) -> anyhow::Result<Option<String>> {
    let conn = self.conn()?;
    Ok(
      conn
        .query_row("SELECT updated_at FROM services WHERE id=?1", params![id], |row| row.get::<_, Option<String>>(0))
        .optional()?
        .flatten(),
    )
  }

  pub fn appointments_updated_at(&self, id: &str) -> anyhow::Result<Option<String>> {
    let conn = self.conn()?;
    Ok(
      conn
        .query_row(
          "SELECT updated_at FROM appointments WHERE id=?1",
          params![id],
          |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten(),
    )
  }

  pub fn visits_updated_at(&self, id: &str) -> anyhow::Result<Option<String>> {
    let conn = self.conn()?;
    Ok(
      conn
        .query_row("SELECT updated_at FROM visits WHERE id=?1", params![id], |row| row.get::<_, Option<String>>(0))
        .optional()?
        .flatten(),
    )
  }

  pub fn visit_items_updated_at(&self, id: &str) -> anyhow::Result<Option<String>> {
    let conn = self.conn()?;
    Ok(
      conn
        .query_row(
          "SELECT updated_at FROM visit_items WHERE id=?1",
          params![id],
          |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten(),
    )
  }

  pub fn cash_updated_at(&self, id: &str) -> anyhow::Result<Option<String>> {
    let conn = self.conn()?;
    Ok(
      conn
        .query_row(
          "SELECT updated_at FROM cash_ledger WHERE id=?1",
          params![id],
          |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten(),
    )
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
      "INSERT INTO visits (id, client_id, doctor_id, date, status, notes, complaints, additional_notes, controls, remarks, analyses, advice, therapies, diagnosis, examinations, created_at, updated_at, deleted)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
       ON CONFLICT(id) DO UPDATE SET
         client_id=excluded.client_id,
         doctor_id=excluded.doctor_id,
         date=excluded.date,
         status=excluded.status,
         notes=excluded.notes,
         complaints=excluded.complaints,
         additional_notes=excluded.additional_notes,
         controls=excluded.controls,
         remarks=excluded.remarks,
         analyses=excluded.analyses,
         advice=excluded.advice,
         therapies=excluded.therapies,
         diagnosis=excluded.diagnosis,
         examinations=excluded.examinations,
         created_at=excluded.created_at,
         updated_at=excluded.updated_at,
         deleted=excluded.deleted",
      params![
        row.id,
        row.client_id,
        row.doctor_id,
        row.date,
        row.status,
        row.notes,
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
        row.deleted
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
}
