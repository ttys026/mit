// Auto-split from the former monolithic tui.rs. Shares the `tests` module
// scope (imports + helpers) of mod.rs via include!; do not add `use` here.

#[test]
fn format_device_list_item_shows_room_column() {
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
fn read_device_categories_from_template_returns_model_category_mapping() {
    let home = make_temp_dir("tui-device-category-map");
    let specs = home.join(".mit").join("cache").join("specs");
    fs::create_dir_all(&specs).unwrap();
    fs::create_dir_all(specs.join("sources")).unwrap();
    fs::write(
        specs.join("sources").join("template_list_device.json"),
        serde_json::to_string_pretty(&json!({
            "result": [
                {
                    "type": "urn:miot-spec-v2:device:speaker:0000A015",
                    "description": {"en": "Speaker", "zh_cn": "音箱"}
                },
                {
                    "model": "legacy.model",
                    "category_name": "legacy-category"
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        specs.join("index.json"),
        serde_json::to_string_pretty(&json!({
            "xiaomi.wifispeaker.lx04": {
                "urn": "urn:miot-spec-v2:device:speaker:0000A015:xiaomi-lx04:1"
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let categories =
        read_device_categories_from_template(home.as_path(), Language::Chinese).unwrap();

    assert_eq!(
        categories
            .get("xiaomi.wifispeaker.lx04")
            .map(String::as_str),
        Some("音箱")
    );
    assert_eq!(
        categories.get("legacy.model").map(String::as_str),
        Some("legacy-category")
    );

    let _ = fs::remove_dir_all(home);
}

#[test]
fn device_models_missing_local_specs_returns_unique_uncached_models() {
    let home = make_temp_dir("tui-missing-device-spec-models");
    let specs = home.join(".mit").join("cache").join("specs").join("models");
    fs::create_dir_all(&specs).unwrap();
    fs::create_dir_all(
        home.join(".mit")
            .join("cache")
            .join("specs")
            .join("sources"),
    )
    .unwrap();
    fs::write(specs.join("xiaomi.gateway.hub1.json"), "{}\n").unwrap();
    fs::write(
        home.join(".mit")
            .join("cache")
            .join("specs")
            .join("sources")
            .join("template_list_device.json"),
        "{ \"result\": [] }\n",
    )
    .unwrap();
    fs::write(
        home.join(".mit")
            .join("cache")
            .join("specs")
            .join("index.json"),
        "{}\n",
    )
    .unwrap();

    let devices = vec![
        Device {
            did: "dev-1".to_string(),
            name: "living-room".to_string(),
            model: "xiaomi.wifispeaker.lx04".to_string(),
            online: true,
            home_id: "home-1".to_string(),
            home_name: "我家".to_string(),
            room_id: "room-1".to_string(),
            room_name: "客厅".to_string(),
        },
        Device {
            did: "dev-2".to_string(),
            name: "living-room-2".to_string(),
            model: "xiaomi.wifispeaker.lx04".to_string(),
            online: true,
            home_id: "home-1".to_string(),
            home_name: "我家".to_string(),
            room_id: "room-2".to_string(),
            room_name: "卧室".to_string(),
        },
        Device {
            did: "dev-3".to_string(),
            name: "gateway".to_string(),
            model: "xiaomi.gateway.hub1".to_string(),
            online: true,
            home_id: "home-1".to_string(),
            home_name: "我家".to_string(),
            room_id: "room-3".to_string(),
            room_name: "书房".to_string(),
        },
        Device {
            did: "dev-4".to_string(),
            name: "blank-model".to_string(),
            model: "   ".to_string(),
            online: true,
            home_id: "home-1".to_string(),
            home_name: "我家".to_string(),
            room_id: "room-4".to_string(),
            room_name: "阳台".to_string(),
        },
    ];

    let missing = device_models_missing_local_specs(home.as_path(), &devices);

    assert_eq!(missing, vec!["xiaomi.wifispeaker.lx04".to_string()]);

    let _ = fs::remove_dir_all(home);
}

#[test]
fn device_models_missing_local_specs_treats_missing_category_metadata_as_missing_specs() {
    let home = make_temp_dir("tui-missing-device-spec-metadata");

    let devices = vec![
        Device {
            did: "dev-1".to_string(),
            name: "living-room".to_string(),
            model: "xiaomi.wifispeaker.lx04".to_string(),
            online: true,
            home_id: "home-1".to_string(),
            home_name: "我家".to_string(),
            room_id: "room-1".to_string(),
            room_name: "客厅".to_string(),
        },
        Device {
            did: "dev-2".to_string(),
            name: "gateway".to_string(),
            model: "xiaomi.gateway.hub1".to_string(),
            online: true,
            home_id: "home-1".to_string(),
            home_name: "我家".to_string(),
            room_id: "room-2".to_string(),
            room_name: "书房".to_string(),
        },
    ];

    let missing = device_models_missing_local_specs(home.as_path(), &devices);

    assert_eq!(
        missing,
        vec![
            "xiaomi.wifispeaker.lx04".to_string(),
            "xiaomi.gateway.hub1".to_string()
        ]
    );

    let _ = fs::remove_dir_all(home);
}

#[test]
fn draw_devices_selected_row_uses_reversed_style() {
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
        device_index: 1,
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
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    assert!(terminal_has_reversed_cell(&terminal));
}

#[test]
fn draw_devices_scrolls_to_keep_active_row_visible() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let devices = (0..12)
        .map(|idx| Device {
            did: format!("dev-{idx}"),
            name: format!("dev{idx}"),
            model: "xiaomi.wifispeaker.lx04".to_string(),
            online: true,

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
        device_index: 10,
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
    let mut terminal = Terminal::new(TestBackend::new(60, 8)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    let text = terminal_text(&terminal);
    assert!(text.contains("dev10"), "{text}");
    assert!(!text.contains("dev0"), "{text}");
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
fn load_cached_devices_from_home_ignores_local_credentials_without_devices_cache() {
    let home = make_temp_dir("tui-cached-device-fallback");
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
            "dev-1001": {
                "did": "dev-1001",
                "name": "living-room",
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
            "dev-1002": {
                "did": "dev-1002",
                "name": "bedroom",
                "model": "xiaomi.wifispeaker.lx04",
                "localIp": "192.168.1.12",
                "token": "ffeeddccbbaa99887766554433221100",
                "source": "direct"
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let devices = load_cached_devices_from_home(&home, "1001").unwrap();
    assert!(devices.is_empty());

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn draw_devices_tab_uses_local_cache_when_device_list_is_empty() {
    let home = make_temp_dir("tui-devices-tab-cache-fallback");
    let mit_dir = home.join(".mit");
    fs::create_dir_all(&mit_dir).unwrap();
    fs::write(
        mit_dir.join("auth.json"),
        serde_json::to_string_pretty(&json!({
            "accounts": [
                persisted_auth_account_json("1001", "账号A", "union-a", "uuid-a", "device-a", "state-a", "token-a", "refresh-a", 1)
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    fs::create_dir_all(mit_dir.join("accounts").join("1001")).unwrap();
    fs::write(
        mit_dir.join("accounts").join("1001").join("devices.json"),
        serde_json::to_string_pretty(&json!({
            "devices": [
                {
                    "did": "dev-cache-1",
                    "name": "cached-speaker",
                    "model": "xiaomi.wifispeaker.lx04",
                    "online": false,
                    "homeId": "cache-account:1001",
                    "homeName": "账号A(1001)",
                    "roomId": "",
                    "roomName": ""
                }
            ],
            "categories": {
                "xiaomi.wifispeaker.lx04": "音箱"
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-cache-tab-account",
        "deviceId": "mico.tui-cache-tab-account",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 1,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));
    let mut app = TuiApp {
        home_dir: home.clone(),
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
    assert!(text.contains("cached-speaker"), "text: {text}");
    assert_eq!(
        app.devices.len(),
        1,
        "logs={:?} bootstrap_pending={:?}",
        app.logs,
        app.bootstrap_pending
    );
    assert_eq!(app.devices[0].did, "dev-cache-1");
    assert_eq!(app.devices[0].home_name, "账号A(1001)");

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn load_cached_devices_from_home_does_not_fallback_to_config_json() {
    let home = make_temp_dir("tui-local-credentials-no-config-fallback");
    let mit_dir = home.join(".mit");
    fs::create_dir_all(&mit_dir).unwrap();
    fs::write(
        mit_dir.join("auth.json"),
        serde_json::to_string_pretty(&json!({
            "accounts": [
                persisted_auth_account_json("1001", "账号A", "union-a", "uuid-a", "device-a", "state-a", "token-a", "refresh-a", 1)
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    let devices = load_cached_devices_from_home(&home, "").unwrap();
    assert!(devices.is_empty());

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
fn devices_tab_r_starts_background_sync() {
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
        crossterm::event::KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
    )
    .unwrap();
    assert!(!quit);
    assert!(app.bootstrap_pending.is_some());
}

#[test]
fn devices_tab_s_no_longer_starts_background_sync() {
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
        crossterm::event::KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE),
    )
    .unwrap();
    assert!(!quit);
    assert!(app.bootstrap_pending.is_none());
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
        boot_state: BootState::Ready,
        boot_spinner_index: 0,
        bootstrap_generation: 0,
        bootstrap_pending: None,
        bootstrap_tx,
        bootstrap_rx,
        property_cache: Arc::new(PropertyCache::new()),
        language: Language::Chinese,
        auto_subscribe_device_status: true,
        settings_selected: 2,
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
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");
    assert!(compact.contains("操作:重置设备缓存"), "{text}");
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
fn start_bootstrap_uses_cached_devices_immediately_while_syncing_in_background() {
    let home = make_temp_dir("tui-bootstrap-immediate-cache");
    let mit_dir = home.join(".mit");
    fs::create_dir_all(&mit_dir).unwrap();
    fs::write(
        mit_dir.join("auth.json"),
        serde_json::to_string_pretty(&json!({
            "accounts": [
                persisted_auth_account_json("1001", "账号A", "union-a", "uuid-a", "device-a", "state-a", "", "", 1)
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    fs::create_dir_all(mit_dir.join("accounts").join("1001")).unwrap();
    fs::write(
        mit_dir.join("accounts").join("1001").join("devices.json"),
        serde_json::to_string_pretty(&json!({
            "devices": [
                {
                    "did": "dev-cache-1",
                    "name": "cached-speaker",
                    "model": "xiaomi.wifispeaker.lx04",
                    "online": false,
                    "homeId": "cache-account:1001",
                    "homeName": "账号A(1001)",
                    "roomId": "",
                    "roomName": ""
                }
            ],
            "categories": {
                "xiaomi.wifispeaker.lx04": "音箱"
            }
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

    app.start_bootstrap();

    assert!(matches!(app.boot_state, BootState::Ready));
    assert_eq!(app.devices.len(), 1);
    assert_eq!(app.devices[0].did, "dev-cache-1");
    assert!(app
        .logs
        .iter()
        .any(|line| line.contains("cached devices while syncing in background")));
    assert!(app.bootstrap_pending.is_some());

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn process_bootstrap_message_preserves_selected_device_did_when_present() {
    let home = make_temp_dir("tui-sync-preserve-selection");
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();

    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-bootstrap-preserve",
        "deviceId": "mico.tui-bootstrap-preserve",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 1,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));
    let mut auth_state = default_auth();
    auth_state.accounts = vec![account.clone()];
    let devices = vec![
        Device {
            did: "dev-1".to_string(),
            name: "living-room".to_string(),
            model: "xiaomi.wifispeaker.lx04".to_string(),
            online: true,

            home_id: "home-1".to_string(),
            home_name: "我家".to_string(),
            room_id: "room-1".to_string(),
            room_name: "a-room".to_string(),
        },
        Device {
            did: "dev-2".to_string(),
            name: "bedroom".to_string(),
            model: "xiaomi.gateway.hub1".to_string(),
            online: true,

            home_id: "home-1".to_string(),
            home_name: "我家".to_string(),
            room_id: "room-2".to_string(),
            room_name: "b-room".to_string(),
        },
    ];

    let mut app = TuiApp {
        home_dir: home.clone(),
        auth_state: default_auth(),
        accounts: vec![account.clone()],
        account_index: 0,
        devices: devices.clone(),
        device_index: 1,
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
        .send(BootstrapMessage::Ready {
            generation: 2,
            uid: "1001".to_string(),
            offline_uids: Vec::new(),
            auth_state,
            accounts: vec![account],
            devices,
            logs: vec!["loaded 2 devices".to_string()],
        })
        .unwrap();

    app.process_background_messages();

    assert_eq!(app.devices[app.device_index].did, "dev-2");

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn process_background_messages_clears_local_transport_refresh_device_id_on_error() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-refresh-retry",
        "deviceId": "mico.tui-refresh-retry",
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
        local_transport_fetching: true,
        local_transport_refresh_generation: 7,
        local_transport_refresh_device_id: Some(account.device_id.clone()),
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
            error: Some("transient network error".to_string()),
        })
        .unwrap();

    app.process_background_messages();

    assert!(!app.local_transport_fetching);
    assert_eq!(app.local_transport_refresh_device_id, None);
    assert!(app
        .logs
        .iter()
        .any(|line| line.contains("transient network error")));
}

#[test]
fn sync_failure_uses_cached_devices_without_quitting() {
    let home = make_temp_dir("tui-sync-failure-cache-recovery");
    let mit_dir = home.join(".mit");
    fs::create_dir_all(&mit_dir).unwrap();
    fs::write(
        mit_dir.join("auth.json"),
        serde_json::to_string_pretty(&json!({
            "accounts": [
                persisted_auth_account_json("1001", "账号A", "union-a", "uuid-a", "device-a", "state-a", "token-a", "refresh-a", 1)
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    fs::create_dir_all(mit_dir.join("accounts").join("1001")).unwrap();
    fs::write(
        mit_dir.join("accounts").join("1001").join("devices.json"),
        serde_json::to_string_pretty(&json!({
            "devices": [
                {
                    "did": "dev-cache-1",
                    "name": "cached-speaker",
                    "model": "xiaomi.wifispeaker.lx04",
                    "online": false,
                    "homeId": "cache-account:1001",
                    "homeName": "账号A(1001)",
                    "roomId": "",
                    "roomName": ""
                }
            ],
            "categories": {
                "xiaomi.wifispeaker.lx04": "音箱"
            }
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
    assert!(matches!(app.boot_state, BootState::Ready));
    assert!(app.bootstrap_pending.is_some());

    let deadline = Instant::now() + Duration::from_secs(2);
    while app.devices.is_empty() && Instant::now() < deadline {
        app.process_background_messages();
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(app.devices.len(), 1);
    assert_eq!(app.devices[0].did, "dev-cache-1");

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn sync_downloads_missing_specs_and_enriches_cached_devices_file() {
    let _guard = env_guard();
    let server = MockMicoServer::start();
    let home = make_temp_dir("tui-sync-enriches-cached-devices");
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
    let mut auth_state = default_auth();
    auth_state.accounts = vec![account.clone()];
    std::env::set_var("MIT_HOME", &home);
    std::env::set_var("MIT_PROFILE_DIR", &home);
    std::env::set_var("MIT_MICO_BASE_URL", server.base_url());
    std::env::set_var("MIT_USER_PROFILE_URL", server.user_profile_url());
    std::env::set_var(
        "MIT_MIOT_SPEC_URL_BASE",
        format!("{}/miot-spec-v2/instance", server.base_url()),
    );

    let mut app = TuiApp {
        home_dir: home.clone(),
        auth_state,
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

    let devices_path = home
        .join(".mit")
        .join("accounts")
        .join("1001")
        .join("devices.json");
    let deadline = Instant::now() + Duration::from_secs(8);
    let cached_payload = loop {
        app.process_background_messages();
        if devices_path.exists() {
            if let Some(payload) = fs::read_to_string(&devices_path)
                .ok()
                .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            {
                if payload
                    .get("categories")
                    .and_then(|categories| categories.get("xiaomi.wifispeaker.lx04"))
                    .and_then(Value::as_str)
                    == Some("音箱")
                {
                    break payload;
                }
            }
        }
        assert!(
            Instant::now() < deadline,
            "logs={:?} requests={:?}",
            app.logs,
            server.requests()
        );
        thread::sleep(Duration::from_millis(10));
    };

    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        app.process_background_messages();
        if app.bootstrap_pending.is_none()
            && app
                .devices
                .iter()
                .any(|device| device.model == "xiaomi.wifispeaker.lx04")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "logs={:?} requests={:?} pending={:?} devices={:?}",
            app.logs,
            server.requests(),
            app.bootstrap_pending,
            app.devices
        );
        thread::sleep(Duration::from_millis(10));
    }

    assert_eq!(
        cached_payload
            .get("devices")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(4)
    );
    assert_eq!(
        cached_payload
            .get("categories")
            .and_then(|categories| categories.get("xiaomi.wifispeaker.lx04"))
            .and_then(Value::as_str),
        Some("音箱")
    );
    assert!(server.requests().iter().any(|request| {
        request.method == "GET" && request.path == "/miot-spec-v2/template/list/device"
    }));

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("音箱") || text.contains("音 箱"), "{text}");

    std::env::remove_var("MIT_HOME");
    std::env::remove_var("MIT_PROFILE_DIR");
    std::env::remove_var("MIT_MICO_BASE_URL");
    std::env::remove_var("MIT_USER_PROFILE_URL");
    std::env::remove_var("MIT_MIOT_SPEC_URL_BASE");
    let _ = fs::remove_dir_all(&home);
}
