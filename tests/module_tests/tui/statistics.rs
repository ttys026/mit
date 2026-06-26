// Auto-split: shares the `tests` module scope (imports + helpers) of
// mod.rs via include!; do not add `use` here.

#[test]
fn prop_dialog_statistics_tab_renders_controls_and_bar_chart() {
    let mut app = app_with_single_readonly_prop_dialog();
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.items[0].prop = PropItem {
        siid: 2,
        piid: 2,
        name: "功耗 / 总耗电".to_string(),
        format: "float".to_string(),
        writable: false,
        value_options: Vec::new(),
    };
    dialog.items.push(ToggleItem {
        prop: PropItem {
            siid: 4,
            piid: 1,
            name: "功耗 / 峰值".to_string(),
            format: "float".to_string(),
            writable: false,
            value_options: Vec::new(),
        },
        value: json!(0),
    });
    dialog.items.push(raw_device_statistics_item(json!({
        "requests": [
            {
                "key": "2.2",
                "data_type": "stat_day_v3",
                "response": {
                    "code": 0,
                    "result": [
                        {"time": 0, "value": 1.25},
                        {"time": 86400, "value": "2.5"}
                    ]
                }
            },
            {
                "key": "4.1",
                "data_type": "stat_day_v3",
                "response": {
                    "code": 0,
                    "result": [
                        {"time": 0, "value": 9.0}
                    ]
                }
            }
        ],
        "ui": {"period": "week"},
        "date_filter": {
            "time_start": 0,
            "time_end": 604799
        }
    })));
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();

    app.prop_dialog.as_mut().unwrap().active_tab = PropDialogTab::Statistics;
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let stats_text = terminal_text(&terminal);
    let compact = stats_text.replace(' ', "");
    assert!(compact.contains("S:统计项"), "{stats_text}");
    assert!(compact.contains("功耗/总耗电"), "{stats_text}");
    assert!(compact.contains("周▾"), "{stats_text}");
    // The range label renders the date_filter timestamps as *local* dates, so the
    // exact strings depend on the machine timezone (e.g. time_end 604799 rounds to
    // Jan 7 in UTC but Jan 8 in UTC+8). Compute the expectation the same way the
    // production code does so the assertion is timezone-independent.
    let expected_range = format!(
        "{} - {}",
        super::format_operation_record_date(0),
        super::format_operation_record_date(604_799),
    );
    assert!(stats_text.contains(&expected_range), "{stats_text}");
    assert!(compact.contains("值↑时间→"), "{stats_text}");
    // The x-axis labels render the bucket dates as *local* dates, so the exact day
    // depends on the machine timezone (e.g. time_end 604799 is Jan 7 in UTC but Jan 8
    // in UTC+8). Derive the first/last labels with the same helpers production uses so
    // the assertions stay timezone-independent.
    let first_label = super::format_statistics_date_label(
        super::timestamp_to_local_date(0).expect("epoch local date"),
        super::StatisticsPeriod::Week,
    );
    let last_label = super::format_statistics_date_label(
        super::timestamp_to_local_date(604_799).expect("range-end local date"),
        super::StatisticsPeriod::Week,
    );
    assert!(stats_text.contains(&first_label), "{stats_text}");
    let chart_area = ratatui::layout::Rect::new(0, 6, 120, 18);
    // Y axis upper bound is max value * 1.1 (2.5 * 1.1 = 2.75).
    assert!(stats_text.contains("2.75"), "{stats_text}");
    // Per-bar value labels are not drawn anymore; the value is in the tooltip.
    assert!(
        terminal_find_substring_position_in_area(&terminal, "1.25", chart_area).is_none(),
        "{stats_text}"
    );
    let first_bar_position =
        terminal_find_substring_position_in_area(&terminal, "█", chart_area).expect("bar rendered");
    let first_time_position =
        terminal_find_substring_position_in_area(&terminal, first_label.as_str(), chart_area)
            .expect("first chart time label rendered");
    let last_time_position =
        terminal_find_substring_position_in_area(&terminal, last_label.as_str(), chart_area)
            .expect("last chart time label rendered");
    // Bars and labels are left-aligned: the first bar sits in the left portion
    // of the chart, just right of the Y axis, not centered.
    assert!(first_bar_position.0 < 30, "{stats_text}");
    assert!(first_time_position.0 < last_time_position.0, "{stats_text}");
    assert_eq!(first_time_position.1, last_time_position.1, "{stats_text}");
    assert!(!stats_text.contains("\"requests\""), "{stats_text}");
}

