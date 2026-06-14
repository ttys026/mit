// Auto-split from the former monolithic tui.rs. Shares the `tests` module
// scope (imports + helpers) of mod.rs via include!; do not add `use` here.

#[test]
fn preview_set_command_masks_long_params() {
    assert_eq!(
        format_preview_props_set_command("device-123", 2, 1, &json!(1234567)),
        "mit props set device-123 2 1 \"...\""
    );
}

#[test]
fn collect_readable_props_prefers_description_trans_copy() {
    let spec = json!({
        "services": [
            {
                "iid": 2,
                "properties": [
                    {
                        "iid": 1,
                        "description": "Power",
                        "description_trans": "电源",
                        "format": "bool",
                        "access": ["read", "write"]
                    }
                ]
            }
        ]
    });
    let props = collect_readable_props(&spec, Language::Chinese);
    assert_eq!(props.len(), 1);
    assert_eq!(props[0].name, "电源");
    assert!(props[0].writable);
}

#[test]
fn collect_readable_props_includes_read_only_and_combines_service_and_property_labels() {
    let spec = json!({
        "services": [
            {
                "iid": 2,
                "description": "Speaker Service",
                "description_trans": "扬声器服务",
                "properties": [
                    {
                        "iid": 1,
                        "description": "Power",
                        "description_trans": "电源",
                        "format": "bool",
                        "access": ["read", "write"]
                    },
                    {
                        "iid": 2,
                        "description": "ReadOnlyVolume",
                        "description_trans": "只读音量",
                        "format": "uint8",
                        "access": ["read"]
                    }
                ]
            }
        ]
    });

    let props = collect_readable_props(&spec, Language::Chinese);
    assert_eq!(props.len(), 2);
    assert_eq!(props[0].name, "扬声器服务 / 电源");
    assert_eq!(props[1].name, "扬声器服务 / 只读音量");
    assert!(props[0].writable);
    assert!(!props[1].writable);
}

#[test]
fn collect_readable_props_hides_properties_without_read_access() {
    let spec = json!({
        "services": [
            {
                "iid": 2,
                "properties": [
                    {
                        "iid": 1,
                        "description": "Readable",
                        "format": "bool",
                        "access": ["read", "write"]
                    },
                    {
                        "iid": 2,
                        "description": "WriteOnly",
                        "format": "bool",
                        "access": ["write"]
                    },
                    {
                        "iid": 3,
                        "description": "NoAccess",
                        "format": "bool",
                        "access": []
                    },
                    {
                        "iid": 4,
                        "description": "MissingAccess",
                        "format": "bool"
                    }
                ]
            }
        ]
    });

    let props = collect_readable_props(&spec, Language::Chinese);
    assert_eq!(props.len(), 1);
    assert_eq!(props[0].name, "Readable");
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
    };

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(!text.contains("':' command"));
    assert!(!text.contains("Command (Enter run, Esc cancel)"));
}

#[test]
fn handle_key_blocks_normal_actions_until_boot_ready() {
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

    let changed = handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(!changed);
    assert_eq!(app.active_tab, 0);

    let quit = handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
    )
    .unwrap();
    assert!(quit);
}

#[test]
fn mouse_click_is_ignored_while_prop_editing() {
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
                        name: "Brightness".to_string(),
                        format: "uint8".to_string(),
                        writable: true,
                        value_options: Vec::new(),
                    },
                    value: json!(50),
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
            editing: true,
            edit_buffer: "true".to_string(),
            edit_cursor: 2,
            edit_error: None,
            refreshing: false,
            refresh_rx: None,
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
    };

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 4,
            row: 5,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();

    assert_eq!(app.prop_dialog.as_ref().unwrap().selected, 0);
}

#[test]
fn number_shortcuts_switch_tabs() {
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
    };

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE),
    )
    .unwrap();
    assert_eq!(app.active_tab, 1);

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE),
    )
    .unwrap();
    assert_eq!(app.active_tab, 2);

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE),
    )
    .unwrap();
    assert_eq!(app.active_tab, 3);

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
    )
    .unwrap();
    assert_eq!(app.active_tab, 0);
}

#[test]
fn clicking_top_bar_tabs_switches_active_tab() {
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
    };

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 12,
            row: 1,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();
    assert_eq!(app.active_tab, 1);

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 22,
            row: 1,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();
    assert_eq!(app.active_tab, 2);

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 31,
            row: 1,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();
    assert_eq!(app.active_tab, 3);
}

