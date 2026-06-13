use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

#[path = "support/mock_mico_server.rs"]
mod mock_mico_server;

use mock_mico_server::MockMicoServer;

#[test]
fn auth_status_text_mode_is_rejected_as_invalid_subcommand() {
    let test_home = make_temp_dir("mit-cli-output-auth-status");
    write_auth_fixture(&test_home);

    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["auth", "status"])
        .env("MIT_HOME", &test_home)
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert!(!output.status.success());
    assert_eq!(stdout, "");
    assert!(stderr.contains("status"));

    let _ = fs::remove_dir_all(&test_home);
}

#[test]
fn auth_status_json_mode_is_rejected_as_invalid_subcommand() {
    let test_home = make_temp_dir("mit-cli-output-auth-status-json");
    write_auth_fixture(&test_home);

    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["--json", "auth", "status"])
        .env("MIT_HOME", &test_home)
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert!(!output.status.success());
    assert_eq!(stdout, "");
    assert!(stderr.contains("status"));
    assert!(stderr.contains("Usage:"));

    let _ = fs::remove_dir_all(&test_home);
}

#[test]
fn auth_list_json_reports_xiaomi_and_validates_mijia_status() {
    let server = MockMicoServer::start();
    let test_home = make_temp_dir("mit-cli-output-auth-list-status");
    let auth_dir = test_home.join(".mit");
    fs::create_dir_all(&auth_dir).unwrap();
    fs::write(
        auth_dir.join("auth.json"),
        r#"{
  "accounts": [
    {
      "version": 1,
      "xiaomi": {
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "abcd1234abcd1234abcd1234abcd1234",
        "deviceId": "mico.abcd1234abcd1234abcd1234abcd1234",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 32503680000
      },
      "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"},
      "mijia": {
        "ua": "Android-15-test",
        "deviceId": "mijia-device-a",
        "passO": "pass-o-a",
        "ssecurity": "AQIDBAUGBwgJCgsMDQ4PEA==",
        "passToken": "pass-token-a",
        "userId": "1001",
        "cUserId": "c-1001",
        "serviceToken": "service-token-a",
        "expireTime": 222,
        "saveTime": 123
      }
    }
  ],
  "pendingAuth": null
}
"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["--json", "auth", "list"])
        .env("MIT_HOME", &test_home)
        .env("MIT_MIJIA_API_BASE_URL", server.mijia_api_base_url())
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let payload: Value = serde_json::from_str(stdout.trim()).unwrap();

    assert!(output.status.success(), "stderr: {stderr}");
    assert_eq!(payload["accounts"][0]["xiaomiStatus"], "loggedIn");
    assert_eq!(payload["accounts"][0]["mijiaStatus"], "loggedIn");
    let requests = server.requests();
    let check_request = requests
        .iter()
        .find(|request| request.path == "/app/v2/message/v2/check_new_msg")
        .expect("check_new_msg request");
    let cookie = check_request
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("cookie"))
        .map(|(_, value)| value.as_str())
        .unwrap_or_default();
    assert!(cookie.contains("userId=1001;"), "{cookie}");
    assert!(requests
        .iter()
        .any(|request| request.path == "/app/v2/message/v2/check_new_msg"));

    let _ = fs::remove_dir_all(&test_home);
}

