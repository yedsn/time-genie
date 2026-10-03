use std::collections::HashMap;

use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::database::Database;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UnassignedStateDto {
    pub session_id: String,
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

pub fn get_state(database: &Database) -> Result<Option<UnassignedStateDto>, String> {
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let now = now_millis();
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let active_timer = has_active_timer(&transaction, &workspace_id)?;
    let session_id = active_session_id(&transaction, &workspace_id)?;
    let session_id = match (session_id, active_timer) {
        (Some(id), _) => Some(id),
        (None, false) => Some(create_session(&transaction, &workspace_id, now)?),
        (None, true) => None,
    };
    let result = session_id
        .map(|id| load_state(&transaction, &id, now, true))
        .transpose()?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

pub fn resolve_work(
    database: &Database,
    request: ResolveWorkRequest,
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
                &local_date(),
            )?;
        }
    }
    for (task_id, (minutes, complete_task, expected_version)) in merged {
        let work_date = local_date();
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
            crate::time_tracking::complete_task_if_open(
                &transaction,
                &workspace_id,
                &task_id,
                expected_version,
                &work_date,
                now,
            )?;
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
    let result = UnassignedResolveResult {
        session_id: request.session_id,
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
    start_next_session_if_idle(&transaction, &workspace_id, now)?;
    bump_revision(&transaction)?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

pub fn resolve_work_with_hooks(
    database: &Database,
    request: ResolveWorkRequest,
) -> Result<UnassignedResolveResult, String> {
    let work_date = local_date();
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
    let result = resolve_work(database, request)?;
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

pub fn resolve_break(
    database: &Database,
    request: ResolveSessionRequest,
) -> Result<UnassignedResolveResult, String> {
    resolve_without_allocations(database, request, "break")
}

pub fn discard(
    database: &Database,
    request: ResolveSessionRequest,
) -> Result<UnassignedResolveResult, String> {
    resolve_without_allocations(database, request, "discard")
}

fn resolve_without_allocations(
    database: &Database,
    request: ResolveSessionRequest,
    resolution_type: &str,
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
    let result = UnassignedResolveResult {
        session_id: request.session_id,
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
    start_next_session_if_idle(&transaction, &workspace_id, now)?;
    bump_revision(&transaction)?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

pub fn pause_for_timer(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    now: i64,
) -> Result<(), String> {
    let Some(session_id) = active_session_id(transaction, workspace_id)? else {
        return Ok(());
    };
    close_open_segment(transaction, &session_id, now)?;
    refresh_session_duration(transaction, &session_id, now)?;
    Ok(())
}

pub fn resume_after_timer(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    now: i64,
) -> Result<(), String> {
    let session_id = match active_session_id(transaction, workspace_id)? {
        Some(id) => id,
        None => create_session(transaction, workspace_id, now)?,
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
    close_open_segment(transaction, session_id, now)?;
    refresh_session_duration(transaction, session_id, now)?;
    load_state(transaction, session_id, now, false)
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
    transaction
        .execute(
            "INSERT INTO time_entries(
               id, workspace_id, work_date, kind, source_type, state, default_task_id,
               label_snapshot, started_at, ended_at, duration_seconds, note, origin_unassigned_session_id,
               created_at, updated_at, version, created_by_device_id, updated_by_device_id
             ) VALUES (?1, ?2, ?3, ?4, 'unassigned', 'ended', NULL, ?5, ?6, ?7, ?8, ?9, ?10, ?7, ?7, 1, ?11, ?11)",
            params![
                entry_id,
                workspace_id,
                local_date(),
                kind,
                label,
                state.first_started_at,
                now,
                state.elapsed_seconds,
                if kind == "break" { "未执行事项期间记录为休息" } else { "未启动事项期间补分配" },
                state.session_id,
                device_id
            ],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO time_segments(id, workspace_id, entry_id, sequence_no, started_at, ended_at, duration_seconds)
             VALUES (?1, ?2, ?3, 1, ?4, ?5, ?6)",
            params![Uuid::now_v7().to_string(), workspace_id, entry_id, state.first_started_at, now, state.elapsed_seconds],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn create_session(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    now: i64,
) -> Result<String, String> {
    let id = Uuid::now_v7().to_string();
    let threshold = prompt_threshold(transaction, workspace_id)?;
    transaction
        .execute(
            "INSERT INTO unassigned_sessions(
               id, workspace_id, work_date, state, threshold_seconds, duration_seconds,
               first_started_at, created_at, updated_at, version
             ) VALUES (?1, ?2, ?3, 'collecting', ?4, 0, ?5, ?5, ?5, 1)",
            params![id, workspace_id, local_date(), threshold, now],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO unassigned_segments(id, workspace_id, session_id, sequence_no, started_at, duration_seconds)
             VALUES (?1, ?2, ?3, 1, ?4, 0)",
            params![Uuid::now_v7().to_string(), workspace_id, id, now],
        )
        .map_err(|error| error.to_string())?;
    Ok(id)
}

fn start_next_session_if_idle(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    now: i64,
) -> Result<(), String> {
    if !has_active_timer(transaction, workspace_id)? {
        create_session(transaction, workspace_id, now)?;
    }
    Ok(())
}

fn load_state(
    connection: &rusqlite::Connection,
    session_id: &str,
    now: i64,
    update_threshold_state: bool,
) -> Result<UnassignedStateDto, String> {
    let (state, first_started_at, last_ended_at, duration_seconds, threshold_seconds, version):
        (String, i64, Option<i64>, i64, i64, i64) = connection
        .query_row(
            "SELECT state, first_started_at, last_ended_at, duration_seconds, threshold_seconds, version
             FROM unassigned_sessions WHERE id = ?1",
            [session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
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

fn active_session_id(
    connection: &rusqlite::Connection,
    workspace_id: &str,
) -> Result<Option<String>, String> {
    connection
        .query_row(
            "SELECT id FROM unassigned_sessions
             WHERE workspace_id = ?1 AND state IN ('collecting', 'awaiting_resolution')
             ORDER BY created_at LIMIT 1",
            [workspace_id],
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

fn local_date() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
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
    get_state(&database)
}

#[tauri::command]
pub fn unassigned_resolve_work(
    database: tauri::State<'_, Database>,
    request: ResolveWorkRequest,
) -> Result<UnassignedResolveResult, String> {
    crate::supabase::ensure_local_mode(&database)?;
    resolve_work_with_hooks(&database, request)
}

#[tauri::command]
pub fn unassigned_resolve_break(
    database: tauri::State<'_, Database>,
    request: ResolveSessionRequest,
) -> Result<UnassignedResolveResult, String> {
    crate::supabase::ensure_local_mode(&database)?;
    resolve_break(&database, request)
}

#[tauri::command]
pub fn unassigned_discard(
    database: tauri::State<'_, Database>,
    request: ResolveSessionRequest,
) -> Result<UnassignedResolveResult, String> {
    crate::supabase::ensure_local_mode(&database)?;
    discard(&database, request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::{create_task, TaskCreateRequest};
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
}
