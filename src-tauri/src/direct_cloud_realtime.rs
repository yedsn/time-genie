use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio::sync::Notify;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;

use crate::database::Database;

const REALTIME_HEARTBEAT_SECONDS: u64 = 25;
const REALTIME_RECONNECT_SECONDS: u64 = 5;
pub(crate) const REALTIME_RECONNECT_MAX_SECONDS: u64 = 60;
const REALTIME_CONNECT_TIMEOUT_SECONDS: u64 = 12;
const REALTIME_JOIN_TIMEOUT_SECONDS: u64 = 12;

#[derive(Debug, Clone)]
pub(crate) struct RealtimeConnectionConfig {
    pub(crate) workspace_id: String,
    pub(crate) storage_protocol: String,
    pub(crate) project_url: String,
    pub(crate) anon_key: String,
    pub(crate) access_token: String,
}

#[derive(Clone, Copy)]
pub(crate) struct RealtimeListenerHooks {
    pub(crate) load_config: fn(&Database) -> Result<Option<RealtimeConnectionConfig>, String>,
    pub(crate) is_enabled: fn() -> bool,
    pub(crate) on_error: fn(&AppHandle, &Database, &str),
    pub(crate) on_connected: fn(&Database, &AppHandle, &str),
    pub(crate) on_legacy_change: fn(Option<i64>),
    pub(crate) workspace_epoch: fn(&Database, &str) -> i64,
    pub(crate) covered_change_seq: fn(&Database) -> i64,
    pub(crate) on_invalidation: fn(&Database, &WorkspaceInvalidation),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceInvalidation {
    pub(crate) workspace_id: String,
    pub(crate) change_seq: i64,
    pub(crate) workspace_epoch: i64,
    pub(crate) domains: Vec<String>,
    pub(crate) source: String,
    pub(crate) changed_by_device_id: Option<String>,
}

pub(crate) fn realtime_change_seq(message: &Value) -> Option<i64> {
    message
        .pointer("/payload/data/record/change_seq")
        .or_else(|| message.pointer("/payload/record/change_seq"))
        .or_else(|| message.pointer("/payload/data/new/change_seq"))
        .and_then(|value| value.as_i64().or_else(|| value.as_str()?.parse().ok()))
}

fn is_realtime_heartbeat_reply(message: &Value, pending_ref: Option<&str>) -> bool {
    pending_ref.is_some()
        && message.get("event").and_then(Value::as_str) == Some("phx_reply")
        && message.get("topic").and_then(Value::as_str) == Some("phoenix")
        && message.get("ref").and_then(Value::as_str) == pending_ref
}

pub(crate) fn realtime_workspace_invalidation(
    message: &Value,
    expected_workspace_id: &str,
    workspace_epoch: i64,
) -> Option<WorkspaceInvalidation> {
    let record = message
        .pointer("/payload/data/record")
        .or_else(|| message.pointer("/payload/record"))
        .or_else(|| message.pointer("/payload/data/new"))?;
    let change_seq = record
        .get("change_seq")
        .and_then(|value| value.as_i64().or_else(|| value.as_str()?.parse().ok()))?;
    let workspace_id = record
        .get("workspace_id")
        .and_then(Value::as_str)
        .filter(|value| *value == expected_workspace_id)?;
    let domains = record
        .get("entity_type")
        .and_then(Value::as_str)
        .map(realtime_refresh_domains)
        .unwrap_or_default();
    let changed_by_device_id = record
        .get("changed_by_device_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    if domains.is_empty() {
        return None;
    }
    Some(WorkspaceInvalidation {
        workspace_id: workspace_id.to_string(),
        change_seq,
        workspace_epoch,
        domains: domains.into_iter().map(str::to_string).collect(),
        source: "realtime".to_string(),
        changed_by_device_id,
    })
}

pub(crate) fn realtime_refresh_domains(entity_type: &str) -> Vec<&'static str> {
    match entity_type {
        "subjects" | "subject_work_schedules" => vec!["subjects"],
        "tasks"
        | "task_status_events"
        | "task_daily_estimates"
        | "task_day_plans"
        | "task_recurrence_rules"
        | "task_occurrences" => vec!["tasks"],
        "work_days"
        | "time_entries"
        | "time_segments"
        | "time_allocations"
        | "actual_time_adjustments"
        | "actual_adjustment_batches" => vec!["time"],
        "unassigned_sessions" | "unassigned_segments" => vec!["unassigned"],
        "report_templates" | "reports" | "report_tasks" => vec!["reports"],
        "app_settings" | "integration_configs" | "external_bindings" => vec!["settings"],
        _ => Vec::new(),
    }
}