#[test]
fn auth_list_json_renews_expired_mijia_service_token() {
    let server = MockMicoServer::start_with_mijia_renewal();
    let test_home = make_temp_dir("mit-cli-output-auth-list-renew-mijia");
    let auth_dir = test_home.join(".mit");
    fs::create_dir_all(&auth_dir).unwrap();
    let auth_path = auth_dir.join("auth.json");
    fs::write(
        &auth_path,
        r#"{
  "accounts": [
    {
      "version": 1,
      "xiaomi": null,
      "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"},
      "mijia": {
        "ua": "Android-15-test",
        "deviceId": "mijia-device-a",
        "passO": "pass-o-a",
        "ssecurity": "AQIDBAUGBwgJCgsMDQ4PEA==",
        "passToken": "pass-token-a",
        "userId": "1001",
        "cUserId": "c-1001",
        "serviceToken": "service-token-old",
        "expireTime": 222,
        "saveTime": 123
      }
    }
  ],
  "pendingAuth": null
}
"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["--json", "auth", "list"])
        .env("MIT_HOME", &test_home)
        .env(
            "MIT_MIJIA_SERVICE_LOGIN_URL",
            server.mijia_service_login_url(),
        )
        .env("MIT_MIJIA_API_BASE_URL", server.mijia_api_base_url())
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let payload: Value = serde_json::from_str(stdout.trim()).unwrap();

    assert!(output.status.success(), "stderr: {stderr}");
    assert_eq!(payload["accounts"][0]["mijiaStatus"], "loggedIn");

    let saved: Value = serde_json::from_str(&fs::read_to_string(&auth_path).unwrap()).unwrap();
    let mijia = &saved["accounts"][0]["mijia"];
    assert_eq!(mijia["serviceToken"], "service-token-renewed");
    assert_eq!(mijia["passToken"], "pass-token-renewed");
    assert_eq!(mijia["userId"], "1001");
    assert!(mijia["expireTime"].as_i64().unwrap() > 222);
    assert!(mijia["saveTime"].as_i64().unwrap() > 123);

    let requests = server.requests();
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.path == "/app/v2/message/v2/check_new_msg")
            .count(),
        2
    );
    assert!(requests
        .iter()
        .any(|request| request.path == "/pass/serviceLogin"));
    assert!(requests
        .iter()
        .any(|request| request.path == "/mijia/renew-callback"));

    let _ = fs::remove_dir_all(&test_home);
}

#[test]
fn auth_list_json_renews_mijia_service_token_after_http_401() {
    let server = MockMicoServer::start_with_mijia_renewal_unauthorized();
    let test_home = make_temp_dir("mit-cli-output-auth-list-renew-mijia-401");
    let auth_dir = test_home.join(".mit");
    fs::create_dir_all(&auth_dir).unwrap();
    let auth_path = auth_dir.join("auth.json");
    fs::write(
        &auth_path,
        r#"{
  "accounts": [
    {
      "version": 1,
      "xiaomi": null,
      "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"},
      "mijia": {
        "ua": "Android-15-test",
        "deviceId": "mijia-device-a",
        "passO": "pass-o-a",
        "ssecurity": "AQIDBAUGBwgJCgsMDQ4PEA==",
        "passToken": "pass-token-a",
        "userId": "1001",
        "cUserId": "c-1001",
        "serviceToken": "service-token-old",
        "expireTime": 222,
        "saveTime": 123
      }
    }
  ],
  "pendingAuth": null
}
"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["--json", "auth", "list"])
        .env("MIT_HOME", &test_home)
        .env(
            "MIT_MIJIA_SERVICE_LOGIN_URL",
            server.mijia_service_login_url(),
        )
        .env("MIT_MIJIA_API_BASE_URL", server.mijia_api_base_url())
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let payload: Value = serde_json::from_str(stdout.trim()).unwrap();

    assert!(output.status.success(), "stderr: {stderr}");
    assert_eq!(payload["accounts"][0]["mijiaStatus"], "loggedIn");

    let saved: Value = serde_json::from_str(&fs::read_to_string(&auth_path).unwrap()).unwrap();
    let mijia = &saved["accounts"][0]["mijia"];
    assert_eq!(mijia["serviceToken"], "service-token-renewed");
    assert_eq!(mijia["passToken"], "pass-token-renewed");
    assert_eq!(mijia["userId"], "1001");
    assert!(mijia["expireTime"].as_i64().unwrap() > 222);
    assert!(mijia["saveTime"].as_i64().unwrap() > 123);

    let requests = server.requests();
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.path == "/app/v2/message/v2/check_new_msg")
            .count(),
        2
    );
    assert!(requests
        .iter()
        .any(|request| request.path == "/pass/serviceLogin"));
    assert!(requests
        .iter()
        .any(|request| request.path == "/mijia/renew-callback"));

    let _ = fs::remove_dir_all(&test_home);
}

#[test]
fn bare_root_json_mode_prints_json() {
    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .arg("--json")
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let payload: Value = serde_json::from_str(stdout.trim()).unwrap();

    assert!(output.status.success());
    assert_eq!(payload["type"], "help");
    assert_eq!(payload["command"], "mit");
    assert!(payload["text"].as_str().unwrap().contains("Usage: mit"));
    assert!(payload["text"].as_str().unwrap().contains("--json"));
}

#[test]
fn bare_root_uses_chinese_summary_by_default() {
    let output = Command::new(env!("CARGO_BIN_EXE_mit")).output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(output.status.success());
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        vec![
            "mit 可用命令：",
            "- auth：登录与账号管理",
            "- devices：列出设备",
            "- props：读写 MIoT 属性和 action",
            "- push：向已登录账号发送通知",
            "- tui：启动全屏 TUI 控制台",
            "",
            "运行 `mit --help` 查看完整帮助。",
        ]
    );
}

