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
    read_device_categories_from_template, single_line_textarea, tab_index_for_column_with_titles,
    AccountActionDialog, ActionItem, AuthFlowMessage, AuthState, BootState, BootstrapMessage,
    BootstrapPending, ListState, LocalTransportRefreshMessage, PropDialog, PropDialogTab, PropItem,
    ToggleItem, TuiApp,
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
fn preview_push_command_masks_long_params() {
    assert_eq!(
        format_preview_push_command("1001", "hellooo"),
        "mit push --uid 1001 \"...\""
    );
}

#[test]
fn preview_set_command_masks_long_params() {
    assert_eq!(
        format_preview_props_set_command("device-123", 2, 1, &json!(1234567)),
        "mit props set device-123 2 1 \"...\""
    );
}

#[test]
fn extract_auth_url_from_line_accepts_json_and_plain_url() {
    let json_line = r#"{"type":"authUrlPrinted","url":"https://example.com/oauth"}"#;
    assert_eq!(
        super::extract_auth_url_from_line(json_line).as_deref(),
        Some("https://example.com/oauth")
    );
    assert_eq!(
        super::extract_auth_url_from_line("https://example.com/direct").as_deref(),
        Some("https://example.com/direct")
    );
}

#[test]
fn forward_auth_login_output_emits_first_url_from_buffer() {
    let mut cursor = std::io::Cursor::new(
            b"{\"type\":\"authUrlPrinted\",\"url\":\"https://example.com/oauth\"}\n{\"type\":\"authWaiting\"}\n".to_vec(),
        );
    let (tx, rx) = mpsc::channel();
    account_page::forward_auth_login_output_until_eof(&mut cursor, &tx);
    let auth_url = rx
        .recv_timeout(Duration::from_millis(300))
        .unwrap()
        .unwrap();
    assert_eq!(auth_url, "https://example.com/oauth");
}

#[test]
fn account_list_row_shows_xiaomi_and_mijia_login_statuses() {
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-account-status",
        "deviceId": "mico.tui-account-status",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 32503680000_u64,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"},
        "mijia": {
            "ua": "Android-15-test",
            "deviceId": "mijia-device-a",
            "passO": "pass-o-a",
            "ssecurity": "AQIDBAUGBwgJCgsMDQ4PEA==",
            "passToken": "pass-token-a",
            "userId": "1001",
            "cUserId": "c-1001",
            "serviceToken": "service-token-a",
            "expireTime": 222,
            "saveTime": 123
        }
    }));

    let row = account_page::account_list_row(&account, false, Language::Chinese);

    assert_eq!(row.xiaomi_status, "已登录");
    assert_eq!(row.mijia_status, "已登录");
    let columns = account_page::compute_account_list_columns(&[row.clone()], 80, Language::Chinese);
    let header = account_page::format_account_list_header_with_columns(columns, Language::Chinese);
    assert!(header.contains("小米"));
    assert!(header.contains("米家"));
    let item = account_page::format_account_list_item_with_columns(&row, columns);
    assert!(item.contains("已登录"));
}

#[test]
fn account_list_row_marks_missing_tokens_independently() {
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-account-status-missing",
        "deviceId": "mico.tui-account-status-missing",
        "state": "state-a",
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));

    let row = account_page::account_list_row(&account, false, Language::Chinese);

    assert_eq!(row.xiaomi_status, "未登录");
    assert_eq!(row.mijia_status, "未登录");
}

#[cfg(unix)]
#[test]
fn forward_auth_login_output_sends_url_before_eof() {
    use std::io::Write as _;
    use std::os::unix::net::UnixStream;

    let (mut writer, reader) = UnixStream::pair().unwrap();
    let (tx, rx) = mpsc::channel();

    let handle = std::thread::spawn(move || {
        let mut reader = std::io::BufReader::new(reader);
        account_page::forward_auth_login_output_until_eof(&mut reader, &tx);
    });

    writer
            .write_all(
                b"{\"type\":\"authUrlPrinted\",\"url\":\"https://example.com/oauth\"}\n{\"type\":\"authWaiting\"}\n",
            )
            .unwrap();
    writer.flush().unwrap();

    let auth_url = rx
        .recv_timeout(Duration::from_millis(300))
        .unwrap()
        .unwrap();
    assert_eq!(auth_url, "https://example.com/oauth");

    drop(writer);
    handle.join().unwrap();
}

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
fn collect_readable_props_prefers_description_trans_copy() {
    let spec = json!({
        "services": [
            {
                "iid": 2,
                "properties": [
                    {
                        "iid": 1,
                        "description": "Power",
                        "description_trans": "电源",
                        "format": "bool",
                        "access": ["read", "write"]
                    }
                ]
            }
        ]
    });
    let props = collect_readable_props(&spec, Language::Chinese);
    assert_eq!(props.len(), 1);
    assert_eq!(props[0].name, "电源");
    assert!(props[0].writable);
}

#[test]
fn collect_readable_props_includes_read_only_and_combines_service_and_property_labels() {
    let spec = json!({
        "services": [
            {
                "iid": 2,
                "description": "Speaker Service",
                "description_trans": "扬声器服务",
                "properties": [
                    {
                        "iid": 1,
                        "description": "Power",
                        "description_trans": "电源",
                        "format": "bool",
                        "access": ["read", "write"]
                    },
                    {
                        "iid": 2,
                        "description": "ReadOnlyVolume",
                        "description_trans": "只读音量",
                        "format": "uint8",
                        "access": ["read"]
                    }
                ]
            }
        ]
    });

    let props = collect_readable_props(&spec, Language::Chinese);
    assert_eq!(props.len(), 2);
    assert_eq!(props[0].name, "扬声器服务 / 电源");
    assert_eq!(props[1].name, "扬声器服务 / 只读音量");
    assert!(props[0].writable);
    assert!(!props[1].writable);
}

#[test]
fn collect_readable_props_hides_properties_without_read_access() {
    let spec = json!({
        "services": [
            {
                "iid": 2,
                "properties": [
                    {
                        "iid": 1,
                        "description": "Readable",
                        "format": "bool",
                        "access": ["read", "write"]
                    },
                    {
                        "iid": 2,
                        "description": "WriteOnly",
                        "format": "bool",
                        "access": ["write"]
                    },
                    {
                        "iid": 3,
                        "description": "NoAccess",
                        "format": "bool",
                        "access": []
                    },
                    {
                        "iid": 4,
                        "description": "MissingAccess",
                        "format": "bool"
                    }
                ]
            }
        ]
    });

    let props = collect_readable_props(&spec, Language::Chinese);
    assert_eq!(props.len(), 1);
    assert_eq!(props[0].name, "Readable");
}

#[test]
fn format_prop_dialog_action_list_item_line_shows_only_action_name() {
    let action = ActionItem {
        siid: 5,
        aiid: 1,
        name: "开灯".to_string(),
        input_piids: vec![1, 2],
        input_labels: vec!["参数1".to_string(), "参数2".to_string()],
        input_props: Vec::new(),
    };

    assert_eq!(
        format_prop_dialog_action_list_item_line(&action, true),
        "> 开灯"
    );
    assert_eq!(
        format_prop_dialog_action_list_item_line(&action, false),
        "  开灯"
    );
}

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
fn format_device_list_item_orders_columns_room_name_category_account() {
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
fn display_truncate_pad_pads_ascii_to_width() {
    let result = display_truncate_pad("hi", 5);
    assert_eq!(result, "hi   ");
    assert_eq!(result.len(), 5);
}

#[test]
fn display_truncate_pad_truncates_long_ascii() {
    let result = display_truncate_pad("hello world", 5);
    assert_eq!(result, "hello");
}

#[test]
fn display_truncate_pad_handles_cjk_double_width() {
    // "音箱" is 2 CJK chars, each 2 display cols wide = 4 total
    let result = display_truncate_pad("音箱", 10);
    assert_eq!(UnicodeWidthStr::width(result.as_str()), 10);
    assert!(result.starts_with("音箱"));
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
fn draw_accounts_selected_row_uses_reversed_style() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let account_a = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-account-a",
        "deviceId": "mico.tui-account-a",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 1,
        "user": {"uid": "1001", "nickname": "A", "icon": "", "unionId": "union-a"}
    }));
    let account_b = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-account-b",
        "deviceId": "mico.tui-account-b",
        "state": "state-b",
        "accessToken": "token-b",
        "refreshToken": "refresh-b",
        "expiresTs": 1,
        "user": {"uid": "1002", "nickname": "B", "icon": "", "unionId": "union-b"}
    }));
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![account_a, account_b],
        account_index: 1,
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
fn devices_search_focus_footer_shows_enter_escape_and_match_count() {
    let mut app = devices_tab_test_app(vec![
        test_device("dev-kitchen", "kitchen plug", "Kitchen", "A(1001)"),
        test_device("dev-bed", "bedroom lamp", "Bedroom", "A(1001)"),
    ]);
    app.language = Language::English;

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

    assert_eq!(
        super::footer_text(&app),
        "Esc: Back, Enter: View Device, Matched devices: 1 Current Device: dev-bed"
    );
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
fn draw_accounts_scrolls_to_keep_active_row_visible() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let accounts = (0..12)
        .map(|idx| {
            normalize_account(json!({
                "region": "cn",
                "redirectUri": "http://127.0.0.1:8000/login_redirect",
                "uuid": format!("tui-scroll-account-{idx}"),
                "deviceId": format!("mico.tui-scroll-account-{idx}"),
                "state": format!("state-{idx}"),
                "accessToken": format!("token-{idx}"),
                "refreshToken": format!("refresh-{idx}"),
                "expiresTs": 1,
                "user": {
                    "uid": format!("10{idx:02}"),
                    "nickname": format!("acc{idx}"),
                    "icon": "",
                    "unionId": format!("union-{idx}")
                }
            }))
        })
        .collect::<Vec<_>>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts,
        account_index: 10,
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
    assert!(text.contains("acc10"), "{text}");
    assert!(!text.contains("acc0"), "{text}");
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
fn pressing_enter_on_accounts_tab_opens_account_action_dialog() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-account-actions",
        "deviceId": "mico.tui-account-actions",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 1,
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
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(!quit);

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    super::clear_selection_state();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");
    assert!(compact.contains("推送消息"), "{text}");
    assert!(compact.contains("重新登录(小米)"), "{text}");
    assert!(compact.contains("重新登录(米家)"), "{text}");
    assert!(compact.contains("登出"), "{text}");
    assert!(!text.contains("view-device"));
}

#[test]
fn account_action_menu_mouse_wheel_changes_selected_item() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-account-actions-wheel",
        "deviceId": "mico.tui-account-actions-wheel",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 1,
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
        active_tab: 0,
        log_scroll_offset: 0,
        input_mode: false,
        input: String::new(),
        device_search_cursor: 0,
        search_inputs: Default::default(),
        search_cursors: [0; 3],
        prop_dialog: None,
        account_action_dialog: Some(AccountActionDialog::Menu { selected: 0 }),
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
    let area = ratatui::layout::Rect::new(0, 0, 80, 24);
    let popup = super::centered_rect(48, 34, area);

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollDown,
            column: popup.x + 2,
            row: popup.y + 1,
            modifiers: KeyModifiers::NONE,
        },
        area,
    )
    .unwrap();
    assert!(matches!(
        app.account_action_dialog,
        Some(AccountActionDialog::Menu { selected: 1 })
    ));

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollUp,
            column: popup.x + 2,
            row: popup.y + 1,
            modifiers: KeyModifiers::NONE,
        },
        area,
    )
    .unwrap();
    assert!(matches!(
        app.account_action_dialog,
        Some(AccountActionDialog::Menu { selected: 0 })
    ));
}

#[test]
fn clicking_selected_account_action_executes_it() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-account-actions-click",
        "deviceId": "mico.tui-account-actions-click",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 1,
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
        active_tab: 0,
        log_scroll_offset: 0,
        input_mode: false,
        input: String::new(),
        device_search_cursor: 0,
        search_inputs: Default::default(),
        search_cursors: [0; 3],
        prop_dialog: None,
        account_action_dialog: Some(AccountActionDialog::Menu { selected: 0 }),
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
    let area = ratatui::layout::Rect::new(0, 0, 80, 24);
    let popup = super::centered_rect(48, 34, area);

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: popup.x + 2,
            row: popup.y + 1,
            modifiers: KeyModifiers::NONE,
        },
        area,
    )
    .unwrap();

    assert!(matches!(
        app.account_action_dialog,
        Some(AccountActionDialog::PushMessage { .. })
    ));
}

#[test]
fn push_message_action_opens_input_dialog() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-push-message",
        "deviceId": "mico.tui-push-message",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 1,
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
        active_tab: 0,
        log_scroll_offset: 0,
        input_mode: false,
        input: String::new(),
        device_search_cursor: 0,
        search_inputs: Default::default(),
        search_cursors: [0; 3],
        prop_dialog: None,
        account_action_dialog: Some(AccountActionDialog::Menu { selected: 0 }),
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
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(!quit);
    assert!(matches!(
        app.account_action_dialog,
        Some(AccountActionDialog::PushMessage {
            ref uid,
            ref input,
            cursor,
            ..
        }) if uid == "1001" && input.is_empty() && cursor == 0
    ));
}

#[test]
fn push_message_dialog_shows_cursor_and_moves_with_left_right() {
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
        account_action_dialog: Some(AccountActionDialog::PushMessage {
            uid: "1001".to_string(),
            input: "hi".to_string(),
            cursor: 2,
            return_to_menu_selected: 0,
        }),
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
    assert!(text.contains("hi"), "{text}");
    assert!(!text.contains("h|i"), "{text}");
    assert!(!text.contains("hi|"), "{text}");
    assert!(terminal_has_reversed_cell(&terminal));

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
    )
    .unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("hi"), "{text}");
    assert!(!text.contains("h|i"), "{text}");
    assert!(!text.contains("hi|"), "{text}");
    assert!(terminal_has_reversed_cell(&terminal));

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
    )
    .unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("hi"), "{text}");
    assert!(!text.contains("h|i"), "{text}");
    assert!(!text.contains("hi|"), "{text}");
    assert!(terminal_has_reversed_cell(&terminal));
}

#[test]
fn push_message_cursor_row_stays_stable_when_typing_first_char() {
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
        account_action_dialog: Some(AccountActionDialog::PushMessage {
            uid: "1001".to_string(),
            input: String::new(),
            cursor: 0,
            return_to_menu_selected: 0,
        }),
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
    let row_before = terminal_first_reversed_cell_row(&terminal).unwrap();

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
    )
    .unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let row_after = terminal_first_reversed_cell_row(&terminal).unwrap();

    assert_eq!(row_before, row_after);
}

#[test]
fn push_message_dialog_submits_text_for_selected_account_uid() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-push-message-submit",
        "deviceId": "mico.tui-push-message-submit",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 1,
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
        active_tab: 0,
        log_scroll_offset: 0,
        input_mode: false,
        input: String::new(),
        device_search_cursor: 0,
        search_inputs: Default::default(),
        search_cursors: [0; 3],
        prop_dialog: None,
        account_action_dialog: Some(AccountActionDialog::PushMessage {
            uid: "1001".to_string(),
            input: String::new(),
            cursor: 0,
            return_to_menu_selected: 0,
        }),
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

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE),
    )
    .unwrap();
    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE),
    )
    .unwrap();
    let quit = handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(!quit);
    assert!(app.account_action_dialog.is_none());
    assert!(app.logs.iter().any(|line| {
        line.contains("push message") && line.contains("1001") && line.contains("hi")
    }));
}

#[test]
fn escaping_push_message_dialog_restores_previous_menu_selection() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-account-actions-escape",
        "deviceId": "mico.tui-account-actions-escape",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 1,
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
        active_tab: 0,
        log_scroll_offset: 0,
        input_mode: false,
        input: String::new(),
        device_search_cursor: 0,
        search_inputs: Default::default(),
        search_cursors: [0; 3],
        prop_dialog: None,
        account_action_dialog: Some(AccountActionDialog::Menu { selected: 0 }),
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

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();
    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
    )
    .unwrap();

    assert!(matches!(
        app.account_action_dialog,
        Some(AccountActionDialog::Menu { selected: 0 })
    ));
}

#[test]
fn add_account_port_conflict_shows_error_dialog_without_quitting_tui() {
    let _guard = crate::test_support::env_guard();
    std::env::set_var(
        "MIT_TUI_TEST_REAUTH_ERROR",
        "port 8000 is occupied by another process",
    );

    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-add-account-port-conflict",
        "deviceId": "mico.tui-add-account-port-conflict",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 1,
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
        crossterm::event::KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
    )
    .unwrap();
    assert!(!quit);

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("port 8000 is occupied"));

    std::env::remove_var("MIT_TUI_TEST_REAUTH_ERROR");
}

