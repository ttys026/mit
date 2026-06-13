use anyhow::{anyhow, bail, Result};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap};
use ratatui::Terminal;
use ratatui_core::style::Style as TextAreaStyle;
use ratatui_core::widgets::Widget as TextAreaWidget;
use ratatui_crossterm::CrosstermBackend;
use ratatui_textarea::{
    CursorMove, Input as TextAreaInput, Key as TextAreaKey, TextArea, WrapMode,
};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::stdout;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use time::{format_description::FormatItem, macros::format_description};

#[cfg(not(test))]
use std::process::Command;

use crate::cli::ensure_fresh_account;
use crate::mico_api::{Device, MicoClient};
use crate::mips_cloud::{
    config_from_account, start_property_cache_listener, CloudMipsHandle, CloudMipsStatus,
};
use crate::property_cache::PropertyCache;
use crate::spec_cache::load_spec;
use crate::storage::{
    default_auth, get_auth_accounts, get_home_dir, load_auth, load_settings, save_settings,
    AuthAccount, AuthState, Language, UserSettings,
};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
mod datetime;
mod device_cache;
mod device_history;
mod logs;
mod mijia_data;
mod pages;
mod shared;
mod spec;

pub(in crate::tui) use datetime::{
    add_months_to_date, date_end_timestamp, date_start_timestamp, timestamp_to_local_date,
    today_local_date,
};
pub(in crate::tui) use device_cache::*;
pub(in crate::tui) use device_history::*;
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
#[cfg(test)]
pub(in crate::tui) use spec::parse_bool_prop_value;
pub(in crate::tui) use spec::{
    collect_readable_props, extract_actions_from_spec, extract_prop_value,
    format_prop_value_for_dialog, is_error_with_negative_code, parse_prop_input_value,
    read_device_categories_from_template,
};

use self::pages::account as account_page;
use self::pages::bootstrap as bootstrap_page;
use self::pages::device::*;
use self::pages::prop as prop_page;
use shared::*;
use tui_logger::TuiLoggerWidget;

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

fn active_tab_has_search(tab: usize) -> bool {
    matches!(tab, 0..=2)
}

fn search_tab_slot(tab: usize) -> Option<usize> {
    match tab {
        0 => Some(0),
        1 => Some(1),
        2 => Some(2),
        _ => None,
    }
}

fn searchable_main_layout(content_area: Rect) -> [Rect; 3] {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(DEVICE_SEARCH_HEIGHT.saturating_sub(1)),
            Constraint::Min(0),
        ])
        .areas(content_area)
}

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
const SETTINGS_ITEM_COUNT: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsAction {
    ToggleLanguage,
    ToggleAutoSubscribeDeviceStatus,
    ClearCacheKeepAuth,
    ResetAll,
}

#[derive(Clone, Copy, Debug)]
struct ActiveAuthProcess {
    generation: u64,
    pid: u32,
}

struct CloudMipsRuntime {
    key: String,
    _handles: Vec<CloudMipsHandle>,
    rx: Receiver<CloudMipsStatus>,
    last_mqtt_response_at: Option<Instant>,
    last_ping_req_at: Option<Instant>,
    last_ping_resp_at: Option<Instant>,
}

static CLOUD_MIPS_RUNTIME: OnceLock<Mutex<Option<CloudMipsRuntime>>> = OnceLock::new();

fn cloud_mips_runtime() -> &'static Mutex<Option<CloudMipsRuntime>> {
    CLOUD_MIPS_RUNTIME.get_or_init(|| Mutex::new(None))
}

fn update_cloud_mips_runtime_liveness(
    runtime: &mut CloudMipsRuntime,
    status: &CloudMipsStatus,
    now: Instant,
) {
    match status {
        CloudMipsStatus::MessageReceived { .. } | CloudMipsStatus::PropertyApplied { .. } => {
            runtime.last_mqtt_response_at = Some(now);
        }
        CloudMipsStatus::Started { .. } => {}
        CloudMipsStatus::EventReceived { direction, summary } => {
            if direction == "outgoing" && summary.contains("PingReq") {
                runtime.last_ping_req_at = Some(now);
            }
            if direction == "incoming" {
                runtime.last_mqtt_response_at = Some(now);
                if summary.contains("PingResp") {
                    runtime.last_ping_resp_at = Some(now);
                }
            }
        }
        CloudMipsStatus::IgnoredMessage { .. }
        | CloudMipsStatus::AuthRejected { .. }
        | CloudMipsStatus::Error { .. }
        | CloudMipsStatus::Stopped => {}
    }
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

static ACTIVE_AUTH_PROCESS: OnceLock<Mutex<Option<ActiveAuthProcess>>> = OnceLock::new();

fn with_active_auth_process<R>(f: impl FnOnce(&mut Option<ActiveAuthProcess>) -> R) -> R {
    let state = ACTIVE_AUTH_PROCESS.get_or_init(|| Mutex::new(None));
    let mut guard = state.lock().expect("active auth process lock poisoned");
    f(&mut guard)
}

#[cfg(not(test))]
pub(crate) fn set_active_auth_process(generation: u64, pid: u32) {
    with_active_auth_process(|state| {
        *state = Some(ActiveAuthProcess { generation, pid });
    });
}

#[cfg(not(test))]
pub(crate) fn clear_active_auth_process_if_generation(generation: u64) {
    with_active_auth_process(|state| {
        if state
            .as_ref()
            .is_some_and(|active| active.generation == generation)
        {
            *state = None;
        }
    });
}

pub(crate) fn cancel_active_auth_process(generation: u64) -> Result<bool> {
    let pid = with_active_auth_process(|state| match state.as_ref() {
        Some(active) if active.generation == generation => {
            let pid = active.pid;
            *state = None;
            Some(pid)
        }
        _ => None,
    });
    let Some(pid) = pid else {
        return Ok(false);
    };
    terminate_process_by_pid(pid)?;
    Ok(true)
}

fn terminate_process_by_pid(pid: u32) -> Result<()> {
    #[cfg(not(test))]
    {
        let status = Command::new("kill").arg(pid.to_string()).status()?;
        if !status.success() {
            bail!("failed to terminate auth login process: pid={pid}");
        }
        Ok(())
    }

    #[cfg(test)]
    {
        let _ = pid;
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct LocalTransportRefreshMessage {
    generation: u64,
    error: Option<String>,
}

#[derive(Clone, Debug)]
enum AuthFlowMessage {
    Completed {
        generation: u64,
        success: bool,
        detail: String,
    },
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
    result
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
        if matches!(app.boot_state, BootState::Loading) {
            app.boot_spinner_index = app.boot_spinner_index.wrapping_add(1);
        }

        terminal.draw(|frame| draw(frame, app))?;

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

fn handle_key(app: &mut TuiApp, key: crossterm::event::KeyEvent) -> Result<bool> {
    if key.kind != KeyEventKind::Press {
        return Ok(false);
    }

    if !matches!(app.boot_state, BootState::Ready) {
        return match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Ok(true),
            _ => Ok(false),
        };
    }
    app.check_cloud_mips_stale_after_operation();

    if app.prop_dialog.is_some() {
        app.normalize_prop_dialog_tab_state();
        let editing = app
            .prop_dialog
            .as_ref()
            .is_some_and(|dialog| dialog.editing);
        if editing {
            let readonly_detail = app
                .prop_dialog
                .as_ref()
                .is_some_and(|dialog| dialog.active_tab == PropDialogTab::ReadOnly);
            if readonly_detail {
                match key.code {
                    KeyCode::Esc => app.cancel_prop_edit(),
                    KeyCode::Char('C') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                        copy_last_selection(app)
                    }
                    _ => {}
                }
                return Ok(false);
            }
            let editing_operation_records = app.prop_dialog.as_ref().is_some_and(|dialog| {
                dialog.active_tab == PropDialogTab::Logs
                    && dialog.editing
                    && operation_record_menu_is_open(dialog)
            });
            if editing_operation_records {
                match key.code {
                    KeyCode::Esc => app.close_operation_record_menu(),
                    KeyCode::Up => app.move_operation_record_menu(-1),
                    KeyCode::Down => app.move_operation_record_menu(1),
                    KeyCode::Enter | KeyCode::Char(' ') => app.select_operation_record_menu_item(),
                    KeyCode::Char('C') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                        copy_last_selection(app)
                    }
                    _ => {}
                }
                return Ok(false);
            }
            let editing_operation_record_date_picker =
                app.prop_dialog.as_ref().is_some_and(|dialog| {
                    dialog.active_tab == PropDialogTab::Logs
                        && dialog.editing
                        && operation_record_date_picker_is_open(dialog)
                });
            if editing_operation_record_date_picker {
                match key.code {
                    KeyCode::Esc => app.close_operation_record_date_picker(),
                    KeyCode::Enter | KeyCode::Char(' ') => app.select_operation_record_date(),
                    KeyCode::Left => app.move_operation_record_date(-1),
                    KeyCode::Right => app.move_operation_record_date(1),
                    KeyCode::Up => app.move_operation_record_date(-7),
                    KeyCode::Down => app.move_operation_record_date(7),
                    KeyCode::PageUp => app.move_operation_record_month(-1),
                    KeyCode::PageDown => app.move_operation_record_month(1),
                    KeyCode::Char('C') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                        copy_last_selection(app)
                    }
                    _ => {}
                }
                return Ok(false);
            }
            let editing_statistics_menu = app.prop_dialog.as_ref().is_some_and(|dialog| {
                dialog.active_tab == PropDialogTab::Statistics
                    && dialog.editing
                    && (statistics_key_menu_is_open(dialog)
                        || statistics_period_menu_is_open(dialog))
            });
            if editing_statistics_menu {
                match key.code {
                    KeyCode::Esc => app.close_statistics_menu(),
                    KeyCode::Up => app.move_statistics_menu(-1),
                    KeyCode::Down => app.move_statistics_menu(1),
                    KeyCode::Enter | KeyCode::Char(' ') => app.select_statistics_menu_item(),
                    KeyCode::Char('C') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                        copy_last_selection(app)
                    }
                    _ => {}
                }
                return Ok(false);
            }
            let editing_statistics_date_picker = app.prop_dialog.as_ref().is_some_and(|dialog| {
                dialog.active_tab == PropDialogTab::Statistics
                    && dialog.editing
                    && statistics_date_picker_is_open(dialog)
            });
            if editing_statistics_date_picker {
                match key.code {
                    KeyCode::Esc => app.close_statistics_date_picker(),
                    KeyCode::Enter | KeyCode::Char(' ') => app.select_statistics_date(),
                    KeyCode::Left => app.move_statistics_date(-1),
                    KeyCode::Right => app.move_statistics_date(1),
                    KeyCode::Up => app.move_statistics_date(-7),
                    KeyCode::Down => app.move_statistics_date(7),
                    KeyCode::PageUp => app.move_statistics_month(-1),
                    KeyCode::PageDown => app.move_statistics_month(1),
                    KeyCode::Char('C') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                        copy_last_selection(app)
                    }
                    _ => {}
                }
                return Ok(false);
            }
            let editing_actions = app
                .prop_dialog
                .as_ref()
                .is_some_and(|dialog| dialog.active_tab == PropDialogTab::Actions);
            match key.code {
                KeyCode::Esc => app.cancel_prop_edit(),
                KeyCode::Enter => {
                    if let Err(error) = app.submit_selected_prop_edit() {
                        app.set_prop_edit_error(error.to_string());
                    }
                }
                KeyCode::Tab if editing_actions => app.next_action_param_focus(),
                KeyCode::BackTab if editing_actions => app.prev_action_param_focus(),
                KeyCode::Tab => app.prop_edit_cycle_selector(true),
                KeyCode::BackTab => app.prop_edit_cycle_selector(false),
                KeyCode::Backspace => app.prop_edit_backspace(),
                KeyCode::Left => app.prop_edit_move_left(),
                KeyCode::Right => app.prop_edit_move_right(),
                KeyCode::Char('C') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                    copy_last_selection(app)
                }
                KeyCode::Char(ch) => app.prop_edit_push(ch),
                _ => {}
            }
            return Ok(false);
        }
        match key.code {
            KeyCode::Esc => app.prop_dialog = None,
            KeyCode::Up
                if app
                    .prop_dialog
                    .as_ref()
                    .is_some_and(|dialog| dialog.active_tab == PropDialogTab::Logs) =>
            {
                app.move_operation_record_row(-1)
            }
            KeyCode::Down
                if app
                    .prop_dialog
                    .as_ref()
                    .is_some_and(|dialog| dialog.active_tab == PropDialogTab::Logs) =>
            {
                app.move_operation_record_row(1)
            }
            KeyCode::Up => app.prev_bool_item(),
            KeyCode::Down => app.next_bool_item(),
            KeyCode::Char(ch) if ch.is_ascii_digit() => {
                app.set_prop_dialog_tab_by_visible_order(ch)
            }
            KeyCode::Tab | KeyCode::Right => app.switch_prop_dialog_tab(true),
            KeyCode::BackTab | KeyCode::Left => app.switch_prop_dialog_tab(false),
            KeyCode::Char('r') | KeyCode::Char('R') => {
                app.request_prop_dialog_refresh();
            }
            KeyCode::Char('s') | KeyCode::Char('S')
                if app
                    .prop_dialog
                    .as_ref()
                    .is_some_and(|dialog| dialog.active_tab == PropDialogTab::Logs) =>
            {
                app.open_operation_record_menu()
            }
            KeyCode::Char('s') | KeyCode::Char('S')
                if app
                    .prop_dialog
                    .as_ref()
                    .is_some_and(|dialog| dialog.active_tab == PropDialogTab::Statistics) =>
            {
                app.open_statistics_key_menu()
            }
            KeyCode::Char('p') | KeyCode::Char('P')
                if app
                    .prop_dialog
                    .as_ref()
                    .is_some_and(|dialog| dialog.active_tab == PropDialogTab::Statistics) =>
            {
                app.open_statistics_period_menu()
            }
            KeyCode::Char('d') | KeyCode::Char('D')
                if app
                    .prop_dialog
                    .as_ref()
                    .is_some_and(|dialog| dialog.active_tab == PropDialogTab::Logs) =>
            {
                app.open_operation_record_date_picker()
            }
            KeyCode::Char('d') | KeyCode::Char('D')
                if app
                    .prop_dialog
                    .as_ref()
                    .is_some_and(|dialog| dialog.active_tab == PropDialogTab::Statistics) =>
            {
                app.open_statistics_date_picker()
            }
            KeyCode::Char('c') | KeyCode::Char('C')
                if !key.modifiers.contains(KeyModifiers::SHIFT)
                    && app
                        .prop_dialog
                        .as_ref()
                        .is_some_and(|dialog| dialog.active_tab == PropDialogTab::Logs) =>
            {
                app.clear_operation_record_date_filter()
            }
            KeyCode::Char('c') | KeyCode::Char('C')
                if !key.modifiers.contains(KeyModifiers::SHIFT)
                    && app
                        .prop_dialog
                        .as_ref()
                        .is_some_and(|dialog| dialog.active_tab == PropDialogTab::Statistics) =>
            {
                app.clear_statistics_date_filter()
            }
            KeyCode::Enter | KeyCode::Char(' ')
                if app
                    .prop_dialog
                    .as_ref()
                    .is_some_and(|dialog| dialog.active_tab == PropDialogTab::Logs) =>
            {
                app.activate_operation_record_row()
            }
            KeyCode::Enter | KeyCode::Char(' ') => app.activate_selected_prop(),
            KeyCode::Char('C') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                copy_last_selection(app)
            }
            _ => {}
        }
        return Ok(false);
    }

    if app.account_action_dialog.is_some() {
        match app.account_action_dialog.clone() {
            Some(AccountActionDialog::Menu { .. }) => match key.code {
                KeyCode::Esc => app.account_action_dialog = None,
                KeyCode::Up => app.prev_account_action_item(),
                KeyCode::Down => app.next_account_action_item(),
                KeyCode::Enter => account_page::apply_selected_account_action(app)?,
                KeyCode::Char('C') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                    copy_last_selection(app)
                }
                _ => {}
            },
            Some(AccountActionDialog::Reauth { .. }) => match key.code {
                KeyCode::Esc | KeyCode::Enter => app.dismiss_reauth_dialog(),
                KeyCode::Char(ch) if ch.eq_ignore_ascii_case(&'c') => copy_reauth_auth_url(app),
                _ => {}
            },
            Some(AccountActionDialog::PushMessage { .. }) => match key.code {
                KeyCode::Esc => {
                    let return_to_menu_selected = match app.account_action_dialog.as_ref() {
                        Some(AccountActionDialog::PushMessage {
                            return_to_menu_selected,
                            ..
                        }) => *return_to_menu_selected,
                        _ => 0,
                    };
                    app.account_action_dialog = Some(AccountActionDialog::Menu {
                        selected: return_to_menu_selected,
                    });
                }
                KeyCode::Enter => account_page::submit_push_message_dialog(app)?,
                KeyCode::Backspace => account_page::push_message_backspace(app),
                KeyCode::Left => account_page::push_message_move_left(app),
                KeyCode::Right => account_page::push_message_move_right(app),
                KeyCode::Char('C') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                    copy_last_selection(app)
                }
                KeyCode::Char(ch) => account_page::push_message_push_char(app, ch),
                _ => {}
            },
            Some(AccountActionDialog::SettingsConfirm { .. }) => match key.code {
                KeyCode::Esc => app.account_action_dialog = None,
                KeyCode::Enter if app.confirm_settings_action()? => return Ok(true),
                _ => {}
            },
            None => {}
        }
        return Ok(false);
    }

    if app.search_is_active() {
        match key.code {
            KeyCode::Esc => app.blur_search(),
            KeyCode::Tab => app.next_tab(),
            KeyCode::BackTab => app.prev_tab(),
            KeyCode::Left if key.modifiers.contains(KeyModifiers::SHIFT) => app.prev_tab(),
            KeyCode::Right if key.modifiers.contains(KeyModifiers::SHIFT) => app.next_tab(),
            KeyCode::Enter => match app.active_tab {
                0 => app.open_selected_visible_account()?,
                1 => app.open_selected_visible_device(),
                _ => {}
            },
            KeyCode::Backspace => {
                app.search_backspace();
            }
            KeyCode::Left => app.search_move_cursor_left(),
            KeyCode::Right => app.search_move_cursor_right(),
            KeyCode::Up => app.prev_item(),
            KeyCode::Down => app.next_item(),
            KeyCode::PageUp if app.active_tab == 2 => app.scroll_logs_up(LOG_SCROLL_PAGE),
            KeyCode::PageDown if app.active_tab == 2 => app.scroll_logs_down(LOG_SCROLL_PAGE),
            KeyCode::Char(c) => app.search_insert(c),
            _ => {}
        }
        return Ok(false);
    }

    match key.code {
        KeyCode::Char('/') if active_tab_has_search(app.active_tab) => app.focus_search(),
        KeyCode::Char('q') | KeyCode::Esc => return Ok(true),
        KeyCode::Char('1') => app.set_active_tab(0),
        KeyCode::Char('2') => app.set_active_tab(1),
        KeyCode::Char('3') => app.set_active_tab(2),
        KeyCode::Char('4') => app.set_active_tab(3),
        KeyCode::Tab => app.next_tab(),
        KeyCode::BackTab => app.prev_tab(),
        KeyCode::Enter => match app.active_tab {
            0 => app.open_selected_visible_account()?,
            1 => {
                app.open_selected_visible_device();
            }
            3 => app.execute_selected_settings_action()?,
            _ => {}
        },
        KeyCode::Char('r') | KeyCode::Char('R') => {
            app.start_manual_sync();
        }
        KeyCode::Char('C') if key.modifiers.contains(KeyModifiers::SHIFT) => {
            copy_last_selection(app);
        }
        KeyCode::Char(ch) if app.active_tab == 2 && ch.eq_ignore_ascii_case(&'c') => {
            app.clear_logs();
        }
        KeyCode::Char('A') | KeyCode::Char('a') => account_page::start_add_account_auth_flow(app)?,
        KeyCode::Up => app.prev_item(),
        KeyCode::Down => app.next_item(),
        KeyCode::PageUp if app.active_tab == 2 => app.scroll_logs_up(LOG_SCROLL_PAGE),
        KeyCode::PageDown if app.active_tab == 2 => app.scroll_logs_down(LOG_SCROLL_PAGE),
        KeyCode::Left if key.modifiers.contains(KeyModifiers::SHIFT) => app.prev_tab(),
        KeyCode::Right if key.modifiers.contains(KeyModifiers::SHIFT) => app.next_tab(),
        _ => {}
    }

    Ok(false)
}

