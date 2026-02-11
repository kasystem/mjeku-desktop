use anyhow::{anyhow, bail};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::db::Db;
use crate::util::now_iso;

const KEY_CLINIC_ID: &str = "clinic_id";
const KEY_CLINIC_NAME: &str = "clinic_name";
const KEY_ADMIN_SALT: &str = "admin_salt";
const KEY_ADMIN_HASH: &str = "admin_hash";
const KEY_USER_SALT: &str = "user_salt";
const KEY_USER_HASH: &str = "user_hash";
const KEY_CASHIER_SALT: &str = "cashier_salt";
const KEY_CASHIER_HASH: &str = "cashier_hash";

// Legacy (v1) login persistence key for the shared user password.
const KEY_USER_LOGGED_IN: &str = "user_logged_in";

// Session is persisted so the app keeps working fully offline across restarts.
// Values: "" | "owner" | "cashier" | "doctor:<doctor_id>"
const KEY_SESSION: &str = "session";

#[derive(Debug, Clone)]
pub enum UserRole {
  Owner,
  Cashier,
  LogsAdmin,
}

impl UserRole {
  pub fn as_str(&self) -> &'static str {
    match self {
      UserRole::Owner => "owner",
      UserRole::Cashier => "cashier",
      UserRole::LogsAdmin => "logs_admin",
    }
  }
}

