use mit::storage;
use serde_json::json;

#[test]
fn storage_normalization_matches_previous_js_behavior() {
    let auth = storage::normalize_auth(json!({})).unwrap();
    assert!(auth.accounts.is_empty());
    assert!(auth.pending_auth.is_none());

    let migrated_auth = storage::normalize_auth(json!({
        "region": "cn",
        "redirectUri": "https://127.0.0.1:8000/login_redirect",
        "uuid": "legacy-uuid",
        "deviceId": "mico.legacy",
        "state": "legacy-state",
        "accessToken": "legacy-token",
        "refreshToken": "legacy-refresh",
        "expiresTs": 123,
        "user": {
            "uid": "1001",
            "nickname": "主账号"
        }
    }))
    .unwrap();
    assert!(migrated_auth.accounts.is_empty());
    assert!(migrated_auth.pending_auth.is_none());

    let auth_with_accounts = storage::normalize_auth(json!({
    "accounts": [
        {
            "region": "cn",
            "redirectUri": "https://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "mico.a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 111,
            "user": { "uid": "1001", "nickname": "账号A" }
        },
        {
            "region": "cn",
            "redirectUri": "https://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-b",
            "deviceId": "mico.b",
            "state": "state-b",
            "accessToken": "token-b",
            "refreshToken": "refresh-b",
            "expiresTs": 222,
            "user": { "uid": "1002", "nickname": "账号B" }
        }
    ]
    }))
    .unwrap();
    assert_eq!(
        storage::get_auth_accounts(&auth_with_accounts)
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        storage::find_auth_account_by_uid(&auth_with_accounts, "1002")
            .unwrap()
            .user
            .nickname,
        "账号B"
    );

    let pending_auth = storage::set_pending_auth(
        &auth_with_accounts,
        Some(&storage::normalize_account(json!({
            "region": "us",
            "redirectUri": "https://example.com/callback",
            "uuid": "uuid-pending",
            "deviceId": "mico.pending",
            "state": "pending-state"
        }))),
    )
    .unwrap();
    assert_eq!(
        storage::get_pending_auth(&pending_auth)
            .unwrap()
            .unwrap()
            .device_id,
        "mico.pending"
    );
    assert!(storage::clear_pending_auth(&pending_auth)
        .unwrap()
        .pending_auth
        .is_none());

    assert!(storage::get_auth_path().ends_with(".mit/auth.json"));
    assert!(storage::get_accounts_dir().ends_with(".mit/accounts"));
    assert!(storage::get_account_dir("1001").ends_with(".mit/accounts/1001"));
}

#[test]
fn user_settings_default_auto_subscribe_device_status_on() {
    let defaults = storage::UserSettings::default();
    assert!(defaults.auto_subscribe_device_status);

    let migrated: storage::UserSettings = serde_json::from_value(json!({
        "language": "english"
    }))
    .unwrap();
    assert_eq!(migrated.language, storage::Language::English);
    assert!(migrated.auto_subscribe_device_status);

    let opted_out: storage::UserSettings = serde_json::from_value(json!({
        "language": "chinese",
        "autoSubscribeDeviceStatus": false
    }))
    .unwrap();
    assert_eq!(opted_out.language, storage::Language::Chinese);
    assert!(!opted_out.auto_subscribe_device_status);
}