#[test]
fn draw_does_not_render_command_bar() {
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
    assert!(!text.contains("':' command"));
    assert!(!text.contains("Command (Enter run, Esc cancel)"));
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
fn load_cached_devices_from_home_ignores_local_credentials_for_all_accounts() {
    let home = make_temp_dir("tui-local-credentials-merge-all-accounts");
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
            "dev-a": {
                "did": "dev-a",
                "name": "speaker-a",
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
            "dev-b": {
                "did": "dev-b",
                "name": "speaker-b",
                "model": "xiaomi.wifispeaker.lx04",
                "localIp": "192.168.1.12",
                "token": "ffeeddccbbaa99887766554433221100",
                "source": "direct"
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let devices = load_cached_devices_from_home(&home, "").unwrap();
    assert!(devices.is_empty());

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
    }
}

fn app_with_single_readonly_prop_dialog() -> TuiApp {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: Vec::new(),
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
    let column = footer.x.saturating_add(super::display_width(prefix) as u16);
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
fn handle_key_blocks_normal_actions_until_boot_ready() {
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

    let changed = handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(!changed);
    assert_eq!(app.active_tab, 0);

    let quit = handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
    )
    .unwrap();
    assert!(quit);
}

#[test]
fn opening_prop_dialog_failure_shows_offline_instead_of_quitting() {
    let home = make_temp_dir("tui-bool-dialog-offline");
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
        devices: vec![Device {
            did: "dev-offline".to_string(),
            name: "offline-speaker".to_string(),
            model: "xiaomi.wifispeaker.lx04".to_string(),
            online: false,

            home_id: "home-1".to_string(),
            home_name: "我家".to_string(),
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
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(!quit);

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("offline"));
    assert!(app.prop_dialog.is_some());

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn opening_prop_dialog_uses_device_account_instead_of_selected_account() {
    let home = make_temp_dir("tui-bool-dialog-device-account");
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
    let account_a = normalize_account(json!({
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
    let account_b = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "",
        "deviceId": "",
        "state": "",
        "accessToken": "",
        "refreshToken": "",
        "expiresTs": 0,
        "user": {"uid": "1002", "nickname": "账号B", "icon": "", "unionId": "union-b"}
    }));
    let mut app = TuiApp {
        home_dir: home.clone(),
        auth_state: default_auth(),
        accounts: vec![account_a, account_b],
        account_index: 1,
        devices: vec![Device {
            did: "dev-account-a".to_string(),
            name: "speaker-a".to_string(),
            model: "xiaomi.wifispeaker.lx04".to_string(),
            online: false,

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
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(!quit);

    let account_uid = app
        .prop_dialog
        .as_ref()
        .map(|dialog| dialog.account_uid.as_str())
        .unwrap_or_default();
    assert_eq!(account_uid, "1001");

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn pressing_j_does_not_move_selection_anymore() {
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
        crossterm::event::KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
    )
    .unwrap();
    assert!(!quit);
    assert_eq!(app.device_index, 0);
}

#[test]
fn devices_tab_enter_opens_prop_dialog_and_p_does_not() {
    let home = make_temp_dir("tui-devices-enter-open");
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
        devices: vec![Device {
            did: "dev-offline".to_string(),
            name: "offline-speaker".to_string(),
            model: "xiaomi.wifispeaker.lx04".to_string(),
            online: false,

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
        crossterm::event::KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE),
    )
    .unwrap();
    assert!(!quit);
    assert!(app.prop_dialog.is_none());

    let quit = handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(!quit);
    assert!(app.prop_dialog.is_some());

    let _ = fs::remove_dir_all(&home);
}

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
fn clicking_accounts_row_selects_account() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let account_a = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-click-account-a",
        "deviceId": "mico.tui-click-account-a",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 1,
        "user": {"uid": "1001", "nickname": "A", "icon": "", "unionId": "union-a"}
    }));
    let account_b = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-click-account-b",
        "deviceId": "mico.tui-click-account-b",
        "state": "state-b",
        "accessToken": "token-b",
        "refreshToken": "refresh-b",
        "expiresTs": 1,
        "user": {"uid": "1002", "nickname": "B", "icon": "", "unionId": "union-b"}
    }));
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![account_a, account_b],
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
    assert_eq!(app.account_index, 1);
    assert!(app.account_action_dialog.is_none());
}

#[test]
fn mouse_wheel_scroll_changes_active_item() {
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
            kind: crossterm::event::MouseEventKind::ScrollDown,
            column: 2,
            row: 4,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();
    assert_eq!(app.device_index, 1);

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollUp,
            column: 2,
            row: 4,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();
    assert_eq!(app.device_index, 0);
}

#[test]
fn mouse_click_is_ignored_while_prop_editing() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
            items: vec![
                ToggleItem {
                    prop: PropItem {
                        siid: 2,
                        piid: 1,
                        name: "Power".to_string(),
                        format: "bool".to_string(),
                        writable: true,
                        value_options: Vec::new(),
                    },
                    value: Value::Bool(true),
                },
                ToggleItem {
                    prop: PropItem {
                        siid: 2,
                        piid: 2,
                        name: "Brightness".to_string(),
                        format: "uint8".to_string(),
                        writable: true,
                        value_options: Vec::new(),
                    },
                    value: json!(50),
                },
            ],
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
            editing: true,
            edit_buffer: "true".to_string(),
            edit_cursor: 2,
            edit_error: None,
            refreshing: false,
            refresh_rx: None,
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
            column: 4,
            row: 5,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();

    assert_eq!(app.prop_dialog.as_ref().unwrap().selected, 0);
}

#[test]
fn number_shortcuts_switch_tabs() {
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

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE),
    )
    .unwrap();
    assert_eq!(app.active_tab, 1);

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE),
    )
    .unwrap();
    assert_eq!(app.active_tab, 2);

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE),
    )
    .unwrap();
    assert_eq!(app.active_tab, 3);

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
    )
    .unwrap();
    assert_eq!(app.active_tab, 0);
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
fn clicking_top_bar_tabs_switches_active_tab() {
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
            column: 12,
            row: 1,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();
    assert_eq!(app.active_tab, 1);

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 22,
            row: 1,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();
    assert_eq!(app.active_tab, 2);

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 31,
            row: 1,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();
    assert_eq!(app.active_tab, 3);
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
fn settings_tab_enter_on_reset_option_removes_mit_dir_after_single_confirm() {
    let home = make_temp_dir("tui-settings-reset-all");
    let test_home = home.join("test");
    let mit_dir = test_home.join(".mit");
    let _ = fs::create_dir_all(mit_dir.join("accounts").join("1001"));
    fs::write(mit_dir.join("auth.json"), "{\"accounts\":[]}\n").unwrap();
    fs::write(
        mit_dir.join("accounts").join("1001").join("devices.json"),
        "{\"devices\":[]}\n",
    )
    .unwrap();
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: test_home.clone(),
        auth_state: default_auth(),
        accounts: vec![test_account()],
        account_index: 0,
        devices: Vec::new(),
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
        boot_spinner_index: 1,
        bootstrap_generation: 0,
        bootstrap_pending: None,
        bootstrap_tx,
        bootstrap_rx,
        property_cache: Arc::new(PropertyCache::new()),
        language: Language::Chinese,
        auto_subscribe_device_status: true,
        settings_selected: 3,
    };

    let quit = handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(!quit);
    assert!(mit_dir.exists());
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");
    assert!(compact.contains("操作:重置全部设置"), "{text}");
    assert!(compact.contains("该操作不可恢复"), "{text}");

    let quit = handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(quit);
    assert!(!mit_dir.exists());
    assert!(app.accounts.is_empty());
    assert!(app.devices.is_empty());
    assert!(app.logs.iter().any(|line| line.contains("已重置全部设置")));

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn settings_tab_shows_auto_subscribe_cache_clear_and_reset_actions() {
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
        settings_selected: 0,
    };

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");
    assert!(
        compact.contains("自动订阅设备状态(关闭后始终需要手动刷新)：开启"),
        "{text}"
    );
    assert!(compact.contains("重置设备缓存"), "{text}");
    assert!(compact.contains("重置全部设置"), "{text}");
    assert!(!compact.contains("规格缓存时间"), "{text}");
    assert!(!compact.contains("语言偏好"), "{text}");
}

#[test]
fn settings_tab_enter_toggles_auto_subscribe_and_persists() {
    let _guard = env_guard();
    let home = make_temp_dir("tui-settings-auto-subscribe");
    std::env::set_var("MIT_HOME", &home);
    std::env::set_var("MIT_PROFILE_DIR", &home);
    let mut app = devices_tab_test_app(Vec::new());
    app.home_dir = home.clone();
    app.active_tab = 3;
    app.settings_selected = 1;

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();

    assert!(app.account_action_dialog.is_none());
    let settings_text = fs::read_to_string(home.join(".mit").join("settings.json")).unwrap();
    assert!(settings_text.contains("\"autoSubscribeDeviceStatus\": false"));

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");
    assert!(
        compact.contains("自动订阅设备状态(关闭后始终需要手动刷新)：关闭"),
        "{text}"
    );

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn clicking_footer_does_not_copy_status_line() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "footer-copy-a",
            "deviceId": "mico.footer-copy-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 1,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
        account_index: 0,
        devices: vec![Device {
            did: "dev-1".to_string(),
            name: "d1".to_string(),
            model: "xiaomi.wifispeaker.lx04".to_string(),
            online: true,

            home_id: "home-1".to_string(),
            home_name: "我家".to_string(),
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

    let _guard = env_guard();
    let clip_file = make_temp_dir("tui-footer-copy").join("clipboard.txt");
    std::env::set_var("MIT_TEST_CLIPBOARD_FILE", &clip_file);

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 2,
            row: 22,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();

    assert!(!clip_file.exists());
    assert!(!app
        .logs
        .iter()
        .any(|line| line.starts_with(super::FOOTER_COPY_LOG_PREFIX)));

    std::env::remove_var("MIT_TEST_CLIPBOARD_FILE");
    let _ = fs::remove_file(&clip_file);
}

#[test]
fn mouse_selection_state_is_thread_local() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let push_input = "hello world".to_string();
    let push_area =
        super::push_message_textarea_area(ratatui::layout::Rect::new(0, 0, 80, 24), &push_input);
    let push_mouse = crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: push_area.x,
        row: push_area.y,
        modifiers: KeyModifiers::NONE,
    };
    let push_terminal = ratatui::layout::Rect::new(0, 0, 80, 24);

    super::clear_selection_state();
    let push_app = TuiApp {
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
        account_action_dialog: Some(AccountActionDialog::PushMessage {
            uid: "1001".to_string(),
            input: push_input,
            cursor: 11,
            return_to_menu_selected: 0,
        }),
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
    assert!(!super::selection_start(
        &push_app,
        push_mouse,
        push_terminal
    ));
    assert_eq!(
        super::selected_surface()
            .expect("push-message selection should be active")
            .snapshot
            .surface,
        super::SelectionSurface::PushMessageInput
    );

    thread::spawn(|| {
        let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
        let (local_transport_tx, local_transport_rx) =
            mpsc::channel::<LocalTransportRefreshMessage>();
        let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
        let logs_app = TuiApp {
            home_dir: PathBuf::from("."),
            auth_state: default_auth(),
            accounts: Vec::new(),
            account_index: 0,
            devices: Vec::new(),
            device_index: 0,
            logs: VecDeque::from([String::from("log line")]),
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
        let [_tabs_area, content_area, _status_gap_area, _status_bar_area] =
            super::split_main_layout(ratatui::layout::Rect::new(0, 0, 80, 24));
        let [_search_area, _search_border_area, list_area] =
            super::searchable_main_layout(content_area);
        super::clear_selection_state();
        assert!(super::selection_start(
            &logs_app,
            crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left,),
                column: list_area.x,
                row: list_area.y,
                modifiers: KeyModifiers::NONE,
            },
            ratatui::layout::Rect::new(0, 0, 80, 24),
        ));
        assert_eq!(
            super::selected_surface()
                .expect("log selection should be active")
                .snapshot
                .surface,
            super::SelectionSurface::Logs
        );
    })
    .join()
    .unwrap();

    assert_eq!(
        super::selected_surface()
            .expect("push-message selection should remain isolated")
            .snapshot
            .surface,
        super::SelectionSurface::PushMessageInput
    );
}

#[test]
fn selected_push_message_textarea_text_uses_selection_background() {
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
        account_action_dialog: Some(AccountActionDialog::PushMessage {
            uid: "1001".to_string(),
            input: "hello world".to_string(),
            cursor: 11,
            return_to_menu_selected: 0,
        }),
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

    super::clear_selection_state();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let textarea_area =
        super::push_message_textarea_area(ratatui::layout::Rect::new(0, 0, 80, 24), "hello world");
    let column = textarea_area.x;
    let row = textarea_area.y;

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Left),
            column: column + 5,
            row,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Up(crossterm::event::MouseButton::Left),
            column: column + 5,
            row,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();

    let selection = super::selected_surface().expect("textarea selection should persist");
    assert_eq!(
        selection.snapshot.surface,
        super::SelectionSurface::PushMessageInput
    );
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let buffer = terminal.backend().buffer();
    let has_selection_bg = (0..buffer.area.height)
        .any(|y| (0..buffer.area.width).any(|x| buffer[(x, y)].bg == Color::DarkGray));
    assert!(has_selection_bg);
}

#[test]
fn footer_leaves_blank_rows_above_and_below_status_text() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![test_account()],
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

    let lines = terminal_text(&terminal)
        .lines()
        .map(str::to_string)
        .collect::<Vec<_>>();
    assert!(lines[21].trim().is_empty(), "{:?}", lines);
    assert!(!lines[22].trim().is_empty(), "{:?}", lines);
    assert!(lines[23].trim().is_empty(), "{:?}", lines);
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
fn shift_c_recopies_last_mouse_selection() {
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
        logs: VecDeque::from(["gamma".to_string()]),
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

    let _guard = env_guard();
    let clip_file = make_temp_dir("tui-shift-c-copy").join("clipboard.txt");
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

    let _ = fs::remove_file(&clip_file);
    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('C'), KeyModifiers::SHIFT),
    )
    .unwrap();
    let copied = fs::read_to_string(&clip_file).unwrap();
    assert_eq!(copied, "gamma");

    std::env::remove_var("MIT_TEST_CLIPBOARD_FILE");
    let _ = fs::remove_file(&clip_file);
}

#[test]
fn plain_click_outside_selected_text_clears_selection() {
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
        logs: VecDeque::from(["gamma".to_string()]),
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

    let _guard = env_guard();
    let clip_file = make_temp_dir("tui-clear-selection-outside-click").join("clipboard.txt");
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
            column: 5,
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
            column: 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 20,
            row: 10,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();

    let _ = fs::remove_file(&clip_file);
    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('C'), KeyModifiers::SHIFT),
    )
    .unwrap();
    assert!(
        !clip_file.exists(),
        "selection should be cleared by outside click"
    );

    std::env::remove_var("MIT_TEST_CLIPBOARD_FILE");
    let _ = fs::remove_file(&clip_file);
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

