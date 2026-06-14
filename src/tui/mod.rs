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
use ratatui_textarea::{CursorMove, TextArea, WrapMode};
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
mod footer;
mod logs;
mod mijia_data;
mod pages;
mod prop_dialog;
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
pub(in crate::tui) use search::*;
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

fn drain_pending_input_events() -> Result<()> {
    while event::poll(Duration::from_millis(0))? {
        let _ = event::read()?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../tests/module_tests/tui.rs"]
mod tests;
