use aes::Aes128;
use anyhow::{anyhow, bail, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use rsa::{pkcs8::DecodePublicKey, Pkcs1v15Encrypt, RsaPublicKey};

type Aes128CbcEnc = cbc::Encryptor<Aes128>;
type Aes128CbcDec = cbc::Decryptor<Aes128>;
use rand::{rngs::OsRng, RngCore};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use url::Url;

use crate::miot_lan::{is_on_local_subnet, LanConfig, LanDeviceInfo, LanManager, LanPushEvent};
use crate::storage::{get_account_dir, get_home_dir, AuthAccount, UserProfile, DEFAULT_REGION};

const PROJECT_CODE: &str = "mico";
pub const OAUTH_CLIENT_ID: &str = "2882303761520431603";
const OAUTH_AUTH_URL: &str = "https://account.xiaomi.com/oauth2/authorize";
const OAUTH_API_HOST_DEFAULT: &str = "mico.api.mijia.tech";
const USER_PROFILE_URL: &str = "https://open.account.xiaomi.com/user/profile";
const MICO_BASE_URL_ENV: &str = "MIT_MICO_BASE_URL";
const USER_PROFILE_URL_ENV: &str = "MIT_USER_PROFILE_URL";
const TOKEN_EXPIRES_RATIO: f64 = 0.7;
/// Per-attempt socket timeout for a direct LAN (miIO handshake) request. miIO
/// devices answer on the LAN in tens of ms, so a short timeout keeps both the
/// retry-after-packet-loss case and offline/stale-IP detection cheap before any
/// cloud fallback.
const LOCAL_LAN_TIMEOUT: Duration = Duration::from_millis(250);
/// LAN requests are retried this many times on transient UDP errors (packet loss
/// on the private network is common; a single attempt is not reliable).
const LAN_MAX_ATTEMPTS: usize = 3;
/// Max properties per local `get_properties` request. Many miio devices cap
/// their UDP response near 1 KB and silently truncate larger ones (yielding
/// unparseable JSON), so we chunk to keep each response well under that.
const LOCAL_PROPS_CHUNK_SIZE: usize = 4;
const MIOT_FLOW_LOG_FILE: &str = "miot-flow.log";
/// Persisted set of props each device does not serve over local miIO, so we can
/// skip them on LAN without re-probing on every run.
const LAN_UNAVAILABLE_FILE: &str = "lan_unavailable.json";
// Xiaomi OAuth endpoints currently reject requests unless this legacy Python UA is used.
const USER_AGENT: &str = "Python/3.12 aiohttp/3.13.3";
const API_USER_AGENT: &str = "mico/docker";

const PUBLIC_KEY_PEM: &str = "-----BEGIN PUBLIC KEY-----\n\
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAzH220YGgZOlXJ4eSleFb\n\
Beylq4qHsVNzhPTUTy/caDb4a3GzqH6SX4GiYRilZZZrjjU2ckkr8GM66muaIuJw\n\
r8ZB9SSY3Hqwo32tPowpyxobTN1brmqGK146X6JcFWK/QiUYVXZlcHZuMgXLlWyn\n\
zTMVl2fq7wPbzZwOYFxnSRh8YEnXz6edHAqJqLEqZMP00bNFBGP+yc9xmc7ySSyw\n\
OgW/muVzfD09P2iWhl3x8N+fBBWpuI5HjvyQuiX8CZg3xpEeCV8weaprxMxR0epM\n\
3l7T6rJuPXR1D7yhHaEQj2+dyrZTeJO8D8SnOgzV5j4bp1dTunlzBXGYVjqDsRhZ\n\
qQIDAQAB\n\
-----END PUBLIC KEY-----";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallbackInput {
    pub code: String,
    pub state: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_ts: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PushNotificationResult {
    pub notify_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub did: String,
    pub name: String,
    pub model: String,
    pub online: bool,
    /// MiHome `connect_type` (the device-list API's `pid` field): the device's
    /// connection protocol (WiFi / BLE-Mesh / third-party cloud, …). Cached
    /// per-`did`; defaults to `-1` (unknown) for caches written before this
    /// field existed.
    #[serde(default = "default_connect_pid")]
    pub pid: i64,
    pub home_id: String,
    pub home_name: String,
    pub room_id: String,
    pub room_name: String,
}

/// Sentinel `pid` for devices whose connection type is unknown (e.g. loaded
/// from an older cache). Mirrors the official integration's `pid` default.
pub fn default_connect_pid() -> i64 {
    -1
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalCredentialSource {
    Direct,
    RoomGateway,
    HomeGateway,
    FallbackGateway,
}

/// Which transport actually served (or would serve) a device request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MiotChannel {
    /// Direct UDP on the local network (LAN discovery or the legacy handshake).
    Lan,
    /// Xiaomi cloud API.
    Cloud,
}

impl MiotChannel {
    pub fn as_str(self) -> &'static str {
        match self {
            MiotChannel::Lan => "LAN",
            MiotChannel::Cloud => "Cloud",
        }
    }
}

impl std::fmt::Display for MiotChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Whether a device can be reached over the LAN right now, and how.
enum LanEligibility {
    /// On our intranet with a usable credential — LAN can be attempted.
    Eligible(LanTarget),
    /// Not LAN-controllable; `reason` explains why (logged + surfaced under `--LAN`).
    Ineligible(String),
}

/// The credential needed to control a device directly over the LAN.
struct LanTarget {
    ip: String,
    token: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalDeviceCredential {
    pub did: String,
    pub name: String,
    pub model: String,
    pub local_ip: String,
    pub token: String,
    pub source: LocalCredentialSource,
}

#[derive(Clone)]
pub struct MicoClient {
    account_uid: String,
    redirect_uri: String,
    pub device_id: String,
    pub state: String,
    host: String,
    base_url: String,
    user_profile_url: String,
    local_credential_cache: Arc<Mutex<LocalCredentialCache>>,
    last_channel: Arc<Mutex<Option<MiotChannel>>>,
    plain_http_transport: bool,
    access_token: String,
    aes_key: [u8; 16],
    secret_b64: String,
    http: Client,
}

impl MicoClient {
    pub fn new(auth: &AuthAccount) -> Result<Self> {
        let region = normalize_region(&auth.region);
        let redirect_uri = normalize_text(&auth.redirect_uri);
        let uuid = normalize_text(&auth.uuid);
        let device_id = build_device_id(&uuid)?;
        let state = build_state(&device_id);
        let host = build_host(&region);
        let base_url_override = local_http_test_override(MICO_BASE_URL_ENV);
        let user_profile_override = local_http_test_override(USER_PROFILE_URL_ENV);
        let base_url = base_url_override
            .clone()
            .unwrap_or_else(|| format!("https://{host}"));
        let user_profile_url =
            user_profile_override.unwrap_or_else(|| USER_PROFILE_URL.to_string());
        let plain_http_transport = base_url_override.is_some();
        let local_override_transport = plain_http_transport
            || user_profile_url.starts_with("http://127.0.0.1:")
            || user_profile_url.starts_with("http://localhost:");
        let access_token = normalize_text(&auth.access_token);
        let aes_key = random_aes_key();
        let secret_b64 = encrypt_secret(&aes_key)?;
        let client_builder = if local_override_transport {
            Client::builder().no_proxy()
        } else {
            Client::builder()
        };

        Ok(Self {
            account_uid: auth.user.uid.clone(),
            redirect_uri,
            device_id: device_id.clone(),
            state,
            host,
            base_url,
            user_profile_url,
            local_credential_cache: shared_local_credential_cache(&device_id),
            last_channel: Arc::new(Mutex::new(None)),
            plain_http_transport,
            access_token,
            aes_key,
            secret_b64,
            http: client_builder.build()?,
        })
    }

    fn validated_account_uid(&self) -> Result<&str> {
        use std::path::Component;

        let uid = self.account_uid.as_str();
        if uid.trim().is_empty() {
            return Err(anyhow!(
                "account uid is empty; refusing to write per-account local credential snapshot"
            ));
        }
        if uid != uid.trim() {
            return Err(anyhow!(
                "account uid contains leading/trailing whitespace; refusing to write per-account local credential snapshot"
            ));
        }

        let mut components = std::path::Path::new(uid).components();
        match (components.next(), components.next()) {
            (Some(Component::Normal(_)), None) => Ok(uid),
            _ => Err(anyhow!(
                "account uid is not a safe path component: {uid}; refusing to write per-account local credential snapshot"
            )),
        }
    }

    fn local_credentials_path(&self) -> Result<PathBuf> {
        Ok(get_account_dir(self.validated_account_uid()?).join("local_credentials.json"))
    }

    fn write_local_credentials_snapshot(
        &self,
        credentials: &HashMap<String, LocalDeviceCredential>,
    ) -> Result<()> {
        let path = self.local_credentials_path()?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| anyhow!("create {}: {error}", parent.display()))?;
        }
        let mut devices = BTreeMap::new();
        let mut credentials_by_id = BTreeMap::new();
        let mut credential_ids = HashMap::<(String, String), String>::new();
        let mut next_credential_id = 1usize;
        let mut entries = credentials.iter().collect::<Vec<_>>();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        for (did, credential) in entries {
            let local_ip = credential.local_ip.trim().to_string();
            let token = credential.token.trim().to_string();
            if local_ip.is_empty() || token.is_empty() {
                continue;
            }
            let key = (local_ip.clone(), token.clone());
            let credential_id = credential_ids.entry(key).or_insert_with(|| {
                let id = format!("cred-{next_credential_id}");
                next_credential_id += 1;
                credentials_by_id.insert(
                    id.clone(),
                    json!({
                        "localIp": local_ip,
                        "token": token,
                    }),
                );
                id
            });
            devices.insert(did.clone(), Value::String(credential_id.clone()));
        }
        let payload = json!({
            "version": 1,
            "fetchedAt": unix_timestamp(),
            "credentials": credentials_by_id,
            "devices": devices,
        });
        let mut text = serde_json::to_string_pretty(&payload)?;
        text.push('\n');
        crate::storage::write_private_text_file(&path, &text)
            .map_err(|error| anyhow!("write {}: {error}", path.display()))?;
        Ok(())
    }

    pub fn auth_url(&self, skip_confirm: bool) -> String {
        format!(
            "{}?{}",
            OAUTH_AUTH_URL,
            encode_ordered_query(&[
                ("redirect_uri", self.redirect_uri.as_str()),
                ("client_id", OAUTH_CLIENT_ID),
                ("response_type", "code"),
                ("device_id", self.device_id.as_str()),
                ("state", self.state.as_str()),
                ("skip_confirm", python_bool(skip_confirm)),
            ])
        )
    }

    pub fn exchange_code(&mut self, code: &str, state: &str) -> Result<TokenPair> {
        if state != self.state {
            bail!("state 不匹配: expected={} actual={}", self.state, state);
        }
        let payload = ordered_json(&[
            ("client_id", OAUTH_CLIENT_ID),
            ("redirect_uri", self.redirect_uri.as_str()),
            ("code", code),
            ("device_id", self.device_id.as_str()),
        ])?;
        let result = self.get_token(&payload)?;
        self.access_token = result.access_token.clone();
        Ok(result)
    }

    pub fn refresh_token(&mut self, refresh_token: &str) -> Result<TokenPair> {
        let payload = ordered_json(&[
            ("client_id", OAUTH_CLIENT_ID),
            ("redirect_uri", self.redirect_uri.as_str()),
            ("refresh_token", refresh_token),
        ])?;
        let result = self.get_token(&payload)?;
        self.access_token = result.access_token.clone();
        Ok(result)
    }

    pub fn get_user_info(&self) -> Result<UserProfile> {
        if self.access_token.is_empty() {
            bail!("OAuth token 缺失，请先执行 `mit auth login`");
        }
        let url = format!(
            "{}?{}",
            self.user_profile_url,
            encode_ordered_query(&[
                ("clientId", OAUTH_CLIENT_ID),
                ("token", self.access_token.as_str()),
            ])
        );
        let response = self
            .http
            .get(url)
            .header("content-type", "application/x-www-form-urlencoded")
            .header("Connection", "close")
            .send()?;
        let text = response_text(response)?;
        let payload: Value = serde_json::from_str(&text)?;
        let data = payload.get("data").and_then(Value::as_object);
        if value_i64(payload.get("code")) != 0
            || data.is_none()
            || normalize_text_value(data.unwrap().get("unionId")).is_empty()
        {
            bail!("invalid user profile response: {}", text.trim());
        }
        let union_id = normalize_text_value(data.unwrap().get("unionId"));
        let uid = self.post_encrypted(
            "/app/v2/oauth/get_uid_by_unionid",
            &json!({ "union_id": union_id }),
        )?;

        Ok(UserProfile {
            uid: value_to_string(&uid),
            nickname: normalize_text_value(data.unwrap().get("miliaoNick")),
            icon: normalize_text_value(data.unwrap().get("miliaoIcon")),
            union_id,
        })
    }

    pub fn get_devices(&self) -> Result<Vec<Device>> {
        let homes = self.get_homes()?;
        let mut placements = HashMap::new();
        let mut devices = Vec::new();
        let mut seen_dids = HashSet::new();
        for key in ["homelist", "share_home_list"] {
            let Some(list) = homes.get(key).and_then(Value::as_array) else {
                continue;
            };
            for home in list {
                let home_id = normalize_text_value(home.get("id"));
                let home_name = normalize_text_value(home.get("name"));
                let home_owner = home
                    .get("uid")
                    .and_then(Value::as_i64)
                    .or_else(|| self.account_uid.trim().parse::<i64>().ok())
                    .unwrap_or(0);
                let home_id_int = home_id.parse::<i64>().unwrap_or(0);
                let dids = home
                    .get("dids")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                for did in dids {
                    placements.insert(
                        value_to_string(&did),
                        (
                            home_id.clone(),
                            home_name.clone(),
                            home_id.clone(),
                            home_name.clone(),
                        ),
                    );
                }
                let room_list = home
                    .get("roomlist")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                for room in room_list {
                    let room_id = normalize_text_value(room.get("id"));
                    let room_name = normalize_text_value(room.get("name"));
                    let room_dids = room
                        .get("dids")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    for did in room_dids {
                        placements.insert(
                            value_to_string(&did),
                            (
                                home_id.clone(),
                                home_name.clone(),
                                room_id.clone(),
                                room_name.clone(),
                            ),
                        );
                    }
                }

                for info in self.get_home_device_list(home_owner, home_id_int)? {
                    if info.did.is_empty() || !seen_dids.insert(info.did.clone()) {
                        continue;
                    }
                    let placement = placements.get(&info.did).cloned().unwrap_or_else(|| {
                        (
                            home_id.clone(),
                            home_name.clone(),
                            String::new(),
                            String::new(),
                        )
                    });
                    devices.push(Device {
                        did: info.did,
                        name: info.name,
                        model: info.model,
                        online: info.online,
                        pid: info.pid,
                        home_id: placement.0,
                        home_name: placement.1,
                        room_id: placement.2,
                        room_name: placement.3,
                    });
                }
            }
        }

        Ok(devices)
    }

    pub fn get_local_device_credentials(&self) -> Result<Vec<LocalDeviceCredential>> {
        let homes = self.get_homes()?;
        let dids = collect_home_dids(&homes);
        let placements = collect_home_placements(&homes);
        let debug = std::env::var("MIT_LOG_DEVICE_LIST_PAGE_RAW")
            .ok()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);

        let mut devices = Vec::new();
        let mut summaries_by_did: HashMap<String, DeviceSummary> = HashMap::new();
        for batch in dids.chunks(150) {
            let page = self.get_device_list_page(batch, "")?;
            if debug {
                let local_capable = page
                    .values()
                    .filter(|item| !item.local_ip.is_empty() && !item.token.is_empty())
                    .count();
                eprintln!(
                    "[mit] local-cred batch={} page={} local-capable={}",
                    batch.len(),
                    page.len(),
                    local_capable
                );
            }
            let mut matched = 0usize;
            let mut emitted = 0usize;
            for did in batch {
                let Some(info) = page.get(did) else {
                    continue;
                };
                matched += 1;
                if info.local_ip.is_empty() || info.token.is_empty() {
                    continue;
                }
                devices.push(LocalDeviceCredential {
                    did: info.did.clone(),
                    name: info.name.clone(),
                    model: info.model.clone(),
                    local_ip: info.local_ip.clone(),
                    token: info.token.clone(),
                    source: LocalCredentialSource::Direct,
                });
                emitted += 1;
            }
            if debug {
                eprintln!("[mit] local-cred matched={} emitted={}", matched, emitted);
            }
            summaries_by_did.extend(page);
        }

        // Hand every token-bearing device to the LAN manager so background
        // broadcast discovery can locate and keep them alive locally.
        self.register_lan_devices(&summaries_by_did);

        let mut cache = self
            .local_credential_cache
            .lock()
            .map_err(|_| anyhow!("local credential cache lock poisoned"))?;
        cache.fetched_at = unix_timestamp();
        let direct_by_did = devices
            .iter()
            .cloned()
            .map(|item| (item.did.clone(), item))
            .collect::<HashMap<_, _>>();

        let mut room_gateway = HashMap::new();
        let mut home_gateway = HashMap::new();
        for credential in devices.iter().filter(|item| is_gateway_model(&item.model)) {
            if let Some(placement) = placements.get(&credential.did) {
                if !placement.room_id.is_empty() {
                    room_gateway
                        .entry(placement.room_id.clone())
                        .or_insert_with(|| credential.clone());
                }
                if !placement.home_id.is_empty() {
                    home_gateway
                        .entry(placement.home_id.clone())
                        .or_insert_with(|| credential.clone());
                }
            }
        }
        let fallback_gateway = devices
            .iter()
            .find(|item| is_gateway_model(&item.model))
            .cloned();

        let mut resolved_by_did = HashMap::new();
        for did in &dids {
            if let Some(direct) = direct_by_did.get(did) {
                resolved_by_did.insert(did.clone(), direct.clone());
                continue;
            }
            let Some(target) = summaries_by_did.get(did) else {
                continue;
            };
            let Some(placement) = placements.get(did) else {
                continue;
            };
            if !placement.room_id.is_empty() {
                if let Some(proxy) = room_gateway.get(&placement.room_id) {
                    resolved_by_did.insert(
                        did.clone(),
                        routed_credential(target, proxy, LocalCredentialSource::RoomGateway),
                    );
                    continue;
                }
            }
            if !placement.home_id.is_empty() {
                if let Some(proxy) = home_gateway.get(&placement.home_id) {
                    resolved_by_did.insert(
                        did.clone(),
                        routed_credential(target, proxy, LocalCredentialSource::HomeGateway),
                    );
                    continue;
                }
            }
            if let Some(proxy) = &fallback_gateway {
                resolved_by_did.insert(
                    did.clone(),
                    routed_credential(target, proxy, LocalCredentialSource::FallbackGateway),
                );
            }
        }
        if debug {
            eprintln!(
                "[mit] local-cred resolved direct={} routed={}",
                direct_by_did.len(),
                resolved_by_did.len()
            );
        }
        cache.by_did = resolved_by_did
            .iter()
            .map(|(did, credential)| (did.clone(), credential.clone()))
            .collect();
        // No probe step: with subnet-gated routing every resolved credential is
        // "ready"; the same-intranet check at control time decides LAN vs cloud.
        cache.ready_dids = cache.by_did.keys().cloned().collect();
        cache.disabled_dids.clear();
        drop(cache);

        self.write_local_credentials_snapshot(&resolved_by_did)?;

        Ok(devices)
    }

    fn lookup_cached_local_credential(&self, did: &str) -> Result<Option<LocalDeviceCredential>> {
        let cache = self
            .local_credential_cache
            .lock()
            .map_err(|_| anyhow!("local credential cache lock poisoned"))?;
        if cache.fetched_at <= 0 {
            return Ok(None);
        }
        if cache.disabled_dids.contains(did) {
            return Ok(None);
        }
        if !cache.ready_dids.contains(did) {
            return Ok(None);
        }
        Ok(cache.by_did.get(did).cloned())
    }

    fn insert_ready_local_credential(
        cache: &mut LocalCredentialCache,
        did: &str,
        credential: &LocalDeviceCredential,
    ) {
        for key in local_credential_cache_keys(did) {
            cache.disabled_dids.remove(&key);
            cache.ready_dids.insert(key.clone());
            cache.by_did.insert(key, credential.clone());
        }
    }

    fn lan_manager(&self) -> Option<LanManager> {
        lan_manager_for_device(&self.device_id)
    }

    /// Register devices (DID + token + optional cloud IP) with the LAN manager so
    /// it can broadcast-discover them locally. The cloud IP is only a hint —
    /// discovery learns the live address even when the cloud value is stale or
    /// missing. Non-numeric DIDs (sub-devices behind a gateway) are skipped.
    fn register_lan_devices(&self, summaries: &HashMap<String, DeviceSummary>) {
        let Some(lan) = self.lan_manager() else {
            return;
        };
        let infos: Vec<LanDeviceInfo> = summaries
            .values()
            .filter(|summary| !summary.token.trim().is_empty())
            .filter(|summary| {
                let did = summary.did.trim();
                !did.is_empty() && did.chars().all(|c| c.is_ascii_digit())
            })
            .map(|summary| {
                let ip = summary.local_ip.trim();
                LanDeviceInfo {
                    did: summary.did.clone(),
                    token: summary.token.trim().to_string(),
                    model: summary.model.clone(),
                    ip: (!ip.is_empty()).then(|| ip.to_string()),
                }
            })
            .collect();
        if !infos.is_empty() {
            lan.update_devices(infos);
        }
    }

    /// Prime the local fast path for a single device so one-shot CLI commands can
    /// use LAN. First reuses the credential snapshot the TUI persisted to
    /// `~/.mit/accounts/<uid>/local_credentials.json` (no cloud round-trip); only
    /// if that is missing or unreachable does it refresh from the cloud. The
    /// credential is enabled only after a ~200ms probe confirms the device answers
    /// on the LAN, so remote invocations fall straight through to the cloud.
    pub fn prime_local_credential_for(&self, did: &str) -> Result<()> {
        let requested_did = did.trim().to_string();
        if requested_did.is_empty() {
            return Ok(());
        }
        let did = prop_request_did(&requested_did);

        // Already confirmed reachable in this process (e.g. by startup hydration);
        // reuse it without another probe so repeated reads stay cheap.
        if matches!(self.lookup_cached_local_credential(&did), Ok(Some(_))) {
            log_miot_flow(
                "prime",
                "prime",
                &format!("did={requested_did} request_did={did} reason=already-ready"),
            );
            return Ok(());
        }
        if did != requested_did {
            if let Ok(Some(credential)) = self.lookup_cached_local_credential(&requested_did) {
                if let Ok(mut cache) = self.local_credential_cache.lock() {
                    Self::insert_ready_local_credential(&mut cache, &did, &credential);
                }
                log_miot_flow(
                    "prime",
                    "prime",
                    &format!("did={requested_did} request_did={did} reason=aliased-ready"),
                );
                return Ok(());
            }
        }

        let mut snapshot_credential = self.load_snapshot_credential(&requested_did)?;
        if snapshot_credential.is_none() && did != requested_did {
            snapshot_credential = self.load_snapshot_credential(&did)?;
        }

        // 1. Reuse the persisted snapshot synced by the TUI.
        if let Some(credential) = snapshot_credential {
            self.register_lan_credential(&did, &credential);
            if self.prime_local_credential(&did, credential, "snapshot") {
                return Ok(());
            }
            // Snapshot entry was stale/unreachable; refresh from cloud below.
        }

        // 2. Refresh this single device from the cloud.
        let page = match self.get_device_list_page(std::slice::from_ref(&did), "") {
            Ok(page) => page,
            Err(error) => {
                log_miot_flow(
                    "prime",
                    "prime",
                    &format!("did={did} stage=cloud-fetch error={error}"),
                );
                return Ok(());
            }
        };
        self.register_lan_devices(&page);
        let Some(summary) = page.get(&did) else {
            log_miot_flow(
                "prime",
                "prime",
                &format!(
                    "did={did} reason=not-in-device-list (cloud returned no entry for this did)"
                ),
            );
            return Ok(());
        };
        let ip = summary.local_ip.trim();
        let token = summary.token.trim();
        if ip.is_empty() {
            log_miot_flow(
                "prime",
                "prime",
                &format!(
                    "did={did} reason=cloud-localip-empty token_present={} model={} \
                     (cloud did not report a LAN IP; need broadcast discovery)",
                    !token.is_empty(),
                    summary.model
                ),
            );
            return Ok(());
        }
        if token.is_empty() {
            log_miot_flow(
                "prime",
                "prime",
                &format!("did={did} reason=token-empty local_ip={ip}"),
            );
            return Ok(());
        }
        let credential = LocalDeviceCredential {
            did: summary.did.clone(),
            name: summary.name.clone(),
            model: summary.model.clone(),
            local_ip: ip.to_string(),
            token: token.to_string(),
            source: LocalCredentialSource::Direct,
        };
        self.prime_local_credential(&did, credential, "cloud");
        Ok(())
    }

    /// Hydrate the in-memory credential cache from the persisted snapshot — the
    /// same cache `get_local_device_credentials` builds, but sourced from disk
    /// with **no cloud round-trip**. All devices are registered with the LAN
    /// manager for discovery and marked ready; the same-intranet check at control
    /// time decides LAN vs cloud per request (no startup probe). Lets a fresh
    /// process (e.g. a TUI relaunch that skips the cloud sync) use LAN immediately.
    /// Returns the count of credentials whose IP is on a local subnet.
    pub fn hydrate_local_credentials_from_snapshot(&self) -> Result<usize> {
        let creds = self.load_all_snapshot_credentials()?;
        if creds.is_empty() {
            return Ok(0);
        }
        // Register everything with the LAN manager so on-demand / broadcast
        // discovery can locate the devices (and recover if a snapshot IP changed).
        if let Some(lan) = self.lan_manager() {
            let infos: Vec<LanDeviceInfo> = creds
                .iter()
                .filter(|(did, cred)| {
                    did.chars().all(|c| c.is_ascii_digit()) && !cred.token.trim().is_empty()
                })
                .map(|(did, cred)| {
                    let ip = cred.local_ip.trim();
                    LanDeviceInfo {
                        did: did.clone(),
                        token: cred.token.trim().to_string(),
                        model: cred.model.clone(),
                        ip: (!ip.is_empty()).then(|| ip.to_string()),
                    }
                })
                .collect();
            if !infos.is_empty() {
                lan.update_devices(infos);
            }
        }
        let total = creds.len();
        let mut on_subnet = 0usize;
        {
            let mut cache = self
                .local_credential_cache
                .lock()
                .map_err(|_| anyhow!("local credential cache lock poisoned"))?;
            if cache.fetched_at <= 0 {
                cache.fetched_at = unix_timestamp();
            }
            for (did, cred) in &creds {
                if cred
                    .local_ip
                    .trim()
                    .parse::<std::net::Ipv4Addr>()
                    .map(is_on_local_subnet)
                    .unwrap_or(false)
                {
                    on_subnet += 1;
                }
                for key in local_credential_cache_keys(did) {
                    cache.by_did.entry(key.clone()).or_insert_with(|| cred.clone());
                    cache.ready_dids.insert(key);
                }
            }
        }
        log_miot_flow(
            "hydrate",
            "prime",
            &format!("source=snapshot devices={total} on_subnet={on_subnet}"),
        );
        Ok(on_subnet)
    }

    /// Read every `{did → localIp, token}` from the persisted snapshot.
    fn load_all_snapshot_credentials(&self) -> Result<Vec<(String, LocalDeviceCredential)>> {
        let path = self.local_credentials_path()?;
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(_) => return Ok(Vec::new()),
        };
        let snapshot: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        let (Some(devices), Some(creds)) = (
            snapshot.get("devices").and_then(Value::as_object),
            snapshot.get("credentials").and_then(Value::as_object),
        ) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for (did, cred_id) in devices {
            let Some(entry) = cred_id.as_str().and_then(|id| creds.get(id)) else {
                continue;
            };
            let local_ip = entry
                .get("localIp")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let token = entry
                .get("token")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            if local_ip.is_empty() || token.is_empty() {
                continue;
            }
            out.push((
                did.clone(),
                LocalDeviceCredential {
                    did: did.clone(),
                    name: String::new(),
                    model: String::new(),
                    local_ip,
                    token,
                    source: LocalCredentialSource::Direct,
                },
            ));
        }
        Ok(out)
    }

    /// Load a single device's `{localIp, token}` from the persisted snapshot the
    /// TUI writes, if present and complete.
    fn load_snapshot_credential(&self, did: &str) -> Result<Option<LocalDeviceCredential>> {
        let path = self.local_credentials_path()?;
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(_) => return Ok(None),
        };
        let snapshot: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        let Some(cred_id) = snapshot
            .get("devices")
            .and_then(|devices| devices.get(did))
            .and_then(Value::as_str)
        else {
            return Ok(None);
        };
        let Some(entry) = snapshot
            .get("credentials")
            .and_then(|creds| creds.get(cred_id))
        else {
            return Ok(None);
        };
        let local_ip = entry
            .get("localIp")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let token = entry
            .get("token")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if local_ip.is_empty() || token.is_empty() {
            return Ok(None);
        }
        Ok(Some(LocalDeviceCredential {
            did: did.to_string(),
            name: String::new(),
            model: String::new(),
            local_ip,
            token,
            source: LocalCredentialSource::Direct,
        }))
    }

    /// Register one device (DID + token + IP) with the LAN manager for discovery.
    fn register_lan_credential(&self, did: &str, credential: &LocalDeviceCredential) {
        if !did.chars().all(|c| c.is_ascii_digit()) || credential.token.trim().is_empty() {
            return;
        }
        if let Some(lan) = self.lan_manager() {
            let ip = credential.local_ip.trim();
            lan.update_devices(vec![LanDeviceInfo {
                did: did.to_string(),
                token: credential.token.trim().to_string(),
                model: credential.model.clone(),
                ip: (!ip.is_empty()).then(|| ip.to_string()),
            }]);
        }
    }

    /// Seed the in-memory cache with a credential and mark it ready. Returns
    /// whether the device's IP is on a local subnet (i.e. LAN is plausible); the
    /// caller uses `false` to decide whether to refresh a (likely stale) IP from
    /// the cloud. The actual LAN-vs-cloud routing is decided per request.
    fn prime_local_credential(
        &self,
        did: &str,
        credential: LocalDeviceCredential,
        source: &str,
    ) -> bool {
        let ip = credential.local_ip.trim().to_string();
        let on_subnet = ip
            .parse::<std::net::Ipv4Addr>()
            .map(is_on_local_subnet)
            .unwrap_or(false);
        if let Ok(mut cache) = self.local_credential_cache.lock() {
            if cache.fetched_at <= 0 {
                cache.fetched_at = unix_timestamp();
            }
            Self::insert_ready_local_credential(&mut cache, did, &credential);
        }
        log_miot_flow(
            "prime",
            "prime",
            &format!("did={did} ip={ip} source={source} on_subnet={on_subnet} ready=true"),
        );
        on_subnet
    }

    /// Take the LAN push-event receiver (only the first caller gets it). Lets the
    /// UI consume `properties_changed` / `event_occured` pushes from subscribed
    /// devices and online-state changes.
    pub fn lan_push_receiver(&self) -> Option<Receiver<LanPushEvent>> {
        self.lan_manager()?.take_push_receiver()
    }

    /// Whether the device is currently reachable via direct LAN discovery.
    pub fn lan_is_online(&self, did: &str) -> bool {
        self.lan_manager()
            .map(|manager| manager.is_online(did))
            .unwrap_or(false)
    }

    /// Decide whether `did` can be controlled over the LAN right now: it needs a
    /// usable credential (token + IP) whose IP is on one of our local subnets
    /// (the "same intranet" check). That IP + token is what the direct miIO
    /// handshake in [`Self::lan_execute`] talks to.
    fn lan_eligibility(&self, did: &str) -> LanEligibility {
        if !lan_discovery_enabled() {
            return LanEligibility::Ineligible("lan-disabled".to_string());
        }
        let credential = match self.lookup_cached_local_credential(did) {
            Ok(Some(credential)) => credential,
            Ok(None) => return LanEligibility::Ineligible("no-credential".to_string()),
            Err(error) => {
                return LanEligibility::Ineligible(format!("credential-error error={error}"))
            }
        };
        let ip = credential.local_ip.trim();
        let token = credential.token.trim();
        if ip.is_empty() || token.is_empty() {
            return LanEligibility::Ineligible("no-credential".to_string());
        }
        let Ok(parsed) = ip.parse::<std::net::Ipv4Addr>() else {
            return LanEligibility::Ineligible(format!("bad-ip ip={ip}"));
        };
        if !is_on_local_subnet(parsed) {
            return LanEligibility::Ineligible(format!("off-subnet ip={ip}"));
        }
        LanEligibility::Eligible(LanTarget {
            ip: ip.to_string(),
            token: token.to_string(),
        })
    }

    /// Run one request over the LAN. Uses the classic miIO hello-handshake to the
    /// device's on-subnet IP — the single control transport, which reaches both
    /// standalone Wi-Fi devices and gateway-proxied sub-devices (the OT
    /// `LanManager` is kept only for opt-in push subscriptions). Transient UDP
    /// errors (packet loss on the private LAN) are retried a few times.
    fn lan_execute(&self, target: &LanTarget, method: &str, params: &Value) -> Result<Value> {
        let mut last_error = None;
        for attempt in 1..=LAN_MAX_ATTEMPTS {
            match crate::miot_lan::lan_request_direct(
                &target.ip,
                &target.token,
                method,
                params,
                LOCAL_LAN_TIMEOUT,
            ) {
                Ok(value) => return Ok(value),
                Err(error) => {
                    if attempt < LAN_MAX_ATTEMPTS && is_transient_lan_error(&error) {
                        last_error = Some(error);
                        continue;
                    }
                    return Err(error);
                }
            }
        }
        Err(last_error.unwrap_or_else(|| anyhow!("lan request failed")))
    }

    /// The single control router for set/action: detect intranet → try LAN →
    /// fall back to cloud, unless `--LAN` forces LAN with no fallback.
    fn route_control(
        &self,
        op: &str,
        did: &str,
        lan_method: &str,
        lan_params: Value,
        cloud: impl FnOnce() -> Result<Value>,
    ) -> Result<Value> {
        let forced = force_lan_enabled();
        let mode = if forced { "forced-lan" } else { "auto" };

        if forced {
            return match self.lan_eligibility(did) {
                LanEligibility::Eligible(target) => {
                    self.record_channel(MiotChannel::Lan);
                    match self.lan_execute(&target, lan_method, &lan_params) {
                        Ok(value) => {
                            log_route(op, did, mode, "LAN", "forced", "ok", None);
                            Ok(value)
                        }
                        Err(error) => {
                            log_route(op, did, mode, "LAN", "forced", "err", Some(&error));
                            Err(anyhow!("--LAN: LAN control failed for {did}: {error}"))
                        }
                    }
                }
                LanEligibility::Ineligible(reason) => {
                    log_route(op, did, mode, "none", &reason, "err", None);
                    Err(anyhow!("--LAN: {did} not controllable over LAN: {reason}"))
                }
            };
        }

        // AUTO: LAN → cloud.
        match self.lan_eligibility(did) {
            LanEligibility::Eligible(target) => {
                match self.lan_execute(&target, lan_method, &lan_params) {
                    Ok(value) => {
                        self.record_channel(MiotChannel::Lan);
                        log_route(op, did, mode, "LAN", "eligible", "ok", None);
                        Ok(value)
                    }
                    Err(error) => {
                        log_route(op, did, mode, "LAN", "attempt", "err", Some(&error));
                        self.route_cloud(op, did, mode, "lan-failed", cloud)
                    }
                }
            }
            LanEligibility::Ineligible(reason) => self.route_cloud(op, did, mode, &reason, cloud),
        }
    }

    fn route_cloud(
        &self,
        op: &str,
        did: &str,
        mode: &str,
        reason: &str,
        cloud: impl FnOnce() -> Result<Value>,
    ) -> Result<Value> {
        self.record_channel(MiotChannel::Cloud);
        match cloud() {
            Ok(value) => {
                log_route(op, did, mode, "Cloud", reason, "ok", None);
                Ok(value)
            }
            Err(error) => {
                log_route(op, did, mode, "Cloud", reason, "err", Some(&error));
                Err(error)
            }
        }
    }

    fn record_channel(&self, channel: MiotChannel) {
        if let Ok(mut last) = self.last_channel.lock() {
            *last = Some(channel);
        }
    }

    /// The transport used by the most recent single-device request, if known.
    pub fn last_channel(&self) -> Option<MiotChannel> {
        self.last_channel.lock().ok().and_then(|guard| *guard)
    }

    pub fn set_volume(&self, did: &str, volume: u8) -> Result<()> {
        self.set_prop(did, 2, 1, json!(volume))?;
        Ok(())
    }

    pub fn get_prop(&self, did: &str, siid: i64, piid: i64) -> Result<Value> {
        let result = self.get_props_batch(&[(did, siid, piid)])?;
        Ok(result
            .as_array()
            .and_then(|items| items.first())
            .cloned()
            .unwrap_or(Value::Null))
    }

    /// Read a device's properties over the LAN, returning one entry per `item`
    /// (aligned): `Some(value)` when LAN served it, `None` when it should fall
    /// back to cloud.
    ///
    /// Chunks share a single handshake. A property that is in the cloud spec but
    /// not in the device's local miIO implementation makes the device **drop the
    /// whole `get_properties` request** (a timeout), which would otherwise poison
    /// every property in its chunk. So when a chunk fails we re-probe its props
    /// one-at-a-time: the genuinely-local ones still resolve over LAN, and the
    /// ones that time out alone are remembered as cloud-only for this device, so
    /// future reads skip them and stop poisoning chunks.
    fn lan_get_props(
        &self,
        did: &str,
        target: &LanTarget,
        items: &[(usize, i64, i64)],
        mode: &str,
    ) -> Vec<Option<Value>> {
        let n = items.len();
        let mut out: Vec<Option<Value>> = vec![None; n];

        // Skip props already known to be unavailable over LAN for this model
        // (shared across all devices of the model).
        let cache_key = lan_cache_key(did);
        let unavailable = lan_unavailable_snapshot(&cache_key);
        let candidates: Vec<usize> = (0..n)
            .filter(|&i| !unavailable.contains(&(items[i].1, items[i].2)))
            .collect();
        if candidates.is_empty() {
            return out;
        }

        // First pass: one handshake, chunked `get_properties` for all candidates.
        let chunks: Vec<&[usize]> = candidates.chunks(LOCAL_PROPS_CHUNK_SIZE).collect();
        let requests: Vec<(&str, Value)> = chunks
            .iter()
            .map(|chunk| {
                let params: Vec<Value> = chunk
                    .iter()
                    .map(|&i| json!({"did": did, "siid": items[i].1, "piid": items[i].2}))
                    .collect();
                ("get_properties", Value::Array(params))
            })
            .collect();
        // A successful handshake here proves the device is reachable, even if
        // every command fails — so it's safe to isolate failed props below.
        let Some(results) = self.lan_batch_with_retry(target, &requests) else {
            log_route("get", did, mode, "Cloud", "lan-unreachable", "pending", None);
            return out;
        };
        self.record_channel(MiotChannel::Lan);
        for (chunk, res) in chunks.iter().zip(results) {
            if let Ok(value) = res {
                let values = value.as_array();
                for (pos, &i) in chunk.iter().enumerate() {
                    // A negative `code` means the device doesn't serve this prop
                    // locally (cloud may still have it), so leave it unresolved.
                    if let Some(v) = values.and_then(|a| a.get(pos)) {
                        if !is_error_with_negative_code(v) {
                            out[i] = Some(v.clone());
                        }
                    }
                }
            }
        }
        log_route(
            "get",
            did,
            mode,
            "LAN",
            &format!("resolved={}/{}", candidates.iter().filter(|&&i| out[i].is_some()).count(), candidates.len()),
            "ok",
            None,
        );

        // The device is reachable (handshake succeeded); isolate any props the
        // batch didn't resolve, one at a time.
        let missing: Vec<usize> = candidates
            .iter()
            .copied()
            .filter(|&i| out[i].is_none())
            .collect();
        if !missing.is_empty() {
            let probe: Vec<(&str, Value)> = missing
                .iter()
                .map(|&i| {
                    (
                        "get_properties",
                        Value::Array(vec![json!({"did": did, "siid": items[i].1, "piid": items[i].2})]),
                    )
                })
                .collect();
            if let Some(results) = self.lan_batch_with_retry(target, &probe) {
                for (&i, res) in missing.iter().zip(results) {
                    // A timeout (`Err`) is transient (device busy / packet loss),
                    // NOT a real "unavailable": leave it unresolved and do NOT
                    // poison the cache, or a momentary glitch would permanently
                    // mark a perfectly good prop as cloud-only.
                    let Ok(v) = res else { continue };
                    match v.as_array().and_then(|a| a.first().cloned()) {
                        Some(v) if !is_error_with_negative_code(&v) => out[i] = Some(v),
                        // A negative `code` (e.g. -404101 "not found") is a
                        // *definitive* answer that the device doesn't serve this
                        // prop locally — only then do we remember it.
                        _ => {
                            mark_lan_unavailable(&cache_key, items[i].1, items[i].2);
                            log_route(
                                "get",
                                did,
                                mode,
                                "Cloud",
                                &format!("lan-unavailable {}.{}", items[i].1, items[i].2),
                                "pending",
                                None,
                            );
                        }
                    }
                }
            }
        }
        out
    }

    /// Send a batch of requests over one LAN handshake, retrying the handshake on
    /// transient errors. `None` means the device is unreachable (handshake never
    /// succeeded); `Some` carries one per-request result.
    fn lan_batch_with_retry(
        &self,
        target: &LanTarget,
        requests: &[(&str, Value)],
    ) -> Option<Vec<Result<Value>>> {
        for attempt in 1..=LAN_MAX_ATTEMPTS {
            match crate::miot_lan::lan_request_batch(
                &target.ip,
                &target.token,
                requests,
                LOCAL_LAN_TIMEOUT,
            ) {
                Ok(results) => return Some(results),
                Err(error) => {
                    if attempt < LAN_MAX_ATTEMPTS && is_transient_lan_error(&error) {
                        continue;
                    }
                    return None;
                }
            }
        }
        None
    }

    pub fn get_props_batch(&self, params: &[(&str, i64, i64)]) -> Result<Value> {
        self.get_props_batch_inner(params, false)
    }

    /// Like [`Self::get_props_batch`] but a prop the device can't serve over LAN
    /// (a known cloud-only prop) gets the "not available over LAN" placeholder
    /// instead of a slow cloud round-trip. Used by the live prop dialog, which
    /// prioritises fast local reads over fetching static cloud-only metadata.
    pub fn get_props_batch_local(&self, params: &[(&str, i64, i64)]) -> Result<Value> {
        self.get_props_batch_inner(params, true)
    }

    fn get_props_batch_inner(
        &self,
        params: &[(&str, i64, i64)],
        mark_cloud_only: bool,
    ) -> Result<Value> {
        let forced = force_lan_enabled();
        let mode = if forced { "forced-lan" } else { "auto" };

        let mut grouped: HashMap<String, Vec<(usize, i64, i64)>> = HashMap::new();
        for (index, (did, siid, piid)) in params.iter().enumerate() {
            grouped
                .entry(prop_request_did(did))
                .or_default()
                .push((index, *siid, *piid));
        }

        let mut output = vec![Value::Null; params.len()];
        let mut unresolved_indices = Vec::new();
        for (did, items) in grouped {
            // Resolve LAN eligibility once per device (same intranet + credential).
            let target = match self.lan_eligibility(&did) {
                LanEligibility::Eligible(target) => Some(target),
                LanEligibility::Ineligible(reason) => {
                    if forced {
                        return Err(anyhow!("--LAN: {did} not controllable over LAN: {reason}"));
                    }
                    log_route("get", &did, mode, "Cloud", &reason, "pending", None);
                    None
                }
            };

            let prop_values = match target.as_ref() {
                Some(target) => self.lan_get_props(&did, target, &items, mode),
                None => vec![None; items.len()],
            };
            // Props this device can't serve over LAN (learned + persisted). Under
            // `--LAN`, or when the caller opted into local-only reads, we surface a
            // placeholder for these instead of erroring or doing a slow cloud read.
            let unavailable = if forced || mark_cloud_only {
                lan_unavailable_snapshot(&lan_cache_key(&did))
            } else {
                HashSet::new()
            };
            for ((index, siid, piid), value) in items.iter().zip(prop_values) {
                match value {
                    Some(value) => output[*index] = value,
                    None if unavailable.contains(&(*siid, *piid)) => {
                        // Known cloud-only prop → placeholder, never a cloud read.
                        output[*index] = lan_unsupported_marker(&did, *siid, *piid);
                    }
                    // Live dialog under --LAN: a transient LAN miss for a
                    // normally-good prop. Don't error and don't cloud-fetch;
                    // leaving it null lets the dialog keep the prop's previous value.
                    None if mark_cloud_only && forced => {}
                    // Dialog in AUTO, or any non-forced read: fall through to cloud.
                    None if mark_cloud_only => unresolved_indices.push(*index),
                    None if forced => {
                        return Err(anyhow!("--LAN: LAN read failed for {did}"));
                    }
                    None => unresolved_indices.push(*index),
                }
            }
        }

        if forced {
            // No cloud fallback under --LAN: a complete LAN response, or an error.
            if unresolved_indices.is_empty() {
                return Ok(Value::Array(output));
            }
            return Err(anyhow!(
                "--LAN: incomplete LAN response ({} unresolved property/properties)",
                unresolved_indices.len()
            ));
        }

        if unresolved_indices.is_empty() {
            return Ok(Value::Array(output));
        }

        unresolved_indices.sort_unstable();
        unresolved_indices.dedup();
        let fallback_params = unresolved_indices
            .iter()
            .map(|index| {
                let (did, siid, piid) = params[*index];
                json!({
                    "did": prop_request_did(did),
                    "siid": siid,
                    "piid": piid,
                })
            })
            .collect::<Vec<_>>();
        let fallback = self.post_encrypted(
            "/app/v2/miotspec/prop/get",
            &json!({ "params": fallback_params }),
        )?;
        log_route(
            "get",
            "batch",
            mode,
            "Cloud",
            &format!("items={}", unresolved_indices.len()),
            "ok",
            None,
        );
        if let Some(values) = fallback.as_array() {
            for (position, index) in unresolved_indices.iter().enumerate() {
                if let Some(value) = values.get(position) {
                    output[*index] = value.clone();
                }
            }
        }

        let retry_indices = output
            .iter()
            .enumerate()
            .filter_map(|(index, value)| {
                let (did, _, _) = params[index];
                let root_did = root_did_for_sub_device(did)?;
                if is_error_with_negative_code(value) {
                    Some((index, root_did))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        if !retry_indices.is_empty() {
            let retry_params = retry_indices
                .iter()
                .map(|(index, root_did)| {
                    let (_, siid, piid) = params[*index];
                    json!({
                        "did": root_did,
                        "siid": siid,
                        "piid": piid,
                    })
                })
                .collect::<Vec<_>>();
            let retry = self.post_encrypted(
                "/app/v2/miotspec/prop/get",
                &json!({ "params": retry_params }),
            )?;
            if let Some(values) = retry.as_array() {
                for (position, (index, _)) in retry_indices.iter().enumerate() {
                    if let Some(value) = values.get(position) {
                        output[*index] = value.clone();
                    }
                }
            }
        }
        Ok(Value::Array(output))
    }

    pub fn set_prop(&self, did: &str, siid: i64, piid: i64, value: Value) -> Result<Value> {
        let did = prop_request_did(did);
        let entry = json!({
            "did": did,
            "siid": siid,
            "piid": piid,
            "value": value,
        });
        let lan_params = Value::Array(vec![entry.clone()]);
        let cloud_body = json!({ "params": [entry] });
        self.route_control("set", &did, "set_properties", lan_params, || {
            self.post_encrypted("/app/v2/miotspec/prop/set", &cloud_body)
        })
    }

    pub fn action(&self, did: &str, siid: i64, aiid: i64, values: &[Value]) -> Result<Value> {
        let did = prop_request_did(did);
        let lan_params = json!({
            "did": did,
            "siid": siid,
            "aiid": aiid,
            "in": values,
        });
        let cloud_body = json!({
            "params": {
                "did": did,
                "siid": siid,
                "aiid": aiid,
                "in": values,
            }
        });
        self.route_control("action", &did, "action", lan_params, || {
            self.post_encrypted("/app/v2/miotspec/action", &cloud_body)
        })
    }

    pub fn create_app_notification(&self, text: &str) -> Result<String> {
        let content = normalize_text(text);
        if content.is_empty() {
            bail!("push 内容不能为空");
        }
        let result = self.post_encrypted("/app/v2/oauth/save_text", &json!({ "text": content }))?;
        let notify_id = value_to_string(&result).trim().to_string();
        if notify_id.is_empty() {
            bail!("创建推送通知失败：响应缺少通知 ID");
        }
        Ok(notify_id)
    }

    pub fn send_app_notification(&self, notify_id: &str) -> Result<bool> {
        let key = normalize_text(notify_id);
        if key.is_empty() {
            bail!("notify id 不能为空");
        }
        let result = self.post_encrypted("/app/v2/oauth/send_push", &json!({ "key": key }))?;
        match result.as_bool() {
            Some(true) => Ok(true),
            Some(false) => bail!("发送推送通知失败"),
            None => bail!("发送推送通知失败：返回结果无效={result}"),
        }
    }

    pub fn push_notification(&self, text: &str) -> Result<PushNotificationResult> {
        let notify_id = self.create_app_notification(text)?;
        self.send_app_notification(&notify_id)?;
        Ok(PushNotificationResult { notify_id })
    }

    fn get_token(&self, raw_data: &str) -> Result<TokenPair> {
        let url = format!(
            "{}/app/v2/{}/oauth/get_token?{}",
            self.base_url,
            PROJECT_CODE,
            encode_ordered_query(&[("data", raw_data)])
        );
        let response = self
            .http
            .get(url)
            .header("content-type", "application/x-www-form-urlencoded")
            .header("Accept", "*/*")
            .header("Accept-Encoding", "gzip, deflate")
            .header("User-Agent", USER_AGENT)
            .header("Connection", "close")
            .send()?;
        let status = response.status();
        let text = response.text()?;
        let payload = serde_json::from_str::<Value>(&text).ok();
        if !status.is_success() {
            bail!(
                "token exchange failed: status={} body={}",
                status.as_u16(),
                text.trim()
            );
        }

        let result = payload
            .as_ref()
            .and_then(|body| body.get("result"))
            .and_then(Value::as_object);
        let access_token = result
            .and_then(|body| body.get("access_token"))
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default();
        let refresh_token = result
            .and_then(|body| body.get("refresh_token"))
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default();

        if payload
            .as_ref()
            .map(|body| value_i64(body.get("code")))
            .unwrap_or(-1)
            != 0
            || access_token.is_empty()
            || refresh_token.is_empty()
        {
            bail!("invalid token response: {}", text.trim());
        }

        let expires_in = result
            .and_then(|body| body.get("expires_in"))
            .map(|value| value_i64(Some(value)))
            .unwrap_or(0);
        Ok(TokenPair {
            access_token: access_token.to_string(),
            refresh_token: refresh_token.to_string(),
            expires_ts: (unix_timestamp() as f64 + expires_in as f64 * TOKEN_EXPIRES_RATIO).floor()
                as i64,
        })
    }

    fn post_encrypted(&self, path: &str, payload: &Value) -> Result<Value> {
        if self.access_token.is_empty() {
            bail!("OAuth token 缺失，请先执行 `mit auth login`");
        }
        if self.plain_http_transport {
            let body = serde_json::to_string(payload)?;
            let response = self
                .http
                .post(format!("{}{}", self.base_url, path))
                .header("Content-Type", "application/json")
                .body(body)
                .send()?;
            let status = response.status();
            let text = response.text()?;
            if !status.is_success() {
                bail!(
                    "miot api failed: status={} body={}",
                    status.as_u16(),
                    text.trim()
                );
            }
            let envelope: Value = serde_json::from_str(&text)?;
            if value_i64(envelope.get("code")) != 0 {
                bail!(
                    "miot api error: code={} message={}",
                    value_i64(envelope.get("code")),
                    normalize_text_value(envelope.get("message"))
                );
            }
            return Ok(envelope.get("result").cloned().unwrap_or(Value::Null));
        }
        let encrypted_body = self.encrypt_payload(payload)?;
        let response = self
            .http
            .post(format!("{}{}", self.base_url, path))
            .headers(self.request_headers())
            .body(encrypted_body)
            .send()?;
        let status = response.status();
        let text = response.text()?;
        if !status.is_success() {
            bail!(
                "miot api failed: status={} body={}",
                status.as_u16(),
                text.trim()
            );
        }
        let envelope = self.decrypt_payload(&text)?;
        if value_i64(envelope.get("code")) != 0 {
            bail!(
                "miot api error: code={} message={}",
                value_i64(envelope.get("code")),
                normalize_text_value(envelope.get("message"))
            );
        }
        Ok(envelope.get("result").cloned().unwrap_or(Value::Null))
    }

    fn get_homes(&self) -> Result<Value> {
        self.post_encrypted(
            "/app/v2/homeroom/gethome",
            &json!({
                "limit": 150,
                "fetch_share": true,
                "fetch_share_dev": true,
                "plat_form": 0,
                "app_ver": 9,
            }),
        )
    }

    fn get_device_list_page(
        &self,
        dids: &[String],
        start_did: &str,
    ) -> Result<HashMap<String, DeviceSummary>> {
        let debug = std::env::var("MIT_LOG_DEVICE_LIST_PAGE_RAW")
            .ok()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);
        let mut next_start_did = normalize_text(start_did);
        let mut seen_start_dids = HashSet::new();
        let mut devices = HashMap::new();
        loop {
            let mut payload = json!({
                "limit": 200,
                "get_split_device": true,
                "dids": dids,
            });
            if !next_start_did.trim().is_empty() {
                if !seen_start_dids.insert(next_start_did.clone()) {
                    break;
                }
                payload["start_did"] = Value::String(next_start_did.clone());
            }

            let result = self.post_encrypted("/app/v2/home/device_list_page", &payload)?;
            if debug {
                eprintln!(
                    "[mit] device_list_page start_did={} raw={}",
                    next_start_did, result
                );
            }
            if let Some(list) = result.get("list").and_then(Value::as_array) {
                for raw in list {
                    let did = normalize_text_value(raw.get("did"));
                    let model = normalize_text_value(raw.get("model"));
                    if did.is_empty() || model.is_empty() {
                        continue;
                    }
                    devices.insert(
                        did.clone(),
                        DeviceSummary {
                            did,
                            name: normalize_text_value(raw.get("name")),
                            model,
                            online: raw
                                .get("isOnline")
                                .and_then(Value::as_bool)
                                .unwrap_or(false),
                            pid: raw
                                .get("pid")
                                .and_then(Value::as_i64)
                                .unwrap_or(default_connect_pid()),
                            local_ip: normalize_text_value(raw.get("localip")),
                            token: normalize_text_value(raw.get("token")),
                        },
                    );
                }
            }

            let has_more = result
                .get("has_more")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let candidate = normalize_text_value(result.get("next_start_did"));
            if !has_more || candidate.is_empty() {
                break;
            }
            next_start_did = candidate;
        }
        Ok(devices)
    }

    fn get_home_device_list(&self, home_owner: i64, home_id: i64) -> Result<Vec<DeviceSummary>> {
        let mut start_did = String::new();
        let mut devices = Vec::new();
        loop {
            let result = self.post_encrypted(
                "/app/v2/home/home_device_list",
                &json!({
                    "home_owner": home_owner,
                    "home_id": home_id,
                    "limit": 200,
                    "start_did": start_did,
                    "get_split_device": true,
                    "support_smart_home": true,
                    "get_cariot_device": true,
                    "get_third_device": true,
                }),
            )?;
            if let Some(list) = result.get("device_info").and_then(Value::as_array) {
                for raw in list {
                    let did = normalize_text_value(raw.get("did"));
                    let model = normalize_text_value(raw.get("model"));
                    if did.is_empty() || model.is_empty() {
                        continue;
                    }
                    devices.push(DeviceSummary {
                        did,
                        name: normalize_text_value(raw.get("name")),
                        model,
                        online: raw
                            .get("isOnline")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                        pid: raw
                            .get("pid")
                            .and_then(Value::as_i64)
                            .unwrap_or(default_connect_pid()),
                        local_ip: normalize_text_value(raw.get("localip")),
                        token: normalize_text_value(raw.get("token")),
                    });
                }
            }

            start_did = normalize_text_value(result.get("max_did"));
            let has_more = result
                .get("has_more")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if !has_more || start_did.is_empty() {
                break;
            }
        }
        Ok(devices)
    }

    fn encrypt_payload(&self, payload: &Value) -> Result<String> {
        let serialized = serde_json::to_vec(payload)?;
        // PKCS7 always adds padding; allocate one full extra block
        let padded_len = (serialized.len() / 16 + 1) * 16;
        let mut buf = vec![0u8; padded_len];
        buf[..serialized.len()].copy_from_slice(&serialized);
        let encrypted = Aes128CbcEnc::new_from_slices(&self.aes_key, &self.aes_key)
            .map_err(|e| anyhow!("AES key/IV error: {e}"))?
            .encrypt_padded_mut::<Pkcs7>(&mut buf, serialized.len())
            .map_err(|e| anyhow!("AES encrypt error: {e:?}"))?;
        Ok(STANDARD.encode(encrypted))
    }

    fn decrypt_payload(&self, payload: &str) -> Result<Value> {
        let decoded = STANDARD.decode(payload.trim())?;
        let mut buf = decoded;
        let decrypted = Aes128CbcDec::new_from_slices(&self.aes_key, &self.aes_key)
            .map_err(|e| anyhow!("AES key/IV error: {e}"))?
            .decrypt_padded_mut::<Pkcs7>(&mut buf)
            .map_err(|e| anyhow!("AES decrypt error: {e:?}"))?;
        Ok(serde_json::from_slice(decrypted)?)
    }

    fn request_headers(&self) -> reqwest::header::HeaderMap {
        use reqwest::header::{HeaderMap, HeaderValue};

        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", HeaderValue::from_static("text/plain"));
        headers.insert("User-Agent", HeaderValue::from_static(API_USER_AGENT));
        headers.insert("X-Client-BizId", HeaderValue::from_static("micoapi"));
        headers.insert("X-Encrypt-Type", HeaderValue::from_static("1"));
        headers.insert("X-Client-AppId", HeaderValue::from_static(OAUTH_CLIENT_ID));
        headers.insert(
            "X-Client-Secret",
            HeaderValue::from_str(self.secret_b64.as_str()).expect("valid client secret"),
        );
        headers.insert(
            "Host",
            HeaderValue::from_str(self.host.as_str()).expect("valid host"),
        );
        headers.insert(
            "Authorization",
            HeaderValue::from_str(&format!("Bearer{}", self.access_token))
                .expect("valid authorization"),
        );
        headers.insert("Connection", HeaderValue::from_static("close"));
        headers
    }
}

#[derive(Clone, Debug)]
struct DeviceSummary {
    did: String,
    name: String,
    model: String,
    online: bool,
    pid: i64,
    local_ip: String,
    token: String,
}

#[derive(Clone, Debug, Default)]
struct DevicePlacement {
    home_id: String,
    room_id: String,
}

#[derive(Clone, Debug, Default)]
struct LocalCredentialCache {
    fetched_at: i64,
    by_did: HashMap<String, LocalDeviceCredential>,
    ready_dids: HashSet<String>,
    disabled_dids: HashSet<String>,
}

static LOCAL_CREDENTIAL_CACHES: OnceLock<Mutex<HashMap<String, Arc<Mutex<LocalCredentialCache>>>>> =
    OnceLock::new();

fn shared_local_credential_cache(device_id: &str) -> Arc<Mutex<LocalCredentialCache>> {
    let registry = LOCAL_CREDENTIAL_CACHES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut registry = registry
        .lock()
        .expect("local credential cache registry lock poisoned");
    registry
        .entry(device_id.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(LocalCredentialCache::default())))
        .clone()
}

