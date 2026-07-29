use crossterm::event::MouseEvent;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Borders;
use std::cell::RefCell;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{
    action_param_row_layouts, action_param_rows_for_dialog, active_tab_has_search,
    copy_text_to_clipboard, footer_display_text, footer_render_area,
    format_prop_dialog_list_item_line, fullscreen_dialog_inner_area, log_viewer_text_area,
    logs_visible_lines_for_display, note_copy_success, now_epoch_millis,
    operation_records_display_text, prop_dialog_indices_for_tab, prop_edit_textarea_area,
    prop_editor_bottom_lines, prop_editor_layout, push_message_cli_preview_line,
    push_message_command_area, push_message_textarea_area, raw_device_tab_text,
    rendered_textarea_lines, search_plain_line, searchable_main_layout, split_main_layout,
    tab_titles, AccountActionDialog, PropDialogTab, TuiApp,
};
use crate::storage::Language;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SelectionSurface {
    Logs,
    Footer,
    ReauthDialog,
    PropDialogStatus,
    PropDialogList,
    PropEditor,
    PropEditorFooter,
    PushMessageInput,
    PushMessageCommand,
    SearchInput,
}

#[derive(Clone, Debug)]
pub(crate) struct SelectionSnapshot {
    pub(crate) surface: SelectionSurface,
    pub(crate) area: Rect,
    pub(crate) lines: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct ActiveSelection {
    pub(crate) snapshot: SelectionSnapshot,
    pub(crate) anchor: (u16, u16),
    pub(crate) focus: (u16, u16),
    pub(crate) dragged: bool,
}

#[derive(Default)]
struct MouseSelectionState {
    active: Option<ActiveSelection>,
    selected: Option<ActiveSelection>,
    last_text: Option<String>,
}

thread_local! {
    static MOUSE_SELECTION_STATE: RefCell<MouseSelectionState> =
        RefCell::new(MouseSelectionState::default());
}

fn with_mouse_selection_state<T>(f: impl FnOnce(&mut MouseSelectionState) -> T) -> T {
    MOUSE_SELECTION_STATE.with(|state| f(&mut state.borrow_mut()))
}

pub(crate) fn selection_highlight_style() -> Style {
    Style::default().bg(Color::DarkGray).fg(Color::White)
}

pub(crate) fn active_row_style() -> Style {
    Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
}

pub(crate) fn table_header_style() -> Style {
    Style::default()
        .fg(Color::LightGreen)
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn all_borders() -> Borders {
    Borders::ALL
}

pub(crate) fn top_bottom_borders() -> Borders {
    Borders::TOP | Borders::BOTTOM
}

pub(crate) fn tab_index_for_column(column: u16, tabs_area: Rect, lang: Language) -> Option<usize> {
    tab_index_for_column_with_titles(column, tabs_area, &tab_titles(lang))
}

pub(crate) fn tab_index_for_column_with_titles<S: AsRef<str>>(
    column: u16,
    tabs_area: Rect,
    titles: &[S],
) -> Option<usize> {
    if tabs_area.width <= 2 {
        return None;
    }
    let inner_left = tabs_area.x.saturating_add(1);
    let inner_right_exclusive = tabs_area
        .x
        .saturating_add(tabs_area.width.saturating_sub(1));
    if column < inner_left || column >= inner_right_exclusive {
        return None;
    }

    let mut x = inner_left;
    for (idx, title) in titles.iter().enumerate() {
        if x >= inner_right_exclusive {
            break;
        }
        let tab_width = 2u16.saturating_add(display_width(title.as_ref()));
        let tab_end = x.saturating_add(tab_width).min(inner_right_exclusive);
        if column >= x && column < tab_end {
            return Some(idx);
        }
        x = tab_end;
        if idx + 1 < titles.len() && x < inner_right_exclusive {
            if column == x {
                return Some(idx);
            }
            x = x.saturating_add(1);
        }
    }
    None
}

pub(crate) fn display_width(text: &str) -> u16 {
    text.chars()
        .map(|ch| if ch.is_ascii() { 1 } else { 2 })
        .sum::<u16>()
}

pub(crate) fn shrink_largest_width(widths: &mut [usize]) -> bool {
    let Some((largest_idx, largest_value)) =
        widths.iter().enumerate().max_by_key(|(_, width)| **width)
    else {
        return false;
    };
    if *largest_value <= 1 {
        return false;
    }
    widths[largest_idx] -= 1;
    true
}

pub(crate) fn byte_index_for_cell(text: &str, cell: u16) -> usize {
    let mut seen = 0u16;
    let mut out = text.len();
    for (idx, ch) in text.char_indices() {
        if seen >= cell {
            out = idx;
            break;
        }
        seen = seen.saturating_add(ch.width().unwrap_or(0) as u16);
    }
    out
}

pub(crate) fn highlight_line_range(text: &str, start_col: u16, end_col: u16) -> Line<'static> {
    if start_col >= end_col {
        return Line::from(text.to_string());
    }
    let start_byte = byte_index_for_cell(text, start_col);
    let end_byte = byte_index_for_cell(text, end_col);
    if start_byte >= end_byte || start_byte >= text.len() {
        return Line::from(text.to_string());
    }
    let safe_end = end_byte.min(text.len());
    let (left, rest) = text.split_at(start_byte);
    let (middle, right) = rest.split_at(safe_end.saturating_sub(start_byte));
    Line::from(vec![
        Span::raw(left.to_string()),
        Span::styled(middle.to_string(), selection_highlight_style()),
        Span::raw(right.to_string()),
    ])
}

pub(crate) fn apply_selection_highlight_to_area(
    buffer: &mut ratatui::buffer::Buffer,
    area: Rect,
    lines: &[String],
    active: &ActiveSelection,
) {
    let style = selection_highlight_style();
    let bg = style.bg.unwrap_or(Color::Reset);
    let fg = style.fg.unwrap_or(Color::Reset);
    for (idx, line) in lines.iter().enumerate() {
        let Some((mut start, mut end)) = selected_cols_for_line(active, idx) else {
            continue;
        };
        let width = display_width(line);
        if end == u16::MAX {
            end = width;
        }
        start = start.min(width);
        end = end.min(width);
        for col in start..end {
            let x = area.x.saturating_add(col);
            let y = area.y.saturating_add(idx as u16);
            if x < area.x.saturating_add(area.width) && y < area.y.saturating_add(area.height) {
                let cell = &mut buffer[(x, y)];
                cell.bg = bg;
                cell.fg = fg;
            }
        }
    }
}

pub(crate) fn clamp_point_to_area(point: (u16, u16), area: Rect) -> (u16, u16) {
    let max_x = area.x.saturating_add(area.width.saturating_sub(1));
    let max_y = area.y.saturating_add(area.height.saturating_sub(1));
    (point.0.clamp(area.x, max_x), point.1.clamp(area.y, max_y))
}

pub(crate) fn selection_bounds(
    selection: &ActiveSelection,
) -> Option<((usize, u16), (usize, u16))> {
    let area = selection.snapshot.area;
    if area.width == 0 || area.height == 0 {
        return None;
    }
    let anchor = clamp_point_to_area(selection.anchor, area);
    let focus = clamp_point_to_area(selection.focus, area);
    let to_local = |point: (u16, u16)| -> (usize, u16) {
        (
            point.1.saturating_sub(area.y) as usize,
            point.0.saturating_sub(area.x),
        )
    };
    let mut start = to_local(anchor);
    let mut end = to_local(focus);
    if (start.0, start.1) > (end.0, end.1) {
        std::mem::swap(&mut start, &mut end);
    }
    if start == end {
        return None;
    }
    Some((start, end))
}

pub(crate) fn selected_cols_for_line(
    selection: &ActiveSelection,
    line_index: usize,
) -> Option<(u16, u16)> {
    let ((start_row, start_col), (end_row, end_col)) = selection_bounds(selection)?;
    if line_index < start_row || line_index > end_row {
        return None;
    }
    if start_row == end_row {
        return Some((start_col, end_col));
    }
    if line_index == start_row {
        return Some((start_col, u16::MAX));
    }
    if line_index == end_row {
        return Some((0, end_col));
    }
    Some((0, u16::MAX))
}

pub(crate) fn extract_text_from_selection(selection: &ActiveSelection) -> Option<String> {
    let ((start_row, start_col), (end_row, end_col)) = selection_bounds(selection)?;
    let line_count = selection.snapshot.lines.len();
    if line_count == 0 || start_row >= line_count {
        return None;
    }
    let last_row = line_count.saturating_sub(1);
    let clamped_end_row = end_row.min(last_row);
    let mut lines = Vec::new();
    for row in start_row..=clamped_end_row {
        let line = selection
            .snapshot
            .lines
            .get(row)
            .expect("row is clamped to snapshot lines");
        let mut start = 0u16;
        let mut end = display_width(line);
        if row == start_row {
            start = start_col.min(end);
        }
        if row == clamped_end_row && end_row <= last_row {
            end = end_col.min(end);
        }
        if end <= start {
            if start_row == end_row {
                return None;
            }
            lines.push(String::new());
            continue;
        }
        let start_byte = byte_index_for_cell(line, start);
        let end_byte = byte_index_for_cell(line, end);
        lines.push(line[start_byte..end_byte.min(line.len())].to_string());
    }
    let out = lines.join("\n");
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

pub(crate) fn copy_last_selection(app: &mut TuiApp) {
    let text = with_mouse_selection_state(|state| state.last_text.clone());
    let Some(text) = text else {
        app.log("no active text selection to copy".to_string());
        return;
    };
    if let Err(error) = copy_text_to_clipboard(&text) {
        app.log(format!("copy selection failed: {error}"));
    } else {
        note_copy_success(app);
    }
}

pub(crate) fn clear_selection_state() {
    with_mouse_selection_state(|state| {
        state.active = None;
        state.selected = None;
        state.last_text = None;
    });
}

pub(crate) fn point_in_selected_range(selection: &ActiveSelection, point: (u16, u16)) -> bool {
    let area = selection.snapshot.area;
    if point.1 < area.y
        || point.1 >= area.y.saturating_add(area.height)
        || point.0 < area.x
        || point.0 >= area.x.saturating_add(area.width)
    {
        return false;
    }
    let Some(((start_row, start_col), (end_row, end_col))) = selection_bounds(selection) else {
        return false;
    };
    let row = point.1.saturating_sub(area.y) as usize;
    let col = point.0.saturating_sub(area.x);
    if row < start_row || row > end_row {
        return false;
    }
    if start_row == end_row {
        return col >= start_col && col < end_col;
    }
    if row == start_row {
        return col >= start_col;
    }
    if row == end_row {
        return col < end_col;
    }
    true
}

pub(crate) fn click_outside_selected_range(mouse: MouseEvent) -> bool {
    with_mouse_selection_state(|state| {
        let Some(selected) = state.selected.as_ref() else {
            return false;
        };
        !point_in_selected_range(selected, (mouse.column, mouse.row))
    })
}

pub(crate) fn selection_snapshot_for_mouse(
    app: &TuiApp,
    mouse: MouseEvent,
    terminal_area: Rect,
) -> Option<SelectionSnapshot> {
    if let Some(dialog) = &app.account_action_dialog {
        match dialog {
            AccountActionDialog::Reauth { status, auth_url } => {
                let popup = centered_rect(74, 42, terminal_area);
                if mouse.row >= popup.y
                    && mouse.row < popup.y.saturating_add(popup.height)
                    && mouse.column >= popup.x
                    && mouse.column < popup.x.saturating_add(popup.width)
                {
                    return Some(SelectionSnapshot {
                        surface: SelectionSurface::ReauthDialog,
                        area: popup,
                        lines: vec![status.clone(), String::new(), auth_url.clone()],
                    });
                }
            }
            AccountActionDialog::PushMessage {
                uid, input, cursor, ..
            } => {
                let textarea_area = push_message_textarea_area(terminal_area, input);
                if mouse.row >= textarea_area.y
                    && mouse.row < textarea_area.y.saturating_add(textarea_area.height)
                    && mouse.column >= textarea_area.x
                    && mouse.column < textarea_area.x.saturating_add(textarea_area.width)
                {
                    return Some(SelectionSnapshot {
                        surface: SelectionSurface::PushMessageInput,
                        area: textarea_area,
                        lines: rendered_textarea_lines(input, *cursor, true, textarea_area),
                    });
                }
                let command_area = push_message_command_area(terminal_area, input);
                if mouse.row >= command_area.y
                    && mouse.row < command_area.y.saturating_add(command_area.height)
                    && mouse.column >= command_area.x
                    && mouse.column < command_area.x.saturating_add(command_area.width)
                {
                    return Some(SelectionSnapshot {
                        surface: SelectionSurface::PushMessageCommand,
                        area: command_area,
                        lines: vec![push_message_cli_preview_line(
                            uid.as_str(),
                            input.as_str(),
                            app.language,
                        )],
                    });
                }
            }
            AccountActionDialog::SettingsConfirm { .. } => {}
            AccountActionDialog::Menu { .. } => {}
            AccountActionDialog::UpdateAvailable { .. }
            | AccountActionDialog::UpdateRunning { .. }
            | AccountActionDialog::UpdateFinished { .. }
            | AccountActionDialog::ThirdCloudSync { .. } => {}
        }
    }

    if let Some(dialog) = &app.prop_dialog {
        let popup = terminal_area;
        let inner = fullscreen_dialog_inner_area(popup);
        if dialog.editing {
            let layout = prop_editor_layout(dialog, inner, app.language);
            let editor_area = layout.editor_area;
            if dialog.active_tab == PropDialogTab::Actions {
                let rows = action_param_rows_for_dialog(dialog);
                for row_layout in action_param_row_layouts(dialog, editor_area) {
                    if mouse.row >= row_layout.value_area.y
                        && mouse.row
                            < row_layout
                                .value_area
                                .y
                                .saturating_add(row_layout.value_area.height)
                        && mouse.column >= row_layout.value_area.x
                        && mouse.column
                            < row_layout
                                .value_area
                                .x
                                .saturating_add(row_layout.value_area.width)
                    {
                        let row_value = rows.get(row_layout.index).cloned().unwrap_or_default();
                        return Some(SelectionSnapshot {
                            surface: SelectionSurface::PropEditor,
                            area: row_layout.value_area,
                            lines: rendered_textarea_lines(
                                row_value.as_str(),
                                if dialog.writable_selected == row_layout.index {
                                    dialog.edit_cursor
                                } else {
                                    row_value.chars().count()
                                },
                                dialog.writable_selected == row_layout.index,
                                row_layout.value_area,
                            ),
                        });
                    }
                }
            } else {
                let textarea_area = prop_edit_textarea_area(dialog, editor_area);
                if mouse.row >= textarea_area.y
                    && mouse.row < textarea_area.y.saturating_add(textarea_area.height)
                    && mouse.column >= textarea_area.x
                    && mouse.column < textarea_area.x.saturating_add(textarea_area.width)
                {
                    return Some(SelectionSnapshot {
                        surface: SelectionSurface::PropEditor,
                        area: textarea_area,
                        lines: rendered_textarea_lines(
                            dialog.edit_buffer.as_str(),
                            dialog.edit_cursor,
                            true,
                            textarea_area,
                        ),
                    });
                }
            }
            if let Some(footer_area) = layout.footer_area {
                if mouse.row >= footer_area.y
                    && mouse.row < footer_area.y.saturating_add(footer_area.height)
                    && mouse.column >= footer_area.x
                    && mouse.column < footer_area.x.saturating_add(footer_area.width)
                {
                    return Some(SelectionSnapshot {
                        surface: SelectionSurface::PropEditorFooter,
                        area: footer_area,
                        lines: prop_editor_bottom_lines(dialog, app.language),
                    });
                }
            }
        } else if let Some(status) = &dialog.status {
            if mouse.row >= inner.y
                && mouse.row < inner.y.saturating_add(inner.height)
                && mouse.column >= inner.x
                && mouse.column < inner.x.saturating_add(inner.width)
            {
                let text =
                    format!("This device appears offline.\n\n{status}\n\nPress Esc to close");
                return Some(SelectionSnapshot {
                    surface: SelectionSurface::PropDialogStatus,
                    area: inner,
                    lines: text
                        .lines()
                        .map(|line| line.to_string())
                        .collect::<Vec<_>>(),
                });
            }
        } else {
            let sections = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Min(1)])
                .split(inner);
            let list_area = sections[1];
            if mouse.row >= list_area.y
                && mouse.row < list_area.y.saturating_add(list_area.height)
                && mouse.column >= list_area.x
                && mouse.column < list_area.x.saturating_add(list_area.width)
            {
                if dialog.active_tab == PropDialogTab::Logs {
                    return Some(SelectionSnapshot {
                        surface: SelectionSurface::PropDialogList,
                        area: list_area,
                        lines: operation_records_display_text(
                            dialog,
                            app.language,
                            app.accounts.as_slice(),
                        )
                        .lines()
                        .map(|line| line.to_string())
                        .collect(),
                    });
                }
                if dialog.active_tab == PropDialogTab::Statistics {
                    return Some(SelectionSnapshot {
                        surface: SelectionSurface::PropDialogList,
                        area: list_area,
                        lines: raw_device_tab_text(dialog, dialog.active_tab)
                            .lines()
                            .map(|line| line.to_string())
                            .collect(),
                    });
                }
                let active_indices = prop_dialog_indices_for_tab(dialog, dialog.active_tab);
                let selected_local = active_indices
                    .iter()
                    .position(|index| *index == dialog.selected)
                    .or_else(|| {
                        let preferred = match dialog.active_tab {
                            PropDialogTab::Writable => dialog.writable_selected,
                            PropDialogTab::ReadOnly => dialog.readonly_selected,
                            PropDialogTab::Actions => dialog.actions_selected,
                            PropDialogTab::Logs | PropDialogTab::Statistics => dialog.selected,
                        };
                        active_indices.iter().position(|index| *index == preferred)
                    })
                    .or_else(|| (!active_indices.is_empty()).then_some(0));
                let lines = active_indices
                    .iter()
                    .enumerate()
                    .map(|(position, index)| {
                        let item = &dialog.items[*index];
                        format_prop_dialog_list_item_line(
                            item,
                            Some(position) == selected_local,
                            dialog.loading,
                            app.language,
                        )
                    })
                    .collect::<Vec<_>>();
                return Some(SelectionSnapshot {
                    surface: SelectionSurface::PropDialogList,
                    area: list_area,
                    lines,
                });
            }
        }
    }

