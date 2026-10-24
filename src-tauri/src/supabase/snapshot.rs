use serde_json::{json, Value};
use uuid::Uuid;

use crate::database::Database;

use super::*;

pub fn workspace_snapshot_create(
    database: &Database,
    request: CloudWorkspaceSnapshotCreateRequest,
) -> Result<CloudWorkspaceSnapshot, String> {
    let (state, session) = cloud_workspace_context(database, "云端快照")?;
    let snapshot_type = request
        .snapshot_type
        .unwrap_or_else(|| "manual".to_string());
    if !matches!(
        snapshot_type.as_str(),
        "manual" | "automatic" | "pre_restore"
    ) {
        return Err("VALIDATION_ERROR: 不支持的镜像类型".to_string());
    }
    let name = request.name.trim();
    if name.is_empty() || name.chars().count() > 120 {
        return Err("VALIDATION_ERROR: 镜像名称应为 1 到 120 个字符".to_string());
    }
    let value = client(database)?.rpc_for_database_with_timeout(
        database,
        "workspace_snapshot_create",
        json!({
            "p_workspace_id": state.workspace_id,
            "p_device_id": state.device_id,
            "p_operation_id": Uuid::now_v7().to_string(),
            "p_snapshot_type": snapshot_type,
            "p_name": name,
            "p_note": request.note.and_then(|note| {
                let note = note.trim().to_string();
                (!note.is_empty()).then_some(note)
            }),
            "p_lazy": request.lazy.unwrap_or(false),
        }),
        &session,
        SNAPSHOT_TIMEOUT_SECONDS,
    )?;
    serde_json::from_value(value)
        .map_err(|error| format!("CLOUD_RESPONSE_INVALID: 镜像创建响应无效: {error}"))
}

pub fn workspace_snapshot_list(database: &Database) -> Result<Vec<CloudWorkspaceSnapshot>, String> {
    let (state, session) = cloud_workspace_context(database, "云端快照")?;
    let value = client(database)?.rpc_for_database(
        database,
        "workspace_snapshot_list",
        json!({"p_workspace_id": state.workspace_id, "p_device_id": state.device_id}),
        &session,
    )?;
    serde_json::from_value(value)
        .map_err(|error| format!("CLOUD_RESPONSE_INVALID: 镜像列表响应无效: {error}"))
}

pub fn workspace_snapshot_set_protected(
    database: &Database,
    request: CloudWorkspaceSnapshotProtectRequest,
) -> Result<CloudWorkspaceSnapshot, String> {
    let (state, session) = cloud_workspace_context(database, "云端快照")?;
    let value = client(database)?.rpc_for_database(
        database,
        "workspace_snapshot_set_protected",
        json!({
            "p_workspace_id": state.workspace_id,
            "p_device_id": state.device_id,
            "p_snapshot_id": request.snapshot_id,
            "p_protected": request.protected,
        }),
        &session,
    )?;
    serde_json::from_value(value)
        .map_err(|error| format!("CLOUD_RESPONSE_INVALID: 镜像保护响应无效: {error}"))
}

pub fn workspace_snapshot_delete(
    database: &Database,
    request: CloudWorkspaceSnapshotIdRequest,
) -> Result<CloudWorkspaceSnapshotDeleteResult, String> {
    let (state, session) = cloud_workspace_context(database, "云端快照")?;
    let value = client(database)?.rpc_for_database(
        database,
        "workspace_snapshot_delete",
        json!({
            "p_workspace_id": state.workspace_id,
            "p_device_id": state.device_id,
            "p_snapshot_id": request.snapshot_id,
        }),
        &session,
    )?;
    serde_json::from_value(value)
        .map_err(|error| format!("CLOUD_RESPONSE_INVALID: 镜像删除响应无效: {error}"))
}

pub fn workspace_snapshot_preview(
    database: &Database,
    request: CloudWorkspaceSnapshotIdRequest,
) -> Result<CloudWorkspaceSnapshotPreview, String> {
    let (state, session) = cloud_workspace_context(database, "云端快照")?;
    let value = client(database)?.rpc_for_database(
        database,
        "workspace_snapshot_preview",
        json!({
            "p_workspace_id": state.workspace_id,
            "p_device_id": state.device_id,
            "p_snapshot_id": request.snapshot_id,
        }),
        &session,
    )?;
    serde_json::from_value(value)
        .map_err(|error| format!("CLOUD_RESPONSE_INVALID: 快照详情响应无效: {error}"))
}

