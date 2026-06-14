//! Local OAuth callback HTTP server for browser login: Xiaomi OAuth and
//! Mijia QR login, port-conflict handling, and the minimal HTTP request/response
//! plumbing the callback server uses.
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command as ProcessCommand;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use url::Url;

use crate::mico_api::{parse_callback_input, MicoClient};
use crate::mijia_api::{mijia_qr_html, MijiaClient};
use crate::storage::{
    clear_pending_auth, generate_uuid, get_auth_accounts, get_pending_auth, load_auth,
    normalize_account, save_auth, set_pending_auth, sync_xiaomi_auth, upsert_auth_account,
    AuthAccount, AuthState, MijiaAuth, UserProfile,
};

use super::*;

pub(in crate::cli) fn run_xiaomi_login(
    output_mode: OutputMode,
    default_region: &str,
    mode: AuthLoginMode,
) -> Result<()> {
    let auth_state = load_auth()?;
    let existing_pending = get_pending_auth(&auth_state)?;
    let redirect_uri = resolve_redirect_uri();
    let pending_uuid = existing_pending
        .as_ref()
        .map(|account| account.uuid.clone())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(generate_uuid);
    let mut pending = normalize_account(json!({
        "region": existing_pending
            .as_ref()
            .map(|account| account.region.as_str())
            .unwrap_or(default_region),
        "redirectUri": redirect_uri,
        "uuid": pending_uuid,
        "deviceId": existing_pending
            .as_ref()
            .map(|account| account.device_id.as_str())
            .unwrap_or(""),
        "state": existing_pending
            .as_ref()
            .map(|account| account.state.as_str())
            .unwrap_or(""),
        "accessToken": "",
        "refreshToken": "",
        "expiresTs": 0,
        "user": {
            "uid": "",
            "nickname": "",
            "icon": "",
            "unionId": "",
        }
    }));
    let callback_server = CallbackServer::bind(&pending.redirect_uri)?;
    let client = MicoClient::new(&pending)?;
    pending.device_id = client.device_id.clone();
    pending.state = client.state.clone();
    sync_xiaomi_auth(&mut pending);
    let persisted = save_auth(&set_pending_auth(&auth_state, Some(&pending))?)?;
    let pending_auth = get_pending_auth(&persisted)?.unwrap_or(pending.clone());
    let oauth_auth_url = callback_server.auth_url_with_short_redirect(&client.auth_url(false))?;
    let auth_dialog_url = callback_server.short_redirect_uri();
    emit_auth_login_start(&pending_auth, auth_dialog_url.as_str(), output_mode)?;
    let (_auth_state, account) = callback_server
        .wait_for_callback_and_finish_auth(
            persisted,
            pending_auth,
            oauth_auth_url.as_str(),
            mode == AuthLoginMode::Combined,
        )
        .map_err(|error| {
            anyhow!(
                "{error}\n{}",
                callback_server_port_requirement_tip(callback_server.port)
            )
        })?;
    handle_successful_login(output_mode, &account)?;
    Ok(())
}

pub(in crate::cli) fn run_mijia_login(output_mode: OutputMode) -> Result<()> {
    let redirect_uri = resolve_redirect_uri();
    let callback_server = CallbackServer::bind(&redirect_uri)?;
    let auth_dialog_url = callback_server.short_redirect_uri();
    emit_mijia_login_start(auth_dialog_url.as_str(), output_mode)?;
    let (_auth_state, account) =
        callback_server
            .wait_for_mijia_login(load_auth()?)
            .map_err(|error| {
                anyhow!(
                    "{error}\n{}",
                    callback_server_port_requirement_tip(callback_server.port)
                )
            })?;
    handle_successful_login(output_mode, &account)?;
    Ok(())
}

struct CallbackServer {
    listener: TcpListener,
    callback_path: String,
    login_entry_path: String,
    short_redirect_uri: String,
    port: u16,
}

struct ListeningProcess {
    command: String,
    pid: String,
    user: String,
    name: String,
}

