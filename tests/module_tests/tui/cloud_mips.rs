// Auto-split: shares the `tests` module scope (imports + helpers) of
// mod.rs via include!; do not add `use` here.

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
            last_mqtt_response_at: Some(now.checked_sub(Duration::from_secs(301)).unwrap_or(now)),
            last_ping_req_at: Some(now.checked_sub(Duration::from_secs(301)).unwrap_or(now)),
            last_ping_resp_at: Some(now.checked_sub(Duration::from_secs(301)).unwrap_or(now)),
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
        statistics_selected_bar: None,
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
        statistics_selected_bar: None,
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
        statistics_selected_bar: None,
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
fn refresh_cloud_mips_listeners_skips_when_auto_subscribe_setting_off() {
    let _guard = env_guard();
    std::env::remove_var("MIT_DISABLE_CLOUD_MIPS");
    std::env::remove_var("MIT_ENABLE_CLOUD_MIPS_IN_TESTS");
    let mut app = devices_tab_test_app(vec![test_device(
        "dev-kitchen",
        "kitchen plug",
        "Kitchen",
        "A(1001)",
    )]);
    app.active_tab = 3;
    app.settings_selected = 1;

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();
    app.refresh_cloud_mips_listeners();

    let logs = app.logs.iter().cloned().collect::<Vec<_>>().join("\n");
    assert!(logs.contains("cloud MIPS not started: auto subscribe disabled"));

    let mut runtime = super::cloud_mips_runtime().lock().unwrap();
    *runtime = None;
}
