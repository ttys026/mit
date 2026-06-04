use anyhow::{anyhow, Result};
use serde_json::Value;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::storage::write_private_text_file;

const SPEC_BASE_URL_DEFAULT: &str = "https://miot-spec.org/miot-spec-v2/instance";
const SPEC_TEMPLATE_LIST_URL_DEFAULT: &str =
    "https://miot-spec.org/miot-spec-v2/template/list/device";
const SPEC_MULTI_LANG_URL_DEFAULT: &str = "https://miot-spec.org/instance/v2/multiLanguage";
const SPEC_BASE_URL_ENV: &str = "MIT_MIOT_SPEC_URL_BASE";
pub fn specs_dir(home: &Path) -> PathBuf {
    home.join(".mit").join("cache").join("specs")
}

pub fn spec_models_dir(home: &Path) -> PathBuf {
    specs_dir(home).join("models")
}

pub fn spec_sources_dir(home: &Path) -> PathBuf {
    specs_dir(home).join("sources")
}

fn source_cache_path(home: &Path, file_name: &str) -> PathBuf {
    spec_sources_dir(home).join(file_name)
}

pub fn model_cache_path(home: &Path, model: &str) -> PathBuf {
    let sanitized = sanitize_model_name(model);
    spec_models_dir(home).join(format!("{sanitized}.json"))
}

pub fn model_index_path(home: &Path) -> PathBuf {
    specs_dir(home).join("index.json")
}

fn sanitize_model_name(model: &str) -> String {
    model
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
}

pub fn load_spec(home: &Path, model: &str) -> Result<Option<Value>> {
    let path = model_cache_path(home, model);
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path)?;
    Ok(Some(serde_json::from_str(&raw)?))
}

pub fn save_spec(home: &Path, model: &str, payload: &Value) -> Result<PathBuf> {
    let path = model_cache_path(home, model);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut normalized = payload.clone();
    if normalized.get("type").is_none() {
        normalized["type"] = Value::String(model.trim().to_string());
    }
    let mut text = serde_json::to_string_pretty(&normalized)?;
    text.push('\n');
    write_private_text_file(&path, &text)?;
    Ok(path)
}

pub fn sync_model_spec(home: &Path, model: &str) -> Result<PathBuf> {
    if let Some(cached) = load_spec(home, model)? {
        if spec_has_translated_copy(&cached) && !spec_needs_value_list_translation(&cached) {
            return Ok(model_cache_path(home, model));
        }
    }
    let model = model.trim();
    if model.is_empty() {
        return Err(anyhow!("model 不能为空"));
    }
    let base_url =
        env::var(SPEC_BASE_URL_ENV).unwrap_or_else(|_| SPEC_BASE_URL_DEFAULT.to_string());
    let (instance_url, instances_url, template_url, multi_lang_url) = resolve_spec_urls(&base_url);

    let instances = load_or_fetch_cached_source(
        home,
        "instances.json",
        &instances_url,
        "获取设备规格索引失败",
    )?;
    let spec_type = resolve_spec_type_from_instances(model, &instances)?;

    // Prime template index cache; current sync path doesn't consume it directly yet.
    load_or_fetch_cached_source(
        home,
        "template_list_device.json",
        &template_url,
        "获取设备模板索引失败",
    )?;

    let mut payload = fetch_json(
        &format!("{instance_url}?type={}", urlencoding::encode(&spec_type)),
        "获取设备规格失败",
    )?;
    if !payload.is_object() {
        return Err(anyhow!("设备规格格式错误: 返回值不是对象"));
    }
    if let Ok(multi_lang) = fetch_json(
        &format!("{multi_lang_url}?urn={}", urlencoding::encode(&spec_type)),
        "获取设备规格中文翻译失败",
    ) {
        apply_zh_cn_translation(&mut payload, &multi_lang);
    }
    update_model_index(home, model, &spec_type)?;
    save_spec(home, model, &payload)
}

pub fn fetch_remote_spec(model: &str) -> Result<Value> {
    let model = model.trim();
    if model.is_empty() {
        return Err(anyhow!("model 不能为空"));
    }
    let base_url =
        env::var(SPEC_BASE_URL_ENV).unwrap_or_else(|_| SPEC_BASE_URL_DEFAULT.to_string());
    let spec_type = if model.starts_with("urn:") {
        model.to_string()
    } else {
        let instances_url = if let Some(prefix) = base_url.strip_suffix("/instance") {
            format!("{prefix}/instances?status=all")
        } else {
            default_instances_url()
        };
        let instances_response = reqwest::blocking::Client::new().get(instances_url).send()?;
        let instances_status = instances_response.status();
        let instances_text = instances_response.text()?;
        if !instances_status.is_success() {
            return Err(anyhow!(
                "获取设备规格索引失败: status={} body={}",
                instances_status.as_u16(),
                instances_text.trim()
            ));
        }
        let instances: Value = serde_json::from_str(&instances_text)?;
        resolve_spec_type_from_instances(model, &instances)?
    };
    let url = format!("{base_url}?type={}", urlencoding::encode(&spec_type));
    let response = reqwest::blocking::Client::new().get(url).send()?;
    let status = response.status();
    let text = response.text()?;
    if !status.is_success() {
        return Err(anyhow!(
            "获取设备规格失败: status={} body={}",
            status.as_u16(),
            text.trim()
        ));
    }
    let payload: Value = serde_json::from_str(&text)?;
    if !payload.is_object() {
        return Err(anyhow!("设备规格格式错误: 返回值不是对象"));
    }
    Ok(payload)
}