#[test]
fn cli_source_does_not_keep_unused_command_formatters() {
    let source = include_str!("../src/cli.rs");

    assert!(!source.contains("pub fn format_props_set_command("));
    assert!(!source.contains("pub fn format_props_act_command("));
    assert!(!source.contains("pub fn format_push_command("));
    assert!(!source.contains("fn format_cli_value_arg("));
}

#[test]
fn devices_list_all_groups_results_by_account_across_multiple_accounts() {
    let server = MockMicoServer::start();
    let test_home = make_temp_dir("mit-cli-output-devices-list-all-multi");
    write_partial_failure_auth_fixture(&test_home);

    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["devices", "list"])
        .env("MIT_HOME", &test_home)
        .env("MIT_MICO_BASE_URL", server.base_url())
        .env("MIT_USER_PROFILE_URL", server.user_profile_url())
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(output.status.success());
    assert!(stdout.contains("账号A（1001）:"));
    assert!(stdout.contains("账号B（1002）:"));
    assert!(stdout.contains("living-room"));
    assert!(stdout.contains("bedroom"));

    let _ = fs::remove_dir_all(&test_home);
}

#[test]
fn push_json_mode_prints_structured_results_end_to_end() {
    let server = MockMicoServer::start();
    let test_home = make_temp_dir("mit-cli-output-push-json");
    write_auth_fixture(&test_home);

    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["--json", "push", "hello world"])
        .env("MIT_HOME", &test_home)
        .env("MIT_MICO_BASE_URL", server.base_url())
        .env("MIT_USER_PROFILE_URL", server.user_profile_url())
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let payload: Value = serde_json::from_str(stdout.trim()).unwrap();

    assert!(output.status.success());
    assert_eq!(payload["type"], "pushResult");
    assert_eq!(payload["sent"].as_array().unwrap().len(), 1);
    assert_eq!(payload["sent"][0]["uid"], "1001");
    assert_eq!(payload["sent"][0]["nickname"], "账号A");
    assert_eq!(payload["sent"][0]["notifyId"], "notify-1001");
    assert_eq!(payload["failed"], serde_json::json!([]));

    let _ = fs::remove_dir_all(&test_home);
}

#[test]
fn push_json_mode_keeps_partial_failures_in_stdout_and_stderr() {
    let server = MockMicoServer::start_with_push_failure_on_attempt(2);
    let test_home = make_temp_dir("mit-cli-output-push-partial-failure");
    write_partial_failure_auth_fixture(&test_home);

    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["--json", "push", "hello world"])
        .env("MIT_HOME", &test_home)
        .env("MIT_MICO_BASE_URL", server.base_url())
        .env("MIT_USER_PROFILE_URL", server.user_profile_url())
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let payload: Value = serde_json::from_str(stdout.trim()).unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(payload["type"], "pushResult");
    assert_eq!(payload["sent"].as_array().unwrap().len(), 1);
    assert_eq!(payload["sent"][0]["uid"], "1001");
    assert_eq!(payload["sent"][0]["notifyId"], "notify-1001");
    assert_eq!(payload["failed"].as_array().unwrap().len(), 1);
    assert_eq!(payload["failed"][0]["uid"], "1002");
    assert_eq!(payload["failed"][0]["nickname"], "账号B");
    assert_eq!(
        payload["failed"][0]["error"],
        "miot api error: code=500 message=simulated push failure"
    );
    assert!(stderr.contains("❌ 部分账号发送失败:"));
    assert!(
        stderr.contains("账号B（1002） => miot api error: code=500 message=simulated push failure")
    );

    let _ = fs::remove_dir_all(&test_home);
}

#[test]
fn props_get_text_mode_prints_value() {
    let server = MockMicoServer::start();
    let test_home = make_temp_dir("mit-cli-output-props-get");
    write_auth_fixture(&test_home);

    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["props", "get", "dev-1", "2", "1"])
        .env("MIT_HOME", &test_home)
        .env("MIT_MICO_BASE_URL", server.base_url())
        .env("MIT_USER_PROFILE_URL", server.user_profile_url())
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(output.status.success());
    assert!(stdout.contains("living-room"));
    assert!(stdout.contains("2.1"));
    assert!(stdout.contains("\"value\":true"));

    let _ = fs::remove_dir_all(&test_home);
}