#[test]
fn selected_footer_keeps_dim_style() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![test_account()],
        account_index: 0,
        devices: vec![Device {
            did: "dev-1".to_string(),
            name: "d1".to_string(),
            model: "xiaomi.wifispeaker.lx04".to_string(),
            online: true,

            home_id: "home-1".to_string(),
            home_name: "我家".to_string(),
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
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 1,
            row: 22,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    let buffer = terminal.backend().buffer();
    let maybe_col = (0..buffer.area.width).find(|x| buffer[(*x, 22)].symbol() != " ");
    let col = maybe_col.expect("footer should render non-space chars");
    let cell = &buffer[(col, 22)];
    assert!(cell.modifier.contains(Modifier::DIM), "{cell:?}");
}

#[test]
fn footer_copied_badge_is_bold_and_expires_after_one_second() {
    let mut logs = VecDeque::new();
    logs.push_back(format!("{}{}", super::FOOTER_COPY_LOG_PREFIX, 10_000));

    assert!(super::footer_copy_badge_visible_at(&logs, 10_999));
    assert!(!super::footer_copy_badge_visible_at(&logs, 11_000));

    let app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: Vec::new(),
        account_index: 0,
        devices: vec![Device {
            did: "dev-1".to_string(),
            name: "d1".to_string(),
            model: "xiaomi.wifispeaker.lx04".to_string(),
            online: true,

            home_id: "home-1".to_string(),
            home_name: "我家".to_string(),
            room_id: "room-1".to_string(),
            room_name: "客厅".to_string(),
        }],
        device_index: 0,
        logs,
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
    assert_eq!(line.spans[1].style.fg, Some(Color::Blue));
    assert!(line.spans[1].style.add_modifier.contains(Modifier::BOLD));

    let no_badge = super::footer_line(&app, 11_000);
    assert_eq!(no_badge.spans.len(), 1);
}

#[test]
fn footer_text_matches_requested_status_copy() {
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
        devices: vec![Device {
            did: "dev-1".to_string(),
            name: "speaker".to_string(),
            model: "xiaomi.wifispeaker.lx04".to_string(),
            online: true,

            home_id: "home-1".to_string(),
            home_name: "我家".to_string(),
            room_id: "room-1".to_string(),
            room_name: "客厅".to_string(),
        }],
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

    assert_eq!(
        super::footer_text(&app),
        "A: 新增账户, /: 搜索, Enter: 账户操作"
    );

    app.active_tab = 1;
    assert_eq!(
        super::footer_text(&app),
        "R: 刷新, /: 搜索, Enter: 查看设备, 设备总数: 1 当前设备: dev-1"
    );

    app.prop_dialog = Some(PropDialog {
        device_did: "dev-1".to_string(),
        device_name: "speaker".to_string(),
        account_uid: "1001".to_string(),
        items: vec![ToggleItem {
            prop: PropItem {
                siid: 2,
                piid: 1,
                name: "Power".to_string(),
                format: "bool".to_string(),
                writable: true,
                value_options: Vec::new(),
            },
            value: Value::Bool(true),
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
    });
    assert_eq!(
        super::footer_text(&app),
        "R: 刷新, Esc: 返回, Enter: 修改属性, 当前设备: dev-1"
    );

    if let Some(dialog) = app.prop_dialog.as_mut() {
        dialog.active_tab = PropDialogTab::ReadOnly;
    }
    assert_eq!(
        super::footer_text(&app),
        "R: 刷新, Esc: 返回, Enter: 查看属性, 当前设备: dev-1"
    );

    if let Some(dialog) = app.prop_dialog.as_mut() {
        dialog.editing = true;
    }
    assert_eq!(super::footer_text(&app), "Esc: 返回, 当前设备: dev-1");

    app.prop_dialog = None;
    app.account_action_dialog = Some(AccountActionDialog::Menu { selected: 0 });
    assert_eq!(super::footer_text(&app), "Enter: 选择, Esc: 返回");

    app.account_action_dialog = Some(AccountActionDialog::PushMessage {
        uid: "1001".to_string(),
        input: String::new(),
        cursor: 0,
        return_to_menu_selected: 0,
    });
    assert_eq!(super::footer_text(&app), "Enter: 发送, Esc: 返回");

    app.account_action_dialog = Some(AccountActionDialog::Reauth {
        status: "reauth".to_string(),
        auth_url: "http://127.0.0.1".to_string(),
    });
    assert_eq!(super::footer_text(&app), "C: 复制, Esc: 返回".to_string());

    app.account_action_dialog = None;
    app.active_tab = 2;
    assert_eq!(super::footer_text(&app), "C: 清空, /: 搜索");
}

#[test]
fn clicking_refresh_operation_in_footer_triggers_sync() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![test_account()],
        account_index: 0,
        devices: vec![Device {
            did: "dev-1".to_string(),
            name: "living-room".to_string(),
            model: "xiaomi.wifispeaker.lx04".to_string(),
            online: true,
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

    let terminal_area = ratatui::layout::Rect::new(0, 0, 100, 24);
    let (column, row) = footer_click_point(&app, terminal_area, "R: 刷新");

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

    assert!(app.bootstrap_pending.is_some());
}

#[test]
fn clicking_esc_operation_in_footer_matches_escape_behavior() {
    let dialog = PropDialog {
        device_did: "dev-1".to_string(),
        device_name: "living-room".to_string(),
        account_uid: "1001".to_string(),
        items: vec![ToggleItem {
            prop: PropItem {
                siid: 2,
                piid: 1,
                name: "电源".to_string(),
                format: "bool".to_string(),
                writable: true,
                value_options: Vec::new(),
            },
            value: Value::Bool(true),
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
    };
    let mut app = test_app_with_prop_dialog(dialog);

    let terminal_area = ratatui::layout::Rect::new(0, 0, 100, 24);
    let (column, row) = footer_click_point(&app, terminal_area, "Esc: 返回");

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

    assert!(app.prop_dialog.is_none());
}

#[test]
fn clicking_search_operation_in_footer_focuses_device_search() {
    let mut app = devices_tab_test_app(vec![test_device(
        "dev-kitchen",
        "kitchen plug",
        "Kitchen",
        "A(1001)",
    )]);
    app.language = Language::English;
    app.input = "stale".to_string();
    app.device_search_cursor = app.input.chars().count();
    let terminal_area = ratatui::layout::Rect::new(0, 0, 100, 24);
    let (column, row) = footer_click_point(&app, terminal_area, "/: Search");

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

    assert!(app.input_mode);
    assert_eq!(app.input, "stale");
    assert_eq!(app.device_search_cursor, 5);
}

#[test]
fn clicking_non_operation_footer_text_has_no_effect() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![test_account()],
        account_index: 0,
        devices: vec![Device {
            did: "dev-1".to_string(),
            name: "living-room".to_string(),
            model: "xiaomi.wifispeaker.lx04".to_string(),
            online: true,
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

    let terminal_area = ratatui::layout::Rect::new(0, 0, 100, 24);
    let (column, row) = footer_click_point(&app, terminal_area, "当前设备");

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

    assert!(app.bootstrap_pending.is_none());
    assert!(app.prop_dialog.is_none());
}

#[test]
fn exit_result_for_boot_state_is_ok_for_all_states() {
    let ok = exit_result_for_boot_state(&BootState::Ready);
    assert!(ok.is_ok());
}

#[test]
fn start_bootstrap_without_current_account_enters_ready_state() {
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
        local_transport_refresh_generation: 5,
        local_transport_refresh_device_id: None,
        local_transport_force_refresh_pending: true,
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
    assert!(app.bootstrap_pending.is_none());
    assert_eq!(app.local_transport_refresh_generation, 6);
    assert!(!app.local_transport_force_refresh_pending);
    assert!(app.logs.iter().any(|line| line.contains("不存在可用账号")));
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
fn process_bootstrap_message_marks_app_ready_after_success() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();

    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-bootstrap",
        "deviceId": "mico.tui-bootstrap",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 1,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));
    let mut auth_state = default_auth();
    auth_state.accounts = vec![account.clone()];

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
        boot_spinner_index: 0,
        bootstrap_generation: 1,
        bootstrap_pending: Some(BootstrapPending {
            generation: 1,
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
            generation: 1,
            uid: "1001".to_string(),
            offline_uids: Vec::new(),
            auth_state,
            accounts: vec![account.clone()],
            devices: vec![Device {
                did: "dev-1".to_string(),
                name: "living-room".to_string(),
                model: "xiaomi.wifispeaker.lx04".to_string(),
                online: true,

                home_id: "home-1".to_string(),
                home_name: "我家".to_string(),
                room_id: "room-1".to_string(),
                room_name: "客厅".to_string(),
            }],
            logs: vec!["loaded 1 devices".to_string()],
        })
        .unwrap();

    app.process_background_messages();

    assert_eq!(app.boot_state, BootState::Ready);
    assert_eq!(app.devices.len(), 1);
    assert_eq!(app.accounts.len(), 1);
    assert_eq!(app.accounts[0].user.uid, "1001");
    assert!(!app.local_transport_fetching);
    assert_eq!(app.local_transport_refresh_generation, 0);
    assert!(app.local_transport_refresh_device_id.is_none());
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
fn request_local_transport_refresh_skips_account_after_session_warm() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-refresh-skip",
        "deviceId": "mico.tui-refresh-skip",
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
        local_transport_fetching: false,
        local_transport_refresh_generation: 0,
        local_transport_refresh_device_id: Some(account.device_id.clone()),
        local_transport_force_refresh_pending: false,
        local_transport_tx,
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

    app.request_local_transport_refresh(false);

    assert!(!app.local_transport_fetching);
    assert_eq!(
        app.local_transport_refresh_device_id.as_deref(),
        Some(account.device_id.as_str())
    );
}

#[test]
fn request_local_transport_refresh_force_rewarms_same_account() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-refresh-force",
        "deviceId": "mico.tui-refresh-force",
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
        local_transport_fetching: false,
        local_transport_refresh_generation: 0,
        local_transport_refresh_device_id: Some(account.device_id.clone()),
        local_transport_force_refresh_pending: false,
        local_transport_tx,
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

    app.request_local_transport_refresh(true);

    assert!(app.local_transport_fetching);
    assert_eq!(
        app.local_transport_refresh_device_id.as_deref(),
        Some(account.device_id.as_str())
    );
}

#[test]
fn request_local_transport_refresh_force_queues_when_fetching() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-refresh-queue",
        "deviceId": "mico.tui-refresh-queue",
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

    app.request_local_transport_refresh(true);
    assert!(app.local_transport_force_refresh_pending);
    assert!(app.local_transport_fetching);
    assert_eq!(app.local_transport_refresh_generation, 7);

    local_transport_tx
        .send(LocalTransportRefreshMessage {
            generation: 7,
            error: None,
        })
        .unwrap();
    app.process_background_messages();

    assert!(!app.local_transport_force_refresh_pending);
    assert!(app.local_transport_fetching);
    assert_eq!(app.local_transport_refresh_generation, 8);
}

#[test]
fn request_local_transport_refresh_queues_on_account_switch_while_fetching() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let account_a = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-refresh-a",
        "deviceId": "mico.tui-refresh-a",
        "state": "state-a",
        "accessToken": "",
        "refreshToken": "",
        "expiresTs": 0,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));
    let account_b = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-refresh-b",
        "deviceId": "mico.tui-refresh-b",
        "state": "state-b",
        "accessToken": "",
        "refreshToken": "",
        "expiresTs": 0,
        "user": {"uid": "1002", "nickname": "账号B", "icon": "", "unionId": "union-b"}
    }));
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![account_a.clone(), account_b.clone()],
        account_index: 1,
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
        local_transport_refresh_device_id: Some(account_a.device_id.clone()),
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

    app.request_local_transport_refresh(false);
    assert!(app.local_transport_force_refresh_pending);
    assert_eq!(app.local_transport_refresh_generation, 7);

    local_transport_tx
        .send(LocalTransportRefreshMessage {
            generation: 7,
            error: None,
        })
        .unwrap();
    app.process_background_messages();

    assert!(!app.local_transport_force_refresh_pending);
    assert!(app.local_transport_fetching);
    assert_eq!(app.local_transport_refresh_generation, 8);
    assert_eq!(
        app.local_transport_refresh_device_id.as_deref(),
        Some(account_b.device_id.as_str())
    );
}

#[test]
fn process_background_messages_logs_local_transport_refresh_errors() {
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
        local_transport_fetching: true,
        local_transport_refresh_generation: 7,
        local_transport_refresh_device_id: None,
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
            error: Some("snapshot write failed".to_string()),
        })
        .unwrap();

    app.process_background_messages();

    assert!(!app.local_transport_fetching);
    assert!(app
        .logs
        .iter()
        .any(|line| line.contains("snapshot write failed")));
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
fn process_auth_flow_completion_closes_reauth_dialog() {
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
        account_action_dialog: Some(AccountActionDialog::Reauth {
            status: "Waiting browser callback".to_string(),
            auth_url: "https://example.com/auth".to_string(),
        }),
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

    app.auth_flow_tx
        .send(AuthFlowMessage::Completed {
            generation: app.auth_flow_generation,
            success: true,
            detail: "callback received".to_string(),
        })
        .unwrap();

    app.process_background_messages();

    assert!(app.account_action_dialog.is_none());
}

#[test]
fn failed_auth_flow_shows_port_8000_hint_in_reauth_dialog() {
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
        account_action_dialog: Some(AccountActionDialog::Reauth {
            status: "Waiting browser callback".to_string(),
            auth_url: "http://127.0.0.1:8000/login".to_string(),
        }),
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

    app.auth_flow_tx
        .send(AuthFlowMessage::Completed {
            generation: app.auth_flow_generation,
            success: false,
            detail: "auth login process exited with exit status: 1".to_string(),
        })
        .unwrap();

    app.process_background_messages();

    let status = match app.account_action_dialog.as_ref() {
        Some(AccountActionDialog::Reauth { status, .. }) => status,
        _ => panic!("reauth dialog should remain open on auth flow failure"),
    };
    assert!(status.contains("Failed to complete auth flow"));
    assert!(status.contains("登录回调必须使用 8000 端口"));
}

#[test]
fn parse_auth_login_output_line_accepts_json_event_and_plain_url() {
    assert_eq!(
        account_page::parse_auth_login_output_line(
            r#"{"type":"authUrlPrinted","url":"https://example.com/oauth"}"#
        )
        .as_deref(),
        Some("https://example.com/oauth")
    );
    assert_eq!(
        account_page::parse_auth_login_output_line("AUTH_URL https://example.com/direct")
            .as_deref(),
        Some("https://example.com/direct")
    );
}

#[test]
fn prop_dialog_does_not_force_black_popup_background() {
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
        logs: VecDeque::from(vec!["visible beneath".to_string()]),
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
                    piid: 1,
                    name: "Power".to_string(),
                    format: "bool".to_string(),
                    writable: true,
                    value_options: Vec::new(),
                },
                value: Value::Bool(true),
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
            editing: false,
            edit_buffer: String::new(),
            edit_cursor: 0,
            edit_error: None,
            loading_rx: None,
            status: None,
            refreshing: false,
            refresh_rx: None,
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

    let cell = &terminal.backend().buffer()[(0, 0)];
    assert_ne!(cell.bg, Color::Black);
}

