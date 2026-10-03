use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;
use uuid::Uuid;

use crate::database::Database;
use crate::supabase::{client, current_session, storage_mode, CLOUD_SCHEMA};

const REALTIME_HEARTBEAT_SECONDS: u64 = 25;
const REALTIME_RECONNECT_SECONDS: u64 = 5;
const OUTBOX_SENDING_STALE_AFTER_MILLIS: i64 = 5 * 60 * 1000;
const OUTBOX_RETRY_MAX_BACKOFF_MILLIS: i64 = 60 * 1000;

pub fn spawn_background_services(database: Database, app: AppHandle) {
    spawn_background_lease(database.clone(), app.clone());
    spawn_realtime_listener(database, app);
}

fn spawn_background_lease(database: Database, app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut lease_token: Option<String> = None;
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(15)).await;
            let Ok(state) = storage_mode(&database) else {
                lease_token = None;
                continue;
            };
            if state.mode != "cloud" || !state.online {
                lease_token = None;
                continue;
            }
            if let Ok(push) = push_outbox(&database) {
                if push.pending > 0 || push.conflicts > 0 {
                    emit_cloud_refresh(&app, &database, "outbox-state");
                }
                if push.pending == 0 && push.conflicts == 0 {
                    if push.pushed > 0 {
                        emit_cloud_refresh(&app, &database, "outbox");
                    } else if pull(
                        &database,
                        CloudSyncPullRequest {
                            after_change_seq: None,
                            limit: None,
                        },
                    )
                    .is_ok_and(|result| !result.changes.is_empty())
                    {
                        emit_cloud_refresh(&app, &database, "poll");
                    }
                }
            }
            let result = if let Some(token) = lease_token.clone() {
                renew_lease(
                    &database,
                    TrackingLeaseRequest {
                        lease_token: Some(token),
                    },
                )
            } else {
                acquire_lease(&database)
            };
            match result {
                Ok(value) => {
                    lease_token = value
                        .get("leaseToken")
                        .or_else(|| value.get("lease_token"))
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                        .or(lease_token);
                    if value.get("acquired").and_then(Value::as_bool) == Some(false) {
                        lease_token = None;
                    }
                }
                Err(_) => lease_token = None,
            }
        }
    });
}

fn spawn_realtime_listener(database: Database, app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let state = match storage_mode(&database) {
                Ok(state) if state.mode == "cloud" && state.online => state,
                _ => {
                    tokio::time::sleep(Duration::from_secs(REALTIME_RECONNECT_SECONDS)).await;
                    continue;
                }
            };
            let api = match client(&database) {
                Ok(api) => api,
                Err(error) => {
                    emit_cloud_error(&app, &database, &error);
                    tokio::time::sleep(Duration::from_secs(REALTIME_RECONNECT_SECONDS)).await;
                    continue;
                }
            };
            let session = match current_session(&database) {
                Ok(session) => session,
                Err(error) => {
                    emit_cloud_error(&app, &database, &error);
                    tokio::time::sleep(Duration::from_secs(REALTIME_RECONNECT_SECONDS)).await;
                    continue;
                }
            };
            if let Err(error) = run_realtime_connection(
                &database,
                &app,
                &state.workspace_id,
                &api.project_url,
                api.anon_key(),
                &session.access_token,
            )
            .await
            {
                emit_cloud_error(&app, &database, &error);
            }
            tokio::time::sleep(Duration::from_secs(REALTIME_RECONNECT_SECONDS)).await;
        }
    });
}

