// Auto-split: shares the `tests` module scope (imports + helpers) of
// mod.rs via include!; do not add `use` here.

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
fn parse_bool_prop_value_handles_entry_object_shape() {
    assert_eq!(parse_bool_prop_value(&json!({"value": true})), Some(true));
    assert_eq!(parse_bool_prop_value(&json!({"value": 0})), Some(false));
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
