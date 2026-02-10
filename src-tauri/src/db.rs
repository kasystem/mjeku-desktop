use std::collections::HashMap;
use std::fs;
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
  name TEXT NOT NULL,
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

CREATE INDEX IF NOT EXISTS idx_clients_updated_at ON clients(updated_at);

CREATE INDEX IF NOT EXISTS idx_sales_client_id ON sales(client_id);
CREATE INDEX IF NOT EXISTS idx_sales_date ON sales(date);
CREATE INDEX IF NOT EXISTS idx_sales_updated_at ON sales(updated_at);

CREATE INDEX IF NOT EXISTS idx_payments_client_id ON payments(client_id);
CREATE INDEX IF NOT EXISTS idx_payments_sale_id ON payments(sale_id);
CREATE INDEX IF NOT EXISTS idx_payments_date ON payments(date);
CREATE INDEX IF NOT EXISTS idx_payments_updated_at ON payments(updated_at);

CREATE INDEX IF NOT EXISTS idx_sync_queue_status ON sync_queue(status);
CREATE INDEX IF NOT EXISTS idx_sync_queue_created_at ON sync_queue(created_at);
CREATE INDEX IF NOT EXISTS idx_sync_queue_table_row ON sync_queue(table_name, row_id);

CREATE INDEX IF NOT EXISTS idx_doctors_updated_at ON doctors(updated_at);

CREATE INDEX IF NOT EXISTS idx_doctor_accounts_is_admin ON doctor_accounts(is_admin);
CREATE INDEX IF NOT EXISTS idx_doctor_accounts_updated_at ON doctor_accounts(updated_at);

CREATE INDEX IF NOT EXISTS idx_services_updated_at ON services(updated_at);

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

