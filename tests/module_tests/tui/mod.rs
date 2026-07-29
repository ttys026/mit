mod mock_mico_server {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/mock_mico_server.rs"
    ));
}

use super::{
    collect_readable_props, compute_device_list_columns, device_list_row,
    device_models_missing_local_specs, display_truncate_pad, draw, exit_result_for_boot_state,
    extract_actions_from_spec, format_device_list_header, format_device_list_item,
    format_device_list_item_with_columns, format_preview_props_set_command,
    format_preview_push_command, format_prop_dialog_action_list_item_line,
    format_prop_value_for_dialog, handle_key, handle_mouse, load_cached_devices_from_home,
    parse_bool_prop_value, raw_device_logs_item, raw_device_statistics_item,
    read_device_categories_from_template, settings_action_for_row, single_line_textarea,
    strip_ansi, tab_index_for_column_with_titles, AccountActionDialog, ActionItem, AuthFlowMessage,
    AuthState, BootState, BootstrapMessage, BootstrapPending, ListState,
    LocalTransportRefreshMessage, PropDialog, PropDialogTab, PropItem, ToggleItem, TuiApp,
};
use crate::mico_api::Device;
use crate::property_cache::PropertyCache;
use crate::storage::{default_auth, normalize_account, Language};
use crate::tui::pages::account as account_page;
use crossterm::event::{KeyCode, KeyModifiers};
use mock_mico_server::MockMicoServer;
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Modifier};
use ratatui::Terminal;
use serde_json::{json, Value};
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use time::{Date, Duration as TimeDuration, Month};
use unicode_width::UnicodeWidthStr;

#[allow(clippy::too_many_arguments)]
fn persisted_auth_account_json(
    uid: &str,
    nickname: &str,
    union_id: &str,
    uuid: &str,
    device_id: &str,
    state: &str,
    access_token: &str,
    refresh_token: &str,
    expires_ts: i64,
) -> Value {
    json!({
        "xiaomi": {
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": uuid,
            "deviceId": device_id,
            "state": state,
            "accessToken": access_token,
            "refreshToken": refresh_token,
            "expiresTs": expires_ts
        },
        "mijia": null,
        "user": {"uid": uid, "nickname": nickname, "icon": "", "unionId": union_id},
        "version": 1
    })
}

fn terminal_text(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    let mut text = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            text.push_str(buffer[(x, y)].symbol().as_ref());
        }
        text.push('\n');
    }
    text
}

fn test_device(did: &str, name: &str, room: &str, account_label: &str) -> Device {
    Device {
        did: did.to_string(),
        name: name.to_string(),
        model: "xiaomi.wifispeaker.lx04".to_string(),
        online: true,
        pid: 0,
        home_id: "cache-account:1001".to_string(),
        home_name: account_label.to_string(),
        room_id: format!("room-{room}"),
        room_name: room.to_string(),
    }
}

fn devices_tab_test_app(devices: Vec<Device>) -> TuiApp {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![test_account()],
        account_index: 0,
        devices,
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
    }
}

fn app_with_single_readonly_prop_dialog() -> TuiApp {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![test_account_with_mijia()],
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
            statistics_selected_bar: None,
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
    }
}

fn terminal_has_reversed_cell(terminal: &Terminal<TestBackend>) -> bool {
    let buffer = terminal.backend().buffer();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            if buffer[(x, y)].modifier.contains(Modifier::REVERSED) {
                return true;
            }
        }
    }
    false
}

fn terminal_first_reversed_cell_row(terminal: &Terminal<TestBackend>) -> Option<u16> {
    let buffer = terminal.backend().buffer();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            if buffer[(x, y)].modifier.contains(Modifier::REVERSED) {
                return Some(y);
            }
        }
    }
    None
}

fn terminal_has_green_substring(terminal: &Terminal<TestBackend>, needle: &str) -> bool {
    let buffer = terminal.backend().buffer();
    let symbols = needle.chars().map(|ch| ch.to_string()).collect::<Vec<_>>();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            if x as usize + symbols.len() > buffer.area.width as usize {
                break;
            }
            let mut matches = true;
            for (offset, symbol) in symbols.iter().enumerate() {
                let cell = &buffer[(x + offset as u16, y)];
                if cell.symbol() != symbol || cell.fg != Color::Green {
                    matches = false;
                    break;
                }
            }
            if matches {
                return true;
            }
        }
    }
    false
}

