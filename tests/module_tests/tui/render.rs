// Auto-split from the former monolithic tui.rs. Shares the `tests` module
// scope (imports + helpers) of mod.rs via include!; do not add `use` here.

#[test]
fn render_text_input_line_uses_reversed_block_cursor() {
    let textarea = single_line_textarea("true", 2, true);
    assert_eq!(textarea.lines(), ["true"]);
    assert_eq!(textarea.cursor(), (0, 2));
}

#[test]
fn render_text_input_line_shows_cursor_for_empty_input() {
    let textarea = single_line_textarea("", 0, true);
    assert_eq!(textarea.lines(), [""]);
    assert_eq!(textarea.cursor(), (0, 0));
}

#[test]
fn render_text_input_line_keeps_end_cursor_on_last_char() {
    let textarea = single_line_textarea("true", 4, true);
    assert_eq!(textarea.lines(), ["true"]);
    assert_eq!(textarea.cursor(), (0, 4));
}

#[test]
fn draw_shows_loading_splash_while_boot_loading() {
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
        boot_spinner_index: 1,
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
    assert!(text.contains("Loading devices"));
    assert!(text.contains("mit"));
}

#[test]
fn copy_status_badge_uses_chinese_text_and_expires_in_one_second() {
    let mut logs = VecDeque::new();
    logs.push_back(format!("{}{}", super::FOOTER_COPY_LOG_PREFIX, 10_000));
    let app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: Vec::new(),
        account_index: 0,
        devices: Vec::new(),
        device_index: 0,
        logs,
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
        local_transport_tx: mpsc::channel::<LocalTransportRefreshMessage>().0,
        local_transport_rx: mpsc::channel::<LocalTransportRefreshMessage>().1,
        auth_flow_generation: 0,
        auth_flow_tx: mpsc::channel::<AuthFlowMessage>().0,
        auth_flow_rx: mpsc::channel::<AuthFlowMessage>().1,
        offline_account_uids: HashSet::new(),
        boot_state: BootState::Ready,
        boot_spinner_index: 0,
        bootstrap_generation: 0,
        bootstrap_pending: None,
        bootstrap_tx: mpsc::channel::<BootstrapMessage>().0,
        bootstrap_rx: mpsc::channel::<BootstrapMessage>().1,
        property_cache: Arc::new(PropertyCache::new()),
        language: Language::Chinese,
        auto_subscribe_device_status: true,
        settings_selected: 0,
    };

    let line = super::footer_line(&app, 10_999);
    assert_eq!(line.spans.len(), 2);
    assert_eq!(line.spans[1].content.as_ref(), " [已复制]");

    let no_badge = super::footer_line(&app, 11_000);
    assert_eq!(no_badge.spans.len(), 1);
}

#[test]
fn copy_status_badge_uses_blue_style() {
    let mut logs = VecDeque::new();
    logs.push_back(format!("{}{}", super::FOOTER_COPY_LOG_PREFIX, 10_000));
    let app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: Vec::new(),
        account_index: 0,
        devices: Vec::new(),
        device_index: 0,
        logs,
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
        local_transport_tx: mpsc::channel::<LocalTransportRefreshMessage>().0,
        local_transport_rx: mpsc::channel::<LocalTransportRefreshMessage>().1,
        auth_flow_generation: 0,
        auth_flow_tx: mpsc::channel::<AuthFlowMessage>().0,
        auth_flow_rx: mpsc::channel::<AuthFlowMessage>().1,
        offline_account_uids: HashSet::new(),
        boot_state: BootState::Ready,
        boot_spinner_index: 0,
        bootstrap_generation: 0,
        bootstrap_pending: None,
        bootstrap_tx: mpsc::channel::<BootstrapMessage>().0,
        bootstrap_rx: mpsc::channel::<BootstrapMessage>().1,
        property_cache: Arc::new(PropertyCache::new()),
        language: Language::Chinese,
        auto_subscribe_device_status: true,
        settings_selected: 0,
    };
    let line = super::footer_line(&app, 10_999);
    assert_eq!(line.spans[1].style.fg, Some(Color::Blue));
}
