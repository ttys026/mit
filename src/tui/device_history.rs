//! Device history feature: the prop dialog's Logs (operation records) and
//! Statistics tabs — request/record data model, date filtering, paging,
//! period selection, and chart computation. Two views over one Mijia data model.
use ratatui::layout::Rect;
use serde_json::{json, Map, Value};
use time::macros::format_description;
use time::{Date, Duration as TimeDuration, Month, OffsetDateTime, UtcOffset};

use crate::mijia_api::DeviceHistoryQuery;
use crate::storage::{AuthAccount, Language};

use super::shared::*;
use super::{
    add_months_to_date, date_end_timestamp, date_start_timestamp, json_code_is_zero,
    json_compact_text, json_i64, json_text, lang_str, rect_contains, timestamp_to_local_date,
    today_local_date, PropDialog, PropDialogTab, PropItem, ToggleItem,
    OPERATION_RECORD_DATE_FORMAT, OPERATION_RECORD_DATE_PICKER_CALENDAR_HEIGHT,
    OPERATION_RECORD_DATE_PICKER_CALENDAR_WIDTH, OPERATION_RECORD_DATE_PICKER_PREFIX,
    OPERATION_RECORD_MENU_MARKER, OPERATION_RECORD_PAGE_LIMIT, OPERATION_RECORD_TIMESTAMP_FORMAT,
    RAW_LOG_FORMAT, RAW_STATISTICS_FORMAT, STATISTICS_DATE_PICKER_PREFIX,
    STATISTICS_KEY_MENU_MARKER, STATISTICS_PERIOD_MENU_MARKER,
};

pub(in crate::tui) fn mijia_prop_key(prop: &PropItem) -> String {
    format!("{}.{}", prop.siid, prop.piid)
}

pub(in crate::tui) fn mijia_statistics_key(prop: &PropItem) -> Option<String> {
    let name = prop.name.to_ascii_lowercase();
    let looks_like_power_consumption =
        name.contains("power consumption") || prop.name.contains("功耗");
    (looks_like_power_consumption && prop.format.eq_ignore_ascii_case("float"))
        .then(|| mijia_prop_key(prop))
}

pub(in crate::tui) fn raw_device_logs_item(value: Value) -> ToggleItem {
    raw_device_json_item("操作记录", RAW_LOG_FORMAT, value)
}

pub(in crate::tui) fn raw_device_statistics_item(value: Value) -> ToggleItem {
    raw_device_json_item("统计", RAW_STATISTICS_FORMAT, value)
}

pub(in crate::tui) fn raw_device_json_item(name: &str, format: &str, value: Value) -> ToggleItem {
    ToggleItem {
        prop: PropItem {
            siid: 0,
            piid: 0,
            name: name.to_string(),
            format: format.to_string(),
            writable: false,
            value_options: Vec::new(),
        },
        value,
    }
}

pub(in crate::tui) fn is_raw_device_logs_item(item: &ToggleItem) -> bool {
    item.prop.format == RAW_LOG_FORMAT
}

pub(in crate::tui) fn is_raw_device_statistics_item(item: &ToggleItem) -> bool {
    item.prop.format == RAW_STATISTICS_FORMAT
}

pub(in crate::tui) fn is_raw_device_json_item(item: &ToggleItem) -> bool {
    is_raw_device_logs_item(item) || is_raw_device_statistics_item(item)
}

pub(in crate::tui) fn raw_device_logs_index(dialog: &PropDialog) -> Option<usize> {
    dialog
        .items
        .iter()
        .enumerate()
        .find_map(|(index, item)| is_raw_device_logs_item(item).then_some(index))
}

pub(in crate::tui) fn raw_device_statistics_index(dialog: &PropDialog) -> Option<usize> {
    dialog
        .items
        .iter()
        .enumerate()
        .find_map(|(index, item)| is_raw_device_statistics_item(item).then_some(index))
}

pub(in crate::tui) fn operation_record_raw_value(dialog: &PropDialog) -> Option<&Value> {
    raw_device_logs_index(dialog)
        .and_then(|index| dialog.items.get(index))
        .map(|item| &item.value)
}

pub(in crate::tui) fn operation_record_raw_value_mut(
    dialog: &mut PropDialog,
) -> Option<&mut Value> {
    let index = raw_device_logs_index(dialog)?;
    dialog.items.get_mut(index).map(|item| &mut item.value)
}

pub(in crate::tui) fn raw_device_statistics_value(dialog: &PropDialog) -> Option<&Value> {
    raw_device_statistics_index(dialog)
        .and_then(|index| dialog.items.get(index))
        .map(|item| &item.value)
}

pub(in crate::tui) fn raw_device_statistics_value_mut(
    dialog: &mut PropDialog,
) -> Option<&mut Value> {
    let index = raw_device_statistics_index(dialog)?;
    dialog.items.get_mut(index).map(|item| &mut item.value)
}

pub(in crate::tui) fn value_object_mut(value: &mut Value) -> &mut Map<String, Value> {
    if !value.is_object() {
        *value = json!({});
    }
    value
        .as_object_mut()
        .expect("value was normalized to object")
}

pub(in crate::tui) fn set_raw_device_loading_status(value: &mut Value, loading: bool) {
    let object = value_object_mut(value);
    if loading {
        object.insert("status".to_string(), json!("loading"));
    } else {
        object.remove("status");
    }
}

pub(in crate::tui) fn operation_record_ui_mut(value: &mut Value) -> &mut Map<String, Value> {
    let object = value_object_mut(value);
    let ui = object.entry("ui").or_insert_with(|| json!({}));
    value_object_mut(ui)
}

pub(in crate::tui) fn statistics_ui_mut(value: &mut Value) -> &mut Map<String, Value> {
    let object = value_object_mut(value);
    let ui = object.entry("ui").or_insert_with(|| json!({}));
    value_object_mut(ui)
}

pub(in crate::tui) fn operation_record_selected_key(dialog: &PropDialog) -> Option<String> {
    operation_record_raw_value(dialog)
        .and_then(|value| value.get("ui"))
        .and_then(|ui| ui.get("selected_key"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| {
            dialog
                .items
                .get(dialog.selected)
                .filter(|item| !is_raw_device_json_item(item))
                .map(|item| mijia_prop_key(&item.prop))
        })
}

pub(in crate::tui) fn set_operation_record_selected_key(dialog: &mut PropDialog, key: &str) {
    if let Some(value) = operation_record_raw_value_mut(dialog) {
        let ui = operation_record_ui_mut(value);
        ui.insert("selected_key".to_string(), Value::String(key.to_string()));
        ui.insert("active_row".to_string(), json!(0));
    }
    if let Some(index) = dialog
        .items
        .iter()
        .enumerate()
        .filter(|(_, item)| !is_raw_device_json_item(item))
        .find_map(|(index, item)| (mijia_prop_key(&item.prop) == key).then_some(index))
    {
        dialog.selected = index;
    }
}

pub(in crate::tui) fn operation_record_active_row(dialog: &PropDialog) -> usize {
    operation_record_raw_value(dialog)
        .and_then(|value| value.get("ui"))
        .and_then(|ui| ui.get("active_row"))
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0)
}

