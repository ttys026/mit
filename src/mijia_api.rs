use anyhow::{anyhow, bail, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use rand::{rngs::OsRng, RngCore};
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, SET_COOKIE};
use reqwest::StatusCode;
use serde_json::{json, Value};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::env;
use std::io::Read;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use url::Url;

use crate::storage::{mijia_identity, MijiaAuth, UserProfile};

const DEFAULT_LOCALE: &str = "zh_CN";
const DEFAULT_SERVICE_LOGIN_URL: &str =
    "https://account.xiaomi.com/pass/serviceLogin?_json=true&sid=mijia&_locale=zh_CN";
const DEFAULT_LOGIN_URL: &str = "https://account.xiaomi.com/longPolling/loginUrl";
const DEFAULT_API_BASE_URL: &str = "https://api.mijia.tech/app";
const DEFAULT_TIMEZONE: &str = "GMT+08:00";
const MIJIA_SERVICE_LOGIN_URL_ENV: &str = "MIT_MIJIA_SERVICE_LOGIN_URL";
const MIJIA_LOGIN_URL_ENV: &str = "MIT_MIJIA_LOGIN_URL";
const MIJIA_API_BASE_URL_ENV: &str = "MIT_MIJIA_API_BASE_URL";
const DEFAULT_LOGIN_TIMEOUT_SECS: u64 = 120;
const ANDROID_UA_PREFIX: &str = "Android-15-11.0.701-Xiaomi-23046RP50C-OS2.0.212.0.VMYCNXM";

#[derive(Clone, Debug)]
pub struct MijiaLoginSession {
    pub login_url: String,
    pub qr_url: String,
    pub lp_url: String,
    ua: String,
    device_id: String,
    pass_o: String,
}

#[derive(Clone)]
pub struct MijiaClient {
    http: Client,
    service_login_url: String,
    login_url: String,
    api_base_url: String,
    locale: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceHistoryQuery {
    pub time_start: i64,
    pub time_end: i64,
    pub limit: u32,
}

impl DeviceHistoryQuery {
    pub fn recent(limit: u32) -> Self {
        let now_secs = unix_millis() / 1000;
        Self {
            time_start: now_secs.saturating_sub(32 * 24 * 60 * 60),
            time_end: now_secs.saturating_add(60),
            limit,
        }
    }
}

impl MijiaClient {
    pub fn new() -> Result<Self> {
        Ok(Self {
            http: Client::builder()
                .timeout(Duration::from_secs(30))
                .cookie_store(true)
                .build()?,
            service_login_url: env::var(MIJIA_SERVICE_LOGIN_URL_ENV)
                .unwrap_or_else(|_| DEFAULT_SERVICE_LOGIN_URL.to_string()),
            login_url: env::var(MIJIA_LOGIN_URL_ENV)
                .unwrap_or_else(|_| DEFAULT_LOGIN_URL.to_string()),
            api_base_url: env::var(MIJIA_API_BASE_URL_ENV)
                .unwrap_or_else(|_| DEFAULT_API_BASE_URL.to_string()),
            locale: DEFAULT_LOCALE.to_string(),
        })
    }

