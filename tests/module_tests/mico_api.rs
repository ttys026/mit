use super::*;
use crate::storage::normalize_account;
use crate::test_support;
use serde_json::json;
use std::env;
use std::fs;
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::os::unix::io::{FromRawFd, RawFd};
use std::time::{SystemTime, UNIX_EPOCH};

mod mock_mico_server {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/mock_mico_server.rs"
    ));
}

use mock_mico_server::MockMicoServer;

#[test]
fn api_authorization_header_matches_xiaomi_client_format() {
    let client = MicoClient::new(&normalize_account(json!({
        "region": "cn",
        "redirectUri": "https://127.0.0.1:8000/login_redirect",
        "uuid": "abcd1234abcd1234abcd1234abcd1234",
        "accessToken": "token-123"
    })))
    .unwrap();

    let headers = client.request_headers();
    let authorization = headers.get("Authorization").unwrap().to_str().unwrap();
    assert_eq!(authorization, "Bearertoken-123");
}

#[test]
fn collect_home_dids_includes_room_dids() {
    let homes = json!({
        "homelist": [
            {
                "dids": [],
                "roomlist": [
                    { "dids": ["dev-1"] },
                    { "dids": ["dev-2"] }
                ]
            }
        ],
        "share_home_list": [
            {
                "dids": ["dev-3"],
                "roomlist": []
            }
        ]
    });
    let mut dids = collect_home_dids(&homes);
    dids.sort();
    assert_eq!(dids, vec!["dev-1", "dev-2", "dev-3"]);
}

#[test]
fn collect_home_placements_keeps_home_level_devices_out_of_rooms() {
    let homes = json!({
        "homelist": [
            {
                "id": "home-1",
                "dids": ["gw-1", "dev-1"],
                "roomlist": [
                    { "id": "room-1", "dids": ["dev-2"] }
                ]
            }
        ],
        "share_home_list": []
    });

    let placements = collect_home_placements(&homes);

    assert_eq!(placements["gw-1"].home_id, "home-1");
    assert_eq!(placements["gw-1"].room_id, "");

    assert_eq!(placements["dev-1"].home_id, "home-1");
    assert_eq!(placements["dev-1"].room_id, "");

    assert_eq!(placements["dev-2"].home_id, "home-1");
    assert_eq!(placements["dev-2"].room_id, "room-1");
}

#[test]
fn local_credential_cache_is_shared_between_client_instances() {
    let _guard = env_guard();
    let server = MockMicoServer::start();
    let device_id = "mico.cache-share-test";
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "cache-share-test",
        "deviceId": device_id,
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 12345,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));

    std::env::set_var("MIT_MICO_BASE_URL", server.base_url());
    std::env::set_var("MIT_USER_PROFILE_URL", server.user_profile_url());

    let client = MicoClient::new(&account).unwrap();
    client.get_local_device_credentials().unwrap();
    {
        // After a cloud sync the resolved credential is cached as ready (no probe);
        // the same-intranet check at control time decides LAN vs cloud.
        let cache = shared_local_credential_cache(device_id);
        let cache = cache.lock().unwrap();
        assert!(cache.ready_dids.contains("dev-1"));
        assert!(!cache.disabled_dids.contains("dev-1"));
    }

    // A second client for the same account shares the very same process-global
    // cache, so constructing it does not clear the resolved credential.
    let _second_client = MicoClient::new(&account).unwrap();
    assert!(has_cached_local_credential_for_device(device_id, "dev-1"));

    std::env::remove_var("MIT_MICO_BASE_URL");
    std::env::remove_var("MIT_USER_PROFILE_URL");
}