    let [_tabs_area, list_area, _status_gap_area, status_bar_area] =
        split_main_layout(terminal_area);
    let footer_area = footer_render_area(status_bar_area);

    if mouse.row >= footer_area.y
        && mouse.row < footer_area.y.saturating_add(footer_area.height)
        && mouse.column >= footer_area.x
        && mouse.column < footer_area.x.saturating_add(footer_area.width)
    {
        return Some(SelectionSnapshot {
            surface: SelectionSurface::Footer,
            area: footer_area,
            lines: vec![footer_display_text(app, now_epoch_millis())],
        });
    }

    if active_tab_has_search(app.active_tab) {
        let [search_area, _search_border_area, _rest_area] = searchable_main_layout(list_area);
        if mouse.row >= search_area.y
            && mouse.row < search_area.y.saturating_add(search_area.height)
            && mouse.column >= search_area.x
            && mouse.column < search_area.x.saturating_add(search_area.width)
        {
            return Some(SelectionSnapshot {
                surface: SelectionSurface::SearchInput,
                area: search_area,
                lines: vec![search_plain_line(app, search_area.width)],
            });
        }
    }

    if app.active_tab == 2 && {
        let [_search_area, _search_border_area, logs_area] = searchable_main_layout(list_area);
        let text_area = log_viewer_text_area(app, logs_area);
        mouse.row >= text_area.y
            && mouse.row < text_area.y.saturating_add(text_area.height)
            && mouse.column >= text_area.x
            && mouse.column < text_area.x.saturating_add(text_area.width)
    } {
        let [_search_area, _search_border_area, logs_area] = searchable_main_layout(list_area);
        let text_area = log_viewer_text_area(app, logs_area);
        return Some(SelectionSnapshot {
            surface: SelectionSurface::Logs,
            area: text_area,
            lines: logs_visible_lines_for_display(app, text_area),
        });
    }

