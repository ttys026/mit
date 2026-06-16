//! Local network (LAN) control of MIoT Wi-Fi devices over UDP.
//!
//! This is a pragmatic, synchronous port of the LAN flow used by
//! `XiaoMi/ha_xiaomi_home` (`miot/miot_lan.py`). The reference uses an asyncio
//! event loop on a dedicated thread; here we run a single background thread with
//! a blocking [`UdpSocket`] whose read-timeout doubles as the timer tick. The
//! thread is woken promptly for outbound work via a loopback "self-wake" packet
//! sent to the loop socket's own port.
//!
//! Unlike the legacy per-request handshake in [`crate::miio_local`], devices are
//! discovered by broadcasting a 32-byte `MDID` probe (see [`build_probe`]). Each
//! reply teaches us the device's real 8-byte DID, its clock (so we can derive
//! the packet timestamp without a handshake) and its IP. A per-device keepalive
//! state machine (FRESH/PING/DEAD) tracks liveness, and — when enabled — devices
//! that advertise wildcard subscription push `properties_changed` /
//! `event_occured` messages which are surfaced on a channel.

use anyhow::{anyhow, bail, Result};
use rand::Rng;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::miio_local::{decrypt_payload, encrypt_payload};

const OT_HEADER: [u8; 2] = [0x21, 0x31];
const OT_PORT: u16 = 54321;
const OT_PROBE_LEN: usize = 32;
const OT_MSG_LEN: usize = 1400;
const OT_SUPPORT_WILDCARD_SUB: u8 = 0xFE;

const SCAN_INTERVAL_MIN: f64 = 5.0;
const SCAN_INTERVAL_MAX: f64 = 45.0;

const KA_INTERVAL_MIN: f64 = 10.0;
const KA_INTERVAL_MAX: f64 = 50.0;
const FAST_PING_INTERVAL: Duration = Duration::from_secs(5);
const NETWORK_UNSTABLE_CNT_TH: usize = 10;
const NETWORK_UNSTABLE_TIME_TH: f64 = 120.0;
const NETWORK_UNSTABLE_RESUME_TH: f64 = 300.0;

const DEDUP_WINDOW: Duration = Duration::from_secs(5);
/// Upper bound on how long the loop blocks in `recv` before re-checking timers.
const LOOP_TICK_MAX: Duration = Duration::from_millis(1000);

/// Device descriptor handed to the manager from the credential layer.
#[derive(Clone, Debug)]
pub struct LanDeviceInfo {
    pub did: String,
    pub token: String,
    pub model: String,
    pub ip: Option<String>,
}

/// Configuration for [`LanManager::start`].
#[derive(Clone, Debug, Default)]
pub struct LanConfig {
    /// Enable `miIO.sub` push subscription for devices that support it.
    pub enable_subscribe: bool,
    /// Virtual DID used as our identity in probe/subscribe messages. `0` => random.
    pub virtual_did: u64,
}