#[test]
fn write_local_credentials_snapshot_rejects_empty_account_uid() {
    let _guard = env_guard();
    let home = make_temp_home("local-credential-empty-uid");
    env::set_var("MIT_HOME", &home);

    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "local-credential-empty-uid",
        "accessToken": "token-a",
        "user": {"uid": "", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));

    let client = MicoClient::new(&account).unwrap();
    let error = client
        .write_local_credentials_snapshot(&HashMap::new())
        .unwrap_err()
        .to_string();

    assert!(
        error.to_lowercase().contains("uid") || error.to_lowercase().contains("account"),
        "unexpected error: {error}"
    );

    env::remove_var("MIT_HOME");
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn write_local_credentials_snapshot_rejects_unsafe_account_uid() {
    let _guard = env_guard();
    let home = make_temp_home("local-credential-unsafe-uid");
    env::set_var("MIT_HOME", &home);

    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "local-credential-unsafe-uid",
        "accessToken": "token-a",
        "user": {"uid": "../escape", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));

    let client = MicoClient::new(&account).unwrap();
    let error = client
        .write_local_credentials_snapshot(&HashMap::new())
        .unwrap_err()
        .to_string();

    assert!(
        error.to_lowercase().contains("uid") || error.to_lowercase().contains("account"),
        "unexpected error: {error}"
    );

    env::remove_var("MIT_HOME");
    let _ = fs::remove_dir_all(&home);
}

#[test]
#[cfg(not(unix))]
fn write_local_credentials_snapshot_writes_file_on_non_unix() {
    let _guard = env_guard();
    let home = make_temp_home("local-credential-non-unix-perms");
    env::set_var("MIT_HOME", &home);
    env::set_var("MIT_PROFILE_DIR", &home);

    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "local-credential-non-unix-perms",
        "accessToken": "token-a",
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));

    let client = MicoClient::new(&account).unwrap();
    client
        .write_local_credentials_snapshot(&HashMap::new())
        .unwrap();

    let snapshot_path = home
        .join(".mit")
        .join("accounts")
        .join("1001")
        .join("local_credentials.json");
    let text = fs::read_to_string(&snapshot_path).unwrap();
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["version"], 1);
    assert!(value["credentials"].as_object().unwrap().is_empty());
    assert!(value["devices"].as_object().unwrap().is_empty());

    env::remove_var("MIT_HOME");
    env::remove_var("MIT_PROFILE_DIR");
    let _ = fs::remove_dir_all(&home);
}

#[test]
#[cfg(not(unix))]
fn get_local_device_credentials_does_not_fail_when_snapshot_permissions_are_unsupported() {
    let _guard = env_guard();
    let server = MockMicoServer::start();
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "local-credential-non-unix-fetch",
        "deviceId": "mico.local-credential-non-unix-fetch",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 12345,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));

    std::env::set_var("MIT_MICO_BASE_URL", server.base_url());
    std::env::set_var("MIT_USER_PROFILE_URL", server.user_profile_url());

    let client = MicoClient::new(&account).unwrap();
    client.get_local_device_credentials().unwrap();

    std::env::remove_var("MIT_MICO_BASE_URL");
    std::env::remove_var("MIT_USER_PROFILE_URL");
}

#[test]
#[cfg(unix)]
fn get_local_device_credentials_writes_local_credentials_snapshot_for_account() {
    let _guard = env_guard();
    let server = MockMicoServer::start();
    let home = make_temp_home("local-credential-json");
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "local-credential-json",
        "deviceId": "mico.local-credential-json",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 12345,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));

    std::env::set_var("MIT_HOME", &home);
    std::env::set_var("MIT_PROFILE_DIR", &home);
    std::env::set_var("MIT_MICO_BASE_URL", server.base_url());
    std::env::set_var("MIT_USER_PROFILE_URL", server.user_profile_url());
    let profile_home = crate::storage::get_home_dir();

    let client = MicoClient::new(&account).unwrap();
    client.get_local_device_credentials().unwrap();

    let path = profile_home
        .join(".mit")
        .join("accounts")
        .join("1001")
        .join("local_credentials.json");
    let text = fs::read_to_string(&path).unwrap();
    let value: Value = serde_json::from_str(&text).unwrap();

    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        mode, 0o600,
        "snapshot should be written with private permissions"
    );

    assert_eq!(value["version"], 1);
    assert_eq!(value["devices"].as_object().unwrap().len(), 1);
    assert_eq!(value["credentials"].as_object().unwrap().len(), 1);
    let credential_id = value["devices"]["dev-1"].as_str().unwrap();
    assert_eq!(
        value["credentials"][credential_id]["localIp"],
        "192.168.0.20"
    );
    assert_eq!(
        value["credentials"][credential_id]["token"],
        "00112233445566778899aabbccddeeff"
    );

    std::env::remove_var("MIT_HOME");
    std::env::remove_var("MIT_PROFILE_DIR");
    std::env::remove_var("MIT_MICO_BASE_URL");
    std::env::remove_var("MIT_USER_PROFILE_URL");
    let _ = fs::remove_dir_all(&home);
}