pub fn has_cached_local_credential_for_device(device_id: &str, did: &str) -> bool {
    let Some(registry) = LOCAL_CREDENTIAL_CACHES.get() else {
        return false;
    };
    let Ok(registry) = registry.lock() else {
        return false;
    };
    let Some(cache) = registry.get(device_id) else {
        return false;
    };
    let Ok(cache) = cache.lock() else {
        return false;
    };
    if cache.fetched_at <= 0 {
        return false;
    }
    if cache.disabled_dids.contains(did) {
        return false;
    }
    cache.ready_dids.contains(did)
}

static LAN_MANAGERS: OnceLock<Mutex<HashMap<String, Option<LanManager>>>> = OnceLock::new();

/// Per-device set of `(siid, piid)` that the device does not serve over local
/// miIO (cloud-only metadata, props missing from its local spec). Learned when a
/// per-prop probe times out alone; used to skip those props on LAN so they go
/// straight to cloud instead of poisoning a whole chunk. Process-global and keyed
/// by root DID (DIDs are globally unique).
type LanUnavailableProps = HashMap<String, HashSet<(i64, i64)>>;
static LAN_UNAVAILABLE_PROPS: OnceLock<Mutex<LanUnavailableProps>> = OnceLock::new();

/// The registry, loaded from disk on first use so a fresh process (one-shot CLI
/// command, TUI relaunch) skips known cloud-only props without re-probing.
fn lan_unavailable_registry() -> &'static Mutex<LanUnavailableProps> {
    LAN_UNAVAILABLE_PROPS.get_or_init(|| Mutex::new(load_lan_unavailable()))
}