/// Push messages emitted by subscribed devices, plus liveness changes.
#[derive(Clone, Debug)]
pub enum LanPushEvent {
    PropertiesChanged {
        did: String,
        siid: i64,
        piid: i64,
        value: Value,
    },
    EventOccurred {
        did: String,
        siid: i64,
        eiid: i64,
        arguments: Value,
    },
    DeviceState {
        did: String,
        online: bool,
        push_available: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DevState {
    Fresh,
    Ping1,
    Ping2,
    Ping3,
    Dead,
}

struct Device {
    did_u64: u64,
    token: [u8; 16],
    ip: Option<String>,
    /// `unix_now - device_timestamp`, learned from every packet we receive.
    offset: i64,
    state: DevState,
    /// When the next keepalive transition is due, and which state to enter then.
    ka_at: Option<Instant>,
    ka_target: DevState,
    ka_interval: f64,
    online: bool,
    online_history: Vec<(Instant, bool)>,
    online_resume_at: Option<Instant>,
    subscribed: bool,
    sub_ts: u32,
    supported_wildcard_sub: bool,
}

impl Device {
    fn new(info: &LanDeviceInfo) -> Result<Self> {
        let did_u64: u64 = info
            .did
            .trim()
            .parse()
            .map_err(|_| anyhow!("did is not a uint64: {}", info.did))?;
        let token = parse_token(&info.token)?;
        Ok(Self {
            did_u64,
            token,
            ip: info.ip.clone(),
            offset: 0,
            state: DevState::Dead,
            ka_at: None,
            ka_target: DevState::Dead,
            ka_interval: KA_INTERVAL_MIN,
            online: false,
            online_history: Vec::new(),
            online_resume_at: None,
            subscribed: false,
            sub_ts: 0,
            supported_wildcard_sub: false,
        })
    }
}

enum PendingKind {
    /// A caller is blocked on this request; deliver the result/error.
    External(Sender<Result<Value>>),
    /// `miIO.sub` reply; mark the device subscribed on success.
    Subscribe(u32),
    /// `miIO.unsub` reply; nothing to do but consume.
    Unsubscribe,
}

struct Pending {
    did: String,
    kind: PendingKind,
    deadline: Instant,
}

enum Command {
    Request {
        did: String,
        method: String,
        params: Value,
        reply: Sender<Result<Value>>,
        timeout: Duration,
    },
    UpdateDevices(Vec<LanDeviceInfo>),
    DeleteDevices(Vec<String>),
    SetEnableSubscribe(bool),
    Stop,
}

/// State shared between the public handle and the loop thread.
struct LanShared {
    cmd_tx: Sender<Command>,
    wake_sock: UdpSocket,
    wake_port: u16,
    /// Set of currently-online DIDs, written by the loop thread.
    online: Arc<Mutex<HashSet<String>>>,
    push_rx: Mutex<Option<Receiver<LanPushEvent>>>,
    stop: Arc<AtomicBool>,
    join: Mutex<Option<JoinHandle<()>>>,
}

/// Handle to a running LAN manager. Cheap to clone; clones share the thread.
#[derive(Clone)]
pub struct LanManager {
    shared: Arc<LanShared>,
}

impl LanManager {
    /// Start the background discovery/control loop.
    pub fn start(config: LanConfig) -> Result<Self> {
        let socket = UdpSocket::bind(("0.0.0.0", 0))?;
        socket.set_broadcast(true)?;
        let wake_port = socket.local_addr()?.port();

        let wake_sock = UdpSocket::bind(("127.0.0.1", 0))?;
        let (cmd_tx, cmd_rx) = channel::<Command>();
        let (push_tx, push_rx) = channel::<LanPushEvent>();
        let stop = Arc::new(AtomicBool::new(false));

        let virtual_did = if config.virtual_did != 0 {
            config.virtual_did
        } else {
            rand::thread_rng().gen::<u64>()
        };

        let loop_stop = stop.clone();
        let online = Arc::new(Mutex::new(HashSet::new()));
        let loop_online = online.clone();
        let join = std::thread::Builder::new()
            .name("miot_lan".to_string())
            .spawn(move || {
                let mut lan = LanLoop::new(
                    socket,
                    config.enable_subscribe,
                    virtual_did,
                    push_tx,
                    loop_online,
                    loop_stop,
                );
                lan.run(cmd_rx);
            })?;

        Ok(Self {
            shared: Arc::new(LanShared {
                cmd_tx,
                wake_sock,
                wake_port,
                online,
                push_rx: Mutex::new(Some(push_rx)),
                stop,
                join: Mutex::new(Some(join)),
            }),
        })
    }

    /// Register or refresh devices to track (token/model/optional IP).
    pub fn update_devices(&self, devices: Vec<LanDeviceInfo>) {
        let _ = self.shared.cmd_tx.send(Command::UpdateDevices(devices));
        self.wake();
    }

    /// Stop tracking the given DIDs.
    pub fn delete_devices(&self, dids: Vec<String>) {
        let _ = self.shared.cmd_tx.send(Command::DeleteDevices(dids));
        self.wake();
    }

    /// Toggle push subscription at runtime.
    pub fn set_enable_subscribe(&self, enable: bool) {
        let _ = self.shared.cmd_tx.send(Command::SetEnableSubscribe(enable));
        self.wake();
    }

    /// Whether the device is currently considered online on the LAN.
    pub fn is_online(&self, did: &str) -> bool {
        self.shared
            .online
            .lock()
            .map(|set| set.contains(did))
            .unwrap_or(false)
    }

    /// Snapshot of currently-online DIDs.
    pub fn online_dids(&self) -> Vec<String> {
        self.shared
            .online
            .lock()
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Take the single push receiver (only the first caller gets it).
    pub fn take_push_receiver(&self) -> Option<Receiver<LanPushEvent>> {
        self.shared.push_rx.lock().ok().and_then(|mut g| g.take())
    }

    /// Send a MIoT-spec request to a device over the LAN and await the reply.
    ///
    /// Returns the `result` payload (mirroring [`crate::miio_local::MiioUdpClient::request`])
    /// so this is a drop-in replacement on the call site.
    pub fn request(
        &self,
        did: &str,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value> {
        if !self.is_online(did) {
            bail!("lan device offline: {did}");
        }
        let (tx, rx) = channel::<Result<Value>>();
        self.shared
            .cmd_tx
            .send(Command::Request {
                did: did.to_string(),
                method: method.to_string(),
                params,
                reply: tx,
                timeout,
            })
            .map_err(|_| anyhow!("lan loop stopped"))?;
        self.wake();
        match rx.recv_timeout(timeout + Duration::from_millis(500)) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => bail!("lan request timed out: {did}"),
            Err(RecvTimeoutError::Disconnected) => bail!("lan loop stopped"),
        }
    }

    /// Stop the loop and join its thread.
    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        let _ = self.shared.cmd_tx.send(Command::Stop);
        self.wake();
        if let Ok(mut guard) = self.shared.join.lock() {
            if let Some(handle) = guard.take() {
                let _ = handle.join();
            }
        }
    }

    fn wake(&self) {
        let _ = self
            .shared
            .wake_sock
            .send_to(&[0u8], ("127.0.0.1", self.shared.wake_port));
    }
}

// ---------------------------------------------------------------------------
// Loop
// ---------------------------------------------------------------------------

struct LanLoop {
    socket: UdpSocket,
    probe: [u8; OT_PROBE_LEN],
    virtual_did: u64,
    enable_subscribe: bool,
    devices: HashMap<String, Device>,
    pending: HashMap<u32, Pending>,
    dedup: HashMap<String, Instant>,
    msg_id: u32,
    scan_at: Instant,
    scan_interval: f64,
    broadcast_addrs: Vec<Ipv4Addr>,
    online: Arc<Mutex<HashSet<String>>>,
    push_tx: Sender<LanPushEvent>,
    stop: Arc<AtomicBool>,
}

impl LanLoop {
    fn new(
        socket: UdpSocket,
        enable_subscribe: bool,
        virtual_did: u64,
        push_tx: Sender<LanPushEvent>,
        online: Arc<Mutex<HashSet<String>>>,
        stop: Arc<AtomicBool>,
    ) -> Self {
        Self {
            socket,
            probe: build_probe(virtual_did),
            virtual_did,
            enable_subscribe,
            devices: HashMap::new(),
            pending: HashMap::new(),
            dedup: HashMap::new(),
            msg_id: rand::thread_rng().gen_range(1..0x7FFF_FFFF),
            scan_at: Instant::now(),
            scan_interval: SCAN_INTERVAL_MIN,
            broadcast_addrs: broadcast_targets(),
            online,
            push_tx,
            stop,
        }
    }

    fn run(&mut self, cmd_rx: Receiver<Command>) {
        let mut buf = vec![0u8; OT_MSG_LEN];
        loop {
            if self.stop.load(Ordering::SeqCst) {
                break;
            }
            // 1. Drain queued commands.
            loop {
                match cmd_rx.try_recv() {
                    Ok(Command::Stop) => return,
                    Ok(cmd) => self.handle_command(cmd),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return,
                }
            }

            let now = Instant::now();
            // 2. Periodic broadcast scan.
            if now >= self.scan_at {
                self.scan();
                self.scan_interval = (self.scan_interval * 2.0).min(SCAN_INTERVAL_MAX);
                self.scan_at = now + Duration::from_secs_f64(self.scan_interval);
            }
            // 3. Keepalive transitions + online-resume timers.
            self.process_device_timers(now);
            // 4. Expire pending requests.
            self.expire_pending(now);
            // 5. Dedup cleanup.
            self.dedup.retain(|_, expiry| *expiry > now);

            // 6. Block until the next deadline or an inbound packet.
            let timeout = self.next_timeout(now);
            let _ = self.socket.set_read_timeout(Some(timeout));
            match self.socket.recv_from(&mut buf) {
                Ok((n, src)) => self.handle_raw(&buf[..n], src),
                Err(err)
                    if matches!(
                        err.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(_) => {
                    // Transient socket error; avoid a hot spin.
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        }
    }

    fn next_timeout(&self, now: Instant) -> Duration {
        let mut next = now + LOOP_TICK_MAX;
        next = next.min(self.scan_at);
        for dev in self.devices.values() {
            if let Some(at) = dev.ka_at {
                next = next.min(at);
            }
            if let Some(at) = dev.online_resume_at {
                next = next.min(at);
            }
        }
        for p in self.pending.values() {
            next = next.min(p.deadline);
        }
        next.saturating_duration_since(now)
            .max(Duration::from_millis(1))
    }

    fn handle_command(&mut self, cmd: Command) {
        match cmd {
            Command::Request {
                did,
                method,
                params,
                reply,
                timeout,
            } => self.handle_request(did, method, params, reply, timeout),
            Command::UpdateDevices(list) => {
                for info in list {
                    self.upsert_device(info);
                }
            }
            Command::DeleteDevices(dids) => {
                for did in dids {
                    if self.devices.remove(&did).is_some() {
                        self.mark_online_set(&did, false);
                    }
                }
            }
            Command::SetEnableSubscribe(enable) => {
                if enable != self.enable_subscribe {
                    self.enable_subscribe = enable;
                    if !enable {
                        let dids: Vec<String> = self
                            .devices
                            .iter()
                            .filter(|(_, d)| d.subscribed)
                            .map(|(k, _)| k.clone())
                            .collect();
                        for did in dids {
                            self.unsubscribe(&did);
                        }
                    }
                }
            }
            Command::Stop => {}
        }
    }

    fn upsert_device(&mut self, info: LanDeviceInfo) {
        match self.devices.get_mut(&info.did) {
            Some(dev) => {
                if let Ok(token) = parse_token(&info.token) {
                    if token != dev.token {
                        dev.token = token;
                    }
                }
                if dev.ip.is_none() {
                    dev.ip = info.ip.clone();
                }
            }
            None => {
                if let Ok(dev) = Device::new(&info) {
                    self.devices.insert(info.did.clone(), dev);
                }
            }
        }
    }

    fn handle_request(
        &mut self,
        did: String,
        method: String,
        params: Value,
        reply: Sender<Result<Value>>,
        timeout: Duration,
    ) {
        let Some(dev) = self.devices.get(&did) else {
            let _ = reply.send(Err(anyhow!("lan device unknown: {did}")));
            return;
        };
        if dev.ip.is_none() || !dev.online {
            let _ = reply.send(Err(anyhow!("lan device offline: {did}")));
            return;
        }
        let msg_id = self.next_msg_id();
        let msg = json!({
            "id": msg_id,
            "from": "mit",
            "method": method,
            "params": params,
        });
        if let Err(err) = self.send_to_device(&did, &msg) {
            let _ = reply.send(Err(err));
            return;
        }
        self.pending.insert(
            msg_id,
            Pending {
                did,
                kind: PendingKind::External(reply),
                deadline: Instant::now() + timeout,
            },
        );
    }

    /// Build and transmit an encrypted packet to a known device.
    fn send_to_device(&self, did: &str, msg: &Value) -> Result<()> {
        let dev = self
            .devices
            .get(did)
            .ok_or_else(|| anyhow!("lan device unknown: {did}"))?;
        let ip = dev
            .ip
            .as_deref()
            .ok_or_else(|| anyhow!("lan device has no ip: {did}"))?;
        let stamp = (unix_secs() - dev.offset) as u32;
        let packet = gen_packet(dev.did_u64, &dev.token, stamp, msg)?;
        self.socket.send_to(&packet, (ip, OT_PORT))?;
        Ok(())
    }

    fn scan(&self) {
        for addr in &self.broadcast_addrs {
            let _ = self.socket.send_to(&self.probe, (*addr, OT_PORT));
        }
    }

    fn process_device_timers(&mut self, now: Instant) {
        let due: Vec<(String, DevState)> = self
            .devices
            .iter()
            .filter_map(|(did, dev)| {
                dev.ka_at
                    .filter(|at| now >= *at)
                    .map(|_| (did.clone(), dev.ka_target))
            })
            .collect();
        for (did, target) in due {
            self.transition(&did, target, now);
        }

        let resume_due: Vec<String> = self
            .devices
            .iter()
            .filter_map(|(did, dev)| {
                dev.online_resume_at
                    .filter(|at| now >= *at)
                    .map(|_| did.clone())
            })
            .collect();
        for did in resume_due {
            if let Some(dev) = self.devices.get_mut(&did) {
                dev.online_resume_at = None;
            }
            self.set_online(&did, true);
        }
    }

    fn expire_pending(&mut self, now: Instant) {
        let expired: Vec<u32> = self
            .pending
            .iter()
            .filter(|(_, p)| now >= p.deadline)
            .map(|(id, _)| *id)
            .collect();
        for id in expired {
            if let Some(p) = self.pending.remove(&id) {
                if let PendingKind::External(reply) = p.kind {
                    let _ = reply.send(Err(anyhow!("lan request timeout: {}", p.did)));
                }
            }
        }
    }

    // --- keepalive state machine -------------------------------------------

    /// Perform a state transition (mirrors `__update_keep_alive(state)`).
    fn transition(&mut self, did: &str, new_state: DevState, now: Instant) {
        let (send_ping_ip, change_online): (Option<String>, Option<bool>) = {
            let Some(dev) = self.devices.get_mut(did) else {
                return;
            };
            let last = dev.state;
            dev.state = new_state;
            match new_state {
                DevState::Fresh => {
                    let online = if last == DevState::Dead {
                        dev.ka_interval = KA_INTERVAL_MIN;
                        Some(true)
                    } else {
                        None
                    };
                    dev.ka_interval = (dev.ka_interval * 2.0).min(KA_INTERVAL_MAX);
                    dev.ka_at = Some(now + Duration::from_secs_f64(randomize(dev.ka_interval, 0.1)));
                    dev.ka_target = DevState::Ping1;
                    (None, online)
                }
                DevState::Ping1 | DevState::Ping2 | DevState::Ping3 => {
                    dev.ka_at = Some(now + FAST_PING_INTERVAL);
                    dev.ka_target = match new_state {
                        DevState::Ping1 => DevState::Ping2,
                        DevState::Ping2 => DevState::Ping3,
                        _ => DevState::Dead,
                    };
                    (dev.ip.clone(), None)
                }
                DevState::Dead => {
                    let online = if last == DevState::Ping3 {
                        dev.ka_interval = KA_INTERVAL_MIN;
                        Some(false)
                    } else {
                        None
                    };
                    dev.ka_at = None;
                    (None, online)
                }
            }
        };
        if let Some(ip) = send_ping_ip {
            let _ = self.socket.send_to(&self.probe, (ip.as_str(), OT_PORT));
        }
        if let Some(online) = change_online {
            self.change_online(did, online, now);
        }
    }

    /// Force a device to FRESH on hearing from it (mirrors `keep_alive`).
    fn keep_alive(&mut self, did: &str, ip: String, now: Instant) {
        if let Some(dev) = self.devices.get_mut(did) {
            dev.ip = Some(ip);
        }
        self.transition(did, DevState::Fresh, now);
    }

    /// Online change with the unstable-network damper (mirrors `__change_online`).
    fn change_online(&mut self, did: &str, online: bool, now: Instant) {
        let delay = {
            let Some(dev) = self.devices.get_mut(did) else {
                return;
            };
            dev.online_history.push((now, online));
            if dev.online_history.len() > NETWORK_UNSTABLE_CNT_TH {
                dev.online_history.remove(0);
            }
            dev.online_resume_at = None;
            if !online {
                false
            } else {
                let stable = dev.online_history.len() < NETWORK_UNSTABLE_CNT_TH
                    || now
                        .saturating_duration_since(dev.online_history[0].0)
                        .as_secs_f64()
                        > NETWORK_UNSTABLE_TIME_TH;
                if stable {
                    false
                } else {
                    dev.online_resume_at =
                        Some(now + Duration::from_secs_f64(NETWORK_UNSTABLE_RESUME_TH));
                    true
                }
            }
        };
        if !delay {
            self.set_online(did, online);
        }
    }

    fn set_online(&mut self, did: &str, online: bool) {
        let (changed, push_available) = {
            let Some(dev) = self.devices.get_mut(did) else {
                return;
            };
            if dev.online == online {
                (false, dev.subscribed)
            } else {
                dev.online = online;
                (true, dev.subscribed)
            }
        };
        if changed {
            self.mark_online_set(did, online);
            let _ = self.push_tx.send(LanPushEvent::DeviceState {
                did: did.to_string(),
                online,
                push_available,
            });
        }
    }

    fn mark_online_set(&self, did: &str, online: bool) {
        if let Ok(mut set) = self.online.lock() {
            if online {
                set.insert(did.to_string());
            } else {
                set.remove(did);
            }
        }
    }

    // --- subscription ------------------------------------------------------

    fn subscribe(&mut self, did: &str) {
        let msg_id = self.next_msg_id();
        let sub_ts = unix_secs() as u32;
        let msg = json!({
            "id": msg_id,
            "method": "miIO.sub",
            "params": {
                "version": "2.0",
                "did": self.virtual_did.to_string(),
                "update_ts": sub_ts,
                "sub_method": ".",
            }
        });
        if self.send_to_device(did, &msg).is_ok() {
            self.pending.insert(
                msg_id,
                Pending {
                    did: did.to_string(),
                    kind: PendingKind::Subscribe(sub_ts),
                    deadline: Instant::now() + Duration::from_millis(5000),
                },
            );
        }
    }

    fn unsubscribe(&mut self, did: &str) {
        let sub_ts = self.devices.get(did).map(|d| d.sub_ts).unwrap_or(0);
        let msg_id = self.next_msg_id();
        let msg = json!({
            "id": msg_id,
            "method": "miIO.unsub",
            "params": {
                "version": "2.0",
                "did": self.virtual_did.to_string(),
                "update_ts": sub_ts,
                "sub_method": ".",
            }
        });
        if self.send_to_device(did, &msg).is_ok() {
            self.pending.insert(
                msg_id,
                Pending {
                    did: did.to_string(),
                    kind: PendingKind::Unsubscribe,
                    deadline: Instant::now() + Duration::from_millis(5000),
                },
            );
        }
        if let Some(dev) = self.devices.get_mut(did) {
            dev.subscribed = false;
        }
    }

    // --- inbound -----------------------------------------------------------

    fn handle_raw(&mut self, data: &[u8], src: SocketAddr) {
        if data.len() < 16 || data[0..2] != OT_HEADER {
            // Wake packets and junk land here.
            return;
        }
        let SocketAddr::V4(v4) = src else { return };
        let src_ip = v4.ip().to_string();
        let did_u64 = u64::from_be_bytes(data[4..12].try_into().unwrap());
        let did = did_u64.to_string();
        if !self.devices.contains_key(&did) {
            return;
        }
        let now = Instant::now();
        let timestamp = u32::from_be_bytes(data[12..16].try_into().unwrap());
        if let Some(dev) = self.devices.get_mut(&did) {
            dev.offset = unix_secs() - timestamp as i64;
        }

        let subscribed = self.devices.get(&did).map(|d| d.subscribed).unwrap_or(false);
        if data.len() == OT_PROBE_LEN || subscribed {
            self.keep_alive(&did, src_ip, now);
        }

        // Subscription capability advertisement in probe replies.
        if self.enable_subscribe
            && data.len() == OT_PROBE_LEN
            && &data[16..20] == b"MSUB"
            && &data[24..27] == b"PUB"
        {
            let supported = data[28] == OT_SUPPORT_WILDCARD_SUB;
            let sub_ts = u32::from_be_bytes(data[20..24].try_into().unwrap());
            let sub_type = data[27];
            let need_sub = {
                let dev = self.devices.get_mut(&did).unwrap();
                dev.supported_wildcard_sub = supported;
                supported && matches!(sub_type, 0 | 1 | 4) && sub_ts != dev.sub_ts
            };
            if need_sub {
                if let Some(dev) = self.devices.get_mut(&did) {
                    dev.subscribed = false;
                }
                self.subscribe(&did);
            }
        }

        if data.len() > OT_PROBE_LEN {
            let token = self.devices.get(&did).map(|d| d.token);
            if let Some(token) = token {
                if let Ok(msg) = decrypt_packet(&token, data) {
                    self.handle_message(&did, msg);
                }
            }
        }
    }

    fn handle_message(&mut self, did: &str, msg: Value) {
        let Some(id) = msg.get("id").and_then(Value::as_u64) else {
            return;
        };
        let id = id as u32;

        if let Some(pending) = self.pending.remove(&id) {
            match pending.kind {
                PendingKind::External(reply) => {
                    let result = if msg.get("error").is_some() {
                        Err(anyhow!("lan device error: {msg}"))
                    } else {
                        Ok(msg.get("result").cloned().unwrap_or(msg))
                    };
                    let _ = reply.send(result);
                }
                PendingKind::Subscribe(sub_ts) => {
                    let ok = msg
                        .pointer("/result/code")
                        .and_then(Value::as_i64)
                        .map(|c| c == 0)
                        .unwrap_or(false);
                    if ok {
                        if let Some(dev) = self.devices.get_mut(did) {
                            dev.subscribed = true;
                            dev.sub_ts = sub_ts;
                        }
                        let push_available = true;
                        let online = self.devices.get(did).map(|d| d.online).unwrap_or(false);
                        let _ = self.push_tx.send(LanPushEvent::DeviceState {
                            did: did.to_string(),
                            online,
                            push_available,
                        });
                    }
                }
                PendingKind::Unsubscribe => {}
            }
            return;
        }

        // Uplink (device-initiated) message: requires method + params.
        let (Some(method), Some(params)) = (
            msg.get("method").and_then(Value::as_str),
            msg.get("params"),
        ) else {
            return;
        };

        // Filter duplicates within a short window; still ack.
        if self.filter_dup(did, id) {
            self.ack(did, id);
            return;
        }

        match method {
            "properties_changed" => {
                if let Some(items) = params.as_array() {
                    for param in items {
                        let (Some(siid), Some(piid)) = (
                            param.get("siid").and_then(Value::as_i64),
                            param.get("piid").and_then(Value::as_i64),
                        ) else {
                            continue;
                        };
                        let _ = self.push_tx.send(LanPushEvent::PropertiesChanged {
                            did: did.to_string(),
                            siid,
                            piid,
                            value: param.get("value").cloned().unwrap_or(Value::Null),
                        });
                    }
                }
            }
            "event_occured" => {
                if let (Some(siid), Some(eiid)) = (
                    params.get("siid").and_then(Value::as_i64),
                    params.get("eiid").and_then(Value::as_i64),
                ) {
                    let _ = self.push_tx.send(LanPushEvent::EventOccurred {
                        did: did.to_string(),
                        siid,
                        eiid,
                        arguments: params.get("arguments").cloned().unwrap_or(Value::Null),
                    });
                }
            }
            _ => {}
        }
        self.ack(did, id);
    }

    fn ack(&self, did: &str, id: u32) {
        let msg = json!({"id": id, "result": {"code": 0}});
        let _ = self.send_to_device(did, &msg);
    }

    fn filter_dup(&mut self, did: &str, id: u32) -> bool {
        let key = format!("{did}.{id}");
        if self.dedup.contains_key(&key) {
            return true;
        }
        self.dedup.insert(key, Instant::now() + DEDUP_WINDOW);
        false
    }

    fn next_msg_id(&mut self) -> u32 {
        self.msg_id = self.msg_id.wrapping_add(1);
        if self.msg_id == 0 || self.msg_id > 0x8000_0000 {
            self.msg_id = 1;
        }
        self.msg_id
    }
}

// ---------------------------------------------------------------------------
// Packet codec
// ---------------------------------------------------------------------------

/// 32-byte broadcast discovery probe carrying our virtual DID after the `MDID` tag.
pub fn build_probe(virtual_did: u64) -> [u8; OT_PROBE_LEN] {
    let mut p = [0u8; OT_PROBE_LEN];
    p[0] = 0x21;
    p[1] = 0x31;
    p[2] = 0x00;
    p[3] = 0x20; // length = 32
    for byte in p.iter_mut().take(16).skip(4) {
        *byte = 0xFF;
    }
    p[16..20].copy_from_slice(b"MDID");
    p[20..28].copy_from_slice(&virtual_did.to_be_bytes());
    // p[28..32] left as zero
    p
}

/// Build an encrypted command packet using the real 8-byte DID and a clock stamp.
pub fn gen_packet(did: u64, token: &[u8; 16], stamp: u32, clear: &Value) -> Result<Vec<u8>> {
    let body = serde_json::to_vec(clear)?;
    let encrypted = encrypt_payload(token, &body)?;
    let data_len = OT_HEADER_LEN + encrypted.len();
    if data_len > OT_MSG_LEN {
        bail!("lan packet too long: {data_len}");
    }
    let mut pkt = vec![0u8; data_len];
    pkt[0..2].copy_from_slice(&OT_HEADER);
    pkt[2..4].copy_from_slice(&(data_len as u16).to_be_bytes());
    pkt[4..12].copy_from_slice(&did.to_be_bytes());
    pkt[12..16].copy_from_slice(&stamp.to_be_bytes());
    pkt[16..32].copy_from_slice(token); // md5 placeholder
    pkt[32..].copy_from_slice(&encrypted);
    let digest = md5::compute(&pkt);
    pkt[16..32].copy_from_slice(&digest.0);
    Ok(pkt)
}

/// Verify the trailer MD5 and decrypt a device reply into JSON.
pub fn decrypt_packet(token: &[u8; 16], data: &[u8]) -> Result<Value> {
    if data.len() < OT_HEADER_LEN {
        bail!("lan packet too short");
    }
    let data_len = u16::from_be_bytes([data[2], data[3]]) as usize;
    if data_len < OT_HEADER_LEN || data_len > data.len() {
        bail!("lan packet length invalid");
    }
    let md5_orig = &data[16..32];
    let mut check = data[..data_len].to_vec();
    check[16..32].copy_from_slice(token);
    let digest = md5::compute(&check);
    if md5_orig != digest.0 {
        bail!("lan packet md5 mismatch");
    }
    let decrypted = decrypt_payload(token, &data[OT_HEADER_LEN..data_len])?;
    let trimmed = strip_trailing_nulls(&decrypted);
    Ok(serde_json::from_slice(trimmed)?)
}

const OT_HEADER_LEN: usize = 32;

fn strip_trailing_nulls(data: &[u8]) -> &[u8] {
    let mut end = data.len();
    while end > 0 && data[end - 1] == 0 {
        end -= 1;
    }
    &data[..end]
}

fn parse_token(raw: &str) -> Result<[u8; 16]> {
    let hex = raw.trim();
    if hex.len() != 32 {
        bail!("token must be 32 hex chars");
    }
    let mut out = [0u8; 16];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|_| anyhow!("token is not valid hex"))?;
    }
    Ok(out)
}

fn unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn randomize(value: f64, pct: f64) -> f64 {
    let factor = 1.0 + rand::thread_rng().gen_range(-pct..=pct);
    (value * factor).max(0.0)
}

/// IPv4 broadcast targets: the limited broadcast plus each interface's directed
/// broadcast (so multi-homed / Docker bridge setups are reached too).
fn broadcast_targets() -> Vec<Ipv4Addr> {
    let mut out = vec![Ipv4Addr::BROADCAST];
    for addr in local_directed_broadcasts() {
        if !out.contains(&addr) {
            out.push(addr);
        }
    }
    out
}

#[cfg(unix)]
fn local_directed_broadcasts() -> Vec<Ipv4Addr> {
    let mut out = Vec::new();
    unsafe {
        let mut ifap: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut ifap) != 0 {
            return out;
        }
        let mut cur = ifap;
        while !cur.is_null() {
            let ifa = &*cur;
            cur = ifa.ifa_next;
            if ifa.ifa_addr.is_null() || ifa.ifa_netmask.is_null() {
                continue;
            }
            if (*ifa.ifa_addr).sa_family as i32 != libc::AF_INET {
                continue;
            }
            if (ifa.ifa_flags as i32 & libc::IFF_LOOPBACK) != 0 {
                continue;
            }
            let addr_in = &*(ifa.ifa_addr as *const libc::sockaddr_in);
            let mask_in = &*(ifa.ifa_netmask as *const libc::sockaddr_in);
            let addr = u32::from_be(addr_in.sin_addr.s_addr);
            let mask = u32::from_be(mask_in.sin_addr.s_addr);
            if addr == 0 || mask == 0 {
                continue;
            }
            out.push(Ipv4Addr::from(addr | !mask));
        }
        libc::freeifaddrs(ifap);
    }
    out
}

#[cfg(not(unix))]
fn local_directed_broadcasts() -> Vec<Ipv4Addr> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token() -> [u8; 16] {
        [
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD,
            0xEE, 0xFF,
        ]
    }

    #[test]
    fn probe_has_expected_layout() {
        let probe = build_probe(0x0102_0304_0506_0708);
        assert_eq!(&probe[0..4], &[0x21, 0x31, 0x00, 0x20]);
        assert_eq!(&probe[4..16], &[0xFF; 12]);
        assert_eq!(&probe[16..20], b"MDID");
        assert_eq!(
            &probe[20..28],
            &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]
        );
        assert_eq!(&probe[28..32], &[0, 0, 0, 0]);
    }

