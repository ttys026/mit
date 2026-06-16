use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

#[cfg(test)]
use crate::mico_api::Device;
use crate::storage::Language;
use crate::tui::device_list_header_titles;
use crate::tui::shared::{
    display_truncate_pad, display_truncate_pad_with_ellipsis, shrink_largest_width,
    table_header_style,
};

#[derive(Clone, Debug)]
pub(crate) struct DeviceListRow {
    pub(crate) name: String,
    pub(crate) category: String,
    pub(crate) room: String,
    pub(crate) account: String,
    /// Transport label ("LAN"/"Cloud"); empty when unknown.
    pub(crate) channel: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DeviceListColumns {
    pub(crate) name: usize,
    pub(crate) category: usize,
    pub(crate) room: usize,
    pub(crate) account: usize,
    pub(crate) channel: usize,
}

impl DeviceListColumns {
    pub(crate) fn total_width(self) -> usize {
        self.room + self.name + self.category + self.account + self.channel
    }

    fn shrink_largest(&mut self) -> bool {
        let mut widths = [
            self.name,
            self.category,
            self.room,
            self.account,
            self.channel,
        ];
        if !shrink_largest_width(&mut widths) {
            return false;
        }
        [
            self.name,
            self.category,
            self.room,
            self.account,
            self.channel,
        ] = widths;
        true
    }
}

const DEVICE_COL_NAME_MAX: usize = 20;

pub(crate) fn device_list_row(
    name: &str,
    category: &str,
    room: &str,
    account_label: &str,
) -> DeviceListRow {
    let category = if category.trim().is_empty() {
        "-"
    } else {
        category.trim()
    };
    let room = if room.trim().is_empty() {
        "-"
    } else {
        room.trim()
    };
    DeviceListRow {
        name: name.to_string(),
        category: category.to_string(),
        room: room.to_string(),
        account: account_label.trim().to_string(),
        channel: String::new(),
    }
}

pub(crate) fn compute_device_list_columns(
    rows: &[DeviceListRow],
    available_width: usize,
    lang: Language,
) -> DeviceListColumns {
    let headers = device_list_header_titles(lang);
    let mut columns = DeviceListColumns {
        name: UnicodeWidthStr::width(headers[1]) + 2,
        category: UnicodeWidthStr::width(headers[2]) + 2,
        room: UnicodeWidthStr::width(headers[0]) + 2,
        account: UnicodeWidthStr::width(headers[3]) + 2,
        channel: UnicodeWidthStr::width(headers[4]) + 2,
    };
    for row in rows {
        columns.name = columns
            .name
            .max(UnicodeWidthStr::width(row.name.as_str()) + 2);
        columns.category = columns
            .category
            .max(UnicodeWidthStr::width(row.category.as_str()) + 2);
        columns.room = columns
            .room
            .max(UnicodeWidthStr::width(row.room.as_str()) + 2);
        columns.account = columns
            .account
            .max(UnicodeWidthStr::width(row.account.as_str()) + 2);
        columns.channel = columns
            .channel
            .max(UnicodeWidthStr::width(row.channel.as_str()) + 2);
    }
    columns.name = columns.name.min(DEVICE_COL_NAME_MAX);
    while columns.total_width() > available_width && columns.shrink_largest() {}
    columns
}

pub(crate) fn format_device_list_item_with_columns(
    row: &DeviceListRow,
    columns: DeviceListColumns,
) -> String {
    format!(
        "{}{}{}{}{}",
        display_truncate_pad(&row.room, columns.room),
        display_truncate_pad_with_ellipsis(&row.name, columns.name),
        display_truncate_pad(&row.category, columns.category),
        display_truncate_pad(&row.account, columns.account),
        display_truncate_pad(&row.channel, columns.channel),
    )
}

#[cfg(test)]
pub(crate) fn format_device_list_item(
    device: &Device,
    category: &str,
    account_label: &str,
) -> String {
    let row = device_list_row(&device.name, category, &device.room_name, account_label);
    let columns =
        compute_device_list_columns(std::slice::from_ref(&row), usize::MAX, Language::Chinese);
    format_device_list_item_with_columns(&row, columns)
}

#[cfg(test)]
pub(crate) fn format_device_list_header(lang: Language) -> String {
    let columns = compute_device_list_columns(&[], usize::MAX, lang);
    format_device_list_header_with_columns(columns, lang)
}

pub(crate) fn format_device_list_header_with_columns(
    columns: DeviceListColumns,
    lang: Language,
) -> String {
    let headers = device_list_header_titles(lang);
    format!(
        "{}{}{}{}{}",
        display_truncate_pad(headers[0], columns.room),
        display_truncate_pad(headers[1], columns.name),
        display_truncate_pad(headers[2], columns.category),
        display_truncate_pad(headers[3], columns.account),
        display_truncate_pad(headers[4], columns.channel),
    )
}

pub(crate) fn format_device_list_header_line_with_columns(
    columns: DeviceListColumns,
    lang: Language,
) -> Line<'static> {
    Line::from(Span::styled(
        format_device_list_header_with_columns(columns, lang),
        table_header_style(),
    ))
}
