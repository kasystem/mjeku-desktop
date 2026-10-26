mod auth;
mod db;
mod desktop_updates;
mod error_logs;
mod invoice;
mod orthopedics_pdf;
mod physiotherapy_pdf;
mod lab_devices;
mod license_engine;
mod models;
mod sef_bridge;
mod sync_engine;
mod ui_protocol;
mod updates;
mod util;

use std::collections::HashMap;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::bail;
use chrono::Utc;
use chrono::Duration as ChronoDuration;
use chrono::Datelike;
use tauri::{Emitter, Manager};
#[cfg(desktop)]
use tauri_plugin_updater::UpdaterExt;

use crate::auth::{AuthState, AuthStateInfo, SessionKind};
use crate::db::Db;
use crate::license_engine::LicenseEngine;
use crate::models::{
    AppInfo, Appointment, AppointmentUpsertInput, AppointmentsListFilters, CashEntry,
    CashEntryUpsertInput, CashListFilters, Client, ClientPhoto, ClientUpsertInput, DailySalesReport, Doctor,
    DoctorLoginOption, DoctorUpsertInput, Payment, PaymentUpsertInput, PaymentsListFilters, Sale,
    SaleUpsertInput, SalesListFilters, Service, ServiceUpsertInput, Visit, VisitItem,
    VisitItemUpsertInput, VisitItemsListFilters, VisitUpsertInput, VisitsListFilters,
};
use crate::sync_engine::SyncEngine;
use crate::updates::UpdatesEngine;

const FISCAL_PRINTER_PROVIDER_KEY: &str = "fiscal_printer_provider";
const SEF_PRINTER_NAME_KEY: &str = "sef_printer_name";
const KEY_SUPABASE_URL: &str = "supabase_url";
const KEY_SUPABASE_API_KEY: &str = "supabase_api_key";
const KEY_SUPABASE_ANON_KEY: &str = "supabase_anon_key";
const KEY_DESKTOP_UPDATE_API: &str = "desktop_update_api";
const KEY_DESKTOP_UPDATE_LAST_CHECKED_AT: &str = "desktop_update_last_checked_at";
const KEY_DESKTOP_UPDATE_LAST_MANUAL_CHECK_AT: &str = "desktop_update_last_manual_check_at";
const KEY_DESKTOP_UPDATE_LATEST_VERSION: &str = "desktop_update_latest_version";
const KEY_DESKTOP_UPDATE_AVAILABLE: &str = "desktop_update_available";
const KEY_DESKTOP_UPDATE_FIRST_SEEN_AT: &str = "desktop_update_first_seen_at";
const KEY_LAB_DEVICE_PROFILE_ID: &str = "lab_device_profile_id";
const KEY_LAB_DEVICE_PORT_NAME: &str = "lab_device_port_name";
const DESKTOP_UPDATE_FORCE_AFTER_DAYS: i64 = 7;
const DEFAULT_SUPABASE_URL: &str = "https://occzpzryzxabajtmdaas.supabase.co";
const DEFAULT_SUPABASE_API_KEY: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6Im9jY3pwenJ5enhhYmFqdG1kYWFzIiwicm9sZSI6ImFub24iLCJpYXQiOjE3NzA2NTQxMjksImV4cCI6MjA4NjIzMDEyOX0.Fq2bTsVLPpRhoLe845Lf-kMsy8rmPF2ijMZCWBb1zHc";
const DEFAULT_UPDATE_BASE_URL: &str = "https://mjeku-ui.onrender.com";
const OLD_DEFAULT_UPDATE_BASE_URL: &str = "https://mjeku-ui.vercel.app";

const TOKEN_MODE_EXISTING: &str = "existing";
const TOKEN_MODE_NEW: &str = "new";

struct AppState {
    db: Arc<Db>,
    auth: Arc<AuthState>,
    sync: Arc<SyncEngine>,
    updates: Arc<UpdatesEngine>,
    license: Arc<LicenseEngine>,
    error_log_path: PathBuf,
    lab_serial: Arc<lab_devices::LabSerialManager>,
    is_demo_mode: bool,
}

fn err_string(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn ensure_setting_if_empty(db: &Db, key: &str, value: &str) -> anyhow::Result<()> {
    if value.trim().is_empty() {
        return Ok(());
    }
    let current = db.setting_get(key)?.unwrap_or_default();
    if current.trim().is_empty() {
        db.setting_set(key, value)?;
    }
    Ok(())
}

fn add_days_to_iso(ts: &str, days: i64) -> Option<String> {
    let dt = crate::util::parse_rfc3339_to_utc(ts).ok()?;
    let next = dt + ChronoDuration::days(days);
    Some(next.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

/// Krahason dy versione "x.y.z" numerikisht — kthen true vetëm nëse `remote`
/// është STRIKT më i ri se `local`. Kjo shmang rastin kur manifesti i update-it
/// mbetet i ngrirë/gabuar në një version më të vjetër (p.sh. "0.4.9") dhe kyçi
/// i vjetër `!=` e trajtonte gabimisht si "update i ri" edhe pse ishte më i vjetër.
fn version_is_newer(remote: &str, local: &str) -> bool {
    fn parse(v: &str) -> (u64, u64, u64) {
        let v = v.trim().trim_start_matches('v');
        let mut parts = v.split('.').map(|p| p.trim().parse::<u64>().unwrap_or(0));
        (
            parts.next().unwrap_or(0),
            parts.next().unwrap_or(0),
            parts.next().unwrap_or(0),
        )
    }
    parse(remote) > parse(local)
}

fn compute_desktop_update_policy_from_db(
    db: &Db,
    current_version: &str,
) -> anyhow::Result<(bool, Option<String>, Option<String>, Option<String>)> {
    let now = crate::util::parse_rfc3339_to_utc(&crate::util::now_iso())?;
    let available = db
        .setting_get(KEY_DESKTOP_UPDATE_AVAILABLE)?
        .unwrap_or_default()
        .trim()
        == "1";
    let latest = db
        .setting_get(KEY_DESKTOP_UPDATE_LATEST_VERSION)?
        .and_then(|x| {
            let t = x.trim().to_string();
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        });
    let last_manual = db
        .setting_get(KEY_DESKTOP_UPDATE_LAST_MANUAL_CHECK_AT)?
        .and_then(|x| {
            let t = x.trim().to_string();
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        });
    let first_seen = db
        .setting_get(KEY_DESKTOP_UPDATE_FIRST_SEEN_AT)?
        .and_then(|x| {
            let t = x.trim().to_string();
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        });

    let latest_is_new = latest
        .as_deref()
        .map(|x| version_is_newer(x, current_version))
        .unwrap_or(false);
    if !available || !latest_is_new {
        return Ok((false, latest, None, last_manual));
    }

    let base_ts = last_manual.as_deref().or(first_seen.as_deref());
    let force_deadline_at =
        base_ts.and_then(|x| add_days_to_iso(x, DESKTOP_UPDATE_FORCE_AFTER_DAYS));
    let forced = force_deadline_at
        .as_deref()
        .and_then(|x| crate::util::parse_rfc3339_to_utc(x).ok())
        .map(|dl| now > dl)
        .unwrap_or(false);

    Ok((forced, latest, force_deadline_at, last_manual))
}

fn append_error_log(state: &AppState, source: &str, message: &str) {
    let _ = crate::error_logs::append(&state.error_log_path, source, message);
}

fn dev_server_is_reachable(dev_url: &str) -> bool {
    // We only need a cheap check for localhost Vite availability.
    let mut rest = dev_url.trim();
    if let Some(s) = rest.strip_prefix("http://") {
        rest = s;
    } else if let Some(s) = rest.strip_prefix("https://") {
        rest = s;
    }

    let host_port = rest.split('/').next().unwrap_or(rest);
    let (host, port) = match host_port.rsplit_once(':') {
        Some((h, p)) => (h, p),
        None => return false,
    };
    let port: u16 = match port.parse() {
        Ok(p) => p,
        Err(_) => return false,
    };

    let addrs = match (host, port).to_socket_addrs() {
        Ok(a) => a,
        Err(_) => return false,
    };
    for addr in addrs {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
            return true;
        }
    }
    false
}

fn fiscal_temp_dir() -> anyhow::Result<PathBuf> {
    #[cfg(target_os = "windows")]
    let dir = PathBuf::from(r"C:\Temp");
    #[cfg(not(target_os = "windows"))]
    let dir = std::env::temp_dir().join("mjeku-fiscal");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn write_fiscal_temp_inp(prefix: &str, body: &str) -> anyhow::Result<PathBuf> {
    let dir = fiscal_temp_dir()?;
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let rnd = uuid::Uuid::new_v4().to_string();
    let name = format!("{}-{}-{}.inp", prefix, ts, &rnd[..8]);
    let path = dir.join(name);
    let tmp_name = format!(
        "{}.tmp-{}",
        path.file_name()
            .and_then(|x| x.to_str())
            .unwrap_or("fiscal.inp"),
        &rnd[..8]
    );
    let tmp_path = path.with_file_name(tmp_name);
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp_path)?;
        f.write_all(body.as_bytes())?;
        f.flush()?;
        let _ = f.sync_all();
    }
    std::fs::rename(&tmp_path, &path)?;
    Ok(path)
}

fn wait_out_text(inp_path: &Path, timeout: Duration) -> Option<String> {
    let out_path = inp_path.with_extension("out");
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        match std::fs::read_to_string(&out_path) {
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

fn has_note_status_2(raw: &str) -> bool {
    let s = raw.to_ascii_lowercase();
    s.contains("notestatus;2")
}

fn looks_like_jwt_token(token: &str) -> bool {
    let t = token.trim();
    t.starts_with("eyJ") && t.matches('.').count() >= 2
}

fn with_supabase_auth_request(
    req: reqwest::RequestBuilder,
    api_key: &str,
) -> reqwest::RequestBuilder {
    let req = req.header("apikey", api_key);
    if looks_like_jwt_token(api_key) {
        req.bearer_auth(api_key)
    } else {
        req
    }
}

fn normalize_provision_token(v: &str) -> String {
    v.trim().to_ascii_uppercase().replace(' ', "")
}

fn normalize_token_mode(v: Option<&str>, has_clinic_id: bool) -> &'static str {
    let x = v.unwrap_or("").trim().to_ascii_lowercase();
    if x == TOKEN_MODE_NEW || x == "new_clinic" || x == "setup" {
        return TOKEN_MODE_NEW;
    }
    if x == TOKEN_MODE_EXISTING || x == "existing_clinic" || x == "login" {
        return TOKEN_MODE_EXISTING;
    }
    if has_clinic_id {
        TOKEN_MODE_EXISTING
    } else {
        TOKEN_MODE_NEW
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
struct ProvisionTokenRow {
    clinic_id: Option<String>,
    clinic_name: Option<String>,
    mode: Option<String>,
    one_time: Option<bool>,
    disabled: Option<bool>,
    expires_at: Option<String>,
    used_at: Option<String>,
    bootstrap_admin_salt: Option<String>,
    bootstrap_admin_hash: Option<String>,
    bootstrap_user_salt: Option<String>,
    bootstrap_user_hash: Option<String>,
    bootstrap_cashier_salt: Option<String>,
    bootstrap_cashier_hash: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct ProvisionClinicNameRow {
    clinic_name: Option<String>,
}

fn normalize_fiscal_printer_provider(v: &str) -> Option<String> {
    let s = v
        .trim()
        .to_ascii_lowercase()
        .replace('-', "_")
        .replace(' ', "_");
    match s.as_str() {
        "enternet" => Some("enternet".to_string()),
        "global_eu" | "globaleu" | "global" => Some("global_eu".to_string()),
        "sef" => Some("sef".to_string()),
        // SEF përmes PROGRAMERI POS (urë named pipe). E veçantë nga "sef" (Printer Windows).
        "sef_programeri" | "sef_programeri_pos" | "programeri_sef" => {
            Some(crate::sef_bridge::PROVIDER_VALUE.to_string())
        }
        _ => None,
    }
}

async fn get_fiscal_printer_provider(state: &tauri::State<'_, AppState>) -> Result<String, String> {
    let db = state.db.clone();
    let raw = tokio::task::spawn_blocking(move || db.setting_get(FISCAL_PRINTER_PROVIDER_KEY))
        .await
        .map_err(err_string)?
        .map_err(err_string)?
        .unwrap_or_default();

    if raw.trim().is_empty() {
        return Ok("enternet".to_string());
    }

    Ok(normalize_fiscal_printer_provider(&raw).unwrap_or_else(|| raw.trim().to_ascii_lowercase()))
}

fn require_enternet_for_fiscal(provider: &str) -> Result<(), String> {
    if provider == "enternet" {
        return Ok(());
    }
    if provider == "global_eu" {
        return Err("Printeri fiskal i zgjedhur është Global EU. Ky model do të implementohet më vonë; zgjidh ENTERNET te Cilësimet për printim .inp.".to_string());
    }
    Err("Printeri fiskal nuk është i konfiguruar saktë. Te Cilësimet zgjidh ENTERNET, Global EU ose SEF.".to_string())
}

fn build_invoice_pdf_bytes_inner(
    db: &Db,
    data_dir: &std::path::Path,
    sale_id: &str,
    fiscal_only: bool,
) -> Result<Vec<u8>, String> {
    let clinic_name = db
        .setting_get("clinic_name")
        .map_err(err_string)?
        .unwrap_or_else(|| "Klinika".to_string());
    let header_png: Option<Vec<u8>> = db
        .setting_get("pdf_header_path")
        .ok()
        .flatten()
        .and_then(|rel| {
            let rel = rel.trim().to_string();
            if rel.is_empty() { return None; }
            std::fs::read(data_dir.join(rel)).ok()
        });
    let sale = db
        .sales_get(sale_id)
        .map_err(err_string)?
        .ok_or_else(|| "fatura nuk u gjet".to_string())?;
    let invoice_no = db
        .sales_invoice_number(&sale.id)
        .unwrap_or_else(|_| sale.id.clone());
    let client = db
        .clients_get(&sale.client_id)
        .map_err(err_string)?
        .ok_or_else(|| "pacienti nuk u gjet".to_string())?;
    let logo_png: Option<Vec<u8>> = db
        .setting_get("clinic_logo_path")
        .ok()
        .flatten()
        .and_then(|rel| {
            let rel = rel.trim().to_string();
            if rel.is_empty() { return None; }
            std::fs::read(data_dir.join(rel)).ok()
        });
    let vis_items = db
        .visit_items_list(Some(crate::models::VisitItemsListFilters {
            visit_id: Some(sale_id.to_string()),
            client_id: None,
            include_deleted: Some(false),
        }))
        .map_err(err_string)?;
    let mut lines: Vec<crate::invoice::InvoiceLine> = vis_items
        .into_iter()
        .filter(|x| x.deleted == 0)
        .map(|it| crate::invoice::InvoiceLine {
            tooth: it.tooth,
            title: it.title,
            qty: it.qty,
            unit_price: it.unit_price,
            fiscal: it.fiscal == 1,
            vat_code: it.vat_code,
        })
        .collect();
    if fiscal_only && !lines.is_empty() {
        let before = lines.len();
        lines.retain(|ln| ln.fiscal);
        if before > 0 && lines.is_empty() {
            return Err("kjo fature nuk ka pjese fiskale".to_string());
        }
    }
    let mut total = sale.total;
    let mut fiscal_total = sale.total;
    let mut non_fiscal_total = 0.0_f64;
    if !lines.is_empty() {
        total = 0.0;
        fiscal_total = 0.0;
        non_fiscal_total = 0.0;
        for ln in &lines {
            let sub = ln.qty * ln.unit_price;
            total += sub;
            if ln.fiscal { fiscal_total += sub; } else { non_fiscal_total += sub; }
        }
    }
    let bank_account = db
        .setting_get("clinic_bank_account")
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty());
    let data = crate::invoice::InvoicePdfData {
        clinic_name,
        header_png,
        logo_png,
        invoice_id: invoice_no,
        date: sale.date.clone(),
        client_name: client.name,
        client_code: client.patient_code,
        client_dob: client.dob,
        client_address: client.address,
        client_city: client.city,
        client_phone: client.phone,
        client_email: client.email,
        notes: sale.notes.clone(),
        bank_account,
        lines,
        total,
        fiscal_total,
        non_fiscal_total,
    };
    crate::invoice::render_invoice_pdf(&data).map_err(err_string)
}

#[tauri::command]
async fn printers_list() -> Result<Vec<String>, String> {
    #[cfg(target_os = "windows")]
    {
        tokio::task::spawn_blocking(|| {
            use std::os::windows::process::CommandExt;
            let output = std::process::Command::new("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-Command",
                    "[Console]::OutputEncoding=[System.Text.Encoding]::UTF8; $ErrorActionPreference='Stop'; ConvertTo-Json -Compress -InputObject @(Get-CimInstance Win32_Printer | Select-Object -ExpandProperty Name | Sort-Object -Unique)"])
                .creation_flags(0x08000000)
                .output().map_err(|e| format!("Lista e printerëve nuk u lexua: {e}"))?;
            if !output.status.success() {
                return Err("Windows nuk e ktheu listën e printerëve. Kontrollo shërbimin Print Spooler.".to_string());
            }
            serde_json::from_slice::<Vec<String>>(&output.stdout)
                .map_err(|e| format!("Lista e printerëve nuk u lexua: {e}"))
        }).await.map_err(err_string)?
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("Zgjedhja native e printerit nuk është integruar ende në këtë platformë.".to_string())
    }
}

fn print_pdf_to_windows_printer(pdf_bytes: &[u8], printer_name: &str) -> anyhow::Result<()> {
    let file_name = format!("mjeku-print-{}.pdf", uuid::Uuid::new_v4());
    let pdf_path = std::env::temp_dir().join(&file_name);
    std::fs::write(&pdf_path, pdf_bytes)?;

    let pdf_str = pdf_path.to_string_lossy().replace('\'', "''");
    let pr_str = printer_name.trim().replace('\'', "''");

    let script = format!(
        "$file='{pdf}'; $pr='{pr}'; \
         $sh=New-Object -ComObject Shell.Application; \
         $d=[IO.Path]::GetDirectoryName($file); \
         $n=[IO.Path]::GetFileName($file); \
         $item=$sh.NameSpace($d).ParseName($n); \
         if ($item) {{ $item.InvokeVerbEx('printto',$pr) }} \
         else {{ Start-Process -FilePath $file -Verb PrintTo -ArgumentList $pr }}; \
         Start-Sleep 6",
        pdf = pdf_str,
        pr = pr_str
    );

    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script])
        .status()?;

    let _ = std::fs::remove_file(&pdf_path);

    if !status.success() {
        anyhow::bail!(
            "Printimi dështoi (kod {:?}). Kontrolloni emrin e printerit te Cilësimet.",
            status.code()
        );
    }
    Ok(())
}

#[tauri::command]
async fn get_app_info(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<AppInfo, String> {
    let version = app.package_info().version.to_string();
    let ui_version =
        crate::updates::current_ui_version(&app).unwrap_or_else(|_| "seed".to_string());
    let sync_status = state.sync.get_status().await;
    let lic = state.license.get_state().await;
    let now_iso = crate::util::now_iso();
    let license_seconds_left = lic.active_until.as_deref().and_then(|x| {
        let until = crate::util::parse_rfc3339_to_utc(x).ok()?;
        let now = crate::util::parse_rfc3339_to_utc(&now_iso).ok()?;
        let secs = (until - now).num_seconds();
        Some(secs.max(0))
    });

    let db = state.db.clone();
    let current_version = version.clone();
    let (last_sync_time, desktop_update_forced, desktop_update_latest_version, desktop_update_force_deadline_at, desktop_update_last_manual_check_at) =
    tokio::task::spawn_blocking(move || -> anyhow::Result<(Option<String>, bool, Option<String>, Option<String>, Option<String>)> {
      let last_sync_time = db.get_last_sync_time()?;
      let (forced, latest, deadline, last_manual) = compute_desktop_update_policy_from_db(&db, &current_version)?;
      Ok((last_sync_time, forced, latest, deadline, last_manual))
    })
    .await
    .map_err(err_string)?
    .map_err(err_string)?;

    Ok(AppInfo {
        version,
        ui_version,
        sync_status: if state.is_demo_mode { "synced".to_string() } else { sync_status.sync_status },
        last_sync_time,
        last_sync_error: sync_status.last_sync_error,
        license_ok: state.is_demo_mode || lic.ok,
        license_status: if state.is_demo_mode { "ok".to_string() } else { lic.status },
        license_active_until: lic.active_until,
        license_last_checked_at: lic.last_checked_at,
        license_seconds_left,
        desktop_update_forced,
        desktop_update_latest_version,
        desktop_update_force_deadline_at,
        desktop_update_last_manual_check_at,
        is_demo_mode: state.is_demo_mode,
        capabilities: crate::sef_bridge::capabilities(),
    })
}

#[tauri::command]
async fn auth_get_state(state: tauri::State<'_, AppState>) -> Result<AuthStateInfo, String> {
    // Revalidate persisted session expiry on every state refresh instead of
    // trusting an indefinitely cached in-memory role.
    let db = state.db.clone();
    let session = tokio::task::spawn_blocking(move || crate::auth::session_get(&db))
        .await
        .map_err(err_string)?
        .map_err(err_string)?;
    state.auth.set_session(session.clone()).await;
    if matches!(session, SessionKind::None) {
        state.auth.admin_lock().await;
    }
    let admin_unlocked = state.auth.is_admin_unlocked().await;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || crate::auth::read_state(&db, admin_unlocked, session))
        .await
        .map_err(err_string)?
        .map_err(err_string)
}

#[tauri::command]
async fn auth_setup(
    state: tauri::State<'_, AppState>,
    clinic_name: String,
    admin_password: String,
    user_password: String,
    cashier_password: Option<String>,
) -> Result<AuthStateInfo, String> {
    let db = state.db.clone();
    let out = tokio::task::spawn_blocking(move || {
        crate::auth::setup_v2(
            &db,
            &clinic_name,
            &admin_password,
            &user_password,
            cashier_password.as_deref(),
        )
    })
    .await
    .map_err(err_string)?
    .map_err(err_string)?;

    let _ = state.license.check_now().await;
    Ok(out)
}

const DEMO_MODE_TOKEN: &str = "DEMOPROGRAMERI";

#[tauri::command]
async fn provision_apply_token(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    token: String,
) -> Result<AuthStateInfo, String> {
    let token_code = normalize_provision_token(&token);
    if token_code.is_empty() {
        return Err("tokeni eshte i detyrueshem".to_string());
    }

    // "Demo Mode" (vetem mobile, per kerkesen e App Review 2.1(a)): token special
    // qe s'aktivizon nje klinike reale - shkruan nje marker lokal dhe rinis app-in;
    // ne startup-in tjeter, run() e sheh markerin dhe hap baze te dhenash ne memorie.
    #[cfg(target_os = "android")]
    if token_code == DEMO_MODE_TOKEN {
        let data_dir = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("app data dir: {e}"))?;
        std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
        std::fs::write(data_dir.join("demo_mode.flag"), b"1").map_err(|e| e.to_string())?;
        app.restart();
    }

    // iOS s'ka asnje API per nje app qe ta rinise veten (ndryshe nga desktop/Android) - app.restart()
    // aty aborton (SIGABRT), gje qe App Review e regjistroi si "app crashed" (Guideline 2.1(a),
    // refuzuar 2026-08-20, submission f6dc766c). Flag-u shkruhet ne disk normalisht (run() e ardhshme
    // e sheh dhe hap demo mode automatikisht); ne vend te restart() thjesht i kerkojme rishikuesit/
    // perdoruesit ta mbylle plotesisht app-in dhe ta rihape vete, pa asnje crash apo exit i papritur.
    #[cfg(target_os = "ios")]
    if token_code == DEMO_MODE_TOKEN {
        let data_dir = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("app data dir: {e}"))?;
        std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
        std::fs::write(data_dir.join("demo_mode.flag"), b"1").map_err(|e| e.to_string())?;
        return Err(
            "Demo mode activated. Please fully close this app (swipe it away in the app switcher) and reopen it to continue."
                .to_string(),
        );
    }

    let db = state.db.clone();
    let already_configured = tokio::task::spawn_blocking(move || crate::auth::is_configured(&db))
        .await
        .map_err(err_string)?
        .map_err(err_string)?;
    if already_configured {
        return Err("aplikacioni eshte konfiguruar tashme".to_string());
    }

    let db = state.db.clone();
    let (supabase_url, api_key) =
        tokio::task::spawn_blocking(move || -> anyhow::Result<(String, String)> {
            let mut supabase_url = db.setting_get(KEY_SUPABASE_URL)?.unwrap_or_default();
            // Auto-fix typoed URL if it was saved in DB from a previous run
            if supabase_url == "https://occzpryzxabajtmdaas.supabase.co" {
                supabase_url = String::new();
            }
            if supabase_url.trim().is_empty() && !DEFAULT_SUPABASE_URL.trim().is_empty() {
                db.setting_set(KEY_SUPABASE_URL, DEFAULT_SUPABASE_URL)?;
                supabase_url = DEFAULT_SUPABASE_URL.to_string();
            }

            let mut api_key = db
                .setting_get(KEY_SUPABASE_API_KEY)?
                .or_else(|| db.setting_get(KEY_SUPABASE_ANON_KEY).ok().flatten())
                .unwrap_or_default();
            if api_key.trim().is_empty() && !DEFAULT_SUPABASE_API_KEY.trim().is_empty() {
                db.setting_set(KEY_SUPABASE_API_KEY, DEFAULT_SUPABASE_API_KEY)?;
                db.setting_set(KEY_SUPABASE_ANON_KEY, DEFAULT_SUPABASE_API_KEY)?;
                api_key = DEFAULT_SUPABASE_API_KEY.to_string();
            }

            if supabase_url.trim().is_empty() || api_key.trim().is_empty() {
                bail!("mungon konfigurimi i Supabase URL/Key");
            }
            Ok((supabase_url, api_key))
        })
        .await
        .map_err(err_string)?
        .map_err(err_string)?;

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(err_string)?;
    let base = supabase_url.trim_end_matches('/').to_string();
    let parsed_base =
        reqwest::Url::parse(&base).map_err(|e| format!("Supabase URL jo valid: {}", e))?;
    let host = parsed_base.host_str().unwrap_or("").trim().to_string();
    let port = parsed_base.port_or_known_default().unwrap_or(443);
    if host.is_empty() {
        return Err("Supabase URL jo valid: mungon host".to_string());
    }
    if format!("{}:{}", host, port).to_socket_addrs().is_err() {
        return Err(format!(
            "token check network error: DNS dështoi për host '{}'. Kontrollo internetin/DNS dhe Supabase URL.",
            host
        ));
    }
    let token_url = format!("{base}/rest/v1/clinic_tokens");
    let now = crate::util::now_iso();
    let token_eq = format!("eq.{}", token_code);
    let mut token_resp_opt: Option<reqwest::Response> = None;
    let mut token_last_err = String::new();
    for attempt in 1..=3_u64 {
        match with_supabase_auth_request(client.get(&token_url), &api_key)
            .query(&[
                (
                    "select",
                    "token_code,clinic_id,clinic_name,mode,one_time,disabled,expires_at,used_at,bootstrap_admin_salt,bootstrap_admin_hash,bootstrap_user_salt,bootstrap_user_hash,bootstrap_cashier_salt,bootstrap_cashier_hash",
                ),
                ("token_code", &token_eq),
                ("limit", "1"),
            ])
            .send()
            .await
        {
            Ok(resp) => {
                token_resp_opt = Some(resp);
                break;
            }
            Err(e) => {
                token_last_err = crate::util::reqwest_error_detailed(&e);
                if crate::util::is_network_error(&e) && attempt < 3 {
                    tokio::time::sleep(Duration::from_millis(700 * attempt)).await;
                    continue;
                }
                return Err(format!(
                    "token check network error: {}. Kontrollo internetin, daten/oren e Windows-it, firewall/proxy dhe Supabase URL/API key.",
                    token_last_err
                ));
            }
        }
    }
    let Some(resp) = token_resp_opt else {
        return Err(format!(
            "token check network error: {}. Kontrollo internetin, daten/oren e Windows-it, firewall/proxy dhe Supabase URL/API key.",
            token_last_err
        ));
    };
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("token check failed: {} {}", status, body));
    }
    let mut rows: Vec<ProvisionTokenRow> = serde_json::from_str(&body).map_err(err_string)?;
    if rows.is_empty() {
        return Err("tokeni nuk u gjet".to_string());
    }
    let row = rows.remove(0);

    if row.disabled.unwrap_or(false) {
        return Err("tokeni eshte i çaktivizuar".to_string());
    }
    if row.one_time.unwrap_or(true) && row.used_at.as_deref().unwrap_or("").trim().len() > 0 {
        return Err("tokeni eshte perdorur me pare".to_string());
    }
    if let Some(exp) = row
        .expires_at
        .as_deref()
        .map(str::trim)
        .filter(|x| !x.is_empty())
    {
        let now_dt = crate::util::parse_rfc3339_to_utc(&now).map_err(err_string)?;
        let exp_dt = crate::util::parse_rfc3339_to_utc(exp).map_err(err_string)?;
        if now_dt > exp_dt {
            return Err("tokeni ka skaduar".to_string());
        }
    }

    let mut clinic_id = row.clinic_id.as_deref().unwrap_or("").trim().to_string();
    let mut clinic_name = row.clinic_name.as_deref().unwrap_or("").trim().to_string();
    let mode = normalize_token_mode(row.mode.as_deref(), !clinic_id.is_empty());
    if clinic_id.is_empty() {
        clinic_id = uuid::Uuid::new_v4().to_string();
    }

    if clinic_name.is_empty() {
        let registry_url = format!("{base}/rest/v1/clinic_registry");
        let clinic_eq = format!("eq.{}", clinic_id);
        if let Ok(resp) = with_supabase_auth_request(client.get(&registry_url), &api_key)
            .query(&[
                ("select", "clinic_name"),
                ("clinic_id", &clinic_eq),
                ("limit", "1"),
            ])
            .send()
            .await
        {
            if resp.status().is_success() {
                let txt = resp.text().await.unwrap_or_default();
                if let Ok(mut xs) = serde_json::from_str::<Vec<ProvisionClinicNameRow>>(&txt) {
                    if let Some(x) = xs.pop() {
                        let n = x.clinic_name.unwrap_or_default();
                        if !n.trim().is_empty() {
                            clinic_name = n.trim().to_string();
                        }
                    }
                }
            }
        }
    }
    if clinic_name.is_empty() {
        clinic_name = "Klinika".to_string();
    }

    let mut patch = serde_json::json!({
      "clinic_id": clinic_id,
      "clinic_name": clinic_name,
      "used_at": now,
      "updated_at": now
    });
    if row.one_time.unwrap_or(true) {
        patch["disabled"] = serde_json::Value::Bool(true);
    }
    let patch_url = format!(
        "{token_url}?token_code=eq.{}",
        urlencoding::encode(&token_code)
    );
    let mut patch_resp_opt: Option<reqwest::Response> = None;
    let mut patch_last_err = String::new();
    for attempt in 1..=3_u64 {
        match with_supabase_auth_request(client.patch(&patch_url), &api_key)
            .header("Prefer", "return=minimal")
            .json(&patch)
            .send()
            .await
        {
            Ok(resp) => {
                patch_resp_opt = Some(resp);
                break;
            }
            Err(e) => {
                patch_last_err = crate::util::reqwest_error_detailed(&e);
                if crate::util::is_network_error(&e) && attempt < 3 {
                    tokio::time::sleep(Duration::from_millis(700 * attempt)).await;
                    continue;
                }
                return Err(format!(
                    "token update network error: {}. Kontrollo internetin, firewall/proxy dhe provo perseri.",
                    patch_last_err
                ));
            }
        }
    }
    let Some(patch_resp) = patch_resp_opt else {
        return Err(format!(
            "token update network error: {}. Kontrollo internetin, firewall/proxy dhe provo perseri.",
            patch_last_err
        ));
    };
    if !patch_resp.status().is_success() {
        let st = patch_resp.status();
        let txt = patch_resp.text().await.unwrap_or_default();
        return Err(format!("token update failed: {} {}", st, txt));
    }

    let out = if mode == TOKEN_MODE_EXISTING {
        let admin_salt = row
            .bootstrap_admin_salt
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_string();
        let admin_hash = row
            .bootstrap_admin_hash
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_string();
        let user_salt = row
            .bootstrap_user_salt
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_string();
        let user_hash = row
            .bootstrap_user_hash
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_string();
        if admin_salt.is_empty()
            || admin_hash.is_empty()
            || user_salt.is_empty()
            || user_hash.is_empty()
        {
            return Err(
                "tokeni i klinikes ekzistuese nuk ka kredenciale bootstrap (owner/admin)."
                    .to_string(),
            );
        }
        let cashier_salt = row
            .bootstrap_cashier_salt
            .as_deref()
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .map(str::to_string);
        let cashier_hash = row
            .bootstrap_cashier_hash
            .as_deref()
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .map(str::to_string);

        let db = state.db.clone();
        let clinic_id2 = clinic_id.clone();
        let clinic_name2 = clinic_name.clone();
        tokio::task::spawn_blocking(move || {
            crate::auth::provision_existing(
                &db,
                &clinic_id2,
                &clinic_name2,
                &admin_salt,
                &admin_hash,
                &user_salt,
                &user_hash,
                cashier_salt.as_deref(),
                cashier_hash.as_deref(),
            )
        })
        .await
        .map_err(err_string)?
        .map_err(err_string)?
    } else {
        let db = state.db.clone();
        let clinic_id2 = clinic_id.clone();
        let clinic_name2 = clinic_name.clone();
        tokio::task::spawn_blocking(move || {
            crate::auth::provision_new(&db, &clinic_id2, &clinic_name2)
        })
        .await
        .map_err(err_string)?
        .map_err(err_string)?
    };

    state.auth.set_session(SessionKind::None).await;
    state.auth.admin_lock().await;
    let _ = state.license.check_now().await;
    Ok(out)
}

