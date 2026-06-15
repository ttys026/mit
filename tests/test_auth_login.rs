use serde_json::{json, Value};
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

#[path = "support/mock_mico_server.rs"]
mod mock_mico_server;

use mock_mico_server::MockMicoServer;

fn auth_login_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}

#[test]
fn auth_login_starts_callback_server_and_saves_pending_auth_state() {
    let _guard = auth_login_test_guard();
    let test_home = make_temp_dir("mit-login-test");
    let port = pick_free_port();
    let redirect_uri = format!("http://127.0.0.1:{port}/login_redirect");
    let mut child = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["auth", "login"])
        .env("MIT_HOME", &test_home)
        .env("MIT_REDIRECT_URI", redirect_uri.as_str())
        .env("MIT_DISABLE_BROWSER_OPEN", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let auth_path = test_home.join(".mit").join("auth.json");
    for _ in 0..50 {
        if auth_path.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    assert!(auth_path.exists(), "auth.json was not created");

    let _ = TcpListener::bind(("127.0.0.1", port)).unwrap_err();
    thread::sleep(Duration::from_millis(200));

    child.kill().unwrap();
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("打开下面的小米授权链接"));
    assert!(stdout.contains("等待浏览器回调"));
    assert!(stdout.contains(&format!("http://127.0.0.1:{port}/login")));

    let auth: Value = serde_json::from_str(&fs::read_to_string(&auth_path).unwrap()).unwrap();
    let pending = auth.get("pendingAuth").and_then(Value::as_object).unwrap();
    let xiaomi = pending.get("xiaomi").and_then(Value::as_object).unwrap();
    assert_eq!(
        xiaomi.get("uuid").and_then(Value::as_str).unwrap().len(),
        32
    );
    assert!(xiaomi
        .get("deviceId")
        .and_then(Value::as_str)
        .unwrap()
        .starts_with("mico."));
    assert!(!xiaomi
        .get("state")
        .and_then(Value::as_str)
        .unwrap()
        .is_empty());

    let _ = fs::remove_dir_all(&test_home);
}

