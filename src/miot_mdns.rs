//! mDNS discovery of the MIoT central hub gateway (`_miot-central._tcp.local.`).
//!
//! Port of `XiaoMi/ha_xiaomi_home`'s `miot/miot_mdns.py`. This does **not**
//! discover individual devices — it detects whether a central hub gateway is
//! present on the LAN. When one is, direct UDP LAN control should be disabled
//! (the gateway provides local pub/sub over its own MQTT/MIPS bus instead), so
//! [`crate::miot_lan`] is gated off by the routing layer.
//!
//! A primary hub is identified by decoding the base64 `profile` TXT record:
//! `role == 1` (primary) and the MQTT-suite capability bit must be set.

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use anyhow::Result;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use mdns_sd::{ResolvedService, ServiceDaemon, ServiceEvent};

const MIPS_MDNS_TYPE: &str = "_miot-central._tcp.local.";

/// A discovered central hub gateway service.
#[derive(Clone, Debug)]
pub struct CentralHub {
    pub group_id: String,
    pub did: String,
    pub addresses: Vec<Ipv4Addr>,
    pub port: u16,
    pub role: u8,
    pub suite_mqtt: bool,
}

struct Inner {
    daemon: ServiceDaemon,
    has_hub: Arc<AtomicBool>,
    hubs: Arc<Mutex<HashMap<String, CentralHub>>>,
    join: Mutex<Option<JoinHandle<()>>>,
}

/// Background monitor for the presence of a central hub gateway. Cheap to clone.
#[derive(Clone)]
pub struct CentralHubMonitor {
    inner: Arc<Inner>,
}

impl CentralHubMonitor {
    /// Start browsing for `_miot-central._tcp.local.` on a background thread.
    pub fn start() -> Result<Self> {
        let daemon = ServiceDaemon::new()?;
        let rx = daemon.browse(MIPS_MDNS_TYPE)?;
        let has_hub = Arc::new(AtomicBool::new(false));
        let hubs: Arc<Mutex<HashMap<String, CentralHub>>> = Arc::new(Mutex::new(HashMap::new()));

        let t_has = has_hub.clone();
        let t_hubs = hubs.clone();
        let join = std::thread::Builder::new()
            .name("miot_mdns".to_string())
            .spawn(move || {
                // recv() returns Err once the daemon shuts down (channel closes).
                while let Ok(event) = rx.recv() {
                    match event {
                        ServiceEvent::ServiceResolved(rs) => {
                            if let Some(hub) = parse_central_hub(&rs) {
                                if let Ok(mut map) = t_hubs.lock() {
                                    map.insert(rs.get_fullname().to_string(), hub);
                                }
                            }
                        }
                        ServiceEvent::ServiceRemoved(_, fullname) => {
                            if let Ok(mut map) = t_hubs.lock() {
                                map.remove(&fullname);
                            }
                        }
                        _ => {}
                    }
                    let any = t_hubs.lock().map(|m| !m.is_empty()).unwrap_or(false);
                    t_has.store(any, Ordering::SeqCst);
                }
            })?;

        Ok(Self {
            inner: Arc::new(Inner {
                daemon,
                has_hub,
                hubs,
                join: Mutex::new(Some(join)),
            }),
        })
    }

    /// Whether a valid primary central hub gateway is currently present.
    pub fn has_primary_hub(&self) -> bool {
        self.inner.has_hub.load(Ordering::SeqCst)
    }

    /// Snapshot of the discovered central hubs.
    pub fn hubs(&self) -> Vec<CentralHub> {
        self.inner
            .hubs
            .lock()
            .map(|m| m.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Stop browsing and join the background thread.
    pub fn stop(&self) {
        let _ = self.inner.daemon.shutdown();
        if let Ok(mut guard) = self.inner.join.lock() {
            if let Some(handle) = guard.take() {
                let _ = handle.join();
            }
        }
    }
}

/// Decode the base64 `profile` TXT record into a [`CentralHub`], returning `None`
/// unless it is a valid primary (`role == 1`) hub with MQTT-suite support.
fn parse_central_hub(rs: &ResolvedService) -> Option<CentralHub> {
    let profile_b64 = rs.get_property_val_str("profile")?;
    let bin = STANDARD.decode(profile_b64.trim()).ok()?;
    // Layout (from miot_mdns.py): did=[1..9] BE, group_id=hex(reverse([9..17])),
    // role=byte[20]>>4, suite_mqtt=(byte[22]>>1)&1.
    if bin.len() < 23 {
        return None;
    }
    let role = bin[20] >> 4;
    let suite_mqtt = ((bin[22] >> 1) & 0x01) == 0x01;
    if role != 1 || !suite_mqtt {
        return None;
    }
    let did = u64::from_be_bytes(bin[1..9].try_into().ok()?).to_string();
    let mut gid = bin[9..17].to_vec();
    gid.reverse();
    let group_id = hex_lower(&gid);

    Some(CentralHub {
        group_id,
        did,
        addresses: rs.get_addresses_v4().into_iter().collect(),
        port: rs.get_port(),
        role,
        suite_mqtt,
    })
}

fn hex_lower(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_lower_encodes_bytes() {
        assert_eq!(hex_lower(&[0x00, 0x0f, 0xa0, 0xff]), "000fa0ff");
    }
}
