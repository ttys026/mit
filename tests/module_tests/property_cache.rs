use super::*;
use serde_json::json;

#[test]
fn test_cache_new() {
    let cache = PropertyCache::new();
    assert_eq!(cache.get_device("dev-1").properties.len(), 0);
}

#[test]
fn test_set_and_get_property() {
    let cache = PropertyCache::new();
    let mut props = HashMap::new();
    props.insert((2, 1), json!(true));
    cache.set_device_properties("dev-1".to_string(), props);

    let value = cache.get_property("dev-1", 2, 1);
    assert_eq!(value, Some(json!(true)));
}

#[test]
fn test_get_device_returns_map() {
    let cache = PropertyCache::new();
    let mut props = HashMap::new();
    props.insert((2, 1), json!(true));
    props.insert((2, 2), json!(42));
    cache.set_device_properties("dev-1".to_string(), props);

    let device = cache.get_device("dev-1");
    assert_eq!(device.properties.len(), 2);
    assert_eq!(
        device.properties.get(&(2, 1)).map(|v| &v.value),
        Some(&json!(true))
    );
    assert_eq!(
        device.properties.get(&(2, 2)).map(|v| &v.value),
        Some(&json!(42))
    );
}

#[test]
fn test_clear_device() {
    let cache = PropertyCache::new();
    let mut props = HashMap::new();
    props.insert((2, 1), json!(true));
    cache.set_device_properties("dev-1".to_string(), props);

    cache.clear_device("dev-1");
    assert_eq!(cache.get_device("dev-1").properties.len(), 0);
}

#[test]
fn test_clear_all() {
    let cache = PropertyCache::new();
    let mut props1 = HashMap::new();
    props1.insert((2, 1), json!(true));
    cache.set_device_properties("dev-1".to_string(), props1);

    let mut props2 = HashMap::new();
    props2.insert((2, 1), json!(false));
    cache.set_device_properties("dev-2".to_string(), props2);

    cache.clear_all();
    assert_eq!(cache.get_device("dev-1").properties.len(), 0);
    assert_eq!(cache.get_device("dev-2").properties.len(), 0);
}

#[test]
fn test_get_property_uses_root_did_for_sub_devices() {
    let cache = PropertyCache::new();
    let mut props = HashMap::new();
    props.insert((2, 1), json!(true));
    cache.set_device_properties("2045081210".to_string(), props);

    let value = cache.get_property("2045081210.s2", 2, 1);
    assert_eq!(value, Some(json!(true)));
}
