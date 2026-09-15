//! MIoT spec parsing: readable properties, actions, value coercion, device categories.
use anyhow::{bail, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;

use crate::spec_cache::specs_dir;
use crate::storage::Language;

use super::{lang_str, spec_node_label, ActionItem, PropItem, PropValueOption};

pub(in crate::tui) fn collect_readable_props(spec: &Value, lang: Language) -> Vec<PropItem> {
    let mut out = Vec::new();
    let Some(services) = spec.get("services").and_then(Value::as_array) else {
        return out;
    };
    for service in services {
        let siid = service.get("iid").and_then(Value::as_i64).unwrap_or(0);
        let service_name = spec_node_label(service, lang)
            .map(str::trim)
            .filter(|text| !text.is_empty());
        let Some(properties) = service.get("properties").and_then(Value::as_array) else {
            continue;
        };
        for prop in properties {
            let format = prop
                .get("format")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let access = prop
                .get("access")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let readable = access.iter().filter_map(Value::as_str).any(|value| {
                value.eq_ignore_ascii_case("read") || value.eq_ignore_ascii_case("rw")
            });
            if !readable {
                continue;
            }
            let writable = access.iter().filter_map(Value::as_str).any(|value| {
                value.eq_ignore_ascii_case("write") || value.eq_ignore_ascii_case("rw")
            });
            let value_options = prop
                .get("value-list")
                .and_then(Value::as_array)
                .map(|options| {
                    options
                        .iter()
                        .filter_map(|entry| {
                            let value = entry.get("value")?.clone();
                            let label = spec_node_label(entry, lang)
                                .map(str::trim)
                                .filter(|text| !text.is_empty())
                                .map(ToString::to_string)
                                .unwrap_or_else(|| {
                                    serde_json::to_string(&value)
                                        .unwrap_or_else(|_| "null".to_string())
                                });
                            Some(PropValueOption { value, label })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let piid = prop.get("iid").and_then(Value::as_i64).unwrap_or(0);
            if siid <= 0 || piid <= 0 {
                continue;
            }
            let prop_name = spec_node_label(prop, lang)
                .map(str::trim)
                .unwrap_or_default()
                .to_string();
            let name = if let Some(service_name) = service_name {
                if prop_name.is_empty() {
                    service_name.to_string()
                } else {
                    format!("{service_name} / {prop_name}")
                }
            } else if prop_name.is_empty() {
                format!("property {siid}/{piid}")
            } else {
                prop_name
            };
            out.push(PropItem {
                siid,
                piid,
                name,
                format: format.to_string(),
                writable,
                value_options,
            });
        }
    }
    out
}

#[cfg(test)]
pub(in crate::tui) fn parse_bool_prop_value(raw: &Value) -> Option<bool> {
    let first = extract_prop_value(raw)?;
    match first {
        Value::Bool(value) => Some(value),
        Value::Number(n) => Some(n.as_i64().unwrap_or(0) != 0),
        Value::String(text) => match text.trim() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

pub(in crate::tui) fn extract_prop_value(raw: &Value) -> Option<Value> {
    let first = if let Some(array_value) = raw
        .as_array()
        .and_then(|list| list.first())
        .and_then(|entry| entry.get("value"))
        .cloned()
    {
        array_value
    } else if let Some(object_value) = raw.get("value").cloned() {
        object_value
    } else {
        raw.clone()
    };
    Some(first)
}

pub(in crate::tui) fn extract_actions_from_spec(spec: &Value, lang: Language) -> Vec<ActionItem> {
    let mut actions = Vec::new();
    if let Some(services) = spec.get("services").and_then(|v| v.as_array()) {
        for service in services {
            let siid = service.get("iid").and_then(|v| v.as_i64()).unwrap_or(0);
            let property_by_iid = service
                .get("properties")
                .and_then(Value::as_array)
                .map(|properties| {
                    properties
                        .iter()
                        .filter_map(|p| {
                            let piid = p.get("iid").and_then(Value::as_i64)?;
                            let name = spec_node_label(p, lang)
                                .map(str::trim)
                                .filter(|text| !text.is_empty())
                                .unwrap_or("")
                                .to_string();
                            let format = p
                                .get("format")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string();
                            let value_options = p
                                .get("value-list")
                                .and_then(Value::as_array)
                                .map(|options| {
                                    options
                                        .iter()
                                        .filter_map(|entry| {
                                            let value = entry.get("value")?.clone();
                                            let label = spec_node_label(entry, lang)
                                                .map(str::trim)
                                                .filter(|text| !text.is_empty())
                                                .map(ToString::to_string)
                                                .unwrap_or_else(|| {
                                                    serde_json::to_string(&value)
                                                        .unwrap_or_else(|_| "null".to_string())
                                                });
                                            Some(PropValueOption { value, label })
                                        })
                                        .collect::<Vec<_>>()
                                })
                                .unwrap_or_default();
                            Some((
                                piid,
                                PropItem {
                                    siid,
                                    piid,
                                    name,
                                    format,
                                    writable: false,
                                    value_options,
                                },
                            ))
                        })
                        .collect::<HashMap<i64, PropItem>>()
                })
                .unwrap_or_default();
            if let Some(service_actions) = service.get("actions").and_then(|v| v.as_array()) {
                for action in service_actions {
                    if let Some(aiid) = action.get("iid").and_then(|v| v.as_i64()) {
                        let name = spec_node_label(action, lang)
                            .unwrap_or(lang_str(lang, "未知操作", "Unknown Action"))
                            .to_string();
                        let input_piids = action
                            .get("in")
                            .and_then(|v| v.as_array())
                            .map(|values| {
                                values.iter().filter_map(Value::as_i64).collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        let input_labels = input_piids
                            .iter()
                            .enumerate()
                            .map(|(idx, piid)| {
                                property_by_iid
                                    .get(piid)
                                    .map(|prop| prop.name.clone())
                                    .unwrap_or_else(|| format!("参数{}", idx + 1))
                            })
                            .collect::<Vec<_>>();
                        let input_props = input_piids
                            .iter()
                            .enumerate()
                            .map(|(idx, piid)| {
                                property_by_iid.get(piid).cloned().unwrap_or(PropItem {
                                    siid,
                                    piid: *piid,
                                    name: input_labels[idx].clone(),
                                    format: String::new(),
                                    writable: false,
                                    value_options: Vec::new(),
                                })
                            })
                            .collect::<Vec<_>>();
                        actions.push(ActionItem {
                            siid,
                            aiid,
                            name,
                            input_piids,
                            input_labels,
                            input_props,
                        });
                    }
                }
            }
        }
    }
    actions
}

pub(in crate::tui) fn parse_prop_input_value(text: &str) -> Result<Value> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        bail!("输入不能为空");
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Ok(value);
    }
    Ok(Value::String(trimmed.to_string()))
}

pub(in crate::tui) fn format_prop_value_for_dialog(value: &Value) -> String {
    if is_error_with_negative_code(value) {
        return "-".to_string();
    }
    if let Some(text) = value.as_str() {
        return decode_backslash_x_utf8(text).unwrap_or_else(|| text.to_string());
    }
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

/// Check if a value is an error object with a negative error code.
/// Returns true if value is an object with "code" < 0 and a non-empty "did" string.
pub(in crate::tui) fn is_error_with_negative_code(value: &Value) -> bool {
    if let Some(obj) = value.as_object() {
        let has_did = obj
            .get("did")
            .and_then(Value::as_str)
            .is_some_and(|did| !did.trim().is_empty());
        let code_is_negative = obj
            .get("code")
            .and_then(Value::as_i64)
            .is_some_and(|code| code < 0);
        return has_did && code_is_negative;
    }
    false
}

fn decode_backslash_x_utf8(text: &str) -> Option<String> {
    if !text.contains("\\x") {
        return None;
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0_usize;
    let mut decoded_any = false;
    while index < bytes.len() {
        if bytes[index] == b'\\'
            && index + 3 < bytes.len()
            && matches!(bytes[index + 1], b'x' | b'X')
            && bytes[index + 2].is_ascii_hexdigit()
            && bytes[index + 3].is_ascii_hexdigit()
        {
            let high = hex_nibble(bytes[index + 2])?;
            let low = hex_nibble(bytes[index + 3])?;
            out.push((high << 4) | low);
            index += 4;
            decoded_any = true;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    if !decoded_any {
        return None;
    }
    String::from_utf8(out).ok()
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub(in crate::tui) fn read_device_categories_from_template(
    home_dir: &std::path::Path,
    lang: Language,
) -> Result<HashMap<String, String>> {
    let specs = specs_dir(home_dir);
    let template_path = specs.join("sources").join("template_list_device.json");
    if !template_path.exists() {
        return Ok(HashMap::new());
    }
    let template_raw = fs::read_to_string(template_path)?;
    let template_payload: Value = serde_json::from_str(&template_raw)?;
    let Some(template_entries) = template_payload.get("result").and_then(Value::as_array) else {
        return Ok(HashMap::new());
    };

    let mut direct_model_categories = HashMap::new();
    let mut type_categories = HashMap::new();
    for entry in template_entries {
        let category = category_from_template_entry(entry, lang).unwrap_or_else(|| "-".to_string());

        if let Some(model) = entry
            .get("model")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            direct_model_categories.insert(model.to_string(), category.clone());
        }

        if let Some(urn_type) = entry
            .get("type")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            type_categories.insert(urn_type.to_string(), category);
        }
    }

    let model_index_path = specs.join("index.json");
    if !model_index_path.exists() || type_categories.is_empty() {
        return Ok(direct_model_categories);
    }
    let model_index_raw = fs::read_to_string(model_index_path)?;
    let model_index_payload: Value = serde_json::from_str(&model_index_raw)?;
    let Some(model_entries) = model_index_payload.as_object() else {
        return Ok(direct_model_categories);
    };

    let mut resolved = direct_model_categories;
    for (model, metadata) in model_entries {
        if resolved.contains_key(model) {
            continue;
        }
        let Some(urn) = metadata
            .get("urn")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        if let Some((_, category)) = type_categories
            .iter()
            .filter(|(template_urn, _)| urn.starts_with(template_urn.as_str()))
            .max_by_key(|(template_urn, _)| template_urn.len())
        {
            resolved.insert(model.clone(), category.clone());
        }
    }
    Ok(resolved)
}

fn category_from_template_entry(entry: &Value, lang: Language) -> Option<String> {
    let keys: &[&str] = match lang {
        Language::Chinese => &["zh_cn", "en"],
        Language::English => &["en", "zh_cn"],
    };
    entry
        .get("description")
        .and_then(Value::as_object)
        .and_then(|description| {
            keys.iter()
                .find_map(|key| description.get(*key).and_then(Value::as_str))
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            [
                "category_name",
                "categoryName",
                "category",
                "type_name",
                "typeName",
                "type",
            ]
            .into_iter()
            .find_map(|key| {
                entry
                    .get(key)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string)
            })
        })
}
