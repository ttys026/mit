use super::*;
use serde_json::json;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn auth_json_persists_only_accounts_and_pending_auth_at_root() {
    let auth = normalize_auth(json!({
        "accounts": [
            {
                "region": "cn",
                "redirectUri": "http://127.0.0.1:8000/login_redirect",
                "uuid": "uuid-a",
                "deviceId": "mico.a",
                "state": "state-a",
                "accessToken": "token-a",
                "refreshToken": "refresh-a",
                "expiresTs": 111,
                "user": { "uid": "1001", "nickname": "账号A" }
            }
        ],
        "pendingAuth": {
            "region": "us",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-p",
            "deviceId": "mico.pending",
            "state": "state-p"
        }
    }))
    .unwrap();
    let path = temp_test_path("auth-root-shape");

    write_auth(&path, &auth).unwrap();

    let persisted: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let root = persisted.as_object().unwrap();
    assert_eq!(root.len(), 2);
    assert!(root.contains_key("accounts"));
    assert!(root.contains_key("pendingAuth"));
    assert!(!root.contains_key("region"));
    assert!(!root.contains_key("user"));

    let _ = fs::remove_file(&path);
    let _ = fs::remove_dir(path.parent().unwrap());
}

#[test]
fn strict_auth_ignores_legacy_root_fields_when_accounts_are_missing() {
    let auth = normalize_auth(json!({})).unwrap();
    assert!(auth.accounts.is_empty());
    assert!(auth.pending_auth.is_none());

    let legacy_only = normalize_auth(json!({
        "region": "cn",
        "redirectUri": "https://127.0.0.1:8000/login_redirect",
        "uuid": "legacy-uuid",
        "accessToken": "legacy-token",
        "user": { "uid": "1001", "nickname": "主账号" }
    }))
    .unwrap();
    assert!(legacy_only.accounts.is_empty());
    assert!(legacy_only.pending_auth.is_none());
}

#[test]
fn strict_auth_reads_accounts_and_rejects_bad_shapes() {
    let auth_with_accounts = normalize_auth(json!({
        "region": "ignored",
        "accounts": [
            {
                "region": "cn",
                "redirectUri": "https://127.0.0.1:8000/login_redirect",
                "uuid": "uuid-a",
                "accessToken": "token-a",
                "user": { "uid": "1001", "nickname": "账号A" }
            }
        ]
    }))
    .unwrap();
    assert_eq!(auth_with_accounts.accounts.len(), 1);
    assert_eq!(auth_with_accounts.accounts[0].user.uid, "1001");

    assert!(normalize_auth(json!({
        "accounts": [],
        "pendingAuth": "bad-shape"
    }))
    .unwrap_err()
    .to_string()
    .contains("pendingAuth"));

    assert!(normalize_auth(json!({
        "accounts": {}
    }))
    .unwrap_err()
    .to_string()
    .contains("auth.accounts"));
}

#[test]
fn test_build_uses_isolated_profile_dir_by_default() {
    if env::var_os("MIT_PROFILE_DIR").is_some()
        || env::var_os("MIT_HOME").is_some()
        || env::var_os("XMCLI_HOME").is_some()
    {
        return;
    }

    let home = get_home_dir();
    let expected = format!("mit-test-profile-{}", std::process::id());
    assert!(
        home.to_string_lossy().contains(&expected),
        "expected isolated test profile dir, got {}",
        home.display()
    );
}

#[test]
#[cfg(unix)]
fn write_private_text_file_creates_file_with_private_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let path = temp_test_path("private-text-file").with_file_name("private.txt");
    fs::create_dir_all(path.parent().unwrap()).unwrap();

    write_private_text_file(&path, "secret\n").unwrap();

    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);

    let _ = fs::remove_file(&path);
    let _ = fs::remove_dir(path.parent().unwrap());
}

#[test]
#[cfg(unix)]
fn write_private_text_file_does_not_truncate_when_temp_create_fails() {
    use std::os::unix::fs::PermissionsExt;

    let path = temp_test_path("private-text-file-atomic").with_file_name("private.txt");
    let dir = path.parent().unwrap();
    fs::create_dir_all(dir).unwrap();

    fs::write(&path, "old\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();

    // Simulate a failure to create the same-directory temp file by making the directory read-only.
    fs::set_permissions(dir, fs::Permissions::from_mode(0o500)).unwrap();

    let result = write_private_text_file(&path, "new\n");
    assert!(result.is_err());

    let after = fs::read_to_string(&path).unwrap();
    assert_eq!(after, "old\n");

    // Restore permissions to allow cleanup.
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).unwrap();
    let _ = fs::remove_file(&path);
    let _ = fs::remove_dir(dir);
}

#[test]
#[cfg(not(unix))]
fn write_private_text_file_preserves_or_overwrites_existing_file() {
    let path =
        temp_test_path("private-text-file-existing-destination").with_file_name("private.txt");
    let dir = path.parent().unwrap();
    fs::create_dir_all(dir).unwrap();

    fs::write(&path, "old\n").unwrap();

    // On Windows, std::fs::rename can replace an existing destination file.
    // On other platforms, rename may fail if the destination exists.
    // Either way, we must not leave a temp file behind, and we must not corrupt the destination.
    let result = write_private_text_file(&path, "new\n");
    let after = fs::read_to_string(&path).unwrap();
    if result.is_ok() {
        assert_eq!(after, "new\n");
    } else {
        assert_eq!(after, "old\n");
    }

    // Ensure we didn't leave a temp file behind.
    let temp_prefix = ".private.txt.tmp.";
    let leftover_tmp = fs::read_dir(dir)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .any(|name| name.starts_with(temp_prefix));
    assert!(!leftover_tmp);

    let _ = fs::remove_file(&path);
    let _ = fs::remove_dir(dir);
}

fn temp_test_path(prefix: &str) -> std::path::PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir()
        .join("mit-storage-tests")
        .join(format!("{prefix}-{unique}-{}", std::process::id()))
        .join("auth.json")
}
