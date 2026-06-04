use mit::mips_cloud::{
    build_cloud_broker_host, build_cloud_client_id, parse_property_update,
    property_subscription_for, property_topic_filter,
};
use serde_json::json;

#[test]
fn cloud_mips_builds_xiaomi_topic_and_identity() {
    assert_eq!(build_cloud_client_id("abcd"), "ha.abcd");
    assert_eq!(build_cloud_broker_host("cn"), "cn-ha.mqtt.io.mi.com");
    assert_eq!(
        property_topic_filter("mock-device-1"),
        "device/mock-device-1/up/properties_changed/#"
    );
    let specific_property = property_subscription_for("mock-device-1", Some((2, 1)));
    assert_eq!(
        specific_property.topic_filter,
        "device/mock-device-1/up/properties_changed/#"
    );
    assert_eq!(specific_property.property, Some((2, 1)));
}

#[test]
fn cloud_mips_parses_property_update_payload() {
    let payload = json!({
        "params": {
            "did": "mock-device-1",
            "siid": 2,
            "piid": 1,
            "value": true
        }
    })
    .to_string();

    let update = parse_property_update(
        "device/mock-device-1/up/properties_changed/2/1",
        payload.as_bytes(),
    )
    .unwrap();

    assert_eq!(update.did, "mock-device-1");
    assert_eq!(update.siid, 2);
    assert_eq!(update.piid, 1);
    assert_eq!(update.value, json!(true));
}

#[test]
fn cloud_mips_uses_topic_did_when_payload_omits_it() {
    let payload = json!({
        "params": {
            "siid": 3,
            "piid": 8,
            "value": 42
        }
    })
    .to_string();

    let update = parse_property_update(
        "device/mock-topic-did/up/properties_changed/3/8",
        payload.as_bytes(),
    )
    .unwrap();

    assert_eq!(update.did, "mock-topic-did");
    assert_eq!(update.siid, 3);
    assert_eq!(update.piid, 8);
    assert_eq!(update.value, json!(42));
}

#[test]
fn cloud_mips_prefers_topic_did_when_payload_did_differs() {
    let topic_did = "x.mock-region.mock-account.mock-ac-token";
    let payload = json!({
        "params": {
            "did": "mock-payload-did",
            "siid": 2,
            "piid": 4,
            "value": 26
        }
    })
    .to_string();

    let update = parse_property_update(
        &format!("device/{topic_did}/up/properties_changed/2/4"),
        payload.as_bytes(),
    )
    .unwrap();

    assert_eq!(update.did, topic_did);
    assert_eq!(update.siid, 2);
    assert_eq!(update.piid, 4);
    assert_eq!(update.value, json!(26));
}

#[test]
fn cloud_mips_uses_topic_property_iids_when_payload_omits_them() {
    let payload = json!({
        "params": {
            "value": 26
        }
    })
    .to_string();

    let update = parse_property_update(
        "device/mock-topic-did/up/properties_changed/2/4",
        payload.as_bytes(),
    )
    .unwrap();

    assert_eq!(update.did, "mock-topic-did");
    assert_eq!(update.siid, 2);
    assert_eq!(update.piid, 4);
    assert_eq!(update.value, json!(26));
}