impl CallbackServer {
    fn bind(redirect_uri: &str) -> Result<Self> {
        let parsed = Url::parse(redirect_uri)?;
        if parsed.scheme() != "http" {
            bail!("内置回调服务器仅支持 http redirect-uri");
        }
        if parsed.query().is_some() || parsed.fragment().is_some() {
            bail!("redirect-uri 不能包含 query 或 fragment");
        }
        let host = parsed
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("redirect-uri 缺少 host"))?;
        let port = parsed
            .port_or_known_default()
            .ok_or_else(|| anyhow::anyhow!("redirect-uri 缺少端口"))?;
        let callback_path = match parsed.path() {
            "" => "/".to_string(),
            path => path.to_string(),
        };
        let login_entry_path = "/login".to_string();
        let short_redirect_uri = format!("http://{}:{}{}", host, port, login_entry_path);
        let listener = match TcpListener::bind((host, port)) {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
                handle_port_in_use(port)?;
                TcpListener::bind((host, port))?
            }
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            listener,
            callback_path,
            login_entry_path,
            short_redirect_uri,
            port,
        })
    }

    fn auth_url_with_short_redirect(&self, auth_url: &str) -> Result<String> {
        let mut parsed = Url::parse(auth_url)?;
        let mut pairs = parsed
            .query_pairs()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect::<Vec<_>>();
        let mut replaced = false;
        for (key, value) in &mut pairs {
            if key == "redirect_uri" {
                *value = self.short_redirect_uri.clone();
                replaced = true;
            }
        }
        if !replaced {
            pairs.push(("redirect_uri".to_string(), self.short_redirect_uri.clone()));
        }
        let query = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(
                pairs
                    .iter()
                    .map(|(key, value)| (key.as_str(), value.as_str())),
            )
            .finish();
        parsed.set_query(Some(&query));
        Ok(parsed.to_string())
    }

    fn short_redirect_uri(&self) -> String {
        self.short_redirect_uri.clone()
    }

    fn wait_for_callback_and_finish_auth(
        &self,
        auth_state: AuthState,
        pending_auth: AuthAccount,
        oauth_auth_url: &str,
        include_mijia: bool,
    ) -> Result<(AuthState, AuthAccount)> {
        let timeout_secs: u64 = std::env::var("MIT_LOGIN_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(600);
        let deadline = Instant::now() + Duration::from_secs(timeout_secs);

        self.listener.set_nonblocking(true)?;

        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!(
                    "OAuth 登录超时（超过 {timeout_secs} 秒未收到浏览器回调），请重新运行 `mit auth login`"
                );
            }

            let (mut stream, _) = match self.listener.accept() {
                Ok(pair) => pair,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(100).min(remaining));
                    continue;
                }
                Err(e) => return Err(e.into()),
            };
            stream.set_nonblocking(false)?;
            let request = read_http_request(&mut stream)?;
            let outcome = match parse_http_request_line(&request) {
                Ok(request_line) => {
                    if request_line.method != "GET" {
                        write_html_response(
                            &mut stream,
                            "405 Method Not Allowed",
                            "<!doctype html><meta charset=\"utf-8\"><title>mit</title><p>不支持的请求方法</p>",
                        )?;
                        None
                    } else if request_line.path == self.login_entry_path
                        && self.login_entry_path != self.callback_path
                    {
                        let location = if !request_line.query.is_empty()
                            && query_has_auth_callback_params(&request_line.query)
                        {
                            format!("{}?{}", self.callback_path, request_line.query)
                        } else {
                            oauth_auth_url.to_string()
                        };
                        write_redirect_response(&mut stream, "302 Found", &location)?;
                        None
                    } else if request_line.path != self.callback_path {
                        write_html_response(
                            &mut stream,
                            "404 Not Found",
                            "<!doctype html><meta charset=\"utf-8\"><title>mit</title><p>页面不存在</p>",
                        )?;
                        None
                    } else {
                        let callback_input = if request_line.query.is_empty() {
                            request_line.path.clone()
                        } else {
                            format!("?{}", request_line.query)
                        };
                        if include_mijia {
                            let xiaomi =
                                finish_auth_callback(&auth_state, &pending_auth, &callback_input);
                            Some(match xiaomi {
                                Ok((next_auth_state, account)) => finish_mijia_login_response(
                                    &mut stream,
                                    next_auth_state,
                                    Some(account),
                                )
                                .map(|outcome| (outcome, true)),
                                Err(error) => Err(error),
                            })
                        } else {
                            Some(
                                finish_auth_callback(&auth_state, &pending_auth, &callback_input)
                                    .map(|outcome| (outcome, false)),
                            )
                        }
                    }
                }
                Err(error) => {
                    write_html_response(
                        &mut stream,
                        "400 Bad Request",
                        "<!doctype html><meta charset=\"utf-8\"><title>mit</title><p>请求格式错误</p>",
                    )?;
                    return Err(error);
                }
            };

            let Some(outcome) = outcome else {
                continue;
            };

            match outcome {
                Ok((outcome, response_written)) => {
                    if !response_written {
                        write_html_response(
                            &mut stream,
                            "200 OK",
                            "<!doctype html><meta charset=\"utf-8\"><title>mit</title><p>授权成功，可以关闭此页面。</p>",
                        )?;
                    }
                    return Ok(outcome);
                }
                Err(error) => {
                    let _ = write_html_response(
                        &mut stream,
                        "500 Internal Server Error",
                        "<!doctype html><meta charset=\"utf-8\"><title>mit</title><p>授权失败，请查看终端输出。</p>",
                    );
                    return Err(error);
                }
            }
        }
    }

    fn wait_for_mijia_login(&self, auth_state: AuthState) -> Result<(AuthState, AuthAccount)> {
        let timeout_secs: u64 = std::env::var("MIT_LOGIN_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(600);
        let deadline = Instant::now() + Duration::from_secs(timeout_secs);

        self.listener.set_nonblocking(true)?;

        let mut login_page: Option<MijiaLoginPageState> = None;

        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!(
                    "米家登录超时（超过 {timeout_secs} 秒未打开登录页面），请重新运行 `mit auth login mijia`"
                );
            }

            let (mut stream, _) = match self.listener.accept() {
                Ok(pair) => pair,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(100).min(remaining));
                    continue;
                }
                Err(e) => return Err(e.into()),
            };
            stream.set_nonblocking(false)?;
            let request = read_http_request(&mut stream)?;
            let outcome = match parse_http_request_line(&request) {
                Ok(request_line) => {
                    if request_line.method != "GET" {
                        write_html_response(
                            &mut stream,
                            "405 Method Not Allowed",
                            "<!doctype html><meta charset=\"utf-8\"><title>mit</title><p>不支持的请求方法</p>",
                        )?;
                        None
                    } else if request_line.path == self.login_entry_path {
                        if let Some(page) = &login_page {
                            write_html_response(&mut stream, "200 OK", page.html.as_str())?;
                        } else {
                            let page = start_mijia_qr_login(auth_state.clone(), None)?;
                            write_html_response(&mut stream, "200 OK", page.html.as_str())?;
                            login_page = Some(page);
                        }
                        None
                    } else if request_line.path == MIJIA_LOGIN_STATUS_PATH {
                        match &login_page {
                            Some(page) => write_mijia_login_status_response(&mut stream, page)?,
                            None => {
                                write_json_response(
                                    &mut stream,
                                    "200 OK",
                                    &json!({
                                        "status": "pending",
                                        "message": "等待打开米家登录页面"
                                    }),
                                )?;
                                None
                            }
                        }
                    } else {
                        write_html_response(
                            &mut stream,
                            "404 Not Found",
                            "<!doctype html><meta charset=\"utf-8\"><title>mit</title><p>页面不存在</p>",
                        )?;
                        None
                    }
                }
                Err(error) => {
                    write_html_response(
                        &mut stream,
                        "400 Bad Request",
                        "<!doctype html><meta charset=\"utf-8\"><title>mit</title><p>请求格式错误</p>",
                    )?;
                    return Err(error);
                }
            };

            let Some(outcome) = outcome else {
                continue;
            };

            return outcome;
        }
    }
}

