use anyhow::{anyhow, Result};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_REGION: &str = "cn";
pub const DEFAULT_REDIRECT_URI: &str = "http://127.0.0.1:8000/login_redirect";

/// Current on-disk schema version for `auth.json` accounts. Version 1 stored the
/// Xiaomi credentials as flat fields on each account; version 2 nests them under
/// an `xiaomi` object and adds an optional `mijia` object. Accounts read at an
/// older version are migrated to this version (see [`normalize_account_ref`]).
pub const CURRENT_AUTH_VERSION: u32 = 2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Language {
    #[default]
    Chinese,
    English,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserSettings {
    #[serde(default)]
    pub language: Language,
    #[serde(default = "default_auto_subscribe_device_status")]
    pub auto_subscribe_device_status: bool,
}

fn default_auto_subscribe_device_status() -> bool {
    true
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            language: Language::default(),
            auto_subscribe_device_status: default_auto_subscribe_device_status(),
        }
    }
}

pub fn get_settings_path() -> PathBuf {
    get_mit_dir().join("settings.json")
}

pub fn load_settings() -> UserSettings {
    let path = get_settings_path();
    if !path.exists() {
        return UserSettings::default();
    }
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return UserSettings::default(),
    };
    serde_json::from_str::<UserSettings>(&text).unwrap_or_default()
}

pub fn save_settings(settings: &UserSettings) -> Result<()> {
    let path = get_settings_path();
    ensure_dir(path.parent().unwrap_or_else(|| Path::new(".")))?;
    let mut text = serde_json::to_string_pretty(settings)?;
    text.push('\n');
    write_private_text_file(&path, &text)
}

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
    pub xiaomi: Option<XiaomiAuth>,
    pub mijia: Option<MijiaAuth>,
    pub user: UserProfile,
    pub version: u32,
    #[serde(skip)]
    pub region: String,
    #[serde(skip)]
    pub redirect_uri: String,
    #[serde(skip)]
    pub uuid: String,
    #[serde(skip)]
    pub device_id: String,
    #[serde(skip)]
    pub state: String,
    #[serde(skip)]
    pub access_token: String,
    #[serde(skip)]
    pub refresh_token: String,
    #[serde(skip)]
    pub expires_ts: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct XiaomiAuth {
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub redirect_uri: String,
    #[serde(default)]
    pub uuid: String,
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default)]
    pub expires_ts: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MijiaAuth {
    #[serde(default, rename = "ua")]
    pub ua: String,
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub pass_o: String,
    #[serde(default)]
    pub ssecurity: String,
    #[serde(default)]
    pub pass_token: String,
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub c_user_id: String,
    #[serde(default)]
    pub service_token: String,
    #[serde(default)]
    pub expire_time: i64,
    #[serde(default)]
    pub save_time: i64,
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
    normalize_account_ref(&value, true)
}

pub fn normalize_auth(value: Value) -> Result<AuthState> {
    normalize_auth_ref(&value)
}

pub fn normalize_auth_state(auth: &AuthState) -> Result<AuthState> {
    normalize_auth(serde_json::to_value(auth)?)
}