    pub fn prepare_qr_login(&self, existing: Option<&MijiaAuth>) -> Result<MijiaLoginSession> {
        let ua = existing
            .map(|auth| auth.ua.clone())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| build_user_agent(DEFAULT_LOCALE));
        let device_id = existing
            .map(|auth| auth.device_id.clone())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| random_from_alphabet(16, MIJIA_DEVICE_ID_ALPHABET));
        let pass_o = existing
            .map(|auth| auth.pass_o.clone())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| random_hex_lower(16));

        let location_data =
            self.get_location_data(existing, ua.as_str(), device_id.as_str(), pass_o.as_str())?;
        let mut login_url = Url::parse(&self.login_url)?;
        {
            let dc = unix_millis().to_string();
            let mut pairs = login_url.query_pairs_mut();
            for (key, value) in location_data {
                pairs.append_pair(key.as_str(), value.as_str());
            }
            pairs.append_pair("theme", "");
            pairs.append_pair("bizDeviceType", "");
            pairs.append_pair("_hasLogo", "false");
            pairs.append_pair("_qrsize", "240");
            pairs.append_pair("_dc", dc.as_str());
        }

        let login_data = self
            .http
            .get(login_url)
            .headers(login_headers(ua.as_str()))
            .send()
            .and_then(|response| response.error_for_status())?
            .text()?;
        let login_data = parse_xiaomi_json(&login_data)?;
        if value_i64(login_data.get("code")) != 0 {
            bail!(
                "米家二维码登录初始化失败: code={} message={}",
                value_i64(login_data.get("code")),
                text_value(login_data.get("desc"))
                    .or_else(|| text_value(login_data.get("message")))
                    .unwrap_or_default()
            );
        }

        let login_url = text_value(login_data.get("loginUrl"))
            .ok_or_else(|| anyhow!("米家登录响应缺少 loginUrl"))?;
        let qr_url =
            text_value(login_data.get("qr")).ok_or_else(|| anyhow!("米家登录响应缺少 qr"))?;
        let lp_url =
            text_value(login_data.get("lp")).ok_or_else(|| anyhow!("米家登录响应缺少 lp"))?;

        Ok(MijiaLoginSession {
            login_url,
            qr_url,
            lp_url,
            ua,
            device_id,
            pass_o,
        })
    }

    pub fn finish_qr_login(&self, session: &MijiaLoginSession) -> Result<MijiaAuth> {
        let timeout_secs = env::var("MIT_MIJIA_LOGIN_TIMEOUT_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(DEFAULT_LOGIN_TIMEOUT_SECS);
        let text = self
            .http
            .get(session.lp_url.as_str())
            .headers(login_headers(session.ua.as_str()))
            .timeout(Duration::from_secs(timeout_secs))
            .send()
            .and_then(|response| response.error_for_status())?
            .text()?;
        let lp_data = parse_xiaomi_json(&text)?;
        if value_i64(lp_data.get("code")) != 0 {
            bail!(
                "米家扫码登录失败: code={} message={}",
                value_i64(lp_data.get("code")),
                text_value(lp_data.get("desc"))
                    .or_else(|| text_value(lp_data.get("message")))
                    .unwrap_or_default()
            );
        }

        let callback_url = text_value(lp_data.get("location"))
            .ok_or_else(|| anyhow!("米家扫码登录响应缺少 location"))?;
        let callback_response = self
            .http
            .get(callback_url.as_str())
            .headers(login_headers(session.ua.as_str()))
            .send()
            .and_then(|response| response.error_for_status())?;
        let cookies = cookies_from_response(callback_response.headers());

        let service_token = cookies
            .get("serviceToken")
            .or_else(|| cookies.get("yetAnotherServiceToken"))
            .cloned()
            .or_else(|| text_value(lp_data.get("serviceToken")))
            .ok_or_else(|| anyhow!("米家扫码登录未返回 serviceToken"))?;
        let user_id = login_user_id(&cookies, &lp_data);
        let c_user_id = cookies
            .get("cUserId")
            .cloned()
            .or_else(|| text_value(lp_data.get("cUserId")))
            .unwrap_or_default();

        Ok(MijiaAuth {
            ua: session.ua.clone(),
            device_id: session.device_id.clone(),
            pass_o: session.pass_o.clone(),
            ssecurity: text_value(lp_data.get("ssecurity")).unwrap_or_default(),
            pass_token: text_value(lp_data.get("passToken")).unwrap_or_default(),
            user_id,
            c_user_id,
            service_token,
            expire_time: unix_millis() + 30 * 24 * 60 * 60 * 1000,
            save_time: unix_millis(),
        })
    }

    pub fn check_new_msg(&self, auth: &MijiaAuth) -> Result<()> {
        let envelope = self.check_new_msg_envelope(auth)?;
        ensure_mijia_success(&envelope)?;
        Ok(())
    }

    pub fn check_new_msg_with_renewal(&self, auth: &MijiaAuth) -> Result<Option<MijiaAuth>> {
        match self.check_new_msg_envelope(auth) {
            Ok(envelope) if value_i64(envelope.get("code")) == 0 => Ok(None),
            Ok(envelope) if is_mijia_token_expired_response(&envelope) => {
                self.renew_auth_and_check(auth)
            }
            Ok(envelope) => {
                ensure_mijia_success(&envelope)?;
                Ok(None)
            }
            Err(error) if is_http_unauthorized(&error) => self.renew_auth_and_check(auth),
            Err(error) => Err(error),
        }
    }

    fn renew_auth_and_check(&self, auth: &MijiaAuth) -> Result<Option<MijiaAuth>> {
        let renewed = self
            .renew_auth(auth)?
            .ok_or_else(|| anyhow!("米家 token 已过期且无法自动续期，请重新登录米家"))?;
        let envelope = self.check_new_msg_envelope(&renewed)?;
        ensure_mijia_success(&envelope)?;
        Ok(Some(renewed))
    }

    pub fn renew_auth(&self, auth: &MijiaAuth) -> Result<Option<MijiaAuth>> {
        if auth.pass_token.trim().is_empty() || mijia_identity(auth).trim().is_empty() {
            return Ok(None);
        }

        let data = self.get_service_login_data(
            Some(auth),
            auth.ua.as_str(),
            auth.device_id.as_str(),
            auth.pass_o.as_str(),
        )?;
        if value_i64(data.get("code")) != 0 {
            return Ok(None);
        }

        let location = text_value(data.get("location"))
            .ok_or_else(|| anyhow!("米家自动续期响应缺少 location"))?;
        let response = self
            .http
            .get(location.as_str())
            .headers(login_headers(auth.ua.as_str()))
            .send()
            .and_then(|response| response.error_for_status())?;
        let cookies = cookies_from_response(response.headers());
        let service_token = cookies
            .get("serviceToken")
            .or_else(|| cookies.get("yetAnotherServiceToken"))
            .cloned()
            .ok_or_else(|| anyhow!("米家自动续期未返回 serviceToken"))?;
        let now = unix_millis();
        Ok(Some(MijiaAuth {
            ua: auth.ua.clone(),
            device_id: auth.device_id.clone(),
            pass_o: auth.pass_o.clone(),
            ssecurity: text_value(data.get("ssecurity")).unwrap_or_else(|| auth.ssecurity.clone()),
            pass_token: text_value(data.get("passToken"))
                .unwrap_or_else(|| auth.pass_token.clone()),
            user_id: cookies
                .get("userId")
                .cloned()
                .or_else(|| uid_text(data.get("userId")))
                .unwrap_or_else(|| auth.user_id.clone()),
            c_user_id: cookies
                .get("cUserId")
                .cloned()
                .or_else(|| text_value(data.get("cUserId")))
                .unwrap_or_else(|| auth.c_user_id.clone()),
            service_token,
            expire_time: now + 30 * 24 * 60 * 60 * 1000,
            save_time: now,
        }))
    }

    pub fn resolve_user_profile(&self, auth: &MijiaAuth) -> Result<UserProfile> {
        let user_id = auth.user_id.trim();
        if !user_id.is_empty() {
            return Ok(UserProfile {
                uid: user_id.to_string(),
                nickname: String::new(),
                icon: String::new(),
                union_id: String::new(),
            });
        }

        let payload = json!({
            "fg": true,
            "fetch_share": true,
            "fetch_share_dev": true,
            "fetch_cariot": true,
            "limit": 300,
            "app_ver": 7,
            "plat_form": 0
        });
        let envelope = self.post_encrypted("/v2/homeroom/gethome_merged", auth, &payload)?;
        user_profile_from_mijia_home_payload(&envelope)
            .ok_or_else(|| anyhow!("米家登录成功但未能解析小米 UID"))
    }

    pub fn get_user_device_data(
        &self,
        auth: &MijiaAuth,
        did: &str,
        key: &str,
        typ: &str,
    ) -> Result<Value> {
        self.get_user_device_data_with_query(auth, did, key, typ, DeviceHistoryQuery::recent(50))
    }

    pub fn get_user_device_data_with_query(
        &self,
        auth: &MijiaAuth,
        did: &str,
        key: &str,
        typ: &str,
        query: DeviceHistoryQuery,
    ) -> Result<Value> {
        validate_mijia_auth(auth)?;
        self.post_encrypted(
            "/user/get_user_device_data",
            auth,
            &device_history_payload(did, key, typ, query),
        )
    }

    pub fn get_user_statistics(
        &self,
        auth: &MijiaAuth,
        did: &str,
        key: &str,
        data_type: &str,
    ) -> Result<Value> {
        self.get_user_statistics_with_query(
            auth,
            did,
            key,
            data_type,
            DeviceHistoryQuery::recent(31),
        )
    }

    pub fn get_user_statistics_with_query(
        &self,
        auth: &MijiaAuth,
        did: &str,
        key: &str,
        data_type: &str,
        query: DeviceHistoryQuery,
    ) -> Result<Value> {
        validate_mijia_auth(auth)?;
        self.post_encrypted(
            "/v2/user/statistics",
            auth,
            &device_statistics_payload_with_query(did, key, data_type, query),
        )
    }

    fn post_encrypted(&self, uri: &str, auth: &MijiaAuth, payload: &Value) -> Result<Value> {
        let (body, nonce) = encrypted_form_body(uri, "POST", auth, payload)?;
        let response = self
            .http
            .post(format!(
                "{}{}",
                self.api_base_url.trim_end_matches('/'),
                uri
            ))
            .headers(api_headers(auth, self.locale.as_str())?)
            .body(body)
            .send()
            .and_then(|response| response.error_for_status())?;
        let text = response.text()?;
        serde_json::from_str::<Value>(&text).or_else(|_| decrypt_response(auth, &nonce, &text))
    }

    fn check_new_msg_envelope(&self, auth: &MijiaAuth) -> Result<Value> {
        validate_mijia_auth(auth)?;
        self.post_encrypted(
            "/v2/message/v2/check_new_msg",
            auth,
            &check_new_msg_payload(),
        )
    }

    fn get_location_data(
        &self,
        existing: Option<&MijiaAuth>,
        ua: &str,
        device_id: &str,
        pass_o: &str,
    ) -> Result<Vec<(String, String)>> {
        let data = self.get_service_login_data(existing, ua, device_id, pass_o)?;
        let location =
            text_value(data.get("location")).ok_or_else(|| anyhow!("米家登录响应缺少 location"))?;
        let parsed = Url::parse(location.as_str())?;
        Ok(parsed
            .query_pairs()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect())
    }

    fn get_service_login_data(
        &self,
        existing: Option<&MijiaAuth>,
        ua: &str,
        device_id: &str,
        pass_o: &str,
    ) -> Result<Value> {
        let mut headers = login_headers(ua);
        headers.insert(
            "Cookie",
            HeaderValue::from_str(&format!(
                "deviceId={};pass_o={};passToken={};userId={};cUserId={};uLocale={};",
                device_id,
                pass_o,
                existing.map(|auth| auth.pass_token.as_str()).unwrap_or(""),
                existing.map(|auth| auth.user_id.as_str()).unwrap_or(""),
                existing.map(|auth| auth.c_user_id.as_str()).unwrap_or(""),
                self.locale
            ))?,
        );
        let text = self
            .http
            .get(self.service_login_url.as_str())
            .headers(headers)
            .send()
            .and_then(|response| response.error_for_status())?
            .text()?;
        parse_xiaomi_json(&text)
    }
}

