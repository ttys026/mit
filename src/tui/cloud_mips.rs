//! Cloud MIPS (MQTT) property-change listeners: lifecycle, liveness tracking,
//! incoming-message processing, and applying cached updates to the prop dialog.
use std::collections::HashSet;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use crate::mips_cloud::{
    config_from_account, start_property_cache_listener, CloudMipsHandle, CloudMipsStatus,
};
use crate::storage::AuthAccount;

use super::pages::account as account_page;
use super::{device_account_uid, extract_prop_value, TuiApp, CLOUD_MIPS_RESPONSE_STALE_THRESHOLD};

pub(in crate::tui) struct CloudMipsRuntime {
    pub(in crate::tui) key: String,
    pub(in crate::tui) _handles: Vec<CloudMipsHandle>,
    pub(in crate::tui) rx: Receiver<CloudMipsStatus>,
    pub(in crate::tui) last_mqtt_response_at: Option<Instant>,
    pub(in crate::tui) last_ping_req_at: Option<Instant>,
    pub(in crate::tui) last_ping_resp_at: Option<Instant>,
}

pub(in crate::tui) static CLOUD_MIPS_RUNTIME: OnceLock<Mutex<Option<CloudMipsRuntime>>> =
    OnceLock::new();

pub(in crate::tui) fn cloud_mips_runtime() -> &'static Mutex<Option<CloudMipsRuntime>> {
    CLOUD_MIPS_RUNTIME.get_or_init(|| Mutex::new(None))
}

pub(in crate::tui) fn update_cloud_mips_runtime_liveness(
    runtime: &mut CloudMipsRuntime,
    status: &CloudMipsStatus,
    now: Instant,
) {
    match status {
        CloudMipsStatus::MessageReceived { .. } | CloudMipsStatus::PropertyApplied { .. } => {
            runtime.last_mqtt_response_at = Some(now);
        }
        CloudMipsStatus::Started { .. } => {}
        CloudMipsStatus::EventReceived { direction, summary } => {
            if direction == "outgoing" && summary.contains("PingReq") {
                runtime.last_ping_req_at = Some(now);
            }
            if direction == "incoming" {
                runtime.last_mqtt_response_at = Some(now);
                if summary.contains("PingResp") {
                    runtime.last_ping_resp_at = Some(now);
                }
            }
        }
        CloudMipsStatus::IgnoredMessage { .. }
        | CloudMipsStatus::AuthRejected { .. }
        | CloudMipsStatus::Error { .. }
        | CloudMipsStatus::Stopped => {}
    }
}

pub(in crate::tui) fn cloud_mips_runtime_key(groups: &[(AuthAccount, Vec<String>)]) -> String {
    let mut entries = groups
        .iter()
        .map(|(account, dids)| {
            let mut dids = dids.clone();
            dids.sort();
            format!(
                "{}:{}:{}:{}",
                account.user.uid,
                account.uuid,
                account.access_token,
                dids.join(",")
            )
        })
        .collect::<Vec<_>>();
    entries.sort();
    entries.join("|")
}

pub(in crate::tui) fn cloud_mips_disabled_by_user() -> bool {
    std::env::var("MIT_DISABLE_CLOUD_MIPS")
        .ok()
        .is_some_and(|value| !value.trim().is_empty())
}

pub(in crate::tui) fn cloud_mips_disabled() -> bool {
    if cloud_mips_disabled_by_user() {
        return true;
    }
    cfg!(test) && std::env::var("MIT_ENABLE_CLOUD_MIPS_IN_TESTS").is_err()
}

