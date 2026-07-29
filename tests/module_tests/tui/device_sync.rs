// Auto-split: shares the `tests` module scope (imports + helpers) of
// mod.rs via include!; do not add `use` here.

#[test]
fn stale_keypress_uses_recent_pingresp_as_mqtt_response_timer() {
    let _guard = env_guard();
    std::env::remove_var("MIT_DISABLE_CLOUD_MIPS");
    std::env::set_var("MIT_ENABLE_CLOUD_MIPS_IN_TESTS", "1");
    let mut app = test_app_with_prop_dialog(PropDialog {
        device_did: "dev-1".to_string(),
        device_name: "speaker".to_string(),
        account_uid: "1001".to_string(),
        items: vec![ToggleItem {
            prop: PropItem {
                siid: 2,
                piid: 1,
                name: "Power".to_string(),
                format: "bool".to_string(),
                writable: true,
                value_options: Vec::new(),
            },
            value: Value::Bool(true),
        }],
        selected: 0,
        active_tab: PropDialogTab::Writable,
        writable_selected: 0,
        readonly_selected: 0,
        actions: Vec::new(),
        actions_selected: 0,
        writable_list_state: ListState::default(),
        readonly_list_state: ListState::default(),
        actions_list_state: ListState::default(),
        loading: false,
        loading_rx: None,
        status: None,
        editing: true,
        edit_buffer: "true".to_string(),
        edit_cursor: 4,
        edit_error: None,
        refreshing: false,
        refresh_rx: None,
        statistics_selected_bar: None,
    });
    let (_tx, rx) = mpsc::channel();
    let now = Instant::now();
    {
        let mut runtime = super::cloud_mips_runtime().lock().unwrap();
        *runtime = Some(super::CloudMipsRuntime {
            key: "test-runtime".to_string(),
            _handles: Vec::new(),
            rx,
            last_mqtt_response_at: Some(now.checked_sub(Duration::from_secs(8)).unwrap_or(now)),
            last_ping_req_at: Some(now.checked_sub(Duration::from_secs(9)).unwrap_or(now)),
            last_ping_resp_at: Some(now.checked_sub(Duration::from_secs(8)).unwrap_or(now)),
        });
    }

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
    )
    .unwrap();

    let logs = app.logs.iter().cloned().collect::<Vec<_>>().join("\n");
    assert!(!logs.contains("cloud MIPS response stale"));
    assert!(!logs.contains("cloud MIPS waiting for PingResp"));
    assert!(!logs.contains("heartbeat queued"));
    assert!(!logs.contains("subscribe probe"));
    assert!(!app.prop_dialog.as_ref().unwrap().refreshing);

    std::env::remove_var("MIT_ENABLE_CLOUD_MIPS_IN_TESTS");
    let mut runtime = super::cloud_mips_runtime().lock().unwrap();
    *runtime = None;
}

#[test]
fn stale_keypress_refreshes_open_prop_editor() {
    let _guard = env_guard();
    std::env::remove_var("MIT_DISABLE_CLOUD_MIPS");
    std::env::set_var("MIT_ENABLE_CLOUD_MIPS_IN_TESTS", "1");
    let mut app = test_app_with_prop_dialog(PropDialog {
        device_did: "dev-1".to_string(),
        device_name: "speaker".to_string(),
        account_uid: "1001".to_string(),
        items: vec![ToggleItem {
            prop: PropItem {
                siid: 2,
                piid: 1,
                name: "Power".to_string(),
                format: "bool".to_string(),
                writable: true,
                value_options: Vec::new(),
            },
            value: Value::Bool(true),
        }],
        selected: 0,
        active_tab: PropDialogTab::Writable,
        writable_selected: 0,
        readonly_selected: 0,
        actions: Vec::new(),
        actions_selected: 0,
        writable_list_state: ListState::default(),
        readonly_list_state: ListState::default(),
        actions_list_state: ListState::default(),
        loading: false,
        loading_rx: None,
        status: None,
        editing: true,
        edit_buffer: "true".to_string(),
        edit_cursor: 4,
        edit_error: None,
        refreshing: false,
        refresh_rx: None,
        statistics_selected_bar: None,
    });
    let (_tx, rx) = mpsc::channel();
    let now = Instant::now();
    {
        let mut runtime = super::cloud_mips_runtime().lock().unwrap();
        *runtime = Some(super::CloudMipsRuntime {
            key: "test-runtime".to_string(),
            _handles: Vec::new(),
            rx,
            last_mqtt_response_at: Some(now.checked_sub(Duration::from_secs(301)).unwrap_or(now)),
            last_ping_req_at: Some(now.checked_sub(Duration::from_secs(301)).unwrap_or(now)),
            last_ping_resp_at: Some(now.checked_sub(Duration::from_secs(301)).unwrap_or(now)),
        });
    }

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Home, KeyModifiers::NONE),
    )
    .unwrap();

    let dialog = app.prop_dialog.as_ref().unwrap();
    assert!(dialog.editing);
    assert!(dialog.refreshing);
    assert!(dialog.refresh_rx.is_some());
    assert_eq!(dialog.edit_buffer, "true");
    let logs = app.logs.iter().cloned().collect::<Vec<_>>().join("\n");
    assert!(logs.contains("cloud MIPS response stale"));

    std::env::remove_var("MIT_ENABLE_CLOUD_MIPS_IN_TESTS");
    let mut runtime = super::cloud_mips_runtime().lock().unwrap();
    *runtime = None;
}

#[test]
fn process_background_messages_logs_local_transport_refresh_errors() {
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
        account_action_dialog: None,
        account_list_state: ListState::default(),
        device_list_state: ListState::default(),
        local_transport_fetching: true,
        local_transport_refresh_generation: 7,
        local_transport_refresh_device_id: None,
        local_transport_force_refresh_pending: false,
        local_transport_tx: local_transport_tx.clone(),
        local_transport_rx,
        auth_flow_generation: 0,
        auth_flow_tx,
        auth_flow_rx,
        offline_account_uids: HashSet::new(),
        invalid_xiaomi_account_uids: HashSet::new(),
        invalid_mijia_account_uids: HashSet::new(),
        account_check_in_flight: false,
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
        update_check_rx: None,
        update_check_status: None,
        install_rx: None,
    };

    local_transport_tx
        .send(LocalTransportRefreshMessage {
            generation: 7,
            error: Some("snapshot write failed".to_string()),
        })
        .unwrap();

    app.process_background_messages();

    assert!(!app.local_transport_fetching);
    assert!(app
        .logs
        .iter()
        .any(|line| line.contains("snapshot write failed")));
}

