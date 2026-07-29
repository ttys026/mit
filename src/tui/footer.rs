//! Footer: clickable key-hint segments, their actions, the copied-badge,
//! and clipboard / browser helpers used by footer operations.
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use serde_json::Value;
use std::collections::VecDeque;
use std::fs;
#[cfg(not(test))]
use std::process::Command;

use anyhow::{bail, Result};

use super::shared::*;
use super::{
    handle_key, lang_str, logs_lines_for_display, now_epoch_millis, operation_record_requests,
    AccountActionDialog, PropDialogTab, TuiApp, FOOTER_COPY_BADGE_TEXT, FOOTER_COPY_LOG_PREFIX,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::tui) enum FooterOperation {
    Refresh,
    Search,
    Enter,
    SelectRecord,
    DateFilter,
    ClearDateFilter,
    Back,
    AddAccount,
    Copy,
    ClearLogs,
}

#[derive(Clone, Debug)]
pub(in crate::tui) struct FooterSegment {
    text: String,
    operation: Option<FooterOperation>,
}

pub(in crate::tui) fn build_footer_segments(
    ops: &[(&str, FooterOperation)],
    suffix: Option<String>,
) -> Vec<FooterSegment> {
    let mut segments = Vec::new();
    for (index, (label, op)) in ops.iter().enumerate() {
        if index > 0 {
            segments.push(FooterSegment {
                text: ", ".to_string(),
                operation: None,
            });
        }
        segments.push(FooterSegment {
            text: (*label).to_string(),
            operation: Some(*op),
        });
    }
    if let Some(suffix) = suffix.filter(|text| !text.is_empty()) {
        if !segments.is_empty() {
            segments.push(FooterSegment {
                text: ", ".to_string(),
                operation: None,
            });
        }
        segments.push(FooterSegment {
            text: suffix,
            operation: None,
        });
    }
    segments
}