fn resolve_spec_type_from_instances(model_or_type: &str, instances: &Value) -> Result<String> {
    let model_or_type = model_or_type.trim();
    if model_or_type.starts_with("urn:") {
        return Ok(model_or_type.to_string());
    }
    let Some(items) = instances.get("instances").and_then(Value::as_array) else {
        return Err(anyhow!("设备规格索引格式错误: 缺少 instances 数组"));
    };
    let entry = items
        .iter()
        .filter(|item| {
            item.get("model")
                .and_then(Value::as_str)
                .map(str::trim)
                .map(|value| value == model_or_type)
                .unwrap_or(false)
        })
        .max_by_key(|item| instance_ts(item));
    let Some(entry) = entry else {
        return Err(anyhow!("设备规格索引中未找到 model={model_or_type}"));
    };
    let Some(spec_type) = entry.get("type").and_then(Value::as_str).map(str::trim) else {
        return Err(anyhow!("设备规格索引条目缺少 type: model={model_or_type}"));
    };
    if !spec_type.starts_with("urn:") {
        return Err(anyhow!("设备规格索引条目 type 无效: model={model_or_type}"));
    }
    Ok(spec_type.to_string())
}

fn instance_ts(entry: &Value) -> i64 {
    entry
        .get("ts")
        .and_then(Value::as_i64)
        .or_else(|| {
            entry
                .get("ts")
                .and_then(Value::as_str)
                .and_then(|text| text.parse::<i64>().ok())
        })
        .unwrap_or(0)
}

fn resolve_spec_urls(base_url: &str) -> (String, String, String, String) {
    let multi_lang_url = resolve_multi_lang_url(base_url);
    if let Some(prefix) = base_url.strip_suffix("/instance") {
        (
            base_url.to_string(),
            format!("{prefix}/instances?status=all"),
            format!("{prefix}/template/list/device"),
            multi_lang_url,
        )
    } else {
        (
            base_url.to_string(),
            default_instances_url(),
            SPEC_TEMPLATE_LIST_URL_DEFAULT.to_string(),
            multi_lang_url,
        )
    }
}

fn resolve_multi_lang_url(base_url: &str) -> String {
    if let Some(prefix) = base_url.strip_suffix("/miot-spec-v2/instance") {
        return format!("{prefix}/instance/v2/multiLanguage");
    }
    if let Some(prefix) = base_url.strip_suffix("/instance") {
        return format!("{prefix}/instance/v2/multiLanguage");
    }
    SPEC_MULTI_LANG_URL_DEFAULT.to_string()
}

fn default_instances_url() -> String {
    if let Some(prefix) = SPEC_BASE_URL_DEFAULT.strip_suffix("/instance") {
        format!("{prefix}/instances?status=all")
    } else {
        "https://miot-spec.org/miot-spec-v2/instances?status=all".to_string()
    }
}

fn fetch_json(url: &str, context: &str) -> Result<Value> {
    let response = reqwest::blocking::Client::new().get(url).send()?;
    let status = response.status();
    let text = response.text()?;
    if !status.is_success() {
        return Err(anyhow!(
            "{context}: status={} body={}",
            status.as_u16(),
            text.trim()
        ));
    }
    Ok(serde_json::from_str(&text)?)
}

fn load_or_fetch_cached_source(
    home: &Path,
    file_name: &str,
    url: &str,
    context: &str,
) -> Result<Value> {
    let path = source_cache_path(home, file_name);
    if path.exists() {
        let text = fs::read_to_string(&path)?;
        return Ok(serde_json::from_str(&text)?);
    }

    let payload = fetch_json(url, context)?;
    cache_json(&spec_sources_dir(home), file_name, &payload)?;
    Ok(payload)
}