fn settings_action_label(action: SettingsAction, lang: Language) -> &'static str {
    match action {
        SettingsAction::ClearCacheKeepAuth => lang_str(lang, "重置设备缓存", "Reset Device Cache"),
        SettingsAction::ResetAll => lang_str(lang, "重置全部设置", "Reset All Settings"),
        SettingsAction::ToggleLanguage | SettingsAction::ToggleAutoSubscribeDeviceStatus => {
            unreachable!("Toggle settings have no confirm dialog")
        }
    }
}

fn auto_subscribe_device_status_label(lang: Language, enabled: bool) -> String {
    let label = lang_str(
        lang,
        "自动订阅设备状态(关闭后始终需要手动刷新)",
        "Auto Subscribe Device Status (off always requires manual refresh)",
    );
    let separator = lang_str(lang, "：", ": ");
    let state = if enabled {
        lang_str(lang, "开启", "On")
    } else {
        lang_str(lang, "关闭", "Off")
    };
    format!("{label}{separator}{state}")
}

fn settings_confirm_lines(action: SettingsAction, lang: Language) -> Vec<String> {
    let (action_line, show_irreversible_warning) = match action {
        SettingsAction::ClearCacheKeepAuth => (
            lang_str(
                lang,
                "将删除 ~/.mit 中除 auth.json 外的所有缓存",
                "Will delete all caches in ~/.mit except auth.json",
            )
            .to_string(),
            false,
        ),
        SettingsAction::ResetAll => (
            lang_str(
                lang,
                "将彻底删除 ~/.mit 目录（包括账号授权）",
                "Will permanently delete the ~/.mit directory (including account auth)",
            )
            .to_string(),
            true,
        ),
        SettingsAction::ToggleLanguage | SettingsAction::ToggleAutoSubscribeDeviceStatus => {
            unreachable!("Toggle settings have no confirm dialog")
        }
    };
    let mut lines = vec![
        format!(
            "{}: {}",
            lang_str(lang, "操作", "Action"),
            settings_action_label(action, lang)
        ),
        String::new(),
        action_line,
    ];
    if show_irreversible_warning {
        lines.push(String::new());
        lines.push(
            lang_str(lang, "⚠️ 该操作不可恢复", "⚠️ This action is irreversible").to_string(),
        );
    }
    lines
}

fn handle_mouse(
    app: &mut TuiApp,
    mouse: MouseEvent,
    terminal_area: ratatui::layout::Rect,
) -> Result<()> {
    if !matches!(app.boot_state, BootState::Ready) {
        return Ok(());
    }
    let left_down = matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left));
    let left_drag = matches!(mouse.kind, MouseEventKind::Drag(MouseButton::Left));
    let left_up = matches!(mouse.kind, MouseEventKind::Up(MouseButton::Left));
    let scroll_up = matches!(mouse.kind, MouseEventKind::ScrollUp);
    let scroll_down = matches!(mouse.kind, MouseEventKind::ScrollDown);
    if !left_down && !left_drag && !left_up && !scroll_up && !scroll_down {
        return Ok(());
    }
    if (left_down || left_up || scroll_up || scroll_down) && app.prop_dialog.is_none() {
        app.check_cloud_mips_stale_after_operation();
    }
    if left_down && app.search_is_active() {
        let [_tabs_area, content_area, _status_gap_area, _status_bar_area] =
            split_main_layout(terminal_area);
        let [search_area, _search_border_area, _rest_area] = searchable_main_layout(content_area);
        let on_search_row = mouse.row == content_area.y
            && mouse.column >= search_area.x
            && mouse.column < search_area.x.saturating_add(search_area.width);
        if !on_search_row {
            app.blur_search();
        }
    }

    if left_down && click_outside_selected_range(mouse) {
        clear_selection_state();
    }
    if (left_down || scroll_up || scroll_down)
        && handle_operation_record_mouse(app, mouse, terminal_area)
    {
        return Ok(());
    }
    if left_down && handle_statistics_mouse(app, mouse, terminal_area) {
        return Ok(());
    }
    if app.active_tab == 2
        && (left_down || left_drag || left_up)
        && handle_log_scrollbar_mouse(app, mouse, terminal_area)
    {
        return Ok(());
    }
    if left_down && selection_start(app, mouse, terminal_area) {
        return Ok(());
    }
    if left_drag && selection_drag(mouse) {
        return Ok(());
    }
    if left_up && selection_end_and_copy(app, mouse) {
        return Ok(());
    }
    if left_down {
        let [_tabs_area, _content_area, _status_gap_area, status_bar_area] =
            split_main_layout(terminal_area);
        let footer_area = footer_render_area(status_bar_area);
        if mouse.row >= footer_area.y
            && mouse.row < footer_area.y.saturating_add(footer_area.height)
            && mouse.column >= footer_area.x
            && mouse.column < footer_area.x.saturating_add(footer_area.width)
        {
            if let Some(operation) =
                footer_operation_at_column(app, mouse.column.saturating_sub(footer_area.x))
            {
                execute_footer_operation(app, operation)?;
            }
            return Ok(());
        }
    }

    let left_click = left_down;
    if let Some(AccountActionDialog::Menu { selected }) = app.account_action_dialog.as_ref() {
        let popup = centered_rect(48, 34, terminal_area);
        let inner = ratatui::layout::Rect::new(
            popup.x.saturating_add(1),
            popup.y.saturating_add(1),
            popup.width.saturating_sub(2),
            popup.height.saturating_sub(2),
        );
        if inner.width == 0 || inner.height == 0 {
            return Ok(());
        }
        if mouse.row < inner.y
            || mouse.row >= inner.y.saturating_add(inner.height)
            || mouse.column < inner.x
            || mouse.column >= inner.x.saturating_add(inner.width)
        {
            return Ok(());
        }
        if scroll_up {
            app.prev_account_action_item();
            return Ok(());
        }
        if scroll_down {
            app.next_account_action_item();
            return Ok(());
        }

        let selected_before = *selected;
        let clicked_row = (mouse.row - inner.y) as usize;
        if clicked_row >= 4 {
            return Ok(());
        }
        if let Some(AccountActionDialog::Menu { selected }) = &mut app.account_action_dialog {
            *selected = clicked_row;
        }
        if left_click && clicked_row == selected_before {
            account_page::apply_selected_account_action(app)?;
        }
        return Ok(());
    }
    if let Some(AccountActionDialog::PushMessage { input, cursor, .. }) =
        app.account_action_dialog.as_mut()
    {
        let textarea_area = push_message_textarea_area(terminal_area, input);
        if left_down
            && mouse.row >= textarea_area.y
            && mouse.row < textarea_area.y.saturating_add(textarea_area.height)
            && mouse.column >= textarea_area.x
            && mouse.column < textarea_area.x.saturating_add(textarea_area.width)
        {
            *cursor =
                textarea_cursor_for_mouse(input.as_str(), textarea_area, mouse.column, mouse.row);
        }
    }
    if app.account_action_dialog.is_some() {
        return Ok(());
    }
    if app.prop_dialog.is_some() {
        app.normalize_prop_dialog_tab_state();
        if app
            .prop_dialog
            .as_ref()
            .is_some_and(|dialog| dialog.editing)
        {
            let editing_actions = app
                .prop_dialog
                .as_ref()
                .is_some_and(|dialog| dialog.active_tab == PropDialogTab::Actions);
            if left_down {
                let popup = terminal_area;
                let inner = fullscreen_dialog_inner_area(popup);
                if inner.width > 0 && inner.height > 0 {
                    let editor_area = app
                        .prop_dialog
                        .as_ref()
                        .map(|dialog| prop_editor_layout(dialog, inner, app.language).editor_area)
                        .unwrap_or(inner);
                    if editing_actions {
                        if mouse.row >= editor_area.y
                            && mouse.row < editor_area.y.saturating_add(editor_area.height)
                            && mouse.column >= editor_area.x
                            && mouse.column < editor_area.x.saturating_add(editor_area.width)
                        {
                            let (row, was_focused, clicked_value, clicked_cursor) =
                                {
                                    let Some(dialog) = app.prop_dialog.as_ref() else {
                                        return Ok(());
                                    };
                                    let rows = action_param_rows_for_dialog(dialog);
                                    let Some(row_layout) =
                                        action_param_row_layouts(dialog, editor_area)
                                            .into_iter()
                                            .find(|layout| {
                                                mouse.row >= layout.row_area.y
                                                    && mouse.row
                                                        < layout
                                                            .row_area
                                                            .y
                                                            .saturating_add(layout.row_area.height)
                                            })
                                    else {
                                        return Ok(());
                                    };
                                    let clicked_value =
                                        dialog.actions.get(dialog.selected).and_then(|action| {
                                            if mouse.column >= row_layout.value_area.x
                                                && mouse.column
                                                    < row_layout
                                                        .value_area
                                                        .x
                                                        .saturating_add(row_layout.value_area.width)
                                            {
                                                action_param_selector_options(
                                                    dialog,
                                                    action,
                                                    row_layout.index,
                                                )
                                                .and_then(|options| {
                                                    selector_option_value_at_offset(
                                                        options.as_slice(),
                                                        mouse.column - row_layout.value_area.x,
                                                    )
                                                })
                                            } else {
                                                None
                                            }
                                        });
                                    let clicked_cursor = if clicked_value.is_none()
                                        && mouse.column >= row_layout.value_area.x
                                        && mouse.column
                                            < row_layout
                                                .value_area
                                                .x
                                                .saturating_add(row_layout.value_area.width)
                                    {
                                        rows.get(row_layout.index).map(|value| {
                                            textarea_cursor_for_mouse(
                                                value.as_str(),
                                                row_layout.value_area,
                                                mouse.column,
                                                mouse.row,
                                            )
                                        })
                                    } else {
                                        None
                                    };
                                    (
                                        row_layout.index,
                                        dialog.writable_selected == row_layout.index,
                                        clicked_value,
                                        clicked_cursor,
                                    )
                                };
                            app.focus_action_param_row(row);
                            if let Some(value) = clicked_value {
                                if let Some(dialog) = app.prop_dialog.as_mut() {
                                    let mut rows = action_param_rows_for_dialog(dialog);
                                    if row < rows.len() {
                                        rows[row] = serde_json::to_string(&value)
                                            .unwrap_or_else(|_| "null".to_string());
                                        set_action_param_rows_in_dialog(dialog, rows.as_slice());
                                        dialog.edit_cursor = rows[row].chars().count();
                                        dialog.edit_error = None;
                                    }
                                }
                            } else if was_focused {
                                if let Some(cursor) = clicked_cursor {
                                    if let Some(dialog) = app.prop_dialog.as_mut() {
                                        dialog.edit_cursor = cursor;
                                        dialog.edit_error = None;
                                    }
                                }
                            } else if let Some(dialog) = app.prop_dialog.as_mut() {
                                dialog.edit_error = None;
                            }
                            if !was_focused {
                                if let Some(dialog) = app.prop_dialog.as_mut() {
                                    dialog.edit_cursor = dialog
                                        .edit_buffer
                                        .lines()
                                        .nth(dialog.writable_selected)
                                        .map(|line| line.chars().count())
                                        .unwrap_or(0);
                                }
                            }
                            return Ok(());
                        }
                    } else {
                        let clicked_value = {
                            let Some(dialog) = app.prop_dialog.as_ref() else {
                                return Ok(());
                            };
                            let Some(item) = dialog.items.get(dialog.selected) else {
                                return Ok(());
                            };
                            prop_edit_selector_options(item).and_then(|options| {
                                let label_text = format!("> {}: ", item.prop.name);
                                let label_width = display_width(label_text.as_str())
                                    .min(editor_area.width.saturating_sub(1));
                                let value_area = ratatui::layout::Rect::new(
                                    editor_area.x.saturating_add(label_width),
                                    editor_area.y,
                                    editor_area.width.saturating_sub(label_width),
                                    1,
                                );
                                if mouse.row == editor_area.y
                                    && mouse.column >= value_area.x
                                    && mouse.column < value_area.x.saturating_add(value_area.width)
                                {
                                    selector_option_value_at_offset(
                                        options.as_slice(),
                                        mouse.column - value_area.x,
                                    )
                                } else {
                                    None
                                }
                            })
                        };
                        if let Some(value) = clicked_value {
                            if let Some(dialog) = app.prop_dialog.as_mut() {
                                dialog.edit_buffer = serde_json::to_string(&value)
                                    .unwrap_or_else(|_| "null".to_string());
                                dialog.edit_cursor = dialog.edit_buffer.chars().count();
                                dialog.edit_error = None;
                            }
                            return Ok(());
                        }
                        if let Some(dialog) = app.prop_dialog.as_ref() {
                            let textarea_area = prop_edit_textarea_area(dialog, editor_area);
                            if mouse.row >= textarea_area.y
                                && mouse.row < textarea_area.y.saturating_add(textarea_area.height)
                                && mouse.column >= textarea_area.x
                                && mouse.column
                                    < textarea_area.x.saturating_add(textarea_area.width)
                            {
                                let cursor = textarea_cursor_for_mouse(
                                    dialog.edit_buffer.as_str(),
                                    textarea_area,
                                    mouse.column,
                                    mouse.row,
                                );
                                if let Some(dialog) = app.prop_dialog.as_mut() {
                                    dialog.edit_cursor = cursor;
                                    dialog.edit_error = None;
                                }
                                return Ok(());
                            }
                        }
                    }
                }
            }
            return Ok(());
        }
        let popup = terminal_area;
        let inner = fullscreen_dialog_inner_area(popup);
        if inner.width == 0 || inner.height == 0 {
            return Ok(());
        }
        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(1)])
            .split(inner);
        let tabs_area = sections[0];
        if left_click
            && mouse.row >= tabs_area.y
            && mouse.row < tabs_area.y.saturating_add(tabs_area.height)
        {
            let visible_tabs = app
                .prop_dialog
                .as_ref()
                .map(visible_prop_dialog_tabs)
                .unwrap_or_default();
            let tab_titles =
                numbered_prop_dialog_tab_titles(visible_tabs.iter().copied(), app.language);
            if let Some(index) =
                tab_index_for_column_with_titles(mouse.column, tabs_area, &tab_titles)
            {
                if let Some(target) = visible_tabs.get(index).copied() {
                    app.set_prop_dialog_tab(target);
                    return Ok(());
                }
            }
        }

        let list_area = sections[1];
        if mouse.row < list_area.y
            || mouse.row >= list_area.y.saturating_add(list_area.height)
            || mouse.column < list_area.x
            || mouse.column >= list_area.x.saturating_add(list_area.width)
        {
            return Ok(());
        }
        if scroll_up {
            app.prev_bool_item();
            return Ok(());
        }
        if scroll_down {
            app.next_bool_item();
            return Ok(());
        }

        let selected_before = app
            .prop_dialog
            .as_ref()
            .map(|dialog| dialog.selected)
            .unwrap_or(usize::MAX);
        let clicked_row = (mouse.row - list_area.y) as usize;
        if !left_down {
            // Only handle left_down, ignore left_up (like device list)
            return Ok(());
        }
        if let Some(clicked_index) = app.get_prop_dialog_row_index(clicked_row) {
            if clicked_index == selected_before {
                // Already selected: activate/open editor
                app.activate_selected_prop();
            } else {
                // Different row: change selection only
                app.select_prop_dialog_row(clicked_row);
            }
        }
        return Ok(());
    }

    let [tabs_area, content_area, _status_gap_area, _status_bar_area] =
        split_main_layout(terminal_area);
    if left_click {
        let tab_inner_top = tabs_area.y.saturating_add(1);
        let tab_inner_bottom_exclusive = tabs_area
            .y
            .saturating_add(tabs_area.height.saturating_sub(1));
        if mouse.row >= tab_inner_top && mouse.row < tab_inner_bottom_exclusive {
            if let Some(index) = tab_index_for_column(mouse.column, tabs_area, app.language) {
                app.set_active_tab(index);
                return Ok(());
            }
        }
    }

    if app.active_tab == 0 || app.active_tab == 1 || app.active_tab == 2 || app.active_tab == 3 {
        if mouse.row < content_area.y
            || mouse.row >= content_area.y.saturating_add(content_area.height)
            || mouse.column < content_area.x
            || mouse.column >= content_area.x.saturating_add(content_area.width)
        {
            return Ok(());
        }

        if active_tab_has_search(app.active_tab) && left_down {
            let [search_area, _search_border_area, _rest_area] =
                searchable_main_layout(content_area);
            if mouse.row == search_area.y {
                if app.search_is_active() {
                    app.device_search_cursor = search_cursor_for_mouse(
                        app.input.as_str(),
                        app.language,
                        search_area,
                        mouse.column,
                    );
                    app.save_active_search_state();
                } else {
                    app.focus_search();
                }
                return Ok(());
            }
            if app.search_is_active() {
                app.blur_search();
            }
        }

        if scroll_up {
            app.prev_item();
            return Ok(());
        }
        if scroll_down {
            app.next_item();
            return Ok(());
        }

        let header_rows = if app.active_tab == 0 || app.active_tab == 1 {
            DEVICE_SEARCH_HEIGHT as usize + 1
        } else {
            1
        };
        let clicked_row = (mouse.row - content_area.y) as usize;
        if clicked_row < header_rows {
            return Ok(());
        }
        let clicked_row = clicked_row - header_rows;
        match app.active_tab {
            0 => {
                let position = app.account_list_state.offset().saturating_add(clicked_row);
                let Some(idx) = app.filtered_account_indices().get(position).copied() else {
                    return Ok(());
                };
                if idx == app.account_index {
                    app.open_selected_visible_account()?;
                } else {
                    app.account_index = idx;
                    app.request_local_transport_refresh(false);
                }
            }
            1 => {
                if !left_click {
                    return Ok(());
                }
                let position = app.device_list_state.offset().saturating_add(clicked_row);
                let Some(idx) = app.filtered_device_indices().get(position).copied() else {
                    return Ok(());
                };
                if idx == app.device_index {
                    app.open_selected_visible_device();
                } else {
                    app.device_index = idx;
                }
            }
            3 => {
                if !left_click {
                    return Ok(());
                }
                let idx = app.device_list_state.offset().saturating_add(clicked_row);
                if idx >= SETTINGS_ITEM_COUNT {
                    return Ok(());
                }
                let selected_before = app.settings_selected_index();
                app.select_settings_item(idx);
                if idx == selected_before {
                    app.execute_selected_settings_action()?;
                }
            }
            _ => {}
        }
        return Ok(());
    }
    Ok(())
}

