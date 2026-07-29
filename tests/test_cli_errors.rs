use std::fs;
use std::net::TcpListener;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn invalid_region_returns_parse_error() {
    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["auth", "login", "--region", "moon"])
        .output()
        .unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(stderr.contains("invalid value"));
    assert!(stderr.contains("moon"));
}

#[test]
fn unknown_subcommand_returns_usage_error() {
    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["auth", "wat"])
        .output()
        .unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(stderr.contains("Usage:"));
}

#[test]
fn root_help_subcommand_is_rejected() {
    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .arg("help")
        .output()
        .unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(stderr.contains("help"));
    assert!(stderr.contains("Usage:"));
}

#[test]
fn nested_help_subcommand_is_rejected() {
    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["auth", "help"])
        .output()
        .unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(stderr.contains("help"));
    assert!(stderr.contains("Usage:"));
}

#[test]
#[cfg(unix)]
fn auth_login_port_in_use_prompts_and_shows_sudo_command_on_kill_failure() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let test_home = make_temp_dir("mit-cli-auth-login-port-in-use");
    let fake_bin = make_temp_dir("mit-cli-auth-login-port-fake-bin");
    write_fake_lsof(&fake_bin, port);
    write_fake_kill(&fake_bin);

    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["auth", "login"])
        .env("MIT_HOME", &test_home)
        .env(
            "MIT_REDIRECT_URI",
            format!("http://127.0.0.1:{port}/login_redirect"),
        )
        .env(
            "PATH",
            format!("{}:{}", fake_bin.display(), std::env::var("PATH").unwrap()),
        )
        .output()
        .unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert!(!output.status.success());
    assert!(stderr.contains(&format!("端口 {port} 已被占用")));
    assert!(stderr.contains("TestProc"));
    assert!(stderr.contains("12345"));
    assert!(stderr.contains("将尝试执行：`kill -9 12345`"));
    assert!(stderr.contains("手动命令：`sudo kill -9 12345`"));
    assert!(stderr.contains("按 Enter 尝试结束占用进程，按 Ctrl+C 取消。"));
    assert!(stderr.contains("请手动执行 `sudo kill -9 12345` 后重试。"));
    assert!(stderr.contains(&format!("登录回调必须使用 {port} 端口")));

    let _ = fs::remove_dir_all(&test_home);
    let _ = fs::remove_dir_all(&fake_bin);
}

#[test]
fn empty_push_text_returns_usage_error() {
    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["push", "   "])
        .output()
        .unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(stderr.contains("值不能为空"));
}

#[test]
fn json_flag_keeps_runtime_errors_human_readable_on_stderr() {
    let test_home = make_temp_dir("mit-cli-json-runtime-error");
    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["--json", "devices", "list"])
        .env("MIT_HOME", &test_home)
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(stdout, "");
    assert!(stderr.contains("未授权，请先执行 mit auth login"));
    assert!(!stderr.trim_start().starts_with('{'));

    let _ = fs::remove_dir_all(&test_home);
}

#[test]
fn bare_auth_shows_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .arg("auth")
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(output.status.success());
    assert!(stdout.contains("登录与账号管理"));
    assert!(stdout.contains("login"));
    assert!(stdout.contains("通过浏览器登录小米账号"));
    assert!(!stdout.contains("status"));
    assert!(stdout.contains("list"));
    assert!(stdout.contains("列出已保存的小米账号"));
}

#[test]
fn bare_devices_shows_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .arg("devices")
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(output.status.success());
    assert!(stdout.contains("列出设备"));
    assert!(stdout.contains("list"));
    assert!(stdout.contains("列出所有已登录账号的设备"));
}

#[test]
fn bare_third_party_shows_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .arg("third-party")
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(output.status.success());
    assert!(stdout.contains("管理三方平台设备"));
    assert!(stdout.contains("list"));
    assert!(stdout.contains("列出已绑定三方平台及设备"));
    assert!(stdout.contains("sync"));
    assert!(stdout.contains("同步已绑定三方平台的设备状态"));
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

#[cfg(unix)]
fn write_fake_lsof(dir: &std::path::Path, port: u16) {
    let script = dir.join("lsof");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\ncat <<'EOF'\nCOMMAND PID USER FD TYPE DEVICE SIZE/OFF NODE NAME\nTestProc 12345 troy 10u IPv4 0x123 0t0 TCP *:{port} (LISTEN)\nEOF\n"
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();
    }
}

#[cfg(unix)]
fn write_fake_kill(dir: &std::path::Path) {
    let script = dir.join("kill");
    fs::write(
        &script,
        "#!/bin/sh\necho \"Operation not permitted\" 1>&2\nexit 1\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();
    }
}
