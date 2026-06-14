// Auto-split from the former monolithic tui.rs. Shares the `tests` module
// scope (imports + helpers) of mod.rs via include!; do not add `use` here.

#[test]
fn collect_readable_props_filters_by_format_and_access() {
    let spec = json!({
        "services": [
            {
                "iid": 2,
                "properties": [
                    {
                        "iid": 1,
                        "description": "Power",
                        "format": "bool",
                        "access": ["read", "write"]
                    },
                    {
                        "iid": 2,
                        "description": "Volume",
                        "format": "uint8",
                        "access": ["read", "write"]
                    },
                    {
                        "iid": 4,
                        "description": "Mode",
                        "format": "string",
                        "access": ["read"]
                    },
                    {
                        "iid": 3,
                        "description": "ReadOnly",
                        "format": "bool",
                        "access": ["read"]
                    }
                ]
            }
        ]
    });
    let props = collect_readable_props(&spec, Language::Chinese);
    assert_eq!(props.len(), 4);
    assert_eq!(props[0].siid, 2);
    assert_eq!(props[0].piid, 1);
    assert_eq!(props[0].name, "Power");
    assert_eq!(props[0].format, "bool");
    assert!(props[0].writable);
    assert_eq!(props[1].siid, 2);
    assert_eq!(props[1].piid, 2);
    assert_eq!(props[1].name, "Volume");
    assert_eq!(props[1].format, "uint8");
    assert!(props[1].writable);
    assert_eq!(props[2].siid, 2);
    assert_eq!(props[2].piid, 4);
    assert_eq!(props[2].name, "Mode");
    assert_eq!(props[2].format, "string");
    assert!(!props[2].writable);
    assert_eq!(props[3].siid, 2);
    assert_eq!(props[3].piid, 3);
    assert_eq!(props[3].name, "ReadOnly");
    assert_eq!(props[3].format, "bool");
    assert!(!props[3].writable);
}

#[test]
fn devices_tab_slash_focuses_search_and_filters_visible_rows() {
    let mut app = devices_tab_test_app(vec![
        test_device("dev-kitchen", "kitchen plug", "Kitchen", "A(1001)"),
        test_device("dev-bed", "bedroom lamp", "Bedroom", "A(1001)"),
        test_device("dev-hall", "hall camera", "Hall", "A(1001)"),
    ]);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
    )
    .unwrap();
    for ch in ['b', 'e', 'd'] {
        handle_key(
            &mut app,
            crossterm::event::KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
        )
        .unwrap();
    }

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("bedroom lamp"), "{text}");
    assert!(!text.contains("kitchen plug"), "{text}");
    assert!(!text.contains("hall camera"), "{text}");
    assert_eq!(app.devices.len(), 3);
}

#[test]
fn devices_search_escape_blurs_and_restores_number_shortcuts() {
    let mut app = devices_tab_test_app(vec![test_device(
        "dev-kitchen",
        "kitchen plug",
        "Kitchen",
        "A(1001)",
    )]);

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
    )
    .unwrap();
    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE),
    )
    .unwrap();
    assert_eq!(app.active_tab, 1);

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
    )
    .unwrap();
    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE),
    )
    .unwrap();
    assert_eq!(app.active_tab, 2);
}

#[test]
fn devices_search_tab_shortcut_blurs_and_switches_tabs() {
    let mut app = devices_tab_test_app(vec![test_device(
        "dev-kitchen",
        "kitchen plug",
        "Kitchen",
        "A(1001)",
    )]);

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
    )
    .unwrap();
    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
    )
    .unwrap();

    assert_eq!(app.active_tab, 2);
    assert!(!app.input_mode);

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
    )
    .unwrap();
    assert_eq!(app.active_tab, 0);
}

#[test]
fn devices_search_mouse_tab_switch_blurs_and_keeps_tabs_clickable() {
    let mut app = devices_tab_test_app(vec![test_device(
        "dev-kitchen",
        "kitchen plug",
        "Kitchen",
        "A(1001)",
    )]);
    let terminal_area = ratatui::layout::Rect::new(0, 0, 80, 24);
    let [tabs_area, _content_area, _status_gap_area, _status_bar_area] =
        super::split_main_layout(terminal_area);

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
    )
    .unwrap();
    assert!(app.input_mode);

    let logs_column = tab_column_for_index(tabs_area, &super::tab_titles(app.language), 2);
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: logs_column,
            row: tabs_area.y.saturating_add(1),
            modifiers: KeyModifiers::NONE,
        },
        terminal_area,
    )
    .unwrap();

    assert_eq!(app.active_tab, 2);
    assert!(!app.input_mode);

    let devices_column = tab_column_for_index(tabs_area, &super::tab_titles(app.language), 1);
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: devices_column,
            row: tabs_area.y.saturating_add(1),
            modifiers: KeyModifiers::NONE,
        },
        terminal_area,
    )
    .unwrap();
    assert_eq!(app.active_tab, 1);
}