#[test]
fn prop_dialog_refresh_starts_background_worker() {
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
        prop_dialog: Some(PropDialog {
            device_did: "dev-1".to_string(),
            device_name: "dev-1".to_string(),
            account_uid: "1001".to_string(),
            items: vec![ToggleItem {
                prop: PropItem {
                    siid: 2,
                    piid: 1,
                    name: "Power".to_string(),
                    format: "bool".to_string(),
                    writable: true,
                    value_options: Vec::new(),
                },
                value: Value::Bool(true),
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

    app.request_prop_dialog_refresh();

    let dialog = app.prop_dialog.as_ref().unwrap();
    assert!(dialog.refreshing);
    assert!(dialog.refresh_rx.is_some());
}

#[test]
fn prop_dialog_r_key_starts_background_refresh() {
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
        prop_dialog: Some(PropDialog {
            device_did: "dev-1".to_string(),
            device_name: "dev-1".to_string(),
            account_uid: "1001".to_string(),
            items: vec![ToggleItem {
                prop: PropItem {
                    siid: 2,
                    piid: 1,
                    name: "Power".to_string(),
                    format: "bool".to_string(),
                    writable: true,
                    value_options: Vec::new(),
                },
                value: Value::Bool(true),
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

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
    )
    .unwrap();

    let dialog = app.prop_dialog.as_ref().unwrap();
    assert!(dialog.refreshing);
    assert!(dialog.refresh_rx.is_some());
}

#[test]
fn prop_dialog_footer_refresh_shows_props_loading_effect() {
    let mut app = test_app_with_prop_dialog(PropDialog {
        device_did: "dev-1".to_string(),
        device_name: "dev-1".to_string(),
        account_uid: "1001".to_string(),
        items: vec![ToggleItem {
            prop: PropItem {
                siid: 2,
                piid: 1,
                name: "Power".to_string(),
                format: "bool".to_string(),
                writable: true,
                value_options: Vec::new(),
            },
            value: Value::Bool(true),
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
    });
    app.language = Language::English;
    let terminal_area = ratatui::layout::Rect::new(0, 0, 100, 24);
    let (column, row) = footer_click_point(&app, terminal_area, "R: Refresh");

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
    assert!(dialog.refreshing);
    assert!(dialog.refresh_rx.is_some());

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("dev-1 (Loading...)"), "{text}");
    assert!(
        terminal_has_dim_substring(&terminal, "Power"),
        "props row should dim while props are refreshing:\n{text}"
    );
}

#[test]
fn prop_dialog_number_shortcuts_switch_tabs() {
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
        prop_dialog: Some(PropDialog {
            device_did: "dev-1".to_string(),
            device_name: "dev-1".to_string(),
            account_uid: "1001".to_string(),
            items: vec![
                ToggleItem {
                    prop: PropItem {
                        siid: 2,
                        piid: 1,
                        name: "Power".to_string(),
                        format: "bool".to_string(),
                        writable: true,
                        value_options: Vec::new(),
                    },
                    value: Value::Bool(true),
                },
                ToggleItem {
                    prop: PropItem {
                        siid: 2,
                        piid: 2,
                        name: "ReadOnlyVolume".to_string(),
                        format: "uint8".to_string(),
                        writable: false,
                        value_options: Vec::new(),
                    },
                    value: json!(22),
                },
                raw_device_logs_item(json!({"records": []})),
                raw_device_statistics_item(json!({"statistics": []})),
            ],
            selected: 0,
            active_tab: PropDialogTab::Writable,
            writable_selected: 0,
            readonly_selected: 1,
            actions: vec![ActionItem {
                siid: 2,
                aiid: 1,
                name: "toggle".to_string(),
                input_piids: Vec::new(),
                input_labels: Vec::new(),
                input_props: Vec::new(),
            }],
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

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
    )
    .unwrap();
    assert!(matches!(
        app.prop_dialog.as_ref().map(|dialog| dialog.active_tab),
        Some(PropDialogTab::Actions)
    ));

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE),
    )
    .unwrap();
    assert!(matches!(
        app.prop_dialog.as_ref().map(|dialog| dialog.active_tab),
        Some(PropDialogTab::Writable)
    ));

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE),
    )
    .unwrap();
    assert!(matches!(
        app.prop_dialog.as_ref().map(|dialog| dialog.active_tab),
        Some(PropDialogTab::ReadOnly)
    ));

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE),
    )
    .unwrap();
    assert!(matches!(
        app.prop_dialog.as_ref().map(|dialog| dialog.active_tab),
        Some(PropDialogTab::Logs)
    ));

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('5'), KeyModifiers::NONE),
    )
    .unwrap();
    assert!(matches!(
        app.prop_dialog.as_ref().map(|dialog| dialog.active_tab),
        Some(PropDialogTab::Statistics)
    ));
}

#[test]
fn process_prop_dialog_loading_handles_refresh_when_not_loading() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let (_refresh_tx, refresh_rx) = mpsc::channel::<super::PropDialogRefreshMessage>();
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
        prop_dialog: Some(PropDialog {
            device_did: "dev-1".to_string(),
            device_name: "dev-1".to_string(),
            account_uid: "1001".to_string(),
            items: vec![
                ToggleItem {
                    prop: PropItem {
                        siid: 2,
                        piid: 1,
                        name: "Power".to_string(),
                        format: "bool".to_string(),
                        writable: true,
                        value_options: Vec::new(),
                    },
                    value: Value::Bool(true),
                },
                raw_device_logs_item(json!({"records": []})),
            ],
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
            refreshing: true,
            refresh_rx: Some(refresh_rx),
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

    // Replace channel with one that already has a completed refresh payload.
    let (tx, rx) = mpsc::channel::<super::PropDialogRefreshMessage>();
    tx.send(super::PropDialogRefreshMessage::Props(vec![(
        0,
        Value::Bool(false),
    )]))
    .unwrap();
    tx.send(super::PropDialogRefreshMessage::Raw(vec![(
        1,
        json!({"records": [{"event": "updated"}]}),
    )]))
    .unwrap();
    tx.send(super::PropDialogRefreshMessage::Finished).unwrap();
    if let Some(dialog) = app.prop_dialog.as_mut() {
        dialog.refresh_rx = Some(rx);
    }

    app.process_prop_dialog_loading();

    let dialog = app.prop_dialog.as_ref().unwrap();
    assert!(!dialog.refreshing);
    assert!(dialog.refresh_rx.is_none());
    assert_eq!(dialog.items[0].value, Value::Bool(false));
    assert_eq!(dialog.items[1].value["records"][0]["event"], "updated");
}

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
fn process_initial_prop_dialog_loading_leaves_props_ready_while_raw_tabs_continue() {
    let mut app = app_with_single_readonly_prop_dialog();
    app.language = Language::English;
    let (loading_tx, loading_rx) = mpsc::channel::<std::result::Result<Vec<ToggleItem>, String>>();
    loading_tx
        .send(Ok(vec![
            ToggleItem {
                prop: PropItem {
                    siid: 2,
                    piid: 1,
                    name: "Power".to_string(),
                    format: "bool".to_string(),
                    writable: true,
                    value_options: Vec::new(),
                },
                value: Value::Bool(true),
            },
            raw_device_logs_item(json!({"status": "loading"})),
            raw_device_statistics_item(json!({"status": "loading"})),
        ]))
        .unwrap();
    let (_raw_tx, raw_rx) = mpsc::channel::<super::PropDialogRefreshMessage>();
    let dialog = app.prop_dialog.as_mut().unwrap();
    dialog.loading = true;
    dialog.loading_rx = Some(loading_rx);
    dialog.refreshing = true;
    dialog.refresh_rx = Some(raw_rx);
    dialog.active_tab = PropDialogTab::Writable;
    dialog.selected = 0;
    dialog.writable_selected = 0;

    app.process_prop_dialog_loading();

    let dialog = app.prop_dialog.as_mut().unwrap();
    assert!(!dialog.loading);
    assert!(!dialog.refreshing);
    assert!(dialog.refresh_rx.is_some());
    assert!(!super::prop_dialog_active_tab_is_loading(dialog));
    dialog.active_tab = PropDialogTab::Logs;
    assert!(super::prop_dialog_active_tab_is_loading(dialog));
    dialog.active_tab = PropDialogTab::Statistics;
    assert!(super::prop_dialog_active_tab_is_loading(dialog));
}

#[test]
fn process_prop_dialog_loading_preserves_selected_index_after_load() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let (tx, rx) = mpsc::channel::<std::result::Result<Vec<ToggleItem>, String>>();
    tx.send(Ok(vec![
        ToggleItem {
            prop: PropItem {
                siid: 2,
                piid: 1,
                name: "Power".to_string(),
                format: "bool".to_string(),
                writable: true,
                value_options: Vec::new(),
            },
            value: Value::Bool(true),
        },
        ToggleItem {
            prop: PropItem {
                siid: 2,
                piid: 2,
                name: "Switch".to_string(),
                format: "bool".to_string(),
                writable: true,
                value_options: Vec::new(),
            },
            value: Value::Bool(false),
        },
    ]))
    .unwrap();

    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
            items: vec![
                ToggleItem {
                    prop: PropItem {
                        siid: 2,
                        piid: 1,
                        name: "Power".to_string(),
                        format: "bool".to_string(),
                        writable: true,
                        value_options: Vec::new(),
                    },
                    value: Value::Null,
                },
                ToggleItem {
                    prop: PropItem {
                        siid: 2,
                        piid: 2,
                        name: "Switch".to_string(),
                        format: "bool".to_string(),
                        writable: true,
                        value_options: Vec::new(),
                    },
                    value: Value::Null,
                },
            ],
            selected: 1,
            active_tab: PropDialogTab::Writable,
            writable_selected: 1,
            readonly_selected: 0,
            actions: Vec::new(),
            actions_selected: 0,
            writable_list_state: ListState::default(),
            readonly_list_state: ListState::default(),
            actions_list_state: ListState::default(),
            loading: true,
            loading_rx: Some(rx),
            status: None,
            editing: false,
            edit_buffer: String::new(),
            edit_cursor: 0,
            edit_error: None,
            refreshing: false,
            refresh_rx: None,
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

    app.process_prop_dialog_loading();

    let dialog = app.prop_dialog.as_ref().unwrap();
    assert!(!dialog.loading);
    assert_eq!(dialog.selected, 1);
    assert_eq!(dialog.writable_selected, 1);
}

#[test]
fn draw_property_dialog_shows_writable_and_read_only_sections() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
            items: vec![
                ToggleItem {
                    prop: PropItem {
                        siid: 2,
                        piid: 1,
                        name: "扬声器服务 / 电源".to_string(),
                        format: "bool".to_string(),
                        writable: true,
                        value_options: Vec::new(),
                    },
                    value: Value::Bool(true),
                },
                ToggleItem {
                    prop: PropItem {
                        siid: 2,
                        piid: 2,
                        name: "扬声器服务 / 只读音量".to_string(),
                        format: "uint8".to_string(),
                        writable: false,
                        value_options: Vec::new(),
                    },
                    value: json!(20),
                },
            ],
            selected: 0,
            active_tab: PropDialogTab::Writable,
            writable_selected: 0,
            readonly_selected: 1,
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

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");
    assert!(text.contains("dev-1"), "{text}");
    assert!(!compact.contains("设备属性"), "{text}");
    assert!(text.contains("扬 声 器 服 务  / 电 源"), "{text}");
    assert!(!text.contains("只 读 音 量"), "{text}");
}

#[test]
fn prop_dialog_is_fullscreen_and_hides_schema_identifiers() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
            device_did: "客厅音箱".to_string(),
            device_name: "客厅音箱".to_string(),
            account_uid: "1001".to_string(),
            items: vec![ToggleItem {
                prop: PropItem {
                    siid: 2,
                    piid: 1,
                    name: "扬声器服务 / 电源".to_string(),
                    format: "bool".to_string(),
                    writable: true,
                    value_options: Vec::new(),
                },
                value: Value::Bool(true),
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
    let compact = text.replace(' ', "");
    let top_left = &terminal.backend().buffer()[(0, 0)];
    assert_ne!(top_left.symbol(), "┌");
    assert!(compact.contains("客厅音箱"), "{text}");
    assert!(!compact.contains("R刷新"), "{text}");
    assert!(!compact.contains("Esc关闭"), "{text}");
    assert!(compact.contains("修改参数"), "{text}");
    assert!(!compact.contains("快捷操作"), "{text}");
    assert!(!text.contains("Writable"), "{text}");
    assert!(!text.contains("Read-only"), "{text}");
    assert!(!text.contains("(2/1)"), "{text}");
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
fn prop_dialog_number_shortcuts_respect_hidden_actions_tab() {
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

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
    )
    .unwrap();
    assert!(matches!(
        app.prop_dialog.as_ref().map(|dialog| dialog.active_tab),
        Some(PropDialogTab::ReadOnly)
    ));

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE),
    )
    .unwrap();
    assert!(matches!(
        app.prop_dialog.as_ref().map(|dialog| dialog.active_tab),
        Some(PropDialogTab::ReadOnly)
    ));
}

#[test]
fn footer_text_shows_readonly_detail_shortcut() {
    let app = app_with_single_readonly_prop_dialog();
    assert_eq!(
        super::footer_text(&app),
        "R: 刷新, Esc: 返回, Enter: 查看属性, 当前设备: dev-1"
    );
}

#[test]
fn draw_readonly_prop_detail_shows_current_value_and_get_command() {
    let mut app = app_with_single_readonly_prop_dialog();
    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");

    assert!(compact.contains("当前值:22"), "{text}");
    assert!(
        compact.contains("CLI命令(读取):mitpropsgetdev-122"),
        "{text}"
    );
}

#[test]
fn handle_key_opens_and_closes_readonly_prop_detail() {
    let mut app = app_with_single_readonly_prop_dialog();

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(
        app.logs.iter().all(|line| !line.contains("当前属性为只读")),
        "{:?}",
        app.logs
    );

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(app.prop_dialog.is_some());
}