    #[test]
    fn packet_round_trips_through_decrypt() {
        let did: u64 = 123_456_789;
        let stamp: u32 = 1_700_000_000;
        let clear = json!({"id": 7, "method": "get_properties", "params": []});
        let packet = gen_packet(did, &token(), stamp, &clear).unwrap();

        // Header layout.
        assert_eq!(&packet[0..2], &OT_HEADER);
        assert_eq!(
            u16::from_be_bytes([packet[2], packet[3]]) as usize,
            packet.len()
        );
        assert_eq!(
            u64::from_be_bytes(packet[4..12].try_into().unwrap()),
            did
        );
        assert_eq!(u32::from_be_bytes(packet[12..16].try_into().unwrap()), stamp);

        let decoded = decrypt_packet(&token(), &packet).unwrap();
        assert_eq!(decoded, clear);
    }

    #[test]
    fn decrypt_rejects_bad_md5() {
        let clear = json!({"id": 1});
        let mut packet = gen_packet(1, &token(), 1, &clear).unwrap();
        packet[16] ^= 0xFF; // corrupt the trailer md5
        assert!(decrypt_packet(&token(), &packet).is_err());
    }

    #[test]
    fn parse_token_validates_length_and_hex() {
        assert!(parse_token("00112233445566778899aabbccddeeff").is_ok());
        assert!(parse_token("  00112233445566778899aabbccddeeff  ").is_ok());
        assert!(parse_token("00112233").is_err()); // too short
        assert!(parse_token("zz112233445566778899aabbccddeeff").is_err()); // non-hex
    }

