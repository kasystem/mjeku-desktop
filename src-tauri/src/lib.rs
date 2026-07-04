mod auth;
mod db;
mod desktop_updates;
mod error_logs;
mod invoice;
mod license_engine;
mod models;
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

const LOGS_ADMIN_USER: &str = "fatlindadmin";
const LOGS_ADMIN_PASS: &str = "Fatlind0)";
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
const DESKTOP_UPDATE_FORCE_AFTER_DAYS: i64 = 7;
const DEFAULT_SUPABASE_URL: &str = "https://occzpzryzxabajtmdaas.supabase.co";
const DEFAULT_SUPABASE_API_KEY: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6Im9jY3pwenJ5enhhYmFqdG1kYWFzIiwicm9sZSI6ImFub24iLCJpYXQiOjE3NzA2NTQxMjksImV4cCI6MjA4NjIzMDEyOX0.Fq2bTsVLPpRhoLe845Lf-kMsy8rmPF2ijMZCWBb1zHc";
const DEFAULT_UPDATE_BASE_URL: &str = "https://mjeku-ui.vercel.app";

const TOKEN_MODE_EXISTING: &str = "existing";
const TOKEN_MODE_NEW: &str = "new";

struct AppState {
    db: Arc<Db>,
    auth: Arc<AuthState>,
    sync: Arc<SyncEngine>,
    updates: Arc<UpdatesEngine>,
    license: Arc<LicenseEngine>,
    error_log_path: PathBuf,
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
        .map(|x| x.trim().trim_start_matches('v') != current_version.trim().trim_start_matches('v'))
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
        sync_status: sync_status.sync_status,
        last_sync_time,
        last_sync_error: sync_status.last_sync_error,
        license_ok: lic.ok,
        license_status: lic.status,
        license_active_until: lic.active_until,
        license_last_checked_at: lic.last_checked_at,
        license_seconds_left,
        desktop_update_forced,
        desktop_update_latest_version,
        desktop_update_force_deadline_at,
        desktop_update_last_manual_check_at,
    })
}

#[tauri::command]
async fn auth_get_state(state: tauri::State<'_, AppState>) -> Result<AuthStateInfo, String> {
    let db = state.db.clone();
    let admin_unlocked = state.auth.is_admin_unlocked().await;
    let session = state.auth.session().await;
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

#[tauri::command]
async fn provision_apply_token(
    state: tauri::State<'_, AppState>,
    token: String,
) -> Result<AuthStateInfo, String> {
    let token_code = normalize_provision_token(&token);
    if token_code.is_empty() {
        return Err("tokeni eshte i detyrueshem".to_string());
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

#[tauri::command]
async fn auth_logs_admin_login(
    state: tauri::State<'_, AppState>,
    username: String,
    password: String,
) -> Result<bool, String> {
    let user = username.trim().to_lowercase();
    let pass = password.trim();
    if user != LOGS_ADMIN_USER || pass != LOGS_ADMIN_PASS {
        append_error_log(
            state.inner(),
            "auth_logs_admin_login",
            "hyrje e pasakte per logs_admin",
        );
        return Ok(false);
    }

    let db = state.db.clone();
    tokio::task::spawn_blocking(move || crate::auth::session_set_logs_admin(&db))
        .await
        .map_err(err_string)?
        .map_err(err_string)?;

    state
        .auth
        .set_session(SessionKind::User {
            role: crate::auth::UserRole::LogsAdmin,
        })
        .await;
    Ok(true)
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
    let s = state.auth.session().await;
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
    let s = state.auth.session().await;
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
        ] {
            map.remove(k);
        }
    }

    Ok(map)
}

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
    }

    let db = state.db.clone();
    tokio::task::spawn_blocking(move || db.setting_set(&k, &v))
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
    tokio::task::spawn_blocking(move || db.clients_upsert(client))
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
    let desktop_dir: Option<PathBuf> = app.path().document_dir().ok();
    tokio::task::spawn_blocking(move || {
        use base64::{engine::general_purpose, Engine as _};

        let pdf_bytes = build_invoice_pdf_bytes_inner(&db, &data_dir, &sale_id, fiscal_only)?;

        let out_dir = desktop_dir.unwrap_or_else(|| data_dir.join("temp"));
        std::fs::create_dir_all(&out_dir).map_err(err_string)?;
        let out_path = out_dir.join(format!("Fature-{}.pdf", sale_id));
        std::fs::write(&out_path, &pdf_bytes).map_err(err_string)?;

        Ok(general_purpose::STANDARD.encode(pdf_bytes))
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
    let desktop_dir: Option<PathBuf> = app.path().document_dir().ok();
    tokio::task::spawn_blocking(move || {
        use base64::{engine::general_purpose, Engine as _};

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

        let out_dir = desktop_dir.unwrap_or_else(|| data_dir.join("temp"));
        std::fs::create_dir_all(&out_dir).map_err(err_string)?;
        let out_path = out_dir.join(format!("Vizite-{}.pdf", visit.id));
        std::fs::write(&out_path, &pdf_bytes).map_err(err_string)?;

        Ok(general_purpose::STANDARD.encode(pdf_bytes))
    })
    .await
    .map_err(err_string)?
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
    let desktop_dir: Option<PathBuf> = app.path().document_dir().ok();

    tokio::task::spawn_blocking(move || {
        use base64::{engine::general_purpose, Engine as _};

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

        let out_dir = desktop_dir.unwrap_or_else(|| data_dir.join("temp"));
        std::fs::create_dir_all(&out_dir).map_err(err_string)?;
        let out_path = out_dir.join(format!("{}.pdf", file_tag));
        std::fs::write(&out_path, &pdf_bytes).map_err(err_string)?;

        Ok(general_purpose::STANDARD.encode(pdf_bytes))
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
        use base64::{engine::general_purpose, Engine as _};

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

        // Also return base64 for optional download in UI.
        let _b64 = general_purpose::STANDARD.encode(&pdf_bytes);

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
async fn offer_pdf(state: tauri::State<'_, AppState>, offer_id: String) -> Result<String, String> {
    let _ = require_finance(&state).await?;
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
            let db = Arc::new(Db::new(data_dir.join("mjeku.sqlite3"))?);
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

            let session = crate::auth::session_get(db.as_ref()).unwrap_or(SessionKind::None);
            let auth = Arc::new(AuthState::new(session));
            let sync = Arc::new(SyncEngine::new(db.clone())?);
            let updates = Arc::new(UpdatesEngine::new(db.clone())?);
            let license = Arc::new(LicenseEngine::new(db.clone())?);

            UpdatesEngine::apply_pending_on_startup(&handle)?;
            UpdatesEngine::ensure_seed_installed(&handle)?;

            sync.clone().spawn_background();
            updates.clone().spawn_background(handle.clone());
            license.clone().spawn_background();
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
            visits_list,
            visits_upsert,
            visits_delete,
            visit_items_list,
            visit_items_upsert,
            visit_items_delete,
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
