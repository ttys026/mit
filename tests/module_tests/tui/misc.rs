// Auto-split from the former monolithic tui.rs. Shares the `tests` module
// scope (imports + helpers) of mod.rs via include!; do not add `use` here.

#[test]
fn display_truncate_pad_pads_ascii_to_width() {
    let result = display_truncate_pad("hi", 5);
    assert_eq!(result, "hi   ");
    assert_eq!(result.len(), 5);
}

#[test]
fn display_truncate_pad_truncates_long_ascii() {
    let result = display_truncate_pad("hello world", 5);
    assert_eq!(result, "hello");
}

#[test]
fn pressing_j_does_not_move_selection_anymore() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: Vec::new(),
        account_index: 0,
        devices: vec![
            Device {
                did: "dev-1".to_string(),
                name: "d1".to_string(),
                model: "xiaomi.wifispeaker.lx04".to_string(),
                online: true,

                home_id: "cache-account:1001".to_string(),
                home_name: "A(1001)".to_string(),
                room_id: "room-1".to_string(),
                room_name: "客厅".to_string(),
            },
            Device {
                did: "dev-2".to_string(),
                name: "d2".to_string(),
                model: "xiaomi.wifispeaker.lx04".to_string(),
                online: true,

                home_id: "cache-account:1002".to_string(),
                home_name: "B(1002)".to_string(),
                room_id: "room-2".to_string(),
                room_name: "卧室".to_string(),
            },
        ],
        device_index: 0,
        logs: VecDeque::new(),
        active_tab: 1,
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
    };

    let quit = handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
    )
    .unwrap();
    assert!(!quit);
    assert_eq!(app.device_index, 0);
}

#[test]
fn exit_result_for_boot_state_is_ok_for_all_states() {
    let ok = exit_result_for_boot_state(&BootState::Ready);
    assert!(ok.is_ok());
}

#[test]
fn request_local_transport_refresh_force_queues_when_fetching() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-refresh-queue",
        "deviceId": "mico.tui-refresh-queue",
        "state": "state-a",
        "accessToken": "",
        "refreshToken": "",
        "expiresTs": 0,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![account.clone()],
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
        local_transport_fetching: true,
        local_transport_refresh_generation: 7,
        local_transport_refresh_device_id: Some(account.device_id.clone()),
        local_transport_force_refresh_pending: false,
        local_transport_tx: local_transport_tx.clone(),
        local_transport_rx,
        auth_flow_generation: 0,
        auth_flow_tx,
        auth_flow_rx,
        offline_account_uids: HashSet::new(),
        boot_state: BootState::Loading,
        boot_spinner_index: 0,
        bootstrap_generation: 0,
        bootstrap_pending: None,
        bootstrap_tx,
        bootstrap_rx,
        property_cache: Arc::new(PropertyCache::new()),
        language: Language::Chinese,
        auto_subscribe_device_status: true,
        settings_selected: 0,
    };

    app.request_local_transport_refresh(true);
    assert!(app.local_transport_force_refresh_pending);
    assert!(app.local_transport_fetching);
    assert_eq!(app.local_transport_refresh_generation, 7);

    local_transport_tx
        .send(LocalTransportRefreshMessage {
            generation: 7,
            error: None,
        })
        .unwrap();
    app.process_background_messages();

    assert!(!app.local_transport_force_refresh_pending);
    assert!(app.local_transport_fetching);
    assert_eq!(app.local_transport_refresh_generation, 8);
}

#[test]
fn mijia_raw_requests_hide_successful_empty_results() {
    let entries = super::visible_mijia_raw_request_entries(vec![
        json!({
            "key": "2.1",
            "response": {
                "code": 0,
                "message": "ok",
                "result": []
            }
        }),
        json!({
            "key": "2.2",
            "response": {
                "code": 0,
                "message": "ok",
                "result": [{"value": "[true]"}]
            }
        }),
        json!({
            "key": "2.3",
            "response": {
                "code": -8,
                "message": "invalid params",
                "result": null
            }
        }),
    ]);

    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["key"], "2.2");
    assert_eq!(entries[1]["key"], "2.3");
}

#[test]
fn refresh_cloud_mips_listeners_skips_when_auto_subscribe_setting_off() {
    let _guard = env_guard();
    std::env::remove_var("MIT_DISABLE_CLOUD_MIPS");
    std::env::remove_var("MIT_ENABLE_CLOUD_MIPS_IN_TESTS");
    let mut app = devices_tab_test_app(vec![test_device(
        "dev-kitchen",
        "kitchen plug",
        "Kitchen",
        "A(1001)",
    )]);
    app.active_tab = 3;
    app.settings_selected = 1;

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();
    app.refresh_cloud_mips_listeners();

    let logs = app.logs.iter().cloned().collect::<Vec<_>>().join("\n");
    assert!(logs.contains("cloud MIPS not started: auto subscribe disabled"));

    let mut runtime = super::cloud_mips_runtime().lock().unwrap();
    *runtime = None;
}

#[test]
fn start_bootstrap_creates_local_credentials_snapshot_without_restart() {
    let _guard = env_guard();
    let server = MockMicoServer::start();
    let home = make_temp_dir("tui-bootstrap-creates-local-credentials");
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "uuid-a",
        "deviceId": "device-a",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 32503680000_u64,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));
    let mut auth_state = default_auth();
    auth_state.accounts = vec![account.clone()];
    std::env::set_var("MIT_HOME", &home);
    std::env::set_var("MIT_PROFILE_DIR", &home);
    std::env::set_var("MIT_MICO_BASE_URL", server.base_url());
    std::env::set_var("MIT_USER_PROFILE_URL", server.user_profile_url());
    std::env::set_var(
        "MIT_MIOT_SPEC_URL_BASE",
        format!("{}/miot-spec-v2/instance", server.base_url()),
    );

    let mut app = TuiApp {
        home_dir: home.clone(),
        auth_state,
        accounts: vec![account],
        account_index: 0,
        devices: Vec::new(),
        device_index: 0,
        logs: VecDeque::new(),
        active_tab: 1,
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
    };

    app.start_bootstrap();

    let local_credentials_path = home
        .join(".mit")
        .join("accounts")
        .join("1001")
        .join("local_credentials.json");
    let deadline = Instant::now() + Duration::from_secs(8);
    while !local_credentials_path.exists() {
        app.process_background_messages();
        assert!(
            Instant::now() < deadline,
            "logs={:?} requests={:?}",
            app.logs,
            server.requests()
        );
        thread::sleep(Duration::from_millis(10));
    }

    std::env::remove_var("MIT_HOME");
    std::env::remove_var("MIT_PROFILE_DIR");
    std::env::remove_var("MIT_MICO_BASE_URL");
    std::env::remove_var("MIT_USER_PROFILE_URL");
    std::env::remove_var("MIT_MIOT_SPEC_URL_BASE");
    let _ = fs::remove_dir_all(&home);
}
