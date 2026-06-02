use anyhow::{anyhow, bail, Result};
use rumqttc::v5::mqttbytes::QoS;
use rumqttc::v5::{Client, Connection, Event, Incoming, MqttOptions};
use rumqttc::Transport;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::mico_api::OAUTH_CLIENT_ID;
use crate::property_cache::PropertyCache;
use crate::storage::{AuthAccount, DEFAULT_REGION};

const DEFAULT_CLOUD_BROKER_HOST: &str = "ha.mqtt.io.mi.com";
const DEFAULT_CLOUD_MIPS_PORT: u16 = 8883;
const MIHOME_MQTT_KEEPALIVE_SECS: u64 = 60;
const MIPS_CONNECTION_TIMEOUT_SECS: u64 = 10;
const MIPS_RECV_TIMEOUT_SECS: u64 = MIPS_CONNECTION_TIMEOUT_SECS + 5;
const MIPS_RECONNECT_DELAY_SECS: u64 = 2;
const MIPS_REQUEST_QUEUE_MIN_CAPACITY: usize = 16;
const MIPS_REQUEST_QUEUE_SLACK: usize = 16;
const MIPS_APP_ID_ENV: &str = "MIT_MIPS_APP_ID";
const MIPS_BROKER_HOST_ENV: &str = "MIT_MIPS_BROKER_HOST";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloudMipsConfig {
    pub uuid: String,
    pub region: String,
    pub app_id: String,
    pub access_token: String,
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PropertyUpdate {
    pub did: String,
    pub siid: i64,
    pub piid: i64,
    pub value: Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloudMipsSubscription {
    pub topic_filter: String,
    pub property: Option<(i64, i64)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CloudMipsStatus {
    Started { host: String, device_count: usize },
    EventReceived { direction: String, summary: String },
    MessageReceived { topic: String, payload_len: usize },
    PropertyApplied { did: String, siid: i64, piid: i64 },
    IgnoredMessage { reason: String },
    Error { message: String },
    Stopped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReceiveErrorAction {
    Continue,
    Reconnect,
}

#[derive(Debug)]
pub struct CloudMipsHandle {
    stop: Arc<AtomicBool>,
}

impl CloudMipsHandle {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

impl Drop for CloudMipsHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn build_cloud_client_id(uuid: &str) -> String {
    format!("ha.{}", uuid.trim())
}

pub fn build_cloud_broker_host(region: &str) -> String {
    format!("{}-{DEFAULT_CLOUD_BROKER_HOST}", normalize_region(region))
}

pub fn property_topic_filter(did: &str) -> String {
    format!("device/{}/up/properties_changed/#", did.trim())
}

pub fn property_subscription_for(did: &str, property: Option<(i64, i64)>) -> CloudMipsSubscription {
    CloudMipsSubscription {
        topic_filter: property_topic_filter(did),
        property,
    }
}

pub fn parse_property_update(topic: &str, payload: &[u8]) -> Result<PropertyUpdate> {
    let topic_did = did_from_property_topic(topic).unwrap_or_default();
    let text = std::str::from_utf8(payload).map_err(|error| anyhow!("invalid utf8: {error}"))?;
    let body: Value = serde_json::from_str(text)?;
    let params = body
        .get("params")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("property update missing params"))?;

    let did = params
        .get("did")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or(topic_did);
    if did.trim().is_empty() {
        bail!("property update missing did");
    }

    let siid = params
        .get("siid")
        .and_then(Value::as_i64)
        .ok_or_else(|| anyhow!("property update missing siid"))?;
    let piid = params
        .get("piid")
        .and_then(Value::as_i64)
        .ok_or_else(|| anyhow!("property update missing piid"))?;
    let value = params
        .get("value")
        .cloned()
        .ok_or_else(|| anyhow!("property update missing value"))?;

    Ok(PropertyUpdate {
        did,
        siid,
        piid,
        value,
    })
}

pub fn config_from_account(account: &AuthAccount) -> Result<CloudMipsConfig> {
    let uuid = account.uuid.trim().to_string();
    if uuid.is_empty() {
        bail!("OAuth account missing uuid for cloud MIPS");
    }
    let access_token = account.access_token.trim().to_string();
    if access_token.is_empty() {
        bail!("OAuth account missing access token for cloud MIPS");
    }
    let region = normalize_region(account.region.as_str());
    let app_id = std::env::var(MIPS_APP_ID_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| OAUTH_CLIENT_ID.to_string());
    let host = std::env::var(MIPS_BROKER_HOST_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| build_cloud_broker_host(&region));
    Ok(CloudMipsConfig {
        uuid,
        region,
        app_id,
        access_token,
        host,
        port: DEFAULT_CLOUD_MIPS_PORT,
    })
}

pub fn start_property_cache_listener(
    config: CloudMipsConfig,
    dids: Vec<String>,
    property_cache: Arc<PropertyCache>,
    status_tx: Option<Sender<CloudMipsStatus>>,
) -> Result<CloudMipsHandle> {
    let dids = normalized_dids(dids);
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    thread::Builder::new()
        .name("mit-cloud-mips".to_string())
        .spawn(move || {
            if let Err(error) = run_property_cache_listener(
                config,
                dids,
                property_cache,
                status_tx.clone(),
                thread_stop,
            ) {
                send_status(
                    &status_tx,
                    CloudMipsStatus::Error {
                        message: error.to_string(),
                    },
                );
            }
            send_status(&status_tx, CloudMipsStatus::Stopped);
        })
        .map_err(|error| anyhow!("spawn cloud MIPS listener: {error}"))?;

    Ok(CloudMipsHandle { stop })
}

pub fn start_stdout_subscription(
    config: CloudMipsConfig,
    subscriptions: Vec<CloudMipsSubscription>,
    line_tx: Sender<String>,
) -> Result<CloudMipsHandle> {
    let subscriptions = normalized_subscriptions(subscriptions);
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    thread::Builder::new()
        .name("mit-cloud-mips-sub".to_string())
        .spawn(move || {
            if let Err(error) =
                run_stdout_subscription(config, subscriptions, line_tx.clone(), thread_stop)
            {
                send_line(&line_tx, format!("cloud MIPS error: {error}"));
            }
            send_line(&line_tx, "cloud MIPS stopped".to_string());
        })
        .map_err(|error| anyhow!("spawn cloud MIPS subscription: {error}"))?;

    Ok(CloudMipsHandle { stop })
}

fn run_property_cache_listener(
    config: CloudMipsConfig,
    dids: Vec<String>,
    property_cache: Arc<PropertyCache>,
    status_tx: Option<Sender<CloudMipsStatus>>,
    stop: Arc<AtomicBool>,
) -> Result<()> {
    if dids.is_empty() {
        return Ok(());
    }

    let topic_filters = dids
        .iter()
        .map(|did| property_topic_filter(did))
        .collect::<Vec<_>>();

    while !stop.load(Ordering::SeqCst) {
        let (_client, mut connection) = match open_mqtt_connection(&config, &topic_filters) {
            Ok(connection) => connection,
            Err(error) => {
                send_status(
                    &status_tx,
                    CloudMipsStatus::Error {
                        message: format!("mqtt connect setup: {error}; reconnecting"),
                    },
                );
                sleep_before_reconnect(&stop);
                continue;
            }
        };
        send_status(
            &status_tx,
            CloudMipsStatus::Started {
                host: config.host.clone(),
                device_count: dids.len(),
            },
        );

        loop {
            if stop.load(Ordering::SeqCst) {
                break;
            }
            match connection.recv_timeout(Duration::from_secs(MIPS_RECV_TIMEOUT_SECS)) {
                Ok(Ok(Event::Incoming(Incoming::Publish(publish)))) => {
                    let topic = String::from_utf8_lossy(&publish.topic).into_owned();
                    let payload_len = publish.payload.len();
                    send_status(
                        &status_tx,
                        CloudMipsStatus::MessageReceived {
                            topic: topic.clone(),
                            payload_len,
                        },
                    );
                    match parse_property_update(topic.as_str(), &publish.payload) {
                        Ok(update) => {
                            property_cache.set_property(
                                update.did.clone(),
                                update.siid,
                                update.piid,
                                update.value,
                            );
                            send_status(
                                &status_tx,
                                CloudMipsStatus::PropertyApplied {
                                    did: update.did,
                                    siid: update.siid,
                                    piid: update.piid,
                                },
                            );
                        }
                        Err(error) => send_status(
                            &status_tx,
                            CloudMipsStatus::IgnoredMessage {
                                reason: error.to_string(),
                            },
                        ),
                    }
                }
                Ok(Ok(Event::Incoming(packet))) => send_status(
                    &status_tx,
                    CloudMipsStatus::EventReceived {
                        direction: "incoming".to_string(),
                        summary: format!("{packet:?}"),
                    },
                ),
                Ok(Ok(Event::Outgoing(packet))) => send_status(
                    &status_tx,
                    CloudMipsStatus::EventReceived {
                        direction: "outgoing".to_string(),
                        summary: format!("{packet:?}"),
                    },
                ),
                Ok(Err(error)) => {
                    send_status(
                        &status_tx,
                        CloudMipsStatus::Error {
                            message: format!("{error}; reconnecting"),
                        },
                    );
                    break;
                }
                Err(error) => {
                    let message = format!("{error:?}");
                    if receive_error_action(message.as_str()) == ReceiveErrorAction::Continue {
                        continue;
                    }
                    send_status(
                        &status_tx,
                        CloudMipsStatus::Error {
                            message: format!("mqtt receive loop: {message}; reconnecting"),
                        },
                    );
                    break;
                }
            }
        }
        if !stop.load(Ordering::SeqCst) {
            send_status(
                &status_tx,
                CloudMipsStatus::EventReceived {
                    direction: "reconnect".to_string(),
                    summary: format!("retrying in {MIPS_RECONNECT_DELAY_SECS}s"),
                },
            );
            sleep_before_reconnect(&stop);
        }
    }

    Ok(())
}

fn run_stdout_subscription(
    config: CloudMipsConfig,
    subscriptions: Vec<CloudMipsSubscription>,
    line_tx: Sender<String>,
    stop: Arc<AtomicBool>,
) -> Result<()> {
    if subscriptions.is_empty() {
        return Ok(());
    }

    let topic_filters = subscriptions
        .iter()
        .map(|subscription| subscription.topic_filter.clone())
        .collect::<Vec<_>>();

    while !stop.load(Ordering::SeqCst) {
        let (_client, mut connection) = match open_mqtt_connection(&config, &topic_filters) {
            Ok(connection) => connection,
            Err(error) => {
                send_line(
                    &line_tx,
                    format!("cloud MIPS error: mqtt connect setup: {error}; reconnecting"),
                );
                sleep_before_reconnect(&stop);
                continue;
            }
        };
        send_line(
            &line_tx,
            format!(
                "cloud MIPS listening on {} for {} subscriptions",
                config.host,
                subscriptions.len()
            ),
        );

        loop {
            if stop.load(Ordering::SeqCst) {
                break;
            }
            match connection.recv_timeout(Duration::from_secs(MIPS_RECV_TIMEOUT_SECS)) {
                Ok(Ok(Event::Incoming(Incoming::Publish(publish)))) => {
                    let topic = String::from_utf8_lossy(&publish.topic).into_owned();
                    send_line(
                        &line_tx,
                        format!(
                            "cloud MIPS message: topic={} bytes={}",
                            topic,
                            publish.payload.len()
                        ),
                    );
                    match parse_property_update(topic.as_str(), &publish.payload) {
                        Ok(update) => {
                            if subscription_matches_update(&subscriptions, &update) {
                                send_line(
                                    &line_tx,
                                    format!(
                                        "property update: did={} siid={} piid={} value={}",
                                        update.did,
                                        update.siid,
                                        update.piid,
                                        serde_json::to_string(&update.value)?
                                    ),
                                );
                            }
                        }
                        Err(error) => {
                            send_line(&line_tx, format!("cloud MIPS ignored message: {error}"));
                        }
                    }
                }
                Ok(Ok(Event::Incoming(packet))) => {
                    send_line(&line_tx, format!("cloud MIPS mqtt incoming: {packet:?}"));
                }
                Ok(Ok(Event::Outgoing(packet))) => {
                    send_line(&line_tx, format!("cloud MIPS mqtt outgoing: {packet:?}"));
                }
                Ok(Err(error)) => {
                    send_line(&line_tx, format!("cloud MIPS error: {error}; reconnecting"));
                    break;
                }
                Err(error) => {
                    let message = format!("{error:?}");
                    if receive_error_action(message.as_str()) == ReceiveErrorAction::Continue {
                        continue;
                    }
                    send_line(
                        &line_tx,
                        format!("cloud MIPS error: mqtt receive loop: {message}; reconnecting"),
                    );
                    break;
                }
            }
        }
        if !stop.load(Ordering::SeqCst) {
            send_line(
                &line_tx,
                format!("cloud MIPS reconnecting in {MIPS_RECONNECT_DELAY_SECS}s"),
            );
            sleep_before_reconnect(&stop);
        }
    }

    Ok(())
}

fn open_mqtt_connection(
    config: &CloudMipsConfig,
    topic_filters: &[String],
) -> Result<(Client, Connection)> {
    let mut options = MqttOptions::new(
        build_cloud_client_id(&config.uuid),
        &config.host,
        config.port,
    );
    options.set_credentials(config.app_id.clone(), config.access_token.clone());
    options.set_clean_start(true);
    options.set_keep_alive(Duration::from_secs(MIHOME_MQTT_KEEPALIVE_SECS));
    options.set_connection_timeout(MIPS_CONNECTION_TIMEOUT_SECS);
    options.set_transport(Transport::tls_with_default_config());

    let (client, connection) =
        Client::new(options, mqtt_request_queue_capacity(topic_filters.len()));
    for topic_filter in topic_filters {
        client.subscribe(topic_filter.as_str(), QoS::ExactlyOnce)?;
    }
    Ok((client, connection))
}

fn receive_error_action(message: &str) -> ReceiveErrorAction {
    if message == "Timeout" {
        ReceiveErrorAction::Continue
    } else {
        ReceiveErrorAction::Reconnect
    }
}

fn sleep_before_reconnect(stop: &AtomicBool) {
    for _ in 0..MIPS_RECONNECT_DELAY_SECS * 10 {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn did_from_property_topic(topic: &str) -> Option<String> {
    let mut parts = topic.split('/');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some("device"), Some(did), Some("up"), Some("properties_changed"))
            if !did.trim().is_empty() =>
        {
            Some(did.trim().to_string())
        }
        _ => None,
    }
}

fn normalized_dids(dids: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for did in dids {
        let did = did.trim().to_string();
        if did.is_empty() || !seen.insert(did.clone()) {
            continue;
        }
        out.push(did);
    }
    out
}

fn normalized_subscriptions(
    subscriptions: Vec<CloudMipsSubscription>,
) -> Vec<CloudMipsSubscription> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for mut subscription in subscriptions {
        subscription.topic_filter = subscription.topic_filter.trim().to_string();
        if subscription.topic_filter.is_empty()
            || !seen.insert((subscription.topic_filter.clone(), subscription.property))
        {
            continue;
        }
        out.push(subscription);
    }
    out
}

fn subscription_matches_update(
    subscriptions: &[CloudMipsSubscription],
    update: &PropertyUpdate,
) -> bool {
    subscriptions.iter().any(|subscription| {
        did_from_property_topic(subscription.topic_filter.as_str()).as_deref()
            == Some(update.did.as_str())
            && match subscription.property {
                Some((siid, piid)) => update.siid == siid && update.piid == piid,
                None => true,
            }
    })
}

fn mqtt_request_queue_capacity(subscription_count: usize) -> usize {
    subscription_count
        .saturating_add(MIPS_REQUEST_QUEUE_SLACK)
        .max(MIPS_REQUEST_QUEUE_MIN_CAPACITY)
}

fn normalize_region(region: &str) -> String {
    let region = region.trim().to_ascii_lowercase();
    if region.is_empty() {
        DEFAULT_REGION.to_string()
    } else {
        region
    }
}

fn send_status(status_tx: &Option<Sender<CloudMipsStatus>>, status: CloudMipsStatus) {
    if let Some(tx) = status_tx {
        let _ = tx.send(status);
    }
}

fn send_line(line_tx: &Sender<String>, line: String) {
    let _ = line_tx.send(line);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mqtt_request_queue_capacity_leaves_room_for_all_subscriptions() {
        assert_eq!(
            mqtt_request_queue_capacity(0),
            MIPS_REQUEST_QUEUE_MIN_CAPACITY
        );
        assert!(mqtt_request_queue_capacity(80) > 80);
    }

    #[test]
    fn mqtt_recv_timeout_allows_connection_timeout_to_complete() {
        assert!(MIPS_RECV_TIMEOUT_SECS > MIPS_CONNECTION_TIMEOUT_SECS);
    }

    #[test]
    fn mqtt_timeout_is_idle_not_reconnect_or_notice() {
        assert_eq!(
            receive_error_action("Timeout"),
            ReceiveErrorAction::Continue
        );
    }

    #[test]
    fn mqtt_non_timeout_receive_errors_trigger_reconnect() {
        assert_eq!(
            receive_error_action("Io(Custom { kind: UnexpectedEof })"),
            ReceiveErrorAction::Reconnect
        );
    }

    #[test]
    fn stdout_subscription_filters_specific_property_client_side() {
        let subscriptions = vec![property_subscription_for("dev-1", Some((2, 1)))];

        assert!(subscription_matches_update(
            &subscriptions,
            &PropertyUpdate {
                did: "dev-1".to_string(),
                siid: 2,
                piid: 1,
                value: Value::Bool(true),
            },
        ));
        assert!(!subscription_matches_update(
            &subscriptions,
            &PropertyUpdate {
                did: "dev-1".to_string(),
                siid: 2,
                piid: 2,
                value: Value::Bool(true),
            },
        ));
    }
}
