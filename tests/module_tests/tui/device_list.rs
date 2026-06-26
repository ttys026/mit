// Auto-split: shares the `tests` module scope (imports + helpers) of
// mod.rs via include!; do not add `use` here.

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
fn format_device_list_item_shows_room_column() {
    let device = Device {
        did: "dev-1".to_string(),
        name: "living-room".to_string(),
        model: "xiaomi.wifispeaker.lx04".to_string(),
        online: true,
        pid: 0,

        home_id: "home-1".to_string(),
        home_name: "我家".to_string(),
        room_id: "room-1".to_string(),
        room_name: "客厅".to_string(),
    };

    let line = format_device_list_item(&device, "speaker", "账号A(1001)");

    assert!(line.starts_with("客厅"), "{line}");
    assert!(line.contains("living-room"), "{line}");
    assert!(line.contains("speaker"), "{line}");
    assert!(line.contains("账号A(1001)"), "{line}");
    assert!(!line.contains("account="), "{line}");
    assert!(!line.contains("[本地]"), "{line}");
    assert!(!line.contains("[远程]"), "{line}");
}

#[test]
fn format_device_list_item_does_not_show_mode_field() {
    let device = Device {
        did: "dev-1".to_string(),
        name: "living-room".to_string(),
        model: "xiaomi.wifispeaker.lx04".to_string(),
        online: true,
        pid: 0,

        home_id: "home-1".to_string(),
        home_name: "我家".to_string(),
        room_id: "room-1".to_string(),
        room_name: "客厅".to_string(),
    };

    let line = format_device_list_item(&device, "speaker", "账号A(1001)");

    assert!(line.starts_with("客厅"), "{line}");
    assert!(line.contains("living-room"), "{line}");
    assert!(line.contains("speaker"), "{line}");
    assert!(line.contains("账号A(1001)"), "{line}");
    assert!(!line.contains("account="), "{line}");
    assert!(!line.contains("[本地]"), "{line}");
    assert!(!line.contains("[远程]"), "{line}");
}

#[test]
fn format_device_list_header_contains_column_names() {
    let header = format_device_list_header(Language::Chinese);
    assert!(
        header.starts_with(super::device_list_header_titles(Language::Chinese)[0]),
        "{header}"
    );
    assert!(
        header.contains(super::device_list_header_titles(Language::Chinese)[1]),
        "{header}"
    );
    assert!(
        header.contains(super::device_list_header_titles(Language::Chinese)[2]),
        "{header}"
    );
    assert!(
        header.contains(super::device_list_header_titles(Language::Chinese)[3]),
        "{header}"
    );
}

#[test]
fn format_device_list_item_caps_device_name_column_to_ten_chinese_chars_width() {
    let device = Device {
        did: "dev-1".to_string(),
        name: "A very long device name".to_string(),
        model: "xiaomi.wifispeaker.lx04".to_string(),
        online: true,
        pid: 0,

        home_id: "home-1".to_string(),
        home_name: "我家".to_string(),
        room_id: "room-1".to_string(),
        room_name: "客厅".to_string(),
    };

    let row = device_list_row(&device.name, "sp", &device.room_name, "a1");
    let columns =
        compute_device_list_columns(std::slice::from_ref(&row), usize::MAX, Language::Chinese);
    let line = format_device_list_item_with_columns(&row, columns);
    assert_eq!(columns.name, 20);
    assert_eq!(
        UnicodeWidthStr::width(line.as_str()),
        columns.total_width(),
        "{line}"
    );
    assert!(line.contains("..."), "{line}");
}

#[test]
fn format_device_list_item_uses_longest_value_plus_one_for_columns() {
    let device = Device {
        did: "dev-1".to_string(),
        name: "name".to_string(),
        model: "xiaomi.wifispeaker.lx04".to_string(),
        online: true,
        pid: 0,

        home_id: "home-1".to_string(),
        home_name: "我家".to_string(),
        room_id: "room-1".to_string(),
        room_name: "客厅".to_string(),
    };

    let category = "cat";
    let account_label = "acc";
    let row = device_list_row(&device.name, category, &device.room_name, account_label);
    let columns =
        compute_device_list_columns(std::slice::from_ref(&row), usize::MAX, Language::Chinese);
    let line = format_device_list_item_with_columns(&row, columns);
    assert!(line.starts_with("客厅"), "{line}");
    assert!(line.contains("name"), "{line}");
    assert!(line.contains("cat"), "{line}");
    assert!(line.contains("acc"), "{line}");
    assert_eq!(
        UnicodeWidthStr::width(line.as_str()),
        columns.total_width(),
        "{line}"
    );
}