pub(in crate::tui) fn set_operation_record_active_row(dialog: &mut PropDialog, row: usize) {
    if let Some(value) = operation_record_raw_value_mut(dialog) {
        operation_record_ui_mut(value).insert("active_row".to_string(), json!(row));
    }
}

pub(in crate::tui) fn operation_record_menu_is_open(dialog: &PropDialog) -> bool {
    dialog.editing
        && dialog.active_tab == PropDialogTab::Logs
        && dialog.edit_buffer == OPERATION_RECORD_MENU_MARKER
}

#[derive(Clone, Copy, Debug)]
pub(in crate::tui) struct OperationRecordDatePickerState {
    pub(in crate::tui) cursor: Date,
    pub(in crate::tui) pending_start: Option<Date>,
}

pub(in crate::tui) fn operation_record_date_picker_is_open(dialog: &PropDialog) -> bool {
    dialog.editing
        && dialog.active_tab == PropDialogTab::Logs
        && operation_record_date_picker_state(dialog).is_some()
}

pub(in crate::tui) fn encode_operation_record_date_picker(
    state: OperationRecordDatePickerState,
) -> String {
    let start = state
        .pending_start
        .map(|date| date.to_julian_day().to_string())
        .unwrap_or_else(|| "-".to_string());
    format!(
        "{}{}:{}",
        OPERATION_RECORD_DATE_PICKER_PREFIX,
        state.cursor.to_julian_day(),
        start
    )
}

pub(in crate::tui) fn operation_record_date_picker_state(
    dialog: &PropDialog,
) -> Option<OperationRecordDatePickerState> {
    let rest = dialog
        .edit_buffer
        .strip_prefix(OPERATION_RECORD_DATE_PICKER_PREFIX)?;
    let (cursor, start) = rest.split_once(':')?;
    let cursor = cursor
        .parse::<i32>()
        .ok()
        .and_then(|day| Date::from_julian_day(day).ok())?;
    let pending_start = if start == "-" {
        None
    } else {
        start
            .parse::<i32>()
            .ok()
            .and_then(|day| Date::from_julian_day(day).ok())
    };
    Some(OperationRecordDatePickerState {
        cursor,
        pending_start,
    })
}

pub(in crate::tui) fn set_operation_record_date_picker_state(
    dialog: &mut PropDialog,
    state: OperationRecordDatePickerState,
) {
    dialog.editing = true;
    dialog.edit_buffer = encode_operation_record_date_picker(state);
    dialog.edit_cursor = operation_record_active_row(dialog);
}

pub(in crate::tui) fn operation_record_date_picker_popup_area(terminal_area: Rect) -> Rect {
    let width = OPERATION_RECORD_DATE_PICKER_CALENDAR_WIDTH
        .saturating_add(2)
        .min(terminal_area.width);
    let height = OPERATION_RECORD_DATE_PICKER_CALENDAR_HEIGHT
        .saturating_add(2)
        .min(terminal_area.height);
    Rect::new(
        terminal_area
            .x
            .saturating_add(terminal_area.width.saturating_sub(width) / 2),
        terminal_area
            .y
            .saturating_add(terminal_area.height.saturating_sub(height) / 2),
        width,
        height,
    )
}

pub(in crate::tui) fn operation_record_date_picker_calendar_area(
    terminal_area: Rect,
) -> Option<Rect> {
    let popup = operation_record_date_picker_popup_area(terminal_area);
    if popup.width == 0 || popup.height == 0 {
        return None;
    }
    let width = OPERATION_RECORD_DATE_PICKER_CALENDAR_WIDTH.min(popup.width.saturating_sub(2));
    let height = OPERATION_RECORD_DATE_PICKER_CALENDAR_HEIGHT.min(popup.height.saturating_sub(2));
    if width == 0 || height == 0 {
        return None;
    }
    Some(Rect::new(
        popup.x.saturating_add(1),
        popup.y.saturating_add(1),
        width,
        height,
    ))
}

pub(in crate::tui) fn operation_record_date_at_position(
    terminal_area: Rect,
    dialog: &PropDialog,
    column: u16,
    row: u16,
) -> Option<Option<Date>> {
    let popup = operation_record_date_picker_popup_area(terminal_area);
    if !rect_contains(popup, column, row) {
        return None;
    }
    let Some(calendar_area) = operation_record_date_picker_calendar_area(terminal_area) else {
        return Some(None);
    };
    if !rect_contains(calendar_area, column, row) {
        return Some(None);
    }
    let Some(state) = operation_record_date_picker_state(dialog) else {
        return Some(None);
    };
    let relative_y = row.saturating_sub(calendar_area.y);
    if relative_y < 2 {
        return Some(None);
    }
    let week_index = relative_y.saturating_sub(2);
    if week_index >= 6 {
        return Some(None);
    }
    let relative_x = column.saturating_sub(calendar_area.x);
    let day_index = relative_x / 3;
    if day_index >= 7 {
        return Some(None);
    }
    let Ok(first_of_month) = Date::from_calendar_date(state.cursor.year(), state.cursor.month(), 1)
    else {
        return Some(None);
    };
    let start_offset = i64::from(first_of_month.weekday().number_days_from_sunday());
    let first_visible = first_of_month.saturating_sub(TimeDuration::days(start_offset));
    let offset_days = i64::from(week_index) * 7 + i64::from(day_index);
    Some(Some(
        first_visible.saturating_add(TimeDuration::days(offset_days)),
    ))
}

pub(in crate::tui) fn statistics_date_at_position(
    terminal_area: Rect,
    dialog: &PropDialog,
    column: u16,
    row: u16,
) -> Option<Option<Date>> {
    let popup = operation_record_date_picker_popup_area(terminal_area);
    if !rect_contains(popup, column, row) {
        return None;
    }
    let Some(calendar_area) = operation_record_date_picker_calendar_area(terminal_area) else {
        return Some(None);
    };
    if !rect_contains(calendar_area, column, row) {
        return Some(None);
    }
    let Some(state) = statistics_date_picker_state(dialog) else {
        return Some(None);
    };
    let relative_y = row.saturating_sub(calendar_area.y);
    if relative_y < 2 {
        return Some(None);
    }
    let week_index = relative_y.saturating_sub(2);
    if week_index >= 6 {
        return Some(None);
    }
    let relative_x = column.saturating_sub(calendar_area.x);
    let day_index = relative_x / 3;
    if day_index >= 7 {
        return Some(None);
    }
    let Ok(first_of_month) = Date::from_calendar_date(state.cursor.year(), state.cursor.month(), 1)
    else {
        return Some(None);
    };
    let start_offset = i64::from(first_of_month.weekday().number_days_from_sunday());
    let first_visible = first_of_month.saturating_sub(TimeDuration::days(start_offset));
    let offset_days = i64::from(week_index) * 7 + i64::from(day_index);
    Some(Some(
        first_visible.saturating_add(TimeDuration::days(offset_days)),
    ))
}

pub(in crate::tui) fn operation_record_date_filter(value: &Value) -> Option<(i64, i64)> {
    value.get("date_filter").and_then(|filter| {
        Some((
            json_i64(filter.get("time_start"))?,
            json_i64(filter.get("time_end"))?,
        ))
    })
}

