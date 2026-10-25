use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use tauri::{AppHandle, Emitter};
use tokio::sync::Notify;

use crate::database::Database;
use crate::direct_cloud_realtime::{
    RealtimeConnectionConfig, RealtimeListenerHooks, WorkspaceInvalidation,
};
use crate::supabase::{
    client, current_session, direct_cloud_workbench_bootstrap, storage_mode, StorageModeSnapshot,
};

static REALTIME_ENABLED: AtomicBool = AtomicBool::new(false);
static REALTIME_WAKEUP: OnceLock<Arc<Notify>> = OnceLock::new();
static CLOUD_OPERATION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

pub fn spawn_background_services(database: Database, app: AppHandle) {
    let _ = APP_HANDLE.set(app.clone());
    crate::direct_cloud_realtime::spawn_realtime_listener(
        database.clone(),
        app,
        realtime_wakeup(),
        RealtimeListenerHooks {
            load_config: realtime_connection_config,
            is_enabled: realtime_enabled,
            on_error: emit_realtime_error,
            on_connected: handle_realtime_connected,
            on_legacy_change: ignore_retired_legacy_change,
            workspace_epoch: realtime_workspace_epoch,
            covered_change_seq: realtime_covered_change_seq,
            on_invalidation: handle_realtime_invalidation,
        },
    );
    if storage_mode(&database).is_ok_and(|state| state.mode == "cloud") {
        enter_direct_cloud_mode();
    }
}

fn realtime_wakeup() -> Arc<Notify> {
    REALTIME_WAKEUP
        .get_or_init(|| Arc::new(Notify::new()))
        .clone()
}

pub(crate) fn wake_after_connection_ready() {
    enter_direct_cloud_mode();
}

pub(crate) fn enter_direct_cloud_mode() {
    REALTIME_ENABLED.store(true, Ordering::Release);
    realtime_wakeup().notify_one();
}

#[tauri::command]
pub fn cloud_connection_wake() {
    if realtime_enabled() {
        realtime_wakeup().notify_one();
    }
}

pub(crate) fn disable_cloud_sync() {
    REALTIME_ENABLED.store(false, Ordering::Release);
    realtime_wakeup().notify_one();
}

pub(crate) fn emit_revalidated_query(
    database: &Database,
    workspace_id: &str,
    workspace_epoch: i64,
    change_seq: i64,
    domains: &[String],
) {
    let Some(app) = APP_HANDLE.get() else {
        return;
    };
    let _ = app.emit(
        "work-data-changed",
        WorkspaceInvalidation {
            workspace_id: workspace_id.to_string(),
            change_seq,
            workspace_epoch,
            domains: domains.to_vec(),
            source: "cloud-cache-revalidated".to_string(),
            changed_by_device_id: None,
        },
    );
    if let Ok(state) = storage_mode(database) {
        let _ = app.emit("cloud-sync-state-changed", state);
    }
}

pub(crate) fn with_cloud_sync_operation_lock<T>(
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let _guard = CLOUD_OPERATION_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    operation()
}

fn realtime_enabled() -> bool {
    REALTIME_ENABLED.load(Ordering::Acquire)
}

fn realtime_connection_config(
    database: &Database,
) -> Result<Option<RealtimeConnectionConfig>, String> {
    let state = storage_mode(database)?;
    if state.mode != "cloud" || !state.online {
        return Ok(None);
    }
    let api = client(database)?;
    let session = current_session(database)?;
    let anon_key = api.anon_key().to_string();
    Ok(Some(RealtimeConnectionConfig {
        workspace_id: state.workspace_id,
        storage_protocol: crate::storage_contract::DIRECT_CLOUD_PROTOCOL.to_string(),
        project_url: api.project_url,
        anon_key,
        access_token: session.access_token,
    }))
}

fn handle_realtime_connected(database: &Database, app: &AppHandle, _storage_protocol: &str) {
    let database = database.clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let query_database = database.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            direct_cloud_workbench_bootstrap(&query_database)
        })
        .await;
        match result {
            Ok(Ok(response)) => {
                let _ = app.emit(
                    "work-data-changed",
                    WorkspaceInvalidation {
                        workspace_id: response.metadata.workspace_id,
                        change_seq: response.metadata.change_seq,
                        workspace_epoch: response.metadata.workspace_epoch,
                        domains: [
                            "subjects",
                            "tasks",
                            "time",
                            "unassigned",
                            "reports",
                            "settings",
                        ]
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                        source: "realtime-connected".to_string(),
                        changed_by_device_id: None,
                    },
                );
                emit_cloud_state(&app, &database, true, None);
            }
            Ok(Err(error)) => emit_cloud_state(&app, &database, true, Some(&error)),
            Err(error) => emit_cloud_state(
                &app,
                &database,
                true,
                Some(&format!(
                    "SYNC_WORKER_ERROR: Realtime 初始查询失败: {error}"
                )),
            ),
        }
    });
}

