use anyhow::{anyhow, bail, Result};
use clap::{ArgAction, Args, Command, CommandFactory, Parser, Subcommand, ValueEnum};
use serde::Serialize;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command as ProcessCommand;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use url::Url;

use crate::mico_api::{is_auth_expired, parse_callback_input, MicoClient};
use crate::mijia_api::{is_mijia_auth_present, mijia_qr_html, MijiaClient};
use crate::mips_cloud::{
    config_from_account, property_subscription_for, start_stdout_subscription, CloudMipsHandle,
    CloudMipsSubscription,
};
use crate::storage::{
    clear_pending_auth, find_auth_account_by_uid, generate_uuid, get_auth_accounts,
    get_pending_auth, load_auth, normalize_account, save_auth, set_pending_auth, sync_xiaomi_auth,
    upsert_auth_account, AuthAccount, AuthState, MijiaAuth, UserProfile, DEFAULT_REGION,
};
#[cfg(not(test))]
use crate::tui::open_url_in_browser;

#[derive(Clone, Debug, Parser)]
#[command(
    name = "mit",
    version = env!("CARGO_PKG_VERSION"),
    disable_version_flag = true
)]
pub struct Cli {
    #[arg(long, global = true, help = "使用 JSON 格式输出")]
    pub json: bool,
    #[command(subcommand)]
    pub command: Option<RootCommand>,
}

#[derive(Clone, Debug, Subcommand)]
pub enum RootCommand {
    #[command(about = "登录与账号管理")]
    Auth(AuthArgs),
    #[command(about = "列出设备")]
    Devices(DevicesArgs),
    #[command(about = "读写 MIoT 属性和 action")]
    Props(PropsArgs),
    #[command(about = "向已登录账号发送通知")]
    Push(PushArgs),
    #[command(about = "启动全屏 TUI 控制台")]
    Tui(TuiArgs),
}

#[derive(Clone, Debug, Args)]
pub struct AuthArgs {
    #[command(subcommand)]
    pub command: Option<AuthCommand>,
}

#[derive(Clone, Debug, Subcommand)]
pub enum AuthCommand {
    #[command(about = "通过浏览器登录小米账号和米家账号")]
    Login(AuthLoginArgs),
    #[command(about = "列出已保存的小米账号")]
    List,
}

#[derive(Clone, Debug, Args, Default)]
pub struct AuthLoginArgs {
    #[arg(long, value_enum, ignore_case = true, help = "登录区域")]
    pub region: Option<AuthRegion>,
    #[command(subcommand)]
    pub flow: Option<AuthLoginFlow>,
}

#[derive(Clone, Debug, Subcommand)]
pub enum AuthLoginFlow {
    #[command(about = "只登录小米 OAuth")]
    Xiaomi(AuthLoginXiaomiArgs),
    #[command(about = "只登录米家")]
    Mijia,
}

#[derive(Clone, Debug, Args, Default)]
pub struct AuthLoginXiaomiArgs {
    #[arg(long, value_enum, ignore_case = true, help = "登录区域")]
    pub region: Option<AuthRegion>,
}

#[derive(Clone, Debug, ValueEnum)]
pub enum AuthRegion {
    Cn,
    De,
    I2,
    Ru,
    Sg,
    Us,
}

impl AuthRegion {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Cn => "cn",
            Self::De => "de",
            Self::I2 => "i2",
            Self::Ru => "ru",
            Self::Sg => "sg",
            Self::Us => "us",
        }
    }
}

#[derive(Clone, Debug, Args)]
pub struct DevicesArgs {
    #[command(subcommand)]
    pub command: Option<DevicesCommand>,
}

#[derive(Clone, Debug, Args)]
pub struct PropsArgs {
    #[command(subcommand)]
    pub command: Option<PropsCommand>,
}

#[derive(Clone, Debug, Subcommand)]
pub enum PropsCommand {
    #[command(about = "读取一个属性")]
    Get(PropsGetArgs),
    #[command(about = "写入一个属性")]
    Set(PropsSetArgs),
    #[command(about = "调用一个 action")]
    Act(PropsActArgs),
    #[command(about = "订阅属性变化")]
    Sub(PropsSubArgs),
}

#[derive(Clone, Debug, Args)]
pub struct PropsGetArgs {
    #[arg(help = "设备 DID")]
    pub did: String,
    #[arg(help = "服务 IID")]
    pub siid: i64,
    #[arg(help = "属性 IID")]
    pub piid: i64,
}

#[derive(Clone, Debug, Args)]
pub struct PropsSetArgs {
    #[arg(help = "设备 DID")]
    pub did: String,
    #[arg(help = "服务 IID")]
    pub siid: i64,
    #[arg(help = "属性 IID")]
    pub piid: i64,
    #[arg(help = "要写入的值；会先按 JSON 解析，失败则按字符串处理")]
    pub value: String,
}

#[derive(Clone, Debug, Args)]
pub struct PropsActArgs {
    #[arg(help = "设备 DID")]
    pub did: String,
    #[arg(help = "服务 IID")]
    pub siid: i64,
    #[arg(help = "action IID")]
    pub aiid: i64,
    #[arg(
        help = "action 参数；每个值会先按 JSON 解析，失败则按字符串处理",
        num_args = 0..
    )]
    pub values: Vec<String>,
}

#[derive(Clone, Debug, Args)]
pub struct PropsSubArgs {
    #[arg(help = "设备 DID；省略则订阅所有设备")]
    pub did: Option<String>,
    #[arg(help = "服务 IID；需与属性 IID 一起提供", requires = "piid")]
    pub siid: Option<i64>,
    #[arg(help = "属性 IID；需与服务 IID 一起提供", requires = "siid")]
    pub piid: Option<i64>,
}

#[derive(Clone, Debug, Subcommand)]
pub enum DevicesCommand {
    #[command(about = "列出所有已登录账号的设备")]
    List,
}