fn rect_contains(area: Rect, column: u16, row: u16) -> bool {
    row >= area.y
        && row < area.y.saturating_add(area.height)
        && column >= area.x
        && column < area.x.saturating_add(area.width)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FooterOperation {
    Refresh,
    Search,
    Enter,
    SelectRecord,
    DateFilter,
    ClearDateFilter,
    Back,
    AddAccount,
    Copy,
    ClearLogs,
}

#[derive(Clone, Debug)]
struct FooterSegment {
    text: String,
    operation: Option<FooterOperation>,
}

fn build_footer_segments(
    ops: &[(&str, FooterOperation)],
    suffix: Option<String>,
) -> Vec<FooterSegment> {
    let mut segments = Vec::new();
    for (index, (label, op)) in ops.iter().enumerate() {
        if index > 0 {
            segments.push(FooterSegment {
                text: ", ".to_string(),
                operation: None,
            });
        }
        segments.push(FooterSegment {
            text: (*label).to_string(),
            operation: Some(*op),
        });
    }
    if let Some(suffix) = suffix.filter(|text| !text.is_empty()) {
        if !segments.is_empty() {
            segments.push(FooterSegment {
                text: ", ".to_string(),
                operation: None,
            });
        }
        segments.push(FooterSegment {
            text: suffix,
            operation: None,
        });
    }
    segments
}

fn footer_segments(app: &TuiApp) -> Vec<FooterSegment> {
    let lang = app.language;
    if let Some(dialog) = &app.account_action_dialog {
        return match dialog {
            AccountActionDialog::Menu { .. } => build_footer_segments(
                &[
                    (
                        lang_str(lang, "Enter: 选择", "Enter: Select"),
                        FooterOperation::Enter,
                    ),
                    (
                        lang_str(lang, "Esc: 返回", "Esc: Back"),
                        FooterOperation::Back,
                    ),
                ],
                None,
            ),
            AccountActionDialog::PushMessage { .. } => build_footer_segments(
                &[
                    (
                        lang_str(lang, "Enter: 发送", "Enter: Send"),
                        FooterOperation::Enter,
                    ),
                    (
                        lang_str(lang, "Esc: 返回", "Esc: Back"),
                        FooterOperation::Back,
                    ),
                ],
                None,
            ),
            AccountActionDialog::Reauth { auth_url, .. } => {
                let mut ops: Vec<(&str, FooterOperation)> = Vec::new();
                if !auth_url.trim().is_empty() && auth_url.trim() != "-" {
                    ops.push((lang_str(lang, "C: 复制", "C: Copy"), FooterOperation::Copy));
                }
                ops.push((
                    lang_str(lang, "Esc: 返回", "Esc: Back"),
                    FooterOperation::Back,
                ));
                build_footer_segments(ops.as_slice(), None)
            }
            AccountActionDialog::SettingsConfirm { .. } => build_footer_segments(
                &[
                    (
                        lang_str(lang, "Enter: 确认", "Enter: Confirm"),
                        FooterOperation::Enter,
                    ),
                    (
                        lang_str(lang, "Esc: 返回", "Esc: Back"),
                        FooterOperation::Back,
                    ),
                ],
                None,
            ),
        };
    }

    let current_device_label = lang_str(lang, "当前设备", "Current Device");
    if let Some(dialog) = &app.prop_dialog {
        if dialog.editing {
            if dialog.active_tab == PropDialogTab::Logs {
                return build_footer_segments(
                    &[
                        (
                            lang_str(lang, "Enter: 选择", "Enter: Select"),
                            FooterOperation::Enter,
                        ),
                        (
                            lang_str(lang, "Esc: 返回", "Esc: Back"),
                            FooterOperation::Back,
                        ),
                    ],
                    Some(format!("{current_device_label}: {}", dialog.device_did)),
                );
            }
            if dialog.active_tab == PropDialogTab::Statistics {
                return build_footer_segments(
                    &[
                        (
                            lang_str(lang, "Enter: 选择", "Enter: Select"),
                            FooterOperation::Enter,
                        ),
                        (
                            lang_str(lang, "Esc: 返回", "Esc: Back"),
                            FooterOperation::Back,
                        ),
                    ],
                    Some(format!("{current_device_label}: {}", dialog.device_did)),
                );
            }
            if dialog.active_tab == PropDialogTab::ReadOnly {
                return build_footer_segments(
                    &[(
                        lang_str(lang, "Esc: 返回", "Esc: Back"),
                        FooterOperation::Back,
                    )],
                    Some(format!("{current_device_label}: {}", dialog.device_did)),
                );
            }
            return build_footer_segments(
                &[
                    (
                        lang_str(lang, "Enter: 执行", "Enter: Execute"),
                        FooterOperation::Enter,
                    ),
                    (
                        lang_str(lang, "Esc: 返回", "Esc: Back"),
                        FooterOperation::Back,
                    ),
                ],
                Some(format!("{current_device_label}: {}", dialog.device_did)),
            );
        }
        return match dialog.active_tab {
            PropDialogTab::Writable => build_footer_segments(
                &[
                    (
                        lang_str(lang, "R: 刷新", "R: Refresh"),
                        FooterOperation::Refresh,
                    ),
                    (
                        lang_str(lang, "Esc: 返回", "Esc: Back"),
                        FooterOperation::Back,
                    ),
                    (
                        lang_str(lang, "Enter: 修改属性", "Enter: Edit Property"),
                        FooterOperation::Enter,
                    ),
                ],
                Some(format!("{current_device_label}: {}", dialog.device_did)),
            ),
            PropDialogTab::ReadOnly => build_footer_segments(
                &[
                    (
                        lang_str(lang, "R: 刷新", "R: Refresh"),
                        FooterOperation::Refresh,
                    ),
                    (
                        lang_str(lang, "Esc: 返回", "Esc: Back"),
                        FooterOperation::Back,
                    ),
                    (
                        lang_str(lang, "Enter: 查看属性", "Enter: View Property"),
                        FooterOperation::Enter,
                    ),
                ],
                Some(format!("{current_device_label}: {}", dialog.device_did)),
            ),
            PropDialogTab::Actions => build_footer_segments(
                &[
                    (
                        lang_str(lang, "R: 刷新", "R: Refresh"),
                        FooterOperation::Refresh,
                    ),
                    (
                        lang_str(lang, "Esc: 返回", "Esc: Back"),
                        FooterOperation::Back,
                    ),
                ],
                Some(format!("{current_device_label}: {}", dialog.device_did)),
            ),
            PropDialogTab::Logs => {
                let mut ops = vec![(
                    lang_str(lang, "R: 刷新", "R: Refresh"),
                    FooterOperation::Refresh,
                )];
                if !operation_record_requests(dialog).is_empty() {
                    ops.push((
                        lang_str(lang, "S: 选择记录", "S: Select Record"),
                        FooterOperation::SelectRecord,
                    ));
                }
                ops.push((
                    lang_str(lang, "D: 日期", "D: Date"),
                    FooterOperation::DateFilter,
                ));
                ops.push((
                    lang_str(lang, "C: 清除", "C: Clear"),
                    FooterOperation::ClearDateFilter,
                ));
                ops.push((
                    lang_str(lang, "Esc: 返回", "Esc: Back"),
                    FooterOperation::Back,
                ));
                build_footer_segments(
                    ops.as_slice(),
                    Some(format!("{current_device_label}: {}", dialog.device_did)),
                )
            }
            PropDialogTab::Statistics => build_footer_segments(
                &[
                    (
                        lang_str(lang, "R: 刷新", "R: Refresh"),
                        FooterOperation::Refresh,
                    ),
                    (
                        lang_str(lang, "P: 周/月/年", "P: Period"),
                        FooterOperation::DateFilter,
                    ),
                    (
                        lang_str(lang, "D: 日期", "D: Date"),
                        FooterOperation::DateFilter,
                    ),
                    (
                        lang_str(lang, "C: 清除", "C: Clear"),
                        FooterOperation::ClearDateFilter,
                    ),
                    (
                        lang_str(lang, "Esc: 返回", "Esc: Back"),
                        FooterOperation::Back,
                    ),
                ],
                Some(format!("{current_device_label}: {}", dialog.device_did)),
            ),
        };
    }

    if app.search_is_active() {
        return match app.active_tab {
            0 => {
                let matched_label = lang_str(lang, "匹配账户", "Matched accounts");
                build_footer_segments(
                    &[
                        (
                            lang_str(lang, "Esc: 返回", "Esc: Back"),
                            FooterOperation::Back,
                        ),
                        (
                            lang_str(lang, "Enter: 账户操作", "Enter: Account Actions"),
                            FooterOperation::Enter,
                        ),
                    ],
                    Some(format!(
                        "{matched_label}: {}",
                        app.filtered_account_indices().len()
                    )),
                )
            }
            1 => {
                let matched_label = lang_str(lang, "匹配设备", "Matched devices");
                build_footer_segments(
                    &[
                        (
                            lang_str(lang, "Esc: 返回", "Esc: Back"),
                            FooterOperation::Back,
                        ),
                        (
                            lang_str(lang, "Enter: 查看设备", "Enter: View Device"),
                            FooterOperation::Enter,
                        ),
                    ],
                    Some(format!(
                        "{matched_label}: {} {current_device_label}: {}",
                        app.filtered_device_indices().len(),
                        app.selected_device_did().unwrap_or("-")
                    )),
                )
            }
            2 => {
                let matched_label = lang_str(lang, "匹配日志", "Matched logs");
                build_footer_segments(
                    &[(
                        lang_str(lang, "Esc: 返回", "Esc: Back"),
                        FooterOperation::Back,
                    )],
                    Some(format!(
                        "{matched_label}: {}",
                        logs_lines_for_display(app).len()
                    )),
                )
            }
            _ => Vec::new(),
        };
    }

    match app.active_tab {
        0 => build_footer_segments(
            &[
                (
                    lang_str(lang, "A: 新增账户", "A: Add Account"),
                    FooterOperation::AddAccount,
                ),
                (
                    lang_str(lang, "/: 搜索", "/: Search"),
                    FooterOperation::Search,
                ),
                (
                    lang_str(lang, "Enter: 账户操作", "Enter: Account Actions"),
                    FooterOperation::Enter,
                ),
            ],
            None,
        ),
        1 => {
            let total_label = lang_str(lang, "设备总数", "Total Devices");
            build_footer_segments(
                &[
                    (
                        lang_str(lang, "R: 刷新", "R: Refresh"),
                        FooterOperation::Refresh,
                    ),
                    (
                        lang_str(lang, "/: 搜索", "/: Search"),
                        FooterOperation::Search,
                    ),
                    (
                        lang_str(lang, "Enter: 查看设备", "Enter: View Device"),
                        FooterOperation::Enter,
                    ),
                ],
                Some(format!(
                    "{total_label}: {} {current_device_label}: {}",
                    app.devices.len(),
                    app.selected_device_did().unwrap_or("-")
                )),
            )
        }
        3 => build_footer_segments(
            &[(
                lang_str(lang, "Enter: 选择", "Enter: Select"),
                FooterOperation::Enter,
            )],
            Some("".to_string()),
        ),
        _ => build_footer_segments(
            &[
                (
                    lang_str(lang, "C: 清空", "C: Clear"),
                    FooterOperation::ClearLogs,
                ),
                (
                    lang_str(lang, "/: 搜索", "/: Search"),
                    FooterOperation::Search,
                ),
            ],
            Some("".to_string()),
        ),
    }
}

fn footer_operation_at_column(app: &TuiApp, column: u16) -> Option<FooterOperation> {
    let mut start = 0_u16;
    for segment in footer_segments(app) {
        let width = display_width(segment.text.as_str());
        let end = start.saturating_add(width);
        if column >= start && column < end {
            return segment.operation;
        }
        start = end;
    }
    None
}

fn execute_footer_operation(app: &mut TuiApp, operation: FooterOperation) -> Result<()> {
    match operation {
        FooterOperation::Refresh => {
            let _ = handle_key(
                app,
                crossterm::event::KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
            )?;
            Ok(())
        }
        FooterOperation::Search => {
            let _ = handle_key(
                app,
                crossterm::event::KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
            )?;
            Ok(())
        }
        FooterOperation::Enter => {
            let _ = handle_key(
                app,
                crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            )?;
            Ok(())
        }
        FooterOperation::SelectRecord => {
            let _ = handle_key(
                app,
                crossterm::event::KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE),
            )?;
            Ok(())
        }
        FooterOperation::DateFilter => {
            let _ = handle_key(
                app,
                crossterm::event::KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
            )?;
            Ok(())
        }
        FooterOperation::ClearDateFilter => {
            let _ = handle_key(
                app,
                crossterm::event::KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
            )?;
            Ok(())
        }
        FooterOperation::Back => {
            let _ = handle_key(
                app,
                crossterm::event::KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            )?;
            Ok(())
        }
        FooterOperation::AddAccount => {
            let _ = handle_key(
                app,
                crossterm::event::KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
            )?;
            Ok(())
        }
        FooterOperation::Copy => {
            copy_reauth_auth_url(app);
            Ok(())
        }
        FooterOperation::ClearLogs => {
            let _ = handle_key(
                app,
                crossterm::event::KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
            )?;
            Ok(())
        }
    }
}

fn footer_display_text(app: &TuiApp, now_ms: u128) -> String {
    let mut text = footer_text(app);
    if let Some(badge) = footer_copy_badge_text(&app.logs, now_ms) {
        text.push_str(badge);
    }
    text
}

fn note_copy_success(app: &mut TuiApp) {
    app.log(format!("{FOOTER_COPY_LOG_PREFIX}{}", now_epoch_millis()));
}

fn copy_reauth_auth_url(app: &mut TuiApp) {
    let Some(auth_url) = app
        .account_action_dialog
        .as_ref()
        .and_then(|dialog| match dialog {
            AccountActionDialog::Reauth { auth_url, .. } if !auth_url.trim().is_empty() => {
                Some(auth_url.clone())
            }
            _ => None,
        })
    else {
        app.log("no auth url to copy".to_string());
        return;
    };

    if let Err(error) = copy_text_to_clipboard(auth_url.as_str()) {
        app.log(format!("copy auth url failed: {error}"));
    } else {
        note_copy_success(app);
    }
}

fn char_cursor_byte_index(input: &str, cursor: usize) -> usize {
    input
        .char_indices()
        .nth(cursor)
        .map(|(index, _)| index)
        .unwrap_or(input.len())
}

fn search_prefix(lang: Language) -> String {
    let label = lang_str(lang, "搜索", "Search");
    format!("/ {label}: ")
}

fn visible_search_input(input: &str, width: u16) -> (String, usize) {
    if width == 0 {
        return (String::new(), input.chars().count());
    }
    if display_width(input) <= width {
        return (input.to_string(), 0);
    }
    if width <= 3 {
        return (".".repeat(width as usize), input.chars().count());
    }

    let tail_width = width.saturating_sub(3);
    let mut start_byte = input.len();
    let mut start_char = input.chars().count();
    let mut seen_width = 0u16;
    for (idx, ch) in input.char_indices().rev() {
        let char_width = if ch.is_ascii() { 1 } else { 2 };
        if seen_width.saturating_add(char_width) > tail_width {
            break;
        }
        seen_width = seen_width.saturating_add(char_width);
        start_byte = idx;
        start_char = start_char.saturating_sub(1);
    }

    let mut visible = "...".to_string();
    visible.push_str(&input[start_byte..]);
    (visible, start_char)
}

fn search_plain_line(app: &TuiApp, area_width: u16) -> String {
    let prefix = search_prefix(app.language);
    let prefix_width = display_width(prefix.as_str());
    let input_width = area_width.saturating_sub(prefix_width);
    let (visible_input, _) = visible_search_input(app.input.as_str(), input_width);
    format!("{prefix}{visible_input}")
}

fn search_cursor_for_mouse(input: &str, lang: Language, area: Rect, column: u16) -> usize {
    if area.width == 0 {
        return 0;
    }
    let target_col = column
        .saturating_sub(area.x)
        .min(area.width.saturating_sub(1));
    let prefix_width = display_width(search_prefix(lang).as_str());
    let input_width = area.width.saturating_sub(prefix_width);
    let input_col = target_col.saturating_sub(prefix_width);
    let (_, start_char) = visible_search_input(input, input_width);
    let ellipsis_width = if start_char > 0 { 3 } else { 0 };
    if start_char > 0 && input_col <= ellipsis_width {
        return start_char;
    }
    let input_col = input_col.saturating_sub(ellipsis_width);
    let mut cursor = 0usize;
    let mut cursor_col = 0u16;
    for ch in input.chars().skip(start_char) {
        let char_width = if ch.is_ascii() { 1 } else { 2 };
        let next_col = cursor_col.saturating_add(char_width);
        if next_col > input_col {
            break;
        }
        cursor = cursor.saturating_add(1);
        cursor_col = next_col;
    }
    start_char.saturating_add(cursor).min(input.chars().count())
}

fn search_line(app: &TuiApp, area_width: u16) -> Line<'static> {
    let focused = app.search_is_active();
    let prefix_style = if focused {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Blue)
    };
    let prefix = search_prefix(app.language);
    let prefix_width = display_width(prefix.as_str());
    let input_width = area_width.saturating_sub(prefix_width);
    let (visible_input, start_char) = visible_search_input(app.input.as_str(), input_width);
    let mut spans = vec![Span::styled(prefix, prefix_style)];
    if focused {
        let cursor = app.device_search_cursor.min(app.input.chars().count());
        let ellipsis_chars = if start_char > 0 { 3 } else { 0 };
        let visible_cursor = cursor
            .saturating_sub(start_char)
            .saturating_add(ellipsis_chars)
            .min(visible_input.chars().count());
        let cursor_byte = char_cursor_byte_index(visible_input.as_str(), visible_cursor);
        let before = &visible_input[..cursor_byte];
        spans.push(Span::raw(before.to_string()));
        if let Some(ch) = visible_input[cursor_byte..].chars().next() {
            let ch_len = ch.len_utf8();
            spans.push(Span::styled(
                ch.to_string(),
                Style::default().bg(Color::Green).fg(Color::Black),
            ));
            spans.push(Span::raw(visible_input[cursor_byte + ch_len..].to_string()));
        } else {
            spans.push(Span::styled(
                " ",
                Style::default().bg(Color::Green).fg(Color::Black),
            ));
        }
    } else {
        spans.push(Span::raw(visible_input));
    }
    Line::from(spans)
}

fn render_search_bar(
    frame: &mut ratatui::Frame<'_>,
    app: &TuiApp,
    search_area: Rect,
    border_area: Rect,
) {
    frame.render_widget(
        Paragraph::new(search_line(app, search_area.width)),
        search_area,
    );
    frame.render_widget(
        Block::default().borders(ratatui::widgets::Borders::BOTTOM),
        border_area,
    );
}

