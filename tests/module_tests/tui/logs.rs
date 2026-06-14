// Auto-split from the former monolithic tui.rs. Shares the `tests` module
// scope (imports + helpers) of mod.rs via include!; do not add `use` here.

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
fn format_prop_dialog_action_list_item_line_shows_only_action_name() {
    let action = ActionItem {
        siid: 5,
        aiid: 1,
        name: "开灯".to_string(),
        input_piids: vec![1, 2],
        input_labels: vec!["参数1".to_string(), "参数2".to_string()],
        input_props: Vec::new(),
    };

    assert_eq!(
        format_prop_dialog_action_list_item_line(&action, true),
        "> 开灯"
    );
    assert_eq!(
        format_prop_dialog_action_list_item_line(&action, false),
        "  开灯"
    );
}

#[test]
fn logs_tab_c_clears_log_buffer_and_scroll_offset() {
    let mut app = logs_tab_test_app(vec!["alpha boot complete", "beta sync done"]);
    app.log_scroll_offset = 7;

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
    )
    .unwrap();

    assert!(app.logs.is_empty());
    assert_eq!(app.log_scroll_offset, 0);
}

#[test]
fn log_buffer_keeps_latest_1000_entries_fifo() {
    let mut app = logs_tab_test_app(Vec::new());

    for idx in 0..1005 {
        app.log(format!("log-{idx:04}"));
    }

    assert_eq!(app.logs.len(), 1000);
    assert_eq!(
        app.logs.front().map(|line| super::log_entry_message(line)),
        Some("log-0005")
    );
    assert_eq!(
        app.logs.back().map(|line| super::log_entry_message(line)),
        Some("log-1004")
    );
}

#[test]
fn logs_tab_renders_timestamp_before_each_log_line() {
    let mut app = logs_tab_test_app(vec!["mqtt connected"]);
    app.language = Language::English;
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    let text = terminal_text(&terminal);
    let line = text
        .lines()
        .find(|line| line.contains("mqtt connected"))
        .unwrap_or_else(|| panic!("{text}"));
    let prefix = line
        .split("mqtt connected")
        .next()
        .unwrap_or_default()
        .trim_start();
    assert_clock_timestamp_prefix(prefix, &text);
}

#[test]
fn logs_tab_mouse_wheel_scrolls_to_older_entries() {
    let logs = (0..30)
        .map(|idx| format!("log-{idx:02}"))
        .collect::<Vec<_>>();
    let mut app = logs_tab_test_app(logs.iter().map(String::as_str).collect());
    app.language = Language::English;
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let before = terminal_text(&terminal);
    assert!(before.contains("log-29"), "{before}");
    assert!(!before.contains("log-00"), "{before}");

    for _ in 0..20 {
        handle_mouse(
            &mut app,
            crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::ScrollDown,
                column: 2,
                row: 5,
                modifiers: KeyModifiers::NONE,
            },
            ratatui::layout::Rect::new(0, 0, 80, 24),
        )
        .unwrap();
    }

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let after = terminal_text(&terminal);
    assert!(after.contains("log-00"), "{after}");
    assert!(!after.contains("log-29"), "{after}");
}

#[test]
fn logs_tab_new_log_does_not_shift_scrolled_view_window() {
    let logs = (0..30)
        .map(|idx| format!("log-{idx:02}"))
        .collect::<Vec<_>>();
    let mut app = logs_tab_test_app(logs.iter().map(String::as_str).collect());
    app.language = Language::English;
    let terminal_area = ratatui::layout::Rect::new(0, 0, 80, 24);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

    app.log_scroll_offset = 5;
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let before = terminal_text(&terminal);
    assert!(
        top_log_line(&terminal, terminal_area).contains("log-24"),
        "{before}"
    );

    app.log(format!("inserted {}", "x".repeat(160)));
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let after = terminal_text(&terminal);
    assert!(
        top_log_line(&terminal, terminal_area).contains("log-24"),
        "{after}"
    );
    assert!(
        !top_log_line(&terminal, terminal_area).contains("inserted"),
        "{after}"
    );
}