#[derive(Clone, Debug, Args, Default)]
pub struct PushArgs {
    #[arg(long = "uid", help = "目标账号 UID；可重复发送到多个账号")]
    pub uids: Vec<String>,
    #[arg(
        required = true,
        num_args = 1..,
        value_parser = parse_non_empty_text,
        help = "通知文本"
    )]
    pub message: Vec<String>,
}

#[derive(Clone, Debug, Args, Default)]
pub struct TuiArgs {
    #[arg(long = "uid", help = "指定进入 TUI 后默认使用的账号 UID")]
    pub uid: Option<String>,
}

fn parse_non_empty_text(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Err("值不能为空".to_string())
    } else {
        Ok(trimmed.to_string())
    }
}

pub fn build_command() -> Command {
    Cli::command()
        .bin_name("mit")
        .disable_help_flag(true)
        .disable_help_subcommand(true)
        .arg(
            clap::Arg::new("version")
                .short('V')
                .short_alias('v')
                .long("version")
                .action(ArgAction::Version)
                .help("显示版本信息"),
        )
        .arg(
            clap::Arg::new("help")
                .short('h')
                .short_alias('H')
                .long("help")
                .global(true)
                .action(ArgAction::Help)
                .help("显示帮助信息"),
        )
}

pub fn normalize_args_for_clap(args: &[String]) -> Vec<String> {
    let mut normalized = args.to_vec();

    let cmd = build_command();
    if let Some((command_index, command)) = find_subcommand_index(&normalized, 1, &cmd) {
        normalized[command_index] = command.get_name().to_string();

        if let Some((subcommand_index, subcommand)) =
            find_subcommand_index(&normalized, command_index + 1, &command)
        {
            normalized[subcommand_index] = subcommand.get_name().to_string();
        }
    }

    normalized
}

fn find_subcommand_index(
    args: &[String],
    start: usize,
    command: &Command,
) -> Option<(usize, Command)> {
    let mut skip_next = false;

    for (index, arg) in args.iter().enumerate().skip(start) {
        if skip_next {
            skip_next = false;
            continue;
        }

        if arg == "--" {
            break;
        }

        if let Some(takes_value) = option_takes_value(command, arg) {
            skip_next = takes_value;
            continue;
        }

        if arg.starts_with('-') && arg != "-" {
            continue;
        }

        let candidate = arg.to_lowercase();
        return command
            .get_subcommands()
            .find(|subcommand| subcommand.get_name() == candidate)
            .cloned()
            .map(|subcommand| (index, subcommand));
    }

    None
}

fn option_takes_value(command: &Command, arg: &str) -> Option<bool> {
    if let Some(long) = arg.strip_prefix("--") {
        if long.is_empty() {
            return None;
        }

        let (name, has_inline_value) = match long.split_once('=') {
            Some((name, _)) => (name, true),
            None => (long, false),
        };

        return command
            .get_arguments()
            .find(|option| option.get_long() == Some(name))
            .map(|option| arg_expects_value(option) && !has_inline_value);
    }

    if let Some(shorts) = arg.strip_prefix('-') {
        if shorts.is_empty() {
            return None;
        }

        let short = shorts.chars().next()?;
        return command
            .get_arguments()
            .find(|option| option.get_short() == Some(short))
            .map(|option| arg_expects_value(option) && shorts.len() == 1);
    }

    None
}

fn arg_expects_value(arg: &clap::Arg) -> bool {
    matches!(arg.get_action(), ArgAction::Set | ArgAction::Append)
}

