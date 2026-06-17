use anyhow::{anyhow, Result};
use clap::{ArgAction, Args, Command, CommandFactory, Parser, Subcommand, ValueEnum};
use serde::Serialize;
use std::io::Write;
use std::sync::{mpsc, Arc, Mutex};

use crate::mico_api::is_auth_expired;
use crate::mijia_api::{is_mijia_auth_present, MijiaClient};
use crate::storage::{
    find_auth_account_by_uid, get_auth_accounts, load_auth, save_auth, sync_xiaomi_auth,
    upsert_auth_account, AuthAccount, AuthState, MijiaAuth, DEFAULT_REGION,
};
#[cfg(not(test))]
use crate::tui::open_url_in_browser;

mod commands;
mod login;
pub use commands::{
    ensure_fresh_account, format_props_get_command, push_to_accounts, push_to_all_accounts,
    push_to_all_accounts_with, FreshAuth,
};
use commands::{
    handle_auth_logout, handle_cache, handle_devices, handle_logs, handle_props, handle_push,
    handle_reset, handle_stats, handle_tui, handle_update,
};
use login::{run_mijia_login, run_xiaomi_login};

#[derive(Clone, Debug, Parser)]
#[command(
    name = "mit",
    version = env!("CARGO_PKG_VERSION"),
    disable_version_flag = true
)]
pub struct Cli {
    #[arg(long, global = true, help = "使用 JSON 格式输出")]
    pub json: bool,
    #[arg(long, global = true, help = "输出诊断日志（含 LAN/云通道选择原因）")]
    pub verbose: bool,
    #[arg(
        long = "LAN",
        visible_alias = "lan",
        global = true,
        help = "强制局域网控制，禁止云端回退（设备不在同一内网时直接报错）"
    )]
    pub force_lan: bool,
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
    #[command(about = "查看设备操作记录（米家历史日志）")]
    Logs(LogsArgs),
    #[command(about = "查看设备统计数据（米家统计）")]
    Stats(StatsArgs),
    #[command(about = "清理缓存（保留登录）")]
    Cache(CacheArgs),
    #[command(about = "重置全部数据（删除 ~/.mit）")]
    Reset(ResetArgs),
    #[command(about = "检查并升级到最新版本")]
    Update(UpdateArgs),
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
    #[command(about = "登出账号并删除其本地缓存")]
    Logout(AuthLogoutArgs),
}

#[derive(Clone, Debug, Args, Default)]
pub struct AuthLogoutArgs {
    #[arg(long = "uid", help = "要登出的账号 UID；仅有一个账号时可省略")]
    pub uid: Option<String>,
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

#[derive(Clone, Debug, Args)]
pub struct LogsArgs {
    #[arg(help = "设备 DID")]
    pub did: String,
    #[arg(
        required = true,
        num_args = 1..,
        help = "属性键，形如 <siid>.<piid>（例如 2.1）"
    )]
    pub keys: Vec<String>,
    #[arg(long, default_value_t = 50, help = "每个键最多返回的记录条数")]
    pub limit: u32,
}

#[derive(Clone, Debug, Args)]
pub struct StatsArgs {
    #[arg(help = "设备 DID")]
    pub did: String,
    #[arg(help = "统计键，形如 <siid>.<piid>（例如 3.1）")]
    pub key: String,
    #[arg(long, value_enum, ignore_case = true, default_value_t = StatsPeriod::Week, help = "统计周期")]
    pub period: StatsPeriod,
    #[arg(long, default_value_t = 31, help = "最多返回的数据点条数")]
    pub limit: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum StatsPeriod {
    Week,
    Month,
    Year,
}

impl StatsPeriod {
    /// Mijia statistics data type for this period (mirrors the TUI's StatisticsPeriod).
    fn data_type(self) -> &'static str {
        match self {
            Self::Week | Self::Month => "stat_day_v3",
            Self::Year => "stat_month_v3",
        }
    }
}

#[derive(Clone, Debug, Args)]
pub struct CacheArgs {
    #[command(subcommand)]
    pub command: Option<CacheCommand>,
}

#[derive(Clone, Debug, Subcommand)]
pub enum CacheCommand {
    #[command(about = "删除设备/规格缓存，保留登录信息")]
    Clean,
}

#[derive(Clone, Debug, Args, Default)]
pub struct ResetArgs {
    #[arg(long, help = "确认删除 ~/.mit 下的全部数据（必填，避免误操作）")]
    pub yes: bool,
}

#[derive(Clone, Debug, Args, Default)]
pub struct UpdateArgs {
    #[arg(long, help = "只检查最新版本，不执行升级")]
    pub check: bool,
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
    crate::mico_api::set_verbose_logging(args.verbose);
    crate::mico_api::set_force_lan(args.force_lan);
    let output_mode = OutputMode::from_json_flag(args.json);
    match args.command {
        Some(RootCommand::Auth(args)) => handle_auth(output_mode, args),
        Some(RootCommand::Devices(args)) => handle_devices(output_mode, args),
        Some(RootCommand::Props(args)) => handle_props(output_mode, args),
        Some(RootCommand::Push(args)) => handle_push(output_mode, args),
        Some(RootCommand::Logs(args)) => handle_logs(output_mode, args),
        Some(RootCommand::Stats(args)) => handle_stats(output_mode, args),
        Some(RootCommand::Cache(args)) => handle_cache(output_mode, args),
        Some(RootCommand::Reset(args)) => handle_reset(output_mode, args),
        Some(RootCommand::Update(args)) => handle_update(output_mode, args),
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
        AuthCommand::Logout(args) => handle_auth_logout(output_mode, args),
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
