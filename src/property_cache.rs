use serde_json::Value;
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::Instant;

/// A single property value with timestamp.
#[derive(Clone, Debug)]
pub struct CachedValue {
    pub value: Value,
    pub timestamp: Instant,
}

/// Device property map: (siid, piid) -> (value, timestamp).
#[derive(Clone, Debug, Default)]
pub struct DevicePropertyMap {
    pub properties: HashMap<(i64, i64), CachedValue>,
}

/// In-memory cache for all device properties.
/// Thread-safe wrapper around HashMap<device_id, DevicePropertyMap>.
pub struct PropertyCache {
    devices: RwLock<HashMap<String, DevicePropertyMap>>,
}

impl PropertyCache {
    pub fn new() -> Self {
        Self {
            devices: RwLock::new(HashMap::new()),
        }
    }

    /// Get all properties for a device.
    /// Returns empty map if device not in cache yet.
    pub fn get_device(&self, device_id: &str) -> DevicePropertyMap {
        let device_id = prop_cache_lookup_did(device_id);
        self.devices
            .read()
            .unwrap()
            .get(device_id.as_str())
            .cloned()
            .unwrap_or_default()
    }

    /// Get a single property value.
    pub fn get_property(&self, device_id: &str, siid: i64, piid: i64) -> Option<Value> {
        let device_id = prop_cache_lookup_did(device_id);
        self.devices
            .read()
            .unwrap()
            .get(device_id.as_str())
            .and_then(|dev| dev.properties.get(&(siid, piid)).map(|cv| cv.value.clone()))
    }

    /// Set properties for a device (batch update).
    pub fn set_device_properties(&self, device_id: String, properties: HashMap<(i64, i64), Value>) {
        let device_id = prop_cache_lookup_did(&device_id);
        let mut devices = self.devices.write().unwrap();
        let now = Instant::now();
        let device_props = devices.entry(device_id).or_default();
        for (key, value) in properties {
            device_props.properties.insert(
                key,
                CachedValue {
                    value,
                    timestamp: now,
                },
            );
        }
    }

    /// Clear all properties for a device.
    pub fn clear_device(&self, device_id: &str) {
        let device_id = prop_cache_lookup_did(device_id);
        self.devices.write().unwrap().remove(device_id.as_str());
    }

    /// Clear all cached properties.
    pub fn clear_all(&self) {
        self.devices.write().unwrap().clear();
    }
}

fn prop_cache_lookup_did(did: &str) -> String {
    let (root, suffix) = did.rsplit_once(".s").unwrap_or((did, ""));
    if root.is_empty() || suffix.is_empty() {
        return did.to_string();
    }
    if suffix.chars().all(|ch| ch.is_ascii_digit()) {
        root.to_string()
    } else {
        did.to_string()
    }
}

impl Default for PropertyCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "../tests/module_tests/property_cache.rs"]
mod tests;
