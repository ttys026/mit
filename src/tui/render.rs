//! Rendering: the top-level draw() frame composition, main-area layout,
//! text-area widget rendering, and the push-message dialog layout.
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Clear, List, ListItem, Padding, Paragraph, Tabs, Wrap};
use ratatui_core::style::Style as TextAreaStyle;
use ratatui_core::widgets::Widget as TextAreaWidget;
use ratatui_textarea::{CursorMove, TextArea, WrapMode};
use tui_logger::TuiLoggerWidget;

use crate::storage::Language;

use super::pages::account as account_page;
use super::pages::bootstrap as bootstrap_page;
use super::pages::prop as prop_page;
use super::shared::*;
use super::*;

pub(in crate::tui) fn searchable_main_layout(content_area: Rect) -> [Rect; 3] {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(DEVICE_SEARCH_HEIGHT.saturating_sub(1)),
            Constraint::Min(0),
        ])
        .areas(content_area)
}

pub(in crate::tui) fn draw(frame: &mut ratatui::Frame<'_>, app: &mut TuiApp) {
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
                        app.invalid_xiaomi_account_uids
                            .contains(account.user.uid.as_str()),
                        app.invalid_mijia_account_uids
                            .contains(account.user.uid.as_str()),
                        app.account_check_in_flight,
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
                    let mut row = device_list_row(
                        &device.name,
                        category,
                        &device.room_name,
                        account_label.as_str(),
                    );
                    row.connect = connect_type_label(device.pid, app.language);
                    row.channel = crate::mico_api::device_link_channel(&device.did)
                        .as_str()
                        .to_string();
                    row
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
            // Combined item: shows the running version and checks for updates on Enter.
            let version_label = {
                let prefix = lang_str(app.language, "当前版本", "Current Version");
                let separator = lang_str(app.language, "：", ": ");
                let status = app.update_check_status.clone().unwrap_or_else(|| {
                    lang_str(
                        app.language,
                        "回车检查更新",
                        "press Enter to check for updates",
                    )
                    .to_string()
                });
                let (open, close) = match app.language {
                    Language::Chinese => ("（", "）"),
                    Language::English => (" (", ")"),
                };
                format!(
                    "{prefix}{separator}v{}{open}{status}{close}",
                    env!("CARGO_PKG_VERSION")
                )
            };
            let action_labels = [
                lang_label.to_string(),
                auto_subscribe_device_status_label(app.language, app.auto_subscribe_device_status),
                version_label,
                lang_str(app.language, "在 GitHub 查看", "View on GitHub").to_string(),
                lang_str(
                    app.language,
                    "重新同步三方设备状态",
                    "Re-sync Third-party Device Status",
                )
                .to_string(),
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
            app.device_list_state
                .select(Some(settings_row_for_action(selected_idx)));
            let items: Vec<ListItem> = settings_rows()
                .into_iter()
                .map(|row| match row {
                    SettingsRow::Separator => ListItem::new("─".repeat(list_area.width as usize))
                        .style(Style::default().fg(Color::DarkGray)),
                    SettingsRow::Action(action_idx) => {
                        let mut item = ListItem::new(action_labels[action_idx].clone());
                        if action_idx == selected_idx {
                            item = item.style(active_row_style());
                        }
                        item
                    }
                })
                .collect();
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
                    lang_str(
                        app.language,
                        "重新登录(小米: 设备列表/设备操作)",
                        "Relogin (Xiaomi: Device List / Device Control)",
                    ),
                    lang_str(
                        app.language,
                        "重新登录(米家: 操作记录/能耗统计)",
                        "Relogin (Mijia: Action History / Energy Stats)",
                    ),
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
                let backdrop = expand_rect(popup, 2, 1, frame.area());
                frame.render_widget(Clear, backdrop);
                let lines = settings_confirm_lines(*action, app.language);
                frame.render_widget(
                    Paragraph::new(lines.join("\n"))
                        .block(
                            Block::default()
                                .borders(all_borders())
                                .padding(Padding::new(1, 1, 1, 1))
                                .title(lang_str(app.language, "确认操作", "Confirm Action")),
                        )
                        .wrap(Wrap { trim: true }),
                    popup,
                );
            }
            AccountActionDialog::UpdateAvailable { latest } => {
                let popup = centered_rect(60, 30, frame.area());
                frame.render_widget(Clear, popup);
                let body = [
                    format!(
                        "{}: {latest}",
                        lang_str(app.language, "发现新版本", "New version")
                    ),
                    format!(
                        "{}: v{}",
                        lang_str(app.language, "当前版本", "Current"),
                        env!("CARGO_PKG_VERSION")
                    ),
                    String::new(),
                    lang_str(
                        app.language,
                        "Enter 立即升级 / Esc 取消",
                        "Enter to update now / Esc to cancel",
                    )
                    .to_string(),
                ];
                frame.render_widget(
                    Paragraph::new(body.join("\n"))
                        .block(Block::default().borders(all_borders()).title(lang_str(
                            app.language,
                            "更新",
                            "Update",
                        )))
                        .wrap(Wrap { trim: true }),
                    popup,
                );
            }
            AccountActionDialog::UpdateRunning { latest, lines, .. } => {
                let popup = centered_rect(72, 50, frame.area());
                frame.render_widget(Clear, popup);
                let mut body = vec![
                    format!(
                        "{} {latest}…",
                        lang_str(app.language, "正在升级到", "Updating to")
                    ),
                    indeterminate_bar(app.boot_spinner_index, 28),
                    String::new(),
                ];
                body.extend(lines.iter().cloned());
                frame.render_widget(
                    Paragraph::new(body.join("\n"))
                        .block(Block::default().borders(all_borders()).title(lang_str(
                            app.language,
                            "正在升级",
                            "Updating",
                        )))
                        .wrap(Wrap { trim: true }),
                    popup,
                );
            }
            AccountActionDialog::UpdateFinished { success, message } => {
                let popup = centered_rect(60, 30, frame.area());
                frame.render_widget(Clear, popup);
                let icon = if *success { "✅" } else { "❌" };
                let hint = if *success {
                    lang_str(app.language, "正在重启…", "restarting…")
                } else {
                    lang_str(app.language, "按 Esc 关闭", "Press Esc to close")
                };
                let body = [format!("{icon} {message}"), String::new(), hint.to_string()];
                frame.render_widget(
                    Paragraph::new(body.join("\n"))
                        .block(
                            Block::default()
                                .borders(top_bottom_borders())
                                .title(lang_str(app.language, "升级结果", "Update Result")),
                        )
                        .wrap(Wrap { trim: true }),
                    popup,
                );
            }
            AccountActionDialog::ThirdCloudSync {
                groups,
                running,
                message,
                ..
            } => {
                let popup = centered_rect(72, 50, frame.area());
                let backdrop = expand_rect(popup, 2, 1, frame.area());
                frame.render_widget(Clear, backdrop);
                let mut body = vec![message.clone()];
                if *running {
                    body.push(indeterminate_bar(app.boot_spinner_index, 28));
                }
                body.push(String::new());
                if groups.is_empty() {
                    body.push(lang_str(app.language, "暂无平台", "No platforms").to_string());
                } else {
                    body.extend(groups.iter().map(|group| {
                        let state = match &group.status {
                            ThirdCloudSyncStatus::Pending => {
                                lang_str(app.language, "等待中", "Pending").to_string()
                            }
                            ThirdCloudSyncStatus::Running => {
                                lang_str(app.language, "同步中", "Syncing").to_string()
                            }
                            ThirdCloudSyncStatus::Success { detail } => format!(
                                "{} - {}",
                                lang_str(app.language, "成功", "Success"),
                                detail
                            ),
                            ThirdCloudSyncStatus::Failed { error } => {
                                format!("{} - {}", lang_str(app.language, "失败", "Failed"), error)
                            }
                        };
                        format!("{}  {}", thirdcloud_group_label(group), state)
                    }));
                }
                frame.render_widget(
                    Paragraph::new(body.join("\n"))
                        .block(
                            Block::default()
                                .borders(all_borders())
                                .padding(Padding::new(1, 1, 1, 1))
                                .title(lang_str(
                                    app.language,
                                    "三方设备状态同步",
                                    "Third-party Status Sync",
                                )),
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

pub(in crate::tui) fn configure_textarea_style(textarea: &mut TextArea<'_>) {
    textarea.set_cursor_line_style(TextAreaStyle::default());
    textarea.set_wrap_mode(WrapMode::WordOrGlyph);
}

pub(in crate::tui) fn single_line_textarea(
    input: &str,
    cursor: usize,
    focused: bool,
) -> TextArea<'static> {
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

pub(in crate::tui) fn render_textarea_widget(
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

pub(in crate::tui) fn render_textarea_to_scratch(
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

pub(in crate::tui) fn rendered_textarea_cursor_cell(
    input: &str,
    cursor: usize,
    area: Rect,
) -> Option<(u16, u16)> {
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

pub(in crate::tui) fn push_message_dialog_popup(terminal_area: Rect) -> Rect {
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

pub(in crate::tui) fn push_message_dialog_sections(terminal_area: Rect, input: &str) -> [Rect; 3] {
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

pub(in crate::tui) fn footer_render_area(area: Rect) -> Rect {
    if area.height == 0 || area.width == 0 {
        Rect::new(area.x, area.y, area.width, 0)
    } else if area.height >= 3 {
        Rect::new(area.x, area.y.saturating_add(1), area.width, 1)
    } else {
        Rect::new(area.x, area.y, area.width, 1)
    }
}

pub(in crate::tui) fn split_main_layout(area: Rect) -> [Rect; 4] {
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

pub(in crate::tui) fn core_color_to_ratatui(color: ratatui_core::style::Color) -> Color {
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

/// A bouncing indeterminate progress bar, e.g. `[░░▮▮▮▮░░░░░░]`, animated by
/// `frame_index` (which advances each render frame while an upgrade runs).
fn indeterminate_bar(frame_index: usize, width: usize) -> String {
    let width = width.max(4);
    let block = (width / 5).max(3);
    let span = width - block;
    let cycle = span * 2;
    let phase = if cycle == 0 { 0 } else { frame_index % cycle };
    let pos = if phase <= span { phase } else { cycle - phase };
    let mut bar = String::with_capacity(width);
    for cell in 0..width {
        bar.push(if cell >= pos && cell < pos + block {
            '▮'
        } else {
            '░'
        });
    }
    format!("[{bar}]")
}
