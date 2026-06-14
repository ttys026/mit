use anyhow::{anyhow, bail, Result};
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::text::{Line, Span};
use ratatui::widgets::ListState;
use std::io::BufRead;
#[cfg(not(test))]
use std::io::Read;
#[cfg(not(test))]
use std::thread;
use unicode_width::UnicodeWidthStr;

use crate::mijia_api::is_mijia_auth_present;
use crate::storage::{get_auth_accounts, AuthAccount, Language};
use crate::tui::extract_auth_url_from_line;
#[cfg(not(test))]
use crate::tui::open_url_in_browser;
use crate::tui::shared::{
    display_truncate_pad, display_truncate_pad_with_ellipsis, shrink_largest_width,
    table_header_style,
};
use crate::tui::{
    account_list_header_titles, apply_single_line_textarea_key, device_account_uid, lang_str,
    AccountActionDialog, PropDialog, PropDialogTab, TuiApp,
};
#[cfg(not(test))]
use crate::tui::{
    cancel_active_auth_process, clear_active_auth_process_if_generation, set_active_auth_process,
};

pub(crate) fn format_account_label(account: &AuthAccount) -> String {
    if account.user.nickname.trim().is_empty() {
        account.user.uid.clone()
    } else {
        format!("{}({})", account.user.nickname, account.user.uid)
    }
}

pub(crate) fn parse_auth_login_output_line(line: &str) -> Option<String> {
    extract_auth_url_from_line(line)
}

pub(crate) fn forward_auth_login_output_until_eof<R: BufRead>(
    reader: &mut R,
    tx: &std::sync::mpsc::Sender<Result<String>>,
) {
    let mut line = String::new();
    let mut sent_auth_url = false;
    loop {
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                if !sent_auth_url {
                    if let Some(auth_url) = parse_auth_login_output_line(&line) {
                        let _ = tx.send(Ok(auth_url));
                        sent_auth_url = true;
                    }
                }
                line.clear();
            }
            Err(error) => {
                if !sent_auth_url {
                    let _ = tx.send(Err(error.into()));
                }
                return;
            }
        }
    }
    if !sent_auth_url {
        let _ = tx.send(Err(anyhow!("auth login did not emit auth url")));
    }
}

#[derive(Clone, Debug)]
pub(crate) struct AccountListRow {
    pub(crate) region: String,
    pub(crate) nickname: String,
    pub(crate) uid: String,
    pub(crate) xiaomi_status: String,
    pub(crate) mijia_status: String,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AccountListColumns {
    pub(crate) region: usize,
    pub(crate) nickname: usize,
    pub(crate) uid: usize,
    pub(crate) xiaomi_status: usize,
    pub(crate) mijia_status: usize,
}

impl AccountListColumns {
    fn total_width(self) -> usize {
        self.region + self.nickname + self.uid + self.xiaomi_status + self.mijia_status
    }

