// Auto-split: shares the `tests` module scope (imports + helpers) of
// mod.rs via include!; do not add `use` here.

#[test]
fn account_and_logs_search_bars_render_with_bottom_border() {
    let terminal_area = ratatui::layout::Rect::new(0, 0, 80, 24);
    let [_tabs_area, content_area, _status_gap_area, _status_bar_area] =
        super::split_main_layout(terminal_area);

    for mut app in [
        accounts_tab_test_app(vec![test_account_with("1001", "Kitchen Account", "cn")]),
        logs_tab_test_app(vec!["visible log line"]),
    ] {
        app.language = Language::English;
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();

        let text = terminal_text(&terminal);
        let lines = text.lines().collect::<Vec<_>>();
        let search_row = content_area.y as usize;
        let border_row = content_area.y.saturating_add(1) as usize;
        assert!(lines[search_row].contains("/ Search:"), "{text}");
        assert!(!lines[search_row].starts_with("│"), "{text}");
        assert!(lines[border_row].contains("─"), "{text}");
        assert!(!lines[border_row].contains("Search"), "{text}");
    }
}

#[test]
fn push_message_dialog_shows_cursor_and_moves_with_left_right() {
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
            input: "hi".to_string(),
            cursor: 2,
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
    let text = terminal_text(&terminal);
    assert!(text.contains("hi"), "{text}");
    assert!(!text.contains("h|i"), "{text}");
    assert!(!text.contains("hi|"), "{text}");
    assert!(terminal_has_reversed_cell(&terminal));

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
    )
    .unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("hi"), "{text}");
    assert!(!text.contains("h|i"), "{text}");
    assert!(!text.contains("hi|"), "{text}");
    assert!(terminal_has_reversed_cell(&terminal));

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
    )
    .unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("hi"), "{text}");
    assert!(!text.contains("h|i"), "{text}");
    assert!(!text.contains("hi|"), "{text}");
    assert!(terminal_has_reversed_cell(&terminal));
}

#[test]
fn push_message_dialog_submits_text_for_selected_account_uid() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-push-message-submit",
        "deviceId": "mico.tui-push-message-submit",
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

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE),
    )
    .unwrap();
    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE),
    )
    .unwrap();
    let quit = handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(!quit);
    assert!(app.account_action_dialog.is_none());
    assert!(app.logs.iter().any(|line| {
        line.contains("push message") && line.contains("1001") && line.contains("hi")
    }));
}

#[test]
fn escaping_push_message_dialog_restores_previous_menu_selection() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-account-actions-escape",
        "deviceId": "mico.tui-account-actions-escape",
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

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();
    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
    )
    .unwrap();

    assert!(matches!(
        app.account_action_dialog,
        Some(AccountActionDialog::Menu { selected: 0 })
    ));
}

#[test]
fn account_action_menu_closes_on_click_away() {
    let mut app = app_with_single_readonly_prop_dialog();
    app.prop_dialog = None;
    app.account_action_dialog = Some(AccountActionDialog::Menu { selected: 0 });
    let terminal_area = ratatui::layout::Rect::new(0, 0, 120, 40);

    // A click outside the centered popup dismisses it.
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
        terminal_area,
    )
    .unwrap();
    assert!(app.account_action_dialog.is_none());
}

#[test]
fn account_action_menu_click_inside_keeps_open() {
    let mut app = app_with_single_readonly_prop_dialog();
    app.prop_dialog = None;
    app.account_action_dialog = Some(AccountActionDialog::Menu { selected: 0 });
    let terminal_area = ratatui::layout::Rect::new(0, 0, 120, 40);
    let popup = super::centered_rect(48, 34, terminal_area);

    // A click inside the popup does not dismiss it.
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: popup.x + popup.width / 2,
            row: popup.y + popup.height / 2,
            modifiers: KeyModifiers::NONE,
        },
        terminal_area,
    )
    .unwrap();
    assert!(app.account_action_dialog.is_some());
}

#[test]
fn preview_push_command_masks_long_params() {
    assert_eq!(
        format_preview_push_command("1001", "hellooo"),
        "mit push --uid 1001 \"...\""
    );
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