#[cfg(not(target_os = "windows"))]
#[test]
fn auth_login_text_mode_attempts_to_open_browser_automatically() {
    let _guard = auth_login_test_guard();
    let test_home = make_temp_dir("mit-login-auto-open");
    let opener_dir = test_home.join("bin");
    fs::create_dir_all(&opener_dir).unwrap();
    let opener_log = test_home.join("browser-open.log");
    let opener_path = opener_dir.join(browser_opener_name());
    fs::write(
        &opener_path,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n",
            opener_log.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&opener_path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let port = pick_free_port();
    let redirect_uri = format!("http://127.0.0.1:{port}/login_redirect");
    let path_env = format!(
        "{}:{}",
        opener_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["auth", "login"])
        .env("MIT_HOME", &test_home)
        .env("MIT_REDIRECT_URI", redirect_uri.as_str())
        .env("PATH", path_env)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    for _ in 0..50 {
        if opener_log.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    assert!(opener_log.exists(), "browser opener was not invoked");

    child.kill().unwrap();
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let opener_args = fs::read_to_string(&opener_log).unwrap();

    assert!(stdout.contains("打开下面的小米授权链接"));
    assert!(stdout.contains(&format!("http://127.0.0.1:{port}/login")));
    assert!(opener_args.contains(&format!("http://127.0.0.1:{port}/login")));

    let _ = fs::remove_dir_all(&test_home);
}

#[cfg(not(target_os = "windows"))]
#[test]
fn auth_login_skips_browser_open_when_disabled_for_tests() {
    let _guard = auth_login_test_guard();
    let test_home = make_temp_dir("mit-login-no-auto-open");
    let opener_dir = test_home.join("bin");
    fs::create_dir_all(&opener_dir).unwrap();
    let opener_log = test_home.join("browser-open.log");
    let opener_path = opener_dir.join(browser_opener_name());
    fs::write(
        &opener_path,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n",
            opener_log.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&opener_path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let port = pick_free_port();
    let redirect_uri = format!("http://127.0.0.1:{port}/login_redirect");
    let path_env = format!(
        "{}:{}",
        opener_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["auth", "login"])
        .env("MIT_HOME", &test_home)
        .env("MIT_REDIRECT_URI", redirect_uri.as_str())
        .env("MIT_DISABLE_BROWSER_OPEN", "1")
        .env("PATH", path_env)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let auth_path = test_home.join(".mit").join("auth.json");
    for _ in 0..50 {
        if auth_path.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    assert!(auth_path.exists(), "auth.json was not created");

    for _ in 0..50 {
        if opener_log.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    assert!(!opener_log.exists(), "browser opener should stay disabled");

    child.kill().unwrap();
    let _ = child.wait_with_output().unwrap();

    let _ = fs::remove_dir_all(&test_home);
}

#[test]
fn auth_login_xiaomi_subcommand_uses_existing_oauth_flow() {
    let _guard = auth_login_test_guard();
    let test_home = make_temp_dir("mit-login-xiaomi-subcommand");
    let port = pick_free_port();
    let redirect_uri = format!("http://127.0.0.1:{port}/login_redirect");
    let mut child = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["auth", "login", "xiaomi"])
        .env("MIT_HOME", &test_home)
        .env("MIT_REDIRECT_URI", redirect_uri.as_str())
        .env("MIT_DISABLE_BROWSER_OPEN", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let auth_path = test_home.join(".mit").join("auth.json");
    for _ in 0..50 {
        if auth_path.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    assert!(auth_path.exists(), "auth.json was not created");

    child.kill().unwrap();
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains(&format!("http://127.0.0.1:{port}/login")));

    let _ = fs::remove_dir_all(&test_home);
}

#[test]
fn auth_login_mijia_subcommand_shows_qr_and_saves_mijia_auth() {
    let _guard = auth_login_test_guard();
    let server = MockMicoServer::start();
    let test_home = make_temp_dir("mit-login-mijia-subcommand");
    let port = pick_free_port();
    let redirect_uri = format!("http://127.0.0.1:{port}/login_redirect");
    let child = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["auth", "login", "mijia"])
        .env("MIT_HOME", &test_home)
        .env("MIT_REDIRECT_URI", redirect_uri.as_str())
        .env("MIT_DISABLE_BROWSER_OPEN", "1")
        .env(
            "MIT_MIJIA_SERVICE_LOGIN_URL",
            server.mijia_service_login_url(),
        )
        .env("MIT_MIJIA_LOGIN_URL", server.mijia_login_url())
        .env("MIT_MIJIA_API_BASE_URL", server.mijia_api_base_url())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    wait_for_port_in_use(&redirect_uri);
    let qr_response = send_login_entry_open(&redirect_uri);
    assert!(
        qr_response.contains("请使用米家 App 扫描二维码完成登录"),
        "qr response: {qr_response}"
    );
    assert!(
        qr_response.contains("<img") && qr_response.contains("/mijia/qr.png"),
        "qr response: {qr_response}"
    );
    assert!(
        qr_response.contains("/mijia_login_status")
            && qr_response.contains("授权成功，可以关闭此页面")
            && qr_response.contains("本地登录服务已停止或超时"),
        "qr response: {qr_response}"
    );

    let status_response = wait_for_mijia_status_success(&redirect_uri);
    assert!(
        status_response.contains("\"status\":\"succeeded\"")
            && status_response.contains("授权成功，可以关闭此页面"),
        "status response: {status_response}"
    );

    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "stderr: {stderr}");

    let auth_path = test_home.join(".mit").join("auth.json");
    let auth: Value = serde_json::from_str(&fs::read_to_string(&auth_path).unwrap()).unwrap();
    let mijia = auth["accounts"][0]["mijia"].as_object().unwrap();
    assert_eq!(mijia["userId"], "1001");
    assert_eq!(mijia["serviceToken"], "service-token-a");

    let requests = server.requests();
    assert!(requests
        .iter()
        .any(|request| request.path == "/pass/serviceLogin"));
    assert!(requests
        .iter()
        .any(|request| request.path == "/longPolling/loginUrl"));
    assert!(requests
        .iter()
        .any(|request| request.path == "/longPolling/lp"));

    let _ = fs::remove_dir_all(&test_home);
}

#[test]
fn auth_login_mijia_subcommand_merges_existing_xiaomi_account_by_uid() {
    let _guard = auth_login_test_guard();
    let server = MockMicoServer::start();
    let test_home = make_temp_dir("mit-login-mijia-merge-xiaomi");
    let auth_dir = test_home.join(".mit");
    fs::create_dir_all(&auth_dir).unwrap();
    let auth_path = auth_dir.join("auth.json");
    fs::write(
        &auth_path,
        serde_json::to_string_pretty(&json!({
            "accounts": [
                {
                    "version": 1,
                    "xiaomi": {
                        "region": "cn",
                        "redirectUri": "http://127.0.0.1:8000/login_redirect",
                        "uuid": "uuid-a",
                        "deviceId": "mico.uuid-a",
                        "state": "state-a",
                        "accessToken": "token-a",
                        "refreshToken": "refresh-a",
                        "expiresTs": 111
                    },
                    "mijia": null,
                    "user": {
                        "uid": "1001",
                        "nickname": "账号A",
                        "icon": "icon-a",
                        "unionId": "union-a"
                    }
                }
            ],
            "pendingAuth": null
        }))
        .unwrap(),
    )
    .unwrap();
    let port = pick_free_port();
    let redirect_uri = format!("http://127.0.0.1:{port}/login_redirect");
    let child = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["auth", "login", "mijia"])
        .env("MIT_HOME", &test_home)
        .env("MIT_REDIRECT_URI", redirect_uri.as_str())
        .env("MIT_DISABLE_BROWSER_OPEN", "1")
        .env(
            "MIT_MIJIA_SERVICE_LOGIN_URL",
            server.mijia_service_login_url(),
        )
        .env("MIT_MIJIA_LOGIN_URL", server.mijia_login_url())
        .env("MIT_MIJIA_API_BASE_URL", server.mijia_api_base_url())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    wait_for_port_in_use(&redirect_uri);
    let qr_response = send_login_entry_open(&redirect_uri);
    assert!(
        qr_response.contains("请使用米家 App 扫描二维码完成登录"),
        "qr response: {qr_response}"
    );
    let status_response = wait_for_mijia_status_success(&redirect_uri);
    assert!(
        status_response.contains("\"status\":\"succeeded\""),
        "status response: {status_response}"
    );

    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "stderr: {stderr}");

    let auth: Value = serde_json::from_str(&fs::read_to_string(&auth_path).unwrap()).unwrap();
    let accounts = auth["accounts"].as_array().unwrap();
    assert_eq!(accounts.len(), 1);
    let account = &accounts[0];
    assert!(account["xiaomi"].is_object());
    assert_eq!(account["user"]["uid"], "1001");
    assert_eq!(account["user"]["nickname"], "账号A");
    assert_eq!(account["mijia"]["userId"], "1001");
    assert_eq!(account["mijia"]["serviceToken"], "service-token-a");

    let _ = fs::remove_dir_all(&test_home);
}

#[test]
fn auth_login_xiaomi_subcommand_merges_existing_mijia_account_by_resolved_uid() {
    let _guard = auth_login_test_guard();
    let server = MockMicoServer::start();
    let test_home = make_temp_dir("mit-login-xiaomi-merge-mijia");
    let auth_dir = test_home.join(".mit");
    fs::create_dir_all(&auth_dir).unwrap();
    let auth_path = auth_dir.join("auth.json");
    fs::write(
        &auth_path,
        serde_json::to_string_pretty(&json!({
            "accounts": [
                {
                    "version": 1,
                    "xiaomi": null,
                    "mijia": {
                        "ua": "Android-15-test",
                        "deviceId": "mijia-device-a",
                        "passO": "pass-o-a",
                        "ssecurity": "AQIDBAUGBwgJCgsMDQ4PEA==",
                        "passToken": "pass-token-a",
                        "userId": "",
                        "cUserId": "c-1001",
                        "serviceToken": "service-token-a",
                        "expireTime": 222,
                        "saveTime": 123
                    },
                    "user": {
                        "uid": "c-1001",
                        "nickname": "",
                        "icon": "",
                        "unionId": ""
                    }
                }
            ],
            "pendingAuth": null
        }))
        .unwrap(),
    )
    .unwrap();
    let port = pick_free_port();
    let redirect_uri = format!("http://127.0.0.1:{port}/login_redirect");
    let child = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["auth", "login", "xiaomi"])
        .env("MIT_HOME", &test_home)
        .env("MIT_REDIRECT_URI", redirect_uri.as_str())
        .env("MIT_DISABLE_BROWSER_OPEN", "1")
        .env("MIT_MICO_BASE_URL", server.base_url())
        .env("MIT_USER_PROFILE_URL", server.user_profile_url())
        .env("MIT_MIJIA_API_BASE_URL", server.mijia_api_base_url())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    for _ in 0..50 {
        if auth_path.exists() {
            let auth: Value =
                serde_json::from_str(&fs::read_to_string(&auth_path).unwrap()).unwrap();
            if auth.get("pendingAuth").and_then(Value::as_object).is_some() {
                break;
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    let auth: Value = serde_json::from_str(&fs::read_to_string(&auth_path).unwrap()).unwrap();
    let state = auth
        .get("pendingAuth")
        .and_then(Value::as_object)
        .and_then(|pending| pending.get("xiaomi"))
        .and_then(Value::as_object)
        .and_then(|xiaomi| xiaomi.get("state"))
        .and_then(Value::as_str)
        .unwrap()
        .to_string();

    let callback_response = send_callback(&redirect_uri, &state);
    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "stderr: {stderr}");
    assert!(
        callback_response.contains("授权成功，可以关闭此页面"),
        "callback response: {callback_response}"
    );

    let auth: Value = serde_json::from_str(&fs::read_to_string(&auth_path).unwrap()).unwrap();
    let accounts = auth["accounts"].as_array().unwrap();
    assert_eq!(accounts.len(), 1);
    let account = &accounts[0];
    assert!(account["xiaomi"].is_object());
    assert_eq!(account["user"]["uid"], "1001");
    assert_eq!(account["user"]["nickname"], "账号A");
    assert_eq!(account["mijia"]["userId"], "1001");
    assert_eq!(account["mijia"]["serviceToken"], "service-token-a");
    let requests = server.requests();
    assert!(requests
        .iter()
        .any(|request| request.path == "/app/v2/homeroom/gethome_merged"));

    let _ = fs::remove_dir_all(&test_home);
}

#[test]
fn auth_login_rejects_redirect_uri_flag() {
    let _guard = auth_login_test_guard();
    let test_home = make_temp_dir("mit-login-reject-flag");
    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args([
            "auth",
            "login",
            "--redirect-uri",
            "http://127.0.0.1:9999/login_redirect",
        ])
        .env("MIT_HOME", &test_home)
        .env("MIT_DISABLE_BROWSER_OPEN", "1")
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "expected failure when --redirect-uri is passed"
    );
    let _ = fs::remove_dir_all(&test_home);
}

#[test]
fn auth_login_times_out_when_no_browser_connects() {
    let _guard = auth_login_test_guard();
    let test_home = make_temp_dir("mit-login-timeout");
    let port = pick_free_port();
    let redirect_uri = format!("http://127.0.0.1:{port}/login_redirect");
    let child = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["auth", "login"])
        .env("MIT_HOME", &test_home)
        .env("MIT_REDIRECT_URI", redirect_uri.as_str())
        .env("MIT_DISABLE_BROWSER_OPEN", "1")
        .env("MIT_LOGIN_TIMEOUT_SECS", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let auth_path = test_home.join(".mit").join("auth.json");
    for _ in 0..50 {
        if auth_path.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    assert!(auth_path.exists(), "auth.json was not created");

    let output = child.wait_with_output().unwrap();
    assert!(
        !output.status.success(),
        "expected process to exit with failure on timeout"
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("超时"),
        "expected timeout message in stderr, got: {stderr}"
    );

    let _ = fs::remove_dir_all(&test_home);
}

#[test]
fn auth_login_json_emits_ndjson_progress() {
    let _guard = auth_login_test_guard();
    let server = MockMicoServer::start();
    let test_home = make_temp_dir("mit-login-json-test");
    let port = pick_free_port();
    let redirect_uri = format!("http://127.0.0.1:{port}/login_redirect");
    let child = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["--json", "auth", "login"])
        .env("MIT_HOME", &test_home)
        .env("MIT_REDIRECT_URI", redirect_uri.as_str())
        .env("MIT_MICO_BASE_URL", server.base_url())
        .env("MIT_USER_PROFILE_URL", server.user_profile_url())
        .env(
            "MIT_MIJIA_SERVICE_LOGIN_URL",
            server.mijia_service_login_url(),
        )
        .env("MIT_MIJIA_LOGIN_URL", server.mijia_login_url())
        .env("MIT_MIJIA_API_BASE_URL", server.mijia_api_base_url())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let auth_path = test_home.join(".mit").join("auth.json");
    for _ in 0..50 {
        if auth_path.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    assert!(auth_path.exists(), "auth.json was not created");

    let auth: Value = serde_json::from_str(&fs::read_to_string(&auth_path).unwrap()).unwrap();
    let state = auth
        .get("pendingAuth")
        .and_then(Value::as_object)
        .and_then(|pending| pending.get("xiaomi"))
        .and_then(Value::as_object)
        .and_then(|xiaomi| xiaomi.get("state"))
        .and_then(Value::as_str)
        .unwrap()
        .to_string();

    let login_open_response = send_login_entry_open(&redirect_uri);
    let callback_redirect_response = send_login_entry_callback(&redirect_uri, &state);
    let callback_response = send_callback(&redirect_uri, &state);
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let events = stdout
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let requests = server.requests();

    assert!(
        output.status.success(),
        "stderr: {stderr}\nrequests: {requests:#?}"
    );
    assert!(
        login_open_response.starts_with("HTTP/1.1 302 Found"),
        "login open response: {login_open_response}\nstderr: {stderr}\nstdout: {stdout}"
    );
    assert!(
        login_open_response.contains("Location: https://account.xiaomi.com/oauth2/authorize?"),
        "login open response: {login_open_response}\nstderr: {stderr}\nstdout: {stdout}"
    );
    assert!(
        login_open_response.contains(&format!(
            "redirect_uri=http%3A%2F%2F127.0.0.1%3A{port}%2Flogin"
        )),
        "login open response: {login_open_response}\nstderr: {stderr}\nstdout: {stdout}"
    );
    assert!(
        callback_redirect_response.starts_with("HTTP/1.1 302 Found"),
        "login callback response: {callback_redirect_response}\nstderr: {stderr}\nstdout: {stdout}"
    );
    assert!(
        callback_redirect_response.contains("Location: /login_redirect?"),
        "login entry response: {callback_redirect_response}\nstderr: {stderr}\nstdout: {stdout}"
    );
    assert!(
        callback_response.starts_with("HTTP/1.1 302 Found"),
        "callback response: {callback_response}\nstderr: {stderr}\nstdout: {stdout}"
    );
    assert!(
        callback_response.contains("Location: ") && callback_response.contains("/mijia/qr-login"),
        "callback response: {callback_response}\nstderr: {stderr}\nstdout: {stdout}"
    );
    assert!(
        !callback_response.contains("请使用米家 App 扫描二维码完成登录")
            && !callback_response.contains("<img"),
        "callback response: {callback_response}\nstderr: {stderr}\nstdout: {stdout}"
    );
    assert_eq!(events[0]["type"], "authUrlPrinted");
    let auth_url = events[0]["url"].as_str().unwrap_or_default();
    assert_eq!(auth_url, format!("http://127.0.0.1:{port}/login"));
    assert_eq!(events[1]["type"], "authWaiting");
    assert_eq!(events[1]["region"], "cn");
    assert_eq!(events[1]["redirectUri"], redirect_uri);
    let success_index = events
        .iter()
        .position(|event| event["type"] == "authLoginSucceeded")
        .expect("missing authLoginSucceeded");
    assert!(success_index > 1, "stdout: {stdout}");
    assert_eq!(events[success_index]["uid"], "1001");
    assert_eq!(events[success_index]["nickname"], "账号A");
    assert!(requests
        .iter()
        .any(|request| request.path == "/app/v2/mico/oauth/get_token"));
    assert!(requests
        .iter()
        .any(|request| request.path == "/user/profile"));
    assert!(requests
        .iter()
        .any(|request| request.path == "/app/v2/oauth/get_uid_by_unionid"));
    assert!(requests
        .iter()
        .any(|request| request.path == "/longPolling/lp"));

    let auth: Value = serde_json::from_str(&fs::read_to_string(&auth_path).unwrap()).unwrap();
    let mijia = auth["accounts"][0]["mijia"].as_object().unwrap();
    assert_eq!(mijia["serviceToken"], "service-token-a");

    let _ = fs::remove_dir_all(&test_home);
}

#[test]
fn auth_login_handles_chunked_callback_request() {
    let _guard = auth_login_test_guard();
    let server = MockMicoServer::start();
    let test_home = make_temp_dir("mit-login-chunked-callback");
    let port = pick_free_port();
    let redirect_uri = format!("http://127.0.0.1:{port}/login_redirect");
    let child = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["--json", "auth", "login"])
        .env("MIT_HOME", &test_home)
        .env("MIT_REDIRECT_URI", redirect_uri.as_str())
        .env("MIT_MICO_BASE_URL", server.base_url())
        .env("MIT_USER_PROFILE_URL", server.user_profile_url())
        .env(
            "MIT_MIJIA_SERVICE_LOGIN_URL",
            server.mijia_service_login_url(),
        )
        .env("MIT_MIJIA_LOGIN_URL", server.mijia_login_url())
        .env("MIT_MIJIA_API_BASE_URL", server.mijia_api_base_url())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let auth_path = test_home.join(".mit").join("auth.json");
    for _ in 0..50 {
        if auth_path.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    assert!(auth_path.exists(), "auth.json was not created");

    let auth: Value = serde_json::from_str(&fs::read_to_string(&auth_path).unwrap()).unwrap();
    let state = auth
        .get("pendingAuth")
        .and_then(Value::as_object)
        .and_then(|pending| pending.get("xiaomi"))
        .and_then(Value::as_object)
        .and_then(|xiaomi| xiaomi.get("state"))
        .and_then(Value::as_str)
        .unwrap()
        .to_string();

    let callback_response = send_callback_in_chunks(&redirect_uri, &state);
    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert!(output.status.success(), "stderr: {stderr}");
    assert!(
        callback_response.starts_with("HTTP/1.1 302 Found"),
        "callback response: {callback_response}\nstderr: {stderr}"
    );
    assert!(
        callback_response.contains("Location: ") && callback_response.contains("/mijia/qr-login"),
        "callback response: {callback_response}\nstderr: {stderr}"
    );
    assert!(
        !callback_response.contains("请使用米家 App 扫描二维码完成登录")
            && !callback_response.contains("<img"),
        "callback response: {callback_response}\nstderr: {stderr}"
    );

    let _ = fs::remove_dir_all(&test_home);
}

fn send_callback(redirect_uri: &str, state: &str) -> String {
    let parsed = url::Url::parse(redirect_uri).unwrap();
    let host = parsed.host_str().unwrap();
    let port = parsed.port_or_known_default().unwrap();
    let path = parsed.path();
    let mut stream = std::net::TcpStream::connect((host, port)).unwrap();
    stream
        .write_all(
            format!(
                "GET {}?code=test-code&state={} HTTP/1.1\r\nHost: {}:{}\r\nConnection: close\r\n\r\n",
                path, state, host, port
            )
            .as_bytes(),
        )
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

fn send_callback_in_chunks(redirect_uri: &str, state: &str) -> String {
    let parsed = url::Url::parse(redirect_uri).unwrap();
    let host = parsed.host_str().unwrap();
    let port = parsed.port_or_known_default().unwrap();
    let path = parsed.path();
    let mut stream = std::net::TcpStream::connect((host, port)).unwrap();
    let first_chunk = format!(
        "GET {}?code=test-code&state={} HTTP/1.1\r\nHost: {}:{}\r\n",
        path, state, host, port
    );
    stream.write_all(first_chunk.as_bytes()).unwrap();
    thread::sleep(Duration::from_millis(150));
    let _ = stream.write_all(b"Connection: close\r\n\r\n");
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    response
}

fn send_login_entry_callback(redirect_uri: &str, state: &str) -> String {
    let parsed = url::Url::parse(redirect_uri).unwrap();
    let host = parsed.host_str().unwrap();
    let port = parsed.port_or_known_default().unwrap();
    let mut stream = std::net::TcpStream::connect((host, port)).unwrap();
    stream
        .write_all(
            format!(
                "GET /login?code=entry-code&state={} HTTP/1.1\r\nHost: {}:{}\r\nConnection: close\r\n\r\n",
                state, host, port
            )
            .as_bytes(),
        )
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

fn send_login_entry_open(redirect_uri: &str) -> String {
    send_get_path(redirect_uri, "/login")
}

fn wait_for_mijia_status_success(redirect_uri: &str) -> String {
    let mut last_response = String::new();
    for _ in 0..50 {
        last_response = send_get_path(redirect_uri, "/mijia_login_status");
        if last_response.contains("\"status\":\"succeeded\"") {
            return last_response;
        }
        thread::sleep(Duration::from_millis(100));
    }
    panic!("mijia login status did not succeed: {last_response}");
}

fn send_get_path(redirect_uri: &str, path: &str) -> String {
    let parsed = url::Url::parse(redirect_uri).unwrap();
    let host = parsed.host_str().unwrap();
    let port = parsed.port_or_known_default().unwrap();
    let mut stream = std::net::TcpStream::connect((host, port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .unwrap();
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    response
}

fn pick_free_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn wait_for_port_in_use(redirect_uri: &str) {
    let parsed = url::Url::parse(redirect_uri).unwrap();
    let host = parsed.host_str().unwrap();
    let port = parsed.port_or_known_default().unwrap();
    for _ in 0..50 {
        if TcpListener::bind((host, port)).is_err() {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }
    panic!("callback server did not bind {host}:{port}");
}

fn make_temp_dir(prefix: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::current_dir()
        .unwrap()
        .join("target")
        .join("test-artifacts")
        .join(format!("{prefix}-{unique}-{}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    path
}

#[cfg(target_os = "macos")]
fn browser_opener_name() -> &'static str {
    "open"
}

#[cfg(target_os = "linux")]
fn browser_opener_name() -> &'static str {
    "xdg-open"
}