pub fn is_mijia_auth_present(auth: Option<&MijiaAuth>) -> bool {
    auth.is_some_and(|auth| !auth.service_token.trim().is_empty())
}

fn check_new_msg_payload() -> Value {
    json!({
        "begin_at": (unix_millis() / 1000).saturating_sub(3600)
    })
}

fn device_history_payload(did: &str, key: &str, typ: &str, query: DeviceHistoryQuery) -> Value {
    json!({
        "did": mijia_request_did(did),
        "key": key,
        "type": typ,
        "time_start": query.time_start,
        "time_end": query.time_end,
        "limit": query.limit
    })
}

#[cfg(test)]
fn device_statistics_payload(did: &str, key: &str, data_type: &str) -> Value {
    device_statistics_payload_with_query(did, key, data_type, DeviceHistoryQuery::recent(31))
}

fn device_statistics_payload_with_query(
    did: &str,
    key: &str,
    data_type: &str,
    query: DeviceHistoryQuery,
) -> Value {
    json!({
        "did": mijia_request_did(did),
        "key": key,
        "data_type": data_type,
        "time_start": query.time_start,
        "time_end": query.time_end,
        "limit": query.limit
    })
}

fn mijia_request_did(did: &str) -> String {
    let Some((root, suffix)) = did.rsplit_once(".s") else {
        return did.to_string();
    };
    if root.trim().is_empty() || suffix.trim().is_empty() {
        return did.to_string();
    }
    if suffix.chars().all(|ch| ch.is_ascii_digit()) {
        root.to_string()
    } else {
        did.to_string()
    }
}