fn handle_port_in_use(port: u16) -> Result<()> {
    let processes = match find_listening_processes(port) {
        Ok(processes) if !processes.is_empty() => processes,
        Ok(_) => {
            eprintln!("端口 {port} 已被占用。");
            eprintln!(
                "可运行 `lsof -nP -iTCP:{port} -sTCP:LISTEN` 查看占用进程，然后用 `sudo kill -9 <PID>` 结束它。"
            );
            eprintln!("{}", callback_server_port_requirement_tip(port));
            return Ok(());
        }
        Err(error) => {
            eprintln!("端口 {port} 已被占用，但无法自动查询占用进程：{error}");
            eprintln!(
                "可运行 `lsof -nP -iTCP:{port} -sTCP:LISTEN` 查看占用进程，然后用 `sudo kill -9 <PID>` 结束它。"
            );
            eprintln!("{}", callback_server_port_requirement_tip(port));
            return Ok(());
        }
    };

    eprintln!("端口 {port} 已被占用：");
    for process in &processes {
        eprintln!(
            "- {} (PID {}, 用户 {}): {}",
            process.command, process.pid, process.user, process.name
        );
    }
    let pids = processes
        .iter()
        .map(|process| process.pid.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    eprintln!("将尝试执行：`kill -9 {pids}`");
    eprintln!("手动命令：`sudo kill -9 {pids}`");

    eprintln!("按 Enter 尝试结束占用进程，按 Ctrl+C 取消。");
    eprint!("> ");
    std::io::stderr().flush()?;
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;

    if let Err(error) = kill_listening_processes(&processes) {
        eprintln!("自动结束进程失败：{error}");
        eprintln!("请手动执行 `sudo kill -9 {pids}` 后重试。");
    } else {
        eprintln!("已尝试结束占用进程，正在重试绑定端口 {port}...");
    }
    eprintln!("{}", callback_server_port_requirement_tip(port));

    Ok(())
}

fn callback_server_port_requirement_tip(port: u16) -> String {
    format!("登录回调必须使用 {port} 端口，请确保 127.0.0.1:{port} 可用，并在浏览器回调完成前保持 mit 进程运行。")
}

fn kill_listening_processes(processes: &[ListeningProcess]) -> Result<()> {
    let pids = processes
        .iter()
        .map(|process| process.pid.as_str())
        .collect::<Vec<_>>();
    if pids.is_empty() {
        bail!("未找到可结束的进程");
    }
    let output = ProcessCommand::new("kill")
        .args(["-9"])
        .args(&pids)
        .output()?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        bail!("kill 退出码: {}", output.status);
    }
    bail!("kill 执行失败: {stderr}");
}

