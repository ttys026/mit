use anyhow::{anyhow, Result};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_REGION: &str = "cn";
pub const DEFAULT_REDIRECT_URI: &str = "http://127.0.0.1:8000/login_redirect";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserProfile {
    pub uid: String,
    pub nickname: String,
    pub icon: String,
    pub union_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AuthAccount {
    pub version: u32,
    pub region: String,
    pub redirect_uri: String,
    pub uuid: String,
    pub device_id: String,
    pub state: String,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_ts: i64,
    pub user: UserProfile,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AuthState {
    pub accounts: Vec<AuthAccount>,
    pub pending_auth: Option<AuthAccount>,
}

#[cfg(not(test))]
pub fn get_home_dir() -> PathBuf {
    for key in ["MIT_PROFILE_DIR", "MIT_HOME", "XMCLI_HOME"] {
        if let Some(value) = env::var_os(key) {
            let path = PathBuf::from(value);
            if !path.as_os_str().is_empty() {
                return path;
            }
        }
    }
    for key in ["USERPROFILE", "HOME"] {
        if let Some(value) = env::var_os(key) {
            let path = PathBuf::from(value);
            if !path.as_os_str().is_empty() {
                return path;
            }
        }
    }
    PathBuf::from(".")
}

#[cfg(test)]
pub fn get_home_dir() -> PathBuf {
    for key in ["MIT_PROFILE_DIR", "MIT_HOME", "XMCLI_HOME"] {
        if let Some(value) = env::var_os(key) {
            let path = PathBuf::from(value);
            if !path.as_os_str().is_empty() {
                return path;
            }
        }
    }
    env::temp_dir().join(format!("mit-test-profile-{}", std::process::id()))
}

pub fn get_mit_dir() -> PathBuf {
    get_home_dir().join(".mit")
}

pub fn get_auth_path() -> PathBuf {
    get_mit_dir().join("auth.json")
}

pub fn get_accounts_dir() -> PathBuf {
    get_mit_dir().join("accounts")
}

pub fn get_account_dir(uid: &str) -> PathBuf {
    get_accounts_dir().join(uid.trim())
}

pub fn generate_uuid() -> String {
    let mut bytes = [0_u8; 16];
    OsRng.fill_bytes(&mut bytes);
    let mut out = String::with_capacity(32);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

pub fn default_auth() -> AuthState {
    AuthState {
        accounts: Vec::new(),
        pending_auth: None,
    }
}

pub fn normalize_account(value: Value) -> AuthAccount {
    normalize_account_ref(&value)
}

pub fn normalize_auth(value: Value) -> Result<AuthState> {
    normalize_auth_ref(&value)
}

pub fn normalize_auth_state(auth: &AuthState) -> Result<AuthState> {
    normalize_auth(serde_json::to_value(auth)?)
}

pub fn has_persisted_auth_data(account: &AuthAccount) -> bool {
    !(account.device_id.is_empty()
        && account.state.is_empty()
        && account.access_token.is_empty()
        && account.refresh_token.is_empty()
        && account.user.uid.is_empty()
        && account.expires_ts == 0)
}

pub fn get_auth_accounts(auth: &AuthState) -> Result<Vec<AuthAccount>> {
    Ok(normalize_auth_state(auth)?.accounts)
}

pub fn get_pending_auth(auth: &AuthState) -> Result<Option<AuthAccount>> {
    Ok(normalize_auth_state(auth)?.pending_auth)
}

pub fn find_auth_account_by_uid(auth: &AuthState, uid: &str) -> Option<AuthAccount> {
    auth.accounts
        .iter()
        .find(|account| account.user.uid == uid)
        .cloned()
}

pub fn upsert_auth_account(auth: &AuthState, account: &AuthAccount) -> Result<AuthState> {
    let normalized = normalize_auth_state(auth)?;
    let next_account = normalize_account(serde_json::to_value(account)?);
    let mut next_accounts = normalized.accounts.clone();
    if let Some(index) = next_accounts
        .iter()
        .position(|item| same_account_identity(item, &next_account))
    {
        next_accounts[index] = merge_accounts(&next_accounts[index], &next_account);
    } else {
        next_accounts.push(next_account.clone());
    }
    build_auth_state(next_accounts, normalized.pending_auth)
}

pub fn set_pending_auth(auth: &AuthState, pending_auth: Option<&AuthAccount>) -> Result<AuthState> {
    let normalized = normalize_auth_state(auth)?;
    build_auth_state(
        normalized.accounts,
        pending_auth
            .map(|account| normalize_account(serde_json::to_value(account).unwrap_or(Value::Null)))
            .filter(has_persisted_auth_data),
    )
}

pub fn clear_pending_auth(auth: &AuthState) -> Result<AuthState> {
    let normalized = normalize_auth_state(auth)?;
    build_auth_state(normalized.accounts, None)
}

pub fn load_auth() -> Result<AuthState> {
    read_json(&get_auth_path(), || Ok(default_auth()), normalize_auth_ref)
}

pub fn save_auth(auth: &AuthState) -> Result<AuthState> {
    write_auth(&get_auth_path(), auth)
}

fn read_json<T, F, N>(path: &Path, fallback: F, normalize: N) -> Result<T>
where
    F: FnOnce() -> Result<T>,
    N: Fn(&Value) -> Result<T>,
{
    if !path.exists() {
        return fallback();
    }
    let text = fs::read_to_string(path)?;
    let value: Value = serde_json::from_str(&text)?;
    normalize(&value)
}

fn write_auth(path: &Path, auth: &AuthState) -> Result<AuthState> {
    ensure_dir(path.parent().unwrap_or_else(|| Path::new(".")))?;
    let normalized = normalize_auth_ref(&auth_storage_value(auth)?)?;
    let mut text = serde_json::to_string_pretty(&auth_storage_value(&normalized)?)?;
    text.push('\n');
    write_private_text_file(path, &text)?;
    Ok(normalized)
}

fn auth_storage_value(auth: &AuthState) -> Result<Value> {
    let normalized = normalize_auth_state(auth)?;
    Ok(json!({
        "accounts": normalized.accounts,
        "pendingAuth": normalized.pending_auth,
    }))
}

fn ensure_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

pub(crate) fn set_private_file_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

pub(crate) fn write_private_text_file(path: &Path, text: &str) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));

    // Write to a same-directory temporary file first, then rename over the destination.
    // This avoids truncating the previous good snapshot if a write fails mid-way.
    #[cfg(unix)]
    {
        use std::io::{self, Write};
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        use std::time::{SystemTime, UNIX_EPOCH};

        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".to_string());

        let mut attempt: u32 = 0;
        let pid = std::process::id();
        let (temp_path, mut file) = loop {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let candidate = parent.join(format!(".{file_name}.tmp.{pid}.{unique}.{attempt}"));
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&candidate)
            {
                Ok(file) => break (candidate, file),
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists && attempt < 10 => {
                    attempt += 1;
                }
                Err(err) => return Err(err.into()),
            }
        };

        let write_result: io::Result<()> = (|| {
            // Ensure private permissions immediately, even if the file already existed due to races.
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
            file.write_all(text.as_bytes())?;
            file.flush()?;
            file.sync_all()?;
            Ok(())
        })();

        if let Err(err) = write_result {
            let _ = fs::remove_file(&temp_path);
            return Err(err.into());
        }

        drop(file);

        if let Err(err) = fs::rename(&temp_path, path) {
            let _ = fs::remove_file(&temp_path);
            return Err(err.into());
        }

        if let Ok(dir_file) = fs::File::open(parent) {
            let _ = dir_file.sync_all();
        }
    }

    #[cfg(not(unix))]
    {
        use std::io::{self, Write};
        use std::time::{SystemTime, UNIX_EPOCH};

        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".to_string());

        let mut attempt: u32 = 0;
        let pid = std::process::id();
        let (temp_path, mut file) = loop {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let candidate = parent.join(format!(".{file_name}.tmp.{pid}.{unique}.{attempt}"));
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(file) => break (candidate, file),
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists && attempt < 10 => {
                    attempt += 1;
                }
                Err(err) => return Err(err.into()),
            }
        };

        let write_result: io::Result<()> = (|| {
            file.write_all(text.as_bytes())?;
            file.flush()?;
            file.sync_all()?;
            Ok(())
        })();

        if let Err(err) = write_result {
            let _ = fs::remove_file(&temp_path);
            return Err(err.into());
        }

        drop(file);

        if let Err(err) = fs::rename(&temp_path, path) {
            // On some platforms the destination can't be atomically overwritten.
            // Preserve any existing file rather than deleting it.
            let _ = fs::remove_file(&temp_path);
            return Err(err.into());
        }

        if let Ok(dir_file) = fs::File::open(parent) {
            let _ = dir_file.sync_all();
        }
    }

    // Postcondition: best-effort ensure private permissions.
    set_private_file_permissions(path)?;
    Ok(())
}

