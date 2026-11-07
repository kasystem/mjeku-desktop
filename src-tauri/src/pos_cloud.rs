//! Cloud sync target in the PROGRAMERI POS Supabase project (schema `mjeku`).
//!
//! Since 0.6.1 a clinic opened from PROGRAMERI POS receives a device identity from POS
//! (bridge op `cloud.provision`; POS proves its own activation, the clinic types nothing).
//! The sync engine switches to this project only when the clinic was copied there
//! (`migrated == true`); until then it keeps using the old Mjeku project exactly as before.
//! Row Level Security isolates every row by the clinic in the device's server-issued claim.

use crate::db::Db;
use serde_json::Value;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const KEY_URL: &str = "pos_cloud_url";
const KEY_ANON: &str = "pos_cloud_anon_key";
const KEY_EMAIL: &str = "pos_cloud_device_email";
const KEY_PASSWORD: &str = "pos_cloud_device_password";
const KEY_MIGRATED: &str = "pos_cloud_migrated";
const KEY_ACCESS: &str = "pos_cloud_access_token";
const KEY_REFRESH: &str = "pos_cloud_refresh_token";
const KEY_EXPIRES: &str = "pos_cloud_token_expires_at";
const KEY_LAST_PROVISION: &str = "pos_cloud_last_provision_at";
/// Set once when sync first runs against the POS project (full pull from the start).
pub const KEY_SWITCHED: &str = "pos_cloud_switched_at";

const OFFICIAL_HOST: &str = "qomktqjbghdzwomlbhvb.supabase.co";
const PROVISION_INTERVAL_SECS: i64 = 6 * 60 * 60;
pub const SCHEMA: &str = "mjeku";