pub(in crate::tui) fn operation_record_date_filter_for_dialog(
    dialog: &PropDialog,
) -> Option<(i64, i64)> {
    operation_record_raw_value(dialog).and_then(operation_record_date_filter)
}

pub(in crate::tui) fn operation_record_default_query(value: Option<&Value>) -> DeviceHistoryQuery {
    let mut query = DeviceHistoryQuery::recent(OPERATION_RECORD_PAGE_LIMIT);
    if let Some((time_start, time_end)) = value.and_then(operation_record_date_filter) {
        query.time_start = time_start;
        query.time_end = time_end;
    }
    query
}

pub(in crate::tui) fn clear_operation_record_date_filter_value(value: &mut Value) {
    if let Some(object) = value.as_object_mut() {
        object.remove("date_filter");
        if let Some(ui) = object.get_mut("ui").and_then(Value::as_object_mut) {
            ui.insert("active_row".to_string(), json!(0));
        }
    }
}

pub(in crate::tui) fn raw_device_tab_text(dialog: &PropDialog, tab: PropDialogTab) -> String {
    let item = dialog.items.iter().find(|item| match tab {
        PropDialogTab::Logs => is_raw_device_logs_item(item),
        PropDialogTab::Statistics => is_raw_device_statistics_item(item),
        _ => false,
    });
    item.map(|item| {
        serde_json::to_string_pretty(&item.value)
            .or_else(|_| serde_json::to_string(&item.value))
            .unwrap_or_else(|_| "null".to_string())
    })
    .unwrap_or_else(|| {
        if dialog.loading {
            "加载中...".to_string()
        } else {
            "暂无数据".to_string()
        }
    })
}

pub(in crate::tui) fn operation_record_tab_titles(
    dialog: &PropDialog,
    lang: Language,
) -> Vec<String> {
    operation_record_requests(dialog)
        .iter()
        .filter_map(|request| operation_record_request_key(request))
        .map(|key| operation_record_key_label(dialog, key, lang))
        .collect()
}

pub(in crate::tui) fn operation_record_selector_label(
    dialog: &PropDialog,
    lang: Language,
) -> String {
    let requests = operation_record_requests(dialog);
    let current = operation_record_selected_request(dialog, requests.as_slice())
        .and_then(operation_record_request_key)
        .map(|key| operation_record_key_label(dialog, key, lang))
        .unwrap_or_else(|| {
            if operation_record_logs_are_loading(dialog) {
                lang_str(lang, "加载中...", "Loading...").to_string()
            } else {
                lang_str(lang, "暂无操作记录", "No operation records").to_string()
            }
        });
    format!(
        "{}: {current} ▾",
        lang_str(lang, "S: 选择记录", "S: Record")
    )
}

pub(in crate::tui) fn operation_record_date_filter_label(
    dialog: &PropDialog,
    lang: Language,
) -> String {
    let Some((time_start, time_end)) = operation_record_date_filter_for_dialog(dialog) else {
        return lang_str(lang, "D: 选择日期范围", "D: Select Date Range").to_string();
    };
    let start = format_operation_record_date(time_start);
    let end = format_operation_record_date(time_end);
    format!("{start} - {end}")
}

pub(in crate::tui) fn operation_record_date_filter_area(
    selector_row_area: Rect,
    dialog: &PropDialog,
    lang: Language,
) -> Option<Rect> {
    let width = display_width(operation_record_date_filter_label(dialog, lang).as_str());
    if width == 0 || selector_row_area.width == 0 || selector_row_area.height == 0 {
        return None;
    }
    let width = width.min(selector_row_area.width);
    Some(Rect::new(
        selector_row_area
            .x
            .saturating_add(selector_row_area.width.saturating_sub(width)),
        selector_row_area.y,
        width,
        1,
    ))
}

pub(in crate::tui) fn operation_record_dropdown_width(dialog: &PropDialog, lang: Language) -> u16 {
    operation_record_tab_titles(dialog, lang)
        .into_iter()
        .map(|title| display_width(title.as_str()).saturating_add(6))
        .max()
        .unwrap_or(24)
        .max(24)
}

pub(in crate::tui) fn operation_record_selector_height(dialog: &PropDialog) -> u16 {
    if raw_device_logs_index(dialog).is_some() {
        2
    } else {
        0
    }
}

pub(in crate::tui) fn operation_record_dropdown_area(
    list_area: Rect,
    dialog: &PropDialog,
    lang: Language,
) -> Option<Rect> {
    let request_count = operation_record_requests(dialog).len();
    if request_count == 0 || list_area.height <= 2 {
        return None;
    }
    let height = request_count
        .saturating_add(2)
        .min(list_area.height.saturating_sub(2) as usize)
        .min(u16::MAX as usize) as u16;
    if height == 0 {
        return None;
    }
    Some(Rect::new(
        list_area.x,
        list_area.y.saturating_add(2),
        operation_record_dropdown_width(dialog, lang).min(list_area.width),
        height,
    ))
}

pub(in crate::tui) fn operation_records_display_text(
    dialog: &PropDialog,
    lang: Language,
    accounts: &[AuthAccount],
) -> String {
    let mut lines = Vec::new();
    if operation_record_selector_height(dialog) > 0 {
        lines.push(operation_record_selector_label(dialog, lang));
        lines.push(String::new());
    }
    lines.extend(operation_records_table_lines(dialog, lang, accounts));
    lines.join("\n")
}

pub(in crate::tui) fn operation_records_table_lines(
    dialog: &PropDialog,
    lang: Language,
    accounts: &[AuthAccount],
) -> Vec<String> {
    let logs_loading = operation_record_logs_are_loading(dialog);
    let requests = operation_record_requests(dialog);
    let Some(request) = operation_record_selected_request(dialog, requests.as_slice()) else {
        if logs_loading {
            return vec![lang_str(lang, "加载中...", "Loading...").to_string()];
        }
        return vec![lang_str(lang, "暂无操作记录", "No operation records").to_string()];
    };
    if let Some(error) = request.get("error").and_then(Value::as_str) {
        return vec![format!("{}: {error}", lang_str(lang, "错误", "Error"))];
    }
    let Some(response) = request.get("response") else {
        if logs_loading || operation_record_request_is_loading_more(request) {
            return vec![lang_str(lang, "加载中...", "Loading...").to_string()];
        }
        return vec![lang_str(lang, "暂无操作记录", "No operation records").to_string()];
    };
    if !json_code_is_zero(response.get("code")) {
        let code = json_i64(response.get("code"))
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_string());
        let message = json_text(response.get("message"))
            .or_else(|| json_text(response.get("desc")))
            .unwrap_or_default();
        return vec![format!(
            "{}: code={}{}",
            lang_str(lang, "错误", "Error"),
            code,
            if message.is_empty() {
                String::new()
            } else {
                format!(" message={message}")
            }
        )];
    }
    let Some(records) = response.get("result").and_then(Value::as_array) else {
        if logs_loading {
            return vec![lang_str(lang, "加载中...", "Loading...").to_string()];
        }
        return vec![lang_str(lang, "暂无操作记录", "No operation records").to_string()];
    };
    if records.is_empty() {
        if logs_loading {
            return vec![lang_str(lang, "加载中...", "Loading...").to_string()];
        }
        return vec![lang_str(lang, "暂无操作记录", "No operation records").to_string()];
    }

    let user_width = operation_record_user_column_width(records, accounts, lang);
    let mut lines = vec![format_operation_record_table_row(
        lang_str(lang, "用户", "User"),
        lang_str(lang, "时间", "Time"),
        lang_str(lang, "值", "Value"),
        user_width,
    )];
    lines.extend(records.iter().map(|record| {
        let time = format_operation_record_timestamp(record.get("time"));
        let value = json_text(record.get("value"))
            .unwrap_or_else(|| json_compact_text(record.get("value")).unwrap_or_default());
        let uid = json_text(record.get("uid"));
        let user = operation_record_user_label(accounts, uid.as_deref());
        format_operation_record_table_row(user.as_str(), time.as_str(), value.as_str(), user_width)
    }));
    if operation_record_request_is_loading_more(request) {
        lines.push(lang_str(lang, "加载中...", "Loading...").to_string());
    } else if operation_record_request_has_more(request) {
        lines.push(lang_str(lang, "加载更多", "Load More").to_string());
    } else if operation_record_request_no_more(request) {
        lines.push(lang_str(lang, "没有更多记录", "No More Records").to_string());
    }
    lines
}

