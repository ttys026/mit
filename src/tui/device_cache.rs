//! Per-account device list caching, local-credential snapshots, MIoT spec
//! sync for unknown models, and device list merge/sort/tagging helpers.
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;

use crate::mico_api::Device;
use crate::spec_cache::{load_spec, specs_dir, sync_model_spec};
use crate::storage::{get_auth_accounts, normalize_auth, write_private_text_file, Language};

use super::{read_device_categories_from_template, CACHE_ACCOUNT_PREFIX};

pub(in crate::tui) fn unique_device_models(devices: &[Device]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut models = Vec::new();
    for device in devices {
        let model = device.model.trim();
        if model.is_empty() {
            continue;
        }
        if seen.insert(model.to_string()) {
            models.push(model.to_string());
        }
    }
    models
}

pub(in crate::tui) fn device_models_missing_local_specs(
    home_dir: &std::path::Path,
    devices: &[Device],
) -> Vec<String> {
    let models = unique_device_models(devices);
    if models.is_empty() {
        return models;
    }

    let specs = specs_dir(home_dir);
    if !specs
        .join("sources")
        .join("template_list_device.json")
        .exists()
        || !specs.join("index.json").exists()
    {
        return models;
    }

    models
        .into_iter()
        .filter(|model| !matches!(load_spec(home_dir, model), Ok(Some(_))))
        .collect()
}

pub(in crate::tui) fn sync_specs_for_models(
    home_dir: &std::path::Path,
    models: &[String],
) -> Vec<String> {
    let mut logs = Vec::new();
    let mut synced = 0_usize;
    for model in models {
        match sync_model_spec(home_dir, model.as_str()) {
            Ok(path) => {
                logs.push(format!("spec cached: {} -> {}", model, path.display()));
                synced += 1;
            }
            Err(error) => {
                logs.push(format!("spec sync failed {}: {}", model, error));
            }
        }
    }
    logs.push(format!("spec sync completed ({synced} models)"));
    logs
}