fn draw(frame: &mut ratatui::Frame<'_>, app: &mut TuiApp) {
    match &app.boot_state {
        BootState::Loading => {
            bootstrap_page::draw_bootstrap_splash(frame, app.boot_spinner_index);
            return;
        }
        BootState::Ready => {}
    }
    let selected_text = selected_surface();

    let [tabs_area, content_area, _status_gap_area, status_bar_area] =
        split_main_layout(frame.area());

    let titles = tab_titles(app.language)
        .iter()
        .map(|name| Line::from(Span::styled(*name, Style::default().fg(Color::Blue))))
        .collect::<Vec<_>>();
    let tabs = Tabs::new(titles)
        .block(Block::default().borders(all_borders()))
        .highlight_style(
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
        .select(app.active_tab);
    frame.render_widget(tabs, tabs_area);
    if app.bootstrap_pending.is_some() {
        let badge = lang_str(app.language, "刷新中...", "Refreshing...");
        let badge_width = badge.chars().count() as u16 + 4;
        if tabs_area.width > badge_width + 2 {
            let badge_area = ratatui::layout::Rect::new(
                tabs_area
                    .x
                    .saturating_add(tabs_area.width.saturating_sub(badge_width + 1)),
                tabs_area.y.saturating_add(1),
                badge_width,
                1,
            );
            frame.render_widget(
                Paragraph::new(format!(" {badge}")).style(
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                badge_area,
            );
        }
    }

    match app.active_tab {
        0 => {
            app.ensure_account_selection_visible();
            let filtered_indices = app.filtered_account_indices();
            let selected = filtered_indices
                .iter()
                .position(|index| *index == app.account_index);
            app.account_list_state.select(selected);
            let [search_area, search_border_area, rest_area] = searchable_main_layout(content_area);
            let [header_area, list_area] = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(0)])
                .areas(rest_area);
            let rows = filtered_indices
                .iter()
                .filter_map(|index| app.accounts.get(*index))
                .map(|account| {
                    account_page::account_list_row(
                        account,
                        app.offline_account_uids.contains(account.user.uid.as_str()),
                        app.language,
                    )
                })
                .collect::<Vec<_>>();
            let columns = account_page::compute_account_list_columns(
                &rows,
                header_area.width as usize,
                app.language,
            );
            let items = rows
                .iter()
                .enumerate()
                .map(|(idx, row)| {
                    let mut item = ListItem::new(
                        account_page::format_account_list_item_with_columns(row, columns),
                    );
                    if selected == Some(idx) {
                        item = item.style(active_row_style());
                    }
                    item
                })
                .collect::<Vec<_>>();
            render_search_bar(frame, app, search_area, search_border_area);
            frame.render_widget(
                Paragraph::new(account_page::format_account_list_header_line_with_columns(
                    columns,
                    app.language,
                )),
                header_area,
            );
            frame.render_stateful_widget(List::new(items), list_area, &mut app.account_list_state);
        }
        1 => {
            app.hydrate_devices_from_cache_if_empty();
            app.ensure_device_selection_visible();
            let device_categories = read_device_categories(app.home_dir.as_path(), app.language);
            let filtered_indices = app.filtered_device_indices_with_categories(&device_categories);
            let selected = filtered_indices
                .iter()
                .position(|index| *index == app.device_index);
            app.device_list_state.select(selected);
            let [search_area, search_border_area, rest_area] = searchable_main_layout(content_area);
            let [header_area, list_area] = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(0)])
                .areas(rest_area);
            let rows = filtered_indices
                .iter()
                .filter_map(|index| app.devices.get(*index))
                .map(|device| {
                    let account_label = app.device_account_label(device);
                    let category = device_categories
                        .get(device.model.as_str())
                        .map(String::as_str)
                        .unwrap_or("-");
                    device_list_row(
                        &device.name,
                        category,
                        &device.room_name,
                        account_label.as_str(),
                    )
                })
                .collect::<Vec<_>>();
            let columns =
                compute_device_list_columns(&rows, header_area.width as usize, app.language);
            let items = rows
                .iter()
                .enumerate()
                .map(|(idx, row)| {
                    let mut item =
                        ListItem::new(format_device_list_item_with_columns(row, columns));
                    if selected == Some(idx) {
                        item = item.style(active_row_style());
                    }
                    item
                })
                .collect::<Vec<_>>();
            render_search_bar(frame, app, search_area, search_border_area);
            frame.render_widget(
                Paragraph::new(format_device_list_header_line_with_columns(
                    columns,
                    app.language,
                )),
                header_area,
            );
            frame.render_stateful_widget(List::new(items), list_area, &mut app.device_list_state);
        }
        3 => {
            let selected = Some(
                app.settings_selected_index()
                    .min(SETTINGS_ITEM_COUNT.saturating_sub(1)),
            );
            app.device_list_state.select(selected);
            let [header_area, list_area] = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(0)])
                .areas(content_area);
            frame.render_widget(
                Paragraph::new(lang_str(app.language, "设置项", "Settings")),
                header_area,
            );

            let lang_label = match app.language {
                Language::Chinese => "语言 / Language: 中文",
                Language::English => "Language / 语言: English",
            };
            let rows = [
                lang_label.to_string(),
                auto_subscribe_device_status_label(app.language, app.auto_subscribe_device_status),
                lang_str(
                    app.language,
                    "重置设备缓存（重新同步设备）",
                    "Reset Device Cache (Re-sync Devices)",
                )
                .to_string(),
                lang_str(
                    app.language,
                    "重置全部设置（删除 ~/.mit）",
                    "Reset All Settings (Delete ~/.mit)",
                )
                .to_string(),
            ];
            let selected_idx = app.settings_selected_index();
            let items = rows
                .iter()
                .enumerate()
                .map(|(idx, row)| {
                    let mut item = ListItem::new(row.clone());
                    if idx == selected_idx {
                        item = item.style(active_row_style());
                    }
                    item
                })
                .collect::<Vec<_>>();
            frame.render_stateful_widget(List::new(items), list_area, &mut app.device_list_state);
        }
        _ => {
            let [search_area, search_border_area, list_area] = searchable_main_layout(content_area);
            let (visual_lines, log_text_area, scrollbar_area) =
                log_visual_lines_and_areas(app, list_area);
            remember_log_text_width(log_text_area.width);
            let line_count = visual_lines.len();
            app.clamp_log_scroll_offset_for_view(line_count, log_text_area.height);
            let offset = log_scroll_offset_for_view(app, line_count, log_text_area.height);
            let visible_lines = visual_lines
                .into_iter()
                .skip(offset)
                .take(log_text_area.height as usize)
                .collect::<Vec<_>>();
            let selected_text = selected_surface();
            let selected_text = if selected_text
                .as_ref()
                .is_some_and(|active| log_selection_is_stale(active, log_text_area, &visible_lines))
            {
                clear_selection_state();
                None
            } else {
                selected_text
            };
            let lines = visible_lines
                .iter()
                .enumerate()
                .map(|(idx, line)| {
                    if let Some(active) = selected_text.as_ref() {
                        if active.snapshot.surface == SelectionSurface::Logs {
                            if let Some((mut start, mut end)) = selected_cols_for_line(active, idx)
                            {
                                let width = display_width(line);
                                if end == u16::MAX {
                                    end = width;
                                }
                                start = start.min(width);
                                end = end.min(width);
                                return highlight_line_range(line, start, end);
                            }
                        }
                    }
                    highlight_log_search_matches(line, app.search_query())
                })
                .collect::<Vec<_>>();
            render_search_bar(frame, app, search_area, search_border_area);
            frame.render_widget(
                TuiLoggerWidget::default()
                    .output_timestamp(None)
                    .output_level(None)
                    .output_target(false)
                    .output_file(false)
                    .output_line(false),
                list_area,
            );
            frame.render_widget(Paragraph::new(Text::from(lines)), log_text_area);
            if let Some(scrollbar_area) = scrollbar_area {
                if let Some(geometry) =
                    log_scrollbar_geometry(line_count, scrollbar_area.height, app.log_scroll_offset)
                {
                    frame.render_widget(
                        Paragraph::new(Text::from(log_scrollbar_lines(
                            geometry,
                            scrollbar_area.height,
                        ))),
                        scrollbar_area,
                    );
                }
            }
        }
    }

    if let Some(dialog) = &app.account_action_dialog {
        match dialog {
            AccountActionDialog::Menu { selected } => {
                let popup = centered_rect(48, 34, frame.area());
                let items = [
                    lang_str(app.language, "推送消息", "Push Message"),
                    lang_str(app.language, "重新登录(小米)", "Relogin (Xiaomi)"),
                    lang_str(app.language, "重新登录(米家)", "Relogin (Mijia)"),
                    lang_str(app.language, "退出登录", "Logout"),
                ]
                .iter()
                .enumerate()
                .map(|(idx, item)| {
                    let selected_marker = if idx == *selected { ">" } else { " " };
                    ListItem::new(format!("{selected_marker} {item}"))
                })
                .collect::<Vec<_>>();
                frame.render_widget(Clear, popup);
                frame.render_widget(
                    List::new(items).block(
                        Block::default().borders(all_borders()).title(lang_str(
                            app.language,
                            "账户操作",
                            "Account Actions",
                        )),
                    ),
                    popup,
                );
            }
            AccountActionDialog::Reauth { status, auth_url } => {
                let popup = centered_rect(74, 42, frame.area());
                frame.render_widget(Clear, popup);
                let reauth_lines = [status.clone(), String::new(), auth_url.clone()];
                let reauth_text = if let Some(active) = selected_text.as_ref() {
                    if active.snapshot.surface == SelectionSurface::ReauthDialog {
                        Text::from(
                            reauth_lines
                                .iter()
                                .enumerate()
                                .map(|(idx, line)| {
                                    if let Some((mut start, mut end)) =
                                        selected_cols_for_line(active, idx)
                                    {
                                        let width = display_width(line);
                                        if end == u16::MAX {
                                            end = width;
                                        }
                                        start = start.min(width);
                                        end = end.min(width);
                                        highlight_line_range(line, start, end)
                                    } else {
                                        Line::from(line.clone())
                                    }
                                })
                                .collect::<Vec<_>>(),
                        )
                    } else {
                        Text::from(reauth_lines.join("\n"))
                    }
                } else {
                    Text::from(reauth_lines.join("\n"))
                };
                frame.render_widget(
                    Paragraph::new(reauth_text)
                        .block(
                            Block::default()
                                .borders(top_bottom_borders())
                                .title(lang_str(app.language, "登录", "Login")),
                        )
                        .wrap(Wrap { trim: true }),
                    popup,
                );
            }
            AccountActionDialog::PushMessage {
                uid, input, cursor, ..
            } => {
                let popup = push_message_dialog_popup(frame.area());
                frame.render_widget(Clear, popup);
                frame.render_widget(
                    Block::default()
                        .borders(top_bottom_borders())
                        .title(lang_str(app.language, "推送通知", "Push Notification")),
                    popup,
                );
                let inner = ratatui::layout::Rect::new(
                    popup.x.saturating_add(1),
                    popup.y.saturating_add(1),
                    popup.width.saturating_sub(2),
                    popup.height.saturating_sub(2),
                );
                let sections = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(3),
                        Constraint::Length(push_message_textarea_area(frame.area(), input).height),
                        Constraint::Min(1),
                    ])
                    .split(inner);
                frame.render_widget(
                    Paragraph::new(format!(
                        "{}: {uid}\n\n{}:",
                        lang_str(app.language, "账户 ID", "Account ID"),
                        lang_str(app.language, "消息内容", "Message")
                    ))
                    .wrap(Wrap { trim: false }),
                    sections[0],
                );
                let textarea = single_line_textarea(input, *cursor, true);
                render_textarea_widget(&textarea, sections[1], frame.buffer_mut());
                let command_area = push_message_command_area(frame.area(), input.as_str());
                let command_line =
                    push_message_cli_preview_line(uid.as_str(), input.as_str(), app.language);
                {
                    let buffer = frame.buffer_mut();
                    for x in command_area.x..command_area.x.saturating_add(command_area.width) {
                        let cell = &mut buffer[(x, command_area.y)];
                        cell.skip = false;
                        cell.set_symbol(" ");
                    }
                    buffer.set_stringn(
                        command_area.x,
                        command_area.y,
                        command_line,
                        command_area.width as usize,
                        Style::default(),
                    );
                }
                if let Some(active) = selected_text.as_ref() {
                    if active.snapshot.surface == SelectionSurface::PushMessageInput {
                        apply_selection_highlight_to_area(
                            frame.buffer_mut(),
                            sections[1],
                            active.snapshot.lines.as_slice(),
                            active,
                        );
                    } else if active.snapshot.surface == SelectionSurface::PushMessageCommand {
                        apply_selection_highlight_to_area(
                            frame.buffer_mut(),
                            command_area,
                            active.snapshot.lines.as_slice(),
                            active,
                        );
                    }
                }
            }
            AccountActionDialog::SettingsConfirm { action } => {
                let popup = centered_rect(74, 42, frame.area());
                frame.render_widget(Clear, popup);
                let lines = settings_confirm_lines(*action, app.language);
                frame.render_widget(
                    Paragraph::new(lines.join("\n"))
                        .block(
                            Block::default()
                                .borders(top_bottom_borders())
                                .title(lang_str(app.language, "确认操作", "Confirm Action")),
                        )
                        .wrap(Wrap { trim: true }),
                    popup,
                );
            }
        }
    }

    prop_page::draw_prop_dialog(frame, app, selected_text.as_ref());

    if let Some(active) = selected_text.as_ref() {
        if matches!(
            active.snapshot.surface,
            SelectionSurface::PropEditor
                | SelectionSurface::PushMessageInput
                | SelectionSurface::PushMessageCommand
                | SelectionSurface::SearchInput
        ) {
            apply_selection_highlight_to_area(
                frame.buffer_mut(),
                active.snapshot.area,
                active.snapshot.lines.as_slice(),
                active,
            );
        }
    }

    let now = now_epoch_millis();
    if let Some(active) = selected_text.as_ref() {
        if active.snapshot.surface == SelectionSurface::Footer {
            let footer_area = footer_render_area(status_bar_area);
            frame.render_widget(Paragraph::new(footer_line(app, now)), footer_area);
            apply_selection_highlight_to_area(
                frame.buffer_mut(),
                footer_area,
                &[footer_display_text(app, now)],
                active,
            );
            return;
        }
    }
    let footer_line = footer_line(app, now);
    frame.render_widget(
        Paragraph::new(footer_line),
        footer_render_area(status_bar_area),
    );
}

fn footer_text(app: &TuiApp) -> String {
    footer_segments(app)
        .into_iter()
        .map(|segment| segment.text)
        .collect::<String>()
}

fn footer_line(app: &TuiApp, now_ms: u128) -> Line<'static> {
    let mut spans = vec![Span::styled(
        footer_text(app),
        Style::default().add_modifier(Modifier::DIM),
    )];
    if let Some(badge) = footer_copy_badge_text(&app.logs, now_ms) {
        spans.push(Span::styled(
            badge,
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        ));
    }
    Line::from(spans)
}

fn footer_copy_badge_text(logs: &VecDeque<String>, now_ms: u128) -> Option<&'static str> {
    footer_copy_badge_visible_at(logs, now_ms).then_some(FOOTER_COPY_BADGE_TEXT)
}