#[derive(Debug, Clone)]
pub enum SessionKind {
  None,
  User { role: UserRole },
  Doctor { doctor_id: String, is_admin: bool },
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionInfo {
  pub kind: String, // none | user | doctor
  pub user_role: Option<String>, // owner | cashier
  pub doctor_id: Option<String>,
  pub doctor_name: Option<String>,
  pub doctor_is_admin: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AuthStateInfo {
  pub configured: bool,
  pub clinic_id: Option<String>,
  pub clinic_name: Option<String>,
  // Vendor-only unlock (shows Supabase keys, update URL, etc).
  pub admin_unlocked: bool,
  // Current active session (shared user or doctor).
  pub session: SessionInfo,
}

pub struct AuthState {
  admin_unlocked: tokio::sync::RwLock<bool>,
  session: tokio::sync::RwLock<SessionKind>,
}

impl AuthState {
  pub fn new(initial_session: SessionKind) -> Self {
    Self {
      admin_unlocked: tokio::sync::RwLock::new(false),
      session: tokio::sync::RwLock::new(initial_session),
    }
  }

  pub async fn is_admin_unlocked(&self) -> bool {
    *self.admin_unlocked.read().await
  }

  pub async fn admin_lock(&self) {
    let mut w = self.admin_unlocked.write().await;
    *w = false;
  }

  pub async fn admin_unlock(&self) {
    let mut w = self.admin_unlocked.write().await;
    *w = true;
  }

  pub async fn session(&self) -> SessionKind {
    self.session.read().await.clone()
  }

  pub async fn set_session(&self, s: SessionKind) {
    let mut w = self.session.write().await;
    *w = s;
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
      && user_hash.as_deref().unwrap_or("").trim().len() > 0,
  )
}

fn session_info_from_kind(db: &Db, kind: &SessionKind) -> anyhow::Result<SessionInfo> {
  match kind {
    SessionKind::None => Ok(SessionInfo {
      kind: "none".to_string(),
      user_role: None,
      doctor_id: None,
      doctor_name: None,
      doctor_is_admin: false,
    }),
    SessionKind::User { role } => Ok(SessionInfo {
      kind: "user".to_string(),
      user_role: Some(role.as_str().to_string()),
      doctor_id: None,
      doctor_name: None,
      doctor_is_admin: matches!(role, UserRole::Owner | UserRole::Cashier),
    }),
    SessionKind::Doctor { doctor_id, is_admin } => {
      let doctor_name = db
        .doctors_get(doctor_id)?
        .filter(|d| d.deleted == 0)
        .map(|d| d.name);
      Ok(SessionInfo {
        kind: "doctor".to_string(),
        user_role: None,
        doctor_id: Some(doctor_id.clone()),
        doctor_name,
        doctor_is_admin: *is_admin,
      })
    }
  }
}

pub fn read_state(db: &Db, admin_unlocked: bool, session_kind: SessionKind) -> anyhow::Result<AuthStateInfo> {
  let clinic_id = db.setting_get(KEY_CLINIC_ID)?;
  let clinic_name = db.setting_get(KEY_CLINIC_NAME)?;
  let configured = is_configured(db)?;
  Ok(AuthStateInfo {
    configured,
    clinic_id,
    clinic_name,
    admin_unlocked,
    session: session_info_from_kind(db, &session_kind)?,
  })
}

pub fn setup(db: &Db, clinic_name: &str, admin_password: &str, user_password: &str) -> anyhow::Result<AuthStateInfo> {
  setup_v2(db, clinic_name, admin_password, user_password, None)
}

pub fn setup_v2(
  db: &Db,
  clinic_name: &str,
  admin_password: &str,
  owner_password: &str,
  cashier_password: Option<&str>,
) -> anyhow::Result<AuthStateInfo> {
  let clinic_name = clinic_name.trim();
  if clinic_name.is_empty() {
    bail!("emri i klinikes eshte i detyrueshem");
  }
  let admin_password = admin_password.trim();
  if admin_password.len() < 6 {
    bail!("fjalekalimi i adminit duhet te kete te pakten 6 karaktere");
  }
  let owner_password = owner_password.trim();
  if owner_password.len() < 4 {
    bail!("fjalekalimi i pergjithshem duhet te kete te pakten 4 karaktere");
  }
  let cashier_password = cashier_password.map(|x| x.trim().to_string()).filter(|x| !x.is_empty());
  if let Some(pw) = cashier_password.as_deref() {
    if pw.len() < 4 {
      bail!("fjalekalimi i arketares duhet te kete te pakten 4 karaktere");
    }
  }

  if is_configured(db)? {
    bail!("aplikacioni eshte konfiguruar tashme");
  }

  let clinic_id = Uuid::new_v4().to_string();
  let admin_salt = Uuid::new_v4().to_string();
  let admin_hash = hash_password(&admin_salt, admin_password);
  let user_salt = Uuid::new_v4().to_string();
  let user_hash = hash_password(&user_salt, owner_password);

  let cashier_salt = cashier_password.as_deref().map(|_| Uuid::new_v4().to_string());
  let cashier_hash = cashier_password
    .as_deref()
    .and_then(|pw| cashier_salt.as_deref().map(|salt| hash_password(salt, pw)));

  db.setting_set(KEY_CLINIC_ID, &clinic_id)?;
  db.setting_set(KEY_CLINIC_NAME, clinic_name)?;
  db.setting_set(KEY_ADMIN_SALT, &admin_salt)?;
  db.setting_set(KEY_ADMIN_HASH, &admin_hash)?;
  db.setting_set(KEY_USER_SALT, &user_salt)?;
  db.setting_set(KEY_USER_HASH, &user_hash)?;
  if let (Some(salt), Some(hash)) = (cashier_salt.as_deref(), cashier_hash.as_deref()) {
    db.setting_set(KEY_CASHIER_SALT, salt)?;
    db.setting_set(KEY_CASHIER_HASH, hash)?;
  }

  // Start with no logged-in session.
  db.setting_set(KEY_SESSION, "")?;
  db.setting_set(KEY_USER_LOGGED_IN, "0")?;

  read_state(db, false, SessionKind::None)
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

pub fn user_verify_role(db: &Db, password: &str) -> anyhow::Result<Option<UserRole>> {
  let password = password.trim();
  if password.is_empty() {
    return Ok(None);
  }

  // Owner password (legacy `user_*` keys).
  {
    let salt = db
      .setting_get(KEY_USER_SALT)?
      .ok_or_else(|| anyhow!("mungon user_salt"))?;
    let expected = db
      .setting_get(KEY_USER_HASH)?
      .ok_or_else(|| anyhow!("mungon user_hash"))?;
    let got = hash_password(&salt, password);
    if expected.trim().eq_ignore_ascii_case(got.trim()) {
      return Ok(Some(UserRole::Owner));
    }
  }

  // Optional cashier password.
  let salt = match db.setting_get(KEY_CASHIER_SALT)? {
    Some(s) if !s.trim().is_empty() => s,
    _ => return Ok(None),
  };
  let expected = match db.setting_get(KEY_CASHIER_HASH)? {
    Some(h) if !h.trim().is_empty() => h,
    _ => return Ok(None),
  };
  let got = hash_password(&salt, password);
  if expected.trim().eq_ignore_ascii_case(got.trim()) {
    return Ok(Some(UserRole::Cashier));
  }
  Ok(None)
}

pub fn session_get(db: &Db) -> anyhow::Result<SessionKind> {
  if !is_configured(db)? {
    return Ok(SessionKind::None);
  }

  let raw = db.setting_get(KEY_SESSION)?.unwrap_or_default();
  let raw = raw.trim();
  if raw.is_empty() {
    // Legacy fallback.
    if db.setting_get(KEY_USER_LOGGED_IN)?.unwrap_or_default().trim() == "1" {
      let _ = db.setting_set(KEY_SESSION, "owner");
      return Ok(SessionKind::User { role: UserRole::Owner });
    }
    return Ok(SessionKind::None);
  }

  if raw.eq_ignore_ascii_case("user") {
    // Legacy value.
    let _ = db.setting_set(KEY_SESSION, "owner");
    return Ok(SessionKind::User { role: UserRole::Owner });
  }
  if raw.eq_ignore_ascii_case("owner") {
    return Ok(SessionKind::User { role: UserRole::Owner });
  }
  if raw.eq_ignore_ascii_case("cashier") {
    return Ok(SessionKind::User { role: UserRole::Cashier });
  }
  if raw.eq_ignore_ascii_case("logs_admin") {
    return Ok(SessionKind::User {
      role: UserRole::LogsAdmin,
    });
  }

  if let Some(rest) = raw.strip_prefix("doctor:") {
    let doctor_id = rest.trim().to_string();
    if doctor_id.is_empty() {
      return Ok(SessionKind::None);
    }

    // Validate the doctor still exists and has local credentials.
    let is_admin = match db.doctor_account_get(&doctor_id)? {
      Some((_salt, _hash, is_admin)) => is_admin,
      None => {
        let _ = session_clear(db);
        return Ok(SessionKind::None);
      }
    };
    let ok_doc = db
      .doctors_get(&doctor_id)?
      .filter(|d| d.deleted == 0)
      .is_some();
    if !ok_doc {
      let _ = session_clear(db);
      return Ok(SessionKind::None);
    }
    return Ok(SessionKind::Doctor { doctor_id, is_admin });
  }

  Ok(SessionKind::None)
}

pub fn session_set_user(db: &Db) -> anyhow::Result<()> {
  db.setting_set(KEY_SESSION, "owner")?;
  // Legacy key for older UI bundles.
  db.setting_set(KEY_USER_LOGGED_IN, "1")?;
  Ok(())
}

pub fn session_set_cashier(db: &Db) -> anyhow::Result<()> {
  db.setting_set(KEY_SESSION, "cashier")?;
  db.setting_set(KEY_USER_LOGGED_IN, "0")?;
  Ok(())
}

pub fn session_set_logs_admin(db: &Db) -> anyhow::Result<()> {
  db.setting_set(KEY_SESSION, "logs_admin")?;
  db.setting_set(KEY_USER_LOGGED_IN, "0")?;
  Ok(())
}

pub fn session_set_doctor(db: &Db, doctor_id: &str) -> anyhow::Result<SessionKind> {
  let doctor_id = doctor_id.trim();
  if doctor_id.is_empty() {
    bail!("doctor_id eshte i detyrueshem");
  }
  let is_admin = db
    .doctor_account_get(doctor_id)?
    .map(|(_, _, a)| a)
    .unwrap_or(false);
  db.setting_set(KEY_SESSION, &format!("doctor:{doctor_id}"))?;
  db.setting_set(KEY_USER_LOGGED_IN, "0")?;
  Ok(SessionKind::Doctor {
    doctor_id: doctor_id.to_string(),
    is_admin,
  })
}

pub fn session_clear(db: &Db) -> anyhow::Result<()> {
  db.setting_set(KEY_SESSION, "")?;
  db.setting_set(KEY_USER_LOGGED_IN, "0")?;
  Ok(())
}

pub enum DoctorVerify {
  NoAccount,
  WrongPassword,
  Ok { is_admin: bool },
}

pub fn doctor_verify(db: &Db, doctor_id: &str, password: &str) -> anyhow::Result<DoctorVerify> {
  let doctor_id = doctor_id.trim();
  if doctor_id.is_empty() {
    bail!("doctor_id eshte i detyrueshem");
  }
  let password = password.trim();
  if password.is_empty() {
    bail!("fjalekalimi eshte i detyrueshem");
  }

  let Some((salt, expected, is_admin)) = db.doctor_account_get(doctor_id)? else {
    return Ok(DoctorVerify::NoAccount);
  };
  let got = hash_password(&salt, password);
  if expected.trim().eq_ignore_ascii_case(got.trim()) {
    Ok(DoctorVerify::Ok { is_admin })
  } else {
    Ok(DoctorVerify::WrongPassword)
  }
}

pub fn doctor_account_update(db: &Db, doctor_id: &str, password: Option<&str>, is_admin: bool) -> anyhow::Result<()> {
  let doctor_id = doctor_id.trim();
  if doctor_id.is_empty() {
    bail!("doctor_id eshte i detyrueshem");
  }

  // Ensure doctor exists and isn't deleted.
  let doc = db
    .doctors_get(doctor_id)?
    .ok_or_else(|| anyhow!("mjeku nuk u gjet"))?;
  if doc.deleted != 0 {
    bail!("mjeku eshte fshire");
  }

  let now = now_iso();
  if let Some(pw) = password.map(|x| x.trim()).filter(|x| !x.is_empty()) {
    if pw.len() < 4 {
      bail!("fjalekalimi i mjekut duhet te kete te pakten 4 karaktere");
    }
    let salt = Uuid::new_v4().to_string();
    let hash = hash_password(&salt, pw);
    db.doctor_account_set(doctor_id, &salt, &hash, is_admin, &now)?;
    return Ok(());
  }

  // Update only flags (keep existing password).
  let Some((salt, hash, _)) = db.doctor_account_get(doctor_id)? else {
    bail!("ky mjek nuk ka login; vendos nje fjalekalim");
  };
  db.doctor_account_set(doctor_id, &salt, &hash, is_admin, &now)?;
  Ok(())
}
