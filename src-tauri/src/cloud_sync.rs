use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio::sync::Notify;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;
use uuid::Uuid;

use crate::database::Database;
use crate::supabase::{
    client, current_session, direct_cloud_workbench_bootstrap, ensure_current_device_authorized,
    mark_auth_blocked, storage_mode, CLOUD_SCHEMA,
};

const REALTIME_HEARTBEAT_SECONDS: u64 = 25;
const REALTIME_RECONNECT_SECONDS: u64 = 5;
const REALTIME_RECONNECT_MAX_SECONDS: u64 = 60;
const OUTBOX_SENDING_STALE_AFTER_MILLIS: i64 = 5 * 60 * 1000;
const OUTBOX_RETRY_MAX_BACKOFF_MILLIS: i64 = 60 * 1000;
const SYNC_SETTLE_GRACE_MILLIS: u64 = 1_500;
const SYNC_SIGNAL_LOCAL_DIRTY: u8 = 1;
const SYNC_SIGNAL_REMOTE_HINT: u8 = 1 << 1;
const SYNC_SIGNAL_CONNECTION_READY: u8 = 1 << 2;
const SYNC_SIGNAL_MANUAL_RETRY: u8 = 1 << 3;
static TRACKING_LEASE_TOKENS: OnceLock<Mutex<HashMap<(String, String), String>>> = OnceLock::new();
static CLOUD_SYNC_OPERATION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static CLOUD_SYNC_WAKEUP: OnceLock<Arc<Notify>> = OnceLock::new();
static CLOUD_MODE_WAKEUP: OnceLock<Arc<Notify>> = OnceLock::new();
static CLOUD_SYNC_APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();
static CLOUD_SYNC_SIGNALS: AtomicU8 = AtomicU8::new(0);
static CLOUD_SYNC_OBSERVED_REMOTE_SEQ: AtomicI64 = AtomicI64::new(0);
static CLOUD_SYNC_ENABLED: AtomicBool = AtomicBool::new(false);
static LEGACY_SYNC_ENABLED: AtomicBool = AtomicBool::new(false);
static CLOUD_SYNC_ACTIVE: AtomicBool = AtomicBool::new(false);
static CLOUD_SAVE_ACTIVITY: AtomicUsize = AtomicUsize::new(0);
#[cfg(test)]
static CLOUD_SYNC_TEST_STATE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub fn spawn_background_services(database: Database, app: AppHandle) {
    let _ = CLOUD_SYNC_APP_HANDLE.set(app.clone());
    spawn_sync_coordinator(database.clone(), app.clone());
    spawn_tracking_lease_maintainer(database.clone(), app.clone());
    spawn_realtime_listener(database.clone(), app);
    match storage_mode(&database) {
        Ok(state)
            if state.mode == "cloud"
                && state.storage_protocol == crate::storage_contract::DIRECT_CLOUD_PROTOCOL =>
        {
            enter_direct_cloud_mode();
        }
        Ok(state) if state.mode == "cloud" => wake_after_connection_ready(),
        _ => disable_cloud_sync(),
    }
}

fn cloud_sync_wakeup() -> Arc<Notify> {
    CLOUD_SYNC_WAKEUP
        .get_or_init(|| Arc::new(Notify::new()))
        .clone()
}

fn cloud_mode_wakeup() -> Arc<Notify> {
    CLOUD_MODE_WAKEUP
        .get_or_init(|| Arc::new(Notify::new()))
        .clone()
}

fn signal_cloud_sync(signal: u8, observed_change_seq: Option<i64>) {
    CLOUD_SYNC_ACTIVE.store(true, Ordering::Release);
    CLOUD_SYNC_SIGNALS.fetch_or(signal, Ordering::Release);
    if let Some(observed_change_seq) = observed_change_seq {
        CLOUD_SYNC_OBSERVED_REMOTE_SEQ.fetch_max(observed_change_seq, Ordering::AcqRel);
    }
    cloud_sync_wakeup().notify_one();
}

fn begin_cloud_save_activity() {
    CLOUD_SAVE_ACTIVITY.fetch_add(1, Ordering::AcqRel);
}

fn end_cloud_save_activity() {
    let _ = CLOUD_SAVE_ACTIVITY.fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
        Some(value.saturating_sub(1))
    });
}

pub(crate) fn wake_after_connection_ready() {
    CLOUD_SYNC_ENABLED.store(true, Ordering::Release);
    LEGACY_SYNC_ENABLED.store(true, Ordering::Release);
    cloud_mode_wakeup().notify_waiters();
    signal_cloud_sync(SYNC_SIGNAL_CONNECTION_READY, None);
}

pub(crate) fn enter_direct_cloud_mode() {
    CLOUD_SYNC_ENABLED.store(true, Ordering::Release);
    LEGACY_SYNC_ENABLED.store(false, Ordering::Release);
    CLOUD_SYNC_ACTIVE.store(false, Ordering::Release);
    CLOUD_SAVE_ACTIVITY.store(0, Ordering::Release);
    CLOUD_SYNC_SIGNALS.store(0, Ordering::Release);
    CLOUD_SYNC_OBSERVED_REMOTE_SEQ.store(0, Ordering::Release);
    clear_tracking_lease_tokens();
    cloud_mode_wakeup().notify_waiters();
}

pub(crate) fn disable_cloud_sync() {
    CLOUD_SYNC_ENABLED.store(false, Ordering::Release);
    LEGACY_SYNC_ENABLED.store(false, Ordering::Release);
    CLOUD_SYNC_ACTIVE.store(false, Ordering::Release);
    CLOUD_SAVE_ACTIVITY.store(0, Ordering::Release);
    CLOUD_SYNC_SIGNALS.store(0, Ordering::Release);
    CLOUD_SYNC_OBSERVED_REMOTE_SEQ.store(0, Ordering::Release);
    clear_tracking_lease_tokens();
    cloud_mode_wakeup().notify_waiters();
}

fn spawn_sync_coordinator(database: Database, app: AppHandle) {
    let wakeup = cloud_sync_wakeup();
    tauri::async_runtime::spawn(async move {
        let mut retry_after = None::<Duration>;
        loop {
            if let Some(delay) = retry_after.take() {
                tokio::select! {
                    _ = tokio::time::sleep(delay) => {
                        CLOUD_SYNC_SIGNALS.fetch_or(SYNC_SIGNAL_LOCAL_DIRTY, Ordering::Release);
                    }
                    _ = wakeup.notified() => {}
                }
            } else {
                wakeup.notified().await;
            }
            let signals = CLOUD_SYNC_SIGNALS.swap(0, Ordering::AcqRel);
            if !LEGACY_SYNC_ENABLED.load(Ordering::Acquire) {
                CLOUD_SYNC_ACTIVE.store(false, Ordering::Release);
                retry_after = None;
                continue;
            }
            if signals == 0 {
                continue;
            }
            CLOUD_SYNC_ACTIVE.store(true, Ordering::Release);
            let pending_before_round = storage_mode(&database)
                .map(|state| state.mode == "cloud" && state.pending_operations > 0)
                .unwrap_or(false);
            if pending_before_round {
                begin_cloud_save_activity();
            }
            if let Ok(status) = sync_status(&database) {
                let _ = app.emit("cloud-sync-state-changed", &status);
            }
            let database_for_round = database.clone();
            let observed_remote_seq = CLOUD_SYNC_OBSERVED_REMOTE_SEQ.swap(0, Ordering::AcqRel);
            let round = tauri::async_runtime::spawn_blocking(move || {
                run_sync_coordinator_round(&database_for_round, signals, observed_remote_seq)
            })
            .await;
            match round {
                Ok(Ok(result)) => {
                    if !result.domains.is_empty() {
                        emit_cloud_refresh_domains(
                            &app,
                            &database,
                            &result.source,
                            result.last_change_seq,
                            &result.domains,
                        );
                    } else if let Ok(status) = sync_status(&database) {
                        let _ = app.emit("cloud-sync-state-changed", &status);
                    }
                    retry_after = result.retry_after;
                }
                Ok(Err(error)) => {
                    CLOUD_SYNC_ACTIVE.store(false, Ordering::Release);
                    emit_cloud_error(&app, &database, &error);
                    retry_after = next_outbox_retry_delay(&database).ok().flatten();
                }
                Err(error) => {
                    CLOUD_SYNC_ACTIVE.store(false, Ordering::Release);
                    emit_cloud_error(
                        &app,
                        &database,
                        &format!("SYNC_WORKER_ERROR: 后台同步任务异常: {error}"),
                    );
                }
            }
            if pending_before_round {
                end_cloud_save_activity();
                if let Ok(status) = sync_status(&database) {
                    let _ = app.emit("cloud-sync-state-changed", &status);
                }
            }
            if CLOUD_SYNC_SIGNALS.load(Ordering::Acquire) != 0 {
                cloud_sync_wakeup().notify_one();
            } else if retry_after.is_none() {
                let interrupted = tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(SYNC_SETTLE_GRACE_MILLIS)) => false,
                    _ = wakeup.notified() => true,
                };
                if interrupted || CLOUD_SYNC_SIGNALS.load(Ordering::Acquire) != 0 {
                    cloud_sync_wakeup().notify_one();
                    continue;
                }
                CLOUD_SYNC_ACTIVE.store(false, Ordering::Release);
                if CLOUD_SYNC_SIGNALS.load(Ordering::Acquire) != 0 {
                    CLOUD_SYNC_ACTIVE.store(true, Ordering::Release);
                    cloud_sync_wakeup().notify_one();
                } else if let Ok(status) = sync_status(&database) {
                    let _ = app.emit("cloud-sync-state-changed", &status);
                }
            } else {
                CLOUD_SYNC_ACTIVE.store(false, Ordering::Release);
                if let Ok(status) = sync_status(&database) {
                    let _ = app.emit("cloud-sync-state-changed", &status);
                }
            }
        }
    });
}

fn spawn_tracking_lease_maintainer(database: Database, app: AppHandle) {
    let mode_wakeup = cloud_mode_wakeup();
    tauri::async_runtime::spawn(async move {
        loop {
            while !LEGACY_SYNC_ENABLED.load(Ordering::Acquire) {
                mode_wakeup.notified().await;
            }
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(30)) => {}
                _ = mode_wakeup.notified() => {
                    continue;
                }
            }
            if !LEGACY_SYNC_ENABLED.load(Ordering::Acquire) {
                continue;
            }
            let database_for_lease = database.clone();
            let result = tauri::async_runtime::spawn_blocking(move || {
                let state = storage_mode(&database_for_lease)?;
                if state.mode != "cloud" || !state.online {
                    clear_tracking_lease_token(&database_for_lease);
                    return Ok(());
                }
                if state.storage_protocol == crate::storage_contract::DIRECT_CLOUD_PROTOCOL {
                    // Direct-v2 timer writes claim the lease inside their own
                    // mutation transaction; the legacy background lease must
                    // not probe that workspace with sync_v1 RPCs.
                    return Ok(());
                }
                acquire_or_renew_tracking_lease(&database_for_lease).map(|_| ())
            })
            .await;
            if let Ok(Err(error)) = result {
                if is_workspace_epoch_stale_error(&error) {
                    signal_cloud_sync(SYNC_SIGNAL_CONNECTION_READY, None);
                }
                emit_cloud_error(&app, &database, &error);
            }
        }
    });
}

fn acquire_or_renew_tracking_lease(database: &Database) -> Result<Value, String> {
    let _operation_guard = cloud_sync_operation_guard();
    let token = current_tracking_lease_token(database);
    let result = if let Some(token) = token {
        match renew_lease_locked(
            database,
            TrackingLeaseRequest {
                lease_token: Some(token),
            },
        ) {
            Ok(value) => Ok(value),
            Err(_) => {
                clear_tracking_lease_token(database);
                acquire_lease_locked(database)
            }
        }
    } else {
        acquire_lease_locked(database)
    };
    match &result {
        Ok(value) => {
            // An expired-lease takeover is a business boundary, not a heartbeat.
            // Adopt the server image before remembering the new token so every
            // subsequent local transition starts from the authoritative version.
            adopt_expired_lease_takeover(database, value)?;
            update_tracking_lease_token(database, value);
        }
        Err(_) => clear_tracking_lease_token(database),
    }
    result
}

fn tracking_lease_key(database: &Database) -> Option<(String, String)> {
    storage_mode(database)
        .ok()
        .map(|state| (state.workspace_id, state.device_id))
}

fn current_tracking_lease_token(database: &Database) -> Option<String> {
    let key = tracking_lease_key(database)?;
    TRACKING_LEASE_TOKENS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .ok()
        .and_then(|tokens| tokens.get(&key).cloned())
}

fn update_tracking_lease_token(database: &Database, value: &Value) {
    let next_token = value
        .get("leaseToken")
        .or_else(|| value.get("lease_token"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    if value.get("acquired").and_then(Value::as_bool) == Some(false) {
        clear_tracking_lease_token(database);
        return;
    }
    if let (Some(key), Some(next_token)) = (tracking_lease_key(database), next_token) {
        if let Ok(mut tokens) = TRACKING_LEASE_TOKENS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
        {
            tokens.insert(key, next_token);
        }
    }
}

fn clear_tracking_lease_token(database: &Database) {
    let Some(key) = tracking_lease_key(database) else {
        return;
    };
    if let Ok(mut tokens) = TRACKING_LEASE_TOKENS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        tokens.remove(&key);
    }
}

fn clear_tracking_lease_tokens() {
    if let Ok(mut tokens) = TRACKING_LEASE_TOKENS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        tokens.clear();
    }
}

fn spawn_realtime_listener(database: Database, app: AppHandle) {
    let mode_wakeup = cloud_mode_wakeup();
    tauri::async_runtime::spawn(async move {
        let mut reconnect_attempt = 0_u32;
        loop {
            while !CLOUD_SYNC_ENABLED.load(Ordering::Acquire) {
                mode_wakeup.notified().await;
            }
            let config_database = database.clone();
            let config = tauri::async_runtime::spawn_blocking(move || {
                let state = storage_mode(&config_database)?;
                if state.mode != "cloud" || !state.online {
                    return Ok(None);
                }
                let api = client(&config_database)?;
                let session = current_session(&config_database)?;
                let anon_key = api.anon_key().to_string();
                Ok::<_, String>(Some((
                    state.workspace_id,
                    state.storage_protocol,
                    api.project_url,
                    anon_key,
                    session.access_token,
                )))
            })
            .await;
            let (workspace_id, storage_protocol, project_url, anon_key, access_token) = match config
            {
                Ok(Ok(Some(config))) => config,
                Ok(Ok(None)) => {
                    tokio::time::sleep(Duration::from_secs(REALTIME_RECONNECT_SECONDS)).await;
                    reconnect_attempt = 0;
                    continue;
                }
                Ok(Err(error)) => {
                    emit_cloud_error(&app, &database, &error);
                    tokio::time::sleep(Duration::from_secs(REALTIME_RECONNECT_SECONDS)).await;
                    continue;
                }
                Err(error) => {
                    emit_cloud_error(
                        &app,
                        &database,
                        &format!("SYNC_WORKER_ERROR: Realtime 配置读取失败: {error}"),
                    );
                    tokio::time::sleep(Duration::from_secs(REALTIME_RECONNECT_SECONDS)).await;
                    continue;
                }
            };
            if let Err(error) = run_realtime_connection(
                &database,
                &app,
                &workspace_id,
                &storage_protocol,
                &project_url,
                &anon_key,
                &access_token,
            )
            .await
            {
                if CLOUD_SYNC_ENABLED.load(Ordering::Acquire) {
                    emit_cloud_error(&app, &database, &error);
                }
            }
            if !CLOUD_SYNC_ENABLED.load(Ordering::Acquire) {
                reconnect_attempt = 0;
                continue;
            }
            let delay = realtime_reconnect_delay(reconnect_attempt, now_millis() as u64);
            reconnect_attempt = reconnect_attempt.saturating_add(1);
            tokio::select! {
                _ = tokio::time::sleep(delay) => {}
                _ = mode_wakeup.notified() => {}
            }
        }
    });
}

fn realtime_reconnect_delay(attempt: u32, jitter_seed: u64) -> Duration {
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

async fn run_realtime_connection(
    database: &Database,
    app: &AppHandle,
    workspace_id: &str,
    storage_protocol: &str,
    project_url: &str,
    anon_key: &str,
    access_token: &str,
) -> Result<(), String> {
    let realtime_url = realtime_url(project_url, anon_key)?;
    let (socket, _) = connect_async(realtime_url.as_str())
        .await
        .map_err(|error| format!("NETWORK_ERROR: Supabase Realtime 连接失败: {error}"))?;
    let (mut writer, mut reader) = socket.split();
    let topic = format!("realtime:{CLOUD_SCHEMA}:workspace_changes:{workspace_id}");
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
                            "schema": CLOUD_SCHEMA,
                            "table": "workspace_changes",
                            "filter": format!("workspace_id=eq.{workspace_id}")
                        }],
                        "private": false
                    },
                    "access_token": access_token
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
    let mode_wakeup = cloud_mode_wakeup();
    loop {
        tokio::select! {
            _ = mode_wakeup.notified() => {
                if !CLOUD_SYNC_ENABLED.load(Ordering::Acquire) {
                    let _ = writer.send(Message::Close(None)).await;
                    return Ok(());
                }
            }
            _ = heartbeat.tick() => {
                let heartbeat_database = database.clone();
                let current_state = tauri::async_runtime::spawn_blocking(move || storage_mode(&heartbeat_database))
                    .await
                    .map_err(|error| format!("SYNC_WORKER_ERROR: Realtime 状态读取失败: {error}"))??;
                if current_state.mode != "cloud" || current_state.workspace_id != workspace_id {
                    let _ = writer.send(Message::Close(None)).await;
                    return Ok(());
                }
                let reference = heartbeat_ref.to_string();
                heartbeat_ref = heartbeat_ref.wrapping_add(1);
                writer.send(Message::Text(json!({
                    "topic": "phoenix",
                    "event": "heartbeat",
                    "payload": {},
                    "ref": reference
                }).to_string().into())).await
                    .map_err(|error| format!("NETWORK_ERROR: Supabase Realtime 心跳失败: {error}"))?;
            }
            incoming = reader.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        let message: Value = serde_json::from_str(text.as_ref()).unwrap_or(Value::Null);
                        let event = message.get("event").and_then(Value::as_str).unwrap_or_default();
                        let join_failed = event == "phx_reply"
                            && message.get("ref").and_then(Value::as_str) == Some("1")
                            && message.pointer("/payload/status").and_then(Value::as_str) == Some("error");
                        if join_failed {
                            let reason = message
                                .pointer("/payload/response/reason")
                                .and_then(Value::as_str)
                                .unwrap_or("频道订阅被拒绝");
                            return Err(format!("CLOUD_REQUEST_FAILED: Supabase Realtime {reason}"));
                        } else if event == "phx_reply"
                            && message.get("ref").and_then(Value::as_str) == Some("1")
                            && message.pointer("/payload/status").and_then(Value::as_str) == Some("ok")
                        {
                            if storage_protocol
                                != crate::storage_contract::DIRECT_CLOUD_PROTOCOL
                            {
                                signal_cloud_sync(SYNC_SIGNAL_CONNECTION_READY, None);
                            } else {
                                spawn_direct_cloud_catch_up(database, app);
                            }
                        } else if event == "postgres_changes" {
                            if storage_protocol
                                == crate::storage_contract::DIRECT_CLOUD_PROTOCOL
                            {
                                if let Some(invalidation) = realtime_workspace_invalidation(
                                    &message,
                                    workspace_id,
                                    current_workspace_epoch(database, workspace_id).unwrap_or(1),
                                ) {
                                    let _ = app.emit("work-data-changed", &invalidation);
                                }
                            } else {
                                signal_cloud_sync(
                                    SYNC_SIGNAL_REMOTE_HINT,
                                    realtime_change_seq(&message),
                                );
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
}

fn realtime_change_seq(message: &Value) -> Option<i64> {
    message
        .pointer("/payload/data/record/change_seq")
        .or_else(|| message.pointer("/payload/record/change_seq"))
        .or_else(|| message.pointer("/payload/data/new/change_seq"))
        .and_then(|value| value.as_i64().or_else(|| value.as_str()?.parse().ok()))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceInvalidation {
    workspace_id: String,
    change_seq: i64,
    workspace_epoch: i64,
    domains: Vec<String>,
    source: String,
}

fn realtime_workspace_invalidation(
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
    if domains.is_empty() {
        return None;
    }
    Some(WorkspaceInvalidation {
        workspace_id: workspace_id.to_string(),
        change_seq,
        workspace_epoch,
        domains: domains.into_iter().map(str::to_string).collect(),
        source: "realtime".to_string(),
    })
}

fn realtime_refresh_domains(entity_type: &str) -> Vec<&'static str> {
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

fn current_workspace_epoch(database: &Database, workspace_id: &str) -> Result<i64, String> {
    database
        .open()?
        .query_row(
            "SELECT workspace_epoch FROM local_sync_state WHERE workspace_id=?1",
            [workspace_id],
            |row| row.get(0),
        )
        .optional()
        .map(|value| value.unwrap_or(1))
        .map_err(|error| error.to_string())
}

fn realtime_url(project_url: &str, anon_key: &str) -> Result<Url, String> {
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

fn emit_cloud_refresh_domains(
    app: &AppHandle,
    database: &Database,
    source: &str,
    revision: i64,
    domains: &[String],
) {
    if let Ok(status) = sync_status(database) {
        let _ = app.emit("cloud-sync-state-changed", &status);
        let _ = app.emit(
            "work-data-changed",
            &WorkspaceInvalidation {
                workspace_id: status.workspace_id,
                change_seq: revision,
                workspace_epoch: status.workspace_epoch,
                domains: domains.to_vec(),
                source: source.to_string(),
            },
        );
    }
}

fn emit_cloud_error(app: &AppHandle, database: &Database, error: &str) {
    if let Ok(status) = sync_status(database) {
        if status.mode == "cloud" {
            let _ = update_sync_state(
                database,
                &status.workspace_id,
                &status.device_id,
                status.last_change_seq,
                Some(error),
            );
        }
        let status = sync_status(database).unwrap_or(status);
        let _ = app.emit(
            "cloud-sync-state-changed",
            json!({
                "mode": status.mode,
                "storageProtocol": status.storage_protocol,
                "storageBackend": status.storage_backend,
                "cloudAccessState": status.cloud_access_state,
                "cloudWritable": status.cloud_writable,
                "readOnlyReason": status.read_only_reason,
                "workspaceId": status.workspace_id,
                "deviceId": status.device_id,
                "online": false,
                "syncState": status.sync_state,
                "syncing": false,
                "saving": false,
                "lastChangeSeq": status.last_change_seq,
                "lastSyncedAt": status.last_synced_at,
                "lastError": status.last_error.as_deref().unwrap_or(error),
                "pendingOperations": status.pending_operations,
                "conflictCount": status.conflict_count,
                "authBlocked": status.auth_blocked,
                "authBlockedReason": status.auth_blocked_reason,
                "workspaceEpoch": status.workspace_epoch,
                "epochBlocked": status.epoch_blocked,
                "epochBlockedReason": status.epoch_blocked_reason,
                "error": error
            }),
        );
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudSyncStatus {
    pub mode: String,
    pub storage_protocol: String,
    pub storage_backend: String,
    pub cloud_access_state: String,
    pub cloud_writable: bool,
    pub read_only_reason: Option<String>,
    pub workspace_id: String,
    pub device_id: String,
    pub online: bool,
    pub sync_state: String,
    pub syncing: bool,
    pub saving: bool,
    pub last_change_seq: i64,
    pub last_synced_at: Option<i64>,
    pub last_error: Option<String>,
    pub pending_operations: i64,
    pub conflict_count: i64,
    pub auth_blocked: bool,
    pub auth_blocked_reason: Option<String>,
    pub workspace_epoch: i64,
    pub epoch_blocked: bool,
    pub epoch_blocked_reason: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalCacheResetCount {
    pub entity: String,
    pub local_count: i64,
    pub cloud_count: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalCacheResetPreview {
    pub workspace_id: String,
    pub pending_operations: i64,
    pub conflict_count: i64,
    pub local_total: i64,
    pub cloud_total: i64,
    pub entities: Vec<LocalCacheResetCount>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudSyncPullRequest {
    pub after_change_seq: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceChange {
    #[serde(alias = "change_seq")]
    pub change_seq: i64,
    #[serde(alias = "workspace_id")]
    pub workspace_id: String,
    #[serde(alias = "entity_type")]
    pub entity_type: String,
    #[serde(alias = "entity_id")]
    pub entity_id: String,
    pub operation: String,
    #[serde(alias = "entity_version")]
    pub entity_version: i64,
    #[serde(alias = "changed_at")]
    pub changed_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudSyncPullResult {
    pub changes: Vec<WorkspaceChange>,
    pub last_change_seq: i64,
    pub has_more: bool,
    pub domains: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct IncrementalPullResponse {
    #[serde(default)]
    changes: Vec<WorkspaceChange>,
    #[serde(default)]
    entities: Vec<IncrementalEntity>,
    #[serde(default, alias = "coveredChangeSeq")]
    covered_change_seq: i64,
    #[serde(default, alias = "latestChangeSeq")]
    latest_change_seq: i64,
    #[serde(default, alias = "hasMore")]
    has_more: bool,
    #[serde(default, alias = "resetRequired")]
    reset_required: bool,
    #[serde(default = "default_workspace_epoch", alias = "workspaceEpoch")]
    workspace_epoch: i64,
}

fn default_workspace_epoch() -> i64 {
    1
}

#[derive(Debug, Clone, Deserialize)]
struct IncrementalEntity {
    #[serde(alias = "entityType")]
    entity_type: String,
    #[serde(alias = "sourceEntityType")]
    source_entity_type: String,
    #[serde(alias = "entityId")]
    entity_id: String,
    #[serde(alias = "changeSeq")]
    change_seq: i64,
    deleted: bool,
    data: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct SharedUnassignedPayload {
    id: String,
    state: String,
    version: i64,
    #[serde(deserialize_with = "deserialize_millis")]
    first_started_at: i64,
    #[serde(default, deserialize_with = "deserialize_optional_millis")]
    resolved_at: Option<i64>,
    #[serde(default)]
    resolution_type: Option<String>,
    #[serde(default)]
    predecessor_session_id: Option<String>,
    #[serde(default)]
    segments: Vec<SharedUnassignedSegmentPayload>,
}

#[derive(Debug, Deserialize)]
struct SharedUnassignedSegmentPayload {
    id: String,
    session_id: String,
    sequence_no: i64,
    #[serde(deserialize_with = "deserialize_millis")]
    started_at: i64,
    #[serde(default, deserialize_with = "deserialize_optional_millis")]
    ended_at: Option<i64>,
    duration_seconds: i64,
}

fn deserialize_millis<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    timestamp_value_millis(&value)
        .ok_or_else(|| serde::de::Error::custom("expected Unix milliseconds or RFC 3339 timestamp"))
}

fn deserialize_optional_millis<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    if value.is_null() {
        return Ok(None);
    }
    timestamp_value_millis(&value)
        .map(Some)
        .ok_or_else(|| serde::de::Error::custom("expected Unix milliseconds or RFC 3339 timestamp"))
}

fn validate_shared_unassigned_payload(
    entity_id: &str,
    data: &Value,
) -> Result<SharedUnassignedPayload, String> {
    let payload: SharedUnassignedPayload = serde_json::from_value(data.clone())
        .map_err(|error| format!("VALIDATION_ERROR: 共享未归属会话格式无效: {error}"))?;
    if payload.id != entity_id || Uuid::parse_str(&payload.id).is_err() {
        return Err("VALIDATION_ERROR: 共享未归属会话身份不匹配".to_string());
    }
    if payload.version < 1 || payload.first_started_at <= 0 {
        return Err("VALIDATION_ERROR: 共享未归属会话版本或开始时间无效".to_string());
    }
    if !matches!(
        payload.state.as_str(),
        "collecting" | "awaiting_resolution" | "resolved" | "discarded"
    ) {
        return Err("VALIDATION_ERROR: 共享未归属会话状态无效".to_string());
    }
    if matches!(payload.state.as_str(), "resolved" | "discarded")
        && (payload.resolved_at.is_none() || payload.resolution_type.is_none())
    {
        return Err("VALIDATION_ERROR: 已处理共享未归属会话缺少终态信息".to_string());
    }
    if let Some(predecessor) = payload.predecessor_session_id.as_deref() {
        if Uuid::parse_str(predecessor).is_err() || predecessor == payload.id {
            return Err("VALIDATION_ERROR: 共享未归属会话前序身份无效".to_string());
        }
    }
    for segment in &payload.segments {
        let reason = if Uuid::parse_str(&segment.id).is_err() {
            Some("分段 ID 无效")
        } else if segment.session_id != payload.id {
            Some("分段所属会话不匹配")
        } else if segment.sequence_no < 1 {
            Some("分段序号无效")
        } else if segment.started_at <= 0 {
            Some("分段开始时间无效")
        } else if segment.duration_seconds < 0 {
            Some("分段时长为负数")
        } else if segment
            .ended_at
            .is_some_and(|ended_at| ended_at < segment.started_at)
        {
            Some("分段结束时间早于开始时间")
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(format!(
                "VALIDATION_ERROR: 共享未归属会话分段无效: {reason}（序号 {}）",
                segment.sequence_no
            ));
        }
    }
    Ok(payload)
}

struct SyncCoordinatorRoundResult {
    domains: Vec<String>,
    last_change_seq: i64,
    source: String,
    retry_after: Option<Duration>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudSyncPushResult {
    pub pushed: i64,
    pub pending: i64,
    pub conflicts: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LegacyQueueDrainResult {
    pub(crate) pushed: i64,
    pub(crate) last_change_seq: i64,
    pub(crate) domains: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudSyncQueueItem {
    pub operation_id: String,
    pub operation_type: String,
    pub entity_type: String,
    pub entity_id: Option<String>,
    pub state: String,
    pub payload: Value,
    pub attempt_count: i64,
    pub error: Option<String>,
    pub coalesced_count: i64,
    pub base_version: Option<i64>,
    pub payload_version: Option<i64>,
    pub depends_on_operation_id: Option<String>,
    pub recovery_kind: Option<String>,
    pub recovery_context: Option<Value>,
    pub created_at: i64,
    pub last_attempt_at: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudSyncConflict {
    pub operation_id: String,
    pub operation_type: String,
    pub entity_type: String,
    pub entity_id: Option<String>,
    pub base_version: Option<i64>,
    pub local_payload: Value,
    pub cloud_payload: Option<Value>,
    pub cloud_payload_error: Option<String>,
    pub error: Option<String>,
    pub conflict_reason: Option<String>,
    pub attempt_count: i64,
    pub created_at: i64,
    pub last_attempt_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudConflictResolveRequest {
    pub operation_id: String,
    pub strategy: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudConflictResolveAllRequest {
    pub strategy: String,
}

#[derive(Debug, Clone)]
struct OutboxOperation {
    operation_id: String,
    workspace_epoch: i64,
    operation_type: String,
    entity_type: String,
    entity_id: Option<String>,
    base_version: Option<i64>,
    payload_version: Option<i64>,
    coalesced_count: i64,
    depends_on_operation_id: Option<String>,
    payload: Value,
}

#[derive(Debug, PartialEq, Eq)]
enum TimerSyncRecovery {
    Acknowledged,
    Rebased,
    Followed { entity_id: String, version: i64 },
    Conflict(String),
}

pub(crate) fn stable_timer_operation_id(
    entity_id: &str,
    base_version: Option<i64>,
    payload_version: i64,
    operation_type: &str,
) -> String {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!(
            "timegenie:timer-operation:{entity_id}:{}:{payload_version}:{operation_type}",
            base_version
                .map(|value| value.to_string())
                .unwrap_or_else(|| "new".to_string())
        )
        .as_bytes(),
    )
    .to_string()
}

struct ActiveTimerEntryOverlay {
    entry_id: String,
    timer_chain_id: Option<String>,
    previous_entry_id: Option<String>,
    split_boundary_at: Option<i64>,
    last_continuous_at: Option<i64>,
    work_date: String,
    kind: String,
    source_type: String,
    state: String,
    default_task_id: Option<String>,
    label_snapshot: String,
    started_at: i64,
    ended_at: Option<i64>,
    duration_seconds: i64,
    note: Option<String>,
    origin_unassigned_session_id: Option<String>,
    created_at: i64,
    updated_at: i64,
    version: i64,
    deleted_at: Option<i64>,
}

struct ActiveTimerSegmentOverlay {
    entry_id: String,
    segment_id: String,
    sequence_no: i64,
    started_at: i64,
    duration_seconds: i64,
}

struct ActiveUnassignedSegmentOverlay {
    session_id: String,
    segment_id: String,
    sequence_no: i64,
    started_at: i64,
    duration_seconds: i64,
    lease_token: Option<String>,
}

struct ActiveUnassignedSessionOverlay {
    session_id: String,
    last_continuous_at: Option<i64>,
    work_date: String,
    state: String,
    threshold_seconds: i64,
    duration_seconds: i64,
    first_started_at: i64,
    last_ended_at: Option<i64>,
    prompted_at: Option<i64>,
    resolution_type: Option<String>,
    generated_entry_id: Option<String>,
    resolved_at: Option<i64>,
    created_at: i64,
    updated_at: i64,
    version: i64,
}

#[derive(Default)]
struct LocalTrackingOverlay {
    active_timer_entries: Vec<ActiveTimerEntryOverlay>,
    active_timer_segments: Vec<ActiveTimerSegmentOverlay>,
    active_unassigned_sessions: Vec<ActiveUnassignedSessionOverlay>,
    active_unassigned_segments: Vec<ActiveUnassignedSegmentOverlay>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackingLeaseRequest {
    pub lease_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudTimerStartRequest {
    pub task_id: Option<String>,
    pub note: Option<String>,
    pub operation_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudTimerVersionRequest {
    pub entry_id: String,
    pub expected_version: i64,
    pub operation_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudUnassignedResolveRequest {
    pub session_id: String,
    pub expected_version: i64,
    pub allocations: Option<Vec<Value>>,
    pub operation_id: String,
}

pub fn sync_status(database: &Database) -> Result<CloudSyncStatus, String> {
    let state = storage_mode(database)?;
    Ok(CloudSyncStatus {
        mode: state.mode,
        storage_protocol: state.storage_protocol,
        storage_backend: state.storage_backend,
        cloud_access_state: state.cloud_access_state,
        cloud_writable: state.cloud_writable,
        read_only_reason: state.read_only_reason,
        workspace_id: state.workspace_id,
        device_id: state.device_id,
        online: state.online,
        sync_state: state.sync_state,
        syncing: CLOUD_SYNC_ACTIVE.load(Ordering::Acquire),
        saving: CLOUD_SAVE_ACTIVITY.load(Ordering::Acquire) > 0,
        last_change_seq: state.last_change_seq,
        last_synced_at: state.last_synced_at,
        last_error: state.last_error,
        pending_operations: state.pending_operations,
        conflict_count: state.conflict_count,
        auth_blocked: state.auth_blocked,
        auth_blocked_reason: state.auth_blocked_reason,
        workspace_epoch: state.workspace_epoch,
        epoch_blocked: state.epoch_blocked,
        epoch_blocked_reason: state.epoch_blocked_reason,
    })
}

fn run_sync_coordinator_round(
    database: &Database,
    signals: u8,
    observed_remote_seq: i64,
) -> Result<SyncCoordinatorRoundResult, String> {
    let _operation_guard = cloud_sync_operation_guard();
    let state = storage_mode(database)?;
    if state.mode != "cloud" {
        CLOUD_SYNC_ENABLED.store(false, Ordering::Release);
        LEGACY_SYNC_ENABLED.store(false, Ordering::Release);
        clear_tracking_lease_token(database);
        return Ok(SyncCoordinatorRoundResult {
            domains: Vec::new(),
            last_change_seq: state.last_change_seq,
            source: "local-mode".to_string(),
            retry_after: None,
        });
    }
    if state.storage_protocol == crate::storage_contract::DIRECT_CLOUD_PROTOCOL {
        LEGACY_SYNC_ENABLED.store(false, Ordering::Release);
        clear_tracking_lease_token(database);
        return Ok(SyncCoordinatorRoundResult {
            domains: Vec::new(),
            last_change_seq: state.last_change_seq,
            source: "direct-cloud".to_string(),
            retry_after: None,
        });
    }
    CLOUD_SYNC_ENABLED.store(true, Ordering::Release);
    LEGACY_SYNC_ENABLED.store(true, Ordering::Release);
    if state.epoch_blocked {
        return Ok(SyncCoordinatorRoundResult {
            domains: Vec::new(),
            last_change_seq: state.last_change_seq,
            source: "epoch-blocked".to_string(),
            retry_after: None,
        });
    }
    if !state.online {
        return Ok(SyncCoordinatorRoundResult {
            domains: Vec::new(),
            last_change_seq: state.last_change_seq,
            source: "offline".to_string(),
            retry_after: next_outbox_retry_delay(database)?,
        });
    }
    clear_stale_deferred_workspaces(database, &state.workspace_id)?;
    let only_remote_hint = signals
        & (SYNC_SIGNAL_LOCAL_DIRTY | SYNC_SIGNAL_CONNECTION_READY | SYNC_SIGNAL_MANUAL_RETRY)
        == 0;
    if only_remote_hint
        && observed_remote_seq > 0
        && observed_remote_seq <= state.last_change_seq
        && state.pending_operations == 0
        && state.conflict_count == 0
    {
        return Ok(SyncCoordinatorRoundResult {
            domains: Vec::new(),
            last_change_seq: state.last_change_seq,
            source: "realtime-duplicate".to_string(),
            retry_after: None,
        });
    }
    recover_interrupted_sending_operations(database, &state.workspace_id)?;
    let push = match push_outbox_locked(database, signals & SYNC_SIGNAL_MANUAL_RETRY != 0, true) {
        Ok(push) => push,
        Err(error) if is_workspace_epoch_stale_error(&error) => {
            let server_epoch = parse_server_workspace_epoch(&error).ok_or(error)?;
            if has_old_epoch_local_changes(database, &state.workspace_id, server_epoch)? {
                mark_epoch_blocked(
                    database,
                    &state.workspace_id,
                    server_epoch,
                    "云端工作空间已恢复，本机仍保留旧世代修改",
                )?;
                return Err(
                    "WORKSPACE_EPOCH_STALE: 本机仍有旧世代修改，请先导出救援备份或明确放弃后重新下载"
                        .to_string(),
                );
            }
            let last_change_seq = pull_snapshot_for_epoch(database, server_epoch, true)?;
            return Ok(SyncCoordinatorRoundResult {
                domains: vec!["subjects", "tasks", "time", "reports", "settings"]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
                last_change_seq,
                source: "epoch-reset".to_string(),
                retry_after: None,
            });
        }
        Err(error) => return Err(error),
    };
    let mut domains = HashSet::<String>::new();
    rewind_for_ready_deferred_entities(database, &state.workspace_id)?;
    let mut last_change_seq = storage_mode(database)?.last_change_seq;
    if push.conflicts == 0 {
        loop {
            let pulled = pull_locked(
                database,
                CloudSyncPullRequest {
                    after_change_seq: Some(last_change_seq),
                    limit: Some(200),
                },
            )?;
            let previous_seq = last_change_seq;
            last_change_seq = pulled.last_change_seq.max(last_change_seq);
            domains.extend(pulled.domains);
            if !pulled.has_more || last_change_seq <= previous_seq {
                break;
            }
            std::thread::yield_now();
        }
    }
    let source = if signals & SYNC_SIGNAL_MANUAL_RETRY != 0 {
        "manual-retry"
    } else if signals & SYNC_SIGNAL_REMOTE_HINT != 0 || observed_remote_seq > state.last_change_seq
    {
        "realtime"
    } else if signals & SYNC_SIGNAL_CONNECTION_READY != 0 {
        "catch-up"
    } else {
        "outbox"
    };
    let mut domains = domains.into_iter().collect::<Vec<_>>();
    domains.sort();
    let retry_after = next_outbox_retry_delay(database)?;
    Ok(SyncCoordinatorRoundResult {
        domains,
        last_change_seq,
        source: source.to_string(),
        retry_after,
    })
}
#[cfg(test)]
pub(crate) fn run_sync_coordinator_round_for_test(database: &Database) -> Result<String, String> {
    run_sync_coordinator_round(database, SYNC_SIGNAL_LOCAL_DIRTY, 0).map(|result| result.source)
}

fn clear_stale_deferred_workspaces(
    database: &Database,
    current_workspace_id: &str,
) -> Result<(), String> {
    database
        .open()?
        .execute(
            "DELETE FROM cloud_deferred_entities WHERE workspace_id <> ?1",
            [current_workspace_id],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn next_outbox_retry_delay(database: &Database) -> Result<Option<Duration>, String> {
    let state = storage_mode(database)?;
    if state.mode != "cloud" {
        return Ok(None);
    }
    let connection = database.open()?;
    let mut statement = connection.prepare("SELECT last_attempt_at, attempt_count FROM sync_outbox WHERE workspace_id=?1 AND state IN ('pending','failed')").map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([state.workspace_id], |row| {
            Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|error| error.to_string())?;
    let now = now_millis();
    let mut minimum = None::<i64>;
    for row in rows {
        let (last_attempt_at, attempt_count) = row.map_err(|error| error.to_string())?;
        let ready_at = retry_ready_at(last_attempt_at, attempt_count).unwrap_or(now);
        minimum = Some(minimum.map_or(ready_at, |value| value.min(ready_at)));
    }
    Ok(minimum
        .map(|ready_at| Duration::from_millis(ready_at.saturating_sub(now).max(1_000) as u64)))
}

fn rewind_for_ready_deferred_entities(
    database: &Database,
    cloud_workspace_id: &str,
) -> Result<(), String> {
    let connection = database.open()?;
    let rewind_to = connection.query_row(
        "SELECT MIN(deferred.observed_change_seq - 1) FROM cloud_deferred_entities deferred
         WHERE deferred.workspace_id=?1 AND NOT EXISTS(
           SELECT 1 FROM sync_outbox outbox
           WHERE outbox.workspace_id=deferred.workspace_id
             AND outbox.entity_id=deferred.entity_id
             AND outbox.entity_type=CASE deferred.entity_type
               WHEN 'subjects' THEN 'subject' WHEN 'tasks' THEN 'task'
               WHEN 'subject_work_schedules' THEN 'subject_work_schedule'
               WHEN 'task_daily_estimates' THEN 'task_daily_estimate'
               WHEN 'task_day_plans' THEN 'task_day_plan'
               WHEN 'task_recurrence_rules' THEN 'task_recurrence_rule'
               WHEN 'task_occurrences' THEN 'task_occurrence'
               WHEN 'time_entries' THEN 'time_entry'
               WHEN 'report_templates' THEN 'report_template' WHEN 'reports' THEN 'report'
               WHEN 'app_settings' THEN 'app_setting' WHEN 'integration_configs' THEN 'integration_config'
               WHEN 'external_bindings' THEN 'external_binding' ELSE deferred.entity_type END
             AND outbox.state IN ('pending','sending','failed','conflict')
         )",
        [cloud_workspace_id],
        |row| row.get::<_, Option<i64>>(0),
    ).map_err(|error| error.to_string())?;
    if let Some(rewind_to) = rewind_to {
        connection.execute(
            "UPDATE local_sync_state SET last_change_seq=MIN(last_change_seq, ?2) WHERE workspace_id=?1",
            params![cloud_workspace_id, rewind_to.max(0)],
        ).map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
pub fn pull(
    database: &Database,
    request: CloudSyncPullRequest,
) -> Result<CloudSyncPullResult, String> {
    let _operation_guard = cloud_sync_operation_guard();
    pull_locked(database, request)
}

fn catch_up_incremental(database: &Database) -> Result<CloudSyncPullResult, String> {
    let _operation_guard = cloud_sync_operation_guard();
    let state = storage_mode(database)?;
    if state.mode != "cloud" || !state.online {
        return Ok(CloudSyncPullResult {
            changes: Vec::new(),
            last_change_seq: state.last_change_seq,
            has_more: false,
            domains: Vec::new(),
        });
    }
    if state.storage_protocol == crate::storage_contract::DIRECT_CLOUD_PROTOCOL {
        return catch_up_direct_cloud(database, state);
    }
    let mut last_change_seq = state.last_change_seq;
    let mut changes = Vec::new();
    let mut domains = HashSet::new();
    loop {
        let pulled = pull_locked(
            database,
            CloudSyncPullRequest {
                after_change_seq: Some(last_change_seq),
                limit: Some(500),
            },
        )?;
        let previous_seq = last_change_seq;
        last_change_seq = pulled.last_change_seq.max(last_change_seq);
        changes.extend(pulled.changes);
        domains.extend(pulled.domains);
        if !pulled.has_more || last_change_seq <= previous_seq {
            break;
        }
    }
    let mut domains = domains.into_iter().collect::<Vec<_>>();
    domains.sort();
    Ok(CloudSyncPullResult {
        changes,
        last_change_seq,
        has_more: false,
        domains,
    })
}

fn catch_up_direct_cloud(
    database: &Database,
    previous: crate::supabase::StorageModeSnapshot,
) -> Result<CloudSyncPullResult, String> {
    let response = direct_cloud_workbench_bootstrap(database)?;
    let metadata = response.metadata;
    let changed = metadata.change_seq > previous.last_change_seq
        || metadata.workspace_epoch != previous.workspace_epoch;
    let domains = if changed {
        direct_cloud_catch_up_domains()
    } else {
        Vec::new()
    };
    Ok(CloudSyncPullResult {
        changes: Vec::new(),
        last_change_seq: metadata.change_seq,
        has_more: false,
        domains,
    })
}

fn direct_cloud_catch_up_domains() -> Vec<String> {
    [
        "subjects",
        "tasks",
        "time",
        "unassigned",
        "reports",
        "settings",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn spawn_direct_cloud_catch_up(database: &Database, app: &AppHandle) {
    let database = database.clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let database_for_query = database.clone();
        let result =
            tauri::async_runtime::spawn_blocking(move || catch_up_incremental(&database_for_query))
                .await;
        match result {
            Ok(Ok(catch_up)) if !catch_up.domains.is_empty() => {
                emit_cloud_refresh_domains(
                    &app,
                    &database,
                    "catch-up",
                    catch_up.last_change_seq,
                    &catch_up.domains,
                );
            }
            Ok(Err(error)) => emit_cloud_error(&app, &database, &error),
            Err(error) => emit_cloud_error(
                &app,
                &database,
                &format!("SYNC_WORKER_ERROR: 云端补查任务异常: {error}"),
            ),
            Ok(Ok(_)) => {
                if let Ok(status) = sync_status(&database) {
                    let _ = app.emit("cloud-sync-state-changed", &status);
                }
            }
        }
    });
}

fn pull_locked(
    database: &Database,
    request: CloudSyncPullRequest,
) -> Result<CloudSyncPullResult, String> {
    let state = storage_mode(database)?;
    if state.mode != "cloud" {
        return Err("OFFLINE_RESTRICTED: 只有云端模式可以拉取 Supabase 增量".to_string());
    }
    let session = current_session(database)?;
    let after = request
        .after_change_seq
        .unwrap_or(state.last_change_seq)
        .max(0);
    let limit = request.limit.unwrap_or(500).clamp(1, 1000);
    let response = client(database)?.rpc_for_database_with_epoch(
        database,
        "cloud_incremental_pull",
        json!({
            "p_workspace_id": state.workspace_id,
            "p_device_id": state.device_id,
            "p_after_change_seq": after,
            "p_limit": limit
        }),
        &session,
        state.workspace_epoch,
    );
    let response = match response {
        Ok(response) => response,
        Err(error) if is_workspace_epoch_stale_error(&error) => {
            let server_epoch = parse_server_workspace_epoch(&error).ok_or(error)?;
            if has_old_epoch_local_changes(database, &state.workspace_id, server_epoch)? {
                mark_epoch_blocked(
                    database,
                    &state.workspace_id,
                    server_epoch,
                    "云端工作空间已恢复，本机仍保留旧世代修改",
                )?;
                return Err(
                    "WORKSPACE_EPOCH_STALE: 本机仍有旧世代修改，请先导出救援备份或明确放弃后重新下载"
                        .to_string(),
                );
            }
            let last_change_seq = pull_snapshot_for_epoch(database, server_epoch, true)?;
            return Ok(CloudSyncPullResult {
                changes: Vec::new(),
                last_change_seq,
                has_more: false,
                domains: vec!["subjects", "tasks", "time", "reports", "settings"]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
            });
        }
        Err(error) => return Err(error),
    };
    let batch: IncrementalPullResponse = serde_json::from_value(response)
        .map_err(|error| format!("NETWORK_ERROR: 云端增量响应无效: {error}"))?;
    if batch.reset_required || batch.workspace_epoch != state.workspace_epoch {
        if has_old_epoch_local_changes(database, &state.workspace_id, batch.workspace_epoch)? {
            mark_epoch_blocked(
                database,
                &state.workspace_id,
                batch.workspace_epoch,
                "云端工作空间已回滚，本机仍保留旧世代修改",
            )?;
            return Err(
                "WORKSPACE_EPOCH_STALE: 本机仍有旧世代修改，请先导出救援备份或明确放弃后重新下载"
                    .to_string(),
            );
        }
        let last_change_seq = pull_snapshot_for_epoch(database, batch.workspace_epoch, true)?;
        return Ok(CloudSyncPullResult {
            changes: batch.changes,
            last_change_seq,
            has_more: false,
            domains: vec!["subjects", "tasks", "time", "reports", "settings"]
                .into_iter()
                .map(str::to_string)
                .collect(),
        });
    }
    let last_change_seq = batch.covered_change_seq.max(after);
    let domains = apply_incremental_entities(
        database,
        &state.workspace_id,
        &state.device_id,
        last_change_seq,
        &batch.entities,
    )?;
    Ok(CloudSyncPullResult {
        has_more: batch.has_more || last_change_seq < batch.latest_change_seq,
        changes: batch.changes,
        last_change_seq,
        domains,
    })
}

fn local_cache_workspace_id(database: &Database) -> Result<String, String> {
    database
        .open()?
        .query_row(
            "SELECT id FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())
}

fn incremental_outbox_identity(entity: &IncrementalEntity) -> (&str, &str) {
    match entity.entity_type.as_str() {
        "subjects" => ("subject", entity.entity_id.as_str()),
        "subject_work_schedules" => ("subject_work_schedule", entity.entity_id.as_str()),
        "tasks" => ("task", entity.entity_id.as_str()),
        "task_daily_estimates" => ("task_daily_estimate", entity.entity_id.as_str()),
        "task_day_plans" => ("task_day_plan", entity.entity_id.as_str()),
        "task_recurrence_rules" => ("task_recurrence_rule", entity.entity_id.as_str()),
        "task_occurrences" => ("task_occurrence", entity.entity_id.as_str()),
        "unassigned_sessions" => ("unassigned_session", entity.entity_id.as_str()),
        "time_entries" => ("time_entry", entity.entity_id.as_str()),
        "actual_time_adjustments" => ("actual_time_adjustment", entity.entity_id.as_str()),
        "report_templates" => ("report_template", entity.entity_id.as_str()),
        "reports" => ("report", entity.entity_id.as_str()),
        "app_settings" => ("app_setting", entity.entity_id.as_str()),
        "integration_configs" => ("integration_config", entity.entity_id.as_str()),
        "external_bindings" => ("external_binding", entity.entity_id.as_str()),
        other => (other, entity.entity_id.as_str()),
    }
}

fn expected_incremental_entity_type(source_entity_type: &str) -> Option<&str> {
    match source_entity_type {
        "time_segments" | "time_allocations" => Some("time_entries"),
        "report_tasks" => Some("reports"),
        "subjects"
        | "subject_work_schedules"
        | "work_days"
        | "app_settings"
        | "tasks"
        | "task_status_events"
        | "task_daily_estimates"
        | "task_day_plans"
        | "task_recurrence_rules"
        | "task_occurrences"
        | "unassigned_sessions"
        | "time_entries"
        | "actual_time_adjustments"
        | "report_templates"
        | "reports"
        | "integration_configs"
        | "external_bindings" => Some(source_entity_type),
        _ => None,
    }
}

fn incremental_domain(entity_type: &str) -> Result<&'static str, String> {
    match entity_type {
        "subjects" => Ok("subjects"),
        "subject_work_schedules" => Ok("subjects"),
        "tasks"
        | "task_status_events"
        | "task_daily_estimates"
        | "task_day_plans"
        | "task_recurrence_rules"
        | "task_occurrences" => Ok("tasks"),
        "work_days" | "time_entries" | "actual_time_adjustments" => Ok("time"),
        "unassigned_sessions" => Ok("unassigned"),
        "report_templates" | "reports" => Ok("reports"),
        "app_settings" | "integration_configs" | "external_bindings" => Ok("settings"),
        other => Err(format!("VALIDATION_ERROR: 不支持的云端增量实体 {other}")),
    }
}

fn entity_has_dirty_outbox(
    transaction: &Transaction<'_>,
    cloud_workspace_id: &str,
    entity: &IncrementalEntity,
) -> Result<bool, String> {
    let (entity_type, entity_id) = incremental_outbox_identity(entity);
    transaction
        .query_row(
            "SELECT EXISTS(
               SELECT 1 FROM sync_outbox
               WHERE workspace_id = ?1 AND entity_type = ?2 AND entity_id = ?3
                 AND state IN ('pending','sending','failed','conflict')
             )",
            params![cloud_workspace_id, entity_type, entity_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())
}

fn acknowledge_authoritative_unassigned_candidate(
    transaction: &Transaction<'_>,
    cloud_workspace_id: &str,
    entity: &IncrementalEntity,
) -> Result<(), String> {
    if entity.entity_type != "unassigned_sessions" || entity.deleted {
        return Ok(());
    }
    transaction
        .execute(
            "DELETE FROM sync_outbox
             WHERE workspace_id=?1 AND entity_type='unassigned_session'
               AND entity_id=?2 AND operation_type='unassigned_session_create'
               AND state IN ('pending','sending','failed')
               AND NOT EXISTS(
                 SELECT 1 FROM sync_outbox dependent
                 WHERE dependent.workspace_id=sync_outbox.workspace_id
                   AND dependent.depends_on_operation_id=sync_outbox.operation_id
               )",
            params![cloud_workspace_id, entity.entity_id],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn defer_incremental_entity(
    transaction: &Transaction<'_>,
    cloud_workspace_id: &str,
    entity: &IncrementalEntity,
) -> Result<(), String> {
    let version = entity
        .data
        .as_ref()
        .and_then(|value| value.get("version"))
        .and_then(Value::as_i64)
        .unwrap_or_default();
    transaction
        .execute(
            "INSERT INTO cloud_deferred_entities(workspace_id, entity_type, entity_id, observed_version, observed_change_seq, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(workspace_id, entity_type, entity_id) DO UPDATE SET
               observed_version = MAX(observed_version, excluded.observed_version),
               observed_change_seq = MAX(observed_change_seq, excluded.observed_change_seq),
               updated_at = excluded.updated_at",
            params![
                cloud_workspace_id,
                entity.entity_type,
                entity.entity_id,
                version,
                entity.change_seq,
                now_millis()
            ],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn clear_deferred_entity(
    transaction: &Transaction<'_>,
    cloud_workspace_id: &str,
    entity: &IncrementalEntity,
) -> Result<(), String> {
    transaction
        .execute(
            "DELETE FROM cloud_deferred_entities WHERE workspace_id = ?1 AND entity_type = ?2 AND entity_id = ?3",
            params![cloud_workspace_id, entity.entity_type, entity.entity_id],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn deferred_entity_exists(
    transaction: &Transaction<'_>,
    cloud_workspace_id: &str,
    entity: &IncrementalEntity,
) -> Result<bool, String> {
    transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM cloud_deferred_entities
             WHERE workspace_id=?1 AND entity_type=?2 AND entity_id=?3)",
            params![cloud_workspace_id, entity.entity_type, entity.entity_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())
}

fn apply_incremental_entities(
    database: &Database,
    cloud_workspace_id: &str,
    device_id: &str,
    covered_change_seq: i64,
    entities: &[IncrementalEntity],
) -> Result<Vec<String>, String> {
    let local_workspace_id = local_cache_workspace_id(database)?;
    let mut connection = database.open()?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let mut domains = HashSet::<String>::new();
    let mut applied_task_parents = Vec::<(String, Option<String>)>::new();
    let mut resolved_unassigned_boundaries = Vec::<(String, i64)>::new();
    for entity in entities {
        validate_incremental_identity(entity)?;
        let domain = incremental_domain(&entity.entity_type)?;
        acknowledge_authoritative_unassigned_candidate(&transaction, cloud_workspace_id, entity)?;
        if entity_has_dirty_outbox(&transaction, cloud_workspace_id, entity)? {
            defer_incremental_entity(&transaction, cloud_workspace_id, entity)?;
            continue;
        }
        let was_deferred = deferred_entity_exists(&transaction, cloud_workspace_id, entity)?;
        if !was_deferred
            && !incremental_entity_needs_apply(&transaction, &local_workspace_id, entity)?
        {
            clear_deferred_entity(&transaction, cloud_workspace_id, entity)?;
            continue;
        }
        apply_incremental_entity(&transaction, &local_workspace_id, entity)?;
        if entity.entity_type == "unassigned_sessions" && !entity.deleted {
            let data = entity.data.as_ref().expect("validated unassigned payload");
            if matches!(text(data, "state").as_str(), "resolved" | "discarded") {
                if let Some(resolved_at) = optional_millis(data, "resolved_at") {
                    resolved_unassigned_boundaries.push((text(data, "id"), resolved_at));
                }
            }
        }
        if entity.entity_type == "tasks" && !entity.deleted {
            let data = entity.data.as_ref().expect("validated task payload");
            applied_task_parents.push((text(data, "id"), optional_text(data, "parent_id")));
        }
        clear_deferred_entity(&transaction, cloud_workspace_id, entity)?;
        domains.insert(domain.to_string());
    }
    for (task_id, parent_id) in applied_task_parents {
        transaction
            .execute(
                "UPDATE tasks SET parent_id = ?1 WHERE workspace_id = ?2 AND id = ?3",
                params![parent_id, local_workspace_id, task_id],
            )
            .map_err(|error| error.to_string())?;
    }
    for (predecessor_session_id, resolved_at) in resolved_unassigned_boundaries {
        ensure_next_shared_unassigned_session(
            &transaction,
            &local_workspace_id,
            &predecessor_session_id,
            resolved_at,
        )?;
    }
    if !domains.is_empty() {
        transaction
            .execute(
                "UPDATE app_metadata SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT) WHERE key = 'global_revision'",
                [],
            )
            .map_err(|error| error.to_string())?;
    }
    transaction
        .execute(
            "INSERT INTO local_sync_state(workspace_id, device_id, last_change_seq, last_full_sync_at, last_error)
             VALUES (?1, ?2, ?3, ?4, NULL)
             ON CONFLICT(workspace_id) DO UPDATE SET device_id = excluded.device_id,
               last_change_seq = MAX(local_sync_state.last_change_seq, excluded.last_change_seq),
               last_full_sync_at = excluded.last_full_sync_at, last_error = NULL",
            params![cloud_workspace_id, device_id, covered_change_seq, now_millis()],
        )
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())?;
    for entity in entities {
        if entity.entity_type != "unassigned_sessions" || entity.deleted {
            continue;
        }
        let Some(data) = entity.data.as_ref() else {
            continue;
        };
        if matches!(text(data, "state").as_str(), "resolved" | "discarded") {
            queue_next_shared_unassigned_candidate(database, data)?;
        }
    }
    let mut domains = domains.into_iter().collect::<Vec<_>>();
    domains.sort();
    Ok(domains)
}

fn queue_next_shared_unassigned_candidate(
    database: &Database,
    resolved: &Value,
) -> Result<(), String> {
    let state = storage_mode(database)?;
    if state.mode != "cloud" {
        return Ok(());
    }
    let predecessor_session_id = text(resolved, "id");
    let connection = database.open()?;
    let candidate: Option<String> = connection
        .query_row(
            "SELECT id FROM unassigned_sessions
             WHERE predecessor_session_id=?1
               AND state IN ('collecting','awaiting_resolution')
               AND shared_source='cloud' AND migration_state='candidate'
             ORDER BY created_at LIMIT 1",
            [&predecessor_session_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some(candidate) = candidate else {
        return Ok(());
    };
    let already_queued: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sync_outbox
             WHERE workspace_id=?1 AND entity_type='unassigned_session' AND entity_id=?2
               AND state IN ('pending','sending','failed','conflict'))",
            params![state.workspace_id, candidate],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    drop(connection);
    if !already_queued {
        enqueue_entity_deferred(
            database,
            "unassigned_session_create",
            "unassigned_session",
            Some(&candidate),
            None,
            None,
        )?;
        wake_after_confirmed_cloud_enqueue(database);
    }
    Ok(())
}

fn wake_after_confirmed_cloud_enqueue(database: &Database) {
    signal_cloud_sync(SYNC_SIGNAL_LOCAL_DIRTY, None);
    if let Some(app) = CLOUD_SYNC_APP_HANDLE.get() {
        if let Ok(status) = sync_status(database) {
            let _ = app.emit("cloud-sync-state-changed", &status);
        }
    }
}

fn ensure_next_shared_unassigned_session(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    predecessor_session_id: &str,
    resolved_at: i64,
) -> Result<(), String> {
    let active_timer: bool = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM time_entries
             WHERE workspace_id=?1 AND state IN ('running','paused') AND deleted_at IS NULL)",
            [workspace_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if active_timer {
        return Ok(());
    }
    let active_session: Option<(String, Option<String>)> = transaction
        .query_row(
            "SELECT id,predecessor_session_id FROM unassigned_sessions
             WHERE workspace_id=?1 AND state IN ('collecting','awaiting_resolution')
             ORDER BY created_at LIMIT 1",
            [workspace_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if let Some((active_session_id, predecessor)) = active_session {
        if predecessor.as_deref() == Some(predecessor_session_id) {
            let work_date = crate::work_calendar::work_date_at(
                crate::work_calendar::workspace_timezone(transaction, workspace_id)?,
                resolved_at,
            )?;
            transaction
                .execute(
                    "UPDATE unassigned_sessions
                     SET first_started_at=?1,work_date=?2,duration_seconds=0,
                         last_ended_at=NULL,prompted_at=NULL,updated_at=?1
                     WHERE id=?3",
                    params![resolved_at, work_date, active_session_id],
                )
                .map_err(|error| error.to_string())?;
            transaction
                .execute(
                    "UPDATE unassigned_segments
                     SET started_at=?1,ended_at=NULL,duration_seconds=0
                     WHERE session_id=?2 AND ended_at IS NULL",
                    params![resolved_at, active_session_id],
                )
                .map_err(|error| error.to_string())?;
        }
        return Ok(());
    }
    let threshold: i64 = transaction
        .query_row(
            "SELECT value_json FROM app_settings
             WHERE workspace_id=?1 AND key='unassigned_prompt_seconds'",
            [workspace_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .and_then(|value| serde_json::from_str(&value).ok())
        .unwrap_or(300);
    let session_id = Uuid::now_v7().to_string();
    let work_date = crate::work_calendar::work_date_at(
        crate::work_calendar::workspace_timezone(transaction, workspace_id)?,
        resolved_at,
    )?;
    let inserted = transaction
        .execute(
            "INSERT OR IGNORE INTO unassigned_sessions(
               id,workspace_id,work_date,state,threshold_seconds,duration_seconds,
               first_started_at,created_at,updated_at,version,shared_source,
               predecessor_session_id,migration_state
             ) VALUES (?1,?2,?3,'collecting',?4,0,?5,?5,?5,1,'cloud',?6,'candidate')",
            params![
                session_id,
                workspace_id,
                work_date,
                threshold,
                resolved_at,
                predecessor_session_id
            ],
        )
        .map_err(|error| error.to_string())?;
    if inserted > 0 {
        transaction
            .execute(
                "INSERT INTO unassigned_segments(
                   id,workspace_id,session_id,sequence_no,started_at,duration_seconds
                 ) VALUES (?1,?2,?3,1,?4,0)",
                params![
                    Uuid::now_v7().to_string(),
                    workspace_id,
                    session_id,
                    resolved_at
                ],
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn incremental_entity_needs_apply(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    entity: &IncrementalEntity,
) -> Result<bool, String> {
    let exists = |sql: &str, values: &[&dyn rusqlite::ToSql]| -> Result<bool, String> {
        transaction
            .query_row(sql, values, |row| row.get(0))
            .map_err(|error| error.to_string())
    };
    if entity.deleted {
        return match entity.entity_type.as_str() {
            "task_daily_estimates" | "task_day_plans" | "task_occurrences" => {
                let (left, right) = entity.entity_id.split_once('|').expect("validated composite key");
                let table = entity.entity_type.as_str();
                let date_column = if table == "task_occurrences" { "occurrence_date" } else { "work_date" };
                exists(&format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE workspace_id=?1 AND task_id=?2 AND {date_column}=?3)"), &[&workspace_id, &left, &right])
            }
            "work_days" | "app_settings" | "integration_configs" => {
                let (table, key) = match entity.entity_type.as_str() { "work_days" => ("work_days", "work_date"), "app_settings" => ("app_settings", "key"), _ => ("integration_configs", "provider") };
                exists(&format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE workspace_id=?1 AND {key}=?2)"), &[&workspace_id, &entity.entity_id])
            }
            "task_recurrence_rules" => exists("SELECT EXISTS(SELECT 1 FROM task_recurrence_rules WHERE workspace_id=?1 AND task_id=?2)", &[&workspace_id, &entity.entity_id]),
            "external_bindings" => {
                let parts = entity.entity_id.split('|').collect::<Vec<_>>();
                exists("SELECT EXISTS(SELECT 1 FROM external_bindings WHERE workspace_id=?1 AND provider=?2 AND entity_type=?3 AND entity_id=?4)", &[&workspace_id, &parts[0], &parts[1], &parts[2]])
            }
            other => {
                let table = other;
                exists(&format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE workspace_id=?1 AND id=?2)"), &[&workspace_id, &entity.entity_id])
            }
        };
    }
    let data = entity.data.as_ref().expect("validated incremental data");
    let incoming_version = data.get("version").and_then(Value::as_i64);
    let current_version: Option<i64> = match entity.entity_type.as_str() {
        "work_days" => transaction.query_row("SELECT version FROM work_days WHERE workspace_id=?1 AND work_date=?2", params![workspace_id,entity.entity_id], |row| row.get(0)).optional(),
        "app_settings" => transaction.query_row("SELECT version FROM app_settings WHERE workspace_id=?1 AND key=?2", params![workspace_id,entity.entity_id], |row| row.get(0)).optional(),
        "integration_configs" => transaction.query_row("SELECT version FROM integration_configs WHERE workspace_id=?1 AND provider=?2", params![workspace_id,entity.entity_id], |row| row.get(0)).optional(),
        "task_daily_estimates" | "task_day_plans" | "task_occurrences" => {
            let (left,right)=entity.entity_id.split_once('|').expect("validated composite key");
            match entity.entity_type.as_str() {
                "task_daily_estimates" => transaction.query_row("SELECT version FROM task_daily_estimates WHERE workspace_id=?1 AND task_id=?2 AND work_date=?3",params![workspace_id,left,right],|row|row.get(0)).optional(),
                "task_day_plans" => transaction.query_row("SELECT version FROM task_day_plans WHERE workspace_id=?1 AND task_id=?2 AND work_date=?3",params![workspace_id,left,right],|row|row.get(0)).optional(),
                _ => transaction.query_row("SELECT version FROM task_occurrences WHERE workspace_id=?1 AND task_id=?2 AND occurrence_date=?3",params![workspace_id,left,right],|row|row.get(0)).optional(),
            }
        }
        "task_recurrence_rules" => {
            return recurrence_rules_differ(
                transaction,
                workspace_id,
                &entity.entity_id,
                data,
            );
        }
        "task_status_events" => return exists("SELECT NOT EXISTS(SELECT 1 FROM task_status_events WHERE workspace_id=?1 AND id=?2)", &[&workspace_id,&entity.entity_id]),
        "external_bindings" => {
            return external_binding_differs(transaction, workspace_id, data);
        }
        "unassigned_sessions" => return Ok(true),
        "subject_work_schedules" => transaction.query_row("SELECT version FROM subject_work_schedules WHERE workspace_id=?1 AND id=?2",params![workspace_id,entity.entity_id],|row|row.get(0)).optional(),
        other => transaction.query_row(&format!("SELECT version FROM {other} WHERE workspace_id=?1 AND id=?2"),params![workspace_id,entity.entity_id],|row|row.get(0)).optional(),
    }.map_err(|error| error.to_string())?;
    Ok(incoming_version
        .is_none_or(|incoming| current_version.is_none_or(|current| current < incoming)))
}

fn recurrence_rules_differ(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    task_id: &str,
    data: &Value,
) -> Result<bool, String> {
    let incoming_rules = data
        .get("rules")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let local_count = transaction
        .query_row(
            "SELECT COUNT(*) FROM task_recurrence_rules WHERE workspace_id=?1 AND task_id=?2",
            params![workspace_id, task_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| error.to_string())?;
    if local_count != incoming_rules.len() as i64 {
        return Ok(true);
    }
    for rule in incoming_rules {
        let differs = transaction
            .query_row(
                "SELECT frequency <> ?3
                     OR COALESCE(weekdays_mask, -1) <> COALESCE(?4, -1)
                     OR effective_start <> ?5
                     OR COALESCE(effective_end, '') <> COALESCE(?6, '')
                     OR created_at <> ?7
                     OR updated_at <> ?8
                     OR version <> ?9
                 FROM task_recurrence_rules
                 WHERE workspace_id=?1 AND task_id=?2 AND id=?10",
                params![
                    workspace_id,
                    task_id,
                    text(rule, "frequency"),
                    optional_integer(rule, "weekdays_mask"),
                    text(rule, "effective_start"),
                    optional_text(rule, "effective_end"),
                    millis(rule, "created_at"),
                    millis(rule, "updated_at"),
                    integer(rule, "version"),
                    text(rule, "id"),
                ],
                |row| row.get::<_, bool>(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        if differs != Some(false) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn external_binding_differs(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    data: &Value,
) -> Result<bool, String> {
    let differs = transaction
        .query_row(
            "SELECT external_id <> ?5
                 OR COALESCE(external_revision, '') <> COALESCE(?6, '')
                 OR COALESCE(last_synced_hash, '') <> COALESCE(?7, '')
                 OR COALESCE(last_synced_at, -1) <> COALESCE(?8, -1)
             FROM external_bindings
             WHERE workspace_id=?1 AND provider=?2 AND entity_type=?3 AND entity_id=?4",
            params![
                workspace_id,
                text(data, "provider"),
                text(data, "entity_type"),
                text(data, "entity_id"),
                text(data, "external_id"),
                optional_text(data, "external_revision"),
                optional_text(data, "last_synced_hash"),
                optional_millis(data, "last_synced_at"),
            ],
            |row| row.get::<_, bool>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    Ok(differs != Some(false))
}

fn apply_incremental_entity(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    entity: &IncrementalEntity,
) -> Result<(), String> {
    if entity.deleted {
        return delete_incremental_entity(transaction, workspace_id, entity);
    }
    let data = entity
        .data
        .as_ref()
        .ok_or_else(|| "VALIDATION_ERROR: 非删除增量缺少实体内容".to_string())?;
    match entity.entity_type.as_str() {
        "subjects" => transaction.execute("INSERT INTO subjects(id,workspace_id,name,sort_order,created_at,updated_at,version,created_by_device_id,updated_by_device_id,deleted_at) VALUES (?1,?2,?3,?4,?5,?6,?7,NULL,NULL,?8) ON CONFLICT(id) DO UPDATE SET name=excluded.name,sort_order=excluded.sort_order,updated_at=excluded.updated_at,version=excluded.version,deleted_at=excluded.deleted_at", params![text(data,"id"),workspace_id,text(data,"name"),integer(data,"sort_order"),millis(data,"created_at"),millis(data,"updated_at"),integer(data,"version"),optional_millis(data,"deleted_at")]),
        "subject_work_schedules" => {
            let schedule_id = text(data, "id");
            transaction.execute("INSERT INTO subject_work_schedules(id,workspace_id,subject_id,weekday,is_workday,created_at,updated_at,version) VALUES (?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(id) DO UPDATE SET subject_id=excluded.subject_id,weekday=excluded.weekday,is_workday=excluded.is_workday,updated_at=excluded.updated_at,version=excluded.version", params![schedule_id,workspace_id,text(data,"subject_id"),integer(data,"weekday"),boolean(data,"is_workday") as i64,millis(data,"created_at"),millis(data,"updated_at"),integer(data,"version")]).map_err(|error| error.to_string())?;
            transaction.execute("DELETE FROM subject_work_schedule_segments WHERE schedule_id=?1", [&schedule_id]).map_err(|error| error.to_string())?;
            for (index, segment) in data.get("segments").and_then(Value::as_array).into_iter().flatten().enumerate() {
                transaction.execute("INSERT INTO subject_work_schedule_segments(id,workspace_id,schedule_id,sequence_no,start_minute,end_minute) VALUES (?1,?2,?3,?4,?5,?6)", params![text(segment,"id"),workspace_id,schedule_id,index as i64 + 1,integer(segment,"start_minute"),integer(segment,"end_minute")]).map_err(|error| error.to_string())?;
            }
            Ok(1)
        },
        "work_days" => transaction.execute("INSERT INTO work_days(workspace_id,work_date,timezone,work_period_text,note,settled_at,created_at,updated_at,version) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(workspace_id,work_date) DO UPDATE SET timezone=excluded.timezone,work_period_text=excluded.work_period_text,note=excluded.note,settled_at=excluded.settled_at,updated_at=excluded.updated_at,version=excluded.version", params![workspace_id,text(data,"work_date"),text(data,"timezone"),optional_text(data,"work_period_text"),optional_text(data,"note"),optional_millis(data,"settled_at"),millis(data,"created_at"),millis(data,"updated_at"),integer(data,"version")]),
        "app_settings" => transaction.execute("INSERT INTO app_settings(workspace_id,key,value_json,updated_at,version) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(workspace_id,key) DO UPDATE SET value_json=excluded.value_json,updated_at=excluded.updated_at,version=excluded.version", params![workspace_id,text(data,"key"),data.get("value_json").cloned().unwrap_or(Value::Null).to_string(),millis(data,"updated_at"),integer(data,"version")]),
        "tasks" => {
            let changed = transaction.execute("INSERT INTO tasks(id,workspace_id,subject_id,parent_id,title,status,planned_date,planned_time,estimate_minutes,note,project_name,solution_name,source_type,source_ref,sort_order,completed_at,created_at,updated_at,version,deleted_at,day_instance_date,day_instance_kind,day_instance_source_task_id) VALUES (?1,?2,?3,NULL,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22) ON CONFLICT(id) DO UPDATE SET subject_id=excluded.subject_id,parent_id=NULL,title=excluded.title,status=excluded.status,planned_date=excluded.planned_date,planned_time=excluded.planned_time,estimate_minutes=excluded.estimate_minutes,note=excluded.note,project_name=excluded.project_name,solution_name=excluded.solution_name,source_type=excluded.source_type,source_ref=excluded.source_ref,sort_order=excluded.sort_order,completed_at=excluded.completed_at,updated_at=excluded.updated_at,version=excluded.version,deleted_at=excluded.deleted_at,day_instance_date=excluded.day_instance_date,day_instance_kind=excluded.day_instance_kind,day_instance_source_task_id=excluded.day_instance_source_task_id", params![text(data,"id"),workspace_id,text(data,"subject_id"),text(data,"title"),text(data,"status"),optional_text(data,"planned_date"),optional_time(data,"planned_time"),optional_integer(data,"estimate_minutes"),optional_text(data,"note"),optional_text(data,"project_name"),optional_text(data,"solution_name"),text(data,"source_type"),optional_text(data,"source_ref"),integer(data,"sort_order"),optional_millis(data,"completed_at"),millis(data,"created_at"),millis(data,"updated_at"),integer(data,"version"),optional_millis(data,"deleted_at"),optional_text(data,"day_instance_date"),optional_text(data,"day_instance_kind"),optional_text(data,"day_instance_source_task_id")]).map_err(|error| error.to_string())?;
            Ok(changed)
        }
        "task_status_events" => transaction.execute("INSERT INTO task_status_events(id,workspace_id,task_id,status,occurred_at,source_type,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(id) DO UPDATE SET status=excluded.status,occurred_at=excluded.occurred_at,source_type=excluded.source_type", params![text(data,"id"),workspace_id,text(data,"task_id"),text(data,"status"),millis(data,"occurred_at"),text(data,"source_type"),millis(data,"created_at")]),
        "task_daily_estimates" => transaction.execute("INSERT INTO task_daily_estimates(workspace_id,task_id,work_date,estimate_minutes,created_at,updated_at,version) VALUES (?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(task_id,work_date) DO UPDATE SET estimate_minutes=excluded.estimate_minutes,updated_at=excluded.updated_at,version=excluded.version", params![workspace_id,text(data,"task_id"),text(data,"work_date"),integer(data,"estimate_minutes"),millis(data,"created_at"),millis(data,"updated_at"),integer(data,"version")]),
        "task_day_plans" => transaction.execute("INSERT INTO task_day_plans(workspace_id,task_id,work_date,state,instance_task_id,created_at,updated_at,version) VALUES (?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(task_id,work_date) DO UPDATE SET state=excluded.state,instance_task_id=excluded.instance_task_id,updated_at=excluded.updated_at,version=excluded.version", params![workspace_id,text(data,"task_id"),text(data,"work_date"),text(data,"state"),optional_text(data,"instance_task_id"),millis(data,"created_at"),millis(data,"updated_at"),integer(data,"version")]),
        "task_recurrence_rules" => {
            transaction.execute("DELETE FROM task_recurrence_rules WHERE workspace_id=?1 AND task_id=?2", params![workspace_id,entity.entity_id]).map_err(|error| error.to_string())?;
            for rule in data.get("rules").and_then(Value::as_array).cloned().unwrap_or_default() {
                transaction.execute("INSERT INTO task_recurrence_rules(id,workspace_id,task_id,frequency,weekdays_mask,effective_start,effective_end,created_at,updated_at,version) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)", params![text(&rule,"id"),workspace_id,text(&rule,"task_id"),text(&rule,"frequency"),optional_integer(&rule,"weekdays_mask"),text(&rule,"effective_start"),optional_text(&rule,"effective_end"),millis(&rule,"created_at"),millis(&rule,"updated_at"),integer(&rule,"version")]).map_err(|error| error.to_string())?;
            }
            Ok(0)
        }
        "task_occurrences" => transaction.execute("INSERT INTO task_occurrences(workspace_id,task_id,occurrence_date,origin,status,completed_at,created_at,updated_at,version) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(task_id,occurrence_date) DO UPDATE SET origin=excluded.origin,status=excluded.status,completed_at=excluded.completed_at,updated_at=excluded.updated_at,version=excluded.version", params![workspace_id,text(data,"task_id"),text(data,"occurrence_date"),text(data,"origin"),text(data,"status"),optional_millis(data,"completed_at"),millis(data,"created_at"),millis(data,"updated_at"),integer(data,"version")]),
        "unassigned_sessions" => {
            apply_incremental_unassigned_session(transaction, workspace_id, data)
        }
        "time_entries" => apply_incremental_time_entry(transaction, workspace_id, data),
        "actual_time_adjustments" => transaction.execute(
            "INSERT INTO actual_time_adjustments(id,workspace_id,subject_id,task_id,work_date,delta_minutes,baseline_minutes,target_minutes,note,source_type,batch_id,created_at,updated_at,version,created_by_device_id,updated_by_device_id,deleted_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,(SELECT id FROM devices WHERE id=?15 AND workspace_id=?2),(SELECT id FROM devices WHERE id=?16 AND workspace_id=?2),?17)
             ON CONFLICT(id) DO UPDATE SET subject_id=excluded.subject_id,task_id=excluded.task_id,work_date=excluded.work_date,delta_minutes=excluded.delta_minutes,baseline_minutes=excluded.baseline_minutes,target_minutes=excluded.target_minutes,note=excluded.note,source_type=excluded.source_type,batch_id=excluded.batch_id,updated_at=excluded.updated_at,version=excluded.version,updated_by_device_id=excluded.updated_by_device_id,deleted_at=excluded.deleted_at",
            params![text(data,"id"),workspace_id,text(data,"subject_id"),text(data,"task_id"),text(data,"work_date"),integer(data,"delta_minutes"),integer(data,"baseline_minutes"),integer(data,"target_minutes"),optional_text(data,"note"),optional_text(data,"source_type").unwrap_or_else(|| "quick_actual_adjustment".to_string()),text(data,"batch_id"),millis(data,"created_at"),millis(data,"updated_at"),integer(data,"version"),optional_text(data,"created_by_device_id"),optional_text(data,"updated_by_device_id"),optional_millis(data,"deleted_at")],
        ),
        "report_templates" => {
            if boolean(data, "is_default") && optional_millis(data, "deleted_at").is_none() {
                transaction.execute(
                    "UPDATE report_templates SET is_default=0 WHERE workspace_id=?1 AND report_type=?2 AND id<>?3 AND deleted_at IS NULL",
                    params![workspace_id, text(data, "report_type"), text(data, "id")],
                ).map_err(|error| error.to_string())?;
            }
            transaction.execute("INSERT INTO report_templates(id,workspace_id,report_type,subject_id,name,description,content,is_default,is_builtin,sort_order,created_at,updated_at,version,deleted_at) VALUES (?1,?2,?3,NULL,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13) ON CONFLICT(id) DO UPDATE SET report_type=excluded.report_type,subject_id=NULL,name=excluded.name,description=excluded.description,content=excluded.content,is_default=excluded.is_default,is_builtin=excluded.is_builtin,sort_order=excluded.sort_order,updated_at=excluded.updated_at,version=excluded.version,deleted_at=excluded.deleted_at", params![text(data,"id"),workspace_id,text(data,"report_type"),data.get("name").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| format!("迁移模板 - {}", text(data,"id").chars().take(8).collect::<String>())),optional_text(data,"description"),text(data,"content"),boolean(data,"is_default") as i64,boolean(data,"is_builtin") as i64,integer(data,"sort_order"),millis(data,"created_at"),millis(data,"updated_at"),integer(data,"version"),optional_millis(data,"deleted_at")])
        },
        "reports" => apply_incremental_report(transaction, workspace_id, data),
        "integration_configs" => transaction.execute("INSERT INTO integration_configs(workspace_id,provider,enabled,config_json,secret_ref,updated_at,version) VALUES (?1,?2,?3,?4,COALESCE((SELECT secret_ref FROM integration_configs WHERE workspace_id=?1 AND provider=?2),NULL),?5,?6) ON CONFLICT(workspace_id,provider) DO UPDATE SET enabled=excluded.enabled,config_json=excluded.config_json,updated_at=excluded.updated_at,version=excluded.version", params![workspace_id,text(data,"provider"),boolean(data,"enabled") as i64,data.get("config_json").cloned().unwrap_or_else(|| json!({})).to_string(),millis(data,"updated_at"),integer(data,"version")]),
        "external_bindings" => transaction.execute("INSERT INTO external_bindings(id,workspace_id,provider,entity_type,entity_id,external_id,external_revision,last_synced_hash,last_synced_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(workspace_id,provider,entity_type,entity_id) DO UPDATE SET external_id=excluded.external_id,external_revision=excluded.external_revision,last_synced_hash=excluded.last_synced_hash,last_synced_at=excluded.last_synced_at", params![text(data,"id"),workspace_id,text(data,"provider"),text(data,"entity_type"),text(data,"entity_id"),text(data,"external_id"),optional_text(data,"external_revision"),optional_text(data,"last_synced_hash"),optional_millis(data,"last_synced_at")]),
        other => return Err(format!("VALIDATION_ERROR: 不支持的云端增量实体 {other}")),
    }
    .map(|_| ())
    .map_err(|error| error.to_string())
}

fn validate_incremental_identity(entity: &IncrementalEntity) -> Result<(), String> {
    let expected_type =
        expected_incremental_entity_type(&entity.source_entity_type).ok_or_else(|| {
            format!(
                "VALIDATION_ERROR: 不支持的云端增量来源 {}",
                entity.source_entity_type
            )
        })?;
    if expected_type != entity.entity_type {
        return Err(format!(
            "VALIDATION_ERROR: 云端增量聚合类型不匹配 {} -> {}",
            entity.source_entity_type, entity.entity_type
        ));
    }
    validate_incremental_entity_key(&entity.entity_type, &entity.entity_id)?;
    if entity.deleted {
        if entity.data.is_some() {
            return Err("VALIDATION_ERROR: 删除增量不得携带实体内容".to_string());
        }
        return Ok(());
    }
    let data = entity.data.as_ref();
    let matches = match entity.entity_type.as_str() {
        "subjects"
        | "subject_work_schedules"
        | "tasks"
        | "task_status_events"
        | "time_entries"
        | "report_templates"
        | "reports"
        | "actual_time_adjustments" => {
            data.and_then(|row| row.get("id")).and_then(Value::as_str)
                == Some(entity.entity_id.as_str())
        }
        "work_days" => {
            data.and_then(|row| row.get("work_date"))
                .and_then(Value::as_str)
                == Some(entity.entity_id.as_str())
        }
        "app_settings" => {
            data.and_then(|row| row.get("key")).and_then(Value::as_str)
                == Some(entity.entity_id.as_str())
        }
        "integration_configs" => {
            data.and_then(|row| row.get("provider"))
                .and_then(Value::as_str)
                == Some(entity.entity_id.as_str())
        }
        "unassigned_sessions" => {
            validate_shared_unassigned_payload(&entity.entity_id, data.expect("validated data"))?;
            true
        }
        "task_recurrence_rules" => {
            data.and_then(|row| row.get("task_id"))
                .and_then(Value::as_str)
                == Some(entity.entity_id.as_str())
        }
        "task_daily_estimates" | "task_day_plans" => data.is_some_and(|row| {
            format!("{}|{}", text(row, "task_id"), text(row, "work_date")) == entity.entity_id
        }),
        "task_occurrences" => data.is_some_and(|row| {
            format!("{}|{}", text(row, "task_id"), text(row, "occurrence_date")) == entity.entity_id
        }),
        "external_bindings" => data.is_some_and(|row| {
            format!(
                "{}|{}|{}",
                text(row, "provider"),
                text(row, "entity_type"),
                text(row, "entity_id")
            ) == entity.entity_id
        }),
        _ => false,
    };
    if matches {
        Ok(())
    } else {
        Err(format!(
            "VALIDATION_ERROR: 云端增量实体身份不匹配 {}:{}",
            entity.entity_type, entity.entity_id
        ))
    }
}

fn validate_incremental_entity_key(entity_type: &str, entity_id: &str) -> Result<(), String> {
    let parts = entity_id.split('|').collect::<Vec<_>>();
    let valid_date = |value: &str| chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok();
    let valid = match entity_type {
        "task_daily_estimates" | "task_day_plans" | "task_occurrences" => {
            parts.len() == 2 && Uuid::parse_str(parts[0]).is_ok() && valid_date(parts[1])
        }
        "external_bindings" => {
            parts.len() == 3
                && !parts[0].is_empty()
                && !parts[1].is_empty()
                && Uuid::parse_str(parts[2]).is_ok()
        }
        "work_days" => parts.len() == 1 && valid_date(entity_id),
        "app_settings" | "integration_configs" => parts.len() == 1 && !entity_id.trim().is_empty(),
        "subjects"
        | "subject_work_schedules"
        | "tasks"
        | "task_status_events"
        | "task_recurrence_rules"
        | "unassigned_sessions"
        | "time_entries"
        | "report_templates"
        | "reports"
        | "actual_time_adjustments" => parts.len() == 1 && Uuid::parse_str(entity_id).is_ok(),
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(format!(
            "VALIDATION_ERROR: 云端增量实体键无效 {entity_type}:{entity_id}"
        ))
    }
}

fn apply_incremental_time_entry(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    data: &Value,
) -> rusqlite::Result<usize> {
    let entry_id = text(data, "id");
    let source_type = text(data, "source_type");
    let timer_chain_id = optional_text(data, "timer_chain_id")
        .or_else(|| (source_type == "timer").then(|| entry_id.clone()));
    let last_continuous_at = optional_millis(data, "last_continuous_at").or_else(|| {
        optional_millis(data, "ended_at")
            .or_else(|| optional_millis(data, "updated_at"))
            .or_else(|| optional_millis(data, "started_at"))
    });
    let origin_unassigned_session_id = optional_text(data, "origin_unassigned_session_id");
    if let Some(session_id) = origin_unassigned_session_id.as_deref() {
        // A losing device may have optimistically generated a different final
        // entry for this session. Incremental entities can arrive before the
        // terminal session row, so remove the local loser before inserting the
        // authoritative entry and satisfying the one-result constraint.
        let losing_entry_ids = transaction
            .prepare(
                "SELECT id FROM time_entries
                 WHERE workspace_id=?1 AND origin_unassigned_session_id=?2 AND id<>?3",
            )?
            .query_map(params![workspace_id, session_id, entry_id], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for losing_entry_id in losing_entry_ids {
            transaction.execute(
                "UPDATE unassigned_sessions SET generated_entry_id=NULL
                 WHERE workspace_id=?1 AND generated_entry_id=?2",
                params![workspace_id, losing_entry_id],
            )?;
            transaction.execute(
                "DELETE FROM time_allocations WHERE workspace_id=?1 AND entry_id=?2",
                params![workspace_id, losing_entry_id],
            )?;
            transaction.execute(
                "DELETE FROM time_segments WHERE workspace_id=?1 AND entry_id=?2",
                params![workspace_id, losing_entry_id],
            )?;
            transaction.execute(
                "DELETE FROM time_entries WHERE workspace_id=?1 AND id=?2",
                params![workspace_id, losing_entry_id],
            )?;
        }
    }
    transaction.execute(
        "DELETE FROM time_segments WHERE workspace_id=?1 AND entry_id=?2",
        params![workspace_id, entry_id],
    )?;
    transaction.execute(
        "DELETE FROM time_allocations WHERE workspace_id=?1 AND entry_id=?2",
        params![workspace_id, entry_id],
    )?;
    let changed = transaction.execute("INSERT INTO time_entries(id,workspace_id,work_date,kind,source_type,state,default_task_id,label_snapshot,started_at,ended_at,duration_seconds,note,origin_unassigned_session_id,created_at,updated_at,version,deleted_at,timer_chain_id,previous_entry_id,split_boundary_at,last_continuous_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21) ON CONFLICT(id) DO UPDATE SET work_date=excluded.work_date,kind=excluded.kind,source_type=excluded.source_type,state=excluded.state,default_task_id=excluded.default_task_id,label_snapshot=excluded.label_snapshot,started_at=excluded.started_at,ended_at=excluded.ended_at,duration_seconds=excluded.duration_seconds,note=excluded.note,origin_unassigned_session_id=excluded.origin_unassigned_session_id,updated_at=excluded.updated_at,version=excluded.version,deleted_at=excluded.deleted_at,timer_chain_id=excluded.timer_chain_id,previous_entry_id=excluded.previous_entry_id,split_boundary_at=excluded.split_boundary_at,last_continuous_at=excluded.last_continuous_at", params![entry_id,workspace_id,text(data,"work_date"),text(data,"kind"),source_type,text(data,"state"),optional_text(data,"default_task_id"),text(data,"label_snapshot"),millis(data,"started_at"),optional_millis(data,"ended_at"),integer(data,"duration_seconds"),optional_text(data,"note"),origin_unassigned_session_id,millis(data,"created_at"),millis(data,"updated_at"),integer(data,"version"),optional_millis(data,"deleted_at"),timer_chain_id,optional_text(data,"previous_entry_id"),optional_millis(data,"split_boundary_at"),last_continuous_at])?;
    for row in data
        .get("segments")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        transaction.execute("INSERT INTO time_segments(id,workspace_id,entry_id,sequence_no,started_at,ended_at,duration_seconds) VALUES (?1,?2,?3,?4,?5,?6,?7)", params![text(&row,"id"),workspace_id,entry_id,integer(&row,"sequence_no"),millis(&row,"started_at"),optional_millis(&row,"ended_at"),integer(&row,"duration_seconds")])?;
    }
    for row in data
        .get("allocations")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        transaction.execute("INSERT INTO time_allocations(id,workspace_id,entry_id,task_id,minutes,note,created_at,updated_at,version) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)", params![text(&row,"id"),workspace_id,entry_id,text(&row,"task_id"),integer(&row,"minutes"),optional_text(&row,"note"),millis(&row,"created_at"),millis(&row,"updated_at"),integer(&row,"version")])?;
    }
    if let Some(session_id) = origin_unassigned_session_id {
        transaction.execute(
            "UPDATE unassigned_sessions
             SET generated_entry_id=?1
             WHERE workspace_id=?2 AND id=?3
               AND state IN ('resolved','discarded') AND generated_entry_id IS NULL",
            params![entry_id, workspace_id, session_id],
        )?;
    }
    Ok(changed)
}

fn apply_incremental_unassigned_session(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    data: &Value,
) -> Result<usize, rusqlite::Error> {
    let session_id = text(data, "id");
    let incoming_state = text(data, "state");
    let incoming_segments = data
        .get("segments")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let incoming_active = incoming_state == "collecting"
        || (incoming_state == "awaiting_resolution"
            && incoming_segments
                .iter()
                .any(|segment| optional_millis(segment, "ended_at").is_none()));
    let incoming_unresolved = matches!(
        incoming_state.as_str(),
        "collecting" | "awaiting_resolution"
    );
    let state = if incoming_active {
        "collecting".to_string()
    } else {
        incoming_state.clone()
    };
    let superseded_at =
        optional_millis(data, "resolved_at").unwrap_or_else(|| millis(data, "first_started_at"));
    let incoming_predecessor = optional_text(data, "predecessor_session_id");
    if incoming_active {
        if let Some(predecessor_session_id) = incoming_predecessor.as_deref() {
            let competing_session_ids = {
                let mut statement = transaction.prepare(
                    "SELECT id FROM unassigned_sessions
                     WHERE workspace_id=?1 AND id<>?2 AND predecessor_session_id=?3",
                )?;
                let rows = statement
                    .query_map(
                        params![workspace_id, session_id, predecessor_session_id],
                        |row| row.get(0),
                    )?
                    .collect::<Result<Vec<String>, _>>()?;
                rows
            };
            for competing_session_id in competing_session_ids {
                transaction.execute(
                    "DELETE FROM sync_outbox WHERE entity_type='unassigned_session' AND entity_id=?1",
                    [competing_session_id],
                )?;
            }
            transaction.execute(
                "DELETE FROM sync_outbox
                 WHERE workspace_id=?1 AND entity_type='unassigned_session'
                   AND entity_id IN (SELECT id FROM unassigned_sessions
                     WHERE workspace_id=?1 AND id<>?2 AND predecessor_session_id=?3)",
                params![workspace_id, session_id, predecessor_session_id],
            )?;
            transaction.execute(
                "UPDATE unassigned_sessions SET predecessor_session_id=NULL
                 WHERE workspace_id=?1 AND id<>?2 AND predecessor_session_id=?3",
                params![workspace_id, session_id, predecessor_session_id],
            )?;
        }
    }
    let incoming_work_date = text(data, "work_date");
    let incoming_started_at = millis(data, "first_started_at");
    let newer_local_session: Option<String> = if incoming_unresolved {
        transaction
            .query_row(
                "SELECT id FROM unassigned_sessions
                 WHERE workspace_id=?1 AND id<>?2 AND state IN ('collecting','awaiting_resolution')
                   AND (?5=0 OR (
                     shared_source='cloud' AND migration_state='adopted'
                     AND (work_date>?3 OR (work_date=?3 AND first_started_at>?4))
                   ))
                 ORDER BY work_date DESC,first_started_at DESC,id DESC LIMIT 1",
                params![
                    workspace_id,
                    session_id,
                    incoming_work_date,
                    incoming_started_at,
                    incoming_active as i64
                ],
                |row| row.get(0),
            )
            .optional()?
    } else {
        None
    };
    let incoming_superseded = newer_local_session.is_some();
    if incoming_active && !incoming_superseded {
        transaction.execute(
            "DELETE FROM sync_outbox
             WHERE workspace_id=?1 AND entity_type='unassigned_session' AND entity_id<>?2
               AND state IN ('pending','failed','conflict')",
            params![workspace_id, session_id],
        )?;
        transaction.execute(
            "UPDATE unassigned_sessions
             SET predecessor_session_id=NULL
             WHERE workspace_id=?1 AND id<>?2 AND state IN ('collecting','awaiting_resolution')",
            params![workspace_id, session_id],
        )?;
    } else if incoming_active {
        transaction.execute(
            "UPDATE unassigned_sessions SET predecessor_session_id=NULL
             WHERE workspace_id=?1 AND id=?2",
            params![workspace_id, session_id],
        )?;
    }
    let generated_entry_id = optional_text(data, "generated_entry_id").filter(|entry_id| {
        transaction
            .query_row(
                "SELECT 1 FROM time_entries WHERE workspace_id=?1 AND id=?2",
                params![workspace_id, entry_id],
                |_| Ok(()),
            )
            .optional()
            .ok()
            .flatten()
            .is_some()
    });
    let last_continuous_at = optional_millis(data, "last_continuous_at")
        .or_else(|| optional_millis(data, "last_ended_at"))
        .or_else(|| optional_millis(data, "updated_at"))
        .or_else(|| optional_millis(data, "first_started_at"));
    if incoming_active && !incoming_superseded {
        transaction.execute(
            "UPDATE unassigned_sessions
             SET state='discarded',resolution_type='discard',duration_seconds=0,
                 resolved_at=COALESCE(resolved_at,?1),last_ended_at=COALESCE(last_ended_at,?1),
                 updated_at=MAX(updated_at,?1),migration_state='superseded'
             WHERE workspace_id=?2 AND id<>?3
               AND state IN ('collecting','awaiting_resolution')
               AND (shared_source<>'cloud' OR migration_state<>'adopted'
                    OR work_date<?4 OR (work_date=?4 AND first_started_at<=?5)
                    OR NOT EXISTS(SELECT 1 FROM unassigned_segments open_segment
                                  WHERE open_segment.session_id=unassigned_sessions.id
                                    AND open_segment.ended_at IS NULL))",
            params![
                superseded_at,
                workspace_id,
                session_id,
                incoming_work_date,
                incoming_started_at
            ],
        )?;
        transaction.execute(
            "UPDATE unassigned_segments
             SET ended_at=MAX(started_at,?1),duration_seconds=0
             WHERE workspace_id=?2 AND ended_at IS NULL AND session_id<>?3",
            params![superseded_at, workspace_id, session_id],
        )?;
    } else if matches!(incoming_state.as_str(), "resolved" | "discarded") {
        // A terminal shared result can arrive before the authoritative next
        // anchor. Retire only local/migrated candidates here; an already
        // adopted cloud anchor may belong to a newer boundary and must not be
        // discarded by an out-of-order historical notification.
        transaction.execute(
            "UPDATE unassigned_sessions
             SET state='discarded',resolution_type='discard',duration_seconds=0,
                 resolved_at=COALESCE(resolved_at,?1),last_ended_at=COALESCE(last_ended_at,?1),
                 updated_at=MAX(updated_at,?1),migration_state='superseded'
             WHERE workspace_id=?2 AND id<>?3
               AND state IN ('collecting','awaiting_resolution')
               AND (shared_source<>'cloud' OR migration_state='candidate')
               AND COALESCE(predecessor_session_id,'')<>?3",
            params![superseded_at, workspace_id, session_id],
        )?;
        transaction.execute(
            "UPDATE unassigned_segments
             SET ended_at=MAX(started_at,?1),duration_seconds=0
             WHERE workspace_id=?2 AND ended_at IS NULL
               AND session_id IN (
                 SELECT id FROM unassigned_sessions
                 WHERE workspace_id=?2 AND id<>?3 AND migration_state='superseded'
               )",
            params![superseded_at, workspace_id, session_id],
        )?;
    }
    let state = if incoming_superseded {
        "discarded".to_string()
    } else {
        state
    };
    if matches!(state.as_str(), "collecting" | "awaiting_resolution") {
        if let Some(predecessor_session_id) = incoming_predecessor.as_deref() {
            transaction.execute(
                "DELETE FROM sync_outbox
                 WHERE entity_type='unassigned_session'
                   AND entity_id IN (
                     SELECT id FROM unassigned_sessions
                     WHERE workspace_id=?1 AND id<>?2 AND predecessor_session_id=?3
                   )",
                params![workspace_id, session_id, predecessor_session_id],
            )?;
            transaction.execute(
                "UPDATE unassigned_segments
                 SET ended_at=COALESCE(ended_at,MAX(started_at,?1)),
                     duration_seconds=CASE WHEN ended_at IS NULL
                       THEN MAX(0,(MAX(started_at,?1)-started_at)/1000)
                       ELSE duration_seconds END
                 WHERE workspace_id=?2 AND session_id IN (
                   SELECT id FROM unassigned_sessions
                   WHERE workspace_id=?2 AND id<>?3 AND predecessor_session_id=?4
                 )",
                params![
                    superseded_at,
                    workspace_id,
                    session_id,
                    predecessor_session_id
                ],
            )?;
            transaction.execute(
                "UPDATE unassigned_sessions
                 SET state='discarded',resolution_type='discard',
                     resolved_at=COALESCE(resolved_at,?1),last_ended_at=COALESCE(last_ended_at,?1),
                     updated_at=MAX(updated_at,?1),migration_state='superseded',
                     predecessor_session_id=NULL
                 WHERE workspace_id=?2 AND id<>?3 AND predecessor_session_id=?4",
                params![
                    superseded_at,
                    workspace_id,
                    session_id,
                    predecessor_session_id
                ],
            )?;
        }
    }
    let changed = transaction.execute(
        "INSERT INTO unassigned_sessions(
           id,workspace_id,work_date,state,threshold_seconds,duration_seconds,
           first_started_at,last_ended_at,prompted_at,resolution_type,generated_entry_id,
           resolved_at,created_at,updated_at,version,shared_source,predecessor_session_id,
           migration_state,resolution_operation_id,last_continuous_at
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,'cloud',?16,'adopted',?17,?18)
         ON CONFLICT(id) DO UPDATE SET
           work_date=excluded.work_date,state=excluded.state,
           threshold_seconds=excluded.threshold_seconds,duration_seconds=excluded.duration_seconds,
           first_started_at=excluded.first_started_at,last_ended_at=excluded.last_ended_at,
           prompted_at=excluded.prompted_at,resolution_type=excluded.resolution_type,
           generated_entry_id=excluded.generated_entry_id,resolved_at=excluded.resolved_at,
           updated_at=excluded.updated_at,version=excluded.version,shared_source='cloud',
           predecessor_session_id=excluded.predecessor_session_id,migration_state='adopted',
           resolution_operation_id=excluded.resolution_operation_id,
           last_continuous_at=excluded.last_continuous_at",
        params![
            session_id,
            workspace_id,
            text(data, "work_date"),
            state,
            integer(data, "threshold_seconds"),
            if incoming_active && !incoming_superseded {
                0
            } else {
                if incoming_superseded {
                    0
                } else {
                    integer(data, "duration_seconds")
                }
            },
            millis(data, "first_started_at"),
            optional_millis(data, "last_ended_at"),
            if incoming_active && !incoming_superseded {
                None
            } else {
                if incoming_superseded {
                    None
                } else {
                    optional_millis(data, "prompted_at")
                }
            },
            if incoming_superseded {
                Some("discard".to_string())
            } else {
                optional_text(data, "resolution_type")
            },
            if incoming_superseded {
                None
            } else {
                generated_entry_id
            },
            if incoming_superseded {
                Some(superseded_at)
            } else {
                optional_millis(data, "resolved_at")
            },
            millis(data, "created_at"),
            millis(data, "updated_at"),
            integer(data, "version"),
            incoming_predecessor,
            optional_text(data, "resolution_operation_id"),
            last_continuous_at,
        ],
    )?;
    if incoming_superseded {
        transaction.execute(
            "UPDATE unassigned_sessions
             SET state='discarded',resolution_type='discard',generated_entry_id=NULL,
                 duration_seconds=0,prompted_at=NULL,last_ended_at=?1,resolved_at=?1,
                 migration_state='superseded',last_continuous_at=?1
             WHERE workspace_id=?2 AND id=?3",
            params![superseded_at, workspace_id, session_id],
        )?;
    }
    transaction.execute(
        "DELETE FROM unassigned_segments WHERE workspace_id=?1 AND session_id=?2",
        params![workspace_id, session_id],
    )?;
    let segments = if incoming_superseded {
        Vec::new()
    } else if incoming_active {
        let started_at = incoming_segments
            .iter()
            .filter(|segment| optional_millis(segment, "ended_at").is_none())
            .map(|segment| millis(segment, "started_at"))
            .max()
            .unwrap_or_else(|| millis(data, "first_started_at"));
        vec![serde_json::json!({
            "id": Uuid::now_v7().to_string(),
            "session_id": session_id,
            "sequence_no": 1,
            "started_at": started_at,
            "ended_at": Value::Null,
            "duration_seconds": 0,
            "lease_token": Value::Null
        })]
    } else {
        incoming_segments
    };
    for segment in segments {
        transaction.execute(
            "INSERT INTO unassigned_segments(
               id,workspace_id,session_id,sequence_no,started_at,ended_at,duration_seconds,lease_token
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                text(&segment, "id"),
                workspace_id,
                session_id,
                integer(&segment, "sequence_no"),
                millis(&segment, "started_at"),
                optional_millis(&segment, "ended_at"),
                integer(&segment, "duration_seconds"),
                optional_text(&segment, "lease_token"),
            ],
        )?;
    }
    if incoming_active && !incoming_superseded {
        transaction.execute(
            "DELETE FROM sync_outbox
             WHERE entity_type='unassigned_session'
               AND entity_id IN (SELECT id FROM unassigned_sessions
                 WHERE workspace_id=?1 AND migration_state='superseded')",
            [workspace_id],
        )?;
    }
    Ok(changed)
}

fn apply_incremental_report(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    data: &Value,
) -> rusqlite::Result<usize> {
    let report_id = text(data, "id");
    transaction.execute(
        "DELETE FROM report_tasks WHERE workspace_id=?1 AND report_id=?2",
        params![workspace_id, report_id],
    )?;
    let changed = transaction.execute("INSERT INTO reports(id,workspace_id,report_type,subject_id,period_start,period_end,reference_date,template_id,markdown_content,content_source,generation_count,input_revision_hash,generated_at,created_at,updated_at,version,deleted_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17) ON CONFLICT(id) DO UPDATE SET report_type=excluded.report_type,subject_id=excluded.subject_id,period_start=excluded.period_start,period_end=excluded.period_end,reference_date=excluded.reference_date,template_id=excluded.template_id,markdown_content=excluded.markdown_content,content_source=excluded.content_source,generation_count=excluded.generation_count,input_revision_hash=excluded.input_revision_hash,generated_at=excluded.generated_at,updated_at=excluded.updated_at,version=excluded.version,deleted_at=excluded.deleted_at", params![report_id,workspace_id,text(data,"report_type"),text(data,"subject_id"),text(data,"period_start"),text(data,"period_end"),text(data,"reference_date"),optional_text(data,"template_id"),text(data,"markdown_content"),text(data,"content_source"),integer(data,"generation_count"),optional_text(data,"input_revision_hash"),optional_millis(data,"generated_at"),millis(data,"created_at"),millis(data,"updated_at"),integer(data,"version"),optional_millis(data,"deleted_at")])?;
    for row in data
        .get("report_tasks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        transaction.execute("INSERT INTO report_tasks(report_id,task_id,workspace_id,sort_order,title_snapshot,path_snapshot) VALUES (?1,?2,?3,?4,?5,?6)", params![report_id,text(&row,"task_id"),workspace_id,integer(&row,"sort_order"),text(&row,"title_snapshot"),text(&row,"path_snapshot")])?;
    }
    Ok(changed)
}

fn delete_incremental_entity(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    entity: &IncrementalEntity,
) -> Result<(), String> {
    let changed = match entity.entity_type.as_str() {
        "subjects" => transaction.execute(
            "DELETE FROM subjects WHERE workspace_id=?1 AND id=?2",
            params![workspace_id, entity.entity_id],
        ),
        "subject_work_schedules" => transaction.execute(
            "DELETE FROM subject_work_schedules WHERE workspace_id=?1 AND id=?2",
            params![workspace_id, entity.entity_id],
        ),
        "work_days" => transaction.execute(
            "DELETE FROM work_days WHERE workspace_id=?1 AND work_date=?2",
            params![workspace_id, entity.entity_id],
        ),
        "app_settings" => transaction.execute(
            "DELETE FROM app_settings WHERE workspace_id=?1 AND key=?2",
            params![workspace_id, entity.entity_id],
        ),
        "tasks" => transaction.execute(
            "DELETE FROM tasks WHERE workspace_id=?1 AND id=?2",
            params![workspace_id, entity.entity_id],
        ),
        "task_status_events" => transaction.execute(
            "DELETE FROM task_status_events WHERE workspace_id=?1 AND id=?2",
            params![workspace_id, entity.entity_id],
        ),
        "task_daily_estimates" => {
            let key = entity
                .entity_id
                .split_once('|')
                .ok_or_else(|| "VALIDATION_ERROR: 按日预估增量键无效".to_string())?;
            transaction.execute("DELETE FROM task_daily_estimates WHERE workspace_id=?1 AND task_id=?2 AND work_date=?3", params![workspace_id,key.0,key.1])
        }
        "task_day_plans" => {
            let key = entity
                .entity_id
                .split_once('|')
                .ok_or_else(|| "VALIDATION_ERROR: 今日计划增量键无效".to_string())?;
            transaction.execute(
                "DELETE FROM task_day_plans WHERE workspace_id=?1 AND task_id=?2 AND work_date=?3",
                params![workspace_id, key.0, key.1],
            )
        }
        "task_recurrence_rules" => transaction.execute(
            "DELETE FROM task_recurrence_rules WHERE workspace_id=?1 AND task_id=?2",
            params![workspace_id, entity.entity_id],
        ),
        "task_occurrences" => {
            let key = entity
                .entity_id
                .split_once('|')
                .ok_or_else(|| "VALIDATION_ERROR: 重复轮次增量键无效".to_string())?;
            transaction.execute("DELETE FROM task_occurrences WHERE workspace_id=?1 AND task_id=?2 AND occurrence_date=?3", params![workspace_id,key.0,key.1])
        }
        "unassigned_sessions" => {
            transaction
                .execute(
                    "DELETE FROM unassigned_segments WHERE workspace_id=?1 AND session_id=?2",
                    params![workspace_id, entity.entity_id],
                )
                .map_err(|error| error.to_string())?;
            transaction.execute(
                "DELETE FROM unassigned_sessions WHERE workspace_id=?1 AND id=?2",
                params![workspace_id, entity.entity_id],
            )
        }
        "time_entries" => transaction.execute(
            "DELETE FROM time_entries WHERE workspace_id=?1 AND id=?2",
            params![workspace_id, entity.entity_id],
        ),
        "actual_time_adjustments" => transaction.execute(
            "DELETE FROM actual_time_adjustments WHERE workspace_id=?1 AND id=?2",
            params![workspace_id, entity.entity_id],
        ),
        "report_templates" => transaction.execute(
            "DELETE FROM report_templates WHERE workspace_id=?1 AND id=?2",
            params![workspace_id, entity.entity_id],
        ),
        "reports" => transaction.execute(
            "DELETE FROM reports WHERE workspace_id=?1 AND id=?2",
            params![workspace_id, entity.entity_id],
        ),
        "integration_configs" => transaction.execute(
            "DELETE FROM integration_configs WHERE workspace_id=?1 AND provider=?2",
            params![workspace_id, entity.entity_id],
        ),
        "external_bindings" => {
            let parts = entity.entity_id.split('|').collect::<Vec<_>>();
            if parts.len() != 3 {
                return Err("VALIDATION_ERROR: 外部绑定增量键无效".to_string());
            }
            transaction.execute("DELETE FROM external_bindings WHERE workspace_id=?1 AND provider=?2 AND entity_type=?3 AND entity_id=?4", params![workspace_id,parts[0],parts[1],parts[2]])
        }
        other => {
            return Err(format!(
                "VALIDATION_ERROR: 不支持删除的云端增量实体 {other}"
            ))
        }
    };
    changed.map(|_| ()).map_err(|error| error.to_string())
}

#[cfg(test)]
pub fn enqueue_entity(
    database: &Database,
    operation_type: &str,
    entity_type: &str,
    entity_id: Option<&str>,
    base_version: Option<i64>,
    operation_id: Option<&str>,
) -> Result<(), String> {
    enqueue_entity_deferred(
        database,
        operation_type,
        entity_type,
        entity_id,
        base_version,
        operation_id,
    )?;
    let state = storage_mode(database)?;
    if state.mode == "cloud" && state.online {
        let result = push_outbox(database)?;
        if result.conflicts > 0 {
            return Err("SYNC_CONFLICT: 云端数据已变化，本地修改已保留在冲突队列".to_string());
        }
    }
    Ok(())
}

pub fn enqueue_entity_deferred(
    database: &Database,
    operation_type: &str,
    entity_type: &str,
    entity_id: Option<&str>,
    base_version: Option<i64>,
    operation_id: Option<&str>,
) -> Result<(), String> {
    let state = storage_mode(database)?;
    if state.mode != "cloud" {
        return Ok(());
    }
    ensure_supported_cloud_entity_type(entity_type)?;
    let payload = entity_payload(database, entity_type, entity_id)?;
    let operation_id = operation_id
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| Uuid::now_v7().to_string());
    let connection = database.open()?;
    connection
        .execute(
            "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, workspace_epoch, state, attempt_count, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'pending', 0, ?10)
             ON CONFLICT(operation_id) DO NOTHING",
            params![
                operation_id,
                state.workspace_id,
                state.device_id,
                operation_type,
                entity_type,
                entity_id,
                base_version,
                payload.to_string(),
                state.workspace_epoch,
                now_millis()
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub(crate) fn enqueue_entity_in_transaction(
    transaction: &Transaction<'_>,
    state: &crate::supabase::StorageModeSnapshot,
    operation_type: &str,
    entity_type: &str,
    entity_id: Option<&str>,
    base_version: Option<i64>,
    operation_id: Option<&str>,
) -> Result<Option<String>, String> {
    enqueue_entity_in_transaction_with_dependency(
        transaction,
        state,
        operation_type,
        entity_type,
        entity_id,
        base_version,
        operation_id,
        None,
    )
}

pub(crate) fn enqueue_entity_in_transaction_with_dependency(
    transaction: &Transaction<'_>,
    state: &crate::supabase::StorageModeSnapshot,
    operation_type: &str,
    entity_type: &str,
    entity_id: Option<&str>,
    base_version: Option<i64>,
    operation_id: Option<&str>,
    depends_on_operation_id: Option<&str>,
) -> Result<Option<String>, String> {
    if state.mode != "cloud" {
        return Ok(None);
    }
    ensure_supported_cloud_entity_type(entity_type)?;
    let payload = entity_payload_from_connection(transaction, entity_type, entity_id)?;
    let payload_version = payload.get("version").and_then(Value::as_i64);
    let generated_operation_id = if entity_type == "time_entry"
        && payload.get("source_type").and_then(Value::as_str) == Some("timer")
    {
        stable_timer_operation_id(
            entity_id.ok_or_else(|| "VALIDATION_ERROR: 计时同步缺少实体 ID".to_string())?,
            base_version,
            payload_version.ok_or_else(|| "VALIDATION_ERROR: 计时同步缺少目标版本".to_string())?,
            operation_type,
        )
    } else {
        operation_id
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| Uuid::now_v7().to_string())
    };
    if entity_type == "time_entry" && depends_on_operation_id.is_none() {
        if let Some(existing_operation_id) = coalesce_pending_time_entry(
            transaction,
            &state.workspace_id,
            entity_id,
            operation_type,
            &payload,
        )? {
            return Ok(Some(existing_operation_id));
        }
    }
    if entity_type == "unassigned_session"
        && depends_on_operation_id.is_none()
        && matches!(
            operation_type,
            "unassigned_session_pause"
                | "unassigned_session_resume"
                | "unassigned_session_awaiting_resolution"
        )
    {
        if let Some(existing_operation_id) = coalesce_pending_unassigned_lifecycle(
            transaction,
            &state.workspace_id,
            entity_id,
            operation_type,
            &payload,
        )? {
            return Ok(Some(existing_operation_id));
        }
    }
    transaction
        .execute(
            "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, payload_version, depends_on_operation_id, workspace_epoch, state, attempt_count, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'pending', 0, ?12)
             ON CONFLICT(operation_id) DO NOTHING",
            params![
                generated_operation_id,
                state.workspace_id,
                state.device_id,
                operation_type,
                entity_type,
                entity_id,
                base_version,
                payload.to_string(),
                payload_version,
                depends_on_operation_id,
                state.workspace_epoch,
                now_millis()
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(Some(generated_operation_id))
}

pub(crate) fn enqueue_payload_in_transaction(
    transaction: &Transaction<'_>,
    state: &crate::supabase::StorageModeSnapshot,
    operation_type: &str,
    entity_type: &str,
    entity_id: &str,
    base_version: Option<i64>,
    payload: &Value,
    operation_id: Option<&str>,
    depends_on_operation_id: Option<&str>,
) -> Result<Option<String>, String> {
    if state.mode != "cloud" {
        return Ok(None);
    }
    ensure_supported_cloud_entity_type(entity_type)?;
    validate_planning_entity_payload(entity_type, entity_id, payload)?;
    let generated_operation_id = operation_id
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| Uuid::now_v7().to_string());
    let payload_version = payload.get("version").and_then(Value::as_i64);
    transaction
        .execute(
            "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, payload_version, depends_on_operation_id, workspace_epoch, state, attempt_count, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'pending', 0, ?12)
             ON CONFLICT(operation_id) DO NOTHING",
            params![
                generated_operation_id,
                state.workspace_id,
                state.device_id,
                operation_type,
                entity_type,
                entity_id,
                base_version,
                payload.to_string(),
                payload_version,
                depends_on_operation_id,
                state.workspace_epoch,
                now_millis()
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(Some(generated_operation_id))
}

pub(crate) fn enqueue_actual_adjustment_batch_in_transaction(
    transaction: &Transaction<'_>,
    state: &crate::supabase::StorageModeSnapshot,
    operation_id: &str,
    payload: &Value,
) -> Result<Option<String>, String> {
    if state.mode != "cloud" {
        return Ok(None);
    }
    Uuid::parse_str(operation_id)
        .map_err(|_| "VALIDATION_ERROR: 云端实际调整批次请求标识必须是 UUID".to_string())?;
    let batch_id = payload
        .get("batchId")
        .and_then(Value::as_str)
        .ok_or_else(|| "VALIDATION_ERROR: 实际调整批次缺少批次 ID".to_string())?;
    Uuid::parse_str(batch_id)
        .map_err(|_| "VALIDATION_ERROR: 云端实际调整批次 ID 必须是 UUID".to_string())?;
    let mut task_ids = std::collections::HashSet::new();
    if let Some(items) = payload.get("items").and_then(Value::as_array) {
        for item in items {
            if let Some(task_id) = item.get("taskId").and_then(Value::as_str) {
                task_ids.insert(task_id.to_string());
            }
        }
    }
    if let Some(created_tasks) = payload.get("createdTasks").and_then(Value::as_array) {
        for created in created_tasks {
            if let Some(task_id) = created
                .get("task")
                .and_then(|task| task.get("id"))
                .and_then(Value::as_str)
            {
                task_ids.insert(task_id.to_string());
            }
        }
    }
    let mut dependency: Option<(i64, String)> = None;
    for task_id in task_ids {
        let candidate = transaction
            .query_row(
                "SELECT created_at,operation_id FROM sync_outbox
                 WHERE workspace_id=?1 AND entity_type='task' AND entity_id=?2
                   AND state IN ('pending','sending','failed')
                 ORDER BY created_at DESC,rowid DESC LIMIT 1",
                params![state.workspace_id, task_id],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        if candidate.as_ref().is_some_and(|candidate| {
            dependency
                .as_ref()
                .is_none_or(|current| candidate > current)
        }) {
            dependency = candidate;
        }
    }
    transaction
        .execute(
            "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, payload_version, depends_on_operation_id, workspace_epoch, state, attempt_count, created_at)
             VALUES (?1, ?2, ?3, 'actual_adjustment_batch_apply', 'actual_adjustment_batch', ?4, NULL, ?5, NULL, ?6, ?7, 'pending', 0, ?8)
             ON CONFLICT(operation_id) DO NOTHING",
            params![operation_id, state.workspace_id, state.device_id, batch_id, payload.to_string(), dependency.map(|item| item.1), state.workspace_epoch, now_millis()],
        )
        .map_err(|error| error.to_string())?;
    Ok(Some(operation_id.to_string()))
}

fn validate_planning_entity_payload(
    entity_type: &str,
    entity_id: &str,
    payload: &Value,
) -> Result<(), String> {
    match entity_type {
        "task_day_plan" => {
            let task_id = payload
                .get("task_id")
                .and_then(Value::as_str)
                .ok_or_else(|| "VALIDATION_ERROR: 今日计划同步缺少事项 ID".to_string())?;
            let work_date = payload
                .get("work_date")
                .and_then(Value::as_str)
                .ok_or_else(|| "VALIDATION_ERROR: 今日计划同步缺少日期".to_string())?;
            chrono::NaiveDate::parse_from_str(work_date, "%Y-%m-%d")
                .map_err(|_| "VALIDATION_ERROR: 今日计划同步日期无效".to_string())?;
            if entity_id != format!("{task_id}|{work_date}") {
                return Err("VALIDATION_ERROR: 今日计划同步实体 ID 无效".to_string());
            }
            if !matches!(
                payload.get("state").and_then(Value::as_str),
                Some("included") | Some("excluded")
            ) {
                return Err("VALIDATION_ERROR: 今日计划同步状态无效".to_string());
            }
        }
        "task_daily_estimate" => {
            let task_id = payload
                .get("task_id")
                .and_then(Value::as_str)
                .ok_or_else(|| "VALIDATION_ERROR: 按日预估同步缺少事项 ID".to_string())?;
            let work_date = payload
                .get("work_date")
                .and_then(Value::as_str)
                .ok_or_else(|| "VALIDATION_ERROR: 按日预估同步缺少日期".to_string())?;
            chrono::NaiveDate::parse_from_str(work_date, "%Y-%m-%d")
                .map_err(|_| "VALIDATION_ERROR: 按日预估同步日期无效".to_string())?;
            if entity_id != format!("{task_id}|{work_date}") {
                return Err("VALIDATION_ERROR: 按日预估同步实体 ID 无效".to_string());
            }
            if payload.get("deleted").and_then(Value::as_bool) != Some(true)
                && payload
                    .get("estimate_minutes")
                    .and_then(Value::as_i64)
                    .is_none()
            {
                return Err("VALIDATION_ERROR: 按日预估同步缺少预计分钟".to_string());
            }
        }
        "task_recurrence_rule" => {
            let task_id = payload
                .get("task_id")
                .and_then(Value::as_str)
                .ok_or_else(|| "VALIDATION_ERROR: 重复规则同步缺少事项 ID".to_string())?;
            if entity_id != task_id {
                return Err("VALIDATION_ERROR: 重复规则同步实体 ID 无效".to_string());
            }
            if !matches!(
                payload.get("action").and_then(Value::as_str),
                Some("save") | Some("close")
            ) {
                return Err("VALIDATION_ERROR: 重复规则同步动作无效".to_string());
            }
            if !payload.get("rules").is_some_and(Value::is_array) {
                return Err("VALIDATION_ERROR: 重复规则同步缺少规则快照".to_string());
            }
        }
        _ => {}
    }
    Ok(())
}

// Only never-attempted pending timer snapshots are replaceable. Sending, failed,
// and conflict rows are hard boundaries because their remote outcome may matter.
fn coalesce_pending_time_entry(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    entity_id: Option<&str>,
    operation_type: &str,
    payload: &Value,
) -> Result<Option<String>, String> {
    let Some(entity_id) = entity_id else {
        return Ok(None);
    };
    let mut statement = transaction
        .prepare(
            "SELECT operation_id, state, attempt_count, coalesced_count, base_version, payload_version
             FROM sync_outbox
             WHERE workspace_id = ?1 AND entity_type = 'time_entry' AND entity_id = ?2
             ORDER BY created_at, rowid",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![workspace_id, entity_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<i64>>(5)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(statement);
    if rows.is_empty()
        || rows
            .iter()
            .any(|(_, state, attempt_count, _, _, _)| state != "pending" || *attempt_count != 0)
    {
        return Ok(None);
    }

    let kept_operation_id = rows[0].0.clone();
    let coalesced_count = rows.iter().map(|row| row.3).sum::<i64>() + 1;
    let payload_version = payload.get("version").and_then(Value::as_i64);
    let mut expected_base_version = rows[0].4;
    let mut expected_version = expected_base_version.unwrap_or(0);
    for row in &rows {
        if row.4 != expected_base_version {
            return Ok(None);
        }
        expected_version = match expected_version.checked_add(row.3) {
            Some(version) => version,
            None => return Ok(None),
        };
        if row.5 != Some(expected_version) {
            return Ok(None);
        }
        expected_base_version = Some(expected_version);
    }
    expected_version = match expected_version.checked_add(1) {
        Some(version) => version,
        None => return Ok(None),
    };
    if payload_version != Some(expected_version) {
        return Ok(None);
    }
    let replacement_operation_id =
        if payload.get("source_type").and_then(Value::as_str) == Some("timer") {
            stable_timer_operation_id(
                entity_id,
                rows[0].4,
                payload_version.unwrap_or_default(),
                operation_type,
            )
        } else {
            kept_operation_id.clone()
        };
    transaction
        .execute(
            "UPDATE sync_outbox
             SET operation_type = ?1, payload_json = ?2, payload_version = ?3,
                 coalesced_count = ?4
             WHERE operation_id = ?5",
            params![
                operation_type,
                payload.to_string(),
                payload_version,
                coalesced_count,
                kept_operation_id
            ],
        )
        .map_err(|error| error.to_string())?;
    for (operation_id, _, _, _, _, _) in rows.iter().skip(1) {
        transaction
            .execute(
                "UPDATE sync_outbox SET depends_on_operation_id = ?1 WHERE depends_on_operation_id = ?2",
                params![kept_operation_id, operation_id],
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "DELETE FROM sync_outbox WHERE operation_id = ?1",
                [operation_id],
            )
            .map_err(|error| error.to_string())?;
    }
    if replacement_operation_id != kept_operation_id {
        transaction
            .execute(
                "UPDATE sync_outbox SET operation_id=?1 WHERE operation_id=?2",
                params![replacement_operation_id, kept_operation_id],
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "UPDATE sync_outbox SET depends_on_operation_id=?1 WHERE depends_on_operation_id=?2",
                params![replacement_operation_id, kept_operation_id],
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(Some(replacement_operation_id))
}

// Lifecycle images may replace one another only while every earlier image is
// still unattempted. Create and terminal resolution operations remain hard
// boundaries because they use dedicated RPCs and carry business side effects.
fn coalesce_pending_unassigned_lifecycle(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    entity_id: Option<&str>,
    operation_type: &str,
    payload: &Value,
) -> Result<Option<String>, String> {
    let Some(entity_id) = entity_id else {
        return Ok(None);
    };
    let mut statement = transaction
        .prepare(
            "SELECT operation_id,operation_type,state,attempt_count,coalesced_count,base_version,payload_version
             FROM sync_outbox
             WHERE workspace_id=?1 AND entity_type='unassigned_session' AND entity_id=?2
             ORDER BY created_at,rowid",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![workspace_id, entity_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, Option<i64>>(5)?,
                row.get::<_, Option<i64>>(6)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(statement);
    if rows.is_empty()
        || rows
            .iter()
            .any(|(_, existing_type, state, attempt_count, _, _, _)| {
                state != "pending"
                    || *attempt_count != 0
                    || !matches!(
                        existing_type.as_str(),
                        "unassigned_session_pause"
                            | "unassigned_session_resume"
                            | "unassigned_session_awaiting_resolution"
                    )
            })
    {
        return Ok(None);
    }
    let kept_operation_id = rows[0].0.clone();
    let mut expected_base_version = rows[0].5;
    let mut expected_version = expected_base_version.unwrap_or(0);
    for row in &rows {
        if row.5 != expected_base_version {
            return Ok(None);
        }
        expected_version = match expected_version.checked_add(row.4) {
            Some(version) => version,
            None => return Ok(None),
        };
        if row.6 != Some(expected_version) {
            return Ok(None);
        }
        expected_base_version = Some(expected_version);
    }
    expected_version = match expected_version.checked_add(1) {
        Some(version) => version,
        None => return Ok(None),
    };
    let payload_version = payload.get("version").and_then(Value::as_i64);
    if payload_version != Some(expected_version) {
        return Ok(None);
    }
    let coalesced_count = rows.iter().map(|row| row.4).sum::<i64>() + 1;
    transaction
        .execute(
            "UPDATE sync_outbox
             SET operation_type=?1,payload_json=?2,payload_version=?3,coalesced_count=?4
             WHERE operation_id=?5",
            params![
                operation_type,
                payload.to_string(),
                payload_version,
                coalesced_count,
                kept_operation_id
            ],
        )
        .map_err(|error| error.to_string())?;
    for row in rows.iter().skip(1) {
        transaction
            .execute(
                "UPDATE sync_outbox SET depends_on_operation_id=?1 WHERE depends_on_operation_id=?2",
                params![kept_operation_id, row.0],
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute("DELETE FROM sync_outbox WHERE operation_id=?1", [&row.0])
            .map_err(|error| error.to_string())?;
    }
    Ok(Some(kept_operation_id))
}

fn ensure_supported_cloud_entity_type(entity_type: &str) -> Result<(), String> {
    if matches!(
        entity_type,
        "subject"
            | "subject_work_schedule"
            | "task"
            | "time_entry"
            | "timer_transition"
            | "actual_time_adjustment"
            | "actual_adjustment_batch"
            | "unassigned_session"
            | "report"
            | "report_template"
            | "app_setting"
            | "integration_config"
            | "task_occurrence"
            | "task_day_plan"
            | "task_daily_estimate"
            | "task_recurrence_rule"
            | "external_binding"
    ) {
        Ok(())
    } else {
        Err(format!(
            "VALIDATION_ERROR: 不支持同步实体 {entity_type}，请先扩展 Supabase cloud_apply_patch"
        ))
    }
}

fn flush_if_online_with_signal_result(database: &Database) -> Result<bool, String> {
    if CLOUD_SYNC_ENABLED.load(Ordering::Acquire) {
        if storage_mode(database).is_ok_and(|state| state.mode != "cloud") {
            return Ok(false);
        }
        signal_cloud_sync(SYNC_SIGNAL_LOCAL_DIRTY, None);
        if let Some(app) = CLOUD_SYNC_APP_HANDLE.get() {
            if let Ok(status) = sync_status(database) {
                let _ = app.emit("cloud-sync-state-changed", &status);
            }
        }
        return Ok(true);
    }
    Ok(false)
}

pub fn flush_if_online(database: &Database) -> Result<(), String> {
    flush_if_online_with_signal_result(database)?;
    Ok(())
}

#[cfg(test)]
pub fn push_outbox(database: &Database) -> Result<CloudSyncPushResult, String> {
    push_outbox_with_retry_mode(database, false, false)
}

fn push_outbox_with_retry_mode(
    database: &Database,
    retry_immediately: bool,
    drain_queue: bool,
) -> Result<CloudSyncPushResult, String> {
    let _operation_guard = cloud_sync_operation_guard();
    push_outbox_locked(database, retry_immediately, drain_queue)
}

fn cloud_sync_operation_guard() -> std::sync::MutexGuard<'static, ()> {
    CLOUD_SYNC_OPERATION_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) fn with_cloud_sync_operation_lock<T>(
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let _operation_guard = cloud_sync_operation_guard();
    operation()
}

fn push_outbox_locked(
    database: &Database,
    retry_immediately: bool,
    drain_queue: bool,
) -> Result<CloudSyncPushResult, String> {
    let state = require_cloud(database)?;
    recover_interrupted_sending_operations(database, &state.workspace_id)?;
    mark_historical_timer_recovery_candidates(database, &state.workspace_id)?;
    recover_historical_missing_task_occurrences(database, &state.workspace_id)?;
    ensure_current_device_authorized(database).map_err(|error| {
        if error.starts_with("DEVICE_REVOKED:") {
            let _ = mark_auth_blocked(database, "device_revoked");
        }
        error
    })?;
    let session = current_session(database)?;
    let api = client(database)?;
    let mut pushed = 0;
    loop {
        let operations = claim_pending_operations_with_retry_mode(
            database,
            &state.workspace_id,
            retry_immediately,
        )?;
        if operations.is_empty() {
            break;
        }

        let mut batch_stopped = false;
        let mut retry_rebased_occurrence = false;
        for (index, operation) in operations.iter().enumerate() {
            let response = if operation.entity_type == "unassigned_session"
                && operation.operation_type == "unassigned_session_create"
            {
                api.rpc_for_database_with_epoch(
                    database,
                    "unassigned_get_or_create_shared",
                    json!({
                        "p_workspace_id": state.workspace_id,
                        "p_device_id": state.device_id,
                        "p_candidate": operation.payload,
                        "p_predecessor_session_id": operation.payload.get("predecessor_session_id"),
                        "p_started_at": Value::Null
                    }),
                    &session,
                    operation.workspace_epoch,
                )
            } else if operation.entity_type == "unassigned_session"
                && matches!(
                    operation.operation_type.as_str(),
                    "unassigned_resolve_work" | "unassigned_resolve_break" | "unassigned_discard"
                )
            {
                let resolution_type = match operation.operation_type.as_str() {
                    "unassigned_resolve_work" => "work",
                    "unassigned_resolve_break" => "break",
                    _ => "discard",
                };
                api.rpc_for_database_with_epoch(
                    database,
                    "unassigned_resolve_shared",
                    json!({
                        "p_workspace_id": state.workspace_id,
                        "p_device_id": state.device_id,
                        "p_operation_id": operation.operation_id,
                        "p_session_id": operation.entity_id,
                        "p_expected_version": operation.base_version,
                        "p_resolution_type": resolution_type,
                        "p_payload": operation.payload
                    }),
                    &session,
                    operation.workspace_epoch,
                )
            } else if operation.entity_type == "unassigned_session"
                && operation.operation_type == "unassigned_anchor_reset"
            {
                api.rpc_for_database_with_epoch(
                    database,
                    "unassigned_anchor_reset",
                    json!({
                        "p_workspace_id": state.workspace_id,
                        "p_device_id": state.device_id,
                        "p_operation_id": operation.operation_id,
                        "p_session_id": operation.entity_id,
                        "p_expected_version": operation.base_version,
                        "p_resumed_at_millis": operation.payload.get("first_started_at")
                    }),
                    &session,
                    operation.workspace_epoch,
                )
            } else if operation.entity_type == "actual_adjustment_batch" {
                api.rpc_for_database_with_epoch(
                    database,
                    "actual_adjustment_batch_apply",
                    json!({
                        "p_workspace_id": state.workspace_id,
                        "p_device_id": state.device_id,
                        "p_operation_id": operation.operation_id,
                        "p_payload": operation.payload
                    }),
                    &session,
                    operation.workspace_epoch,
                )
            } else if operation.entity_type == "timer_transition" {
                api.rpc_for_database_with_epoch(
                    database,
                    "timer_transition_apply",
                    json!({
                        "p_workspace_id": state.workspace_id,
                        "p_device_id": state.device_id,
                        "p_operation_id": operation.operation_id,
                        "p_payload": operation.payload
                    }),
                    &session,
                    operation.workspace_epoch,
                )
            } else {
                api.rpc_for_database_with_epoch(
                    database,
                    "cloud_apply_patch",
                    json!({
                        "p_workspace_id": state.workspace_id,
                        "p_device_id": state.device_id,
                        "p_operation_id": operation.operation_id,
                        "p_operation_type": operation.operation_type,
                        "p_entity_type": operation.entity_type,
                        "p_entity_id": operation.entity_id,
                        "p_base_version": operation.base_version,
                        "p_payload_version": operation.payload_version,
                        "p_coalesced_count": operation.coalesced_count,
                        "p_payload": operation.payload
                    }),
                    &session,
                    operation.workspace_epoch,
                )
            };
            match response {
                Ok(value) => {
                    if operation.entity_type == "time_entry"
                        && operation.payload.get("source_type").and_then(Value::as_str)
                            == Some("timer")
                        && value.get("accepted").and_then(Value::as_bool) == Some(false)
                    {
                        match reconcile_timer_sync_context(database, &state, operation, &value)? {
                            TimerSyncRecovery::Acknowledged => {
                                delete_outbox(database, &operation.operation_id)?;
                                pushed += 1;
                                continue;
                            }
                            TimerSyncRecovery::Rebased => {
                                release_unprocessed_claims(database, &operations[index + 1..])?;
                                batch_stopped = true;
                                break;
                            }
                            TimerSyncRecovery::Followed { entity_id, version } => {
                                emit_timer_fact_followed(
                                    &operation.operation_type,
                                    &entity_id,
                                    version,
                                );
                                pushed += 1;
                                continue;
                            }
                            TimerSyncRecovery::Conflict(reason) => {
                                mark_outbox_recovery_error(
                                    database,
                                    &operation.operation_id,
                                    "conflict",
                                    "competing_edit",
                                    &reason,
                                    Some(&value),
                                )?;
                                release_unprocessed_claims(database, &operations[index + 1..])?;
                                batch_stopped = true;
                                break;
                            }
                        }
                    }
                    if operation.entity_type == "unassigned_session"
                        && operation.operation_type == "unassigned_session_create"
                        && !value.is_null()
                    {
                        adopt_shared_unassigned_rpc_result(database, &value)?;
                    }
                    if operation.entity_type == "unassigned_session"
                        && operation.operation_type == "unassigned_anchor_reset"
                    {
                        if let Some(session) =
                            value.get("session").filter(|session| !session.is_null())
                        {
                            adopt_shared_unassigned_rpc_result(database, session)?;
                        }
                    }
                    adopt_superseded_shared_unassigned_lifecycle_result(
                        database, operation, &value,
                    )?;
                    if operation.entity_type == "unassigned_session"
                        && matches!(
                            operation.operation_type.as_str(),
                            "unassigned_resolve_work"
                                | "unassigned_resolve_break"
                                | "unassigned_discard"
                        )
                    {
                        adopt_shared_unassigned_resolution_result(database, operation, &value)?;
                    }
                    if operation.entity_type == "time_entry" {
                        adopt_authoritative_calendar_timer_result(database, operation, &value)?;
                        if let Some(session) = value
                            .get("unassignedSession")
                            .filter(|session| !session.is_null())
                        {
                            adopt_shared_unassigned_rpc_result(database, session)?;
                        }
                    }
                    if operation.entity_type == "timer_transition" {
                        adopt_timer_transition_occurrence_result(database, operation, &value)?;
                    }
                    if operation.entity_type == "actual_adjustment_batch" {
                        adopt_actual_adjustment_batch_result(database, operation, &value)?;
                    }
                    if operation.entity_type == "report_template" {
                        adopt_authoritative_report_template_result(database, operation, &value)?;
                    }
                    delete_outbox(database, &operation.operation_id)?;
                    pushed += 1;
                }
                Err(error)
                    if is_conflict_error(&error) && operation.entity_type == "time_entry" =>
                {
                    let context = fetch_timer_sync_context(database, &state, operation)?;
                    match reconcile_timer_sync_context(database, &state, operation, &context)? {
                        TimerSyncRecovery::Acknowledged => {
                            delete_outbox(database, &operation.operation_id)?;
                            pushed += 1;
                            continue;
                        }
                        TimerSyncRecovery::Rebased => {}
                        TimerSyncRecovery::Followed { entity_id, version } => {
                            emit_timer_fact_followed(
                                &operation.operation_type,
                                &entity_id,
                                version,
                            );
                            pushed += 1;
                            continue;
                        }
                        TimerSyncRecovery::Conflict(reason) => {
                            mark_outbox_recovery_error(
                                database,
                                &operation.operation_id,
                                "conflict",
                                "competing_edit",
                                &reason,
                                Some(&context),
                            )?;
                        }
                    }
                    release_unprocessed_claims(database, &operations[index + 1..])?;
                    batch_stopped = true;
                    break;
                }
                Err(error)
                    if is_conflict_error(&error) && operation.entity_type == "timer_transition" =>
                {
                    match reconcile_timer_transition_conflict(database, &state, operation)? {
                        TimerSyncRecovery::Followed { entity_id, version } => {
                            emit_timer_fact_followed(
                                &operation.operation_type,
                                &entity_id,
                                version,
                            );
                            pushed += 1;
                            continue;
                        }
                        TimerSyncRecovery::Acknowledged => {
                            delete_outbox(database, &operation.operation_id)?;
                            pushed += 1;
                            continue;
                        }
                        TimerSyncRecovery::Rebased => {}
                        TimerSyncRecovery::Conflict(reason) => {
                            mark_outbox_recovery_error(
                                database,
                                &operation.operation_id,
                                "conflict",
                                "competing_edit",
                                &reason,
                                None,
                            )?;
                        }
                    }
                    release_unprocessed_claims(database, &operations[index + 1..])?;
                    batch_stopped = true;
                    break;
                }
                Err(error) if is_workspace_epoch_stale_error(&error) => {
                    mark_outbox_error(database, &operation.operation_id, "pending", &error)?;
                    release_unprocessed_claims(database, &operations[index + 1..])?;
                    let server_epoch = parse_server_workspace_epoch(&error)
                        .unwrap_or_else(|| state.workspace_epoch.saturating_add(1));
                    mark_epoch_blocked(
                        database,
                        &state.workspace_id,
                        server_epoch,
                        "云端工作空间已回滚，本机旧世代修改尚未处理",
                    )?;
                    batch_stopped = true;
                    break;
                }
                Err(error)
                    if operation.entity_type == "task_occurrence"
                        && is_missing_task_occurrence_conflict(&error) =>
                {
                    rebase_missing_task_occurrence(database, &state.workspace_id, operation)?;
                    release_unprocessed_claims(database, &operations[index + 1..])?;
                    batch_stopped = true;
                    retry_rebased_occurrence = true;
                    break;
                }
                Err(error) if is_conflict_error(&error) => {
                    mark_outbox_error(database, &operation.operation_id, "conflict", &error)?;
                    release_unprocessed_claims(database, &operations[index + 1..])?;
                    batch_stopped = true;
                    break;
                }
                Err(error)
                    if operation.entity_type == "unassigned_session"
                        && operation.operation_type == "unassigned_discard"
                        && is_missing_shared_unassigned_session_error(&error) =>
                {
                    let recovery = api
                        .rpc_for_database_with_epoch(
                            database,
                            "unassigned_get_or_create_shared",
                            json!({
                                "p_workspace_id": state.workspace_id,
                                "p_device_id": state.device_id,
                                "p_candidate": operation.payload,
                                "p_predecessor_session_id": operation
                                    .payload
                                    .get("predecessor_session_id"),
                                "p_started_at": Value::Null
                            }),
                            &session,
                            operation.workspace_epoch,
                        )
                        .and_then(|authoritative| {
                            let authoritative_id =
                                authoritative.get("id").and_then(Value::as_str).ok_or_else(
                                    || "VALIDATION_ERROR: 权威未归属会话缺少 ID".to_string(),
                                )?;
                            if operation.entity_id.as_deref() == Some(authoritative_id) {
                                api.rpc_for_database_with_epoch(
                                    database,
                                    "unassigned_resolve_shared",
                                    json!({
                                        "p_workspace_id": state.workspace_id,
                                        "p_device_id": state.device_id,
                                        "p_operation_id": operation.operation_id,
                                        "p_session_id": operation.entity_id,
                                        "p_expected_version": operation.base_version,
                                        "p_resolution_type": "discard",
                                        "p_payload": operation.payload
                                    }),
                                    &session,
                                    operation.workspace_epoch,
                                )
                                .and_then(|value| {
                                    adopt_shared_unassigned_resolution_result(
                                        database, operation, &value,
                                    )
                                })
                            } else {
                                recover_orphaned_shared_unassigned_discard(
                                    database,
                                    operation,
                                    &authoritative,
                                )
                            }
                        });
                    match recovery {
                        Ok(()) => {
                            delete_outbox(database, &operation.operation_id)?;
                            pushed += 1;
                            continue;
                        }
                        Err(recovery_error) => {
                            let combined_error = format!(
                                "{error}; ORPHANED_UNASSIGNED_RECOVERY_FAILED: {recovery_error}"
                            );
                            mark_outbox_error(
                                database,
                                &operation.operation_id,
                                "pending",
                                &combined_error,
                            )?;
                            release_unprocessed_claims(database, &operations[index + 1..])?;
                            update_sync_state(
                                database,
                                &state.workspace_id,
                                &state.device_id,
                                state.last_change_seq,
                                Some(&combined_error),
                            )?;
                            batch_stopped = true;
                            break;
                        }
                    }
                }
                Err(error) => {
                    mark_outbox_error(database, &operation.operation_id, "pending", &error)?;
                    release_unprocessed_claims(database, &operations[index + 1..])?;
                    update_sync_state(
                        database,
                        &state.workspace_id,
                        &state.device_id,
                        state.last_change_seq,
                        Some(&error),
                    )?;
                    batch_stopped = true;
                    break;
                }
            }
        }
        if retry_rebased_occurrence {
            continue;
        }
        if batch_stopped || !drain_queue {
            break;
        }
    }
    let (pending, conflicts) = outbox_counts(database, &state.workspace_id)?;
    Ok(CloudSyncPushResult {
        pushed,
        pending,
        conflicts,
    })
}

/// Drains the legacy outbox and pulls every pending incremental page while the
/// upgrade operation holds the sync lock. The callback runs before the lock is
/// released so the protocol switch cannot race a normal sync round.
pub(crate) fn drain_legacy_queue_then<T>(
    database: &Database,
    after_drain: impl FnOnce(&LegacyQueueDrainResult) -> Result<T, String>,
) -> Result<T, String> {
    let _operation_guard = cloud_sync_operation_guard();
    let result = drain_legacy_queue_locked(database)?;
    after_drain(&result)
}

fn drain_legacy_queue_locked(database: &Database) -> Result<LegacyQueueDrainResult, String> {
    let state = storage_mode(database)?;
    if state.mode != "cloud" {
        return Err("CLOUD_MODE_REQUIRED: 只有云端模式可以处理旧同步队列".to_string());
    }
    if state.storage_protocol != crate::storage_contract::LEGACY_SYNC_PROTOCOL {
        return Err("CLIENT_UPGRADE_REQUIRED: 当前工作空间已不是旧同步协议".to_string());
    }
    if !state.online {
        return Err("AUTH_REQUIRED: 云端会话不可用，无法完成旧同步队列".to_string());
    }

    let push = push_outbox_locked(database, true, true)?;
    if push.pending > 0 || push.conflicts > 0 {
        return Err(format!(
            "LEGACY_QUEUE_BLOCKED: 旧同步队列仍有 {} 条待处理操作和 {} 条冲突，请处理后重试",
            push.pending, push.conflicts
        ));
    }

    let mut last_change_seq = storage_mode(database)?.last_change_seq;
    let mut domains = std::collections::BTreeSet::<String>::new();
    loop {
        let previous_seq = last_change_seq;
        let pulled = pull_locked(
            database,
            CloudSyncPullRequest {
                after_change_seq: Some(last_change_seq),
                limit: Some(500),
            },
        )?;
        last_change_seq = pulled.last_change_seq.max(last_change_seq);
        domains.extend(pulled.domains);
        if !pulled.has_more {
            break;
        }
        if last_change_seq <= previous_seq {
            return Err("SYNC_CONVERGENCE_STALLED: 云端增量游标没有推进，升级已暂停".to_string());
        }
    }

    let final_state = storage_mode(database)?;
    if final_state.pending_operations > 0 || final_state.conflict_count > 0 {
        return Err(format!(
            "LEGACY_QUEUE_BLOCKED: 最终刷新后仍有 {} 条待处理操作和 {} 条冲突",
            final_state.pending_operations, final_state.conflict_count
        ));
    }
    Ok(LegacyQueueDrainResult {
        pushed: push.pushed,
        last_change_seq,
        domains: domains.into_iter().collect(),
    })
}

fn fetch_timer_sync_context(
    database: &Database,
    state: &crate::supabase::StorageModeSnapshot,
    operation: &OutboxOperation,
) -> Result<Value, String> {
    let session = current_session(database)?;
    let request = json!({
        "deviceId": state.device_id,
        "entityType": operation.entity_type,
        "entityId": operation.entity_id,
        "baseVersion": operation.base_version,
        "payloadVersion": operation.payload_version,
        "coalescedCount": operation.coalesced_count,
        "payload": operation.payload
    });
    client(database)?.rpc_for_database(
        database,
        "timer_sync_context_get",
        json!({
            "p_workspace_id": state.workspace_id,
            "p_operation_id": operation.operation_id,
            "p_entity_id": operation.entity_id,
            "p_operation_type": operation.operation_type,
            "p_request": request
        }),
        &session,
    )
}

fn timer_transition_timer_operation(operation: &OutboxOperation) -> Option<OutboxOperation> {
    let active = operation.payload.get("active_entry")?;
    let entity_id = active.get("id")?.as_str()?.to_string();
    Some(OutboxOperation {
        operation_id: operation.operation_id.clone(),
        workspace_epoch: operation.workspace_epoch,
        operation_type: "timer_transition_start".to_string(),
        entity_type: "time_entry".to_string(),
        entity_id: Some(entity_id),
        base_version: None,
        payload_version: active.get("version").and_then(Value::as_i64),
        coalesced_count: 1,
        depends_on_operation_id: operation.depends_on_operation_id.clone(),
        payload: active.clone(),
    })
}

fn rollback_superseded_timer_transition_task(
    database: &Database,
    operation: &OutboxOperation,
) -> Result<(), String> {
    if operation
        .payload
        .get("reopen_task_base_version")
        .is_some_and(|value| !value.is_null())
    {
        return Ok(());
    }
    let Some(task_id) = operation
        .payload
        .get("task")
        .filter(|value| !value.is_null())
        .and_then(|task| task.get("id"))
        .and_then(Value::as_str)
    else {
        return Ok(());
    };
    let mut connection = database.open()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    transaction
        .execute("DELETE FROM task_status_events WHERE task_id=?1", [task_id])
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "DELETE FROM tasks WHERE id=?1 AND source_type='timer_quick_create'",
            [task_id],
        )
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())
}

fn reconcile_timer_transition_conflict(
    database: &Database,
    state: &crate::supabase::StorageModeSnapshot,
    operation: &OutboxOperation,
) -> Result<TimerSyncRecovery, String> {
    let Some(timer_operation) = timer_transition_timer_operation(operation) else {
        return Ok(TimerSyncRecovery::Conflict(
            "计时切换缺少新的活动计时快照，无法自动跟随".to_string(),
        ));
    };
    let context = fetch_timer_sync_context(database, state, &timer_operation)?;
    let recovery = reconcile_timer_sync_context(database, state, &timer_operation, &context)?;
    if matches!(recovery, TimerSyncRecovery::Followed { .. }) {
        rollback_superseded_timer_transition_task(database, operation)?;
        delete_outbox_with_dependents(database, &[operation.operation_id.clone()])?;
    }
    Ok(recovery)
}

fn emit_timer_fact_followed(operation_type: &str, entity_id: &str, version: i64) {
    if let Some(app) = CLOUD_SYNC_APP_HANDLE.get() {
        let _ = app.emit(
            "timer-fact-followed",
            json!({
                "operationType": operation_type,
                "entityId": entity_id,
                "version": version
            }),
        );
    }
}

fn is_timer_start_operation(operation_type: &str) -> bool {
    matches!(operation_type, "timer_start" | "timer_transition_start")
}

fn timer_fact_can_follow(operation: &OutboxOperation, authoritative: &Value) -> bool {
    if authoritative.get("source_type").and_then(Value::as_str) != Some("timer") {
        return false;
    }
    let state = authoritative.get("state").and_then(Value::as_str);
    match operation.operation_type.as_str() {
        "timer_start" | "timer_transition_start" => {
            matches!(state, Some("running" | "paused" | "ended"))
        }
        "timer_pause" => matches!(state, Some("paused" | "ended")),
        "timer_resume" => matches!(state, Some("running" | "ended")),
        "timer_stop"
        | "timer_transition_stop"
        | "timer_continuity_close"
        | "timer_calendar_split"
        | "time_allocation_replace" => state == Some("ended"),
        _ => false,
    }
}

fn follow_authoritative_timer_fact(
    database: &Database,
    state: &crate::supabase::StorageModeSnapshot,
    operation: &OutboxOperation,
    authoritative: &Value,
) -> Result<TimerSyncRecovery, String> {
    let authoritative_id = authoritative
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "SYNC_ERROR: 权威计时结果缺少 ID".to_string())?;
    let version = authoritative
        .get("version")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    adopt_authoritative_calendar_timer_result(
        database,
        operation,
        &json!({ "superseded": true, "authoritative": authoritative }),
    )?;
    if is_timer_start_operation(&operation.operation_type) {
        if operation.entity_id.as_deref() == Some(authoritative_id) {
            delete_outbox_with_dependents(database, &[operation.operation_id.clone()])?;
        } else {
            delete_outbox_entity_chain(database, &state.workspace_id, operation)?;
        }
    } else {
        delete_outbox_entity_chain(database, &state.workspace_id, operation)?;
    }
    Ok(TimerSyncRecovery::Followed {
        entity_id: authoritative_id.to_string(),
        version,
    })
}

fn reconcile_timer_sync_context(
    database: &Database,
    state: &crate::supabase::StorageModeSnapshot,
    operation: &OutboxOperation,
    context: &Value,
) -> Result<TimerSyncRecovery, String> {
    let processed = context
        .get("processedOperation")
        .filter(|value| !value.is_null());
    if processed
        .and_then(|value| value.get("matchesRequest"))
        .and_then(Value::as_bool)
        == Some(true)
    {
        if let Some(result) = processed.and_then(|value| value.get("result")) {
            adopt_authoritative_calendar_timer_result(database, operation, result)?;
        }
        return Ok(TimerSyncRecovery::Acknowledged);
    }

    let Some(entity_id) = operation.entity_id.as_deref() else {
        return Ok(TimerSyncRecovery::Conflict(
            "计时同步操作缺少实体标识".to_string(),
        ));
    };
    let authoritative = context
        .get("authoritative")
        .filter(|value| !value.is_null());
    let cloud_version = context
        .get("currentVersion")
        .and_then(Value::as_i64)
        .or_else(|| authoritative?.get("version")?.as_i64());
    let Some(cloud_version) = cloud_version else {
        return Ok(TimerSyncRecovery::Conflict(
            "云端计时记录已不存在，无法自动覆盖".to_string(),
        ));
    };
    let Some(authoritative) = authoritative else {
        return Ok(TimerSyncRecovery::Conflict(
            "云端没有返回完整权威计时快照，无法证明自动恢复安全".to_string(),
        ));
    };
    if timer_fact_can_follow(operation, authoritative) {
        let authoritative_device = authoritative
            .get("updated_by_device_id")
            .and_then(Value::as_str);
        let same_device = authoritative_device == Some(state.device_id.as_str());
        let payload_matches = timer_business_images_equal(authoritative, &operation.payload);
        if !same_device || !payload_matches {
            return follow_authoritative_timer_fact(database, state, operation, authoritative);
        }
    }
    let chain = load_timer_entity_chain(database, &state.workspace_id, entity_id)?;
    let Some(latest) = chain.last() else {
        return Ok(TimerSyncRecovery::Acknowledged);
    };
    let latest_version = latest
        .payload_version
        .or_else(|| latest.payload.get("version").and_then(Value::as_i64))
        .unwrap_or_default();
    if cloud_version == latest_version {
        if timer_business_images_equal(authoritative, &latest.payload) {
            adopt_authoritative_timer_image(database, operation, authoritative)?;
            return Ok(TimerSyncRecovery::Acknowledged);
        }
        return Ok(TimerSyncRecovery::Conflict(format!(
            "云端与本机同为版本 {cloud_version}，但计时内容不同"
        )));
    }
    if cloud_version > latest_version {
        if timer_business_images_equal(authoritative, &latest.payload) {
            adopt_authoritative_timer_image(database, operation, authoritative)?;
            return Ok(TimerSyncRecovery::Acknowledged);
        }
        return Ok(TimerSyncRecovery::Conflict(format!(
            "云端计时已推进到版本 {cloud_version}，超过本机最终版本 {latest_version}"
        )));
    }
    if authoritative
        .get("updated_by_device_id")
        .and_then(Value::as_str)
        .is_some_and(|device_id| device_id != state.device_id)
    {
        return Ok(TimerSyncRecovery::Conflict(
            "云端计时包含另一台设备的修改，不能自动覆盖".to_string(),
        ));
    }
    if !timer_chain_can_rebase(database, &state.workspace_id, &chain)? {
        return Ok(TimerSyncRecovery::Conflict(
            "计时链包含已经发送的跨实体副作用，不能自动压缩".to_string(),
        ));
    }
    rebase_timer_entity_chain(
        database,
        &state.workspace_id,
        entity_id,
        cloud_version,
        Some(authoritative),
        &chain,
    )?;
    Ok(TimerSyncRecovery::Rebased)
}

fn load_timer_entity_chain(
    database: &Database,
    workspace_id: &str,
    entity_id: &str,
) -> Result<Vec<OutboxOperation>, String> {
    let connection = database.open()?;
    let mut statement = connection
        .prepare(
            "SELECT operation_id,workspace_epoch,operation_type,entity_type,entity_id,base_version,payload_version,coalesced_count,depends_on_operation_id,payload_json
             FROM sync_outbox WHERE workspace_id=?1 AND entity_type='time_entry' AND entity_id=?2
             ORDER BY created_at,rowid",
        )
        .map_err(|error| error.to_string())?;
    let result = statement
        .query_map(params![workspace_id, entity_id], |row| {
            let payload: String = row.get(9)?;
            Ok(OutboxOperation {
                operation_id: row.get(0)?,
                workspace_epoch: row.get(1)?,
                operation_type: row.get(2)?,
                entity_type: row.get(3)?,
                entity_id: row.get(4)?,
                base_version: row.get(5)?,
                payload_version: row.get(6)?,
                coalesced_count: row.get(7)?,
                depends_on_operation_id: row.get(8)?,
                payload: serde_json::from_str(&payload).unwrap_or(Value::Null),
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(result)
}

fn timer_chain_can_rebase(
    database: &Database,
    workspace_id: &str,
    chain: &[OutboxOperation],
) -> Result<bool, String> {
    let operation_ids = chain
        .iter()
        .map(|operation| operation.operation_id.as_str())
        .collect::<HashSet<_>>();
    let connection = database.open()?;
    let mut statement = connection
        .prepare(
            "SELECT depends_on_operation_id,state,attempt_count FROM sync_outbox
             WHERE workspace_id=?1 AND entity_type<>'time_entry' AND depends_on_operation_id IS NOT NULL",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([workspace_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(|error| error.to_string())?;
    for row in rows {
        let (dependency, state, attempt_count) = row.map_err(|error| error.to_string())?;
        if operation_ids.contains(dependency.as_str()) && (state != "pending" || attempt_count != 0)
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn rebase_timer_entity_chain(
    database: &Database,
    workspace_id: &str,
    entity_id: &str,
    cloud_version: i64,
    authoritative: Option<&Value>,
    chain: &[OutboxOperation],
) -> Result<(), String> {
    let first = chain
        .first()
        .ok_or_else(|| "SYNC_ERROR: 计时同步链为空".to_string())?;
    let latest = chain
        .last()
        .ok_or_else(|| "SYNC_ERROR: 计时同步链为空".to_string())?;
    let next_version = cloud_version.saturating_add(1);
    let mut payload = latest.payload.clone();
    if let Some(object) = payload.as_object_mut() {
        object.insert("version".to_string(), json!(next_version));
        object.insert("updated_at".to_string(), json!(now_millis()));
    }
    let replacement_id = stable_timer_operation_id(
        entity_id,
        Some(cloud_version),
        next_version,
        &latest.operation_type,
    );
    let chain_ids = chain
        .iter()
        .map(|operation| operation.operation_id.clone())
        .collect::<Vec<_>>();
    let recovery_context = json!({
        "message": "已按云端权威版本自动压缩本机计时链",
        "fromBaseVersion": first.base_version,
        "cloudVersion": cloud_version,
        "localFinalVersion": latest.payload_version,
        "authoritative": authoritative
    });
    let mut connection = database.open()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "UPDATE sync_outbox SET operation_id=?1,operation_type=?2,base_version=?3,
             payload_json=?4,payload_version=?5,coalesced_count=1,state='pending',
             attempt_count=0,last_attempt_at=NULL,error_json=NULL,recovery_kind='rebased',
             recovery_context_json=?6,recovery_updated_at=?7
             WHERE workspace_id=?8 AND operation_id=?9",
            params![
                replacement_id,
                latest.operation_type,
                cloud_version,
                payload.to_string(),
                next_version,
                recovery_context.to_string(),
                now_millis(),
                workspace_id,
                first.operation_id
            ],
        )
        .map_err(|error| error.to_string())?;
    for operation_id in chain_ids.iter().skip(1) {
        transaction
            .execute(
                "UPDATE sync_outbox SET depends_on_operation_id=?1 WHERE depends_on_operation_id=?2",
                params![replacement_id, operation_id],
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "DELETE FROM sync_outbox WHERE operation_id=?1",
                [operation_id],
            )
            .map_err(|error| error.to_string())?;
    }
    transaction
        .execute(
            "UPDATE sync_outbox SET depends_on_operation_id=?1 WHERE depends_on_operation_id=?2",
            params![replacement_id, first.operation_id],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "UPDATE time_entries SET version=?1,updated_at=?2 WHERE id=?3",
            params![next_version, now_millis(), entity_id],
        )
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())
}

fn timer_business_images_equal(left: &Value, right: &Value) -> bool {
    let keys = [
        "id",
        "work_date",
        "kind",
        "source_type",
        "state",
        "default_task_id",
        "label_snapshot",
        "started_at",
        "ended_at",
        "duration_seconds",
        "note",
        "origin_unassigned_session_id",
        "timer_chain_id",
        "previous_entry_id",
        "split_boundary_at",
        "last_continuous_at",
        "segments",
        "allocations",
    ];
    keys.iter().all(|key| left.get(*key) == right.get(*key))
}

fn mark_outbox_recovery_error(
    database: &Database,
    operation_id: &str,
    state: &str,
    recovery_kind: &str,
    message: &str,
    context: Option<&Value>,
) -> Result<(), String> {
    let recovery_context = json!({
        "message": message,
        "context": context
    });
    let error_json = (state == "conflict").then(|| json!({ "message": message }).to_string());
    database
        .open()?
        .execute(
            "UPDATE sync_outbox
             SET state=?1,last_attempt_at=?2,error_json=?3,recovery_kind=?4,
                 recovery_context_json=?5,recovery_updated_at=?2
             WHERE operation_id=?6",
            params![
                state,
                now_millis(),
                error_json,
                recovery_kind,
                recovery_context.to_string(),
                operation_id
            ],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn adopt_authoritative_report_template_result(
    database: &Database,
    operation: &OutboxOperation,
    value: &Value,
) -> Result<(), String> {
    let Some(authoritative) = value.get("authoritative").filter(|row| !row.is_null()) else {
        return Ok(());
    };
    let template_id = authoritative
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "SYNC_ERROR: 权威报告模板缺少 ID".to_string())?;
    if operation.entity_id.as_deref() != Some(template_id) {
        return Err("SYNC_ERROR: 权威报告模板与待同步实体不一致".to_string());
    }
    let incoming_version = authoritative
        .get("version")
        .and_then(Value::as_i64)
        .ok_or_else(|| "SYNC_ERROR: 权威报告模板缺少版本".to_string())?;
    let local_workspace_id = local_cache_workspace_id(database)?;
    let mut connection = database.open()?;
    let current_version: Option<i64> = connection
        .query_row(
            "SELECT version FROM report_templates WHERE workspace_id=?1 AND id=?2",
            params![local_workspace_id, template_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if current_version.is_some_and(|version| version > incoming_version) {
        return Ok(());
    }
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    apply_incremental_entity(
        &transaction,
        &local_workspace_id,
        &IncrementalEntity {
            entity_type: "report_templates".to_string(),
            source_entity_type: "report_templates".to_string(),
            entity_id: template_id.to_string(),
            change_seq: 0,
            deleted: false,
            data: Some(authoritative.clone()),
        },
    )?;
    transaction
        .execute(
            "UPDATE app_metadata SET value=CAST(CAST(value AS INTEGER)+1 AS TEXT) WHERE key='global_revision'",
            [],
        )
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())
}

fn adopt_timer_transition_occurrence_result(
    database: &Database,
    operation: &OutboxOperation,
    value: &Value,
) -> Result<(), String> {
    let Some(authoritative) = value.get("occurrence").filter(|row| !row.is_null()) else {
        return Ok(());
    };
    let expected = operation
        .payload
        .get("occurrence")
        .filter(|row| !row.is_null())
        .ok_or_else(|| "SYNC_ERROR: 计时转换返回了未请求的周期轮次".to_string())?;
    let task_id = authoritative
        .get("task_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "SYNC_ERROR: 权威周期轮次缺少事项 ID".to_string())?;
    let occurrence_date = authoritative
        .get("occurrence_date")
        .and_then(Value::as_str)
        .ok_or_else(|| "SYNC_ERROR: 权威周期轮次缺少日期".to_string())?;
    if expected.get("task_id").and_then(Value::as_str) != Some(task_id)
        || expected.get("occurrence_date").and_then(Value::as_str) != Some(occurrence_date)
    {
        return Err("SYNC_ERROR: 权威周期轮次与计时转换目标不一致".to_string());
    }
    let incoming_version = authoritative
        .get("version")
        .and_then(Value::as_i64)
        .ok_or_else(|| "SYNC_ERROR: 权威周期轮次缺少版本".to_string())?;
    let local_workspace_id = local_cache_workspace_id(database)?;
    let mut connection = database.open()?;
    let current_version: Option<i64> = connection
        .query_row(
            "SELECT version FROM task_occurrences WHERE workspace_id=?1 AND task_id=?2 AND occurrence_date=?3",
            params![local_workspace_id, task_id, occurrence_date],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if current_version.is_some_and(|version| version > incoming_version) {
        return Ok(());
    }
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    apply_incremental_entity(
        &transaction,
        &local_workspace_id,
        &IncrementalEntity {
            entity_type: "task_occurrences".to_string(),
            source_entity_type: "task_occurrences".to_string(),
            entity_id: format!("{task_id}|{occurrence_date}"),
            change_seq: 0,
            deleted: false,
            data: Some(authoritative.clone()),
        },
    )?;
    transaction
        .execute(
            "UPDATE app_metadata SET value=CAST(CAST(value AS INTEGER)+1 AS TEXT) WHERE key='global_revision'",
            [],
        )
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())
}

fn adopt_authoritative_calendar_timer_result(
    database: &Database,
    operation: &OutboxOperation,
    value: &Value,
) -> Result<(), String> {
    let Some(candidate_id) = operation.entity_id.as_deref() else {
        return Ok(());
    };
    let authoritative = if value
        .get("superseded")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        value.get("authoritative")
    } else {
        value.get("authoritative")
    };
    let Some(authoritative) = authoritative.filter(|row| !row.is_null()) else {
        return Ok(());
    };
    let authoritative_id = authoritative
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "SYNC_ERROR: 权威计时切片缺少 ID".to_string())?;
    if authoritative_id == candidate_id {
        return adopt_authoritative_timer_image(database, operation, authoritative);
    }
    let mut connection = database.open()?;
    let local_workspace_id: String = connection
        .query_row(
            "SELECT id FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "UPDATE time_entries SET timer_chain_id=NULL, deleted_at=COALESCE(deleted_at,updated_at) WHERE workspace_id=?1 AND id=?2",
            params![local_workspace_id, candidate_id],
        )
        .map_err(|error| error.to_string())?;
    apply_incremental_time_entry(&transaction, &local_workspace_id, authoritative)
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "UPDATE time_allocations SET entry_id=?1 WHERE workspace_id=?2 AND entry_id=?3",
            params![authoritative_id, local_workspace_id, candidate_id],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "UPDATE time_entries SET previous_entry_id=?1 WHERE workspace_id=?2 AND previous_entry_id=?3",
            params![authoritative_id, local_workspace_id, candidate_id],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "UPDATE sync_outbox SET entity_id=?1 WHERE entity_id=?2 AND operation_id<>?3",
            params![authoritative_id, candidate_id, operation.operation_id],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "DELETE FROM time_segments WHERE workspace_id=?1 AND entry_id=?2",
            params![local_workspace_id, candidate_id],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "DELETE FROM time_entries WHERE workspace_id=?1 AND id=?2",
            params![local_workspace_id, candidate_id],
        )
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())
}

fn adopt_authoritative_timer_image(
    database: &Database,
    operation: &OutboxOperation,
    authoritative: &Value,
) -> Result<(), String> {
    let candidate_id = operation
        .entity_id
        .as_deref()
        .ok_or_else(|| "SYNC_ERROR: 权威计时结果缺少本地实体 ID".to_string())?;
    let authoritative_id = authoritative
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "SYNC_ERROR: 权威计时结果缺少 ID".to_string())?;
    if candidate_id != authoritative_id {
        return Ok(());
    }
    let mut connection = database.open()?;
    let local_workspace_id: String = connection
        .query_row(
            "SELECT id FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let cloud_workspace_id = storage_mode(database)?.workspace_id;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    apply_incremental_time_entry(&transaction, &local_workspace_id, authoritative)
        .map_err(|error| error.to_string())?;
    let authoritative_version = authoritative.get("version").and_then(Value::as_i64);
    transaction
        .execute(
            "UPDATE sync_outbox SET base_version=?1 WHERE workspace_id=?2
             AND entity_type='time_entry' AND entity_id=?3 AND operation_id<>?4
             AND base_version IS NOT NULL AND base_version<?1",
            params![
                authoritative_version,
                cloud_workspace_id,
                candidate_id,
                operation.operation_id
            ],
        )
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())
}

fn adopt_expired_lease_takeover(database: &Database, lease: &Value) -> Result<(), String> {
    let workspace_epoch = storage_mode(database)?.workspace_epoch;
    if lease.get("takeover").and_then(Value::as_bool) != Some(true) {
        return Ok(());
    }
    let Some(authoritative) = lease
        .get("authoritativeTimer")
        .filter(|value| !value.is_null())
    else {
        return Ok(());
    };
    let entity_id = authoritative
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "SYNC_ERROR: 接管计时快照缺少 ID".to_string())?;
    let operation = OutboxOperation {
        operation_id: stable_timer_operation_id(
            entity_id,
            authoritative.get("version").and_then(Value::as_i64),
            authoritative
                .get("version")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            "timer_takeover_recover",
        ),
        workspace_epoch,
        operation_type: "timer_takeover_recover".to_string(),
        entity_type: "time_entry".to_string(),
        entity_id: Some(entity_id.to_string()),
        base_version: authoritative.get("version").and_then(Value::as_i64),
        payload_version: authoritative.get("version").and_then(Value::as_i64),
        coalesced_count: 1,
        depends_on_operation_id: None,
        payload: authoritative.clone(),
    };
    adopt_authoritative_timer_image(database, &operation, authoritative)?;
    crate::time_tracking::coordinate_active_timer(database, now_millis())
}

fn is_shared_unassigned_lifecycle_operation(operation: &OutboxOperation) -> bool {
    operation.entity_type == "unassigned_session"
        && matches!(
            operation.operation_type.as_str(),
            "unassigned_session_pause"
                | "unassigned_session_resume"
                | "unassigned_session_awaiting_resolution"
        )
}

fn adopt_superseded_shared_unassigned_lifecycle_result(
    database: &Database,
    operation: &OutboxOperation,
    value: &Value,
) -> Result<bool, String> {
    if !is_shared_unassigned_lifecycle_operation(operation)
        || !value
            .get("superseded")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        return Ok(false);
    }
    let session = value
        .get("session")
        .ok_or_else(|| "VALIDATION_ERROR: 共享未归属生命周期让步结果缺少会话".to_string())?;
    adopt_shared_unassigned_rpc_result(database, session)?;
    queue_next_shared_unassigned_candidate(database, session)?;
    Ok(true)
}

fn adopt_shared_unassigned_rpc_result(database: &Database, value: &Value) -> Result<(), String> {
    let local_workspace_id = local_cache_workspace_id(database)?;
    let mut connection = database.open()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    apply_incremental_unassigned_session(&transaction, &local_workspace_id, value)
        .map_err(|error| error.to_string())?;
    if matches!(text(value, "state").as_str(), "resolved" | "discarded") {
        if let Some(resolved_at) = optional_millis(value, "resolved_at") {
            ensure_next_shared_unassigned_session(
                &transaction,
                &local_workspace_id,
                &text(value, "id"),
                resolved_at,
            )?;
        }
    }
    transaction
        .execute(
            "UPDATE app_metadata SET value=CAST(CAST(value AS INTEGER)+1 AS TEXT)
             WHERE key='global_revision'",
            [],
        )
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())
}

#[cfg(test)]
pub(crate) fn apply_remote_unassigned_session_for_test(
    database: &Database,
    value: &Value,
) -> Result<(), String> {
    adopt_shared_unassigned_rpc_result(database, value)
}

fn adopt_shared_unassigned_resolution_result(
    database: &Database,
    operation: &OutboxOperation,
    value: &Value,
) -> Result<(), String> {
    let accepted = value
        .get("accepted")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let session = value
        .get("session")
        .ok_or_else(|| "VALIDATION_ERROR: 共享未归属处理结果缺少会话".to_string())?;
    if let Some(entry) = value.get("generatedEntry").filter(|entry| !entry.is_null()) {
        let mut connection = database.open()?;
        let local_workspace_id: String = connection
            .query_row(
                "SELECT id FROM workspaces WHERE storage_mode='local' AND deleted_at IS NULL ORDER BY created_at LIMIT 1",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| error.to_string())?;
        apply_incremental_time_entry(&transaction, &local_workspace_id, entry)
            .map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())?;
    }
    adopt_shared_unassigned_rpc_result(database, session)?;
    if !accepted {
        rollback_losing_unassigned_resolution(database, operation)?;
    }
    if let Some(next_session) = value
        .get("nextSession")
        .filter(|session| !session.is_null())
    {
        adopt_shared_unassigned_rpc_result(database, next_session)?;
    } else {
        queue_next_shared_unassigned_candidate(database, session)?;
    }
    Ok(())
}

fn is_missing_shared_unassigned_session_error(error: &str) -> bool {
    error
        .to_ascii_lowercase()
        .contains("not_found: shared unassigned session")
}

fn recover_orphaned_shared_unassigned_discard(
    database: &Database,
    operation: &OutboxOperation,
    authoritative: &Value,
) -> Result<(), String> {
    if operation.operation_type != "unassigned_discard" {
        return Err("VALIDATION_ERROR: 只能自动收敛丢弃未归属时间操作".to_string());
    }
    if !authoritative.is_null() {
        let authoritative_id = authoritative
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| "VALIDATION_ERROR: 权威未归属会话缺少 ID".to_string())?;
        if operation.entity_id.as_deref() == Some(authoritative_id) {
            return Err("SYNC_ERROR: 云端重新采用了孤儿未归属会话，需保留原操作重试".to_string());
        }
        adopt_shared_unassigned_rpc_result(database, authoritative)?;
    }
    rollback_losing_unassigned_resolution(database, operation)?;
    if let Some(session_id) = operation.entity_id.as_deref() {
        database
            .open()?
            .execute(
                "UPDATE unassigned_sessions
                 SET migration_state='superseded'
                 WHERE id=?1 AND resolution_operation_id=?2",
                params![session_id, operation.operation_id],
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn rollback_losing_unassigned_resolution(
    database: &Database,
    operation: &OutboxOperation,
) -> Result<(), String> {
    let mut connection = database.open()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let mut pending = vec![operation.operation_id.clone()];
    let mut dependent_ids = HashSet::<String>::new();
    while let Some(parent_id) = pending.pop() {
        let children = transaction
            .prepare("SELECT operation_id FROM sync_outbox WHERE depends_on_operation_id=?1")
            .and_then(|mut statement| {
                statement
                    .query_map([parent_id.as_str()], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()
            })
            .map_err(|error| error.to_string())?;
        for child_id in children {
            if dependent_ids.insert(child_id.clone()) {
                pending.push(child_id);
            }
        }
    }
    let mut dependent_operations = Vec::<OutboxOperation>::new();
    for dependent_id in &dependent_ids {
        let dependent = transaction
            .query_row(
                "SELECT operation_id,workspace_epoch,operation_type,entity_type,entity_id,base_version,payload_version,coalesced_count,depends_on_operation_id,payload_json
                 FROM sync_outbox WHERE operation_id=?1",
                [dependent_id],
                |row| {
                    let payload: String = row.get(9)?;
                    Ok(OutboxOperation {
                        operation_id: row.get(0)?,
                        workspace_epoch: row.get(1)?,
                        operation_type: row.get(2)?,
                        entity_type: row.get(3)?,
                        entity_id: row.get(4)?,
                        base_version: row.get(5)?,
                        payload_version: row.get(6)?,
                        coalesced_count: row.get(7)?,
                        depends_on_operation_id: row.get(8)?,
                        payload: serde_json::from_str(&payload).unwrap_or(Value::Null),
                    })
                },
            )
            .map_err(|error| error.to_string())?;
        dependent_operations.push(dependent);
    }
    for dependent in &dependent_operations {
        rollback_losing_unassigned_dependent(&transaction, dependent)?;
    }
    for dependent_id in dependent_ids {
        transaction
            .execute(
                "DELETE FROM sync_outbox WHERE operation_id=?1",
                [dependent_id],
            )
            .map_err(|error| error.to_string())?;
    }
    if let Some(entry_id) = operation
        .payload
        .get("generated_entry_id")
        .and_then(Value::as_str)
    {
        if let Some(session_id) = operation.entity_id.as_deref() {
            transaction
                .execute(
                    "UPDATE unassigned_sessions SET generated_entry_id=NULL
                     WHERE id=?1 AND generated_entry_id=?2",
                    params![session_id, entry_id],
                )
                .map_err(|error| error.to_string())?;
        }
        transaction
            .execute("DELETE FROM time_allocations WHERE entry_id=?1", [entry_id])
            .map_err(|error| error.to_string())?;
        transaction
            .execute("DELETE FROM time_segments WHERE entry_id=?1", [entry_id])
            .map_err(|error| error.to_string())?;
        transaction
            .execute("DELETE FROM time_entries WHERE id=?1", [entry_id])
            .map_err(|error| error.to_string())?;
    }
    transaction.commit().map_err(|error| error.to_string())
}

fn rollback_losing_unassigned_dependent(
    transaction: &Transaction<'_>,
    operation: &OutboxOperation,
) -> Result<(), String> {
    let Some(entity_id) = operation.entity_id.as_deref() else {
        return Ok(());
    };
    match operation.operation_type.as_str() {
        "task_set_completed_from_unassigned" => {
            let Some(base_version) = operation.base_version else {
                return Ok(());
            };
            let payload_version = operation
                .payload
                .get("version")
                .and_then(Value::as_i64)
                .unwrap_or(base_version + 1);
            let completed_at = optional_millis(&operation.payload, "completed_at");
            transaction
                .execute(
                    "UPDATE tasks SET status='open',completed_at=NULL,version=?1
                     WHERE id=?2 AND status='done' AND version=?3",
                    params![base_version, entity_id, payload_version],
                )
                .map_err(|error| error.to_string())?;
            if let Some(completed_at) = completed_at {
                transaction
                    .execute(
                        "DELETE FROM task_status_events
                         WHERE task_id=?1 AND status='done' AND occurred_at=?2 AND source_type='user'",
                        params![entity_id, completed_at],
                    )
                    .map_err(|error| error.to_string())?;
            }
        }
        "task_occurrence_set_completed_from_unassigned" => {
            let Some((task_id, occurrence_date)) = entity_id.split_once('|') else {
                return Ok(());
            };
            if let Some(base_version) = operation.base_version {
                let payload_version = operation
                    .payload
                    .get("version")
                    .and_then(Value::as_i64)
                    .unwrap_or(base_version + 1);
                transaction
                    .execute(
                        "UPDATE task_occurrences SET status='open',completed_at=NULL,version=?1
                         WHERE task_id=?2 AND occurrence_date=?3 AND status='done' AND version=?4",
                        params![base_version, task_id, occurrence_date, payload_version],
                    )
                    .map_err(|error| error.to_string())?;
            } else {
                transaction
                    .execute(
                        "DELETE FROM task_occurrences
                         WHERE task_id=?1 AND occurrence_date=?2 AND status='done'",
                        params![task_id, occurrence_date],
                    )
                    .map_err(|error| error.to_string())?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn list_conflicts(database: &Database) -> Result<Vec<CloudSyncConflict>, String> {
    let state = storage_mode(database)?;
    if state.mode != "cloud" {
        return Ok(Vec::new());
    }
    let mut conflicts = list_local_conflicts(database, &state.workspace_id)?;
    if !conflicts.is_empty() {
        match load_cloud_snapshot(database, &state.workspace_id) {
            Ok(snapshot) => {
                for conflict in &mut conflicts {
                    conflict.cloud_payload = cloud_conflict_payload(
                        &snapshot,
                        &conflict.entity_type,
                        conflict.entity_id.as_deref(),
                        &conflict.local_payload,
                    );
                }
            }
            Err(error) => {
                for conflict in &mut conflicts {
                    conflict.cloud_payload_error = Some(error.clone());
                }
            }
        }
    }
    Ok(conflicts)
}

fn list_local_conflicts(
    database: &Database,
    workspace_id: &str,
) -> Result<Vec<CloudSyncConflict>, String> {
    let connection = database.open()?;
    let mut statement = connection
        .prepare(
            "SELECT operation_id, operation_type, entity_type, entity_id, base_version, payload_json,
                    error_json, attempt_count, created_at, last_attempt_at,recovery_context_json
             FROM sync_outbox
             WHERE workspace_id = ?1 AND state = 'conflict'
             ORDER BY created_at, operation_id",
        )
        .map_err(|error| error.to_string())?;
    let conflicts = statement
        .query_map([workspace_id], |row| {
            let payload: String = row.get(5)?;
            let error_json: Option<String> = row.get(6)?;
            let recovery_context_json: Option<String> = row.get(10)?;
            let recovery_context = recovery_context_json
                .as_deref()
                .and_then(|value| serde_json::from_str::<Value>(value).ok());
            let error = error_json.and_then(|value| {
                serde_json::from_str::<Value>(&value)
                    .ok()
                    .and_then(|json| {
                        json.get("message")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned)
                    })
                    .or(Some(value))
            });
            Ok(CloudSyncConflict {
                operation_id: row.get(0)?,
                operation_type: row.get(1)?,
                entity_type: row.get(2)?,
                entity_id: row.get(3)?,
                base_version: row.get(4)?,
                local_payload: serde_json::from_str(&payload).unwrap_or(Value::Null),
                cloud_payload: recovery_context
                    .as_ref()
                    .and_then(|value| value.pointer("/context/authoritative"))
                    .filter(|value| !value.is_null())
                    .cloned(),
                cloud_payload_error: None,
                error,
                conflict_reason: recovery_context.as_ref().and_then(|value| {
                    value
                        .get("message")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                }),
                attempt_count: row.get(7)?,
                created_at: row.get(8)?,
                last_attempt_at: row.get(9)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(conflicts)
}

fn load_cloud_snapshot(database: &Database, workspace_id: &str) -> Result<Value, String> {
    let session = current_session(database)?;
    client(database)?.rpc_for_database(
        database,
        "cloud_snapshot_get",
        json!({ "p_workspace_id": workspace_id }),
        &session,
    )
}

pub fn resolve_conflict(
    database: &Database,
    request: CloudConflictResolveRequest,
) -> Result<CloudSyncStatus, String> {
    let state = require_cloud(database)?;
    let conflict = load_conflict(database, &state.workspace_id, &request.operation_id)?;
    resolve_loaded_conflict(database, &state, &conflict, &request.strategy, None)?;
    if request.strategy == "keep_local"
        && list_local_conflicts(database, &state.workspace_id)?.is_empty()
    {
        return finish_keep_local_conflict_resolution(database);
    }
    signal_cloud_sync(SYNC_SIGNAL_MANUAL_RETRY | SYNC_SIGNAL_REMOTE_HINT, None);
    sync_status(database)
}

pub fn resolve_all_conflicts(
    database: &Database,
    request: CloudConflictResolveAllRequest,
) -> Result<CloudSyncStatus, String> {
    let state = require_cloud(database)?;
    validate_conflict_strategy(&request.strategy)?;
    let mut attempts_by_operation = HashMap::<String, usize>::new();

    loop {
        let resolved = resolve_current_conflicts(
            database,
            &state,
            &request.strategy,
            &mut attempts_by_operation,
        )?;
        if resolved == 0 {
            break;
        }
    }
    if request.strategy == "keep_local" {
        finish_keep_local_conflict_resolution(database)
    } else {
        signal_cloud_sync(SYNC_SIGNAL_MANUAL_RETRY | SYNC_SIGNAL_REMOTE_HINT, None);
        sync_status(database)
    }
}

fn finish_keep_local_conflict_resolution(database: &Database) -> Result<CloudSyncStatus, String> {
    finish_keep_local_conflict_resolution_with(
        database,
        |database| push_outbox_with_retry_mode(database, true, true),
        |database| catch_up_incremental(database).map(|_| ()),
    )
}

fn finish_keep_local_conflict_resolution_with(
    database: &Database,
    push: impl FnOnce(&Database) -> Result<CloudSyncPushResult, String>,
    pull: impl FnOnce(&Database) -> Result<(), String>,
) -> Result<CloudSyncStatus, String> {
    let pushed = push(database).map_err(|error| {
        format!("KEEP_LOCAL_OVERWRITE_FAILED: 本地版本已重新排队，但覆盖云端失败：{error}")
    })?;
    let status = sync_status(database)?;
    if pushed.conflicts > 0 || status.conflict_count > 0 {
        return Err("KEEP_LOCAL_OVERWRITE_CONFLICT: 云端数据在处理期间再次变化，本地版本尚未完成覆盖，请重新确认冲突".to_string());
    }
    if pushed.pending > 0 || status.pending_operations > 0 {
        return Err(format!(
            "KEEP_LOCAL_OVERWRITE_PENDING: 本地版本尚未全部覆盖云端，仍有 {} 项等待同步",
            pushed.pending.max(status.pending_operations)
        ));
    }
    pull(database).map_err(|error| {
        format!("KEEP_LOCAL_REFRESH_FAILED: 本地版本已覆盖云端，但刷新云端新版本失败：{error}")
    })?;
    let status = sync_status(database)?;
    if status.conflict_count > 0 || status.pending_operations > 0 {
        return Err(
            "KEEP_LOCAL_NOT_CONVERGED: 本地版本覆盖后同步状态尚未收敛，请重新同步".to_string(),
        );
    }
    signal_cloud_sync(SYNC_SIGNAL_REMOTE_HINT, None);
    Ok(status)
}

fn resolve_current_conflicts(
    database: &Database,
    state: &crate::supabase::StorageModeSnapshot,
    strategy: &str,
    attempts_by_operation: &mut HashMap<String, usize>,
) -> Result<usize, String> {
    let conflicts = list_local_conflicts(database, &state.workspace_id)?;
    if conflicts.is_empty() {
        return Ok(0);
    }
    let cloud_snapshot = if strategy == "keep_local" {
        let session = current_session(database)?;
        Some(client(database)?.rpc_for_database(
            database,
            "cloud_snapshot_get",
            json!({ "p_workspace_id": state.workspace_id }),
            &session,
        )?)
    } else {
        None
    };

    for listed_conflict in &conflicts {
        let attempts = attempts_by_operation
            .entry(listed_conflict.operation_id.clone())
            .or_default();
        *attempts += 1;
        if *attempts > 3 {
            return Err(
                "SYNC_CONFLICT_REPEATED: 云端数据仍在持续变化，已停止批量处理以避免覆盖新的远端修改"
                    .to_string(),
            );
        }
        let conflict =
            match load_conflict(database, &state.workspace_id, &listed_conflict.operation_id) {
                Ok(conflict) => conflict,
                Err(error) if error.starts_with("NOT_FOUND:") => continue,
                Err(error) => return Err(error),
            };
        resolve_loaded_conflict(
            database,
            state,
            &conflict,
            strategy,
            cloud_snapshot.as_ref(),
        )?;
    }
    Ok(conflicts.len())
}

fn validate_conflict_strategy(strategy: &str) -> Result<(), String> {
    if matches!(strategy, "use_cloud" | "keep_local") {
        Ok(())
    } else {
        Err("VALIDATION_ERROR: 冲突处理策略只能是 use_cloud 或 keep_local".to_string())
    }
}

fn resolve_loaded_conflict(
    database: &Database,
    state: &crate::supabase::StorageModeSnapshot,
    conflict: &OutboxOperation,
    strategy: &str,
    cloud_snapshot: Option<&Value>,
) -> Result<(), String> {
    match strategy {
        "use_cloud" => {
            if conflict.entity_type == "actual_adjustment_batch" {
                rollback_actual_adjustment_batch_local_images(database, conflict)?;
            }
            delete_outbox_entity_chain(database, &state.workspace_id, &conflict)?;
            mark_conflict_entity_for_cloud_reconciliation(database, state, conflict)?;
        }
        "keep_local" => {
            if conflict.entity_type == "actual_adjustment_batch" {
                return Err(
                    "ADJUSTMENT_BASELINE_CONFLICT: 云端调整基线已经变化，不能直接覆盖；请使用云端并重新打开调整面板"
                        .to_string(),
                );
            }
            let latest_local =
                latest_outbox_entity_operation(database, &state.workspace_id, &conflict)?;
            let fetched_snapshot;
            let snapshot = match cloud_snapshot {
                Some(snapshot) => snapshot,
                None => {
                    let session = current_session(database)?;
                    fetched_snapshot = client(database)?.rpc_for_database(
                        database,
                        "cloud_snapshot_get",
                        json!({ "p_workspace_id": state.workspace_id }),
                        &session,
                    )?;
                    &fetched_snapshot
                }
            };
            let cloud_version = cloud_entity_version(
                snapshot,
                &latest_local.entity_type,
                latest_local.entity_id.as_deref(),
                &latest_local.payload,
            );
            let local_delete_of_absent_daily_estimate = latest_local.entity_type
                == "task_daily_estimate"
                && latest_local.payload.get("deleted").and_then(Value::as_bool) == Some(true)
                && cloud_version.is_none();
            if cloud_version.is_none() && latest_local.entity_type == "task_occurrence" {
                return rebase_missing_task_occurrence(database, &state.workspace_id, conflict);
            }
            if cloud_version.is_none()
                && conflict.base_version.is_some()
                && !local_delete_of_absent_daily_estimate
                && latest_local.entity_type != "task_recurrence_rule"
            {
                return Err(
                    "SYNC_CONFLICT_DELETED: 云端实体已被删除，不能用本地更新覆盖；请选择使用云端，或将本地内容复制为新事项"
                        .to_string(),
                );
            }
            let (rebased_version, payload) = if latest_local.entity_type == "task_recurrence_rule" {
                rebase_recurrence_payload(snapshot, &latest_local.payload)?
            } else {
                let mut payload = latest_local.payload.clone();
                let next_version = cloud_version.map_or(1, |version| version + 1);
                if let Some(object) = payload.as_object_mut() {
                    object.insert("version".to_string(), json!(next_version));
                    if object.contains_key("updated_at") {
                        object.insert("updated_at".to_string(), json!(now_millis()));
                    }
                }
                (cloud_version, payload)
            };
            database
                .open()?
                .execute(
                    "UPDATE sync_outbox
                     SET operation_type = ?1, base_version = ?2, payload_json = ?3,
                         payload_version = ?4, coalesced_count = 1,
                         state = 'pending', error_json = NULL
                     WHERE workspace_id = ?5 AND operation_id = ?6 AND state = 'conflict'",
                    params![
                        latest_local.operation_type,
                        rebased_version,
                        payload.to_string(),
                        payload.get("version").and_then(Value::as_i64),
                        state.workspace_id,
                        conflict.operation_id
                    ],
                )
                .map_err(|error| error.to_string())?;
            delete_following_outbox_entity_operations(
                database,
                &state.workspace_id,
                &latest_local,
                &conflict.operation_id,
            )?;
        }
        _ => return validate_conflict_strategy(strategy),
    }
    Ok(())
}

fn is_missing_task_occurrence_conflict(error: &str) -> bool {
    error.contains("SYNC_CONFLICT")
        && error.contains("expected version")
        && (error.contains("current version <NULL>") || error.contains("current version null"))
}

fn recover_historical_missing_task_occurrences(
    database: &Database,
    workspace_id: &str,
) -> Result<usize, String> {
    let connection = database.open()?;
    let mut statement = connection
        .prepare(
            "SELECT operation_id,workspace_epoch,operation_type,entity_type,entity_id,base_version,payload_version,coalesced_count,depends_on_operation_id,payload_json
             FROM sync_outbox
             WHERE workspace_id=?1 AND entity_type='task_occurrence' AND state='conflict'
               AND error_json LIKE '%SYNC_CONFLICT%' AND error_json LIKE '%expected version%'
               AND (error_json LIKE '%current version <NULL>%' OR error_json LIKE '%current version null%')
             ORDER BY created_at,rowid",
        )
        .map_err(|error| error.to_string())?;
    let operations = statement
        .query_map([workspace_id], |row| {
            let payload: String = row.get(9)?;
            Ok(OutboxOperation {
                operation_id: row.get(0)?,
                workspace_epoch: row.get(1)?,
                operation_type: row.get(2)?,
                entity_type: row.get(3)?,
                entity_id: row.get(4)?,
                base_version: row.get(5)?,
                payload_version: row.get(6)?,
                coalesced_count: row.get(7)?,
                depends_on_operation_id: row.get(8)?,
                payload: serde_json::from_str(&payload).unwrap_or(Value::Null),
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(statement);
    drop(connection);
    let mut recovered = 0;
    let mut entities = HashSet::new();
    for operation in operations {
        let Some(entity_id) = operation.entity_id.as_deref() else {
            continue;
        };
        if entities.insert(entity_id.to_string()) {
            rebase_missing_task_occurrence(database, workspace_id, &operation)?;
            recovered += 1;
        }
    }
    Ok(recovered)
}

fn rebase_missing_task_occurrence(
    database: &Database,
    workspace_id: &str,
    operation: &OutboxOperation,
) -> Result<(), String> {
    if operation.entity_type != "task_occurrence" {
        return Err("VALIDATION_ERROR: 只有周期事项轮次可以从缺失云端实体重基".to_string());
    }
    let entity_id = operation
        .entity_id
        .as_deref()
        .ok_or_else(|| "VALIDATION_ERROR: 周期事项轮次同步缺少实体 ID".to_string())?;
    let (task_id, occurrence_date) = entity_id
        .split_once('|')
        .ok_or_else(|| "VALIDATION_ERROR: 周期事项轮次同步实体 ID 无效".to_string())?;
    let latest = latest_outbox_entity_operation(database, workspace_id, operation)?;
    let mut payload = latest.payload.clone();
    if payload.get("task_id").and_then(Value::as_str) != Some(task_id)
        || payload.get("occurrence_date").and_then(Value::as_str) != Some(occurrence_date)
    {
        return Err("VALIDATION_ERROR: 周期事项轮次载荷身份不一致".to_string());
    }
    let now = now_millis();
    if let Some(object) = payload.as_object_mut() {
        object.insert("version".to_string(), json!(1));
        if object.contains_key("updated_at") {
            object.insert("updated_at".to_string(), json!(now));
        }
    }
    let recovery_context = json!({
        "message": "云端缺少周期事项轮次，已将本机最终状态重基为创建",
        "previousBaseVersion": operation.base_version,
        "localFinalVersion": latest.payload_version
    });
    let mut connection = database.open()?;
    let local_workspace_id = local_cache_workspace_id(database)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "UPDATE sync_outbox SET operation_type=?1,base_version=NULL,payload_json=?2,payload_version=1,
             coalesced_count=1,state='pending',attempt_count=0,last_attempt_at=NULL,error_json=NULL,
             recovery_kind='rebased_missing_occurrence',recovery_context_json=?3,recovery_updated_at=?4
             WHERE workspace_id=?5 AND operation_id=?6",
            params![
                latest.operation_type,
                payload.to_string(),
                recovery_context.to_string(),
                now,
                workspace_id,
                operation.operation_id
            ],
        )
        .map_err(|error| error.to_string())?;
    let mut statement = transaction
        .prepare(
            "SELECT operation_id FROM sync_outbox WHERE workspace_id=?1 AND entity_type='task_occurrence'
             AND entity_id=?2 AND operation_id<>?3
             AND rowid>=(SELECT rowid FROM sync_outbox WHERE operation_id=?3)",
        )
        .map_err(|error| error.to_string())?;
    let following = statement
        .query_map(
            params![workspace_id, entity_id, operation.operation_id],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(statement);
    for following_id in following {
        transaction
            .execute(
                "UPDATE sync_outbox SET depends_on_operation_id=?1 WHERE depends_on_operation_id=?2",
                params![operation.operation_id, following_id],
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "DELETE FROM sync_outbox WHERE operation_id=?1",
                [&following_id],
            )
            .map_err(|error| error.to_string())?;
    }
    transaction
        .execute(
            "UPDATE task_occurrences SET version=1,updated_at=?1 WHERE workspace_id=?2 AND task_id=?3 AND occurrence_date=?4",
            params![now, local_workspace_id, task_id, occurrence_date],
        )
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())
}

fn rollback_actual_adjustment_batch_local_images(
    database: &Database,
    conflict: &OutboxOperation,
) -> Result<(), String> {
    let created_task_ids = conflict
        .payload
        .get("createdTasks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|wrapper| wrapper.get("task"))
        .filter_map(|task| task.get("id"))
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    let adjustment_ids = conflict
        .payload
        .get("adjustments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|adjustment| adjustment.get("id"))
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    let mut connection = database.open()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    for adjustment_id in adjustment_ids {
        transaction
            .execute(
                "DELETE FROM actual_time_adjustments WHERE id=?1",
                [adjustment_id],
            )
            .map_err(|error| error.to_string())?;
    }
    for task_id in created_task_ids {
        transaction
            .execute("DELETE FROM tasks WHERE id=?1", [task_id])
            .map_err(|error| {
                format!(
                    "SYNC_CONFLICT_REQUIRES_MANUAL_REPAIR: 云端拒绝的批次事项已被本地其他记录引用，无法自动撤销：{error}"
                )
            })?;
    }
    transaction
        .execute(
            "DELETE FROM actual_adjustment_batches WHERE client_request_id=?1",
            [conflict.operation_id.as_str()],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "UPDATE app_metadata SET value=CAST(CAST(value AS INTEGER)+1 AS TEXT) WHERE key='global_revision'",
            [],
        )
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())
}

fn adopt_actual_adjustment_batch_result(
    database: &Database,
    operation: &OutboxOperation,
    value: &Value,
) -> Result<(), String> {
    let authoritative = value
        .get("adjustments")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let discarded = value
        .get("discardedAdjustmentIds")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect::<HashSet<_>>();
    if authoritative.is_empty() && discarded.is_empty() {
        return Ok(());
    }
    let optimistic_ids = operation
        .payload
        .get("adjustments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|adjustment| adjustment.get("id"))
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect::<HashSet<_>>();
    let mut connection = database.open()?;
    let local_workspace_id: String = connection
        .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    for id in discarded {
        if optimistic_ids.contains(&id) {
            transaction
                .execute("DELETE FROM actual_time_adjustments WHERE id=?1", [id])
                .map_err(|error| error.to_string())?;
        }
    }
    for adjustment in authoritative {
        let id = text(&adjustment, "id");
        if id.is_empty() || !optimistic_ids.contains(&id) {
            continue;
        }
        transaction
            .execute(
                "UPDATE actual_time_adjustments
                 SET delta_minutes=?1,baseline_minutes=?2,target_minutes=?3,note=?4,
                     updated_at=?5,version=?6,updated_by_device_id=?7,deleted_at=?8
                 WHERE workspace_id=?9 AND id=?10",
                params![
                    integer(&adjustment, "delta_minutes"),
                    integer(&adjustment, "baseline_minutes"),
                    integer(&adjustment, "target_minutes"),
                    optional_text(&adjustment, "note"),
                    millis(&adjustment, "updated_at"),
                    integer(&adjustment, "version"),
                    optional_text(&adjustment, "updated_by_device_id"),
                    optional_millis(&adjustment, "deleted_at"),
                    local_workspace_id,
                    id,
                ],
            )
            .map_err(|error| error.to_string())?;
    }
    transaction
        .execute(
            "UPDATE app_metadata SET value=CAST(CAST(value AS INTEGER)+1 AS TEXT) WHERE key='global_revision'",
            [],
        )
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())
}

fn incremental_entity_type_for_outbox(entity_type: &str) -> Option<&'static str> {
    match entity_type {
        "subject" => Some("subjects"),
        "task" => Some("tasks"),
        "task_occurrence" => Some("task_occurrences"),
        "task_day_plan" => Some("task_day_plans"),
        "task_daily_estimate" => Some("task_daily_estimates"),
        "task_recurrence_rule" => Some("task_recurrence_rules"),
        "time_entry" => Some("time_entries"),
        "actual_time_adjustment" => Some("actual_time_adjustments"),
        "report" => Some("reports"),
        "report_template" => Some("report_templates"),
        "app_setting" => Some("app_settings"),
        "integration_config" => Some("integration_configs"),
        "external_binding" => Some("external_bindings"),
        "unassigned_session" => Some("unassigned_sessions"),
        _ => None,
    }
}

fn mark_conflict_entity_for_cloud_reconciliation(
    database: &Database,
    state: &crate::supabase::StorageModeSnapshot,
    conflict: &OutboxOperation,
) -> Result<(), String> {
    let Some(entity_type) = incremental_entity_type_for_outbox(&conflict.entity_type) else {
        return Ok(());
    };
    let Some(entity_id) = conflict.entity_id.as_deref() else {
        return Ok(());
    };
    database
        .open()?
        .execute(
            "INSERT INTO cloud_deferred_entities(
               workspace_id,entity_type,entity_id,observed_version,observed_change_seq,updated_at
             ) VALUES (?1,?2,?3,?4,?5,?6)
             ON CONFLICT(workspace_id,entity_type,entity_id) DO UPDATE SET
               observed_version=MAX(observed_version,excluded.observed_version),
               observed_change_seq=MIN(observed_change_seq,excluded.observed_change_seq),
               updated_at=excluded.updated_at",
            params![
                state.workspace_id,
                entity_type,
                entity_id,
                conflict.base_version.unwrap_or_default(),
                state.last_change_seq.saturating_add(1),
                now_millis(),
            ],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn latest_outbox_entity_operation(
    database: &Database,
    workspace_id: &str,
    conflict: &OutboxOperation,
) -> Result<OutboxOperation, String> {
    let Some(entity_id) = conflict.entity_id.as_deref() else {
        return Ok(conflict.clone());
    };
    database
        .open()?
        .query_row(
            "SELECT operation_id, workspace_epoch, operation_type, entity_type, entity_id, base_version, payload_version, coalesced_count, depends_on_operation_id, payload_json
             FROM sync_outbox
             WHERE workspace_id = ?1
               AND entity_type = ?2
               AND entity_id = ?3
               AND rowid >= (SELECT rowid FROM sync_outbox WHERE operation_id = ?4)
             ORDER BY rowid DESC
             LIMIT 1",
            params![
                workspace_id,
                conflict.entity_type,
                entity_id,
                conflict.operation_id
            ],
            |row| {
                let payload: String = row.get(9)?;
                Ok(OutboxOperation {
                    operation_id: row.get(0)?,
                    workspace_epoch: row.get(1)?,
                    operation_type: row.get(2)?,
                    entity_type: row.get(3)?,
                    entity_id: row.get(4)?,
                    base_version: row.get(5)?,
                    payload_version: row.get(6)?,
                    coalesced_count: row.get(7)?,
                    depends_on_operation_id: row.get(8)?,
                    payload: serde_json::from_str(&payload).unwrap_or(Value::Null),
                })
            },
        )
        .map_err(|error| error.to_string())
}

fn delete_following_outbox_entity_operations(
    database: &Database,
    workspace_id: &str,
    operation: &OutboxOperation,
    keep_operation_id: &str,
) -> Result<(), String> {
    let Some(entity_id) = operation.entity_id.as_deref() else {
        return Ok(());
    };
    let mut connection = database.open()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let mut statement = transaction
        .prepare(
            "SELECT operation_id FROM sync_outbox
             WHERE workspace_id = ?1
               AND entity_type = ?2
               AND entity_id = ?3
               AND operation_id <> ?4
               AND rowid >= (SELECT rowid FROM sync_outbox WHERE operation_id = ?4)",
        )
        .map_err(|error| error.to_string())?;
    let following_ids = statement
        .query_map(
            params![
                workspace_id,
                operation.entity_type,
                entity_id,
                keep_operation_id
            ],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(statement);
    for operation_id in following_ids {
        transaction
            .execute(
                "UPDATE sync_outbox SET depends_on_operation_id = ?1 WHERE depends_on_operation_id = ?2",
                params![keep_operation_id, operation_id],
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "DELETE FROM sync_outbox WHERE operation_id = ?1",
                [operation_id],
            )
            .map_err(|error| error.to_string())?;
    }
    transaction.commit().map_err(|error| error.to_string())
}

fn delete_outbox_entity_chain(
    database: &Database,
    workspace_id: &str,
    conflict: &OutboxOperation,
) -> Result<(), String> {
    let Some(entity_id) = conflict.entity_id.as_deref() else {
        return delete_outbox_with_dependents(database, &[conflict.operation_id.clone()]);
    };
    let connection = database.open()?;
    let mut statement = connection
        .prepare(
            "SELECT operation_id FROM sync_outbox
             WHERE workspace_id = ?1
               AND entity_type = ?2
               AND entity_id = ?3
               AND rowid >= (SELECT rowid FROM sync_outbox WHERE operation_id = ?4)",
        )
        .map_err(|error| error.to_string())?;
    let operation_ids = statement
        .query_map(
            params![
                workspace_id,
                conflict.entity_type,
                entity_id,
                conflict.operation_id
            ],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(statement);
    drop(connection);
    delete_outbox_with_dependents(database, &operation_ids)
}

fn delete_outbox_with_dependents(
    database: &Database,
    operation_ids: &[String],
) -> Result<(), String> {
    let mut connection = database.open()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let mut pending = operation_ids.to_vec();
    let mut all_ids = HashSet::<String>::new();
    while let Some(operation_id) = pending.pop() {
        if !all_ids.insert(operation_id.clone()) {
            continue;
        }
        let mut statement = transaction
            .prepare("SELECT operation_id FROM sync_outbox WHERE depends_on_operation_id = ?1")
            .map_err(|error| error.to_string())?;
        let dependents = statement
            .query_map([operation_id.as_str()], |row| row.get::<_, String>(0))
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        drop(statement);
        pending.extend(dependents);
    }
    for operation_id in all_ids {
        transaction
            .execute(
                "DELETE FROM sync_outbox WHERE operation_id = ?1",
                [operation_id],
            )
            .map_err(|error| error.to_string())?;
    }
    transaction.commit().map_err(|error| error.to_string())
}

fn load_conflict(
    database: &Database,
    workspace_id: &str,
    operation_id: &str,
) -> Result<OutboxOperation, String> {
    database
        .open()?
        .query_row(
            "SELECT operation_id, workspace_epoch, operation_type, entity_type, entity_id, base_version, payload_version, coalesced_count, depends_on_operation_id, payload_json
             FROM sync_outbox
             WHERE workspace_id = ?1 AND operation_id = ?2 AND state = 'conflict'",
            params![workspace_id, operation_id],
            |row| {
                let payload: String = row.get(9)?;
                Ok(OutboxOperation {
                    operation_id: row.get(0)?,
                    workspace_epoch: row.get(1)?,
                    operation_type: row.get(2)?,
                    entity_type: row.get(3)?,
                    entity_id: row.get(4)?,
                    base_version: row.get(5)?,
                    payload_version: row.get(6)?,
                    coalesced_count: row.get(7)?,
                    depends_on_operation_id: row.get(8)?,
                    payload: serde_json::from_str(&payload).unwrap_or(Value::Null),
                })
            },
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "NOT_FOUND: 待处理的同步冲突不存在".to_string())
}

fn cloud_entity_version(
    snapshot: &Value,
    entity_type: &str,
    entity_id: Option<&str>,
    payload: &Value,
) -> Option<i64> {
    cloud_entity_payload(snapshot, entity_type, entity_id, payload)?
        .get("version")
        .and_then(Value::as_i64)
}

fn cloud_conflict_payload(
    snapshot: &Value,
    entity_type: &str,
    entity_id: Option<&str>,
    payload: &Value,
) -> Option<Value> {
    if entity_type != "task_recurrence_rule" {
        return cloud_entity_payload(snapshot, entity_type, entity_id, payload).cloned();
    }
    let task_id = payload.get("task_id")?.as_str()?;
    let task = snapshot
        .get("tasks")?
        .as_array()?
        .iter()
        .find(|task| task.get("id").and_then(Value::as_str) == Some(task_id))?;
    let rules = snapshot
        .get("task_recurrence_rules")?
        .as_array()?
        .iter()
        .filter(|rule| rule.get("task_id").and_then(Value::as_str) == Some(task_id))
        .cloned()
        .collect::<Vec<_>>();
    let version = rules
        .iter()
        .find(|rule| rule.get("effective_end").is_none_or(Value::is_null))
        .or_else(|| {
            rules.iter().max_by_key(|rule| {
                rule.get("version")
                    .and_then(Value::as_i64)
                    .unwrap_or_default()
            })
        })
        .and_then(|rule| rule.get("version"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    Some(json!({
        "task_id": task_id,
        "task_version": task.get("version").and_then(Value::as_i64),
        "rules": rules,
        "version": version
    }))
}

fn rebase_recurrence_payload(
    snapshot: &Value,
    local_payload: &Value,
) -> Result<(Option<i64>, Value), String> {
    let task_id = local_payload
        .get("task_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "VALIDATION_ERROR: 重复规则冲突缺少事项 ID".to_string())?;
    let task_version = snapshot
        .get("tasks")
        .and_then(Value::as_array)
        .and_then(|tasks| {
            tasks
                .iter()
                .find(|task| task.get("id").and_then(Value::as_str) == Some(task_id))
        })
        .and_then(|task| task.get("version"))
        .and_then(Value::as_i64)
        .ok_or_else(|| {
            "SYNC_CONFLICT_DELETED: 云端事项已被删除，不能保留本地重复规则".to_string()
        })?;
    let mut cloud_rules = snapshot
        .get("task_recurrence_rules")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|rule| rule.get("task_id").and_then(Value::as_str) == Some(task_id))
        .cloned()
        .collect::<Vec<_>>();
    let active_index = cloud_rules
        .iter()
        .position(|rule| rule.get("effective_end").is_none_or(Value::is_null));
    let cloud_rule_version =
        active_index.and_then(|index| cloud_rules[index].get("version").and_then(Value::as_i64));
    let action = local_payload
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| "VALIDATION_ERROR: 重复规则冲突缺少动作".to_string())?;
    let now = now_millis();
    let mut payload = local_payload.clone();
    let object = payload
        .as_object_mut()
        .ok_or_else(|| "VALIDATION_ERROR: 重复规则冲突载荷无效".to_string())?;
    object.insert("task_expected_version".to_string(), json!(task_version));
    object.insert("task_version".to_string(), json!(task_version + 1));
    object.insert(
        "rule_expected_version".to_string(),
        json!(cloud_rule_version),
    );

    match action {
        "save" => {
            let frequency = local_payload
                .get("frequency")
                .and_then(Value::as_str)
                .ok_or_else(|| "VALIDATION_ERROR: 重复规则冲突缺少频率".to_string())?;
            let effective_start = local_payload
                .get("effective_start")
                .and_then(Value::as_str)
                .ok_or_else(|| "VALIDATION_ERROR: 重复规则冲突缺少开始日期".to_string())?;
            let desired_start = chrono::NaiveDate::parse_from_str(effective_start, "%Y-%m-%d")
                .map_err(|_| "VALIDATION_ERROR: 重复规则冲突开始日期无效".to_string())?;
            let weekdays_mask = local_payload
                .get("weekdays_mask")
                .cloned()
                .unwrap_or(Value::Null);
            let local_target = local_payload
                .get("rules")
                .and_then(Value::as_array)
                .and_then(|rules| {
                    let rule_id = local_payload.get("rule_id").and_then(Value::as_str)?;
                    rules
                        .iter()
                        .find(|rule| rule.get("id").and_then(Value::as_str) == Some(rule_id))
                })
                .cloned()
                .ok_or_else(|| "VALIDATION_ERROR: 重复规则冲突缺少目标规则".to_string())?;
            let (rule_id, next_rule_version) = if let Some(index) = active_index {
                let active_start = cloud_rules[index]
                    .get("effective_start")
                    .and_then(Value::as_str)
                    .and_then(|value| chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
                    .ok_or_else(|| "CACHE_ERROR: 云端重复规则开始日期无效".to_string())?;
                if active_start == desired_start || active_start > desired_start {
                    let id = cloud_rules[index]
                        .get("id")
                        .and_then(Value::as_str)
                        .ok_or_else(|| "CACHE_ERROR: 云端重复规则缺少 ID".to_string())?
                        .to_string();
                    let version = cloud_rule_version.unwrap_or(0) + 1;
                    let rule = cloud_rules[index]
                        .as_object_mut()
                        .ok_or_else(|| "CACHE_ERROR: 云端重复规则格式无效".to_string())?;
                    rule.insert("frequency".to_string(), json!(frequency));
                    rule.insert("weekdays_mask".to_string(), weekdays_mask.clone());
                    rule.insert("effective_start".to_string(), json!(effective_start));
                    rule.insert("effective_end".to_string(), Value::Null);
                    rule.insert("updated_at".to_string(), json!(now));
                    rule.insert("version".to_string(), json!(version));
                    (id, version)
                } else {
                    let previous_day = desired_start
                        .pred_opt()
                        .ok_or_else(|| "VALIDATION_ERROR: 重复规则开始日期无法回退".to_string())?
                        .format("%Y-%m-%d")
                        .to_string();
                    let active = cloud_rules[index]
                        .as_object_mut()
                        .ok_or_else(|| "CACHE_ERROR: 云端重复规则格式无效".to_string())?;
                    active.insert("effective_end".to_string(), json!(previous_day));
                    active.insert("updated_at".to_string(), json!(now));
                    active.insert(
                        "version".to_string(),
                        json!(cloud_rule_version.unwrap_or(0) + 1),
                    );
                    let id = Uuid::now_v7().to_string();
                    let mut target = local_target;
                    let target_object = target
                        .as_object_mut()
                        .ok_or_else(|| "VALIDATION_ERROR: 重复规则冲突目标无效".to_string())?;
                    target_object.insert("id".to_string(), json!(id));
                    target_object.insert("task_id".to_string(), json!(task_id));
                    target_object.insert("frequency".to_string(), json!(frequency));
                    target_object.insert("weekdays_mask".to_string(), weekdays_mask.clone());
                    target_object.insert("effective_start".to_string(), json!(effective_start));
                    target_object.insert("effective_end".to_string(), Value::Null);
                    target_object.insert("created_at".to_string(), json!(now));
                    target_object.insert("updated_at".to_string(), json!(now));
                    target_object.insert("version".to_string(), json!(1));
                    cloud_rules.push(target);
                    (id, 1)
                }
            } else {
                let id = Uuid::now_v7().to_string();
                let mut target = local_target;
                let target_object = target
                    .as_object_mut()
                    .ok_or_else(|| "VALIDATION_ERROR: 重复规则冲突目标无效".to_string())?;
                target_object.insert("id".to_string(), json!(id));
                target_object.insert("task_id".to_string(), json!(task_id));
                target_object.insert("frequency".to_string(), json!(frequency));
                target_object.insert("weekdays_mask".to_string(), weekdays_mask.clone());
                target_object.insert("effective_start".to_string(), json!(effective_start));
                target_object.insert("effective_end".to_string(), Value::Null);
                target_object.insert("created_at".to_string(), json!(now));
                target_object.insert("updated_at".to_string(), json!(now));
                target_object.insert("version".to_string(), json!(1));
                cloud_rules.push(target);
                (id, 1)
            };
            object.insert("rule_id".to_string(), json!(rule_id));
            object.insert("version".to_string(), json!(next_rule_version));
        }
        "close" => {
            let index = active_index.ok_or_else(|| {
                "SYNC_CONFLICT_DELETED: 云端重复规则已关闭，无需再次保留本地关闭操作".to_string()
            })?;
            let effective_end = local_payload
                .get("effective_end")
                .and_then(Value::as_str)
                .ok_or_else(|| "VALIDATION_ERROR: 重复规则关闭冲突缺少结束日期".to_string())?;
            let end = chrono::NaiveDate::parse_from_str(effective_end, "%Y-%m-%d")
                .map_err(|_| "VALIDATION_ERROR: 重复规则冲突结束日期无效".to_string())?;
            let start = cloud_rules[index]
                .get("effective_start")
                .and_then(Value::as_str)
                .and_then(|value| chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
                .ok_or_else(|| "CACHE_ERROR: 云端重复规则开始日期无效".to_string())?;
            let rule_id = cloud_rules[index]
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| "CACHE_ERROR: 云端重复规则缺少 ID".to_string())?
                .to_string();
            let next_rule_version = cloud_rule_version.unwrap_or(0) + 1;
            if end < start {
                cloud_rules.remove(index);
            } else {
                let rule = cloud_rules[index]
                    .as_object_mut()
                    .ok_or_else(|| "CACHE_ERROR: 云端重复规则格式无效".to_string())?;
                rule.insert("effective_end".to_string(), json!(effective_end));
                rule.insert("updated_at".to_string(), json!(now));
                rule.insert("version".to_string(), json!(next_rule_version));
            }
            object.insert("rule_id".to_string(), json!(rule_id));
            object.insert("version".to_string(), json!(next_rule_version));
        }
        _ => return Err("VALIDATION_ERROR: 重复规则冲突动作无效".to_string()),
    }
    object.insert("rules".to_string(), Value::Array(cloud_rules));
    Ok((cloud_rule_version, payload))
}

fn cloud_entity_payload<'a>(
    snapshot: &'a Value,
    entity_type: &str,
    entity_id: Option<&str>,
    payload: &Value,
) -> Option<&'a Value> {
    let (table, key, expected) = match entity_type {
        "subject" => ("subjects", "id", entity_id?),
        "subject_work_schedule" => ("subject_work_schedules", "id", entity_id?),
        "task" => ("tasks", "id", entity_id?),
        "time_entry" => ("time_entries", "id", entity_id?),
        "actual_time_adjustment" => ("actual_time_adjustments", "id", entity_id?),
        "unassigned_session" => ("unassigned_sessions", "id", entity_id?),
        "report" => ("reports", "id", entity_id?),
        "report_template" => ("report_templates", "id", entity_id?),
        "app_setting" => ("app_settings", "key", payload.get("key")?.as_str()?),
        "integration_config" => (
            "integration_configs",
            "provider",
            payload.get("provider")?.as_str()?,
        ),
        "task_occurrence" => {
            let expected_task_id = payload.get("task_id")?.as_str()?;
            let expected_date = payload.get("occurrence_date")?.as_str()?;
            return snapshot
                .get("task_occurrences")?
                .as_array()?
                .iter()
                .find(|row| {
                    row.get("task_id").and_then(Value::as_str) == Some(expected_task_id)
                        && row.get("occurrence_date").and_then(Value::as_str) == Some(expected_date)
                });
        }
        "task_daily_estimate" => {
            let expected_task_id = payload.get("task_id")?.as_str()?;
            let expected_date = payload.get("work_date")?.as_str()?;
            return snapshot
                .get("task_daily_estimates")?
                .as_array()?
                .iter()
                .find(|row| {
                    row.get("task_id").and_then(Value::as_str) == Some(expected_task_id)
                        && row.get("work_date").and_then(Value::as_str) == Some(expected_date)
                });
        }
        "task_day_plan" => {
            let expected_task_id = payload.get("task_id")?.as_str()?;
            let expected_date = payload.get("work_date")?.as_str()?;
            return snapshot
                .get("task_day_plans")?
                .as_array()?
                .iter()
                .find(|row| {
                    row.get("task_id").and_then(Value::as_str) == Some(expected_task_id)
                        && row.get("work_date").and_then(Value::as_str) == Some(expected_date)
                });
        }
        "task_recurrence_rule" => {
            let expected_task_id = payload.get("task_id")?.as_str()?;
            let rules = snapshot.get("task_recurrence_rules")?.as_array()?.iter();
            return rules
                .clone()
                .find(|row| {
                    row.get("task_id").and_then(Value::as_str) == Some(expected_task_id)
                        && row.get("effective_end").is_none_or(Value::is_null)
                })
                .or_else(|| {
                    rules
                        .filter(|row| {
                            row.get("task_id").and_then(Value::as_str) == Some(expected_task_id)
                        })
                        .max_by_key(|row| {
                            row.get("version")
                                .and_then(Value::as_i64)
                                .unwrap_or_default()
                        })
                });
        }
        _ => return None,
    };
    snapshot
        .get(table)?
        .as_array()?
        .iter()
        .find(|row| row.get(key).and_then(Value::as_str) == Some(expected))
}

pub fn pull_snapshot(database: &Database) -> Result<i64, String> {
    let state = storage_mode(database)?;
    if state.mode != "cloud" {
        return Err("OFFLINE_RESTRICTED: 当前不是 Supabase 云端模式".to_string());
    }
    ensure_no_dirty_outbox(database, &state.workspace_id)?;
    pull_snapshot_for_epoch(database, state.workspace_epoch, false)
}

const LOCAL_CACHE_RESET_ENTITIES: &[(&str, &str)] = &[
    ("subjects", "主题"),
    ("tasks", "事项"),
    ("task_day_plans", "今日计划"),
    ("task_daily_estimates", "每日估时"),
    ("task_recurrence_rules", "重复规则"),
    ("time_entries", "时间记录"),
    ("actual_time_adjustments", "实际用时调整"),
    ("unassigned_sessions", "未归属时间"),
    ("reports", "报告"),
];

pub fn preview_local_cache_reset(database: &Database) -> Result<LocalCacheResetPreview, String> {
    let _operation_guard = cloud_sync_operation_guard();
    let state = storage_mode(database)?;
    if state.mode != "cloud" {
        return Err("OFFLINE_RESTRICTED: 当前不是 Supabase 云端模式".to_string());
    }
    if !state.online {
        return Err("AUTH_REQUIRED: 云端会话不可用".to_string());
    }
    let snapshot = load_cloud_snapshot(database, &state.workspace_id)?;
    let local_workspace_id = local_cache_workspace_id(database)?;
    let connection = database.open()?;
    let mut entities = Vec::with_capacity(LOCAL_CACHE_RESET_ENTITIES.len());
    let mut local_total = 0;
    let mut cloud_total = 0;
    for (table, label) in LOCAL_CACHE_RESET_ENTITIES {
        let local_count = connection
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE workspace_id=?1"),
                [&local_workspace_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let cloud_count = snapshot
            .get(*table)
            .and_then(Value::as_array)
            .map(|rows| rows.len() as i64)
            .unwrap_or_default();
        local_total += local_count;
        cloud_total += cloud_count;
        entities.push(LocalCacheResetCount {
            entity: (*label).to_string(),
            local_count,
            cloud_count,
        });
    }
    Ok(LocalCacheResetPreview {
        workspace_id: state.workspace_id,
        pending_operations: state.pending_operations,
        conflict_count: state.conflict_count,
        local_total,
        cloud_total,
        entities,
    })
}

fn pull_snapshot_for_epoch(
    database: &Database,
    expected_epoch: i64,
    discard_dirty_outbox: bool,
) -> Result<i64, String> {
    let state = storage_mode(database)?;
    if !state.online {
        return Err("AUTH_REQUIRED: 云端会话不可用".to_string());
    }
    let expected_local_revision = local_revision(database)?;
    let session = current_session(database)?;
    let snapshot = client(database)?.rpc_for_database_with_epoch(
        database,
        "cloud_snapshot_get",
        json!({ "p_workspace_id": state.workspace_id }),
        &session,
        expected_epoch,
    )?;
    let server_epoch = snapshot
        .get("workspace_epoch")
        .and_then(Value::as_i64)
        .unwrap_or(expected_epoch);
    if server_epoch != expected_epoch {
        return Err(format!(
            "WORKSPACE_EPOCH_STALE: 快照世代在下载期间从 {expected_epoch} 变为 {server_epoch}"
        ));
    }
    let latest_change_seq = snapshot
        .get("latest_change_seq")
        .and_then(Value::as_i64)
        .unwrap_or(state.last_change_seq);
    apply_snapshot_with_reset_options(
        database,
        &snapshot,
        Some(expected_local_revision),
        Some(&state.workspace_id),
        Some((
            &state.workspace_id,
            &state.device_id,
            latest_change_seq,
            server_epoch,
        )),
        false,
        discard_dirty_outbox,
    )?;
    Ok(latest_change_seq)
}

pub fn reset_local_cache_from_cloud(database: &Database) -> Result<CloudSyncStatus, String> {
    let _operation_guard = cloud_sync_operation_guard();
    let state = storage_mode(database)?;
    if state.mode != "cloud" {
        return Err("OFFLINE_RESTRICTED: 当前不是 Supabase 云端模式".to_string());
    }
    if !state.online {
        return Err("AUTH_REQUIRED: 云端会话不可用".to_string());
    }
    let expected_local_revision = local_revision(database)?;
    let session = current_session(database)?;
    let snapshot = client(database)?.rpc_for_database_with_epoch(
        database,
        "cloud_snapshot_get",
        json!({ "p_workspace_id": state.workspace_id }),
        &session,
        state.workspace_epoch,
    )?;
    let latest_change_seq = snapshot
        .get("latest_change_seq")
        .and_then(Value::as_i64)
        .unwrap_or(state.last_change_seq);
    apply_snapshot_with_reset_options(
        database,
        &snapshot,
        Some(expected_local_revision),
        Some(&state.workspace_id),
        Some((
            &state.workspace_id,
            &state.device_id,
            latest_change_seq,
            state.workspace_epoch,
        )),
        false,
        true,
    )?;
    finish_cache_rebuild_sync_locked(database, "重置本地数据")
}

/// Rebuilds the restoring device from a newly committed workspace epoch. This
/// differs from the ordinary cache reset: the local state still contains the
/// previous epoch immediately after the restore RPC succeeds, so the server
/// epoch supplied by the restore result is authoritative for this one download.
pub(crate) fn rebuild_local_cache_after_workspace_restore_locked(
    database: &Database,
    restored_epoch: i64,
    reset_change_seq: i64,
) -> Result<CloudSyncStatus, String> {
    let state = storage_mode(database)?;
    if state.mode != "cloud" {
        return Err("OFFLINE_RESTRICTED: 当前不是 Supabase 云端模式".to_string());
    }
    if !state.online {
        return Err("AUTH_REQUIRED: 云端会话不可用".to_string());
    }
    if restored_epoch <= state.workspace_epoch {
        return Err("WORKSPACE_EPOCH_INVALID: 恢复结果没有返回新的工作空间世代".to_string());
    }
    let expected_local_revision = local_revision(database)?;
    let session = current_session(database)?;
    let snapshot = client(database)?.rpc_for_database_with_epoch(
        database,
        "cloud_snapshot_get",
        json!({ "p_workspace_id": state.workspace_id }),
        &session,
        restored_epoch,
    )?;
    let server_epoch = snapshot
        .get("workspace_epoch")
        .and_then(Value::as_i64)
        .unwrap_or(restored_epoch);
    if server_epoch != restored_epoch {
        return Err(format!(
            "WORKSPACE_EPOCH_STALE: 恢复后下载期间工作空间世代从 {restored_epoch} 变为 {server_epoch}"
        ));
    }
    let latest_change_seq = snapshot
        .get("latest_change_seq")
        .and_then(Value::as_i64)
        .unwrap_or(reset_change_seq)
        .max(reset_change_seq);
    apply_snapshot_with_reset_options(
        database,
        &snapshot,
        Some(expected_local_revision),
        Some(&state.workspace_id),
        Some((
            &state.workspace_id,
            &state.device_id,
            latest_change_seq,
            restored_epoch,
        )),
        false,
        true,
    )?;
    finish_cache_rebuild_sync_locked(database, "云端恢复后的本地重建")
}

pub fn discard_old_epoch_changes_and_redownload(
    database: &Database,
) -> Result<CloudSyncStatus, String> {
    let _operation_guard = cloud_sync_operation_guard();
    let state = storage_mode(database)?;
    if state.mode != "cloud" || !state.epoch_blocked {
        return Err("WORKSPACE_EPOCH_NOT_BLOCKED: 当前设备没有待处理的旧世代修改".to_string());
    }
    if !state.online {
        return Err("AUTH_REQUIRED: 云端会话不可用".to_string());
    }
    let safety_directory = database.data_directory().join("backups");
    let safety_path = safety_directory.join(format!(
        "timegenie-before-epoch-reset-{}.timegenie-backup",
        now_millis()
    ));
    crate::data_protection::export_automatic_safety_backup(
        database,
        safety_path.display().to_string(),
    )?;

    let expected_local_revision = local_revision(database)?;
    let session = current_session(database)?;
    let snapshot = client(database)?.rpc_for_database_with_epoch(
        database,
        "cloud_snapshot_get",
        json!({ "p_workspace_id": state.workspace_id }),
        &session,
        state.workspace_epoch,
    )?;
    let server_epoch = snapshot
        .get("workspace_epoch")
        .and_then(Value::as_i64)
        .unwrap_or(state.workspace_epoch);
    let latest_change_seq = snapshot
        .get("latest_change_seq")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    apply_snapshot_with_reset_options(
        database,
        &snapshot,
        Some(expected_local_revision),
        Some(&state.workspace_id),
        Some((
            &state.workspace_id,
            &state.device_id,
            latest_change_seq,
            server_epoch,
        )),
        false,
        true,
    )?;
    finish_cache_rebuild_sync_locked(database, "放弃旧世代修改后的本地重建")
}

fn finish_cache_rebuild_sync_locked(
    database: &Database,
    operation_label: &str,
) -> Result<CloudSyncStatus, String> {
    finish_cache_rebuild_sync_with(database, operation_label, |database| {
        push_outbox_locked(database, true, true)
    })
}

fn finish_cache_rebuild_sync_with(
    database: &Database,
    operation_label: &str,
    push: impl FnOnce(&Database) -> Result<CloudSyncPushResult, String>,
) -> Result<CloudSyncStatus, String> {
    let before = sync_status(database)?;
    if before.pending_operations == 0 && before.conflict_count == 0 {
        return Ok(before);
    }

    let result = push(database).map_err(|error| {
        format!(
            "CACHE_REBUILD_SYNC_FAILED: {operation_label}已完成，但随后产生的共享状态未能同步：{error}"
        )
    })?;
    let status = sync_status(database)?;
    if result.conflicts > 0 || status.conflict_count > 0 {
        return Err(format!(
            "CACHE_REBUILD_SYNC_CONFLICT: {operation_label}已完成，但随后产生的共享状态遇到同步冲突"
        ));
    }
    if result.pending > 0 || status.pending_operations > 0 {
        return Err(format!(
            "CACHE_REBUILD_SYNC_PENDING: {operation_label}已完成，但仍有 {} 项共享状态等待同步",
            result.pending.max(status.pending_operations)
        ));
    }
    Ok(status)
}

pub fn acquire_lease(database: &Database) -> Result<Value, String> {
    let _operation_guard = cloud_sync_operation_guard();
    acquire_lease_locked(database)
}

fn acquire_lease_locked(database: &Database) -> Result<Value, String> {
    let state = require_cloud(database)?;
    let session = current_session(database)?;
    let api = client(database)?;
    api.rpc_for_database(
        database,
        "tracking_lease_acquire",
        json!({
            "p_workspace_id": state.workspace_id,
            "p_device_id": state.device_id,
            "p_lease_token": Uuid::now_v7().to_string()
        }),
        &session,
    )
}

pub fn renew_lease(database: &Database, request: TrackingLeaseRequest) -> Result<Value, String> {
    let _operation_guard = cloud_sync_operation_guard();
    renew_lease_locked(database, request)
}

fn renew_lease_locked(database: &Database, request: TrackingLeaseRequest) -> Result<Value, String> {
    let token = request
        .lease_token
        .ok_or_else(|| "VALIDATION_ERROR: 缺少租约令牌".to_string())?;
    let state = require_cloud(database)?;
    let session = current_session(database)?;
    client(database)?.rpc_for_database(
        database,
        "tracking_lease_renew",
        json!({ "p_workspace_id": state.workspace_id, "p_device_id": state.device_id, "p_lease_token": token }),
        &session,
    )
}

pub fn release_lease(database: &Database, request: TrackingLeaseRequest) -> Result<Value, String> {
    let _operation_guard = cloud_sync_operation_guard();
    release_lease_locked(database, request)
}

fn release_lease_locked(
    database: &Database,
    request: TrackingLeaseRequest,
) -> Result<Value, String> {
    let token = request
        .lease_token
        .ok_or_else(|| "VALIDATION_ERROR: 缺少租约令牌".to_string())?;
    let state = require_cloud(database)?;
    let session = current_session(database)?;
    client(database)?.rpc_for_database(
        database,
        "tracking_lease_release",
        json!({ "p_workspace_id": state.workspace_id, "p_device_id": state.device_id, "p_lease_token": token }),
        &session,
    )
}

pub fn get_lease(database: &Database) -> Result<Value, String> {
    let state = require_cloud(database)?;
    let session = current_session(database)?;
    let api = client(database)?;
    api
        .request_for_database(
            database,
            api.get(format!(
                "{}/rest/v1/tracking_leases?workspace_id=eq.{}&select=workspace_id,holder_device_id,lease_token,expires_at,version",
                api.project_url, state.workspace_id
            )),
            &session,
        )?
        .json()
        .map_err(|error| format!("NETWORK_ERROR: 读取采集租约失败: {error}"))
}

pub fn legacy_cloud_timer_start_local_first(
    database: &Database,
    request: CloudTimerStartRequest,
) -> Result<crate::time_tracking::TimeEntryDto, String> {
    crate::time_tracking::start_timer_with_hooks_for_sync(
        database,
        crate::time_tracking::TimerStartRequest {
            task_id: request.task_id,
            note: request.note,
            client_request_id: request.operation_id,
        },
    )
}

pub fn legacy_cloud_timer_pause_local_first(
    database: &Database,
    request: CloudTimerVersionRequest,
) -> Result<crate::time_tracking::TimeEntryDto, String> {
    crate::time_tracking::pause_timer_for_sync(
        database,
        crate::time_tracking::TimerVersionRequest {
            entry_id: request.entry_id,
            expected_version: request.expected_version,
        },
    )
}

pub fn legacy_cloud_timer_resume_local_first(
    database: &Database,
    request: CloudTimerVersionRequest,
) -> Result<crate::time_tracking::TimeEntryDto, String> {
    crate::time_tracking::resume_timer_for_sync(
        database,
        crate::time_tracking::TimerResumeRequest {
            entry_id: request.entry_id,
            expected_version: request.expected_version,
            operation_id: request.operation_id,
        },
    )
}

pub fn legacy_cloud_timer_stop_local_first(
    database: &Database,
    request: CloudTimerVersionRequest,
) -> Result<crate::time_tracking::TimeEntryDto, String> {
    crate::time_tracking::stop_timer_with_hooks_for_sync(
        database,
        crate::time_tracking::TimerStopRequest {
            entry_id: request.entry_id,
            expected_version: request.expected_version,
            create_default_allocation: false,
        },
    )
}

fn require_cloud(database: &Database) -> Result<crate::supabase::StorageModeSnapshot, String> {
    let state = storage_mode(database)?;
    if state.mode != "cloud" {
        return Err("OFFLINE_RESTRICTED: 当前不是 Supabase 云端模式".to_string());
    }
    if !state.online {
        return Err("AUTH_REQUIRED: 云端会话不可用".to_string());
    }
    Ok(state)
}

fn entity_payload(
    database: &Database,
    entity_type: &str,
    entity_id: Option<&str>,
) -> Result<Value, String> {
    let connection = database.open()?;
    entity_payload_from_connection(&connection, entity_type, entity_id)
}

pub(crate) fn entity_payload_from_connection(
    connection: &rusqlite::Connection,
    entity_type: &str,
    entity_id: Option<&str>,
) -> Result<Value, String> {
    let local_workspace_id: String = connection
        .query_row(
            "SELECT id FROM workspaces WHERE storage_mode = 'local' AND deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let (sql, identifier) = match entity_type {
        "subject" => ("SELECT * FROM subjects WHERE workspace_id = ?1 AND id = ?2", entity_id),
        "subject_work_schedule" => {
            let id = entity_id.ok_or_else(|| "VALIDATION_ERROR: 主体工作时段同步缺少实体 ID".to_string())?;
            let mut value = query_json_row(connection, "SELECT * FROM subject_work_schedules WHERE workspace_id=?1 AND id=?2", &local_workspace_id, id)?;
            let mut statement = connection.prepare("SELECT * FROM subject_work_schedule_segments WHERE workspace_id=?1 AND schedule_id=?2 ORDER BY sequence_no").map_err(|error| error.to_string())?;
            let column_names = statement.column_names().into_iter().map(ToOwned::to_owned).collect::<Vec<_>>();
            let rows = statement.query_map(params![local_workspace_id,id], |row| row_to_json(row,&column_names)).map_err(|error| error.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|error| error.to_string())?;
            value.as_object_mut().ok_or_else(|| "CACHE_ERROR: 主体工作时段缓存不是对象".to_string())?.insert("segments".to_string(), Value::Array(rows));
            return Ok(value);
        }
        "task" => ("SELECT * FROM tasks WHERE workspace_id = ?1 AND id = ?2", entity_id),
        "time_entry" => {
            let id = entity_id.ok_or_else(|| "VALIDATION_ERROR: 时间记录同步缺少实体 ID".to_string())?;
            let mut value = query_json_row(
                &connection,
                "SELECT * FROM time_entries WHERE workspace_id = ?1 AND id = ?2",
                &local_workspace_id,
                id,
            )?;
            for (key, sql) in [
                ("segments", "SELECT * FROM time_segments WHERE workspace_id = ?1 AND entry_id = ?2 ORDER BY sequence_no"),
                ("allocations", "SELECT * FROM time_allocations WHERE workspace_id = ?1 AND entry_id = ?2 ORDER BY created_at"),
            ] {
                let mut statement = connection.prepare(sql).map_err(|error| error.to_string())?;
                let column_names = statement.column_names().into_iter().map(ToOwned::to_owned).collect::<Vec<_>>();
                let rows = statement
                    .query_map(params![local_workspace_id, id], |row| row_to_json(row, &column_names))
                    .map_err(|error| error.to_string())?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| error.to_string())?;
                value.as_object_mut().ok_or_else(|| "CACHE_ERROR: 时间记录缓存不是对象".to_string())?.insert(key.to_string(), Value::Array(rows));
            }
            let work_date = value.get("work_date").and_then(Value::as_str).unwrap_or_default();
            let work_day = query_json_row(
                &connection,
                "SELECT * FROM work_days WHERE workspace_id = ?1 AND work_date = ?2",
                &local_workspace_id,
                work_date,
            ).ok();
            value.as_object_mut().ok_or_else(|| "CACHE_ERROR: 时间记录缓存不是对象".to_string())?.insert("work_day".to_string(), work_day.unwrap_or(Value::Null));
            return Ok(value);
        }
        "actual_time_adjustment" => (
            "SELECT * FROM actual_time_adjustments WHERE workspace_id = ?1 AND id = ?2",
            entity_id,
        ),
        "unassigned_session" => {
            let id = entity_id.ok_or_else(|| "VALIDATION_ERROR: 未归属会话同步缺少实体 ID".to_string())?;
            let mut value = query_json_row(
                &connection,
                "SELECT * FROM unassigned_sessions WHERE workspace_id = ?1 AND id = ?2",
                &local_workspace_id,
                id,
            )?;
            let mut statement = connection
                .prepare("SELECT * FROM unassigned_segments WHERE workspace_id = ?1 AND session_id = ?2 ORDER BY sequence_no")
                .map_err(|error| error.to_string())?;
            let column_names = statement
                .column_names()
                .into_iter()
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>();
            let rows = statement
                .query_map(params![local_workspace_id, id], |row| {
                    row_to_json(row, &column_names)
                })
                .map_err(|error| error.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?;
            value
                .as_object_mut()
                .ok_or_else(|| "CACHE_ERROR: 未归属会话缓存不是对象".to_string())?
                .insert("segments".to_string(), Value::Array(rows));
            if let Some(entry_id) = value.get("generated_entry_id").and_then(Value::as_str) {
                let mut entry = query_json_row(
                    connection,
                    "SELECT * FROM time_entries WHERE workspace_id = ?1 AND id = ?2",
                    &local_workspace_id,
                    entry_id,
                )?;
                for (key, sql) in [
                    ("segments", "SELECT * FROM time_segments WHERE workspace_id = ?1 AND entry_id = ?2 ORDER BY sequence_no"),
                    ("allocations", "SELECT * FROM time_allocations WHERE workspace_id = ?1 AND entry_id = ?2 ORDER BY created_at"),
                ] {
                    let mut statement = connection.prepare(sql).map_err(|error| error.to_string())?;
                    let column_names = statement
                        .column_names()
                        .into_iter()
                        .map(ToOwned::to_owned)
                        .collect::<Vec<_>>();
                    let rows = statement
                        .query_map(params![local_workspace_id, entry_id], |row| {
                            row_to_json(row, &column_names)
                        })
                        .map_err(|error| error.to_string())?
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|error| error.to_string())?;
                    entry
                        .as_object_mut()
                        .ok_or_else(|| "CACHE_ERROR: 未归属生成记录缓存不是对象".to_string())?
                        .insert(key.to_string(), Value::Array(rows));
                }
                value
                    .as_object_mut()
                    .expect("unassigned session payload is an object")
                    .insert("generated_entry".to_string(), entry);
            }
            return Ok(value);
        }
        "report_template" => ("SELECT * FROM report_templates WHERE workspace_id = ?1 AND id = ?2", entity_id),
        "app_setting" => ("SELECT * FROM app_settings WHERE workspace_id = ?1 AND key = ?2", entity_id),
        "integration_config" => ("SELECT workspace_id, provider, enabled, config_json, updated_at, version FROM integration_configs WHERE workspace_id = ?1 AND provider = ?2", entity_id),
        "external_binding" => {
            let key = entity_id
                .ok_or_else(|| "VALIDATION_ERROR: 外部绑定同步缺少实体 ID".to_string())?;
            let (provider, remainder) = key
                .split_once('|')
                .ok_or_else(|| "VALIDATION_ERROR: 外部绑定同步实体 ID 无效".to_string())?;
            let (entity_kind, bound_entity_id) = remainder
                .split_once('|')
                .ok_or_else(|| "VALIDATION_ERROR: 外部绑定同步实体 ID 无效".to_string())?;
            if provider.is_empty() || entity_kind.is_empty() || bound_entity_id.is_empty() {
                return Err("VALIDATION_ERROR: 外部绑定同步实体 ID 无效".to_string());
            }
            return query_json_row_with_params(
                connection,
                "SELECT * FROM external_bindings WHERE workspace_id = ?1 AND provider = ?2 AND entity_type = ?3 AND entity_id = ?4",
                params![local_workspace_id, provider, entity_kind, bound_entity_id],
                key,
            );
        }
        "task_occurrence" => {
            let key = entity_id
                .ok_or_else(|| "VALIDATION_ERROR: 重复事项轮次同步缺少实体 ID".to_string())?;
            let (task_id, occurrence_date) = key
                .split_once('|')
                .ok_or_else(|| "VALIDATION_ERROR: 重复事项轮次同步实体 ID 无效".to_string())?;
            return query_json_row_with_params(
                connection,
                "SELECT * FROM task_occurrences WHERE workspace_id = ?1 AND task_id = ?2 AND occurrence_date = ?3",
                params![local_workspace_id, task_id, occurrence_date],
                key,
            );
        }
        "task_daily_estimate" => {
            let key = entity_id
                .ok_or_else(|| "VALIDATION_ERROR: 按日预估同步缺少实体 ID".to_string())?;
            let (task_id, work_date) = key
                .split_once('|')
                .ok_or_else(|| "VALIDATION_ERROR: 按日预估同步实体 ID 无效".to_string())?;
            return query_json_row_with_params(
                connection,
                "SELECT * FROM task_daily_estimates WHERE workspace_id = ?1 AND task_id = ?2 AND work_date = ?3",
                params![local_workspace_id, task_id, work_date],
                key,
            );
        }
        "task_day_plan" => {
            let key = entity_id
                .ok_or_else(|| "VALIDATION_ERROR: 今日计划同步缺少实体 ID".to_string())?;
            let (task_id, work_date) = key
                .split_once('|')
                .ok_or_else(|| "VALIDATION_ERROR: 今日计划同步实体 ID 无效".to_string())?;
            return query_json_row_with_params(
                connection,
                "SELECT * FROM task_day_plans WHERE workspace_id = ?1 AND task_id = ?2 AND work_date = ?3",
                params![local_workspace_id, task_id, work_date],
                key,
            );
        }
        "task_recurrence_rule" => {
            let task_id = entity_id
                .ok_or_else(|| "VALIDATION_ERROR: 重复规则同步缺少事项 ID".to_string())?;
            let mut statement = connection
                .prepare(
                    "SELECT * FROM task_recurrence_rules WHERE workspace_id = ?1 AND task_id = ?2 ORDER BY effective_start, id",
                )
                .map_err(|error| error.to_string())?;
            let column_names = statement
                .column_names()
                .into_iter()
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>();
            let rules = statement
                .query_map(params![local_workspace_id, task_id], |row| {
                    row_to_json(row, &column_names)
                })
                .map_err(|error| error.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?;
            let task_version: i64 = connection
                .query_row(
                    "SELECT version FROM tasks WHERE workspace_id = ?1 AND id = ?2",
                    params![local_workspace_id, task_id],
                    |row| row.get(0),
                )
                .map_err(|error| error.to_string())?;
            let rule_version = rules
                .iter()
                .filter_map(|rule| rule.get("version").and_then(Value::as_i64))
                .max()
                .unwrap_or(0);
            return Ok(json!({
                "task_id": task_id,
                "task_version": task_version,
                "rules": rules,
                "version": rule_version
            }));
        }
        "report" => {
            let id = entity_id.ok_or_else(|| "VALIDATION_ERROR: 报告同步缺少实体 ID".to_string())?;
            let mut value = query_json_row(
                &connection,
                "SELECT * FROM reports WHERE workspace_id = ?1 AND id = ?2",
                &local_workspace_id,
                id,
            )?;
            let mut statement = connection
                .prepare("SELECT * FROM report_tasks WHERE workspace_id = ?1 AND report_id = ?2 ORDER BY sort_order")
                .map_err(|error| error.to_string())?;
            let column_names = statement
                .column_names()
                .into_iter()
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>();
            let rows = statement
                .query_map(params![local_workspace_id, id], |row| row_to_json(row, &column_names))
                .map_err(|error| error.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?;
            value
                .as_object_mut()
                .ok_or_else(|| "CACHE_ERROR: 报告缓存不是对象".to_string())?
                .insert("report_tasks".to_string(), Value::Array(rows));
            return Ok(value);
        }
        _ => return Err(format!("VALIDATION_ERROR: 不支持同步实体 {entity_type}")),
    };
    let id =
        identifier.ok_or_else(|| format!("VALIDATION_ERROR: {entity_type} 同步缺少实体 ID"))?;
    let mut value = query_json_row(&connection, sql, &local_workspace_id, id)?;
    normalize_embedded_json_fields(&mut value, entity_type);
    if entity_type == "task" {
        let mut statement = connection
            .prepare("SELECT * FROM task_status_events WHERE workspace_id = ?1 AND task_id = ?2 ORDER BY occurred_at")
            .map_err(|error| error.to_string())?;
        let column_names = statement
            .column_names()
            .into_iter()
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        let rows = statement
            .query_map(params![local_workspace_id, id], |row| {
                row_to_json(row, &column_names)
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        value
            .as_object_mut()
            .ok_or_else(|| "CACHE_ERROR: 事项缓存不是对象".to_string())?
            .insert("status_events".to_string(), Value::Array(rows));
    }
    Ok(value)
}

fn normalize_embedded_json_fields(value: &mut Value, entity_type: &str) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    let fields: &[&str] = match entity_type {
        "app_setting" => &["value_json"],
        "integration_config" => &["config_json"],
        _ => &[],
    };
    for field in fields {
        let Some(raw) = object.get(*field).and_then(Value::as_str) else {
            continue;
        };
        if let Ok(parsed) = serde_json::from_str::<Value>(raw) {
            object.insert((*field).to_string(), parsed);
        }
    }
}

fn query_json_row(
    connection: &rusqlite::Connection,
    sql: &str,
    workspace_id: &str,
    id: &str,
) -> Result<Value, String> {
    query_json_row_with_params(connection, sql, params![workspace_id, id], id)
}

fn query_json_row_with_params<P>(
    connection: &rusqlite::Connection,
    sql: &str,
    params: P,
    id_for_error: &str,
) -> Result<Value, String>
where
    P: rusqlite::Params,
{
    let mut statement = connection.prepare(sql).map_err(|error| error.to_string())?;
    let column_names = statement
        .column_names()
        .into_iter()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    statement
        .query_row(params, |row| row_to_json(row, &column_names))
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("NOT_FOUND: 待同步实体不存在 {id_for_error}"))
}

fn row_to_json(row: &rusqlite::Row<'_>, column_names: &[String]) -> rusqlite::Result<Value> {
    let mut object = serde_json::Map::new();
    for (index, name) in column_names.iter().enumerate() {
        let value: rusqlite::types::Value = row.get(index)?;
        object.insert(name.clone(), sqlite_value_to_json(value));
    }
    Ok(Value::Object(object))
}

fn sqlite_value_to_json(value: rusqlite::types::Value) -> Value {
    match value {
        rusqlite::types::Value::Null => Value::Null,
        rusqlite::types::Value::Integer(value) => json!(value),
        rusqlite::types::Value::Real(value) => json!(value),
        rusqlite::types::Value::Text(value) => json!(value),
        rusqlite::types::Value::Blob(value) => json!(String::from_utf8_lossy(&value)),
    }
}

#[cfg(test)]
fn claim_pending_operations(
    database: &Database,
    workspace_id: &str,
) -> Result<Vec<OutboxOperation>, String> {
    claim_pending_operations_with_retry_mode(database, workspace_id, false)
}

fn claim_pending_operations_with_retry_mode(
    database: &Database,
    workspace_id: &str,
    retry_immediately: bool,
) -> Result<Vec<OutboxOperation>, String> {
    let mut connection = database.open()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let now = now_millis();
    let stale_sending_before = now.saturating_sub(OUTBOX_SENDING_STALE_AFTER_MILLIS);
    let mut statement = transaction
        .prepare(
            "SELECT operation_id, workspace_epoch, operation_type, entity_type, entity_id, base_version, payload_version, coalesced_count, depends_on_operation_id, payload_json, state, attempt_count, last_attempt_at, created_at
             FROM sync_outbox
             WHERE workspace_id = ?1
               AND workspace_epoch = COALESCE((SELECT workspace_epoch FROM local_sync_state WHERE workspace_id = ?1), 1)
               AND NOT EXISTS (SELECT 1 FROM local_sync_state WHERE workspace_id = ?1 AND epoch_blocked <> 0)
               AND state IN ('pending','failed','sending','conflict')
             ORDER BY created_at, rowid",
        )
        .map_err(|error| error.to_string())?;
    let candidates = statement
        .query_map([workspace_id], |row| {
            let payload: String = row.get(9)?;
            let state: String = row.get(10)?;
            let attempt_count: i64 = row.get(11)?;
            let last_attempt_at: Option<i64> = row.get(12)?;
            let created_at: i64 = row.get(13)?;
            Ok(OutboxOperation {
                operation_id: row.get(0)?,
                workspace_epoch: row.get(1)?,
                operation_type: row.get(2)?,
                entity_type: row.get(3)?,
                entity_id: row.get(4)?,
                base_version: row.get(5)?,
                payload_version: row.get(6)?,
                coalesced_count: row.get(7)?,
                depends_on_operation_id: row.get(8)?,
                payload: serde_json::from_str(&payload).unwrap_or(Value::Null),
            })
            .map(|operation| (operation, state, attempt_count, last_attempt_at, created_at))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(statement);

    let mut blocked_entities = HashSet::<String>::new();
    let mut result = Vec::new();
    for (operation, state, attempt_count, last_attempt_at, created_at) in candidates {
        let entity_key = outbox_entity_key(&operation);
        if entity_key
            .as_ref()
            .is_some_and(|key| blocked_entities.contains(key))
        {
            continue;
        }
        let dependency_ready = operation
            .depends_on_operation_id
            .as_deref()
            .map(|dependency_id| {
                transaction
                    .query_row(
                        "SELECT state FROM sync_outbox WHERE operation_id = ?1",
                        [dependency_id],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()
                    .map(|state| state.is_none())
                    .map_err(|error| error.to_string())
            })
            .transpose()?
            .unwrap_or(true);
        let retry_ready = dependency_ready
            && match state.as_str() {
                "conflict" => false,
                "sending" => last_attempt_at.unwrap_or(created_at) < stale_sending_before,
                _ if retry_immediately => true,
                _ => retry_ready_at(last_attempt_at, attempt_count)
                    .is_none_or(|ready_at| ready_at <= now),
            };
        if retry_ready {
            if let Some(key) = entity_key {
                blocked_entities.insert(key);
            }
            result.push(operation);
        } else if let Some(key) = entity_key {
            blocked_entities.insert(key);
        }
    }

    for operation in &result {
        transaction
            .execute(
                "UPDATE sync_outbox
                 SET state = 'sending', attempt_count = attempt_count + 1, last_attempt_at = ?1, error_json = NULL
                 WHERE workspace_id = ?2
                   AND operation_id = ?3
                   AND (state IN ('pending','failed') OR (state = 'sending' AND COALESCE(last_attempt_at, created_at) < ?4))",
                params![now, workspace_id, operation.operation_id, stale_sending_before],
            )
            .map_err(|error| error.to_string())?;
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

fn recover_interrupted_sending_operations(
    database: &Database,
    workspace_id: &str,
) -> Result<(), String> {
    database
        .open()?
        .execute(
            "UPDATE sync_outbox
             SET state = 'pending', last_attempt_at = NULL
             WHERE workspace_id = ?1 AND state = 'sending'",
            [workspace_id],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn mark_historical_timer_recovery_candidates(
    database: &Database,
    workspace_id: &str,
) -> Result<usize, String> {
    let mut connection = database.open()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let mut updated = transaction
        .execute(
            "UPDATE sync_outbox
             SET state=CASE WHEN state='conflict' AND recovery_kind='competing_edit' THEN 'pending' ELSE state END,
                 error_json=CASE WHEN state='conflict' AND recovery_kind='competing_edit' THEN NULL ELSE error_json END,
                 last_attempt_at=CASE WHEN state='conflict' AND recovery_kind='competing_edit' THEN NULL ELSE last_attempt_at END,
                 recovery_kind=CASE WHEN recovery_kind='competing_edit' THEN 'historical_candidate' ELSE COALESCE(recovery_kind,'historical_candidate') END,
                 recovery_context_json=COALESCE(recovery_context_json,error_json),
                 recovery_updated_at=?1
             WHERE workspace_id=?2 AND entity_type='time_entry'
               AND (recovery_kind IS NULL OR recovery_kind='competing_edit')
               AND (error_json LIKE '%IDEMPOTENCY_CONFLICT%' OR error_json LIKE '%SYNC_CONFLICT%'
                    OR recovery_kind='competing_edit' OR state='conflict')",
            params![now_millis(), workspace_id],
        )
        .map_err(|error| error.to_string())?;
    let mut statement = transaction
        .prepare(
            "SELECT rowid,entity_id,base_version,payload_version,coalesced_count
             FROM sync_outbox
             WHERE workspace_id=?1 AND entity_type='time_entry' AND entity_id IS NOT NULL
             ORDER BY entity_id,created_at,rowid",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([workspace_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(statement);
    let mut previous_entity = None::<String>;
    let mut expected_base = None::<i64>;
    for (rowid, entity_id, base_version, payload_version, coalesced_count) in rows {
        if previous_entity.as_deref() != Some(entity_id.as_str()) {
            previous_entity = Some(entity_id);
            expected_base = base_version;
        }
        let valid_span = match (base_version, payload_version) {
            (None, Some(target)) => target == coalesced_count,
            (Some(base), Some(target)) => target == base.saturating_add(coalesced_count),
            _ => false,
        };
        let continuous = base_version == expected_base;
        if !valid_span || !continuous {
            updated += transaction
                .execute(
                    "UPDATE sync_outbox SET recovery_kind=COALESCE(recovery_kind,'historical_candidate'),
                     recovery_context_json=COALESCE(recovery_context_json,?1),recovery_updated_at=?2
                     WHERE rowid=?3 AND recovery_kind IS NULL",
                    params![
                        json!({
                            "message": "检测到不连续的历史计时版本链",
                            "expectedBaseVersion": expected_base,
                            "actualBaseVersion": base_version,
                            "payloadVersion": payload_version,
                            "coalescedCount": coalesced_count
                        })
                        .to_string(),
                        now_millis(),
                        rowid
                    ],
                )
                .map_err(|error| error.to_string())?;
        }
        expected_base = payload_version;
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(updated)
}

fn outbox_entity_key(operation: &OutboxOperation) -> Option<String> {
    Some(format!(
        "{}:{}",
        operation.entity_type,
        operation.entity_id.as_deref()?
    ))
}

fn retry_ready_at(last_attempt_at: Option<i64>, attempt_count: i64) -> Option<i64> {
    let last_attempt_at = last_attempt_at?;
    let exponent = attempt_count.clamp(0, 6) as u32;
    let backoff = (1_i64 << exponent)
        .saturating_mul(1000)
        .min(OUTBOX_RETRY_MAX_BACKOFF_MILLIS);
    Some(last_attempt_at.saturating_add(backoff))
}

fn mark_outbox_error(
    database: &Database,
    operation_id: &str,
    state: &str,
    error: &str,
) -> Result<(), String> {
    database
        .open()?
        .execute(
            "UPDATE sync_outbox SET state = ?1, last_attempt_at = ?2, error_json = ?3 WHERE operation_id = ?4",
            params![state, now_millis(), json!({ "message": error }).to_string(), operation_id],
        )
        .map(|_| ())
        .map_err(|database_error| database_error.to_string())
}

fn release_unprocessed_claims(
    database: &Database,
    operations: &[OutboxOperation],
) -> Result<(), String> {
    let mut connection = database.open()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    for operation in operations {
        transaction
            .execute(
                "UPDATE sync_outbox
                 SET state = 'pending', last_attempt_at = NULL, error_json = NULL
                 WHERE operation_id = ?1 AND state = 'sending'",
                [operation.operation_id.as_str()],
            )
            .map_err(|error| error.to_string())?;
    }
    transaction.commit().map_err(|error| error.to_string())
}

fn delete_outbox(database: &Database, operation_id: &str) -> Result<(), String> {
    database
        .open()?
        .execute(
            "DELETE FROM sync_outbox WHERE operation_id = ?1",
            [operation_id],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn outbox_counts(database: &Database, workspace_id: &str) -> Result<(i64, i64), String> {
    database
        .open()?
        .query_row(
            "SELECT
               SUM(CASE WHEN state IN ('pending','sending','failed') THEN 1 ELSE 0 END),
               SUM(CASE WHEN state = 'conflict' THEN 1 ELSE 0 END)
             FROM sync_outbox WHERE workspace_id = ?1",
            [workspace_id],
            |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?.unwrap_or(0),
                    row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                ))
            },
        )
        .map_err(|error| error.to_string())
}

fn list_sync_queue(
    database: &Database,
    workspace_id: &str,
) -> Result<Vec<CloudSyncQueueItem>, String> {
    let connection = database.open()?;
    let mut statement = connection
        .prepare(
            "SELECT operation_id, operation_type, entity_type, entity_id, state, payload_json,
                    attempt_count, error_json, coalesced_count, depends_on_operation_id,
                    created_at, last_attempt_at,base_version,payload_version,recovery_kind,recovery_context_json
             FROM sync_outbox
             WHERE workspace_id = ?1 AND state IN ('pending','sending','failed','conflict')
             ORDER BY created_at, rowid",
        )
        .map_err(|error| error.to_string())?;
    let items = statement
        .query_map([workspace_id], |row| {
            let payload: String = row.get(5)?;
            let error_json: Option<String> = row.get(7)?;
            let recovery_context_json: Option<String> = row.get(15)?;
            let error = error_json.and_then(|value| {
                serde_json::from_str::<Value>(&value)
                    .ok()
                    .and_then(|json| {
                        json.get("message")
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                    })
                    .or(Some(value))
            });
            Ok(CloudSyncQueueItem {
                operation_id: row.get(0)?,
                operation_type: row.get(1)?,
                entity_type: row.get(2)?,
                entity_id: row.get(3)?,
                state: row.get(4)?,
                payload: serde_json::from_str(&payload).unwrap_or(Value::Null),
                attempt_count: row.get(6)?,
                error,
                coalesced_count: row.get(8)?,
                base_version: row.get(12)?,
                payload_version: row.get(13)?,
                depends_on_operation_id: row.get(9)?,
                recovery_kind: row.get(14)?,
                recovery_context: recovery_context_json
                    .as_deref()
                    .and_then(|value| serde_json::from_str(value).ok()),
                created_at: row.get(10)?,
                last_attempt_at: row.get(11)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(items)
}

fn ensure_no_dirty_outbox(database: &Database, workspace_id: &str) -> Result<(), String> {
    let (pending, conflicts) = outbox_counts(database, workspace_id)?;
    if pending > 0 || conflicts > 0 {
        return Err(
            "SYNC_PENDING: 本机仍有待同步或冲突操作，不能用云端快照覆盖本地编辑".to_string(),
        );
    }
    Ok(())
}

fn has_old_epoch_local_changes(
    database: &Database,
    workspace_id: &str,
    server_epoch: i64,
) -> Result<bool, String> {
    database
        .open()?
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM sync_outbox
                WHERE workspace_id=?1 AND workspace_epoch<>?2
                  AND state IN ('pending','sending','failed','conflict')
                UNION ALL
                SELECT 1 FROM cloud_deferred_entities WHERE workspace_id=?1
            )",
            params![workspace_id, server_epoch],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())
}

fn mark_epoch_blocked(
    database: &Database,
    workspace_id: &str,
    server_epoch: i64,
    reason: &str,
) -> Result<(), String> {
    database
        .open()?
        .execute(
            "INSERT INTO local_sync_state(workspace_id,device_id,last_change_seq,workspace_epoch,epoch_blocked,epoch_blocked_reason,epoch_blocked_at,last_error)
             VALUES(?1,(SELECT json_extract(value_json,'$') FROM device_settings WHERE key='device_id'),0,?2,1,?3,?4,?3)
             ON CONFLICT(workspace_id) DO UPDATE SET workspace_epoch=excluded.workspace_epoch,epoch_blocked=1,epoch_blocked_reason=excluded.epoch_blocked_reason,epoch_blocked_at=excluded.epoch_blocked_at,last_error=excluded.last_error",
            params![workspace_id, server_epoch, reason, now_millis()],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn is_conflict_error(error: &str) -> bool {
    error.contains("SYNC_CONFLICT")
        || error.contains("VERSION_CONFLICT")
        || error.contains("ADJUSTMENT_BASELINE_CONFLICT")
}

fn is_workspace_epoch_stale_error(error: &str) -> bool {
    error.contains("WORKSPACE_EPOCH_STALE")
}

fn parse_server_workspace_epoch(error: &str) -> Option<i64> {
    let marker = "server=";
    let start = error.find(marker)? + marker.len();
    let digits = error[start..]
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect::<String>();
    digits.parse().ok()
}

fn update_sync_state(
    database: &Database,
    workspace_id: &str,
    device_id: &str,
    last_change_seq: i64,
    last_error: Option<&str>,
) -> Result<(), String> {
    database
        .open()?
        .execute(
            "INSERT INTO local_sync_state(workspace_id, device_id, last_change_seq, last_full_sync_at, last_error)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(workspace_id) DO UPDATE SET device_id = excluded.device_id, last_change_seq = excluded.last_change_seq, last_full_sync_at = excluded.last_full_sync_at, last_error = excluded.last_error",
            params![workspace_id, device_id, last_change_seq, now_millis(), last_error],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn local_revision(database: &Database) -> Result<i64, String> {
    database
        .open()?
        .query_row(
            "SELECT CAST(value AS INTEGER) FROM app_metadata WHERE key = 'global_revision'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())
}

#[cfg(test)]
fn apply_snapshot(
    database: &Database,
    snapshot: &Value,
    expected_local_revision: Option<i64>,
    sync_workspace_id: Option<&str>,
    sync_state_update: Option<(&str, &str, i64, i64)>,
) -> Result<(), String> {
    apply_snapshot_with_options(
        database,
        snapshot,
        expected_local_revision,
        sync_workspace_id,
        sync_state_update,
        true,
    )
}

#[cfg(test)]
fn apply_snapshot_with_options(
    database: &Database,
    snapshot: &Value,
    expected_local_revision: Option<i64>,
    sync_workspace_id: Option<&str>,
    sync_state_update: Option<(&str, &str, i64, i64)>,
    preserve_local_tracking: bool,
) -> Result<(), String> {
    apply_snapshot_with_policy(
        database,
        snapshot,
        expected_local_revision,
        sync_workspace_id,
        sync_state_update,
        preserve_local_tracking,
        false,
    )
}

fn apply_snapshot_with_reset_options(
    database: &Database,
    snapshot: &Value,
    expected_local_revision: Option<i64>,
    sync_workspace_id: Option<&str>,
    sync_state_update: Option<(&str, &str, i64, i64)>,
    preserve_local_tracking: bool,
    discard_dirty_outbox: bool,
) -> Result<(), String> {
    apply_snapshot_with_policy(
        database,
        snapshot,
        expected_local_revision,
        sync_workspace_id,
        sync_state_update,
        preserve_local_tracking,
        discard_dirty_outbox,
    )
}

fn apply_snapshot_with_policy(
    database: &Database,
    snapshot: &Value,
    expected_local_revision: Option<i64>,
    sync_workspace_id: Option<&str>,
    sync_state_update: Option<(&str, &str, i64, i64)>,
    preserve_local_tracking: bool,
    discard_dirty_outbox: bool,
) -> Result<(), String> {
    let resolved_unassigned_boundary = sync_workspace_id.and_then(|_| {
        snapshot
            .get("unassigned_sessions")
            .and_then(Value::as_array)
            .and_then(|sessions| {
                sessions
                    .iter()
                    .filter(|session| {
                        matches!(
                            session.get("state").and_then(Value::as_str),
                            Some("resolved") | Some("discarded")
                        )
                    })
                    .filter_map(|session| {
                        optional_millis(session, "resolved_at")
                            .map(|resolved_at| (text(session, "id"), resolved_at))
                    })
                    .max_by_key(|(_, resolved_at)| *resolved_at)
            })
    });
    let mut connection = database.open()?;
    let local_workspace_id: String = connection
        .query_row(
            "SELECT id FROM workspaces WHERE storage_mode = 'local' AND deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    if let Some(expected_revision) = expected_local_revision {
        let current_revision: i64 = transaction
            .query_row(
                "SELECT CAST(value AS INTEGER) FROM app_metadata WHERE key = 'global_revision'",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if current_revision != expected_revision {
            return Err("SYNC_RETRY: 拉取期间本地数据发生变化，已取消本次快照覆盖".to_string());
        }
    }
    if let Some(workspace_id) = sync_workspace_id {
        let pending: i64 = transaction
            .query_row(
                "SELECT COUNT(*) FROM sync_outbox WHERE workspace_id = ?1 AND state IN ('pending','sending','failed','conflict')",
                [workspace_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if pending > 0 && !discard_dirty_outbox {
            return Err("SYNC_PENDING: 本机产生了新的待同步操作，已取消本次快照覆盖".to_string());
        }
        if discard_dirty_outbox {
            let target_epoch = sync_state_update.map(|value| value.3).unwrap_or(i64::MAX);
            let epoch_blocked: bool = transaction
                .query_row(
                    "SELECT COALESCE(epoch_blocked,0) FROM local_sync_state WHERE workspace_id=?1",
                    [workspace_id],
                    |row| row.get(0),
                )
                .unwrap_or(false);
            if epoch_blocked {
                transaction
                    .execute(
                        "DELETE FROM sync_outbox WHERE workspace_id = ?1 AND workspace_epoch <> ?2",
                        params![workspace_id, target_epoch],
                    )
                    .map_err(|error| error.to_string())?;
            } else {
                transaction
                    .execute(
                        "DELETE FROM sync_outbox WHERE workspace_id = ?1",
                        [workspace_id],
                    )
                    .map_err(|error| error.to_string())?;
            }
            transaction
                .execute(
                    "DELETE FROM cloud_deferred_entities WHERE workspace_id = ?1",
                    [workspace_id],
                )
                .map_err(|error| error.to_string())?;
        }
    }
    if sync_workspace_id.is_some()
        && !matches!(
            snapshot.get("actual_time_adjustments"),
            Some(Value::Array(_))
        )
    {
        return Err(
            "CLOUD_SCHEMA_OUTDATED: 云端快照缺少今日实际用时数据。请先部署 supabase/20261020b_timegenie_actual_adjustment_snapshot.sql，已取消本次覆盖。"
                .to_string(),
        );
    }
    let integration_secret_refs = integration_secret_refs(&transaction, &local_workspace_id)?;
    let local_tracking_overlay = if preserve_local_tracking {
        local_tracking_overlay(
            &transaction,
            &local_workspace_id,
            snapshot,
            sync_workspace_id.is_none(),
        )?
    } else {
        LocalTrackingOverlay::default()
    };
    clear_snapshot_cache(&transaction, &local_workspace_id)?;
    replace_snapshot_table(&transaction, snapshot, "subjects", &local_workspace_id)?;
    replace_snapshot_table(
        &transaction,
        snapshot,
        "subject_work_schedules",
        &local_workspace_id,
    )?;
    replace_snapshot_table(
        &transaction,
        snapshot,
        "subject_work_schedule_segments",
        &local_workspace_id,
    )?;
    replace_snapshot_table(&transaction, snapshot, "tasks", &local_workspace_id)?;
    replace_snapshot_table(
        &transaction,
        snapshot,
        "task_status_events",
        &local_workspace_id,
    )?;
    replace_snapshot_table(
        &transaction,
        snapshot,
        "task_day_plans",
        &local_workspace_id,
    )?;
    replace_snapshot_table(
        &transaction,
        snapshot,
        "task_daily_estimates",
        &local_workspace_id,
    )?;
    replace_snapshot_table(
        &transaction,
        snapshot,
        "task_recurrence_rules",
        &local_workspace_id,
    )?;
    replace_snapshot_table(
        &transaction,
        snapshot,
        "task_occurrences",
        &local_workspace_id,
    )?;
    replace_snapshot_table(&transaction, snapshot, "work_days", &local_workspace_id)?;
    replace_snapshot_table(&transaction, snapshot, "time_entries", &local_workspace_id)?;
    replace_snapshot_table(&transaction, snapshot, "time_segments", &local_workspace_id)?;
    replace_snapshot_table(
        &transaction,
        snapshot,
        "time_allocations",
        &local_workspace_id,
    )?;
    replace_snapshot_table(
        &transaction,
        snapshot,
        "actual_time_adjustments",
        &local_workspace_id,
    )?;
    replace_snapshot_table(
        &transaction,
        snapshot,
        "unassigned_sessions",
        &local_workspace_id,
    )?;
    restore_snapshot_time_entry_links(&transaction, snapshot, &local_workspace_id)?;
    replace_snapshot_table(
        &transaction,
        snapshot,
        "unassigned_segments",
        &local_workspace_id,
    )?;
    replace_snapshot_table(
        &transaction,
        snapshot,
        "report_templates",
        &local_workspace_id,
    )?;
    replace_snapshot_table(&transaction, snapshot, "reports", &local_workspace_id)?;
    replace_snapshot_table(&transaction, snapshot, "report_tasks", &local_workspace_id)?;
    replace_snapshot_table(&transaction, snapshot, "app_settings", &local_workspace_id)?;
    replace_snapshot_table(
        &transaction,
        snapshot,
        "integration_configs",
        &local_workspace_id,
    )?;
    restore_integration_secret_refs(&transaction, &local_workspace_id, &integration_secret_refs)?;
    replace_snapshot_table(
        &transaction,
        snapshot,
        "external_bindings",
        &local_workspace_id,
    )?;
    restore_local_tracking_overlay(&transaction, &local_workspace_id, &local_tracking_overlay)?;
    if let Some((predecessor_session_id, resolved_at)) = resolved_unassigned_boundary.as_ref() {
        ensure_next_shared_unassigned_session(
            &transaction,
            &local_workspace_id,
            predecessor_session_id,
            *resolved_at,
        )?;
    }
    transaction
        .execute(
            "UPDATE app_metadata SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT) WHERE key = 'global_revision'",
            [],
        )
        .map_err(|error| error.to_string())?;
    if let Some((workspace_id, device_id, last_change_seq, workspace_epoch)) = sync_state_update {
        transaction
            .execute(
                "INSERT INTO local_sync_state(workspace_id, device_id, last_change_seq, last_full_sync_at, last_error, workspace_epoch, epoch_blocked, epoch_blocked_reason, epoch_blocked_at)
                 VALUES (?1, ?2, ?3, ?4, NULL, ?5, 0, NULL, NULL)
                 ON CONFLICT(workspace_id) DO UPDATE SET device_id=excluded.device_id,last_change_seq=excluded.last_change_seq,last_full_sync_at=excluded.last_full_sync_at,last_error=NULL,workspace_epoch=excluded.workspace_epoch,epoch_blocked=0,epoch_blocked_reason=NULL,epoch_blocked_at=NULL",
                params![workspace_id, device_id, last_change_seq, now_millis(), workspace_epoch],
            )
            .map_err(|error| error.to_string())?;
    }
    transaction.commit().map_err(|error| error.to_string())?;
    if let Some((predecessor_session_id, _)) = resolved_unassigned_boundary {
        queue_next_shared_unassigned_candidate(database, &json!({ "id": predecessor_session_id }))?;
    }
    Ok(())
}

fn integration_secret_refs(
    transaction: &Transaction<'_>,
    workspace_id: &str,
) -> Result<HashMap<String, String>, String> {
    let mut statement = transaction
        .prepare(
            "SELECT provider, secret_ref FROM integration_configs
             WHERE workspace_id = ?1 AND secret_ref IS NOT NULL AND secret_ref != ''",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([workspace_id], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|error| error.to_string())?;
    let mut refs = HashMap::new();
    for row in rows {
        let (provider, secret_ref): (String, String) = row.map_err(|error| error.to_string())?;
        refs.insert(provider, secret_ref);
    }
    Ok(refs)
}

fn local_tracking_overlay(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    snapshot: &Value,
    preserve_unassigned: bool,
) -> Result<LocalTrackingOverlay, String> {
    let mut overlay = LocalTrackingOverlay::default();

    let mut timer_entry_statement = transaction
        .prepare(
            "SELECT id, timer_chain_id, previous_entry_id, split_boundary_at,
                    last_continuous_at, work_date, kind, source_type, state, default_task_id,
                    label_snapshot, started_at, ended_at, duration_seconds, note,
                    origin_unassigned_session_id, created_at, updated_at, version, deleted_at
             FROM time_entries
             WHERE workspace_id = ?1 AND state IN ('running', 'paused') AND deleted_at IS NULL",
        )
        .map_err(|error| error.to_string())?;
    let timer_entry_rows = timer_entry_statement
        .query_map([workspace_id], |row| {
            Ok(ActiveTimerEntryOverlay {
                entry_id: row.get(0)?,
                timer_chain_id: row.get(1)?,
                previous_entry_id: row.get(2)?,
                split_boundary_at: row.get(3)?,
                last_continuous_at: row.get(4)?,
                work_date: row.get(5)?,
                kind: row.get(6)?,
                source_type: row.get(7)?,
                state: row.get(8)?,
                default_task_id: row.get(9)?,
                label_snapshot: row.get(10)?,
                started_at: row.get(11)?,
                ended_at: row.get(12)?,
                duration_seconds: row.get(13)?,
                note: row.get(14)?,
                origin_unassigned_session_id: row.get(15)?,
                created_at: row.get(16)?,
                updated_at: row.get(17)?,
                version: row.get(18)?,
                deleted_at: row.get(19)?,
            })
        })
        .map_err(|error| error.to_string())?;
    for row in timer_entry_rows {
        let entry = row.map_err(|error| error.to_string())?;
        let snapshot_entry = snapshot
            .get("time_entries")
            .and_then(Value::as_array)
            .and_then(|rows| {
                rows.iter().find(|item| {
                    item.get("id").and_then(Value::as_str) == Some(entry.entry_id.as_str())
                })
            });
        let snapshot_version = snapshot_entry.map(|item| integer(item, "version"));
        let snapshot_has_equal_or_newer_running_entry = snapshot_entry.is_some_and(|item| {
            matches!(
                item.get("state").and_then(Value::as_str),
                Some("running") | Some("paused")
            ) && snapshot_version.unwrap_or_default() >= entry.version
        });
        let snapshot_has_newer_entry =
            snapshot_version.is_some_and(|version| version > entry.version);
        if !snapshot_has_equal_or_newer_running_entry && !snapshot_has_newer_entry {
            overlay.active_timer_entries.push(entry);
        }
    }

    let mut timer_statement = transaction
        .prepare(
            "SELECT s.entry_id, s.id, s.sequence_no, s.started_at, s.duration_seconds
             FROM time_segments s
             JOIN time_entries e ON e.id = s.entry_id
             WHERE s.workspace_id = ?1 AND s.ended_at IS NULL
               AND e.workspace_id = ?1 AND e.state = 'running' AND e.deleted_at IS NULL",
        )
        .map_err(|error| error.to_string())?;
    let timer_rows = timer_statement
        .query_map([workspace_id], |row| {
            Ok(ActiveTimerSegmentOverlay {
                entry_id: row.get(0)?,
                segment_id: row.get(1)?,
                sequence_no: row.get(2)?,
                started_at: row.get(3)?,
                duration_seconds: row.get(4)?,
            })
        })
        .map_err(|error| error.to_string())?;
    for row in timer_rows {
        let segment = row.map_err(|error| error.to_string())?;
        let local_entry_preserved = overlay
            .active_timer_entries
            .iter()
            .any(|entry| entry.entry_id == segment.entry_id);
        let snapshot_has_running_entry = snapshot
            .get("time_entries")
            .and_then(Value::as_array)
            .is_some_and(|rows| {
                rows.iter().any(|item| {
                    item.get("id").and_then(Value::as_str) == Some(segment.entry_id.as_str())
                        && matches!(
                            item.get("state").and_then(Value::as_str),
                            Some("running") | Some("paused")
                        )
                })
            });
        if !local_entry_preserved && !snapshot_has_running_entry {
            continue;
        }
        let segment_in_snapshot = snapshot
            .get("time_segments")
            .and_then(Value::as_array)
            .and_then(|rows| {
                rows.iter().find(|item| {
                    item.get("id").and_then(Value::as_str) == Some(segment.segment_id.as_str())
                        && item.get("entry_id").and_then(Value::as_str)
                            == Some(segment.entry_id.as_str())
                        && item.get("ended_at").is_some_and(Value::is_null)
                })
            });
        if segment_in_snapshot.is_none() {
            overlay.active_timer_segments.push(segment);
        }
    }

    if !preserve_unassigned {
        return Ok(overlay);
    }

    let mut unassigned_statement = transaction
        .prepare(
            "SELECT s.session_id, s.id, s.sequence_no, s.started_at, s.duration_seconds, s.lease_token
             FROM unassigned_segments s
             JOIN unassigned_sessions u ON u.id = s.session_id
             WHERE s.workspace_id = ?1 AND s.ended_at IS NULL
               AND u.workspace_id = ?1 AND u.state IN ('collecting', 'awaiting_resolution')",
        )
        .map_err(|error| error.to_string())?;
    let unassigned_rows = unassigned_statement
        .query_map([workspace_id], |row| {
            Ok(ActiveUnassignedSegmentOverlay {
                session_id: row.get(0)?,
                segment_id: row.get(1)?,
                sequence_no: row.get(2)?,
                started_at: row.get(3)?,
                duration_seconds: row.get(4)?,
                lease_token: row.get(5)?,
            })
        })
        .map_err(|error| error.to_string())?;

    let mut session_statement = transaction
        .prepare(
            "SELECT id, last_continuous_at, work_date, state, threshold_seconds, duration_seconds,
                    first_started_at, last_ended_at, prompted_at, resolution_type,
                    generated_entry_id, resolved_at, created_at, updated_at, version
             FROM unassigned_sessions
             WHERE workspace_id = ?1 AND state IN ('collecting', 'awaiting_resolution')",
        )
        .map_err(|error| error.to_string())?;
    let session_rows = session_statement
        .query_map([workspace_id], |row| {
            Ok(ActiveUnassignedSessionOverlay {
                session_id: row.get(0)?,
                last_continuous_at: row.get(1)?,
                work_date: row.get(2)?,
                state: row.get(3)?,
                threshold_seconds: row.get(4)?,
                duration_seconds: row.get(5)?,
                first_started_at: row.get(6)?,
                last_ended_at: row.get(7)?,
                prompted_at: row.get(8)?,
                resolution_type: row.get(9)?,
                generated_entry_id: row.get(10)?,
                resolved_at: row.get(11)?,
                created_at: row.get(12)?,
                updated_at: row.get(13)?,
                version: row.get(14)?,
            })
        })
        .map_err(|error| error.to_string())?;
    for row in session_rows {
        let session = row.map_err(|error| error.to_string())?;
        let session_in_snapshot = snapshot
            .get("unassigned_sessions")
            .and_then(Value::as_array)
            .and_then(|rows| {
                rows.iter().find(|item| {
                    item.get("id").and_then(Value::as_str) == Some(session.session_id.as_str())
                })
            });
        let snapshot_version = session_in_snapshot.map(|item| integer(item, "version"));
        let snapshot_has_equal_or_newer_active_session = session_in_snapshot.is_some_and(|item| {
            matches!(
                item.get("state").and_then(Value::as_str),
                Some("collecting") | Some("awaiting_resolution")
            ) && snapshot_version.unwrap_or_default() >= session.version
        });
        let snapshot_has_newer_session =
            snapshot_version.is_some_and(|version| version > session.version);
        if !snapshot_has_equal_or_newer_active_session && !snapshot_has_newer_session {
            overlay.active_unassigned_sessions.push(session);
        }
    }

    for row in unassigned_rows {
        let segment = row.map_err(|error| error.to_string())?;
        let session_in_snapshot = snapshot
            .get("unassigned_sessions")
            .and_then(Value::as_array)
            .and_then(|rows| {
                rows.iter().find(|item| {
                    item.get("id").and_then(Value::as_str) == Some(segment.session_id.as_str())
                        && matches!(
                            item.get("state").and_then(Value::as_str),
                            Some("collecting") | Some("awaiting_resolution")
                        )
                })
            });
        let local_session_preserved = overlay
            .active_unassigned_sessions
            .iter()
            .any(|session| session.session_id == segment.session_id);
        if session_in_snapshot.is_none() && !local_session_preserved {
            continue;
        }
        let segment_in_snapshot = snapshot
            .get("unassigned_segments")
            .and_then(Value::as_array)
            .and_then(|rows| {
                rows.iter().find(|item| {
                    item.get("id").and_then(Value::as_str) == Some(segment.segment_id.as_str())
                        && item.get("session_id").and_then(Value::as_str)
                            == Some(segment.session_id.as_str())
                        && item.get("ended_at").is_some_and(Value::is_null)
                })
            });
        if segment_in_snapshot.is_none() {
            overlay.active_unassigned_segments.push(segment);
        }
    }

    Ok(overlay)
}

fn restore_local_tracking_overlay(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    overlay: &LocalTrackingOverlay,
) -> Result<(), String> {
    if let Some(session) = overlay.active_unassigned_sessions.first() {
        transaction
            .execute(
                "DELETE FROM unassigned_segments
                 WHERE session_id IN (
                   SELECT id FROM unassigned_sessions
                   WHERE workspace_id = ?1 AND id <> ?2
                     AND state IN ('collecting', 'awaiting_resolution')
                 )",
                params![workspace_id, session.session_id],
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "DELETE FROM unassigned_sessions
                 WHERE workspace_id = ?1 AND id <> ?2
                   AND state IN ('collecting', 'awaiting_resolution')",
                params![workspace_id, session.session_id],
            )
            .map_err(|error| error.to_string())?;
    }
    for session in &overlay.active_unassigned_sessions {
        transaction
            .execute(
                "INSERT INTO unassigned_sessions(
                   id, workspace_id, work_date, state, threshold_seconds, duration_seconds,
                   first_started_at, last_ended_at, prompted_at, resolution_type,
                   generated_entry_id, resolved_at, created_at, updated_at, version,
                   last_continuous_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                 ON CONFLICT(id) DO UPDATE SET
                   work_date = excluded.work_date, state = excluded.state,
                   threshold_seconds = excluded.threshold_seconds,
                   duration_seconds = excluded.duration_seconds,
                   first_started_at = excluded.first_started_at,
                   last_ended_at = excluded.last_ended_at,
                   prompted_at = excluded.prompted_at,
                   resolution_type = excluded.resolution_type,
                   generated_entry_id = excluded.generated_entry_id,
                   resolved_at = excluded.resolved_at,
                   created_at = excluded.created_at,
                   updated_at = excluded.updated_at,
                   version = excluded.version,
                   last_continuous_at = excluded.last_continuous_at",
                params![
                    session.session_id,
                    workspace_id,
                    session.work_date,
                    session.state,
                    session.threshold_seconds,
                    session.duration_seconds,
                    session.first_started_at,
                    session.last_ended_at,
                    session.prompted_at,
                    session.resolution_type,
                    session.generated_entry_id,
                    session.resolved_at,
                    session.created_at,
                    session.updated_at,
                    session.version,
                    session.last_continuous_at
                ],
            )
            .map_err(|error| error.to_string())?;
    }
    for entry in &overlay.active_timer_entries {
        transaction
            .execute(
                "DELETE FROM time_segments WHERE workspace_id = ?1 AND entry_id = ?2",
                params![workspace_id, entry.entry_id],
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "DELETE FROM time_allocations WHERE workspace_id = ?1 AND entry_id = ?2",
                params![workspace_id, entry.entry_id],
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "INSERT INTO time_entries(
                   id, workspace_id, work_date, kind, source_type, state, default_task_id,
                   label_snapshot, started_at, ended_at, duration_seconds, note,
                   origin_unassigned_session_id, created_at, updated_at, version, deleted_at,
                   timer_chain_id, previous_entry_id, split_boundary_at, last_continuous_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)
                 ON CONFLICT(id) DO UPDATE SET
                   work_date = excluded.work_date,
                   kind = excluded.kind,
                   source_type = excluded.source_type,
                   state = excluded.state,
                   default_task_id = excluded.default_task_id,
                   label_snapshot = excluded.label_snapshot,
                   started_at = excluded.started_at,
                   ended_at = excluded.ended_at,
                   duration_seconds = excluded.duration_seconds,
                   note = excluded.note,
                   origin_unassigned_session_id = excluded.origin_unassigned_session_id,
                   updated_at = excluded.updated_at,
                   version = excluded.version,
                   deleted_at = excluded.deleted_at,
                   timer_chain_id = excluded.timer_chain_id,
                   previous_entry_id = excluded.previous_entry_id,
                   split_boundary_at = excluded.split_boundary_at,
                   last_continuous_at = excluded.last_continuous_at",
                params![
                    entry.entry_id,
                    workspace_id,
                    entry.work_date,
                    entry.kind,
                    entry.source_type,
                    entry.state,
                    entry.default_task_id,
                    entry.label_snapshot,
                    entry.started_at,
                    entry.ended_at,
                    entry.duration_seconds,
                    entry.note,
                    entry.origin_unassigned_session_id,
                    entry.created_at,
                    entry.updated_at,
                    entry.version,
                    entry.deleted_at,
                    entry.timer_chain_id,
                    entry.previous_entry_id,
                    entry.split_boundary_at,
                    entry.last_continuous_at
                ],
            )
            .map_err(|error| error.to_string())?;
    }
    for segment in &overlay.active_timer_segments {
        transaction
            .execute(
                "INSERT INTO time_segments(id, workspace_id, entry_id, sequence_no, started_at, duration_seconds)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(id) DO UPDATE SET ended_at = NULL, started_at = excluded.started_at,
                   sequence_no = excluded.sequence_no, duration_seconds = excluded.duration_seconds",
                params![
                    segment.segment_id,
                    workspace_id,
                    segment.entry_id,
                    segment.sequence_no,
                    segment.started_at,
                    segment.duration_seconds
                ],
            )
            .map_err(|error| error.to_string())?;
    }
    for segment in &overlay.active_unassigned_segments {
        transaction
            .execute(
                "INSERT INTO unassigned_segments(id, workspace_id, session_id, sequence_no, started_at, duration_seconds, lease_token)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(id) DO UPDATE SET ended_at = NULL, started_at = excluded.started_at,
                   sequence_no = excluded.sequence_no, duration_seconds = excluded.duration_seconds,
                   lease_token = excluded.lease_token",
                params![
                    segment.segment_id,
                    workspace_id,
                    segment.session_id,
                    segment.sequence_no,
                    segment.started_at,
                    segment.duration_seconds,
                    segment.lease_token
                ],
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn restore_integration_secret_refs(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    secret_refs: &HashMap<String, String>,
) -> Result<(), String> {
    for (provider, secret_ref) in secret_refs {
        transaction
            .execute(
                "UPDATE integration_configs SET secret_ref = ?1 WHERE workspace_id = ?2 AND provider = ?3",
                params![secret_ref, workspace_id, provider],
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn replace_snapshot_table(
    transaction: &Transaction<'_>,
    snapshot: &Value,
    table: &str,
    workspace_id: &str,
) -> Result<(), String> {
    let rows = snapshot
        .get(table)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    match table {
        "subjects" => {
            for row in rows {
                transaction.execute("INSERT INTO subjects(id, workspace_id, name, sort_order, created_at, updated_at, version, created_by_device_id, updated_by_device_id, deleted_at) VALUES (?1,?2,?3,?4,?5,?6,?7,NULL,NULL,?8)", params![text(&row,"id"),workspace_id,text(&row,"name"),integer(&row,"sort_order"),millis(&row,"created_at"),millis(&row,"updated_at"),integer(&row,"version"),optional_millis(&row,"deleted_at")]).map_err(|error| error.to_string())?;
            }
        }
        "subject_work_schedules" => {
            replace_simple_rows(transaction, table, workspace_id, rows, |tx, row, ws| {
                tx.execute("INSERT INTO subject_work_schedules(id,workspace_id,subject_id,weekday,is_workday,created_at,updated_at,version) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)", params![text(row,"id"),ws,text(row,"subject_id"),integer(row,"weekday"),boolean(row,"is_workday") as i64,millis(row,"created_at"),millis(row,"updated_at"),integer(row,"version")])
            })?
        }
        "subject_work_schedule_segments" => {
            replace_simple_rows(transaction, table, workspace_id, rows, |tx, row, ws| {
                tx.execute("INSERT INTO subject_work_schedule_segments(id,workspace_id,schedule_id,sequence_no,start_minute,end_minute) VALUES (?1,?2,?3,?4,?5,?6)", params![text(row,"id"),ws,text(row,"schedule_id"),integer(row,"sequence_no"),integer(row,"start_minute"),integer(row,"end_minute")])
            })?
        }
        "tasks" => {
            for row in &rows {
                transaction.execute("INSERT INTO tasks(id,workspace_id,subject_id,parent_id,title,status,planned_date,planned_time,estimate_minutes,note,project_name,solution_name,source_type,source_ref,sort_order,completed_at,created_at,updated_at,version,deleted_at,day_instance_date,day_instance_kind,day_instance_source_task_id) VALUES (?1,?2,?3,NULL,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,NULL)", params![text(row,"id"),workspace_id,text(row,"subject_id"),text(row,"title"),text(row,"status"),optional_text(row,"planned_date"),optional_time(row,"planned_time"),optional_integer(row,"estimate_minutes"),optional_text(row,"note"),optional_text(row,"project_name"),optional_text(row,"solution_name"),text(row,"source_type"),optional_text(row,"source_ref"),integer(row,"sort_order"),optional_millis(row,"completed_at"),millis(row,"created_at"),millis(row,"updated_at"),integer(row,"version"),optional_millis(row,"deleted_at"),optional_text(row,"day_instance_date"),optional_text(row,"day_instance_kind")]).map_err(|error| format!("LOCAL_SNAPSHOT_APPLY_FAILED: tasks.day_instance: {error}"))?;
            }
            for row in &rows {
                if let Some(source_task_id) = optional_text(row, "day_instance_source_task_id") {
                    transaction
                        .execute(
                            "UPDATE tasks SET day_instance_source_task_id=?1 WHERE workspace_id=?2 AND id=?3",
                            params![source_task_id, workspace_id, text(row, "id")],
                        )
                        .map_err(|error| format!("LOCAL_SNAPSHOT_APPLY_FAILED: tasks.day_instance_source_task_id: {error}"))?;
                }
            }
            for row in rows {
                if let Some(parent_id) = optional_text(&row, "parent_id") {
                    transaction
                        .execute(
                            "UPDATE tasks SET parent_id = ?1 WHERE workspace_id = ?2 AND id = ?3",
                            params![parent_id, workspace_id, text(&row, "id")],
                        )
                        .map_err(|error| {
                            format!(
                                "LOCAL_SNAPSHOT_APPLY_FAILED: tasks.parent_id: task={}, parent={parent_id}: {error}",
                                text(&row, "id")
                            )
                        })?;
                }
            }
        }
        "task_status_events" => {
            replace_simple_rows(transaction, table, workspace_id, rows, |tx, row, ws| {
                tx.execute("INSERT INTO task_status_events(id,workspace_id,task_id,status,occurred_at,source_type,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7)", params![text(row,"id"),ws,text(row,"task_id"),text(row,"status"),millis(row,"occurred_at"),text(row,"source_type"),millis(row,"created_at")])
            })?
        }
        "task_daily_estimates" => {
            replace_simple_rows(transaction, table, workspace_id, rows, |tx, row, ws| {
                tx.execute("INSERT INTO task_daily_estimates(workspace_id,task_id,work_date,estimate_minutes,created_at,updated_at,version) VALUES (?1,?2,?3,?4,?5,?6,?7)", params![ws,text(row,"task_id"),text(row,"work_date"),integer(row,"estimate_minutes"),millis(row,"created_at"),millis(row,"updated_at"),integer(row,"version")])
            })?
        }
        "task_day_plans" => {
            replace_simple_rows(transaction, table, workspace_id, rows, |tx, row, ws| {
                tx.execute("INSERT INTO task_day_plans(workspace_id,task_id,work_date,state,instance_task_id,created_at,updated_at,version) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)", params![ws,text(row,"task_id"),text(row,"work_date"),text(row,"state"),optional_text(row,"instance_task_id"),millis(row,"created_at"),millis(row,"updated_at"),integer(row,"version")])
            })?
        }
        "task_recurrence_rules" => {
            replace_simple_rows(transaction, table, workspace_id, rows, |tx, row, ws| {
                tx.execute("INSERT INTO task_recurrence_rules(id,workspace_id,task_id,frequency,weekdays_mask,effective_start,effective_end,created_at,updated_at,version) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)", params![text(row,"id"),ws,text(row,"task_id"),text(row,"frequency"),optional_integer(row,"weekdays_mask"),text(row,"effective_start"),optional_text(row,"effective_end"),millis(row,"created_at"),millis(row,"updated_at"),integer(row,"version")])
            })?
        }
        "task_occurrences" => {
            replace_simple_rows(transaction, table, workspace_id, rows, |tx, row, ws| {
                tx.execute("INSERT INTO task_occurrences(workspace_id,task_id,occurrence_date,origin,status,completed_at,created_at,updated_at,version) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)", params![ws,text(row,"task_id"),text(row,"occurrence_date"),text(row,"origin"),text(row,"status"),optional_millis(row,"completed_at"),millis(row,"created_at"),millis(row,"updated_at"),integer(row,"version")])
            })?
        }
        "work_days" => {
            replace_simple_rows(transaction, table, workspace_id, rows, |tx, row, ws| {
                tx.execute("INSERT INTO work_days(workspace_id,work_date,timezone,work_period_text,note,settled_at,created_at,updated_at,version) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)", params![ws,text(row,"work_date"),text(row,"timezone"),optional_text(row,"work_period_text"),optional_text(row,"note"),optional_millis(row,"settled_at"),millis(row,"created_at"),millis(row,"updated_at"),integer(row,"version")])
            })?
        }
        "time_entries" => {
            replace_simple_rows(
                transaction,
                table,
                workspace_id,
                rows.clone(),
                |tx, row, ws| {
                    let entry_id = text(row, "id");
                    let source_type = text(row, "source_type");
                    let timer_chain_id = optional_text(row, "timer_chain_id")
                        .or_else(|| (source_type == "timer").then(|| entry_id.clone()));
                    let last_continuous_at = optional_millis(row, "last_continuous_at")
                        .or_else(|| optional_millis(row, "ended_at"))
                        .or_else(|| optional_millis(row, "updated_at"))
                        .or_else(|| optional_millis(row, "started_at"));
                    tx.execute("INSERT INTO time_entries(id,workspace_id,work_date,kind,source_type,state,default_task_id,label_snapshot,started_at,ended_at,duration_seconds,note,origin_unassigned_session_id,created_at,updated_at,version,deleted_at,timer_chain_id,previous_entry_id,split_boundary_at,last_continuous_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,NULL,?13,?14,?15,?16,?17,NULL,?18,?19)", params![entry_id,ws,text(row,"work_date"),text(row,"kind"),source_type,text(row,"state"),optional_text(row,"default_task_id"),text(row,"label_snapshot"),millis(row,"started_at"),optional_millis(row,"ended_at"),integer(row,"duration_seconds"),optional_text(row,"note"),millis(row,"created_at"),millis(row,"updated_at"),integer(row,"version"),optional_millis(row,"deleted_at"),timer_chain_id,optional_millis(row,"split_boundary_at"),last_continuous_at])
                },
            )?;
            for row in rows {
                if let Some(previous_entry_id) = optional_text(&row, "previous_entry_id") {
                    transaction
                        .execute(
                            "UPDATE time_entries SET previous_entry_id=?1 WHERE workspace_id=?2 AND id=?3",
                            params![previous_entry_id, workspace_id, text(&row, "id")],
                        )
                        .map_err(|error| format!("LOCAL_SNAPSHOT_APPLY_FAILED: time_entries.previous_entry_id: {error}"))?;
                }
            }
        }
        "time_segments" => {
            replace_simple_rows(transaction, table, workspace_id, rows, |tx, row, ws| {
                tx.execute("INSERT INTO time_segments(id,workspace_id,entry_id,sequence_no,started_at,ended_at,duration_seconds) VALUES (?1,?2,?3,?4,?5,?6,?7)", params![text(row,"id"),ws,text(row,"entry_id"),integer(row,"sequence_no"),millis(row,"started_at"),optional_millis(row,"ended_at"),integer(row,"duration_seconds")])
            })?
        }
        "time_allocations" => {
            replace_simple_rows(transaction, table, workspace_id, rows, |tx, row, ws| {
                tx.execute("INSERT INTO time_allocations(id,workspace_id,entry_id,task_id,minutes,note,created_at,updated_at,version) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)", params![text(row,"id"),ws,text(row,"entry_id"),text(row,"task_id"),integer(row,"minutes"),optional_text(row,"note"),millis(row,"created_at"),millis(row,"updated_at"),integer(row,"version")])
            })?
        }
        "actual_time_adjustments" => {
            replace_simple_rows(transaction, table, workspace_id, rows, |tx, row, ws| {
                tx.execute("INSERT INTO actual_time_adjustments(id,workspace_id,subject_id,task_id,work_date,delta_minutes,baseline_minutes,target_minutes,note,source_type,batch_id,created_at,updated_at,version,created_by_device_id,updated_by_device_id,deleted_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,(SELECT id FROM devices WHERE id=?15 AND workspace_id=?2),(SELECT id FROM devices WHERE id=?16 AND workspace_id=?2),?17)", params![text(row,"id"),ws,text(row,"subject_id"),text(row,"task_id"),text(row,"work_date"),integer(row,"delta_minutes"),integer(row,"baseline_minutes"),integer(row,"target_minutes"),optional_text(row,"note"),optional_text(row,"source_type").unwrap_or_else(|| "quick_actual_adjustment".to_string()),text(row,"batch_id"),millis(row,"created_at"),millis(row,"updated_at"),integer(row,"version"),optional_text(row,"created_by_device_id"),optional_text(row,"updated_by_device_id"),optional_millis(row,"deleted_at")])
            })?
        }
        "unassigned_sessions" => {
            replace_simple_rows(transaction, table, workspace_id, rows, |tx, row, ws| {
                let last_continuous_at = optional_millis(row, "last_continuous_at")
                    .or_else(|| optional_millis(row, "last_ended_at"))
                    .or_else(|| optional_millis(row, "updated_at"))
                    .or_else(|| optional_millis(row, "first_started_at"));
                tx.execute("INSERT INTO unassigned_sessions(id,workspace_id,work_date,state,threshold_seconds,duration_seconds,first_started_at,last_ended_at,prompted_at,resolution_type,generated_entry_id,resolved_at,created_at,updated_at,version,last_continuous_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)", params![text(row,"id"),ws,text(row,"work_date"),text(row,"state"),integer(row,"threshold_seconds"),integer(row,"duration_seconds"),millis(row,"first_started_at"),optional_millis(row,"last_ended_at"),optional_millis(row,"prompted_at"),optional_text(row,"resolution_type"),optional_text(row,"generated_entry_id"),optional_millis(row,"resolved_at"),millis(row,"created_at"),millis(row,"updated_at"),integer(row,"version"),last_continuous_at])
            })?
        }
        "unassigned_segments" => {
            replace_simple_rows(transaction, table, workspace_id, rows, |tx, row, ws| {
                tx.execute("INSERT INTO unassigned_segments(id,workspace_id,session_id,sequence_no,started_at,ended_at,duration_seconds,lease_token) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)", params![text(row,"id"),ws,text(row,"session_id"),integer(row,"sequence_no"),millis(row,"started_at"),optional_millis(row,"ended_at"),integer(row,"duration_seconds"),optional_text(row,"lease_token")])
            })?
        }
        "report_templates" => {
            replace_simple_rows(transaction, table, workspace_id, rows, |tx, row, ws| {
                tx.execute("INSERT INTO report_templates(id,workspace_id,report_type,subject_id,name,description,content,is_default,is_builtin,sort_order,created_at,updated_at,version,deleted_at) VALUES (?1,?2,?3,NULL,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)", params![text(row,"id"),ws,text(row,"report_type"),row.get("name").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| format!("迁移模板 - {}", text(row,"id").chars().take(8).collect::<String>())),optional_text(row,"description"),text(row,"content"),boolean(row,"is_default") as i64,boolean(row,"is_builtin") as i64,integer(row,"sort_order"),millis(row,"created_at"),millis(row,"updated_at"),integer(row,"version"),optional_millis(row,"deleted_at")])
            })?
        }
        "reports" => replace_simple_rows(transaction, table, workspace_id, rows, |tx, row, ws| {
            tx.execute("INSERT INTO reports(id,workspace_id,report_type,subject_id,period_start,period_end,reference_date,template_id,markdown_content,content_source,generation_count,input_revision_hash,generated_at,created_at,updated_at,version,deleted_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)", params![text(row,"id"),ws,text(row,"report_type"),text(row,"subject_id"),text(row,"period_start"),text(row,"period_end"),text(row,"reference_date"),optional_text(row,"template_id"),text(row,"markdown_content"),text(row,"content_source"),integer(row,"generation_count"),optional_text(row,"input_revision_hash"),optional_millis(row,"generated_at"),millis(row,"created_at"),millis(row,"updated_at"),integer(row,"version"),optional_millis(row,"deleted_at")])
        })?,
        "report_tasks" => {
            replace_simple_rows(transaction, table, workspace_id, rows, |tx, row, ws| {
                tx.execute("INSERT INTO report_tasks(report_id,task_id,workspace_id,sort_order,title_snapshot,path_snapshot) VALUES (?1,?2,?3,?4,?5,?6)", params![text(row,"report_id"),text(row,"task_id"),ws,integer(row,"sort_order"),text(row,"title_snapshot"),text(row,"path_snapshot")])
            })?
        }
        "app_settings" => {
            replace_simple_rows(transaction, table, workspace_id, rows, |tx, row, ws| {
                tx.execute("INSERT INTO app_settings(workspace_id,key,value_json,updated_at,version) VALUES (?1,?2,?3,?4,?5)", params![ws,text(row,"key"),row.get("value_json").cloned().unwrap_or(Value::Null).to_string(),millis(row,"updated_at"),integer(row,"version")])
            })?
        }
        "integration_configs" => {
            for row in rows {
                transaction.execute(
                    "INSERT INTO integration_configs(workspace_id,provider,enabled,config_json,secret_ref,updated_at,version)
                     VALUES (?1,?2,?3,?4,NULL,?5,?6)
                     ON CONFLICT(workspace_id,provider) DO UPDATE SET enabled = excluded.enabled, config_json = excluded.config_json, updated_at = excluded.updated_at, version = excluded.version",
                    params![workspace_id,text(&row,"provider"),boolean(&row,"enabled") as i64,row.get("config_json").cloned().unwrap_or_else(|| json!({})).to_string(),millis(&row,"updated_at"),integer(&row,"version")],
                ).map_err(|error| error.to_string())?;
            }
        }
        "external_bindings" => {
            replace_simple_rows(transaction, table, workspace_id, rows, |tx, row, ws| {
                tx.execute("INSERT INTO external_bindings(id,workspace_id,provider,entity_type,entity_id,external_id,external_revision,last_synced_hash,last_synced_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)", params![text(row,"id"),ws,text(row,"provider"),text(row,"entity_type"),text(row,"entity_id"),text(row,"external_id"),optional_text(row,"external_revision"),optional_text(row,"last_synced_hash"),optional_millis(row,"last_synced_at")])
            })?
        }
        _ => {}
    }
    Ok(())
}

fn replace_simple_rows<F>(
    transaction: &Transaction<'_>,
    table: &str,
    workspace_id: &str,
    rows: Vec<Value>,
    mut insert: F,
) -> Result<(), String>
where
    F: FnMut(&Transaction<'_>, &Value, &str) -> rusqlite::Result<usize>,
{
    let _ = workspace_id;
    for row in &rows {
        insert(transaction, row, workspace_id)
            .map_err(|error| format!("LOCAL_SNAPSHOT_APPLY_FAILED: {table}: {error}"))?;
    }
    Ok(())
}

fn restore_snapshot_time_entry_links(
    transaction: &Transaction<'_>,
    snapshot: &Value,
    workspace_id: &str,
) -> Result<(), String> {
    let rows = snapshot
        .get("time_entries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for row in rows {
        let Some(origin_unassigned_session_id) =
            optional_text(&row, "origin_unassigned_session_id")
        else {
            continue;
        };
        transaction
            .execute(
                "UPDATE time_entries SET origin_unassigned_session_id=?1 WHERE workspace_id=?2 AND id=?3",
                params![
                    origin_unassigned_session_id,
                    workspace_id,
                    text(&row, "id")
                ],
            )
            .map_err(|error| {
                format!(
                    "LOCAL_SNAPSHOT_APPLY_FAILED: time_entries.origin_unassigned_session_id: {error}"
                )
            })?;
    }
    Ok(())
}

fn clear_snapshot_cache(transaction: &Transaction<'_>, workspace_id: &str) -> Result<(), String> {
    transaction
        .execute(
            "UPDATE time_entries SET origin_unassigned_session_id = NULL WHERE workspace_id = ?1",
            [workspace_id],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "UPDATE time_entries SET previous_entry_id = NULL WHERE workspace_id = ?1",
            [workspace_id],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "UPDATE unassigned_sessions SET generated_entry_id = NULL WHERE workspace_id = ?1",
            [workspace_id],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "UPDATE tasks SET parent_id = NULL WHERE workspace_id = ?1",
            [workspace_id],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "DELETE FROM plan_import_items WHERE workspace_id = ?1",
            [workspace_id],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "DELETE FROM plan_import_batches WHERE workspace_id = ?1",
            [workspace_id],
        )
        .map_err(|error| error.to_string())?;
    for table in [
        "external_bindings",
        "integration_configs",
        "subject_work_schedule_segments",
        "subject_work_schedules",
        "report_tasks",
        "task_occurrences",
        "task_recurrence_rules",
        "task_day_plans",
        "task_daily_estimates",
        "time_allocations",
        "actual_time_adjustments",
        "time_segments",
        "unassigned_segments",
        "time_entries",
        "unassigned_sessions",
        "task_status_events",
        "reports",
        "report_templates",
        "tasks",
        "work_days",
        "app_settings",
        "subjects",
    ] {
        transaction
            .execute(
                &format!("DELETE FROM {table} WHERE workspace_id = ?1"),
                [workspace_id],
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}
fn optional_text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}
fn integer(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(Value::as_i64).unwrap_or(0)
}
fn optional_integer(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_i64)
}
fn boolean(value: &Value, key: &str) -> bool {
    value
        .get(key)
        .is_some_and(|value| value.as_bool().unwrap_or_else(|| value.as_i64() == Some(1)))
}
fn optional_time(value: &Value, key: &str) -> Option<String> {
    optional_text(value, key).map(|time| time.chars().take(5).collect())
}
fn millis(value: &Value, key: &str) -> i64 {
    optional_millis(value, key).unwrap_or(0)
}
fn optional_millis(value: &Value, key: &str) -> Option<i64> {
    let candidate = value.get(key)?;
    if candidate.is_null() {
        return None;
    }
    timestamp_value_millis(candidate)
}

fn timestamp_value_millis(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(parse_timestamp_millis))
}

fn parse_timestamp_millis(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|time| time.timestamp_millis())
        .or_else(|| {
            let normalized = normalize_postgrest_timestamp(value);
            (normalized != value).then(|| {
                chrono::DateTime::parse_from_rfc3339(&normalized)
                    .ok()
                    .map(|time| time.timestamp_millis())
            })?
        })
}

fn normalize_postgrest_timestamp(value: &str) -> String {
    let mut normalized = value.replace(' ', "T");
    if normalized.len() >= 3 {
        let suffix = &normalized[normalized.len() - 3..];
        if (suffix.starts_with('+') || suffix.starts_with('-'))
            && suffix[1..]
                .chars()
                .all(|character| character.is_ascii_digit())
        {
            normalized.push_str(":00");
        }
    }
    normalized
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[tauri::command]
pub fn cloud_sync_status(database: tauri::State<'_, Database>) -> Result<CloudSyncStatus, String> {
    sync_status(&database)
}

#[tauri::command]
pub fn cloud_sync_queue(
    database: tauri::State<'_, Database>,
) -> Result<Vec<CloudSyncQueueItem>, String> {
    let state = storage_mode(&database)?;
    if state.mode != "cloud" {
        return Ok(Vec::new());
    }
    list_sync_queue(&database, &state.workspace_id)
}

#[tauri::command]
pub fn cloud_sync_refresh(_database: tauri::State<'_, Database>) -> Result<(), String> {
    signal_cloud_sync(SYNC_SIGNAL_MANUAL_RETRY, None);
    Ok(())
}

#[tauri::command]
pub async fn cloud_sync_catch_up(
    database: tauri::State<'_, Database>,
    app: AppHandle,
) -> Result<CloudSyncPullResult, String> {
    let database = database.inner().clone();
    let database_for_query = database.clone();
    let result =
        tauri::async_runtime::spawn_blocking(move || catch_up_incremental(&database_for_query))
            .await
            .map_err(|error| format!("SYNC_WORKER_ERROR: 云端刷新任务异常: {error}"))?;
    if let Ok(catch_up) = &result {
        if !catch_up.domains.is_empty() {
            emit_cloud_refresh_domains(
                &app,
                &database,
                "catch-up",
                catch_up.last_change_seq,
                &catch_up.domains,
            );
        } else if let Ok(status) = sync_status(&database) {
            let _ = app.emit("cloud-sync-state-changed", &status);
        }
    }
    result
}

#[tauri::command]
pub async fn cloud_sync_save(
    database: tauri::State<'_, Database>,
    app: AppHandle,
) -> Result<CloudSyncPushResult, String> {
    let database = database.inner().clone();
    begin_cloud_save_activity();
    if let Ok(status) = sync_status(&database) {
        let _ = app.emit("cloud-sync-state-changed", &status);
    }
    let database_for_sync = database.clone();
    let worker_result = tauri::async_runtime::spawn_blocking(move || {
        push_outbox_with_retry_mode(&database_for_sync, true, true)
    })
    .await;
    end_cloud_save_activity();
    let result =
        worker_result.map_err(|error| format!("SYNC_WORKER_ERROR: 手动保存任务异常: {error}"))?;

    match result {
        Ok(result) => {
            if result.pending == 0 && result.conflicts == 0 {
                if let Ok(state) = storage_mode(&database) {
                    let _ = update_sync_state(
                        &database,
                        &state.workspace_id,
                        &state.device_id,
                        state.last_change_seq,
                        None,
                    );
                }
            }
            if let Ok(status) = sync_status(&database) {
                let _ = app.emit("cloud-sync-state-changed", &status);
            }
            Ok(result)
        }
        Err(error) => {
            emit_cloud_error(&app, &database, &error);
            Err(error)
        }
    }
}

#[tauri::command]
pub fn cloud_sync_reset_local_cache_preview(
    database: tauri::State<'_, Database>,
) -> Result<LocalCacheResetPreview, String> {
    preview_local_cache_reset(&database)
}

#[tauri::command]
pub fn cloud_sync_reset_local_cache(
    database: tauri::State<'_, Database>,
) -> Result<CloudSyncStatus, String> {
    reset_local_cache_from_cloud(&database)
}

#[tauri::command]
pub fn cloud_sync_discard_old_epoch_changes(
    database: tauri::State<'_, Database>,
) -> Result<CloudSyncStatus, String> {
    discard_old_epoch_changes_and_redownload(&database)
}

pub fn refresh_unassigned_state_for_cloud_mode(
    database: &Database,
) -> Result<Option<crate::unassigned::UnassignedStateDto>, String> {
    let state = storage_mode(database)?;
    if state.mode != "cloud" {
        return crate::unassigned::get_state(database);
    }
    let (result, sync_changed) =
        crate::unassigned::get_state_with_cloud_context_and_sync_change(database, Some(&state))?;
    if sync_changed {
        flush_if_online(database)?;
    }
    Ok(result)
}

#[tauri::command]
pub fn cloud_sync_conflicts(
    database: tauri::State<'_, Database>,
) -> Result<Vec<CloudSyncConflict>, String> {
    list_conflicts(&database)
}

#[tauri::command]
pub fn cloud_sync_resolve_conflict(
    database: tauri::State<'_, Database>,
    request: CloudConflictResolveRequest,
) -> Result<CloudSyncStatus, String> {
    resolve_conflict(&database, request)
}

#[tauri::command]
pub fn cloud_sync_resolve_all_conflicts(
    database: tauri::State<'_, Database>,
    request: CloudConflictResolveAllRequest,
) -> Result<CloudSyncStatus, String> {
    resolve_all_conflicts(&database, request)
}

#[tauri::command]
pub fn tracking_lease_acquire(database: tauri::State<'_, Database>) -> Result<Value, String> {
    acquire_lease(&database)
}

#[tauri::command]
pub fn tracking_lease_renew(
    database: tauri::State<'_, Database>,
    request: TrackingLeaseRequest,
) -> Result<Value, String> {
    renew_lease(&database, request)
}

#[tauri::command]
pub fn tracking_lease_release(
    database: tauri::State<'_, Database>,
    request: TrackingLeaseRequest,
) -> Result<Value, String> {
    release_lease(&database, request)
}

#[tauri::command]
pub fn tracking_lease_get(database: tauri::State<'_, Database>) -> Result<Value, String> {
    get_lease(&database)
}

#[tauri::command]
pub fn cloud_timer_start(
    app: tauri::AppHandle,
    database: tauri::State<'_, Database>,
    request: CloudTimerStartRequest,
) -> Result<crate::time_tracking::TimeEntryDto, String> {
    let result = legacy_cloud_timer_start_local_first(&database, request)?;
    let _ = crate::refresh_tray_presentation(&app, &database);
    Ok(result)
}

#[tauri::command]
pub fn cloud_timer_get_state(
    database: tauri::State<'_, Database>,
) -> Result<Option<crate::time_tracking::TimeEntryDto>, String> {
    crate::time_tracking::get_timer_state(&database)
}

#[tauri::command]
pub fn cloud_time_entry_list(
    database: tauri::State<'_, Database>,
    work_date: String,
) -> Result<crate::time_tracking::TimeEntryListResult, String> {
    crate::time_tracking::list_time_entries(
        &database,
        crate::time_tracking::TimeEntryListRequest {
            work_date,
            include_breaks: true,
        },
    )
}

#[tauri::command]
pub fn cloud_timer_pause(
    app: tauri::AppHandle,
    database: tauri::State<'_, Database>,
    request: CloudTimerVersionRequest,
) -> Result<crate::time_tracking::TimeEntryDto, String> {
    let result = legacy_cloud_timer_pause_local_first(&database, request)?;
    let _ = crate::refresh_tray_presentation(&app, &database);
    Ok(result)
}

#[tauri::command]
pub fn cloud_timer_resume(
    app: tauri::AppHandle,
    database: tauri::State<'_, Database>,
    request: CloudTimerVersionRequest,
) -> Result<crate::time_tracking::TimeEntryDto, String> {
    let result = legacy_cloud_timer_resume_local_first(&database, request)?;
    let _ = crate::refresh_tray_presentation(&app, &database);
    Ok(result)
}

#[tauri::command]
pub fn cloud_timer_stop(
    app: tauri::AppHandle,
    database: tauri::State<'_, Database>,
    request: CloudTimerVersionRequest,
) -> Result<crate::time_tracking::TimeEntryDto, String> {
    let result = legacy_cloud_timer_stop_local_first(&database, request)?;
    let _ = crate::refresh_tray_presentation(&app, &database);
    Ok(result)
}

#[tauri::command]
pub fn cloud_unassigned_get_state(
    database: tauri::State<'_, Database>,
) -> Result<Option<crate::unassigned::UnassignedStateDto>, String> {
    refresh_unassigned_state_for_cloud_mode(&database)
}

#[tauri::command]
pub fn cloud_unassigned_resolve_work(
    database: tauri::State<'_, Database>,
    request: CloudUnassignedResolveRequest,
) -> Result<crate::unassigned::UnassignedResolveResult, String> {
    let allocations = request
        .allocations
        .unwrap_or_default()
        .into_iter()
        .map(serde_json::from_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("VALIDATION_ERROR: 未归属时间分配格式无效: {error}"))?;
    crate::unassigned::resolve_work_for_sync(
        &database,
        crate::unassigned::ResolveWorkRequest {
            session_id: request.session_id,
            expected_version: request.expected_version,
            operation_id: request.operation_id,
            allocations,
        },
    )
}

#[tauri::command]
pub fn cloud_unassigned_resolve_break(
    database: tauri::State<'_, Database>,
    request: CloudUnassignedResolveRequest,
) -> Result<crate::unassigned::UnassignedResolveResult, String> {
    crate::unassigned::resolve_break_for_sync(
        &database,
        crate::unassigned::ResolveSessionRequest {
            session_id: request.session_id,
            expected_version: request.expected_version,
            operation_id: request.operation_id,
        },
    )
}

#[tauri::command]
pub fn cloud_unassigned_discard(
    database: tauri::State<'_, Database>,
    request: CloudUnassignedResolveRequest,
) -> Result<crate::unassigned::UnassignedResolveResult, String> {
    crate::unassigned::discard_for_sync(
        &database,
        crate::unassigned::ResolveSessionRequest {
            session_id: request.session_id,
            expected_version: request.expected_version,
            operation_id: request.operation_id,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{self, SettingsScope, SettingsUpdate};
    use crate::subjects::{create_subject, SubjectCreateRequest};
    use crate::tasks::{create_task, TaskCreateRequest};
    use crate::time_tracking::{self, TimerStartRequest};
    use tempfile::tempdir;

    #[test]
    fn realtime_url_uses_secure_websocket_and_encoded_key() {
        let url = realtime_url("https://example.supabase.co", "anon key+/=").unwrap();
        assert_eq!(url.scheme(), "wss");
        assert_eq!(url.path(), "/realtime/v1/websocket");
        let query = url
            .query_pairs()
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(
            query.get("apikey").map(|value| value.as_ref()),
            Some("anon key+/=")
        );
        assert_eq!(query.get("vsn").map(|value| value.as_ref()), Some("1.0.0"));

        let local_url = realtime_url("http://127.0.0.1:54321", "local-key").unwrap();
        assert_eq!(local_url.scheme(), "ws");
        assert_eq!(local_url.host_str(), Some("127.0.0.1"));
    }

    #[test]
    fn actual_adjustment_baseline_error_is_a_sync_conflict() {
        assert!(is_conflict_error(
            "CLOUD_REQUEST_FAILED: ADJUSTMENT_BASELINE_CONFLICT: actual adjustment baseline changed"
        ));
        assert!(is_conflict_error(
            "CLOUD_REQUEST_FAILED: VERSION_CONFLICT: actual adjustment task version changed"
        ));
        assert!(!is_conflict_error(
            "CLOUD_REQUEST_FAILED: network request timed out"
        ));
    }

    fn incremental_entity(
        entity_type: &str,
        source_entity_type: &str,
        entity_id: impl Into<String>,
        deleted: bool,
        data: Option<Value>,
    ) -> IncrementalEntity {
        IncrementalEntity {
            entity_type: entity_type.to_string(),
            source_entity_type: source_entity_type.to_string(),
            entity_id: entity_id.into(),
            change_seq: 1,
            deleted,
            data,
        }
    }

    fn cloud_database_for_incremental_test(
        file_name: &str,
    ) -> (tempfile::TempDir, Database, String, String, String) {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(directory.path().join(file_name)).unwrap();
        let cloud_workspace_id = Uuid::now_v7().to_string();
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "cloud_workspace_id".to_string(),
                value: json!(cloud_workspace_id.clone()),
            },
        )
        .unwrap();
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "storage_mode".to_string(),
                value: json!("cloud"),
            },
        )
        .unwrap();
        let connection = database.open().unwrap();
        let local_workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let subject_id: String = connection
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        drop(connection);
        (
            directory,
            database,
            cloud_workspace_id,
            local_workspace_id,
            subject_id,
        )
    }

    #[test]
    fn isolated_cloud_caches_converge_history_without_creating_outbox_work() {
        let (_directory_a, database_a, cloud_workspace_a, _local_workspace_a, _subject_a) =
            cloud_database_for_incremental_test("history-cache-device-a.sqlite3");
        let (_directory_b, database_b, _initial_cloud_workspace_b, _local_workspace_b, _subject_b) =
            cloud_database_for_incremental_test("history-cache-device-b.sqlite3");
        settings::update_setting(
            &database_b,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "cloud_workspace_id".to_string(),
                value: json!(cloud_workspace_a.clone()),
            },
        )
        .unwrap();
        let cloud_workspace_b = cloud_workspace_a.clone();
        let device_a = storage_mode(&database_a).unwrap().device_id;
        let device_b = storage_mode(&database_b).unwrap().device_id;
        let entry_id = Uuid::now_v7().to_string();
        let started_at = 1_780_278_600_000_i64;
        let version_one = json!({
            "id": entry_id,
            "work_date": "2026-05-30",
            "kind": "work",
            "source_type": "manual",
            "state": "ended",
            "default_task_id": null,
            "label_snapshot": "跨设备历史记录",
            "started_at": started_at,
            "ended_at": started_at + 600_000,
            "duration_seconds": 600,
            "note": "设备 A 创建",
            "origin_unassigned_session_id": null,
            "created_at": started_at,
            "updated_at": started_at + 600_000,
            "version": 1,
            "deleted_at": null,
            "segments": [],
            "allocations": []
        });
        for (database, cloud_workspace, device_id) in [
            (&database_a, &cloud_workspace_a, &device_a),
            (&database_b, &cloud_workspace_b, &device_b),
        ] {
            apply_incremental_entities(
                database,
                cloud_workspace,
                device_id,
                1,
                &[incremental_entity(
                    "time_entries",
                    "time_entries",
                    entry_id.clone(),
                    false,
                    Some(version_one.clone()),
                )],
            )
            .unwrap();
        }
        for day in 23..=29 {
            let additional_id = Uuid::now_v7().to_string();
            let additional = json!({
                "id": additional_id,
                "work_date": format!("2026-05-{day:02}"),
                "kind": "work",
                "source_type": "manual",
                "state": "ended",
                "default_task_id": null,
                "label_snapshot": format!("历史日期 {day}"),
                "started_at": started_at - i64::from(30 - day) * 86_400_000,
                "ended_at": started_at - i64::from(30 - day) * 86_400_000 + 600_000,
                "duration_seconds": 600,
                "note": null,
                "origin_unassigned_session_id": null,
                "created_at": started_at,
                "updated_at": started_at,
                "version": 1,
                "deleted_at": null,
                "segments": [],
                "allocations": []
            });
            for (database, cloud_workspace, device_id) in [
                (&database_a, &cloud_workspace_a, &device_a),
                (&database_b, &cloud_workspace_b, &device_b),
            ] {
                apply_incremental_entities(
                    database,
                    cloud_workspace,
                    device_id,
                    i64::from(day - 21),
                    &[incremental_entity(
                        "time_entries",
                        "time_entries",
                        additional_id.clone(),
                        false,
                        Some(additional.clone()),
                    )],
                )
                .unwrap();
            }
        }
        let version_two = json!({
            "id": entry_id,
            "work_date": "2026-05-30",
            "kind": "work",
            "source_type": "manual",
            "state": "ended",
            "default_task_id": null,
            "label_snapshot": "跨设备历史记录",
            "started_at": started_at,
            "ended_at": started_at + 1_200_000,
            "duration_seconds": 1_200,
            "note": "设备 B 已修正",
            "origin_unassigned_session_id": null,
            "created_at": started_at,
            "updated_at": started_at + 1_200_000,
            "version": 2,
            "deleted_at": null,
            "segments": [],
            "allocations": []
        });
        for (database, cloud_workspace, device_id) in [
            (&database_a, &cloud_workspace_a, &device_a),
            (&database_b, &cloud_workspace_b, &device_b),
        ] {
            apply_incremental_entities(
                database,
                cloud_workspace,
                device_id,
                2,
                &[incremental_entity(
                    "time_entries",
                    "time_entries",
                    entry_id.clone(),
                    false,
                    Some(version_two.clone()),
                )],
            )
            .unwrap();
        }
        let outbox_before = |database: &Database| -> i64 {
            database
                .open()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM sync_outbox", [], |row| row.get(0))
                .unwrap()
        };
        let before_a = outbox_before(&database_a);
        let before_b = outbox_before(&database_b);
        let history_a = time_tracking::list_time_entry_history(
            &database_a,
            time_tracking::TimeEntryHistoryRequest {
                page: 1,
                include_breaks: true,
            },
        )
        .unwrap();
        let history_b = time_tracking::list_time_entry_history(
            &database_b,
            time_tracking::TimeEntryHistoryRequest {
                page: 1,
                include_breaks: true,
            },
        )
        .unwrap();
        let older_a = time_tracking::list_time_entry_history(
            &database_a,
            time_tracking::TimeEntryHistoryRequest {
                page: 2,
                include_breaks: true,
            },
        )
        .unwrap();
        let older_b = time_tracking::list_time_entry_history(
            &database_b,
            time_tracking::TimeEntryHistoryRequest {
                page: 2,
                include_breaks: true,
            },
        )
        .unwrap();
        let comparable = |history: time_tracking::TimeEntryHistoryResult| {
            let mut value = serde_json::to_value(history).unwrap();
            value.as_object_mut().unwrap().remove("revision");
            value
        };
        assert_eq!(comparable(history_a), comparable(history_b));
        assert_eq!(comparable(older_a), comparable(older_b));
        let refreshed = time_tracking::list_time_entry_history(
            &database_a,
            time_tracking::TimeEntryHistoryRequest {
                page: 1,
                include_breaks: true,
            },
        )
        .unwrap();
        assert_eq!(refreshed.groups[0].raw_minutes, 20);
        assert_eq!(refreshed.total_days, 8);
        assert_eq!(refreshed.total_pages, 2);
        assert_eq!(refreshed.groups.len(), 7);
        assert_eq!(refreshed.groups[0].entries[0].version, 2);
        assert_eq!(
            refreshed.groups[0].entries[0].note.as_deref(),
            Some("设备 B 已修正")
        );
        assert_eq!(outbox_before(&database_a), before_a);
        assert_eq!(outbox_before(&database_b), before_b);
        assert_eq!(
            storage_mode(&database_a).unwrap().pending_operations,
            before_a
        );
        assert_eq!(
            storage_mode(&database_b).unwrap().pending_operations,
            before_b
        );
        assert!(!storage_mode(&database_a).unwrap().online);
        assert!(!storage_mode(&database_b).unwrap().online);
    }

    #[test]
    fn authoritative_calendar_timer_replaces_conflicting_local_candidate_and_redirects_dependents()
    {
        let (_directory, database, cloud_workspace_id, local_workspace_id, _subject_id) =
            cloud_database_for_incremental_test("calendar-timer-authoritative.sqlite3");
        let connection = database.open().unwrap();
        let device_id: String = connection
            .query_row(
                "SELECT id FROM devices WHERE workspace_id=?1 LIMIT 1",
                [&local_workspace_id],
                |row| row.get(0),
            )
            .unwrap();
        let candidate_id = Uuid::now_v7().to_string();
        let authoritative_id = Uuid::now_v7().to_string();
        let chain_id = Uuid::now_v7().to_string();
        let task_id: String = connection
            .query_row(
                "SELECT id FROM tasks WHERE workspace_id=?1 AND deleted_at IS NULL LIMIT 1",
                [&local_workspace_id],
                |row| row.get(0),
            )
            .unwrap_or_else(|_| {
                let subject_id: String = connection.query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0)).unwrap();
                let id=Uuid::now_v7().to_string();
                connection.execute("INSERT INTO tasks(id,workspace_id,subject_id,title,status,source_type,sort_order,created_at,updated_at,version) VALUES (?1,?2,?3,'候选映射','open','manual',1,1,1,1)",params![id,local_workspace_id,subject_id]).unwrap();
                id
            });
        connection.execute("INSERT INTO time_entries(id,workspace_id,work_date,kind,source_type,state,default_task_id,label_snapshot,started_at,ended_at,duration_seconds,created_at,updated_at,version,timer_chain_id,last_continuous_at) VALUES (?1,?2,'2026-10-06','work','timer','ended',?3,'本地候选',1000,61000,60,1000,61000,1,?4,61000)",params![candidate_id,local_workspace_id,task_id,chain_id]).unwrap();
        connection.execute("INSERT INTO time_segments(id,workspace_id,entry_id,sequence_no,started_at,ended_at,duration_seconds) VALUES (?1,?2,?3,1,1000,61000,60)",params![Uuid::now_v7().to_string(),local_workspace_id,candidate_id]).unwrap();
        connection.execute("INSERT INTO time_allocations(id,workspace_id,entry_id,task_id,minutes,created_at,updated_at,version) VALUES (?1,?2,?3,?4,1,61000,61000,1)",params![Uuid::now_v7().to_string(),local_workspace_id,candidate_id,task_id]).unwrap();
        let operation = OutboxOperation {
            operation_id: Uuid::now_v7().to_string(),
            workspace_epoch: 1,
            operation_type: "timer_rollover_create".to_string(),
            entity_type: "time_entry".to_string(),
            entity_id: Some(candidate_id.clone()),
            base_version: None,
            payload_version: Some(1),
            coalesced_count: 1,
            depends_on_operation_id: None,
            payload: json!({}),
        };
        connection.execute("INSERT INTO sync_outbox(operation_id,workspace_id,device_id,operation_type,entity_type,entity_id,payload_json,state,attempt_count,created_at) VALUES (?1,?2,?3,'allocation_followup','time_entry',?4,'{}','pending',0,2)",params![Uuid::now_v7().to_string(),cloud_workspace_id,device_id,candidate_id]).unwrap();
        drop(connection);
        adopt_authoritative_calendar_timer_result(&database,&operation,&json!({
            "superseded":true,
            "authoritative":{
                "id":authoritative_id,"work_date":"2026-10-06","kind":"work","source_type":"timer","state":"ended",
                "default_task_id":task_id,"label_snapshot":"云端权威","started_at":1000,"ended_at":61000,"duration_seconds":60,
                "created_at":1000,"updated_at":61000,"version":1,"timer_chain_id":chain_id,"last_continuous_at":61000,
                "segments":[],"allocations":[]
            }
        })).unwrap();
        let connection = database.open().unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM time_entries WHERE id=?1",
                    [candidate_id],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM time_entries WHERE id=?1",
                    [authoritative_id.clone()],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT entity_id FROM sync_outbox WHERE operation_type='allocation_followup'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            authoritative_id
        );
    }

    #[test]
    fn incremental_protocol_rejects_unknown_sources_invalid_keys_and_incomplete_tombstones() {
        let id = Uuid::now_v7().to_string();
        assert!(validate_incremental_identity(&incremental_entity(
            "unknown_entities",
            "unknown_entities",
            id.clone(),
            true,
            None,
        ))
        .unwrap_err()
        .starts_with("VALIDATION_ERROR:"));
        assert!(validate_incremental_identity(&incremental_entity(
            "task_daily_estimates",
            "task_daily_estimates",
            format!("{id}|2026-02-30"),
            true,
            None,
        ))
        .is_err());
        assert!(validate_incremental_identity(&incremental_entity(
            "time_entries",
            "time_segments",
            id.clone(),
            true,
            Some(json!({ "id": id })),
        ))
        .unwrap_err()
        .contains("删除增量"));
        let missing_deleted = serde_json::from_value::<IncrementalEntity>(json!({
            "entity_type": "tasks",
            "source_entity_type": "tasks",
            "entity_id": id,
            "data": null
        }));
        assert!(missing_deleted.is_err());
    }

    #[test]
    fn incremental_domains_cover_planning_time_and_reports() {
        assert_eq!(
            incremental_domain("subject_work_schedules").unwrap(),
            "subjects"
        );
        assert_eq!(incremental_domain("task_daily_estimates").unwrap(), "tasks");
        assert_eq!(incremental_domain("task_day_plans").unwrap(), "tasks");
        assert_eq!(
            incremental_domain("task_recurrence_rules").unwrap(),
            "tasks"
        );
        assert_eq!(incremental_domain("task_occurrences").unwrap(), "tasks");
        assert_eq!(incremental_domain("time_entries").unwrap(), "time");
        assert_eq!(incremental_domain("reports").unwrap(), "reports");
    }

    #[test]
    fn subject_work_schedule_incremental_replaces_complete_day_image() {
        let (_directory, database, cloud_workspace_id, local_workspace_id, subject_id) =
            cloud_database_for_incremental_test("schedule-incremental.sqlite3");
        let connection = database.open().unwrap();
        let schedule_id: String = connection
            .query_row(
                "SELECT id FROM subject_work_schedules WHERE workspace_id=?1 AND subject_id=?2 AND weekday=0",
                params![local_workspace_id, subject_id],
                |row| row.get(0),
            )
            .unwrap();
        drop(connection);
        let entity = incremental_entity(
            "subject_work_schedules",
            "subject_work_schedules",
            schedule_id.clone(),
            false,
            Some(json!({
                "id": schedule_id,
                "workspace_id": cloud_workspace_id,
                "subject_id": subject_id,
                "weekday": 0,
                "is_workday": true,
                "created_at": 1_000,
                "updated_at": 2_000,
                "version": 2,
                "segments": [
                    {"id": Uuid::now_v7().to_string(), "sequence_no": 1, "start_minute": 540, "end_minute": 720},
                    {"id": Uuid::now_v7().to_string(), "sequence_no": 2, "start_minute": 810, "end_minute": 1020}
                ]
            })),
        );
        let domains = apply_incremental_entities(
            &database,
            &cloud_workspace_id,
            &storage_mode(&database).unwrap().device_id,
            1,
            &[entity],
        )
        .unwrap();
        assert_eq!(domains, vec!["subjects"]);
        let schedule = crate::subject_work_schedules::get_schedule(&database, &subject_id).unwrap();
        assert_eq!(schedule[0].segments[0].start, "09:00");
        assert_eq!(schedule[0].segments[1].end, "17:00");
        assert_eq!(schedule[0].version, 2);
    }

    #[test]
    fn incremental_batch_defers_dirty_entity_but_applies_other_entities_and_advances_cursor() {
        let (_directory, database, cloud_workspace_id, _local_workspace_id, subject_id) =
            cloud_database_for_incremental_test("incremental-dirty.sqlite3");
        let device_id = storage_mode(&database).unwrap().device_id;
        let dirty_task_id = Uuid::now_v7().to_string();
        let clean_task_id = Uuid::now_v7().to_string();
        let now = now_millis();
        database.open().unwrap().execute(
            "INSERT INTO sync_outbox(operation_id,workspace_id,device_id,operation_type,entity_type,entity_id,payload_json,state,attempt_count,created_at) VALUES (?1,?2,?3,'task.update','task',?4,'{}','pending',0,?5)",
            params![Uuid::now_v7().to_string(), cloud_workspace_id, device_id, dirty_task_id, now],
        ).unwrap();
        let task_data = |id: &str, title: &str| {
            json!({
                "id": id, "subject_id": subject_id, "parent_id": null, "title": title,
                "status": "open", "planned_date": null, "planned_time": null,
                "estimate_minutes": null, "note": null, "project_name": null,
                "solution_name": null, "source_type": "manual", "source_ref": null,
                "sort_order": 20, "completed_at": null, "created_at": now,
                "updated_at": now, "version": 2, "deleted_at": null
            })
        };
        let mut dirty_entity = incremental_entity(
            "tasks",
            "tasks",
            dirty_task_id.clone(),
            false,
            Some(task_data(&dirty_task_id, "本机待同步")),
        );
        dirty_entity.change_seq = 37;
        let entities = vec![
            dirty_entity,
            incremental_entity(
                "tasks",
                "tasks",
                clean_task_id.clone(),
                false,
                Some(task_data(&clean_task_id, "远端可应用")),
            ),
        ];

        let domains =
            apply_incremental_entities(&database, &cloud_workspace_id, &device_id, 42, &entities)
                .unwrap();
        let connection = database.open().unwrap();
        assert_eq!(domains, vec!["tasks"]);
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM tasks WHERE id=?1",
                    [&dirty_task_id],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT title FROM tasks WHERE id=?1",
                    [&clean_task_id],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "远端可应用"
        );
        assert_eq!(connection.query_row("SELECT observed_change_seq FROM cloud_deferred_entities WHERE workspace_id=?1 AND entity_type='tasks' AND entity_id=?2", params![cloud_workspace_id, dirty_task_id], |row| row.get::<_, i64>(0)).unwrap(), 37);
        assert_eq!(
            connection
                .query_row(
                    "SELECT last_change_seq FROM local_sync_state WHERE workspace_id=?1",
                    [&cloud_workspace_id],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            42
        );
    }

    #[test]
    fn incremental_task_parent_is_restored_after_child_before_parent_batch() {
        let (_directory, database, cloud_workspace_id, _local_workspace_id, subject_id) =
            cloud_database_for_incremental_test("incremental-parent-order.sqlite3");
        let device_id = storage_mode(&database).unwrap().device_id;
        let parent_id = Uuid::now_v7().to_string();
        let child_id = Uuid::now_v7().to_string();
        let now = now_millis();
        let task_data = |id: &str, parent: Option<&str>, title: &str| {
            json!({
                "id": id, "subject_id": subject_id, "parent_id": parent, "title": title,
                "status": "open", "planned_date": null, "planned_time": null,
                "estimate_minutes": null, "note": null, "project_name": null,
                "solution_name": null, "source_type": "manual", "source_ref": null,
                "sort_order": 20, "completed_at": null, "created_at": now,
                "updated_at": now, "version": 1, "deleted_at": null
            })
        };
        apply_incremental_entities(
            &database,
            &cloud_workspace_id,
            &device_id,
            7,
            &[
                incremental_entity(
                    "tasks",
                    "tasks",
                    child_id.clone(),
                    false,
                    Some(task_data(&child_id, Some(&parent_id), "子事项")),
                ),
                incremental_entity(
                    "tasks",
                    "tasks",
                    parent_id.clone(),
                    false,
                    Some(task_data(&parent_id, None, "父事项")),
                ),
            ],
        )
        .unwrap();
        let saved_parent: Option<String> = database
            .open()
            .unwrap()
            .query_row(
                "SELECT parent_id FROM tasks WHERE id=?1",
                [&child_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(saved_parent.as_deref(), Some(parent_id.as_str()));
    }

    #[test]
    fn duplicate_incremental_entity_advances_cursor_without_frontend_domain_or_revision() {
        let (_directory, database, cloud_workspace_id, _local_workspace_id, subject_id) =
            cloud_database_for_incremental_test("incremental-idempotent.sqlite3");
        let device_id = storage_mode(&database).unwrap().device_id;
        let task_id = Uuid::now_v7().to_string();
        let now = now_millis();
        let task = json!({"id":task_id,"subject_id":subject_id,"parent_id":null,"title":"幂等事项","status":"open","planned_date":null,"planned_time":null,"estimate_minutes":null,"note":null,"project_name":null,"solution_name":null,"source_type":"manual","source_ref":null,"sort_order":1,"completed_at":null,"created_at":now,"updated_at":now,"version":1,"deleted_at":null});
        let entity = incremental_entity("tasks", "tasks", task_id, false, Some(task));
        assert_eq!(
            apply_incremental_entities(
                &database,
                &cloud_workspace_id,
                &device_id,
                1,
                std::slice::from_ref(&entity)
            )
            .unwrap(),
            vec!["tasks"]
        );
        let revision_before = local_revision(&database).unwrap();
        assert!(apply_incremental_entities(
            &database,
            &cloud_workspace_id,
            &device_id,
            2,
            &[entity]
        )
        .unwrap()
        .is_empty());
        assert_eq!(local_revision(&database).unwrap(), revision_before);
        assert_eq!(storage_mode(&database).unwrap().last_change_seq, 2);
    }

    #[test]
    fn realtime_change_seq_accepts_supabase_payload_shapes() {
        assert_eq!(
            realtime_change_seq(
                &json!({ "payload": { "data": { "record": { "change_seq": 42 } } } })
            ),
            Some(42)
        );
        assert_eq!(
            realtime_change_seq(&json!({ "payload": { "record": { "change_seq": "43" } } })),
            Some(43)
        );
        assert_eq!(realtime_change_seq(&json!({ "payload": {} })), None);
    }

    #[test]
    fn realtime_invalidation_contains_only_refresh_coordinates() {
        let invalidation = realtime_workspace_invalidation(
            &json!({
                "event": "postgres_changes",
                "payload": {
                    "data": {
                        "record": {
                            "change_seq": 42,
                            "workspace_id": "workspace-1",
                            "entity_type": "tasks",
                            "entity_id": "task-secret-id",
                            "title": "不应出现在通知中的业务实体"
                        }
                    }
                }
            }),
            "workspace-1",
            7,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(invalidation).unwrap(),
            json!({
                "workspaceId": "workspace-1",
                "changeSeq": 42,
                "workspaceEpoch": 7,
                "domains": ["tasks"],
                "source": "realtime"
            })
        );
    }

    #[test]
    fn realtime_entity_types_map_to_all_business_refresh_domains() {
        let cases: &[(&str, &str)] = &[
            ("subjects", "subjects"),
            ("subject_work_schedules", "subjects"),
            ("tasks", "tasks"),
            ("task_status_events", "tasks"),
            ("task_daily_estimates", "tasks"),
            ("task_day_plans", "tasks"),
            ("task_recurrence_rules", "tasks"),
            ("task_occurrences", "tasks"),
            ("work_days", "time"),
            ("time_entries", "time"),
            ("time_segments", "time"),
            ("time_allocations", "time"),
            ("actual_time_adjustments", "time"),
            ("actual_adjustment_batches", "time"),
            ("unassigned_sessions", "unassigned"),
            ("unassigned_segments", "unassigned"),
            ("report_templates", "reports"),
            ("reports", "reports"),
            ("report_tasks", "reports"),
            ("app_settings", "settings"),
            ("integration_configs", "settings"),
            ("external_bindings", "settings"),
        ];
        for (entity_type, domain) in cases {
            assert_eq!(
                realtime_refresh_domains(entity_type),
                vec![*domain],
                "unexpected domain for {entity_type}"
            );
        }
        assert!(realtime_refresh_domains("future_entity").is_empty());
    }

    #[test]
    fn realtime_invalidation_rejects_other_workspaces_and_unknown_entities() {
        let other_workspace = json!({
            "payload": {"record": {
                "change_seq": 1,
                "workspace_id": "workspace-2",
                "entity_type": "tasks"
            }}
        });
        assert!(realtime_workspace_invalidation(&other_workspace, "workspace-1", 1).is_none());

        let unsupported_entity = json!({
            "payload": {"record": {
                "change_seq": 1,
                "workspace_id": "workspace-1",
                "entity_type": "future_entity"
            }}
        });
        assert!(realtime_workspace_invalidation(&unsupported_entity, "workspace-1", 1).is_none());
    }

    #[test]
    fn direct_cloud_catch_up_invalidates_every_business_domain() {
        assert_eq!(
            direct_cloud_catch_up_domains(),
            vec![
                "subjects",
                "tasks",
                "time",
                "unassigned",
                "reports",
                "settings",
            ]
        );
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

    #[test]
    fn sync_signals_merge_without_queue_growth() {
        let _guard = CLOUD_SYNC_TEST_STATE_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        CLOUD_SYNC_SIGNALS.store(0, Ordering::Release);
        CLOUD_SYNC_OBSERVED_REMOTE_SEQ.store(0, Ordering::Release);
        CLOUD_SYNC_ACTIVE.store(false, Ordering::Release);
        signal_cloud_sync(SYNC_SIGNAL_LOCAL_DIRTY, None);
        signal_cloud_sync(SYNC_SIGNAL_REMOTE_HINT, Some(7));
        signal_cloud_sync(SYNC_SIGNAL_REMOTE_HINT, Some(5));
        signal_cloud_sync(SYNC_SIGNAL_MANUAL_RETRY, None);
        let signals = CLOUD_SYNC_SIGNALS.swap(0, Ordering::AcqRel);
        assert_eq!(signals & SYNC_SIGNAL_LOCAL_DIRTY, SYNC_SIGNAL_LOCAL_DIRTY);
        assert_eq!(signals & SYNC_SIGNAL_REMOTE_HINT, SYNC_SIGNAL_REMOTE_HINT);
        assert_eq!(signals & SYNC_SIGNAL_MANUAL_RETRY, SYNC_SIGNAL_MANUAL_RETRY);
        assert_eq!(CLOUD_SYNC_OBSERVED_REMOTE_SEQ.load(Ordering::Acquire), 7);
        assert!(CLOUD_SYNC_ACTIVE.load(Ordering::Acquire));
        CLOUD_SYNC_ACTIVE.store(false, Ordering::Release);
    }

    #[test]
    fn transport_activity_does_not_report_saving_without_an_outbox_push() {
        let _guard = CLOUD_SYNC_TEST_STATE_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("cloud-transport-vs-saving.sqlite3"))
                .unwrap();
        CLOUD_SYNC_ACTIVE.store(true, Ordering::Release);
        CLOUD_SAVE_ACTIVITY.store(0, Ordering::Release);

        let pulling = sync_status(&database).unwrap();
        assert!(pulling.syncing);
        assert!(!pulling.saving);

        begin_cloud_save_activity();
        let pushing = sync_status(&database).unwrap();
        assert!(pushing.syncing);
        assert!(pushing.saving);

        end_cloud_save_activity();
        CLOUD_SYNC_ACTIVE.store(false, Ordering::Release);
        let settled = sync_status(&database).unwrap();
        assert!(!settled.syncing);
        assert!(!settled.saving);
    }

    #[test]
    fn concurrent_signal_storm_is_coalesced_without_losing_late_work() {
        use std::sync::atomic::AtomicUsize;
        use std::thread;

        let _guard = CLOUD_SYNC_TEST_STATE_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        CLOUD_SYNC_SIGNALS.store(0, Ordering::Release);
        CLOUD_SYNC_OBSERVED_REMOTE_SEQ.store(0, Ordering::Release);
        CLOUD_SYNC_ACTIVE.store(false, Ordering::Release);
        let producer_count = 8;
        let done = Arc::new(AtomicUsize::new(0));
        let mut producers = Vec::new();
        for producer in 0..producer_count {
            let done = Arc::clone(&done);
            producers.push(thread::spawn(move || {
                for index in 0..1_000 {
                    let signal = match index % 4 {
                        0 => SYNC_SIGNAL_LOCAL_DIRTY,
                        1 => SYNC_SIGNAL_REMOTE_HINT,
                        2 => SYNC_SIGNAL_CONNECTION_READY,
                        _ => SYNC_SIGNAL_MANUAL_RETRY,
                    };
                    let observed = (signal == SYNC_SIGNAL_REMOTE_HINT)
                        .then_some((producer * 1_000 + index + 1) as i64);
                    signal_cloud_sync(signal, observed);
                }
                done.fetch_add(1, Ordering::Release);
            }));
        }

        let mut observed_signals = 0_u8;
        let mut observed_remote_seq = 0_i64;
        while done.load(Ordering::Acquire) < producer_count
            || CLOUD_SYNC_SIGNALS.load(Ordering::Acquire) != 0
        {
            observed_signals |= CLOUD_SYNC_SIGNALS.swap(0, Ordering::AcqRel);
            observed_remote_seq =
                observed_remote_seq.max(CLOUD_SYNC_OBSERVED_REMOTE_SEQ.swap(0, Ordering::AcqRel));
            thread::yield_now();
        }
        for producer in producers {
            producer.join().unwrap();
        }
        observed_signals |= CLOUD_SYNC_SIGNALS.swap(0, Ordering::AcqRel);
        observed_remote_seq =
            observed_remote_seq.max(CLOUD_SYNC_OBSERVED_REMOTE_SEQ.swap(0, Ordering::AcqRel));

        assert_eq!(
            observed_signals,
            SYNC_SIGNAL_LOCAL_DIRTY
                | SYNC_SIGNAL_REMOTE_HINT
                | SYNC_SIGNAL_CONNECTION_READY
                | SYNC_SIGNAL_MANUAL_RETRY
        );
        assert_eq!(observed_remote_seq, 7_998);
        assert_eq!(CLOUD_SYNC_SIGNALS.load(Ordering::Acquire), 0);
        CLOUD_SYNC_ACTIVE.store(false, Ordering::Release);
    }

    #[test]
    fn deferred_entity_rewinds_cursor_after_matching_outbox_is_removed() {
        let (_directory, database, cloud_workspace_id, _local_workspace_id, _subject_id) =
            cloud_database_for_incremental_test("deferred-rewind.sqlite3");
        let state = storage_mode(&database).unwrap();
        let entity_id = Uuid::now_v7().to_string();
        let connection = database.open().unwrap();
        connection.execute(
            "INSERT INTO local_sync_state(workspace_id,device_id,last_change_seq,last_full_sync_at) VALUES (?1,?2,20,1) ON CONFLICT(workspace_id) DO UPDATE SET last_change_seq=20",
            params![cloud_workspace_id, state.device_id],
        ).unwrap();
        connection.execute(
            "INSERT INTO cloud_deferred_entities(workspace_id,entity_type,entity_id,observed_version,observed_change_seq,updated_at) VALUES (?1,'tasks',?2,3,12,1)",
            params![cloud_workspace_id, entity_id],
        ).unwrap();
        connection.execute(
            "INSERT INTO sync_outbox(operation_id,workspace_id,device_id,operation_type,entity_type,entity_id,payload_json,state,attempt_count,created_at) VALUES (?1,?2,?3,'task.update','task',?4,'{}','pending',0,1)",
            params![Uuid::now_v7().to_string(), cloud_workspace_id, state.device_id, entity_id],
        ).unwrap();
        drop(connection);

        rewind_for_ready_deferred_entities(&database, &cloud_workspace_id).unwrap();
        assert_eq!(storage_mode(&database).unwrap().last_change_seq, 20);
        database
            .open()
            .unwrap()
            .execute(
                "DELETE FROM sync_outbox WHERE workspace_id=?1",
                [&cloud_workspace_id],
            )
            .unwrap();
        rewind_for_ready_deferred_entities(&database, &cloud_workspace_id).unwrap();
        assert_eq!(storage_mode(&database).unwrap().last_change_seq, 11);
    }

    #[test]
    fn workspace_switch_removes_stale_deferred_entities() {
        let (_directory, database, cloud_workspace_id, _local_workspace_id, _subject_id) =
            cloud_database_for_incremental_test("deferred-workspace-switch.sqlite3");
        database.open().unwrap().execute(
            "INSERT INTO cloud_deferred_entities(workspace_id,entity_type,entity_id,observed_version,observed_change_seq,updated_at) VALUES ('old-workspace','tasks',?1,1,2,3), (?2,'tasks',?3,1,2,3)",
            params![Uuid::now_v7().to_string(), cloud_workspace_id, Uuid::now_v7().to_string()],
        ).unwrap();
        clear_stale_deferred_workspaces(&database, &cloud_workspace_id).unwrap();
        let rows: Vec<String> = database
            .open()
            .unwrap()
            .prepare("SELECT workspace_id FROM cloud_deferred_entities")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(rows, vec![cloud_workspace_id]);
    }

    #[test]
    fn outbox_retry_delay_is_one_shot_and_bounded() {
        let (_directory, database, cloud_workspace_id) =
            cloud_database_with_outbox_state("one-shot-retry.sqlite3", "pending");
        database
            .open()
            .unwrap()
            .execute(
                "UPDATE sync_outbox SET attempt_count=6,last_attempt_at=?1 WHERE workspace_id=?2",
                params![now_millis(), cloud_workspace_id],
            )
            .unwrap();
        let delay = next_outbox_retry_delay(&database).unwrap().unwrap();
        assert!(delay <= Duration::from_millis(OUTBOX_RETRY_MAX_BACKOFF_MILLIS as u64));
        assert!(delay > Duration::from_millis(1));
    }

    #[test]
    fn foreground_sync_wakeup_does_not_open_database_or_wait_for_network() {
        let _guard = CLOUD_SYNC_TEST_STATE_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        CLOUD_SYNC_ENABLED.store(true, Ordering::Release);
        CLOUD_SYNC_SIGNALS.store(0, Ordering::Release);
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("foreground-wakeup.sqlite3")).unwrap();
        std::fs::remove_file(directory.path().join("foreground-wakeup.sqlite3")).unwrap();
        let started = std::time::Instant::now();
        flush_if_online(&database).unwrap();
        assert!(started.elapsed() < Duration::from_millis(50));
        assert_ne!(
            CLOUD_SYNC_SIGNALS.load(Ordering::Acquire) & SYNC_SIGNAL_LOCAL_DIRTY,
            0
        );
        CLOUD_SYNC_ENABLED.store(false, Ordering::Release);
        CLOUD_SYNC_SIGNALS.store(0, Ordering::Release);
    }

    #[test]
    fn local_mode_business_wakeup_does_not_start_cloud_coordination() {
        let _guard = CLOUD_SYNC_TEST_STATE_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        disable_cloud_sync();
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("local-wakeup.sqlite3")).unwrap();
        assert!(!flush_if_online_with_signal_result(&database).unwrap());
    }

    #[test]
    fn direct_cloud_mode_keeps_realtime_but_stops_legacy_coordination() {
        let _guard = CLOUD_SYNC_TEST_STATE_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (directory, database, _) =
            cloud_database_with_outbox_state("direct-mode-workers.sqlite3", "pending");
        database
            .open()
            .unwrap()
            .execute(
                "INSERT INTO device_settings(key, value_json, updated_at)
                 VALUES('cloud_storage_protocol', '\"direct_v2\"', 1)
                 ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json, updated_at=excluded.updated_at",
                [],
            )
            .unwrap();
        database
            .open()
            .unwrap()
            .execute("DELETE FROM sync_outbox", [])
            .unwrap();

        enter_direct_cloud_mode();
        assert!(CLOUD_SYNC_ENABLED.load(Ordering::Acquire));
        assert!(!LEGACY_SYNC_ENABLED.load(Ordering::Acquire));
        CLOUD_SYNC_SIGNALS.store(SYNC_SIGNAL_LOCAL_DIRTY, Ordering::Release);
        assert_eq!(
            run_sync_coordinator_round_for_test(&database).unwrap(),
            "direct-cloud"
        );
        assert!(CLOUD_SYNC_SIGNALS.load(Ordering::Acquire) & SYNC_SIGNAL_LOCAL_DIRTY != 0);
        assert!(!LEGACY_SYNC_ENABLED.load(Ordering::Acquire));
        disable_cloud_sync();
        drop(directory);
    }

    #[test]
    fn incremental_planning_entities_apply_estimate_recurrence_and_occurrence() {
        let (_directory, database, cloud_workspace_id, _local_workspace_id, subject_id) =
            cloud_database_for_incremental_test("incremental-planning.sqlite3");
        let device_id = storage_mode(&database).unwrap().device_id;
        let task_id = Uuid::now_v7().to_string();
        let rule_id = Uuid::now_v7().to_string();
        let now = now_millis();
        let task = json!({
            "id":task_id,"subject_id":subject_id,"parent_id":null,"title":"重复事项","status":"open",
            "planned_date":null,"planned_time":null,"estimate_minutes":null,"note":null,"project_name":null,
            "solution_name":null,"source_type":"manual","source_ref":null,"sort_order":1,"completed_at":null,
            "created_at":now,"updated_at":now,"version":1,"deleted_at":null
        });
        let estimate = json!({"task_id":task_id,"work_date":"2026-10-06","estimate_minutes":45,"created_at":now,"updated_at":now,"version":1});
        let recurrence = json!({"task_id":task_id,"rules":[{"id":rule_id,"task_id":task_id,"frequency":"weekly","weekdays_mask":2,"effective_start":"2026-10-06","effective_end":null,"created_at":now,"updated_at":now,"version":1}]});
        let occurrence = json!({"task_id":task_id,"occurrence_date":"2026-10-06","origin":"scheduled","status":"done","completed_at":now,"created_at":now,"updated_at":now,"version":1});
        apply_incremental_entities(
            &database,
            &cloud_workspace_id,
            &device_id,
            10,
            &[
                incremental_entity("tasks", "tasks", task_id.clone(), false, Some(task)),
                incremental_entity(
                    "task_daily_estimates",
                    "task_daily_estimates",
                    format!("{task_id}|2026-10-06"),
                    false,
                    Some(estimate),
                ),
                incremental_entity(
                    "task_recurrence_rules",
                    "task_recurrence_rules",
                    task_id.clone(),
                    false,
                    Some(recurrence),
                ),
                incremental_entity(
                    "task_occurrences",
                    "task_occurrences",
                    format!("{task_id}|2026-10-06"),
                    false,
                    Some(occurrence),
                ),
            ],
        )
        .unwrap();
        let connection = database.open().unwrap();
        assert_eq!(connection.query_row("SELECT estimate_minutes FROM task_daily_estimates WHERE task_id=?1 AND work_date='2026-10-06'",[&task_id],|r|r.get::<_,i64>(0)).unwrap(),45);
        assert_eq!(
            connection
                .query_row(
                    "SELECT frequency FROM task_recurrence_rules WHERE task_id=?1",
                    [&task_id],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            "weekly"
        );
        assert_eq!(connection.query_row("SELECT status FROM task_occurrences WHERE task_id=?1 AND occurrence_date='2026-10-06'",[&task_id],|r|r.get::<_,String>(0)).unwrap(),"done");
    }

    #[test]
    fn incremental_day_plan_applies_overwrites_and_deletes_composite_entity() {
        let (_directory, database, cloud_workspace_id, _local_workspace_id, subject_id) =
            cloud_database_for_incremental_test("incremental-day-plan.sqlite3");
        let device_id = storage_mode(&database).unwrap().device_id;
        let task_id = Uuid::now_v7().to_string();
        let work_date = "2026-10-19";
        let now = now_millis();
        let task = json!({
            "id":task_id,"subject_id":subject_id,"parent_id":null,"title":"云端今日事项","status":"open",
            "planned_date":null,"planned_time":null,"estimate_minutes":null,"note":null,"project_name":null,
            "solution_name":null,"source_type":"manual","source_ref":null,"sort_order":1,"completed_at":null,
            "created_at":now,"updated_at":now,"version":1,"deleted_at":null
        });
        let entity_id = format!("{task_id}|{work_date}");
        let included = json!({"task_id":task_id,"work_date":work_date,"state":"included","created_at":now,"updated_at":now,"version":1});
        apply_incremental_entities(
            &database,
            &cloud_workspace_id,
            &device_id,
            2,
            &[
                incremental_entity("tasks", "tasks", task_id.clone(), false, Some(task)),
                incremental_entity(
                    "task_day_plans",
                    "task_day_plans",
                    entity_id.clone(),
                    false,
                    Some(included),
                ),
            ],
        )
        .unwrap();
        let included_state: (String, i64) = database
            .open()
            .unwrap()
            .query_row(
                "SELECT state,version FROM task_day_plans WHERE task_id=?1 AND work_date=?2",
                params![task_id, work_date],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(included_state, ("included".to_string(), 1));

        let excluded = json!({"task_id":task_id,"work_date":work_date,"state":"excluded","created_at":now,"updated_at":now + 1,"version":2});
        apply_incremental_entities(
            &database,
            &cloud_workspace_id,
            &device_id,
            3,
            &[incremental_entity(
                "task_day_plans",
                "task_day_plans",
                entity_id.clone(),
                false,
                Some(excluded),
            )],
        )
        .unwrap();
        let excluded_state: (String, i64) = database
            .open()
            .unwrap()
            .query_row(
                "SELECT state,version FROM task_day_plans WHERE task_id=?1 AND work_date=?2",
                params![task_id, work_date],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(excluded_state, ("excluded".to_string(), 2));

        apply_incremental_entities(
            &database,
            &cloud_workspace_id,
            &device_id,
            4,
            &[incremental_entity(
                "task_day_plans",
                "task_day_plans",
                entity_id,
                true,
                None,
            )],
        )
        .unwrap();
        let count: i64 = database
            .open()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM task_day_plans WHERE task_id=?1 AND work_date=?2",
                params![task_id, work_date],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn day_plan_and_estimate_reverse_arrival_orders_converge_to_membership() {
        let (_directory, database, cloud_workspace_id, _local_workspace_id, subject_id) =
            cloud_database_for_incremental_test("incremental-day-plan-order.sqlite3");
        let device_id = storage_mode(&database).unwrap().device_id;
        let work_date = "2026-10-19";
        let now = now_millis();
        let first_id = Uuid::now_v7().to_string();
        let second_id = Uuid::now_v7().to_string();
        let task = |id: &str, title: &str| {
            json!({
                "id":id,"subject_id":subject_id,"parent_id":null,"title":title,"status":"open",
                "planned_date":null,"planned_time":null,"estimate_minutes":null,"note":null,"project_name":null,
                "solution_name":null,"source_type":"manual","source_ref":null,"sort_order":1,"completed_at":null,
                "created_at":now,"updated_at":now,"version":1,"deleted_at":null
            })
        };
        apply_incremental_entities(
            &database,
            &cloud_workspace_id,
            &device_id,
            2,
            &[
                incremental_entity(
                    "tasks",
                    "tasks",
                    first_id.clone(),
                    false,
                    Some(task(&first_id, "预计先到")),
                ),
                incremental_entity(
                    "tasks",
                    "tasks",
                    second_id.clone(),
                    false,
                    Some(task(&second_id, "成员先到")),
                ),
            ],
        )
        .unwrap();
        let estimate = |id: &str| json!({"task_id":id,"work_date":work_date,"estimate_minutes":30,"created_at":now,"updated_at":now,"version":1});
        let plan = |id: &str| json!({"task_id":id,"work_date":work_date,"state":"included","created_at":now,"updated_at":now,"version":1});
        apply_incremental_entities(
            &database,
            &cloud_workspace_id,
            &device_id,
            3,
            &[
                incremental_entity(
                    "task_daily_estimates",
                    "task_daily_estimates",
                    format!("{first_id}|{work_date}"),
                    false,
                    Some(estimate(&first_id)),
                ),
                incremental_entity(
                    "task_day_plans",
                    "task_day_plans",
                    format!("{second_id}|{work_date}"),
                    false,
                    Some(plan(&second_id)),
                ),
            ],
        )
        .unwrap();
        let interim = crate::today_plans::snapshot(
            &database,
            crate::today_plans::TaskDayPlanSnapshotRequest {
                subject_id: subject_id.clone(),
                work_date: work_date.to_string(),
            },
        )
        .unwrap();
        assert!(interim
            .tasks
            .iter()
            .any(
                |task| task.day_instance_source_task_id.as_deref() == Some(first_id.as_str())
                    && task.today_member
            ));
        assert!(interim
            .tasks
            .iter()
            .any(
                |task| task.day_instance_source_task_id.as_deref() == Some(second_id.as_str())
                    && task.today_member
            ));

        apply_incremental_entities(
            &database,
            &cloud_workspace_id,
            &device_id,
            4,
            &[
                incremental_entity(
                    "task_day_plans",
                    "task_day_plans",
                    format!("{first_id}|{work_date}"),
                    false,
                    Some(plan(&first_id)),
                ),
                incremental_entity(
                    "task_daily_estimates",
                    "task_daily_estimates",
                    format!("{second_id}|{work_date}"),
                    false,
                    Some(estimate(&second_id)),
                ),
            ],
        )
        .unwrap();
        let final_snapshot = crate::today_plans::snapshot(
            &database,
            crate::today_plans::TaskDayPlanSnapshotRequest {
                subject_id,
                work_date: work_date.to_string(),
            },
        )
        .unwrap();
        for task_id in [&first_id, &second_id] {
            let row = final_snapshot
                .tasks
                .iter()
                .find(|task| task.day_instance_source_task_id.as_deref() == Some(task_id.as_str()))
                .unwrap();
            assert!(row.today_member);
            assert_eq!(row.day_plan_state.as_deref(), Some("included"));
            assert_eq!(row.today_estimate_minutes, Some(30));
        }
    }

    #[test]
    fn incremental_planning_tombstones_and_empty_recurrence_remove_local_rows() {
        let (_directory, database, cloud_workspace_id, _local_workspace_id, subject_id) =
            cloud_database_for_incremental_test("incremental-planning-delete.sqlite3");
        let device_id = storage_mode(&database).unwrap().device_id;
        let task_id = Uuid::now_v7().to_string();
        let rule_id = Uuid::now_v7().to_string();
        let now = now_millis();
        let task = json!({
            "id":task_id,"subject_id":subject_id,"parent_id":null,"title":"待清理规划","status":"open",
            "planned_date":null,"planned_time":null,"estimate_minutes":null,"note":null,"project_name":null,
            "solution_name":null,"source_type":"manual","source_ref":null,"sort_order":1,"completed_at":null,
            "created_at":now,"updated_at":now,"version":1,"deleted_at":null
        });
        let estimate = json!({"task_id":task_id,"work_date":"2026-10-06","estimate_minutes":45,"created_at":now,"updated_at":now,"version":1});
        let recurrence = json!({"task_id":task_id,"rules":[{"id":rule_id,"task_id":task_id,"frequency":"daily","weekdays_mask":null,"effective_start":"2026-10-06","effective_end":null,"created_at":now,"updated_at":now,"version":1}]});
        let occurrence = json!({"task_id":task_id,"occurrence_date":"2026-10-06","origin":"scheduled","status":"open","completed_at":null,"created_at":now,"updated_at":now,"version":1});
        apply_incremental_entities(
            &database,
            &cloud_workspace_id,
            &device_id,
            10,
            &[
                incremental_entity("tasks", "tasks", task_id.clone(), false, Some(task)),
                incremental_entity(
                    "task_daily_estimates",
                    "task_daily_estimates",
                    format!("{task_id}|2026-10-06"),
                    false,
                    Some(estimate),
                ),
                incremental_entity(
                    "task_recurrence_rules",
                    "task_recurrence_rules",
                    task_id.clone(),
                    false,
                    Some(recurrence),
                ),
                incremental_entity(
                    "task_occurrences",
                    "task_occurrences",
                    format!("{task_id}|2026-10-06"),
                    false,
                    Some(occurrence),
                ),
            ],
        )
        .unwrap();
        let domains = apply_incremental_entities(
            &database,
            &cloud_workspace_id,
            &device_id,
            13,
            &[
                incremental_entity(
                    "task_daily_estimates",
                    "task_daily_estimates",
                    format!("{task_id}|2026-10-06"),
                    true,
                    None,
                ),
                incremental_entity(
                    "task_recurrence_rules",
                    "task_recurrence_rules",
                    task_id.clone(),
                    false,
                    Some(json!({"task_id":task_id,"rules":[]})),
                ),
                incremental_entity(
                    "task_occurrences",
                    "task_occurrences",
                    format!("{task_id}|2026-10-06"),
                    true,
                    None,
                ),
            ],
        )
        .unwrap();
        assert_eq!(domains, vec!["tasks"]);
        let connection = database.open().unwrap();
        let counts: (i64, i64, i64) = connection
            .query_row(
                "SELECT
                   (SELECT COUNT(*) FROM task_daily_estimates WHERE task_id=?1),
                   (SELECT COUNT(*) FROM task_recurrence_rules WHERE task_id=?1),
                   (SELECT COUNT(*) FROM task_occurrences WHERE task_id=?1)",
                [&task_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(counts, (0, 0, 0));
    }

    #[test]
    fn unassigned_change_replaces_device_local_session_with_shared_session() {
        let (_directory, database, cloud_workspace_id, _local_workspace_id, _subject_id) =
            cloud_database_for_incremental_test("incremental-unassigned-hint.sqlite3");
        let device_id = storage_mode(&database).unwrap().device_id;
        let local_state = crate::unassigned::get_state(&database).unwrap().unwrap();
        let work_date = local_state.work_date.clone();
        let remote_id = Uuid::now_v7().to_string();
        let now = now_millis();
        let domains = apply_incremental_entities(
            &database,
            &cloud_workspace_id,
            &device_id,
            14,
            &[incremental_entity(
                "unassigned_sessions",
                "unassigned_sessions",
                remote_id.clone(),
                false,
                Some(json!({
                    "id":remote_id,"work_date":work_date,"state":"collecting",
                    "threshold_seconds":300,"duration_seconds":1,"first_started_at":now,
                    "last_ended_at":null,"prompted_at":null,"resolution_type":null,
                    "generated_entry_id":null,"resolved_at":null,"created_at":now,
                    "updated_at":now,"version":1,"segments":[]
                })),
            )],
        )
        .unwrap();
        assert_eq!(domains, vec!["unassigned"]);
        assert_eq!(
            crate::unassigned::get_state(&database)
                .unwrap()
                .unwrap()
                .session_id,
            remote_id
        );
        let old_state: String = database
            .open()
            .unwrap()
            .query_row(
                "SELECT state FROM unassigned_sessions WHERE id=?1",
                [&local_state.session_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(old_state, "discarded");
        assert_eq!(storage_mode(&database).unwrap().last_change_seq, 14);
    }

    #[test]
    fn authoritative_unassigned_incremental_acknowledges_same_candidate_create() {
        let (_directory, database, cloud_workspace_id, _local_workspace_id, _subject_id) =
            cloud_database_for_incremental_test("incremental-unassigned-candidate-ack.sqlite3");
        let storage = storage_mode(&database).unwrap();
        let local = crate::unassigned::get_state(&database).unwrap().unwrap();
        let resumed_at = local.first_started_at + 2_000;
        let operation_id = Uuid::now_v7().to_string();
        database
            .open()
            .unwrap()
            .execute(
                "INSERT INTO sync_outbox(
                   operation_id,workspace_id,device_id,operation_type,entity_type,entity_id,
                   payload_json,state,attempt_count,created_at
                 ) VALUES (?1,?2,?3,'unassigned_session_create','unassigned_session',?4,?5,'pending',0,?6)",
                params![
                    operation_id,
                    cloud_workspace_id,
                    storage.device_id,
                    local.session_id,
                    json!({"id":local.session_id}).to_string(),
                    now_millis()
                ],
            )
            .unwrap();

        apply_incremental_entities(
            &database,
            &cloud_workspace_id,
            &storage.device_id,
            15,
            &[incremental_entity(
                "unassigned_sessions",
                "unassigned_sessions",
                local.session_id.clone(),
                false,
                Some(json!({
                    "id":local.session_id,"work_date":local.work_date,"state":"collecting",
                    "threshold_seconds":300,"duration_seconds":0,
                    "first_started_at":resumed_at,"last_ended_at":null,
                    "prompted_at":null,"resolution_type":null,
                    "generated_entry_id":null,"resolved_at":null,
                    "created_at":local.first_started_at,"updated_at":resumed_at,
                    "version":local.version+1,"segments":[{
                        "id":Uuid::now_v7().to_string(),"session_id":local.session_id,
                        "sequence_no":1,"started_at":resumed_at,"ended_at":null,
                        "duration_seconds":0,"lease_token":null
                    }]
                })),
            )],
        )
        .unwrap();

        let adopted = crate::unassigned::get_state(&database).unwrap().unwrap();
        assert_eq!(adopted.first_started_at, resumed_at);
        assert_eq!(adopted.version, local.version + 1);
        assert_eq!(
            sync_status(&database).unwrap().pending_operations,
            0,
            "服务端已经确认同 ID 锚点后，不应继续保留候选创建操作"
        );
    }

    #[test]
    fn incoming_latest_anchor_discards_historical_pending() {
        let (_directory, database, cloud_workspace_id, local_workspace_id, _subject_id) =
            cloud_database_for_incremental_test("incremental-unassigned-anchor-normalize.sqlite3");
        let device_id = storage_mode(&database).unwrap().device_id;
        let current = crate::unassigned::get_state(&database).unwrap().unwrap();
        let now = now_millis();
        let historical_id = Uuid::now_v7().to_string();
        let historical_segment_id = Uuid::now_v7().to_string();
        let connection = database.open().unwrap();
        connection.execute(
            "INSERT INTO unassigned_sessions(
               id,workspace_id,work_date,state,threshold_seconds,duration_seconds,first_started_at,
               last_ended_at,created_at,updated_at,version,shared_source,migration_state,last_continuous_at
             ) VALUES (?1,?2,'2026-10-04','awaiting_resolution',300,600,?3,?4,?3,?4,2,'cloud','adopted',?4)",
            params![historical_id, local_workspace_id, now - 700_000, now - 100_000],
        ).unwrap();
        connection.execute(
            "INSERT INTO unassigned_segments(id,workspace_id,session_id,sequence_no,started_at,ended_at,duration_seconds)
             VALUES (?1,?2,?3,1,?4,?5,600)",
            params![historical_segment_id, local_workspace_id, historical_id, now - 700_000, now - 100_000],
        ).unwrap();
        drop(connection);

        let remote_id = Uuid::now_v7().to_string();
        apply_incremental_entities(
            &database,
            &cloud_workspace_id,
            &device_id,
            15,
            &[incremental_entity(
                "unassigned_sessions",
                "unassigned_sessions",
                remote_id.clone(),
                false,
                Some(json!({
                    "id":remote_id,"work_date":current.work_date,"state":"awaiting_resolution",
                    "threshold_seconds":300,"duration_seconds":999,"first_started_at":now-60_000,
                    "last_ended_at":null,"prompted_at":now,"resolution_type":null,
                    "generated_entry_id":null,"resolved_at":null,"created_at":now-60_000,
                    "updated_at":now,"version":8,"segments":[
                        {"id":Uuid::now_v7().to_string(),"session_id":remote_id,"sequence_no":1,"started_at":now-120_000,"ended_at":now-60_000,"duration_seconds":60},
                        {"id":Uuid::now_v7().to_string(),"session_id":remote_id,"sequence_no":2,"started_at":now-60_000,"ended_at":null,"duration_seconds":999}
                    ]
                })),
            )],
        ).unwrap();

        let connection = database.open().unwrap();
        let historical: (String, i64, i64) = connection.query_row(
            "SELECT state,duration_seconds,(SELECT COUNT(*) FROM unassigned_segments WHERE session_id=?1)
             FROM unassigned_sessions WHERE id=?1",
            [&historical_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).unwrap();
        assert_eq!(historical, ("discarded".to_string(), 0, 1));
        let active: (String, i64, Option<i64>, i64) = connection.query_row(
            "SELECT state,duration_seconds,prompted_at,
                    (SELECT COUNT(*) FROM unassigned_segments WHERE session_id=?1 AND ended_at IS NULL)
             FROM unassigned_sessions WHERE id=?1",
            [&remote_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).unwrap();
        assert_eq!(active, ("collecting".to_string(), 0, None, 1));
        let segment: (i64, i64, Option<i64>, i64) = connection.query_row(
            "SELECT sequence_no,started_at,ended_at,duration_seconds FROM unassigned_segments WHERE session_id=?1",
            [&remote_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).unwrap();
        assert_eq!(segment, (1, now - 60_000, None, 0));
    }

    #[test]
    fn incoming_historical_pending_is_superseded_when_latest_anchor_exists() {
        let (_directory, database, cloud_workspace_id, _local_workspace_id, _subject_id) =
            cloud_database_for_incremental_test("incremental-unassigned-closed-history.sqlite3");
        let device_id = storage_mode(&database).unwrap().device_id;
        let today = crate::unassigned::get_state(&database).unwrap().unwrap();
        let historical_id = Uuid::now_v7().to_string();
        let started_at = today.first_started_at - 120_000;
        let ended_at = today.first_started_at - 60_000;

        apply_incremental_entities(
            &database,
            &cloud_workspace_id,
            &device_id,
            16,
            &[incremental_entity(
                "unassigned_sessions",
                "unassigned_sessions",
                historical_id.clone(),
                false,
                Some(json!({
                    "id":historical_id,"work_date":"2026-10-07",
                    "state":"awaiting_resolution","threshold_seconds":300,
                    "duration_seconds":60,"first_started_at":started_at,
                    "last_ended_at":ended_at,"prompted_at":null,
                    "resolution_type":null,"generated_entry_id":null,
                    "resolved_at":null,"created_at":started_at,
                    "updated_at":ended_at,"version":2,"segments":[{
                        "id":Uuid::now_v7().to_string(),"session_id":historical_id,
                        "sequence_no":1,"started_at":started_at,
                        "ended_at":ended_at,"duration_seconds":60,
                        "lease_token":null
                    }]
                })),
            )],
        )
        .unwrap();

        let connection = database.open().unwrap();
        let historical: (String, i64, i64) = connection
            .query_row(
                "SELECT state,duration_seconds,
                        (SELECT COUNT(*) FROM unassigned_segments
                         WHERE session_id=?1 AND ended_at IS NULL)
                 FROM unassigned_sessions WHERE id=?1",
                [&historical_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(historical, ("discarded".to_string(), 0, 0));
        assert_eq!(
            crate::unassigned::get_state(&database)
                .unwrap()
                .unwrap()
                .session_id,
            today.session_id
        );
    }

    #[test]
    fn shared_unassigned_payload_parses_complete_aggregate_and_rejects_invalid_identity() {
        let session_id = Uuid::now_v7().to_string();
        let segment_id = Uuid::now_v7().to_string();
        let payload = json!({
            "id": session_id,
            "state": "resolved",
            "version": 4,
            "first_started_at": 1_000,
            "resolved_at": 61_000,
            "resolution_type": "work",
            "predecessor_session_id": null,
            "segments": [{
                "id": segment_id,
                "session_id": session_id,
                "sequence_no": 1,
                "started_at": 1_000,
                "ended_at": 61_000,
                "duration_seconds": 60
            }]
        });
        let parsed = validate_shared_unassigned_payload(&session_id, &payload).unwrap();
        assert_eq!(parsed.state, "resolved");
        assert_eq!(parsed.segments.len(), 1);

        let invalid_id = Uuid::now_v7().to_string();
        assert!(validate_shared_unassigned_payload(&invalid_id, &payload)
            .unwrap_err()
            .contains("身份不匹配"));

        let mut invalid_terminal = payload.clone();
        invalid_terminal["resolved_at"] = Value::Null;
        assert!(
            validate_shared_unassigned_payload(&session_id, &invalid_terminal)
                .unwrap_err()
                .contains("缺少终态")
        );
    }

    #[test]
    fn shared_unassigned_payload_accepts_postgrest_timestamp_strings() {
        let session_id = Uuid::now_v7().to_string();
        let segment_id = Uuid::now_v7().to_string();
        let payload = json!({
            "id": session_id,
            "state": "resolved",
            "version": 2,
            "first_started_at": "2026-10-04T16:08:51.957933+00:00",
            "resolved_at": "2026-10-04T16:09:51.957933+00:00",
            "resolution_type": "work",
            "predecessor_session_id": null,
            "segments": [{
                "id": segment_id,
                "session_id": session_id,
                "sequence_no": 1,
                "started_at": "2026-10-04T16:08:51.957933+00:00",
                "ended_at": "2026-10-04T16:09:51.957933+00:00",
                "duration_seconds": 60
            }]
        });

        let parsed = validate_shared_unassigned_payload(&session_id, &payload).unwrap();
        assert_eq!(parsed.first_started_at, 1_791_130_131_957);
        assert_eq!(parsed.resolved_at, Some(1_791_130_191_957));
        assert_eq!(parsed.segments[0].started_at, parsed.first_started_at);
        assert_eq!(parsed.segments[0].ended_at, parsed.resolved_at);
    }

    #[test]
    fn shared_unassigned_payload_reports_reversed_segment_boundary() {
        let session_id = Uuid::now_v7().to_string();
        let payload = json!({
            "id": session_id,
            "state": "awaiting_resolution",
            "version": 2,
            "first_started_at": "2026-10-04T16:09:51.957933+00:00",
            "resolved_at": null,
            "resolution_type": null,
            "segments": [{
                "id": Uuid::now_v7().to_string(),
                "session_id": session_id,
                "sequence_no": 132,
                "started_at": "2026-10-06T14:25:42.110728+00:00",
                "ended_at": "2026-10-04T16:00:00+00:00",
                "duration_seconds": 0
            }]
        });

        let error = validate_shared_unassigned_payload(&session_id, &payload).unwrap_err();
        assert!(error.contains("结束时间早于开始时间"));
        assert!(error.contains("序号 132"));
    }

    #[test]
    fn remote_resolution_starts_next_shared_session_at_server_boundary_when_idle() {
        let _guard = CLOUD_SYNC_TEST_STATE_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        CLOUD_SYNC_SIGNALS.store(0, Ordering::Release);
        let (_directory, database, cloud_workspace_id, _local_workspace_id, _subject_id) =
            cloud_database_for_incremental_test("incremental-unassigned-resolved-idle.sqlite3");
        let device_id = storage_mode(&database).unwrap().device_id;
        let old = crate::unassigned::get_state(&database).unwrap().unwrap();
        let resolved_at = now_millis() - 5_000;
        let domains = apply_incremental_entities(
            &database,
            &cloud_workspace_id,
            &device_id,
            21,
            &[incremental_entity(
                "unassigned_sessions",
                "unassigned_sessions",
                old.session_id.clone(),
                false,
                Some(json!({
                    "id":old.session_id,"work_date":"2026-10-06","state":"resolved",
                    "threshold_seconds":300,"duration_seconds":60,"first_started_at":resolved_at-60_000,
                    "last_ended_at":resolved_at,"prompted_at":null,"resolution_type":"work",
                    "generated_entry_id":null,"resolved_at":resolved_at,"created_at":resolved_at-60_000,
                    "updated_at":resolved_at,"version":old.version+1,"segments":[]
                })),
            )],
        )
        .unwrap();
        assert_eq!(domains, vec!["unassigned"]);
        let next = crate::unassigned::get_state(&database).unwrap().unwrap();
        assert_ne!(next.session_id, old.session_id);
        assert_eq!(next.first_started_at, resolved_at);
        assert!(next.elapsed_seconds >= 4);
        let predecessor: String = database
            .open()
            .unwrap()
            .query_row(
                "SELECT predecessor_session_id FROM unassigned_sessions WHERE id=?1",
                [&next.session_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(predecessor, old.session_id);
        let queued: (String, i64) = database
            .open()
            .unwrap()
            .query_row(
                "SELECT state,attempt_count FROM sync_outbox
                 WHERE operation_type='unassigned_session_create' AND entity_id=?1",
                [&next.session_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(queued, ("pending".to_string(), 0));
        assert!(next_outbox_retry_delay(&database).unwrap().is_some());
        assert_ne!(
            CLOUD_SYNC_SIGNALS.load(Ordering::Acquire) & SYNC_SIGNAL_LOCAL_DIRTY,
            0
        );
        CLOUD_SYNC_SIGNALS.store(0, Ordering::Release);
    }

    #[test]
    fn remote_resolution_can_arrive_before_its_generated_time_entry() {
        let (_directory, database, cloud_workspace_id, _local_workspace_id, _subject_id) =
            cloud_database_for_incremental_test(
                "incremental-unassigned-resolution-before-entry.sqlite3",
            );
        let device_id = storage_mode(&database).unwrap().device_id;
        let old = crate::unassigned::get_state(&database).unwrap().unwrap();
        let entry_id = Uuid::now_v7().to_string();
        let resolved_at = now_millis() - 1_000;

        apply_incremental_entities(
            &database,
            &cloud_workspace_id,
            &device_id,
            31,
            &[incremental_entity(
                "unassigned_sessions",
                "unassigned_sessions",
                old.session_id.clone(),
                false,
                Some(json!({
                    "id":old.session_id,"work_date":"2026-10-06","state":"resolved",
                    "threshold_seconds":300,"duration_seconds":60,
                    "first_started_at":resolved_at-60_000,"last_ended_at":resolved_at,
                    "prompted_at":null,"resolution_type":"break",
                    "generated_entry_id":entry_id,"resolved_at":resolved_at,
                    "created_at":resolved_at-60_000,"updated_at":resolved_at,
                    "version":old.version+1,"segments":[]
                })),
            )],
        )
        .unwrap();

        let connection = database.open().unwrap();
        let terminal: (String, Option<String>) = connection
            .query_row(
                "SELECT state,generated_entry_id FROM unassigned_sessions WHERE id=?1",
                [&old.session_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(terminal.0, "resolved");
        assert_eq!(terminal.1, None);
        drop(connection);

        apply_incremental_entities(
            &database,
            &cloud_workspace_id,
            &device_id,
            32,
            &[incremental_entity(
                "time_entries",
                "time_entries",
                entry_id.clone(),
                false,
                Some(json!({
                    "id":entry_id,"work_date":"2026-10-06","kind":"break",
                    "source_type":"unassigned","state":"ended","default_task_id":null,
                    "label_snapshot":"未归属时间（休息）","started_at":resolved_at-60_000,
                    "ended_at":resolved_at,"duration_seconds":60,"note":null,
                    "origin_unassigned_session_id":old.session_id,"created_at":resolved_at,
                    "updated_at":resolved_at,"version":1,"deleted_at":null,
                    "segments":[],"allocations":[]
                })),
            )],
        )
        .unwrap();

        let linked_entry: String = database
            .open()
            .unwrap()
            .query_row(
                "SELECT generated_entry_id FROM unassigned_sessions WHERE id=?1",
                [&old.session_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(linked_entry, entry_id);
    }

    #[test]
    fn authoritative_unassigned_entry_replaces_optimistic_loser_before_session_incremental() {
        let (_directory, database, cloud_workspace_id, local_workspace_id, _subject_id) =
            cloud_database_for_incremental_test(
                "incremental-authoritative-unassigned-entry-first.sqlite3",
            );
        let device_id = storage_mode(&database).unwrap().device_id;
        let session = crate::unassigned::get_state(&database).unwrap().unwrap();
        let losing_entry_id = Uuid::now_v7().to_string();
        let winner_entry_id = Uuid::now_v7().to_string();
        let ended_at = now_millis();
        let connection = database.open().unwrap();
        connection.execute(
            "INSERT INTO time_entries(id,workspace_id,work_date,kind,source_type,state,label_snapshot,started_at,ended_at,duration_seconds,origin_unassigned_session_id,created_at,updated_at,version)
             VALUES (?1,?2,?3,'break','unassigned','ended','本机乐观输家',?4,?5,60,?6,?4,?5,1)",
            params![losing_entry_id,local_workspace_id,session.work_date,ended_at-60_000,ended_at,session.session_id],
        ).unwrap();
        connection
            .execute(
                "UPDATE unassigned_sessions SET state='discarded',resolution_type='discard',
             generated_entry_id=?1,resolved_at=?2,last_ended_at=?2,version=version+1 WHERE id=?3",
                params![losing_entry_id, ended_at, session.session_id],
            )
            .unwrap();
        drop(connection);

        apply_incremental_entities(
            &database,
            &cloud_workspace_id,
            &device_id,
            33,
            &[incremental_entity(
                "time_entries",
                "time_entries",
                winner_entry_id.clone(),
                false,
                Some(json!({
                    "id":winner_entry_id,"work_date":session.work_date,"kind":"work",
                    "source_type":"unassigned","state":"ended","default_task_id":null,
                    "label_snapshot":"云端权威胜者","started_at":ended_at-60_000,
                    "ended_at":ended_at,"duration_seconds":60,"note":null,
                    "origin_unassigned_session_id":session.session_id,"created_at":ended_at,
                    "updated_at":ended_at,"version":1,"deleted_at":null,
                    "segments":[],"allocations":[]
                })),
            )],
        )
        .unwrap();

        let connection = database.open().unwrap();
        let entries = connection
            .prepare(
                "SELECT id FROM time_entries WHERE origin_unassigned_session_id=?1 ORDER BY id",
            )
            .unwrap()
            .query_map([&session.session_id], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(entries, vec![winner_entry_id.clone()]);
        let linked: Option<String> = connection
            .query_row(
                "SELECT generated_entry_id FROM unassigned_sessions WHERE id=?1",
                [&session.session_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(linked.as_deref(), Some(winner_entry_id.as_str()));
    }

    #[test]
    fn superseded_lifecycle_operation_adopts_remote_terminal_session() {
        let (_directory, database, _cloud_workspace_id, _local_workspace_id, _subject_id) =
            cloud_database_for_incremental_test(
                "superseded-unassigned-lifecycle-adopts-terminal.sqlite3",
            );
        let old = crate::unassigned::get_state(&database).unwrap().unwrap();
        let resolved_at = now_millis() - 1_000;
        let operation = OutboxOperation {
            operation_id: Uuid::now_v7().to_string(),
            workspace_epoch: 1,
            operation_type: "unassigned_session_awaiting_resolution".to_string(),
            entity_type: "unassigned_session".to_string(),
            entity_id: Some(old.session_id.clone()),
            base_version: Some(old.version),
            payload_version: Some(old.version + 1),
            coalesced_count: 1,
            depends_on_operation_id: None,
            payload: json!({"id":old.session_id,"version":old.version+1}),
        };
        let adopted = adopt_superseded_shared_unassigned_lifecycle_result(
            &database,
            &operation,
            &json!({
                "superseded":true,
                "session":{
                    "id":old.session_id,"work_date":"2026-10-06",
                    "state":"resolved","threshold_seconds":300,
                    "duration_seconds":60,"first_started_at":resolved_at-60_000,
                    "last_ended_at":resolved_at,"prompted_at":null,
                    "resolution_type":"break","generated_entry_id":null,
                    "resolved_at":resolved_at,"created_at":resolved_at-60_000,
                    "updated_at":resolved_at,"version":old.version+2,
                    "segments":[]
                }
            }),
        )
        .unwrap();
        assert!(adopted);
        let terminal: (String, String, i64) = database
            .open()
            .unwrap()
            .query_row(
                "SELECT state,resolution_type,version FROM unassigned_sessions WHERE id=?1",
                [&old.session_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            terminal,
            ("resolved".to_string(), "break".to_string(), old.version + 2)
        );
    }

    #[test]
    fn remote_resolution_does_not_restart_unassigned_while_task_timer_is_active() {
        let (_directory, database, cloud_workspace_id, local_workspace_id, subject_id) =
            cloud_database_for_incremental_test("incremental-unassigned-resolved-busy.sqlite3");
        let device_id = storage_mode(&database).unwrap().device_id;
        let old = crate::unassigned::get_state(&database).unwrap().unwrap();
        let now = now_millis();
        let task_id = Uuid::now_v7().to_string();
        let entry_id = Uuid::now_v7().to_string();
        let connection = database.open().unwrap();
        connection.execute("INSERT INTO tasks(id,workspace_id,subject_id,title,status,source_type,sort_order,created_at,updated_at,version) VALUES (?1,?2,?3,'计时中','open','manual',1,?4,?4,1)",params![task_id,local_workspace_id,subject_id,now]).unwrap();
        connection.execute("INSERT INTO time_entries(id,workspace_id,work_date,kind,source_type,state,default_task_id,label_snapshot,started_at,duration_seconds,created_at,updated_at,version) VALUES (?1,?2,'2026-10-06','work','timer','running',?3,'计时中',?4,0,?4,?4,1)",params![entry_id,local_workspace_id,task_id,now]).unwrap();
        drop(connection);
        apply_incremental_entities(
            &database,
            &cloud_workspace_id,
            &device_id,
            22,
            &[incremental_entity(
                "unassigned_sessions",
                "unassigned_sessions",
                old.session_id.clone(),
                false,
                Some(json!({
                    "id":old.session_id,"work_date":"2026-10-06","state":"discarded",
                    "threshold_seconds":300,"duration_seconds":10,"first_started_at":now-10_000,
                    "last_ended_at":now,"prompted_at":null,"resolution_type":"discard",
                    "generated_entry_id":null,"resolved_at":now,"created_at":now-10_000,
                    "updated_at":now,"version":old.version+1,"segments":[]
                })),
            )],
        )
        .unwrap();
        let active_unassigned: i64 = database.open().unwrap().query_row(
            "SELECT COUNT(*) FROM unassigned_sessions WHERE state IN ('collecting','awaiting_resolution')",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(active_unassigned, 0);
    }

    #[test]
    fn remote_resolution_supersedes_a_different_local_session_without_restarting_it() {
        let (_directory, database, cloud_workspace_id, _local_workspace_id, _subject_id) =
            cloud_database_for_incremental_test(
                "incremental-unassigned-resolved-other-local.sqlite3",
            );
        let device_id = storage_mode(&database).unwrap().device_id;
        let local = crate::unassigned::get_state(&database).unwrap().unwrap();
        let remote_id = Uuid::now_v7().to_string();
        let resolved_at = now_millis() - 3_000;
        apply_incremental_entities(
            &database,
            &cloud_workspace_id,
            &device_id,
            23,
            &[incremental_entity(
                "unassigned_sessions",
                "unassigned_sessions",
                remote_id.clone(),
                false,
                Some(json!({
                    "id":remote_id,"work_date":"2026-10-06","state":"resolved",
                    "threshold_seconds":300,"duration_seconds":60,"first_started_at":resolved_at-60_000,
                    "last_ended_at":resolved_at,"prompted_at":null,"resolution_type":"break",
                    "generated_entry_id":null,"resolved_at":resolved_at,"created_at":resolved_at-60_000,
                    "updated_at":resolved_at,"version":2,"segments":[]
                })),
            )],
        )
        .unwrap();

        let connection = database.open().unwrap();
        let local_state: (String, Option<i64>) = connection
            .query_row(
                "SELECT state,resolved_at FROM unassigned_sessions WHERE id=?1",
                [&local.session_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(local_state, ("discarded".to_string(), Some(resolved_at)));
        let local_open_segments: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM unassigned_segments WHERE session_id=?1 AND ended_at IS NULL",
                [&local.session_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(local_open_segments, 0);
        let next: (String, i64, String) = connection
            .query_row(
                "SELECT id,first_started_at,predecessor_session_id FROM unassigned_sessions
                 WHERE state IN ('collecting','awaiting_resolution')",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_ne!(next.0, local.session_id);
        assert_eq!(next.1, resolved_at);
        assert_eq!(next.2, remote_id);
    }

    #[test]
    fn repeated_remote_resolution_keeps_one_next_session_and_original_boundary() {
        let (_directory, database, cloud_workspace_id, _local_workspace_id, _subject_id) =
            cloud_database_for_incremental_test("incremental-unassigned-resolved-repeat.sqlite3");
        let device_id = storage_mode(&database).unwrap().device_id;
        let old = crate::unassigned::get_state(&database).unwrap().unwrap();
        let resolved_at = now_millis() - 4_000;
        let entity = incremental_entity(
            "unassigned_sessions",
            "unassigned_sessions",
            old.session_id.clone(),
            false,
            Some(json!({
                "id":old.session_id,"work_date":"2026-10-06","state":"discarded",
                "threshold_seconds":300,"duration_seconds":10,"first_started_at":resolved_at-10_000,
                "last_ended_at":resolved_at,"prompted_at":null,"resolution_type":"discard",
                "generated_entry_id":null,"resolved_at":resolved_at,"created_at":resolved_at-10_000,
                "updated_at":resolved_at,"version":old.version+1,"segments":[]
            })),
        );
        apply_incremental_entities(
            &database,
            &cloud_workspace_id,
            &device_id,
            24,
            &[entity.clone()],
        )
        .unwrap();
        apply_incremental_entities(
            &database,
            &cloud_workspace_id,
            &device_id,
            25,
            &[entity.clone()],
        )
        .unwrap();

        let connection = database.open().unwrap();
        let next: (i64, i64) = connection
            .query_row(
                "SELECT COUNT(*),MIN(first_started_at) FROM unassigned_sessions
                 WHERE predecessor_session_id=?1 AND state IN ('collecting','awaiting_resolution')",
                [&old.session_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(next, (1, resolved_at));

        let candidate_id: String = connection
            .query_row(
                "SELECT id FROM unassigned_sessions
                 WHERE predecessor_session_id=?1 AND state IN ('collecting','awaiting_resolution')",
                [&old.session_id],
                |row| row.get(0),
            )
            .unwrap();
        connection
            .execute(
                "UPDATE unassigned_sessions SET migration_state='adopted' WHERE id=?1",
                [&candidate_id],
            )
            .unwrap();
        connection
            .execute(
                "DELETE FROM sync_outbox WHERE entity_type='unassigned_session' AND entity_id=?1",
                [&candidate_id],
            )
            .unwrap();
        drop(connection);

        apply_incremental_entities(&database, &cloud_workspace_id, &device_id, 26, &[entity])
            .unwrap();
        let repeated_outbox: i64 = database
            .open()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM sync_outbox
                 WHERE entity_type='unassigned_session' AND entity_id=?1",
                [&candidate_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            repeated_outbox, 0,
            "已被云端采用的下一段未归属会话不能因重复终态通知再次入队"
        );
    }

    #[test]
    fn remote_next_session_supersedes_local_candidate_with_same_predecessor() {
        let (_directory, database, cloud_workspace_id, _local_workspace_id, _subject_id) =
            cloud_database_for_incremental_test(
                "incremental-unassigned-authoritative-next-session.sqlite3",
            );
        let device_id = storage_mode(&database).unwrap().device_id;
        let old = crate::unassigned::get_state(&database).unwrap().unwrap();
        let resolved_at = now_millis() - 4_000;
        apply_incremental_entities(
            &database,
            &cloud_workspace_id,
            &device_id,
            24,
            &[incremental_entity(
                "unassigned_sessions",
                "unassigned_sessions",
                old.session_id.clone(),
                false,
                Some(json!({
                    "id":old.session_id,"work_date":"2026-10-06","state":"resolved",
                    "threshold_seconds":300,"duration_seconds":10,
                    "first_started_at":resolved_at-10_000,"last_ended_at":resolved_at,
                    "prompted_at":null,"resolution_type":"break","generated_entry_id":null,
                    "resolved_at":resolved_at,"created_at":resolved_at-10_000,
                    "updated_at":resolved_at,"version":old.version+1,"segments":[]
                })),
            )],
        )
        .unwrap();
        let connection = database.open().unwrap();
        let local_candidate: String = connection
            .query_row(
                "SELECT id FROM unassigned_sessions WHERE predecessor_session_id=?1",
                [&old.session_id],
                |row| row.get(0),
            )
            .unwrap();
        drop(connection);
        enqueue_entity_deferred(
            &database,
            "unassigned_session_create",
            "unassigned_session",
            Some(&local_candidate),
            None,
            None,
        )
        .unwrap();
        let remote_id = Uuid::now_v7().to_string();
        apply_incremental_entities(
            &database,
            &cloud_workspace_id,
            &device_id,
            25,
            &[incremental_entity(
                "unassigned_sessions",
                "unassigned_sessions",
                remote_id.clone(),
                false,
                Some(json!({
                    "id":remote_id,"work_date":"2026-10-06","state":"collecting",
                    "threshold_seconds":300,"duration_seconds":0,
                    "first_started_at":resolved_at,"last_ended_at":null,"prompted_at":null,
                    "resolution_type":null,"generated_entry_id":null,"resolved_at":null,
                    "predecessor_session_id":old.session_id,"created_at":resolved_at,
                    "updated_at":resolved_at,"version":1,"segments":[]
                })),
            )],
        )
        .unwrap();

        let connection = database.open().unwrap();
        let active: Vec<String> = connection
            .prepare(
                "SELECT id FROM unassigned_sessions
                 WHERE state IN ('collecting','awaiting_resolution') ORDER BY id",
            )
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(active, vec![remote_id]);
        let local_state: (String, Option<String>) = connection
            .query_row(
                "SELECT migration_state,predecessor_session_id FROM unassigned_sessions WHERE id=?1",
                [&local_candidate],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(local_state, ("superseded".to_string(), None));
        let stale_outbox: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sync_outbox WHERE entity_id=?1
                   AND state IN ('pending','sending','failed','conflict')",
                [&local_candidate],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stale_outbox, 0);
    }

    #[test]
    fn losing_shared_resolution_removes_all_dependent_outbox_and_optimistic_entry() {
        let (_directory, database, _cloud_workspace_id, local_workspace_id, _subject_id) =
            cloud_database_for_incremental_test("unassigned-resolution-loser.sqlite3");
        let now = now_millis();
        let session_id = Uuid::now_v7().to_string();
        let entry_id = Uuid::now_v7().to_string();
        let resolution_operation_id = Uuid::now_v7().to_string();
        let entry_operation_id = Uuid::now_v7().to_string();
        let task_operation_id = Uuid::now_v7().to_string();
        let connection = database.open().unwrap();
        let task_id: String = connection
            .query_row("SELECT id FROM tasks LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let cloud_workspace_id = storage_mode(&database).unwrap().workspace_id;
        connection
            .execute(
                "UPDATE tasks SET status='done',completed_at=?1,updated_at=?1,version=2 WHERE id=?2",
                params![now, task_id],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO task_status_events(id,workspace_id,task_id,status,occurred_at,source_type,created_at)
                 VALUES (?1,?2,?3,'done',?4,'user',?4)",
                params![Uuid::now_v7().to_string(), local_workspace_id, task_id, now],
            )
            .unwrap();
        connection.execute(
            "INSERT INTO unassigned_sessions(id,workspace_id,work_date,state,threshold_seconds,duration_seconds,first_started_at,last_ended_at,resolution_type,resolved_at,created_at,updated_at,version,shared_source,migration_state,resolution_operation_id)
             VALUES (?1,?2,'2026-10-06','resolved',300,60,?3,?4,'work',?4,?3,?4,2,'cloud','candidate',?5)",
            params![session_id,local_workspace_id,now-60_000,now,resolution_operation_id],
        ).unwrap();
        connection.execute(
            "INSERT INTO time_entries(id,workspace_id,work_date,kind,source_type,state,label_snapshot,started_at,ended_at,duration_seconds,origin_unassigned_session_id,created_at,updated_at,version)
             VALUES (?1,?2,'2026-10-06','work','unassigned','ended','临时结果',?3,?4,60,?5,?3,?4,1)",
            params![entry_id,local_workspace_id,now-60_000,now,session_id],
        ).unwrap();
        connection
            .execute(
                "UPDATE unassigned_sessions SET generated_entry_id=?1 WHERE id=?2",
                params![entry_id, session_id],
            )
            .unwrap();
        connection.execute(
            "INSERT INTO sync_outbox(operation_id,workspace_id,device_id,operation_type,entity_type,entity_id,base_version,payload_json,state,attempt_count,created_at)
             VALUES (?1,?2,?3,'unassigned_resolve_work','unassigned_session',?4,1,?5,'sending',1,?6)",
            params![resolution_operation_id,cloud_workspace_id,device_id,session_id,json!({"generated_entry_id":entry_id}).to_string(),now],
        ).unwrap();
        connection.execute(
            "INSERT INTO sync_outbox(operation_id,workspace_id,device_id,operation_type,entity_type,entity_id,payload_json,depends_on_operation_id,state,attempt_count,created_at)
             VALUES (?1,?2,?3,'unassigned_time_entry_create','time_entry',?4,'{}',?5,'pending',0,?6)",
            params![entry_operation_id,cloud_workspace_id,device_id,entry_id,resolution_operation_id,now+1],
        ).unwrap();
        connection.execute(
            "INSERT INTO sync_outbox(operation_id,workspace_id,device_id,operation_type,entity_type,entity_id,payload_json,depends_on_operation_id,state,attempt_count,created_at)
             VALUES (?1,?2,?3,'task_set_completed_from_unassigned','task',?4,'{}',?5,'pending',0,?6)",
            params![task_operation_id,cloud_workspace_id,device_id,task_id,entry_operation_id,now+2],
        ).unwrap();
        connection.execute(
            "UPDATE sync_outbox SET base_version=1,payload_json=?1,payload_version=2 WHERE operation_id=?2",
            params![json!({"id":task_id,"status":"done","completed_at":now,"version":2}).to_string(),task_operation_id],
        ).unwrap();
        let queued_task: (String, Option<i64>, Option<i64>, Option<String>) = connection
            .query_row(
                "SELECT operation_type,base_version,payload_version,depends_on_operation_id FROM sync_outbox WHERE operation_id=?1",
                [&task_operation_id],
                |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            queued_task,
            (
                "task_set_completed_from_unassigned".to_string(),
                Some(1),
                Some(2),
                Some(entry_operation_id.clone())
            )
        );
        drop(connection);
        let operation = OutboxOperation {
            operation_id: resolution_operation_id.clone(),
            workspace_epoch: 1,
            operation_type: "unassigned_resolve_work".to_string(),
            entity_type: "unassigned_session".to_string(),
            entity_id: Some(session_id.clone()),
            base_version: Some(1),
            payload_version: Some(2),
            coalesced_count: 1,
            depends_on_operation_id: None,
            payload: json!({"generated_entry_id":entry_id}),
        };
        let winner_resolved_at = now + 500;
        adopt_shared_unassigned_resolution_result(&database, &operation, &json!({
            "accepted":false,
            "session":{
                "id":session_id,"work_date":"2026-10-06","state":"discarded",
                "threshold_seconds":300,"duration_seconds":60,"first_started_at":now-60_000,
                "last_ended_at":winner_resolved_at,"prompted_at":null,"resolution_type":"discard",
                "generated_entry_id":null,"resolved_at":winner_resolved_at,"created_at":now-60_000,
                "updated_at":winner_resolved_at,"version":2,"segments":[]
            }
        })).unwrap();

        let connection = database.open().unwrap();
        let dependents: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sync_outbox WHERE operation_id IN (?1,?2)",
                params![entry_operation_id, task_operation_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(dependents, 0);
        let optimistic_entry: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM time_entries WHERE id=?1",
                [&entry_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(optimistic_entry, 0);
        let task_state: (String, Option<i64>, i64) = connection
            .query_row(
                "SELECT status,completed_at,version FROM tasks WHERE id=?1",
                [&task_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(task_state, ("open".to_string(), None, 1));
        let task_events: i64 = connection.query_row(
            "SELECT COUNT(*) FROM task_status_events WHERE task_id=?1 AND status='done' AND occurred_at=?2",
            params![task_id,now],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(task_events, 0);
        let terminal: (String, String, i64) = connection
            .query_row(
                "SELECT state,resolution_type,resolved_at FROM unassigned_sessions WHERE id=?1",
                [&session_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            terminal,
            (
                "discarded".to_string(),
                "discard".to_string(),
                winner_resolved_at
            )
        );
    }

    #[test]
    fn missing_shared_discard_adopts_authoritative_anchor_and_removes_orphaned_entry() {
        let (_directory, database, cloud_workspace_id, local_workspace_id, _subject_id) =
            cloud_database_for_incremental_test("missing-shared-unassigned-discard.sqlite3");
        let now = now_millis();
        let orphan_session_id = Uuid::now_v7().to_string();
        let orphan_entry_id = Uuid::now_v7().to_string();
        let operation_id = Uuid::now_v7().to_string();
        let connection = database.open().unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        connection.execute(
            "INSERT INTO unassigned_sessions(id,workspace_id,work_date,state,threshold_seconds,duration_seconds,first_started_at,last_ended_at,resolution_type,resolved_at,created_at,updated_at,version,shared_source,migration_state,resolution_operation_id)
             VALUES (?1,?2,'2026-10-14','discarded',300,60,?3,?4,'discard',?4,?3,?4,3,'cloud','candidate',?5)",
            params![orphan_session_id,local_workspace_id,now-60_000,now,operation_id],
        ).unwrap();
        connection.execute(
            "INSERT INTO time_entries(id,workspace_id,work_date,kind,source_type,state,label_snapshot,started_at,ended_at,duration_seconds,origin_unassigned_session_id,created_at,updated_at,version)
             VALUES (?1,?2,'2026-10-14','break','unassigned','ended','无效时间',?3,?4,60,?5,?3,?4,1)",
            params![orphan_entry_id,local_workspace_id,now-60_000,now,orphan_session_id],
        ).unwrap();
        connection
            .execute(
                "UPDATE unassigned_sessions SET generated_entry_id=?1 WHERE id=?2",
                params![orphan_entry_id, orphan_session_id],
            )
            .unwrap();
        connection.execute(
            "INSERT INTO sync_outbox(operation_id,workspace_id,device_id,operation_type,entity_type,entity_id,base_version,payload_json,payload_version,state,attempt_count,created_at)
             VALUES (?1,?2,?3,'unassigned_discard','unassigned_session',?4,1,?5,3,'sending',135,?6)",
            params![operation_id,cloud_workspace_id,device_id,orphan_session_id,json!({"generated_entry_id":orphan_entry_id}).to_string(),now],
        ).unwrap();
        drop(connection);

        let authoritative_id = crate::unassigned::get_state(&database)
            .unwrap()
            .unwrap()
            .session_id;
        let authoritative =
            entity_payload(&database, "unassigned_session", Some(&authoritative_id)).unwrap();
        let operation = OutboxOperation {
            operation_id: operation_id.clone(),
            workspace_epoch: 1,
            operation_type: "unassigned_discard".to_string(),
            entity_type: "unassigned_session".to_string(),
            entity_id: Some(orphan_session_id.clone()),
            base_version: Some(1),
            payload_version: Some(3),
            coalesced_count: 1,
            depends_on_operation_id: None,
            payload: json!({"generated_entry_id":orphan_entry_id}),
        };

        recover_orphaned_shared_unassigned_discard(&database, &operation, &authoritative).unwrap();

        let connection = database.open().unwrap();
        let orphan: (Option<String>, String) = connection
            .query_row(
                "SELECT generated_entry_id,migration_state FROM unassigned_sessions WHERE id=?1",
                [&orphan_session_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(orphan, (None, "superseded".to_string()));
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM time_entries WHERE id=?1",
                    [&orphan_entry_id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
        let authoritative_source: (String, String) = connection
            .query_row(
                "SELECT shared_source,migration_state FROM unassigned_sessions WHERE id=?1",
                [&authoritative_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            authoritative_source,
            ("cloud".to_string(), "adopted".to_string())
        );
    }

    #[test]
    fn missing_shared_unassigned_error_match_is_narrow() {
        assert!(is_missing_shared_unassigned_session_error(
            "CLOUD_REQUEST_FAILED: NOT_FOUND: shared unassigned session"
        ));
        assert!(!is_missing_shared_unassigned_session_error(
            "NETWORK_ERROR: request timed out"
        ));
        assert!(!is_missing_shared_unassigned_session_error(
            "CLOUD_REQUEST_FAILED: NOT_FOUND: task does not exist"
        ));
    }

    #[test]
    fn incremental_time_and_report_aggregates_replace_children_atomically() {
        let (_directory, database, cloud_workspace_id, _local_workspace_id, subject_id) =
            cloud_database_for_incremental_test("incremental-aggregates.sqlite3");
        let device_id = storage_mode(&database).unwrap().device_id;
        let task_id = Uuid::now_v7().to_string();
        let entry_id = Uuid::now_v7().to_string();
        let segment_id = Uuid::now_v7().to_string();
        let allocation_id = Uuid::now_v7().to_string();
        let report_id = Uuid::now_v7().to_string();
        let now = now_millis();
        database.open().unwrap().execute("INSERT INTO tasks(id,workspace_id,subject_id,title,status,source_type,sort_order,created_at,updated_at,version) SELECT ?1,id,?2,'关联事项','open','manual',1,?3,?3,1 FROM workspaces LIMIT 1",params![task_id,subject_id,now]).unwrap();
        let entry = json!({"id":entry_id,"work_date":"2026-10-06","kind":"work","source_type":"manual","state":"ended","default_task_id":task_id,"label_snapshot":"聚合工时","started_at":now,"ended_at":now+3600000,"duration_seconds":3600,"note":null,"origin_unassigned_session_id":null,"created_at":now,"updated_at":now,"version":1,"deleted_at":null,"segments":[{"id":segment_id,"sequence_no":1,"started_at":now,"ended_at":now+3600000,"duration_seconds":3600}],"allocations":[{"id":allocation_id,"task_id":task_id,"minutes":60,"note":null,"created_at":now,"updated_at":now,"version":1}]});
        let report = json!({"id":report_id,"report_type":"daily","subject_id":subject_id,"period_start":"2026-10-06","period_end":"2026-10-06","reference_date":"2026-10-06","template_id":null,"markdown_content":"日报内容","content_source":"edited","generation_count":1,"input_revision_hash":null,"generated_at":now,"created_at":now,"updated_at":now,"version":1,"deleted_at":null,"report_tasks":[{"task_id":task_id,"sort_order":1,"title_snapshot":"关联事项","path_snapshot":"默认 / 关联事项"}]});
        let domains = apply_incremental_entities(
            &database,
            &cloud_workspace_id,
            &device_id,
            11,
            &[
                incremental_entity(
                    "time_entries",
                    "time_segments",
                    entry_id.clone(),
                    false,
                    Some(entry),
                ),
                incremental_entity(
                    "reports",
                    "report_tasks",
                    report_id.clone(),
                    false,
                    Some(report),
                ),
            ],
        )
        .unwrap();
        assert_eq!(domains, vec!["reports", "time"]);
        let connection = database.open().unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM time_segments WHERE entry_id=?1",
                    [&entry_id],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT minutes FROM time_allocations WHERE entry_id=?1",
                    [&entry_id],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            60
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM report_tasks WHERE report_id=?1",
                    [&report_id],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
    }

    #[test]
    fn calendar_day_fields_are_compatible_across_incremental_snapshot_and_payloads() {
        let (_directory, database, cloud_workspace_id, local_workspace_id, _subject_id) =
            cloud_database_for_incremental_test("calendar-day-protocol.sqlite3");
        let device_id = storage_mode(&database).unwrap().device_id;
        let now = now_millis();
        let legacy_entry_id = Uuid::now_v7().to_string();
        let modern_entry_id = Uuid::now_v7().to_string();
        let legacy_session_id = Uuid::now_v7().to_string();
        let modern_chain_id = Uuid::now_v7().to_string();
        let modern_boundary = now - 30_000;

        let legacy_entry = json!({
            "id": legacy_entry_id, "work_date": "2026-10-05",
            "kind": "work", "source_type": "timer", "state": "ended",
            "default_task_id": null, "label_snapshot": "旧计时",
            "started_at": now - 60_000, "ended_at": now, "duration_seconds": 60,
            "note": null, "origin_unassigned_session_id": null,
            "created_at": now - 60_000, "updated_at": now, "version": 1,
            "deleted_at": null, "segments": [], "allocations": []
        });
        let modern_entry = json!({
            "id": modern_entry_id, "timer_chain_id": modern_chain_id,
            "previous_entry_id": legacy_entry_id, "split_boundary_at": modern_boundary,
            "last_continuous_at": now, "work_date": "2026-10-06",
            "kind": "work", "source_type": "timer", "state": "ended",
            "default_task_id": null, "label_snapshot": "新计时",
            "started_at": modern_boundary, "ended_at": now, "duration_seconds": 30,
            "note": null, "origin_unassigned_session_id": null,
            "created_at": modern_boundary, "updated_at": now, "version": 1,
            "deleted_at": null, "segments": [], "allocations": []
        });
        let legacy_session = json!({
            "id": legacy_session_id, "work_date": "2026-10-05",
            "state": "resolved", "threshold_seconds": 300, "duration_seconds": 60,
            "first_started_at": now - 60_000, "last_ended_at": now,
            "prompted_at": null, "resolution_type": "discard",
            "generated_entry_id": null, "resolved_at": now,
            "created_at": now - 60_000, "updated_at": now, "version": 1,
            "predecessor_session_id": null, "resolution_operation_id": null,
            "segments": []
        });
        apply_incremental_entities(
            &database,
            &cloud_workspace_id,
            &device_id,
            10,
            &[
                incremental_entity(
                    "time_entries",
                    "time_entries",
                    legacy_entry_id.clone(),
                    false,
                    Some(legacy_entry),
                ),
                incremental_entity(
                    "time_entries",
                    "time_entries",
                    modern_entry_id.clone(),
                    false,
                    Some(modern_entry),
                ),
                incremental_entity(
                    "unassigned_sessions",
                    "unassigned_sessions",
                    legacy_session_id.clone(),
                    false,
                    Some(legacy_session),
                ),
            ],
        )
        .unwrap();

        let connection = database.open().unwrap();
        let legacy_fields: (String, i64) = connection
            .query_row(
                "SELECT timer_chain_id,last_continuous_at FROM time_entries WHERE id=?1",
                [&legacy_entry_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(legacy_fields, (legacy_entry_id.clone(), now));
        let modern_fields: (String, String, i64, i64) = connection.query_row(
            "SELECT timer_chain_id,previous_entry_id,split_boundary_at,last_continuous_at FROM time_entries WHERE id=?1",
            [&modern_entry_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).unwrap();
        assert_eq!(
            modern_fields,
            (
                modern_chain_id.clone(),
                legacy_entry_id.clone(),
                modern_boundary,
                now
            )
        );
        let session_continuity: i64 = connection
            .query_row(
                "SELECT last_continuous_at FROM unassigned_sessions WHERE id=?1",
                [&legacy_session_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(session_continuity, now);
        drop(connection);

        let payload = entity_payload(&database, "time_entry", Some(&modern_entry_id)).unwrap();
        assert_eq!(payload["timer_chain_id"], json!(modern_chain_id));
        assert_eq!(payload["previous_entry_id"], json!(legacy_entry_id));
        assert_eq!(payload["split_boundary_at"], json!(modern_boundary));
        assert_eq!(payload["last_continuous_at"], json!(now));

        let snapshot_entry_id = Uuid::now_v7().to_string();
        let snapshot_session_id = Uuid::now_v7().to_string();
        let snapshot = json!({
            "time_entries": [{
                "id": snapshot_entry_id, "work_date": "2026-10-04",
                "kind": "work", "source_type": "timer", "state": "ended",
                "default_task_id": null, "label_snapshot": "旧快照",
                "started_at": now - 120_000, "ended_at": now - 60_000,
                "duration_seconds": 60, "note": null,
                "origin_unassigned_session_id": null, "created_at": now - 120_000,
                "updated_at": now - 60_000, "version": 1, "deleted_at": null
            }],
            "unassigned_sessions": [{
                "id": snapshot_session_id, "work_date": "2026-10-04",
                "state": "discarded", "threshold_seconds": 300,
                "duration_seconds": 0, "first_started_at": now - 120_000,
                "last_ended_at": now - 60_000, "prompted_at": null,
                "resolution_type": "discard", "generated_entry_id": null,
                "resolved_at": now - 60_000, "created_at": now - 120_000,
                "updated_at": now - 60_000, "version": 1
            }]
        });
        let mut connection = database.open().unwrap();
        let transaction = connection.transaction().unwrap();
        replace_snapshot_table(&transaction, &snapshot, "time_entries", &local_workspace_id)
            .unwrap();
        replace_snapshot_table(
            &transaction,
            &snapshot,
            "unassigned_sessions",
            &local_workspace_id,
        )
        .unwrap();
        transaction.commit().unwrap();
        let connection = database.open().unwrap();
        let snapshot_chain: String = connection
            .query_row(
                "SELECT timer_chain_id FROM time_entries WHERE id=?1",
                [&snapshot_entry_id],
                |row| row.get(0),
            )
            .unwrap();
        let snapshot_continuity: i64 = connection
            .query_row(
                "SELECT last_continuous_at FROM unassigned_sessions WHERE id=?1",
                [&snapshot_session_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(snapshot_chain, snapshot_entry_id);
        assert_eq!(snapshot_continuity, now - 60_000);
    }

    #[test]
    fn planning_payload_validation_covers_composite_ids_dates_and_actions() {
        let task_id = Uuid::now_v7().to_string();
        let estimate_id = format!("{task_id}|2026-10-05");
        assert!(validate_planning_entity_payload(
            "task_day_plan",
            &estimate_id,
            &json!({ "task_id": task_id, "work_date": "2026-10-05", "state": "included", "version": 1 })
        )
        .is_ok());
        assert!(validate_planning_entity_payload(
            "task_day_plan",
            &estimate_id,
            &json!({ "task_id": task_id, "work_date": "2026-10-05", "state": "unknown", "version": 1 })
        )
        .is_err());
        assert!(validate_planning_entity_payload(
            "task_daily_estimate",
            &estimate_id,
            &json!({
                "task_id": task_id,
                "work_date": "2026-10-05",
                "estimate_minutes": 45,
                "version": 1
            })
        )
        .is_ok());
        assert!(validate_planning_entity_payload(
            "task_daily_estimate",
            &estimate_id,
            &json!({ "task_id": task_id, "work_date": "2026-02-30", "deleted": true })
        )
        .unwrap_err()
        .starts_with("VALIDATION_ERROR:"));
        assert!(validate_planning_entity_payload(
            "task_daily_estimate",
            &format!("{task_id}|2026-10-06"),
            &json!({ "task_id": task_id, "work_date": "2026-10-05", "deleted": true })
        )
        .is_err());
        assert!(validate_planning_entity_payload(
            "task_daily_estimate",
            &estimate_id,
            &json!({ "task_id": task_id, "work_date": "2026-10-05" })
        )
        .is_err());

        assert!(validate_planning_entity_payload(
            "task_recurrence_rule",
            &task_id,
            &json!({ "action": "save", "task_id": task_id, "rules": [] })
        )
        .is_ok());
        assert!(validate_planning_entity_payload(
            "task_recurrence_rule",
            &task_id,
            &json!({ "action": "replace", "task_id": task_id, "rules": [] })
        )
        .is_err());
        assert!(validate_planning_entity_payload(
            "task_recurrence_rule",
            "other-task",
            &json!({ "action": "close", "task_id": task_id, "rules": [] })
        )
        .is_err());
    }

    #[test]
    fn planning_conflict_payload_reads_estimate_and_full_recurrence_group() {
        let task_id = Uuid::now_v7().to_string();
        let active_id = Uuid::now_v7().to_string();
        let history_id = Uuid::now_v7().to_string();
        let snapshot = json!({
            "tasks": [{ "id": task_id, "version": 7 }],
            "task_daily_estimates": [{
                "task_id": task_id,
                "work_date": "2026-10-05",
                "estimate_minutes": 45,
                "version": 2
            }],
            "task_day_plans": [{
                "task_id": task_id,
                "work_date": "2026-10-05",
                "state": "excluded",
                "version": 4
            }],
            "task_recurrence_rules": [
                { "id": history_id, "task_id": task_id, "frequency": "daily", "effective_start": "2026-10-01", "effective_end": "2026-10-04", "version": 2 },
                { "id": active_id, "task_id": task_id, "frequency": "weekdays", "effective_start": "2026-10-05", "effective_end": null, "version": 3 }
            ]
        });
        let estimate = cloud_conflict_payload(
            &snapshot,
            "task_daily_estimate",
            Some(&format!("{task_id}|2026-10-05")),
            &json!({ "task_id": task_id, "work_date": "2026-10-05" }),
        )
        .unwrap();
        assert_eq!(estimate["estimate_minutes"], 45);
        let day_plan = cloud_conflict_payload(
            &snapshot,
            "task_day_plan",
            Some(&format!("{task_id}|2026-10-05")),
            &json!({ "task_id": task_id, "work_date": "2026-10-05" }),
        )
        .unwrap();
        assert_eq!(day_plan["state"], "excluded");
        assert_eq!(day_plan["version"], 4);

        let recurrence = cloud_conflict_payload(
            &snapshot,
            "task_recurrence_rule",
            Some(&task_id),
            &json!({ "task_id": task_id }),
        )
        .unwrap();
        assert_eq!(recurrence["task_version"], 7);
        assert_eq!(recurrence["version"], 3);
        assert_eq!(recurrence["rules"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn keep_local_rebases_recurrence_versions_and_preserves_cloud_history() {
        let task_id = Uuid::now_v7().to_string();
        let active_id = Uuid::now_v7().to_string();
        let history_id = Uuid::now_v7().to_string();
        let local_id = Uuid::now_v7().to_string();
        let snapshot = json!({
            "tasks": [{ "id": task_id, "version": 9 }],
            "task_recurrence_rules": [
                { "id": history_id, "task_id": task_id, "frequency": "daily", "weekdays_mask": null, "effective_start": "2026-09-01", "effective_end": "2026-10-04", "created_at": 1, "updated_at": 2, "version": 2 },
                { "id": active_id, "task_id": task_id, "frequency": "daily", "weekdays_mask": null, "effective_start": "2026-10-05", "effective_end": null, "created_at": 3, "updated_at": 4, "version": 4 }
            ]
        });
        let local = json!({
            "action": "save",
            "task_id": task_id,
            "task_expected_version": 7,
            "task_version": 8,
            "rule_expected_version": 3,
            "rule_id": local_id,
            "frequency": "weekly",
            "weekdays_mask": 5,
            "effective_start": "2026-10-05",
            "version": 1,
            "rules": [{
                "id": local_id,
                "task_id": task_id,
                "frequency": "weekly",
                "weekdays_mask": 5,
                "effective_start": "2026-10-05",
                "effective_end": null,
                "created_at": 5,
                "updated_at": 6,
                "version": 1
            }]
        });
        let (base_version, rebased) = rebase_recurrence_payload(&snapshot, &local).unwrap();
        assert_eq!(base_version, Some(4));
        assert_eq!(rebased["task_expected_version"], 9);
        assert_eq!(rebased["task_version"], 10);
        assert_eq!(rebased["rule_expected_version"], 4);
        assert_eq!(rebased["rule_id"], active_id);
        assert_eq!(rebased["version"], 5);
        let rules = rebased["rules"].as_array().unwrap();
        assert_eq!(rules.len(), 2);
        assert!(rules.iter().any(|rule| rule["id"] == history_id));
        assert!(rules.iter().any(|rule| {
            rule["id"] == active_id
                && rule["frequency"] == "weekly"
                && rule["weekdays_mask"] == 5
                && rule["version"] == 5
        }));
    }

    #[test]
    fn local_mode_rejects_cloud_pull_and_lease() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("cloud-sync.sqlite3")).unwrap();
        assert!(pull(
            &database,
            CloudSyncPullRequest {
                after_change_seq: None,
                limit: None
            }
        )
        .is_err());
        assert!(acquire_lease(&database).is_err());
    }

    #[test]
    fn cloud_unassigned_refresh_falls_back_to_local_cache_without_session() {
        let _guard = CLOUD_SYNC_TEST_STATE_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap();
        CLOUD_SYNC_ENABLED.store(true, Ordering::Release);
        CLOUD_SYNC_ACTIVE.store(false, Ordering::Release);
        CLOUD_SYNC_SIGNALS.store(0, Ordering::Release);
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(
            directory
                .path()
                .join("cloud-unassigned-refresh-offline.sqlite3"),
        )
        .unwrap();
        let cloud_workspace_id = Uuid::now_v7().to_string();
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "cloud_workspace_id".to_string(),
                value: json!(cloud_workspace_id),
            },
        )
        .unwrap();
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "storage_mode".to_string(),
                value: json!("cloud"),
            },
        )
        .unwrap();

        let state = refresh_unassigned_state_for_cloud_mode(&database)
            .unwrap()
            .expect("本地缓存应继续提供未归属状态");
        assert_eq!(state.state, "collecting");
        assert_eq!(
            CLOUD_SYNC_SIGNALS.load(Ordering::Acquire) & SYNC_SIGNAL_CONNECTION_READY,
            0
        );
        assert_ne!(
            CLOUD_SYNC_SIGNALS.load(Ordering::Acquire) & SYNC_SIGNAL_LOCAL_DIRTY,
            0
        );
        CLOUD_SYNC_SIGNALS.store(0, Ordering::Release);
        CLOUD_SYNC_ACTIVE.store(false, Ordering::Release);
        let queued_before: i64 = database
            .open()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM sync_outbox", [], |row| row.get(0))
            .unwrap();
        let refreshed = refresh_unassigned_state_for_cloud_mode(&database)
            .unwrap()
            .expect("后续读取应继续返回本地缓存");
        assert_eq!(refreshed.session_id, state.session_id);
        let queued_after: i64 = database
            .open()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM sync_outbox", [], |row| row.get(0))
            .unwrap();
        assert_eq!(queued_after, queued_before);
        CLOUD_SYNC_ENABLED.store(false, Ordering::Release);
    }

    #[test]
    fn direct_snapshot_refresh_is_blocked_by_all_dirty_outbox_states() {
        for dirty_state in ["pending", "failed", "sending", "conflict"] {
            let (_directory, database, _) = cloud_database_with_outbox_state(
                &format!("cloud-refresh-dirty-{dirty_state}.sqlite3"),
                dirty_state,
            );

            let error = pull_snapshot(&database).unwrap_err();
            assert!(
                error.starts_with("SYNC_PENDING:"),
                "dirty outbox state {dirty_state} should block direct snapshot refresh, got {error}"
            );
        }
    }

    #[test]
    fn incremental_pull_is_not_globally_blocked_by_dirty_outbox_states() {
        for dirty_state in ["pending", "failed", "sending", "conflict"] {
            let (_directory, database, _) = cloud_database_with_outbox_state(
                &format!("cloud-pull-dirty-{dirty_state}.sqlite3"),
                dirty_state,
            );

            let error = pull(
                &database,
                CloudSyncPullRequest {
                    after_change_seq: None,
                    limit: None,
                },
            )
            .unwrap_err();
            assert!(
                !error.starts_with("SYNC_PENDING:"),
                "dirty outbox state {dirty_state} must be handled per entity, got {error}"
            );
        }
    }

    #[test]
    fn reset_snapshot_import_discards_all_dirty_outbox_states() {
        for dirty_state in ["pending", "failed", "sending", "conflict"] {
            let (_directory, database, workspace_id) = cloud_database_with_outbox_state(
                &format!("cloud-reset-dirty-{dirty_state}.sqlite3"),
                dirty_state,
            );
            let connection = database.open().unwrap();
            let device_id: String = connection
                .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
                .unwrap();
            let subject_id: String = connection
                .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
                .unwrap();
            let now = now_millis();
            drop(connection);

            apply_snapshot_with_reset_options(
                &database,
                &json!({
                    "subjects": [{
                        "id": subject_id,
                        "name": "云端名称",
                        "sort_order": 10,
                        "created_at": now,
                        "updated_at": now,
                        "version": 2,
                        "deleted_at": null
                    }],
                    "actual_time_adjustments": []
                }),
                None,
                Some(&workspace_id),
                Some((&workspace_id, &device_id, 12, 1)),
                false,
                true,
            )
            .unwrap();

            let connection = database.open().unwrap();
            let outbox_count: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM sync_outbox WHERE workspace_id = ?1",
                    [&workspace_id],
                    |row| row.get(0),
                )
                .unwrap();
            let subject_name: String = connection
                .query_row("SELECT name FROM subjects LIMIT 1", [], |row| row.get(0))
                .unwrap();
            assert_eq!(
                outbox_count, 0,
                "dirty state {dirty_state} should be discarded"
            );
            assert_eq!(subject_name, "云端名称");
        }
    }

    #[test]
    fn reset_snapshot_restores_today_instance_parent_links_after_all_tasks_exist() {
        let (_directory, database, workspace_id) =
            cloud_database_with_outbox_state("cloud-reset-today-parent.sqlite3", "pending");
        let connection = database.open().unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let subject_id: String = connection
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        drop(connection);
        let source_parent_id = Uuid::now_v7().to_string();
        let source_child_id = Uuid::now_v7().to_string();
        let instance_parent_id = Uuid::now_v7().to_string();
        let instance_child_id = Uuid::now_v7().to_string();
        let now = now_millis();
        let task = |id: &str,
                    parent_id: Option<&str>,
                    title: &str,
                    day: Option<&str>,
                    source: Option<&str>| {
            json!({
                "id":id,"subject_id":subject_id,"parent_id":parent_id,"title":title,"status":"open",
                "planned_date":null,"planned_time":null,"estimate_minutes":null,"note":null,
                "project_name":null,"solution_name":null,"source_type":"manual","source_ref":null,
                "sort_order":1,"completed_at":null,"created_at":now,"updated_at":now,"version":1,
                "deleted_at":null,"day_instance_date":day,"day_instance_kind":day.map(|_| "member"),
                "day_instance_source_task_id":source
            })
        };
        apply_snapshot_with_reset_options(
            &database,
            &json!({
                "subjects":[{"id":subject_id,"name":"云端主题","sort_order":1,"created_at":now,"updated_at":now,"version":1,"deleted_at":null}],
                "tasks":[
                    task(&instance_child_id,Some(&instance_parent_id),"实例子事项",Some("2026-10-22"),Some(&source_child_id)),
                    task(&source_child_id,Some(&source_parent_id),"源子事项",None,None),
                    task(&instance_parent_id,None,"实例父事项",Some("2026-10-22"),Some(&source_parent_id)),
                    task(&source_parent_id,None,"源父事项",None,None)
                ],
                "actual_time_adjustments":[]
            }),
            None,
            Some(&workspace_id),
            Some((&workspace_id, &device_id, 12, 1)),
            false,
            true,
        )
        .unwrap();
        let connection = database.open().unwrap();
        let restored_parent: String = connection
            .query_row(
                "SELECT parent_id FROM tasks WHERE id=?1",
                [&instance_child_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(restored_parent, instance_parent_id);
        let source_parent: String = connection
            .query_row(
                "SELECT parent_id FROM tasks WHERE id=?1",
                [&source_child_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(source_parent, source_parent_id);
    }

    #[test]
    fn cache_rebuild_waits_for_followup_outbox_before_reporting_success() {
        let (_directory, database, workspace_id) =
            cloud_database_with_outbox_state("cache-rebuild-followup.sqlite3", "pending");
        let status = finish_cache_rebuild_sync_with(&database, "重置本地数据", |database| {
            database
                .open()?
                .execute(
                    "DELETE FROM sync_outbox WHERE workspace_id=?1",
                    [&workspace_id],
                )
                .map_err(|error| error.to_string())?;
            Ok(CloudSyncPushResult {
                pushed: 1,
                pending: 0,
                conflicts: 0,
            })
        })
        .unwrap();

        assert_eq!(status.pending_operations, 0);
        assert_eq!(status.conflict_count, 0);
    }

    #[test]
    fn cache_rebuild_reports_followup_sync_failure_without_dropping_outbox() {
        let (_directory, database, workspace_id) =
            cloud_database_with_outbox_state("cache-rebuild-followup-failure.sqlite3", "pending");
        let error = finish_cache_rebuild_sync_with(&database, "重置本地数据", |_| {
            Err("NETWORK_ERROR: 测试网络中断".to_string())
        })
        .unwrap_err();

        assert!(error.starts_with("CACHE_REBUILD_SYNC_FAILED:"));
        assert!(error.contains("重置本地数据已完成"));
        let remaining: i64 = database
            .open()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM sync_outbox WHERE workspace_id=?1",
                [&workspace_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(remaining, 1);
    }

    #[test]
    fn keep_local_resolution_reports_success_only_after_push_and_refresh_converge() {
        let (_directory, database, workspace_id) =
            cloud_database_with_outbox_state("keep-local-converged.sqlite3", "pending");
        let pushed = std::cell::Cell::new(false);
        let pulled = std::cell::Cell::new(false);
        let status = finish_keep_local_conflict_resolution_with(
            &database,
            |database| {
                pushed.set(true);
                database
                    .open()?
                    .execute(
                        "DELETE FROM sync_outbox WHERE workspace_id=?1",
                        [&workspace_id],
                    )
                    .map_err(|error| error.to_string())?;
                Ok(CloudSyncPushResult {
                    pushed: 1,
                    pending: 0,
                    conflicts: 0,
                })
            },
            |_| {
                assert!(pushed.get());
                pulled.set(true);
                Ok(())
            },
        )
        .unwrap();

        assert!(pulled.get());
        assert_eq!(status.pending_operations, 0);
        assert_eq!(status.conflict_count, 0);
    }

    #[test]
    fn keep_local_resolution_does_not_report_success_while_push_is_pending() {
        let (_directory, database, _workspace_id) =
            cloud_database_with_outbox_state("keep-local-pending.sqlite3", "pending");
        let pull_called = std::cell::Cell::new(false);
        let error = finish_keep_local_conflict_resolution_with(
            &database,
            |_| {
                Ok(CloudSyncPushResult {
                    pushed: 0,
                    pending: 1,
                    conflicts: 0,
                })
            },
            |_| {
                pull_called.set(true);
                Ok(())
            },
        )
        .unwrap_err();

        assert!(error.starts_with("KEEP_LOCAL_OVERWRITE_PENDING:"));
        assert!(!pull_called.get());
    }

    fn cloud_database_with_outbox_state(
        file_name: &str,
        outbox_state: &str,
    ) -> (tempfile::TempDir, Database, String) {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(directory.path().join(file_name)).unwrap();
        let cloud_workspace_id = Uuid::now_v7().to_string();
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "cloud_workspace_id".to_string(),
                value: json!(cloud_workspace_id.clone()),
            },
        )
        .unwrap();
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "storage_mode".to_string(),
                value: json!("cloud"),
            },
        )
        .unwrap();
        let device_id = storage_mode(&database).unwrap().device_id;
        database
            .open()
            .unwrap()
            .execute(
                "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, state, attempt_count, created_at, error_json)
                 VALUES (?1, ?2, ?3, 'task_update', 'task', 'task-1', 1, '{}', ?4, 1, 1, '{}')",
                params![
                    Uuid::now_v7().to_string(),
                    &cloud_workspace_id,
                    device_id,
                    outbox_state
                ],
            )
            .unwrap();
        (directory, database, cloud_workspace_id)
    }

    #[test]
    fn sync_queue_lists_local_operation_details_without_network() {
        let (_directory, database, workspace_id) =
            cloud_database_with_outbox_state("cloud-sync-queue.sqlite3", "failed");
        database
            .open()
            .unwrap()
            .execute(
                "UPDATE sync_outbox
                 SET payload_json = ?1, attempt_count = 3, error_json = ?2, coalesced_count = 2
                 WHERE workspace_id = ?3",
                params![
                    json!({ "id": "task-1", "title": "待同步事项" }).to_string(),
                    json!({ "message": "NETWORK_ERROR: 暂时无法连接" }).to_string(),
                    &workspace_id
                ],
            )
            .unwrap();

        let items = list_sync_queue(&database, &workspace_id).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].state, "failed");
        assert_eq!(items[0].attempt_count, 3);
        assert_eq!(items[0].coalesced_count, 2);
        assert_eq!(items[0].payload["title"], "待同步事项");
        assert_eq!(
            items[0].error.as_deref(),
            Some("NETWORK_ERROR: 暂时无法连接")
        );
    }

    #[test]
    fn cloud_mode_queues_complete_entity_image_idempotently() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("cloud-outbox.sqlite3")).unwrap();
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "cloud_workspace_id".to_string(),
                value: json!(Uuid::now_v7().to_string()),
            },
        )
        .unwrap();
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "storage_mode".to_string(),
                value: json!("cloud"),
            },
        )
        .unwrap();
        let subject = create_subject(
            &database,
            SubjectCreateRequest {
                name: "离线主体".to_string(),
            },
        )
        .unwrap()
        .subject;
        let operation_id = Uuid::now_v7().to_string();
        enqueue_entity_deferred(
            &database,
            "subject_create",
            "subject",
            Some(&subject.id),
            None,
            Some(&operation_id),
        )
        .unwrap();
        enqueue_entity_deferred(
            &database,
            "subject_create",
            "subject",
            Some(&subject.id),
            None,
            Some(&operation_id),
        )
        .unwrap();
        let connection = database.open().unwrap();
        let row: (i64, String, String) = connection
            .query_row(
                "SELECT COUNT(*), entity_id, payload_json FROM sync_outbox WHERE operation_id = ?1",
                [&operation_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(row.0, 1);
        assert_eq!(row.1, subject.id);
        assert_eq!(
            serde_json::from_str::<Value>(&row.2).unwrap()["name"],
            "离线主体"
        );
        assert!(pull_snapshot(&database)
            .unwrap_err()
            .starts_with("SYNC_PENDING:"));
    }

    #[test]
    fn legacy_cloud_timer_commands_use_local_cache_and_outbox() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(
            directory
                .path()
                .join("legacy-cloud-timer-local-first.sqlite3"),
        )
        .unwrap();
        let cloud_workspace_id = Uuid::now_v7().to_string();
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "cloud_workspace_id".to_string(),
                value: json!(cloud_workspace_id.clone()),
            },
        )
        .unwrap();
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "storage_mode".to_string(),
                value: json!("cloud"),
            },
        )
        .unwrap();

        let start_operation_id = Uuid::now_v7().to_string();
        let started = legacy_cloud_timer_start_local_first(
            &database,
            CloudTimerStartRequest {
                task_id: None,
                note: Some("兼容入口".to_string()),
                operation_id: start_operation_id.clone(),
            },
        )
        .unwrap();
        let paused = legacy_cloud_timer_pause_local_first(
            &database,
            CloudTimerVersionRequest {
                entry_id: started.id.clone(),
                expected_version: started.version,
                operation_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        let resume_operation_id = Uuid::now_v7().to_string();
        let resumed = legacy_cloud_timer_resume_local_first(
            &database,
            CloudTimerVersionRequest {
                entry_id: started.id.clone(),
                expected_version: paused.version,
                operation_id: resume_operation_id.clone(),
            },
        )
        .unwrap();
        let stopped = legacy_cloud_timer_stop_local_first(
            &database,
            CloudTimerVersionRequest {
                entry_id: started.id.clone(),
                expected_version: resumed.version,
                operation_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();

        assert_eq!(stopped.state, "ended");
        assert!(stopped.allocations.is_empty());
        let connection = database.open().unwrap();
        let outbox = connection
            .prepare(
                "SELECT operation_id, operation_type, entity_type, entity_id, base_version, payload_version, coalesced_count, payload_json FROM sync_outbox WHERE workspace_id = ?1 ORDER BY created_at, rowid",
            )
            .unwrap()
            .query_map([cloud_workspace_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, String>(7)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(outbox.len(), 1);
        let timer = outbox.iter().find(|row| row.2 == "time_entry").unwrap();
        assert_eq!(
            timer.0,
            stable_timer_operation_id(&started.id, None, stopped.version, "timer_stop")
        );
        assert_eq!(timer.1, "timer_stop");
        assert_eq!(timer.3.as_deref(), Some(started.id.as_str()));
        assert_eq!(timer.4, None);
        assert_eq!(timer.5, Some(stopped.version));
        assert_eq!(timer.6, 4);
        let payload: Value = serde_json::from_str(&timer.7).unwrap();
        assert_eq!(payload["state"], "ended");
        assert_eq!(payload["version"], stopped.version);
        assert!(outbox.iter().all(|row| row.2 != "unassigned_session"));
    }

    #[test]
    fn legacy_cloud_unassigned_commands_use_local_cache_and_outbox() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(
            directory
                .path()
                .join("legacy-cloud-unassigned-local-first.sqlite3"),
        )
        .unwrap();
        let cloud_workspace_id = Uuid::now_v7().to_string();
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "cloud_workspace_id".to_string(),
                value: json!(cloud_workspace_id.clone()),
            },
        )
        .unwrap();
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "storage_mode".to_string(),
                value: json!("cloud"),
            },
        )
        .unwrap();

        let first = crate::unassigned::get_state(&database).unwrap().unwrap();
        database
            .open()
            .unwrap()
            .execute(
                "UPDATE unassigned_segments SET started_at = started_at - 60000 WHERE session_id = ?1 AND ended_at IS NULL",
                [&first.session_id],
            )
            .unwrap();
        let first = crate::unassigned::get_state(&database).unwrap().unwrap();
        let break_operation_id = Uuid::now_v7().to_string();
        let break_result = crate::unassigned::resolve_break_for_sync(
            &database,
            crate::unassigned::ResolveSessionRequest {
                session_id: first.session_id,
                expected_version: first.version,
                operation_id: break_operation_id.clone(),
            },
        )
        .unwrap();

        let second = crate::unassigned::get_state(&database).unwrap().unwrap();
        database
            .open()
            .unwrap()
            .execute(
                "UPDATE unassigned_segments SET started_at = started_at - 60000 WHERE session_id = ?1 AND ended_at IS NULL",
                [&second.session_id],
            )
            .unwrap();
        let second = crate::unassigned::get_state(&database).unwrap().unwrap();
        let discard_operation_id = Uuid::now_v7().to_string();
        let discard_result = crate::unassigned::discard_for_sync(
            &database,
            crate::unassigned::ResolveSessionRequest {
                session_id: second.session_id,
                expected_version: second.version,
                operation_id: discard_operation_id.clone(),
            },
        )
        .unwrap();

        let connection = database.open().unwrap();
        let outbox = connection
            .prepare(
                "SELECT operation_id, operation_type, entity_type, entity_id, base_version FROM sync_outbox WHERE workspace_id = ?1 ORDER BY created_at, rowid",
            )
            .unwrap()
            .query_map([cloud_workspace_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(outbox.len(), 2);
        assert_eq!(outbox[0].0, break_operation_id);
        assert_eq!(outbox[0].1, "unassigned_resolve_break");
        assert_eq!(outbox[0].2, "unassigned_session");
        assert_eq!(
            outbox[0].3.as_deref(),
            Some(break_result.session_id.as_str())
        );
        assert_eq!(outbox[1].0, discard_operation_id);
        assert_eq!(outbox[1].1, "unassigned_discard");
        assert_eq!(outbox[1].2, "unassigned_session");
        assert_eq!(
            outbox[1].3.as_deref(),
            Some(discard_result.session_id.as_str())
        );
    }

    #[test]
    fn outbox_conflict_is_counted_separately() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("cloud-conflict.sqlite3")).unwrap();
        let connection = database.open().unwrap();
        let local_workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        connection.execute(
            "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, state, attempt_count, created_at, error_json)
             VALUES (?1, ?2, ?3, 'task_update', 'task', ?4, 1, '{}', 'conflict', 1, ?5, '{}')",
            params![Uuid::now_v7().to_string(), &local_workspace_id, &device_id, Uuid::now_v7().to_string(), now_millis()],
        ).unwrap();
        assert_eq!(
            outbox_counts(&database, &local_workspace_id).unwrap(),
            (0, 1)
        );

        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "cloud_workspace_id".to_string(),
                value: json!(local_workspace_id),
            },
        )
        .unwrap();
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "storage_mode".to_string(),
                value: json!("cloud"),
            },
        )
        .unwrap();
        let conflicts = list_conflicts(&database).unwrap();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].entity_type, "task");
        assert_eq!(conflicts[0].base_version, Some(1));
        assert_eq!(conflicts[0].local_payload, json!({}));
    }

    #[test]
    fn missing_task_occurrence_conflict_rebases_latest_local_state_as_create() {
        let (_directory, database, _cloud_workspace_id, workspace_id, subject_id) =
            cloud_database_for_incremental_test("missing-task-occurrence-rebase.sqlite3");
        let task_id = Uuid::now_v7().to_string();
        let entity_id = format!("{task_id}|2026-10-19");
        let conflict_id = Uuid::now_v7().to_string();
        let following_id = Uuid::now_v7().to_string();
        let connection = database.open().unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        connection.execute(
            "INSERT INTO tasks(id,workspace_id,subject_id,title,status,source_type,sort_order,created_at,updated_at,version) VALUES(?1,(SELECT id FROM workspaces LIMIT 1),?2,'周期冲突','open','manual',10,1,1,1)",
            params![task_id, subject_id],
        ).unwrap();
        connection.execute(
            "INSERT INTO task_occurrences(workspace_id,task_id,occurrence_date,origin,status,created_at,updated_at,version) VALUES((SELECT id FROM workspaces LIMIT 1),?1,'2026-10-19','scheduled','open',1,3,3)",
            [&task_id],
        ).unwrap();
        connection.execute(
            "INSERT INTO sync_outbox(operation_id,workspace_id,device_id,operation_type,entity_type,entity_id,base_version,payload_json,payload_version,state,attempt_count,created_at,error_json) VALUES(?1,?2,?3,'task_set_completed','task_occurrence',?4,1,?5,2,'conflict',1,1,?6)",
            params![
                conflict_id,
                workspace_id,
                device_id,
                entity_id,
                json!({"task_id":task_id,"occurrence_date":"2026-10-19","origin":"scheduled","status":"done","completed_at":2,"created_at":1,"updated_at":2,"version":2}).to_string(),
                json!({"message":"CLOUD_REQUEST_FAILED: SYNC_CONFLICT: expected version 1, current version <NULL>"}).to_string()
            ],
        ).unwrap();
        connection.execute(
            "INSERT INTO sync_outbox(operation_id,workspace_id,device_id,operation_type,entity_type,entity_id,base_version,payload_json,payload_version,state,attempt_count,created_at) VALUES(?1,?2,?3,'task_set_completed','task_occurrence',?4,2,?5,3,'pending',0,2)",
            params![
                following_id,
                workspace_id,
                device_id,
                entity_id,
                json!({"task_id":task_id,"occurrence_date":"2026-10-19","origin":"scheduled","status":"open","completed_at":null,"created_at":1,"updated_at":3,"version":3}).to_string()
            ],
        ).unwrap();
        drop(connection);

        assert_eq!(
            recover_historical_missing_task_occurrences(&database, &workspace_id).unwrap(),
            1
        );
        let connection = database.open().unwrap();
        let row: (Option<i64>, i64, String, String, i64) = connection.query_row(
            "SELECT base_version,payload_version,payload_json,state,attempt_count FROM sync_outbox WHERE operation_id=?1",
            [&conflict_id],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?)),
        ).unwrap();
        assert_eq!(row.0, None);
        assert_eq!(row.1, 1);
        assert_eq!(row.3, "pending");
        assert_eq!(row.4, 0);
        let payload: Value = serde_json::from_str(&row.2).unwrap();
        assert_eq!(payload["status"], "open");
        assert_eq!(payload["version"], 1);
        assert_eq!(connection.query_row("SELECT version FROM task_occurrences WHERE task_id=?1 AND occurrence_date='2026-10-19'",[&task_id],|row|row.get::<_,i64>(0)).unwrap(),1);
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM sync_outbox WHERE operation_id=?1",
                    [&following_id],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn missing_entity_rebase_remains_restricted_to_task_occurrences() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("missing-task-rebase-guard.sqlite3"))
                .unwrap();
        let workspace_id: String = database
            .open()
            .unwrap()
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let operation = OutboxOperation {
            operation_id: Uuid::now_v7().to_string(),
            workspace_epoch: 1,
            operation_type: "task_update".to_string(),
            entity_type: "task".to_string(),
            entity_id: Some(Uuid::now_v7().to_string()),
            base_version: Some(2),
            payload_version: Some(3),
            coalesced_count: 1,
            depends_on_operation_id: None,
            payload: json!({"version":3}),
        };
        assert!(
            rebase_missing_task_occurrence(&database, &workspace_id, &operation)
                .unwrap_err()
                .contains("只有周期事项轮次")
        );
    }

    #[test]
    fn timer_transition_adopts_authoritative_occurrence_without_reverting_newer_local_state() {
        let (_directory, database, workspace_id, local_workspace_id, subject_id) =
            cloud_database_for_incremental_test(
                "timer-transition-authoritative-occurrence.sqlite3",
            );
        let task_id = Uuid::now_v7().to_string();
        let connection = database.open().unwrap();
        connection.execute(
            "INSERT INTO tasks(id,workspace_id,subject_id,title,status,source_type,sort_order,created_at,updated_at,version) VALUES(?1,?2,?3,'周期权威轮次','open','manual',10,1,1,1)",
            params![task_id,local_workspace_id,subject_id],
        ).unwrap();
        connection.execute(
            "INSERT INTO task_occurrences(workspace_id,task_id,occurrence_date,origin,status,created_at,updated_at,version) VALUES(?1,?2,'2026-10-19','manual','open',1,1,1)",
            params![local_workspace_id,task_id],
        ).unwrap();
        drop(connection);
        let operation = OutboxOperation {
            operation_id: Uuid::now_v7().to_string(),
            workspace_epoch: 1,
            operation_type: "timer_transition_apply".to_string(),
            entity_type: "timer_transition".to_string(),
            entity_id: Some(Uuid::now_v7().to_string()),
            base_version: None,
            payload_version: None,
            coalesced_count: 1,
            depends_on_operation_id: None,
            payload: json!({
                "occurrence": {"task_id":task_id,"occurrence_date":"2026-10-19","version":1}
            }),
        };
        adopt_timer_transition_occurrence_result(&database,&operation,&json!({
            "occurrence": {"task_id":task_id,"occurrence_date":"2026-10-19","origin":"scheduled","status":"open","completed_at":null,"created_at":10,"updated_at":10,"version":1}
        })).unwrap();
        let row: (String,i64,i64) = database.open().unwrap().query_row(
            "SELECT origin,created_at,version FROM task_occurrences WHERE task_id=?1 AND occurrence_date='2026-10-19'",
            [&task_id],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
        ).unwrap();
        assert_eq!(row, ("scheduled".to_string(), 1, 1));

        database.open().unwrap().execute(
            "UPDATE task_occurrences SET status='done',completed_at=20,updated_at=20,version=2 WHERE task_id=?1",
            [&task_id],
        ).unwrap();
        adopt_timer_transition_occurrence_result(&database,&operation,&json!({
            "occurrence": {"task_id":task_id,"occurrence_date":"2026-10-19","origin":"scheduled","status":"open","completed_at":null,"created_at":10,"updated_at":10,"version":1}
        })).unwrap();
        let row: (String,i64) = database.open().unwrap().query_row(
            "SELECT status,version FROM task_occurrences WHERE task_id=?1 AND occurrence_date='2026-10-19'",
            [&task_id],
            |row| Ok((row.get(0)?,row.get(1)?)),
        ).unwrap();
        assert_eq!(row, ("done".to_string(), 2));
        assert!(!workspace_id.is_empty());
    }

    #[test]
    fn actual_adjustment_batch_use_cloud_rolls_back_only_its_optimistic_images() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(
            directory
                .path()
                .join("actual-adjustment-batch-use-cloud.sqlite3"),
        )
        .unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let subject_id: String = connection
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let operation_id = Uuid::now_v7().to_string();
        let batch_id = Uuid::now_v7().to_string();
        let existing_task_id = Uuid::now_v7().to_string();
        let created_task_id = Uuid::now_v7().to_string();
        let existing_adjustment_id = Uuid::now_v7().to_string();
        let created_adjustment_id = Uuid::now_v7().to_string();
        connection.execute("INSERT INTO tasks(id,workspace_id,subject_id,title,status,source_type,sort_order,created_at,updated_at,version) VALUES(?1,?2,?3,'保留事项','open','manual',10,1,1,1)",params![existing_task_id,workspace_id,subject_id]).unwrap();
        connection.execute("INSERT INTO tasks(id,workspace_id,subject_id,title,status,planned_date,source_type,source_ref,sort_order,created_at,updated_at,version) VALUES(?1,?2,?3,'撤销事项','open','2026-10-15','manual','actual_adjustment_quick_create',20,1,1,1)",params![created_task_id,workspace_id,subject_id]).unwrap();
        for (id, task_id, delta, baseline, target) in [
            (
                &existing_adjustment_id,
                &existing_task_id,
                5_i64,
                0_i64,
                5_i64,
            ),
            (
                &created_adjustment_id,
                &created_task_id,
                15_i64,
                0_i64,
                15_i64,
            ),
        ] {
            connection.execute("INSERT INTO actual_time_adjustments(id,workspace_id,subject_id,task_id,work_date,delta_minutes,baseline_minutes,target_minutes,batch_id,created_at,updated_at,version) VALUES(?1,?2,?3,?4,'2026-10-15',?5,?6,?7,?8,1,1,1)",params![id,workspace_id,subject_id,task_id,delta,baseline,target,batch_id]).unwrap();
        }
        connection.execute("INSERT INTO actual_adjustment_batches(workspace_id,client_request_id,batch_id,request_hash,result_json,created_at) VALUES(?1,?2,?3,'hash','{}',1)",params![workspace_id,operation_id,batch_id]).unwrap();
        let payload = json!({
            "createdTasks": [{ "clientTaskId": "draft", "task": { "id": created_task_id } }],
            "adjustments": [{ "id": existing_adjustment_id }, { "id": created_adjustment_id }]
        });
        connection.execute("INSERT INTO sync_outbox(operation_id,workspace_id,device_id,operation_type,entity_type,entity_id,payload_json,state,attempt_count,created_at,error_json) VALUES(?1,?2,?3,'actual_adjustment_batch_apply','actual_adjustment_batch',?4,?5,'conflict',1,1,'{}')",params![operation_id,workspace_id,device_id,batch_id,payload.to_string()]).unwrap();
        drop(connection);

        let conflict = load_conflict(&database, &workspace_id, &operation_id).unwrap();
        rollback_actual_adjustment_batch_local_images(&database, &conflict).unwrap();
        let connection = database.open().unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM tasks WHERE id=?1",
                    [created_task_id],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM tasks WHERE id=?1",
                    [existing_task_id],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM actual_time_adjustments WHERE batch_id=?1",
                    [batch_id],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM actual_adjustment_batches WHERE client_request_id=?1",
                    [operation_id],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn actual_adjustment_batch_success_adopts_rebased_cloud_delta() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(
            directory
                .path()
                .join("actual-adjustment-batch-adopt-rebase.sqlite3"),
        )
        .unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let subject_id: String = connection
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let task_id = Uuid::now_v7().to_string();
        let adjustment_id = Uuid::now_v7().to_string();
        let discarded_id = Uuid::now_v7().to_string();
        let batch_id = Uuid::now_v7().to_string();
        connection.execute("INSERT INTO tasks(id,workspace_id,subject_id,title,status,source_type,sort_order,created_at,updated_at,version) VALUES(?1,?2,?3,'重算目标','open','manual',10,1,1,1)",params![task_id,workspace_id,subject_id]).unwrap();
        for (id, delta, baseline, target) in [
            (&adjustment_id, 15_i64, 60_i64, 75_i64),
            (&discarded_id, 5_i64, 75_i64, 80_i64),
        ] {
            connection.execute("INSERT INTO actual_time_adjustments(id,workspace_id,subject_id,task_id,work_date,delta_minutes,baseline_minutes,target_minutes,batch_id,created_at,updated_at,version) VALUES(?1,?2,?3,?4,'2026-10-20',?5,?6,?7,?8,1,1,1)",params![id,workspace_id,subject_id,task_id,delta,baseline,target,batch_id]).unwrap();
        }
        drop(connection);
        let operation = OutboxOperation {
            operation_id: Uuid::now_v7().to_string(),
            workspace_epoch: 1,
            operation_type: "actual_adjustment_batch_apply".to_string(),
            entity_type: "actual_adjustment_batch".to_string(),
            entity_id: Some(batch_id.clone()),
            base_version: None,
            payload_version: None,
            coalesced_count: 1,
            depends_on_operation_id: None,
            payload: json!({
                "adjustments": [{"id": adjustment_id}, {"id": discarded_id}]
            }),
        };
        adopt_actual_adjustment_batch_result(
            &database,
            &operation,
            &json!({
                "adjustments": [{
                    "id": adjustment_id,
                    "delta_minutes": -20,
                    "baseline_minutes": 75,
                    "target_minutes": 55,
                    "note": null,
                    "updated_at": 20,
                    "version": 1,
                    "updated_by_device_id": device_id,
                    "deleted_at": null
                }],
                "discardedAdjustmentIds": [discarded_id]
            }),
        )
        .unwrap();
        let connection = database.open().unwrap();
        let adopted: (i64,i64,i64) = connection.query_row(
            "SELECT delta_minutes,baseline_minutes,target_minutes FROM actual_time_adjustments WHERE id=?1",
            [&adjustment_id],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
        ).unwrap();
        assert_eq!(adopted, (-20, 75, 55));
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM actual_time_adjustments WHERE id=?1",
                    [discarded_id],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn use_cloud_conflict_resolution_deletes_following_operations_for_same_entity_only() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(
            directory
                .path()
                .join("cloud-use-cloud-clears-entity-chain.sqlite3"),
        )
        .unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let task_id = Uuid::now_v7().to_string();
        let other_task_id = Uuid::now_v7().to_string();
        let conflict_id = Uuid::now_v7().to_string();
        let following_same_entity_id = Uuid::now_v7().to_string();
        let dependent_id = Uuid::now_v7().to_string();
        let other_entity_id = Uuid::now_v7().to_string();
        for (operation_id, entity_id, state, created_at) in [
            (&conflict_id, &task_id, "conflict", 1_i64),
            (&following_same_entity_id, &task_id, "pending", 2_i64),
            (&other_entity_id, &other_task_id, "pending", 3_i64),
        ] {
            connection
                .execute(
                    "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, state, attempt_count, created_at, error_json)
                     VALUES (?1, ?2, ?3, 'task_update', 'task', ?4, 1, '{}', ?5, 0, ?6, '{}')",
                    params![
                        operation_id,
                        &workspace_id,
                        &device_id,
                        entity_id,
                        state,
                        created_at
                    ],
                )
                .unwrap();
        }
        connection
            .execute(
                "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, depends_on_operation_id, state, attempt_count, created_at, error_json)
                 VALUES (?1, ?2, ?3, 'task_set_completed_from_allocation', 'task', ?4, 1, '{}', ?5, 'pending', 0, 4, '{}')",
                params![
                    &dependent_id,
                    &workspace_id,
                    &device_id,
                    Uuid::now_v7().to_string(),
                    &following_same_entity_id
                ],
            )
            .unwrap();
        drop(connection);

        let conflict = load_conflict(&database, &workspace_id, &conflict_id).unwrap();
        delete_outbox_entity_chain(&database, &workspace_id, &conflict).unwrap();

        let connection = database.open().unwrap();
        let remaining = connection
            .prepare("SELECT operation_id FROM sync_outbox ORDER BY created_at")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(remaining, vec![other_entity_id]);
    }

    #[test]
    fn batch_use_cloud_resolution_clears_all_current_conflicts_once() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(
            directory
                .path()
                .join("cloud-batch-use-cloud-conflicts.sqlite3"),
        )
        .unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "cloud_workspace_id".to_string(),
                value: json!(workspace_id),
            },
        )
        .unwrap();
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "storage_mode".to_string(),
                value: json!("cloud"),
            },
        )
        .unwrap();
        let state = storage_mode(&database).unwrap();
        let first_entity_id = Uuid::now_v7().to_string();
        let second_entity_id = Uuid::now_v7().to_string();
        let unrelated_entity_id = Uuid::now_v7().to_string();
        let first_conflict_id = Uuid::now_v7().to_string();
        let first_following_id = Uuid::now_v7().to_string();
        let first_dependent_id = Uuid::now_v7().to_string();
        let second_conflict_id = Uuid::now_v7().to_string();
        let unrelated_id = Uuid::now_v7().to_string();
        for (operation_id, entity_id, state, created_at) in [
            (&first_conflict_id, &first_entity_id, "conflict", 1_i64),
            (&first_following_id, &first_entity_id, "pending", 2_i64),
            (&second_conflict_id, &second_entity_id, "conflict", 4_i64),
            (&unrelated_id, &unrelated_entity_id, "pending", 5_i64),
        ] {
            connection
                .execute(
                    "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, state, attempt_count, created_at, error_json)
                     VALUES (?1, ?2, ?3, 'task_update', 'task', ?4, 1, '{}', ?5, 0, ?6, '{}')",
                    params![operation_id, &workspace_id, &device_id, entity_id, state, created_at],
                )
                .unwrap();
        }
        connection
            .execute(
                "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, depends_on_operation_id, state, attempt_count, created_at)
                 VALUES (?1, ?2, ?3, 'task_set_completed_from_allocation', 'task', ?4, 1, '{}', ?5, 'pending', 0, 3)",
                params![
                    &first_dependent_id,
                    &workspace_id,
                    &device_id,
                    Uuid::now_v7().to_string(),
                    &first_following_id
                ],
            )
            .unwrap();
        drop(connection);

        let mut attempts = HashMap::new();
        let resolved =
            resolve_current_conflicts(&database, &state, "use_cloud", &mut attempts).unwrap();

        assert_eq!(resolved, 2);
        assert!(list_conflicts(&database).unwrap().is_empty());
        let remaining = database
            .open()
            .unwrap()
            .prepare("SELECT operation_id FROM sync_outbox ORDER BY created_at")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(remaining, vec![unrelated_id]);
        assert_eq!(attempts.get(&first_conflict_id), Some(&1));
        assert_eq!(attempts.get(&second_conflict_id), Some(&1));
    }

    #[test]
    fn batch_conflict_resolution_skips_same_entity_conflicts_already_collapsed() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(
            directory
                .path()
                .join("cloud-batch-skips-collapsed-conflicts.sqlite3"),
        )
        .unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "cloud_workspace_id".to_string(),
                value: json!(workspace_id),
            },
        )
        .unwrap();
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "storage_mode".to_string(),
                value: json!("cloud"),
            },
        )
        .unwrap();
        let state = storage_mode(&database).unwrap();
        let entity_id = Uuid::now_v7().to_string();
        let first_conflict_id = Uuid::now_v7().to_string();
        let second_conflict_id = Uuid::now_v7().to_string();
        for (operation_id, created_at) in
            [(&first_conflict_id, 1_i64), (&second_conflict_id, 2_i64)]
        {
            connection
                .execute(
                    "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, state, attempt_count, created_at, error_json)
                     VALUES (?1, ?2, ?3, 'task_update', 'task', ?4, 1, '{}', 'conflict', 0, ?5, '{}')",
                    params![operation_id, &workspace_id, &device_id, &entity_id, created_at],
                )
                .unwrap();
        }
        drop(connection);

        let mut attempts = HashMap::new();
        let listed =
            resolve_current_conflicts(&database, &state, "use_cloud", &mut attempts).unwrap();

        assert_eq!(listed, 2);
        assert!(list_conflicts(&database).unwrap().is_empty());
        assert_eq!(attempts.get(&first_conflict_id), Some(&1));
        assert_eq!(attempts.get(&second_conflict_id), Some(&1));
    }

    #[test]
    fn time_entry_coalescing_stops_at_non_replaceable_outbox_states() {
        for boundary_state in ["sending", "failed", "conflict"] {
            let directory = tempdir().unwrap();
            let database = Database::initialize_at(
                directory
                    .path()
                    .join(format!("timer-coalesce-{boundary_state}.sqlite3")),
            )
            .unwrap();
            let mut connection = database.open().unwrap();
            let workspace_id: String = connection
                .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
                .unwrap();
            let device_id: String = connection
                .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
                .unwrap();
            let entry_id = Uuid::now_v7().to_string();
            let boundary_id = Uuid::now_v7().to_string();
            connection
                .execute(
                    "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, payload_version, state, attempt_count, created_at)
                     VALUES (?1, ?2, ?3, 'timer_start', 'time_entry', ?4, NULL, ?5, 1, ?6, 0, 1)",
                    params![
                        &boundary_id,
                        &workspace_id,
                        &device_id,
                        &entry_id,
                        json!({ "id": entry_id, "version": 1 }).to_string(),
                        boundary_state
                    ],
                )
                .unwrap();

            let transaction = connection.transaction().unwrap();
            let result = coalesce_pending_time_entry(
                &transaction,
                &workspace_id,
                Some(&entry_id),
                "timer_stop",
                &json!({ "id": entry_id, "version": 2, "state": "ended" }),
            )
            .unwrap();
            assert_eq!(
                result, None,
                "state {boundary_state} must be a hard boundary"
            );
            transaction.commit().unwrap();

            let row: (String, i64, i64) = connection
                .query_row(
                    "SELECT state, payload_version, coalesced_count FROM sync_outbox WHERE operation_id = ?1",
                    [&boundary_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap();
            assert_eq!(row, (boundary_state.to_string(), 1, 1));
        }
    }

    #[test]
    fn time_entry_coalescing_rejects_an_unexplained_version_gap() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("timer-coalesce-version-gap.sqlite3"))
                .unwrap();
        let mut connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let entry_id = Uuid::now_v7().to_string();
        let operation_id = Uuid::now_v7().to_string();
        connection
            .execute(
                "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, payload_version, state, attempt_count, created_at)
                 VALUES (?1, ?2, ?3, 'timer_pause', 'time_entry', ?4, 4, ?5, 6, 'pending', 0, 1)",
                params![
                    &operation_id,
                    &workspace_id,
                    &device_id,
                    &entry_id,
                    json!({ "id": entry_id, "version": 6 }).to_string()
                ],
            )
            .unwrap();

        let transaction = connection.transaction().unwrap();
        let result = coalesce_pending_time_entry(
            &transaction,
            &workspace_id,
            Some(&entry_id),
            "timer_stop",
            &json!({ "id": entry_id, "version": 7, "state": "ended" }),
        )
        .unwrap();
        assert_eq!(result, None);
        transaction.commit().unwrap();

        let row: (String, i64, i64) = connection
            .query_row(
                "SELECT operation_type, payload_version, coalesced_count FROM sync_outbox WHERE operation_id = ?1",
                [&operation_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(row, ("timer_pause".to_string(), 6, 1));
    }

    #[test]
    fn unassigned_lifecycle_coalescing_preserves_terminal_and_sending_boundaries() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(
            directory
                .path()
                .join("unassigned-lifecycle-coalescing.sqlite3"),
        )
        .unwrap();
        let mut connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let session_id = Uuid::now_v7().to_string();
        let first_operation_id = Uuid::now_v7().to_string();
        connection.execute(
            "INSERT INTO sync_outbox(operation_id,workspace_id,device_id,operation_type,entity_type,entity_id,base_version,payload_json,payload_version,state,attempt_count,coalesced_count,created_at)
             VALUES (?1,?2,?3,'unassigned_session_pause','unassigned_session',?4,1,?5,2,'pending',0,1,1)",
            params![first_operation_id,workspace_id,device_id,session_id,json!({"id":session_id,"version":2}).to_string()],
        ).unwrap();
        let transaction = connection.transaction().unwrap();
        let coalesced = coalesce_pending_unassigned_lifecycle(
            &transaction,
            &workspace_id,
            Some(&session_id),
            "unassigned_session_resume",
            &json!({"id":session_id,"version":3}),
        )
        .unwrap();
        assert_eq!(coalesced.as_deref(), Some(first_operation_id.as_str()));
        transaction.commit().unwrap();
        let row: (String,i64,i64) = connection.query_row(
            "SELECT operation_type,payload_version,coalesced_count FROM sync_outbox WHERE operation_id=?1",
            [&first_operation_id],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
        ).unwrap();
        assert_eq!(row, ("unassigned_session_resume".to_string(), 3, 2));

        connection
            .execute(
                "UPDATE sync_outbox SET state='sending' WHERE operation_id=?1",
                [&first_operation_id],
            )
            .unwrap();
        let transaction = connection.transaction().unwrap();
        assert_eq!(
            coalesce_pending_unassigned_lifecycle(
                &transaction,
                &workspace_id,
                Some(&session_id),
                "unassigned_session_pause",
                &json!({"id":session_id,"version":4})
            )
            .unwrap(),
            None
        );
        transaction.commit().unwrap();

        connection
            .execute(
                "UPDATE sync_outbox SET state='pending',attempt_count=0 WHERE operation_id=?1",
                [&first_operation_id],
            )
            .unwrap();
        connection.execute(
            "INSERT INTO sync_outbox(operation_id,workspace_id,device_id,operation_type,entity_type,entity_id,base_version,payload_json,payload_version,state,attempt_count,coalesced_count,created_at)
             VALUES (?1,?2,?3,'unassigned_resolve_break','unassigned_session',?4,3,?5,4,'pending',0,1,2)",
            params![Uuid::now_v7().to_string(),workspace_id,device_id,session_id,json!({"id":session_id,"version":4}).to_string()],
        ).unwrap();
        let transaction = connection.transaction().unwrap();
        assert_eq!(
            coalesce_pending_unassigned_lifecycle(
                &transaction,
                &workspace_id,
                Some(&session_id),
                "unassigned_session_resume",
                &json!({"id":session_id,"version":5})
            )
            .unwrap(),
            None
        );
        transaction.commit().unwrap();
    }

    #[test]
    fn keep_local_conflict_resolution_uses_latest_following_payload_for_same_entity() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(
            directory
                .path()
                .join("cloud-keep-local-uses-latest-entity-chain.sqlite3"),
        )
        .unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let task_id = Uuid::now_v7().to_string();
        let conflict_id = Uuid::now_v7().to_string();
        let middle_id = Uuid::now_v7().to_string();
        let latest_id = Uuid::now_v7().to_string();
        let dependent_id = Uuid::now_v7().to_string();
        for (operation_id, operation_type, state, title, version, created_at) in [
            (
                &conflict_id,
                "task_update",
                "conflict",
                "本地旧标题",
                2_i64,
                1_i64,
            ),
            (
                &middle_id,
                "task_update",
                "pending",
                "本地中间标题",
                3_i64,
                2_i64,
            ),
            (
                &latest_id,
                "task_set_completed",
                "pending",
                "本地最终标题",
                4_i64,
                3_i64,
            ),
        ] {
            connection
                .execute(
                    "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, payload_version, state, attempt_count, created_at, error_json)
                     VALUES (?1, ?2, ?3, ?4, 'task', ?5, ?6, ?7, ?8, ?9, 0, ?10, '{}')",
                    params![
                        operation_id,
                        &workspace_id,
                        &device_id,
                        operation_type,
                        &task_id,
                        version - 1,
                        json!({ "id": task_id, "title": title, "version": version }).to_string(),
                        version,
                        state,
                        created_at
                    ],
                )
                .unwrap();
        }
        connection
            .execute(
                "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, depends_on_operation_id, state, attempt_count, created_at)
                 VALUES (?1, ?2, ?3, 'report_update', 'report', ?4, 1, '{}', ?5, 'pending', 0, 4)",
                params![
                    &dependent_id,
                    &workspace_id,
                    &device_id,
                    Uuid::now_v7().to_string(),
                    &latest_id
                ],
            )
            .unwrap();
        drop(connection);

        let conflict = load_conflict(&database, &workspace_id, &conflict_id).unwrap();
        let latest = latest_outbox_entity_operation(&database, &workspace_id, &conflict).unwrap();
        assert_eq!(latest.operation_id, latest_id);
        assert_eq!(latest.payload["title"], "本地最终标题");

        delete_following_outbox_entity_operations(&database, &workspace_id, &latest, &conflict_id)
            .unwrap();
        let connection = database.open().unwrap();
        let remaining = connection
            .prepare(
                "SELECT operation_id, depends_on_operation_id FROM sync_outbox ORDER BY created_at",
            )
            .unwrap()
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            remaining,
            vec![
                (conflict_id.clone(), None),
                (dependent_id, Some(conflict_id))
            ]
        );
    }

    #[test]
    fn flush_if_online_preserves_queued_conflicts_for_background_sync() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("cloud-existing-conflict.sqlite3"))
                .unwrap();
        let connection = database.open().unwrap();
        let local_workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        connection
            .execute(
                "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, state, attempt_count, created_at, error_json)
                 VALUES (?1, ?2, ?3, 'task_update', 'task', ?4, 1, '{}', 'conflict', 1, ?5, '{}')",
                params![
                    Uuid::now_v7().to_string(),
                    &local_workspace_id,
                    &device_id,
                    Uuid::now_v7().to_string(),
                    now_millis()
                ],
            )
            .unwrap();
        drop(connection);

        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "cloud_workspace_id".to_string(),
                value: json!(local_workspace_id),
            },
        )
        .unwrap();
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "storage_mode".to_string(),
                value: json!("cloud"),
            },
        )
        .unwrap();

        flush_if_online(&database).unwrap();
        let queued: i64 = database
            .open()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM sync_outbox WHERE state = 'conflict'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(queued, 1);
    }

    #[test]
    fn outbox_claim_skips_fresh_sending_and_retries_stale_sending() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("cloud-claim.sqlite3")).unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let pending_id = Uuid::now_v7().to_string();
        let fresh_sending_id = Uuid::now_v7().to_string();
        let stale_sending_id = Uuid::now_v7().to_string();
        let now = now_millis();
        let stale_attempt_at = now - OUTBOX_SENDING_STALE_AFTER_MILLIS - 1;
        for (operation_id, state, attempt_count, last_attempt_at) in [
            (&pending_id, "pending", 0_i64, None),
            (&fresh_sending_id, "sending", 3_i64, Some(now)),
            (&stale_sending_id, "sending", 2_i64, Some(stale_attempt_at)),
        ] {
            connection
                .execute(
                    "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, state, attempt_count, created_at, last_attempt_at)
                     VALUES (?1, ?2, ?3, 'subject_create', 'subject', ?4, NULL, '{}', ?5, ?6, ?7, ?8)",
                    params![operation_id, &workspace_id, &device_id, Uuid::now_v7().to_string(), state, attempt_count, now, last_attempt_at],
                )
                .unwrap();
        }
        drop(connection);

        let claimed = claim_pending_operations(&database, &workspace_id).unwrap();
        assert_eq!(claimed.len(), 2);
        assert!(claimed
            .iter()
            .any(|operation| operation.operation_id == pending_id));
        assert!(claimed
            .iter()
            .any(|operation| operation.operation_id == stale_sending_id));

        let connection = database.open().unwrap();
        let pending_state: (String, i64) = connection
            .query_row(
                "SELECT state, attempt_count FROM sync_outbox WHERE operation_id = ?1",
                [&pending_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let fresh_sending_state: (String, i64) = connection
            .query_row(
                "SELECT state, attempt_count FROM sync_outbox WHERE operation_id = ?1",
                [&fresh_sending_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let stale_sending_state: (String, i64) = connection
            .query_row(
                "SELECT state, attempt_count FROM sync_outbox WHERE operation_id = ?1",
                [&stale_sending_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(pending_state, ("sending".to_string(), 1));
        assert_eq!(fresh_sending_state, ("sending".to_string(), 3));
        assert_eq!(stale_sending_state, ("sending".to_string(), 3));
    }

    #[test]
    fn release_unprocessed_claims_returns_queued_operations_to_pending() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("cloud-release-claims.sqlite3")).unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let first_id = Uuid::now_v7().to_string();
        let second_id = Uuid::now_v7().to_string();
        let now = now_millis();
        for operation_id in [&first_id, &second_id] {
            connection
                .execute(
                    "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, state, attempt_count, created_at, last_attempt_at)
                     VALUES (?1, ?2, ?3, 'task_update', 'task', ?4, 1, '{}', 'sending', 1, ?5, ?5)",
                    params![operation_id, &workspace_id, &device_id, operation_id, now],
                )
                .unwrap();
        }
        drop(connection);

        let operation = OutboxOperation {
            operation_id: second_id.clone(),
            workspace_epoch: 1,
            operation_type: "task_update".to_string(),
            entity_type: "task".to_string(),
            entity_id: Some(second_id.clone()),
            base_version: Some(1),
            payload_version: None,
            coalesced_count: 1,
            depends_on_operation_id: None,
            payload: Value::Object(Default::default()),
        };
        release_unprocessed_claims(&database, &[operation]).unwrap();

        let state: String = database
            .open()
            .unwrap()
            .query_row(
                "SELECT state FROM sync_outbox WHERE operation_id = ?1",
                [&second_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(state, "pending");
    }

    #[test]
    fn manual_claim_retries_recent_pending_operation_immediately() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("cloud-manual-retry.sqlite3")).unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let operation_id = Uuid::now_v7().to_string();
        let now = now_millis();
        connection
            .execute(
                "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, state, attempt_count, created_at, last_attempt_at)
                 VALUES (?1, ?2, ?3, 'task_update', 'task', ?4, 1, '{}', 'pending', 3, ?5, ?5)",
                params![&operation_id, &workspace_id, &device_id, &operation_id, now],
            )
            .unwrap();
        drop(connection);

        let automatic = claim_pending_operations(&database, &workspace_id).unwrap();
        assert!(automatic.is_empty());
        let manual =
            claim_pending_operations_with_retry_mode(&database, &workspace_id, true).unwrap();
        assert_eq!(manual.len(), 1);
        assert_eq!(manual[0].operation_id, operation_id);
    }

    #[test]
    fn timer_idempotency_conflict_repair_marks_candidate_without_mutating_chain() {
        let (_directory, database, workspace_id) =
            cloud_database_with_outbox_state("timer-idempotency-repair.sqlite3", "pending");
        let connection = database.open().unwrap();
        connection
            .execute(
                "DELETE FROM sync_outbox WHERE workspace_id=?1",
                [&workspace_id],
            )
            .unwrap();
        let device_id = storage_mode(&database).unwrap().device_id;
        let operation_id = Uuid::now_v7().to_string();
        let dependent_id = Uuid::now_v7().to_string();
        let payload = json!({ "id": "entry-1", "version": 2 });
        connection
            .execute(
                "INSERT INTO sync_outbox(
                   operation_id,workspace_id,device_id,operation_type,entity_type,entity_id,
                   base_version,payload_json,payload_version,state,attempt_count,created_at,error_json
                 ) VALUES (?1,?2,?3,'timer_continuity_close','time_entry','entry-1',1,?4,2,'pending',28,1,?5)",
                params![
                    &operation_id,
                    &workspace_id,
                    &device_id,
                    payload.to_string(),
                    json!({
                        "message": "CLOUD_REQUEST_FAILED: IDEMPOTENCY_CONFLICT: operation_id was already used for different content"
                    })
                    .to_string()
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO sync_outbox(
                   operation_id,workspace_id,device_id,operation_type,entity_type,entity_id,
                   payload_json,depends_on_operation_id,state,attempt_count,created_at
                 ) VALUES (?1,?2,?3,'timer_rollover_create','time_entry','entry-2','{}',?4,'pending',0,2)",
                params![&dependent_id, &workspace_id, &device_id, &operation_id],
            )
            .unwrap();
        drop(connection);

        assert_eq!(
            mark_historical_timer_recovery_candidates(&database, &workspace_id).unwrap(),
            2
        );

        let connection = database.open().unwrap();
        let repaired: (String, String, i64, Option<i64>, Option<String>, String, Option<String>) = connection
            .query_row(
                "SELECT operation_id,state,attempt_count,last_attempt_at,error_json,payload_json,recovery_kind
                 FROM sync_outbox WHERE entity_id='entry-1'",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(repaired.0, operation_id);
        assert_eq!(repaired.1, "pending");
        assert_eq!(repaired.2, 28);
        assert_eq!(repaired.3, None);
        assert!(repaired
            .4
            .as_deref()
            .is_some_and(|value| value.contains("IDEMPOTENCY_CONFLICT")));
        assert_eq!(repaired.5, payload.to_string());
        assert_eq!(repaired.6.as_deref(), Some("historical_candidate"));
        let dependency: String = connection
            .query_row(
                "SELECT depends_on_operation_id FROM sync_outbox WHERE operation_id=?1",
                [&dependent_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(dependency, operation_id);
    }

    #[test]
    fn recover_interrupted_sending_operations_releases_all_workspace_claims() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("cloud-recover-sending.sqlite3"))
                .unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let operation_id = Uuid::now_v7().to_string();
        let now = now_millis();
        connection
            .execute(
                "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, state, attempt_count, created_at, last_attempt_at)
                 VALUES (?1, ?2, ?3, 'task_update', 'task', ?4, 1, '{}', 'sending', 1, ?5, ?5)",
                params![&operation_id, &workspace_id, &device_id, &operation_id, now],
            )
            .unwrap();
        drop(connection);

        recover_interrupted_sending_operations(&database, &workspace_id).unwrap();
        let state: (String, Option<i64>) = database
            .open()
            .unwrap()
            .query_row(
                "SELECT state, last_attempt_at FROM sync_outbox WHERE operation_id = ?1",
                [&operation_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(state, ("pending".to_string(), None));
    }

    #[test]
    fn outbox_claim_preserves_insert_order_for_same_millisecond_operations() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("cloud-claim-stable-order.sqlite3"))
                .unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let first_id = Uuid::now_v7().to_string();
        let second_id = Uuid::now_v7().to_string();
        let created_at = now_millis();
        for operation_id in [&second_id, &first_id] {
            connection
                .execute(
                    "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, state, attempt_count, created_at)
                     VALUES (?1, ?2, ?3, 'task_update', 'task', ?4, 1, '{}', 'pending', 0, ?5)",
                    params![
                        operation_id,
                        &workspace_id,
                        &device_id,
                        Uuid::now_v7().to_string(),
                        created_at
                    ],
                )
                .unwrap();
        }
        drop(connection);

        let claimed = claim_pending_operations(&database, &workspace_id).unwrap();
        let claimed_ids = claimed
            .into_iter()
            .map(|operation| operation.operation_id)
            .collect::<Vec<_>>();
        assert_eq!(claimed_ids, vec![second_id, first_id]);
    }

    #[test]
    fn outbox_claim_takes_only_the_oldest_ready_operation_per_entity() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(
            directory
                .path()
                .join("cloud-claim-one-ready-operation-per-entity.sqlite3"),
        )
        .unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let task_id = Uuid::now_v7().to_string();
        let other_task_id = Uuid::now_v7().to_string();
        let first_same_entity_id = Uuid::now_v7().to_string();
        let second_same_entity_id = Uuid::now_v7().to_string();
        let other_entity_id = Uuid::now_v7().to_string();
        let now = now_millis();
        for (operation_id, entity_id, created_offset) in [
            (&first_same_entity_id, &task_id, 0_i64),
            (&second_same_entity_id, &task_id, 1_i64),
            (&other_entity_id, &other_task_id, 2_i64),
        ] {
            connection
                .execute(
                    "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, state, attempt_count, created_at)
                     VALUES (?1, ?2, ?3, 'task_update', 'task', ?4, 1, '{}', 'pending', 0, ?5)",
                    params![
                        operation_id,
                        &workspace_id,
                        &device_id,
                        entity_id,
                        now + created_offset,
                    ],
                )
                .unwrap();
        }
        drop(connection);

        let claimed = claim_pending_operations(&database, &workspace_id).unwrap();
        let claimed_ids = claimed
            .into_iter()
            .map(|operation| operation.operation_id)
            .collect::<Vec<_>>();
        assert_eq!(claimed_ids, vec![first_same_entity_id, other_entity_id]);

        let connection = database.open().unwrap();
        let second_state: (String, i64) = connection
            .query_row(
                "SELECT state, attempt_count FROM sync_outbox WHERE operation_id = ?1",
                [&second_same_entity_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(second_state, ("pending".to_string(), 0));
    }

    #[test]
    fn outbox_claim_can_continue_with_the_next_operation_after_success() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(
            directory
                .path()
                .join("cloud-claim-next-operation-after-success.sqlite3"),
        )
        .unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let task_id = Uuid::now_v7().to_string();
        let first_operation_id = Uuid::now_v7().to_string();
        let second_operation_id = Uuid::now_v7().to_string();
        let now = now_millis();
        for (operation_id, created_offset) in
            [(&first_operation_id, 0_i64), (&second_operation_id, 1_i64)]
        {
            connection
                .execute(
                    "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, state, attempt_count, created_at)
                     VALUES (?1, ?2, ?3, 'task_update', 'task', ?4, 1, '{}', 'pending', 0, ?5)",
                    params![
                        operation_id,
                        &workspace_id,
                        &device_id,
                        &task_id,
                        now + created_offset,
                    ],
                )
                .unwrap();
        }
        drop(connection);

        let first_batch =
            claim_pending_operations_with_retry_mode(&database, &workspace_id, true).unwrap();
        assert_eq!(first_batch.len(), 1);
        assert_eq!(first_batch[0].operation_id, first_operation_id);

        delete_outbox(&database, &first_operation_id).unwrap();

        let second_batch =
            claim_pending_operations_with_retry_mode(&database, &workspace_id, true).unwrap();
        assert_eq!(second_batch.len(), 1);
        assert_eq!(second_batch[0].operation_id, second_operation_id);
    }

    #[test]
    fn outbox_claim_respects_retry_backoff_for_recent_failures() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("cloud-backoff.sqlite3")).unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let fresh_failure_id = Uuid::now_v7().to_string();
        let ready_failure_id = Uuid::now_v7().to_string();
        let now = now_millis();
        for (operation_id, attempt_count, last_attempt_at) in [
            (&fresh_failure_id, 3_i64, now),
            (&ready_failure_id, 3_i64, now - 10_000),
        ] {
            connection
                .execute(
                    "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, state, attempt_count, created_at, last_attempt_at)
                     VALUES (?1, ?2, ?3, 'subject_create', 'subject', ?4, NULL, '{}', 'pending', ?5, ?6, ?7)",
                    params![operation_id, &workspace_id, &device_id, Uuid::now_v7().to_string(), attempt_count, now, last_attempt_at],
                )
                .unwrap();
        }
        drop(connection);

        let claimed = claim_pending_operations(&database, &workspace_id).unwrap();
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].operation_id, ready_failure_id);

        let connection = database.open().unwrap();
        let fresh_state: (String, i64) = connection
            .query_row(
                "SELECT state, attempt_count FROM sync_outbox WHERE operation_id = ?1",
                [&fresh_failure_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let ready_state: (String, i64) = connection
            .query_row(
                "SELECT state, attempt_count FROM sync_outbox WHERE operation_id = ?1",
                [&ready_failure_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(fresh_state, ("pending".to_string(), 3));
        assert_eq!(ready_state, ("sending".to_string(), 4));
    }

    #[test]
    fn outbox_claim_does_not_skip_over_older_not_ready_operation_for_same_entity() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("cloud-backoff-entity-order.sqlite3"))
                .unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let task_id = Uuid::now_v7().to_string();
        let other_task_id = Uuid::now_v7().to_string();
        let older_not_ready_id = Uuid::now_v7().to_string();
        let newer_same_entity_id = Uuid::now_v7().to_string();
        let ready_other_entity_id = Uuid::now_v7().to_string();
        let now = now_millis();
        for (operation_id, entity_id, attempt_count, last_attempt_at, created_offset) in [
            (&older_not_ready_id, &task_id, 3_i64, Some(now), 0_i64),
            (&newer_same_entity_id, &task_id, 0_i64, None, 1_i64),
            (&ready_other_entity_id, &other_task_id, 0_i64, None, 2_i64),
        ] {
            connection
                .execute(
                    "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, state, attempt_count, created_at, last_attempt_at)
                     VALUES (?1, ?2, ?3, 'task_update', 'task', ?4, 1, '{}', 'pending', ?5, ?6, ?7)",
                    params![
                        operation_id,
                        &workspace_id,
                        &device_id,
                        entity_id,
                        attempt_count,
                        now + created_offset,
                        last_attempt_at
                    ],
                )
                .unwrap();
        }
        drop(connection);

        let claimed = claim_pending_operations(&database, &workspace_id).unwrap();
        let claimed_ids = claimed
            .into_iter()
            .map(|operation| operation.operation_id)
            .collect::<Vec<_>>();
        assert_eq!(claimed_ids, vec![ready_other_entity_id]);
    }

    #[test]
    fn outbox_claim_does_not_skip_over_active_sending_operation_for_same_entity() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(
            directory
                .path()
                .join("cloud-active-sending-entity-order.sqlite3"),
        )
        .unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let task_id = Uuid::now_v7().to_string();
        let sending_id = Uuid::now_v7().to_string();
        let pending_same_entity_id = Uuid::now_v7().to_string();
        let now = now_millis();
        for (operation_id, state, last_attempt_at, created_offset) in [
            (&sending_id, "sending", Some(now), 0_i64),
            (&pending_same_entity_id, "pending", None, 1_i64),
        ] {
            connection
                .execute(
                    "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, state, attempt_count, created_at, last_attempt_at)
                     VALUES (?1, ?2, ?3, 'task_update', 'task', ?4, 1, '{}', ?5, 0, ?6, ?7)",
                    params![
                        operation_id,
                        &workspace_id,
                        &device_id,
                        &task_id,
                        state,
                        now + created_offset,
                        last_attempt_at
                    ],
                )
                .unwrap();
        }
        drop(connection);

        let claimed = claim_pending_operations(&database, &workspace_id).unwrap();
        assert!(claimed.is_empty());
    }

    #[test]
    fn outbox_claim_does_not_skip_over_conflict_for_same_entity() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("cloud-conflict-entity-order.sqlite3"))
                .unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let task_id = Uuid::now_v7().to_string();
        let conflict_id = Uuid::now_v7().to_string();
        let pending_same_entity_id = Uuid::now_v7().to_string();
        let now = now_millis();
        for (operation_id, state, created_offset) in [
            (&conflict_id, "conflict", 0_i64),
            (&pending_same_entity_id, "pending", 1_i64),
        ] {
            connection
                .execute(
                    "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, state, attempt_count, created_at)
                     VALUES (?1, ?2, ?3, 'task_update', 'task', ?4, 1, '{}', ?5, 0, ?6)",
                    params![
                        operation_id,
                        &workspace_id,
                        &device_id,
                        &task_id,
                        state,
                        now + created_offset,
                    ],
                )
                .unwrap();
        }
        drop(connection);

        let claimed = claim_pending_operations(&database, &workspace_id).unwrap();
        assert!(claimed.is_empty());
    }

    #[test]
    fn outbox_claim_waits_for_dependency_but_claims_unrelated_entities() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(
            directory
                .path()
                .join("cloud-outbox-dependency-order.sqlite3"),
        )
        .unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let time_entry_id = Uuid::now_v7().to_string();
        let parent_id = Uuid::now_v7().to_string();
        let dependent_id = Uuid::now_v7().to_string();
        let unrelated_id = Uuid::now_v7().to_string();
        let dependent_task_id = Uuid::now_v7().to_string();
        let unrelated_task_id = Uuid::now_v7().to_string();
        for (operation_id, entity_type, entity_id, depends_on, created_at) in [
            (&parent_id, "time_entry", &time_entry_id, None, 1_i64),
            (
                &dependent_id,
                "task",
                &dependent_task_id,
                Some(parent_id.as_str()),
                2_i64,
            ),
            (&unrelated_id, "task", &unrelated_task_id, None, 3_i64),
        ] {
            connection
                .execute(
                    "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, depends_on_operation_id, state, attempt_count, created_at)
                     VALUES (?1, ?2, ?3, 'update', ?4, ?5, 1, '{}', ?6, 'pending', 0, ?7)",
                    params![
                        operation_id,
                        &workspace_id,
                        &device_id,
                        entity_type,
                        entity_id,
                        depends_on,
                        created_at
                    ],
                )
                .unwrap();
        }
        drop(connection);

        let first_claim = claim_pending_operations(&database, &workspace_id).unwrap();
        let first_ids = first_claim
            .iter()
            .map(|operation| operation.operation_id.as_str())
            .collect::<Vec<_>>();
        assert!(first_ids.contains(&parent_id.as_str()));
        assert!(first_ids.contains(&unrelated_id.as_str()));
        assert!(!first_ids.contains(&dependent_id.as_str()));

        delete_outbox(&database, &parent_id).unwrap();
        delete_outbox(&database, &unrelated_id).unwrap();
        let second_claim = claim_pending_operations(&database, &workspace_id).unwrap();
        assert_eq!(second_claim.len(), 1);
        assert_eq!(second_claim[0].operation_id, dependent_id);
    }

    #[test]
    fn cloud_entity_version_reads_versioned_snapshot_entities() {
        let snapshot = json!({
            "tasks": [{"id": "task-a", "title": "云端事项", "status": "done", "version": 4}],
            "task_occurrences": [{"task_id": "task-a", "occurrence_date": "2026-09-29", "status": "done", "version": 5}],
            "time_entries": [{"id": "entry-a", "label_snapshot": "云端计时", "state": "ended", "version": 6}],
            "app_settings": [{"key": "salary_hourly_rate", "version": 3}],
            "integration_configs": [{"provider": "seatable", "version": 2}]
        });
        assert_eq!(
            cloud_entity_version(&snapshot, "task", Some("task-a"), &Value::Null),
            Some(4)
        );
        assert_eq!(
            cloud_entity_payload(&snapshot, "task", Some("task-a"), &Value::Null)
                .and_then(|payload| payload.get("title")),
            Some(&json!("云端事项"))
        );
        assert_eq!(
            cloud_entity_version(
                &snapshot,
                "task_occurrence",
                Some("task-a|2026-09-29"),
                &json!({"task_id": "task-a", "occurrence_date": "2026-09-29"})
            ),
            Some(5)
        );
        assert_eq!(
            cloud_entity_payload(
                &snapshot,
                "task_occurrence",
                Some("task-a|2026-09-29"),
                &json!({"task_id": "task-a", "occurrence_date": "2026-09-29"})
            )
            .and_then(|payload| payload.get("status")),
            Some(&json!("done"))
        );
        assert_eq!(
            cloud_entity_payload(&snapshot, "time_entry", Some("entry-a"), &Value::Null)
                .and_then(|payload| payload.get("label_snapshot")),
            Some(&json!("云端计时"))
        );
        assert_eq!(
            cloud_entity_version(
                &snapshot,
                "app_setting",
                None,
                &json!({"key": "salary_hourly_rate"})
            ),
            Some(3)
        );
        assert_eq!(
            cloud_entity_version(
                &snapshot,
                "integration_config",
                None,
                &json!({"provider": "seatable"})
            ),
            Some(2)
        );
        assert_eq!(
            cloud_entity_version(&snapshot, "task", Some("missing"), &Value::Null),
            None
        );
    }

    #[test]
    fn cloud_timestamp_parser_accepts_postgrest_timezone_without_colon() {
        let value = json!({ "started_at": "2026-10-03 09:08:07.123456+00" });
        assert_eq!(optional_millis(&value, "started_at"), Some(1791018487123));
    }

    #[test]
    fn entity_payload_preserves_embedded_json_types() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("cloud-json-payload.sqlite3")).unwrap();

        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Shared,
                key: "salary_hourly_rate".to_string(),
                value: json!(42.5),
            },
        )
        .unwrap();
        settings::update_integration_config(
            &database,
            settings::IntegrationConfigRequest {
                provider: "seatable".to_string(),
                enabled: true,
                config: json!({"serverUrl": "https://example.test", "syncEnabled": true}),
                expected_version: None,
            },
        )
        .unwrap();

        let setting = entity_payload(&database, "app_setting", Some("salary_hourly_rate")).unwrap();
        assert_eq!(setting["value_json"], json!(42.5));
        assert!(setting["value_json"].is_number());

        let integration =
            entity_payload(&database, "integration_config", Some("seatable")).unwrap();
        assert_eq!(
            integration["config_json"],
            json!({"serverUrl": "https://example.test", "syncEnabled": true})
        );
        assert!(integration["config_json"].is_object());
    }

    #[test]
    fn outbox_rejects_entity_types_not_supported_by_cloud_apply_patch() {
        assert!(ensure_supported_cloud_entity_type("task").is_ok());
        assert!(ensure_supported_cloud_entity_type("time_entry").is_ok());
        assert!(ensure_supported_cloud_entity_type("unassigned_session").is_ok());
        let error = ensure_supported_cloud_entity_type("work_day").unwrap_err();
        assert!(error.starts_with("VALIDATION_ERROR:"));
        assert!(error.contains("cloud_apply_patch"));
    }

    #[test]
    fn entity_payload_selects_external_binding_by_composite_identity() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(
            directory
                .path()
                .join("cloud-external-binding-payload.sqlite3"),
        )
        .unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let task_id = Uuid::now_v7().to_string();
        for (provider, external_id) in [("seatable", "sea-row-1"), ("notion", "notion-row-1")] {
            connection
                .execute(
                    "INSERT INTO external_bindings(id, workspace_id, provider, entity_type, entity_id, external_id, last_synced_at)
                     VALUES (?1, ?2, ?3, 'task', ?4, ?5, ?6)",
                    params![
                        Uuid::now_v7().to_string(),
                        &workspace_id,
                        provider,
                        &task_id,
                        external_id,
                        now_millis()
                    ],
                )
                .unwrap();
        }
        drop(connection);

        let payload = entity_payload(
            &database,
            "external_binding",
            Some(&format!("seatable|task|{task_id}")),
        )
        .unwrap();
        assert_eq!(payload["provider"], "seatable");
        assert_eq!(payload["entity_type"], "task");
        assert_eq!(payload["entity_id"], task_id);
        assert_eq!(payload["external_id"], "sea-row-1");
    }

    #[test]
    fn snapshot_import_restores_out_of_order_task_tree() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("cloud-snapshot.sqlite3")).unwrap();
        let connection = database.open().unwrap();
        let subject_id: String = connection
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        drop(connection);
        let parent_id = Uuid::now_v7().to_string();
        let child_id = Uuid::now_v7().to_string();
        let now = now_millis();
        let snapshot = json!({
            "subjects": [{"id": subject_id, "name": "默认", "sort_order": 10, "created_at": now, "updated_at": now, "version": 1, "deleted_at": null}],
            "tasks": [
                {"id": child_id, "subject_id": subject_id, "parent_id": parent_id, "title": "子项", "status": "open", "source_type": "manual", "sort_order": 10, "created_at": now, "updated_at": now, "version": 1},
                {"id": parent_id, "subject_id": subject_id, "parent_id": null, "title": "父项", "status": "open", "source_type": "manual", "sort_order": 10, "created_at": now, "updated_at": now, "version": 1}
            ]
        });
        apply_snapshot(&database, &snapshot, None, None, None).unwrap();
        apply_snapshot(&database, &snapshot, None, None, None).unwrap();
        let restored_parent: String = database
            .open()
            .unwrap()
            .query_row(
                "SELECT parent_id FROM tasks WHERE id = ?1",
                [&child_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(restored_parent, parent_id);
    }

    #[test]
    fn snapshot_import_restores_archived_day_instance_tree() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(
            directory
                .path()
                .join("cloud-snapshot-archived-day-instance.sqlite3"),
        )
        .unwrap();
        let connection = database.open().unwrap();
        let subject_id: String = connection
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        drop(connection);

        let parent_id = Uuid::now_v7().to_string();
        let child_id = Uuid::now_v7().to_string();
        let now = now_millis();
        let snapshot = json!({
            "subjects": [{
                "id": subject_id, "name": "默认", "sort_order": 10,
                "created_at": now, "updated_at": now, "version": 1, "deleted_at": null
            }],
            "tasks": [
                {
                    "id": child_id, "subject_id": subject_id, "parent_id": parent_id,
                    "title": "已归档子项", "status": "open", "source_type": "manual",
                    "sort_order": 20, "created_at": now, "updated_at": now, "version": 2,
                    "deleted_at": now, "day_instance_date": "2026-10-21",
                    "day_instance_kind": "member"
                },
                {
                    "id": parent_id, "subject_id": subject_id, "parent_id": null,
                    "title": "已归档父项", "status": "open", "source_type": "manual",
                    "sort_order": 10, "created_at": now, "updated_at": now, "version": 2,
                    "deleted_at": now, "day_instance_date": "2026-10-21",
                    "day_instance_kind": "context"
                }
            ]
        });

        apply_snapshot(&database, &snapshot, None, None, None).unwrap();

        let restored_parent: String = database
            .open()
            .unwrap()
            .query_row(
                "SELECT parent_id FROM tasks WHERE id = ?1",
                [&child_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(restored_parent, parent_id);
    }

    #[test]
    fn snapshot_import_advances_checkpoint_in_the_same_transaction() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("cloud-snapshot-checkpoint.sqlite3"))
                .unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let subject_id: String = connection
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        drop(connection);

        let now = now_millis();
        let snapshot = json!({
            "subjects": [{"id": subject_id, "name": "云端主体", "sort_order": 10, "created_at": now, "updated_at": now, "version": 7, "deleted_at": null}],
            "actual_time_adjustments": []
        });
        apply_snapshot(
            &database,
            &snapshot,
            None,
            Some(&workspace_id),
            Some((&workspace_id, &device_id, 42, 1)),
        )
        .unwrap();

        let connection = database.open().unwrap();
        let saved: (String, i64, i64) = connection
            .query_row(
                "SELECT s.name, s.version, l.last_change_seq
                 FROM subjects s CROSS JOIN local_sync_state l
                 WHERE s.id = ?1 AND l.workspace_id = ?2",
                params![subject_id, workspace_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(saved, ("云端主体".to_string(), 7, 42));
    }

    #[test]
    fn snapshot_import_restores_daily_estimates_and_recurring_data() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("cloud-snapshot-recurring.sqlite3"))
                .unwrap();
        let connection = database.open().unwrap();
        let subject_id: String = connection
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        drop(connection);

        let task_id = Uuid::now_v7().to_string();
        let rule_id = Uuid::now_v7().to_string();
        let now = now_millis();
        let snapshot = json!({
            "subjects": [{"id": subject_id, "name": "默认", "sort_order": 10, "created_at": now, "updated_at": now, "version": 1, "deleted_at": null}],
            "tasks": [{"id": task_id, "subject_id": subject_id, "parent_id": null, "title": "每日复盘", "status": "open", "source_type": "manual", "sort_order": 10, "created_at": now, "updated_at": now, "version": 1}],
            "task_daily_estimates": [{"task_id": task_id, "work_date": "2026-09-29", "estimate_minutes": 40, "created_at": now, "updated_at": now, "version": 2}],
            "task_day_plans": [
                {"task_id": task_id, "work_date": "2026-09-29", "state": "included", "created_at": now, "updated_at": now, "version": 5},
                {"task_id": task_id, "work_date": "2026-09-30", "state": "excluded", "created_at": now, "updated_at": now, "version": 3}
            ],
            "task_recurrence_rules": [{"id": rule_id, "task_id": task_id, "frequency": "daily", "weekdays_mask": null, "effective_start": "2026-09-29", "effective_end": null, "created_at": now, "updated_at": now, "version": 3}],
            "task_occurrences": [{"task_id": task_id, "occurrence_date": "2026-09-29", "origin": "scheduled", "status": "open", "completed_at": null, "created_at": now, "updated_at": now, "version": 4}]
        });

        apply_snapshot(&database, &snapshot, None, None, None).unwrap();

        let connection = database.open().unwrap();
        let daily_estimate: i64 = connection
            .query_row(
                "SELECT estimate_minutes FROM task_daily_estimates WHERE task_id = ?1 AND work_date = '2026-09-29'",
                [&task_id],
                |row| row.get(0),
            )
            .unwrap();
        let frequency: String = connection
            .query_row(
                "SELECT frequency FROM task_recurrence_rules WHERE id = ?1",
                [&rule_id],
                |row| row.get(0),
            )
            .unwrap();
        let occurrence_status: String = connection
            .query_row(
                "SELECT status FROM task_occurrences WHERE task_id = ?1 AND occurrence_date = '2026-09-29'",
                [&task_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(daily_estimate, 40);
        assert_eq!(frequency, "daily");
        assert_eq!(occurrence_status, "open");
        let plans: Vec<(String, String, i64)> = connection
            .prepare("SELECT work_date,state,version FROM task_day_plans WHERE task_id=?1 ORDER BY work_date")
            .unwrap()
            .query_map([&task_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            plans,
            vec![
                ("2026-09-29".to_string(), "included".to_string(), 5),
                ("2026-09-30".to_string(), "excluded".to_string(), 3)
            ]
        );
        let counts: (i64, i64, i64, i64) = connection
            .query_row(
                "SELECT
                   (SELECT COUNT(*) FROM task_daily_estimates WHERE task_id = ?1),
                   (SELECT COUNT(*) FROM task_day_plans WHERE task_id = ?1),
                   (SELECT COUNT(*) FROM task_recurrence_rules WHERE task_id = ?1),
                   (SELECT COUNT(*) FROM task_occurrences WHERE task_id = ?1)",
                [&task_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(counts, (1, 2, 1, 1));
    }

    #[test]
    fn cloud_snapshot_without_actual_adjustments_does_not_clear_local_rows() {
        let (_directory, database, cloud_workspace_id, local_workspace_id, subject_id) =
            cloud_database_for_incremental_test("cloud-snapshot-missing-adjustments.sqlite3");
        let task_id = Uuid::now_v7().to_string();
        let adjustment_id = Uuid::now_v7().to_string();
        let batch_id = Uuid::now_v7().to_string();
        let connection = database.open().unwrap();
        connection
            .execute(
                "INSERT INTO tasks(id,workspace_id,subject_id,title,status,source_type,sort_order,created_at,updated_at,version) VALUES(?1,?2,?3,'今日用时保护','open','manual',10,1,1,1)",
                params![task_id, local_workspace_id, subject_id],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO actual_time_adjustments(id,workspace_id,subject_id,task_id,work_date,delta_minutes,baseline_minutes,target_minutes,batch_id,created_at,updated_at,version) VALUES(?1,?2,?3,?4,'2026-10-20',15,0,15,?5,1,1,1)",
                params![adjustment_id, local_workspace_id, subject_id, task_id, batch_id],
            )
            .unwrap();
        drop(connection);

        let error = apply_snapshot(
            &database,
            &json!({"subjects": []}),
            None,
            Some(&cloud_workspace_id),
            None,
        )
        .unwrap_err();

        assert!(error.starts_with("CLOUD_SCHEMA_OUTDATED:"));
        let saved: i64 = database
            .open()
            .unwrap()
            .query_row(
                "SELECT target_minutes FROM actual_time_adjustments WHERE id=?1 AND deleted_at IS NULL",
                [&adjustment_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(saved, 15);
    }

    #[test]
    fn snapshot_import_restores_actual_time_adjustments_with_unknown_device_ids() {
        let (_directory, database, cloud_workspace_id, _local_workspace_id, subject_id) =
            cloud_database_for_incremental_test("cloud-snapshot-restores-adjustments.sqlite3");
        let task_id = Uuid::now_v7().to_string();
        let adjustment_id = Uuid::now_v7().to_string();
        let batch_id = Uuid::now_v7().to_string();
        let missing_device_id = Uuid::now_v7().to_string();
        let now = now_millis();
        let snapshot = json!({
            "subjects": [{"id": subject_id, "name": "默认", "sort_order": 10, "created_at": now, "updated_at": now, "version": 1, "deleted_at": null}],
            "tasks": [{"id": task_id, "subject_id": subject_id, "parent_id": null, "title": "今日用时", "status": "open", "source_type": "manual", "sort_order": 10, "created_at": now, "updated_at": now, "version": 1}],
            "actual_time_adjustments": [{
                "id": adjustment_id, "subject_id": subject_id, "task_id": task_id,
                "work_date": "2026-10-20", "delta_minutes": 15, "baseline_minutes": 0,
                "target_minutes": 15, "note": null, "source_type": "quick_actual_adjustment",
                "batch_id": batch_id, "created_at": now, "updated_at": now, "version": 1,
                "created_by_device_id": missing_device_id, "updated_by_device_id": missing_device_id, "deleted_at": null
            }]
        });

        apply_snapshot(&database, &snapshot, None, Some(&cloud_workspace_id), None).unwrap();

        let saved: (i64, Option<String>, Option<String>) = database
            .open()
            .unwrap()
            .query_row(
                "SELECT target_minutes,created_by_device_id,updated_by_device_id FROM actual_time_adjustments WHERE id=?1 AND deleted_at IS NULL",
                [&adjustment_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(saved, (15, None, None));
    }

    #[test]
    fn incremental_actual_adjustment_ignores_unknown_device_ids() {
        let (_directory, database, _cloud_workspace_id, local_workspace_id, subject_id) =
            cloud_database_for_incremental_test("cloud-incremental-adjustment-device.sqlite3");
        let task_id = Uuid::now_v7().to_string();
        let adjustment_id = Uuid::now_v7().to_string();
        let missing_device_id = Uuid::now_v7().to_string();
        let now = now_millis();
        let mut connection = database.open().unwrap();
        connection
            .execute(
                "INSERT INTO tasks(id,workspace_id,subject_id,title,status,source_type,sort_order,created_at,updated_at,version)
                 VALUES(?1,?2,?3,'增量实际用时','open','manual',10,?4,?4,1)",
                params![task_id, local_workspace_id, subject_id, now],
            )
            .unwrap();
        let transaction = connection.transaction().unwrap();
        apply_incremental_entity(
            &transaction,
            &local_workspace_id,
            &IncrementalEntity {
                entity_type: "actual_time_adjustments".to_string(),
                source_entity_type: "actual_time_adjustments".to_string(),
                entity_id: adjustment_id.clone(),
                change_seq: 1,
                deleted: false,
                data: Some(json!({
                    "id": adjustment_id,
                    "subject_id": subject_id,
                    "task_id": task_id,
                    "work_date": "2026-10-20",
                    "delta_minutes": 15,
                    "baseline_minutes": 0,
                    "target_minutes": 15,
                    "note": null,
                    "batch_id": Uuid::now_v7().to_string(),
                    "created_at": now,
                    "updated_at": now,
                    "version": 1,
                    "created_by_device_id": missing_device_id,
                    "updated_by_device_id": missing_device_id,
                    "deleted_at": null
                })),
            },
        )
        .unwrap();
        transaction.commit().unwrap();

        let saved: (i64, Option<String>, Option<String>) = connection
            .query_row(
                "SELECT target_minutes,created_by_device_id,updated_by_device_id FROM actual_time_adjustments WHERE id=?1",
                [&adjustment_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(saved, (15, None, None));
    }

    #[test]
    fn snapshot_import_preserves_local_open_timer_segment() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(
            directory
                .path()
                .join("cloud-snapshot-open-timer-segment.sqlite3"),
        )
        .unwrap();
        let connection = database.open().unwrap();
        let subject_id: String = connection
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        drop(connection);
        let task = create_task(
            &database,
            TaskCreateRequest {
                subject_id: subject_id.clone(),
                day_instance_date: None,
                parent_id: None,
                title: "同步中计时".to_string(),
                planned_date: None,
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        let started = time_tracking::start_timer(
            &database,
            TimerStartRequest {
                task_id: Some(task.id.clone()),
                note: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        database
            .open()
            .unwrap()
            .execute(
                "UPDATE time_segments SET started_at = started_at - 4000 WHERE entry_id = ?1 AND ended_at IS NULL",
                [&started.id],
            )
            .unwrap();

        let now = now_millis();
        let snapshot = json!({
            "subjects": [{"id": subject_id.clone(), "name": "默认", "sort_order": 10, "created_at": now, "updated_at": now, "version": 1, "deleted_at": null}],
            "tasks": [{"id": task.id.clone(), "subject_id": subject_id.clone(), "parent_id": null, "title": task.title.clone(), "status": "open", "source_type": "manual", "sort_order": 10, "created_at": now, "updated_at": now, "version": task.version}],
            "time_entries": [{"id": started.id.clone(), "work_date": started.work_date.clone(), "kind": "work", "source_type": "timer", "state": "running", "default_task_id": task.id.clone(), "label_snapshot": started.label.clone(), "started_at": started.started_at, "ended_at": null, "duration_seconds": 0, "note": null, "origin_unassigned_session_id": null, "created_at": now, "updated_at": now, "version": started.version, "deleted_at": null}],
            "time_segments": []
        });
        apply_snapshot(&database, &snapshot, None, None, None).unwrap();

        let restored = time_tracking::get_timer_state(&database).unwrap().unwrap();
        assert_eq!(restored.id, started.id);
        assert!(restored.duration_seconds >= 3);
    }

    #[test]
    fn snapshot_import_preserves_local_running_timer_missing_from_cloud() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(
            directory
                .path()
                .join("cloud-snapshot-missing-running-timer.sqlite3"),
        )
        .unwrap();
        let connection = database.open().unwrap();
        let subject_id: String = connection
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        drop(connection);
        let task = create_task(
            &database,
            TaskCreateRequest {
                subject_id: subject_id.clone(),
                day_instance_date: None,
                parent_id: None,
                title: "本机未推送计时".to_string(),
                planned_date: None,
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        let started = time_tracking::start_timer(
            &database,
            TimerStartRequest {
                task_id: Some(task.id.clone()),
                note: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        database
            .open()
            .unwrap()
            .execute(
                "UPDATE time_segments SET started_at = started_at - 4000 WHERE entry_id = ?1 AND ended_at IS NULL",
                [&started.id],
            )
            .unwrap();

        let now = now_millis();
        let snapshot = json!({
            "subjects": [{"id": subject_id.clone(), "name": "默认", "sort_order": 10, "created_at": now, "updated_at": now, "version": 1, "deleted_at": null}],
            "tasks": [{"id": task.id.clone(), "subject_id": subject_id.clone(), "parent_id": null, "title": task.title.clone(), "status": "open", "source_type": "manual", "sort_order": 10, "created_at": now, "updated_at": now, "version": task.version}],
            "time_entries": [],
            "time_segments": [],
            "actual_time_adjustments": []
        });
        apply_snapshot(&database, &snapshot, None, None, None).unwrap();

        let restored = time_tracking::get_timer_state(&database).unwrap().unwrap();
        assert_eq!(restored.id, started.id);
        assert_eq!(restored.state, "running");
        assert!(restored.duration_seconds >= 3);
    }

    #[test]
    fn reset_snapshot_import_does_not_preserve_local_running_timer_missing_from_cloud() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(
            directory
                .path()
                .join("cloud-reset-missing-running-timer.sqlite3"),
        )
        .unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let subject_id: String = connection
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        drop(connection);
        let task = create_task(
            &database,
            TaskCreateRequest {
                subject_id: subject_id.clone(),
                day_instance_date: None,
                parent_id: None,
                title: "本机缓存损坏计时".to_string(),
                planned_date: None,
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        let started = time_tracking::start_timer(
            &database,
            TimerStartRequest {
                task_id: Some(task.id.clone()),
                note: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();

        let now = now_millis();
        let snapshot = json!({
            "subjects": [{"id": subject_id.clone(), "name": "默认", "sort_order": 10, "created_at": now, "updated_at": now, "version": 1, "deleted_at": null}],
            "tasks": [{"id": task.id.clone(), "subject_id": subject_id.clone(), "parent_id": null, "title": task.title.clone(), "status": "open", "source_type": "manual", "sort_order": 10, "created_at": now, "updated_at": now, "version": task.version}],
            "time_entries": [],
            "time_segments": [],
            "actual_time_adjustments": []
        });
        apply_snapshot_with_options(
            &database,
            &snapshot,
            None,
            Some(&workspace_id),
            Some((&workspace_id, &device_id, 7, 1)),
            false,
        )
        .unwrap();

        assert!(time_tracking::get_timer_state(&database).unwrap().is_none());
        let entry_count: i64 = database
            .open()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM time_entries WHERE id = ?1",
                [&started.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(entry_count, 0);
        let last_change_seq: i64 = database
            .open()
            .unwrap()
            .query_row(
                "SELECT last_change_seq FROM local_sync_state WHERE workspace_id = ?1",
                [&workspace_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(last_change_seq, 7);
    }

    #[test]
    fn snapshot_import_keeps_local_running_timer_over_stale_cloud_end_state() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(
            directory
                .path()
                .join("cloud-snapshot-stale-ended-running-timer.sqlite3"),
        )
        .unwrap();
        let connection = database.open().unwrap();
        let subject_id: String = connection
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        drop(connection);
        let task = create_task(
            &database,
            TaskCreateRequest {
                subject_id: subject_id.clone(),
                day_instance_date: None,
                parent_id: None,
                title: "旧云端结束状态".to_string(),
                planned_date: None,
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        let started = time_tracking::start_timer(
            &database,
            TimerStartRequest {
                task_id: Some(task.id.clone()),
                note: Some("本机仍在计时".to_string()),
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        database
            .open()
            .unwrap()
            .execute(
                "UPDATE time_segments SET started_at = started_at - 4000 WHERE entry_id = ?1 AND ended_at IS NULL",
                [&started.id],
            )
            .unwrap();

        let now = now_millis();
        let snapshot = json!({
            "subjects": [{"id": subject_id.clone(), "name": "默认", "sort_order": 10, "created_at": now, "updated_at": now, "version": 1, "deleted_at": null}],
            "tasks": [{"id": task.id.clone(), "subject_id": subject_id.clone(), "parent_id": null, "title": task.title.clone(), "status": "open", "source_type": "manual", "sort_order": 10, "created_at": now, "updated_at": now, "version": task.version}],
            "time_entries": [{"id": started.id.clone(), "work_date": started.work_date.clone(), "kind": "work", "source_type": "timer", "state": "ended", "default_task_id": task.id.clone(), "label_snapshot": started.label.clone(), "started_at": started.started_at, "ended_at": now, "duration_seconds": 2, "note": "旧状态", "origin_unassigned_session_id": null, "created_at": now, "updated_at": now, "version": started.version, "deleted_at": null}],
            "time_segments": [{"id": Uuid::now_v7().to_string(), "entry_id": started.id.clone(), "sequence_no": 1, "started_at": started.started_at, "ended_at": now, "duration_seconds": 2}]
        });
        apply_snapshot(&database, &snapshot, None, None, None).unwrap();

        let restored = time_tracking::get_timer_state(&database).unwrap().unwrap();
        assert_eq!(restored.id, started.id);
        assert_eq!(restored.state, "running");
        assert_eq!(restored.note.as_deref(), Some("本机仍在计时"));
        assert!(restored.duration_seconds >= 3);
    }

    #[test]
    fn snapshot_import_preserves_local_open_unassigned_segment() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(
            directory
                .path()
                .join("cloud-snapshot-open-unassigned-segment.sqlite3"),
        )
        .unwrap();
        let state = crate::unassigned::get_state(&database).unwrap().unwrap();
        database
            .open()
            .unwrap()
            .execute(
                "UPDATE unassigned_segments SET started_at = started_at - 4000 WHERE session_id = ?1 AND ended_at IS NULL",
                [&state.session_id],
            )
            .unwrap();

        let now = now_millis();
        let snapshot = json!({
            "unassigned_sessions": [{"id": state.session_id, "work_date": state.work_date, "state": "collecting", "threshold_seconds": 300, "duration_seconds": 0, "first_started_at": state.first_started_at, "last_ended_at": null, "prompted_at": null, "resolution_type": null, "generated_entry_id": null, "resolved_at": null, "created_at": now, "updated_at": now, "version": state.version}],
            "unassigned_segments": []
        });
        apply_snapshot(&database, &snapshot, None, None, None).unwrap();

        let restored = crate::unassigned::get_state(&database).unwrap().unwrap();
        assert_eq!(restored.session_id, state.session_id);
        assert!(restored.elapsed_seconds >= 3);
    }

    #[test]
    fn snapshot_import_preserves_local_unassigned_session_missing_from_cloud() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(
            directory
                .path()
                .join("cloud-snapshot-missing-unassigned-session.sqlite3"),
        )
        .unwrap();
        let state = crate::unassigned::get_state(&database).unwrap().unwrap();
        database
            .open()
            .unwrap()
            .execute(
                "UPDATE unassigned_segments SET started_at = started_at - 4000 WHERE session_id = ?1 AND ended_at IS NULL",
                [&state.session_id],
            )
            .unwrap();

        let snapshot = json!({
            "unassigned_sessions": [],
            "unassigned_segments": []
        });
        apply_snapshot(&database, &snapshot, None, None, None).unwrap();

        let restored = crate::unassigned::get_state(&database).unwrap().unwrap();
        assert_eq!(restored.session_id, state.session_id);
        assert!(restored.elapsed_seconds >= 3);
        let restored_state: String = database
            .open()
            .unwrap()
            .query_row(
                "SELECT state FROM unassigned_sessions WHERE id = ?1",
                [&state.session_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(restored_state, "collecting");
    }

    #[test]
    fn cloud_snapshot_terminal_unassigned_restarts_from_server_boundary() {
        let (_directory, database, cloud_workspace_id, _local_workspace_id, _subject_id) =
            cloud_database_for_incremental_test("cloud-snapshot-terminal-unassigned.sqlite3");
        let device_id = storage_mode(&database).unwrap().device_id;
        let local = crate::unassigned::get_state(&database).unwrap().unwrap();
        let remote_id = Uuid::now_v7().to_string();
        let resolved_at = now_millis() - 5_000;
        let snapshot = json!({
            "unassigned_sessions": [{
                "id":remote_id,"work_date":"2026-10-06","state":"resolved",
                "threshold_seconds":300,"duration_seconds":60,"first_started_at":resolved_at-60_000,
                "last_ended_at":resolved_at,"prompted_at":null,"resolution_type":"work",
                "generated_entry_id":null,"resolved_at":resolved_at,"created_at":resolved_at-60_000,
                "updated_at":resolved_at,"version":2
            }],
            "unassigned_segments": [],
            "actual_time_adjustments": []
        });
        apply_snapshot(
            &database,
            &snapshot,
            None,
            Some(&cloud_workspace_id),
            Some((&cloud_workspace_id, &device_id, 31, 1)),
        )
        .unwrap();

        let connection = database.open().unwrap();
        let local_exists: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM unassigned_sessions WHERE id=?1",
                [&local.session_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(local_exists, 0);
        let next: (String, i64, String) = connection
            .query_row(
                "SELECT id,first_started_at,predecessor_session_id FROM unassigned_sessions
                 WHERE state IN ('collecting','awaiting_resolution')",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(next.1, resolved_at);
        assert_eq!(next.2, remote_id);
        let next_outbox: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sync_outbox WHERE operation_type='unassigned_session_create' AND entity_id=?1",
                [&next.0],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(next_outbox, 1);
        assert_eq!(storage_mode(&database).unwrap().last_change_seq, 31);
    }

    #[test]
    fn snapshot_import_keeps_local_unassigned_session_over_stale_cloud_resolution() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(
            directory
                .path()
                .join("cloud-snapshot-stale-unassigned-resolution.sqlite3"),
        )
        .unwrap();
        let state = crate::unassigned::get_state(&database).unwrap().unwrap();
        database
            .open()
            .unwrap()
            .execute(
                "UPDATE unassigned_segments SET started_at = started_at - 4000 WHERE session_id = ?1 AND ended_at IS NULL",
                [&state.session_id],
            )
            .unwrap();

        let now = now_millis();
        let snapshot = json!({
            "unassigned_sessions": [{
                "id": state.session_id,
                "work_date": "2026-10-04",
                "state": "resolved",
                "threshold_seconds": 300,
                "duration_seconds": 1,
                "first_started_at": state.first_started_at,
                "last_ended_at": now,
                "prompted_at": null,
                "resolution_type": "work",
                "generated_entry_id": null,
                "resolved_at": now,
                "created_at": now,
                "updated_at": now,
                "version": state.version
            }],
            "unassigned_segments": []
        });
        apply_snapshot(&database, &snapshot, None, None, None).unwrap();

        let restored = crate::unassigned::get_state(&database).unwrap().unwrap();
        assert_eq!(restored.session_id, state.session_id);
        assert_eq!(restored.state, "collecting");
        assert!(restored.elapsed_seconds >= 3);
    }

    #[test]
    fn snapshot_import_keeps_local_unassigned_when_cloud_has_another_active_session() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(
            directory
                .path()
                .join("cloud-snapshot-other-active-unassigned-session.sqlite3"),
        )
        .unwrap();
        let state = crate::unassigned::get_state(&database).unwrap().unwrap();
        database
            .open()
            .unwrap()
            .execute(
                "UPDATE unassigned_segments SET started_at = started_at - 4000 WHERE session_id = ?1 AND ended_at IS NULL",
                [&state.session_id],
            )
            .unwrap();

        let now = now_millis();
        let cloud_session_id = Uuid::now_v7().to_string();
        let cloud_segment_id = Uuid::now_v7().to_string();
        let snapshot = json!({
            "unassigned_sessions": [{
                "id": cloud_session_id,
                "work_date": "2026-10-04",
                "state": "collecting",
                "threshold_seconds": 300,
                "duration_seconds": 0,
                "first_started_at": now,
                "last_ended_at": null,
                "prompted_at": null,
                "resolution_type": null,
                "generated_entry_id": null,
                "resolved_at": null,
                "created_at": now,
                "updated_at": now,
                "version": 1
            }],
            "unassigned_segments": [{
                "id": cloud_segment_id,
                "session_id": cloud_session_id,
                "sequence_no": 1,
                "started_at": now,
                "ended_at": null,
                "duration_seconds": 0,
                "lease_token": null
            }]
        });
        apply_snapshot(&database, &snapshot, None, None, None).unwrap();

        let restored = crate::unassigned::get_state(&database).unwrap().unwrap();
        assert_eq!(restored.session_id, state.session_id);
        assert!(restored.elapsed_seconds >= 3);
        let connection = database.open().unwrap();
        let active_sessions: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM unassigned_sessions WHERE state IN ('collecting', 'awaiting_resolution')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(active_sessions, 1);
        let cloud_session_exists: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM unassigned_sessions WHERE id = ?1",
                [&cloud_session_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(cloud_session_exists, 0);
    }

    #[test]
    fn snapshot_import_does_not_overwrite_a_new_local_edit() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("cloud-snapshot-race.sqlite3")).unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let subject_id: String = connection
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let revision = local_revision(&database).unwrap();
        connection
            .execute(
                "UPDATE subjects SET name = '本地新名称', version = version + 1 WHERE id = ?1",
                [&subject_id],
            )
            .unwrap();
        connection.execute(
            "UPDATE app_metadata SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT) WHERE key = 'global_revision'",
            [],
        ).unwrap();
        drop(connection);
        let now = now_millis();
        let snapshot = json!({
            "subjects": [{"id": subject_id, "name": "云端旧名称", "sort_order": 10, "created_at": now, "updated_at": now, "version": 1, "deleted_at": null}]
        });

        let error = apply_snapshot(
            &database,
            &snapshot,
            Some(revision),
            Some(&workspace_id),
            None,
        )
        .unwrap_err();
        assert!(error.starts_with("SYNC_RETRY:"));
        let saved_name: String = database
            .open()
            .unwrap()
            .query_row(
                "SELECT name FROM subjects WHERE id = ?1",
                [&subject_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(saved_name, "本地新名称");
    }

    #[test]
    fn snapshot_import_is_blocked_by_all_dirty_outbox_states() {
        for dirty_state in ["pending", "failed", "sending", "conflict"] {
            let directory = tempdir().unwrap();
            let database = Database::initialize_at(
                directory
                    .path()
                    .join(format!("cloud-snapshot-dirty-{dirty_state}.sqlite3")),
            )
            .unwrap();
            let connection = database.open().unwrap();
            let workspace_id: String = connection
                .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
                .unwrap();
            let subject_id: String = connection
                .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
                .unwrap();
            let device_id: String = connection
                .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
                .unwrap();
            let revision = local_revision(&database).unwrap();
            connection
                .execute(
                    "UPDATE subjects SET name = '本地新名称', version = version + 1 WHERE id = ?1",
                    [&subject_id],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, state, attempt_count, created_at, error_json)
                     VALUES (?1, ?2, ?3, 'subject_update', 'subject', ?4, 1, '{}', ?5, 1, ?6, '{}')",
                    params![
                        Uuid::now_v7().to_string(),
                        workspace_id,
                        device_id,
                        subject_id,
                        dirty_state,
                        now_millis()
                    ],
                )
                .unwrap();
            drop(connection);

            let now = now_millis();
            let snapshot = json!({
                "subjects": [{"id": subject_id, "name": "云端旧名称", "sort_order": 10, "created_at": now, "updated_at": now, "version": 1, "deleted_at": null}]
            });
            let error = apply_snapshot(
                &database,
                &snapshot,
                Some(revision),
                Some(&workspace_id),
                None,
            )
            .unwrap_err();
            assert!(
                error.starts_with("SYNC_PENDING:"),
                "dirty outbox state {dirty_state} should block snapshot import, got {error}"
            );
            let saved_name: String = database
                .open()
                .unwrap()
                .query_row(
                    "SELECT name FROM subjects WHERE id = ?1",
                    [&subject_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(saved_name, "本地新名称");
        }
    }

    #[test]
    fn snapshot_import_removes_stale_integration_configs() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(
            directory
                .path()
                .join("cloud-snapshot-integration-configs.sqlite3"),
        )
        .unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let now = now_millis();
        connection
            .execute(
                "INSERT INTO integration_configs(workspace_id,provider,enabled,config_json,secret_ref,updated_at,version)
                 VALUES (?1,'seatable',1,?2,NULL,?3,1)",
                params![workspace_id, json!({ "syncEnabled": true }).to_string(), now],
            )
            .unwrap();
        drop(connection);

        apply_snapshot(&database, &json!({}), None, None, None).unwrap();

        let count: i64 = database
            .open()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM integration_configs", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn snapshot_import_preserves_matching_integration_secret_ref() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(
            directory
                .path()
                .join("cloud-snapshot-integration-secret-ref.sqlite3"),
        )
        .unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let now = now_millis();
        connection
            .execute(
                "INSERT INTO integration_configs(workspace_id,provider,enabled,config_json,secret_ref,updated_at,version)
                 VALUES (?1,'seatable',1,?2,'keyring://seatable',?3,1)",
                params![workspace_id, json!({ "syncEnabled": true }).to_string(), now],
            )
            .unwrap();
        drop(connection);

        let snapshot = json!({
            "integration_configs": [{
                "provider": "seatable",
                "enabled": false,
                "config_json": { "syncEnabled": false, "serverUrl": "https://cloud.example" },
                "updated_at": now + 1_000,
                "version": 2
            }]
        });
        apply_snapshot(&database, &snapshot, None, None, None).unwrap();

        let saved: (i64, String, String, i64) = database
            .open()
            .unwrap()
            .query_row(
                "SELECT enabled, config_json, secret_ref, version FROM integration_configs WHERE provider = 'seatable'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(saved.0, 0);
        assert_eq!(
            serde_json::from_str::<Value>(&saved.1).unwrap(),
            json!({ "syncEnabled": false, "serverUrl": "https://cloud.example" })
        );
        assert_eq!(saved.2, "keyring://seatable");
        assert_eq!(saved.3, 2);
    }

    #[test]
    fn snapshot_import_preserves_device_hooks() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("cloud-hooks-local.sqlite3")).unwrap();
        let connection = database.open().unwrap();
        let hook_id = Uuid::now_v7().to_string();
        let now = now_millis();
        connection
            .execute(
                "INSERT INTO device_hooks(id,name,event_type,action_type,action_config_json,timeout_seconds,enabled,sort_order,created_at,updated_at)
                 VALUES (?1,'local only','timer.started','uri','{\"uriTemplate\":\"test://start\"}',10,1,10,?2,?2)",
                params![hook_id, now],
            )
            .unwrap();
        drop(connection);
        let snapshot = json!({});

        apply_snapshot(&database, &snapshot, None, None, None).unwrap();

        let connection = database.open().unwrap();
        let name: String = connection
            .query_row(
                "SELECT name FROM device_hooks WHERE id = ?1",
                [&hook_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(name, "local only");
    }

    fn timer_recovery_fixture(
        file_name: &str,
    ) -> (tempfile::TempDir, Database, String, String, String, String) {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(directory.path().join(file_name)).unwrap();
        let cloud_workspace_id = Uuid::now_v7().to_string();
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "cloud_workspace_id".to_string(),
                value: json!(cloud_workspace_id.clone()),
            },
        )
        .unwrap();
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "storage_mode".to_string(),
                value: json!("cloud"),
            },
        )
        .unwrap();
        let connection = database.open().unwrap();
        let local_workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let subject_id: String = connection
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let device_id = storage_mode(&database).unwrap().device_id;
        let task_id = Uuid::now_v7().to_string();
        let entry_id = Uuid::now_v7().to_string();
        connection.execute(
            "INSERT INTO tasks(id,workspace_id,subject_id,title,status,source_type,sort_order,created_at,updated_at,version)
             VALUES (?1,?2,?3,'恢复事项','open','manual',10,1000,1000,1)",
            params![task_id, local_workspace_id, subject_id],
        ).unwrap();
        connection.execute(
            "INSERT INTO time_entries(id,workspace_id,work_date,kind,source_type,state,default_task_id,label_snapshot,started_at,ended_at,duration_seconds,note,created_at,updated_at,version,created_by_device_id,updated_by_device_id,timer_chain_id,last_continuous_at)
             VALUES (?1,?2,'2026-10-14','work','timer','ended',?3,'恢复事项',1000,361000,360,'最终本地状态',1000,361000,8,?4,?4,?1,361000)",
            params![entry_id, local_workspace_id, task_id, device_id],
        ).unwrap();
        for sequence in 1..=6_i64 {
            let started_at = 1000 + (sequence - 1) * 60_000;
            connection.execute(
                "INSERT INTO time_segments(id,workspace_id,entry_id,sequence_no,started_at,ended_at,duration_seconds)
                 VALUES (?1,?2,?3,?4,?5,?6,60)",
                params![Uuid::now_v7().to_string(), local_workspace_id, entry_id, sequence, started_at, started_at + 60_000],
            ).unwrap();
        }
        connection.execute(
            "INSERT INTO time_allocations(id,workspace_id,entry_id,task_id,minutes,note,created_at,updated_at,version)
             VALUES (?1,?2,?3,?4,6,'最终分配',361000,361000,1)",
            params![Uuid::now_v7().to_string(), local_workspace_id, entry_id, task_id],
        ).unwrap();
        drop(connection);
        (
            directory,
            database,
            cloud_workspace_id,
            local_workspace_id,
            device_id,
            entry_id,
        )
    }

    fn insert_timer_version_chain(
        database: &Database,
        cloud_workspace_id: &str,
        device_id: &str,
        entry_id: &str,
    ) -> Vec<String> {
        let final_payload = entity_payload(database, "time_entry", Some(entry_id)).unwrap();
        let connection = database.open().unwrap();
        let mut operation_ids = Vec::new();
        for (index, version) in (5_i64..=8).enumerate() {
            let operation_type = match version {
                5 | 6 => "timer_continuity_close",
                7 => "timer_stop",
                _ => "time_allocation_replace",
            };
            let operation_id =
                stable_timer_operation_id(entry_id, Some(version - 1), version, operation_type);
            let mut payload = final_payload.clone();
            payload["version"] = json!(version);
            connection.execute(
                "INSERT INTO sync_outbox(operation_id,workspace_id,device_id,operation_type,entity_type,entity_id,base_version,payload_json,payload_version,state,attempt_count,created_at)
                 VALUES (?1,?2,?3,?4,'time_entry',?5,?6,?7,?8,'pending',0,?9)",
                params![operation_id,cloud_workspace_id,device_id,operation_type,entry_id,version-1,payload.to_string(),version,index as i64 + 1],
            ).unwrap();
            operation_ids.push(operation_id);
        }
        operation_ids
    }

    #[test]
    fn stable_timer_operation_ids_bind_the_complete_transition() {
        let same_a = stable_timer_operation_id("entry", Some(4), 5, "timer_stop");
        let same_b = stable_timer_operation_id("entry", Some(4), 5, "timer_stop");
        assert_eq!(same_a, same_b);
        assert_ne!(
            same_a,
            stable_timer_operation_id("entry", Some(3), 5, "timer_stop")
        );
        assert_ne!(
            same_a,
            stable_timer_operation_id("entry", Some(4), 6, "timer_stop")
        );
        assert_ne!(
            same_a,
            stable_timer_operation_id("entry", Some(4), 5, "timer_pause")
        );
        assert_ne!(
            same_a,
            stable_timer_operation_id("other", Some(4), 5, "timer_stop")
        );
    }

    #[test]
    fn timer_follow_classifier_covers_all_timer_intents_and_missing_authority() {
        let base = OutboxOperation {
            operation_id: "operation".to_string(),
            workspace_epoch: 1,
            operation_type: String::new(),
            entity_type: "time_entry".to_string(),
            entity_id: Some("entry".to_string()),
            base_version: Some(1),
            payload_version: Some(2),
            coalesced_count: 1,
            depends_on_operation_id: None,
            payload: json!({}),
        };
        for (operation_type, state) in [
            ("timer_start", "running"),
            ("timer_pause", "paused"),
            ("timer_resume", "running"),
            ("timer_stop", "ended"),
            ("timer_continuity_close", "ended"),
            ("timer_calendar_split", "ended"),
        ] {
            let mut operation = base.clone();
            operation.operation_type = operation_type.to_string();
            assert!(timer_fact_can_follow(
                &operation,
                &json!({"source_type":"timer","state":state})
            ));
            assert!(!timer_fact_can_follow(&operation, &json!({})));
        }
    }

    #[test]
    fn timer_chain_rebase_collapses_4_to_8_onto_cloud_6_and_is_repeatable() {
        let (_directory, database, cloud_workspace_id, _local_workspace_id, device_id, entry_id) =
            timer_recovery_fixture("timer-chain-rebase.sqlite3");
        let old_ids =
            insert_timer_version_chain(&database, &cloud_workspace_id, &device_id, &entry_id);
        let dependent_id = Uuid::now_v7().to_string();
        database.open().unwrap().execute(
            "INSERT INTO sync_outbox(operation_id,workspace_id,device_id,operation_type,entity_type,entity_id,base_version,payload_json,depends_on_operation_id,state,attempt_count,created_at)
             VALUES (?1,?2,?3,'task_set_completed_from_allocation','task','dependent-task',1,'{}',?4,'pending',0,10)",
            params![dependent_id,cloud_workspace_id,device_id,old_ids[3]],
        ).unwrap();
        let chain = load_timer_entity_chain(&database, &cloud_workspace_id, &entry_id).unwrap();
        rebase_timer_entity_chain(&database, &cloud_workspace_id, &entry_id, 6, None, &chain)
            .unwrap();
        let replacement_id =
            stable_timer_operation_id(&entry_id, Some(6), 7, "time_allocation_replace");
        let connection = database.open().unwrap();
        let row: (i64,i64,i64,String,Option<String>,String) = connection.query_row(
            "SELECT base_version,payload_version,coalesced_count,state,error_json,recovery_kind FROM sync_outbox WHERE entity_type='time_entry' AND entity_id=?1",
            [&entry_id],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?)),
        ).unwrap();
        assert_eq!(
            row,
            (6, 7, 1, "pending".to_string(), None, "rebased".to_string())
        );
        let payload_text: String = connection
            .query_row(
                "SELECT payload_json FROM sync_outbox WHERE operation_id=?1",
                [&replacement_id],
                |row| row.get(0),
            )
            .unwrap();
        let payload: Value = serde_json::from_str(&payload_text).unwrap();
        assert_eq!(payload["version"], 7);
        assert_eq!(payload["segments"].as_array().unwrap().len(), 6);
        assert_eq!(payload["allocations"].as_array().unwrap().len(), 1);
        let dependency: String = connection
            .query_row(
                "SELECT depends_on_operation_id FROM sync_outbox WHERE operation_id=?1",
                [&dependent_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(dependency, replacement_id);
        drop(connection);
        let rebased_chain =
            load_timer_entity_chain(&database, &cloud_workspace_id, &entry_id).unwrap();
        rebase_timer_entity_chain(
            &database,
            &cloud_workspace_id,
            &entry_id,
            6,
            None,
            &rebased_chain,
        )
        .unwrap();
        let count: i64 = database
            .open()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM sync_outbox WHERE entity_type='time_entry' AND entity_id=?1",
                [&entry_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn timer_recovery_classifier_covers_ack_rebase_and_competing_edits() {
        let (_directory, database, cloud_workspace_id, _local_workspace_id, device_id, entry_id) =
            timer_recovery_fixture("timer-recovery-classifier.sqlite3");
        insert_timer_version_chain(&database, &cloud_workspace_id, &device_id, &entry_id);
        let state = storage_mode(&database).unwrap();
        let operation = load_timer_entity_chain(&database, &cloud_workspace_id, &entry_id)
            .unwrap()
            .remove(0);
        assert_eq!(
            reconcile_timer_sync_context(
                &database,
                &state,
                &operation,
                &json!({
                    "processedOperation": {"matchesRequest": true, "result": null}
                })
            )
            .unwrap(),
            TimerSyncRecovery::Acknowledged
        );
        let local_final = load_timer_entity_chain(&database, &cloud_workspace_id, &entry_id)
            .unwrap()
            .last()
            .unwrap()
            .payload
            .clone();
        let mut same_version = local_final.clone();
        same_version["updated_by_device_id"] = json!(device_id);
        assert_eq!(
            reconcile_timer_sync_context(
                &database,
                &state,
                &operation,
                &json!({
                    "currentVersion": 8, "authoritative": same_version
                })
            )
            .unwrap(),
            TimerSyncRecovery::Acknowledged
        );

        let (_directory, database, cloud_workspace_id, _local_workspace_id, device_id, entry_id) =
            timer_recovery_fixture("timer-recovery-rebase-classifier.sqlite3");
        insert_timer_version_chain(&database, &cloud_workspace_id, &device_id, &entry_id);
        let state = storage_mode(&database).unwrap();
        let operation = load_timer_entity_chain(&database, &cloud_workspace_id, &entry_id)
            .unwrap()
            .remove(0);
        let mut authoritative = operation.payload.clone();
        authoritative["version"] = json!(6);
        authoritative["updated_by_device_id"] = json!(device_id);
        assert_eq!(
            reconcile_timer_sync_context(
                &database,
                &state,
                &operation,
                &json!({
                    "currentVersion": 6, "authoritative": authoritative
                })
            )
            .unwrap(),
            TimerSyncRecovery::Rebased
        );

        let (_directory, database, cloud_workspace_id, _local_workspace_id, device_id, entry_id) =
            timer_recovery_fixture("timer-recovery-conflict-classifier.sqlite3");
        insert_timer_version_chain(&database, &cloud_workspace_id, &device_id, &entry_id);
        let state = storage_mode(&database).unwrap();
        let operation = load_timer_entity_chain(&database, &cloud_workspace_id, &entry_id)
            .unwrap()
            .remove(0);
        let mut authoritative = operation.payload.clone();
        authoritative["version"] = json!(6);
        authoritative["updated_by_device_id"] = json!(Uuid::now_v7().to_string());
        let followed = reconcile_timer_sync_context(
            &database,
            &state,
            &operation,
            &json!({
                "currentVersion": 6, "authoritative": authoritative
            }),
        )
        .unwrap();
        assert!(matches!(followed, TimerSyncRecovery::Followed { .. }));
        let mut newer = operation.payload.clone();
        newer["version"] = json!(9);
        newer["note"] = json!("另一份最终状态");
        newer["updated_by_device_id"] = json!(device_id);
        assert!(matches!(
            reconcile_timer_sync_context(
                &database,
                &state,
                &operation,
                &json!({
                    "currentVersion": 9, "authoritative": newer
                })
            )
            .unwrap(),
            TimerSyncRecovery::Followed { .. }
        ));
    }

    #[test]
    fn authoritative_timer_adoption_replaces_full_aggregate_and_server_version() {
        let (_directory, database, cloud_workspace_id, local_workspace_id, device_id, entry_id) =
            timer_recovery_fixture("timer-authoritative-adoption.sqlite3");
        insert_timer_version_chain(&database, &cloud_workspace_id, &device_id, &entry_id);
        let operation = load_timer_entity_chain(&database, &cloud_workspace_id, &entry_id)
            .unwrap()
            .remove(0);
        let following_operation_id = Uuid::now_v7().to_string();
        database.open().unwrap().execute(
            "INSERT INTO sync_outbox(operation_id,workspace_id,device_id,operation_type,entity_type,entity_id,base_version,payload_json,payload_version,state,attempt_count,created_at)
             VALUES (?1,?2,?3,'timer_followup','time_entry',?4,5,'{}',8,'pending',0,20)",
            params![following_operation_id,cloud_workspace_id,device_id,entry_id],
        ).unwrap();
        let task_id: String = database
            .open()
            .unwrap()
            .query_row(
                "SELECT id FROM tasks WHERE title='恢复事项'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let authoritative = json!({
            "id":entry_id,"work_date":"2026-10-14","kind":"work","source_type":"timer",
            "state":"ended","default_task_id":task_id,"label_snapshot":"云端权威状态",
            "started_at":1000,"ended_at":181000,"duration_seconds":180,"note":"权威结果",
            "origin_unassigned_session_id":null,"created_at":1000,"updated_at":181000,"version":7,
            "deleted_at":null,"timer_chain_id":entry_id,"previous_entry_id":null,"split_boundary_at":null,
            "last_continuous_at":181000,
            "segments":[{"id":Uuid::now_v7().to_string(),"sequence_no":1,"started_at":1000,"ended_at":181000,"duration_seconds":180}],
            "allocations":[{"id":Uuid::now_v7().to_string(),"task_id":task_id,"minutes":3,"note":"云端分配","created_at":181000,"updated_at":181000,"version":1}]
        });
        adopt_authoritative_timer_image(&database, &operation, &authoritative).unwrap();
        let connection = database.open().unwrap();
        let saved: (i64,String,i64) = connection.query_row(
            "SELECT version,label_snapshot,duration_seconds FROM time_entries WHERE workspace_id=?1 AND id=?2",
            params![local_workspace_id,entry_id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
        ).unwrap();
        assert_eq!(saved, (7, "云端权威状态".to_string(), 180));
        let children: (i64,i64,i64) = connection.query_row(
            "SELECT (SELECT COUNT(*) FROM time_segments WHERE entry_id=?1),(SELECT COUNT(*) FROM time_allocations WHERE entry_id=?1),(SELECT minutes FROM time_allocations WHERE entry_id=?1)",
            [&entry_id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
        ).unwrap();
        assert_eq!(children, (1, 1, 3));
        let following_base: i64 = connection
            .query_row(
                "SELECT base_version FROM sync_outbox WHERE operation_id=?1",
                [&following_operation_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(following_base, 7);
    }

    #[test]
    fn lease_renewal_without_takeover_does_not_mutate_the_local_timer() {
        let (_directory, database, _cloud_workspace_id, _local_workspace_id, _device_id, entry_id) =
            timer_recovery_fixture("timer-lease-renewal-no-takeover.sqlite3");

        adopt_expired_lease_takeover(
            &database,
            &json!({
                "acquired": true,
                "takeover": false,
                "authoritativeTimer": {"id": entry_id, "version": 6}
            }),
        )
        .unwrap();

        let connection = database.open().unwrap();
        let saved: (String, i64, String) = connection
            .query_row(
                "SELECT state,version,label_snapshot FROM time_entries WHERE id=?1",
                [&entry_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(saved, ("ended".to_string(), 8, "恢复事项".to_string()));
        let queued: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sync_outbox WHERE entity_id=?1",
                [&entry_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(queued, 0);
    }

    #[test]
    fn expired_lease_takeover_adopts_server_timer_before_creating_recovery_transition() {
        let (_directory, database, cloud_workspace_id, local_workspace_id, device_id, entry_id) =
            timer_recovery_fixture("timer-expired-lease-takeover.sqlite3");
        let task_id: String = database
            .open()
            .unwrap()
            .query_row(
                "SELECT id FROM tasks WHERE title='恢复事项'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let now = now_millis();
        let work_date =
            crate::work_calendar::work_date_at("Asia/Shanghai".parse().unwrap(), now).unwrap();
        let started_at = now - 300_000;
        let last_continuous_at = now - 120_000;
        let authoritative = json!({
            "id":entry_id,"work_date":work_date,"kind":"work","source_type":"timer",
            "state":"running","default_task_id":task_id,"label_snapshot":"接管前云端状态",
            "started_at":started_at,"ended_at":null,"duration_seconds":0,"note":"云端接管基线",
            "origin_unassigned_session_id":null,"created_at":started_at,"updated_at":last_continuous_at,
            "version":6,"deleted_at":null,"timer_chain_id":entry_id,"previous_entry_id":null,
            "split_boundary_at":null,"last_continuous_at":last_continuous_at,
            "created_by_device_id":device_id,"updated_by_device_id":device_id,
            "segments":[{
                "id":Uuid::now_v7().to_string(),"sequence_no":1,"started_at":started_at,
                "ended_at":null,"duration_seconds":0
            }],
            "allocations":[]
        });

        adopt_expired_lease_takeover(
            &database,
            &json!({
                "acquired": true,
                "takeover": true,
                "previousHolderDeviceId": Uuid::now_v7().to_string(),
                "authoritativeTimer": authoritative
            }),
        )
        .unwrap();

        let connection = database.open().unwrap();
        let saved: (String, i64, String, i64) = connection
            .query_row(
                "SELECT state,version,label_snapshot,duration_seconds FROM time_entries WHERE workspace_id=?1 AND id=?2",
                params![local_workspace_id,entry_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(saved.0, "running");
        assert_eq!(saved.1, 7);
        assert_eq!(saved.2, "接管前云端状态");
        assert_eq!(saved.3, 180);
        let segments: (i64, i64) = connection
            .query_row(
                "SELECT COUNT(*),SUM(CASE WHEN ended_at IS NULL THEN 1 ELSE 0 END) FROM time_segments WHERE entry_id=?1",
                [&entry_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(segments, (2, 1));
        let queued: (String, i64, i64, i64) = connection
            .query_row(
                "SELECT operation_type,base_version,payload_version,coalesced_count FROM sync_outbox WHERE workspace_id=?1 AND entity_id=?2",
                params![cloud_workspace_id,entry_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(queued, ("timer_continuity_close".to_string(), 6, 7, 1));
    }

    #[test]
    fn timer_rebase_rejects_attempted_cross_entity_side_effect_but_redirects_unattempted_one() {
        for (state_name, attempt_count, expected) in [("pending", 0, true), ("sending", 1, false)] {
            let file_name = format!("timer-side-effect-{state_name}.sqlite3");
            let (
                _directory,
                database,
                cloud_workspace_id,
                _local_workspace_id,
                device_id,
                entry_id,
            ) = timer_recovery_fixture(&file_name);
            let ids =
                insert_timer_version_chain(&database, &cloud_workspace_id, &device_id, &entry_id);
            database.open().unwrap().execute(
                "INSERT INTO sync_outbox(operation_id,workspace_id,device_id,operation_type,entity_type,entity_id,base_version,payload_json,depends_on_operation_id,state,attempt_count,created_at)
                 VALUES (?1,?2,?3,'task_set_completed_from_allocation','task','side-effect-task',1,'{}',?4,?5,?6,20)",
                params![Uuid::now_v7().to_string(),cloud_workspace_id,device_id,ids[3],state_name,attempt_count],
            ).unwrap();
            let chain = load_timer_entity_chain(&database, &cloud_workspace_id, &entry_id).unwrap();
            assert_eq!(
                timer_chain_can_rebase(&database, &cloud_workspace_id, &chain).unwrap(),
                expected
            );
            if expected {
                rebase_timer_entity_chain(
                    &database,
                    &cloud_workspace_id,
                    &entry_id,
                    6,
                    None,
                    &chain,
                )
                .unwrap();
                let replacement =
                    stable_timer_operation_id(&entry_id, Some(6), 7, "time_allocation_replace");
                let dependency: String = database.open().unwrap().query_row(
                    "SELECT depends_on_operation_id FROM sync_outbox WHERE entity_type='task' AND entity_id='side-effect-task'",
                    [],|row|row.get(0),
                ).unwrap();
                assert_eq!(dependency, replacement);
            }
        }
    }

    #[test]
    fn historical_scan_marks_conflicts_and_version_gaps_without_deleting_offline_data() {
        let (_directory, database, cloud_workspace_id, _local_workspace_id, device_id, entry_id) =
            timer_recovery_fixture("timer-historical-scan.sqlite3");
        let ids = insert_timer_version_chain(&database, &cloud_workspace_id, &device_id, &entry_id);
        let connection = database.open().unwrap();
        connection
            .execute(
                "UPDATE sync_outbox SET error_json=?1 WHERE operation_id=?2",
                params![
                    json!({"message":"SYNC_CONFLICT: expected version 4, current version 6"})
                        .to_string(),
                    ids[0]
                ],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE sync_outbox SET state='conflict',recovery_kind='competing_edit' WHERE operation_id=?1",
                [&ids[0]],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE sync_outbox SET base_version=99 WHERE operation_id=?1",
                [&ids[2]],
            )
            .unwrap();
        drop(connection);
        assert!(
            mark_historical_timer_recovery_candidates(&database, &cloud_workspace_id).unwrap() >= 2
        );
        let connection = database.open().unwrap();
        let count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM sync_outbox WHERE entity_type='time_entry' AND recovery_kind='historical_candidate'",[],|row|row.get(0),
        ).unwrap();
        assert!(count >= 2);
        let recovered: (String, String, Option<String>) = connection
            .query_row(
                "SELECT state,recovery_kind,error_json FROM sync_outbox WHERE operation_id=?1",
                [&ids[0]],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            recovered,
            (
                "pending".to_string(),
                "historical_candidate".to_string(),
                None
            )
        );
        let aggregate: (String,i64,i64,i64) = connection.query_row(
            "SELECT state,duration_seconds,(SELECT COUNT(*) FROM time_segments WHERE entry_id=?1),(SELECT COUNT(*) FROM time_allocations WHERE entry_id=?1) FROM time_entries WHERE id=?1",
            [&entry_id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
        ).unwrap();
        assert_eq!(aggregate, ("ended".to_string(), 360, 6, 1));
        drop(connection);
        assert_eq!(
            mark_historical_timer_recovery_candidates(&database, &cloud_workspace_id).unwrap(),
            0
        );
    }

    #[test]
    fn recovery_metadata_only_counts_explicit_conflicts() {
        let (_directory, database, cloud_workspace_id, _local_workspace_id, device_id, entry_id) =
            timer_recovery_fixture("timer-recovery-status.sqlite3");
        let ids = insert_timer_version_chain(&database, &cloud_workspace_id, &device_id, &entry_id);
        mark_outbox_recovery_error(
            &database,
            &ids[0],
            "pending",
            "historical_candidate",
            "等待自动收敛",
            Some(&json!({"currentVersion":6})),
        )
        .unwrap();
        assert_eq!(outbox_counts(&database, &cloud_workspace_id).unwrap().1, 0);
        let queue = list_sync_queue(&database, &cloud_workspace_id).unwrap();
        let waiting = queue
            .iter()
            .find(|item| item.operation_id == ids[0])
            .unwrap();
        assert_eq!(waiting.base_version, Some(4));
        assert_eq!(waiting.payload_version, Some(5));
        assert_eq!(
            waiting.recovery_kind.as_deref(),
            Some("historical_candidate")
        );
        mark_outbox_recovery_error(
            &database,
            &ids[0],
            "conflict",
            "competing_edit",
            "另一设备修改了计时",
            Some(&json!({"authoritative":{"id":entry_id,"version":6}})),
        )
        .unwrap();
        assert_eq!(outbox_counts(&database, &cloud_workspace_id).unwrap().1, 1);
        let conflicts = list_local_conflicts(&database, &cloud_workspace_id).unwrap();
        assert_eq!(
            conflicts[0].conflict_reason.as_deref(),
            Some("另一设备修改了计时")
        );
        assert_eq!(conflicts[0].cloud_payload.as_ref().unwrap()["version"], 6);
    }
}
