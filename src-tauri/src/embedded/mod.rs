//! Mjeku "embedded" — Mjeku i mbështjellë brenda PROGRAMERI POS Mobile (Android/iOS), pa Tauri.
//!
//! Kërkesë e pronarit 2026-11-26: "duhet wrapped brenda programeri pos". POS Mobile (MAUI) e shfaq
//! UI-në e Mjekut (i njëjti ui-seed i ndërtuar në binar) në WebView-in e vet dhe ia kalon çdo
//! `invoke(cmd, args)` këtij moduli me C ABI. Komandat janë PIKËRISHT funksionet #[tauri::command]
//! të lib.rs (dispatcher-i gjenerohet nga scripts/gen_embedded_dispatch.py), kështu që logjika,
//! DB-ja dhe sinkronizimi janë të njëjta me Mjeku Desktop/Android. Build-i desktop nuk preket:
//! ky modul kompilohet vetëm me `--no-default-features --features embedded`.
//!
//! Kontrata me hostin (POS):
//!   mjeku_embedded_start(data_dir, version) -> json {ok, error?, ui_dir?}
//!   mjeku_embedded_invoke(cmd, args_json)   -> json {ok:true, value} | {ok:false, error}
//!   mjeku_embedded_ui_dir()                 -> rruga e UI-së aktuale (index.html)
//!   mjeku_embedded_set_host_callback(cb)    -> ngjarjet drejt hostit (save_file, reload, restart, emit)
//!   mjeku_embedded_free(ptr)                -> liron stringjet e kthyera
//! Fiskalizimi: në celular Mjeku nuk fiskalizon (si në app-in Mjeku Android) — asgjë fiskale këtu.

mod dispatch;

use std::ffi::{c_char, CStr, CString};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use serde::Serialize;
use serde_json::{json, Value};

use crate::AppState;

// ---------------------------------------------------------------- shims (AppHandle / State)

/// Zëvendëson `tauri::State` — vetëm një referencë te gjendja e vetme e app-it.
pub struct State<'a, T: 'static>(pub &'a T);

impl<T> Clone for State<'_, T> {
    fn clone(&self) -> Self {
        State(self.0)
    }
}
impl<T> Copy for State<'_, T> {}

impl<'a, T> std::ops::Deref for State<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.0
    }
}

impl<'a, T> State<'a, T> {
    pub fn inner(&self) -> &'a T {
        self.0
    }
}

pub struct PackageInfo {
    pub version: semver_lite::Version,
}

pub mod semver_lite {
    #[derive(Clone)]
    pub struct Version(pub String);
    impl std::fmt::Display for Version {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.0)
        }
    }
}

struct Inner {
    data_dir: PathBuf,
    version: String,
}

/// Zëvendëson `tauri::AppHandle`: rrugët vijnë nga hosti (sandbox-i i POS-it), dritaret nuk ekzistojnë.
#[derive(Clone)]
pub struct AppHandle(Arc<Inner>);

pub struct PathResolver(Arc<Inner>);

impl PathResolver {
    pub fn app_data_dir(&self) -> std::io::Result<PathBuf> {
        Ok(self.0.data_dir.clone())
    }
    pub fn app_local_data_dir(&self) -> std::io::Result<PathBuf> {
        Ok(self.0.data_dir.clone())
    }
    pub fn app_cache_dir(&self) -> std::io::Result<PathBuf> {
        Ok(self.0.data_dir.join("cache"))
    }
    pub fn app_log_dir(&self) -> std::io::Result<PathBuf> {
        Ok(self.0.data_dir.join("logs"))
    }
    /// Në celular s'ka Desktop: skedarët e eksportuar shkojnë te `exports/` dhe hosti i ndan/ruan.
    pub fn desktop_dir(&self) -> std::io::Result<PathBuf> {
        Ok(self.0.data_dir.join("exports"))
    }
    pub fn document_dir(&self) -> std::io::Result<PathBuf> {
        Ok(self.0.data_dir.join("exports"))
    }
    pub fn download_dir(&self) -> std::io::Result<PathBuf> {
        Ok(self.0.data_dir.join("exports"))
    }
}