fn lan_unavailable_snapshot(did: &str) -> HashSet<(i64, i64)> {
    lan_unavailable_registry()
        .lock()
        .ok()
        .and_then(|map| map.get(did).cloned())
        .unwrap_or_default()
}

fn mark_lan_unavailable(did: &str, siid: i64, piid: i64) {
    if let Ok(mut map) = lan_unavailable_registry().lock() {
        if map.entry(did.to_string()).or_default().insert((siid, piid)) {
            save_lan_unavailable(&map);
        }
    }
}

fn lan_unavailable_path() -> Option<PathBuf> {
    let home = get_home_dir();
    if home.as_os_str().is_empty() {
        return None;
    }
    Some(home.join(".mit").join("cache").join(LAN_UNAVAILABLE_FILE))
}

fn load_lan_unavailable() -> LanUnavailableProps {
    let mut map = LanUnavailableProps::new();
    let Some(path) = lan_unavailable_path() else {
        return map;
    };
    let Ok(text) = fs::read_to_string(&path) else {
        return map;
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return map;
    };
    if let Some(object) = value.as_object() {
        for (did, entries) in object {
            let set: HashSet<(i64, i64)> = entries
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|pair| {
                    let pair = pair.as_array()?;
                    Some((pair.first()?.as_i64()?, pair.get(1)?.as_i64()?))
                })
                .collect();
            if !set.is_empty() {
                map.insert(did.clone(), set);
            }
        }
    }
    map
}

