//! Mijia cloud data loaders for the device Logs (operation records) and
//! Statistics tabs, returning UI-shaped JSON used by the prop dialog.
use serde_json::{json, Map, Value};

use crate::mijia_api::{DeviceHistoryQuery, MijiaClient};
use crate::storage::AuthAccount;

use super::{
    json_i64, statistics_default_query, StatisticsPeriod, MIJIA_PROP_DATA_TYPE,
    OPERATION_RECORD_PAGE_LIMIT,
};

pub(in crate::tui) fn load_mijia_device_logs_json(
    account: &AuthAccount,
    did: &str,
    keys: &[String],
) -> Value {
    load_mijia_device_logs_json_with_query(account, did, keys, None)
}

pub(in crate::tui) fn load_mijia_device_logs_json_with_query(
    account: &AuthAccount,
    did: &str,
    keys: &[String],
    date_filter: Option<(i64, i64)>,
) -> Value {
    load_mijia_raw_json(account, |client, auth| {
        let mut query = DeviceHistoryQuery::recent(OPERATION_RECORD_PAGE_LIMIT);
        if let Some((time_start, time_end)) = date_filter {
            query.time_start = time_start;
            query.time_end = time_end;
        }
        json!({
            "requests": visible_mijia_raw_request_entries(keys
                .iter()
                .map(|key| {
                    match client.get_user_device_data_with_query(auth, did, key, MIJIA_PROP_DATA_TYPE, query) {
                        Ok(response) => {
                            let result_len = response
                                .get("result")
                                .and_then(Value::as_array)
                                .map(Vec::len)
                                .unwrap_or(0);
                            json!({
                                "key": key,
                                "type": MIJIA_PROP_DATA_TYPE,
                                "response": response,
                                "pagination": {
                                    "time_start": query.time_start,
                                    "time_end": query.time_end,
                                    "limit": query.limit,
                                    "has_more": result_len >= query.limit as usize && result_len > 0,
                                    "no_more": result_len < query.limit as usize,
                                    "loading_more": false
                                }
                            })
                        }
                        Err(error) => json!({
                            "key": key,
                            "type": MIJIA_PROP_DATA_TYPE,
                            "error": error.to_string()
                        }),
                    }
                })
                .collect::<Vec<_>>())
            ,
            "date_filter": date_filter.map(|(time_start, time_end)| json!({
                "time_start": time_start,
                "time_end": time_end
            }))
        })
    })
}

pub(in crate::tui) fn load_mijia_device_statistics_json(
    account: &AuthAccount,
    did: &str,
    keys: &[String],
) -> Value {
    let period = StatisticsPeriod::Week;
    load_mijia_device_statistics_json_with_query(
        account,
        did,
        keys,
        period,
        statistics_default_query(period),
        None,
    )
}

pub(in crate::tui) fn load_mijia_device_statistics_json_with_query(
    account: &AuthAccount,
    did: &str,
    keys: &[String],
    period: StatisticsPeriod,
    query: DeviceHistoryQuery,
    selected_key: Option<String>,
) -> Value {
    load_mijia_raw_json(account, |client, auth| {
        let data_type = period.data_type();
        let mut ui = Map::new();
        ui.insert("period".to_string(), json!(period.key()));
        if let Some(selected_key) = selected_key.as_deref() {
            ui.insert("selected_key".to_string(), json!(selected_key));
        }
        json!({
            "requests": visible_mijia_raw_request_entries(keys
                .iter()
                .map(|key| {
                    match client.get_user_statistics_with_query(auth, did, key, data_type, query) {
                        Ok(response) => json!({
                            "key": key,
                            "data_type": data_type,
                            "response": response
                        }),
                        Err(error) => json!({
                            "key": key,
                            "data_type": data_type,
                            "error": error.to_string()
                        }),
                    }
                })
                .collect::<Vec<_>>()),
            "ui": Value::Object(ui),
            "date_filter": {
                "time_start": query.time_start,
                "time_end": query.time_end
            }
        })
    })
}

pub(in crate::tui) fn visible_mijia_raw_request_entries(entries: Vec<Value>) -> Vec<Value> {
    entries
        .into_iter()
        .filter(|entry| !is_successful_empty_mijia_response(entry.get("response")))
        .collect()
}

fn is_successful_empty_mijia_response(response: Option<&Value>) -> bool {
    let Some(response) = response else {
        return false;
    };
    json_code_is_zero(response.get("code"))
        && response
            .get("result")
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty)
}

pub(in crate::tui) fn json_code_is_zero(value: Option<&Value>) -> bool {
    json_i64(value) == Some(0)
}

fn load_mijia_raw_json(
    account: &AuthAccount,
    fetch: impl FnOnce(&MijiaClient, &crate::storage::MijiaAuth) -> Value,
) -> Value {
    let Some(mijia) = account.mijia.as_ref() else {
        return json!({
            "error": "mijia auth is missing"
        });
    };
    match MijiaClient::new() {
        Ok(client) => fetch(&client, mijia),
        Err(error) => json!({
            "error": error.to_string()
        }),
    }
}