pub fn mijia_qr_html(session: &MijiaLoginSession, status_path: &str) -> String {
    let status_path_json =
        serde_json::to_string(status_path).unwrap_or_else(|_| "\"/mijia_login_status\"".into());
    format!(
        "<!doctype html><meta charset=\"utf-8\"><title>mit 米家登录</title>\
<style>body{{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;max-width:560px;margin:48px auto;padding:0 24px;line-height:1.6;color:#1f2937}}img{{width:240px;height:240px;border:1px solid #e5e7eb}}code{{word-break:break-all}}.error{{color:#b91c1c}}</style>\
<h1>米家登录</h1>\
<p id=\"mit-login-status\">请使用米家 App 扫描二维码完成登录。</p>\
<p id=\"mit-login-qr\"><img src=\"{}\" alt=\"米家登录二维码\"></p>\
<p id=\"mit-login-note\">扫码确认后保持此页面和终端打开，mit 会自动保存米家登录凭据。</p>\
<p id=\"mit-login-link\">如果二维码无法显示，可以<a href=\"{}\">打开登录页面</a>。<br><code>{}</code></p>\
<script>(function(){{\
var statusUrl={};\
var startedAt=Date.now();\
var failedSince=0;\
var timeoutMs=130000;\
var statusEl=document.getElementById('mit-login-status');\
var qrEl=document.getElementById('mit-login-qr');\
var noteEl=document.getElementById('mit-login-note');\
var linkEl=document.getElementById('mit-login-link');\
function hideLoginDetails(){{if(qrEl)qrEl.style.display='none';if(noteEl)noteEl.style.display='none';if(linkEl)linkEl.style.display='none';}}\
function showSuccess(message){{hideLoginDetails();statusEl.className='';statusEl.textContent=message||'授权成功，可以关闭此页面。';}}\
function showError(message){{hideLoginDetails();statusEl.className='error';statusEl.textContent=message;}}\
function timedOut(){{return Date.now()-startedAt>timeoutMs;}}\
async function poll(){{\
try{{\
var response=await fetch(statusUrl,{{cache:'no-store'}});\
if(!response.ok)throw new Error('bad status');\
failedSince=0;\
var data=await response.json();\
if(data.status==='succeeded'){{showSuccess(data.message);return;}}\
if(data.status==='failed'){{showError(data.message||'米家登录失败，请重新运行 mit auth login mijia 后重试。');return;}}\
if(timedOut()){{showError('米家登录超时，请重新运行 mit auth login mijia 后重试。');return;}}\
}}catch(error){{\
if(!failedSince)failedSince=Date.now();\
if(Date.now()-failedSince>5000||timedOut()){{showError('本地登录服务已停止或超时，请重新运行 mit auth login mijia 后重试。');return;}}\
}}\
setTimeout(poll,1000);\
}}\
setTimeout(poll,1000);\
}})();</script>",
        escape_html(session.qr_url.as_str()),
        escape_html(session.login_url.as_str()),
        escape_html(session.login_url.as_str()),
        status_path_json
    )
}