fn save_lan_unavailable(map: &LanUnavailableProps) {
    let Some(path) = lan_unavailable_path() else {
        return;
    };
    let object: serde_json::Map<String, Value> = map
        .iter()
        .filter(|(_, set)| !set.is_empty())
        .map(|(did, set)| {
            let mut pairs: Vec<(i64, i64)> = set.iter().copied().collect();
            pairs.sort_unstable();
            let list = pairs.into_iter().map(|(s, p)| json!([s, p])).collect();
            (did.clone(), Value::Array(list))
        })
        .collect();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(&Value::Object(object)) {
        let _ = fs::write(&path, text);
    }
}

/// did → model, populated by callers (device list, prop dialog) so the
/// cloud-only-prop cache can be keyed by model: every device of a model shares
/// the same local-vs-cloud spec gap, so one device's learning covers them all.
static DEVICE_MODELS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

/// Record a device's model so its LAN-unavailable props are cached per model.
pub fn note_device_model(did: &str, model: &str) {
    let model = model.trim();
    if model.is_empty() {
        return;
    }
    let registry = DEVICE_MODELS.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut map) = registry.lock() {
        map.insert(prop_request_did(did), model.to_string());
    }
}

/// The key under which a device's LAN-unavailable props are cached: its model
/// when known (shared by all devices of that model), otherwise the root DID.
fn lan_cache_key(did: &str) -> String {
    DEVICE_MODELS
        .get()
        .and_then(|registry| registry.lock().ok().and_then(|map| map.get(did).cloned()))
        .filter(|model| !model.is_empty())
        .unwrap_or_else(|| did.to_string())
}