#[test]
fn computed_device_list_columns_match_longest_plus_one_with_name_cap() {
    let rows = vec![
        device_list_row("A", "x", "-", "acc"),
        device_list_row(
            "A very long device name that exceeds the cap",
            "音箱",
            "客厅",
            "账号ABC(12345)",
        ),
    ];
    let columns = compute_device_list_columns(&rows, usize::MAX, Language::Chinese);
    assert_eq!(columns.name, 20);
    assert_eq!(
        columns.category,
        UnicodeWidthStr::width(super::device_list_header_titles(Language::Chinese)[2]) + 2
    );
    assert_eq!(
        columns.account,
        UnicodeWidthStr::width("账号ABC(12345)") + 2
    );

    let short = format_device_list_item_with_columns(&rows[0], columns);
    let long = format_device_list_item_with_columns(&rows[1], columns);
    assert_eq!(
        UnicodeWidthStr::width(short.as_str()),
        columns.total_width(),
        "short: {short}"
    );
    assert_eq!(
        UnicodeWidthStr::width(long.as_str()),
        columns.total_width(),
        "long: {long}"
    );
}

#[test]
fn computed_device_list_columns_shrink_to_available_width() {
    let rows = vec![device_list_row(
        "A very long device name that exceeds the cap",
        "speaker-category",
        super::device_list_header_titles(Language::Chinese)[0],
        "account-123",
    )];
    let columns = compute_device_list_columns(&rows, 28, Language::Chinese);
    assert_eq!(columns.total_width(), 28);
}

#[test]
fn format_device_list_item_orders_columns_room_name_category_account() {
    let device = Device {
        did: "dev-1".to_string(),
        name: "living-room".to_string(),
        model: "xiaomi.wifispeaker.lx04".to_string(),
        online: true,
        pid: 0,

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
fn connect_type_label_maps_pid_to_official_enum() {
    use crate::tui::connect_type_label;
    // Known connect types from Xiaomi's official ha_xiaomi_home enum.
    assert_eq!(connect_type_label(0, Language::English), "WiFi");
    assert_eq!(connect_type_label(16, Language::English), "BLE-Mesh");
    assert_eq!(
        connect_type_label(14, Language::English),
        "Third-party cloud"
    );
    assert_eq!(connect_type_label(14, Language::Chinese), "第三方云接入");

    // Unsynced devices (pid < 0) render as "-".
    assert_eq!(connect_type_label(-1, Language::Chinese), "-");

    // Unknown codes (e.g. 21, absent from the official enum) keep the raw value.
    assert_eq!(connect_type_label(21, Language::English), "Other(21)");
    assert_eq!(connect_type_label(21, Language::Chinese), "其他(21)");
}

#[test]
fn device_table_renders_connect_type_column() {
    let device = Device {
        did: "dev-1".to_string(),
        name: "客厅灯".to_string(),
        model: "xiaomi.switch.2wpro2".to_string(),
        online: true,
        pid: 16,
        home_id: "cache-account:1001".to_string(),
        home_name: "账号A(1001)".to_string(),
        room_id: "room-1".to_string(),
        room_name: "客厅".to_string(),
    };
    let mut app = devices_tab_test_app(vec![device]);
    // English renders ASCII headers (CJK cells get space-separated in the test
    // backend buffer, which defeats a substring match).
    app.language = Language::English;
    let mut terminal = Terminal::new(TestBackend::new(120, 12)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("Connect"), "connect header missing: {text}");
    assert!(text.contains("BLE-Mesh"), "connect value missing: {text}");
}

#[test]
fn device_list_shows_connect_and_channel_columns() {
    let header = format_device_list_header(Language::English);
    // Connect column header is index [3]; Channel is index [5].
    assert!(
        header.contains(super::device_list_header_titles(Language::English)[3]),
        "{header}"
    );
    assert!(
        header.contains(super::device_list_header_titles(Language::English)[5]),
        "{header}"
    );

    let mut row = device_list_row("name", "cat", "客厅", "acc");
    row.connect = "BLE-Mesh".to_string();
    row.channel = "LAN".to_string();
    let columns =
        compute_device_list_columns(std::slice::from_ref(&row), usize::MAX, Language::English);
    let line = format_device_list_item_with_columns(&row, columns);
    assert!(line.contains("BLE-Mesh"), "{line}");
    assert!(line.contains("LAN"), "{line}");
    assert_eq!(
        UnicodeWidthStr::width(line.as_str()),
        columns.total_width(),
        "{line}"
    );
}