fn normalize_auth_ref(value: &Value) -> Result<AuthState> {
    let object = value.as_object().cloned().unwrap_or_default();
    if object.contains_key("accounts") && !matches!(object.get("accounts"), Some(Value::Array(_))) {
        return Err(anyhow!("auth.accounts 必须是数组"));
    }
    if matches!(
        object.get("pendingAuth"),
        Some(value) if !value.is_null() && !value.is_object()
    ) {
        return Err(anyhow!("auth.pendingAuth 必须是对象或 null"));
    }
    let Some(Value::Array(accounts)) = object.get("accounts") else {
        return Ok(default_auth());
    };

    let mut normalized_accounts = Vec::new();
    for account in accounts {
        push_account(&mut normalized_accounts, account);
    }

    let pending_auth = object
        .get("pendingAuth")
        .filter(|pending| pending.is_object())
        .map(normalize_account_ref)
        .filter(has_persisted_auth_data);

    build_auth_state(normalized_accounts, pending_auth)
}

fn normalize_account_ref(value: &Value) -> AuthAccount {
    let object = value.as_object().cloned().unwrap_or_default();
    let user = object
        .get("user")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    let region = trimmed_value(object.get("region"));
    let redirect_uri = trimmed_value(object.get("redirectUri"));
    let uuid = trimmed_value(object.get("uuid"));

    AuthAccount {
        version: 1,
        region: if region.is_empty() {
            DEFAULT_REGION.to_string()
        } else {
            region
        },
        redirect_uri: if redirect_uri.is_empty() {
            DEFAULT_REDIRECT_URI.to_string()
        } else {
            redirect_uri
        },
        uuid: if uuid.is_empty() {
            generate_uuid()
        } else {
            uuid
        },
        device_id: trimmed_value(object.get("deviceId")),
        state: trimmed_value(object.get("state")),
        access_token: trimmed_value(object.get("accessToken")),
        refresh_token: trimmed_value(object.get("refreshToken")),
        expires_ts: number_value(object.get("expiresTs")),
        user: UserProfile {
            uid: trimmed_value(user.get("uid")),
            nickname: trimmed_value(user.get("nickname")),
            icon: trimmed_value(user.get("icon")),
            union_id: trimmed_value(user.get("unionId")),
        },
    }
}