/// The placeholder value shown for a prop the device can't serve over LAN, used
/// under `--LAN` where there is no cloud fallback.
fn lan_unsupported_marker(did: &str, siid: i64, piid: i64) -> Value {
    json!({
        "code": -1,
        "did": did,
        "siid": siid,
        "piid": piid,
        "lanUnsupported": true,
        "value": "此属性不支持局域网模式 (not available over LAN)",
    })
}

fn env_flag(name: &str) -> bool {
    env::var(name)
        .map(|value| {
            let value = value.trim();
            value == "1" || value.eq_ignore_ascii_case("true")
        })
        .unwrap_or(false)
}

/// LAN control is on by default; set `MIT_DISABLE_LAN_DISCOVERY=1` to force every
/// request to the cloud (and skip the background push/discovery thread entirely).
fn lan_discovery_enabled() -> bool {
    !env_flag("MIT_DISABLE_LAN_DISCOVERY")
}

/// Process-global LAN manager per device-id (account), started lazily so all
/// `MicoClient` clones for an account share one background discovery thread.
fn lan_manager_for_device(device_id: &str) -> Option<LanManager> {
    if !lan_discovery_enabled() {
        return None;
    }
    let registry = LAN_MANAGERS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut registry = registry.lock().ok()?;
    if let Some(slot) = registry.get(device_id) {
        return slot.clone();
    }
    let manager = LanManager::start(LanConfig {
        enable_subscribe: env_flag("MIT_LAN_SUBSCRIBE"),
        virtual_did: 0,
    })
    .ok();
    registry.insert(device_id.to_string(), manager.clone());
    manager
}

