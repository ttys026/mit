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
    format_statistics_value, fullscreen_dialog_inner_area, lang_str,
    operation_record_date_filter_area, operation_record_date_filter_label,
    operation_record_date_picker_calendar_area, operation_record_date_picker_popup_area,
    operation_record_date_picker_state, operation_record_dropdown_area,
    operation_record_menu_is_open, operation_record_selected_request_index,
    operation_record_selector_height, operation_record_selector_label, operation_record_tab_titles,
    operation_records_active_visual_index, operation_records_table_lines,
    prop_dialog_active_tab_is_loading, prop_dialog_indices_for_tab, prop_dialog_title,
    prop_edit_textarea_area, prop_editor_header_lines, prop_editor_layout,
    push_message_cli_preview_line, push_message_command_area, render_textarea_widget,
    single_line_textarea, statistics_chart_layout, statistics_chart_points,
    statistics_current_key_label, statistics_date_filter_area, statistics_date_filter_label,
    statistics_date_picker_state, statistics_dropdown_area, statistics_key_menu_is_open,
    statistics_period_area, statistics_period_dropdown_area, statistics_period_label,
    statistics_period_menu_is_open, statistics_period_options, statistics_selector_height,
    statistics_selector_label, statistics_tab_titles, textarea_visual_height, top_bottom_borders,
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
                let list_area = sections[1];
                if !prop_dialog_account_has_mijia(app.accounts.as_slice(), &dialog.account_uid) {
                    draw_mijia_login_prompt(
                        frame,
                        list_area,
                        app.language,
                        lang_str(app.language, "操作记录", "operation records"),
                    );
                    return;
                }
                let style = if prop_dialog_active_tab_is_loading(dialog) {
                    Style::default().add_modifier(Modifier::DIM)
                } else {
                    Style::default()
                };
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
                let list_area = sections[1];
                if !prop_dialog_account_has_mijia(app.accounts.as_slice(), &dialog.account_uid) {
                    draw_mijia_login_prompt(
                        frame,
                        list_area,
                        app.language,
                        lang_str(app.language, "统计数据", "statistics"),
                    );
                    return;
                }
                let style = if prop_dialog_active_tab_is_loading(dialog) {
                    Style::default().add_modifier(Modifier::DIM)
                } else {
                    Style::default()
                };
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
                if prop_dialog_active_tab_is_loading(dialog) {
                    let center_y = body_area.y + body_area.height / 2;
                    let center_area = Rect::new(body_area.x, center_y, body_area.width, 1);
                    frame.render_widget(
                        Paragraph::new(lang_str(app.language, "加载中...", "Loading..."))
                            .alignment(ratatui::layout::Alignment::Center),
                        center_area,
                    );
                } else {
                    match statistics_chart_points(dialog, app.language) {
                        Ok(points) => {
                            let selected_bar = dialog.statistics_selected_bar;
                            let key_label = statistics_current_key_label(dialog, app.language);
                            draw_statistics_bar_chart(
                                frame,
                                body_area,
                                points.as_slice(),
                                style,
                                app.language,
                                selected_bar,
                                key_label.as_str(),
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
                let items = [
                    "推送消息",
                    "重新登录(小米: 设备列表/设备操作)",
                    "重新登录(米家: 操作记录/能耗统计)",
                    "登出",
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
            // Upgrade dialogs are drawn by the main frame, not layered over a prop dialog.
            AccountActionDialog::UpdateAvailable { .. }
            | AccountActionDialog::UpdateRunning { .. }
            | AccountActionDialog::UpdateFinished { .. }
            | AccountActionDialog::ThirdCloudSync { .. } => {}
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

/// Whether the account owning the dialog's device has Mijia credentials. The
/// Statistics and Logs tabs read from the Mijia cloud, so without them we prompt
/// the user to log in rather than showing an error.
fn prop_dialog_account_has_mijia(
    accounts: &[crate::storage::AuthAccount],
    account_uid: &str,
) -> bool {
    accounts
        .iter()
        .find(|account| account.user.uid == account_uid)
        .is_some_and(|account| crate::mijia_api::is_mijia_auth_present(account.mijia.as_ref()))
}

/// Render the "log in to Mijia first" prompt shown in the Statistics / Logs tabs
/// when the current account has no Mijia credentials.
fn draw_mijia_login_prompt(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    lang: crate::storage::Language,
    activity: &str,
) {
    let message = match lang {
        crate::storage::Language::Chinese => {
            format!("请先登录米家后查看{activity}。\n\n运行：mit auth login mijia")
        }
        crate::storage::Language::English => {
            format!("Log in to Mijia to view {activity}.\n\nRun: mit auth login mijia")
        }
    };
    frame.render_widget(Paragraph::new(message).wrap(Wrap { trim: false }), area);
}

fn draw_statistics_bar_chart(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    points: &[StatisticsChartPoint],
    style: Style,
    lang: crate::storage::Language,
    selected_bar: Option<usize>,
    key_label: &str,
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
    let Some(layout) = statistics_chart_layout(area, points) else {
        return;
    };

    let axis_style = style.fg(Color::DarkGray);
    let bar_style = style.fg(Color::Blue);
    let label_style = style.fg(Color::Blue);
    let bounds_height = layout
        .x_label_row
        .saturating_sub(layout.value_label_row)
        .saturating_add(1);
    // Time labels share the otherwise-empty label row, so they may reach into
    // the left padding to stay centered under their bars (req 5: even spacing).
    let time_label_left = layout.axis_col.saturating_sub(layout.y_label_width);
    let time_bounds = Rect::new(
        time_label_left,
        layout.value_label_row,
        layout.plot_right.saturating_sub(time_label_left),
        bounds_height,
    );

    let time_label_visible =
        chart_time_label_visibility(points, layout.visible_count, layout.slot_width as usize);
    let bar_text = "█".repeat(layout.bar_width as usize);
    let zero_bar_text = "▁".repeat(layout.bar_width as usize);

    {
        let buffer = frame.buffer_mut();
        // Y axis: vertical line + corner, then ticks and their value labels.
        for row in layout.bar_top..layout.axis_row {
            buffer.set_stringn(layout.axis_col, row, "│", 1, axis_style);
        }
        buffer.set_stringn(layout.axis_col, layout.axis_row, "└", 1, axis_style);
        // X axis baseline: start at axis_col+1 so it connects to the └ corner.
        let x_axis_start = layout.axis_col.saturating_add(1);
        let x_axis_width = layout.plot_right.saturating_sub(x_axis_start) as usize;
        let x_axis_full = "─".repeat(x_axis_width);
        buffer.set_stringn(
            x_axis_start,
            layout.axis_row,
            x_axis_full.as_str(),
            x_axis_width,
            axis_style,
        );
        for i in 0..layout.y_tick_count {
            let row = layout.y_tick_row(i);
            if i > 0 {
                buffer.set_stringn(layout.axis_col, row, "┤", 1, axis_style);
            }
            let text = format_statistics_value(layout.y_tick_value(i));
            let width = display_width(text.as_str()).min(layout.y_label_width) as usize;
            if width > 0 {
                let label_x = layout.axis_col.saturating_sub(width as u16);
                buffer.set_stringn(
                    label_x,
                    row,
                    display_truncate_pad(text.as_str(), width),
                    width,
                    label_style,
                );
            }
        }

        // Bars (left-aligned) with value and time labels.
        for (index, point) in points.iter().take(layout.visible_count).enumerate() {
            let x = layout.bar_x(index);
            if x >= layout.plot_right {
                break;
            }
            let width =
                (layout.bar_width as usize).min(layout.plot_right.saturating_sub(x) as usize);
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
                ((value / layout.y_axis_max) * f64::from(layout.bar_height))
                    .floor()
                    .max(1.0)
                    .min(f64::from(layout.bar_height)) as u16
            };
            for row_offset in 0..scaled_height {
                buffer.set_stringn(
                    x,
                    layout.bar_bottom.saturating_sub(row_offset),
                    bar_text.as_str(),
                    width,
                    bar_style,
                );
            }
            if scaled_height == 0 {
                buffer.set_stringn(
                    x,
                    layout.bar_bottom,
                    zero_bar_text.as_str(),
                    width,
                    bar_style,
                );
            }
            if time_label_visible.get(index).copied().unwrap_or(false) {
                draw_centered_chart_text(
                    buffer,
                    point.label.as_str(),
                    x,
                    width as u16,
                    layout.x_label_row,
                    time_bounds,
                    label_style,
                );
            }
        }
    }

    draw_statistics_chart_crosshair(
        frame,
        area,
        &layout,
        points,
        selected_bar,
        lang,
        key_label,
        style,
    );
}

/// Draw the click-to-inspect crosshair (a bar-wide vertical band) over the
/// selected bar plus a tooltip with its date and value.
#[allow(clippy::too_many_arguments)]
fn draw_statistics_chart_crosshair(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    layout: &crate::tui::StatisticsChartLayout,
    points: &[StatisticsChartPoint],
    selected_bar: Option<usize>,
    lang: crate::storage::Language,
    key_label: &str,
    style: Style,
) {
    let Some(index) = selected_bar.filter(|index| *index < layout.visible_count) else {
        return;
    };
    let Some(point) = points.get(index) else {
        return;
    };
    let x = layout.bar_x(index);
    let width = (layout.bar_width).min(layout.plot_right.saturating_sub(x));
    if width == 0 {
        return;
    }

    {
        let buffer = frame.buffer_mut();
        for row in layout.bar_top..=layout.bar_bottom {
            for col in x..x.saturating_add(width) {
                if col < layout.plot_right {
                    buffer[(col, row)].set_bg(Color::DarkGray);
                }
            }
        }
    }

    let line1 = format!("{}: {}", lang_str(lang, "日期", "Date"), point.label);
    let line2 = format!("{key_label}: {}", point.text_value);
    let inner_left = area.x.saturating_add(1);
    let inner_right = area.x.saturating_add(area.width).saturating_sub(1);
    let inner_top = area.y.saturating_add(1);
    let inner_bottom = area.y.saturating_add(area.height).saturating_sub(1);
    if inner_right <= inner_left || inner_bottom <= inner_top {
        return;
    }
    let avail_w = inner_right.saturating_sub(inner_left);
    let content_w = display_width(line1.as_str())
        .max(display_width(line2.as_str()))
        .min(avail_w.saturating_sub(2));
    if content_w == 0 {
        return;
    }
    let tooltip_w = content_w.saturating_add(2);
    let tooltip_h = 4u16.min(inner_bottom.saturating_sub(inner_top));
    if tooltip_h < 3 {
        return;
    }

    // Prefer placing the tooltip to the right of the bar, fall back to the left.
    let mut tx = x.saturating_add(width).saturating_add(1);
    if tx.saturating_add(tooltip_w) > inner_right {
        tx = x.saturating_sub(tooltip_w.saturating_add(1));
    }
    tx = tx.clamp(inner_left, inner_right.saturating_sub(tooltip_w));
    let mut ty = layout.bar_top;
    if ty.saturating_add(tooltip_h) > inner_bottom {
        ty = inner_bottom.saturating_sub(tooltip_h);
    }
    ty = ty.max(inner_top);

    let tooltip_area = Rect::new(tx, ty, tooltip_w, tooltip_h);
    frame.render_widget(Clear, tooltip_area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(display_truncate_pad(line1.as_str(), content_w as usize)),
            Line::from(display_truncate_pad(line2.as_str(), content_w as usize)),
        ])
        .block(Block::default().borders(all_borders()))
        .style(style),
        tooltip_area,
    );
}

/// Decide which bars get an X-axis (time) label. When every label fits with at
/// least one blank column between neighbours they are all shown; otherwise the
/// labels collapse to at most five, evenly distributed and including both ends,
/// so they stay readable instead of bunching up (req 3).
fn chart_time_label_visibility(
    points: &[StatisticsChartPoint],
    visible_count: usize,
    slot_width: usize,
) -> Vec<bool> {
    const MAX_COLLAPSED_LABELS: usize = 5;
    let mut visible = vec![false; visible_count];
    if visible_count == 0 || slot_width == 0 {
        return visible;
    }
    if visible_count == 1 {
        visible[0] = true;
        return visible;
    }
    let max_label_width = points
        .iter()
        .take(visible_count)
        .map(|point| display_width(point.label.as_str()) as usize)
        .max()
        .unwrap_or(0);
    // Smallest index step that keeps a blank column between adjacent labels.
    let min_stride = max_label_width
        .saturating_add(2)
        .div_ceil(slot_width.max(1))
        .max(1);
    if min_stride <= 1 {
        // Everything fits: label every bar.
        visible.iter_mut().for_each(|flag| *flag = true);
        return visible;
    }
    // Collapsed: spread out at most five labels (and never more than fit).
    let max_fit = (visible_count - 1) / min_stride + 1;
    let count = max_fit.clamp(2, MAX_COLLAPSED_LABELS);
    for j in 0..count {
        let index =
            ((j as f64) * ((visible_count - 1) as f64) / ((count - 1) as f64)).round() as usize;
        visible[index.min(visible_count - 1)] = true;
    }
    visible
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
