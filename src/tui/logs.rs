//! Log viewer: entry encoding/timestamps, line wrapping, scrollbar geometry,
//! search highlighting, and the TuiApp log/scroll state methods.
use crossterm::event::MouseEvent;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::cell::RefCell;
use time::format_description::FormatItem;
use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};
use unicode_width::UnicodeWidthChar;

use super::shared::*;
use super::{
    now_epoch_millis, rect_contains, searchable_main_layout, split_main_layout, TuiApp,
    FOOTER_COPY_LOG_PREFIX,
};

const LOG_MAX: usize = 1000;
pub(in crate::tui) const LOG_SCROLL_PAGE: usize = 10;
const LOG_ENTRY_PREFIX: &str = "__log_at=";
const LOG_SCROLLBAR_MARGIN_WIDTH: u16 = 3;
pub(in crate::tui) const LOG_SCROLLBAR_THUMB: &str = "█";
const LOG_SCROLLBAR_TRACK: &str = "║";
const LOG_TIMESTAMP_FORMAT: &[FormatItem<'static>] =
    format_description!("[hour]:[minute]:[second]");

thread_local! {
    static LAST_LOG_TEXT_WIDTH: RefCell<Option<u16>> = const { RefCell::new(None) };
}

pub(in crate::tui) fn handle_log_scrollbar_mouse(
    app: &mut TuiApp,
    mouse: MouseEvent,
    terminal_area: ratatui::layout::Rect,
) -> bool {
    let [_tabs_area, content_area, _status_gap_area, _status_bar_area] =
        split_main_layout(terminal_area);
    let [_search_area, _search_border_area, logs_area] = searchable_main_layout(content_area);
    let (visual_lines, _log_text_area, scrollbar_area) = log_visual_lines_and_areas(app, logs_area);
    let Some(scrollbar_area) = scrollbar_area else {
        return false;
    };
    if !rect_contains(scrollbar_area, mouse.column, mouse.row) {
        return false;
    }
    app.log_scroll_offset =
        log_scroll_offset_for_scrollbar_row(mouse.row, scrollbar_area, visual_lines.len());
    true
}

fn encode_log_entry(message: String) -> String {
    if message.starts_with(FOOTER_COPY_LOG_PREFIX) || message.starts_with(LOG_ENTRY_PREFIX) {
        message
    } else {
        format!("{LOG_ENTRY_PREFIX}{}\t{message}", now_epoch_millis())
    }
}

fn split_log_entry(line: &str) -> (Option<u128>, &str) {
    let Some(rest) = line.strip_prefix(LOG_ENTRY_PREFIX) else {
        return (None, line);
    };
    let Some((timestamp, message)) = rest.split_once('\t') else {
        return (None, line);
    };
    match timestamp.parse::<u128>() {
        Ok(timestamp) => (Some(timestamp), message),
        Err(_) => (None, line),
    }
}

pub(in crate::tui) fn log_entry_message(line: &str) -> &str {
    split_log_entry(line).1
}

fn format_log_timestamp(timestamp_ms: u128) -> String {
    let Ok(seconds) = i64::try_from(timestamp_ms / 1000) else {
        return "--:--:--".to_string();
    };
    let Ok(timestamp) = OffsetDateTime::from_unix_timestamp(seconds) else {
        return "--:--:--".to_string();
    };
    let timestamp = match UtcOffset::current_local_offset() {
        Ok(offset) => timestamp.to_offset(offset),
        Err(_) => timestamp,
    };
    timestamp
        .format(LOG_TIMESTAMP_FORMAT)
        .unwrap_or_else(|_| "--:--:--".to_string())
}

fn format_log_entry_for_display(line: &str) -> String {
    let (timestamp, message) = split_log_entry(line);
    let timestamp = format_log_timestamp(timestamp.unwrap_or_else(now_epoch_millis));
    format!("[{timestamp}] {message}")
}

pub(in crate::tui) fn remember_log_text_width(width: u16) {
    LAST_LOG_TEXT_WIDTH.with(|state| *state.borrow_mut() = Some(width));
}

fn last_log_text_width() -> Option<u16> {
    LAST_LOG_TEXT_WIDTH.with(|state| *state.borrow())
}

fn log_entry_visual_row_count_for_anchor(entry: &str) -> usize {
    if entry.starts_with(FOOTER_COPY_LOG_PREFIX) {
        return 0;
    }
    let width = last_log_text_width()
        .filter(|width| *width > 0)
        .unwrap_or(u16::MAX);
    wrap_log_line(format_log_entry_for_display(entry).as_str(), width).len()
}

fn log_selection_is_active() -> bool {
    selected_surface()
        .map(|active| active.snapshot.surface == SelectionSurface::Logs)
        .unwrap_or(false)
}

pub(in crate::tui) fn log_selection_is_stale(active: &ActiveSelection, area: Rect, visible_lines: &[String]) -> bool {
    active.snapshot.surface == SelectionSurface::Logs
        && (active.snapshot.area != area || active.snapshot.lines.as_slice() != visible_lines)
}

pub(in crate::tui) fn logs_lines_for_display(app: &TuiApp) -> Vec<String> {
    let query = if app.active_tab == 2 {
        app.search_query().to_lowercase()
    } else {
        String::new()
    };
    app.logs
        .iter()
        .rev()
        .filter(|line| !line.starts_with(FOOTER_COPY_LOG_PREFIX))
        .filter(|line| {
            let message = log_entry_message(line);
            query.is_empty() || message.to_lowercase().contains(query.as_str())
        })
        .map(|line| format_log_entry_for_display(line))
        .collect::<Vec<_>>()
}

pub(in crate::tui) fn log_scroll_offset_for_view(app: &TuiApp, line_count: usize, viewport_height: u16) -> usize {
    let visible_rows = viewport_height as usize;
    app.log_scroll_offset
        .min(line_count.saturating_sub(visible_rows))
}

fn wrap_log_line(line: &str, width: u16) -> Vec<String> {
    if width == 0 || line.is_empty() {
        return vec![line.to_string()];
    }
    let mut rows = Vec::new();
    let mut row = String::new();
    let mut row_width = 0u16;
    for ch in line.chars() {
        let char_width = ch.width().unwrap_or(0) as u16;
        if !row.is_empty() && row_width.saturating_add(char_width) > width {
            rows.push(std::mem::take(&mut row));
            row_width = 0;
        }
        row.push(ch);
        row_width = row_width.saturating_add(char_width);
    }
    rows.push(row);
    rows
}

fn log_visual_lines_for_width(app: &TuiApp, width: u16) -> Vec<String> {
    logs_lines_for_display(app)
        .into_iter()
        .flat_map(|line| wrap_log_line(line.as_str(), width))
        .collect()
}

pub(in crate::tui) fn logs_visible_lines_for_display(app: &TuiApp, viewport_area: Rect) -> Vec<String> {
    let lines = log_visual_lines_for_width(app, viewport_area.width);
    let offset = log_scroll_offset_for_view(app, lines.len(), viewport_area.height);
    lines
        .into_iter()
        .skip(offset)
        .take(viewport_area.height as usize)
        .collect()
}

fn log_viewer_areas_for_line_count(
    list_area: Rect,
    visual_line_count: usize,
) -> (Rect, Option<Rect>) {
    if list_area.width <= LOG_SCROLLBAR_MARGIN_WIDTH.saturating_add(1)
        || list_area.height == 0
        || visual_line_count <= list_area.height as usize
    {
        return (list_area, None);
    }
    let [text_area, _margin_area, scrollbar_area] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(LOG_SCROLLBAR_MARGIN_WIDTH),
            Constraint::Length(1),
        ])
        .areas(list_area);
    (text_area, Some(scrollbar_area))
}