#[test]
fn logs_tab_new_log_does_not_shift_selected_view_window() {
    let logs = (0..30)
        .map(|idx| format!("log-{idx:02}"))
        .collect::<Vec<_>>();
    let mut app = logs_tab_test_app(logs.iter().map(String::as_str).collect());
    app.language = Language::English;
    let terminal_area = ratatui::layout::Rect::new(0, 0, 80, 24);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    super::clear_selection_state();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let before = terminal_text(&terminal);
    assert!(
        top_log_line(&terminal, terminal_area).contains("log-29"),
        "{before}"
    );

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: log_message_start_column(),
            row: log_first_row(terminal_area),
            modifiers: KeyModifiers::NONE,
        },
        terminal_area,
    )
    .unwrap();
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Left),
            column: log_message_start_column().saturating_add(6),
            row: log_first_row(terminal_area),
            modifiers: KeyModifiers::NONE,
        },
        terminal_area,
    )
    .unwrap();

    app.log("new selected-anchor log");
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let after = terminal_text(&terminal);
    assert!(
        top_log_line(&terminal, terminal_area).contains("log-29"),
        "{after}"
    );
    assert!(
        !top_log_line(&terminal, terminal_area).contains("new selected-anchor log"),
        "{after}"
    );

    super::clear_selection_state();
}

#[test]
fn logs_tab_overflow_renders_scrollbar_thumb_that_moves() {
    let logs = (0..30)
        .map(|idx| format!("log-{idx:02}"))
        .collect::<Vec<_>>();
    let mut app = logs_tab_test_app(logs.iter().map(String::as_str).collect());
    app.language = Language::English;
    let terminal_area = ratatui::layout::Rect::new(0, 0, 80, 24);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let before = log_scrollbar_thumb_row(&terminal, terminal_area)
        .unwrap_or_else(|| panic!("{}", terminal_text(&terminal)));

    for _ in 0..20 {
        handle_mouse(
            &mut app,
            crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::ScrollDown,
                column: 2,
                row: 5,
                modifiers: KeyModifiers::NONE,
            },
            terminal_area,
        )
        .unwrap();
    }

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let after = log_scrollbar_thumb_row(&terminal, terminal_area)
        .unwrap_or_else(|| panic!("{}", terminal_text(&terminal)));
    assert!(after > before, "before={before} after={after}");
}

#[test]
fn logs_tab_leaves_blank_margin_before_scrollbar() {
    let logs = (0..30)
        .map(|idx| format!("log-{idx:02} {}", "x".repeat(90)))
        .collect::<Vec<_>>();
    let mut app = logs_tab_test_app(logs.iter().map(String::as_str).collect());
    app.language = Language::English;
    let terminal_area = ratatui::layout::Rect::new(0, 0, 80, 24);
    let [_tabs_area, content_area, _status_gap_area, _status_bar_area] =
        super::split_main_layout(terminal_area);
    let [_search_area, _search_border_area, list_area] =
        super::searchable_main_layout(content_area);
    let margin_x = list_area
        .x
        .saturating_add(list_area.width.saturating_sub(2));
    let scrollbar_x = list_area
        .x
        .saturating_add(list_area.width.saturating_sub(1));
    let first_log_row = list_area.y;
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(margin_x, first_log_row)].symbol(), " ");
    assert_eq!(
        buffer[(scrollbar_x, first_log_row)].symbol(),
        super::LOG_SCROLLBAR_THUMB
    );
}