#[test]
fn process_initial_prop_dialog_loading_leaves_props_ready_while_raw_tabs_continue() {
    let mut app = app_with_single_readonly_prop_dialog();
    app.language = Language::English;
    let (loading_tx, loading_rx) = mpsc::channel::<std::result::Result<Vec<ToggleItem>, String>>();
    loading_tx
        .send(Ok(vec![
            ToggleItem {
                prop: PropItem {
                    siid: 2,
                    piid: 1,
                    name: "Power".to_string(),
                    format: "bool".to_string(),
                    writable: true,
                    value_options: Vec::new(),
                },
                value: Value::Bool(true),
            },
            raw_device_logs_item(json!({"status": "loading"})),
            raw_device_statistics_item(json!({"status": "loading"})),
        ]))
        .unwrap();
    let (_raw_tx, raw_rx) = mpsc::channel::<super::PropDialogRefreshMessage>();
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.loading = true;
    dialog.loading_rx = Some(loading_rx);
    dialog.refreshing = true;
    dialog.refresh_rx = Some(raw_rx);
    dialog.active_tab = PropDialogTab::Writable;
    dialog.selected = 0;
    dialog.writable_selected = 0;

    app.process_prop_dialog_loading();

    let dialog = app.prop_dialog.as_mut().unwrap();
    assert!(!dialog.loading);
    assert!(!dialog.refreshing);
    assert!(dialog.refresh_rx.is_some());
    assert!(!super::prop_dialog_active_tab_is_loading(dialog));
    dialog.active_tab = PropDialogTab::Logs;
    assert!(super::prop_dialog_active_tab_is_loading(dialog));
    dialog.active_tab = PropDialogTab::Statistics;
    assert!(super::prop_dialog_active_tab_is_loading(dialog));
}

#[test]
fn stale_mouse_click_inside_prop_dialog_does_not_refresh() {
    let _guard = env_guard();
    std::env::remove_var("MIT_DISABLE_CLOUD_MIPS");
    std::env::set_var("MIT_ENABLE_CLOUD_MIPS_IN_TESTS", "1");
    let mut app = test_app_with_prop_dialog(PropDialog {
        device_did: "dev-1".to_string(),
        device_name: "speaker".to_string(),
        account_uid: "1001".to_string(),
        items: vec![ToggleItem {
            prop: PropItem {
                siid: 2,
                piid: 1,
                name: "Power".to_string(),
                format: "bool".to_string(),
                writable: true,
                value_options: Vec::new(),
            },
            value: Value::Bool(true),
        }],
        selected: 0,
        active_tab: PropDialogTab::Writable,
        writable_selected: 0,
        readonly_selected: 0,
        actions: Vec::new(),
        actions_selected: 0,
        writable_list_state: ListState::default(),
        readonly_list_state: ListState::default(),
        actions_list_state: ListState::default(),
        loading: false,
        loading_rx: None,
        status: None,
        editing: false,
        edit_buffer: String::new(),
        edit_cursor: 0,
        edit_error: None,
        refreshing: false,
        refresh_rx: None,
        statistics_selected_bar: None,
    });
    let (_tx, rx) = mpsc::channel();
    let now = Instant::now();
    {
        let mut runtime = super::cloud_mips_runtime().lock().unwrap();
        *runtime = Some(super::CloudMipsRuntime {
            key: "test-runtime".to_string(),
            _handles: Vec::new(),
            rx,
            last_mqtt_response_at: Some(now.checked_sub(Duration::from_secs(301)).unwrap_or(now)),
            last_ping_req_at: Some(now.checked_sub(Duration::from_secs(301)).unwrap_or(now)),
            last_ping_resp_at: Some(now.checked_sub(Duration::from_secs(301)).unwrap_or(now)),
        });
    }

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 2,
            row: 10,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();

    let dialog = app.prop_dialog.as_ref().unwrap();
    assert!(!dialog.refreshing);
    assert!(dialog.refresh_rx.is_none());
    let logs = app.logs.iter().cloned().collect::<Vec<_>>().join("\n");
    assert!(!logs.contains("cloud MIPS response stale"));

    std::env::remove_var("MIT_ENABLE_CLOUD_MIPS_IN_TESTS");
    let mut runtime = super::cloud_mips_runtime().lock().unwrap();
    *runtime = None;
}

#[test]
fn process_bootstrap_failure_logs_error_and_keeps_tui_ready() {
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
        invalid_xiaomi_account_uids: HashSet::new(),
        invalid_mijia_account_uids: HashSet::new(),
        account_check_in_flight: false,
        boot_state: BootState::Loading,
        boot_spinner_index: 0,
        bootstrap_generation: 1,
        bootstrap_pending: Some(BootstrapPending {
            generation: 1,
            uid: "1001".to_string(),
            refresh_local_transport_if_missing: false,
        }),
        bootstrap_tx: bootstrap_tx.clone(),
        bootstrap_rx,
        property_cache: Arc::new(PropertyCache::new()),
        language: Language::Chinese,
        auto_subscribe_device_status: true,
        settings_selected: 0,
        update_check_rx: None,
        update_check_status: None,
        install_rx: None,
    };

    bootstrap_tx
        .send(BootstrapMessage::Failed {
            generation: 1,
            uid: "1001".to_string(),
            auth_state: None,
            accounts: None,
            error: "boom".to_string(),
        })
        .unwrap();

    app.process_background_messages();

    assert!(matches!(app.boot_state, BootState::Ready));
    assert!(app.logs.iter().any(|line| line.contains("boom")));
}