#[test]
fn prop_dialog_tab_switch_shows_active_subtab_only() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
            items: vec![
                ToggleItem {
                    prop: PropItem {
                        siid: 2,
                        piid: 1,
                        name: "writable-power".to_string(),
                        format: "bool".to_string(),
                        writable: true,
                        value_options: Vec::new(),
                    },
                    value: Value::Bool(true),
                },
                ToggleItem {
                    prop: PropItem {
                        siid: 2,
                        piid: 2,
                        name: "readonly-volume".to_string(),
                        format: "uint8".to_string(),
                        writable: false,
                        value_options: Vec::new(),
                    },
                    value: json!(30),
                },
            ],
            selected: 0,
            active_tab: PropDialogTab::Writable,
            writable_selected: 0,
            readonly_selected: 1,
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
    let before = terminal_text(&terminal);
    assert!(before.contains("writable-power"), "{before}");

    let quit = handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(!quit);
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let after = terminal_text(&terminal);
    assert!(!after.contains("writable-power"), "{after}");
    assert!(after.contains("readonly-volume"), "{after}");
}

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
    assert!(
        stats_text.contains("1970-01-01 - 1970-01-08"),
        "{stats_text}"
    );
    assert!(compact.contains("值↑时间→"), "{stats_text}");
    assert!(stats_text.contains("01-01"), "{stats_text}");
    assert!(stats_text.contains("2.5"), "{stats_text}");
    let chart_title_position =
        terminal_find_substring_position(&terminal, "值").expect("chart title rendered");
    let first_value_position =
        terminal_find_substring_position(&terminal, "1.25").expect("bar value label rendered");
    let first_bar_position =
        terminal_find_substring_position(&terminal, "█").expect("bar rendered");
    let second_value_position =
        terminal_find_substring_position(&terminal, "2.5").expect("second bar value rendered");
    let second_time_position =
        terminal_find_substring_position(&terminal, "01-02").expect("second time label rendered");
    let first_time_position = terminal_find_substring_position_in_area(
        &terminal,
        "01-01",
        ratatui::layout::Rect::new(0, 6, 120, 16),
    )
    .expect("first chart time label rendered");
    assert!(
        first_bar_position.0 >= chart_title_position.0.saturating_add(3),
        "{stats_text}"
    );
    assert!(
        first_time_position.0 > chart_title_position.0.saturating_add(15),
        "{stats_text}"
    );
    let buffer = terminal.backend().buffer();
    assert_eq!(
        buffer[(
            first_value_position.0,
            first_value_position.1.saturating_add(1)
        )]
            .symbol(),
        "█",
        "{stats_text}"
    );
    assert_eq!(
        second_value_position
            .0
            .saturating_mul(2)
            .saturating_add(super::display_width("2.5")),
        second_time_position
            .0
            .saturating_mul(2)
            .saturating_add(super::display_width("01-02")),
        "{stats_text}"
    );
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
    let title_position =
        terminal_find_substring_position_in_area(&terminal, "值", chart_area).unwrap();
    let max_label_position =
        terminal_find_substring_position_in_area(&terminal, "10", chart_area).unwrap();

    assert!(
        max_label_position.1 > title_position.1.saturating_add(2),
        "{stats_text}"
    );
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
        .saturating_add((super::display_width("01-02") / 2) as u16);
    let buffer = terminal.backend().buffer();

    assert_eq!(
        buffer[(zero_marker_column, zero_label_position.1.saturating_sub(1))].symbol(),
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
fn prop_dialog_statistics_month_labels_stride_when_dense() {
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
    let visible_labels = [
        "01-01", "01-04", "01-07", "01-10", "01-13", "01-16", "01-19", "01-22", "01-25", "01-28",
        "01-31",
    ];
    let positions = visible_labels
        .iter()
        .map(|label| {
            terminal_find_substring_position_in_area(&terminal, label, chart_area)
                .unwrap_or_else(|| panic!("{label} missing\n{stats_text}"))
        })
        .collect::<Vec<_>>();

    assert!(
        terminal_find_substring_position_in_area(&terminal, "01-02", chart_area).is_none(),
        "{stats_text}"
    );
    assert!(
        terminal_find_substring_position_in_area(&terminal, "01-03", chart_area).is_none(),
        "{stats_text}"
    );
    for pair in positions.windows(2) {
        assert_eq!(pair[0].1, pair[1].1, "{stats_text}");
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
    assert!(compact.contains("此设备不支持查看统计数据"), "{stats_text}");
    assert!(!compact.contains("值↑时间→"), "{stats_text}");
    assert!(!stats_text.contains('█'), "{stats_text}");
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

#[test]
fn readonly_tab_omits_type_marker_and_sorts_short_to_long() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
            items: vec![
                ToggleItem {
                    prop: PropItem {
                        siid: 2,
                        piid: 1,
                        name: "zz-long".to_string(),
                        format: "bool".to_string(),
                        writable: false,
                        value_options: Vec::new(),
                    },
                    value: Value::Bool(true),
                },
                ToggleItem {
                    prop: PropItem {
                        siid: 2,
                        piid: 2,
                        name: "mid".to_string(),
                        format: "uint8".to_string(),
                        writable: false,
                        value_options: Vec::new(),
                    },
                    value: json!(30),
                },
                ToggleItem {
                    prop: PropItem {
                        siid: 2,
                        piid: 3,
                        name: "s".to_string(),
                        format: "uint8".to_string(),
                        writable: false,
                        value_options: Vec::new(),
                    },
                    value: json!(1),
                },
            ],
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
    assert!(!text.contains("uint8|ro"), "{text}");
    assert!(!text.contains("bool|ro"), "{text}");
    // Check that readonly properties are rendered without [ro] marker
    assert!(text.contains("s ="), "{text}"); // shortest name
    assert!(text.contains("mid ="), "{text}"); // medium name
    assert!(text.contains("zz-long ="), "{text}"); // longest name
    let compact = text.replace(' ', "");
    let p_s = compact.find("s=").unwrap_or(usize::MAX);
    let p_mid = compact.find("mid=").unwrap_or(usize::MAX);
    let p_long = compact.find("zz-long=").unwrap_or(usize::MAX);
    assert!(p_s < p_mid && p_mid < p_long, "{text}");
}

#[test]
fn dialog_subtab_click_bounds_handle_wide_char_titles() {
    let area = ratatui::layout::Rect::new(1, 1, 40, 3);
    let titles = super::all_prop_dialog_tab_titles(Language::Chinese);
    // "操作" uses wide glyphs and still occupies this column in the first tab.
    assert_eq!(tab_index_for_column_with_titles(7, area, &titles), Some(0));
}

#[test]
fn prop_dialog_visible_tabs_hide_empty_categories_and_keep_order() {
    let dialog = PropDialog {
        device_did: "dev-1".to_string(),
        device_name: "dev-1".to_string(),
        account_uid: "1001".to_string(),
        items: vec![ToggleItem {
            prop: PropItem {
                siid: 2,
                piid: 1,
                name: "p1".to_string(),
                format: "bool".to_string(),
                writable: true,
                value_options: Vec::new(),
            },
            value: Value::Bool(true),
        }],
        selected: 0,
        active_tab: PropDialogTab::Writable,
        writable_selected: 0,
        readonly_selected: 0,
        actions: vec![ActionItem {
            siid: 2,
            aiid: 1,
            name: "toggle".to_string(),
            input_piids: Vec::new(),
            input_labels: Vec::new(),
            input_props: Vec::new(),
        }],
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
    };

    assert_eq!(
        super::visible_prop_dialog_tabs(&dialog),
        vec![PropDialogTab::Actions, PropDialogTab::Writable]
    );
}

#[test]
fn prop_dialog_visible_tab_titles_are_renumbered_one_based() {
    let dialog = PropDialog {
        device_did: "dev-1".to_string(),
        device_name: "dev-1".to_string(),
        account_uid: "1001".to_string(),
        items: vec![
            ToggleItem {
                prop: PropItem {
                    siid: 2,
                    piid: 1,
                    name: "p1".to_string(),
                    format: "bool".to_string(),
                    writable: true,
                    value_options: Vec::new(),
                },
                value: Value::Bool(true),
            },
            ToggleItem {
                prop: PropItem {
                    siid: 2,
                    piid: 2,
                    name: "p2".to_string(),
                    format: "bool".to_string(),
                    writable: false,
                    value_options: Vec::new(),
                },
                value: Value::Bool(false),
            },
        ],
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
    };

    assert_eq!(
        super::visible_prop_dialog_tab_titles(&dialog, Language::Chinese),
        vec!["1:修改参数".to_string(), "2:只读属性".to_string()]
    );
}

#[test]
fn prop_dialog_visible_tab_titles_empty_when_no_actions_or_properties() {
    let dialog = PropDialog {
        device_did: "dev-1".to_string(),
        device_name: "dev-1".to_string(),
        account_uid: "1001".to_string(),
        items: Vec::new(),
        selected: 0,
        active_tab: PropDialogTab::Actions,
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
    };

    assert_eq!(
        super::visible_prop_dialog_tab_titles(&dialog, Language::Chinese),
        Vec::<String>::new()
    );
}

#[test]
fn prop_dialog_visible_tab_titles_actions_only_renumber_from_one() {
    let dialog = PropDialog {
        device_did: "dev-1".to_string(),
        device_name: "dev-1".to_string(),
        account_uid: "1001".to_string(),
        items: Vec::new(),
        selected: 0,
        active_tab: PropDialogTab::Actions,
        writable_selected: 0,
        readonly_selected: 0,
        actions: vec![ActionItem {
            siid: 2,
            aiid: 1,
            name: "toggle".to_string(),
            input_piids: Vec::new(),
            input_labels: Vec::new(),
            input_props: Vec::new(),
        }],
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
    };

    assert_eq!(
        super::visible_prop_dialog_tab_titles(&dialog, Language::Chinese),
        vec!["1:快捷操作".to_string()]
    );
}

#[test]
fn prop_dialog_visible_tab_titles_readonly_only_renumber_from_one() {
    let dialog = PropDialog {
        device_did: "dev-1".to_string(),
        device_name: "dev-1".to_string(),
        account_uid: "1001".to_string(),
        items: vec![
            ToggleItem {
                prop: PropItem {
                    siid: 2,
                    piid: 2,
                    name: "p2".to_string(),
                    format: "bool".to_string(),
                    writable: false,
                    value_options: Vec::new(),
                },
                value: Value::Bool(false),
            },
            raw_device_logs_item(json!({"result": []})),
            raw_device_statistics_item(json!({"result": []})),
        ],
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
    };

    assert_eq!(
        super::visible_prop_dialog_tab_titles(&dialog, Language::Chinese),
        vec![
            "1:只读属性".to_string(),
            "2:操作记录".to_string(),
            "3:统计".to_string(),
        ]
    );
}

#[test]
fn prop_dialog_mouse_tab_hit_testing_uses_visible_tabs_when_first_hidden() {
    let mut app = test_app_with_prop_dialog(PropDialog {
        device_did: "dev-1".to_string(),
        device_name: "dev-1".to_string(),
        account_uid: "1001".to_string(),
        items: vec![
            ToggleItem {
                prop: PropItem {
                    siid: 2,
                    piid: 1,
                    name: "writable-only-item".to_string(),
                    format: "bool".to_string(),
                    writable: true,
                    value_options: Vec::new(),
                },
                value: Value::Bool(true),
            },
            ToggleItem {
                prop: PropItem {
                    siid: 2,
                    piid: 2,
                    name: "readonly-only-item".to_string(),
                    format: "uint8".to_string(),
                    writable: false,
                    value_options: Vec::new(),
                },
                value: json!(7),
            },
        ],
        selected: 1,
        active_tab: PropDialogTab::ReadOnly,
        writable_selected: 0,
        readonly_selected: 1,
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
    });

    let terminal_area = ratatui::layout::Rect::new(0, 0, 80, 24);
    let tabs_area = prop_dialog_tabs_area(terminal_area);
    let visible_titles =
        super::visible_prop_dialog_tab_titles(app.prop_dialog.as_ref().unwrap(), Language::Chinese);
    let first_visible_tab_column = tab_column_for_index(tabs_area, &visible_titles, 0);

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: first_visible_tab_column,
            row: tabs_area.y,
            modifiers: KeyModifiers::NONE,
        },
        terminal_area,
    )
    .unwrap();

    assert_eq!(
        app.prop_dialog.as_ref().map(|dialog| dialog.active_tab),
        Some(PropDialogTab::Writable)
    );

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("writable-only-item"), "{text}");
    assert!(!text.contains("readonly-only-item"), "{text}");
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

