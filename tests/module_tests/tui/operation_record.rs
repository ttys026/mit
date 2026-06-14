// Auto-split: shares the `tests` module scope (imports + helpers) of
// mod.rs via include!; do not add `use` here.

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
    app.accounts = vec![test_account_with_mijia()];
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
    app.accounts = vec![test_account_with_mijia()];
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
    app.accounts = vec![test_account_with_mijia()];
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
    app.accounts = vec![test_account_with_mijia()];
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
fn prop_dialog_operation_records_s_shortcut_opens_selector_and_footer_mentions_it() {
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

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.replace(' ', "").contains("S:选择记录"), "{text}");

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('S'), KeyModifiers::NONE),
    )
    .unwrap();
    assert!(app
        .prop_dialog
        .as_ref()
        .is_some_and(|dialog| dialog.editing));
}

#[test]
fn prop_dialog_operation_records_footer_shows_date_shortcuts_and_picker_opens() {
    let mut app = app_with_single_readonly_prop_dialog();
    app.language = Language::English;
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.items.push(raw_device_logs_item(json!({
        "requests": [
            {"key": "2.1", "response": {"code": 0, "result": [{"time": 0, "value": "[true]", "uid": "1001"}]}}
        ]
    })));
    dialog.active_tab = PropDialogTab::Logs;
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("D: Date"), "{text}");
    assert!(text.contains("C: Clear"), "{text}");

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
    )
    .unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let picker_text = terminal_text(&terminal);
    assert!(picker_text.contains("Date Range"), "{picker_text}");
    assert!(!picker_text.contains("Select Start"), "{picker_text}");
    assert!(!picker_text.contains("Cancel"), "{picker_text}");
}

#[test]
fn logs_tab_slash_focuses_search_and_filters_visible_rows() {
    let mut app = logs_tab_test_app(vec!["alpha boot complete", "beta sync done"]);
    app.language = Language::English;
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
    )
    .unwrap();
    for ch in ['s', 'y', 'n', 'c'] {
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
}

#[test]
fn logs_tab_search_mode_c_keeps_typing_into_query() {
    let mut app = logs_tab_test_app(vec!["alpha boot complete", "beta sync done"]);

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
    )
    .unwrap();
    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
    )
    .unwrap();

    assert_eq!(app.search_query(), "c");
    assert_eq!(app.logs.len(), 2);
}

#[test]
fn logs_tab_search_highlights_matching_text() {
    let mut app = logs_tab_test_app(vec!["mqtt connected"]);
    app.language = Language::English;
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
    )
    .unwrap();
    for ch in ['m', 'q', 't', 't'] {
        handle_key(
            &mut app,
            crossterm::event::KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
        )
        .unwrap();
    }

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(terminal_has_yellow_background_substring(&terminal, "mqtt"));
}

#[test]
fn prop_dialog_operation_records_date_picker_sets_and_clears_filter() {
    let mut app = app_with_single_readonly_prop_dialog();
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.items.push(raw_device_logs_item(json!({
        "requests": [
            {"key": "2.1", "response": {"code": 0, "result": [{"time": 0, "value": "[true]", "uid": "1001"}]}}
        ]
    })));
    dialog.active_tab = PropDialogTab::Logs;

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('D'), KeyModifiers::NONE),
    )
    .unwrap();
    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();
    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
    )
    .unwrap();
    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();

    let logs = app
        .prop_dialog
        .as_ref()
        .unwrap()
        .items
        .last()
        .unwrap()
        .value
        .clone();
    assert!(logs.get("date_filter").is_some(), "{logs}");
    assert!(!app.prop_dialog.as_ref().unwrap().editing);

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
    )
    .unwrap();
    let logs = &app
        .prop_dialog
        .as_ref()
        .unwrap()
        .items
        .last()
        .unwrap()
        .value;
    assert!(logs.get("date_filter").is_none(), "{logs}");
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
fn mijia_raw_requests_hide_successful_empty_results() {
    let entries = super::visible_mijia_raw_request_entries(vec![
        json!({
            "key": "2.1",
            "response": {
                "code": 0,
                "message": "ok",
                "result": []
            }
        }),
        json!({
            "key": "2.2",
            "response": {
                "code": 0,
                "message": "ok",
                "result": [{"value": "[true]"}]
            }
        }),
        json!({
            "key": "2.3",
            "response": {
                "code": -8,
                "message": "invalid params",
                "result": null
            }
        }),
    ]);

    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["key"], "2.2");
    assert_eq!(entries[1]["key"], "2.3");
}
