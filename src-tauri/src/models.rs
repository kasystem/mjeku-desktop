use serde::{Deserialize, Serialize};

fn default_vat_code() -> String {
    "C".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppInfo {
    pub version: String,
    pub ui_version: String,
    pub sync_status: String, // synced | pending | error
    pub last_sync_time: Option<String>,
    pub last_sync_error: Option<String>,
    pub license_ok: bool,
    pub license_status: String, // ok | expired | disabled | offline_grace | unconfigured | unknown
    pub license_active_until: Option<String>,
    pub license_last_checked_at: Option<String>,
    pub license_seconds_left: Option<i64>,
    pub desktop_update_forced: bool,
    pub desktop_update_latest_version: Option<String>,
    pub desktop_update_force_deadline_at: Option<String>,
    pub desktop_update_last_manual_check_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Client {
    pub id: String,
    pub name: String,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub notes: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub parent_name: Option<String>,
    pub dob: Option<String>, // YYYY-MM-DD
    pub gender: Option<String>,
    pub city: Option<String>,
    pub address: Option<String>,
    pub allergies: Option<String>,
    pub weight_kg: Option<f64>,
    pub height_cm: Option<f64>,
    pub patient_code: Option<String>,
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
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub parent_name: Option<String>,
    pub dob: Option<String>,
    pub gender: Option<String>,
    pub city: Option<String>,
    pub address: Option<String>,
    pub allergies: Option<String>,
    pub weight_kg: Option<f64>,
    pub height_cm: Option<f64>,
    pub patient_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sale {
    pub id: String,
    pub client_id: String,
    pub date: Option<String>, // YYYY-MM-DD
    pub total: f64,
    pub notes: Option<String>,
    #[serde(default)]
    pub fiscalized: i64, // 1/0
    #[serde(default)]
    pub fiscalized_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailySaleRow {
    pub sale_id: String,
    pub client_id: String,
    pub client_name: String,
    pub date: Option<String>, // YYYY-MM-DD
    pub total: f64,
    pub fiscal_total: f64,
    pub non_fiscal_total: f64,
    pub notes: Option<String>,
    pub updated_at: String,
    pub classification: String, // fiscal | non_fiscal | mixed
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailySalesReport {
    pub date: String, // YYYY-MM-DD
    pub total: f64,
    pub fiscal_total: f64,
    pub non_fiscal_total: f64,
    pub count_sales: i64,
    pub count_fiscal_only: i64,
    pub count_non_fiscal_only: i64,
    pub count_mixed: i64,
    pub rows: Vec<DailySaleRow>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Doctor {
    pub id: String,
    pub code: Option<String>,
    pub name: String,
    pub title: Option<String>,
    pub specialty: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorLoginOption {
    pub id: String,
    pub code: Option<String>,
    pub name: String,
    pub title: Option<String>,
    pub specialty: Option<String>,
    pub has_account: bool,
    pub is_admin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorUpsertInput {
    pub id: Option<String>,
    pub code: Option<String>,
    pub name: String,
    pub title: Option<String>,
    pub specialty: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Service {
    pub id: String,
    pub title: String,
    pub default_price: f64,
    #[serde(default = "default_vat_code")]
    pub vat_code: String, // A | C | D | E
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceUpsertInput {
    pub id: Option<String>,
    pub title: String,
    pub default_price: f64,
    pub vat_code: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Appointment {
    pub id: String,
    pub client_id: String,
    pub doctor_id: Option<String>,
    pub start_at: String, // RFC3339
    pub end_at: Option<String>,
    pub status: String, // scheduled | done | cancelled
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppointmentsListFilters {
    pub client_id: Option<String>,
    pub doctor_id: Option<String>,
    pub start_from: Option<String>,
    pub start_to: Option<String>,
    pub status: Option<String>,
    pub include_deleted: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppointmentUpsertInput {
    pub id: Option<String>,
    pub client_id: String,
    pub doctor_id: Option<String>,
    pub start_at: String,
    pub end_at: Option<String>,
    pub status: String,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Visit {
    pub id: String,
    pub client_id: String,
    pub doctor_id: Option<String>,
    pub date: Option<String>,       // YYYY-MM-DD
    pub visit_time: Option<String>, // HH:MM
    pub status: String,             // draft | final
    pub notes: Option<String>,
    pub body_weight: Option<String>,
    pub body_weight_unit: Option<String>,
    pub body_height: Option<String>,
    pub body_height_unit: Option<String>,
    pub head_circumference: Option<String>,
    pub head_circumference_unit: Option<String>,
    pub body_temperature: Option<String>,
    pub body_temperature_unit: Option<String>,
    pub blood_oxygen: Option<String>,
    pub blood_oxygen_unit: Option<String>,
    pub glycemia: Option<String>,
    pub glycemia_unit: Option<String>,
    pub pulse: Option<String>,
    pub pulse_unit: Option<String>,
    pub bmi: Option<String>,
    pub blood_pressure_systolic: Option<String>,
    pub blood_pressure_diastolic: Option<String>,
    pub blood_pressure_unit: Option<String>,
    pub complaints: Option<String>,
    pub additional_notes: Option<String>,
    pub controls: Option<String>,
    pub remarks: Option<String>,
    pub analyses: Option<String>,
    pub advice: Option<String>,
    pub therapies: Option<String>,
    pub diagnosis: Option<String>,
    pub examinations: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VisitsListFilters {
    pub client_id: Option<String>,
    pub doctor_id: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub status: Option<String>,
    pub include_deleted: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisitUpsertInput {
    pub id: Option<String>,
    pub client_id: String,
    pub doctor_id: Option<String>,
    pub date: Option<String>,
    pub visit_time: Option<String>,
    pub status: String,
    pub notes: Option<String>,
    pub body_weight: Option<String>,
    pub body_weight_unit: Option<String>,
    pub body_height: Option<String>,
    pub body_height_unit: Option<String>,
    pub head_circumference: Option<String>,
    pub head_circumference_unit: Option<String>,
    pub body_temperature: Option<String>,
    pub body_temperature_unit: Option<String>,
    pub blood_oxygen: Option<String>,
    pub blood_oxygen_unit: Option<String>,
    pub glycemia: Option<String>,
    pub glycemia_unit: Option<String>,
    pub pulse: Option<String>,
    pub pulse_unit: Option<String>,
    pub bmi: Option<String>,
    pub blood_pressure_systolic: Option<String>,
    pub blood_pressure_diastolic: Option<String>,
    pub blood_pressure_unit: Option<String>,
    pub complaints: Option<String>,
    pub additional_notes: Option<String>,
    pub controls: Option<String>,
    pub remarks: Option<String>,
    pub analyses: Option<String>,
    pub advice: Option<String>,
    pub therapies: Option<String>,
    pub diagnosis: Option<String>,
    pub examinations: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisitItem {
    pub id: String,
    pub visit_id: String,
    pub client_id: String,
    pub tooth: Option<String>, // e.g. "13"
    pub title: String,
    pub qty: f64,
    pub unit_price: f64,
    pub fiscal: i64, // 1/0
    #[serde(default = "default_vat_code")]
    pub vat_code: String, // A | C | D | E
    #[serde(default)]
    pub fiscalized: i64, // 1/0
    #[serde(default)]
    pub fiscalized_at: Option<String>,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VisitItemsListFilters {
    pub visit_id: Option<String>,
    pub client_id: Option<String>,
    pub include_deleted: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisitItemUpsertInput {
    pub id: Option<String>,
    pub visit_id: String,
    pub client_id: String,
    pub tooth: Option<String>,
    pub title: String,
    pub qty: f64,
    pub unit_price: f64,
    pub fiscal: Option<bool>,
    pub vat_code: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CashEntry {
    pub id: String,
    pub r#type: String, // income | expense
    pub date: Option<String>,
    pub amount: f64,
    pub category: Option<String>,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CashListFilters {
    pub r#type: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub include_deleted: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CashEntryUpsertInput {
    pub id: Option<String>,
    pub r#type: String,
    pub date: Option<String>,
    pub amount: f64,
    pub category: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorAccount {
    pub doctor_id: String,
    pub clinic_id: Option<String>,
    pub salt: String,
    pub password_hash: String,
    pub is_admin: i64,
    pub created_at: String,
    pub updated_at: String,
    pub deleted: i64,
}
