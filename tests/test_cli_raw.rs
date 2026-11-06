use std::fs;
use std::net::{TcpListener, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mit::miot_lan::{decrypt_packet, gen_packet};
use serde_json::{json, Value};

const DID: &str = "123456789";
const TOKEN: [u8; 16] = [0x11; 16];
const TOKEN_HEX: &str = "11111111111111111111111111111111";

struct Profile(PathBuf);

impl Profile {
    fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("mit-raw-{}-{nanos}", std::process::id()));
        fs::create_dir_all(path.join(".mit")).unwrap();
        Self(path)
    }

    fn seed(&self, address: &str, uids: &[&str]) {
        let accounts: Vec<Value> = uids
            .iter()
            .map(|uid| {
                json!({
                    "version": 2,
                    "xiaomi": {
                        "region": "cn",
                        "redirectUri": "http://127.0.0.1:8000/login_redirect",
                        "uuid": format!("{uid:0>32}"),
                        "accessToken": format!("expired-token-{uid}"),
                        "refreshToken": format!("refresh-must-not-be-used-{uid}"),
                        "expiresTs": 1
                    },
                    "user": { "uid": uid, "nickname": "Test", "icon": "", "unionId": format!("test-{uid}") }
                })
            })
            .collect();
        fs::write(
            self.0.join(".mit/auth.json"),
            json!({"accounts": accounts}).to_string(),
        )
        .unwrap();
        for uid in uids {
            let dir = self.0.join(".mit/accounts").join(uid);
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join("local_credentials.json"),
                json!({
                    "version": 1,
                    "devices": { DID: "cred-1" },
                    "credentials": { "cred-1": { "localIp": address, "token": TOKEN_HEX } }
                })
                .to_string(),
            )
            .unwrap();
        }
    }
}

impl Drop for Profile {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run(home: &Path, args: &[&str]) -> Output {
    // Any accidental OAuth refresh / cloud lookup reaches this listener.
    let cloud = TcpListener::bind("127.0.0.1:0").unwrap();
    cloud.set_nonblocking(true).unwrap();
    let url = format!("http://{}", cloud.local_addr().unwrap());
    let output = Command::new(env!("CARGO_BIN_EXE_mit"))
        .args(args)
        .env("MIT_HOME", home)
        .env("MIT_MICO_BASE_URL", &url)
        .env("MIT_USER_PROFILE_URL", &url)
        .output()
        .unwrap();
    assert!(cloud.accept().is_err(), "raw must never contact the cloud");
    output
}

fn mock_device(reply: Value) -> (String, thread::JoinHandle<Value>) {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let address = socket.local_addr().unwrap().to_string();
    let worker = thread::spawn(move || {
        let mut buf = [0u8; 2048];
        let (n, peer) = socket.recv_from(&mut buf).unwrap();
        assert_eq!(n, 32);
        assert!(buf[4..32].iter().all(|byte| *byte == 0xff));
        let mut hello = [0u8; 32];
        hello[..4].copy_from_slice(&[0x21, 0x31, 0, 32]);
        hello[8..12].copy_from_slice(&(123456789u32).to_be_bytes());
        hello[12..16].copy_from_slice(&100u32.to_be_bytes());
        socket.send_to(&hello, peer).unwrap();
        let (n, sender) = socket.recv_from(&mut buf).unwrap();
        assert_eq!(sender, peer);
        let command = decrypt_packet(&TOKEN, &buf[..n]).unwrap();
        if reply.is_null() {
            // Simulate a lost acknowledgement after the device has executed a
            // write. The CLI must time out without sending the write again.
            socket
                .set_read_timeout(Some(Duration::from_secs(4)))
                .unwrap();
            assert!(socket.recv_from(&mut buf).is_err(), "raw retried a write");
            return command;
        }
        let mut reply = reply;
        reply["id"] = command["id"].clone();
        socket
            .send_to(&gen_packet(123456789, &TOKEN, 101, &reply).unwrap(), peer)
            .unwrap();
        command
    });
    (address, worker)
}

#[test]
fn raw_reads_and_backs_up_with_expired_cloud_auth() {
    let home = Profile::new();
    let (address, worker) = mock_device(json!({ "result": [100, 0] }));
    home.seed(&address, &["1001"]);
    let output = run(
        &home.0,
        &[
            "--json",
            "raw",
            DID,
            "get_prop",
            r#"["tds_warn_thd","lightMode"]"#,
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        json!([100, 0])
    );
    let command = worker.join().unwrap();
    assert_eq!(command["method"], "get_prop");
    assert_eq!(command["params"], json!(["tds_warn_thd", "lightMode"]));
    assert!(!String::from_utf8_lossy(&output.stdout).contains(TOKEN_HEX));
}

#[test]
fn raw_passes_write_parameter_types_unchanged() {
    for params in [r#"["0"]"#, "[0]", r#"{"value":1}"#] {
        let home = Profile::new();
        let (address, worker) = mock_device(json!({ "result": ["ok"] }));
        home.seed(&address, &["1001"]);
        let output = run(&home.0, &["raw", DID, "set_lightMode", params]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let command = worker.join().unwrap();
        assert_eq!(command["method"], "set_lightMode");
        assert_eq!(
            command["params"],
            serde_json::from_str::<Value>(params).unwrap()
        );
    }
}

#[test]
fn raw_defaults_to_empty_array_and_selects_account() {
    let home = Profile::new();
    let (address, worker) = mock_device(json!({ "result": {"model": "test"} }));
    home.seed(&address, &["1001", "1002"]);
    let output = run(
        &home.0,
        &["--LAN", "raw", DID, "miIO.info", "--uid", "1002"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(worker.join().unwrap()["params"], json!([]));
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        json!({"model": "test"})
    );
}

#[test]
fn raw_device_error_fails_without_backup_output() {
    let home = Profile::new();
    let (address, worker) =
        mock_device(json!({ "error": {"code": -32601, "message": "unknown method"} }));
    home.seed(&address, &["1001"]);
    let output = run(&home.0, &["--json", "raw", DID, "unsupported"]);
    worker.join().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown method"));
}

#[test]
fn raw_does_not_retry_a_write_after_a_lost_reply() {
    let home = Profile::new();
    let (address, worker) = mock_device(Value::Null);
    home.seed(&address, &["1001"]);
    let output = run(
        &home.0,
        &["--json", "raw", DID, "set_lightMode", r#"["1"]"#],
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(worker.join().unwrap()["method"], "set_lightMode");
}

#[test]
fn raw_rejects_invalid_parameters_before_device_access() {
    let home = Profile::new();
    for params in ["[", "null", "1", r#""text""#, "true"] {
        let output = run(&home.0, &["raw", DID, "get_prop", params]);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
    let output = run(&home.0, &["raw", DID, "   "]);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn raw_missing_or_ambiguous_credentials_fail_without_sending() {
    let home = Profile::new();
    let output = run(&home.0, &["raw", DID, "get_prop"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("本地凭据"));

    let device = UdpSocket::bind("127.0.0.1:0").unwrap();
    device.set_nonblocking(true).unwrap();
    home.seed(&device.local_addr().unwrap().to_string(), &["1001", "1002"]);
    let output = run(&home.0, &["raw", DID, "set_lightMode", "[1]"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--uid"));
    assert!(device.recv_from(&mut [0u8; 2048]).is_err());
}