#[test]
fn stale_keypress_uses_recent_pingresp_as_mqtt_response_timer() {
    let _guard = env_guard();
    std::env::remove_var("MIT_DISABLE_CLOUD_MIPS");
    std::env::set_var("MIT_ENABLE_CLOUD_MIPS_IN_TESTS", "1");
    let mut app = test_app_with_prop_dialog(PropDialog {
        device_did: "dev-1".to_string(),
        device_name: "speaker".to_string(),
        account_uid: "1001".to_string(),
        items: vec![ToggleItem {
            prop: PropItem {
                siid: 2,
                piid: 1,
                name: "Power".to_string(),
                format: "bool".to_string(),
                writable: true,
                value_options: Vec::new(),
            },
            value: Value::Bool(true),
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
        editing: true,
        edit_buffer: "true".to_string(),
        edit_cursor: 4,
        edit_error: None,
        refreshing: false,
        refresh_rx: None,
    });
    let (_tx, rx) = mpsc::channel();
    let now = Instant::now();
    {
        let mut runtime = super::cloud_mips_runtime().lock().unwrap();
        *runtime = Some(super::CloudMipsRuntime {
            key: "test-runtime".to_string(),
            _handles: Vec::new(),
            rx,
            last_mqtt_response_at: Some(now - Duration::from_secs(8)),
            last_ping_req_at: Some(now - Duration::from_secs(9)),
            last_ping_resp_at: Some(now - Duration::from_secs(8)),
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
    assert!(!logs.contains("heartbeat queued"));
    assert!(!logs.contains("subscribe probe"));
    assert!(!app.prop_dialog.as_ref().unwrap().refreshing);

    std::env::remove_var("MIT_ENABLE_CLOUD_MIPS_IN_TESTS");
    let mut runtime = super::cloud_mips_runtime().lock().unwrap();
    *runtime = None;
}

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
            last_mqtt_response_at: Some(now - Duration::from_secs(301)),
            last_ping_req_at: Some(now - Duration::from_secs(301)),
            last_ping_resp_at: Some(now - Duration::from_secs(301)),
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
fn stale_keypress_refreshes_open_prop_editor() {
    let _guard = env_guard();
    std::env::remove_var("MIT_DISABLE_CLOUD_MIPS");
    std::env::set_var("MIT_ENABLE_CLOUD_MIPS_IN_TESTS", "1");
    let mut app = test_app_with_prop_dialog(PropDialog {
        device_did: "dev-1".to_string(),
        device_name: "speaker".to_string(),
        account_uid: "1001".to_string(),
        items: vec![ToggleItem {
            prop: PropItem {
                siid: 2,
                piid: 1,
                name: "Power".to_string(),
                format: "bool".to_string(),
                writable: true,
                value_options: Vec::new(),
            },
            value: Value::Bool(true),
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
        editing: true,
        edit_buffer: "true".to_string(),
        edit_cursor: 4,
        edit_error: None,
        refreshing: false,
        refresh_rx: None,
    });
    let (_tx, rx) = mpsc::channel();
    let now = Instant::now();
    {
        let mut runtime = super::cloud_mips_runtime().lock().unwrap();
        *runtime = Some(super::CloudMipsRuntime {
            key: "test-runtime".to_string(),
            _handles: Vec::new(),
            rx,
            last_mqtt_response_at: Some(now - Duration::from_secs(301)),
            last_ping_req_at: Some(now - Duration::from_secs(301)),
            last_ping_resp_at: Some(now - Duration::from_secs(301)),
        });
    }

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Home, KeyModifiers::NONE),
    )
    .unwrap();

    let dialog = app.prop_dialog.as_ref().unwrap();
    assert!(dialog.editing);
    assert!(dialog.refreshing);
    assert!(dialog.refresh_rx.is_some());
    assert_eq!(dialog.edit_buffer, "true");
    let logs = app.logs.iter().cloned().collect::<Vec<_>>().join("\n");
    assert!(logs.contains("cloud MIPS response stale"));

    std::env::remove_var("MIT_ENABLE_CLOUD_MIPS_IN_TESTS");
    let mut runtime = super::cloud_mips_runtime().lock().unwrap();
    *runtime = None;
}

#[test]
fn stale_mouse_click_inside_prop_dialog_does_not_refresh() {
    let _guard = env_guard();
    std::env::remove_var("MIT_DISABLE_CLOUD_MIPS");
    std::env::set_var("MIT_ENABLE_CLOUD_MIPS_IN_TESTS", "1");
    let mut app = test_app_with_prop_dialog(PropDialog {
        device_did: "dev-1".to_string(),
        device_name: "speaker".to_string(),
        account_uid: "1001".to_string(),
        items: vec![ToggleItem {
            prop: PropItem {
                siid: 2,
                piid: 1,
                name: "Power".to_string(),
                format: "bool".to_string(),
                writable: true,
                value_options: Vec::new(),
            },
            value: Value::Bool(true),
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
    });
    let (_tx, rx) = mpsc::channel();
    let now = Instant::now();
    {
        let mut runtime = super::cloud_mips_runtime().lock().unwrap();
        *runtime = Some(super::CloudMipsRuntime {
            key: "test-runtime".to_string(),
            _handles: Vec::new(),
            rx,
            last_mqtt_response_at: Some(now - Duration::from_secs(301)),
            last_ping_req_at: Some(now - Duration::from_secs(301)),
            last_ping_resp_at: Some(now - Duration::from_secs(301)),
        });
    }

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 2,
            row: 10,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();

    let dialog = app.prop_dialog.as_ref().unwrap();
    assert!(!dialog.refreshing);
    assert!(dialog.refresh_rx.is_none());
    let logs = app.logs.iter().cloned().collect::<Vec<_>>().join("\n");
    assert!(!logs.contains("cloud MIPS response stale"));

    std::env::remove_var("MIT_ENABLE_CLOUD_MIPS_IN_TESTS");
    let mut runtime = super::cloud_mips_runtime().lock().unwrap();
    *runtime = None;
}

#[test]
fn prop_dialog_keyboard_tab_cycles_over_visible_tabs_when_middle_hidden() {
    let mut app = test_app_with_prop_dialog(PropDialog {
        device_did: "dev-1".to_string(),
        device_name: "dev-1".to_string(),
        account_uid: "1001".to_string(),
        items: vec![ToggleItem {
            prop: PropItem {
                siid: 2,
                piid: 2,
                name: "readonly-only-item".to_string(),
                format: "uint8".to_string(),
                writable: false,
                value_options: Vec::new(),
            },
            value: json!(9),
        }],
        selected: 0,
        active_tab: PropDialogTab::Actions,
        writable_selected: 0,
        readonly_selected: 0,
        actions: vec![ActionItem {
            siid: 2,
            aiid: 1,
            name: "action-only-item".to_string(),
            input_piids: Vec::new(),
            input_labels: Vec::new(),
            input_props: Vec::new(),
        }],
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
    });

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
    )
    .unwrap();
    assert_eq!(
        app.prop_dialog.as_ref().map(|dialog| dialog.active_tab),
        Some(PropDialogTab::ReadOnly)
    );

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("readonly-only-item"), "{text}");
    assert!(!text.contains("action-only-item"), "{text}");
}

#[test]
fn prop_dialog_hidden_active_tab_is_normalized_before_render() {
    let mut app = test_app_with_prop_dialog(PropDialog {
        device_did: "dev-1".to_string(),
        device_name: "dev-1".to_string(),
        account_uid: "1001".to_string(),
        items: vec![ToggleItem {
            prop: PropItem {
                siid: 2,
                piid: 2,
                name: "readonly-only-item".to_string(),
                format: "uint8".to_string(),
                writable: false,
                value_options: Vec::new(),
            },
            value: json!(11),
        }],
        selected: 0,
        active_tab: PropDialogTab::Writable,
        writable_selected: 0,
        readonly_selected: 0,
        actions: vec![ActionItem {
            siid: 2,
            aiid: 1,
            name: "action-only-item".to_string(),
            input_piids: Vec::new(),
            input_labels: Vec::new(),
            input_props: Vec::new(),
        }],
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
    });

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    assert_eq!(
        app.prop_dialog.as_ref().map(|dialog| dialog.active_tab),
        Some(PropDialogTab::Actions)
    );

    let text = terminal_text(&terminal);
    assert!(text.contains("action-only-item"), "{text}");
    assert!(!text.contains("readonly-only-item"), "{text}");
}

#[test]
fn mouse_scroll_moves_selection_inside_prop_dialog() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
            items: vec![
                ToggleItem {
                    prop: PropItem {
                        siid: 2,
                        piid: 1,
                        name: "p1".to_string(),
                        format: "bool".to_string(),
                        writable: true,
                        value_options: Vec::new(),
                    },
                    value: Value::Bool(true),
                },
                ToggleItem {
                    prop: PropItem {
                        siid: 2,
                        piid: 2,
                        name: "p2".to_string(),
                        format: "uint8".to_string(),
                        writable: true,
                        value_options: Vec::new(),
                    },
                    value: json!(20),
                },
            ],
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
            kind: crossterm::event::MouseEventKind::ScrollDown,
            column: 2,
            row: 10,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();

    assert_eq!(app.prop_dialog.as_ref().unwrap().selected, 1);

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollUp,
            column: 2,
            row: 10,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();
    assert_eq!(app.prop_dialog.as_ref().unwrap().selected, 0);

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
    )
    .unwrap();
    assert_eq!(app.prop_dialog.as_ref().unwrap().selected, 1);

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
    )
    .unwrap();
    assert_eq!(app.prop_dialog.as_ref().unwrap().selected, 0);
}

#[test]
fn clicking_active_prop_dialog_item_executes_it() {
    let _guard = env_guard();
    let server = MockMicoServer::start();
    let home = make_temp_dir("tui-bool-dialog-click-active");
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
    std::env::set_var("MIT_HOME", &home);
    std::env::set_var("MIT_PROFILE_DIR", &home);
    std::env::set_var("MIT_MICO_BASE_URL", server.base_url());
    std::env::set_var("MIT_USER_PROFILE_URL", server.user_profile_url());

    let mut app = TuiApp {
        home_dir: home.clone(),
        auth_state: AuthState {
            accounts: vec![account.clone()],
            pending_auth: None,
        },
        accounts: vec![account],
        account_index: 0,
        devices: vec![Device {
            did: "dev-1".to_string(),
            name: "speaker".to_string(),
            model: "xiaomi.wifispeaker.lx04".to_string(),
            online: true,

            home_id: "home-1".to_string(),
            home_name: "我家".to_string(),
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
        prop_dialog: Some(PropDialog {
            device_did: "dev-1".to_string(),
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: vec![ToggleItem {
                prop: PropItem {
                    siid: 2,
                    piid: 1,
                    name: "Power".to_string(),
                    format: "bool".to_string(),
                    writable: true,
                    value_options: Vec::new(),
                },
                value: Value::Bool(true),
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
            row: 4,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();
    assert_eq!(app.prop_dialog.as_ref().unwrap().selected, 0);
    assert!(app
        .prop_dialog
        .as_ref()
        .is_some_and(|dialog| dialog.editing));
    assert!(app
        .prop_dialog
        .as_ref()
        .is_some_and(|dialog| dialog.editing));
    assert_eq!(
        app.prop_dialog.as_ref().unwrap().items[0].value,
        Value::Bool(true)
    );

    std::env::remove_var("MIT_HOME");
    std::env::remove_var("MIT_PROFILE_DIR");
    std::env::remove_var("MIT_MICO_BASE_URL");
    std::env::remove_var("MIT_USER_PROFILE_URL");
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn prop_dialog_actions_tab_renders_action_items() {
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
        prop_dialog: Some(PropDialog {
            device_did: "dev-1".to_string(),
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: vec![ToggleItem {
                prop: PropItem {
                    siid: 2,
                    piid: 1,
                    name: "电源".to_string(),
                    format: "bool".to_string(),
                    writable: true,
                    value_options: Vec::new(),
                },
                value: Value::Bool(true),
            }],
            selected: 0,
            active_tab: PropDialogTab::Actions,
            writable_selected: 0,
            readonly_selected: 0,
            actions: vec![ActionItem {
                siid: 5,
                aiid: 1,
                name: "唤醒".to_string(),
                input_piids: Vec::new(),
                input_labels: Vec::new(),
                input_props: Vec::new(),
            }],
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

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");
    assert!(compact.contains("唤醒"), "{text}");
}

#[test]
fn action_param_labels_use_same_service_property_translations() {
    let spec = json!({
        "services": [{
            "iid": 5,
            "properties": [
                {"iid": 1, "description_trans": "音量", "description": "Volume"},
                {"iid": 2, "description": "Mode"}
            ],
            "actions": [{
                "iid": 9,
                "description_trans": "设置",
                "in": [1, 2, 99]
            }]
        }]
    });
    let actions = extract_actions_from_spec(&spec, Language::Chinese);
    assert_eq!(actions.len(), 1);
    assert_eq!(
        actions[0].input_labels,
        vec!["音量".to_string(), "Mode".to_string(), "参数3".to_string()]
    );
}

#[test]
fn action_param_edit_supports_tab_and_click_focus_switch() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: Vec::new(),
            selected: 0,
            active_tab: PropDialogTab::Actions,
            writable_selected: 0,
            readonly_selected: 0,
            actions: vec![ActionItem {
                siid: 5,
                aiid: 1,
                name: "设置".to_string(),
                input_piids: vec![1, 2],
                input_labels: vec!["音量".to_string(), "模式".to_string()],
                input_props: Vec::new(),
            }],
            actions_selected: 0,
            writable_list_state: ListState::default(),
            readonly_list_state: ListState::default(),
            actions_list_state: ListState::default(),
            loading: false,
            loading_rx: None,
            status: None,
            editing: true,
            edit_buffer: "\n".to_string(),
            edit_cursor: 0,
            edit_error: None,
            refreshing: false,
            refresh_rx: None,
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

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
    )
    .unwrap();
    assert_eq!(app.prop_dialog.as_ref().unwrap().writable_selected, 1);

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
    )
    .unwrap();
    assert_eq!(app.prop_dialog.as_ref().unwrap().writable_selected, 0);

    let editor_area = super::prop_editor_layout(
        app.prop_dialog.as_ref().unwrap(),
        ratatui::layout::Rect::new(1, 1, 78, 22),
        Language::Chinese,
    )
    .editor_area;
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 4,
            row: editor_area.y.saturating_add(1),
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();
    assert_eq!(app.prop_dialog.as_ref().unwrap().writable_selected, 1);
}

#[test]
fn action_param_textarea_row_focus_updates_cursor_and_input() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: Vec::new(),
            selected: 0,
            active_tab: PropDialogTab::Actions,
            writable_selected: 0,
            readonly_selected: 0,
            actions: vec![ActionItem {
                siid: 5,
                aiid: 1,
                name: "设置".to_string(),
                input_piids: vec![1, 2],
                input_labels: vec!["音量".to_string(), "模式".to_string()],
                input_props: Vec::new(),
            }],
            actions_selected: 0,
            writable_list_state: ListState::default(),
            readonly_list_state: ListState::default(),
            actions_list_state: ListState::default(),
            loading: false,
            loading_rx: None,
            status: None,
            editing: true,
            edit_buffer: "1\n2".to_string(),
            edit_cursor: 1,
            edit_error: None,
            refreshing: false,
            refresh_rx: None,
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
    let row_before = terminal_first_reversed_cell_row(&terminal).unwrap();

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
    )
    .unwrap();
    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE),
    )
    .unwrap();

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let row_after = terminal_first_reversed_cell_row(&terminal).unwrap();
    assert!(
        row_after > row_before,
        "row_before={row_before}, row_after={row_after}"
    );
    assert_eq!(app.prop_dialog.as_ref().unwrap().edit_buffer, "1\n23");
}

#[test]
fn clicking_action_param_textarea_moves_cursor_to_clicked_character() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: Vec::new(),
            selected: 0,
            active_tab: PropDialogTab::Actions,
            writable_selected: 0,
            readonly_selected: 0,
            actions: vec![ActionItem {
                siid: 5,
                aiid: 1,
                name: "执行文本指令".to_string(),
                input_piids: vec![1],
                input_labels: vec!["参数1".to_string()],
                input_props: vec![PropItem {
                    siid: 5,
                    piid: 1,
                    name: "参数1".to_string(),
                    format: "string".to_string(),
                    writable: false,
                    value_options: Vec::new(),
                }],
            }],
            actions_selected: 0,
            writable_list_state: ListState::default(),
            readonly_list_state: ListState::default(),
            actions_list_state: ListState::default(),
            loading: false,
            loading_rx: None,
            status: None,
            editing: true,
            edit_buffer: "abcdef".to_string(),
            edit_cursor: 6,
            edit_error: None,
            refreshing: false,
            refresh_rx: None,
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
    let (column, row) = terminal_find_substring_position(&terminal, "abcdef").unwrap();

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: column + 2,
            row,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();

    assert_eq!(app.prop_dialog.as_ref().unwrap().edit_cursor, 2);
}

#[test]
fn draw_edit_mode_shows_visible_input_cursor() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
            device_name: "living-room".to_string(),
            account_uid: "1001".to_string(),
            items: vec![ToggleItem {
                prop: PropItem {
                    siid: 2,
                    piid: 1,
                    name: "power".to_string(),
                    format: "string".to_string(),
                    writable: true,
                    value_options: Vec::new(),
                },
                value: Value::String("true".to_string()),
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
            editing: true,
            edit_buffer: "true".to_string(),
            edit_cursor: 4,
            edit_error: None,
            refreshing: false,
            refresh_rx: None,
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
    assert!(text.contains("true"), "{text}");
    assert!(!text.contains("true|"), "{text}");
    assert!(terminal_has_reversed_cell(&terminal));
}

#[test]
fn clicking_prop_edit_textarea_moves_cursor_to_clicked_character() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
            device_name: "living-room".to_string(),
            account_uid: "1001".to_string(),
            items: vec![ToggleItem {
                prop: PropItem {
                    siid: 2,
                    piid: 1,
                    name: "power".to_string(),
                    format: "string".to_string(),
                    writable: true,
                    value_options: Vec::new(),
                },
                value: Value::String("on".to_string()),
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
            editing: true,
            edit_buffer: "abcdef".to_string(),
            edit_cursor: 6,
            edit_error: None,
            refreshing: false,
            refresh_rx: None,
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
    let (column, row) = terminal_find_substring_position(&terminal, "abcdef").unwrap();

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: column + 2,
            row,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();

    assert_eq!(app.prop_dialog.as_ref().unwrap().edit_cursor, 2);
}

#[test]
fn action_param_edit_mode_shows_action_title_not_property_title() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: vec![ToggleItem {
                prop: PropItem {
                    siid: 2,
                    piid: 9,
                    name: "只读属性".to_string(),
                    format: "string".to_string(),
                    writable: false,
                    value_options: Vec::new(),
                },
                value: Value::String("x".to_string()),
            }],
            selected: 0,
            active_tab: PropDialogTab::Actions,
            writable_selected: 0,
            readonly_selected: 0,
            actions: vec![ActionItem {
                siid: 5,
                aiid: 1,
                name: "唤醒".to_string(),
                input_piids: vec![1],
                input_labels: vec!["参数1".to_string()],
                input_props: Vec::new(),
            }],
            actions_selected: 0,
            writable_list_state: ListState::default(),
            readonly_list_state: ListState::default(),
            actions_list_state: ListState::default(),
            loading: false,
            loading_rx: None,
            status: None,
            editing: true,
            edit_buffer: "1".to_string(),
            edit_cursor: 1,
            edit_error: None,
            refreshing: false,
            refresh_rx: None,
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
    let compact = text.replace(' ', "");
    assert!(text.contains("speaker"), "{text}");
    assert!(!text.contains("设备属性"), "{text}");
    assert!(compact.contains("唤醒"), "{text}");
    assert!(!compact.contains("Editing:唤醒"), "{text}");
    assert!(!compact.contains("[action]"), "{text}");
    assert!(!compact.contains("参数数量"), "{text}");
    assert!(
        !compact.contains("Input(Tab切换参数,点击行切换焦点):"),
        "{text}"
    );
    assert!(!compact.contains("输入参数(Tab切换参数焦点):"), "{text}");
    assert!(compact.contains("CLI命令(执行):"), "{text}");
    assert!(compact.contains("mitpropsactdev-1511"), "{text}");
}

#[test]
fn action_bool_param_uses_selector_editor() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: vec![ToggleItem {
                prop: PropItem {
                    siid: 5,
                    piid: 2,
                    name: "指令静默执行".to_string(),
                    format: "bool".to_string(),
                    writable: false,
                    value_options: Vec::new(),
                },
                value: Value::Bool(false),
            }],
            selected: 0,
            active_tab: PropDialogTab::Actions,
            writable_selected: 0,
            readonly_selected: 0,
            actions: vec![ActionItem {
                siid: 5,
                aiid: 1,
                name: "执行指令".to_string(),
                input_piids: vec![2],
                input_labels: vec!["指令静默执行".to_string()],
                input_props: Vec::new(),
            }],
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

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(app
        .prop_dialog
        .as_ref()
        .is_some_and(|dialog| dialog.editing));

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");
    assert!(compact.contains("[true]"), "{text}");
    assert!(compact.contains("[false]"), "{text}");
}

#[test]
fn action_editor_layout_is_compact_without_duplicate_help_lines() {
    let dialog = PropDialog {
        device_did: "718342728.s16".to_string(),
        device_name: "右键-客厅".to_string(),
        account_uid: "1001".to_string(),
        items: vec![ToggleItem {
            prop: PropItem {
                siid: 5,
                piid: 2,
                name: "指令静默执行".to_string(),
                format: "bool".to_string(),
                writable: false,
                value_options: Vec::new(),
            },
            value: Value::Bool(true),
        }],
        selected: 0,
        active_tab: PropDialogTab::Actions,
        writable_selected: 0,
        readonly_selected: 0,
        actions: vec![ActionItem {
            siid: 5,
            aiid: 4,
            name: "执行文本指令".to_string(),
            input_piids: vec![2],
            input_labels: vec!["指令静默执行".to_string()],
            input_props: vec![PropItem {
                siid: 5,
                piid: 2,
                name: "指令静默执行".to_string(),
                format: "bool".to_string(),
                writable: false,
                value_options: Vec::new(),
            }],
        }],
        actions_selected: 0,
        writable_list_state: ListState::default(),
        readonly_list_state: ListState::default(),
        actions_list_state: ListState::default(),
        loading: false,
        loading_rx: None,
        status: None,
        editing: true,
        edit_buffer: "true".to_string(),
        edit_cursor: 4,
        edit_error: None,
        refreshing: false,
        refresh_rx: None,
    };
    let mut app = test_app_with_prop_dialog(dialog);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    let text = terminal_text(&terminal);
    let compact_lines = text
        .lines()
        .map(|line| line.replace(' ', ""))
        .collect::<Vec<_>>();
    let title_row = compact_lines
        .iter()
        .position(|line| line.contains("右键-客厅"))
        .unwrap();
    let action_row = compact_lines
        .iter()
        .position(|line| line.contains("执行文本指令"))
        .unwrap();
    let param_row = compact_lines
        .iter()
        .position(|line| line.contains("指令静默执行:[true],[false]"))
        .unwrap();
    let command_row = compact_lines
        .iter()
        .position(|line| line.contains("CLI命令(执行):mitpropsact718342728.s1654true"))
        .unwrap();
    let help_rows = compact_lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.contains("Esc:返回"))
        .map(|(idx, _)| idx)
        .collect::<Vec<_>>();

    assert!(!compact_lines[title_row].contains("R刷新"), "{text}");
    assert!(!compact_lines[title_row].contains("Esc关闭"), "{text}");
    assert_eq!(help_rows, vec![22], "{text}");
    assert!(
        action_row > 0 && compact_lines[action_row - 1].is_empty(),
        "{text}"
    );
    assert!(action_row <= title_row + 2, "{text}");
    assert!(param_row <= action_row + 2, "{text}");
    assert!(command_row <= param_row + 2, "{text}");
}

#[test]
fn action_without_params_keeps_command_compact() {
    let dialog = PropDialog {
        device_did: "718342728.s16".to_string(),
        device_name: "右键-客厅".to_string(),
        account_uid: "1001".to_string(),
        items: Vec::new(),
        selected: 0,
        active_tab: PropDialogTab::Actions,
        writable_selected: 0,
        readonly_selected: 0,
        actions: vec![ActionItem {
            siid: 5,
            aiid: 4,
            name: "无参动作".to_string(),
            input_piids: Vec::new(),
            input_labels: Vec::new(),
            input_props: Vec::new(),
        }],
        actions_selected: 0,
        writable_list_state: ListState::default(),
        readonly_list_state: ListState::default(),
        actions_list_state: ListState::default(),
        loading: false,
        loading_rx: None,
        status: None,
        editing: true,
        edit_buffer: String::new(),
        edit_cursor: 0,
        edit_error: None,
        refreshing: false,
        refresh_rx: None,
    };
    let mut app = test_app_with_prop_dialog(dialog);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    let text = terminal_text(&terminal);
    let compact_lines = text
        .lines()
        .map(|line| line.replace(' ', ""))
        .collect::<Vec<_>>();
    let action_row = compact_lines
        .iter()
        .position(|line| line.contains("无参动作"))
        .unwrap();
    let command_row = compact_lines
        .iter()
        .position(|line| line.contains("CLI命令(执行):mitpropsact718342728.s1654"))
        .unwrap();

    assert!(command_row <= action_row + 2, "{text}");
}

#[test]
fn action_enum_param_uses_selector_editor() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: vec![ToggleItem {
                prop: PropItem {
                    siid: 5,
                    piid: 3,
                    name: "执行模式".to_string(),
                    format: "uint8".to_string(),
                    writable: false,
                    value_options: vec![
                        super::PropValueOption {
                            label: "optionA".to_string(),
                            value: Value::Number(0.into()),
                        },
                        super::PropValueOption {
                            label: "optionB".to_string(),
                            value: Value::Number(1.into()),
                        },
                        super::PropValueOption {
                            label: "optionC".to_string(),
                            value: Value::Number(2.into()),
                        },
                    ],
                },
                value: Value::Number(1.into()),
            }],
            selected: 0,
            active_tab: PropDialogTab::Actions,
            writable_selected: 0,
            readonly_selected: 0,
            actions: vec![ActionItem {
                siid: 5,
                aiid: 1,
                name: "执行指令".to_string(),
                input_piids: vec![3],
                input_labels: vec!["执行模式".to_string()],
                input_props: Vec::new(),
            }],
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

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();
    assert!(app
        .prop_dialog
        .as_ref()
        .is_some_and(|dialog| dialog.editing));

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");
    assert!(compact.contains("[optionA]"), "{text}");
    assert!(compact.contains("[optionB]"), "{text}");
    assert!(compact.contains("[optionC]"), "{text}");
}

#[test]
fn action_bool_param_without_readable_prop_still_uses_selector_editor() {
    let spec = json!({
        "services": [{
            "iid": 5,
            "properties": [{
                "iid": 2,
                "description_trans": "指令静默执行",
                "format": "bool",
                "access": [],
                "type": "urn:miot-spec-v2:property:silent-execution:000000FB:xiaomi-oh4w:1"
            }],
            "actions": [{
                "iid": 1,
                "description_trans": "执行指令",
                "in": [2]
            }]
        }]
    });
    let actions = extract_actions_from_spec(&spec, Language::Chinese);

    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: Vec::new(),
            selected: 0,
            active_tab: PropDialogTab::Actions,
            writable_selected: 0,
            readonly_selected: 0,
            actions,
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

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");
    assert!(compact.contains("[true]"), "{text}");
    assert!(compact.contains("[false]"), "{text}");
}

#[test]
fn prop_editor_bottom_lines_show_get_then_set_for_writable_props() {
    let dialog = PropDialog {
        device_did: "dev-1".to_string(),
        device_name: "speaker".to_string(),
        account_uid: "1001".to_string(),
        items: vec![ToggleItem {
            prop: PropItem {
                siid: 2,
                piid: 1,
                name: "Power".to_string(),
                format: "bool".to_string(),
                writable: true,
                value_options: Vec::new(),
            },
            value: Value::Bool(true),
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
        editing: true,
        edit_buffer: "true".to_string(),
        edit_cursor: 4,
        edit_error: None,
        refreshing: false,
        refresh_rx: None,
    };

    assert_eq!(
        super::prop_editor_bottom_lines(&dialog, Language::Chinese),
        vec![
            "CLI 命令(读取): mit props get dev-1 2 1 --json".to_string(),
            "CLI 命令(执行): mit props set dev-1 2 1 true".to_string(),
        ]
    );
}

#[test]
fn draw_writable_prop_editor_shows_get_command_above_set_command() {
    let mut app = test_app_with_prop_dialog(PropDialog {
        device_did: "dev-1".to_string(),
        device_name: "speaker".to_string(),
        account_uid: "1001".to_string(),
        items: vec![ToggleItem {
            prop: PropItem {
                siid: 2,
                piid: 1,
                name: "Power".to_string(),
                format: "bool".to_string(),
                writable: true,
                value_options: Vec::new(),
            },
            value: Value::Bool(true),
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
        editing: true,
        edit_buffer: "true".to_string(),
        edit_cursor: 4,
        edit_error: None,
        refreshing: false,
        refresh_rx: None,
    });

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");

    let get_pos = compact
        .find("CLI命令(读取):mitpropsgetdev-121--json")
        .unwrap();
    let set_pos = compact
        .find("CLI命令(执行):mitpropssetdev-121true")
        .unwrap();
    assert!(get_pos < set_pos, "{text}");
}

#[test]
fn action_enum_param_without_readable_prop_still_uses_selector_editor() {
    let spec = json!({
        "services": [{
            "iid": 5,
            "properties": [{
                "iid": 3,
                "description_trans": "执行模式",
                "format": "uint8",
                "access": [],
                "value-list": [
                    {"value": 0, "description_trans": "optionA"},
                    {"value": 1, "description_trans": "optionB"},
                    {"value": 2, "description_trans": "optionC"}
                ]
            }],
            "actions": [{
                "iid": 1,
                "description_trans": "执行指令",
                "in": [3]
            }]
        }]
    });
    let actions = extract_actions_from_spec(&spec, Language::Chinese);

    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: Vec::new(),
            selected: 0,
            active_tab: PropDialogTab::Actions,
            writable_selected: 0,
            readonly_selected: 0,
            actions,
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

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");
    assert!(compact.contains("[optionA]"), "{text}");
    assert!(compact.contains("[optionB]"), "{text}");
    assert!(compact.contains("[optionC]"), "{text}");
}

#[test]
fn writable_bool_prop_enters_selector_editor_before_execution() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: vec![ToggleItem {
                prop: PropItem {
                    siid: 2,
                    piid: 1,
                    name: "电源".to_string(),
                    format: "bool".to_string(),
                    writable: true,
                    value_options: Vec::new(),
                },
                value: Value::Bool(true),
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

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();

    assert!(app
        .prop_dialog
        .as_ref()
        .is_some_and(|dialog| dialog.editing));
    assert!(app
        .prop_dialog
        .as_ref()
        .is_some_and(|dialog| dialog.editing));

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");
    assert!(compact.contains("[true]"), "{text}");
    assert!(compact.contains("[false]"), "{text}");
}

#[test]
fn writable_bool_prop_selector_highlights_current_option_in_green() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: vec![ToggleItem {
                prop: PropItem {
                    siid: 2,
                    piid: 1,
                    name: "电源".to_string(),
                    format: "bool".to_string(),
                    writable: true,
                    value_options: Vec::new(),
                },
                value: Value::Bool(true),
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

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(terminal_has_green_substring(&terminal, "[true]"));
}

#[test]
fn writable_enum_prop_enters_selector_editor_before_execution() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: vec![ToggleItem {
                prop: PropItem {
                    siid: 2,
                    piid: 2,
                    name: "模式".to_string(),
                    format: "uint8".to_string(),
                    writable: true,
                    value_options: vec![
                        super::PropValueOption {
                            label: "optionA".to_string(),
                            value: Value::Number(0.into()),
                        },
                        super::PropValueOption {
                            label: "optionB".to_string(),
                            value: Value::Number(1.into()),
                        },
                        super::PropValueOption {
                            label: "optionC".to_string(),
                            value: Value::Number(2.into()),
                        },
                    ],
                },
                value: Value::Number(1.into()),
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

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();

    assert!(app
        .prop_dialog
        .as_ref()
        .is_some_and(|dialog| dialog.editing));
    assert!(app
        .prop_dialog
        .as_ref()
        .is_some_and(|dialog| dialog.editing));

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    let compact = text.replace(' ', "");
    assert!(compact.contains("[optionA]"), "{text}");
    assert!(compact.contains("[optionB]"), "{text}");
    assert!(compact.contains("[optionC]"), "{text}");
}

#[test]
fn action_enum_param_selector_highlights_current_option_in_green() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: vec![ToggleItem {
                prop: PropItem {
                    siid: 5,
                    piid: 3,
                    name: "执行模式".to_string(),
                    format: "uint8".to_string(),
                    writable: false,
                    value_options: vec![
                        super::PropValueOption {
                            label: "optionA".to_string(),
                            value: Value::Number(0.into()),
                        },
                        super::PropValueOption {
                            label: "optionB".to_string(),
                            value: Value::Number(1.into()),
                        },
                        super::PropValueOption {
                            label: "optionC".to_string(),
                            value: Value::Number(2.into()),
                        },
                    ],
                },
                value: Value::Number(1.into()),
            }],
            selected: 0,
            active_tab: PropDialogTab::Actions,
            writable_selected: 0,
            readonly_selected: 0,
            actions: vec![ActionItem {
                siid: 5,
                aiid: 1,
                name: "执行指令".to_string(),
                input_piids: vec![3],
                input_labels: vec!["执行模式".to_string()],
                input_props: Vec::new(),
            }],
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

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(terminal_has_green_substring(&terminal, "[optionB]"));
}

#[test]
fn clicking_writable_bool_prop_selector_option_updates_selection() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: vec![ToggleItem {
                prop: PropItem {
                    siid: 2,
                    piid: 1,
                    name: "电源".to_string(),
                    format: "bool".to_string(),
                    writable: true,
                    value_options: Vec::new(),
                },
                value: Value::Bool(true),
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

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let (column, row) =
        terminal_find_substring_position(&terminal, "[false]").expect("selector option");

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 80, 24),
    )
    .unwrap();

    assert_eq!(
        app.prop_dialog.as_ref().unwrap().edit_buffer,
        "false".to_string()
    );
}

#[test]
fn clicking_action_enum_param_selector_option_updates_selection() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: vec![ToggleItem {
                prop: PropItem {
                    siid: 5,
                    piid: 3,
                    name: "执行模式".to_string(),
                    format: "uint8".to_string(),
                    writable: false,
                    value_options: vec![
                        super::PropValueOption {
                            label: "optionA".to_string(),
                            value: Value::Number(0.into()),
                        },
                        super::PropValueOption {
                            label: "optionB".to_string(),
                            value: Value::Number(1.into()),
                        },
                        super::PropValueOption {
                            label: "optionC".to_string(),
                            value: Value::Number(2.into()),
                        },
                    ],
                },
                value: Value::Number(1.into()),
            }],
            selected: 0,
            active_tab: PropDialogTab::Actions,
            writable_selected: 0,
            readonly_selected: 0,
            actions: vec![ActionItem {
                siid: 5,
                aiid: 1,
                name: "执行指令".to_string(),
                input_piids: vec![3],
                input_labels: vec!["执行模式".to_string()],
                input_props: Vec::new(),
            }],
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

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .unwrap();

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let (column, row) =
        terminal_find_substring_position(&terminal, "[optionC]").expect("selector option");

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 100, 24),
    )
    .unwrap();

    assert_eq!(
        app.prop_dialog.as_ref().unwrap().edit_buffer,
        "2".to_string()
    );
}

#[test]
fn clicking_action_editor_cli_command_does_not_copy_on_single_click() {
    let _guard = env_guard();
    let clip_file = make_temp_dir("tui-action-command-copy").join("clipboard.txt");
    std::env::set_var("MIT_TEST_CLIPBOARD_FILE", &clip_file);

    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: vec![ToggleItem {
                prop: PropItem {
                    siid: 5,
                    piid: 2,
                    name: "静默执行".to_string(),
                    format: "bool".to_string(),
                    writable: false,
                    value_options: Vec::new(),
                },
                value: Value::Bool(false),
            }],
            selected: 0,
            active_tab: PropDialogTab::Actions,
            writable_selected: 0,
            readonly_selected: 0,
            actions: vec![ActionItem {
                siid: 5,
                aiid: 4,
                name: "执行命令".to_string(),
                input_piids: vec![1, 2],
                input_labels: vec!["参数1".to_string(), "参数2".to_string()],
                input_props: vec![
                    PropItem {
                        siid: 5,
                        piid: 1,
                        name: "参数1".to_string(),
                        format: "string".to_string(),
                        writable: false,
                        value_options: Vec::new(),
                    },
                    PropItem {
                        siid: 5,
                        piid: 2,
                        name: "静默执行".to_string(),
                        format: "bool".to_string(),
                        writable: false,
                        value_options: Vec::new(),
                    },
                ],
            }],
            actions_selected: 0,
            writable_list_state: ListState::default(),
            readonly_list_state: ListState::default(),
            actions_list_state: ListState::default(),
            loading: false,
            loading_rx: None,
            status: None,
            editing: true,
            edit_buffer: "\"arg1\"\ntrue".to_string(),
            edit_cursor: 4,
            edit_error: None,
            refreshing: false,
            refresh_rx: None,
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

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let expected = r#"mit props act dev-1 5 4 "arg1" true"#;
    let (column, row) =
        terminal_find_substring_position(&terminal, expected).expect("command preview");

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 100, 24),
    )
    .unwrap();

    assert!(!clip_file.exists());
    assert!(!app
        .logs
        .iter()
        .any(|line| line.starts_with(super::FOOTER_COPY_LOG_PREFIX)));

    std::env::remove_var("MIT_TEST_CLIPBOARD_FILE");
    let _ = std::fs::remove_file(&clip_file);
}

#[test]
fn action_param_textarea_refocus_moves_cursor_to_end() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: Vec::new(),
            selected: 0,
            active_tab: PropDialogTab::Actions,
            writable_selected: 0,
            readonly_selected: 0,
            actions: vec![ActionItem {
                siid: 5,
                aiid: 1,
                name: "设置".to_string(),
                input_piids: vec![1, 2],
                input_labels: vec!["参数1".to_string(), "参数2".to_string()],
                input_props: vec![
                    PropItem {
                        siid: 5,
                        piid: 1,
                        name: "参数1".to_string(),
                        format: "string".to_string(),
                        writable: false,
                        value_options: Vec::new(),
                    },
                    PropItem {
                        siid: 5,
                        piid: 2,
                        name: "参数2".to_string(),
                        format: "string".to_string(),
                        writable: false,
                        value_options: Vec::new(),
                    },
                ],
            }],
            actions_selected: 0,
            writable_list_state: ListState::default(),
            readonly_list_state: ListState::default(),
            actions_list_state: ListState::default(),
            loading: false,
            loading_rx: None,
            status: None,
            editing: true,
            edit_buffer: "abcdef\nxy".to_string(),
            edit_cursor: 1,
            edit_error: None,
            refreshing: false,
            refresh_rx: None,
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

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
    )
    .unwrap();

    let dialog = app.prop_dialog.as_ref().unwrap();
    assert_eq!(dialog.writable_selected, 1);
    assert_eq!(dialog.edit_cursor, 2);
}

