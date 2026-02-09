use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppInfo {
  pub version: String,
  pub ui_version: String,
  pub sync_status: String, // synced | pending | error
  pub last_sync_time: Option<String>,
  pub last_sync_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Client {
  pub id: String,
  pub name: String,
  pub phone: Option<String>,
  pub email: Option<String>,
  pub notes: Option<String>,
  pub created_at: String,
  pub updated_at: String,
  pub deleted: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientUpsertInput {
  pub id: Option<String>,
  pub name: String,
  pub phone: Option<String>,
  pub email: Option<String>,
  pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sale {
  pub id: String,
  pub client_id: String,
  pub date: Option<String>, // YYYY-MM-DD
  pub total: f64,
  pub notes: Option<String>,
  pub created_at: String,
  pub updated_at: String,
  pub deleted: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SalesListFilters {
  pub client_id: Option<String>,
  pub date_from: Option<String>,
  pub date_to: Option<String>,
  pub include_deleted: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaleUpsertInput {
  pub id: Option<String>,
  pub client_id: String,
  pub date: Option<String>,
  pub total: f64,
  pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Payment {
  pub id: String,
  pub client_id: String,
  pub sale_id: Option<String>,
  pub date: Option<String>,
  pub amount: f64,
  pub method: String, // cash | card | bank | other
  pub notes: Option<String>,
  pub created_at: String,
  pub updated_at: String,
  pub deleted: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PaymentsListFilters {
  pub client_id: Option<String>,
  pub sale_id: Option<String>,
  pub date_from: Option<String>,
  pub date_to: Option<String>,
  pub include_deleted: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentUpsertInput {
  pub id: Option<String>,
  pub client_id: String,
  pub sale_id: Option<String>,
  pub date: Option<String>,
  pub amount: f64,
  pub method: String,
  pub notes: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SyncQueueItem {
  pub id: String,
  pub table_name: String,
  pub row_id: String,
  pub op: String,
  pub payload: String,
  pub created_at: String,
  pub status: String,
  pub last_error: Option<String>,
}