    fn shrink_largest(&mut self) -> bool {
        let mut widths = [
            self.region,
            self.nickname,
            self.uid,
            self.xiaomi_status,
            self.mijia_status,
        ];
        if !shrink_largest_width(&mut widths) {
            return false;
        }
        [
            self.region,
            self.nickname,
            self.uid,
            self.xiaomi_status,
            self.mijia_status,
        ] = widths;
        true
    }
}

pub(crate) fn account_list_row(
    account: &AuthAccount,
    offline: bool,
    lang: Language,
) -> AccountListRow {
    let region = if account.region.trim().is_empty() {
        "-"
    } else {
        account.region.trim()
    };
    let nickname = if account.user.nickname.trim().is_empty() {
        "-"
    } else {
        account.user.nickname.trim()
    };
    let uid = if account.user.uid.trim().is_empty() {
        "-"
    } else {
        account.user.uid.trim()
    };
    let has_xiaomi =
        !account.access_token.trim().is_empty() || !account.refresh_token.trim().is_empty();
    let xiaomi_status = match (has_xiaomi, offline, lang) {
        (false, _, Language::Chinese) => "未登录",
        (false, _, Language::English) => "Missing",
        (true, true, Language::Chinese) => "离线",
        (true, true, Language::English) => "Offline",
        (true, false, Language::Chinese) => "已登录",
        (true, false, Language::English) => "Logged In",
    };
    let mijia_status = match (is_mijia_auth_present(account.mijia.as_ref()), lang) {
        (true, Language::Chinese) => "已登录",
        (true, Language::English) => "Logged In",
        (false, Language::Chinese) => "未登录",
        (false, Language::English) => "Missing",
    };
    AccountListRow {
        region: region.to_string().to_uppercase(),
        nickname: nickname.to_string(),
        uid: uid.to_string(),
        xiaomi_status: xiaomi_status.to_string(),
        mijia_status: mijia_status.to_string(),
    }
}

pub(crate) fn compute_account_list_columns(
    rows: &[AccountListRow],
    available_width: usize,
    lang: Language,
) -> AccountListColumns {
    let headers = account_list_header_titles(lang);
    let mut columns = AccountListColumns {
        region: UnicodeWidthStr::width(headers[0]) + 2,
        nickname: UnicodeWidthStr::width(headers[1]) + 2,
        uid: UnicodeWidthStr::width(headers[2]) + 2,
        xiaomi_status: UnicodeWidthStr::width(headers[3]) + 2,
        mijia_status: UnicodeWidthStr::width(headers[4]) + 2,
    };
    for row in rows {
        columns.region = columns
            .region
            .max(UnicodeWidthStr::width(row.region.as_str()) + 2);
        columns.nickname = columns
            .nickname
            .max(UnicodeWidthStr::width(row.nickname.as_str()) + 2);
        columns.uid = columns
            .uid
            .max(UnicodeWidthStr::width(row.uid.as_str()) + 2);
        columns.xiaomi_status = columns
            .xiaomi_status
            .max(UnicodeWidthStr::width(row.xiaomi_status.as_str()) + 2);
        columns.mijia_status = columns
            .mijia_status
            .max(UnicodeWidthStr::width(row.mijia_status.as_str()) + 2);
    }
    while columns.total_width() > available_width && columns.shrink_largest() {}
    columns
}

pub(crate) fn format_account_list_item_with_columns(
    row: &AccountListRow,
    columns: AccountListColumns,
) -> String {
    format!(
        "{}{}{}{}{}",
        display_truncate_pad(&row.region, columns.region),
        display_truncate_pad_with_ellipsis(&row.nickname, columns.nickname),
        display_truncate_pad(&row.uid, columns.uid),
        display_truncate_pad(&row.xiaomi_status, columns.xiaomi_status),
        display_truncate_pad(&row.mijia_status, columns.mijia_status),
    )
}

pub(crate) fn format_account_list_header_with_columns(
    columns: AccountListColumns,
    lang: Language,
) -> String {
    let headers = account_list_header_titles(lang);
    format!(
        "{}{}{}{}{}",
        display_truncate_pad(headers[0], columns.region),
        display_truncate_pad(headers[1], columns.nickname),
        display_truncate_pad(headers[2], columns.uid),
        display_truncate_pad(headers[3], columns.xiaomi_status),
        display_truncate_pad(headers[4], columns.mijia_status),
    )
}

pub(crate) fn format_account_list_header_line_with_columns(
    columns: AccountListColumns,
    lang: Language,
) -> Line<'static> {
    Line::from(Span::styled(
        format_account_list_header_with_columns(columns, lang),
        table_header_style(),
    ))
}

impl TuiApp {
    pub(crate) fn account_open_account_action_dialog(&mut self) -> Result<()> {
        if self.active_tab != 0 {
            return Ok(());
        }
        if self.accounts.get(self.account_index).is_none() {
            bail!("当前没有选中账号");
        }
        self.prop_dialog = None;
        self.account_action_dialog = Some(AccountActionDialog::Menu { selected: 0 });
        Ok(())
    }