#[test]
fn dragging_selected_text_shows_footer_copied_badge() {
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
        logs: VecDeque::from(["alpha".to_string(), "beta".to_string(), "gamma".to_string()]),
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

    let _guard = env_guard();
    let clip_file = make_temp_dir("tui-selection-copy-badge").join("clipboard.txt");
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
    assert_eq!(copied, "gamma");
    assert!(app
        .logs
        .iter()
        .any(|line| line.starts_with(super::FOOTER_COPY_LOG_PREFIX)));
    let line = super::footer_line(&app, super::now_epoch_millis());
    assert!(line
        .spans
        .iter()
        .any(|span| span.content.as_ref().contains("[已复制]")));

    std::env::remove_var("MIT_TEST_CLIPBOARD_FILE");
    let _ = fs::remove_file(&clip_file);
}

#[test]
fn dragging_action_editor_cli_command_copies_preview_and_shows_badge() {
    let _guard = env_guard();
    let clip_file = make_temp_dir("tui-action-command-select-copy").join("clipboard.txt");
    std::env::set_var("MIT_TEST_CLIPBOARD_FILE", &clip_file);
    super::clear_selection_state();

    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: vec![ToggleItem {
                prop: PropItem {
                    siid: 5,
                    piid: 2,
                    name: "静默执行".to_string(),
                    format: "bool".to_string(),
                    writable: false,
                    value_options: Vec::new(),
                },
                value: Value::Bool(false),
            }],
            selected: 0,
            active_tab: PropDialogTab::Actions,
            writable_selected: 0,
            readonly_selected: 0,
            actions: vec![ActionItem {
                siid: 5,
                aiid: 4,
                name: "执行命令".to_string(),
                input_piids: vec![1, 2],
                input_labels: vec!["参数1".to_string(), "参数2".to_string()],
                input_props: vec![
                    PropItem {
                        siid: 5,
                        piid: 1,
                        name: "参数1".to_string(),
                        format: "string".to_string(),
                        writable: false,
                        value_options: Vec::new(),
                    },
                    PropItem {
                        siid: 5,
                        piid: 2,
                        name: "静默执行".to_string(),
                        format: "bool".to_string(),
                        writable: false,
                        value_options: Vec::new(),
                    },
                ],
            }],
            actions_selected: 0,
            writable_list_state: ListState::default(),
            readonly_list_state: ListState::default(),
            actions_list_state: ListState::default(),
            loading: false,
            loading_rx: None,
            status: None,
            editing: true,
            edit_buffer: "\"arg1\"\ntrue".to_string(),
            edit_cursor: 4,
            edit_error: None,
            refreshing: false,
            refresh_rx: None,
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

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let expected = r#"mit props act dev-1 5 4 "arg1" true"#;
    let (column, row) =
        terminal_find_substring_position(&terminal, expected).expect("command preview");
    let end_column = column.saturating_add(super::display_width(expected));

    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 100, 24),
    )
    .unwrap();
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Left),
            column: end_column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 100, 24),
    )
    .unwrap();
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Up(crossterm::event::MouseButton::Left),
            column: end_column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        ratatui::layout::Rect::new(0, 0, 100, 24),
    )
    .unwrap();

    assert_eq!(fs::read_to_string(&clip_file).unwrap(), expected);
    assert!(app
        .logs
        .iter()
        .any(|line| line.starts_with(super::FOOTER_COPY_LOG_PREFIX)));
    let line = super::footer_line(&app, super::now_epoch_millis());
    assert!(line
        .spans
        .iter()
        .any(|span| span.content.as_ref().contains("[已复制]")));

    std::env::remove_var("MIT_TEST_CLIPBOARD_FILE");
    let _ = fs::remove_file(&clip_file);
}

