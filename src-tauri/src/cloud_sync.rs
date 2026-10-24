use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use tauri::{AppHandle, Emitter};
use tokio::sync::Notify;

use crate::database::Database;
use crate::direct_cloud_realtime::{
    RealtimeConnectionConfig, RealtimeListenerHooks, WorkspaceInvalidation,
};
use crate::supabase::{client, current_session, direct_cloud_workbench_bootstrap, storage_mode};

static REALTIME_ENABLED: AtomicBool = AtomicBool::new(false);
static REALTIME_WAKEUP: OnceLock<Arc<Notify>> = OnceLock::new();
static CLOUD_OPERATION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub fn spawn_background_services(database: Database, app: AppHandle) {
    crate::direct_cloud_realtime::spawn_realtime_listener(
        database.clone(),
        app,
        realtime_wakeup(),
        RealtimeListenerHooks {
            load_config: realtime_connection_config,
            is_enabled: realtime_enabled,
            on_error: emit_cloud_error,
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
    realtime_wakeup().notify_waiters();
}

pub(crate) fn disable_cloud_sync() {
    REALTIME_ENABLED.store(false, Ordering::Release);
    realtime_wakeup().notify_waiters();
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
            }
            Ok(Err(error)) => emit_cloud_error(&app, &database, &error),
            Err(error) => emit_cloud_error(
                &app,
                &database,
                &format!("SYNC_WORKER_ERROR: Realtime 初始查询失败: {error}"),
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

fn emit_cloud_error(app: &AppHandle, database: &Database, error: &str) {
    if let Ok(state) = storage_mode(database) {
        let _ = app.emit(
            "cloud-sync-state-changed",
            serde_json::json!({
                "mode": state.mode,
                "storageProtocol": state.storage_protocol,
                "storageBackend": state.storage_backend,
                "cloudAccessState": "offline_readonly",
                "cloudWritable": false,
                "readOnlyReason": error,
                "workspaceId": state.workspace_id,
                "deviceId": state.device_id,
                "online": false,
                "lastChangeSeq": state.last_change_seq,
                "lastSyncedAt": state.last_synced_at,
                "lastError": error,
                "workspaceEpoch": state.workspace_epoch,
                "error": error
            }),
        );
    }
}
