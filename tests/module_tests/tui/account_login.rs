// Auto-split: shares the `tests` module scope (imports + helpers) of
// mod.rs via include!; do not add `use` here.

#[test]
fn forward_auth_login_output_emits_first_url_from_buffer() {
    let mut cursor = std::io::Cursor::new(
            b"{\"type\":\"authUrlPrinted\",\"url\":\"https://example.com/oauth\"}\n{\"type\":\"authWaiting\"}\n".to_vec(),
        );
    let (tx, rx) = mpsc::channel();
    account_page::forward_auth_login_output_until_eof(&mut cursor, &tx);
    let auth_url = rx
        .recv_timeout(Duration::from_millis(300))
        .unwrap()
        .unwrap();
    assert_eq!(auth_url, "https://example.com/oauth");
}

#[test]
fn account_list_row_shows_xiaomi_and_mijia_login_statuses() {
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-account-status",
        "deviceId": "mico.tui-account-status",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 32503680000_u64,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"},
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
    }));

    let row = account_page::account_list_row(&account, false, Language::Chinese);

    assert_eq!(row.xiaomi_status, "已登录");
    assert_eq!(row.mijia_status, "已登录");
    let columns = account_page::compute_account_list_columns(
        std::slice::from_ref(&row),
        80,
        Language::Chinese,
    );
    let header = account_page::format_account_list_header_with_columns(columns, Language::Chinese);
    assert!(header.contains("小米"));
    assert!(header.contains("米家"));
    let item = account_page::format_account_list_item_with_columns(&row, columns);
    assert!(item.contains("已登录"));
}

#[cfg(unix)]
#[test]
fn forward_auth_login_output_sends_url_before_eof() {
    use std::io::Write as _;
    use std::os::unix::net::UnixStream;

    let (mut writer, reader) = UnixStream::pair().unwrap();
    let (tx, rx) = mpsc::channel();

    let handle = std::thread::spawn(move || {
        let mut reader = std::io::BufReader::new(reader);
        account_page::forward_auth_login_output_until_eof(&mut reader, &tx);
    });

    writer
            .write_all(
                b"{\"type\":\"authUrlPrinted\",\"url\":\"https://example.com/oauth\"}\n{\"type\":\"authWaiting\"}\n",
            )
            .unwrap();
    writer.flush().unwrap();

    let auth_url = rx
        .recv_timeout(Duration::from_millis(300))
        .unwrap()
        .unwrap();
    assert_eq!(auth_url, "https://example.com/oauth");

    drop(writer);
    handle.join().unwrap();
}

#[test]
fn add_account_port_conflict_shows_error_dialog_without_quitting_tui() {
    let _guard = crate::test_support::env_guard();
    std::env::set_var(
        "MIT_TUI_TEST_REAUTH_ERROR",
        "port 8000 is occupied by another process",
    );

    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-add-account-port-conflict",
        "deviceId": "mico.tui-add-account-port-conflict",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 1,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![account],
        account_index: 0,
        devices: Vec::new(),
        device_index: 0,
        logs: VecDeque::new(),
        active_tab: 0,
        log_scroll_offset: 0,
        input_mode: false,
        input: String::new(),
        device_search_cursor: 0,
        search_inputs: Default::default(),
        search_cursors: [0; 3],
        prop_dialog: None,
        account_action_dialog: None,
        account_list_state: ListState::default(),
        device_list_state: ListState::default(),
        local_transport_fetching: false,
        local_transport_refresh_generation: 0,
        local_transport_refresh_device_id: None,
        local_transport_force_refresh_pending: false,
        local_transport_tx,
        local_transport_rx,
        auth_flow_generation: 0,
        auth_flow_tx,
        auth_flow_rx,
        offline_account_uids: HashSet::new(),
        boot_state: BootState::Ready,
        boot_spinner_index: 0,
        bootstrap_generation: 0,
        bootstrap_pending: None,
        bootstrap_tx,
        bootstrap_rx,
        property_cache: Arc::new(PropertyCache::new()),
        language: Language::Chinese,
        auto_subscribe_device_status: true,
        settings_selected: 0,
        update_check_rx: None,
        update_check_status: None,
        install_rx: None,
    };

    let quit = handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
    )
    .unwrap();
    assert!(!quit);

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("port 8000 is occupied"));

    std::env::remove_var("MIT_TUI_TEST_REAUTH_ERROR");
}

#[test]
fn parse_auth_login_output_line_accepts_json_event_and_plain_url() {
    assert_eq!(
        account_page::parse_auth_login_output_line(
            r#"{"type":"authUrlPrinted","url":"https://example.com/oauth"}"#
        )
        .as_deref(),
        Some("https://example.com/oauth")
    );
    assert_eq!(
        account_page::parse_auth_login_output_line("AUTH_URL https://example.com/direct")
            .as_deref(),
        Some("https://example.com/direct")
    );
}

#[test]
fn prop_dialog_operation_records_user_column_expands_to_nickname() {
    let mut app = app_with_single_readonly_prop_dialog();
    app.accounts = vec![test_account_with_mijia_for("1001", "VeryLongOperatorName", "cn")];
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.items[0].prop = PropItem {
        siid: 2,
        piid: 1,
        name: "Power".to_string(),
        format: "bool".to_string(),
        writable: false,
        value_options: Vec::new(),
    };
    dialog.items.push(raw_device_logs_item(json!({
        "requests": [
            {
                "key": "2.1",
                "response": {
                    "code": 0,
                    "result": [{"time": 0, "value": "[true]", "uid": "1001"}]
                }
            }
        ]
    })));
    dialog.active_tab = PropDialogTab::Logs;
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    let text = terminal_text(&terminal);
    assert!(text.contains("VeryLongOperatorName"), "{text}");
    assert!(text.replace(' ', "").contains("用户"), "{text}");
}

