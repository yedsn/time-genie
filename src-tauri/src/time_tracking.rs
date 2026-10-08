use std::collections::HashMap;

use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::database::Database;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TimeAllocationDto {
    pub id: String,
    pub task_id: String,
    pub task_title: String,
    pub minutes: i64,
    pub note: Option<String>,
    pub version: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TimeEntryDto {
    pub id: String,
    pub work_date: String,
    pub label: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub duration_seconds: i64,
    pub settlement_minutes: i64,
    pub allocated_minutes: i64,
    pub default_task_id: Option<String>,
    pub note: Option<String>,
    pub kind: String,
    pub source_type: String,
    pub state: String,
    pub version: i64,
    pub timer_chain_id: Option<String>,
    pub previous_entry_id: Option<String>,
    pub split_boundary_at: Option<i64>,
    pub allocations: Vec<TimeAllocationDto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeEntryListResult {
    pub entries: Vec<TimeEntryDto>,
    pub raw_minutes: i64,
    pub allocated_minutes: i64,
    pub pending_minutes: i64,
    pub break_minutes: i64,
    pub revision: i64,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TimerStopResultDto {
    pub current_entry: TimeEntryDto,
    pub slices: Vec<TimeEntryDto>,
    pub total_duration_seconds: i64,
    pub total_settlement_minutes: i64,
}

const CONTINUITY_GRACE_MILLIS: i64 = 90_000;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimerStartRequest {
    pub task_id: Option<String>,
    pub note: Option<String>,
    pub client_request_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimerVersionRequest {
    pub entry_id: String,
    pub expected_version: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimerResumeRequest {
    pub entry_id: String,
    pub expected_version: i64,
    pub operation_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimerStopRequest {
    pub entry_id: String,
    pub expected_version: i64,
    pub create_default_allocation: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeEntryListRequest {
    pub work_date: String,
    pub include_breaks: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualEntryRequest {
    pub task_id: Option<String>,
    pub work_date: String,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub minutes: Option<i64>,
    pub note: Option<String>,
    #[serde(default)]
    pub complete_task: bool,
    pub task_expected_version: Option<i64>,
    pub client_request_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeEntryUpdateRequest {
    pub entry_id: String,
    pub started_at: i64,
    pub ended_at: i64,
    pub note: Option<String>,
    pub expected_version: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeEntryDispositionRequest {
    pub entry_id: String,
    pub expected_version: i64,
    pub disposition: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AllocationReplaceRequest {
    pub entry_id: String,
    pub allocations: Vec<AllocationInput>,
    pub expected_version: i64,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AllocationInput {
    pub task_id: String,
    pub minutes: i64,
    pub note: Option<String>,
    #[serde(default)]
    pub complete_task: bool,
    pub task_expected_version: Option<i64>,
}

pub fn get_timer_state(database: &Database) -> Result<Option<TimeEntryDto>, String> {
    coordinate_active_timer(database, now_millis())?;
    let connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let entry_id: Option<String> = connection
        .query_row(
            "SELECT id FROM time_entries
             WHERE workspace_id = ?1 AND state IN ('running', 'paused') AND deleted_at IS NULL
             LIMIT 1",
            [workspace_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    entry_id
        .map(|id| load_entry(&connection, &id, now_millis()))
        .transpose()
}

pub(crate) fn get_time_entry_by_id(
    database: &Database,
    entry_id: &str,
) -> Result<TimeEntryDto, String> {
    let connection = database.open()?;
    load_entry(&connection, entry_id, now_millis())
}

pub fn start_timer_with_hooks_for_sync(
    database: &Database,
    request: TimerStartRequest,
) -> Result<TimeEntryDto, String> {
    let operation_id = request.client_request_id.clone();
    let state = crate::supabase::storage_mode(database)?;
    let result = start_timer_with_hooks_and_cloud_operation(
        database,
        request,
        Some((&state, "timer_start", Some(operation_id.as_str()))),
    )?;
    crate::cloud_sync::flush_if_online(database)?;
    Ok(result)
}

fn start_timer_with_hooks_and_cloud_operation(
    database: &Database,
    request: TimerStartRequest,
    cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str, Option<&str>)>,
) -> Result<TimeEntryDto, String> {
    let operation_id = request.client_request_id.clone();
    let result = start_timer_with_cloud_operation(database, request, cloud_operation)?;
    publish_timer_hook(
        database,
        "timer.started",
        format!("timer.started:{operation_id}"),
        &result,
    );
    Ok(result)
}

pub fn stop_timer_with_hooks_for_sync(
    database: &Database,
    request: TimerStopRequest,
) -> Result<TimeEntryDto, String> {
    let state = crate::supabase::storage_mode(database)?;
    let result =
        stop_timer_with_hooks_and_cloud_operation(database, request, Some((&state, "timer_stop")))?;
    crate::cloud_sync::flush_if_online(database)?;
    Ok(result)
}

fn stop_timer_with_hooks_and_cloud_operation(
    database: &Database,
    request: TimerStopRequest,
    cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str)>,
) -> Result<TimeEntryDto, String> {
    let result = stop_timer_with_cloud_operation(database, request, cloud_operation)?;
    publish_timer_hook(
        database,
        "timer.stopped",
        format!("timer.stopped:{}:{}", result.id, result.version),
        &result,
    );
    Ok(result)
}

fn timer_chain_result(
    database: &Database,
    current_entry: TimeEntryDto,
) -> Result<TimerStopResultDto, String> {
    let connection = database.open()?;
    let chain_id = current_entry
        .timer_chain_id
        .clone()
        .unwrap_or_else(|| current_entry.id.clone());
    let mut statement = connection
        .prepare(
            "SELECT id FROM time_entries
             WHERE workspace_id = (SELECT workspace_id FROM time_entries WHERE id = ?1)
               AND COALESCE(timer_chain_id, id) = ?2 AND deleted_at IS NULL
             ORDER BY work_date, started_at, id",
        )
        .map_err(|error| error.to_string())?;
    let ids = statement
        .query_map(params![current_entry.id, chain_id], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let now = now_millis();
    let slices = ids
        .iter()
        .map(|id| load_entry(&connection, id, now))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(TimerStopResultDto {
        total_duration_seconds: slices.iter().map(|entry| entry.duration_seconds).sum(),
        total_settlement_minutes: slices.iter().map(|entry| entry.settlement_minutes).sum(),
        current_entry: slices
            .iter()
            .find(|entry| entry.id == current_entry.id)
            .cloned()
            .unwrap_or(current_entry),
        slices,
    })
}

pub(crate) fn publish_timer_hook(
    database: &Database,
    event_type: &str,
    event_id: String,
    entry: &TimeEntryDto,
) {
    let event = crate::automation_hooks::timer_event(database, event_type, event_id, entry);
    match event
        .and_then(|event| crate::automation_hooks::publish(database.clone(), event).map(|_| ()))
    {
        Ok(()) => {}
        Err(error) => eprintln!("计时 Hook 调度失败，不影响计时结果: {error}"),
    }
}

#[cfg(test)]
pub fn start_timer(
    database: &Database,
    request: TimerStartRequest,
) -> Result<TimeEntryDto, String> {
    start_timer_with_cloud_operation(database, request, None)
}

fn start_timer_with_cloud_operation(
    database: &Database,
    request: TimerStartRequest,
    cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str, Option<&str>)>,
) -> Result<TimeEntryDto, String> {
    validate_operation_id(&request.client_request_id)?;
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let device_id = device_id(&connection, &workspace_id)?;
    if let Some(result) =
        processed_result::<TimeEntryDto>(&connection, &workspace_id, &request.client_request_id)?
    {
        return Ok(result);
    }
    let label = if let Some(task_id) = &request.task_id {
        selectable_task_title(&connection, &workspace_id, task_id)?
    } else {
        "未关联事项".to_string()
    };
    let now = now_millis();
    let entry_id = Uuid::now_v7().to_string();
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    coordinate_active_timer_in_transaction(&transaction, &workspace_id, now)?;
    let active: i64 = transaction
        .query_row(
            "SELECT COUNT(*) FROM time_entries
             WHERE workspace_id = ?1 AND state IN ('running', 'paused') AND deleted_at IS NULL",
            [&workspace_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if active > 0 {
        return Err("ACTIVE_TIMER_EXISTS: 已有计时正在运行或暂停".to_string());
    }
    let work_date = crate::work_calendar::current_work_date(&transaction, now)?;
    if let Some(task_id) = request.task_id.as_deref() {
        crate::recurring::ensure_occurrence_if_recurring(
            &transaction,
            &workspace_id,
            task_id,
            &work_date,
            now,
        )?;
    }
    crate::unassigned::clear_for_timer(
        &transaction,
        &workspace_id,
        now,
        cloud_operation.is_some(),
    )?;
    transaction
        .execute(
            "INSERT INTO time_entries(
               id, workspace_id, work_date, kind, source_type, state, default_task_id,
               label_snapshot, started_at, duration_seconds, note, created_at, updated_at, version,
               created_by_device_id, updated_by_device_id, timer_chain_id, last_continuous_at
             ) VALUES (?1, ?2, ?3, 'work', 'timer', 'running', ?4, ?5, ?6, 0, ?7, ?6, ?6, 1, ?8, ?8, ?1, ?6)",
            params![entry_id, workspace_id, work_date, request.task_id, label, now, request.note, device_id],
        )
        .map_err(map_active_timer_error)?;
    transaction
        .execute(
            "INSERT INTO time_segments(id, workspace_id, entry_id, sequence_no, started_at, duration_seconds)
             VALUES (?1, ?2, ?3, 1, ?4, 0)",
            params![Uuid::now_v7().to_string(), workspace_id, entry_id, now],
        )
        .map_err(|error| error.to_string())?;
    record_runtime_heartbeat(&transaction, &workspace_id, now)?;
    bump_revision(&transaction)?;
    let result = load_entry(&transaction, &entry_id, now)?;
    save_processed_result(
        &transaction,
        &workspace_id,
        &device_id,
        &request.client_request_id,
        "timer_start",
        &result,
    )?;
    if let Some((state, operation_type, operation_id)) = cloud_operation {
        crate::cloud_sync::enqueue_entity_in_transaction(
            &transaction,
            state,
            operation_type,
            "time_entry",
            Some(&entry_id),
            None,
            operation_id,
        )?;
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

pub fn pause_timer_for_sync(
    database: &Database,
    request: TimerVersionRequest,
) -> Result<TimeEntryDto, String> {
    let state = crate::supabase::storage_mode(database)?;
    let result =
        pause_timer_with_cloud_operation(database, request, Some((&state, "timer_pause")))?;
    crate::cloud_sync::flush_if_online(database)?;
    Ok(result)
}

fn pause_timer_with_cloud_operation(
    database: &Database,
    request: TimerVersionRequest,
    cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str)>,
) -> Result<TimeEntryDto, String> {
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let now = now_millis();
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let (entry_id, active_version) = resolve_active_timer_request(
        &transaction,
        &workspace_id,
        &request.entry_id,
        request.expected_version,
        now,
    )?;
    let state = entry_state(&transaction, &workspace_id, &entry_id, active_version)?;
    if state != "running" {
        return Err("VALIDATION_ERROR: 只有运行中的计时可以暂停".to_string());
    }
    close_open_segment(&transaction, &entry_id, now)?;
    let duration = segment_duration_sum(&transaction, &entry_id)?;
    update_entry_state(
        &transaction,
        &entry_id,
        active_version,
        "paused",
        None,
        duration,
        now,
    )?;
    bump_revision(&transaction)?;
    let result = load_entry(&transaction, &entry_id, now)?;
    if let Some((state, operation_type)) = cloud_operation {
        crate::cloud_sync::enqueue_entity_in_transaction(
            &transaction,
            state,
            operation_type,
            "time_entry",
            Some(&entry_id),
            Some(active_version),
            None,
        )?;
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

pub fn resume_timer_for_sync(
    database: &Database,
    request: TimerResumeRequest,
) -> Result<TimeEntryDto, String> {
    let operation_id = request.operation_id.clone();
    let state = crate::supabase::storage_mode(database)?;
    let result = resume_timer_with_cloud_operation(
        database,
        request,
        Some((&state, "timer_resume", Some(operation_id.as_str()))),
    )?;
    crate::cloud_sync::flush_if_online(database)?;
    Ok(result)
}

fn resume_timer_with_cloud_operation(
    database: &Database,
    request: TimerResumeRequest,
    cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str, Option<&str>)>,
) -> Result<TimeEntryDto, String> {
    validate_operation_id(&request.operation_id)?;
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let device_id = device_id(&connection, &workspace_id)?;
    if let Some(result) =
        processed_result::<TimeEntryDto>(&connection, &workspace_id, &request.operation_id)?
    {
        return Ok(result);
    }
    let now = now_millis();
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let (entry_id, active_version) = resolve_active_timer_request(
        &transaction,
        &workspace_id,
        &request.entry_id,
        request.expected_version,
        now,
    )?;
    let state = entry_state(&transaction, &workspace_id, &entry_id, active_version)?;
    if state != "paused" {
        return Err("VALIDATION_ERROR: 只有暂停中的计时可以继续".to_string());
    }
    let sequence: i64 = transaction
        .query_row(
            "SELECT COALESCE(MAX(sequence_no), 0) + 1 FROM time_segments WHERE entry_id = ?1",
            [&entry_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO time_segments(id, workspace_id, entry_id, sequence_no, started_at, duration_seconds)
             VALUES (?1, ?2, ?3, ?4, ?5, 0)",
            params![Uuid::now_v7().to_string(), workspace_id, entry_id, sequence, now],
        )
        .map_err(|error| error.to_string())?;
    let changed = transaction
        .execute(
            "UPDATE time_entries SET state = 'running', last_continuous_at = ?1, updated_at = ?1, version = version + 1
             WHERE id = ?2 AND version = ?3 AND state = 'paused' AND deleted_at IS NULL",
            params![now, entry_id, active_version],
        )
        .map_err(|error| error.to_string())?;
    if changed == 0 {
        return Err("VERSION_CONFLICT: 计时状态已变化".to_string());
    }
    bump_revision(&transaction)?;
    record_runtime_heartbeat(&transaction, &workspace_id, now)?;
    let result = load_entry(&transaction, &entry_id, now)?;
    save_processed_result(
        &transaction,
        &workspace_id,
        &device_id,
        &request.operation_id,
        "timer_resume",
        &result,
    )?;
    if let Some((state, operation_type, operation_id)) = cloud_operation {
        crate::cloud_sync::enqueue_entity_in_transaction(
            &transaction,
            state,
            operation_type,
            "time_entry",
            Some(&entry_id),
            Some(active_version),
            operation_id,
        )?;
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

#[cfg(test)]
pub fn stop_timer(database: &Database, request: TimerStopRequest) -> Result<TimeEntryDto, String> {
    stop_timer_with_cloud_operation(database, request, None)
}

fn stop_timer_with_cloud_operation(
    database: &Database,
    request: TimerStopRequest,
    cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str)>,
) -> Result<TimeEntryDto, String> {
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let now = now_millis();
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let already_stopped: Option<i64> = transaction
        .query_row(
            "SELECT version FROM time_entries
             WHERE id=?1 AND workspace_id=?2 AND source_type='timer' AND state='ended'
               AND version=?3+1 AND deleted_at IS NULL",
            params![request.entry_id, workspace_id, request.expected_version],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if already_stopped.is_some() {
        return load_entry(&transaction, &request.entry_id, now);
    }
    let (entry_id, active_version) = resolve_active_timer_request(
        &transaction,
        &workspace_id,
        &request.entry_id,
        request.expected_version,
        now,
    )?;
    let state = entry_state(&transaction, &workspace_id, &entry_id, active_version)?;
    if state == "running" {
        close_open_segment(&transaction, &entry_id, now)?;
    } else if state != "paused" {
        return Err("VALIDATION_ERROR: 当前计时已经结束".to_string());
    }
    let duration = segment_duration_sum(&transaction, &entry_id)?;
    update_entry_state(
        &transaction,
        &entry_id,
        active_version,
        "ended",
        Some(now),
        duration,
        now,
    )?;
    if request.create_default_allocation {
        let (default_task_id, chain_id): (Option<String>, String) = transaction
            .query_row(
                "SELECT default_task_id, COALESCE(timer_chain_id,id) FROM time_entries WHERE id = ?1",
                [&entry_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|error| error.to_string())?;
        if let Some(task_id) = default_task_id {
            if is_selectable_task(&transaction, &workspace_id, &task_id)? {
                let mut statement = transaction
                    .prepare(
                        "SELECT id,work_date,started_at,duration_seconds FROM time_entries
                         WHERE workspace_id=?1 AND COALESCE(timer_chain_id,id)=?2
                           AND source_type='timer' AND state='ended' AND deleted_at IS NULL
                         ORDER BY work_date,started_at,id",
                    )
                    .map_err(|error| error.to_string())?;
                let slices = statement
                    .query_map(params![workspace_id, chain_id], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, i64>(3)?,
                        ))
                    })
                    .map_err(|error| error.to_string())?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| error.to_string())?;
                drop(statement);
                let minutes_by_entry = allocate_chain_minutes(&slices);
                for (slice_id, work_date, _, _) in slices {
                    let minutes = minutes_by_entry.get(&slice_id).copied().unwrap_or(0);
                    if minutes <= 0 {
                        continue;
                    }
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
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, 1)
                             ON CONFLICT(entry_id, task_id) DO UPDATE SET minutes = excluded.minutes, updated_at = excluded.updated_at, version = time_allocations.version + 1",
                            params![Uuid::now_v7().to_string(), workspace_id, slice_id, task_id, minutes, now],
                        )
                        .map_err(|error| error.to_string())?;
                }
            } else {
                transaction
                    .execute(
                        "UPDATE time_entries SET default_task_id = NULL
                         WHERE workspace_id=?1 AND COALESCE(timer_chain_id,id)=?2",
                        params![workspace_id, chain_id],
                    )
                    .map_err(|error| error.to_string())?;
            }
        }
    }
    crate::unassigned::start_after_timer(
        &transaction,
        &workspace_id,
        now,
        cloud_operation.is_some(),
    )?;
    bump_revision(&transaction)?;
    record_runtime_heartbeat(&transaction, &workspace_id, now)?;
    let result = load_entry(&transaction, &entry_id, now)?;
    if let Some((state, operation_type)) = cloud_operation {
        crate::cloud_sync::enqueue_entity_in_transaction(
            &transaction,
            state,
            operation_type,
            "time_entry",
            Some(&entry_id),
            Some(active_version),
            None,
        )?;
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

pub fn list_time_entries(
    database: &Database,
    request: TimeEntryListRequest,
) -> Result<TimeEntryListResult, String> {
    validate_date(&request.work_date)?;
    coordinate_active_timer(database, now_millis())?;
    let connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let now = now_millis();
    let mut statement = connection
        .prepare(
            "SELECT id FROM time_entries
             WHERE workspace_id = ?1 AND work_date = ?2 AND deleted_at IS NULL
               AND (?3 = 1 OR kind != 'break')
             ORDER BY started_at DESC",
        )
        .map_err(|error| error.to_string())?;
    let ids = statement
        .query_map(
            params![workspace_id, request.work_date, request.include_breaks],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let entries = ids
        .iter()
        .map(|id| load_entry(&connection, id, now))
        .collect::<Result<Vec<_>, _>>()?;
    let raw_minutes = entries
        .iter()
        .filter(|entry| entry.kind == "work")
        .map(|entry| entry.settlement_minutes)
        .sum();
    let allocated_minutes = entries.iter().map(|entry| entry.allocated_minutes).sum();
    let break_minutes = entries
        .iter()
        .filter(|entry| entry.kind == "break")
        .map(|entry| entry.settlement_minutes)
        .sum();
    let revision = current_revision(&connection)?;
    Ok(TimeEntryListResult {
        entries,
        raw_minutes,
        allocated_minutes,
        pending_minutes: raw_minutes - allocated_minutes,
        break_minutes,
        revision,
    })
}

#[cfg(test)]
pub fn create_manual_entry(
    database: &Database,
    request: ManualEntryRequest,
) -> Result<TimeEntryDto, String> {
    create_manual_entry_with_cloud_operation(database, request, None)
}

fn create_manual_entry_with_cloud_operation(
    database: &Database,
    request: ManualEntryRequest,
    cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str, Option<&str>)>,
) -> Result<TimeEntryDto, String> {
    validate_date(&request.work_date)?;
    validate_operation_id(&request.client_request_id)?;
    let (started_at, ended_at, duration_seconds) = manual_time_range(&request)?;
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let device_id = device_id(&connection, &workspace_id)?;
    if let Some(result) =
        processed_result::<TimeEntryDto>(&connection, &workspace_id, &request.client_request_id)?
    {
        return Ok(result);
    }
    let label = if let Some(task_id) = &request.task_id {
        selectable_task_title(&connection, &workspace_id, task_id)?
    } else {
        "未关联事项".to_string()
    };
    let id = Uuid::now_v7().to_string();
    let now = now_millis();
    let completed_task_outbox = request
        .complete_task
        .then(|| {
            request
                .task_id
                .as_ref()
                .zip(request.task_expected_version)
                .map(|(task_id, version)| (task_id.clone(), version))
        })
        .flatten();
    let mut completed_occurrence_outbox = Vec::<(String, Option<i64>)>::new();
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO time_entries(
               id, workspace_id, work_date, kind, source_type, state, default_task_id,
               label_snapshot, started_at, ended_at, duration_seconds, note,
               created_at, updated_at, version, created_by_device_id, updated_by_device_id
             ) VALUES (?1, ?2, ?3, 'work', 'manual', 'ended', ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10, 1, ?11, ?11)",
            params![id, workspace_id, request.work_date, request.task_id, label, started_at, ended_at, duration_seconds, request.note, now, device_id],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO time_segments(id, workspace_id, entry_id, sequence_no, started_at, ended_at, duration_seconds)
             VALUES (?1, ?2, ?3, 1, ?4, ?5, ?6)",
            params![Uuid::now_v7().to_string(), workspace_id, id, started_at, ended_at, duration_seconds],
        )
        .map_err(|error| error.to_string())?;
    if let Some(task_id) = request.task_id {
        let minutes = settlement_minutes(duration_seconds);
        if minutes > 0 {
            if request.complete_task {
                validate_task_completion(
                    &transaction,
                    &workspace_id,
                    &task_id,
                    request.task_expected_version,
                    &request.work_date,
                )?;
            }
            crate::recurring::ensure_occurrence_if_recurring(
                &transaction,
                &workspace_id,
                &task_id,
                &request.work_date,
                now,
            )?;
            transaction
                .execute(
                    "INSERT INTO time_allocations(id, workspace_id, entry_id, task_id, minutes, created_at, updated_at, version)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, 1)",
                    params![Uuid::now_v7().to_string(), workspace_id, id, task_id, minutes, now],
                )
                .map_err(|error| error.to_string())?;
            if request.complete_task {
                if let Some(occurrence_outbox) = complete_task_if_open(
                    &transaction,
                    &workspace_id,
                    &task_id,
                    request.task_expected_version,
                    &request.work_date,
                    now,
                )? {
                    completed_occurrence_outbox.push(occurrence_outbox);
                }
            }
        }
    }
    bump_revision(&transaction)?;
    let result = load_entry(&transaction, &id, now)?;
    save_processed_result(
        &transaction,
        &workspace_id,
        &device_id,
        &request.client_request_id,
        "time_entry_create_manual",
        &result,
    )?;
    if let Some((state, operation_type, operation_id)) = cloud_operation {
        crate::cloud_sync::enqueue_entity_in_transaction(
            &transaction,
            state,
            operation_type,
            "time_entry",
            Some(&id),
            None,
            operation_id,
        )?;
        if let Some((task_id, base_version)) = completed_task_outbox {
            crate::cloud_sync::enqueue_entity_in_transaction(
                &transaction,
                state,
                "task_set_completed_from_time_entry",
                "task",
                Some(&task_id),
                Some(base_version),
                None,
            )?;
        }
        for (entity_id, base_version) in completed_occurrence_outbox {
            crate::cloud_sync::enqueue_entity_in_transaction(
                &transaction,
                state,
                "task_occurrence_set_completed_from_time_entry",
                "task_occurrence",
                Some(&entity_id),
                base_version,
                None,
            )?;
        }
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

#[cfg(test)]
pub fn update_time_entry(
    database: &Database,
    request: TimeEntryUpdateRequest,
) -> Result<TimeEntryDto, String> {
    update_time_entry_with_cloud_operation(database, request, None)
}

fn update_time_entry_with_cloud_operation(
    database: &Database,
    request: TimeEntryUpdateRequest,
    cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str)>,
) -> Result<TimeEntryDto, String> {
    if request.ended_at <= request.started_at {
        return Err("VALIDATION_ERROR: 结束时间必须晚于开始时间".to_string());
    }
    let duration_seconds = (request.ended_at - request.started_at) / 1_000;
    let minutes = settlement_minutes(duration_seconds);
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let state = entry_state(
        &transaction,
        &workspace_id,
        &request.entry_id,
        request.expected_version,
    )?;
    if state != "ended" {
        return Err("VALIDATION_ERROR: 只能修正已结束的时间记录".to_string());
    }
    let allocated: i64 = transaction
        .query_row(
            "SELECT COALESCE(SUM(minutes), 0) FROM time_allocations WHERE entry_id = ?1",
            [&request.entry_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if allocated > minutes {
        return Err(format!(
            "ALLOCATION_EXCEEDS_DURATION: 分配时间超出修正后的原始时间 {} 分钟",
            allocated - minutes
        ));
    }
    let now = now_millis();
    transaction
        .execute(
            "UPDATE time_entries SET started_at = ?1, ended_at = ?2, duration_seconds = ?3,
                    note = ?4, updated_at = ?5, version = version + 1
             WHERE id = ?6 AND version = ?7 AND state = 'ended' AND deleted_at IS NULL",
            params![
                request.started_at,
                request.ended_at,
                duration_seconds,
                request.note,
                now,
                request.entry_id,
                request.expected_version
            ],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "DELETE FROM time_segments WHERE entry_id = ?1",
            [&request.entry_id],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO time_segments(id, workspace_id, entry_id, sequence_no, started_at, ended_at, duration_seconds)
             VALUES (?1, ?2, ?3, 1, ?4, ?5, ?6)",
            params![Uuid::now_v7().to_string(), workspace_id, request.entry_id, request.started_at, request.ended_at, duration_seconds],
        )
        .map_err(|error| error.to_string())?;
    clear_workday_settlement(&transaction, &workspace_id, &request.entry_id)?;
    bump_revision(&transaction)?;
    let result = load_entry(&transaction, &request.entry_id, now)?;
    if let Some((state, operation_type)) = cloud_operation {
        crate::cloud_sync::enqueue_entity_in_transaction(
            &transaction,
            state,
            operation_type,
            "time_entry",
            Some(&request.entry_id),
            Some(request.expected_version),
            None,
        )?;
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

#[cfg(test)]
pub fn update_time_entry_disposition(
    database: &Database,
    request: TimeEntryDispositionRequest,
) -> Result<TimeEntryDto, String> {
    update_time_entry_disposition_with_cloud_operation(database, request, None)
}

fn update_time_entry_disposition_with_cloud_operation(
    database: &Database,
    request: TimeEntryDispositionRequest,
    cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str)>,
) -> Result<TimeEntryDto, String> {
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let current = load_entry(&connection, &request.entry_id, now_millis())?;
    if current.version != request.expected_version {
        return Err("VERSION_CONFLICT: 时间记录已被更新".to_string());
    }
    if current.state != "ended" {
        return Err("VALIDATION_ERROR: 只能处理已结束的时间记录".to_string());
    }
    if request.disposition != "break" && request.disposition != "discard" {
        return Err("VALIDATION_ERROR: 处理方式必须是休息或无效".to_string());
    }

    let now = now_millis();
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "DELETE FROM time_allocations WHERE entry_id = ?1",
            [&request.entry_id],
        )
        .map_err(|error| error.to_string())?;
    if request.disposition == "break" {
        transaction
            .execute(
                "UPDATE time_entries
                 SET kind = 'break', default_task_id = NULL, label_snapshot = '休息时间',
                     note = '已标记为休息时间', updated_at = ?1, version = version + 1
                 WHERE id = ?2 AND version = ?3 AND deleted_at IS NULL",
                params![now, request.entry_id, request.expected_version],
            )
            .map_err(|error| error.to_string())?;
    } else {
        transaction
            .execute(
                "UPDATE time_entries
                 SET default_task_id = NULL, note = '已标记为无效时间',
                     deleted_at = ?1, updated_at = ?1, version = version + 1
                 WHERE id = ?2 AND version = ?3 AND deleted_at IS NULL",
                params![now, request.entry_id, request.expected_version],
            )
            .map_err(|error| error.to_string())?;
    }
    clear_workday_settlement(&transaction, &workspace_id, &request.entry_id)?;
    bump_revision(&transaction)?;
    let result = if request.disposition == "break" {
        load_entry(&transaction, &request.entry_id, now)?
    } else {
        let mut result = current;
        result.version += 1;
        result.default_task_id = None;
        result.allocated_minutes = 0;
        result.allocations.clear();
        result.note = Some("已标记为无效时间".to_string());
        result
    };
    if let Some((state, operation_type)) = cloud_operation {
        crate::cloud_sync::enqueue_entity_in_transaction(
            &transaction,
            state,
            operation_type,
            "time_entry",
            Some(&request.entry_id),
            Some(request.expected_version),
            None,
        )?;
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

#[cfg(test)]
pub fn replace_allocations(
    database: &Database,
    request: AllocationReplaceRequest,
) -> Result<TimeEntryDto, String> {
    replace_allocations_with_cloud_operation(database, request, None)
}

fn replace_allocations_with_cloud_operation(
    database: &Database,
    request: AllocationReplaceRequest,
    cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str)>,
) -> Result<TimeEntryDto, String> {
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let current = load_entry(&connection, &request.entry_id, now_millis())?;
    if current.version != request.expected_version {
        return Err("VERSION_CONFLICT: 时间记录已被更新".to_string());
    }
    if current.state != "ended" {
        return Err("VALIDATION_ERROR: 只能分配已结束的时间记录".to_string());
    }
    if current.kind != "work" && !(current.kind == "break" && current.source_type == "unassigned") {
        return Err("VALIDATION_ERROR: 只能分配工作记录或未归属处理记录".to_string());
    }
    let mut merged = HashMap::<String, (i64, Option<String>, bool, Option<i64>)>::new();
    for allocation in request.allocations {
        if allocation.minutes < 0 {
            return Err("VALIDATION_ERROR: 分配分钟不能为负数".to_string());
        }
        if allocation.minutes == 0 {
            continue;
        }
        ensure_selectable_task(&connection, &workspace_id, &allocation.task_id)?;
        let item = merged.entry(allocation.task_id).or_insert((
            0,
            allocation.note.clone(),
            false,
            allocation.task_expected_version,
        ));
        item.0 += allocation.minutes;
        if allocation.note.is_some() {
            item.1 = allocation.note;
        }
        item.2 |= allocation.complete_task;
        if let (Some(existing), Some(candidate)) = (item.3, allocation.task_expected_version) {
            if existing != candidate {
                return Err("VERSION_CONFLICT: 同一事项的版本信息不一致".to_string());
            }
        } else if item.3.is_none() {
            item.3 = allocation.task_expected_version;
        }
    }
    let total: i64 = merged.values().map(|(minutes, _, _, _)| *minutes).sum();
    if total > current.settlement_minutes {
        return Err(format!(
            "ALLOCATION_EXCEEDS_DURATION: 分配时间超出原始时间 {} 分钟",
            total - current.settlement_minutes
        ));
    }
    let completed_task_outbox = merged
        .iter()
        .filter_map(|(task_id, (_, _, complete_task, expected_version))| {
            complete_task
                .then_some(*expected_version)
                .flatten()
                .map(|version| (task_id.clone(), version))
        })
        .collect::<Vec<_>>();
    let now = now_millis();
    let mut completed_occurrence_outbox = Vec::<(String, Option<i64>)>::new();
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    for (task_id, (_, _, complete_task, expected_version)) in &merged {
        if *complete_task {
            validate_task_completion(
                &transaction,
                &workspace_id,
                task_id,
                *expected_version,
                &current.work_date,
            )?;
        }
    }
    transaction
        .execute(
            "DELETE FROM time_allocations WHERE entry_id = ?1",
            [&request.entry_id],
        )
        .map_err(|error| error.to_string())?;
    for (task_id, (minutes, note, complete_task, expected_version)) in merged {
        crate::recurring::ensure_occurrence_if_recurring(
            &transaction,
            &workspace_id,
            &task_id,
            &current.work_date,
            now,
        )?;
        transaction
            .execute(
                "INSERT INTO time_allocations(id, workspace_id, entry_id, task_id, minutes, note, created_at, updated_at, version)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, 1)",
                params![Uuid::now_v7().to_string(), workspace_id, request.entry_id, task_id, minutes, note, now],
            )
            .map_err(|error| error.to_string())?;
        if complete_task {
            if let Some(occurrence_outbox) = complete_task_if_open(
                &transaction,
                &workspace_id,
                &task_id,
                expected_version,
                &current.work_date,
                now,
            )? {
                completed_occurrence_outbox.push(occurrence_outbox);
            }
        }
    }
    let should_convert_to_work = current.kind == "break" && total > 0;
    transaction
        .execute(
            "UPDATE time_entries
             SET kind = CASE WHEN ?4 THEN 'work' ELSE kind END,
                 label_snapshot = CASE WHEN ?4 THEN '未归属时间重新分配' ELSE label_snapshot END,
                 note = CASE WHEN ?4 THEN '从休息或无效时间重新分配' ELSE note END,
                 updated_at = ?1,
                 version = version + 1
             WHERE id = ?2 AND version = ?3 AND deleted_at IS NULL",
            params![
                now,
                request.entry_id,
                request.expected_version,
                should_convert_to_work
            ],
        )
        .map_err(|error| error.to_string())?;
    clear_workday_settlement(&transaction, &workspace_id, &request.entry_id)?;
    bump_revision(&transaction)?;
    let result = load_entry(&transaction, &request.entry_id, now)?;
    if let Some((state, operation_type)) = cloud_operation {
        let time_entry_operation_id = crate::cloud_sync::enqueue_entity_in_transaction(
            &transaction,
            state,
            operation_type,
            "time_entry",
            Some(&request.entry_id),
            Some(request.expected_version),
            None,
        )?;
        for (task_id, base_version) in completed_task_outbox {
            crate::cloud_sync::enqueue_entity_in_transaction_with_dependency(
                &transaction,
                state,
                "task_set_completed_from_allocation",
                "task",
                Some(&task_id),
                Some(base_version),
                None,
                time_entry_operation_id.as_deref(),
            )?;
        }
        for (entity_id, base_version) in completed_occurrence_outbox {
            crate::cloud_sync::enqueue_entity_in_transaction_with_dependency(
                &transaction,
                state,
                "task_occurrence_set_completed_from_allocation",
                "task_occurrence",
                Some(&entity_id),
                base_version,
                None,
                time_entry_operation_id.as_deref(),
            )?;
        }
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

fn replace_allocations_with_hooks_and_cloud_operation(
    database: &Database,
    request: AllocationReplaceRequest,
    cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str)>,
) -> Result<TimeEntryDto, String> {
    let work_date = get_time_entry_by_id(database, &request.entry_id)?.work_date;
    let candidates = request
        .allocations
        .iter()
        .filter(|allocation| allocation.complete_task)
        .map(|allocation| {
            crate::tasks::task_is_completed(database, &allocation.task_id, Some(&work_date))
                .map(|completed| (allocation.task_id.clone(), completed))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let result = replace_allocations_with_cloud_operation(database, request, cloud_operation)?;
    for (task_id, was_completed) in candidates {
        if !was_completed && crate::tasks::task_is_completed(database, &task_id, Some(&work_date))?
        {
            crate::tasks::publish_task_completed_hook(database, &task_id, Some(&work_date), None);
        }
    }
    Ok(result)
}

fn load_entry(
    connection: &rusqlite::Connection,
    entry_id: &str,
    current_time: i64,
) -> Result<TimeEntryDto, String> {
    let mut entry: TimeEntryDto = connection
        .query_row(
            "SELECT id, work_date, label_snapshot, started_at, ended_at, duration_seconds,
                    default_task_id, note, kind, source_type, state, version,
                    timer_chain_id, previous_entry_id, split_boundary_at
             FROM time_entries WHERE id = ?1 AND deleted_at IS NULL",
            [entry_id],
            |row| {
                Ok(TimeEntryDto {
                    id: row.get(0)?,
                    work_date: row.get(1)?,
                    label: row.get(2)?,
                    started_at: row.get(3)?,
                    ended_at: row.get(4)?,
                    duration_seconds: row.get(5)?,
                    settlement_minutes: 0,
                    allocated_minutes: 0,
                    default_task_id: row.get(6)?,
                    note: row.get(7)?,
                    kind: row.get(8)?,
                    source_type: row.get(9)?,
                    state: row.get(10)?,
                    version: row.get(11)?,
                    timer_chain_id: row.get(12)?,
                    previous_entry_id: row.get(13)?,
                    split_boundary_at: row.get(14)?,
                    allocations: Vec::new(),
                })
            },
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "NOT_FOUND: 时间记录不存在".to_string())?;
    if entry.state == "running" {
        let open_started_at: Option<i64> = connection
            .query_row(
                "SELECT started_at FROM time_segments WHERE entry_id = ?1 AND ended_at IS NULL",
                [entry_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        if let Some(started_at) = open_started_at {
            entry.duration_seconds += (current_time - started_at).max(0) / 1_000;
        }
    }
    let mut statement = connection
        .prepare(
            "SELECT a.id, a.task_id, COALESCE(t.title, '已删除事项'), a.minutes, a.note, a.version
             FROM time_allocations a LEFT JOIN tasks t ON t.id = a.task_id
             WHERE a.entry_id = ?1 ORDER BY a.created_at, a.id",
        )
        .map_err(|error| error.to_string())?;
    entry.allocations = statement
        .query_map([entry_id], |row| {
            Ok(TimeAllocationDto {
                id: row.get(0)?,
                task_id: row.get(1)?,
                task_title: row.get(2)?,
                minutes: row.get(3)?,
                note: row.get(4)?,
                version: row.get(5)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    entry.settlement_minutes = chain_settlement_minutes(connection, &entry)?
        .unwrap_or_else(|| settlement_minutes(entry.duration_seconds));
    entry.allocated_minutes = entry.allocations.iter().map(|item| item.minutes).sum();
    Ok(entry)
}

fn chain_settlement_minutes(
    connection: &rusqlite::Connection,
    entry: &TimeEntryDto,
) -> Result<Option<i64>, String> {
    if entry.source_type != "timer" || entry.state != "ended" {
        return Ok(None);
    }
    let chain_id = entry.timer_chain_id.as_deref().unwrap_or(&entry.id);
    let active_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM time_entries
             WHERE COALESCE(timer_chain_id,id)=?1 AND state IN ('running','paused')
               AND deleted_at IS NULL",
            [chain_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if active_count > 0 {
        return Ok(None);
    }
    let mut statement = connection
        .prepare(
            "SELECT id,work_date,started_at,duration_seconds FROM time_entries
             WHERE COALESCE(timer_chain_id,id)=?1 AND source_type='timer'
               AND state='ended' AND deleted_at IS NULL
             ORDER BY work_date,started_at,id",
        )
        .map_err(|error| error.to_string())?;
    let slices = statement
        .query_map([chain_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(allocate_chain_minutes(&slices).remove(&entry.id))
}

fn allocate_chain_minutes(slices: &[(String, String, i64, i64)]) -> HashMap<String, i64> {
    let total_seconds = slices
        .iter()
        .map(|(_, _, _, seconds)| (*seconds).max(0))
        .sum::<i64>();
    let total_minutes = settlement_minutes(total_seconds);
    let mut result = slices
        .iter()
        .map(|(id, _, _, seconds)| (id.clone(), (*seconds).max(0) / 60))
        .collect::<HashMap<_, _>>();
    let base_minutes = result.values().sum::<i64>();
    let mut remaining = (total_minutes - base_minutes).max(0);
    let mut ranked = slices
        .iter()
        .filter(|(_, _, _, seconds)| *seconds > 0 && *seconds % 60 > 0)
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        (right.3 % 60)
            .cmp(&(left.3 % 60))
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.cmp(&right.2))
            .then_with(|| left.0.cmp(&right.0))
    });
    for (id, _, _, _) in ranked {
        if remaining == 0 {
            break;
        }
        *result.entry(id.clone()).or_default() += 1;
        remaining -= 1;
    }
    result
}

fn entry_state(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    entry_id: &str,
    expected_version: i64,
) -> Result<String, String> {
    connection
        .query_row(
            "SELECT state FROM time_entries
             WHERE id = ?1 AND workspace_id = ?2 AND version = ?3 AND deleted_at IS NULL",
            params![entry_id, workspace_id, expected_version],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "VERSION_CONFLICT: 计时状态已变化".to_string())
}

fn resolve_active_timer_request(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    requested_entry_id: &str,
    expected_version: i64,
    now: i64,
) -> Result<(String, i64), String> {
    let requested: Option<(String, String, i64)> = transaction
        .query_row(
            "SELECT COALESCE(timer_chain_id,id),state,version FROM time_entries
             WHERE id=?1 AND workspace_id=?2 AND source_type='timer' AND deleted_at IS NULL",
            params![requested_entry_id, workspace_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some((chain_id, requested_state, requested_version)) = requested else {
        return Err("NOT_FOUND: 计时记录不存在".to_string());
    };
    if matches!(requested_state.as_str(), "running" | "paused")
        && requested_version != expected_version
    {
        return Err("VERSION_CONFLICT: 计时状态已变化".to_string());
    }

    coordinate_active_timer_in_transaction(transaction, workspace_id, now)?;
    transaction
        .query_row(
            "SELECT id,version FROM time_entries
             WHERE workspace_id=?1 AND COALESCE(timer_chain_id,id)=?2
               AND state IN ('running','paused') AND deleted_at IS NULL
             ORDER BY started_at DESC LIMIT 1",
            params![workspace_id, chain_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "VALIDATION_ERROR: 当前计时已经结束".to_string())
}

fn update_entry_state(
    transaction: &Transaction<'_>,
    entry_id: &str,
    expected_version: i64,
    state: &str,
    ended_at: Option<i64>,
    duration_seconds: i64,
    now: i64,
) -> Result<(), String> {
    let changed = transaction
        .execute(
            "UPDATE time_entries SET state = ?1, ended_at = ?2, duration_seconds = ?3,
                    last_continuous_at = ?4, updated_at = ?4, version = version + 1
             WHERE id = ?5 AND version = ?6 AND deleted_at IS NULL",
            params![
                state,
                ended_at,
                duration_seconds,
                now,
                entry_id,
                expected_version
            ],
        )
        .map_err(|error| error.to_string())?;
    if changed == 0 {
        Err("VERSION_CONFLICT: 计时状态已变化".to_string())
    } else {
        Ok(())
    }
}

fn close_open_segment(
    transaction: &Transaction<'_>,
    entry_id: &str,
    ended_at: i64,
) -> Result<(), String> {
    let started_at: i64 = transaction
        .query_row(
            "SELECT started_at FROM time_segments WHERE entry_id = ?1 AND ended_at IS NULL",
            [entry_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "VALIDATION_ERROR: 找不到运行中的有效时间段".to_string())?;
    transaction
        .execute(
            "UPDATE time_segments SET ended_at = ?1, duration_seconds = MAX(0, (?1 - started_at) / 1000)
             WHERE entry_id = ?2 AND ended_at IS NULL",
            params![ended_at, entry_id],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())?;
    if ended_at < started_at {
        return Err("VALIDATION_ERROR: 系统时间早于计时开始时间".to_string());
    }
    Ok(())
}

fn segment_duration_sum(connection: &rusqlite::Connection, entry_id: &str) -> Result<i64, String> {
    connection
        .query_row(
            "SELECT COALESCE(SUM(duration_seconds), 0) FROM time_segments WHERE entry_id = ?1",
            [entry_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())
}

pub(crate) fn coordinate_active_timer(database: &Database, now: i64) -> Result<(), String> {
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    coordinate_active_timer_in_transaction(&transaction, &workspace_id, now)?;
    transaction.commit().map_err(|error| error.to_string())
}

fn coordinate_active_timer_in_transaction(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    now: i64,
) -> Result<Option<String>, String> {
    let cloud_state = crate::supabase::storage_mode_from_connection(transaction, false).ok();
    type ActiveTimer = (
        String,
        String,
        String,
        String,
        Option<String>,
        String,
        Option<String>,
        i64,
        String,
        i64,
        i64,
    );
    let active: Option<ActiveTimer> = transaction
        .query_row(
            "SELECT id,work_date,state,COALESCE(timer_chain_id,id),default_task_id,
                    label_snapshot,note,started_at,kind,version,
                    COALESCE(last_continuous_at,updated_at,started_at)
             FROM time_entries
             WHERE workspace_id=?1 AND state IN ('running','paused') AND deleted_at IS NULL
             LIMIT 1",
            [workspace_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                    row.get(10)?,
                ))
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some((
        entry_id,
        work_date,
        state,
        chain_id,
        default_task_id,
        label,
        note,
        _started_at,
        kind,
        version,
        last_continuous_at,
    )) = active
    else {
        return Ok(None);
    };
    let timezone = crate::work_calendar::workspace_timezone(transaction, workspace_id)?;
    let current_date = crate::work_calendar::work_date_at(timezone, now)?;
    let interrupted = now.saturating_sub(last_continuous_at) > CONTINUITY_GRACE_MILLIS;
    if current_date == work_date {
        if interrupted && state == "running" {
            let open_started_at: i64 = transaction
                .query_row(
                    "SELECT started_at FROM time_segments WHERE entry_id=?1 AND ended_at IS NULL",
                    [&entry_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "VALIDATION_ERROR: 找不到运行中的有效时间段".to_string())?;
            let confirmed_end = last_continuous_at.min(now).max(open_started_at);
            close_open_segment(transaction, &entry_id, confirmed_end)?;
            let duration = segment_duration_sum(transaction, &entry_id)?;
            let sequence: i64 = transaction
                .query_row(
                    "SELECT COALESCE(MAX(sequence_no),0)+1 FROM time_segments WHERE entry_id=?1",
                    [&entry_id],
                    |row| row.get(0),
                )
                .map_err(|error| error.to_string())?;
            transaction
                .execute(
                    "INSERT INTO time_segments(id,workspace_id,entry_id,sequence_no,started_at,duration_seconds)
                     VALUES (?1,?2,?3,?4,?5,0)",
                    params![Uuid::now_v7().to_string(), workspace_id, entry_id, sequence, now],
                )
                .map_err(|error| error.to_string())?;
            transaction
                .execute(
                    "UPDATE time_entries SET duration_seconds=?1,last_continuous_at=?2,
                            updated_at=?2,version=version+1 WHERE id=?3",
                    params![duration, now, entry_id],
                )
                .map_err(|error| error.to_string())?;
            bump_revision(transaction)?;
            enqueue_timer_coordination_rows(
                transaction,
                cloud_state.as_ref(),
                workspace_id,
                &[(&entry_id, version, "timer_continuity_close", None)],
            )?;
        } else {
            transaction
                .execute(
                    "UPDATE time_entries SET last_continuous_at=?1 WHERE id=?2",
                    params![now, entry_id],
                )
                .map_err(|error| error.to_string())?;
        }
        record_runtime_heartbeat(transaction, workspace_id, now)?;
        return Ok(Some(entry_id));
    }

    let device_id = device_id(transaction, workspace_id)?;
    if state == "paused" {
        let boundary = crate::work_calendar::day_bounds(timezone, &work_date)?
            .end_at
            .min(now);
        transaction
            .execute(
                "UPDATE time_entries SET state='ended',ended_at=?1,split_boundary_at=?1,
                        last_continuous_at=?1,updated_at=?2,version=version+1
                 WHERE id=?3 AND version=?4",
                params![boundary, now, entry_id, version],
            )
            .map_err(|error| error.to_string())?;
        let next_id = insert_timer_slice(
            transaction,
            workspace_id,
            &device_id,
            &chain_id,
            Some(&entry_id),
            &current_date,
            &kind,
            "paused",
            default_task_id.as_deref(),
            &label,
            note.as_deref(),
            now,
            None,
            now,
            now,
        )?;
        enqueue_timer_coordination_rows(
            transaction,
            cloud_state.as_ref(),
            workspace_id,
            &[
                (&entry_id, version, "timer_rollover_close", None),
                (
                    &next_id,
                    0,
                    "timer_rollover_create",
                    Some(entry_id.as_str()),
                ),
            ],
        )?;
        record_runtime_heartbeat(transaction, workspace_id, now)?;
        bump_revision(transaction)?;
        return Ok(Some(next_id));
    }

    let open_started_at: i64 = transaction
        .query_row(
            "SELECT started_at FROM time_segments WHERE entry_id=?1 AND ended_at IS NULL",
            [&entry_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "VALIDATION_ERROR: 找不到运行中的有效时间段".to_string())?;
    if interrupted {
        let confirmed_end = last_continuous_at.min(now).max(open_started_at);
        close_open_segment(transaction, &entry_id, confirmed_end)?;
        let duration = segment_duration_sum(transaction, &entry_id)?;
        transaction
            .execute(
                "UPDATE time_entries SET state='ended',ended_at=?1,duration_seconds=?2,
                        split_boundary_at=?1,last_continuous_at=?1,updated_at=?3,version=version+1
                 WHERE id=?4 AND version=?5",
                params![confirmed_end, duration, now, entry_id, version],
            )
            .map_err(|error| error.to_string())?;
        let next_id = insert_timer_slice(
            transaction,
            workspace_id,
            &device_id,
            &chain_id,
            Some(&entry_id),
            &current_date,
            &kind,
            "running",
            default_task_id.as_deref(),
            &label,
            note.as_deref(),
            now,
            None,
            now,
            now,
        )?;
        enqueue_timer_coordination_rows(
            transaction,
            cloud_state.as_ref(),
            workspace_id,
            &[
                (&entry_id, version, "timer_interruption_close", None),
                (
                    &next_id,
                    0,
                    "timer_rollover_create",
                    Some(entry_id.as_str()),
                ),
            ],
        )?;
        record_runtime_heartbeat(transaction, workspace_id, now)?;
        bump_revision(transaction)?;
        return Ok(Some(next_id));
    }

    let slices = crate::work_calendar::split_interval_by_day(timezone, open_started_at, now)?;
    if slices.len() < 2 {
        return Err("VALIDATION_ERROR: 计时日期与有效时间段边界不一致".to_string());
    }
    let first = &slices[0];
    close_open_segment(transaction, &entry_id, first.ended_at)?;
    let duration = segment_duration_sum(transaction, &entry_id)?;
    transaction
        .execute(
            "UPDATE time_entries SET state='ended',ended_at=?1,duration_seconds=?2,
                    split_boundary_at=?1,last_continuous_at=?1,updated_at=?3,version=version+1
             WHERE id=?4 AND version=?5",
            params![first.ended_at, duration, now, entry_id, version],
        )
        .map_err(|error| error.to_string())?;

    let closed_entry_id = entry_id.clone();
    let mut previous_id = entry_id;
    let mut coordinated_rows = vec![(
        closed_entry_id.clone(),
        version,
        "timer_rollover_close",
        None,
    )];
    for (index, slice) in slices.iter().enumerate().skip(1) {
        let is_last = index + 1 == slices.len();
        let next_id = insert_timer_slice(
            transaction,
            workspace_id,
            &device_id,
            &chain_id,
            Some(&previous_id),
            &slice.work_date,
            &kind,
            if is_last { "running" } else { "ended" },
            default_task_id.as_deref(),
            &label,
            note.as_deref(),
            slice.started_at,
            (!is_last).then_some(slice.ended_at),
            slice.started_at,
            if is_last { now } else { slice.ended_at },
        )?;
        coordinated_rows.push((
            next_id.clone(),
            0,
            "timer_rollover_create",
            Some(previous_id.clone()),
        ));
        previous_id = next_id;
    }
    let row_refs = coordinated_rows
        .iter()
        .map(|(id, base_version, operation_type, predecessor)| {
            (
                id.as_str(),
                *base_version,
                *operation_type,
                predecessor.as_deref(),
            )
        })
        .collect::<Vec<_>>();
    enqueue_timer_coordination_rows(transaction, cloud_state.as_ref(), workspace_id, &row_refs)?;
    record_runtime_heartbeat(transaction, workspace_id, now)?;
    bump_revision(transaction)?;
    Ok(Some(previous_id))
}

fn stable_calendar_operation_id(chain_id: &str, work_date: &str, action: &str) -> String {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("timegenie:calendar-timer:{chain_id}:{work_date}:{action}").as_bytes(),
    )
    .to_string()
}

fn enqueue_timer_coordination_rows(
    transaction: &Transaction<'_>,
    cloud_state: Option<&crate::supabase::StorageModeSnapshot>,
    workspace_id: &str,
    rows: &[(&str, i64, &str, Option<&str>)],
) -> Result<(), String> {
    let Some(cloud_state) = cloud_state.filter(|state| state.mode == "cloud") else {
        return Ok(());
    };
    let mut operation_by_entry = std::collections::HashMap::<String, String>::new();
    for (entry_id, base_version, operation_type, predecessor_id) in rows {
        let (chain_id, work_date): (String, String) = transaction
            .query_row(
                "SELECT COALESCE(timer_chain_id,id),work_date FROM time_entries WHERE workspace_id=?1 AND id=?2",
                params![workspace_id, entry_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|error| error.to_string())?;
        let operation_id = stable_calendar_operation_id(&chain_id, &work_date, operation_type);
        let dependency = predecessor_id
            .and_then(|id| operation_by_entry.get(id))
            .map(String::as_str);
        crate::cloud_sync::enqueue_entity_in_transaction_with_dependency(
            transaction,
            cloud_state,
            operation_type,
            "time_entry",
            Some(entry_id),
            (*base_version > 0).then_some(*base_version),
            Some(&operation_id),
            dependency,
        )?;
        operation_by_entry.insert((*entry_id).to_string(), operation_id);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn insert_timer_slice(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    device_id: &str,
    chain_id: &str,
    previous_entry_id: Option<&str>,
    work_date: &str,
    kind: &str,
    state: &str,
    default_task_id: Option<&str>,
    label: &str,
    note: Option<&str>,
    started_at: i64,
    ended_at: Option<i64>,
    split_boundary_at: i64,
    last_continuous_at: i64,
) -> Result<String, String> {
    let entry_id = Uuid::now_v7().to_string();
    let duration_seconds = ended_at
        .map(|ended| ended.saturating_sub(started_at) / 1_000)
        .unwrap_or(0);
    transaction
        .execute(
            "INSERT INTO time_entries(
               id,workspace_id,work_date,kind,source_type,state,default_task_id,
               label_snapshot,started_at,ended_at,duration_seconds,note,created_at,updated_at,
               version,created_by_device_id,updated_by_device_id,timer_chain_id,
               previous_entry_id,split_boundary_at,last_continuous_at
             ) VALUES (?1,?2,?3,?4,'timer',?5,?6,?7,?8,?9,?10,?11,?8,?12,1,?13,?13,?14,?15,?16,?17)",
            params![
                entry_id, workspace_id, work_date, kind, state, default_task_id, label,
                started_at, ended_at, duration_seconds, note, last_continuous_at, device_id,
                chain_id, previous_entry_id, split_boundary_at, last_continuous_at,
            ],
        )
        .map_err(map_active_timer_error)?;
    if state == "running" {
        transaction
            .execute(
                "INSERT INTO time_segments(id,workspace_id,entry_id,sequence_no,started_at,duration_seconds)
                 VALUES (?1,?2,?3,1,?4,0)",
                params![Uuid::now_v7().to_string(), workspace_id, entry_id, started_at],
            )
            .map_err(|error| error.to_string())?;
    } else if ended_at.is_some() {
        transaction
            .execute(
                "INSERT INTO time_segments(id,workspace_id,entry_id,sequence_no,started_at,ended_at,duration_seconds)
                 VALUES (?1,?2,?3,1,?4,?5,?6)",
                params![Uuid::now_v7().to_string(), workspace_id, entry_id, started_at, ended_at, duration_seconds],
            )
            .map_err(|error| error.to_string())?;
    }
    if let Some(task_id) = default_task_id {
        crate::recurring::ensure_occurrence_if_recurring(
            transaction,
            workspace_id,
            task_id,
            work_date,
            last_continuous_at,
        )?;
    }
    Ok(entry_id)
}

fn record_runtime_heartbeat(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    now: i64,
) -> Result<(), String> {
    transaction
        .execute(
            "INSERT INTO tracking_runtime_state(workspace_id,last_heartbeat_at,updated_at)
             VALUES (?1,?2,?2)
             ON CONFLICT(workspace_id) DO UPDATE SET
               last_heartbeat_at=excluded.last_heartbeat_at,updated_at=excluded.updated_at",
            params![workspace_id, now],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn selectable_task_title(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    task_id: &str,
) -> Result<String, String> {
    ensure_selectable_task(connection, workspace_id, task_id)?;
    connection
        .query_row("SELECT title FROM tasks WHERE id = ?1", [task_id], |row| {
            row.get(0)
        })
        .map_err(|error| error.to_string())
}

fn ensure_selectable_task(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    task_id: &str,
) -> Result<(), String> {
    let selectable = selectable_task_state(connection, workspace_id, task_id)?;
    match selectable {
        Some(1) => Ok(()),
        Some(_) => Err("TASK_NOT_SELECTABLE: 父事项不能直接计时或分配工时".to_string()),
        None => Err("NOT_FOUND: 事项不存在".to_string()),
    }
}

fn is_selectable_task(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    task_id: &str,
) -> Result<bool, String> {
    selectable_task_state(connection, workspace_id, task_id).map(|value| value == Some(1))
}

fn selectable_task_state(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    task_id: &str,
) -> Result<Option<i64>, String> {
    connection
        .query_row(
            "SELECT CASE WHEN EXISTS(
               SELECT 1 FROM tasks child WHERE child.parent_id = task.id AND child.deleted_at IS NULL
             ) THEN 0 ELSE 1 END
             FROM tasks task WHERE task.id = ?1 AND task.workspace_id = ?2 AND task.deleted_at IS NULL",
            params![task_id, workspace_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())
}

pub(crate) fn validate_task_completion(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    task_id: &str,
    expected_version: Option<i64>,
    work_date: &str,
) -> Result<(), String> {
    let state: Option<(String, i64)> = connection
        .query_row(
            "SELECT status, version FROM tasks
             WHERE id = ?1 AND workspace_id = ?2 AND deleted_at IS NULL",
            params![task_id, workspace_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some((status, version)) = state else {
        return Err("NOT_FOUND: 事项不存在".to_string());
    };
    if crate::recurring::get_active_rule(connection, workspace_id, task_id)?.is_some()
        || crate::recurring::get_rule_for_date(connection, workspace_id, task_id, work_date)?
            .is_some()
    {
        match expected_version {
            Some(expected) if expected == version => return Ok(()),
            Some(_) => return Err("VERSION_CONFLICT: 事项状态已变化，请刷新后重试".to_string()),
            None => return Err("VALIDATION_ERROR: 完成事项时缺少事项版本".to_string()),
        }
    }
    if status == "done" {
        return Ok(());
    }
    match expected_version {
        Some(expected) if expected == version => Ok(()),
        Some(_) => Err("VERSION_CONFLICT: 事项状态已变化，请刷新后重试".to_string()),
        None => Err("VALIDATION_ERROR: 完成事项时缺少事项版本".to_string()),
    }
}

pub(crate) fn complete_task_if_open(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    task_id: &str,
    expected_version: Option<i64>,
    work_date: &str,
    now: i64,
) -> Result<Option<(String, Option<i64>)>, String> {
    if crate::recurring::get_active_rule(transaction, workspace_id, task_id)?.is_some()
        || crate::recurring::get_rule_for_date(transaction, workspace_id, task_id, work_date)?
            .is_some()
    {
        let previous =
            crate::recurring::occurrence_for_date(transaction, workspace_id, task_id, work_date)?;
        let occurrence = crate::recurring::set_occurrence_status(
            transaction,
            workspace_id,
            task_id,
            work_date,
            true,
            None,
            now,
        )?;
        if previous
            .as_ref()
            .is_some_and(|item| item.status == occurrence.status)
        {
            return Ok(None);
        }
        return Ok(Some((
            crate::recurring::occurrence_entity_id(task_id, work_date),
            previous.map(|item| item.version),
        )));
    }
    let status: String = transaction
        .query_row(
            "SELECT status FROM tasks WHERE id = ?1 AND workspace_id = ?2 AND deleted_at IS NULL",
            params![task_id, workspace_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if status == "done" {
        return Ok(None);
    }
    let expected_version =
        expected_version.ok_or_else(|| "VALIDATION_ERROR: 完成事项时缺少事项版本".to_string())?;
    let changed = transaction
        .execute(
            "UPDATE tasks SET status = 'done', completed_at = ?1, updated_at = ?1, version = version + 1
             WHERE id = ?2 AND workspace_id = ?3 AND version = ?4 AND status = 'open' AND deleted_at IS NULL",
            params![now, task_id, workspace_id, expected_version],
        )
        .map_err(|error| error.to_string())?;
    if changed == 0 {
        return Err("VERSION_CONFLICT: 事项状态已变化，请刷新后重试".to_string());
    }
    transaction
        .execute(
            "INSERT INTO task_status_events(id, workspace_id, task_id, status, occurred_at, source_type, created_at)
             VALUES (?1, ?2, ?3, 'done', ?4, 'user', ?4)",
            params![Uuid::now_v7().to_string(), workspace_id, task_id, now],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())?;
    Ok(None)
}

fn manual_time_range(request: &ManualEntryRequest) -> Result<(i64, i64, i64), String> {
    match (request.minutes, request.started_at, request.ended_at) {
        (Some(minutes), None, None) if minutes > 0 => {
            let ended_at = now_millis();
            Ok((ended_at - minutes * 60_000, ended_at, minutes * 60))
        }
        (None, Some(started_at), Some(ended_at)) if ended_at > started_at => {
            Ok((started_at, ended_at, (ended_at - started_at) / 1_000))
        }
        _ => Err("VALIDATION_ERROR: 请填写正整数分钟，或同时填写有效的开始与结束时间".to_string()),
    }
}

fn clear_workday_settlement(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    entry_id: &str,
) -> Result<(), String> {
    transaction
        .execute(
            "UPDATE work_days SET settled_at = NULL, updated_at = ?1, version = version + 1
             WHERE workspace_id = ?2 AND work_date = (SELECT work_date FROM time_entries WHERE id = ?3)",
            params![now_millis(), workspace_id, entry_id],
        )
        .map(|_| ())
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

fn validate_date(value: &str) -> Result<(), String> {
    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map(|_| ())
        .map_err(|_| format!("VALIDATION_ERROR: 非法日期 {value}"))
}

fn settlement_minutes(duration_seconds: i64) -> i64 {
    if duration_seconds <= 0 {
        0
    } else {
        (duration_seconds + 59) / 60
    }
}

fn current_revision(connection: &rusqlite::Connection) -> Result<i64, String> {
    connection
        .query_row(
            "SELECT CAST(value AS INTEGER) FROM app_metadata WHERE key = 'global_revision'",
            [],
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

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn map_active_timer_error(error: rusqlite::Error) -> String {
    let message = error.to_string();
    if message.contains("uq_time_entries_active") || message.contains("UNIQUE constraint failed") {
        "ACTIVE_TIMER_EXISTS: 已有计时正在运行或暂停".to_string()
    } else {
        message
    }
}

#[tauri::command]
pub fn timer_get_state(
    database: tauri::State<'_, Database>,
) -> Result<Option<TimeEntryDto>, String> {
    get_timer_state(&database)
}

#[tauri::command]
pub fn timer_start(
    database: tauri::State<'_, Database>,
    request: TimerStartRequest,
) -> Result<TimeEntryDto, String> {
    let operation_id = request.client_request_id.clone();
    let state = crate::supabase::storage_mode(&database)?;
    let result = start_timer_with_hooks_and_cloud_operation(
        &database,
        request,
        Some((&state, "timer_start", Some(operation_id.as_str()))),
    )?;
    crate::cloud_sync::flush_if_online(&database)?;
    Ok(result)
}

#[tauri::command]
pub fn timer_pause(
    database: tauri::State<'_, Database>,
    request: TimerVersionRequest,
) -> Result<TimeEntryDto, String> {
    let expected_version = request.expected_version;
    let state = crate::supabase::storage_mode(&database)?;
    let result =
        pause_timer_with_cloud_operation(&database, request, Some((&state, "timer_pause")))?;
    crate::cloud_sync::flush_if_online(&database)?;
    let _ = expected_version;
    Ok(result)
}

#[tauri::command]
pub fn timer_resume(
    database: tauri::State<'_, Database>,
    request: TimerResumeRequest,
) -> Result<TimeEntryDto, String> {
    let expected_version = request.expected_version;
    let operation_id = request.operation_id.clone();
    let state = crate::supabase::storage_mode(&database)?;
    let result = resume_timer_with_cloud_operation(
        &database,
        request,
        Some((&state, "timer_resume", Some(operation_id.as_str()))),
    )?;
    crate::cloud_sync::flush_if_online(&database)?;
    let _ = expected_version;
    Ok(result)
}

#[tauri::command]
pub fn timer_stop(
    database: tauri::State<'_, Database>,
    request: TimerStopRequest,
) -> Result<TimerStopResultDto, String> {
    let expected_version = request.expected_version;
    let state = crate::supabase::storage_mode(&database)?;
    let result = stop_timer_with_hooks_and_cloud_operation(
        &database,
        request,
        Some((&state, "timer_stop")),
    )?;
    crate::cloud_sync::flush_if_online(&database)?;
    let _ = expected_version;
    timer_chain_result(&database, result)
}

#[tauri::command]
pub fn time_entry_list(
    database: tauri::State<'_, Database>,
    request: TimeEntryListRequest,
) -> Result<TimeEntryListResult, String> {
    list_time_entries(&database, request)
}

#[tauri::command]
pub fn time_entry_create_manual(
    database: tauri::State<'_, Database>,
    request: ManualEntryRequest,
) -> Result<TimeEntryDto, String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let operation_id = request.client_request_id.clone();
    let state = crate::supabase::storage_mode(&database)?;
    let result = create_manual_entry_with_cloud_operation(
        &database,
        request,
        Some((
            &state,
            "time_entry_create_manual",
            Some(operation_id.as_str()),
        )),
    )?;
    crate::cloud_sync::flush_if_online(&database)?;
    Ok(result)
}

#[tauri::command]
pub fn time_entry_update(
    database: tauri::State<'_, Database>,
    request: TimeEntryUpdateRequest,
) -> Result<TimeEntryDto, String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let state = crate::supabase::storage_mode(&database)?;
    let result = update_time_entry_with_cloud_operation(
        &database,
        request,
        Some((&state, "time_entry_update")),
    )?;
    crate::cloud_sync::flush_if_online(&database)?;
    Ok(result)
}

#[tauri::command]
pub fn time_entry_update_disposition(
    database: tauri::State<'_, Database>,
    request: TimeEntryDispositionRequest,
) -> Result<TimeEntryDto, String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let state = crate::supabase::storage_mode(&database)?;
    let result = update_time_entry_disposition_with_cloud_operation(
        &database,
        request,
        Some((&state, "time_entry_update_disposition")),
    )?;
    crate::cloud_sync::flush_if_online(&database)?;
    Ok(result)
}

#[tauri::command]
pub fn time_allocation_replace(
    database: tauri::State<'_, Database>,
    request: AllocationReplaceRequest,
) -> Result<TimeEntryDto, String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let state = crate::supabase::storage_mode(&database)?;
    let result = replace_allocations_with_hooks_and_cloud_operation(
        &database,
        request,
        Some((&state, "time_allocation_replace")),
    )?;
    crate::cloud_sync::flush_if_online(&database)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recurring::{save_rule, RecurrenceSaveRequest};
    use crate::settings::{self, SettingsScope, SettingsUpdate};
    use crate::tasks::{create_task, TaskCreateRequest};
    use serde_json::{json, Value};
    use tempfile::tempdir;

    fn test_work_date() -> String {
        let timezone =
            crate::work_calendar::validate_timezone(crate::work_calendar::DEFAULT_TIMEZONE)
                .unwrap();
        crate::work_calendar::work_date_at(timezone, now_millis()).unwrap()
    }

    fn setup() -> (Database, String, String) {
        let directory = tempdir().unwrap();
        let path = directory.path().join("time.sqlite3");
        let database = Database::initialize_at(path).unwrap();
        std::mem::forget(directory);
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let subject_id: String = connection
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        drop(connection);
        let task = create_task(
            &database,
            TaskCreateRequest {
                subject_id,
                parent_id: None,
                title: "计时事项".to_string(),
                planned_date: None,
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        (database, workspace_id, task.id)
    }

    #[test]
    fn cloud_time_entry_writes_persist_outbox_in_the_same_transaction() {
        let (database, _workspace_id, task_id) = setup();
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
        let operation_id = Uuid::now_v7().to_string();
        let started_at = now_millis() - 3_600_000;
        let ended_at = started_at + 1_800_000;

        let created = create_manual_entry_with_cloud_operation(
            &database,
            ManualEntryRequest {
                task_id: Some(task_id.clone()),
                work_date: "2026-10-03".to_string(),
                started_at: Some(started_at),
                ended_at: Some(ended_at),
                minutes: None,
                note: Some("云端手动记录".to_string()),
                complete_task: false,
                task_expected_version: None,
                client_request_id: operation_id.clone(),
            },
            Some((
                &state,
                "time_entry_create_manual",
                Some(operation_id.as_str()),
            )),
        )
        .unwrap();
        let updated = update_time_entry_with_cloud_operation(
            &database,
            TimeEntryUpdateRequest {
                entry_id: created.id.clone(),
                started_at,
                ended_at: started_at + 2_400_000,
                note: Some("修正后的云端手动记录".to_string()),
                expected_version: created.version,
            },
            Some((&state, "time_entry_update")),
        )
        .unwrap();
        replace_allocations_with_cloud_operation(
            &database,
            AllocationReplaceRequest {
                entry_id: created.id.clone(),
                expected_version: updated.version,
                allocations: vec![AllocationInput {
                    task_id: task_id.clone(),
                    minutes: 40,
                    note: Some("全部归属并完成".to_string()),
                    complete_task: true,
                    task_expected_version: Some(1),
                }],
            },
            Some((&state, "time_allocation_replace")),
        )
        .unwrap();

        let connection = database.open().unwrap();
        let outbox = connection
            .prepare(
                "SELECT operation_id, operation_type, entity_type, entity_id, base_version, payload_json, depends_on_operation_id, payload_version, coalesced_count FROM sync_outbox ORDER BY created_at, rowid",
            )
            .unwrap()
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                    row.get::<_, i64>(8)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(outbox.len(), 2);
        let time_entry = outbox.iter().find(|row| row.2 == "time_entry").unwrap();
        let value: Value = serde_json::from_str(&time_entry.5).unwrap();
        assert_eq!(time_entry.0, operation_id);
        assert_eq!(time_entry.1, "time_allocation_replace");
        assert_eq!(time_entry.3.as_deref(), Some(created.id.as_str()));
        assert_eq!(time_entry.4, None);
        assert_eq!(time_entry.7, Some(3));
        assert_eq!(time_entry.8, 3);
        assert_eq!(value["note"], "修正后的云端手动记录");
        assert_eq!(value["duration_seconds"], 2_400);
        assert!(value["allocations"].as_array().is_some_and(|items| {
            items
                .iter()
                .any(|item| item["task_id"] == task_id && item["minutes"] == 40)
        }));
        let task_completion = outbox
            .iter()
            .find(|row| row.1 == "task_set_completed_from_allocation")
            .unwrap();
        assert_eq!(task_completion.2, "task");
        assert_eq!(task_completion.3.as_deref(), Some(task_id.as_str()));
        assert_eq!(task_completion.4, Some(1));
        assert_eq!(task_completion.6.as_deref(), Some(operation_id.as_str()));
    }

    #[test]
    fn cloud_mode_timer_can_write_local_cache_when_cloud_is_unavailable() {
        let (database, _workspace_id, task_id) = setup();
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
        let operation_id = Uuid::now_v7().to_string();
        let state = crate::supabase::storage_mode(&database).unwrap();
        let entry = start_timer_with_hooks_and_cloud_operation(
            &database,
            TimerStartRequest {
                task_id: Some(task_id),
                note: Some("离线云端模式继续本地计时".to_string()),
                client_request_id: operation_id.clone(),
            },
            Some((&state, "timer_start", Some(operation_id.as_str()))),
        )
        .unwrap();

        crate::cloud_sync::flush_if_online(&database).unwrap();
        let active = get_timer_state(&database).unwrap().unwrap();
        assert_eq!(active.id, entry.id);
        assert_eq!(active.state, "running");

        let connection = database.open().unwrap();
        let outbox: (String, String, String) = connection
            .query_row(
                "SELECT operation_type, entity_type, entity_id FROM sync_outbox
                 WHERE workspace_id = ?1 AND entity_type = 'time_entry'",
                [cloud_workspace_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            outbox,
            (
                "timer_start".to_string(),
                "time_entry".to_string(),
                entry.id
            )
        );
    }

    #[test]
    fn read_only_timer_refresh_does_not_grow_the_outbox() {
        let (database, _workspace_id, task_id) = setup();
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
        let started = start_timer_with_cloud_operation(
            &database,
            TimerStartRequest {
                task_id: Some(task_id),
                note: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
            Some((&state, "timer_start", None)),
        )
        .unwrap();
        let before: i64 = database
            .open()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM sync_outbox", [], |row| row.get(0))
            .unwrap();

        for _ in 0..12 {
            let refreshed = get_timer_state(&database).unwrap().unwrap();
            assert_eq!(refreshed.id, started.id);
        }

        let after: i64 = database
            .open()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM sync_outbox", [], |row| row.get(0))
            .unwrap();
        assert_eq!(after, before);
    }

    #[test]
    fn cloud_timer_lifecycle_persists_outbox_in_the_same_transaction() {
        let (database, _workspace_id, task_id) = setup();
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
        let start_operation_id = Uuid::now_v7().to_string();
        let resume_operation_id = Uuid::now_v7().to_string();

        let started = start_timer_with_cloud_operation(
            &database,
            TimerStartRequest {
                task_id: Some(task_id),
                note: Some("云端计时事务队列".to_string()),
                client_request_id: start_operation_id.clone(),
            },
            Some((&state, "timer_start", Some(start_operation_id.as_str()))),
        )
        .unwrap();
        let paused = pause_timer_with_cloud_operation(
            &database,
            TimerVersionRequest {
                entry_id: started.id.clone(),
                expected_version: started.version,
            },
            Some((&state, "timer_pause")),
        )
        .unwrap();
        let resumed = resume_timer_with_cloud_operation(
            &database,
            TimerResumeRequest {
                entry_id: started.id.clone(),
                expected_version: paused.version,
                operation_id: resume_operation_id.clone(),
            },
            Some((&state, "timer_resume", Some(resume_operation_id.as_str()))),
        )
        .unwrap();
        stop_timer_with_cloud_operation(
            &database,
            TimerStopRequest {
                entry_id: started.id.clone(),
                expected_version: resumed.version,
                create_default_allocation: false,
            },
            Some((&state, "timer_stop")),
        )
        .unwrap();

        let connection = database.open().unwrap();
        let outbox = connection
            .prepare(
                "SELECT operation_id, operation_type, entity_type, entity_id, base_version, payload_json, payload_version, coalesced_count FROM sync_outbox ORDER BY created_at, rowid",
            )
            .unwrap()
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(outbox.len(), 1);
        let timer = outbox.iter().find(|row| row.2 == "time_entry").unwrap();
        assert_eq!(timer.0, start_operation_id);
        assert_eq!(timer.1, "timer_stop");
        assert_eq!(timer.4, None);
        assert_eq!(timer.6, Some(4));
        assert_eq!(timer.7, 4);
        let value: Value = serde_json::from_str(&timer.5).unwrap();
        assert_eq!(timer.3.as_deref(), Some(started.id.as_str()));
        assert_eq!(value["id"], started.id);
        assert_eq!(value["state"], "ended");
        assert_eq!(value["version"], resumed.version + 1);
        assert_eq!(value["segments"].as_array().map(Vec::len), Some(2));
        assert!(outbox.iter().all(|row| row.2 != "unassigned_session"));
    }

    #[test]
    fn timer_pause_resume_stop_excludes_pause_duration() {
        let (database, _, task_id) = setup();
        let started = start_timer(
            &database,
            TimerStartRequest {
                task_id: Some(task_id),
                note: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        let connection = database.open().unwrap();
        connection
            .execute(
                "UPDATE time_segments SET started_at = started_at - 120000 WHERE entry_id = ?1",
                [&started.id],
            )
            .unwrap();
        drop(connection);
        let paused = pause_timer_with_cloud_operation(
            &database,
            TimerVersionRequest {
                entry_id: started.id.clone(),
                expected_version: started.version,
            },
            None,
        )
        .unwrap();
        assert!(paused.duration_seconds >= 120);
        let resumed = resume_timer_with_cloud_operation(
            &database,
            TimerResumeRequest {
                entry_id: started.id.clone(),
                expected_version: paused.version,
                operation_id: Uuid::now_v7().to_string(),
            },
            None,
        )
        .unwrap();
        let stopped = stop_timer(
            &database,
            TimerStopRequest {
                entry_id: started.id,
                expected_version: resumed.version,
                create_default_allocation: true,
            },
        )
        .unwrap();
        assert_eq!(stopped.state, "ended");
        assert_eq!(stopped.allocations.len(), 1);
        assert_eq!(stopped.allocated_minutes, stopped.settlement_minutes);
    }

    #[test]
    fn timer_stop_can_defer_default_allocation_until_confirmation() {
        let (database, _, task_id) = setup();
        let started = start_timer(
            &database,
            TimerStartRequest {
                task_id: Some(task_id.clone()),
                note: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        database
            .open()
            .unwrap()
            .execute(
                "UPDATE time_segments SET started_at = started_at - 120000 WHERE entry_id = ?1",
                [&started.id],
            )
            .unwrap();

        let stopped = stop_timer(
            &database,
            TimerStopRequest {
                entry_id: started.id.clone(),
                expected_version: started.version,
                create_default_allocation: false,
            },
        )
        .unwrap();

        assert_eq!(stopped.state, "ended");
        assert_eq!(stopped.default_task_id.as_deref(), Some(task_id.as_str()));
        assert_eq!(stopped.allocated_minutes, 0);
        assert!(stopped.allocations.is_empty());

        let allocated = replace_allocations(
            &database,
            AllocationReplaceRequest {
                entry_id: stopped.id,
                expected_version: stopped.version,
                allocations: vec![AllocationInput {
                    task_id: task_id.clone(),
                    minutes: stopped.settlement_minutes,
                    note: None,
                    complete_task: false,
                    task_expected_version: None,
                }],
            },
        )
        .unwrap();

        assert_eq!(allocated.allocated_minutes, stopped.settlement_minutes);
        assert_eq!(allocated.allocations.len(), 1);
        assert_eq!(allocated.allocations[0].task_id, task_id);
    }

    #[test]
    fn timer_can_stop_unassigned_when_original_task_was_deleted() {
        let (database, workspace_id, task_id) = setup();
        let started = start_timer(
            &database,
            TimerStartRequest {
                task_id: Some(task_id.clone()),
                note: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        let connection = database.open().unwrap();
        connection
            .execute(
                "UPDATE time_segments SET started_at = started_at - 120000 WHERE entry_id = ?1",
                [&started.id],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE tasks SET deleted_at = ?1 WHERE id = ?2",
                params![now_millis(), task_id],
            )
            .unwrap();
        let subject_id: String = connection
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        drop(connection);

        let stopped = stop_timer(
            &database,
            TimerStopRequest {
                entry_id: started.id,
                expected_version: started.version,
                create_default_allocation: true,
            },
        )
        .unwrap();
        assert_eq!(stopped.state, "ended");
        assert!(stopped.settlement_minutes >= 2);
        assert_eq!(stopped.allocated_minutes, 0);
        assert!(stopped.allocations.is_empty());
        assert_eq!(stopped.default_task_id, None);

        let replacement = create_task(
            &database,
            TaskCreateRequest {
                subject_id,
                parent_id: None,
                title: "重新归属事项".to_string(),
                planned_date: None,
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        let allocated = replace_allocations(
            &database,
            AllocationReplaceRequest {
                entry_id: stopped.id,
                allocations: vec![AllocationInput {
                    task_id: replacement.id.clone(),
                    minutes: stopped.settlement_minutes,
                    note: None,
                    complete_task: false,
                    task_expected_version: None,
                }],
                expected_version: stopped.version,
            },
        )
        .unwrap();
        assert_eq!(allocated.allocated_minutes, stopped.settlement_minutes);
        assert_eq!(allocated.allocations[0].task_id, replacement.id);
        assert_eq!(allocated.allocations[0].minutes, stopped.settlement_minutes);
        assert_eq!(
            database
                .open()
                .unwrap()
                .query_row(
                    "SELECT workspace_id FROM time_entries WHERE id = ?1",
                    [&allocated.id],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            workspace_id
        );
    }

    #[test]
    fn active_timer_is_restored_after_database_reopen() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("restore.sqlite3");
        let database = Database::initialize_at(path.clone()).unwrap();
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
                title: "重启恢复事项".to_string(),
                planned_date: None,
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        let started = start_timer(
            &database,
            TimerStartRequest {
                task_id: Some(task.id),
                note: Some("需要恢复".to_string()),
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        drop(database);

        let reopened = Database::initialize_at(path).unwrap();
        let restored = get_timer_state(&reopened).unwrap().unwrap();
        assert_eq!(restored.id, started.id);
        assert_eq!(restored.state, "running");
        assert_eq!(restored.note.as_deref(), Some("需要恢复"));
    }

    #[test]
    fn running_timer_state_includes_open_segment_duration() {
        let (database, _, task_id) = setup();
        let started = start_timer(
            &database,
            TimerStartRequest {
                task_id: Some(task_id),
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

        let refreshed = get_timer_state(&database).unwrap().unwrap();
        assert!(refreshed.duration_seconds >= 3);
        assert!(refreshed.settlement_minutes >= 1);
    }

    #[test]
    fn prevents_parallel_timer_and_rejects_parent_task() {
        let (database, _, task_id) = setup();
        let started = start_timer(
            &database,
            TimerStartRequest {
                task_id: Some(task_id.clone()),
                note: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        assert!(start_timer(
            &database,
            TimerStartRequest {
                task_id: None,
                note: None,
                client_request_id: Uuid::now_v7().to_string(),
            }
        )
        .is_err());
        stop_timer(
            &database,
            TimerStopRequest {
                entry_id: started.id,
                expected_version: started.version,
                create_default_allocation: false,
            },
        )
        .unwrap();
        let connection = database.open().unwrap();
        let subject_id: String = connection
            .query_row(
                "SELECT subject_id FROM tasks WHERE id = ?1",
                [&task_id],
                |row| row.get(0),
            )
            .unwrap();
        drop(connection);
        create_task(
            &database,
            TaskCreateRequest {
                subject_id,
                parent_id: Some(task_id.clone()),
                title: "子项".to_string(),
                planned_date: None,
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        assert!(start_timer(
            &database,
            TimerStartRequest {
                task_id: Some(task_id),
                note: None,
                client_request_id: Uuid::now_v7().to_string(),
            }
        )
        .is_err());
    }

    #[test]
    fn manual_entry_and_allocation_validation_work() {
        let (database, _, task_id) = setup();
        let manual = create_manual_entry(
            &database,
            ManualEntryRequest {
                task_id: None,
                work_date: test_work_date(),
                started_at: None,
                ended_at: None,
                minutes: Some(30),
                note: Some("补录".to_string()),
                complete_task: false,
                task_expected_version: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        assert_eq!(manual.settlement_minutes, 30);
        let allocated = replace_allocations(
            &database,
            AllocationReplaceRequest {
                entry_id: manual.id.clone(),
                allocations: vec![AllocationInput {
                    task_id: task_id.clone(),
                    minutes: 30,
                    note: None,
                    complete_task: false,
                    task_expected_version: None,
                }],
                expected_version: manual.version,
            },
        )
        .unwrap();
        assert_eq!(allocated.allocated_minutes, 30);
        assert!(replace_allocations(
            &database,
            AllocationReplaceRequest {
                entry_id: manual.id,
                allocations: vec![AllocationInput {
                    task_id,
                    minutes: 31,
                    note: None,
                    complete_task: false,
                    task_expected_version: None,
                }],
                expected_version: allocated.version,
            }
        )
        .is_err());
    }

    #[test]
    fn manual_entry_can_complete_selected_task() {
        let (database, _, task_id) = setup();
        let task_version: i64 = database
            .open()
            .unwrap()
            .query_row(
                "SELECT version FROM tasks WHERE id = ?1",
                [&task_id],
                |row| row.get(0),
            )
            .unwrap();

        let manual = create_manual_entry(
            &database,
            ManualEntryRequest {
                task_id: Some(task_id.clone()),
                work_date: test_work_date(),
                started_at: None,
                ended_at: None,
                minutes: Some(25),
                note: Some("补录并完成".to_string()),
                complete_task: true,
                task_expected_version: Some(task_version),
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();

        assert_eq!(manual.allocated_minutes, 25);
        let connection = database.open().unwrap();
        let status: String = connection
            .query_row(
                "SELECT status FROM tasks WHERE id = ?1",
                [&task_id],
                |row| row.get(0),
            )
            .unwrap();
        let done_events: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM task_status_events WHERE task_id = ?1 AND status = 'done'",
                [&task_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "done");
        assert_eq!(done_events, 1);
    }

    #[test]
    fn allocation_can_leave_minutes_unassigned() {
        let (database, _, task_id) = setup();
        let manual = create_manual_entry(
            &database,
            ManualEntryRequest {
                task_id: None,
                work_date: test_work_date(),
                started_at: None,
                ended_at: None,
                minutes: Some(30),
                note: Some("只归属一部分".to_string()),
                complete_task: false,
                task_expected_version: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();

        let allocated = replace_allocations(
            &database,
            AllocationReplaceRequest {
                entry_id: manual.id,
                allocations: vec![AllocationInput {
                    task_id,
                    minutes: 20,
                    note: None,
                    complete_task: false,
                    task_expected_version: None,
                }],
                expected_version: manual.version,
            },
        )
        .unwrap();

        assert_eq!(allocated.settlement_minutes, 30);
        assert_eq!(allocated.allocated_minutes, 20);
        assert_eq!(allocated.allocations.len(), 1);
        assert_eq!(allocated.allocations[0].minutes, 20);
    }

    #[test]
    fn allocation_can_complete_only_selected_tasks() {
        let (database, _, first_task_id) = setup();
        let subject_id: String = database
            .open()
            .unwrap()
            .query_row(
                "SELECT subject_id FROM tasks WHERE id = ?1",
                [&first_task_id],
                |row| row.get(0),
            )
            .unwrap();
        let second = create_task(
            &database,
            TaskCreateRequest {
                subject_id,
                parent_id: None,
                title: "保持未完成".to_string(),
                planned_date: None,
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        let first_version: i64 = database
            .open()
            .unwrap()
            .query_row(
                "SELECT version FROM tasks WHERE id = ?1",
                [&first_task_id],
                |row| row.get(0),
            )
            .unwrap();
        let manual = create_manual_entry(
            &database,
            ManualEntryRequest {
                task_id: None,
                work_date: test_work_date(),
                started_at: None,
                ended_at: None,
                minutes: Some(90),
                note: None,
                complete_task: false,
                task_expected_version: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        let allocated = replace_allocations(
            &database,
            AllocationReplaceRequest {
                entry_id: manual.id,
                allocations: vec![
                    AllocationInput {
                        task_id: first_task_id.clone(),
                        minutes: 60,
                        note: None,
                        complete_task: true,
                        task_expected_version: Some(first_version),
                    },
                    AllocationInput {
                        task_id: second.id.clone(),
                        minutes: 30,
                        note: None,
                        complete_task: false,
                        task_expected_version: None,
                    },
                ],
                expected_version: manual.version,
            },
        )
        .unwrap();
        assert_eq!(allocated.allocated_minutes, 90);
        let connection = database.open().unwrap();
        let first_status: String = connection
            .query_row(
                "SELECT status FROM tasks WHERE id = ?1",
                [&first_task_id],
                |row| row.get(0),
            )
            .unwrap();
        let second_status: String = connection
            .query_row(
                "SELECT status FROM tasks WHERE id = ?1",
                [&second.id],
                |row| row.get(0),
            )
            .unwrap();
        let events: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM task_status_events WHERE task_id = ?1 AND status = 'done'",
                [&first_task_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(first_status, "done");
        assert_eq!(second_status, "open");
        assert_eq!(events, 1);

        let resumed = start_timer(
            &database,
            TimerStartRequest {
                task_id: Some(first_task_id),
                note: Some("完成后继续执行".to_string()),
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        assert_eq!(resumed.state, "running");
        stop_timer(
            &database,
            TimerStopRequest {
                entry_id: resumed.id,
                expected_version: resumed.version,
                create_default_allocation: false,
            },
        )
        .unwrap();
    }

    #[test]
    fn allocation_completes_only_selected_recurring_occurrences() {
        let (database, workspace_id, first_task_id) = setup();
        let connection = database.open().unwrap();
        let subject_id: String = connection
            .query_row(
                "SELECT subject_id FROM tasks WHERE id = ?1",
                [&first_task_id],
                |row| row.get(0),
            )
            .unwrap();
        let first_version: i64 = connection
            .query_row(
                "SELECT version FROM tasks WHERE id = ?1",
                [&first_task_id],
                |row| row.get(0),
            )
            .unwrap();
        drop(connection);
        let second = create_task(
            &database,
            TaskCreateRequest {
                subject_id,
                parent_id: None,
                title: "第二个重复事项".to_string(),
                planned_date: None,
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        let today = test_work_date();
        save_rule(
            &database,
            RecurrenceSaveRequest {
                task_id: first_task_id.clone(),
                task_expected_version: first_version,
                frequency: "daily".to_string(),
                weekdays_mask: None,
                effective_start: today.clone(),
                rule_expected_version: None,
            },
        )
        .unwrap();
        save_rule(
            &database,
            RecurrenceSaveRequest {
                task_id: second.id.clone(),
                task_expected_version: second.version,
                frequency: "daily".to_string(),
                weekdays_mask: None,
                effective_start: today.clone(),
                rule_expected_version: None,
            },
        )
        .unwrap();
        let first_current_version: i64 = database
            .open()
            .unwrap()
            .query_row(
                "SELECT version FROM tasks WHERE id = ?1",
                [&first_task_id],
                |row| row.get(0),
            )
            .unwrap();
        let manual = create_manual_entry(
            &database,
            ManualEntryRequest {
                task_id: None,
                work_date: today.clone(),
                started_at: None,
                ended_at: None,
                minutes: Some(60),
                note: None,
                complete_task: false,
                task_expected_version: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        replace_allocations(
            &database,
            AllocationReplaceRequest {
                entry_id: manual.id,
                expected_version: manual.version,
                allocations: vec![
                    AllocationInput {
                        task_id: first_task_id.clone(),
                        minutes: 30,
                        note: None,
                        complete_task: true,
                        task_expected_version: Some(first_current_version),
                    },
                    AllocationInput {
                        task_id: second.id.clone(),
                        minutes: 30,
                        note: None,
                        complete_task: false,
                        task_expected_version: None,
                    },
                ],
            },
        )
        .unwrap();
        let connection = database.open().unwrap();
        let first_status: String = connection
            .query_row(
                "SELECT status FROM task_occurrences WHERE workspace_id = ?1 AND task_id = ?2 AND occurrence_date = ?3",
                params![workspace_id, first_task_id, today],
                |row| row.get(0),
            )
            .unwrap();
        let second_status: String = connection
            .query_row(
                "SELECT status FROM task_occurrences WHERE workspace_id = ?1 AND task_id = ?2 AND occurrence_date = ?3",
                params![workspace_id, second.id, today],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(first_status, "done");
        assert_eq!(second_status, "open");
        let base_done_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE id IN (?1, ?2) AND status = 'done'",
                params![first_task_id, second.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(base_done_count, 0);
    }

    #[test]
    fn cloud_recurring_completion_depends_on_the_final_time_entry_operation() {
        let (database, _workspace_id, task_id) = setup();
        let task_version: i64 = database
            .open()
            .unwrap()
            .query_row(
                "SELECT version FROM tasks WHERE id = ?1",
                [&task_id],
                |row| row.get(0),
            )
            .unwrap();
        let today = test_work_date();
        save_rule(
            &database,
            RecurrenceSaveRequest {
                task_id: task_id.clone(),
                task_expected_version: task_version,
                frequency: "daily".to_string(),
                weekdays_mask: None,
                effective_start: today.clone(),
                rule_expected_version: None,
            },
        )
        .unwrap();
        let current_task_version: i64 = database
            .open()
            .unwrap()
            .query_row(
                "SELECT version FROM tasks WHERE id = ?1",
                [&task_id],
                |row| row.get(0),
            )
            .unwrap();
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
        let state = crate::supabase::storage_mode(&database).unwrap();
        let operation_id = Uuid::now_v7().to_string();
        let entry = create_manual_entry_with_cloud_operation(
            &database,
            ManualEntryRequest {
                task_id: None,
                work_date: today.clone(),
                started_at: None,
                ended_at: None,
                minutes: Some(30),
                note: None,
                complete_task: false,
                task_expected_version: None,
                client_request_id: operation_id.clone(),
            },
            Some((
                &state,
                "time_entry_create_manual",
                Some(operation_id.as_str()),
            )),
        )
        .unwrap();
        replace_allocations_with_cloud_operation(
            &database,
            AllocationReplaceRequest {
                entry_id: entry.id,
                expected_version: entry.version,
                allocations: vec![AllocationInput {
                    task_id: task_id.clone(),
                    minutes: 30,
                    note: None,
                    complete_task: true,
                    task_expected_version: Some(current_task_version),
                }],
            },
            Some((&state, "time_allocation_replace")),
        )
        .unwrap();

        let connection = database.open().unwrap();
        let occurrence: (String, Option<String>) = connection
            .query_row(
                "SELECT entity_id, depends_on_operation_id FROM sync_outbox WHERE operation_type = 'task_occurrence_set_completed_from_allocation'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(occurrence.0, format!("{task_id}|{today}"));
        assert_eq!(occurrence.1.as_deref(), Some(operation_id.as_str()));
    }

    #[test]
    fn task_version_conflict_rolls_back_allocation_changes() {
        let (database, _, task_id) = setup();
        let manual = create_manual_entry(
            &database,
            ManualEntryRequest {
                task_id: None,
                work_date: test_work_date(),
                started_at: None,
                ended_at: None,
                minutes: Some(30),
                note: None,
                complete_task: false,
                task_expected_version: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        let result = replace_allocations(
            &database,
            AllocationReplaceRequest {
                entry_id: manual.id.clone(),
                allocations: vec![AllocationInput {
                    task_id,
                    minutes: 30,
                    note: None,
                    complete_task: true,
                    task_expected_version: Some(999),
                }],
                expected_version: manual.version,
            },
        );
        assert!(result.is_err());
        let allocation_count: i64 = database
            .open()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM time_allocations WHERE entry_id = ?1",
                [&manual.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(allocation_count, 0);
    }

    #[test]
    fn timer_can_run_without_default_task() {
        let (database, _, _) = setup();
        let started = start_timer(
            &database,
            TimerStartRequest {
                task_id: None,
                note: Some("先记录，稍后归属".to_string()),
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        assert_eq!(started.label, "未关联事项");
        assert_eq!(started.default_task_id, None);
        assert_eq!(started.note.as_deref(), Some("先记录，稍后归属"));

        let stopped = stop_timer(
            &database,
            TimerStopRequest {
                entry_id: started.id,
                expected_version: started.version,
                create_default_allocation: true,
            },
        )
        .unwrap();
        assert_eq!(stopped.state, "ended");
        assert!(stopped.allocations.is_empty());
    }

    #[test]
    fn corrected_entry_preserves_valid_reallocation() {
        let (database, _, task_id) = setup();
        let manual = create_manual_entry(
            &database,
            ManualEntryRequest {
                task_id: None,
                work_date: test_work_date(),
                started_at: None,
                ended_at: None,
                minutes: Some(30),
                note: None,
                complete_task: false,
                task_expected_version: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        let allocated = replace_allocations(
            &database,
            AllocationReplaceRequest {
                entry_id: manual.id.clone(),
                allocations: vec![AllocationInput {
                    task_id: task_id.clone(),
                    minutes: 20,
                    note: None,
                    complete_task: false,
                    task_expected_version: None,
                }],
                expected_version: manual.version,
            },
        )
        .unwrap();
        let corrected = update_time_entry(
            &database,
            TimeEntryUpdateRequest {
                entry_id: manual.id.clone(),
                started_at: manual.started_at,
                ended_at: manual.started_at + 25 * 60_000,
                note: Some("修正为 25 分钟".to_string()),
                expected_version: allocated.version,
            },
        )
        .unwrap();
        assert_eq!(corrected.settlement_minutes, 25);
        assert_eq!(corrected.allocated_minutes, 20);

        let reallocated = replace_allocations(
            &database,
            AllocationReplaceRequest {
                entry_id: manual.id,
                allocations: vec![AllocationInput {
                    task_id,
                    minutes: 25,
                    note: Some("最终归属".to_string()),
                    complete_task: false,
                    task_expected_version: None,
                }],
                expected_version: corrected.version,
            },
        )
        .unwrap();
        assert_eq!(reallocated.settlement_minutes, 25);
        assert_eq!(reallocated.allocated_minutes, 25);
        assert_eq!(reallocated.allocations.len(), 1);
    }

    #[test]
    fn unassigned_break_entry_can_be_reallocated_to_work() {
        let (database, workspace_id, task_id) = setup();
        let entry_id = Uuid::now_v7().to_string();
        let now = now_millis();
        database
            .open()
            .unwrap()
            .execute(
                "INSERT INTO time_entries(
                   id, workspace_id, work_date, kind, source_type, state, label_snapshot,
                   started_at, ended_at, duration_seconds, note, created_at, updated_at, version
                 ) VALUES (?1, ?2, ?3, 'break', 'unassigned', 'ended', '误记为休息', ?4, ?5, 600, '稍后重新分配', ?5, ?5, 1)",
                params![entry_id, workspace_id, test_work_date(), now - 600_000, now],
            )
            .unwrap();

        let updated = replace_allocations(
            &database,
            AllocationReplaceRequest {
                entry_id: entry_id.clone(),
                expected_version: 1,
                allocations: vec![AllocationInput {
                    task_id,
                    minutes: 10,
                    note: None,
                    complete_task: false,
                    task_expected_version: None,
                }],
            },
        )
        .unwrap();

        assert_eq!(updated.kind, "work");
        assert_eq!(updated.allocated_minutes, 10);
        assert_eq!(updated.allocations.len(), 1);
    }

    #[test]
    fn ended_entry_can_be_marked_break_or_discarded() {
        let (database, _, task_id) = setup();
        let manual = create_manual_entry(
            &database,
            ManualEntryRequest {
                task_id: Some(task_id.clone()),
                work_date: test_work_date(),
                started_at: None,
                ended_at: None,
                minutes: Some(15),
                note: None,
                complete_task: false,
                task_expected_version: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        assert_eq!(manual.kind, "work");
        assert_eq!(manual.allocated_minutes, 15);

        let marked_break = update_time_entry_disposition(
            &database,
            TimeEntryDispositionRequest {
                entry_id: manual.id.clone(),
                expected_version: manual.version,
                disposition: "break".to_string(),
            },
        )
        .unwrap();
        assert_eq!(marked_break.kind, "break");
        assert_eq!(marked_break.allocated_minutes, 0);

        update_time_entry_disposition(
            &database,
            TimeEntryDispositionRequest {
                entry_id: manual.id.clone(),
                expected_version: marked_break.version,
                disposition: "discard".to_string(),
            },
        )
        .unwrap();
        let deleted_at: Option<i64> = database
            .open()
            .unwrap()
            .query_row(
                "SELECT deleted_at FROM time_entries WHERE id = ?1",
                [&manual.id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(deleted_at.is_some());
    }

    fn shanghai_millis(value: &str) -> i64 {
        use chrono::NaiveDateTime;
        use chrono::TimeZone;
        let timezone: chrono_tz::Tz = "Asia/Shanghai".parse().unwrap();
        timezone
            .from_local_datetime(
                &NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S").unwrap(),
            )
            .single()
            .unwrap()
            .timestamp_millis()
    }

    fn seed_active_timer_at(
        database: &Database,
        workspace_id: &str,
        task_id: &str,
        state: &str,
        started_at: i64,
        last_continuous_at: i64,
    ) -> String {
        let entry_id = Uuid::now_v7().to_string();
        let work_date =
            crate::work_calendar::work_date_at("Asia/Shanghai".parse().unwrap(), started_at)
                .unwrap();
        let connection = database.open().unwrap();
        connection
            .execute(
                "INSERT INTO time_entries(
                   id,workspace_id,work_date,kind,source_type,state,default_task_id,
                   label_snapshot,started_at,duration_seconds,created_at,updated_at,version,
                   timer_chain_id,last_continuous_at
                 ) VALUES (?1,?2,?3,'work','timer',?4,?5,'跨日测试',?6,0,?6,?7,1,?1,?7)",
                params![
                    entry_id,
                    workspace_id,
                    work_date,
                    state,
                    task_id,
                    started_at,
                    last_continuous_at
                ],
            )
            .unwrap();
        if state == "running" {
            connection
                .execute(
                    "INSERT INTO time_segments(id,workspace_id,entry_id,sequence_no,started_at,duration_seconds)
                     VALUES (?1,?2,?3,1,?4,0)",
                    params![Uuid::now_v7().to_string(), workspace_id, entry_id, started_at],
                )
                .unwrap();
        }
        entry_id
    }

    #[test]
    fn running_timer_is_split_into_every_workspace_calendar_day() {
        let (database, workspace_id, task_id) = setup();
        let started_at = shanghai_millis("2026-10-05 23:59:30");
        let now = shanghai_millis("2026-10-07 00:00:30");
        let entry_id = seed_active_timer_at(
            &database,
            &workspace_id,
            &task_id,
            "running",
            started_at,
            now - 1_000,
        );

        coordinate_active_timer(&database, now).unwrap();
        let connection = database.open().unwrap();
        let rows = connection
            .prepare("SELECT id,work_date,state,duration_seconds,previous_entry_id FROM time_entries WHERE timer_chain_id=?1 ORDER BY work_date")
            .unwrap()
            .query_map([&entry_id], |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,row.get::<_,i64>(3)?,row.get::<_,Option<String>>(4)?)))
            .unwrap()
            .collect::<Result<Vec<_>,_>>()
            .unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows.iter().map(|row| row.1.as_str()).collect::<Vec<_>>(),
            vec!["2026-10-05", "2026-10-06", "2026-10-07"]
        );
        assert_eq!(
            rows.iter().map(|row| row.3).collect::<Vec<_>>(),
            vec![30, 86_400, 0]
        );
        assert_eq!(
            rows.iter().map(|row| row.2.as_str()).collect::<Vec<_>>(),
            vec!["ended", "ended", "running"]
        );
        assert_eq!(rows[1].4.as_deref(), Some(rows[0].0.as_str()));
        assert_eq!(rows[2].4.as_deref(), Some(rows[1].0.as_str()));
        let open_segments: i64 = connection.query_row("SELECT COUNT(*) FROM time_segments WHERE ended_at IS NULL AND entry_id IN (SELECT id FROM time_entries WHERE timer_chain_id=?1)",[&entry_id],|row|row.get(0)).unwrap();
        assert_eq!(open_segments, 1);
    }

    #[test]
    fn paused_timer_crosses_day_without_counting_pause_interval() {
        let (database, workspace_id, task_id) = setup();
        let started_at = shanghai_millis("2026-10-05 23:50:00");
        let now = shanghai_millis("2026-10-06 08:00:00");
        let entry_id = seed_active_timer_at(
            &database,
            &workspace_id,
            &task_id,
            "paused",
            started_at,
            started_at + 60_000,
        );
        database
            .open()
            .unwrap()
            .execute(
                "UPDATE time_entries SET duration_seconds=60 WHERE id=?1",
                [&entry_id],
            )
            .unwrap();

        coordinate_active_timer(&database, now).unwrap();
        let connection = database.open().unwrap();
        let rows = connection.prepare("SELECT work_date,state,duration_seconds,started_at FROM time_entries WHERE timer_chain_id=?1 ORDER BY work_date").unwrap().query_map([&entry_id],|row|Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,i64>(2)?,row.get::<_,i64>(3)?))).unwrap().collect::<Result<Vec<_>,_>>().unwrap();
        assert_eq!(
            rows,
            vec![
                (
                    "2026-10-05".to_string(),
                    "ended".to_string(),
                    60,
                    started_at
                ),
                ("2026-10-06".to_string(), "paused".to_string(), 0, now)
            ]
        );
    }

    #[test]
    fn interrupted_running_timer_excludes_gap_and_restarts_at_recovery() {
        let (database, workspace_id, task_id) = setup();
        let started_at = shanghai_millis("2026-10-06 09:00:00");
        let last_continuous = shanghai_millis("2026-10-06 09:01:00");
        let now = shanghai_millis("2026-10-06 10:00:00");
        let entry_id = seed_active_timer_at(
            &database,
            &workspace_id,
            &task_id,
            "running",
            started_at,
            last_continuous,
        );

        coordinate_active_timer(&database, now).unwrap();
        let connection = database.open().unwrap();
        let duration: i64 = connection
            .query_row(
                "SELECT duration_seconds FROM time_entries WHERE id=?1",
                [&entry_id],
                |row| row.get(0),
            )
            .unwrap();
        let segments = connection.prepare("SELECT started_at,ended_at,duration_seconds FROM time_segments WHERE entry_id=?1 ORDER BY sequence_no").unwrap().query_map([&entry_id],|row|Ok((row.get::<_,i64>(0)?,row.get::<_,Option<i64>>(1)?,row.get::<_,i64>(2)?))).unwrap().collect::<Result<Vec<_>,_>>().unwrap();
        assert_eq!(duration, 60);
        assert_eq!(
            segments,
            vec![(started_at, Some(last_continuous), 60), (now, None, 0)]
        );
    }

    #[test]
    fn overnight_interruption_closes_old_day_and_restarts_today() {
        let (database, workspace_id, task_id) = setup();
        let started_at = shanghai_millis("2026-10-05 23:50:00");
        let last_continuous = shanghai_millis("2026-10-05 23:51:00");
        let now = shanghai_millis("2026-10-06 08:00:00");
        let entry_id = seed_active_timer_at(
            &database,
            &workspace_id,
            &task_id,
            "running",
            started_at,
            last_continuous,
        );

        coordinate_active_timer(&database, now).unwrap();
        let connection = database.open().unwrap();
        let rows = connection.prepare("SELECT work_date,state,duration_seconds,started_at FROM time_entries WHERE timer_chain_id=?1 ORDER BY work_date").unwrap().query_map([&entry_id],|row|Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,i64>(2)?,row.get::<_,i64>(3)?))).unwrap().collect::<Result<Vec<_>,_>>().unwrap();
        assert_eq!(
            rows,
            vec![
                (
                    "2026-10-05".to_string(),
                    "ended".to_string(),
                    60,
                    started_at
                ),
                ("2026-10-06".to_string(), "running".to_string(), 0, now)
            ]
        );
    }

    #[test]
    fn chain_settlement_allocates_one_rounded_total_across_calendar_slices() {
        let slices = vec![
            ("a".to_string(), "2026-10-05".to_string(), 100, 30),
            ("b".to_string(), "2026-10-06".to_string(), 200, 30),
        ];
        let minutes = allocate_chain_minutes(&slices);
        assert_eq!(minutes.values().sum::<i64>(), 1);
        assert_eq!(minutes["a"], 1);
        assert_eq!(minutes["b"], 0);

        let multi_day = vec![
            ("a".to_string(), "2026-10-05".to_string(), 100, 59),
            ("b".to_string(), "2026-10-06".to_string(), 200, 61),
            ("c".to_string(), "2026-10-07".to_string(), 300, 121),
        ];
        let minutes = allocate_chain_minutes(&multi_day);
        assert_eq!(minutes.values().sum::<i64>(), settlement_minutes(241));
        assert_eq!(minutes["a"], 1);
        assert_eq!(minutes["b"], 2);
        assert_eq!(minutes["c"], 2);
    }

    #[test]
    fn cloud_rollover_queues_deterministic_close_then_create_operations() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("cloud-rollover-outbox.sqlite3"))
                .unwrap();
        let cloud_workspace_id = Uuid::now_v7().to_string();
        crate::settings::update_setting(
            &database,
            crate::settings::SettingsUpdate {
                scope: crate::settings::SettingsScope::Device,
                key: "cloud_workspace_id".to_string(),
                value: serde_json::json!(cloud_workspace_id.clone()),
            },
        )
        .unwrap();
        crate::settings::update_setting(
            &database,
            crate::settings::SettingsUpdate {
                scope: crate::settings::SettingsScope::Device,
                key: "storage_mode".to_string(),
                value: serde_json::json!("cloud"),
            },
        )
        .unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let timezone =
            crate::work_calendar::workspace_timezone(&connection, &workspace_id).unwrap();
        let today = crate::work_calendar::work_date_at(timezone, now_millis()).unwrap();
        let yesterday = (chrono::NaiveDate::parse_from_str(&today, "%Y-%m-%d").unwrap()
            - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
        let boundary = crate::work_calendar::day_bounds(timezone, &yesterday)
            .unwrap()
            .end_at;
        let entry_id = Uuid::now_v7().to_string();
        let device_id: String = connection
            .query_row(
                "SELECT id FROM devices WHERE workspace_id=?1 LIMIT 1",
                [&workspace_id],
                |row| row.get(0),
            )
            .unwrap();
        connection.execute("INSERT INTO time_entries(id,workspace_id,work_date,kind,source_type,state,label_snapshot,started_at,duration_seconds,created_at,updated_at,version,created_by_device_id,updated_by_device_id,timer_chain_id,last_continuous_at) VALUES (?1,?2,?3,'work','timer','running','跨日队列',?4,0,?4,?4,1,?5,?5,?1,?4)",params![entry_id,workspace_id,yesterday,boundary-60_000,device_id]).unwrap();
        connection.execute("INSERT INTO time_segments(id,workspace_id,entry_id,sequence_no,started_at,duration_seconds) VALUES (?1,?2,?3,1,?4,0)",params![Uuid::now_v7().to_string(),workspace_id,entry_id,boundary-60_000]).unwrap();
        drop(connection);
        coordinate_active_timer(&database, boundary + 30_000).unwrap();
        let connection = database.open().unwrap();
        let rows=connection.prepare("SELECT operation_type,depends_on_operation_id,operation_id FROM sync_outbox WHERE workspace_id=?1 AND operation_type LIKE 'timer_rollover_%' ORDER BY created_at,rowid").unwrap().query_map([cloud_workspace_id],|row|Ok((row.get::<_,String>(0)?,row.get::<_,Option<String>>(1)?,row.get::<_,String>(2)?))).unwrap().collect::<Result<Vec<_>,_>>().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, "timer_rollover_close");
        assert_eq!(rows[1].0, "timer_rollover_create");
        assert_eq!(rows[1].1.as_deref(), Some(rows[0].2.as_str()));
        drop(connection);
        coordinate_active_timer(&database, boundary + 30_000).unwrap();
        let count: i64 = database
            .open()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM sync_outbox WHERE operation_type LIKE 'timer_rollover_%'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn stopped_cross_day_chain_returns_all_slices_with_chain_level_minutes() {
        let (database, workspace_id, task_id) = setup();
        let timezone = "Asia/Shanghai".parse().unwrap();
        let today = crate::work_calendar::work_date_at(timezone, now_millis()).unwrap();
        let yesterday = (chrono::NaiveDate::parse_from_str(&today, "%Y-%m-%d").unwrap()
            - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
        let boundary = crate::work_calendar::day_bounds(timezone, &today)
            .unwrap()
            .start_at;
        let started_at = boundary - 30_000;
        let stop_at = boundary + 30_000;
        let _entry_id = seed_active_timer_at(
            &database,
            &workspace_id,
            &task_id,
            "running",
            started_at,
            stop_at - 1_000,
        );
        coordinate_active_timer(&database, stop_at).unwrap();
        let active = get_timer_state(&database).unwrap().unwrap();
        let stopped = stop_timer(
            &database,
            TimerStopRequest {
                entry_id: active.id,
                expected_version: active.version,
                create_default_allocation: false,
            },
        )
        .unwrap();
        let chain = timer_chain_result(&database, stopped).unwrap();
        assert_eq!(chain.slices.len(), 2);
        assert_eq!(chain.total_duration_seconds, 60);
        assert_eq!(chain.total_settlement_minutes, 1);
        assert_eq!(
            chain
                .slices
                .iter()
                .map(|slice| slice.settlement_minutes)
                .sum::<i64>(),
            1
        );
        assert_eq!(chain.slices[0].work_date, yesterday);
        assert_eq!(chain.slices[0].settlement_minutes, 1);
        assert_eq!(chain.slices[1].work_date, today);
        assert_eq!(chain.slices[1].settlement_minutes, 0);
    }
}