pub(in crate::tui) fn footer_segments(app: &TuiApp) -> Vec<FooterSegment> {
    let lang = app.language;
    if let Some(dialog) = &app.account_action_dialog {
        return match dialog {
            AccountActionDialog::Menu { .. } => build_footer_segments(
                &[
                    (
                        lang_str(lang, "Enter: 选择", "Enter: Select"),
                        FooterOperation::Enter,
                    ),
                    (
                        lang_str(lang, "Esc: 返回", "Esc: Back"),
                        FooterOperation::Back,
                    ),
                ],
                None,
            ),
            AccountActionDialog::PushMessage { .. } => build_footer_segments(
                &[
                    (
                        lang_str(lang, "Enter: 发送", "Enter: Send"),
                        FooterOperation::Enter,
                    ),
                    (
                        lang_str(lang, "Esc: 返回", "Esc: Back"),
                        FooterOperation::Back,
                    ),
                ],
                None,
            ),
            AccountActionDialog::Reauth { auth_url, .. } => {
                let mut ops: Vec<(&str, FooterOperation)> = Vec::new();
                if !auth_url.trim().is_empty() && auth_url.trim() != "-" {
                    ops.push((lang_str(lang, "C: 复制", "C: Copy"), FooterOperation::Copy));
                }
                ops.push((
                    lang_str(lang, "Esc: 返回", "Esc: Back"),
                    FooterOperation::Back,
                ));
                build_footer_segments(ops.as_slice(), None)
            }
            AccountActionDialog::SettingsConfirm { .. } => build_footer_segments(
                &[
                    (
                        lang_str(lang, "Enter: 确认", "Enter: Confirm"),
                        FooterOperation::Enter,
                    ),
                    (
                        lang_str(lang, "Esc: 返回", "Esc: Back"),
                        FooterOperation::Back,
                    ),
                ],
                None,
            ),
            AccountActionDialog::UpdateAvailable { .. } => build_footer_segments(
                &[
                    (
                        lang_str(lang, "Enter: 升级", "Enter: Update"),
                        FooterOperation::Enter,
                    ),
                    (
                        lang_str(lang, "Esc: 取消", "Esc: Cancel"),
                        FooterOperation::Back,
                    ),
                ],
                None,
            ),
            AccountActionDialog::UpdateRunning { .. } => build_footer_segments(
                &[(
                    lang_str(lang, "Esc/q: 取消升级", "Esc/q: Cancel"),
                    FooterOperation::Back,
                )],
                None,
            ),
            AccountActionDialog::UpdateFinished { .. } => build_footer_segments(
                &[(
                    lang_str(lang, "Esc: 关闭", "Esc: Close"),
                    FooterOperation::Back,
                )],
                None,
            ),
            AccountActionDialog::ThirdCloudSync { running, .. } => {
                let label = if *running {
                    lang_str(lang, "Esc: 后台运行", "Esc: Run in background")
                } else {
                    lang_str(lang, "Esc: 关闭", "Esc: Close")
                };
                build_footer_segments(&[(label, FooterOperation::Back)], None)
            }
        };
    }

    let current_device_label = lang_str(lang, "当前设备", "Current Device");
    if let Some(dialog) = &app.prop_dialog {
        if dialog.editing {
            if dialog.active_tab == PropDialogTab::Logs {
                return build_footer_segments(
                    &[
                        (
                            lang_str(lang, "Enter: 选择", "Enter: Select"),
                            FooterOperation::Enter,
                        ),
                        (
                            lang_str(lang, "Esc: 返回", "Esc: Back"),
                            FooterOperation::Back,
                        ),
                    ],
                    Some(format!("{current_device_label}: {}", dialog.device_did)),
                );
            }
            if dialog.active_tab == PropDialogTab::Statistics {
                return build_footer_segments(
                    &[
                        (
                            lang_str(lang, "Enter: 选择", "Enter: Select"),
                            FooterOperation::Enter,
                        ),
                        (
                            lang_str(lang, "Esc: 返回", "Esc: Back"),
                            FooterOperation::Back,
                        ),
                    ],
                    Some(format!("{current_device_label}: {}", dialog.device_did)),
                );
            }
            if dialog.active_tab == PropDialogTab::ReadOnly {
                return build_footer_segments(
                    &[(
                        lang_str(lang, "Esc: 返回", "Esc: Back"),
                        FooterOperation::Back,
                    )],
                    Some(format!("{current_device_label}: {}", dialog.device_did)),
                );
            }
            return build_footer_segments(
                &[
                    (
                        lang_str(lang, "Enter: 执行", "Enter: Execute"),
                        FooterOperation::Enter,
                    ),
                    (
                        lang_str(lang, "Esc: 返回", "Esc: Back"),
                        FooterOperation::Back,
                    ),
                ],
                Some(format!("{current_device_label}: {}", dialog.device_did)),
            );
        }
        return match dialog.active_tab {
            PropDialogTab::Writable => build_footer_segments(
                &[
                    (
                        lang_str(lang, "R: 刷新", "R: Refresh"),
                        FooterOperation::Refresh,
                    ),
                    (
                        lang_str(lang, "Esc: 返回", "Esc: Back"),
                        FooterOperation::Back,
                    ),
                    (
                        lang_str(lang, "Enter: 修改属性", "Enter: Edit Property"),
                        FooterOperation::Enter,
                    ),
                ],
                Some(format!("{current_device_label}: {}", dialog.device_did)),
            ),
            PropDialogTab::ReadOnly => build_footer_segments(
                &[
                    (
                        lang_str(lang, "R: 刷新", "R: Refresh"),
                        FooterOperation::Refresh,
                    ),
                    (
                        lang_str(lang, "Esc: 返回", "Esc: Back"),
                        FooterOperation::Back,
                    ),
                    (
                        lang_str(lang, "Enter: 查看属性", "Enter: View Property"),
                        FooterOperation::Enter,
                    ),
                ],
                Some(format!("{current_device_label}: {}", dialog.device_did)),
            ),
            PropDialogTab::Actions => build_footer_segments(
                &[
                    (
                        lang_str(lang, "R: 刷新", "R: Refresh"),
                        FooterOperation::Refresh,
                    ),
                    (
                        lang_str(lang, "Esc: 返回", "Esc: Back"),
                        FooterOperation::Back,
                    ),
                ],
                Some(format!("{current_device_label}: {}", dialog.device_did)),
            ),
            PropDialogTab::Logs => {
                let mut ops = vec![(
                    lang_str(lang, "R: 刷新", "R: Refresh"),
                    FooterOperation::Refresh,
                )];
                if !operation_record_requests(dialog).is_empty() {
                    ops.push((
                        lang_str(lang, "S: 选择记录", "S: Select Record"),
                        FooterOperation::SelectRecord,
                    ));
                }
                ops.push((
                    lang_str(lang, "D: 日期", "D: Date"),
                    FooterOperation::DateFilter,
                ));
                ops.push((
                    lang_str(lang, "C: 清除", "C: Clear"),
                    FooterOperation::ClearDateFilter,
                ));
                ops.push((
                    lang_str(lang, "Esc: 返回", "Esc: Back"),
                    FooterOperation::Back,
                ));
                build_footer_segments(
                    ops.as_slice(),
                    Some(format!("{current_device_label}: {}", dialog.device_did)),
                )
            }
            PropDialogTab::Statistics => build_footer_segments(
                &[
                    (
                        lang_str(lang, "R: 刷新", "R: Refresh"),
                        FooterOperation::Refresh,
                    ),
                    (
                        lang_str(lang, "P: 周/月/年", "P: Period"),
                        FooterOperation::DateFilter,
                    ),
                    (
                        lang_str(lang, "D: 日期", "D: Date"),
                        FooterOperation::DateFilter,
                    ),
                    (
                        lang_str(lang, "C: 清除", "C: Clear"),
                        FooterOperation::ClearDateFilter,
                    ),
                    (
                        lang_str(lang, "Esc: 返回", "Esc: Back"),
                        FooterOperation::Back,
                    ),
                ],
                Some(format!("{current_device_label}: {}", dialog.device_did)),
            ),
        };
    }

    if app.search_is_active() {
        return match app.active_tab {
            0 => {
                let matched_label = lang_str(lang, "匹配账户", "Matched accounts");
                build_footer_segments(
                    &[
                        (
                            lang_str(lang, "Esc: 返回", "Esc: Back"),
                            FooterOperation::Back,
                        ),
                        (
                            lang_str(lang, "Enter: 账户操作", "Enter: Account Actions"),
                            FooterOperation::Enter,
                        ),
                    ],
                    Some(format!(
                        "{matched_label}: {}",
                        app.filtered_account_indices().len()
                    )),
                )
            }
            1 => {
                let matched_label = lang_str(lang, "匹配设备", "Matched devices");
                build_footer_segments(
                    &[
                        (
                            lang_str(lang, "Esc: 返回", "Esc: Back"),
                            FooterOperation::Back,
                        ),
                        (
                            lang_str(lang, "Enter: 查看设备", "Enter: View Device"),
                            FooterOperation::Enter,
                        ),
                    ],
                    Some(format!(
                        "{matched_label}: {} {current_device_label}: {}",
                        app.filtered_device_indices().len(),
                        app.selected_device_did().unwrap_or("-")
                    )),
                )
            }
            2 => {
                let matched_label = lang_str(lang, "匹配日志", "Matched logs");
                build_footer_segments(
                    &[(
                        lang_str(lang, "Esc: 返回", "Esc: Back"),
                        FooterOperation::Back,
                    )],
                    Some(format!(
                        "{matched_label}: {}",
                        logs_lines_for_display(app).len()
                    )),
                )
            }
            _ => Vec::new(),
        };
    }

    match app.active_tab {
        0 => build_footer_segments(
            &[
                (
                    lang_str(lang, "A: 新增账户", "A: Add Account"),
                    FooterOperation::AddAccount,
                ),
                (
                    lang_str(lang, "/: 搜索", "/: Search"),
                    FooterOperation::Search,
                ),
                (
                    lang_str(lang, "Enter: 账户操作", "Enter: Account Actions"),
                    FooterOperation::Enter,
                ),
            ],
            None,
        ),
        1 => {
            let total_label = lang_str(lang, "设备总数", "Total Devices");
            build_footer_segments(
                &[
                    (
                        lang_str(lang, "R: 刷新", "R: Refresh"),
                        FooterOperation::Refresh,
                    ),
                    (
                        lang_str(lang, "/: 搜索", "/: Search"),
                        FooterOperation::Search,
                    ),
                    (
                        lang_str(lang, "Enter: 查看设备", "Enter: View Device"),
                        FooterOperation::Enter,
                    ),
                ],
                Some(format!(
                    "{total_label}: {} {current_device_label}: {}",
                    app.devices.len(),
                    app.selected_device_did().unwrap_or("-")
                )),
            )
        }
        3 => build_footer_segments(
            &[(
                lang_str(lang, "Enter: 选择", "Enter: Select"),
                FooterOperation::Enter,
            )],
            Some("".to_string()),
        ),
        _ => build_footer_segments(
            &[
                (
                    lang_str(lang, "C: 清空", "C: Clear"),
                    FooterOperation::ClearLogs,
                ),
                (
                    lang_str(lang, "/: 搜索", "/: Search"),
                    FooterOperation::Search,
                ),
            ],
            Some("".to_string()),
        ),
    }
}

