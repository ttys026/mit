//! Command handlers: auth logout, device history (logs/stats), cache/reset,
//! devices, props (get/set/act/sub), push — plus the shared account/device
//! resolution and push helpers reused by the TUI.
use anyhow::{bail, Result};
use serde::Serialize;
use serde_json::Value;

use crate::mico_api::{MicoClient, MiotChannel};
use crate::mijia_api::DeviceHistoryQuery;
use crate::mips_cloud::{
    config_from_account, property_subscription_for, start_stdout_subscription, CloudMipsHandle,
    CloudMipsSubscription,
};
use crate::storage::{
    get_auth_accounts, get_mit_dir, load_auth, save_auth, AuthAccount, AuthState,
};

use super::*;

pub(in crate::cli) fn handle_auth_logout(
    output_mode: OutputMode,
    args: AuthLogoutArgs,
) -> Result<()> {
    let auth_state = load_auth()?;
    let uids = get_auth_accounts(&auth_state)?
        .into_iter()
        .map(|account| account.user.uid)
        .filter(|uid| !uid.trim().is_empty())
        .collect::<Vec<_>>();
    let uid = match args.uid {
        Some(uid) => {
            if !uids.iter().any(|known| known == &uid) {
                bail!("未找到账号 {uid}");
            }
            uid
        }
        None => match uids.as_slice() {
            [] => bail!("没有可登出的账号"),
            [single] => single.clone(),
            _ => bail!("存在多个账号，请用 --uid 指定要登出的账号"),
        },
    };
    crate::actions::logout_account(&get_mit_dir(), auth_state, &uid)?;
    match output_mode {
        OutputMode::Text => println!("✅ 已登出账号 {uid}"),
        OutputMode::Json => print_json(&LogoutOutput {
            kind: "authLogout",
            uid,
        })?,
    }
    Ok(())
}

pub(in crate::cli) fn handle_logs(output_mode: OutputMode, args: LogsArgs) -> Result<()> {
    let target = find_target_device(&args.did)?;
    let did = target.device.did.as_str();
    let mut entries = Vec::new();
    for key in &args.keys {
        let query = DeviceHistoryQuery::recent(args.limit);
        let value = crate::actions::device_history(&target.fresh.auth, did, key, query)?;
        match output_mode {
            OutputMode::Text => println!(
                "{}（{did}） {key} => {}",
                target.device.name,
                serde_json::to_string(&value)?
            ),
            OutputMode::Json => entries.push(DeviceDataEntry {
                key: key.clone(),
                value,
            }),
        }
    }
    if output_mode == OutputMode::Json {
        print_json(&DeviceDataOutput {
            kind: "deviceLogs",
            device_did: target.device.did,
            device_name: target.device.name,
            entries,
        })?;
    }
    Ok(())
}

pub(in crate::cli) fn handle_stats(output_mode: OutputMode, args: StatsArgs) -> Result<()> {
    let target = find_target_device(&args.did)?;
    let did = target.device.did.as_str();
    let query = DeviceHistoryQuery::recent(args.limit);
    let value = crate::actions::device_statistics(
        &target.fresh.auth,
        did,
        &args.key,
        args.period.data_type(),
        query,
    )?;
    match output_mode {
        OutputMode::Text => println!(
            "{}（{did}） {} [{:?}] => {}",
            target.device.name,
            args.key,
            args.period,
            serde_json::to_string(&value)?
        ),
        OutputMode::Json => print_json(&DeviceDataOutput {
            kind: "deviceStatistics",
            device_did: target.device.did,
            device_name: target.device.name,
            entries: vec![DeviceDataEntry {
                key: args.key,
                value,
            }],
        })?,
    }
    Ok(())
}

pub(in crate::cli) fn handle_cache(output_mode: OutputMode, args: CacheArgs) -> Result<()> {
    match args.command {
        None => show_subcommand_help(output_mode, "cache"),
        Some(CacheCommand::Clean) => {
            let removed = crate::actions::clear_device_cache(&get_mit_dir())?;
            match output_mode {
                OutputMode::Text => println!("✅ 已清理缓存，删除 {removed} 项（保留登录）"),
                OutputMode::Json => print_json(&CacheCleanOutput {
                    kind: "cacheClean",
                    removed,
                })?,
            }
            Ok(())
        }
    }
}