pub(in crate::tui) fn read_device_categories(
    home_dir: &std::path::Path,
    lang: Language,
) -> HashMap<String, String> {
    let mut categories = read_device_categories_from_cached_devices(home_dir).unwrap_or_default();
    categories.extend(read_device_categories_from_template(home_dir, lang).unwrap_or_default());
    categories
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(in crate::tui) struct CachedDevicesPayload {
    #[serde(default)]
    devices: Vec<Device>,
    #[serde(default)]
    categories: HashMap<String, String>,
}

pub(in crate::tui) fn local_credentials_snapshot_path(
    home_dir: &std::path::Path,
    uid: &str,
) -> Option<std::path::PathBuf> {
    let uid = uid.trim();
    if uid.is_empty() {
        return None;
    }
    Some(
        home_dir
            .join(".mit")
            .join("accounts")
            .join(uid)
            .join("local_credentials.json"),
    )
}

pub(in crate::tui) fn load_cached_devices_from_home(
    home_dir: &std::path::Path,
    uid: &str,
) -> Result<Vec<Device>> {
    let auth_path = home_dir.join(".mit").join("auth.json");
    if !auth_path.exists() {
        return Ok(Vec::new());
    }
    let auth_text = fs::read_to_string(auth_path)?;
    let auth_value: Value = serde_json::from_str(&auth_text)?;
    let auth_state = normalize_auth(auth_value)?;
    let accounts = get_auth_accounts(&auth_state)?;

    let target_accounts = if uid.trim().is_empty() {
        accounts
            .into_iter()
            .filter(|account| !account.user.uid.trim().is_empty())
            .collect::<Vec<_>>()
    } else {
        accounts
            .into_iter()
            .filter(|account| account.user.uid == uid)
            .collect::<Vec<_>>()
    };

    let mut devices = Vec::new();
    for account in target_accounts {
        let account_uid = account.user.uid.clone();
        if account_uid.trim().is_empty() {
            continue;
        }

        // devices.json is the only cached source of device metadata (name/model/room).
        // local_credentials.json now stores only local transport credentials.
        if let Ok(cached) = load_cached_devices_for_account(home_dir, &account_uid) {
            if !cached.is_empty() {
                devices.extend(cached);
            }
        }
    }

    Ok(devices)
}

pub(in crate::tui) fn cache_devices_for_account(
    home_dir: &std::path::Path,
    uid: &str,
    devices: &[Device],
    categories: &HashMap<String, String>,
) -> Result<()> {
    let uid = uid.trim();
    if uid.is_empty() {
        return Ok(());
    }
    let path = home_dir
        .join(".mit")
        .join("accounts")
        .join(uid)
        .join("devices.json");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let devices_by_uid: Vec<Device> = devices
        .iter()
        .filter(|d| d.home_id == format!("{CACHE_ACCOUNT_PREFIX}{uid}"))
        .cloned()
        .collect();
    let categories_by_uid = devices_by_uid
        .iter()
        .filter_map(|device| {
            categories
                .get(device.model.as_str())
                .map(|category| (device.model.clone(), category.clone()))
        })
        .collect::<HashMap<_, _>>();
    let text = serde_json::to_string_pretty(&CachedDevicesPayload {
        devices: devices_by_uid,
        categories: categories_by_uid,
    })?;
    write_private_text_file(&path, &format!("{}\n", text))?;
    Ok(())
}

pub(in crate::tui) fn load_cached_devices_for_account(
    home_dir: &std::path::Path,
    uid: &str,
) -> Result<Vec<Device>> {
    let path = home_dir
        .join(".mit")
        .join("accounts")
        .join(uid.trim())
        .join("devices.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(path)?;
    let value: Value = serde_json::from_str(&text)?;
    if value.is_array() {
        return Ok(serde_json::from_value(value)?);
    }
    if let Some(devices) = value.get("devices") {
        return Ok(serde_json::from_value(devices.clone())?);
    }
    Ok(Vec::new())
}

pub(in crate::tui) fn cache_devices_for_accounts(
    home_dir: &std::path::Path,
    devices: &[Device],
    categories: &HashMap<String, String>,
) -> Result<()> {
    let account_uids = devices
        .iter()
        .filter_map(device_account_uid)
        .map(ToString::to_string)
        .collect::<HashSet<_>>();
    for uid in account_uids {
        cache_devices_for_account(home_dir, uid.as_str(), devices, categories)?;
    }
    Ok(())
}

pub(in crate::tui) fn read_device_categories_from_cached_devices(
    home_dir: &std::path::Path,
) -> Result<HashMap<String, String>> {
    let accounts_dir = home_dir.join(".mit").join("accounts");
    if !accounts_dir.exists() {
        return Ok(HashMap::new());
    }
    let mut categories = HashMap::new();
    for entry in fs::read_dir(accounts_dir)? {
        let entry = entry?;
        let path = entry.path().join("devices.json");
        if !path.exists() {
            continue;
        }
        let text = fs::read_to_string(path)?;
        let value: Value = serde_json::from_str(&text)?;
        let Some(category_map) = value.get("categories").and_then(Value::as_object) else {
            continue;
        };
        for (model, category) in category_map {
            let Some(category) = category
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            categories.insert(model.clone(), category.to_string());
        }
    }
    Ok(categories)
}

pub(in crate::tui) fn tag_devices_with_account(
    devices: Vec<Device>,
    account_uid: &str,
    account_label: &str,
) -> Vec<Device> {
    devices
        .into_iter()
        .map(|mut device| {
            device.home_id = format!("{CACHE_ACCOUNT_PREFIX}{account_uid}");
            device.home_name = account_label.to_string();
            device
        })
        .collect()
}

pub(in crate::tui) fn device_account_uid(device: &Device) -> Option<&str> {
    device
        .home_id
        .strip_prefix(CACHE_ACCOUNT_PREFIX)
        .map(str::trim)
        .filter(|uid| !uid.is_empty())
}

pub(in crate::tui) fn merge_devices(into: &mut Vec<Device>, extra: Vec<Device>) {
    let mut seen = into
        .iter()
        .map(|device| format!("{}::{}", device.home_id, device.did))
        .collect::<HashSet<_>>();
    for device in extra {
        let key = format!("{}::{}", device.home_id, device.did);
        if seen.insert(key) {
            into.push(device);
        }
    }
}

pub(in crate::tui) fn sort_devices_by_room(devices: &mut [Device]) {
    devices.sort_by(|a, b| a.room_name.cmp(&b.room_name).then(a.name.cmp(&b.name)));
}