#[test]
fn extract_auth_url_from_line_accepts_json_and_plain_url() {
    let json_line = r#"{"type":"authUrlPrinted","url":"https://example.com/oauth"}"#;
    assert_eq!(
        super::extract_auth_url_from_line(json_line).as_deref(),
        Some("https://example.com/oauth")
    );
    assert_eq!(
        super::extract_auth_url_from_line("https://example.com/direct").as_deref(),
        Some("https://example.com/direct")
    );
}

#[test]
fn process_auth_flow_completion_closes_reauth_dialog() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: Vec::new(),
        account_index: 0,
        devices: Vec::new(),
        device_index: 0,
        logs: VecDeque::new(),
        active_tab: 0,
        log_scroll_offset: 0,
        input_mode: false,
        input: String::new(),
        device_search_cursor: 0,
        search_inputs: Default::default(),
        search_cursors: [0; 3],
        prop_dialog: None,
        account_action_dialog: Some(AccountActionDialog::Reauth {
            status: "Waiting browser callback".to_string(),
            auth_url: "https://example.com/auth".to_string(),
        }),
        account_list_state: ListState::default(),
        device_list_state: ListState::default(),
        local_transport_fetching: false,
        local_transport_refresh_generation: 0,
        local_transport_refresh_device_id: None,
        local_transport_force_refresh_pending: false,
        local_transport_tx,
        local_transport_rx,
        auth_flow_generation: 0,
        auth_flow_tx,
        auth_flow_rx,
        offline_account_uids: HashSet::new(),
        boot_state: BootState::Ready,
        boot_spinner_index: 0,
        bootstrap_generation: 0,
        bootstrap_pending: None,
        bootstrap_tx,
        bootstrap_rx,
        property_cache: Arc::new(PropertyCache::new()),
        language: Language::Chinese,
        auto_subscribe_device_status: true,
        settings_selected: 0,
        update_check_rx: None,
        update_check_status: None,
        install_rx: None,
    };

    app.auth_flow_tx
        .send(AuthFlowMessage::Completed {
            generation: app.auth_flow_generation,
            success: true,
            detail: "callback received".to_string(),
        })
        .unwrap();

    app.process_background_messages();

    assert!(app.account_action_dialog.is_none());
}

#[test]
fn failed_auth_flow_shows_port_8000_hint_in_reauth_dialog() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: Vec::new(),
        account_index: 0,
        devices: Vec::new(),
        device_index: 0,
        logs: VecDeque::new(),
        active_tab: 0,
        log_scroll_offset: 0,
        input_mode: false,
        input: String::new(),
        device_search_cursor: 0,
        search_inputs: Default::default(),
        search_cursors: [0; 3],
        prop_dialog: None,
        account_action_dialog: Some(AccountActionDialog::Reauth {
            status: "Waiting browser callback".to_string(),
            auth_url: "http://127.0.0.1:8000/login".to_string(),
        }),
        account_list_state: ListState::default(),
        device_list_state: ListState::default(),
        local_transport_fetching: false,
        local_transport_refresh_generation: 0,
        local_transport_refresh_device_id: None,
        local_transport_force_refresh_pending: false,
        local_transport_tx,
        local_transport_rx,
        auth_flow_generation: 0,
        auth_flow_tx,
        auth_flow_rx,
        offline_account_uids: HashSet::new(),
        boot_state: BootState::Ready,
        boot_spinner_index: 0,
        bootstrap_generation: 0,
        bootstrap_pending: None,
        bootstrap_tx,
        bootstrap_rx,
        property_cache: Arc::new(PropertyCache::new()),
        language: Language::Chinese,
        auto_subscribe_device_status: true,
        settings_selected: 0,
        update_check_rx: None,
        update_check_status: None,
        install_rx: None,
    };

    app.auth_flow_tx
        .send(AuthFlowMessage::Completed {
            generation: app.auth_flow_generation,
            success: false,
            detail: "auth login process exited with exit status: 1".to_string(),
        })
        .unwrap();

    app.process_background_messages();

    let status = match app.account_action_dialog.as_ref() {
        Some(AccountActionDialog::Reauth { status, .. }) => status,
        _ => panic!("reauth dialog should remain open on auth flow failure"),
    };
    assert!(status.contains("Failed to complete auth flow"));
    assert!(status.contains("登录回调必须使用 8000 端口"));
}

#[test]
fn accounts_search_matches_region_nickname_and_uid() {
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

    for query in ["sg", "Bedroom Account", "2002"] {
        let mut app = accounts_tab_test_app(vec![
            test_account_with("1001", "Kitchen Account", "cn"),
            test_account_with("2002", "Bedroom Account", "sg"),
        ]);
        app.language = Language::English;
        app.input = query.to_string();

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let text = terminal_text(&terminal);
        assert!(
            text.contains("Bedroom Account"),
            "query={query} text={text}"
        );
        assert!(
            !text.contains("Kitchen Account"),
            "query={query} text={text}"
        );
    }
}
