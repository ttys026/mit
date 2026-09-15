//! Integration tests for the cache/reset/logout CLI commands added for TUI↔CLI parity.
//! These operate purely on the profile directory (no network), so they run against a
//! throwaway MIT_HOME without a mock server.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_home(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("mit-cli-cmd-{tag}-{}-{nanos}", std::process::id()))
}

/// Seed a profile with an auth file plus a device cache and a spec cache dir.
fn seed_profile(home: &Path) -> PathBuf {
    let mit = home.join(".mit");
    fs::create_dir_all(mit.join("accounts").join("1001")).unwrap();
    fs::create_dir_all(mit.join("cache").join("specs")).unwrap();
    fs::write(mit.join("auth.json"), "{\"accounts\":[]}").unwrap();
    fs::write(mit.join("accounts").join("1001").join("devices.json"), "[]").unwrap();
    fs::write(mit.join("cache").join("specs").join("index.json"), "{}").unwrap();
    mit
}

#[test]
fn reset_without_yes_refuses_and_keeps_profile() {
    let home = unique_home("reset-noyes");
    let mit = seed_profile(&home);

    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["reset"])
        .env("MIT_HOME", &home)
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("--yes"), "stderr was: {stderr}");
    assert!(
        mit.exists(),
        "reset without --yes must not delete the profile"
    );

    fs::remove_dir_all(&home).ok();
}

#[test]
fn reset_with_yes_deletes_profile() {
    let home = unique_home("reset-yes");
    let mit = seed_profile(&home);

    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["reset", "--yes"])
        .env("MIT_HOME", &home)
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(!mit.exists(), "reset --yes must delete ~/.mit");

    fs::remove_dir_all(&home).ok();
}

#[test]
fn cache_clean_removes_caches_but_keeps_auth() {
    let home = unique_home("cache-clean");
    let mit = seed_profile(&home);

    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["cache", "clean"])
        .env("MIT_HOME", &home)
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(mit.join("auth.json").exists(), "auth.json must be kept");
    assert!(!mit.join("cache").exists(), "cache dir must be removed");
    assert!(
        !mit.join("accounts").exists(),
        "account caches must be removed"
    );

    fs::remove_dir_all(&home).ok();
}

#[test]
fn cache_clean_json_reports_removed_count() {
    let home = unique_home("cache-json");
    seed_profile(&home);

    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["--json", "cache", "clean"])
        .env("MIT_HOME", &home)
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("\"type\":\"cacheClean\""),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("\"removed\""), "stdout: {stdout}");

    fs::remove_dir_all(&home).ok();
}

#[test]
fn auth_logout_without_accounts_errors() {
    let home = unique_home("logout-empty");
    let mit = home.join(".mit");
    fs::create_dir_all(&mit).unwrap();
    fs::write(mit.join("auth.json"), "{\"accounts\":[]}").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(["auth", "logout"])
        .env("MIT_HOME", &home)
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("没有可登出的账号"), "stderr: {stderr}");

    fs::remove_dir_all(&home).ok();
}
