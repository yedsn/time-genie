use std::collections::HashMap;

use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::database::Database;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UnassignedStateDto {
    pub session_id: String,
    pub work_date: String,
    pub state: String,
    pub first_started_at: i64,
    pub last_ended_at: Option<i64>,
    pub current_segment_started_at: Option<i64>,
    pub elapsed_seconds: i64,
    pub required_minutes: i64,
    pub threshold_seconds: i64,
    pub must_resolve: bool,
    pub version: i64,
}

const CONTINUITY_GRACE_MILLIS: i64 = 90_000;

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UnassignedAllocationInput {
    pub task_id: String,
    pub minutes: i64,
    #[serde(default)]
    pub complete_task: bool,
    pub task_expected_version: Option<i64>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ResolveWorkRequest {
    pub session_id: String,
    pub allocations: Vec<UnassignedAllocationInput>,
    pub expected_version: i64,
    pub operation_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveSessionRequest {
    pub session_id: String,
    pub expected_version: i64,
    pub operation_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UnassignedResolveResult {
    pub session_id: String,
    pub generated_entry_id: Option<String>,
    pub resolution_type: String,
    pub elapsed_seconds: i64,
    pub required_minutes: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UnassignedStateSnapshotDto {
    pub current: Option<UnassignedStateDto>,
    pub historical_pending: Vec<UnassignedStateDto>,
    pub historical_pending_count: i64,
    pub earliest_historical_date: Option<String>,
}

pub fn get_state(database: &Database) -> Result<Option<UnassignedStateDto>, String> {
    get_state_with_cloud_context(database, None)
}

#[cfg(test)]
pub fn get_state_snapshot(database: &Database) -> Result<UnassignedStateSnapshotDto, String> {
    let current = get_state(database)?;
    let connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    load_state_snapshot(&connection, &workspace_id, current, now_millis())
}

fn load_state_snapshot(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    current: Option<UnassignedStateDto>,
    now: i64,
) -> Result<UnassignedStateSnapshotDto, String> {
    let current_date = crate::work_calendar::current_work_date(connection, now)?;
    let ids = connection
        .prepare(
            "SELECT id FROM unassigned_sessions
             WHERE workspace_id=?1 AND work_date<?2 AND state IN ('collecting','awaiting_resolution')
             ORDER BY work_date,id",
        )
        .map_err(|error| error.to_string())?
        .query_map(params![workspace_id, current_date], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let historical_pending = ids
        .iter()
        .map(|id| load_state(connection, id, now, false))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(UnassignedStateSnapshotDto {
        historical_pending_count: historical_pending.len() as i64,
        earliest_historical_date: historical_pending
            .first()
            .map(|item| item.work_date.clone()),
        current,
        historical_pending,
    })
}

pub fn get_session_state(
    database: &Database,
    session_id: &str,
) -> Result<UnassignedStateDto, String> {
    let connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM unassigned_sessions WHERE id=?1 AND workspace_id=?2 AND state IN ('collecting','awaiting_resolution'))",
            params![session_id, workspace_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if !exists {
        return Err("NOT_FOUND: 未归属会话不存在或已处理".to_string());
    }
    load_state(&connection, session_id, now_millis(), false)
}

pub(crate) fn get_state_with_cloud_context(
    database: &Database,
    cloud_state: Option<&crate::supabase::StorageModeSnapshot>,
) -> Result<Option<UnassignedStateDto>, String> {
    Ok(get_state_with_cloud_context_and_sync_change(database, cloud_state)?.0)
}

pub(crate) fn get_state_with_cloud_context_and_sync_change(
    database: &Database,
    cloud_state: Option<&crate::supabase::StorageModeSnapshot>,
) -> Result<(Option<UnassignedStateDto>, bool), String> {
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let now = now_millis();
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let current_date = crate::work_calendar::current_work_date(&transaction, now)?;
    let session_before_coordinate = active_session_id(&transaction, &workspace_id, &current_date)?;
    let version_before_coordinate = session_before_coordinate
        .as_deref()
        .map(|id| session_version(&transaction, id))
        .transpose()?;
    coordinate_unassigned_sessions(
        &transaction,
        &workspace_id,
        now,
        if cloud_state.is_some() {
            "cloud"
        } else {
            "local"
        },
    )?;
    let active_timer = has_active_timer(&transaction, &workspace_id)?;
    let session_id = active_session_id(&transaction, &workspace_id, &current_date)?;
    let existing_version = version_before_coordinate;
    let mut created = session_before_coordinate.is_none() && session_id.is_some();
    let session_id = match (session_id, active_timer) {
        (Some(id), _) => Some(id),
        (None, false) => {
            created = true;
            Some(create_session(
                &transaction,
                &workspace_id,
                now,
                if cloud_state.is_some() {
                    "cloud"
                } else {
                    "local"
                },
                None,
            )?)
        }
        (None, true) => None,
    };
    let result = session_id
        .map(|id| load_state(&transaction, &id, now, true))
        .transpose()?;
    let mut sync_changed = false;
    if let (Some(state), Some(result)) = (cloud_state, result.as_ref()) {
        let migrated_candidate: bool = transaction
            .query_row(
                "SELECT shared_source <> 'cloud' OR migration_state = 'candidate'
                 FROM unassigned_sessions WHERE id=?1",
                [&result.session_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if migrated_candidate {
            transaction
                .execute(
                    "UPDATE unassigned_sessions
                     SET shared_source='cloud', migration_state='candidate'
                     WHERE id=?1",
                    [&result.session_id],
                )
                .map_err(|error| error.to_string())?;
        }
        let candidate_already_queued: bool = migrated_candidate
            && transaction
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sync_outbox
                     WHERE workspace_id=?1 AND entity_type='unassigned_session'
                       AND entity_id=?2 AND state IN ('pending','sending','failed'))",
                    params![state.workspace_id, result.session_id],
                    |row| row.get(0),
                )
                .map_err(|error| error.to_string())?;
        if created
            || (migrated_candidate && !candidate_already_queued)
            || (!migrated_candidate && existing_version != Some(result.version))
        {
            sync_changed = crate::cloud_sync::enqueue_entity_in_transaction(
                &transaction,
                state,
                if created || migrated_candidate {
                    "unassigned_session_create"
                } else {
                    "unassigned_session_awaiting_resolution"
                },
                "unassigned_session",
                Some(&result.session_id),
                if created || migrated_candidate {
                    None
                } else {
                    existing_version
                },
                None,
            )?
            .is_some();
        }
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok((result, sync_changed))
}

pub(crate) fn coordinate_sessions(database: &Database, now: i64) -> Result<(), String> {
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    coordinate_unassigned_sessions(&transaction, &workspace_id, now, "local")?;
    transaction.commit().map_err(|error| error.to_string())
}

#[cfg(test)]
pub fn resolve_work(
    database: &Database,
    request: ResolveWorkRequest,
) -> Result<UnassignedResolveResult, String> {
    resolve_work_with_cloud_operation(database, request, None)
}

fn resolve_work_with_cloud_operation(
    database: &Database,
    request: ResolveWorkRequest,
    cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str, Option<&str>)>,
) -> Result<UnassignedResolveResult, String> {
    validate_operation_id(&request.operation_id)?;
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let device_id = device_id(&connection, &workspace_id)?;
    if let Some(result) = processed_result::<UnassignedResolveResult>(
        &connection,
        &workspace_id,
        &request.operation_id,
    )? {
        return Ok(result);
    }
    let now = now_millis();
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let state = seal_session(
        &transaction,
        &workspace_id,
        &request.session_id,
        request.expected_version,
        now,
    )?;
    let mut merged = HashMap::<String, (i64, bool, Option<i64>)>::new();
    for allocation in request.allocations {
        if allocation.minutes < 0 {
            return Err("VALIDATION_ERROR: 分配分钟不能为负数".to_string());
        }
        if allocation.minutes == 0 {
            continue;
        }
        ensure_selectable_task(&transaction, &workspace_id, &allocation.task_id)?;
        let item = merged.entry(allocation.task_id).or_insert((
            0,
            false,
            allocation.task_expected_version,
        ));
        item.0 += allocation.minutes;
        item.1 |= allocation.complete_task;
        if let (Some(existing), Some(candidate)) = (item.2, allocation.task_expected_version) {
            if existing != candidate {
                return Err("VERSION_CONFLICT: 同一事项的版本信息不一致".to_string());
            }
        } else if item.2.is_none() {
            item.2 = allocation.task_expected_version;
        }
    }
    let total: i64 = merged.values().map(|(minutes, _, _)| *minutes).sum();
    if total <= 0 {
        return Err("VALIDATION_ERROR: 请至少分配 1 分钟到事项".to_string());
    }
    if total > state.required_minutes {
        return Err(format!(
            "ALLOCATION_EXCEEDS_DURATION: 分配时间超出未归属时间 {} 分钟",
            total - state.required_minutes
        ));
    }

    let entry_id = Uuid::now_v7().to_string();
    let completed_task_outbox = merged
        .iter()
        .filter_map(|(task_id, (_, complete_task, expected_version))| {
            complete_task
                .then_some(*expected_version)
                .flatten()
                .map(|version| (task_id.clone(), version))
        })
        .collect::<Vec<_>>();
    let label = if merged.len() == 1 {
        let task_id = merged.keys().next().expect("single allocation");
        task_title(&transaction, task_id)?
    } else {
        format!("未归属时间分配到 {} 个事项", merged.len())
    };
    insert_generated_entry(
        &transaction,
        &workspace_id,
        &device_id,
        &entry_id,
        &state,
        "work",
        &label,
        now,
    )?;
    for (task_id, (_, complete_task, expected_version)) in &merged {
        if *complete_task {
            crate::time_tracking::validate_task_completion(
                &transaction,
                &workspace_id,
                task_id,
                *expected_version,
                &state.work_date,
            )?;
        }
    }
    let mut completed_occurrence_outbox = Vec::<(String, Option<i64>)>::new();
    for (task_id, (minutes, complete_task, expected_version)) in merged {
        let work_date = state.work_date.clone();
        crate::recurring::ensure_occurrence_if_recurring(
            &transaction,
            &workspace_id,
            &task_id,
            &work_date,
            now,
        )?;
        transaction
            .execute(
                "INSERT INTO time_allocations(id, workspace_id, entry_id, task_id, minutes, created_at, updated_at, version)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, 1)",
                params![Uuid::now_v7().to_string(), workspace_id, entry_id, task_id, minutes, now],
            )
            .map_err(|error| error.to_string())?;
        if complete_task {
            if let Some(occurrence_outbox) = crate::time_tracking::complete_task_if_open(
                &transaction,
                &workspace_id,
                &task_id,
                expected_version,
                &work_date,
                now,
            )? {
                completed_occurrence_outbox.push(occurrence_outbox);
            }
        }
    }
    finish_session(
        &transaction,
        &request.session_id,
        request.expected_version + 1,
        "resolved",
        "work",
        Some(&entry_id),
        now,
    )?;
    transaction
        .execute(
            "UPDATE unassigned_sessions SET resolution_operation_id=?1 WHERE id=?2",
            params![request.operation_id, request.session_id],
        )
        .map_err(|error| error.to_string())?;
    let result = UnassignedResolveResult {
        session_id: request.session_id.clone(),
        generated_entry_id: Some(entry_id),
        resolution_type: "work".to_string(),
        elapsed_seconds: state.elapsed_seconds,
        required_minutes: state.required_minutes,
    };
    save_processed_result(
        &transaction,
        &workspace_id,
        &device_id,
        &request.operation_id,
        "unassigned_resolve_work",
        &result,
    )?;
    start_next_session_if_idle(
        &transaction,
        &workspace_id,
        now,
        cloud_operation.is_some(),
        Some(&request.session_id),
    )?;
    bump_revision(&transaction)?;
    if let Some((state, operation_type, operation_id)) = cloud_operation {
        let session_operation_id = crate::cloud_sync::enqueue_entity_in_transaction(
            &transaction,
            state,
            operation_type,
            "unassigned_session",
            Some(&result.session_id),
            Some(request.expected_version),
            operation_id,
        )?;
        let generated_entry_operation_id =
            if let Some(entry_id) = result.generated_entry_id.as_deref() {
                crate::cloud_sync::enqueue_entity_in_transaction_with_dependency(
                    &transaction,
                    state,
                    "unassigned_time_entry_create",
                    "time_entry",
                    Some(entry_id),
                    None,
                    None,
                    session_operation_id.as_deref(),
                )?
            } else {
                None
            };
        for (task_id, base_version) in completed_task_outbox {
            crate::cloud_sync::enqueue_entity_in_transaction_with_dependency(
                &transaction,
                state,
                "task_set_completed_from_unassigned",
                "task",
                Some(&task_id),
                Some(base_version),
                None,
                generated_entry_operation_id.as_deref(),
            )?;
        }
        for (entity_id, base_version) in completed_occurrence_outbox {
            crate::cloud_sync::enqueue_entity_in_transaction_with_dependency(
                &transaction,
                state,
                "task_occurrence_set_completed_from_unassigned",
                "task_occurrence",
                Some(&entity_id),
                base_version,
                None,
                generated_entry_operation_id.as_deref(),
            )?;
        }
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

fn resolve_work_with_hooks_and_cloud_operation(
    database: &Database,
    request: ResolveWorkRequest,
    cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str, Option<&str>)>,
) -> Result<UnassignedResolveResult, String> {
    let connection = database.open()?;
    let work_date: String = connection
        .query_row(
            "SELECT work_date FROM unassigned_sessions WHERE id=?1",
            [&request.session_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let operation_id = request.operation_id.clone();
    let candidates = request
        .allocations
        .iter()
        .filter(|allocation| allocation.complete_task)
        .map(|allocation| {
            crate::tasks::task_is_completed(database, &allocation.task_id, Some(&work_date))
                .map(|completed| (allocation.task_id.clone(), completed))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let result = resolve_work_with_cloud_operation(database, request, cloud_operation)?;
    for (task_id, was_completed) in candidates {
        if !was_completed && crate::tasks::task_is_completed(database, &task_id, Some(&work_date))?
        {
            crate::tasks::publish_task_completed_hook(
                database,
                &task_id,
                Some(&work_date),
                Some(&operation_id),
            );
        }
    }
    Ok(result)
}

pub fn resolve_work_for_sync(
    database: &Database,
    request: ResolveWorkRequest,
) -> Result<UnassignedResolveResult, String> {
    let operation_id = request.operation_id.clone();
    let state = crate::supabase::storage_mode(database)?;
    let result = resolve_work_with_hooks_and_cloud_operation(
        database,
        request,
        Some((
            &state,
            "unassigned_resolve_work",
            Some(operation_id.as_str()),
        )),
    )?;
    crate::cloud_sync::flush_if_online(database)?;
    Ok(result)
}

#[cfg(test)]
pub fn discard(
    database: &Database,
    request: ResolveSessionRequest,
) -> Result<UnassignedResolveResult, String> {
    resolve_without_allocations(database, request, "discard", None)
}

fn resolve_without_allocations(
    database: &Database,
    request: ResolveSessionRequest,
    resolution_type: &str,
    cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str, Option<&str>)>,
) -> Result<UnassignedResolveResult, String> {
    validate_operation_id(&request.operation_id)?;
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let device_id = device_id(&connection, &workspace_id)?;
    if let Some(result) = processed_result::<UnassignedResolveResult>(
        &connection,
        &workspace_id,
        &request.operation_id,
    )? {
        return Ok(result);
    }
    let now = now_millis();
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let state = seal_session(
        &transaction,
        &workspace_id,
        &request.session_id,
        request.expected_version,
        now,
    )?;
    let entry_id = if resolution_type == "break" || resolution_type == "discard" {
        let id = Uuid::now_v7().to_string();
        insert_generated_entry(
            &transaction,
            &workspace_id,
            &device_id,
            &id,
            &state,
            "break",
            if resolution_type == "break" {
                "休息时间"
            } else {
                "无效时间"
            },
            now,
        )?;
        Some(id)
    } else {
        None
    };
    let final_state = if resolution_type == "discard" {
        "discarded"
    } else {
        "resolved"
    };
    finish_session(
        &transaction,
        &request.session_id,
        request.expected_version + 1,
        final_state,
        resolution_type,
        entry_id.as_deref(),
        now,
    )?;
    transaction
        .execute(
            "UPDATE unassigned_sessions SET resolution_operation_id=?1 WHERE id=?2",
            params![request.operation_id, request.session_id],
        )
        .map_err(|error| error.to_string())?;
    let result = UnassignedResolveResult {
        session_id: request.session_id.clone(),
        generated_entry_id: entry_id,
        resolution_type: resolution_type.to_string(),
        elapsed_seconds: state.elapsed_seconds,
        required_minutes: state.required_minutes,
    };
    save_processed_result(
        &transaction,
        &workspace_id,
        &device_id,
        &request.operation_id,
        if resolution_type == "break" {
            "unassigned_resolve_break"
        } else {
            "unassigned_discard"
        },
        &result,
    )?;
    start_next_session_if_idle(
        &transaction,
        &workspace_id,
        now,
        cloud_operation.is_some(),
        Some(&request.session_id),
    )?;
    bump_revision(&transaction)?;
    if let Some((state, operation_type, operation_id)) = cloud_operation {
        let session_operation_id = crate::cloud_sync::enqueue_entity_in_transaction(
            &transaction,
            state,
            operation_type,
            "unassigned_session",
            Some(&result.session_id),
            Some(request.expected_version),
            operation_id,
        )?;
        if let Some(entry_id) = result.generated_entry_id.as_deref() {
            crate::cloud_sync::enqueue_entity_in_transaction_with_dependency(
                &transaction,
                state,
                "unassigned_time_entry_create",
                "time_entry",
                Some(entry_id),
                None,
                None,
                session_operation_id.as_deref(),
            )?;
        }
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

pub fn resolve_break_for_sync(
    database: &Database,
    request: ResolveSessionRequest,
) -> Result<UnassignedResolveResult, String> {
    let operation_id = request.operation_id.clone();
    let state = crate::supabase::storage_mode(database)?;
    let result = resolve_without_allocations(
        database,
        request,
        "break",
        Some((
            &state,
            "unassigned_resolve_break",
            Some(operation_id.as_str()),
        )),
    )?;
    crate::cloud_sync::flush_if_online(database)?;
    Ok(result)
}

pub fn discard_for_sync(
    database: &Database,
    request: ResolveSessionRequest,
) -> Result<UnassignedResolveResult, String> {
    let operation_id = request.operation_id.clone();
    let state = crate::supabase::storage_mode(database)?;
    let result = resolve_without_allocations(
        database,
        request,
        "discard",
        Some((&state, "unassigned_discard", Some(operation_id.as_str()))),
    )?;
    crate::cloud_sync::flush_if_online(database)?;
    Ok(result)
}

pub fn pause_for_timer(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    now: i64,
    cloud_state: Option<&crate::supabase::StorageModeSnapshot>,
) -> Result<(), String> {
    coordinate_unassigned_sessions(
        transaction,
        workspace_id,
        now,
        if cloud_state.is_some() {
            "cloud"
        } else {
            "local"
        },
    )?;
    let current_date = crate::work_calendar::current_work_date(transaction, now)?;
    let Some(session_id) = active_session_id(transaction, workspace_id, &current_date)? else {
        return Ok(());
    };
    let base_version = session_version(transaction, &session_id)?;
    close_open_segment(transaction, &session_id, now)?;
    refresh_session_duration(transaction, &session_id, now)?;
    if let Some(state) = cloud_state {
        crate::cloud_sync::enqueue_entity_in_transaction(
            transaction,
            state,
            "unassigned_session_pause",
            "unassigned_session",
            Some(&session_id),
            Some(base_version),
            None,
        )?;
    }
    Ok(())
}

pub fn resume_after_timer(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    now: i64,
    cloud_state: Option<&crate::supabase::StorageModeSnapshot>,
) -> Result<(), String> {
    coordinate_unassigned_sessions(
        transaction,
        workspace_id,
        now,
        if cloud_state.is_some() {
            "cloud"
        } else {
            "local"
        },
    )?;
    let current_date = crate::work_calendar::current_work_date(transaction, now)?;
    let existing_session = active_session_id(transaction, workspace_id, &current_date)?;
    let session_id = match existing_session.as_ref() {
        Some(id) => id.clone(),
        None => create_session(
            transaction,
            workspace_id,
            now,
            if cloud_state.is_some() {
                "cloud"
            } else {
                "local"
            },
            None,
        )?,
    };
    let base_version = if existing_session.is_some() {
        Some(session_version(transaction, &session_id)?)
    } else {
        None
    };
    let open_count: i64 = transaction
        .query_row(
            "SELECT COUNT(*) FROM unassigned_segments WHERE session_id = ?1 AND ended_at IS NULL",
            [&session_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if open_count == 0 {
        let sequence: i64 = transaction
            .query_row(
                "SELECT COALESCE(MAX(sequence_no), 0) + 1 FROM unassigned_segments WHERE session_id = ?1",
                [&session_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "INSERT INTO unassigned_segments(id, workspace_id, session_id, sequence_no, started_at, duration_seconds)
                 VALUES (?1, ?2, ?3, ?4, ?5, 0)",
                params![Uuid::now_v7().to_string(), workspace_id, session_id, sequence, now],
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "UPDATE unassigned_sessions SET updated_at = ?1, version = version + 1 WHERE id = ?2",
                params![now, session_id],
            )
            .map_err(|error| error.to_string())?;
    }
    if let Some(state) = cloud_state {
        crate::cloud_sync::enqueue_entity_in_transaction(
            transaction,
            state,
            if base_version.is_some() {
                "unassigned_session_resume"
            } else {
                "unassigned_session_create"
            },
            "unassigned_session",
            Some(&session_id),
            base_version,
            None,
        )?;
    }
    Ok(())
}

fn seal_session(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    session_id: &str,
    expected_version: i64,
    now: i64,
) -> Result<UnassignedStateDto, String> {
    let version: Option<i64> = transaction
        .query_row(
            "SELECT version FROM unassigned_sessions
             WHERE id = ?1 AND workspace_id = ?2 AND state IN ('collecting', 'awaiting_resolution')",
            params![session_id, workspace_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if version != Some(expected_version) {
        return Err("VERSION_CONFLICT: 未归属时间已变化，请刷新后重试".to_string());
    }
    let open_segment: bool = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM unassigned_segments WHERE session_id=?1 AND ended_at IS NULL)",
            [session_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let effective_end = if open_segment {
        close_open_segment(transaction, session_id, now)?;
        now
    } else {
        transaction
            .query_row(
                "SELECT COALESCE(MAX(ended_at),?2) FROM unassigned_segments WHERE session_id=?1",
                params![session_id, now],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?
    };
    refresh_session_duration(transaction, session_id, effective_end)?;
    load_state(transaction, session_id, effective_end, false)
}

fn finish_session(
    transaction: &Transaction<'_>,
    session_id: &str,
    expected_version: i64,
    state: &str,
    resolution_type: &str,
    generated_entry_id: Option<&str>,
    now: i64,
) -> Result<(), String> {
    let changed = transaction
        .execute(
            "UPDATE unassigned_sessions
             SET state = ?1, resolution_type = ?2, generated_entry_id = ?3,
                 resolved_at = ?4, last_ended_at = ?4, updated_at = ?4, version = version + 1
             WHERE id = ?5 AND version = ?6 AND state IN ('collecting', 'awaiting_resolution')",
            params![
                state,
                resolution_type,
                generated_entry_id,
                now,
                session_id,
                expected_version
            ],
        )
        .map_err(|error| error.to_string())?;
    if changed == 0 {
        Err("VERSION_CONFLICT: 未归属时间已被处理".to_string())
    } else {
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn insert_generated_entry(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    device_id: &str,
    entry_id: &str,
    state: &UnassignedStateDto,
    kind: &str,
    label: &str,
    now: i64,
) -> Result<(), String> {
    let ended_at = state.last_ended_at.unwrap_or(now);
    transaction
        .execute(
            "INSERT INTO time_entries(
               id, workspace_id, work_date, kind, source_type, state, default_task_id,
               label_snapshot, started_at, ended_at, duration_seconds, note, origin_unassigned_session_id,
               created_at, updated_at, version, created_by_device_id, updated_by_device_id
             ) VALUES (?1, ?2, ?3, ?4, 'unassigned', 'ended', NULL, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11, 1, ?12, ?12)",
            params![
                entry_id,
                workspace_id,
                state.work_date,
                kind,
                label,
                state.first_started_at,
                ended_at,
                state.elapsed_seconds,
                if kind == "break" { "未执行事项期间记录为休息" } else { "未启动事项期间补分配" },
                state.session_id,
                now,
                device_id
            ],
        )
        .map_err(|error| error.to_string())?;
    let segments = {
        let mut statement = transaction
            .prepare(
                "SELECT started_at,ended_at,duration_seconds FROM unassigned_segments
                 WHERE session_id=?1 AND ended_at IS NOT NULL ORDER BY sequence_no",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([&state.session_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        rows
    };
    if segments.is_empty() {
        transaction
            .execute(
                "INSERT INTO time_segments(id,workspace_id,entry_id,sequence_no,started_at,ended_at,duration_seconds)
                 VALUES (?1,?2,?3,1,?4,?5,?6)",
                params![Uuid::now_v7().to_string(),workspace_id,entry_id,state.first_started_at,ended_at,state.elapsed_seconds],
            )
            .map_err(|error| error.to_string())?;
    } else {
        for (index, (started_at, segment_ended_at, duration_seconds)) in
            segments.into_iter().enumerate()
        {
            transaction
                .execute(
                    "INSERT INTO time_segments(id,workspace_id,entry_id,sequence_no,started_at,ended_at,duration_seconds)
                     VALUES (?1,?2,?3,?4,?5,?6,?7)",
                    params![Uuid::now_v7().to_string(),workspace_id,entry_id,index as i64 + 1,started_at,segment_ended_at,duration_seconds],
                )
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn create_session(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    now: i64,
    shared_source: &str,
    predecessor_session_id: Option<&str>,
) -> Result<String, String> {
    let work_date = crate::work_calendar::current_work_date(transaction, now)?;
    create_session_for_date(
        transaction,
        workspace_id,
        &work_date,
        now,
        None,
        shared_source,
        predecessor_session_id,
    )
}

fn create_session_for_date(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    work_date: &str,
    started_at: i64,
    ended_at: Option<i64>,
    shared_source: &str,
    predecessor_session_id: Option<&str>,
) -> Result<String, String> {
    let id = Uuid::now_v7().to_string();
    let threshold = prompt_threshold(transaction, workspace_id)?;
    let duration_seconds = ended_at
        .map(|ended| ended.saturating_sub(started_at) / 1_000)
        .unwrap_or(0);
    let state = if duration_seconds > threshold {
        "awaiting_resolution"
    } else {
        "collecting"
    };
    let updated_at = ended_at.unwrap_or(started_at);
    transaction
        .execute(
            "INSERT INTO unassigned_sessions(
               id,workspace_id,work_date,state,threshold_seconds,duration_seconds,
               first_started_at,last_ended_at,prompted_at,created_at,updated_at,version,
               shared_source,predecessor_session_id,migration_state,last_continuous_at
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?7,?10,1,?11,?12,'ready',?10)",
            params![
                id,
                workspace_id,
                work_date,
                state,
                threshold,
                duration_seconds,
                started_at,
                ended_at,
                (duration_seconds > threshold).then_some(updated_at),
                updated_at,
                shared_source,
                predecessor_session_id,
            ],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO unassigned_segments(
               id,workspace_id,session_id,sequence_no,started_at,ended_at,duration_seconds
             ) VALUES (?1,?2,?3,1,?4,?5,?6)",
            params![
                Uuid::now_v7().to_string(),
                workspace_id,
                id,
                started_at,
                ended_at,
                duration_seconds,
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(id)
}

fn start_next_session_if_idle(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    now: i64,
    shared: bool,
    predecessor_session_id: Option<&str>,
) -> Result<(), String> {
    if !has_active_timer(transaction, workspace_id)? {
        create_session(
            transaction,
            workspace_id,
            now,
            if shared { "cloud" } else { "local" },
            predecessor_session_id,
        )?;
    }
    Ok(())
}

fn session_version(connection: &rusqlite::Connection, session_id: &str) -> Result<i64, String> {
    connection
        .query_row(
            "SELECT version FROM unassigned_sessions WHERE id=?1",
            [session_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())
}

fn load_state(
    connection: &rusqlite::Connection,
    session_id: &str,
    now: i64,
    update_threshold_state: bool,
) -> Result<UnassignedStateDto, String> {
    let (work_date, state, first_started_at, last_ended_at, duration_seconds, threshold_seconds, version):
        (String, String, i64, Option<i64>, i64, i64, i64) = connection
        .query_row(
            "SELECT work_date, state, first_started_at, last_ended_at, duration_seconds, threshold_seconds, version
             FROM unassigned_sessions WHERE id = ?1",
            [session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?)),
        )
        .map_err(|error| error.to_string())?;
    let current_segment_started_at: Option<i64> = connection
        .query_row(
            "SELECT started_at FROM unassigned_segments WHERE session_id = ?1 AND ended_at IS NULL",
            [session_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let live_seconds = current_segment_started_at
        .map(|started_at| ((now - started_at).max(0)) / 1_000)
        .unwrap_or(0);
    let elapsed_seconds = duration_seconds + live_seconds;
    let must_resolve = elapsed_seconds > threshold_seconds;
    let mut next_state = state;
    let mut next_version = version;
    if update_threshold_state && must_resolve && next_state == "collecting" {
        connection
            .execute(
                "UPDATE unassigned_sessions
                 SET state = 'awaiting_resolution', prompted_at = COALESCE(prompted_at, ?1), updated_at = ?1, version = version + 1
                 WHERE id = ?2 AND version = ?3",
                params![now, session_id, version],
            )
            .map_err(|error| error.to_string())?;
        next_state = "awaiting_resolution".to_string();
        next_version += 1;
    }
    Ok(UnassignedStateDto {
        session_id: session_id.to_string(),
        work_date,
        state: next_state,
        first_started_at,
        last_ended_at,
        current_segment_started_at,
        elapsed_seconds,
        required_minutes: settlement_minutes(elapsed_seconds),
        threshold_seconds,
        must_resolve,
        version: next_version,
    })
}

fn close_open_segment(
    transaction: &Transaction<'_>,
    session_id: &str,
    now: i64,
) -> Result<(), String> {
    transaction
        .execute(
            "UPDATE unassigned_segments
             SET ended_at = ?1, duration_seconds = MAX(0, (?1 - started_at) / 1000)
             WHERE session_id = ?2 AND ended_at IS NULL",
            params![now, session_id],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn refresh_session_duration(
    transaction: &Transaction<'_>,
    session_id: &str,
    now: i64,
) -> Result<(), String> {
    let duration: i64 = transaction
        .query_row(
            "SELECT COALESCE(SUM(duration_seconds), 0) FROM unassigned_segments WHERE session_id = ?1",
            [session_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let threshold: i64 = transaction
        .query_row(
            "SELECT threshold_seconds FROM unassigned_sessions WHERE id = ?1",
            [session_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "UPDATE unassigned_sessions
             SET duration_seconds = ?1, last_ended_at = ?2,
                 state = CASE WHEN ?1 > threshold_seconds THEN 'awaiting_resolution' ELSE state END,
                 prompted_at = CASE WHEN ?1 > threshold_seconds THEN COALESCE(prompted_at, ?2) ELSE prompted_at END,
                 updated_at = ?2, version = version + 1 WHERE id = ?3",
            params![duration, now, session_id],
        )
        .map_err(|error| error.to_string())?;
    let _ = threshold;
    Ok(())
}

fn coordinate_unassigned_sessions(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    now: i64,
    shared_source: &str,
) -> Result<(), String> {
    let timezone = crate::work_calendar::workspace_timezone(transaction, workspace_id)?;
    let current_date = crate::work_calendar::work_date_at(timezone, now)?;
    type ActiveSession = (
        String,
        String,
        i64,
        i64,
        Option<String>,
        String,
        Option<i64>,
    );
    let sessions = {
        let mut statement = transaction
            .prepare(
                "SELECT u.id,u.work_date,u.first_started_at,
                        COALESCE(last_continuous_at,updated_at,first_started_at),
                        predecessor_session_id,shared_source,
                        (SELECT started_at FROM unassigned_segments s
                         WHERE s.session_id=u.id AND s.ended_at IS NULL LIMIT 1)
                 FROM unassigned_sessions u
                 WHERE u.workspace_id=?1 AND u.state IN ('collecting','awaiting_resolution')
                 ORDER BY u.work_date,u.created_at",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([workspace_id], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<ActiveSession>, _>>()
            .map_err(|error| error.to_string())?;
        rows
    };
    let has_timer = has_active_timer(transaction, workspace_id)?;
    let current = sessions
        .iter()
        .find(|session| session.1 == current_date)
        .cloned();
    let latest_open_old = sessions
        .iter()
        .filter(|session| session.1 < current_date && session.6.is_some())
        .max_by(|left, right| left.1.cmp(&right.1).then(left.2.cmp(&right.2)))
        .cloned();

    for (session_id, work_date, first_started_at, last_continuous_at, _, _, open_started_at) in
        sessions.iter().filter(|session| session.1 < current_date)
    {
        let Some(open_started_at) = open_started_at else {
            continue;
        };
        let bounds = crate::work_calendar::day_bounds(timezone, work_date)?;
        let is_latest = latest_open_old
            .as_ref()
            .is_some_and(|latest| latest.0 == *session_id);
        let continuous =
            is_latest && now.saturating_sub(*last_continuous_at) <= CONTINUITY_GRACE_MILLIS;
        let boundary = if continuous {
            bounds.end_at.min(now)
        } else {
            (*last_continuous_at)
                .max(*first_started_at)
                .max(*open_started_at)
                .min(bounds.end_at)
                .min(now)
        };
        close_open_segment(transaction, session_id, boundary)?;
        refresh_session_duration(transaction, session_id, boundary)?;
        transaction
            .execute(
                "UPDATE unassigned_sessions SET last_continuous_at=?1 WHERE id=?2",
                params![boundary, session_id],
            )
            .map_err(|error| error.to_string())?;
    }

    if let Some((session_id, _, first_started_at, last_continuous_at, _, _, open_started_at)) =
        current.as_ref()
    {
        if let Some(open_started_at) = open_started_at {
            if now.saturating_sub(*last_continuous_at) > CONTINUITY_GRACE_MILLIS {
                let confirmed_end = (*last_continuous_at)
                    .max(*first_started_at)
                    .max(*open_started_at)
                    .min(now);
                close_open_segment(transaction, session_id, confirmed_end)?;
                refresh_session_duration(transaction, session_id, confirmed_end)?;
                if !has_timer {
                    let sequence: i64 = transaction
                        .query_row(
                            "SELECT COALESCE(MAX(sequence_no),0)+1 FROM unassigned_segments WHERE session_id=?1",
                            [session_id],
                            |row| row.get(0),
                        )
                        .map_err(|error| error.to_string())?;
                    transaction.execute(
                        "INSERT INTO unassigned_segments(id,workspace_id,session_id,sequence_no,started_at,duration_seconds) VALUES (?1,?2,?3,?4,?5,0)",
                        params![Uuid::now_v7().to_string(),workspace_id,session_id,sequence,now],
                    ).map_err(|error| error.to_string())?;
                }
            }
        }
        transaction.execute(
            "UPDATE unassigned_sessions SET last_continuous_at=?1,updated_at=MAX(updated_at,?1) WHERE id=?2",
            params![now,session_id],
        ).map_err(|error| error.to_string())?;
        return Ok(());
    }

    if has_timer {
        return Ok(());
    }

    let continuous_old = latest_open_old
        .as_ref()
        .is_some_and(|session| now.saturating_sub(session.3) <= CONTINUITY_GRACE_MILLIS);
    if continuous_old {
        let latest = latest_open_old.as_ref().unwrap();
        let old_end = crate::work_calendar::day_bounds(timezone, &latest.1)?.end_at;
        let slices = crate::work_calendar::split_interval_by_day(timezone, old_end, now)?;
        let mut predecessor = Some(latest.0.clone());
        for (index, slice) in slices.iter().enumerate() {
            let is_last = index + 1 == slices.len();
            let session_id = create_session_for_date(
                transaction,
                workspace_id,
                &slice.work_date,
                slice.started_at,
                (!is_last).then_some(slice.ended_at),
                shared_source,
                predecessor.as_deref(),
            )?;
            predecessor = Some(session_id.clone());
            let _ = is_last;
        }
    } else {
        create_session(
            transaction,
            workspace_id,
            now,
            shared_source,
            latest_open_old.as_ref().map(|session| session.0.as_str()),
        )?;
    }
    Ok(())
}

fn active_session_id(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    work_date: &str,
) -> Result<Option<String>, String> {
    connection
        .query_row(
            "SELECT id FROM unassigned_sessions
             WHERE workspace_id = ?1 AND work_date=?2
               AND state IN ('collecting', 'awaiting_resolution')
             ORDER BY created_at LIMIT 1",
            params![workspace_id, work_date],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())
}

fn has_active_timer(connection: &rusqlite::Connection, workspace_id: &str) -> Result<bool, String> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM time_entries
             WHERE workspace_id = ?1 AND state IN ('running', 'paused') AND deleted_at IS NULL)",
            [workspace_id],
            |row| row.get::<_, i64>(0),
        )
        .map(|value| value == 1)
        .map_err(|error| error.to_string())
}

fn prompt_threshold(connection: &rusqlite::Connection, workspace_id: &str) -> Result<i64, String> {
    let value: String = connection
        .query_row(
            "SELECT value_json FROM app_settings WHERE workspace_id = ?1 AND key = 'unassigned_prompt_seconds'",
            [workspace_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .unwrap_or_else(|| "300".to_string());
    serde_json::from_str::<i64>(&value).map_err(|error| error.to_string())
}

fn ensure_selectable_task(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    task_id: &str,
) -> Result<(), String> {
    let selectable: Option<i64> = connection
        .query_row(
            "SELECT CASE WHEN EXISTS(
               SELECT 1 FROM tasks child WHERE child.parent_id = task.id AND child.deleted_at IS NULL
             ) THEN 0 ELSE 1 END
             FROM tasks task WHERE task.id = ?1 AND task.workspace_id = ?2 AND task.deleted_at IS NULL",
            params![task_id, workspace_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    match selectable {
        Some(1) => Ok(()),
        Some(_) => Err("TASK_NOT_SELECTABLE: 父事项不能直接分配工时".to_string()),
        None => Err("NOT_FOUND: 事项不存在".to_string()),
    }
}

fn task_title(connection: &rusqlite::Connection, task_id: &str) -> Result<String, String> {
    connection
        .query_row("SELECT title FROM tasks WHERE id = ?1", [task_id], |row| {
            row.get(0)
        })
        .map_err(|error| error.to_string())
}

fn processed_result<T: for<'de> Deserialize<'de>>(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    operation_id: &str,
) -> Result<Option<T>, String> {
    let json: Option<String> = connection
        .query_row(
            "SELECT result_json FROM processed_operations WHERE workspace_id = ?1 AND operation_id = ?2",
            params![workspace_id, operation_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    json.map(|value| serde_json::from_str(&value).map_err(|error| error.to_string()))
        .transpose()
}

fn save_processed_result<T: Serialize>(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    device_id: &str,
    operation_id: &str,
    operation_type: &str,
    result: &T,
) -> Result<(), String> {
    transaction
        .execute(
            "INSERT INTO processed_operations(workspace_id, operation_id, device_id, operation_type, result_json, processed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![workspace_id, operation_id, device_id, operation_type, serde_json::to_string(result).map_err(|error| error.to_string())?, now_millis()],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn validate_operation_id(value: &str) -> Result<(), String> {
    Uuid::parse_str(value)
        .map(|_| ())
        .map_err(|_| "VALIDATION_ERROR: operationId 必须是 UUID".to_string())
}

fn settlement_minutes(duration_seconds: i64) -> i64 {
    if duration_seconds <= 0 {
        0
    } else {
        (duration_seconds + 59) / 60
    }
}

fn workspace_id(connection: &rusqlite::Connection) -> Result<String, String> {
    connection
        .query_row(
            "SELECT id FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())
}

fn device_id(connection: &rusqlite::Connection, workspace_id: &str) -> Result<String, String> {
    connection
        .query_row(
            "SELECT id FROM devices WHERE workspace_id = ?1 AND revoked_at IS NULL ORDER BY created_at LIMIT 1",
            [workspace_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())
}

fn bump_revision(connection: &rusqlite::Connection) -> Result<(), String> {
    connection
        .execute(
            "UPDATE app_metadata SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT) WHERE key = 'global_revision'",
            [],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[tauri::command]
pub fn unassigned_get_state(
    database: tauri::State<'_, Database>,
) -> Result<Option<UnassignedStateDto>, String> {
    let state = crate::supabase::storage_mode(&database)?;
    if state.mode == "cloud" {
        crate::cloud_sync::refresh_unassigned_state_for_cloud_mode(&database)
    } else {
        get_state(&database)
    }
}

#[tauri::command]
pub fn unassigned_get_state_snapshot(
    database: tauri::State<'_, Database>,
) -> Result<UnassignedStateSnapshotDto, String> {
    let state = crate::supabase::storage_mode(&database)?;
    let current = if state.mode == "cloud" {
        crate::cloud_sync::refresh_unassigned_state_for_cloud_mode(&database)?
    } else {
        get_state(&database)?
    };
    let connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    load_state_snapshot(&connection, &workspace_id, current, now_millis())
}

#[tauri::command]
pub fn unassigned_get_session(
    database: tauri::State<'_, Database>,
    session_id: String,
) -> Result<UnassignedStateDto, String> {
    get_session_state(&database, &session_id)
}

#[tauri::command]
pub fn unassigned_resolve_work(
    database: tauri::State<'_, Database>,
    request: ResolveWorkRequest,
) -> Result<UnassignedResolveResult, String> {
    resolve_work_for_sync(&database, request)
}

#[tauri::command]
pub fn unassigned_resolve_break(
    database: tauri::State<'_, Database>,
    request: ResolveSessionRequest,
) -> Result<UnassignedResolveResult, String> {
    resolve_break_for_sync(&database, request)
}

#[tauri::command]
pub fn unassigned_discard(
    database: tauri::State<'_, Database>,
    request: ResolveSessionRequest,
) -> Result<UnassignedResolveResult, String> {
    discard_for_sync(&database, request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{self, SettingsScope, SettingsUpdate};
    use crate::tasks::{create_task, TaskCreateRequest};
    use serde_json::json;
    use tempfile::tempdir;

    fn setup() -> (Database, String) {
        let directory = tempdir().unwrap();
        let path = directory.path().join("unassigned.sqlite3");
        let database = Database::initialize_at(path).unwrap();
        std::mem::forget(directory);
        let connection = database.open().unwrap();
        let subject_id: String = connection
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        drop(connection);
        let task = create_task(
            &database,
            TaskCreateRequest {
                subject_id,
                parent_id: None,
                title: "未归属分配事项".to_string(),
                planned_date: None,
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        (database, task.id)
    }

    #[test]
    fn creates_collecting_session_and_resolves_to_multiple_actions() {
        let (database, task_id) = setup();
        let initial = get_state(&database).unwrap().unwrap();
        let connection = database.open().unwrap();
        connection
            .execute(
                "UPDATE unassigned_segments SET started_at = started_at - 360000 WHERE session_id = ?1 AND ended_at IS NULL",
                [&initial.session_id],
            )
            .unwrap();
        drop(connection);
        let due = get_state(&database).unwrap().unwrap();
        assert!(due.must_resolve);
        assert!(due.elapsed_seconds >= 360);
        let result = resolve_work(
            &database,
            ResolveWorkRequest {
                session_id: due.session_id,
                allocations: vec![UnassignedAllocationInput {
                    task_id: task_id.clone(),
                    minutes: due.required_minutes,
                    complete_task: true,
                    task_expected_version: Some(1),
                }],
                expected_version: due.version,
                operation_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        assert_eq!(result.resolution_type, "work");
        assert!(result.generated_entry_id.is_some());
        let task_status: String = database
            .open()
            .unwrap()
            .query_row("SELECT status FROM tasks WHERE id = ?1", [task_id], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(task_status, "done");
        let next = get_state(&database).unwrap().unwrap();
        assert_ne!(next.session_id, result.session_id);
    }

    #[test]
    fn allows_partial_allocation_and_can_discard_empty_allocation() {
        let (database, task_id) = setup();
        let initial = get_state(&database).unwrap().unwrap();
        let connection = database.open().unwrap();
        connection
            .execute(
                "UPDATE unassigned_segments SET started_at = started_at - 120000 WHERE session_id = ?1 AND ended_at IS NULL",
                [&initial.session_id],
            )
            .unwrap();
        drop(connection);
        let initial = get_state(&database).unwrap().unwrap();
        assert!(initial.required_minutes >= 2);
        let result = resolve_work(
            &database,
            ResolveWorkRequest {
                session_id: initial.session_id.clone(),
                allocations: vec![UnassignedAllocationInput {
                    task_id,
                    minutes: 1,
                    complete_task: false,
                    task_expected_version: None,
                }],
                expected_version: initial.version,
                operation_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        assert_eq!(result.resolution_type, "work");
        let entry_id = result.generated_entry_id.unwrap();
        let allocated: i64 = database
            .open()
            .unwrap()
            .query_row(
                "SELECT COALESCE(SUM(minutes), 0) FROM time_allocations WHERE entry_id = ?1",
                [&entry_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(allocated, 1);

        let next = get_state(&database).unwrap().unwrap();
        assert!(resolve_work(
            &database,
            ResolveWorkRequest {
                session_id: next.session_id.clone(),
                allocations: Vec::new(),
                expected_version: next.version,
                operation_id: Uuid::now_v7().to_string(),
            }
        )
        .is_err());
        let refreshed = get_state(&database).unwrap().unwrap();
        let result = discard(
            &database,
            ResolveSessionRequest {
                session_id: refreshed.session_id,
                expected_version: refreshed.version,
                operation_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        assert_eq!(result.resolution_type, "discard");
        assert!(result.generated_entry_id.is_some());
    }

    #[test]
    fn local_mode_refreshes_do_not_create_shared_outbox() {
        let (database, _) = setup();
        let first = get_state(&database).unwrap().unwrap();
        for _ in 0..10 {
            let refreshed = get_state(&database).unwrap().unwrap();
            assert_eq!(refreshed.session_id, first.session_id);
        }
        let connection = database.open().unwrap();
        let outbox_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sync_outbox WHERE entity_type='unassigned_session'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(outbox_count, 0);
        let shared_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM unassigned_sessions WHERE shared_source='cloud'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(shared_count, 0);
    }

    #[test]
    fn cloud_mode_refreshes_do_not_upload_per_second() {
        let (database, _) = setup();
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
        let state = crate::supabase::storage_mode(&database).unwrap();
        let first = get_state_with_cloud_context(&database, Some(&state))
            .unwrap()
            .unwrap();
        let initial_outbox: i64 = database
            .open()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM sync_outbox WHERE workspace_id=?1 AND entity_type='unassigned_session'",
                [&cloud_workspace_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(initial_outbox, 1);
        for _ in 0..10 {
            let refreshed = get_state_with_cloud_context(&database, Some(&state))
                .unwrap()
                .unwrap();
            assert_eq!(refreshed.session_id, first.session_id);
        }
        let final_outbox: i64 = database
            .open()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM sync_outbox WHERE workspace_id=?1 AND entity_type='unassigned_session'",
                [&cloud_workspace_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(final_outbox, initial_outbox);
    }

    #[test]
    fn cloud_mode_unassigned_work_can_write_local_cache_when_cloud_is_unavailable() {
        let (database, task_id) = setup();
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
        let initial = get_state(&database).unwrap().unwrap();
        database
            .open()
            .unwrap()
            .execute(
                "UPDATE unassigned_segments SET started_at = started_at - 120000 WHERE session_id = ?1 AND ended_at IS NULL",
                [&initial.session_id],
            )
            .unwrap();
        let due = get_state(&database).unwrap().unwrap();
        let operation_id = Uuid::now_v7().to_string();
        let state = crate::supabase::storage_mode(&database).unwrap();
        let result = resolve_work_with_hooks_and_cloud_operation(
            &database,
            ResolveWorkRequest {
                session_id: due.session_id,
                allocations: vec![UnassignedAllocationInput {
                    task_id,
                    minutes: due.required_minutes,
                    complete_task: false,
                    task_expected_version: None,
                }],
                expected_version: due.version,
                operation_id: operation_id.clone(),
            },
            Some((
                &state,
                "unassigned_resolve_work",
                Some(operation_id.as_str()),
            )),
        )
        .unwrap();
        crate::cloud_sync::flush_if_online(&database).unwrap();

        let entry_id = result.generated_entry_id.unwrap();
        let connection = database.open().unwrap();
        let outbox = connection
            .prepare(
                "SELECT operation_id, operation_type, entity_type, entity_id, base_version, payload_json FROM sync_outbox WHERE workspace_id = ?1 ORDER BY created_at, rowid",
            )
            .unwrap()
            .query_map([cloud_workspace_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(outbox.len(), 2);
        assert_eq!(outbox[0].0, operation_id);
        assert_eq!(outbox[0].1, "unassigned_resolve_work");
        assert_eq!(outbox[0].2, "unassigned_session");
        assert_eq!(outbox[1].1, "unassigned_time_entry_create");
        assert_eq!(outbox[1].2, "time_entry");
        assert_eq!(outbox[1].3, entry_id);
        assert_eq!(outbox[1].4, None);
        let entry_payload: serde_json::Value = serde_json::from_str(&outbox[1].5).unwrap();
        assert_eq!(
            entry_payload.get("origin_unassigned_session_id"),
            Some(&serde_json::Value::String(result.session_id))
        );
        assert!(entry_payload
            .get("allocations")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|allocations| !allocations.is_empty()));
    }

    #[test]
    fn cloud_mode_unassigned_break_and_discard_queue_generated_entries_transactionally() {
        let (database, _) = setup();
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
        let state = crate::supabase::storage_mode(&database).unwrap();

        let first = get_state(&database).unwrap().unwrap();
        database
            .open()
            .unwrap()
            .execute(
                "UPDATE unassigned_segments SET started_at = started_at - 60000 WHERE session_id = ?1 AND ended_at IS NULL",
                [&first.session_id],
            )
            .unwrap();
        let first = get_state(&database).unwrap().unwrap();
        let break_operation_id = Uuid::now_v7().to_string();
        let break_result = resolve_without_allocations(
            &database,
            ResolveSessionRequest {
                session_id: first.session_id,
                expected_version: first.version,
                operation_id: break_operation_id.clone(),
            },
            "break",
            Some((
                &state,
                "unassigned_resolve_break",
                Some(break_operation_id.as_str()),
            )),
        )
        .unwrap();

        let second = get_state(&database).unwrap().unwrap();
        database
            .open()
            .unwrap()
            .execute(
                "UPDATE unassigned_segments SET started_at = started_at - 60000 WHERE session_id = ?1 AND ended_at IS NULL",
                [&second.session_id],
            )
            .unwrap();
        let second = get_state(&database).unwrap().unwrap();
        let discard_operation_id = Uuid::now_v7().to_string();
        let discard_result = resolve_without_allocations(
            &database,
            ResolveSessionRequest {
                session_id: second.session_id,
                expected_version: second.version,
                operation_id: discard_operation_id.clone(),
            },
            "discard",
            Some((
                &state,
                "unassigned_discard",
                Some(discard_operation_id.as_str()),
            )),
        )
        .unwrap();

        let connection = database.open().unwrap();
        let outbox = connection
            .prepare(
                "SELECT operation_id, operation_type, entity_type, entity_id, base_version FROM sync_outbox ORDER BY created_at, rowid",
            )
            .unwrap()
            .query_map([], |row| {
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

        assert_eq!(outbox.len(), 4);
        assert_eq!(outbox[0].0, break_operation_id);
        assert_eq!(outbox[0].1, "unassigned_resolve_break");
        assert_eq!(outbox[0].2, "unassigned_session");
        assert_eq!(outbox[1].1, "unassigned_time_entry_create");
        assert_eq!(outbox[1].2, "time_entry");
        assert_eq!(
            outbox[1].3.as_deref(),
            break_result.generated_entry_id.as_deref()
        );
        assert_eq!(outbox[1].4, None);
        assert_eq!(outbox[2].0, discard_operation_id);
        assert_eq!(outbox[2].1, "unassigned_discard");
        assert_eq!(outbox[2].2, "unassigned_session");
        assert_eq!(outbox[3].1, "unassigned_time_entry_create");
        assert_eq!(outbox[3].2, "time_entry");
        assert_eq!(
            outbox[3].3.as_deref(),
            discard_result.generated_entry_id.as_deref()
        );
        assert_eq!(outbox[3].4, None);
    }

    fn shanghai_millis(value: &str) -> i64 {
        use chrono::{NaiveDateTime, TimeZone};
        let timezone: chrono_tz::Tz = "Asia/Shanghai".parse().unwrap();
        timezone
            .from_local_datetime(
                &NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S").unwrap(),
            )
            .single()
            .unwrap()
            .timestamp_millis()
    }

    fn reset_unassigned_sessions(database: &Database) -> String {
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        connection
            .execute("DELETE FROM unassigned_segments", [])
            .unwrap();
        connection
            .execute("DELETE FROM unassigned_sessions", [])
            .unwrap();
        workspace_id
    }

    fn seed_unassigned_session(
        database: &Database,
        workspace_id: &str,
        work_date: &str,
        started_at: i64,
        last_continuous_at: i64,
    ) -> String {
        let session_id = Uuid::now_v7().to_string();
        let connection = database.open().unwrap();
        connection.execute(
            "INSERT INTO unassigned_sessions(
               id,workspace_id,work_date,state,threshold_seconds,duration_seconds,
               first_started_at,created_at,updated_at,version,shared_source,migration_state,last_continuous_at
             ) VALUES (?1,?2,?3,'collecting',300,0,?4,?4,?5,1,'local','ready',?5)",
            params![session_id,workspace_id,work_date,started_at,last_continuous_at],
        ).unwrap();
        connection.execute(
            "INSERT INTO unassigned_segments(id,workspace_id,session_id,sequence_no,started_at,duration_seconds) VALUES (?1,?2,?3,1,?4,0)",
            params![Uuid::now_v7().to_string(),workspace_id,session_id,started_at],
        ).unwrap();
        session_id
    }

    #[test]
    fn unassigned_continuity_creates_one_session_per_calendar_day() {
        let (database, _) = setup();
        let workspace_id = reset_unassigned_sessions(&database);
        let started_at = shanghai_millis("2026-10-05 23:59:30");
        let now = shanghai_millis("2026-10-07 00:00:30");
        let first_session = seed_unassigned_session(
            &database,
            &workspace_id,
            "2026-10-05",
            started_at,
            now - 1_000,
        );
        let mut connection = database.open().unwrap();
        let transaction = connection.transaction().unwrap();
        coordinate_unassigned_sessions(&transaction, &workspace_id, now, "local").unwrap();
        transaction.commit().unwrap();

        let connection = database.open().unwrap();
        let rows = connection.prepare("SELECT id,work_date,duration_seconds,predecessor_session_id FROM unassigned_sessions ORDER BY work_date").unwrap().query_map([],|row|Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,i64>(2)?,row.get::<_,Option<String>>(3)?))).unwrap().collect::<Result<Vec<_>,_>>().unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows.iter().map(|row| row.1.as_str()).collect::<Vec<_>>(),
            vec!["2026-10-05", "2026-10-06", "2026-10-07"]
        );
        assert_eq!(
            rows.iter().map(|row| row.2).collect::<Vec<_>>(),
            vec![30, 86_400, 0]
        );
        assert_eq!(rows[1].3.as_deref(), Some(first_session.as_str()));
        assert_eq!(rows[2].3.as_deref(), Some(rows[1].0.as_str()));
        let open_segments: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM unassigned_segments WHERE ended_at IS NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(open_segments, 1);
    }

    #[test]
    fn existing_today_session_does_not_leave_yesterday_segment_open() {
        let (database, _) = setup();
        let workspace_id = reset_unassigned_sessions(&database);
        let old_start = shanghai_millis("2026-10-05 23:59:00");
        let today_start = shanghai_millis("2026-10-06 08:00:00");
        let now = shanghai_millis("2026-10-06 08:01:00");
        let old_session = seed_unassigned_session(
            &database,
            &workspace_id,
            "2026-10-05",
            old_start,
            shanghai_millis("2026-10-06 00:00:00"),
        );
        seed_unassigned_session(
            &database,
            &workspace_id,
            "2026-10-06",
            today_start,
            now - 1_000,
        );
        let mut connection = database.open().unwrap();
        let transaction = connection.transaction().unwrap();
        coordinate_unassigned_sessions(&transaction, &workspace_id, now, "local").unwrap();
        transaction.commit().unwrap();
        let connection = database.open().unwrap();
        let old_open: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM unassigned_segments WHERE session_id=?1 AND ended_at IS NULL",
                [&old_session],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(old_open, 0);
    }

    #[test]
    fn overnight_unassigned_interruption_excludes_offline_gap() {
        let (database, _) = setup();
        let workspace_id = reset_unassigned_sessions(&database);
        let started_at = shanghai_millis("2026-10-05 23:50:00");
        let last_continuous = shanghai_millis("2026-10-05 23:51:00");
        let now = shanghai_millis("2026-10-06 08:00:00");
        let old_session = seed_unassigned_session(
            &database,
            &workspace_id,
            "2026-10-05",
            started_at,
            last_continuous,
        );
        let mut connection = database.open().unwrap();
        let transaction = connection.transaction().unwrap();
        coordinate_unassigned_sessions(&transaction, &workspace_id, now, "local").unwrap();
        transaction.commit().unwrap();
        let connection = database.open().unwrap();
        let old_duration: i64 = connection
            .query_row(
                "SELECT duration_seconds FROM unassigned_sessions WHERE id=?1",
                [&old_session],
                |row| row.get(0),
            )
            .unwrap();
        let today: (String,i64) = connection.query_row("SELECT work_date,first_started_at FROM unassigned_sessions WHERE work_date='2026-10-06'",[],|row|Ok((row.get(0)?,row.get(1)?))).unwrap();
        assert_eq!(old_duration, 60);
        assert_eq!(today, ("2026-10-06".to_string(), now));
    }

    #[test]
    fn resolving_yesterday_uses_session_date_and_real_segment_end() {
        let (database, task_id) = setup();
        let workspace_id = reset_unassigned_sessions(&database);
        let started_at = shanghai_millis("2026-10-05 23:58:00");
        let ended_at = shanghai_millis("2026-10-05 23:59:00");
        let session_id =
            seed_unassigned_session(&database, &workspace_id, "2026-10-05", started_at, ended_at);
        let connection = database.open().unwrap();
        connection.execute("UPDATE unassigned_segments SET ended_at=?1,duration_seconds=60 WHERE session_id=?2",params![ended_at,session_id]).unwrap();
        connection.execute("UPDATE unassigned_sessions SET duration_seconds=60,last_ended_at=?1,state='awaiting_resolution',version=2 WHERE id=?2",params![ended_at,session_id]).unwrap();
        drop(connection);
        let result = resolve_work(
            &database,
            ResolveWorkRequest {
                session_id: session_id.clone(),
                allocations: vec![UnassignedAllocationInput {
                    task_id,
                    minutes: 1,
                    complete_task: false,
                    task_expected_version: None,
                }],
                expected_version: 2,
                operation_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        let entry_id = result.generated_entry_id.unwrap();
        let connection = database.open().unwrap();
        let entry: (String, i64, i64) = connection
            .query_row(
                "SELECT work_date,started_at,ended_at FROM time_entries WHERE id=?1",
                [&entry_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(entry, ("2026-10-05".to_string(), started_at, ended_at));
    }

    #[test]
    fn state_snapshot_lists_historical_pending_sessions_separately_by_date() {
        let (database, _) = setup();
        let workspace_id = reset_unassigned_sessions(&database);
        let first_start = shanghai_millis("2026-10-04 10:00:00");
        let first_end = shanghai_millis("2026-10-04 10:10:00");
        let second_start = shanghai_millis("2026-10-05 11:00:00");
        let second_end = shanghai_millis("2026-10-05 11:20:00");
        for (date, started_at, ended_at, duration) in [
            ("2026-10-04", first_start, first_end, 600),
            ("2026-10-05", second_start, second_end, 1200),
        ] {
            let session_id =
                seed_unassigned_session(&database, &workspace_id, date, started_at, ended_at);
            let connection = database.open().unwrap();
            connection.execute("UPDATE unassigned_segments SET ended_at=?1,duration_seconds=?2 WHERE session_id=?3",params![ended_at,duration,session_id]).unwrap();
            connection.execute("UPDATE unassigned_sessions SET state='awaiting_resolution',duration_seconds=?1,last_ended_at=?2 WHERE id=?3",params![duration,ended_at,session_id]).unwrap();
        }

        let snapshot = get_state_snapshot(&database).unwrap();
        assert!(snapshot.current.is_some());
        assert_eq!(snapshot.historical_pending_count, 2);
        assert_eq!(
            snapshot.earliest_historical_date.as_deref(),
            Some("2026-10-04")
        );
        assert_eq!(
            snapshot
                .historical_pending
                .iter()
                .map(|state| state.work_date.as_str())
                .collect::<Vec<_>>(),
            vec!["2026-10-04", "2026-10-05"]
        );
        assert_eq!(
            snapshot
                .historical_pending
                .iter()
                .map(|state| state.elapsed_seconds)
                .collect::<Vec<_>>(),
            vec![600, 1200]
        );
    }
}