#[test]
#[cfg(unix)]
fn get_local_device_credentials_returns_error_when_snapshot_write_fails() {
    let _guard = env_guard();
    let server = MockMicoServer::start();
    let home = make_temp_home("local-credential-write-error");
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "local-credential-write-error",
        "deviceId": "mico.local-credential-write-error",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 12345,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));

    std::env::set_var("MIT_HOME", &home);
    std::env::set_var("MIT_PROFILE_DIR", &home);
    std::env::set_var("MIT_MICO_BASE_URL", server.base_url());
    std::env::set_var("MIT_USER_PROFILE_URL", server.user_profile_url());
    let profile_home = crate::storage::get_home_dir();
    let account_dir = profile_home.join(".mit").join("accounts").join("1001");
    fs::create_dir_all(&account_dir).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&account_dir, fs::Permissions::from_mode(0o500)).unwrap();
    }

    let client = MicoClient::new(&account).unwrap();
    let error = client
        .get_local_device_credentials()
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("local_credentials.json"),
        "write failures should identify the snapshot path"
    );

    std::env::remove_var("MIT_HOME");
    std::env::remove_var("MIT_PROFILE_DIR");
    std::env::remove_var("MIT_MICO_BASE_URL");
    std::env::remove_var("MIT_USER_PROFILE_URL");
    let _ = fs::remove_dir_all(&home);
}

#[test]
#[cfg(unix)]
fn get_local_device_credentials_keeps_resolved_creds_when_snapshot_write_fails() {
    let _guard = env_guard();
    let server = MockMicoServer::start();
    let home = make_temp_home("local-credential-write-error-side-effects");
    let device_id = "mico.local-credential-write-error-side-effects";
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "local-credential-write-error-side-effects",
        "deviceId": device_id,
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 12345,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));

    std::env::set_var("MIT_HOME", &home);
    std::env::set_var("MIT_PROFILE_DIR", &home);
    std::env::set_var("MIT_MICO_BASE_URL", server.base_url());
    std::env::set_var("MIT_USER_PROFILE_URL", server.user_profile_url());
    let profile_home = crate::storage::get_home_dir();
    let account_dir = profile_home.join(".mit").join("accounts").join("1001");
    fs::create_dir_all(&account_dir).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&account_dir, fs::Permissions::from_mode(0o500)).unwrap();
    }

    let client = MicoClient::new(&account).unwrap();
    assert!(
        client.get_local_device_credentials().is_err(),
        "snapshot write failure should still surface as an error"
    );

    let cache = shared_local_credential_cache(device_id);
    let cache = cache.lock().unwrap();
    assert!(
        cache.ready_dids.contains("dev-1"),
        "the in-memory cache should still hold the resolved credential even if snapshot persistence fails"
    );
    assert!(cache.by_did.contains_key("dev-1"));
    assert!(!cache.disabled_dids.contains("dev-1"));

    std::env::remove_var("MIT_HOME");
    std::env::remove_var("MIT_PROFILE_DIR");
    std::env::remove_var("MIT_MICO_BASE_URL");
    std::env::remove_var("MIT_USER_PROFILE_URL");
    let _ = fs::remove_dir_all(&home);
}