#[test]
fn sync_command_keeps_ui_ready_while_background_sync_runs() {
    let home = make_temp_dir("tui-sync-non-blocking-ready");
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
    let mut app = TuiApp {
        home_dir: home.clone(),
        auth_state: default_auth(),
        accounts: vec![account],
        account_index: 0,
        devices: vec![Device {
            did: "dev-1".to_string(),
            name: "cached-device".to_string(),
            model: "xiaomi.gateway.hub1".to_string(),
            online: true,
            pid: 0,

            home_id: "home-1".to_string(),
            home_name: "账号A(1001)".to_string(),
            room_id: "room-1".to_string(),
            room_name: "客厅".to_string(),
        }],
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
        invalid_xiaomi_account_uids: HashSet::new(),
        invalid_mijia_account_uids: HashSet::new(),
        account_check_in_flight: false,
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

    app.start_manual_sync();
    assert!(
        matches!(app.boot_state, BootState::Ready),
        "sync should keep UI interactive while background sync runs"
    );
    assert!(app.bootstrap_pending.is_some());
    assert_eq!(app.devices.len(), 1);

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn process_bootstrap_message_applies_refreshed_auth_state() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();

    let old = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-old",
        "deviceId": "mico.tui-old",
        "state": "state-a",
        "accessToken": "old-token",
        "refreshToken": "refresh",
        "expiresTs": 1,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));
    let refreshed = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-old",
        "deviceId": "mico.tui-new",
        "state": "state-b",
        "accessToken": "new-token",
        "refreshToken": "new-refresh",
        "expiresTs": 2,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));
    let mut refreshed_state = default_auth();
    refreshed_state.accounts = vec![refreshed.clone()];

    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: {
            let mut state = default_auth();
            state.accounts = vec![old.clone()];
            state
        },
        accounts: vec![old],
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
        invalid_xiaomi_account_uids: HashSet::new(),
        invalid_mijia_account_uids: HashSet::new(),
        account_check_in_flight: false,
        boot_state: BootState::Loading,
        boot_spinner_index: 0,
        bootstrap_generation: 1,
        bootstrap_pending: Some(BootstrapPending {
            generation: 1,
            uid: "1001".to_string(),
            refresh_local_transport_if_missing: false,
        }),
        bootstrap_tx: bootstrap_tx.clone(),
        bootstrap_rx,
        property_cache: Arc::new(PropertyCache::new()),
        language: Language::Chinese,
        auto_subscribe_device_status: true,
        settings_selected: 0,
        update_check_rx: None,
        update_check_status: None,
        install_rx: None,
    };

    bootstrap_tx
        .send(BootstrapMessage::Ready {
            generation: 1,
            uid: "1001".to_string(),
            offline_uids: Vec::new(),
            auth_state: refreshed_state.clone(),
            accounts: vec![refreshed.clone()],
            devices: Vec::new(),
            logs: vec!["ok".to_string()],
        })
        .unwrap();

    app.process_background_messages();

    assert_eq!(app.auth_state, refreshed_state);
    assert_eq!(app.accounts, vec![refreshed]);
    assert_eq!(app.account_index, 0);
}

#[test]
fn process_bootstrap_failure_applies_refreshed_auth_state() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();

    let old = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-old",
        "deviceId": "mico.tui-old",
        "state": "state-a",
        "accessToken": "old-token",
        "refreshToken": "refresh",
        "expiresTs": 1,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));
    let refreshed = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-old",
        "deviceId": "mico.tui-new",
        "state": "state-b",
        "accessToken": "new-token",
        "refreshToken": "new-refresh",
        "expiresTs": 2,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));
    let mut refreshed_state = default_auth();
    refreshed_state.accounts = vec![refreshed.clone()];

    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: {
            let mut state = default_auth();
            state.accounts = vec![old.clone()];
            state
        },
        accounts: vec![old],
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
        invalid_xiaomi_account_uids: HashSet::new(),
        invalid_mijia_account_uids: HashSet::new(),
        account_check_in_flight: false,
        boot_state: BootState::Loading,
        boot_spinner_index: 0,
        bootstrap_generation: 1,
        bootstrap_pending: Some(BootstrapPending {
            generation: 1,
            uid: "1001".to_string(),
            refresh_local_transport_if_missing: false,
        }),
        bootstrap_tx: bootstrap_tx.clone(),
        bootstrap_rx,
        property_cache: Arc::new(PropertyCache::new()),
        language: Language::Chinese,
        auto_subscribe_device_status: true,
        settings_selected: 0,
        update_check_rx: None,
        update_check_status: None,
        install_rx: None,
    };

    bootstrap_tx
        .send(BootstrapMessage::Failed {
            generation: 1,
            uid: "1001".to_string(),
            auth_state: Some(refreshed_state.clone()),
            accounts: Some(vec![refreshed.clone()]),
            error: "boom".to_string(),
        })
        .unwrap();

    app.process_background_messages();

    assert_eq!(app.auth_state, refreshed_state);
    assert_eq!(app.accounts, vec![refreshed]);
    assert_eq!(app.account_index, 0);
    assert!(matches!(app.boot_state, BootState::Ready));
    assert!(app.logs.iter().any(|line| line.contains("boom")));
}

#[test]
fn exit_result_for_boot_state_is_ok_for_all_states() {
    let ok = exit_result_for_boot_state(&BootState::Ready);
    assert!(ok.is_ok());
}