#[test]
fn prop_dialog_statistics_chart_points_fill_zero_daily_range() {
    let start = Date::from_calendar_date(2026, Month::January, 1).unwrap();
    let end = start.saturating_add(TimeDuration::days(3));
    let third_day = start.saturating_add(TimeDuration::days(2));
    let mut app = app_with_single_readonly_prop_dialog();
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.items[0].prop = PropItem {
        siid: 2,
        piid: 2,
        name: "Energy".to_string(),
        format: "float".to_string(),
        writable: false,
        value_options: Vec::new(),
    };
    dialog.items.push(raw_device_statistics_item(json!({
        "requests": [
            {
                "key": "2.2",
                "response": {
                    "code": 0,
                    "result": [
                        {"time": super::date_start_timestamp(start), "value": 1.0},
                        {"time": super::date_start_timestamp(third_day), "value": 3.0}
                    ]
                }
            }
        ],
        "ui": {"period": "week"},
        "date_filter": {
            "time_start": super::date_start_timestamp(start),
            "time_end": super::date_end_timestamp(end)
        }
    })));
    dialog.active_tab = PropDialogTab::Statistics;

    let points = super::statistics_chart_points(dialog, Language::Chinese).unwrap();

    assert_eq!(
        points
            .iter()
            .map(|point| point.label.as_str())
            .collect::<Vec<_>>(),
        ["01-01", "01-02", "01-03", "01-04"]
    );
    assert_eq!(
        points
            .iter()
            .map(|point| point.text_value.as_str())
            .collect::<Vec<_>>(),
        ["1", "0", "3", "0"]
    );
}

#[test]
fn prop_dialog_statistics_tallest_bar_keeps_value_label_headroom() {
    let start = Date::from_calendar_date(2026, Month::January, 1).unwrap();
    let end = start.saturating_add(TimeDuration::days(1));
    let mut app = app_with_single_readonly_prop_dialog();
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.items[0].prop = PropItem {
        siid: 2,
        piid: 2,
        name: "Energy".to_string(),
        format: "float".to_string(),
        writable: false,
        value_options: Vec::new(),
    };
    dialog.items.push(raw_device_statistics_item(json!({
        "requests": [
            {
                "key": "2.2",
                "response": {
                    "code": 0,
                    "result": [
                        {"time": super::date_start_timestamp(start), "value": 10.0},
                        {"time": super::date_start_timestamp(end), "value": 1.0}
                    ]
                }
            }
        ],
        "ui": {"period": "week"},
        "date_filter": {
            "time_start": super::date_start_timestamp(start),
            "time_end": super::date_end_timestamp(end)
        }
    })));
    dialog.active_tab = PropDialogTab::Statistics;
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let stats_text = terminal_text(&terminal);
    let chart_area = ratatui::layout::Rect::new(0, 6, 100, 16);
    // The Y axis tops out at max * 1.1 (10 * 1.1 = 11), so the tallest bar never
    // reaches the very top of the plot: there is headroom above it.
    let top_tick_position =
        terminal_find_substring_position_in_area(&terminal, "11", chart_area).expect("y axis max");
    let buffer = terminal.backend().buffer();
    let mut top_bar_row = None;
    'rows: for y in chart_area.y..chart_area.y.saturating_add(chart_area.height) {
        for x in 0..buffer.area.width {
            if buffer[(x, y)].symbol() == "█" {
                top_bar_row = Some(y);
                break 'rows;
            }
        }
    }
    let top_bar_row = top_bar_row.expect("bar rendered");
    assert!(top_bar_row > top_tick_position.1, "{stats_text}");
}

