use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;
use uuid::Uuid;

use crate::database::Database;

use super::*;

pub fn migration_preview(
    database: &Database,
    request: MigrationPreviewRequest,
) -> Result<MigrationPreview, String> {
    if request.direction != "local_to_cloud" && request.direction != "cloud_to_local_snapshot" {
        return Err("VALIDATION_ERROR: 不支持的迁移方向".to_string());
    }
    let connection = database.open()?;
    let source_workspace_id: String = connection
        .query_row(
            "SELECT id FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    drop(connection);
    let entity_names = crate::data_protection::data_package_copy_entity_names()?;
    let local = local_snapshot(database, &source_workspace_id)?;
    let workspace = workspace_preview(database)?;
    let cloud_counts = if let Some(workspace) = &workspace {
        if request.direction == "cloud_to_local_snapshot" {
            let context = data_copy_target_context(database)?;
            let value = client(database)?.rpc_for_database_with_epoch(
                database,
                "data_copy_export_preview",
                json!({
                    "p_workspace_id": context.workspace_id,
                    "p_device_id": context.device_id,
                    "p_expected_epoch": context.workspace_epoch,
                }),
                &context.session,
                context.workspace_epoch,
            )?;
            let preview: DataCopyExportPreviewResponse = serde_json::from_value(value)
                .map_err(|error| format!("CLOUD_RESPONSE_INVALID: 云端复制预览无效: {error}"))?;
            if preview.workspace_id != workspace.id
                || preview.package_version != crate::data_protection::DATA_PACKAGE_FORMAT_VERSION
                || preview.workspace_epoch != context.workspace_epoch
                || preview.change_seq < 0
            {
                return Err("CLOUD_RESPONSE_INVALID: 云端复制预览元数据不一致".to_string());
            }
            preview.entity_counts
        } else {
            let session = current_session(database)?;
            let snapshot = client(database)?.rpc_for_database(
                database,
                "cloud_snapshot_get",
                json!({ "p_workspace_id": workspace.id.clone() }),
                &session,
            )?;
            entity_names
                .iter()
                .map(|entity| {
                    (
                        entity.clone(),
                        snapshot
                            .get(entity)
                            .and_then(Value::as_array)
                            .map(|rows| rows.len() as i64)
                            .unwrap_or(0),
                    )
                })
                .collect()
        }
    } else {
        HashMap::new()
    };
    let entities = entity_names
        .iter()
        .map(|entity| MigrationEntityCount {
            entity: entity.clone(),
            local_count: local
                .get(entity)
                .and_then(Value::as_array)
                .map(|rows| rows.len() as i64)
                .unwrap_or(0),
            cloud_count: cloud_counts.get(entity).copied(),
        })
        .collect::<Vec<_>>();
    let (
        source_entity_count,
        target_entity_count,
        target_is_empty,
        replace_entity_count,
        safety_snapshot_required,
    ) = migration_preview_impact(&request.direction, &entities);
    let cloud_has_business_data = entities
        .iter()
        .map(|entity| entity.cloud_count.unwrap_or(0))
        .sum::<i64>()
        > 0;
    let conflicts = migration_conflicts(
        &request.direction,
        workspace.is_some(),
        cloud_has_business_data,
    );
    let (preview_source_workspace_id, preview_target_workspace_id) =
        if request.direction == "cloud_to_local_snapshot" {
            (
                workspace
                    .as_ref()
                    .map(|workspace| workspace.id.clone())
                    .unwrap_or_default(),
                Some(source_workspace_id),
            )
        } else {
            (
                source_workspace_id,
                workspace.as_ref().map(|workspace| workspace.id.clone()),
            )
        };
    Ok(MigrationPreview {
        direction: request.direction,
        source_workspace_id: preview_source_workspace_id,
        target_workspace_id: preview_target_workspace_id,
        package_version: crate::data_protection::DATA_PACKAGE_FORMAT_VERSION,
        entities,
        source_entity_count,
        target_entity_count,
        target_is_empty,
        replace_entity_count,
        safety_snapshot_required,
        can_execute: conflicts.is_empty(),
        conflicts,
    })
}

pub(crate) fn migration_preview_impact(
    direction: &str,
    entities: &[MigrationEntityCount],
) -> (i64, i64, bool, i64, bool) {
    let local_entity_count = entities.iter().map(|entity| entity.local_count).sum();
    let cloud_entity_count = entities
        .iter()
        .map(|entity| entity.cloud_count.unwrap_or(0))
        .sum();
    let (source_entity_count, target_entity_count) = if direction == "cloud_to_local_snapshot" {
        (cloud_entity_count, local_entity_count)
    } else {
        (local_entity_count, cloud_entity_count)
    };
    let target_is_empty = target_entity_count == 0;
    (
        source_entity_count,
        target_entity_count,
        target_is_empty,
        target_entity_count,
        direction == "local_to_cloud" && !target_is_empty,
    )
}

pub(crate) fn build_data_copy_chunks(
    snapshot: &Value,
) -> Result<(Vec<DataCopyChunk>, String), String> {
    let mut chunks = Vec::new();
    for entity_name in crate::data_protection::data_package_copy_entity_names()? {
        let rows = snapshot
            .get(&entity_name)
            .and_then(Value::as_array)
            .ok_or_else(|| format!("DATA_PACKAGE_ENTITY_MISSING: {entity_name}"))?;
        let mut pending = Vec::<Value>::new();
        for row in rows {
            let mut candidate = pending.clone();
            candidate.push(row.clone());
            let candidate_value = Value::Array(candidate);
            let candidate_bytes = crate::data_protection::canonical_json_bytes(&candidate_value)?;
            if candidate_bytes.len() > DATA_COPY_CHUNK_MAX_BYTES {
                if pending.is_empty() {
                    return Err(format!("DATA_PACKAGE_ROW_TOO_LARGE: {entity_name}"));
                }
                push_data_copy_chunk(&mut chunks, &entity_name, std::mem::take(&mut pending))?;
                pending.push(row.clone());
            } else {
                pending = candidate_value.as_array().cloned().unwrap_or_default();
            }
        }
        push_data_copy_chunk(&mut chunks, &entity_name, pending)?;
    }
    if chunks.len() > 256 {
        return Err("DATA_PACKAGE_TOO_MANY_CHUNKS: 数据包分块超过 256 个".to_string());
    }
    let hash_chain = chunks
        .iter()
        .map(|chunk| chunk.content_hash.as_str())
        .collect::<String>();
    Ok((
        chunks,
        crate::data_protection::sha256_hex(hash_chain.as_bytes()),
    ))
}

fn push_data_copy_chunk(
    chunks: &mut Vec<DataCopyChunk>,
    entity_name: &str,
    rows: Vec<Value>,
) -> Result<(), String> {
    let value = Value::Array(rows);
    let bytes = crate::data_protection::canonical_json_bytes(&value)?;
    if bytes.len() > DATA_COPY_CHUNK_MAX_BYTES {
        return Err(format!("DATA_PACKAGE_CHUNK_TOO_LARGE: {entity_name}"));
    }
    let content_text = String::from_utf8(bytes.clone())
        .map_err(|error| format!("DATA_PACKAGE_ENCODING_INVALID: {error}"))?;
    chunks.push(DataCopyChunk {
        chunk_index: chunks.len(),
        entity_name: entity_name.to_string(),
        content_text,
        content_hash: crate::data_protection::sha256_hex(&bytes),
        row_count: value.as_array().map(Vec::len).unwrap_or(0),
    });
    Ok(())
}

pub fn stage_local_data_copy(database: &Database) -> Result<DataCopyStageResult, String> {
    let context = data_copy_target_context(database)?;
    let connection = database.open()?;
    let source_workspace_id: String = connection
        .query_row(
            "SELECT id FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    drop(connection);

    let snapshot = local_snapshot(database, &source_workspace_id)?;
    let (chunks, content_hash) = build_data_copy_chunks(&snapshot)?;
    let api = client(database)?;
    let begin = api.rpc_for_database(
        database,
        "data_copy_stage_begin",
        json!({
            "p_workspace_id": context.workspace_id,
            "p_device_id": context.device_id,
            "p_expected_epoch": context.workspace_epoch,
            "p_package_version": crate::data_protection::DATA_PACKAGE_FORMAT_VERSION,
            "p_expected_chunk_count": chunks.len(),
            "p_expected_content_hash": content_hash,
        }),
        &context.session,
    )?;
    let package_id = begin
        .get("packageId")
        .and_then(Value::as_str)
        .ok_or_else(|| "CLOUD_RESPONSE_INVALID: staging begin 缺少 packageId".to_string())?
        .to_string();

    let upload_result = (|| {
        for chunk in &chunks {
            api.rpc_for_database(
                database,
                "data_copy_stage_chunk_put",
                json!({
                    "p_workspace_id": context.workspace_id,
                    "p_device_id": context.device_id,
                    "p_expected_epoch": context.workspace_epoch,
                    "p_package_id": package_id,
                    "p_chunk_index": chunk.chunk_index,
                    "p_entity_name": chunk.entity_name,
                    "p_content_text": chunk.content_text,
                    "p_content_hash": chunk.content_hash,
                    "p_row_count": chunk.row_count,
                }),
                &context.session,
            )?;
        }
        api.rpc_for_database(
            database,
            "data_copy_stage_validate",
            json!({
                "p_workspace_id": context.workspace_id,
                "p_device_id": context.device_id,
                "p_expected_epoch": context.workspace_epoch,
                "p_package_id": package_id,
            }),
            &context.session,
        )
    })();
    let validated = match upload_result {
        Ok(value) => value,
        Err(error) => {
            let _ = api.rpc_for_database(
                database,
                "data_copy_stage_discard",
                json!({
                    "p_workspace_id": context.workspace_id,
                    "p_device_id": context.device_id,
                    "p_expected_epoch": context.workspace_epoch,
                    "p_package_id": package_id,
                }),
                &context.session,
            );
            return Err(error);
        }
    };
    let entity_counts = serde_json::from_value(
        validated
            .get("entityCounts")
            .cloned()
            .unwrap_or_else(|| json!({})),
    )
    .map_err(|error| format!("CLOUD_RESPONSE_INVALID: staging entityCounts 无效: {error}"))?;
    Ok(DataCopyStageResult {
        package_id,
        status: validated
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("validated")
            .to_string(),
        entity_counts,
        content_hash,
        chunk_count: chunks.len(),
    })
}

pub fn replace_staged_local_data_copy(
    database: &Database,
    request: DataCopyReplaceRequest,
) -> Result<DataCopyReplaceResult, String> {
    crate::cloud_sync::with_cloud_sync_operation_lock(|| {
        replace_staged_local_data_copy_locked(database, request)
    })
}

fn replace_staged_local_data_copy_locked(
    database: &Database,
    request: DataCopyReplaceRequest,
) -> Result<DataCopyReplaceResult, String> {
    if !request.confirmed {
        return Err(
            "DATA_COPY_REPLACE_CONFIRMATION_REQUIRED: 完整替换云端数据必须明确确认".to_string(),
        );
    }
    let package_id = Uuid::parse_str(request.package_id.trim())
        .map_err(|_| "VALIDATION_ERROR: packageId 必须是 UUID".to_string())?;
    let operation_id = Uuid::parse_str(request.operation_id.trim())
        .map_err(|_| "VALIDATION_ERROR: operationId 必须是 UUID".to_string())?;
    let content_hash = request.content_hash.trim().to_ascii_lowercase();
    if content_hash.len() != 64 || !content_hash.bytes().all(|value| value.is_ascii_hexdigit()) {
        return Err("VALIDATION_ERROR: contentHash 必须是 SHA-256 十六进制值".to_string());
    }

    let context = data_copy_target_context(database)?;
    let body = json!({
        "p_workspace_id": context.workspace_id,
        "p_device_id": context.device_id,
        "p_expected_epoch": context.workspace_epoch,
        "p_package_id": package_id,
        "p_expected_content_hash": content_hash,
        "p_operation_id": operation_id,
        "p_confirmed": true,
    });
    let api = client(database)?;
    let value = match api.rpc_for_database_with_epoch_and_timeout(
        database,
        "data_copy_replace_commit",
        body.clone(),
        &context.session,
        context.workspace_epoch,
        RESTORE_TIMEOUT_SECONDS,
    ) {
        Ok(value) => value,
        Err(error)
            if error.starts_with("NETWORK_ERROR:")
                || error.starts_with("CLOUD_TIMEOUT:")
                || error.starts_with("CLOUD_SERVICE_UNAVAILABLE:") =>
        {
            api.rpc_for_database_with_epoch_and_timeout(
                database,
                "data_copy_replace_commit",
                body,
                &context.session,
                context.workspace_epoch,
                RESTORE_TIMEOUT_SECONDS,
            )
            .map_err(|retry_error| {
                record_direct_cloud_access_error(&context.workspace_id, &retry_error);
                retry_error
            })?
        }
        Err(error) => {
            record_direct_cloud_access_error(&context.workspace_id, &error);
            return Err(error);
        }
    };
    let replaced: DataCopyReplaceResult = serde_json::from_value(value).map_err(|error| {
        let error = format!("CLOUD_RESPONSE_INVALID: data_copy_replace_commit 响应无效: {error}");
        record_direct_cloud_access_error(&context.workspace_id, &error);
        error
    })?;
    if !replaced.replaced
        || replaced.package_id != package_id.to_string()
        || replaced.content_hash != content_hash
        || replaced.previous_epoch != context.workspace_epoch
        || replaced.workspace_epoch <= replaced.previous_epoch
    {
        let error = "CLOUD_RESPONSE_INVALID: 云端没有返回一致的数据复制替换结果".to_string();
        record_direct_cloud_access_error(&context.workspace_id, &error);
        return Err(error);
    }
    record_data_copy_replace_checkpoint(database, &context, &replaced)?;
    record_direct_cloud_access_success(&context.workspace_id);
    Ok(replaced)
}

fn record_data_copy_replace_checkpoint(
    database: &Database,
    context: &DirectCloudReadContext,
    result: &DataCopyReplaceResult,
) -> Result<(), String> {
    write_direct_cloud_checkpoint(
        database,
        &DirectCloudCheckpoint {
            workspace_id: context.workspace_id.clone(),
            device_id: context.device_id.clone(),
            change_seq: result.reset_change_seq,
            workspace_epoch: result.workspace_epoch,
            fetched_at: None,
            last_error: None,
            auth_blocked_reason: None,
        },
    )?;
    DIRECT_TRACKING_LEASE_TOKENS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(&direct_tracking_lease_key(context));
    Ok(())
}

pub fn begin_cloud_data_copy_export(database: &Database) -> Result<DataCopyExportManifest, String> {
    let context = data_copy_target_context(database)?;
    let value = client(database)?.rpc_for_database_with_epoch_and_timeout(
        database,
        "data_copy_export_begin",
        json!({
            "p_workspace_id": context.workspace_id,
            "p_device_id": context.device_id,
            "p_expected_epoch": context.workspace_epoch,
        }),
        &context.session,
        context.workspace_epoch,
        SNAPSHOT_TIMEOUT_SECONDS,
    )?;
    let manifest: DataCopyExportManifest = serde_json::from_value(value)
        .map_err(|error| format!("CLOUD_RESPONSE_INVALID: 云端导出清单无效: {error}"))?;
    validate_data_copy_export_manifest(&manifest, context.workspace_epoch)?;
    Ok(manifest)
}

pub fn get_cloud_data_copy_export_chunk(
    database: &Database,
    request: DataCopyExportChunkRequest,
) -> Result<DataCopyExportChunk, String> {
    let package_id = Uuid::parse_str(request.package_id.trim())
        .map_err(|_| "VALIDATION_ERROR: packageId 必须是 UUID".to_string())?;
    if request.chunk_index >= 256 {
        return Err("VALIDATION_ERROR: chunkIndex 超出范围".to_string());
    }
    let context = data_copy_target_context(database)?;
    let value = client(database)?.rpc_for_database_with_epoch(
        database,
        "data_copy_export_chunk_get",
        json!({
            "p_workspace_id": context.workspace_id,
            "p_device_id": context.device_id,
            "p_package_id": package_id,
            "p_chunk_index": request.chunk_index,
        }),
        &context.session,
        context.workspace_epoch,
    )?;
    let chunk: DataCopyExportChunk = serde_json::from_value(value)
        .map_err(|error| format!("CLOUD_RESPONSE_INVALID: 云端导出分块无效: {error}"))?;
    validate_data_copy_export_chunk(&chunk, &package_id.to_string(), request.chunk_index)?;
    Ok(chunk)
}

pub fn discard_cloud_data_copy_export(
    database: &Database,
    request: DataCopyExportDiscardRequest,
) -> Result<Value, String> {
    let package_id = Uuid::parse_str(request.package_id.trim())
        .map_err(|_| "VALIDATION_ERROR: packageId 必须是 UUID".to_string())?;
    let context = data_copy_target_context(database)?;
    client(database)?.rpc_for_database_with_epoch(
        database,
        "data_copy_export_discard",
        json!({
            "p_workspace_id": context.workspace_id,
            "p_device_id": context.device_id,
            "p_package_id": package_id,
        }),
        &context.session,
        context.workspace_epoch,
    )
}

pub fn replace_local_with_cloud_data_copy(
    database: &Database,
    request: DataCopyReplaceLocalRequest,
) -> Result<DataCopyReplaceLocalResult, String> {
    crate::cloud_sync::with_cloud_sync_operation_lock(|| {
        replace_local_with_cloud_data_copy_locked(database, request)
    })
}

fn replace_local_with_cloud_data_copy_locked(
    database: &Database,
    request: DataCopyReplaceLocalRequest,
) -> Result<DataCopyReplaceLocalResult, String> {
    if !request.confirmed {
        return Err(
            "DATA_COPY_REPLACE_CONFIRMATION_REQUIRED: 完整替换本地数据必须明确确认".to_string(),
        );
    }
    let manifest = begin_cloud_data_copy_export(database)?;
    let download_result = download_cloud_data_copy(database, &manifest);
    let _ = discard_cloud_data_copy_export(
        database,
        DataCopyExportDiscardRequest {
            package_id: manifest.package_id.clone(),
        },
    );
    let snapshot = download_result?;
    replace_local_with_downloaded_data_copy(database, &manifest, snapshot)
}

pub(crate) fn replace_local_with_downloaded_data_copy(
    database: &Database,
    manifest: &DataCopyExportManifest,
    snapshot: Value,
) -> Result<DataCopyReplaceLocalResult, String> {
    let safety_directory = database.data_directory().join("backups");
    let safety_path = safety_directory.join(format!(
        "timegenie-before-cloud-copy-{}-{}.timegenie-backup",
        now_seconds(),
        Uuid::now_v7()
    ));
    let safety_backup = crate::data_protection::export_automatic_safety_backup(
        database,
        safety_path.display().to_string(),
    )?;

    let workspace_id =
        crate::data_protection::replace_local_business_data_from_copy(database, &snapshot)?;
    Ok(DataCopyReplaceLocalResult {
        replaced: true,
        package_id: manifest.package_id.clone(),
        content_hash: manifest.content_hash.clone(),
        safety_backup_path: safety_backup.path,
        workspace_id,
        entity_counts: manifest.entity_counts.clone(),
    })
}

fn download_cloud_data_copy(
    database: &Database,
    manifest: &DataCopyExportManifest,
) -> Result<Value, String> {
    download_data_copy_from_fetch(manifest, |chunk_index| {
        get_cloud_data_copy_export_chunk(
            database,
            DataCopyExportChunkRequest {
                package_id: manifest.package_id.clone(),
                chunk_index,
            },
        )
    })
}

pub(crate) fn download_data_copy_from_fetch<F>(
    manifest: &DataCopyExportManifest,
    mut fetch: F,
) -> Result<Value, String>
where
    F: FnMut(usize) -> Result<DataCopyExportChunk, String>,
{
    let entity_names = crate::data_protection::data_package_copy_entity_names()?;
    let mut snapshot = entity_names
        .iter()
        .map(|entity| (entity.clone(), Value::Array(Vec::new())))
        .collect::<serde_json::Map<_, _>>();
    let mut hash_chain = String::with_capacity(manifest.chunk_count * 64);
    for chunk_index in 0..manifest.chunk_count {
        let chunk = fetch(chunk_index)?;
        validate_data_copy_export_chunk(&chunk, &manifest.package_id, chunk_index)?;
        let rows: Vec<Value> = serde_json::from_str(&chunk.content_text).map_err(|_| {
            "DATA_PACKAGE_CHUNK_JSON_INVALID: 云端导出分块不是有效 JSON".to_string()
        })?;
        snapshot
            .get_mut(&chunk.entity_name)
            .and_then(Value::as_array_mut)
            .ok_or_else(|| {
                format!(
                    "DATA_PACKAGE_ENTITY_MISSING: 云端导出包含未知实体 {}",
                    chunk.entity_name
                )
            })?
            .extend(rows);
        hash_chain.push_str(&chunk.content_hash);
    }
    if crate::data_protection::sha256_hex(hash_chain.as_bytes()) != manifest.content_hash {
        return Err("DATA_PACKAGE_CONTENT_HASH_MISMATCH: 云端导出整包哈希校验失败".to_string());
    }
    for entity_name in entity_names {
        let actual_count = snapshot
            .get(&entity_name)
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or_default() as i64;
        if manifest.entity_counts.get(&entity_name).copied() != Some(actual_count) {
            return Err(format!(
                "DATA_PACKAGE_ENTITY_COUNT_MISMATCH: {entity_name} 实体数量与清单不一致"
            ));
        }
    }
    Ok(Value::Object(snapshot))
}

pub(crate) fn validate_data_copy_export_manifest(
    manifest: &DataCopyExportManifest,
    expected_epoch: i64,
) -> Result<(), String> {
    Uuid::parse_str(&manifest.package_id)
        .map_err(|_| "CLOUD_RESPONSE_INVALID: 云端导出 packageId 无效".to_string())?;
    if manifest.package_version != crate::data_protection::DATA_PACKAGE_FORMAT_VERSION
        || manifest.workspace_epoch != expected_epoch
        || manifest.change_seq < 0
        || !(22..=256).contains(&manifest.chunk_count)
        || manifest.content_hash.len() != 64
        || !manifest
            .content_hash
            .bytes()
            .all(|value| value.is_ascii_hexdigit())
    {
        return Err("CLOUD_RESPONSE_INVALID: 云端导出清单元数据不一致".to_string());
    }
    let expected_entities = crate::data_protection::data_package_copy_entity_names()?;
    if manifest.entity_counts.len() != expected_entities.len()
        || expected_entities.iter().any(|entity| {
            manifest
                .entity_counts
                .get(entity)
                .is_none_or(|count| *count < 0)
        })
    {
        return Err("CLOUD_RESPONSE_INVALID: 云端导出实体目录不完整".to_string());
    }
    Ok(())
}

pub(crate) fn validate_data_copy_export_chunk(
    chunk: &DataCopyExportChunk,
    package_id: &str,
    chunk_index: usize,
) -> Result<(), String> {
    if chunk.package_id != package_id
        || chunk.chunk_index != chunk_index
        || !crate::data_protection::data_package_copy_entity_names()?.contains(&chunk.entity_name)
        || chunk.content_text.as_bytes().len() > DATA_COPY_CHUNK_MAX_BYTES
        || crate::data_protection::sha256_hex(chunk.content_text.as_bytes()) != chunk.content_hash
    {
        return Err("DATA_PACKAGE_CHUNK_HASH_MISMATCH: 云端导出分块校验失败".to_string());
    }
    let rows: Value = serde_json::from_str(&chunk.content_text)
        .map_err(|_| "DATA_PACKAGE_CHUNK_JSON_INVALID: 云端导出分块不是有效 JSON".to_string())?;
    if rows.as_array().map(Vec::len) != Some(chunk.row_count) {
        return Err("DATA_PACKAGE_ROW_COUNT_MISMATCH: 云端导出分块行数不一致".to_string());
    }
    Ok(())
}

pub(crate) fn migration_conflicts(
    direction: &str,
    cloud_workspace_exists: bool,
    cloud_has_business_data: bool,
) -> Vec<String> {
    let mut conflicts = Vec::new();
    if direction == "cloud_to_local_snapshot" && !cloud_workspace_exists {
        conflicts
            .push("当前账号还没有可使用的云端工作空间，不能用空云端快照覆盖本机数据".to_string());
    }
    if direction == "cloud_to_local_snapshot" && cloud_workspace_exists && !cloud_has_business_data
    {
        conflicts.push(
            "云端工作空间还没有事项、计时或报告数据，不能用空云端快照覆盖本机数据".to_string(),
        );
    }
    conflicts
}