pub(in crate::tui) fn operation_records_active_visual_index(dialog: &PropDialog) -> Option<usize> {
    let row_count = operation_record_selectable_row_count(dialog);
    if row_count == 0 {
        return None;
    }
    Some(operation_record_active_row(dialog).min(row_count - 1) + 1)
}

pub(in crate::tui) fn operation_record_user_column_width(
    records: &[Value],
    accounts: &[AuthAccount],
    lang: Language,
) -> usize {
    let header_width = display_width(lang_str(lang, "用户", "User")) as usize;
    records
        .iter()
        .filter_map(|record| json_text(record.get("uid")))
        .map(|uid| operation_record_user_label(accounts, Some(uid.as_str())))
        .map(|user| display_width(user.as_str()) as usize)
        .max()
        .unwrap_or(header_width)
        .max(header_width)
}

pub(in crate::tui) fn operation_record_requests(dialog: &PropDialog) -> Vec<&Value> {
    raw_device_logs_index(dialog)
        .and_then(|index| dialog.items.get(index))
        .and_then(|item| item.value.get("requests"))
        .and_then(Value::as_array)
        .map(|requests| requests.iter().collect())
        .unwrap_or_default()
}

pub(in crate::tui) fn operation_record_logs_are_loading(dialog: &PropDialog) -> bool {
    operation_record_raw_value(dialog)
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
        .is_some_and(|status| status == "loading")
}

pub(in crate::tui) fn raw_device_statistics_are_loading(dialog: &PropDialog) -> bool {
    raw_device_statistics_value(dialog)
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
        .is_some_and(|status| status == "loading")
}

pub(in crate::tui) fn prop_dialog_active_tab_is_loading(dialog: &PropDialog) -> bool {
    if dialog.loading {
        return true;
    }
    match dialog.active_tab {
        PropDialogTab::Logs => operation_record_logs_are_loading(dialog),
        PropDialogTab::Statistics => raw_device_statistics_are_loading(dialog),
        PropDialogTab::Writable | PropDialogTab::ReadOnly | PropDialogTab::Actions => {
            dialog.refreshing
        }
    }
}

pub(in crate::tui) fn operation_record_selected_request<'a>(
    dialog: &PropDialog,
    requests: &'a [&'a Value],
) -> Option<&'a Value> {
    if requests.is_empty() {
        return None;
    }
    let selected_key = operation_record_selected_key(dialog);
    selected_key
        .as_deref()
        .and_then(|key| {
            requests
                .iter()
                .copied()
                .find(|request| operation_record_request_key(request) == Some(key))
        })
        .or_else(|| requests.first().copied())
}

pub(in crate::tui) fn operation_record_selected_request_index(dialog: &PropDialog) -> usize {
    let requests = operation_record_requests(dialog);
    let selected_key = operation_record_selected_key(dialog);
    selected_key
        .as_deref()
        .and_then(|key| {
            requests
                .iter()
                .position(|request| operation_record_request_key(request) == Some(key))
        })
        .unwrap_or(0)
}

pub(in crate::tui) fn operation_record_request_key(request: &Value) -> Option<&str> {
    request.get("key").and_then(Value::as_str)
}

pub(in crate::tui) fn operation_record_key_label(
    dialog: &PropDialog,
    key: &str,
    _lang: Language,
) -> String {
    dialog
        .items
        .iter()
        .filter(|item| !is_raw_device_json_item(item))
        .find(|item| mijia_prop_key(&item.prop) == key)
        .map(|item| item.prop.name.clone())
        .unwrap_or_else(|| key.to_string())
}

pub(in crate::tui) fn operation_record_tab_indices(dialog: &PropDialog) -> Vec<usize> {
    let raw_index = raw_device_logs_index(dialog);
    let mut indices = operation_record_requests(dialog)
        .iter()
        .filter_map(|request| operation_record_request_key(request))
        .filter_map(|key| {
            dialog
                .items
                .iter()
                .enumerate()
                .filter(|(_, item)| !is_raw_device_json_item(item))
                .find_map(|(index, item)| (mijia_prop_key(&item.prop) == key).then_some(index))
                .or(raw_index)
        })
        .collect::<Vec<_>>();
    indices.dedup();
    if indices.is_empty() {
        if let Some(index) = raw_index {
            indices.push(index);
        }
    }
    indices
}

pub(in crate::tui) fn statistics_requests(dialog: &PropDialog) -> Vec<&Value> {
    raw_device_statistics_index(dialog)
        .and_then(|index| dialog.items.get(index))
        .and_then(|item| item.value.get("requests"))
        .and_then(Value::as_array)
        .map(|requests| requests.iter().collect())
        .unwrap_or_default()
}

pub(in crate::tui) fn statistics_request_key(request: &Value) -> Option<&str> {
    request.get("key").and_then(Value::as_str)
}

pub(in crate::tui) fn statistics_selected_key(dialog: &PropDialog) -> Option<String> {
    raw_device_statistics_value(dialog)
        .and_then(|value| value.get("ui"))
        .and_then(|ui| ui.get("selected_key"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| {
            dialog
                .items
                .get(dialog.selected)
                .filter(|item| !is_raw_device_json_item(item))
                .map(|item| mijia_prop_key(&item.prop))
        })
        .or_else(|| {
            statistics_requests(dialog)
                .first()
                .and_then(|request| statistics_request_key(request))
                .map(ToString::to_string)
        })
}

pub(in crate::tui) fn set_statistics_selected_key(dialog: &mut PropDialog, key: &str) {
    if let Some(value) = raw_device_statistics_value_mut(dialog) {
        statistics_ui_mut(value).insert("selected_key".to_string(), Value::String(key.to_string()));
    }
    if let Some(index) = dialog
        .items
        .iter()
        .enumerate()
        .filter(|(_, item)| !is_raw_device_json_item(item))
        .find_map(|(index, item)| (mijia_prop_key(&item.prop) == key).then_some(index))
    {
        dialog.selected = index;
    }
}

pub(in crate::tui) fn statistics_selected_request<'a>(
    dialog: &PropDialog,
    requests: &'a [&'a Value],
) -> Option<&'a Value> {
    if requests.is_empty() {
        return None;
    }
    let selected_key = statistics_selected_key(dialog);
    selected_key
        .as_deref()
        .and_then(|key| {
            requests
                .iter()
                .copied()
                .find(|request| statistics_request_key(request) == Some(key))
        })
        .or_else(|| requests.first().copied())
}