fn validate_mijia_auth(auth: &MijiaAuth) -> Result<()> {
    if auth.service_token.trim().is_empty() {
        bail!("米家 serviceToken 缺失");
    }
    if auth.ssecurity.trim().is_empty() {
        bail!("米家 ssecurity 缺失");
    }
    let uid = mijia_identity(auth);
    if uid.trim().is_empty() {
        bail!("米家 userId 缺失");
    }
    Ok(())
}

fn ensure_mijia_success(envelope: &Value) -> Result<()> {
    if value_i64(envelope.get("code")) == 0 {
        return Ok(());
    }
    bail!(
        "米家 token 校验失败: code={} message={}",
        value_i64(envelope.get("code")),
        text_value(envelope.get("message"))
            .or_else(|| text_value(envelope.get("desc")))
            .unwrap_or_default()
    )
}

fn is_mijia_token_expired_response(envelope: &Value) -> bool {
    let code = value_i64(envelope.get("code"));
    let message = text_value(envelope.get("message"))
        .or_else(|| text_value(envelope.get("desc")))
        .unwrap_or_default();
    code == 2
        || code == 3
        || message.contains("auth err")
        || matches!(
            message.as_str(),
            "invalid signature" | "SERVICETOKEN_EXPIRED"
        )
}

fn is_http_unauthorized(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<reqwest::Error>()
        .and_then(reqwest::Error::status)
        .is_some_and(|status| status == StatusCode::UNAUTHORIZED)
}

fn login_headers(ua: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("User-Agent", HeaderValue::from_str(ua).expect("valid ua"));
    headers.insert("Accept-Encoding", HeaderValue::from_static("gzip"));
    headers.insert(
        "Content-Type",
        HeaderValue::from_static("application/x-www-form-urlencoded"),
    );
    headers.insert("Connection", HeaderValue::from_static("keep-alive"));
    headers
}