    #[test]
    fn strip_trailing_nulls_only_trims_the_tail() {
        assert_eq!(strip_trailing_nulls(b"abc\0\0"), b"abc");
        assert_eq!(strip_trailing_nulls(b"a\0b\0"), b"a\0b");
        assert_eq!(strip_trailing_nulls(b""), b"");
    }

    #[test]
    fn broadcast_targets_include_limited_broadcast() {
        assert!(broadcast_targets().contains(&Ipv4Addr::BROADCAST));
    }

    #[test]
    fn manager_lifecycle_handles_offline_device() {
        // Tolerate sandboxes that disallow broadcast sockets.
        let Ok(manager) = LanManager::start(LanConfig::default()) else {
            return;
        };
        manager.update_devices(vec![LanDeviceInfo {
            did: "123456789".to_string(),
            token: "00112233445566778899aabbccddeeff".to_string(),
            model: "test.model.v1".to_string(),
            ip: None,
        }]);
        // An undiscovered device is offline, so requests fail fast (caller falls back).
        assert!(!manager.is_online("123456789"));
        let result = manager.request(
            "123456789",
            "get_properties",
            json!([]),
            Duration::from_millis(300),
        );
        assert!(result.is_err());
        // The loop must stop and join without hanging.
        manager.stop();
    }

    #[test]
    fn decrypt_tolerates_trailing_null_padding() {
        // Some devices append a redundant NUL inside the JSON payload region.
        let clear = json!({"id": 2, "result": {"code": 0}});
        let body = {
            let mut b = serde_json::to_vec(&clear).unwrap();
            b.push(0); // redundant trailing null
            b
        };
        let encrypted = encrypt_payload(&token(), &body).unwrap();
        let data_len = OT_HEADER_LEN + encrypted.len();
        let mut pkt = vec![0u8; data_len];
        pkt[0..2].copy_from_slice(&OT_HEADER);
        pkt[2..4].copy_from_slice(&(data_len as u16).to_be_bytes());
        pkt[4..12].copy_from_slice(&2u64.to_be_bytes());
        pkt[16..32].copy_from_slice(&token());
        pkt[32..].copy_from_slice(&encrypted);
        let digest = md5::compute(&pkt);
        pkt[16..32].copy_from_slice(&digest.0);

        let decoded = decrypt_packet(&token(), &pkt).unwrap();
        assert_eq!(decoded, clear);
    }
}
