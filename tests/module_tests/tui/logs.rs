// Auto-split: shares the `tests` module scope (imports + helpers) of
// mod.rs via include!; do not add `use` here.

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
fn log_selection_clears_when_search_changes_visible_content() {
    let mut app = logs_tab_test_app(vec!["alpha boot complete", "beta sync done"]);
    app.language = Language::English;
    let terminal_area = ratatui::layout::Rect::new(0, 0, 80, 24);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    super::clear_selection_state();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
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
            column: log_message_start_column().saturating_add(4),
            row: log_first_row(terminal_area),
            modifiers: KeyModifiers::NONE,
        },
        terminal_area,
    )
    .unwrap();
    assert!(super::selected_surface().is_some());

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
    )
    .unwrap();
    for ch in "sync".chars() {
        handle_key(
            &mut app,
            crossterm::event::KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
        )
        .unwrap();
    }
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);

    assert!(text.contains("beta sync done"), "{text}");
    assert!(!text.contains("alpha boot complete"), "{text}");
    assert!(super::selected_surface().is_none());
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
