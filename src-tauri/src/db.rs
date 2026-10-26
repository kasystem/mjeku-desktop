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

/// Shitja e gatshme për thirrjen SEF `fiscalize` (shih `Db::sef_collect_sale`).
#[derive(Debug, Clone)]
pub struct SefSaleData {
    pub sale: Sale,
    pub items: Vec<VisitItem>,
    pub used_non_fiscal_fallback: bool,
    /// (metoda, shuma) e pagesave jo të fshira të shitjes.
    pub payments: Vec<(String, f64)>,
}

/// Të dhënat e kuponit SEF që Mjeku ruan te shitja (burimi i së vërtetës: PROGRAMERI POS).
#[derive(Debug, Clone, serde::Serialize)]
pub struct SefCouponInfo {
    pub coupon_id: String,
    pub daily_number: Option<i64>,
    pub status: String,
    pub verification_url: Option<String>,
}

pub struct Db {
    db_path: PathBuf,
    conn: Mutex<Connection>,
}

#[cfg(test)]
mod vat_service_tests {
    use super::*;

    fn visit(vat_code: Option<&str>) -> ServiceUpsertInput {
        ServiceUpsertInput {
            id: None, title: "Vizitë".to_string(), default_price: 0.0,
            vat_code: vat_code.map(str::to_string), notes: None,
        }
    }

    #[test]
    fn registered_business_preserves_all_four_categories_and_defaults_to_a() {
        let db = Db::new_in_memory().unwrap();
        db.setting_set("clinic_vat_registered", "1").unwrap();
        for code in ["A", "C", "D", "E"] {
            let saved = db.services_upsert(visit(Some(code))).unwrap();
            assert_eq!(saved.vat_code, code);
            let persisted = db.services_list(None).unwrap().into_iter().find(|row| row.id == saved.id).unwrap();
            assert_eq!(persisted.vat_code, code);
        }
        assert_eq!(db.services_upsert(visit(None)).unwrap().vat_code, "A");
        assert!(db.services_upsert(visit(Some("B"))).is_err());
    }

    #[test]
    fn unregistered_business_uses_only_c() {
        let db = Db::new_in_memory().unwrap();
        for code in [None, Some("A"), Some("C"), Some("D"), Some("E")] {
            assert_eq!(db.services_upsert(visit(code)).unwrap().vat_code, "C");
        }
    }
}

#[cfg(test)]
mod orthopedic_lifecycle_tests {
    use super::*;

    fn setup_episode() -> Db {
        let db = Db::new_in_memory().expect("clinic schema including orthopedics v4");
        db.conn.lock().unwrap().execute(
            "INSERT INTO clients(id,name,deleted) VALUES('ortho-patient','Test Patient',0)", []
        ).unwrap();
        db.orthopedic_episode_upsert(&serde_json::json!({
            "id":"ortho-episode","client_id":"ortho-patient",
            "diagnosis":"Clinical test","region":"Knee","laterality":"left",
            "status":"active","notes":"","created_at":"2026-10-25T12:00:00Z",
            "updated_at":"2026-10-25T12:00:00Z"
        })).unwrap();
        db
    }

    #[test]
    fn physiotherapy_link_preserves_patient_and_immutable_session_history() {
        let db=setup_episode();
        db.orthopedic_rehab_upsert(&serde_json::json!({
            "id":"rehab-p0","episode_id":"ortho-episode","protocol":"Postoperative knee",
            "phase":"phase_1","load_restrictions":"No full weight bearing",
            "rom_restrictions":"Flexion maximum 60 degrees",
            "status":"active","created_at":"2026-10-25T18:00:00Z"
        })).expect("source orthopedic protocol");
        {
            let conn=db.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO clients(id,name,deleted) VALUES('other-physio-patient','Other Patient',0)",
                []
            ).unwrap();
        }
        db.orthopedic_episode_upsert(&serde_json::json!({
            "id":"other-ortho-episode","client_id":"other-physio-patient",
            "diagnosis":"Other case","region":"Ankle","laterality":"right",
            "status":"active","created_at":"2026-10-25","updated_at":"2026-10-25"
        })).unwrap();
        db.orthopedic_rehab_upsert(&serde_json::json!({
            "id":"other-rehab","episode_id":"other-ortho-episode",
            "protocol":"Other protocol","phase":"phase_1","status":"active",
            "created_at":"2026-10-25"
        })).unwrap();
        assert!(db.physiotherapy_referral_create(
            "ortho-episode","other-rehab","doctor-a"
        ).is_err(),"Cross-patient rehabilitation referral must be denied");
        assert!(db.physiotherapy_referral_create("ortho-episode","unknown-plan","doctor-a").is_err());
        let referral=db.physiotherapy_referral_create(
            "ortho-episode","rehab-p0","doctor-a"
        ).expect("referral from real orthopedic plan");
        assert_eq!(referral["client_id"],"ortho-patient");
        let repeat=db.physiotherapy_referral_create(
            "ortho-episode","rehab-p0","doctor-a"
        ).unwrap();
        assert_eq!(referral["id"],repeat["id"],"referrals are idempotent per plan");
        let episode_id=referral["id"].as_str().unwrap();
        let list=db.physiotherapy_episodes_list(Some("ortho-episode".into())).unwrap();
        assert_eq!(list.len(),1);
        assert_eq!(list[0]["patient_name"],"Test Patient");
        assert_eq!(list[0]["load_restrictions"],"No full weight bearing");
        assert_eq!(list[0]["rom_restrictions"],"Flexion maximum 60 degrees");

        let session=serde_json::json!({
            "physiotherapy_episode_id":episode_id,
            "session_date":"2026-10-25","intervention":"Supervised ROM exercise",
            "response":"Tolerated within restrictions",
            "pain_before":5,"pain_after":3,"rom_joint":"Knee",
            "rom_movement":"Flexion","rom_degrees":50.0,"mmt_grade":3,
            "next_plan":"Repeat only with therapist"
        });
        assert!(db.physiotherapy_session_add(&session,"").is_err());
        let mut invalid=session.clone();
        invalid["pain_before"]=serde_json::json!(11);
        assert!(db.physiotherapy_session_add(&invalid,"doctor-a").is_err());
        invalid=session.clone();
        invalid["mmt_grade"]=serde_json::json!(8);
        assert!(db.physiotherapy_session_add(&invalid,"doctor-a").is_err());
        invalid=session.clone();
        invalid["rom_degrees"]=serde_json::json!(400.0);
        assert!(db.physiotherapy_session_add(&invalid,"doctor-a").is_err());
        let row=db.physiotherapy_session_add(&session,"doctor-a").unwrap();
        assert_eq!(row["pain_before"],5);
        assert_eq!(row["pain_after"],3);
        let history=db.physiotherapy_sessions_list(episode_id).unwrap();
        assert_eq!(history.len(),1);
        assert_eq!(history[0]["rom_degrees"],50.0);
        assert_eq!(history[0]["therapist_id"],"doctor-a");

        let conn=db.conn.lock().unwrap();
        assert!(conn.execute("UPDATE physiotherapy_sessions SET pain_after=0 WHERE id=?1",
            [row["id"].as_str().unwrap()]).is_err());
        drop(conn);
        db.orthopedic_report_version_create(&serde_json::json!({
            "episode_id":"ortho-episode","type":"final_summary","status":"final",
            "body_json":"{\"narrative\":\"Discharge referral test\"}"
        }),"doctor-a").unwrap();
        db.orthopedic_episode_transition(
            "ortho-episode","close","Clinically complete after rehabilitation","doctor-a"
        ).unwrap();
        assert!(db.physiotherapy_session_add(&session,"doctor-a").is_err());
        // A bypass of Tauri still cannot insert while linked orthopedics is closed.
        let conn=db.conn.lock().unwrap();
        assert!(conn.execute(
            "INSERT INTO physiotherapy_sessions(
             id,physiotherapy_episode_id,session_date,therapist_id,intervention,created_at)
             VALUES('fake-bypass',?1,'2026-10-26','fake','Unauthorized','2026-10-26')",
            [episode_id]
        ).is_err());
    }

    fn setup_physio() -> (Db,String) {
        let db=setup_episode();
        db.orthopedic_rehab_upsert(&serde_json::json!({
            "id":"rehab-full","episode_id":"ortho-episode",
            "protocol":"Clinical recovery",
            "phase":"phase_1","load_restrictions":"Partial weight bearing",
            "rom_restrictions":"Flexion 60 degrees",
            "status":"active","created_at":"2026-10-26"
        })).unwrap();
        let referral=db.physiotherapy_referral_create(
            "ortho-episode","rehab-full","doctor-a").unwrap();
        (db,referral["id"].as_str().unwrap().to_string())
    }

    #[test]
    fn physio_body_map_markers_preserve_normalized_coordinates_and_actor() {
        let (db,id)=setup_physio();
        let marker=serde_json::json!({
            "physiotherapy_episode_id":id,"view":"anterior",
            "region":"left_knee","laterality":"left",
            "classification":"pain","x":0.38,"y":0.76,
            "note":"Patient reports discomfort"
        });
        let saved=db.physiotherapy_body_marker_add(&marker,"doctor-a").unwrap();
        assert_eq!(saved["x"],0.38);
        assert_eq!(saved["clinician_id"],"doctor-a");
        let mut invalid=marker.clone();
        invalid["y"]=serde_json::json!(-0.5);
        assert!(db.physiotherapy_body_marker_add(&invalid,"doctor-a").is_err());
        invalid=marker.clone();
        invalid["laterality"]=serde_json::json!("invalid");
        assert!(db.physiotherapy_body_marker_add(&invalid,"doctor-a").is_err());
        let list=db.physiotherapy_body_markers_list(&id).unwrap();
        assert_eq!(list.len(),1);
        assert_eq!(list[0]["region"],"left_knee");
        assert!(db.conn.lock().unwrap().execute(
            "UPDATE physiotherapy_body_markers SET x=0.0 WHERE id=?1",
            [saved["id"].as_str().unwrap()]
        ).is_err());
    }

    #[test]
    fn physio_migrations_can_reapply_without_losing_existing_data() {
        let (db,id)=setup_physio();
        let conn=db.conn.lock().unwrap();
        for v in [
            include_str!("../migrations/physiotherapy_v1.sql"),
            include_str!("../migrations/physiotherapy_v2.sql"),
            include_str!("../migrations/physiotherapy_v3.sql"),
            include_str!("../migrations/physiotherapy_v4.sql"),
            include_str!("../migrations/physiotherapy_v5.sql")
        ] {conn.execute_batch(v).expect("idempotent physiotherapy migration");}
        let present:i64=conn.query_row(
            "SELECT count(*) FROM physiotherapy_episodes WHERE id=?1",
            [id.as_str()],|r|r.get(0)).unwrap();
        assert_eq!(present,1,"Migration must preserve existing rehabilitation episode");
    }

    #[test]
    fn physio_measurements_and_care_plans_keep_validation_and_versions() {
        let (db,id)=setup_physio();
        let rom=serde_json::json!({
            "physiotherapy_episode_id":id,"kind":"rom","value":50.0,
            "unit":"deg","region":"Knee","movement":"Flexion",
            "laterality":"left","method":"Goniometer","assessed_at":"2026-10-26"
        });
        assert!(db.physiotherapy_measurement_add(&rom,"doctor-a").is_ok());
        let mut bad=rom.clone();bad["value"]=serde_json::json!(370.0);
        assert!(db.physiotherapy_measurement_add(&bad,"doctor-a").is_err());
        bad=rom.clone();bad["unit"]=serde_json::json!("percent");
        assert!(db.physiotherapy_measurement_add(&bad,"doctor-a").is_err());
        let measured=db.physiotherapy_measurements_list(&id).unwrap();
        assert_eq!(measured.len(),1);
        let first=&measured[0];
        let mut correction=rom.clone();
        correction["value"]=serde_json::json!(49.0);
        correction["correction_of"]=first["id"].clone();
        assert!(db.physiotherapy_measurement_add(&correction,"doctor-a").is_err());
        correction["correction_reason"]=serde_json::json!("Calibration reviewed with goniometer");
        assert!(db.physiotherapy_measurement_add(&correction,"doctor-a").is_ok());
        assert_eq!(db.physiotherapy_measurements_list(&id).unwrap().len(),2);

        let plan=serde_json::json!({
            "physiotherapy_episode_id":id,"goal":"Restore controlled knee movement",
            "measurable_target":"Flexion 60 degrees",
            "frequency":"3 sessions per week","duration":"4 weeks",
            "interventions":"Supervised motion","status":"approved",
            "phase":"phase_1"
        });
        let first=db.physiotherapy_care_plan_create(&plan,"doctor-a").unwrap();
        assert_eq!(first["version"],1);
        let second=db.physiotherapy_care_plan_create(&plan,"doctor-a").unwrap();
        assert_eq!(second["version"],2);
        let list=db.physiotherapy_care_plans_list(&id).unwrap();
        assert_eq!(list.len(),2);
        assert_eq!(list[0]["version"],2);
        assert!(db.conn.lock().unwrap().execute(
            "UPDATE physiotherapy_care_plans SET goal='tampered' WHERE id=?1",
            [first["id"].as_str().unwrap()]
        ).is_err());
    }

    #[test]
    fn physio_hep_requires_current_approved_plan_and_snapshot_restrictions() {
        let (db,id)=setup_physio();
        let exercise=serde_json::json!({
            "title":"Gentle knee flexion","instructions":"Flex only within surgeon limits",
            "anatomical_region":"Knee","contraindications":"Stop with increased pain"
        });
        let created=db.physiotherapy_exercise_create(&exercise,"doctor-a").unwrap();
        let bad_video=serde_json::json!({
            "title":"Unsafe video","instructions":"Perform exercise carefully",
            "media_ref":"https://example.org/clip.mp4"
        });
        assert!(db.physiotherapy_exercise_create(&bad_video,"doctor-a").is_err());
        let plan=serde_json::json!({
            "physiotherapy_episode_id":id,"goal":"Return to measured activity",
            "status":"approved","frequency":"Weekly"
        });
        let care=db.physiotherapy_care_plan_create(&plan,"doctor-a").unwrap();
        let hep=serde_json::json!({
            "physiotherapy_episode_id":id,
            "care_plan_id":care["id"],"exercise_id":created["id"],
            "sets":3,"repetitions":10,"frequency":"Every day","status":"approved"
        });
        let assigned=db.physiotherapy_hep_assign(&hep,"doctor-a").unwrap();
        assert_eq!(assigned["load_restrictions_snapshot"],"Partial weight bearing");
        let list=db.physiotherapy_hep_list(&id).unwrap();
        assert_eq!(list.len(),1);
        assert_eq!(list[0]["needs_review"],false);
        let record=serde_json::json!({
            "hep_id":assigned["id"],"performed_date":"2026-10-26",
            "performed":true,"self_reported":true,"response":"Patient reports no difficulty"
        });
        assert!(db.physiotherapy_hep_log_add(&record,"doctor-a").is_ok());
        assert_eq!(db.physiotherapy_hep_list(&id).unwrap()[0]["completed_logs"],1);
        db.conn.lock().unwrap().execute(
            "UPDATE orthopedic_rehab_plans SET load_restrictions='No weight bearing' WHERE id='rehab-full'",
            []
        ).unwrap();
        assert_eq!(db.physiotherapy_hep_list(&id).unwrap()[0]["needs_review"],true);
        assert!(db.physiotherapy_hep_log_add(&record,"doctor-a").is_err());
        assert!(db.physiotherapy_hep_assign(&hep,"doctor-a").is_ok(),
            "new authorized assignment captures latest restrictions");
    }

    #[test]
    fn physio_saved_reports_and_documents_preserve_patient_identity() {
        let (db,id)=setup_physio();
        let report=serde_json::json!({
            "physiotherapy_episode_id":id,"type":"progress","status":"final",
            "narrative":"Patient shows progress with supervised rehabilitation"
        });
        let signed=db.physiotherapy_report_create(&report,"doctor-a").unwrap();
        assert_eq!(signed["version"],1);
        let data=db.physiotherapy_document_data(signed["id"].as_str().unwrap(),"report").unwrap();
        assert_eq!(data["patient_name"],"Test Patient");
        assert_eq!(data["status"],"final");
        assert!(data["clinical_summary_json"].as_str().unwrap().contains("sessions"));
        let amendment=serde_json::json!({
            "physiotherapy_episode_id":id,"type":"progress","status":"amendment",
            "narrative":"Professional reassessment after new findings",
            "reason":"Corrected functional limitation after examination"
        });
        assert_eq!(db.physiotherapy_report_create(&amendment,"doctor-a").unwrap()["version"],2);
        assert!(db.conn.lock().unwrap().execute(
            "DELETE FROM physiotherapy_reports WHERE id=?1",
            [signed["id"].as_str().unwrap()]
        ).is_err());
        let docs=db.physiotherapy_document_register(
            "ortho-episode","progress","Report.pdf","rehab-pdf/unique.pdf",
            &"a".repeat(64),1000,"Therapist report","doctor-a").unwrap();
        assert_eq!(docs["client_id"],"ortho-patient");
        assert_eq!(db.physiotherapy_documents_list("ortho-episode").unwrap().len(),1);
        assert!(db.physiotherapy_document_register(
            "other-episode","progress","Report.pdf","rehab-pdf/other.pdf",
            &"b".repeat(64),1000,"","doctor-a").is_err());
        assert!(db.conn.lock().unwrap().execute(
            "UPDATE physiotherapy_documents SET client_id='wrong' WHERE id=?1",
            [docs["id"].as_str().unwrap()]
        ).is_err());
    }

    #[test]
    fn pdf_data_uses_persisted_patient_and_report_version() {
        let db = setup_episode();
        assert!(db.orthopedic_report_pdf_data("nonexistent-report").is_err());
        let report = db.orthopedic_report_version_create(&serde_json::json!({
            "episode_id":"ortho-episode","type":"final_summary","status":"final",
            "body_json":serde_json::json!({
                "narrative":"Stable source-of-truth clinical notes",
                "fractures":[{"boneRegion":"Femur","currentStatus":"healing"}]
            }).to_string()
        }), "doctor-a").unwrap();
        let record = db.orthopedic_report_pdf_data(report["id"].as_str().unwrap()).unwrap();
        assert_eq!(record["patient_name"],"Test Patient");
        assert_eq!(record["diagnosis"],"Clinical test");
        assert_eq!(record["episode_id"],"ortho-episode");
        assert_eq!(record["status"],"final");
        assert_eq!(record["author_id"],"doctor-a");
        assert_eq!(record["body_json"],report["body_json"]);
        let pdf = crate::orthopedics_pdf::render_orthopedic_pdf(&record,"Test Clinic").unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
        assert!(pdf.len()>1500);
    }

    #[test]
    fn discharge_requires_signed_final_summary_and_valid_transition_reason() {
        let db = setup_episode();
        assert!(db.orthopedic_episode_transition(
            "ortho-episode","close","Documented recovery complete","doctor-a"
        ).is_err());
        let report = db.orthopedic_report_version_create(&serde_json::json!({
            "episode_id":"ortho-episode","type":"final_summary","status":"final",
            "body_json":"{\"narrative\":\"Clinical findings and final plan\"}"
        }),"doctor-a").unwrap();
        assert!(db.orthopedic_episode_transition(
            "ortho-episode","close","short","doctor-a"
        ).is_err());
        let closed = db.orthopedic_episode_transition(
            "ortho-episode","close","Documented recovery complete","doctor-a"
        ).unwrap();
        assert_eq!(closed["to_status"],"closed");
        assert_eq!(closed["final_report_id"],report["id"]);
        assert!(db.orthopedic_episode_transition(
            "ortho-episode","close","Duplicate closing event","doctor-a"
        ).is_err());
        assert!(db.orthopedic_episode_transition(
            "ortho-episode","reopen","short","doctor-a"
        ).is_err());
    }

    #[test]
    fn closed_episode_is_read_only_but_signed_amendments_and_reopening_work() {
        let db = setup_episode();
        db.orthopedic_report_version_create(&serde_json::json!({
            "episode_id":"ortho-episode","type":"final_summary","status":"final",
            "body_json":"{\"narrative\":\"Discharge plan\"}"
        }),"doctor-a").unwrap();
        db.orthopedic_episode_transition(
            "ortho-episode","close","Final clinical discharge summary","doctor-a"
        ).unwrap();

        let marker = serde_json::json!({
            "id":"marker-a","episode_id":"ortho-episode","region":"left_knee",
            "side":"left","view":"anterior","injury_type":"fracture",
            "note":"","x":0.5,"y":0.5,"created_at":"2026-10-25"
        });
        assert!(db.orthopedic_marker_upsert(&marker).is_err());
        assert!(db.orthopedic_episode_upsert(&serde_json::json!({
            "id":"ortho-episode","client_id":"ortho-patient","diagnosis":"Tampering",
            "region":"Knee","laterality":"left","status":"active"
        })).is_err());
        assert!(db.orthopedic_report_version_create(&serde_json::json!({
            "episode_id":"ortho-episode","type":"final_summary","status":"draft",
            "body_json":"{\"narrative\":\"Not an amendment\"}"
        }),"doctor-a").is_err());
        assert!(db.orthopedic_report_version_create(&serde_json::json!({
            "episode_id":"ortho-episode","type":"final_summary","status":"amendment",
            "reason":"Corrected postoperative finding",
            "body_json":"{\"narrative\":\"Corrected description\"}"
        }),"doctor-a").is_ok());
        let history = db.orthopedic_episode_transitions_list("ortho-episode").unwrap();
        assert_eq!(history.len(),1);
        assert_eq!(history[0]["clinician_id"],"doctor-a");
        assert!(db.conn.lock().unwrap().execute(
            "DELETE FROM orthopedic_episode_transitions WHERE episode_id='ortho-episode'",[]
        ).is_err());

        db.orthopedic_episode_transition(
            "ortho-episode","reopen","New clinical finding requires review","doctor-b"
        ).unwrap();
        assert!(db.orthopedic_marker_upsert(&marker).is_ok());
        let history = db.orthopedic_episode_transitions_list("ortho-episode").unwrap();
        assert_eq!(history.len(),2);
        assert_eq!(history[0]["to_status"],"active");
    }
}

impl Db {
    fn orthopedic_require_episode(conn: &Connection, episode_id: &str) -> anyhow::Result<()> {
        if episode_id.trim().is_empty() ||
            !conn.query_row("SELECT EXISTS(SELECT 1 FROM orthopedic_episodes WHERE id=?1)",
                rusqlite::params![episode_id], |r| r.get::<_, bool>(0))? {
            bail!("Episodi ortopedik nuk ekziston.");
        }
        Ok(())
    }

    fn orthopedic_require_surgery(conn: &Connection, surgery_id: &str) -> anyhow::Result<()> {
        if surgery_id.trim().is_empty() ||
            !conn.query_row("SELECT EXISTS(SELECT 1 FROM orthopedic_surgeries WHERE id=?1)",
                rusqlite::params![surgery_id], |r| r.get::<_, bool>(0))? {
            bail!("Plani kirurgjik nuk ekziston.");
        }
        Ok(())
    }

