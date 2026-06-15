use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedRequest {
    pub method: String,
    pub path: String,
    pub query: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

pub struct MockMicoServer {
    address: SocketAddr,
    #[allow(dead_code)]
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
enum MockFixture {
    Default,
    MijiaRenewal,
    MijiaRenewalUnauthorized,
    RoutedLocalCredentials,
    SubDeviceDidRequiresRoot,
}

impl MockMicoServer {
    pub fn start() -> Self {
        Self::start_with_fixture(None, MockFixture::Default)
    }

    #[allow(dead_code)]
    pub fn start_with_mijia_renewal() -> Self {
        Self::start_with_fixture(None, MockFixture::MijiaRenewal)
    }

    #[allow(dead_code)]
    pub fn start_with_mijia_renewal_unauthorized() -> Self {
        Self::start_with_fixture(None, MockFixture::MijiaRenewalUnauthorized)
    }

    /// Opt-in fixture for exercising routed local credential behavior.
    #[allow(dead_code)]
    pub fn start_with_routed_local_credentials() -> Self {
        Self::start_with_fixture(None, MockFixture::RoutedLocalCredentials)
    }

    #[allow(dead_code)]
    pub fn start_with_sub_device_dids_require_root() -> Self {
        Self::start_with_fixture(None, MockFixture::SubDeviceDidRequiresRoot)
    }

    #[allow(dead_code)]
    pub fn start_with_push_failure_on_attempt(attempt: usize) -> Self {
        Self::start_with_fixture(Some(attempt), MockFixture::Default)
    }

    fn start_with_fixture(fail_push_on_attempt: Option<usize>, fixture: MockFixture) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let push_attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let thread_requests = Arc::clone(&requests);
        let thread_shutdown = Arc::clone(&shutdown);
        let thread_push_attempts = Arc::clone(&push_attempts);
        let thread_base_url = format!("http://{address}");
        let handle = thread::spawn(move || loop {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            if thread_shutdown.load(Ordering::SeqCst) {
                break;
            }
            let request = read_request(&mut stream);
            if let Some(request) = request {
                if let Ok(mut requests) = thread_requests.lock() {
                    requests.push(request.clone());
                }
                let _ = write_json_response(
                    &mut stream,
                    route(
                        &request,
                        fail_push_on_attempt,
                        &thread_push_attempts,
                        fixture,
                        thread_base_url.as_str(),
                    ),
                );
            }
        });

        Self {
            address,
            requests,
            shutdown,
            handle: Some(handle),
        }
    }

    pub fn base_url(&self) -> String {
        format!("http://{}", self.address)
    }

    #[allow(dead_code)]
    pub fn mijia_service_login_url(&self) -> String {
        format!("{}/pass/serviceLogin", self.base_url())
    }

    #[allow(dead_code)]
    pub fn mijia_login_url(&self) -> String {
        format!("{}/longPolling/loginUrl", self.base_url())
    }

    #[allow(dead_code)]
    pub fn mijia_api_base_url(&self) -> String {
        format!("{}/app", self.base_url())
    }

    #[allow(dead_code)]
    pub fn user_profile_url(&self) -> String {
        format!("{}/user/profile", self.base_url())
    }

    #[allow(dead_code)]
    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for MockMicoServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.address);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Option<RecordedRequest> {
    let mut data = Vec::new();
    let mut buffer = [0_u8; 1024];
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                data.extend_from_slice(&buffer[..count]);
                if data.windows(4).any(|chunk| chunk == b"\r\n\r\n") {
                    break;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.kind() == std::io::ErrorKind::TimedOut =>
            {
                break;
            }
            Err(_) => return None,
        }
    }
    if data.is_empty() {
        return None;
    }

    let header_end = data
        .windows(4)
        .position(|chunk| chunk == b"\r\n\r\n")
        .map(|index| index + 4)
        .unwrap_or(data.len());
    let headers = String::from_utf8_lossy(&data[..header_end]).into_owned();
    let mut lines = headers.lines();
    let request_line = lines.next()?;
    let request_headers = lines
        .clone()
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_string(), value.trim().to_string()))
        })
        .collect::<Vec<_>>();
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();
    let (path, query) = target
        .split_once('?')
        .map(|(path, query)| (path.to_string(), query.to_string()))
        .unwrap_or_else(|| (target, String::new()));
    let content_length = lines
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.eq_ignore_ascii_case("content-length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);
    while data.len() < header_end + content_length {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => data.extend_from_slice(&buffer[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.kind() == std::io::ErrorKind::TimedOut =>
            {
                break;
            }
            Err(_) => break,
        }
    }
    let body = String::from_utf8_lossy(&data[header_end..]).into_owned();

    Some(RecordedRequest {
        method,
        path,
        query,
        headers: request_headers,
        body,
    })
}

struct MockResponse {
    status: &'static str,
    body: Value,
    headers: Vec<(String, String)>,
}

impl From<Value> for MockResponse {
    fn from(body: Value) -> Self {
        Self {
            status: "200 OK",
            body,
            headers: Vec::new(),
        }
    }
}

fn route(
    request: &RecordedRequest,
    fail_push_on_attempt: Option<usize>,
    push_attempts: &std::sync::atomic::AtomicUsize,
    fixture: MockFixture,
    base_url: &str,
) -> MockResponse {
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/pass/serviceLogin")
            if matches!(
                fixture,
                MockFixture::MijiaRenewal | MockFixture::MijiaRenewalUnauthorized
            ) =>
        {
            json!({
            "code": 0,
            "location": format!("{base_url}/mijia/renew-callback"),
            "ssecurity": "AQIDBAUGBwgJCgsMDQ4PEA==",
            "passToken": "pass-token-renewed",
            "userId": "1001",
            "cUserId": "c-1001"
            })
            .into()
        }
        ("GET", "/pass/serviceLogin") => json!({
            "code": 70016,
            "location": format!("{base_url}/mijia/login?sid=mijia&_sign=sign-a&qs=qs-a&callback=callback-a")
        })
        .into(),
        ("GET", "/mijia/renew-callback") => MockResponse {
            status: "200 OK",
            body: json!({
                "code": 0,
                "result": "ok"
            }),
            headers: vec![
                (
                    "Set-Cookie".to_string(),
                    "serviceToken=service-token-renewed; Path=/; HttpOnly".to_string(),
                ),
                (
                    "Set-Cookie".to_string(),
                    "userId=1001; Path=/; HttpOnly".to_string(),
                ),
                (
                    "Set-Cookie".to_string(),
                    "cUserId=c-1001; Path=/; HttpOnly".to_string(),
                ),
            ],
        },
        ("GET", "/longPolling/loginUrl") => json!({
            "code": 0,
            "loginUrl": format!("{base_url}/mijia/qr-login"),
            "qr": format!("{base_url}/mijia/qr.png"),
            "lp": format!("{base_url}/longPolling/lp")
        })
        .into(),
        ("GET", "/longPolling/lp") => json!({
            "code": 0,
            "psecurity": "psecurity-a",
            "nonce": "nonce-a",
            "ssecurity": "AQIDBAUGBwgJCgsMDQ4PEA==",
            "passToken": "pass-token-a",
            "userId": "1001",
            "cUserId": "c-1001",
            "serviceToken": "service-token-a",
            "location": format!("{base_url}/mijia/callback")
        })
        .into(),
        ("GET", "/mijia/callback") => json!({
            "code": 0,
            "result": "ok"
        })
        .into(),
        ("POST", "/app/v2/message/v2/check_new_msg") if fixture == MockFixture::MijiaRenewal => {
            let attempt = push_attempts.fetch_add(1, Ordering::SeqCst) + 1;
            if attempt == 1 {
                json!({
                    "code": 3,
                    "message": "SERVICETOKEN_EXPIRED"
                })
                .into()
            } else {
                json!({
                    "code": 0,
                    "result": {
                        "has_new_msg": false
                    }
                })
                .into()
            }
        }
        ("POST", "/app/v2/message/v2/check_new_msg")
            if fixture == MockFixture::MijiaRenewalUnauthorized =>
        {
            let attempt = push_attempts.fetch_add(1, Ordering::SeqCst) + 1;
            if attempt == 1 {
                MockResponse {
                    status: "401 Unauthorized",
                    body: json!({
                        "code": 401,
                        "message": "Unauthorized"
                    }),
                    headers: Vec::new(),
                }
            } else {
                json!({
                    "code": 0,
                    "result": {
                        "has_new_msg": false
                    }
                })
                .into()
            }
        }
        ("POST", "/app/v2/message/v2/check_new_msg") => json!({
            "code": 0,
            "result": {
                "has_new_msg": false
            }
        })
        .into(),
        ("POST", "/app/v2/homeroom/gethome_merged") => json!({
            "code": 0,
            "result": {
                "homelist": [
                    {
                        "id": "home-a",
                        "name": "Home",
                        "uid": 1001,
                        "roomlist": []
                    }
                ],
                "share_home_list": []
            }
        })
        .into(),
        ("GET", "/miot-spec-v2/instances") => json!({
            "instances": [
                {
                    "model": "xiaomi.wifispeaker.lx04",
                    "type": "urn:miot-spec-v2:device:speaker:0000A015:xiaomi-lx04:1",
                    "ts": 100
                },
                {
                    "model": "xiaomi.wifispeaker.oh2p",
                    "type": "urn:miot-spec-v2:device:speaker:0000A015:xiaomi-oh2p:1",
                    "ts": 100
                },
                {
                    "model": "xiaomi.wifispeaker.oh4w",
                    "type": "urn:miot-spec-v2:device:speaker:0000A015:xiaomi-oh4w:1",
                    "ts": 100
                },
                {
                    "model": "xiaomi.gateway.v3",
                    "type": "urn:miot-spec-v2:device:gateway:0000A019:xiaomi-v3:1",
                    "ts": 100
                }
            ]
        })
        .into(),
        ("GET", "/miot-spec-v2/template/list/device") => json!({
            "result": [
                {
                    "type": "urn:miot-spec-v2:device:speaker:0000A015",
                    "description": {"en": "Speaker", "zh_cn": "音箱"}
                },
                {
                    "type": "urn:miot-spec-v2:device:gateway:0000A019",
                    "description": {"en": "Gateway", "zh_cn": "网关"}
                }
            ]
        })
        .into(),
        ("GET", "/miot-spec-v2/instance") => {
            let urn = request
                .query
                .split('&')
                .find_map(|entry| entry.split_once('='))
                .filter(|(key, _)| *key == "type")
                .map(|(_, value)| value.replace("%3A", ":"))
                .unwrap_or_default();
            json!({
                "type": urn,
                "services": []
            })
            .into()
        }
        ("GET", "/instance/v2/multiLanguage") => json!({
            "data": {"zh_cn": {}}
        })
        .into(),
        ("GET", "/app/v2/mico/oauth/get_token") => json!({
            "code": 0,
            "result": {
                "access_token": "token-a",
                "refresh_token": "refresh-a",
                "expires_in": 7200
            }
        })
        .into(),
        ("GET", "/user/profile") => json!({
            "code": 0,
            "data": {
                "miliaoNick": "账号A",
                "miliaoIcon": "",
                "unionId": "union-a"
            }
        })
        .into(),
        ("POST", "/app/v2/oauth/get_uid_by_unionid") => json!({
            "code": 0,
            "result": "1001"
        })
        .into(),
        ("POST", "/app/v2/homeroom/gethome") => match fixture {
            MockFixture::Default
            | MockFixture::MijiaRenewalUnauthorized
            | MockFixture::SubDeviceDidRequiresRoot => json!({
                "code": 0,
                "result": {
                    "homelist": [
                        {
                            "id": "home-1",
                            "name": "我家",
                            "uid": 1001,
                            "dids": ["dev-1", "dev-2", "dev-3"],
                            "roomlist": [
                                {
                                    "id": "room-1",
                                    "name": "客厅",
                                    "dids": ["dev-1"]
                                },
                                {
                                    "id": "room-2",
                                    "name": "卧室",
                                    "dids": ["dev-2"]
                                },
                                {
                                    "id": "room-3",
                                    "name": "厨房",
                                    "dids": ["dev-3"]
                                }
                            ]
                        }
                    ],
                    "share_home_list": []
                }
            })
            .into(),
            MockFixture::MijiaRenewal | MockFixture::RoutedLocalCredentials => json!({
                "code": 0,
                "result": {
                    "homelist": [
                        {
                            "id": "home-1",
                            "name": "我家",
                            "dids": ["gw-1", "dev-2"],
                            "roomlist": [
                                {
                                    "id": "room-1",
                                    "name": "客厅",
                                    "dids": ["gw-1"]
                                },
                                {
                                    "id": "room-2",
                                    "name": "卧室",
                                    "dids": ["dev-2"]
                                }
                            ]
                        }
                    ],
                    "share_home_list": []
                }
            })
            .into(),
        },

        ("POST", "/app/v2/home/home_device_list") => {
            let body: Value = serde_json::from_str(&request.body).unwrap_or_else(|_| json!({}));
            let include_third = body
                .get("get_third_device")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let mut device_info = vec![
                json!({
                    "did": "dev-1",
                    "name": "living-room",
                    "model": "xiaomi.wifispeaker.lx04",
                    "isOnline": true,
                    "localip": "192.168.1.10",
                    "token": "token-dev-1"
                }),
                json!({
                    "did": "dev-2",
                    "name": "bedroom",
                    "model": "xiaomi.wifispeaker.oh2p",
                    "isOnline": false,
                    "localip": "192.168.1.11",
                    "token": "token-dev-2"
                }),
                json!({
                    "did": "dev-3",
                    "name": "kitchen",
                    "model": "xiaomi.wifispeaker.oh4w",
                    "isOnline": true,
                    "localip": "",
                    "token": ""
                }),
            ];
            if include_third {
                device_info.push(json!({
                    "did": "third-1",
                    "name": "third-party-light",
                    "model": "third.party.light",
                    "isOnline": true,
                    "localip": "",
                    "token": ""
                }));
            }
            json!({
                "code": 0,
                "result": {
                    "device_info": device_info,
                    "has_more": false,
                    "max_did": ""
                }
            })
            .into()
        }

        ("POST", "/app/v2/home/device_list_page") => match fixture {
            MockFixture::Default
            | MockFixture::MijiaRenewalUnauthorized
            | MockFixture::SubDeviceDidRequiresRoot => json!({
                "code": 0,
                "result": {
                    "list": [
                        {
                            "did": "dev-1",
                            "name": "living-room",
                            "model": "xiaomi.wifispeaker.lx04",
                            "spec_type": "urn:miot-spec-v2:device:speaker:0000A015:xiaomi-lx04:1",
                            "isOnline": true,
                            "voice_ctrl": 2,
                            "localip": "192.168.0.20",
                            "token": "00112233445566778899aabbccddeeff"
                        },
                        {
                            "did": "dev-2",
                            "name": "bedroom",
                            "model": "xiaomi.wifispeaker.oh2p",
                            "spec_type": "urn:miot-spec-v2:device:speaker:0000A015:xiaomi-oh2p:1",
                            "isOnline": true,
                            "voice_ctrl": 1
                        },
                        {
                            "did": "dev-3",
                            "name": "kitchen",
                            "model": "xiaomi.wifispeaker.oh4w",
                            "spec_type": "urn:miot-spec-v2:device:speaker:0000A015:xiaomi-oh4w:1",
                            "isOnline": true,
                            "voice_ctrl": 1
                        }
                    ],
                    "has_more": false
                }
            })
            .into(),
            MockFixture::MijiaRenewal | MockFixture::RoutedLocalCredentials => json!({
                "code": 0,
                "result": {
                    "list": [
                        {
                            "did": "gw-1",
                            "name": "gateway",
                            "model": "xiaomi.gateway.v3",
                            "spec_type": "urn:miot-spec-v2:device:gateway:0000A019:xiaomi-v3:1",
                            "isOnline": true,
                            "voice_ctrl": 2,
                            "localip": "127.0.0.1",
                            "token": "00112233445566778899aabbccddeeff"
                        },
                        {
                            "did": "dev-2",
                            "name": "bedroom",
                            "model": "xiaomi.wifispeaker.oh2p",
                            "spec_type": "urn:miot-spec-v2:device:speaker:0000A015:xiaomi-oh2p:1",
                            "isOnline": true,
                            "voice_ctrl": 1
                        }
                    ],
                    "has_more": false
                }
            })
            .into(),
        },

        ("POST", "/app/v2/miotspec/action") | ("POST", "/miotspec/action") => json!({
            "code": 0,
            "result": {
                "ok": true
            }
        })
        .into(),
        ("POST", "/app/v2/miotspec/prop/get") | ("POST", "/miotspec/prop/get") => {
            let payload: Value = serde_json::from_str(&request.body).unwrap_or_else(|_| json!({}));
            let params = payload
                .get("params")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let result = params
                .into_iter()
                .map(|item| {
                    let did_text = item
                        .get("did")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    if fixture == MockFixture::SubDeviceDidRequiresRoot && did_text.contains(".s") {
                        return json!({
                            "did": did_text,
                            "siid": item.get("siid").cloned().unwrap_or(Value::Null),
                            "piid": item.get("piid").cloned().unwrap_or(Value::Null),
                            "code": -704042011
                        });
                    }
                    let piid = item.get("piid").and_then(Value::as_i64).unwrap_or(0);
                    json!({
                        "did": item.get("did").cloned().unwrap_or(Value::Null),
                        "siid": item.get("siid").cloned().unwrap_or(Value::Null),
                        "piid": item.get("piid").cloned().unwrap_or(Value::Null),
                        "value": piid % 2 == 1
                    })
                })
                .collect::<Vec<_>>();
            json!({
                "code": 0,
                "result": result
            })
            .into()
        }
        ("POST", "/app/v2/miotspec/prop/set") | ("POST", "/miotspec/prop/set") => json!({
            "code": 0,
            "result": [
                {
                    "did": "dev-1",
                    "siid": 2,
                    "piid": 1,
                    "code": 0
                }
            ]
        })
        .into(),
        ("POST", "/app/v2/oauth/save_text") => {
            let attempt = push_attempts.fetch_add(1, Ordering::SeqCst) + 1;
            if fail_push_on_attempt == Some(attempt) {
                json!({
                    "code": 500,
                    "message": "simulated push failure"
                })
                .into()
            } else {
                json!({
                    "code": 0,
                    "result": "notify-1001"
                })
                .into()
            }
        }
        ("POST", "/app/v2/oauth/send_push") => json!({
            "code": 0,
            "result": true
        })
        .into(),
        ("GET", path) if path.ends_with("/releases/latest") => {
            if path.contains("ratelimited") {
                MockResponse {
                    status: "403 Forbidden",
                    body: json!({ "message": "API rate limit exceeded for 1.2.3.4" }),
                    headers: vec![
                        ("x-ratelimit-remaining".to_string(), "0".to_string()),
                        ("x-ratelimit-reset".to_string(), "9999999999".to_string()),
                    ],
                }
            } else {
                json!({ "tag_name": "v9.9.9" }).into()
            }
        }
        _ => json!({
            "code": 404,
            "message": format!("unexpected route: {} {}", request.method, request.path)
        })
        .into(),
    }
}

fn write_json_response(stream: &mut TcpStream, response: MockResponse) -> std::io::Result<()> {
    let text = serde_json::to_string(&response.body).unwrap();
    let headers = response
        .headers
        .iter()
        .map(|(name, value)| format!("{name}: {value}\r\n"))
        .collect::<String>();
    let response = format!(
        "HTTP/1.1 {}\r\nContent-Type: application/json\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n{}",
        response.status,
        headers,
        text.len(),
        text
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    Ok(())
}