#[test]
fn log_scrollbar_thumb_height_stays_constant_across_positions() {
    let logs = (0..20)
        .map(|idx| format!("log-{idx:02}"))
        .collect::<Vec<_>>();
    let mut app = logs_tab_test_app(logs.iter().map(String::as_str).collect());
    app.language = Language::English;
    let terminal_area = ratatui::layout::Rect::new(0, 0, 80, 24);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let heights = (0..=4)
        .map(|offset| {
            app.log_scroll_offset = offset;
            terminal.draw(|frame| draw(frame, &mut app)).unwrap();
            log_scrollbar_thumb_height(&terminal, terminal_area)
        })
        .collect::<Vec<_>>();

    assert!(
        heights.iter().all(|height| *height > 0)
            && heights.windows(2).all(|pair| pair[0] == pair[1]),
        "{heights:?}"
    );
}

#[test]
fn dragging_log_scrollbar_scrolls_to_pointer_position() {
    let logs = (0..30)
        .map(|idx| format!("log-{idx:02}"))
        .collect::<Vec<_>>();
    let mut app = logs_tab_test_app(logs.iter().map(String::as_str).collect());
    app.language = Language::English;
    let terminal_area = ratatui::layout::Rect::new(0, 0, 80, 24);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let Some((scrollbar_x, thumb_row, bottom_row)) =
        log_scrollbar_drag_points(&terminal, terminal_area)
    else {
        panic!("{}", terminal_text(&terminal));
    };

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: scrollbar_x,
            row: thumb_row,
            modifiers: KeyModifiers::NONE,
        },
        terminal_area,
    )
    .unwrap();
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Left),
            column: scrollbar_x,
            row: bottom_row,
            modifiers: KeyModifiers::NONE,
        },
        terminal_area,
    )
    .unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("log-00"), "{text}");
    assert!(!text.contains("log-29"), "{text}");
}

#[test]
fn releasing_log_scrollbar_updates_to_release_position() {
    let logs = (0..30)
        .map(|idx| format!("log-{idx:02}"))
        .collect::<Vec<_>>();
    let mut app = logs_tab_test_app(logs.iter().map(String::as_str).collect());
    app.language = Language::English;
    let terminal_area = ratatui::layout::Rect::new(0, 0, 80, 24);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let Some((scrollbar_x, thumb_row, bottom_row)) =
        log_scrollbar_drag_points(&terminal, terminal_area)
    else {
        panic!("{}", terminal_text(&terminal));
    };
    let middle_row = thumb_row.saturating_add((bottom_row.saturating_sub(thumb_row)) / 2);

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: scrollbar_x,
            row: thumb_row,
            modifiers: KeyModifiers::NONE,
        },
        terminal_area,
    )
    .unwrap();
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Left),
            column: scrollbar_x,
            row: middle_row,
            modifiers: KeyModifiers::NONE,
        },
        terminal_area,
    )
    .unwrap();
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Up(crossterm::event::MouseButton::Left),
            column: scrollbar_x,
            row: bottom_row,
            modifiers: KeyModifiers::NONE,
        },
        terminal_area,
    )
    .unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("log-00"), "{text}");
}