    /// Shared Physiotherapy episode. A clinician can refer an active orthopedic
    /// rehabilitation plan only to its own patient. Repeating the same referral
    /// returns the existing immutable row (one source of truth per plan).
    pub fn physiotherapy_referral_create(
        &self, orthopedic_episode_id: &str, rehab_plan_id: &str, clinician: &str,
    ) -> anyhow::Result<serde_json::Value> {
        if clinician.trim().is_empty() { bail!("Mjeku i identifikuar kërkohet për referim."); }
        if orthopedic_episode_id.trim().is_empty() || rehab_plan_id.trim().is_empty() {
            bail!("Zgjidh episodin ortopedik dhe protokollin e rehabilitimit.");
        }
        let conn = self.conn.lock().unwrap();
        let patient: String = conn.query_row(
            "SELECT e.client_id FROM orthopedic_episodes e
             JOIN orthopedic_rehab_plans p ON p.episode_id=e.id
             WHERE e.id=?1 AND p.id=?2 AND e.status='active'",
            rusqlite::params![orthopedic_episode_id,rehab_plan_id],
            |r| r.get(0),
        ).optional()?.ok_or_else(|| anyhow!(
            "Protokolli nuk i përket një episodi ortopedik aktiv."
        ))?;
        conn.execute(
            "INSERT OR IGNORE INTO physiotherapy_episodes
             (id,client_id,orthopedic_episode_id,rehab_plan_id,status,created_by,created_at)
             VALUES(?1,?2,?3,?4,'active',?5,?6)",
            rusqlite::params![Uuid::new_v4().to_string(),patient,
                orthopedic_episode_id,rehab_plan_id,clinician,now_iso()],
        )?;
        let data: serde_json::Value = conn.query_row(
            "SELECT id,client_id,orthopedic_episode_id,rehab_plan_id,status,created_by,created_at
             FROM physiotherapy_episodes WHERE rehab_plan_id=?1",
            [rehab_plan_id],
            |r| Ok(serde_json::json!({
                "id":r.get::<_,String>(0)?,
                "client_id":r.get::<_,String>(1)?,
                "orthopedic_episode_id":r.get::<_,String>(2)?,
                "rehab_plan_id":r.get::<_,String>(3)?,
                "status":r.get::<_,String>(4)?,
                "created_by":r.get::<_,String>(5)?,
                "created_at":r.get::<_,String>(6)?
            }))
        )?;
        Ok(data)
    }

