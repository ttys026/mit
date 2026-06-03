use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn root_help_lists_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .arg("--help")
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("auth"));
    assert!(stdout.contains("登录与账号管理"));
    assert!(stdout.contains("devices"));
    assert!(stdout.contains("列出设备"));
    assert!(stdout.contains("props"));
    assert!(stdout.contains("读写 MIoT 属性和 action"));
    assert!(stdout.contains("push"));
    assert!(stdout.contains("向已登录账号发送通知"));
    assert!(stdout.contains("tui"));
    assert!(stdout.contains("启动全屏 TUI 控制台"));
    assert!(stdout.contains("--json"));
    assert!(stdout.contains("使用 JSON 格式输出"));
    assert!(!stdout.contains("help     Print this message or the help of the given subcommand(s)"));
}

#[test]
fn bare_root_prints_command_summary() {
    let bare = Command::new(env!("CARGO_BIN_EXE_mit")).output().unwrap();

    assert!(bare.status.success());
    assert_eq!(String::from_utf8(bare.stderr).unwrap(), "");
    let stdout = String::from_utf8(bare.stdout).unwrap();
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
fn nested_help_lists_auth_subcommands() {
    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["auth", "--help"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("login"));
    assert!(stdout.contains("通过浏览器登录小米账号"));
    assert!(!stdout.contains("status"));
    assert!(stdout.contains("list"));
    assert!(stdout.contains("列出已保存的小米账号"));
    assert!(!stdout.contains("help     Print this message or the help of the given subcommand(s)"));
}

#[test]
fn auth_login_help_lists_argument_descriptions() {
    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["auth", "login", "--help"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(stdout.contains("登录区域"));
    assert!(
        !stdout.contains("redirect-uri"),
        "redirect-uri flag should not appear in help"
    );
}

#[test]
fn root_help_alias_h_in_help_flag_triggers_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .arg("-H")
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(output.status.success());
    assert!(stdout.contains("Usage: mit"));
}

#[test]
fn version_flags_print_cargo_version() {
    for flag in ["--version", "-V", "-v"] {
        let output = Command::new(env!("CARGO_BIN_EXE_mit"))
            .arg(flag)
            .output()
            .unwrap();
        let stdout = String::from_utf8(output.stdout).unwrap();

        assert!(output.status.success(), "flag={flag}, stdout={stdout}");
        assert!(
            stdout.contains(env!("CARGO_PKG_VERSION")),
            "flag={flag}, stdout={stdout}"
        );
        assert!(stdout.contains("mit"), "flag={flag}, stdout={stdout}");
    }
}

#[test]
fn nested_help_alias_h_in_help_flag_triggers_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["auth", "login", "-H"])
        .output()
        .unwrap();
    let stdout = normalize_help_output(&String::from_utf8(output.stdout).unwrap());
    let stderr = normalize_help_output(&String::from_utf8(output.stderr).unwrap());

    assert!(output.status.success());
    assert!(
        (stdout.contains("Usage: mit") && stdout.contains("auth login"))
            || (stderr.contains("Usage: mit") && stderr.contains("auth login")),
        "stdout={stdout}\nstderr={stderr}"
    );
}

#[test]
fn nested_help_lists_devices_subcommands() {
    let devices_output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["devices", "--help"])
        .output()
        .unwrap();
    let devices_stdout = String::from_utf8(devices_output.stdout).unwrap();
    assert!(devices_stdout.contains("list"));
    assert!(devices_stdout.contains("列出所有已登录账号的设备"));
}

#[test]
fn nested_help_lists_push_subcommands() {
    let push_output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["push", "--help"])
        .output()
        .unwrap();
    let push_stdout = String::from_utf8(push_output.stdout).unwrap();

    assert!(push_stdout.contains("向已登录账号发送通知"));
}

#[test]
fn nested_help_lists_props_subcommands() {
    let props_output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["props", "--help"])
        .output()
        .unwrap();
    let props_stdout = String::from_utf8(props_output.stdout).unwrap();

    assert!(props_stdout.contains("get"));
    assert!(props_stdout.contains("读取一个属性"));
    assert!(props_stdout.contains("set"));
    assert!(props_stdout.contains("写入一个属性"));
    assert!(props_stdout.contains("act"));
    assert!(props_stdout.contains("调用一个 action"));
    assert!(props_stdout.contains("sub"));
    assert!(props_stdout.contains("订阅属性变化"));
}

#[test]
fn props_sub_help_lists_optional_subscription_arguments() {
    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["props", "sub", "--help"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(stdout.contains("订阅属性变化"));
    assert!(stdout.contains("设备 DID"));
    assert!(stdout.contains("服务 IID"));
    assert!(stdout.contains("属性 IID"));
}

#[test]
fn props_sub_requires_siid_and_piid_together() {
    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["props", "sub", "dev-1", "2"])
        .output()
        .unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert!(!output.status.success());
    assert!(stderr.contains("<PIID>"));
    assert!(stderr.contains("Usage: mit props sub <DID> <SIID> <PIID>"));
}

#[test]
fn clap_errors_use_stable_binary_name_when_argv0_has_windows_extension() {
    let args = ["mit.exe", "props", "sub", "dev-1", "2"].map(String::from);
    let error = mit::cli::build_command()
        .try_get_matches_from(mit::cli::normalize_args_for_clap(&args))
        .unwrap_err();
    let stderr = strip_ansi_sequences(&error.to_string());

    assert!(stderr.contains("<PIID>"));
    assert!(stderr.contains("Usage: mit props sub <DID> <SIID> <PIID>"));
    assert!(!stderr.contains("Usage: mit.exe"));
}

#[test]
fn push_help_lists_argument_and_option_descriptions() {
    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["push", "--help"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(stdout.contains("通知文本"));
    assert!(stdout.contains("目标账号 UID；可重复发送到多个账号"));
}

#[test]
fn tui_help_lists_tui_specific_options() {
    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["tui", "--help"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(stdout.contains("启动全屏 TUI 控制台"));
    assert!(stdout.contains("--uid"));
    assert!(stdout.contains("指定进入 TUI 后默认使用的账号 UID"));
}

fn normalize_help_output(text: &str) -> String {
    strip_ansi_sequences(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn strip_ansi_sequences(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && matches!(chars.peek(), Some('[')) {
            chars.next();
            for next in chars.by_ref() {
                if ('@'..='~').contains(&next) {
                    break;
                }
            }
            continue;
        }

        out.push(ch);
    }

    out
}

#[test]
fn auth_status_is_rejected_as_invalid_subcommand() {
    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["auth", "status"])
        .output()
        .unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert!(!output.status.success());
    assert!(stderr.contains("status"));
    assert!(stderr.contains("Usage:"));
}

#[test]
fn bare_commands_note_help_compatibility_in_readme() {
    let readme = std::fs::read_to_string("README.md").unwrap();

    assert!(readme.contains("bare `auth` keeps compatibility with `mit auth --help`"));
    assert!(readme.contains("bare `devices` keeps compatibility with `mit devices --help`"));
}

#[test]
fn nested_commands_remain_case_insensitive() {
    let test_home = make_temp_dir("mit-cli-case-nested");
    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["AUTH", "LIST"])
        .env("MIT_HOME", &test_home)
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(output.status.success());
    assert!(stdout.contains("账号数："));

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