/// Whether any running LAN manager has discovered this DID online. DIDs are
/// globally unique, so we can scan every account's manager without needing to
/// know which account owns the device.
fn lan_device_is_online_any(did: &str) -> bool {
    let Some(registry) = LAN_MANAGERS.get() else {
        return false;
    };
    let Ok(registry) = registry.lock() else {
        return false;
    };
    registry
        .values()
        .flatten()
        .any(|manager| manager.is_online(did))
}

/// Whether any account has a ready local credential for this DID whose IP is on
/// one of our local subnets (i.e. the same-intranet check would pass, so LAN
/// control is plausible).
fn local_credential_on_subnet_any(did: &str) -> bool {
    let Some(registry) = LOCAL_CREDENTIAL_CACHES.get() else {
        return false;
    };
    let Ok(registry) = registry.lock() else {
        return false;
    };
    registry.values().any(|cache| {
        cache
            .lock()
            .map(|cache| {
                cache.fetched_at > 0
                    && cache.ready_dids.contains(did)
                    && cache
                        .by_did
                        .get(did)
                        .and_then(|cred| cred.local_ip.trim().parse::<std::net::Ipv4Addr>().ok())
                        .map(is_on_local_subnet)
                        .unwrap_or(false)
            })
            .unwrap_or(false)
    })
}