#[test]
fn props_set_json_mode_prints_structured_result() {
    let server = MockMicoServer::start();
    let test_home = make_temp_dir("mit-cli-output-props-set-json");
    write_auth_fixture(&test_home);

    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["--json", "props", "set", "dev-1", "2", "1", "true"])
        .env("MIT_HOME", &test_home)
        .env("MIT_MICO_BASE_URL", server.base_url())
        .env("MIT_USER_PROFILE_URL", server.user_profile_url())
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let payload: Value = serde_json::from_str(stdout.trim()).unwrap();

    assert!(output.status.success());
    assert_eq!(payload["type"], "propSet");
    assert_eq!(payload["deviceDid"], "dev-1");
    assert_eq!(payload["siid"], 2);
    assert_eq!(payload["piid"], 1);
    assert_eq!(payload["value"], serde_json::json!(true));
    assert!(payload["result"].is_array());

    let _ = fs::remove_dir_all(&test_home);
}

#[test]
fn props_act_json_mode_prints_structured_result() {
    let server = MockMicoServer::start();
    let test_home = make_temp_dir("mit-cli-output-props-act-json");
    write_auth_fixture(&test_home);

    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["--json", "props", "act", "dev-1", "5", "1", "1", "2"])
        .env("MIT_HOME", &test_home)
        .env("MIT_MICO_BASE_URL", server.base_url())
        .env("MIT_USER_PROFILE_URL", server.user_profile_url())
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let payload: Value = serde_json::from_str(stdout.trim()).unwrap();

    assert!(output.status.success());
    assert_eq!(payload["type"], "propAct");
    assert_eq!(payload["deviceDid"], "dev-1");
    assert_eq!(payload["siid"], 5);
    assert_eq!(payload["aiid"], 1);
    assert_eq!(payload["values"], serde_json::json!([1, 2]));
    assert_eq!(payload["result"]["ok"], serde_json::json!(true));

    let _ = fs::remove_dir_all(&test_home);
}

#[test]
fn props_act_json_mode_accepts_separate_value_args() {
    let server = MockMicoServer::start();
    let test_home = make_temp_dir("mit-cli-output-props-act-json-separate-args");
    write_auth_fixture(&test_home);

    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args([
            "--json",
            "props",
            "act",
            "dev-1",
            "5",
            "1",
            r#""arg1""#,
            "true",
        ])
        .env("MIT_HOME", &test_home)
        .env("MIT_MICO_BASE_URL", server.base_url())
        .env("MIT_USER_PROFILE_URL", server.user_profile_url())
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let payload: Value = serde_json::from_str(stdout.trim()).unwrap();

    assert!(output.status.success());
    assert_eq!(payload["type"], "propAct");
    assert_eq!(payload["deviceDid"], "dev-1");
    assert_eq!(payload["siid"], 5);
    assert_eq!(payload["aiid"], 1);
    assert_eq!(payload["values"], serde_json::json!(["arg1", true]));
    assert_eq!(payload["result"]["ok"], serde_json::json!(true));

    let _ = fs::remove_dir_all(&test_home);
}

#[test]
fn props_sub_rejects_json_mode_before_subscribing() {
    let test_home = make_temp_dir("mit-cli-output-props-sub-json");
    write_auth_fixture(&test_home);

    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["--json", "props", "sub"])
        .env("MIT_HOME", &test_home)
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert!(!output.status.success());
    assert_eq!(stdout, "");
    assert!(stderr.contains("props sub 不支持 --json"));

    let _ = fs::remove_dir_all(&test_home);
}