pub(in crate::tui) fn footer_operation_at_column(
    app: &TuiApp,
    column: u16,
) -> Option<FooterOperation> {
    let mut start = 0_u16;
    for segment in footer_segments(app) {
        let width = display_width(segment.text.as_str());
        let end = start.saturating_add(width);
        if column >= start && column < end {
            return segment.operation;
        }
        start = end;
    }
    None
}

pub(in crate::tui) fn execute_footer_operation(
    app: &mut TuiApp,
    operation: FooterOperation,
) -> Result<()> {
    match operation {
        FooterOperation::Refresh => {
            let _ = handle_key(
                app,
                crossterm::event::KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
            )?;
            Ok(())
        }
        FooterOperation::Search => {
            let _ = handle_key(
                app,
                crossterm::event::KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
            )?;
            Ok(())
        }
        FooterOperation::Enter => {
            let _ = handle_key(
                app,
                crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            )?;
            Ok(())
        }
        FooterOperation::SelectRecord => {
            let _ = handle_key(
                app,
                crossterm::event::KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE),
            )?;
            Ok(())
        }
        FooterOperation::DateFilter => {
            let _ = handle_key(
                app,
                crossterm::event::KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
            )?;
            Ok(())
        }
        FooterOperation::ClearDateFilter => {
            let _ = handle_key(
                app,
                crossterm::event::KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
            )?;
            Ok(())
        }
        FooterOperation::Back => {
            let _ = handle_key(
                app,
                crossterm::event::KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            )?;
            Ok(())
        }
        FooterOperation::AddAccount => {
            let _ = handle_key(
                app,
                crossterm::event::KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
            )?;
            Ok(())
        }
        FooterOperation::Copy => {
            copy_reauth_auth_url(app);
            Ok(())
        }
        FooterOperation::ClearLogs => {
            let _ = handle_key(
                app,
                crossterm::event::KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
            )?;
            Ok(())
        }
    }
}