fn terminal_has_yellow_background_substring(
    terminal: &Terminal<TestBackend>,
    needle: &str,
) -> bool {
    let buffer = terminal.backend().buffer();
    let symbols = needle.chars().map(|ch| ch.to_string()).collect::<Vec<_>>();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            if x as usize + symbols.len() > buffer.area.width as usize {
                break;
            }
            let mut matches = true;
            for (offset, symbol) in symbols.iter().enumerate() {
                let cell = &buffer[(x + offset as u16, y)];
                if cell.symbol() != symbol || cell.bg != Color::Yellow {
                    matches = false;
                    break;
                }
            }
            if matches {
                return true;
            }
        }
    }
    false
}

fn terminal_has_dim_substring(terminal: &Terminal<TestBackend>, needle: &str) -> bool {
    let buffer = terminal.backend().buffer();
    let symbols = needle.chars().map(|ch| ch.to_string()).collect::<Vec<_>>();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            if x as usize + symbols.len() > buffer.area.width as usize {
                break;
            }
            let matches = symbols.iter().enumerate().all(|(offset, symbol)| {
                let cell = &buffer[(x + offset as u16, y)];
                cell.symbol() == symbol && cell.modifier.contains(Modifier::DIM)
            });
            if matches {
                return true;
            }
        }
    }
    false
}

fn assert_clock_timestamp_prefix(prefix: &str, text: &str) {
    let bytes = prefix.as_bytes();
    assert_eq!(bytes.len(), "[00:00:00] ".len(), "{text}");
    assert_eq!(bytes[0], b'[', "{text}");
    assert_eq!(bytes[3], b':', "{text}");
    assert_eq!(bytes[6], b':', "{text}");
    assert_eq!(bytes[9], b']', "{text}");
    assert_eq!(bytes[10], b' ', "{text}");
    for index in [1usize, 2, 4, 5, 7, 8] {
        assert!(bytes[index].is_ascii_digit(), "{text}");
    }
}

fn assert_timestamped_log_lines(copied: &str, messages: &[&str]) {
    let lines = copied.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), messages.len(), "{copied}");
    for (line, message) in lines.iter().zip(messages) {
        let Some(prefix) = line.strip_suffix(message) else {
            panic!("{copied}");
        };
        assert_clock_timestamp_prefix(prefix, copied);
    }
}

fn log_message_start_column() -> u16 {
    super::display_width("[00:00:00] ")
}

fn log_first_row(terminal_area: ratatui::layout::Rect) -> u16 {
    let [_tabs_area, content_area, _status_gap_area, _status_bar_area] =
        super::split_main_layout(terminal_area);
    let [_search_area, _search_border_area, list_area] =
        super::searchable_main_layout(content_area);
    list_area.y
}

fn top_log_line(terminal: &Terminal<TestBackend>, terminal_area: ratatui::layout::Rect) -> String {
    terminal_text(terminal)
        .lines()
        .nth(log_first_row(terminal_area) as usize)
        .unwrap_or_default()
        .to_string()
}

fn log_scrollbar_thumb_row(
    terminal: &Terminal<TestBackend>,
    terminal_area: ratatui::layout::Rect,
) -> Option<u16> {
    log_scrollbar_drag_points(terminal, terminal_area).map(|(_, thumb_row, _)| thumb_row)
}

fn log_scrollbar_thumb_height(
    terminal: &Terminal<TestBackend>,
    terminal_area: ratatui::layout::Rect,
) -> usize {
    let [_tabs_area, content_area, _status_gap_area, _status_bar_area] =
        super::split_main_layout(terminal_area);
    let [_search_area, _search_border_area, list_area] =
        super::searchable_main_layout(content_area);
    let x = list_area
        .x
        .saturating_add(list_area.width.saturating_sub(1));
    let buffer = terminal.backend().buffer();
    (list_area.y..list_area.y.saturating_add(list_area.height))
        .filter(|row| buffer[(x, *row)].symbol() == super::LOG_SCROLLBAR_THUMB)
        .count()
}

