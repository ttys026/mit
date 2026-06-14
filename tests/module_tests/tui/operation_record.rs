// Auto-split from the former monolithic tui.rs. Shares the `tests` module
// scope (imports + helpers) of mod.rs via include!; do not add `use` here.

#[test]
fn process_prop_dialog_loading_clears_props_loading_while_records_remain_pending() {
    let mut app = app_with_single_readonly_prop_dialog();
    app.language = Language::English;
    let (tx, rx) = mpsc::channel::<super::PropDialogRefreshMessage>();
    tx.send(super::PropDialogRefreshMessage::Props(vec![(
        0,
        Value::Bool(false),
    )]))
    .unwrap();
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.items[0].prop.writable = true;
    dialog.items[0].prop.name = "Power".to_string();
    dialog.items[0].value = Value::Bool(true);
    dialog.active_tab = PropDialogTab::Writable;
    dialog.selected = 0;
    dialog.writable_selected = 0;
    dialog.items.push(raw_device_logs_item(json!({
        "status": "loading",
        "requests": [
            {
                "key": "2.1",
                "response": {
                    "code": 0,
                    "result": [{"time": 0, "value": "[true]", "uid": "1001"}]
                }
            }
        ]
    })));
    dialog.refreshing = true;
    dialog.refresh_rx = Some(rx);

    app.process_prop_dialog_loading();

    let dialog = app.prop_dialog.as_ref().unwrap();
    assert!(!dialog.refreshing);
    assert!(dialog.refresh_rx.is_some());
    assert_eq!(dialog.items[0].value, Value::Bool(false));
    assert!(super::operation_record_logs_are_loading(dialog));

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(!text.contains("Loading..."), "{text}");
    assert!(
        !terminal_has_dim_substring(&terminal, "Power"),
        "props row should not dim while records are still loading:\n{text}"
    );

    drop(tx);
}

#[test]
fn prop_dialog_action_tab_does_not_inherit_operation_record_loading_state() {
    let mut app = app_with_single_readonly_prop_dialog();
    app.language = Language::English;
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog
        .items
        .push(raw_device_logs_item(json!({"status": "loading"})));
    dialog.actions = vec![ActionItem {
        siid: 2,
        aiid: 1,
        name: "Reboot".to_string(),
        input_piids: Vec::new(),
        input_labels: Vec::new(),
        input_props: Vec::new(),
    }];
    dialog.active_tab = PropDialogTab::Actions;
    dialog.selected = 0;
    dialog.actions_selected = 0;
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    let text = terminal_text(&terminal);
    assert!(text.contains("Reboot"), "{text}");
    assert!(!text.contains("Loading..."), "{text}");
    assert!(
        !terminal_has_dim_substring(&terminal, "Reboot"),
        "action rows should not dim while only operation records are loading:\n{text}"
    );
}

