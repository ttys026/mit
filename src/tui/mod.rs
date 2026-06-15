use anyhow::{anyhow, bail, Result};
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::widgets::ListState;
use ratatui::Terminal;
use ratatui_crossterm::CrosstermBackend;
use serde_json::Value;
use std::collections::{HashSet, VecDeque};
use std::io::stdout;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use time::{format_description::FormatItem, macros::format_description};

use crate::cli::ensure_fresh_account;
use crate::mico_api::{Device, MicoClient};
use crate::property_cache::PropertyCache;
use crate::storage::{
    default_auth, get_auth_accounts, get_home_dir, load_auth, load_settings, save_settings,
    AuthAccount, AuthState, Language, UserSettings,
};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;
mod auth_flow;
mod cloud_mips;
mod datetime;
mod device_cache;
mod device_history;
mod events;
mod footer;
mod logs;
mod mijia_data;
mod pages;
mod prop_dialog;
mod render;
mod search;
mod shared;
mod spec;

pub(in crate::tui) use auth_flow::*;
#[cfg(test)]
pub(in crate::tui) use cloud_mips::*;
pub(in crate::tui) use datetime::{
    add_months_to_date, date_end_timestamp, date_start_timestamp, timestamp_to_local_date,
    today_local_date,
};
pub(in crate::tui) use device_cache::*;
pub(in crate::tui) use device_history::*;
pub(in crate::tui) use events::*;
pub(crate) use footer::extract_auth_url_from_line;
#[cfg(not(test))]
pub(crate) use footer::open_url_in_browser;
pub(in crate::tui) use footer::*;
pub(in crate::tui) use logs::{
    handle_log_scrollbar_mouse, highlight_log_search_matches, log_scroll_offset_for_view,
    log_scrollbar_geometry, log_scrollbar_lines, log_selection_is_stale, log_viewer_text_area,
    log_visual_lines_and_areas, logs_lines_for_display, logs_visible_lines_for_display,
    remember_log_text_width, LOG_SCROLL_PAGE,
};
#[cfg(test)]
pub(in crate::tui) use logs::{log_entry_message, LOG_SCROLLBAR_THUMB};
#[cfg(test)]
pub(in crate::tui) use mijia_data::visible_mijia_raw_request_entries;
pub(in crate::tui) use mijia_data::{
    json_code_is_zero, load_mijia_device_logs_json, load_mijia_device_logs_json_with_query,
    load_mijia_device_statistics_json, load_mijia_device_statistics_json_with_query,
};
pub(in crate::tui) use prop_dialog::*;
pub(in crate::tui) use render::*;
pub(in crate::tui) use search::*;
#[cfg(test)]
pub(in crate::tui) use spec::parse_bool_prop_value;
pub(in crate::tui) use spec::{
    collect_readable_props, extract_actions_from_spec, extract_prop_value,
    format_prop_value_for_dialog, is_error_with_negative_code, parse_prop_input_value,
    read_device_categories_from_template,
};

use self::pages::account as account_page;
use self::pages::device::*;
use shared::*;

const OPERATION_RECORD_TIMESTAMP_FORMAT: &[FormatItem<'static>] =
    format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");
const OPERATION_RECORD_DATE_FORMAT: &[FormatItem<'static>] =
    format_description!("[year]-[month]-[day]");
const OPERATION_RECORD_DATE_PICKER_CALENDAR_WIDTH: u16 = 21;
const OPERATION_RECORD_DATE_PICKER_CALENDAR_HEIGHT: u16 = 9;
const CLOUD_MIPS_RESPONSE_STALE_THRESHOLD: Duration = Duration::from_secs(5 * 60);
const FOOTER_COPY_LOG_PREFIX: &str = "__footer_copied_at=";
const FOOTER_COPY_BADGE_TEXT: &str = " [已复制]";
const CACHE_ACCOUNT_PREFIX: &str = "cache-account:";
const DEVICE_SEARCH_HEIGHT: u16 = 2;
const RAW_LOG_FORMAT: &str = "__mit_raw_device_logs";
const RAW_STATISTICS_FORMAT: &str = "__mit_raw_device_statistics";
const MIJIA_PROP_DATA_TYPE: &str = "prop";
const OPERATION_RECORD_PAGE_LIMIT: u32 = 50;
const OPERATION_RECORD_MENU_MARKER: &str = "__mit_operation_record_menu";
const OPERATION_RECORD_DATE_PICKER_PREFIX: &str = "__mit_operation_record_date_picker:";
const STATISTICS_KEY_MENU_MARKER: &str = "__mit_statistics_key_menu";
const STATISTICS_PERIOD_MENU_MARKER: &str = "__mit_statistics_period_menu";
const STATISTICS_DATE_PICKER_PREFIX: &str = "__mit_statistics_date_picker:";

pub(crate) fn tab_titles(lang: Language) -> [&'static str; 4] {
    match lang {
        Language::Chinese => ["1:账号", "2:设备", "3:日志", "4:设置"],
        Language::English => ["1:Account", "2:Device", "3:Log", "4:Settings"],
    }
}
pub(crate) fn account_list_header_titles(lang: Language) -> [&'static str; 5] {
    match lang {
        Language::Chinese => ["地区", "昵称", "ID", "小米", "米家"],
        Language::English => ["Region", "Nickname", "ID", "Xiaomi", "Mijia"],
    }
}
pub(crate) fn device_list_header_titles(lang: Language) -> [&'static str; 4] {
    match lang {
        Language::Chinese => ["房间", "名称", "类别", "账户"],
        Language::English => ["Room", "Name", "Category", "Account"],
    }
}
const STATUS_BAR_MARGIN_TOP: u16 = 1;
const SETTINGS_ITEM_COUNT: usize = 6;
/// Action indices that get a divider line drawn immediately before them, splitting
/// the list into groups: (leading) | toggles | version/links | destructive resets.
const SETTINGS_SEPARATORS: [usize; 3] = [0, 2, 4];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsAction {
    ToggleLanguage,
    ToggleAutoSubscribeDeviceStatus,
    VersionAndCheckUpdate,
    ViewGithub,
    ClearCacheKeepAuth,
    ResetAll,
}

/// A row in the rendered settings list: a selectable action or a group divider.
/// Only [`SettingsRow::Action`] rows participate in selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::tui) enum SettingsRow {
    Separator,
    Action(usize),
}

/// Ordered render rows for the settings list: each action interleaved with the
/// group dividers from [`SETTINGS_SEPARATORS`] (which includes a leading divider).
pub(in crate::tui) fn settings_rows() -> Vec<SettingsRow> {
    let mut rows = Vec::with_capacity(SETTINGS_ITEM_COUNT + SETTINGS_SEPARATORS.len());
    for action in 0..SETTINGS_ITEM_COUNT {
        if SETTINGS_SEPARATORS.contains(&action) {
            rows.push(SettingsRow::Separator);
        }
        rows.push(SettingsRow::Action(action));
    }
    rows
}

/// Settings action index at a render-row, or `None` for a padding/divider row.
pub(in crate::tui) fn settings_action_for_row(row: usize) -> Option<usize> {
    match settings_rows().get(row) {
        Some(SettingsRow::Action(action)) => Some(*action),
        _ => None,
    }
}

/// Render-row index for a settings action, accounting for padding and divider rows.
pub(in crate::tui) fn settings_row_for_action(action: usize) -> usize {
    settings_rows()
        .iter()
        .position(|row| matches!(row, SettingsRow::Action(found) if *found == action))
        .unwrap_or(action)
}

pub(in crate::tui) fn lang_str(lang: Language, zh: &'static str, en: &'static str) -> &'static str {
    match lang {
        Language::Chinese => zh,
        Language::English => en,
    }
}