#[test]
fn read_device_categories_from_template_returns_model_category_mapping() {
    let home = make_temp_dir("tui-device-category-map");
    let specs = home.join(".mit").join("cache").join("specs");
    fs::create_dir_all(&specs).unwrap();
    fs::create_dir_all(specs.join("sources")).unwrap();
    fs::write(
        specs.join("sources").join("template_list_device.json"),
        serde_json::to_string_pretty(&json!({
            "result": [
                {
                    "type": "urn:miot-spec-v2:device:speaker:0000A015",
                    "description": {"en": "Speaker", "zh_cn": "音箱"}
                },
                {
                    "model": "legacy.model",
                    "category_name": "legacy-category"
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        specs.join("index.json"),
        serde_json::to_string_pretty(&json!({
            "xiaomi.wifispeaker.lx04": {
                "urn": "urn:miot-spec-v2:device:speaker:0000A015:xiaomi-lx04:1"
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let categories =
        read_device_categories_from_template(home.as_path(), Language::Chinese).unwrap();

    assert_eq!(
        categories
            .get("xiaomi.wifispeaker.lx04")
            .map(String::as_str),
        Some("音箱")
    );
    assert_eq!(
        categories.get("legacy.model").map(String::as_str),
        Some("legacy-category")
    );

    let _ = fs::remove_dir_all(home);
}

#[test]
fn device_models_missing_local_specs_returns_unique_uncached_models() {
    let home = make_temp_dir("tui-missing-device-spec-models");
    let specs = home.join(".mit").join("cache").join("specs").join("models");
    fs::create_dir_all(&specs).unwrap();
    fs::create_dir_all(
        home.join(".mit")
            .join("cache")
            .join("specs")
            .join("sources"),
    )
    .unwrap();
    fs::write(specs.join("xiaomi.gateway.hub1.json"), "{}\n").unwrap();
    fs::write(
        home.join(".mit")
            .join("cache")
            .join("specs")
            .join("sources")
            .join("template_list_device.json"),
        "{ \"result\": [] }\n",
    )
    .unwrap();
    fs::write(
        home.join(".mit")
            .join("cache")
            .join("specs")
            .join("index.json"),
        "{}\n",
    )
    .unwrap();

    let devices = vec![
        Device {
            did: "dev-1".to_string(),
            name: "living-room".to_string(),
            model: "xiaomi.wifispeaker.lx04".to_string(),
            online: true,
            pid: 0,
            home_id: "home-1".to_string(),
            home_name: "我家".to_string(),
            room_id: "room-1".to_string(),
            room_name: "客厅".to_string(),
        },
        Device {
            did: "dev-2".to_string(),
            name: "living-room-2".to_string(),
            model: "xiaomi.wifispeaker.lx04".to_string(),
            online: true,
            pid: 0,
            home_id: "home-1".to_string(),
            home_name: "我家".to_string(),
            room_id: "room-2".to_string(),
            room_name: "卧室".to_string(),
        },
        Device {
            did: "dev-3".to_string(),
            name: "gateway".to_string(),
            model: "xiaomi.gateway.hub1".to_string(),
            online: true,
            pid: 0,
            home_id: "home-1".to_string(),
            home_name: "我家".to_string(),
            room_id: "room-3".to_string(),
            room_name: "书房".to_string(),
        },
        Device {
            did: "dev-4".to_string(),
            name: "blank-model".to_string(),
            model: "   ".to_string(),
            online: true,
            pid: 0,
            home_id: "home-1".to_string(),
            home_name: "我家".to_string(),
            room_id: "room-4".to_string(),
            room_name: "阳台".to_string(),
        },
    ];

    let missing = device_models_missing_local_specs(home.as_path(), &devices);

    assert_eq!(missing, vec!["xiaomi.wifispeaker.lx04".to_string()]);

    let _ = fs::remove_dir_all(home);
}

#[test]
fn device_models_missing_local_specs_treats_missing_category_metadata_as_missing_specs() {
    let home = make_temp_dir("tui-missing-device-spec-metadata");

    let devices = vec![
        Device {
            did: "dev-1".to_string(),
            name: "living-room".to_string(),
            model: "xiaomi.wifispeaker.lx04".to_string(),
            online: true,
            pid: 0,
            home_id: "home-1".to_string(),
            home_name: "我家".to_string(),
            room_id: "room-1".to_string(),
            room_name: "客厅".to_string(),
        },
        Device {
            did: "dev-2".to_string(),
            name: "gateway".to_string(),
            model: "xiaomi.gateway.hub1".to_string(),
            online: true,
            pid: 0,
            home_id: "home-1".to_string(),
            home_name: "我家".to_string(),
            room_id: "room-2".to_string(),
            room_name: "书房".to_string(),
        },
    ];

    let missing = device_models_missing_local_specs(home.as_path(), &devices);

    assert_eq!(
        missing,
        vec![
            "xiaomi.wifispeaker.lx04".to_string(),
            "xiaomi.gateway.hub1".to_string()
        ]
    );

    let _ = fs::remove_dir_all(home);
}

#[test]
fn devices_tab_r_starts_background_sync() {
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
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
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
        invalid_xiaomi_account_uids: HashSet::new(),
        invalid_mijia_account_uids: HashSet::new(),
        account_check_in_flight: false,
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
        crossterm::event::KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
    )
    .unwrap();
    assert!(!quit);
    assert!(app.bootstrap_pending.is_some());
}

#[test]
fn devices_tab_s_no_longer_starts_background_sync() {
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
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
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
        invalid_xiaomi_account_uids: HashSet::new(),
        invalid_mijia_account_uids: HashSet::new(),
        account_check_in_flight: false,
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
        crossterm::event::KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE),
    )
    .unwrap();
    assert!(!quit);
    assert!(app.bootstrap_pending.is_none());
}

#[test]
fn load_cached_devices_from_home_ignores_local_credentials_for_all_accounts() {
    let home = make_temp_dir("tui-local-credentials-merge-all-accounts");
    let mit_dir = home.join(".mit");
    fs::create_dir_all(&mit_dir).unwrap();
    fs::write(
        mit_dir.join("auth.json"),
        serde_json::to_string_pretty(&json!({
            "accounts": [
                persisted_auth_account_json("1001", "账号A", "union-a", "uuid-a", "device-a", "state-a", "token-a", "refresh-a", 1),
                persisted_auth_account_json("1002", "账号B", "union-b", "uuid-b", "device-b", "state-b", "token-b", "refresh-b", 1)
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    fs::create_dir_all(mit_dir.join("accounts").join("1001")).unwrap();
    fs::create_dir_all(mit_dir.join("accounts").join("1002")).unwrap();
    fs::write(
        mit_dir
            .join("accounts")
            .join("1001")
            .join("local_credentials.json"),
        serde_json::to_string_pretty(&json!({
            "dev-a": {
                "did": "dev-a",
                "name": "speaker-a",
                "model": "xiaomi.wifispeaker.lx04",
                "localIp": "192.168.1.11",
                "token": "00112233445566778899aabbccddeeff",
                "source": "direct"
            }
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        mit_dir
            .join("accounts")
            .join("1002")
            .join("local_credentials.json"),
        serde_json::to_string_pretty(&json!({
            "dev-b": {
                "did": "dev-b",
                "name": "speaker-b",
                "model": "xiaomi.wifispeaker.lx04",
                "localIp": "192.168.1.12",
                "token": "ffeeddccbbaa99887766554433221100",
                "source": "direct"
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let devices = load_cached_devices_from_home(&home, "").unwrap();
    assert!(devices.is_empty());

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn start_bootstrap_without_current_account_enters_ready_state() {
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
        account_action_dialog: None,
        account_list_state: ListState::default(),
        device_list_state: ListState::default(),
        local_transport_fetching: false,
        local_transport_refresh_generation: 5,
        local_transport_refresh_device_id: None,
        local_transport_force_refresh_pending: true,
        local_transport_tx,
        local_transport_rx,
        auth_flow_generation: 0,
        auth_flow_tx,
        auth_flow_rx,
        offline_account_uids: HashSet::new(),
        invalid_xiaomi_account_uids: HashSet::new(),
        invalid_mijia_account_uids: HashSet::new(),
        account_check_in_flight: false,
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

    app.start_bootstrap();

    assert!(matches!(app.boot_state, BootState::Ready));
    assert!(app.bootstrap_pending.is_none());
    assert_eq!(app.local_transport_refresh_generation, 6);
    assert!(!app.local_transport_force_refresh_pending);
    assert!(app.logs.iter().any(|line| line.contains("不存在可用账号")));
}

#[test]
fn request_local_transport_refresh_skips_account_after_session_warm() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-refresh-skip",
        "deviceId": "mico.tui-refresh-skip",
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
        local_transport_fetching: false,
        local_transport_refresh_generation: 0,
        local_transport_refresh_device_id: Some(account.device_id.clone()),
        local_transport_force_refresh_pending: false,
        local_transport_tx,
        local_transport_rx,
        auth_flow_generation: 0,
        auth_flow_tx,
        auth_flow_rx,
        offline_account_uids: HashSet::new(),
        invalid_xiaomi_account_uids: HashSet::new(),
        invalid_mijia_account_uids: HashSet::new(),
        account_check_in_flight: false,
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
        update_check_rx: None,
        update_check_status: None,
        install_rx: None,
    };

    app.request_local_transport_refresh(false);

    assert!(!app.local_transport_fetching);
    assert_eq!(
        app.local_transport_refresh_device_id.as_deref(),
        Some(account.device_id.as_str())
    );
}

#[test]
fn request_local_transport_refresh_force_rewarms_same_account() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-refresh-force",
        "deviceId": "mico.tui-refresh-force",
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
        local_transport_fetching: false,
        local_transport_refresh_generation: 0,
        local_transport_refresh_device_id: Some(account.device_id.clone()),
        local_transport_force_refresh_pending: false,
        local_transport_tx,
        local_transport_rx,
        auth_flow_generation: 0,
        auth_flow_tx,
        auth_flow_rx,
        offline_account_uids: HashSet::new(),
        invalid_xiaomi_account_uids: HashSet::new(),
        invalid_mijia_account_uids: HashSet::new(),
        account_check_in_flight: false,
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
        update_check_rx: None,
        update_check_status: None,
        install_rx: None,
    };

    app.request_local_transport_refresh(true);

    assert!(app.local_transport_fetching);
    assert_eq!(
        app.local_transport_refresh_device_id.as_deref(),
        Some(account.device_id.as_str())
    );
}

#[test]
fn request_local_transport_refresh_queues_on_account_switch_while_fetching() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let account_a = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-refresh-a",
        "deviceId": "mico.tui-refresh-a",
        "state": "state-a",
        "accessToken": "",
        "refreshToken": "",
        "expiresTs": 0,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));
    let account_b = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-refresh-b",
        "deviceId": "mico.tui-refresh-b",
        "state": "state-b",
        "accessToken": "",
        "refreshToken": "",
        "expiresTs": 0,
        "user": {"uid": "1002", "nickname": "账号B", "icon": "", "unionId": "union-b"}
    }));
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![account_a.clone(), account_b.clone()],
        account_index: 1,
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
        local_transport_refresh_device_id: Some(account_a.device_id.clone()),
        local_transport_force_refresh_pending: false,
        local_transport_tx: local_transport_tx.clone(),
        local_transport_rx,
        auth_flow_generation: 0,
        auth_flow_tx,
        auth_flow_rx,
        offline_account_uids: HashSet::new(),
        invalid_xiaomi_account_uids: HashSet::new(),
        invalid_mijia_account_uids: HashSet::new(),
        account_check_in_flight: false,
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
        update_check_rx: None,
        update_check_status: None,
        install_rx: None,
    };

    app.request_local_transport_refresh(false);
    assert!(app.local_transport_force_refresh_pending);
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
    assert_eq!(
        app.local_transport_refresh_device_id.as_deref(),
        Some(account_b.device_id.as_str())
    );
}

#[test]
fn process_bootstrap_message_marks_app_ready_after_success() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();

    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-bootstrap",
        "deviceId": "mico.tui-bootstrap",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 1,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));
    let mut auth_state = default_auth();
    auth_state.accounts = vec![account.clone()];

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
        invalid_xiaomi_account_uids: HashSet::new(),
        invalid_mijia_account_uids: HashSet::new(),
        account_check_in_flight: false,
        boot_state: BootState::Loading,
        boot_spinner_index: 0,
        bootstrap_generation: 1,
        bootstrap_pending: Some(BootstrapPending {
            generation: 1,
            uid: "1001".to_string(),
            refresh_local_transport_if_missing: false,
        }),
        bootstrap_tx: bootstrap_tx.clone(),
        bootstrap_rx,
        property_cache: Arc::new(PropertyCache::new()),
        language: Language::Chinese,
        auto_subscribe_device_status: true,
        settings_selected: 0,
        update_check_rx: None,
        update_check_status: None,
        install_rx: None,
    };

    bootstrap_tx
        .send(BootstrapMessage::Ready {
            generation: 1,
            uid: "1001".to_string(),
            offline_uids: Vec::new(),
            auth_state,
            accounts: vec![account.clone()],
            devices: vec![Device {
                did: "dev-1".to_string(),
                name: "living-room".to_string(),
                model: "xiaomi.wifispeaker.lx04".to_string(),
                online: true,
                pid: 0,

                home_id: "home-1".to_string(),
                home_name: "我家".to_string(),
                room_id: "room-1".to_string(),
                room_name: "客厅".to_string(),
            }],
            logs: vec!["loaded 1 devices".to_string()],
        })
        .unwrap();

    app.process_background_messages();

    assert_eq!(app.boot_state, BootState::Ready);
    assert_eq!(app.devices.len(), 1);
    assert_eq!(app.accounts.len(), 1);
    assert_eq!(app.accounts[0].user.uid, "1001");
    assert!(!app.local_transport_fetching);
    assert_eq!(app.local_transport_refresh_generation, 0);
    assert!(app.local_transport_refresh_device_id.is_none());
}

#[test]
fn process_bootstrap_message_ignores_stale_results_when_not_pending() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();

    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: Vec::new(),
        account_index: 0,
        devices: vec![Device {
            did: "dev-new".to_string(),
            name: "new".to_string(),
            model: "new.model".to_string(),
            online: true,
            pid: 0,

            home_id: "home-1".to_string(),
            home_name: "我家".to_string(),
            room_id: "room-1".to_string(),
            room_name: "客厅".to_string(),
        }],
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
        invalid_xiaomi_account_uids: HashSet::new(),
        invalid_mijia_account_uids: HashSet::new(),
        account_check_in_flight: false,
        boot_state: BootState::Ready,
        boot_spinner_index: 0,
        bootstrap_generation: 2,
        bootstrap_pending: None,
        bootstrap_tx: bootstrap_tx.clone(),
        bootstrap_rx,
        property_cache: Arc::new(PropertyCache::new()),
        language: Language::Chinese,
        auto_subscribe_device_status: true,
        settings_selected: 0,
        update_check_rx: None,
        update_check_status: None,
        install_rx: None,
    };

    bootstrap_tx
        .send(BootstrapMessage::Ready {
            generation: 1,
            uid: "1001".to_string(),
            offline_uids: Vec::new(),
            auth_state: default_auth(),
            accounts: Vec::new(),
            devices: vec![Device {
                did: "dev-stale".to_string(),
                name: "stale".to_string(),
                model: "stale.model".to_string(),
                online: true,
                pid: 0,

                home_id: "home-1".to_string(),
                home_name: "我家".to_string(),
                room_id: "room-1".to_string(),
                room_name: "客厅".to_string(),
            }],
            logs: vec!["stale".to_string()],
        })
        .unwrap();

    app.process_background_messages();

    assert_eq!(app.devices[0].did, "dev-new");
    assert_eq!(app.boot_state, BootState::Ready);
}

#[test]
fn process_bootstrap_message_ignores_stale_results_for_wrong_generation() {
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
        invalid_xiaomi_account_uids: HashSet::new(),
        invalid_mijia_account_uids: HashSet::new(),
        account_check_in_flight: false,
        boot_state: BootState::Loading,
        boot_spinner_index: 0,
        bootstrap_generation: 2,
        bootstrap_pending: Some(BootstrapPending {
            generation: 2,
            uid: "1001".to_string(),
            refresh_local_transport_if_missing: false,
        }),
        bootstrap_tx: bootstrap_tx.clone(),
        bootstrap_rx,
        property_cache: Arc::new(PropertyCache::new()),
        language: Language::Chinese,
        auto_subscribe_device_status: true,
        settings_selected: 0,
        update_check_rx: None,
        update_check_status: None,
        install_rx: None,
    };

    bootstrap_tx
        .send(BootstrapMessage::Failed {
            generation: 1,
            uid: "1001".to_string(),
            auth_state: None,
            accounts: None,
            error: "stale".to_string(),
        })
        .unwrap();

    app.process_background_messages();

    assert_eq!(app.boot_state, BootState::Loading);
    assert!(app.logs.is_empty());
}

#[test]
fn process_bootstrap_message_rewarms_local_transport_when_snapshot_missing() {
    let home = make_temp_dir("tui-bootstrap-missing-local-credentials");
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();

    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-bootstrap-refresh",
        "deviceId": "mico.tui-bootstrap-refresh",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 1,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));
    let mut auth_state = default_auth();
    auth_state.accounts = vec![account.clone()];

    let mut app = TuiApp {
        home_dir: home.clone(),
        auth_state: default_auth(),
        accounts: vec![account.clone()],
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
        local_transport_refresh_generation: 7,
        local_transport_refresh_device_id: Some(account.device_id.clone()),
        local_transport_force_refresh_pending: false,
        local_transport_tx,
        local_transport_rx,
        auth_flow_generation: 0,
        auth_flow_tx,
        auth_flow_rx,
        offline_account_uids: HashSet::new(),
        invalid_xiaomi_account_uids: HashSet::new(),
        invalid_mijia_account_uids: HashSet::new(),
        account_check_in_flight: false,
        boot_state: BootState::Ready,
        boot_spinner_index: 0,
        bootstrap_generation: 1,
        bootstrap_pending: Some(BootstrapPending {
            generation: 1,
            uid: "1001".to_string(),
            refresh_local_transport_if_missing: true,
        }),
        bootstrap_tx: bootstrap_tx.clone(),
        bootstrap_rx,
        property_cache: Arc::new(PropertyCache::new()),
        language: Language::Chinese,
        auto_subscribe_device_status: true,
        settings_selected: 0,
        update_check_rx: None,
        update_check_status: None,
        install_rx: None,
    };

    bootstrap_tx
        .send(BootstrapMessage::Ready {
            generation: 1,
            uid: "1001".to_string(),
            offline_uids: Vec::new(),
            auth_state,
            accounts: vec![account],
            devices: vec![Device {
                did: "dev-1".to_string(),
                name: "living-room".to_string(),
                model: "xiaomi.wifispeaker.lx04".to_string(),
                online: true,
                pid: 0,
                home_id: "home-1".to_string(),
                home_name: "我家".to_string(),
                room_id: "room-1".to_string(),
                room_name: "客厅".to_string(),
            }],
            logs: vec!["loaded 1 devices".to_string()],
        })
        .unwrap();

    app.process_background_messages();

    assert_eq!(app.local_transport_refresh_generation, 8);

    let _ = fs::remove_dir_all(&home);
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
        invalid_xiaomi_account_uids: HashSet::new(),
        invalid_mijia_account_uids: HashSet::new(),
        account_check_in_flight: false,
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
        update_check_rx: None,
        update_check_status: None,
        install_rx: None,
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
        invalid_xiaomi_account_uids: HashSet::new(),
        invalid_mijia_account_uids: HashSet::new(),
        account_check_in_flight: false,
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

#[test]
fn load_cached_devices_from_home_ignores_local_credentials_without_devices_cache() {
    let home = make_temp_dir("tui-cached-device-fallback");
    let mit_dir = home.join(".mit");
    fs::create_dir_all(&mit_dir).unwrap();
    fs::write(
        mit_dir.join("auth.json"),
        serde_json::to_string_pretty(&json!({
            "accounts": [
                persisted_auth_account_json("1001", "账号A", "union-a", "uuid-a", "device-a", "state-a", "token-a", "refresh-a", 1),
                persisted_auth_account_json("1002", "账号B", "union-b", "uuid-b", "device-b", "state-b", "token-b", "refresh-b", 1)
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    fs::create_dir_all(mit_dir.join("accounts").join("1001")).unwrap();
    fs::create_dir_all(mit_dir.join("accounts").join("1002")).unwrap();
    fs::write(
        mit_dir
            .join("accounts")
            .join("1001")
            .join("local_credentials.json"),
        serde_json::to_string_pretty(&json!({
            "dev-1001": {
                "did": "dev-1001",
                "name": "living-room",
                "model": "xiaomi.wifispeaker.lx04",
                "localIp": "192.168.1.11",
                "token": "00112233445566778899aabbccddeeff",
                "source": "direct"
            }
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        mit_dir
            .join("accounts")
            .join("1002")
            .join("local_credentials.json"),
        serde_json::to_string_pretty(&json!({
            "dev-1002": {
                "did": "dev-1002",
                "name": "bedroom",
                "model": "xiaomi.wifispeaker.lx04",
                "localIp": "192.168.1.12",
                "token": "ffeeddccbbaa99887766554433221100",
                "source": "direct"
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let devices = load_cached_devices_from_home(&home, "1001").unwrap();
    assert!(devices.is_empty());

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn load_cached_devices_from_home_does_not_fallback_to_config_json() {
    let home = make_temp_dir("tui-local-credentials-no-config-fallback");
    let mit_dir = home.join(".mit");
    fs::create_dir_all(&mit_dir).unwrap();
    fs::write(
        mit_dir.join("auth.json"),
        serde_json::to_string_pretty(&json!({
            "accounts": [
                persisted_auth_account_json("1001", "账号A", "union-a", "uuid-a", "device-a", "state-a", "token-a", "refresh-a", 1)
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    let devices = load_cached_devices_from_home(&home, "").unwrap();
    assert!(devices.is_empty());

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn start_bootstrap_uses_cached_devices_immediately_while_syncing_in_background() {
    let home = make_temp_dir("tui-bootstrap-immediate-cache");
    let mit_dir = home.join(".mit");
    fs::create_dir_all(&mit_dir).unwrap();
    fs::write(
        mit_dir.join("auth.json"),
        serde_json::to_string_pretty(&json!({
            "accounts": [
                persisted_auth_account_json("1001", "账号A", "union-a", "uuid-a", "device-a", "state-a", "", "", 1)
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    fs::create_dir_all(mit_dir.join("accounts").join("1001")).unwrap();
    fs::write(
        mit_dir.join("accounts").join("1001").join("devices.json"),
        serde_json::to_string_pretty(&json!({
            "devices": [
                {
                    "did": "dev-cache-1",
                    "name": "cached-speaker",
                    "model": "xiaomi.wifispeaker.lx04",
                    "online": false,
                    "homeId": "cache-account:1001",
                    "homeName": "账号A(1001)",
                    "roomId": "",
                    "roomName": ""
                }
            ],
            "categories": {
                "xiaomi.wifispeaker.lx04": "音箱"
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let broken_account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "",
        "deviceId": "",
        "state": "",
        "accessToken": "",
        "refreshToken": "",
        "expiresTs": 0,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));
    let mut app = TuiApp {
        home_dir: home.clone(),
        auth_state: default_auth(),
        accounts: vec![broken_account],
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
        invalid_xiaomi_account_uids: HashSet::new(),
        invalid_mijia_account_uids: HashSet::new(),
        account_check_in_flight: false,
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

    app.start_bootstrap();

    assert!(matches!(app.boot_state, BootState::Ready));
    assert_eq!(app.devices.len(), 1);
    assert_eq!(app.devices[0].did, "dev-cache-1");
    assert!(app
        .logs
        .iter()
        .any(|line| line.contains("cached devices while syncing in background")));
    assert!(app.bootstrap_pending.is_some());

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn process_bootstrap_message_preserves_selected_device_did_when_present() {
    let home = make_temp_dir("tui-sync-preserve-selection");
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();

    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-bootstrap-preserve",
        "deviceId": "mico.tui-bootstrap-preserve",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 1,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));
    let mut auth_state = default_auth();
    auth_state.accounts = vec![account.clone()];
    let devices = vec![
        Device {
            did: "dev-1".to_string(),
            name: "living-room".to_string(),
            model: "xiaomi.wifispeaker.lx04".to_string(),
            online: true,
            pid: 0,

            home_id: "home-1".to_string(),
            home_name: "我家".to_string(),
            room_id: "room-1".to_string(),
            room_name: "a-room".to_string(),
        },
        Device {
            did: "dev-2".to_string(),
            name: "bedroom".to_string(),
            model: "xiaomi.gateway.hub1".to_string(),
            online: true,
            pid: 0,

            home_id: "home-1".to_string(),
            home_name: "我家".to_string(),
            room_id: "room-2".to_string(),
            room_name: "b-room".to_string(),
        },
    ];

    let mut app = TuiApp {
        home_dir: home.clone(),
        auth_state: default_auth(),
        accounts: vec![account.clone()],
        account_index: 0,
        devices: devices.clone(),
        device_index: 1,
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
        invalid_xiaomi_account_uids: HashSet::new(),
        invalid_mijia_account_uids: HashSet::new(),
        account_check_in_flight: false,
        boot_state: BootState::Ready,
        boot_spinner_index: 0,
        bootstrap_generation: 2,
        bootstrap_pending: Some(BootstrapPending {
            generation: 2,
            uid: "1001".to_string(),
            refresh_local_transport_if_missing: false,
        }),
        bootstrap_tx: bootstrap_tx.clone(),
        bootstrap_rx,
        property_cache: Arc::new(PropertyCache::new()),
        language: Language::Chinese,
        auto_subscribe_device_status: true,
        settings_selected: 0,
        update_check_rx: None,
        update_check_status: None,
        install_rx: None,
    };

    bootstrap_tx
        .send(BootstrapMessage::Ready {
            generation: 2,
            uid: "1001".to_string(),
            offline_uids: Vec::new(),
            auth_state,
            accounts: vec![account],
            devices,
            logs: vec!["loaded 2 devices".to_string()],
        })
        .unwrap();

    app.process_background_messages();

    assert_eq!(app.devices[app.device_index].did, "dev-2");

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn process_background_messages_clears_local_transport_refresh_device_id_on_error() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-refresh-retry",
        "deviceId": "mico.tui-refresh-retry",
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
        invalid_xiaomi_account_uids: HashSet::new(),
        invalid_mijia_account_uids: HashSet::new(),
        account_check_in_flight: false,
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
        update_check_rx: None,
        update_check_status: None,
        install_rx: None,
    };

    local_transport_tx
        .send(LocalTransportRefreshMessage {
            generation: 7,
            error: Some("transient network error".to_string()),
        })
        .unwrap();

    app.process_background_messages();

    assert!(!app.local_transport_fetching);
    assert_eq!(app.local_transport_refresh_device_id, None);
    assert!(app
        .logs
        .iter()
        .any(|line| line.contains("transient network error")));
}

#[test]
fn sync_failure_uses_cached_devices_without_quitting() {
    let home = make_temp_dir("tui-sync-failure-cache-recovery");
    let mit_dir = home.join(".mit");
    fs::create_dir_all(&mit_dir).unwrap();
    fs::write(
        mit_dir.join("auth.json"),
        serde_json::to_string_pretty(&json!({
            "accounts": [
                persisted_auth_account_json("1001", "账号A", "union-a", "uuid-a", "device-a", "state-a", "token-a", "refresh-a", 1)
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    fs::create_dir_all(mit_dir.join("accounts").join("1001")).unwrap();
    fs::write(
        mit_dir.join("accounts").join("1001").join("devices.json"),
        serde_json::to_string_pretty(&json!({
            "devices": [
                {
                    "did": "dev-cache-1",
                    "name": "cached-speaker",
                    "model": "xiaomi.wifispeaker.lx04",
                    "online": false,
                    "homeId": "cache-account:1001",
                    "homeName": "账号A(1001)",
                    "roomId": "",
                    "roomName": ""
                }
            ],
            "categories": {
                "xiaomi.wifispeaker.lx04": "音箱"
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let broken_account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "",
        "deviceId": "",
        "state": "",
        "accessToken": "",
        "refreshToken": "",
        "expiresTs": 0,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));
    let mut app = TuiApp {
        home_dir: home.clone(),
        auth_state: default_auth(),
        accounts: vec![broken_account],
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
        invalid_xiaomi_account_uids: HashSet::new(),
        invalid_mijia_account_uids: HashSet::new(),
        account_check_in_flight: false,
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

    app.start_manual_sync();
    assert!(matches!(app.boot_state, BootState::Ready));
    assert!(app.bootstrap_pending.is_some());

    let deadline = Instant::now() + Duration::from_secs(2);
    while app.devices.is_empty() && Instant::now() < deadline {
        app.process_background_messages();
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(app.devices.len(), 1);
    assert_eq!(app.devices[0].did, "dev-cache-1");

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn sync_downloads_missing_specs_and_enriches_cached_devices_file() {
    let _guard = env_guard();
    let server = MockMicoServer::start();
    let home = make_temp_dir("tui-sync-enriches-cached-devices");
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
        invalid_xiaomi_account_uids: HashSet::new(),
        invalid_mijia_account_uids: HashSet::new(),
        account_check_in_flight: false,
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

    app.start_manual_sync();

    let devices_path = home
        .join(".mit")
        .join("accounts")
        .join("1001")
        .join("devices.json");
    let deadline = Instant::now() + Duration::from_secs(8);
    let cached_payload = loop {
        app.process_background_messages();
        if devices_path.exists() {
            if let Some(payload) = fs::read_to_string(&devices_path)
                .ok()
                .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            {
                if payload
                    .get("categories")
                    .and_then(|categories| categories.get("xiaomi.wifispeaker.lx04"))
                    .and_then(Value::as_str)
                    == Some("音箱")
                {
                    break payload;
                }
            }
        }
        assert!(
            Instant::now() < deadline,
            "logs={:?} requests={:?}",
            app.logs,
            server.requests()
        );
        thread::sleep(Duration::from_millis(10));
    };

    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        app.process_background_messages();
        if app.bootstrap_pending.is_none()
            && app
                .devices
                .iter()
                .any(|device| device.model == "xiaomi.wifispeaker.lx04")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "logs={:?} requests={:?} pending={:?} devices={:?}",
            app.logs,
            server.requests(),
            app.bootstrap_pending,
            app.devices
        );
        thread::sleep(Duration::from_millis(10));
    }

    assert_eq!(
        cached_payload
            .get("devices")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(4)
    );
    assert_eq!(
        cached_payload
            .get("categories")
            .and_then(|categories| categories.get("xiaomi.wifispeaker.lx04"))
            .and_then(Value::as_str),
        Some("音箱")
    );
    assert!(server.requests().iter().any(|request| {
        request.method == "GET" && request.path == "/miot-spec-v2/template/list/device"
    }));

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("音箱") || text.contains("音 箱"), "{text}");

    std::env::remove_var("MIT_HOME");
    std::env::remove_var("MIT_PROFILE_DIR");
    std::env::remove_var("MIT_MICO_BASE_URL");
    std::env::remove_var("MIT_USER_PROFILE_URL");
    std::env::remove_var("MIT_MIOT_SPEC_URL_BASE");
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn clicking_refresh_operation_in_footer_triggers_sync() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![test_account()],
        account_index: 0,
        devices: vec![Device {
            did: "dev-1".to_string(),
            name: "living-room".to_string(),
            model: "xiaomi.wifispeaker.lx04".to_string(),
            online: true,
            pid: 0,
            home_id: "cache-account:1001".to_string(),
            home_name: "账号A(1001)".to_string(),
            room_id: "room-1".to_string(),
            room_name: "客厅".to_string(),
        }],
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
        invalid_xiaomi_account_uids: HashSet::new(),
        invalid_mijia_account_uids: HashSet::new(),
        account_check_in_flight: false,
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

    let terminal_area = ratatui::layout::Rect::new(0, 0, 100, 24);
    let (column, row) = footer_click_point(&app, terminal_area, "R: 刷新");

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        terminal_area,
    )
    .unwrap();

    assert!(app.bootstrap_pending.is_some());
}