fn make_temp_dir(prefix: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::current_dir()
        .unwrap()
        .join("target")
        .join("test-artifacts")
        .join(format!("{prefix}-{unique}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_auth_fixture(home: &std::path::Path) {
    let auth_dir = home.join(".mit");
    fs::create_dir_all(&auth_dir).unwrap();
    fs::write(
        auth_dir.join("auth.json"),
        r#"{
  "accounts": [
    {
      "version": 1,
      "xiaomi": {
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "abcd1234abcd1234abcd1234abcd1234",
        "deviceId": "mico.abcd1234abcd1234abcd1234abcd1234",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 12345
      },
      "mijia": null,
      "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }
  ],
  "pendingAuth": null
}
"#,
    )
    .unwrap();
}

#[test]
fn source_tests_live_under_tests_folder() {
    use std::path::Path;

    assert!(
        !Path::new("src/tui/tests.rs").exists(),
        "src/tui/tests.rs must not exist; move tests into the tests/ directory"
    );

    let mut offending: Vec<String> = Vec::new();
    fn scan_dir(dir: &std::path::Path, offenders: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                scan_dir(&path, offenders);
            } else if path.extension().map(|s| s == "rs").unwrap_or(false) {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if contains_inline_test_module(&content) {
                        offenders.push(path.to_string_lossy().to_string());
                    }
                }
            }
        }
    }
    scan_dir(Path::new("src"), &mut offending);
    if !offending.is_empty() {
        panic!(
            "Found inline test modules under src/: {}\nMove these modules into tests/ and use #[path = \"../tests/...\"] hooks in source files.",
            offending.join(", ")
        );
    }

    assert_externalized_test_hook(
        include_str!("../src/storage.rs"),
        "#[path = \"../tests/module_tests/storage.rs\"]",
        "src/storage.rs",
    );
    assert_externalized_test_hook(
        include_str!("../src/spec_cache.rs"),
        "#[path = \"../tests/module_tests/spec_cache.rs\"]",
        "src/spec_cache.rs",
    );
    assert_externalized_test_hook(
        include_str!("../src/property_cache.rs"),
        "#[path = \"../tests/module_tests/property_cache.rs\"]",
        "src/property_cache.rs",
    );
    assert_externalized_test_hook(
        include_str!("../src/mico_api.rs"),
        "#[path = \"../tests/module_tests/mico_api.rs\"]",
        "src/mico_api.rs",
    );
    assert_externalized_test_hook(
        include_str!("../src/mijia_api.rs"),
        "#[path = \"../tests/module_tests/mijia_api.rs\"]",
        "src/mijia_api.rs",
    );
    assert_externalized_test_hook(
        include_str!("../src/tui/mod.rs"),
        "#[path = \"../../tests/module_tests/tui.rs\"]",
        "src/tui/mod.rs",
    );
}

fn assert_externalized_test_hook(source: &str, expected_path: &str, file: &str) {
    let sanitized = sanitize_rust_source(source);
    let lines: Vec<&str> = sanitized.lines().collect();

    for i in 0..lines.len().saturating_sub(2) {
        if lines[i].trim_start() == "#[cfg(test)]"
            && lines[i + 1].trim_start() == expected_path
            && lines[i + 2].trim_start() == "mod tests;"
        {
            return;
        }
    }

    panic!(
        "{file} must contain the exact adjacent hook block:\n#[cfg(test)]\n{expected_path}\nmod tests;"
    );
}

fn contains_inline_test_module(source: &str) -> bool {
    let sanitized = sanitize_rust_source(source);
    let lines: Vec<&str> = sanitized.lines().collect();

    for i in 0..lines.len() {
        if lines[i].trim_start() != "#[cfg(test)]" {
            continue;
        }

        if lines
            .get(i + 1)
            .map(|line| line.trim_start())
            .is_some_and(|line| line.starts_with("#[path = \""))
            && lines.get(i + 2).map(|line| line.trim_start()) == Some("mod tests;")
        {
            continue;
        }

        for trimmed in lines.iter().skip(i + 1).map(|line| line.trim_start()) {
            if trimmed == "#[cfg(test)]" {
                break;
            }
            if is_module_declaration(trimmed) {
                return true;
            }
        }
    }

    false
}

fn is_module_declaration(line: &str) -> bool {
    let line = strip_visibility_prefix(line);
    let Some(rest) = line.strip_prefix("mod") else {
        return false;
    };
    if !rest.chars().next().is_some_and(|ch| ch.is_whitespace()) {
        return false;
    }

    let rest = rest.trim_start();
    rest.starts_with('{') || rest.starts_with(';')
}

fn strip_visibility_prefix(line: &str) -> &str {
    let line = line.strip_prefix("pub ").unwrap_or(line);
    let line = line.strip_prefix("pub(crate) ").unwrap_or(line);
    let line = line.strip_prefix("pub(super) ").unwrap_or(line);
    line.strip_prefix("pub(in ")
        .and_then(|rest| rest.find(')').map(|idx| &rest[idx + 1..]))
        .unwrap_or(line)
}