fn footer_copy_badge_visible_at(logs: &VecDeque<String>, now_ms: u128) -> bool {
    let Some(entry) = logs
        .iter()
        .rev()
        .find(|line| line.starts_with(FOOTER_COPY_LOG_PREFIX))
    else {
        return false;
    };
    let Ok(copied_at_ms) = entry[FOOTER_COPY_LOG_PREFIX.len()..].parse::<u128>() else {
        return false;
    };
    now_ms.saturating_sub(copied_at_ms) < 1_000
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

    fn check_cloud_mips_stale_after_operation(&mut self) {
        if !self.auto_subscribe_device_status || cloud_mips_disabled() {
            return;
        }
        let now = Instant::now();
        let snapshot = {
            let Ok(runtime) = cloud_mips_runtime().lock() else {
                self.log("cloud MIPS runtime lock poisoned");
                return;
            };
            runtime.as_ref().map(|runtime| {
                (
                    runtime.last_mqtt_response_at,
                    runtime.last_ping_req_at,
                    runtime.last_ping_resp_at,
                )
            })
        };
        let Some((last_mqtt_response_at, last_ping_req_at, last_ping_resp_at)) = snapshot else {
            self.refresh_cloud_mips_listeners();
            self.request_prop_dialog_refresh_allow_editing();
            return;
        };

        let latest_response_at = match (last_mqtt_response_at, last_ping_resp_at) {
            (Some(response_at), Some(ping_resp_at)) => Some(response_at.max(ping_resp_at)),
            (Some(response_at), None) => Some(response_at),
            (None, Some(ping_resp_at)) => Some(ping_resp_at),
            (None, None) => None,
        };
        if let Some(latest_response_at) = latest_response_at {
            let elapsed = now
                .checked_duration_since(latest_response_at)
                .unwrap_or_default();
            if elapsed <= CLOUD_MIPS_RESPONSE_STALE_THRESHOLD {
                return;
            }
            self.log(format!(
                "cloud MIPS response stale: last response {}s ago; restarting listeners",
                elapsed.as_secs()
            ));
            self.restart_cloud_mips_listeners();
            self.request_prop_dialog_refresh_allow_editing();
            return;
        }

        if let Some(last_ping_req_at) = last_ping_req_at {
            let elapsed = now
                .checked_duration_since(last_ping_req_at)
                .unwrap_or_default();
            self.log(format!(
                "cloud MIPS waiting for PingResp: last PingReq {}s ago",
                elapsed.as_secs()
            ));
            if elapsed > CLOUD_MIPS_RESPONSE_STALE_THRESHOLD {
                self.restart_cloud_mips_listeners();
                self.request_prop_dialog_refresh_allow_editing();
            }
        }
    }

    fn restart_cloud_mips_listeners(&mut self) {
        let Ok(mut runtime) = cloud_mips_runtime().lock() else {
            self.log("cloud MIPS runtime lock poisoned");
            return;
        };
        *runtime = None;
        drop(runtime);
        self.refresh_cloud_mips_listeners();
    }

    fn refresh_cloud_mips_listeners(&mut self) {
        if !self.auto_subscribe_device_status {
            self.log("cloud MIPS not started: auto subscribe disabled");
            if let Ok(mut runtime) = cloud_mips_runtime().lock() {
                *runtime = None;
            }
            return;
        }

        if cloud_mips_disabled() {
            if cloud_mips_disabled_by_user() {
                self.log("cloud MIPS not started: disabled by MIT_DISABLE_CLOUD_MIPS");
            }
            if let Ok(mut runtime) = cloud_mips_runtime().lock() {
                *runtime = None;
            }
            return;
        }

        let groups = self.cloud_mips_account_device_groups();
        if groups.is_empty() {
            let account_count = self.accounts.len();
            let oauth_account_count = self
                .accounts
                .iter()
                .filter(|account| !account.access_token.trim().is_empty())
                .count();
            let offline_account_count = self.offline_account_uids.len();
            let tagged_device_count = self
                .devices
                .iter()
                .filter(|device| device_account_uid(device).is_some())
                .count();
            self.log(format!(
                "cloud MIPS not started: no eligible OAuth account/device groups \
                 (accounts={account_count}, oauth_accounts={oauth_account_count}, \
                 offline_accounts={offline_account_count}, devices={}, tagged_devices={tagged_device_count})",
                self.devices.len()
            ));
            if let Ok(mut runtime) = cloud_mips_runtime().lock() {
                *runtime = None;
            }
            return;
        }

        let key = cloud_mips_runtime_key(&groups);
        let Ok(mut runtime) = cloud_mips_runtime().lock() else {
            self.log("cloud MIPS runtime lock poisoned");
            return;
        };
        if runtime.as_ref().is_some_and(|current| current.key == key) {
            return;
        }

        let account_count = groups.len();
        let device_count = groups.iter().map(|(_, dids)| dids.len()).sum::<usize>();
        self.log(format!(
            "cloud MIPS starting: accounts={account_count} devices={device_count}"
        ));

        let (tx, rx) = mpsc::channel();
        let mut handles = Vec::new();
        let mut errors = Vec::new();
        for (account, dids) in groups {
            match config_from_account(&account).and_then(|config| {
                start_property_cache_listener(
                    config,
                    dids,
                    self.property_cache.clone(),
                    Some(tx.clone()),
                )
            }) {
                Ok(handle) => handles.push(handle),
                Err(error) => errors.push(format!(
                    "{}: {error}",
                    account_page::format_account_label(&account)
                )),
            }
        }
        drop(tx);

        let started_count = handles.len();
        if handles.is_empty() {
            *runtime = None;
        } else {
            *runtime = Some(CloudMipsRuntime {
                key,
                _handles: handles,
                rx,
                last_mqtt_response_at: None,
                last_ping_req_at: None,
                last_ping_resp_at: None,
            });
        }
        drop(runtime);

        for error in errors {
            self.log(format!("cloud MIPS start failed: {error}"));
        }
        if started_count > 0 {
            self.log(format!(
                "cloud MIPS listener threads spawned: {started_count}"
            ));
        }
    }

    fn cloud_mips_account_device_groups(&self) -> Vec<(AuthAccount, Vec<String>)> {
        let mut groups = Vec::new();
        for account in &self.accounts {
            if account.access_token.trim().is_empty()
                || self.offline_account_uids.contains(&account.user.uid)
            {
                continue;
            }
            let dids = self
                .devices
                .iter()
                .filter(|device| device_account_uid(device) == Some(account.user.uid.as_str()))
                .map(|device| device.did.clone())
                .collect::<Vec<_>>();
            if dids.is_empty() {
                continue;
            }
            groups.push((account.clone(), dids));
        }
        groups
    }

    fn process_cloud_mips_messages(&mut self) {
        let statuses = {
            let Ok(mut runtime) = cloud_mips_runtime().lock() else {
                self.log("cloud MIPS runtime lock poisoned");
                return;
            };
            let Some(runtime) = runtime.as_mut() else {
                return;
            };
            let statuses = runtime.rx.try_iter().collect::<Vec<_>>();
            let now = Instant::now();
            for status in &statuses {
                update_cloud_mips_runtime_liveness(runtime, status, now);
            }
            statuses
        };

        for status in statuses {
            match status {
                CloudMipsStatus::Started { host, device_count } => self.log(format!(
                    "cloud MIPS listening on {host} for {device_count} devices"
                )),
                CloudMipsStatus::EventReceived { direction, summary } => {
                    self.log(format!("cloud MIPS mqtt {direction}: {summary}"))
                }
                CloudMipsStatus::MessageReceived { topic, payload_len } => self.log(format!(
                    "cloud MIPS message: topic={topic} bytes={payload_len}"
                )),
                CloudMipsStatus::PropertyApplied { did, siid, piid } => self.log(format!(
                    "cloud MIPS property update: did={did} siid={siid} piid={piid}"
                )),
                CloudMipsStatus::Error { message } => {
                    self.log(format!("cloud MIPS error: {message}"))
                }
                CloudMipsStatus::IgnoredMessage { reason } => {
                    self.log(format!("cloud MIPS ignored message: {reason}"))
                }
                CloudMipsStatus::AuthRejected { message } => {
                    self.log(format!(
                        "cloud MIPS auth rejected: {message}; refreshing auth"
                    ));
                    self.handle_cloud_mips_auth_rejected();
                }
                CloudMipsStatus::Stopped => self.log("cloud MIPS stopped"),
            }
        }
    }

    fn handle_cloud_mips_auth_rejected(&mut self) {
        let groups = self.cloud_mips_account_device_groups();
        let stale_uids = groups
            .iter()
            .map(|(account, _)| account.user.uid.clone())
            .collect::<HashSet<_>>();
        if stale_uids.is_empty() {
            return;
        }
        for account in &mut self.accounts {
            if stale_uids.contains(&account.user.uid) {
                account.expires_ts = 1;
            }
        }
        match cloud_mips_runtime().lock() {
            Ok(mut runtime) => *runtime = None,
            Err(_) => self.log("cloud MIPS runtime lock poisoned"),
        }
        self.start_background_sync();
    }

    fn apply_cached_prop_dialog_updates(&mut self) {
        let Some(dialog) = self.prop_dialog.as_mut() else {
            return;
        };
        if dialog.loading || dialog.editing {
            return;
        }
        for item in &mut dialog.items {
            let Some(cached) = self.property_cache.get_property(
                dialog.device_did.as_str(),
                item.prop.siid,
                item.prop.piid,
            ) else {
                continue;
            };
            if let Some(value) = extract_prop_value(&cached) {
                item.value = value;
            }
        }
    }

    fn should_apply_bootstrap(&self, generation: u64, uid: &str) -> bool {
        self.bootstrap_pending
            .as_ref()
            .is_some_and(|pending| pending.generation == generation && pending.uid == uid)
    }

    fn process_background_messages(&mut self) {
        self.process_cloud_mips_messages();
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

    fn search_is_active(&self) -> bool {
        self.input_mode && active_tab_has_search(self.active_tab)
    }

    fn search_query(&self) -> &str {
        self.input.trim()
    }

    fn save_active_search_state(&mut self) {
        if let Some(slot) = search_tab_slot(self.active_tab) {
            self.search_inputs[slot] = self.input.clone();
            self.search_cursors[slot] = self.device_search_cursor.min(self.input.chars().count());
        }
    }

    fn load_active_search_state(&mut self) {
        if let Some(slot) = search_tab_slot(self.active_tab) {
            self.input = self.search_inputs[slot].clone();
            self.device_search_cursor = self.search_cursors[slot].min(self.input.chars().count());
        } else {
            self.input.clear();
            self.device_search_cursor = 0;
        }
    }

    fn focus_search(&mut self) {
        self.input_mode = true;
        self.device_search_cursor = self.device_search_cursor.min(self.input.chars().count());
        self.save_active_search_state();
        self.reset_log_scroll_if_active();
        self.ensure_search_selection_visible();
    }

    fn blur_search(&mut self) {
        self.save_active_search_state();
        self.input_mode = false;
    }

    fn search_insert(&mut self, ch: char) {
        let cursor = self.device_search_cursor.min(self.input.chars().count());
        let index = char_cursor_byte_index(self.input.as_str(), cursor);
        self.input.insert(index, ch);
        self.device_search_cursor = cursor.saturating_add(1);
        self.save_active_search_state();
        self.reset_log_scroll_if_active();
        self.ensure_search_selection_visible();
    }

    fn search_backspace(&mut self) {
        let cursor = self.device_search_cursor.min(self.input.chars().count());
        if cursor == 0 {
            return;
        }
        let start = char_cursor_byte_index(self.input.as_str(), cursor - 1);
        let end = char_cursor_byte_index(self.input.as_str(), cursor);
        self.input.replace_range(start..end, "");
        self.device_search_cursor = cursor - 1;
        self.save_active_search_state();
        self.reset_log_scroll_if_active();
        self.ensure_search_selection_visible();
    }

    fn search_move_cursor_left(&mut self) {
        self.device_search_cursor = self.device_search_cursor.saturating_sub(1);
        self.save_active_search_state();
    }

    fn search_move_cursor_right(&mut self) {
        self.device_search_cursor = (self.device_search_cursor + 1).min(self.input.chars().count());
        self.save_active_search_state();
    }

    fn ensure_search_selection_visible(&mut self) {
        match self.active_tab {
            0 => self.ensure_account_selection_visible(),
            1 => self.ensure_device_selection_visible(),
            _ => {}
        }
    }

    fn account_matches_search(&self, account: &AuthAccount, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let query = query.to_lowercase();
        let row = account_page::account_list_row(
            account,
            self.offline_account_uids
                .contains(account.user.uid.as_str()),
            self.language,
        );
        let label = account_page::format_account_label(account);
        [
            row.region.as_str(),
            row.nickname.as_str(),
            row.uid.as_str(),
            row.xiaomi_status.as_str(),
            row.mijia_status.as_str(),
            label.as_str(),
        ]
        .iter()
        .any(|value| value.to_lowercase().contains(query.as_str()))
    }

    fn filtered_account_indices(&self) -> Vec<usize> {
        let query = self.search_query();
        self.accounts
            .iter()
            .enumerate()
            .filter_map(|(index, account)| {
                self.account_matches_search(account, query).then_some(index)
            })
            .collect()
    }

    fn selected_account_is_visible(&self) -> bool {
        self.filtered_account_indices()
            .contains(&self.account_index)
    }

    fn ensure_account_selection_visible(&mut self) {
        let indices = self.filtered_account_indices();
        if indices.is_empty() {
            self.account_index = 0;
            self.account_list_state.select(None);
            return;
        }
        if !indices.contains(&self.account_index) {
            self.account_index = indices[0];
        }
    }

    fn open_selected_visible_account(&mut self) -> Result<()> {
        if self.selected_account_is_visible() {
            account_page::open_account_action_dialog(self)?;
        }
        Ok(())
    }

    fn open_selected_visible_device(&mut self) {
        if !self.selected_device_is_visible() {
            return;
        }
        if let Err(error) = self.open_prop_dialog() {
            account_page::open_offline_prop_dialog_for_current(self, &error);
        }
    }

    fn device_matches_search(&self, device: &Device, category: &str, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let query = query.to_lowercase();
        [device.room_name.as_str(), device.name.as_str(), category]
            .iter()
            .any(|value| value.to_lowercase().contains(query.as_str()))
    }

    fn filtered_device_indices(&self) -> Vec<usize> {
        let categories = read_device_categories(self.home_dir.as_path(), self.language);
        self.filtered_device_indices_with_categories(&categories)
    }

    fn filtered_device_indices_with_categories(
        &self,
        categories: &HashMap<String, String>,
    ) -> Vec<usize> {
        let query = self.search_query();
        self.devices
            .iter()
            .enumerate()
            .filter_map(|(index, device)| {
                let category = categories
                    .get(device.model.as_str())
                    .map(String::as_str)
                    .unwrap_or("-");
                self.device_matches_search(device, category, query)
                    .then_some(index)
            })
            .collect()
    }

    fn selected_device_is_visible(&self) -> bool {
        self.filtered_device_indices().contains(&self.device_index)
    }

    fn ensure_device_selection_visible(&mut self) {
        let indices = self.filtered_device_indices();
        if indices.is_empty() {
            self.device_index = 0;
            self.device_list_state.select(None);
            return;
        }
        if !indices.contains(&self.device_index) {
            self.device_index = indices[0];
        }
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
            2 => SettingsAction::ClearCacheKeepAuth,
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

    fn execute_selected_settings_action(&mut self) -> Result<()> {
        let action = self.selected_settings_action();
        match action {
            SettingsAction::ToggleLanguage | SettingsAction::ToggleAutoSubscribeDeviceStatus => {
                self.execute_settings_action(action)
            }
            _ => {
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

    fn prop_dialog_has_toggle_items(&self) -> bool {
        self.prop_dialog.as_ref().is_some_and(|dialog| {
            !dialog.loading
                && dialog.status.is_none()
                && !matches!(
                    dialog.active_tab,
                    PropDialogTab::Logs | PropDialogTab::Statistics
                )
                && !prop_dialog_indices_for_tab(dialog, dialog.active_tab).is_empty()
        })
    }

    fn normalize_prop_dialog_tab_state(&mut self) {
        if let Some(dialog) = &mut self.prop_dialog {
            let visible_tabs = visible_prop_dialog_tabs(dialog);
            let Some(default_tab) = visible_tabs.first().copied() else {
                return;
            };
            if !visible_tabs.contains(&dialog.active_tab) {
                dialog.active_tab = default_tab;
            }
            let indices = prop_dialog_indices_for_tab(dialog, dialog.active_tab);
            if indices.is_empty() {
                return;
            }
            let preferred = match dialog.active_tab {
                PropDialogTab::Writable => dialog.writable_selected,
                PropDialogTab::ReadOnly => dialog.readonly_selected,
                PropDialogTab::Actions => dialog.actions_selected,
                PropDialogTab::Logs | PropDialogTab::Statistics => dialog.selected,
            };
            let selected = if indices.contains(&dialog.selected) {
                dialog.selected
            } else if indices.contains(&preferred) {
                preferred
            } else {
                indices[0]
            };
            dialog.selected = selected;
            match dialog.active_tab {
                PropDialogTab::Writable => dialog.writable_selected = selected,
                PropDialogTab::ReadOnly => dialog.readonly_selected = selected,
                PropDialogTab::Actions => dialog.actions_selected = selected,
                PropDialogTab::Logs | PropDialogTab::Statistics => {}
            }
        }
    }

    fn switch_prop_dialog_tab(&mut self, forward: bool) {
        if let Some(dialog) = &mut self.prop_dialog {
            if dialog.editing {
                return;
            }
            let visible_tabs = visible_prop_dialog_tabs(dialog);
            if visible_tabs.is_empty() {
                return;
            }
            let current_index = visible_tabs
                .iter()
                .position(|tab| *tab == dialog.active_tab)
                .unwrap_or(0);
            let next_index = if forward {
                (current_index + 1) % visible_tabs.len()
            } else if current_index == 0 {
                visible_tabs.len() - 1
            } else {
                current_index - 1
            };
            self.set_prop_dialog_tab(visible_tabs[next_index]);
        }
    }

    fn set_prop_dialog_tab_by_visible_order(&mut self, key: char) {
        let Some(position) = key
            .to_digit(10)
            .and_then(|value| usize::try_from(value).ok())
        else {
            return;
        };
        if position == 0 {
            return;
        }
        let target = self
            .prop_dialog
            .as_ref()
            .and_then(|dialog| visible_prop_dialog_tabs(dialog).get(position - 1).copied());
        if let Some(target) = target {
            self.set_prop_dialog_tab(target);
        }
    }

    fn set_prop_dialog_tab(&mut self, target: PropDialogTab) {
        if let Some(dialog) = &mut self.prop_dialog {
            if dialog.editing {
                return;
            }
            let visible_tabs = visible_prop_dialog_tabs(dialog);
            if !visible_tabs.contains(&target) {
                return;
            }
            match dialog.active_tab {
                PropDialogTab::Writable => dialog.writable_selected = dialog.selected,
                PropDialogTab::ReadOnly => dialog.readonly_selected = dialog.selected,
                PropDialogTab::Actions => dialog.actions_selected = dialog.selected,
                PropDialogTab::Logs | PropDialogTab::Statistics => {}
            }
            dialog.active_tab = target;
            // Reset active index to 0 when switching tabs
            match target {
                PropDialogTab::Writable => dialog.writable_selected = 0,
                PropDialogTab::ReadOnly => dialog.readonly_selected = 0,
                PropDialogTab::Actions => dialog.actions_selected = 0,
                PropDialogTab::Logs | PropDialogTab::Statistics => {}
            }
            let indices = prop_dialog_indices_for_tab(dialog, target);
            if let Some(selected) = indices.first().copied() {
                dialog.selected = selected;
            }
        }
    }

    fn select_prop_dialog_row(&mut self, row: usize) -> bool {
        if let Some(dialog) = &mut self.prop_dialog {
            let indices = prop_dialog_indices_for_tab(dialog, dialog.active_tab);
            if indices.is_empty() {
                return false;
            }
            let offset = match dialog.active_tab {
                PropDialogTab::Writable => dialog.writable_list_state.offset(),
                PropDialogTab::ReadOnly => dialog.readonly_list_state.offset(),
                PropDialogTab::Actions => dialog.actions_list_state.offset(),
                PropDialogTab::Logs | PropDialogTab::Statistics => 0,
            };
            let position = offset.saturating_add(row);
            if let Some(global_index) = indices.get(position) {
                dialog.selected = *global_index;
                match dialog.active_tab {
                    PropDialogTab::Writable => dialog.writable_selected = dialog.selected,
                    PropDialogTab::ReadOnly => dialog.readonly_selected = dialog.selected,
                    PropDialogTab::Actions => dialog.actions_selected = dialog.selected,
                    PropDialogTab::Logs | PropDialogTab::Statistics => {}
                }
                return true;
            }
        }
        false
    }

    fn get_prop_dialog_row_index(&self, row: usize) -> Option<usize> {
        if let Some(dialog) = &self.prop_dialog {
            let indices = prop_dialog_indices_for_tab(dialog, dialog.active_tab);
            if indices.is_empty() {
                return None;
            }
            let offset = match dialog.active_tab {
                PropDialogTab::Writable => dialog.writable_list_state.offset(),
                PropDialogTab::ReadOnly => dialog.readonly_list_state.offset(),
                PropDialogTab::Actions => dialog.actions_list_state.offset(),
                PropDialogTab::Logs | PropDialogTab::Statistics => 0,
            };
            let position = offset.saturating_add(row);
            indices.get(position).copied()
        } else {
            None
        }
    }

    fn activate_selected_prop(&mut self) {
        if !self.prop_dialog_has_toggle_items() {
            return;
        }
        let active_tab = self
            .prop_dialog
            .as_ref()
            .map(|dialog| dialog.active_tab)
            .unwrap_or(PropDialogTab::Writable);
        if active_tab == PropDialogTab::Actions {
            if let Err(error) = self.start_selected_action_params_edit() {
                account_page::open_offline_prop_dialog_for_current(self, &error);
            }
            return;
        }
        if active_tab == PropDialogTab::ReadOnly {
            let _ = self.start_selected_readonly_prop_detail();
            return;
        }
        if matches!(active_tab, PropDialogTab::Logs | PropDialogTab::Statistics) {
            return;
        }
        let Some(item) = self
            .prop_dialog
            .as_ref()
            .and_then(|dialog| dialog.items.get(dialog.selected))
            .cloned()
        else {
            return;
        };
        if !item.prop.writable {
            self.log(format!("property is read-only: {}", item.prop.name));
            return;
        }
        if let Err(error) = self.start_selected_prop_edit() {
            account_page::open_offline_prop_dialog_for_current(self, &error);
        }
    }

    fn start_selected_readonly_prop_detail(&mut self) -> Result<()> {
        let dialog = self
            .prop_dialog
            .as_mut()
            .ok_or_else(|| anyhow!("property dialog is not open"))?;
        if dialog.loading {
            bail!("属性仍在加载中");
        }
        if dialog.status.is_some() {
            bail!("当前设备离线，无法查看属性");
        }
        let item = dialog
            .items
            .get(dialog.selected)
            .ok_or_else(|| anyhow!("未选中只读属性"))?;
        if item.prop.writable {
            bail!("当前属性可写");
        }
        dialog.editing = true;
        dialog.edit_error = None;
        dialog.edit_buffer.clear();
        dialog.edit_cursor = 0;
        Ok(())
    }

    fn start_selected_action_params_edit(&mut self) -> Result<()> {
        let dialog = self
            .prop_dialog
            .as_mut()
            .ok_or_else(|| anyhow!("property dialog is not open"))?;
        if dialog.loading {
            bail!("属性仍在加载中");
        }
        if dialog.status.is_some() {
            bail!("当前设备离线，无法执行操作");
        }
        let action = dialog
            .actions
            .get(dialog.selected)
            .ok_or_else(|| anyhow!("未选中可执行操作"))?;
        dialog.editing = true;
        dialog.edit_error = None;
        let placeholders = action
            .input_piids
            .iter()
            .enumerate()
            .map(|(idx, _)| action_param_default_row(dialog, action, idx))
            .collect::<Vec<_>>();
        set_action_param_rows_in_dialog(dialog, placeholders.as_slice());
        dialog.writable_selected = 0;
        dialog.edit_cursor = placeholders.first().map(|s| s.chars().count()).unwrap_or(0);
        Ok(())
    }

    fn start_selected_prop_edit(&mut self) -> Result<()> {
        let dialog = self
            .prop_dialog
            .as_mut()
            .ok_or_else(|| anyhow!("property dialog is not open"))?;
        if dialog.loading {
            bail!("属性仍在加载中");
        }
        if dialog.status.is_some() {
            bail!("当前设备离线，无法设置属性");
        }
        let item = dialog
            .items
            .get(dialog.selected)
            .ok_or_else(|| anyhow!("未选中可编辑属性"))?;
        if !item.prop.writable {
            bail!("当前属性为只读");
        }
        dialog.editing = true;
        dialog.edit_error = None;
        if let Some(options) = prop_edit_selector_options(item) {
            let index = prop_edit_selector_index(item, options.as_slice());
            dialog.edit_buffer =
                serde_json::to_string(&options[index].1).unwrap_or_else(|_| "null".to_string());
        } else {
            dialog.edit_buffer =
                serde_json::to_string(&item.value).unwrap_or_else(|_| "null".to_string());
        }
        dialog.edit_cursor = dialog.edit_buffer.chars().count();
        Ok(())
    }

    fn cancel_prop_edit(&mut self) {
        if let Some(dialog) = &mut self.prop_dialog {
            dialog.editing = false;
            dialog.edit_buffer.clear();
            dialog.edit_cursor = 0;
            dialog.edit_error = None;
        }
    }

    fn prop_edit_push(&mut self, ch: char) {
        if let Some(dialog) = &mut self.prop_dialog {
            if dialog.editing {
                if dialog.active_tab == PropDialogTab::Actions {
                    apply_action_param_row_key(
                        dialog,
                        crossterm::event::KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
                    );
                    dialog.edit_error = None;
                    return;
                }
                if prop_edit_selector_is_active(dialog) {
                    return;
                }
                apply_single_line_textarea_key(
                    &mut dialog.edit_buffer,
                    &mut dialog.edit_cursor,
                    crossterm::event::KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
                );
                dialog.edit_error = None;
            }
        }
    }

    fn prop_edit_backspace(&mut self) {
        if let Some(dialog) = &mut self.prop_dialog {
            if dialog.editing {
                if dialog.active_tab == PropDialogTab::Actions {
                    apply_action_param_row_key(
                        dialog,
                        crossterm::event::KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
                    );
                    dialog.edit_error = None;
                    return;
                }
                if prop_edit_selector_is_active(dialog) {
                    return;
                }
                apply_single_line_textarea_key(
                    &mut dialog.edit_buffer,
                    &mut dialog.edit_cursor,
                    crossterm::event::KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
                );
                dialog.edit_error = None;
            }
        }
    }

    fn prop_edit_move_left(&mut self) {
        if let Some(dialog) = &mut self.prop_dialog {
            if dialog.editing {
                if dialog.active_tab == PropDialogTab::Actions {
                    apply_action_param_row_key(
                        dialog,
                        crossterm::event::KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
                    );
                    return;
                }
                if cycle_prop_edit_selector(dialog, false) {
                    return;
                }
                apply_single_line_textarea_key(
                    &mut dialog.edit_buffer,
                    &mut dialog.edit_cursor,
                    crossterm::event::KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
                );
            }
        }
    }

    fn prop_edit_move_right(&mut self) {
        if let Some(dialog) = &mut self.prop_dialog {
            if dialog.editing {
                if dialog.active_tab == PropDialogTab::Actions {
                    apply_action_param_row_key(
                        dialog,
                        crossterm::event::KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
                    );
                    return;
                }
                if cycle_prop_edit_selector(dialog, true) {
                    return;
                }
                apply_single_line_textarea_key(
                    &mut dialog.edit_buffer,
                    &mut dialog.edit_cursor,
                    crossterm::event::KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
                );
            }
        }
    }

    fn set_prop_edit_error(&mut self, message: String) {
        if let Some(dialog) = &mut self.prop_dialog {
            if dialog.editing {
                dialog.edit_error = Some(message);
            }
        }
    }

    fn prop_edit_cycle_selector(&mut self, forward: bool) {
        if let Some(dialog) = &mut self.prop_dialog {
            if !dialog.editing || dialog.active_tab == PropDialogTab::Actions {
                return;
            }
            let _ = cycle_prop_edit_selector(dialog, forward);
        }
    }

    fn submit_selected_prop_edit(&mut self) -> Result<()> {
        if self
            .prop_dialog
            .as_ref()
            .is_some_and(|dialog| dialog.active_tab == PropDialogTab::Actions)
        {
            return self.submit_selected_action_params_edit();
        }
        let (device_did, account_uid, index, prop, value) = {
            let dialog = self
                .prop_dialog
                .as_ref()
                .ok_or_else(|| anyhow!("property dialog is not open"))?;
            if !dialog.editing {
                bail!("当前未处于编辑状态");
            }
            let item = dialog
                .items
                .get(dialog.selected)
                .ok_or_else(|| anyhow!("未选中可编辑属性"))?;
            if !item.prop.writable {
                bail!("当前属性为只读");
            }
            let value = prop_edit_selector_value(dialog, item)
                .or_else(|| parse_prop_input_value(dialog.edit_buffer.as_str()).ok())
                .ok_or_else(|| anyhow!("输入值解析失败"))?;
            (
                dialog.device_did.clone(),
                dialog.account_uid.clone(),
                dialog.selected,
                item.prop.clone(),
                value,
            )
        };
        let client = self.refresh_client_for_account_uid(account_uid.as_str(), true)?;
        let result = client.set_prop(&device_did, prop.siid, prop.piid, value.clone())?;
        if let Some(dialog) = &mut self.prop_dialog {
            if let Some(item) = dialog.items.get_mut(index) {
                item.value = value.clone();
            }
            dialog.selected = index;
            match dialog.active_tab {
                PropDialogTab::Writable => dialog.writable_selected = index,
                PropDialogTab::ReadOnly => dialog.readonly_selected = index,
                PropDialogTab::Actions => dialog.actions_selected = index,
                PropDialogTab::Logs | PropDialogTab::Statistics => {}
            }
            dialog.editing = false;
            dialog.edit_buffer.clear();
            dialog.edit_cursor = 0;
            dialog.edit_error = None;
        }
        self.log(format!(
            "set {}/{}/{} => {}",
            device_did, prop.siid, prop.piid, value
        ));
        let _ = result;
        Ok(())
    }

    fn next_action_param_focus(&mut self) {
        if let Some(dialog) = &mut self.prop_dialog {
            if dialog.active_tab != PropDialogTab::Actions || !dialog.editing {
                return;
            }
            let rows = action_param_rows_for_dialog(dialog);
            if rows.is_empty() {
                return;
            }
            let next = (dialog.writable_selected + 1) % rows.len();
            dialog.writable_selected = next;
            dialog.edit_cursor = rows[next].chars().count();
        }
    }

    fn prev_action_param_focus(&mut self) {
        if let Some(dialog) = &mut self.prop_dialog {
            if dialog.active_tab != PropDialogTab::Actions || !dialog.editing {
                return;
            }
            let rows = action_param_rows_for_dialog(dialog);
            if rows.is_empty() {
                return;
            }
            let prev = if dialog.writable_selected == 0 {
                rows.len() - 1
            } else {
                dialog.writable_selected - 1
            };
            dialog.writable_selected = prev;
            dialog.edit_cursor = rows[prev].chars().count();
        }
    }

    fn focus_action_param_row(&mut self, row: usize) {
        if let Some(dialog) = &mut self.prop_dialog {
            if dialog.active_tab != PropDialogTab::Actions || !dialog.editing {
                return;
            }
            let rows = action_param_rows_for_dialog(dialog);
            if rows.is_empty() || row >= rows.len() {
                return;
            }
            dialog.writable_selected = row;
            dialog.edit_cursor = rows[row].chars().count();
        }
    }

    fn submit_selected_action_params_edit(&mut self) -> Result<()> {
        let (
            device_did,
            account_uid,
            index,
            action,
            rows,
            labels,
            selector_options,
            selector_defaults,
        ) = {
            let dialog = self
                .prop_dialog
                .as_ref()
                .ok_or_else(|| anyhow!("property dialog is not open"))?;
            if !dialog.editing {
                bail!("当前未处于编辑状态");
            }
            let action = dialog
                .actions
                .get(dialog.selected)
                .ok_or_else(|| anyhow!("未选中可执行操作"))?;
            (
                dialog.device_did.clone(),
                dialog.account_uid.clone(),
                dialog.selected,
                action.clone(),
                action_param_rows_for_dialog(dialog),
                action.input_labels.clone(),
                action
                    .input_piids
                    .iter()
                    .enumerate()
                    .map(|(idx, _)| action_param_selector_options(dialog, action, idx))
                    .collect::<Vec<_>>(),
                action
                    .input_piids
                    .iter()
                    .enumerate()
                    .map(|(idx, _)| action_param_selector_value(dialog, action, idx, None))
                    .collect::<Vec<_>>(),
            )
        };
        let mut values = Vec::with_capacity(rows.len());
        for (idx, row) in rows.iter().enumerate() {
            if let Some(options) = selector_options.get(idx).and_then(|entry| entry.as_ref()) {
                let mut selected = selector_defaults
                    .get(idx)
                    .cloned()
                    .flatten()
                    .or_else(|| options.first().map(|(_, value)| value.clone()));
                if let Ok(parsed) = parse_prop_input_value(row.as_str()) {
                    if options.iter().any(|(_, value)| *value == parsed) {
                        selected = Some(parsed);
                    }
                }
                if let Some(value) = selected {
                    values.push(value);
                    continue;
                }
            }
            match parse_prop_input_value(row.as_str()) {
                Ok(v) => values.push(v),
                Err(error) => {
                    let label = labels
                        .get(idx)
                        .cloned()
                        .unwrap_or_else(|| format!("参数{}", idx + 1));
                    if let Some(dialog) = &mut self.prop_dialog {
                        dialog.edit_error =
                            Some(format!("参数{}({})错误: {}", idx + 1, label, error));
                        dialog.writable_selected = idx;
                        dialog.edit_cursor = dialog.edit_cursor.min(row.chars().count());
                    }
                    bail!("参数解析失败");
                }
            }
        }
        let client = self.refresh_client_for_account_uid(account_uid.as_str(), true)?;
        let result = client.action(&device_did, action.siid, action.aiid, values.as_slice())?;
        if let Some(dialog) = &mut self.prop_dialog {
            dialog.editing = false;
            dialog.edit_buffer.clear();
            dialog.edit_cursor = 0;
            dialog.edit_error = None;
            dialog.selected = index;
            dialog.actions_selected = index;
        }
        self.log(format!(
            "act {device_did} {}.{} => {}",
            action.siid, action.aiid, result
        ));
        Ok(())
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

    fn open_prop_dialog(&mut self) -> Result<()> {
        let selected = self
            .devices
            .get(self.device_index)
            .cloned()
            .ok_or_else(|| anyhow!("当前没有选中设备"))?;
        let account_uid = device_account_uid(&selected)
            .ok_or_else(|| anyhow!("设备缺少所属账号，无法读取属性"))?
            .to_string();
        let spec = load_spec(&self.home_dir, &selected.model)?;
        let spec = spec.ok_or_else(|| anyhow!("未找到设备规格，请先 sync-specs"))?;
        let props = collect_readable_props(&spec, self.language);
        let actions = extract_actions_from_spec(&spec, self.language);
        if props.is_empty() && actions.is_empty() {
            bail!("该设备规格没有可用属性或操作");
        }
        let account = self
            .accounts
            .iter()
            .find(|account| account.user.uid == account_uid)
            .cloned()
            .ok_or_else(|| anyhow!("设备所属账号未授权: {}", account_uid))?;
        let placeholder_items = props
            .iter()
            .cloned()
            .map(|prop| ToggleItem {
                prop,
                value: Value::Null,
            })
            .chain([
                raw_device_logs_item(json!({"status": "loading"})),
                raw_device_statistics_item(json!({"status": "loading"})),
            ])
            .collect::<Vec<_>>();
        let writable_indices = placeholder_items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                (item.prop.writable && !is_raw_device_json_item(item)).then_some(index)
            })
            .collect::<Vec<_>>();
        let readonly_indices = placeholder_items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                (!item.prop.writable && !is_raw_device_json_item(item)).then_some(index)
            })
            .collect::<Vec<_>>();
        let active_tab = if !actions.is_empty() {
            PropDialogTab::Actions
        } else if writable_indices.is_empty() {
            PropDialogTab::ReadOnly
        } else {
            PropDialogTab::Writable
        };
        let initial_selected = match active_tab {
            PropDialogTab::Writable => writable_indices.first().copied(),
            PropDialogTab::ReadOnly => readonly_indices.first().copied(),
            PropDialogTab::Actions => Some(0),
            PropDialogTab::Logs | PropDialogTab::Statistics => None,
        }
        .unwrap_or(0);
        let did = selected.did.clone();
        let property_cache = self.property_cache.clone();
        let (tx, rx) = mpsc::channel::<std::result::Result<Vec<ToggleItem>, String>>();
        let (raw_tx, raw_rx) = mpsc::channel::<PropDialogRefreshMessage>();
        thread::spawn(move || {
            let result = (|| -> Result<(Vec<ToggleItem>, Vec<String>, Vec<String>)> {
                let query_refs = props
                    .iter()
                    .map(|prop| (did.as_str(), prop.siid, prop.piid))
                    .collect::<Vec<_>>();

                // Try to get from cache first
                let mut cached_values: Vec<Option<Value>> = query_refs
                    .iter()
                    .map(|(_did, siid, piid)| property_cache.get_property(&did, *siid, *piid))
                    .collect();

                // If any values are missing from cache, fetch from API and update cache
                let has_missing = cached_values.iter().any(|v| v.is_none());
                if has_missing {
                    let client = MicoClient::new(&account)?;
                    let raw_values = client.get_props_batch(&query_refs)?;
                    let list = raw_values
                        .as_array()
                        .ok_or_else(|| anyhow!("property loading did not return list"))?;

                    // Update cache and cached_values
                    let mut cache_update: std::collections::HashMap<(i64, i64), Value> =
                        std::collections::HashMap::new();
                    for (idx, (_did, siid, piid)) in query_refs.iter().enumerate() {
                        let value = list.get(idx).cloned().unwrap_or(Value::Null);
                        cached_values[idx] = Some(value.clone());
                        cache_update.insert((*siid, *piid), value);
                    }
                    property_cache.set_device_properties(did.clone(), cache_update);
                }

                let mijia_prop_keys = props.iter().map(mijia_prop_key).collect::<Vec<_>>();
                let mijia_statistics_keys = props
                    .iter()
                    .filter_map(mijia_statistics_key)
                    .collect::<Vec<_>>();
                let items = props
                    .into_iter()
                    .enumerate()
                    .map(|(index, prop)| ToggleItem {
                        prop,
                        value: cached_values
                            .get(index)
                            .and_then(|v| v.as_ref())
                            .and_then(extract_prop_value)
                            .unwrap_or(Value::Null),
                    })
                    .collect::<Vec<_>>();
                Ok((items, mijia_prop_keys, mijia_statistics_keys))
            })();
            let (mut items, mijia_prop_keys, mijia_statistics_keys) = match result {
                Ok(result) => result,
                Err(error) => {
                    let _ = tx.send(Err(error.to_string()));
                    let _ = raw_tx.send(PropDialogRefreshMessage::Finished);
                    return;
                }
            };
            let logs_index = items.len();
            let statistics_index = logs_index + 1;
            items.push(raw_device_logs_item(json!({"status": "loading"})));
            items.push(raw_device_statistics_item(json!({"status": "loading"})));
            if tx.send(Ok(items)).is_err() {
                return;
            }
            let result = (|| -> Result<()> {
                let logs_value =
                    load_mijia_device_logs_json(&account, did.as_str(), mijia_prop_keys.as_slice());
                if raw_tx
                    .send(PropDialogRefreshMessage::Raw(vec![(
                        logs_index, logs_value,
                    )]))
                    .is_err()
                {
                    return Ok(());
                }
                let statistics_value = load_mijia_device_statistics_json(
                    &account,
                    did.as_str(),
                    mijia_statistics_keys.as_slice(),
                );
                if raw_tx
                    .send(PropDialogRefreshMessage::Raw(vec![(
                        statistics_index,
                        statistics_value,
                    )]))
                    .is_err()
                {
                    return Ok(());
                }
                Ok(())
            })();
            match result {
                Ok(()) => {
                    let _ = raw_tx.send(PropDialogRefreshMessage::Finished);
                }
                Err(error) => {
                    let _ = raw_tx.send(PropDialogRefreshMessage::Error(error.to_string()));
                }
            }
        });
        self.account_action_dialog = None;
        self.prop_dialog = Some(PropDialog {
            device_did: selected.did.clone(),
            device_name: selected.name.clone(),
            account_uid,
            items: placeholder_items,
            selected: initial_selected,
            active_tab,
            writable_selected: writable_indices.first().copied().unwrap_or(0),
            readonly_selected: readonly_indices.first().copied().unwrap_or(0),
            actions,
            actions_selected: 0,
            writable_list_state: ListState::default(),
            readonly_list_state: ListState::default(),
            actions_list_state: ListState::default(),
            loading: true,
            loading_rx: Some(rx),
            status: None,
            editing: false,
            edit_buffer: String::new(),
            edit_cursor: 0,
            edit_error: None,
            refreshing: false,
            refresh_rx: Some(raw_rx),
        });
        self.log(format!(
            "opened property dialog for {} ({})",
            selected.name, selected.did
        ));
        Ok(())
    }

    fn request_prop_dialog_refresh(&mut self) {
        self.request_prop_dialog_refresh_inner(false);
    }

    fn request_prop_dialog_refresh_allow_editing(&mut self) {
        self.request_prop_dialog_refresh_inner(true);
    }

    fn request_prop_dialog_refresh_inner(&mut self, allow_editing: bool) {
        let (
            device_did,
            account,
            queries,
            mijia_prop_keys,
            mijia_statistics_keys,
            operation_record_date_filter,
            statistics_period,
            statistics_query,
            statistics_selected_key,
            logs_index,
            statistics_index,
        ) = match &self.prop_dialog {
            Some(dialog)
                if !dialog.loading
                    && dialog.status.is_none()
                    && !dialog.items.is_empty()
                    && (allow_editing || !dialog.editing)
                    && !dialog.refreshing
                    && dialog.refresh_rx.is_none() =>
            {
                let Some(account) = self
                    .accounts
                    .iter()
                    .find(|account| account.user.uid == dialog.account_uid)
                    .cloned()
                else {
                    return;
                };
                let statistics_period = statistics_period_for_dialog(dialog);
                let statistics_query =
                    statistics_query_for_value(raw_device_statistics_value(dialog));
                (
                    dialog.device_did.clone(),
                    account,
                    dialog
                        .items
                        .iter()
                        .enumerate()
                        .filter_map(|(index, item)| {
                            (!is_raw_device_json_item(item)).then_some((
                                index,
                                item.prop.siid,
                                item.prop.piid,
                            ))
                        })
                        .collect::<Vec<_>>(),
                    dialog
                        .items
                        .iter()
                        .filter(|item| !is_raw_device_json_item(item))
                        .map(|item| mijia_prop_key(&item.prop))
                        .collect::<Vec<_>>(),
                    dialog
                        .items
                        .iter()
                        .filter(|item| !is_raw_device_json_item(item))
                        .filter_map(|item| mijia_statistics_key(&item.prop))
                        .collect::<Vec<_>>(),
                    operation_record_date_filter_for_dialog(dialog),
                    statistics_period,
                    statistics_query,
                    statistics_selected_key(dialog),
                    raw_device_logs_index(dialog),
                    raw_device_statistics_index(dialog),
                )
            }
            _ => return,
        };
        let property_cache = self.property_cache.clone();
        let (tx, rx) = mpsc::channel::<PropDialogRefreshMessage>();
        if let Some(dialog) = &mut self.prop_dialog {
            dialog.refreshing = !queries.is_empty();
            if let Some(index) = logs_index {
                if let Some(item) = dialog.items.get_mut(index) {
                    set_raw_device_loading_status(&mut item.value, true);
                }
            }
            if let Some(index) = statistics_index {
                if let Some(item) = dialog.items.get_mut(index) {
                    set_raw_device_loading_status(&mut item.value, true);
                }
            }
            dialog.refresh_rx = Some(rx);
        }
        thread::spawn(move || {
            let result = (|| -> Result<()> {
                if !queries.is_empty() {
                    let query_refs = queries
                        .iter()
                        .map(|(_, siid, piid)| (device_did.as_str(), *siid, *piid))
                        .collect::<Vec<_>>();
                    let client = MicoClient::new(&account)?;
                    let values = client.get_props_batch(&query_refs)?;
                    let list = values
                        .as_array()
                        .ok_or_else(|| anyhow!("property refresh did not return list"))?;

                    // Update cache with fresh values
                    let mut cache_update: std::collections::HashMap<(i64, i64), Value> =
                        std::collections::HashMap::new();
                    for (idx, (_, siid, piid)) in queries.iter().enumerate() {
                        let value = list.get(idx).cloned().unwrap_or(Value::Null);
                        cache_update.insert((*siid, *piid), value);
                    }
                    property_cache.set_device_properties(device_did.clone(), cache_update);

                    let updates = queries
                        .iter()
                        .enumerate()
                        .map(|(idx, (item_index, _, _))| {
                            (
                                *item_index,
                                list.get(idx)
                                    .and_then(extract_prop_value)
                                    .unwrap_or(Value::Null),
                            )
                        })
                        .collect::<Vec<_>>();
                    if tx.send(PropDialogRefreshMessage::Props(updates)).is_err() {
                        return Ok(());
                    }
                }

                if let Some(index) = logs_index {
                    let value = load_mijia_device_logs_json_with_query(
                        &account,
                        device_did.as_str(),
                        mijia_prop_keys.as_slice(),
                        operation_record_date_filter,
                    );
                    if tx
                        .send(PropDialogRefreshMessage::Raw(vec![(index, value)]))
                        .is_err()
                    {
                        return Ok(());
                    }
                }
                if let Some(index) = statistics_index {
                    let value = load_mijia_device_statistics_json_with_query(
                        &account,
                        device_did.as_str(),
                        mijia_statistics_keys.as_slice(),
                        statistics_period,
                        statistics_query,
                        statistics_selected_key,
                    );
                    if tx
                        .send(PropDialogRefreshMessage::Raw(vec![(index, value)]))
                        .is_err()
                    {
                        return Ok(());
                    }
                }

                Ok(())
            })();
            match result {
                Ok(()) => {
                    let _ = tx.send(PropDialogRefreshMessage::Finished);
                }
                Err(error) => {
                    let _ = tx.send(PropDialogRefreshMessage::Error(error.to_string()));
                }
            }
        });
    }

    fn process_prop_dialog_loading(&mut self) {
        let recv = match self.prop_dialog.as_ref() {
            Some(dialog) if dialog.loading => match dialog.loading_rx.as_ref() {
                Some(rx) => match rx.try_recv() {
                    Ok(result) => Some(result),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        Some(Err("property loading worker disconnected".to_string()))
                    }
                },
                None => None,
            },
            _ => None,
        };
        if let Some(result) = recv {
            if let Some(dialog) = &mut self.prop_dialog {
                dialog.loading = false;
                dialog.loading_rx = None;
                match result {
                    Ok(items) => {
                        dialog.refreshing = false;
                        let previous_selected = dialog.selected;
                        let previous_writable_selected = dialog.writable_selected;
                        let previous_readonly_selected = dialog.readonly_selected;
                        let previous_actions_selected = dialog.actions_selected;
                        dialog.items = items;
                        let writable_indices =
                            prop_dialog_indices_for_tab(dialog, PropDialogTab::Writable);
                        let readonly_indices =
                            prop_dialog_indices_for_tab(dialog, PropDialogTab::ReadOnly);
                        let actions_indices =
                            prop_dialog_indices_for_tab(dialog, PropDialogTab::Actions);
                        let logs_indices = prop_dialog_indices_for_tab(dialog, PropDialogTab::Logs);
                        let statistics_indices =
                            prop_dialog_indices_for_tab(dialog, PropDialogTab::Statistics);
                        let preserve_index = |indices: &[usize], preferred: usize| {
                            indices
                                .iter()
                                .copied()
                                .find(|index| *index == preferred)
                                .or_else(|| indices.first().copied())
                                .unwrap_or(0)
                        };
                        dialog.writable_selected =
                            preserve_index(&writable_indices, previous_writable_selected);
                        dialog.readonly_selected =
                            preserve_index(&readonly_indices, previous_readonly_selected);
                        dialog.actions_selected =
                            preserve_index(&actions_indices, previous_actions_selected);
                        dialog.selected = match dialog.active_tab {
                            PropDialogTab::Writable => {
                                preserve_index(&writable_indices, previous_selected)
                            }
                            PropDialogTab::ReadOnly => {
                                preserve_index(&readonly_indices, previous_selected)
                            }
                            PropDialogTab::Actions => {
                                preserve_index(&actions_indices, previous_selected)
                            }
                            PropDialogTab::Logs => preserve_index(&logs_indices, previous_selected),
                            PropDialogTab::Statistics => {
                                preserve_index(&statistics_indices, previous_selected)
                            }
                        };
                        match dialog.active_tab {
                            PropDialogTab::Writable => dialog.writable_selected = dialog.selected,
                            PropDialogTab::ReadOnly => dialog.readonly_selected = dialog.selected,
                            PropDialogTab::Actions => dialog.actions_selected = dialog.selected,
                            PropDialogTab::Logs | PropDialogTab::Statistics => {}
                        }
                        dialog.status = None;
                    }
                    Err(error) => {
                        dialog.refreshing = false;
                        dialog.items.clear();
                        dialog.status = Some(error.clone());
                        self.log(format!("property dialog loading failed: {error}"));
                    }
                }
            }
        }

        let (mut refresh_messages, refresh_disconnected) = match self.prop_dialog.as_ref() {
            Some(dialog) if dialog.refreshing || dialog.refresh_rx.is_some() => {
                let Some(rx) = dialog.refresh_rx.as_ref() else {
                    return;
                };
                let mut messages = Vec::new();
                let mut disconnected = false;
                loop {
                    match rx.try_recv() {
                        Ok(message) => messages.push(message),
                        Err(mpsc::TryRecvError::Empty) => break,
                        Err(mpsc::TryRecvError::Disconnected) => {
                            disconnected = true;
                            break;
                        }
                    }
                }
                (messages, disconnected)
            }
            _ => return,
        };
        if refresh_messages.is_empty() {
            if refresh_disconnected {
                refresh_messages.push(PropDialogRefreshMessage::Error(
                    "property refresh worker disconnected".to_string(),
                ));
            } else {
                return;
            }
        }
        let mut error_to_log = None;
        if let Some(dialog) = &mut self.prop_dialog {
            let mut close_refresh = false;
            for message in refresh_messages {
                match message {
                    PropDialogRefreshMessage::Props(updates) => {
                        for (index, value) in updates {
                            if let Some(item) = dialog.items.get_mut(index) {
                                item.value = value;
                            }
                        }
                        dialog.refreshing = false;
                    }
                    PropDialogRefreshMessage::Raw(updates) => {
                        for (index, value) in updates {
                            if let Some(item) = dialog.items.get_mut(index) {
                                item.value = value;
                            }
                        }
                    }
                    PropDialogRefreshMessage::Finished => {
                        close_refresh = true;
                    }
                    PropDialogRefreshMessage::Error(error) => {
                        if let Some(value) = operation_record_raw_value_mut(dialog) {
                            set_raw_device_loading_status(value, false);
                            set_all_operation_record_requests_loading(value, false);
                        }
                        if let Some(index) = raw_device_statistics_index(dialog) {
                            if let Some(item) = dialog.items.get_mut(index) {
                                set_raw_device_loading_status(&mut item.value, false);
                            }
                        }
                        error_to_log = Some(error);
                        close_refresh = true;
                    }
                }
            }
            if close_refresh || refresh_disconnected {
                dialog.refreshing = false;
                dialog.refresh_rx = None;
            }
        }
        if let Some(error) = error_to_log {
            self.log(format!("property dialog refresh failed: {error}"));
        }
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

pub(crate) fn extract_auth_url_from_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if let Some(auth_url) = trimmed.strip_prefix("AUTH_URL ") {
        return Some(auth_url.trim().to_string());
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return Some(trimmed.to_string());
    }

    let value = serde_json::from_str::<Value>(trimmed).ok()?;
    let event_type = value
        .get("type")
        .or_else(|| value.get("kind"))
        .and_then(Value::as_str)?;
    if event_type != "authUrlPrinted" {
        return None;
    }
    value
        .get("url")
        .and_then(Value::as_str)
        .map(|url| url.to_string())
}

fn copy_text_to_clipboard(text: &str) -> Result<()> {
    if let Some(path) = std::env::var_os("MIT_TEST_CLIPBOARD_FILE") {
        fs::write(path, text)?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        let mut child = std::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()?;
        if let Some(stdin) = child.stdin.as_mut() {
            use std::io::Write;
            stdin.write_all(text.as_bytes())?;
        }
        let status = child.wait()?;
        if status.success() {
            Ok(())
        } else {
            bail!("pbcopy exited with status {status}")
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        bail!("clipboard copy is not supported on this platform")
    }
}

#[cfg(not(test))]
pub(crate) fn open_url_in_browser(url: &str) -> Result<()> {
    if std::env::var("MIT_DISABLE_BROWSER_OPEN")
        .ok()
        .is_some_and(|value| {
            let value = value.trim();
            !value.is_empty() && value != "0" && !value.eq_ignore_ascii_case("false")
        })
    {
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(url);
        command
    };

    #[cfg(target_os = "linux")]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };

    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    };

    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        bail!("browser launcher exited with status {status}");
    }
}

fn cloud_mips_runtime_key(groups: &[(AuthAccount, Vec<String>)]) -> String {
    let mut entries = groups
        .iter()
        .map(|(account, dids)| {
            let mut dids = dids.clone();
            dids.sort();
            format!(
                "{}:{}:{}:{}",
                account.user.uid,
                account.uuid,
                account.access_token,
                dids.join(",")
            )
        })
        .collect::<Vec<_>>();
    entries.sort();
    entries.join("|")
}

fn cloud_mips_disabled_by_user() -> bool {
    std::env::var("MIT_DISABLE_CLOUD_MIPS")
        .ok()
        .is_some_and(|value| !value.trim().is_empty())
}

fn cloud_mips_disabled() -> bool {
    if cloud_mips_disabled_by_user() {
        return true;
    }
    cfg!(test) && std::env::var("MIT_ENABLE_CLOUD_MIPS_IN_TESTS").is_err()
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
}

fn configure_textarea_style(textarea: &mut TextArea<'_>) {
    textarea.set_cursor_line_style(TextAreaStyle::default());
    textarea.set_wrap_mode(WrapMode::WordOrGlyph);
}

fn single_line_textarea(input: &str, cursor: usize, focused: bool) -> TextArea<'static> {
    let mut textarea = TextArea::from([input.to_string()]);
    configure_textarea_style(&mut textarea);
    if !focused {
        textarea.set_cursor_style(TextAreaStyle::default());
    }
    textarea.move_cursor(CursorMove::Head);
    for _ in 0..cursor.min(input.chars().count()) {
        textarea.move_cursor(CursorMove::Forward);
    }
    textarea
}