fn ignore_retired_legacy_change(_change_seq: Option<i64>) {}

fn realtime_workspace_epoch(database: &Database, _workspace_id: &str) -> i64 {
    storage_mode(database)
        .map(|state| state.workspace_epoch)
        .unwrap_or(1)
}

fn realtime_covered_change_seq(database: &Database) -> i64 {
    storage_mode(database)
        .map(|state| state.last_change_seq)
        .unwrap_or_default()
}

fn invalidate_persistent_read_cache(database: &Database, workspace_id: &str, domains: &[String]) {
    let Ok(session) = current_session(database) else {
        return;
    };
    let Ok(cache) = crate::cloud_read_cache::CloudReadCache::for_database(database) else {
        return;
    };
    let _ = cache.invalidate_domain(&session.user_id, workspace_id, "workspace");
    for domain in domains {
        let _ = cache.invalidate_domain(&session.user_id, workspace_id, domain);
    }
}

fn handle_realtime_invalidation(database: &Database, invalidation: &WorkspaceInvalidation) {
    invalidate_persistent_read_cache(database, &invalidation.workspace_id, &invalidation.domains);
}

fn cloud_state_payload(
    state: &StorageModeSnapshot,
    realtime_connected: bool,
    realtime_error: Option<&str>,
) -> serde_json::Value {
    let mut payload = serde_json::to_value(state).unwrap_or_else(|_| serde_json::json!({}));
    if let Some(object) = payload.as_object_mut() {
        object.insert(
            "realtimeConnected".to_string(),
            serde_json::json!(realtime_connected),
        );
        object.insert(
            "realtimeError".to_string(),
            realtime_error.map_or(serde_json::Value::Null, |error| serde_json::json!(error)),
        );
    }
    payload
}

fn emit_cloud_state(
    app: &AppHandle,
    database: &Database,
    realtime_connected: bool,
    realtime_error: Option<&str>,
) {
    if let Ok(state) = storage_mode(database) {
        let _ = app.emit(
            "cloud-sync-state-changed",
            cloud_state_payload(&state, realtime_connected, realtime_error),
        );
    }
}

fn emit_realtime_error(app: &AppHandle, database: &Database, error: &str) {
    emit_cloud_state(app, database, false, Some(error));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn writable_cloud_state() -> StorageModeSnapshot {
        StorageModeSnapshot {
            mode: "cloud".to_string(),
            storage_protocol: crate::storage_contract::DIRECT_CLOUD_PROTOCOL.to_string(),
            storage_backend: "direct_supabase".to_string(),
            cloud_access_state: "online_writable".to_string(),
            cloud_writable: true,
            read_only_reason: None,
            workspace_id: "workspace-1".to_string(),
            device_id: "device-1".to_string(),
            online: true,
            last_change_seq: 7,
            last_synced_at: Some(123),
            cache_source: None,
            cache_fetched_at: None,
            last_error: None,
            auth_blocked: false,
            auth_blocked_reason: None,
            workspace_epoch: 1,
        }
    }

    #[test]
    fn realtime_failure_does_not_downgrade_rest_access_to_read_only() {
        let payload = cloud_state_payload(
            &writable_cloud_state(),
            false,
            Some("NETWORK_ERROR: Supabase Realtime 心跳失败: Broken pipe"),
        );
        assert_eq!(payload["cloudAccessState"], "online_writable");
        assert_eq!(payload["cloudWritable"], true);
        assert_eq!(payload["online"], true);
        assert_eq!(payload["realtimeConnected"], false);
        assert_eq!(
            payload["realtimeError"],
            "NETWORK_ERROR: Supabase Realtime 心跳失败: Broken pipe"
        );
        assert!(payload["readOnlyReason"].is_null());
    }

    #[test]
    fn realtime_recovery_clears_only_the_realtime_error() {
        let payload = cloud_state_payload(&writable_cloud_state(), true, None);
        assert_eq!(payload["cloudAccessState"], "online_writable");
        assert_eq!(payload["cloudWritable"], true);
        assert_eq!(payload["realtimeConnected"], true);
        assert!(payload["realtimeError"].is_null());
    }
}
