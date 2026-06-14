// Auto-split from the former monolithic tui.rs. Shares the `tests` module
// scope (imports + helpers) of mod.rs via include!; do not add `use` here.

#[test]
fn preview_push_command_masks_long_params() {
    assert_eq!(
        format_preview_push_command("1001", "hellooo"),
        "mit push --uid 1001 \"...\""
    );
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
fn account_list_row_marks_missing_tokens_independently() {
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-account-status-missing",
        "deviceId": "mico.tui-account-status-missing",
        "state": "state-a",
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));

    let row = account_page::account_list_row(&account, false, Language::Chinese);

    assert_eq!(row.xiaomi_status, "未登录");
    assert_eq!(row.mijia_status, "未登录");
}

#[test]
fn format_device_list_item_orders_columns_room_name_category_account() {
    let device = Device {
        did: "dev-1".to_string(),
        name: "living-room".to_string(),
        model: "xiaomi.wifispeaker.lx04".to_string(),
        online: true,

        home_id: "home-1".to_string(),
        home_name: "我家".to_string(),
        room_id: "room-1".to_string(),
        room_name: "客厅".to_string(),
    };

    let line = format_device_list_item(&device, "speaker", "账号A(1001)");
    let parts = [
        line.find("客厅").unwrap_or_default(),
        line.find("living-room").unwrap_or_default(),
        line.find("speaker").unwrap_or_default(),
        line.find("账号A(1001)").unwrap_or_default(),
    ];

    assert!(
        parts[0] < parts[1] && parts[1] < parts[2] && parts[2] < parts[3],
        "{line}"
    );
}

#[test]
fn draw_accounts_selected_row_uses_reversed_style() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let account_a = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-account-a",
        "deviceId": "mico.tui-account-a",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 1,
        "user": {"uid": "1001", "nickname": "A", "icon": "", "unionId": "union-a"}
    }));
    let account_b = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-account-b",
        "deviceId": "mico.tui-account-b",
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

    assert!(terminal_has_reversed_cell(&terminal));
}

#[test]
fn draw_accounts_scrolls_to_keep_active_row_visible() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let accounts = (0..12)
        .map(|idx| {
            normalize_account(json!({
                "region": "cn",
                "redirectUri": "http://127.0.0.1:8000/login_redirect",
                "uuid": format!("tui-scroll-account-{idx}"),
                "deviceId": format!("mico.tui-scroll-account-{idx}"),
                "state": format!("state-{idx}"),
                "accessToken": format!("token-{idx}"),
                "refreshToken": format!("refresh-{idx}"),
                "expiresTs": 1,
                "user": {
                    "uid": format!("10{idx:02}"),
                    "nickname": format!("acc{idx}"),
                    "icon": "",
                    "unionId": format!("union-{idx}")
                }
            }))
        })
        .collect::<Vec<_>>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts,
        account_index: 10,
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
    let mut terminal = Terminal::new(TestBackend::new(60, 8)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    let text = terminal_text(&terminal);
    assert!(text.contains("acc10"), "{text}");
    assert!(!text.contains("acc0"), "{text}");
}

#[test]
fn pressing_enter_on_accounts_tab_opens_account_action_dialog() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-account-actions",
        "deviceId": "mico.tui-account-actions",
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
    };

    let quit = handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(!quit);

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    super::clear_selection_state();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");
    assert!(compact.contains("推送消息"), "{text}");
    assert!(compact.contains("重新登录(小米)"), "{text}");
    assert!(compact.contains("重新登录(米家)"), "{text}");
    assert!(compact.contains("登出"), "{text}");
    assert!(!text.contains("view-device"));
}

#[test]
fn account_action_menu_mouse_wheel_changes_selected_item() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-account-actions-wheel",
        "deviceId": "mico.tui-account-actions-wheel",
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
        account_action_dialog: Some(AccountActionDialog::Menu { selected: 0 }),
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
    let area = ratatui::layout::Rect::new(0, 0, 80, 24);
    let popup = super::centered_rect(48, 34, area);

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollDown,
            column: popup.x + 2,
            row: popup.y + 1,
            modifiers: KeyModifiers::NONE,
        },
        area,
    )
    .unwrap();
    assert!(matches!(
        app.account_action_dialog,
        Some(AccountActionDialog::Menu { selected: 1 })
    ));

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollUp,
            column: popup.x + 2,
            row: popup.y + 1,
            modifiers: KeyModifiers::NONE,
        },
        area,
    )
    .unwrap();
    assert!(matches!(
        app.account_action_dialog,
        Some(AccountActionDialog::Menu { selected: 0 })
    ));
}

#[test]
fn clicking_selected_account_action_executes_it() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-account-actions-click",
        "deviceId": "mico.tui-account-actions-click",
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
        account_action_dialog: Some(AccountActionDialog::Menu { selected: 0 }),
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
    let area = ratatui::layout::Rect::new(0, 0, 80, 24);
    let popup = super::centered_rect(48, 34, area);

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: popup.x + 2,
            row: popup.y + 1,
            modifiers: KeyModifiers::NONE,
        },
        area,
    )
    .unwrap();

    assert!(matches!(
        app.account_action_dialog,
        Some(AccountActionDialog::PushMessage { .. })
    ));
}

#[test]
fn push_message_action_opens_input_dialog() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-push-message",
        "deviceId": "mico.tui-push-message",
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
        account_action_dialog: Some(AccountActionDialog::Menu { selected: 0 }),
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
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(!quit);
    assert!(matches!(
        app.account_action_dialog,
        Some(AccountActionDialog::PushMessage {
            ref uid,
            ref input,
            cursor,
            ..
        }) if uid == "1001" && input.is_empty() && cursor == 0
    ));
}

#[test]
fn push_message_cursor_row_stays_stable_when_typing_first_char() {
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
        account_action_dialog: Some(AccountActionDialog::PushMessage {
            uid: "1001".to_string(),
            input: String::new(),
            cursor: 0,
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

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let row_before = terminal_first_reversed_cell_row(&terminal).unwrap();

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
    )
    .unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let row_after = terminal_first_reversed_cell_row(&terminal).unwrap();

    assert_eq!(row_before, row_after);
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

#[test]
fn selected_push_message_textarea_text_uses_selection_background() {
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
        account_action_dialog: Some(AccountActionDialog::PushMessage {
            uid: "1001".to_string(),
            input: "hello world".to_string(),
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

    super::clear_selection_state();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let textarea_area =
        super::push_message_textarea_area(ratatui::layout::Rect::new(0, 0, 80, 24), "hello world");
    let column = textarea_area.x;
    let row = textarea_area.y;

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
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Left),
            column: column + 5,
            row,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Up(crossterm::event::MouseButton::Left),
            column: column + 5,
            row,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();

    let selection = super::selected_surface().expect("textarea selection should persist");
    assert_eq!(
        selection.snapshot.surface,
        super::SelectionSurface::PushMessageInput
    );
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let buffer = terminal.backend().buffer();
    let has_selection_bg = (0..buffer.area.height)
        .any(|y| (0..buffer.area.width).any(|x| buffer[(x, y)].bg == Color::DarkGray));
    assert!(has_selection_bg);
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