#[test]
fn settings_tab_enter_on_reset_option_removes_mit_dir_after_single_confirm() {
    let home = make_temp_dir("tui-settings-reset-all");
    let test_home = home.join("test");
    let mit_dir = test_home.join(".mit");
    let _ = fs::create_dir_all(mit_dir.join("accounts").join("1001"));
    fs::write(mit_dir.join("auth.json"), "{\"accounts\":[]}\n").unwrap();
    fs::write(
        mit_dir.join("accounts").join("1001").join("devices.json"),
        "{\"devices\":[]}\n",
    )
    .unwrap();
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: test_home.clone(),
        auth_state: default_auth(),
        accounts: vec![test_account()],
        account_index: 0,
        devices: Vec::new(),
        device_index: 0,
        logs: VecDeque::new(),
        active_tab: 3,
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
        boot_spinner_index: 1,
        bootstrap_generation: 0,
        bootstrap_pending: None,
        bootstrap_tx,
        bootstrap_rx,
        property_cache: Arc::new(PropertyCache::new()),
        language: Language::Chinese,
        auto_subscribe_device_status: true,
        settings_selected: 3,
    };

    let quit = handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(!quit);
    assert!(mit_dir.exists());
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");
    assert!(compact.contains("操作:重置全部设置"), "{text}");
    assert!(compact.contains("该操作不可恢复"), "{text}");

    let quit = handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(quit);
    assert!(!mit_dir.exists());
    assert!(app.accounts.is_empty());
    assert!(app.devices.is_empty());
    assert!(app.logs.iter().any(|line| line.contains("已重置全部设置")));

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn settings_tab_shows_auto_subscribe_cache_clear_and_reset_actions() {
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
        active_tab: 3,
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

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");
    assert!(
        compact.contains("自动订阅设备状态(关闭后始终需要手动刷新)：开启"),
        "{text}"
    );
    assert!(compact.contains("重置设备缓存"), "{text}");
    assert!(compact.contains("重置全部设置"), "{text}");
    assert!(!compact.contains("规格缓存时间"), "{text}");
    assert!(!compact.contains("语言偏好"), "{text}");
}

#[test]
fn settings_tab_enter_toggles_auto_subscribe_and_persists() {
    let _guard = env_guard();
    let home = make_temp_dir("tui-settings-auto-subscribe");
    std::env::set_var("MIT_HOME", &home);
    std::env::set_var("MIT_PROFILE_DIR", &home);
    let mut app = devices_tab_test_app(Vec::new());
    app.home_dir = home.clone();
    app.active_tab = 3;
    app.settings_selected = 1;

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();

    assert!(app.account_action_dialog.is_none());
    let settings_text = fs::read_to_string(home.join(".mit").join("settings.json")).unwrap();
    assert!(settings_text.contains("\"autoSubscribeDeviceStatus\": false"));

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");
    assert!(
        compact.contains("自动订阅设备状态(关闭后始终需要手动刷新)：关闭"),
        "{text}"
    );

    let _ = fs::remove_dir_all(&home);
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
    });
    let (_tx, rx) = mpsc::channel();
    let now = Instant::now();
    {
        let mut runtime = super::cloud_mips_runtime().lock().unwrap();
        *runtime = Some(super::CloudMipsRuntime {
            key: "test-runtime".to_string(),
            _handles: Vec::new(),
            rx,
            last_mqtt_response_at: Some(now - Duration::from_secs(301)),
            last_ping_req_at: Some(now - Duration::from_secs(301)),
            last_ping_resp_at: Some(now - Duration::from_secs(301)),
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
fn action_param_labels_use_same_service_property_translations() {
    let spec = json!({
        "services": [{
            "iid": 5,
            "properties": [
                {"iid": 1, "description_trans": "音量", "description": "Volume"},
                {"iid": 2, "description": "Mode"}
            ],
            "actions": [{
                "iid": 9,
                "description_trans": "设置",
                "in": [1, 2, 99]
            }]
        }]
    });
    let actions = extract_actions_from_spec(&spec, Language::Chinese);
    assert_eq!(actions.len(), 1);
    assert_eq!(
        actions[0].input_labels,
        vec!["音量".to_string(), "Mode".to_string(), "参数3".to_string()]
    );
}

#[test]
fn action_param_edit_supports_tab_and_click_focus_switch() {
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
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: Vec::new(),
            selected: 0,
            active_tab: PropDialogTab::Actions,
            writable_selected: 0,
            readonly_selected: 0,
            actions: vec![ActionItem {
                siid: 5,
                aiid: 1,
                name: "设置".to_string(),
                input_piids: vec![1, 2],
                input_labels: vec!["音量".to_string(), "模式".to_string()],
                input_props: Vec::new(),
            }],
            actions_selected: 0,
            writable_list_state: ListState::default(),
            readonly_list_state: ListState::default(),
            actions_list_state: ListState::default(),
            loading: false,
            loading_rx: None,
            status: None,
            editing: true,
            edit_buffer: "\n".to_string(),
            edit_cursor: 0,
            edit_error: None,
            refreshing: false,
            refresh_rx: None,
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
    };

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
    )
    .unwrap();
    assert_eq!(app.prop_dialog.as_ref().unwrap().writable_selected, 1);

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
    )
    .unwrap();
    assert_eq!(app.prop_dialog.as_ref().unwrap().writable_selected, 0);

    let editor_area = super::prop_editor_layout(
        app.prop_dialog.as_ref().unwrap(),
        ratatui::layout::Rect::new(1, 1, 78, 22),
        Language::Chinese,
    )
    .editor_area;
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 4,
            row: editor_area.y.saturating_add(1),
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();
    assert_eq!(app.prop_dialog.as_ref().unwrap().writable_selected, 1);
}