#[test]
fn prop_dialog_operation_records_render_subtabs_and_table() {
    let mut app = app_with_single_readonly_prop_dialog();
    app.accounts = vec![test_account()];
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.account_uid = "1001".to_string();
    dialog.items[0].prop = PropItem {
        siid: 2,
        piid: 1,
        name: "开关 / 开关状态".to_string(),
        format: "bool".to_string(),
        writable: false,
        value_options: Vec::new(),
    };
    dialog.items.push(ToggleItem {
        prop: PropItem {
            siid: 2,
            piid: 2,
            name: "功耗 / 电功率".to_string(),
            format: "uint16".to_string(),
            writable: false,
            value_options: Vec::new(),
        },
        value: json!(17),
    });
    dialog.items.push(raw_device_logs_item(json!({
        "requests": [
            {
                "key": "2.1",
                "type": "prop",
                "response": {
                    "code": 0,
                    "message": "ok",
                    "result": [
                        {"time": 0, "value": "[true]", "uid": "1001"}
                    ]
                }
            },
            {
                "key": "2.2",
                "type": "prop",
                "response": {
                    "code": 0,
                    "message": "ok",
                    "result": [
                        {"time": 60, "value": "[17]", "uid": "1001"}
                    ]
                }
            }
        ]
    })));
    dialog.active_tab = PropDialogTab::Logs;
    dialog.selected = 0;
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let first_text = terminal_text(&terminal);
    let first_compact = first_text.replace(' ', "");
    assert!(first_compact.contains("开关/开关状态"), "{first_text}");
    assert!(first_compact.contains("S:选择记录"), "{first_text}");
    assert!(first_compact.contains("用户时间值"), "{first_text}");
    assert!(first_text.contains("1970-01-01"), "{first_text}");
    assert!(first_text.contains("[true]"), "{first_text}");
    assert!(first_compact.contains("账号A"), "{first_text}");
    assert!(!first_text.contains("\"requests\""), "{first_text}");

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('S'), KeyModifiers::NONE),
    )
    .unwrap();
    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
    )
    .unwrap();
    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let second_text = terminal_text(&terminal);
    assert!(second_text.contains("[17]"), "{second_text}");
    assert!(!second_text.contains("[true]"), "{second_text}");
}

#[test]
fn prop_dialog_operation_records_dropdown_selects_active_key() {
    let mut app = app_with_single_readonly_prop_dialog();
    app.accounts = vec![test_account()];
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.account_uid = "1001".to_string();
    dialog.items[0].prop = PropItem {
        siid: 2,
        piid: 1,
        name: "开关 / 开关状态".to_string(),
        format: "bool".to_string(),
        writable: false,
        value_options: Vec::new(),
    };
    dialog.items.push(ToggleItem {
        prop: PropItem {
            siid: 2,
            piid: 2,
            name: "功耗 / 电功率".to_string(),
            format: "uint16".to_string(),
            writable: false,
            value_options: Vec::new(),
        },
        value: json!(17),
    });
    dialog.items.push(raw_device_logs_item(json!({
        "requests": [
            {
                "key": "2.1",
                "response": {
                    "code": 0,
                    "result": [{"time": 0, "value": "[true]", "uid": "1001"}]
                }
            },
            {
                "key": "2.2",
                "response": {
                    "code": 0,
                    "result": [{"time": 60, "value": "[17]", "uid": "1001"}]
                }
            }
        ]
    })));
    dialog.active_tab = PropDialogTab::Logs;
    dialog.selected = 0;
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let closed_text = terminal_text(&terminal);
    let closed_compact = closed_text.replace(' ', "");
    assert!(closed_compact.contains("S:选择记录"), "{closed_text}");
    assert!(closed_text.contains("▾"), "{closed_text}");

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('S'), KeyModifiers::NONE),
    )
    .unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let open_text = terminal_text(&terminal);
    let open_compact = open_text.replace(' ', "");
    assert!(open_text.contains("┌"), "{open_text}");
    assert!(open_compact.contains("│功耗/电功率"), "{open_text}");

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
    )
    .unwrap();
    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let selected_text = terminal_text(&terminal);
    assert!(selected_text.contains("[17]"), "{selected_text}");
    assert!(!selected_text.contains("[true]"), "{selected_text}");
}

#[test]
fn prop_dialog_operation_records_arrow_keys_move_open_dropdown_highlight() {
    let mut app = app_with_single_readonly_prop_dialog();
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.items[0].prop = PropItem {
        siid: 2,
        piid: 1,
        name: "Power".to_string(),
        format: "bool".to_string(),
        writable: false,
        value_options: Vec::new(),
    };
    dialog.items.push(ToggleItem {
        prop: PropItem {
            siid: 2,
            piid: 2,
            name: "Energy".to_string(),
            format: "uint16".to_string(),
            writable: false,
            value_options: Vec::new(),
        },
        value: json!(17),
    });
    dialog.items.push(raw_device_logs_item(json!({
        "requests": [
            {"key": "2.1", "response": {"code": 0, "result": [{"time": 0, "value": "[true]", "uid": "1001"}]}},
            {"key": "2.2", "response": {"code": 0, "result": [{"time": 60, "value": "[17]", "uid": "1001"}]}}
        ]
    })));
    dialog.active_tab = PropDialogTab::Logs;
    dialog.selected = 0;
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('S'), KeyModifiers::NONE),
    )
    .unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(terminal_has_green_substring(&terminal, "Power"));

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(app
        .prop_dialog
        .as_ref()
        .is_some_and(|dialog| dialog.editing && dialog.edit_cursor == 1));
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(terminal_has_green_substring(&terminal, "Energy"));
}