#[test]
fn devices_search_refocus_preserves_previous_query() {
    let mut app = devices_tab_test_app(vec![
        test_device("dev-kitchen", "kitchen plug", "Kitchen", "A(1001)"),
        test_device("dev-bed", "bedroom lamp", "Bedroom", "A(1001)"),
    ]);
    let terminal_area = ratatui::layout::Rect::new(0, 0, 80, 24);
    let [_tabs_area, content_area, _status_gap_area, _status_bar_area] =
        super::split_main_layout(terminal_area);

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
    )
    .unwrap();
    for ch in ['b', 'e', 'd'] {
        handle_key(
            &mut app,
            crossterm::event::KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
        )
        .unwrap();
    }
    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
    )
    .unwrap();

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
    )
    .unwrap();
    assert!(app.input_mode);
    assert_eq!(app.input, "bed");
    assert_eq!(app.device_search_cursor, 3);

    for ch in ['k', 'i', 't'] {
        handle_key(
            &mut app,
            crossterm::event::KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
        )
        .unwrap();
    }
    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
    )
    .unwrap();

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: content_area.x.saturating_add(2),
            row: content_area.y,
            modifiers: KeyModifiers::NONE,
        },
        terminal_area,
    )
    .unwrap();
    assert!(app.input_mode);
    assert_eq!(app.input, "bedkit");
}

#[test]
fn devices_search_focused_click_moves_cursor_to_character() {
    let mut app = devices_tab_test_app(Vec::new());
    app.language = Language::English;
    let terminal_area = ratatui::layout::Rect::new(0, 0, 80, 24);
    let [_tabs_area, content_area, _status_gap_area, _status_bar_area] =
        super::split_main_layout(terminal_area);

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
    )
    .unwrap();
    for ch in ['a', 'b', 'c', 'd'] {
        handle_key(
            &mut app,
            crossterm::event::KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
        )
        .unwrap();
    }

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: content_area
                .x
                .saturating_add("/ Search: ".width() as u16)
                .saturating_add(2),
            row: content_area.y,
            modifiers: KeyModifiers::NONE,
        },
        terminal_area,
    )
    .unwrap();
    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('X'), KeyModifiers::NONE),
    )
    .unwrap();

    assert_eq!(app.input, "abXcd");
}

#[test]
fn clicking_device_search_field_focuses_search() {
    let mut app = devices_tab_test_app(vec![
        test_device("dev-kitchen", "kitchen plug", "Kitchen", "A(1001)"),
        test_device("dev-bed", "bedroom lamp", "Bedroom", "A(1001)"),
    ]);
    let terminal_area = ratatui::layout::Rect::new(0, 0, 80, 24);
    let [_tabs_area, content_area, _status_gap_area, _status_bar_area] =
        super::split_main_layout(terminal_area);

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: content_area.x.saturating_add(2),
            row: content_area.y,
            modifiers: KeyModifiers::NONE,
        },
        terminal_area,
    )
    .unwrap();
    for ch in ['b', 'e', 'd'] {
        handle_key(
            &mut app,
            crossterm::event::KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
        )
        .unwrap();
    }

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("bedroom lamp"), "{text}");
    assert!(!text.contains("kitchen plug"), "{text}");
}

#[test]
fn devices_search_with_no_matches_does_not_open_hidden_device() {
    let mut app = devices_tab_test_app(vec![test_device(
        "dev-kitchen",
        "kitchen plug",
        "Kitchen",
        "A(1001)",
    )]);

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
    )
    .unwrap();
    for ch in ['z', 'z', 'z'] {
        handle_key(
            &mut app,
            crossterm::event::KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
        )
        .unwrap();
    }
    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
    )
    .unwrap();
    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();

    assert!(app.prop_dialog.is_none());
}