#[test]
fn action_param_textarea_row_focus_updates_cursor_and_input() {
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
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: Vec::new(),
            selected: 0,
            active_tab: PropDialogTab::Actions,
            writable_selected: 0,
            readonly_selected: 0,
            actions: vec![ActionItem {
                siid: 5,
                aiid: 1,
                name: "设置".to_string(),
                input_piids: vec![1, 2],
                input_labels: vec!["音量".to_string(), "模式".to_string()],
                input_props: Vec::new(),
            }],
            actions_selected: 0,
            writable_list_state: ListState::default(),
            readonly_list_state: ListState::default(),
            actions_list_state: ListState::default(),
            loading: false,
            loading_rx: None,
            status: None,
            editing: true,
            edit_buffer: "1\n2".to_string(),
            edit_cursor: 1,
            edit_error: None,
            refreshing: false,
            refresh_rx: None,
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
    };

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let row_before = terminal_first_reversed_cell_row(&terminal).unwrap();

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
    )
    .unwrap();
    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE),
    )
    .unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let row_after = terminal_first_reversed_cell_row(&terminal).unwrap();
    assert!(
        row_after > row_before,
        "row_before={row_before}, row_after={row_after}"
    );
    assert_eq!(app.prop_dialog.as_ref().unwrap().edit_buffer, "1\n23");
}

#[test]
fn clicking_action_param_textarea_moves_cursor_to_clicked_character() {
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
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: Vec::new(),
            selected: 0,
            active_tab: PropDialogTab::Actions,
            writable_selected: 0,
            readonly_selected: 0,
            actions: vec![ActionItem {
                siid: 5,
                aiid: 1,
                name: "执行文本指令".to_string(),
                input_piids: vec![1],
                input_labels: vec!["参数1".to_string()],
                input_props: vec![PropItem {
                    siid: 5,
                    piid: 1,
                    name: "参数1".to_string(),
                    format: "string".to_string(),
                    writable: false,
                    value_options: Vec::new(),
                }],
            }],
            actions_selected: 0,
            writable_list_state: ListState::default(),
            readonly_list_state: ListState::default(),
            actions_list_state: ListState::default(),
            loading: false,
            loading_rx: None,
            status: None,
            editing: true,
            edit_buffer: "abcdef".to_string(),
            edit_cursor: 6,
            edit_error: None,
            refreshing: false,
            refresh_rx: None,
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
    };

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let (column, row) = terminal_find_substring_position(&terminal, "abcdef").unwrap();

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: column + 2,
            row,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();

    assert_eq!(app.prop_dialog.as_ref().unwrap().edit_cursor, 2);
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
    };

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("true"), "{text}");
    assert!(!text.contains("true|"), "{text}");
    assert!(terminal_has_reversed_cell(&terminal));
}

#[test]
fn clicking_prop_edit_textarea_moves_cursor_to_clicked_character() {
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
            edit_buffer: "abcdef".to_string(),
            edit_cursor: 6,
            edit_error: None,
            refreshing: false,
            refresh_rx: None,
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
    };

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let (column, row) = terminal_find_substring_position(&terminal, "abcdef").unwrap();

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: column + 2,
            row,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();

    assert_eq!(app.prop_dialog.as_ref().unwrap().edit_cursor, 2);
}