pub(crate) fn realtime_url(project_url: &str, anon_key: &str) -> Result<Url, String> {
    let mut url = Url::parse(project_url)
        .map_err(|_| "CLOUD_NOT_CONFIGURED: Supabase Project URL 无效".to_string())?;
    let websocket_scheme = match url.scheme() {
        "https" => "wss",
        "http" => "ws",
        _ => return Err("CLOUD_NOT_CONFIGURED: Supabase Realtime 地址无效".to_string()),
    };
    url.set_scheme(websocket_scheme)
        .map_err(|_| "CLOUD_NOT_CONFIGURED: Supabase Realtime 地址无效".to_string())?;
    url.set_path("/realtime/v1/websocket");
    url.set_query(None);
    url.query_pairs_mut()
        .append_pair("apikey", anon_key)
        .append_pair("vsn", "1.0.0");
    Ok(url)
}

pub(crate) fn realtime_reconnect_delay(attempt: u32, jitter_seed: u64) -> Duration {
    let exponent = attempt.min(4);
    let base_millis = REALTIME_RECONNECT_SECONDS
        .saturating_mul(1_u64 << exponent)
        .saturating_mul(1_000)
        .min(REALTIME_RECONNECT_MAX_SECONDS * 1_000);
    let jitter_window = (base_millis / 5).max(1);
    Duration::from_millis(
        (base_millis + jitter_seed % jitter_window).min(REALTIME_RECONNECT_MAX_SECONDS * 1_000),
    )
}

pub(crate) fn spawn_realtime_listener(
    database: Database,
    app: AppHandle,
    mode_wakeup: Arc<Notify>,
    hooks: RealtimeListenerHooks,
) {
    tauri::async_runtime::spawn(async move {
        let mut reconnect_attempt = 0_u32;
        loop {
            while !(hooks.is_enabled)() {
                mode_wakeup.notified().await;
            }
            let config_database = database.clone();
            let config =
                tauri::async_runtime::spawn_blocking(move || (hooks.load_config)(&config_database))
                    .await;
            let config = match config {
                Ok(Ok(Some(config))) => config,
                Ok(Ok(None)) => {
                    tokio::time::sleep(Duration::from_secs(REALTIME_RECONNECT_SECONDS)).await;
                    reconnect_attempt = 0;
                    continue;
                }
                Ok(Err(error)) => {
                    (hooks.on_error)(&app, &database, &error);
                    tokio::time::sleep(Duration::from_secs(REALTIME_RECONNECT_SECONDS)).await;
                    continue;
                }
                Err(error) => {
                    (hooks.on_error)(
                        &app,
                        &database,
                        &format!("SYNC_WORKER_ERROR: Realtime 配置读取失败: {error}"),
                    );
                    tokio::time::sleep(Duration::from_secs(REALTIME_RECONNECT_SECONDS)).await;
                    continue;
                }
            };
            let mut connected = false;
            let mut reconnect_immediately = false;
            if let Err(error) = run_realtime_connection(
                &database,
                &app,
                &config,
                &mode_wakeup,
                hooks,
                &mut connected,
                &mut reconnect_immediately,
            )
            .await
            {
                if (hooks.is_enabled)() {
                    (hooks.on_error)(&app, &database, &error);
                }
            }
            if !(hooks.is_enabled)() {
                reconnect_attempt = 0;
                continue;
            }
            if reconnect_immediately {
                reconnect_attempt = 0;
                continue;
            }
            if connected {
                reconnect_attempt = 0;
            }
            crate::direct_cloud_observability::record_realtime_reconnect(&config.workspace_id);
            let jitter_seed = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let delay = realtime_reconnect_delay(reconnect_attempt, jitter_seed);
            reconnect_attempt = reconnect_attempt.saturating_add(1);
            tokio::select! {
                _ = tokio::time::sleep(delay) => {}
                _ = mode_wakeup.notified() => {}
            }
        }
    });
}