#[test]
fn devices_search_clicking_away_blurs_and_restores_shortcuts() {
    let mut app = devices_tab_test_app(vec![
        test_device("dev-kitchen", "kitchen plug", "Kitchen", "A(1001)"),
        test_device("dev-bed", "bedroom lamp", "Bedroom", "A(1001)"),
    ]);
    let terminal_area = ratatui::layout::Rect::new(0, 0, 80, 24);
    let [_tabs_area, content_area, _status_gap_area, _status_bar_area] =
        super::split_main_layout(terminal_area);

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
    )
    .unwrap();
    assert!(app.input_mode);

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: content_area.x.saturating_add(2),
            row: content_area.y.saturating_add(1),
            modifiers: KeyModifiers::NONE,
        },
        terminal_area,
    )
    .unwrap();
    assert!(!app.input_mode);

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE),
    )
    .unwrap();
    assert_eq!(app.active_tab, 2);
}

#[test]
fn devices_search_clicking_status_gap_blurs() {
    let mut app = devices_tab_test_app(vec![test_device(
        "dev-kitchen",
        "kitchen plug",
        "Kitchen",
        "A(1001)",
    )]);
    let terminal_area = ratatui::layout::Rect::new(0, 0, 80, 24);
    let [_tabs_area, _content_area, status_gap_area, _status_bar_area] =
        super::split_main_layout(terminal_area);

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
    )
    .unwrap();
    assert!(app.input_mode);

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: status_gap_area.x,
            row: status_gap_area.y,
            modifiers: KeyModifiers::NONE,
        },
        terminal_area,
    )
    .unwrap();
    assert!(!app.input_mode);
}

#[test]
fn devices_search_left_and_right_move_text_cursor() {
    let mut app = devices_tab_test_app(Vec::new());

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
    )
    .unwrap();
    for key in [
        KeyCode::Char('a'),
        KeyCode::Char('c'),
        KeyCode::Left,
        KeyCode::Char('b'),
        KeyCode::Right,
        KeyCode::Char('d'),
    ] {
        handle_key(
            &mut app,
            crossterm::event::KeyEvent::new(key, KeyModifiers::NONE),
        )
        .unwrap();
    }

    assert_eq!(app.input, "abcd");
}

#[test]
fn devices_search_up_and_down_navigate_matching_items() {
    let mut app = devices_tab_test_app(vec![
        test_device("dev-kitchen", "kitchen plug", "Kitchen", "A(1001)"),
        test_device("dev-bed", "bedroom lamp", "Bedroom", "A(1001)"),
        test_device("dev-desk", "desk lamp", "Office", "A(1001)"),
    ]);

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
    )
    .unwrap();
    for ch in ['l', 'a', 'm', 'p'] {
        handle_key(
            &mut app,
            crossterm::event::KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
        )
        .unwrap();
    }
    assert_eq!(app.devices[app.device_index].did, "dev-bed");

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
    )
    .unwrap();
    assert_eq!(app.devices[app.device_index].did, "dev-desk");

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
    )
    .unwrap();
    assert_eq!(app.devices[app.device_index].did, "dev-bed");
}

#[test]
fn devices_search_bar_renders_plain_with_bottom_border_under_it() {
    let mut app = devices_tab_test_app(vec![test_device(
        "dev-kitchen",
        "kitchen plug",
        "Kitchen",
        "A(1001)",
    )]);
    app.language = Language::English;
    let terminal_area = ratatui::layout::Rect::new(0, 0, 80, 24);
    let [_tabs_area, content_area, _status_gap_area, _status_bar_area] =
        super::split_main_layout(terminal_area);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    let text = terminal_text(&terminal);
    let lines = text.lines().collect::<Vec<_>>();
    let search_row = content_area.y as usize;
    let border_row = content_area.y.saturating_add(1) as usize;
    let header_row = content_area.y.saturating_add(2) as usize;
    assert!(lines[search_row].contains("/ Search:"), "{text}");
    assert!(!lines[search_row].starts_with("│"), "{text}");
    assert!(lines[border_row].contains("─"), "{text}");
    assert!(!lines[border_row].contains("Search"), "{text}");
    assert!(lines[header_row].contains("Room"), "{text}");
    assert!(
        !text
            .lines()
            .any(|line| line.starts_with("┌") && line.contains("─") && line.contains("Search")),
        "{text}"
    );
}