#[test]
fn action_param_edit_mode_shows_action_title_not_property_title() {
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
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: vec![ToggleItem {
                prop: PropItem {
                    siid: 2,
                    piid: 9,
                    name: "只读属性".to_string(),
                    format: "string".to_string(),
                    writable: false,
                    value_options: Vec::new(),
                },
                value: Value::String("x".to_string()),
            }],
            selected: 0,
            active_tab: PropDialogTab::Actions,
            writable_selected: 0,
            readonly_selected: 0,
            actions: vec![ActionItem {
                siid: 5,
                aiid: 1,
                name: "唤醒".to_string(),
                input_piids: vec![1],
                input_labels: vec!["参数1".to_string()],
                input_props: Vec::new(),
            }],
            actions_selected: 0,
            writable_list_state: ListState::default(),
            readonly_list_state: ListState::default(),
            actions_list_state: ListState::default(),
            loading: false,
            loading_rx: None,
            status: None,
            editing: true,
            edit_buffer: "1".to_string(),
            edit_cursor: 1,
            edit_error: None,
            refreshing: false,
            refresh_rx: None,
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
    };

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");
    assert!(text.contains("speaker"), "{text}");
    assert!(!text.contains("设备属性"), "{text}");
    assert!(compact.contains("唤醒"), "{text}");
    assert!(!compact.contains("Editing:唤醒"), "{text}");
    assert!(!compact.contains("[action]"), "{text}");
    assert!(!compact.contains("参数数量"), "{text}");
    assert!(
        !compact.contains("Input(Tab切换参数,点击行切换焦点):"),
        "{text}"
    );
    assert!(!compact.contains("输入参数(Tab切换参数焦点):"), "{text}");
    assert!(compact.contains("CLI命令(执行):"), "{text}");
    assert!(compact.contains("mitpropsactdev-1511"), "{text}");
}

