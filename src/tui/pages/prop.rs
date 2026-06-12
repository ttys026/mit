use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::calendar::{CalendarEventStore, Monthly};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Tabs, Wrap};

use crate::tui::shared::{
    active_row_style, all_borders, apply_selection_highlight_to_area, centered_rect,
    display_truncate_pad, display_width, highlight_line_range, selected_cols_for_line,
    SelectionSurface,
};
use crate::tui::{
    action_param_row_layouts, action_param_rows_for_dialog,
    format_prop_dialog_action_list_item_line, format_prop_dialog_list_item_line,
    fullscreen_dialog_inner_area, lang_str, operation_record_date_filter_area,
    operation_record_date_filter_label, operation_record_date_picker_calendar_area,
    operation_record_date_picker_popup_area, operation_record_date_picker_state,
    operation_record_dropdown_area, operation_record_menu_is_open,
    operation_record_selected_request_index, operation_record_selector_height,
    operation_record_selector_label, operation_record_tab_titles,
    operation_records_active_visual_index, operation_records_table_lines,
    prop_dialog_active_tab_is_loading, prop_dialog_indices_for_tab, prop_dialog_title,
    prop_edit_textarea_area, prop_editor_header_lines, prop_editor_layout,
    push_message_cli_preview_line, push_message_command_area, render_textarea_widget,
    single_line_textarea, statistics_chart_points, statistics_current_key_label,
    statistics_date_filter_area, statistics_date_filter_label, statistics_date_picker_state,
    statistics_dropdown_area, statistics_key_menu_is_open, statistics_period_area,
    statistics_period_dropdown_area, statistics_period_label, statistics_period_menu_is_open,
    statistics_period_options, statistics_selector_height, statistics_selector_label,
    statistics_tab_titles, textarea_visual_height, top_bottom_borders,
    visible_prop_dialog_tab_titles, visible_prop_dialog_tabs, AccountActionDialog, PropDialogTab,
    StatisticsChartPoint, TuiApp,
};