fn sanitize_rust_source(source: &str) -> String {
    enum State {
        Normal,
        LineComment,
        BlockComment(usize),
        String,
        Char,
        RawString(usize),
    }

    let bytes = source.as_bytes();
    let mut output = String::with_capacity(source.len());
    let mut i = 0;
    let mut state = State::Normal;
    let mut escaped = false;

    while i < bytes.len() {
        match state {
            State::Normal => {
                if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    state = State::LineComment;
                    i += 2;
                    continue;
                }
                if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                    state = State::BlockComment(1);
                    i += 2;
                    continue;
                }
                if bytes[i] == b'r' {
                    let mut j = i + 1;
                    while j < bytes.len() && bytes[j] == b'#' {
                        j += 1;
                    }
                    if j < bytes.len() && bytes[j] == b'"' {
                        let hashes = j - i - 1;
                        output.push('r');
                        for _ in 0..hashes {
                            output.push('#');
                        }
                        output.push('"');
                        i = j + 1;
                        state = State::RawString(hashes);
                        continue;
                    }
                }
                if bytes[i] == b'"' {
                    output.push('"');
                    i += 1;
                    state = State::String;
                    escaped = false;
                    continue;
                }
                if bytes[i] == b'\'' {
                    output.push('\'');
                    i += 1;
                    state = State::Char;
                    escaped = false;
                    continue;
                }
                output.push(bytes[i] as char);
                i += 1;
            }
            State::LineComment => {
                if bytes[i] == b'\n' {
                    output.push('\n');
                    i += 1;
                    state = State::Normal;
                } else if bytes[i] == b'\r' {
                    output.push('\r');
                    i += 1;
                    if i < bytes.len() && bytes[i] == b'\n' {
                        output.push('\n');
                        i += 1;
                    }
                    state = State::Normal;
                } else {
                    i += 1;
                }
            }
            State::BlockComment(depth) => {
                if bytes[i] == b'\n' {
                    output.push('\n');
                    i += 1;
                    continue;
                }
                if bytes[i] == b'\r' {
                    output.push('\r');
                    i += 1;
                    if i < bytes.len() && bytes[i] == b'\n' {
                        output.push('\n');
                        i += 1;
                    }
                    continue;
                }
                if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                    state = State::BlockComment(depth + 1);
                    i += 2;
                    continue;
                }
                if bytes[i] == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    let next_depth = depth.saturating_sub(1);
                    i += 2;
                    if next_depth == 0 {
                        state = State::Normal;
                    } else {
                        state = State::BlockComment(next_depth);
                    }
                    continue;
                }
                i += 1;
            }
            State::String => {
                output.push(bytes[i] as char);
                if escaped {
                    escaped = false;
                } else if bytes[i] == b'\\' {
                    escaped = true;
                } else if bytes[i] == b'"' {
                    state = State::Normal;
                }
                i += 1;
            }
            State::Char => {
                output.push(bytes[i] as char);
                if escaped {
                    escaped = false;
                } else if bytes[i] == b'\\' {
                    escaped = true;
                } else if bytes[i] == b'\'' {
                    state = State::Normal;
                }
                i += 1;
            }
            State::RawString(hashes) => {
                output.push(bytes[i] as char);
                if bytes[i] == b'"' {
                    let mut matched = true;
                    for offset in 0..hashes {
                        if i + 1 + offset >= bytes.len() || bytes[i + 1 + offset] != b'#' {
                            matched = false;
                            break;
                        }
                    }
                    if matched {
                        for _ in 0..hashes {
                            i += 1;
                            output.push('#');
                        }
                        state = State::Normal;
                        i += 1;
                        continue;
                    }
                }
                i += 1;
            }
        }
    }

    output
}

fn write_partial_failure_auth_fixture(home: &std::path::Path) {
    let auth_dir = home.join(".mit");
    fs::create_dir_all(&auth_dir).unwrap();
    fs::write(
        auth_dir.join("auth.json"),
        r#"{
  "accounts": [
    {
      "version": 1,
      "xiaomi": {
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "abcd1234abcd1234abcd1234abcd1234",
        "deviceId": "mico.abcd1234abcd1234abcd1234abcd1234",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 12345
      },
      "mijia": null,
      "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    },
    {
      "version": 1,
      "xiaomi": {
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "abcd1234abcd1234abcd1234abcd1235",
        "deviceId": "mico.abcd1234abcd1234abcd1234abcd1235",
        "state": "state-b",
        "accessToken": "token-b",
        "refreshToken": "refresh-b",
        "expiresTs": 67890
      },
      "mijia": null,
      "user": {"uid": "1002", "nickname": "账号B", "icon": "", "unionId": "union-b"}
    }
  ],
  "pendingAuth": null
}
"#,
    )
    .unwrap();
}