#[test]
fn prop_dialog_statistics_zero_value_renders_baseline_marker() {
    let start = Date::from_calendar_date(2026, Month::January, 1).unwrap();
    let end = start.saturating_add(TimeDuration::days(1));
    let mut app = app_with_single_readonly_prop_dialog();
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.items[0].prop = PropItem {
        siid: 2,
        piid: 2,
        name: "Energy".to_string(),
        format: "float".to_string(),
        writable: false,
        value_options: Vec::new(),
    };
    dialog.items.push(raw_device_statistics_item(json!({
        "requests": [
            {
                "key": "2.2",
                "response": {
                    "code": 0,
                    "result": [
                        {"time": super::date_start_timestamp(start), "value": 4.0},
                        {"time": super::date_start_timestamp(end), "value": 0.0}
                    ]
                }
            }
        ],
        "ui": {"period": "week"},
        "date_filter": {
            "time_start": super::date_start_timestamp(start),
            "time_end": super::date_end_timestamp(end)
        }
    })));
    dialog.active_tab = PropDialogTab::Statistics;
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let stats_text = terminal_text(&terminal);
    let chart_area = ratatui::layout::Rect::new(0, 6, 100, 16);
    let zero_label_position =
        terminal_find_substring_position_in_area(&terminal, "01-02", chart_area).unwrap();
    let zero_marker_column = zero_label_position
        .0
        .saturating_add(super::display_width("01-02") / 2);
    let buffer = terminal.backend().buffer();

    // The x labels sit one row below the axis baseline, so the zero-bar marker
    // is two rows above its time label.
    assert_eq!(
        buffer[(zero_marker_column, zero_label_position.1.saturating_sub(2))].symbol(),
        "▁",
        "{stats_text}"
    );
}

#[test]
fn prop_dialog_statistics_chart_points_use_range_endpoint_labels_for_month_and_year() {
    let month_start = Date::from_calendar_date(2026, Month::January, 1).unwrap();
    let month_end = Date::from_calendar_date(2026, Month::January, 5).unwrap();
    let third_day = month_start.saturating_add(TimeDuration::days(2));
    let mut month_app = app_with_single_readonly_prop_dialog();
    let month_dialog = month_app.prop_dialog.as_mut().unwrap();
    month_dialog.items[0].prop = PropItem {
        siid: 2,
        piid: 2,
        name: "Energy".to_string(),
        format: "float".to_string(),
        writable: false,
        value_options: Vec::new(),
    };
    month_dialog.items.push(raw_device_statistics_item(json!({
        "requests": [
            {
                "key": "2.2",
                "response": {
                    "code": 0,
                    "result": [
                        {"time": super::date_start_timestamp(third_day), "value": 4.0}
                    ]
                }
            }
        ],
        "ui": {"period": "month"},
        "date_filter": {
            "time_start": super::date_start_timestamp(month_start),
            "time_end": super::date_end_timestamp(month_end)
        }
    })));

    let month_points = super::statistics_chart_points(month_dialog, Language::Chinese).unwrap();

    assert_eq!(month_points.first().unwrap().label, "01-01");
    assert_eq!(month_points.last().unwrap().label, "01-05");
    assert_eq!(
        month_points
            .iter()
            .map(|point| point.text_value.as_str())
            .collect::<Vec<_>>(),
        ["0", "0", "4", "0", "0"]
    );

    let year_start = Date::from_calendar_date(2025, Month::January, 15).unwrap();
    let year_end = Date::from_calendar_date(2025, Month::March, 10).unwrap();
    let february = Date::from_calendar_date(2025, Month::February, 1).unwrap();
    let mut year_app = app_with_single_readonly_prop_dialog();
    let year_dialog = year_app.prop_dialog.as_mut().unwrap();
    year_dialog.items[0].prop = PropItem {
        siid: 2,
        piid: 2,
        name: "Energy".to_string(),
        format: "float".to_string(),
        writable: false,
        value_options: Vec::new(),
    };
    year_dialog.items.push(raw_device_statistics_item(json!({
        "requests": [
            {
                "key": "2.2",
                "response": {
                    "code": 0,
                    "result": [
                        {"time": super::date_start_timestamp(february), "value": 8.0}
                    ]
                }
            }
        ],
        "ui": {"period": "year"},
        "date_filter": {
            "time_start": super::date_start_timestamp(year_start),
            "time_end": super::date_end_timestamp(year_end)
        }
    })));

    let year_points = super::statistics_chart_points(year_dialog, Language::Chinese).unwrap();

    assert_eq!(
        year_points
            .iter()
            .map(|point| point.label.as_str())
            .collect::<Vec<_>>(),
        ["2025-01", "2025-02", "2025-03"]
    );
    assert_eq!(
        year_points
            .iter()
            .map(|point| point.text_value.as_str())
            .collect::<Vec<_>>(),
        ["0", "8", "0"]
    );
}