impl TuiApp {
    pub(in crate::tui) fn check_cloud_mips_stale_after_operation(&mut self) {
        if !self.auto_subscribe_device_status || cloud_mips_disabled() {
            return;
        }
        let now = Instant::now();
        let snapshot = {
            let Ok(runtime) = cloud_mips_runtime().lock() else {
                self.log("cloud MIPS runtime lock poisoned");
                return;
            };
            runtime.as_ref().map(|runtime| {
                (
                    runtime.last_mqtt_response_at,
                    runtime.last_ping_req_at,
                    runtime.last_ping_resp_at,
                )
            })
        };
        let Some((last_mqtt_response_at, last_ping_req_at, last_ping_resp_at)) = snapshot else {
            self.refresh_cloud_mips_listeners();
            self.request_prop_dialog_refresh_allow_editing();
            return;
        };

        let latest_response_at = match (last_mqtt_response_at, last_ping_resp_at) {
            (Some(response_at), Some(ping_resp_at)) => Some(response_at.max(ping_resp_at)),
            (Some(response_at), None) => Some(response_at),
            (None, Some(ping_resp_at)) => Some(ping_resp_at),
            (None, None) => None,
        };
        if let Some(latest_response_at) = latest_response_at {
            let elapsed = now
                .checked_duration_since(latest_response_at)
                .unwrap_or_default();
            if elapsed <= CLOUD_MIPS_RESPONSE_STALE_THRESHOLD {
                return;
            }
            self.log(format!(
                "cloud MIPS response stale: last response {}s ago; restarting listeners",
                elapsed.as_secs()
            ));
            self.restart_cloud_mips_listeners();
            self.request_prop_dialog_refresh_allow_editing();
            return;
        }

        if let Some(last_ping_req_at) = last_ping_req_at {
            let elapsed = now
                .checked_duration_since(last_ping_req_at)
                .unwrap_or_default();
            self.log(format!(
                "cloud MIPS waiting for PingResp: last PingReq {}s ago",
                elapsed.as_secs()
            ));
            if elapsed > CLOUD_MIPS_RESPONSE_STALE_THRESHOLD {
                self.restart_cloud_mips_listeners();
                self.request_prop_dialog_refresh_allow_editing();
            }
        }
    }

    pub(in crate::tui) fn restart_cloud_mips_listeners(&mut self) {
        let Ok(mut runtime) = cloud_mips_runtime().lock() else {
            self.log("cloud MIPS runtime lock poisoned");
            return;
        };
        *runtime = None;
        drop(runtime);
        self.refresh_cloud_mips_listeners();
    }

    pub(in crate::tui) fn refresh_cloud_mips_listeners(&mut self) {
        if !self.auto_subscribe_device_status {
            self.log("cloud MIPS not started: auto subscribe disabled");
            if let Ok(mut runtime) = cloud_mips_runtime().lock() {
                *runtime = None;
            }
            return;
        }

        if cloud_mips_disabled() {
            if cloud_mips_disabled_by_user() {
                self.log("cloud MIPS not started: disabled by MIT_DISABLE_CLOUD_MIPS");
            }
            if let Ok(mut runtime) = cloud_mips_runtime().lock() {
                *runtime = None;
            }
            return;
        }

        let groups = self.cloud_mips_account_device_groups();
        if groups.is_empty() {
            let account_count = self.accounts.len();
            let oauth_account_count = self
                .accounts
                .iter()
                .filter(|account| !account.access_token.trim().is_empty())
                .count();
            let offline_account_count = self.offline_account_uids.len();
            let tagged_device_count = self
                .devices
                .iter()
                .filter(|device| device_account_uid(device).is_some())
                .count();
            self.log(format!(
                "cloud MIPS not started: no eligible OAuth account/device groups \
                 (accounts={account_count}, oauth_accounts={oauth_account_count}, \
                 offline_accounts={offline_account_count}, devices={}, tagged_devices={tagged_device_count})",
                self.devices.len()
            ));
            if let Ok(mut runtime) = cloud_mips_runtime().lock() {
                *runtime = None;
            }
            return;
        }

        let key = cloud_mips_runtime_key(&groups);
        let Ok(mut runtime) = cloud_mips_runtime().lock() else {
            self.log("cloud MIPS runtime lock poisoned");
            return;
        };
        if runtime.as_ref().is_some_and(|current| current.key == key) {
            return;
        }

        let account_count = groups.len();
        let device_count = groups.iter().map(|(_, dids)| dids.len()).sum::<usize>();
        self.log(format!(
            "cloud MIPS starting: accounts={account_count} devices={device_count}"
        ));

        let (tx, rx) = mpsc::channel();
        let mut handles = Vec::new();
        let mut errors = Vec::new();
        for (account, dids) in groups {
            match config_from_account(&account).and_then(|config| {
                start_property_cache_listener(
                    config,
                    dids,
                    self.property_cache.clone(),
                    Some(tx.clone()),
                )
            }) {
                Ok(handle) => handles.push(handle),
                Err(error) => errors.push(format!(
                    "{}: {error}",
                    account_page::format_account_label(&account)
                )),
            }
        }
        drop(tx);

        let started_count = handles.len();
        if handles.is_empty() {
            *runtime = None;
        } else {
            *runtime = Some(CloudMipsRuntime {
                key,
                _handles: handles,
                rx,
                last_mqtt_response_at: None,
                last_ping_req_at: None,
                last_ping_resp_at: None,
            });
        }
        drop(runtime);

        for error in errors {
            self.log(format!("cloud MIPS start failed: {error}"));
        }
        if started_count > 0 {
            self.log(format!(
                "cloud MIPS listener threads spawned: {started_count}"
            ));
        }
    }