pub(crate) fn draw_prop_dialog(
    frame: &mut ratatui::Frame<'_>,
    app: &mut TuiApp,
    selected_text: Option<&crate::tui::shared::ActiveSelection>,
) {
    app.normalize_prop_dialog_tab_state();
    if let Some(dialog) = &mut app.prop_dialog {
        let popup = frame.area();
        frame.render_widget(Clear, popup);
        let title = prop_dialog_title(dialog, app.language);
        frame.render_widget(
            Block::default()
                .title(title)
                .style(Style::default().bg(Color::Reset)),
            popup,
        );
        let inner = fullscreen_dialog_inner_area(popup);
        if let Some(status) = &dialog.status {
            let status_text =
                format!("This device appears offline.\n\n{status}\n\nPress Esc to close");
            let status_widget = if let Some(active) = selected_text.as_ref() {
                if active.snapshot.surface == SelectionSurface::PropDialogStatus {
                    let lines = status_text
                        .lines()
                        .enumerate()
                        .map(|(idx, line)| {
                            if let Some((mut start, mut end)) = selected_cols_for_line(active, idx)
                            {
                                let width = display_width(line);
                                if end == u16::MAX {
                                    end = width;
                                }
                                start = start.min(width);
                                end = end.min(width);
                                highlight_line_range(line, start, end)
                            } else {
                                Line::from(line.to_string())
                            }
                        })
                        .collect::<Vec<_>>();
                    Text::from(lines)
                } else {
                    Text::from(status_text.clone())
                }
            } else {
                Text::from(status_text.clone())
            };
            let active_tab_loading = prop_dialog_active_tab_is_loading(dialog);
            let style = if active_tab_loading {
                Style::default().add_modifier(Modifier::DIM)
            } else {
                Style::default()
            };
            frame.render_widget(
                Paragraph::new(status_widget)
                    .wrap(Wrap { trim: true })
                    .style(style),
                inner,
            );
        }
        let active_tab_loading = prop_dialog_active_tab_is_loading(dialog);
        let edit_style = if active_tab_loading {
            Style::default().add_modifier(Modifier::DIM)
        } else {
            Style::default()
        };
        if dialog.editing
            && !matches!(
                dialog.active_tab,
                PropDialogTab::Logs | PropDialogTab::Statistics
            )
        {
            let layout = prop_editor_layout(dialog, inner, app.language);
            let top_text = Text::from(
                prop_editor_header_lines(dialog, app.language)
                    .into_iter()
                    .map(Line::from)
                    .collect::<Vec<_>>(),
            );
            frame.render_widget(
                Paragraph::new(top_text)
                    .wrap(Wrap { trim: false })
                    .style(edit_style),
                layout.header_area,
            );
            if dialog.active_tab == PropDialogTab::ReadOnly {
                if let Some(footer_area) = layout.footer_area {
                    let bottom_text = Text::from(
                        super::super::prop_editor_bottom_lines(dialog, app.language)
                            .into_iter()
                            .map(Line::from)
                            .collect::<Vec<_>>(),
                    );
                    frame.render_widget(
                        Paragraph::new(bottom_text)
                            .wrap(Wrap { trim: false })
                            .style(edit_style),
                        footer_area,
                    );
                }
            } else if dialog.active_tab == PropDialogTab::Actions {
                let rows = action_param_rows_for_dialog(dialog);
                let focused = if rows.is_empty() {
                    0
                } else {
                    dialog.writable_selected.min(rows.len() - 1)
                };
                let row_layouts = action_param_row_layouts(dialog, layout.editor_area);
                for row_layout in row_layouts {
                    let idx = row_layout.index;
                    let Some(value) = rows.get(idx) else {
                        break;
                    };
                    let label_style = if idx == focused {
                        edit_style.fg(Color::Green).add_modifier(Modifier::BOLD)
                    } else {
                        edit_style
                    };
                    frame.render_widget(
                        Paragraph::new(row_layout.label_text.clone()).style(label_style),
                        row_layout.label_area,
                    );
                    if row_layout.value_area.width > 0 && row_layout.value_area.height > 0 {
                        if let Some(selector_line) =
                            dialog.actions.get(dialog.selected).and_then(|action| {
                                super::super::action_param_selector_line(
                                    dialog,
                                    action,
                                    idx,
                                    Some(value.as_str()),
                                    edit_style,
                                )
                            })
                        {
                            frame.render_widget(
                                Paragraph::new(selector_line),
                                row_layout.value_area,
                            );
                        } else {
                            let cursor = if idx == focused {
                                dialog.edit_cursor.min(value.chars().count())
                            } else {
                                value.chars().count()
                            };
                            let textarea =
                                single_line_textarea(value.as_str(), cursor, idx == focused);
                            render_textarea_widget(
                                &textarea,
                                row_layout.value_area,
                                frame.buffer_mut(),
                            );
                            if let Some(active) = selected_text.as_ref() {
                                if active.snapshot.surface == SelectionSurface::PropEditor
                                    && active.snapshot.area == row_layout.value_area
                                {
                                    apply_selection_highlight_to_area(
                                        frame.buffer_mut(),
                                        row_layout.value_area,
                                        active.snapshot.lines.as_slice(),
                                        active,
                                    );
                                }
                            }
                        }
                    }
                }
            } else {
                let Some(item) = dialog.items.get(dialog.selected) else {
                    return;
                };
                if let Some(selector_line) =
                    super::super::prop_edit_selector_line(dialog, edit_style)
                {
                    let label_text = format!("> {}: ", item.prop.name);
                    let label_width = display_width(label_text.as_str())
                        .min(layout.editor_area.width.saturating_sub(1));
                    let label_area = ratatui::layout::Rect::new(
                        layout.editor_area.x,
                        layout.editor_area.y,
                        label_width,
                        1,
                    );
                    let value_area = ratatui::layout::Rect::new(
                        layout.editor_area.x.saturating_add(label_width),
                        layout.editor_area.y,
                        layout.editor_area.width.saturating_sub(label_width),
                        1,
                    );
                    frame.render_widget(
                        Paragraph::new(label_text)
                            .style(edit_style.fg(Color::Green).add_modifier(Modifier::BOLD)),
                        label_area,
                    );
                    frame.render_widget(Paragraph::new(selector_line), value_area);
                } else {
                    let textarea =
                        single_line_textarea(dialog.edit_buffer.as_str(), dialog.edit_cursor, true);
                    let textarea_area = prop_edit_textarea_area(dialog, layout.editor_area);
                    render_textarea_widget(&textarea, textarea_area, frame.buffer_mut());
                    if let Some(active) = selected_text.as_ref() {
                        if active.snapshot.surface == SelectionSurface::PropEditor {
                            apply_selection_highlight_to_area(
                                frame.buffer_mut(),
                                textarea_area,
                                active.snapshot.lines.as_slice(),
                                active,
                            );
                        }
                    }
                }
            }
            let bottom_lines = super::super::prop_editor_bottom_lines(dialog, app.language);
            if let Some(footer_area) = layout.footer_area {
                let bottom_text = if let Some(active) = selected_text.as_ref() {
                    if active.snapshot.surface == SelectionSurface::PropEditorFooter {
                        Text::from(
                            bottom_lines
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
                        Text::from(
                            bottom_lines
                                .iter()
                                .cloned()
                                .map(Line::from)
                                .collect::<Vec<_>>(),
                        )
                    }
                } else {
                    Text::from(
                        bottom_lines
                            .iter()
                            .cloned()
                            .map(Line::from)
                            .collect::<Vec<_>>(),
                    )
                };
                frame.render_widget(
                    Paragraph::new(bottom_text)
                        .wrap(Wrap { trim: false })
                        .style(edit_style),
                    footer_area,
                );
            }
        } else {
            let sections = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Min(1)])
                .split(inner);
            let visible_tabs = visible_prop_dialog_tabs(dialog);
            let tab_titles = visible_prop_dialog_tab_titles(dialog, app.language)
                .into_iter()
                .map(|name| Line::from(Span::styled(name, Style::default().fg(Color::Blue))))
                .collect::<Vec<_>>();
            let selected_tab = visible_tabs
                .iter()
                .position(|tab| *tab == dialog.active_tab)
                .unwrap_or(0);
            frame.render_widget(
                Tabs::new(tab_titles)
                    .select(selected_tab)
                    .highlight_style(
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    )
                    .block(Block::default().borders(top_bottom_borders())),
                sections[0],
            );
            if dialog.active_tab == PropDialogTab::Logs {
                let style = if prop_dialog_active_tab_is_loading(dialog) {
                    Style::default().add_modifier(Modifier::DIM)
                } else {
                    Style::default()
                };
                let list_area = sections[1];
                let selector_height = operation_record_selector_height(dialog);
                let (selector_area, body_area) = if selector_height == 0 {
                    (None, list_area)
                } else {
                    let record_sections = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Length(selector_height), Constraint::Min(1)])
                        .split(list_area);
                    (Some(record_sections[0]), record_sections[1])
                };
                let active_visual_index = operation_records_active_visual_index(dialog);
                let list_items =
                    operation_records_table_lines(dialog, app.language, app.accounts.as_slice())
                        .into_iter()
                        .enumerate()
                        .map(|(idx, line_text)| {
                            let line = if let Some(active) = selected_text.as_ref() {
                                if active.snapshot.surface == SelectionSurface::PropDialogList {
                                    if let Some((mut start, mut end)) =
                                        selected_cols_for_line(active, idx)
                                    {
                                        let width = display_width(&line_text);
                                        if end == u16::MAX {
                                            end = width;
                                        }
                                        start = start.min(width);
                                        end = end.min(width);
                                        highlight_line_range(&line_text, start, end)
                                    } else {
                                        Line::from(line_text)
                                    }
                                } else {
                                    Line::from(line_text)
                                }
                            } else {
                                Line::from(line_text)
                            };
                            let mut item = ListItem::new(line);
                            if active_visual_index == Some(idx) {
                                item = item.style(active_row_style());
                            }
                            item
                        })
                        .collect::<Vec<_>>();
                dialog.readonly_list_state.select(active_visual_index);
                frame.render_stateful_widget(
                    List::new(list_items).style(style),
                    body_area,
                    &mut dialog.readonly_list_state,
                );
                if let Some(selector_area) = selector_area {
                    let selector_row_area = ratatui::layout::Rect::new(
                        selector_area.x,
                        selector_area.y,
                        selector_area.width,
                        selector_area.height.min(2),
                    );
                    frame.render_widget(
                        Block::default()
                            .borders(Borders::BOTTOM)
                            .border_style(Style::default()),
                        selector_row_area,
                    );
                    let right_area =
                        operation_record_date_filter_area(selector_row_area, dialog, app.language);
                    let left_width = right_area
                        .map(|area| area.x.saturating_sub(selector_row_area.x).saturating_sub(1))
                        .unwrap_or(selector_row_area.width);
                    let left_area = ratatui::layout::Rect::new(
                        selector_row_area.x,
                        selector_row_area.y,
                        left_width,
                        1,
                    );
                    frame.render_widget(
                        Paragraph::new(Line::from(Span::styled(
                            operation_record_selector_label(dialog, app.language),
                            Style::default().fg(Color::Blue),
                        ))),
                        left_area,
                    );
                    if let Some(right_area) = right_area {
                        frame.render_widget(
                            Paragraph::new(Line::from(Span::styled(
                                operation_record_date_filter_label(dialog, app.language),
                                Style::default().fg(Color::Blue),
                            ))),
                            right_area,
                        );
                    }
                    if operation_record_menu_is_open(dialog) {
                        if let Some(dropdown_area) =
                            operation_record_dropdown_area(list_area, dialog, app.language)
                        {
                            let titles = operation_record_tab_titles(dialog, app.language);
                            let selected_index = if dialog.editing {
                                dialog.edit_cursor.min(titles.len().saturating_sub(1))
                            } else {
                                operation_record_selected_request_index(dialog)
                            };
                            let items = titles
                                .into_iter()
                                .enumerate()
                                .map(|(index, title)| {
                                    let line = if index == selected_index {
                                        Line::from(Span::styled(
                                            title,
                                            Style::default()
                                                .fg(Color::Green)
                                                .add_modifier(Modifier::BOLD),
                                        ))
                                    } else {
                                        Line::from(Span::styled(
                                            title,
                                            Style::default().fg(Color::Blue),
                                        ))
                                    };
                                    ListItem::new(line)
                                })
                                .collect::<Vec<_>>();
                            frame.render_widget(Clear, dropdown_area);
                            frame.render_widget(
                                List::new(items).block(Block::default().borders(all_borders())),
                                dropdown_area,
                            );
                        }
                    }
                    draw_operation_record_date_picker(frame, dialog, app.language);
                }
                return;
            }
            if dialog.active_tab == PropDialogTab::Statistics {
                let style = if prop_dialog_active_tab_is_loading(dialog) {
                    Style::default().add_modifier(Modifier::DIM)
                } else {
                    Style::default()
                };
                let list_area = sections[1];
                let selector_height = statistics_selector_height(dialog);
                let (selector_area, body_area) = if selector_height == 0 {
                    (None, list_area)
                } else {
                    let stats_sections = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Length(selector_height), Constraint::Min(1)])
                        .split(list_area);
                    (Some(stats_sections[0]), stats_sections[1])
                };
                if let Some(selector_area) = selector_area {
                    let selector_row_area = ratatui::layout::Rect::new(
                        selector_area.x,
                        selector_area.y,
                        selector_area.width,
                        selector_area.height.min(2),
                    );
                    frame.render_widget(
                        Block::default()
                            .borders(Borders::BOTTOM)
                            .border_style(Style::default()),
                        selector_row_area,
                    );
                    let date_area =
                        statistics_date_filter_area(selector_row_area, dialog, app.language);
                    let period_area =
                        statistics_period_area(selector_row_area, date_area, dialog, app.language);
                    let left_width = period_area
                        .map(|area| area.x.saturating_sub(selector_row_area.x).saturating_sub(1))
                        .or_else(|| {
                            date_area.map(|area| {
                                area.x.saturating_sub(selector_row_area.x).saturating_sub(1)
                            })
                        })
                        .unwrap_or(selector_row_area.width);
                    let statistics_titles = statistics_tab_titles(dialog, app.language);
                    let current_label = statistics_current_key_label(dialog, app.language);
                    if !current_label.is_empty() && left_width > 0 {
                        let left_area = ratatui::layout::Rect::new(
                            selector_row_area.x,
                            selector_row_area.y,
                            left_width,
                            1,
                        );
                        let selector_label = if statistics_titles.len() > 1 {
                            statistics_selector_label(dialog, app.language)
                        } else {
                            current_label
                        };
                        frame.render_widget(
                            Paragraph::new(Line::from(Span::styled(
                                display_truncate_pad(selector_label.as_str(), left_width as usize),
                                Style::default().fg(Color::Blue),
                            ))),
                            left_area,
                        );
                    }
                    if let Some(period_area) = period_area {
                        frame.render_widget(
                            Paragraph::new(Line::from(Span::styled(
                                statistics_period_label(dialog, app.language),
                                Style::default().fg(Color::Blue),
                            ))),
                            period_area,
                        );
                    }
                    if let Some(date_area) = date_area {
                        frame.render_widget(
                            Paragraph::new(Line::from(Span::styled(
                                statistics_date_filter_label(dialog, app.language),
                                Style::default().fg(Color::Blue),
                            ))),
                            date_area,
                        );
                    }
                }
                match statistics_chart_points(dialog, app.language) {
                    Ok(points) => {
                        draw_statistics_bar_chart(
                            frame,
                            body_area,
                            points.as_slice(),
                            style,
                            app.language,
                        );
                    }
                    Err(message) => {
                        frame.render_widget(
                            Paragraph::new(message)
                                .wrap(Wrap { trim: false })
                                .style(style),
                            body_area,
                        );
                    }
                }
                if let Some(selector_area) = selector_area {
                    let selector_row_area = ratatui::layout::Rect::new(
                        selector_area.x,
                        selector_area.y,
                        selector_area.width,
                        selector_area.height.min(2),
                    );
                    if statistics_key_menu_is_open(dialog) {
                        if let Some(dropdown_area) =
                            statistics_dropdown_area(list_area, dialog, app.language)
                        {
                            let titles = statistics_tab_titles(dialog, app.language);
                            let selected_index =
                                dialog.edit_cursor.min(titles.len().saturating_sub(1));
                            let items = titles
                                .into_iter()
                                .enumerate()
                                .map(|(index, title)| {
                                    let style = if index == selected_index {
                                        Style::default()
                                            .fg(Color::Green)
                                            .add_modifier(Modifier::BOLD)
                                    } else {
                                        Style::default().fg(Color::Blue)
                                    };
                                    ListItem::new(Line::from(Span::styled(title, style)))
                                })
                                .collect::<Vec<_>>();
                            frame.render_widget(Clear, dropdown_area);
                            frame.render_widget(
                                List::new(items).block(Block::default().borders(all_borders())),
                                dropdown_area,
                            );
                        }
                    }
                    if statistics_period_menu_is_open(dialog) {
                        if let Some(dropdown_area) = statistics_period_dropdown_area(
                            list_area,
                            selector_row_area,
                            dialog,
                            app.language,
                        ) {
                            let titles = statistics_period_options(app.language);
                            let selected_index =
                                dialog.edit_cursor.min(titles.len().saturating_sub(1));
                            let items = titles
                                .into_iter()
                                .enumerate()
                                .map(|(index, title)| {
                                    let style = if index == selected_index {
                                        Style::default()
                                            .fg(Color::Green)
                                            .add_modifier(Modifier::BOLD)
                                    } else {
                                        Style::default().fg(Color::Blue)
                                    };
                                    ListItem::new(Line::from(Span::styled(title, style)))
                                })
                                .collect::<Vec<_>>();
                            frame.render_widget(Clear, dropdown_area);
                            frame.render_widget(
                                List::new(items).block(Block::default().borders(all_borders())),
                                dropdown_area,
                            );
                        }
                    }
                    draw_statistics_date_picker(frame, dialog, app.language);
                }
                return;
            }
            let active_indices = prop_dialog_indices_for_tab(dialog, dialog.active_tab);
            let mut selected_local = active_indices
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
                });
            if selected_local.is_none() && !active_indices.is_empty() {
                selected_local = Some(0);
            }
            if let Some(position) = selected_local {
                dialog.selected = active_indices[position];
                match dialog.active_tab {
                    PropDialogTab::Writable => dialog.writable_selected = dialog.selected,
                    PropDialogTab::ReadOnly => dialog.readonly_selected = dialog.selected,
                    PropDialogTab::Actions => dialog.actions_selected = dialog.selected,
                    PropDialogTab::Logs | PropDialogTab::Statistics => {}
                }
            }
            let list_items = active_indices
                .iter()
                .enumerate()
                .map(|(position, index)| {
                    let line_text = match dialog.active_tab {
                        PropDialogTab::Actions => format_prop_dialog_action_list_item_line(
                            &dialog.actions[*index],
                            Some(position) == selected_local,
                        ),
                        PropDialogTab::Logs | PropDialogTab::Statistics => String::new(),
                        _ => {
                            let item = &dialog.items[*index];
                            format_prop_dialog_list_item_line(
                                item,
                                Some(position) == selected_local,
                                dialog.loading,
                                app.language,
                            )
                        }
                    };
                    if let Some(active) = selected_text.as_ref() {
                        if active.snapshot.surface == SelectionSurface::PropDialogList {
                            if let Some((mut start, mut end)) =
                                selected_cols_for_line(active, position)
                            {
                                let width = display_width(&line_text);
                                if end == u16::MAX {
                                    end = width;
                                }
                                start = start.min(width);
                                end = end.min(width);
                                return ListItem::new(highlight_line_range(&line_text, start, end));
                            }
                        }
                    }
                    if prop_dialog_active_tab_is_loading(dialog) {
                        let gray_line = Line::from(Span::styled(
                            line_text,
                            Style::default().add_modifier(Modifier::DIM),
                        ));
                        ListItem::new(gray_line)
                    } else {
                        ListItem::new(line_text)
                    }
                })
                .collect::<Vec<_>>();
            let list_state = match dialog.active_tab {
                PropDialogTab::Writable => &mut dialog.writable_list_state,
                PropDialogTab::ReadOnly => &mut dialog.readonly_list_state,
                PropDialogTab::Actions => &mut dialog.actions_list_state,
                PropDialogTab::Logs | PropDialogTab::Statistics => &mut dialog.readonly_list_state,
            };
            list_state.select(selected_local);
            frame.render_stateful_widget(List::new(list_items), sections[1], list_state);
        }
    }

    if let Some(dialog) = &app.account_action_dialog {
        match dialog {
            AccountActionDialog::Menu { selected } => {
                let popup = centered_rect(48, 34, frame.area());
                let items = ["推送消息", "重新登录(小米)", "重新登录(米家)", "登出"]
                    .iter()
                    .enumerate()
                    .map(|(idx, item)| {
                        let selected_marker = if idx == *selected { ">" } else { " " };
                        ListItem::new(format!("{selected_marker} {item}"))
                    })
                    .collect::<Vec<_>>();
                frame.render_widget(Clear, popup);
                frame.render_widget(
                    List::new(items)
                        .block(Block::default().borders(all_borders()).title("账户操作")),
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
                let popup = centered_rect(74, 46, frame.area());
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
                frame.render_widget(
                    Paragraph::new(format!(
                        "{}: {uid}\n\n{}:",
                        lang_str(app.language, "账户 ID", "Account ID"),
                        lang_str(app.language, "消息", "Message")
                    ))
                    .wrap(Wrap { trim: false }),
                    sections[0],
                );
                let textarea = single_line_textarea(input, *cursor, true);
                render_textarea_widget(&textarea, sections[1], frame.buffer_mut());
                let command_area = push_message_command_area(frame.area(), input.as_str());
                frame.render_widget(
                    Paragraph::new(push_message_cli_preview_line(
                        uid.as_str(),
                        input.as_str(),
                        app.language,
                    ))
                    .wrap(Wrap { trim: false }),
                    command_area,
                );
                if let Some(active) = selected_text.as_ref() {
                    if active.snapshot.surface == SelectionSurface::PushMessageCommand {
                        apply_selection_highlight_to_area(
                            frame.buffer_mut(),
                            command_area,
                            active.snapshot.lines.as_slice(),
                            active,
                        );
                    }
                }
            }
            AccountActionDialog::SettingsConfirm { .. } => {}
        }
    }
}