#[test]
fn prop_dialog_statistics_month_labels_collapse_to_at_most_five() {
    let start = Date::from_calendar_date(2026, Month::January, 1).unwrap();
    let end = Date::from_calendar_date(2026, Month::January, 31).unwrap();
    let mut app = app_with_single_readonly_prop_dialog();
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.items[0].prop = PropItem {
        siid: 2,
        piid: 2,
        name: "Energy".to_string(),
        format: "float".to_string(),
        writable: false,
        value_options: Vec::new(),
    };
    dialog.items.push(raw_device_statistics_item(json!({
        "requests": [
            {
                "key": "2.2",
                "response": {
                    "code": 0,
                    "result": [
                        {"time": super::date_start_timestamp(start), "value": 1.0},
                        {"time": super::date_start_timestamp(end), "value": 1.0}
                    ]
                }
            }
        ],
        "ui": {"period": "month"},
        "date_filter": {
            "time_start": super::date_start_timestamp(start),
            "time_end": super::date_end_timestamp(end)
        }
    })));
    dialog.active_tab = PropDialogTab::Statistics;
    let mut terminal = Terminal::new(TestBackend::new(90, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let stats_text = terminal_text(&terminal);
    let chart_area = ratatui::layout::Rect::new(0, 6, 90, 16);
    // 31 daily labels cannot all fit, so they collapse to five evenly spaced
    // labels including both endpoints (req 3).
    let visible_labels = ["01-01", "01-09", "01-16", "01-24", "01-31"];
    let positions = visible_labels
        .iter()
        .map(|label| {
            terminal_find_substring_position_in_area(&terminal, label, chart_area)
                .unwrap_or_else(|| panic!("{label} missing\n{stats_text}"))
        })
        .collect::<Vec<_>>();

    // Intermediate days are dropped, not crammed in.
    for dropped in ["01-02", "01-04", "01-07", "01-13", "01-20"] {
        assert!(
            terminal_find_substring_position_in_area(&terminal, dropped, chart_area).is_none(),
            "{dropped} should be hidden\n{stats_text}"
        );
    }
    for pair in positions.windows(2) {
        assert_eq!(pair[0].1, pair[1].1, "{stats_text}");
        // Comfortable spacing between labels: label width plus a gap.
        assert!(
            pair[1].0 >= pair[0].0.saturating_add("01-01".len() as u16 + 1),
            "{stats_text}"
        );
    }
}

#[test]
fn statistics_default_query_uses_rolling_time_windows() {
    let today = super::today_local_date();

    for (period, days) in [
        (super::StatisticsPeriod::Week, 7),
        (super::StatisticsPeriod::Month, 30),
        (super::StatisticsPeriod::Year, 365),
    ] {
        let query = super::statistics_default_query(period);

        assert_eq!(
            query.time_start,
            super::date_start_timestamp(today.saturating_sub(TimeDuration::days(days))),
            "{period:?}"
        );
        assert_eq!(
            query.time_end,
            super::date_end_timestamp(today),
            "{period:?}"
        );
    }
    assert_eq!(super::StatisticsPeriod::Month.data_type(), "stat_day_v3");
}

#[test]
fn prop_dialog_statistics_tab_shows_single_key_description_in_top_bar() {
    let mut app = app_with_single_readonly_prop_dialog();
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.items[0].prop = PropItem {
        siid: 2,
        piid: 2,
        name: "功耗 / 总耗电".to_string(),
        format: "float".to_string(),
        writable: false,
        value_options: Vec::new(),
    };
    dialog.items.push(raw_device_statistics_item(json!({
        "requests": [
            {
                "key": "2.2",
                "data_type": "stat_day_v3",
                "response": {
                    "code": 0,
                    "result": [
                        {"time": 0, "value": 1.25}
                    ]
                }
            }
        ],
        "ui": {"period": "week"}
    })));
    dialog.active_tab = PropDialogTab::Statistics;
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let stats_text = terminal_text(&terminal);
    let compact = stats_text.replace(' ', "");
    assert!(compact.contains("功耗/总耗电"), "{stats_text}");
}

#[test]
fn prop_dialog_statistics_dropdown_selects_active_key_and_period() {
    let mut app = app_with_single_readonly_prop_dialog();
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.items[0].prop = PropItem {
        siid: 2,
        piid: 2,
        name: "Energy".to_string(),
        format: "float".to_string(),
        writable: false,
        value_options: Vec::new(),
    };
    dialog.items.push(ToggleItem {
        prop: PropItem {
            siid: 4,
            piid: 1,
            name: "Peak".to_string(),
            format: "float".to_string(),
            writable: false,
            value_options: Vec::new(),
        },
        value: json!(0),
    });
    dialog.items.push(raw_device_statistics_item(json!({
        "requests": [
            {"key": "2.2", "response": {"code": 0, "result": [{"time": 0, "value": 1.0}]}},
            {"key": "4.1", "response": {"code": 0, "result": [{"time": 0, "value": 9.0}]}}
        ]
    })));
    dialog.active_tab = PropDialogTab::Statistics;
    dialog.selected = 0;
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('S'), KeyModifiers::NONE),
    )
    .unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let open_text = terminal_text(&terminal);
    assert!(
        terminal_has_green_substring(&terminal, "Energy"),
        "{open_text}"
    );

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
    assert!(selected_text.contains("Peak"), "{selected_text}");
    assert!(selected_text.contains("9"), "{selected_text}");

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('P'), KeyModifiers::NONE),
    )
    .unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let period_text = terminal_text(&terminal);
    assert!(period_text.contains("周"), "{period_text}");
    assert!(period_text.contains("月"), "{period_text}");
    assert!(period_text.contains("年"), "{period_text}");

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
    let dialog = app.prop_dialog.as_ref().unwrap();
    assert_eq!(
        super::statistics_period_for_dialog(dialog),
        super::StatisticsPeriod::Month
    );
    let date_filter = super::raw_device_statistics_value(dialog)
        .and_then(|value| value.get("date_filter"))
        .expect("date filter set after changing statistics period");
    assert_eq!(
        super::json_i64(date_filter.get("time_start")),
        Some(super::date_start_timestamp(
            super::today_local_date().saturating_sub(TimeDuration::days(30))
        ))
    );
    assert_eq!(
        super::json_i64(date_filter.get("time_end")),
        Some(super::date_end_timestamp(super::today_local_date()))
    );
}