pub(in crate::tui) fn statistics_selected_request_index(dialog: &PropDialog) -> usize {
    let requests = statistics_requests(dialog);
    let selected_key = statistics_selected_key(dialog);
    selected_key
        .as_deref()
        .and_then(|key| {
            requests
                .iter()
                .position(|request| statistics_request_key(request) == Some(key))
        })
        .unwrap_or(0)
}

pub(in crate::tui) fn statistics_key_label(
    dialog: &PropDialog,
    key: &str,
    _lang: Language,
) -> String {
    dialog
        .items
        .iter()
        .filter(|item| !is_raw_device_json_item(item))
        .find(|item| mijia_prop_key(&item.prop) == key)
        .map(|item| item.prop.name.clone())
        .unwrap_or_else(|| key.to_string())
}

pub(in crate::tui) fn statistics_tab_titles(dialog: &PropDialog, lang: Language) -> Vec<String> {
    statistics_requests(dialog)
        .iter()
        .filter_map(|request| statistics_request_key(request))
        .map(|key| statistics_key_label(dialog, key, lang))
        .collect()
}

pub(in crate::tui) fn statistics_current_key_label(dialog: &PropDialog, lang: Language) -> String {
    let requests = statistics_requests(dialog);
    statistics_selected_request(dialog, requests.as_slice())
        .and_then(statistics_request_key)
        .map(|key| statistics_key_label(dialog, key, lang))
        .unwrap_or_else(|| {
            if raw_device_statistics_are_loading(dialog) {
                lang_str(lang, "加载中...", "Loading...").to_string()
            } else {
                lang_str(lang, "暂无统计", "No stats").to_string()
            }
        })
}

pub(in crate::tui) fn statistics_selector_label(dialog: &PropDialog, lang: Language) -> String {
    let current = statistics_current_key_label(dialog, lang);
    format!("{}: {current} ▾", lang_str(lang, "S: 统计项", "S: Stats"))
}

pub(in crate::tui) fn statistics_period_for_value(value: Option<&Value>) -> StatisticsPeriod {
    value
        .and_then(|value| value.get("ui"))
        .and_then(|ui| ui.get("period"))
        .and_then(Value::as_str)
        .and_then(StatisticsPeriod::from_key)
        .unwrap_or(StatisticsPeriod::Week)
}

pub(in crate::tui) fn statistics_period_for_dialog(dialog: &PropDialog) -> StatisticsPeriod {
    statistics_period_for_value(raw_device_statistics_value(dialog))
}

pub(in crate::tui) fn set_statistics_period(dialog: &mut PropDialog, period: StatisticsPeriod) {
    if let Some(value) = raw_device_statistics_value_mut(dialog) {
        statistics_ui_mut(value).insert("period".to_string(), json!(period.key()));
    }
}

pub(in crate::tui) fn statistics_period_label(dialog: &PropDialog, lang: Language) -> String {
    format!("{} ▾", statistics_period_for_dialog(dialog).label(lang))
}

pub(in crate::tui) fn statistics_date_filter(value: &Value) -> Option<(i64, i64)> {
    value.get("date_filter").and_then(|filter| {
        Some((
            json_i64(filter.get("time_start"))?,
            json_i64(filter.get("time_end"))?,
        ))
    })
}

pub(in crate::tui) fn statistics_date_filter_for_dialog(dialog: &PropDialog) -> Option<(i64, i64)> {
    raw_device_statistics_value(dialog).and_then(statistics_date_filter)
}

pub(in crate::tui) fn statistics_date_filter_label(dialog: &PropDialog, lang: Language) -> String {
    let Some((time_start, time_end)) = statistics_date_filter_for_dialog(dialog) else {
        return lang_str(lang, "D: 选择时间范围", "D: Select Range").to_string();
    };
    let start = format_operation_record_date(time_start);
    let end = format_operation_record_date(time_end);
    format!("{start} - {end}")
}

pub(in crate::tui) fn statistics_period_date_range(
    period: StatisticsPeriod,
    cursor: Date,
) -> (Date, Date) {
    match period {
        StatisticsPeriod::Week => {
            let start = cursor.saturating_sub(TimeDuration::days(i64::from(
                cursor.weekday().number_days_from_monday(),
            )));
            (start, start.saturating_add(TimeDuration::days(6)))
        }
        StatisticsPeriod::Month => {
            let start =
                Date::from_calendar_date(cursor.year(), cursor.month(), 1).unwrap_or(cursor);
            let next_month = add_months_to_date(start, 1);
            (start, next_month.saturating_sub(TimeDuration::DAY))
        }
        StatisticsPeriod::Year => {
            let start =
                Date::from_calendar_date(cursor.year(), Month::January, 1).unwrap_or(cursor);
            let end =
                Date::from_calendar_date(cursor.year(), Month::December, 31).unwrap_or(cursor);
            (start, end)
        }
    }
}

pub(in crate::tui) fn statistics_period_date_range_ending_today(
    period: StatisticsPeriod,
) -> (Date, Date) {
    let end = today_local_date();
    let start = match period {
        StatisticsPeriod::Week => end.saturating_sub(TimeDuration::days(7)),
        StatisticsPeriod::Month => end.saturating_sub(TimeDuration::days(30)),
        StatisticsPeriod::Year => end.saturating_sub(TimeDuration::days(365)),
    };
    (start, end)
}

pub(in crate::tui) fn statistics_query_limit(
    period: StatisticsPeriod,
    time_start: i64,
    time_end: i64,
) -> u32 {
    if time_end < time_start {
        return 1;
    }
    let days = (time_end - time_start) / 86_400 + 1;
    let count = match period {
        StatisticsPeriod::Week | StatisticsPeriod::Month => days,
        StatisticsPeriod::Year => {
            let start = timestamp_to_local_date(time_start);
            let end = timestamp_to_local_date(time_end);
            match (start, end) {
                (Some(start), Some(end)) => {
                    let start_month = i64::from(start.year()) * 12 + i64::from(start.month() as u8);
                    let end_month = i64::from(end.year()) * 12 + i64::from(end.month() as u8);
                    end_month.saturating_sub(start_month).saturating_add(1)
                }
                _ => 12,
            }
        }
    };
    count.clamp(1, u32::MAX as i64) as u32
}

pub(in crate::tui) fn statistics_default_query(period: StatisticsPeriod) -> DeviceHistoryQuery {
    let (start, end) = statistics_period_date_range_ending_today(period);
    let time_start = date_start_timestamp(start);
    let time_end = date_end_timestamp(end);
    DeviceHistoryQuery {
        time_start,
        time_end,
        limit: statistics_query_limit(period, time_start, time_end),
    }
}

pub(in crate::tui) fn statistics_query_for_value(value: Option<&Value>) -> DeviceHistoryQuery {
    let period = statistics_period_for_value(value);
    if let Some((time_start, time_end)) = value.and_then(statistics_date_filter) {
        return DeviceHistoryQuery {
            time_start,
            time_end,
            limit: statistics_query_limit(period, time_start, time_end),
        };
    }
    statistics_default_query(period)
}

