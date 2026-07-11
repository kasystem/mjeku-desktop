use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, bail, Context};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tauri::Manager;
use uuid::Uuid;
use zip::ZipArchive;

use crate::db::Db;

const DEFAULT_UPDATE_BASE_URL: &str = "https://mjeku-ui.onrender.com";
const FALLBACK_UPDATE_BASE_URL: &str = "https://mjeku-ui.vercel.app";

const KEY_UPDATE_BASE_URL: &str = "update_base_url";

// The UI seed ships inside the binary itself: on Android the Tauri resource dir is an
// `asset://` URI that std::fs cannot read, so file-based resource lookup breaks there.
const SEED_ZIP_BYTES: &[u8] = include_bytes!("../resources/ui-seed.zip");
const SEED_VER_TEXT: &str = include_str!("../resources/ui-seed-version.txt");

const UI_ROOT_DIRNAME: &str = "ui";
const UI_VERSIONS_DIRNAME: &str = "versions";
const UI_TMP_DIRNAME: &str = "tmp";

const CURRENT_PTR_FILENAME: &str = "current";
const PENDING_PTR_FILENAME: &str = "pending";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateManifest {
    pub latest_version: String,
    pub published_at: String,
    pub sha256: String,
    pub bundle_path: String,
}

pub struct UpdatesEngine {
    db: std::sync::Arc<Db>,
    client: reqwest::Client,
    lock: tokio::sync::Mutex<()>,
}

fn ui_bundle_looks_compatible(dir: &Path) -> bool {
    // Compatibility guard: when the backend API changes, we must not keep an older UI bundle
    // (even if it has index.html), otherwise the app breaks offline. We detect a compatible UI
    // by checking for a few "needles" that are expected to exist in the built JS bundle.
    let assets = dir.join("assets");
    let rd = match fs::read_dir(&assets) {
        Ok(r) => r,
        Err(_) => return false,
    };

    let mut has_clinic_name = false;
    let mut has_doctor_login = false;
    let mut has_doctors_login_options = false;
    let mut has_doctor_account_update = false;
    let mut has_sales_daily_report = false;

    for ent in rd.flatten() {
        let p = ent.path();
        if p.extension().and_then(|s| s.to_str()) != Some("js") {
            continue;
        }
        if let Ok(bytes) = fs::read(&p) {
            if !has_clinic_name
                && bytes
                    .windows(b"clinicName".len())
                    .any(|w| w == b"clinicName")
            {
                has_clinic_name = true;
            }
            if !has_doctor_login
                && bytes
                    .windows(b"auth_doctor_login".len())
                    .any(|w| w == b"auth_doctor_login")
            {
                has_doctor_login = true;
            }
            if !has_doctors_login_options
                && bytes
                    .windows(b"doctors_login_options".len())
                    .any(|w| w == b"doctors_login_options")
            {
                has_doctors_login_options = true;
            }
            if !has_doctor_account_update
                && bytes
                    .windows(b"doctor_account_update".len())
                    .any(|w| w == b"doctor_account_update")
            {
                has_doctor_account_update = true;
            }
            if !has_sales_daily_report
                && bytes
                    .windows(b"sales_daily_report".len())
                    .any(|w| w == b"sales_daily_report")
            {
                has_sales_daily_report = true;
            }

            if has_clinic_name
                && has_doctor_login
                && has_doctors_login_options
                && has_doctor_account_update
                && has_sales_daily_report
            {
                return true;
            }
        }
    }
    false
}

