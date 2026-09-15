//! Search bar: input editing, rendering, and account/device filtering
//! (which rows match the active query and keeping the selection visible).
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use anyhow::Result;
use std::collections::HashMap;

use crate::mico_api::Device;
use crate::storage::{AuthAccount, Language};

use super::pages::account as account_page;
use super::shared::*;
use super::{lang_str, read_device_categories, TuiApp};

pub(in crate::tui) fn active_tab_has_search(tab: usize) -> bool {
    matches!(tab, 0..=2)
}

pub(in crate::tui) fn search_tab_slot(tab: usize) -> Option<usize> {
    match tab {
        0 => Some(0),
        1 => Some(1),
        2 => Some(2),
        _ => None,
    }
}
pub(in crate::tui) fn char_cursor_byte_index(input: &str, cursor: usize) -> usize {
    input
        .char_indices()
        .nth(cursor)
        .map(|(index, _)| index)
        .unwrap_or(input.len())
}

pub(in crate::tui) fn search_prefix(lang: Language) -> String {
    let label = lang_str(lang, "搜索", "Search");
    format!("/ {label}: ")
}

pub(in crate::tui) fn visible_search_input(input: &str, width: u16) -> (String, usize) {
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

pub(in crate::tui) fn search_plain_line(app: &TuiApp, area_width: u16) -> String {
    let prefix = search_prefix(app.language);
    let prefix_width = display_width(prefix.as_str());
    let input_width = area_width.saturating_sub(prefix_width);
    let (visible_input, _) = visible_search_input(app.input.as_str(), input_width);
    format!("{prefix}{visible_input}")
}

pub(in crate::tui) fn search_cursor_for_mouse(
    input: &str,
    lang: Language,
    area: Rect,
    column: u16,
) -> usize {
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

pub(in crate::tui) fn search_line(app: &TuiApp, area_width: u16) -> Line<'static> {
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

pub(in crate::tui) fn render_search_bar(
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

impl TuiApp {
    pub(in crate::tui) fn search_is_active(&self) -> bool {
        self.input_mode && active_tab_has_search(self.active_tab)
    }

    pub(in crate::tui) fn search_query(&self) -> &str {
        self.input.trim()
    }

    pub(in crate::tui) fn save_active_search_state(&mut self) {
        if let Some(slot) = search_tab_slot(self.active_tab) {
            self.search_inputs[slot] = self.input.clone();
            self.search_cursors[slot] = self.device_search_cursor.min(self.input.chars().count());
        }
    }

    pub(in crate::tui) fn load_active_search_state(&mut self) {
        if let Some(slot) = search_tab_slot(self.active_tab) {
            self.input = self.search_inputs[slot].clone();
            self.device_search_cursor = self.search_cursors[slot].min(self.input.chars().count());
        } else {
            self.input.clear();
            self.device_search_cursor = 0;
        }
    }

    pub(in crate::tui) fn focus_search(&mut self) {
        self.input_mode = true;
        self.device_search_cursor = self.device_search_cursor.min(self.input.chars().count());
        self.save_active_search_state();
        self.reset_log_scroll_if_active();
        self.ensure_search_selection_visible();
    }

    pub(in crate::tui) fn blur_search(&mut self) {
        self.save_active_search_state();
        self.input_mode = false;
    }

    pub(in crate::tui) fn search_insert(&mut self, ch: char) {
        let cursor = self.device_search_cursor.min(self.input.chars().count());
        let index = char_cursor_byte_index(self.input.as_str(), cursor);
        self.input.insert(index, ch);
        self.device_search_cursor = cursor.saturating_add(1);
        self.save_active_search_state();
        self.reset_log_scroll_if_active();
        self.ensure_search_selection_visible();
    }

    pub(in crate::tui) fn search_backspace(&mut self) {
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

    pub(in crate::tui) fn search_move_cursor_left(&mut self) {
        self.device_search_cursor = self.device_search_cursor.saturating_sub(1);
        self.save_active_search_state();
    }

    pub(in crate::tui) fn search_move_cursor_right(&mut self) {
        self.device_search_cursor = (self.device_search_cursor + 1).min(self.input.chars().count());
        self.save_active_search_state();
    }

    pub(in crate::tui) fn ensure_search_selection_visible(&mut self) {
        match self.active_tab {
            0 => self.ensure_account_selection_visible(),
            1 => self.ensure_device_selection_visible(),
            _ => {}
        }
    }

    pub(in crate::tui) fn account_matches_search(
        &self,
        account: &AuthAccount,
        query: &str,
    ) -> bool {
        if query.is_empty() {
            return true;
        }
        let query = query.to_lowercase();
        let row = account_page::account_list_row(
            account,
            self.offline_account_uids
                .contains(account.user.uid.as_str()),
            self.invalid_xiaomi_account_uids
                .contains(account.user.uid.as_str()),
            self.invalid_mijia_account_uids
                .contains(account.user.uid.as_str()),
            self.account_check_in_flight,
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

    pub(in crate::tui) fn filtered_account_indices(&self) -> Vec<usize> {
        let query = self.search_query();
        self.accounts
            .iter()
            .enumerate()
            .filter_map(|(index, account)| {
                self.account_matches_search(account, query).then_some(index)
            })
            .collect()
    }

    pub(in crate::tui) fn selected_account_is_visible(&self) -> bool {
        self.filtered_account_indices()
            .contains(&self.account_index)
    }

    pub(in crate::tui) fn ensure_account_selection_visible(&mut self) {
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

    pub(in crate::tui) fn open_selected_visible_account(&mut self) -> Result<()> {
        if self.selected_account_is_visible() {
            account_page::open_account_action_dialog(self)?;
        }
        Ok(())
    }

    pub(in crate::tui) fn open_selected_visible_device(&mut self) {
        if !self.selected_device_is_visible() {
            return;
        }
        if let Err(error) = self.open_prop_dialog() {
            account_page::open_offline_prop_dialog_for_current(self, &error);
        }
    }

    pub(in crate::tui) fn device_matches_search(
        &self,
        device: &Device,
        category: &str,
        query: &str,
    ) -> bool {
        if query.is_empty() {
            return true;
        }
        let query = query.to_lowercase();
        [device.room_name.as_str(), device.name.as_str(), category]
            .iter()
            .any(|value| value.to_lowercase().contains(query.as_str()))
    }

    pub(in crate::tui) fn filtered_device_indices(&self) -> Vec<usize> {
        let categories = read_device_categories(self.home_dir.as_path(), self.language);
        self.filtered_device_indices_with_categories(&categories)
    }

    pub(in crate::tui) fn filtered_device_indices_with_categories(
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

    pub(in crate::tui) fn selected_device_is_visible(&self) -> bool {
        self.filtered_device_indices().contains(&self.device_index)
    }

    pub(in crate::tui) fn ensure_device_selection_visible(&mut self) {
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
}
