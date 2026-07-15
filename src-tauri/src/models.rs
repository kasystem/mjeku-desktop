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
    #[serde(default)]
    pub is_demo_mode: bool,
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
    #[serde(default)]
    pub specialty_report: Option<String>,
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
    #[serde(default)]
    pub specialty_report: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegularInvoice {
    pub id: String,
    pub sale_id: String,
    pub invoice_number: Option<String>,
    pub client_id: Option<String>,
    pub client_name: Option<String>,
    pub date: Option<String>,
    pub total: f64,
    pub pdf_filename: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthlyReportRow {
    pub month: String,
    pub total: f64,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockSupplier {
    #[serde(default)]
    pub deleted: i64,
    pub id: String,
    pub name: String,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub address: Option<String>,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockItem {
    #[serde(default)]
    pub deleted: i64,
    pub id: String,
    pub name: String,
    pub unit: Option<String>,
    pub category: Option<String>,
    pub supplier_id: Option<String>,
    pub min_quantity: f64,
    #[serde(default)]
    pub sale_price: f64,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub supplier_name: Option<String>,
    #[serde(default)]
    pub current_quantity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaleItem {
    pub id: String,
    pub sale_id: String,
    pub item_type: String, // "service" | "product"
    pub ref_id: Option<String>,
    pub title: String,
    pub qty: f64,
    pub unit_price: f64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaleItemInput {
    pub item_type: String,
    pub ref_id: Option<String>,
    pub title: String,
    pub qty: f64,
    pub unit_price: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockMovement {
    #[serde(default)]
    pub deleted: i64,
    pub id: String,
    pub item_id: String,
    pub movement_type: String,
    pub quantity: f64,
    pub price_per_unit: Option<f64>,
    pub notes: Option<String>,
    pub date: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Offer {
    pub id: String,
    pub clinic_id: String,
    pub client_id: String,
    pub offer_number: String,
    pub status: String, // draft|sent|accepted|rejected|invoiced
    pub valid_until: Option<String>, // YYYY-MM-DD
    pub notes: Option<String>,
    pub vat_pct: f64,
    pub subtotal: f64,
    pub vat_amount: f64,
    pub total: f64,
    pub invoice_id: Option<String>,
    pub source_offer_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfferItem {
    pub id: String,
    pub offer_id: String,
    pub clinic_id: String,
    pub description: String,
    pub qty: f64,
    pub unit_price: f64,
    pub discount_pct: f64,
    pub line_total: f64,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfferItemInput {
    pub id: Option<String>,
    pub description: String,
    pub qty: f64,
    pub unit_price: f64,
    pub discount_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientPhoto {
    pub id: String,
    pub client_id: String,
    pub stage: String, // before|after|other
    pub label: String,
    pub file_path: String,
    pub taken_at: Option<String>,
    pub created_at: String,
    pub deleted: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prescription {
    pub id: String,
    pub visit_id: Option<String>,
    pub client_id: Option<String>,
    pub doctor_id: Option<String>,
    pub kind: String, // recete | udhezim
    pub title: String,
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub deleted: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FiscalJob {
    pub id: String,
    pub sale_id: String,
    pub status: String, // pending | done | failed | cancelled
    pub requested_by: String,
    pub error: Option<String>,
    pub processed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub deleted: i64,
}

/// Profil i njohur i një analizuesi laboratorik (marka/modeli). Jo i ruajtur
/// në DB — listë statike e ndërtuar nga specifikimet publike ASTM E1394-97,
/// të cilat prodhues të ndryshëm i implementojnë me variacione të vogla
/// (baud rate, checksum strict/relaxed). Duhet verifikuar kundër dokumentit
/// ICD të vetë pajisjes fizike përpara përdorimit real klinik.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzerProfile {
    pub id: String,
    pub brand: String,
    pub model: String,
    pub protocol: String, // astm_e1394
    pub baud_rate: u32,
    pub data_bits: u8,
    pub parity: String, // none | even | odd
    pub stop_bits: u8,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabDeviceStatus {
    pub connected: bool,
    pub port_name: Option<String>,
    pub profile_id: Option<String>,
    pub last_error: Option<String>,
    pub last_message_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabInboxItem {
    pub id: String,
    pub profile_id: String,
    pub patient_ref_raw: String,
    pub formatted_text: String,
    pub matched_client_id: Option<String>,
    pub matched_visit_id: Option<String>,
    pub status: String, // unmatched | assigned
    pub received_at: String,
}
