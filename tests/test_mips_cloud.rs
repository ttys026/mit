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
        property_topic_filter("1234567890"),
        "device/1234567890/up/properties_changed/#"
    );
    let specific_property = property_subscription_for("1234567890", Some((2, 1)));
    assert_eq!(
        specific_property.topic_filter,
        "device/1234567890/up/properties_changed/#"
    );
    assert_eq!(specific_property.property, Some((2, 1)));
}

#[test]
fn cloud_mips_parses_property_update_payload() {
    let payload = json!({
        "params": {
            "did": "1234567890",
            "siid": 2,
            "piid": 1,
            "value": true
        }
    })
    .to_string();

    let update = parse_property_update(
        "device/1234567890/up/properties_changed/2/1",
        payload.as_bytes(),
    )
    .unwrap();

    assert_eq!(update.did, "1234567890");
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
        "device/topic-did/up/properties_changed/3/8",
        payload.as_bytes(),
    )
    .unwrap();

    assert_eq!(update.did, "topic-did");
    assert_eq!(update.siid, 3);
    assert_eq!(update.piid, 8);
    assert_eq!(update.value, json!(42));
}
