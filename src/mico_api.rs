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
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use url::Url;

use crate::miio_local::MiioUdpClient;
use crate::storage::{get_account_dir, get_home_dir, AuthAccount, UserProfile, DEFAULT_REGION};

const PROJECT_CODE: &str = "mico";
pub const OAUTH_CLIENT_ID: &str = "2882303761520431603";
const OAUTH_AUTH_URL: &str = "https://account.xiaomi.com/oauth2/authorize";
const OAUTH_API_HOST_DEFAULT: &str = "mico.api.mijia.tech";
const USER_PROFILE_URL: &str = "https://open.account.xiaomi.com/user/profile";
const MICO_BASE_URL_ENV: &str = "MIT_MICO_BASE_URL";
const USER_PROFILE_URL_ENV: &str = "MIT_USER_PROFILE_URL";
const TOKEN_EXPIRES_RATIO: f64 = 0.7;
const LOCAL_MIIO_PORT: u16 = 54321;
const LOCAL_UDP_MAX_ATTEMPTS: usize = 1;
const LOCAL_UDP_PROBE_TIMEOUT: Duration = Duration::from_millis(200);
const MIOT_FLOW_LOG_FILE: &str = "miot-flow.log";
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
    pub home_id: String,
    pub home_name: String,
    pub room_id: String,
    pub room_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalCredentialSource {
    Direct,
    RoomGateway,
    HomeGateway,
    FallbackGateway,
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
        for key in ["homelist", "share_home_list"] {
            let Some(list) = homes.get(key).and_then(Value::as_array) else {
                continue;
            };
            for home in list {
                let home_id = normalize_text_value(home.get("id"));
                let home_name = normalize_text_value(home.get("name"));
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
            }
        }

        let mut dids = placements
            .keys()
            .filter(|did| !did.trim().is_empty())
            .cloned()
            .collect::<Vec<_>>();
        dids.sort();

        let mut devices = Vec::new();
        for batch in dids.chunks(150) {
            let page = self.get_device_list_page(batch, "")?;
            for did in batch {
                let Some(info) = page.get(did) else {
                    continue;
                };
                let placement = placements.get(did).cloned().unwrap_or_else(|| {
                    (String::new(), String::new(), String::new(), String::new())
                });
                devices.push(Device {
                    did: info.did.clone(),
                    name: info.name.clone(),
                    model: info.model.clone(),
                    online: info.online,
                    home_id: placement.0,
                    home_name: placement.1,
                    room_id: placement.2,
                    room_name: placement.3,
                });
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
            .filter(|(did, _)| !cache.disabled_dids.contains(*did))
            .map(|(did, credential)| (did.clone(), credential.clone()))
            .collect();
        let known_dids = cache.by_did.keys().cloned().collect::<HashSet<_>>();
        cache.ready_dids.retain(|did| known_dids.contains(did));
        let credentials_to_probe = cache
            .by_did
            .iter()
            .filter(|(did, _)| !cache.disabled_dids.contains(*did))
            .map(|(did, credential)| (did.clone(), credential.clone()))
            .collect::<Vec<_>>();
        drop(cache);

        for (did, credential) in credentials_to_probe {
            if self.local_device_probe_succeeds(&credential) {
                let mut cache = self
                    .local_credential_cache
                    .lock()
                    .map_err(|_| anyhow!("local credential cache lock poisoned"))?;
                cache.ready_dids.insert(did);
                continue;
            }
            let mut cache = self
                .local_credential_cache
                .lock()
                .map_err(|_| anyhow!("local credential cache lock poisoned"))?;
            cache.ready_dids.remove(&did);
            cache.disabled_dids.insert(did);
        }

        let mut cache = self
            .local_credential_cache
            .lock()
            .map_err(|_| anyhow!("local credential cache lock poisoned"))?;
        let disabled_dids = cache.disabled_dids.clone();
        cache.by_did.retain(|did, _| !disabled_dids.contains(did));
        let known_dids = cache.by_did.keys().cloned().collect::<HashSet<_>>();
        cache.ready_dids.retain(|did| known_dids.contains(did));
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

    fn invalidate_cached_local_credential(&self, did: &str) -> Result<()> {
        let mut cache = self
            .local_credential_cache
            .lock()
            .map_err(|_| anyhow!("local credential cache lock poisoned"))?;
        cache.by_did.remove(did);
        cache.ready_dids.remove(did);
        cache.disabled_dids.insert(did.to_string());
        Ok(())
    }

    fn local_miio_client_for_did(&self, did: &str) -> Result<Option<MiioUdpClient>> {
        let Some(credential) = self.lookup_cached_local_credential(did)? else {
            return Ok(None);
        };
        let token = credential.token.trim();
        if token.is_empty() || credential.local_ip.trim().is_empty() {
            return Ok(None);
        }
        let addr = if credential.local_ip.contains(':') {
            credential.local_ip
        } else {
            format!("{}:{LOCAL_MIIO_PORT}", credential.local_ip.trim())
        };
        Ok(Some(MiioUdpClient::new(&addr, token)?))
    }

    fn local_device_probe_succeeds(&self, credential: &LocalDeviceCredential) -> bool {
        let token = credential.token.trim();
        if token.is_empty() || credential.local_ip.trim().is_empty() {
            return false;
        }
        let addr = if credential.local_ip.contains(':') {
            credential.local_ip.clone()
        } else {
            format!("{}:{LOCAL_MIIO_PORT}", credential.local_ip.trim())
        };
        MiioUdpClient::with_timeout(&addr, token, LOCAL_UDP_PROBE_TIMEOUT)
            .and_then(|client| client.probe())
            .is_ok()
    }

    fn try_local_request_with_retry(
        &self,
        did: &str,
        method: &str,
        params: Value,
    ) -> Option<Value> {
        let client = match self.local_miio_client_for_did(did) {
            Ok(Some(client)) => client,
            Ok(None) => {
                log_miot_flow(
                    method,
                    "local-udp",
                    &format!("did={did} reason=missing-local-credential"),
                );
                return None;
            }
            Err(error) => {
                log_miot_flow(
                    method,
                    "local-udp",
                    &format!("did={did} reason=credential-error error={error}"),
                );
                return None;
            }
        };

        for attempt in 1..=LOCAL_UDP_MAX_ATTEMPTS {
            match client.request(method, params.clone()) {
                Ok(value) => return Some(value),
                Err(error)
                    if attempt < LOCAL_UDP_MAX_ATTEMPTS && is_transient_local_udp_error(&error) =>
                {
                    continue;
                }
                Err(error) => {
                    log_miot_flow(
                        method,
                        "local-udp",
                        &format!("did={did} attempt={attempt} error={error}"),
                    );
                    if let Err(invalidate_error) = self.invalidate_cached_local_credential(did) {
                        log_miot_flow(
                            method,
                            "local-udp",
                            &format!("did={did} stage=invalidate error={invalidate_error}"),
                        );
                    }
                    return None;
                }
            }
        }

        None
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

    pub fn get_props_batch(&self, params: &[(&str, i64, i64)]) -> Result<Value> {
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
            let miio_params = items
                .iter()
                .map(|(_, siid, piid)| {
                    json!({
                        "did": did,
                        "siid": siid,
                        "piid": piid,
                    })
                })
                .collect::<Vec<_>>();
            let local = self.try_local_request_with_retry(
                did.as_str(),
                "get_properties",
                Value::Array(miio_params),
            );
            if let Some(result) = local {
                log_miot_flow("get_props_batch", "local-udp", &format!("did={did}"));
                if let Some(values) = result.as_array() {
                    for (item_index, (_, _, _)) in items.iter().enumerate() {
                        if let Some(value) = values.get(item_index) {
                            output[items[item_index].0] = value.clone();
                            continue;
                        }
                        unresolved_indices.push(items[item_index].0);
                    }
                } else {
                    unresolved_indices.extend(items.iter().map(|(index, _, _)| *index));
                }
            } else {
                log_miot_flow(
                    "get_props_batch",
                    "cloud",
                    &format!("did={did} reason=fallback"),
                );
                unresolved_indices.extend(items.iter().map(|(index, _, _)| *index));
            }
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
        log_miot_flow(
            "get_props_batch",
            "cloud",
            &format!("items={}", unresolved_indices.len()),
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
        if let Some(result) = self.try_local_request_with_retry(
            did.as_str(),
            "set_properties",
            Value::Array(vec![json!({
                "did": did,
                "siid": siid,
                "piid": piid,
                "value": value.clone(),
            })]),
        ) {
            log_miot_flow("set_prop", "local-udp", &format!("did={did} {siid}.{piid}"));
            return Ok(result);
        }
        log_miot_flow(
            "set_prop",
            "cloud",
            &format!("did={did} {siid}.{piid} reason=fallback"),
        );
        self.post_encrypted(
            "/app/v2/miotspec/prop/set",
            &json!({
                "params": [
                    {
                        "did": did,
                        "siid": siid,
                        "piid": piid,
                        "value": value,
                    }
                ]
            }),
        )
    }

    pub fn action(&self, did: &str, siid: i64, aiid: i64, values: &[Value]) -> Result<Value> {
        let did = prop_request_did(did);
        if let Some(result) = self.try_local_request_with_retry(
            did.as_str(),
            "action",
            json!({
                "did": did,
                "siid": siid,
                "aiid": aiid,
                "in": values,
            }),
        ) {
            log_miot_flow("action", "local-udp", &format!("did={did} {siid}.{aiid}"));
            return Ok(result);
        }
        log_miot_flow(
            "action",
            "cloud",
            &format!("did={did} {siid}.{aiid} reason=fallback"),
        );
        self.post_encrypted(
            "/app/v2/miotspec/action",
            &json!({
                "params": {
                    "did": did,
                    "siid": siid,
                    "aiid": aiid,
                    "in": values,
                }
            }),
        )
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

fn log_miot_flow(operation: &str, flow: &str, detail: &str) {
    let line = format!("[mit] miot-flow op={operation} transport={flow} {detail}");
    if let Some(path) = miot_flow_log_path() {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(file, "{line}");
        }
    }
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

fn is_transient_local_udp_error(error: &anyhow::Error) -> bool {
    let text = error.to_string().to_ascii_lowercase();
    text.contains("timed out")
        || text.contains("would block")
        || text.contains("resource temporarily unavailable")
        || text.contains("os error 35")
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