async fn run_realtime_connection(
    database: &Database,
    app: &AppHandle,
    workspace_id: &str,
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
    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                let current_state = storage_mode(database)?;
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
                        } else if event == "postgres_changes" {
                            match pull(database, CloudSyncPullRequest { after_change_seq: None, limit: None }) {
                                Ok(result) if !result.changes.is_empty() => emit_cloud_refresh(app, database, "realtime"),
                                Ok(_) => {}
                                Err(error) if !error.starts_with("SYNC_PENDING:") => emit_cloud_error(app, database, &error),
                                Err(_) => {}
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

fn emit_cloud_refresh(app: &AppHandle, database: &Database, source: &str) {
    if let Ok(status) = sync_status(database) {
        let _ = app.emit("cloud-sync-state-changed", &status);
        let _ = app.emit(
            "work-data-changed",
            json!({
                "revision": status.last_change_seq,
                "domains": ["subjects", "tasks", "time", "unassigned", "reports", "settings"],
                "source": source
            }),
        );
    }
}

fn emit_cloud_error(app: &AppHandle, database: &Database, error: &str) {
    if let Ok(status) = sync_status(database) {
        let _ = app.emit(
            "cloud-sync-state-changed",
            json!({
                "mode": status.mode,
                "workspaceId": status.workspace_id,
                "deviceId": status.device_id,
                "online": false,
                "syncState": "error",
                "lastChangeSeq": status.last_change_seq,
                "lastSyncedAt": status.last_synced_at,
                "lastError": error,
                "pendingOperations": status.pending_operations,
                "conflictCount": status.conflict_count,
                "error": error
            }),
        );
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudSyncStatus {
    pub mode: String,
    pub workspace_id: String,
    pub device_id: String,
    pub online: bool,
    pub sync_state: String,
    pub last_change_seq: i64,
    pub last_synced_at: Option<i64>,
    pub last_error: Option<String>,
    pub pending_operations: i64,
    pub conflict_count: i64,
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
    pub change_seq: i64,
    pub workspace_id: String,
    pub entity_type: String,
    pub entity_id: String,
    pub operation: String,
    pub entity_version: i64,
    pub changed_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudSyncPullResult {
    pub changes: Vec<WorkspaceChange>,
    pub last_change_seq: i64,
    pub has_more: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudSyncPushResult {
    pub pushed: i64,
    pub pending: i64,
    pub conflicts: i64,
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
    pub error: Option<String>,
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

#[derive(Debug)]
struct OutboxOperation {
    operation_id: String,
    operation_type: String,
    entity_type: String,
    entity_id: Option<String>,
    base_version: Option<i64>,
    payload: Value,
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
        workspace_id: state.workspace_id,
        device_id: state.device_id,
        online: state.online,
        sync_state: state.sync_state,
        last_change_seq: state.last_change_seq,
        last_synced_at: state.last_synced_at,
        last_error: state.last_error,
        pending_operations: state.pending_operations,
        conflict_count: state.conflict_count,
    })
}

pub fn pull(
    database: &Database,
    request: CloudSyncPullRequest,
) -> Result<CloudSyncPullResult, String> {
    let state = storage_mode(database)?;
    if state.mode != "cloud" {
        return Err("OFFLINE_RESTRICTED: 只有云端模式可以拉取 Supabase 增量".to_string());
    }
    let session = current_session(database)?;
    let api = client(database)?;
    let after = request
        .after_change_seq
        .unwrap_or(state.last_change_seq)
        .max(0);
    let limit = request.limit.unwrap_or(500).clamp(1, 1000);
    let response = api.request(
        api.get(format!(
            "{}/rest/v1/workspace_changes?workspace_id=eq.{}&change_seq=gt.{}&select=change_seq,workspace_id,entity_type,entity_id,operation,entity_version,changed_at&order=change_seq.asc&limit={}",
            api.project_url, state.workspace_id, after, limit
        )),
        &session,
    )?;
    let changes: Vec<WorkspaceChange> = response
        .json()
        .map_err(|error| format!("NETWORK_ERROR: 云端增量响应无效: {error}"))?;
    let last_change_seq = changes
        .last()
        .map(|change| change.change_seq)
        .unwrap_or(after);
    if !changes.is_empty() {
        pull_snapshot(database)?;
    } else {
        update_sync_state(
            database,
            &state.workspace_id,
            &state.device_id,
            last_change_seq,
            None,
        )?;
    }
    Ok(CloudSyncPullResult {
        has_more: changes.len() as i64 == limit,
        changes,
        last_change_seq,
    })
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

#[cfg(test)]
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
    let payload = entity_payload(database, entity_type, entity_id)?;
    let operation_id = operation_id
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| Uuid::now_v7().to_string());
    let connection = database.open()?;
    connection
        .execute(
            "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, state, attempt_count, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'pending', 0, ?9)
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
) -> Result<(), String> {
    if state.mode != "cloud" {
        return Ok(());
    }
    let payload = entity_payload_from_connection(transaction, entity_type, entity_id)?;
    let operation_id = operation_id
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| Uuid::now_v7().to_string());
    transaction
        .execute(
            "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, state, attempt_count, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'pending', 0, ?9)
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
                now_millis()
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn flush_if_online(database: &Database) -> Result<(), String> {
    let state = storage_mode(database)?;
    if state.mode == "cloud" && state.online {
        let result = push_outbox(database)?;
        if result.conflicts > 0 {
            return Err("SYNC_CONFLICT: 云端数据已变化，本地修改已保留在冲突队列".to_string());
        }
    }
    Ok(())
}

pub fn push_outbox(database: &Database) -> Result<CloudSyncPushResult, String> {
    let state = require_cloud(database)?;
    let session = current_session(database)?;
    let api = client(database)?;
    let operations = claim_pending_operations(database, &state.workspace_id)?;
    let mut pushed = 0;
    for operation in operations {
        let response = api.rpc(
            "cloud_apply_patch",
            json!({
                "p_workspace_id": state.workspace_id,
                "p_device_id": state.device_id,
                "p_operation_id": operation.operation_id,
                "p_operation_type": operation.operation_type,
                "p_entity_type": operation.entity_type,
                "p_entity_id": operation.entity_id,
                "p_base_version": operation.base_version,
                "p_payload": operation.payload
            }),
            &session,
        );
        match response {
            Ok(_) => {
                delete_outbox(database, &operation.operation_id)?;
                pushed += 1;
            }
            Err(error) if is_conflict_error(&error) => {
                mark_outbox_error(database, &operation.operation_id, "conflict", &error)?;
                break;
            }
            Err(error) => {
                mark_outbox_error(database, &operation.operation_id, "pending", &error)?;
                update_sync_state(
                    database,
                    &state.workspace_id,
                    &state.device_id,
                    state.last_change_seq,
                    Some(&error),
                )?;
                break;
            }
        }
    }
    let (pending, conflicts) = outbox_counts(database, &state.workspace_id)?;
    if should_pull_snapshot_after_push(pushed, pending, conflicts) {
        pull_snapshot(database)?;
    }
    Ok(CloudSyncPushResult {
        pushed,
        pending,
        conflicts,
    })
}

fn should_pull_snapshot_after_push(pushed: i64, pending: i64, conflicts: i64) -> bool {
    pushed > 0 && pending == 0 && conflicts == 0
}

pub fn list_conflicts(database: &Database) -> Result<Vec<CloudSyncConflict>, String> {
    let state = storage_mode(database)?;
    if state.mode != "cloud" {
        return Ok(Vec::new());
    }
    let connection = database.open()?;
    let mut statement = connection
        .prepare(
            "SELECT operation_id, operation_type, entity_type, entity_id, base_version, payload_json,
                    error_json, attempt_count, created_at, last_attempt_at
             FROM sync_outbox
             WHERE workspace_id = ?1 AND state = 'conflict'
             ORDER BY created_at, operation_id",
        )
        .map_err(|error| error.to_string())?;
    let conflicts = statement
        .query_map([state.workspace_id], |row| {
            let payload: String = row.get(5)?;
            let error_json: Option<String> = row.get(6)?;
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
                error,
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

pub fn resolve_conflict(
    database: &Database,
    request: CloudConflictResolveRequest,
) -> Result<CloudSyncStatus, String> {
    let state = require_cloud(database)?;
    let conflict = load_conflict(database, &state.workspace_id, &request.operation_id)?;
    match request.strategy.as_str() {
        "use_cloud" => {
            delete_outbox(database, &conflict.operation_id)?;
        }
        "keep_local" => {
            let session = current_session(database)?;
            let snapshot = client(database)?.rpc(
                "cloud_snapshot_get",
                json!({ "p_workspace_id": state.workspace_id }),
                &session,
            )?;
            let cloud_version = cloud_entity_version(
                &snapshot,
                &conflict.entity_type,
                conflict.entity_id.as_deref(),
                &conflict.payload,
            );
            if cloud_version.is_none() && conflict.base_version.is_some() {
                return Err(
                    "SYNC_CONFLICT_DELETED: 云端实体已被删除，不能用本地更新覆盖；请选择使用云端，或将本地内容复制为新事项"
                        .to_string(),
                );
            }
            let mut payload = conflict.payload.clone();
            if let (Some(version), Some(object)) = (cloud_version, payload.as_object_mut()) {
                object.insert("version".to_string(), json!(version + 1));
                if object.contains_key("updated_at") {
                    object.insert("updated_at".to_string(), json!(now_millis()));
                }
            }
            database
                .open()?
                .execute(
                    "UPDATE sync_outbox
                     SET base_version = ?1, payload_json = ?2, state = 'pending', error_json = NULL
                     WHERE workspace_id = ?3 AND operation_id = ?4 AND state = 'conflict'",
                    params![
                        cloud_version,
                        payload.to_string(),
                        state.workspace_id,
                        conflict.operation_id
                    ],
                )
                .map_err(|error| error.to_string())?;
        }
        _ => {
            return Err("VALIDATION_ERROR: 冲突处理策略只能是 use_cloud 或 keep_local".to_string())
        }
    }

    let result = push_outbox(database)?;
    if result.pending == 0 && result.conflicts == 0 {
        pull_snapshot(database)?;
    }
    sync_status(database)
}

fn load_conflict(
    database: &Database,
    workspace_id: &str,
    operation_id: &str,
) -> Result<OutboxOperation, String> {
    database
        .open()?
        .query_row(
            "SELECT operation_id, operation_type, entity_type, entity_id, base_version, payload_json
             FROM sync_outbox
             WHERE workspace_id = ?1 AND operation_id = ?2 AND state = 'conflict'",
            params![workspace_id, operation_id],
            |row| {
                let payload: String = row.get(5)?;
                Ok(OutboxOperation {
                    operation_id: row.get(0)?,
                    operation_type: row.get(1)?,
                    entity_type: row.get(2)?,
                    entity_id: row.get(3)?,
                    base_version: row.get(4)?,
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
    let (table, key, expected) = match entity_type {
        "subject" => ("subjects", "id", entity_id?),
        "task" => ("tasks", "id", entity_id?),
        "time_entry" => ("time_entries", "id", entity_id?),
        "report" => ("reports", "id", entity_id?),
        "report_template" => ("report_templates", "id", entity_id?),
        "app_setting" => ("app_settings", "key", payload.get("key")?.as_str()?),
        "integration_config" => (
            "integration_configs",
            "provider",
            payload.get("provider")?.as_str()?,
        ),
        _ => return None,
    };
    snapshot
        .get(table)?
        .as_array()?
        .iter()
        .find(|row| row.get(key).and_then(Value::as_str) == Some(expected))
        .and_then(|row| row.get("version"))
        .and_then(Value::as_i64)
}

pub fn pull_snapshot(database: &Database) -> Result<i64, String> {
    let state = storage_mode(database)?;
    if state.mode != "cloud" {
        return Err("OFFLINE_RESTRICTED: 当前不是 Supabase 云端模式".to_string());
    }
    let (pending, conflicts) = outbox_counts(database, &state.workspace_id)?;
    if pending > 0 || conflicts > 0 {
        return Err(
            "SYNC_PENDING: 本机仍有待同步或冲突操作，不能用云端快照覆盖本地编辑".to_string(),
        );
    }
    if !state.online {
        return Err("AUTH_REQUIRED: 云端会话不可用".to_string());
    }
    let expected_local_revision = local_revision(database)?;
    let session = current_session(database)?;
    let snapshot = client(database)?.rpc(
        "cloud_snapshot_get",
        json!({ "p_workspace_id": state.workspace_id }),
        &session,
    )?;
    let latest_change_seq = snapshot
        .get("latest_change_seq")
        .and_then(Value::as_i64)
        .unwrap_or(state.last_change_seq);
    apply_snapshot(
        database,
        &snapshot,
        Some(expected_local_revision),
        Some(&state.workspace_id),
        Some((&state.workspace_id, &state.device_id, latest_change_seq)),
    )?;
    Ok(latest_change_seq)
}

pub fn acquire_lease(database: &Database) -> Result<Value, String> {
    let state = require_cloud(database)?;
    let session = current_session(database)?;
    let api = client(database)?;
    api.rpc(
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
    let token = request
        .lease_token
        .ok_or_else(|| "VALIDATION_ERROR: 缺少租约令牌".to_string())?;
    let state = require_cloud(database)?;
    let session = current_session(database)?;
    client(database)?.rpc(
        "tracking_lease_renew",
        json!({ "p_workspace_id": state.workspace_id, "p_device_id": state.device_id, "p_lease_token": token }),
        &session,
    )
}

pub fn release_lease(database: &Database, request: TrackingLeaseRequest) -> Result<Value, String> {
    let token = request
        .lease_token
        .ok_or_else(|| "VALIDATION_ERROR: 缺少租约令牌".to_string())?;
    let state = require_cloud(database)?;
    let session = current_session(database)?;
    client(database)?.rpc(
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
        .request(
            api.get(format!(
                "{}/rest/v1/tracking_leases?workspace_id=eq.{}&select=workspace_id,holder_device_id,lease_token,expires_at,version",
                api.project_url, state.workspace_id
            )),
            &session,
        )?
        .json()
        .map_err(|error| format!("NETWORK_ERROR: 读取采集租约失败: {error}"))
}

pub fn timer_start(database: &Database, request: CloudTimerStartRequest) -> Result<Value, String> {
    let operation_id = request.operation_id.clone();
    let state = require_cloud(database)?;
    ensure_clean_outbox(database, &state.workspace_id)?;
    let session = current_session(database)?;
    let result = client(database)?.rpc(
        "timer_start",
        json!({
            "p_workspace_id": state.workspace_id,
            "p_task_id": request.task_id,
            "p_device_id": state.device_id,
            "p_operation_id": request.operation_id,
            "p_note": request.note
        }),
        &session,
    )?;
    pull_snapshot(database)?;
    if let Some(entry) = crate::time_tracking::get_timer_state(database)? {
        crate::time_tracking::publish_timer_hook(
            database,
            "timer.started",
            format!("timer.started:{operation_id}"),
            &entry,
        );
    }
    Ok(result)
}

pub fn timer_get_state(database: &Database) -> Result<Option<Value>, String> {
    let state = require_cloud(database)?;
    let session = current_session(database)?;
    let api = client(database)?;
    let rows: Vec<Value> = api
        .request(
            api.get(format!(
                "{}/rest/v1/time_entries?workspace_id=eq.{}&state=in.(running,paused)&deleted_at=is.null&select=*&order=created_at.desc&limit=1",
                api.project_url, state.workspace_id
            )),
            &session,
        )?
        .json()
        .map_err(|error| format!("NETWORK_ERROR: 读取云端计时状态失败: {error}"))?;
    Ok(rows.into_iter().next())
}

pub fn time_entry_list(database: &Database, work_date: &str) -> Result<Vec<Value>, String> {
    let state = require_cloud(database)?;
    let session = current_session(database)?;
    let api = client(database)?;
    api.request(
        api.get(format!(
            "{}/rest/v1/time_entries?workspace_id=eq.{}&work_date=eq.{}&deleted_at=is.null&select=*&order=started_at.desc",
            api.project_url, state.workspace_id, work_date
        )),
        &session,
    )?
    .json()
    .map_err(|error| format!("NETWORK_ERROR: 读取云端时间记录失败: {error}"))
}

pub fn timer_action(
    database: &Database,
    action: &str,
    request: CloudTimerVersionRequest,
) -> Result<Value, String> {
    let operation_id = request.operation_id.clone();
    let entry_id = request.entry_id.clone();
    let state = require_cloud(database)?;
    ensure_clean_outbox(database, &state.workspace_id)?;
    let session = current_session(database)?;
    let result = client(database)?.rpc(
        action,
        json!({
            "p_workspace_id": state.workspace_id,
            "p_entry_id": request.entry_id,
            "p_device_id": state.device_id,
            "p_expected_version": request.expected_version,
            "p_operation_id": request.operation_id
        }),
        &session,
    )?;
    pull_snapshot(database)?;
    if action == "timer_stop" {
        if let Ok(entry) = crate::time_tracking::get_time_entry_by_id(database, &entry_id) {
            crate::time_tracking::publish_timer_hook(
                database,
                "timer.stopped",
                format!("timer.stopped:{operation_id}"),
                &entry,
            );
        }
    }
    Ok(result)
}

pub fn unassigned_get_state(database: &Database) -> Result<Option<Value>, String> {
    let state = require_cloud(database)?;
    let session = current_session(database)?;
    client(database)?
        .rpc(
            "unassigned_get_state",
            json!({ "p_workspace_id": state.workspace_id }),
            &session,
        )
        .map(|value| if value.is_null() { None } else { Some(value) })
}

pub fn unassigned_resolve(
    database: &Database,
    action: &str,
    request: CloudUnassignedResolveRequest,
) -> Result<Value, String> {
    let state = require_cloud(database)?;
    ensure_clean_outbox(database, &state.workspace_id)?;
    let session = current_session(database)?;
    let result = client(database)?.rpc(
        action,
        json!({
            "p_workspace_id": state.workspace_id,
            "p_session_id": request.session_id,
            "p_device_id": state.device_id,
            "p_expected_version": request.expected_version,
            "p_allocations": request.allocations.unwrap_or_default(),
            "p_operation_id": request.operation_id
        }),
        &session,
    )?;
    pull_snapshot(database)?;
    Ok(result)
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

#[cfg(test)]
fn entity_payload(
    database: &Database,
    entity_type: &str,
    entity_id: Option<&str>,
) -> Result<Value, String> {
    let connection = database.open()?;
    entity_payload_from_connection(&connection, entity_type, entity_id)
}

fn entity_payload_from_connection(
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
        "report_template" => ("SELECT * FROM report_templates WHERE workspace_id = ?1 AND id = ?2", entity_id),
        "app_setting" => ("SELECT * FROM app_settings WHERE workspace_id = ?1 AND key = ?2", entity_id),
        "integration_config" => ("SELECT workspace_id, provider, enabled, config_json, updated_at, version FROM integration_configs WHERE workspace_id = ?1 AND provider = ?2", entity_id),
        "external_binding" => ("SELECT * FROM external_bindings WHERE workspace_id = ?1 AND entity_id = ?2", entity_id),
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
    let mut statement = connection.prepare(sql).map_err(|error| error.to_string())?;
    let column_names = statement
        .column_names()
        .into_iter()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    statement
        .query_row(params![workspace_id, id], |row| {
            row_to_json(row, &column_names)
        })
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("NOT_FOUND: 待同步实体不存在 {id}"))
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

fn claim_pending_operations(
    database: &Database,
    workspace_id: &str,
) -> Result<Vec<OutboxOperation>, String> {
    let mut connection = database.open()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let now = now_millis();
    let stale_sending_before = now.saturating_sub(OUTBOX_SENDING_STALE_AFTER_MILLIS);
    let mut statement = transaction
        .prepare(
            "SELECT operation_id, operation_type, entity_type, entity_id, base_version, payload_json, state, attempt_count, last_attempt_at
             FROM sync_outbox
             WHERE workspace_id = ?1
               AND (state IN ('pending','failed') OR (state = 'sending' AND COALESCE(last_attempt_at, created_at) < ?2))
             ORDER BY created_at, operation_id",
        )
        .map_err(|error| error.to_string())?;
    let candidates = statement
        .query_map(params![workspace_id, stale_sending_before], |row| {
            let payload: String = row.get(5)?;
            let state: String = row.get(6)?;
            let attempt_count: i64 = row.get(7)?;
            let last_attempt_at: Option<i64> = row.get(8)?;
            Ok(OutboxOperation {
                operation_id: row.get(0)?,
                operation_type: row.get(1)?,
                entity_type: row.get(2)?,
                entity_id: row.get(3)?,
                base_version: row.get(4)?,
                payload: serde_json::from_str(&payload).unwrap_or(Value::Null),
            })
            .map(|operation| (operation, state, attempt_count, last_attempt_at))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(statement);

    let result = candidates
        .into_iter()
        .filter_map(|(operation, state, attempt_count, last_attempt_at)| {
            let retry_ready = state == "sending"
                || retry_ready_at(last_attempt_at, attempt_count)
                    .is_none_or(|ready_at| ready_at <= now);
            retry_ready.then_some(operation)
        })
        .collect::<Vec<_>>();

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

fn ensure_clean_outbox(database: &Database, workspace_id: &str) -> Result<(), String> {
    let (pending, conflicts) = outbox_counts(database, workspace_id)?;
    if pending > 0 || conflicts > 0 {
        Err("OFFLINE_RESTRICTED: 请先同步或处理本机待同步操作，再执行全局计时操作".to_string())
    } else {
        Ok(())
    }
}

fn is_conflict_error(error: &str) -> bool {
    error.contains("SYNC_CONFLICT") || error.contains("VERSION_CONFLICT")
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

fn apply_snapshot(
    database: &Database,
    snapshot: &Value,
    expected_local_revision: Option<i64>,
    sync_workspace_id: Option<&str>,
    sync_state_update: Option<(&str, &str, i64)>,
) -> Result<(), String> {
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
        if pending > 0 {
            return Err("SYNC_PENDING: 本机产生了新的待同步操作，已取消本次快照覆盖".to_string());
        }
    }
    clear_snapshot_cache(&transaction, &local_workspace_id)?;
    replace_snapshot_table(&transaction, snapshot, "subjects", &local_workspace_id)?;
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
        "unassigned_sessions",
        &local_workspace_id,
    )?;
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
    replace_snapshot_table(
        &transaction,
        snapshot,
        "external_bindings",
        &local_workspace_id,
    )?;
    transaction
        .execute(
            "UPDATE app_metadata SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT) WHERE key = 'global_revision'",
            [],
        )
        .map_err(|error| error.to_string())?;
    if let Some((workspace_id, device_id, last_change_seq)) = sync_state_update {
        transaction
            .execute(
                "INSERT INTO local_sync_state(workspace_id, device_id, last_change_seq, last_full_sync_at, last_error)
                 VALUES (?1, ?2, ?3, ?4, NULL)
                 ON CONFLICT(workspace_id) DO UPDATE SET device_id = excluded.device_id, last_change_seq = excluded.last_change_seq, last_full_sync_at = excluded.last_full_sync_at, last_error = excluded.last_error",
                params![workspace_id, device_id, last_change_seq, now_millis()],
            )
            .map_err(|error| error.to_string())?;
    }
    transaction.commit().map_err(|error| error.to_string())
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
        "tasks" => {
            for row in &rows {
                transaction.execute("INSERT INTO tasks(id,workspace_id,subject_id,parent_id,title,status,planned_date,planned_time,estimate_minutes,note,project_name,solution_name,source_type,source_ref,sort_order,completed_at,created_at,updated_at,version,deleted_at) VALUES (?1,?2,?3,NULL,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)", params![text(row,"id"),workspace_id,text(row,"subject_id"),text(row,"title"),text(row,"status"),optional_text(row,"planned_date"),optional_time(row,"planned_time"),optional_integer(row,"estimate_minutes"),optional_text(row,"note"),optional_text(row,"project_name"),optional_text(row,"solution_name"),text(row,"source_type"),optional_text(row,"source_ref"),integer(row,"sort_order"),optional_millis(row,"completed_at"),millis(row,"created_at"),millis(row,"updated_at"),integer(row,"version"),optional_millis(row,"deleted_at")]).map_err(|error| error.to_string())?;
            }
            for row in rows {
                if let Some(parent_id) = optional_text(&row, "parent_id") {
                    transaction
                        .execute(
                            "UPDATE tasks SET parent_id = ?1 WHERE workspace_id = ?2 AND id = ?3",
                            params![parent_id, workspace_id, text(&row, "id")],
                        )
                        .map_err(|error| error.to_string())?;
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
            replace_simple_rows(transaction, table, workspace_id, rows, |tx, row, ws| {
                tx.execute("INSERT INTO time_entries(id,workspace_id,work_date,kind,source_type,state,default_task_id,label_snapshot,started_at,ended_at,duration_seconds,note,origin_unassigned_session_id,created_at,updated_at,version,deleted_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)", params![text(row,"id"),ws,text(row,"work_date"),text(row,"kind"),text(row,"source_type"),text(row,"state"),optional_text(row,"default_task_id"),text(row,"label_snapshot"),millis(row,"started_at"),optional_millis(row,"ended_at"),integer(row,"duration_seconds"),optional_text(row,"note"),optional_text(row,"origin_unassigned_session_id"),millis(row,"created_at"),millis(row,"updated_at"),integer(row,"version"),optional_millis(row,"deleted_at")])
            })?
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
        "unassigned_sessions" => {
            replace_simple_rows(transaction, table, workspace_id, rows, |tx, row, ws| {
                tx.execute("INSERT INTO unassigned_sessions(id,workspace_id,work_date,state,threshold_seconds,duration_seconds,first_started_at,last_ended_at,prompted_at,resolution_type,generated_entry_id,resolved_at,created_at,updated_at,version) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)", params![text(row,"id"),ws,text(row,"work_date"),text(row,"state"),integer(row,"threshold_seconds"),integer(row,"duration_seconds"),millis(row,"first_started_at"),optional_millis(row,"last_ended_at"),optional_millis(row,"prompted_at"),optional_text(row,"resolution_type"),optional_text(row,"generated_entry_id"),optional_millis(row,"resolved_at"),millis(row,"created_at"),millis(row,"updated_at"),integer(row,"version")])
            })?
        }
        "unassigned_segments" => {
            replace_simple_rows(transaction, table, workspace_id, rows, |tx, row, ws| {
                tx.execute("INSERT INTO unassigned_segments(id,workspace_id,session_id,sequence_no,started_at,ended_at,duration_seconds,lease_token) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)", params![text(row,"id"),ws,text(row,"session_id"),integer(row,"sequence_no"),millis(row,"started_at"),optional_millis(row,"ended_at"),integer(row,"duration_seconds"),optional_text(row,"lease_token")])
            })?
        }
        "report_templates" => {
            replace_simple_rows(transaction, table, workspace_id, rows, |tx, row, ws| {
                tx.execute("INSERT INTO report_templates(id,workspace_id,report_type,subject_id,content,is_builtin,created_at,updated_at,version,deleted_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)", params![text(row,"id"),ws,text(row,"report_type"),optional_text(row,"subject_id"),text(row,"content"),boolean(row,"is_builtin") as i64,millis(row,"created_at"),millis(row,"updated_at"),integer(row,"version"),optional_millis(row,"deleted_at")])
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
    let _ = table;
    let _ = workspace_id;
    for row in &rows {
        insert(transaction, row, workspace_id).map_err(|error| error.to_string())?;
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
            "UPDATE plan_import_items SET confirmed_task_id = NULL WHERE workspace_id = ?1",
            [workspace_id],
        )
        .map_err(|error| error.to_string())?;
    for table in [
        "external_bindings",
        "integration_configs",
        "report_tasks",
        "task_occurrences",
        "task_recurrence_rules",
        "task_daily_estimates",
        "time_allocations",
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
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
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
    candidate.as_i64().or_else(|| {
        candidate.as_str().and_then(|value| {
            chrono::DateTime::parse_from_rfc3339(value)
                .ok()
                .map(|time| time.timestamp_millis())
        })
    })
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
pub fn cloud_sync_pull(
    database: tauri::State<'_, Database>,
    request: CloudSyncPullRequest,
) -> Result<CloudSyncPullResult, String> {
    pull(&database, request)
}

#[tauri::command]
pub fn cloud_sync_push(
    database: tauri::State<'_, Database>,
) -> Result<CloudSyncPushResult, String> {
    push_outbox(&database)
}

#[tauri::command]
pub fn cloud_sync_refresh(database: tauri::State<'_, Database>) -> Result<CloudSyncStatus, String> {
    pull_snapshot(&database)?;
    sync_status(&database)
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
    database: tauri::State<'_, Database>,
    request: CloudTimerStartRequest,
) -> Result<Value, String> {
    timer_start(&database, request)
}

#[tauri::command]
pub fn cloud_timer_get_state(
    database: tauri::State<'_, Database>,
) -> Result<Option<Value>, String> {
    timer_get_state(&database)
}

#[tauri::command]
pub fn cloud_time_entry_list(
    database: tauri::State<'_, Database>,
    work_date: String,
) -> Result<Vec<Value>, String> {
    time_entry_list(&database, &work_date)
}

#[tauri::command]
pub fn cloud_timer_pause(
    database: tauri::State<'_, Database>,
    request: CloudTimerVersionRequest,
) -> Result<Value, String> {
    timer_action(&database, "timer_pause", request)
}

#[tauri::command]
pub fn cloud_timer_resume(
    database: tauri::State<'_, Database>,
    request: CloudTimerVersionRequest,
) -> Result<Value, String> {
    timer_action(&database, "timer_resume", request)
}

#[tauri::command]
pub fn cloud_timer_stop(
    database: tauri::State<'_, Database>,
    request: CloudTimerVersionRequest,
) -> Result<Value, String> {
    timer_action(&database, "timer_stop", request)
}

#[tauri::command]
pub fn cloud_unassigned_get_state(
    database: tauri::State<'_, Database>,
) -> Result<Option<Value>, String> {
    unassigned_get_state(&database)
}

#[tauri::command]
pub fn cloud_unassigned_resolve_work(
    database: tauri::State<'_, Database>,
    request: CloudUnassignedResolveRequest,
) -> Result<Value, String> {
    unassigned_resolve(&database, "unassigned_resolve_work", request)
}

#[tauri::command]
pub fn cloud_unassigned_resolve_break(
    database: tauri::State<'_, Database>,
    request: CloudUnassignedResolveRequest,
) -> Result<Value, String> {
    unassigned_resolve(&database, "unassigned_resolve_break", request)
}

#[tauri::command]
pub fn cloud_unassigned_discard(
    database: tauri::State<'_, Database>,
    request: CloudUnassignedResolveRequest,
) -> Result<Value, String> {
    unassigned_resolve(&database, "unassigned_discard", request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{self, SettingsScope, SettingsUpdate};
    use crate::subjects::{create_subject, SubjectCreateRequest};
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
    fn cloud_entity_version_reads_versioned_snapshot_entities() {
        let snapshot = json!({
            "tasks": [{"id": "task-a", "version": 4}],
            "app_settings": [{"key": "salary_hourly_rate", "version": 3}],
            "integration_configs": [{"provider": "seatable", "version": 2}]
        });
        assert_eq!(
            cloud_entity_version(&snapshot, "task", Some("task-a"), &Value::Null),
            Some(4)
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
    fn outbox_only_refreshes_snapshot_after_the_queue_is_clean() {
        assert!(should_pull_snapshot_after_push(1, 0, 0));
        assert!(!should_pull_snapshot_after_push(0, 0, 0));
        assert!(!should_pull_snapshot_after_push(1, 1, 0));
        assert!(!should_pull_snapshot_after_push(1, 0, 1));
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
            "subjects": [{"id": subject_id, "name": "云端主体", "sort_order": 10, "created_at": now, "updated_at": now, "version": 7, "deleted_at": null}]
        });
        apply_snapshot(
            &database,
            &snapshot,
            None,
            Some(&workspace_id),
            Some((&workspace_id, &device_id, 42)),
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
}