fn find_listening_processes(port: u16) -> Result<Vec<ListeningProcess>> {
    let output = ProcessCommand::new("lsof")
        .args(["-nP", "-sTCP:LISTEN"])
        .arg(format!("-iTCP:{port}"))
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("lsof failed for port {port}: {stderr}"));
    }

    let stdout = String::from_utf8(output.stdout)?;
    let mut processes = Vec::new();
    for line in stdout.lines().skip(1) {
        let columns = line.split_whitespace().collect::<Vec<_>>();
        if columns.len() < 9 {
            continue;
        }
        processes.push(ListeningProcess {
            command: columns[0].to_string(),
            pid: columns[1].to_string(),
            user: columns[2].to_string(),
            name: columns[8..].join(" "),
        });
    }
    Ok(processes)
}

struct HttpRequestLine {
    method: String,
    path: String,
    query: String,
}

fn finish_auth_callback(
    auth_state: &AuthState,
    pending_auth: &AuthAccount,
    raw_input: &str,
) -> Result<(AuthState, AuthAccount)> {
    let mut client = MicoClient::new(pending_auth)?;
    let callback = parse_callback_input(raw_input)?;
    let token = client.exchange_code(&callback.code, &callback.state)?;
    let user = client.get_user_info()?;
    let mut account = pending_auth.clone();
    account.device_id = client.device_id.clone();
    account.state = client.state.clone();
    account.access_token = token.access_token;
    account.refresh_token = token.refresh_token;
    account.expires_ts = token.expires_ts;
    account.user = user;
    sync_xiaomi_auth(&mut account);
    let auth_state = resolve_mijia_accounts_for_xiaomi_uid(auth_state, account.user.uid.as_str());
    let auth_state = save_auth(&clear_pending_auth(&upsert_auth_account(
        &auth_state,
        &account,
    )?)?)?;
    let saved_account = get_auth_accounts(&auth_state)?
        .into_iter()
        .find(|item| item.user.uid == account.user.uid)
        .ok_or_else(|| anyhow!("授权成功后未找到账号 {}", account.user.uid))?;
    Ok((auth_state, saved_account))
}

