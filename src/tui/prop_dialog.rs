//! Prop dialog editing helpers: action-parameter rows, value selectors,
//! prop-edit state, tab visibility/titles, CLI command previews, and the
//! prop-editor layout/measurement used by the dialog renderer in pages::prop.
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui_textarea::{Input as TextAreaInput, Key as TextAreaKey};
use serde_json::Value;

use crate::storage::Language;

use super::shared::*;
use super::{
    format_prop_value_for_dialog, is_error_with_negative_code, is_raw_device_json_item, lang_str,
    operation_record_tab_indices, parse_prop_input_value, prop_dialog_active_tab_is_loading,
    readonly_prop_detail_command, single_line_textarea, statistics_tab_indices, ActionItem,
    PropDialog, PropDialogTab, PropItem, ToggleItem,
};

pub(in crate::tui) fn apply_single_line_textarea_key(
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

pub(in crate::tui) fn apply_action_param_row_key(
    dialog: &mut PropDialog,
    key: crossterm::event::KeyEvent,
) {
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

pub(in crate::tui) fn textarea_input_from_key_event(
    key: crossterm::event::KeyEvent,
) -> TextAreaInput {
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

pub(in crate::tui) fn action_param_rows_for_dialog(dialog: &PropDialog) -> Vec<String> {
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

pub(in crate::tui) fn set_action_param_rows_in_dialog(dialog: &mut PropDialog, rows: &[String]) {
    dialog.edit_buffer = rows.join("\n");
}

pub(in crate::tui) fn action_param_selector_options(
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

pub(in crate::tui) fn action_param_selector_index(
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

pub(in crate::tui) fn action_param_selector_value(
    dialog: &PropDialog,
    action: &ActionItem,
    index: usize,
    row: Option<&str>,
) -> Option<Value> {
    let options = action_param_selector_options(dialog, action, index)?;
    let idx = action_param_selector_index(dialog, action, index, options.as_slice(), row);
    options.get(idx).map(|(_, value)| value.clone())
}

pub(in crate::tui) fn action_param_default_row(
    dialog: &PropDialog,
    action: &ActionItem,
    index: usize,
) -> String {
    if let Some(value) = action_param_selector_value(dialog, action, index, None) {
        return serde_json::to_string(&value).unwrap_or_else(|_| "null".to_string());
    }
    String::new()
}

pub(in crate::tui) fn selector_option_line(
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

pub(in crate::tui) fn selector_option_index_for_offset(
    options: &[(String, Value)],
    offset: u16,
) -> Option<usize> {
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

pub(in crate::tui) fn selector_option_value_at_offset(
    options: &[(String, Value)],
    offset: u16,
) -> Option<Value> {
    let index = selector_option_index_for_offset(options, offset)?;
    options.get(index).map(|(_, value)| value.clone())
}

pub(in crate::tui) fn action_param_selector_line(
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

pub(in crate::tui) fn selector_options_for_prop(prop: &PropItem) -> Option<Vec<(String, Value)>> {
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

pub(in crate::tui) fn prop_edit_selector_options(
    item: &ToggleItem,
) -> Option<Vec<(String, Value)>> {
    selector_options_for_prop(&item.prop)
}

pub(in crate::tui) fn prop_edit_selector_index(
    item: &ToggleItem,
    options: &[(String, Value)],
) -> usize {
    options
        .iter()
        .position(|(_, value)| *value == item.value)
        .unwrap_or(0)
}

pub(in crate::tui) fn prop_edit_selector_is_active(dialog: &PropDialog) -> bool {
    if dialog.active_tab == PropDialogTab::Actions || !dialog.editing {
        return false;
    }
    dialog
        .items
        .get(dialog.selected)
        .and_then(prop_edit_selector_options)
        .is_some()
}

pub(in crate::tui) fn prop_edit_selector_value(
    dialog: &PropDialog,
    item: &ToggleItem,
) -> Option<Value> {
    let options = prop_edit_selector_options(item)?;
    if let Ok(current) = parse_prop_input_value(dialog.edit_buffer.as_str()) {
        if let Some((_, value)) = options.iter().find(|(_, value)| *value == current) {
            return Some(value.clone());
        }
    }
    let index = prop_edit_selector_index(item, options.as_slice());
    options.get(index).map(|(_, value)| value.clone())
}

pub(in crate::tui) fn cycle_prop_edit_selector(dialog: &mut PropDialog, forward: bool) -> bool {
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

pub(in crate::tui) fn prop_edit_selector_options_text(dialog: &PropDialog) -> Option<String> {
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

pub(in crate::tui) fn prop_edit_selector_line(
    dialog: &PropDialog,
    base_style: Style,
) -> Option<Line<'static>> {
    let item = dialog.items.get(dialog.selected)?;
    let options = prop_edit_selector_options(item)?;
    let active_value = prop_edit_selector_value(dialog, item);
    Some(selector_option_line(
        options.as_slice(),
        active_value.as_ref(),
        base_style,
    ))
}

pub(in crate::tui) fn visible_prop_dialog_tabs(dialog: &PropDialog) -> Vec<PropDialogTab> {
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
pub(in crate::tui) fn all_prop_dialog_tabs() -> [PropDialogTab; 5] {
    [
        PropDialogTab::Actions,
        PropDialogTab::Writable,
        PropDialogTab::ReadOnly,
        PropDialogTab::Logs,
        PropDialogTab::Statistics,
    ]
}

pub(in crate::tui) fn prop_dialog_tab_title(tab: PropDialogTab, lang: Language) -> &'static str {
    match tab {
        PropDialogTab::Actions => lang_str(lang, "快捷操作", "Quick Actions"),
        PropDialogTab::Writable => lang_str(lang, "修改参数", "Edit Properties"),
        PropDialogTab::ReadOnly => lang_str(lang, "只读属性", "Read-only Properties"),
        PropDialogTab::Logs => lang_str(lang, "操作记录", "Operation Records"),
        PropDialogTab::Statistics => lang_str(lang, "统计", "Stats"),
    }
}

pub(in crate::tui) fn numbered_prop_dialog_tab_titles(
    tabs: impl IntoIterator<Item = PropDialogTab>,
    lang: Language,
) -> Vec<String> {
    tabs.into_iter()
        .enumerate()
        .map(|(index, tab)| format!("{}:{}", index + 1, prop_dialog_tab_title(tab, lang)))
        .collect()
}

#[cfg(test)]
pub(in crate::tui) fn all_prop_dialog_tab_titles(lang: Language) -> Vec<String> {
    numbered_prop_dialog_tab_titles(all_prop_dialog_tabs(), lang)
}

pub(in crate::tui) fn visible_prop_dialog_tab_titles(
    dialog: &PropDialog,
    lang: Language,
) -> Vec<String> {
    numbered_prop_dialog_tab_titles(visible_prop_dialog_tabs(dialog), lang)
}

pub(in crate::tui) fn prop_dialog_indices_for_tab(
    dialog: &PropDialog,
    tab: PropDialogTab,
) -> Vec<usize> {
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

pub(in crate::tui) fn format_prop_dialog_list_item_line(
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

pub(in crate::tui) fn format_prop_dialog_action_list_item_line(
    action: &ActionItem,
    selected: bool,
) -> String {
    let selected_marker = if selected { ">" } else { " " };
    format!("{selected_marker} {}", action.name)
}

pub(in crate::tui) fn action_params_command_preview(dialog: &PropDialog) -> Option<String> {
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

pub(in crate::tui) fn prop_edit_command_preview(dialog: &PropDialog) -> Option<String> {
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

pub(in crate::tui) fn prop_edit_get_command(dialog: &PropDialog) -> Option<String> {
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

pub(in crate::tui) fn preview_param(text: &str) -> String {
    if text.chars().count() > 6 {
        "\"...\"".to_string()
    } else {
        text.to_string()
    }
}

pub(in crate::tui) fn preview_value_arg(value: &Value) -> String {
    let arg = serde_json::to_string(value).unwrap_or_else(|_| "null".to_string());
    if arg.chars().count() > 6 {
        "\"...\"".to_string()
    } else {
        arg
    }
}

pub(in crate::tui) fn format_preview_props_set_command(
    did: &str,
    siid: i64,
    piid: i64,
    value: &Value,
) -> String {
    format!(
        "mit props set {} {siid} {piid} {}",
        did,
        preview_value_arg(value)
    )
}

pub(in crate::tui) fn format_preview_props_act_command(
    did: &str,
    siid: i64,
    aiid: i64,
    values: &[Value],
) -> String {
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

pub(in crate::tui) fn format_preview_push_command(uid: &str, text: &str) -> String {
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

pub(in crate::tui) fn wrapped_text_line_count(text: &str, width: u16) -> u16 {
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

pub(in crate::tui) fn action_param_row_height(
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

pub(in crate::tui) fn prop_editor_bottom_lines(dialog: &PropDialog, lang: Language) -> Vec<String> {
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