fn api_headers(auth: &MijiaAuth, locale: &str) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert("User-Agent", HeaderValue::from_str(auth.ua.as_str())?);
    headers.insert(
        "Content-Type",
        HeaderValue::from_static("application/x-www-form-urlencoded"),
    );
    headers.insert("Accept-Encoding", HeaderValue::from_static("identity"));
    headers.insert("miot-accept-encoding", HeaderValue::from_static("GZIP"));
    headers.insert(
        "miot-encrypt-algorithm",
        HeaderValue::from_static("ENCRYPT-RC4"),
    );
    headers.insert(
        "x-xiaomi-protocal-flag-cli",
        HeaderValue::from_static("PROTOCAL-HTTP2"),
    );
    headers.insert(
        "Cookie",
        HeaderValue::from_str(&format!(
            "userId={};cUserId={};yetAnotherServiceToken={};serviceToken={};locale={};timezone={};is_daylight=0;dst_offset=0;channel=MI_APP_STORE;PassportDeviceId={};",
            mijia_identity(auth),
            auth.c_user_id,
            auth.service_token,
            auth.service_token,
            locale,
            DEFAULT_TIMEZONE,
            auth.device_id
        ))?,
    );
    Ok(headers)
}

fn encrypted_form_body(
    uri: &str,
    method: &str,
    auth: &MijiaAuth,
    payload: &Value,
) -> Result<(String, String)> {
    let nonce = gen_nonce();
    let signed_nonce = signed_nonce(auth.ssecurity.as_str(), nonce.as_str())?;
    let data = serde_json::to_string(payload)?;
    let mut params = vec![("data".to_string(), data)];
    let rc4_hash = enc_signature(uri, method, signed_nonce.as_str(), &params);
    params.push(("rc4_hash__".to_string(), rc4_hash));

    let mut encrypted = Vec::new();
    for (key, value) in params {
        encrypted.push((key, encrypt_rc4(signed_nonce.as_str(), value.as_str())?));
    }
    let signature = enc_signature(uri, method, signed_nonce.as_str(), &encrypted);
    encrypted.push(("signature".to_string(), signature));
    encrypted.push(("ssecurity".to_string(), auth.ssecurity.clone()));
    encrypted.push(("_nonce".to_string(), nonce.clone()));

    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(
            encrypted
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
        )
        .finish();
    Ok((body, nonce))
}

fn decrypt_response(auth: &MijiaAuth, nonce: &str, text: &str) -> Result<Value> {
    let signed_nonce = signed_nonce(auth.ssecurity.as_str(), nonce)?;
    let bytes = decrypt_rc4(signed_nonce.as_str(), text.trim())?;
    parse_plain_or_gzip_json(&bytes)
}

fn parse_plain_or_gzip_json(bytes: &[u8]) -> Result<Value> {
    match serde_json::from_slice(bytes) {
        Ok(value) => Ok(value),
        Err(json_error) => {
            let mut decoder = flate2::read::GzDecoder::new(bytes);
            let mut decoded = Vec::new();
            decoder.read_to_end(&mut decoded).map_err(|gzip_error| {
                anyhow!("米家响应解码失败: json={json_error}; gzip={gzip_error}")
            })?;
            Ok(serde_json::from_slice(&decoded)?)
        }
    }
}

fn gen_nonce() -> String {
    let mut random = [0_u8; 8];
    OsRng.fill_bytes(&mut random);
    let mut bytes = random.to_vec();
    let minutes = unix_millis() / 60000;
    let minute_bytes = minutes.to_be_bytes();
    let first = minute_bytes
        .iter()
        .position(|byte| *byte != 0)
        .unwrap_or(minute_bytes.len() - 1);
    bytes.extend_from_slice(&minute_bytes[first..]);
    STANDARD.encode(bytes)
}

fn signed_nonce(ssecurity: &str, nonce: &str) -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(STANDARD.decode(ssecurity)?);
    hasher.update(STANDARD.decode(nonce)?);
    Ok(STANDARD.encode(hasher.finalize()))
}

fn enc_signature(
    uri: &str,
    method: &str,
    signed_nonce: &str,
    params: &[(String, String)],
) -> String {
    let mut parts = vec![method.to_uppercase(), uri.to_string()];
    for (key, value) in params {
        parts.push(format!("{key}={value}"));
    }
    parts.push(signed_nonce.to_string());
    STANDARD.encode(Sha1::digest(parts.join("&").as_bytes()))
}

fn encrypt_rc4(password_b64: &str, payload: &str) -> Result<String> {
    Ok(STANDARD.encode(rc4_crypt(
        &STANDARD.decode(password_b64)?,
        payload.as_bytes(),
    )?))
}