CREATE INDEX IF NOT EXISTS idx_cash_ledger_type ON cash_ledger(type);
CREATE INDEX IF NOT EXISTS idx_cash_ledger_date ON cash_ledger(date);
CREATE INDEX IF NOT EXISTS idx_cash_ledger_updated_at ON cash_ledger(updated_at);
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
        "SELECT id, name, phone, email, notes, created_at, updated_at, deleted
         FROM clients
         WHERE deleted = 0 AND (name LIKE ?1 OR phone LIKE ?1 OR email LIKE ?1)
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
      "SELECT id, name, phone, email, notes, created_at, updated_at, deleted
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

  pub fn clients_get(&self, id: &str) -> anyhow::Result<Option<Client>> {
    let conn = self.conn()?;
    Ok(
      conn
        .query_row(
          "SELECT id, name, phone, email, notes, created_at, updated_at, deleted FROM clients WHERE id=?1",
          params![id],
          |row| {
            Ok(Client {
              id: row.get(0)?,
              name: row.get(1)?,
              phone: row.get(2)?,
              email: row.get(3)?,
              notes: row.get(4)?,
              created_at: row.get(5)?,
              updated_at: row.get(6)?,
              deleted: row.get(7)?,
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
    let now = now_iso();

    let mut conn = self.conn()?;
    let tx = conn.transaction()?;
    let existing_created_at: Option<String> = tx
      .query_row("SELECT created_at FROM clients WHERE id=?1", params![id], |row| row.get(0))
      .optional()?;
    let created_at = existing_created_at.unwrap_or_else(|| now.clone());

    tx.execute(
      "INSERT INTO clients (id, name, phone, email, notes, created_at, updated_at, deleted)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)
       ON CONFLICT(id) DO UPDATE SET
         name=excluded.name,
         phone=excluded.phone,
         email=excluded.email,
         notes=excluded.notes,
         updated_at=excluded.updated_at,
         deleted=excluded.deleted",
      params![id, name, &phone, &email, &notes, &created_at, &now],
    )?;
    let row = Client {
      id: id.clone(),
      name: name.to_string(),
      phone,
      email,
      notes,
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
        "SELECT id, name, phone, email, notes, created_at, updated_at, deleted FROM clients WHERE id=?1",
        params![id],
        |r| {
          Ok(Client {
            id: r.get(0)?,
            name: r.get(1)?,
            phone: r.get(2)?,
            email: r.get(3)?,
            notes: r.get(4)?,
            created_at: r.get(5)?,
            updated_at: r.get(6)?,
            deleted: r.get(7)?,
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
      "SELECT id, client_id, date, total, notes, created_at, updated_at, deleted
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

  pub fn sales_get(&self, id: &str) -> anyhow::Result<Option<Sale>> {
    let conn = self.conn()?;
    Ok(
      conn
        .query_row(
          "SELECT id, client_id, date, total, notes, created_at, updated_at, deleted FROM sales WHERE id=?1",
          params![id],
          |row| {
            Ok(Sale {
              id: row.get(0)?,
              client_id: row.get(1)?,
              date: row.get(2)?,
              total: row.get(3)?,
              notes: row.get(4)?,
              created_at: row.get(5)?,
              updated_at: row.get(6)?,
              deleted: row.get(7)?,
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
    let existing_created_at: Option<String> = tx
      .query_row("SELECT created_at FROM sales WHERE id=?1", params![id], |row| row.get(0))
      .optional()?;
    let created_at = existing_created_at.unwrap_or_else(|| now.clone());

    tx.execute(
      "INSERT INTO sales (id, client_id, date, total, notes, created_at, updated_at, deleted)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)
       ON CONFLICT(id) DO UPDATE SET
         client_id=excluded.client_id,
         date=excluded.date,
         total=excluded.total,
         notes=excluded.notes,
         updated_at=excluded.updated_at,
         deleted=excluded.deleted",
      params![id, &client_id, &date, total, &notes, &created_at, &now],
    )?;
    let row = Sale {
      id: id.clone(),
      client_id,
      date,
      total,
      notes,
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
        "SELECT id, client_id, date, total, notes, created_at, updated_at, deleted FROM sales WHERE id=?1",
        params![id],
        |r| {
          Ok(Sale {
            id: r.get(0)?,
            client_id: r.get(1)?,
            date: r.get(2)?,
            total: r.get(3)?,
            notes: r.get(4)?,
            created_at: r.get(5)?,
            updated_at: r.get(6)?,
            deleted: r.get(7)?,
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
          "SELECT id, name, phone, email, notes, created_at, updated_at, deleted FROM doctors WHERE id=?1",
          params![id],
          |row| {
            Ok(Doctor {
              id: row.get(0)?,
              name: row.get(1)?,
              phone: row.get(2)?,
              email: row.get(3)?,
              notes: row.get(4)?,
              created_at: row.get(5)?,
              updated_at: row.get(6)?,
              deleted: row.get(7)?,
            })
          },
        )
        .optional()?,
    )
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
        "SELECT id, name, phone, email, notes, created_at, updated_at, deleted
         FROM doctors
         WHERE deleted = 0 AND (name LIKE ?1 OR phone LIKE ?1 OR email LIKE ?1)
         ORDER BY updated_at DESC
         LIMIT 1000",
      )?;
      let rows = stmt.query_map(params![like], |row| {
        Ok(Doctor {
          id: row.get(0)?,
          name: row.get(1)?,
          phone: row.get(2)?,
          email: row.get(3)?,
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
      "SELECT id, name, phone, email, notes, created_at, updated_at, deleted
       FROM doctors
       WHERE deleted = 0
       ORDER BY updated_at DESC
       LIMIT 1000",
    )?;
    let rows = stmt.query_map([], |row| {
      Ok(Doctor {
        id: row.get(0)?,
        name: row.get(1)?,
        phone: row.get(2)?,
        email: row.get(3)?,
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

  pub fn doctors_login_options(&self) -> anyhow::Result<Vec<crate::models::DoctorLoginOption>> {
    let conn = self.conn()?;
    let mut stmt = conn.prepare(
      "SELECT d.id, d.name,
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
        name: row.get(1)?,
        has_account: row.get::<_, i64>(2)? == 1,
        is_admin: row.get::<_, i64>(3)? == 1,
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
    let phone = input.phone;
    let email = input.email;
    let notes = input.notes;
    let now = now_iso();

    let mut conn = self.conn()?;
    let tx = conn.transaction()?;
    let existing_created_at: Option<String> = tx
      .query_row("SELECT created_at FROM doctors WHERE id=?1", params![id], |row| row.get(0))
      .optional()?;
    let created_at = existing_created_at.unwrap_or_else(|| now.clone());

    tx.execute(
      "INSERT INTO doctors (id, name, phone, email, notes, created_at, updated_at, deleted)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)
       ON CONFLICT(id) DO UPDATE SET
         name=excluded.name,
         phone=excluded.phone,
         email=excluded.email,
         notes=excluded.notes,
         updated_at=excluded.updated_at,
         deleted=excluded.deleted",
      params![id, name, &phone, &email, &notes, &created_at, &now],
    )?;
    let row = Doctor {
      id: id.clone(),
      name: name.to_string(),
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
        "SELECT id, name, phone, email, notes, created_at, updated_at, deleted FROM doctors WHERE id=?1",
        params![id],
        |r| {
          Ok(Doctor {
            id: r.get(0)?,
            name: r.get(1)?,
            phone: r.get(2)?,
            email: r.get(3)?,
            notes: r.get(4)?,
            created_at: r.get(5)?,
            updated_at: r.get(6)?,
            deleted: r.get(7)?,
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
        "SELECT id, title, default_price, notes, created_at, updated_at, deleted
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
          notes: row.get(3)?,
          created_at: row.get(4)?,
          updated_at: row.get(5)?,
          deleted: row.get(6)?,
        })
      })?;
      for r in rows {
        out.push(r?);
      }
      return Ok(out);
    }

    let mut stmt = conn.prepare(
      "SELECT id, title, default_price, notes, created_at, updated_at, deleted
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
        notes: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
        deleted: row.get(6)?,
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
    let notes = input.notes;
    let now = now_iso();

    let mut conn = self.conn()?;
    let tx = conn.transaction()?;
    let existing_created_at: Option<String> = tx
      .query_row("SELECT created_at FROM services WHERE id=?1", params![id], |row| row.get(0))
      .optional()?;
    let created_at = existing_created_at.unwrap_or_else(|| now.clone());

    tx.execute(
      "INSERT INTO services (id, title, default_price, notes, created_at, updated_at, deleted)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)
       ON CONFLICT(id) DO UPDATE SET
         title=excluded.title,
         default_price=excluded.default_price,
         notes=excluded.notes,
         updated_at=excluded.updated_at,
         deleted=excluded.deleted",
      params![id, title, default_price, &notes, &created_at, &now],
    )?;
    let row = Service {
      id: id.clone(),
      title: title.to_string(),
      default_price,
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
        "SELECT id, title, default_price, notes, created_at, updated_at, deleted FROM services WHERE id=?1",
        params![id],
        |r| {
          Ok(Service {
            id: r.get(0)?,
            title: r.get(1)?,
            default_price: r.get(2)?,
            notes: r.get(3)?,
            created_at: r.get(4)?,
            updated_at: r.get(5)?,
            deleted: r.get(6)?,
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
      "SELECT id, client_id, doctor_id, date, status, notes, created_at, updated_at, deleted
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

  pub fn visits_get(&self, id: &str) -> anyhow::Result<Option<Visit>> {
    let conn = self.conn()?;
    Ok(
      conn
        .query_row(
          "SELECT id, client_id, doctor_id, date, status, notes, created_at, updated_at, deleted
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
              created_at: row.get(6)?,
              updated_at: row.get(7)?,
              deleted: row.get(8)?,
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
    let now = now_iso();

    let mut conn = self.conn()?;
    let tx = conn.transaction()?;
    let existing_created_at: Option<String> = tx
      .query_row("SELECT created_at FROM visits WHERE id=?1", params![id], |row| row.get(0))
      .optional()?;
    let created_at = existing_created_at.unwrap_or_else(|| now.clone());

    tx.execute(
      "INSERT INTO visits (id, client_id, doctor_id, date, status, notes, created_at, updated_at, deleted)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0)
       ON CONFLICT(id) DO UPDATE SET
         client_id=excluded.client_id,
         doctor_id=excluded.doctor_id,
         date=excluded.date,
         status=excluded.status,
         notes=excluded.notes,
         updated_at=excluded.updated_at,
         deleted=excluded.deleted",
      params![id, &client_id, &doctor_id, &date, &status, &notes, &created_at, &now],
    )?;
    let row = Visit {
      id: id.clone(),
      client_id,
      doctor_id,
      date,
      status,
      notes,
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
        "SELECT id, client_id, doctor_id, date, status, notes, created_at, updated_at, deleted FROM visits WHERE id=?1",
        params![id],
        |r| {
          Ok(Visit {
            id: r.get(0)?,
            client_id: r.get(1)?,
            doctor_id: r.get(2)?,
            date: r.get(3)?,
            status: r.get(4)?,
            notes: r.get(5)?,
            created_at: r.get(6)?,
            updated_at: r.get(7)?,
            deleted: r.get(8)?,
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
      "SELECT id, visit_id, client_id, tooth, title, qty, unit_price, fiscal, notes, created_at, updated_at, deleted
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
        notes: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
        deleted: row.get(11)?,
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
          "SELECT id, visit_id, client_id, tooth, title, qty, unit_price, fiscal, notes, created_at, updated_at, deleted
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
              notes: row.get(8)?,
              created_at: row.get(9)?,
              updated_at: row.get(10)?,
              deleted: row.get(11)?,
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
    let notes = input.notes;
    let now = now_iso();

    let mut conn = self.conn()?;
    let tx = conn.transaction()?;
    let existing_created_at: Option<String> = tx
      .query_row("SELECT created_at FROM visit_items WHERE id=?1", params![id], |row| row.get(0))
      .optional()?;
    let created_at = existing_created_at.unwrap_or_else(|| now.clone());

    tx.execute(
      "INSERT INTO visit_items (id, visit_id, client_id, tooth, title, qty, unit_price, fiscal, notes, created_at, updated_at, deleted)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0)
       ON CONFLICT(id) DO UPDATE SET
         visit_id=excluded.visit_id,
         client_id=excluded.client_id,
         tooth=excluded.tooth,
         title=excluded.title,
         qty=excluded.qty,
         unit_price=excluded.unit_price,
         fiscal=excluded.fiscal,
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
        "SELECT id, visit_id, client_id, tooth, title, qty, unit_price, fiscal, notes, created_at, updated_at, deleted
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
            notes: r.get(8)?,
            created_at: r.get(9)?,
            updated_at: r.get(10)?,
            deleted: r.get(11)?,
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
      "INSERT INTO clients (id, name, phone, email, notes, created_at, updated_at, deleted)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
       ON CONFLICT(id) DO UPDATE SET
         name=excluded.name,
         phone=excluded.phone,
         email=excluded.email,
         notes=excluded.notes,
         created_at=excluded.created_at,
         updated_at=excluded.updated_at,
         deleted=excluded.deleted",
      params![
        row.id,
        row.name,
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

  pub fn apply_remote_sale(&self, row: &Sale) -> anyhow::Result<()> {
    let conn = self.conn()?;
    conn.execute(
      "INSERT INTO sales (id, client_id, date, total, notes, created_at, updated_at, deleted)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
       ON CONFLICT(id) DO UPDATE SET
         client_id=excluded.client_id,
         date=excluded.date,
         total=excluded.total,
         notes=excluded.notes,
         created_at=excluded.created_at,
         updated_at=excluded.updated_at,
         deleted=excluded.deleted",
      params![
        row.id,
        row.client_id,
        row.date,
        row.total,
        row.notes,
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
      "INSERT INTO doctors (id, name, phone, email, notes, created_at, updated_at, deleted)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
       ON CONFLICT(id) DO UPDATE SET
         name=excluded.name,
         phone=excluded.phone,
         email=excluded.email,
         notes=excluded.notes,
         created_at=excluded.created_at,
         updated_at=excluded.updated_at,
         deleted=excluded.deleted",
      params![
        row.id,
        row.name,
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
      "INSERT INTO services (id, title, default_price, notes, created_at, updated_at, deleted)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
       ON CONFLICT(id) DO UPDATE SET
         title=excluded.title,
         default_price=excluded.default_price,
         notes=excluded.notes,
         created_at=excluded.created_at,
         updated_at=excluded.updated_at,
         deleted=excluded.deleted",
      params![
        row.id,
        row.title,
        row.default_price,
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
      "INSERT INTO visits (id, client_id, doctor_id, date, status, notes, created_at, updated_at, deleted)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
       ON CONFLICT(id) DO UPDATE SET
         client_id=excluded.client_id,
         doctor_id=excluded.doctor_id,
         date=excluded.date,
         status=excluded.status,
         notes=excluded.notes,
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
      "INSERT INTO visit_items (id, visit_id, client_id, tooth, title, qty, unit_price, fiscal, notes, created_at, updated_at, deleted)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
       ON CONFLICT(id) DO UPDATE SET
         visit_id=excluded.visit_id,
         client_id=excluded.client_id,
         tooth=excluded.tooth,
         title=excluded.title,
         qty=excluded.qty,
         unit_price=excluded.unit_price,
         fiscal=excluded.fiscal,
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
