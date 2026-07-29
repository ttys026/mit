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

use crate::mijia_api::{DeviceHistoryQuery, MijiaClient, ThirdCloudGroup};
use crate::storage::{save_auth, AuthAccount, AuthState, MijiaAuth};

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

#[derive(Clone, Debug)]
pub struct ThirdPartyPlatformDevices {
    pub group: ThirdCloudGroup,
    pub devices: Vec<Value>,
}

/// List connected third-party cloud groups and their devices for a Mijia account.
pub fn list_third_party_platforms(auth: &MijiaAuth) -> Result<Vec<ThirdPartyPlatformDevices>> {
    let client = MijiaClient::new()?;
    let groups = client
        .get_thirdcloud_groups(auth)?
        .into_iter()
        .filter(|group| group.bind_status == 0)
        .collect::<Vec<_>>();
    if groups.is_empty() {
        return Ok(Vec::new());
    }

    let group_ids = groups
        .iter()
        .map(|group| group.group_id)
        .collect::<Vec<_>>();
    let device_list = client.get_thirdcloud_device_list(auth, &group_ids)?;
    let mut devices_by_group = third_party_devices_by_group(&device_list);
    Ok(groups
        .into_iter()
        .map(|group| {
            let devices = devices_by_group.remove(&group.group_id).unwrap_or_default();
            ThirdPartyPlatformDevices { group, devices }
        })
        .collect())
}

#[derive(Clone, Debug)]
pub struct ThirdPartyDeviceSyncGroupResult {
    pub group: ThirdCloudGroup,
    pub success: bool,
    pub code: Option<i64>,
    pub message: Option<String>,
    pub result: Option<Value>,
    pub device_count: Option<usize>,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ThirdPartyDeviceSyncSummary {
    pub groups: Vec<ThirdPartyDeviceSyncGroupResult>,
    pub ok_count: usize,
    pub failed_count: usize,
    pub cancelled: bool,
}

#[derive(Clone, Debug)]
pub enum ThirdPartyDeviceSyncProgress {
    Planned(Vec<ThirdCloudGroup>),
    GroupStarted(ThirdCloudGroup),
    GroupFinished(ThirdPartyDeviceSyncGroupResult),
}

impl ThirdPartyDeviceSyncGroupResult {
    pub fn result_text(&self) -> String {
        self.result
            .as_ref()
            .and_then(|value| json_text(Some(value)))
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "ok".to_string())
    }

    pub fn failure_detail(&self) -> String {
        self.error.clone().unwrap_or_else(|| {
            self.message
                .clone()
                .unwrap_or_else(|| "同步失败".to_string())
        })
    }
}

/// Sync status for all bound third-party cloud groups in a Mijia account.
///
/// The API choreography is shared by the CLI and TUI:
/// 1. load bound third-party groups,
/// 2. call `thirdcloud2cloud/sync` for each bound group,
/// 3. refresh `device_list` after successful syncs to report a device count.
///
/// `on_progress` receives owned progress snapshots so interactive front-ends can
/// update their UI. Returning `false` cancels the remaining work.
pub fn sync_third_party_devices<F>(
    auth: &MijiaAuth,
    mut on_progress: F,
) -> Result<ThirdPartyDeviceSyncSummary>
where
    F: FnMut(ThirdPartyDeviceSyncProgress) -> bool,
{
    let client = MijiaClient::new()?;
    let groups = client
        .get_thirdcloud_groups(auth)?
        .into_iter()
        .filter(|group| group.bind_status == 0)
        .collect::<Vec<_>>();
    if !on_progress(ThirdPartyDeviceSyncProgress::Planned(groups.clone())) {
        return Ok(cancelled_third_party_sync(Vec::new()));
    }

    let mut results = Vec::new();
    let mut ok_count = 0usize;
    let mut failed_count = 0usize;
    for group in groups {
        if !on_progress(ThirdPartyDeviceSyncProgress::GroupStarted(group.clone())) {
            return Ok(cancelled_third_party_sync(results));
        }

        let result = sync_third_party_group(&client, auth, group);
        if result.success {
            ok_count += 1;
        } else {
            failed_count += 1;
        }
        results.push(result.clone());
        if !on_progress(ThirdPartyDeviceSyncProgress::GroupFinished(result)) {
            return Ok(ThirdPartyDeviceSyncSummary {
                groups: results,
                ok_count,
                failed_count,
                cancelled: true,
            });
        }
    }

    Ok(ThirdPartyDeviceSyncSummary {
        groups: results,
        ok_count,
        failed_count,
        cancelled: false,
    })
}