fn spec_node_label(node: &Value, lang: Language) -> Option<&str> {
    match lang {
        Language::Chinese => node
            .get("description_trans")
            .and_then(Value::as_str)
            .or_else(|| node.get("description").and_then(Value::as_str)),
        Language::English => node
            .get("description")
            .and_then(Value::as_str)
            .or_else(|| node.get("description_trans").and_then(Value::as_str)),
    }
}

fn readonly_prop_detail_command(dialog: &PropDialog) -> Option<String> {
    let item = dialog.items.get(dialog.selected)?;
    if item.prop.writable {
        return None;
    }
    Some(crate::cli::format_props_get_command(
        dialog.device_did.as_str(),
        item.prop.siid,
        item.prop.piid,
    ))
}

#[derive(Clone, Debug)]
struct LocalTransportRefreshMessage {
    generation: u64,
    error: Option<String>,
}

/// Result of a background GitHub "check update" request: `Ok(tag)` is the latest
/// release tag (e.g. `v1.2.0`), `Err` is a human-readable failure message.
struct UpdateCheckMessage {
    latest: std::result::Result<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum BootState {
    Loading,
    Ready,
}

type BootstrapSyncResult = (
    AuthState,
    Vec<AuthAccount>,
    Vec<Device>,
    Vec<String>,
    Vec<String>,
);

#[derive(Clone, Debug)]
struct BootstrapPending {
    generation: u64,
    uid: String,
    refresh_local_transport_if_missing: bool,
}

#[derive(Clone, Debug)]
enum BootstrapMessage {
    Ready {
        generation: u64,
        uid: String,
        offline_uids: Vec<String>,
        auth_state: AuthState,
        accounts: Vec<AuthAccount>,
        devices: Vec<Device>,
        logs: Vec<String>,
    },
    Failed {
        generation: u64,
        uid: String,
        auth_state: Option<AuthState>,
        accounts: Option<Vec<AuthAccount>>,
        error: String,
    },
}

pub fn run(default_uid: Option<&str>) -> Result<()> {
    let mut app = TuiApp::new(default_uid)?;

    enable_raw_mode()?;
    let mut out = stdout();
    out.execute(EnterAlternateScreen)?;
    out.execute(EnableMouseCapture)?;
    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend)?;

    app.start_bootstrap();
    let result = run_loop(&mut terminal, &mut app);

    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    terminal.backend_mut().execute(DisableMouseCapture)?;
    terminal.show_cursor()?;

    // After a successful in-app upgrade, relaunch into the freshly installed binary.
    if result.is_ok()
        && matches!(
            app.account_action_dialog,
            Some(AccountActionDialog::UpdateFinished { success: true, .. })
        )
    {
        return restart_into_new_binary();
    }
    result
}

/// Replace the current process with a fresh invocation of the (just-upgraded)
/// binary, preserving the original arguments.
fn restart_into_new_binary() -> Result<()> {
    let exe = std::env::current_exe()?;
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    println!("正在重启 mit…");
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // `exec` only returns if it fails to replace the process image.
        Err(std::process::Command::new(exe).args(args).exec().into())
    }
    #[cfg(not(unix))]
    {
        std::process::Command::new(exe).args(args).spawn()?;
        std::process::exit(0);
    }
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut TuiApp,
) -> Result<()> {
    loop {
        let boot_was_loading = matches!(app.boot_state, BootState::Loading);
        app.process_background_messages();
        app.process_prop_dialog_loading();
        app.apply_cached_prop_dialog_updates();
        if boot_was_loading && !matches!(app.boot_state, BootState::Loading) {
            drain_pending_input_events()?;
        }
        if matches!(app.boot_state, BootState::Loading)
            || matches!(
                app.account_action_dialog,
                Some(AccountActionDialog::UpdateRunning { .. })
            )
        {
            // Advance the spinner/indeterminate progress bar each frame.
            app.boot_spinner_index = app.boot_spinner_index.wrapping_add(1);
        }

        terminal.draw(|frame| draw(frame, app))?;

        // A successful upgrade replaced the binary on disk; show the confirmation
        // briefly, then exit the loop so `run` can relaunch into the new version.
        if matches!(
            app.account_action_dialog,
            Some(AccountActionDialog::UpdateFinished { success: true, .. })
        ) {
            thread::sleep(Duration::from_millis(900));
            return Ok(());
        }

        if !event::poll(Duration::from_millis(200))? {
            continue;
        }

        match event::read()? {
            Event::Key(key) => {
                if handle_key(app, key)? {
                    return exit_result_for_boot_state(&app.boot_state);
                }
            }
            Event::Mouse(mouse) => {
                let size = terminal.size()?;
                let area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
                handle_mouse(app, mouse, area)?;
            }
            _ => {}
        }
    }
}

fn exit_result_for_boot_state(boot_state: &BootState) -> Result<()> {
    let _ = boot_state;
    Ok(())
}

fn now_epoch_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

struct TuiApp {
    home_dir: PathBuf,
    auth_state: AuthState,
    accounts: Vec<AuthAccount>,
    account_index: usize,
    devices: Vec<Device>,
    device_index: usize,
    logs: VecDeque<String>,
    active_tab: usize,
    log_scroll_offset: usize,
    input_mode: bool,
    input: String,
    device_search_cursor: usize,
    search_inputs: [String; 3],
    search_cursors: [usize; 3],
    prop_dialog: Option<PropDialog>,
    account_action_dialog: Option<AccountActionDialog>,
    account_list_state: ListState,
    device_list_state: ListState,
    local_transport_fetching: bool,
    local_transport_refresh_generation: u64,
    local_transport_refresh_device_id: Option<String>,
    local_transport_force_refresh_pending: bool,
    local_transport_tx: Sender<LocalTransportRefreshMessage>,
    local_transport_rx: Receiver<LocalTransportRefreshMessage>,
    auth_flow_generation: u64,
    auth_flow_tx: Sender<AuthFlowMessage>,
    auth_flow_rx: Receiver<AuthFlowMessage>,
    offline_account_uids: HashSet<String>,
    boot_state: BootState,
    boot_spinner_index: usize,
    bootstrap_generation: u64,
    bootstrap_pending: Option<BootstrapPending>,
    bootstrap_tx: Sender<BootstrapMessage>,
    bootstrap_rx: Receiver<BootstrapMessage>,
    property_cache: Arc<PropertyCache>,
    language: Language,
    auto_subscribe_device_status: bool,
    settings_selected: usize,
    /// Receiver for an in-flight background "check update" request, if one is running.
    update_check_rx: Option<Receiver<UpdateCheckMessage>>,
    /// Latest "check update" outcome shown next to the Settings item, if any.
    update_check_status: Option<String>,
    /// Receiver for output/result of the in-flight in-app upgrade, if one is running.
    install_rx: Option<Receiver<InstallMessage>>,
}