    pub(in crate::tui) fn cloud_mips_account_device_groups(
        &self,
    ) -> Vec<(AuthAccount, Vec<String>)> {
        let mut groups = Vec::new();
        for account in &self.accounts {
            if account.access_token.trim().is_empty()
                || self.offline_account_uids.contains(&account.user.uid)
            {
                continue;
            }
            let dids = self
                .devices
                .iter()
                .filter(|device| device_account_uid(device) == Some(account.user.uid.as_str()))
                .map(|device| device.did.clone())
                .collect::<Vec<_>>();
            if dids.is_empty() {
                continue;
            }
            groups.push((account.clone(), dids));
        }
        groups
    }

    pub(in crate::tui) fn process_cloud_mips_messages(&mut self) {
        let statuses = {
            let Ok(mut runtime) = cloud_mips_runtime().lock() else {
                self.log("cloud MIPS runtime lock poisoned");
                return;
            };
            let Some(runtime) = runtime.as_mut() else {
                return;
            };
            let statuses = runtime.rx.try_iter().collect::<Vec<_>>();
            let now = Instant::now();
            for status in &statuses {
                update_cloud_mips_runtime_liveness(runtime, status, now);
            }
            statuses
        };

        for status in statuses {
            match status {
                CloudMipsStatus::Started { host, device_count } => self.log(format!(
                    "cloud MIPS listening on {host} for {device_count} devices"
                )),
                CloudMipsStatus::EventReceived { direction, summary } => {
                    self.log(format!("cloud MIPS mqtt {direction}: {summary}"))
                }
                CloudMipsStatus::MessageReceived { topic, payload_len } => self.log(format!(
                    "cloud MIPS message: topic={topic} bytes={payload_len}"
                )),
                CloudMipsStatus::PropertyApplied { did, siid, piid } => self.log(format!(
                    "cloud MIPS property update: did={did} siid={siid} piid={piid}"
                )),
                CloudMipsStatus::Error { message } => {
                    self.log(format!("cloud MIPS error: {message}"))
                }
                CloudMipsStatus::IgnoredMessage { reason } => {
                    self.log(format!("cloud MIPS ignored message: {reason}"))
                }
                CloudMipsStatus::AuthRejected { message } => {
                    self.log(format!(
                        "cloud MIPS auth rejected: {message}; refreshing auth"
                    ));
                    self.handle_cloud_mips_auth_rejected();
                }
                CloudMipsStatus::Stopped => self.log("cloud MIPS stopped"),
            }
        }
    }

    pub(in crate::tui) fn handle_cloud_mips_auth_rejected(&mut self) {
        let groups = self.cloud_mips_account_device_groups();
        let stale_uids = groups
            .iter()
            .map(|(account, _)| account.user.uid.clone())
            .collect::<HashSet<_>>();
        if stale_uids.is_empty() {
            return;
        }
        for account in &mut self.accounts {
            if stale_uids.contains(&account.user.uid) {
                account.expires_ts = 1;
            }
        }
        match cloud_mips_runtime().lock() {
            Ok(mut runtime) => *runtime = None,
            Err(_) => self.log("cloud MIPS runtime lock poisoned"),
        }
        self.start_background_sync();
    }

    pub(in crate::tui) fn apply_cached_prop_dialog_updates(&mut self) {
        let Some(dialog) = self.prop_dialog.as_mut() else {
            return;
        };
        if dialog.loading || dialog.editing {
            return;
        }
        for item in &mut dialog.items {
            let Some(cached) = self.property_cache.get_property(
                dialog.device_did.as_str(),
                item.prop.siid,
                item.prop.piid,
            ) else {
                continue;
            };
            if let Some(value) = extract_prop_value(&cached) {
                item.value = value;
            }
        }
    }
}
