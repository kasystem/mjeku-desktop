mod auth;
mod db;
mod invoice;
mod models;
mod sync_engine;
mod ui_protocol;
mod updates;
mod util;

use std::collections::HashMap;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use anyhow::bail;
use tauri::Manager;

use crate::auth::{AuthState, AuthStateInfo, SessionKind};
use crate::db::Db;
use crate::models::{
  Appointment, AppointmentUpsertInput, AppointmentsListFilters, AppInfo, CashEntry, CashEntryUpsertInput, CashListFilters,
  Client, ClientUpsertInput, DailySalesReport, Doctor, DoctorLoginOption, DoctorUpsertInput, Payment, PaymentUpsertInput,
  PaymentsListFilters, Sale, SaleUpsertInput, SalesListFilters, Service, ServiceUpsertInput, Visit, VisitItem,
  VisitItemUpsertInput, VisitItemsListFilters, VisitUpsertInput, VisitsListFilters,
};
use crate::sync_engine::SyncEngine;
use crate::updates::UpdatesEngine;

struct AppState {
  db: Arc<Db>,
  auth: Arc<AuthState>,
  sync: Arc<SyncEngine>,
  updates: Arc<UpdatesEngine>,
}

fn err_string(e: impl std::fmt::Display) -> String {
  e.to_string()
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

#[tauri::command]
async fn get_app_info(app: tauri::AppHandle, state: tauri::State<'_, AppState>) -> Result<AppInfo, String> {
  let version = app.package_info().version.to_string();
  let ui_version = crate::updates::current_ui_version(&app).unwrap_or_else(|_| "seed".to_string());
  let sync_status = state.sync.get_status().await;

  let db = state.db.clone();
  let last_sync_time = tokio::task::spawn_blocking(move || db.get_last_sync_time())
    .await
    .map_err(err_string)?
    .map_err(err_string)?;

  Ok(AppInfo {
    version,
    ui_version,
    sync_status: sync_status.sync_status,
    last_sync_time,
    last_sync_error: sync_status.last_sync_error,
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
) -> Result<AuthStateInfo, String> {
  let db = state.db.clone();
  tokio::task::spawn_blocking(move || crate::auth::setup(&db, &clinic_name, &admin_password, &user_password))
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn auth_admin_unlock(state: tauri::State<'_, AppState>, password: String) -> Result<bool, String> {
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
async fn auth_admin_change_password(state: tauri::State<'_, AppState>, new_password: String) -> Result<(), String> {
  let db = state.db.clone();
  tokio::task::spawn_blocking(move || crate::auth::admin_change_password(&db, &new_password))
    .await
    .map_err(err_string)?
    .map_err(err_string)?;
  state.auth.admin_lock().await;
  Ok(())
}

#[tauri::command]
async fn auth_user_login(state: tauri::State<'_, AppState>, password: String) -> Result<bool, String> {
  let db = state.db.clone();
  let ok = tokio::task::spawn_blocking(move || crate::auth::user_verify(&db, &password))
    .await
    .map_err(err_string)?
    .map_err(err_string)?;
  if ok {
    let db2 = state.db.clone();
    tokio::task::spawn_blocking(move || crate::auth::session_set_user(&db2))
      .await
      .map_err(err_string)?
      .map_err(err_string)?;
    state.auth.set_session(SessionKind::User).await;
  }
  Ok(ok)
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
async fn doctors_login_options(state: tauri::State<'_, AppState>) -> Result<Vec<DoctorLoginOption>, String> {
  let db = state.db.clone();
  tokio::task::spawn_blocking(move || db.doctors_login_options())
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn auth_doctor_login(state: tauri::State<'_, AppState>, doctor_id: String, password: String) -> Result<bool, String> {
  let db = state.db.clone();
  let did = doctor_id.clone();
  let res = tokio::task::spawn_blocking(move || crate::auth::doctor_verify(&db, &did, &password))
    .await
    .map_err(err_string)?
    .map_err(err_string)?;

  match res {
    crate::auth::DoctorVerify::NoAccount => Err("ky mjek nuk ka login. kontakto administratorin.".to_string()),
    crate::auth::DoctorVerify::WrongPassword => Ok(false),
    crate::auth::DoctorVerify::Ok { .. } => {
      let db2 = state.db.clone();
      let did2 = doctor_id.clone();
      let session = tokio::task::spawn_blocking(move || crate::auth::session_set_doctor(&db2, &did2))
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
  let _ = require_finance(&state).await?;
  let db = state.db.clone();
  tokio::task::spawn_blocking(move || crate::auth::doctor_account_update(&db, &doctor_id, password.as_deref(), is_admin))
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

async fn require_login(state: &tauri::State<'_, AppState>) -> Result<SessionKind, String> {
  let s = state.auth.session().await;
  match s {
    SessionKind::None => Err("duhet te hysh per te vazhduar".to_string()),
    _ => Ok(s),
  }
}

async fn require_finance(state: &tauri::State<'_, AppState>) -> Result<SessionKind, String> {
  let s = require_login(state).await?;
  match &s {
    SessionKind::User => Ok(s),
    SessionKind::Doctor { is_admin: true, .. } => Ok(s),
    _ => Err("nuk ke akses per kete seksion".to_string()),
  }
}

#[tauri::command]
async fn settings_get_all(state: tauri::State<'_, AppState>) -> Result<HashMap<String, String>, String> {
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
      "admin_salt",
      "admin_hash",
      "user_salt",
      "user_hash",
      "user_logged_in",
      "session",
    ] {
      map.remove(k);
    }
  }

  Ok(map)
}

#[tauri::command]
async fn settings_set(state: tauri::State<'_, AppState>, key: String, value: String) -> Result<(), String> {
  let k = key.trim().to_string();
  if k.is_empty() {
    return Err("key is required".to_string());
  }

  let protected = matches!(
    k.as_str(),
    "supabase_url"
      | "supabase_api_key"
      | "supabase_anon_key"
      | "update_base_url"
      | "clinic_id"
      | "admin_salt"
      | "admin_hash"
      | "user_salt"
      | "user_hash"
      | "user_logged_in"
      | "session"
  );
  if protected {
    if !state.auth.is_admin_unlocked().await {
      return Err("kjo vlere kerkon hyrje si admin".to_string());
    }
  } else {
    let _ = require_login(&state).await?;
  }

  let db = state.db.clone();
  tokio::task::spawn_blocking(move || db.setting_set(&k, &value))
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn clients_list(state: tauri::State<'_, AppState>, search: Option<String>) -> Result<Vec<Client>, String> {
  let _ = require_login(&state).await?;
  let db = state.db.clone();
  tokio::task::spawn_blocking(move || db.clients_list(search))
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn clients_upsert(state: tauri::State<'_, AppState>, client: ClientUpsertInput) -> Result<Client, String> {
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
async fn sales_list(state: tauri::State<'_, AppState>, filters: Option<SalesListFilters>) -> Result<Vec<Sale>, String> {
  let _ = require_finance(&state).await?;
  let db = state.db.clone();
  tokio::task::spawn_blocking(move || db.sales_list(filters))
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn sales_daily_report(state: tauri::State<'_, AppState>, date: String) -> Result<DailySalesReport, String> {
  let _ = require_finance(&state).await?;
  let db = state.db.clone();
  tokio::task::spawn_blocking(move || db.sales_daily_report(&date))
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn sales_upsert(state: tauri::State<'_, AppState>, sale: SaleUpsertInput) -> Result<Sale, String> {
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
async fn payments_upsert(state: tauri::State<'_, AppState>, payment: PaymentUpsertInput) -> Result<Payment, String> {
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
async fn doctors_list(state: tauri::State<'_, AppState>, search: Option<String>) -> Result<Vec<Doctor>, String> {
  let session = require_login(&state).await?;
  let db = state.db.clone();
  tokio::task::spawn_blocking(move || match session {
    SessionKind::Doctor {
      doctor_id,
      is_admin: false,
    } => Ok(db.doctors_get(&doctor_id)?.into_iter().filter(|d| d.deleted == 0).collect()),
    _ => db.doctors_list(search),
  })
  .await
  .map_err(err_string)?
  .map_err(err_string)
}

#[tauri::command]
async fn doctors_upsert(state: tauri::State<'_, AppState>, doctor: DoctorUpsertInput) -> Result<Doctor, String> {
  let _ = require_finance(&state).await?;
  let db = state.db.clone();
  tokio::task::spawn_blocking(move || db.doctors_upsert(doctor))
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn doctors_delete(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
  let _ = require_finance(&state).await?;
  let db = state.db.clone();
  tokio::task::spawn_blocking(move || db.doctors_delete(&id))
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn services_list(state: tauri::State<'_, AppState>, search: Option<String>) -> Result<Vec<Service>, String> {
  let _ = require_login(&state).await?;
  let db = state.db.clone();
  tokio::task::spawn_blocking(move || db.services_list(search))
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn services_upsert(state: tauri::State<'_, AppState>, service: ServiceUpsertInput) -> Result<Service, String> {
  let _ = require_finance(&state).await?;
  let db = state.db.clone();
  tokio::task::spawn_blocking(move || db.services_upsert(service))
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn services_delete(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
  let _ = require_finance(&state).await?;
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
async fn visits_list(state: tauri::State<'_, AppState>, filters: Option<VisitsListFilters>) -> Result<Vec<Visit>, String> {
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
async fn visits_upsert(state: tauri::State<'_, AppState>, visit: VisitUpsertInput) -> Result<Visit, String> {
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
        let existing = db.visits_get(id)?.ok_or_else(|| anyhow::anyhow!("vizita nuk u gjet"))?;
        if existing.deleted == 0 && existing.doctor_id.as_deref() != Some(doctor_id.as_str()) {
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
async fn cash_list(state: tauri::State<'_, AppState>, filters: Option<CashListFilters>) -> Result<Vec<CashEntry>, String> {
  let _ = require_finance(&state).await?;
  let db = state.db.clone();
  tokio::task::spawn_blocking(move || db.cash_list(filters))
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn cash_upsert(state: tauri::State<'_, AppState>, entry: CashEntryUpsertInput) -> Result<CashEntry, String> {
  let _ = require_finance(&state).await?;
  let db = state.db.clone();
  tokio::task::spawn_blocking(move || db.cash_upsert(entry))
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn cash_delete(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
  let _ = require_finance(&state).await?;
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
async fn updates_check_now(app: tauri::AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
  let _ = require_login(&state).await?;
  state.updates.check_now(&app).await.map_err(err_string)
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
async fn invoice_export_pdf(state: tauri::State<'_, AppState>, sale_id: String) -> Result<String, String> {
  let _ = require_finance(&state).await?;
  let sale_id = sale_id.trim().to_string();
  if sale_id.is_empty() {
    return Err("sale_id eshte i detyrueshem".to_string());
  }

  let db = state.db.clone();
  tokio::task::spawn_blocking(move || {
    use base64::{engine::general_purpose, Engine as _};

    let clinic_name = db
      .setting_get("clinic_name")
      .map_err(err_string)?
      .unwrap_or_else(|| "Klinika".to_string());

    let sale = db
      .sales_get(&sale_id)
      .map_err(err_string)?
      .ok_or_else(|| "fatura nuk u gjet".to_string())?;
    let client = db
      .clients_get(&sale.client_id)
      .map_err(err_string)?
      .ok_or_else(|| "pacienti nuk u gjet".to_string())?;

    let vis_items = db
      .visit_items_list(Some(crate::models::VisitItemsListFilters {
        visit_id: Some(sale_id.clone()),
        client_id: None,
        include_deleted: Some(false),
      }))
      .map_err(err_string)?;
    let lines: Vec<crate::invoice::InvoiceLine> = vis_items
      .into_iter()
      .filter(|x| x.deleted == 0)
      .map(|it| crate::invoice::InvoiceLine {
        tooth: it.tooth,
        title: it.title,
        qty: it.qty,
        unit_price: it.unit_price,
        fiscal: it.fiscal == 1,
      })
      .collect();

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
        if ln.fiscal {
          fiscal_total += sub;
        } else {
          non_fiscal_total += sub;
        }
      }
    }

    let data = crate::invoice::InvoicePdfData {
      clinic_name,
      invoice_id: sale.id.clone(),
      date: sale.date.clone(),
      client_name: client.name,
      client_phone: client.phone,
      client_email: client.email,
      notes: sale.notes.clone(),
      lines,
      total,
      fiscal_total,
      non_fiscal_total,
    };

    let pdf_bytes = crate::invoice::render_invoice_pdf(&data).map_err(err_string)?;
    Ok(general_purpose::STANDARD.encode(pdf_bytes))
  })
  .await
  .map_err(err_string)?
}

fn main() {
  tauri::Builder::default()
    .register_uri_scheme_protocol("mjeku", ui_protocol::handle)
    .setup(|app| {
      let handle = app.handle();
      let data_dir = handle.path().app_data_dir().map_err(|e| anyhow::anyhow!(e))?;
      let db = Arc::new(Db::new(data_dir.join("mjeku.sqlite3"))?);

      let session = crate::auth::session_get(db.as_ref()).unwrap_or(SessionKind::None);
      let auth = Arc::new(AuthState::new(session));
      let sync = Arc::new(SyncEngine::new(db.clone())?);
      let updates = Arc::new(UpdatesEngine::new(db.clone())?);

      UpdatesEngine::apply_pending_on_startup(&handle)?;
      UpdatesEngine::ensure_seed_installed(&handle)?;

      sync.clone().spawn_background();
      updates.clone().spawn_background(handle.clone());

      app.manage(AppState {
        db,
        auth,
        sync,
        updates,
      });

      // In dev, load the Vite dev server for fast iteration.
      if tauri::is_dev() {
        if let Some(win) = app.get_webview_window("main") {
          let dev_url = std::env::var("MJEKU_DEV_URL").unwrap_or_else(|_| "http://127.0.0.1:5173".to_string());
          if dev_server_is_reachable(&dev_url) {
            let js = format!("window.location.replace({:?});", dev_url);
            let _ = win.eval(&js);
          }
        }
      }
      Ok(())
    })
    .invoke_handler(tauri::generate_handler![
      get_app_info,
      auth_get_state,
      auth_setup,
      auth_admin_unlock,
      auth_admin_lock,
      auth_admin_change_password,
      auth_user_login,
      auth_user_logout,
      auth_doctor_login,
      auth_doctor_logout,
      doctors_login_options,
      doctor_account_update,
      settings_get_all,
      settings_set,
      clients_list,
      clients_upsert,
      clients_delete,
      sales_list,
      sales_daily_report,
      sales_upsert,
      sales_delete,
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
      cash_list,
      cash_upsert,
      cash_delete,
      sync_now,
      updates_check_now,
      updates_apply_downloaded,
      reload_ui,
      invoice_export_pdf
    ])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
