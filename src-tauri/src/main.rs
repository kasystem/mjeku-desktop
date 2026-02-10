mod db;
mod models;
mod sync_engine;
mod ui_protocol;
mod updates;
mod util;

use std::collections::HashMap;
use std::sync::Arc;

use tauri::Manager;

use crate::db::Db;
use crate::models::{
  AppInfo, Client, ClientUpsertInput, Payment, PaymentUpsertInput, PaymentsListFilters, Sale, SaleUpsertInput,
  SalesListFilters,
};
use crate::sync_engine::SyncEngine;
use crate::updates::UpdatesEngine;

struct AppState {
  db: Arc<Db>,
  sync: Arc<SyncEngine>,
  updates: Arc<UpdatesEngine>,
}

fn err_string(e: impl std::fmt::Display) -> String {
  e.to_string()
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
async fn settings_get_all(state: tauri::State<'_, AppState>) -> Result<HashMap<String, String>, String> {
  let db = state.db.clone();
  tokio::task::spawn_blocking(move || db.settings_get_all())
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn settings_set(state: tauri::State<'_, AppState>, key: String, value: String) -> Result<(), String> {
  let db = state.db.clone();
  tokio::task::spawn_blocking(move || db.setting_set(&key, &value))
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn clients_list(state: tauri::State<'_, AppState>, search: Option<String>) -> Result<Vec<Client>, String> {
  let db = state.db.clone();
  tokio::task::spawn_blocking(move || db.clients_list(search))
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn clients_upsert(state: tauri::State<'_, AppState>, client: ClientUpsertInput) -> Result<Client, String> {
  let db = state.db.clone();
  tokio::task::spawn_blocking(move || db.clients_upsert(client))
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn clients_delete(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
  let db = state.db.clone();
  tokio::task::spawn_blocking(move || db.clients_delete(&id))
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn sales_list(state: tauri::State<'_, AppState>, filters: Option<SalesListFilters>) -> Result<Vec<Sale>, String> {
  let db = state.db.clone();
  tokio::task::spawn_blocking(move || db.sales_list(filters))
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn sales_upsert(state: tauri::State<'_, AppState>, sale: SaleUpsertInput) -> Result<Sale, String> {
  let db = state.db.clone();
  tokio::task::spawn_blocking(move || db.sales_upsert(sale))
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn sales_delete(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
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
  let db = state.db.clone();
  tokio::task::spawn_blocking(move || db.payments_list(filters))
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn payments_upsert(state: tauri::State<'_, AppState>, payment: PaymentUpsertInput) -> Result<Payment, String> {
  let db = state.db.clone();
  tokio::task::spawn_blocking(move || db.payments_upsert(payment))
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn payments_delete(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
  let db = state.db.clone();
  tokio::task::spawn_blocking(move || db.payments_delete(&id))
    .await
    .map_err(err_string)?
    .map_err(err_string)
}

#[tauri::command]
async fn sync_now(state: tauri::State<'_, AppState>) -> Result<(), String> {
  state.sync.sync_now().await.map_err(err_string)
}

#[tauri::command]
async fn updates_check_now(app: tauri::AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
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

fn main() {
  tauri::Builder::default()
    .register_uri_scheme_protocol("mjeku", ui_protocol::handle)
    .setup(|app| {
      let handle = app.handle();
      let data_dir = handle.path().app_data_dir().map_err(|e| anyhow::anyhow!(e))?;
      let db = Arc::new(Db::new(data_dir.join("mjeku.sqlite3"))?);

      let sync = Arc::new(SyncEngine::new(db.clone())?);
      let updates = Arc::new(UpdatesEngine::new(db.clone())?);

      UpdatesEngine::apply_pending_on_startup(&handle)?;
      UpdatesEngine::ensure_seed_installed(&handle)?;

      sync.clone().spawn_background();
      updates.clone().spawn_background(handle.clone());

      app.manage(AppState { db, sync, updates });

      // In dev, load the Vite dev server for fast iteration.
      if tauri::is_dev() {
        if let Some(win) = app.get_webview_window("main") {
          let dev_url = std::env::var("MJEKU_DEV_URL").unwrap_or_else(|_| "http://127.0.0.1:5173".to_string());
          let js = format!("window.location.replace({:?});", dev_url);
          let _ = win.eval(&js);
        }
      }
      Ok(())
    })
    .invoke_handler(tauri::generate_handler![
      get_app_info,
      settings_get_all,
      settings_set,
      clients_list,
      clients_upsert,
      clients_delete,
      sales_list,
      sales_upsert,
      sales_delete,
      payments_list,
      payments_upsert,
      payments_delete,
      sync_now,
      updates_check_now,
      updates_apply_downloaded,
      reload_ui
    ])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
