// Auto-split from the former monolithic tui.rs. Shares the `tests` module
// scope (imports + helpers) of mod.rs via include!; do not add `use` here.

#[test]
fn display_truncate_pad_handles_cjk_double_width() {
    // "音箱" is 2 CJK chars, each 2 display cols wide = 4 total
    let result = display_truncate_pad("音箱", 10);
    assert_eq!(UnicodeWidthStr::width(result.as_str()), 10);
    assert!(result.starts_with("音箱"));
}

#[test]
fn mouse_wheel_scroll_changes_active_item() {
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

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollDown,
            column: 2,
            row: 4,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();
    assert_eq!(app.device_index, 1);

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollUp,
            column: 2,
            row: 4,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();
    assert_eq!(app.device_index, 0);
}

#[test]
fn mouse_selection_state_is_thread_local() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let push_input = "hello world".to_string();
    let push_area =
        super::push_message_textarea_area(ratatui::layout::Rect::new(0, 0, 80, 24), &push_input);
    let push_mouse = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: push_area.x,
        row: push_area.y,
        modifiers: KeyModifiers::NONE,
    };
    let push_terminal = ratatui::layout::Rect::new(0, 0, 80, 24);

    super::clear_selection_state();
    let push_app = TuiApp {
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
        account_action_dialog: Some(AccountActionDialog::PushMessage {
            uid: "1001".to_string(),
            input: push_input,
            cursor: 11,
            return_to_menu_selected: 0,
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
    };
    assert!(!super::selection_start(
        &push_app,
        push_mouse,
        push_terminal
    ));
    assert_eq!(
        super::selected_surface()
            .expect("push-message selection should be active")
            .snapshot
            .surface,
        super::SelectionSurface::PushMessageInput
    );

    thread::spawn(|| {
        let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
        let (local_transport_tx, local_transport_rx) =
            mpsc::channel::<LocalTransportRefreshMessage>();
        let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
        let logs_app = TuiApp {
            home_dir: PathBuf::from("."),
            auth_state: default_auth(),
            accounts: Vec::new(),
            account_index: 0,
            devices: Vec::new(),
            device_index: 0,
            logs: VecDeque::from([String::from("log line")]),
            active_tab: 2,
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
        let [_tabs_area, content_area, _status_gap_area, _status_bar_area] =
            super::split_main_layout(ratatui::layout::Rect::new(0, 0, 80, 24));
        let [_search_area, _search_border_area, list_area] =
            super::searchable_main_layout(content_area);
        super::clear_selection_state();
        assert!(super::selection_start(
            &logs_app,
            crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left,),
                column: list_area.x,
                row: list_area.y,
                modifiers: KeyModifiers::NONE,
            },
            ratatui::layout::Rect::new(0, 0, 80, 24),
        ));
        assert_eq!(
            super::selected_surface()
                .expect("log selection should be active")
                .snapshot
                .surface,
            super::SelectionSurface::Logs
        );
    })
    .join()
    .unwrap();

    assert_eq!(
        super::selected_surface()
            .expect("push-message selection should remain isolated")
            .snapshot
            .surface,
        super::SelectionSurface::PushMessageInput
    );
}

#[test]
fn shift_c_recopies_last_mouse_selection() {
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
        logs: VecDeque::from(["gamma".to_string()]),
        active_tab: 2,
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

    let _guard = env_guard();
    let clip_file = make_temp_dir("tui-shift-c-copy").join("clipboard.txt");
    std::env::set_var("MIT_TEST_CLIPBOARD_FILE", &clip_file);
    let message_column = log_message_start_column();

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: message_column,
            row: 5,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Left),
            column: message_column + 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Up(crossterm::event::MouseButton::Left),
            column: message_column + 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();

    let _ = fs::remove_file(&clip_file);
    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('C'), KeyModifiers::SHIFT),
    )
    .unwrap();
    let copied = fs::read_to_string(&clip_file).unwrap();
    assert_eq!(copied, "gamma");

    std::env::remove_var("MIT_TEST_CLIPBOARD_FILE");
    let _ = fs::remove_file(&clip_file);
}

#[test]
fn plain_click_outside_selected_text_clears_selection() {
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
        logs: VecDeque::from(["gamma".to_string()]),
        active_tab: 2,
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

    let _guard = env_guard();
    let clip_file = make_temp_dir("tui-clear-selection-outside-click").join("clipboard.txt");
    std::env::set_var("MIT_TEST_CLIPBOARD_FILE", &clip_file);

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 0,
            row: 5,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Left),
            column: 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Up(crossterm::event::MouseButton::Left),
            column: 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 20,
            row: 10,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();

    let _ = fs::remove_file(&clip_file);
    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('C'), KeyModifiers::SHIFT),
    )
    .unwrap();
    assert!(
        !clip_file.exists(),
        "selection should be cleared by outside click"
    );

    std::env::remove_var("MIT_TEST_CLIPBOARD_FILE");
    let _ = fs::remove_file(&clip_file);
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
    });
    let (_tx, rx) = mpsc::channel();
    let now = Instant::now();
    {
        let mut runtime = super::cloud_mips_runtime().lock().unwrap();
        *runtime = Some(super::CloudMipsRuntime {
            key: "test-runtime".to_string(),
            _handles: Vec::new(),
            rx,
            last_mqtt_response_at: Some(now - Duration::from_secs(8)),
            last_ping_req_at: Some(now - Duration::from_secs(9)),
            last_ping_resp_at: Some(now - Duration::from_secs(8)),
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
fn stale_keypress_skips_heartbeat_when_mips_disabled() {
    let _guard = env_guard();
    std::env::remove_var("MIT_DISABLE_CLOUD_MIPS");
    std::env::set_var("MIT_ENABLE_CLOUD_MIPS_IN_TESTS", "1");
    let mut app = devices_tab_test_app(vec![test_device(
        "dev-kitchen",
        "kitchen plug",
        "Kitchen",
        "A(1001)",
    )]);
    app.auto_subscribe_device_status = false;
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
        crossterm::event::KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
    )
    .unwrap();

    let logs = app.logs.iter().cloned().collect::<Vec<_>>().join("\n");
    assert!(!logs.contains("cloud MIPS response stale"));
    assert!(!logs.contains("cloud MIPS waiting for PingResp"));

    std::env::remove_var("MIT_ENABLE_CLOUD_MIPS_IN_TESTS");
    let mut runtime = super::cloud_mips_runtime().lock().unwrap();
    *runtime = None;
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