pub(in crate::tui) fn set_statistics_date_filter_value(
    value: &mut Value,
    period: StatisticsPeriod,
    cursor: Date,
) {
    let (start, end) = statistics_period_date_range(period, cursor);
    set_statistics_date_filter_value_from_range(value, start, end);
}

pub(in crate::tui) fn set_statistics_date_filter_value_ending_today(
    value: &mut Value,
    period: StatisticsPeriod,
) {
    let (start, end) = statistics_period_date_range_ending_today(period);
    set_statistics_date_filter_value_from_range(value, start, end);
}

pub(in crate::tui) fn set_statistics_date_filter_value_from_range(
    value: &mut Value,
    start: Date,
    end: Date,
) {
    let object = value_object_mut(value);
    object.insert(
        "date_filter".to_string(),
        json!({
            "time_start": date_start_timestamp(start),
            "time_end": date_end_timestamp(end)
        }),
    );
}

pub(in crate::tui) fn statistics_period_options(lang: Language) -> Vec<String> {
    StatisticsPeriod::all()
        .into_iter()
        .map(|period| period.label(lang).to_string())
        .collect()
}

pub(in crate::tui) fn statistics_selector_height(dialog: &PropDialog) -> u16 {
    if raw_device_statistics_index(dialog).is_some() {
        2
    } else {
        0
    }
}

pub(in crate::tui) fn statistics_dropdown_width(dialog: &PropDialog, lang: Language) -> u16 {
    statistics_tab_titles(dialog, lang)
        .into_iter()
        .map(|title| display_width(title.as_str()).saturating_add(6))
        .max()
        .unwrap_or(24)
        .max(24)
}

pub(in crate::tui) fn statistics_dropdown_area(
    list_area: Rect,
    dialog: &PropDialog,
    lang: Language,
) -> Option<Rect> {
    let request_count = statistics_requests(dialog).len();
    if request_count <= 1 || list_area.height <= 2 {
        return None;
    }
    let height = request_count
        .saturating_add(2)
        .min(list_area.height.saturating_sub(2) as usize)
        .min(u16::MAX as usize) as u16;
    if height == 0 {
        return None;
    }
    Some(Rect::new(
        list_area.x,
        list_area.y.saturating_add(2),
        statistics_dropdown_width(dialog, lang).min(list_area.width),
        height,
    ))
}

pub(in crate::tui) fn statistics_date_filter_area(
    selector_row_area: Rect,
    dialog: &PropDialog,
    lang: Language,
) -> Option<Rect> {
    let width = display_width(statistics_date_filter_label(dialog, lang).as_str());
    if width == 0 || selector_row_area.width == 0 || selector_row_area.height == 0 {
        return None;
    }
    let width = width.min(selector_row_area.width);
    Some(Rect::new(
        selector_row_area
            .x
            .saturating_add(selector_row_area.width.saturating_sub(width)),
        selector_row_area.y,
        width,
        1,
    ))
}

pub(in crate::tui) fn statistics_period_area(
    selector_row_area: Rect,
    date_area: Option<Rect>,
    dialog: &PropDialog,
    lang: Language,
) -> Option<Rect> {
    let width = display_width(statistics_period_label(dialog, lang).as_str());
    if width == 0 || selector_row_area.width == 0 || selector_row_area.height == 0 {
        return None;
    }
    let right_edge = date_area
        .map(|area| area.x.saturating_sub(2))
        .unwrap_or_else(|| selector_row_area.x.saturating_add(selector_row_area.width));
    if right_edge <= selector_row_area.x {
        return None;
    }
    let width = width.min(right_edge.saturating_sub(selector_row_area.x));
    Some(Rect::new(
        right_edge.saturating_sub(width),
        selector_row_area.y,
        width,
        1,
    ))
}

pub(in crate::tui) fn statistics_period_dropdown_area(
    list_area: Rect,
    selector_row_area: Rect,
    dialog: &PropDialog,
    lang: Language,
) -> Option<Rect> {
    if list_area.height <= 2 {
        return None;
    }
    let date_area = statistics_date_filter_area(selector_row_area, dialog, lang);
    let period_area = statistics_period_area(selector_row_area, date_area, dialog, lang)?;
    let height = 5_u16.min(list_area.height.saturating_sub(2));
    if height == 0 {
        return None;
    }
    Some(Rect::new(
        period_area.x.saturating_sub(2),
        list_area.y.saturating_add(2),
        period_area.width.saturating_add(4).min(list_area.width),
        height,
    ))
}

pub(in crate::tui) fn statistics_key_menu_is_open(dialog: &PropDialog) -> bool {
    dialog.editing
        && dialog.active_tab == PropDialogTab::Statistics
        && dialog.edit_buffer == STATISTICS_KEY_MENU_MARKER
}

pub(in crate::tui) fn statistics_period_menu_is_open(dialog: &PropDialog) -> bool {
    dialog.editing
        && dialog.active_tab == PropDialogTab::Statistics
        && dialog.edit_buffer == STATISTICS_PERIOD_MENU_MARKER
}

pub(in crate::tui) fn encode_statistics_date_picker(
    state: OperationRecordDatePickerState,
) -> String {
    format!(
        "{}{}",
        STATISTICS_DATE_PICKER_PREFIX,
        state.cursor.to_julian_day()
    )
}

pub(in crate::tui) fn statistics_date_picker_state(
    dialog: &PropDialog,
) -> Option<OperationRecordDatePickerState> {
    let rest = dialog
        .edit_buffer
        .strip_prefix(STATISTICS_DATE_PICKER_PREFIX)?;
    let cursor = rest
        .parse::<i32>()
        .ok()
        .and_then(|day| Date::from_julian_day(day).ok())?;
    Some(OperationRecordDatePickerState {
        cursor,
        pending_start: None,
    })
}

pub(in crate::tui) fn statistics_date_picker_is_open(dialog: &PropDialog) -> bool {
    dialog.editing
        && dialog.active_tab == PropDialogTab::Statistics
        && statistics_date_picker_state(dialog).is_some()
}

pub(in crate::tui) fn set_statistics_date_picker_state(
    dialog: &mut PropDialog,
    state: OperationRecordDatePickerState,
) {
    dialog.editing = true;
    dialog.edit_buffer = encode_statistics_date_picker(state);
    dialog.edit_cursor = statistics_selected_request_index(dialog);
}

pub(in crate::tui) fn statistics_tab_indices(dialog: &PropDialog) -> Vec<usize> {
    let raw_index = raw_device_statistics_index(dialog);
    let mut indices = statistics_requests(dialog)
        .iter()
        .filter_map(|request| statistics_request_key(request))
        .filter_map(|key| {
            dialog
                .items
                .iter()
                .enumerate()
                .filter(|(_, item)| !is_raw_device_json_item(item))
                .find_map(|(index, item)| (mijia_prop_key(&item.prop) == key).then_some(index))
                .or(raw_index)
        })
        .collect::<Vec<_>>();
    indices.dedup();
    if indices.is_empty() {
        if let Some(index) = raw_index {
            indices.push(index);
        }
    }
    indices
}