/// Best-effort, non-initializing resolution of the transport a request to this
/// DID would use right now under AUTO routing: `Lan` when the device is locally
/// reachable (LAN-discovered online, or a credential on our subnet), otherwise
/// `Cloud`. Intended for status display.
pub fn device_link_channel(did: &str) -> MiotChannel {
    if !lan_discovery_enabled() {
        return MiotChannel::Cloud;
    }
    if lan_device_is_online_any(did) || local_credential_on_subnet_any(did) {
        MiotChannel::Lan
    } else {
        MiotChannel::Cloud
    }
}

pub fn parse_callback_input(input: &str) -> Result<CallbackInput> {
    let raw = normalize_text(input);
    if raw.is_empty() {
        bail!("请提供完整的回调 URL");
    }
    let (code, state) = if raw.starts_with("http://") || raw.starts_with("https://") {
        let params = Url::parse(&raw)?;
        let mut code = String::new();
        let mut state = String::new();
        for (key, value) in params.query_pairs() {
            if key == "code" {
                code = normalize_text(value.as_ref());
            } else if key == "state" {
                state = normalize_text(value.as_ref());
            }
        }
        (code, state)
    } else {
        let query = raw.trim_start_matches(['?', '#']);
        let parsed = url::form_urlencoded::parse(query.as_bytes()).collect::<Vec<_>>();
        (
            parsed
                .iter()
                .find(|(key, _)| key == "code")
                .map(|(_, value)| normalize_text(value.as_ref()))
                .unwrap_or_default(),
            parsed
                .iter()
                .find(|(key, _)| key == "state")
                .map(|(_, value)| normalize_text(value.as_ref()))
                .unwrap_or_default(),
        )
    };
    if code.is_empty() || state.is_empty() {
        bail!("回调中缺少 code 或 state");
    }
    Ok(CallbackInput { code, state })
}

pub fn build_device_id(uuid: &str) -> Result<String> {
    let normalized = normalize_text(uuid);
    if normalized.is_empty() {
        bail!("auth.uuid 未配置");
    }
    Ok(format!("{PROJECT_CODE}.{normalized}"))
}

pub fn build_state(device_id: &str) -> String {
    format!("{:x}", Sha1::digest(format!("d={device_id}").as_bytes()))
}

