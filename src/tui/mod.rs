use anyhow::{anyhow, bail, Result};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap};
use ratatui::Terminal;
use ratatui_core::style::Style as TextAreaStyle;
use ratatui_core::widgets::Widget as TextAreaWidget;
use ratatui_textarea::{
    CursorMove, Input as TextAreaInput, Key as TextAreaKey, TextArea, WrapMode,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::stdout;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(not(test))]
use std::process::Command;

use crate::cli::ensure_fresh_account;
use crate::mico_api::{Device, MicoClient};
use crate::property_cache::PropertyCache;
use crate::spec_cache::{load_spec, specs_dir, sync_model_spec};
use crate::storage::{
    default_auth, get_auth_accounts, get_home_dir, load_auth, load_settings, normalize_auth,
    save_settings, AuthAccount, AuthState, Language, UserSettings,
};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
mod pages;
mod shared;

use self::pages::account as account_page;
use self::pages::bootstrap as bootstrap_page;
use self::pages::device::*;
use self::pages::prop as prop_page;
use shared::*;

const LOG_MAX: usize = 120;
const FOOTER_COPY_LOG_PREFIX: &str = "__footer_copied_at=";
const FOOTER_COPY_BADGE_TEXT: &str = " [已复制]";
const CACHE_ACCOUNT_PREFIX: &str = "cache-account:";
pub(crate) fn tab_titles(lang: Language) -> [&'static str; 4] {
    match lang {
        Language::Chinese => ["1:账号", "2:设备", "3:日志", "4:设置"],
        Language::English => ["1:Account", "2:Device", "3:Log", "4:Settings"],
    }
}
pub(crate) fn account_list_header_titles(lang: Language) -> [&'static str; 4] {
    match lang {
        Language::Chinese => ["地区", "昵称", "ID", "状态"],
        Language::English => ["Region", "Nickname", "ID", "Status"],
    }
}
pub(crate) fn device_list_header_titles(lang: Language) -> [&'static str; 4] {
    match lang {
        Language::Chinese => ["房间", "名称", "类别", "账户"],
        Language::English => ["Room", "Name", "Category", "Account"],
    }
}
const STATUS_BAR_MARGIN_TOP: u16 = 1;
const SETTINGS_ITEM_COUNT: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsAction {
    ClearCacheKeepAuth,
    ResetAll,
    ToggleLanguage,
}

#[derive(Clone, Copy, Debug)]
struct ActiveAuthProcess {
    generation: u64,
    pid: u32,
}

pub(in crate::tui) fn lang_str(
    lang: Language,
    zh: &'static str,
    en: &'static str,
) -> &'static str {
    match lang {
        Language::Chinese => zh,
        Language::English => en,
    }
}

fn spec_node_label<'a>(node: &'a Value, lang: Language) -> Option<&'a str> {
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

fn readonly_prop_detail_command(dialog: &BoolDialog) -> Option<String> {
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
                .is_some_and(|dialog| dialog.active_tab == BoolDialogTab::ReadOnly);
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
            let editing_actions = app
                .prop_dialog
                .as_ref()
                .is_some_and(|dialog| dialog.active_tab == BoolDialogTab::Actions);
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
            KeyCode::Up => app.prev_bool_item(),
            KeyCode::Down => app.next_bool_item(),
            KeyCode::Char(ch) if ch.is_ascii_digit() => {
                app.set_prop_dialog_tab_by_visible_order(ch)
            }
            KeyCode::Tab | KeyCode::Right => app.switch_prop_dialog_tab(true),
            KeyCode::BackTab | KeyCode::Left => app.switch_prop_dialog_tab(false),
            KeyCode::Char('r') | KeyCode::Char('R') => {
                app.request_prop_dialog_refresh();
                app.last_bool_refresh = Instant::now();
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
                KeyCode::Enter => {
                    if app.confirm_settings_action()? {
                        return Ok(true);
                    }
                }
                _ => {}
            },
            None => {}
        }
        return Ok(false);
    }

    if app.input_mode {
        match key.code {
            KeyCode::Esc => {
                app.input_mode = false;
                app.input.clear();
            }
            KeyCode::Enter => {
                let command = app.input.trim().to_string();
                app.input_mode = false;
                app.input.clear();
                if !command.is_empty() {
                    app.exec_command(&command)?;
                }
            }
            KeyCode::Backspace => {
                app.input.pop();
            }
            KeyCode::Char(c) => {
                app.input.push(c);
            }
            _ => {}
        }
        return Ok(false);
    }

    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => return Ok(true),
        KeyCode::Char('1') => app.active_tab = 0,
        KeyCode::Char('2') => app.active_tab = 1,
        KeyCode::Char('3') => app.active_tab = 2,
        KeyCode::Char('4') => app.active_tab = 3,
        KeyCode::Tab => app.next_tab(),
        KeyCode::BackTab => app.prev_tab(),
        KeyCode::Enter => match app.active_tab {
            0 => account_page::open_account_action_dialog(app)?,
            1 => {
                if let Err(error) = app.open_prop_dialog() {
                    account_page::open_offline_prop_dialog_for_current(app, &error);
                }
            }
            3 => app.execute_selected_settings_action()?,
            _ => {}
        },
        KeyCode::Char('r') | KeyCode::Char('R') => {
            app.exec_command("sync")?;
        }
        KeyCode::Char('C') if key.modifiers.contains(KeyModifiers::SHIFT) => {
            copy_last_selection(app);
        }
        KeyCode::Char('A') | KeyCode::Char('a') => account_page::start_add_account_auth_flow(app)?,
        KeyCode::Up => app.prev_item(),
        KeyCode::Down => app.next_item(),
        KeyCode::Left if key.modifiers.contains(KeyModifiers::SHIFT) => app.prev_tab(),
        KeyCode::Right if key.modifiers.contains(KeyModifiers::SHIFT) => app.next_tab(),
        _ => {}
    }

    Ok(false)
}