#[test]
fn prop_dialog_operation_records_arrow_keys_move_rows_not_record_type() {
    let mut app = app_with_single_readonly_prop_dialog();
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.items[0].prop = PropItem {
        siid: 2,
        piid: 1,
        name: "Power".to_string(),
        format: "bool".to_string(),
        writable: false,
        value_options: Vec::new(),
    };
    dialog.items.push(ToggleItem {
        prop: PropItem {
            siid: 2,
            piid: 2,
            name: "Energy".to_string(),
            format: "uint16".to_string(),
            writable: false,
            value_options: Vec::new(),
        },
        value: json!(17),
    });
    dialog.items.push(raw_device_logs_item(json!({
        "requests": [
            {
                "key": "2.1",
                "response": {
                    "code": 0,
                    "result": [
                        {"time": 0, "value": "[true]", "uid": "1001"},
                        {"time": 60, "value": "[false]", "uid": "1001"}
                    ]
                }
            },
            {
                "key": "2.2",
                "response": {"code": 0, "result": [{"time": 120, "value": "[17]", "uid": "1001"}]}
            }
        ]
    })));
    dialog.active_tab = PropDialogTab::Logs;
    dialog.selected = 0;
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let first_text = terminal_text(&terminal);
    assert!(
        terminal_has_reversed_substring(&terminal, "[true]"),
        "{first_text}"
    );

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
    )
    .unwrap();
    assert_eq!(
        app.prop_dialog.as_ref().unwrap().selected,
        0,
        "row navigation must not switch the selected record type"
    );
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("[true]"), "{text}");
    assert!(text.contains("[false]"), "{text}");
    assert!(!text.contains("[17]"), "{text}");
    assert!(
        terminal_has_reversed_substring(&terminal, "[false]"),
        "{text}"
    );
}

#[test]
fn prop_dialog_operation_records_scroll_moves_rows_not_record_type() {
    let mut app = app_with_single_readonly_prop_dialog();
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.items[0].prop = PropItem {
        siid: 2,
        piid: 1,
        name: "Power".to_string(),
        format: "bool".to_string(),
        writable: false,
        value_options: Vec::new(),
    };
    dialog.items.push(ToggleItem {
        prop: PropItem {
            siid: 2,
            piid: 2,
            name: "Energy".to_string(),
            format: "uint16".to_string(),
            writable: false,
            value_options: Vec::new(),
        },
        value: json!(17),
    });
    dialog.items.push(raw_device_logs_item(json!({
        "requests": [
            {
                "key": "2.1",
                "response": {
                    "code": 0,
                    "result": [
                        {"time": 0, "value": "[true]", "uid": "1001"},
                        {"time": 60, "value": "[false]", "uid": "1001"}
                    ]
                }
            },
            {
                "key": "2.2",
                "response": {"code": 0, "result": [{"time": 120, "value": "[17]", "uid": "1001"}]}
            }
        ]
    })));
    dialog.active_tab = PropDialogTab::Logs;
    dialog.selected = 0;
    let terminal_area = ratatui::layout::Rect::new(0, 0, 120, 24);

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollDown,
            column: 2,
            row: 8,
            modifiers: KeyModifiers::NONE,
        },
        terminal_area,
    )
    .unwrap();

    let dialog = app.prop_dialog.as_ref().unwrap();
    assert_eq!(dialog.selected, 0);
    assert_eq!(super::operation_record_active_row(dialog), 1);

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollUp,
            column: 2,
            row: 8,
            modifiers: KeyModifiers::NONE,
        },
        terminal_area,
    )
    .unwrap();
    let dialog = app.prop_dialog.as_ref().unwrap();
    assert_eq!(dialog.selected, 0);
    assert_eq!(super::operation_record_active_row(dialog), 0);
}