fn cache_json(base_dir: &Path, file_name: &str, payload: &Value) -> Result<PathBuf> {
    let path = base_dir.join(file_name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut text = serde_json::to_string_pretty(payload)?;
    text.push('\n');
    write_private_text_file(&path, &text)?;
    Ok(path)
}

fn update_model_index(home: &Path, model: &str, spec_type: &str) -> Result<()> {
    let index_path = model_index_path(home);
    let mut index = if index_path.exists() {
        let text = fs::read_to_string(&index_path)?;
        serde_json::from_str::<Value>(&text)?
    } else {
        Value::Object(serde_json::Map::new())
    };
    if !index.is_object() {
        index = Value::Object(serde_json::Map::new());
    }

    index[model] = serde_json::json!({
        "model": model,
        "urn": spec_type,
        "specPath": format!("models/{}.json", sanitize_model_name(model)),
        "updatedAt": unix_timestamp()
    });

    if let Some(parent) = index_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut text = serde_json::to_string_pretty(&index)?;
    text.push('\n');
    write_private_text_file(&index_path, &text)?;
    Ok(())
}

fn unix_timestamp() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn apply_zh_cn_translation(spec: &mut Value, multi_lang: &Value) {
    let Some(translation_map) = multi_lang
        .get("data")
        .and_then(Value::as_object)
        .and_then(|data| data.get("zh_cn"))
        .and_then(Value::as_object)
    else {
        return;
    };

    let Some(services) = spec.get_mut("services").and_then(Value::as_array_mut) else {
        return;
    };

    for service in services {
        let siid = service.get("iid").and_then(Value::as_i64).unwrap_or(0);
        if siid <= 0 {
            continue;
        }
        if let Some(text) = translated_text(translation_map, format!("service:{siid}").as_str()) {
            service["description_trans"] = Value::String(text);
        }
        if let Some(properties) = service.get_mut("properties").and_then(Value::as_array_mut) {
            for property in properties {
                let piid = property.get("iid").and_then(Value::as_i64).unwrap_or(0);
                if piid <= 0 {
                    continue;
                }
                if let Some(text) = translated_text(
                    translation_map,
                    format!("service:{siid}:property:{piid}").as_str(),
                ) {
                    property["description_trans"] = Value::String(text);
                }
                if let Some(value_list) =
                    property.get_mut("value-list").and_then(Value::as_array_mut)
                {
                    for (index, entry) in value_list.iter_mut().enumerate() {
                        if let Some(text) = translated_text(
                            translation_map,
                            format!("service:{siid}:property:{piid}:valuelist:{index}").as_str(),
                        ) {
                            entry["description_trans"] = Value::String(text);
                        }
                    }
                }
            }
        }
        if let Some(events) = service.get_mut("events").and_then(Value::as_array_mut) {
            for event in events {
                let eiid = event.get("iid").and_then(Value::as_i64).unwrap_or(0);
                if eiid <= 0 {
                    continue;
                }
                if let Some(text) = translated_text(
                    translation_map,
                    format!("service:{siid}:event:{eiid}").as_str(),
                ) {
                    event["description_trans"] = Value::String(text);
                }
            }
        }
        if let Some(actions) = service.get_mut("actions").and_then(Value::as_array_mut) {
            for action in actions {
                let aiid = action.get("iid").and_then(Value::as_i64).unwrap_or(0);
                if aiid <= 0 {
                    continue;
                }
                if let Some(text) = translated_text(
                    translation_map,
                    format!("service:{siid}:action:{aiid}").as_str(),
                ) {
                    action["description_trans"] = Value::String(text);
                }
            }
        }
    }
}

fn translated_text(translation_map: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    translation_map
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            let padded = pad_miot_translation_key(key);
            if padded == key {
                return None;
            }
            translation_map
                .get(padded.as_str())
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
        })
}

fn pad_miot_translation_key(key: &str) -> String {
    key.split(':')
        .map(|part| {
            part.parse::<u32>()
                .map(|value| format!("{value:03}"))
                .unwrap_or_else(|_| part.to_string())
        })
        .collect::<Vec<_>>()
        .join(":")
}

fn spec_has_translated_copy(spec: &Value) -> bool {
    has_description_trans(spec)
        || spec
            .get("services")
            .and_then(Value::as_array)
            .is_some_and(|services| {
                services.iter().any(|service| {
                    has_description_trans(service)
                        || service
                            .get("properties")
                            .and_then(Value::as_array)
                            .is_some_and(|properties| {
                                properties.iter().any(|property| {
                                    has_description_trans(property)
                                        || property
                                            .get("value-list")
                                            .and_then(Value::as_array)
                                            .is_some_and(|items| {
                                                items.iter().any(has_description_trans)
                                            })
                                })
                            })
                        || service
                            .get("events")
                            .and_then(Value::as_array)
                            .is_some_and(|events| events.iter().any(has_description_trans))
                        || service
                            .get("actions")
                            .and_then(Value::as_array)
                            .is_some_and(|actions| actions.iter().any(has_description_trans))
                })
            })
}

fn spec_needs_value_list_translation(spec: &Value) -> bool {
    spec.get("services")
        .and_then(Value::as_array)
        .is_some_and(|services| {
            services.iter().any(|service| {
                service
                    .get("properties")
                    .and_then(Value::as_array)
                    .is_some_and(|properties| {
                        properties.iter().any(|property| {
                            property
                                .get("value-list")
                                .and_then(Value::as_array)
                                .is_some_and(|items| {
                                    items.iter().any(|item| !has_description_trans(item))
                                })
                        })
                    })
            })
        })
}

fn has_description_trans(node: &Value) -> bool {
    node.get("description_trans")
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
}

#[cfg(test)]
#[path = "../tests/module_tests/spec_cache.rs"]
mod tests;