#[test]
fn action_bool_param_uses_selector_editor() {
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
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: vec![ToggleItem {
                prop: PropItem {
                    siid: 5,
                    piid: 2,
                    name: "指令静默执行".to_string(),
                    format: "bool".to_string(),
                    writable: false,
                    value_options: Vec::new(),
                },
                value: Value::Bool(false),
            }],
            selected: 0,
            active_tab: PropDialogTab::Actions,
            writable_selected: 0,
            readonly_selected: 0,
            actions: vec![ActionItem {
                siid: 5,
                aiid: 1,
                name: "执行指令".to_string(),
                input_piids: vec![2],
                input_labels: vec!["指令静默执行".to_string()],
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
    };

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(app
        .prop_dialog
        .as_ref()
        .is_some_and(|dialog| dialog.editing));

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");
    assert!(compact.contains("[true]"), "{text}");
    assert!(compact.contains("[false]"), "{text}");
}

#[test]
fn action_editor_layout_is_compact_without_duplicate_help_lines() {
    let dialog = PropDialog {
        device_did: "718342728.s16".to_string(),
        device_name: "右键-客厅".to_string(),
        account_uid: "1001".to_string(),
        items: vec![ToggleItem {
            prop: PropItem {
                siid: 5,
                piid: 2,
                name: "指令静默执行".to_string(),
                format: "bool".to_string(),
                writable: false,
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
            aiid: 4,
            name: "执行文本指令".to_string(),
            input_piids: vec![2],
            input_labels: vec!["指令静默执行".to_string()],
            input_props: vec![PropItem {
                siid: 5,
                piid: 2,
                name: "指令静默执行".to_string(),
                format: "bool".to_string(),
                writable: false,
                value_options: Vec::new(),
            }],
        }],
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
    };
    let mut app = test_app_with_prop_dialog(dialog);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    let text = terminal_text(&terminal);
    let compact_lines = text
        .lines()
        .map(|line| line.replace(' ', ""))
        .collect::<Vec<_>>();
    let title_row = compact_lines
        .iter()
        .position(|line| line.contains("右键-客厅"))
        .unwrap();
    let action_row = compact_lines
        .iter()
        .position(|line| line.contains("执行文本指令"))
        .unwrap();
    let param_row = compact_lines
        .iter()
        .position(|line| line.contains("指令静默执行:[true],[false]"))
        .unwrap();
    let command_row = compact_lines
        .iter()
        .position(|line| line.contains("CLI命令(执行):mitpropsact718342728.s1654true"))
        .unwrap();
    let help_rows = compact_lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.contains("Esc:返回"))
        .map(|(idx, _)| idx)
        .collect::<Vec<_>>();

    assert!(!compact_lines[title_row].contains("R刷新"), "{text}");
    assert!(!compact_lines[title_row].contains("Esc关闭"), "{text}");
    assert_eq!(help_rows, vec![22], "{text}");
    assert!(
        action_row > 0 && compact_lines[action_row - 1].is_empty(),
        "{text}"
    );
    assert!(action_row <= title_row + 2, "{text}");
    assert!(param_row <= action_row + 2, "{text}");
    assert!(command_row <= param_row + 2, "{text}");
}

#[test]
fn action_without_params_keeps_command_compact() {
    let dialog = PropDialog {
        device_did: "718342728.s16".to_string(),
        device_name: "右键-客厅".to_string(),
        account_uid: "1001".to_string(),
        items: Vec::new(),
        selected: 0,
        active_tab: PropDialogTab::Actions,
        writable_selected: 0,
        readonly_selected: 0,
        actions: vec![ActionItem {
            siid: 5,
            aiid: 4,
            name: "无参动作".to_string(),
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
        editing: true,
        edit_buffer: String::new(),
        edit_cursor: 0,
        edit_error: None,
        refreshing: false,
        refresh_rx: None,
    };
    let mut app = test_app_with_prop_dialog(dialog);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    let text = terminal_text(&terminal);
    let compact_lines = text
        .lines()
        .map(|line| line.replace(' ', ""))
        .collect::<Vec<_>>();
    let action_row = compact_lines
        .iter()
        .position(|line| line.contains("无参动作"))
        .unwrap();
    let command_row = compact_lines
        .iter()
        .position(|line| line.contains("CLI命令(执行):mitpropsact718342728.s1654"))
        .unwrap();

    assert!(command_row <= action_row + 2, "{text}");
}

#[test]
fn action_enum_param_uses_selector_editor() {
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
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: vec![ToggleItem {
                prop: PropItem {
                    siid: 5,
                    piid: 3,
                    name: "执行模式".to_string(),
                    format: "uint8".to_string(),
                    writable: false,
                    value_options: vec![
                        super::PropValueOption {
                            label: "optionA".to_string(),
                            value: Value::Number(0.into()),
                        },
                        super::PropValueOption {
                            label: "optionB".to_string(),
                            value: Value::Number(1.into()),
                        },
                        super::PropValueOption {
                            label: "optionC".to_string(),
                            value: Value::Number(2.into()),
                        },
                    ],
                },
                value: Value::Number(1.into()),
            }],
            selected: 0,
            active_tab: PropDialogTab::Actions,
            writable_selected: 0,
            readonly_selected: 0,
            actions: vec![ActionItem {
                siid: 5,
                aiid: 1,
                name: "执行指令".to_string(),
                input_piids: vec![3],
                input_labels: vec!["执行模式".to_string()],
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
    };

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(app
        .prop_dialog
        .as_ref()
        .is_some_and(|dialog| dialog.editing));

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");
    assert!(compact.contains("[optionA]"), "{text}");
    assert!(compact.contains("[optionB]"), "{text}");
    assert!(compact.contains("[optionC]"), "{text}");
}

#[test]
fn action_bool_param_without_readable_prop_still_uses_selector_editor() {
    let spec = json!({
        "services": [{
            "iid": 5,
            "properties": [{
                "iid": 2,
                "description_trans": "指令静默执行",
                "format": "bool",
                "access": [],
                "type": "urn:miot-spec-v2:property:silent-execution:000000FB:xiaomi-oh4w:1"
            }],
            "actions": [{
                "iid": 1,
                "description_trans": "执行指令",
                "in": [2]
            }]
        }]
    });
    let actions = extract_actions_from_spec(&spec, Language::Chinese);

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
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: Vec::new(),
            selected: 0,
            active_tab: PropDialogTab::Actions,
            writable_selected: 0,
            readonly_selected: 0,
            actions,
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
    };

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");
    assert!(compact.contains("[true]"), "{text}");
    assert!(compact.contains("[false]"), "{text}");
}

#[test]
fn prop_editor_bottom_lines_show_get_then_set_for_writable_props() {
    let dialog = PropDialog {
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
    };

    assert_eq!(
        super::prop_editor_bottom_lines(&dialog, Language::Chinese),
        vec![
            "CLI 命令(读取): mit props get dev-1 2 1 --json".to_string(),
            "CLI 命令(执行): mit props set dev-1 2 1 true".to_string(),
        ]
    );
}

#[test]
fn draw_writable_prop_editor_shows_get_command_above_set_command() {
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
    });

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");

    let get_pos = compact
        .find("CLI命令(读取):mitpropsgetdev-121--json")
        .unwrap();
    let set_pos = compact
        .find("CLI命令(执行):mitpropssetdev-121true")
        .unwrap();
    assert!(get_pos < set_pos, "{text}");
}

#[test]
fn action_enum_param_without_readable_prop_still_uses_selector_editor() {
    let spec = json!({
        "services": [{
            "iid": 5,
            "properties": [{
                "iid": 3,
                "description_trans": "执行模式",
                "format": "uint8",
                "access": [],
                "value-list": [
                    {"value": 0, "description_trans": "optionA"},
                    {"value": 1, "description_trans": "optionB"},
                    {"value": 2, "description_trans": "optionC"}
                ]
            }],
            "actions": [{
                "iid": 1,
                "description_trans": "执行指令",
                "in": [3]
            }]
        }]
    });
    let actions = extract_actions_from_spec(&spec, Language::Chinese);

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
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: Vec::new(),
            selected: 0,
            active_tab: PropDialogTab::Actions,
            writable_selected: 0,
            readonly_selected: 0,
            actions,
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
    };

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");
    assert!(compact.contains("[optionA]"), "{text}");
    assert!(compact.contains("[optionB]"), "{text}");
    assert!(compact.contains("[optionC]"), "{text}");
}