fn log_scrollbar_drag_points(
    terminal: &Terminal<TestBackend>,
    terminal_area: ratatui::layout::Rect,
) -> Option<(u16, u16, u16)> {
    let [_tabs_area, content_area, _status_gap_area, _status_bar_area] =
        super::split_main_layout(terminal_area);
    let [_search_area, _search_border_area, list_area] =
        super::searchable_main_layout(content_area);
    let x = list_area
        .x
        .saturating_add(list_area.width.saturating_sub(1));
    let buffer = terminal.backend().buffer();
    let thumb_row = (list_area.y..list_area.y.saturating_add(list_area.height))
        .find(|row| buffer[(x, *row)].symbol() == super::LOG_SCROLLBAR_THUMB)?;
    Some((
        x,
        thumb_row,
        list_area
            .y
            .saturating_add(list_area.height.saturating_sub(1)),
    ))
}

fn terminal_find_substring_position(
    terminal: &Terminal<TestBackend>,
    needle: &str,
) -> Option<(u16, u16)> {
    let buffer = terminal.backend().buffer();
    let symbols = needle.chars().map(|ch| ch.to_string()).collect::<Vec<_>>();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            if x as usize + symbols.len() > buffer.area.width as usize {
                break;
            }
            let matches = symbols
                .iter()
                .enumerate()
                .all(|(offset, symbol)| buffer[(x + offset as u16, y)].symbol() == symbol);
            if matches {
                return Some((x, y));
            }
        }
    }
    None
}

fn terminal_find_substring_position_in_area(
    terminal: &Terminal<TestBackend>,
    needle: &str,
    area: ratatui::layout::Rect,
) -> Option<(u16, u16)> {
    let buffer = terminal.backend().buffer();
    let symbols = needle.chars().map(|ch| ch.to_string()).collect::<Vec<_>>();
    let right = area.x.saturating_add(area.width);
    let bottom = area.y.saturating_add(area.height);
    for y in area.y..bottom {
        for x in area.x..right {
            if x as usize + symbols.len() > right as usize {
                break;
            }
            let matches = symbols
                .iter()
                .enumerate()
                .all(|(offset, symbol)| buffer[(x + offset as u16, y)].symbol() == symbol);
            if matches {
                return Some((x, y));
            }
        }
    }
    None
}

fn terminal_has_reversed_substring(terminal: &Terminal<TestBackend>, needle: &str) -> bool {
    let buffer = terminal.backend().buffer();
    let symbols = needle.chars().map(|ch| ch.to_string()).collect::<Vec<_>>();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            if x as usize + symbols.len() > buffer.area.width as usize {
                break;
            }
            let matches = symbols.iter().enumerate().all(|(offset, symbol)| {
                let cell = &buffer[(x + offset as u16, y)];
                cell.symbol() == symbol && cell.modifier.contains(Modifier::REVERSED)
            });
            if matches {
                return true;
            }
        }
    }
    false
}

fn footer_click_point(
    app: &TuiApp,
    terminal_area: ratatui::layout::Rect,
    label: &str,
) -> (u16, u16) {
    let areas = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Length(3),
            ratatui::layout::Constraint::Min(10),
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Length(3),
        ])
        .split(terminal_area);
    let footer = super::footer_render_area(areas[3]);
    let text = super::footer_text(app);
    let prefix = text
        .split_once(label)
        .map(|(left, _)| left)
        .expect("footer label present");
    let column = footer.x.saturating_add(super::display_width(prefix));
    (column, footer.y)
}

fn test_account() -> crate::storage::AuthAccount {
    normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "uuid-a",
        "deviceId": "device-a",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 32503680000_u64,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }))
}

fn test_account_with_mijia() -> crate::storage::AuthAccount {
    test_account_with_mijia_for("1001", "账号A", "cn")
}

