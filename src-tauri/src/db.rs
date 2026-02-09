use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{anyhow, bail, Context};
use rusqlite::{params, Connection, OptionalExtension, OpenFlags};
use uuid::Uuid;

use crate::models::{
  Client, ClientUpsertInput, Payment, PaymentUpsertInput, PaymentsListFilters, Sale, SaleUpsertInput,
  SalesListFilters, SyncQueueItem,
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
}