pub fn has_persisted_auth_data(account: &AuthAccount) -> bool {
    has_xiaomi_auth_data(account.xiaomi.as_ref()) || has_mijia_auth_data(account.mijia.as_ref())
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
    let merge_indexes = account_merge_indexes(&next_accounts, &next_account);
    if let Some(first_index) = merge_indexes.first().copied() {
        let mut merged = next_account;
        for index in &merge_indexes {
            merged = merge_accounts(&next_accounts[*index], &merged);
        }
        let merge_index_set = merge_indexes.to_vec();
        next_accounts = next_accounts
            .into_iter()
            .enumerate()
            .filter_map(|(index, account)| {
                if index == first_index {
                    Some(merged.clone())
                } else if merge_index_set.contains(&index) {
                    None
                } else {
                    Some(account)
                }
            })
            .collect();
    } else {
        next_accounts.push(next_account);
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
        .map(|pending| normalize_account_ref(pending, false))
        .filter(has_persisted_auth_data);

    build_auth_state(normalized_accounts, pending_auth)
}

fn normalize_account_ref(value: &Value, allow_flat_xiaomi: bool) -> AuthAccount {
    let object = value.as_object().cloned().unwrap_or_default();
    let user = object
        .get("user")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    // Legacy (version < 2) accounts stored the Xiaomi credentials as flat fields
    // on the account itself. Migrate them by wrapping those fields into `xiaomi`;
    // such accounts never had Mijia credentials, so `mijia` becomes null.
    let stored_version = object.get("version").and_then(Value::as_u64).unwrap_or(0) as u32;
    let allow_flat_xiaomi = allow_flat_xiaomi || stored_version < CURRENT_AUTH_VERSION;

    let xiaomi = object
        .get("xiaomi")
        .and_then(normalize_xiaomi_auth_ref)
        .or_else(|| {
            allow_flat_xiaomi
                .then(|| normalize_xiaomi_auth_ref(value))
                .flatten()
        });
    let mijia = object.get("mijia").and_then(normalize_mijia_auth_ref);
    let mijia_uid = mijia.as_ref().map(mijia_identity).unwrap_or_default();
    let user_uid = trimmed_value(user.get("uid")).or_if_empty(&mijia_uid);
    let xiaomi_values = xiaomi.clone().unwrap_or_default();

    AuthAccount {
        version: CURRENT_AUTH_VERSION,
        xiaomi,
        mijia,
        user: UserProfile {
            uid: user_uid,
            nickname: trimmed_value(user.get("nickname")),
            icon: trimmed_value(user.get("icon")),
            union_id: trimmed_value(user.get("unionId")),
        },
        region: xiaomi_values.region,
        redirect_uri: xiaomi_values.redirect_uri,
        uuid: xiaomi_values.uuid,
        device_id: xiaomi_values.device_id,
        state: xiaomi_values.state,
        access_token: xiaomi_values.access_token,
        refresh_token: xiaomi_values.refresh_token,
        expires_ts: xiaomi_values.expires_ts,
    }
}

fn normalize_xiaomi_auth_ref(value: &Value) -> Option<XiaomiAuth> {
    if !value.is_object() {
        return None;
    }
    let object = value.as_object().cloned().unwrap_or_default();
    let raw_uuid = trimmed_value(object.get("uuid"));
    let raw_device_id = trimmed_value(object.get("deviceId"));
    let raw_state = trimmed_value(object.get("state"));
    let raw_access_token = trimmed_value(object.get("accessToken"));
    let raw_refresh_token = trimmed_value(object.get("refreshToken"));
    let raw_expires_ts = number_value(object.get("expiresTs"));
    if raw_uuid.is_empty()
        && raw_device_id.is_empty()
        && raw_state.is_empty()
        && raw_access_token.is_empty()
        && raw_refresh_token.is_empty()
        && raw_expires_ts == 0
    {
        return None;
    }
    let region = trimmed_value(object.get("region"));
    let redirect_uri = trimmed_value(object.get("redirectUri"));
    let auth = XiaomiAuth {
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
        uuid: if raw_uuid.is_empty() {
            generate_uuid()
        } else {
            raw_uuid
        },
        device_id: raw_device_id,
        state: raw_state,
        access_token: raw_access_token,
        refresh_token: raw_refresh_token,
        expires_ts: raw_expires_ts,
    };
    has_xiaomi_auth_data(Some(&auth)).then_some(auth)
}

fn normalize_mijia_auth_ref(value: &Value) -> Option<MijiaAuth> {
    let object = value.as_object().cloned().unwrap_or_default();
    let auth = MijiaAuth {
        ua: trimmed_value(object.get("ua")),
        device_id: trimmed_value(object.get("deviceId")),
        pass_o: trimmed_value(object.get("passO")),
        ssecurity: trimmed_value(object.get("ssecurity")),
        pass_token: trimmed_value(object.get("passToken")),
        user_id: trimmed_value(object.get("userId")),
        c_user_id: trimmed_value(object.get("cUserId")),
        service_token: trimmed_value(object.get("serviceToken")),
        expire_time: number_value(object.get("expireTime")),
        save_time: number_value(object.get("saveTime")),
    };
    has_mijia_auth_data(Some(&auth)).then_some(auth)
}

pub fn has_mijia_auth_data(auth: Option<&MijiaAuth>) -> bool {
    auth.is_some_and(|auth| {
        !auth.service_token.is_empty()
            || !auth.ssecurity.is_empty()
            || !auth.user_id.is_empty()
            || !auth.c_user_id.is_empty()
    })
}

pub fn has_xiaomi_auth_data(auth: Option<&XiaomiAuth>) -> bool {
    auth.is_some_and(|auth| {
        !auth.device_id.is_empty()
            || !auth.state.is_empty()
            || !auth.access_token.is_empty()
            || !auth.refresh_token.is_empty()
            || !auth.uuid.is_empty()
    })
}

pub fn sync_xiaomi_auth(account: &mut AuthAccount) {
    let auth = XiaomiAuth {
        region: account.region.clone(),
        redirect_uri: account.redirect_uri.clone(),
        uuid: account.uuid.clone(),
        device_id: account.device_id.clone(),
        state: account.state.clone(),
        access_token: account.access_token.clone(),
        refresh_token: account.refresh_token.clone(),
        expires_ts: account.expires_ts,
    };
    account.xiaomi = has_xiaomi_auth_data(Some(&auth)).then_some(auth);
}

pub fn mijia_identity(auth: &MijiaAuth) -> String {
    auth.user_id.clone().or_if_empty(&auth.c_user_id)
}

fn push_account(accounts: &mut Vec<AuthAccount>, candidate: &Value) {
    let account = normalize_account_ref(candidate, false);
    if !has_persisted_auth_data(&account) {
        return;
    }
    let merge_indexes = account_merge_indexes(accounts, &account);
    if let Some(first_index) = merge_indexes.first().copied() {
        let mut merged = account;
        for index in &merge_indexes {
            merged = merge_accounts(&accounts[*index], &merged);
        }
        let merge_index_set = merge_indexes.to_vec();
        let next_accounts = accounts
            .drain(..)
            .enumerate()
            .filter_map(|(index, account)| {
                if index == first_index {
                    Some(merged.clone())
                } else if merge_index_set.contains(&index) {
                    None
                } else {
                    Some(account)
                }
            })
            .collect();
        *accounts = next_accounts;
    } else {
        accounts.push(account);
    }
}

fn account_merge_indexes(accounts: &[AuthAccount], account: &AuthAccount) -> Vec<usize> {
    accounts
        .iter()
        .enumerate()
        .filter_map(|(index, item)| same_account_identity(item, account).then_some(index))
        .collect::<Vec<_>>()
}

fn same_account_identity(left: &AuthAccount, right: &AuthAccount) -> bool {
    if !left.user.uid.is_empty() && !right.user.uid.is_empty() && left.user.uid == right.user.uid {
        return true;
    }
    if let (Some(left_mijia), Some(right_mijia)) = (&left.mijia, &right.mijia) {
        if !left_mijia.user_id.is_empty()
            && !right_mijia.user_id.is_empty()
            && left_mijia.user_id == right_mijia.user_id
        {
            return true;
        }
        if !left_mijia.c_user_id.is_empty()
            && !right_mijia.c_user_id.is_empty()
            && left_mijia.c_user_id == right_mijia.c_user_id
        {
            return true;
        }
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
    let xiaomi = merge_xiaomi_auth(current.xiaomi.as_ref(), next.xiaomi.as_ref());
    let xiaomi_values = xiaomi.clone().unwrap_or_default();
    AuthAccount {
        version: next.version,
        xiaomi,
        mijia: merge_mijia_auth(current.mijia.as_ref(), next.mijia.as_ref()),
        user: merge_user_profile(current, next),
        region: xiaomi_values.region,
        redirect_uri: xiaomi_values.redirect_uri,
        uuid: xiaomi_values.uuid,
        device_id: xiaomi_values.device_id,
        state: xiaomi_values.state,
        access_token: xiaomi_values.access_token,
        refresh_token: xiaomi_values.refresh_token,
        expires_ts: xiaomi_values.expires_ts,
    }
}

fn merge_user_profile(current: &AuthAccount, next: &AuthAccount) -> UserProfile {
    let prefer_next = next.xiaomi.is_some() || current.xiaomi.is_none();
    let (primary, fallback) = if prefer_next {
        (&next.user, &current.user)
    } else {
        (&current.user, &next.user)
    };
    UserProfile {
        uid: primary.uid.clone().or_if_empty(&fallback.uid),
        nickname: primary.nickname.clone().or_if_empty(&fallback.nickname),
        icon: primary.icon.clone().or_if_empty(&fallback.icon),
        union_id: primary.union_id.clone().or_if_empty(&fallback.union_id),
    }
}

fn merge_xiaomi_auth(
    current: Option<&XiaomiAuth>,
    next: Option<&XiaomiAuth>,
) -> Option<XiaomiAuth> {
    match (current, next) {
        (None, None) => None,
        (Some(current), None) => Some(current.clone()),
        (None, Some(next)) => Some(next.clone()),
        (Some(current), Some(next)) => Some(XiaomiAuth {
            region: next.region.clone().or_if_empty(&current.region),
            redirect_uri: next.redirect_uri.clone().or_if_empty(&current.redirect_uri),
            uuid: next.uuid.clone().or_if_empty(&current.uuid),
            device_id: next.device_id.clone().or_if_empty(&current.device_id),
            state: next.state.clone().or_if_empty(&current.state),
            access_token: next.access_token.clone().or_if_empty(&current.access_token),
            refresh_token: next
                .refresh_token
                .clone()
                .or_if_empty(&current.refresh_token),
            expires_ts: if next.expires_ts == 0 {
                current.expires_ts
            } else {
                next.expires_ts
            },
        }),
    }
}

fn merge_mijia_auth(current: Option<&MijiaAuth>, next: Option<&MijiaAuth>) -> Option<MijiaAuth> {
    match (current, next) {
        (None, None) => None,
        (Some(current), None) => Some(current.clone()),
        (None, Some(next)) => Some(next.clone()),
        (Some(current), Some(next)) => Some(MijiaAuth {
            ua: next.ua.clone().or_if_empty(&current.ua),
            device_id: next.device_id.clone().or_if_empty(&current.device_id),
            pass_o: next.pass_o.clone().or_if_empty(&current.pass_o),
            ssecurity: next.ssecurity.clone().or_if_empty(&current.ssecurity),
            pass_token: next.pass_token.clone().or_if_empty(&current.pass_token),
            user_id: next.user_id.clone().or_if_empty(&current.user_id),
            c_user_id: next.c_user_id.clone().or_if_empty(&current.c_user_id),
            service_token: next
                .service_token
                .clone()
                .or_if_empty(&current.service_token),
            expire_time: if next.expire_time == 0 {
                current.expire_time
            } else {
                next.expire_time
            },
            save_time: if next.save_time == 0 {
                current.save_time
            } else {
                next.save_time
            },
        }),
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