pub(in crate::tui) fn log_viewer_text_area(app: &TuiApp, list_area: Rect) -> Rect {
    let visual_line_count = log_visual_lines_for_width(app, list_area.width).len();
    log_viewer_areas_for_line_count(list_area, visual_line_count).0
}

pub(in crate::tui) fn log_visual_lines_and_areas(app: &TuiApp, list_area: Rect) -> (Vec<String>, Rect, Option<Rect>) {
    let full_width_lines = log_visual_lines_for_width(app, list_area.width);
    let (text_area, scrollbar_area) =
        log_viewer_areas_for_line_count(list_area, full_width_lines.len());
    if scrollbar_area.is_some() {
        let visual_lines = log_visual_lines_for_width(app, text_area.width);
        (visual_lines, text_area, scrollbar_area)
    } else {
        (full_width_lines, text_area, scrollbar_area)
    }
}

fn log_scroll_offset_for_scrollbar_row(row: u16, scrollbar_area: Rect, line_count: usize) -> usize {
    let Some(geometry) = log_scrollbar_geometry(line_count, scrollbar_area.height, 0) else {
        return 0;
    };
    if geometry.max_offset == 0 || geometry.track_movement == 0 {
        return 0;
    }
    let row_index = row
        .saturating_sub(scrollbar_area.y)
        .min(scrollbar_area.height.saturating_sub(1)) as usize;
    let thumb_start = row_index.min(geometry.track_movement);
    thumb_start
        .saturating_mul(geometry.max_offset)
        .saturating_add(geometry.track_movement / 2)
        / geometry.track_movement
}

#[derive(Clone, Copy, Debug)]
pub(in crate::tui) struct LogScrollbarGeometry {
    max_offset: usize,
    track_movement: usize,
    thumb_start: usize,
    thumb_height: usize,
}