#[test]
fn writable_bool_prop_enters_selector_editor_before_execution() {
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
    };

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();

    assert!(app
        .prop_dialog
        .as_ref()
        .is_some_and(|dialog| dialog.editing));
    assert!(app
        .prop_dialog
        .as_ref()
        .is_some_and(|dialog| dialog.editing));

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");
    assert!(compact.contains("[true]"), "{text}");
    assert!(compact.contains("[false]"), "{text}");
}

#[test]
fn writable_bool_prop_selector_highlights_current_option_in_green() {
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
    };

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(terminal_has_green_substring(&terminal, "[true]"));
}

#[test]
fn writable_enum_prop_enters_selector_editor_before_execution() {
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
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: vec![ToggleItem {
                prop: PropItem {
                    siid: 2,
                    piid: 2,
                    name: "模式".to_string(),
                    format: "uint8".to_string(),
                    writable: true,
                    value_options: vec![
                        super::PropValueOption {
                            label: "optionA".to_string(),
                            value: Value::Number(0.into()),
                        },
                        super::PropValueOption {
                            label: "optionB".to_string(),
                            value: Value::Number(1.into()),
                        },
                        super::PropValueOption {
                            label: "optionC".to_string(),
                            value: Value::Number(2.into()),
                        },
                    ],
                },
                value: Value::Number(1.into()),
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
    };

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();

    assert!(app
        .prop_dialog
        .as_ref()
        .is_some_and(|dialog| dialog.editing));
    assert!(app
        .prop_dialog
        .as_ref()
        .is_some_and(|dialog| dialog.editing));

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");
    assert!(compact.contains("[optionA]"), "{text}");
    assert!(compact.contains("[optionB]"), "{text}");
    assert!(compact.contains("[optionC]"), "{text}");
}

#[test]
fn action_enum_param_selector_highlights_current_option_in_green() {
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
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: vec![ToggleItem {
                prop: PropItem {
                    siid: 5,
                    piid: 3,
                    name: "执行模式".to_string(),
                    format: "uint8".to_string(),
                    writable: false,
                    value_options: vec![
                        super::PropValueOption {
                            label: "optionA".to_string(),
                            value: Value::Number(0.into()),
                        },
                        super::PropValueOption {
                            label: "optionB".to_string(),
                            value: Value::Number(1.into()),
                        },
                        super::PropValueOption {
                            label: "optionC".to_string(),
                            value: Value::Number(2.into()),
                        },
                    ],
                },
                value: Value::Number(1.into()),
            }],
            selected: 0,
            active_tab: PropDialogTab::Actions,
            writable_selected: 0,
            readonly_selected: 0,
            actions: vec![ActionItem {
                siid: 5,
                aiid: 1,
                name: "执行指令".to_string(),
                input_piids: vec![3],
                input_labels: vec!["执行模式".to_string()],
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
    };

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(terminal_has_green_substring(&terminal, "[optionB]"));
}

#[test]
fn clicking_writable_bool_prop_selector_option_updates_selection() {
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
    };

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let (column, row) =
        terminal_find_substring_position(&terminal, "[false]").expect("selector option");

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();

    assert_eq!(
        app.prop_dialog.as_ref().unwrap().edit_buffer,
        "false".to_string()
    );
}

#[test]
fn clicking_action_enum_param_selector_option_updates_selection() {
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
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: vec![ToggleItem {
                prop: PropItem {
                    siid: 5,
                    piid: 3,
                    name: "执行模式".to_string(),
                    format: "uint8".to_string(),
                    writable: false,
                    value_options: vec![
                        super::PropValueOption {
                            label: "optionA".to_string(),
                            value: Value::Number(0.into()),
                        },
                        super::PropValueOption {
                            label: "optionB".to_string(),
                            value: Value::Number(1.into()),
                        },
                        super::PropValueOption {
                            label: "optionC".to_string(),
                            value: Value::Number(2.into()),
                        },
                    ],
                },
                value: Value::Number(1.into()),
            }],
            selected: 0,
            active_tab: PropDialogTab::Actions,
            writable_selected: 0,
            readonly_selected: 0,
            actions: vec![ActionItem {
                siid: 5,
                aiid: 1,
                name: "执行指令".to_string(),
                input_piids: vec![3],
                input_labels: vec!["执行模式".to_string()],
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
    };

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let (column, row) =
        terminal_find_substring_position(&terminal, "[optionC]").expect("selector option");

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 100, 24),
    )
    .unwrap();

    assert_eq!(
        app.prop_dialog.as_ref().unwrap().edit_buffer,
        "2".to_string()
    );
}

