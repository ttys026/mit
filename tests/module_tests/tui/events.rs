// Auto-split: shares the `tests` module scope (imports + helpers) of
// mod.rs via include!; do not add `use` here.

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
                pid: 0,

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
                pid: 0,

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
        invalid_xiaomi_account_uids: HashSet::new(),
        invalid_mijia_account_uids: HashSet::new(),
        account_check_in_flight: false,
        boot_state: BootState::Ready,
        boot_spinner_index: 1,
        bootstrap_generation: 0,
        bootstrap_pending: None,
        bootstrap_tx,
        bootstrap_rx,
        property_cache: Arc::new(PropertyCache::new()),
        language: Language::Chinese,
        auto_subscribe_device_status: true,
        settings_selected: 6,
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
    assert!(mit_dir.exists());
    // Tall enough that the centered confirm dialog clears the settings list below it.
    let mut terminal = Terminal::new(TestBackend::new(100, 44)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");
    let action = match &app.account_action_dialog {
        Some(AccountActionDialog::SettingsConfirm { action }) => format!("{action:?}"),
        other => panic!("expected reset confirm dialog, got {other:?}"),
    };
    assert_eq!(action, "ResetAll");
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
fn settings_tab_shows_combined_version_view_github_and_separator() {
    let mut app = devices_tab_test_app(Vec::new());
    app.active_tab = 3;
    app.language = Language::Chinese;

    let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");

    // Combined item: current version plus a prompt to check for updates on Enter.
    assert!(
        compact.contains(&format!(
            "当前版本：v{}（回车检查更新）",
            env!("CARGO_PKG_VERSION")
        )),
        "{text}"
    );
    // New "view on GitHub" item.
    assert!(compact.contains("在GitHub查看"), "{text}");
    assert!(compact.contains("重新同步三方设备状态"), "{text}");
    // A full-width separator divides the toggleable settings from the action items.
    assert!(
        text.lines()
            .any(|line| line.contains('─') && line.chars().all(|c| c == '─' || c == ' ')),
        "{text}"
    );
}

#[test]
fn settings_separator_rows_map_to_no_action() {
    // Layout: ─, [lang, auto], ─, [version, github], ─, [thirdcloud], ─, [reset cache, reset all].
    assert_eq!(settings_action_for_row(0), None); // leading divider
    assert_eq!(settings_action_for_row(1), Some(0));
    assert_eq!(settings_action_for_row(2), Some(1));
    assert_eq!(settings_action_for_row(3), None); // divider before version group
    assert_eq!(settings_action_for_row(4), Some(2));
    assert_eq!(settings_action_for_row(5), Some(3));
    assert_eq!(settings_action_for_row(6), None); // divider before thirdcloud group
    assert_eq!(settings_action_for_row(7), Some(4));
    assert_eq!(settings_action_for_row(8), None); // divider before reset group
    assert_eq!(settings_action_for_row(9), Some(5));
    assert_eq!(settings_action_for_row(10), Some(6));
    assert_eq!(settings_action_for_row(11), None); // past the end
}

#[test]
fn settings_tab_enter_on_view_github_logs_repo_url() {
    let mut app = devices_tab_test_app(Vec::new());
    app.active_tab = 3;
    app.settings_selected = 3; // "View on GitHub"

    let quit = handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();

    assert!(!quit);
    assert!(app.account_action_dialog.is_none());
    assert!(
        app.logs
            .iter()
            .any(|line| line.contains("github.com/ttys026/mit")),
        "{:?}",
        app.logs
    );
}

#[test]
fn settings_tab_enter_on_thirdcloud_sync_without_mijia_shows_dialog() {
    let mut app = devices_tab_test_app(Vec::new());
    app.active_tab = 3;
    app.settings_selected = 4; // "重新同步三方设备状态"

    let quit = handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();

    assert!(!quit);
    match &app.account_action_dialog {
        Some(AccountActionDialog::ThirdCloudSync {
            groups,
            running,
            message,
            ..
        }) => {
            assert!(!running);
            assert!(groups.is_empty());
            assert!(message.contains("未登录米家"), "{message}");
        }
        other => panic!("expected thirdcloud sync dialog, got {other:?}"),
    }
}

#[test]
fn strip_ansi_removes_color_codes_and_control_bytes() {
    // Mirrors real install-script output: SGR color codes around plain text.
    assert_eq!(
        strip_ansi("\u{1b}[0;36m\u{1b}[1m==>\u{1b}[0m Downloading"),
        "==> Downloading"
    );
    assert_eq!(
        strip_ansi("\u{1b}[0;32m\u{1b}[1m \u{2713}\u{1b}[0m Latest version: v1.1.0"),
        " ✓ Latest version: v1.1.0"
    );
    // Carriage returns and other control bytes are dropped; plain text is untouched.
    assert_eq!(strip_ansi("done\r"), "done");
    assert_eq!(
        strip_ansi("mit-1.1.0-aarch64-apple-darwin.tar.gz"),
        "mit-1.1.0-aarch64-apple-darwin.tar.gz"
    );
}

#[test]
fn update_available_dialog_pops_and_install_completes() {
    let mut app = devices_tab_test_app(Vec::new());
    app.active_tab = 3;

    // A newer version was found → the upgrade dialog pops automatically.
    app.apply_update_check_result(Ok("v9.9.9".to_string()));
    assert!(matches!(
        app.account_action_dialog,
        Some(AccountActionDialog::UpdateAvailable { .. })
    ));

    // Enter confirms → install begins (the test stub streams a line then succeeds).
    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();

    // Drain background install messages until the dialog reports a result.
    let mut waited = 0;
    let success = loop {
        app.process_background_messages();
        if let Some(AccountActionDialog::UpdateFinished { success, .. }) =
            &app.account_action_dialog
        {
            break *success;
        }
        assert!(waited < 100, "install never finished");
        thread::sleep(Duration::from_millis(5));
        waited += 1;
    };
    assert!(success);

    // Esc closes the result dialog.
    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(app.account_action_dialog.is_none());
}

#[test]
fn update_running_dialog_cancels_on_esc_q_and_ctrl_c() {
    for key in [
        crossterm::event::KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        crossterm::event::KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
        crossterm::event::KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
    ] {
        let mut app = devices_tab_test_app(Vec::new());
        app.active_tab = 3;
        app.account_action_dialog = Some(AccountActionDialog::UpdateRunning {
            latest: "v9.9.9".to_string(),
            lines: vec!["downloading…".to_string()],
            pid: None, // no real process to kill in the test
        });

        let quit = handle_key(&mut app, key).unwrap();

        assert!(!quit, "cancel must not quit the app: {:?}", key.code);
        assert!(
            app.account_action_dialog.is_none(),
            "dialog should close on {:?}",
            key.code
        );
        assert!(app.logs.iter().any(|line| line.contains("已取消升级")));
    }
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
fn clicking_devices_row_only_changes_active_index() {
    let home = make_temp_dir("tui-devices-click-open");
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
        devices: vec![
            Device {
                did: "dev-1".to_string(),
                name: "d1".to_string(),
                model: "xiaomi.wifispeaker.lx04".to_string(),
                online: false,
                pid: 0,

                home_id: "cache-account:1001".to_string(),
                home_name: "账号A(1001)".to_string(),
                room_id: "room-1".to_string(),
                room_name: "客厅".to_string(),
            },
            Device {
                did: "dev-2".to_string(),
                name: "d2".to_string(),
                model: "xiaomi.wifispeaker.lx04".to_string(),
                online: false,
                pid: 0,

                home_id: "cache-account:1001".to_string(),
                home_name: "账号A(1001)".to_string(),
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

    // Device tab has a search row, an empty spacer row, and a 1-line header. Clicking the
    // header row should not select a device.
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 2,
            row: 5,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();
    assert_eq!(app.device_index, 0);
    assert!(app.prop_dialog.is_none());

    // Click second device row (first data row starts at y=6 for default 80x24 layout).
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 2,
            row: 7,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();
    assert_eq!(app.device_index, 1);
    assert!(app.prop_dialog.is_none());

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn clicking_selected_device_row_opens_dialog() {
    let home = make_temp_dir("tui-devices-click-enter");
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
        devices: vec![
            Device {
                did: "dev-1".to_string(),
                name: "d1".to_string(),
                model: "xiaomi.wifispeaker.lx04".to_string(),
                online: false,
                pid: 0,

                home_id: "cache-account:1001".to_string(),
                home_name: "账号A(1001)".to_string(),
                room_id: "room-1".to_string(),
                room_name: "客厅".to_string(),
            },
            Device {
                did: "dev-2".to_string(),
                name: "d2".to_string(),
                model: "xiaomi.wifispeaker.lx04".to_string(),
                online: false,
                pid: 0,

                home_id: "cache-account:1001".to_string(),
                home_name: "账号A(1001)".to_string(),
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

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 2,
            row: 7,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();
    assert_eq!(app.device_index, 1);
    assert!(app.prop_dialog.is_none());

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 2,
            row: 7,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();
    assert!(app.prop_dialog.is_some());

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn device_row_mouse_up_does_not_open_dialog() {
    let home = make_temp_dir("tui-devices-mouse-up");
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
        devices: vec![
            Device {
                did: "dev-1".to_string(),
                name: "d1".to_string(),
                model: "xiaomi.wifispeaker.lx04".to_string(),
                online: false,
                pid: 0,

                home_id: "cache-account:1001".to_string(),
                home_name: "账号A(1001)".to_string(),
                room_id: "room-1".to_string(),
                room_name: "客厅".to_string(),
            },
            Device {
                did: "dev-2".to_string(),
                name: "d2".to_string(),
                model: "xiaomi.wifispeaker.lx04".to_string(),
                online: false,
                pid: 0,

                home_id: "cache-account:1001".to_string(),
                home_name: "账号A(1001)".to_string(),
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

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 2,
            row: 7,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();
    assert_eq!(app.device_index, 1);
    assert!(app.prop_dialog.is_none());

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Up(crossterm::event::MouseButton::Left),
            column: 2,
            row: 7,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();
    assert_eq!(app.device_index, 1);
    assert!(app.prop_dialog.is_none());

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn settings_tab_enter_purges_devices_cache_after_single_confirm() {
    let home = make_temp_dir("tui-settings-purge-devices-cache");
    let test_home = home.join("test");
    let mit_dir = test_home.join(".mit");
    let _ = fs::create_dir_all(&mit_dir);
    let _ = fs::create_dir_all(mit_dir.join("accounts").join("1001"));
    let _ = fs::create_dir_all(mit_dir.join("accounts").join("2002"));
    let _ = fs::create_dir_all(mit_dir.join("cache").join("specs"));
    let cache_a = mit_dir.join("accounts").join("1001").join("devices.json");
    let cache_b = mit_dir.join("accounts").join("2002").join("devices.json");
    let auth_file = mit_dir.join("auth.json");
    let extra_settings_file = mit_dir.join("settings.json");
    fs::write(&cache_a, "{\"devices\":[]}\n").unwrap();
    fs::write(&cache_b, "{\"devices\":[]}\n").unwrap();
    fs::write(
        mit_dir.join("cache").join("specs").join("index.json"),
        "{}\n",
    )
    .unwrap();
    fs::write(&auth_file, "{\"accounts\":[]}\n").unwrap();
    fs::write(&extra_settings_file, "{}\n").unwrap();

    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: test_home.clone(),
        auth_state: default_auth(),
        accounts: vec![test_account()],
        account_index: 0,
        devices: vec![Device {
            did: "dev-1".to_string(),
            name: "设备1".to_string(),
            model: "xiaomi.test.v1".to_string(),
            online: true,
            pid: 0,
            home_id: "cache-account:1001".to_string(),
            home_name: "账号A".to_string(),
            room_id: "room-a".to_string(),
            room_name: "客厅".to_string(),
        }],
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
        settings_selected: 5,
        update_check_rx: None,
        update_check_status: None,
        install_rx: None,
    };
    app.property_cache.set_device_properties(
        "dev-1".to_string(),
        std::collections::HashMap::from([((2, 1), json!(true))]),
    );

    let quit = handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(!quit);
    assert!(cache_a.exists());
    assert!(cache_b.exists());
    assert!(mit_dir.join("accounts").exists());
    assert!(mit_dir.join("cache").exists());
    assert!(extra_settings_file.exists());
    // Tall enough that the centered confirm dialog clears the settings list below it.
    let mut terminal = Terminal::new(TestBackend::new(100, 44)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");
    let action = match &app.account_action_dialog {
        Some(AccountActionDialog::SettingsConfirm { action }) => format!("{action:?}"),
        other => panic!("expected cache confirm dialog, got {other:?}"),
    };
    assert_eq!(action, "ClearCacheKeepAuth");
    assert!(!compact.contains("该操作不可恢复"), "{text}");

    let quit = handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(!quit);
    assert!(!cache_a.exists());
    assert!(!cache_b.exists());
    assert!(!mit_dir.join("accounts").exists());
    assert!(!mit_dir.join("cache").exists());
    assert!(!extra_settings_file.exists());
    assert!(auth_file.exists());
    assert!(app.devices.is_empty());
    assert!(app.property_cache.get_property("dev-1", 2, 1).is_none());
    assert!(matches!(app.boot_state, BootState::Loading));
    assert!(app.bootstrap_pending.is_some());
    assert!(app.logs.iter().any(|line| line.contains("已清理缓存")));

    let _ = fs::remove_dir_all(&home);
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
                pid: 0,

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
                pid: 0,

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
        crossterm::event::KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
    )
    .unwrap();
    assert!(!quit);
    assert_eq!(app.device_index, 0);
}

#[test]
fn device_viewport_keeps_window_anchor_when_moving_up_from_bottom_item() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let devices = (0..10)
        .map(|idx| Device {
            did: format!("dev-{idx}"),
            name: format!("{idx}"),
            model: "xiaomi.wifispeaker.lx04".to_string(),
            online: true,
            pid: 0,

            home_id: format!("cache-account:10{idx:02}"),
            home_name: format!("acc{idx}(10{idx:02})"),
            room_id: format!("room-{idx}"),
            room_name: "客厅".to_string(),
        })
        .collect::<Vec<_>>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: Vec::new(),
        account_index: 0,
        devices,
        device_index: 7,
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
    let mut terminal = Terminal::new(TestBackend::new(60, 7)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert_eq!(app.device_list_state.offset(), 4);

    let quit = handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(!quit);
    assert_eq!(app.device_index, 6);

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    let text = terminal_text(&terminal);
    assert!(text.contains("acc4"), "{text}");
    assert!(text.contains("acc5"), "{text}");
    assert!(text.contains("acc6"), "{text}");
    assert!(text.contains("acc7"), "{text}");
    assert!(!text.contains("acc0"), "{text}");
    assert!(!text.contains("acc1"), "{text}");
    assert!(!text.contains("acc2"), "{text}");
    assert!(!text.contains("acc3"), "{text}");
    assert!(!text.contains("account="), "{text}");
    assert_eq!(app.device_list_state.offset(), 4);
}

#[test]
fn clicking_accounts_row_selects_account() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let account_a = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-click-account-a",
        "deviceId": "mico.tui-click-account-a",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 1,
        "user": {"uid": "1001", "nickname": "A", "icon": "", "unionId": "union-a"}
    }));
    let account_b = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-click-account-b",
        "deviceId": "mico.tui-click-account-b",
        "state": "state-b",
        "accessToken": "token-b",
        "refreshToken": "refresh-b",
        "expiresTs": 1,
        "user": {"uid": "1002", "nickname": "B", "icon": "", "unionId": "union-b"}
    }));
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![account_a, account_b],
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
            row: 7,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();
    assert_eq!(app.account_index, 1);
    assert!(app.account_action_dialog.is_none());
}