fn resolve_mijia_accounts_for_xiaomi_uid(auth_state: &AuthState, xiaomi_uid: &str) -> AuthState {
    let xiaomi_uid = xiaomi_uid.trim();
    if xiaomi_uid.is_empty() {
        return auth_state.clone();
    }
    let has_mijia_candidate = auth_state.accounts.iter().any(|account| {
        account.xiaomi.is_none()
            && account.mijia.as_ref().is_some_and(|mijia| {
                mijia.user_id.trim().is_empty()
                    || (mijia.user_id.trim() == xiaomi_uid && account.user.uid.trim() != xiaomi_uid)
            })
    });
    if !has_mijia_candidate {
        return auth_state.clone();
    }

    let Ok(client) = MijiaClient::new() else {
        return auth_state.clone();
    };
    let mut next = auth_state.clone();
    for account in &mut next.accounts {
        if account.xiaomi.is_some() {
            continue;
        }
        let Some(mijia) = account.mijia.as_mut() else {
            continue;
        };
        if mijia.user_id.trim() == xiaomi_uid {
            account.user.uid = xiaomi_uid.to_string();
            continue;
        }
        if !mijia.user_id.trim().is_empty() {
            continue;
        }
        if let Ok(profile) = client.resolve_user_profile(mijia) {
            if profile.uid.trim() == xiaomi_uid {
                mijia.user_id = profile.uid.clone();
                account.user.uid = profile.uid.clone();
                fill_empty_user_profile(&mut account.user, &profile);
            }
        }
    }
    next
}

fn finish_mijia_login_response(
    stream: &mut TcpStream,
    auth_state: AuthState,
    existing_account: Option<AuthAccount>,
) -> Result<(AuthState, AuthAccount)> {
    let client = MijiaClient::new()?;
    let existing_mijia = existing_account
        .as_ref()
        .and_then(|account| account.mijia.as_ref());
    let session = client.prepare_qr_login(existing_mijia)?;
    write_redirect_response(stream, "302 Found", session.login_url.as_str())?;
    finish_mijia_login_session(client, session, auth_state, existing_account)
}

fn start_mijia_qr_login(
    auth_state: AuthState,
    existing_account: Option<AuthAccount>,
) -> Result<MijiaLoginPageState> {
    let client = MijiaClient::new()?;
    let existing_mijia = existing_account
        .as_ref()
        .and_then(|account| account.mijia.as_ref());
    let session = client.prepare_qr_login(existing_mijia)?;
    let html = mijia_qr_html(&session, MIJIA_LOGIN_STATUS_PATH);
    let result = Arc::new(Mutex::new(None));
    let worker_result = Arc::clone(&result);
    std::thread::spawn(move || {
        let outcome = finish_mijia_login_session(client, session, auth_state, existing_account)
            .map_err(|error| error.to_string());
        if let Ok(mut result) = worker_result.lock() {
            *result = Some(outcome);
        }
    });
    Ok(MijiaLoginPageState { html, result })
}

fn finish_mijia_login_session(
    client: MijiaClient,
    session: crate::mijia_api::MijiaLoginSession,
    auth_state: AuthState,
    existing_account: Option<AuthAccount>,
) -> Result<(AuthState, AuthAccount)> {
    let mut mijia_auth = client.finish_qr_login(&session)?;
    let mijia_profile = client.resolve_user_profile(&mijia_auth)?;
    if mijia_auth.user_id.trim().is_empty() {
        mijia_auth.user_id = mijia_profile.uid.clone();
    }
    save_mijia_auth(auth_state, existing_account, mijia_auth, mijia_profile)
}

fn write_mijia_login_status_response(
    stream: &mut TcpStream,
    page: &MijiaLoginPageState,
) -> Result<Option<Result<(AuthState, AuthAccount)>>> {
    let snapshot = page
        .result
        .lock()
        .map_err(|_| anyhow!("米家登录状态锁已损坏"))?
        .clone();
    match snapshot {
        Some(Ok(outcome)) => {
            write_json_response(
                stream,
                "200 OK",
                &json!({
                    "status": "succeeded",
                    "message": "授权成功，可以关闭此页面。"
                }),
            )?;
            Ok(Some(Ok(outcome)))
        }
        Some(Err(message)) => {
            write_json_response(
                stream,
                "200 OK",
                &json!({
                    "status": "failed",
                    "message": format!("米家登录失败：{message}。请重新运行 `mit auth login mijia` 后重试。")
                }),
            )?;
            Ok(Some(Err(anyhow!(message))))
        }
        None => {
            write_json_response(
                stream,
                "200 OK",
                &json!({
                    "status": "pending",
                    "message": "等待米家扫码确认"
                }),
            )?;
            Ok(None)
        }
    }
}