pub fn run(args: Cli) -> Result<()> {
    let output_mode = OutputMode::from_json_flag(args.json);
    match args.command {
        Some(RootCommand::Auth(args)) => handle_auth(output_mode, args),
        Some(RootCommand::Devices(args)) => handle_devices(output_mode, args),
        Some(RootCommand::Props(args)) => handle_props(output_mode, args),
        Some(RootCommand::Push(args)) => handle_push(output_mode, args),
        Some(RootCommand::Tui(args)) => handle_tui(output_mode, args),
        None => show_help(output_mode),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PushSent {
    pub uid: String,
    pub nickname: String,
    pub notify_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PushFailed {
    pub account: AuthAccount,
    pub error: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PushSummary {
    pub sent: Vec<PushSent>,
    pub failed: Vec<PushFailed>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutputMode {
    Text,
    Json,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AuthLoginMode {
    Combined,
    XiaomiOnly,
    MijiaOnly,
}

const MIJIA_LOGIN_STATUS_PATH: &str = "/mijia_login_status";

type MijiaLoginSharedResult = Arc<Mutex<Option<Result<(AuthState, AuthAccount), String>>>>;

struct MijiaLoginPageState {
    html: String,
    result: MijiaLoginSharedResult,
}

impl OutputMode {
    fn from_json_flag(json: bool) -> Self {
        if json {
            Self::Json
        } else {
            Self::Text
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthAccountOutput {
    uid: String,
    nickname: String,
    region: String,
    expires_ts: i64,
    xiaomi_status: String,
    mijia_status: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthListOutput {
    #[serde(rename = "type")]
    kind: &'static str,
    account_count: usize,
    accounts: Vec<AuthAccountOutput>,
}

struct AccountDevice {
    uid: String,
    nickname: String,
    device: crate::mico_api::Device,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountDevicesGroupOutput {
    uid: String,
    nickname: String,
    devices: Vec<crate::mico_api::Device>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DevicesListOutput {
    #[serde(rename = "type")]
    kind: &'static str,
    accounts: Vec<AccountDevicesGroupOutput>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PushFailedOutput {
    uid: String,
    nickname: String,
    error: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PushResultOutput {
    #[serde(rename = "type")]
    kind: &'static str,
    sent: Vec<PushSent>,
    failed: Vec<PushFailedOutput>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthUrlPrintedEvent {
    #[serde(rename = "type")]
    kind: &'static str,
    url: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthWaitingEvent {
    #[serde(rename = "type")]
    kind: &'static str,
    region: String,
    redirect_uri: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthLoginSucceededEvent {
    #[serde(rename = "type")]
    kind: &'static str,
    uid: String,
    nickname: String,
}

#[derive(Clone, Debug, Serialize)]
struct HelpOutput {
    #[serde(rename = "type")]
    kind: &'static str,
    command: &'static str,
    text: String,
}

fn show_help(output_mode: OutputMode) -> Result<()> {
    match output_mode {
        OutputMode::Text => {
            println!("mit 可用命令：");
            for subcommand in build_command()
                .get_subcommands()
                .filter(|subcommand| subcommand.get_name() != "help")
            {
                let about = subcommand
                    .get_about()
                    .map(|about| about.to_string())
                    .unwrap_or_default();
                println!("- {}：{}", subcommand.get_name(), about);
            }
            println!();
            println!("运行 `mit --help` 查看完整帮助。");
        }
        OutputMode::Json => {
            let help = Cli::command().render_long_help().to_string();
            print_json(&HelpOutput {
                kind: "help",
                command: "mit",
                text: help,
            })?;
        }
    }
    Ok(())
}

fn show_subcommand_help(output_mode: OutputMode, name: &'static str) -> Result<()> {
    let mut cmd = build_command();
    let subcmd = cmd
        .find_subcommand_mut(name)
        .expect("subcommand must exist");
    match output_mode {
        OutputMode::Text => {
            subcmd.print_long_help()?;
            println!();
        }
        OutputMode::Json => {
            let text = subcmd.render_long_help().to_string();
            print_json(&HelpOutput {
                kind: "help",
                command: name,
                text,
            })?;
        }
    }
    Ok(())
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string(value)?);
    Ok(())
}

fn print_auth_list(output_mode: OutputMode, auth: &AuthState) -> Result<()> {
    let (next_auth, accounts) = format_auth_accounts(auth)?;
    if &next_auth != auth {
        save_auth(&next_auth)?;
    }
    match output_mode {
        OutputMode::Text => {
            println!("账号数：{}", accounts.len());
            for account in &accounts {
                println!(
                    "- {}（{}），区域 {}，小米 {}，米家 {}",
                    if account.nickname.is_empty() {
                        "-"
                    } else {
                        account.nickname.as_str()
                    },
                    if account.uid.is_empty() {
                        "-"
                    } else {
                        account.uid.as_str()
                    },
                    account.region,
                    text_login_status(account.xiaomi_status.as_str()),
                    text_login_status(account.mijia_status.as_str())
                );
            }
        }
        OutputMode::Json => print_json(&AuthListOutput {
            kind: "authList",
            account_count: accounts.len(),
            accounts,
        })?,
    }
    Ok(())
}

fn format_auth_accounts(auth: &AuthState) -> Result<(AuthState, Vec<AuthAccountOutput>)> {
    let mijia_client = MijiaClient::new()?;
    let mut next_auth = auth.clone();
    let mut outputs = Vec::new();
    for mut account in get_auth_accounts(auth)? {
        let xiaomi_status = xiaomi_login_status(&account).to_string();
        let (mijia_status, renewed_mijia) = mijia_login_status(&mijia_client, &account);
        if let Some(renewed_mijia) = renewed_mijia {
            account.mijia = Some(renewed_mijia);
            next_auth = upsert_auth_account(&next_auth, &account)?;
        }
        outputs.push(AuthAccountOutput {
            uid: account.user.uid.clone(),
            nickname: account.user.nickname.clone(),
            region: account.region,
            expires_ts: account.expires_ts,
            xiaomi_status,
            mijia_status: mijia_status.to_string(),
        });
    }
    Ok((next_auth, outputs))
}

fn xiaomi_login_status(account: &AuthAccount) -> &'static str {
    if account.access_token.trim().is_empty() && account.refresh_token.trim().is_empty() {
        "missing"
    } else if is_auth_expired(account) && account.refresh_token.trim().is_empty() {
        "expired"
    } else {
        "loggedIn"
    }
}

fn mijia_login_status(
    client: &MijiaClient,
    account: &AuthAccount,
) -> (&'static str, Option<MijiaAuth>) {
    let Some(mijia) = account.mijia.as_ref() else {
        return ("missing", None);
    };
    if !is_mijia_auth_present(Some(mijia)) {
        return ("missing", None);
    }
    match client.check_new_msg_with_renewal(mijia) {
        Ok(renewed) => ("loggedIn", renewed),
        Err(_) => ("invalid", None),
    }
}

fn text_login_status(status: &str) -> &'static str {
    match status {
        "loggedIn" => "已登录",
        "expired" => "已过期",
        "invalid" => "无效",
        _ => "未登录",
    }
}

fn emit_auth_login_start(
    pending_auth: &AuthAccount,
    auth_url: &str,
    output_mode: OutputMode,
) -> Result<()> {
    match output_mode {
        OutputMode::Text => {
            println!("打开下面的小米授权链接：");
            println!("{auth_url}");
            println!();
            println!("等待浏览器回调...");
            std::io::stdout().flush()?;
            #[cfg(not(test))]
            if let Err(error) = open_url_in_browser(auth_url) {
                eprintln!("无法自动打开浏览器: {error}");
            }
        }
        OutputMode::Json => {
            print_json(&AuthUrlPrintedEvent {
                kind: "authUrlPrinted",
                url: auth_url.to_string(),
            })?;
            print_json(&AuthWaitingEvent {
                kind: "authWaiting",
                region: pending_auth.region.clone(),
                redirect_uri: pending_auth.redirect_uri.clone(),
            })?;
            std::io::stdout().flush()?;
        }
    }
    Ok(())
}

fn emit_mijia_login_start(auth_url: &str, output_mode: OutputMode) -> Result<()> {
    match output_mode {
        OutputMode::Text => {
            println!("打开下面的米家登录链接：");
            println!("{auth_url}");
            println!();
            println!("请在浏览器页面中使用米家 App 扫描二维码...");
            std::io::stdout().flush()?;
            #[cfg(not(test))]
            if let Err(error) = open_url_in_browser(auth_url) {
                eprintln!("无法自动打开浏览器: {error}");
            }
        }
        OutputMode::Json => {
            print_json(&AuthUrlPrintedEvent {
                kind: "authUrlPrinted",
                url: auth_url.to_string(),
            })?;
            std::io::stdout().flush()?;
        }
    }
    Ok(())
}

fn handle_successful_login(output_mode: OutputMode, account: &AuthAccount) -> Result<()> {
    match output_mode {
        OutputMode::Text => {
            println!("授权成功: {} ({})", account.user.nickname, account.user.uid);
        }
        OutputMode::Json => {
            print_json(&AuthLoginSucceededEvent {
                kind: "authLoginSucceeded",
                uid: account.user.uid.clone(),
                nickname: account.user.nickname.clone(),
            })?;
        }
    }
    Ok(())
}

fn format_push_result_output(result: &PushSummary) -> PushResultOutput {
    PushResultOutput {
        kind: "pushResult",
        sent: result.sent.clone(),
        failed: result
            .failed
            .iter()
            .map(|item| PushFailedOutput {
                uid: item.account.user.uid.clone(),
                nickname: item.account.user.nickname.clone(),
                error: item.error.clone(),
            })
            .collect(),
    }
}

fn handle_auth(output_mode: OutputMode, args: AuthArgs) -> Result<()> {
    let Some(command) = args.command else {
        return show_subcommand_help(output_mode, "auth");
    };
    match command {
        AuthCommand::Login(args) => handle_auth_login(output_mode, args),
        AuthCommand::List => {
            print_auth_list(output_mode, &load_auth()?)?;
            Ok(())
        }
    }
}

fn handle_auth_login(output_mode: OutputMode, args: AuthLoginArgs) -> Result<()> {
    let (mode, region) = match args.flow {
        Some(AuthLoginFlow::Xiaomi(xiaomi_args)) => (
            AuthLoginMode::XiaomiOnly,
            xiaomi_args
                .region
                .as_ref()
                .or(args.region.as_ref())
                .map(AuthRegion::as_str)
                .unwrap_or(DEFAULT_REGION)
                .to_string(),
        ),
        Some(AuthLoginFlow::Mijia) => (AuthLoginMode::MijiaOnly, DEFAULT_REGION.to_string()),
        None => (
            AuthLoginMode::Combined,
            args.region
                .as_ref()
                .map(AuthRegion::as_str)
                .unwrap_or(DEFAULT_REGION)
                .to_string(),
        ),
    };

    match mode {
        AuthLoginMode::Combined | AuthLoginMode::XiaomiOnly => {
            run_xiaomi_login(output_mode, region.as_str(), mode)
        }
        AuthLoginMode::MijiaOnly => run_mijia_login(output_mode),
    }
}

fn run_xiaomi_login(
    output_mode: OutputMode,
    default_region: &str,
    mode: AuthLoginMode,
) -> Result<()> {
    let auth_state = load_auth()?;
    let existing_pending = get_pending_auth(&auth_state)?;
    let redirect_uri = resolve_redirect_uri();
    let pending_uuid = existing_pending
        .as_ref()
        .map(|account| account.uuid.clone())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(generate_uuid);
    let mut pending = normalize_account(json!({
        "region": existing_pending
            .as_ref()
            .map(|account| account.region.as_str())
            .unwrap_or(default_region),
        "redirectUri": redirect_uri,
        "uuid": pending_uuid,
        "deviceId": existing_pending
            .as_ref()
            .map(|account| account.device_id.as_str())
            .unwrap_or(""),
        "state": existing_pending
            .as_ref()
            .map(|account| account.state.as_str())
            .unwrap_or(""),
        "accessToken": "",
        "refreshToken": "",
        "expiresTs": 0,
        "user": {
            "uid": "",
            "nickname": "",
            "icon": "",
            "unionId": "",
        }
    }));
    let callback_server = CallbackServer::bind(&pending.redirect_uri)?;
    let client = MicoClient::new(&pending)?;
    pending.device_id = client.device_id.clone();
    pending.state = client.state.clone();
    sync_xiaomi_auth(&mut pending);
    let persisted = save_auth(&set_pending_auth(&auth_state, Some(&pending))?)?;
    let pending_auth = get_pending_auth(&persisted)?.unwrap_or(pending.clone());
    let oauth_auth_url = callback_server.auth_url_with_short_redirect(&client.auth_url(false))?;
    let auth_dialog_url = callback_server.short_redirect_uri();
    emit_auth_login_start(&pending_auth, auth_dialog_url.as_str(), output_mode)?;
    let (_auth_state, account) = callback_server
        .wait_for_callback_and_finish_auth(
            persisted,
            pending_auth,
            oauth_auth_url.as_str(),
            mode == AuthLoginMode::Combined,
        )
        .map_err(|error| {
            anyhow!(
                "{error}\n{}",
                callback_server_port_requirement_tip(callback_server.port)
            )
        })?;
    handle_successful_login(output_mode, &account)?;
    Ok(())
}

fn run_mijia_login(output_mode: OutputMode) -> Result<()> {
    let redirect_uri = resolve_redirect_uri();
    let callback_server = CallbackServer::bind(&redirect_uri)?;
    let auth_dialog_url = callback_server.short_redirect_uri();
    emit_mijia_login_start(auth_dialog_url.as_str(), output_mode)?;
    let (_auth_state, account) =
        callback_server
            .wait_for_mijia_login(load_auth()?)
            .map_err(|error| {
                anyhow!(
                    "{error}\n{}",
                    callback_server_port_requirement_tip(callback_server.port)
                )
            })?;
    handle_successful_login(output_mode, &account)?;
    Ok(())
}

struct CallbackServer {
    listener: TcpListener,
    callback_path: String,
    login_entry_path: String,
    short_redirect_uri: String,
    port: u16,
}

struct ListeningProcess {
    command: String,
    pid: String,
    user: String,
    name: String,
}

impl CallbackServer {
    fn bind(redirect_uri: &str) -> Result<Self> {
        let parsed = Url::parse(redirect_uri)?;
        if parsed.scheme() != "http" {
            bail!("内置回调服务器仅支持 http redirect-uri");
        }
        if parsed.query().is_some() || parsed.fragment().is_some() {
            bail!("redirect-uri 不能包含 query 或 fragment");
        }
        let host = parsed
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("redirect-uri 缺少 host"))?;
        let port = parsed
            .port_or_known_default()
            .ok_or_else(|| anyhow::anyhow!("redirect-uri 缺少端口"))?;
        let callback_path = match parsed.path() {
            "" => "/".to_string(),
            path => path.to_string(),
        };
        let login_entry_path = "/login".to_string();
        let short_redirect_uri = format!("http://{}:{}{}", host, port, login_entry_path);
        let listener = match TcpListener::bind((host, port)) {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
                handle_port_in_use(port)?;
                TcpListener::bind((host, port))?
            }
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            listener,
            callback_path,
            login_entry_path,
            short_redirect_uri,
            port,
        })
    }

    fn auth_url_with_short_redirect(&self, auth_url: &str) -> Result<String> {
        let mut parsed = Url::parse(auth_url)?;
        let mut pairs = parsed
            .query_pairs()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect::<Vec<_>>();
        let mut replaced = false;
        for (key, value) in &mut pairs {
            if key == "redirect_uri" {
                *value = self.short_redirect_uri.clone();
                replaced = true;
            }
        }
        if !replaced {
            pairs.push(("redirect_uri".to_string(), self.short_redirect_uri.clone()));
        }
        let query = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(
                pairs
                    .iter()
                    .map(|(key, value)| (key.as_str(), value.as_str())),
            )
            .finish();
        parsed.set_query(Some(&query));
        Ok(parsed.to_string())
    }

    fn short_redirect_uri(&self) -> String {
        self.short_redirect_uri.clone()
    }

    fn wait_for_callback_and_finish_auth(
        &self,
        auth_state: AuthState,
        pending_auth: AuthAccount,
        oauth_auth_url: &str,
        include_mijia: bool,
    ) -> Result<(AuthState, AuthAccount)> {
        let timeout_secs: u64 = std::env::var("MIT_LOGIN_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(600);
        let deadline = Instant::now() + Duration::from_secs(timeout_secs);

        self.listener.set_nonblocking(true)?;

        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!(
                    "OAuth 登录超时（超过 {timeout_secs} 秒未收到浏览器回调），请重新运行 `mit auth login`"
                );
            }

            let (mut stream, _) = match self.listener.accept() {
                Ok(pair) => pair,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(100).min(remaining));
                    continue;
                }
                Err(e) => return Err(e.into()),
            };
            stream.set_nonblocking(false)?;
            let request = read_http_request(&mut stream)?;
            let outcome = match parse_http_request_line(&request) {
                Ok(request_line) => {
                    if request_line.method != "GET" {
                        write_html_response(
                            &mut stream,
                            "405 Method Not Allowed",
                            "<!doctype html><meta charset=\"utf-8\"><title>mit</title><p>不支持的请求方法</p>",
                        )?;
                        None
                    } else if request_line.path == self.login_entry_path
                        && self.login_entry_path != self.callback_path
                    {
                        let location = if !request_line.query.is_empty()
                            && query_has_auth_callback_params(&request_line.query)
                        {
                            format!("{}?{}", self.callback_path, request_line.query)
                        } else {
                            oauth_auth_url.to_string()
                        };
                        write_redirect_response(&mut stream, "302 Found", &location)?;
                        None
                    } else if request_line.path != self.callback_path {
                        write_html_response(
                            &mut stream,
                            "404 Not Found",
                            "<!doctype html><meta charset=\"utf-8\"><title>mit</title><p>页面不存在</p>",
                        )?;
                        None
                    } else {
                        let callback_input = if request_line.query.is_empty() {
                            request_line.path.clone()
                        } else {
                            format!("?{}", request_line.query)
                        };
                        if include_mijia {
                            let xiaomi =
                                finish_auth_callback(&auth_state, &pending_auth, &callback_input);
                            Some(match xiaomi {
                                Ok((next_auth_state, account)) => finish_mijia_login_response(
                                    &mut stream,
                                    next_auth_state,
                                    Some(account),
                                )
                                .map(|outcome| (outcome, true)),
                                Err(error) => Err(error),
                            })
                        } else {
                            Some(
                                finish_auth_callback(&auth_state, &pending_auth, &callback_input)
                                    .map(|outcome| (outcome, false)),
                            )
                        }
                    }
                }
                Err(error) => {
                    write_html_response(
                        &mut stream,
                        "400 Bad Request",
                        "<!doctype html><meta charset=\"utf-8\"><title>mit</title><p>请求格式错误</p>",
                    )?;
                    return Err(error);
                }
            };

            let Some(outcome) = outcome else {
                continue;
            };

            match outcome {
                Ok((outcome, response_written)) => {
                    if !response_written {
                        write_html_response(
                            &mut stream,
                            "200 OK",
                            "<!doctype html><meta charset=\"utf-8\"><title>mit</title><p>授权成功，可以关闭此页面。</p>",
                        )?;
                    }
                    return Ok(outcome);
                }
                Err(error) => {
                    let _ = write_html_response(
                        &mut stream,
                        "500 Internal Server Error",
                        "<!doctype html><meta charset=\"utf-8\"><title>mit</title><p>授权失败，请查看终端输出。</p>",
                    );
                    return Err(error);
                }
            }
        }
    }

    fn wait_for_mijia_login(&self, auth_state: AuthState) -> Result<(AuthState, AuthAccount)> {
        let timeout_secs: u64 = std::env::var("MIT_LOGIN_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(600);
        let deadline = Instant::now() + Duration::from_secs(timeout_secs);

        self.listener.set_nonblocking(true)?;

        let mut login_page: Option<MijiaLoginPageState> = None;

        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!(
                    "米家登录超时（超过 {timeout_secs} 秒未打开登录页面），请重新运行 `mit auth login mijia`"
                );
            }

            let (mut stream, _) = match self.listener.accept() {
                Ok(pair) => pair,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(100).min(remaining));
                    continue;
                }
                Err(e) => return Err(e.into()),
            };
            stream.set_nonblocking(false)?;
            let request = read_http_request(&mut stream)?;
            let outcome = match parse_http_request_line(&request) {
                Ok(request_line) => {
                    if request_line.method != "GET" {
                        write_html_response(
                            &mut stream,
                            "405 Method Not Allowed",
                            "<!doctype html><meta charset=\"utf-8\"><title>mit</title><p>不支持的请求方法</p>",
                        )?;
                        None
                    } else if request_line.path == self.login_entry_path {
                        if let Some(page) = &login_page {
                            write_html_response(&mut stream, "200 OK", page.html.as_str())?;
                        } else {
                            let page = start_mijia_qr_login(auth_state.clone(), None)?;
                            write_html_response(&mut stream, "200 OK", page.html.as_str())?;
                            login_page = Some(page);
                        }
                        None
                    } else if request_line.path == MIJIA_LOGIN_STATUS_PATH {
                        match &login_page {
                            Some(page) => write_mijia_login_status_response(&mut stream, page)?,
                            None => {
                                write_json_response(
                                    &mut stream,
                                    "200 OK",
                                    &json!({
                                        "status": "pending",
                                        "message": "等待打开米家登录页面"
                                    }),
                                )?;
                                None
                            }
                        }
                    } else {
                        write_html_response(
                            &mut stream,
                            "404 Not Found",
                            "<!doctype html><meta charset=\"utf-8\"><title>mit</title><p>页面不存在</p>",
                        )?;
                        None
                    }
                }
                Err(error) => {
                    write_html_response(
                        &mut stream,
                        "400 Bad Request",
                        "<!doctype html><meta charset=\"utf-8\"><title>mit</title><p>请求格式错误</p>",
                    )?;
                    return Err(error);
                }
            };

            let Some(outcome) = outcome else {
                continue;
            };

            return outcome;
        }
    }
}

fn handle_port_in_use(port: u16) -> Result<()> {
    let processes = match find_listening_processes(port) {
        Ok(processes) if !processes.is_empty() => processes,
        Ok(_) => {
            eprintln!("端口 {port} 已被占用。");
            eprintln!(
                "可运行 `lsof -nP -iTCP:{port} -sTCP:LISTEN` 查看占用进程，然后用 `sudo kill -9 <PID>` 结束它。"
            );
            eprintln!("{}", callback_server_port_requirement_tip(port));
            return Ok(());
        }
        Err(error) => {
            eprintln!("端口 {port} 已被占用，但无法自动查询占用进程：{error}");
            eprintln!(
                "可运行 `lsof -nP -iTCP:{port} -sTCP:LISTEN` 查看占用进程，然后用 `sudo kill -9 <PID>` 结束它。"
            );
            eprintln!("{}", callback_server_port_requirement_tip(port));
            return Ok(());
        }
    };

    eprintln!("端口 {port} 已被占用：");
    for process in &processes {
        eprintln!(
            "- {} (PID {}, 用户 {}): {}",
            process.command, process.pid, process.user, process.name
        );
    }
    let pids = processes
        .iter()
        .map(|process| process.pid.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    eprintln!("将尝试执行：`kill -9 {pids}`");
    eprintln!("手动命令：`sudo kill -9 {pids}`");

    eprintln!("按 Enter 尝试结束占用进程，按 Ctrl+C 取消。");
    eprint!("> ");
    std::io::stderr().flush()?;
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;

    if let Err(error) = kill_listening_processes(&processes) {
        eprintln!("自动结束进程失败：{error}");
        eprintln!("请手动执行 `sudo kill -9 {pids}` 后重试。");
    } else {
        eprintln!("已尝试结束占用进程，正在重试绑定端口 {port}...");
    }
    eprintln!("{}", callback_server_port_requirement_tip(port));

    Ok(())
}

fn callback_server_port_requirement_tip(port: u16) -> String {
    format!("登录回调必须使用 {port} 端口，请确保 127.0.0.1:{port} 可用，并在浏览器回调完成前保持 mit 进程运行。")
}

fn kill_listening_processes(processes: &[ListeningProcess]) -> Result<()> {
    let pids = processes
        .iter()
        .map(|process| process.pid.as_str())
        .collect::<Vec<_>>();
    if pids.is_empty() {
        bail!("未找到可结束的进程");
    }
    let output = ProcessCommand::new("kill")
        .args(["-9"])
        .args(&pids)
        .output()?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        bail!("kill 退出码: {}", output.status);
    }
    bail!("kill 执行失败: {stderr}");
}

fn find_listening_processes(port: u16) -> Result<Vec<ListeningProcess>> {
    let output = ProcessCommand::new("lsof")
        .args(["-nP", "-sTCP:LISTEN"])
        .arg(format!("-iTCP:{port}"))
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("lsof failed for port {port}: {stderr}"));
    }

    let stdout = String::from_utf8(output.stdout)?;
    let mut processes = Vec::new();
    for line in stdout.lines().skip(1) {
        let columns = line.split_whitespace().collect::<Vec<_>>();
        if columns.len() < 9 {
            continue;
        }
        processes.push(ListeningProcess {
            command: columns[0].to_string(),
            pid: columns[1].to_string(),
            user: columns[2].to_string(),
            name: columns[8..].join(" "),
        });
    }
    Ok(processes)
}

struct HttpRequestLine {
    method: String,
    path: String,
    query: String,
}

fn finish_auth_callback(
    auth_state: &AuthState,
    pending_auth: &AuthAccount,
    raw_input: &str,
) -> Result<(AuthState, AuthAccount)> {
    let mut client = MicoClient::new(pending_auth)?;
    let callback = parse_callback_input(raw_input)?;
    let token = client.exchange_code(&callback.code, &callback.state)?;
    let user = client.get_user_info()?;
    let mut account = pending_auth.clone();
    account.device_id = client.device_id.clone();
    account.state = client.state.clone();
    account.access_token = token.access_token;
    account.refresh_token = token.refresh_token;
    account.expires_ts = token.expires_ts;
    account.user = user;
    sync_xiaomi_auth(&mut account);
    let auth_state = resolve_mijia_accounts_for_xiaomi_uid(auth_state, account.user.uid.as_str());
    let auth_state = save_auth(&clear_pending_auth(&upsert_auth_account(
        &auth_state,
        &account,
    )?)?)?;
    let saved_account = get_auth_accounts(&auth_state)?
        .into_iter()
        .find(|item| item.user.uid == account.user.uid)
        .ok_or_else(|| anyhow!("授权成功后未找到账号 {}", account.user.uid))?;
    Ok((auth_state, saved_account))
}

fn resolve_mijia_accounts_for_xiaomi_uid(auth_state: &AuthState, xiaomi_uid: &str) -> AuthState {
    let xiaomi_uid = xiaomi_uid.trim();
    if xiaomi_uid.is_empty() {
        return auth_state.clone();
    }
    let has_mijia_candidate = auth_state.accounts.iter().any(|account| {
        account.xiaomi.is_none()
            && account.mijia.as_ref().is_some_and(|mijia| {
                mijia.user_id.trim().is_empty()
                    || (mijia.user_id.trim() == xiaomi_uid && account.user.uid.trim() != xiaomi_uid)
            })
    });
    if !has_mijia_candidate {
        return auth_state.clone();
    }

    let Ok(client) = MijiaClient::new() else {
        return auth_state.clone();
    };
    let mut next = auth_state.clone();
    for account in &mut next.accounts {
        if account.xiaomi.is_some() {
            continue;
        }
        let Some(mijia) = account.mijia.as_mut() else {
            continue;
        };
        if mijia.user_id.trim() == xiaomi_uid {
            account.user.uid = xiaomi_uid.to_string();
            continue;
        }
        if !mijia.user_id.trim().is_empty() {
            continue;
        }
        if let Ok(profile) = client.resolve_user_profile(mijia) {
            if profile.uid.trim() == xiaomi_uid {
                mijia.user_id = profile.uid.clone();
                account.user.uid = profile.uid.clone();
                fill_empty_user_profile(&mut account.user, &profile);
            }
        }
    }
    next
}

fn finish_mijia_login_response(
    stream: &mut TcpStream,
    auth_state: AuthState,
    existing_account: Option<AuthAccount>,
) -> Result<(AuthState, AuthAccount)> {
    let client = MijiaClient::new()?;
    let existing_mijia = existing_account
        .as_ref()
        .and_then(|account| account.mijia.as_ref());
    let session = client.prepare_qr_login(existing_mijia)?;
    write_redirect_response(stream, "302 Found", session.login_url.as_str())?;
    finish_mijia_login_session(client, session, auth_state, existing_account)
}

fn start_mijia_qr_login(
    auth_state: AuthState,
    existing_account: Option<AuthAccount>,
) -> Result<MijiaLoginPageState> {
    let client = MijiaClient::new()?;
    let existing_mijia = existing_account
        .as_ref()
        .and_then(|account| account.mijia.as_ref());
    let session = client.prepare_qr_login(existing_mijia)?;
    let html = mijia_qr_html(&session, MIJIA_LOGIN_STATUS_PATH);
    let result = Arc::new(Mutex::new(None));
    let worker_result = Arc::clone(&result);
    std::thread::spawn(move || {
        let outcome = finish_mijia_login_session(client, session, auth_state, existing_account)
            .map_err(|error| error.to_string());
        if let Ok(mut result) = worker_result.lock() {
            *result = Some(outcome);
        }
    });
    Ok(MijiaLoginPageState { html, result })
}

fn finish_mijia_login_session(
    client: MijiaClient,
    session: crate::mijia_api::MijiaLoginSession,
    auth_state: AuthState,
    existing_account: Option<AuthAccount>,
) -> Result<(AuthState, AuthAccount)> {
    let mut mijia_auth = client.finish_qr_login(&session)?;
    let mijia_profile = client.resolve_user_profile(&mijia_auth)?;
    if mijia_auth.user_id.trim().is_empty() {
        mijia_auth.user_id = mijia_profile.uid.clone();
    }
    save_mijia_auth(auth_state, existing_account, mijia_auth, mijia_profile)
}

fn write_mijia_login_status_response(
    stream: &mut TcpStream,
    page: &MijiaLoginPageState,
) -> Result<Option<Result<(AuthState, AuthAccount)>>> {
    let snapshot = page
        .result
        .lock()
        .map_err(|_| anyhow!("米家登录状态锁已损坏"))?
        .clone();
    match snapshot {
        Some(Ok(outcome)) => {
            write_json_response(
                stream,
                "200 OK",
                &json!({
                    "status": "succeeded",
                    "message": "授权成功，可以关闭此页面。"
                }),
            )?;
            Ok(Some(Ok(outcome)))
        }
        Some(Err(message)) => {
            write_json_response(
                stream,
                "200 OK",
                &json!({
                    "status": "failed",
                    "message": format!("米家登录失败：{message}。请重新运行 `mit auth login mijia` 后重试。")
                }),
            )?;
            Ok(Some(Err(anyhow!(message))))
        }
        None => {
            write_json_response(
                stream,
                "200 OK",
                &json!({
                    "status": "pending",
                    "message": "等待米家扫码确认"
                }),
            )?;
            Ok(None)
        }
    }
}

fn save_mijia_auth(
    auth_state: AuthState,
    existing_account: Option<AuthAccount>,
    mijia_auth: MijiaAuth,
    mijia_profile: UserProfile,
) -> Result<(AuthState, AuthAccount)> {
    let profile_uid = mijia_profile.uid.trim();
    if profile_uid.is_empty() {
        bail!("米家登录成功但未能解析小米 UID");
    }
    let existing_account = existing_account.filter(|account| {
        let uid = account.user.uid.trim();
        uid.is_empty() || uid == profile_uid
    });
    let mut account = existing_account.unwrap_or_else(|| {
        normalize_account(json!({
            "xiaomi": null,
            "mijia": serde_json::to_value(&mijia_auth).unwrap_or(Value::Null),
            "user": &mijia_profile,
        }))
    });
    if account.user.uid.trim().is_empty() {
        account.user.uid = mijia_profile.uid.clone();
    }
    fill_empty_user_profile(&mut account.user, &mijia_profile);
    account.mijia = Some(mijia_auth.clone());
    let auth_state = save_auth(&clear_pending_auth(&upsert_auth_account(
        &auth_state,
        &account,
    )?)?)?;
    let saved_uid = account.user.uid.clone();
    let saved_account = get_auth_accounts(&auth_state)?
        .into_iter()
        .find(|item| item.user.uid == saved_uid)
        .ok_or_else(|| anyhow!("米家授权成功后未找到账号 {}", saved_uid))?;
    Ok((auth_state, saved_account))
}

fn fill_empty_user_profile(user: &mut UserProfile, profile: &UserProfile) {
    if user.uid.trim().is_empty() {
        user.uid = profile.uid.clone();
    }
    if user.nickname.trim().is_empty() {
        user.nickname = profile.nickname.clone();
    }
    if user.icon.trim().is_empty() {
        user.icon = profile.icon.clone();
    }
    if user.union_id.trim().is_empty() {
        user.union_id = profile.union_id.clone();
    }
}

fn read_http_request(stream: &mut TcpStream) -> Result<String> {
    let mut data = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        data.extend_from_slice(&buffer[..count]);
        if data.windows(4).any(|chunk| chunk == b"\r\n\r\n") || data.len() >= 16 * 1024 {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&data).into_owned())
}

fn parse_http_request_line(request: &str) -> Result<HttpRequestLine> {
    let line = request
        .lines()
        .next()
        .ok_or_else(|| anyhow::anyhow!("无效的 HTTP 请求"))?;
    let mut parts = line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("无效的 HTTP 请求方法"))?;
    let target = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("无效的 HTTP 请求路径"))?;
    let url = Url::parse(&format!("http://127.0.0.1{target}"))?;
    Ok(HttpRequestLine {
        method: method.to_string(),
        path: url.path().to_string(),
        query: url.query().unwrap_or_default().to_string(),
    })
}