fn decrypt_rc4(password_b64: &str, payload: &str) -> Result<Vec<u8>> {
    rc4_crypt(&STANDARD.decode(password_b64)?, &STANDARD.decode(payload)?)
}

fn rc4_crypt(key: &[u8], input: &[u8]) -> Result<Vec<u8>> {
    if key.is_empty() {
        bail!("RC4 key is empty");
    }
    let mut s = [0_u8; 256];
    for (index, item) in s.iter_mut().enumerate() {
        *item = index as u8;
    }
    let mut j = 0usize;
    for i in 0..256 {
        j = (j + s[i] as usize + key[i % key.len()] as usize) & 0xff;
        s.swap(i, j);
    }

    let mut i = 0usize;
    j = 0;
    for _ in 0..1024 {
        i = (i + 1) & 0xff;
        j = (j + s[i] as usize) & 0xff;
        s.swap(i, j);
        let _ = s[(s[i] as usize + s[j] as usize) & 0xff];
    }

    let mut out = Vec::with_capacity(input.len());
    for byte in input {
        i = (i + 1) & 0xff;
        j = (j + s[i] as usize) & 0xff;
        s.swap(i, j);
        let k = s[(s[i] as usize + s[j] as usize) & 0xff];
        out.push(byte ^ k);
    }
    Ok(out)
}

fn parse_xiaomi_json(text: &str) -> Result<Value> {
    let trimmed = text.trim().trim_start_matches("&&&START&&&").trim();
    Ok(serde_json::from_str(trimmed)?)
}

fn cookies_from_response(headers: &HeaderMap) -> std::collections::HashMap<String, String> {
    headers
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|cookie| cookie.split(';').next())
        .filter_map(|pair| pair.split_once('='))
        .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
        .collect()
}

fn login_user_id(cookies: &std::collections::HashMap<String, String>, lp_data: &Value) -> String {
    cookies
        .get("userId")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| uid_text(lp_data.get("userId")))
        .unwrap_or_default()
}

fn user_profile_from_mijia_home_payload(payload: &Value) -> Option<UserProfile> {
    let homes = payload
        .get("result")
        .and_then(|result| result.get("homelist"))
        .or_else(|| payload.get("homelist"))
        .and_then(Value::as_array)?;
    let uid = homes
        .iter()
        .find_map(|home| uid_text(home.get("uid")))
        .filter(|value| !value.is_empty())?;
    Some(UserProfile {
        uid,
        nickname: String::new(),
        icon: String::new(),
        union_id: String::new(),
    })
}

fn text_value(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(|text| text.trim().to_string())
}