fn draw_operation_record_date_picker(
    frame: &mut ratatui::Frame<'_>,
    dialog: &super::super::PropDialog,
    lang: crate::storage::Language,
) {
    let Some(state) = operation_record_date_picker_state(dialog) else {
        return;
    };
    let popup = operation_record_date_picker_popup_area(frame.area());
    if popup.width == 0 || popup.height == 0 {
        return;
    }
    let Some(calendar_area) = operation_record_date_picker_calendar_area(frame.area()) else {
        return;
    };
    let mut events = CalendarEventStore::default();
    if let Some(start) = state.pending_start {
        events.add(
            start,
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        );
    }
    events.add(
        state.cursor,
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD | Modifier::REVERSED),
    );
    let calendar = Monthly::new(state.cursor, events)
        .show_month_header(
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        )
        .show_weekdays_header(Style::default().add_modifier(Modifier::DIM))
        .show_surrounding(Style::default().add_modifier(Modifier::DIM));

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Block::default()
            .borders(all_borders())
            .title(lang_str(lang, "日期范围", "Date Range")),
        popup,
    );
    frame.render_widget(calendar, calendar_area);
}

fn draw_statistics_bar_chart(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    points: &[StatisticsChartPoint],
    style: Style,
    lang: crate::storage::Language,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    frame.render_widget(
        Paragraph::new("")
            .block(Block::default().borders(all_borders()).title(lang_str(
                lang,
                "值 ↑  时间 →",
                "Value ↑  Time →",
            )))
            .style(style),
        area,
    );
    if points.is_empty() || area.width < 8 || area.height < 6 {
        return;
    }

    let inner = Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(1),
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    );
    if inner.width == 0 || inner.height < 3 {
        return;
    }
    let horizontal_padding = 3.min(inner.width.saturating_sub(1) / 2);
    let vertical_padding = 1.min(inner.height.saturating_sub(1) / 2);
    let plot_area = Rect::new(
        inner.x.saturating_add(horizontal_padding),
        inner.y.saturating_add(vertical_padding),
        inner
            .width
            .saturating_sub(horizontal_padding.saturating_mul(2)),
        inner
            .height
            .saturating_sub(vertical_padding.saturating_mul(2)),
    );
    if plot_area.width == 0 || plot_area.height < 3 {
        return;
    }

    let point_count = points.len();
    let plot_width = plot_area.width as usize;
    let gap = if point_count <= 1 {
        0
    } else if plot_width >= point_count.saturating_add(point_count.saturating_sub(1) * 2) {
        2
    } else if plot_width >= point_count.saturating_add(point_count.saturating_sub(1)) {
        1
    } else {
        0
    };
    let max_visible_points = if gap == 0 {
        plot_width.max(1)
    } else {
        (plot_width.saturating_add(gap) / (gap + 1)).max(1)
    };
    let visible_count = points.len().min(max_visible_points);
    if visible_count == 0 {
        return;
    }
    let total_gap = if visible_count > 1 {
        gap.saturating_mul(visible_count - 1)
    } else {
        0
    };
    let bar_width =
        ((plot_area.width as usize).saturating_sub(total_gap) / visible_count).clamp(1, 7);
    let slot_width = bar_width.saturating_add(if visible_count > 1 { gap } else { 0 });
    let group_width = bar_width
        .saturating_mul(visible_count)
        .saturating_add(total_gap);
    let group_offset = (plot_area.width as usize).saturating_sub(group_width) / 2;
    let group_x = plot_area
        .x
        .saturating_add(group_offset.min(u16::MAX as usize) as u16);
    let time_row = plot_area
        .y
        .saturating_add(plot_area.height.saturating_sub(1));
    let bar_top = plot_area.y.saturating_add(1);
    if time_row <= bar_top {
        return;
    }
    let bar_height = time_row.saturating_sub(bar_top);
    let bar_bottom = time_row.saturating_sub(1);
    let max_value = points
        .iter()
        .filter_map(|point| point.value.is_finite().then_some(point.value.max(0.0)))
        .fold(0.0_f64, f64::max)
        .max(0.0);
    let y_axis_max = if max_value > 0.0 {
        max_value * 1.1
    } else {
        1.0
    };
    let value_style = style.fg(Color::Green);
    let bar_style = style.fg(Color::Blue);
    let label_style = style.fg(Color::Blue);
    let bar_text = "█".repeat(bar_width);
    let zero_bar_text = "▁".repeat(bar_width);
    let time_label_visible = chart_striped_label_visibility(
        points,
        visible_count,
        slot_width,
        group_x,
        bar_width as u16,
        plot_area,
        |point| point.label.as_str(),
    );
    let value_label_visible = chart_value_label_visibility(
        points,
        visible_count,
        group_x,
        slot_width,
        bar_width as u16,
        plot_area,
    );
    let buffer = frame.buffer_mut();
    for (index, point) in points.iter().take(visible_count).enumerate() {
        let x = group_x
            .saturating_add((index.saturating_mul(slot_width)).min(u16::MAX as usize) as u16);
        if x >= plot_area.x.saturating_add(plot_area.width) {
            break;
        }
        let width = bar_width.min(
            plot_area
                .x
                .saturating_add(plot_area.width)
                .saturating_sub(x) as usize,
        );
        if width == 0 {
            continue;
        }
        let value = if point.value.is_finite() {
            point.value.max(0.0)
        } else {
            0.0
        };
        let scaled_height = if value <= 0.0 {
            0
        } else {
            ((value / y_axis_max) * f64::from(bar_height))
                .floor()
                .max(1.0)
                .min(f64::from(bar_height)) as u16
        };
        for row_offset in 0..scaled_height {
            buffer.set_stringn(
                x,
                bar_bottom.saturating_sub(row_offset),
                bar_text.as_str(),
                width,
                bar_style,
            );
        }
        if scaled_height == 0 {
            buffer.set_stringn(x, bar_bottom, zero_bar_text.as_str(), width, bar_style);
        }
        let value_row = if scaled_height == 0 {
            bar_bottom
        } else {
            bar_bottom.saturating_sub(scaled_height)
        };
        if value_label_visible.get(index).copied().unwrap_or(false) {
            draw_centered_chart_text(
                buffer,
                point.text_value.as_str(),
                x,
                width as u16,
                value_row,
                plot_area,
                value_style,
            );
        }
        if time_label_visible.get(index).copied().unwrap_or(false) {
            draw_centered_chart_text(
                buffer,
                point.label.as_str(),
                x,
                width as u16,
                time_row,
                plot_area,
                label_style,
            );
        }
    }
}