#[test]
#[cfg(unix)]
fn local_credential_snapshot_includes_routed_entries_with_source_labels() {
    let _guard = env_guard();
    let server = MockMicoServer::start_with_routed_local_credentials();
    let home = make_temp_home("local-credential-routed-json");
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "local-credential-routed-json",
        "deviceId": "mico.local-credential-routed-json",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 12345,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));

    std::env::set_var("MIT_HOME", &home);
    std::env::set_var("MIT_PROFILE_DIR", &home);
    std::env::set_var("MIT_MICO_BASE_URL", server.base_url());
    std::env::set_var("MIT_USER_PROFILE_URL", server.user_profile_url());
    let profile_home = crate::storage::get_home_dir();

    let client = MicoClient::new(&account).unwrap();
    let direct = client.get_local_device_credentials().unwrap();
    assert_eq!(direct.len(), 1, "only the gateway has direct credentials");

    let path = profile_home
        .join(".mit")
        .join("accounts")
        .join("1001")
        .join("local_credentials.json");
    let value: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();

    assert_eq!(value["credentials"].as_object().unwrap().len(), 1);
    assert_eq!(value["devices"].as_object().unwrap().len(), 2);

    let gw_cred = value["devices"]["gw-1"].as_str().unwrap();
    let dev_cred = value["devices"]["dev-2"].as_str().unwrap();
    assert_eq!(dev_cred, gw_cred);
    assert_eq!(
        value["credentials"][dev_cred]["localIp"],
        value["credentials"][gw_cred]["localIp"]
    );
    assert_eq!(
        value["credentials"][dev_cred]["token"], value["credentials"][gw_cred]["token"],
        "routed devices should persist the credential they inherited"
    );

    std::env::remove_var("MIT_HOME");
    std::env::remove_var("MIT_PROFILE_DIR");
    std::env::remove_var("MIT_MICO_BASE_URL");
    std::env::remove_var("MIT_USER_PROFILE_URL");
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn get_props_batch_does_not_fetch_local_credentials_on_cache_miss() {
    let _guard = env_guard();
    let server = MockMicoServer::start();
    let device_id = "mico.no-fetch-test";
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "no-fetch-test",
        "deviceId": device_id,
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 12345,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));

    std::env::set_var("MIT_MICO_BASE_URL", server.base_url());
    std::env::set_var("MIT_USER_PROFILE_URL", server.user_profile_url());

    let client = MicoClient::new(&account).unwrap();
    let _ = client.get_props_batch(&[("dev-1", 2, 1)]).unwrap();
    let requests = server.requests();
    assert!(requests
        .iter()
        .all(|request| request.path != "/app/v2/home/device_list_page"));

    std::env::remove_var("MIT_MICO_BASE_URL");
    std::env::remove_var("MIT_USER_PROFILE_URL");
}

#[test]
fn get_props_batch_retries_sub_device_dids_with_root_did() {
    let _guard = env_guard();
    let server = MockMicoServer::start_with_sub_device_dids_require_root();
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "sub-device-root-did-fallback",
        "deviceId": "mico.sub-device-root-did-fallback",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 12345,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));

    std::env::set_var("MIT_MICO_BASE_URL", server.base_url());
    std::env::set_var("MIT_USER_PROFILE_URL", server.user_profile_url());

    let client = MicoClient::new(&account).unwrap();
    let values = client
        .get_props_batch(&[("2045081210.s2", 2, 1), ("2045081210.s3", 2, 1)])
        .unwrap();
    let items = values.as_array().unwrap();

    assert_eq!(items.len(), 2);
    assert_eq!(items[0].get("value").and_then(Value::as_bool), Some(true));
    assert_eq!(items[1].get("value").and_then(Value::as_bool), Some(true));

    std::env::remove_var("MIT_MICO_BASE_URL");
    std::env::remove_var("MIT_USER_PROFILE_URL");
}