#[test]
fn prop_dialog_statistics_without_response_renders_unsupported_message() {
    let mut app = app_with_single_readonly_prop_dialog();
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.items[0].prop = PropItem {
        siid: 2,
        piid: 2,
        name: "功耗 / 总耗电".to_string(),
        format: "float".to_string(),
        writable: false,
        value_options: Vec::new(),
    };
    dialog.items.push(raw_device_statistics_item(json!({
        "requests": [
            {
                "key": "2.2",
                "data_type": "stat_day_v3"
            }
        ],
        "ui": {"period": "week"}
    })));
    dialog.active_tab = PropDialogTab::Statistics;
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let stats_text = terminal_text(&terminal);
    let compact = stats_text.replace(' ', "");
    assert!(
        compact.contains("此设备不支持查看统计数据或所选周期内暂无数据"),
        "{stats_text}"
    );
    assert!(!compact.contains("值↑时间→"), "{stats_text}");
    assert!(!stats_text.contains('█'), "{stats_text}");
}

#[test]
fn prop_dialog_statistics_date_picker_omits_inline_description() {
    let mut app = app_with_single_readonly_prop_dialog();
    app.language = Language::English;
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.items[0].prop = PropItem {
        siid: 2,
        piid: 2,
        name: "Energy".to_string(),
        format: "float".to_string(),
        writable: false,
        value_options: Vec::new(),
    };
    dialog.items.push(raw_device_statistics_item(json!({
        "requests": [
            {"key": "2.2", "response": {"code": 0, "result": [{"time": 0, "value": 1.0}]}}
        ],
        "ui": {"period": "week"}
    })));
    dialog.active_tab = PropDialogTab::Statistics;
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('D'), KeyModifiers::NONE),
    )
    .unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let picker_text = terminal_text(&terminal);

    assert!(picker_text.contains("Stats Range"), "{picker_text}");
    assert!(!picker_text.contains("Select Week"), "{picker_text}");
    assert!(!picker_text.contains("Cancel"), "{picker_text}");
}