fn chart_striped_label_visibility(
    points: &[StatisticsChartPoint],
    visible_count: usize,
    slot_width: usize,
    group_x: u16,
    bar_width: u16,
    bounds: Rect,
    text: impl Fn(&StatisticsChartPoint) -> &str,
) -> Vec<bool> {
    let mut visible = vec![false; visible_count];
    if visible_count == 0 || slot_width == 0 {
        return visible;
    }
    let max_label_width = points
        .iter()
        .take(visible_count)
        .map(|point| display_width(text(point)) as usize)
        .max()
        .unwrap_or(0);
    let stride = max_label_width
        .saturating_add(1)
        .saturating_add(slot_width.saturating_sub(1))
        .checked_div(slot_width)
        .unwrap_or(1)
        .max(1);
    let mut intervals: Vec<(usize, u16, u16)> = Vec::new();
    for index in (0..visible_count).step_by(stride) {
        let Some((start, width)) = chart_centered_text_bounds(
            text(&points[index]),
            chart_bar_x(group_x, slot_width, index),
            bar_width,
            bounds,
        ) else {
            continue;
        };
        let end = start.saturating_add(width as u16);
        if intervals
            .last()
            .is_none_or(|(_, _, prev_end)| *prev_end < start)
        {
            visible[index] = true;
            intervals.push((index, start, end));
        }
    }
    if visible_count > 1 {
        let last_index = visible_count - 1;
        if !visible[last_index] {
            if let Some((start, width)) = chart_centered_text_bounds(
                text(&points[last_index]),
                chart_bar_x(group_x, slot_width, last_index),
                bar_width,
                bounds,
            ) {
                let end = start.saturating_add(width as u16);
                while intervals
                    .last()
                    .is_some_and(|(_, interval_start, interval_end)| {
                        *interval_start <= end && start <= *interval_end
                    })
                {
                    if let Some((index, _, _)) = intervals.pop() {
                        visible[index] = false;
                    }
                }
                if intervals
                    .last()
                    .is_none_or(|(_, _, prev_end)| *prev_end < start)
                {
                    visible[last_index] = true;
                }
            }
        }
    }
    visible
}

