use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Clear, List, ListItem, Paragraph, Tabs, Wrap};

use crate::tui::shared::{
    all_borders, apply_selection_highlight_to_area, centered_rect, display_width,
    highlight_line_range, selected_cols_for_line, SelectionSurface,
};
use crate::tui::{
    action_param_row_layouts, action_param_rows_for_dialog,
    format_prop_dialog_action_list_item_line, format_prop_dialog_list_item_line,
    fullscreen_dialog_inner_area, lang_str, prop_dialog_indices_for_tab, prop_dialog_title,
    prop_edit_textarea_area, prop_editor_header_lines, prop_editor_layout,
    push_message_cli_preview_line, push_message_command_area, render_textarea_widget,
    single_line_textarea, textarea_visual_height, top_bottom_borders,
    visible_prop_dialog_tab_titles, visible_prop_dialog_tabs, AccountActionDialog, BoolDialogTab,
    TuiApp,
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
            let style = if dialog.loading || dialog.refreshing {
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
        let edit_style = if dialog.loading || dialog.refreshing {
            Style::default().add_modifier(Modifier::DIM)
        } else {
            Style::default()
        };
        if dialog.editing {
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
            if dialog.active_tab == BoolDialogTab::ReadOnly {
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
            } else if dialog.active_tab == BoolDialogTab::Actions {
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
            let active_indices = prop_dialog_indices_for_tab(dialog, dialog.active_tab);
            let mut selected_local = active_indices
                .iter()
                .position(|index| *index == dialog.selected)
                .or_else(|| {
                    let preferred = match dialog.active_tab {
                        BoolDialogTab::Writable => dialog.writable_selected,
                        BoolDialogTab::ReadOnly => dialog.readonly_selected,
                        BoolDialogTab::Actions => dialog.actions_selected,
                    };
                    active_indices.iter().position(|index| *index == preferred)
                });
            if selected_local.is_none() && !active_indices.is_empty() {
                selected_local = Some(0);
            }
            if let Some(position) = selected_local {
                dialog.selected = active_indices[position];
                match dialog.active_tab {
                    BoolDialogTab::Writable => dialog.writable_selected = dialog.selected,
                    BoolDialogTab::ReadOnly => dialog.readonly_selected = dialog.selected,
                    BoolDialogTab::Actions => dialog.actions_selected = dialog.selected,
                }
            }
            let list_items = active_indices
                .iter()
                .enumerate()
                .map(|(position, index)| {
                    let line_text = match dialog.active_tab {
                        BoolDialogTab::Actions => format_prop_dialog_action_list_item_line(
                            &dialog.actions[*index],
                            Some(position) == selected_local,
                        ),
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
                    if dialog.loading || dialog.refreshing {
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
                BoolDialogTab::Writable => &mut dialog.writable_list_state,
                BoolDialogTab::ReadOnly => &mut dialog.readonly_list_state,
                BoolDialogTab::Actions => &mut dialog.actions_list_state,
            };
            list_state.select(selected_local);
            frame.render_stateful_widget(List::new(list_items), sections[1], list_state);
        }
    }

    if let Some(dialog) = &app.account_action_dialog {
        match dialog {
            AccountActionDialog::Menu { selected } => {
                let popup = centered_rect(48, 34, frame.area());
                let items = ["推送消息", "重新登录", "登出"]
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