fn settings_action_label(action: SettingsAction, lang: Language) -> &'static str {
    match action {
        SettingsAction::ClearCacheKeepAuth => {
            lang_str(lang, "重置设备缓存", "Reset Device Cache")
        }
        SettingsAction::ResetAll => lang_str(lang, "重置全部设置", "Reset All Settings"),
        SettingsAction::ToggleLanguage => unreachable!("ToggleLanguage has no confirm dialog"),
    }
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
        SettingsAction::ToggleLanguage => unreachable!("ToggleLanguage has no confirm dialog"),
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
    if app.input_mode {
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

    if left_down && click_outside_selected_range(mouse) {
        clear_selection_state();
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
        if clicked_row >= 3 {
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
                .is_some_and(|dialog| dialog.active_tab == BoolDialogTab::Actions);
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

    let [tabs_area, list_area, _status_gap_area, _status_bar_area] =
        split_main_layout(terminal_area);
    if left_click {
        let tab_inner_top = tabs_area.y.saturating_add(1);
        let tab_inner_bottom_exclusive = tabs_area
            .y
            .saturating_add(tabs_area.height.saturating_sub(1));
        if mouse.row >= tab_inner_top && mouse.row < tab_inner_bottom_exclusive {
            if let Some(index) = tab_index_for_column(mouse.column, tabs_area, app.language) {
                app.active_tab = index;
                return Ok(());
            }
        }
    }

    if app.active_tab == 0 || app.active_tab == 1 || app.active_tab == 3 {
        if mouse.row < list_area.y
            || mouse.row >= list_area.y.saturating_add(list_area.height)
            || mouse.column < list_area.x
            || mouse.column >= list_area.x.saturating_add(list_area.width)
        {
            return Ok(());
        }

        if scroll_up {
            app.prev_item();
            return Ok(());
        }
        if scroll_down {
            app.next_item();
            return Ok(());
        }

        let clicked_row = (mouse.row - list_area.y) as usize;
        if clicked_row == 0 {
            return Ok(());
        }
        let clicked_row = clicked_row - 1;
        match app.active_tab {
            0 => {
                let idx = app.account_list_state.offset().saturating_add(clicked_row);
                if idx >= app.accounts.len() {
                    return Ok(());
                }
                if idx == app.account_index {
                    account_page::open_account_action_dialog(app)?;
                } else {
                    app.account_index = idx;
                }
            }
            1 => {
                if !left_click {
                    return Ok(());
                }
                let idx = app.device_list_state.offset().saturating_add(clicked_row);
                if idx >= app.devices.len() {
                    return Ok(());
                }
                if idx == app.device_index {
                    if let Err(error) = app.open_prop_dialog() {
                        account_page::open_offline_prop_dialog_for_current(app, &error);
                    }
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

fn logs_lines_for_display(app: &TuiApp) -> Vec<String> {
    app.logs
        .iter()
        .rev()
        .filter(|line| !line.starts_with(FOOTER_COPY_LOG_PREFIX))
        .cloned()
        .collect::<Vec<_>>()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FooterOperation {
    Refresh,
    Enter,
    Back,
    AddAccount,
    Copy,
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
            if dialog.active_tab == BoolDialogTab::ReadOnly {
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
            BoolDialogTab::Writable => build_footer_segments(
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
            BoolDialogTab::ReadOnly => build_footer_segments(
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
            BoolDialogTab::Actions => build_footer_segments(
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
        };
    }

    match app.active_tab {
        0 => vec![
            FooterSegment {
                text: lang_str(lang, "A: 新增账户", "A: Add Account").to_string(),
                operation: Some(FooterOperation::AddAccount),
            },
            FooterSegment {
                text: " ".to_string(),
                operation: None,
            },
            FooterSegment {
                text: lang_str(lang, "Enter: 账户操作", "Enter: Account Actions").to_string(),
                operation: Some(FooterOperation::Enter),
            },
        ],
        1 => {
            let total_label = lang_str(lang, "设备总数", "Total Devices");
            build_footer_segments(
                &[
                    (
                        lang_str(lang, "R: 刷新", "R: Refresh"),
                        FooterOperation::Refresh,
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
        _ => vec![FooterSegment {
            text: format!(
                "accounts={}  devices={}  selected_device={}",
                app.accounts.len(),
                app.devices.len(),
                app.selected_device_did().unwrap_or("-")
            ),
            operation: None,
        }],
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
        FooterOperation::Enter => {
            let _ = handle_key(
                app,
                crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
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
        .block(Block::default().borders(all_borders()).title("MI Tui"))
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
            let selected = if app.accounts.is_empty() {
                None
            } else {
                Some(app.account_index.min(app.accounts.len().saturating_sub(1)))
            };
            app.account_list_state.select(selected);
            let [header_area, list_area] = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(0)])
                .areas(content_area);
            let rows = app
                .accounts
                .iter()
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
                    if idx == app.account_index {
                        item = item.style(active_row_style());
                    }
                    item
                })
                .collect::<Vec<_>>();
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
            let device_categories = read_device_categories(app.home_dir.as_path(), app.language);
            let selected = if app.devices.is_empty() {
                None
            } else {
                Some(app.device_index.min(app.devices.len().saturating_sub(1)))
            };
            app.device_list_state.select(selected);
            let [header_area, list_area] = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(0)])
                .areas(content_area);
            let rows = app
                .devices
                .iter()
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
                    if idx == app.device_index {
                        item = item.style(active_row_style());
                    }
                    item
                })
                .collect::<Vec<_>>();
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
            let lines = logs_lines_for_display(app)
                .into_iter()
                .enumerate()
                .map(|(idx, line)| {
                    if let Some(active) = selected_text.as_ref() {
                        if active.snapshot.surface == SelectionSurface::Logs {
                            if let Some((mut start, mut end)) = selected_cols_for_line(active, idx)
                            {
                                let width = display_width(&line);
                                if end == u16::MAX {
                                    end = width;
                                }
                                start = start.min(width);
                                end = end.min(width);
                                return ListItem::new(highlight_line_range(&line, start, end));
                            }
                        }
                    }
                    ListItem::new(line)
                })
                .collect::<Vec<_>>();
            frame.render_widget(List::new(lines).block(Block::default()), content_area);
        }
    }

    if let Some(dialog) = &app.account_action_dialog {
        match dialog {
            AccountActionDialog::Menu { selected } => {
                let popup = centered_rect(48, 34, frame.area());
                let items = [
                    lang_str(app.language, "推送消息", "Push Message"),
                    lang_str(app.language, "登录", "Login"),
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
                        Block::default()
                            .borders(all_borders())
                            .title(lang_str(app.language, "账户操作", "Account Actions")),
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
    input_mode: bool,
    input: String,
    prop_dialog: Option<BoolDialog>,
    account_action_dialog: Option<AccountActionDialog>,
    account_list_state: ListState,
    device_list_state: ListState,
    last_bool_refresh: Instant,
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
            input_mode: false,
            input: String::new(),
            prop_dialog: None,
            account_action_dialog: None,
            account_list_state: ListState::default(),
            device_list_state: ListState::default(),
            last_bool_refresh: Instant::now(),
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
            settings_selected: 0,
        })
    }

    fn start_bootstrap(&mut self) {
        self.start_bootstrap_internal(true);
    }

    fn start_background_sync(&mut self) {
        self.start_bootstrap_internal(false);
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
                        device_categories =
                            read_device_categories_from_template(
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

    fn log(&mut self, message: impl Into<String>) {
        self.logs.push_back(message.into());
        while self.logs.len() > LOG_MAX {
            self.logs.pop_front();
        }
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
            1 => SettingsAction::ClearCacheKeepAuth,
            _ => SettingsAction::ResetAll,
        }
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
                let _ = save_settings(&UserSettings {
                    language: self.language,
                });
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
        let mit_dir = self.home_dir.join(".mit");
        let mut removed_entries = 0usize;
        if mit_dir.exists() {
            for entry in fs::read_dir(&mit_dir)? {
                let entry = entry?;
                let path = entry.path();
                let keep_auth = path
                    .file_name()
                    .is_some_and(|name| name == std::ffi::OsStr::new("auth.json"));
                if keep_auth {
                    continue;
                }
                if path.is_dir() {
                    fs::remove_dir_all(path)?;
                } else {
                    fs::remove_file(path)?;
                }
                removed_entries += 1;
            }
        }

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
        let mit_dir = self.home_dir.join(".mit");
        if mit_dir.exists() {
            fs::remove_dir_all(&mit_dir)?;
        }
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
            SettingsAction::ToggleLanguage => self.execute_settings_action(action),
            _ => {
                self.account_action_dialog = Some(AccountActionDialog::SettingsConfirm { action });
                Ok(())
            }
        }
    }

    fn next_tab(&mut self) {
        self.active_tab = (self.active_tab + 1) % tab_titles(self.language).len();
    }

    fn prev_tab(&mut self) {
        self.active_tab = if self.active_tab == 0 {
            tab_titles(self.language).len().saturating_sub(1)
        } else {
            self.active_tab - 1
        };
    }

    fn next_item(&mut self) {
        match self.active_tab {
            0 => {
                if !self.accounts.is_empty() {
                    // Account switches invalidate pending bootstrap results for the previous account.
                    self.bootstrap_pending = None;
                    self.account_index = (self.account_index + 1) % self.accounts.len();
                    self.request_local_transport_refresh(false);
                }
            }
            1 => {
                if !self.devices.is_empty() {
                    self.device_index = (self.device_index + 1) % self.devices.len();
                }
            }
            3 => {
                if SETTINGS_ITEM_COUNT > 0 {
                    self.select_settings_item(
                        (self.settings_selected_index() + 1) % SETTINGS_ITEM_COUNT,
                    );
                }
            }
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
                BoolDialogTab::Writable => dialog.writable_selected = dialog.selected,
                BoolDialogTab::ReadOnly => dialog.readonly_selected = dialog.selected,
                BoolDialogTab::Actions => dialog.actions_selected = dialog.selected,
            }
        }
    }

    fn prop_dialog_has_toggle_items(&self) -> bool {
        self.prop_dialog.as_ref().is_some_and(|dialog| {
            !dialog.loading
                && dialog.status.is_none()
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
                BoolDialogTab::Writable => dialog.writable_selected,
                BoolDialogTab::ReadOnly => dialog.readonly_selected,
                BoolDialogTab::Actions => dialog.actions_selected,
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
                BoolDialogTab::Writable => dialog.writable_selected = selected,
                BoolDialogTab::ReadOnly => dialog.readonly_selected = selected,
                BoolDialogTab::Actions => dialog.actions_selected = selected,
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

    fn set_prop_dialog_tab(&mut self, target: BoolDialogTab) {
        if let Some(dialog) = &mut self.prop_dialog {
            if dialog.editing {
                return;
            }
            let visible_tabs = visible_prop_dialog_tabs(dialog);
            if !visible_tabs.contains(&target) {
                return;
            }
            match dialog.active_tab {
                BoolDialogTab::Writable => dialog.writable_selected = dialog.selected,
                BoolDialogTab::ReadOnly => dialog.readonly_selected = dialog.selected,
                BoolDialogTab::Actions => dialog.actions_selected = dialog.selected,
            }
            dialog.active_tab = target;
            // Reset active index to 0 when switching tabs
            match target {
                BoolDialogTab::Writable => dialog.writable_selected = 0,
                BoolDialogTab::ReadOnly => dialog.readonly_selected = 0,
                BoolDialogTab::Actions => dialog.actions_selected = 0,
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
                BoolDialogTab::Writable => dialog.writable_list_state.offset(),
                BoolDialogTab::ReadOnly => dialog.readonly_list_state.offset(),
                BoolDialogTab::Actions => dialog.actions_list_state.offset(),
            };
            let position = offset.saturating_add(row);
            if let Some(global_index) = indices.get(position) {
                dialog.selected = *global_index;
                match dialog.active_tab {
                    BoolDialogTab::Writable => dialog.writable_selected = dialog.selected,
                    BoolDialogTab::ReadOnly => dialog.readonly_selected = dialog.selected,
                    BoolDialogTab::Actions => dialog.actions_selected = dialog.selected,
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
                BoolDialogTab::Writable => dialog.writable_list_state.offset(),
                BoolDialogTab::ReadOnly => dialog.readonly_list_state.offset(),
                BoolDialogTab::Actions => dialog.actions_list_state.offset(),
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
            .unwrap_or(BoolDialogTab::Writable);
        if active_tab == BoolDialogTab::Actions {
            if let Err(error) = self.start_selected_action_params_edit() {
                account_page::open_offline_prop_dialog_for_current(self, &error);
            }
            return;
        }
        if active_tab == BoolDialogTab::ReadOnly {
            let _ = self.start_selected_readonly_prop_detail();
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
                if dialog.active_tab == BoolDialogTab::Actions {
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
                if dialog.active_tab == BoolDialogTab::Actions {
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
                if dialog.active_tab == BoolDialogTab::Actions {
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
                if dialog.active_tab == BoolDialogTab::Actions {
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
            if !dialog.editing || dialog.active_tab == BoolDialogTab::Actions {
                return;
            }
            let _ = cycle_prop_edit_selector(dialog, forward);
        }
    }

    fn submit_selected_prop_edit(&mut self) -> Result<()> {
        if self
            .prop_dialog
            .as_ref()
            .is_some_and(|dialog| dialog.active_tab == BoolDialogTab::Actions)
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
                BoolDialogTab::Writable => dialog.writable_selected = index,
                BoolDialogTab::ReadOnly => dialog.readonly_selected = index,
                BoolDialogTab::Actions => dialog.actions_selected = index,
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
            if dialog.active_tab != BoolDialogTab::Actions || !dialog.editing {
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
            if dialog.active_tab != BoolDialogTab::Actions || !dialog.editing {
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
            if dialog.active_tab != BoolDialogTab::Actions || !dialog.editing {
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
            *selected = (*selected + 1) % 3;
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
                BoolDialogTab::Writable => dialog.writable_selected = dialog.selected,
                BoolDialogTab::ReadOnly => dialog.readonly_selected = dialog.selected,
                BoolDialogTab::Actions => dialog.actions_selected = dialog.selected,
            }
        }
    }

    fn prev_account_action_item(&mut self) {
        if let Some(AccountActionDialog::Menu { selected }) = &mut self.account_action_dialog {
            *selected = if *selected == 0 { 2 } else { *selected - 1 };
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
            .map(|prop| BoolToggleItem {
                prop,
                value: Value::Null,
            })
            .collect::<Vec<_>>();
        let writable_indices = placeholder_items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| item.prop.writable.then_some(index))
            .collect::<Vec<_>>();
        let readonly_indices = placeholder_items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| (!item.prop.writable).then_some(index))
            .collect::<Vec<_>>();
        let active_tab = if !actions.is_empty() {
            BoolDialogTab::Actions
        } else if writable_indices.is_empty() {
            BoolDialogTab::ReadOnly
        } else {
            BoolDialogTab::Writable
        };
        let initial_selected = match active_tab {
            BoolDialogTab::Writable => writable_indices.first().copied(),
            BoolDialogTab::ReadOnly => readonly_indices.first().copied(),
            BoolDialogTab::Actions => Some(0),
        }
        .unwrap_or(0);
        let did = selected.did.clone();
        let property_cache = self.property_cache.clone();
        let (tx, rx) = mpsc::channel::<std::result::Result<Vec<BoolToggleItem>, String>>();
        thread::spawn(move || {
            let result = (|| -> Result<Vec<BoolToggleItem>> {
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

                let items = props
                    .into_iter()
                    .enumerate()
                    .map(|(index, prop)| BoolToggleItem {
                        prop,
                        value: cached_values
                            .get(index)
                            .and_then(|v| v.as_ref())
                            .and_then(extract_prop_value)
                            .unwrap_or(Value::Null),
                    })
                    .collect::<Vec<_>>();
                Ok(items)
            })()
            .map_err(|error| error.to_string());
            let _ = tx.send(result);
        });
        self.account_action_dialog = None;
        self.prop_dialog = Some(BoolDialog {
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
            refresh_rx: None,
        });
        self.last_bool_refresh = Instant::now();
        self.log(format!(
            "opened property dialog for {} ({})",
            selected.name, selected.did
        ));
        Ok(())
    }

    fn request_prop_dialog_refresh(&mut self) {
        let (device_did, account, queries) = match &self.prop_dialog {
            Some(dialog)
                if !dialog.loading
                    && dialog.status.is_none()
                    && !dialog.items.is_empty()
                    && !dialog.editing
                    && !dialog.refreshing =>
            {
                let Some(account) = self
                    .accounts
                    .iter()
                    .find(|account| account.user.uid == dialog.account_uid)
                    .cloned()
                else {
                    return;
                };
                (
                    dialog.device_did.clone(),
                    account,
                    dialog
                        .items
                        .iter()
                        .map(|item| (item.prop.siid, item.prop.piid))
                        .collect::<Vec<_>>(),
                )
            }
            _ => return,
        };
        let property_cache = self.property_cache.clone();
        let (tx, rx) = mpsc::channel::<std::result::Result<Vec<Value>, String>>();
        if let Some(dialog) = &mut self.prop_dialog {
            dialog.refreshing = true;
            dialog.refresh_rx = Some(rx);
        }
        thread::spawn(move || {
            let result = (|| -> Result<Vec<Value>> {
                let query_refs = queries
                    .iter()
                    .map(|(siid, piid)| (device_did.as_str(), *siid, *piid))
                    .collect::<Vec<_>>();
                let client = MicoClient::new(&account)?;
                let values = client.get_props_batch(&query_refs)?;
                let list = values
                    .as_array()
                    .ok_or_else(|| anyhow!("property refresh did not return list"))?;

                // Update cache with fresh values
                let mut cache_update: std::collections::HashMap<(i64, i64), Value> =
                    std::collections::HashMap::new();
                for (idx, (siid, piid)) in queries.iter().enumerate() {
                    let value = list.get(idx).cloned().unwrap_or(Value::Null);
                    cache_update.insert((*siid, *piid), value);
                }
                property_cache.set_device_properties(device_did.clone(), cache_update);

                Ok(list
                    .iter()
                    .map(|raw| extract_prop_value(raw).unwrap_or(Value::Null))
                    .collect::<Vec<_>>())
            })()
            .map_err(|error| error.to_string());
            let _ = tx.send(result);
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
                        let previous_selected = dialog.selected;
                        let previous_writable_selected = dialog.writable_selected;
                        let previous_readonly_selected = dialog.readonly_selected;
                        let previous_actions_selected = dialog.actions_selected;
                        dialog.items = items;
                        let writable_indices =
                            prop_dialog_indices_for_tab(dialog, BoolDialogTab::Writable);
                        let readonly_indices =
                            prop_dialog_indices_for_tab(dialog, BoolDialogTab::ReadOnly);
                        let actions_indices =
                            prop_dialog_indices_for_tab(dialog, BoolDialogTab::Actions);
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
                            BoolDialogTab::Writable => {
                                preserve_index(&writable_indices, previous_selected)
                            }
                            BoolDialogTab::ReadOnly => {
                                preserve_index(&readonly_indices, previous_selected)
                            }
                            BoolDialogTab::Actions => {
                                preserve_index(&actions_indices, previous_selected)
                            }
                        };
                        match dialog.active_tab {
                            BoolDialogTab::Writable => dialog.writable_selected = dialog.selected,
                            BoolDialogTab::ReadOnly => dialog.readonly_selected = dialog.selected,
                            BoolDialogTab::Actions => dialog.actions_selected = dialog.selected,
                        }
                        dialog.status = None;
                    }
                    Err(error) => {
                        dialog.items.clear();
                        dialog.status = Some(error.clone());
                        self.log(format!("property dialog loading failed: {error}"));
                    }
                }
            }
        }

        let refresh_recv = match self.prop_dialog.as_ref() {
            Some(dialog) if dialog.refreshing => match dialog.refresh_rx.as_ref() {
                Some(rx) => match rx.try_recv() {
                    Ok(result) => Some(result),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        Some(Err("property refresh worker disconnected".to_string()))
                    }
                },
                None => None,
            },
            _ => None,
        };
        let Some(refresh_result) = refresh_recv else {
            return;
        };
        if let Some(dialog) = &mut self.prop_dialog {
            dialog.refreshing = false;
            dialog.refresh_rx = None;
            match refresh_result {
                Ok(values) => {
                    for (index, value) in values.into_iter().enumerate() {
                        if let Some(item) = dialog.items.get_mut(index) {
                            item.value = value;
                        }
                    }
                }
                Err(error) => {
                    self.log(format!("property dialog refresh failed: {error}"));
                }
            }
        }
    }

    fn prev_item(&mut self) {
        match self.active_tab {
            0 => {
                if !self.accounts.is_empty() {
                    // Account switches invalidate pending bootstrap results for the previous account.
                    self.bootstrap_pending = None;
                    self.account_index = if self.account_index == 0 {
                        self.accounts.len() - 1
                    } else {
                        self.account_index - 1
                    };
                    self.request_local_transport_refresh(false);
                }
            }
            1 => {
                if !self.devices.is_empty() {
                    self.device_index = if self.device_index == 0 {
                        self.devices.len() - 1
                    } else {
                        self.device_index - 1
                    };
                }
            }
            3 => {
                if SETTINGS_ITEM_COUNT > 0 {
                    self.select_settings_item(if self.settings_selected_index() == 0 {
                        SETTINGS_ITEM_COUNT - 1
                    } else {
                        self.settings_selected_index() - 1
                    });
                }
            }
            _ => {}
        }
    }

    fn refresh_client_for_current_account(
        &mut self,
        refresh_local_transport: bool,
    ) -> Result<crate::mico_api::MicoClient> {
        let uid = self
            .current_uid()
            .ok_or_else(|| anyhow!("不存在可用账号"))?
            .to_string();
        self.refresh_client_for_account_uid(uid.as_str(), refresh_local_transport)
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

    fn exec_command(&mut self, command_line: &str) -> Result<()> {
        self.log(format!("> {command_line}"));
        let command = parse_command(command_line)?;
        match command {
            TuiCommand::Sync => {
                self.start_background_sync();
                self.log("sync started in background");
            }
            TuiCommand::SyncSpecs => {
                // Foreground spec sync is newer state than any in-flight startup bootstrap.
                self.bootstrap_pending = None;
                self.sync_specs()?;
            }
            TuiCommand::Get { did, siid, piid } => {
                let did = self.resolve_did(&did)?;
                let client = self.refresh_client_for_current_account(true)?;
                let value = client.get_prop(&did, siid, piid)?;
                let display = extract_prop_value(&value)
                    .map(|entry| format_prop_value_for_dialog(&entry))
                    .unwrap_or_else(|| value.to_string());
                self.log(format!("get {did} {siid}.{piid} => {display}"));
            }
            TuiCommand::Set {
                did,
                siid,
                piid,
                value,
            } => {
                let did = self.resolve_did(&did)?;
                let client = self.refresh_client_for_current_account(true)?;
                let result = client.set_prop(&did, siid, piid, value.clone())?;
                self.log(format!(
                    "set {did} {siid}.{piid} <= {} => {}",
                    value, result
                ));
            }
            TuiCommand::Act {
                did,
                siid,
                aiid,
                values,
            } => {
                let did = self.resolve_did(&did)?;
                let client = self.refresh_client_for_current_account(true)?;
                let value = client.action(&did, siid, aiid, &values)?;
                self.log(format!("act {did} {siid}.{aiid} => {}", value));
            }
            TuiCommand::Help => {
                self.log("sync | sync-specs | get <did|@> <siid> <piid> | set <did|@> <siid> <piid> <json> | act <did|@> <siid> <aiid> <json[]> | property dialog: press Enter on device");
            }
        }
        Ok(())
    }

    fn resolve_did(&self, did: &str) -> Result<String> {
        if did == "@" {
            return self
                .selected_device_did()
                .map(ToString::to_string)
                .ok_or_else(|| anyhow!("当前没有选中设备"));
        }
        Ok(did.to_string())
    }

    fn sync_specs(&mut self) -> Result<()> {
        if self.devices.is_empty() {
            self.log("no devices loaded, use sync first");
            return Ok(());
        }
        let unique_models = unique_device_models(&self.devices);
        for line in sync_specs_for_models(&self.home_dir, unique_models.as_slice()) {
            self.log(line);
        }
        Ok(())
    }
}

fn unique_device_models(devices: &[Device]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut models = Vec::new();
    for device in devices {
        let model = device.model.trim();
        if model.is_empty() {
            continue;
        }
        if seen.insert(model.to_string()) {
            models.push(model.to_string());
        }
    }
    models
}

fn device_models_missing_local_specs(
    home_dir: &std::path::Path,
    devices: &[Device],
) -> Vec<String> {
    let models = unique_device_models(devices);
    if models.is_empty() {
        return models;
    }

    let specs = specs_dir(home_dir);
    if !specs
        .join("sources")
        .join("template_list_device.json")
        .exists()
        || !specs.join("index.json").exists()
    {
        return models;
    }

    models
        .into_iter()
        .filter(|model| !matches!(load_spec(home_dir, model), Ok(Some(_))))
        .collect()
}

fn sync_specs_for_models(home_dir: &std::path::Path, models: &[String]) -> Vec<String> {
    let mut logs = Vec::new();
    let mut synced = 0_usize;
    for model in models {
        match sync_model_spec(home_dir, model.as_str()) {
            Ok(path) => {
                logs.push(format!("spec cached: {} -> {}", model, path.display()));
                synced += 1;
            }
            Err(error) => {
                logs.push(format!("spec sync failed {}: {}", model, error));
            }
        }
    }
    logs.push(format!("spec sync completed ({synced} models)"));
    logs
}

fn read_device_categories(home_dir: &std::path::Path, lang: Language) -> HashMap<String, String> {
    let mut categories = read_device_categories_from_cached_devices(home_dir).unwrap_or_default();
    categories.extend(
        read_device_categories_from_template(home_dir, lang).unwrap_or_default(),
    );
    categories
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct CachedDevicesPayload {
    #[serde(default)]
    devices: Vec<Device>,
    #[serde(default)]
    categories: HashMap<String, String>,
}

#[derive(Debug, PartialEq)]
enum TuiCommand {
    Sync,
    SyncSpecs,
    Get {
        did: String,
        siid: i64,
        piid: i64,
    },
    Set {
        did: String,
        siid: i64,
        piid: i64,
        value: Value,
    },
    Act {
        did: String,
        siid: i64,
        aiid: i64,
        values: Vec<Value>,
    },
    Help,
}

fn parse_i64(text: &str, field: &str) -> Result<i64> {
    text.parse::<i64>()
        .map_err(|_| anyhow!("{field} 必须是整数: {text}"))
}

fn parse_command(line: &str) -> Result<TuiCommand> {
    let mut parts = line.split_whitespace();
    let command = parts.next().unwrap_or_default();

    match command {
        "sync" => Ok(TuiCommand::Sync),
        "sync-specs" => Ok(TuiCommand::SyncSpecs),
        "help" => Ok(TuiCommand::Help),
        "get" => {
            let did = parts
                .next()
                .ok_or_else(|| anyhow!("get 需要 did|@ siid piid"))?
                .to_string();
            let siid = parse_i64(parts.next().ok_or_else(|| anyhow!("缺少 siid"))?, "siid")?;
            let piid = parse_i64(parts.next().ok_or_else(|| anyhow!("缺少 piid"))?, "piid")?;
            Ok(TuiCommand::Get { did, siid, piid })
        }
        "set" => {
            let did = parts
                .next()
                .ok_or_else(|| anyhow!("set 需要 did|@ siid piid value"))?
                .to_string();
            let siid = parse_i64(parts.next().ok_or_else(|| anyhow!("缺少 siid"))?, "siid")?;
            let piid = parse_i64(parts.next().ok_or_else(|| anyhow!("缺少 piid"))?, "piid")?;
            let raw = parts.collect::<Vec<_>>().join(" ");
            if raw.trim().is_empty() {
                bail!("缺少 value，示例：set @ 2 1 true");
            }
            let value = serde_json::from_str::<Value>(&raw)
                .map_err(|error| anyhow!("set 的 value 必须是合法 JSON: {error}"))?;
            Ok(TuiCommand::Set {
                did,
                siid,
                piid,
                value,
            })
        }
        "act" => {
            let did = parts
                .next()
                .ok_or_else(|| anyhow!("act 需要 did|@ siid aiid values"))?
                .to_string();
            let siid = parse_i64(parts.next().ok_or_else(|| anyhow!("缺少 siid"))?, "siid")?;
            let aiid = parse_i64(parts.next().ok_or_else(|| anyhow!("缺少 aiid"))?, "aiid")?;
            let raw = parts.collect::<Vec<_>>().join(" ");
            let values = match serde_json::from_str::<Value>(&raw)? {
                Value::Array(values) => values,
                _ => bail!("act 的 values 必须是 JSON 数组"),
            };
            Ok(TuiCommand::Act {
                did,
                siid,
                aiid,
                values,
            })
        }
        _ => bail!("未知命令: {command}"),
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

fn local_credentials_snapshot_path(
    home_dir: &std::path::Path,
    uid: &str,
) -> Option<std::path::PathBuf> {
    let uid = uid.trim();
    if uid.is_empty() {
        return None;
    }
    Some(
        home_dir
            .join(".mit")
            .join("accounts")
            .join(uid)
            .join("local_credentials.json"),
    )
}

fn load_cached_devices_from_home(home_dir: &std::path::Path, uid: &str) -> Result<Vec<Device>> {
    let auth_path = home_dir.join(".mit").join("auth.json");
    if !auth_path.exists() {
        return Ok(Vec::new());
    }
    let auth_text = fs::read_to_string(auth_path)?;
    let auth_value: Value = serde_json::from_str(&auth_text)?;
    let auth_state = normalize_auth(auth_value)?;
    let accounts = get_auth_accounts(&auth_state)?;

    let target_accounts = if uid.trim().is_empty() {
        accounts
            .into_iter()
            .filter(|account| !account.user.uid.trim().is_empty())
            .collect::<Vec<_>>()
    } else {
        accounts
            .into_iter()
            .filter(|account| account.user.uid == uid)
            .collect::<Vec<_>>()
    };

    let mut devices = Vec::new();
    for account in target_accounts {
        let account_uid = account.user.uid.clone();
        if account_uid.trim().is_empty() {
            continue;
        }

        // devices.json is the only cached source of device metadata (name/model/room).
        // local_credentials.json now stores only local transport credentials.
        if let Ok(cached) = load_cached_devices_for_account(home_dir, &account_uid) {
            if !cached.is_empty() {
                devices.extend(cached);
            }
        }
    }

    Ok(devices)
}

fn cache_devices_for_account(
    home_dir: &std::path::Path,
    uid: &str,
    devices: &[Device],
    categories: &HashMap<String, String>,
) -> Result<()> {
    let uid = uid.trim();
    if uid.is_empty() {
        return Ok(());
    }
    let path = home_dir
        .join(".mit")
        .join("accounts")
        .join(uid)
        .join("devices.json");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let devices_by_uid: Vec<Device> = devices
        .iter()
        .filter(|d| d.home_id == format!("{CACHE_ACCOUNT_PREFIX}{uid}"))
        .cloned()
        .collect();
    let categories_by_uid = devices_by_uid
        .iter()
        .filter_map(|device| {
            categories
                .get(device.model.as_str())
                .map(|category| (device.model.clone(), category.clone()))
        })
        .collect::<HashMap<_, _>>();
    let text = serde_json::to_string_pretty(&CachedDevicesPayload {
        devices: devices_by_uid,
        categories: categories_by_uid,
    })?;
    fs::write(&path, format!("{}\n", text))?;
    Ok(())
}

fn load_cached_devices_for_account(home_dir: &std::path::Path, uid: &str) -> Result<Vec<Device>> {
    let path = home_dir
        .join(".mit")
        .join("accounts")
        .join(uid.trim())
        .join("devices.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(path)?;
    let value: Value = serde_json::from_str(&text)?;
    if value.is_array() {
        return Ok(serde_json::from_value(value)?);
    }
    if let Some(devices) = value.get("devices") {
        return Ok(serde_json::from_value(devices.clone())?);
    }
    Ok(Vec::new())
}

fn cache_devices_for_accounts(
    home_dir: &std::path::Path,
    devices: &[Device],
    categories: &HashMap<String, String>,
) -> Result<()> {
    let account_uids = devices
        .iter()
        .filter_map(device_account_uid)
        .map(ToString::to_string)
        .collect::<HashSet<_>>();
    for uid in account_uids {
        cache_devices_for_account(home_dir, uid.as_str(), devices, categories)?;
    }
    Ok(())
}

fn read_device_categories_from_cached_devices(
    home_dir: &std::path::Path,
) -> Result<HashMap<String, String>> {
    let accounts_dir = home_dir.join(".mit").join("accounts");
    if !accounts_dir.exists() {
        return Ok(HashMap::new());
    }
    let mut categories = HashMap::new();
    for entry in fs::read_dir(accounts_dir)? {
        let entry = entry?;
        let path = entry.path().join("devices.json");
        if !path.exists() {
            continue;
        }
        let text = fs::read_to_string(path)?;
        let value: Value = serde_json::from_str(&text)?;
        let Some(category_map) = value.get("categories").and_then(Value::as_object) else {
            continue;
        };
        for (model, category) in category_map {
            let Some(category) = category
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            categories.insert(model.clone(), category.to_string());
        }
    }
    Ok(categories)
}

fn tag_devices_with_account(
    devices: Vec<Device>,
    account_uid: &str,
    account_label: &str,
) -> Vec<Device> {
    devices
        .into_iter()
        .map(|mut device| {
            device.home_id = format!("{CACHE_ACCOUNT_PREFIX}{account_uid}");
            device.home_name = account_label.to_string();
            device
        })
        .collect()
}

fn device_account_uid(device: &Device) -> Option<&str> {
    device
        .home_id
        .strip_prefix(CACHE_ACCOUNT_PREFIX)
        .map(str::trim)
        .filter(|uid| !uid.is_empty())
}

fn merge_devices(into: &mut Vec<Device>, extra: Vec<Device>) {
    let mut seen = into
        .iter()
        .map(|device| format!("{}::{}", device.home_id, device.did))
        .collect::<HashSet<_>>();
    for device in extra {
        let key = format!("{}::{}", device.home_id, device.did);
        if seen.insert(key) {
            into.push(device);
        }
    }
}

fn sort_devices_by_room(devices: &mut [Device]) {
    devices.sort_by(|a, b| a.room_name.cmp(&b.room_name).then(a.name.cmp(&b.name)));
}

#[derive(Clone, Debug, PartialEq)]
struct BoolPropItem {
    siid: i64,
    piid: i64,
    name: String,
    format: String,
    writable: bool,
    value_options: Vec<BoolPropValueOption>,
}

#[derive(Clone, Debug, PartialEq)]
struct BoolPropValueOption {
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
    input_props: Vec<BoolPropItem>,
}

#[derive(Clone, Debug)]
struct BoolToggleItem {
    prop: BoolPropItem,
    value: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BoolDialogTab {
    Actions,
    Writable,
    ReadOnly,
}

#[derive(Debug)]
struct BoolDialog {
    device_did: String,
    device_name: String,
    account_uid: String,
    items: Vec<BoolToggleItem>,
    selected: usize,
    active_tab: BoolDialogTab,
    writable_selected: usize,
    readonly_selected: usize,
    actions: Vec<ActionItem>,
    actions_selected: usize,
    writable_list_state: ListState,
    readonly_list_state: ListState,
    actions_list_state: ListState,
    loading: bool,
    loading_rx: Option<Receiver<std::result::Result<Vec<BoolToggleItem>, String>>>,
    status: Option<String>,
    editing: bool,
    edit_buffer: String,
    edit_cursor: usize,
    edit_error: Option<String>,
    refreshing: bool,
    refresh_rx: Option<Receiver<std::result::Result<Vec<Value>, String>>>,
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

pub(in crate::tui) fn prop_edit_textarea_area(dialog: &BoolDialog, editor_area: Rect) -> Rect {
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

fn apply_action_param_row_key(dialog: &mut BoolDialog, key: crossterm::event::KeyEvent) {
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

fn action_param_rows_for_dialog(dialog: &BoolDialog) -> Vec<String> {
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

fn set_action_param_rows_in_dialog(dialog: &mut BoolDialog, rows: &[String]) {
    dialog.edit_buffer = rows.join("\n");
}

fn action_param_selector_options(
    dialog: &BoolDialog,
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
    dialog: &BoolDialog,
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
    dialog: &BoolDialog,
    action: &ActionItem,
    index: usize,
    row: Option<&str>,
) -> Option<Value> {
    let options = action_param_selector_options(dialog, action, index)?;
    let idx = action_param_selector_index(dialog, action, index, options.as_slice(), row);
    options.get(idx).map(|(_, value)| value.clone())
}

fn action_param_default_row(dialog: &BoolDialog, action: &ActionItem, index: usize) -> String {
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
    dialog: &BoolDialog,
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

fn selector_options_for_prop(prop: &BoolPropItem) -> Option<Vec<(String, Value)>> {
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

fn prop_edit_selector_options(item: &BoolToggleItem) -> Option<Vec<(String, Value)>> {
    selector_options_for_prop(&item.prop)
}

fn prop_edit_selector_index(item: &BoolToggleItem, options: &[(String, Value)]) -> usize {
    options
        .iter()
        .position(|(_, value)| *value == item.value)
        .unwrap_or(0)
}

fn prop_edit_selector_is_active(dialog: &BoolDialog) -> bool {
    if dialog.active_tab == BoolDialogTab::Actions || !dialog.editing {
        return false;
    }
    dialog
        .items
        .get(dialog.selected)
        .and_then(prop_edit_selector_options)
        .is_some()
}

fn prop_edit_selector_value(dialog: &BoolDialog, item: &BoolToggleItem) -> Option<Value> {
    let options = prop_edit_selector_options(item)?;
    if let Ok(current) = parse_prop_input_value(dialog.edit_buffer.as_str()) {
        if let Some((_, value)) = options.iter().find(|(_, value)| *value == current) {
            return Some(value.clone());
        }
    }
    let index = prop_edit_selector_index(item, options.as_slice());
    options.get(index).map(|(_, value)| value.clone())
}

fn cycle_prop_edit_selector(dialog: &mut BoolDialog, forward: bool) -> bool {
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

fn prop_edit_selector_options_text(dialog: &BoolDialog) -> Option<String> {
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

fn prop_edit_selector_line(dialog: &BoolDialog, base_style: Style) -> Option<Line<'static>> {
    let item = dialog.items.get(dialog.selected)?;
    let options = prop_edit_selector_options(item)?;
    let active_value = prop_edit_selector_value(dialog, item);
    Some(selector_option_line(
        options.as_slice(),
        active_value.as_ref(),
        base_style,
    ))
}

fn visible_prop_dialog_tabs(dialog: &BoolDialog) -> Vec<BoolDialogTab> {
    let mut tabs = Vec::new();
    if !dialog.actions.is_empty() {
        tabs.push(BoolDialogTab::Actions);
    }
    if !prop_dialog_indices_for_tab(dialog, BoolDialogTab::Writable).is_empty() {
        tabs.push(BoolDialogTab::Writable);
    }
    if !prop_dialog_indices_for_tab(dialog, BoolDialogTab::ReadOnly).is_empty() {
        tabs.push(BoolDialogTab::ReadOnly);
    }
    tabs
}

#[cfg(test)]
fn all_prop_dialog_tabs() -> [BoolDialogTab; 3] {
    [
        BoolDialogTab::Actions,
        BoolDialogTab::Writable,
        BoolDialogTab::ReadOnly,
    ]
}

fn prop_dialog_tab_title(tab: BoolDialogTab, lang: Language) -> &'static str {
    match tab {
        BoolDialogTab::Actions => lang_str(lang, "快捷操作", "Quick Actions"),
        BoolDialogTab::Writable => lang_str(lang, "修改参数", "Edit Properties"),
        BoolDialogTab::ReadOnly => lang_str(lang, "只读属性", "Read-only Properties"),
    }
}

fn numbered_prop_dialog_tab_titles(
    tabs: impl IntoIterator<Item = BoolDialogTab>,
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

fn visible_prop_dialog_tab_titles(dialog: &BoolDialog, lang: Language) -> Vec<String> {
    numbered_prop_dialog_tab_titles(visible_prop_dialog_tabs(dialog), lang)
}

fn prop_dialog_indices_for_tab(dialog: &BoolDialog, tab: BoolDialogTab) -> Vec<usize> {
    match tab {
        BoolDialogTab::Actions => (0..dialog.actions.len()).collect(),
        _ => {
            let mut indices = dialog
                .items
                .iter()
                .enumerate()
                .filter_map(|(index, item)| match tab {
                    BoolDialogTab::Writable if item.prop.writable => Some(index),
                    BoolDialogTab::ReadOnly if !item.prop.writable => Some(index),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if matches!(tab, BoolDialogTab::ReadOnly) {
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
    item: &BoolToggleItem,
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

fn action_params_command_preview(dialog: &BoolDialog) -> Option<String> {
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

fn prop_edit_command_preview(dialog: &BoolDialog) -> Option<String> {
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

fn prop_edit_get_command(dialog: &BoolDialog) -> Option<String> {
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

pub(in crate::tui) fn prop_dialog_title(dialog: &BoolDialog, lang: Language) -> String {
    let dialog_title = if dialog.device_name.trim().is_empty() {
        dialog.device_did.as_str()
    } else {
        dialog.device_name.as_str()
    };
    if dialog.loading || dialog.refreshing {
        format!(
            "{dialog_title} ({})",
            lang_str(lang, "刷新中...", "Loading...")
        )
    } else {
        dialog_title.to_string()
    }
}

pub(in crate::tui) fn prop_editor_header_lines(dialog: &BoolDialog, lang: Language) -> Vec<String> {
    if dialog.active_tab == BoolDialogTab::ReadOnly {
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
    if dialog.active_tab == BoolDialogTab::Actions {
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
    dialog: &BoolDialog,
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
    dialog: &BoolDialog,
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
    dialog: &BoolDialog,
    inner: Rect,
    lang: Language,
) -> PropEditorLayout {
    let header_lines = prop_editor_header_lines(dialog, lang)
        .iter()
        .map(|line| wrapped_text_line_count(line, inner.width))
        .sum::<u16>();
    let action_rows = action_param_rows_for_dialog(dialog);
    let editor_lines = if dialog.active_tab == BoolDialogTab::Actions {
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
    } else if dialog.active_tab == BoolDialogTab::ReadOnly {
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

    let needs_header_editor_gap = dialog.active_tab == BoolDialogTab::Actions;
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

fn prop_editor_bottom_lines(dialog: &BoolDialog, lang: Language) -> Vec<String> {
    if dialog.active_tab == BoolDialogTab::ReadOnly {
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
    if dialog.active_tab != BoolDialogTab::Actions {
        if let Some(command) = prop_edit_get_command(dialog) {
            bottom_lines.push(format!(
                "{}: {command}",
                lang_str(lang, "CLI 命令(读取)", "CLI Command (Read)")
            ));
        }
    }
    let command_preview = if dialog.active_tab == BoolDialogTab::Actions {
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

fn collect_readable_props(spec: &Value, lang: Language) -> Vec<BoolPropItem> {
    let mut out = Vec::new();
    let Some(services) = spec.get("services").and_then(Value::as_array) else {
        return out;
    };
    for service in services {
        let siid = service.get("iid").and_then(Value::as_i64).unwrap_or(0);
        let service_name = spec_node_label(service, lang)
            .map(str::trim)
            .filter(|text| !text.is_empty());
        let Some(properties) = service.get("properties").and_then(Value::as_array) else {
            continue;
        };
        for prop in properties {
            let format = prop
                .get("format")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let access = prop
                .get("access")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let readable = access.iter().filter_map(Value::as_str).any(|value| {
                value.eq_ignore_ascii_case("read") || value.eq_ignore_ascii_case("rw")
            });
            if !readable {
                continue;
            }
            let writable = access.iter().filter_map(Value::as_str).any(|value| {
                value.eq_ignore_ascii_case("write") || value.eq_ignore_ascii_case("rw")
            });
            let value_options = prop
                .get("value-list")
                .and_then(Value::as_array)
                .map(|options| {
                    options
                        .iter()
                        .filter_map(|entry| {
                            let value = entry.get("value")?.clone();
                            let label = spec_node_label(entry, lang)
                                .map(str::trim)
                                .filter(|text| !text.is_empty())
                                .map(ToString::to_string)
                                .unwrap_or_else(|| {
                                    serde_json::to_string(&value)
                                        .unwrap_or_else(|_| "null".to_string())
                                });
                            Some(BoolPropValueOption { value, label })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let piid = prop.get("iid").and_then(Value::as_i64).unwrap_or(0);
            if siid <= 0 || piid <= 0 {
                continue;
            }
            let prop_name = spec_node_label(prop, lang)
                .map(str::trim)
                .unwrap_or_default()
                .to_string();
            let name = if let Some(service_name) = service_name {
                if prop_name.is_empty() {
                    service_name.to_string()
                } else {
                    format!("{service_name} / {prop_name}")
                }
            } else if prop_name.is_empty() {
                format!("property {siid}/{piid}")
            } else {
                prop_name
            };
            out.push(BoolPropItem {
                siid,
                piid,
                name,
                format: format.to_string(),
                writable,
                value_options,
            });
        }
    }
    out
}

#[cfg(test)]
fn parse_bool_prop_value(raw: &Value) -> Option<bool> {
    let first = extract_prop_value(raw)?;
    match first {
        Value::Bool(value) => Some(value),
        Value::Number(n) => Some(n.as_i64().unwrap_or(0) != 0),
        Value::String(text) => match text.trim() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn extract_prop_value(raw: &Value) -> Option<Value> {
    let first = if let Some(array_value) = raw
        .as_array()
        .and_then(|list| list.first())
        .and_then(|entry| entry.get("value"))
        .cloned()
    {
        array_value
    } else if let Some(object_value) = raw.get("value").cloned() {
        object_value
    } else {
        raw.clone()
    };
    Some(first)
}

fn extract_actions_from_spec(spec: &Value, lang: Language) -> Vec<ActionItem> {
    let mut actions = Vec::new();
    if let Some(services) = spec.get("services").and_then(|v| v.as_array()) {
        for service in services {
            let siid = service.get("iid").and_then(|v| v.as_i64()).unwrap_or(0);
            let property_by_iid = service
                .get("properties")
                .and_then(Value::as_array)
                .map(|properties| {
                    properties
                        .iter()
                        .filter_map(|p| {
                            let piid = p.get("iid").and_then(Value::as_i64)?;
                            let name = spec_node_label(p, lang)
                                .map(str::trim)
                                .filter(|text| !text.is_empty())
                                .unwrap_or("")
                                .to_string();
                            let format = p
                                .get("format")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string();
                            let value_options = p
                                .get("value-list")
                                .and_then(Value::as_array)
                                .map(|options| {
                                    options
                                        .iter()
                                        .filter_map(|entry| {
                                            let value = entry.get("value")?.clone();
                                            let label = spec_node_label(entry, lang)
                                                .map(str::trim)
                                                .filter(|text| !text.is_empty())
                                                .map(ToString::to_string)
                                                .unwrap_or_else(|| {
                                                    serde_json::to_string(&value)
                                                        .unwrap_or_else(|_| "null".to_string())
                                                });
                                            Some(BoolPropValueOption { value, label })
                                        })
                                        .collect::<Vec<_>>()
                                })
                                .unwrap_or_default();
                            Some((
                                piid,
                                BoolPropItem {
                                    siid,
                                    piid,
                                    name,
                                    format,
                                    writable: false,
                                    value_options,
                                },
                            ))
                        })
                        .collect::<HashMap<i64, BoolPropItem>>()
                })
                .unwrap_or_default();
            if let Some(service_actions) = service.get("actions").and_then(|v| v.as_array()) {
                for action in service_actions {
                    if let Some(aiid) = action.get("iid").and_then(|v| v.as_i64()) {
                        let name = spec_node_label(action, lang)
                            .unwrap_or(lang_str(lang, "未知操作", "Unknown Action"))
                            .to_string();
                        let input_piids = action
                            .get("in")
                            .and_then(|v| v.as_array())
                            .map(|values| {
                                values.iter().filter_map(Value::as_i64).collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        let input_labels = input_piids
                            .iter()
                            .enumerate()
                            .map(|(idx, piid)| {
                                property_by_iid
                                    .get(piid)
                                    .map(|prop| prop.name.clone())
                                    .unwrap_or_else(|| format!("参数{}", idx + 1))
                            })
                            .collect::<Vec<_>>();
                        let input_props = input_piids
                            .iter()
                            .enumerate()
                            .map(|(idx, piid)| {
                                property_by_iid.get(piid).cloned().unwrap_or(BoolPropItem {
                                    siid,
                                    piid: *piid,
                                    name: input_labels[idx].clone(),
                                    format: String::new(),
                                    writable: false,
                                    value_options: Vec::new(),
                                })
                            })
                            .collect::<Vec<_>>();
                        actions.push(ActionItem {
                            siid,
                            aiid,
                            name,
                            input_piids,
                            input_labels,
                            input_props,
                        });
                    }
                }
            }
        }
    }
    actions
}

fn parse_prop_input_value(text: &str) -> Result<Value> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        bail!("输入不能为空");
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Ok(value);
    }
    Ok(Value::String(trimmed.to_string()))
}

fn format_prop_value_for_dialog(value: &Value) -> String {
    if is_error_with_negative_code(value) {
        return "-".to_string();
    }
    if let Some(text) = value.as_str() {
        return decode_backslash_x_utf8(text).unwrap_or_else(|| text.to_string());
    }
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

/// Check if a value is an error object with a negative error code.
/// Returns true if value is an object with "code" < 0 and a non-empty "did" string.
fn is_error_with_negative_code(value: &Value) -> bool {
    if let Some(obj) = value.as_object() {
        let has_did = obj
            .get("did")
            .and_then(Value::as_str)
            .is_some_and(|did| !did.trim().is_empty());
        let code_is_negative = obj
            .get("code")
            .and_then(Value::as_i64)
            .is_some_and(|code| code < 0);
        return has_did && code_is_negative;
    }
    false
}

fn decode_backslash_x_utf8(text: &str) -> Option<String> {
    if !text.contains("\\x") {
        return None;
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0_usize;
    let mut decoded_any = false;
    while index < bytes.len() {
        if bytes[index] == b'\\'
            && index + 3 < bytes.len()
            && matches!(bytes[index + 1], b'x' | b'X')
            && bytes[index + 2].is_ascii_hexdigit()
            && bytes[index + 3].is_ascii_hexdigit()
        {
            let high = hex_nibble(bytes[index + 2])?;
            let low = hex_nibble(bytes[index + 3])?;
            out.push((high << 4) | low);
            index += 4;
            decoded_any = true;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    if !decoded_any {
        return None;
    }
    String::from_utf8(out).ok()
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn read_device_categories_from_template(
    home_dir: &std::path::Path,
    lang: Language,
) -> Result<HashMap<String, String>> {
    let specs = specs_dir(home_dir);
    let template_path = specs.join("sources").join("template_list_device.json");
    if !template_path.exists() {
        return Ok(HashMap::new());
    }
    let template_raw = fs::read_to_string(template_path)?;
    let template_payload: Value = serde_json::from_str(&template_raw)?;
    let Some(template_entries) = template_payload.get("result").and_then(Value::as_array) else {
        return Ok(HashMap::new());
    };

    let mut direct_model_categories = HashMap::new();
    let mut type_categories = HashMap::new();
    for entry in template_entries {
        let category =
            category_from_template_entry(entry, lang).unwrap_or_else(|| "-".to_string());

        if let Some(model) = entry
            .get("model")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            direct_model_categories.insert(model.to_string(), category.clone());
        }

        if let Some(urn_type) = entry
            .get("type")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            type_categories.insert(urn_type.to_string(), category);
        }
    }

    let model_index_path = specs.join("index.json");
    if !model_index_path.exists() || type_categories.is_empty() {
        return Ok(direct_model_categories);
    }
    let model_index_raw = fs::read_to_string(model_index_path)?;
    let model_index_payload: Value = serde_json::from_str(&model_index_raw)?;
    let Some(model_entries) = model_index_payload.as_object() else {
        return Ok(direct_model_categories);
    };

    let mut resolved = direct_model_categories;
    for (model, metadata) in model_entries {
        if resolved.contains_key(model) {
            continue;
        }
        let Some(urn) = metadata
            .get("urn")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        if let Some((_, category)) = type_categories
            .iter()
            .filter(|(template_urn, _)| urn.starts_with(template_urn.as_str()))
            .max_by_key(|(template_urn, _)| template_urn.len())
        {
            resolved.insert(model.clone(), category.clone());
        }
    }
    Ok(resolved)
}

fn category_from_template_entry(entry: &Value, lang: Language) -> Option<String> {
    let keys: &[&str] = match lang {
        Language::Chinese => &["zh_cn", "en"],
        Language::English => &["en", "zh_cn"],
    };
    entry
        .get("description")
        .and_then(Value::as_object)
        .and_then(|description| {
            keys.iter()
                .find_map(|key| description.get(*key).and_then(Value::as_str))
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            [
                "category_name",
                "categoryName",
                "category",
                "type_name",
                "typeName",
                "type",
            ]
            .into_iter()
            .find_map(|key| {
                entry
                    .get(key)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string)
            })
        })
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