fn test_account_with_mijia_for(
    uid: &str,
    nickname: &str,
    region: &str,
) -> crate::storage::AuthAccount {
    normalize_account(json!({
        "xiaomi": {
            "region": region,
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": format!("uuid-{uid}"),
            "deviceId": format!("device-{uid}"),
            "state": format!("state-{uid}"),
            "accessToken": format!("token-{uid}"),
            "refreshToken": format!("refresh-{uid}"),
            "expiresTs": 32503680000_u64
        },
        "mijia": {
            "ua": "test-ua",
            "deviceId": format!("device-{uid}"),
            "serviceToken": "test-service-token",
            "userId": uid,
            "cUserId": uid,
            "ssecurity": "test-ssecurity",
            "passToken": "test-pass-token",
            "passO": "",
            "expireTime": 32503680000_u64,
            "saveTime": 0
        },
        "user": {"uid": uid, "nickname": nickname, "icon": "", "unionId": format!("union-{uid}")},
        "version": 2
    }))
}

fn test_account_with(uid: &str, nickname: &str, region: &str) -> crate::storage::AuthAccount {
    normalize_account(json!({
        "region": region,
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": format!("uuid-{uid}"),
        "deviceId": format!("device-{uid}"),
        "state": format!("state-{uid}"),
        "accessToken": format!("token-{uid}"),
        "refreshToken": format!("refresh-{uid}"),
        "expiresTs": 32503680000_u64,
        "user": {
            "uid": uid,
            "nickname": nickname,
            "icon": "",
            "unionId": format!("union-{uid}")
        }
    }))
}

fn accounts_tab_test_app(accounts: Vec<crate::storage::AuthAccount>) -> TuiApp {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts,
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
    }
}

fn logs_tab_test_app(logs: Vec<&str>) -> TuiApp {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![test_account()],
        account_index: 0,
        devices: Vec::new(),
        device_index: 0,
        logs: logs.into_iter().map(ToString::to_string).collect(),
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
    }
}

fn test_app_with_prop_dialog(dialog: PropDialog) -> TuiApp {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![test_account()],
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
        prop_dialog: Some(dialog),
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
    }
}

fn prop_dialog_tabs_area(terminal_area: ratatui::layout::Rect) -> ratatui::layout::Rect {
    let popup = super::centered_rect(98, 95, terminal_area);
    let inner = ratatui::layout::Rect::new(
        popup.x.saturating_add(1),
        popup.y.saturating_add(1),
        popup.width.saturating_sub(2),
        popup.height.saturating_sub(2),
    );
    let sections = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Length(3),
            ratatui::layout::Constraint::Min(1),
        ])
        .split(inner);
    sections[0]
}

fn tab_column_for_index<S: AsRef<str>>(
    tabs_area: ratatui::layout::Rect,
    titles: &[S],
    expected_index: usize,
) -> u16 {
    let inner_left = tabs_area.x.saturating_add(1);
    let inner_right_exclusive = tabs_area
        .x
        .saturating_add(tabs_area.width.saturating_sub(1));
    for column in inner_left..inner_right_exclusive {
        if tab_index_for_column_with_titles(column, tabs_area, titles) == Some(expected_index) {
            return column;
        }
    }
    inner_left
}

fn make_temp_dir(prefix: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::current_dir()
        .unwrap()
        .join("target")
        .join("test-artifacts")
        .join(format!("{prefix}-{unique}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn env_guard() -> std::sync::MutexGuard<'static, ()> {
    crate::test_support::env_guard()
}

// ---- Per-feature test files (share this module's imports + helpers) ----

// ---- Per-feature test files (share this module's imports + helpers) ----

// ---- Per-feature test files (share this module's imports + helpers) ----
include!("account_login.rs");
include!("account.rs");
include!("device_list.rs");
include!("device_sync.rs");
include!("cloud_mips.rs");
include!("prop_collect.rs");
include!("prop_editing.rs");
include!("prop_actions.rs");
include!("prop_render.rs");
include!("operation_record.rs");
include!("statistics.rs");
include!("logs.rs");
include!("search.rs");
include!("footer.rs");
include!("render.rs");
include!("events.rs");