#[test]
fn prop_dialog_statistics_date_picker_mouse_click_selects_date() {
    let mut app = app_with_single_readonly_prop_dialog();
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.items[0].prop = PropItem {
        siid: 2,
        piid: 2,
        name: "Energy".to_string(),
        format: "float".to_string(),
        writable: false,
        value_options: Vec::new(),
    };
    dialog.items.push(raw_device_statistics_item(json!({
        "requests": [
            {"key": "2.2", "response": {"code": 0, "result": [{"time": 0, "value": 1.0}]}}
        ],
        "ui": {"period": "week"}
    })));
    dialog.active_tab = PropDialogTab::Statistics;
    let terminal_area = ratatui::layout::Rect::new(0, 0, 120, 24);
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('D'), KeyModifiers::NONE),
    )
    .unwrap();
    let before = super::statistics_date_picker_state(app.prop_dialog.as_ref().unwrap())
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

    let dialog = app.prop_dialog.as_ref().unwrap();
    assert!(!dialog.editing);
    let date_filter = super::raw_device_statistics_value(dialog)
        .and_then(|value| value.get("date_filter"))
        .expect("date filter selected by mouse click");
    let (start, end) =
        super::statistics_period_date_range(super::StatisticsPeriod::Week, target_date);
    assert_eq!(
        super::json_i64(date_filter.get("time_start")),
        Some(super::date_start_timestamp(start))
    );
    assert_eq!(
        super::json_i64(date_filter.get("time_end")),
        Some(super::date_end_timestamp(end))
    );
}