fn validate_automatic_snapshot_time(value: &str) -> Result<&str, String> {
    let trimmed = value.trim();
    let mut parts = trimmed.split(':');
    let hour = parts.next().and_then(|part| part.parse::<u32>().ok());
    let minute = parts.next().and_then(|part| part.parse::<u32>().ok());
    let valid = trimmed.len() == 5
        && trimmed.as_bytes()[2] == b':'
        && parts.next().is_none()
        && hour.is_some_and(|hour| hour < 24)
        && minute.is_some_and(|minute| minute < 60);
    if valid {
        Ok(trimmed)
    } else {
        Err("VALIDATION_ERROR: 自动快照时间格式必须为 HH:MM".to_string())
    }
}

fn validate_snapshot_retention_days(value: i64) -> Result<i64, String> {
    if (1..=365).contains(&value) {
        Ok(value)
    } else {
        Err("VALIDATION_ERROR: 快照最长保留天数必须在 1 到 365 天之间".to_string())
    }
}

#[cfg(test)]
mod snapshot_setting_tests {
    use super::{validate_automatic_snapshot_time, validate_snapshot_retention_days};

    #[test]
    fn accepts_only_24_hour_clock_times() {
        assert_eq!(validate_automatic_snapshot_time("00:00").unwrap(), "00:00");
        assert_eq!(validate_automatic_snapshot_time("03:00").unwrap(), "03:00");
        assert_eq!(validate_automatic_snapshot_time("23:59").unwrap(), "23:59");
        assert!(validate_automatic_snapshot_time("24:00").is_err());
        assert!(validate_automatic_snapshot_time("03:60").is_err());
        assert!(validate_automatic_snapshot_time("3:00").is_err());
        assert!(validate_automatic_snapshot_time("03:00:00").is_err());
        assert!(validate_automatic_snapshot_time("daily").is_err());
    }

    #[test]
    fn accepts_snapshot_retention_between_one_and_365_days() {
        assert_eq!(validate_snapshot_retention_days(1).unwrap(), 1);
        assert_eq!(validate_snapshot_retention_days(7).unwrap(), 7);
        assert_eq!(validate_snapshot_retention_days(365).unwrap(), 365);
        assert!(validate_snapshot_retention_days(0).is_err());
        assert!(validate_snapshot_retention_days(366).is_err());
    }
}

pub fn workspace_snapshot_settings_get(
    database: &Database,
) -> Result<CloudWorkspaceSnapshotSettings, String> {
    let (state, session) = cloud_workspace_context(database, "云端快照")?;
    let value = client(database)?.rpc_for_database(
        database,
        "workspace_snapshot_settings_get",
        json!({"p_workspace_id": state.workspace_id, "p_device_id": state.device_id}),
        &session,
    )?;
    serde_json::from_value(value)
        .map_err(|error| format!("CLOUD_RESPONSE_INVALID: 快照设置响应无效: {error}"))
}

pub fn workspace_snapshot_settings_update(
    database: &Database,
    request: CloudWorkspaceSnapshotSettingsUpdateRequest,
) -> Result<CloudWorkspaceSnapshotSettings, String> {
    let (state, session) = cloud_workspace_context(database, "云端快照")?;
    let time = validate_automatic_snapshot_time(&request.automatic_snapshot_time)?;
    let retention_days = validate_snapshot_retention_days(request.snapshot_retention_days)?;
    let value = client(database)?.rpc_for_database(
        database,
        "workspace_snapshot_settings_update",
        json!({
            "p_workspace_id": state.workspace_id,
            "p_device_id": state.device_id,
            "p_operation_id": Uuid::now_v7().to_string(),
            "p_automatic_snapshot_enabled": request.automatic_snapshot_enabled,
            "p_automatic_snapshot_time": time,
            "p_snapshot_retention_days": retention_days,
        }),
        &session,
    )?;
    serde_json::from_value(value)
        .map_err(|error| format!("CLOUD_RESPONSE_INVALID: 快照设置保存响应无效: {error}"))
}