#[test]
fn prop_dialog_operation_records_active_row_stays_in_view() {
    let mut app = app_with_single_readonly_prop_dialog();
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.items[0].prop = PropItem {
        siid: 2,
        piid: 1,
        name: "Power".to_string(),
        format: "bool".to_string(),
        writable: false,
        value_options: Vec::new(),
    };
    let records = (0..30)
        .map(|index| {
            json!({
                "time": index * 60,
                "value": format!("[row-{index:02}]"),
                "uid": "1001"
            })
        })
        .collect::<Vec<_>>();
    dialog.items.push(raw_device_logs_item(json!({
        "requests": [
            {
                "key": "2.1",
                "response": {
                    "code": 0,
                    "result": records
                }
            }
        ]
    })));
    dialog.active_tab = PropDialogTab::Logs;
    dialog.selected = 0;
    for _ in 0..20 {
        handle_key(
            &mut app,
            crossterm::event::KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        )
        .unwrap();
    }
    let mut terminal = Terminal::new(TestBackend::new(120, 16)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    let text = terminal_text(&terminal);
    assert!(text.contains("[row-20]"), "{text}");
    assert!(
        terminal_has_reversed_substring(&terminal, "[row-20]"),
        "{text}"
    );
}

#[test]
fn prop_dialog_operation_records_page_limit_defaults_to_fifty() {
    assert_eq!(super::OPERATION_RECORD_PAGE_LIMIT, 50);
}

#[test]
fn prop_dialog_operation_records_selector_row_shows_right_aligned_date_status() {
    let mut app = app_with_single_readonly_prop_dialog();
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.items.push(raw_device_logs_item(json!({
        "requests": [
            {"key": "2.1", "response": {"code": 0, "result": [{"time": 0, "value": "[true]", "uid": "1001"}]}}
        ]
    })));
    dialog.active_tab = PropDialogTab::Logs;
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let buffer = terminal.backend().buffer();
    let placeholder_x = (0..buffer.area.width)
        .rev()
        .find(|x| buffer[(*x, 4)].symbol() == "选")
        .expect("right-aligned date placeholder");
    assert!(
        placeholder_x > 90,
        "placeholder should be aligned on the right side, got x={placeholder_x}"
    );

    let dialog = app.prop_dialog.as_mut().unwrap();
    let logs = dialog.items.last_mut().unwrap();
    logs.value["date_filter"] = json!({
        "time_start": 0,
        "time_end": 86_399
    });
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let row = terminal_text(&terminal)
        .lines()
        .nth(4)
        .unwrap_or_default()
        .replace(' ', "");
    assert!(row.contains("1970-01-01"), "{row}");
    assert!(!row.contains("选择日期范围"), "{row}");
}

#[test]
fn prop_dialog_operation_records_date_picker_accepts_mouse_date_selection() {
    let mut app = app_with_single_readonly_prop_dialog();
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.items.push(raw_device_logs_item(json!({
        "requests": [
            {"key": "2.1", "response": {"code": 0, "result": [{"time": 0, "value": "[true]", "uid": "1001"}]}}
        ]
    })));
    dialog.active_tab = PropDialogTab::Logs;
    let terminal_area = ratatui::layout::Rect::new(0, 0, 120, 24);
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('D'), KeyModifiers::NONE),
    )
    .unwrap();
    let before = super::operation_record_date_picker_state(app.prop_dialog.as_ref().unwrap())
        .unwrap()
        .cursor;
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    let calendar_area =
        super::operation_record_date_picker_calendar_area(terminal_area).expect("calendar area");
    let target_day = if before.day() == 1 { 2 } else { 1 };
    let target_date = Date::from_calendar_date(before.year(), before.month(), target_day).unwrap();
    let needle = format!("{target_day:>2}");
    let (column, row) =
        terminal_find_substring_position_in_area(&terminal, needle.as_str(), calendar_area)
            .unwrap_or_else(|| panic!("{}", terminal_text(&terminal)));

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: column.saturating_add(1),
            row,
            modifiers: KeyModifiers::NONE,
        },
        terminal_area,
    )
    .unwrap();

    let after =
        super::operation_record_date_picker_state(app.prop_dialog.as_ref().unwrap()).unwrap();
    assert_eq!(after.cursor, target_date);
    assert_eq!(after.pending_start, Some(target_date));
    assert_ne!(after.cursor, before);
}