pub(in crate::cli) fn handle_reset(output_mode: OutputMode, args: ResetArgs) -> Result<()> {
    if !args.yes {
        bail!("此操作会删除 ~/.mit 下的全部数据；如确认请加 --yes");
    }
    crate::actions::reset_profile(&get_mit_dir())?;
    match output_mode {
        OutputMode::Text => println!("✅ 已重置全部数据（~/.mit 已删除）"),
        OutputMode::Json => print_json(&ResetOutput { kind: "reset" })?,
    }
    Ok(())
}

pub(in crate::cli) fn handle_update(output_mode: OutputMode, args: UpdateArgs) -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    let latest = crate::actions::latest_release_tag()?;
    // Release tags are `vX.Y.Z`; compare against the bare cargo version.
    let up_to_date = latest.trim_start_matches('v') == current;

    match output_mode {
        OutputMode::Text if up_to_date => println!("✅ 已是最新版本 v{current}"),
        OutputMode::Text => println!("发现新版本 {latest}（当前 v{current}）"),
        OutputMode::Json => print_json(&UpdateCheckOutput {
            kind: "updateCheck",
            current,
            latest: latest.as_str(),
            up_to_date,
        })?,
    }

    // `--check`, JSON mode, or already-current: report only, never run the installer.
    if args.check || up_to_date || output_mode == OutputMode::Json {
        return Ok(());
    }

    println!("正在通过安装脚本升级…");
    run_install_script()?;
    println!("✅ 升级完成，请重新运行 mit 验证版本。");
    Ok(())
}