#[tauri::command]
async fn auth_admin_unlock(
    state: tauri::State<'_, AppState>,
    password: String,
) -> Result<bool, String> {
    let db = state.db.clone();
    let ok = tokio::task::spawn_blocking(move || crate::auth::admin_verify(&db, &password))
        .await
        .map_err(err_string)?
        .map_err(err_string)?;
    if ok {
        state.auth.admin_unlock().await;
        Ok(true)
    } else {
        Ok(false)
    }
}

#[tauri::command]
async fn auth_admin_lock(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.auth.admin_lock().await;
    Ok(())
}

#[tauri::command]
async fn auth_admin_change_password(
    state: tauri::State<'_, AppState>,
    new_password: String,
) -> Result<(), String> {
    let _ = require_login(&state).await?;
    if !state.auth.is_admin_unlocked().await {
        return Err("kjo veprim kerkon hyrje si admin".to_string());
    }
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || crate::auth::admin_change_password(&db, &new_password))
        .await
        .map_err(err_string)?
        .map_err(err_string)?;
    state.auth.admin_lock().await;
    Ok(())
}

#[tauri::command]
async fn auth_user_login(
    state: tauri::State<'_, AppState>,
    password: String,
) -> Result<bool, String> {
    let db = state.db.clone();
    let role = tokio::task::spawn_blocking(move || crate::auth::user_verify_role(&db, &password))
        .await
        .map_err(err_string)?
        .map_err(err_string)?;
    let Some(role) = role else {
        return Ok(false);
    };

    let db2 = state.db.clone();
    match role {
        crate::auth::UserRole::Owner => {
            tokio::task::spawn_blocking(move || crate::auth::session_set_user(&db2))
                .await
                .map_err(err_string)?
                .map_err(err_string)?;
            state
                .auth
                .set_session(SessionKind::User {
                    role: crate::auth::UserRole::Owner,
                })
                .await;
        }
        crate::auth::UserRole::Cashier => {
            tokio::task::spawn_blocking(move || crate::auth::session_set_cashier(&db2))
                .await
                .map_err(err_string)?
                .map_err(err_string)?;
            state
                .auth
                .set_session(SessionKind::User {
                    role: crate::auth::UserRole::Cashier,
                })
                .await;
        }
        crate::auth::UserRole::LogsAdmin => {
            tokio::task::spawn_blocking(move || crate::auth::session_set_logs_admin(&db2))
                .await
                .map_err(err_string)?
                .map_err(err_string)?;
            state
                .auth
                .set_session(SessionKind::User {
                    role: crate::auth::UserRole::LogsAdmin,
                })
                .await;
        }
    }

    Ok(true)
}

// Legacy standalone logs-admin password login is intentionally disabled.
// Logs administrators must authenticate through the existing role-based user login.
// Never reintroduce a shared credential compiled into the executable.
#[tauri::command]
async fn auth_logs_admin_login(
    state: tauri::State<'_, AppState>,
    _username: String,
    _password: String,
) -> Result<bool, String> {
    append_error_log(
        state.inner(),
        "auth_logs_admin_login",
        "legacy logs-admin login rejected",
    );
    Ok(false)
}

#[tauri::command]
async fn auth_user_logout(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || crate::auth::session_clear(&db))
        .await
        .map_err(err_string)?
        .map_err(err_string)?;
    state.auth.set_session(SessionKind::None).await;
    state.auth.admin_lock().await;
    Ok(())
}

#[tauri::command]
async fn doctors_login_options(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<DoctorLoginOption>, String> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.doctors_login_options())
        .await
        .map_err(err_string)?
        .map_err(err_string)
}

#[tauri::command]
async fn auth_doctor_login(
    state: tauri::State<'_, AppState>,
    doctor_id: String,
    password: String,
) -> Result<bool, String> {
    // Support logging in by either doctor UUID or a short code.
    let key = doctor_id.trim().to_string();
    if key.is_empty() {
        return Err("doctor_id eshte i detyrueshem".to_string());
    }

    let db = state.db.clone();
    let resolved = tokio::task::spawn_blocking(move || db.doctor_id_from_code_or_id(&key))
        .await
        .map_err(err_string)?
        .map_err(err_string)?;
    let Some(did) = resolved else {
        return Err("mjeku nuk u gjet".to_string());
    };

    let db2 = state.db.clone();
    let did2 = did.clone();
    let pw2 = password.clone();
    let res = tokio::task::spawn_blocking(move || crate::auth::doctor_verify(&db2, &did2, &pw2))
        .await
        .map_err(err_string)?
        .map_err(err_string)?;

    match res {
        crate::auth::DoctorVerify::NoAccount => {
            Err("ky mjek nuk ka login. kontakto administratorin.".to_string())
        }
        crate::auth::DoctorVerify::WrongPassword => Ok(false),
        crate::auth::DoctorVerify::Ok { .. } => {
            let db2 = state.db.clone();
            let session =
                tokio::task::spawn_blocking(move || crate::auth::session_set_doctor(&db2, &did))
                    .await
                    .map_err(err_string)?
                    .map_err(err_string)?;
            state.auth.set_session(session).await;
            Ok(true)
        }
    }
}

#[tauri::command]
async fn auth_doctor_logout(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || crate::auth::session_clear(&db))
        .await
        .map_err(err_string)?
        .map_err(err_string)?;
    state.auth.set_session(SessionKind::None).await;
    state.auth.admin_lock().await;
    Ok(())
}

#[tauri::command]
async fn doctor_account_update(
    state: tauri::State<'_, AppState>,
    doctor_id: String,
    password: Option<String>,
    is_admin: bool,
) -> Result<(), String> {
    let _ = require_owner(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        crate::auth::doctor_account_update(&db, &doctor_id, password.as_deref(), is_admin)
    })
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

async fn require_login(state: &tauri::State<'_, AppState>) -> Result<SessionKind, String> {
    // Recheck expiry and account validity at the command authorization boundary.
    let db = state.db.clone();
    let s = tokio::task::spawn_blocking(move || crate::auth::session_get(&db))
        .await
        .map_err(err_string)?
        .map_err(err_string)?;
    if matches!(s, SessionKind::None) {
        state.auth.set_session(SessionKind::None).await;
        state.auth.admin_lock().await;
    }
    match s {
        SessionKind::None => Err("duhet te hysh per te vazhduar".to_string()),
        SessionKind::User {
            role: crate::auth::UserRole::LogsAdmin,
        } => Ok(s),
        _ => {
            if !state.license.is_ok().await {
                return Err("licenca skadoi ose eshte e bllokuar".to_string());
            }
            Ok(s)
        }
    }
}

async fn require_finance(state: &tauri::State<'_, AppState>) -> Result<SessionKind, String> {
    let s = require_login(state).await?;
    match &s {
        SessionKind::User {
            role: crate::auth::UserRole::Owner,
        } => Ok(s),
        SessionKind::User {
            role: crate::auth::UserRole::Cashier,
        } => Ok(s),
        SessionKind::Doctor { is_admin: true, .. } => Ok(s),
        _ => Err("nuk ke akses per kete seksion".to_string()),
    }
}

async fn require_logs_admin(state: &tauri::State<'_, AppState>) -> Result<(), String> {
    // The legacy logs-admin login is disabled; never trust an in-memory role.
    let db = state.db.clone();
    let s = tokio::task::spawn_blocking(move || crate::auth::session_get(&db))
        .await
        .map_err(err_string)?
        .map_err(err_string)?;
    match s {
        SessionKind::User {
            role: crate::auth::UserRole::LogsAdmin,
        } => Ok(()),
        _ => Err("nuk ke akses ne error logs".to_string()),
    }
}

async fn require_owner(state: &tauri::State<'_, AppState>) -> Result<SessionKind, String> {
    let s = require_login(state).await?;
    match &s {
        SessionKind::User {
            role: crate::auth::UserRole::Owner,
        } => Ok(s),
        SessionKind::Doctor { is_admin: true, .. } => Ok(s),
        _ => Err("nuk ke akses per kete seksion".to_string()),
    }
}

#[tauri::command]
async fn settings_get_all(
    state: tauri::State<'_, AppState>,
) -> Result<HashMap<String, String>, String> {
    let _ = require_login(&state).await?;
    let db = state.db.clone();
    let is_admin = state.auth.is_admin_unlocked().await;
    let mut map = tokio::task::spawn_blocking(move || db.settings_get_all())
        .await
        .map_err(err_string)?
        .map_err(err_string)?;

    // Hide sensitive settings from non-admin users.
    if !is_admin {
        for k in [
            "supabase_url",
            "supabase_api_key",
            "supabase_anon_key",
            "update_base_url",
            "desktop_update_api",
            "admin_salt",
            "admin_hash",
            "user_salt",
            "user_hash",
            "cashier_salt",
            "cashier_hash",
            "user_logged_in",
            "session",
            "session_expires_at",
        ] {
            map.remove(k);
        }
    }

    Ok(map)
}

/// Clinical data is not available to cashier or logs-only accounts.
async fn require_orthopedic_clinician(
    state: &tauri::State<'_, AppState>,
) -> Result<SessionKind, String> {
    let session = require_login(state).await?;
    match session {
        SessionKind::Doctor { .. } |
        SessionKind::User { role: crate::auth::UserRole::Owner } => Ok(session),
        _ => Err("Kërkohet rol klinik për të dhënat ortopedike.".to_string()),
    }
}

/// Only a doctor may authorize actual surgery or implantation; a clinic
/// administrator can manage clinic settings but cannot certify procedures.
async fn require_orthopedic_surgeon(
    state: &tauri::State<'_, AppState>,
) -> Result<SessionKind, String> {
    let session = require_orthopedic_clinician(state).await?;
    match session {
        SessionKind::Doctor { .. } => Ok(session),
        _ => Err("Vetëm mjeku i identifikuar mund të finalizojë ndërhyrje/implante.".to_string()),
    }
}

