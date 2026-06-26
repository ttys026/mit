use mit::mico_api::{
    build_device_id, build_state, parse_callback_input, MicoClient, OAUTH_CLIENT_ID,
};
use mit::storage::normalize_account;
use mit::test_support;
use serde_json::json;

#[path = "support/mock_mico_server.rs"]
mod mock_mico_server;

use mock_mico_server::MockMicoServer;

fn env_guard() -> std::sync::MutexGuard<'static, ()> {
    test_support::env_guard()
}

#[test]
fn mico_helpers_match_previous_js_behavior() {
    let callback =
        parse_callback_input("https://127.0.0.1:8000/login_redirect?code=abc&state=xyz").unwrap();
    assert_eq!(callback.code, "abc");
    assert_eq!(callback.state, "xyz");

    assert_eq!(build_device_id("abcd").unwrap(), "mico.abcd");
    assert_eq!(build_state("mico.abcd").len(), 40);

    let client = MicoClient::new(&normalize_account(json!({
        "region": "cn",
        "redirectUri": "https://127.0.0.1:8000/login_redirect",
        "uuid": "abcd1234abcd1234abcd1234abcd1234",
        "accessToken": ""
    })))
    .unwrap();

    let auth_url = client.auth_url(false);
    assert!(auth_url.contains(&format!("client_id={OAUTH_CLIENT_ID}")));
    assert!(auth_url.contains("device_id=mico.abcd1234abcd1234abcd1234abcd1234"));
    assert!(auth_url.contains("skip_confirm=False"));
}

#[test]
fn mico_get_props_batch_sends_single_batched_request() {
    let _guard = env_guard();
    let server = MockMicoServer::start();
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "abcd1234abcd1234abcd1234abcd1234",
        "accessToken": "token-a"
    }));

    std::env::set_var("MIT_MICO_BASE_URL", server.base_url());
    let client = MicoClient::new(&account).unwrap();
    let result = client
        .get_props_batch(&[("dev-1", 2, 1), ("dev-1", 2, 2), ("dev-1", 3, 1)])
        .unwrap();

    let requests = server
        .requests()
        .into_iter()
        .filter(|request| request.path == "/app/v2/miotspec/prop/get")
        .collect::<Vec<_>>();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].body.contains(r#""piid":1"#));
    assert!(requests[0].body.contains(r#""piid":2"#));
    assert_eq!(result.as_array().unwrap().len(), 3);
    std::env::remove_var("MIT_MICO_BASE_URL");
}

#[test]
fn mico_get_devices_requests_third_party_devices() {
    let _guard = env_guard();
    let server = MockMicoServer::start();
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "abcd1234abcd1234abcd1234abcd1234",
        "accessToken": "token-a",
        "user": {"uid": "1001"}
    }));

    std::env::set_var("MIT_MICO_BASE_URL", server.base_url());
    let client = MicoClient::new(&account).unwrap();
    let devices = client.get_devices().unwrap();

    assert!(
        devices.iter().any(|device| device.did == "third-1"),
        "third-party devices should be returned when get_third_device is requested"
    );
    // The connect type (`pid`) is captured from the API and carried per-did.
    let third = devices
        .iter()
        .find(|device| device.did == "third-1")
        .expect("third-party device present");
    assert_eq!(third.pid, 14, "third-party cloud device should carry pid=14");
    let wifi = devices
        .iter()
        .find(|device| device.did == "dev-1")
        .expect("wifi device present");
    assert_eq!(wifi.pid, 0, "wifi device should carry pid=0");
    let requests = server.requests();
    let request = requests
        .iter()
        .find(|request| request.path == "/app/v2/home/home_device_list")
        .expect("home_device_list should be requested");
    let body: serde_json::Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(
        body.get("get_third_device")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    std::env::remove_var("MIT_MICO_BASE_URL");
}
