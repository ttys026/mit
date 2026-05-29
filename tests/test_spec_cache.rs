use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mit::spec_cache::{load_spec, model_cache_path, save_spec, sync_model_spec};
use mit::test_support;
use serde_json::json;

#[test]
fn model_cache_path_sanitizes_model_name() {
    let home = make_temp_dir("mit-spec-cache-path");
    let path = model_cache_path(&home, "xiaomi/wifispeaker:lx04");

    assert!(path.ends_with(".mit/cache/specs/models/xiaomi_wifispeaker_lx04.json"));
}

#[test]
fn save_and_load_spec_roundtrip() {
    let home = make_temp_dir("mit-spec-cache-roundtrip");
    let model = "xiaomi.wifispeaker.lx04";
    let payload = json!({
        "type": model,
        "services": [
            { "iid": 2, "type": "urn:miot-spec-v2:service:play-control:0000780D:..." }
        ]
    });

    save_spec(&home, model, &payload).unwrap();
    let loaded = load_spec(&home, model).unwrap().unwrap();

    assert_eq!(loaded["type"], model);
    assert!(loaded["services"].is_array());

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn sync_model_spec_caches_instances_template_and_model_metadata() {
    let _guard = test_support::env_guard();
    let home = make_temp_dir("mit-spec-cache-pipeline");
    let model = "xiaomi.wifispeaker.lx04";
    let old_type = "urn:miot-spec-v2:device:speaker:0000A015:xiaomi-lx04:1";
    let new_type = "urn:miot-spec-v2:device:speaker:0000A015:xiaomi-lx04:2";

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}/instance");

    let instances_body = json!({
        "instances": [
            {"model": model, "type": old_type, "ts": 100},
            {"model": model, "type": new_type, "ts": 200}
        ]
    })
    .to_string();
    let template_body = json!({
        "result": [
            {"model": model, "type": "speaker", "status": "released"}
        ]
    })
    .to_string();
    let spec_body = json!({
        "type": new_type,
        "services": []
    })
    .to_string();

    std::thread::spawn(move || {
        for stream in listener.incoming().take(3) {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            let path = request_line
                .split_whitespace()
                .nth(1)
                .unwrap_or("/")
                .to_string();
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                if header == "\r\n" || header.is_empty() {
                    break;
                }
            }

            let (status, body) = if path.starts_with("/instances?status=all") {
                ("200 OK", instances_body.as_str())
            } else if path.starts_with("/template/list/device") {
                ("200 OK", template_body.as_str())
            } else if path.starts_with("/instance?type=") {
                ("200 OK", spec_body.as_str())
            } else {
                ("404 Not Found", "not found")
            };
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
        }
    });

    let prev = std::env::var_os("MIT_MIOT_SPEC_URL_BASE");
    std::env::set_var("MIT_MIOT_SPEC_URL_BASE", &base_url);

    sync_model_spec(&home, model).unwrap();

    let spec = load_spec(&home, model).unwrap().unwrap();
    assert_eq!(spec["type"], new_type);

    let specs_dir = home.join(".mit").join("cache").join("specs");
    let instances_cache: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(specs_dir.join("sources").join("instances.json")).unwrap(),
    )
    .unwrap();
    assert!(instances_cache["instances"].is_array());
    let template_cache: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(specs_dir.join("sources").join("template_list_device.json")).unwrap(),
    )
    .unwrap();
    assert!(template_cache["result"].is_array());
    let metadata: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(specs_dir.join("index.json")).unwrap()).unwrap();
    assert_eq!(metadata[model]["urn"], new_type);
    assert_eq!(
        metadata[model]["specPath"],
        "models/xiaomi.wifispeaker.lx04.json"
    );
    assert!(metadata[model].get("token").is_none());
    assert!(metadata[model].get("template").is_none());
    assert!(metadata[model].get("instance").is_none());

    if let Some(prev) = prev {
        std::env::set_var("MIT_MIOT_SPEC_URL_BASE", prev);
    } else {
        std::env::remove_var("MIT_MIOT_SPEC_URL_BASE");
    }
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn sync_model_spec_resolves_from_instances_without_device_hints() {
    let _guard = test_support::env_guard();
    let home = make_temp_dir("mit-spec-cache-hints");
    let model = "xiaomi.wifispeaker.lx04";
    let spec_type = "urn:miot-spec-v2:device:speaker:0000A015:xiaomi-lx04:9";

    let specs_dir = home.join(".mit").join("cache").join("specs");
    fs::create_dir_all(&specs_dir).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}/instance");

    let instances_body = json!({
        "instances": [
            {"model": model, "type": spec_type, "ts": 200}
        ]
    })
    .to_string();
    let template_body = json!({
        "result": [{"model": model, "type": "speaker"}]
    })
    .to_string();
    let spec_body = json!({
        "type": spec_type,
        "services": []
    })
    .to_string();

    std::thread::spawn(move || {
        for stream in listener.incoming().take(3) {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            let path = request_line
                .split_whitespace()
                .nth(1)
                .unwrap_or("/")
                .to_string();
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                if header == "\r\n" || header.is_empty() {
                    break;
                }
            }

            let (status, body) = if path.starts_with("/instances?status=all") {
                ("200 OK", instances_body.as_str())
            } else if path.starts_with("/template/list/device") {
                ("200 OK", template_body.as_str())
            } else if path.starts_with("/instance?type=") {
                ("200 OK", spec_body.as_str())
            } else {
                ("404 Not Found", "not found")
            };
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
        }
    });

    let prev = std::env::var_os("MIT_MIOT_SPEC_URL_BASE");
    std::env::set_var("MIT_MIOT_SPEC_URL_BASE", &base_url);

    sync_model_spec(&home, model).unwrap();

    let metadata: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(specs_dir.join("index.json")).unwrap()).unwrap();
    assert_eq!(metadata[model]["urn"], spec_type);
    assert!(metadata[model].get("token").is_none());
    assert_eq!(
        metadata[model]["specPath"],
        "models/xiaomi.wifispeaker.lx04.json"
    );
    assert!(specs_dir.join("sources").join("instances.json").exists());

    if let Some(prev) = prev {
        std::env::set_var("MIT_MIOT_SPEC_URL_BASE", prev);
    } else {
        std::env::remove_var("MIT_MIOT_SPEC_URL_BASE");
    }
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn sync_model_spec_applies_zh_cn_translation_copy_to_cached_spec() {
    let _guard = test_support::env_guard();
    let home = make_temp_dir("mit-spec-cache-zh-cn-translation");
    let model = "xiaomi.wifispeaker.lx04";
    let spec_type = "urn:miot-spec-v2:device:speaker:0000A015:xiaomi-lx04:9";

    let specs_dir = home.join(".mit").join("cache").join("specs");
    fs::create_dir_all(&specs_dir).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}/instance");

    let instances_body = json!({
        "instances": [
            {"model": model, "type": spec_type, "ts": 200}
        ]
    })
    .to_string();
    let template_body = json!({
        "result": [{"model": model, "type": "speaker"}]
    })
    .to_string();
    let spec_body = json!({
        "type": spec_type,
        "description": "Speaker",
        "services": [
            {
                "iid": 2,
                "type": "urn:miot-spec-v2:service:speaker:00007808:xiaomi-lx04:1",
                "description": "Speaker Service",
                "properties": [
                    {
                        "iid": 1,
                        "type": "urn:miot-spec-v2:property:on:00000006:xiaomi-lx04:1",
                        "description": "Power",
                        "format": "bool",
                        "access": ["read", "write"]
                    }
                ]
            }
        ]
    })
    .to_string();
    let translation_body = json!({
        "data": {
            "zh_cn": {
                "service:002": "扬声器服务",
                "service:002:property:001": "电源"
            }
        }
    })
    .to_string();

    let request_count = Arc::new(AtomicUsize::new(0));
    let request_count_for_thread = request_count.clone();
    std::thread::spawn(move || {
        listener.set_nonblocking(true).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while request_count_for_thread.load(Ordering::SeqCst) < 4
            && std::time::Instant::now() < deadline
        {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    request_count_for_thread.fetch_add(1, Ordering::SeqCst);
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut request_line = String::new();
                    reader.read_line(&mut request_line).unwrap();
                    let path = request_line
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("/")
                        .to_string();
                    loop {
                        let mut header = String::new();
                        reader.read_line(&mut header).unwrap();
                        if header == "\r\n" || header.is_empty() {
                            break;
                        }
                    }

                    let (status, body) = if path.starts_with("/instances?status=all") {
                        ("200 OK", instances_body.as_str())
                    } else if path.starts_with("/template/list/device") {
                        ("200 OK", template_body.as_str())
                    } else if path.starts_with("/instance?type=") {
                        ("200 OK", spec_body.as_str())
                    } else if path.starts_with("/instance/v2/multiLanguage?urn=") {
                        ("200 OK", translation_body.as_str())
                    } else {
                        ("404 Not Found", "not found")
                    };
                    let response = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream.write_all(response.as_bytes()).unwrap();
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("unexpected listener error: {err}"),
            }
        }
    });

    let prev = std::env::var_os("MIT_MIOT_SPEC_URL_BASE");
    std::env::set_var("MIT_MIOT_SPEC_URL_BASE", &base_url);

    sync_model_spec(&home, model).unwrap();

    let spec = load_spec(&home, model).unwrap().unwrap();
    assert_eq!(spec["services"][0]["description_trans"], "扬声器服务");
    assert_eq!(
        spec["services"][0]["properties"][0]["description_trans"],
        "电源"
    );
    assert_eq!(request_count.load(Ordering::SeqCst), 4);

    if let Some(prev) = prev {
        std::env::set_var("MIT_MIOT_SPEC_URL_BASE", prev);
    } else {
        std::env::remove_var("MIT_MIOT_SPEC_URL_BASE");
    }
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn sync_model_spec_backfills_translation_for_existing_cached_spec() {
    let _guard = test_support::env_guard();
    let home = make_temp_dir("mit-spec-cache-backfill-translation");
    let model = "xiaomi.wifispeaker.lx04";
    let spec_type = "urn:miot-spec-v2:device:speaker:0000A015:xiaomi-lx04:9";

    save_spec(
        &home,
        model,
        &json!({
            "type": spec_type,
            "description": "Speaker",
            "services": [
                {
                    "iid": 2,
                    "description": "Speaker Service",
                    "properties": [
                        {
                            "iid": 1,
                            "description": "Power",
                            "format": "bool",
                            "access": ["read", "write"]
                        }
                    ]
                }
            ]
        }),
    )
    .unwrap();

    let specs_dir = home.join(".mit").join("cache").join("specs");
    fs::create_dir_all(&specs_dir).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}/instance");

    let instances_body = json!({
        "instances": [
            {"model": model, "type": spec_type, "ts": 200}
        ]
    })
    .to_string();
    let template_body = json!({
        "result": [{"model": model, "type": "speaker"}]
    })
    .to_string();
    let spec_body = json!({
        "type": spec_type,
        "description": "Speaker",
        "services": [
            {
                "iid": 2,
                "description": "Speaker Service",
                "properties": [
                    {
                        "iid": 1,
                        "description": "Power",
                        "format": "bool",
                        "access": ["read", "write"]
                    }
                ]
            }
        ]
    })
    .to_string();
    let translation_body = json!({
        "data": {
            "zh_cn": {
                "service:002": "扬声器服务",
                "service:002:property:001": "电源"
            }
        }
    })
    .to_string();

    std::thread::spawn(move || {
        for stream in listener.incoming().take(4) {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            let path = request_line
                .split_whitespace()
                .nth(1)
                .unwrap_or("/")
                .to_string();
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                if header == "\r\n" || header.is_empty() {
                    break;
                }
            }

            let (status, body) = if path.starts_with("/instances?status=all") {
                ("200 OK", instances_body.as_str())
            } else if path.starts_with("/template/list/device") {
                ("200 OK", template_body.as_str())
            } else if path.starts_with("/instance?type=") {
                ("200 OK", spec_body.as_str())
            } else if path.starts_with("/instance/v2/multiLanguage?urn=") {
                ("200 OK", translation_body.as_str())
            } else {
                ("404 Not Found", "not found")
            };
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
        }
    });

    let prev = std::env::var_os("MIT_MIOT_SPEC_URL_BASE");
    std::env::set_var("MIT_MIOT_SPEC_URL_BASE", &base_url);

    sync_model_spec(&home, model).unwrap();

    let spec = load_spec(&home, model).unwrap().unwrap();
    assert_eq!(spec["services"][0]["description_trans"], "扬声器服务");
    assert_eq!(
        spec["services"][0]["properties"][0]["description_trans"],
        "电源"
    );

    if let Some(prev) = prev {
        std::env::set_var("MIT_MIOT_SPEC_URL_BASE", prev);
    } else {
        std::env::remove_var("MIT_MIOT_SPEC_URL_BASE");
    }
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn sync_model_spec_reuses_cached_instances_and_template_between_models() {
    let _guard = test_support::env_guard();
    let home = make_temp_dir("mit-spec-cache-reuse-shared-sources");
    let model_a = "xiaomi.wifispeaker.lx04";
    let model_b = "xiaomi.wifispeaker.oh2p";
    let type_a = "urn:miot-spec-v2:device:speaker:0000A015:xiaomi-lx04:9";
    let type_b = "urn:miot-spec-v2:device:speaker:0000A015:xiaomi-oh2p:9";

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}/instance");

    let instances_body = json!({
        "instances": [
            {"model": model_a, "type": type_a, "ts": 200},
            {"model": model_b, "type": type_b, "ts": 200}
        ]
    })
    .to_string();
    let template_body = json!({
        "result": [
            {"model": model_a, "type": "speaker"},
            {"model": model_b, "type": "speaker"}
        ]
    })
    .to_string();
    let translation_body = json!({
        "data": {
            "zh_cn": {}
        }
    })
    .to_string();

    let request_paths = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let request_paths_for_thread = request_paths.clone();
    let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let done_for_thread = done.clone();
    let handle = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut request_line = String::new();
                    reader.read_line(&mut request_line).unwrap();
                    let path = request_line
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("/")
                        .to_string();
                    request_paths_for_thread.lock().unwrap().push(path.clone());
                    loop {
                        let mut header = String::new();
                        reader.read_line(&mut header).unwrap();
                        if header == "\r\n" || header.is_empty() {
                            break;
                        }
                    }

                    let (status, body) = if path.starts_with("/instances?status=all") {
                        ("200 OK", instances_body.clone())
                    } else if path.starts_with("/template/list/device") {
                        ("200 OK", template_body.clone())
                    } else if path.starts_with("/instance?type=") {
                        let urn = path
                            .split("type=")
                            .nth(1)
                            .unwrap_or_default()
                            .replace("%3A", ":");
                        let spec_body = json!({
                            "type": urn,
                            "services": []
                        })
                        .to_string();
                        ("200 OK", spec_body)
                    } else if path.starts_with("/instance/v2/multiLanguage?urn=") {
                        ("200 OK", translation_body.clone())
                    } else {
                        ("404 Not Found", "not found".to_string())
                    };
                    let response = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream.write_all(response.as_bytes()).unwrap();
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    if done_for_thread.load(Ordering::SeqCst) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("unexpected listener error: {err}"),
            }
        }
    });

    let prev = std::env::var_os("MIT_MIOT_SPEC_URL_BASE");
    std::env::set_var("MIT_MIOT_SPEC_URL_BASE", &base_url);

    sync_model_spec(&home, model_a).unwrap();
    sync_model_spec(&home, model_b).unwrap();

    done.store(true, Ordering::SeqCst);
    handle.join().unwrap();

    let paths = request_paths.lock().unwrap();
    let instances_requests = paths
        .iter()
        .filter(|path| path.starts_with("/instances?status=all"))
        .count();
    let template_requests = paths
        .iter()
        .filter(|path| path.starts_with("/template/list/device"))
        .count();
    let spec_requests = paths
        .iter()
        .filter(|path| path.starts_with("/instance?type="))
        .count();
    let translation_requests = paths
        .iter()
        .filter(|path| path.starts_with("/instance/v2/multiLanguage?urn="))
        .count();

    assert_eq!(instances_requests, 1, "requests={paths:?}");
    assert_eq!(template_requests, 1, "requests={paths:?}");
    assert_eq!(spec_requests, 2, "requests={paths:?}");
    assert_eq!(translation_requests, 2, "requests={paths:?}");

    if let Some(prev) = prev {
        std::env::set_var("MIT_MIOT_SPEC_URL_BASE", prev);
    } else {
        std::env::remove_var("MIT_MIOT_SPEC_URL_BASE");
    }
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn sync_model_spec_reuses_source_cache_between_models() {
    let _guard = test_support::env_guard();
    let home = make_temp_dir("mit-spec-cache-ttl-zero-refresh");
    let model_a = "xiaomi.wifispeaker.lx04";
    let model_b = "xiaomi.wifispeaker.oh2p";
    let type_a = "urn:miot-spec-v2:device:speaker:0000A015:xiaomi-lx04:9";
    let type_b = "urn:miot-spec-v2:device:speaker:0000A015:xiaomi-oh2p:9";

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}/instance");

    let instances_body = json!({
        "instances": [
            {"model": model_a, "type": type_a, "ts": 200},
            {"model": model_b, "type": type_b, "ts": 200}
        ]
    })
    .to_string();
    let template_body = json!({
        "result": [
            {"model": model_a, "type": "speaker"},
            {"model": model_b, "type": "speaker"}
        ]
    })
    .to_string();
    let translation_body = json!({
        "data": {
            "zh_cn": {}
        }
    })
    .to_string();

    let request_paths = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let request_paths_for_thread = request_paths.clone();
    let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let done_for_thread = done.clone();
    let handle = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut request_line = String::new();
                    reader.read_line(&mut request_line).unwrap();
                    let path = request_line
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("/")
                        .to_string();
                    request_paths_for_thread.lock().unwrap().push(path.clone());
                    loop {
                        let mut header = String::new();
                        reader.read_line(&mut header).unwrap();
                        if header == "\r\n" || header.is_empty() {
                            break;
                        }
                    }

                    let (status, body) = if path.starts_with("/instances?status=all") {
                        ("200 OK", instances_body.clone())
                    } else if path.starts_with("/template/list/device") {
                        ("200 OK", template_body.clone())
                    } else if path.starts_with("/instance?type=") {
                        let urn = path
                            .split("type=")
                            .nth(1)
                            .unwrap_or_default()
                            .replace("%3A", ":");
                        let spec_body = json!({
                            "type": urn,
                            "services": []
                        })
                        .to_string();
                        ("200 OK", spec_body)
                    } else if path.starts_with("/instance/v2/multiLanguage?urn=") {
                        ("200 OK", translation_body.clone())
                    } else {
                        ("404 Not Found", "not found".to_string())
                    };
                    let response = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream.write_all(response.as_bytes()).unwrap();
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    if done_for_thread.load(Ordering::SeqCst) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("unexpected listener error: {err}"),
            }
        }
    });

    let prev_base = std::env::var_os("MIT_MIOT_SPEC_URL_BASE");
    std::env::set_var("MIT_MIOT_SPEC_URL_BASE", &base_url);

    sync_model_spec(&home, model_a).unwrap();
    sync_model_spec(&home, model_b).unwrap();

    done.store(true, Ordering::SeqCst);
    handle.join().unwrap();

    let paths = request_paths.lock().unwrap();
    let instances_requests = paths
        .iter()
        .filter(|path| path.starts_with("/instances?status=all"))
        .count();
    let template_requests = paths
        .iter()
        .filter(|path| path.starts_with("/template/list/device"))
        .count();

    assert_eq!(instances_requests, 1, "requests={paths:?}");
    assert_eq!(template_requests, 1, "requests={paths:?}");

    if let Some(prev) = prev_base {
        std::env::set_var("MIT_MIOT_SPEC_URL_BASE", prev);
    } else {
        std::env::remove_var("MIT_MIOT_SPEC_URL_BASE");
    }
    let _ = fs::remove_dir_all(&home);
}

fn make_temp_dir(prefix: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::current_dir()
        .unwrap()
        .join("target")
        .join("test-artifacts")
        .join(format!("{prefix}-{unique}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}