impl UpdatesEngine {
    pub fn new(db: std::sync::Arc<Db>) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .context("build http client")?;
        Ok(Self {
            db,
            client,
            lock: tokio::sync::Mutex::new(()),
        })
    }

    pub fn spawn_background(self: std::sync::Arc<Self>, app: tauri::AppHandle) {
        tauri::async_runtime::spawn(async move {
            // Startup check.
            let _ = self.check_now(&app).await;

            let mut interval = tokio::time::interval(Duration::from_secs(6 * 60 * 60));
            loop {
                interval.tick().await;
                let _ = self.check_now(&app).await;
            }
        });
    }

    pub async fn check_now(&self, app: &tauri::AppHandle) -> anyhow::Result<()> {
        let _guard = self.lock.lock().await;

        ensure_ui_dirs(app)?;

        let configured = self
            .db
            .setting_get(KEY_UPDATE_BASE_URL)?
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_UPDATE_BASE_URL.to_string());
        let configured = configured.trim_end_matches('/').to_string();

        // Provo bazën e konfiguruar/default (Render), pastaj Vercel si fallback nëse
        // e para dështon (rrjet, 402 faturimi, etj.) - njësoj si endpoint-et e EXE updater-it.
        let mut candidates = vec![configured];
        if candidates.iter().all(|b| b != FALLBACK_UPDATE_BASE_URL) {
            candidates.push(FALLBACK_UPDATE_BASE_URL.to_string());
        }

        let mut last_err: Option<anyhow::Error> = None;
        let mut found: Option<(String, UpdateManifest)> = None;
        for base in &candidates {
            let manifest_url = format!("{base}/manifest.json");
            let attempt = async {
                let resp = self
                    .client
                    .get(&manifest_url)
                    .send()
                    .await
                    .context("fetch manifest")?;
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                if !status.is_success() {
                    bail!("manifest fetch failed: {status} {body}");
                }
                serde_json::from_str::<UpdateManifest>(&body).context("parse manifest.json")
            }
            .await;
            match attempt {
                Ok(m) => {
                    found = Some((base.clone(), m));
                    break;
                }
                Err(e) => last_err = Some(e),
            }
        }
        let (base, manifest) = match found {
            Some(x) => x,
            None => return Err(last_err.unwrap_or_else(|| anyhow!("manifest fetch failed"))),
        };

        let latest = manifest.latest_version.trim().to_string();
        if latest.is_empty() {
            bail!("manifest latestVersion is empty");
        }

        if version_installed(app, &latest)? {
            // Already downloaded; mark pending if not current.
            let current = current_ui_version(app).unwrap_or_else(|_| "unknown".to_string());
            if current != latest {
                set_pointer_atomic(&pending_ptr_path(app)?, &latest)?;
            }
            prune_versions(app)?;
            return Ok(());
        }

        // Download bundle.
        let bundle_url = if manifest.bundle_path.starts_with('/') {
            format!("{base}{}", manifest.bundle_path)
        } else {
            format!("{base}/{}", manifest.bundle_path)
        };

        let bytes = self
            .client
            .get(&bundle_url)
            .send()
            .await
            .context("download bundle")?
            .bytes()
            .await?;
        let got = sha256_hex(bytes.as_ref());
        if !eq_hex(&got, &manifest.sha256) {
            bail!("sha256 mismatch: expected {}, got {}", manifest.sha256, got);
        }

        // Write to temp file.
        let tmp_dir = tmp_dir(app)?;
        fs::create_dir_all(&tmp_dir)?;
        let tmp_zip = tmp_dir.join(format!("ui-{}.zip", Uuid::new_v4()));
        fs::write(&tmp_zip, &bytes)?;

        // Extract into a temp folder and then rename into place.
        let versions = versions_dir(app)?;
        fs::create_dir_all(&versions)?;
        let extracting_dir = versions.join(format!(".extracting-{}-{}", latest, Uuid::new_v4()));
        fs::create_dir_all(&extracting_dir)?;
        extract_zip(&tmp_zip, &extracting_dir)?;

        // Basic validation.
        let idx = extracting_dir.join("index.html");
        if !idx.exists() {
            bail!("bundle missing index.html after extraction");
        }

        let final_dir = versions.join(&latest);
        if final_dir.exists() {
            // If it appeared concurrently, keep it.
            let _ = fs::remove_dir_all(&extracting_dir);
        } else {
            fs::rename(&extracting_dir, &final_dir)?;
        }

        // Mark pending and keep.
        set_pointer_atomic(&pending_ptr_path(app)?, &latest)?;
        prune_versions(app)?;

        let _ = fs::remove_file(&tmp_zip);
        Ok(())
    }

    pub fn apply_pending_on_startup(app: &tauri::AppHandle) -> anyhow::Result<()> {
        ensure_ui_dirs(app)?;
        let pending = pending_ui_version(app)?;
        if let Some(v) = pending {
            if version_installed(app, &v)? {
                set_pointer_atomic(&current_ptr_path(app)?, &v)?;
                clear_pending(app)?;
            }
        }
        Ok(())
    }

    pub fn ensure_seed_installed(app: &tauri::AppHandle) -> anyhow::Result<()> {
        ensure_ui_dirs(app)?;

        // Read the seed version bundled in this exe's resources.
        let seed_ver = SEED_VER_TEXT.to_string();
        let seed_ver = seed_ver.trim().to_string();
        let seed_ver = if seed_ver.is_empty() {
            "seed".to_string()
        } else {
            seed_ver
        };

        let versions = versions_dir(app)?;
        fs::create_dir_all(&versions)?;
        let seed_dir = versions.join(&seed_ver);

        // If this exact seed version is already extracted, point to it and return.
        // This handles: fresh installs, re-installs of the same version.
        if seed_dir.join("index.html").exists() {
            set_pointer_atomic(&current_ptr_path(app)?, &seed_ver)?;
            return Ok(());
        }

        // The seed version from this exe is not yet extracted — install it.
        // (This is the path taken when upgrading to a new exe with a new UI bundle.)
        let current_dir = current_ui_dir(app).ok();
        let current_has_index = current_dir
            .as_ref()
            .is_some_and(|d| d.join("index.html").exists());

        match extract_seed_resource(app, &seed_dir) {
            Ok(()) => {
                set_pointer_atomic(&current_ptr_path(app)?, &seed_ver)?;
                Ok(())
            }
            Err(e) => {
                // Extraction failed. Keep previous UI rather than showing an error page.
                if current_has_index {
                    return Ok(());
                }

                // No previous UI either — render a minimal offline fallback.
                fs::create_dir_all(&seed_dir)?;
                fs::write(
                    seed_dir.join("index.html"),
                    format!(
            "<!doctype html><html><body><h1>Mjeku UI not installed</h1><p>{}</p></body></html>",
            html_escape(&format!("{e}"))
          ),
                )?;
                set_pointer_atomic(&current_ptr_path(app)?, &seed_ver)?;
                Ok(())
            }
        }
    }

    pub fn apply_downloaded_now(app: &tauri::AppHandle) -> anyhow::Result<bool> {
        let pending = pending_ui_version(app)?;
        if let Some(v) = pending {
            if version_installed(app, &v)? {
                set_pointer_atomic(&current_ptr_path(app)?, &v)?;
                clear_pending(app)?;
                return Ok(true);
            }
        }
        Ok(false)
    }
}