#[test]
fn get_props_batch_strips_sub_device_suffix_in_cloud_prop_get() {
    let _guard = env_guard();
    let server = MockMicoServer::start();
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "sub-device-strip-suffix",
        "deviceId": "mico.sub-device-strip-suffix",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 12345,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));

    std::env::set_var("MIT_MICO_BASE_URL", server.base_url());
    std::env::set_var("MIT_USER_PROFILE_URL", server.user_profile_url());

    let client = MicoClient::new(&account).unwrap();
    let _ = client.get_props_batch(&[("2045081210.s2", 2, 1)]).unwrap();
    let requests = server.requests();
    let prop_get_request = requests
        .iter()
        .rev()
        .find(|request| request.path == "/app/v2/miotspec/prop/get")
        .expect("prop/get request should be sent");
    let body: Value = serde_json::from_str(&prop_get_request.body).unwrap();
    let did = body["params"][0]["did"].as_str().unwrap_or_default();
    assert_eq!(did, "2045081210");

    std::env::remove_var("MIT_MICO_BASE_URL");
    std::env::remove_var("MIT_USER_PROFILE_URL");
}

#[test]
fn set_prop_strips_sub_device_suffix_before_requesting() {
    let _guard = env_guard();
    let server = MockMicoServer::start();
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "sub-device-write-test",
        "deviceId": "mico.sub-device-write-test",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 12345,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));

    std::env::set_var("MIT_MICO_BASE_URL", server.base_url());
    std::env::set_var("MIT_USER_PROFILE_URL", server.user_profile_url());

    let client = MicoClient::new(&account).unwrap();
    client.set_prop("2045081210.s2", 2, 1, json!(true)).unwrap();

    let requests = server.requests();
    let set_request = requests
        .iter()
        .rev()
        .find(|request| request.path == "/app/v2/miotspec/prop/set")
        .expect("prop/set request should be sent");
    let body: Value = serde_json::from_str(&set_request.body).unwrap();
    let did = body["params"][0]["did"].as_str().unwrap_or_default();
    assert_eq!(did, "2045081210");

    std::env::remove_var("MIT_MICO_BASE_URL");
    std::env::remove_var("MIT_USER_PROFILE_URL");
}

#[test]
fn action_strips_sub_device_suffix_before_requesting() {
    let _guard = env_guard();
    let server = MockMicoServer::start();
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "sub-device-action-test",
        "deviceId": "mico.sub-device-action-test",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 12345,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));

    std::env::set_var("MIT_MICO_BASE_URL", server.base_url());
    std::env::set_var("MIT_USER_PROFILE_URL", server.user_profile_url());

    let client = MicoClient::new(&account).unwrap();
    client.action("2045081210.s3", 5, 1, &[json!(1)]).unwrap();

    let requests = server.requests();
    let action_request = requests
        .iter()
        .rev()
        .find(|request| request.path == "/app/v2/miotspec/action")
        .expect("action request should be sent");
    let body: Value = serde_json::from_str(&action_request.body).unwrap();
    let did = body["params"]["did"].as_str().unwrap_or_default();
    assert_eq!(did, "2045081210");

    std::env::remove_var("MIT_MICO_BASE_URL");
    std::env::remove_var("MIT_USER_PROFILE_URL");
}

#[test]
fn verified_local_device_stays_local_after_credential_cache_ttl_expires() {
    let device_id = "mico.session-ready-test";
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "session-ready-test",
        "deviceId": device_id,
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 12345,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));
    let client = MicoClient::new(&account).unwrap();
    {
        let cache = shared_local_credential_cache(device_id);
        let mut cache = cache.lock().unwrap();
        cache.fetched_at = unix_timestamp() - Duration::from_secs(300).as_secs() as i64 - 1;
        cache.by_did.insert(
            "dev-1".to_string(),
            LocalDeviceCredential {
                did: "dev-1".to_string(),
                name: "living-room".to_string(),
                model: "xiaomi.wifispeaker.lx04".to_string(),
                local_ip: "127.0.0.1".to_string(),
                token: "00112233445566778899aabbccddeeff".to_string(),
                source: LocalCredentialSource::Direct,
            },
        );
        cache.ready_dids.insert("dev-1".to_string());
    }

    assert!(
        has_cached_local_credential_for_device(device_id, "dev-1"),
        "TUI status should stay local for a device that was already cached in this session"
    );
    let _ = client;
}