#[test]
fn prop_dialog_date_picker_popup_wraps_fixed_calendar_with_one_cell_padding() {
    let terminal_area = ratatui::layout::Rect::new(0, 0, 120, 24);
    let large_terminal_area = ratatui::layout::Rect::new(0, 0, 200, 60);
    let popup = super::operation_record_date_picker_popup_area(terminal_area);
    let large_popup = super::operation_record_date_picker_popup_area(large_terminal_area);
    let calendar =
        super::operation_record_date_picker_calendar_area(terminal_area).expect("calendar area");

    assert_eq!(popup.width, large_popup.width);
    assert_eq!(popup.height, large_popup.height);
    assert_eq!(popup.width, calendar.width.saturating_add(2));
    assert_eq!(popup.height, calendar.height.saturating_add(2));
    assert_eq!(calendar.x, popup.x.saturating_add(1));
    assert_eq!(calendar.y, popup.y.saturating_add(1));
}

#[test]
fn prop_dialog_operation_records_user_column_expands_to_nickname() {
    let mut app = app_with_single_readonly_prop_dialog();
    app.accounts = vec![test_account_with("1001", "VeryLongOperatorName", "cn")];
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.items[0].prop = PropItem {
        siid: 2,
        piid: 1,
        name: "Power".to_string(),
        format: "bool".to_string(),
        writable: false,
        value_options: Vec::new(),
    };
    dialog.items.push(raw_device_logs_item(json!({
        "requests": [
            {
                "key": "2.1",
                "response": {
                    "code": 0,
                    "result": [{"time": 0, "value": "[true]", "uid": "1001"}]
                }
            }
        ]
    })));
    dialog.active_tab = PropDialogTab::Logs;
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    let text = terminal_text(&terminal);
    assert!(text.contains("VeryLongOperatorName"), "{text}");
    assert!(text.replace(' ', "").contains("用户"), "{text}");
}

#[test]
fn prop_dialog_operation_records_load_more_row_can_be_active() {
    let mut app = app_with_single_readonly_prop_dialog();
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.items[0].prop = PropItem {
        siid: 2,
        piid: 1,
        name: "Power".to_string(),
        format: "bool".to_string(),
        writable: false,
        value_options: Vec::new(),
    };
    dialog.items.push(raw_device_logs_item(json!({
        "requests": [
            {
                "key": "2.1",
                "has_more": true,
                "response": {
                    "code": 0,
                    "result": [
                        {"time": 60, "value": "[true]", "uid": "1001"},
                        {"time": 0, "value": "[false]", "uid": "1001"}
                    ]
                }
            }
        ]
    })));
    dialog.active_tab = PropDialogTab::Logs;
    dialog.selected = 0;
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.replace(' ', "").contains("加载更多"), "{text}");
    assert!(
        terminal_has_reversed_substring(&terminal, "[true]"),
        "{text}"
    );

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
    )
    .unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let second_text = terminal_text(&terminal);
    assert!(
        terminal_has_reversed_substring(&terminal, "[false]"),
        "{second_text}"
    );

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
    )
    .unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let third_text = terminal_text(&terminal);
    let reversed_row = terminal_first_reversed_cell_row(&terminal).unwrap();
    let reversed_text = third_text
        .lines()
        .nth(reversed_row as usize)
        .unwrap_or_default()
        .replace(' ', "");
    assert!(reversed_text.contains("加载更多"), "{third_text}");
}

