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
    pub subject_id: Option<String>,
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

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TimeEntryListResult {
    pub entries: Vec<TimeEntryDto>,
    pub raw_minutes: i64,
    pub allocated_minutes: i64,
    pub pending_minutes: i64,
    pub break_minutes: i64,
    pub revision: i64,
}

pub const TIME_ENTRY_HISTORY_DAYS_PER_PAGE: i64 = 7;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TimeEntryHistoryRequest {
    pub page: i64,
    #[serde(default = "default_include_breaks")]
    pub include_breaks: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TimeEntryHistoryDayDto {
    pub work_date: String,
    pub entry_count: i64,
    pub raw_minutes: i64,
    pub allocated_minutes: i64,
    pub pending_minutes: i64,
    pub break_minutes: i64,
    pub entries: Vec<TimeEntryDto>,
    pub adjustment_minutes: i64,
    pub final_actual_minutes: i64,
    pub adjustments: Vec<crate::actual_time_adjustments::ActualTimeAdjustmentDto>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TimeEntryHistoryResult {
    pub page: i64,
    pub page_size: i64,
    pub total_days: i64,
    pub total_pages: i64,
    pub groups: Vec<TimeEntryHistoryDayDto>,
    pub revision: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TimerStopResultDto {
    pub current_entry: TimeEntryDto,
    pub slices: Vec<TimeEntryDto>,
    pub total_duration_seconds: i64,
    pub total_settlement_minutes: i64,
}

const CONTINUITY_GRACE_MILLIS: i64 = 90_000;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TimerStartRequest {
    pub task_id: Option<String>,
    pub note: Option<String>,
    pub client_request_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TimerVersionRequest {
    pub entry_id: String,
    pub expected_version: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TimerResumeRequest {
    pub entry_id: String,
    pub expected_version: i64,
    pub operation_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TimerStopRequest {
    pub entry_id: String,
    pub expected_version: i64,
    pub create_default_allocation: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TimerTransitionNewTaskRequest {
    pub subject_id: String,
    pub parent_id: Option<String>,
    pub expected_parent_version: Option<i64>,
    pub title: String,
    #[serde(default)]
    pub day_instance_date: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TimerTransitionRequest {
    pub client_request_id: String,
    pub current_entry_id: Option<String>,
    pub current_entry_expected_version: Option<i64>,
    pub task_id: Option<String>,
    #[serde(default)]
    pub reopen_completed_task_version: Option<i64>,
    pub new_task: Option<TimerTransitionNewTaskRequest>,
    pub note: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TimerTransitionResultDto {
    pub created_task: Option<crate::tasks::TaskDto>,
    pub previous_stop_result: Option<TimerStopResultDto>,
    pub active_entry: TimeEntryDto,
    pub transition_at: i64,
    pub no_op: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TimerRecoveryRequest {
    pub entry_id: String,
    pub expected_version: i64,
    pub client_request_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TimerTakeoverRequest {
    pub entry_id: String,
    pub expected_version: i64,
    pub lease_token: String,
    pub client_request_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TimerRecoveryResultDto {
    pub active_entry: TimeEntryDto,
    pub slices: Vec<TimeEntryDto>,
    pub recovered: bool,
    pub calendar_rollover: bool,
    pub continuity_interrupted: bool,
    pub created_slice_count: i64,
    pub observed_at: i64,
    pub no_op: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TimeEntryListRequest {
    pub work_date: String,
    pub include_breaks: bool,
}

fn default_include_breaks() -> bool {
    true
}

#[derive(Debug, Serialize, Deserialize, Clone)]
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

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TimeEntryUpdateRequest {
    pub entry_id: String,
    pub started_at: i64,
    pub ended_at: i64,
    pub note: Option<String>,
    pub expected_version: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TimeEntryDispositionRequest {
    pub entry_id: String,
    pub expected_version: i64,
    pub disposition: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AllocationReplaceRequest {
    pub entry_id: String,
    pub allocations: Vec<AllocationInput>,
    pub expected_version: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
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
    timer_chain_result_from_connection(&connection, current_entry)
}

fn timer_chain_result_from_connection(
    connection: &rusqlite::Connection,
    current_entry: TimeEntryDto,
) -> Result<TimerStopResultDto, String> {
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

fn transition_timer_with_cloud_operation(
    database: &Database,
    request: TimerTransitionRequest,
    _cloud_state: Option<&crate::supabase::StorageModeSnapshot>,
) -> Result<TimerTransitionResultDto, String> {
    validate_operation_id(&request.client_request_id)?;
    if request.task_id.is_some() == request.new_task.is_some() {
        return Err("VALIDATION_ERROR: 必须且只能选择一个目标事项".to_string());
    }
    if request.current_entry_id.is_some() != request.current_entry_expected_version.is_some() {
        return Err("VALIDATION_ERROR: 当前计时 ID 和版本必须同时提供".to_string());
    }

    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    if let Some(result) = processed_result::<TimerTransitionResultDto>(
        &connection,
        &workspace_id,
        &request.client_request_id,
    )? {
        return Ok(result);
    }
    let device_id = device_id(&connection, &workspace_id)?;
    let now = now_millis();
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;

    let active = if let (Some(entry_id), Some(expected_version)) = (
        request.current_entry_id.as_deref(),
        request.current_entry_expected_version,
    ) {
        let (resolved_id, resolved_version) = resolve_active_timer_request(
            &transaction,
            &workspace_id,
            entry_id,
            expected_version,
            now,
        )?;
        Some((resolved_id, resolved_version))
    } else {
        coordinate_active_timer_in_transaction(&transaction, &workspace_id, now)?;
        let active: Option<(String, i64)> = transaction
            .query_row(
                "SELECT id,version FROM time_entries
                 WHERE workspace_id=?1 AND state IN ('running','paused') AND deleted_at IS NULL
                 LIMIT 1",
                [&workspace_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        if active.is_some() {
            return Err("VERSION_CONFLICT: 当前计时状态已变化，请重新加载".to_string());
        }
        None
    };

    let work_date = crate::work_calendar::current_work_date(&transaction, now)?;
    let mut created_task = None;
    let target_task_id = if let Some(task_id) = request.task_id.as_deref() {
        ensure_selectable_task(&transaction, &workspace_id, task_id)?;
        if let Some(expected_version) = request.reopen_completed_task_version {
            let task: (String, Option<i64>, i64, bool) = transaction.query_row(
                "SELECT status,completed_at,version,EXISTS(SELECT 1 FROM task_recurrence_rules r WHERE r.task_id=tasks.id AND r.effective_end IS NULL) FROM tasks WHERE id=?1 AND workspace_id=?2 AND deleted_at IS NULL",
                params![task_id, workspace_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            ).map_err(|error| error.to_string())?;
            if task.0 != "done" || task.2 != expected_version {
                return Err("VERSION_CONFLICT: 已完成事项状态已变化，请重新加载".to_string());
            }
            if task.3 {
                return Err(
                    "VALIDATION_ERROR: 重复事项不能通过普通事项继续动作重新打开".to_string()
                );
            }
            let completed_at = task
                .1
                .ok_or_else(|| "VALIDATION_ERROR: 已完成事项缺少完成时间".to_string())?;
            let timezone = crate::work_calendar::workspace_timezone(&transaction, &workspace_id)?;
            if crate::work_calendar::work_date_at(timezone, completed_at)? != work_date {
                return Err("VALIDATION_ERROR: 只能继续今天完成的事项".to_string());
            }
            let changed = transaction.execute(
                "UPDATE tasks SET status='open',completed_at=NULL,updated_at=?1,version=version+1 WHERE id=?2 AND workspace_id=?3 AND status='done' AND version=?4 AND deleted_at IS NULL",
                params![now, task_id, workspace_id, expected_version],
            ).map_err(|error| error.to_string())?;
            if changed != 1 {
                return Err("VERSION_CONFLICT: 已完成事项状态已变化，请重新加载".to_string());
            }
            transaction.execute(
                "INSERT INTO task_status_events(id,workspace_id,task_id,status,occurred_at,source_type,created_at) VALUES(?1,?2,?3,'open',?4,'user',?4)",
                params![Uuid::now_v7().to_string(), workspace_id, task_id, now],
            ).map_err(|error| error.to_string())?;
        } else {
            let status: String = transaction
                .query_row("SELECT status FROM tasks WHERE id=?1", [task_id], |row| {
                    row.get(0)
                })
                .map_err(|error| error.to_string())?;
            if status != "open" {
                return Err("TASK_NOT_SELECTABLE: 已完成事项需使用继续动作".to_string());
            }
        }
        task_id.to_string()
    } else {
        let draft = request.new_task.as_ref().expect("validated new task");
        let task_request = crate::tasks::TaskCreateRequest {
            subject_id: draft.subject_id.clone(),
            day_instance_date: draft.day_instance_date.clone(),
            parent_id: draft.parent_id.clone(),
            title: draft.title.clone(),
            planned_date: draft.day_instance_date.is_none().then(|| work_date.clone()),
            estimate_minutes: None,
            note: None,
            project_name: None,
            solution_name: None,
        };
        let (task, _) = crate::tasks::create_task_in_transaction(
            &transaction,
            &workspace_id,
            &task_request,
            "timer_quick_create",
            draft.expected_parent_version,
            now,
            None,
            None,
            None,
        )?;
        let id = task.id.clone();
        created_task = Some(task);
        id
    };
    if created_task
        .as_ref()
        .is_some_and(|task| task.day_instance_date.is_none())
    {
        crate::today_plans::ensure_included_in_transaction(
            &transaction,
            &workspace_id,
            &target_task_id,
            &work_date,
            now,
            None,
        )?;
    }

    if let Some((active_id, _)) = active.as_ref() {
        let current_task_id: Option<String> = transaction
            .query_row(
                "SELECT default_task_id FROM time_entries WHERE id=?1",
                [active_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if created_task.is_none() && current_task_id.as_deref() == Some(target_task_id.as_str()) {
            let result = TimerTransitionResultDto {
                created_task: None,
                previous_stop_result: None,
                active_entry: load_entry(&transaction, active_id, now)?,
                transition_at: now,
                no_op: true,
            };
            save_processed_result(
                &transaction,
                &workspace_id,
                &device_id,
                &request.client_request_id,
                "timer_transition",
                &result,
            )?;
            transaction.commit().map_err(|error| error.to_string())?;
            return Ok(result);
        }
    }

    let previous_stop_result = if let Some((active_id, active_version)) = active {
        let state = entry_state(&transaction, &workspace_id, &active_id, active_version)?;
        if state == "running" {
            close_open_segment(&transaction, &active_id, now)?;
        } else if state != "paused" {
            return Err("VERSION_CONFLICT: 当前计时状态已变化".to_string());
        }
        let duration = segment_duration_sum(&transaction, &active_id)?;
        update_entry_state(
            &transaction,
            &active_id,
            active_version,
            "ended",
            Some(now),
            duration,
            now,
        )?;
        let stopped = load_entry(&transaction, &active_id, now)?;
        Some(timer_chain_result_from_connection(&transaction, stopped)?)
    } else {
        None
    };

    crate::recurring::ensure_occurrence_if_recurring(
        &transaction,
        &workspace_id,
        &target_task_id,
        &work_date,
        now,
    )?;
    crate::unassigned::clear_for_timer(&transaction, &workspace_id, now, false)?;
    let label = selectable_task_title(&transaction, &workspace_id, &target_task_id)?;
    let entry_id = Uuid::now_v7().to_string();
    transaction
        .execute(
            "INSERT INTO time_entries(
           id,workspace_id,work_date,kind,source_type,state,default_task_id,label_snapshot,
           started_at,duration_seconds,note,created_at,updated_at,version,created_by_device_id,
           updated_by_device_id,timer_chain_id,last_continuous_at
         ) VALUES(?1,?2,?3,'work','timer','running',?4,?5,?6,0,?7,?6,?6,1,?8,?8,?1,?6)",
            params![
                entry_id,
                workspace_id,
                work_date,
                target_task_id,
                label,
                now,
                request
                    .note
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty()),
                device_id
            ],
        )
        .map_err(map_active_timer_error)?;
    transaction.execute(
        "INSERT INTO time_segments(id,workspace_id,entry_id,sequence_no,started_at,duration_seconds)
         VALUES(?1,?2,?3,1,?4,0)",
        params![Uuid::now_v7().to_string(),workspace_id,entry_id,now],
    ).map_err(|error|error.to_string())?;
    record_runtime_heartbeat(&transaction, &workspace_id, now)?;
    bump_revision(&transaction)?;
    let active_entry = load_entry(&transaction, &entry_id, now)?;
    let result = TimerTransitionResultDto {
        created_task,
        previous_stop_result,
        active_entry,
        transition_at: now,
        no_op: false,
    };
    save_processed_result(
        &transaction,
        &workspace_id,
        &device_id,
        &request.client_request_id,
        "timer_transition",
        &result,
    )?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

#[cfg(test)]
pub(crate) fn transition_timer_for_cloud_test(
    database: &Database,
    request: TimerTransitionRequest,
    state: &crate::supabase::StorageModeSnapshot,
) -> Result<TimerTransitionResultDto, String> {
    transition_timer_with_cloud_operation(database, request, Some(state))
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
    _cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str, Option<&str>)>,
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
    crate::unassigned::clear_for_timer(&transaction, &workspace_id, now, false)?;
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
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

fn pause_timer_with_cloud_operation(
    database: &Database,
    request: TimerVersionRequest,
    _cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str)>,
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
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

fn resume_timer_with_cloud_operation(
    database: &Database,
    request: TimerResumeRequest,
    _cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str, Option<&str>)>,
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
    _cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str)>,
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
    crate::unassigned::start_after_timer(&transaction, &workspace_id, now, false)?;
    bump_revision(&transaction)?;
    record_runtime_heartbeat(&transaction, &workspace_id, now)?;
    let result = load_entry(&transaction, &entry_id, now)?;
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
        pending_minutes: (raw_minutes - allocated_minutes).max(0),
        break_minutes,
        revision,
    })
}

pub fn list_time_entry_history(
    database: &Database,
    request: TimeEntryHistoryRequest,
) -> Result<TimeEntryHistoryResult, String> {
    if request.page < 1 {
        return Err("VALIDATION_ERROR: 历史记录页码必须从 1 开始".to_string());
    }
    let connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let total_days: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM (
               SELECT work_date FROM time_entries WHERE workspace_id=?1 AND deleted_at IS NULL AND (?2=1 OR kind!='break')
               UNION SELECT work_date FROM actual_time_adjustments WHERE workspace_id=?1 AND deleted_at IS NULL
             )",
            params![workspace_id, request.include_breaks],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let total_pages = if total_days == 0 {
        0
    } else {
        (total_days + TIME_ENTRY_HISTORY_DAYS_PER_PAGE - 1) / TIME_ENTRY_HISTORY_DAYS_PER_PAGE
    };
    let page = if total_pages == 0 {
        1
    } else {
        request.page.min(total_pages)
    };
    let offset = (page - 1) * TIME_ENTRY_HISTORY_DAYS_PER_PAGE;
    let work_dates = {
        let mut statement = connection
            .prepare(
                "SELECT work_date FROM (
                   SELECT work_date FROM time_entries WHERE workspace_id=?1 AND deleted_at IS NULL AND (?2=1 OR kind!='break')
                   UNION SELECT work_date FROM actual_time_adjustments WHERE workspace_id=?1 AND deleted_at IS NULL
                 )
                 ORDER BY work_date DESC
                 LIMIT ?3 OFFSET ?4",
            )
            .map_err(|error| error.to_string())?;
        let dates = statement
            .query_map(
                params![
                    workspace_id,
                    request.include_breaks,
                    TIME_ENTRY_HISTORY_DAYS_PER_PAGE,
                    offset
                ],
                |row| row.get::<_, String>(0),
            )
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        dates
    };
    let now = now_millis();
    let mut groups = Vec::with_capacity(work_dates.len());
    for work_date in work_dates {
        let ids = {
            let mut statement = connection
                .prepare(
                    "SELECT id FROM time_entries
                     WHERE workspace_id=?1 AND work_date=?2 AND deleted_at IS NULL
                       AND (?3=1 OR kind!='break')
                     ORDER BY started_at DESC,id DESC",
                )
                .map_err(|error| error.to_string())?;
            let entry_ids = statement
                .query_map(
                    params![workspace_id, work_date, request.include_breaks],
                    |row| row.get::<_, String>(0),
                )
                .map_err(|error| error.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?;
            entry_ids
        };
        let entries = ids
            .iter()
            .map(|id| load_entry(&connection, id, now))
            .collect::<Result<Vec<_>, _>>()?;
        let raw_minutes = entries
            .iter()
            .filter(|entry| entry.kind == "work")
            .map(|entry| entry.settlement_minutes)
            .sum();
        let allocated_minutes = entries
            .iter()
            .filter(|entry| entry.kind == "work")
            .map(|entry| entry.allocated_minutes)
            .sum();
        let break_minutes = entries
            .iter()
            .filter(|entry| entry.kind == "break")
            .map(|entry| entry.settlement_minutes)
            .sum();
        let adjustment_ids = {
            let mut statement = connection.prepare("SELECT id FROM actual_time_adjustments WHERE workspace_id=?1 AND work_date=?2 AND deleted_at IS NULL ORDER BY created_at DESC,id DESC").map_err(|error|error.to_string())?;
            let result = statement
                .query_map(params![workspace_id, work_date], |row| {
                    row.get::<_, String>(0)
                })
                .map_err(|error| error.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?;
            result
        };
        let adjustments = adjustment_ids
            .iter()
            .map(|id| crate::actual_time_adjustments::load_adjustment_for_history(&connection, id))
            .collect::<Result<Vec<_>, _>>()?;
        let adjustment_minutes = adjustments
            .iter()
            .map(|item| item.delta_minutes)
            .sum::<i64>();
        groups.push(TimeEntryHistoryDayDto {
            work_date,
            entry_count: entries.len() as i64,
            raw_minutes,
            allocated_minutes,
            pending_minutes: (raw_minutes - allocated_minutes).max(0),
            break_minutes,
            entries,
            adjustment_minutes,
            final_actual_minutes: (allocated_minutes + adjustment_minutes).max(0),
            adjustments,
        });
    }
    Ok(TimeEntryHistoryResult {
        page,
        page_size: TIME_ENTRY_HISTORY_DAYS_PER_PAGE,
        total_days,
        total_pages,
        groups,
        revision: current_revision(&connection)?,
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
    _cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str, Option<&str>)>,
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
    _cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str)>,
) -> Result<TimeEntryDto, String> {
    if request.ended_at <= request.started_at {
        return Err("VALIDATION_ERROR: 结束时间必须晚于开始时间".to_string());
    }
    let duration_seconds = (request.ended_at - request.started_at) / 1_000;
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let now = now_millis();
    let current = load_entry(&transaction, &request.entry_id, now)?;
    if current.version != request.expected_version {
        return Err("VERSION_CONFLICT: 时间记录已被更新".to_string());
    }
    if current.state != "ended" {
        return Err("VALIDATION_ERROR: 只能修正已结束的时间记录".to_string());
    }
    crate::work_calendar::ensure_current_work_date(&transaction, &current.work_date, now)?;
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
    _cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str)>,
) -> Result<TimeEntryDto, String> {
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let now = now_millis();
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let current = load_entry(&transaction, &request.entry_id, now)?;
    if current.version != request.expected_version {
        return Err("VERSION_CONFLICT: 时间记录已被更新".to_string());
    }
    if current.state != "ended" {
        return Err("VALIDATION_ERROR: 只能处理已结束的时间记录".to_string());
    }
    if request.disposition != "break" && request.disposition != "discard" {
        return Err("VALIDATION_ERROR: 处理方式必须是休息或无效".to_string());
    }
    crate::work_calendar::ensure_current_work_date(&transaction, &current.work_date, now)?;
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
    _cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str)>,
) -> Result<TimeEntryDto, String> {
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let now = now_millis();
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let current = load_entry(&transaction, &request.entry_id, now)?;
    if current.version != request.expected_version {
        return Err("VERSION_CONFLICT: 时间记录已被更新".to_string());
    }
    if current.state != "ended" {
        return Err("VALIDATION_ERROR: 只能分配已结束的时间记录".to_string());
    }
    if current.kind != "work" && !(current.kind == "break" && current.source_type == "unassigned") {
        return Err("VALIDATION_ERROR: 只能分配工作记录或未归属处理记录".to_string());
    }
    crate::work_calendar::ensure_current_work_date(&transaction, &current.work_date, now)?;
    let mut merged = HashMap::<String, (i64, Option<String>, bool, Option<i64>)>::new();
    for allocation in request.allocations {
        if allocation.minutes < 0 {
            return Err("VALIDATION_ERROR: 分配分钟不能为负数".to_string());
        }
        if allocation.minutes == 0 && !allocation.complete_task {
            continue;
        }
        ensure_selectable_task(&transaction, &workspace_id, &allocation.task_id)?;
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
        if minutes > 0 {
            transaction
                .execute(
                    "INSERT INTO time_allocations(id, workspace_id, entry_id, task_id, minutes, note, created_at, updated_at, version)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, 1)",
                    params![Uuid::now_v7().to_string(), workspace_id, request.entry_id, task_id, minutes, note, now],
                )
                .map_err(|error| error.to_string())?;
        }
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
            "SELECT a.id, a.task_id, COALESCE(t.title, '已删除事项'), t.subject_id, a.minutes, a.note, a.version
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
                subject_id: row.get(3)?,
                minutes: row.get(4)?,
                note: row.get(5)?,
                version: row.get(6)?,
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

pub(crate) fn load_entry_at(
    connection: &rusqlite::Connection,
    entry_id: &str,
    current_time: i64,
) -> Result<TimeEntryDto, String> {
    load_entry(connection, entry_id, current_time)
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

    let mut previous_id = entry_id;
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
        previous_id = next_id;
    }
    record_runtime_heartbeat(transaction, workspace_id, now)?;
    bump_revision(transaction)?;
    Ok(Some(previous_id))
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
            "SELECT result_json FROM local_request_results WHERE workspace_id = ?1 AND request_id = ?2",
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
            "INSERT INTO local_request_results(workspace_id, request_id, device_id, request_type, result_json, processed_at)
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

fn local_timer_recovery(
    database: &Database,
    request: TimerRecoveryRequest,
) -> Result<TimerRecoveryResultDto, String> {
    let before = get_time_entry_by_id(database, &request.entry_id)?;
    if before.version != request.expected_version {
        return Err("VERSION_CONFLICT: 计时版本已变化，请重新加载".to_string());
    }
    if before.state != "running" && before.state != "paused" {
        return Err("VERSION_CONFLICT: 计时已经结束，请重新加载".to_string());
    }
    let connection = database.open()?;
    let workspace = workspace_id(&connection)?;
    let chain_id = before
        .timer_chain_id
        .clone()
        .unwrap_or_else(|| before.id.clone());
    let before_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM time_entries
             WHERE workspace_id=?1 AND COALESCE(timer_chain_id,id)=?2 AND deleted_at IS NULL",
            params![workspace, chain_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    drop(connection);

    let observed_at = now_millis();
    coordinate_active_timer(database, observed_at)?;
    let connection = database.open()?;
    let active_id: Option<String> = connection
        .query_row(
            "SELECT id FROM time_entries
             WHERE workspace_id=?1 AND state IN ('running','paused') AND deleted_at IS NULL
             LIMIT 1",
            [&workspace],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let active_id =
        active_id.ok_or_else(|| "VALIDATION_ERROR: 计时恢复后找不到活动计时".to_string())?;
    let active_entry = load_entry(&connection, &active_id, observed_at)?;
    let chain_result = timer_chain_result_from_connection(&connection, active_entry.clone())?;
    let after_count = chain_result.slices.len() as i64;
    let recovered = active_entry.id != before.id || active_entry.version != before.version;
    let calendar_rollover = active_entry.work_date != before.work_date;
    Ok(TimerRecoveryResultDto {
        active_entry,
        slices: chain_result.slices,
        recovered,
        calendar_rollover,
        continuity_interrupted: recovered && !calendar_rollover,
        created_slice_count: (after_count - before_count).max(0),
        observed_at,
        no_op: !recovered,
    })
}

#[tauri::command]
pub async fn timer_get_state(
    database: tauri::State<'_, Database>,
) -> Result<Option<TimeEntryDto>, String> {
    let database = database.inner().clone();
    crate::cloud_executor::run_for_storage(database, timer_state_for_storage).await
}

pub fn timer_state_for_storage(database: &Database) -> Result<Option<TimeEntryDto>, String> {
    crate::storage_contract::route(
        database,
        crate::storage_contract::BusinessDomain::Time,
        crate::storage_contract::StorageOperation::Query,
        get_timer_state,
        crate::supabase::direct_cloud_active_timer,
    )
}

pub(crate) fn timer_start_for_storage(
    database: &Database,
    request: TimerStartRequest,
) -> Result<TimeEntryDto, String> {
    let direct_request = request.clone();
    crate::storage_contract::route(
        database,
        crate::storage_contract::BusinessDomain::Time,
        crate::storage_contract::StorageOperation::Mutation,
        |database| {
            crate::supabase::ensure_repository_write_mode(database)?;
            let operation_id = request.client_request_id.clone();
            let state = crate::supabase::storage_mode(database)?;
            let result = start_timer_with_hooks_and_cloud_operation(
                database,
                request,
                Some((&state, "timer_start", Some(operation_id.as_str()))),
            )?;
            Ok(result)
        },
        |database| crate::supabase::direct_cloud_timer_start(database, direct_request),
    )
}

pub(crate) fn timer_pause_for_storage(
    database: &Database,
    request: TimerVersionRequest,
) -> Result<TimeEntryDto, String> {
    let direct_request = request.clone();
    crate::storage_contract::route(
        database,
        crate::storage_contract::BusinessDomain::Time,
        crate::storage_contract::StorageOperation::Mutation,
        |database| {
            crate::supabase::ensure_repository_write_mode(database)?;
            let state = crate::supabase::storage_mode(database)?;
            let result =
                pause_timer_with_cloud_operation(database, request, Some((&state, "timer_pause")))?;
            Ok(result)
        },
        |database| crate::supabase::direct_cloud_timer_pause(database, direct_request),
    )
}

pub(crate) fn timer_resume_for_storage(
    database: &Database,
    request: TimerResumeRequest,
) -> Result<TimeEntryDto, String> {
    let direct_request = request.clone();
    crate::storage_contract::route(
        database,
        crate::storage_contract::BusinessDomain::Time,
        crate::storage_contract::StorageOperation::Mutation,
        |database| {
            crate::supabase::ensure_repository_write_mode(database)?;
            let operation_id = request.operation_id.clone();
            let state = crate::supabase::storage_mode(database)?;
            let result = resume_timer_with_cloud_operation(
                database,
                request,
                Some((&state, "timer_resume", Some(operation_id.as_str()))),
            )?;
            Ok(result)
        },
        |database| crate::supabase::direct_cloud_timer_resume(database, direct_request),
    )
}

pub(crate) fn timer_stop_for_storage(
    database: &Database,
    request: TimerStopRequest,
) -> Result<TimerStopResultDto, String> {
    let direct_request = request.clone();
    crate::storage_contract::route(
        database,
        crate::storage_contract::BusinessDomain::Time,
        crate::storage_contract::StorageOperation::Mutation,
        |database| {
            crate::supabase::ensure_repository_write_mode(database)?;
            let state = crate::supabase::storage_mode(database)?;
            let result = stop_timer_with_hooks_and_cloud_operation(
                database,
                request,
                Some((&state, "timer_stop")),
            )?;
            timer_chain_result(database, result)
        },
        |database| crate::supabase::direct_cloud_timer_stop(database, direct_request),
    )
}

#[tauri::command]
pub async fn timer_start(
    app: tauri::AppHandle,
    database: tauri::State<'_, Database>,
    request: TimerStartRequest,
) -> Result<TimeEntryDto, String> {
    let database = database.inner().clone();
    let storage = database.clone();
    let result = crate::cloud_executor::run_for_storage(storage, move |database| {
        timer_start_for_storage(database, request)
    })
    .await?;
    let _ = crate::refresh_tray_presentation_with_timer(&app, &database, Some(result.clone()));
    Ok(result)
}

#[tauri::command]
pub async fn timer_transition(
    app: tauri::AppHandle,
    database: tauri::State<'_, Database>,
    request: TimerTransitionRequest,
) -> Result<TimerTransitionResultDto, String> {
    let database = database.inner().clone();
    let storage = database.clone();
    let result = crate::cloud_executor::run_for_storage(storage, move |database| {
        let direct_request = request.clone();
        crate::storage_contract::route(
            database,
            crate::storage_contract::BusinessDomain::Time,
            crate::storage_contract::StorageOperation::Mutation,
            |database| {
                crate::supabase::ensure_repository_write_mode(database)?;
                let state = crate::supabase::storage_mode(database)?;
                transition_timer_with_cloud_operation(database, request, Some(&state))
            },
            |database| crate::supabase::direct_cloud_timer_transition(database, direct_request),
        )
    })
    .await?;
    let _ = crate::refresh_tray_presentation_with_timer(
        &app,
        &database,
        Some(result.active_entry.clone()),
    );
    Ok(result)
}

#[tauri::command]
pub async fn timer_pause(
    app: tauri::AppHandle,
    database: tauri::State<'_, Database>,
    request: TimerVersionRequest,
) -> Result<TimeEntryDto, String> {
    let database = database.inner().clone();
    let storage = database.clone();
    let result = crate::cloud_executor::run_for_storage(storage, move |database| {
        timer_pause_for_storage(database, request)
    })
    .await?;
    let _ = crate::refresh_tray_presentation_with_timer(&app, &database, Some(result.clone()));
    Ok(result)
}

#[tauri::command]
pub async fn timer_resume(
    app: tauri::AppHandle,
    database: tauri::State<'_, Database>,
    request: TimerResumeRequest,
) -> Result<TimeEntryDto, String> {
    let database = database.inner().clone();
    let storage = database.clone();
    let result = crate::cloud_executor::run_for_storage(storage, move |database| {
        timer_resume_for_storage(database, request)
    })
    .await?;
    let _ = crate::refresh_tray_presentation_with_timer(&app, &database, Some(result.clone()));
    Ok(result)
}

#[tauri::command]
pub async fn timer_stop(
    app: tauri::AppHandle,
    database: tauri::State<'_, Database>,
    request: TimerStopRequest,
) -> Result<TimerStopResultDto, String> {
    let database = database.inner().clone();
    let storage = database.clone();
    let result = crate::cloud_executor::run_for_storage(storage, move |database| {
        timer_stop_for_storage(database, request)
    })
    .await?;
    let _ = crate::refresh_tray_presentation_with_timer(&app, &database, None);
    Ok(result)
}

#[tauri::command]
pub async fn timer_recover(
    app: tauri::AppHandle,
    database: tauri::State<'_, Database>,
    request: TimerRecoveryRequest,
) -> Result<TimerRecoveryResultDto, String> {
    let database = database.inner().clone();
    let storage = database.clone();
    let result = crate::cloud_executor::run_for_storage(storage, move |database| {
        let direct_request = request.clone();
        crate::storage_contract::route(
            database,
            crate::storage_contract::BusinessDomain::Time,
            crate::storage_contract::StorageOperation::Mutation,
            |database| local_timer_recovery(database, request),
            |database| crate::supabase::direct_cloud_timer_recover(database, direct_request),
        )
    })
    .await?;
    let _ = crate::refresh_tray_presentation_with_timer(
        &app,
        &database,
        Some(result.active_entry.clone()),
    );
    Ok(result)
}

#[tauri::command]
pub async fn timer_takeover(
    app: tauri::AppHandle,
    database: tauri::State<'_, Database>,
    request: TimerTakeoverRequest,
) -> Result<TimerRecoveryResultDto, String> {
    let database = database.inner().clone();
    let storage = database.clone();
    let result = crate::cloud_executor::run_for_storage(storage, move |database| {
        let direct_request = request.clone();
        let local_request = TimerRecoveryRequest {
            entry_id: request.entry_id,
            expected_version: request.expected_version,
            client_request_id: request.client_request_id,
        };
        crate::storage_contract::route(
            database,
            crate::storage_contract::BusinessDomain::Time,
            crate::storage_contract::StorageOperation::Mutation,
            |database| local_timer_recovery(database, local_request),
            |database| crate::supabase::direct_cloud_timer_takeover(database, direct_request),
        )
    })
    .await?;
    let _ = crate::refresh_tray_presentation_with_timer(
        &app,
        &database,
        Some(result.active_entry.clone()),
    );
    Ok(result)
}

#[tauri::command]
pub async fn time_entry_list(
    database: tauri::State<'_, Database>,
    request: TimeEntryListRequest,
) -> Result<TimeEntryListResult, String> {
    let database = database.inner().clone();
    crate::cloud_executor::run_for_storage(database, move |database| {
        let direct_request = request.clone();
        crate::storage_contract::route(
            database,
            crate::storage_contract::BusinessDomain::Time,
            crate::storage_contract::StorageOperation::Query,
            |database| list_time_entries(database, request),
            |database| {
                crate::supabase::direct_cloud_time_entry_list_for_command(database, direct_request)
            },
        )
    })
    .await
}

#[tauri::command]
pub async fn time_entry_history_list(
    database: tauri::State<'_, Database>,
    request: TimeEntryHistoryRequest,
) -> Result<TimeEntryHistoryResult, String> {
    let database = database.inner().clone();
    crate::cloud_executor::run_for_storage(database, move |database| {
        let direct_request = request.clone();
        crate::storage_contract::route(
            database,
            crate::storage_contract::BusinessDomain::Time,
            crate::storage_contract::StorageOperation::Query,
            |database| list_time_entry_history(database, request),
            |database| {
                crate::supabase::direct_cloud_time_history_for_command(database, direct_request)
            },
        )
    })
    .await
}

#[tauri::command]
pub async fn time_entry_create_manual(
    database: tauri::State<'_, Database>,
    request: ManualEntryRequest,
) -> Result<TimeEntryDto, String> {
    let database = database.inner().clone();
    let direct_request = request.clone();
    crate::storage_contract::route_async(
        database,
        crate::storage_contract::BusinessDomain::Time,
        crate::storage_contract::StorageOperation::Mutation,
        |database| {
            crate::supabase::ensure_repository_write_mode(database)?;
            let operation_id = request.client_request_id.clone();
            let state = crate::supabase::storage_mode(database)?;
            let result = create_manual_entry_with_cloud_operation(
                database,
                request,
                Some((
                    &state,
                    "time_entry_create_manual",
                    Some(operation_id.as_str()),
                )),
            )?;
            Ok(result)
        },
        |database| crate::supabase::direct_cloud_manual_time_create(database, direct_request),
    )
    .await
}

#[tauri::command]
pub async fn time_entry_update(
    database: tauri::State<'_, Database>,
    request: TimeEntryUpdateRequest,
) -> Result<TimeEntryDto, String> {
    let database = database.inner().clone();
    let direct_request = request.clone();
    crate::storage_contract::route_async(
        database,
        crate::storage_contract::BusinessDomain::Time,
        crate::storage_contract::StorageOperation::Mutation,
        |database| {
            crate::supabase::ensure_repository_write_mode(database)?;
            let state = crate::supabase::storage_mode(database)?;
            let result = update_time_entry_with_cloud_operation(
                database,
                request,
                Some((&state, "time_entry_update")),
            )?;
            Ok(result)
        },
        |database| crate::supabase::direct_cloud_time_entry_update(database, direct_request),
    )
    .await
}

#[tauri::command]
pub async fn time_entry_update_disposition(
    database: tauri::State<'_, Database>,
    request: TimeEntryDispositionRequest,
) -> Result<TimeEntryDto, String> {
    let database = database.inner().clone();
    let direct_request = request.clone();
    crate::storage_contract::route_async(
        database,
        crate::storage_contract::BusinessDomain::Time,
        crate::storage_contract::StorageOperation::Mutation,
        |database| {
            crate::supabase::ensure_repository_write_mode(database)?;
            let state = crate::supabase::storage_mode(database)?;
            let result = update_time_entry_disposition_with_cloud_operation(
                database,
                request,
                Some((&state, "time_entry_update_disposition")),
            )?;
            Ok(result)
        },
        |database| crate::supabase::direct_cloud_time_entry_disposition(database, direct_request),
    )
    .await
}

#[tauri::command]
pub async fn time_allocation_replace(
    database: tauri::State<'_, Database>,
    request: AllocationReplaceRequest,
) -> Result<TimeEntryDto, String> {
    let database = database.inner().clone();
    let direct_request = request.clone();
    crate::storage_contract::route_async(
        database,
        crate::storage_contract::BusinessDomain::Time,
        crate::storage_contract::StorageOperation::Mutation,
        |database| {
            crate::supabase::ensure_repository_write_mode(database)?;
            let state = crate::supabase::storage_mode(database)?;
            let result = replace_allocations_with_hooks_and_cloud_operation(
                database,
                request,
                Some((&state, "time_allocation_replace")),
            )?;
            Ok(result)
        },
        |database| crate::supabase::direct_cloud_time_allocation_replace(database, direct_request),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recurring::{save_rule, RecurrenceSaveRequest};
    use crate::settings::{self, SettingsScope, SettingsUpdate};
    use crate::tasks::{create_task, TaskCreateRequest};
    use serde_json::json;
    use tempfile::tempdir;

    fn assert_legacy_sync_runtime_removed(database: &Database) {
        let connection = database.open().unwrap();
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type='table' AND name IN (
                   'sync_outbox','local_sync_state','cloud_deferred_entities',
                   'legacy_abandoned_outbox','processed_operations','sync_items','sync_runs'
                 )",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

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
                day_instance_date: None,
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

    fn subject_id(database: &Database) -> String {
        database
            .open()
            .unwrap()
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap()
    }

    fn transition(
        database: &Database,
        request: TimerTransitionRequest,
    ) -> Result<TimerTransitionResultDto, String> {
        transition_timer_with_cloud_operation(database, request, None)
    }

    fn cloud_state(database: &Database) -> crate::supabase::StorageModeSnapshot {
        let connection = database.open().unwrap();
        let local_workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        crate::supabase::StorageModeSnapshot {
            mode: "cloud".to_string(),
            storage_protocol: crate::storage_contract::LEGACY_SYNC_PROTOCOL.to_string(),
            storage_backend: "legacy_sync_sqlite".to_string(),
            cloud_access_state: "legacy_sync_offline".to_string(),
            cloud_writable: true,
            read_only_reason: None,
            workspace_id: local_workspace_id,
            device_id,
            online: false,
            last_change_seq: 0,
            last_synced_at: None,
            cache_source: None,
            cache_fetched_at: None,
            last_error: None,
            auth_blocked: false,
            auth_blocked_reason: None,
            workspace_epoch: 1,
        }
    }

    #[test]
    fn transition_creates_root_task_and_starts_timer_atomically() {
        let (database, _, _) = setup();
        let operation_id = Uuid::now_v7().to_string();
        let result = transition(
            &database,
            TimerTransitionRequest {
                client_request_id: operation_id.clone(),
                current_entry_id: None,
                current_entry_expected_version: None,
                task_id: None,
                reopen_completed_task_version: None,
                new_task: Some(TimerTransitionNewTaskRequest {
                    subject_id: subject_id(&database),
                    parent_id: None,
                    expected_parent_version: None,
                    title: "临时一级事项".to_string(),
                    day_instance_date: None,
                }),
                note: None,
            },
        )
        .unwrap();
        let created = result.created_task.as_ref().unwrap();
        assert_eq!(created.title, "临时一级事项");
        assert_eq!(
            created.planned_date.as_deref(),
            Some(test_work_date().as_str())
        );
        assert_eq!(
            result.active_entry.default_task_id.as_deref(),
            Some(created.id.as_str())
        );
        assert_eq!(result.active_entry.state, "running");
        let day_plan: (String, i64) = database
            .open()
            .unwrap()
            .query_row(
                "SELECT state,version FROM task_day_plans WHERE task_id=?1 AND work_date=?2",
                params![created.id, test_work_date()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(day_plan, ("included".to_string(), 1));

        let repeated = transition(
            &database,
            TimerTransitionRequest {
                client_request_id: operation_id,
                current_entry_id: None,
                current_entry_expected_version: None,
                task_id: None,
                reopen_completed_task_version: None,
                new_task: Some(TimerTransitionNewTaskRequest {
                    subject_id: subject_id(&database),
                    parent_id: None,
                    expected_parent_version: None,
                    title: "临时一级事项".to_string(),
                    day_instance_date: None,
                }),
                note: None,
            },
        )
        .unwrap();
        assert_eq!(repeated.active_entry.id, result.active_entry.id);
        assert_eq!(repeated.created_task.unwrap().id, created.id);
    }

    #[test]
    fn transition_creates_today_instance_without_source_mapping() {
        let (database, _, _) = setup();
        let work_date = test_work_date();
        let result = transition(
            &database,
            TimerTransitionRequest {
                client_request_id: Uuid::now_v7().to_string(),
                current_entry_id: None,
                current_entry_expected_version: None,
                task_id: None,
                reopen_completed_task_version: None,
                new_task: Some(TimerTransitionNewTaskRequest {
                    subject_id: subject_id(&database),
                    parent_id: None,
                    expected_parent_version: None,
                    title: "今日临时事项".to_string(),
                    day_instance_date: Some(work_date.clone()),
                }),
                note: None,
            },
        )
        .unwrap();
        let created = result.created_task.unwrap();
        assert_eq!(
            created.day_instance_date.as_deref(),
            Some(work_date.as_str())
        );
        assert_eq!(created.day_instance_kind.as_deref(), Some("member"));
        assert_eq!(created.day_instance_source_task_id, None);
        assert_eq!(created.planned_date, None);
        let mapping_count: i64 = database
            .open()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM task_day_plans WHERE task_id=?1",
                [&created.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(mapping_count, 0);
        assert_eq!(
            result.active_entry.default_task_id.as_deref(),
            Some(created.id.as_str())
        );
    }

    #[test]
    fn transition_creates_child_and_switches_at_one_boundary() {
        let (database, _, initial_task_id) = setup();
        let parent = create_task(
            &database,
            TaskCreateRequest {
                subject_id: subject_id(&database),
                day_instance_date: None,
                parent_id: None,
                title: "父事项".to_string(),
                planned_date: None,
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        let active = start_timer(
            &database,
            TimerStartRequest {
                task_id: Some(initial_task_id),
                note: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        let result = transition(
            &database,
            TimerTransitionRequest {
                client_request_id: Uuid::now_v7().to_string(),
                current_entry_id: Some(active.id),
                current_entry_expected_version: Some(active.version),
                task_id: None,
                reopen_completed_task_version: None,
                new_task: Some(TimerTransitionNewTaskRequest {
                    subject_id: parent.subject_id.clone(),
                    parent_id: Some(parent.id.clone()),
                    expected_parent_version: Some(parent.version),
                    title: "临时二级事项".to_string(),
                    day_instance_date: None,
                }),
                note: None,
            },
        )
        .unwrap();
        let child = result.created_task.unwrap();
        assert_eq!(child.parent_id.as_deref(), Some(parent.id.as_str()));
        assert_eq!(result.active_entry.started_at, result.transition_at);
        assert_eq!(
            result
                .previous_stop_result
                .as_ref()
                .unwrap()
                .current_entry
                .ended_at,
            Some(result.transition_at)
        );
        let active_count: i64 = database
            .open()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM time_entries WHERE state IN ('running','paused') AND deleted_at IS NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(active_count, 1);
    }

    #[test]
    fn transition_from_paused_timer_ends_it_and_starts_target() {
        let (database, _, initial_task_id) = setup();
        let target = create_task(
            &database,
            TaskCreateRequest {
                subject_id: subject_id(&database),
                day_instance_date: None,
                parent_id: None,
                title: "暂停后切换目标".to_string(),
                planned_date: None,
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        let active = start_timer(
            &database,
            TimerStartRequest {
                task_id: Some(initial_task_id),
                note: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        let paused = pause_timer_with_cloud_operation(
            &database,
            TimerVersionRequest {
                entry_id: active.id,
                expected_version: active.version,
            },
            None,
        )
        .unwrap();

        let result = transition(
            &database,
            TimerTransitionRequest {
                client_request_id: Uuid::now_v7().to_string(),
                current_entry_id: Some(paused.id.clone()),
                current_entry_expected_version: Some(paused.version),
                task_id: Some(target.id.clone()),
                reopen_completed_task_version: None,
                new_task: None,
                note: None,
            },
        )
        .unwrap();

        assert_eq!(
            result.active_entry.default_task_id.as_deref(),
            Some(target.id.as_str())
        );
        assert_eq!(result.active_entry.state, "running");
        assert_eq!(
            result
                .previous_stop_result
                .as_ref()
                .unwrap()
                .current_entry
                .state,
            "ended"
        );
        assert_eq!(
            result
                .previous_stop_result
                .as_ref()
                .unwrap()
                .current_entry
                .ended_at,
            Some(result.active_entry.started_at)
        );
        let active_count: i64 = database
            .open()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM time_entries WHERE state IN ('running','paused') AND deleted_at IS NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(active_count, 1);
    }

    #[test]
    fn transition_to_current_task_is_no_op() {
        let (database, _, task_id) = setup();
        let active = start_timer(
            &database,
            TimerStartRequest {
                task_id: Some(task_id.clone()),
                note: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        let result = transition(
            &database,
            TimerTransitionRequest {
                client_request_id: Uuid::now_v7().to_string(),
                current_entry_id: Some(active.id.clone()),
                current_entry_expected_version: Some(active.version),
                task_id: Some(task_id),
                reopen_completed_task_version: None,
                new_task: None,
                note: None,
            },
        )
        .unwrap();
        assert!(result.no_op);
        assert_eq!(result.active_entry.id, active.id);
        assert!(result.previous_stop_result.is_none());
    }

    #[test]
    fn transition_reopens_today_completed_task_and_starts_atomically() {
        let (database, _, task_id) = setup();
        let now = now_millis();
        database
            .open()
            .unwrap()
            .execute(
                "UPDATE tasks SET status='done',completed_at=?1,version=2 WHERE id=?2",
                params![now, task_id],
            )
            .unwrap();
        let result = transition(
            &database,
            TimerTransitionRequest {
                client_request_id: Uuid::now_v7().to_string(),
                current_entry_id: None,
                current_entry_expected_version: None,
                task_id: Some(task_id.clone()),
                reopen_completed_task_version: Some(2),
                new_task: None,
                note: None,
            },
        )
        .unwrap();
        assert_eq!(
            result.active_entry.default_task_id.as_deref(),
            Some(task_id.as_str())
        );
        let (status, completed_at, version): (String, Option<i64>, i64) = database
            .open()
            .unwrap()
            .query_row(
                "SELECT status,completed_at,version FROM tasks WHERE id=?1",
                [&task_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!((status.as_str(), completed_at, version), ("open", None, 3));
    }

    #[test]
    fn transition_continue_failure_rolls_back_task_and_active_timer() {
        let (database, _, initial_task_id) = setup();
        let completed = create_task(
            &database,
            TaskCreateRequest {
                subject_id: subject_id(&database),
                day_instance_date: None,
                parent_id: None,
                title: "待继续".into(),
                planned_date: None,
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        database
            .open()
            .unwrap()
            .execute(
                "UPDATE tasks SET status='done',completed_at=?1,version=2 WHERE id=?2",
                params![now_millis(), completed.id],
            )
            .unwrap();
        let active = start_timer(
            &database,
            TimerStartRequest {
                task_id: Some(initial_task_id.clone()),
                note: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        let error = transition(
            &database,
            TimerTransitionRequest {
                client_request_id: Uuid::now_v7().to_string(),
                current_entry_id: Some(active.id.clone()),
                current_entry_expected_version: Some(active.version),
                task_id: Some(completed.id.clone()),
                reopen_completed_task_version: Some(999),
                new_task: None,
                note: None,
            },
        )
        .unwrap_err();
        assert!(error.contains("VERSION_CONFLICT"));
        let connection = database.open().unwrap();
        let status: String = connection
            .query_row(
                "SELECT status FROM tasks WHERE id=?1",
                [&completed.id],
                |row| row.get(0),
            )
            .unwrap();
        let active_state: String = connection
            .query_row(
                "SELECT state FROM time_entries WHERE id=?1",
                [&active.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            (status.as_str(), active_state.as_str()),
            ("done", "running")
        );
        drop(connection);
        assert_legacy_sync_runtime_removed(&database);
    }

    #[test]
    fn transition_rolls_back_new_task_when_parent_version_is_stale() {
        let (database, _, _) = setup();
        let parent = create_task(
            &database,
            TaskCreateRequest {
                subject_id: subject_id(&database),
                day_instance_date: None,
                parent_id: None,
                title: "并发父事项".to_string(),
                planned_date: None,
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        let error = transition(
            &database,
            TimerTransitionRequest {
                client_request_id: Uuid::now_v7().to_string(),
                current_entry_id: None,
                current_entry_expected_version: None,
                task_id: None,
                reopen_completed_task_version: None,
                new_task: Some(TimerTransitionNewTaskRequest {
                    subject_id: parent.subject_id.clone(),
                    parent_id: Some(parent.id),
                    expected_parent_version: Some(parent.version + 1),
                    title: "不应创建".to_string(),
                    day_instance_date: None,
                }),
                note: None,
            },
        )
        .unwrap_err();
        assert!(error.starts_with("VERSION_CONFLICT:"));
        let count: i64 = database
            .open()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE title='不应创建'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
        let connection = database.open().unwrap();
        let day_plan_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM task_day_plans WHERE task_id IN (SELECT id FROM tasks WHERE title='不应创建')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let active_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM time_entries WHERE state IN ('running','paused') AND deleted_at IS NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!((day_plan_count, active_count), (0, 0));
    }

    #[test]
    fn transition_rolls_back_new_task_and_day_plan_when_timer_insert_fails() {
        let (database, _, _) = setup();
        database
            .open()
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER reject_transition_timer
                 BEFORE INSERT ON time_entries
                 BEGIN
                   SELECT RAISE(FAIL, 'forced timer failure');
                 END;",
            )
            .unwrap();

        let error = transition(
            &database,
            TimerTransitionRequest {
                client_request_id: Uuid::now_v7().to_string(),
                current_entry_id: None,
                current_entry_expected_version: None,
                task_id: None,
                reopen_completed_task_version: None,
                new_task: Some(TimerTransitionNewTaskRequest {
                    subject_id: subject_id(&database),
                    parent_id: None,
                    expected_parent_version: None,
                    title: "计时失败回滚事项".to_string(),
                    day_instance_date: None,
                }),
                note: None,
            },
        )
        .unwrap_err();
        assert!(error.contains("forced timer failure"));
        let connection = database.open().unwrap();
        let task_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE title='计时失败回滚事项'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let day_plan_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM task_day_plans", [], |row| row.get(0))
            .unwrap();
        let active_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM time_entries WHERE state IN ('running','paused') AND deleted_at IS NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!((task_count, day_plan_count, active_count), (0, 0, 0));
    }

    #[test]
    fn transition_rejects_completed_parent_without_creating_or_starting() {
        let (database, _, _) = setup();
        let parent = create_task(
            &database,
            TaskCreateRequest {
                subject_id: subject_id(&database),
                day_instance_date: None,
                parent_id: None,
                title: "已完成父事项".to_string(),
                planned_date: None,
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        database
            .open()
            .unwrap()
            .execute(
                "UPDATE tasks SET status='done',completed_at=?1,version=version+1 WHERE id=?2",
                params![now_millis(), parent.id],
            )
            .unwrap();

        let error = transition(
            &database,
            TimerTransitionRequest {
                client_request_id: Uuid::now_v7().to_string(),
                current_entry_id: None,
                current_entry_expected_version: None,
                task_id: None,
                reopen_completed_task_version: None,
                new_task: Some(TimerTransitionNewTaskRequest {
                    subject_id: parent.subject_id,
                    parent_id: Some(parent.id),
                    expected_parent_version: Some(parent.version + 1),
                    title: "不应创建在已完成父项下".to_string(),
                    day_instance_date: None,
                }),
                note: None,
            },
        )
        .unwrap_err();
        assert!(error.contains("已完成"));
        let connection = database.open().unwrap();
        let task_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE title='不应创建在已完成父项下'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let active_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM time_entries WHERE state IN ('running','paused') AND deleted_at IS NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!((task_count, active_count), (0, 0));
    }

    #[test]
    fn transition_uses_workspace_work_date_across_natural_date_boundary() {
        let (database, _, _) = setup();
        // 使用固定偏移的设备时区（UTC+14）与固定工作空间时区（UTC-10），二者恒定相差 24 小时，
        // 因此任意时刻两侧的自然日期都不相同，从而证明事务使用的是工作空间日期而不是设备本地日期。
        let device_timezone: chrono_tz::Tz = "Pacific/Kiritimati".parse().unwrap();
        database
            .open()
            .unwrap()
            .execute("UPDATE workspaces SET timezone='Pacific/Honolulu'", [])
            .unwrap();
        let timestamp = now_millis();
        let expected_work_date =
            crate::work_calendar::current_work_date(&database.open().unwrap(), timestamp).unwrap();
        let device_date = crate::work_calendar::work_date_at(device_timezone, timestamp).unwrap();
        assert_ne!(
            expected_work_date, device_date,
            "工作空间日期 {expected_work_date} 与设备日期 {device_date} 应当不同"
        );

        let result = transition(
            &database,
            TimerTransitionRequest {
                client_request_id: Uuid::now_v7().to_string(),
                current_entry_id: None,
                current_entry_expected_version: None,
                task_id: None,
                reopen_completed_task_version: None,
                new_task: Some(TimerTransitionNewTaskRequest {
                    subject_id: subject_id(&database),
                    parent_id: None,
                    expected_parent_version: None,
                    title: "跨自然日事项".to_string(),
                    day_instance_date: None,
                }),
                note: None,
            },
        )
        .unwrap();
        assert_eq!(
            result
                .created_task
                .as_ref()
                .unwrap()
                .planned_date
                .as_deref(),
            Some(expected_work_date.as_str())
        );
        assert_eq!(result.active_entry.work_date, expected_work_date);
    }

    #[test]
    fn transition_coordinates_cross_day_chain_before_switching() {
        let (database, workspace_id, initial_task_id) = setup();
        let target = create_task(
            &database,
            TaskCreateRequest {
                subject_id: subject_id(&database),
                day_instance_date: None,
                parent_id: None,
                title: "跨日切换目标".to_string(),
                planned_date: None,
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        let timezone =
            crate::work_calendar::current_workspace_timezone(&database.open().unwrap()).unwrap();
        let current_date = crate::work_calendar::work_date_at(timezone, now_millis()).unwrap();
        let current_start = crate::work_calendar::day_bounds(timezone, &current_date)
            .unwrap()
            .start_at;
        let active_id = seed_active_timer_at(
            &database,
            &workspace_id,
            &initial_task_id,
            "running",
            current_start - 30_000,
            now_millis(),
        );

        let result = transition(
            &database,
            TimerTransitionRequest {
                client_request_id: Uuid::now_v7().to_string(),
                current_entry_id: Some(active_id),
                current_entry_expected_version: Some(1),
                task_id: Some(target.id),
                reopen_completed_task_version: None,
                new_task: None,
                note: None,
            },
        )
        .unwrap();
        let stopped = result.previous_stop_result.unwrap();
        assert!(stopped.slices.len() >= 2);
        assert_eq!(stopped.current_entry.ended_at, Some(result.transition_at));
        assert_eq!(result.active_entry.started_at, result.transition_at);
    }

    #[test]
    fn transition_creates_current_occurrence_for_recurring_target() {
        let (database, workspace_id, _) = setup();
        let target = create_task(
            &database,
            TaskCreateRequest {
                subject_id: subject_id(&database),
                day_instance_date: None,
                parent_id: None,
                title: "重复计时目标".to_string(),
                planned_date: None,
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        save_rule(
            &database,
            RecurrenceSaveRequest {
                task_id: target.id.clone(),
                task_expected_version: target.version,
                frequency: "daily".to_string(),
                weekdays_mask: None,
                effective_start: test_work_date(),
                rule_expected_version: None,
            },
        )
        .unwrap();

        transition(
            &database,
            TimerTransitionRequest {
                client_request_id: Uuid::now_v7().to_string(),
                current_entry_id: None,
                current_entry_expected_version: None,
                task_id: Some(target.id.clone()),
                reopen_completed_task_version: None,
                new_task: None,
                note: None,
            },
        )
        .unwrap();
        let occurrence_count: i64 = database
            .open()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM task_occurrences WHERE workspace_id=?1 AND task_id=?2 AND occurrence_date=?3",
                params![workspace_id, target.id, test_work_date()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(occurrence_count, 1);
    }

    #[test]
    fn transition_is_idempotent_without_creating_outbox() {
        let (database, _, task_id) = setup();
        let state = cloud_state(&database);
        let operation_id = Uuid::now_v7().to_string();
        let request = TimerTransitionRequest {
            client_request_id: operation_id.clone(),
            current_entry_id: None,
            current_entry_expected_version: None,
            task_id: Some(task_id),
            reopen_completed_task_version: None,
            new_task: None,
            note: None,
        };
        let first = transition_timer_with_cloud_operation(&database, request.clone(), Some(&state))
            .unwrap();
        let repeated =
            transition_timer_with_cloud_operation(&database, request, Some(&state)).unwrap();
        assert_eq!(repeated.active_entry.id, first.active_entry.id);
        assert_legacy_sync_runtime_removed(&database);
    }

    #[test]
    fn cloud_compatible_new_task_transition_persists_business_rows_without_outbox() {
        let (database, _, _) = setup();
        let state = cloud_state(&database);
        let operation_id = Uuid::now_v7().to_string();
        let result = transition_timer_with_cloud_operation(
            &database,
            TimerTransitionRequest {
                client_request_id: operation_id.clone(),
                current_entry_id: None,
                current_entry_expected_version: None,
                task_id: None,
                reopen_completed_task_version: None,
                new_task: Some(TimerTransitionNewTaskRequest {
                    subject_id: subject_id(&database),
                    parent_id: None,
                    expected_parent_version: None,
                    title: "云端原子今日事项".to_string(),
                    day_instance_date: None,
                }),
                note: None,
            },
            Some(&state),
        )
        .unwrap();
        let created = result.created_task.unwrap();
        assert_eq!(
            result.active_entry.default_task_id.as_deref(),
            Some(created.id.as_str())
        );
        let connection = database.open().unwrap();
        let day_plan_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM task_day_plans WHERE task_id=?1 AND work_date=?2 AND state='included'",
                params![created.id, test_work_date()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(day_plan_count, 1);
        drop(connection);
        assert_legacy_sync_runtime_removed(&database);
    }

    #[test]
    fn cloud_compatible_transition_creates_one_recurring_occurrence_without_outbox() {
        let (database, _, ordinary_task_id) = setup();
        let state = cloud_state(&database);
        let ordinary_operation_id = Uuid::now_v7().to_string();
        transition_timer_with_cloud_operation(
            &database,
            TimerTransitionRequest {
                client_request_id: ordinary_operation_id.clone(),
                current_entry_id: None,
                current_entry_expected_version: None,
                task_id: Some(ordinary_task_id),
                reopen_completed_task_version: None,
                new_task: None,
                note: None,
            },
            Some(&state),
        )
        .unwrap();
        let ordinary_occurrences: i64 = database
            .open()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM task_occurrences", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(ordinary_occurrences, 0);

        database
            .open()
            .unwrap()
            .execute(
                "UPDATE time_entries SET state='ended',ended_at=updated_at,version=version+1 WHERE state='running'",
                [],
            )
            .unwrap();
        let recurring = create_task(
            &database,
            TaskCreateRequest {
                subject_id: subject_id(&database),
                day_instance_date: None,
                parent_id: None,
                title: "云端周期计时目标".to_string(),
                planned_date: None,
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        save_rule(
            &database,
            RecurrenceSaveRequest {
                task_id: recurring.id.clone(),
                task_expected_version: recurring.version,
                frequency: "daily".to_string(),
                weekdays_mask: None,
                effective_start: test_work_date(),
                rule_expected_version: None,
            },
        )
        .unwrap();

        let first_operation_id = Uuid::now_v7().to_string();
        let first = transition_timer_with_cloud_operation(
            &database,
            TimerTransitionRequest {
                client_request_id: first_operation_id.clone(),
                current_entry_id: None,
                current_entry_expected_version: None,
                task_id: Some(recurring.id.clone()),
                reopen_completed_task_version: None,
                new_task: None,
                note: None,
            },
            Some(&state),
        )
        .unwrap();
        let occurrence_count: i64 = database
            .open()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM task_occurrences WHERE task_id=?1",
                [&recurring.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(occurrence_count, 1);

        database
            .open()
            .unwrap()
            .execute(
                "UPDATE time_entries SET state='ended',ended_at=updated_at,version=version+1 WHERE id=?1",
                [&first.active_entry.id],
            )
            .unwrap();
        let second_operation_id = Uuid::now_v7().to_string();
        transition_timer_with_cloud_operation(
            &database,
            TimerTransitionRequest {
                client_request_id: second_operation_id.clone(),
                current_entry_id: None,
                current_entry_expected_version: None,
                task_id: Some(recurring.id.clone()),
                reopen_completed_task_version: None,
                new_task: None,
                note: None,
            },
            Some(&state),
        )
        .unwrap();
        let connection = database.open().unwrap();
        let occurrence_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM task_occurrences WHERE task_id=?1",
                [&recurring.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(occurrence_count, 1);
        drop(connection);
        assert_legacy_sync_runtime_removed(&database);
    }

    fn seed_history_entry(
        database: &Database,
        workspace_id: &str,
        task_id: &str,
        work_date: &str,
        source_type: &str,
        kind: &str,
        started_at: i64,
        duration_seconds: i64,
        allocated_minutes: i64,
        deleted: bool,
    ) -> String {
        let entry_id = Uuid::now_v7().to_string();
        let ended_at = started_at + duration_seconds * 1_000;
        database
            .open()
            .unwrap()
            .execute(
                "INSERT INTO time_entries(
               id,workspace_id,work_date,kind,source_type,state,default_task_id,label_snapshot,
               started_at,ended_at,duration_seconds,created_at,updated_at,version,deleted_at
             ) VALUES (?1,?2,?3,?4,?5,'ended',?6,?7,?8,?9,?10,?8,?9,1,?11)",
                params![
                    entry_id,
                    workspace_id,
                    work_date,
                    kind,
                    source_type,
                    task_id,
                    format!("{work_date}-{source_type}-{kind}"),
                    started_at,
                    ended_at,
                    duration_seconds,
                    deleted.then_some(ended_at)
                ],
            )
            .unwrap();
        if allocated_minutes > 0 {
            database.open().unwrap().execute(
                "INSERT INTO time_allocations(id,workspace_id,entry_id,task_id,minutes,created_at,updated_at,version)
                 VALUES (?1,?2,?3,?4,?5,?6,?6,1)",
                params![Uuid::now_v7().to_string(),workspace_id,entry_id,task_id,allocated_minutes,ended_at],
            ).unwrap();
        }
        entry_id
    }

    #[test]
    fn history_result_serializes_camel_case_and_empty_page() {
        let (database, _, _) = setup();
        let result = list_time_entry_history(
            &database,
            TimeEntryHistoryRequest {
                page: 1,
                include_breaks: true,
            },
        )
        .unwrap();
        let value = serde_json::to_value(result).unwrap();
        assert_eq!(value["page"], 1);
        assert_eq!(value["pageSize"], TIME_ENTRY_HISTORY_DAYS_PER_PAGE);
        assert_eq!(value["totalDays"], 0);
        assert_eq!(value["totalPages"], 0);
        assert_eq!(value["groups"], json!([]));
        assert!(value.get("page_size").is_none());
    }

    #[test]
    fn history_pages_by_complete_non_empty_days_and_sorts_entries() {
        let (database, workspace_id, task_id) = setup();
        let base = shanghai_millis("2026-10-01 09:00:00");
        for day in 1..=16 {
            if day == 8 {
                continue;
            }
            let work_date = format!("2026-10-{day:02}");
            seed_history_entry(
                &database,
                &workspace_id,
                &task_id,
                &work_date,
                "manual",
                "work",
                base + day as i64 * 86_400_000,
                600,
                5,
                false,
            );
        }
        let later = seed_history_entry(
            &database,
            &workspace_id,
            &task_id,
            "2026-10-16",
            "manual",
            "work",
            base + 16 * 86_400_000,
            120,
            0,
            false,
        );

        let first = list_time_entry_history(
            &database,
            TimeEntryHistoryRequest {
                page: 1,
                include_breaks: true,
            },
        )
        .unwrap();
        let second = list_time_entry_history(
            &database,
            TimeEntryHistoryRequest {
                page: 2,
                include_breaks: true,
            },
        )
        .unwrap();
        let third = list_time_entry_history(
            &database,
            TimeEntryHistoryRequest {
                page: 3,
                include_breaks: true,
            },
        )
        .unwrap();
        assert_eq!(first.total_days, 15);
        assert_eq!(first.total_pages, 3);
        assert_eq!(first.groups.len(), 7);
        assert_eq!(second.groups.len(), 7);
        assert_eq!(third.groups.len(), 1);
        assert_eq!(first.groups[0].work_date, "2026-10-16");
        assert_eq!(first.groups[0].entries.len(), 2);
        assert_eq!(first.groups[0].entries[0].id, later);
        assert_eq!(first.groups[6].work_date, "2026-10-10");
        assert_eq!(second.groups[0].work_date, "2026-10-09");
        assert_eq!(third.groups[0].work_date, "2026-10-01");
        assert!(first
            .groups
            .iter()
            .all(|group| group.work_date != "2026-10-08"));
        assert!(second
            .groups
            .iter()
            .all(|group| group.work_date != "2026-10-08"));
        assert!(third
            .groups
            .iter()
            .all(|group| group.work_date != "2026-10-08"));
    }

    #[test]
    fn historical_entry_writes_are_rejected_without_disturbing_active_timer() {
        let (database, workspace_id, task_id) = setup();
        let base = shanghai_millis("2026-09-01 09:00:00");
        for day in 1..=7 {
            seed_history_entry(
                &database,
                &workspace_id,
                &task_id,
                &format!("2026-09-{day:02}"),
                "manual",
                "work",
                base + day as i64 * 86_400_000,
                600,
                0,
                false,
            );
        }
        let active = start_timer(
            &database,
            TimerStartRequest {
                task_id: Some(task_id.clone()),
                note: Some("验收中的当前计时".to_string()),
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        let oldest = list_time_entry_history(
            &database,
            TimeEntryHistoryRequest {
                page: 2,
                include_breaks: true,
            },
        )
        .unwrap();
        let target = &oldest.groups[0].entries[0];
        let before = get_time_entry_by_id(&database, &target.id).unwrap();
        let correction_error = update_time_entry(
            &database,
            TimeEntryUpdateRequest {
                entry_id: target.id.clone(),
                started_at: target.started_at - 60_000,
                ended_at: target.ended_at.unwrap() + 60_000,
                note: Some("历史修正".to_string()),
                expected_version: target.version,
            },
        )
        .unwrap_err();
        assert!(correction_error.starts_with("HISTORICAL_RECORD_READ_ONLY:"));
        let allocation_error = replace_allocations(
            &database,
            AllocationReplaceRequest {
                entry_id: target.id.clone(),
                expected_version: target.version,
                allocations: vec![AllocationInput {
                    task_id: task_id.clone(),
                    minutes: 5,
                    note: None,
                    complete_task: false,
                    task_expected_version: None,
                }],
            },
        )
        .unwrap_err();
        assert!(allocation_error.starts_with("HISTORICAL_RECORD_READ_ONLY:"));
        let current_before = get_timer_state(&database).unwrap().unwrap();
        assert_eq!(current_before.id, active.id);
        let refreshed = list_time_entry_history(
            &database,
            TimeEntryHistoryRequest {
                page: 2,
                include_breaks: true,
            },
        )
        .unwrap();
        let current_after = get_timer_state(&database).unwrap().unwrap();
        assert_eq!(refreshed.page, 2);
        assert_eq!(current_after.id, active.id);
        assert_eq!(current_after.state, "running");
        let disposition_error = update_time_entry_disposition(
            &database,
            TimeEntryDispositionRequest {
                entry_id: refreshed.groups[0].entries[0].id.clone(),
                expected_version: refreshed.groups[0].entries[0].version,
                disposition: "discard".to_string(),
            },
        )
        .unwrap_err();
        assert!(disposition_error.starts_with("HISTORICAL_RECORD_READ_ONLY:"));
        assert_eq!(
            get_time_entry_by_id(&database, &target.id).unwrap().version,
            before.version
        );
        assert_eq!(
            get_time_entry_by_id(&database, &target.id)
                .unwrap()
                .allocated_minutes,
            before.allocated_minutes
        );
        assert_eq!(get_timer_state(&database).unwrap().unwrap().id, active.id);
    }

    #[test]
    fn history_includes_formal_sources_summaries_and_excludes_deleted_entries() {
        let (database, workspace_id, task_id) = setup();
        let work_date = "2026-10-06";
        let base = shanghai_millis("2026-10-06 09:00:00");
        seed_history_entry(
            &database,
            &workspace_id,
            &task_id,
            work_date,
            "timer",
            "work",
            base,
            61,
            1,
            false,
        );
        seed_history_entry(
            &database,
            &workspace_id,
            &task_id,
            work_date,
            "manual",
            "work",
            base + 3_600_000,
            600,
            5,
            false,
        );
        seed_history_entry(
            &database,
            &workspace_id,
            &task_id,
            work_date,
            "unassigned",
            "break",
            base + 7_200_000,
            300,
            0,
            false,
        );
        seed_history_entry(
            &database,
            &workspace_id,
            &task_id,
            work_date,
            "manual",
            "work",
            base + 10_800_000,
            600,
            0,
            true,
        );
        let before: Vec<(String, i64)> = database
            .open()
            .unwrap()
            .prepare("SELECT id,version FROM time_entries ORDER BY id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_legacy_sync_runtime_removed(&database);

        let result = list_time_entry_history(
            &database,
            TimeEntryHistoryRequest {
                page: 1,
                include_breaks: true,
            },
        )
        .unwrap();
        let group = &result.groups[0];
        assert_eq!(group.entry_count, 3);
        assert_eq!(group.raw_minutes, 12);
        assert_eq!(group.allocated_minutes, 6);
        assert_eq!(group.pending_minutes, 6);
        assert_eq!(group.break_minutes, 5);
        assert_eq!(
            group
                .entries
                .iter()
                .map(|entry| entry.source_type.as_str())
                .collect::<Vec<_>>(),
            vec!["unassigned", "manual", "timer"]
        );
        let after: Vec<(String, i64)> = database
            .open()
            .unwrap()
            .prepare("SELECT id,version FROM time_entries ORDER BY id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(after, before);
        assert_legacy_sync_runtime_removed(&database);
    }

    #[test]
    fn cloud_compatible_time_entry_writes_commit_without_outbox() {
        let (database, _workspace_id, task_id) = setup();
        let completion_only_task = create_task(
            &database,
            TaskCreateRequest {
                subject_id: subject_id(&database),
                day_instance_date: None,
                parent_id: None,
                title: "云端只完成不分配".to_string(),
                planned_date: None,
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
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
        let state = crate::supabase::storage_mode(&database).unwrap();
        let operation_id = Uuid::now_v7().to_string();
        let started_at = now_millis() - 3_600_000;
        let ended_at = started_at + 1_800_000;

        let created = create_manual_entry_with_cloud_operation(
            &database,
            ManualEntryRequest {
                task_id: Some(task_id.clone()),
                work_date: test_work_date(),
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
                allocations: vec![
                    AllocationInput {
                        task_id: task_id.clone(),
                        minutes: 40,
                        note: Some("全部归属".to_string()),
                        complete_task: false,
                        task_expected_version: None,
                    },
                    AllocationInput {
                        task_id: completion_only_task.id.clone(),
                        minutes: 0,
                        note: None,
                        complete_task: true,
                        task_expected_version: Some(completion_only_task.version),
                    },
                ],
            },
            Some((&state, "time_allocation_replace")),
        )
        .unwrap();

        let connection = database.open().unwrap();
        let entry: (String, i64, i64) = connection
            .query_row(
                "SELECT note,duration_seconds,version FROM time_entries WHERE id=?1",
                [&created.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(entry, ("修正后的云端手动记录".to_string(), 2_400, 3));
        let allocation_minutes: i64 = connection
            .query_row(
                "SELECT minutes FROM time_allocations WHERE entry_id=?1 AND task_id=?2",
                params![created.id, task_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(allocation_minutes, 40);
        let completed_status: String = connection
            .query_row(
                "SELECT status FROM tasks WHERE id=?1",
                [&completion_only_task.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(completed_status, "done");
        drop(connection);
        assert_legacy_sync_runtime_removed(&database);
    }

    #[test]
    fn cloud_compatible_timer_start_does_not_create_outbox() {
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

        let active = get_timer_state(&database).unwrap().unwrap();
        assert_eq!(active.id, entry.id);
        assert_eq!(active.state, "running");

        assert_legacy_sync_runtime_removed(&database);
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
        assert_legacy_sync_runtime_removed(&database);

        for _ in 0..12 {
            let refreshed = get_timer_state(&database).unwrap().unwrap();
            assert_eq!(refreshed.id, started.id);
        }

        assert_legacy_sync_runtime_removed(&database);
    }

    #[test]
    fn cloud_compatible_timer_lifecycle_commits_without_outbox() {
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
        let (state_value, version): (String, i64) = connection
            .query_row(
                "SELECT state,version FROM time_entries WHERE id=?1",
                [&started.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(state_value, "ended");
        assert_eq!(version, resumed.version + 1);
        let segment_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM time_segments WHERE entry_id=?1",
                [&started.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(segment_count, 2);
        drop(connection);
        assert_legacy_sync_runtime_removed(&database);
    }

    #[test]
    fn timer_lifecycle_keeps_one_contiguous_chain_without_outbox() {
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
        let state = crate::supabase::storage_mode(&database).unwrap();
        let start_operation_id = Uuid::now_v7().to_string();
        let started = start_timer_with_cloud_operation(
            &database,
            TimerStartRequest {
                task_id: Some(task_id.clone()),
                note: Some("离线完整生命周期".to_string()),
                client_request_id: start_operation_id.clone(),
            },
            Some((&state, "timer_start", Some(start_operation_id.as_str()))),
        )
        .unwrap();

        let recovery_at = now_millis();
        let original_started_at = recovery_at - 600_000;
        let original_last_continuous_at = recovery_at - 120_000;
        database
            .open()
            .unwrap()
            .execute(
                "UPDATE time_entries SET started_at=?1,last_continuous_at=?2,updated_at=?2 WHERE id=?3",
                params![original_started_at, original_last_continuous_at, started.id],
            )
            .unwrap();
        database
            .open()
            .unwrap()
            .execute(
                "UPDATE time_segments SET started_at=?1 WHERE entry_id=?2 AND ended_at IS NULL",
                params![original_started_at, started.id],
            )
            .unwrap();
        coordinate_active_timer(&database, recovery_at).unwrap();

        let recovered = get_time_entry_by_id(&database, &started.id).unwrap();
        assert_eq!(recovered.version, 2);
        let paused = pause_timer_with_cloud_operation(
            &database,
            TimerVersionRequest {
                entry_id: started.id.clone(),
                expected_version: recovered.version,
            },
            Some((&state, "timer_pause")),
        )
        .unwrap();
        let resume_operation_id = Uuid::now_v7().to_string();
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
        let stopped = stop_timer_with_cloud_operation(
            &database,
            TimerStopRequest {
                entry_id: started.id.clone(),
                expected_version: resumed.version,
                create_default_allocation: false,
            },
            Some((&state, "timer_stop")),
        )
        .unwrap();
        let allocated = replace_allocations_with_cloud_operation(
            &database,
            AllocationReplaceRequest {
                entry_id: started.id.clone(),
                expected_version: stopped.version,
                allocations: vec![AllocationInput {
                    task_id: task_id.clone(),
                    minutes: stopped.settlement_minutes,
                    note: Some("离线结束后分配".to_string()),
                    complete_task: false,
                    task_expected_version: None,
                }],
            },
            Some((&state, "time_allocation_replace")),
        )
        .unwrap();

        assert_eq!(allocated.version, 6);
        assert_eq!(allocated.allocations.len(), 1);
        let connection = database.open().unwrap();
        let segment_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM time_segments WHERE entry_id=?1",
                [&started.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(segment_count, 3);
        drop(connection);
        assert_legacy_sync_runtime_removed(&database);
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
                day_instance_date: None,
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
                day_instance_date: None,
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
                day_instance_date: None,
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
    fn manual_entry_allows_allocation_over_duration() {
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
        let overallocated = replace_allocations(
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
            },
        )
        .unwrap();
        assert_eq!(overallocated.settlement_minutes, 30);
        assert_eq!(overallocated.allocated_minutes, 31);
        let summary = list_time_entries(
            &database,
            TimeEntryListRequest {
                work_date: test_work_date(),
                include_breaks: true,
            },
        )
        .unwrap();
        assert_eq!(summary.pending_minutes, 0);
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
                day_instance_date: None,
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
    fn allocation_can_complete_a_zero_minute_task_without_creating_an_allocation() {
        let (database, _, allocated_task_id) = setup();
        let subject_id = subject_id(&database);
        let completion_only_task = create_task(
            &database,
            TaskCreateRequest {
                subject_id,
                day_instance_date: None,
                parent_id: None,
                title: "只完成不分配时间".to_string(),
                planned_date: None,
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
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
                entry_id: manual.id,
                expected_version: manual.version,
                allocations: vec![
                    AllocationInput {
                        task_id: allocated_task_id.clone(),
                        minutes: 30,
                        note: None,
                        complete_task: false,
                        task_expected_version: None,
                    },
                    AllocationInput {
                        task_id: completion_only_task.id.clone(),
                        minutes: 0,
                        note: None,
                        complete_task: true,
                        task_expected_version: Some(completion_only_task.version),
                    },
                ],
            },
        )
        .unwrap();

        assert_eq!(allocated.allocated_minutes, 30);
        assert_eq!(allocated.allocations.len(), 1);
        assert_eq!(allocated.allocations[0].task_id, allocated_task_id);
        let connection = database.open().unwrap();
        let completion_only_allocations: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM time_allocations WHERE entry_id = ?1 AND task_id = ?2",
                params![allocated.id, completion_only_task.id],
                |row| row.get(0),
            )
            .unwrap();
        let completion_only_status: String = connection
            .query_row(
                "SELECT status FROM tasks WHERE id = ?1",
                [&completion_only_task.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(completion_only_allocations, 0);
        assert_eq!(completion_only_status, "done");
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
                day_instance_date: None,
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
    fn cloud_compatible_recurring_completion_updates_occurrence_without_outbox() {
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
        let occurrence_status: String = connection
            .query_row(
                "SELECT status FROM task_occurrences WHERE task_id=?1 AND occurrence_date=?2",
                params![task_id, today],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(occurrence_status, "done");
        drop(connection);
        assert_legacy_sync_runtime_removed(&database);
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
    fn corrected_entry_preserves_allocation_over_corrected_duration() {
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
                ended_at: manual.started_at + 15 * 60_000,
                note: Some("修正为 15 分钟".to_string()),
                expected_version: allocated.version,
            },
        )
        .unwrap();
        assert_eq!(corrected.settlement_minutes, 15);
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
        assert_eq!(reallocated.settlement_minutes, 15);
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
    fn repeated_continuity_closes_update_business_state_without_outbox() {
        let (database, workspace_id, task_id) = setup();
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
        let started_at = shanghai_millis("2026-10-06 09:00:00");
        let first_recovery = shanghai_millis("2026-10-06 10:00:00");
        let second_recovery = shanghai_millis("2026-10-06 11:00:00");
        let entry_id = seed_active_timer_at(
            &database,
            &workspace_id,
            &task_id,
            "running",
            started_at,
            started_at + 60_000,
        );

        coordinate_active_timer(&database, first_recovery).unwrap();
        coordinate_active_timer(&database, second_recovery).unwrap();
        let connection = database.open().unwrap();
        let version: i64 = connection
            .query_row(
                "SELECT version FROM time_entries WHERE id=?1",
                [&entry_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, 3);
        let segment_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM time_segments WHERE entry_id=?1",
                [&entry_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(segment_count, 3);
        drop(connection);
        assert_legacy_sync_runtime_removed(&database);
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
    fn cloud_compatible_rollover_splits_entries_without_outbox() {
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
        let entries: Vec<(String, String)> = connection.prepare(
            "SELECT work_date,state FROM time_entries WHERE timer_chain_id=?1 ORDER BY work_date"
        ).unwrap().query_map([&entry_id], |row| Ok((row.get(0)?, row.get(1)?))).unwrap().collect::<Result<_, _>>().unwrap();
        assert_eq!(
            entries,
            vec![
                (yesterday, "ended".to_string()),
                (today, "running".to_string())
            ]
        );
        drop(connection);
        coordinate_active_timer(&database, boundary + 30_000).unwrap();
        assert_legacy_sync_runtime_removed(&database);
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