    None
}

pub(crate) fn selection_start(app: &TuiApp, mouse: MouseEvent, terminal_area: Rect) -> bool {
    let snapshot = selection_snapshot_for_mouse(app, mouse, terminal_area);
    with_mouse_selection_state(|state| {
        if let Some(snapshot) = snapshot {
            let consume = !matches!(
                snapshot.surface,
                SelectionSurface::Footer
                    | SelectionSurface::PropDialogList
                    | SelectionSurface::PropEditor
                    | SelectionSurface::PropEditorFooter
                    | SelectionSurface::PushMessageInput
                    | SelectionSurface::PushMessageCommand
                    | SelectionSurface::SearchInput
            );
            let active = ActiveSelection {
                snapshot,
                anchor: (mouse.column, mouse.row),
                focus: (mouse.column, mouse.row),
                dragged: false,
            };
            state.active = Some(active.clone());
            state.selected = Some(active);
            consume
        } else {
            state.active = None;
            false
        }
    })
}

pub(crate) fn selection_drag(mouse: MouseEvent) -> bool {
    with_mouse_selection_state(|state| {
        let Some(active) = &mut state.active else {
            return false;
        };
        active.focus = (mouse.column, mouse.row);
        active.dragged = active.dragged || active.focus != active.anchor;
        state.selected = Some(active.clone());
        true
    })
}

