
use super::*;

#[test]
fn login_user_id_prefers_sts_cookie_over_long_polling_payload() {
    let mut headers = HeaderMap::new();
    headers.append(
        SET_COOKIE,
        HeaderValue::from_static("userId=3009043526; Path=/; HttpOnly"),
    );
    headers.append(
        SET_COOKIE,
        HeaderValue::from_static("cUserId=encrypted-user; Path=/; HttpOnly"),
    );
    let cookies = cookies_from_response(&headers);
    let payload = json!({"userId": "", "cUserId": "payload-user"});

    assert_eq!(login_user_id(&cookies, &payload), "3009043526");
}

#[test]
fn default_mijia_api_base_url_uses_mijia_sid_host() {
    std::env::remove_var(MIJIA_API_BASE_URL_ENV);

    let client = MijiaClient::new().unwrap();

    assert_eq!(client.api_base_url, "https://api.mijia.tech/app");
}

#[test]
fn check_new_msg_payload_includes_begin_at_timestamp() {
    let payload = check_new_msg_payload();
    let begin_at = payload
        .get("begin_at")
        .and_then(Value::as_i64)
        .expect("begin_at timestamp");

    assert!(begin_at > 0);
}

#[test]
fn device_history_payload_targets_device_key_type_and_recent_window() {
    let payload = device_history_payload("device-1", "2.1", "prop", DeviceHistoryQuery::recent(5));

    assert_eq!(payload["did"], "device-1");
    assert_eq!(payload["key"], "2.1");
    assert_eq!(payload["type"], "prop");
    assert!(payload.get("uid").is_none());
    assert_eq!(payload["limit"], 5);
    let time_start = payload["time_start"].as_i64().unwrap();
    let time_end = payload["time_end"].as_i64().unwrap();
    assert!(time_end >= time_start);
    assert!(time_end - time_start <= 32 * 24 * 60 * 60 + 60);
}

#[test]
fn device_history_payload_strips_sub_device_suffix() {
    let payload = device_history_payload(
        "2045081210.s2",
        "2.1",
        "prop",
        DeviceHistoryQuery::recent(5),
    );

    assert_eq!(payload["did"], "2045081210");
}

#[test]
fn device_history_payload_accepts_explicit_window_and_limit() {
    let payload = device_history_payload(
        "device-1",
        "2.1",
        "prop",
        DeviceHistoryQuery {
            time_start: 1_700_000_000,
            time_end: 1_700_086_399,
            limit: 20,
        },
    );

    assert_eq!(payload["time_start"], 1_700_000_000);
    assert_eq!(payload["time_end"], 1_700_086_399);
    assert_eq!(payload["limit"], 20);
}

#[test]
fn device_statistics_payload_targets_device_key_type_and_recent_window() {
    let payload = device_statistics_payload("device-1", "11.1", "stat_day_v3");

    assert_eq!(payload["did"], "device-1");
    assert_eq!(payload["key"], "11.1");
    assert_eq!(payload["data_type"], "stat_day_v3");
    assert!(payload.get("uid").is_none());
    assert_eq!(payload["limit"], 31);
    let time_start = payload["time_start"].as_i64().unwrap();
    let time_end = payload["time_end"].as_i64().unwrap();
    assert!(time_end >= time_start);
    assert!(time_end - time_start <= 32 * 24 * 60 * 60 + 60);
}

#[test]
fn device_statistics_payload_accepts_explicit_window_and_limit() {
    let payload = device_statistics_payload_with_query(
        "device-1",
        "11.1",
        "stat_day_v3",
        DeviceHistoryQuery {
            time_start: 1_700_000_000,
            time_end: 1_700_604_799,
            limit: 7,
        },
    );

    assert_eq!(payload["time_start"], 1_700_000_000);
    assert_eq!(payload["time_end"], 1_700_604_799);
    assert_eq!(payload["limit"], 7);
}

#[test]
fn device_statistics_payload_strips_sub_device_suffix() {
    let payload = device_statistics_payload("2045081210.s2", "11.1", "stat_day_v3");

    assert_eq!(payload["did"], "2045081210");
}

#[test]
fn user_profile_from_mijia_home_payload_uses_owned_home_uid() {
    let payload = json!({
        "code": 0,
        "result": {
            "homelist": [
                {
                    "id": "home-a",
                    "name": "Home",
                    "uid": 3009043526_u64,
                    "roomlist": []
                }
            ],
            "share_home_list": [
                {
                    "id": "shared-home",
                    "name": "Shared",
                    "uid": 2002,
                    "roomlist": []
                }
            ]
        }
    });

    let profile = user_profile_from_mijia_home_payload(&payload).unwrap();
    assert_eq!(profile.uid, "3009043526");
    assert_eq!(profile.nickname, "");
}
