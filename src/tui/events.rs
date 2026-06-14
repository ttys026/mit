//! Input dispatch: keyboard and mouse event handling that routes to the
//! appropriate TuiApp methods and footer operations, plus settings labels.
use crossterm::event::{
    self, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use std::time::Duration;

use anyhow::Result;

use crate::storage::Language;

use super::pages::account as account_page;
use super::shared::*;
use super::*;

pub(in crate::tui) fn handle_key(
    app: &mut TuiApp,
    key: crossterm::event::KeyEvent,
) -> Result<bool> {
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

pub(in crate::tui) fn settings_action_label(
    action: SettingsAction,
    lang: Language,
) -> &'static str {
    match action {
        SettingsAction::ClearCacheKeepAuth => lang_str(lang, "重置设备缓存", "Reset Device Cache"),
        SettingsAction::ResetAll => lang_str(lang, "重置全部设置", "Reset All Settings"),
        SettingsAction::ToggleLanguage | SettingsAction::ToggleAutoSubscribeDeviceStatus => {
            unreachable!("Toggle settings have no confirm dialog")
        }
    }
}

pub(in crate::tui) fn auto_subscribe_device_status_label(lang: Language, enabled: bool) -> String {
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

pub(in crate::tui) fn settings_confirm_lines(
    action: SettingsAction,
    lang: Language,
) -> Vec<String> {
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

pub(in crate::tui) fn handle_mouse(
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

pub(in crate::tui) fn rect_contains(area: Rect, column: u16, row: u16) -> bool {
    row >= area.y
        && row < area.y.saturating_add(area.height)
        && column >= area.x
        && column < area.x.saturating_add(area.width)
}

pub(in crate::tui) fn drain_pending_input_events() -> Result<()> {
    while event::poll(Duration::from_millis(0))? {
        let _ = event::read()?;
    }
    Ok(())
}