#[test]
fn clicking_action_editor_cli_command_does_not_copy_on_single_click() {
    let _guard = env_guard();
    let clip_file = make_temp_dir("tui-action-command-copy").join("clipboard.txt");
    std::env::set_var("MIT_TEST_CLIPBOARD_FILE", &clip_file);

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
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: vec![ToggleItem {
                prop: PropItem {
                    siid: 5,
                    piid: 2,
                    name: "静默执行".to_string(),
                    format: "bool".to_string(),
                    writable: false,
                    value_options: Vec::new(),
                },
                value: Value::Bool(false),
            }],
            selected: 0,
            active_tab: PropDialogTab::Actions,
            writable_selected: 0,
            readonly_selected: 0,
            actions: vec![ActionItem {
                siid: 5,
                aiid: 4,
                name: "执行命令".to_string(),
                input_piids: vec![1, 2],
                input_labels: vec!["参数1".to_string(), "参数2".to_string()],
                input_props: vec![
                    PropItem {
                        siid: 5,
                        piid: 1,
                        name: "参数1".to_string(),
                        format: "string".to_string(),
                        writable: false,
                        value_options: Vec::new(),
                    },
                    PropItem {
                        siid: 5,
                        piid: 2,
                        name: "静默执行".to_string(),
                        format: "bool".to_string(),
                        writable: false,
                        value_options: Vec::new(),
                    },
                ],
            }],
            actions_selected: 0,
            writable_list_state: ListState::default(),
            readonly_list_state: ListState::default(),
            actions_list_state: ListState::default(),
            loading: false,
            loading_rx: None,
            status: None,
            editing: true,
            edit_buffer: "\"arg1\"\ntrue".to_string(),
            edit_cursor: 4,
            edit_error: None,
            refreshing: false,
            refresh_rx: None,
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
    };

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let expected = r#"mit props act dev-1 5 4 "arg1" true"#;
    let (column, row) =
        terminal_find_substring_position(&terminal, expected).expect("command preview");

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 100, 24),
    )
    .unwrap();

    assert!(!clip_file.exists());
    assert!(!app
        .logs
        .iter()
        .any(|line| line.starts_with(super::FOOTER_COPY_LOG_PREFIX)));

    std::env::remove_var("MIT_TEST_CLIPBOARD_FILE");
    let _ = std::fs::remove_file(&clip_file);
}

#[test]
fn action_param_textarea_refocus_moves_cursor_to_end() {
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
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: Vec::new(),
            selected: 0,
            active_tab: PropDialogTab::Actions,
            writable_selected: 0,
            readonly_selected: 0,
            actions: vec![ActionItem {
                siid: 5,
                aiid: 1,
                name: "设置".to_string(),
                input_piids: vec![1, 2],
                input_labels: vec!["参数1".to_string(), "参数2".to_string()],
                input_props: vec![
                    PropItem {
                        siid: 5,
                        piid: 1,
                        name: "参数1".to_string(),
                        format: "string".to_string(),
                        writable: false,
                        value_options: Vec::new(),
                    },
                    PropItem {
                        siid: 5,
                        piid: 2,
                        name: "参数2".to_string(),
                        format: "string".to_string(),
                        writable: false,
                        value_options: Vec::new(),
                    },
                ],
            }],
            actions_selected: 0,
            writable_list_state: ListState::default(),
            readonly_list_state: ListState::default(),
            actions_list_state: ListState::default(),
            loading: false,
            loading_rx: None,
            status: None,
            editing: true,
            edit_buffer: "abcdef\nxy".to_string(),
            edit_cursor: 1,
            edit_error: None,
            refreshing: false,
            refresh_rx: None,
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
    };

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
    )
    .unwrap();

    let dialog = app.prop_dialog.as_ref().unwrap();
    assert_eq!(dialog.writable_selected, 1);
    assert_eq!(dialog.edit_cursor, 2);
}

