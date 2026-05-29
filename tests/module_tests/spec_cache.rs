use super::{
    apply_zh_cn_translation, resolve_spec_type_from_instances, spec_needs_value_list_translation,
};
use serde_json::json;

#[test]
fn resolve_spec_type_uses_model_mapping() {
    let instances = json!({
        "instances": [
            {
                "model": "xiaomi.wifispeaker.lx04",
                "type": "urn:miot-spec-v2:device:speaker:0000A015:xiaomi-lx04:1"
            }
        ]
    });
    let resolved = resolve_spec_type_from_instances("xiaomi.wifispeaker.lx04", &instances).unwrap();
    assert_eq!(
        resolved,
        "urn:miot-spec-v2:device:speaker:0000A015:xiaomi-lx04:1"
    );
}

#[test]
fn resolve_spec_type_keeps_urn_input() {
    let input = "urn:miot-spec-v2:device:speaker:0000A015:xiaomi-lx04:1";
    let resolved = resolve_spec_type_from_instances(input, &json!({ "instances": [] })).unwrap();
    assert_eq!(resolved, input);
}

#[test]
fn resolve_spec_type_prefers_latest_ts_for_same_model() {
    let instances = json!({
        "instances": [
            {
                "model": "xiaomi.wifispeaker.lx04",
                "type": "urn:miot-spec-v2:device:speaker:0000A015:xiaomi-lx04:1",
                "ts": 100
            },
            {
                "model": "xiaomi.wifispeaker.lx04",
                "type": "urn:miot-spec-v2:device:speaker:0000A015:xiaomi-lx04:2",
                "ts": 200
            }
        ]
    });
    let resolved = resolve_spec_type_from_instances("xiaomi.wifispeaker.lx04", &instances).unwrap();
    assert_eq!(
        resolved,
        "urn:miot-spec-v2:device:speaker:0000A015:xiaomi-lx04:2"
    );
}

#[test]
fn apply_zh_cn_translation_persists_value_list_copy() {
    let mut spec = json!({
        "services": [
            {
                "iid": 2,
                "description": "Gateway",
                "properties": [
                    {
                        "iid": 1,
                        "description": "Access Mode",
                        "value-list": [
                            {"value": 0, "description": "LAN"},
                            {"value": 1, "description": "Wireless 5 G"},
                            {"value": 2, "description": "Wireless 2 G"}
                        ]
                    }
                ]
            }
        ]
    });
    let multi_lang = json!({
        "data": {
            "zh_cn": {
                "service:002": "网关",
                "service:002:property:001": "接入方式",
                "service:002:property:001:valuelist:000": "有线",
                "service:002:property:001:valuelist:001": "5G 无线",
                "service:002:property:001:valuelist:002": "2.4G 无线"
            }
        }
    });

    apply_zh_cn_translation(&mut spec, &multi_lang);

    assert_eq!(spec["services"][0]["description_trans"], "网关");
    assert_eq!(
        spec["services"][0]["properties"][0]["description_trans"],
        "接入方式"
    );
    assert_eq!(
        spec["services"][0]["properties"][0]["value-list"][0]["description_trans"],
        "有线"
    );
    assert_eq!(
        spec["services"][0]["properties"][0]["value-list"][1]["description_trans"],
        "5G 无线"
    );
    assert_eq!(
        spec["services"][0]["properties"][0]["value-list"][2]["description_trans"],
        "2.4G 无线"
    );
}

#[test]
fn spec_needs_value_list_translation_detects_missing_value_list_copy() {
    let spec = json!({
        "services": [
            {
                "iid": 2,
                "description_trans": "网关",
                "properties": [
                    {
                        "iid": 1,
                        "description_trans": "接入方式",
                        "value-list": [
                            {"value": 0, "description": "LAN"},
                            {"value": 1, "description": "Wireless 5 G", "description_trans": "5G 无线"}
                        ]
                    }
                ]
            }
        ]
    });

    assert!(spec_needs_value_list_translation(&spec));
}