pub(in crate::tui) fn footer_display_text(app: &TuiApp, now_ms: u128) -> String {
    let mut text = footer_text(app);
    if let Some(badge) = footer_copy_badge_text(&app.logs, now_ms) {
        text.push_str(badge);
    }
    text
}

pub(in crate::tui) fn note_copy_success(app: &mut TuiApp) {
    app.log(format!("{FOOTER_COPY_LOG_PREFIX}{}", now_epoch_millis()));
}

pub(in crate::tui) fn copy_reauth_auth_url(app: &mut TuiApp) {
    let Some(auth_url) = app
        .account_action_dialog
        .as_ref()
        .and_then(|dialog| match dialog {
            AccountActionDialog::Reauth { auth_url, .. } if !auth_url.trim().is_empty() => {
                Some(auth_url.clone())
            }
            _ => None,
        })
    else {
        app.log("no auth url to copy".to_string());
        return;
    };

    if let Err(error) = copy_text_to_clipboard(auth_url.as_str()) {
        app.log(format!("copy auth url failed: {error}"));
    } else {
        note_copy_success(app);
    }
}

pub(in crate::tui) fn footer_text(app: &TuiApp) -> String {
    footer_segments(app)
        .into_iter()
        .map(|segment| segment.text)
        .collect::<String>()
}