fn render_textarea_widget(
    textarea: &TextArea<'_>,
    area: ratatui::layout::Rect,
    buffer: &mut ratatui::buffer::Buffer,
) {
    let core_area = ratatui_core::layout::Rect::new(area.x, area.y, area.width, area.height);
    let scratch = render_textarea_to_scratch(textarea, area);

    for y in 0..area.height {
        for x in 0..area.width {
            let src = &scratch[(core_area.x + x, core_area.y + y)];
            let dst = &mut buffer[(area.x + x, area.y + y)];
            dst.set_symbol(src.symbol());
            dst.fg = core_color_to_ratatui(src.fg);
            dst.bg = core_color_to_ratatui(src.bg);
            dst.modifier = ratatui::style::Modifier::from_bits_retain(src.modifier.bits());
            dst.skip = src.skip;
        }
    }
}

fn render_textarea_to_scratch(
    textarea: &TextArea<'_>,
    area: ratatui::layout::Rect,
) -> ratatui_core::buffer::Buffer {
    let core_area = ratatui_core::layout::Rect::new(area.x, area.y, area.width, area.height);
    let mut scratch = ratatui_core::buffer::Buffer::empty(core_area);
    TextAreaWidget::render(textarea, core_area, &mut scratch);
    scratch
}

pub(in crate::tui) fn rendered_textarea_lines(
    input: &str,
    cursor: usize,
    focused: bool,
    area: Rect,
) -> Vec<String> {
    if area.width == 0 || area.height == 0 {
        return Vec::new();
    }
    let textarea = single_line_textarea(input, cursor, focused);
    let scratch = render_textarea_to_scratch(&textarea, area);
    let core_area = ratatui_core::layout::Rect::new(area.x, area.y, area.width, area.height);
    (0..area.height)
        .map(|y| {
            let mut line = String::new();
            for x in 0..area.width {
                line.push_str(
                    scratch[(core_area.x + x, core_area.y + y)]
                        .symbol()
                        .as_ref(),
                );
            }
            line.trim_end_matches(' ').to_string()
        })
        .collect()
}

