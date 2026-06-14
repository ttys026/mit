//! Prop dialog editing helpers: action-parameter rows, value selectors,
//! prop-edit state, tab visibility/titles, CLI command previews, and the
//! prop-editor layout/measurement used by the dialog renderer in pages::prop.
use anyhow::{anyhow, bail, Result};
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::ListState;
use ratatui_textarea::{Input as TextAreaInput, Key as TextAreaKey};
use serde_json::{json, Value};
use std::sync::mpsc;
use std::thread;

use crate::mico_api::MicoClient;
use crate::spec_cache::load_spec;
use crate::storage::Language;

use super::pages::account as account_page;
use super::shared::*;
use super::{
    collect_readable_props, device_account_uid, extract_actions_from_spec, extract_prop_value,
    format_prop_value_for_dialog, is_error_with_negative_code, is_raw_device_json_item, lang_str,
    load_mijia_device_logs_json, load_mijia_device_logs_json_with_query,
    load_mijia_device_statistics_json, load_mijia_device_statistics_json_with_query,
    mijia_prop_key, mijia_statistics_key, operation_record_date_filter_for_dialog,
    operation_record_raw_value_mut, operation_record_tab_indices, parse_prop_input_value,
    prop_dialog_active_tab_is_loading, raw_device_logs_index, raw_device_logs_item,
    raw_device_statistics_index, raw_device_statistics_item, raw_device_statistics_value,
    readonly_prop_detail_command, set_all_operation_record_requests_loading,
    set_raw_device_loading_status, single_line_textarea, statistics_period_for_dialog,
    statistics_query_for_value, statistics_selected_key, statistics_tab_indices, ActionItem,
    PropDialog, PropDialogRefreshMessage, PropDialogTab, PropItem, ToggleItem, TuiApp,
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

impl TuiApp {
    pub(in crate::tui) fn prop_dialog_has_toggle_items(&self) -> bool {
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

    pub(in crate::tui) fn normalize_prop_dialog_tab_state(&mut self) {
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

    pub(in crate::tui) fn switch_prop_dialog_tab(&mut self, forward: bool) {
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

    pub(in crate::tui) fn set_prop_dialog_tab_by_visible_order(&mut self, key: char) {
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

    pub(in crate::tui) fn set_prop_dialog_tab(&mut self, target: PropDialogTab) {
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
            dialog.statistics_selected_bar = None;
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

    pub(in crate::tui) fn select_prop_dialog_row(&mut self, row: usize) -> bool {
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

    pub(in crate::tui) fn get_prop_dialog_row_index(&self, row: usize) -> Option<usize> {
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

    pub(in crate::tui) fn activate_selected_prop(&mut self) {
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

    pub(in crate::tui) fn start_selected_readonly_prop_detail(&mut self) -> Result<()> {
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

    pub(in crate::tui) fn start_selected_action_params_edit(&mut self) -> Result<()> {
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

    pub(in crate::tui) fn start_selected_prop_edit(&mut self) -> Result<()> {
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

    pub(in crate::tui) fn cancel_prop_edit(&mut self) {
        if let Some(dialog) = &mut self.prop_dialog {
            dialog.editing = false;
            dialog.edit_buffer.clear();
            dialog.edit_cursor = 0;
            dialog.edit_error = None;
        }
    }

    pub(in crate::tui) fn prop_edit_push(&mut self, ch: char) {
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

    pub(in crate::tui) fn prop_edit_backspace(&mut self) {
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

    pub(in crate::tui) fn prop_edit_move_left(&mut self) {
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

    pub(in crate::tui) fn prop_edit_move_right(&mut self) {
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

    pub(in crate::tui) fn set_prop_edit_error(&mut self, message: String) {
        if let Some(dialog) = &mut self.prop_dialog {
            if dialog.editing {
                dialog.edit_error = Some(message);
            }
        }
    }

    pub(in crate::tui) fn prop_edit_cycle_selector(&mut self, forward: bool) {
        if let Some(dialog) = &mut self.prop_dialog {
            if !dialog.editing || dialog.active_tab == PropDialogTab::Actions {
                return;
            }
            let _ = cycle_prop_edit_selector(dialog, forward);
        }
    }

    pub(in crate::tui) fn submit_selected_prop_edit(&mut self) -> Result<()> {
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

    pub(in crate::tui) fn next_action_param_focus(&mut self) {
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

    pub(in crate::tui) fn prev_action_param_focus(&mut self) {
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

    pub(in crate::tui) fn focus_action_param_row(&mut self, row: usize) {
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

    pub(in crate::tui) fn submit_selected_action_params_edit(&mut self) -> Result<()> {
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

    pub(in crate::tui) fn open_prop_dialog(&mut self) -> Result<()> {
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
            statistics_selected_bar: None,
        });
        self.log(format!(
            "opened property dialog for {} ({})",
            selected.name, selected.did
        ));
        Ok(())
    }

    pub(in crate::tui) fn request_prop_dialog_refresh(&mut self) {
        self.request_prop_dialog_refresh_inner(false);
    }

    pub(in crate::tui) fn request_prop_dialog_refresh_allow_editing(&mut self) {
        self.request_prop_dialog_refresh_inner(true);
    }

    pub(in crate::tui) fn request_prop_dialog_refresh_inner(&mut self, allow_editing: bool) {
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

    pub(in crate::tui) fn process_prop_dialog_loading(&mut self) {
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
}
