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

/// GitHub repository slug (`owner/name`) used for the homepage link and update checks.
pub const GITHUB_REPO: &str = "ttys026/mit";

/// GitHub repository homepage URL.
pub fn github_home_url() -> String {
    format!("https://github.com/{GITHUB_REPO}")
}

/// URL of the install script that `mit update` shells out to in order to upgrade.
pub fn install_script_url() -> String {
    format!("https://raw.githubusercontent.com/{GITHUB_REPO}/main/install.sh")
}

/// Query the GitHub API for the latest published release tag (e.g. `v1.2.0`).
///
/// Blocking call; callers in the TUI run it on a background thread so the event
/// loop never stalls on the network.
pub fn latest_release_tag() -> Result<String> {
    // `MIT_GITHUB_API_BASE` lets tests point the lookup at a mock server.
    let base = std::env::var("MIT_GITHUB_API_BASE")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "https://api.github.com".to_string());
    let url = format!("{base}/repos/{GITHUB_REPO}/releases/latest");
    let response = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?
        .get(url)
        // GitHub rejects API requests without a User-Agent.
        .header("User-Agent", concat!("mit/", env!("CARGO_PKG_VERSION")))
        .header("Accept", "application/vnd.github+json")
        .send()?;
    let status = response.status();
    // Capture rate-limit headers before `text()` consumes the response.
    let remaining = header_value(response.headers(), "x-ratelimit-remaining");
    let reset = header_value(response.headers(), "x-ratelimit-reset")
        .and_then(|value| value.parse::<i64>().ok());
    let text = response.text()?;
    if !status.is_success() {
        let code = status.as_u16();
        // 403/429 with no remaining budget means we hit GitHub's unauthenticated
        // 60-requests/hour limit; say so plainly with a retry hint.
        if (code == 403 || code == 429) && remaining.as_deref() == Some("0") {
            return Err(anyhow!(
                "GitHub 检查更新已达限流上限（未登录每小时 60 次）{}",
                rate_limit_retry_hint(reset)
            ));
        }
        return Err(anyhow!(
            "检查更新失败: status={} body={}",
            code,
            text.trim()
        ));
    }
    let payload: Value = serde_json::from_str(&text)?;
    payload
        .get("tag_name")
        .and_then(Value::as_str)
        .map(|tag| tag.trim().to_string())
        .filter(|tag| !tag.is_empty())
        .ok_or_else(|| anyhow!("GitHub 未返回 tag_name"))
}

fn header_value(headers: &reqwest::header::HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim().to_string())
}

/// Turn an `x-ratelimit-reset` epoch into a "retry in ~N minutes" hint.
fn rate_limit_retry_hint(reset_epoch: Option<i64>) -> String {
    let Some(reset) = reset_epoch else {
        return "，请稍后重试".to_string();
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0);
    let minutes = ((reset - now) + 59) / 60; // round up
    if minutes > 1 {
        format!("，请约 {minutes} 分钟后重试")
    } else {
        "，请稍后重试".to_string()
    }
}

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
    account.mijia.as_ref().ok_or_else(|| {
        anyhow!(
            "账号 {} 未登录米家，请先运行 mit auth login mijia 登录米家",
            account.user.uid
        )
    })
}

fn remove_dir_all_if_exists(path: &Path) -> Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}