#[derive(Clone, Debug)]
pub(in crate::tui) struct StatisticsChartPoint {
    pub(in crate::tui) label: String,
    pub(in crate::tui) value: f64,
    pub(in crate::tui) text_value: String,
}

pub(in crate::tui) fn statistics_chart_points(
    dialog: &PropDialog,
    lang: Language,
) -> Result<Vec<StatisticsChartPoint>, String> {
    let requests = statistics_requests(dialog);
    let Some(request) = statistics_selected_request(dialog, requests.as_slice()) else {
        if raw_device_statistics_are_loading(dialog) {
            return Err(lang_str(lang, "加载中...", "Loading...").to_string());
        }
        return Err(statistics_unsupported_message(lang));
    };
    if let Some(error) = request.get("error").and_then(Value::as_str) {
        return Err(format!("{}: {error}", lang_str(lang, "错误", "Error")));
    }
    let Some(response) = request.get("response") else {
        if raw_device_statistics_are_loading(dialog) {
            return Err(lang_str(lang, "加载中...", "Loading...").to_string());
        }
        return Err(statistics_unsupported_message(lang));
    };
    if !json_code_is_zero(response.get("code")) {
        let code = json_i64(response.get("code"))
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_string());
        let message = json_text(response.get("message"))
            .or_else(|| json_text(response.get("desc")))
            .unwrap_or_default();
        return Err(format!(
            "{}: code={}{}",
            lang_str(lang, "错误", "Error"),
            code,
            if message.is_empty() {
                String::new()
            } else {
                format!(" message={message}")
            }
        ));
    }
    let Some(records) = response.get("result").and_then(Value::as_array) else {
        return Err(statistics_unsupported_message(lang));
    };
    let period = statistics_period_for_dialog(dialog);
    let Some((range_start, range_end)) = statistics_chart_date_range(dialog, period) else {
        return Err(statistics_unsupported_message(lang));
    };
    if range_end < range_start {
        return Err(statistics_unsupported_message(lang));
    }
    let buckets = statistics_chart_buckets(period, range_start, range_end);
    if buckets.is_empty() {
        return Err(statistics_unsupported_message(lang));
    }
    let mut values = vec![0.0_f64; buckets.len()];
    for record in records {
        let Some(time) = json_i64(record.get("time")) else {
            continue;
        };
        let Some(value) = statistics_numeric_value(record.get("value")) else {
            continue;
        };
        let Some(date) = timestamp_to_local_date(time) else {
            continue;
        };
        let Some(index) =
            statistics_chart_bucket_index(period, range_start, range_end, buckets.len(), date)
        else {
            continue;
        };
        values[index] += value;
    }
    Ok(buckets
        .into_iter()
        .zip(values)
        .map(|(date, value)| StatisticsChartPoint {
            label: format_statistics_date_label(date, period),
            value,
            text_value: format_statistics_value(value),
        })
        .collect())
}

pub(in crate::tui) fn statistics_unsupported_message(lang: Language) -> String {
    lang_str(
        lang,
        "此设备不支持查看统计数据",
        "This device does not support stats",
    )
    .to_string()
}

pub(in crate::tui) fn statistics_chart_date_range(
    dialog: &PropDialog,
    period: StatisticsPeriod,
) -> Option<(Date, Date)> {
    if let Some((time_start, time_end)) = statistics_date_filter_for_dialog(dialog) {
        return Some((
            timestamp_to_local_date(time_start)?,
            timestamp_to_local_date(time_end)?,
        ));
    }
    let query = statistics_default_query(period);
    Some((
        timestamp_to_local_date(query.time_start)?,
        timestamp_to_local_date(query.time_end)?,
    ))
}

pub(in crate::tui) fn statistics_chart_buckets(
    period: StatisticsPeriod,
    start: Date,
    end: Date,
) -> Vec<Date> {
    let time_start = date_start_timestamp(start);
    let time_end = date_end_timestamp(end);
    let count = statistics_query_limit(period, time_start, time_end).max(1) as usize;
    match period {
        StatisticsPeriod::Week | StatisticsPeriod::Month => (0..count)
            .map(|offset| start.saturating_add(TimeDuration::days(offset as i64)))
            .take_while(|date| *date <= end)
            .collect(),
        StatisticsPeriod::Year => (0..count)
            .map(|index| {
                if index == 0 {
                    start
                } else if index == count.saturating_sub(1) {
                    end
                } else {
                    add_months_to_date(start, index as i32)
                }
            })
            .collect(),
    }
}

pub(in crate::tui) fn statistics_chart_bucket_index(
    period: StatisticsPeriod,
    start: Date,
    end: Date,
    bucket_count: usize,
    date: Date,
) -> Option<usize> {
    if bucket_count == 0 || date < start || date > end {
        return None;
    }
    let index = match period {
        StatisticsPeriod::Week | StatisticsPeriod::Month => {
            i64::from(date.to_julian_day() - start.to_julian_day())
        }
        StatisticsPeriod::Year => {
            let start_month = i64::from(start.year()) * 12 + i64::from(start.month() as u8);
            let date_month = i64::from(date.year()) * 12 + i64::from(date.month() as u8);
            date_month.saturating_sub(start_month)
        }
    };
    if index < 0 {
        return None;
    }
    Some((index as usize).min(bucket_count.saturating_sub(1)))
}

pub(in crate::tui) fn statistics_numeric_value(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => {
            let text = text.trim();
            text.parse::<f64>().ok().or_else(|| {
                serde_json::from_str::<Value>(text)
                    .ok()
                    .and_then(|value| statistics_numeric_value(Some(&value)))
            })
        }
        Value::Array(values) => values
            .first()
            .and_then(|value| statistics_numeric_value(Some(value))),
        _ => None,
    }
}