#[test]
fn prop_dialog_operation_records_no_more_row_can_be_scrolled_into_view() {
    let mut app = app_with_single_readonly_prop_dialog();
    app.language = Language::English;
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.items[0].prop = PropItem {
        siid: 2,
        piid: 1,
        name: "Power".to_string(),
        format: "bool".to_string(),
        writable: false,
        value_options: Vec::new(),
    };
    let records = (0..18)
        .map(|index| {
            json!({
                "time": index * 60,
                "value": format!("[row-{index:02}]"),
                "uid": "1001"
            })
        })
        .collect::<Vec<_>>();
    dialog.items.push(raw_device_logs_item(json!({
        "requests": [
            {
                "key": "2.1",
                "pagination": {"no_more": true},
                "response": {
                    "code": 0,
                    "result": records
                }
            }
        ]
    })));
    dialog.active_tab = PropDialogTab::Logs;
    dialog.selected = 0;
    for _ in 0..18 {
        handle_key(
            &mut app,
            crossterm::event::KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        )
        .unwrap();
    }
    let mut terminal = Terminal::new(TestBackend::new(120, 12)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    let text = terminal_text(&terminal);
    let dialog = app.prop_dialog.as_ref().unwrap();
    assert_eq!(super::operation_record_active_row(dialog), 18);
    assert!(text.contains("No More Records"), "{text}");
    assert!(
        terminal_has_reversed_substring(&terminal, "No More Records"),
        "{text}"
    );
}

#[test]
fn prop_dialog_operation_records_body_click_changes_active_row() {
    let mut app = app_with_single_readonly_prop_dialog();
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.items[0].prop = PropItem {
        siid: 2,
        piid: 1,
        name: "Power".to_string(),
        format: "bool".to_string(),
        writable: false,
        value_options: Vec::new(),
    };
    dialog.items.push(raw_device_logs_item(json!({
        "requests": [
            {
                "key": "2.1",
                "response": {
                    "code": 0,
                    "result": [
                        {"time": 1_900_000_060, "value": "[true]", "uid": "1001"},
                        {"time": 1_900_000_000, "value": "[false]", "uid": "1001"}
                    ]
                }
            }
        ]
    })));
    dialog.active_tab = PropDialogTab::Logs;
    dialog.selected = 0;
    let terminal_area = ratatui::layout::Rect::new(0, 0, 120, 24);
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let (column, row) =
        terminal_find_substring_position(&terminal, "[false]").expect("second row rendered");
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        terminal_area,
    )
    .unwrap();

    let dialog = app.prop_dialog.as_ref().unwrap();
    assert_eq!(super::operation_record_active_row(dialog), 1);
}