fn rendered_textarea_cursor_cell(input: &str, cursor: usize, area: Rect) -> Option<(u16, u16)> {
    if area.width == 0 || area.height == 0 {
        return None;
    }
    let textarea = single_line_textarea(input, cursor, true);
    let scratch = render_textarea_to_scratch(&textarea, area);
    let core_area = ratatui_core::layout::Rect::new(area.x, area.y, area.width, area.height);
    for y in 0..area.height {
        for x in 0..area.width {
            if scratch[(core_area.x + x, core_area.y + y)]
                .modifier
                .contains(ratatui_core::style::Modifier::REVERSED)
            {
                return Some((x, y));
            }
        }
    }
    None
}

pub(in crate::tui) fn textarea_cursor_for_mouse(
    input: &str,
    area: Rect,
    column: u16,
    row: u16,
) -> usize {
    if area.width == 0 || area.height == 0 {
        return 0;
    }
    let target_col = column
        .saturating_sub(area.x)
        .min(area.width.saturating_sub(1));
    let target_row = row
        .saturating_sub(area.y)
        .min(area.height.saturating_sub(1));
    let mut best = 0usize;
    for idx in 0..=input.chars().count() {
        let Some((cursor_col, cursor_row)) = rendered_textarea_cursor_cell(input, idx, area) else {
            continue;
        };
        if cursor_row < target_row || (cursor_row == target_row && cursor_col <= target_col) {
            best = idx;
        } else {
            break;
        }
    }
    best
}

pub(in crate::tui) fn prop_edit_textarea_area(dialog: &PropDialog, editor_area: Rect) -> Rect {
    let textarea_height = textarea_visual_height(dialog.edit_buffer.as_str(), editor_area.width)
        .max(2)
        .min(editor_area.height.max(1));
    Rect::new(
        editor_area.x,
        editor_area.y,
        editor_area.width,
        textarea_height,
    )
}

fn push_message_dialog_popup(terminal_area: Rect) -> Rect {
    centered_rect(74, 46, terminal_area)
}

pub(in crate::tui) fn push_message_textarea_area(terminal_area: Rect, input: &str) -> Rect {
    push_message_dialog_sections(terminal_area, input)[1]
}

pub(in crate::tui) fn push_message_command_area(terminal_area: Rect, input: &str) -> Rect {
    let popup = push_message_dialog_popup(terminal_area);
    let _ = input;
    Rect::new(
        popup.x.saturating_add(1),
        popup.y.saturating_add(popup.height.saturating_sub(2)),
        popup.width.saturating_sub(2),
        1,
    )
}

pub(in crate::tui) fn push_message_cli_preview_line(
    uid: &str,
    input: &str,
    lang: Language,
) -> String {
    format!(
        "{}: {}",
        lang_str(lang, "CLI 命令", "CLI Command"),
        format_preview_push_command(uid, input)
    )
}

fn push_message_dialog_sections(terminal_area: Rect, input: &str) -> [Rect; 3] {
    let popup = push_message_dialog_popup(terminal_area);
    let inner = ratatui::layout::Rect::new(
        popup.x.saturating_add(1),
        popup.y.saturating_add(1),
        popup.width.saturating_sub(2),
        popup.height.saturating_sub(2),
    );
    let textarea_height =
        textarea_visual_height(input, inner.width).min(inner.height.saturating_sub(3));
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(textarea_height.max(1)),
            Constraint::Min(1),
        ])
        .split(inner);
    [sections[0], sections[1], sections[2]]
}

fn footer_render_area(area: Rect) -> Rect {
    if area.height == 0 || area.width == 0 {
        Rect::new(area.x, area.y, area.width, 0)
    } else if area.height >= 3 {
        Rect::new(area.x, area.y.saturating_add(1), area.width, 1)
    } else {
        Rect::new(area.x, area.y, area.width, 1)
    }
}

fn split_main_layout(area: Rect) -> [Rect; 4] {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(STATUS_BAR_MARGIN_TOP),
            Constraint::Length(2),
        ])
        .areas(area)
}

pub(in crate::tui) fn main_content_area(area: Rect) -> Rect {
    split_main_layout(area)[1]
}

pub(in crate::tui) fn fullscreen_dialog_inner_area(area: Rect) -> Rect {
    let base = Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(1),
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    );
    let content = main_content_area(area);
    let max_bottom = content.y.saturating_add(content.height);
    let clipped_height = base.height.min(max_bottom.saturating_sub(base.y));
    Rect::new(base.x, base.y, base.width, clipped_height)
}

fn core_color_to_ratatui(color: ratatui_core::style::Color) -> Color {
    match color {
        ratatui_core::style::Color::Reset => Color::Reset,
        ratatui_core::style::Color::Black => Color::Black,
        ratatui_core::style::Color::Red => Color::Red,
        ratatui_core::style::Color::Green => Color::Green,
        ratatui_core::style::Color::Yellow => Color::Yellow,
        ratatui_core::style::Color::Blue => Color::Blue,
        ratatui_core::style::Color::Magenta => Color::Magenta,
        ratatui_core::style::Color::Cyan => Color::Cyan,
        ratatui_core::style::Color::Gray => Color::Gray,
        ratatui_core::style::Color::DarkGray => Color::DarkGray,
        ratatui_core::style::Color::LightRed => Color::LightRed,
        ratatui_core::style::Color::LightGreen => Color::LightGreen,
        ratatui_core::style::Color::LightYellow => Color::LightYellow,
        ratatui_core::style::Color::LightBlue => Color::LightBlue,
        ratatui_core::style::Color::LightMagenta => Color::LightMagenta,
        ratatui_core::style::Color::LightCyan => Color::LightCyan,
        ratatui_core::style::Color::White => Color::White,
        ratatui_core::style::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
        ratatui_core::style::Color::Indexed(n) => Color::Indexed(n),
    }
}

fn apply_single_line_textarea_key(
    input: &mut String,
    cursor: &mut usize,
    key: crossterm::event::KeyEvent,
) {
    let mut textarea = single_line_textarea(input.as_str(), *cursor, true);
    let _ = textarea.input(textarea_input_from_key_event(key));
    let col = textarea.cursor().1;
    *input = textarea.lines().first().cloned().unwrap_or_default();
    *cursor = col.min(input.chars().count());
}

fn apply_action_param_row_key(dialog: &mut PropDialog, key: crossterm::event::KeyEvent) {
    let mut rows = action_param_rows_for_dialog(dialog);
    if rows.is_empty() {
        return;
    }
    let focus = dialog.writable_selected.min(rows.len() - 1);
    let Some(action) = dialog.actions.get(dialog.selected) else {
        return;
    };
    if let Some(options) = action_param_selector_options(dialog, action, focus) {
        let forward = matches!(key.code, KeyCode::Right | KeyCode::Tab);
        let backward = matches!(key.code, KeyCode::Left | KeyCode::BackTab);
        if forward || backward {
            let index = action_param_selector_index(
                dialog,
                action,
                focus,
                options.as_slice(),
                Some(rows[focus].as_str()),
            );
            let next = if forward {
                (index + 1) % options.len()
            } else if index == 0 {
                options.len() - 1
            } else {
                index - 1
            };
            rows[focus] =
                serde_json::to_string(&options[next].1).unwrap_or_else(|_| "null".to_string());
            set_action_param_rows_in_dialog(dialog, rows.as_slice());
            dialog.edit_cursor = rows[focus].chars().count();
            return;
        }
        if matches!(
            key.code,
            KeyCode::Char(_) | KeyCode::Backspace | KeyCode::Delete | KeyCode::Enter
        ) {
            return;
        }
    }
    let mut textarea = single_line_textarea(
        rows[focus].as_str(),
        dialog.edit_cursor.min(rows[focus].chars().count()),
        true,
    );
    let _ = textarea.input(textarea_input_from_key_event(key));
    let col = textarea.cursor().1;
    rows[focus] = textarea.lines().first().cloned().unwrap_or_default();
    set_action_param_rows_in_dialog(dialog, rows.as_slice());
    dialog.edit_cursor = col.min(rows[focus].chars().count());
}

fn textarea_input_from_key_event(key: crossterm::event::KeyEvent) -> TextAreaInput {
    let textarea_key = match key.code {
        KeyCode::Char(ch) => TextAreaKey::Char(ch),
        KeyCode::Backspace => TextAreaKey::Backspace,
        KeyCode::Enter => TextAreaKey::Enter,
        KeyCode::Left => TextAreaKey::Left,
        KeyCode::Right => TextAreaKey::Right,
        KeyCode::Up => TextAreaKey::Up,
        KeyCode::Down => TextAreaKey::Down,
        KeyCode::Tab => TextAreaKey::Tab,
        KeyCode::Delete => TextAreaKey::Delete,
        KeyCode::Home => TextAreaKey::Home,
        KeyCode::End => TextAreaKey::End,
        KeyCode::PageUp => TextAreaKey::PageUp,
        KeyCode::PageDown => TextAreaKey::PageDown,
        KeyCode::Esc => TextAreaKey::Esc,
        KeyCode::F(n) => TextAreaKey::F(n),
        _ => TextAreaKey::Null,
    };
    TextAreaInput {
        key: textarea_key,
        ctrl: key.modifiers.contains(KeyModifiers::CONTROL),
        alt: key.modifiers.contains(KeyModifiers::ALT),
        shift: key.modifiers.contains(KeyModifiers::SHIFT),
    }
}

fn action_param_rows_for_dialog(dialog: &PropDialog) -> Vec<String> {
    let Some(action) = dialog.actions.get(dialog.selected) else {
        return Vec::new();
    };
    let expected = action.input_labels.len();
    if expected == 0 {
        return Vec::new();
    }
    let mut rows = dialog
        .edit_buffer
        .split('\n')
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    if rows.len() < expected {
        rows.resize(expected, String::new());
    } else if rows.len() > expected {
        rows.truncate(expected);
    }
    rows
}

fn set_action_param_rows_in_dialog(dialog: &mut PropDialog, rows: &[String]) {
    dialog.edit_buffer = rows.join("\n");
}

fn action_param_selector_options(
    dialog: &PropDialog,
    action: &ActionItem,
    index: usize,
) -> Option<Vec<(String, Value)>> {
    if let Some(options) = action
        .input_props
        .get(index)
        .and_then(selector_options_for_prop)
    {
        return Some(options);
    }
    let piid = *action.input_piids.get(index)?;
    let item = dialog
        .items
        .iter()
        .find(|item| item.prop.siid == action.siid && item.prop.piid == piid)?;
    prop_edit_selector_options(item)
}

fn action_param_selector_index(
    dialog: &PropDialog,
    action: &ActionItem,
    index: usize,
    options: &[(String, Value)],
    row: Option<&str>,
) -> usize {
    if let Some(row) = row {
        if let Ok(parsed) = parse_prop_input_value(row) {
            if let Some(found) = options.iter().position(|(_, value)| *value == parsed) {
                return found;
            }
        }
    }
    let piid = action.input_piids.get(index).copied().unwrap_or_default();
    let current = dialog
        .items
        .iter()
        .find(|item| item.prop.siid == action.siid && item.prop.piid == piid)
        .map(|item| item.value.clone());
    current
        .and_then(|current| options.iter().position(|(_, value)| *value == current))
        .unwrap_or(0)
}

fn action_param_selector_value(
    dialog: &PropDialog,
    action: &ActionItem,
    index: usize,
    row: Option<&str>,
) -> Option<Value> {
    let options = action_param_selector_options(dialog, action, index)?;
    let idx = action_param_selector_index(dialog, action, index, options.as_slice(), row);
    options.get(idx).map(|(_, value)| value.clone())
}

fn action_param_default_row(dialog: &PropDialog, action: &ActionItem, index: usize) -> String {
    if let Some(value) = action_param_selector_value(dialog, action, index, None) {
        return serde_json::to_string(&value).unwrap_or_else(|_| "null".to_string());
    }
    String::new()
}

fn selector_option_line(
    options: &[(String, Value)],
    active_value: Option<&Value>,
    base_style: Style,
) -> Line<'static> {
    let mut spans = Vec::new();
    for (idx, (label, value)) in options.iter().enumerate() {
        if idx > 0 {
            spans.push(Span::styled(", ".to_string(), base_style));
        }
        let style = if active_value.is_some_and(|active| *active == *value) {
            base_style.fg(Color::Green).add_modifier(Modifier::BOLD)
        } else {
            base_style
        };
        spans.push(Span::styled(format!("[{label}]"), style));
    }
    Line::from(spans)
}

fn selector_option_index_for_offset(options: &[(String, Value)], offset: u16) -> Option<usize> {
    let mut column = 0_usize;
    let offset = offset as usize;
    for (idx, (label, _)) in options.iter().enumerate() {
        if idx > 0 {
            column += 2;
        }
        let option_text = format!("[{label}]");
        let width = display_width(option_text.as_str()) as usize;
        if offset >= column && offset < column + width {
            return Some(idx);
        }
        column += width;
    }
    None
}

fn selector_option_value_at_offset(options: &[(String, Value)], offset: u16) -> Option<Value> {
    let index = selector_option_index_for_offset(options, offset)?;
    options.get(index).map(|(_, value)| value.clone())
}

fn action_param_selector_line(
    dialog: &PropDialog,
    action: &ActionItem,
    index: usize,
    row: Option<&str>,
    base_style: Style,
) -> Option<Line<'static>> {
    let options = action_param_selector_options(dialog, action, index)?;
    let active_value = action_param_selector_value(dialog, action, index, row);
    Some(selector_option_line(
        options.as_slice(),
        active_value.as_ref(),
        base_style,
    ))
}