pub(in crate::tui) fn log_scrollbar_geometry(
    visual_line_count: usize,
    viewport_height: u16,
    offset: usize,
) -> Option<LogScrollbarGeometry> {
    let viewport_height = viewport_height as usize;
    if viewport_height == 0 || visual_line_count <= viewport_height {
        return None;
    }
    let max_offset = visual_line_count.saturating_sub(viewport_height);
    let thumb_height = viewport_height
        .saturating_mul(viewport_height)
        .saturating_add(visual_line_count.saturating_sub(1))
        / visual_line_count;
    let thumb_height = thumb_height.clamp(1, viewport_height);
    let track_movement = viewport_height.saturating_sub(thumb_height);
    let thumb_start = if max_offset == 0 || track_movement == 0 {
        0
    } else {
        offset
            .min(max_offset)
            .saturating_mul(track_movement)
            .saturating_add(max_offset / 2)
            / max_offset
    };
    Some(LogScrollbarGeometry {
        max_offset,
        track_movement,
        thumb_start,
        thumb_height,
    })
}

pub(in crate::tui) fn log_scrollbar_lines(geometry: LogScrollbarGeometry, height: u16) -> Vec<Line<'static>> {
    (0..height as usize)
        .map(|row| {
            let symbol = if row >= geometry.thumb_start
                && row < geometry.thumb_start.saturating_add(geometry.thumb_height)
            {
                LOG_SCROLLBAR_THUMB
            } else {
                LOG_SCROLLBAR_TRACK
            };
            Line::from(symbol)
        })
        .collect()
}

fn log_search_highlight_style() -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(Color::Yellow)
        .add_modifier(Modifier::BOLD)
}

fn next_char_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index < text.len() && !text.is_char_boundary(index) {
        index = index.saturating_add(1);
    }
    index
}

pub(in crate::tui) fn highlight_log_search_matches(line: &str, query: &str) -> Line<'static> {
    let query = query.trim();
    if query.is_empty() {
        return Line::from(line.to_string());
    }
    let haystack = line.to_lowercase();
    let needle = query.to_lowercase();
    if needle.is_empty() {
        return Line::from(line.to_string());
    }

    let mut spans = Vec::new();
    let mut cursor = 0usize;
    while cursor < line.len() {
        let Some(search_from) = haystack.get(cursor..) else {
            break;
        };
        let Some(relative_start) = search_from.find(needle.as_str()) else {
            break;
        };
        let start = cursor.saturating_add(relative_start);
        if !line.is_char_boundary(start) {
            cursor = next_char_boundary(line, start.saturating_add(1));
            continue;
        }
        let mut end = start.saturating_add(needle.len()).min(line.len());
        while end < line.len() && !line.is_char_boundary(end) {
            end = end.saturating_add(1);
        }
        if end <= start {
            break;
        }
        if start > cursor {
            spans.push(Span::raw(line[cursor..start].to_string()));
        }
        spans.push(Span::styled(
            line[start..end].to_string(),
            log_search_highlight_style(),
        ));
        cursor = end;
    }

    if cursor == 0 {
        return Line::from(line.to_string());
    }
    if cursor < line.len() {
        spans.push(Span::raw(line[cursor..].to_string()));
    }
    Line::from(spans)
}


impl TuiApp {
    pub(in crate::tui) fn log(&mut self, message: impl Into<String>) {
        let entry = encode_log_entry(message.into());
        let anchor_added_rows = if self.log_scroll_offset > 0 || log_selection_is_active() {
            log_entry_visual_row_count_for_anchor(&entry)
        } else {
            0
        };
        self.logs.push_back(entry);
        while self.logs.len() > LOG_MAX {
            self.logs.pop_front();
        }
        if anchor_added_rows > 0 {
            self.log_scroll_offset = self.log_scroll_offset.saturating_add(anchor_added_rows);
        }
        self.clamp_log_scroll_offset_to_content();
    }

    pub(in crate::tui) fn clear_logs(&mut self) {
        self.logs.clear();
        self.log_scroll_offset = 0;
        if log_selection_is_active() {
            clear_selection_state();
        }
    }

    pub(in crate::tui) fn clamp_log_scroll_offset_to_content(&mut self) {
        self.log_scroll_offset = self.log_scroll_offset.min(LOG_MAX.saturating_sub(1));
    }

    pub(in crate::tui) fn clamp_log_scroll_offset_for_view(&mut self, line_count: usize, viewport_height: u16) {
        self.log_scroll_offset = log_scroll_offset_for_view(self, line_count, viewport_height);
    }

    pub(in crate::tui) fn scroll_logs_up(&mut self, amount: usize) {
        self.log_scroll_offset = self.log_scroll_offset.saturating_sub(amount);
    }

    pub(in crate::tui) fn scroll_logs_down(&mut self, amount: usize) {
        self.log_scroll_offset = self.log_scroll_offset.saturating_add(amount);
    }

    pub(in crate::tui) fn reset_log_scroll_if_active(&mut self) {
        if self.active_tab == 2 {
            self.log_scroll_offset = 0;
        }
    }
}