/// Shell out to the project's install script to upgrade in place.
#[cfg(not(target_os = "windows"))]
fn run_install_script() -> Result<()> {
    let command = format!("curl -sSfL {} | sh", crate::actions::install_script_url());
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(&command)
        .status()?;
    if !status.success() {
        bail!("安装脚本执行失败（退出码 {:?}）", status.code());
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn run_install_script() -> Result<()> {
    bail!(
        "Windows 暂不支持自动升级，请访问 {} 手动下载最新版本",
        crate::actions::github_home_url()
    );
}

pub(in crate::cli) fn handle_devices(output_mode: OutputMode, args: DevicesArgs) -> Result<()> {
    let Some(command) = args.command else {
        return show_subcommand_help(output_mode, "devices");
    };
    match command {
        DevicesCommand::List => {
            let entries = handle_devices_list()?;
            match output_mode {
                OutputMode::Text => {
                    let mut current_uid = String::new();
                    for entry in &entries {
                        if entry.uid != current_uid {
                            current_uid = entry.uid.clone();
                            println!("{}（{}）:", entry.nickname, entry.uid);
                        }
                        println!("  {}", format_device_line_in_group(&entry.device));
                    }
                }
                OutputMode::Json => {
                    let mut groups: Vec<AccountDevicesGroupOutput> = Vec::new();
                    for entry in entries {
                        if let Some(group) = groups.iter_mut().find(|g| g.uid == entry.uid) {
                            group.devices.push(entry.device);
                        } else {
                            groups.push(AccountDevicesGroupOutput {
                                uid: entry.uid,
                                nickname: entry.nickname,
                                devices: vec![entry.device],
                            });
                        }
                    }
                    print_json(&DevicesListOutput {
                        kind: "devicesList",
                        accounts: groups,
                    })?;
                }
            }
            Ok(())
        }
    }
}

fn handle_devices_list() -> Result<Vec<AccountDevice>> {
    let auth_state = load_auth()?;
    let accounts = get_auth_accounts(&auth_state)?
        .into_iter()
        .filter(|a| !a.access_token.is_empty() || !a.refresh_token.is_empty())
        .collect::<Vec<_>>();
    if accounts.is_empty() {
        bail!("未授权，请先执行 mit auth login");
    }
    let mut result = Vec::new();
    for account in accounts {
        let fresh = ensure_fresh_account(auth_state.clone(), account)?;
        for device in fresh.client.get_devices()? {
            result.push(AccountDevice {
                uid: fresh.auth.user.uid.clone(),
                nickname: fresh.auth.user.nickname.clone(),
                device,
            });
        }
    }
    Ok(result)
}

#[derive(Clone)]
struct TargetDevice {
    fresh: FreshAuth,
    device: crate::mico_api::Device,
}

fn normalize_command_did(did: &str) -> String {
    if let Some((prefix, suffix)) = did.rsplit_once(".s") {
        if !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit()) {
            return prefix.to_string();
        }
    }
    did.to_string()
}

fn find_target_device(did: &str) -> Result<TargetDevice> {
    let auth_state = load_auth()?;
    let accounts = get_auth_accounts(&auth_state)?
        .into_iter()
        .filter(|a| !a.access_token.is_empty() || !a.refresh_token.is_empty())
        .collect::<Vec<_>>();
    if accounts.is_empty() {
        bail!("未授权，请先执行 mit auth login");
    }
    let normalized_did = normalize_command_did(did);

    // Fast path: resolve the device from the on-disk cache the TUI/sync writes
    // (`devices.json`), so a single `props` command does not pay for a full cloud
    // device-list fetch. The LAN credential (IP + token) comes from the snapshot,
    // so this whole path stays off the network.
    for account in &accounts {
        let cached = load_cached_devices(&account.user.uid)
            .into_iter()
            .find(|device| normalize_command_did(device.did.as_str()) == normalized_did);
        if let Some(device) = cached {
            let fresh = ensure_fresh_account(auth_state.clone(), account.clone())?;
            return Ok(TargetDevice { fresh, device });
        }
    }

    // Slow path: the device isn't cached yet — fall back to the cloud device list.
    let mut working_auth_state = auth_state;
    for account in accounts {
        let fresh = ensure_fresh_account(working_auth_state.clone(), account)?;
        working_auth_state = fresh.auth_state.clone();
        for device in fresh.client.get_devices()? {
            if normalize_command_did(device.did.as_str()) == normalized_did {
                return Ok(TargetDevice { fresh, device });
            }
        }
    }
    bail!("未找到设备 {did}");
}

/// Read the per-account `devices.json` cache (written by the TUI / device sync)
/// without any cloud round-trip. Tolerates both the `{ "devices": [...] }` and
/// bare-array layouts; returns empty on any miss so callers fall back to cloud.
fn load_cached_devices(uid: &str) -> Vec<crate::mico_api::Device> {
    let uid = uid.trim();
    if uid.is_empty() {
        return Vec::new();
    }
    let path = crate::storage::get_account_dir(uid).join("devices.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    let devices = if value.is_array() {
        value
    } else {
        value.get("devices").cloned().unwrap_or(Value::Null)
    };
    serde_json::from_value::<Vec<crate::mico_api::Device>>(devices).unwrap_or_default()
}

fn parse_cli_value(text: &str) -> Result<Value> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        bail!("输入不能为空");
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Ok(value);
    }
    Ok(Value::String(trimmed.to_string()))
}