async fn run_realtime_connection(
    database: &Database,
    app: &AppHandle,
    config: &RealtimeConnectionConfig,
    mode_wakeup: &Arc<Notify>,
    hooks: RealtimeListenerHooks,
    connected: &mut bool,
    reconnect_immediately: &mut bool,
) -> Result<(), String> {
    let url = realtime_url(&config.project_url, &config.anon_key)?;
    let (socket, _) = tokio::time::timeout(
        Duration::from_secs(REALTIME_CONNECT_TIMEOUT_SECONDS),
        connect_async(url.as_str()),
    )
    .await
    .map_err(|_| "NETWORK_ERROR: Supabase Realtime 连接超时".to_string())?
    .map_err(|error| format!("NETWORK_ERROR: Supabase Realtime 连接失败: {error}"))?;
    let (mut writer, mut reader) = socket.split();
    let topic = format!(
        "realtime:{}:workspace_changes:{}",
        crate::supabase::CLOUD_SCHEMA,
        config.workspace_id
    );
    writer
        .send(Message::Text(
            json!({
                "topic": topic,
                "event": "phx_join",
                "payload": {
                    "config": {
                        "broadcast": { "ack": false, "self": false },
                        "presence": { "key": "" },
                        "postgres_changes": [{
                            "event": "*",
                            "schema": crate::supabase::CLOUD_SCHEMA,
                            "table": "workspace_changes",
                            "filter": format!("workspace_id=eq.{}", config.workspace_id)
                        }],
                        "private": false
                    },
                    "access_token": config.access_token
                },
                "ref": "1",
                "join_ref": "1"
            })
            .to_string()
            .into(),
        ))
        .await
        .map_err(|error| format!("NETWORK_ERROR: Supabase Realtime 订阅失败: {error}"))?;

    let mut heartbeat = tokio::time::interval(Duration::from_secs(REALTIME_HEARTBEAT_SECONDS));
    heartbeat.tick().await;
    let mut heartbeat_ref = 2_u64;
    let mut pending_heartbeat_ref: Option<String> = None;
    let join_timeout = tokio::time::sleep(Duration::from_secs(REALTIME_JOIN_TIMEOUT_SECONDS));
    tokio::pin!(join_timeout);
    loop {
        tokio::select! {
            _ = &mut join_timeout, if !*connected => {
                return Err("NETWORK_ERROR: Supabase Realtime 订阅确认超时".to_string());
            }
            _ = mode_wakeup.notified() => {
                *reconnect_immediately = true;
                let _ = writer.send(Message::Close(None)).await;
                return Ok(());
            }
            _ = heartbeat.tick() => {
                if pending_heartbeat_ref.is_some() {
                    return Err("NETWORK_ERROR: Supabase Realtime 心跳响应超时".to_string());
                }
                let heartbeat_database = database.clone();
                let workspace_id = config.workspace_id.clone();
                let current_workspace = tauri::async_runtime::spawn_blocking(move || {
                    (hooks.load_config)(&heartbeat_database)
                        .map(|value| value.map(|value| value.workspace_id))
                })
                .await
                .map_err(|error| format!("SYNC_WORKER_ERROR: Realtime 状态读取失败: {error}"))??;
                if current_workspace.as_deref() != Some(workspace_id.as_str()) {
                    let _ = writer.send(Message::Close(None)).await;
                    return Ok(());
                }
                let reference = heartbeat_ref.to_string();
                heartbeat_ref = heartbeat_ref.wrapping_add(1);
                writer.send(Message::Text(json!({
                    "topic": "phoenix", "event": "heartbeat", "payload": {}, "ref": reference
                }).to_string().into())).await
                    .map_err(|error| format!("NETWORK_ERROR: Supabase Realtime 心跳失败: {error}"))?;
                pending_heartbeat_ref = Some(reference);
            }
            incoming = reader.next() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    let message: Value = serde_json::from_str(text.as_ref()).unwrap_or(Value::Null);
                    let event = message.get("event").and_then(Value::as_str).unwrap_or_default();
                    let join_reply = event == "phx_reply"
                        && message.get("ref").and_then(Value::as_str) == Some("1");
                    if join_reply && message.pointer("/payload/status").and_then(Value::as_str) == Some("error") {
                        let reason = message.pointer("/payload/response/reason")
                            .and_then(Value::as_str).unwrap_or("频道订阅被拒绝");
                        return Err(format!("CLOUD_REQUEST_FAILED: Supabase Realtime {reason}"));
                    } else if join_reply && message.pointer("/payload/status").and_then(Value::as_str) == Some("ok") {
                        *connected = true;
                        (hooks.on_connected)(database, app, &config.storage_protocol);
                    } else if is_realtime_heartbeat_reply(&message, pending_heartbeat_ref.as_deref()) {
                        pending_heartbeat_ref = None;
                    } else if event == "postgres_changes" {
                        if config.storage_protocol == crate::storage_contract::DIRECT_CLOUD_PROTOCOL {
                            if let Some(invalidation) = realtime_workspace_invalidation(
                                &message,
                                &config.workspace_id,
                                (hooks.workspace_epoch)(database, &config.workspace_id),
                            ) {
                                if invalidation.change_seq > (hooks.covered_change_seq)(database) {
                                    (hooks.on_invalidation)(database, &invalidation);
                                    let _ = app.emit("work-data-changed", &invalidation);
                                }
                            }
                        } else {
                            (hooks.on_legacy_change)(realtime_change_seq(&message));
                        }
                    } else if event == "phx_error" || event == "phx_close" {
                        return Err("NETWORK_ERROR: Supabase Realtime 频道已断开".to_string());
                    }
                }
                Some(Ok(Message::Ping(payload))) => {
                    writer.send(Message::Pong(payload)).await
                        .map_err(|error| format!("NETWORK_ERROR: Supabase Realtime 响应失败: {error}"))?;
                }
                Some(Ok(Message::Close(_))) | None => {
                    return Err("NETWORK_ERROR: Supabase Realtime 连接已关闭".to_string());
                }
                Some(Ok(_)) => {}
                Some(Err(error)) => {
                    return Err(format!("NETWORK_ERROR: Supabase Realtime 读取失败: {error}"));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn realtime_change_seq_accepts_supabase_payload_shapes() {
        assert_eq!(
            realtime_change_seq(&json!({"payload":{"data":{"record":{"change_seq":42}}}})),
            Some(42)
        );
        assert_eq!(
            realtime_change_seq(&json!({"payload":{"record":{"change_seq":"43"}}})),
            Some(43)
        );
        assert_eq!(realtime_change_seq(&json!({"payload":{}})), None);
    }

    #[test]
    fn realtime_heartbeat_reply_only_clears_the_matching_pending_heartbeat() {
        let reply = json!({"topic":"phoenix","event":"phx_reply","ref":"7"});
        assert!(is_realtime_heartbeat_reply(&reply, Some("7")));
        assert!(!is_realtime_heartbeat_reply(&reply, Some("8")));
        assert!(!is_realtime_heartbeat_reply(&reply, None));
        assert!(!is_realtime_heartbeat_reply(
            &json!({"topic":"realtime:timegenie","event":"phx_reply","ref":"7"}),
            Some("7"),
        ));
    }

    #[test]
    fn realtime_invalidation_carries_sequence_and_epoch_only_for_matching_workspace() {
        let message = json!({"payload":{"record":{
            "change_seq":42,"workspace_id":"workspace-1","entity_type":"tasks",
            "changed_by_device_id":"device-1","title":"不得进入通知"
        }}});
        let invalidation = realtime_workspace_invalidation(&message, "workspace-1", 7).unwrap();
        assert_eq!(invalidation.change_seq, 42);
        assert_eq!(invalidation.workspace_epoch, 7);
        assert_eq!(invalidation.domains, vec!["tasks"]);
        assert!(realtime_workspace_invalidation(&message, "workspace-2", 7).is_none());
    }

    #[test]
    fn realtime_reconnect_backoff_is_jittered_and_bounded() {
        let first = realtime_reconnect_delay(0, 123);
        let later = realtime_reconnect_delay(4, 456);
        assert!(first >= Duration::from_secs(REALTIME_RECONNECT_SECONDS));
        assert!(later > first);
        assert!(later <= Duration::from_secs(REALTIME_RECONNECT_MAX_SECONDS));
        assert_ne!(
            realtime_reconnect_delay(1, 1),
            realtime_reconnect_delay(1, 2)
        );
    }
}