impl TuiApp {
    fn new(default_uid: Option<&str>) -> Result<Self> {
        let auth_state = load_auth()?;
        let accounts = get_auth_accounts(&auth_state)?
            .into_iter()
            .filter(|a| !a.access_token.is_empty() || !a.refresh_token.is_empty())
            .collect::<Vec<_>>();
        if accounts.is_empty() {
            bail!("未授权，请先执行 mit auth login");
        }
        let account_index = default_uid
            .and_then(|uid| accounts.iter().position(|account| account.user.uid == uid))
            .unwrap_or(0);

        let home_dir = get_home_dir();
        let settings = load_settings();
        let (local_transport_tx, local_transport_rx) = mpsc::channel();
        let (bootstrap_tx, bootstrap_rx) = mpsc::channel();
        let (auth_flow_tx, auth_flow_rx) = mpsc::channel();
        let property_cache = Arc::new(PropertyCache::new());

        Ok(Self {
            home_dir,
            auth_state,
            accounts,
            account_index,
            devices: Vec::new(),
            device_index: 0,
            logs: VecDeque::new(),
            active_tab: 0,
            log_scroll_offset: 0,
            input_mode: false,
            input: String::new(),
            device_search_cursor: 0,
            search_inputs: Default::default(),
            search_cursors: [0; 3],
            prop_dialog: None,
            account_action_dialog: None,
            account_list_state: ListState::default(),
            device_list_state: ListState::default(),
            local_transport_fetching: true,
            local_transport_refresh_generation: 0,
            local_transport_refresh_device_id: Some("device-a".to_string()),
            local_transport_force_refresh_pending: false,
            local_transport_tx,
            local_transport_rx,
            auth_flow_generation: 0,
            auth_flow_tx,
            auth_flow_rx,
            offline_account_uids: HashSet::new(),
            boot_state: BootState::Loading,
            boot_spinner_index: 0,
            bootstrap_generation: 0,
            bootstrap_pending: None,
            bootstrap_tx,
            bootstrap_rx,
            property_cache,
            language: settings.language,
            auto_subscribe_device_status: settings.auto_subscribe_device_status,
            settings_selected: 0,
            update_check_rx: None,
            update_check_status: None,
            install_rx: None,
        })
    }

    fn start_bootstrap(&mut self) {
        self.start_bootstrap_internal(true);
    }

    fn start_background_sync(&mut self) {
        self.start_bootstrap_internal(false);
    }

    fn start_manual_sync(&mut self) {
        self.log("> sync");
        self.start_background_sync();
        self.log("sync started in background");
    }

    fn start_bootstrap_internal(&mut self, show_loading_splash: bool) {
        if show_loading_splash {
            self.boot_state = BootState::Loading;
            self.boot_spinner_index = 0;
            self.devices.clear();
            self.device_index = 0;
            self.prop_dialog = None;
            self.account_action_dialog = None;
            self.local_transport_refresh_generation =
                self.local_transport_refresh_generation.saturating_add(1);
            self.local_transport_fetching = false;
            self.local_transport_refresh_device_id = None;
            self.local_transport_force_refresh_pending = false;
            self.log("Starting TUI bootstrap");
        } else {
            self.log("Starting background sync");
        }
        self.bootstrap_generation = self.bootstrap_generation.saturating_add(1);
        let generation = self.bootstrap_generation;

        let tx = self.bootstrap_tx.clone();
        let account = self.current_account().cloned();
        let all_accounts = self.accounts.clone();
        let requested_uid = account
            .as_ref()
            .map(|a| a.user.uid.clone())
            .unwrap_or_default();
        let selected_uid = if requested_uid.trim().is_empty() {
            all_accounts
                .iter()
                .find(|account| !account.user.uid.trim().is_empty())
                .map(|account| account.user.uid.clone())
                .unwrap_or_default()
        } else {
            requested_uid.clone()
        };
        self.bootstrap_pending = if selected_uid.is_empty() {
            None
        } else {
            Some(BootstrapPending {
                generation,
                uid: selected_uid.clone(),
                refresh_local_transport_if_missing: true,
            })
        };

        if show_loading_splash {
            self.hydrate_devices_from_cache_if_empty();
            if !self.devices.is_empty() {
                self.boot_state = BootState::Ready;
                self.log(format!(
                    "bootstrap: using {} cached devices while syncing in background",
                    self.devices.len()
                ));
            }
        }

        if selected_uid.is_empty() {
            if show_loading_splash {
                self.boot_state = BootState::Ready;
                self.log("bootstrap: 不存在可用账号，请先登录");
            } else {
                self.log("background sync skipped: 不存在可用账号，请先登录");
            }
            return;
        }

        let home_dir = self.home_dir.clone();
        let auth_state = self.auth_state.clone();

        thread::spawn(move || {
            let mut refreshed_auth_state = None;
            let mut refreshed_accounts = None;
            let result = (|| -> Result<BootstrapSyncResult> {
                let selected_uid = selected_uid.clone();
                let mut working_auth_state = auth_state;
                let mut accounts = all_accounts
                    .into_iter()
                    .filter(|a| !a.user.uid.trim().is_empty())
                    .collect::<Vec<_>>();
                if accounts.is_empty() {
                    accounts = get_auth_accounts(&working_auth_state)?
                        .into_iter()
                        .filter(|a| !a.user.uid.trim().is_empty())
                        .collect::<Vec<_>>();
                }
                if accounts.is_empty() {
                    bail!("不存在可用账号");
                }

                let mut ordered_uids = Vec::new();
                if !selected_uid.is_empty()
                    && accounts
                        .iter()
                        .any(|account| account.user.uid == selected_uid)
                {
                    ordered_uids.push(selected_uid.clone());
                }
                for account in &accounts {
                    if !ordered_uids.iter().any(|uid| uid == &account.user.uid) {
                        ordered_uids.push(account.user.uid.clone());
                    }
                }

                let mut merged_devices = Vec::new();
                let mut offline_uids = HashSet::new();
                let mut fetched_remote_any = false;
                for account_uid in ordered_uids {
                    let Some(candidate) = accounts
                        .iter()
                        .find(|account| account.user.uid == account_uid)
                        .cloned()
                    else {
                        continue;
                    };
                    let fresh = match ensure_fresh_account(working_auth_state.clone(), candidate) {
                        Ok(fresh) => fresh,
                        Err(_) => {
                            let cached_devices =
                                load_cached_devices_from_home(&home_dir, &account_uid)
                                    .unwrap_or_default();
                            if !cached_devices.is_empty() {
                                merge_devices(&mut merged_devices, cached_devices);
                            }
                            offline_uids.insert(account_uid);
                            continue;
                        }
                    };
                    working_auth_state = fresh.auth_state;
                    accounts = get_auth_accounts(&working_auth_state)?
                        .into_iter()
                        .filter(|a| !a.access_token.is_empty() || !a.refresh_token.is_empty())
                        .collect::<Vec<_>>();
                    let account_label = accounts
                        .iter()
                        .find(|account| account.user.uid == account_uid)
                        .map(account_page::format_account_label)
                        .unwrap_or(account_uid.clone());
                    match fresh.client.get_devices() {
                        Ok(devices) => {
                            fetched_remote_any = true;
                            let tagged = tag_devices_with_account(
                                devices,
                                account_uid.as_str(),
                                account_label.as_str(),
                            );
                            merge_devices(&mut merged_devices, tagged);
                            offline_uids.remove(account_uid.as_str());
                        }
                        Err(_) => {
                            let cached_devices =
                                load_cached_devices_from_home(&home_dir, &account_uid)
                                    .unwrap_or_default();
                            if !cached_devices.is_empty() {
                                merge_devices(&mut merged_devices, cached_devices);
                            }
                            offline_uids.insert(account_uid);
                        }
                    }
                }

                if merged_devices.is_empty() {
                    let cached_devices =
                        load_cached_devices_from_home(&home_dir, "").unwrap_or_default();
                    if cached_devices.is_empty() {
                        bail!("all account sync attempts failed");
                    }
                    merged_devices = cached_devices;
                }

                refreshed_auth_state = Some(working_auth_state.clone());
                refreshed_accounts = Some(accounts.clone());
                let count = merged_devices.len();
                let mut logs = if !fetched_remote_any {
                    vec![format!("loaded {count} cached devices")]
                } else {
                    vec![format!("loaded {count} devices")]
                };
                let mut device_categories =
                    read_device_categories_from_template(home_dir.as_path(), Language::Chinese)
                        .unwrap_or_default();
                if fetched_remote_any {
                    let missing_spec_models =
                        device_models_missing_local_specs(home_dir.as_path(), &merged_devices);
                    if !missing_spec_models.is_empty() {
                        logs.push(format!(
                            "syncing {} missing device specs",
                            missing_spec_models.len()
                        ));
                        logs.extend(sync_specs_for_models(
                            home_dir.as_path(),
                            missing_spec_models.as_slice(),
                        ));
                        device_categories = read_device_categories_from_template(
                            home_dir.as_path(),
                            Language::Chinese,
                        )
                        .unwrap_or_default();
                    }
                    if let Err(error) = cache_devices_for_accounts(
                        home_dir.as_path(),
                        &merged_devices,
                        &device_categories,
                    ) {
                        logs.push(format!("cache enriched devices failed: {error}"));
                    }
                }

                Ok((
                    working_auth_state,
                    accounts,
                    merged_devices,
                    logs,
                    offline_uids.into_iter().collect::<Vec<_>>(),
                ))
            })();

            let _ = match result {
                Ok((_auth_state, _accounts, devices, logs, offline_uids)) => {
                    let auth_state = refreshed_auth_state.unwrap_or(_auth_state);
                    let accounts = refreshed_accounts.unwrap_or(_accounts);
                    tx.send(BootstrapMessage::Ready {
                        generation,
                        uid: selected_uid.clone(),
                        offline_uids,
                        auth_state,
                        accounts,
                        devices,
                        logs,
                    })
                }
                Err(error) => tx.send(BootstrapMessage::Failed {
                    generation,
                    uid: selected_uid.clone(),
                    auth_state: refreshed_auth_state,
                    accounts: refreshed_accounts,
                    error: error.to_string(),
                }),
            };
        });
    }