fn selector_options_for_prop(prop: &PropItem) -> Option<Vec<(String, Value)>> {
    if prop.format == "bool" {
        return Some(vec![
            ("true".to_string(), Value::Bool(true)),
            ("false".to_string(), Value::Bool(false)),
        ]);
    }
    if prop.value_options.is_empty() {
        return None;
    }
    Some(
        prop.value_options
            .iter()
            .map(|option| {
                let label = if option.label.trim().is_empty() {
                    format_prop_value_for_dialog(&option.value)
                } else {
                    option.label.clone()
                };
                (label, option.value.clone())
            })
            .collect::<Vec<_>>(),
    )
}

fn prop_edit_selector_options(item: &ToggleItem) -> Option<Vec<(String, Value)>> {
    selector_options_for_prop(&item.prop)
}

fn prop_edit_selector_index(item: &ToggleItem, options: &[(String, Value)]) -> usize {
    options
        .iter()
        .position(|(_, value)| *value == item.value)
        .unwrap_or(0)
}

fn prop_edit_selector_is_active(dialog: &PropDialog) -> bool {
    if dialog.active_tab == PropDialogTab::Actions || !dialog.editing {
        return false;
    }
    dialog
        .items
        .get(dialog.selected)
        .and_then(prop_edit_selector_options)
        .is_some()
}

fn prop_edit_selector_value(dialog: &PropDialog, item: &ToggleItem) -> Option<Value> {
    let options = prop_edit_selector_options(item)?;
    if let Ok(current) = parse_prop_input_value(dialog.edit_buffer.as_str()) {
        if let Some((_, value)) = options.iter().find(|(_, value)| *value == current) {
            return Some(value.clone());
        }
    }
    let index = prop_edit_selector_index(item, options.as_slice());
    options.get(index).map(|(_, value)| value.clone())
}

fn cycle_prop_edit_selector(dialog: &mut PropDialog, forward: bool) -> bool {
    let Some(item) = dialog.items.get(dialog.selected) else {
        return false;
    };
    let Some(options) = prop_edit_selector_options(item) else {
        return false;
    };
    let current_index = prop_edit_selector_value(dialog, item)
        .and_then(|value| options.iter().position(|(_, option)| *option == value))
        .unwrap_or_else(|| prop_edit_selector_index(item, options.as_slice()));
    let next_index = if forward {
        (current_index + 1) % options.len()
    } else if current_index == 0 {
        options.len() - 1
    } else {
        current_index - 1
    };
    dialog.edit_buffer =
        serde_json::to_string(&options[next_index].1).unwrap_or_else(|_| "null".to_string());
    dialog.edit_cursor = dialog.edit_buffer.chars().count();
    dialog.edit_error = None;
    true
}

fn prop_edit_selector_options_text(dialog: &PropDialog) -> Option<String> {
    let item = dialog.items.get(dialog.selected)?;
    let options = prop_edit_selector_options(item)?;
    Some(
        options
            .iter()
            .map(|(label, _)| format!("[{label}]"))
            .collect::<Vec<_>>()
            .join(", "),
    )
}

fn prop_edit_selector_line(dialog: &PropDialog, base_style: Style) -> Option<Line<'static>> {
    let item = dialog.items.get(dialog.selected)?;
    let options = prop_edit_selector_options(item)?;
    let active_value = prop_edit_selector_value(dialog, item);
    Some(selector_option_line(
        options.as_slice(),
        active_value.as_ref(),
        base_style,
    ))
}

fn visible_prop_dialog_tabs(dialog: &PropDialog) -> Vec<PropDialogTab> {
    let mut tabs = Vec::new();
    if !dialog.actions.is_empty() {
        tabs.push(PropDialogTab::Actions);
    }
    if !prop_dialog_indices_for_tab(dialog, PropDialogTab::Writable).is_empty() {
        tabs.push(PropDialogTab::Writable);
    }
    if !prop_dialog_indices_for_tab(dialog, PropDialogTab::ReadOnly).is_empty() {
        tabs.push(PropDialogTab::ReadOnly);
    }
    if !prop_dialog_indices_for_tab(dialog, PropDialogTab::Logs).is_empty() {
        tabs.push(PropDialogTab::Logs);
    }
    if !prop_dialog_indices_for_tab(dialog, PropDialogTab::Statistics).is_empty() {
        tabs.push(PropDialogTab::Statistics);
    }
    tabs
}

#[cfg(test)]
fn all_prop_dialog_tabs() -> [PropDialogTab; 5] {
    [
        PropDialogTab::Actions,
        PropDialogTab::Writable,
        PropDialogTab::ReadOnly,
        PropDialogTab::Logs,
        PropDialogTab::Statistics,
    ]
}

fn prop_dialog_tab_title(tab: PropDialogTab, lang: Language) -> &'static str {
    match tab {
        PropDialogTab::Actions => lang_str(lang, "快捷操作", "Quick Actions"),
        PropDialogTab::Writable => lang_str(lang, "修改参数", "Edit Properties"),
        PropDialogTab::ReadOnly => lang_str(lang, "只读属性", "Read-only Properties"),
        PropDialogTab::Logs => lang_str(lang, "操作记录", "Operation Records"),
        PropDialogTab::Statistics => lang_str(lang, "统计", "Stats"),
    }
}

fn numbered_prop_dialog_tab_titles(
    tabs: impl IntoIterator<Item = PropDialogTab>,
    lang: Language,
) -> Vec<String> {
    tabs.into_iter()
        .enumerate()
        .map(|(index, tab)| format!("{}:{}", index + 1, prop_dialog_tab_title(tab, lang)))
        .collect()
}

#[cfg(test)]
fn all_prop_dialog_tab_titles(lang: Language) -> Vec<String> {
    numbered_prop_dialog_tab_titles(all_prop_dialog_tabs(), lang)
}

fn visible_prop_dialog_tab_titles(dialog: &PropDialog, lang: Language) -> Vec<String> {
    numbered_prop_dialog_tab_titles(visible_prop_dialog_tabs(dialog), lang)
}

fn prop_dialog_indices_for_tab(dialog: &PropDialog, tab: PropDialogTab) -> Vec<usize> {
    match tab {
        PropDialogTab::Actions => (0..dialog.actions.len()).collect(),
        PropDialogTab::Logs => operation_record_tab_indices(dialog),
        PropDialogTab::Statistics => statistics_tab_indices(dialog),
        _ => {
            let mut indices = dialog
                .items
                .iter()
                .enumerate()
                .filter_map(|(index, item)| match tab {
                    PropDialogTab::Writable
                        if item.prop.writable && !is_raw_device_json_item(item) =>
                    {
                        Some(index)
                    }
                    PropDialogTab::ReadOnly
                        if !item.prop.writable && !is_raw_device_json_item(item) =>
                    {
                        Some(index)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            if matches!(tab, PropDialogTab::ReadOnly) {
                indices.sort_by(|left, right| {
                    let left_name = &dialog.items[*left].prop.name;
                    let right_name = &dialog.items[*right].prop.name;
                    left_name
                        .chars()
                        .count()
                        .cmp(&right_name.chars().count())
                        .then_with(|| left_name.cmp(right_name))
                });
            }
            indices
        }
    }
}

fn format_prop_dialog_list_item_line(
    item: &ToggleItem,
    selected: bool,
    loading: bool,
    lang: Language,
) -> String {
    let selected_marker = if selected { ">" } else { " " };
    let value = if (loading && item.value.is_null())
        || (!item.prop.writable && is_error_with_negative_code(&item.value))
    {
        "-".to_string()
    } else {
        format_prop_value_for_dialog(&item.value)
    };
    let value = if item.prop.value_options.is_empty() {
        value
    } else if let Some(option) = item
        .prop
        .value_options
        .iter()
        .find(|option| option.value == item.value)
    {
        format!("{} ({value})", option.label)
    } else {
        value
    };
    let option_text = if item.prop.value_options.is_empty() {
        String::new()
    } else {
        format!(
            " | {}: {}",
            lang_str(lang, "选项", "Options"),
            item.prop
                .value_options
                .iter()
                .map(|option| option.label.as_str())
                .collect::<Vec<_>>()
                .join("/")
        )
    };
    format!(
        "{selected_marker} {} = {}{}",
        item.prop.name, value, option_text
    )
}

fn format_prop_dialog_action_list_item_line(action: &ActionItem, selected: bool) -> String {
    let selected_marker = if selected { ">" } else { " " };
    format!("{selected_marker} {}", action.name)
}

fn action_params_command_preview(dialog: &PropDialog) -> Option<String> {
    let action = dialog.actions.get(dialog.selected)?;
    let rows = action_param_rows_for_dialog(dialog);
    if rows.len() != action.input_piids.len() {
        return None;
    }
    let mut values = Vec::with_capacity(rows.len());
    for row in rows {
        values.push(
            parse_prop_input_value(row.as_str()).unwrap_or_else(|_| Value::String(row.clone())),
        );
    }
    Some(format_preview_props_act_command(
        dialog.device_did.as_str(),
        action.siid,
        action.aiid,
        values.as_slice(),
    ))
}

fn prop_edit_command_preview(dialog: &PropDialog) -> Option<String> {
    let item = dialog.items.get(dialog.selected)?;
    if !item.prop.writable {
        return None;
    }
    let value = prop_edit_selector_value(dialog, item).unwrap_or_else(|| {
        parse_prop_input_value(dialog.edit_buffer.as_str())
            .unwrap_or_else(|_| Value::String(dialog.edit_buffer.clone()))
    });
    Some(format_preview_props_set_command(
        dialog.device_did.as_str(),
        item.prop.siid,
        item.prop.piid,
        &value,
    ))
}

fn prop_edit_get_command(dialog: &PropDialog) -> Option<String> {
    let item = dialog.items.get(dialog.selected)?;
    if !item.prop.writable {
        return None;
    }
    Some(crate::cli::format_props_get_command(
        dialog.device_did.as_str(),
        item.prop.siid,
        item.prop.piid,
    ))
}

fn preview_param(text: &str) -> String {
    if text.chars().count() > 6 {
        "\"...\"".to_string()
    } else {
        text.to_string()
    }
}

fn preview_value_arg(value: &Value) -> String {
    let arg = serde_json::to_string(value).unwrap_or_else(|_| "null".to_string());
    if arg.chars().count() > 6 {
        "\"...\"".to_string()
    } else {
        arg
    }
}

fn format_preview_props_set_command(did: &str, siid: i64, piid: i64, value: &Value) -> String {
    format!(
        "mit props set {} {siid} {piid} {}",
        did,
        preview_value_arg(value)
    )
}

fn format_preview_props_act_command(did: &str, siid: i64, aiid: i64, values: &[Value]) -> String {
    let args = values
        .iter()
        .map(preview_value_arg)
        .collect::<Vec<_>>()
        .join(" ");
    if args.is_empty() {
        format!("mit props act {did} {siid} {aiid}")
    } else {
        format!("mit props act {did} {siid} {aiid} {args}")
    }
}

fn format_preview_push_command(uid: &str, text: &str) -> String {
    format!("mit push --uid {} {}", uid, preview_param(text))
}

pub(in crate::tui) fn prop_dialog_title(dialog: &PropDialog, lang: Language) -> String {
    let dialog_title = if dialog.device_name.trim().is_empty() {
        dialog.device_did.as_str()
    } else {
        dialog.device_name.as_str()
    };
    if prop_dialog_active_tab_is_loading(dialog) {
        format!(
            "{dialog_title} ({})",
            lang_str(lang, "刷新中...", "Loading...")
        )
    } else {
        dialog_title.to_string()
    }
}

pub(in crate::tui) fn prop_editor_header_lines(dialog: &PropDialog, lang: Language) -> Vec<String> {
    if dialog.active_tab == PropDialogTab::ReadOnly {
        let Some(item) = dialog.items.get(dialog.selected) else {
            return vec!["No selected property".to_string()];
        };
        return vec![
            format!(
                "{}: {} [{}]",
                lang_str(lang, "当前属性", "Property"),
                item.prop.name,
                item.prop.format
            ),
            format!(
                "{}: {}",
                lang_str(lang, "当前值", "Value"),
                format_prop_value_for_dialog(&item.value)
            ),
        ];
    }
    if dialog.active_tab == PropDialogTab::Actions {
        return dialog
            .actions
            .get(dialog.selected)
            .map(|action| vec![String::new(), action.name.clone()])
            .unwrap_or_else(|| vec![String::new(), "No selected action".to_string()]);
    }

    let Some(item) = dialog.items.get(dialog.selected) else {
        return vec!["No selected property".to_string()];
    };
    let selector_hint = if prop_edit_selector_options_text(dialog).is_some() {
        lang_str(lang, "选择值:", "Select value:")
    } else {
        lang_str(lang, "输入值:", "Enter value:")
    };
    let mut lines = vec![
        format!(
            "{}: {} [{}]",
            lang_str(lang, "当前操作", "Action"),
            item.prop.name,
            item.prop.format
        ),
        format!(
            "{}: {}",
            lang_str(lang, "当前值", "Value"),
            format_prop_value_for_dialog(&item.value)
        ),
    ];
    if !item.prop.value_options.is_empty() {
        lines.push(format!(
            "Options: {}",
            item.prop
                .value_options
                .iter()
                .map(|option| format!("{}={}", option.label, option.value))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    lines.push(String::new());
    lines.push(selector_hint.to_string());
    lines
}

fn wrapped_text_line_count(text: &str, width: u16) -> u16 {
    let width = width.max(1);
    text.split('\n')
        .map(|line| {
            let display = display_width(line).max(1);
            ((display - 1) / width).saturating_add(1)
        })
        .sum::<u16>()
        .max(1)
}

pub(in crate::tui) fn textarea_visual_height(text: &str, width: u16) -> u16 {
    wrapped_text_line_count(text, width).max(1)
}

fn action_param_row_height(
    dialog: &PropDialog,
    action: &ActionItem,
    index: usize,
    value: &str,
    width: u16,
) -> u16 {
    if action_param_selector_options(dialog, action, index).is_some() {
        return 1;
    }
    textarea_visual_height(value, width)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::tui) struct ActionParamRowLayout {
    pub(in crate::tui) index: usize,
    pub(in crate::tui) label_text: String,
    pub(in crate::tui) row_area: Rect,
    pub(in crate::tui) label_area: Rect,
    pub(in crate::tui) value_area: Rect,
}

pub(in crate::tui) fn action_param_row_layouts(
    dialog: &PropDialog,
    editor_area: Rect,
) -> Vec<ActionParamRowLayout> {
    let rows = action_param_rows_for_dialog(dialog);
    let Some(action) = dialog.actions.get(dialog.selected) else {
        return Vec::new();
    };
    let focused = if rows.is_empty() {
        0
    } else {
        dialog.writable_selected.min(rows.len() - 1)
    };
    let mut next_y = editor_area.y;
    let mut remaining = editor_area.height;
    let mut layouts = Vec::new();
    for (idx, row_value) in rows.iter().enumerate() {
        if remaining == 0 {
            break;
        }
        let label = action
            .input_labels
            .get(idx)
            .cloned()
            .unwrap_or_else(|| format!("参数{}", idx + 1));
        let marker = if idx == focused { ">" } else { " " };
        let label_text = format!("{marker} {label}: ");
        let label_width =
            display_width(label_text.as_str()).min(editor_area.width.saturating_sub(1));
        let value_width = editor_area.width.saturating_sub(label_width);
        let row_height = if action_param_selector_options(dialog, action, idx).is_some() {
            1
        } else {
            textarea_visual_height(row_value.as_str(), value_width).max(1)
        }
        .min(remaining);
        layouts.push(ActionParamRowLayout {
            index: idx,
            label_text,
            row_area: Rect::new(editor_area.x, next_y, editor_area.width, row_height),
            label_area: Rect::new(editor_area.x, next_y, label_width, 1),
            value_area: Rect::new(
                editor_area.x.saturating_add(label_width),
                next_y,
                value_width,
                row_height,
            ),
        });
        next_y = next_y.saturating_add(row_height);
        remaining = remaining.saturating_sub(row_height);
    }
    layouts
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::tui) struct PropEditorLayout {
    pub(in crate::tui) header_area: Rect,
    pub(in crate::tui) editor_area: Rect,
    pub(in crate::tui) footer_area: Option<Rect>,
}

pub(in crate::tui) fn prop_editor_layout(
    dialog: &PropDialog,
    inner: Rect,
    lang: Language,
) -> PropEditorLayout {
    let header_lines = prop_editor_header_lines(dialog, lang)
        .iter()
        .map(|line| wrapped_text_line_count(line, inner.width))
        .sum::<u16>();
    let action_rows = action_param_rows_for_dialog(dialog);
    let editor_lines = if dialog.active_tab == PropDialogTab::Actions {
        dialog
            .actions
            .get(dialog.selected)
            .map(|action| {
                action
                    .input_piids
                    .iter()
                    .enumerate()
                    .map(|(idx, _)| {
                        let label = action
                            .input_labels
                            .get(idx)
                            .cloned()
                            .unwrap_or_else(|| format!("参数{}", idx + 1));
                        let marker = if idx == dialog.writable_selected {
                            ">"
                        } else {
                            " "
                        };
                        let label_text = format!("{marker} {label}: ");
                        let label_width =
                            display_width(label_text.as_str()).min(inner.width.saturating_sub(1));
                        action_param_row_height(
                            dialog,
                            action,
                            idx,
                            action_rows.get(idx).map(String::as_str).unwrap_or(""),
                            inner.width.saturating_sub(label_width),
                        )
                    })
                    .sum::<u16>()
            })
            .unwrap_or(0)
    } else if dialog.active_tab == PropDialogTab::ReadOnly {
        0
    } else if prop_edit_selector_options_text(dialog).is_some() {
        1
    } else {
        wrapped_text_line_count(dialog.edit_buffer.as_str(), inner.width).max(2)
    };
    let footer_lines = prop_editor_bottom_lines(dialog, lang)
        .iter()
        .map(|line| wrapped_text_line_count(line, inner.width))
        .sum::<u16>();

    let mut next_y = inner.y;
    let mut remaining = inner.height;

    let header_height = header_lines.min(remaining);
    let header_area = Rect::new(inner.x, next_y, inner.width, header_height);
    next_y = next_y.saturating_add(header_height);
    remaining = remaining.saturating_sub(header_height);

    let needs_header_editor_gap = dialog.active_tab == PropDialogTab::Actions;
    if header_height > 0 && editor_lines > 0 && remaining > 0 && needs_header_editor_gap {
        next_y = next_y.saturating_add(1);
        remaining = remaining.saturating_sub(1);
    }

    let editor_height = editor_lines.min(remaining);
    let editor_area = Rect::new(inner.x, next_y, inner.width, editor_height);
    next_y = next_y.saturating_add(editor_height);
    remaining = remaining.saturating_sub(editor_height);

    let footer_area = if footer_lines > 0 && remaining > 0 {
        next_y = next_y.saturating_add(1);
        remaining = remaining.saturating_sub(1);
        let footer_height = footer_lines.min(remaining);
        Some(Rect::new(inner.x, next_y, inner.width, footer_height))
    } else {
        None
    };

    PropEditorLayout {
        header_area,
        editor_area,
        footer_area,
    }
}

fn prop_editor_bottom_lines(dialog: &PropDialog, lang: Language) -> Vec<String> {
    if dialog.active_tab == PropDialogTab::ReadOnly {
        return readonly_prop_detail_command(dialog)
            .map(|command| {
                vec![format!(
                    "{}: {command}",
                    lang_str(lang, "CLI 命令(读取)", "CLI Command (Read)")
                )]
            })
            .unwrap_or_default();
    }
    let mut bottom_lines = Vec::new();
    if dialog.active_tab != PropDialogTab::Actions {
        if let Some(command) = prop_edit_get_command(dialog) {
            bottom_lines.push(format!(
                "{}: {command}",
                lang_str(lang, "CLI 命令(读取)", "CLI Command (Read)")
            ));
        }
    }
    let command_preview = if dialog.active_tab == PropDialogTab::Actions {
        action_params_command_preview(dialog)
    } else {
        prop_edit_command_preview(dialog)
    };
    if let Some(command) = command_preview {
        bottom_lines.push(format!(
            "{}: {command}",
            lang_str(lang, "CLI 命令(执行)", "CLI Command (Execute)")
        ));
    }
    if let Some(error) = &dialog.edit_error {
        if !bottom_lines.is_empty() {
            bottom_lines.push(String::new());
        }
        bottom_lines.push(format!("Error: {error}"));
    }
    bottom_lines
}

fn drain_pending_input_events() -> Result<()> {
    while event::poll(Duration::from_millis(0))? {
        let _ = event::read()?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../tests/module_tests/tui.rs"]
mod tests;