    pub(crate) fn account_open_offline_prop_dialog_for_current(&mut self, error: &anyhow::Error) {
        if let Some(device) = self.devices.get(self.device_index) {
            let message = format!("{} ({}) offline: {error}", device.name, device.did);
            self.prop_dialog = Some(PropDialog {
                device_did: device.did.clone(),
                device_name: device.name.clone(),
                account_uid: device_account_uid(device).unwrap_or_default().to_string(),
                items: Vec::new(),
                selected: 0,
                active_tab: PropDialogTab::Writable,
                writable_selected: 0,
                readonly_selected: 0,
                actions: Vec::new(),
                actions_selected: 0,
                writable_list_state: ListState::default(),
                readonly_list_state: ListState::default(),
                actions_list_state: ListState::default(),
                loading: false,
                loading_rx: None,
                status: Some(message.clone()),
                editing: false,
                edit_buffer: String::new(),
                edit_cursor: 0,
                edit_error: None,
                refreshing: false,
                refresh_rx: None,
                statistics_selected_bar: None,
            });
            self.log(message);
        } else {
            self.log(format!("open property dialog failed: {error}"));
        }
    }

    pub(crate) fn account_apply_selected_account_action(&mut self) -> Result<()> {
        let selected = self
            .account_action_dialog
            .as_ref()
            .and_then(|dialog| match dialog {
                AccountActionDialog::Menu { selected } => Some(*selected),
                AccountActionDialog::Reauth { .. } => None,
                AccountActionDialog::PushMessage { .. } => None,
                AccountActionDialog::SettingsConfirm { .. } => None,
            })
            .ok_or_else(|| anyhow!("账号操作菜单未打开"))?;
        let uid = self.current_uid().unwrap_or("-").to_string();
        match selected {
            0 => {
                self.account_action_dialog = Some(AccountActionDialog::PushMessage {
                    uid: uid.clone(),
                    input: String::new(),
                    cursor: 0,
                    return_to_menu_selected: selected,
                });
            }
            1 => {
                let result = self.account_start_reauthenticate_flow("xiaomi");
                self.account_show_reauth_dialog_for_result("登录(小米)", uid.as_str(), result);
            }
            2 => {
                let result = self.account_start_reauthenticate_flow("mijia");
                self.account_show_reauth_dialog_for_result("登录(米家)", uid.as_str(), result);
            }
            3 => {
                self.account_logout_current_account(uid.as_str())?;
                self.account_action_dialog = None;
            }
            _ => {}
        }
        Ok(())
    }

    pub(crate) fn account_push_message_for_account(
        &mut self,
        uid: &str,
        message: &str,
    ) -> Result<()> {
        if uid.trim().is_empty() || uid == "-" {
            bail!("当前没有可推送的账号");
        }
        if message.trim().is_empty() {
            bail!("推送消息不能为空");
        }
        #[cfg(test)]
        {
            self.log(format!("account action: push message ({uid}): {message}"));
            Ok(())
        }
        #[cfg(not(test))]
        {
            let summary = crate::cli::push_to_accounts(message, &[uid.to_string()])?;
            self.log(format!(
                "account action: push message ({uid}): \"{message}\" sent={}, failed={}",
                summary.sent.len(),
                summary.failed.len()
            ));
            Ok(())
        }
    }

    pub(crate) fn account_push_message_push_char(&mut self, ch: char) {
        if let Some(AccountActionDialog::PushMessage { input, cursor, .. }) =
            &mut self.account_action_dialog
        {
            apply_single_line_textarea_key(
                input,
                cursor,
                crossterm::event::KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
            );
        }
    }

    pub(crate) fn account_push_message_backspace(&mut self) {
        if let Some(AccountActionDialog::PushMessage { input, cursor, .. }) =
            &mut self.account_action_dialog
        {
            apply_single_line_textarea_key(
                input,
                cursor,
                crossterm::event::KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
            );
        }
    }

    pub(crate) fn account_push_message_move_left(&mut self) {
        if let Some(AccountActionDialog::PushMessage { input, cursor, .. }) =
            &mut self.account_action_dialog
        {
            apply_single_line_textarea_key(
                input,
                cursor,
                crossterm::event::KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
            );
        }
    }