    fn current_account(&self) -> Option<&AuthAccount> {
        self.accounts.get(self.account_index)
    }

    fn should_apply_bootstrap(&self, generation: u64, uid: &str) -> bool {
        self.bootstrap_pending
            .as_ref()
            .is_some_and(|pending| pending.generation == generation && pending.uid == uid)
    }

    fn process_background_messages(&mut self) {
        self.process_cloud_mips_messages();
        let mut update_check_results = Vec::new();
        if let Some(rx) = &self.update_check_rx {
            while let Ok(message) = rx.try_recv() {
                update_check_results.push(message.latest);
            }
        }
        if !update_check_results.is_empty() {
            // The worker sends exactly one message then exits; stop polling once received.
            self.update_check_rx = None;
        }
        for latest in update_check_results {
            self.apply_update_check_result(latest);
        }

        let mut install_messages = Vec::new();
        if let Some(rx) = &self.install_rx {
            while let Ok(message) = rx.try_recv() {
                install_messages.push(message);
            }
        }
        for message in install_messages {
            self.apply_install_message(message);
        }
        while let Ok(message) = self.bootstrap_rx.try_recv() {
            match message {
                BootstrapMessage::Ready {
                    generation,
                    uid,
                    offline_uids,
                    auth_state,
                    accounts,
                    devices,
                    logs,
                } => {
                    if !self.should_apply_bootstrap(generation, uid.as_str()) {
                        continue;
                    }
                    let refresh_local_transport_if_missing =
                        self.bootstrap_pending.as_ref().is_some_and(|pending| {
                            pending.generation == generation
                                && pending.uid == uid
                                && pending.refresh_local_transport_if_missing
                        });
                    self.bootstrap_pending = None;
                    self.auth_state = auth_state;
                    self.accounts = accounts;
                    self.account_index = self
                        .accounts
                        .iter()
                        .position(|account| account.user.uid == uid)
                        .unwrap_or(0);
                    if self.account_index >= self.accounts.len() {
                        self.account_index = 0;
                    }
                    let selected_did = self.selected_device_did().map(ToString::to_string);
                    let previous_index = self.device_index;
                    let mut merged_devices = devices;
                    merge_devices(
                        &mut merged_devices,
                        load_cached_devices_from_home(&self.home_dir, "").unwrap_or_default(),
                    );
                    sort_devices_by_room(&mut merged_devices);
                    self.devices = merged_devices;
                    if self.devices.is_empty() {
                        self.device_index = 0;
                    } else if let Some(selected_did) = selected_did {
                        self.device_index = self
                            .devices
                            .iter()
                            .position(|device| device.did == selected_did)
                            .unwrap_or_else(|| previous_index.min(self.devices.len() - 1));
                    } else {
                        self.device_index = previous_index.min(self.devices.len() - 1);
                    }
                    for account in &self.accounts {
                        self.offline_account_uids.remove(account.user.uid.as_str());
                    }
                    for offline_uid in offline_uids {
                        self.offline_account_uids.insert(offline_uid);
                    }
                    self.boot_state = BootState::Ready;
                    for log in logs {
                        self.log(log);
                    }
                    self.refresh_cloud_mips_listeners();
                    let should_rewarm_local_transport = refresh_local_transport_if_missing
                        && self
                            .current_uid()
                            .is_some_and(|uid| !self.offline_account_uids.contains(uid))
                        && self.current_account_local_credentials_snapshot_missing();
                    if should_rewarm_local_transport {
                        self.request_local_transport_refresh(true);
                    }
                }
                BootstrapMessage::Failed {
                    generation,
                    uid,
                    auth_state,
                    accounts,
                    error,
                } => {
                    if !self.should_apply_bootstrap(generation, uid.as_str()) {
                        continue;
                    }
                    self.bootstrap_pending = None;
                    if !uid.is_empty() {
                        self.offline_account_uids.insert(uid.clone());
                    }
                    if let (Some(auth_state), Some(accounts)) = (auth_state, accounts) {
                        self.auth_state = auth_state;
                        self.accounts = accounts;
                        self.account_index = self
                            .accounts
                            .iter()
                            .position(|account| account.user.uid == uid)
                            .unwrap_or(0);
                        if self.account_index >= self.accounts.len() {
                            self.account_index = 0;
                        }
                    }
                    self.boot_state = BootState::Ready;
                    self.log(format!("bootstrap failed: {error}"));
                }
            }
        }

        while let Ok(message) = self.local_transport_rx.try_recv() {
            if message.generation != self.local_transport_refresh_generation {
                continue;
            }
            self.local_transport_fetching = false;
            if let Some(err) = message.error.as_deref() {
                self.log(format!(
                    "Local transport refresh failed in background: {err}"
                ));
                self.local_transport_refresh_device_id = None;
            }
            if self.local_transport_force_refresh_pending {
                self.local_transport_force_refresh_pending = false;
                self.request_local_transport_refresh(true);
            }
        }

        while let Ok(message) = self.auth_flow_rx.try_recv() {
            match message {
                AuthFlowMessage::Completed {
                    generation,
                    success,
                    detail,
                } => {
                    if generation != self.auth_flow_generation {
                        continue;
                    }
                    if success {
                        if matches!(
                            self.account_action_dialog,
                            Some(AccountActionDialog::Reauth { .. })
                        ) {
                            self.account_action_dialog = None;
                        }
                        self.log(format!("auth flow completed: {detail}"));
                        match load_auth().and_then(|auth_state| {
                            let accounts = get_auth_accounts(&auth_state)?
                                .into_iter()
                                .filter(|a| {
                                    !a.access_token.is_empty() || !a.refresh_token.is_empty()
                                })
                                .collect::<Vec<_>>();
                            Ok((auth_state, accounts))
                        }) {
                            Ok((auth_state, accounts)) => {
                                self.auth_state = auth_state;
                                self.accounts = accounts;
                                if self.account_index >= self.accounts.len() {
                                    self.account_index = 0;
                                }
                                self.refresh_cloud_mips_listeners();
                            }
                            Err(error) => {
                                self.log(format!("auth flow reload failed: {error}"));
                            }
                        }
                    } else {
                        if let Some(AccountActionDialog::Reauth { status, .. }) =
                            &mut self.account_action_dialog
                        {
                            let mut message = format!("Failed to complete auth flow: {detail}");
                            let tip = "登录回调必须使用 8000 端口，请确保 127.0.0.1:8000 可用，并在浏览器回调完成前保持 mit 进程运行。";
                            if !message.contains(tip) {
                                message = format!("{message}\n{tip}");
                            }
                            *status = message;
                        }
                        self.log(format!("auth flow failed: {detail}"));
                    }
                }
            }
        }
    }