pub fn ui_root_dir(app: &tauri::AppHandle) -> anyhow::Result<PathBuf> {
    Ok(app.path().app_data_dir()?.join(UI_ROOT_DIRNAME))
}

pub fn versions_dir(app: &tauri::AppHandle) -> anyhow::Result<PathBuf> {
    Ok(ui_root_dir(app)?.join(UI_VERSIONS_DIRNAME))
}

pub fn tmp_dir(app: &tauri::AppHandle) -> anyhow::Result<PathBuf> {
    Ok(ui_root_dir(app)?.join(UI_TMP_DIRNAME))
}

pub fn current_ptr_path(app: &tauri::AppHandle) -> anyhow::Result<PathBuf> {
    Ok(ui_root_dir(app)?.join(CURRENT_PTR_FILENAME))
}

pub fn pending_ptr_path(app: &tauri::AppHandle) -> anyhow::Result<PathBuf> {
    Ok(ui_root_dir(app)?.join(PENDING_PTR_FILENAME))
}

pub fn current_ui_version(app: &tauri::AppHandle) -> anyhow::Result<String> {
    let p = current_ptr_path(app)?;
    if !p.exists() {
        return Ok("seed".to_string());
    }
    let v = fs::read_to_string(p)?.trim().to_string();
    Ok(if v.is_empty() { "seed".to_string() } else { v })
}

pub fn pending_ui_version(app: &tauri::AppHandle) -> anyhow::Result<Option<String>> {
    let p = pending_ptr_path(app)?;
    if !p.exists() {
        return Ok(None);
    }
    let v = fs::read_to_string(p)?.trim().to_string();
    if v.is_empty() {
        Ok(None)
    } else {
        Ok(Some(v))
    }
}