#[tauri::command]
async fn orthopedic_episodes_list(state: tauri::State<'_, AppState>, client_id: Option<String>) -> Result<Vec<serde_json::Value>, String> {
    let _ = require_orthopedic_clinician(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.orthopedic_episodes_list(client_id)).await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn orthopedic_episode_upsert(state: tauri::State<'_, AppState>, episode: serde_json::Value) -> Result<(), String> {
    let _ = require_orthopedic_clinician(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.orthopedic_episode_upsert(&episode)).await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn orthopedic_episode_transition(
    state: tauri::State<'_, AppState>, episode_id: String,
    action: String, reason: String,
) -> Result<serde_json::Value, String> {
    let doctor = match require_orthopedic_surgeon(&state).await? {
        SessionKind::Doctor { doctor_id, .. } => doctor_id,
        _ => return Err("Kërkohet mjeku i identifikuar.".to_string()),
    };
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.orthopedic_episode_transition(
        &episode_id, &action, &reason, &doctor,
    )).await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn orthopedic_episode_transitions_list(
    state: tauri::State<'_, AppState>, episode_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    let _ = require_orthopedic_clinician(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.orthopedic_episode_transitions_list(&episode_id))
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn orthopedic_surgery_upsert(state: tauri::State<'_, AppState>, value: serde_json::Value) -> Result<(), String> {
    let status = value["status"].as_str().unwrap_or("draft");
    if matches!(status, "approved" | "scheduled" | "ready" | "in_progress" | "completed") {
        let _ = require_orthopedic_surgeon(&state).await?;
    } else {
        let _ = require_orthopedic_clinician(&state).await?;
    } let db=state.db.clone(); tokio::task::spawn_blocking(move||{db.orthopedic_surgery_upsert(&value)?; db.orthopedic_audit("surgery",value["id"].as_str().unwrap_or_default(),&value)}).await.map_err(err_string)?.map_err(err_string) }
#[tauri::command]
async fn orthopedic_implant_upsert(state: tauri::State<'_, AppState>, value: serde_json::Value) -> Result<(), String> {
    let session = if value["status"].as_str() == Some("implanted")
       || value["removal_status"].as_str() == Some("removed") {
        require_orthopedic_surgeon(&state).await?
    } else {
        require_orthopedic_clinician(&state).await?
    };
    let actor = match session {
        SessionKind::Doctor { doctor_id, .. } => doctor_id,
        SessionKind::User { role: crate::auth::UserRole::Owner } => "clinic_owner".to_string(),
        _ => return Err("Nuk ke autorizim për implante.".to_string()),
    };
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        db.orthopedic_implant_upsert(&value, &actor)?;
        db.orthopedic_audit("implant", value["id"].as_str().unwrap_or_default(), &value)
    }).await.map_err(err_string)?.map_err(err_string)
}
#[tauri::command]
async fn orthopedic_rehab_upsert(state: tauri::State<'_, AppState>, value: serde_json::Value) -> Result<(), String> { let _=require_orthopedic_clinician(&state).await?; let db=state.db.clone(); tokio::task::spawn_blocking(move||{db.orthopedic_rehab_upsert(&value)?; db.orthopedic_audit("rehab",value["id"].as_str().unwrap_or_default(),&value)}).await.map_err(err_string)?.map_err(err_string) }

#[tauri::command]
async fn orthopedic_fracture_followups_list(
    state: tauri::State<'_, AppState>, fracture_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    let _ = require_orthopedic_clinician(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.orthopedic_fracture_followups_list(&fracture_id))
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn orthopedic_fracture_followup_add(
    state: tauri::State<'_, AppState>, value: serde_json::Value,
) -> Result<(), String> {
    let surgeon_id = match require_orthopedic_surgeon(&state).await? {
        SessionKind::Doctor { doctor_id, .. } => doctor_id,
        _ => return Err("Kërkohet mjeku.".to_string()),
    };
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.orthopedic_fracture_followup_add(&value, &surgeon_id))
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn orthopedic_operations_list(
    state: tauri::State<'_, AppState>, episode_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    let _ = require_orthopedic_clinician(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.orthopedic_operations_list(&episode_id))
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn orthopedic_operation_finalize(
    state: tauri::State<'_, AppState>, value: serde_json::Value,
) -> Result<(), String> {
    let surgeon_id = match require_orthopedic_surgeon(&state).await? {
        SessionKind::Doctor { doctor_id, .. } => doctor_id,
        _ => return Err("Kërkohet kirurgu.".to_string()),
    };
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.orthopedic_operation_finalize(&value, &surgeon_id))
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn orthopedic_surgeries_list(state: tauri::State<'_, AppState>, episode_id: String) -> Result<Vec<serde_json::Value>, String> {
    let _ = require_orthopedic_clinician(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.orthopedic_surgeries_list(&episode_id)).await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn orthopedic_implants_list(state: tauri::State<'_, AppState>, episode_id: String) -> Result<Vec<serde_json::Value>, String> {
    let _ = require_orthopedic_clinician(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.orthopedic_implants_list(&episode_id)).await.map_err(err_string)?.map_err(err_string)
}

/// An authenticated doctor links an existing orthopedic rehabilitation
/// protocol to the shared Physiotherapy core. The patient ID is derived by SQL.
#[tauri::command]
async fn physiotherapy_referral_create(
    state: tauri::State<'_, AppState>,
    orthopedic_episode_id: String, rehab_plan_id: String,
) -> Result<serde_json::Value, String> {
    let actor=match require_orthopedic_surgeon(&state).await? {
        SessionKind::Doctor { doctor_id, .. } => doctor_id,
        _ => return Err("Kërkohet identiteti i mjekut.".into()),
    };
    let db=state.db.clone();
    tokio::task::spawn_blocking(move || db.physiotherapy_referral_create(
        &orthopedic_episode_id,&rehab_plan_id,&actor
    )).await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn physiotherapy_episodes_list(
    state: tauri::State<'_, AppState>, orthopedic_episode_id: Option<String>,
) -> Result<Vec<serde_json::Value>, String> {
    let _=require_orthopedic_clinician(&state).await?;
    let db=state.db.clone();
    tokio::task::spawn_blocking(move || db.physiotherapy_episodes_list(orthopedic_episode_id))
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn physiotherapy_sessions_list(
    state: tauri::State<'_, AppState>, physiotherapy_episode_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    let _=require_orthopedic_clinician(&state).await?;
    let db=state.db.clone();
    tokio::task::spawn_blocking(move || db.physiotherapy_sessions_list(
        &physiotherapy_episode_id
    )).await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn physiotherapy_session_add(
    state: tauri::State<'_, AppState>, value: serde_json::Value,
) -> Result<serde_json::Value, String> {
    // Only authenticated Doctor sessions may document treatment in the
    // current auth model. A separate physiotherapist role is future work.
    let actor=match require_orthopedic_surgeon(&state).await? {
        SessionKind::Doctor { doctor_id, .. } => doctor_id,
        _ => return Err("Kërkohet profesionist klinik i identifikuar.".into()),
    };
    let db=state.db.clone();
    tokio::task::spawn_blocking(move || db.physiotherapy_session_add(&value,&actor))
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn physiotherapy_measurements_list(state: tauri::State<'_, AppState>, physiotherapy_episode_id: String)
    -> Result<Vec<serde_json::Value>, String> {
    let _=require_orthopedic_surgeon(&state).await?;
    let db=state.db.clone();
    tokio::task::spawn_blocking(move || db.physiotherapy_measurements_list(&physiotherapy_episode_id))
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn physiotherapy_measurement_add(state: tauri::State<'_, AppState>, value: serde_json::Value)
    -> Result<serde_json::Value, String> {
    let doctor=match require_orthopedic_surgeon(&state).await? {
        SessionKind::Doctor { doctor_id, .. }=>doctor_id,
        _=>return Err("Kërkohet përdorues klinik i identifikuar.".to_string()),
    };
    let db=state.db.clone();
    tokio::task::spawn_blocking(move || db.physiotherapy_measurement_add(&value,&doctor))
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn physiotherapy_care_plans_list(state: tauri::State<'_, AppState>, physiotherapy_episode_id: String)
    -> Result<Vec<serde_json::Value>, String> {
    let _=require_orthopedic_surgeon(&state).await?;
    let db=state.db.clone();
    tokio::task::spawn_blocking(move || db.physiotherapy_care_plans_list(&physiotherapy_episode_id))
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn physiotherapy_care_plan_create(state: tauri::State<'_, AppState>, value: serde_json::Value)
    -> Result<serde_json::Value, String> {
    let doctor=match require_orthopedic_surgeon(&state).await? {
        SessionKind::Doctor { doctor_id, .. }=>doctor_id,
        _=>return Err("Kërkohet përdorues klinik i identifikuar.".to_string()),
    };
    let db=state.db.clone();
    tokio::task::spawn_blocking(move || db.physiotherapy_care_plan_create(&value,&doctor))
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn physiotherapy_exercises_list(state: tauri::State<'_, AppState>)
    -> Result<Vec<serde_json::Value>, String> {
    let _=require_orthopedic_surgeon(&state).await?;
    let db=state.db.clone();
    tokio::task::spawn_blocking(move || db.physiotherapy_exercises_list())
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn physiotherapy_exercise_create(state: tauri::State<'_, AppState>, value: serde_json::Value)
    -> Result<serde_json::Value, String> {
    let doctor=match require_orthopedic_surgeon(&state).await? {
        SessionKind::Doctor { doctor_id, .. }=>doctor_id,
        _=>return Err("Kërkohet përdorues klinik i identifikuar.".to_string()),
    };
    let db=state.db.clone();
    tokio::task::spawn_blocking(move || db.physiotherapy_exercise_create(&value,&doctor))
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn physiotherapy_hep_list(state: tauri::State<'_, AppState>, physiotherapy_episode_id: String)
    -> Result<Vec<serde_json::Value>, String> {
    let _=require_orthopedic_surgeon(&state).await?;
    let db=state.db.clone();
    tokio::task::spawn_blocking(move || db.physiotherapy_hep_list(&physiotherapy_episode_id))
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn physiotherapy_hep_assign(state: tauri::State<'_, AppState>, value: serde_json::Value)
    -> Result<serde_json::Value, String> {
    let doctor=match require_orthopedic_surgeon(&state).await? {
        SessionKind::Doctor { doctor_id, .. }=>doctor_id,
        _=>return Err("Kërkohet përdorues klinik i identifikuar.".to_string()),
    };
    let db=state.db.clone();
    tokio::task::spawn_blocking(move || db.physiotherapy_hep_assign(&value,&doctor))
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn physiotherapy_hep_logs_list(
    state: tauri::State<'_, AppState>,physiotherapy_episode_id:String,
)->Result<Vec<serde_json::Value>,String>{
    let _=require_orthopedic_surgeon(&state).await?;
    let db=state.db.clone();
    tokio::task::spawn_blocking(
        move || db.physiotherapy_hep_logs_list(&physiotherapy_episode_id)
    ).await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn physiotherapy_hep_log_add(state: tauri::State<'_, AppState>, value: serde_json::Value)
    -> Result<serde_json::Value, String> {
    let doctor=match require_orthopedic_surgeon(&state).await? {
        SessionKind::Doctor { doctor_id, .. }=>doctor_id,
        _=>return Err("Kërkohet përdorues klinik i identifikuar.".to_string()),
    };
    let db=state.db.clone();
    tokio::task::spawn_blocking(move || db.physiotherapy_hep_log_add(&value,&doctor))
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn physiotherapy_reports_list(state: tauri::State<'_, AppState>, physiotherapy_episode_id: String)
    -> Result<Vec<serde_json::Value>, String> {
    let _=require_orthopedic_surgeon(&state).await?;
    let db=state.db.clone();
    tokio::task::spawn_blocking(move || db.physiotherapy_reports_list(&physiotherapy_episode_id))
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn physiotherapy_report_create(state: tauri::State<'_, AppState>, value: serde_json::Value)
    -> Result<serde_json::Value, String> {
    let doctor=match require_orthopedic_surgeon(&state).await? {
        SessionKind::Doctor { doctor_id, .. }=>doctor_id,
        _=>return Err("Kërkohet përdorues klinik i identifikuar.".to_string()),
    };
    let db=state.db.clone();
    tokio::task::spawn_blocking(move || db.physiotherapy_report_create(&value,&doctor))
        .await.map_err(err_string)?.map_err(err_string)
}

/// Export a clinical referral or a specific immutable report version.
/// The receiving reader gets the actual PDF, not a rendered WebView screen.
#[tauri::command]
async fn physiotherapy_document_export_pdf(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    document_id: String, kind: String,
) -> Result<String, String> {
    let _=require_orthopedic_surgeon(&state).await?;
    if !matches!(kind.as_str(),"referral"|"report") {
        return Err("Zgjidh referimin ose raportin klinik.".to_string());
    }
    let db=state.db.clone();
    tokio::task::spawn_blocking(move || {
        let record=db.physiotherapy_document_data(&document_id,&kind)
            .map_err(err_string)?;
        let clinic=db.setting_get("clinic_name").map_err(err_string)?
            .unwrap_or_else(||"Klinika".into());
        let bytes=crate::physiotherapy_pdf::render(&record,&clinic,&kind)
            .map_err(err_string)?;
        let safe_id:String=document_id.chars()
            .filter(|c|c.is_ascii_alphanumeric()).take(12).collect();
        let name=if kind=="referral" {"Fizioterapi-Referim"}else{"Fizioterapi-Raport"};
        let file=format!("{}-{}.pdf",name,safe_id);
        #[cfg(mobile)]
        { return save_pdf_via_dialog(&app,&file,&bytes); }
        #[cfg(desktop)]
        {
            use tauri_plugin_dialog::DialogExt;
            let picked=app.dialog().file()
                .set_title("Ruaj dokumentin klinik PDF")
                .set_file_name(&file).add_filter("PDF",&["pdf"])
                .blocking_save_file()
                .ok_or_else(||"Ruajtja e dokumentit u anulua.".to_string())?;
            let path=picked.as_path().ok_or_else(
                ||"Përzgjidh një destinacion skedari.".to_string())?;
            std::fs::write(path,&bytes).map_err(err_string)?;
            #[cfg(target_os="windows")]
            {let _=std::process::Command::new("explorer.exe").arg(path).spawn();}
            Ok(path.display().to_string())
        }
    }).await.map_err(err_string)?
}

#[tauri::command]
async fn physiotherapy_documents_list(
    state: tauri::State<'_, AppState>, orthopedic_episode_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    let _=require_orthopedic_surgeon(&state).await?;
    let db=state.db.clone();
    tokio::task::spawn_blocking(move || db.physiotherapy_documents_list(&orthopedic_episode_id))
        .await.map_err(err_string)?.map_err(err_string)
}

/// Store a patient-linked clinical PDF through the native picker; never
/// send an OS path through the webview, and validate the file on the backend.
#[tauri::command]
async fn physiotherapy_document_attach(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    orthopedic_episode_id: String, category: String, source_note: String,
) -> Result<serde_json::Value, String> {
    let actor=match require_orthopedic_surgeon(&state).await? {
        SessionKind::Doctor { doctor_id, .. }=>doctor_id,
        _=>return Err("Kërkohet identiteti i profesionistit.".to_string()),
    };
    let db=state.db.clone();
    tokio::task::spawn_blocking(move || {
        #[cfg(mobile)]
        {let _=(app,db,orthopedic_episode_id,category,source_note,actor);
         Err("Importi i dokumentit kryhet në aplikacionin desktop.".into())}
        #[cfg(desktop)]
        {
            use tauri_plugin_dialog::DialogExt;
            use sha2::{Digest,Sha256};
            let file=app.dialog().file().set_title("Zgjidh raportin e rehabilitimit PDF")
                .add_filter("PDF",&["pdf"]).blocking_pick_file()
                .ok_or_else(||"Përzgjedhja e skedarit u anulua.".to_string())?;
            let selected=file.as_path().ok_or_else(
                ||"Zgjidh një skedar PDF të disponueshëm.".to_string())?;
            let metadata=std::fs::metadata(selected).map_err(err_string)?;
            if metadata.len()<100 || metadata.len()>20*1024*1024 {
                return Err("Dokumenti duhet të jetë PDF nën 20 MB.".into());
            }
            let bytes=std::fs::read(selected).map_err(err_string)?;
            if !bytes.starts_with(b"%PDF-") || !bytes.windows(5).any(|w|w==b"%%EOF") {
                return Err("Skedari i zgjedhur nuk është PDF i plotë.".into());
            }
            let digest=hex::encode(Sha256::digest(&bytes));
            let basename=selected.file_name().and_then(|x|x.to_str())
                .unwrap_or("Raport-Fizioterapie.pdf").to_string();
            let relative=format!("rehab-pdf/{}.pdf",uuid::Uuid::new_v4());
            let base=app.path().app_data_dir().map_err(err_string)?;
            let directory=base.join("rehab-pdf");
            std::fs::create_dir_all(&directory).map_err(err_string)?;
            let target=base.join(&relative);
            std::fs::write(&target,&bytes).map_err(err_string)?;
            match db.physiotherapy_document_register(
                &orthopedic_episode_id,&category,&basename,&relative,
                &digest,bytes.len() as i64,&source_note,&actor
            ){
                Ok(data)=>Ok(data),
                Err(e)=>{
                    let _=std::fs::remove_file(&target);
                    Err(err_string(e))
                }
            }
        }
    }).await.map_err(err_string)?
}

/// Resolves only locally managed PDF IDs; validates checksum before opening.
#[tauri::command]
async fn physiotherapy_document_open(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>, document_id: String,
) -> Result<String, String> {
    let _=require_orthopedic_surgeon(&state).await?;
    let db=state.db.clone();
    tokio::task::spawn_blocking(move || {
        #[cfg(mobile)]
        {let _=(app,db,document_id);
         Err("Hapja e PDF kryhet nga aplikacioni desktop.".into())}
        #[cfg(desktop)]
        {
            use sha2::{Digest,Sha256};
            let (relative,expected)=db.physiotherapy_document_file(&document_id)
                .map_err(err_string)?;
            let name=relative.strip_prefix("rehab-pdf/")
                .ok_or_else(||"Vendndodhje e pavlefshme e dokumentit.".to_string())?;
            if name.contains('/')||name.contains('\\')||name.contains("..")
                ||!name.ends_with(".pdf") {
                return Err("Skedari nuk është në dosjen klinike.".into());
            }
            let path=app.path().app_data_dir().map_err(err_string)?.join(&relative);
            let bytes=std::fs::read(&path).map_err(err_string)?;
            let actual=hex::encode(Sha256::digest(&bytes));
            if actual!=expected || !bytes.starts_with(b"%PDF-") {
                return Err("Integriteti i dokumentit klinik nuk përputhet.".into());
            }
            #[cfg(target_os="windows")]
            {std::process::Command::new("explorer.exe").arg(&path).spawn().map_err(err_string)?;}
            #[cfg(not(target_os="windows"))]
            {open::that(&path).map_err(err_string)?;}
            Ok("Dokumenti u hap në lexuesin PDF.".to_string())
        }
    }).await.map_err(err_string)?
}

#[tauri::command]
async fn physiotherapy_body_markers_list(
    state:tauri::State<'_,AppState>,physiotherapy_episode_id:String,
)->Result<Vec<serde_json::Value>,String>{
    let _=require_orthopedic_surgeon(&state).await?;
    let db=state.db.clone();
    tokio::task::spawn_blocking(
        move || db.physiotherapy_body_markers_list(&physiotherapy_episode_id)
    ).await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn physiotherapy_body_marker_add(
    state:tauri::State<'_,AppState>,value:serde_json::Value,
)->Result<serde_json::Value,String>{
    let actor=match require_orthopedic_surgeon(&state).await?{
        SessionKind::Doctor{doctor_id,..}=>doctor_id,
        _=>return Err("Kërkohet mjeku i identifikuar.".into())
    };
    let db=state.db.clone();
    tokio::task::spawn_blocking(
        move || db.physiotherapy_body_marker_add(&value,&actor)
    ).await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn orthopedic_rehab_plans_list(state: tauri::State<'_, AppState>, episode_id: String) -> Result<Vec<serde_json::Value>, String> {
    let _ = require_orthopedic_clinician(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.orthopedic_rehab_plans_list(&episode_id)).await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn orthopedic_rehab_phase_reviews_list(
    state: tauri::State<'_, AppState>, rehab_plan_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    let _ = require_orthopedic_clinician(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.orthopedic_rehab_phase_reviews_list(&rehab_plan_id))
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn orthopedic_rehab_phase_review_add(
    state: tauri::State<'_, AppState>, value: serde_json::Value,
) -> Result<(), String> {
    let clinician = match require_orthopedic_surgeon(&state).await? {
        SessionKind::Doctor { doctor_id, .. } => doctor_id,
        _ => return Err("Kërkohet mjeku.".to_string()),
    };
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.orthopedic_rehab_phase_review_add(&value,&clinician))
        .await.map_err(err_string)?.map_err(err_string)
}

/// Native A4 PDF export of an existing, immutable clinical report version.
/// The report/patient data come from SQLite, not from the webview DOM.
#[tauri::command]
async fn orthopedic_report_export_pdf(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    report_id: String,
) -> Result<String, String> {
    let _ = require_orthopedic_clinician(&state).await?;
    if report_id.trim().is_empty() {
        return Err("Zgjidh një raport të ruajtur për eksport.".to_string());
    }
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        let record = db.orthopedic_report_pdf_data(&report_id).map_err(err_string)?;
        let clinic = db.setting_get("clinic_name")
            .map_err(err_string)?
            .unwrap_or_else(|| "Klinika".to_string());
        let bytes = crate::orthopedics_pdf::render_orthopedic_pdf(&record, &clinic)
            .map_err(err_string)?;
        let version = record["version"].as_i64().unwrap_or(1);
        let id: String = report_id.chars().filter(|c| c.is_ascii_alphanumeric()).take(12).collect();
        let filename = format!("Ortopedi-Raport-v{}-{}.pdf", version, id);
        #[cfg(mobile)]
        {
            return save_pdf_via_dialog(&app, &filename, &bytes);
        }
        #[cfg(desktop)]
        {
            use tauri_plugin_dialog::DialogExt;
            let chosen = app.dialog().file()
                .set_title("Ruaj raportin klinik PDF - A4")
                .set_file_name(&filename)
                .add_filter("PDF", &["pdf"])
                .blocking_save_file()
                .ok_or_else(|| "Ruajtja e PDF u anulua.".to_string())?;
            let path = chosen.as_path()
                .ok_or_else(|| "Zgjedh një vendndodhje skedari në Windows.".to_string())?;
            std::fs::write(path, &bytes).map_err(err_string)?;
            // Open the generated PDF in Windows' registered PDF reader so the
            // clinician sees the actual A4 document and can use its Print dialog.
            // Never trigger WebView/window.print or run a shell command string.
            #[cfg(target_os = "windows")]
            {
                let _ = std::process::Command::new("explorer.exe").arg(path).spawn();
            }
            Ok(path.display().to_string())
        }
    }).await.map_err(err_string)?
}

#[tauri::command]
async fn orthopedic_reports_list(
    state: tauri::State<'_, AppState>, episode_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    let _ = require_orthopedic_clinician(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.orthopedic_reports_list(&episode_id))
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn orthopedic_report_version_create(
    state: tauri::State<'_, AppState>, value: serde_json::Value,
) -> Result<serde_json::Value, String> {
    // Distinguish medical report signing from surgery finalization.
    let clinician = match require_orthopedic_clinician(&state).await? {
        SessionKind::Doctor { doctor_id, .. } => doctor_id,
        _ => return Err("Për nënshkrimin e raportit duhet të identifikohesh si Mjek, jo si Pronar.".to_string()),
    };
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.orthopedic_report_version_create(&value,&clinician))
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn orthopedic_markers_list(state: tauri::State<'_, AppState>, episode_id: String) -> Result<Vec<serde_json::Value>, String> { let _=require_orthopedic_clinician(&state).await?; let db=state.db.clone(); tokio::task::spawn_blocking(move||db.orthopedic_markers_list(&episode_id)).await.map_err(err_string)?.map_err(err_string) }
#[tauri::command]
async fn orthopedic_marker_upsert(state: tauri::State<'_, AppState>, value: serde_json::Value) -> Result<(), String> { let _=require_orthopedic_clinician(&state).await?; let db=state.db.clone(); tokio::task::spawn_blocking(move||{db.orthopedic_marker_upsert(&value)?; db.orthopedic_audit("marker",value["id"].as_str().unwrap_or_default(),&value)}).await.map_err(err_string)?.map_err(err_string) }

#[tauri::command]
async fn orthopedic_assessments_list(state: tauri::State<'_, AppState>, episode_id: String) -> Result<Vec<serde_json::Value>, String> { let _=require_orthopedic_clinician(&state).await?; let db=state.db.clone(); tokio::task::spawn_blocking(move||db.orthopedic_assessments_list(&episode_id)).await.map_err(err_string)?.map_err(err_string) }
#[tauri::command]
async fn orthopedic_assessment_upsert(state: tauri::State<'_, AppState>, value: serde_json::Value) -> Result<(), String> { let _=require_orthopedic_clinician(&state).await?; let db=state.db.clone(); tokio::task::spawn_blocking(move||{db.orthopedic_assessment_upsert(&value)?; db.orthopedic_audit("assessment",value["id"].as_str().unwrap_or_default(),&value)}).await.map_err(err_string)?.map_err(err_string) }

#[tauri::command]
async fn orthopedic_fractures_list(state: tauri::State<'_, AppState>, episode_id: String) -> Result<Vec<serde_json::Value>, String> { let _=require_orthopedic_clinician(&state).await?; let db=state.db.clone(); tokio::task::spawn_blocking(move||db.orthopedic_fractures_list(&episode_id)).await.map_err(err_string)?.map_err(err_string) }
#[tauri::command]
async fn orthopedic_fracture_upsert(state: tauri::State<'_, AppState>, value: serde_json::Value) -> Result<(), String> { let _=require_orthopedic_clinician(&state).await?; let db=state.db.clone(); tokio::task::spawn_blocking(move||{db.orthopedic_fracture_upsert(&value)?; db.orthopedic_audit("fracture",value["id"].as_str().unwrap_or_default(),&value)}).await.map_err(err_string)?.map_err(err_string) }

#[tauri::command]
async fn orthopedic_imaging_links_list(state: tauri::State<'_, AppState>, episode_id: String) -> Result<Vec<serde_json::Value>, String> { let _=require_orthopedic_clinician(&state).await?; let db=state.db.clone(); tokio::task::spawn_blocking(move||db.orthopedic_imaging_links_list(&episode_id)).await.map_err(err_string)?.map_err(err_string) }
#[tauri::command]
async fn orthopedic_imaging_link_upsert(state: tauri::State<'_, AppState>, value: serde_json::Value) -> Result<(), String> { let _=require_orthopedic_clinician(&state).await?; let db=state.db.clone(); tokio::task::spawn_blocking(move||{db.orthopedic_imaging_link_upsert(&value)?; db.orthopedic_audit("imaging_link",value["id"].as_str().unwrap_or_default(),&value)}).await.map_err(err_string)?.map_err(err_string) }

#[tauri::command]
async fn orthopedic_postop_visits_list(state: tauri::State<'_, AppState>, episode_id: String) -> Result<Vec<serde_json::Value>, String> { let _=require_orthopedic_clinician(&state).await?; let db=state.db.clone(); tokio::task::spawn_blocking(move||db.orthopedic_postop_visits_list(&episode_id)).await.map_err(err_string)?.map_err(err_string) }
#[tauri::command]
async fn orthopedic_postop_visit_upsert(state: tauri::State<'_, AppState>, value: serde_json::Value) -> Result<(), String> { let _=require_orthopedic_clinician(&state).await?; let db=state.db.clone(); tokio::task::spawn_blocking(move||{db.orthopedic_postop_visit_upsert(&value)?; db.orthopedic_audit("postop_visit",value["id"].as_str().unwrap_or_default(),&value)}).await.map_err(err_string)?.map_err(err_string) }

#[tauri::command]
async fn orthopedic_implants_search(state: tauri::State<'_, AppState>, query: String) -> Result<Vec<serde_json::Value>, String> { let _=require_orthopedic_clinician(&state).await?; let db=state.db.clone(); tokio::task::spawn_blocking(move||db.orthopedic_implants_search(&query)).await.map_err(err_string)?.map_err(err_string) }

#[tauri::command]
async fn settings_set(
    state: tauri::State<'_, AppState>,
    key: String,
    value: String,
) -> Result<(), String> {
    let k = key.trim().to_string();
    if k.is_empty() {
        return Err("key is required".to_string());
    }
    let mut v = value;

    // Authentication/session state must only be changed through auth commands.
    // Vendor admin unlock must not permit arbitrary session or password-hash writes.
    if matches!(
        k.as_str(),
        "admin_salt" | "admin_hash" | "user_salt" | "user_hash"
            | "cashier_salt" | "cashier_hash" | "user_logged_in"
            | "session" | "session_expires_at"
    ) {
        return Err("ky konfigurim menaxhohet vetem nga autentikimi".to_string());
    }

    let protected = matches!(
        k.as_str(),
        "supabase_url"
            | "supabase_api_key"
            | "supabase_anon_key"
            | "update_base_url"
            | "desktop_update_api"
            | "clinic_id"
            | "admin_salt"
            | "admin_hash"
            | "user_salt"
            | "user_hash"
            | "cashier_salt"
            | "cashier_hash"
            | "user_logged_in"
            | "session"
    );
    if protected {
        if !state.auth.is_admin_unlocked().await {
            // During first-time setup (app not configured yet), allow bootstrap of
            // vendor-sensitive connectivity keys without admin unlock.
            let setup_bootstrap_key = matches!(
                k.as_str(),
                "supabase_url"
                    | "supabase_api_key"
                    | "supabase_anon_key"
                    | "update_base_url"
                    | "desktop_update_api"
            );
            if setup_bootstrap_key {
                let db = state.db.clone();
                let configured =
                    tokio::task::spawn_blocking(move || crate::auth::is_configured(&db))
                        .await
                        .map_err(err_string)?
                        .map_err(err_string)?;
                if configured {
                    return Err("kjo vlere kerkon hyrje si admin".to_string());
                }
            } else {
                return Err("kjo vlere kerkon hyrje si admin".to_string());
            }
        }
    } else {
        let _ = require_login(&state).await?;
    }

    if k == FISCAL_PRINTER_PROVIDER_KEY {
        v = normalize_fiscal_printer_provider(&v).ok_or_else(|| {
            "vlera e printerit fiskal duhet të jetë ENTERNET, Global EU ose SEF".to_string()
        })?;
        // SEF (PROGRAMERI POS): ruhet vetëm nga `sef_bridge_select_engine` (pas
        // konfigurimit "gati"), dhe nuk lejohet kthimi prapa (e njëjta arkë s'bën PEF dhe SEF).
        let current = get_fiscal_printer_provider(&state).await?;
        if current == crate::sef_bridge::PROVIDER_VALUE && v != crate::sef_bridge::PROVIDER_VALUE {
            return Err(crate::sef_bridge::MSG_SWITCH_BACK_REFUSED.to_string());
        }
        if v == crate::sef_bridge::PROVIDER_VALUE && current != crate::sef_bridge::PROVIDER_VALUE {
            return Err(crate::sef_bridge::MSG_SELECT_VIA_CONFIGURE.to_string());
        }
    }

    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.setting_set(&k, &v))
        .await
        .map_err(err_string)?
        .map_err(err_string)
}

#[tauri::command]
fn lab_devices_list_profiles() -> Vec<crate::models::AnalyzerProfile> {
    lab_devices::known_profiles()
}

#[tauri::command]
async fn lab_serial_ports_list(state: tauri::State<'_, AppState>) -> Result<Vec<String>, String> {
    let _ = require_login(&state).await?;
    Ok(lab_devices::list_serial_ports())
}

#[derive(serde::Serialize)]
struct LabDeviceDefault {
    profile_id: Option<String>,
    port_name: Option<String>,
}

#[tauri::command]
async fn lab_device_get_default(
    state: tauri::State<'_, AppState>,
) -> Result<LabDeviceDefault, String> {
    let _ = require_login(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        let profile_id = db.setting_get(KEY_LAB_DEVICE_PROFILE_ID)?;
        let port_name = db.setting_get(KEY_LAB_DEVICE_PORT_NAME)?;
        Ok::<_, anyhow::Error>(LabDeviceDefault {
            profile_id,
            port_name,
        })
    })
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn lab_device_set_default(
    state: tauri::State<'_, AppState>,
    profile_id: String,
    port_name: String,
) -> Result<(), String> {
    let _ = require_login(&state).await?;
    if lab_devices::find_profile(&profile_id).is_none() {
        return Err("profil i panjohur i analizuesit".to_string());
    }
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        db.setting_set(KEY_LAB_DEVICE_PROFILE_ID, &profile_id)?;
        db.setting_set(KEY_LAB_DEVICE_PORT_NAME, &port_name)?;
        Ok::<_, anyhow::Error>(())
    })
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn lab_device_connect(
    state: tauri::State<'_, AppState>,
    port_name: String,
    profile_id: String,
) -> Result<(), String> {
    let _ = require_login(&state).await?;
    let profile = lab_devices::find_profile(&profile_id).ok_or("profil i panjohur i analizuesit")?;
    let db = state.db.clone();
    let lab_serial = state.lab_serial.clone();
    tokio::task::spawn_blocking(move || lab_serial.connect(db, port_name, profile))
        .await
        .map_err(err_string)?
        .map_err(err_string)
}

#[tauri::command]
async fn lab_device_disconnect(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let _ = require_login(&state).await?;
    let lab_serial = state.lab_serial.clone();
    tokio::task::spawn_blocking(move || lab_serial.disconnect())
        .await
        .map_err(err_string)
}

#[tauri::command]
async fn lab_device_status(
    state: tauri::State<'_, AppState>,
) -> Result<crate::models::LabDeviceStatus, String> {
    let _ = require_login(&state).await?;
    Ok(state.lab_serial.status())
}

#[tauri::command]
async fn lab_inbox_list(
    state: tauri::State<'_, AppState>,
    only_unmatched: Option<bool>,
) -> Result<Vec<crate::models::LabInboxItem>, String> {
    let _ = require_login(&state).await?;
    let db = state.db.clone();
    let only_unmatched = only_unmatched.unwrap_or(true);
    tokio::task::spawn_blocking(move || db.lab_inbox_list(only_unmatched))
        .await
        .map_err(err_string)?
        .map_err(err_string)
}

#[tauri::command]
async fn lab_inbox_assign(
    state: tauri::State<'_, AppState>,
    inbox_id: String,
    visit_id: String,
) -> Result<(), String> {
    let _ = require_login(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.lab_inbox_assign_to_visit(&inbox_id, &visit_id))
        .await
        .map_err(err_string)?
        .map_err(err_string)
}

fn decode_base64_data(data: &str) -> anyhow::Result<Vec<u8>> {
    use base64::{engine::general_purpose, Engine as _};
    let s = data.trim();
    let s = if let Some((_, b64)) = s.split_once(",") {
        b64
    } else {
        s
    };
    let bytes = general_purpose::STANDARD.decode(s)?;
    Ok(bytes)
}

#[tauri::command]
async fn clinic_asset_set_png(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    key: String,
    base64_png: String,
) -> Result<(), String> {
    let _ = require_owner(&state).await?;
    let k = key.trim().to_lowercase();
    let (file_name, setting_key) = match k.as_str() {
        "logo" => ("clinic_logo.png", "clinic_logo_path"),
        "pdf_header" => ("pdf_header.png", "pdf_header_path"),
        _ => return Err("key duhet te jete: logo ose pdf_header".to_string()),
    };

    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| anyhow::anyhow!(e))
        .map_err(err_string)?;
    let assets_dir = data_dir.join("assets");
    let out_path = assets_dir.join(file_name);
    let rel = format!("assets/{}", file_name);
    let db = state.db.clone();

    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        std::fs::create_dir_all(&assets_dir)?;
        let bytes = decode_base64_data(&base64_png)?;
        std::fs::write(&out_path, bytes)?;
        db.setting_set(setting_key, &rel)?;
        Ok(())
    })
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn clinic_asset_clear_png(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    key: String,
) -> Result<(), String> {
    let _ = require_owner(&state).await?;
    let k = key.trim().to_lowercase();
    let (file_name, setting_key) = match k.as_str() {
        "logo" => ("clinic_logo.png", "clinic_logo_path"),
        "pdf_header" => ("pdf_header.png", "pdf_header_path"),
        _ => return Err("key duhet te jete: logo ose pdf_header".to_string()),
    };
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| anyhow::anyhow!(e))
        .map_err(err_string)?;
    let out_path = data_dir.join("assets").join(file_name);
    let db = state.db.clone();

    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let _ = std::fs::remove_file(&out_path);
        db.setting_set(setting_key, "")?;
        Ok(())
    })
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn clients_list(
    state: tauri::State<'_, AppState>,
    search: Option<String>,
) -> Result<Vec<Client>, String> {
    let _ = require_login(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.clients_list(search))
        .await
        .map_err(err_string)?
        .map_err(err_string)
}

#[tauri::command]
async fn clients_upsert(
    state: tauri::State<'_, AppState>,
    client: ClientUpsertInput,
) -> Result<Client, String> {
    let _ = require_login(&state).await?;
    let db = state.db.clone();
    let is_demo_mode = state.is_demo_mode;
    tokio::task::spawn_blocking(move || {
        if is_demo_mode && client.id.is_none() {
            let existing = db.clients_list(None)?;
            if existing.iter().any(|c| c.deleted == 0) {
                bail!("Modaliteti Demo lejon vetëm 1 pacient. Fshi pacientin ekzistues për të shtuar një tjetër.");
            }
        }
        db.clients_upsert(client)
    })
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn clients_delete(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    let _ = require_login(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.clients_delete(&id))
        .await
        .map_err(err_string)?
        .map_err(err_string)
}

#[tauri::command]
async fn sales_list(
    state: tauri::State<'_, AppState>,
    filters: Option<SalesListFilters>,
) -> Result<Vec<Sale>, String> {
    let session = require_finance(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || match session {
        SessionKind::User {
            role: crate::auth::UserRole::Cashier,
        } => db.sales_list_fiscal_only(filters),
        _ => db.sales_list(filters),
    })
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn sales_daily_report(
    state: tauri::State<'_, AppState>,
    date: String,
) -> Result<DailySalesReport, String> {
    let session = require_login(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || match session {
        SessionKind::User {
            role: crate::auth::UserRole::Cashier,
        } => {
            // Cashier is fiscal-only: show only the fiscal portion (hide non-fiscal-only sales).
            let mut rep = db.sales_daily_report(&date)?;
            rep.rows.retain(|r| r.fiscal_total > 0.0);
            for r in &mut rep.rows {
                r.non_fiscal_total = 0.0;
                r.total = r.fiscal_total;
                r.classification = "fiscal".to_string();
            }
            let fiscal_total: f64 = rep.rows.iter().map(|r| r.fiscal_total).sum();
            rep.total = fiscal_total;
            rep.fiscal_total = fiscal_total;
            rep.non_fiscal_total = 0.0;
            rep.count_sales = rep.rows.len() as i64;
            rep.count_fiscal_only = rep.count_sales;
            rep.count_non_fiscal_only = 0;
            rep.count_mixed = 0;
            Ok(rep)
        }
        SessionKind::Doctor {
            doctor_id,
            is_admin: false,
        } => db.sales_daily_report_for_doctor(&date, &doctor_id),
        _ => db.sales_daily_report(&date),
    })
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn sales_upsert(
    state: tauri::State<'_, AppState>,
    sale: SaleUpsertInput,
) -> Result<Sale, String> {
    let _ = require_finance(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.sales_upsert(sale))
        .await
        .map_err(err_string)?
        .map_err(err_string)
}

#[tauri::command]
async fn sales_delete(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    let _ = require_finance(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.sales_delete(&id))
        .await
        .map_err(err_string)?
        .map_err(err_string)
}

#[tauri::command]
async fn sales_mark_fiscalized_manual(
    state: tauri::State<'_, AppState>,
    sale_id: String,
    reason: Option<String>,
) -> Result<(), String> {
    let _ = require_finance(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        db.sales_mark_fiscalized_manual(&sale_id, reason.as_deref())
    })
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn sales_mark_non_fiscal_manual(
    state: tauri::State<'_, AppState>,
    sale_id: String,
    reason: Option<String>,
) -> Result<(), String> {
    let _ = require_finance(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        db.sales_mark_non_fiscal_manual(&sale_id, reason.as_deref())
    })
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn payments_list(
    state: tauri::State<'_, AppState>,
    filters: Option<PaymentsListFilters>,
) -> Result<Vec<Payment>, String> {
    let _ = require_finance(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.payments_list(filters))
        .await
        .map_err(err_string)?
        .map_err(err_string)
}

#[tauri::command]
async fn payments_upsert(
    state: tauri::State<'_, AppState>,
    payment: PaymentUpsertInput,
) -> Result<Payment, String> {
    let _ = require_finance(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.payments_upsert(payment))
        .await
        .map_err(err_string)?
        .map_err(err_string)
}

#[tauri::command]
async fn payments_delete(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    let _ = require_finance(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.payments_delete(&id))
        .await
        .map_err(err_string)?
        .map_err(err_string)
}

#[tauri::command]
async fn doctors_list(
    state: tauri::State<'_, AppState>,
    search: Option<String>,
) -> Result<Vec<Doctor>, String> {
    let session = require_login(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || match session {
        SessionKind::Doctor {
            doctor_id,
            is_admin: false,
        } => Ok(db
            .doctors_get(&doctor_id)?
            .into_iter()
            .filter(|d| d.deleted == 0)
            .collect()),
        _ => db.doctors_list(search),
    })
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn doctors_upsert(
    state: tauri::State<'_, AppState>,
    doctor: DoctorUpsertInput,
) -> Result<Doctor, String> {
    let _ = require_owner(&state).await?;
    let db = state.db.clone();
    let row = tokio::task::spawn_blocking(move || db.doctors_upsert(doctor))
        .await
        .map_err(err_string)?
        .map_err(err_string)?;
    let _ = state.sync.sync_now().await;
    Ok(row)
}

#[tauri::command]
async fn doctors_delete(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    let _ = require_owner(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.doctors_delete(&id))
        .await
        .map_err(err_string)?
        .map_err(err_string)?;
    let _ = state.sync.sync_now().await;
    Ok(())
}

#[tauri::command]
async fn services_list(
    state: tauri::State<'_, AppState>,
    search: Option<String>,
) -> Result<Vec<Service>, String> {
    let _ = require_login(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.services_list(search))
        .await
        .map_err(err_string)?
        .map_err(err_string)
}

#[tauri::command]
async fn services_upsert(
    state: tauri::State<'_, AppState>,
    service: ServiceUpsertInput,
) -> Result<Service, String> {
    let _ = require_owner(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.services_upsert(service))
        .await
        .map_err(err_string)?
        .map_err(err_string)
}

#[tauri::command]
async fn services_delete(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    let _ = require_owner(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.services_delete(&id))
        .await
        .map_err(err_string)?
        .map_err(err_string)
}

#[tauri::command]
async fn appointments_list(
    state: tauri::State<'_, AppState>,
    filters: Option<AppointmentsListFilters>,
) -> Result<Vec<Appointment>, String> {
    let session = require_login(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        let mut f = filters.unwrap_or_default();
        if let SessionKind::Doctor {
            doctor_id,
            is_admin: false,
        } = session
        {
            f.doctor_id = Some(doctor_id);
        }
        db.appointments_list(Some(f))
    })
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn appointments_upsert(
    state: tauri::State<'_, AppState>,
    appointment: AppointmentUpsertInput,
) -> Result<Appointment, String> {
    let session = require_login(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        let mut a = appointment;
        if let SessionKind::Doctor {
            doctor_id,
            is_admin: false,
        } = session
        {
            a.doctor_id = Some(doctor_id);
        }
        db.appointments_upsert(a)
    })
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn appointments_delete(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    let session = require_login(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        if let SessionKind::Doctor {
            doctor_id,
            is_admin: false,
        } = session
        {
            let a = db
                .appointments_get(&id)?
                .ok_or_else(|| anyhow::anyhow!("termini nuk u gjet"))?;
            if a.deleted == 0 && a.doctor_id.as_deref() != Some(doctor_id.as_str()) {
                bail!("nuk ke akses per kete termin");
            }
        }
        db.appointments_delete(&id)
    })
    .await
    .map_err(err_string)?
    .map_err(err_string)
}




#[tauri::command]
async fn dental_treatment_plans_list(state:tauri::State<'_,AppState>,patient_id:String)->Result<Vec<serde_json::Value>,String>{
    let (doctor_id,is_admin)=match require_login(&state).await?{
        SessionKind::Doctor{doctor_id,is_admin}=>(doctor_id,is_admin),
        _=>return Err("Doctor session required".into())
    };
    let db=state.db.clone();
    tokio::task::spawn_blocking(move||{
        if !is_admin&&!db.dental_patient_access(&patient_id,&doctor_id)?{bail!("Patient access denied");}
        db.dental_treatment_plans_list(&patient_id)
    }).await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn dental_periodontal_exams_list(state:tauri::State<'_,AppState>,patient_id:String)->Result<Vec<serde_json::Value>,String>{
    let (doctor_id,is_admin)=match require_login(&state).await?{
        SessionKind::Doctor{doctor_id,is_admin}=>(doctor_id,is_admin),
        _=>return Err("Doctor session required".into())
    };
    let db=state.db.clone();
    tokio::task::spawn_blocking(move||{
        if !is_admin&&!db.dental_patient_access(&patient_id,&doctor_id)?{bail!("Patient access denied");}
        db.dental_periodontal_exams_list(&patient_id)
    }).await.map_err(err_string)?.map_err(err_string)
}
#[tauri::command]
async fn dental_periodontal_exam_create(state:tauri::State<'_,AppState>,exam:serde_json::Value)->Result<serde_json::Value,String>{
    let doctor_id=match require_login(&state).await?{
        SessionKind::Doctor{doctor_id,..}=>doctor_id,
        _=>return Err("Doctor session required".into())
    };
    let db=state.db.clone();
    tokio::task::spawn_blocking(move||db.dental_periodontal_exam_create(exam,&doctor_id))
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn dental_finding_correct(state:tauri::State<'_,AppState>,correction:serde_json::Value)->Result<serde_json::Value,String>{
    let doctor_id=match require_login(&state).await?{
        SessionKind::Doctor{doctor_id,..}=>doctor_id,
        _=>return Err("Doctor session required".into())
    };
    let db=state.db.clone();
    tokio::task::spawn_blocking(move||db.dental_finding_correct(correction,&doctor_id))
        .await.map_err(err_string)?.map_err(err_string)
}
#[tauri::command]
async fn dental_finding_corrections_list(state:tauri::State<'_,AppState>,patient_id:String)->Result<Vec<serde_json::Value>,String>{
    let (doctor_id,is_admin)=match require_login(&state).await?{
        SessionKind::Doctor{doctor_id,is_admin}=>(doctor_id,is_admin),
        _=>return Err("Doctor session required".into())
    };
    let db=state.db.clone();
    tokio::task::spawn_blocking(move||{
        if !is_admin&&!db.dental_patient_access(&patient_id,&doctor_id)?{bail!("Patient access denied");}
        db.dental_finding_corrections_list(&patient_id)
    }).await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn dental_findings_list(state:tauri::State<'_,AppState>,patient_id:String)->Result<Vec<serde_json::Value>,String>{
    let session=require_login(&state).await?;
    let (doctor_id,is_admin)=match session{
        SessionKind::Doctor{doctor_id,is_admin}=>(doctor_id,is_admin),
        _=>return Err("Doctor session required".into())
    };
    let db=state.db.clone();
    tokio::task::spawn_blocking(move||{
        if !is_admin {
            if !db.dental_patient_access(&patient_id,&doctor_id)?{bail!("Patient access denied");}
        }
        db.dental_findings_list(&patient_id)
    }).await.map_err(err_string)?.map_err(err_string)
}
#[tauri::command]
async fn dental_finding_create(state:tauri::State<'_,AppState>,finding:serde_json::Value)->Result<serde_json::Value,String>{
    let session=require_login(&state).await?;
    let doctor_id=match session{
        SessionKind::Doctor{doctor_id,..}=>doctor_id,
        _=>return Err("Doctor session required".into())
    };
    let db=state.db.clone();
    tokio::task::spawn_blocking(move||db.dental_finding_create(finding,&doctor_id))
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn visits_list(
    state: tauri::State<'_, AppState>,
    filters: Option<VisitsListFilters>,
) -> Result<Vec<Visit>, String> {
    let session = require_login(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        let mut f = filters.unwrap_or_default();
        if let SessionKind::Doctor {
            doctor_id,
            is_admin: false,
        } = session
        {
            f.doctor_id = Some(doctor_id);
        }
        db.visits_list(Some(f))
    })
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn visits_upsert(
    state: tauri::State<'_, AppState>,
    visit: VisitUpsertInput,
) -> Result<Visit, String> {
    let session = require_login(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        let mut v = visit;
        if let SessionKind::Doctor {
            doctor_id,
            is_admin: false,
        } = session
        {
            if let Some(id) = v.id.as_deref().filter(|x| !x.trim().is_empty()) {
                let existing = db
                    .visits_get(id)?
                    .ok_or_else(|| anyhow::anyhow!("vizita nuk u gjet"))?;
                if existing.deleted == 0
                    && existing.doctor_id.as_deref() != Some(doctor_id.as_str())
                {
                    bail!("nuk ke akses per kete vizite");
                }
            }
            v.doctor_id = Some(doctor_id);
        }
        db.visits_upsert(v)
    })
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn visits_delete(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    let session = require_login(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        if let SessionKind::Doctor {
            doctor_id,
            is_admin: false,
        } = session
        {
            let v = db
                .visits_get(&id)?
                .ok_or_else(|| anyhow::anyhow!("vizita nuk u gjet"))?;
            if v.deleted == 0 && v.doctor_id.as_deref() != Some(doctor_id.as_str()) {
                bail!("nuk ke akses per kete vizite");
            }
        }
        db.visits_delete(&id)
    })
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn visit_items_list(
    state: tauri::State<'_, AppState>,
    filters: Option<VisitItemsListFilters>,
) -> Result<Vec<VisitItem>, String> {
    let session = require_login(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        let f = filters.unwrap_or_default();
        if let SessionKind::Doctor {
            doctor_id,
            is_admin: false,
        } = session
        {
            let vid = f
                .visit_id
                .as_deref()
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .ok_or_else(|| anyhow::anyhow!("per mjek duhet visit_id"))?;
            let v = db
                .visits_get(&vid)?
                .ok_or_else(|| anyhow::anyhow!("vizita nuk u gjet"))?;
            if v.deleted == 0 && v.doctor_id.as_deref() != Some(doctor_id.as_str()) {
                bail!("nuk ke akses per kete vizite");
            }
            return db.visit_items_list(Some(VisitItemsListFilters {
                visit_id: Some(vid),
                client_id: None,
                include_deleted: f.include_deleted,
            }));
        }
        db.visit_items_list(Some(f))
    })
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn visit_items_upsert(
    state: tauri::State<'_, AppState>,
    item: VisitItemUpsertInput,
) -> Result<VisitItem, String> {
    let session = require_login(&state).await?;
    // Cashier role is fiscal-only.
    let mut item = item;
    if let SessionKind::User {
        role: crate::auth::UserRole::Cashier,
    } = &session
    {
        item.fiscal = Some(true);
    }
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        if let SessionKind::Doctor {
            doctor_id,
            is_admin: false,
        } = session
        {
            let v = db
                .visits_get(&item.visit_id)?
                .ok_or_else(|| anyhow::anyhow!("vizita nuk u gjet"))?;
            if v.deleted == 0 && v.doctor_id.as_deref() != Some(doctor_id.as_str()) {
                bail!("nuk ke akses per kete vizite");
            }
        }
        db.visit_items_upsert(item)
    })
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn visit_items_delete(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    let session = require_login(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        if let SessionKind::Doctor {
            doctor_id,
            is_admin: false,
        } = session
        {
            let it = db
                .visit_items_get(&id)?
                .ok_or_else(|| anyhow::anyhow!("procedura nuk u gjet"))?;
            let v = db
                .visits_get(&it.visit_id)?
                .ok_or_else(|| anyhow::anyhow!("vizita nuk u gjet"))?;
            if v.deleted == 0 && v.doctor_id.as_deref() != Some(doctor_id.as_str()) {
                bail!("nuk ke akses per kete vizite");
            }
        }
        db.visit_items_delete(&id)
    })
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn cash_list(
    state: tauri::State<'_, AppState>,
    filters: Option<CashListFilters>,
) -> Result<Vec<CashEntry>, String> {
    let _ = require_owner(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.cash_list(filters))
        .await
        .map_err(err_string)?
        .map_err(err_string)
}

#[tauri::command]
async fn cash_upsert(
    state: tauri::State<'_, AppState>,
    entry: CashEntryUpsertInput,
) -> Result<CashEntry, String> {
    let _ = require_owner(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.cash_upsert(entry))
        .await
        .map_err(err_string)?
        .map_err(err_string)
}

#[tauri::command]
async fn cash_delete(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    let _ = require_owner(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.cash_delete(&id))
        .await
        .map_err(err_string)?
        .map_err(err_string)
}

#[tauri::command]
async fn sync_now(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let _ = require_login(&state).await?;
    state.sync.sync_now().await.map_err(err_string)
}

#[tauri::command]
async fn updates_check_now(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let _ = require_login(&state).await?;
    state.updates.check_now(&app).await.map_err(err_string)
}

async fn desktop_updates_check_and_store(
    app: &tauri::AppHandle,
    db: Arc<Db>,
    mark_manual: bool,
) -> Result<crate::desktop_updates::DesktopUpdateInfo, String> {
    let db_for_api = db.clone();
    let api = tokio::task::spawn_blocking(move || db_for_api.setting_get(KEY_DESKTOP_UPDATE_API))
        .await
        .map_err(err_string)?
        .map_err(err_string)?;

    let mut info = crate::desktop_updates::check_now(app, api.as_deref())
        .await
        .map_err(err_string)?;
    let now = crate::util::now_iso();
    let current_version_norm = info
        .current_version
        .trim()
        .trim_start_matches('v')
        .to_string();
    let latest_version_norm = info
        .latest_version
        .as_deref()
        .map(|x| x.trim().trim_start_matches('v').to_string())
        .filter(|x| !x.is_empty());
    let update_available = info.update_available
        && latest_version_norm.as_deref() != Some(current_version_norm.as_str());

    let db_for_store = db.clone();
    let mark_manual_store = mark_manual;
    let latest_store = latest_version_norm.clone();
    let now_store = now.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let prev_latest = db_for_store
            .setting_get(KEY_DESKTOP_UPDATE_LATEST_VERSION)?
            .unwrap_or_default()
            .trim()
            .to_string();
        db_for_store.setting_set(KEY_DESKTOP_UPDATE_LAST_CHECKED_AT, &now_store)?;
        db_for_store.setting_set(
            KEY_DESKTOP_UPDATE_AVAILABLE,
            if update_available { "1" } else { "0" },
        )?;
        db_for_store.setting_set(
            KEY_DESKTOP_UPDATE_LATEST_VERSION,
            latest_store.as_deref().unwrap_or(""),
        )?;
        if mark_manual_store {
            db_for_store.setting_set(KEY_DESKTOP_UPDATE_LAST_MANUAL_CHECK_AT, &now_store)?;
        }
        if update_available {
            let latest_now = latest_store.as_deref().unwrap_or("");
            let first_seen = db_for_store
                .setting_get(KEY_DESKTOP_UPDATE_FIRST_SEEN_AT)?
                .unwrap_or_default();
            if first_seen.trim().is_empty() || prev_latest.trim() != latest_now.trim() {
                db_for_store.setting_set(KEY_DESKTOP_UPDATE_FIRST_SEEN_AT, &now_store)?;
            }
        } else {
            db_for_store.setting_set(KEY_DESKTOP_UPDATE_FIRST_SEEN_AT, "")?;
        }
        Ok(())
    })
    .await
    .map_err(err_string)?
    .map_err(err_string)?;

    let db_for_policy = db.clone();
    let current_version_for_policy = info.current_version.clone();
    let (forced, _latest, force_deadline_at, last_manual) =
        tokio::task::spawn_blocking(move || {
            compute_desktop_update_policy_from_db(&db_for_policy, &current_version_for_policy)
        })
        .await
        .map_err(err_string)?
        .map_err(err_string)?;

    info.forced = forced;
    info.force_deadline_at = force_deadline_at;
    info.last_manual_check_at = last_manual;
    info.checked_at = Some(now);
    Ok(info)
}

#[tauri::command]
async fn desktop_updates_check_now(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<crate::desktop_updates::DesktopUpdateInfo, String> {
    let _ = require_login(&state).await?;
    desktop_updates_check_and_store(&app, state.db.clone(), true).await
}

#[tauri::command]
async fn desktop_updates_open_download(
    state: tauri::State<'_, AppState>,
    url: String,
) -> Result<(), String> {
    let _ = require_login(&state).await?;
    crate::desktop_updates::open_external(&url).map_err(err_string)
}

#[tauri::command]
async fn updates_apply_downloaded(app: tauri::AppHandle) -> Result<(), String> {
    // Switch the pointer (if a pending version exists) and reload.
    let _ = UpdatesEngine::apply_downloaded_now(&app).map_err(err_string)?;
    reload_ui(app).await
}

#[tauri::command]
async fn reload_ui(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("main") {
        win.eval("window.location.reload()").map_err(err_string)?;
    }
    Ok(())
}

#[tauri::command]
async fn invoice_export_fiscal_inp(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    sale_id: String,
    allow_any_if_no_fiscal: Option<bool>,
) -> Result<String, String> {
    let _ = require_finance(&state).await?;
    let provider = get_fiscal_printer_provider(&state).await?;

    if provider == crate::sef_bridge::PROVIDER_VALUE {
        // SEF përmes PROGRAMERI POS: asnjë skedar .inp, asnjë printim PDF këtu.
        let db = state.db.clone();
        let allow_any = allow_any_if_no_fiscal.unwrap_or(false);
        return tokio::task::spawn_blocking(move || sef_fiscalize_blocking(&db, &sale_id, allow_any))
            .await
            .map_err(err_string)?;
    }

    if provider == "sef" {
        let sale_id = sale_id.trim().to_string();
        if sale_id.is_empty() {
            return Err("sale_id eshte i detyrueshem".to_string());
        }
        let db = state.db.clone();
        let data_dir = app
            .path()
            .app_data_dir()
            .map_err(|e| anyhow::anyhow!(e))
            .map_err(err_string)?;
        return tokio::task::spawn_blocking(move || {
            let printer_name = db
                .setting_get(SEF_PRINTER_NAME_KEY)
                .map_err(err_string)?
                .unwrap_or_default();
            let printer_name = printer_name.trim().to_string();
            if printer_name.is_empty() {
                return Err("Emri i printerit SEF nuk është konfiguruar. Shko te Cilësimet > Printer SEF dhe vendos emrin.".to_string());
            }
            let pdf_bytes = build_invoice_pdf_bytes_inner(&db, &data_dir, &sale_id, false)?;
            print_pdf_to_windows_printer(&pdf_bytes, &printer_name).map_err(err_string)?;
            Ok("Fatura u dërgua në printer.".to_string())
        })
        .await
        .map_err(err_string)?;
    }

    require_enternet_for_fiscal(&provider)?;
    let sale_id = sale_id.trim().to_string();
    let allow_any_items_if_no_fiscal = allow_any_if_no_fiscal.unwrap_or(false);
    if sale_id.is_empty() {
        return Err("sale_id eshte i detyrueshem".to_string());
    }

    #[cfg(target_os = "windows")]
    let out_dir = PathBuf::from(r"C:\Temp");
    #[cfg(not(target_os = "windows"))]
    let out_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| anyhow::anyhow!(e))
        .map_err(err_string)?
        .join("fiscal");
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<String> {
        let p = db.fiscal_receipt_generate_inp(&sale_id, &out_dir, allow_any_items_if_no_fiscal)?;
        Ok(p.display().to_string())
    })
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn fiscal_report_x_inp(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let _ = require_finance(&state).await?;
    let provider = get_fiscal_printer_provider(&state).await?;
    if provider == crate::sef_bridge::PROVIDER_VALUE {
        return sef_report_summary("report.x", None).await;
    }
    require_enternet_for_fiscal(&provider)?;
    tokio::task::spawn_blocking(move || -> anyhow::Result<String> {
        let mut files: Vec<String> = Vec::new();

        let x1 = write_fiscal_temp_inp("x-raport", "X,1,______,_,__;")?;
        files.push(x1.display().to_string());

        let g1 = write_fiscal_temp_inp("x-status", "G,1,______,_,__;NoteStatus")?;
        files.push(g1.display().to_string());
        let g1_out = wait_out_text(&g1, Duration::from_secs(4));
        let mut used_fallback = false;

        if let Some(out) = g1_out.as_deref() {
            if has_note_status_2(out) {
                used_fallback = true;
                let n = write_fiscal_temp_inp("x-close-open", "N,1,______,_,__;")?;
                files.push(n.display().to_string());
                let _ = wait_out_text(&n, Duration::from_secs(3));

                let x2 = write_fiscal_temp_inp("x-raport-retry", "X,1,______,_,__;")?;
                files.push(x2.display().to_string());
                let _ = wait_out_text(&x2, Duration::from_secs(4));

                let g2 =
                    write_fiscal_temp_inp("x-status-after-retry", "G,1,______,_,__;NoteStatus")?;
                files.push(g2.display().to_string());
                let g2_out = wait_out_text(&g2, Duration::from_secs(4)).unwrap_or_default();

                if has_note_status_2(&g2_out) {
                    bail!("X Raport nuk po ben: edhe pas N mbetet fature e hapur (NoteStatus;2).");
                }
            }
        }

        if used_fallback {
            return Ok(format!(
                "X Raport u dergua me fallback (G -> N -> X). Temp: {}",
                files.join(" | ")
            ));
        }

        if g1_out.is_none() {
            return Ok(format!(
                "X Raport u krijua ne Temp, por pa konfirmim nga pajisja fiskale. Temp: {}",
                files.join(" | ")
            ));
        }

        Ok(format!("X Raport u krijua ne Temp: {}", files.join(" | ")))
    })
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn fiscal_report_z_inp(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let _ = require_finance(&state).await?;
    let provider = get_fiscal_printer_provider(&state).await?;
    if provider == crate::sef_bridge::PROVIDER_VALUE {
        return sef_report_summary("report.z", None).await;
    }
    require_enternet_for_fiscal(&provider)?;
    tokio::task::spawn_blocking(move || -> anyhow::Result<String> {
        let mut files: Vec<String> = Vec::new();

        let z1 = write_fiscal_temp_inp("z-raport", "Z,1,______,_,__;")?;
        files.push(z1.display().to_string());

        let g1 = write_fiscal_temp_inp("z-status", "G,1,______,_,__;NoteStatus")?;
        files.push(g1.display().to_string());
        let g1_out = wait_out_text(&g1, Duration::from_secs(4));
        let mut used_fallback = false;

        if let Some(out) = g1_out.as_deref() {
            if has_note_status_2(out) {
                used_fallback = true;
                let n = write_fiscal_temp_inp("z-close-open", "N,1,______,_,__;")?;
                files.push(n.display().to_string());
                let _ = wait_out_text(&n, Duration::from_secs(3));

                let z2 = write_fiscal_temp_inp("z-raport-retry", "Z,1,______,_,__;")?;
                files.push(z2.display().to_string());
                let _ = wait_out_text(&z2, Duration::from_secs(4));

                let g2 =
                    write_fiscal_temp_inp("z-status-after-retry", "G,1,______,_,__;NoteStatus")?;
                files.push(g2.display().to_string());
                let g2_out = wait_out_text(&g2, Duration::from_secs(4)).unwrap_or_default();

                if has_note_status_2(&g2_out) {
                    bail!("Z Raport nuk po ben: edhe pas N mbetet fature e hapur (NoteStatus;2).");
                }
            }
        }

        if used_fallback {
            return Ok(format!(
                "Z Raport u dergua me fallback (G -> N -> Z). Temp: {}",
                files.join(" | ")
            ));
        }

        if g1_out.is_none() {
            return Ok(format!(
                "Z Raport u krijua ne Temp, por pa konfirmim nga pajisja fiskale. Temp: {}",
                files.join(" | ")
            ));
        }

        Ok(format!("Z Raport u krijua ne Temp: {}", files.join(" | ")))
    })
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn admin_reset_clinic(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let _ = require_login(&state).await?;
    if !state.auth.is_admin_unlocked().await {
        return Err("kjo veprim kerkon hyrje si admin".to_string());
    }
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| anyhow::anyhow!(e))
        .map_err(err_string)?;
    std::fs::create_dir_all(&data_dir).map_err(err_string)?;
    std::fs::write(data_dir.join("reset.flag"), b"1").map_err(err_string)?;

    // Restart the app so the DB connection can be safely recreated.
    let exe = std::env::current_exe().map_err(err_string)?;
    let _ = std::process::Command::new(exe)
        .spawn()
        .map_err(err_string)?;
    std::process::exit(0);
}

#[tauri::command]
async fn history_reset_all(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let _ = require_owner(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.history_reset_all())
        .await
        .map_err(err_string)?
        .map_err(err_string)
}

// Ne mobile, document_dir() shkon te nje dosje private e app-it (Android/data/<pkg>/files/Documents)
// qe Android 11+ e fsheh nga app-i standard "Files" (scoped storage). Prandaj ketu hapim save-dialog-un
// nativ (SAF) qe useri te zgjedhe vete vendin — skedari garantohet i dukshem pas kesaj.
#[cfg(mobile)]
fn save_pdf_via_dialog(app: &tauri::AppHandle, default_name: &str, pdf_bytes: &[u8]) -> Result<String, String> {
    use std::io::Write;
    use tauri_plugin_dialog::DialogExt;
    use tauri_plugin_fs::FsExt;

    let chosen = app
        .dialog()
        .file()
        .set_file_name(default_name)
        .add_filter("PDF", &["pdf"])
        .blocking_save_file()
        .ok_or_else(|| "Eksportimi u anulua".to_string())?;

    let mut opts = tauri_plugin_fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    let mut file = app.fs().open(chosen.clone(), opts).map_err(|e| e.to_string())?;
    file.write_all(pdf_bytes).map_err(|e| e.to_string())?;

    Ok(chosen
        .as_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| default_name.to_string()))
}

#[tauri::command]
async fn invoice_export_pdf(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    sale_id: String,
) -> Result<String, String> {
    let session = require_finance(&state).await?;
    let fiscal_only = matches!(
        session,
        SessionKind::User {
            role: crate::auth::UserRole::Cashier
        }
    );
    let sale_id = sale_id.trim().to_string();
    if sale_id.is_empty() {
        return Err("sale_id eshte i detyrueshem".to_string());
    }

    let db = state.db.clone();
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| anyhow::anyhow!(e))
        .map_err(err_string)?;
    #[cfg(desktop)]
    let desktop_dir = app.path().desktop_dir().ok();
    #[cfg(mobile)]
    let app_for_save = app.clone();
    tokio::task::spawn_blocking(move || {
        let pdf_bytes = build_invoice_pdf_bytes_inner(&db, &data_dir, &sale_id, fiscal_only)?;

        #[cfg(mobile)]
        {
            return save_pdf_via_dialog(&app_for_save, &format!("Fature-{}.pdf", sale_id), &pdf_bytes);
        }
        #[cfg(desktop)]
        {
            let out_dir = desktop_dir.unwrap_or_else(|| data_dir.join("temp"));
            std::fs::create_dir_all(&out_dir).map_err(err_string)?;
            let out_path = out_dir.join(format!("Fature-{}.pdf", sale_id));
            std::fs::write(&out_path, &pdf_bytes).map_err(err_string)?;
            Ok(out_path.display().to_string())
        }
    })
    .await
    .map_err(err_string)?
}

#[tauri::command]
async fn visit_export_pdf(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    visit_id: String,
) -> Result<String, String> {
    let session = require_login(&state).await?;
    let visit_id = visit_id.trim().to_string();
    if visit_id.is_empty() {
        return Err("visit_id eshte i detyrueshem".to_string());
    }

    let db = state.db.clone();
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| anyhow::anyhow!(e))
        .map_err(err_string)?;
    #[cfg(desktop)]
    let desktop_dir = app.path().desktop_dir().ok();
    #[cfg(mobile)]
    let app_for_save = app.clone();
    tokio::task::spawn_blocking(move || {
        let clinic_name = db
            .setting_get("clinic_name")
            .map_err(err_string)?
            .unwrap_or_else(|| "Klinika".to_string());

        let header_png: Option<Vec<u8>> = db
            .setting_get("pdf_header_path")
            .ok()
            .flatten()
            .and_then(|rel| {
                let rel = rel.trim().to_string();
                if rel.is_empty() {
                    return None;
                }
                std::fs::read(data_dir.join(rel)).ok()
            });

        let logo_png: Option<Vec<u8>> =
            db.setting_get("clinic_logo_path")
                .ok()
                .flatten()
                .and_then(|rel| {
                    let rel = rel.trim().to_string();
                    if rel.is_empty() {
                        return None;
                    }
                    std::fs::read(data_dir.join(rel)).ok()
                });

        let visit = db
            .visits_get(&visit_id)
            .map_err(err_string)?
            .ok_or_else(|| "vizita nuk u gjet".to_string())?;

        match &session {
            SessionKind::Doctor {
                doctor_id,
                is_admin: false,
            } => {
                let row_doctor = visit.doctor_id.as_deref().unwrap_or("").trim().to_string();
                if row_doctor.is_empty() || row_doctor != doctor_id.trim() {
                    return Err("nuk ke akses ne kete vizite".to_string());
                }
            }
            _ => {}
        }

        let client = db
            .clients_get(&visit.client_id)
            .map_err(err_string)?
            .ok_or_else(|| "pacienti nuk u gjet".to_string())?;

        let doctor_name = if let Some(did) = visit
            .doctor_id
            .as_deref()
            .map(str::trim)
            .filter(|x| !x.is_empty())
        {
            db.doctors_list(None)
                .ok()
                .and_then(|rows| rows.into_iter().find(|d| d.id == did))
                .map(|d| d.name)
        } else {
            None
        };

        let lines: Vec<crate::invoice::InvoiceLine> = db
            .visit_items_list(Some(crate::models::VisitItemsListFilters {
                visit_id: Some(visit.id.clone()),
                client_id: None,
                include_deleted: Some(false),
            }))
            .map_err(err_string)?
            .into_iter()
            .filter(|x| x.deleted == 0)
            .map(|it| crate::invoice::InvoiceLine {
                tooth: it.tooth,
                title: it.title,
                qty: it.qty,
                unit_price: it.unit_price,
                fiscal: it.fiscal == 1,
                vat_code: it.vat_code,
            })
            .collect();

        let total: f64 = lines.iter().map(|x| x.qty * x.unit_price).sum();

        let data = crate::invoice::VisitPdfData {
            clinic_name,
            header_png,
            logo_png,
            visit_id: visit.id.clone(),
            date: visit.date.clone(),
            visit_time: visit.visit_time.clone(),
            status: visit.status.clone(),
            doctor_name,
            client_name: client.name,
            client_code: client.patient_code,
            client_dob: client.dob,
            client_address: client.address,
            client_city: client.city,
            client_phone: client.phone,
            client_email: client.email,
            notes: visit.notes.clone(),
            body_weight: visit.body_weight.clone(),
            body_weight_unit: visit.body_weight_unit.clone(),
            body_height: visit.body_height.clone(),
            body_height_unit: visit.body_height_unit.clone(),
            head_circumference: visit.head_circumference.clone(),
            head_circumference_unit: visit.head_circumference_unit.clone(),
            body_temperature: visit.body_temperature.clone(),
            body_temperature_unit: visit.body_temperature_unit.clone(),
            blood_oxygen: visit.blood_oxygen.clone(),
            blood_oxygen_unit: visit.blood_oxygen_unit.clone(),
            glycemia: visit.glycemia.clone(),
            glycemia_unit: visit.glycemia_unit.clone(),
            pulse: visit.pulse.clone(),
            pulse_unit: visit.pulse_unit.clone(),
            bmi: visit.bmi.clone(),
            blood_pressure_systolic: visit.blood_pressure_systolic.clone(),
            blood_pressure_diastolic: visit.blood_pressure_diastolic.clone(),
            blood_pressure_unit: visit.blood_pressure_unit.clone(),
            complaints: visit.complaints.clone(),
            additional_notes: visit.additional_notes.clone(),
            controls: visit.controls.clone(),
            remarks: visit.remarks.clone(),
            analyses: visit.analyses.clone(),
            advice: visit.advice.clone(),
            therapies: visit.therapies.clone(),
            diagnosis: visit.diagnosis.clone(),
            examinations: visit.examinations.clone(),
            lines,
            total,
        };

        let pdf_bytes = crate::invoice::render_visit_pdf(&data).map_err(err_string)?;

        #[cfg(mobile)]
        {
            return save_pdf_via_dialog(&app_for_save, &format!("Vizite-{}.pdf", visit.id), &pdf_bytes);
        }
        #[cfg(desktop)]
        {
            let out_dir = desktop_dir.unwrap_or_else(|| data_dir.join("temp"));
            std::fs::create_dir_all(&out_dir).map_err(err_string)?;
            let out_path = out_dir.join(format!("Vizite-{}.pdf", visit.id));
            std::fs::write(&out_path, &pdf_bytes).map_err(err_string)?;
            Ok(out_path.display().to_string())
        }
    })
    .await
    .map_err(err_string)?
}

fn fiscal_job_process_inner(db: &Db, job: &crate::models::FiscalJob) -> Result<(), String> {
    let provider_raw = db
        .setting_get(FISCAL_PRINTER_PROVIDER_KEY)
        .map_err(err_string)?
        .unwrap_or_default();
    let provider = normalize_fiscal_printer_provider(&provider_raw)
        .unwrap_or_else(|| provider_raw.trim().to_ascii_lowercase());
    if provider == crate::sef_bridge::PROVIDER_VALUE {
        // Radha fiskale (p.sh. nga telefoni) fiskalizohet te ky PC përmes PROGRAMERI POS.
        return sef_fiscalize_blocking(db, &job.sale_id, true).map(|_| ());
    }
    if provider != "enternet" && provider != "global_eu" {
        return Err("Provideri fiskal i këtij kompjuteri nuk është Enternet/Global EU".to_string());
    }
    #[cfg(target_os = "windows")]
    let out_dir = std::path::PathBuf::from(r"C:\Temp");
    #[cfg(not(target_os = "windows"))]
    return Err("Fiskalizimi i radhës bëhet vetëm nga kompjuteri i klinikës".to_string());
    #[cfg(target_os = "windows")]
    {
        db.fiscal_receipt_generate_inp(&job.sale_id, &out_dir, true)
            .map_err(err_string)?;
        Ok(())
    }
}

fn fiscal_job_mark(db: &Db, job: &crate::models::FiscalJob, status: &str, error: Option<String>) -> anyhow::Result<()> {
    let now = crate::util::now_iso();
    let mut row = job.clone();
    row.status = status.to_string();
    row.error = error;
    row.processed_at = if status == "done" { Some(now.clone()) } else { row.processed_at.clone() };
    row.updated_at = now;
    db.fiscal_job_upsert(&row)
}

const FISCAL_JOB_AUTO_MAX_AGE_SECS: i64 = 2 * 3600; // 2 orë — më të vjetrat kërkojnë aprovim manual

fn fiscal_job_age_secs(created_at: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(created_at)
        .map(|t| (chrono::Utc::now() - t.with_timezone(&chrono::Utc)).num_seconds())
        .unwrap_or(i64::MAX)
}

#[tauri::command]
async fn fiscal_jobs_list(
    state: tauri::State<'_, AppState>,
    status: Option<String>,
) -> Result<Vec<crate::models::FiscalJob>, String> {
    let _ = require_login(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.fiscal_jobs_list(status))
        .await
        .map_err(err_string)?
        .map_err(err_string)
}

#[tauri::command]
async fn fiscal_job_create(
    state: tauri::State<'_, AppState>,
    sale_id: String,
) -> Result<crate::models::FiscalJob, String> {
    let session = require_login(&state).await?;
    let sale_id = sale_id.trim().to_string();
    if sale_id.is_empty() {
        return Err("sale_id eshte i detyrueshem".to_string());
    }
    let requested_by = match &session {
        SessionKind::Doctor { doctor_id, .. } => format!("mjeku:{doctor_id}"),
        _ => "perdorues".to_string(),
    };
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<crate::models::FiscalJob> {
        let now = crate::util::now_iso();
        let row = crate::models::FiscalJob {
            id: uuid::Uuid::new_v4().to_string(),
            sale_id,
            status: "pending".to_string(),
            requested_by,
            error: None,
            processed_at: None,
            created_at: now.clone(),
            updated_at: now,
            deleted: 0,
        };
        db.fiscal_job_upsert(&row)?;
        Ok(row)
    })
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn fiscal_job_cancel(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    let _ = require_login(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        if let Some(job) = db.fiscal_job_get(&id)? {
            if job.status == "pending" {
                fiscal_job_mark(&db, &job, "cancelled", None)?;
            }
        }
        Ok(())
    })
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn fiscal_job_process(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    let _ = require_login(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let job = db
            .fiscal_job_get(&id)
            .map_err(err_string)?
            .ok_or_else(|| "Job-i nuk u gjet".to_string())?;
        if job.status != "pending" && job.status != "failed" {
            return Err(format!("Job-i është '{}'", job.status));
        }
        match fiscal_job_process_inner(&db, &job) {
            Ok(()) => fiscal_job_mark(&db, &job, "done", None).map_err(err_string),
            Err(e) => {
                let _ = fiscal_job_mark(&db, &job, "failed", Some(e.clone()));
                Err(e)
            }
        }
    })
    .await
    .map_err(err_string)?
}

// ---------------------------------------------------------------------------
// SEF (PROGRAMERI POS) — urë me shërbimin fiskal të PROGRAMERI POS.
// Përdoret vetëm kur fiscal_printer_provider = "sef_programeri".
// ---------------------------------------------------------------------------

/// Një fiskalizim SEF në një kohë nga ky proces (klikime të dyfishta, radha fiskale).
fn sef_fiscal_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

/// Thirrje e urës nga kod sinkron (brenda spawn_blocking ose fijes së radhës fiskale).
fn sef_call_blocking(
    op: &str,
    params: serde_json::Value,
    timeout: Duration,
) -> Result<serde_json::Value, crate::sef_bridge::BridgeError> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| {
            crate::sef_bridge::BridgeError::new(
                "INTERNAL",
                "Mjeku nuk arriti të nisë lidhjen me PROGRAMERI POS.",
            )
        })?;
    rt.block_on(crate::sef_bridge::call(op, params, timeout))
}

fn cents_to_text(c: i64) -> String {
    let sign = if c < 0 { "-" } else { "" };
    let a = c.unsigned_abs();
    format!("{sign}{}.{:02}", a / 100, a % 100)
}

/// Snapshot fiscal-only: no patient ID, diagnosis, clinical notes or ATK credentials.
fn sef_build_sale(
    db: &Db, sale_id: &str, allow_any: bool
) -> Result<(crate::db::SefSaleData, crate::sef_bridge::FiscalizeRequest, String), String> {
    use sha2::Digest;
    let data = db.sef_collect_sale(sale_id, allow_any).map_err(err_string)?;
    let rates = crate::sef_bridge::VatRates::from_settings(|k| db.setting_get(k).ok().flatten());
    let lines: Vec<crate::sef_bridge::SaleLine> = data.items.iter().map(|it|
        crate::sef_bridge::SaleLine {
            id: it.id.clone(), title: it.title.clone(), qty: it.qty,
            unit_price: it.unit_price, vat_code: it.vat_code.clone()
        }).collect();
    let payments: Vec<crate::sef_bridge::SalePayment> = data.payments.iter().map(|(m,a)|
        crate::sef_bridge::SalePayment { method:m.clone(), amount:*a }).collect();
    let req = crate::sef_bridge::build_fiscalize_request_strict(&lines, &payments, &rates)
        .map_err(err_string)?;
    let canonical = serde_json::to_vec(&serde_json::json!({
        "items": &req.items, "payments": &req.payments
    })).map_err(err_string)?;
    let hash = hex::encode(sha2::Sha256::digest(canonical));
    Ok((data, req, hash))
}

/// Lookup never fiscalizes; it resolves an uncertain POS outcome by the original sale ID.
fn sef_reconcile_blocking(
    db: &Db, sale_id: &str, record: &crate::db::SefLedgerRecord
) -> Result<String, String> {
    let session=crate::sef_bridge::Session::from_env().map_err(err_string)?;
    if session.context.as_deref()!=Some(record.context_id.as_str()) {
        return Err("Konteksti SEF i biznesit ndryshoi. Kuponi nuk mund të rikuperohet nga ky kontekst.".into());
    }
    let lookup=sef_call_blocking("sale.lookup",
        serde_json::json!({"saleId":sale_id}),crate::sef_bridge::DEFAULT_TIMEOUT)
        .map_err(|e|format!("Rikuperimi SEF nuk u konfirmua nga PROGRAMERI POS ({}). Mos e fiskalizo përsëri. Përditëso të dy aplikacionet nëse mungon sale.lookup.",e.code))?;
    let found=lookup["found"].as_bool()==Some(true);
    let status=lookup["status"].as_str().unwrap_or("UNKNOWN");
    if !found {
        if matches!(record.status.as_str(),"IN_FLIGHT"|"FAILED") {
            db.sef_ledger_transition(sale_id,"UNKNOWN","LOOKUP_UNCERTAIN",None,None,None)
                .map_err(err_string)?;
        }
        return Err("PROGRAMERI POS nuk konfirmoi një kupon për këtë faturë. Gjendja mbetet E PASIGURT; kontrollo historikun fiskal në POS para çdo veprimi.".into());
    }
    if status=="FAILED" {
        db.sef_ledger_transition(sale_id,"FAILED","POS_FAILED",None,None,Some("POS_FAILED"))
            .map_err(err_string)?;
        return Err("PROGRAMERI POS e raporton kuponin si të refuzuar përfundimisht. Kërko kontroll fiskal; mos e dërgo përsëri automatikisht.".into());
    }
    if !matches!(status,"FISCALIZED"|"PENDING_OFFLINE") {
        return Err("Statusi fiskal nga PROGRAMERI POS nuk është i konfirmuar.".into());
    }
    let coupon=crate::sef_bridge::parse_coupon_result(lookup["coupon"].clone()).map_err(err_string)?;
    if coupon.total_cents!=Some(record.total_cents) {
        return Err("Totali i kuponit në PROGRAMERI POS nuk përputhet me kërkesën e ruajtur. Kërko verifikim manual.".into());
    }
    if let Some(existing)=record.coupon_id.as_deref() {
        if existing!=coupon.coupon_id {
            return Err("Identiteti i kuponit ndryshoi. Fiskalizimi është bllokuar.".into());
        }
    }
    if db.sef_sale_coupon(sale_id).map_err(err_string)?.is_none() {
        // The app may have crashed after POS issued the coupon but before local commit.
        // Only apply the result if the current commercial projection still matches
        // the original immutable request. No new fiscalization is performed here.
        let (data,_,hash)=sef_build_sale(db,sale_id,record.allow_any)?;
        if hash!=record.request_sha256 {
            return Err("Të dhënat e faturës kanë ndryshuar pas kërkesës SEF. Kuponi ekziston në POS; kërko përputhje manuale, pa krijuar kupon tjetër.".into());
        }
        let info=crate::db::SefCouponInfo {
            coupon_id:coupon.coupon_id.clone(),daily_number:coupon.daily_number,
            status:status.to_string(),verification_url:coupon.verification_url
        };
        db.sef_mark_fiscalized(&data,record.total_cents as f64/100.0,&info)
            .map_err(|e|format!("Kuponi ekziston në POS, por regjistrimi lokal dështoi: {e}"))?;
    }
    db.sef_ledger_transition(sale_id,status,"POS_RECONCILED",
        Some(&coupon.coupon_id),coupon.daily_number,None).map_err(err_string)?;
    Ok(format!("Kuponi SEF {} u verifikua në PROGRAMERI POS ({}).",coupon.coupon_id,status))
}

/// Persist the fiscal-only snapshot BEFORE crossing the process boundary.
/// Any uncertain outcome is reconciled by sale.lookup; it is never blindly resent.
fn sef_fiscalize_blocking(db: &Db, sale_id: &str, allow_any: bool) -> Result<String, String> {
    let sale_id=sale_id.trim().to_string();
    if sale_id.is_empty(){return Err("sale_id eshte i detyrueshem".into());}
    let session=crate::sef_bridge::Session::from_env().map_err(err_string)?;
    let context=session.context.as_deref().filter(|s|!s.trim().is_empty())
        .ok_or_else(||"PROGRAMERI POS nuk dërgoi kontekstin fiskal të biznesit.".to_string())?;
    let _guard=sef_fiscal_lock().lock().unwrap_or_else(|p|p.into_inner());
    if let Some(existing)=db.sef_ledger_get(&sale_id).map_err(err_string)? {
        if existing.context_id!=context {
            return Err("Kjo faturë ka histori SEF në një kontekst tjetër. Fiskalizimi është bllokuar.".into());
        }
        if existing.status=="FISCALIZED" {
            return Ok("Kuponi SEF për këtë faturë është regjistruar më parë.".into());
        }
        if existing.status!="READY" {
            return sef_reconcile_blocking(db,&sale_id,&existing);
        }
    }
    let (data,req,hash)=sef_build_sale(db,&sale_id,allow_any)?;
    let issued_at=chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs,false);
    let params=crate::sef_bridge::fiscalize_params(&data.sale.id,&issued_at,&req);
    let payload=serde_json::to_string(&params).map_err(err_string)?;
    let record=db.sef_ledger_prepare(&sale_id,&hash,&payload,allow_any,req.total_cents,context)
        .map_err(err_string)?;
    if record.status!="READY" {
        return sef_reconcile_blocking(db,&sale_id,&record);
    }
    db.sef_ledger_transition(&sale_id,"IN_FLIGHT","DISPATCH",None,None,None)
        .map_err(err_string)?;
    let original_params:serde_json::Value=serde_json::from_str(&record.request_json).map_err(err_string)?;
    let response=sef_call_blocking("fiscalize",original_params,crate::sef_bridge::DEFAULT_TIMEOUT);
    let res=match response {
        Ok(res)=>res,
        Err(e)=>{
            let _=db.sef_ledger_transition(&sale_id,"UNKNOWN","BRIDGE_UNCERTAIN",
                None,None,Some(&e.code));
            return Err(format!("SEF nuk u konfirmua ({}). Gjendja u ruajt për rikuperim; mos e dërgo faturën përsëri pa verifikim.",e.code));
        }
    };
    let coupon=match crate::sef_bridge::parse_coupon_result(res) {
        Ok(c) if matches!(c.status.as_str(),"FISCALIZED"|"PENDING_OFFLINE") &&
            c.total_cents==Some(req.total_cents)=>c,
        _=>{
            let _=db.sef_ledger_transition(&sale_id,"UNKNOWN","INVALID_POS_RESULT",
                None,None,Some("PROTOCOL"));
            return Err("Përgjigjja e PROGRAMERI POS nuk konfirmon kupon të vlefshëm. Kontrollo POS para çdo tentative tjetër.".into());
        }
    };
    let info=crate::db::SefCouponInfo {
        coupon_id:coupon.coupon_id.clone(),daily_number:coupon.daily_number,
        status:coupon.status.clone(),verification_url:coupon.verification_url.clone()
    };
    if let Err(e)=db.sef_mark_fiscalized(&data,req.total_cents as f64/100.0,&info) {
        let _=db.sef_ledger_transition(&sale_id,"UNKNOWN","LOCAL_COMMIT_FAILED",
            None,None,Some("LOCAL_DB"));
        return Err(format!("Kuponi SEF {} u lëshua nga POS, por ruajtja lokale dështoi ({e}). Përdor Rikupero, jo fiskalizim të ri.",coupon.coupon_id));
    }
    db.sef_ledger_transition(&sale_id,&coupon.status,"POS_CONFIRMED",
        Some(&coupon.coupon_id),coupon.daily_number,None).map_err(err_string)?;
    let label=coupon.daily_number.map(|n|n.to_string()).unwrap_or(coupon.coupon_id);
    let mut msg=format!("Kuponi SEF nr. {label} u lëshua ({} €).",cents_to_text(req.total_cents));
    if coupon.status=="PENDING_OFFLINE" {
        msg.push_str(" Kuponi është ruajtur në POS dhe pret dërgimin në ATK.");
    }
    if coupon.duplicate==Some(true){msg.push_str(" Kupon ekzistues, pa dublim.");}
    Ok(msg)
}

async fn require_sef_provider(state: &tauri::State<'_, AppState>) -> Result<(), String> {
    let provider = get_fiscal_printer_provider(state).await?;
    if provider != crate::sef_bridge::PROVIDER_VALUE {
        return Err("Motori fiskal nuk është SEF (PROGRAMERI POS).".to_string());
    }
    Ok(())
}

fn sef_today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

async fn sef_report_summary(op: &str, date: Option<String>) -> Result<String, String> {
    let date = date
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty())
        .unwrap_or_else(sef_today);
    let r = crate::sef_bridge::call(
        op,
        serde_json::json!({ "date": date }),
        crate::sef_bridge::DEFAULT_TIMEOUT,
    )
    .await
    .map_err(err_string)?;
    let t = &r["totals"];
    let sales = t["salesCents"].as_i64().unwrap_or(0);
    let returns = t["returnsCents"].as_i64().unwrap_or(0);
    let count = t["count"].as_i64().unwrap_or(0);
    let name = if op == "report.z" { "Z" } else { "X" };
    Ok(format!(
        "{name} Raport SEF ({date}): shitje {} €, kthime {} €, kuponë {count}.",
        cents_to_text(sales),
        cents_to_text(returns)
    ))
}

#[derive(serde::Serialize)]
struct SefBridgeInfo {
    /// Diagnostic origin of this Mjeku process, never used for authorization.
    launch_mode: &'static str,
    /// Windows/Android: zgjedhja "SEF (PROGRAMERI POS)" shfaqet.
    supported: bool,
    /// Mjeku u nis nga PROGRAMERI POS (variablat e seancës ekzistojnë).
    launched: bool,
    context: Option<String>,
    provider_value: &'static str,
    provider: String,
}

#[tauri::command]
async fn sef_bridge_info(state: tauri::State<'_, AppState>) -> Result<SefBridgeInfo, String> {
    let _ = require_login(&state).await?;
    let provider = get_fiscal_printer_provider(&state).await?;
    let launch_mode = match std::env::var("PROGRAMERI_MJEKU_LAUNCH_MODE").as_deref() {
        Ok("sef-bridge") => "sef-bridge",
        Ok("local-token-no-sef") => "local-token-no-sef",
        Ok("pef-launch-only") => "pef-launch-only",
        Ok("legacy-no-bridge") => "legacy-no-bridge",
        _ => "standalone-or-unknown",
    };
    Ok(SefBridgeInfo {
        launch_mode,
        supported: crate::sef_bridge::platform_supported(),
        launched: crate::sef_bridge::launched_from_programeri(),
        context: crate::sef_bridge::context_id(),
        provider_value: crate::sef_bridge::PROVIDER_VALUE,
        provider,
    })
}

#[tauri::command]
async fn sef_bridge_status(state: tauri::State<'_, AppState>) -> Result<serde_json::Value, String> {
    let _ = require_login(&state).await?;
    crate::sef_bridge::call(
        "status",
        serde_json::json!({}),
        crate::sef_bridge::DEFAULT_TIMEOUT,
    )
    .await
    .map_err(err_string)
}

/// Zgjedh SEF (PROGRAMERI POS) si motor fiskal: hap konfigurimin SEF të PROGRAMERI POS
/// dhe e ruan motorin vetëm kur kthehet `ready:true`. Kur SEF është tashmë motori,
/// vetëm rihap konfigurimin dhe kthen gjendjen `ready`.
#[tauri::command]
async fn sef_bridge_select_engine(state: tauri::State<'_, AppState>) -> Result<bool, String> {
    let session = require_login(&state).await?;
    if matches!(
        session,
        SessionKind::User {
            role: crate::auth::UserRole::Cashier
        } | SessionKind::Doctor {
            is_admin: false,
            ..
        }
    ) {
        return Err("nuk ke akses per kete seksion".to_string());
    }
    if !crate::sef_bridge::platform_supported() {
        return Err(crate::sef_bridge::MSG_UNSUPPORTED_PLATFORM.to_string());
    }
    let current = get_fiscal_printer_provider(&state).await?;
    // PROGRAMERI POS dhe moduli Mjeku duhet të jenë aktivë (përgjigjet `status`).
    crate::sef_bridge::call(
        "status",
        serde_json::json!({}),
        crate::sef_bridge::DEFAULT_TIMEOUT,
    )
    .await
    .map_err(err_string)?;
    let r = crate::sef_bridge::call(
        "configure",
        serde_json::json!({}),
        crate::sef_bridge::CONFIGURE_TIMEOUT,
    )
    .await
    .map_err(err_string)?;
    let ready = r.get("ready").and_then(|x| x.as_bool()) == Some(true);
    if current == crate::sef_bridge::PROVIDER_VALUE {
        return Ok(ready);
    }
    if !ready {
        return Err(crate::sef_bridge::MSG_CONFIGURE_NOT_READY.to_string());
    }
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        db.setting_set(FISCAL_PRINTER_PROVIDER_KEY, crate::sef_bridge::PROVIDER_VALUE)
    })
    .await
    .map_err(err_string)?
    .map_err(err_string)?;
    Ok(true)
}

#[tauri::command]
async fn sef_ledger_list(state: tauri::State<'_, AppState>)
    -> Result<Vec<serde_json::Value>, String> {
    let _=require_finance(&state).await?;
    require_sef_provider(&state).await?;
    let db=state.db.clone();
    tokio::task::spawn_blocking(move || db.sef_ledger_list())
        .await.map_err(err_string)?.map_err(err_string)
}

/// Read-only POS lookup and local state repair. This command never issues a coupon.
#[tauri::command]
async fn sef_sale_reconcile(
    state: tauri::State<'_, AppState>, sale_id: String
) -> Result<String, String> {
    let _=require_finance(&state).await?;
    require_sef_provider(&state).await?;
    let sale_id=sale_id.trim().to_string();
    if sale_id.is_empty(){return Err("Zgjidh faturën për rikuperim.".into());}
    let db=state.db.clone();
    tokio::task::spawn_blocking(move || {
        let _guard=sef_fiscal_lock().lock().unwrap_or_else(|p|p.into_inner());
        let record=db.sef_ledger_get(&sale_id).map_err(err_string)?
            .ok_or_else(||"Nuk ka një tentativë SEF të regjistruar për këtë faturë.".to_string())?;
        if record.status=="READY" {
            return Err("Kjo faturë nuk është dërguar ende në PROGRAMERI POS.".into());
        }
        sef_reconcile_blocking(&db,&sale_id,&record)
    }).await.map_err(err_string)?
}

#[tauri::command]
async fn sef_coupons_list(
    state: tauri::State<'_, AppState>,
    from: Option<String>,
    to: Option<String>,
) -> Result<serde_json::Value, String> {
    let _ = require_finance(&state).await?;
    require_sef_provider(&state).await?;
    let mut params = serde_json::Map::new();
    if let Some(f) = from.map(|x| x.trim().to_string()).filter(|x| !x.is_empty()) {
        params.insert("from".to_string(), serde_json::Value::String(f));
    }
    if let Some(t) = to.map(|x| x.trim().to_string()).filter(|x| !x.is_empty()) {
        params.insert("to".to_string(), serde_json::Value::String(t));
    }
    crate::sef_bridge::call(
        "coupons.list",
        serde_json::Value::Object(params),
        crate::sef_bridge::DEFAULT_TIMEOUT,
    )
    .await
    .map_err(err_string)
}

#[tauri::command]
async fn sef_coupon_copy(
    state: tauri::State<'_, AppState>,
    coupon_id: String,
) -> Result<serde_json::Value, String> {
    let _ = require_finance(&state).await?;
    require_sef_provider(&state).await?;
    let coupon_id = coupon_id.trim().to_string();
    if coupon_id.is_empty() {
        return Err("Zgjidh kuponin.".to_string());
    }
    crate::sef_bridge::call(
        "coupon.copy",
        serde_json::json!({ "couponId": coupon_id }),
        crate::sef_bridge::DEFAULT_TIMEOUT,
    )
    .await
    .map_err(err_string)
}

#[derive(Debug, serde::Deserialize)]
struct SefReturnItemInput {
    id: String,
    qty: f64,
}

#[tauri::command]
async fn sef_coupon_return(
    state: tauri::State<'_, AppState>,
    coupon_id: String,
    items: Vec<SefReturnItemInput>,
    reason: String,
) -> Result<serde_json::Value, String> {
    let _ = require_finance(&state).await?;
    require_sef_provider(&state).await?;
    let coupon_id = coupon_id.trim().to_string();
    let reason = reason.trim().to_string();
    if coupon_id.is_empty() {
        return Err("Zgjidh kuponin.".to_string());
    }
    if reason.is_empty() {
        return Err("Shkruaj arsyen e kthimit.".to_string());
    }
    let mut out = Vec::new();
    for it in items {
        if !it.qty.is_finite() || it.qty <= 0.0 || it.id.trim().is_empty() {
            continue;
        }
        let milli = crate::sef_bridge::qty_to_milli(it.qty);
        if milli <= 0 {
            continue;
        }
        out.push(serde_json::json!({
            "id": it.id.trim(),
            "qty": crate::sef_bridge::qty_string(milli),
        }));
    }
    if out.is_empty() {
        return Err("Zgjidh të paktën një artikull dhe sasinë për kthim.".to_string());
    }
    crate::sef_bridge::call(
        "coupon.return",
        serde_json::json!({ "couponId": coupon_id, "items": out, "reason": reason }),
        crate::sef_bridge::DEFAULT_TIMEOUT,
    )
    .await
    .map_err(err_string)
}

#[tauri::command]
async fn sef_coupon_cancel(
    state: tauri::State<'_, AppState>,
    coupon_id: String,
    reason: String,
) -> Result<serde_json::Value, String> {
    let _ = require_finance(&state).await?;
    require_sef_provider(&state).await?;
    let coupon_id = coupon_id.trim().to_string();
    let reason = reason.trim().to_string();
    if coupon_id.is_empty() {
        return Err("Zgjidh kuponin.".to_string());
    }
    if reason.is_empty() {
        return Err("Shkruaj arsyen e anulimit.".to_string());
    }
    crate::sef_bridge::call(
        "coupon.cancel",
        serde_json::json!({ "couponId": coupon_id, "reason": reason }),
        crate::sef_bridge::DEFAULT_TIMEOUT,
    )
    .await
    .map_err(err_string)
}

/// Raporti X/Z i plotë (tekst + totale) për faqen "Kuponët".
#[tauri::command]
async fn sef_report(
    state: tauri::State<'_, AppState>,
    kind: String,
    date: Option<String>,
) -> Result<serde_json::Value, String> {
    let _ = require_finance(&state).await?;
    require_sef_provider(&state).await?;
    let op = match kind.trim().to_ascii_lowercase().as_str() {
        "x" => "report.x",
        "z" => "report.z",
        _ => return Err("Raporti duhet të jetë X ose Z.".to_string()),
    };
    let date = date
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty())
        .unwrap_or_else(sef_today);
    crate::sef_bridge::call(
        op,
        serde_json::json!({ "date": date }),
        crate::sef_bridge::DEFAULT_TIMEOUT,
    )
    .await
    .map_err(err_string)
}

/// Rreshtat e fiskalizuar të një shitjeje (për zgjedhjen e artikujve te "Kthe").
#[tauri::command]
async fn sef_sale_items(
    state: tauri::State<'_, AppState>,
    sale_id: String,
) -> Result<Vec<VisitItem>, String> {
    let _ = require_finance(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        db.visit_items_list(Some(VisitItemsListFilters {
            visit_id: Some(sale_id.trim().to_string()),
            client_id: None,
            include_deleted: Some(false),
        }))
        .map(|rows| {
            rows.into_iter()
                .filter(|x| x.deleted == 0 && x.fiscalized == 1)
                .collect::<Vec<_>>()
        })
    })
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

/// Kuponi SEF i ruajtur lokalisht te shitja (nëse ka).
#[tauri::command]
async fn sef_sale_coupon(
    state: tauri::State<'_, AppState>,
    sale_id: String,
) -> Result<Option<crate::db::SefCouponInfo>, String> {
    let _ = require_finance(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.sef_sale_coupon(&sale_id))
        .await
        .map_err(err_string)?
        .map_err(err_string)
}

#[tauri::command]
async fn prescriptions_list(
    state: tauri::State<'_, AppState>,
    kind: Option<String>,
    client_id: Option<String>,
) -> Result<Vec<crate::models::Prescription>, String> {
    let _ = require_login(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.prescriptions_list(kind, client_id))
        .await
        .map_err(err_string)?
        .map_err(err_string)
}

#[tauri::command]
async fn prescriptions_delete(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let _ = require_login(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.prescriptions_delete(&id))
        .await
        .map_err(err_string)?
        .map_err(err_string)
}

#[tauri::command]
async fn prescription_export_pdf(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    visit_id: Option<String>,
    kind: Option<String>,
    title: Option<String>,
    content: Option<String>,
    save: Option<bool>,
) -> Result<String, String> {
    let session = require_login(&state).await?;
    let visit_id = visit_id.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    let kind = match kind.as_deref().map(str::trim) {
        Some("udhezim") => "udhezim".to_string(),
        _ => "recete".to_string(),
    };
    let title = title.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
    let content_in = content.map(|c| c.trim().to_string()).filter(|c| !c.is_empty());
    let save = save.unwrap_or(false);

    let db = state.db.clone();
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| anyhow::anyhow!(e))
        .map_err(err_string)?;
    #[cfg(desktop)]
    let desktop_dir = app.path().desktop_dir().ok();
    #[cfg(mobile)]
    let app_for_save = app.clone();

    tokio::task::spawn_blocking(move || {
        let clinic_name = db
            .setting_get("clinic_name")
            .map_err(err_string)?
            .unwrap_or_else(|| "Klinika".to_string());
        let format = db
            .setting_get("print_format")
            .ok()
            .flatten()
            .map(|f| f.trim().to_lowercase())
            .filter(|f| matches!(f.as_str(), "a4" | "a5" | "termik"))
            .unwrap_or_else(|| "a4".to_string());

        let logo_png: Option<Vec<u8>> = db
            .setting_get("clinic_logo_path")
            .ok()
            .flatten()
            .and_then(|rel| {
                let rel = rel.trim().to_string();
                if rel.is_empty() { return None; }
                std::fs::read(data_dir.join(rel)).ok()
            });

        let today = chrono::Local::now().format("%Y-%m-%d").to_string();

        let (date, doctor_id_opt, client_opt, diagnosis, content_final, file_tag) = match &visit_id {
            Some(vid) => {
                let visit = db
                    .visits_get(vid)
                    .map_err(err_string)?
                    .ok_or_else(|| "vizita nuk u gjet".to_string())?;
                if let SessionKind::Doctor { doctor_id, is_admin: false } = &session {
                    let row_doctor = visit.doctor_id.as_deref().unwrap_or("").trim().to_string();
                    if row_doctor.is_empty() || row_doctor != doctor_id.trim() {
                        return Err("nuk ke akses ne kete vizite".to_string());
                    }
                }
                let client = db
                    .clients_get(&visit.client_id)
                    .map_err(err_string)?
                    .ok_or_else(|| "pacienti nuk u gjet".to_string())?;
                let content = content_in.clone().or_else(|| visit.therapies.clone());
                (
                    visit.date.clone().unwrap_or_else(|| today.clone()),
                    visit.doctor_id.clone(),
                    Some(client),
                    visit.diagnosis.clone(),
                    content,
                    format!("{}-{}", if kind == "udhezim" { "Udhezim" } else { "Receta" }, visit.id),
                )
            }
            None => {
                let did = match &session {
                    SessionKind::Doctor { doctor_id, .. } => Some(doctor_id.clone()),
                    _ => None,
                };
                let ts = chrono::Local::now().format("%H%M%S").to_string();
                (
                    today.clone(),
                    did,
                    None,
                    None,
                    content_in.clone(),
                    format!("{}-{}", if kind == "udhezim" { "Udhezim" } else { "Receta" }, ts),
                )
            }
        };

        let (doctor_name, doctor_title, doctor_specialty) = if let Some(did) = doctor_id_opt
            .as_deref()
            .map(str::trim)
            .filter(|x| !x.is_empty())
        {
            db.doctors_get(did)
                .ok()
                .flatten()
                .map(|d| (Some(d.name), d.title, d.specialty))
                .unwrap_or((None, None, None))
        } else {
            (None, None, None)
        };

        // Ruaje ne DB (sinkronizohet ne background) nese kerkohet.
        if save {
            let now = crate::util::now_iso();
            let row = crate::models::Prescription {
                id: uuid::Uuid::new_v4().to_string(),
                visit_id: visit_id.clone(),
                client_id: client_opt.as_ref().map(|c| c.id.clone()),
                doctor_id: doctor_id_opt.clone(),
                kind: kind.clone(),
                title: title.clone().unwrap_or_default(),
                content: content_final.clone().unwrap_or_default(),
                created_at: now.clone(),
                updated_at: now,
                deleted: 0,
            };
            db.prescriptions_upsert(&row).map_err(err_string)?;
        }

        let data = crate::invoice::PrescriptionPdfData {
            clinic_name,
            logo_png,
            format,
            kind: kind.clone(),
            doc_title: title.clone(),
            date,
            doctor_name,
            doctor_title,
            doctor_specialty,
            client_name: client_opt.as_ref().map(|c| c.name.clone()),
            client_dob: client_opt.as_ref().and_then(|c| c.dob.clone()),
            client_code: client_opt.as_ref().and_then(|c| c.patient_code.clone()),
            diagnosis,
            content: content_final,
        };

        let pdf_bytes = crate::invoice::render_prescription_pdf(&data).map_err(err_string)?;

        #[cfg(mobile)]
        {
            return save_pdf_via_dialog(&app_for_save, &format!("{}.pdf", file_tag), &pdf_bytes);
        }
        #[cfg(desktop)]
        {
            let out_dir = desktop_dir.unwrap_or_else(|| data_dir.join("temp"));
            std::fs::create_dir_all(&out_dir).map_err(err_string)?;
            let out_path = out_dir.join(format!("{}.pdf", file_tag));
            std::fs::write(&out_path, &pdf_bytes).map_err(err_string)?;
            Ok(out_path.display().to_string())
        }
    })
    .await
    .map_err(err_string)?
}

#[tauri::command]
async fn error_logs_list(
    state: tauri::State<'_, AppState>,
    limit: Option<u32>,
) -> Result<Vec<crate::error_logs::ErrorLogEntry>, String> {
    let _ = require_logs_admin(&state).await?;
    let path = state.error_log_path.clone();
    let lim = limit.unwrap_or(500).clamp(1, 5000) as usize;
    tokio::task::spawn_blocking(move || crate::error_logs::list(&path, lim))
        .await
        .map_err(err_string)?
        .map_err(err_string)
}

#[tauri::command]
async fn error_logs_clear(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let _ = require_logs_admin(&state).await?;
    let path = state.error_log_path.clone();
    tokio::task::spawn_blocking(move || crate::error_logs::clear(&path))
        .await
        .map_err(err_string)?
        .map_err(err_string)
}

#[tauri::command]
async fn regular_invoice_create(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    sale_id: String,
) -> Result<crate::models::RegularInvoice, String> {
    require_finance(&state).await?;
    let sale_id = sale_id.trim().to_string();
    if sale_id.is_empty() {
        return Err("sale_id eshte i detyrueshem".to_string());
    }
    let db = state.db.clone();
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| anyhow::anyhow!(e))
        .map_err(err_string)?;
    #[cfg(desktop)]
    let desktop_dir = app.path().desktop_dir().ok();
    #[cfg(mobile)]
    let desktop_dir: Option<PathBuf> = app.path().document_dir().ok();

    tokio::task::spawn_blocking(move || {
        let clinic_name = db
            .setting_get("clinic_name")
            .ok()
            .flatten()
            .unwrap_or_else(|| "Klinika".to_string());
        let bank_account = db.setting_get("clinic_bank_account").ok().flatten()
            .filter(|s| !s.trim().is_empty());
        let header_png: Option<Vec<u8>> = db
            .setting_get("pdf_header_path")
            .ok()
            .flatten()
            .and_then(|rel| {
                let rel = rel.trim().to_string();
                if rel.is_empty() { return None; }
                std::fs::read(data_dir.join(rel)).ok()
            });
        let logo_png: Option<Vec<u8>> = db
            .setting_get("clinic_logo_path")
            .ok()
            .flatten()
            .and_then(|rel| {
                let rel = rel.trim().to_string();
                if rel.is_empty() { return None; }
                std::fs::read(data_dir.join(rel)).ok()
            });

        let sale = db.sales_get(&sale_id).map_err(err_string)?
            .ok_or_else(|| "fatura nuk u gjet".to_string())?;
        let invoice_no = db.sales_invoice_number(&sale.id).unwrap_or_else(|_| sale.id.clone());
        let client = db.clients_get(&sale.client_id).map_err(err_string)?
            .ok_or_else(|| "pacienti nuk u gjet".to_string())?;

        let vis_items = db.visit_items_list(Some(crate::models::VisitItemsListFilters {
            visit_id: Some(sale_id.clone()),
            client_id: None,
            include_deleted: Some(false),
        })).map_err(err_string)?;
        let lines: Vec<crate::invoice::InvoiceLine> = vis_items
            .into_iter()
            .filter(|x| x.deleted == 0)
            .map(|it| crate::invoice::InvoiceLine {
                tooth: it.tooth,
                title: it.title,
                qty: it.qty,
                unit_price: it.unit_price,
                fiscal: it.fiscal == 1,
                vat_code: it.vat_code,
            })
            .collect();

        let (total, fiscal_total, non_fiscal_total) = if lines.is_empty() {
            (sale.total, sale.total, 0.0)
        } else {
            let mut t = 0.0_f64; let mut ft = 0.0_f64; let mut nft = 0.0_f64;
            for ln in &lines {
                let sub = ln.qty * ln.unit_price;
                t += sub;
                if ln.fiscal { ft += sub; } else { nft += sub; }
            }
            (t, ft, nft)
        };

        let data = crate::invoice::InvoicePdfData {
            clinic_name,
            header_png,
            logo_png,
            invoice_id: invoice_no.clone(),
            date: sale.date.clone(),
            client_name: client.name.clone(),
            client_code: client.patient_code.clone(),
            client_dob: client.dob.clone(),
            client_address: client.address.clone(),
            client_city: client.city.clone(),
            client_phone: client.phone.clone(),
            client_email: client.email.clone(),
            notes: sale.notes.clone(),
            bank_account,
            lines,
            total,
            fiscal_total,
            non_fiscal_total,
        };

        let pdf_bytes = crate::invoice::render_invoice_pdf(&data).map_err(err_string)?;

        // Save to Desktop/Faturat_Rregullta/
        let out_dir = desktop_dir
            .map(|d| d.join("Faturat_Rregullta"))
            .unwrap_or_else(|| data_dir.join("Faturat_Rregullta"));
        std::fs::create_dir_all(&out_dir).map_err(err_string)?;
        let filename = format!("Fature-Rregullt-{}.pdf", sale.id);
        let out_path = out_dir.join(&filename);
        std::fs::write(&out_path, &pdf_bytes).map_err(err_string)?;

        // Record in DB.
        let id = uuid::Uuid::new_v4().to_string();
        let now = crate::util::now_iso();
        db.regular_invoice_insert(
            &id,
            &sale_id,
            Some(&invoice_no),
            Some(&sale.client_id),
            Some(&client.name),
            sale.date.as_deref(),
            total,
            Some(&filename),
            &now,
        ).map_err(err_string)?;

        Ok(crate::models::RegularInvoice {
            id,
            sale_id,
            invoice_number: Some(invoice_no),
            client_id: Some(sale.client_id),
            client_name: Some(client.name),
            date: sale.date,
            total,
            pdf_filename: Some(filename),
            created_at: now,
        })
    })
    .await
    .map_err(err_string)?
}

#[tauri::command]
async fn regular_invoices_list(
    state: tauri::State<'_, AppState>,
    date_from: Option<String>,
    date_to: Option<String>,
) -> Result<Vec<crate::models::RegularInvoice>, String> {
    require_finance(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        db.regular_invoices_list(
            date_from.as_deref(),
            date_to.as_deref(),
        ).map_err(err_string)
    })
    .await
    .map_err(err_string)?
}

#[tauri::command]
fn app_restart(app: tauri::AppHandle) {
    app.restart();
}

#[tauri::command]
async fn sales_monthly_report(state: tauri::State<'_, AppState>, year: i32) -> Result<Vec<crate::models::MonthlyReportRow>, String> {
    let _ = require_finance(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.sales_monthly_report(year))
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn stock_suppliers_list(state: tauri::State<'_, AppState>) -> Result<Vec<crate::models::StockSupplier>, String> {
    let _ = require_login(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.stock_suppliers_list())
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn stock_supplier_upsert(state: tauri::State<'_, AppState>, supplier: crate::models::StockSupplier) -> Result<(), String> {
    let _ = require_login(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.stock_supplier_upsert(&supplier))
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn stock_supplier_delete(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    let _ = require_login(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.stock_supplier_delete(&id))
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn stock_items_list(state: tauri::State<'_, AppState>) -> Result<Vec<crate::models::StockItem>, String> {
    let _ = require_login(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.stock_items_list())
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn stock_item_upsert(state: tauri::State<'_, AppState>, item: crate::models::StockItem) -> Result<(), String> {
    let _ = require_login(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.stock_item_upsert(&item))
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn stock_item_delete(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    let _ = require_login(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.stock_item_delete(&id))
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn stock_movements_list(state: tauri::State<'_, AppState>, item_id: String) -> Result<Vec<crate::models::StockMovement>, String> {
    let _ = require_login(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.stock_movements_list(&item_id))
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn stock_movement_add(state: tauri::State<'_, AppState>, movement: crate::models::StockMovement) -> Result<(), String> {
    let _ = require_login(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.stock_movement_add(&movement))
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn sale_items_list(state: tauri::State<'_, AppState>, sale_id: String) -> Result<Vec<crate::models::SaleItem>, String> {
    let _ = require_finance(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.sale_items_list(&sale_id))
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn sale_items_replace(state: tauri::State<'_, AppState>, sale_id: String, items: Vec<crate::models::SaleItemInput>) -> Result<(), String> {
    let _ = require_finance(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.sale_items_replace(&sale_id, &items))
        .await.map_err(err_string)?.map_err(err_string)
}

// ─── Offer commands ───────────────────────────────────────────────────────────

#[tauri::command]
async fn offers_list(state: tauri::State<'_, AppState>) -> Result<Vec<crate::models::Offer>, String> {
    let _ = require_finance(&state).await?;
    let db = state.db.clone();
    let clinic_id = tokio::task::spawn_blocking({
        let db = db.clone();
        move || db.setting_get("clinic_id")
    }).await.map_err(err_string)?.map_err(err_string)?.unwrap_or_default();
    tokio::task::spawn_blocking(move || db.offers_list(&clinic_id))
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn offer_items_list(state: tauri::State<'_, AppState>, offer_id: String) -> Result<Vec<crate::models::OfferItem>, String> {
    let _ = require_finance(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.offer_items_list(&offer_id))
        .await.map_err(err_string)?.map_err(err_string)
}

#[tauri::command]
async fn offer_upsert(
    state: tauri::State<'_, AppState>,
    client_id: String,
    status: String,
    valid_until: Option<String>,
    notes: Option<String>,
    vat_pct: f64,
    items: Vec<crate::models::OfferItemInput>,
    id: Option<String>,
    source_offer_id: Option<String>,
) -> Result<crate::models::Offer, String> {
    let _ = require_finance(&state).await?;
    let db = state.db.clone();
    let now = crate::util::now_iso();

    let clinic_id = tokio::task::spawn_blocking({
        let db = db.clone();
        move || db.setting_get("clinic_id")
    }).await.map_err(err_string)?.map_err(err_string)?.unwrap_or_default();

    // Compute totals
    let subtotal_raw: f64 = items.iter().map(|it| it.qty * it.unit_price * (1.0 - it.discount_pct / 100.0)).sum();
    let subtotal = (subtotal_raw * 100.0).round() / 100.0;
    let vat_amount = (subtotal * vat_pct / 100.0 * 100.0).round() / 100.0;
    let total = (subtotal + vat_amount) * 100.0 / 100.0;
    let total = (total * 100.0).round() / 100.0;

    let offer_id = id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let is_new = id.is_none();

    // Get offer_number
    let offer_number = if is_new {
        let db2 = db.clone();
        let now_dt = chrono::Utc::now();
        let month = now_dt.month();
        let year = now_dt.year();
        tokio::task::spawn_blocking(move || db2.next_offer_number(month, year))
            .await.map_err(err_string)?.map_err(err_string)?
    } else {
        let db2 = db.clone();
        let oid = offer_id.clone();
        tokio::task::spawn_blocking(move || db2.offer_get(&oid))
            .await.map_err(err_string)?.map_err(err_string)?
            .map(|o| o.offer_number)
            .unwrap_or_else(|| "000/00/0000".to_string())
    };

    let created_at = if is_new {
        now.clone()
    } else {
        let db2 = db.clone();
        let oid = offer_id.clone();
        tokio::task::spawn_blocking(move || db2.offer_get(&oid))
            .await.map_err(err_string)?.map_err(err_string)?
            .map(|o| o.created_at)
            .unwrap_or_else(|| now.clone())
    };

    let offer = crate::models::Offer {
        id: offer_id.clone(),
        clinic_id: clinic_id.clone(),
        client_id,
        offer_number,
        status,
        valid_until,
        notes,
        vat_pct,
        subtotal,
        vat_amount,
        total,
        invoice_id: None,
        source_offer_id,
        created_at,
        updated_at: now.clone(),
        deleted_at: None,
    };

    let db2 = db.clone();
    let offer2 = offer.clone();
    tokio::task::spawn_blocking(move || db2.offer_upsert(&offer2))
        .await.map_err(err_string)?.map_err(err_string)?;

    let db3 = db.clone();
    let now2 = crate::util::now_iso();
    let oid = offer_id.clone();
    let cid = clinic_id.clone();
    tokio::task::spawn_blocking(move || db3.offer_items_replace(&oid, &cid, &items, &now2))
        .await.map_err(err_string)?.map_err(err_string)?;

    // Queue sync for offer
    let db4 = db.clone();
    let offer3 = offer.clone();
    tokio::task::spawn_blocking(move || db4.offer_queue_upsert(&offer3))
        .await.map_err(err_string)?.map_err(err_string)?;

    // Queue sync for items
    let db5 = db.clone();
    let oid2 = offer_id.clone();
    let updated_items = tokio::task::spawn_blocking(move || db5.offer_items_list(&oid2))
        .await.map_err(err_string)?.map_err(err_string)?;
    let db6 = db.clone();
    tokio::task::spawn_blocking(move || {
        for item in &updated_items {
            db6.offer_item_queue_upsert(item)?;
        }
        Ok::<_, anyhow::Error>(())
    }).await.map_err(err_string)?.map_err(err_string)?;

    Ok(offer)
}

#[tauri::command]
async fn offer_delete(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    let _ = require_finance(&state).await?;
    let db = state.db.clone();
    let now = crate::util::now_iso();
    let id2 = id.clone();
    let now2 = now.clone();
    tokio::task::spawn_blocking(move || db.offer_soft_delete(&id2, &now2))
        .await.map_err(err_string)?.map_err(err_string)?;
    // Queue a soft-delete by re-fetching the row and queuing it
    let db2 = state.db.clone();
    let deleted_offer = tokio::task::spawn_blocking(move || db2.offer_get(&id))
        .await.map_err(err_string)?.map_err(err_string)?;
    if let Some(offer) = deleted_offer {
        let db3 = state.db.clone();
        tokio::task::spawn_blocking(move || db3.offer_queue_upsert(&offer))
            .await.map_err(err_string)?.map_err(err_string)?;
    }
    Ok(())
}

#[tauri::command]
async fn offer_set_status(
    state: tauri::State<'_, AppState>,
    id: String,
    status: String,
    invoice_id: Option<String>,
) -> Result<(), String> {
    let _ = require_finance(&state).await?;
    let db = state.db.clone();
    let now = crate::util::now_iso();
    let id2 = id.clone();
    let status2 = status.clone();
    let invoice_id2 = invoice_id.clone();
    let now2 = now.clone();
    tokio::task::spawn_blocking(move || db.offer_set_status(&id2, &status2, invoice_id2.as_deref(), &now2))
        .await.map_err(err_string)?.map_err(err_string)?;
    // Queue sync
    let db2 = state.db.clone();
    let updated_offer = tokio::task::spawn_blocking(move || db2.offer_get(&id))
        .await.map_err(err_string)?.map_err(err_string)?;
    if let Some(offer) = updated_offer {
        let db3 = state.db.clone();
        tokio::task::spawn_blocking(move || db3.offer_queue_upsert(&offer))
            .await.map_err(err_string)?.map_err(err_string)?;
    }
    Ok(())
}

#[tauri::command]
async fn offer_pdf(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    offer_id: String,
) -> Result<String, String> {
    let _ = require_finance(&state).await?;
    #[cfg(desktop)]
    let desktop_dir = app.path().desktop_dir().ok();
    #[cfg(desktop)]
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| anyhow::anyhow!(e))
        .map_err(err_string)?;
    let db = state.db.clone();
    let oid = offer_id.clone();
    let offer = tokio::task::spawn_blocking(move || db.offer_get(&oid))
        .await.map_err(err_string)?.map_err(err_string)?
        .ok_or_else(|| "Oferta nuk u gjet".to_string())?;

    let db2 = state.db.clone();
    let oid2 = offer_id.clone();
    let items = tokio::task::spawn_blocking(move || db2.offer_items_list(&oid2))
        .await.map_err(err_string)?.map_err(err_string)?;

    let db3 = state.db.clone();
    let client_id = offer.client_id.clone();
    let (clinic_name, clinic_address, clinic_phone, header_png, logo_png, client_name, client_phone, client_email) =
        tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            let clinic_name = db3.setting_get("clinic_name")?.unwrap_or_default();
            let clinic_address = db3.setting_get("clinic_address")?;
            let clinic_phone = db3.setting_get("clinic_phone")?;
            let decode = |key: &str| -> Option<Vec<u8>> {
                db3.setting_get(key).ok().flatten()
                    .and_then(|s| base64::Engine::decode(&base64::engine::general_purpose::STANDARD, s.trim()).ok())
            };
            let header_png = decode("header_image_b64");
            let logo_png = decode("logo_image_b64");
            let client = db3.clients_get(&client_id)?;
            let client_name = client.as_ref().map(|c| c.name.clone()).unwrap_or_default();
            let client_phone = client.as_ref().and_then(|c| c.phone.clone());
            let client_email = client.as_ref().and_then(|c| c.email.clone());
            Ok((clinic_name, clinic_address, clinic_phone, header_png, logo_png, client_name, client_phone, client_email))
        }).await.map_err(err_string)?.map_err(err_string)?;

    let pdf_data = crate::invoice::OfferPdfData {
        clinic_name,
        clinic_address,
        clinic_phone,
        header_png,
        logo_png,
        offer_number: offer.offer_number.clone(),
        date: offer.created_at.get(..10).unwrap_or("").to_string(),
        valid_until: offer.valid_until.clone(),
        client_name,
        client_phone,
        client_email,
        notes: offer.notes.clone(),
        lines: items.iter().map(|it| crate::invoice::OfferPdfLine {
            description: it.description.clone(),
            qty: it.qty,
            unit_price: it.unit_price,
            discount_pct: it.discount_pct,
            line_total: it.line_total,
        }).collect(),
        vat_pct: offer.vat_pct,
        subtotal: offer.subtotal,
        vat_amount: offer.vat_amount,
        total: offer.total,
    };

    let pdf_bytes = crate::invoice::generate_offer_pdf(&pdf_data).map_err(err_string)?;

    // Ruaje gjithmone ne disk qe eksporti te funksionoje edhe ne native mobile, ku
    // shkarkimi permes Blob ne WebView s'eshte i besueshem. Kthimi mbetet base64:
    // `print()` ne OffersPage.tsx e perdor per parapamje ne blob.
    #[cfg(desktop)]
    {
        let out_dir = desktop_dir.unwrap_or_else(|| data_dir.join("temp"));
        std::fs::create_dir_all(&out_dir).map_err(err_string)?;
        let out_path = out_dir.join(format!("Oferta-{}.pdf", offer.offer_number.replace('/', "-")));
        std::fs::write(&out_path, &pdf_bytes).map_err(err_string)?;
    }
    #[cfg(mobile)]
    {
        let app_for_save = app.clone();
        let pdf_bytes2 = pdf_bytes.clone();
        let file_name = format!("Oferta-{}.pdf", offer.offer_number.replace('/', "-"));
        let _ = tokio::task::spawn_blocking(move || save_pdf_via_dialog(&app_for_save, &file_name, &pdf_bytes2))
            .await
            .map_err(err_string)?;
    }

    Ok(base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &pdf_bytes))
}

#[tauri::command]
async fn client_photos_list(
    state: tauri::State<'_, AppState>,
    client_id: String,
) -> Result<Vec<ClientPhoto>, String> {
    let _ = require_login(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.client_photos_list(&client_id))
        .await
        .map_err(err_string)?
        .map_err(err_string)
}

#[tauri::command]
async fn client_photo_add(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    client_id: String,
    stage: String,
    label: Option<String>,
    base64_data: String,
    ext: Option<String>,
) -> Result<ClientPhoto, String> {
    let _ = require_login(&state).await?;
    if client_id.trim().is_empty() {
        return Err("client_id eshte i detyrueshem".to_string());
    }
    let ext = ext
        .map(|e| e.trim().trim_start_matches('.').to_lowercase())
        .filter(|e| matches!(e.as_str(), "png" | "jpg" | "jpeg" | "webp"))
        .unwrap_or_else(|| "jpg".to_string());
    let stage = match stage.trim() {
        "after" => "after",
        "other" => "other",
        _ => "before",
    }
    .to_string();

    let bytes = {
        use base64::{engine::general_purpose, Engine as _};
        general_purpose::STANDARD
            .decode(base64_data.trim())
            .map_err(err_string)?
    };
    if bytes.is_empty() {
        return Err("Foto e zbrazet".to_string());
    }
    if bytes.len() > 15 * 1024 * 1024 {
        return Err("Foto shume e madhe (max 15MB)".to_string());
    }

    let data_dir = app.path().app_data_dir().map_err(err_string)?;
    let dir = data_dir.join("photos").join(client_id.trim());
    std::fs::create_dir_all(&dir).map_err(err_string)?;
    let id = uuid::Uuid::new_v4().to_string();
    let file_path = dir.join(format!("{id}.{ext}"));
    std::fs::write(&file_path, &bytes).map_err(err_string)?;

    let row = ClientPhoto {
        id,
        client_id: client_id.trim().to_string(),
        stage,
        label: label.unwrap_or_default().trim().to_string(),
        file_path: file_path.display().to_string(),
        taken_at: None,
        created_at: chrono::Utc::now().to_rfc3339(),
        deleted: 0,
    };
    let db = state.db.clone();
    let row2 = row.clone();
    tokio::task::spawn_blocking(move || db.client_photo_add(&row2))
        .await
        .map_err(err_string)?
        .map_err(err_string)?;
    Ok(row)
}

#[tauri::command]
async fn client_photo_data(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<String, String> {
    let _ = require_login(&state).await?;
    let db = state.db.clone();
    let photo = tokio::task::spawn_blocking(move || db.client_photo_get(&id))
        .await
        .map_err(err_string)?
        .map_err(err_string)?
        .ok_or_else(|| "Foto nuk u gjet".to_string())?;
    let bytes = std::fs::read(&photo.file_path).map_err(err_string)?;
    use base64::{engine::general_purpose, Engine as _};
    Ok(general_purpose::STANDARD.encode(bytes))
}

#[tauri::command]
async fn client_photo_delete(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let _ = require_login(&state).await?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.client_photo_delete(&id))
        .await
        .map_err(err_string)?
        .map_err(err_string)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .register_uri_scheme_protocol("mjeku", ui_protocol::handle)
        .setup(|app| {
            #[cfg(desktop)]
            app.handle().plugin(tauri_plugin_updater::Builder::new().build())?;
            let handle = app.handle();
            let data_dir = handle
                .path()
                .app_data_dir()
                .map_err(|e| anyhow::anyhow!(e))?;

            // Clinic reset is applied on startup (safe) so we can delete the SQLite file while it's closed.
            let reset_flag = data_dir.join("reset.flag");
            if reset_flag.exists() {
                let _ = std::fs::remove_file(data_dir.join("mjeku.sqlite3"));
                let _ = std::fs::remove_dir_all(data_dir.join("ui"));
                let _ = std::fs::remove_dir_all(data_dir.join("fiscal"));
                let _ = std::fs::remove_dir_all(data_dir.join("assets"));
                let _ = std::fs::remove_file(&reset_flag);
            }
            // "Demo Mode" (mobile-only, per App Store review requirement): kur eshte
            // futur token-i special (shih provision_apply_token) ne nje ekzekutim te
            // meparshem, marker-i eshte shkruar ketu dhe app-i eshte rinisur - baza e
            // te dhenave hapet ne memorie (asgje s'shkruhet ne disk, zhduket kur mbyllet
            // procesi) dhe klinika demo konfigurohet automatikisht me login te hapur.
            #[cfg(mobile)]
            let is_demo_mode = data_dir.join("demo_mode.flag").exists();
            #[cfg(not(mobile))]
            let is_demo_mode = false;

            let db = if is_demo_mode {
                Arc::new(Db::new_in_memory()?)
            } else {
                Arc::new(Db::new(data_dir.join("mjeku.sqlite3"))?)
            };
            if is_demo_mode && !crate::auth::is_configured(&db)? {
                crate::auth::setup_v2(
                    &db,
                    "Demo Klinika PROGRAMERI",
                    "demo123456",
                    "demo1234",
                    None,
                )?;
            }
            if db
                .setting_get(FISCAL_PRINTER_PROVIDER_KEY)?
                .unwrap_or_default()
                .trim()
                .is_empty()
            {
                db.setting_set(FISCAL_PRINTER_PROVIDER_KEY, "enternet")?;
            }

            // Auto-fix typoed URL if it was saved in DB from a previous run (startup check)
            let current_db_url = db.setting_get(KEY_SUPABASE_URL)?.unwrap_or_default();
            if current_db_url == "https://occzpryzxabajtmdaas.supabase.co" {
                db.setting_set(KEY_SUPABASE_URL, DEFAULT_SUPABASE_URL)?;
            }

            // Auto-fix future timestamps (clock skew recovery)
            // If the stored timestamp is way in the future (e.g. > 24h from now), reset it to allow re-check.
            let now_utc = Utc::now();
            for key in ["license_last_checked_at", "license_last_seen_device_time"] {
                if let Ok(Some(val)) = db.setting_get(key) {
                    if let Ok(ts_utc) = crate::util::parse_rfc3339_to_utc(&val) {
                        if ts_utc > now_utc + ChronoDuration::days(1) {
                            db.setting_set(key, "")?;
                        }
                    }
                }
            }

            ensure_setting_if_empty(db.as_ref(), KEY_SUPABASE_URL, DEFAULT_SUPABASE_URL)?;
            ensure_setting_if_empty(db.as_ref(), KEY_SUPABASE_API_KEY, DEFAULT_SUPABASE_API_KEY)?;
            ensure_setting_if_empty(db.as_ref(), KEY_SUPABASE_ANON_KEY, DEFAULT_SUPABASE_API_KEY)?;
            ensure_setting_if_empty(db.as_ref(), "update_base_url", DEFAULT_UPDATE_BASE_URL)?;
            // Migrim: klientët ekzistues e kishin "update_base_url" të vendosur eksplicit
            // (jo bosh) te Vercel-i i vjetër - ensure_setting_if_empty s'e prek atë. E
            // kalojmë te Render nëse ende ka vlerën default të vjetër (jo një URL të
            // personalizuar nga useri).
            if let Ok(Some(cur)) = db.setting_get("update_base_url") {
                if cur.trim().trim_end_matches('/') == OLD_DEFAULT_UPDATE_BASE_URL {
                    let _ = db.setting_set("update_base_url", DEFAULT_UPDATE_BASE_URL);
                }
            }
            if let Err(e) = db.fiscal_clear_article_on_app_open() {
                eprintln!("startup clear article failed: {e}");
            }
            let error_log_path = data_dir.join("logs").join("errors.log");
            db.setting_set("error_log_path", &error_log_path.display().to_string())?;
            let has_visit_service = db.services_list(None)?.into_iter().any(|s| {
                let t = s.title.trim().to_ascii_lowercase().replace('ë', "e");
                t == "vizite" || t == "vizita"
            });
            if !has_visit_service {
                let vat_registered = db
                    .setting_get("clinic_vat_registered")?
                    .unwrap_or_default()
                    .trim()
                    == "1";
                db.services_upsert(ServiceUpsertInput {
                    id: None,
                    title: "Vizitë".to_string(),
                    default_price: 0.0,
                    vat_code: Some(if vat_registered { "A" } else { "C" }.to_string()),
                    notes: Some("Shërbim bazë i vizitës".to_string()),
                })?;
            }

            let session = if is_demo_mode {
                // Reviewer-i futet direkt ne app, pa pasur nevoje te shkruaje asgje.
                SessionKind::User { role: crate::auth::UserRole::Owner }
            } else {
                crate::auth::session_get(db.as_ref()).unwrap_or(SessionKind::None)
            };
            let auth = Arc::new(AuthState::new(session));
            let sync = Arc::new(SyncEngine::new(db.clone())?);
            let updates = Arc::new(UpdatesEngine::new(db.clone())?);
            let license = Arc::new(LicenseEngine::new(db.clone())?);

            UpdatesEngine::apply_pending_on_startup(&handle)?;
            UpdatesEngine::ensure_seed_installed(&handle)?;

            // Demo mode s'ka clinic_id/license real dhe s'duhet te kontaktoje asnje
            // server - sync/license mbeten krejtesisht te fikur.
            if !is_demo_mode {
                sync.clone().spawn_background();
            }

            // Procesori i radhës fiskale: çdo 15s proceson job-et 'pending' më të reja se 2 orë.
            // Më të vjetrat presin aprovim manual te Sporteli (mbrojtje nga printimet aksidentale).
            #[cfg(all(desktop, target_os = "windows"))]
            {
                let db_fj = db.clone();
                tauri::async_runtime::spawn(async move {
                    let mut tick = tokio::time::interval(Duration::from_secs(15));
                    loop {
                        tick.tick().await;
                        let db2 = db_fj.clone();
                        let _ = tokio::task::spawn_blocking(move || {
                            let jobs = match db2.fiscal_jobs_list(Some("pending".to_string())) {
                                Ok(j) => j,
                                Err(_) => return,
                            };
                            for job in jobs {
                                if fiscal_job_age_secs(&job.created_at) > FISCAL_JOB_AUTO_MAX_AGE_SECS {
                                    continue; // kërkon aprovim manual
                                }
                                match fiscal_job_process_inner(&db2, &job) {
                                    Ok(()) => { let _ = fiscal_job_mark(&db2, &job, "done", None); }
                                    Err(e) => { let _ = fiscal_job_mark(&db2, &job, "failed", Some(e)); }
                                }
                            }
                        })
                        .await;
                    }
                });
            }
            updates.clone().spawn_background(handle.clone());
            if !is_demo_mode {
                license.clone().spawn_background();
            }
            {
                let db_bg = db.clone();
                let handle_bg = handle.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = desktop_updates_check_and_store(&handle_bg, db_bg.clone(), false).await;
                    let mut interval = tokio::time::interval(Duration::from_secs(6 * 60 * 60));
                    loop {
                        interval.tick().await;
                        let _ =
                            desktop_updates_check_and_store(&handle_bg, db_bg.clone(), false).await;
                    }
                });
            }

            app.manage(AppState {
                db,
                auth,
                sync,
                updates,
                license,
                error_log_path,
                lab_serial: Arc::new(lab_devices::LabSerialManager::new()),
                is_demo_mode,
            });

            // Silent background updater: check once on startup, download+install, then restart.
            #[cfg(desktop)]
            if !tauri::is_dev() {
                let handle_upd = handle.clone();
                tauri::async_runtime::spawn(async move {
                    // Wait 30 s so the app is fully ready before hitting the network.
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    if let Ok(updater) = handle_upd.updater() {
                        if let Ok(Some(update)) = updater.check().await {
                            let new_version = update.version.clone();
                            let handle2 = handle_upd.clone();
                            // download_and_install runs the NSIS installer /S (silent, no UAC with currentUser).
                            if update.download_and_install(|_, _| {}, || {}).await.is_ok() {
                                if let Some(win) = handle2.get_webview_window("main") {
                                    let _ = win.emit("mjeku-update-ready", &new_version);
                                }
                                // Give the frontend 6 seconds to show the countdown toast.
                                tokio::time::sleep(Duration::from_secs(6)).await;
                                handle2.restart();
                            }
                        }
                    }
                });
            }

            // In dev, load the Vite dev server for fast iteration.
            #[cfg(desktop)]
            if tauri::is_dev() {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.maximize();
                    let dev_url = std::env::var("MJEKU_DEV_URL")
                        .unwrap_or_else(|_| "http://127.0.0.1:5173".to_string());
                    if dev_server_is_reachable(&dev_url) {
                        let js = format!("window.location.replace({:?});", dev_url);
                        let _ = win.eval(&js);
                    }
                }
            } else if let Some(win) = app.get_webview_window("main") {
                let _ = win.maximize();
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_app_info,
            printers_list,
            auth_get_state,
            auth_setup,
            provision_apply_token,
            auth_admin_unlock,
            auth_admin_lock,
            auth_admin_change_password,
            auth_user_login,
            auth_logs_admin_login,
            auth_user_logout,
            auth_doctor_login,
            auth_doctor_logout,
            doctors_login_options,
            doctor_account_update,
            settings_get_all,
            orthopedic_episodes_list,
            orthopedic_episode_upsert,
            orthopedic_episode_transition,
            orthopedic_episode_transitions_list,
            orthopedic_surgery_upsert,
            orthopedic_surgeries_list,
            orthopedic_fracture_followups_list,
            orthopedic_fracture_followup_add,
            orthopedic_operations_list,
            orthopedic_operation_finalize,
            orthopedic_implants_list,
            orthopedic_rehab_plans_list,
            physiotherapy_referral_create,
            physiotherapy_episodes_list,
            physiotherapy_sessions_list,
            physiotherapy_session_add,
            physiotherapy_measurements_list,
            physiotherapy_measurement_add,
            physiotherapy_care_plans_list,
            physiotherapy_care_plan_create,
            physiotherapy_exercises_list,
            physiotherapy_exercise_create,
            physiotherapy_hep_list,
            physiotherapy_hep_assign,
            physiotherapy_hep_log_add,
            physiotherapy_hep_logs_list,
            physiotherapy_body_markers_list,
            physiotherapy_body_marker_add,
            physiotherapy_reports_list,
            physiotherapy_report_create,
            physiotherapy_document_export_pdf,
            physiotherapy_documents_list,
            physiotherapy_document_attach,
            physiotherapy_document_open,
            orthopedic_rehab_phase_reviews_list,
            orthopedic_rehab_phase_review_add,
            orthopedic_reports_list,
            orthopedic_report_export_pdf,
            orthopedic_report_version_create,
            orthopedic_implant_upsert,
            orthopedic_rehab_upsert,
            orthopedic_markers_list,
            orthopedic_marker_upsert,
            orthopedic_assessments_list,
            orthopedic_assessment_upsert,
            orthopedic_fractures_list,
            orthopedic_fracture_upsert,
            orthopedic_imaging_links_list,
            orthopedic_imaging_link_upsert,
            orthopedic_postop_visits_list,
            orthopedic_postop_visit_upsert,
            orthopedic_implants_search,
            settings_set,
            clinic_asset_set_png,
            clinic_asset_clear_png,
            clients_list,
            clients_upsert,
            clients_delete,
            sales_list,
            sales_daily_report,
            sales_upsert,
            sales_delete,
            sales_mark_fiscalized_manual,
            sales_mark_non_fiscal_manual,
            payments_list,
            payments_upsert,
            payments_delete,
            doctors_list,
            doctors_upsert,
            doctors_delete,
            services_list,
            services_upsert,
            services_delete,
            appointments_list,
            appointments_upsert,
            appointments_delete,
            dental_treatment_plans_list,
            dental_periodontal_exams_list,
            dental_periodontal_exam_create,
            dental_finding_correct,
            dental_finding_corrections_list,
            dental_findings_list,
            dental_finding_create,
            visits_list,
            visits_upsert,
            visits_delete,
            visit_items_list,
            visit_items_upsert,
            visit_items_delete,
            lab_devices_list_profiles,
            lab_serial_ports_list,
            lab_device_get_default,
            lab_device_set_default,
            lab_device_connect,
            lab_device_disconnect,
            lab_device_status,
            lab_inbox_list,
            lab_inbox_assign,
            client_photos_list,
            client_photo_add,
            client_photo_data,
            client_photo_delete,
            cash_list,
            cash_upsert,
            cash_delete,
            sync_now,
            updates_check_now,
            desktop_updates_check_now,
            desktop_updates_open_download,
            updates_apply_downloaded,
            reload_ui,
            invoice_export_fiscal_inp,
            fiscal_report_x_inp,
            fiscal_report_z_inp,
            admin_reset_clinic,
            history_reset_all,
            invoice_export_pdf,
            visit_export_pdf,
            prescription_export_pdf,
            prescriptions_list,
            fiscal_jobs_list,
            fiscal_job_create,
            fiscal_job_cancel,
            fiscal_job_process,
            sef_bridge_info,
            sef_bridge_status,
            sef_bridge_select_engine,
            sef_ledger_list,
            sef_sale_reconcile,
            sef_coupons_list,
            sef_coupon_copy,
            sef_coupon_return,
            sef_coupon_cancel,
            sef_report,
            sef_sale_items,
            sef_sale_coupon,
            prescriptions_delete,
            error_logs_list,
            error_logs_clear,
            regular_invoice_create,
            regular_invoices_list,
            app_restart,
            sales_monthly_report,
            stock_suppliers_list,
            stock_supplier_upsert,
            stock_supplier_delete,
            stock_items_list,
            stock_item_upsert,
            stock_item_delete,
            stock_movements_list,
            stock_movement_add,
            sale_items_list,
            sale_items_replace,
            offers_list,
            offer_items_list,
            offer_upsert,
            offer_delete,
            offer_set_status,
            offer_pdf
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