pub(in crate::tui) fn format_statistics_value(value: f64) -> String {
    if (value.fract()).abs() < f64::EPSILON {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

pub(in crate::tui) fn format_statistics_date_label(date: Date, period: StatisticsPeriod) -> String {
    match period {
        StatisticsPeriod::Year => date
            .format(format_description!("[year]-[month]"))
            .unwrap_or_else(|_| "-".to_string()),
        StatisticsPeriod::Week | StatisticsPeriod::Month => date
            .format(format_description!("[month]-[day]"))
            .unwrap_or_else(|_| "-".to_string()),
    }
}

pub(in crate::tui) fn operation_record_request_is_loading_more(request: &Value) -> bool {
    request
        .get("pagination")
        .and_then(|pagination| pagination.get("loading_more"))
        .and_then(Value::as_bool)
        .or_else(|| request.get("loading_more").and_then(Value::as_bool))
        .unwrap_or(false)
}

pub(in crate::tui) fn operation_record_request_has_more(request: &Value) -> bool {
    request
        .get("pagination")
        .and_then(|pagination| pagination.get("has_more"))
        .and_then(Value::as_bool)
        .or_else(|| request.get("has_more").and_then(Value::as_bool))
        .unwrap_or(false)
}

pub(in crate::tui) fn operation_record_request_no_more(request: &Value) -> bool {
    request
        .get("pagination")
        .and_then(|pagination| pagination.get("no_more"))
        .and_then(Value::as_bool)
        .or_else(|| request.get("no_more").and_then(Value::as_bool))
        .unwrap_or(false)
}

pub(in crate::tui) fn operation_record_request_records(request: &Value) -> &[Value] {
    request
        .get("response")
        .and_then(|response| response.get("result"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

pub(in crate::tui) fn operation_record_selectable_row_count(dialog: &PropDialog) -> usize {
    let requests = operation_record_requests(dialog);
    let Some(request) = operation_record_selected_request(dialog, requests.as_slice()) else {
        return 0;
    };
    let records = operation_record_request_records(request);
    let mut count = records.len();
    if operation_record_request_has_more(request)
        || operation_record_request_is_loading_more(request)
        || operation_record_request_no_more(request)
    {
        count = count.saturating_add(1);
    }
    count
}

pub(in crate::tui) fn operation_record_active_row_is_load_more(dialog: &PropDialog) -> bool {
    let requests = operation_record_requests(dialog);
    let Some(request) = operation_record_selected_request(dialog, requests.as_slice()) else {
        return false;
    };
    let records_len = operation_record_request_records(request).len();
    records_len > 0
        && operation_record_active_row(dialog) == records_len
        && operation_record_request_has_more(request)
        && !operation_record_request_is_loading_more(request)
}

pub(in crate::tui) fn format_operation_record_table_row(
    user: &str,
    time: &str,
    value: &str,
    user_width: usize,
) -> String {
    format!(
        "{}  {}  {}",
        display_truncate_pad(user, user_width),
        display_truncate_pad(time, 19),
        value
    )
}

pub(in crate::tui) fn format_operation_record_timestamp(value: Option<&Value>) -> String {
    let Some(mut seconds) = json_i64(value) else {
        return "-".to_string();
    };
    if seconds > 10_000_000_000 {
        seconds /= 1000;
    }
    let Ok(timestamp) = OffsetDateTime::from_unix_timestamp(seconds) else {
        return "-".to_string();
    };
    let timestamp = match UtcOffset::current_local_offset() {
        Ok(offset) => timestamp.to_offset(offset),
        Err(_) => timestamp,
    };
    timestamp
        .format(OPERATION_RECORD_TIMESTAMP_FORMAT)
        .unwrap_or_else(|_| "-".to_string())
}

pub(in crate::tui) fn format_operation_record_date(seconds: i64) -> String {
    let Some(date) = timestamp_to_local_date(seconds) else {
        return "-".to_string();
    };
    date.format(OPERATION_RECORD_DATE_FORMAT)
        .unwrap_or_else(|_| "-".to_string())
}

pub(in crate::tui) fn operation_record_user_label(
    accounts: &[AuthAccount],
    uid: Option<&str>,
) -> String {
    let uid = uid.map(str::trim).filter(|value| !value.is_empty());
    let Some(uid) = uid else {
        return "-".to_string();
    };
    accounts
        .iter()
        .find(|account| account.user.uid == uid)
        .map(|account| account.user.nickname.trim())
        .filter(|nickname| !nickname.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| uid.to_string())
}

pub(in crate::tui) fn operation_record_request_mut_in_value<'a>(
    value: &'a mut Value,
    key: &str,
) -> Option<&'a mut Value> {
    value
        .get_mut("requests")
        .and_then(Value::as_array_mut)?
        .iter_mut()
        .find(|request| operation_record_request_key(request) == Some(key))
}

pub(in crate::tui) fn set_operation_record_request_loading(
    value: &mut Value,
    key: &str,
    loading: bool,
) {
    if let Some(request) = operation_record_request_mut_in_value(value, key) {
        let pagination = value_object_mut(
            value_object_mut(request)
                .entry("pagination")
                .or_insert_with(|| json!({})),
        );
        pagination.insert("loading_more".to_string(), json!(loading));
    }
}

pub(in crate::tui) fn set_all_operation_record_requests_loading(value: &mut Value, loading: bool) {
    let Some(requests) = value.get_mut("requests").and_then(Value::as_array_mut) else {
        return;
    };
    for request in requests {
        let pagination = value_object_mut(
            value_object_mut(request)
                .entry("pagination")
                .or_insert_with(|| json!({})),
        );
        pagination.insert("loading_more".to_string(), json!(loading));
    }
}

pub(in crate::tui) fn oldest_operation_record_time(request: &Value) -> Option<i64> {
    operation_record_request_records(request)
        .iter()
        .filter_map(|record| json_i64(record.get("time")))
        .min()
}

pub(in crate::tui) fn merge_operation_record_page(
    value: &mut Value,
    key: &str,
    query: DeviceHistoryQuery,
    response: Value,
) {
    let new_count = response
        .get("result")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let Some(request) = operation_record_request_mut_in_value(value, key) else {
        return;
    };
    let pagination = value_object_mut(
        value_object_mut(request)
            .entry("pagination")
            .or_insert_with(|| json!({})),
    );
    pagination.insert("time_start".to_string(), json!(query.time_start));
    pagination.insert("time_end".to_string(), json!(query.time_end));
    pagination.insert("limit".to_string(), json!(query.limit));
    pagination.insert("loading_more".to_string(), json!(false));

    if !json_code_is_zero(response.get("code")) {
        pagination.insert("has_more".to_string(), json!(false));
        pagination.insert("no_more".to_string(), json!(true));
        value_object_mut(request).insert("response".to_string(), response);
        return;
    }

    let has_more = new_count >= query.limit as usize && new_count > 0;
    pagination.insert("has_more".to_string(), json!(has_more));
    pagination.insert("no_more".to_string(), json!(!has_more));

    let Some(new_records) = response.get("result").and_then(Value::as_array) else {
        return;
    };
    let response_value = value_object_mut(request)
        .entry("response")
        .or_insert_with(|| json!({"code": 0, "result": []}));
    let response_object = value_object_mut(response_value);
    response_object.insert("code".to_string(), json!(0));
    let result = response_object.entry("result").or_insert_with(|| json!([]));
    if !result.is_array() {
        *result = json!([]);
    }
    if let Some(records) = result.as_array_mut() {
        records.extend(new_records.iter().cloned());
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::tui) enum StatisticsPeriod {
    Week,
    Month,
    Year,
}

impl StatisticsPeriod {
    pub(in crate::tui) fn key(self) -> &'static str {
        match self {
            Self::Week => "week",
            Self::Month => "month",
            Self::Year => "year",
        }
    }

    pub(in crate::tui) fn label(self, lang: Language) -> &'static str {
        match self {
            Self::Week => lang_str(lang, "周", "Week"),
            Self::Month => lang_str(lang, "月", "Month"),
            Self::Year => lang_str(lang, "年", "Year"),
        }
    }

    pub(in crate::tui) fn data_type(self) -> &'static str {
        match self {
            Self::Week => "stat_day_v3",
            Self::Month => "stat_day_v3",
            Self::Year => "stat_month_v3",
        }
    }

    pub(in crate::tui) fn from_key(value: &str) -> Option<Self> {
        match value {
            "week" => Some(Self::Week),
            "month" => Some(Self::Month),
            "year" => Some(Self::Year),
            _ => None,
        }
    }

    pub(in crate::tui) fn all() -> [Self; 3] {
        [Self::Week, Self::Month, Self::Year]
    }
}