fn query_has_auth_callback_params(query: &str) -> bool {
    let mut has_code = false;
    let mut has_state = false;
    for (key, _) in url::form_urlencoded::parse(query.as_bytes()) {
        if key == "code" {
            has_code = true;
        } else if key == "state" {
            has_state = true;
        }
    }
    has_code && has_state
}

fn write_html_response(stream: &mut TcpStream, status: &str, body: &str) -> Result<()> {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    let _ = stream.shutdown(std::net::Shutdown::Both);
    Ok(())
}

fn write_json_response(stream: &mut TcpStream, status: &str, body: &Value) -> Result<()> {
    let body = serde_json::to_string(body)?;
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json; charset=utf-8\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    let _ = stream.shutdown(std::net::Shutdown::Both);
    Ok(())
}

fn write_redirect_response(stream: &mut TcpStream, status: &str, location: &str) -> Result<()> {
    let response = format!(
        "HTTP/1.1 {status}\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    let _ = stream.shutdown(std::net::Shutdown::Both);
    Ok(())
}

fn resolve_redirect_uri() -> String {
    // MIT_REDIRECT_URI: env-var override used by tests to avoid port conflicts
    std::env::var("MIT_REDIRECT_URI")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| crate::storage::DEFAULT_REDIRECT_URI.to_string())
}