#[test]
fn long_log_line_wraps_in_log_viewer() {
    let mut app = logs_tab_test_app(vec!["abcdefghijklmnopqrstuvwxyz"]);
    app.language = Language::English;
    let mut terminal = Terminal::new(TestBackend::new(24, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let first_line = text
        .lines()
        .find(|line| line.contains("abcdefghijklm"))
        .unwrap_or_else(|| panic!("{text}"));
    let prefix = first_line
        .split("abcdefghijklm")
        .next()
        .unwrap_or_default()
        .trim_start();
    assert_clock_timestamp_prefix(prefix, &text);
    assert!(text.contains("nopqrstuvwxyz"), "{text}");
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
fn opening_device_dialog_shows_schema_with_placeholders_while_loading() {
    let home = make_temp_dir("tui-devices-loading-open");
    let specs_dir = home.join(".mit").join("cache").join("specs");
    fs::create_dir_all(&specs_dir).unwrap();
    fs::create_dir_all(specs_dir.join("models")).unwrap();
    fs::write(
        specs_dir
            .join("models")
            .join("xiaomi.wifispeaker.lx04.json"),
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
    };

    let quit = handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(!quit);
    assert!(app.prop_dialog.is_some());
    let dialog = app.prop_dialog.as_ref().unwrap();
    assert!(dialog.loading);
    assert_eq!(dialog.items.len(), 3);
    assert_eq!(
        super::visible_prop_dialog_tab_titles(dialog, Language::Chinese),
        vec![
            "1:修改参数".to_string(),
            "2:操作记录".to_string(),
            "3:统计".to_string(),
        ]
    );

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("Power"), "{text}");
    assert!(text.contains("= -"), "{text}");
    assert!(!text.contains("Loading properties"), "{text}");

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn dragging_logs_text_autocopies_selection() {
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
        logs: VecDeque::from(["alpha".to_string(), "beta".to_string()]),
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
    let clip_file = make_temp_dir("tui-log-drag-copy").join("clipboard.txt");
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

    let copied = fs::read_to_string(&clip_file).unwrap();
    assert_eq!(copied, "beta");

    std::env::remove_var("MIT_TEST_CLIPBOARD_FILE");
    let _ = fs::remove_file(&clip_file);
}

#[test]
fn log_selection_survives_scroll_when_visible_content_does_not_change() {
    let mut app = logs_tab_test_app(vec!["alpha", "beta", "gamma"]);
    app.language = Language::English;
    let terminal_area = ratatui::layout::Rect::new(0, 0, 80, 24);
    super::clear_selection_state();

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: log_message_start_column(),
            row: log_first_row(terminal_area),
            modifiers: KeyModifiers::NONE,
        },
        terminal_area,
    )
    .unwrap();
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Left),
            column: log_message_start_column().saturating_add(5),
            row: log_first_row(terminal_area),
            modifiers: KeyModifiers::NONE,
        },
        terminal_area,
    )
    .unwrap();
    assert!(super::selected_surface().is_some());

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollDown,
            column: 2,
            row: log_first_row(terminal_area),
            modifiers: KeyModifiers::NONE,
        },
        terminal_area,
    )
    .unwrap();

    assert!(super::selected_surface().is_some());
}

#[test]
fn dragging_beyond_last_log_still_copies_all_logs() {
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
        logs: VecDeque::from(["alpha".to_string(), "beta".to_string()]),
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
    let clip_file = make_temp_dir("tui-log-select-all-copy").join("clipboard.txt");
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
            column: 79,
            row: 20,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Up(crossterm::event::MouseButton::Left),
            column: 79,
            row: 20,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();

    let copied = fs::read_to_string(&clip_file).unwrap();
    assert_timestamped_log_lines(&copied, &["beta", "alpha"]);

    std::env::remove_var("MIT_TEST_CLIPBOARD_FILE");
    let _ = fs::remove_file(&clip_file);
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
        crossterm::event::KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
    )
    .unwrap();

    let dialog = app.prop_dialog.as_ref().unwrap();
    assert!(dialog.refreshing);
    assert!(dialog.refresh_rx.is_some());
}