fn save_mijia_auth(
    auth_state: AuthState,
    existing_account: Option<AuthAccount>,
    mijia_auth: MijiaAuth,
    mijia_profile: UserProfile,
) -> Result<(AuthState, AuthAccount)> {
    let profile_uid = mijia_profile.uid.trim();
    if profile_uid.is_empty() {
        bail!("米家登录成功但未能解析小米 UID");
    }
    let existing_account = existing_account.filter(|account| {
        let uid = account.user.uid.trim();
        uid.is_empty() || uid == profile_uid
    });
    let mut account = existing_account.unwrap_or_else(|| {
        normalize_account(json!({
            "xiaomi": null,
            "mijia": serde_json::to_value(&mijia_auth).unwrap_or(Value::Null),
            "user": &mijia_profile,
        }))
    });
    if account.user.uid.trim().is_empty() {
        account.user.uid = mijia_profile.uid.clone();
    }
    fill_empty_user_profile(&mut account.user, &mijia_profile);
    account.mijia = Some(mijia_auth.clone());
    let auth_state = save_auth(&clear_pending_auth(&upsert_auth_account(
        &auth_state,
        &account,
    )?)?)?;
    let saved_uid = account.user.uid.clone();
    let saved_account = get_auth_accounts(&auth_state)?
        .into_iter()
        .find(|item| item.user.uid == saved_uid)
        .ok_or_else(|| anyhow!("米家授权成功后未找到账号 {}", saved_uid))?;
    Ok((auth_state, saved_account))
}

fn fill_empty_user_profile(user: &mut UserProfile, profile: &UserProfile) {
    if user.uid.trim().is_empty() {
        user.uid = profile.uid.clone();
    }
    if user.nickname.trim().is_empty() {
        user.nickname = profile.nickname.clone();
    }
    if user.icon.trim().is_empty() {
        user.icon = profile.icon.clone();
    }
    if user.union_id.trim().is_empty() {
        user.union_id = profile.union_id.clone();
    }
}

fn read_http_request(stream: &mut TcpStream) -> Result<String> {
    let mut data = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        data.extend_from_slice(&buffer[..count]);
        if data.windows(4).any(|chunk| chunk == b"\r\n\r\n") || data.len() >= 16 * 1024 {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&data).into_owned())
}

fn parse_http_request_line(request: &str) -> Result<HttpRequestLine> {
    let line = request
        .lines()
        .next()
        .ok_or_else(|| anyhow::anyhow!("无效的 HTTP 请求"))?;
    let mut parts = line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("无效的 HTTP 请求方法"))?;
    let target = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("无效的 HTTP 请求路径"))?;
    let url = Url::parse(&format!("http://127.0.0.1{target}"))?;
    Ok(HttpRequestLine {
        method: method.to_string(),
        path: url.path().to_string(),
        query: url.query().unwrap_or_default().to_string(),
    })
}

fn query_has_auth_callback_params(query: &str) -> bool {
    let mut has_code = false;
    let mut has_state = false;
    for (key, _) in url::form_urlencoded::parse(query.as_bytes()) {
        if key == "code" {
            has_code = true;
        } else if key == "state" {
            has_state = true;
        }
    }
    has_code && has_state
}

fn write_html_response(stream: &mut TcpStream, status: &str, body: &str) -> Result<()> {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    let _ = stream.shutdown(std::net::Shutdown::Both);
    Ok(())
}

fn write_json_response(stream: &mut TcpStream, status: &str, body: &Value) -> Result<()> {
    let body = serde_json::to_string(body)?;
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json; charset=utf-8\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    let _ = stream.shutdown(std::net::Shutdown::Both);
    Ok(())
}

fn write_redirect_response(stream: &mut TcpStream, status: &str, location: &str) -> Result<()> {
    let response = format!(
        "HTTP/1.1 {status}\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    let _ = stream.shutdown(std::net::Shutdown::Both);
    Ok(())
}

fn resolve_redirect_uri() -> String {
    // MIT_REDIRECT_URI: env-var override used by tests to avoid port conflicts
    std::env::var("MIT_REDIRECT_URI")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| crate::storage::DEFAULT_REDIRECT_URI.to_string())
}