#[test]
fn prop_dialog_operation_records_load_more_row_triggers_by_click_without_dialog_refreshing() {
    let mut app = app_with_single_readonly_prop_dialog();
    app.language = Language::English;
    app.accounts = vec![test_account()];
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.items[0].prop = PropItem {
        siid: 2,
        piid: 1,
        name: "Power".to_string(),
        format: "bool".to_string(),
        writable: false,
        value_options: Vec::new(),
    };
    dialog.items.push(raw_device_logs_item(json!({
        "requests": [
            {
                "key": "2.1",
                "pagination": {"has_more": true, "loading_more": false},
                "response": {
                    "code": 0,
                    "result": [
                        {"time": 1_900_000_060, "value": "[true]", "uid": "1001"},
                        {"time": 1_900_000_000, "value": "[false]", "uid": "1001"}
                    ]
                }
            }
        ]
    })));
    dialog.active_tab = PropDialogTab::Logs;
    dialog.selected = 0;
    let terminal_area = ratatui::layout::Rect::new(0, 0, 120, 24);
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let (column, row) =
        terminal_find_substring_position(&terminal, "Load More").expect("load more row rendered");
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        terminal_area,
    )
    .unwrap();

    let dialog = app.prop_dialog.as_ref().unwrap();
    let requests = super::operation_record_requests(dialog);
    assert_eq!(super::operation_record_active_row(dialog), 2);
    assert!(
        super::operation_record_request_is_loading_more(requests[0]),
        "active_row={} request={}",
        super::operation_record_active_row(dialog),
        requests[0]
    );
    assert!(
        !dialog.refreshing,
        "operation-record pagination should not use the whole-dialog refresh flag"
    );
    assert!(dialog.refresh_rx.is_some());
}

#[test]
fn prop_dialog_operation_records_selector_click_opens_and_selects() {
    let mut app = app_with_single_readonly_prop_dialog();
    app.accounts = vec![test_account()];
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.account_uid = "1001".to_string();
    dialog.items[0].prop = PropItem {
        siid: 2,
        piid: 1,
        name: "开关 / 开关状态".to_string(),
        format: "bool".to_string(),
        writable: false,
        value_options: Vec::new(),
    };
    dialog.items.push(ToggleItem {
        prop: PropItem {
            siid: 2,
            piid: 2,
            name: "功耗 / 电功率".to_string(),
            format: "uint16".to_string(),
            writable: false,
            value_options: Vec::new(),
        },
        value: json!(17),
    });
    dialog.items.push(raw_device_logs_item(json!({
        "requests": [
            {
                "key": "2.1",
                "response": {
                    "code": 0,
                    "result": [{"time": 0, "value": "[true]", "uid": "1001"}]
                }
            },
            {
                "key": "2.2",
                "response": {
                    "code": 0,
                    "result": [{"time": 60, "value": "[17]", "uid": "1001"}]
                }
            }
        ]
    })));
    dialog.active_tab = PropDialogTab::Logs;
    dialog.selected = 0;
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 1,
            row: 4,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 120, 24),
    )
    .unwrap();
    assert!(
        app.prop_dialog
            .as_ref()
            .is_some_and(|dialog| dialog.editing),
        "clicking selector row should open operation record menu"
    );

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let open_text = terminal_text(&terminal);
    let open_compact = open_text.replace(' ', "");
    assert!(open_compact.contains("功耗/电功率"), "{open_text}");

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 2,
            row: 8,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 120, 24),
    )
    .unwrap();
    assert!(
        app.prop_dialog
            .as_ref()
            .is_some_and(|dialog| !dialog.editing),
        "clicking dropdown item should close operation record menu"
    );
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let selected_text = terminal_text(&terminal);
    assert!(selected_text.contains("[17]"), "{selected_text}");
    assert!(!selected_text.contains("[true]"), "{selected_text}");
}

#[test]
fn prop_dialog_operation_records_selector_row_has_no_left_margin_and_bottom_border() {
    let mut app = app_with_single_readonly_prop_dialog();
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.items[0].prop = PropItem {
        siid: 2,
        piid: 1,
        name: "开关 / 开关状态".to_string(),
        format: "bool".to_string(),
        writable: false,
        value_options: Vec::new(),
    };
    dialog.items.push(raw_device_logs_item(json!({
        "requests": [
            {
                "key": "2.1",
                "response": {
                    "code": 0,
                    "result": [{"time": 0, "value": "[true]", "uid": "1001"}]
                }
            }
        ]
    })));
    dialog.active_tab = PropDialogTab::Logs;
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(1, 4)].symbol(), "S");
    assert_eq!(buffer[(1, 5)].symbol(), "─");
    assert_eq!(buffer[(1, 5)].fg, Color::Reset);
}