    pub(crate) fn account_push_message_move_right(&mut self) {
        if let Some(AccountActionDialog::PushMessage { input, cursor, .. }) =
            &mut self.account_action_dialog
        {
            apply_single_line_textarea_key(
                input,
                cursor,
                crossterm::event::KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
            );
        }
    }

    pub(crate) fn account_submit_push_message_dialog(&mut self) -> Result<()> {
        let (uid, message) = match &self.account_action_dialog {
            Some(AccountActionDialog::PushMessage { uid, input, .. }) => {
                (uid.clone(), input.trim().to_string())
            }
            _ => bail!("推送对话框未打开"),
        };
        self.account_push_message_for_account(uid.as_str(), message.as_str())?;
        self.account_action_dialog = None;
        Ok(())
    }

    pub(crate) fn account_logout_current_account(&mut self, uid: &str) -> Result<()> {
        // Remove the account from auth state and delete its on-disk cache via the
        // shared action layer (the same path the CLI's `auth logout` uses), then
        // reconcile the in-memory TUI state below.
        self.auth_state = crate::actions::logout_account(
            &self.home_dir.join(".mit"),
            self.auth_state.clone(),
            uid,
        )?;
        self.accounts = get_auth_accounts(&self.auth_state)?
            .into_iter()
            .filter(|a| !a.user.uid.trim().is_empty())
            .collect::<Vec<_>>();
        if self.account_index >= self.accounts.len() {
            self.account_index = self.accounts.len().saturating_sub(1);
        }

        let removed_dids = self
            .devices
            .iter()
            .filter(|device| device_account_uid(device) == Some(uid))
            .map(|device| device.did.clone())
            .collect::<Vec<_>>();
        self.devices
            .retain(|device| device_account_uid(device) != Some(uid));
        if self.devices.is_empty() {
            self.device_index = 0;
            self.prop_dialog = None;
        } else if self.device_index >= self.devices.len() {
            self.device_index = self.devices.len() - 1;
        }
        for did in removed_dids {
            self.property_cache.clear_device(did.as_str());
        }

        self.offline_account_uids.remove(uid);

        self.bootstrap_pending = None;
        if self.accounts.is_empty() {
            self.active_tab = 0;
        } else {
            self.request_local_transport_refresh(false);
        }
        self.log(format!("account action: logout ({uid})"));
        Ok(())
    }

    pub(crate) fn account_show_reauth_dialog_for_result(
        &mut self,
        action: &str,
        uid: &str,
        result: Result<String>,
    ) {
        match result {
            Ok(auth_url) => {
                self.log(format!("{action} url ({uid}): {auth_url}"));
                self.account_action_dialog = Some(AccountActionDialog::Reauth {
                    status: lang_str(self.language, "等待登录回调", "Waiting for login callback")
                        .to_string(),
                    auth_url,
                });
            }
            Err(error) => {
                let status = format!("Failed to start auth flow: {error}");
                self.log(format!("{action} failed ({uid}): {error}"));
                self.account_action_dialog = Some(AccountActionDialog::Reauth {
                    status,
                    auth_url: "-".to_string(),
                });
            }
        }
    }

    pub(crate) fn account_start_reauthenticate_flow(&mut self, _flow: &str) -> Result<String> {
        #[cfg(test)]
        {
            if let Ok(error_text) = std::env::var("MIT_TUI_TEST_REAUTH_ERROR") {
                if !error_text.trim().is_empty() {
                    return Err(anyhow!("{error_text}"));
                }
            }
            Ok("http://127.0.0.1:8000/login_redirect?code=test&state=test".to_string())
        }

        #[cfg(not(test))]
        {
            self.account_start_login_process(Some(_flow))
        }
    }

    pub(crate) fn account_start_add_account_auth_flow(&mut self) -> Result<()> {
        if self.active_tab != 0 {
            return Ok(());
        }
        self.prop_dialog = None;
        let uid = self.current_uid().unwrap_or("-").to_string();
        let result = self.account_start_combined_auth_flow();
        self.account_show_reauth_dialog_for_result("add-account", uid.as_str(), result);
        Ok(())
    }