pub fn clear_pending(app: &tauri::AppHandle) -> anyhow::Result<()> {
    let p = pending_ptr_path(app)?;
    if p.exists() {
        let _ = fs::remove_file(p);
    }
    Ok(())
}

pub fn current_ui_dir(app: &tauri::AppHandle) -> anyhow::Result<PathBuf> {
    let v = current_ui_version(app)?;
    Ok(versions_dir(app)?.join(v))
}

pub fn ensure_ui_dirs(app: &tauri::AppHandle) -> anyhow::Result<()> {
    fs::create_dir_all(versions_dir(app)?)?;
    fs::create_dir_all(tmp_dir(app)?)?;
    Ok(())
}

fn version_installed(app: &tauri::AppHandle, v: &str) -> anyhow::Result<bool> {
    Ok(versions_dir(app)?.join(v).join("index.html").exists())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn eq_hex(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

fn extract_seed_resource(_app: &tauri::AppHandle, dest: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dest)?;
    let cursor = std::io::Cursor::new(SEED_ZIP_BYTES);
    let archive = ZipArchive::new(cursor).context("read embedded seed zip")?;
    extract_zip_archive(archive, dest)?;
    Ok(())
}

fn extract_zip(zip_path: &Path, dest: &Path) -> anyhow::Result<()> {
    let f =
        fs::File::open(zip_path).with_context(|| format!("open zip: {}", zip_path.display()))?;
    let archive = ZipArchive::new(f).context("read zip archive")?;
    extract_zip_archive(archive, dest)
}

fn extract_zip_archive<R: std::io::Read + std::io::Seek>(
    mut archive: ZipArchive<R>,
    dest: &Path,
) -> anyhow::Result<()> {

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().to_string();
        let outpath = safe_zip_path(dest, &name)?;

        if file.is_dir() {
            fs::create_dir_all(&outpath)?;
            continue;
        }

        if let Some(parent) = outpath.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut outfile = fs::File::create(&outpath)?;
        std::io::copy(&mut file, &mut outfile)?;
        outfile.flush()?;
    }
    Ok(())
}

fn safe_zip_path(dest: &Path, name: &str) -> anyhow::Result<PathBuf> {
    let rel = Path::new(name);
    if rel
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        bail!("zip path traversal blocked: {name}");
    }
    Ok(dest.join(rel))
}

fn set_pointer_atomic(ptr: &Path, version: &str) -> anyhow::Result<()> {
    let dir = ptr
        .parent()
        .ok_or_else(|| anyhow!("pointer has no parent"))?;
    fs::create_dir_all(dir)?;
    let tmp = ptr.with_extension(format!("tmp-{}", Uuid::new_v4()));
    fs::write(&tmp, format!("{version}\n"))?;

    // Windows doesn't allow renaming over an existing file; do a safe swap.
    let bak = ptr.with_extension(format!("bak-{}", Uuid::new_v4()));
    if ptr.exists() {
        let _ = fs::rename(ptr, &bak);
    }
    fs::rename(&tmp, ptr)?;
    let _ = fs::remove_file(bak);
    Ok(())
}

fn prune_versions(app: &tauri::AppHandle) -> anyhow::Result<()> {
    let versions = versions_dir(app)?;
    let current = current_ui_version(app).unwrap_or_else(|_| "seed".to_string());
    let pending = pending_ui_version(app).unwrap_or(None);

    let mut dirs: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
    for ent in fs::read_dir(&versions)? {
        let ent = ent?;
        if !ent.file_type()?.is_dir() {
            continue;
        }
        let name = ent.file_name().to_string_lossy().to_string();
        if name == current || pending.as_deref() == Some(&name) {
            continue;
        }
        let m = ent
            .metadata()?
            .modified()
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        dirs.push((ent.path(), m));
    }

    // Keep 2 previous versions (besides current/pending).
    dirs.sort_by_key(|(_, m)| *m);
    while dirs.len() > 2 {
        let (p, _) = dirs.remove(0);
        let _ = fs::remove_dir_all(p);
    }
    Ok(())
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
