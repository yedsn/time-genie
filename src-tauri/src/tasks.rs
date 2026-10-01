use std::collections::{HashMap, HashSet};

use chrono::NaiveDate;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::database::Database;
use crate::recurring::{RecurrenceRuleDto, TaskOccurrenceDto};

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TaskDto {
    pub id: String,
    pub subject_id: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub status: String,
    pub planned_date: Option<String>,
    pub estimate_minutes: Option<i64>,
    pub today_estimate_minutes: Option<i64>,
    pub note: Option<String>,
    pub project_name: Option<String>,
    pub solution_name: Option<String>,
    pub sort_order: i64,
    pub depth: usize,
    pub path_label: String,
    pub selectable: bool,
    pub direct_minutes: i64,
    pub total_minutes: i64,
    pub execution_state: String,
    pub recurrence: Option<RecurrenceRuleDto>,
    pub occurrence_date: Option<String>,
    pub occurrence_origin: Option<String>,
    pub occurrence_version: Option<i64>,
    pub version: i64,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TaskListResult {
    pub tasks: Vec<TaskDto>,
    pub revision: i64,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TaskListRequest {
    pub subject_id: Option<String>,
    pub include_completed: Option<bool>,
    pub planned_date: Option<String>,
    pub today_date: Option<String>,
    pub today_only: Option<bool>,
    pub daily_estimate_date: Option<String>,
    pub query: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDailyEstimateSetRequest {
    pub task_id: String,
    pub work_date: String,
    pub estimate_minutes: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDailyEstimateDto {
    pub task_id: String,
    pub work_date: String,
    pub estimate_minutes: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskCreateRequest {
    pub subject_id: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub planned_date: Option<String>,
    pub estimate_minutes: Option<i64>,
    pub note: Option<String>,
    pub project_name: Option<String>,
    pub solution_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskUpdateRequest {
    pub id: String,
    pub expected_version: i64,
    pub title: Option<String>,
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub parent_id: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub planned_date: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub estimate_minutes: Option<Option<i64>>,
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub note: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub project_name: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub solution_name: Option<Option<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStatusRequest {
    pub id: String,
    pub expected_version: i64,
    pub done: bool,
    pub occurrence_date: Option<String>,
    pub occurrence_expected_version: Option<i64>,
}

pub(crate) fn task_is_completed(
    database: &Database,
    task_id: &str,
    occurrence_date: Option<&str>,
) -> Result<bool, String> {
    let connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    if let Some(date) = occurrence_date {
        if crate::recurring::get_active_rule(&connection, &workspace_id, task_id)?.is_some()
            || crate::recurring::get_rule_for_date(&connection, &workspace_id, task_id, date)?
                .is_some()
        {
            return Ok(crate::recurring::occurrence_for_date(
                &connection,
                &workspace_id,
                task_id,
                date,
            )?
            .is_some_and(|occurrence| occurrence.status == "done"));
        }
    }
    let status: String = connection
        .query_row(
            "SELECT status FROM tasks WHERE id = ?1 AND workspace_id = ?2 AND deleted_at IS NULL",
            params![task_id, workspace_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    Ok(status == "done")
}

pub(crate) fn load_task_for_hook(
    database: &Database,
    task_id: &str,
    occurrence_date: Option<&str>,
) -> Result<TaskDto, String> {
    let connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let mut task = load_task(&connection, task_id)?;
    apply_recurrence_context(
        &connection,
        &workspace_id,
        occurrence_date,
        std::slice::from_mut(&mut task),
    )?;
    let tasks = list_tasks(
        database,
        TaskListRequest {
            subject_id: Some(task.subject_id.clone()),
            include_completed: Some(true),
            planned_date: None,
            today_date: occurrence_date.map(ToOwned::to_owned),
            today_only: Some(false),
            daily_estimate_date: None,
            query: None,
        },
    )?
    .tasks;
    tasks
        .into_iter()
        .find(|candidate| candidate.id == task_id)
        .ok_or_else(|| "NOT_FOUND: 事项不存在".to_string())
}

pub(crate) fn publish_task_completed_hook(
    database: &Database,
    task_id: &str,
    occurrence_date: Option<&str>,
    operation_id: Option<&str>,
) {
    let result = load_task_for_hook(database, task_id, occurrence_date).and_then(|task| {
        let version = task.occurrence_version.unwrap_or(task.version);
        let event_id = operation_id
            .map(|value| format!("task.completed:{value}:{task_id}"))
            .unwrap_or_else(|| {
                format!(
                    "task.completed:{task_id}:{}:{version}",
                    occurrence_date.unwrap_or("global")
                )
            });
        crate::automation_hooks::task_completed_event(database, event_id, &task)
            .and_then(|event| crate::automation_hooks::publish(database.clone(), event).map(|_| ()))
    });
    if let Err(error) = result {
        eprintln!("事项完成 Hook 调度失败，不影响事项结果: {error}");
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDeleteRequest {
    pub id: String,
    pub expected_version: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskReorderRequest {
    pub id: String,
    pub expected_version: i64,
    pub parent_id: Option<String>,
    pub sort_order: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskChangeParentRequest {
    pub id: String,
    pub expected_version: i64,
    pub new_parent_id: Option<String>,
    pub sort_order: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDuplicateRequest {
    pub id: String,
    pub expected_version: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDuplicateResult {
    pub root_id: String,
    pub tasks: Vec<TaskDto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanImportPreview {
    pub source_markdown: String,
    pub items: Vec<PlanImportItemDto>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanImportItemDto {
    pub client_id: String,
    pub parent_client_id: Option<String>,
    pub title: String,
    pub estimate_minutes: Option<i64>,
    pub sort_order: i64,
    pub source_line_no: usize,
    pub source_text: String,
    pub parse_status: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanImportPreviewRequest {
    pub markdown: String,
}

pub fn list_tasks(database: &Database, request: TaskListRequest) -> Result<TaskListResult, String> {
    let connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let include_completed = request.include_completed.unwrap_or(true);
    let query = request
        .query
        .as_ref()
        .map(|value| format!("%{}%", value.trim()))
        .filter(|value| value != "%%");
    let mut statement = connection
        .prepare(
            "SELECT id, subject_id, parent_id, title, status, planned_date,
                    estimate_minutes, note, project_name, solution_name, sort_order, version
             FROM tasks
             WHERE workspace_id = ?1 AND deleted_at IS NULL
               AND (?2 IS NULL OR subject_id = ?2)
               AND (?3 IS NULL OR planned_date = ?3)
               AND (?4 = 1 OR status = 'open')
               AND (?5 IS NULL OR title LIKE ?5)
             ORDER BY subject_id, sort_order, created_at",
        )
        .map_err(|error| error.to_string())?;
    let mut rows = statement
        .query_map(
            params![
                workspace_id,
                request.subject_id,
                request.planned_date,
                include_completed,
                query
            ],
            task_from_row,
        )
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    if let Some(work_date) = request.daily_estimate_date.as_deref() {
        apply_daily_estimates(&connection, &workspace_id, work_date, &mut rows)?;
    }
    apply_recurrence_context(
        &connection,
        &workspace_id,
        request.today_date.as_deref(),
        &mut rows,
    )?;
    enrich_and_order_tasks(
        &connection,
        &workspace_id,
        request.today_date.as_deref(),
        &mut rows,
    )?;
    if request.today_only.unwrap_or(false) {
        let today_date = request
            .today_date
            .as_deref()
            .ok_or_else(|| "VALIDATION_ERROR: 今日事项筛选缺少日期".to_string())?;
        filter_today_tasks(
            &connection,
            &workspace_id,
            request.subject_id.as_deref(),
            today_date,
            &mut rows,
        )?;
    }
    let revision = connection
        .query_row(
            "SELECT CAST(value AS INTEGER) FROM app_metadata WHERE key = 'global_revision'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    Ok(TaskListResult {
        tasks: rows,
        revision,
    })
}

pub fn set_task_daily_estimate(
    database: &Database,
    request: TaskDailyEstimateSetRequest,
) -> Result<TaskDailyEstimateDto, String> {
    validate_date(&request.work_date)?;
    validate_estimate(request.estimate_minutes)?;
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let exists: Option<i64> = connection
        .query_row(
            "SELECT 1 FROM tasks WHERE id = ?1 AND workspace_id = ?2 AND deleted_at IS NULL",
            params![request.task_id, workspace_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if exists.is_none() {
        return Err("NOT_FOUND: 事项不存在".to_string());
    }

    let now = now_millis();
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    crate::recurring::ensure_occurrence_if_recurring(
        &transaction,
        &workspace_id,
        &request.task_id,
        &request.work_date,
        now,
    )?;
    if let Some(minutes) = request.estimate_minutes {
        transaction
            .execute(
                "INSERT INTO task_daily_estimates(
                    workspace_id, task_id, work_date, estimate_minutes, created_at, updated_at, version
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?5, 1)
                 ON CONFLICT(task_id, work_date) DO UPDATE SET
                    estimate_minutes = excluded.estimate_minutes,
                    updated_at = excluded.updated_at,
                    version = task_daily_estimates.version + 1",
                params![workspace_id, request.task_id, request.work_date, minutes, now],
            )
            .map_err(|error| error.to_string())?;
    } else {
        transaction
            .execute(
                "DELETE FROM task_daily_estimates
                 WHERE workspace_id = ?1 AND task_id = ?2 AND work_date = ?3",
                params![workspace_id, request.task_id, request.work_date],
            )
            .map_err(|error| error.to_string())?;
    }
    bump_revision(&transaction)?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(TaskDailyEstimateDto {
        task_id: request.task_id,
        work_date: request.work_date,
        estimate_minutes: request.estimate_minutes,
    })
}

pub fn create_task(database: &Database, request: TaskCreateRequest) -> Result<TaskDto, String> {
    create_task_with_source(database, request, "manual")
}

pub fn create_quick_task(
    database: &Database,
    request: TaskCreateRequest,
) -> Result<TaskDto, String> {
    create_task_with_source(database, request, "timer_quick_create")
}

fn create_task_with_source(
    database: &Database,
    request: TaskCreateRequest,
    source_type: &str,
) -> Result<TaskDto, String> {
    validate_title(&request.title)?;
    validate_estimate(request.estimate_minutes)?;
    let connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    ensure_subject(&connection, &workspace_id, &request.subject_id)?;
    if let Some(parent_id) = &request.parent_id {
        ensure_parent(&connection, &workspace_id, parent_id, &request.subject_id)?;
        ensure_no_direct_time(&connection, parent_id)?;
        ensure_not_recurring(&connection, &workspace_id, parent_id)?;
    }
    let now = now_millis();
    let id = Uuid::now_v7().to_string();
    let sort_order = next_sort_order(
        &connection,
        &workspace_id,
        &request.subject_id,
        request.parent_id.as_deref(),
    )?;
    connection
        .execute(
            "INSERT INTO tasks(
                id, workspace_id, subject_id, parent_id, title, status, planned_date,
                estimate_minutes, note, project_name, solution_name, source_type, sort_order,
                created_at, updated_at, version
             ) VALUES (?1, ?2, ?3, ?4, ?5, 'open', ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13, 1)",
            params![
                id,
                workspace_id,
                request.subject_id,
                request.parent_id,
                request.title.trim(),
                request.planned_date,
                request.estimate_minutes,
                request.note,
                request.project_name,
                request.solution_name,
                source_type,
                sort_order,
                now
            ],
        )
        .map_err(|error| error.to_string())?;
    bump_revision(&connection)?;
    load_task(&connection, &id)
}

pub fn update_task(database: &Database, request: TaskUpdateRequest) -> Result<TaskDto, String> {
    if let Some(title) = &request.title {
        validate_title(title)?;
    }
    if let Some(estimate) = request.estimate_minutes.flatten() {
        validate_estimate(Some(estimate))?;
    }
    let connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let current = load_task(&connection, &request.id)?;
    if current.version != request.expected_version {
        return Err("VERSION_CONFLICT: 事项已被其他窗口或设备更新".to_string());
    }
    if let Some(parent_id) = request.parent_id.as_ref().and_then(Clone::clone) {
        ensure_parent(&connection, &workspace_id, &parent_id, &current.subject_id)?;
        if parent_id == current.id || is_descendant(&connection, &current.id, &parent_id)? {
            return Err("TASK_TREE_CYCLE: 事项层级不能形成循环".to_string());
        }
        ensure_not_recurring(&connection, &workspace_id, &parent_id)?;
    }
    let now = now_millis();
    connection
        .execute(
            "UPDATE tasks SET
               title = COALESCE(?1, title),
               parent_id = CASE WHEN ?2 THEN ?3 ELSE parent_id END,
               planned_date = CASE WHEN ?4 THEN ?5 ELSE planned_date END,
               estimate_minutes = CASE WHEN ?6 THEN ?7 ELSE estimate_minutes END,
               note = CASE WHEN ?8 THEN ?9 ELSE note END,
               project_name = CASE WHEN ?10 THEN ?11 ELSE project_name END,
               solution_name = CASE WHEN ?12 THEN ?13 ELSE solution_name END,
               updated_at = ?14, version = version + 1
             WHERE id = ?15 AND workspace_id = ?16 AND deleted_at IS NULL AND version = ?17",
            params![
                request.title,
                request.parent_id.is_some(),
                request.parent_id.flatten(),
                request.planned_date.is_some(),
                request.planned_date.flatten(),
                request.estimate_minutes.is_some(),
                request.estimate_minutes.flatten(),
                request.note.is_some(),
                request.note.flatten(),
                request.project_name.is_some(),
                request.project_name.flatten(),
                request.solution_name.is_some(),
                request.solution_name.flatten(),
                now,
                request.id,
                workspace_id,
                request.expected_version
            ],
        )
        .map_err(|error| error.to_string())?;
    if connection.changes() == 0 {
        return Err("VERSION_CONFLICT: 事项已被其他窗口或设备更新".to_string());
    }
    bump_revision(&connection)?;
    load_task(&connection, &request.id)
}

pub fn set_task_status(database: &Database, request: TaskStatusRequest) -> Result<TaskDto, String> {
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let current = load_task(&connection, &request.id)?;
    if current.version != request.expected_version {
        return Err("VERSION_CONFLICT: 事项已被其他窗口或设备更新".to_string());
    }
    let now = now_millis();
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    if let Some(occurrence_date) = request.occurrence_date.as_deref() {
        if crate::recurring::get_active_rule(&transaction, &workspace_id, &request.id)?.is_some()
            || crate::recurring::get_rule_for_date(
                &transaction,
                &workspace_id,
                &request.id,
                occurrence_date,
            )?
            .is_some()
        {
            crate::recurring::set_occurrence_status(
                &transaction,
                &workspace_id,
                &request.id,
                occurrence_date,
                request.done,
                request.occurrence_expected_version,
                now,
            )?;
            bump_revision(&transaction)?;
            transaction.commit().map_err(|error| error.to_string())?;
            let connection = database.open()?;
            let mut task = load_task(&connection, &current.id)?;
            apply_recurrence_context(
                &connection,
                &workspace_id,
                Some(occurrence_date),
                std::slice::from_mut(&mut task),
            )?;
            return Ok(task);
        }
    }
    if crate::recurring::task_has_active_rule(&transaction, &workspace_id, &request.id)? {
        return Err("VALIDATION_ERROR: 重复事项完成操作必须指定轮次日期".to_string());
    }
    let changed = transaction
        .execute(
            "UPDATE tasks SET status = ?1, completed_at = ?2, updated_at = ?3, version = version + 1
             WHERE id = ?4 AND workspace_id = ?5 AND version = ?6 AND deleted_at IS NULL",
            params![if request.done { "done" } else { "open" }, if request.done { Some(now) } else { None }, now, request.id, workspace_id, request.expected_version],
        )
        .map_err(|error| error.to_string())?;
    if changed == 0 {
        return Err("VERSION_CONFLICT: 事项已被其他窗口或设备更新".to_string());
    }
    transaction
        .execute(
            "INSERT INTO task_status_events(id, workspace_id, task_id, status, occurred_at, source_type, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'user', ?5)",
            params![Uuid::now_v7().to_string(), workspace_id, request.id, if request.done { "done" } else { "open" }, now],
        )
        .map_err(|error| error.to_string())?;
    bump_revision(&transaction)?;
    transaction.commit().map_err(|error| error.to_string())?;
    let connection = database.open()?;
    load_task(&connection, &current.id)
}

pub fn delete_task(
    database: &Database,
    request: TaskDeleteRequest,
) -> Result<Vec<(String, i64)>, String> {
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let current = load_task(&connection, &request.id)?;
    if current.version != request.expected_version {
        return Err("VERSION_CONFLICT: 事项已被其他窗口或设备更新".to_string());
    }
    let now = now_millis();
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let subtree_versions = {
        let mut statement = transaction
            .prepare(
                "WITH RECURSIVE subtree(id, version) AS (
                   SELECT id, version FROM tasks
                   WHERE workspace_id = ?1 AND id = ?2 AND deleted_at IS NULL
                   UNION ALL
                   SELECT child.id, child.version FROM tasks child
                   JOIN subtree parent ON child.parent_id = parent.id
                   WHERE child.workspace_id = ?1 AND child.deleted_at IS NULL
                 ) SELECT id, version FROM subtree",
            )
            .map_err(|error| error.to_string())?;
        let versions = statement
            .query_map(params![workspace_id, request.id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        versions
    };
    let changed = transaction
        .execute(
            "UPDATE tasks SET deleted_at = ?1, updated_at = ?1, version = version + 1
             WHERE workspace_id = ?2 AND id = ?3 AND version = ?4 AND deleted_at IS NULL",
            params![now, workspace_id, request.id, request.expected_version],
        )
        .map_err(|error| error.to_string())?;
    if changed == 0 {
        return Err("VERSION_CONFLICT: 事项已被其他窗口或设备更新".to_string());
    }
    transaction
        .execute(
            "WITH RECURSIVE subtree(id) AS (
               SELECT id FROM tasks WHERE workspace_id = ?2 AND parent_id = ?3 AND deleted_at IS NULL
               UNION ALL
               SELECT child.id FROM tasks child JOIN subtree parent ON child.parent_id = parent.id
               WHERE child.workspace_id = ?2 AND child.deleted_at IS NULL
             )
             UPDATE tasks SET deleted_at = ?1, updated_at = ?1, version = version + 1
             WHERE id IN (SELECT id FROM subtree)",
            params![now, workspace_id, request.id],
        )
        .map_err(|error| error.to_string())?;
    bump_revision(&transaction)?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(subtree_versions)
}

pub fn reorder_task(database: &Database, request: TaskReorderRequest) -> Result<TaskDto, String> {
    let connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let current = load_task(&connection, &request.id)?;
    if current.version != request.expected_version {
        return Err("VERSION_CONFLICT: 事项已被其他窗口或设备更新".to_string());
    }
    if current.parent_id != request.parent_id {
        return Err("VALIDATION_ERROR: 拖动排序不能改变事项层级".to_string());
    }
    let now = now_millis();
    connection
        .execute(
            "UPDATE tasks SET parent_id = ?1, sort_order = ?2, updated_at = ?3, version = version + 1
             WHERE id = ?4 AND workspace_id = ?5 AND version = ?6 AND deleted_at IS NULL",
            params![request.parent_id, request.sort_order, now, request.id, workspace_id, request.expected_version],
        )
        .map_err(|error| error.to_string())?;
    if connection.changes() == 0 {
        return Err("VERSION_CONFLICT: 事项已被其他窗口或设备更新".to_string());
    }
    bump_revision(&connection)?;
    load_task(&connection, &request.id)
}

pub fn change_task_parent(
    database: &Database,
    request: TaskChangeParentRequest,
) -> Result<TaskDto, String> {
    let connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let current = load_task(&connection, &request.id)?;
    if current.version != request.expected_version {
        return Err("VERSION_CONFLICT: 事项已被其他窗口或设备更新".to_string());
    }
    if let Some(parent_id) = &request.new_parent_id {
        ensure_parent(&connection, &workspace_id, parent_id, &current.subject_id)?;
        if parent_id == &request.id || is_descendant(&connection, &request.id, parent_id)? {
            return Err("TASK_TREE_CYCLE: 事项层级不能形成循环".to_string());
        }
        ensure_no_direct_time(&connection, parent_id)?;
        ensure_not_recurring(&connection, &workspace_id, parent_id)?;
    }
    let sort_order = request.sort_order.unwrap_or(next_sort_order(
        &connection,
        &workspace_id,
        &current.subject_id,
        request.new_parent_id.as_deref(),
    )?);
    let changed = connection
        .execute(
            "UPDATE tasks SET parent_id = ?1, sort_order = ?2, updated_at = ?3, version = version + 1
             WHERE id = ?4 AND workspace_id = ?5 AND version = ?6 AND deleted_at IS NULL",
            params![request.new_parent_id, sort_order, now_millis(), request.id, workspace_id, request.expected_version],
        )
        .map_err(|error| error.to_string())?;
    if changed == 0 {
        return Err("VERSION_CONFLICT: 事项已被其他窗口或设备更新".to_string());
    }
    bump_revision(&connection)?;
    load_task(&connection, &current.id)
}

pub fn duplicate_task_subtree(
    database: &Database,
    request: TaskDuplicateRequest,
) -> Result<TaskDuplicateResult, String> {
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let root = load_task(&connection, &request.id)?;
    if root.version != request.expected_version {
        return Err("VERSION_CONFLICT: 事项已被其他窗口或设备更新".to_string());
    }
    let all = list_tasks(
        database,
        TaskListRequest {
            subject_id: Some(root.subject_id.clone()),
            include_completed: Some(true),
            planned_date: None,
            today_date: None,
            today_only: Some(false),
            daily_estimate_date: None,
            query: None,
        },
    )?
    .tasks;
    let subtree_ids = collect_subtree_ids(&root.id, &all);
    let source = all
        .into_iter()
        .filter(|task| subtree_ids.contains(&task.id))
        .collect::<Vec<_>>();
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let now = now_millis();
    let mut id_map = HashMap::new();
    let mut created_ids = Vec::new();
    for task in &source {
        let id = Uuid::now_v7().to_string();
        let parent_id = if task.id == root.id {
            task.parent_id.clone()
        } else {
            task.parent_id
                .as_ref()
                .and_then(|parent_id| id_map.get(parent_id).cloned())
        };
        let title = if task.id == root.id {
            format!("{} - 副本", task.title)
        } else {
            task.title.clone()
        };
        transaction
            .execute(
                "INSERT INTO tasks(
                    id, workspace_id, subject_id, parent_id, title, status, planned_date,
                    estimate_minutes, note, project_name, solution_name, source_type, source_ref,
                    sort_order, completed_at, created_at, updated_at, version
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'manual', ?12, ?13, NULL, ?14, ?14, 1)",
                params![
                    id, workspace_id, task.subject_id, parent_id, title, "open", task.planned_date,
                    task.estimate_minutes, task.note, task.project_name, task.solution_name, task.id,
                    task.sort_order + 1, now
                ],
            )
            .map_err(|error| error.to_string())?;
        id_map.insert(task.id.clone(), id.clone());
        created_ids.push(id);
    }
    bump_revision(&transaction)?;
    transaction.commit().map_err(|error| error.to_string())?;
    let connection = database.open()?;
    let tasks = created_ids
        .iter()
        .map(|id| load_task(&connection, id))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(TaskDuplicateResult {
        root_id: id_map[&root.id].clone(),
        tasks,
    })
}

pub fn preview_plan_import(request: PlanImportPreviewRequest) -> Result<PlanImportPreview, String> {
    Ok(parse_plan_markdown(&request.markdown))
}

pub(crate) fn parse_plan_markdown(markdown: &str) -> PlanImportPreview {
    let mut items = Vec::new();
    let mut warnings = Vec::new();
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut root_order = 0_i64;
    let mut child_orders: HashMap<String, i64> = HashMap::new();
    let mut in_tomorrow_section = false;

    for (line_index, raw_line) in markdown.lines().enumerate() {
        let trimmed = raw_line.trim();
        let is_heading = trimmed.starts_with('#');
        if is_heading {
            let heading = trimmed.trim_start_matches('#').trim();
            in_tomorrow_section = heading.contains("明日计划")
                || heading.contains("下周")
                || heading.contains("下一工作日");
            continue;
        }
        if !in_tomorrow_section || trimmed.is_empty() {
            continue;
        }
        let indent = raw_line
            .chars()
            .take_while(|character| character.is_whitespace())
            .count();
        let Some(title) = strip_list_marker(trimmed) else {
            warnings.push(format!("第 {} 行未识别为事项：{}", line_index + 1, trimmed));
            items.push(PlanImportItemDto {
                client_id: format!("import-{}", items.len() + 1),
                parent_client_id: None,
                title: trimmed.to_string(),
                estimate_minutes: None,
                sort_order: root_order + 10,
                source_line_no: line_index + 1,
                source_text: raw_line.to_string(),
                parse_status: "unrecognized".to_string(),
            });
            continue;
        };
        let (title, estimate_minutes) = parse_estimate(title);
        let client_id = format!("import-{}", items.len() + 1);
        while stack
            .last()
            .is_some_and(|(previous_indent, _)| *previous_indent >= indent)
        {
            stack.pop();
        }
        let parent_client_id = stack.last().map(|(_, id)| id.clone());
        let sort_order = if let Some(parent) = &parent_client_id {
            let order = child_orders.entry(parent.clone()).or_insert(0);
            *order += 10;
            *order
        } else {
            root_order += 10;
            root_order
        };
        items.push(PlanImportItemDto {
            client_id: client_id.clone(),
            parent_client_id,
            title,
            estimate_minutes,
            sort_order,
            source_line_no: line_index + 1,
            source_text: raw_line.to_string(),
            parse_status: "recognized".to_string(),
        });
        stack.push((indent, client_id));
    }
    if in_tomorrow_section && items.is_empty() {
        warnings.push("找到明日计划标题，但没有识别到事项".to_string());
    }
    PlanImportPreview {
        source_markdown: markdown.to_string(),
        items,
        warnings,
    }
}

fn deserialize_double_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Some(Option::<T>::deserialize(deserializer)?))
}

fn collect_subtree_ids(root_id: &str, tasks: &[TaskDto]) -> std::collections::HashSet<String> {
    let mut ids = std::collections::HashSet::from([root_id.to_string()]);
    let mut changed = true;
    while changed {
        changed = false;
        for task in tasks {
            if task
                .parent_id
                .as_ref()
                .is_some_and(|parent_id| ids.contains(parent_id))
                && ids.insert(task.id.clone())
            {
                changed = true;
            }
        }
    }
    ids
}

fn strip_list_marker(value: &str) -> Option<String> {
    let value = value.trim_start();
    if let Some((number, title)) = value.split_once('.') {
        if !number.is_empty() && number.chars().all(|character| character.is_ascii_digit()) {
            return non_empty_title(title);
        }
    }
    for marker in ["- ", "* ", "+ "] {
        if let Some(title) = value.strip_prefix(marker) {
            return non_empty_title(title);
        }
    }
    None
}

fn non_empty_title(value: &str) -> Option<String> {
    let title = value.trim().to_string();
    (!title.is_empty()).then_some(title)
}

fn parse_estimate(title: String) -> (String, Option<i64>) {
    let Some(start) = title.rfind('<') else {
        return (title, None);
    };
    let Some(end) = title[start..].find('>') else {
        return (title, None);
    };
    let end = start + end;
    let metadata = &title[start + 1..end];
    let normalized = metadata.replace('：', ":");
    let Some(value) = normalized.strip_prefix("预计:").map(str::trim) else {
        return (title, None);
    };
    let minutes = if let Some(hours) = value.strip_suffix('h') {
        hours
            .trim()
            .parse::<f64>()
            .ok()
            .map(|hours| (hours * 60.0).round() as i64)
    } else if let Some(minutes) = value.strip_suffix("min") {
        minutes.trim().parse::<i64>().ok()
    } else {
        None
    };
    let clean_title = title[..start].trim_end().to_string();
    (clean_title, minutes.filter(|minutes| *minutes > 0))
}

fn task_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskDto> {
    Ok(TaskDto {
        id: row.get(0)?,
        subject_id: row.get(1)?,
        parent_id: row.get(2)?,
        title: row.get(3)?,
        status: row.get(4)?,
        planned_date: row.get(5)?,
        estimate_minutes: row.get(6)?,
        today_estimate_minutes: None,
        note: row.get(7)?,
        project_name: row.get(8)?,
        solution_name: row.get(9)?,
        sort_order: row.get(10)?,
        depth: 0,
        path_label: String::new(),
        selectable: true,
        direct_minutes: 0,
        total_minutes: 0,
        execution_state: "not_started".to_string(),
        recurrence: None,
        occurrence_date: None,
        occurrence_origin: None,
        occurrence_version: None,
        version: row.get(11)?,
    })
}

fn apply_recurrence_context(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    context_date: Option<&str>,
    tasks: &mut [TaskDto],
) -> Result<(), String> {
    if let Some(date) = context_date {
        validate_date(date)?;
    }
    for task in tasks {
        let active_rule = crate::recurring::get_active_rule(connection, workspace_id, &task.id)?;
        let dated_rule = context_date
            .map(|date| {
                crate::recurring::get_rule_for_date(connection, workspace_id, &task.id, date)
            })
            .transpose()?
            .flatten();
        task.recurrence = active_rule.or_else(|| dated_rule.clone());
        let Some(date) = context_date else { continue };
        let occurrence =
            crate::recurring::occurrence_for_date(connection, workspace_id, &task.id, date)?;
        let scheduled = dated_rule
            .as_ref()
            .map(|rule| crate::recurring::rule_matches_date(rule, date))
            .transpose()?
            .unwrap_or(false);
        if !scheduled && occurrence.is_none() {
            continue;
        }
        let occurrence = occurrence.unwrap_or(TaskOccurrenceDto {
            task_id: task.id.clone(),
            occurrence_date: date.to_string(),
            origin: "scheduled".to_string(),
            status: "open".to_string(),
            completed_at: None,
            version: 0,
        });
        task.status = occurrence.status.clone();
        task.occurrence_date = Some(occurrence.occurrence_date);
        task.occurrence_origin = Some(occurrence.origin);
        task.occurrence_version = Some(occurrence.version);
    }
    Ok(())
}

fn apply_daily_estimates(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    work_date: &str,
    tasks: &mut [TaskDto],
) -> Result<(), String> {
    validate_date(work_date)?;
    let mut statement = connection
        .prepare(
            "SELECT task_id, estimate_minutes FROM task_daily_estimates
             WHERE workspace_id = ?1 AND work_date = ?2",
        )
        .map_err(|error| error.to_string())?;
    let estimates = statement
        .query_map(params![workspace_id, work_date], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<HashMap<_, _>, _>>()
        .map_err(|error| error.to_string())?;
    for task in tasks {
        task.today_estimate_minutes = estimates.get(&task.id).copied();
    }
    Ok(())
}

fn enrich_and_order_tasks(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    context_date: Option<&str>,
    tasks: &mut Vec<TaskDto>,
) -> Result<(), String> {
    let mut direct_minutes = HashMap::<String, i64>::new();
    let mut statement = connection
        .prepare(
            "SELECT a.task_id, COALESCE(SUM(a.minutes), 0)
             FROM time_allocations a
             JOIN tasks t ON t.id = a.task_id
             JOIN time_entries e ON e.id = a.entry_id
             WHERE a.workspace_id = ?1 AND t.deleted_at IS NULL
               AND (?2 IS NULL OR e.work_date = ?2)
             GROUP BY a.task_id",
        )
        .map_err(|error| error.to_string())?;
    for row in statement
        .query_map(params![workspace_id, context_date], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|error| error.to_string())?
    {
        let (task_id, minutes) = row.map_err(|error| error.to_string())?;
        direct_minutes.insert(task_id, minutes);
    }

    let active_states = connection
        .prepare(
            "SELECT default_task_id, state FROM time_entries
             WHERE workspace_id = ?1 AND default_task_id IS NOT NULL
               AND state IN ('running', 'paused') AND deleted_at IS NULL",
        )
        .and_then(|mut statement| {
            statement
                .query_map([workspace_id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<Result<HashMap<_, _>, _>>()
        })
        .map_err(|error| error.to_string())?;

    let mut children = HashMap::<Option<String>, Vec<String>>::new();
    let by_id = tasks
        .iter()
        .map(|task| (task.id.clone(), task.clone()))
        .collect::<HashMap<_, _>>();
    for task in tasks.iter() {
        let parent = task
            .parent_id
            .as_ref()
            .filter(|parent_id| by_id.contains_key(*parent_id))
            .cloned();
        children.entry(parent).or_default().push(task.id.clone());
    }
    for task_ids in children.values_mut() {
        task_ids.sort_by_key(|id| {
            by_id
                .get(id)
                .map(|task| (task.sort_order, task.id.clone()))
                .unwrap_or_default()
        });
    }

    let total_minutes = tasks
        .iter()
        .map(|task| {
            (
                task.id.clone(),
                subtree_minutes(&task.id, &children, &direct_minutes),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut ordered_ids = Vec::new();
    flatten_tree(None, &children, &mut ordered_ids);
    let mut enriched = Vec::with_capacity(tasks.len());
    for id in ordered_ids {
        let Some(mut task) = by_id.get(&id).cloned() else {
            continue;
        };
        let path = task_path(&task.id, &by_id);
        let has_children = children
            .get(&Some(task.id.clone()))
            .is_some_and(|items| !items.is_empty());
        task.depth = path.len().saturating_sub(1);
        task.path_label = path.join(" / ");
        task.selectable = !has_children;
        task.direct_minutes = *direct_minutes.get(&task.id).unwrap_or(&0);
        task.total_minutes = *total_minutes.get(&task.id).unwrap_or(&task.direct_minutes);
        task.execution_state = if task.status == "done" {
            "done".to_string()
        } else if let Some(state) = active_states.get(&task.id) {
            state.clone()
        } else if task.total_minutes > 0 {
            "started".to_string()
        } else {
            "not_started".to_string()
        };
        enriched.push(task);
    }
    *tasks = enriched;
    Ok(())
}

fn filter_today_tasks(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    subject_id: Option<&str>,
    today_date: &str,
    tasks: &mut Vec<TaskDto>,
) -> Result<(), String> {
    let mut candidate_ids = HashSet::new();
    for task in tasks.iter() {
        if task.occurrence_date.as_deref() == Some(today_date) {
            candidate_ids.insert(task.id.clone());
        }
    }
    let mut statement = connection
        .prepare(
            "SELECT t.id
             FROM tasks t
             WHERE t.workspace_id = ?1 AND t.deleted_at IS NULL
               AND (?2 IS NULL OR t.subject_id = ?2)
               AND (
                 (NOT EXISTS (
                    SELECT 1 FROM task_recurrence_rules rr
                    WHERE rr.task_id = t.id AND rr.effective_end IS NULL
                  ) AND t.status = 'open' AND t.planned_date IS NOT NULL AND t.planned_date <= ?3)
                 OR EXISTS (
                   SELECT 1 FROM time_allocations a
                   JOIN time_entries e ON e.id = a.entry_id
                   WHERE a.task_id = t.id AND e.workspace_id = ?1
                     AND e.work_date = ?3 AND e.deleted_at IS NULL
                 )
                 OR (
                   t.source_type = 'timer_quick_create'
                   AND date(t.created_at / 1000, 'unixepoch', 'localtime') = ?3
                 )
               )",
        )
        .map_err(|error| error.to_string())?;
    for id in statement
        .query_map(params![workspace_id, subject_id, today_date], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|error| error.to_string())?
    {
        candidate_ids.insert(id.map_err(|error| error.to_string())?);
    }

    let by_id = tasks
        .iter()
        .map(|task| (task.id.clone(), task))
        .collect::<HashMap<_, _>>();
    let mut visible_ids = candidate_ids.clone();
    for candidate_id in candidate_ids {
        let mut parent_id = by_id
            .get(&candidate_id)
            .and_then(|task| task.parent_id.clone());
        while let Some(id) = parent_id {
            if !visible_ids.insert(id.clone()) {
                break;
            }
            parent_id = by_id.get(&id).and_then(|task| task.parent_id.clone());
        }
    }
    tasks.retain(|task| visible_ids.contains(&task.id));
    Ok(())
}

fn flatten_tree(
    parent_id: Option<String>,
    children: &HashMap<Option<String>, Vec<String>>,
    output: &mut Vec<String>,
) {
    for id in children.get(&parent_id).into_iter().flatten() {
        output.push(id.clone());
        flatten_tree(Some(id.clone()), children, output);
    }
}

fn subtree_minutes(
    task_id: &str,
    children: &HashMap<Option<String>, Vec<String>>,
    direct_minutes: &HashMap<String, i64>,
) -> i64 {
    let descendants = children.get(&Some(task_id.to_string()));
    if descendants.is_none_or(Vec::is_empty) {
        return *direct_minutes.get(task_id).unwrap_or(&0);
    }
    descendants
        .into_iter()
        .flatten()
        .map(|child_id| subtree_minutes(child_id, children, direct_minutes))
        .sum()
}

fn task_path(task_id: &str, tasks: &HashMap<String, TaskDto>) -> Vec<String> {
    let mut path = Vec::new();
    let mut current_id = Some(task_id.to_string());
    let mut visited = std::collections::HashSet::new();
    while let Some(id) = current_id {
        if !visited.insert(id.clone()) {
            break;
        }
        let Some(task) = tasks.get(&id) else { break };
        path.push(task.title.clone());
        current_id = task.parent_id.clone();
    }
    path.reverse();
    path
}

fn load_task(connection: &rusqlite::Connection, id: &str) -> Result<TaskDto, String> {
    connection
        .query_row(
            "SELECT id, subject_id, parent_id, title, status, planned_date,
                    estimate_minutes, note, project_name, solution_name, sort_order, version
             FROM tasks WHERE id = ?1 AND deleted_at IS NULL",
            [id],
            task_from_row,
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "NOT_FOUND: 事项不存在".to_string())
}

fn ensure_subject(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    subject_id: &str,
) -> Result<(), String> {
    let exists: Option<i64> = connection
        .query_row(
            "SELECT 1 FROM subjects WHERE id = ?1 AND workspace_id = ?2 AND deleted_at IS NULL",
            params![subject_id, workspace_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    exists
        .map(|_| ())
        .ok_or_else(|| "NOT_FOUND: 主体不存在".to_string())
}

fn ensure_parent(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    parent_id: &str,
    subject_id: &str,
) -> Result<(), String> {
    let parent_subject: Option<String> = connection
        .query_row("SELECT subject_id FROM tasks WHERE id = ?1 AND workspace_id = ?2 AND deleted_at IS NULL", params![parent_id, workspace_id], |row| row.get(0))
        .optional()
        .map_err(|error| error.to_string())?;
    match parent_subject {
        Some(value) if value == subject_id => Ok(()),
        Some(_) => Err("VALIDATION_ERROR: 父事项必须属于同一主体".to_string()),
        None => Err("NOT_FOUND: 父事项不存在".to_string()),
    }
}

fn ensure_no_direct_time(connection: &rusqlite::Connection, task_id: &str) -> Result<(), String> {
    let direct_minutes: i64 = connection
        .query_row(
            "SELECT COALESCE(SUM(minutes), 0) FROM time_allocations WHERE task_id = ?1",
            [task_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if direct_minutes > 0 {
        Err("TASK_HAS_DIRECT_TIME: 有直接工时的事项不能成为父事项".to_string())
    } else {
        Ok(())
    }
}

fn ensure_not_recurring(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    task_id: &str,
) -> Result<(), String> {
    if crate::recurring::task_has_active_rule(connection, workspace_id, task_id)? {
        Err("TASK_RECURRING: 重复事项新增子项前必须先关闭重复".to_string())
    } else {
        Ok(())
    }
}

fn is_descendant(
    connection: &rusqlite::Connection,
    ancestor_id: &str,
    candidate_id: &str,
) -> Result<bool, String> {
    let mut current = Some(candidate_id.to_string());
    for _ in 0..128 {
        let Some(id) = current else { return Ok(false) };
        if id == ancestor_id {
            return Ok(true);
        }
        current = connection
            .query_row(
                "SELECT parent_id FROM tasks WHERE id = ?1 AND deleted_at IS NULL",
                [id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?
            .flatten();
    }
    Err("TASK_TREE_CYCLE: 事项层级过深或存在循环".to_string())
}

fn next_sort_order(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    subject_id: &str,
    parent_id: Option<&str>,
) -> Result<i64, String> {
    connection
        .query_row(
            "SELECT COALESCE(MAX(sort_order), 0) + 10 FROM tasks
             WHERE workspace_id = ?1 AND subject_id = ?2 AND deleted_at IS NULL
               AND ((parent_id = ?3) OR (parent_id IS NULL AND ?3 IS NULL))",
            params![workspace_id, subject_id, parent_id],
            |row| row.get(0),
        )
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

fn bump_revision(connection: &rusqlite::Connection) -> Result<(), String> {
    connection
        .execute("UPDATE app_metadata SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT) WHERE key = 'global_revision'", [])
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn validate_title(title: &str) -> Result<(), String> {
    if title.trim().is_empty() {
        Err("VALIDATION_ERROR: 事项名称不能为空".to_string())
    } else {
        Ok(())
    }
}

fn validate_estimate(estimate: Option<i64>) -> Result<(), String> {
    if estimate.is_some_and(|minutes| minutes <= 0) {
        Err("VALIDATION_ERROR: 预计用时必须大于 0 分钟".to_string())
    } else {
        Ok(())
    }
}

fn validate_date(value: &str) -> Result<(), String> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map(|_| ())
        .map_err(|_| "VALIDATION_ERROR: 日期格式必须为 YYYY-MM-DD".to_string())
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[tauri::command]
pub fn task_list(
    database: tauri::State<'_, Database>,
    request: TaskListRequest,
) -> Result<TaskListResult, String> {
    list_tasks(&database, request)
}

#[tauri::command]
pub fn task_daily_estimate_set(
    database: tauri::State<'_, Database>,
    request: TaskDailyEstimateSetRequest,
) -> Result<TaskDailyEstimateDto, String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    set_task_daily_estimate(&database, request)
}

#[tauri::command]
pub fn task_create(
    database: tauri::State<'_, Database>,
    request: TaskCreateRequest,
) -> Result<TaskDto, String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let result = create_task(&database, request)?;
    crate::cloud_sync::enqueue_entity(
        &database,
        "task_create",
        "task",
        Some(&result.id),
        None,
        None,
    )?;
    Ok(result)
}

#[tauri::command]
pub fn task_create_quick(
    database: tauri::State<'_, Database>,
    request: TaskCreateRequest,
) -> Result<TaskDto, String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let result = create_quick_task(&database, request)?;
    crate::cloud_sync::enqueue_entity(
        &database,
        "task_create_quick",
        "task",
        Some(&result.id),
        None,
        None,
    )?;
    Ok(result)
}

#[tauri::command]
pub fn task_update(
    database: tauri::State<'_, Database>,
    request: TaskUpdateRequest,
) -> Result<TaskDto, String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let base_version = request.expected_version;
    let result = update_task(&database, request)?;
    crate::cloud_sync::enqueue_entity(
        &database,
        "task_update",
        "task",
        Some(&result.id),
        Some(base_version),
        None,
    )?;
    Ok(result)
}

#[tauri::command]
pub fn task_set_completed(
    database: tauri::State<'_, Database>,
    request: TaskStatusRequest,
) -> Result<TaskDto, String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let should_publish = request.done
        && !task_is_completed(&database, &request.id, request.occurrence_date.as_deref())?;
    let task_id = request.id.clone();
    let occurrence_date = request.occurrence_date.clone();
    let base_version = request.expected_version;
    let result = set_task_status(&database, request)?;
    crate::cloud_sync::enqueue_entity(
        &database,
        "task_set_completed",
        "task",
        Some(&result.id),
        Some(base_version),
        None,
    )?;
    if should_publish {
        publish_task_completed_hook(&database, &task_id, occurrence_date.as_deref(), None);
    }
    Ok(result)
}

#[tauri::command]
pub fn task_delete_subtree(
    database: tauri::State<'_, Database>,
    request: TaskDeleteRequest,
) -> Result<(), String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let subtree_versions = delete_task(&database, request)?;
    enqueue_task_subtree(&database, &subtree_versions, "task_delete")
}

#[tauri::command]
pub fn task_reorder_subtree(
    database: tauri::State<'_, Database>,
    request: TaskReorderRequest,
) -> Result<TaskDto, String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let base_version = request.expected_version;
    let result = reorder_task(&database, request)?;
    crate::cloud_sync::enqueue_entity(
        &database,
        "task_reorder",
        "task",
        Some(&result.id),
        Some(base_version),
        None,
    )?;
    Ok(result)
}

#[tauri::command]
pub fn task_change_parent(
    database: tauri::State<'_, Database>,
    request: TaskChangeParentRequest,
) -> Result<TaskDto, String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let base_version = request.expected_version;
    let result = change_task_parent(&database, request)?;
    crate::cloud_sync::enqueue_entity(
        &database,
        "task_change_parent",
        "task",
        Some(&result.id),
        Some(base_version),
        None,
    )?;
    Ok(result)
}

#[tauri::command]
pub fn task_duplicate_subtree(
    database: tauri::State<'_, Database>,
    request: TaskDuplicateRequest,
) -> Result<TaskDuplicateResult, String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let result = duplicate_task_subtree(&database, request)?;
    for task in &result.tasks {
        crate::cloud_sync::enqueue_entity_deferred(
            &database,
            "task_duplicate",
            "task",
            Some(&task.id),
            None,
            None,
        )?;
    }
    crate::cloud_sync::flush_if_online(&database)?;
    Ok(result)
}

fn enqueue_task_subtree(
    database: &Database,
    subtree_versions: &[(String, i64)],
    operation_type: &str,
) -> Result<(), String> {
    for (id, base_version) in subtree_versions {
        crate::cloud_sync::enqueue_entity_deferred(
            database,
            operation_type,
            "task",
            Some(id),
            Some(*base_version),
            None,
        )?;
    }
    crate::cloud_sync::flush_if_online(database)
}

#[tauri::command]
pub fn plan_import_preview(request: PlanImportPreviewRequest) -> Result<PlanImportPreview, String> {
    preview_plan_import(request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use tempfile::tempdir;

    fn database() -> Database {
        let directory = tempdir().unwrap();
        let path = directory.path().join("tasks.sqlite3");
        let database = Database::initialize_at(path).unwrap();
        std::mem::forget(directory);
        database
    }

    #[test]
    fn parses_nested_tomorrow_plan_and_estimates() {
        let preview = parse_plan_markdown("## 明日计划：\n\n1. 运维相关\n   - 处理异常 <预计：0.5h>\n   - 整理文档 <预计: 20min>\n");
        assert_eq!(preview.items.len(), 3);
        assert_eq!(
            preview.items[1].parent_client_id.as_deref(),
            Some("import-1")
        );
        assert_eq!(preview.items[1].estimate_minutes, Some(30));
        assert_eq!(preview.items[2].estimate_minutes, Some(20));
    }

    #[test]
    fn task_crud_checks_version_and_status_history() {
        let database = database();
        let connection = database.open().unwrap();
        let subject_id: String = connection
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let task = create_task(
            &database,
            TaskCreateRequest {
                subject_id,
                parent_id: None,
                title: "测试事项".to_string(),
                planned_date: Some("2026-09-24".to_string()),
                estimate_minutes: Some(30),
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        let scheduled = update_task(
            &database,
            TaskUpdateRequest {
                id: task.id.clone(),
                expected_version: task.version,
                title: None,
                parent_id: None,
                planned_date: Some(Some("2026-09-28".to_string())),
                estimate_minutes: Some(Some(90)),
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        assert_eq!(scheduled.planned_date.as_deref(), Some("2026-09-28"));
        assert_eq!(scheduled.estimate_minutes, Some(90));
        let cleared = update_task(
            &database,
            TaskUpdateRequest {
                id: task.id.clone(),
                expected_version: scheduled.version,
                title: None,
                parent_id: None,
                planned_date: Some(None),
                estimate_minutes: Some(None),
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        assert_eq!(cleared.planned_date, None);
        assert_eq!(cleared.estimate_minutes, None);
        let done = set_task_status(
            &database,
            TaskStatusRequest {
                id: task.id.clone(),
                expected_version: cleared.version,
                done: true,
                occurrence_date: None,
                occurrence_expected_version: None,
            },
        )
        .unwrap();
        assert_eq!(done.status, "done");
        assert_eq!(done.version, cleared.version + 1);
        let status_events: i64 = database
            .open()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM task_status_events WHERE task_id = ?1",
                [&task.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status_events, 1);
        assert!(set_task_status(
            &database,
            TaskStatusRequest {
                id: task.id,
                expected_version: cleared.version,
                done: false,
                occurrence_date: None,
                occurrence_expected_version: None,
            }
        )
        .is_err());
    }

    #[test]
    fn daily_estimate_is_scoped_by_date_and_does_not_change_overall_estimate() {
        let database = database();
        let connection = database.open().unwrap();
        let subject_id: String = connection
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        drop(connection);
        let task = create_task(
            &database,
            TaskCreateRequest {
                subject_id: subject_id.clone(),
                parent_id: None,
                title: "按日预计事项".to_string(),
                planned_date: Some("2026-09-29".to_string()),
                estimate_minutes: Some(240),
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();

        set_task_daily_estimate(
            &database,
            TaskDailyEstimateSetRequest {
                task_id: task.id.clone(),
                work_date: "2026-09-29".to_string(),
                estimate_minutes: Some(60),
            },
        )
        .unwrap();
        let today = list_tasks(
            &database,
            TaskListRequest {
                subject_id: Some(subject_id.clone()),
                include_completed: Some(true),
                planned_date: None,
                today_date: None,
                today_only: Some(false),
                daily_estimate_date: Some("2026-09-29".to_string()),
                query: None,
            },
        )
        .unwrap();
        let today_task = today.tasks.iter().find(|item| item.id == task.id).unwrap();
        assert_eq!(today_task.today_estimate_minutes, Some(60));
        assert_eq!(today_task.estimate_minutes, Some(240));

        let next_day = list_tasks(
            &database,
            TaskListRequest {
                subject_id: Some(subject_id),
                include_completed: Some(true),
                planned_date: None,
                today_date: None,
                today_only: Some(false),
                daily_estimate_date: Some("2026-09-30".to_string()),
                query: None,
            },
        )
        .unwrap();
        assert_eq!(
            next_day
                .tasks
                .iter()
                .find(|item| item.id == task.id)
                .unwrap()
                .today_estimate_minutes,
            None
        );

        set_task_daily_estimate(
            &database,
            TaskDailyEstimateSetRequest {
                task_id: task.id.clone(),
                work_date: "2026-09-29".to_string(),
                estimate_minutes: None,
            },
        )
        .unwrap();
        let count: i64 = database
            .open()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM task_daily_estimates WHERE task_id = ?1",
                [&task.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn tree_operations_preserve_hierarchy_and_delete_full_subtree() {
        let database = database();
        let connection = database.open().unwrap();
        let subject_id: String = connection
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        drop(connection);
        let root = create_test_task(&database, &subject_id, None, "根事项");
        let child = create_test_task(&database, &subject_id, Some(root.id.clone()), "子事项");
        let grandchild = create_test_task(&database, &subject_id, Some(child.id.clone()), "孙事项");

        let duplicate = duplicate_task_subtree(
            &database,
            TaskDuplicateRequest {
                id: root.id.clone(),
                expected_version: root.version,
            },
        )
        .unwrap();
        assert_eq!(duplicate.tasks.len(), 3);
        assert_eq!(duplicate.tasks[0].title, "根事项 - 副本");

        assert!(reorder_task(
            &database,
            TaskReorderRequest {
                id: child.id.clone(),
                expected_version: child.version,
                parent_id: None,
                sort_order: 10,
            }
        )
        .is_err());

        let updated_child = update_task(
            &database,
            TaskUpdateRequest {
                id: child.id.clone(),
                expected_version: child.version,
                title: Some("更新后的子事项".to_string()),
                parent_id: None,
                planned_date: None,
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        let deleted_versions = delete_task(
            &database,
            TaskDeleteRequest {
                id: root.id.clone(),
                expected_version: root.version,
            },
        )
        .unwrap();
        let deleted_versions = deleted_versions.into_iter().collect::<HashMap<_, _>>();
        assert_eq!(deleted_versions.get(&root.id), Some(&root.version));
        assert_eq!(
            deleted_versions.get(&child.id),
            Some(&updated_child.version)
        );
        assert_eq!(
            deleted_versions.get(&grandchild.id),
            Some(&grandchild.version)
        );
        let remaining: i64 = database
            .open()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE id IN (?1, ?2, ?3) AND deleted_at IS NULL",
                params![root.id, child.id, grandchild.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(remaining, 0);
    }

    #[test]
    fn task_list_derives_parent_totals_and_selectability() {
        let database = database();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let subject_id: String = connection
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        drop(connection);
        let root = create_test_task(&database, &subject_id, None, "汇总事项");
        let child = create_test_task(&database, &subject_id, Some(root.id.clone()), "叶子事项");
        let connection = database.open().unwrap();
        let now = now_millis();
        let entry_id = Uuid::now_v7().to_string();
        connection
            .execute(
                "INSERT INTO time_entries(id, workspace_id, work_date, kind, source_type, state, label_snapshot, started_at, ended_at, duration_seconds, created_at, updated_at, version)
                 VALUES (?1, ?2, '2026-09-24', 'work', 'manual', 'ended', '测试', ?3, ?3, 1800, ?3, ?3, 1)",
                params![entry_id, workspace_id, now],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO time_allocations(id, workspace_id, entry_id, task_id, minutes, created_at, updated_at, version)
                 VALUES (?1, ?2, ?3, ?4, 30, ?5, ?5, 1)",
                params![Uuid::now_v7().to_string(), workspace_id, entry_id, child.id, now],
            )
            .unwrap();
        drop(connection);

        let result = list_tasks(
            &database,
            TaskListRequest {
                subject_id: Some(subject_id),
                include_completed: Some(true),
                planned_date: None,
                today_date: None,
                today_only: Some(false),
                daily_estimate_date: None,
                query: None,
            },
        )
        .unwrap();
        let root_result = result.tasks.iter().find(|task| task.id == root.id).unwrap();
        let child_result = result
            .tasks
            .iter()
            .find(|task| task.id == child.id)
            .unwrap();
        assert!(!root_result.selectable);
        assert_eq!(root_result.total_minutes, 30);
        assert_eq!(root_result.direct_minutes, 0);
        assert!(child_result.selectable);
        assert_eq!(child_result.direct_minutes, 30);
        assert_eq!(child_result.path_label, "汇总事项 / 叶子事项");
    }

    #[test]
    fn regular_task_list_keeps_undated_new_tasks_when_today_context_is_present() {
        let database = database();
        let connection = database.open().unwrap();
        let subject_id: String = connection
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        drop(connection);

        let root = create_test_task(&database, &subject_id, None, "新建一级事项");
        let child = create_test_task(&database, &subject_id, Some(root.id.clone()), "新建子事项");
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();

        let regular = list_tasks(
            &database,
            TaskListRequest {
                subject_id: Some(subject_id.clone()),
                include_completed: Some(true),
                planned_date: None,
                today_date: Some(today.clone()),
                today_only: Some(false),
                daily_estimate_date: Some(today.clone()),
                query: None,
            },
        )
        .unwrap();
        let regular_ids = regular
            .tasks
            .iter()
            .map(|task| task.id.as_str())
            .collect::<HashSet<_>>();
        assert!(regular_ids.contains(root.id.as_str()));
        assert!(regular_ids.contains(child.id.as_str()));

        let today_only = list_tasks(
            &database,
            TaskListRequest {
                subject_id: Some(subject_id),
                include_completed: Some(true),
                planned_date: None,
                today_date: Some(today.clone()),
                today_only: Some(true),
                daily_estimate_date: Some(today),
                query: None,
            },
        )
        .unwrap();
        let today_ids = today_only
            .tasks
            .iter()
            .map(|task| task.id.as_str())
            .collect::<HashSet<_>>();
        assert!(!today_ids.contains(root.id.as_str()));
        assert!(!today_ids.contains(child.id.as_str()));
    }

    #[test]
    fn today_task_filter_keeps_overdue_and_today_work_but_excludes_future() {
        let database = database();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let subject_id: String = connection
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        drop(connection);

        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let yesterday = (chrono::Local::now() - chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        let tomorrow = (chrono::Local::now() + chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        let overdue = create_task(
            &database,
            TaskCreateRequest {
                subject_id: subject_id.clone(),
                parent_id: None,
                title: "逾期未完成".to_string(),
                planned_date: Some(yesterday),
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        let current = create_task(
            &database,
            TaskCreateRequest {
                subject_id: subject_id.clone(),
                parent_id: None,
                title: "今天开始".to_string(),
                planned_date: Some(today.clone()),
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        let future = create_task(
            &database,
            TaskCreateRequest {
                subject_id: subject_id.clone(),
                parent_id: None,
                title: "未来事项".to_string(),
                planned_date: Some(tomorrow),
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        let actual_only = create_task(
            &database,
            TaskCreateRequest {
                subject_id: subject_id.clone(),
                parent_id: None,
                title: "今天提前执行".to_string(),
                planned_date: None,
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        let ordinary_without_date = create_task(
            &database,
            TaskCreateRequest {
                subject_id: subject_id.clone(),
                parent_id: None,
                title: "无日期普通事项".to_string(),
                planned_date: None,
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        let quick = create_quick_task(
            &database,
            TaskCreateRequest {
                subject_id: subject_id.clone(),
                parent_id: None,
                title: "今天临时事项".to_string(),
                planned_date: None,
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();

        let connection = database.open().unwrap();
        let now = now_millis();
        let entry_id = Uuid::now_v7().to_string();
        connection
            .execute(
                "INSERT INTO time_entries(id, workspace_id, work_date, kind, source_type, state, label_snapshot, started_at, ended_at, duration_seconds, created_at, updated_at, version)
                 VALUES (?1, ?2, ?3, 'work', 'manual', 'ended', '今天提前执行', ?4, ?4, 1800, ?4, ?4, 1)",
                params![entry_id, workspace_id, today, now],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO time_allocations(id, workspace_id, entry_id, task_id, minutes, created_at, updated_at, version)
                 VALUES (?1, ?2, ?3, ?4, 30, ?5, ?5, 1)",
                params![Uuid::now_v7().to_string(), workspace_id, entry_id, actual_only.id, now],
            )
            .unwrap();
        drop(connection);

        let result = list_tasks(
            &database,
            TaskListRequest {
                subject_id: Some(subject_id),
                include_completed: Some(true),
                planned_date: None,
                today_date: Some(chrono::Local::now().format("%Y-%m-%d").to_string()),
                today_only: Some(true),
                daily_estimate_date: Some(chrono::Local::now().format("%Y-%m-%d").to_string()),
                query: None,
            },
        )
        .unwrap();
        let ids = result
            .tasks
            .iter()
            .map(|task| task.id.as_str())
            .collect::<HashSet<_>>();
        assert!(ids.contains(overdue.id.as_str()));
        assert!(ids.contains(current.id.as_str()));
        assert!(ids.contains(actual_only.id.as_str()));
        assert!(ids.contains(quick.id.as_str()));
        assert!(!ids.contains(future.id.as_str()));
        assert!(!ids.contains(ordinary_without_date.id.as_str()));
    }

    fn create_test_task(
        database: &Database,
        subject_id: &str,
        parent_id: Option<String>,
        title: &str,
    ) -> TaskDto {
        create_task(
            database,
            TaskCreateRequest {
                subject_id: subject_id.to_string(),
                parent_id,
                title: title.to_string(),
                planned_date: None,
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap()
    }
}