#[test]
fn prop_dialog_statistics_chart_click_shows_crosshair_tooltip_and_click_away_clears() {
    let start = Date::from_calendar_date(2026, Month::January, 1).unwrap();
    let end = start.saturating_add(TimeDuration::days(2));
    let mut app = app_with_single_readonly_prop_dialog();
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.items[0].prop = PropItem {
        siid: 2,
        piid: 2,
        name: "Energy".to_string(),
        format: "float".to_string(),
        writable: false,
        value_options: Vec::new(),
    };
    dialog.items.push(raw_device_statistics_item(json!({
        "requests": [
            {
                "key": "2.2",
                "response": {
                    "code": 0,
                    "result": [
                        {"time": super::date_start_timestamp(start), "value": 5.0}
                    ]
                }
            }
        ],
        "ui": {"period": "week"},
        "date_filter": {
            "time_start": super::date_start_timestamp(start),
            "time_end": super::date_end_timestamp(end)
        }
    })));
    dialog.active_tab = PropDialogTab::Statistics;
    let terminal_area = ratatui::layout::Rect::new(0, 0, 120, 24);
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    let chart_area = ratatui::layout::Rect::new(0, 6, 120, 18);
    let bar_position =
        terminal_find_substring_position_in_area(&terminal, "█", chart_area).expect("bar rendered");

    // Clicking a bar selects it (crosshair + tooltip).
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: bar_position.0,
            row: bar_position.1,
            modifiers: KeyModifiers::NONE,
        },
        terminal_area,
    )
    .unwrap();
    assert_eq!(
        app.prop_dialog.as_ref().unwrap().statistics_selected_bar,
        Some(0)
    );

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let tooltip_text = terminal_text(&terminal);
    let compact = tooltip_text.replace(' ', "");
    assert!(compact.contains("日期:01-01"), "{tooltip_text}");
    assert!(compact.contains("Energy:5"), "{tooltip_text}");
    // The selected bar is highlighted by a crosshair background band.
    assert_eq!(
        terminal.backend().buffer()[(bar_position.0, bar_position.1)].bg,
        Color::DarkGray,
        "{tooltip_text}"
    );

    // Clicking an empty area of the chart clears the crosshair + tooltip.
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 110,
            row: bar_position.1,
            modifiers: KeyModifiers::NONE,
        },
        terminal_area,
    )
    .unwrap();
    assert_eq!(
        app.prop_dialog.as_ref().unwrap().statistics_selected_bar,
        None
    );

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let cleared_text = terminal_text(&terminal);
    assert!(
        !cleared_text.replace(' ', "").contains("日期:01-01"),
        "{cleared_text}"
    );
}

#[test]
fn prop_dialog_statistics_period_menu_closes_on_click_away() {
    let mut app = app_with_single_readonly_prop_dialog();
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.items[0].prop = PropItem {
        siid: 2,
        piid: 2,
        name: "Energy".to_string(),
        format: "float".to_string(),
        writable: false,
        value_options: Vec::new(),
    };
    dialog.items.push(raw_device_statistics_item(json!({
        "requests": [
            {"key": "2.2", "response": {"code": 0, "result": [{"time": 0, "value": 1.0}]}}
        ],
        "ui": {"period": "week"}
    })));
    dialog.active_tab = PropDialogTab::Statistics;
    let terminal_area = ratatui::layout::Rect::new(0, 0, 120, 24);
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('P'), KeyModifiers::NONE),
    )
    .unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(
        super::statistics_period_menu_is_open(app.prop_dialog.as_ref().unwrap()),
        "period menu should be open"
    );

    // Clicking outside the dropdown closes it (generic click-away).
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 1,
            row: 1,
            modifiers: KeyModifiers::NONE,
        },
        terminal_area,
    )
    .unwrap();
    assert!(
        !super::statistics_period_menu_is_open(app.prop_dialog.as_ref().unwrap()),
        "period menu should be closed after click-away"
    );
}

#[test]
fn mijia_statistics_key_uses_power_consumption_float_props_only() {
    let power = PropItem {
        siid: 4,
        piid: 1,
        name: "Power Consumption / Power Consumption".to_string(),
        format: "float".to_string(),
        writable: false,
        value_options: Vec::new(),
    };
    let switch_state = PropItem {
        siid: 2,
        piid: 1,
        name: "Switch / Switch Status".to_string(),
        format: "bool".to_string(),
        writable: true,
        value_options: Vec::new(),
    };
    let electric_power = PropItem {
        siid: 4,
        piid: 2,
        name: "Power Consumption / Electric Power".to_string(),
        format: "uint16".to_string(),
        writable: false,
        value_options: Vec::new(),
    };

    assert_eq!(super::mijia_statistics_key(&power), Some("4.1".to_string()));
    assert_eq!(super::mijia_statistics_key(&switch_state), None);
    assert_eq!(super::mijia_statistics_key(&electric_power), None);
}