/// Remote of the POS project: anon/publishable key as `apikey`, device JWT as bearer.
#[derive(Clone)]
pub struct PosRemote {
    pub base: String,
    pub anon_key: String,
    pub bearer: String,
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn get(db: &Db, key: &str) -> String {
    db.setting_get(key).ok().flatten().unwrap_or_default()
}

fn official(url: &str) -> bool {
    url.trim_end_matches('/') == format!("https://{OFFICIAL_HOST}")
}

/// Asks PROGRAMERI POS for this PC's identity (at most every 6 hours). Only when Mjeku was
/// launched by POS; never fails the caller (offline/older POS simply keeps the old project).
pub async fn provision_if_due(db: &Db) {
    if !crate::sef_bridge::launched_from_programeri() {
        return;
    }
    let clinic_id = get(db, "clinic_id");
    if clinic_id.trim().is_empty() {
        return;
    }
    let last: i64 = get(db, KEY_LAST_PROVISION).parse().unwrap_or(0);
    if now_secs() - last < PROVISION_INTERVAL_SECS && !get(db, KEY_PASSWORD).is_empty() {
        return;
    }
    let label = std::env::var("COMPUTERNAME").unwrap_or_default();
    let result = crate::sef_bridge::call(
        "cloud.provision",
        serde_json::json!({
            "clinicId": clinic_id,
            "deviceLabel": label,
            "appVersion": env!("CARGO_PKG_VERSION"),
        }),
        Duration::from_secs(60),
    )
    .await;
    let Ok(v) = result else {
        return;
    };
    let _ = store_provision(db, &clinic_id, &v);
}

fn store_provision(db: &Db, clinic_id: &str, v: &Value) -> anyhow::Result<()> {
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
    if s("clinic_id") != clinic_id {
        anyhow::bail!("cloud.provision returned another clinic");
    }
    let url = s("supabase_url");
    if !official(&url) || s("schema") != SCHEMA {
        anyhow::bail!("cloud.provision returned an unexpected endpoint");
    }
    let email = s("device_email");
    let password = s("device_password");
    let anon = s("anon_key");
    if email.is_empty() || password.is_empty() || anon.is_empty() {
        anyhow::bail!("cloud.provision returned an incomplete identity");
    }
    let changed_identity = get(db, KEY_EMAIL) != email;
    db.setting_set(KEY_URL, url.trim_end_matches('/'))?;
    db.setting_set(KEY_ANON, &anon)?;
    db.setting_set(KEY_EMAIL, &email)?;
    db.setting_set(KEY_PASSWORD, &password)?;
    // The password was rotated by the server: drop the old session.
    db.setting_set(KEY_ACCESS, "")?;
    if changed_identity {
        db.setting_set(KEY_REFRESH, "")?;
    }
    db.setting_set(KEY_EXPIRES, "0")?;
    let migrated = v.get("migrated").and_then(Value::as_bool).unwrap_or(false);
    db.setting_set(KEY_MIGRATED, if migrated { "1" } else { "0" })?;
    db.setting_set(KEY_LAST_PROVISION, &now_secs().to_string())?;
    Ok(())
}

/// The POS remote when this clinic was migrated and the device holds an identity; otherwise
/// None and the caller keeps the old project.
pub async fn remote(db: &Db, client: &reqwest::Client) -> anyhow::Result<Option<PosRemote>> {
    if get(db, KEY_MIGRATED) != "1" {
        return Ok(None);
    }
    let base = get(db, KEY_URL);
    let anon = get(db, KEY_ANON);
    if !official(&base) || anon.is_empty() || get(db, KEY_PASSWORD).is_empty() {
        return Ok(None);
    }
    let token = access_token(db, client, &base, &anon).await?;
    Ok(Some(PosRemote { base, anon_key: anon, bearer: token }))
}

async fn access_token(db: &Db, client: &reqwest::Client, base: &str, anon: &str) -> anyhow::Result<String> {
    let access = get(db, KEY_ACCESS);
    let expires: i64 = get(db, KEY_EXPIRES).parse().unwrap_or(0);
    if !access.is_empty() && expires - 120 > now_secs() {
        return Ok(access);
    }
    let refresh = get(db, KEY_REFRESH);
    if !refresh.is_empty() {
        if let Ok(t) = token_request(client, base, anon, "refresh_token", serde_json::json!({ "refresh_token": refresh })).await {
            return save_tokens(db, &t);
        }
    }
    let t = token_request(
        client,
        base,
        anon,
        "password",
        serde_json::json!({ "email": get(db, KEY_EMAIL), "password": get(db, KEY_PASSWORD) }),
    )
    .await?;
    save_tokens(db, &t)
}

async fn token_request(client: &reqwest::Client, base: &str, anon: &str, grant: &str, body: Value) -> anyhow::Result<Value> {
    let resp = client
        .post(format!("{base}/auth/v1/token?grant_type={grant}"))
        .header("apikey", anon)
        .json(&body)
        .send()
        .await?;
    let status = resp.status();
    let json: Value = resp.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        anyhow::bail!("pos cloud auth {grant} failed: {status}");
    }
    Ok(json)
}

fn save_tokens(db: &Db, t: &Value) -> anyhow::Result<String> {
    let access = t.get("access_token").and_then(Value::as_str).unwrap_or("").to_string();
    if access.is_empty() {
        anyhow::bail!("pos cloud auth returned no token");
    }
    let refresh = t.get("refresh_token").and_then(Value::as_str).unwrap_or("");
    let expires_in = t.get("expires_in").and_then(Value::as_i64).unwrap_or(3600);
    db.setting_set(KEY_ACCESS, &access)?;
    if !refresh.is_empty() {
        db.setting_set(KEY_REFRESH, refresh)?;
    }
    db.setting_set(KEY_EXPIRES, &(now_secs() + expires_in).to_string())?;
    Ok(access)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_official_pos_project_is_accepted() {
        assert!(official("https://qomktqjbghdzwomlbhvb.supabase.co"));
        assert!(official("https://qomktqjbghdzwomlbhvb.supabase.co/"));
        assert!(!official("http://qomktqjbghdzwomlbhvb.supabase.co"));
        assert!(!official("https://occzpzryzxabajtmdaas.supabase.co"));
        assert!(!official("https://evil.example/qomktqjbghdzwomlbhvb.supabase.co"));
    }
}