#[test]
fn devices_search_matches_room_name_device_name_and_category_only() {
    let home = make_temp_dir("tui-device-search-fields");
    let cached_device_dir = home.join(".mit").join("accounts").join("1001");
    fs::create_dir_all(&cached_device_dir).unwrap();
    fs::write(
        cached_device_dir.join("devices.json"),
        serde_json::to_string_pretty(&json!({
            "categories": {
                "xiaomi.wifispeaker.lx04": "Smart Category"
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

    for query in ["Kitchen", "kitchen plug", "Smart Category"] {
        let mut app = devices_tab_test_app(vec![test_device(
            "hidden-did",
            "kitchen plug",
            "Kitchen",
            "A(1001)",
        )]);
        app.home_dir = home.clone();
        app.language = Language::English;
        app.input = query.to_string();

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let text = terminal_text(&terminal);
        assert!(text.contains("kitchen plug"), "query={query} text={text}");
    }

    let mut app = devices_tab_test_app(vec![test_device(
        "hidden-did",
        "kitchen plug",
        "Kitchen",
        "A(1001)",
    )]);
    app.home_dir = home.clone();
    app.language = Language::English;
    app.input = "hidden-did".to_string();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(!text.contains("kitchen plug"), "{text}");

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn accounts_tab_slash_focuses_search_and_filters_visible_rows() {
    let mut app = accounts_tab_test_app(vec![
        test_account_with("1001", "Kitchen Account", "cn"),
        test_account_with("2002", "Bedroom Account", "sg"),
    ]);
    app.language = Language::English;
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
    )
    .unwrap();
    for ch in ['b', 'e', 'd'] {
        handle_key(
            &mut app,
            crossterm::event::KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
        )
        .unwrap();
    }

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("Bedroom Account"), "{text}");
    assert!(!text.contains("Kitchen Account"), "{text}");
    assert_eq!(app.accounts.len(), 2);
}

#[test]
fn accounts_search_matches_region_nickname_and_uid() {
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

    for query in ["sg", "Bedroom Account", "2002"] {
        let mut app = accounts_tab_test_app(vec![
            test_account_with("1001", "Kitchen Account", "cn"),
            test_account_with("2002", "Bedroom Account", "sg"),
        ]);
        app.language = Language::English;
        app.input = query.to_string();

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let text = terminal_text(&terminal);
        assert!(
            text.contains("Bedroom Account"),
            "query={query} text={text}"
        );
        assert!(
            !text.contains("Kitchen Account"),
            "query={query} text={text}"
        );
    }
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
fn search_query_ellipsizes_at_beginning_without_wrapping() {
    let mut app = accounts_tab_test_app(vec![test_account_with("1001", "Kitchen Account", "cn")]);
    app.language = Language::English;
    app.input_mode = true;
    app.input = "abcdefghijklmnopqrstuvwxyz0123456789".to_string();
    app.device_search_cursor = app.input.chars().count();
    let terminal_area = ratatui::layout::Rect::new(0, 0, 32, 24);
    let [_tabs_area, content_area, _status_gap_area, _status_bar_area] =
        super::split_main_layout(terminal_area);
    let mut terminal = Terminal::new(TestBackend::new(32, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    let text = terminal_text(&terminal);
    let lines = text.lines().collect::<Vec<_>>();
    let search_row = content_area.y as usize;
    let border_row = content_area.y.saturating_add(1) as usize;
    let header_row = content_area.y.saturating_add(2) as usize;
    assert!(
        lines[search_row].contains("/ Search: ...rstuvwxyz0123456789"),
        "{text}"
    );
    assert!(!lines[search_row].contains("abcdef"), "{text}");
    assert!(lines[border_row].contains("─"), "{text}");
    assert!(lines[header_row].contains("Region"), "{text}");
}

#[test]
fn search_textarea_text_is_selectable() {
    let mut app = accounts_tab_test_app(vec![test_account_with("1001", "Kitchen Account", "cn")]);
    app.language = Language::English;
    let terminal_area = ratatui::layout::Rect::new(0, 0, 80, 24);
    let [_tabs_area, content_area, _status_gap_area, _status_bar_area] =
        super::split_main_layout(terminal_area);

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
    )
    .unwrap();
    for ch in ['n', 'e', 'e', 'd', 'l', 'e'] {
        handle_key(
            &mut app,
            crossterm::event::KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
        )
        .unwrap();
    }

    let _guard = env_guard();
    let clip_file = make_temp_dir("tui-search-select-copy").join("clipboard.txt");
    std::env::set_var("MIT_TEST_CLIPBOARD_FILE", &clip_file);
    super::clear_selection_state();

    let start_column = content_area.x;
    let end_column = content_area
        .x
        .saturating_add("/ Search: needle".width() as u16);
    for (kind, column) in [
        (
            crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            start_column,
        ),
        (
            crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Left),
            end_column,
        ),
        (
            crossterm::event::MouseEventKind::Up(crossterm::event::MouseButton::Left),
            end_column,
        ),
    ] {
        handle_mouse(
            &mut app,
            crossterm::event::MouseEvent {
                kind,
                column,
                row: content_area.y,
                modifiers: KeyModifiers::NONE,
            },
            terminal_area,
        )
        .unwrap();
    }

    assert_eq!(fs::read_to_string(&clip_file).unwrap(), "/ Search: needle");
    assert_eq!(
        super::selected_surface()
            .expect("search selection should persist")
            .snapshot
            .surface,
        super::SelectionSurface::SearchInput
    );

    std::env::remove_var("MIT_TEST_CLIPBOARD_FILE");
    let _ = fs::remove_file(&clip_file);
}

#[test]
fn search_tab_switch_blurs_and_click_away_restores_shortcuts() {
    let mut app = accounts_tab_test_app(vec![
        test_account_with("1001", "Kitchen Account", "cn"),
        test_account_with("2002", "Bedroom Account", "sg"),
    ]);
    app.language = Language::English;
    let terminal_area = ratatui::layout::Rect::new(0, 0, 80, 24);
    let [tabs_area, content_area, _status_gap_area, _status_bar_area] =
        super::split_main_layout(terminal_area);

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
    )
    .unwrap();
    assert!(app.input_mode);

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: content_area.x,
            row: content_area.y.saturating_add(1),
            modifiers: KeyModifiers::NONE,
        },
        terminal_area,
    )
    .unwrap();
    assert!(!app.input_mode);

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
    )
    .unwrap();
    for ch in ['b', 'e', 'd'] {
        handle_key(
            &mut app,
            crossterm::event::KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
        )
        .unwrap();
    }
    let logs_column = tab_column_for_index(tabs_area, &super::tab_titles(app.language), 2);
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: logs_column,
            row: tabs_area.y.saturating_add(1),
            modifiers: KeyModifiers::NONE,
        },
        terminal_area,
    )
    .unwrap();

    assert_eq!(app.active_tab, 2);
    assert!(!app.input_mode);
    assert_eq!(app.input, "");

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
    )
    .unwrap();
    assert_eq!(app.active_tab, 0);
    assert!(!app.input_mode);
    assert_eq!(app.input, "bed");

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("Bedroom Account"), "{text}");
    assert!(!text.contains("Kitchen Account"), "{text}");
}

