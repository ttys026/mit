use std::cell::RefCell;
use std::rc::Rc;

use anyhow::anyhow;
use mit::cli::{push_to_all_accounts_with, PushSent, PushSummary};
use mit::storage::normalize_auth;
use serde_json::json;

#[test]
fn push_to_all_accounts_matches_previous_js_behavior() {
    let auth_state = normalize_auth(json!({
        "activeUid": "1001",
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

    let calls = Rc::new(RefCell::new(Vec::<(String, String)>::new()));
    let result = push_to_all_accounts_with(
        "hello",
        || Ok(auth_state.clone()),
        |auth| Ok(auth.accounts.clone()),
        {
            let calls = Rc::clone(&calls);
            move |state, account, text| {
                calls
                    .borrow_mut()
                    .push((account.user.uid.clone(), text.to_string()));
                Ok((
                    state,
                    account.clone(),
                    format!("notify-{}", account.user.uid),
                ))
            }
        },
    )
    .unwrap();

    assert_eq!(
        calls.borrow().clone(),
        vec![
            ("1001".to_string(), "hello".to_string()),
            ("1002".to_string(), "hello".to_string())
        ]
    );
    assert_eq!(
        result,
        PushSummary {
            sent: vec![
                PushSent {
                    uid: "1001".to_string(),
                    nickname: "账号A".to_string(),
                    notify_id: "notify-1001".to_string(),
                },
                PushSent {
                    uid: "1002".to_string(),
                    nickname: "账号B".to_string(),
                    notify_id: "notify-1002".to_string(),
                }
            ],
            failed: vec![],
        }
    );
}

#[test]
fn push_to_all_accounts_keeps_partial_failures_in_summary() {
    let auth_state = normalize_auth(json!({
        "activeUid": "1001",
        "accounts": [
            {
                "region": "cn",
                "redirectUri": "https://127.0.0.1:8000/login_redirect",
                "uuid": "uuid-a",
                "accessToken": "token-a",
                "refreshToken": "refresh-a",
                "expiresTs": 111,
                "user": { "uid": "1001", "nickname": "账号A" }
            },
            {
                "region": "cn",
                "redirectUri": "https://127.0.0.1:8000/login_redirect",
                "uuid": "uuid-b",
                "accessToken": "token-b",
                "refreshToken": "refresh-b",
                "expiresTs": 222,
                "user": { "uid": "1002", "nickname": "账号B" }
            }
        ]
    }))
    .unwrap();

    let calls = Rc::new(RefCell::new(Vec::<String>::new()));
    let result = push_to_all_accounts_with(
        "hello",
        || Ok(auth_state.clone()),
        |auth| Ok(auth.accounts.clone()),
        {
            let calls = Rc::clone(&calls);
            move |state, account, _text| {
                calls.borrow_mut().push(account.user.uid.clone());
                if account.user.uid == "1002" {
                    Err(anyhow!("network timeout"))
                } else {
                    Ok((
                        state,
                        account.clone(),
                        format!("notify-{}", account.user.uid),
                    ))
                }
            }
        },
    )
    .unwrap();

    assert_eq!(
        calls.borrow().clone(),
        vec!["1001".to_string(), "1002".to_string()]
    );
    assert_eq!(result.sent.len(), 1);
    assert_eq!(result.sent[0].uid, "1001");
    assert_eq!(result.sent[0].notify_id, "notify-1001");
    assert_eq!(result.failed.len(), 1);
    assert_eq!(result.failed[0].account.user.uid, "1002");
    assert_eq!(result.failed[0].account.user.nickname, "账号B");
    assert_eq!(result.failed[0].error, "network timeout");
}

#[test]
fn push_to_all_accounts_skips_accounts_without_login_tokens() {
    let auth_state = normalize_auth(json!({
        "activeUid": "1001",
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
                "accessToken": "",
                "refreshToken": "",
                "expiresTs": 222,
                "user": { "uid": "1002", "nickname": "账号B" }
            }
        ]
    }))
    .unwrap();

    let calls = Rc::new(RefCell::new(Vec::<String>::new()));
    let result = push_to_all_accounts_with(
        "hello",
        || Ok(auth_state.clone()),
        |auth| Ok(auth.accounts.clone()),
        {
            let calls = Rc::clone(&calls);
            move |state, account, _text| {
                calls.borrow_mut().push(account.user.uid.clone());
                Ok((
                    state,
                    account.clone(),
                    format!("notify-{}", account.user.uid),
                ))
            }
        },
    )
    .unwrap();

    assert_eq!(calls.borrow().clone(), vec!["1001".to_string()]);
    assert_eq!(
        result.sent,
        vec![PushSent {
            uid: "1001".to_string(),
            nickname: "账号A".to_string(),
            notify_id: "notify-1001".to_string(),
        }]
    );
    assert!(result.failed.is_empty());
}