fn chart_value_label_visibility(
    points: &[StatisticsChartPoint],
    visible_count: usize,
    group_x: u16,
    slot_width: usize,
    bar_width: u16,
    bounds: Rect,
) -> Vec<bool> {
    let mut visible = vec![false; visible_count];
    let mut last_end = bounds.x;
    for (index, point) in points.iter().take(visible_count).enumerate() {
        if !point.value.is_finite() || point.value <= 0.0 {
            continue;
        }
        let Some((start, width)) = chart_centered_text_bounds(
            point.text_value.as_str(),
            chart_bar_x(group_x, slot_width, index),
            bar_width,
            bounds,
        ) else {
            continue;
        };
        let end = start.saturating_add(width as u16);
        if start >= last_end {
            visible[index] = true;
            last_end = end;
        }
    }
    visible
}

fn chart_bar_x(group_x: u16, slot_width: usize, index: usize) -> u16 {
    group_x.saturating_add((index.saturating_mul(slot_width)).min(u16::MAX as usize) as u16)
}

fn draw_centered_chart_text(
    buffer: &mut ratatui::buffer::Buffer,
    text: &str,
    bar_x: u16,
    bar_width: u16,
    row: u16,
    bounds: Rect,
    style: Style,
) {
    if row < bounds.y {
        return;
    }
    let Some((text_x, text_width)) = chart_centered_text_bounds(text, bar_x, bar_width, bounds)
    else {
        return;
    };
    buffer.set_stringn(
        text_x,
        row,
        display_truncate_pad(text, text_width),
        text_width,
        style,
    );
}