#[test]
fn search_queries_are_persisted_per_tab_when_switching_tabs() {
    let mut app = accounts_tab_test_app(vec![
        test_account_with("1001", "CN Account", "cn"),
        test_account_with("2002", "SG Account", "sg"),
    ]);
    app.language = Language::English;
    app.devices = vec![
        test_device("dev-fan", "fans", "Living", "A(1001)"),
        test_device("dev-lamp", "lamp", "Bedroom", "A(1001)"),
    ];
    app.logs = VecDeque::from([
        "mqtt connected".to_string(),
        "bootstrap complete".to_string(),
    ]);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
    )
    .unwrap();
    for ch in ['C', 'N'] {
        handle_key(
            &mut app,
            crossterm::event::KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
        )
        .unwrap();
    }
    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
    )
    .unwrap();
    assert_eq!(app.active_tab, 1);
    assert_eq!(app.input, "");

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
    )
    .unwrap();
    for ch in ['f', 'a', 'n', 's'] {
        handle_key(
            &mut app,
            crossterm::event::KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
        )
        .unwrap();
    }
    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
    )
    .unwrap();
    assert_eq!(app.active_tab, 2);
    assert_eq!(app.input, "");

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
    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
    )
    .unwrap();
    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
    )
    .unwrap();
    assert_eq!(app.active_tab, 0);
    assert_eq!(app.input, "CN");
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("CN Account"), "{text}");
    assert!(!text.contains("SG Account"), "{text}");

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
    )
    .unwrap();
    assert_eq!(app.active_tab, 1);
    assert_eq!(app.input, "fans");
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("fans"), "{text}");
    assert!(!text.contains("lamp"), "{text}");

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
    )
    .unwrap();
    assert_eq!(app.active_tab, 2);
    assert_eq!(app.input, "mqtt");
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("mqtt connected"), "{text}");
    assert!(!text.contains("bootstrap complete"), "{text}");
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