    /// Both department screens read the same joined data; restrictions always
    /// come from orthopedic_rehab_plans, never an obsolete physio copy.
    pub fn physiotherapy_episodes_list(
        &self, orthopedic_episode_id: Option<String>,
    ) -> anyhow::Result<Vec<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT f.id,f.client_id,c.name,f.orthopedic_episode_id,e.diagnosis,
                    e.region,e.laterality,e.status,f.rehab_plan_id,p.protocol,
                    p.phase,COALESCE(p.load_restrictions,''),COALESCE(p.rom_restrictions,''),
                    f.created_by,f.created_at,f.status,
                    (SELECT COUNT(*) FROM physiotherapy_sessions s
                     WHERE s.physiotherapy_episode_id=f.id)
             FROM physiotherapy_episodes f
             JOIN clients c ON c.id=f.client_id AND c.deleted=0
             JOIN orthopedic_episodes e ON e.id=f.orthopedic_episode_id
             JOIN orthopedic_rehab_plans p ON p.id=f.rehab_plan_id
             WHERE (?1 IS NULL OR f.orthopedic_episode_id=?1)
             ORDER BY f.created_at DESC,f.id DESC"
        )?;
        let rows=stmt.query_map([orthopedic_episode_id], |r| Ok(serde_json::json!({
            "id":r.get::<_,String>(0)?,
            "client_id":r.get::<_,String>(1)?,
            "patient_name":r.get::<_,String>(2)?,
            "orthopedic_episode_id":r.get::<_,String>(3)?,
            "diagnosis":r.get::<_,String>(4)?,
            "region":r.get::<_,String>(5)?,
            "laterality":r.get::<_,String>(6)?,
            "orthopedic_status":r.get::<_,String>(7)?,
            "rehab_plan_id":r.get::<_,String>(8)?,
            "protocol":r.get::<_,String>(9)?,
            "phase":r.get::<_,String>(10)?,
            "load_restrictions":r.get::<_,String>(11)?,
            "rom_restrictions":r.get::<_,String>(12)?,
            "created_by":r.get::<_,String>(13)?,
            "created_at":r.get::<_,String>(14)?,
            "status":r.get::<_,String>(15)?,
            "session_count":r.get::<_,i64>(16)?
        })))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn physiotherapy_sessions_list(
        &self, physiotherapy_episode_id: &str,
    ) -> anyhow::Result<Vec<serde_json::Value>> {
        let conn=self.conn.lock().unwrap();
        let mut stmt=conn.prepare(
            "SELECT id,physiotherapy_episode_id,session_date,therapist_id,
                    intervention,response,pain_before,pain_after,
                    rom_joint,rom_movement,rom_degrees,mmt_grade,next_plan,created_at
             FROM physiotherapy_sessions
             WHERE physiotherapy_episode_id=?1 ORDER BY session_date DESC,created_at DESC"
        )?;
        let rows=stmt.query_map([physiotherapy_episode_id],|r|Ok(serde_json::json!({
            "id":r.get::<_,String>(0)?,
            "physiotherapy_episode_id":r.get::<_,String>(1)?,
            "session_date":r.get::<_,String>(2)?,
            "therapist_id":r.get::<_,String>(3)?,
            "intervention":r.get::<_,String>(4)?,
            "response":r.get::<_,String>(5)?,
            "pain_before":r.get::<_,Option<i64>>(6)?,
            "pain_after":r.get::<_,Option<i64>>(7)?,
            "rom_joint":r.get::<_,String>(8)?,
            "rom_movement":r.get::<_,String>(9)?,
            "rom_degrees":r.get::<_,Option<f64>>(10)?,
            "mmt_grade":r.get::<_,Option<i64>>(11)?,
            "next_plan":r.get::<_,String>(12)?,
            "created_at":r.get::<_,String>(13)?
        })))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn physiotherapy_session_add(
        &self, value: &serde_json::Value, clinician: &str,
    ) -> anyhow::Result<serde_json::Value> {
        if clinician.trim().is_empty() { bail!("Kërkohet përdorues klinik i identifikuar."); }
        let episode=value["physiotherapy_episode_id"].as_str().unwrap_or_default();
        let date=value["session_date"].as_str().unwrap_or_default();
        let treatment=value["intervention"].as_str().unwrap_or_default().trim();
        if episode.trim().is_empty() || treatment.chars().count()<3 || treatment.chars().count()>4000 {
            bail!("Seanca kërkon episodin dhe përshkrimin e trajtimit (3–4000 karaktere).");
        }
        chrono::NaiveDate::parse_from_str(date,"%Y-%m-%d")
            .map_err(|_| anyhow!("Data e seancës duhet të jetë YYYY-MM-DD."))?;
        fn score(v: &serde_json::Value, key: &str, max: i64) -> anyhow::Result<Option<i64>> {
            if v.get(key).is_none() || v[key].is_null() { return Ok(None); }
            let s=v[key].as_i64().ok_or_else(|| anyhow!("Matje e pavlefshme: {}",key))?;
            if !(0..=max).contains(&s) { bail!("Matja {} jashtë intervalit 0..{}.",key,max); }
            Ok(Some(s))
        }
        let pain_before=score(value,"pain_before",10)?;
        let pain_after=score(value,"pain_after",10)?;
        let mmt=score(value,"mmt_grade",5)?;
        let rom=value.get("rom_degrees").filter(|v|!v.is_null()).map(|v|v.as_f64()
            .ok_or_else(||anyhow!("ROM duhet të jetë numerik."))).transpose()?;
        if let Some(v)=rom {
            if !v.is_finite() || !(0.0..=360.0).contains(&v) {
                bail!("ROM duhet të jetë në intervalin 0..360°.");
            }
            if value["rom_joint"].as_str().unwrap_or_default().trim().is_empty()
                || value["rom_movement"].as_str().unwrap_or_default().trim().is_empty() {
                bail!("Matja ROM kërkon nyjën dhe lëvizjen.");
            }
        }
        let conn=self.conn.lock().unwrap();
        let active:i64=conn.query_row(
            "SELECT COUNT(*) FROM physiotherapy_episodes f
             JOIN orthopedic_episodes e ON e.id=f.orthopedic_episode_id
             WHERE f.id=?1 AND f.status='active' AND e.status='active'",
            [episode], |r|r.get(0))?;
        if active!=1 { bail!("Seanca refuzohet: referimi ose episodi ortopedik nuk është aktiv."); }
        let id=Uuid::new_v4().to_string();
        let now=now_iso();
        let response=value["response"].as_str().unwrap_or_default();
        let next=value["next_plan"].as_str().unwrap_or_default();
        if response.len()>4000 || next.len()>4000 { bail!("Përmbajtja e seancës është shumë e gjatë."); }
        conn.execute(
            "INSERT INTO physiotherapy_sessions(
             id,physiotherapy_episode_id,session_date,therapist_id,intervention,response,
             pain_before,pain_after,rom_joint,rom_movement,rom_degrees,mmt_grade,next_plan,created_at
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
            rusqlite::params![id,episode,date,clinician,treatment,response,pain_before,pain_after,
                value["rom_joint"].as_str().unwrap_or_default(),
                value["rom_movement"].as_str().unwrap_or_default(),
                rom,mmt,next,now]
        )?;
        Ok(serde_json::json!({
            "id":id,"physiotherapy_episode_id":episode,"session_date":date,
            "therapist_id":clinician,"intervention":treatment,"response":response,
            "pain_before":pain_before,"pain_after":pain_after,"rom_joint":value["rom_joint"],
            "rom_movement":value["rom_movement"],"rom_degrees":rom,
            "mmt_grade":mmt,"next_plan":next,"created_at":now
        }))
    }

    /// All physiotherapy writes derive actor identity from the Tauri session.
    /// The linked orthopedic episode must still be active.
    fn physio_require_active(conn: &Connection, id: &str) -> anyhow::Result<()> {
        let valid:bool=conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM physiotherapy_episodes f
             JOIN orthopedic_episodes e ON e.id=f.orthopedic_episode_id
             WHERE f.id=?1 AND f.status='active' AND e.status='active')",
            [id],|r|r.get(0))?;
        if !valid { bail!("Episodi i fizioterapisë nuk është aktiv ose nuk ekziston."); }
        Ok(())
    }

    pub fn physiotherapy_measurements_list(&self, episode: &str)
        -> anyhow::Result<Vec<serde_json::Value>> {
        let conn=self.conn.lock().unwrap();
        let mut stmt=conn.prepare(
            "SELECT id,physiotherapy_episode_id,session_id,kind,region,laterality,movement,
             value,unit,method,instrument,instrument_version,context,interpretation,
             assessor_id,assessed_at,correction_of,correction_reason,created_at
             FROM physiotherapy_measurements WHERE physiotherapy_episode_id=?1
             ORDER BY assessed_at DESC,created_at DESC")?;
        let rows=stmt.query_map([episode],|r|Ok(serde_json::json!({
            "id":r.get::<_,String>(0)?,"physiotherapy_episode_id":r.get::<_,String>(1)?,
            "session_id":r.get::<_,Option<String>>(2)?,"kind":r.get::<_,String>(3)?,
            "region":r.get::<_,String>(4)?,"laterality":r.get::<_,String>(5)?,
            "movement":r.get::<_,String>(6)?,"value":r.get::<_,f64>(7)?,
            "unit":r.get::<_,String>(8)?,"method":r.get::<_,String>(9)?,
            "instrument":r.get::<_,String>(10)?,"instrument_version":r.get::<_,String>(11)?,
            "context":r.get::<_,String>(12)?,"interpretation":r.get::<_,String>(13)?,
            "assessor_id":r.get::<_,String>(14)?,"assessed_at":r.get::<_,String>(15)?,
            "correction_of":r.get::<_,Option<String>>(16)?,
            "correction_reason":r.get::<_,String>(17)?,
            "created_at":r.get::<_,String>(18)?
        })))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn physiotherapy_measurement_add(
        &self,v:&serde_json::Value,actor:&str
    )->anyhow::Result<serde_json::Value> {
        if actor.trim().is_empty() { bail!("Kërkohet profesionist klinik i identifikuar."); }
        let ep=v["physiotherapy_episode_id"].as_str().unwrap_or_default();
        let kind=v["kind"].as_str().unwrap_or_default();
        let value=v["value"].as_f64().ok_or_else(|| anyhow!("Matja kërkon vlerë numerike."))?;
        if !value.is_finite() || value<0.0 || value>=1_000_000.0 {
            bail!("Vlerë matjeje jashtë kufijve.");
        }
        let (max,unit,integers)=match kind {
            "rom"=>(360.0,"deg",false),
            "mmt"=>(5.0,"grade_0_5",true),
            "nrs"=>(10.0,"nrs_0_10",true),
            "functional"=>(999999.0,"",false),
            _=>bail!("Lloji i matjes është i pavlefshëm.")
        };
        if value>max || (integers && value.fract()!=0.0) { bail!("Vlera nuk përputhet me shkallën klinike."); }
        let claimed_unit=v["unit"].as_str().unwrap_or_default();
        if kind!="functional" && claimed_unit!=unit { bail!("Njësi e papërshtatshme matjeje."); }
        if kind=="functional" && (claimed_unit.trim().is_empty() ||
            v["instrument"].as_str().unwrap_or_default().trim().is_empty() ||
            v["instrument_version"].as_str().unwrap_or_default().trim().is_empty()) {
            bail!("Testi funksional kërkon instrument, version dhe njësi.");
        }
        let region=v["region"].as_str().unwrap_or_default();
        let movement=v["movement"].as_str().unwrap_or_default();
        if (kind=="rom" && (region.trim().is_empty() || movement.trim().is_empty())) ||
           (kind=="mmt" && region.trim().is_empty()) {
            bail!("Matja kërkon regjionin dhe lëvizjen e përcaktuar.");
        }
        let side=v["laterality"].as_str().unwrap_or("not_applicable");
        if !matches!(side,"left"|"right"|"bilateral"|"midline"|"not_applicable") { bail!("Ana anatomike është e pavlefshme."); }
        let method=v["method"].as_str().unwrap_or_default();
        if method.trim().is_empty() { bail!("Metoda klinike është e detyrueshme."); }
        let assessed=v["assessed_at"].as_str().unwrap_or_default();
        chrono::NaiveDate::parse_from_str(assessed,"%Y-%m-%d")
            .map_err(|_|anyhow!("Data e matjes është e pavlefshme."))?;
        let correction=v["correction_of"].as_str().filter(|s|!s.trim().is_empty());
        let reason=v["correction_reason"].as_str().unwrap_or_default();
        if correction.is_some() && reason.trim().chars().count()<10 {
            bail!("Korrigjimi kërkon arsye klinike me të paktën 10 karaktere.");
        }
        let conn=self.conn.lock().unwrap();
        Self::physio_require_active(&conn,ep)?;
        let id=Uuid::new_v4().to_string();let now=now_iso();
        conn.execute(
            "INSERT INTO physiotherapy_measurements(
             id,physiotherapy_episode_id,session_id,kind,region,laterality,movement,value,
             unit,method,instrument,instrument_version,context,interpretation,assessor_id,
             assessed_at,correction_of,correction_reason,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
            rusqlite::params![id,ep,v["session_id"].as_str().filter(|s|!s.is_empty()),
                kind,region,side,movement,value,claimed_unit,method,
                v["instrument"].as_str().unwrap_or_default(),
                v["instrument_version"].as_str().unwrap_or_default(),
                v["context"].as_str().unwrap_or_default(),
                v["interpretation"].as_str().unwrap_or_default(),
                actor,assessed,correction,reason.trim(),now]
        )?;
        Ok(serde_json::json!({"id":id,"physiotherapy_episode_id":ep,"kind":kind,
            "region":region,"laterality":side,"movement":movement,"value":value,
            "unit":claimed_unit,"method":method,"instrument":v["instrument"],
            "instrument_version":v["instrument_version"],"context":v["context"],
            "interpretation":v["interpretation"],"assessor_id":actor,
            "assessed_at":assessed,"correction_of":correction,
            "correction_reason":reason.trim(),"created_at":now}))
    }

    pub fn physiotherapy_care_plans_list(&self,ep:&str)->anyhow::Result<Vec<serde_json::Value>> {
        let conn=self.conn.lock().unwrap();
        let mut s=conn.prepare(
            "SELECT id,physiotherapy_episode_id,version,status,goal,measurable_target,
             frequency,duration,interventions,precautions,phase,clinician_id,reason,created_at
             FROM physiotherapy_care_plans WHERE physiotherapy_episode_id=?1 ORDER BY version DESC")?;
        let rows=s.query_map([ep],|r|Ok(serde_json::json!({
            "id":r.get::<_,String>(0)?,"physiotherapy_episode_id":r.get::<_,String>(1)?,
            "version":r.get::<_,i64>(2)?,"status":r.get::<_,String>(3)?,
            "goal":r.get::<_,String>(4)?,"measurable_target":r.get::<_,String>(5)?,
            "frequency":r.get::<_,String>(6)?,"duration":r.get::<_,String>(7)?,
            "interventions":r.get::<_,String>(8)?,"precautions":r.get::<_,String>(9)?,
            "phase":r.get::<_,String>(10)?,"clinician_id":r.get::<_,String>(11)?,
            "reason":r.get::<_,String>(12)?,"created_at":r.get::<_,String>(13)?
        })))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn physiotherapy_care_plan_create(
        &self,v:&serde_json::Value,actor:&str
    )->anyhow::Result<serde_json::Value> {
        if actor.trim().is_empty(){bail!("Kërkohet profesionist i identifikuar.");}
        let ep=v["physiotherapy_episode_id"].as_str().unwrap_or_default();
        let goal=v["goal"].as_str().unwrap_or_default().trim();
        let status=v["status"].as_str().unwrap_or("draft");
        let reason=v["reason"].as_str().unwrap_or_default();
        if goal.chars().count()<8 || goal.chars().count()>3000 {
            bail!("Plani kërkon objektiv klinik (8–3000 karaktere).");
        }
        if !matches!(status,"draft"|"approved"|"amendment") {bail!("Status i pavlefshëm i planit.");}
        if status=="amendment" && reason.trim().chars().count()<10 {
            bail!("Amendamenti kërkon arsyetim.");
        }
        let mut conn=self.conn.lock().unwrap();
        let tx=conn.transaction()?;
        Self::physio_require_active(&tx,ep)?;
        let source_phase:String=tx.query_row(
            "SELECT p.phase FROM physiotherapy_episodes f
             JOIN orthopedic_rehab_plans p ON p.id=f.rehab_plan_id
             WHERE f.id=?1",[ep],|r|r.get(0))?;
        let incoming_phase=v["phase"].as_str().unwrap_or_default();
        if !incoming_phase.is_empty() && incoming_phase!=source_phase {
            bail!("Faza e trajtimit duhet të përputhet me protokollin ortopedik të miratuar.");
        }
        let last:i64=tx.query_row(
            "SELECT COALESCE(MAX(version),0) FROM physiotherapy_care_plans
             WHERE physiotherapy_episode_id=?1",[ep],|r|r.get(0))?;
        if status=="amendment" {
            let n:i64=tx.query_row(
                "SELECT COUNT(*) FROM physiotherapy_care_plans
                 WHERE physiotherapy_episode_id=?1 AND status IN ('approved','amendment')",
                [ep],|r|r.get(0))?;
            if n==0 {bail!("Amendamenti kërkon plan të miratuar më parë.");}
        }
        let id=Uuid::new_v4().to_string();let now=now_iso();let version=last+1;
        tx.execute(
            "INSERT INTO physiotherapy_care_plans(
             id,physiotherapy_episode_id,version,status,goal,measurable_target,frequency,
             duration,interventions,precautions,phase,clinician_id,reason,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
            rusqlite::params![id,ep,version,status,goal,
                v["measurable_target"].as_str().unwrap_or_default(),
                v["frequency"].as_str().unwrap_or_default(),
                v["duration"].as_str().unwrap_or_default(),
                v["interventions"].as_str().unwrap_or_default(),
                v["precautions"].as_str().unwrap_or_default(),
                source_phase,actor,reason.trim(),now]
        )?;
        tx.commit()?;
        Ok(serde_json::json!({"id":id,"physiotherapy_episode_id":ep,
            "version":version,"status":status,"goal":goal,
            "measurable_target":v["measurable_target"],"frequency":v["frequency"],
            "duration":v["duration"],"interventions":v["interventions"],
            "precautions":v["precautions"],"phase":source_phase,
            "clinician_id":actor,"reason":reason.trim(),"created_at":now}))
    }

    pub fn physiotherapy_exercises_list(&self)->anyhow::Result<Vec<serde_json::Value>>{
        let conn=self.conn.lock().unwrap();
        let mut stmt=conn.prepare(
            "SELECT id,title,anatomical_region,instructions,media_ref,media_rights,
             contraindications,version,created_by,created_at
             FROM physiotherapy_exercises ORDER BY title,version DESC")?;
        let rows=stmt.query_map([],|r|Ok(serde_json::json!({
            "id":r.get::<_,String>(0)?,"title":r.get::<_,String>(1)?,
            "anatomical_region":r.get::<_,String>(2)?,
            "instructions":r.get::<_,String>(3)?,"media_ref":r.get::<_,String>(4)?,
            "media_rights":r.get::<_,String>(5)?,
            "contraindications":r.get::<_,String>(6)?,
            "version":r.get::<_,i64>(7)?,
            "created_by":r.get::<_,String>(8)?,"created_at":r.get::<_,String>(9)?
        })))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn physiotherapy_exercise_create(
        &self,v:&serde_json::Value,actor:&str
    )->anyhow::Result<serde_json::Value>{
        if actor.trim().is_empty(){bail!("Kërkohet identiteti i profesionistit.");}
        let title=v["title"].as_str().unwrap_or_default().trim();
        let instructions=v["instructions"].as_str().unwrap_or_default().trim();
        let media=v["media_ref"].as_str().unwrap_or_default().trim();
        let rights=v["media_rights"].as_str().unwrap_or_default().trim();
        if title.chars().count()<3 || title.chars().count()>160 ||
           instructions.chars().count()<8 || instructions.chars().count()>4000 {
            bail!("Emri dhe udhëzimet e ushtrimit nuk janë të plota.");
        }
        if !media.is_empty() && (!media.starts_with("https://") || rights.len()<5 || media.len()>1500) {
            bail!("Video kërkon referencë HTTPS dhe konfirmim të të drejtave të përdorimit.");
        }
        let id=Uuid::new_v4().to_string();let now=now_iso();
        let conn=self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO physiotherapy_exercises
             (id,title,anatomical_region,instructions,media_ref,media_rights,
              contraindications,version,created_by,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,1,?8,?9)",
            rusqlite::params![id,title,v["anatomical_region"].as_str().unwrap_or_default(),
                instructions,media,rights,v["contraindications"].as_str().unwrap_or_default(),
                actor,now]
        )?;
        Ok(serde_json::json!({"id":id,"title":title,
            "anatomical_region":v["anatomical_region"],"instructions":instructions,
            "media_ref":media,"media_rights":rights,
            "contraindications":v["contraindications"],"version":1,
            "created_by":actor,"created_at":now}))
    }

    pub fn physiotherapy_hep_list(&self,ep:&str)->anyhow::Result<Vec<serde_json::Value>> {
        let conn=self.conn.lock().unwrap();
        let mut stmt=conn.prepare(
            "SELECT h.id,h.physiotherapy_episode_id,h.care_plan_id,h.exercise_id,
             x.title,x.instructions,x.media_ref,x.contraindications,
             h.sets,h.repetitions,h.frequency,h.duration_seconds,
             h.instructions,h.authorized_by,h.load_restrictions_snapshot,
             h.rom_restrictions_snapshot,h.status,h.replaces_id,h.created_at,
             (SELECT COUNT(*) FROM physiotherapy_hep_logs l
              WHERE l.hep_id=h.id AND l.performed=1),
             (CASE WHEN h.load_restrictions_snapshot=COALESCE(p.load_restrictions,'')
               AND h.rom_restrictions_snapshot=COALESCE(p.rom_restrictions,'')
               AND h.care_plan_id=(
                SELECT id FROM physiotherapy_care_plans cp
                WHERE cp.physiotherapy_episode_id=h.physiotherapy_episode_id
                ORDER BY version DESC LIMIT 1)
               THEN 0 ELSE 1 END)
             FROM physiotherapy_hep h
             JOIN physiotherapy_exercises x ON x.id=h.exercise_id
             JOIN physiotherapy_episodes f ON f.id=h.physiotherapy_episode_id
             JOIN orthopedic_rehab_plans p ON p.id=f.rehab_plan_id
             WHERE h.physiotherapy_episode_id=?1 ORDER BY h.created_at DESC")?;
        let rows=stmt.query_map([ep],|r|Ok(serde_json::json!({
            "id":r.get::<_,String>(0)?,"physiotherapy_episode_id":r.get::<_,String>(1)?,
            "care_plan_id":r.get::<_,String>(2)?,"exercise_id":r.get::<_,String>(3)?,
            "title":r.get::<_,String>(4)?,"exercise_instructions":r.get::<_,String>(5)?,
            "media_ref":r.get::<_,String>(6)?,"contraindications":r.get::<_,String>(7)?,
            "sets":r.get::<_,i64>(8)?,"repetitions":r.get::<_,i64>(9)?,
            "frequency":r.get::<_,String>(10)?,"duration_seconds":r.get::<_,Option<i64>>(11)?,
            "instructions":r.get::<_,String>(12)?,
            "authorized_by":r.get::<_,String>(13)?,
            "load_restrictions_snapshot":r.get::<_,String>(14)?,
            "rom_restrictions_snapshot":r.get::<_,String>(15)?,
            "status":r.get::<_,String>(16)?,"replaces_id":r.get::<_,Option<String>>(17)?,
            "created_at":r.get::<_,String>(18)?,"completed_logs":r.get::<_,i64>(19)?,
            "needs_review":r.get::<_,i64>(20)?==1
        })))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn physiotherapy_hep_assign(
        &self,v:&serde_json::Value,actor:&str
    )->anyhow::Result<serde_json::Value>{
        if actor.trim().is_empty(){bail!("Kërkohet miratues klinik.");}
        let ep=v["physiotherapy_episode_id"].as_str().unwrap_or_default();
        let plan=v["care_plan_id"].as_str().unwrap_or_default();
        let exercise=v["exercise_id"].as_str().unwrap_or_default();
        let sets=v["sets"].as_i64().unwrap_or(0);
        let reps=v["repetitions"].as_i64().unwrap_or(0);
        let freq=v["frequency"].as_str().unwrap_or_default().trim();
        if !(1..=30).contains(&sets) || !(1..=500).contains(&reps) ||
           freq.len()<3 || freq.len()>160 { bail!("Doza dhe frekuenca e HEP janë të pavlefshme."); }
        let duration=v["duration_seconds"].as_i64();
        if duration.map(|n|n<1||n>36000).unwrap_or(false) { bail!("Kohëzgjatje e pavlefshme."); }
        let status=v["status"].as_str().unwrap_or("approved");
        if !matches!(status,"approved"|"withdrawn"){bail!("Status HEP i pavlefshëm.");}
        let replacement=v["replaces_id"].as_str().filter(|s|!s.trim().is_empty());
        let conn=self.conn.lock().unwrap();
        Self::physio_require_active(&conn,ep)?;
        let permitted: Option<(String,String)>=conn.query_row(
            "SELECT COALESCE(p.load_restrictions,''),COALESCE(p.rom_restrictions,'')
             FROM physiotherapy_episodes f
             JOIN orthopedic_rehab_plans p ON p.id=f.rehab_plan_id
             JOIN physiotherapy_care_plans cp ON cp.physiotherapy_episode_id=f.id
             WHERE f.id=?1 AND cp.id=?2 AND cp.status IN ('approved','amendment')
             AND cp.version=(SELECT MAX(version) FROM physiotherapy_care_plans
                 WHERE physiotherapy_episode_id=f.id)",
            rusqlite::params![ep,plan],
            |r|Ok((r.get(0)?,r.get(1)?))
        ).optional()?;
        let (load,rom)=permitted.ok_or_else(||anyhow!(
            "HEP kërkon versionin aktual të planit të miratuar."
        ))?;
        let exercise_exists:bool=conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM physiotherapy_exercises WHERE id=?1)",
            [exercise],|r|r.get(0))?;
        if !exercise_exists{bail!("Ushtrimi nuk ekziston në katalog.");}
        if let Some(old_id)=replacement {
            let old_exists:bool=conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM physiotherapy_hep
                 WHERE id=?1 AND physiotherapy_episode_id=?2)",
                rusqlite::params![old_id,ep],|r|r.get(0))?;
            if !old_exists{bail!("Ushtrimi i zëvendësuar nuk i përket episodit.");}
        }
        let id=Uuid::new_v4().to_string();let now=now_iso();
        conn.execute(
            "INSERT INTO physiotherapy_hep
             (id,physiotherapy_episode_id,care_plan_id,exercise_id,sets,repetitions,
             frequency,duration_seconds,instructions,authorized_by,
             load_restrictions_snapshot,rom_restrictions_snapshot,status,replaces_id,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
            rusqlite::params![id,ep,plan,exercise,sets,reps,freq,duration,
                v["instructions"].as_str().unwrap_or_default(),actor,load,rom,
                status,replacement,now]
        )?;
        Ok(serde_json::json!({"id":id,"physiotherapy_episode_id":ep,
            "care_plan_id":plan,"exercise_id":exercise,"sets":sets,"repetitions":reps,
            "frequency":freq,"duration_seconds":duration,"authorized_by":actor,
            "load_restrictions_snapshot":load,"rom_restrictions_snapshot":rom,
            "status":status,"replaces_id":replacement,"created_at":now}))
    }

    pub fn physiotherapy_hep_logs_list(&self,ep:&str)
        ->anyhow::Result<Vec<serde_json::Value>>{
        let conn=self.conn.lock().unwrap();
        let mut stmt=conn.prepare(
            "SELECT l.id,l.hep_id,x.title,l.performed_date,l.self_reported,
             l.performed,l.response,l.recorded_by,l.created_at
             FROM physiotherapy_hep_logs l
             JOIN physiotherapy_hep h ON h.id=l.hep_id
             JOIN physiotherapy_exercises x ON x.id=h.exercise_id
             WHERE h.physiotherapy_episode_id=?1
             ORDER BY l.performed_date DESC,l.created_at DESC")?;
        let rows=stmt.query_map([ep],|r|Ok(serde_json::json!({
            "id":r.get::<_,String>(0)?,"hep_id":r.get::<_,String>(1)?,
            "exercise_title":r.get::<_,String>(2)?,
            "performed_date":r.get::<_,String>(3)?,
            "self_reported":r.get::<_,i64>(4)?==1,
            "performed":r.get::<_,i64>(5)?==1,
            "response":r.get::<_,String>(6)?,
            "recorded_by":r.get::<_,String>(7)?,
            "created_at":r.get::<_,String>(8)?
        })))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn physiotherapy_hep_log_add(
        &self,v:&serde_json::Value,actor:&str
    )->anyhow::Result<serde_json::Value>{
        if actor.trim().is_empty(){bail!("Kërkohet përdorues i identifikuar.");}
        let hep=v["hep_id"].as_str().unwrap_or_default();
        let day=v["performed_date"].as_str().unwrap_or_default();
        chrono::NaiveDate::parse_from_str(day,"%Y-%m-%d")
            .map_err(|_|anyhow!("Data e ushtrimit nuk është e vlefshme."))?;
        let response=v["response"].as_str().unwrap_or_default();
        if response.len()>2000 {bail!("Përgjigje shumë e gjatë.");}
        let performed=v["performed"].as_bool().ok_or_else(||anyhow!("Mungon statusi i kryerjes."))?;
        let self_reported=v["self_reported"].as_bool().unwrap_or(true);
        let conn=self.conn.lock().unwrap();
        let valid:bool=conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM physiotherapy_hep h
             JOIN physiotherapy_episodes f ON f.id=h.physiotherapy_episode_id
             JOIN orthopedic_episodes e ON e.id=f.orthopedic_episode_id
             JOIN orthopedic_rehab_plans p ON p.id=f.rehab_plan_id
             WHERE h.id=?1 AND h.status='approved' AND f.status='active'
             AND e.status='active'
             AND NOT EXISTS(SELECT 1 FROM physiotherapy_hep h2
               WHERE h2.replaces_id=h.id)
             AND h.load_restrictions_snapshot=COALESCE(p.load_restrictions,'')
             AND h.rom_restrictions_snapshot=COALESCE(p.rom_restrictions,'')
             AND h.care_plan_id=(
                SELECT id FROM physiotherapy_care_plans cp
                WHERE cp.physiotherapy_episode_id=f.id
                ORDER BY version DESC LIMIT 1))",
            [hep],|r|r.get(0))?;
        if !valid{bail!("Ushtrimi kërkon rishikim ose nuk është më aktiv.");}
        let id=Uuid::new_v4().to_string();let now=now_iso();
        conn.execute(
            "INSERT INTO physiotherapy_hep_logs
             (id,hep_id,performed_date,self_reported,performed,response,recorded_by,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            rusqlite::params![id,hep,day,if self_reported{1}else{0},
                if performed{1}else{0},response,actor,now]
        )?;
        Ok(serde_json::json!({"id":id,"hep_id":hep,"performed_date":day,
            "self_reported":self_reported,"performed":performed,
            "response":response,"recorded_by":actor,"created_at":now}))
    }

    pub fn physiotherapy_reports_list(&self,ep:&str)->anyhow::Result<Vec<serde_json::Value>> {
        let conn=self.conn.lock().unwrap();
        let mut stmt=conn.prepare(
            "SELECT id,physiotherapy_episode_id,type,version,status,narrative,
             clinical_summary_json,author_id,reason,created_at
             FROM physiotherapy_reports WHERE physiotherapy_episode_id=?1
             ORDER BY created_at DESC,version DESC")?;
        let rows=stmt.query_map([ep],|r|Ok(serde_json::json!({
            "id":r.get::<_,String>(0)?,"physiotherapy_episode_id":r.get::<_,String>(1)?,
            "type":r.get::<_,String>(2)?,"version":r.get::<_,i64>(3)?,
            "status":r.get::<_,String>(4)?,"narrative":r.get::<_,String>(5)?,
            "clinical_summary_json":r.get::<_,String>(6)?,
            "author_id":r.get::<_,String>(7)?,"reason":r.get::<_,String>(8)?,
            "created_at":r.get::<_,String>(9)?
        })))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn physiotherapy_report_create(
        &self,v:&serde_json::Value,actor:&str
    )->anyhow::Result<serde_json::Value>{
        if actor.trim().is_empty(){bail!("Nënshkruesi klinik është i detyrueshëm.");}
        let ep=v["physiotherapy_episode_id"].as_str().unwrap_or_default();
        let kind=v["type"].as_str().unwrap_or_default();
        let status=v["status"].as_str().unwrap_or("draft");
        let narrative=v["narrative"].as_str().unwrap_or_default().trim();
        let reason=v["reason"].as_str().unwrap_or_default().trim();
        if !matches!(kind,"initial"|"progress"|"discharge") ||
           !matches!(status,"draft"|"final"|"amendment"){
            bail!("Lloji/statusi i raportit nuk mbështetet.");
        }
        if narrative.chars().count()<15 || narrative.chars().count()>25000{
            bail!("Raporti kërkon përshkrim klinik 15–25000 karaktere.");
        }
        if status=="amendment" && reason.chars().count()<10 {
            bail!("Amendamenti kërkon arsye klinike.");
        }
        // Clinical snapshot contains saved events, not HTML or client-authored
        // pseudo-history. Record is immutable even when later data change.
        let sessions=self.physiotherapy_sessions_list(ep)?;
        let measurements=self.physiotherapy_measurements_list(ep)?;
        let plans=self.physiotherapy_care_plans_list(ep)?;
        let hep=self.physiotherapy_hep_list(ep)?;
        let snapshot=serde_json::json!({
            "sessions":sessions,"measurements":measurements,
            "care_plans":plans,"home_exercises":hep
        }).to_string();
        let mut conn=self.conn.lock().unwrap();
        let tx=conn.transaction()?;
        let episode_state:Option<String>=tx.query_row(
            "SELECT e.status FROM physiotherapy_episodes f
             JOIN orthopedic_episodes e ON e.id=f.orthopedic_episode_id
             WHERE f.id=?1",[ep],|r|r.get(0)).optional()?;
        if episode_state.is_none(){bail!("Episodi nuk ekziston.");}
        if episode_state.as_deref()==Some("closed") && status!="amendment" {
            bail!("Pas mbylljes pranohen vetëm amendamente të raporteve finale.");
        }
        let last:i64=tx.query_row(
            "SELECT COALESCE(MAX(version),0) FROM physiotherapy_reports
             WHERE physiotherapy_episode_id=?1 AND type=?2",
            rusqlite::params![ep,kind],|r|r.get(0))?;
        if status=="amendment" {
            let signed:i64=tx.query_row(
                "SELECT COUNT(*) FROM physiotherapy_reports
                 WHERE physiotherapy_episode_id=?1 AND type=?2 AND status='final'",
                rusqlite::params![ep,kind],|r|r.get(0))?;
            if signed==0{bail!("Amendamenti kërkon raport final ekzistues.");}
        }
        let id=Uuid::new_v4().to_string();let now=now_iso();let version=last+1;
        tx.execute(
            "INSERT INTO physiotherapy_reports
             (id,physiotherapy_episode_id,type,version,status,narrative,
             clinical_summary_json,author_id,reason,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            rusqlite::params![id,ep,kind,version,status,narrative,snapshot,actor,reason,now]
        )?;
        tx.commit()?;
        Ok(serde_json::json!({"id":id,"physiotherapy_episode_id":ep,
            "type":kind,"version":version,"status":status,"narrative":narrative,
            "clinical_summary_json":snapshot,"author_id":actor,
            "reason":reason,"created_at":now}))
    }

    /// Patient identity and restrictions are retrieved exclusively through
    /// server-side joins. Can be used for both clinical reports and referrals.
    pub fn physiotherapy_document_data(&self,document_id:&str,kind:&str)
        ->anyhow::Result<serde_json::Value>{
        let conn=self.conn.lock().unwrap();
        if kind=="referral" {
            return conn.query_row(
                "SELECT f.id,f.client_id,c.name,e.diagnosis,e.region,e.laterality,
                 p.protocol,p.phase,COALESCE(p.load_restrictions,''),
                 COALESCE(p.rom_restrictions,''),f.created_by,f.created_at
                 FROM physiotherapy_episodes f
                 JOIN clients c ON c.id=f.client_id
                 JOIN orthopedic_episodes e ON e.id=f.orthopedic_episode_id
                 JOIN orthopedic_rehab_plans p ON p.id=f.rehab_plan_id
                 WHERE f.id=?1", [document_id], |r|Ok(serde_json::json!({
                    "id":r.get::<_,String>(0)?,"patient_id":r.get::<_,String>(1)?,
                    "patient_name":r.get::<_,String>(2)?,
                    "diagnosis":r.get::<_,String>(3)?,"region":r.get::<_,String>(4)?,
                    "laterality":r.get::<_,String>(5)?,
                    "protocol":r.get::<_,String>(6)?,
                    "phase":r.get::<_,String>(7)?,
                    "load_restrictions":r.get::<_,String>(8)?,
                    "rom_restrictions":r.get::<_,String>(9)?,
                    "author_id":r.get::<_,String>(10)?,
                    "created_at":r.get::<_,String>(11)?
                }))
            ).optional()?.ok_or_else(||anyhow!("Referimi nuk u gjet."));
        }
        conn.query_row(
            "SELECT r.id,r.physiotherapy_episode_id,r.type,r.version,r.status,
             r.narrative,r.clinical_summary_json,r.author_id,r.reason,r.created_at,
             c.name,c.id,e.diagnosis,e.region,e.laterality,p.protocol,p.phase,
             COALESCE(p.load_restrictions,''),COALESCE(p.rom_restrictions,'')
             FROM physiotherapy_reports r
             JOIN physiotherapy_episodes f ON f.id=r.physiotherapy_episode_id
             JOIN clients c ON c.id=f.client_id
             JOIN orthopedic_episodes e ON e.id=f.orthopedic_episode_id
             JOIN orthopedic_rehab_plans p ON p.id=f.rehab_plan_id
             WHERE r.id=?1",[document_id],|r|Ok(serde_json::json!({
                "id":r.get::<_,String>(0)?,
                "physiotherapy_episode_id":r.get::<_,String>(1)?,
                "type":r.get::<_,String>(2)?,"version":r.get::<_,i64>(3)?,
                "status":r.get::<_,String>(4)?,
                "narrative":r.get::<_,String>(5)?,
                "clinical_summary_json":r.get::<_,String>(6)?,
                "author_id":r.get::<_,String>(7)?,
                "reason":r.get::<_,String>(8)?,
                "created_at":r.get::<_,String>(9)?,
                "patient_name":r.get::<_,String>(10)?,
                "patient_id":r.get::<_,String>(11)?,
                "diagnosis":r.get::<_,String>(12)?,
                "region":r.get::<_,String>(13)?,
                "laterality":r.get::<_,String>(14)?,
                "protocol":r.get::<_,String>(15)?,
                "phase":r.get::<_,String>(16)?,
                "load_restrictions":r.get::<_,String>(17)?,
                "rom_restrictions":r.get::<_,String>(18)?
            }))
        ).optional()?.ok_or_else(||anyhow!("Versioni i raportit nuk u gjet."))
    }

    pub fn physiotherapy_document_register(
        &self,episode:&str,category:&str,filename:&str,
        relative_path:&str,sha256:&str,size:i64,source_note:&str,actor:&str
    )->anyhow::Result<serde_json::Value>{
        if !matches!(category,"assessment"|"progress"|"discharge"|"other") ||
            actor.trim().is_empty() || filename.trim().is_empty() ||
            source_note.chars().count()>2000 || size < 100 || size > 20*1024*1024 ||
            sha256.len()!=64 || !sha256.bytes().all(|b|b.is_ascii_hexdigit()) ||
            !relative_path.starts_with("rehab-pdf/") {
            bail!("Regjistrimi i dokumentit klinik është i pavlefshëm.");
        }
        let conn=self.conn.lock().unwrap();
        let client:String=conn.query_row(
            "SELECT client_id FROM orthopedic_episodes WHERE id=?1 AND status='active'",
            [episode],|r|r.get(0)).optional()?.ok_or_else(
                ||anyhow!("Episodi ortopedik nuk është aktiv."))?;
        let id=Uuid::new_v4().to_string();let created=now_iso();
        conn.execute(
            "INSERT INTO physiotherapy_documents
             (id,orthopedic_episode_id,client_id,category,display_name,storage_path,
              content_sha256,content_size,source_note,clinician_id,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            rusqlite::params![id,episode,client,category,filename,relative_path,
                sha256,size,source_note,actor,created]
        )?;
        Ok(serde_json::json!({
            "id":id,"orthopedic_episode_id":episode,"client_id":client,
            "category":category,"display_name":filename,
            "content_sha256":sha256,"content_size":size,
            "source_note":source_note,"clinician_id":actor,"created_at":created
        }))
    }

    pub fn physiotherapy_documents_list(&self,episode:&str)
        ->anyhow::Result<Vec<serde_json::Value>>{
        let conn=self.conn.lock().unwrap();
        let mut stmt=conn.prepare(
            "SELECT id,orthopedic_episode_id,client_id,category,display_name,
             content_sha256,content_size,source_note,clinician_id,created_at
             FROM physiotherapy_documents WHERE orthopedic_episode_id=?1
             ORDER BY created_at DESC")?;
        let rows=stmt.query_map([episode],|r|Ok(serde_json::json!({
            "id":r.get::<_,String>(0)?,"orthopedic_episode_id":r.get::<_,String>(1)?,
            "client_id":r.get::<_,String>(2)?,"category":r.get::<_,String>(3)?,
            "display_name":r.get::<_,String>(4)?,"content_sha256":r.get::<_,String>(5)?,
            "content_size":r.get::<_,i64>(6)?,"source_note":r.get::<_,String>(7)?,
            "clinician_id":r.get::<_,String>(8)?,"created_at":r.get::<_,String>(9)?
        })))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn physiotherapy_document_file(&self,id:&str)
        ->anyhow::Result<(String,String)>{
        let conn=self.conn.lock().unwrap();
        let row=conn.query_row(
            "SELECT storage_path,content_sha256 FROM physiotherapy_documents WHERE id=?1",
            [id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))
        ).optional()?.ok_or_else(||anyhow!("Dokumenti nuk u gjet."))?;
        Ok(row)
    }

    pub fn physiotherapy_body_markers_list(&self,ep:&str)
        ->anyhow::Result<Vec<serde_json::Value>>{
        let conn=self.conn.lock().unwrap();
        let mut stmt=conn.prepare(
            "SELECT id,physiotherapy_episode_id,view,region,laterality,classification,
             x,y,note,clinician_id,observed_at FROM physiotherapy_body_markers
             WHERE physiotherapy_episode_id=?1 ORDER BY observed_at DESC")?;
        let rows=stmt.query_map([ep],|r|Ok(serde_json::json!({
            "id":r.get::<_,String>(0)?,
            "physiotherapy_episode_id":r.get::<_,String>(1)?,
            "view":r.get::<_,String>(2)?,
            "region":r.get::<_,String>(3)?,
            "laterality":r.get::<_,String>(4)?,
            "classification":r.get::<_,String>(5)?,
            "x":r.get::<_,f64>(6)?,"y":r.get::<_,f64>(7)?,
            "note":r.get::<_,String>(8)?,
            "clinician_id":r.get::<_,String>(9)?,
            "observed_at":r.get::<_,String>(10)?
        })))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn physiotherapy_body_marker_add(&self,v:&serde_json::Value,actor:&str)
        ->anyhow::Result<serde_json::Value>{
        if actor.trim().is_empty(){bail!("Kërkohet identiteti i profesionistit.");}
        let ep=v["physiotherapy_episode_id"].as_str().unwrap_or_default();
        let view=v["view"].as_str().unwrap_or_default();
        let side=v["laterality"].as_str().unwrap_or_default();
        let classification=v["classification"].as_str().unwrap_or_default();
        let region=v["region"].as_str().unwrap_or_default();
        let note=v["note"].as_str().unwrap_or_default();
        if !matches!(view,"anterior"|"posterior"|"lateral") ||
            !matches!(side,"left"|"right"|"bilateral"|"midline") ||
            !matches!(classification,"pain"|"restriction"|"weakness"|"other") ||
            region.trim().is_empty() || note.chars().count()>2000{
            bail!("Shenja anatomike kërkon regjion, anë dhe klasifikim.");
        }
        let x=v["x"].as_f64().ok_or_else(||anyhow!("Mungon pozicioni X."))?;
        let y=v["y"].as_f64().ok_or_else(||anyhow!("Mungon pozicioni Y."))?;
        if !x.is_finite()||!y.is_finite()||!(0.0..=1.0).contains(&x)
           ||!(0.0..=1.0).contains(&y){bail!("Koordinata anatomike jashtë intervalit 0..1.");}
        let conn=self.conn.lock().unwrap();
        Self::physio_require_active(&conn,ep)?;
        let id=Uuid::new_v4().to_string();let at=now_iso();
        conn.execute(
            "INSERT INTO physiotherapy_body_markers(
             id,physiotherapy_episode_id,view,region,laterality,classification,
             x,y,note,clinician_id,observed_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            rusqlite::params![id,ep,view,region,side,classification,x,y,note,actor,at]
        )?;
        Ok(serde_json::json!({"id":id,"physiotherapy_episode_id":ep,
            "view":view,"region":region,"laterality":side,
            "classification":classification,"x":x,"y":y,
            "note":note,"clinician_id":actor,"observed_at":at}))
    }

    pub fn orthopedic_episodes_list(&self, client_id: Option<String>) -> anyhow::Result<Vec<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, client_id, diagnosis, region, laterality, status, notes, created_at, updated_at FROM orthopedic_episodes WHERE (?1 IS NULL OR client_id = ?1) ORDER BY updated_at DESC")?;
        let rows = stmt.query_map([client_id], |r| Ok(serde_json::json!({"id":r.get::<_,String>(0)?,"client_id":r.get::<_,String>(1)?,"diagnosis":r.get::<_,String>(2)?,"region":r.get::<_,String>(3)?,"laterality":r.get::<_,String>(4)?,"status":r.get::<_,String>(5)?,"notes":r.get::<_,String>(6)?,"created_at":r.get::<_,String>(7)?,"updated_at":r.get::<_,String>(8)?})))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn orthopedic_episode_upsert(&self, episode: &serde_json::Value) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let client_id = episode["client_id"].as_str().unwrap_or_default();
        if client_id.trim().is_empty() ||
            !conn.query_row("SELECT EXISTS(SELECT 1 FROM clients WHERE id=?1 AND deleted=0)",
                rusqlite::params![client_id], |r| r.get::<_, bool>(0))? {
            bail!("Episodi ortopedik duhet të lidhet me një pacient ekzistues.");
        }
        let incoming_status = episode["status"].as_str().unwrap_or("active");
        if incoming_status != "active" {
            bail!("Statusi i episodit ndryshohet vetëm përmes procesit të mbylljes/rihapjes.");
        }
        let episode_id = episode["id"].as_str().unwrap_or_default();
        if episode_id.trim().is_empty() { bail!("Mungon identifikuesi i episodit."); }
        let existing: Option<(String,String)> = conn.query_row(
            "SELECT client_id,status FROM orthopedic_episodes WHERE id=?1", [episode_id],
            |r| Ok((r.get(0)?,r.get(1)?))).optional()?;
        if let Some((patient, status)) = existing {
            if patient != client_id { bail!("Pacienti i episodit ekzistues nuk mund të ndryshohet."); }
            if status != "active" { bail!("Episodi i mbyllur nuk mund të modifikohet."); }
        }
        let side = episode["laterality"].as_str().unwrap_or_default();
        if !matches!(side, "left" | "right" | "bilateral" | "midline" | "not_applicable") {
            bail!("Ana anatomike nuk është e vlefshme.");
        }
        conn.execute("INSERT INTO orthopedic_episodes (id,client_id,diagnosis,region,laterality,status,notes,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(id) DO UPDATE SET diagnosis=excluded.diagnosis,region=excluded.region,laterality=excluded.laterality,status=excluded.status,notes=excluded.notes,updated_at=excluded.updated_at", rusqlite::params![episode["id"].as_str().unwrap_or_default(),episode["client_id"].as_str().unwrap_or_default(),episode["diagnosis"].as_str().unwrap_or_default(),episode["region"].as_str().unwrap_or_default(),episode["laterality"].as_str().unwrap_or_default(),episode["status"].as_str().unwrap_or("active"),episode["notes"].as_str().unwrap_or_default(),episode["created_at"].as_str().unwrap_or_default(),episode["updated_at"].as_str().unwrap_or_default()])?;
        drop(conn);
        self.orthopedic_audit("episode", episode["id"].as_str().unwrap_or_default(), episode)
    }

    pub fn orthopedic_audit(&self, entity_type: &str, entity_id: &str, payload: &serde_json::Value) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let body = payload.to_string();
        conn.execute("INSERT INTO orthopedic_audit_log(id,entity_type,entity_id,action,payload_json,created_at) VALUES(?1,?2,?3,'upsert',?4,datetime('now'))", rusqlite::params![uuid::Uuid::new_v4().to_string(), entity_type, entity_id, body.clone()])?;
        // Do NOT enqueue orthopedics records into the existing generic sync
        // queue: Supabase MJEKU has no versioned orthopedic remote schema yet.
        // Queuing them would repeatedly fail, and last-write-wins risks loss
        // of finalized clinical records. Keep local audit immutable until
        // an authenticated, conflict-aware clinical sync migration is deployed.
        Ok(())
    }

    /// Versioned discharge and reopening. Actor identity comes from the authenticated
    /// Tauri session, never the request body. Both writes share one transaction.
    pub fn orthopedic_episode_transition(
        &self, episode_id: &str, action: &str, reason: &str, clinician: &str,
    ) -> anyhow::Result<serde_json::Value> {
        if clinician.trim().is_empty() { bail!("Kërkohet identiteti i mjekut."); }
        if reason.trim().chars().count() < 10 || reason.chars().count() > 2000 {
            bail!("Arsyeja klinike duhet të jetë 10 deri në 2000 karaktere.");
        }
        let (expected, next) = match action {
            "close" => ("active", "closed"),
            "reopen" => ("closed", "active"),
            _ => bail!("Veprim i panjohur për episodin."),
        };
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let current: String = tx.query_row(
            "SELECT status FROM orthopedic_episodes WHERE id=?1",
            [episode_id], |r| r.get(0)
        ).optional()?.ok_or_else(|| anyhow!("Episodi ortopedik nuk ekziston."))?;
        if current != expected {
            bail!("Kalimi i statusit nuk lejohet nga gjendja aktuale e episodit.");
        }
        // Closing requires an actual final_summary report signed by a doctor.
        let final_report_id: Option<String> = if action == "close" {
            Some(tx.query_row(
                "SELECT id FROM orthopedic_report_versions
                 WHERE episode_id=?1 AND type='final_summary' AND status='final'
                 ORDER BY version DESC LIMIT 1",
                [episode_id], |r| r.get(0)
            ).optional()?.ok_or_else(|| anyhow!(
                "Mbyllja kërkon raportin final 'Përmbledhja finale' të nënshkruar nga mjeku."
            ))?)
        } else { None };
        let at = now_iso();
        tx.execute(
            "UPDATE orthopedic_episodes SET status=?1,updated_at=?2 WHERE id=?3 AND status=?4",
            rusqlite::params![next,at,episode_id,expected]
        )?;
        let id = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO orthopedic_episode_transitions
             (id,episode_id,from_status,to_status,reason,clinician_id,final_report_id,occurred_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            rusqlite::params![id,episode_id,expected,next,reason.trim(),clinician,final_report_id,at]
        )?;
        tx.commit()?;
        Ok(serde_json::json!({"id":id,"episode_id":episode_id,"from_status":expected,
           "to_status":next,"reason":reason.trim(),"clinician_id":clinician,
           "final_report_id":final_report_id,"occurred_at":at}))
    }

    pub fn orthopedic_episode_transitions_list(
        &self, episode_id: &str,
    ) -> anyhow::Result<Vec<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        Self::orthopedic_require_episode(&conn, episode_id)?;
        let mut stmt = conn.prepare(
            "SELECT id,episode_id,from_status,to_status,reason,clinician_id,final_report_id,occurred_at
             FROM orthopedic_episode_transitions WHERE episode_id=?1
             ORDER BY occurred_at DESC,rowid DESC"
        )?;
        let entries = stmt.query_map([episode_id], |r| Ok(serde_json::json!({
            "id":r.get::<_,String>(0)?,"episode_id":r.get::<_,String>(1)?,
            "from_status":r.get::<_,String>(2)?,"to_status":r.get::<_,String>(3)?,
            "reason":r.get::<_,String>(4)?,"clinician_id":r.get::<_,String>(5)?,
            "final_report_id":r.get::<_,Option<String>>(6)?,
            "occurred_at":r.get::<_,String>(7)?
        })))?;
        Ok(entries.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn orthopedic_surgery_upsert(&self, v: &serde_json::Value) -> anyhow::Result<()> {
        let c=self.conn.lock().unwrap();
        Self::orthopedic_require_episode(&c, v["episode_id"].as_str().unwrap_or_default())?;
        if matches!(v["status"].as_str().unwrap_or("draft"), "in_progress" | "completed") {
            bail!("Plani kirurgjik nuk është raport operativ. Regjistro ndërhyrjen reale veçmas.");
        } let now=v["created_at"].as_str().unwrap_or_default();
        c.execute("INSERT INTO orthopedic_surgeries(id,episode_id,procedure,status,side,site,team,consent_status,planned_date,preop_checklist,postop_plan,payload_json,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?13) ON CONFLICT(id) DO UPDATE SET procedure=excluded.procedure,status=excluded.status,side=excluded.side,site=excluded.site,team=excluded.team,consent_status=excluded.consent_status,planned_date=excluded.planned_date,preop_checklist=excluded.preop_checklist,postop_plan=excluded.postop_plan,payload_json=excluded.payload_json,updated_at=excluded.updated_at", rusqlite::params![v["id"].as_str().unwrap_or_default(),v["episode_id"].as_str().unwrap_or_default(),v["procedure"].as_str().unwrap_or_default(),v["status"].as_str().unwrap_or("draft"),v["side"].as_str().unwrap_or_default(),v["site"].as_str().unwrap_or_default(),v["team"].as_str().unwrap_or_default(),v["consent_status"].as_str().unwrap_or("pending"),v["planned_date"].as_str().unwrap_or_default(),v["preop_checklist"].as_str().unwrap_or_default(),v["postop_plan"].as_str().unwrap_or_default(),v.to_string(),now])?; Ok(())
    }
    pub fn orthopedic_implant_upsert(&self, v: &serde_json::Value, actor: &str) -> anyhow::Result<()> {
        if actor.trim().is_empty() { bail!("Identiteti i përdoruesit kërkohet."); }
        let mut c = self.conn.lock().unwrap();
        let surgery_id = v["surgery_id"].as_str().unwrap_or_default();
        Self::orthopedic_require_surgery(&c, surgery_id)?;
        let id = v["id"].as_str().unwrap_or_default();
        if id.trim().is_empty() { bail!("Mungon identifikuesi i implantit."); }
        let status = v["status"].as_str().unwrap_or("reserved");
        let removal = v["removal_status"].as_str().unwrap_or("not_implanted");
        if !matches!(status, "reserved" | "implanted")
            || !matches!(removal, "not_implanted" | "in_body" | "removed") {
            bail!("Statusi i implantit nuk është i vlefshëm.");
        }
        let serial = v["serial"].as_str().unwrap_or_default().trim();
        let lot = v["lot"].as_str().unwrap_or_default().trim();
        if status == "implanted" {
            if serial.is_empty() && lot.is_empty() { bail!("Implanti i konfirmuar kërkon lot ose serial."); }
            if v["sterility_status"].as_str() != Some("sterile") {
                bail!("Implanti nuk është konfirmuar steril.");
            }
            let expires = v["expiration_date"].as_str().unwrap_or_default();
            if !expires.is_empty() && expires < chrono::Utc::now().format("%Y-%m-%d").to_string().as_str() {
                bail!("Implanti është i skaduar.");
            }
        } else if removal != "not_implanted" {
            bail!("Implanti i rezervuar nuk mund të shënohet në trup.");
        }
        let tx = c.transaction()?;
        let previous: Option<(String,String)> = tx.query_row(
            "SELECT status,removal_status FROM orthopedic_implants WHERE id=?1",
            rusqlite::params![id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        if status=="implanted" && removal=="removed" &&
           previous.as_ref().map(|(old,_)|old.as_str()) != Some("implanted") {
            bail!("Implanti nuk mund të shënohet i hequr pa histori implantimi.");
        }
        if let Some((ref old_status,ref old_removal)) = previous {
            if old_status == "implanted" && status != "implanted" {
                bail!("Implanti i vendosur nuk rikthehet në rezervim.");
            }
            if old_removal == "removed" && removal != "removed" {
                bail!("Implanti i hequr nuk mund të rivendoset pa regjistrim të ri.");
            }
        }
        let mut actual_operation: Option<String> = None;
        if status == "implanted" {
            actual_operation = tx.query_row(
                "SELECT id FROM orthopedic_operations WHERE plan_id=?1 ORDER BY finalized_at DESC LIMIT 1",
                rusqlite::params![surgery_id], |r|r.get::<_,String>(0)).optional()?;
            if actual_operation.is_none() {
                bail!("Implantimi real kërkon raport operativ të finalizuar, jo vetëm planin.");
            }
            if !serial.is_empty() && tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM orthopedic_implants WHERE serial=?1 AND id<>?2 AND status='implanted' AND removal_status='in_body')",
                rusqlite::params![serial,id], |r|r.get::<_,bool>(0))? {
                bail!("Ky numër serial është implantuar te një pacient tjetër.");
            }
        }
        tx.execute("INSERT INTO orthopedic_implants(
            id,surgery_id,udi,lot,serial,status,manufacturer,model,catalog_ref,udi_di,udi_pi,expiration_date,sterility_status,
            anatomical_site,implantation_date,removal_status,payload_json,created_at)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)
            ON CONFLICT(id) DO UPDATE SET
            status=excluded.status,removal_status=excluded.removal_status,
            implantation_date=excluded.implantation_date,payload_json=excluded.payload_json",
            rusqlite::params![
                id,surgery_id,v["udi"].as_str().unwrap_or_default(),lot,serial,status,
                v["manufacturer"].as_str().unwrap_or_default(),
                v["model"].as_str().unwrap_or_default(),v["catalog_ref"].as_str().unwrap_or_default(),
                v["udi_di"].as_str().unwrap_or_default(),v["udi_pi"].as_str().unwrap_or_default(),
                v["expiration_date"].as_str().unwrap_or_default(),
                v["sterility_status"].as_str().unwrap_or_default(),
                v["anatomical_site"].as_str().unwrap_or_default(),
                v["implantation_date"].as_str().unwrap_or_default(),removal,
                v.to_string(),v["created_at"].as_str().unwrap_or_default()
            ])?;
        let event = if previous.is_none() { Some(status) }
            else if previous.as_ref().map(|(s,_)|s.as_str()) != Some(status) {
                Some("implanted")
            } else if previous.as_ref().map(|(_,r)|r.as_str()) != Some(removal)
                && removal=="removed" { Some("removed") }
            else { None };
        if let Some(action)=event {
            tx.execute("INSERT INTO orthopedic_implant_events(
                id,implant_id,operation_id,action,clinician_id,note,occurred_at)
                VALUES(?1,?2,?3,?4,?5,?6,?7)",
                rusqlite::params![Uuid::new_v4().to_string(),id,actual_operation,
                    action,actor,v["change_reason"].as_str().unwrap_or_default(),now_iso()])?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn orthopedic_rehab_upsert(&self, v: &serde_json::Value) -> anyhow::Result<()> {
        let c=self.conn.lock().unwrap();
        Self::orthopedic_require_episode(&c, v["episode_id"].as_str().unwrap_or_default())?; c.execute("INSERT INTO orthopedic_rehab_plans(id,episode_id,protocol,phase,load_restrictions,rom_restrictions,approving_surgeon,start_date,status,payload_json,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11) ON CONFLICT(id) DO UPDATE SET protocol=excluded.protocol,phase=excluded.phase,load_restrictions=excluded.load_restrictions,rom_restrictions=excluded.rom_restrictions,approving_surgeon=excluded.approving_surgeon,start_date=excluded.start_date,status=excluded.status,payload_json=excluded.payload_json", rusqlite::params![v["id"].as_str().unwrap_or_default(),v["episode_id"].as_str().unwrap_or_default(),v["protocol"].as_str().unwrap_or_default(),v["phase"].as_str().unwrap_or("phase_1"),v["load_restrictions"].as_str().unwrap_or_default(),v["rom_restrictions"].as_str().unwrap_or_default(),v["approving_surgeon"].as_str().unwrap_or_default(),v["start_date"].as_str().unwrap_or_default(),v["status"].as_str().unwrap_or("active"),v.to_string(),v["created_at"].as_str().unwrap_or_default()])?; Ok(())
    }

    // Keep the full structured payload for existing WIP records. The linked
    // episode is verified by SQL JOIN; records never leak across patients.
    pub fn orthopedic_fracture_followups_list(&self, fracture_id: &str) -> anyhow::Result<Vec<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id,fracture_id,episode_id,assessment_date,clinical_findings,pain_score,functional_status,alignment,healing_status,imaging_ref,complications,treatment_changes,next_step,clinician_id,created_at FROM orthopedic_fracture_followups WHERE fracture_id=?1 ORDER BY assessment_date,created_at")?;
        let rows = stmt.query_map(rusqlite::params![fracture_id], |r| Ok(serde_json::json!({
            "id":r.get::<_,String>(0)?, "fracture_id":r.get::<_,String>(1)?,
            "episode_id":r.get::<_,String>(2)?, "assessment_date":r.get::<_,String>(3)?,
            "clinical_findings":r.get::<_,String>(4)?, "pain_score":r.get::<_,Option<i64>>(5)?,
            "functional_status":r.get::<_,String>(6)?, "alignment":r.get::<_,String>(7)?,
            "healing_status":r.get::<_,String>(8)?, "imaging_ref":r.get::<_,String>(9)?,
            "complications":r.get::<_,String>(10)?, "treatment_changes":r.get::<_,String>(11)?,
            "next_step":r.get::<_,String>(12)?, "clinician_id":r.get::<_,String>(13)?,
            "created_at":r.get::<_,String>(14)?
        })))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn orthopedic_fracture_followup_add(&self, v: &serde_json::Value, clinician_id: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let episode = v["episode_id"].as_str().unwrap_or_default();
        Self::orthopedic_require_episode(&conn, episode)?;
        let fracture = v["fracture_id"].as_str().unwrap_or_default();
        let owner: Option<String> = conn.query_row(
            "SELECT episode_id FROM orthopedic_fractures WHERE id=?1", [fracture], |r| r.get(0)
        ).optional()?;
        if owner.as_deref() != Some(episode) { bail!("Fraktura nuk i përket episodit të zgjedhur."); }
        if v["assessment_date"].as_str().unwrap_or_default().trim().is_empty()
           || clinician_id.trim().is_empty() { bail!("Kontrolli kërkon datë dhe mjekun vlerësues."); }
        conn.execute("INSERT INTO orthopedic_fracture_followups
            (id,fracture_id,episode_id,assessment_date,clinical_findings,pain_score,functional_status,alignment,healing_status,imaging_ref,complications,treatment_changes,next_step,clinician_id,created_at)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
            rusqlite::params![
                v["id"].as_str().unwrap_or_default(),fracture,episode,
                v["assessment_date"].as_str().unwrap_or_default(),
                v["clinical_findings"].as_str().unwrap_or_default(),
                v["pain_score"].as_i64(),
                v["functional_status"].as_str().unwrap_or_default(),
                v["alignment"].as_str().unwrap_or_default(),
                v["healing_status"].as_str().unwrap_or("unknown"),
                v["imaging_ref"].as_str().unwrap_or_default(),
                v["complications"].as_str().unwrap_or_default(),
                v["treatment_changes"].as_str().unwrap_or_default(),
                v["next_step"].as_str().unwrap_or_default(),
                clinician_id,now_iso()
            ])?;
        Ok(())
    }

    pub fn orthopedic_operations_list(&self, episode_id: &str) -> anyhow::Result<Vec<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id,plan_id,episode_id,actual_procedure,operative_site,laterality,start_at,end_at,surgical_team,findings,technique,complications,operative_note,postop_instructions,surgeon_id,finalized_at FROM orthopedic_operations WHERE episode_id=?1 ORDER BY finalized_at DESC")?;
        let rows = stmt.query_map(rusqlite::params![episode_id], |r| Ok(serde_json::json!({
            "id":r.get::<_,String>(0)?, "plan_id":r.get::<_,String>(1)?,
            "episode_id":r.get::<_,String>(2)?, "actual_procedure":r.get::<_,String>(3)?,
            "operative_site":r.get::<_,String>(4)?, "laterality":r.get::<_,String>(5)?,
            "start_at":r.get::<_,String>(6)?, "end_at":r.get::<_,String>(7)?,
            "surgical_team":r.get::<_,String>(8)?, "findings":r.get::<_,String>(9)?,
            "technique":r.get::<_,String>(10)?, "complications":r.get::<_,String>(11)?,
            "operative_note":r.get::<_,String>(12)?, "postop_instructions":r.get::<_,String>(13)?,
            "surgeon_id":r.get::<_,String>(14)?, "finalized_at":r.get::<_,String>(15)?
        })))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn orthopedic_operation_finalize(&self, v: &serde_json::Value, surgeon_id: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let episode = v["episode_id"].as_str().unwrap_or_default();
        Self::orthopedic_require_episode(&conn, episode)?;
        let plan_id = v["plan_id"].as_str().unwrap_or_default();
        let owner: Option<(String,String,String)> = conn.query_row(
            "SELECT episode_id,side,consent_status FROM orthopedic_surgeries WHERE id=?1",
            [plan_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        let (plan_episode, plan_side, consent) = owner.ok_or_else(|| anyhow!("Plani kirurgjik nuk ekziston."))?;
        if plan_episode != episode || plan_side != v["laterality"].as_str().unwrap_or_default() {
            bail!("Pacienti/episodi ose ana operative nuk përputhet me planin.");
        }
        if consent != "informed_signed" { bail!("Nuk ekziston pëlqimi i informuar i dokumentuar."); }
        for field in ["consent_verified","patient_verified","site_verified","laterality_verified"] {
            if v[field].as_bool() != Some(true) { bail!("Verifikimet kirurgjike janë të paplota: {}",field); }
        }
        for field in ["id","actual_procedure","operative_site","start_at","end_at","surgical_team","operative_note"] {
            if v[field].as_str().unwrap_or_default().trim().is_empty() {
                bail!("Raporti operativ kërkon: {}",field);
            }
        }
        let start = v["start_at"].as_str().unwrap_or_default();
        let end = v["end_at"].as_str().unwrap_or_default();
        if end <= start { bail!("Koha e përfundimit duhet të jetë pas fillimit."); }
        conn.execute("INSERT INTO orthopedic_operations(
            id,plan_id,episode_id,actual_procedure,operative_site,laterality,start_at,end_at,surgical_team,findings,technique,complications,operative_note,postop_instructions,consent_verified,patient_verified,site_verified,laterality_verified,surgeon_id,finalized_at)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,1,1,1,1,?15,?16)",
            rusqlite::params![
                v["id"].as_str().unwrap_or_default(),plan_id,episode,
                v["actual_procedure"].as_str().unwrap_or_default(),
                v["operative_site"].as_str().unwrap_or_default(),
                v["laterality"].as_str().unwrap_or_default(),start,end,
                v["surgical_team"].as_str().unwrap_or_default(),
                v["findings"].as_str().unwrap_or_default(),
                v["technique"].as_str().unwrap_or_default(),
                v["complications"].as_str().unwrap_or_default(),
                v["operative_note"].as_str().unwrap_or_default(),
                v["postop_instructions"].as_str().unwrap_or_default(),
                surgeon_id,now_iso()
            ])?;
        Ok(())
    }

    pub fn orthopedic_surgeries_list(&self, episode_id: &str) -> anyhow::Result<Vec<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT payload_json FROM orthopedic_surgeries WHERE episode_id=?1 ORDER BY created_at DESC")?;
        let items = stmt.query_map(rusqlite::params![episode_id], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;
        items.into_iter()
            .map(|raw| serde_json::from_str::<serde_json::Value>(&raw).map_err(anyhow::Error::from))
            .collect()
    }

    pub fn orthopedic_implants_list(&self, episode_id: &str) -> anyhow::Result<Vec<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT i.payload_json FROM orthopedic_implants i JOIN orthopedic_surgeries s ON s.id=i.surgery_id WHERE s.episode_id=?1 ORDER BY i.created_at DESC")?;
        let items = stmt.query_map(rusqlite::params![episode_id], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;
        items.into_iter()
            .map(|raw| serde_json::from_str::<serde_json::Value>(&raw).map_err(anyhow::Error::from))
            .collect()
    }

    pub fn orthopedic_rehab_plans_list(&self, episode_id: &str) -> anyhow::Result<Vec<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT payload_json FROM orthopedic_rehab_plans WHERE episode_id=?1 ORDER BY created_at DESC")?;
        let items = stmt.query_map(rusqlite::params![episode_id], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;
        items.into_iter()
            .map(|raw| serde_json::from_str::<serde_json::Value>(&raw).map_err(anyhow::Error::from))
            .collect()
    }

    pub fn orthopedic_markers_list(&self, episode_id: &str) -> anyhow::Result<Vec<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, episode_id, view, region, side, injury_type, note, x, y, created_at FROM orthopedic_markers WHERE episode_id=?1 ORDER BY created_at DESC")?;
        let rows = stmt.query_map(rusqlite::params![episode_id], |r| Ok(serde_json::json!({"id":r.get::<_,String>(0)?,"episode_id":r.get::<_,String>(1)?,"view":r.get::<_,String>(2)?,"region":r.get::<_,String>(3)?,"side":r.get::<_,String>(4)?,"injury_type":r.get::<_,String>(5)?,"note":r.get::<_,String>(6)?,"x":r.get::<_,f64>(7)?,"y":r.get::<_,f64>(8)?,"created_at":r.get::<_,String>(9)?})))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
    pub fn orthopedic_marker_upsert(&self, v: &serde_json::Value) -> anyhow::Result<()> {
        let c = self.conn.lock().unwrap();
        Self::orthopedic_require_episode(&c, v["episode_id"].as_str().unwrap_or_default())?;
        let x = v["x"].as_f64().ok_or_else(|| anyhow!("Mungon koordinata X."))?;
        let y = v["y"].as_f64().ok_or_else(|| anyhow!("Mungon koordinata Y."))?;
        if !x.is_finite() || !y.is_finite() || !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) {
            bail!("Markerët skeletorë kërkojnë koordinata të normalizuara 0..1.");
        }
        c.execute("INSERT INTO orthopedic_markers(id,episode_id,view,region,side,injury_type,note,x,y,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT(id) DO UPDATE SET view=excluded.view,region=excluded.region,side=excluded.side,injury_type=excluded.injury_type,note=excluded.note,x=excluded.x,y=excluded.y", rusqlite::params![v["id"].as_str().unwrap_or_default(),v["episode_id"].as_str().unwrap_or_default(),v["view"].as_str().unwrap_or("anterior"),v["region"].as_str().unwrap_or_default(),v["side"].as_str().unwrap_or_default(),v["injury_type"].as_str().unwrap_or_default(),v["note"].as_str().unwrap_or_default(),v["x"].as_f64().unwrap_or(0.0),v["y"].as_f64().unwrap_or(0.0),v["created_at"].as_str().unwrap_or_default()])?; Ok(())
    }

    pub fn orthopedic_assessments_list(&self, episode_id: &str) -> anyhow::Result<Vec<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, episode_id, category, joint, movement, method, degrees, pain, test_name, test_result, note, created_at FROM orthopedic_assessments WHERE episode_id=?1 ORDER BY created_at DESC")?;
        let rows = stmt.query_map(rusqlite::params![episode_id], |r| Ok(serde_json::json!({"id":r.get::<_,String>(0)?,"episode_id":r.get::<_,String>(1)?,"category":r.get::<_,String>(2)?,"joint":r.get::<_,String>(3)?,"movement":r.get::<_,String>(4)?,"method":r.get::<_,String>(5)?,"degrees":r.get::<_,String>(6)?,"pain":r.get::<_,String>(7)?,"test_name":r.get::<_,String>(8)?,"test_result":r.get::<_,String>(9)?,"note":r.get::<_,String>(10)?,"created_at":r.get::<_,String>(11)?})))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
    pub fn orthopedic_assessment_upsert(&self, v: &serde_json::Value) -> anyhow::Result<()> {
        let c = self.conn.lock().unwrap();
        Self::orthopedic_require_episode(&c, v["episode_id"].as_str().unwrap_or_default())?;
        let category = v["category"].as_str().unwrap_or("rom");
        if !matches!(category, "rom" | "special_test" | "deformity") { bail!("Kategori vlerësimi e pavlefshme."); }
        if category == "special_test" && !matches!(v["test_result"].as_str().unwrap_or_default(),
            "positive" | "negative" | "indeterminate" | "not_performed") {
            bail!("Testi special kërkon status të vlefshëm, përfshirë not_performed.");
        }
        c.execute("INSERT INTO orthopedic_assessments(id,episode_id,payload_json,category,joint,movement,method,degrees,pain,test_name,test_result,note,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13) ON CONFLICT(id) DO UPDATE SET payload_json=excluded.payload_json,category=excluded.category,joint=excluded.joint,movement=excluded.movement,method=excluded.method,degrees=excluded.degrees,pain=excluded.pain,test_name=excluded.test_name,test_result=excluded.test_result,note=excluded.note", rusqlite::params![v["id"].as_str().unwrap_or_default(),v["episode_id"].as_str().unwrap_or_default(),v.to_string(),v["category"].as_str().unwrap_or("rom"),v["joint"].as_str().unwrap_or_default(),v["movement"].as_str().unwrap_or_default(),v["method"].as_str().unwrap_or("clinical"),v["degrees"].as_str().unwrap_or_default(),v["pain"].as_str().unwrap_or_default(),v["test_name"].as_str().unwrap_or_default(),v["test_result"].as_str().unwrap_or_default(),v["note"].as_str().unwrap_or_default(),v["created_at"].as_str().unwrap_or_default()])?; Ok(())
    }

    pub fn orthopedic_fractures_list(&self, episode_id: &str) -> anyhow::Result<Vec<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, episode_id, bone_region, laterality, segment, fracture_pattern, open_closed, displacement, classification, classification_source, injury_date, treatment_strategy, current_status, created_at FROM orthopedic_fractures WHERE episode_id=?1 ORDER BY created_at DESC")?;
        let rows = stmt.query_map(rusqlite::params![episode_id], |r| Ok(serde_json::json!({"id":r.get::<_,String>(0)?,"episode_id":r.get::<_,String>(1)?,"bone_region":r.get::<_,String>(2)?,"laterality":r.get::<_,String>(3)?,"segment":r.get::<_,String>(4)?,"fracture_pattern":r.get::<_,String>(5)?,"open_closed":r.get::<_,String>(6)?,"displacement":r.get::<_,String>(7)?,"classification":r.get::<_,String>(8)?,"classification_source":r.get::<_,String>(9)?,"injury_date":r.get::<_,String>(10)?,"treatment_strategy":r.get::<_,String>(11)?,"current_status":r.get::<_,String>(12)?,"created_at":r.get::<_,String>(13)?})))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
    pub fn orthopedic_fracture_upsert(&self, v: &serde_json::Value) -> anyhow::Result<()> {
        let c = self.conn.lock().unwrap();
        Self::orthopedic_require_episode(&c, v["episode_id"].as_str().unwrap_or_default())?;
        if !matches!(v["current_status"].as_str().unwrap_or("unknown"),
            "unknown" | "under_treatment" | "healing" | "healed" | "delayed_union" | "nonunion" | "malunion") {
            bail!("Statusi i frakturës është i pavlefshëm.");
        }
        c.execute("INSERT INTO orthopedic_fractures(id,episode_id,payload_json,bone_region,laterality,segment,fracture_pattern,open_closed,displacement,classification,classification_source,injury_date,treatment_strategy,current_status,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15) ON CONFLICT(id) DO UPDATE SET payload_json=excluded.payload_json,bone_region=excluded.bone_region,laterality=excluded.laterality,segment=excluded.segment,fracture_pattern=excluded.fracture_pattern,open_closed=excluded.open_closed,displacement=excluded.displacement,classification=excluded.classification,classification_source=excluded.classification_source,injury_date=excluded.injury_date,treatment_strategy=excluded.treatment_strategy,current_status=excluded.current_status", rusqlite::params![v["id"].as_str().unwrap_or_default(),v["episode_id"].as_str().unwrap_or_default(),v.to_string(),v["bone_region"].as_str().unwrap_or_default(),v["laterality"].as_str().unwrap_or_default(),v["segment"].as_str().unwrap_or_default(),v["fracture_pattern"].as_str().unwrap_or_default(),v["open_closed"].as_str().unwrap_or("closed"),v["displacement"].as_str().unwrap_or_default(),v["classification"].as_str().unwrap_or_default(),v["classification_source"].as_str().unwrap_or_default(),v["injury_date"].as_str().unwrap_or_default(),v["treatment_strategy"].as_str().unwrap_or_default(),v["current_status"].as_str().unwrap_or("unknown"),v["created_at"].as_str().unwrap_or_default()])?; Ok(())
    }

    pub fn orthopedic_imaging_links_list(&self, episode_id: &str) -> anyhow::Result<Vec<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, episode_id, fracture_id, study_ref, modality, taken_at, reviewer_note, created_at FROM orthopedic_imaging_links WHERE episode_id=?1 ORDER BY created_at DESC")?;
        let rows = stmt.query_map(rusqlite::params![episode_id], |r| Ok(serde_json::json!({"id":r.get::<_,String>(0)?,"episode_id":r.get::<_,String>(1)?,"fracture_id":r.get::<_,String>(2)?,"study_ref":r.get::<_,String>(3)?,"modality":r.get::<_,String>(4)?,"taken_at":r.get::<_,String>(5)?,"reviewer_note":r.get::<_,String>(6)?,"created_at":r.get::<_,String>(7)?})))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
    pub fn orthopedic_imaging_link_upsert(&self, v: &serde_json::Value) -> anyhow::Result<()> {
        let c = self.conn.lock().unwrap();
        Self::orthopedic_require_episode(&c, v["episode_id"].as_str().unwrap_or_default())?;
        c.execute("INSERT INTO orthopedic_imaging_links(id,episode_id,fracture_id,study_ref,modality,taken_at,reviewer_note,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(id) DO UPDATE SET fracture_id=excluded.fracture_id,study_ref=excluded.study_ref,modality=excluded.modality,taken_at=excluded.taken_at,reviewer_note=excluded.reviewer_note", rusqlite::params![v["id"].as_str().unwrap_or_default(),v["episode_id"].as_str().unwrap_or_default(),v["fracture_id"].as_str().unwrap_or_default(),v["study_ref"].as_str().unwrap_or_default(),v["modality"].as_str().unwrap_or_default(),v["taken_at"].as_str().unwrap_or_default(),v["reviewer_note"].as_str().unwrap_or_default(),v["created_at"].as_str().unwrap_or_default()])?; Ok(())
    }

    pub fn orthopedic_postop_visits_list(&self, episode_id: &str) -> anyhow::Result<Vec<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, episode_id, surgery_id, visit_date, findings, complications, next_step, created_at FROM orthopedic_postop_visits WHERE episode_id=?1 ORDER BY visit_date DESC")?;
        let rows = stmt.query_map(rusqlite::params![episode_id], |r| Ok(serde_json::json!({"id":r.get::<_,String>(0)?,"episode_id":r.get::<_,String>(1)?,"surgery_id":r.get::<_,String>(2)?,"visit_date":r.get::<_,String>(3)?,"findings":r.get::<_,String>(4)?,"complications":r.get::<_,String>(5)?,"next_step":r.get::<_,String>(6)?,"created_at":r.get::<_,String>(7)?})))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
    pub fn orthopedic_postop_visit_upsert(&self, v: &serde_json::Value) -> anyhow::Result<()> {
        let c = self.conn.lock().unwrap();
        Self::orthopedic_require_episode(&c, v["episode_id"].as_str().unwrap_or_default())?;
        c.execute("INSERT INTO orthopedic_postop_visits(id,episode_id,surgery_id,visit_date,findings,complications,next_step,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(id) DO UPDATE SET surgery_id=excluded.surgery_id,visit_date=excluded.visit_date,findings=excluded.findings,complications=excluded.complications,next_step=excluded.next_step", rusqlite::params![v["id"].as_str().unwrap_or_default(),v["episode_id"].as_str().unwrap_or_default(),v["surgery_id"].as_str().unwrap_or_default(),v["visit_date"].as_str().unwrap_or_default(),v["findings"].as_str().unwrap_or_default(),v["complications"].as_str().unwrap_or_default(),v["next_step"].as_str().unwrap_or_default(),v["created_at"].as_str().unwrap_or_default()])?; Ok(())
    }

    /// Gjurmueshmëria e implanteve (kriteri i pranimit §6.2): kërkim ndër-pacientësh sipas lotit, serialit ose UDI.
    pub fn orthopedic_implants_search(&self, query: &str) -> anyhow::Result<Vec<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let like = format!("%{}%", query);
        let mut stmt = conn.prepare("SELECT i.id, i.surgery_id, s.episode_id, i.udi, i.lot, i.serial, i.status, i.manufacturer, i.model, i.removal_status, i.implantation_date, i.created_at FROM orthopedic_implants i LEFT JOIN orthopedic_surgeries s ON s.id = i.surgery_id WHERE i.lot LIKE ?1 OR i.serial LIKE ?1 OR i.udi LIKE ?1 OR i.udi_di LIKE ?1 OR i.udi_pi LIKE ?1 OR i.manufacturer LIKE ?1 OR i.model LIKE ?1 ORDER BY i.created_at DESC LIMIT 200")?;
        let rows = stmt.query_map(rusqlite::params![like], |r| Ok(serde_json::json!({"id":r.get::<_,String>(0)?,"surgery_id":r.get::<_,String>(1)?,"episode_id":r.get::<_,Option<String>>(2)?,"udi":r.get::<_,String>(3)?,"lot":r.get::<_,String>(4)?,"serial":r.get::<_,String>(5)?,"status":r.get::<_,String>(6)?,"manufacturer":r.get::<_,String>(7)?,"model":r.get::<_,String>(8)?,"removal_status":r.get::<_,String>(9)?,"implantation_date":r.get::<_,String>(10)?,"created_at":r.get::<_,String>(11)?})))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
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
        // Create orthopedic base tables BEFORE adding their newer columns.
        // The previous order attempted ALTER TABLE on non-existing tables
        // when installing a clean clinic database.
        conn.execute_batch(include_str!("../migrations/orthopedics_v1.sql"))?;
        conn.execute_batch(include_str!("../migrations/orthopedics_v2.sql"))?;
        conn.execute_batch(include_str!("../migrations/orthopedics_v3.sql"))?;
        conn.execute_batch(include_str!("../migrations/orthopedics_v4.sql"))?;
        conn.execute_batch(include_str!("../migrations/physiotherapy_v1.sql"))?;
        conn.execute_batch(include_str!("../migrations/physiotherapy_v2.sql"))?;
        conn.execute_batch(include_str!("../migrations/physiotherapy_v3.sql"))?;
        conn.execute_batch(include_str!("../migrations/physiotherapy_v4.sql"))?;
        conn.execute_batch(include_str!("../migrations/physiotherapy_v5.sql"))?;
        conn.execute_batch(include_str!("../migrations/sef_mjeku_v1.sql"))?;
        conn.execute_batch(include_str!("../migrations/sef_mjeku_v2.sql"))?;
        conn.execute_batch(include_str!("../migrations/sef_mjeku_v3.sql"))?;
            conn.execute_batch(include_str!("../migrations/sef_mjeku_v4.sql"))?;
            conn.execute_batch(include_str!("../migrations/sef_mjeku_v5.sql"))?;
        Self::run_migrations(&conn)?;
        conn.execute_batch(include_str!("../migrations/dental_findings_v1.sql"))?;

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
        // Create orthopedic base tables BEFORE adding their newer columns.
        // The previous order attempted ALTER TABLE on non-existing tables
        // when installing a clean clinic database.
        conn.execute_batch(include_str!("../migrations/orthopedics_v1.sql"))?;
        conn.execute_batch(include_str!("../migrations/orthopedics_v2.sql"))?;
        conn.execute_batch(include_str!("../migrations/orthopedics_v3.sql"))?;
        conn.execute_batch(include_str!("../migrations/orthopedics_v4.sql"))?;
        conn.execute_batch(include_str!("../migrations/physiotherapy_v1.sql"))?;
        conn.execute_batch(include_str!("../migrations/physiotherapy_v2.sql"))?;
        conn.execute_batch(include_str!("../migrations/physiotherapy_v3.sql"))?;
        conn.execute_batch(include_str!("../migrations/physiotherapy_v4.sql"))?;
        conn.execute_batch(include_str!("../migrations/physiotherapy_v5.sql"))?;
        conn.execute_batch(include_str!("../migrations/sef_mjeku_v1.sql"))?;
        conn.execute_batch(include_str!("../migrations/sef_mjeku_v2.sql"))?;
        conn.execute_batch(include_str!("../migrations/sef_mjeku_v3.sql"))?;
            conn.execute_batch(include_str!("../migrations/sef_mjeku_v4.sql"))?;
            conn.execute_batch(include_str!("../migrations/sef_mjeku_v5.sql"))?;
        Self::run_migrations(&conn)?;
        conn.execute_batch(include_str!("../migrations/dental_findings_v1.sql"))?;

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
        // SEF (PROGRAMERI POS): kolona vetëm lokale (nuk sinkronizohen; struct Sale s'i ka).
        for (col, ddl) in [
            ("sef_coupon_id", "TEXT"),
            ("sef_daily_number", "INTEGER"),
            ("sef_status", "TEXT"),
            ("sef_verification_url", "TEXT"),
            ("sef_fiscalized_at", "TEXT"),
        ] {
            Self::add_column_if_missing(conn, "sales", col, ddl)?;
        }

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

        // orthopedics: structured columns (additive; payload_json stays as a free-form fallback for older rows)
        for (col, ddl) in [
            ("category", "TEXT NOT NULL DEFAULT 'rom'"),
            ("joint", "TEXT"), ("movement", "TEXT"), ("method", "TEXT"), ("degrees", "TEXT"),
            ("pain", "TEXT"), ("test_name", "TEXT"), ("test_result", "TEXT"), ("note", "TEXT"),
        ] {
            Self::add_column_if_missing(conn, "orthopedic_assessments", col, ddl)?;
        }
        for (col, ddl) in [
            ("bone_region", "TEXT"), ("laterality", "TEXT"), ("segment", "TEXT"), ("fracture_pattern", "TEXT"),
            ("open_closed", "TEXT NOT NULL DEFAULT 'closed'"), ("displacement", "TEXT"), ("classification", "TEXT"),
            ("classification_source", "TEXT"), ("injury_date", "TEXT"), ("treatment_strategy", "TEXT"),
            ("current_status", "TEXT NOT NULL DEFAULT 'unknown'"),
        ] {
            Self::add_column_if_missing(conn, "orthopedic_fractures", col, ddl)?;
        }
        for (col, ddl) in [
            ("side", "TEXT"), ("site", "TEXT"), ("team", "TEXT"), ("consent_status", "TEXT NOT NULL DEFAULT 'pending'"),
            ("planned_date", "TEXT"), ("preop_checklist", "TEXT"), ("postop_plan", "TEXT"),
        ] {
            Self::add_column_if_missing(conn, "orthopedic_surgeries", col, ddl)?;
        }
        for (col, ddl) in [
            ("manufacturer", "TEXT"), ("model", "TEXT"), ("catalog_ref", "TEXT"), ("udi_di", "TEXT"), ("udi_pi", "TEXT"),
            ("expiration_date", "TEXT"), ("sterility_status", "TEXT"), ("anatomical_site", "TEXT"),
            ("implantation_date", "TEXT"), ("removal_status", "TEXT NOT NULL DEFAULT 'in_body'"),
        ] {
            Self::add_column_if_missing(conn, "orthopedic_implants", col, ddl)?;
        }
        for (col, ddl) in [
            ("load_restrictions", "TEXT"), ("rom_restrictions", "TEXT"), ("approving_surgeon", "TEXT"),
            ("start_date", "TEXT"), ("status", "TEXT NOT NULL DEFAULT 'active'"),
        ] {
            Self::add_column_if_missing(conn, "orthopedic_rehab_plans", col, ddl)?;
        }

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

    // -----------------------------------------------------------------------
    // SEF (PROGRAMERI POS) — përdoret vetëm kur fiscal_printer_provider=sef_programeri.
    // Nuk prek rrugën ENTERNET (`fiscal_receipt_generate_inp`).
    // -----------------------------------------------------------------------

    fn sef_map_visit_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<VisitItem> {
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
    }

    /// Mbledh shitjen, rreshtat dhe pagesat për `fiscalize`, me të njëjtat rregulla
    /// zgjedhjeje si `fiscal_receipt_generate_inp`: rreshtat fiskal të pafiskalizuar;
    /// nëse s'ka dhe `allow_any_items_if_no_fiscal`, të gjithë rreshtat e pafiskalizuar.
    pub fn sef_collect_sale(
        &self,
        sale_id: &str,
        allow_any_items_if_no_fiscal: bool,
    ) -> anyhow::Result<SefSaleData> {
        let sale_id = sale_id.trim();
        if sale_id.is_empty() {
            bail!("sale_id eshte i detyrueshem");
        }
        let conn = self.conn()?;
        let sale = conn
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

        let cols = "id, visit_id, client_id, tooth, title, qty, unit_price, fiscal, vat_code, fiscalized, fiscalized_at, notes, created_at, updated_at, deleted";
        let mut items: Vec<VisitItem> = Vec::new();
        {
            let mut stmt = conn.prepare(&format!(
                "SELECT {cols} FROM visit_items
                 WHERE visit_id=?1 AND deleted=0 AND fiscal=1 AND fiscalized=0
                 ORDER BY created_at ASC"
            ))?;
            for r in stmt.query_map(params![sale_id], Self::sef_map_visit_item)? {
                items.push(r?);
            }
        }
        let mut used_non_fiscal_fallback = false;
        if items.is_empty() && allow_any_items_if_no_fiscal {
            let mut stmt = conn.prepare(&format!(
                "SELECT {cols} FROM visit_items
                 WHERE visit_id=?1 AND deleted=0 AND fiscalized=0
                 ORDER BY created_at ASC"
            ))?;
            for r in stmt.query_map(params![sale_id], Self::sef_map_visit_item)? {
                items.push(r?);
            }
            used_non_fiscal_fallback = !items.is_empty();
        }
        if items.is_empty() {
            bail!("nuk ka rreshta fiskal pa fiskalizuar");
        }

        let mut payments: Vec<(String, f64)> = Vec::new();
        {
            let mut stmt = conn.prepare(
                "SELECT COALESCE(method,'cash'), amount FROM payments
                 WHERE sale_id=?1 AND deleted=0 ORDER BY created_at ASC",
            )?;
            let rows = stmt.query_map(params![sale_id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
            })?;
            for r in rows {
                payments.push(r?);
            }
        }

        Ok(SefSaleData {
            sale,
            items,
            used_non_fiscal_fallback,
            payments,
        })
    }

    /// Pas përgjigjes së suksesshme të `fiscalize` (FISCALIZED ose PENDING_OFFLINE):
    /// shënon rreshtat dhe shitjen si të fiskalizuara (si rruga ENTERNET), ruan
    /// pagesën automatike kur shitja s'ka pagesë, dhe ruan numrin/statusin e kuponit SEF.
    pub fn sef_mark_fiscalized(
        &self,
        data: &SefSaleData,
        total: f64,
        coupon: &SefCouponInfo,
    ) -> anyhow::Result<()> {
        let now = now_iso();
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;

        let already: i64 = tx
            .query_row(
                "SELECT fiscalized FROM sales WHERE id=?1",
                params![&data.sale.id],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0);

        tx.execute(
            "UPDATE sales SET sef_coupon_id=?2, sef_daily_number=?3, sef_status=?4, sef_verification_url=?5, sef_fiscalized_at=?6 WHERE id=?1",
            params![
                &data.sale.id,
                &coupon.coupon_id,
                coupon.daily_number,
                &coupon.status,
                &coupon.verification_url,
                &now
            ],
        )?;

        if already != 0 {
            // Një thirrje paralele e ka shënuar tashmë; ruajmë vetëm të dhënat e kuponit.
            tx.commit()?;
            return Ok(());
        }

        for it in &data.items {
            let mut it = it.clone();
            let updated_fiscal_flag = if data.used_non_fiscal_fallback {
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
            params![&data.sale.id, &now],
        )?;
        let updated_sale = Sale {
            fiscalized: 1,
            fiscalized_at: Some(now.clone()),
            updated_at: now.clone(),
            ..data.sale.clone()
        };
        let payload = serde_json::to_string(&updated_sale)?;
        Self::queue_replace_pending_tx(&tx, "sales", &updated_sale.id, "upsert", &payload, &now)?;

        // Pagesa automatike si te ENTERNET: vetëm kur shitja s'ka asnjë pagesë.
        let auto_note = "[AUTO-FISCAL] Kupon fiskal SEF";
        let has_any_payment = tx
            .query_row(
                "SELECT 1 FROM payments WHERE sale_id=?1 AND deleted=0 LIMIT 1",
                params![&updated_sale.id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .is_some();
        if !has_any_payment {
            let payment_date = updated_sale
                .date
                .clone()
                .or_else(|| now.get(0..10).map(|x| x.to_string()));
            let payment = Payment {
                id: updated_sale.id.clone(),
                client_id: updated_sale.client_id.clone(),
                sale_id: Some(updated_sale.id.clone()),
                date: payment_date,
                amount: total,
                method: "cash".to_string(),
                notes: Some(auto_note.to_string()),
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

    /// Të dhënat lokale SEF të një shitjeje (vetëm lexim, për UI).
    pub fn sef_sale_coupon(&self, sale_id: &str) -> anyhow::Result<Option<SefCouponInfo>> {
        let conn = self.conn()?;
        let row = conn
            .query_row(
                "SELECT sef_coupon_id, sef_daily_number, sef_status, sef_verification_url
                 FROM sales WHERE id=?1",
                params![sale_id.trim()],
                |r| {
                    Ok((
                        r.get::<_, Option<String>>(0)?,
                        r.get::<_, Option<i64>>(1)?,
                        r.get::<_, Option<String>>(2)?,
                        r.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?;
        Ok(row.and_then(|(id, n, s, u)| {
            id.filter(|x| !x.trim().is_empty())
                .map(|coupon_id| SefCouponInfo {
                    coupon_id,
                    daily_number: n,
                    status: s.unwrap_or_default(),
                    verification_url: u,
                })
        }))
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
            // VAT registered: preserve both zero-rate categories A and C.
            if vat_code.is_empty() {
                vat_code = "A".to_string();
            }
            match vat_code.as_str() {
                "A" | "C" | "D" | "E" => {}
                _ => bail!("vat_code duhet te jete A, C, D ose E (kur biznesi eshte ne TVSH)"),
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
            match vat_code.as_str() {
                "A" | "C" | "D" | "E" => {}
                _ => bail!("vat_code duhet te jete A, C, D ose E (kur biznesi eshte ne TVSH)"),
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

impl Db {
    pub fn dental_patient_access(&self,patient_id:&str,doctor_id:&str)->anyhow::Result<bool>{
        let conn=self.conn()?;
        let count:i64=conn.query_row("SELECT COUNT(*) FROM visits WHERE client_id=?1 AND doctor_id=?2 AND deleted=0",params![patient_id,doctor_id],|r|r.get(0))?;
        Ok(count>0)
    }
    pub fn dental_findings_list(&self, patient_id: &str) -> anyhow::Result<Vec<serde_json::Value>> {
        if patient_id.trim().is_empty() { bail!("Patient required"); }
        let conn=self.conn()?;
        let mut stmt=conn.prepare("SELECT id,patient_id,tooth_fdi,surface,kind,status,visit_id,clinician_id,recorded_at,notes FROM dental_findings_v1 WHERE patient_id=?1 ORDER BY recorded_at DESC,id DESC")?;
        let rows=stmt.query_map(params![patient_id],|r|Ok(serde_json::json!({
            "id":r.get::<_,String>(0)?,"patientId":r.get::<_,String>(1)?,
            "tooth":r.get::<_,String>(2)?,"surface":r.get::<_,Option<String>>(3)?,
            "kind":r.get::<_,String>(4)?,"status":r.get::<_,String>(5)?,
            "visitId":r.get::<_,String>(6)?,"clinicianId":r.get::<_,String>(7)?,
            "recordedAt":r.get::<_,String>(8)?,"notes":r.get::<_,Option<String>>(9)?
        })))?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    pub fn dental_finding_create(&self, value:serde_json::Value, actor_id:&str)->anyhow::Result<serde_json::Value>{
        let required=|key:&str|->anyhow::Result<String>{
            let t=value.get(key).and_then(|v|v.as_str()).unwrap_or("").trim();
            if t.is_empty(){bail!("Missing {}",key);}Ok(t.to_owned())
        };
        let patient=required("patientId")?;
        let visit=required("visitId")?;
        let tooth=required("tooth")?;
        let kind=required("kind")?;
        let status=required("status")?;
        let surface=value.get("surface").and_then(|v|v.as_str());
        let notes=value.get("notes").and_then(|v|v.as_str());
        if !["existing","planned","completed"].contains(&status.as_str()){bail!("Invalid status");}
        if !["healthy","caries","missing","extracted","impacted","restoration","crown","bridge","implant","root_canal","fracture","temporary_restoration","prosthetic","other"].contains(&kind.as_str()){bail!("Invalid finding kind");}
        let chars:Vec<char>=tooth.chars().collect();
        if chars.len()!=2 || !chars.iter().all(|c|c.is_ascii_digit()){bail!("Invalid FDI");}
        let q=chars[0].to_digit(10).unwrap();
        let n=chars[1].to_digit(10).unwrap();
        if !((1..=4).contains(&q)&&(1..=8).contains(&n)||(5..=8).contains(&q)&&(1..=5).contains(&n)){bail!("Invalid FDI tooth");}
        if let Some(sf)=surface {
            if !["M","D","B","L","O","I"].contains(&sf){bail!("Invalid surface");}
            if (1..=3).contains(&n)&&sf=="O" || n>=4&&sf=="I"{bail!("Surface does not match tooth anatomy");}
        }
        if ["missing","extracted","impacted"].contains(&kind.as_str()) && surface.is_some() {
            bail!("Whole-tooth finding cannot have a surface");
        }
        if ["healthy","missing","extracted"].contains(&kind.as_str()) && status=="planned" {
            bail!("This finding cannot be a planned procedure");
        }
        if let Some(n)=notes { if n.len()>10_000 { bail!("Notes too long"); } }
        let id=Uuid::new_v4().to_string();
        let now=now_iso();
        let mut conn=self.conn()?;
        let tx=conn.transaction()?;
        let exists:i64=tx.query_row("SELECT COUNT(*) FROM visits WHERE id=?1 AND client_id=?2 AND doctor_id=?3 AND deleted=0",params![visit,patient,actor_id],|r|r.get(0))?;
        if exists!=1{bail!("Visit must belong to patient and acting clinician");}
        tx.execute("INSERT INTO dental_findings_v1(id,patient_id,visit_id,clinician_id,tooth_fdi,surface,kind,status,notes,recorded_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![id,patient,visit,actor_id,tooth,surface,kind,status,notes,now])?;
        tx.execute("INSERT INTO dental_finding_events_v1(id,finding_id,actor_id,event_type,occurred_at,payload_json) VALUES (?1,?2,?3,'created',?4,?5)",
            params![Uuid::new_v4().to_string(),id,actor_id,now,value.to_string()])?;
        tx.commit()?;
        let mut out=value;
        out["id"]=serde_json::json!(id);
        out["clinicianId"]=serde_json::json!(actor_id);
        out["recordedAt"]=serde_json::json!(now);
        Ok(out)
    }
}

#[cfg(test)]
mod dental_findings_tests {
    use super::*;

    fn input(tooth:&str,surface:Option<&str>)->serde_json::Value {
        serde_json::json!({
            "patientId":"patient-test","visitId":"visit-test","tooth":tooth,
            "surface":surface,"kind":"caries","status":"existing","notes":"test"
        })
    }

    #[test]
    fn dental_rejects_invalid_fdi_and_surfaces() {
        let db=Db::new_in_memory().expect("in-memory DB migration");
        assert!(db.dental_finding_create(input("99",None),"doctor-test").is_err());
        assert!(db.dental_finding_create(input("16",Some("I")),"doctor-test").is_err());
        assert!(db.dental_finding_create(input("11",Some("O")),"doctor-test").is_err());
        assert!(db.dental_finding_create(input("56",Some("O")),"doctor-test").is_err());
    }

    #[test]
    fn dental_rejects_unassigned_visit() {
        let db=Db::new_in_memory().expect("in-memory DB migration");
        assert!(db.dental_finding_create(input("16",Some("O")),"doctor-test").is_err());
        assert!(db.dental_findings_list("patient-test").unwrap().is_empty());
    }

    #[test]
    fn dental_rejects_invalid_kind_and_status() {
        let db=Db::new_in_memory().expect("in-memory DB migration");
        let mut finding=input("16",Some("O"));
        finding["kind"]=serde_json::json!("unsupported");
        assert!(db.dental_finding_create(finding,"doctor-test").is_err());
        let mut planned=input("16",Some("O"));
        planned["kind"]=serde_json::json!("missing");
        planned["surface"]=serde_json::Value::Null;
        planned["status"]=serde_json::json!("planned");
        assert!(db.dental_finding_create(planned,"doctor-test").is_err());
    }

    #[test]
    fn dental_correction_requires_original_record() {
        let db=Db::new_in_memory().expect("in-memory DB migration");
        let correction=serde_json::json!({
            "findingId":"nonexistent","reason":"Documentation error",
            "kind":"caries","status":"existing","surface":"O","notes":null
        });
        assert!(db.dental_finding_correct(correction,"doctor-test").is_err());
    }

    #[test]
    fn periodontal_rejects_missing_clinician_and_invalid_flags() {
        let db=Db::new_in_memory().expect("in-memory migration");
        let mut input=serde_json::json!({
            "patientId":"p","visitId":"v","dentition":"permanent",
            "measurements":[{"tooth":"16","site":"MB","probingDepthMm":3,
                "gingivalMarginMm":null,"bleeding":true,"plaque":null,"suppuration":null}]
        });
        assert!(db.dental_periodontal_exam_create(input.clone()," ").is_err());
        input["measurements"][0]["bleeding"]=serde_json::json!("yes");
        assert!(db.dental_periodontal_exam_create(input,"doctor").is_err());
    }

    #[test]
    fn periodontal_exam_round_trip_preserves_nullable_sites() {
        let db=Db::new_in_memory().expect("in-memory migration");
        {
            let conn=db.conn().expect("connection");
            conn.execute("INSERT INTO clients(id,name,deleted) VALUES ('perio-patient','Patient',0)",[])
                .expect("patient fixture");
            conn.execute("INSERT INTO visits(id,client_id,doctor_id,status,deleted) VALUES ('perio-visit','perio-patient','perio-doctor','open',0)",[])
                .expect("visit fixture");
        }
        let input=serde_json::json!({
            "patientId":"perio-patient","visitId":"perio-visit","dentition":"permanent",
            "notes":"Baseline","measurements":[
                {"tooth":"16","site":"MB","probingDepthMm":4.0,"gingivalMarginMm":1.0,
                 "bleeding":true,"plaque":false,"suppuration":null},
                {"tooth":"16","site":"B","probingDepthMm":null,"gingivalMarginMm":-1.0,
                 "bleeding":null,"plaque":true,"suppuration":false}
            ]
        });
        let saved=db.dental_periodontal_exam_create(input,"perio-doctor").expect("create exam");
        let history=db.dental_periodontal_exams_list("perio-patient").expect("history");
        assert_eq!(history.len(),1);
        assert_eq!(history[0]["id"],saved["id"]);
        assert_eq!(history[0]["measurements"].as_array().unwrap().len(),2);
        // SQLite sorts by tooth/site, not by the original input array index:
        // B precedes MB. Compare by tooth+site to verify the persisted observations.
        let measures = history[0]["measurements"].as_array().expect("measurements");
        let mb = measures.iter().find(|m|
            m["tooth"].as_str() == Some("16") && m["site"].as_str() == Some("MB")
        ).expect("mesio-buccal site");
        let b = measures.iter().find(|m|
            m["tooth"].as_str() == Some("16") && m["site"].as_str() == Some("B")
        ).expect("buccal site");
        assert_eq!(mb["bleeding"],serde_json::json!(true));
        assert_eq!(mb["plaque"],serde_json::json!(false));
        assert!(mb["suppuration"].is_null());
        assert!(b["probingDepthMm"].is_null());
        assert_eq!(b["gingivalMarginMm"],serde_json::json!(-1.0));
        assert!(b["bleeding"].is_null());
        assert_eq!(b["plaque"],serde_json::json!(true));
        assert_eq!(b["suppuration"],serde_json::json!(false));
    }

    #[test]
    fn periodontal_schema_is_present_in_memory() {
        let db=Db::new_in_memory().expect("migrations should initialize");
        let conn=db.conn().expect("sqlite connection");
        for table in ["dental_periodontal_exams_v1","dental_periodontal_sites_v1"] {
            let count:i64=conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                params![table],|row|row.get(0)
            ).expect("schema lookup");
            assert_eq!(count,1,"missing {}",table);
        }
    }

    #[test]
    fn periodontal_rejects_duplicate_sites_and_invalid_fdi() {
        let db=Db::new_in_memory().expect("in-memory migration");
        let site=serde_json::json!({
            "tooth":"16","site":"MB","probingDepthMm":4.0,
            "gingivalMarginMm":1.0,"bleeding":true,"plaque":false,"suppuration":null
        });
        let mut exam=serde_json::json!({
            "patientId":"patient-test","visitId":"visit-test","dentition":"permanent",
            "notes":"","measurements":[site.clone(),site]
        });
        assert!(db.dental_periodontal_exam_create(exam.clone(),"doctor-test").is_err());
        exam["measurements"][1]["tooth"]=serde_json::json!("99");
        assert!(db.dental_periodontal_exam_create(exam,"doctor-test").is_err());
    }

    #[test]
    fn periodontal_rejects_unassigned_visit_and_out_of_range_depth() {
        let db=Db::new_in_memory().expect("in-memory migration");
        let mut exam=serde_json::json!({
            "patientId":"patient-test","visitId":"visit-test","dentition":"permanent",
            "notes":"","measurements":[{"tooth":"16","site":"MB","probingDepthMm":4.0,
                "gingivalMarginMm":1.0,"bleeding":true,"plaque":null,"suppuration":null}]
        });
        assert!(db.dental_periodontal_exam_create(exam.clone(),"doctor-test").is_err());
        exam["measurements"][0]["probingDepthMm"]=serde_json::json!(31.0);
        assert!(db.dental_periodontal_exam_create(exam,"doctor-test").is_err());
    }

    #[test]
    fn dental_schema_migration_is_idempotent() {
        let db=Db::new_in_memory().expect("in-memory DB migration");
        let conn=db.conn().unwrap();
        conn.execute_batch(include_str!("../migrations/dental_findings_v1.sql")).unwrap();
        conn.execute_batch(include_str!("../migrations/dental_findings_v1.sql")).unwrap();
    }

    #[test]
    fn orthopedics_schema_migration_is_idempotent() {
        let db = Db::new_in_memory().expect("in-memory DB migration");
        let conn = db.conn.lock().unwrap();
        conn.execute_batch(include_str!("../migrations/orthopedics_v1.sql")).unwrap();
        conn.execute_batch(include_str!("../migrations/orthopedics_v1.sql")).unwrap();
        conn.execute_batch(include_str!("../migrations/orthopedics_v2.sql")).unwrap();
        conn.execute_batch(include_str!("../migrations/orthopedics_v2.sql")).unwrap();
        conn.execute_batch(include_str!("../migrations/orthopedics_v3.sql")).unwrap();
        conn.execute_batch(include_str!("../migrations/orthopedics_v3.sql")).unwrap();
        conn.execute_batch(include_str!("../migrations/orthopedics_v4.sql")).unwrap();
        conn.execute_batch(include_str!("../migrations/orthopedics_v4.sql")).unwrap();
        let count: i64 = conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name LIKE 'orthopedic_%'",
            [],
            |row| row.get(0),
        ).unwrap();
        // v4 adds orthopedic_episode_transitions to the previous 15 tables.
        assert_eq!(count, 16);
        for name in [
            "orthopedic_episode_transitions",
            "ortho_episode_transition_no_update",
            "ortho_markers_closed_insert",
            "ortho_implants_closed_update",
        ] {
            let n: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name=?1", [name],
                |row| row.get(0),
            ).unwrap();
            assert_eq!(n, 1, "missing migration artifact: {}", name);
        }
    }

    #[test]
    fn orthopedics_rejects_orphan_episode_and_marker() {
        let db = Db::new_in_memory().expect("orthopedics migration");
        let episode = serde_json::json!({
            "id":"orphan-episode","client_id":"not-a-client","diagnosis":"Frakturë",
            "region":"Femur","laterality":"left","created_at":now_iso(),"updated_at":now_iso()
        });
        assert!(db.orthopedic_episode_upsert(&episode).is_err());
        let marker = serde_json::json!({
            "id":"orphan-marker","episode_id":"not-an-episode","region":"Femur",
            "view":"anterior","side":"left","injury_type":"fracture",
            "x":0.25,"y":0.5,"created_at":now_iso()
        });
        assert!(db.orthopedic_marker_upsert(&marker).is_err());
    }

    #[test]
    fn orthopedics_rejects_unverified_operation_and_orphan_fracture_followup() {
        let db = Db::new_in_memory().expect("orthopedics migration");
        let operation = serde_json::json!({
            "id":"test-operation","episode_id":"missing-episode","plan_id":"missing",
            "patient_verified":true,"site_verified":true,"laterality_verified":true,
            "consent_verified":true
        });
        assert!(db.orthopedic_operation_finalize(&operation,"doctor-1").is_err());
        let followup = serde_json::json!({
            "id":"test-followup","episode_id":"missing-episode",
            "fracture_id":"missing-fracture", "assessment_date":"2026-10-25"
        });
        assert!(db.orthopedic_fracture_followup_add(&followup,"doctor-1").is_err());
    }
}

impl Db {
    pub fn dental_finding_correct(&self,value:serde_json::Value,actor_id:&str)->anyhow::Result<serde_json::Value>{
        let get=|k:&str|->anyhow::Result<String>{
            let v=value.get(k).and_then(|x|x.as_str()).unwrap_or("").trim();
            if v.is_empty(){bail!("Missing {}",k);}Ok(v.to_owned())
        };
        let finding_id=get("findingId")?;
        let reason=get("reason")?;
        let kind=get("kind")?;
        let status=get("status")?;
        let surface=value.get("surface").and_then(|x|x.as_str());
        let notes=value.get("notes").and_then(|x|x.as_str());
        if reason.len()>2000{bail!("Reason too long");}
        if notes.map_or(false,|n|n.len()>10000){bail!("Notes too long");}
        if !["existing","planned","completed"].contains(&status.as_str()){bail!("Invalid status");}
        if !["healthy","caries","missing","extracted","impacted","restoration","crown","bridge","implant","root_canal","fracture","temporary_restoration","prosthetic","other"].contains(&kind.as_str()){bail!("Invalid kind");}
        if let Some(sf)=surface{if !["M","D","B","L","O","I"].contains(&sf){bail!("Invalid surface");}}
        if ["missing","extracted","impacted"].contains(&kind.as_str()) && surface.is_some(){bail!("Whole-tooth finding cannot have a surface");}
        if ["healthy","missing","extracted"].contains(&kind.as_str()) && status=="planned"{bail!("This finding cannot be planned");}
        let mut conn=self.conn()?;let tx=conn.transaction()?;
        let (tooth,owner):(String,String)=tx.query_row(
            "SELECT tooth_fdi,clinician_id FROM dental_findings_v1 WHERE id=?1",
            params![finding_id],|r|Ok((r.get(0)?,r.get(1)?)))?;
        if owner!=actor_id{bail!("Only the recording clinician can correct this finding");}
        let n=tooth.chars().nth(1).unwrap_or('0').to_digit(10).unwrap_or(0);
        if let Some(sf)=surface{if (n<=3&&sf=="O")||(n>=4&&sf=="I"){bail!("Invalid tooth surface");}}
        let id=Uuid::new_v4().to_string();let now=now_iso();
        tx.execute("INSERT INTO dental_finding_corrections_v1(id,finding_id,actor_id,reason,corrected_kind,corrected_status,corrected_surface,corrected_notes,corrected_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![id,finding_id,actor_id,reason,kind,status,surface,notes,now])?;
        tx.execute("INSERT INTO dental_finding_events_v1(id,finding_id,actor_id,event_type,occurred_at,payload_json) VALUES (?1,?2,?3,'corrected',?4,?5)",
            params![Uuid::new_v4().to_string(),finding_id,actor_id,now,value.to_string()])?;
        tx.commit()?;
        let mut out=value;out["id"]=serde_json::json!(id);out["correctedAt"]=serde_json::json!(now);Ok(out)
    }
    pub fn dental_finding_corrections_list(&self,patient_id:&str)->anyhow::Result<Vec<serde_json::Value>>{
        let conn=self.conn()?;
        let mut stmt=conn.prepare("SELECT c.id,c.finding_id,c.actor_id,c.reason,c.corrected_kind,c.corrected_status,c.corrected_surface,c.corrected_notes,c.corrected_at FROM dental_finding_corrections_v1 c JOIN dental_findings_v1 f ON f.id=c.finding_id WHERE f.patient_id=?1 ORDER BY c.corrected_at,c.id")?;
        let rows=stmt.query_map(params![patient_id],|r|Ok(serde_json::json!({
            "id":r.get::<_,String>(0)?,"findingId":r.get::<_,String>(1)?,"actorId":r.get::<_,String>(2)?,
            "reason":r.get::<_,String>(3)?,"kind":r.get::<_,String>(4)?,"status":r.get::<_,String>(5)?,
            "surface":r.get::<_,Option<String>>(6)?,"notes":r.get::<_,Option<String>>(7)?,"correctedAt":r.get::<_,String>(8)?
        })))?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }
}

impl Db {
    pub fn dental_periodontal_exams_list(&self,patient_id:&str)->anyhow::Result<Vec<serde_json::Value>> {
        if patient_id.trim().is_empty(){bail!("Patient required");}
        let conn=self.conn()?;
        let mut stmt=conn.prepare("SELECT id,patient_id,visit_id,clinician_id,dentition,recorded_at,notes FROM dental_periodontal_exams_v1 WHERE patient_id=?1 ORDER BY recorded_at DESC,id DESC")?;
        let exams=stmt.query_map(params![patient_id],|r|Ok((
            r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,
            r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,r.get::<_,String>(6)?
        )))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let mut output=Vec::new();
        for (id,patient,visit,clinician,dentition,recorded,notes) in exams {
            let mut sites=conn.prepare("SELECT tooth_fdi,site,probing_depth_mm,gingival_margin_mm,bleeding,plaque,suppuration FROM dental_periodontal_sites_v1 WHERE exam_id=?1 ORDER BY tooth_fdi,site")?;
            let measures=sites.query_map(params![id],|r|Ok(serde_json::json!({
                "tooth":r.get::<_,String>(0)?,"site":r.get::<_,String>(1)?,
                "probingDepthMm":r.get::<_,Option<f64>>(2)?,"gingivalMarginMm":r.get::<_,Option<f64>>(3)?,
                "bleeding":r.get::<_,Option<i64>>(4)?.map(|x|x==1),
                "plaque":r.get::<_,Option<i64>>(5)?.map(|x|x==1),
                "suppuration":r.get::<_,Option<i64>>(6)?.map(|x|x==1)
            })))?.collect::<rusqlite::Result<Vec<_>>>()?;
            output.push(serde_json::json!({"id":id,"patientId":patient,"visitId":visit,
                "clinicianId":clinician,"dentition":dentition,"recordedAt":recorded,"notes":notes,"measurements":measures}));
        }
        Ok(output)
    }

    pub fn dental_periodontal_exam_create(&self,value:serde_json::Value,actor:&str)->anyhow::Result<serde_json::Value>{
        let get=|k:&str|->anyhow::Result<String>{
            let s=value.get(k).and_then(|v|v.as_str()).unwrap_or("").trim();
            if s.is_empty(){bail!("Missing {}",k);}Ok(s.to_owned())
        };
        if actor.trim().is_empty(){bail!("Clinician required");}
        let patient=get("patientId")?;
        let visit=get("visitId")?;
        let dentition=get("dentition")?;
        if !["permanent","primary"].contains(&dentition.as_str()){bail!("Invalid dentition");}
        let notes=value.get("notes").and_then(|v|v.as_str()).unwrap_or("");
        if notes.len()>10000{bail!("Notes too long");}
        let measures=value.get("measurements").and_then(|v|v.as_array()).ok_or_else(||anyhow!("Measurements array required"))?;
        if measures.is_empty()||measures.len()>(if dentition=="permanent"{192}else{120}){bail!("Invalid measurement count");}
        let mut seen=std::collections::HashSet::new();
        for m in measures {
            let tooth=m.get("tooth").and_then(|v|v.as_str()).ok_or_else(||anyhow!("Missing tooth"))?;
            let site=m.get("site").and_then(|v|v.as_str()).ok_or_else(||anyhow!("Missing site"))?;
            if !["MB","B","DB","ML","L","DL"].contains(&site){bail!("Invalid periodontal site");}
            let chars:Vec<char>=tooth.chars().collect();
            if chars.len()!=2||!chars.iter().all(|c|c.is_ascii_digit()){bail!("Invalid FDI");}
            let q=chars[0].to_digit(10).unwrap();
            let n=chars[1].to_digit(10).unwrap();
            let valid=if dentition=="permanent"{(1..=4).contains(&q)&&(1..=8).contains(&n)}
                      else{(5..=8).contains(&q)&&(1..=5).contains(&n)};
            if !valid{bail!("Invalid tooth for dentition");}
            if !seen.insert(format!("{}:{}",tooth,site)){bail!("Duplicate periodontal site");}
            if ["probingDepthMm","gingivalMarginMm","bleeding","plaque","suppuration"].iter().all(|key|m.get(*key).map_or(true,|v|v.is_null())) {
                bail!("Empty periodontal site must not be stored");
            }
            for key in ["probingDepthMm","gingivalMarginMm"] {
                if let Some(v)=m.get(key).filter(|v|!v.is_null()){
                    let n=v.as_f64().ok_or_else(||anyhow!("Invalid numeric measurement"))?;
                    if !n.is_finite() || (key=="probingDepthMm"&&(n<0.0||n>30.0))
                        || (key=="gingivalMarginMm"&&n.abs()>30.0){bail!("Measurement out of range");}
                }
            }
            for key in ["bleeding","plaque","suppuration"] {
                if let Some(v)=m.get(key).filter(|v|!v.is_null()){
                    if !v.is_boolean(){bail!("Invalid boolean measurement");}
                }
            }
        }
        let id=Uuid::new_v4().to_string();let now=now_iso();
        let mut conn=self.conn()?;let tx=conn.transaction()?;
        let owned:i64=tx.query_row("SELECT COUNT(*) FROM visits WHERE id=?1 AND client_id=?2 AND doctor_id=?3 AND deleted=0",params![visit,patient,actor],|r|r.get(0))?;
        if owned!=1{bail!("Visit must belong to patient and acting clinician");}
        tx.execute("INSERT INTO dental_periodontal_exams_v1(id,patient_id,visit_id,clinician_id,dentition,recorded_at,notes) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![id,patient,visit,actor,dentition,now,notes])?;
        for m in measures {
            let flag=|k:&str|m.get(k).and_then(|v|v.as_bool()).map(|v|if v{1_i64}else{0_i64});
            tx.execute("INSERT INTO dental_periodontal_sites_v1(exam_id,tooth_fdi,site,probing_depth_mm,gingival_margin_mm,bleeding,plaque,suppuration) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                params![id,m["tooth"].as_str(),m["site"].as_str(),m["probingDepthMm"].as_f64(),m["gingivalMarginMm"].as_f64(),
                    flag("bleeding"),flag("plaque"),flag("suppuration")])?;
        }
        tx.commit()?;
        Ok(serde_json::json!({"id":id,"patientId":patient,"visitId":visit,"clinicianId":actor,
            "dentition":dentition,"recordedAt":now,"notes":notes,"measurements":measures}))
    }
}


impl Db {
    pub fn dental_treatment_plans_list(&self, patient_id: &str) -> anyhow::Result<Vec<serde_json::Value>> {
        if patient_id.trim().is_empty() { bail!("Patient required"); }
        let conn=self.conn()?;
        let mut stmt=conn.prepare("SELECT id,visit_id,clinician_id,version,status,title,currency,notes,created_at,updated_at FROM dental_treatment_plans_v1 WHERE patient_id=?1 ORDER BY created_at DESC")?;
        let plans=stmt.query_map(params![patient_id], |r| Ok(serde_json::json!({
            "id":r.get::<_,String>(0)?,"patientId":patient_id,"visitId":r.get::<_,String>(1)?,
            "clinicianId":r.get::<_,String>(2)?,"version":r.get::<_,i64>(3)?,
            "status":r.get::<_,String>(4)?,"title":r.get::<_,String>(5)?,
            "currency":r.get::<_,String>(6)?,"notes":r.get::<_,String>(7)?,
            "createdAt":r.get::<_,String>(8)?,"updatedAt":r.get::<_,String>(9)?
        })))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let mut result=Vec::new();
        for mut plan in plans {
            let id=plan["id"].as_str().unwrap_or("").to_owned();
            let mut items=conn.prepare("SELECT id,phase_number,tooth_fdi,procedure_name,quantity,unit_price_cents,position,notes FROM dental_treatment_plan_items_v1 WHERE plan_id=?1 ORDER BY phase_number,position")?;
            let rows=items.query_map(params![id],|r|Ok(serde_json::json!({
                "id":r.get::<_,String>(0)?,"phaseNumber":r.get::<_,i64>(1)?,
                "tooth":r.get::<_,Option<String>>(2)?,"procedureName":r.get::<_,String>(3)?,
                "quantity":r.get::<_,f64>(4)?,"unitPriceCents":r.get::<_,i64>(5)?,
                "position":r.get::<_,i64>(6)?,"notes":r.get::<_,String>(7)?
            })))?.collect::<rusqlite::Result<Vec<_>>>()?;
            plan["items"]=serde_json::json!(rows);
            result.push(plan);
        }
        Ok(result)
    }
}

impl Db {
    pub fn orthopedic_rehab_phase_reviews_list(&self, rehab_plan_id: &str) -> anyhow::Result<Vec<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id,rehab_plan_id,phase,criteria,decision,clinician_id,reason,reviewed_at FROM orthopedic_rehab_phase_reviews WHERE rehab_plan_id=?1 ORDER BY reviewed_at DESC")?;
        let rows = stmt.query_map([rehab_plan_id], |r| Ok(serde_json::json!({
            "id":r.get::<_,String>(0)?,"rehab_plan_id":r.get::<_,String>(1)?,
            "phase":r.get::<_,String>(2)?,"criteria":r.get::<_,String>(3)?,
            "decision":r.get::<_,String>(4)?,"clinician_id":r.get::<_,String>(5)?,
            "reason":r.get::<_,String>(6)?,"reviewed_at":r.get::<_,String>(7)?
        })))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn orthopedic_rehab_phase_review_add(&self, value: &serde_json::Value, clinician: &str) -> anyhow::Result<()> {
        if clinician.trim().is_empty() { bail!("Identiteti i profesionistit kërkohet."); }
        let conn = self.conn.lock().unwrap();
        let plan = value["rehab_plan_id"].as_str().unwrap_or_default();
        if !conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM orthopedic_rehab_plans WHERE id=?1)",[plan],|r|r.get::<_,bool>(0))? {
            bail!("Protokolli i rehabilitimit nuk ekziston.");
        }
        let decision = value["decision"].as_str().unwrap_or_default();
        let reason = value["reason"].as_str().unwrap_or_default();
        let criteria = value["criteria"].as_str().unwrap_or_default();
        if !matches!(decision, "continue"|"advance"|"hold"|"stop") ||
            criteria.trim().is_empty() || reason.trim().is_empty() {
            bail!("Kalimi i fazës kërkon kritere, vlerësim dhe arsye klinike.");
        }
        conn.execute("INSERT INTO orthopedic_rehab_phase_reviews(
            id,rehab_plan_id,phase,criteria,decision,clinician_id,reason,reviewed_at)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",rusqlite::params![
            Uuid::new_v4().to_string(),plan,value["phase"].as_str().unwrap_or_default(),
            criteria,decision,clinician,reason,now_iso()])?;
        Ok(())
    }

    pub fn orthopedic_reports_list(&self, episode_id: &str) -> anyhow::Result<Vec<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id,episode_id,type,version,status,body_json,author_id,reason,created_at FROM orthopedic_report_versions WHERE episode_id=?1 ORDER BY type,version DESC")?;
        let rows = stmt.query_map([episode_id], |r| Ok(serde_json::json!({
            "id":r.get::<_,String>(0)?,"episode_id":r.get::<_,String>(1)?,
            "type":r.get::<_,String>(2)?,"version":r.get::<_,i64>(3)?,
            "status":r.get::<_,String>(4)?,"body_json":r.get::<_,String>(5)?,
            "author_id":r.get::<_,String>(6)?,"reason":r.get::<_,String>(7)?,
            "created_at":r.get::<_,String>(8)?
        })))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Fetch a single immutable saved version and its authoritative SQL-linked
    /// patient identity. The PDF renderer must not trust client-supplied details.
    pub fn orthopedic_report_pdf_data(&self, report_id: &str) -> anyhow::Result<serde_json::Value> {
        if report_id.trim().is_empty() { bail!("Zgjidh versionin e raportit."); }
        let conn = self.conn.lock().unwrap();
        let report = conn.query_row(
            "SELECT r.id,r.episode_id,r.type,r.version,r.status,r.body_json,
                    r.author_id,r.reason,r.created_at,e.diagnosis,e.region,e.laterality,
                    e.status,c.name,COALESCE(c.patient_code,'')
             FROM orthopedic_report_versions r
             JOIN orthopedic_episodes e ON e.id=r.episode_id
             JOIN clients c ON c.id=e.client_id
             WHERE r.id=?1",
            [report_id],
            |r| Ok(serde_json::json!({
                "id":r.get::<_,String>(0)?,
                "episode_id":r.get::<_,String>(1)?,
                "type":r.get::<_,String>(2)?,
                "version":r.get::<_,i64>(3)?,
                "status":r.get::<_,String>(4)?,
                "body_json":r.get::<_,String>(5)?,
                "author_id":r.get::<_,String>(6)?,
                "reason":r.get::<_,String>(7)?,
                "created_at":r.get::<_,String>(8)?,
                "diagnosis":r.get::<_,String>(9)?,
                "region":r.get::<_,String>(10)?,
                "laterality":r.get::<_,String>(11)?,
                "episode_status":r.get::<_,String>(12)?,
                "patient_name":r.get::<_,String>(13)?,
                "patient_code":r.get::<_,String>(14)?
            })),
        ).optional()?;
        report.ok_or_else(|| anyhow!("Versioni i raportit ortopedik nuk u gjet."))
    }

    pub fn orthopedic_report_version_create(&self,value: &serde_json::Value, clinician: &str) -> anyhow::Result<serde_json::Value> {
        if clinician.trim().is_empty() { bail!("Mjeku i identifikuar kërkohet."); }
        let episode = value["episode_id"].as_str().unwrap_or_default();
        let kind = value["type"].as_str().unwrap_or_default();
        let status = value["status"].as_str().unwrap_or_default();
        let body = value["body_json"].as_str().unwrap_or_default();
        let reason = value["reason"].as_str().unwrap_or_default();
        if !matches!(kind,"initial_assessment"|"fracture_followup"|"surgical_plan"|
            "operative_report"|"implant_record"|"postoperative_followup"|
            "rehabilitation_progress"|"final_summary") {
            bail!("Lloji i raportit ortopedik nuk mbështetet.");
        }
        if !matches!(status,"draft"|"final"|"amendment") {
            bail!("Statusi i raportit është i pavlefshëm.");
        }
        if body.trim().is_empty() || body.len()>200_000 {
            bail!("Raporti kërkon përmbajtje reale dhe gjatësi të kufizuar.");
        }
        let _:serde_json::Value = serde_json::from_str(body)?;
        if status=="amendment" && reason.trim().is_empty() {
            bail!("Amendamenti i një raporti final kërkon arsye.");
        }
        let mut conn = self.conn.lock().unwrap();
        Self::orthopedic_require_episode(&conn,episode)?;
        let tx = conn.transaction()?;
        let last:i64=tx.query_row(
            "SELECT COALESCE(MAX(version),0) FROM orthopedic_report_versions WHERE episode_id=?1 AND type=?2",
            rusqlite::params![episode,kind],|r|r.get(0))?;
        if status=="amendment" {
            let signed:i64 = tx.query_row(
                "SELECT COUNT(*) FROM orthopedic_report_versions
                 WHERE episode_id=?1 AND type=?2 AND status='final'",
                rusqlite::params![episode,kind], |r|r.get(0))?;
            if signed == 0 { bail!("Amendamenti kërkon një raport final ekzistues."); }
        }
        let version = last+1;
        let id=Uuid::new_v4().to_string(); let now=now_iso();
        tx.execute("INSERT INTO orthopedic_report_versions(
            id,episode_id,type,version,status,body_json,author_id,reason,created_at)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            rusqlite::params![id,episode,kind,version,status,body,clinician,reason,now])?;
        tx.commit()?;
        Ok(serde_json::json!({"id":id,"episode_id":episode,"type":kind,
            "version":version,"status":status,"body_json":body,
            "author_id":clinician,"reason":reason,"created_at":now}))
    }
}


/// Hashes only minimal fiscal state metadata, never clinical content.
fn sef_event_canonical(tx: &rusqlite::Transaction<'_>, kind: &str, event_id: i64) -> anyhow::Result<String> {
    let sql=match kind {
        "SALE" => "SELECT sale_id,event_type,COALESCE(from_status,''),to_status,COALESCE(error_code,''),occurred_at FROM sef_mjeku_events WHERE id=?1",
        "ACTION" => "SELECT request_id,event_type,COALESCE(from_status,''),to_status,COALESCE(error_code,''),occurred_at FROM sef_mjeku_action_events WHERE id=?1",
        _=>bail!("Unknown fiscal audit event kind"),
    };
    let fields:(String,String,String,String,String,String)=tx.query_row(sql,[event_id],|r|Ok((
        r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?
    )))?;
    Ok(serde_json::to_string(&(kind,event_id,fields))?)
}

fn sef_event_hash(previous:&str, canonical:&str)->String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(format!("{previous}\\n{canonical}").as_bytes()))
}

/// Full verification before each fiscal state mutation. Existing pre-migration
/// events are append-only but are not retrospectively covered by the chain.
fn sef_verify_chain_tx(tx:&rusqlite::Transaction<'_>) -> anyhow::Result<usize> {
    let mut stmt=tx.prepare(
        "SELECT source_kind,source_event_id,previous_hash,event_hash
         FROM sef_mjeku_event_chain ORDER BY sequence_no")?;
    let entries=stmt.query_map([],|r|Ok((
        r.get::<_,String>(0)?,r.get::<_,i64>(1)?,
        r.get::<_,String>(2)?,r.get::<_,String>(3)?
    )))?.collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    let mut previous="0".repeat(64);
    for (kind,event_id,prev,hash) in &entries {
        if prev!=&previous || sef_event_hash(&previous,&sef_event_canonical(tx,kind,*event_id)?)!=*hash {
            bail!("Integriteti i historikut SEF nuk përputhet; veprimet fiskale janë bllokuar.");
        }
        previous=hash.clone();
    }
    Ok(entries.len())
}

fn sef_append_chain_tx(tx:&rusqlite::Transaction<'_>,kind:&str)->anyhow::Result<()> {
    let event_id=tx.last_insert_rowid();
    let previous:String=tx.query_row(
        "SELECT event_hash FROM sef_mjeku_event_chain ORDER BY sequence_no DESC LIMIT 1",
        [],|r|r.get(0)).optional()?.unwrap_or_else(||"0".repeat(64));
    let canonical=sef_event_canonical(tx,kind,event_id)?;
    let hash=sef_event_hash(&previous,&canonical);
    tx.execute("INSERT INTO sef_mjeku_event_chain(source_kind,source_event_id,previous_hash,event_hash,created_at)
               VALUES(?1,?2,?3,?4,?5)",params![kind,event_id,previous,hash,now_iso()])?;
    Ok(())
}

/// Local commercial-only fiscal attempt. Never serialize this struct to the WebView.
#[derive(Debug, Clone)]
pub struct SefLedgerRecord {
    pub sale_id: String,
    pub request_sha256: String,
    pub request_json: String,
    pub allow_any: bool,
    pub total_cents: i64,
    pub context_id: String,
    pub status: String,
    pub coupon_id: Option<String>,
}

impl Db {
    /// Read-only audit of new SHA-256-chained events. Older immutable events
    /// predate the chain; disclose that gap instead of pretending they were covered.
    pub fn sef_audit_verify(&self) -> anyhow::Result<serde_json::Value> {
        let mut conn=self.conn()?;
        let tx=conn.transaction()?;
        let verified=sef_verify_chain_tx(&tx)?;
        let existing:i64=tx.query_row(
            "SELECT (SELECT COUNT(*) FROM sef_mjeku_events) +
                    (SELECT COUNT(*) FROM sef_mjeku_action_events)",
            [],|r|r.get(0))?;
        tx.commit()?;
        Ok(serde_json::json!({
            "verifiedEvents":verified,
            "historicalUnchained":existing.saturating_sub(verified as i64),
            "valid":true
        }))
    }

    pub fn sef_ledger_get(&self, sale_id: &str) -> anyhow::Result<Option<SefLedgerRecord>> {
        let conn = self.conn()?;
        conn.query_row(
            "SELECT sale_id,request_sha256,request_json,allow_any,total_cents,context_id,status,coupon_id
             FROM sef_mjeku_transactions WHERE sale_id=?1",
            params![sale_id], |r| Ok(SefLedgerRecord {
                sale_id:r.get(0)?, request_sha256:r.get(1)?,
                request_json:r.get(2)?, allow_any:r.get::<_,i64>(3)? != 0,
                total_cents:r.get(4)?, context_id:r.get(5)?,
                status:r.get(6)?, coupon_id:r.get(7)?
            })
        ).optional().map_err(Into::into)
    }

    /// Snapshot and idempotency identity are immutable after the first submission.
    /// The existing record is returned; never replace a prior commercial request.
    pub fn sef_ledger_prepare(
        &self, sale_id:&str, hash:&str, payload:&str, allow_any:bool,
        total_cents:i64, context:&str
    ) -> anyhow::Result<SefLedgerRecord> {
        if sale_id.trim().is_empty() || hash.len()!=64 || payload.len()<3 ||
           total_cents<=0 || context.trim().is_empty() {
            bail!("Kërkesa fiskale nuk është e plotë.");
        }
        let mut conn=self.conn()?;
        let tx=conn.transaction()?;
        sef_verify_chain_tx(&tx)?;
        let now=now_iso();
        let inserted=tx.execute(
            "INSERT OR IGNORE INTO sef_mjeku_transactions(
             sale_id,request_sha256,request_json,allow_any,total_cents,context_id,
             status,created_at,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,'READY',?7,?7)",
            params![sale_id,hash,payload,i64::from(allow_any),total_cents,context,now]
        )?;
        if inserted==1 {
            tx.execute("INSERT INTO sef_mjeku_events(
                sale_id,event_type,from_status,to_status,occurred_at)
                VALUES(?1,'PREPARED',NULL,'READY',?2)",params![sale_id,now])?;
            sef_append_chain_tx(&tx,"SALE")?;
        }
        let old=tx.query_row(
            "SELECT sale_id,request_sha256,request_json,allow_any,total_cents,context_id,status,coupon_id
             FROM sef_mjeku_transactions WHERE sale_id=?1",
            [sale_id],|r|Ok(SefLedgerRecord {
                sale_id:r.get(0)?,request_sha256:r.get(1)?,
                request_json:r.get(2)?,allow_any:r.get::<_,i64>(3)?!=0,
                total_cents:r.get(4)?,context_id:r.get(5)?,
                status:r.get(6)?,coupon_id:r.get(7)?
            })
        )?;
        if old.request_sha256!=hash || old.context_id!=context ||
           old.total_cents!=total_cents || old.allow_any!=allow_any {
            bail!("Kjo faturë ka një kërkesë SEF ekzistuese me përmbajtje ose kontekst tjetër. Nuk krijohet kupon i dytë.");
        }
        tx.commit()?;
        Ok(old)
    }

    /// Every state transition is atomic with an append-only event.
    pub fn sef_ledger_transition(
        &self,sale_id:&str,next:&str,event:&str,
        coupon_id:Option<&str>,daily_number:Option<i64>,error_code:Option<&str>
    ) -> anyhow::Result<()> {
        let mut conn=self.conn()?;
        let tx=conn.transaction()?;
        sef_verify_chain_tx(&tx)?;
        let old:(String,Option<String>)=tx.query_row(
            "SELECT status,coupon_id FROM sef_mjeku_transactions WHERE sale_id=?1",
            [sale_id],|r|Ok((r.get(0)?,r.get(1)?))
        )?;
        if old.0==next {
            if let Some(id)=coupon_id {
                if old.1.as_deref()!=Some(id) {
                    bail!("Identifikuesi SEF nuk përputhet me regjistrimin ekzistues.");
                }
            }
            tx.commit()?;
            return Ok(());
        }
        let allowed=matches!((old.0.as_str(),next),
            ("READY","IN_FLIGHT") |
            ("IN_FLIGHT","UNKNOWN"|"FISCALIZED"|"PENDING_OFFLINE"|"FAILED") |
            ("UNKNOWN","FISCALIZED"|"PENDING_OFFLINE"|"FAILED") |
            ("PENDING_OFFLINE","FISCALIZED"|"FAILED") |
            ("FAILED","UNKNOWN")
        );
        if !allowed {bail!("Kalimi i statusit fiskal nuk lejohet.");}
        if matches!(next,"FISCALIZED"|"PENDING_OFFLINE") &&
           coupon_id.or(old.1.as_deref()).is_none() {
            bail!("Kuponi i PROGRAMERI POS mungon.");
        }
        if let (Some(old_id),Some(new_id))=(old.1.as_deref(),coupon_id) {
            if old_id!=new_id {bail!("Numri i kuponit fiskal nuk mund të ndryshojë.");}
        }
        let now=now_iso();
        tx.execute("UPDATE sef_mjeku_transactions SET status=?2,
            coupon_id=COALESCE(coupon_id,?3),
            daily_number=COALESCE(daily_number,?4),
            error_code=?5,updated_at=?6 WHERE sale_id=?1",
            params![sale_id,next,coupon_id,daily_number,error_code,now])?;
        tx.execute("INSERT INTO sef_mjeku_events(
            sale_id,event_type,from_status,to_status,error_code,occurred_at)
            VALUES(?1,?2,?3,?4,?5,?6)",
            params![sale_id,event,old.0,next,error_code,now])?;
        sef_append_chain_tx(&tx,"SALE")?;
        tx.commit()?;
        Ok(())
    }

    pub fn sef_ledger_list(&self) -> anyhow::Result<Vec<serde_json::Value>> {
        let conn=self.conn()?;
        let mut stmt=conn.prepare(
            "SELECT sale_id,status,total_cents,coupon_id,daily_number,error_code,created_at,updated_at
             FROM sef_mjeku_transactions ORDER BY updated_at DESC LIMIT 200")?;
        let rows=stmt.query_map([],|r|Ok(serde_json::json!({
            "saleId":r.get::<_,String>(0)?,
            "status":r.get::<_,String>(1)?,
            "totalCents":r.get::<_,i64>(2)?,
            "couponId":r.get::<_,Option<String>>(3)?,
            "dailyNumber":r.get::<_,Option<i64>>(4)?,
            "errorCode":r.get::<_,Option<String>>(5)?,
            "createdAt":r.get::<_,String>(6)?,
            "updatedAt":r.get::<_,String>(7)?
        })))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

#[cfg(test)]
mod sef_mjeku_ledger_tests {
    use super::*;

    #[test]
    fn sef_ledger_is_immutable_idempotent_and_append_only() {
        let db=Db::new_in_memory().unwrap();
        {
            let conn=db.conn().unwrap();
            conn.execute("INSERT INTO clients(id,name) VALUES('test-client','Test')",[]).unwrap();
            conn.execute("INSERT INTO sales(id,client_id,total) VALUES('test-sale','test-client',25.0)",[]).unwrap();
        }
        let hash="a".repeat(64);
        let snapshot=r#"{"source":"mjeku","saleId":"test-sale","items":[]}"#;
        let first=db.sef_ledger_prepare("test-sale",&hash,snapshot,true,2500,"ctx-1").unwrap();
        assert_eq!(first.status,"READY");
        assert_eq!(db.sef_ledger_prepare("test-sale",&hash,snapshot,true,2500,"ctx-1").unwrap().status,"READY");
        assert!(db.sef_ledger_prepare("test-sale",&"b".repeat(64),snapshot,true,2500,"ctx-1").is_err());
        assert!(db.sef_ledger_prepare("test-sale",&hash,snapshot,true,2500,"ctx-2").is_err());
        db.sef_ledger_transition("test-sale","IN_FLIGHT","DISPATCH",None,None,None).unwrap();
        assert!(db.sef_ledger_transition("test-sale","IN_FLIGHT","DISPATCH",None,None,None).is_ok());
        db.sef_ledger_transition("test-sale","UNKNOWN","TIMEOUT",None,None,Some("TIMEOUT")).unwrap();
        assert!(db.sef_ledger_transition("test-sale","IN_FLIGHT","RETRY",None,None,None).is_err());
        db.sef_ledger_transition("test-sale","PENDING_OFFLINE","RECONCILED",Some("1001"),Some(7),None).unwrap();
        db.sef_ledger_transition("test-sale","FISCALIZED","POS_CONFIRMED",Some("1001"),Some(7),None).unwrap();
        assert!(db.sef_ledger_transition("test-sale","FAILED","REVERT",None,None,None).is_err());
        let conn=db.conn().unwrap();
        assert!(conn.execute("DELETE FROM sef_mjeku_transactions WHERE sale_id='test-sale'",[]).is_err());
        assert!(conn.execute("UPDATE sef_mjeku_events SET event_type='x'",[]).is_err());
        assert!(conn.execute("DELETE FROM sef_mjeku_events",[]).is_err());
        let events:i64=conn.query_row("SELECT COUNT(*) FROM sef_mjeku_events WHERE sale_id='test-sale'",[],|r|r.get(0)).unwrap();
        assert_eq!(events,5);
    }
}


/// Durable local record of a fiscal return/cancel. No patient identifiers or clinical notes.
#[derive(Debug, Clone)]
pub struct SefActionRecord {
    pub request_id: String,
    pub kind: String,
    pub original_coupon_id: String,
    pub context_id: String,
    pub request_sha256: String,
    pub request_json: String,
    pub status: String,
    pub coupon_id: Option<String>,
}

impl Db {
    /// A fresh UUID is written in the same transaction as the immutable request.
    /// A partial/unknown previous action on this coupon prevents a new one.
    pub fn sef_action_prepare(
        &self, kind: &str, original_coupon_id: &str,
        params_json: &serde_json::Value, context: &str,
    ) -> anyhow::Result<SefActionRecord> {
        use sha2::Digest;
        if !matches!(kind,"RETURN"|"CANCEL") ||
           original_coupon_id.trim().parse::<i64>().ok().filter(|x|*x>0).is_none() ||
           context.trim().is_empty() {
            bail!("Kërkesa SEF për kthim/anulim nuk është e vlefshme.");
        }
        let request_id=Uuid::new_v4().to_string();
        let mut payload=params_json.clone();
        let obj=payload.as_object_mut().ok_or_else(||anyhow::anyhow!("Invalid SEF action payload"))?;
        obj.insert("requestId".into(),serde_json::json!(request_id));
        let request_json=serde_json::to_string(&payload)?;
        let request_sha256=hex::encode(sha2::Sha256::digest(request_json.as_bytes()));
        let now=now_iso();
        let mut conn=self.conn()?;
        let tx=conn.transaction()?;
        sef_verify_chain_tx(&tx)?;
        tx.execute("INSERT INTO sef_mjeku_actions(
            request_id,kind,original_coupon_id,context_id,request_sha256,request_json,
            status,created_at,updated_at)
            VALUES(?1,?2,?3,?4,?5,?6,'READY',?7,?7)",
            params![request_id,kind,original_coupon_id,context,request_sha256,request_json,now])?;
        tx.execute("INSERT INTO sef_mjeku_action_events(
            request_id,event_type,from_status,to_status,occurred_at)
            VALUES(?1,'PREPARED',NULL,'READY',?2)",params![request_id,now])?;
        sef_append_chain_tx(&tx,"ACTION")?;
        tx.commit()?;
        Ok(SefActionRecord {
            request_id,kind:kind.to_string(),original_coupon_id:original_coupon_id.to_string(),
            context_id:context.to_string(),request_sha256,request_json,
            status:"READY".into(),coupon_id:None
        })
    }

    pub fn sef_action_get(&self, request_id:&str)->anyhow::Result<Option<SefActionRecord>> {
        let conn=self.conn()?;
        conn.query_row("SELECT request_id,kind,original_coupon_id,context_id,
            request_sha256,request_json,status,coupon_id FROM sef_mjeku_actions WHERE request_id=?1",
            [request_id],|r|Ok(SefActionRecord{
                request_id:r.get(0)?,kind:r.get(1)?,original_coupon_id:r.get(2)?,
                context_id:r.get(3)?,request_sha256:r.get(4)?,request_json:r.get(5)?,
                status:r.get(6)?,coupon_id:r.get(7)?
            })).optional().map_err(Into::into)
    }

    /// State changes and immutable audit events commit atomically.
    pub fn sef_action_transition(
        &self,request_id:&str,next:&str,event:&str,
        coupon_id:Option<&str>,daily_number:Option<i64>,error_code:Option<&str>
    )->anyhow::Result<()> {
        let mut conn=self.conn()?;
        let tx=conn.transaction()?;
        sef_verify_chain_tx(&tx)?;
        let old:(String,Option<String>)=tx.query_row(
            "SELECT status,coupon_id FROM sef_mjeku_actions WHERE request_id=?1",
            [request_id],|r|Ok((r.get(0)?,r.get(1)?)))?;
        if old.0==next {
            if let (Some(existing),Some(incoming))=(old.1.as_deref(),coupon_id) {
                if existing!=incoming {bail!("Kuponi SEF i veprimit ndryshoi.");}
            }
            tx.commit()?;
            return Ok(());
        }
        let allowed=matches!((old.0.as_str(),next),
            ("READY","IN_FLIGHT") |
            ("IN_FLIGHT","UNKNOWN"|"FISCALIZED"|"PENDING_OFFLINE"|"FAILED") |
            ("UNKNOWN","FISCALIZED"|"PENDING_OFFLINE"|"FAILED") |
            ("PENDING_OFFLINE","FISCALIZED"|"FAILED")
        );
        if !allowed {bail!("Kalimi i statusit të veprimit SEF nuk lejohet.");}
        if matches!(next,"FISCALIZED"|"PENDING_OFFLINE") &&
           coupon_id.or(old.1.as_deref()).is_none() {
            bail!("Kuponi i veprimit nuk është konfirmuar nga POS.");
        }
        if let (Some(existing),Some(incoming))=(old.1.as_deref(),coupon_id) {
            if existing!=incoming {bail!("Kuponi i veprimit nuk mund të ndryshojë.");}
        }
        let now=now_iso();
        tx.execute("UPDATE sef_mjeku_actions SET status=?2,
            coupon_id=COALESCE(coupon_id,?3),daily_number=COALESCE(daily_number,?4),
            error_code=?5,updated_at=?6 WHERE request_id=?1",
            params![request_id,next,coupon_id,daily_number,error_code,now])?;
        tx.execute("INSERT INTO sef_mjeku_action_events(
            request_id,event_type,from_status,to_status,error_code,occurred_at)
            VALUES(?1,?2,?3,?4,?5,?6)",
            params![request_id,event,old.0,next,error_code,now])?;
        sef_append_chain_tx(&tx,"ACTION")?;
        tx.commit()?;
        Ok(())
    }

    pub fn sef_action_list(&self)->anyhow::Result<Vec<serde_json::Value>> {
        let conn=self.conn()?;
        let mut stmt=conn.prepare("SELECT request_id,kind,original_coupon_id,status,
            coupon_id,daily_number,error_code,created_at,updated_at
            FROM sef_mjeku_actions ORDER BY updated_at DESC LIMIT 200")?;
        let rows=stmt.query_map([],|r|Ok(serde_json::json!({
            "requestId":r.get::<_,String>(0)?,
            "kind":r.get::<_,String>(1)?,
            "originalCouponId":r.get::<_,String>(2)?,
            "status":r.get::<_,String>(3)?,
            "couponId":r.get::<_,Option<String>>(4)?,
            "dailyNumber":r.get::<_,Option<i64>>(5)?,
            "errorCode":r.get::<_,Option<String>>(6)?,
            "createdAt":r.get::<_,String>(7)?,
            "updatedAt":r.get::<_,String>(8)?
        })))?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }
}

#[cfg(test)]
mod sef_mjeku_action_tests {
    use super::*;

    #[test]
    fn action_journal_is_immutable_and_fail_closed_on_uncertain_result() {
        let db=Db::new_in_memory().unwrap();
        let params=serde_json::json!({"couponId":"1001","reason":"Gabim"});
        let first=db.sef_action_prepare("CANCEL","1001",&params,"ctx").unwrap();
        assert_eq!(first.status,"READY");
        assert!(first.request_json.contains(&first.request_id));
        assert!(db.sef_action_prepare("RETURN","1001",&params,"ctx").is_err());
        db.sef_action_transition(&first.request_id,"IN_FLIGHT","DISPATCH",None,None,None).unwrap();
        db.sef_action_transition(&first.request_id,"UNKNOWN","TIMEOUT",None,None,Some("TIMEOUT")).unwrap();
        assert!(db.sef_action_prepare("CANCEL","1001",&params,"ctx").is_err());
        assert!(db.sef_action_transition(&first.request_id,"IN_FLIGHT","RETRY",None,None,None).is_err());
        db.sef_action_transition(&first.request_id,"FISCALIZED","POS_RECONCILED",Some("2001"),Some(2),None).unwrap();
        assert!(db.sef_action_prepare("RETURN","1001",&params,"ctx").is_ok());
        let conn=db.conn().unwrap();
        assert!(conn.execute("DELETE FROM sef_mjeku_actions WHERE request_id=?1",[&first.request_id]).is_err());
        assert!(conn.execute("UPDATE sef_mjeku_actions SET request_json='{}' WHERE request_id=?1",[&first.request_id]).is_err());
        assert!(conn.execute("DELETE FROM sef_mjeku_action_events",[]).is_err());
        conn.execute_batch(include_str!("../migrations/sef_mjeku_v2.sql")).unwrap();
    }
}


impl Db {
    /// Only explicit fiscal classification may cross the POS bridge; no clinical notes.
    pub fn sef_fiscal_catalog_for(&self, item_id: &str) -> anyhow::Result<Option<(String,String)>> {
        let conn=self.conn()?;
        conn.query_row(
            "SELECT category_code,unit_code FROM sef_mjeku_item_catalog WHERE visit_item_id=?1",
            [item_id], |r| Ok((r.get(0)?,r.get(1)?))
        ).optional().map_err(Into::into)
    }

    pub fn sef_item_fiscal_catalog_set(&self, item_id:&str, category_code:&str, unit_code:&str) -> anyhow::Result<()> {
        let category=category_code.trim().to_ascii_uppercase();
        let unit=unit_code.trim().to_ascii_uppercase();
        if !matches!(category.as_str(),"SM"|"BM"|"PM") ||
           !matches!(unit.as_str(),"EA"|"HUR"|"DAY") {
            bail!("Zgjidh kategori (SM/BM/PM) dhe njësi (EA/HUR/DAY) të vlefshme sipas ATK.");
        }
        let mut conn=self.conn()?;
        let tx=conn.transaction()?;
        let allowed:i64=tx.query_row(
            "SELECT COUNT(*) FROM visit_items WHERE id=?1 AND deleted=0 AND fiscal=1 AND fiscalized=0",
            [item_id], |r|r.get(0))?;
        if allowed!=1 { bail!("Kategoria dhe njësia fiskale nuk mund të ndryshohen për këtë procedurë."); }
        let now=now_iso();
        tx.execute("INSERT INTO sef_mjeku_item_catalog(visit_item_id,category_code,unit_code,created_at,updated_at)
             VALUES(?1,?2,?3,?4,?4)
             ON CONFLICT(visit_item_id) DO UPDATE SET
               category_code=excluded.category_code,unit_code=excluded.unit_code,updated_at=excluded.updated_at",
            params![item_id,category,unit,now])?;
        tx.commit()?;
        Ok(())
    }

    pub fn sef_item_fiscal_catalog_list(&self, visit_id:&str) -> anyhow::Result<Vec<serde_json::Value>> {
        let conn=self.conn()?;
        let mut stmt=conn.prepare(
            "SELECT c.visit_item_id,c.category_code,c.unit_code FROM sef_mjeku_item_catalog c
             JOIN visit_items i ON i.id=c.visit_item_id
             WHERE i.visit_id=?1 AND i.deleted=0 ORDER BY i.created_at")?;
        let rows=stmt.query_map([visit_id],|r|Ok(serde_json::json!({
            "itemId":r.get::<_,String>(0)?,
            "categoryCode":r.get::<_,String>(1)?,
            "unitCode":r.get::<_,String>(2)?
        })))?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// Only explicit, non-clinical descriptions are sent to the fiscal engine.
    pub fn sef_fiscal_title_for(&self, item_id:&str)->anyhow::Result<Option<String>> {
        let conn=self.conn()?;
        conn.query_row("SELECT fiscal_title FROM sef_mjeku_item_titles WHERE visit_item_id=?1",
            [item_id],|r|r.get(0)).optional().map_err(Into::into)
    }

    pub fn sef_item_fiscal_title_set(&self, item_id:&str, title:&str)->anyhow::Result<()> {
        let title=title.trim();
        if item_id.trim().is_empty() || title.chars().count()<3 || title.chars().count()>120 ||
           title.chars().any(char::is_control) {
            bail!("Përshkrimi fiskal duhet të jetë 3–120 karaktere pa rreshta të rinj.");
        }
        let mut conn=self.conn()?;
        let tx=conn.transaction()?;
        let allowed:i64=tx.query_row(
            "SELECT COUNT(*) FROM visit_items WHERE id=?1 AND deleted=0 AND fiscal=1 AND fiscalized=0",
            [item_id],|r|r.get(0))?;
        if allowed!=1 {bail!("Përshkrimi fiskal nuk mund të ndryshohet për këtë procedurë.");}
        let now=now_iso();
        tx.execute("INSERT INTO sef_mjeku_item_titles(visit_item_id,fiscal_title,created_at,updated_at)
            VALUES(?1,?2,?3,?3)
            ON CONFLICT(visit_item_id) DO UPDATE SET fiscal_title=excluded.fiscal_title,updated_at=excluded.updated_at",
            params![item_id,title,now])?;
        tx.commit()?;
        Ok(())
    }

    pub fn sef_item_fiscal_titles_list(&self, visit_id:&str)->anyhow::Result<Vec<serde_json::Value>> {
        let conn=self.conn()?;
        let mut stmt=conn.prepare(
            "SELECT t.visit_item_id,t.fiscal_title FROM sef_mjeku_item_titles t
             JOIN visit_items i ON i.id=t.visit_item_id
             WHERE i.visit_id=?1 AND i.deleted=0 ORDER BY i.created_at")?;
        let rows=stmt.query_map([visit_id],|r|Ok(serde_json::json!({
            "itemId":r.get::<_,String>(0)?,
            "fiscalTitle":r.get::<_,String>(1)?
        })))?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }
}

#[cfg(test)]
mod sef_mjeku_fiscal_title_tests {
    use super::*;
    #[test]
    fn fiscal_description_requires_explicit_nonclinical_value() {
        let db=Db::new_in_memory().unwrap();
        assert_eq!(db.sef_fiscal_title_for("unknown").unwrap(),None);
        assert!(db.sef_item_fiscal_title_set("unknown","Vizitë mjekësore").is_err());
        let conn=db.conn().unwrap();
        conn.execute_batch(include_str!("../migrations/sef_mjeku_v3.sql")).unwrap();
    }
}
