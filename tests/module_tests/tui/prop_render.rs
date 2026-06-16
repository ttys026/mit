// Auto-split: shares the `tests` module scope (imports + helpers) of
// mod.rs via include!; do not add `use` here.

#[test]
fn prop_dialog_footer_refresh_shows_props_loading_effect() {
    let mut app = test_app_with_prop_dialog(PropDialog {
        device_did: "dev-1".to_string(),
        device_name: "dev-1".to_string(),
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
    app.language = Language::English;
    let terminal_area = ratatui::layout::Rect::new(0, 0, 100, 24);
    let (column, row) = footer_click_point(&app, terminal_area, "R: Refresh");

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

    let dialog = app.prop_dialog.as_ref().unwrap();
    assert!(dialog.refreshing);
    assert!(dialog.refresh_rx.is_some());

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("dev-1"), "{text}");
    assert!(text.contains("(Loading...)"), "{text}");
    assert!(
        terminal_has_dim_substring(&terminal, "Power"),
        "props row should dim while props are refreshing:\n{text}"
    );
}

#[test]
fn opening_prop_dialog_failure_shows_offline_instead_of_quitting() {
    let home = make_temp_dir("tui-bool-dialog-offline");
    let specs_dir = home.join(".mit").join("specs");
    fs::create_dir_all(&specs_dir).unwrap();
    fs::write(
        specs_dir.join("xiaomi.wifispeaker.lx04.json"),
        serde_json::to_string_pretty(&json!({
            "services": [
                {
                    "iid": 2,
                    "properties": [
                        {
                            "iid": 1,
                            "description": "Power",
                            "format": "bool",
                            "access": ["read", "write"]
                        }
                    ]
                }
            ]
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
        devices: vec![Device {
            did: "dev-offline".to_string(),
            name: "offline-speaker".to_string(),
            model: "xiaomi.wifispeaker.lx04".to_string(),
            online: false,

            home_id: "home-1".to_string(),
            home_name: "我家".to_string(),
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
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(!quit);

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("offline"));
    assert!(app.prop_dialog.is_some());

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn opening_prop_dialog_uses_device_account_instead_of_selected_account() {
    let home = make_temp_dir("tui-bool-dialog-device-account");
    let specs_dir = home.join(".mit").join("specs");
    fs::create_dir_all(&specs_dir).unwrap();
    fs::write(
        specs_dir.join("xiaomi.wifispeaker.lx04.json"),
        serde_json::to_string_pretty(&json!({
            "services": [
                {
                    "iid": 2,
                    "properties": [
                        {
                            "iid": 1,
                            "description": "Power",
                            "format": "bool",
                            "access": ["read", "write"]
                        }
                    ]
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let account_a = normalize_account(json!({
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
    let account_b = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "",
        "deviceId": "",
        "state": "",
        "accessToken": "",
        "refreshToken": "",
        "expiresTs": 0,
        "user": {"uid": "1002", "nickname": "账号B", "icon": "", "unionId": "union-b"}
    }));
    let mut app = TuiApp {
        home_dir: home.clone(),
        auth_state: default_auth(),
        accounts: vec![account_a, account_b],
        account_index: 1,
        devices: vec![Device {
            did: "dev-account-a".to_string(),
            name: "speaker-a".to_string(),
            model: "xiaomi.wifispeaker.lx04".to_string(),
            online: false,

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
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(!quit);

    let account_uid = app
        .prop_dialog
        .as_ref()
        .map(|dialog| dialog.account_uid.as_str())
        .unwrap_or_default();
    assert_eq!(account_uid, "1001");

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn devices_tab_enter_opens_prop_dialog_and_p_does_not() {
    let home = make_temp_dir("tui-devices-enter-open");
    let specs_dir = home.join(".mit").join("specs");
    fs::create_dir_all(&specs_dir).unwrap();
    fs::write(
        specs_dir.join("xiaomi.wifispeaker.lx04.json"),
        serde_json::to_string_pretty(&json!({
            "services": [
                {
                    "iid": 2,
                    "properties": [
                        {
                            "iid": 1,
                            "description": "Power",
                            "format": "bool",
                            "access": ["read", "write"]
                        }
                    ]
                }
            ]
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
        devices: vec![Device {
            did: "dev-offline".to_string(),
            name: "offline-speaker".to_string(),
            model: "xiaomi.wifispeaker.lx04".to_string(),
            online: false,

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
        crossterm::event::KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE),
    )
    .unwrap();
    assert!(!quit);
    assert!(app.prop_dialog.is_none());

    let quit = handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(!quit);
    assert!(app.prop_dialog.is_some());

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn prop_dialog_does_not_force_black_popup_background() {
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
        logs: VecDeque::from(vec!["visible beneath".to_string()]),
        active_tab: 1,
        log_scroll_offset: 0,
        input_mode: false,
        input: String::new(),
        device_search_cursor: 0,
        search_inputs: Default::default(),
        search_cursors: [0; 3],
        prop_dialog: Some(PropDialog {
            device_did: "dev-1".to_string(),
            device_name: "dev-1".to_string(),
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
            editing: false,
            edit_buffer: String::new(),
            edit_cursor: 0,
            edit_error: None,
            loading_rx: None,
            status: None,
            refreshing: false,
            refresh_rx: None,
            statistics_selected_bar: None,
        }),
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
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    let cell = &terminal.backend().buffer()[(0, 0)];
    assert_ne!(cell.bg, Color::Black);
}

#[test]
fn prop_dialog_refresh_starts_background_worker() {
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
        prop_dialog: Some(PropDialog {
            device_did: "dev-1".to_string(),
            device_name: "dev-1".to_string(),
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
        }),
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

    app.request_prop_dialog_refresh();

    let dialog = app.prop_dialog.as_ref().unwrap();
    assert!(dialog.refreshing);
    assert!(dialog.refresh_rx.is_some());
}

#[test]
fn prop_dialog_r_key_starts_background_refresh() {
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
        prop_dialog: Some(PropDialog {
            device_did: "dev-1".to_string(),
            device_name: "dev-1".to_string(),
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
        }),
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

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
    )
    .unwrap();

    let dialog = app.prop_dialog.as_ref().unwrap();
    assert!(dialog.refreshing);
    assert!(dialog.refresh_rx.is_some());
}

#[test]
fn process_prop_dialog_loading_handles_refresh_when_not_loading() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let (_refresh_tx, refresh_rx) = mpsc::channel::<super::PropDialogRefreshMessage>();
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
        prop_dialog: Some(PropDialog {
            device_did: "dev-1".to_string(),
            device_name: "dev-1".to_string(),
            account_uid: "1001".to_string(),
            items: vec![
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
                raw_device_logs_item(json!({"records": []})),
            ],
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
            refreshing: true,
            refresh_rx: Some(refresh_rx),
            statistics_selected_bar: None,
        }),
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

    // Replace channel with one that already has a completed refresh payload.
    let (tx, rx) = mpsc::channel::<super::PropDialogRefreshMessage>();
    tx.send(super::PropDialogRefreshMessage::Props(vec![(
        0,
        Value::Bool(false),
    )]))
    .unwrap();
    tx.send(super::PropDialogRefreshMessage::Raw(vec![(
        1,
        json!({"records": [{"event": "updated"}]}),
    )]))
    .unwrap();
    tx.send(super::PropDialogRefreshMessage::Finished).unwrap();
    if let Some(dialog) = app.prop_dialog.as_mut() {
        dialog.refresh_rx = Some(rx);
    }

    app.process_prop_dialog_loading();

    let dialog = app.prop_dialog.as_ref().unwrap();
    assert!(!dialog.refreshing);
    assert!(dialog.refresh_rx.is_none());
    assert_eq!(dialog.items[0].value, Value::Bool(false));
    assert_eq!(dialog.items[1].value["records"][0]["event"], "updated");
}

#[test]
fn process_prop_dialog_loading_preserves_selected_index_after_load() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let (tx, rx) = mpsc::channel::<std::result::Result<Vec<ToggleItem>, String>>();
    tx.send(Ok(vec![
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
        ToggleItem {
            prop: PropItem {
                siid: 2,
                piid: 2,
                name: "Switch".to_string(),
                format: "bool".to_string(),
                writable: true,
                value_options: Vec::new(),
            },
            value: Value::Bool(false),
        },
    ]))
    .unwrap();

    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
        prop_dialog: Some(PropDialog {
            device_did: "dev-1".to_string(),
            device_name: "dev-1".to_string(),
            account_uid: "1001".to_string(),
            items: vec![
                ToggleItem {
                    prop: PropItem {
                        siid: 2,
                        piid: 1,
                        name: "Power".to_string(),
                        format: "bool".to_string(),
                        writable: true,
                        value_options: Vec::new(),
                    },
                    value: Value::Null,
                },
                ToggleItem {
                    prop: PropItem {
                        siid: 2,
                        piid: 2,
                        name: "Switch".to_string(),
                        format: "bool".to_string(),
                        writable: true,
                        value_options: Vec::new(),
                    },
                    value: Value::Null,
                },
            ],
            selected: 1,
            active_tab: PropDialogTab::Writable,
            writable_selected: 1,
            readonly_selected: 0,
            actions: Vec::new(),
            actions_selected: 0,
            writable_list_state: ListState::default(),
            readonly_list_state: ListState::default(),
            actions_list_state: ListState::default(),
            loading: true,
            loading_rx: Some(rx),
            status: None,
            editing: false,
            edit_buffer: String::new(),
            edit_cursor: 0,
            edit_error: None,
            refreshing: false,
            refresh_rx: None,
            statistics_selected_bar: None,
        }),
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

    app.process_prop_dialog_loading();

    let dialog = app.prop_dialog.as_ref().unwrap();
    assert!(!dialog.loading);
    assert_eq!(dialog.selected, 1);
    assert_eq!(dialog.writable_selected, 1);
}

#[test]
fn prop_dialog_is_fullscreen_and_hides_schema_identifiers() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
        prop_dialog: Some(PropDialog {
            device_did: "客厅音箱".to_string(),
            device_name: "客厅音箱".to_string(),
            account_uid: "1001".to_string(),
            items: vec![ToggleItem {
                prop: PropItem {
                    siid: 2,
                    piid: 1,
                    name: "扬声器服务 / 电源".to_string(),
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
        }),
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
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");
    let top_left = &terminal.backend().buffer()[(0, 0)];
    assert_ne!(top_left.symbol(), "┌");
    assert!(compact.contains("客厅音箱"), "{text}");
    assert!(!compact.contains("R刷新"), "{text}");
    assert!(!compact.contains("Esc关闭"), "{text}");
    assert!(compact.contains("修改参数"), "{text}");
    assert!(!compact.contains("快捷操作"), "{text}");
    assert!(!text.contains("Writable"), "{text}");
    assert!(!text.contains("Read-only"), "{text}");
    assert!(!text.contains("(2/1)"), "{text}");
}

#[test]
fn prop_dialog_tab_switch_shows_active_subtab_only() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
        prop_dialog: Some(PropDialog {
            device_did: "dev-1".to_string(),
            device_name: "dev-1".to_string(),
            account_uid: "1001".to_string(),
            items: vec![
                ToggleItem {
                    prop: PropItem {
                        siid: 2,
                        piid: 1,
                        name: "writable-power".to_string(),
                        format: "bool".to_string(),
                        writable: true,
                        value_options: Vec::new(),
                    },
                    value: Value::Bool(true),
                },
                ToggleItem {
                    prop: PropItem {
                        siid: 2,
                        piid: 2,
                        name: "readonly-volume".to_string(),
                        format: "uint8".to_string(),
                        writable: false,
                        value_options: Vec::new(),
                    },
                    value: json!(30),
                },
            ],
            selected: 0,
            active_tab: PropDialogTab::Writable,
            writable_selected: 0,
            readonly_selected: 1,
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
        }),
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
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let before = terminal_text(&terminal);
    assert!(before.contains("writable-power"), "{before}");

    let quit = handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(!quit);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let after = terminal_text(&terminal);
    assert!(!after.contains("writable-power"), "{after}");
    assert!(after.contains("readonly-volume"), "{after}");
}

#[test]
fn dialog_subtab_click_bounds_handle_wide_char_titles() {
    let area = ratatui::layout::Rect::new(1, 1, 40, 3);
    let titles = super::all_prop_dialog_tab_titles(Language::Chinese);
    // "操作" uses wide glyphs and still occupies this column in the first tab.
    assert_eq!(tab_index_for_column_with_titles(7, area, &titles), Some(0));
}

#[test]
fn prop_dialog_visible_tabs_hide_empty_categories_and_keep_order() {
    let dialog = PropDialog {
        device_did: "dev-1".to_string(),
        device_name: "dev-1".to_string(),
        account_uid: "1001".to_string(),
        items: vec![ToggleItem {
            prop: PropItem {
                siid: 2,
                piid: 1,
                name: "p1".to_string(),
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
        actions: vec![ActionItem {
            siid: 2,
            aiid: 1,
            name: "toggle".to_string(),
            input_piids: Vec::new(),
            input_labels: Vec::new(),
            input_props: Vec::new(),
        }],
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
    };

    assert_eq!(
        super::visible_prop_dialog_tabs(&dialog),
        vec![PropDialogTab::Actions, PropDialogTab::Writable]
    );
}

#[test]
fn prop_dialog_visible_tab_titles_are_renumbered_one_based() {
    let dialog = PropDialog {
        device_did: "dev-1".to_string(),
        device_name: "dev-1".to_string(),
        account_uid: "1001".to_string(),
        items: vec![
            ToggleItem {
                prop: PropItem {
                    siid: 2,
                    piid: 1,
                    name: "p1".to_string(),
                    format: "bool".to_string(),
                    writable: true,
                    value_options: Vec::new(),
                },
                value: Value::Bool(true),
            },
            ToggleItem {
                prop: PropItem {
                    siid: 2,
                    piid: 2,
                    name: "p2".to_string(),
                    format: "bool".to_string(),
                    writable: false,
                    value_options: Vec::new(),
                },
                value: Value::Bool(false),
            },
        ],
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
    };

    assert_eq!(
        super::visible_prop_dialog_tab_titles(&dialog, Language::Chinese),
        vec!["1:修改参数".to_string(), "2:只读属性".to_string()]
    );
}

#[test]
fn prop_dialog_visible_tab_titles_empty_when_no_actions_or_properties() {
    let dialog = PropDialog {
        device_did: "dev-1".to_string(),
        device_name: "dev-1".to_string(),
        account_uid: "1001".to_string(),
        items: Vec::new(),
        selected: 0,
        active_tab: PropDialogTab::Actions,
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
    };

    assert_eq!(
        super::visible_prop_dialog_tab_titles(&dialog, Language::Chinese),
        Vec::<String>::new()
    );
}

#[test]
fn prop_dialog_visible_tab_titles_actions_only_renumber_from_one() {
    let dialog = PropDialog {
        device_did: "dev-1".to_string(),
        device_name: "dev-1".to_string(),
        account_uid: "1001".to_string(),
        items: Vec::new(),
        selected: 0,
        active_tab: PropDialogTab::Actions,
        writable_selected: 0,
        readonly_selected: 0,
        actions: vec![ActionItem {
            siid: 2,
            aiid: 1,
            name: "toggle".to_string(),
            input_piids: Vec::new(),
            input_labels: Vec::new(),
            input_props: Vec::new(),
        }],
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
    };

    assert_eq!(
        super::visible_prop_dialog_tab_titles(&dialog, Language::Chinese),
        vec!["1:快捷操作".to_string()]
    );
}

#[test]
fn prop_dialog_visible_tab_titles_readonly_only_renumber_from_one() {
    let dialog = PropDialog {
        device_did: "dev-1".to_string(),
        device_name: "dev-1".to_string(),
        account_uid: "1001".to_string(),
        items: vec![
            ToggleItem {
                prop: PropItem {
                    siid: 2,
                    piid: 2,
                    name: "p2".to_string(),
                    format: "bool".to_string(),
                    writable: false,
                    value_options: Vec::new(),
                },
                value: Value::Bool(false),
            },
            raw_device_logs_item(json!({"result": []})),
            raw_device_statistics_item(json!({"result": []})),
        ],
        selected: 0,
        active_tab: PropDialogTab::ReadOnly,
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
    };

    assert_eq!(
        super::visible_prop_dialog_tab_titles(&dialog, Language::Chinese),
        vec![
            "1:只读属性".to_string(),
            "2:操作记录".to_string(),
            "3:统计".to_string(),
        ]
    );
}

#[test]
fn prop_dialog_mouse_tab_hit_testing_uses_visible_tabs_when_first_hidden() {
    let mut app = test_app_with_prop_dialog(PropDialog {
        device_did: "dev-1".to_string(),
        device_name: "dev-1".to_string(),
        account_uid: "1001".to_string(),
        items: vec![
            ToggleItem {
                prop: PropItem {
                    siid: 2,
                    piid: 1,
                    name: "writable-only-item".to_string(),
                    format: "bool".to_string(),
                    writable: true,
                    value_options: Vec::new(),
                },
                value: Value::Bool(true),
            },
            ToggleItem {
                prop: PropItem {
                    siid: 2,
                    piid: 2,
                    name: "readonly-only-item".to_string(),
                    format: "uint8".to_string(),
                    writable: false,
                    value_options: Vec::new(),
                },
                value: json!(7),
            },
        ],
        selected: 1,
        active_tab: PropDialogTab::ReadOnly,
        writable_selected: 0,
        readonly_selected: 1,
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

    let terminal_area = ratatui::layout::Rect::new(0, 0, 80, 24);
    let tabs_area = prop_dialog_tabs_area(terminal_area);
    let visible_titles =
        super::visible_prop_dialog_tab_titles(app.prop_dialog.as_ref().unwrap(), Language::Chinese);
    let first_visible_tab_column = tab_column_for_index(tabs_area, &visible_titles, 0);

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: first_visible_tab_column,
            row: tabs_area.y,
            modifiers: KeyModifiers::NONE,
        },
        terminal_area,
    )
    .unwrap();

    assert_eq!(
        app.prop_dialog.as_ref().map(|dialog| dialog.active_tab),
        Some(PropDialogTab::Writable)
    );

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("writable-only-item"), "{text}");
    assert!(!text.contains("readonly-only-item"), "{text}");
}

#[test]
fn prop_dialog_keyboard_tab_cycles_over_visible_tabs_when_middle_hidden() {
    let mut app = test_app_with_prop_dialog(PropDialog {
        device_did: "dev-1".to_string(),
        device_name: "dev-1".to_string(),
        account_uid: "1001".to_string(),
        items: vec![ToggleItem {
            prop: PropItem {
                siid: 2,
                piid: 2,
                name: "readonly-only-item".to_string(),
                format: "uint8".to_string(),
                writable: false,
                value_options: Vec::new(),
            },
            value: json!(9),
        }],
        selected: 0,
        active_tab: PropDialogTab::Actions,
        writable_selected: 0,
        readonly_selected: 0,
        actions: vec![ActionItem {
            siid: 2,
            aiid: 1,
            name: "action-only-item".to_string(),
            input_piids: Vec::new(),
            input_labels: Vec::new(),
            input_props: Vec::new(),
        }],
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

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
    )
    .unwrap();
    assert_eq!(
        app.prop_dialog.as_ref().map(|dialog| dialog.active_tab),
        Some(PropDialogTab::ReadOnly)
    );

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("readonly-only-item"), "{text}");
    assert!(!text.contains("action-only-item"), "{text}");
}

#[test]
fn prop_dialog_hidden_active_tab_is_normalized_before_render() {
    let mut app = test_app_with_prop_dialog(PropDialog {
        device_did: "dev-1".to_string(),
        device_name: "dev-1".to_string(),
        account_uid: "1001".to_string(),
        items: vec![ToggleItem {
            prop: PropItem {
                siid: 2,
                piid: 2,
                name: "readonly-only-item".to_string(),
                format: "uint8".to_string(),
                writable: false,
                value_options: Vec::new(),
            },
            value: json!(11),
        }],
        selected: 0,
        active_tab: PropDialogTab::Writable,
        writable_selected: 0,
        readonly_selected: 0,
        actions: vec![ActionItem {
            siid: 2,
            aiid: 1,
            name: "action-only-item".to_string(),
            input_piids: Vec::new(),
            input_labels: Vec::new(),
            input_props: Vec::new(),
        }],
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

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    assert_eq!(
        app.prop_dialog.as_ref().map(|dialog| dialog.active_tab),
        Some(PropDialogTab::Actions)
    );

    let text = terminal_text(&terminal);
    assert!(text.contains("action-only-item"), "{text}");
    assert!(!text.contains("readonly-only-item"), "{text}");
}

#[test]
fn clicking_active_prop_dialog_item_executes_it() {
    let _guard = env_guard();
    let server = MockMicoServer::start();
    let home = make_temp_dir("tui-bool-dialog-click-active");
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
    std::env::set_var("MIT_HOME", &home);
    std::env::set_var("MIT_PROFILE_DIR", &home);
    std::env::set_var("MIT_MICO_BASE_URL", server.base_url());
    std::env::set_var("MIT_USER_PROFILE_URL", server.user_profile_url());

    let mut app = TuiApp {
        home_dir: home.clone(),
        auth_state: AuthState {
            accounts: vec![account.clone()],
            pending_auth: None,
        },
        accounts: vec![account],
        account_index: 0,
        devices: vec![Device {
            did: "dev-1".to_string(),
            name: "speaker".to_string(),
            model: "xiaomi.wifispeaker.lx04".to_string(),
            online: true,

            home_id: "home-1".to_string(),
            home_name: "我家".to_string(),
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
        prop_dialog: Some(PropDialog {
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
        }),
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

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 2,
            row: 4,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();
    assert_eq!(app.prop_dialog.as_ref().unwrap().selected, 0);
    assert!(app
        .prop_dialog
        .as_ref()
        .is_some_and(|dialog| dialog.editing));
    assert!(app
        .prop_dialog
        .as_ref()
        .is_some_and(|dialog| dialog.editing));
    assert_eq!(
        app.prop_dialog.as_ref().unwrap().items[0].value,
        Value::Bool(true)
    );

    std::env::remove_var("MIT_HOME");
    std::env::remove_var("MIT_PROFILE_DIR");
    std::env::remove_var("MIT_MICO_BASE_URL");
    std::env::remove_var("MIT_USER_PROFILE_URL");
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn prop_dialog_actions_tab_renders_action_items() {
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
        prop_dialog: Some(PropDialog {
            device_did: "dev-1".to_string(),
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: vec![ToggleItem {
                prop: PropItem {
                    siid: 2,
                    piid: 1,
                    name: "电源".to_string(),
                    format: "bool".to_string(),
                    writable: true,
                    value_options: Vec::new(),
                },
                value: Value::Bool(true),
            }],
            selected: 0,
            active_tab: PropDialogTab::Actions,
            writable_selected: 0,
            readonly_selected: 0,
            actions: vec![ActionItem {
                siid: 5,
                aiid: 1,
                name: "唤醒".to_string(),
                input_piids: Vec::new(),
                input_labels: Vec::new(),
                input_props: Vec::new(),
            }],
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
        }),
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

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");
    assert!(compact.contains("唤醒"), "{text}");
}

#[test]
fn preview_set_command_masks_long_params() {
    assert_eq!(
        format_preview_props_set_command("device-123", 2, 1, &json!(1234567)),
        "mit props set device-123 2 1 \"...\""
    );
}

#[test]
fn draw_does_not_render_command_bar() {
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

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(!text.contains("':' command"));
    assert!(!text.contains("Command (Enter run, Esc cancel)"));
}

#[test]
fn draw_readonly_prop_detail_shows_current_value_and_get_command() {
    let mut app = app_with_single_readonly_prop_dialog();
    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");

    assert!(compact.contains("当前值:22"), "{text}");
    assert!(
        compact.contains("CLI命令(读取):mitpropsgetdev-122"),
        "{text}"
    );
}

#[test]
fn handle_key_opens_and_closes_readonly_prop_detail() {
    let mut app = app_with_single_readonly_prop_dialog();

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(
        app.logs.iter().all(|line| !line.contains("当前属性为只读")),
        "{:?}",
        app.logs
    );

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(app.prop_dialog.is_some());
}

#[test]
fn readonly_tab_omits_type_marker_and_sorts_short_to_long() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
        prop_dialog: Some(PropDialog {
            device_did: "dev-1".to_string(),
            device_name: "dev-1".to_string(),
            account_uid: "1001".to_string(),
            items: vec![
                ToggleItem {
                    prop: PropItem {
                        siid: 2,
                        piid: 1,
                        name: "zz-long".to_string(),
                        format: "bool".to_string(),
                        writable: false,
                        value_options: Vec::new(),
                    },
                    value: Value::Bool(true),
                },
                ToggleItem {
                    prop: PropItem {
                        siid: 2,
                        piid: 2,
                        name: "mid".to_string(),
                        format: "uint8".to_string(),
                        writable: false,
                        value_options: Vec::new(),
                    },
                    value: json!(30),
                },
                ToggleItem {
                    prop: PropItem {
                        siid: 2,
                        piid: 3,
                        name: "s".to_string(),
                        format: "uint8".to_string(),
                        writable: false,
                        value_options: Vec::new(),
                    },
                    value: json!(1),
                },
            ],
            selected: 0,
            active_tab: PropDialogTab::ReadOnly,
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
        }),
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
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(!text.contains("uint8|ro"), "{text}");
    assert!(!text.contains("bool|ro"), "{text}");
    // Check that readonly properties are rendered without [ro] marker
    assert!(text.contains("s ="), "{text}"); // shortest name
    assert!(text.contains("mid ="), "{text}"); // medium name
    assert!(text.contains("zz-long ="), "{text}"); // longest name
    let compact = text.replace(' ', "");
    let p_s = compact.find("s=").unwrap_or(usize::MAX);
    let p_mid = compact.find("mid=").unwrap_or(usize::MAX);
    let p_long = compact.find("zz-long=").unwrap_or(usize::MAX);
    assert!(p_s < p_mid && p_mid < p_long, "{text}");
}

#[test]
fn mouse_scroll_moves_selection_inside_prop_dialog() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
        prop_dialog: Some(PropDialog {
            device_did: "dev-1".to_string(),
            device_name: "dev-1".to_string(),
            account_uid: "1001".to_string(),
            items: vec![
                ToggleItem {
                    prop: PropItem {
                        siid: 2,
                        piid: 1,
                        name: "p1".to_string(),
                        format: "bool".to_string(),
                        writable: true,
                        value_options: Vec::new(),
                    },
                    value: Value::Bool(true),
                },
                ToggleItem {
                    prop: PropItem {
                        siid: 2,
                        piid: 2,
                        name: "p2".to_string(),
                        format: "uint8".to_string(),
                        writable: true,
                        value_options: Vec::new(),
                    },
                    value: json!(20),
                },
            ],
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
        }),
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

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollDown,
            column: 2,
            row: 10,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();

    assert_eq!(app.prop_dialog.as_ref().unwrap().selected, 1);

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollUp,
            column: 2,
            row: 10,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();
    assert_eq!(app.prop_dialog.as_ref().unwrap().selected, 0);

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
    )
    .unwrap();
    assert_eq!(app.prop_dialog.as_ref().unwrap().selected, 1);

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
    )
    .unwrap();
    assert_eq!(app.prop_dialog.as_ref().unwrap().selected, 0);
}

#[test]
fn draw_edit_mode_shows_visible_input_cursor() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
        prop_dialog: Some(PropDialog {
            device_did: "dev-1".to_string(),
            device_name: "living-room".to_string(),
            account_uid: "1001".to_string(),
            items: vec![ToggleItem {
                prop: PropItem {
                    siid: 2,
                    piid: 1,
                    name: "power".to_string(),
                    format: "string".to_string(),
                    writable: true,
                    value_options: Vec::new(),
                },
                value: Value::String("true".to_string()),
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
        }),
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

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("true"), "{text}");
    assert!(!text.contains("true|"), "{text}");
    assert!(terminal_has_reversed_cell(&terminal));
}

#[test]
fn draw_edit_mode_wraps_long_input_across_two_lines() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let long_input =
        "{\"alpha\":\"one two three four five six seven eight nine ten eleven twelve\"}";
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
        prop_dialog: Some(PropDialog {
            device_did: "dev-1".to_string(),
            device_name: "living-room".to_string(),
            account_uid: "1001".to_string(),
            items: vec![ToggleItem {
                prop: PropItem {
                    siid: 2,
                    piid: 1,
                    name: "power".to_string(),
                    format: "string".to_string(),
                    writable: true,
                    value_options: Vec::new(),
                },
                value: Value::String("on".to_string()),
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
            edit_buffer: long_input.to_string(),
            edit_cursor: long_input.chars().count(),
            edit_error: None,
            refreshing: false,
            refresh_rx: None,
            statistics_selected_bar: None,
        }),
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

    let mut terminal = Terminal::new(TestBackend::new(72, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    let text = terminal_text(&terminal);
    let lines = text.lines().collect::<Vec<_>>();
    let input_line = lines
        .iter()
        .position(|line| line.replace(' ', "").contains("输入值:"))
        .expect("input label");
    let textarea_lines = &lines[input_line + 1..input_line + 3];
    assert!(
        textarea_lines
            .iter()
            .all(|line| !line.contains("│") && !line.contains("┌") && !line.contains("└")),
        "{text}"
    );
    assert!(!textarea_lines[0].trim().is_empty(), "{text}");
    assert!(!textarea_lines[1].trim().is_empty(), "{text}");
}

#[test]
fn footer_text_shows_readonly_detail_shortcut() {
    let app = app_with_single_readonly_prop_dialog();
    assert_eq!(
        super::footer_text(&app),
        "R: 刷新, Esc: 返回, Enter: 查看属性, 当前设备: dev-1"
    );
}