pub fn is_auth_expired(auth: &AuthAccount) -> bool {
    if auth.expires_ts <= 0 {
        return normalize_text(&auth.access_token).is_empty();
    }
    auth.expires_ts <= unix_timestamp()
}

fn build_host(region: &str) -> String {
    if region == DEFAULT_REGION {
        OAUTH_API_HOST_DEFAULT.to_string()
    } else {
        format!("{region}.{OAUTH_API_HOST_DEFAULT}")
    }
}

fn normalize_region(value: &str) -> String {
    let region = normalize_text(value).to_lowercase();
    if region.is_empty() {
        "cn".to_string()
    } else {
        region
    }
}

fn local_http_test_override(env_key: &str) -> Option<String> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let value = env::var(env_key)
        .ok()
        .map(|value| normalize_text(&value))
        .filter(|value| !value.is_empty())?;
    let parsed = Url::parse(&value).ok()?;
    let is_local = matches!(parsed.host_str(), Some("127.0.0.1" | "localhost"));
    if parsed.scheme() == "http" && is_local {
        Some(value)
    } else {
        None
    }
}

fn normalize_text(value: &str) -> String {
    value.trim().to_string()
}

fn normalize_text_value(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .map(normalize_text)
        .unwrap_or_default()
}

fn python_bool(value: bool) -> &'static str {
    if value {
        "True"
    } else {
        "False"
    }
}

fn ordered_json(fields: &[(&str, &str)]) -> Result<String> {
    use serde::ser::SerializeMap;
    use serde::Serializer;

    let mut buffer = Vec::new();
    let mut serializer = serde_json::Serializer::new(&mut buffer);
    let mut map = serializer.serialize_map(Some(fields.len()))?;
    for (key, value) in fields {
        map.serialize_entry(key, value)?;
    }
    map.end()?;
    Ok(String::from_utf8(buffer)?)
}

fn encode_ordered_query(params: &[(&str, &str)]) -> String {
    params
        .iter()
        .map(|(key, value)| {
            format!(
                "{}={}",
                urlencoding::encode(key),
                urlencoding::encode(value)
            )
        })
        .collect::<Vec<_>>()
        .join("&")
}

fn encrypt_secret(aes_key: &[u8; 16]) -> Result<String> {
    let pub_key = RsaPublicKey::from_public_key_pem(PUBLIC_KEY_PEM)
        .map_err(|e| anyhow!("RSA key parse error: {e}"))?;
    let encrypted = pub_key
        .encrypt(&mut rand::thread_rng(), Pkcs1v15Encrypt, aes_key)
        .map_err(|e| anyhow!("RSA encrypt error: {e}"))?;
    Ok(STANDARD.encode(encrypted))
}

fn random_aes_key() -> [u8; 16] {
    let mut key = [0_u8; 16];
    OsRng.fill_bytes(&mut key);
    key
}

fn response_text(response: reqwest::blocking::Response) -> Result<String> {
    let status = response.status();
    let text = response.text()?;
    if !status.is_success() {
        return Err(anyhow!(
            "request failed: status={} body={}",
            status.as_u16(),
            text.trim()
        ));
    }
    Ok(text)
}

fn value_i64(value: Option<&Value>) -> i64 {
    match value {
        Some(Value::Number(number)) => number
            .as_i64()
            .or_else(|| number.as_u64().map(|v| v as i64))
            .unwrap_or_else(|| number.as_f64().unwrap_or(0.0).trunc() as i64),
        Some(Value::String(text)) => text.trim().parse::<f64>().unwrap_or(0.0).trunc() as i64,
        _ => 0,
    }
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.trim().to_string(),
        Value::Number(number) => number.to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

static MIOT_VERBOSE: AtomicBool = AtomicBool::new(false);
static FORCE_LAN: AtomicBool = AtomicBool::new(false);

/// Enable diagnostic logging to stderr (the `--verbose` CLI flag). Flow logs are
/// always written to `~/.mit/miot-flow.log`; verbose additionally mirrors them to
/// stderr so the LAN-vs-cloud decision is visible at the terminal.
pub fn set_verbose_logging(enabled: bool) {
    MIOT_VERBOSE.store(enabled, Ordering::Relaxed);
}

fn verbose_logging() -> bool {
    MIOT_VERBOSE.load(Ordering::Relaxed)
}

/// Force LAN control with no cloud fallback (the `--LAN` CLI flag). When set, a
/// device that is not on the local subnet fails fast instead of going to cloud.
pub fn set_force_lan(enabled: bool) {
    FORCE_LAN.store(enabled, Ordering::Relaxed);
}

pub fn force_lan_enabled() -> bool {
    FORCE_LAN.load(Ordering::Relaxed)
}

/// UTC wall-clock `HH:MM:SS` so log lines from different runs are distinguishable
/// (the flow log is append-only).
fn flow_log_clock() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!(
        "{:02}:{:02}:{:02}Z",
        (secs / 3600) % 24,
        (secs / 60) % 60,
        secs % 60
    )
}

fn log_miot_flow(operation: &str, flow: &str, detail: &str) {
    let line = format!(
        "[mit {}] miot-flow op={operation} transport={flow} {detail}",
        flow_log_clock()
    );
    if verbose_logging() {
        eprintln!("{line}");
    }
    if let Some(path) = miot_flow_log_path() {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(file, "{line}");
        }
    }
}

/// Emit one standardized routing line so the chosen flow is unambiguous:
/// `op=<op> transport=<LAN|Cloud|none> did=<did> mode=<auto|forced-lan>
/// reason=<…> result=<ok|err|pending>`.
fn log_route(
    op: &str,
    did: &str,
    mode: &str,
    transport: &str,
    reason: &str,
    result: &str,
    error: Option<&anyhow::Error>,
) {
    let mut detail = format!("did={did} mode={mode} reason={reason} result={result}");
    if let Some(error) = error {
        detail.push_str(&format!(" error={error}"));
    }
    log_miot_flow(op, transport, &detail);
}

fn miot_flow_log_path() -> Option<PathBuf> {
    let home = get_home_dir();
    if home.as_os_str().is_empty() {
        return None;
    }
    Some(home.join(".mit").join(MIOT_FLOW_LOG_FILE))
}

fn routed_credential(
    target: &DeviceSummary,
    proxy: &LocalDeviceCredential,
    source: LocalCredentialSource,
) -> LocalDeviceCredential {
    LocalDeviceCredential {
        did: target.did.clone(),
        name: target.name.clone(),
        model: target.model.clone(),
        local_ip: proxy.local_ip.clone(),
        token: proxy.token.clone(),
        source,
    }
}

fn is_gateway_model(model: &str) -> bool {
    let text = model.to_ascii_lowercase();
    text.contains("gateway") || text.contains(".hub")
}

/// Whether a LAN error is a transient private-network glitch worth retrying
/// (UDP packet loss surfaces as a read timeout, and an occasional truncated
/// reply fails AES unpadding) rather than a permanent failure.
fn is_transient_lan_error(error: &anyhow::Error) -> bool {
    let text = error.to_string().to_ascii_lowercase();
    text.contains("timed out")
        || text.contains("would block")
        || text.contains("resource temporarily unavailable")
        || text.contains("os error 35")
        || text.contains("unpaderror")
}

fn root_did_for_sub_device(did: &str) -> Option<&str> {
    let (root, suffix) = did.rsplit_once(".s")?;
    if root.trim().is_empty() || suffix.trim().is_empty() {
        return None;
    }
    if suffix.chars().all(|ch| ch.is_ascii_digit()) {
        Some(root)
    } else {
        None
    }
}

fn prop_request_did(did: &str) -> String {
    root_did_for_sub_device(did).unwrap_or(did).to_string()
}

fn local_credential_cache_keys(did: &str) -> Vec<String> {
    let mut out = vec![did.to_string()];
    let request_did = prop_request_did(did);
    if request_did != did {
        out.push(request_did);
    }
    out
}

fn is_error_with_negative_code(value: &Value) -> bool {
    value
        .get("code")
        .and_then(Value::as_i64)
        .is_some_and(|code| code < 0)
}

fn collect_home_placements(homes: &Value) -> HashMap<String, DevicePlacement> {
    let mut placements = HashMap::new();
    for key in ["homelist", "share_home_list"] {
        let Some(home_list) = homes.get(key).and_then(Value::as_array) else {
            continue;
        };
        for home in home_list {
            let home_id = normalize_text_value(home.get("id"));
            let top_level_dids = home
                .get("dids")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for did in top_level_dids {
                let key = value_to_string(&did);
                if key.is_empty() {
                    continue;
                }
                placements.insert(
                    key,
                    DevicePlacement {
                        home_id: home_id.clone(),
                        room_id: String::new(),
                    },
                );
            }
            let room_list = home
                .get("roomlist")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for room in room_list {
                let room_id = normalize_text_value(room.get("id"));
                let room_dids = room
                    .get("dids")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                for did in room_dids {
                    let key = value_to_string(&did);
                    if key.is_empty() {
                        continue;
                    }
                    placements.insert(
                        key,
                        DevicePlacement {
                            home_id: home_id.clone(),
                            room_id: room_id.clone(),
                        },
                    );
                }
            }
        }
    }
    placements
}

fn collect_home_dids(homes: &Value) -> Vec<String> {
    let mut dids = homes
        .get("homelist")
        .and_then(Value::as_array)
        .into_iter()
        .chain(homes.get("share_home_list").and_then(Value::as_array))
        .flat_map(|homes| homes.iter())
        .flat_map(|home| {
            let top_level = home
                .get("dids")
                .and_then(Value::as_array)
                .into_iter()
                .flat_map(|dids| dids.iter());
            let room_level = home
                .get("roomlist")
                .and_then(Value::as_array)
                .into_iter()
                .flat_map(|rooms| rooms.iter())
                .flat_map(|room| room.get("dids").and_then(Value::as_array).into_iter())
                .flat_map(|dids| dids.iter());
            top_level.chain(room_level).collect::<Vec<_>>()
        })
        .map(value_to_string)
        .filter(|did| !did.trim().is_empty())
        .collect::<Vec<_>>();
    dids.sort();
    dids.dedup();
    dids
}

fn unix_timestamp() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
#[path = "../tests/module_tests/mico_api.rs"]
mod tests;