pub struct WebviewWindow;

impl WebviewWindow {
    pub fn eval(&self, js: &str) -> Result<(), String> {
        if js.contains("location.reload") {
            host_event(json!({"type": "reload"}));
        } else {
            host_event(json!({"type": "eval", "js": js}));
        }
        Ok(())
    }
    pub fn emit<S: Serialize + Clone>(&self, event: &str, payload: S) -> Result<(), String> {
        host_event(json!({"type": "emit", "event": event, "payload": payload}));
        Ok(())
    }
    pub fn maximize(&self) -> Result<(), String> {
        Ok(())
    }
}

impl AppHandle {
    pub fn path(&self) -> PathResolver {
        PathResolver(self.0.clone())
    }
    pub fn package_info(&self) -> PackageInfo {
        PackageInfo {
            version: semver_lite::Version(self.0.version.clone()),
        }
    }
    pub fn get_webview_window(&self, _label: &str) -> Option<WebviewWindow> {
        Some(WebviewWindow)
    }
    pub fn emit<S: Serialize + Clone>(&self, event: &str, payload: S) -> Result<(), String> {
        WebviewWindow.emit(event, payload)
    }
    /// Hosti e rihap faqen e Mjekut (procesi i POS-it nuk rinis).
    pub fn restart(&self) {
        host_event(json!({"type": "restart"}));
    }
    /// Hosti mbyll faqen e Mjekut (kthehet te POS-i); procesi i POS-it mbetet.
    pub fn exit(&self, code: i32) {
        host_event(json!({"type": "exit", "code": code}));
    }
}

// ---------------------------------------------------------------- runtime

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

pub fn runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("mjeku-embedded")
            .build()
            .expect("tokio runtime")
    })
}

/// Zëvendëson `tauri::async_runtime::spawn`.
pub fn spawn<F>(f: F) -> tokio::task::JoinHandle<F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    runtime().spawn(f)
}

// ---------------------------------------------------------------- responses

pub trait IntoResp {
    fn into_resp(self) -> Result<Value, Value>;
}

impl<T: Serialize, E: Serialize> IntoResp for Result<T, E> {
    fn into_resp(self) -> Result<Value, Value> {
        match self {
            Ok(v) => serde_json::to_value(v).map_err(|e| Value::String(e.to_string())),
            Err(e) => Err(serde_json::to_value(e).unwrap_or_else(|e| Value::String(e.to_string()))),
        }
    }
}

// ---------------------------------------------------------------- host events

pub type HostCallback = extern "C" fn(*const c_char);
static HOST_CB: Mutex<Option<HostCallback>> = Mutex::new(None);

pub fn host_event(v: Value) {
    let cb = *HOST_CB.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(cb) = cb {
        if let Ok(s) = CString::new(v.to_string()) {
            cb(s.as_ptr());
        }
    }
}

/// Ruajtja e një skedari në celular (PDF/INP/eksport): shkruhet te `exports/` dhe hosti hap
/// dritaren "Ruaj/Ndaj" të sistemit. Kthen rrugën lokale.
pub fn save_export(app: &AppHandle, default_name: &str, bytes: &[u8]) -> Result<String, String> {
    let dir = app.path().desktop_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let safe: String = default_name
        .chars()
        .map(|c| if c.is_alphanumeric() || "-_. ".contains(c) { c } else { '_' })
        .collect();
    let path = dir.join(if safe.trim().is_empty() { "eksport.bin".to_string() } else { safe });
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    host_event(json!({"type": "save_file", "path": path.display().to_string(), "name": default_name}));
    Ok(path.display().to_string())
}

// ---------------------------------------------------------------- app state

struct Started {
    app: AppHandle,
    state: &'static AppState,
}

static STARTED: OnceLock<Started> = OnceLock::new();
static START_LOCK: Mutex<()> = Mutex::new(());