pub(crate) fn selection_end_and_copy(app: &mut TuiApp, mouse: MouseEvent) -> bool {
    let active = with_mouse_selection_state(|state| {
        let mut active = state.active.take()?;
        active.focus = (mouse.column, mouse.row);
        active.dragged = active.dragged || active.focus != active.anchor;
        Some(active)
    });
    let Some(active) = active else {
        return false;
    };
    if !active.dragged {
        return false;
    }
    let copied_text = extract_text_from_selection(&active);
    with_mouse_selection_state(|state| {
        state.selected = Some(active.clone());
        state.last_text = copied_text.clone();
    });
    if let Some(text) = copied_text {
        if let Err(error) = copy_text_to_clipboard(&text) {
            app.log(format!("copy selection failed: {error}"));
        } else {
            note_copy_success(app);
        }
    }
    true
}

pub(crate) fn selected_surface() -> Option<ActiveSelection> {
    with_mouse_selection_state(|state| state.selected.clone())
}

pub(crate) fn display_truncate_pad(s: &str, width: usize) -> String {
    let mut result = String::new();
    let mut current_width = 0;
    for ch in s.chars() {
        let ch_w = UnicodeWidthStr::width(ch.encode_utf8(&mut [0u8; 4]));
        if current_width + ch_w > width {
            break;
        }
        result.push(ch);
        current_width += ch_w;
    }
    while current_width < width {
        result.push(' ');
        current_width += 1;
    }
    result
}

pub(crate) fn display_truncate_pad_with_ellipsis(s: &str, width: usize) -> String {
    if UnicodeWidthStr::width(s) <= width {
        return display_truncate_pad(s, width);
    }
    if width <= 3 {
        return ".".repeat(width);
    }
    let mut truncated = display_truncate_pad(s, width - 3);
    truncated.push_str("...");
    truncated
}

pub(crate) fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1]);
    horizontal[1]
}

pub(crate) fn expand_rect(area: Rect, horizontal: u16, vertical: u16, bounds: Rect) -> Rect {
    let min_x = area.x.saturating_sub(horizontal).max(bounds.x);
    let min_y = area.y.saturating_sub(vertical).max(bounds.y);
    let max_x = area
        .x
        .saturating_add(area.width)
        .saturating_add(horizontal)
        .min(bounds.x.saturating_add(bounds.width));
    let max_y = area
        .y
        .saturating_add(area.height)
        .saturating_add(vertical)
        .min(bounds.y.saturating_add(bounds.height));
    Rect::new(
        min_x,
        min_y,
        max_x.saturating_sub(min_x),
        max_y.saturating_sub(min_y),
    )
}
