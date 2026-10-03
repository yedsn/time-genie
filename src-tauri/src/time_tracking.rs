use std::collections::HashMap;

use chrono::Local;
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

pub fn start_timer_with_hooks(
    database: &Database,
    request: TimerStartRequest,
) -> Result<TimeEntryDto, String> {
    let operation_id = request.client_request_id.clone();
    let result = start_timer(database, request)?;
    publish_timer_hook(
        database,
        "timer.started",
        format!("timer.started:{operation_id}"),
        &result,
    );
    Ok(result)
}

pub fn stop_timer_with_hooks(
    database: &Database,
    request: TimerStopRequest,
) -> Result<TimeEntryDto, String> {
    let result = stop_timer(database, request)?;
    publish_timer_hook(
        database,
        "timer.stopped",
        format!("timer.stopped:{}:{}", result.id, result.version),
        &result,
    );
    Ok(result)
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

pub fn start_timer(
    database: &Database,
    request: TimerStartRequest,
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
    let work_date = local_date();
    if let Some(task_id) = request.task_id.as_deref() {
        crate::recurring::ensure_occurrence_if_recurring(
            &transaction,
            &workspace_id,
            task_id,
            &work_date,
            now,
        )?;
    }
    crate::unassigned::pause_for_timer(&transaction, &workspace_id, now)?;
    transaction
        .execute(
            "INSERT INTO time_entries(
               id, workspace_id, work_date, kind, source_type, state, default_task_id,
               label_snapshot, started_at, duration_seconds, note, created_at, updated_at, version,
               created_by_device_id, updated_by_device_id
             ) VALUES (?1, ?2, ?3, 'work', 'timer', 'running', ?4, ?5, ?6, 0, ?7, ?6, ?6, 1, ?8, ?8)",
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
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

pub fn pause_timer(
    database: &Database,
    request: TimerVersionRequest,
) -> Result<TimeEntryDto, String> {
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let now = now_millis();
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let state = entry_state(
        &transaction,
        &workspace_id,
        &request.entry_id,
        request.expected_version,
    )?;
    if state != "running" {
        return Err("VALIDATION_ERROR: 只有运行中的计时可以暂停".to_string());
    }
    close_open_segment(&transaction, &request.entry_id, now)?;
    let duration = segment_duration_sum(&transaction, &request.entry_id)?;
    update_entry_state(
        &transaction,
        &request.entry_id,
        request.expected_version,
        "paused",
        None,
        duration,
        now,
    )?;
    bump_revision(&transaction)?;
    let result = load_entry(&transaction, &request.entry_id, now)?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

pub fn resume_timer(
    database: &Database,
    request: TimerResumeRequest,
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
    let state = entry_state(
        &transaction,
        &workspace_id,
        &request.entry_id,
        request.expected_version,
    )?;
    if state != "paused" {
        return Err("VALIDATION_ERROR: 只有暂停中的计时可以继续".to_string());
    }
    let sequence: i64 = transaction
        .query_row(
            "SELECT COALESCE(MAX(sequence_no), 0) + 1 FROM time_segments WHERE entry_id = ?1",
            [&request.entry_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO time_segments(id, workspace_id, entry_id, sequence_no, started_at, duration_seconds)
             VALUES (?1, ?2, ?3, ?4, ?5, 0)",
            params![Uuid::now_v7().to_string(), workspace_id, request.entry_id, sequence, now],
        )
        .map_err(|error| error.to_string())?;
    let changed = transaction
        .execute(
            "UPDATE time_entries SET state = 'running', updated_at = ?1, version = version + 1
             WHERE id = ?2 AND version = ?3 AND state = 'paused' AND deleted_at IS NULL",
            params![now, request.entry_id, request.expected_version],
        )
        .map_err(|error| error.to_string())?;
    if changed == 0 {
        return Err("VERSION_CONFLICT: 计时状态已变化".to_string());
    }
    bump_revision(&transaction)?;
    let result = load_entry(&transaction, &request.entry_id, now)?;
    save_processed_result(
        &transaction,
        &workspace_id,
        &device_id,
        &request.operation_id,
        "timer_resume",
        &result,
    )?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

pub fn stop_timer(database: &Database, request: TimerStopRequest) -> Result<TimeEntryDto, String> {
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let now = now_millis();
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let state = entry_state(
        &transaction,
        &workspace_id,
        &request.entry_id,
        request.expected_version,
    )?;
    if state == "running" {
        close_open_segment(&transaction, &request.entry_id, now)?;
    } else if state != "paused" {
        return Err("VALIDATION_ERROR: 当前计时已经结束".to_string());
    }
    let duration = segment_duration_sum(&transaction, &request.entry_id)?;
    update_entry_state(
        &transaction,
        &request.entry_id,
        request.expected_version,
        "ended",
        Some(now),
        duration,
        now,
    )?;
    if request.create_default_allocation {
        let (default_task_id, work_date): (Option<String>, String) = transaction
            .query_row(
                "SELECT default_task_id, work_date FROM time_entries WHERE id = ?1",
                [&request.entry_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|error| error.to_string())?;
        let minutes = settlement_minutes(duration);
        if let Some(task_id) = default_task_id.filter(|_| minutes > 0) {
            if is_selectable_task(&transaction, &workspace_id, &task_id)? {
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
                        params![Uuid::now_v7().to_string(), workspace_id, request.entry_id, task_id, minutes, now],
                    )
                    .map_err(|error| error.to_string())?;
            } else {
                transaction
                    .execute(
                        "UPDATE time_entries SET default_task_id = NULL WHERE id = ?1",
                        [&request.entry_id],
                    )
                    .map_err(|error| error.to_string())?;
            }
        }
    }
    crate::unassigned::resume_after_timer(&transaction, &workspace_id, now)?;
    bump_revision(&transaction)?;
    let result = load_entry(&transaction, &request.entry_id, now)?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

pub fn list_time_entries(
    database: &Database,
    request: TimeEntryListRequest,
) -> Result<TimeEntryListResult, String> {
    validate_date(&request.work_date)?;
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
                complete_task_if_open(
                    &transaction,
                    &workspace_id,
                    &task_id,
                    request.task_expected_version,
                    &request.work_date,
                    now,
                )?;
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
            complete_task_if_open(
                &transaction,
                &workspace_id,
                &task_id,
                expected_version,
                &current.work_date,
                now,
            )?;
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
        crate::cloud_sync::enqueue_entity_in_transaction(
            &transaction,
            state,
            operation_type,
            "time_entry",
            Some(&request.entry_id),
            Some(request.expected_version),
            None,
        )?;
        for (task_id, base_version) in completed_task_outbox {
            crate::cloud_sync::enqueue_entity_in_transaction(
                &transaction,
                state,
                "task_set_completed_from_allocation",
                "task",
                Some(&task_id),
                Some(base_version),
                None,
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
                    default_task_id, note, kind, source_type, state, version
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
    entry.settlement_minutes = settlement_minutes(entry.duration_seconds);
    entry.allocated_minutes = entry.allocations.iter().map(|item| item.minutes).sum();
    Ok(entry)
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
                    updated_at = ?4, version = version + 1
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
) -> Result<(), String> {
    if crate::recurring::get_active_rule(transaction, workspace_id, task_id)?.is_some()
        || crate::recurring::get_rule_for_date(transaction, workspace_id, task_id, work_date)?
            .is_some()
    {
        crate::recurring::set_occurrence_status(
            transaction,
            workspace_id,
            task_id,
            work_date,
            true,
            None,
            now,
        )?;
        return Ok(());
    }
    let status: String = transaction
        .query_row(
            "SELECT status FROM tasks WHERE id = ?1 AND workspace_id = ?2 AND deleted_at IS NULL",
            params![task_id, workspace_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if status == "done" {
        return Ok(());
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
        .map_err(|error| error.to_string())
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

fn local_date() -> String {
    Local::now().format("%Y-%m-%d").to_string()
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
    let result = start_timer_with_hooks(&database, request)?;
    enqueue_timer_entry_if_cloud(
        &database,
        &result.id,
        None,
        "timer_start",
        Some(&operation_id),
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
    let result = pause_timer(&database, request)?;
    enqueue_timer_entry_if_cloud(
        &database,
        &result.id,
        Some(expected_version),
        "timer_pause",
        None,
    )?;
    crate::cloud_sync::flush_if_online(&database)?;
    Ok(result)
}

#[tauri::command]
pub fn timer_resume(
    database: tauri::State<'_, Database>,
    request: TimerResumeRequest,
) -> Result<TimeEntryDto, String> {
    let expected_version = request.expected_version;
    let operation_id = request.operation_id.clone();
    let result = resume_timer(&database, request)?;
    enqueue_timer_entry_if_cloud(
        &database,
        &result.id,
        Some(expected_version),
        "timer_resume",
        Some(&operation_id),
    )?;
    crate::cloud_sync::flush_if_online(&database)?;
    Ok(result)
}

#[tauri::command]
pub fn timer_stop(
    database: tauri::State<'_, Database>,
    request: TimerStopRequest,
) -> Result<TimeEntryDto, String> {
    let expected_version = request.expected_version;
    let result = stop_timer_with_hooks(&database, request)?;
    enqueue_timer_entry_if_cloud(
        &database,
        &result.id,
        Some(expected_version),
        "timer_stop",
        None,
    )?;
    crate::cloud_sync::flush_if_online(&database)?;
    Ok(result)
}

fn enqueue_timer_entry_if_cloud(
    database: &Database,
    entry_id: &str,
    base_version: Option<i64>,
    operation_type: &str,
    operation_id: Option<&str>,
) -> Result<(), String> {
    crate::cloud_sync::enqueue_entity_deferred(
        database,
        operation_type,
        "time_entry",
        Some(entry_id),
        base_version,
        operation_id,
    )
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
                "SELECT operation_id, operation_type, entity_type, entity_id, base_version, payload_json FROM sync_outbox ORDER BY created_at, operation_type",
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
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert!(outbox.iter().any(
            |(queued_operation_id, operation, entity_type, entity_id, base_version, payload)| {
                let value: Value = serde_json::from_str(payload).unwrap();
                queued_operation_id == &operation_id
                    && operation == "time_entry_create_manual"
                    && entity_type == "time_entry"
                    && entity_id.as_deref() == Some(&created.id)
                    && base_version.is_none()
                    && value["allocations"]
                        .as_array()
                        .is_some_and(|items| items.len() == 1)
            }
        ));
        assert!(outbox.iter().any(
            |(_, operation, entity_type, entity_id, base_version, payload)| {
                let value: Value = serde_json::from_str(payload).unwrap();
                operation == "time_entry_update"
                    && entity_type == "time_entry"
                    && entity_id.as_deref() == Some(&created.id)
                    && *base_version == Some(created.version)
                    && value["note"] == "修正后的云端手动记录"
                    && value["duration_seconds"] == 2_400
            }
        ));
        assert!(outbox.iter().any(
            |(_, operation, entity_type, entity_id, base_version, payload)| {
                let value: Value = serde_json::from_str(payload).unwrap();
                operation == "time_allocation_replace"
                    && entity_type == "time_entry"
                    && entity_id.as_deref() == Some(&created.id)
                    && *base_version == Some(updated.version)
                    && value["allocations"].as_array().is_some_and(|items| {
                        items
                            .iter()
                            .any(|item| item["task_id"] == task_id && item["minutes"] == 40)
                    })
            }
        ));
        assert!(outbox.iter().any(
            |(_, operation, entity_type, entity_id, base_version, payload)| {
                let value: Value = serde_json::from_str(payload).unwrap();
                operation == "task_set_completed_from_allocation"
                    && entity_type == "task"
                    && entity_id.as_deref() == Some(&task_id)
                    && *base_version == Some(1)
                    && value["status"] == "done"
            }
        ));
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
        let entry = start_timer_with_hooks(
            &database,
            TimerStartRequest {
                task_id: Some(task_id),
                note: Some("离线云端模式继续本地计时".to_string()),
                client_request_id: operation_id.clone(),
            },
        )
        .unwrap();
        enqueue_timer_entry_if_cloud(
            &database,
            &entry.id,
            None,
            "timer_start",
            Some(&operation_id),
        )
        .unwrap();

        crate::cloud_sync::flush_if_online(&database).unwrap();
        let active = get_timer_state(&database).unwrap().unwrap();
        assert_eq!(active.id, entry.id);
        assert_eq!(active.state, "running");

        let connection = database.open().unwrap();
        let outbox: (String, String, String) = connection
            .query_row(
                "SELECT operation_type, entity_type, entity_id FROM sync_outbox WHERE workspace_id = ?1",
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
        let paused = pause_timer(
            &database,
            TimerVersionRequest {
                entry_id: started.id.clone(),
                expected_version: started.version,
            },
        )
        .unwrap();
        assert!(paused.duration_seconds >= 120);
        let resumed = resume_timer(
            &database,
            TimerResumeRequest {
                entry_id: started.id.clone(),
                expected_version: paused.version,
                operation_id: Uuid::now_v7().to_string(),
            },
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
                work_date: local_date(),
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
                work_date: local_date(),
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
                work_date: local_date(),
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
                work_date: local_date(),
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
        let today = local_date();
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
    fn task_version_conflict_rolls_back_allocation_changes() {
        let (database, _, task_id) = setup();
        let manual = create_manual_entry(
            &database,
            ManualEntryRequest {
                task_id: None,
                work_date: local_date(),
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
                work_date: local_date(),
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
                params![entry_id, workspace_id, local_date(), now - 600_000, now],
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
                work_date: local_date(),
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
}