#[test]
#[cfg(unix)]
fn miot_flow_logging_stays_off_stderr() {
    let _guard = env_guard();
    let home = make_temp_home("miot-flow-silent");
    env::set_var("MIT_HOME", &home);
    env::set_var("MIT_PROFILE_DIR", &home);

    let stderr = capture_stderr(|| {
        log_route("get", "dev-1", "auto", "LAN", "eligible", "ok", None);
    });

    let profile_home = crate::storage::get_home_dir();
    let log_path = profile_home.join(".mit").join("miot-flow.log");
    let log = fs::read_to_string(&log_path).unwrap();

    assert!(stderr.trim().is_empty(), "unexpected stderr: {stderr}");
    assert!(log.contains("op=get transport=LAN"));
    assert!(log.contains("did=dev-1 mode=auto reason=eligible result=ok"));

    env::remove_var("MIT_HOME");
    env::remove_var("MIT_PROFILE_DIR");
    let _ = fs::remove_dir_all(&home);
}

fn env_guard() -> std::sync::MutexGuard<'static, ()> {
    test_support::env_guard()
}

#[cfg(unix)]
fn capture_stderr<F: FnOnce()>(f: F) -> String {
    unsafe {
        let mut pipes = [0 as RawFd; 2];
        assert_eq!(libc::pipe(pipes.as_mut_ptr()), 0);

        let stderr_fd = libc::dup(libc::STDERR_FILENO);
        assert!(stderr_fd >= 0);
        assert_eq!(
            libc::dup2(pipes[1], libc::STDERR_FILENO),
            libc::STDERR_FILENO
        );
        libc::close(pipes[1]);

        f();
        let _ = std::io::stderr().flush();

        assert_eq!(
            libc::dup2(stderr_fd, libc::STDERR_FILENO),
            libc::STDERR_FILENO
        );
        libc::close(stderr_fd);

        let mut reader = fs::File::from_raw_fd(pipes[0]);
        let mut output = String::new();
        reader.read_to_string(&mut output).unwrap();
        output
    }
}