#[test]
fn prop_dialog_number_shortcuts_switch_tabs() {
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
                        name: "ReadOnlyVolume".to_string(),
                        format: "uint8".to_string(),
                        writable: false,
                        value_options: Vec::new(),
                    },
                    value: json!(22),
                },
                raw_device_logs_item(json!({"records": []})),
                raw_device_statistics_item(json!({"statistics": []})),
            ],
            selected: 0,
            active_tab: PropDialogTab::Writable,
            writable_selected: 0,
            readonly_selected: 1,
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
        crossterm::event::KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
    )
    .unwrap();
    assert!(matches!(
        app.prop_dialog.as_ref().map(|dialog| dialog.active_tab),
        Some(PropDialogTab::Actions)
    ));

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE),
    )
    .unwrap();
    assert!(matches!(
        app.prop_dialog.as_ref().map(|dialog| dialog.active_tab),
        Some(PropDialogTab::Writable)
    ));

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE),
    )
    .unwrap();
    assert!(matches!(
        app.prop_dialog.as_ref().map(|dialog| dialog.active_tab),
        Some(PropDialogTab::ReadOnly)
    ));

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE),
    )
    .unwrap();
    assert!(matches!(
        app.prop_dialog.as_ref().map(|dialog| dialog.active_tab),
        Some(PropDialogTab::Logs)
    ));

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('5'), KeyModifiers::NONE),
    )
    .unwrap();
    assert!(matches!(
        app.prop_dialog.as_ref().map(|dialog| dialog.active_tab),
        Some(PropDialogTab::Statistics)
    ));
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

    app.process_prop_dialog_loading();

    let dialog = app.prop_dialog.as_ref().unwrap();
    assert!(!dialog.loading);
    assert_eq!(dialog.selected, 1);
    assert_eq!(dialog.writable_selected, 1);
}

