use std::collections::{HashMap, HashSet};

use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::database::Database;
use crate::tasks::{self, TaskDto, TaskListRequest};

pub const INCLUDED: &str = "included";
pub const EXCLUDED: &str = "excluded";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDayPlanSnapshotRequest {
    pub subject_id: String,
    pub work_date: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDayPlanSnapshotDto {
    pub subject_id: String,
    pub work_date: String,
    pub tasks: Vec<TaskDto>,
    pub revision: i64,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TaskDayPlanSetRequest {
    pub task_id: String,
    pub work_date: String,
    pub state: String,
    pub expected_version: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDayPlanDto {
    pub task_id: String,
    pub work_date: String,
    pub state: String,
    pub version: i64,
}

pub(crate) struct TaskDayPlanWriteResult {
    pub plan: TaskDayPlanDto,
    pub operation_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDayPlanBatchSetRequest {
    pub subject_id: String,
    pub work_date: String,
    pub items: Vec<TaskDayPlanBatchItem>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TaskDayPlanBatchItem {
    pub task_id: String,
    pub state: String,
    pub expected_version: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDayPlanBatchSetResult {
    pub changed_count: usize,
}

pub fn snapshot(
    database: &Database,
    request: TaskDayPlanSnapshotRequest,
) -> Result<TaskDayPlanSnapshotDto, String> {
    validate_date(&request.work_date)?;
    let mut listed = tasks::list_tasks(
        database,
        TaskListRequest {
            subject_id: Some(request.subject_id.clone()),
            include_completed: Some(true),
            planned_date: None,
            today_date: Some(request.work_date.clone()),
            today_only: Some(false),
            daily_estimate_date: Some(request.work_date.clone()),
            query: None,
        },
    )?;
    let connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    ensure_subject(&connection, &workspace_id, &request.subject_id)?;
    apply_day_plan_context(
        &connection,
        &workspace_id,
        &request.work_date,
        &mut listed.tasks,
        true,
    )?;
    Ok(TaskDayPlanSnapshotDto {
        subject_id: request.subject_id,
        work_date: request.work_date,
        tasks: listed.tasks,
        revision: listed.revision,
    })
}

pub(crate) fn apply_day_plan_context(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    work_date: &str,
    tasks: &mut Vec<TaskDto>,
    filter: bool,
) -> Result<(), String> {
    validate_date(work_date)?;
    let plans = load_plan_rows(connection, workspace_id, work_date)?;
    let estimated = connection
        .prepare("SELECT task_id FROM task_daily_estimates WHERE workspace_id=?1 AND work_date=?2")
        .and_then(|mut statement| {
            statement
                .query_map(params![workspace_id, work_date], |row| {
                    row.get::<_, String>(0)
                })?
                .collect::<Result<HashSet<_>, _>>()
        })
        .map_err(|error| error.to_string())?;

    let mut member_ids = HashSet::new();
    for task in tasks.iter_mut() {
        if let Some((state, version)) = plans.get(&task.id) {
            task.day_plan_state = Some(state.clone());
            task.day_plan_version = Some(*version);
        }
        let explicitly_excluded = task.day_plan_state.as_deref() == Some(EXCLUDED);
        let explicitly_included = task.day_plan_state.as_deref() == Some(INCLUDED);
        let recurring_today = task.occurrence_date.as_deref() == Some(work_date);
        task.today_member = !explicitly_excluded
            && (explicitly_included || estimated.contains(&task.id) || recurring_today);
        task.today_context = false;
        if task.today_member {
            member_ids.insert(task.id.clone());
        }
    }

    let by_id = tasks
        .iter()
        .map(|task| (task.id.clone(), task.parent_id.clone()))
        .collect::<HashMap<_, _>>();
    let mut visible_ids = member_ids.clone();
    for task_id in member_ids {
        let mut parent_id = by_id.get(&task_id).cloned().flatten();
        while let Some(id) = parent_id {
            if !visible_ids.insert(id.clone()) {
                break;
            }
            parent_id = by_id.get(&id).cloned().flatten();
        }
    }
    for task in tasks.iter_mut() {
        task.today_context = visible_ids.contains(&task.id) && !task.today_member;
    }
    if filter {
        tasks.retain(|task| visible_ids.contains(&task.id));
    }
    Ok(())
}

fn load_plan_rows(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    work_date: &str,
) -> Result<HashMap<String, (String, i64)>, String> {
    connection
        .prepare(
            "SELECT task_id,state,version FROM task_day_plans WHERE workspace_id=?1 AND work_date=?2",
        )
        .and_then(|mut statement| {
            statement
                .query_map(params![workspace_id, work_date], |row| {
                    Ok((row.get::<_, String>(0)?, (row.get::<_, String>(1)?, row.get::<_, i64>(2)?)))
                })?
                .collect::<Result<HashMap<_, _>, _>>()
        })
        .map_err(|error| error.to_string())
}

pub(crate) fn write_day_plan_in_transaction(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    task_id: &str,
    work_date: &str,
    state: &str,
    expected_version: Option<i64>,
    now: i64,
    cloud_state: Option<&crate::supabase::StorageModeSnapshot>,
    dependency_operation_id: Option<&str>,
) -> Result<TaskDayPlanWriteResult, String> {
    validate_state(state)?;
    let current: Option<(i64, i64)> = transaction
        .query_row(
            "SELECT version,created_at FROM task_day_plans WHERE workspace_id=?1 AND task_id=?2 AND work_date=?3",
            params![workspace_id, task_id, work_date],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if current.map(|value| value.0) != expected_version {
        return Err("VERSION_CONFLICT: 今日计划已被其他窗口或设备更新".to_string());
    }
    transaction
        .execute(
            "INSERT INTO task_day_plans(workspace_id,task_id,work_date,state,created_at,updated_at,version)
             VALUES(?1,?2,?3,?4,?5,?5,1) ON CONFLICT(task_id,work_date) DO UPDATE SET
             state=excluded.state,updated_at=excluded.updated_at,version=task_day_plans.version+1",
            params![workspace_id, task_id, work_date, state, now],
        )
        .map_err(|error| error.to_string())?;
    let version: i64 = transaction
        .query_row(
            "SELECT version FROM task_day_plans WHERE workspace_id=?1 AND task_id=?2 AND work_date=?3",
            params![workspace_id, task_id, work_date],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let operation_id = if let Some(storage) = cloud_state {
        let entity_id = format!("{task_id}|{work_date}");
        crate::cloud_sync::enqueue_payload_in_transaction(
            transaction,
            storage,
            "task_day_plan_set",
            "task_day_plan",
            &entity_id,
            current.map(|value| value.0),
            &json!({"task_id":task_id,"work_date":work_date,"state":state,"created_at":current.map(|value| value.1).unwrap_or(now),"updated_at":now,"version":version}),
            None,
            dependency_operation_id,
        )?
    } else {
        None
    };
    if state == EXCLUDED {
        crate::tasks::clear_daily_estimate_for_day_plan_in_transaction(
            transaction,
            workspace_id,
            task_id,
            work_date,
            now,
            cloud_state,
            operation_id.as_deref().or(dependency_operation_id),
        )?;
    }
    Ok(TaskDayPlanWriteResult {
        plan: TaskDayPlanDto {
            task_id: task_id.to_string(),
            work_date: work_date.to_string(),
            state: state.to_string(),
            version,
        },
        operation_id,
    })
}

pub(crate) fn current_day_plan_version(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    task_id: &str,
    work_date: &str,
) -> Result<Option<i64>, String> {
    connection
        .query_row(
            "SELECT version FROM task_day_plans WHERE workspace_id=?1 AND task_id=?2 AND work_date=?3",
            params![workspace_id, task_id, work_date],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())
}

pub(crate) fn ensure_included_in_transaction(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    task_id: &str,
    work_date: &str,
    now: i64,
    cloud_state: Option<&crate::supabase::StorageModeSnapshot>,
) -> Result<TaskDayPlanWriteResult, String> {
    let current: Option<(String, i64)> = transaction
        .query_row(
            "SELECT state,version FROM task_day_plans WHERE workspace_id=?1 AND task_id=?2 AND work_date=?3",
            params![workspace_id, task_id, work_date],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if let Some((state, version)) = current.as_ref() {
        if state == INCLUDED {
            return Ok(TaskDayPlanWriteResult {
                plan: TaskDayPlanDto {
                    task_id: task_id.to_string(),
                    work_date: work_date.to_string(),
                    state: state.clone(),
                    version: *version,
                },
                operation_id: None,
            });
        }
    }
    let expected = current.map(|value| value.1);
    write_day_plan_in_transaction(
        transaction,
        workspace_id,
        task_id,
        work_date,
        INCLUDED,
        expected,
        now,
        cloud_state,
        None,
    )
}

fn set_day_plan(
    database: &Database,
    request: TaskDayPlanSetRequest,
    cloud_state: Option<&crate::supabase::StorageModeSnapshot>,
) -> Result<TaskDayPlanDto, String> {
    validate_date(&request.work_date)?;
    validate_state(&request.state)?;
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    ensure_task(&connection, &workspace_id, &request.task_id, None)?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let result = write_day_plan_in_transaction(
        &transaction,
        &workspace_id,
        &request.task_id,
        &request.work_date,
        &request.state,
        request.expected_version,
        now_millis(),
        cloud_state,
        None,
    )?;
    bump_revision(&transaction)?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result.plan)
}

#[cfg(test)]
pub(crate) fn set_day_plan_for_cloud_test(
    database: &Database,
    request: TaskDayPlanSetRequest,
    cloud_state: &crate::supabase::StorageModeSnapshot,
) -> Result<TaskDayPlanDto, String> {
    set_day_plan(database, request, Some(cloud_state))
}

fn set_day_plan_batch(
    database: &Database,
    request: TaskDayPlanBatchSetRequest,
    cloud_state: Option<&crate::supabase::StorageModeSnapshot>,
) -> Result<TaskDayPlanBatchSetResult, String> {
    validate_date(&request.work_date)?;
    let mut seen = HashSet::new();
    for item in &request.items {
        validate_state(&item.state)?;
        if !seen.insert(item.task_id.as_str()) {
            return Err("VALIDATION_ERROR: 批量今日计划不能包含重复事项".to_string());
        }
    }
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    ensure_subject(&connection, &workspace_id, &request.subject_id)?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let now = now_millis();
    for item in &request.items {
        ensure_task(
            &transaction,
            &workspace_id,
            &item.task_id,
            Some(&request.subject_id),
        )?;
        write_day_plan_in_transaction(
            &transaction,
            &workspace_id,
            &item.task_id,
            &request.work_date,
            &item.state,
            item.expected_version,
            now,
            cloud_state,
            None,
        )?;
    }
    bump_revision(&transaction)?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(TaskDayPlanBatchSetResult {
        changed_count: request.items.len(),
    })
}

fn ensure_task(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    task_id: &str,
    subject_id: Option<&str>,
) -> Result<(), String> {
    let actual_subject: Option<String> = connection
        .query_row(
            "SELECT subject_id FROM tasks WHERE workspace_id=?1 AND id=?2 AND deleted_at IS NULL",
            params![workspace_id, task_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    match actual_subject {
        None => Err("NOT_FOUND: 事项不存在".to_string()),
        Some(actual) if subject_id.is_some_and(|expected| expected != actual) => {
            Err("VALIDATION_ERROR: 事项不属于当前主体".to_string())
        }
        Some(_) => Ok(()),
    }
}

fn ensure_subject(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    subject_id: &str,
) -> Result<(), String> {
    connection
        .query_row(
            "SELECT 1 FROM subjects WHERE workspace_id=?1 AND id=?2 AND deleted_at IS NULL",
            params![workspace_id, subject_id],
            |_| Ok(()),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "NOT_FOUND: 主体不存在".to_string())
}

fn workspace_id(connection: &rusqlite::Connection) -> Result<String, String> {
    connection
        .query_row(
            "SELECT id FROM workspaces WHERE deleted_at IS NULL LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())
}

fn validate_state(state: &str) -> Result<(), String> {
    if matches!(state, INCLUDED | EXCLUDED) {
        Ok(())
    } else {
        Err("VALIDATION_ERROR: 今日计划状态无效".to_string())
    }
}

fn validate_date(value: &str) -> Result<(), String> {
    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map(|_| ())
        .map_err(|_| "VALIDATION_ERROR: 日期格式必须为 YYYY-MM-DD".to_string())
}

fn bump_revision(connection: &rusqlite::Connection) -> Result<(), String> {
    connection
        .execute(
            "UPDATE app_metadata SET value=CAST(value AS INTEGER)+1 WHERE key='global_revision'",
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
pub fn task_day_plan_snapshot(
    database: tauri::State<'_, Database>,
    request: TaskDayPlanSnapshotRequest,
) -> Result<TaskDayPlanSnapshotDto, String> {
    snapshot(&database, request)
}

#[tauri::command]
pub fn task_day_plan_set(
    database: tauri::State<'_, Database>,
    request: TaskDayPlanSetRequest,
) -> Result<TaskDayPlanDto, String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let state = crate::supabase::storage_mode(&database)?;
    let result = set_day_plan(&database, request, Some(&state))?;
    crate::cloud_sync::flush_if_online(&database)?;
    Ok(result)
}

#[tauri::command]
pub fn task_day_plan_batch_set(
    database: tauri::State<'_, Database>,
    request: TaskDayPlanBatchSetRequest,
) -> Result<TaskDayPlanBatchSetResult, String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let state = crate::supabase::storage_mode(&database)?;
    let result = set_day_plan_batch(&database, request, Some(&state))?;
    crate::cloud_sync::flush_if_online(&database)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subjects::{create_subject, SubjectCreateRequest};
    use crate::tasks::{TaskCreateRequest, TaskDailyEstimateSetRequest};
    use rusqlite::params;
    use tempfile::tempdir;
    use uuid::Uuid;

    fn database() -> Database {
        let directory = tempdir().unwrap();
        let path = directory.path().join("today-plans.sqlite3");
        let database = Database::initialize_at(path).unwrap();
        std::mem::forget(directory);
        database
    }

    fn subject_id(database: &Database) -> String {
        database
            .open()
            .unwrap()
            .query_row(
                "SELECT id FROM subjects ORDER BY sort_order LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn create_task(database: &Database, title: &str, parent_id: Option<String>) -> TaskDto {
        crate::tasks::create_task(
            database,
            TaskCreateRequest {
                subject_id: subject_id(database),
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

    fn plan_set(
        database: &Database,
        task_id: &str,
        work_date: &str,
        state: &str,
        expected_version: Option<i64>,
    ) -> Result<TaskDayPlanDto, String> {
        set_day_plan(
            database,
            TaskDayPlanSetRequest {
                task_id: task_id.to_string(),
                work_date: work_date.to_string(),
                state: state.to_string(),
                expected_version,
            },
            None,
        )
    }

    #[test]
    fn snapshot_uses_explicit_membership_and_parent_context() {
        let database = database();
        let date = "2026-10-19";
        let parent = create_task(&database, "父事项", None);
        let child = create_task(&database, "今日子事项", Some(parent.id.clone()));
        plan_set(&database, &child.id, date, INCLUDED, None).unwrap();

        let result = snapshot(
            &database,
            TaskDayPlanSnapshotRequest {
                subject_id: subject_id(&database),
                work_date: date.to_string(),
            },
        )
        .unwrap();
        assert_eq!(result.tasks.len(), 2);
        let parent_row = result
            .tasks
            .iter()
            .find(|task| task.id == parent.id)
            .unwrap();
        assert!(!parent_row.today_member);
        assert!(parent_row.today_context);
        let child_row = result
            .tasks
            .iter()
            .find(|task| task.id == child.id)
            .unwrap();
        assert!(child_row.today_member);
        assert!(!child_row.today_context);
    }

    #[test]
    fn activity_alone_does_not_join_today_plan() {
        let database = database();
        let date = "2026-10-19";
        let task = create_task(&database, "仅有实际时间", None);
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let entry_id = Uuid::now_v7().to_string();
        connection.execute(
            "INSERT INTO time_entries(id,workspace_id,work_date,kind,source_type,state,default_task_id,label_snapshot,started_at,ended_at,duration_seconds,created_at,updated_at,version) VALUES(?1,?2,?3,'work','manual','ended',?4,?5,1000,2000,60,1000,2000,1)",
            params![entry_id, workspace_id, date, task.id, task.title],
        ).unwrap();
        connection.execute(
            "INSERT INTO time_allocations(id,workspace_id,entry_id,task_id,minutes,created_at,updated_at,version) VALUES(?1,?2,?3,?4,15,1000,1000,1)",
            params![Uuid::now_v7().to_string(), workspace_id, entry_id, task.id],
        ).unwrap();
        drop(connection);

        let result = snapshot(
            &database,
            TaskDayPlanSnapshotRequest {
                subject_id: subject_id(&database),
                work_date: date.to_string(),
            },
        )
        .unwrap();
        assert!(result.tasks.iter().all(|row| row.id != task.id));
    }

    #[test]
    fn explicit_exclusion_overrides_recurring_rule_for_only_one_date() {
        let database = database();
        let first = "2026-10-19";
        let second = "2026-10-20";
        let task = create_task(&database, "每日重复", None);
        crate::recurring::save_rule(
            &database,
            crate::recurring::RecurrenceSaveRequest {
                task_id: task.id.clone(),
                task_expected_version: task.version,
                frequency: "daily".to_string(),
                weekdays_mask: None,
                effective_start: first.to_string(),
                rule_expected_version: None,
            },
        )
        .unwrap();
        plan_set(&database, &task.id, first, EXCLUDED, None).unwrap();

        let first_result = snapshot(
            &database,
            TaskDayPlanSnapshotRequest {
                subject_id: subject_id(&database),
                work_date: first.to_string(),
            },
        )
        .unwrap();
        assert!(first_result.tasks.iter().all(|row| row.id != task.id));
        let second_result = snapshot(
            &database,
            TaskDayPlanSnapshotRequest {
                subject_id: subject_id(&database),
                work_date: second.to_string(),
            },
        )
        .unwrap();
        assert!(second_result
            .tasks
            .iter()
            .any(|row| row.id == task.id && row.today_member));
    }

    #[test]
    fn estimate_adds_membership_clear_keeps_it_and_remove_clears_estimate() {
        let database = database();
        let date = "2026-10-19";
        let task = create_task(&database, "估时事项", None);
        crate::tasks::set_task_daily_estimate(
            &database,
            TaskDailyEstimateSetRequest {
                task_id: task.id.clone(),
                work_date: date.to_string(),
                estimate_minutes: Some(30),
            },
        )
        .unwrap();
        crate::tasks::set_task_daily_estimate(
            &database,
            TaskDailyEstimateSetRequest {
                task_id: task.id.clone(),
                work_date: date.to_string(),
                estimate_minutes: None,
            },
        )
        .unwrap();
        let kept = snapshot(
            &database,
            TaskDayPlanSnapshotRequest {
                subject_id: subject_id(&database),
                work_date: date.to_string(),
            },
        )
        .unwrap();
        let kept_task = kept.tasks.iter().find(|row| row.id == task.id).unwrap();
        assert!(kept_task.today_member);
        assert_eq!(kept_task.today_estimate_minutes, None);

        plan_set(
            &database,
            &task.id,
            date,
            EXCLUDED,
            kept_task.day_plan_version,
        )
        .unwrap();
        let connection = database.open().unwrap();
        let estimate_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM task_daily_estimates WHERE task_id=?1 AND work_date=?2",
                params![task.id, date],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(estimate_count, 0);
        drop(connection);
        let removed = snapshot(
            &database,
            TaskDayPlanSnapshotRequest {
                subject_id: subject_id(&database),
                work_date: date.to_string(),
            },
        )
        .unwrap();
        assert!(removed.tasks.iter().all(|row| row.id != task.id));
    }

    #[test]
    fn version_conflict_and_duplicate_batch_roll_back() {
        let database = database();
        let date = "2026-10-19";
        let task = create_task(&database, "版本事项", None);
        let first = plan_set(&database, &task.id, date, INCLUDED, None).unwrap();
        let error =
            plan_set(&database, &task.id, date, EXCLUDED, Some(first.version + 1)).unwrap_err();
        assert!(error.starts_with("VERSION_CONFLICT:"));

        let error = set_day_plan_batch(
            &database,
            TaskDayPlanBatchSetRequest {
                subject_id: subject_id(&database),
                work_date: date.to_string(),
                items: vec![
                    TaskDayPlanBatchItem {
                        task_id: task.id.clone(),
                        state: EXCLUDED.to_string(),
                        expected_version: Some(first.version),
                    },
                    TaskDayPlanBatchItem {
                        task_id: task.id.clone(),
                        state: INCLUDED.to_string(),
                        expected_version: Some(first.version),
                    },
                ],
            },
            None,
        )
        .unwrap_err();
        assert!(error.starts_with("VALIDATION_ERROR:"));
        let state: String = database
            .open()
            .unwrap()
            .query_row(
                "SELECT state FROM task_day_plans WHERE task_id=?1 AND work_date=?2",
                params![task.id, date],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(state, INCLUDED);
    }

    #[test]
    fn invalid_date_missing_task_and_cross_subject_batch_are_rejected() {
        let database = database();
        let first_subject_id = subject_id(&database);
        let first = create_task(&database, "第一主体事项", None);
        let second_subject = create_subject(
            &database,
            SubjectCreateRequest {
                name: "第二主体".to_string(),
            },
        )
        .unwrap()
        .subject;
        let second = crate::tasks::create_task(
            &database,
            TaskCreateRequest {
                subject_id: second_subject.id,
                parent_id: None,
                title: "第二主体事项".to_string(),
                planned_date: None,
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();

        let invalid_date =
            plan_set(&database, &first.id, "2026-02-30", INCLUDED, None).unwrap_err();
        assert!(invalid_date.starts_with("VALIDATION_ERROR:"));
        let missing =
            plan_set(&database, "missing-task", "2026-10-19", INCLUDED, None).unwrap_err();
        assert!(missing.starts_with("NOT_FOUND:"));

        let error = set_day_plan_batch(
            &database,
            TaskDayPlanBatchSetRequest {
                subject_id: first_subject_id,
                work_date: "2026-10-19".to_string(),
                items: vec![
                    TaskDayPlanBatchItem {
                        task_id: first.id.clone(),
                        state: INCLUDED.to_string(),
                        expected_version: None,
                    },
                    TaskDayPlanBatchItem {
                        task_id: second.id,
                        state: INCLUDED.to_string(),
                        expected_version: None,
                    },
                ],
            },
            None,
        )
        .unwrap_err();
        assert!(error.starts_with("VALIDATION_ERROR:"));
        let count: i64 = database
            .open()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM task_day_plans WHERE task_id=?1",
                [&first.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn batch_version_conflict_rolls_back_earlier_items() {
        let database = database();
        let date = "2026-10-19";
        let first = create_task(&database, "批量第一项", None);
        let second = create_task(&database, "批量冲突项", None);
        let second_plan = plan_set(&database, &second.id, date, INCLUDED, None).unwrap();

        let error = set_day_plan_batch(
            &database,
            TaskDayPlanBatchSetRequest {
                subject_id: subject_id(&database),
                work_date: date.to_string(),
                items: vec![
                    TaskDayPlanBatchItem {
                        task_id: first.id.clone(),
                        state: INCLUDED.to_string(),
                        expected_version: None,
                    },
                    TaskDayPlanBatchItem {
                        task_id: second.id.clone(),
                        state: EXCLUDED.to_string(),
                        expected_version: Some(second_plan.version + 1),
                    },
                ],
            },
            None,
        )
        .unwrap_err();
        assert!(error.starts_with("VERSION_CONFLICT:"));
        let connection = database.open().unwrap();
        let first_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM task_day_plans WHERE task_id=?1 AND work_date=?2",
                params![first.id, date],
                |row| row.get(0),
            )
            .unwrap();
        let second_state: (String, i64) = connection
            .query_row(
                "SELECT state,version FROM task_day_plans WHERE task_id=?1 AND work_date=?2",
                params![second.id, date],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(first_count, 0);
        assert_eq!(second_state, (INCLUDED.to_string(), second_plan.version));
    }

    #[test]
    fn completed_members_remain_visible_and_context_parents_are_not_executable() {
        let database = database();
        let date = "2026-10-19";
        let parent = create_task(&database, "完成事项父级", None);
        let child = create_task(&database, "今日完成事项", Some(parent.id.clone()));
        plan_set(&database, &child.id, date, INCLUDED, None).unwrap();
        database
            .open()
            .unwrap()
            .execute(
                "UPDATE tasks SET status='done',completed_at=1,updated_at=1,version=version+1 WHERE id=?1",
                [&child.id],
            )
            .unwrap();

        let result = snapshot(
            &database,
            TaskDayPlanSnapshotRequest {
                subject_id: subject_id(&database),
                work_date: date.to_string(),
            },
        )
        .unwrap();
        let parent_row = result
            .tasks
            .iter()
            .find(|task| task.id == parent.id)
            .unwrap();
        let child_row = result
            .tasks
            .iter()
            .find(|task| task.id == child.id)
            .unwrap();
        assert!(parent_row.today_context);
        assert!(!parent_row.selectable);
        assert!(child_row.today_member);
        assert_eq!(child_row.status, "done");
    }
}