fn make_temp_home(prefix: &str) -> std::path::PathBuf {
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

#[test]
fn device_link_channel_defaults_to_cloud_for_unknown_device() {
    // A DID that was never discovered on the LAN nor primed into any credential
    // cache resolves to the cloud channel.
    assert_eq!(device_link_channel("99900011122233377"), MiotChannel::Cloud);
}

#[test]
fn local_credential_cache_keys_include_parent_for_sub_device() {
    assert_eq!(
        local_credential_cache_keys("2045038922.s2"),
        vec!["2045038922.s2".to_string(), "2045038922".to_string()]
    );
    assert_eq!(
        local_credential_cache_keys("2045038922"),
        vec!["2045038922".to_string()]
    );
}

#[test]
fn transient_lan_errors_are_retryable() {
    assert!(is_transient_lan_error(&anyhow::anyhow!(
        "Resource temporarily unavailable (os error 35)"
    )));
    assert!(is_transient_lan_error(&anyhow::anyhow!("operation timed out")));
    assert!(is_transient_lan_error(&anyhow::anyhow!(
        "miio AES decrypt failed: UnpadError"
    )));
    // A genuine protocol/parse error is not retried.
    assert!(!is_transient_lan_error(&anyhow::anyhow!(
        "miio command: bad reply"
    )));
}

#[test]
fn public_and_loopback_ips_are_not_on_local_subnet() {
    use crate::miot_lan::is_on_local_subnet;
    // Public addresses are never on one of our local subnets, so they are not
    // LAN-eligible regardless of the host's interfaces.
    assert!(!is_on_local_subnet("8.8.8.8".parse().unwrap()));
    assert!(!is_on_local_subnet("223.104.121.41".parse().unwrap()));
    // Loopback interfaces are excluded from subnet membership.
    assert!(!is_on_local_subnet("127.0.0.1".parse().unwrap()));
}

#[test]
fn forced_lan_fails_fast_when_device_is_off_subnet() {
    let _guard = env_guard();
    let device_id = "mico.force-lan-off-subnet";
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "force-lan-off-subnet",
        "deviceId": device_id,
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 12345,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));
    // Keep LAN discovery enabled so eligibility reaches the same-intranet check
    // (env_guard disables it by default for the cloud-path tests).
    std::env::set_var("MIT_DISABLE_LAN_DISCOVERY", "0");
    let client = MicoClient::new(&account).unwrap();
    // A primed credential whose IP is a public address (off-subnet).
    {
        let cache = shared_local_credential_cache(device_id);
        let mut cache = cache.lock().unwrap();
        cache.fetched_at = unix_timestamp();
        cache.by_did.insert(
            "dev-1".to_string(),
            LocalDeviceCredential {
                did: "dev-1".to_string(),
                name: "remote".to_string(),
                model: "xiaomi.wifispeaker.lx04".to_string(),
                local_ip: "223.104.121.41".to_string(),
                token: "00112233445566778899aabbccddeeff".to_string(),
                source: LocalCredentialSource::Direct,
            },
        );
        cache.ready_dids.insert("dev-1".to_string());
    }

    crate::mico_api::set_force_lan(true);
    let result = client.get_prop("dev-1", 2, 1);
    crate::mico_api::set_force_lan(false);
    // Restore the test-wide default so other (non-env_guard) tests never observe
    // LAN discovery enabled.
    std::env::set_var("MIT_DISABLE_LAN_DISCOVERY", "1");

    let error = result.expect_err("--LAN must fail fast for an off-subnet device");
    let message = error.to_string();
    assert!(
        message.contains("--LAN") && message.contains("off-subnet"),
        "unexpected error: {message}"
    );
}

#[test]
fn load_snapshot_credential_round_trips_persisted_snapshot() {
    let _guard = env_guard();
    let home = make_temp_home("local-credential-snapshot-reuse");
    env::set_var("MIT_HOME", &home);

    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "snapshot-reuse",
        "deviceId": "mico.snapshot-reuse",
        "accessToken": "token-a",
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));
    let client = MicoClient::new(&account).unwrap();

    let mut creds = HashMap::new();
    creds.insert(
        "2000354985".to_string(),
        LocalDeviceCredential {
            did: "2000354985".to_string(),
            name: "灯".to_string(),
            model: "x.y.z".to_string(),
            local_ip: "192.168.1.50".to_string(),
            token: "00112233445566778899aabbccddeeff".to_string(),
            source: LocalCredentialSource::Direct,
        },
    );
    client.write_local_credentials_snapshot(&creds).unwrap();

    // The CLI reuses the persisted snapshot the TUI wrote — without a cloud call.
    let loaded = client
        .load_snapshot_credential("2000354985")
        .unwrap()
        .expect("snapshot credential should be present");
    assert_eq!(loaded.local_ip, "192.168.1.50");
    assert_eq!(loaded.token, "00112233445566778899aabbccddeeff");

    assert!(client
        .load_snapshot_credential("404040404")
        .unwrap()
        .is_none());

    // The TUI hydration path reads every device from the same snapshot.
    let mut all = client.load_all_snapshot_credentials().unwrap();
    all.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].0, "2000354985");
    assert_eq!(all[0].1.local_ip, "192.168.1.50");
}