#[test]
fn draw_property_dialog_shows_writable_and_read_only_sections() {
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
                        name: "扬声器服务 / 电源".to_string(),
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
                        name: "扬声器服务 / 只读音量".to_string(),
                        format: "uint8".to_string(),
                        writable: false,
                        value_options: Vec::new(),
                    },
                    value: json!(20),
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
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");
    assert!(text.contains("dev-1"), "{text}");
    assert!(!compact.contains("设备属性"), "{text}");
    assert!(text.contains("扬 声 器 服 务  / 电 源"), "{text}");
    assert!(!text.contains("只 读 音 量"), "{text}");
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
fn prop_dialog_number_shortcuts_respect_hidden_actions_tab() {
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
                    piid: 2,
                    name: "ReadOnlyVolume".to_string(),
                    format: "uint8".to_string(),
                    writable: false,
                    value_options: Vec::new(),
                },
                value: json!(22),
            }],
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

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
    )
    .unwrap();
    assert!(matches!(
        app.prop_dialog.as_ref().map(|dialog| dialog.active_tab),
        Some(PropDialogTab::ReadOnly)
    ));

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE),
    )
    .unwrap();
    assert!(matches!(
        app.prop_dialog.as_ref().map(|dialog| dialog.active_tab),
        Some(PropDialogTab::ReadOnly)
    ));
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
fn prop_dialog_applies_cached_mips_property_updates() {
    let mut app = test_app_with_prop_dialog(PropDialog {
        device_did: "dev-1".to_string(),
        device_name: "dev-1".to_string(),
        account_uid: "1001".to_string(),
        items: vec![ToggleItem {
            prop: PropItem {
                siid: 2,
                piid: 1,
                name: "switch".to_string(),
                format: "bool".to_string(),
                writable: true,
                value_options: Vec::new(),
            },
            value: Value::Bool(false),
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
    });
    app.property_cache
        .set_property("dev-1".to_string(), 2, 1, json!(true));

    app.apply_cached_prop_dialog_updates();

    let dialog = app.prop_dialog.as_ref().unwrap();
    assert_eq!(dialog.items[0].value, Value::Bool(true));
}

#[test]
fn process_cloud_mips_messages_logs_messages_and_errors() {
    let _guard = env_guard();
    let dialog = PropDialog {
        device_did: "dev-1".to_string(),
        device_name: "dev-1".to_string(),
        account_uid: "1001".to_string(),
        items: Vec::new(),
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
    };
    let mut app = test_app_with_prop_dialog(dialog);
    let (tx, rx) = mpsc::channel();
    tx.send(crate::mips_cloud::CloudMipsStatus::EventReceived {
        direction: "incoming".to_string(),
        summary: "ConnAck".to_string(),
    })
    .unwrap();
    tx.send(crate::mips_cloud::CloudMipsStatus::EventReceived {
        direction: "outgoing".to_string(),
        summary: "PingReq".to_string(),
    })
    .unwrap();
    tx.send(crate::mips_cloud::CloudMipsStatus::EventReceived {
        direction: "incoming".to_string(),
        summary: "PingResp(PingResp)".to_string(),
    })
    .unwrap();
    tx.send(crate::mips_cloud::CloudMipsStatus::MessageReceived {
        topic: "device/dev-1/up/properties_changed/2/1".to_string(),
        payload_len: 42,
    })
    .unwrap();
    tx.send(crate::mips_cloud::CloudMipsStatus::PropertyApplied {
        did: "dev-1".to_string(),
        siid: 2,
        piid: 1,
    })
    .unwrap();
    tx.send(crate::mips_cloud::CloudMipsStatus::Error {
        message: "mqtt auth failed".to_string(),
    })
    .unwrap();
    drop(tx);
    {
        let mut runtime = super::cloud_mips_runtime().lock().unwrap();
        *runtime = Some(super::CloudMipsRuntime {
            key: "test-runtime".to_string(),
            _handles: Vec::new(),
            rx,
            last_mqtt_response_at: None,
            last_ping_req_at: None,
            last_ping_resp_at: None,
        });
    }

    app.process_cloud_mips_messages();
    let logs = app.logs.iter().cloned().collect::<Vec<_>>().join("\n");
    assert!(logs.contains("cloud MIPS mqtt incoming: ConnAck"));
    assert!(
        logs.contains("cloud MIPS message: topic=device/dev-1/up/properties_changed/2/1 bytes=42")
    );
    assert!(logs.contains("cloud MIPS property update: did=dev-1 siid=2 piid=1"));
    assert!(logs.contains("cloud MIPS error: mqtt auth failed"));
    {
        let runtime = super::cloud_mips_runtime().lock().unwrap();
        let runtime = runtime.as_ref().unwrap();
        assert!(runtime.last_mqtt_response_at.is_some());
        assert!(runtime.last_ping_req_at.is_some());
        assert!(runtime.last_ping_resp_at.is_some());
    }

    let mut runtime = super::cloud_mips_runtime().lock().unwrap();
    *runtime = None;
}

#[test]
fn refresh_cloud_mips_listeners_logs_when_no_eligible_device_groups() {
    let _guard = env_guard();
    std::env::remove_var("MIT_DISABLE_CLOUD_MIPS");
    std::env::set_var("MIT_ENABLE_CLOUD_MIPS_IN_TESTS", "1");

    let mut app = test_app_with_prop_dialog(PropDialog {
        device_did: "dev-1".to_string(),
        device_name: "dev-1".to_string(),
        account_uid: "1001".to_string(),
        items: Vec::new(),
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
    });

    app.refresh_cloud_mips_listeners();
    std::env::remove_var("MIT_ENABLE_CLOUD_MIPS_IN_TESTS");

    let logs = app.logs.iter().cloned().collect::<Vec<_>>().join("\n");
    assert!(logs.contains(
        "cloud MIPS not started: no eligible OAuth account/device groups \
         (accounts=1, oauth_accounts=1, offline_accounts=0, devices=0, tagged_devices=0)"
    ));

    let mut runtime = super::cloud_mips_runtime().lock().unwrap();
    *runtime = None;
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
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");
    assert!(compact.contains("唤醒"), "{text}");
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
fn format_prop_value_for_dialog_decodes_backslash_x_utf8_sequences() {
    let encoded = json!("\\xe6\\x96\\xb0\\xe9\\x98\\xb3");
    assert_eq!(format_prop_value_for_dialog(&encoded), "新阳");
}

#[test]
fn format_prop_value_for_dialog_renders_dash_for_negative_code_object() {
    let value = json!({"code": -704042011, "did": "12345"});
    assert_eq!(format_prop_value_for_dialog(&value), "-");
}
