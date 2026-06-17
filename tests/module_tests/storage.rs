use super::*;
use serde_json::json;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn auth_json_persists_only_accounts_and_pending_auth_at_root() {
    let auth = normalize_auth(json!({
        "accounts": [
            {
                "xiaomi": {
                    "region": "cn",
                    "redirectUri": "http://127.0.0.1:8000/login_redirect",
                    "uuid": "uuid-a",
                    "deviceId": "mico.a",
                    "state": "state-a",
                    "accessToken": "token-a",
                    "refreshToken": "refresh-a",
                    "expiresTs": 111
                },
                "mijia": null,
                "user": { "uid": "1001", "nickname": "账号A" }
            }
        ],
        "pendingAuth": {
            "xiaomi": {
                "region": "us",
                "redirectUri": "http://127.0.0.1:8000/login_redirect",
                "uuid": "uuid-p",
                "deviceId": "mico.pending",
                "state": "state-p"
            },
            "mijia": null
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
    let account = root["accounts"].as_array().unwrap()[0].as_object().unwrap();
    assert!(account.contains_key("xiaomi"));
    assert_eq!(account["mijia"], Value::Null);
    assert!(!account.contains_key("region"));
    assert!(!account.contains_key("accessToken"));

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
                "xiaomi": {
                    "region": "cn",
                    "redirectUri": "https://127.0.0.1:8000/login_redirect",
                    "uuid": "uuid-a",
                    "accessToken": "token-a"
                },
                "mijia": null,
                "user": { "uid": "1001", "nickname": "账号A" }
            }
        ]
    }))
    .unwrap();
    assert_eq!(auth_with_accounts.accounts.len(), 1);
    assert_eq!(auth_with_accounts.accounts[0].user.uid, "1001");

    // A version-1 (flat) account is migrated to version 2: the Xiaomi fields are
    // wrapped into `xiaomi`, the version is bumped, and `mijia` becomes null.
    let legacy_flat_accounts = normalize_auth(json!({
        "accounts": [
            {
                "version": 1,
                "region": "cn",
                "redirectUri": "https://127.0.0.1:8000/login_redirect",
                "uuid": "uuid-a",
                "accessToken": "token-a",
                "user": { "uid": "1001", "nickname": "账号A" }
            }
        ]
    }))
    .unwrap();
    assert_eq!(legacy_flat_accounts.accounts.len(), 1);
    let migrated = &legacy_flat_accounts.accounts[0];
    assert_eq!(migrated.version, CURRENT_AUTH_VERSION);
    assert_eq!(migrated.user.uid, "1001");
    let xiaomi = migrated.xiaomi.as_ref().expect("xiaomi auth migrated");
    assert_eq!(xiaomi.access_token, "token-a");
    assert_eq!(xiaomi.uuid, "uuid-a");
    assert!(migrated.mijia.is_none());

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
fn auth_v1_flat_account_migrates_to_v2_nested_on_write() {
    let auth = normalize_auth(json!({
        "accounts": [
            {
                "version": 1,
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
        ]
    }))
    .unwrap();
    let path = temp_test_path("auth-v1-migrate");

    write_auth(&path, &auth).unwrap();

    let persisted: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let account = persisted["accounts"].as_array().unwrap()[0]
        .as_object()
        .unwrap();
    // Version bumped, Xiaomi fields nested, Mijia null, flat fields gone.
    assert_eq!(account["version"], json!(CURRENT_AUTH_VERSION));
    assert_eq!(account["mijia"], Value::Null);
    assert_eq!(account["xiaomi"]["accessToken"], json!("token-a"));
    assert_eq!(account["xiaomi"]["deviceId"], json!("mico.a"));
    assert_eq!(account["xiaomi"]["expiresTs"], json!(111));
    assert!(!account.contains_key("accessToken"));
    assert!(!account.contains_key("deviceId"));

    let _ = fs::remove_file(&path);
    let _ = fs::remove_dir(path.parent().unwrap());
}

#[test]
fn auth_account_preserves_mijia_credentials_and_merges_by_uid() {
    let auth = normalize_auth(json!({
        "accounts": [
            {
                "xiaomi": {
                    "region": "cn",
                    "redirectUri": "https://127.0.0.1:8000/login_redirect",
                    "uuid": "uuid-a",
                    "accessToken": "token-a",
                    "refreshToken": "refresh-a",
                    "expiresTs": 111
                },
                "user": { "uid": "1001", "nickname": "账号A" },
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
            },
            {
                "xiaomi": {
                    "region": "cn",
                    "redirectUri": "https://127.0.0.1:8000/login_redirect",
                    "uuid": "uuid-b",
                    "accessToken": "token-b",
                    "refreshToken": "refresh-b",
                    "expiresTs": 333
                },
                "mijia": null,
                "user": { "uid": "1001", "nickname": "" }
            }
        ]
    }))
    .unwrap();

    assert_eq!(auth.accounts.len(), 1);
    let account = &auth.accounts[0];
    assert_eq!(account.access_token, "token-b");
    assert_eq!(account.user.nickname, "账号A");
    let mijia = account.mijia.as_ref().expect("mijia auth should be kept");
    assert_eq!(mijia.service_token, "service-token-a");
    assert_eq!(mijia.ssecurity, "AQIDBAUGBwgJCgsMDQ4PEA==");
}

#[test]
fn auth_account_can_be_mijia_only_and_uses_mijia_identity_for_dedupe() {
    let auth = normalize_auth(json!({
        "accounts": [
            {
                "xiaomi": null,
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
            },
            {
                "xiaomi": null,
                "mijia": {
                    "ua": "Android-15-test-2",
                    "deviceId": "mijia-device-b",
                    "passO": "pass-o-b",
                    "ssecurity": "AQIDBAUGBwgJCgsMDQ4PEA==",
                    "passToken": "pass-token-b",
                    "userId": "1001",
                    "cUserId": "c-1001",
                    "serviceToken": "service-token-b",
                    "expireTime": 333,
                    "saveTime": 234
                }
            }
        ]
    }))
    .unwrap();

    assert_eq!(auth.accounts.len(), 1);
    let account = &auth.accounts[0];
    assert_eq!(account.user.uid, "1001");
    let mijia = account.mijia.as_ref().expect("mijia auth should be kept");
    assert_eq!(mijia.service_token, "service-token-b");
    assert_eq!(mijia.device_id, "mijia-device-b");
}

#[test]
fn auth_account_merges_xiaomi_and_mijia_record_by_numeric_uid() {
    let auth = normalize_auth(json!({
        "accounts": [
            {
                "xiaomi": {
                    "region": "cn",
                    "redirectUri": "https://127.0.0.1:8000/login_redirect",
                    "uuid": "uuid-xiaomi",
                    "deviceId": "mico.uuid-xiaomi",
                    "state": "state-xiaomi",
                    "accessToken": "token-xiaomi",
                    "refreshToken": "refresh-xiaomi",
                    "expiresTs": 111
                },
                "mijia": null,
                "user": {
                    "uid": "3009043526",
                    "nickname": "Troy",
                    "icon": "icon-a",
                    "unionId": "union-a"
                }
            },
            {
                "xiaomi": null,
                "mijia": {
                    "ua": "Android-15-test",
                    "deviceId": "mijia-device-a",
                    "passO": "pass-o-a",
                    "ssecurity": "AQIDBAUGBwgJCgsMDQ4PEA==",
                    "passToken": "pass-token-a",
                    "userId": "3009043526",
                    "cUserId": "0rbmwYARaRdlqT4RyQjiZpTm7ZQ",
                    "serviceToken": "service-token-a",
                    "expireTime": 222,
                    "saveTime": 123
                },
                "user": {
                    "uid": "3009043526",
                    "nickname": "",
                    "icon": "",
                    "unionId": ""
                }
            }
        ]
    }))
    .unwrap();

    assert_eq!(auth.accounts.len(), 1);
    let account = &auth.accounts[0];
    assert!(account.xiaomi.is_some());
    assert!(account.mijia.is_some());
    assert_eq!(account.user.uid, "3009043526");
    assert_eq!(account.user.nickname, "Troy");
    assert_eq!(account.user.union_id, "union-a");
}

#[test]
fn auth_account_keeps_xiaomi_and_mijia_records_separate_when_numeric_uid_differs() {
    let auth = normalize_auth(json!({
        "accounts": [
            {
                "xiaomi": {
                    "region": "cn",
                    "redirectUri": "https://127.0.0.1:8000/login_redirect",
                    "uuid": "uuid-xiaomi",
                    "deviceId": "mico.uuid-xiaomi",
                    "state": "state-xiaomi",
                    "accessToken": "token-xiaomi",
                    "refreshToken": "refresh-xiaomi",
                    "expiresTs": 111
                },
                "mijia": null,
                "user": {
                    "uid": "3009043526",
                    "nickname": "Troy",
                    "icon": "icon-a",
                    "unionId": "union-a"
                }
            },
            {
                "xiaomi": null,
                "mijia": {
                    "ua": "Android-15-test",
                    "deviceId": "mijia-device-a",
                    "passO": "pass-o-a",
                    "ssecurity": "AQIDBAUGBwgJCgsMDQ4PEA==",
                    "passToken": "pass-token-a",
                    "userId": "2002",
                    "cUserId": "c-2002",
                    "serviceToken": "service-token-a",
                    "expireTime": 222,
                    "saveTime": 123
                },
                "user": {
                    "uid": "2002",
                    "nickname": "",
                    "icon": "",
                    "unionId": ""
                }
            }
        ]
    }))
    .unwrap();

    assert_eq!(auth.accounts.len(), 2);
    assert_eq!(auth.accounts[0].user.uid, "3009043526");
    assert_eq!(auth.accounts[1].user.uid, "2002");
}

#[test]
fn auth_account_merges_old_cuserid_mijia_record_after_numeric_uid_is_resolved() {
    let auth = normalize_auth(json!({
        "accounts": [
            {
                "xiaomi": {
                    "region": "cn",
                    "redirectUri": "https://127.0.0.1:8000/login_redirect",
                    "uuid": "uuid-xiaomi",
                    "deviceId": "mico.uuid-xiaomi",
                    "state": "state-xiaomi",
                    "accessToken": "token-xiaomi",
                    "refreshToken": "refresh-xiaomi",
                    "expiresTs": 111
                },
                "mijia": null,
                "user": {
                    "uid": "3009043526",
                    "nickname": "Troy",
                    "icon": "icon-a",
                    "unionId": "union-a"
                }
            },
            {
                "xiaomi": null,
                "mijia": {
                    "ua": "Android-15-test-old",
                    "deviceId": "mijia-device-old",
                    "passO": "pass-o-old",
                    "ssecurity": "AQIDBAUGBwgJCgsMDQ4PEA==",
                    "passToken": "pass-token-old",
                    "userId": "",
                    "cUserId": "0rbmwYARaRdlqT4RyQjiZpTm7ZQ",
                    "serviceToken": "service-token-old",
                    "expireTime": 111,
                    "saveTime": 100
                },
                "user": {
                    "uid": "0rbmwYARaRdlqT4RyQjiZpTm7ZQ",
                    "nickname": "",
                    "icon": "",
                    "unionId": ""
                }
            },
            {
                "xiaomi": null,
                "mijia": {
                    "ua": "Android-15-test-new",
                    "deviceId": "mijia-device-new",
                    "passO": "pass-o-new",
                    "ssecurity": "AQIDBAUGBwgJCgsMDQ4PEA==",
                    "passToken": "pass-token-new",
                    "userId": "3009043526",
                    "cUserId": "0rbmwYARaRdlqT4RyQjiZpTm7ZQ",
                    "serviceToken": "service-token-new",
                    "expireTime": 222,
                    "saveTime": 123
                },
                "user": {
                    "uid": "3009043526",
                    "nickname": "",
                    "icon": "",
                    "unionId": ""
                }
            }
        ]
    }))
    .unwrap();

    assert_eq!(auth.accounts.len(), 1);
    let account = &auth.accounts[0];
    assert!(account.xiaomi.is_some());
    assert!(account.mijia.is_some());
    assert_eq!(account.user.uid, "3009043526");
    assert_eq!(account.user.nickname, "Troy");
    let mijia = account.mijia.as_ref().unwrap();
    assert_eq!(mijia.user_id, "3009043526");
    assert_eq!(mijia.service_token, "service-token-new");
}

#[test]
fn auth_account_does_not_merge_mijia_only_record_when_xiaomi_target_is_ambiguous() {
    let auth = normalize_auth(json!({
        "accounts": [
            {
                "xiaomi": {
                    "uuid": "uuid-a",
                    "accessToken": "token-a"
                },
                "mijia": null,
                "user": { "uid": "1001", "nickname": "账号A" }
            },
            {
                "xiaomi": {
                    "uuid": "uuid-b",
                    "accessToken": "token-b"
                },
                "mijia": null,
                "user": { "uid": "1002", "nickname": "账号B" }
            },
            {
                "xiaomi": null,
                "mijia": {
                    "ssecurity": "AQIDBAUGBwgJCgsMDQ4PEA==",
                    "cUserId": "c-ambiguous",
                    "serviceToken": "service-token"
                },
                "user": { "uid": "c-ambiguous" }
            }
        ]
    }))
    .unwrap();

    assert_eq!(auth.accounts.len(), 3);
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
