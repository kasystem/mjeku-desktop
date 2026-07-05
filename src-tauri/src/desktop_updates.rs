use anyhow::{anyhow, bail, Context};
use serde::{Deserialize, Serialize};

const DEFAULT_RELEASE_API: &str =
    "https://api.github.com/repos/kasystem/mjeku-desktop/releases/latest";

#[derive(Debug, Clone, Serialize)]
pub struct DesktopUpdateInfo {
    pub update_available: bool,
    pub current_version: String,
    pub latest_version: Option<String>,
    pub published_at: Option<String>,
    pub download_url: Option<String>,
    pub notes: Option<String>,
    #[serde(default)]
    pub forced: bool,
    pub force_deadline_at: Option<String>,
    pub last_manual_check_at: Option<String>,
    pub checked_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GhRelease {
    tag_name: String,
    published_at: Option<String>,
    body: Option<String>,
    assets: Vec<GhAsset>,
}

fn norm_ver(s: &str) -> String {
    s.trim().trim_start_matches('v').to_string()
}

fn pick_asset(assets: &[GhAsset]) -> Option<String> {
    let os = std::env::consts::OS;
    let want_exts: &[&str] = match os {
        "windows" => &[".exe", ".msi"],
        "macos" => &[".dmg", ".pkg", ".zip"],
        _ => &[".AppImage", ".deb", ".rpm", ".tar.gz", ".zip"],
    };

    // Prefer installers over archives when possible.
    for ext in want_exts {
        if let Some(a) = assets.iter().find(|a| {
            a.name.to_lowercase().ends_with(&ext.to_lowercase())
                && a.name.to_lowercase().contains("setup")
        }) {
            return Some(a.browser_download_url.clone());
        }
    }
    for ext in want_exts {
        if let Some(a) = assets
            .iter()
            .find(|a| a.name.to_lowercase().ends_with(&ext.to_lowercase()))
        {
            return Some(a.browser_download_url.clone());
        }
    }
    None
}

fn pick_api(setting_api: Option<&str>) -> String {
    if let Some(s) = setting_api {
        let s = s.trim();
        if !s.is_empty() {
            return s.to_string();
        }
    }
    let env_api = std::env::var("MJEKU_DESKTOP_UPDATE_API").unwrap_or_default();
    if !env_api.trim().is_empty() {
        return env_api;
    }
    DEFAULT_RELEASE_API.to_string()
}

pub async fn check_now(
    app: &tauri::AppHandle,
    setting_api: Option<&str>,
) -> anyhow::Result<DesktopUpdateInfo> {
    let current_version = app.package_info().version.to_string();
    let api = pick_api(setting_api);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .context("build http client")?;

    let resp = client
        .get(api)
        .header("user-agent", "mjeku-desktop")
        .header("accept", "application/vnd.github+json")
        .send()
        .await
        .context("fetch release")?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        if status == reqwest::StatusCode::NOT_FOUND {
            // Graceful fallback when repository has no published Releases yet.
            return Ok(DesktopUpdateInfo {
        update_available: false,
        current_version: current_version.clone(),
        latest_version: Some(norm_ver(&current_version)),
        published_at: None,
        download_url: None,
        notes: Some("Eshte ne versionin final, nuk ka update.".to_string()),
        forced: false,
        force_deadline_at: None,
        last_manual_check_at: None,
        checked_at: None,
      });
        }
        bail!("update check failed: {status} {body}");
    }

    let rel: GhRelease = serde_json::from_str(&body).context("parse release json")?;
    let latest_version = norm_ver(&rel.tag_name);
    if latest_version.trim().is_empty() {
        return Err(anyhow!("release tag_name is empty"));
    }

    let download_url = pick_asset(&rel.assets);
    let update_available = norm_ver(&current_version) != latest_version;

    Ok(DesktopUpdateInfo {
        update_available,
        current_version,
        latest_version: Some(latest_version),
        published_at: rel.published_at,
        download_url,
        notes: rel.body,
        forced: false,
        force_deadline_at: None,
        last_manual_check_at: None,
        checked_at: None,
    })
}

pub fn open_external(url: &str) -> anyhow::Result<()> {
    let u = url.trim();
    if u.is_empty() {
        bail!("url is empty");
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        open::that(u).context("open external url")?;
        Ok(())
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        anyhow::bail!("Hapja e linqeve të jashtme nuk mbështetet në mobile: {u}")
    }
}
