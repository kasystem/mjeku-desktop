use anyhow::{anyhow, bail};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::db::Db;

const KEY_CLINIC_ID: &str = "clinic_id";
const KEY_CLINIC_NAME: &str = "clinic_name";
const KEY_ADMIN_SALT: &str = "admin_salt";
const KEY_ADMIN_HASH: &str = "admin_hash";
const KEY_USER_SALT: &str = "user_salt";
const KEY_USER_HASH: &str = "user_hash";
const KEY_USER_LOGGED_IN: &str = "user_logged_in";

#[derive(Debug, Clone, serde::Serialize)]
pub struct AuthStateInfo {
  pub configured: bool,
  pub clinic_id: Option<String>,
  pub clinic_name: Option<String>,
  pub admin_unlocked: bool,
  pub user_logged_in: bool,
}

pub struct AuthState {
  admin_unlocked: tokio::sync::RwLock<bool>,
  user_logged_in: tokio::sync::RwLock<bool>,
}

impl AuthState {
  pub fn new(user_logged_in: bool) -> Self {
    Self {
      admin_unlocked: tokio::sync::RwLock::new(false),
      user_logged_in: tokio::sync::RwLock::new(user_logged_in),
    }
  }

  pub async fn is_admin_unlocked(&self) -> bool {
    *self.admin_unlocked.read().await
  }

  pub async fn is_user_logged_in(&self) -> bool {
    *self.user_logged_in.read().await
  }

  pub async fn admin_lock(&self) {
    let mut w = self.admin_unlocked.write().await;
    *w = false;
  }

  pub async fn admin_unlock(&self) {
    let mut w = self.admin_unlocked.write().await;
    *w = true;
  }

  pub async fn user_logout(&self) {
    let mut w = self.user_logged_in.write().await;
    *w = false;
  }

  pub async fn user_login(&self) {
    let mut w = self.user_logged_in.write().await;
    *w = true;
  }
}

fn sha256_hex(s: &str) -> String {
  let mut h = Sha256::new();
  h.update(s.as_bytes());
  hex::encode(h.finalize())
}

fn hash_password(salt: &str, password: &str) -> String {
  sha256_hex(&format!("{salt}:{password}"))
}

pub fn is_configured(db: &Db) -> anyhow::Result<bool> {
  let clinic_id = db.setting_get(KEY_CLINIC_ID)?;
  let admin_salt = db.setting_get(KEY_ADMIN_SALT)?;
  let admin_hash = db.setting_get(KEY_ADMIN_HASH)?;
  let user_salt = db.setting_get(KEY_USER_SALT)?;
  let user_hash = db.setting_get(KEY_USER_HASH)?;
  Ok(
    clinic_id
      .as_deref()
      .unwrap_or("")
      .trim()
      .len()
      > 0
      && admin_salt.as_deref().unwrap_or("").trim().len() > 0
      && admin_hash.as_deref().unwrap_or("").trim().len() > 0
      && user_salt.as_deref().unwrap_or("").trim().len() > 0
      && user_hash.as_deref().unwrap_or("").trim().len() > 0
  )
}

pub fn read_state(db: &Db, admin_unlocked: bool, user_logged_in: bool) -> anyhow::Result<AuthStateInfo> {
  let clinic_id = db.setting_get(KEY_CLINIC_ID)?;
  let clinic_name = db.setting_get(KEY_CLINIC_NAME)?;
  let configured = is_configured(db)?;
  Ok(AuthStateInfo {
    configured,
    clinic_id,
    clinic_name,
    admin_unlocked,
    user_logged_in,
  })
}

pub fn setup(db: &Db, clinic_name: &str, admin_password: &str, user_password: &str) -> anyhow::Result<AuthStateInfo> {
  let clinic_name = clinic_name.trim();
  if clinic_name.is_empty() {
    bail!("emri i klinikes eshte i detyrueshem");
  }
  let admin_password = admin_password.trim();
  if admin_password.len() < 6 {
    bail!("fjalekalimi i adminit duhet te kete te pakten 6 karaktere");
  }
  let user_password = user_password.trim();
  if user_password.len() < 4 {
    bail!("fjalekalimi i pergjithshem duhet te kete te pakten 4 karaktere");
  }

  if is_configured(db)? {
    bail!("aplikacioni eshte konfiguruar tashme");
  }

  let clinic_id = Uuid::new_v4().to_string();
  let admin_salt = Uuid::new_v4().to_string();
  let admin_hash = hash_password(&admin_salt, admin_password);
  let user_salt = Uuid::new_v4().to_string();
  let user_hash = hash_password(&user_salt, user_password);

  db.setting_set(KEY_CLINIC_ID, &clinic_id)?;
  db.setting_set(KEY_CLINIC_NAME, clinic_name)?;
  db.setting_set(KEY_ADMIN_SALT, &admin_salt)?;
  db.setting_set(KEY_ADMIN_HASH, &admin_hash)?;
  db.setting_set(KEY_USER_SALT, &user_salt)?;
  db.setting_set(KEY_USER_HASH, &user_hash)?;
  db.setting_set(KEY_USER_LOGGED_IN, "0")?;

  read_state(db, false, false)
}

pub fn admin_verify(db: &Db, password: &str) -> anyhow::Result<bool> {
  let salt = db
    .setting_get(KEY_ADMIN_SALT)?
    .ok_or_else(|| anyhow!("mungon admin_salt"))?;
  let expected = db
    .setting_get(KEY_ADMIN_HASH)?
    .ok_or_else(|| anyhow!("mungon admin_hash"))?;
  let got = hash_password(&salt, password.trim());
  Ok(expected.trim().eq_ignore_ascii_case(got.trim()))
}

pub fn admin_change_password(db: &Db, new_password: &str) -> anyhow::Result<()> {
  let new_password = new_password.trim();
  if new_password.len() < 6 {
    bail!("fjalekalimi i ri duhet te kete te pakten 6 karaktere");
  }
  let salt = Uuid::new_v4().to_string();
  let hash = hash_password(&salt, new_password);
  db.setting_set(KEY_ADMIN_SALT, &salt)?;
  db.setting_set(KEY_ADMIN_HASH, &hash)?;
  Ok(())
}

pub fn user_verify(db: &Db, password: &str) -> anyhow::Result<bool> {
  let salt = db
    .setting_get(KEY_USER_SALT)?
    .ok_or_else(|| anyhow!("mungon user_salt"))?;
  let expected = db
    .setting_get(KEY_USER_HASH)?
    .ok_or_else(|| anyhow!("mungon user_hash"))?;
  let got = hash_password(&salt, password.trim());
  Ok(expected.trim().eq_ignore_ascii_case(got.trim()))
}

pub fn user_set_logged_in(db: &Db, logged_in: bool) -> anyhow::Result<()> {
  db.setting_set(KEY_USER_LOGGED_IN, if logged_in { "1" } else { "0" })?;
  Ok(())
}

pub fn user_get_logged_in(db: &Db) -> anyhow::Result<bool> {
  Ok(db.setting_get(KEY_USER_LOGGED_IN)?.unwrap_or_default().trim() == "1")
}