fn uid_text(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => {
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn value_i64(value: Option<&Value>) -> i64 {
    match value {
        Some(Value::Number(number)) => number.as_i64().unwrap_or(0),
        Some(Value::String(text)) => text.trim().parse::<i64>().unwrap_or(0),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_user_id_prefers_sts_cookie_over_long_polling_payload() {
        let mut headers = HeaderMap::new();
        headers.append(
            SET_COOKIE,
            HeaderValue::from_static("userId=3009043526; Path=/; HttpOnly"),
        );
        headers.append(
            SET_COOKIE,
            HeaderValue::from_static("cUserId=encrypted-user; Path=/; HttpOnly"),
        );
        let cookies = cookies_from_response(&headers);
        let payload = json!({"userId": "", "cUserId": "payload-user"});

        assert_eq!(login_user_id(&cookies, &payload), "3009043526");
    }

    #[test]
    fn default_mijia_api_base_url_uses_mijia_sid_host() {
        std::env::remove_var(MIJIA_API_BASE_URL_ENV);

        let client = MijiaClient::new().unwrap();

        assert_eq!(client.api_base_url, "https://api.mijia.tech/app");
    }

    #[test]
    fn check_new_msg_payload_includes_begin_at_timestamp() {
        let payload = check_new_msg_payload();
        let begin_at = payload
            .get("begin_at")
            .and_then(Value::as_i64)
            .expect("begin_at timestamp");

        assert!(begin_at > 0);
    }

    #[test]
    fn device_history_payload_targets_device_key_type_and_recent_window() {
        let payload =
            device_history_payload("device-1", "2.1", "prop", DeviceHistoryQuery::recent(5));

        assert_eq!(payload["did"], "device-1");
        assert_eq!(payload["key"], "2.1");
        assert_eq!(payload["type"], "prop");
        assert!(payload.get("uid").is_none());
        assert_eq!(payload["limit"], 5);
        let time_start = payload["time_start"].as_i64().unwrap();
        let time_end = payload["time_end"].as_i64().unwrap();
        assert!(time_end >= time_start);
        assert!(time_end - time_start <= 32 * 24 * 60 * 60 + 60);
    }

    #[test]
    fn device_history_payload_strips_sub_device_suffix() {
        let payload = device_history_payload(
            "2045081210.s2",
            "2.1",
            "prop",
            DeviceHistoryQuery::recent(5),
        );

        assert_eq!(payload["did"], "2045081210");
    }

    #[test]
    fn device_history_payload_accepts_explicit_window_and_limit() {
        let payload = device_history_payload(
            "device-1",
            "2.1",
            "prop",
            DeviceHistoryQuery {
                time_start: 1_700_000_000,
                time_end: 1_700_086_399,
                limit: 20,
            },
        );

        assert_eq!(payload["time_start"], 1_700_000_000);
        assert_eq!(payload["time_end"], 1_700_086_399);
        assert_eq!(payload["limit"], 20);
    }

    #[test]
    fn device_statistics_payload_targets_device_key_type_and_recent_window() {
        let payload = device_statistics_payload("device-1", "11.1", "stat_day_v3");

        assert_eq!(payload["did"], "device-1");
        assert_eq!(payload["key"], "11.1");
        assert_eq!(payload["data_type"], "stat_day_v3");
        assert!(payload.get("uid").is_none());
        assert_eq!(payload["limit"], 31);
        let time_start = payload["time_start"].as_i64().unwrap();
        let time_end = payload["time_end"].as_i64().unwrap();
        assert!(time_end >= time_start);
        assert!(time_end - time_start <= 32 * 24 * 60 * 60 + 60);
    }

    #[test]
    fn device_statistics_payload_accepts_explicit_window_and_limit() {
        let payload = device_statistics_payload_with_query(
            "device-1",
            "11.1",
            "stat_day_v3",
            DeviceHistoryQuery {
                time_start: 1_700_000_000,
                time_end: 1_700_604_799,
                limit: 7,
            },
        );

        assert_eq!(payload["time_start"], 1_700_000_000);
        assert_eq!(payload["time_end"], 1_700_604_799);
        assert_eq!(payload["limit"], 7);
    }

    #[test]
    fn device_statistics_payload_strips_sub_device_suffix() {
        let payload = device_statistics_payload("2045081210.s2", "11.1", "stat_day_v3");

        assert_eq!(payload["did"], "2045081210");
    }

    #[test]
    fn user_profile_from_mijia_home_payload_uses_owned_home_uid() {
        let payload = json!({
            "code": 0,
            "result": {
                "homelist": [
                    {
                        "id": "home-a",
                        "name": "Home",
                        "uid": 3009043526_u64,
                        "roomlist": []
                    }
                ],
                "share_home_list": [
                    {
                        "id": "shared-home",
                        "name": "Shared",
                        "uid": 2002,
                        "roomlist": []
                    }
                ]
            }
        });

        let profile = user_profile_from_mijia_home_payload(&payload).unwrap();
        assert_eq!(profile.uid, "3009043526");
        assert_eq!(profile.nickname, "");
    }
}

fn unix_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn random_hex_lower(len: usize) -> String {
    random_from_alphabet(len, b"0123456789abcdef")
}

const MIJIA_DEVICE_ID_ALPHABET: &[u8] =
    b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ_-";

fn random_from_alphabet(len: usize, alphabet: &[u8]) -> String {
    let mut out = String::with_capacity(len);
    let mut byte = [0_u8; 1];
    for _ in 0..len {
        OsRng.fill_bytes(&mut byte);
        out.push(alphabet[byte[0] as usize % alphabet.len()] as char);
    }
    out
}

fn build_user_agent(locale: &str) -> String {
    let country = locale.split('_').nth(1).unwrap_or("CN");
    let ua_id1 = random_hex_upper(40);
    let ua_id2 = random_hex_upper(32);
    let ua_id3 = random_hex_upper(32);
    let ua_id4 = random_hex_upper(40);
    let pass_o = random_hex_lower(16);
    format!("{ANDROID_UA_PREFIX}-{ua_id1}-{country}-{ua_id3}-{ua_id2}-SmartHome-MI_APP_STORE-{ua_id1}|{ua_id4}|{pass_o}-64")
}

fn random_hex_upper(len: usize) -> String {
    random_from_alphabet(len, b"0123456789ABCDEF")
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