    fn request_local_transport_refresh(&mut self, force: bool) {
        let Some(account) = self.current_account().cloned() else {
            self.local_transport_fetching = false;
            self.local_transport_refresh_device_id = None;
            self.local_transport_force_refresh_pending = false;
            return;
        };
        if self.local_transport_fetching {
            if force
                || self
                    .local_transport_refresh_device_id
                    .as_deref()
                    .is_some_and(|device_id| device_id != account.device_id.as_str())
            {
                self.local_transport_force_refresh_pending = true;
            }
            return;
        }
        if !force
            && self
                .local_transport_refresh_device_id
                .as_deref()
                .is_some_and(|device_id| device_id == account.device_id)
        {
            return;
        }

        self.local_transport_force_refresh_pending = false;
        self.local_transport_fetching = true;
        self.local_transport_refresh_generation =
            self.local_transport_refresh_generation.saturating_add(1);
        self.local_transport_refresh_device_id = Some(account.device_id.clone());
        let generation = self.local_transport_refresh_generation;
        let tx = self.local_transport_tx.clone();
        thread::spawn(move || {
            let error = (|| -> Result<()> {
                let client = MicoClient::new(&account)?;
                client.get_local_device_credentials()?;
                Ok(())
            })()
            .err()
            .map(|e| e.to_string());

            let _ = tx.send(LocalTransportRefreshMessage { generation, error });
        });
    }

    fn current_uid(&self) -> Option<&str> {
        self.accounts
            .get(self.account_index)
            .map(|a| a.user.uid.as_str())
    }

    fn current_account_local_credentials_snapshot_missing(&self) -> bool {
        let Some(uid) = self.current_uid() else {
            return false;
        };
        let Some(path) = local_credentials_snapshot_path(self.home_dir.as_path(), uid) else {
            return false;
        };
        !path.exists()
    }

    fn selected_device_did(&self) -> Option<&str> {
        if self.active_tab == 1 && !self.selected_device_is_visible() {
            return None;
        }
        self.devices.get(self.device_index).map(|d| d.did.as_str())
    }

    fn device_account_label(&self, device: &Device) -> String {
        if device.home_id.starts_with(CACHE_ACCOUNT_PREFIX) && !device.home_name.trim().is_empty() {
            let label = device.home_name.trim();
            if let Some((nickname, _)) = label.split_once('(').or_else(|| label.split_once('（')) {
                let nickname = nickname.trim();
                if !nickname.is_empty() {
                    return nickname.to_string();
                }
            }
            return label.to_string();
        }
        "-".to_string()
    }

    fn hydrate_devices_from_cache_if_empty(&mut self) {
        if !self.devices.is_empty() {
            return;
        }
        match load_cached_devices_from_home(&self.home_dir, "") {
            Ok(mut cached) if !cached.is_empty() => {
                sort_devices_by_room(&mut cached);
                self.devices = cached;
                self.device_index = 0;
                self.log(format!(
                    "loaded {} devices from local cache",
                    self.devices.len()
                ));
            }
            Ok(_) => {}
            Err(error) => self.log(format!("load cached devices failed: {error}")),
        }
    }

    fn settings_selected_index(&self) -> usize {
        if SETTINGS_ITEM_COUNT == 0 {
            return 0;
        }
        self.settings_selected
            .min(SETTINGS_ITEM_COUNT.saturating_sub(1))
    }

    fn select_settings_item(&mut self, index: usize) {
        self.settings_selected = index.min(SETTINGS_ITEM_COUNT.saturating_sub(1));
    }

    fn selected_settings_action(&self) -> SettingsAction {
        match self.settings_selected_index() {
            0 => SettingsAction::ToggleLanguage,
            1 => SettingsAction::ToggleAutoSubscribeDeviceStatus,
            2 => SettingsAction::VersionAndCheckUpdate,
            3 => SettingsAction::ViewGithub,
            4 => SettingsAction::ClearCacheKeepAuth,
            _ => SettingsAction::ResetAll,
        }
    }

