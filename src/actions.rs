//! Front-end-agnostic operations shared by the CLI and the TUI.
//!
//! Both entry points call these functions so that an action behaves identically
//! whether it is triggered from a CLI command or from the full-screen TUI. The
//! lower-level protocol layers (`MicoClient`, `MijiaClient`) are already shared;
//! this module unifies the orchestration that previously lived twice (resolving
//! the profile directory, mutating auth state, and fetching Mijia history).

use anyhow::{anyhow, Result};
use serde_json::Value;
use std::fs;
use std::path::Path;

use crate::mijia_api::{DeviceHistoryQuery, MijiaClient};
use crate::storage::{save_auth, AuthAccount, AuthState};

/// Mijia data type used to query property operation records.
pub const PROP_DATA_TYPE: &str = "prop";

/// Remove `uid` from the auth state (including a matching pending login),
/// persist the change, and delete the account's cache directory under `mit_dir`.
/// Returns the updated, persisted [`AuthState`].
///
/// `mit_dir` is the profile root (`~/.mit`); callers pass [`crate::storage::get_mit_dir`]
/// (CLI) or the TUI's own profile path so behavior is identical from either entry point.
pub fn logout_account(mit_dir: &Path, auth_state: AuthState, uid: &str) -> Result<AuthState> {
    let uid = uid.trim();
    if uid.is_empty() || uid == "-" {
        return Err(anyhow!("当前没有可登出的账号"));
    }
    let mut auth_state = auth_state;
    auth_state
        .accounts
        .retain(|account| account.user.uid != uid);
    if auth_state
        .pending_auth
        .as_ref()
        .is_some_and(|account| account.user.uid == uid)
    {
        auth_state.pending_auth = None;
    }
    let auth_state = save_auth(&auth_state)?;
    remove_dir_all_if_exists(&mit_dir.join("accounts").join(uid))?;
    Ok(auth_state)
}

/// Delete everything under `mit_dir` except `auth.json`, mirroring the TUI's
/// "clear cache (keep auth)" settings action. Returns the number of entries removed.
pub fn clear_device_cache(mit_dir: &Path) -> Result<usize> {
    if !mit_dir.exists() {
        return Ok(0);
    }
    let mut removed = 0usize;
    for entry in fs::read_dir(mit_dir)? {
        let path = entry?.path();
        if path
            .file_name()
            .is_some_and(|name| name == std::ffi::OsStr::new("auth.json"))
        {
            continue;
        }
        if path.is_dir() {
            fs::remove_dir_all(&path)?;
        } else {
            fs::remove_file(&path)?;
        }
        removed += 1;
    }
    Ok(removed)
}

/// Delete the entire `mit_dir` profile directory (auth, caches, everything).
pub fn reset_profile(mit_dir: &Path) -> Result<()> {
    remove_dir_all_if_exists(mit_dir)
}

/// Fetch raw Mijia operation-record logs for a single property `key`
/// (`"<siid>.<piid>"`) of `did`, via the account's Mijia auth.
pub fn device_history(
    account: &AuthAccount,
    did: &str,
    key: &str,
    query: DeviceHistoryQuery,
) -> Result<Value> {
    let mijia = require_mijia(account)?;
    MijiaClient::new()?.get_user_device_data_with_query(mijia, did, key, PROP_DATA_TYPE, query)
}

/// Fetch raw Mijia statistics for a single `key` of `did` for the given
/// `data_type` (e.g. `"stat_day_v3"`), via the account's Mijia auth.
pub fn device_statistics(
    account: &AuthAccount,
    did: &str,
    key: &str,
    data_type: &str,
    query: DeviceHistoryQuery,
) -> Result<Value> {
    let mijia = require_mijia(account)?;
    MijiaClient::new()?.get_user_statistics_with_query(mijia, did, key, data_type, query)
}

fn require_mijia(account: &AuthAccount) -> Result<&crate::storage::MijiaAuth> {
    account
        .mijia
        .as_ref()
        .ok_or_else(|| anyhow!("账号 {} 未登录米家，无法获取历史数据", account.user.uid))
}

fn remove_dir_all_if_exists(path: &Path) -> Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}