#[test]
fn dragging_action_editor_cli_command_copies_preview_and_shows_badge() {
    let _guard = env_guard();
    let clip_file = make_temp_dir("tui-action-command-select-copy").join("clipboard.txt");
    std::env::set_var("MIT_TEST_CLIPBOARD_FILE", &clip_file);
    super::clear_selection_state();

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
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: vec![ToggleItem {
                prop: PropItem {
                    siid: 5,
                    piid: 2,
                    name: "静默执行".to_string(),
                    format: "bool".to_string(),
                    writable: false,
                    value_options: Vec::new(),
                },
                value: Value::Bool(false),
            }],
            selected: 0,
            active_tab: PropDialogTab::Actions,
            writable_selected: 0,
            readonly_selected: 0,
            actions: vec![ActionItem {
                siid: 5,
                aiid: 4,
                name: "执行命令".to_string(),
                input_piids: vec![1, 2],
                input_labels: vec!["参数1".to_string(), "参数2".to_string()],
                input_props: vec![
                    PropItem {
                        siid: 5,
                        piid: 1,
                        name: "参数1".to_string(),
                        format: "string".to_string(),
                        writable: false,
                        value_options: Vec::new(),
                    },
                    PropItem {
                        siid: 5,
                        piid: 2,
                        name: "静默执行".to_string(),
                        format: "bool".to_string(),
                        writable: false,
                        value_options: Vec::new(),
                    },
                ],
            }],
            actions_selected: 0,
            writable_list_state: ListState::default(),
            readonly_list_state: ListState::default(),
            actions_list_state: ListState::default(),
            loading: false,
            loading_rx: None,
            status: None,
            editing: true,
            edit_buffer: "\"arg1\"\ntrue".to_string(),
            edit_cursor: 4,
            edit_error: None,
            refreshing: false,
            refresh_rx: None,
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
    };

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let expected = r#"mit props act dev-1 5 4 "arg1" true"#;
    let (column, row) =
        terminal_find_substring_position(&terminal, expected).expect("command preview");
    let end_column = column.saturating_add(super::display_width(expected));

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 100, 24),
    )
    .unwrap();
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Left),
            column: end_column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 100, 24),
    )
    .unwrap();
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Up(crossterm::event::MouseButton::Left),
            column: end_column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 100, 24),
    )
    .unwrap();

    assert_eq!(fs::read_to_string(&clip_file).unwrap(), expected);
    assert!(app
        .logs
        .iter()
        .any(|line| line.starts_with(super::FOOTER_COPY_LOG_PREFIX)));
    let line = super::footer_line(&app, super::now_epoch_millis());
    assert!(line
        .spans
        .iter()
        .any(|span| span.content.as_ref().contains("[已复制]")));

    std::env::remove_var("MIT_TEST_CLIPBOARD_FILE");
    let _ = fs::remove_file(&clip_file);
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
fn action_param_textarea_grows_height_when_value_wraps() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let long_input = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu";
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
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: Vec::new(),
            selected: 0,
            active_tab: PropDialogTab::Actions,
            writable_selected: 0,
            readonly_selected: 0,
            actions: vec![ActionItem {
                siid: 5,
                aiid: 1,
                name: "执行文本指令".to_string(),
                input_piids: vec![1],
                input_labels: vec!["参数1".to_string()],
                input_props: vec![PropItem {
                    siid: 5,
                    piid: 1,
                    name: "参数1".to_string(),
                    format: "string".to_string(),
                    writable: false,
                    value_options: Vec::new(),
                }],
            }],
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
    };

    let mut terminal = Terminal::new(TestBackend::new(60, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    let text = terminal_text(&terminal);
    let lines = text.lines().collect::<Vec<_>>();
    let first_param_line = lines
        .iter()
        .position(|line| line.replace(' ', "").contains(">参数1:"))
        .expect("action param label");
    let wrapped_line = &lines[first_param_line + 1];
    assert!(!wrapped_line.trim().is_empty(), "{text}");
    let s = wrapped_line.replace(' ', "");
    assert!(
        !s.starts_with("CLI命令:")
            && !s.starts_with("CLI命令(执行):")
            && !s.starts_with("CLI命令(读取):"),
        "{text}"
    );
}

#[test]
fn prop_edit_mode_moves_cursor_with_left_right() {
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
    };

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
    )
    .unwrap();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("true"), "{text}");
    assert!(!text.contains("tru|e"), "{text}");
    assert!(terminal_has_reversed_cell(&terminal));

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
    )
    .unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("true"), "{text}");
    assert!(!text.contains("true|"), "{text}");
    assert!(terminal_has_reversed_cell(&terminal));
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
fn parse_bool_prop_value_handles_entry_object_shape() {
    assert_eq!(parse_bool_prop_value(&json!({"value": true})), Some(true));
    assert_eq!(parse_bool_prop_value(&json!({"value": 0})), Some(false));
}