pub(in crate::tui) fn footer_line(app: &TuiApp, now_ms: u128) -> Line<'static> {
    let mut spans = vec![Span::styled(
        footer_text(app),
        Style::default().add_modifier(Modifier::DIM),
    )];
    if let Some(badge) = footer_copy_badge_text(&app.logs, now_ms) {
        spans.push(Span::styled(
            badge,
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        ));
    }
    Line::from(spans)
}

pub(in crate::tui) fn footer_copy_badge_text(
    logs: &VecDeque<String>,
    now_ms: u128,
) -> Option<&'static str> {
    footer_copy_badge_visible_at(logs, now_ms).then_some(FOOTER_COPY_BADGE_TEXT)
}

pub(in crate::tui) fn footer_copy_badge_visible_at(logs: &VecDeque<String>, now_ms: u128) -> bool {
    let Some(entry) = logs
        .iter()
        .rev()
        .find(|line| line.starts_with(FOOTER_COPY_LOG_PREFIX))
    else {
        return false;
    };
    let Ok(copied_at_ms) = entry[FOOTER_COPY_LOG_PREFIX.len()..].parse::<u128>() else {
        return false;
    };
    now_ms.saturating_sub(copied_at_ms) < 1_000
}

pub(crate) fn extract_auth_url_from_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if let Some(auth_url) = trimmed.strip_prefix("AUTH_URL ") {
        return Some(auth_url.trim().to_string());
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return Some(trimmed.to_string());
    }

    let value = serde_json::from_str::<Value>(trimmed).ok()?;
    let event_type = value
        .get("type")
        .or_else(|| value.get("kind"))
        .and_then(Value::as_str)?;
    if event_type != "authUrlPrinted" {
        return None;
    }
    value
        .get("url")
        .and_then(Value::as_str)
        .map(|url| url.to_string())
}

pub(in crate::tui) fn copy_text_to_clipboard(text: &str) -> Result<()> {
    if let Some(path) = std::env::var_os("MIT_TEST_CLIPBOARD_FILE") {
        fs::write(path, text)?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        let mut child = std::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()?;
        if let Some(stdin) = child.stdin.as_mut() {
            use std::io::Write;
            stdin.write_all(text.as_bytes())?;
        }
        let status = child.wait()?;
        if status.success() {
            Ok(())
        } else {
            bail!("pbcopy exited with status {status}")
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        bail!("clipboard copy is not supported on this platform")
    }
}

#[cfg(not(test))]
pub(crate) fn open_url_in_browser(url: &str) -> Result<()> {
    if std::env::var("MIT_DISABLE_BROWSER_OPEN")
        .ok()
        .is_some_and(|value| {
            let value = value.trim();
            !value.is_empty() && value != "0" && !value.eq_ignore_ascii_case("false")
        })
    {
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(url);
        command
    };

    #[cfg(target_os = "linux")]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };

    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    };

    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        bail!("browser launcher exited with status {status}");
    }
}