fn record_direct_restore_checkpoint(
    database: &Database,
    workspace_id: &str,
    device_id: &str,
    workspace_epoch: i64,
    reset_change_seq: i64,
) -> Result<(), String> {
    write_direct_cloud_checkpoint(
        database,
        &DirectCloudCheckpoint {
            workspace_id: workspace_id.to_string(),
            device_id: device_id.to_string(),
            change_seq: reset_change_seq,
            workspace_epoch,
            fetched_at: None,
            last_error: None,
            auth_blocked_reason: None,
        },
    )
    .map_err(|error| {
        format!("RESTORE_CHECKPOINT_UPDATE_FAILED: 无法保存恢复后的云端世代: {error}")
    })?;
    record_direct_cloud_access_success(workspace_id);
    Ok(())
}

pub fn workspace_restore_prepare(
    database: &Database,
    request: CloudWorkspaceRestorePrepareRequest,
) -> Result<CloudWorkspaceRestorePrepareResult, String> {
    let (state, session) = cloud_workspace_context(database, "云端回滚")?;
    let function = "direct_cloud_workspace_restore_prepare";
    let value = client(database)?.rpc_for_database_with_epoch_and_timeout(database, function, json!({
        "p_workspace_id":state.workspace_id,"p_device_id":state.device_id,"p_target_snapshot_id":request.snapshot_id,"p_expected_epoch":state.workspace_epoch,
        "p_create_safety_snapshot":request.create_safety_snapshot
    }), &session, state.workspace_epoch, RESTORE_TIMEOUT_SECONDS)?;
    serde_json::from_value(value)
        .map_err(|error| format!("CLOUD_RESPONSE_INVALID: 回滚准备响应无效: {error}"))
}

pub fn workspace_restore_commit(
    database: &Database,
    request: CloudWorkspaceRestoreCommitRequest,
) -> Result<CloudWorkspaceRestoreResult, String> {
    crate::cloud_sync::with_cloud_sync_operation_lock(|| {
        workspace_restore_commit_locked(database, request)
    })
}

fn workspace_restore_commit_locked(
    database: &Database,
    request: CloudWorkspaceRestoreCommitRequest,
) -> Result<CloudWorkspaceRestoreResult, String> {
    let (state, session) = cloud_workspace_context(database, "云端回滚")?;
    let function = "direct_cloud_workspace_restore_commit";
    let value = client(database)?.rpc_for_database_with_epoch_and_timeout(database, function, json!({
        "p_workspace_id":state.workspace_id,"p_device_id":state.device_id,"p_lock_token":request.lock_token,
        "p_target_snapshot_id":request.target_snapshot_id,"p_target_content_hash":request.target_content_hash,
        "p_prepared_epoch":request.prepared_epoch,"p_operation_id":Uuid::now_v7().to_string(),"p_confirmed":true
    }), &session, request.prepared_epoch, RESTORE_TIMEOUT_SECONDS)?;
    let mut restored: CloudWorkspaceRestoreResult = serde_json::from_value(value)
        .map_err(|error| format!("CLOUD_RESPONSE_INVALID: 回滚提交响应无效: {error}"))?;
    if !restored.restored {
        return Err("CLOUD_RESPONSE_INVALID: 云端没有确认恢复成功".to_string());
    }
    record_direct_restore_checkpoint(
        database,
        &state.workspace_id,
        &state.device_id,
        restored.workspace_epoch,
        restored.reset_change_seq,
    )?;
    restored.local_cache_rebuilt = false;
    Ok(restored)
}

pub fn workspace_restore_cancel(
    database: &Database,
    request: CloudWorkspaceRestoreCancelRequest,
) -> Result<Value, String> {
    let (state, session) = cloud_workspace_context(database, "云端回滚")?;
    client(database)?.rpc_for_database_with_epoch(database, "workspace_restore_cancel", json!({
        "p_workspace_id":state.workspace_id,"p_device_id":state.device_id,"p_lock_token":request.lock_token
    }), &session, state.workspace_epoch)
}