#[test]
fn prop_dialog_operation_records_dropdown_does_not_expand_selector_row() {
    let mut app = app_with_single_readonly_prop_dialog();
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.items[0].prop = PropItem {
        siid: 2,
        piid: 1,
        name: "开关 / 开关状态".to_string(),
        format: "bool".to_string(),
        writable: false,
        value_options: Vec::new(),
    };
    dialog.items.push(ToggleItem {
        prop: PropItem {
            siid: 2,
            piid: 2,
            name: "功耗 / 电功率".to_string(),
            format: "uint16".to_string(),
            writable: false,
            value_options: Vec::new(),
        },
        value: json!(17),
    });
    dialog.items.push(raw_device_logs_item(json!({
        "requests": [
            {
                "key": "2.1",
                "response": {
                    "code": 0,
                    "result": [{"time": 0, "value": "[true]", "uid": "1001"}]
                }
            },
            {
                "key": "2.2",
                "response": {
                    "code": 0,
                    "result": [{"time": 60, "value": "[17]", "uid": "1001"}]
                }
            }
        ]
    })));
    dialog.active_tab = PropDialogTab::Logs;
    dialog.editing = true;

    assert_eq!(super::operation_record_selector_height(dialog), 2);
}

#[test]
fn prop_dialog_operation_records_loading_shows_loading_text() {
    let mut app = app_with_single_readonly_prop_dialog();
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.active_tab = PropDialogTab::Logs;
    dialog.loading = true;
    dialog
        .items
        .push(raw_device_logs_item(json!({"status": "loading"})));
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");
    assert!(compact.contains("加载中"), "{text}");
    assert!(!compact.contains("暂无操作记录"), "{text}");
}

#[test]
fn prop_dialog_operation_records_prop_refresh_does_not_show_log_loading() {
    let mut app = app_with_single_readonly_prop_dialog();
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.active_tab = PropDialogTab::Logs;
    dialog.refreshing = true;
    dialog.items.push(raw_device_logs_item(json!({
        "requests": [
            {
                "key": "2.1",
                "response": {"code": 0}
            }
        ]
    })));

    let lines = super::operation_records_table_lines(dialog, app.language, app.accounts.as_slice());
    let text = lines.join("\n");
    assert!(text.contains("暂无操作记录"), "{text}");
    assert!(!text.contains("加载中"), "{text}");
}

#[test]
fn prop_dialog_operation_records_empty_results_still_show_date_picker_bar() {
    let mut app = app_with_single_readonly_prop_dialog();
    app.language = Language::English;
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.active_tab = PropDialogTab::Logs;
    dialog.items.push(raw_device_logs_item(json!({
        "requests": []
    })));
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    let text = terminal_text(&terminal);
    assert!(text.contains("D: Select Date Range"), "{text}");
    assert!(text.contains("No operation records"), "{text}");
}

#[test]
fn prop_dialog_operation_records_render_nonzero_code_as_error() {
    let mut app = app_with_single_readonly_prop_dialog();
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.items[0].prop = PropItem {
        siid: 2,
        piid: 1,
        name: "开关 / 开关状态".to_string(),
        format: "bool".to_string(),
        writable: false,
        value_options: Vec::new(),
    };
    dialog.items.push(raw_device_logs_item(json!({
        "requests": [
            {
                "key": "2.1",
                "type": "prop",
                "response": {
                    "code": -8,
                    "message": "invalid params",
                    "result": null
                }
            }
        ]
    })));
    dialog.active_tab = PropDialogTab::Logs;
    dialog.selected = 0;
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");
    assert!(compact.contains("错误"), "{text}");
    assert!(text.contains("code=-8"), "{text}");
    assert!(text.contains("invalid params"), "{text}");
}