    fn save_user_settings(&self) -> Result<()> {
        save_settings(&UserSettings {
            language: self.language,
            auto_subscribe_device_status: self.auto_subscribe_device_status,
        })
    }

    fn execute_settings_action(&mut self, action: SettingsAction) -> Result<()> {
        match action {
            SettingsAction::ClearCacheKeepAuth => self.purge_devices_cache_keep_auth(),
            SettingsAction::ResetAll => self.reset_all_settings(),
            SettingsAction::VersionAndCheckUpdate => {
                self.start_update_check();
                Ok(())
            }
            SettingsAction::ViewGithub => {
                self.open_github_page(&crate::actions::github_home_url());
                Ok(())
            }
            SettingsAction::ToggleLanguage => {
                self.language = match self.language {
                    Language::Chinese => Language::English,
                    Language::English => Language::Chinese,
                };
                let _ = self.save_user_settings();
                Ok(())
            }
            SettingsAction::ToggleAutoSubscribeDeviceStatus => {
                self.auto_subscribe_device_status = !self.auto_subscribe_device_status;
                let _ = self.save_user_settings();
                self.refresh_cloud_mips_listeners();
                Ok(())
            }
        }
    }

    fn confirm_settings_action(&mut self) -> Result<bool> {
        let Some(AccountActionDialog::SettingsConfirm { action }) =
            self.account_action_dialog.clone()
        else {
            return Ok(false);
        };
        self.account_action_dialog = None;
        self.execute_settings_action(action)?;
        Ok(matches!(action, SettingsAction::ResetAll))
    }

    fn purge_devices_cache_keep_auth(&mut self) -> Result<()> {
        let removed_entries = crate::actions::clear_device_cache(&self.home_dir.join(".mit"))?;

        self.devices.clear();
        self.device_index = 0;
        self.prop_dialog = None;
        self.property_cache.clear_all();
        self.start_bootstrap();
        self.log(format!(
            "已清理缓存（保留 auth.json，删除 {removed_entries} 项），正在重启 TUI…"
        ));
        Ok(())
    }

    fn reset_all_settings(&mut self) -> Result<()> {
        crate::actions::reset_profile(&self.home_dir.join(".mit"))?;
        self.auth_state = default_auth();
        self.accounts.clear();
        self.account_index = 0;
        self.devices.clear();
        self.device_index = 0;
        self.prop_dialog = None;
        self.property_cache.clear_all();
        self.offline_account_uids.clear();
        self.bootstrap_pending = None;
        self.active_tab = 0;
        self.log("已重置全部设置（~/.mit 已删除）".to_string());
        Ok(())
    }

    /// Open a GitHub page in the browser.
    #[cfg(not(test))]
    fn open_github_page(&mut self, url: &str) {
        match open_url_in_browser(url) {
            Ok(()) => self.log(format!("已在浏览器打开 GitHub 页面：{url}")),
            Err(error) => self.log(format!("打开 GitHub 页面失败：{error}")),
        }
    }

    /// Test stub: never launch a real browser, just record the intent.
    #[cfg(test)]
    fn open_github_page(&mut self, url: &str) {
        self.log(format!("已在浏览器打开 GitHub 页面：{url}"));
    }

    /// Kick off a background GitHub release lookup; the result is applied in
    /// [`Self::process_background_messages`] so the event loop never blocks on it.
    fn start_update_check(&mut self) {
        self.update_check_status =
            Some(lang_str(self.language, "检查中…", "Checking…").to_string());
        self.log(lang_str(self.language, "正在检查更新…", "Checking for updates…").to_string());
        let (tx, rx) = mpsc::channel();
        self.update_check_rx = Some(rx);
        thread::spawn(move || {
            let latest = crate::actions::latest_release_tag().map_err(|error| error.to_string());
            let _ = tx.send(UpdateCheckMessage { latest });
        });
    }

    fn apply_update_check_result(&mut self, latest: std::result::Result<String, String>) {
        match latest {
            Ok(latest) => {
                let current = env!("CARGO_PKG_VERSION");
                // Tags are published as `vX.Y.Z`; compare against the bare cargo version.
                // The version is already shown in the combined label, so the status
                // here stays terse and only spells out the new version when newer.
                if latest.trim_start_matches('v') == current {
                    self.update_check_status =
                        Some(lang_str(self.language, "已是最新", "up to date").to_string());
                    self.log(format!("检查更新：已是最新版本（{latest}）"));
                } else {
                    self.update_check_status = Some(format!(
                        "{} {latest}",
                        lang_str(self.language, "发现新版本", "update available")
                    ));
                    self.log(format!("检查更新：发现新版本 {latest}（当前 v{current}）"));
                    // Offer the in-app upgrade unless another dialog is already open.
                    if self.account_action_dialog.is_none() {
                        self.account_action_dialog =
                            Some(AccountActionDialog::UpdateAvailable { latest });
                    }
                }
            }
            Err(error) => {
                // Call out rate limiting specifically; the full reason goes to the log.
                let rate_limited =
                    error.contains("限流") || error.to_lowercase().contains("rate limit");
                let label = if rate_limited {
                    lang_str(self.language, "检查失败（限流）", "failed (rate limited)")
                } else {
                    lang_str(self.language, "检查失败", "check failed")
                };
                self.update_check_status = Some(label.to_string());
                self.log(format!("检查更新失败：{error}"));
            }
        }
    }

    /// Confirm the upgrade: spawn the install script (in its own process group so
    /// it can be cancelled) and switch the dialog to its running state.
    fn start_update_install(&mut self, latest: String) {
        self.log(format!("开始升级到 {latest}…"));
        let (tx, rx) = mpsc::channel();
        let pid = spawn_install(tx);
        self.install_rx = Some(rx);
        self.account_action_dialog = Some(AccountActionDialog::UpdateRunning {
            latest,
            lines: Vec::new(),
            pid,
        });
    }

    /// Cancel an in-flight upgrade: kill the install process group, discard its
    /// pending result, and close the dialog.
    fn cancel_update_install(&mut self, pid: Option<u32>) {
        if let Some(pid) = pid {
            kill_process_group(pid);
        }
        // Drop the receiver so the worker's final message is ignored, not re-shown.
        self.install_rx = None;
        self.account_action_dialog = None;
        self.log("已取消升级".to_string());
    }

    fn apply_install_message(&mut self, message: InstallMessage) {
        match message {
            InstallMessage::Line(line) => {
                if let Some(AccountActionDialog::UpdateRunning { lines, .. }) =
                    &mut self.account_action_dialog
                {
                    lines.push(line);
                    // Keep only the most recent lines so the dialog stays compact.
                    const MAX_LINES: usize = 6;
                    if lines.len() > MAX_LINES {
                        lines.drain(0..lines.len() - MAX_LINES);
                    }
                }
            }
            InstallMessage::Done { success, message } => {
                self.install_rx = None;
                self.log(format!("升级结束：{message}"));
                self.account_action_dialog =
                    Some(AccountActionDialog::UpdateFinished { success, message });
            }
        }
    }