fn handle_devices(output_mode: OutputMode, args: DevicesArgs) -> Result<()> {
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
}

struct PropsSubscriptionGroup {
    account: AuthAccount,
    subscriptions: Vec<CloudMipsSubscription>,
}

fn handle_props(output_mode: OutputMode, args: PropsArgs) -> Result<()> {
    let Some(command) = args.command else {
        return show_subcommand_help(output_mode, "props");
    };
    match command {
        PropsCommand::Get(args) => {
            let target = find_target_device(&args.did)?;
            let value =
                target
                    .fresh
                    .client
                    .get_prop(target.device.did.as_str(), args.siid, args.piid)?;
            match output_mode {
                OutputMode::Text => {
                    println!(
                        "{}（{}） {}.{} => {}",
                        target.device.name,
                        target.device.did,
                        args.siid,
                        args.piid,
                        serde_json::to_string(&value)?
                    );
                }
                OutputMode::Json => print_json(&PropGetOutput {
                    kind: "propGet",
                    device_did: target.device.did,
                    device_name: target.device.name,
                    siid: args.siid,
                    piid: args.piid,
                    value,
                })?,
            }
            Ok(())
        }
        PropsCommand::Set(args) => {
            let target = find_target_device(&args.did)?;
            let value = parse_cli_value(&args.value)?;
            let result = target.fresh.client.set_prop(
                target.device.did.as_str(),
                args.siid,
                args.piid,
                value.clone(),
            )?;
            match output_mode {
                OutputMode::Text => {
                    println!(
                        "{}（{}） {}.{} <= {} => {}",
                        target.device.name,
                        target.device.did,
                        args.siid,
                        args.piid,
                        serde_json::to_string(&value)?,
                        serde_json::to_string(&result)?
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
                })?,
            }
            Ok(())
        }
        PropsCommand::Act(args) => {
            let target = find_target_device(&args.did)?;
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
            match output_mode {
                OutputMode::Text => {
                    println!(
                        "{}（{}） {}.{} => {}",
                        target.device.name,
                        target.device.did,
                        args.siid,
                        args.aiid,
                        serde_json::to_string(&result)?
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

fn handle_push(output_mode: OutputMode, args: PushArgs) -> Result<()> {
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

fn handle_tui(output_mode: OutputMode, args: TuiArgs) -> Result<()> {
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