fn push_account(accounts: &mut Vec<AuthAccount>, candidate: &Value) {
    let account = normalize_account_ref(candidate);
    if !has_persisted_auth_data(&account) {
        return;
    }
    if let Some(index) = accounts
        .iter()
        .position(|item| same_account_identity(item, &account))
    {
        accounts[index] = merge_accounts(&accounts[index], &account);
    } else {
        accounts.push(account);
    }
}

fn same_account_identity(left: &AuthAccount, right: &AuthAccount) -> bool {
    if !left.user.uid.is_empty() && !right.user.uid.is_empty() {
        return left.user.uid == right.user.uid;
    }
    if !left.device_id.is_empty()
        && !right.device_id.is_empty()
        && !left.state.is_empty()
        && !right.state.is_empty()
    {
        return left.device_id == right.device_id && left.state == right.state;
    }
    if !left.refresh_token.is_empty() && !right.refresh_token.is_empty() {
        return left.refresh_token == right.refresh_token;
    }
    false
}

fn merge_accounts(current: &AuthAccount, next: &AuthAccount) -> AuthAccount {
    AuthAccount {
        version: next.version,
        region: next.region.clone(),
        redirect_uri: next.redirect_uri.clone(),
        uuid: next.uuid.clone(),
        device_id: next.device_id.clone(),
        state: next.state.clone(),
        access_token: next.access_token.clone(),
        refresh_token: next.refresh_token.clone(),
        expires_ts: next.expires_ts,
        user: UserProfile {
            uid: next.user.uid.clone().or_if_empty(&current.user.uid),
            nickname: next
                .user
                .nickname
                .clone()
                .or_if_empty(&current.user.nickname),
            icon: next.user.icon.clone().or_if_empty(&current.user.icon),
            union_id: next
                .user
                .union_id
                .clone()
                .or_if_empty(&current.user.union_id),
        },
    }
}

fn build_auth_state(
    accounts: Vec<AuthAccount>,
    pending_auth: Option<AuthAccount>,
) -> Result<AuthState> {
    Ok(AuthState {
        accounts,
        pending_auth,
    })
}

fn trimmed_value(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .map(|text| text.trim().to_string())
        .unwrap_or_default()
}

fn number_value(value: Option<&Value>) -> i64 {
    match value {
        Some(Value::Number(number)) => {
            if let Some(value) = number.as_i64() {
                value
            } else if let Some(value) = number.as_u64() {
                value as i64
            } else {
                number.as_f64().unwrap_or(0.0).trunc() as i64
            }
        }
        Some(Value::String(text)) => text.trim().parse::<f64>().unwrap_or(0.0).trunc() as i64,
        _ => 0,
    }
}

trait OrIfEmpty {
    fn or_if_empty(self, fallback: &str) -> String;
}

impl OrIfEmpty for String {
    fn or_if_empty(self, fallback: &str) -> String {
        if self.is_empty() {
            fallback.to_string()
        } else {
            self
        }
    }
}

#[cfg(test)]
#[path = "../tests/module_tests/storage.rs"]
mod tests;