    fn execute_selected_settings_action(&mut self) -> Result<()> {
        let action = self.selected_settings_action();
        match action {
            SettingsAction::ToggleLanguage
            | SettingsAction::ToggleAutoSubscribeDeviceStatus
            | SettingsAction::VersionAndCheckUpdate
            | SettingsAction::ViewGithub => self.execute_settings_action(action),
            SettingsAction::ClearCacheKeepAuth | SettingsAction::ResetAll => {
                self.account_action_dialog = Some(AccountActionDialog::SettingsConfirm { action });
                Ok(())
            }
        }
    }

    fn set_active_tab(&mut self, index: usize) {
        let target = index.min(tab_titles(self.language).len().saturating_sub(1));
        if target != self.active_tab {
            self.blur_search();
            self.active_tab = target;
            self.load_active_search_state();
            if target == 2 {
                self.log_scroll_offset = 0;
            }
        } else {
            self.active_tab = target;
        }
    }

    fn next_tab(&mut self) {
        let next = (self.active_tab + 1) % tab_titles(self.language).len();
        self.set_active_tab(next);
    }

    fn prev_tab(&mut self) {
        let previous = if self.active_tab == 0 {
            tab_titles(self.language).len().saturating_sub(1)
        } else {
            self.active_tab - 1
        };
        self.set_active_tab(previous);
    }

    fn next_item(&mut self) {
        match self.active_tab {
            0 => {
                let indices = self.filtered_account_indices();
                if !indices.is_empty() {
                    // Account switches invalidate pending bootstrap results for the previous account.
                    self.bootstrap_pending = None;
                    let position = indices
                        .iter()
                        .position(|index| *index == self.account_index)
                        .unwrap_or(0);
                    self.account_index = indices[(position + 1) % indices.len()];
                    self.request_local_transport_refresh(false);
                }
            }
            1 => {
                let indices = self.filtered_device_indices();
                if !indices.is_empty() {
                    let position = indices
                        .iter()
                        .position(|index| *index == self.device_index)
                        .unwrap_or(0);
                    self.device_index = indices[(position + 1) % indices.len()];
                }
            }
            2 => self.scroll_logs_down(1),
            3 => self
                .select_settings_item((self.settings_selected_index() + 1) % SETTINGS_ITEM_COUNT),
            _ => {}
        }
    }

    fn next_bool_item(&mut self) {
        if let Some(dialog) = &mut self.prop_dialog {
            let indices = prop_dialog_indices_for_tab(dialog, dialog.active_tab);
            if indices.is_empty() {
                return;
            }
            let current = indices
                .iter()
                .position(|index| *index == dialog.selected)
                .unwrap_or(0);
            let next = (current + 1) % indices.len();
            dialog.selected = indices[next];
            match dialog.active_tab {
                PropDialogTab::Writable => dialog.writable_selected = dialog.selected,
                PropDialogTab::ReadOnly => dialog.readonly_selected = dialog.selected,
                PropDialogTab::Actions => dialog.actions_selected = dialog.selected,
                PropDialogTab::Logs | PropDialogTab::Statistics => {}
            }
        }
    }

    fn next_account_action_item(&mut self) {
        if let Some(AccountActionDialog::Menu { selected }) = &mut self.account_action_dialog {
            *selected = (*selected + 1) % 4;
        }
    }

    fn prev_bool_item(&mut self) {
        if let Some(dialog) = &mut self.prop_dialog {
            let indices = prop_dialog_indices_for_tab(dialog, dialog.active_tab);
            if indices.is_empty() {
                return;
            }
            let current = indices
                .iter()
                .position(|index| *index == dialog.selected)
                .unwrap_or(0);
            let prev = if current == 0 {
                indices.len() - 1
            } else {
                current - 1
            };
            dialog.selected = indices[prev];
            match dialog.active_tab {
                PropDialogTab::Writable => dialog.writable_selected = dialog.selected,
                PropDialogTab::ReadOnly => dialog.readonly_selected = dialog.selected,
                PropDialogTab::Actions => dialog.actions_selected = dialog.selected,
                PropDialogTab::Logs | PropDialogTab::Statistics => {}
            }
        }
    }

    fn prev_account_action_item(&mut self) {
        if let Some(AccountActionDialog::Menu { selected }) = &mut self.account_action_dialog {
            *selected = if *selected == 0 { 3 } else { *selected - 1 };
        }
    }

    fn dismiss_reauth_dialog(&mut self) {
        if let Err(error) = cancel_active_auth_process(self.auth_flow_generation) {
            self.log(format!("failed to cancel auth flow: {error}"));
        }
        self.account_action_dialog = None;
    }

    fn prev_item(&mut self) {
        match self.active_tab {
            0 => {
                let indices = self.filtered_account_indices();
                if !indices.is_empty() {
                    // Account switches invalidate pending bootstrap results for the previous account.
                    self.bootstrap_pending = None;
                    let position = indices
                        .iter()
                        .position(|index| *index == self.account_index)
                        .unwrap_or(0);
                    self.account_index = if position == 0 {
                        indices[indices.len() - 1]
                    } else {
                        indices[position - 1]
                    };
                    self.request_local_transport_refresh(false);
                }
            }
            1 => {
                let indices = self.filtered_device_indices();
                if !indices.is_empty() {
                    let position = indices
                        .iter()
                        .position(|index| *index == self.device_index)
                        .unwrap_or(0);
                    self.device_index = if position == 0 {
                        indices[indices.len() - 1]
                    } else {
                        indices[position - 1]
                    };
                }
            }
            2 => self.scroll_logs_up(1),
            3 => {
                self.select_settings_item(if self.settings_selected_index() == 0 {
                    SETTINGS_ITEM_COUNT - 1
                } else {
                    self.settings_selected_index() - 1
                });
            }
            _ => {}
        }
    }

    fn refresh_client_for_account_uid(
        &mut self,
        uid: &str,
        refresh_local_transport: bool,
    ) -> Result<crate::mico_api::MicoClient> {
        // Any foreground operation that refreshes auth/client is newer than an in-flight bootstrap.
        self.bootstrap_pending = None;
        let account = self
            .accounts
            .iter()
            .find(|account| account.user.uid == uid)
            .cloned()
            .ok_or_else(|| anyhow!("设备所属账号未授权: {uid}"))?;
        let fresh = ensure_fresh_account(self.auth_state.clone(), account)?;
        self.auth_state = fresh.auth_state;
        self.accounts = get_auth_accounts(&self.auth_state)?
            .into_iter()
            .filter(|a| !a.access_token.is_empty() || !a.refresh_token.is_empty())
            .collect::<Vec<_>>();
        if let Some(index) = self
            .accounts
            .iter()
            .position(|account| account.user.uid == uid)
        {
            self.account_index = index;
        } else if self.account_index >= self.accounts.len() {
            self.account_index = 0;
        }
        self.offline_account_uids.remove(uid);
        if refresh_local_transport {
            self.request_local_transport_refresh(false);
        }
        Ok(fresh.client)
    }
}