fn start_inner(data_dir: PathBuf, version: String) -> anyhow::Result<()> {
    let _g = START_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if STARTED.get().is_some() {
        return Ok(());
    }
    std::fs::create_dir_all(&data_dir)?;
    let app = AppHandle(Arc::new(Inner { data_dir, version }));
    // Init në kontekstin e runtime-it (motorët e sinkronizimit/licencës nisin task-et e tyre).
    let state = {
        let _enter = runtime().enter();
        crate::init_app_state(&app)?
    };
    let state: &'static AppState = Box::leak(Box::new(state));
    let _ = STARTED.set(Started { app, state });
    Ok(())
}

// ---------------------------------------------------------------- C ABI

fn cstr(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

fn out(v: Value) -> *mut c_char {
    CString::new(v.to_string())
        .unwrap_or_else(|_| CString::new("{\"ok\":false,\"error\":\"nul\"}").unwrap())
        .into_raw()
}

fn catch<F: FnOnce() -> Value + std::panic::UnwindSafe>(f: F) -> *mut c_char {
    match std::panic::catch_unwind(f) {
        Ok(v) => out(v),
        Err(p) => {
            let msg = p
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "panic".into());
            out(json!({"ok": false, "error": format!("Mjeku: gabim i brendshëm ({msg})")}))
        }
    }
}

#[no_mangle]
pub extern "C" fn mjeku_embedded_start(data_dir: *const c_char, version: *const c_char) -> *mut c_char {
    let data_dir = cstr(data_dir);
    let version = cstr(version);
    catch(move || {
        if data_dir.trim().is_empty() {
            return json!({"ok": false, "error": "data_dir mungon"});
        }
        match start_inner(PathBuf::from(data_dir), version) {
            Ok(()) => {
                let ui = STARTED
                    .get()
                    .and_then(|s| crate::updates::current_ui_dir(&s.app).ok())
                    .map(|p| p.display().to_string());
                json!({"ok": true, "ui_dir": ui})
            }
            Err(e) => json!({"ok": false, "error": format!("{e:#}")}),
        }
    })
}

#[no_mangle]
pub extern "C" fn mjeku_embedded_invoke(cmd: *const c_char, args_json: *const c_char) -> *mut c_char {
    let cmd = cstr(cmd);
    let args = cstr(args_json);
    catch(move || {
        let Some(s) = STARTED.get() else {
            return json!({"ok": false, "error": "Mjeku nuk është nisur"});
        };
        let args: Value = if args.trim().is_empty() {
            Value::Null
        } else {
            match serde_json::from_str(&args) {
                Ok(v) => v,
                Err(e) => return json!({"ok": false, "error": format!("args: {e}")}),
            }
        };
        let app = s.app.clone();
        let st = s.state;
        match runtime().block_on(async move { dispatch::dispatch(&app, st, &cmd, args).await }) {
            Ok(v) => json!({"ok": true, "value": v}),
            Err(e) => json!({"ok": false, "error": e}),
        }
    })
}

#[no_mangle]
pub extern "C" fn mjeku_embedded_ui_dir() -> *mut c_char {
    catch(|| {
        let dir = STARTED
            .get()
            .and_then(|s| crate::updates::current_ui_dir(&s.app).ok())
            .map(|p| p.display().to_string());
        json!({"ok": dir.is_some(), "ui_dir": dir})
    })
}

#[no_mangle]
pub extern "C" fn mjeku_embedded_commands() -> *mut c_char {
    catch(|| json!({"ok": true, "value": dispatch::COMMANDS}))
}

#[no_mangle]
pub extern "C" fn mjeku_embedded_set_host_callback(cb: Option<HostCallback>) {
    *HOST_CB.lock().unwrap_or_else(|e| e.into_inner()) = cb;
}

#[no_mangle]
pub extern "C" fn mjeku_embedded_free(p: *mut c_char) {
    if !p.is_null() {
        unsafe {
            drop(CString::from_raw(p));
        }
    }
}