/// Trailing "（通道: LAN）" / "（通道: Cloud）" tag for text-mode prop output.
fn channel_text_suffix(channel: Option<MiotChannel>) -> String {
    match channel {
        Some(channel) => format!("（通道: {channel}）"),
        None => String::new(),
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PropGetOutput {
    #[serde(rename = "type")]
    kind: &'static str,
    device_did: String,
    device_name: String,
    siid: i64,
    piid: i64,
    value: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    channel: Option<MiotChannel>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PropSetOutput {
    #[serde(rename = "type")]
    kind: &'static str,
    device_did: String,
    device_name: String,
    siid: i64,
    piid: i64,
    value: Value,
    result: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    channel: Option<MiotChannel>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PropActOutput {
    #[serde(rename = "type")]
    kind: &'static str,
    device_did: String,
    device_name: String,
    siid: i64,
    aiid: i64,
    values: Vec<Value>,
    result: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    channel: Option<MiotChannel>,
}

struct PropsSubscriptionGroup {
    account: AuthAccount,
    subscriptions: Vec<CloudMipsSubscription>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LogoutOutput {
    #[serde(rename = "type")]
    kind: &'static str,
    uid: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeviceDataEntry {
    key: String,
    value: Value,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeviceDataOutput {
    #[serde(rename = "type")]
    kind: &'static str,
    device_did: String,
    device_name: String,
    entries: Vec<DeviceDataEntry>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CacheCleanOutput {
    #[serde(rename = "type")]
    kind: &'static str,
    removed: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ResetOutput {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateCheckOutput<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    current: &'a str,
    latest: &'a str,
    up_to_date: bool,
}

pub(in crate::cli) fn handle_props(output_mode: OutputMode, args: PropsArgs) -> Result<()> {
    let Some(command) = args.command else {
        return show_subcommand_help(output_mode, "props");
    };
    match command {
        PropsCommand::Get(args) => {
            let target = find_target_device(&args.did)?;
            let _ = target
                .fresh
                .client
                .prime_local_credential_for(target.device.did.as_str());
            let value =
                target
                    .fresh
                    .client
                    .get_prop(target.device.did.as_str(), args.siid, args.piid)?;
            let channel = target.fresh.client.last_channel();
            match output_mode {
                OutputMode::Text => {
                    println!(
                        "{}（{}） {}.{} => {}{}",
                        target.device.name,
                        target.device.did,
                        args.siid,
                        args.piid,
                        serde_json::to_string(&value)?,
                        channel_text_suffix(channel),
                    );
                }
                OutputMode::Json => print_json(&PropGetOutput {
                    kind: "propGet",
                    device_did: target.device.did,
                    device_name: target.device.name,
                    siid: args.siid,
                    piid: args.piid,
                    value,
                    channel,
                })?,
            }
            Ok(())
        }
        PropsCommand::Set(args) => {
            let target = find_target_device(&args.did)?;
            let _ = target
                .fresh
                .client
                .prime_local_credential_for(target.device.did.as_str());
            let value = parse_cli_value(&args.value)?;
            let result = target.fresh.client.set_prop(
                target.device.did.as_str(),
                args.siid,
                args.piid,
                value.clone(),
            )?;
            let channel = target.fresh.client.last_channel();
            match output_mode {
                OutputMode::Text => {
                    println!(
                        "{}（{}） {}.{} <= {} => {}{}",
                        target.device.name,
                        target.device.did,
                        args.siid,
                        args.piid,
                        serde_json::to_string(&value)?,
                        serde_json::to_string(&result)?,
                        channel_text_suffix(channel),
                    );
                }
                OutputMode::Json => print_json(&PropSetOutput {
                    kind: "propSet",
                    device_did: target.device.did,
                    device_name: target.device.name,
                    siid: args.siid,
                    piid: args.piid,
                    value,
                    result,
                    channel,
                })?,
            }
            Ok(())
        }
        PropsCommand::Act(args) => {
            let target = find_target_device(&args.did)?;
            let _ = target
                .fresh
                .client
                .prime_local_credential_for(target.device.did.as_str());
            let values = args
                .values
                .iter()
                .map(|value| parse_cli_value(value))
                .collect::<Result<Vec<_>>>()?;
            let result = target.fresh.client.action(
                target.device.did.as_str(),
                args.siid,
                args.aiid,
                values.as_slice(),
            )?;
            let channel = target.fresh.client.last_channel();
            match output_mode {
                OutputMode::Text => {
                    println!(
                        "{}（{}） {}.{} => {}{}",
                        target.device.name,
                        target.device.did,
                        args.siid,
                        args.aiid,
                        serde_json::to_string(&result)?,
                        channel_text_suffix(channel),
                    );
                }
                OutputMode::Json => print_json(&PropActOutput {
                    kind: "propAct",
                    device_did: target.device.did,
                    device_name: target.device.name,
                    siid: args.siid,
                    aiid: args.aiid,
                    values,
                    result,
                    channel,
                })?,
            }
            Ok(())
        }
        PropsCommand::Sub(args) => handle_props_sub(output_mode, args),
    }
}

fn handle_props_sub(output_mode: OutputMode, args: PropsSubArgs) -> Result<()> {
    if output_mode == OutputMode::Json {
        bail!("props sub 不支持 --json");
    }
    let property = subscription_property_selector(&args)?;
    let groups = props_subscription_groups(args.did.as_deref(), property)?;
    let account_count = groups.len();
    let filter_count = groups
        .iter()
        .map(|group| group.subscriptions.len())
        .sum::<usize>();
    if groups.is_empty() || filter_count == 0 {
        bail!("未找到可订阅设备");
    }

    let (tx, rx) = mpsc::channel();
    let mut handles: Vec<CloudMipsHandle> = Vec::new();
    for group in groups {
        let config = config_from_account(&group.account)?;
        let handle = start_stdout_subscription(config, group.subscriptions, tx.clone())?;
        handles.push(handle);
    }
    drop(tx);

    println!(
        "cloud MIPS subscription started: accounts={account_count} subscriptions={filter_count}"
    );
    for line in rx {
        println!("{line}");
    }
    drop(handles);

    Ok(())
}

fn subscription_property_selector(args: &PropsSubArgs) -> Result<Option<(i64, i64)>> {
    match (args.siid, args.piid) {
        (Some(siid), Some(piid)) => Ok(Some((siid, piid))),
        (None, None) => Ok(None),
        _ => bail!("siid 和 piid 必须同时提供"),
    }
}

fn props_subscription_groups(
    did: Option<&str>,
    property: Option<(i64, i64)>,
) -> Result<Vec<PropsSubscriptionGroup>> {
    if let Some(did) = did {
        let target = find_target_device(did)?;
        return Ok(vec![PropsSubscriptionGroup {
            account: target.fresh.auth,
            subscriptions: vec![property_subscription_for(
                target.device.did.as_str(),
                property,
            )],
        }]);
    }
    if property.is_some() {
        bail!("指定 siid/piid 时也必须指定 did");
    }

    let auth_state = load_auth()?;
    let accounts = get_auth_accounts(&auth_state)?
        .into_iter()
        .filter(|account| !account.access_token.is_empty() || !account.refresh_token.is_empty())
        .collect::<Vec<_>>();
    if accounts.is_empty() {
        bail!("未授权，请先执行 mit auth login");
    }

    let mut working_auth_state = auth_state;
    let mut groups = Vec::new();
    for account in accounts {
        let fresh = ensure_fresh_account(working_auth_state.clone(), account)?;
        working_auth_state = fresh.auth_state.clone();
        let subscriptions = fresh
            .client
            .get_devices()?
            .into_iter()
            .map(|device| property_subscription_for(device.did.as_str(), None))
            .collect::<Vec<_>>();
        if subscriptions.is_empty() {
            continue;
        }
        groups.push(PropsSubscriptionGroup {
            account: fresh.auth,
            subscriptions,
        });
    }

    Ok(groups)
}

fn format_device_line_in_group(device: &crate::mico_api::Device) -> String {
    let room = if device.room_name.is_empty() {
        if device.home_name.is_empty() {
            "-".to_string()
        } else {
            device.home_name.clone()
        }
    } else {
        device.room_name.clone()
    };
    format!(
        "{}（did: {}，model: {}，房间: {}）",
        device.name, device.did, device.model, room
    )
}

pub(in crate::cli) fn handle_push(output_mode: OutputMode, args: PushArgs) -> Result<()> {
    let text = args.message.join(" ");
    let text = text.trim();
    let result = push_to_accounts(text, &args.uids)?;
    match output_mode {
        OutputMode::Text => {
            for item in &result.sent {
                println!(
                    "已发送到 {}（{}），通知 ID: {}",
                    item.nickname, item.uid, item.notify_id
                );
            }
        }
        OutputMode::Json => print_json(&format_push_result_output(&result))?,
    }
    if !result.failed.is_empty() {
        bail!(
            "部分账号发送失败: {}",
            result
                .failed
                .iter()
                .map(|item| format!("{} => {}", format_account_label(&item.account), item.error))
                .collect::<Vec<_>>()
                .join("; ")
        );
    }
    Ok(())
}

pub(in crate::cli) fn handle_tui(output_mode: OutputMode, args: TuiArgs) -> Result<()> {
    if output_mode == OutputMode::Json {
        bail!("tui 模式不支持 --json");
    }
    crate::tui::run(args.uid.as_deref())
}

pub fn push_to_accounts(text: &str, uids: &[String]) -> Result<PushSummary> {
    let uids = uids.to_vec();
    push_to_all_accounts_with(
        text,
        load_auth,
        move |auth| {
            let accounts = get_auth_accounts(auth)?;
            if uids.is_empty() {
                return Ok(accounts);
            }
            let matched = accounts
                .into_iter()
                .filter(|a| uids.contains(&a.user.uid))
                .collect::<Vec<_>>();
            if matched.is_empty() {
                bail!("未找到指定账号，请先执行 mit auth list 查看可用账号");
            }
            Ok(matched)
        },
        |auth_state, account, text| {
            let fresh = ensure_fresh_account(auth_state, account)?;
            let notify_id = fresh.client.push_notification(text)?.notify_id;
            Ok((fresh.auth_state, fresh.auth, notify_id))
        },
    )
}

pub fn push_to_all_accounts(text: &str) -> Result<PushSummary> {
    push_to_accounts(text, &[])
}

pub fn push_to_all_accounts_with<Load, Get, Ensure>(
    text: &str,
    load_auth_fn: Load,
    get_auth_accounts_fn: Get,
    ensure_fresh_account_fn: Ensure,
) -> Result<PushSummary>
where
    Load: Fn() -> Result<AuthState>,
    Get: Fn(&AuthState) -> Result<Vec<AuthAccount>>,
    Ensure: Fn(AuthState, AuthAccount, &str) -> Result<(AuthState, AuthAccount, String)>,
{
    let mut auth_state = load_auth_fn()?;
    let accounts = get_auth_accounts_fn(&auth_state)?
        .into_iter()
        .filter(|account| !account.access_token.is_empty() || !account.refresh_token.is_empty())
        .collect::<Vec<_>>();
    if accounts.is_empty() {
        bail!("未授权，请先执行 mit auth login");
    }

    let mut sent = Vec::new();
    let mut failed = Vec::new();
    for account in accounts {
        match ensure_fresh_account_fn(auth_state.clone(), account.clone(), text) {
            Ok((next_state, auth, notify_id)) => {
                auth_state = next_state;
                sent.push(PushSent {
                    uid: auth.user.uid,
                    nickname: auth.user.nickname,
                    notify_id,
                });
            }
            Err(error) => failed.push(PushFailed {
                account,
                error: error.to_string(),
            }),
        }
    }

    if sent.is_empty() && !failed.is_empty() {
        bail!(
            "{}",
            failed
                .iter()
                .map(|item| format!("{} => {}", format_account_label(&item.account), item.error))
                .collect::<Vec<_>>()
                .join("; ")
        );
    }

    Ok(PushSummary { sent, failed })
}

#[derive(Clone)]
pub struct FreshAuth {
    pub auth_state: AuthState,
    pub auth: AuthAccount,
    pub client: MicoClient,
}

pub fn ensure_fresh_account(mut auth_state: AuthState, account: AuthAccount) -> Result<FreshAuth> {
    let mut auth = account.clone();
    let mut client = MicoClient::new(&auth)?;
    auth.device_id = client.device_id.clone();
    auth.state = client.state.clone();
    sync_xiaomi_auth(&mut auth);

    if is_auth_expired(&auth) && !auth.refresh_token.is_empty() {
        let refreshed = client.refresh_token(&auth.refresh_token)?;
        let mut updated = auth.clone();
        updated.device_id = client.device_id.clone();
        updated.state = client.state.clone();
        updated.access_token = refreshed.access_token;
        updated.refresh_token = refreshed.refresh_token;
        updated.expires_ts = refreshed.expires_ts;
        sync_xiaomi_auth(&mut updated);
        auth_state = save_auth(&upsert_auth_account(&auth_state, &updated)?)?;
        auth = find_auth_account_by_uid(&auth_state, &updated.user.uid)
            .ok_or_else(|| anyhow!("刷新后未找到账号 {}", updated.user.uid))?;
    }

    if auth.access_token.is_empty() {
        if auth.user.uid.is_empty() {
            bail!("未授权，请先执行 mit auth login");
        }
        bail!("账号 {} 未授权", format_account_label(&auth));
    }

    Ok(FreshAuth {
        auth_state,
        auth: auth.clone(),
        client: MicoClient::new(&auth)?,
    })
}
fn format_account_label(account: &AuthAccount) -> String {
    format!(
        "{}（{}）",
        if account.user.nickname.is_empty() {
            "-"
        } else {
            account.user.nickname.as_str()
        },
        if account.user.uid.is_empty() {
            "-"
        } else {
            account.user.uid.as_str()
        }
    )
}

pub fn format_props_get_command(did: &str, siid: i64, piid: i64) -> String {
    format!("mit props get {did} {siid} {piid} --json")
}