#[derive(Clone, Debug, PartialEq)]
struct PropItem {
    siid: i64,
    piid: i64,
    name: String,
    format: String,
    writable: bool,
    value_options: Vec<PropValueOption>,
}

#[derive(Clone, Debug, PartialEq)]
struct PropValueOption {
    value: Value,
    label: String,
}

#[derive(Clone, Debug)]
struct ActionItem {
    siid: i64,
    aiid: i64,
    name: String,
    input_piids: Vec<i64>,
    input_labels: Vec<String>,
    input_props: Vec<PropItem>,
}

#[derive(Clone, Debug)]
struct ToggleItem {
    prop: PropItem,
    value: Value,
}

#[derive(Debug)]
enum PropDialogRefreshMessage {
    Props(Vec<(usize, Value)>),
    Raw(Vec<(usize, Value)>),
    Finished,
    Error(String),
}

fn json_i64(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn json_text(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => Some(text.trim().to_string()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn json_compact_text(value: Option<&Value>) -> Option<String> {
    serde_json::to_string(value?).ok()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PropDialogTab {
    Actions,
    Writable,
    ReadOnly,
    Logs,
    Statistics,
}

#[derive(Debug)]
struct PropDialog {
    device_did: String,
    device_name: String,
    account_uid: String,
    items: Vec<ToggleItem>,
    selected: usize,
    active_tab: PropDialogTab,
    writable_selected: usize,
    readonly_selected: usize,
    actions: Vec<ActionItem>,
    actions_selected: usize,
    writable_list_state: ListState,
    readonly_list_state: ListState,
    actions_list_state: ListState,
    loading: bool,
    loading_rx: Option<Receiver<std::result::Result<Vec<ToggleItem>, String>>>,
    status: Option<String>,
    editing: bool,
    edit_buffer: String,
    edit_cursor: usize,
    edit_error: Option<String>,
    refreshing: bool,
    refresh_rx: Option<Receiver<PropDialogRefreshMessage>>,
    /// Index of the statistics bar the user clicked, used to render the
    /// crosshair + tooltip. Cleared on click-away or when the chart data
    /// changes (period/key/date change, tab switch).
    statistics_selected_bar: Option<usize>,
}

#[derive(Clone, Debug)]
enum AccountActionDialog {
    Menu {
        selected: usize,
    },
    Reauth {
        status: String,
        auth_url: String,
    },
    PushMessage {
        uid: String,
        input: String,
        cursor: usize,
        return_to_menu_selected: usize,
    },
    SettingsConfirm {
        action: SettingsAction,
    },
    /// A newer release was found; prompt the user to upgrade in place.
    UpdateAvailable {
        latest: String,
    },
    /// The install script is running; `lines` holds its most recent output and
    /// `pid` is the install process-group leader so a cancel can signal it.
    UpdateRunning {
        latest: String,
        lines: Vec<String>,
        pid: Option<u32>,
    },
    /// The upgrade finished (or failed); `message` is the outcome shown to the user.
    UpdateFinished {
        success: bool,
        message: String,
    },
}

/// A message from the background install thread to the UI: a line of script
/// output, or the terminal result once the script exits.
enum InstallMessage {
    Line(String),
    Done { success: bool, message: String },
}

/// Remove ANSI/CSI escape sequences (and stray control bytes like `\r`) from a
/// line so colored script output renders as plain text in the TUI dialog.
pub(in crate::tui) fn strip_ansi(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\u{1b}' => {
                i += 1;
                if i < chars.len() && chars[i] == '[' {
                    i += 1;
                    // CSI params/intermediates run until a final byte in 0x40..=0x7e.
                    while i < chars.len() && !('\u{40}'..='\u{7e}').contains(&chars[i]) {
                        i += 1;
                    }
                }
                i += 1; // skip the final byte (or the lone char after a bare ESC)
            }
            ch if ch.is_control() => i += 1,
            ch => {
                out.push(ch);
                i += 1;
            }
        }
    }
    out
}

/// Spawn the install script in its own process group, streaming each output line
/// over `tx` and a final [`InstallMessage::Done`] from background threads. Returns
/// the process-group id so the UI can cancel it. The whole pipeline's stderr is
/// merged into stdout so the dialog shows everything the script prints.
#[cfg(not(test))]
fn spawn_install(tx: Sender<InstallMessage>) -> Option<u32> {
    use std::io::{BufRead, BufReader};
    let command = format!(
        "{{ curl -sSfL {} | sh ; }} 2>&1",
        crate::actions::install_script_url()
    );
    let mut builder = std::process::Command::new("sh");
    builder
        .arg("-c")
        .arg(&command)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    // Install over the running binary (the script honours INSTALL_DIR verbatim) so
    // the post-update restart picks up the new version — including for dev builds,
    // where current_exe lives under target/ rather than a system bin dir.
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
    {
        builder.env("INSTALL_DIR", dir);
    }
    // Own process group so a cancel can signal the whole pipeline (curl + sh), not
    // just the outer shell.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        builder.process_group(0);
    }
    let mut child = match builder.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = tx.send(InstallMessage::Done {
                success: false,
                message: format!("无法启动安装脚本：{error}"),
            });
            return None;
        }
    };
    let pid = child.id();
    // Stream output on its own thread.
    if let Some(stdout) = child.stdout.take() {
        let line_tx = tx.clone();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                // The script prints colored output; strip ANSI so the dialog stays readable.
                let _ = line_tx.send(InstallMessage::Line(strip_ansi(&line)));
            }
        });
    }
    // Detect completion via the child's exit status, NOT stdout EOF — a backgrounded
    // grandchild can hold the pipe open long after the install finishes.
    thread::spawn(move || {
        let done = match child.wait() {
            Ok(status) if status.success() => InstallMessage::Done {
                success: true,
                message: "升级完成".to_string(),
            },
            Ok(status) => InstallMessage::Done {
                success: false,
                message: format!("安装脚本退出码 {:?}", status.code()),
            },
            Err(error) => InstallMessage::Done {
                success: false,
                message: format!("等待安装脚本失败：{error}"),
            },
        };
        let _ = tx.send(done);
    });
    Some(pid)
}

/// Test stub: never spawn a real installer; simulate a quick successful upgrade.
#[cfg(test)]
fn spawn_install(tx: Sender<InstallMessage>) -> Option<u32> {
    let _ = tx.send(InstallMessage::Line("[test] running install".to_string()));
    let _ = tx.send(InstallMessage::Done {
        success: true,
        message: "升级完成（测试）".to_string(),
    });
    None
}

/// Send `SIGKILL` to an install process group (leader pid == its group id).
#[cfg(unix)]
fn kill_process_group(pid: u32) {
    // Safety: a bare `kill(2)`; a negative pid targets the whole process group.
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
}

#[cfg(not(unix))]
fn kill_process_group(pid: u32) {
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status();
}

#[cfg(test)]
#[path = "../../tests/module_tests/tui/mod.rs"]
mod tests;