fn chart_centered_text_bounds(
    text: &str,
    bar_x: u16,
    bar_width: u16,
    bounds: Rect,
) -> Option<(u16, usize)> {
    if bounds.width == 0 || bar_width == 0 {
        return None;
    }
    let bounds_right = bounds.x.saturating_add(bounds.width);
    let text_width = display_width(text).min(bounds.width) as usize;
    if text_width == 0 {
        return None;
    }
    let bar_center = bar_x.saturating_mul(2).saturating_add(bar_width);
    let mut text_x = bar_center.saturating_sub(text_width as u16) / 2;
    if text_x < bounds.x {
        text_x = bounds.x;
    }
    if text_x.saturating_add(text_width as u16) > bounds_right {
        text_x = bounds_right.saturating_sub(text_width as u16);
    }
    Some((text_x, text_width))
}

fn draw_statistics_date_picker(
    frame: &mut ratatui::Frame<'_>,
    dialog: &super::super::PropDialog,
    lang: crate::storage::Language,
) {
    let Some(state) = statistics_date_picker_state(dialog) else {
        return;
    };
    let popup = operation_record_date_picker_popup_area(frame.area());
    if popup.width == 0 || popup.height == 0 {
        return;
    }
    let Some(calendar_area) = operation_record_date_picker_calendar_area(frame.area()) else {
        return;
    };
    let mut events = CalendarEventStore::default();
    events.add(
        state.cursor,
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD | Modifier::REVERSED),
    );
    let calendar = Monthly::new(state.cursor, events)
        .show_month_header(
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        )
        .show_weekdays_header(Style::default().add_modifier(Modifier::DIM))
        .show_surrounding(Style::default().add_modifier(Modifier::DIM));

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Block::default().borders(all_borders()).title(lang_str(
            lang,
            "统计时间范围",
            "Stats Range",
        )),
        popup,
    );
    frame.render_widget(calendar, calendar_area);
}