pub(crate) fn storage_mode_from_connection(
    connection: &rusqlite::Connection,
    online: bool,
) -> Result<StorageModeSnapshot, String> {
    let local_workspace_id: String = connection
        .query_row(
            "SELECT id FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let device_id: String = connection
        .query_row(
            "SELECT id FROM devices WHERE workspace_id = ?1 AND revoked_at IS NULL ORDER BY created_at LIMIT 1",
            [&local_workspace_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let mode: String = connection
        .query_row(
            "SELECT json_extract(value_json, '$') FROM device_settings WHERE key = 'storage_mode'",
            [],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| "local".to_string());
    let storage_protocol = if mode == "cloud" {
        crate::storage_contract::DIRECT_CLOUD_PROTOCOL.to_string()
    } else {
        "local".to_string()
    };
    let workspace_id = if mode == "cloud" {
        connection
            .query_row(
                "SELECT json_extract(value_json, '$') FROM device_settings WHERE key = 'cloud_workspace_id'",
                [],
                |row| row.get(0),
            )
            .map_err(|_| "CLOUD_NOT_CONFIGURED: 尚未初始化 Supabase 工作空间".to_string())?
    } else {
        local_workspace_id.clone()
    };
    let checkpoint = read_direct_cloud_checkpoint(connection, &workspace_id);
    let last_change_seq = checkpoint.change_seq;
    let last_synced_at = checkpoint.fetched_at;
    let last_error = checkpoint.last_error.clone();
    let workspace_epoch = checkpoint.workspace_epoch;
    let cloud_workspace_id: Option<String> = connection
        .query_row(
            "SELECT json_extract(value_json, '$') FROM device_settings WHERE key = 'cloud_workspace_id'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let auth_blocked_reason = cloud_workspace_id.as_deref().and_then(|workspace_id| {
        read_direct_cloud_checkpoint(connection, workspace_id).auth_blocked_reason
    });
    let auth_blocked = auth_blocked_reason.is_some();
    let direct_cloud = mode == "cloud";
    let runtime_access = direct_cloud
        .then(|| direct_cloud_access_runtime(&workspace_id))
        .flatten();
    let (cloud_access_state, cloud_writable, read_only_reason) = if mode == "local" {
        ("local_writable".to_string(), true, None)
    } else if auth_blocked {
        (
            "reauth_required".to_string(),
            false,
            Some(
                auth_blocked_reason
                    .clone()
                    .unwrap_or_else(|| "云端授权已失效，请重新登录".to_string()),
            ),
        )
    } else if !online {
        let state = runtime_access
            .as_ref()
            .map(|runtime| runtime.state)
            .filter(|state| *state == DirectCloudAccessState::ReauthRequired)
            .unwrap_or(DirectCloudAccessState::OfflineReadonly);
        let reason = runtime_access
            .as_ref()
            .and_then(|runtime| runtime.reason.clone())
            .or_else(|| Some("当前无法连接 Supabase，云端数据暂时只读".to_string()));
        (state.as_str().to_string(), state.writable(), reason)
    } else {
        let runtime = runtime_access.unwrap_or(DirectCloudAccessRuntime {
            state: DirectCloudAccessState::Connecting,
            reason: Some("正在连接 Supabase 并加载云端数据".to_string()),
        });
        (
            runtime.state.as_str().to_string(),
            runtime.state.writable(),
            runtime.reason,
        )
    };
    let cache_status = if mode == "cloud" {
        connection
            .path()
            .map(std::path::Path::new)
            .and_then(std::path::Path::parent)
            .and_then(|directory| {
                crate::cloud_read_cache::CloudReadCache::at_data_directory(directory).ok()
            })
            .and_then(|cache| {
                cache
                    .latest_for_workspace(&workspace_id, now_seconds() * 1_000)
                    .ok()
            })
            .flatten()
    } else {
        None
    };
    Ok(StorageModeSnapshot {
        online: mode == "local" || online,
        storage_backend: if mode == "local" {
            "local_sqlite".to_string()
        } else {
            "direct_supabase".to_string()
        },
        mode,
        storage_protocol,
        cloud_access_state,
        cloud_writable,
        read_only_reason,
        workspace_id,
        device_id,
        last_change_seq,
        last_synced_at,
        cache_source: cache_status.as_ref().map(|(source, _)| source.clone()),
        cache_fetched_at: cache_status.map(|(_, fetched_at)| fetched_at),
        last_error,
        auth_blocked,
        auth_blocked_reason,
        workspace_epoch,
    })
}