fn cancelled_third_party_sync(
    groups: Vec<ThirdPartyDeviceSyncGroupResult>,
) -> ThirdPartyDeviceSyncSummary {
    let ok_count = groups.iter().filter(|group| group.success).count();
    let failed_count = groups.len().saturating_sub(ok_count);
    ThirdPartyDeviceSyncSummary {
        groups,
        ok_count,
        failed_count,
        cancelled: true,
    }
}

fn sync_third_party_group(
    client: &MijiaClient,
    auth: &MijiaAuth,
    group: ThirdCloudGroup,
) -> ThirdPartyDeviceSyncGroupResult {
    match client.sync_thirdcloud_group(auth, group.group_id) {
        Ok(envelope) => {
            let code = json_i64(envelope.get("code"));
            let message = json_text(envelope.get("message"))
                .or_else(|| json_text(envelope.get("desc")))
                .filter(|value| !value.trim().is_empty());
            let success = code == Some(0);
            let device_count = success
                .then(|| {
                    client
                        .get_thirdcloud_device_list(auth, &[group.group_id])
                        .ok()
                })
                .flatten()
                .and_then(|device_list| third_party_device_count(&device_list, group.group_id));
            let error = (!success).then(|| third_party_failure_detail(&envelope));
            ThirdPartyDeviceSyncGroupResult {
                group,
                success,
                code,
                message,
                result: envelope.get("result").cloned(),
                device_count,
                error,
            }
        }
        Err(error) => ThirdPartyDeviceSyncGroupResult {
            group,
            success: false,
            code: None,
            message: None,
            result: None,
            device_count: None,
            error: Some(error.to_string()),
        },
    }
}

fn third_party_failure_detail(envelope: &Value) -> String {
    let code = json_i64(envelope.get("code")).unwrap_or_default();
    let message = json_text(envelope.get("message"))
        .or_else(|| json_text(envelope.get("desc")))
        .or_else(|| json_text(envelope.get("result")))
        .unwrap_or_default();
    if message.is_empty() {
        format!("code={code}")
    } else {
        format!("code={code}, {message}")
    }
}

fn third_party_device_count(envelope: &Value, group_id: i64) -> Option<usize> {
    envelope
        .get("result")
        .and_then(|result| result.get("list"))
        .and_then(Value::as_array)?
        .iter()
        .find(|group| json_i64(group.get("group_id")) == Some(group_id))
        .and_then(|group| group.get("dev_list"))
        .and_then(Value::as_array)
        .map(Vec::len)
}

fn third_party_devices_by_group(envelope: &Value) -> std::collections::HashMap<i64, Vec<Value>> {
    envelope
        .get("result")
        .and_then(|result| result.get("list"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|group| {
            let group_id =
                json_i64(group.get("group_id")).or_else(|| json_i64(group.get("groupId")))?;
            let devices = group
                .get("dev_list")
                .or_else(|| group.get("devices"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            Some((group_id, devices))
        })
        .collect()
}

fn json_i64(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn json_text(value: Option<&Value>) -> Option<String> {
    let value = value?;
    match value {
        Value::String(text) => Some(text.trim().to_string()),
        Value::Number(_) | Value::Bool(_) => Some(value.to_string()),
        Value::Null => None,
        other => serde_json::to_string(other).ok(),
    }
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