#[test]
fn draw_edit_mode_wraps_long_input_across_two_lines() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let long_input =
        "{\"alpha\":\"one two three four five six seven eight nine ten eleven twelve\"}";
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
            device_name: "living-room".to_string(),
            account_uid: "1001".to_string(),
            items: vec![ToggleItem {
                prop: PropItem {
                    siid: 2,
                    piid: 1,
                    name: "power".to_string(),
                    format: "string".to_string(),
                    writable: true,
                    value_options: Vec::new(),
                },
                value: Value::String("on".to_string()),
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
            editing: true,
            edit_buffer: long_input.to_string(),
            edit_cursor: long_input.chars().count(),
            edit_error: None,
            refreshing: false,
            refresh_rx: None,
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

    let mut terminal = Terminal::new(TestBackend::new(72, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    let text = terminal_text(&terminal);
    let lines = text.lines().collect::<Vec<_>>();
    let input_line = lines
        .iter()
        .position(|line| line.replace(' ', "").contains("输入值:"))
        .expect("input label");
    let textarea_lines = &lines[input_line + 1..input_line + 3];
    assert!(
        textarea_lines
            .iter()
            .all(|line| !line.contains("│") && !line.contains("┌") && !line.contains("└")),
        "{text}"
    );
    assert!(!textarea_lines[0].trim().is_empty(), "{text}");
    assert!(!textarea_lines[1].trim().is_empty(), "{text}");
}

#[test]
fn action_param_textarea_grows_height_when_value_wraps() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let long_input = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu";
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
            device_name: "speaker".to_string(),
            account_uid: "1001".to_string(),
            items: Vec::new(),
            selected: 0,
            active_tab: PropDialogTab::Actions,
            writable_selected: 0,
            readonly_selected: 0,
            actions: vec![ActionItem {
                siid: 5,
                aiid: 1,
                name: "执行文本指令".to_string(),
                input_piids: vec![1],
                input_labels: vec!["参数1".to_string()],
                input_props: vec![PropItem {
                    siid: 5,
                    piid: 1,
                    name: "参数1".to_string(),
                    format: "string".to_string(),
                    writable: false,
                    value_options: Vec::new(),
                }],
            }],
            actions_selected: 0,
            writable_list_state: ListState::default(),
            readonly_list_state: ListState::default(),
            actions_list_state: ListState::default(),
            loading: false,
            loading_rx: None,
            status: None,
            editing: true,
            edit_buffer: long_input.to_string(),
            edit_cursor: long_input.chars().count(),
            edit_error: None,
            refreshing: false,
            refresh_rx: None,
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

    let mut terminal = Terminal::new(TestBackend::new(60, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();

    let text = terminal_text(&terminal);
    let lines = text.lines().collect::<Vec<_>>();
    let first_param_line = lines
        .iter()
        .position(|line| line.replace(' ', "").contains(">参数1:"))
        .expect("action param label");
    let wrapped_line = &lines[first_param_line + 1];
    assert!(!wrapped_line.trim().is_empty(), "{text}");
    let s = wrapped_line.replace(' ', "");
    assert!(
        !s.starts_with("CLI命令:")
            && !s.starts_with("CLI命令(执行):")
            && !s.starts_with("CLI命令(读取):"),
        "{text}"
    );
}

#[test]
fn prop_edit_mode_moves_cursor_with_left_right() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();
    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: vec![normalize_account(json!({
            "region": "cn",
            "redirectUri": "http://127.0.0.1:8000/login_redirect",
            "uuid": "uuid-a",
            "deviceId": "device-a",
            "state": "state-a",
            "accessToken": "token-a",
            "refreshToken": "refresh-a",
            "expiresTs": 32503680000_u64,
            "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
        }))],
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
            device_name: "living-room".to_string(),
            account_uid: "1001".to_string(),
            items: vec![ToggleItem {
                prop: PropItem {
                    siid: 2,
                    piid: 1,
                    name: "power".to_string(),
                    format: "string".to_string(),
                    writable: true,
                    value_options: Vec::new(),
                },
                value: Value::String("true".to_string()),
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
            editing: true,
            edit_buffer: "true".to_string(),
            edit_cursor: 4,
            edit_error: None,
            refreshing: false,
            refresh_rx: None,
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

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
    )
    .unwrap();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("true"), "{text}");
    assert!(!text.contains("tru|e"), "{text}");
    assert!(terminal_has_reversed_cell(&terminal));

    handle_key(
        &mut app,
        crossterm::event::KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
    )
    .unwrap();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let text = terminal_text(&terminal);
    assert!(text.contains("true"), "{text}");
    assert!(!text.contains("true|"), "{text}");
    assert!(terminal_has_reversed_cell(&terminal));
}

#[test]
fn process_bootstrap_message_ignores_stale_results_when_not_pending() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();

    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: default_auth(),
        accounts: Vec::new(),
        account_index: 0,
        devices: vec![Device {
            did: "dev-new".to_string(),
            name: "new".to_string(),
            model: "new.model".to_string(),
            online: true,

            home_id: "home-1".to_string(),
            home_name: "我家".to_string(),
            room_id: "room-1".to_string(),
            room_name: "客厅".to_string(),
        }],
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
        boot_state: BootState::Ready,
        boot_spinner_index: 0,
        bootstrap_generation: 2,
        bootstrap_pending: None,
        bootstrap_tx: bootstrap_tx.clone(),
        bootstrap_rx,
        property_cache: Arc::new(PropertyCache::new()),
        language: Language::Chinese,
        auto_subscribe_device_status: true,
        settings_selected: 0,
    };

    bootstrap_tx
        .send(BootstrapMessage::Ready {
            generation: 1,
            uid: "1001".to_string(),
            offline_uids: Vec::new(),
            auth_state: default_auth(),
            accounts: Vec::new(),
            devices: vec![Device {
                did: "dev-stale".to_string(),
                name: "stale".to_string(),
                model: "stale.model".to_string(),
                online: true,

                home_id: "home-1".to_string(),
                home_name: "我家".to_string(),
                room_id: "room-1".to_string(),
                room_name: "客厅".to_string(),
            }],
            logs: vec!["stale".to_string()],
        })
        .unwrap();

    app.process_background_messages();

    assert_eq!(app.devices[0].did, "dev-new");
    assert_eq!(app.boot_state, BootState::Ready);
}

#[test]
fn process_bootstrap_message_ignores_stale_results_for_wrong_generation() {
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
        .send(BootstrapMessage::Failed {
            generation: 1,
            uid: "1001".to_string(),
            auth_state: None,
            accounts: None,
            error: "stale".to_string(),
        })
        .unwrap();

    app.process_background_messages();

    assert_eq!(app.boot_state, BootState::Loading);
    assert!(app.logs.is_empty());
}

#[test]
fn process_bootstrap_message_applies_refreshed_auth_state() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();

    let old = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-old",
        "deviceId": "mico.tui-old",
        "state": "state-a",
        "accessToken": "old-token",
        "refreshToken": "refresh",
        "expiresTs": 1,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));
    let refreshed = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-old",
        "deviceId": "mico.tui-new",
        "state": "state-b",
        "accessToken": "new-token",
        "refreshToken": "new-refresh",
        "expiresTs": 2,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));
    let mut refreshed_state = default_auth();
    refreshed_state.accounts = vec![refreshed.clone()];

    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: {
            let mut state = default_auth();
            state.accounts = vec![old.clone()];
            state
        },
        accounts: vec![old],
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
        boot_spinner_index: 0,
        bootstrap_generation: 1,
        bootstrap_pending: Some(BootstrapPending {
            generation: 1,
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
            generation: 1,
            uid: "1001".to_string(),
            offline_uids: Vec::new(),
            auth_state: refreshed_state.clone(),
            accounts: vec![refreshed.clone()],
            devices: Vec::new(),
            logs: vec!["ok".to_string()],
        })
        .unwrap();

    app.process_background_messages();

    assert_eq!(app.auth_state, refreshed_state);
    assert_eq!(app.accounts, vec![refreshed]);
    assert_eq!(app.account_index, 0);
}

#[test]
fn process_bootstrap_message_rewarms_local_transport_when_snapshot_missing() {
    let home = make_temp_dir("tui-bootstrap-missing-local-credentials");
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();

    let account = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-bootstrap-refresh",
        "deviceId": "mico.tui-bootstrap-refresh",
        "state": "state-a",
        "accessToken": "token-a",
        "refreshToken": "refresh-a",
        "expiresTs": 1,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));
    let mut auth_state = default_auth();
    auth_state.accounts = vec![account.clone()];

    let mut app = TuiApp {
        home_dir: home.clone(),
        auth_state: default_auth(),
        accounts: vec![account.clone()],
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
        local_transport_refresh_generation: 7,
        local_transport_refresh_device_id: Some(account.device_id.clone()),
        local_transport_force_refresh_pending: false,
        local_transport_tx,
        local_transport_rx,
        auth_flow_generation: 0,
        auth_flow_tx,
        auth_flow_rx,
        offline_account_uids: HashSet::new(),
        boot_state: BootState::Ready,
        boot_spinner_index: 0,
        bootstrap_generation: 1,
        bootstrap_pending: Some(BootstrapPending {
            generation: 1,
            uid: "1001".to_string(),
            refresh_local_transport_if_missing: true,
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
            generation: 1,
            uid: "1001".to_string(),
            offline_uids: Vec::new(),
            auth_state,
            accounts: vec![account],
            devices: vec![Device {
                did: "dev-1".to_string(),
                name: "living-room".to_string(),
                model: "xiaomi.wifispeaker.lx04".to_string(),
                online: true,
                home_id: "home-1".to_string(),
                home_name: "我家".to_string(),
                room_id: "room-1".to_string(),
                room_name: "客厅".to_string(),
            }],
            logs: vec!["loaded 1 devices".to_string()],
        })
        .unwrap();

    app.process_background_messages();

    assert_eq!(app.local_transport_refresh_generation, 8);

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn process_bootstrap_failure_logs_error_and_keeps_tui_ready() {
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
        boot_spinner_index: 0,
        bootstrap_generation: 1,
        bootstrap_pending: Some(BootstrapPending {
            generation: 1,
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
        .send(BootstrapMessage::Failed {
            generation: 1,
            uid: "1001".to_string(),
            auth_state: None,
            accounts: None,
            error: "boom".to_string(),
        })
        .unwrap();

    app.process_background_messages();

    assert!(matches!(app.boot_state, BootState::Ready));
    assert!(app.logs.iter().any(|line| line.contains("boom")));
}

#[test]
fn process_bootstrap_failure_applies_refreshed_auth_state() {
    let (bootstrap_tx, bootstrap_rx) = mpsc::channel::<BootstrapMessage>();
    let (local_transport_tx, local_transport_rx) = mpsc::channel::<LocalTransportRefreshMessage>();
    let (auth_flow_tx, auth_flow_rx) = mpsc::channel::<AuthFlowMessage>();

    let old = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-old",
        "deviceId": "mico.tui-old",
        "state": "state-a",
        "accessToken": "old-token",
        "refreshToken": "refresh",
        "expiresTs": 1,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));
    let refreshed = normalize_account(json!({
        "region": "cn",
        "redirectUri": "http://127.0.0.1:8000/login_redirect",
        "uuid": "tui-old",
        "deviceId": "mico.tui-new",
        "state": "state-b",
        "accessToken": "new-token",
        "refreshToken": "new-refresh",
        "expiresTs": 2,
        "user": {"uid": "1001", "nickname": "账号A", "icon": "", "unionId": "union-a"}
    }));
    let mut refreshed_state = default_auth();
    refreshed_state.accounts = vec![refreshed.clone()];

    let mut app = TuiApp {
        home_dir: PathBuf::from("."),
        auth_state: {
            let mut state = default_auth();
            state.accounts = vec![old.clone()];
            state
        },
        accounts: vec![old],
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
        boot_spinner_index: 0,
        bootstrap_generation: 1,
        bootstrap_pending: Some(BootstrapPending {
            generation: 1,
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
        .send(BootstrapMessage::Failed {
            generation: 1,
            uid: "1001".to_string(),
            auth_state: Some(refreshed_state.clone()),
            accounts: Some(vec![refreshed.clone()]),
            error: "boom".to_string(),
        })
        .unwrap();

    app.process_background_messages();

    assert_eq!(app.auth_state, refreshed_state);
    assert_eq!(app.accounts, vec![refreshed]);
    assert_eq!(app.account_index, 0);
    assert!(matches!(app.boot_state, BootState::Ready));
    assert!(app.logs.iter().any(|line| line.contains("boom")));
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
fn sync_command_keeps_ui_ready_while_background_sync_runs() {
    let home = make_temp_dir("tui-sync-non-blocking-ready");
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
        home_dir: home.clone(),
        auth_state: default_auth(),
        accounts: vec![account],
        account_index: 0,
        devices: vec![Device {
            did: "dev-1".to_string(),
            name: "cached-device".to_string(),
            model: "xiaomi.gateway.hub1".to_string(),
            online: true,

            home_id: "home-1".to_string(),
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
    assert!(
        matches!(app.boot_state, BootState::Ready),
        "sync should keep UI interactive while background sync runs"
    );
    assert!(app.bootstrap_pending.is_some());
    assert_eq!(app.devices.len(), 1);

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

#[test]
fn start_bootstrap_creates_local_credentials_snapshot_without_restart() {
    let _guard = env_guard();
    let server = MockMicoServer::start();
    let home = make_temp_dir("tui-bootstrap-creates-local-credentials");
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

    app.start_bootstrap();

    let local_credentials_path = home
        .join(".mit")
        .join("accounts")
        .join("1001")
        .join("local_credentials.json");
    let deadline = Instant::now() + Duration::from_secs(8);
    while !local_credentials_path.exists() {
        app.process_background_messages();
        assert!(
            Instant::now() < deadline,
            "logs={:?} requests={:?}",
            app.logs,
            server.requests()
        );
        thread::sleep(Duration::from_millis(10));
    }

    std::env::remove_var("MIT_HOME");
    std::env::remove_var("MIT_PROFILE_DIR");
    std::env::remove_var("MIT_MICO_BASE_URL");
    std::env::remove_var("MIT_USER_PROFILE_URL");
    std::env::remove_var("MIT_MIOT_SPEC_URL_BASE");
    let _ = fs::remove_dir_all(&home);
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

#[test]
fn parse_bool_prop_value_handles_entry_object_shape() {
    assert_eq!(parse_bool_prop_value(&json!({"value": true})), Some(true));
    assert_eq!(parse_bool_prop_value(&json!({"value": 0})), Some(false));
}

#[test]
fn format_prop_value_for_dialog_decodes_backslash_x_utf8_sequences() {
    let encoded = json!("\\xe6\\x96\\xb0\\xe9\\x98\\xb3");
    assert_eq!(format_prop_value_for_dialog(&encoded), "新阳");
}

#[test]
fn format_prop_value_for_dialog_renders_dash_for_negative_code_object() {
    let value = json!({"code": -704042011, "did": "12345"});
    assert_eq!(format_prop_value_for_dialog(&value), "-");
}