    pub(crate) fn account_start_combined_auth_flow(&mut self) -> Result<String> {
        #[cfg(test)]
        {
            if let Ok(error_text) = std::env::var("MIT_TUI_TEST_REAUTH_ERROR") {
                if !error_text.trim().is_empty() {
                    return Err(anyhow!("{error_text}"));
                }
            }
            Ok("http://127.0.0.1:8000/login_redirect?code=test&state=test".to_string())
        }

        #[cfg(not(test))]
        {
            self.account_start_login_process(None)
        }
    }

    #[cfg(not(test))]
    fn account_start_login_process(&mut self, flow: Option<&str>) -> Result<String> {
        use std::sync::mpsc;

        let _ = cancel_active_auth_process(self.auth_flow_generation);
        self.auth_flow_generation = self.auth_flow_generation.saturating_add(1);
        let generation = self.auth_flow_generation;
        let auth_flow_tx = self.auth_flow_tx.clone();
        let executable = std::env::current_exe()?;
        let mut command = std::process::Command::new(executable);
        command
            .arg("--json")
            .arg("auth")
            .arg("login")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        if let Some(flow) = flow {
            command.arg(flow);
        }
        let mut child = command.spawn()?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("failed to capture auth login output"))?;
        let stderr = child.stderr.take();
        set_active_auth_process(generation, child.id());
        let (tx, rx) = mpsc::channel::<Result<String>>();

        thread::spawn(move || {
            let mut reader = std::io::BufReader::new(stdout);
            forward_auth_login_output_until_eof(&mut reader, &tx);
        });

        if let Some(stderr) = stderr {
            thread::spawn(move || {
                let mut reader = std::io::BufReader::new(stderr);
                let mut sink = String::new();
                let _ = reader.read_to_string(&mut sink);
            });
        }

        thread::spawn(move || {
            let status = child.wait();
            clear_active_auth_process_if_generation(generation);
            let success = status.as_ref().map(|s| s.success()).unwrap_or(false);
            let detail = match status {
                Ok(status) if status.success() => "auth login process completed".to_string(),
                Ok(status) => format!("auth login process exited with {status}"),
                Err(error) => format!("auth login process wait failed: {error}"),
            };
            let _ = auth_flow_tx.send(crate::tui::AuthFlowMessage::Completed {
                generation,
                success,
                detail,
            });
        });

        let auth_url = match rx.recv_timeout(std::time::Duration::from_secs(6)) {
            Ok(Ok(auth_url)) => auth_url,
            Ok(Err(error)) => {
                let _ = cancel_active_auth_process(generation);
                return Err(error);
            }
            Err(_) => {
                let _ = cancel_active_auth_process(generation);
                return Err(anyhow!("timed out waiting for auth login url"));
            }
        };

        if let Err(error) = open_url_in_browser(&auth_url) {
            self.log(format!("failed to open browser automatically: {error}"));
        }

        Ok(auth_url)
    }
}

pub(crate) fn open_account_action_dialog(app: &mut TuiApp) -> Result<()> {
    app.account_open_account_action_dialog()
}

pub(crate) fn open_offline_prop_dialog_for_current(app: &mut TuiApp, error: &anyhow::Error) {
    app.account_open_offline_prop_dialog_for_current(error)
}

pub(crate) fn apply_selected_account_action(app: &mut TuiApp) -> Result<()> {
    app.account_apply_selected_account_action()
}

pub(crate) fn push_message_push_char(app: &mut TuiApp, ch: char) {
    app.account_push_message_push_char(ch)
}

pub(crate) fn push_message_backspace(app: &mut TuiApp) {
    app.account_push_message_backspace()
}

pub(crate) fn push_message_move_left(app: &mut TuiApp) {
    app.account_push_message_move_left()
}

pub(crate) fn push_message_move_right(app: &mut TuiApp) {
    app.account_push_message_move_right()
}

pub(crate) fn submit_push_message_dialog(app: &mut TuiApp) -> Result<()> {
    app.account_submit_push_message_dialog()
}

pub(crate) fn start_add_account_auth_flow(app: &mut TuiApp) -> Result<()> {
    app.account_start_add_account_auth_flow()
}
