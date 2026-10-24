use std::collections::HashSet;

use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::database::Database;
use crate::tasks::{self, TaskDto};

pub const INCLUDED: &str = "included";
pub const EXCLUDED: &str = "excluded";

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TaskDayPlanSetRequest {
    pub task_id: String,
    pub work_date: String,
    pub state: String,
    pub expected_version: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDayPlanDto {
    pub task_id: String,
    pub work_date: String,
    pub state: String,
    pub version: i64,
    pub instance_task_id: Option<String>,
}

pub(crate) struct TaskDayPlanWriteResult {
    pub plan: TaskDayPlanDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDayPlanBatchSetRequest {
    pub subject_id: String,
    pub work_date: String,
    pub items: Vec<TaskDayPlanBatchItem>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TaskDayPlanBatchItem {
    pub task_id: String,
    pub state: String,
    pub expected_version: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDayPlanBatchSetResult {
    pub changed_count: usize,
}

pub fn snapshot(
    database: &Database,
    request: TaskDayPlanSnapshotRequest,
) -> Result<TaskDayPlanSnapshotDto, String> {
    validate_date(&request.work_date)?;
    let connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    ensure_subject(&connection, &workspace_id, &request.subject_id)?;
    drop(connection);
    let storage = crate::supabase::storage_mode(database)?;
    materialize_today_instances(
        database,
        &workspace_id,
        &request.subject_id,
        &request.work_date,
        Some(&storage),
    )?;
    let mut listed =
        tasks::list_day_instance_tasks(database, &request.subject_id, &request.work_date)?;
    let connection = database.open()?;
    for task in &mut listed.tasks {
        task.today_member = task.day_instance_kind.as_deref() == Some("member");
        task.today_context = task.day_instance_kind.as_deref() == Some("context");
        if let Some((state, version)) = connection
            .query_row(
                "SELECT state,version FROM task_day_plans WHERE workspace_id=?1 AND work_date=?2 AND instance_task_id=?3",
                params![workspace_id, request.work_date, task.id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?
        {
            task.day_plan_state = Some(state);
            task.day_plan_version = Some(version);
        }
    }
    Ok(TaskDayPlanSnapshotDto {
        subject_id: request.subject_id,
        work_date: request.work_date,
        tasks: listed.tasks,
        revision: listed.revision,
    })
}

#[derive(Debug, Clone)]
struct SourceTaskRow {
    subject_id: String,
    parent_id: Option<String>,
    title: String,
    planned_date: Option<String>,
    estimate_minutes: Option<i64>,
    note: Option<String>,
    project_name: Option<String>,
    solution_name: Option<String>,
    source_type: String,
    sort_order: i64,
}

fn load_source_task(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    task_id: &str,
) -> Result<
    (
        SourceTaskRow,
        Option<String>,
        Option<String>,
        Option<String>,
    ),
    String,
> {
    transaction
        .query_row(
            "SELECT subject_id,parent_id,title,planned_date,estimate_minutes,note,
                    project_name,solution_name,source_type,sort_order,
                    day_instance_date,day_instance_kind,day_instance_source_task_id
             FROM tasks WHERE workspace_id=?1 AND id=?2 AND deleted_at IS NULL",
            params![workspace_id, task_id],
            |row| {
                Ok((
                    SourceTaskRow {
                        subject_id: row.get(0)?,
                        parent_id: row.get(1)?,
                        title: row.get(2)?,
                        planned_date: row.get(3)?,
                        estimate_minutes: row.get(4)?,
                        note: row.get(5)?,
                        project_name: row.get(6)?,
                        solution_name: row.get(7)?,
                        source_type: row.get(8)?,
                        sort_order: row.get(9)?,
                    },
                    row.get(10)?,
                    row.get(11)?,
                    row.get(12)?,
                ))
            },
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "NOT_FOUND: 事项不存在".to_string())
}

fn find_active_instance(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    source_task_id: &str,
    work_date: &str,
) -> Result<Option<(String, String)>, String> {
    transaction
        .query_row(
            "SELECT id,day_instance_kind FROM tasks
             WHERE workspace_id=?1 AND day_instance_source_task_id=?2 AND day_instance_date=?3
               AND deleted_at IS NULL LIMIT 1",
            params![workspace_id, source_task_id, work_date],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| error.to_string())
}

fn upsert_plan_mapping(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    source_task_id: &str,
    work_date: &str,
    state: &str,
    instance_task_id: Option<&str>,
    now: i64,
    cloud_state: Option<&crate::supabase::StorageModeSnapshot>,
    dependency_operation_id: Option<&str>,
) -> Result<TaskDayPlanDto, String> {
    let current: Option<(String, Option<String>, i64, i64)> = transaction
        .query_row(
            "SELECT state,instance_task_id,version,created_at FROM task_day_plans
             WHERE workspace_id=?1 AND task_id=?2 AND work_date=?3",
            params![workspace_id, source_task_id, work_date],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if current
        .as_ref()
        .is_some_and(|(current_state, current_instance, _, _)| {
            current_state == state && current_instance.as_deref() == instance_task_id
        })
    {
        let (_, _, version, _) = current.unwrap();
        return Ok(TaskDayPlanDto {
            task_id: source_task_id.to_string(),
            work_date: work_date.to_string(),
            state: state.to_string(),
            version,
            instance_task_id: instance_task_id.map(str::to_string),
        });
    }
    let previous_version = current.as_ref().map(|item| item.2);
    let created_at = current.as_ref().map(|item| item.3).unwrap_or(now);
    transaction
        .execute(
            "INSERT INTO task_day_plans(workspace_id,task_id,work_date,state,instance_task_id,created_at,updated_at,version)
             VALUES(?1,?2,?3,?4,?5,?6,?6,1)
             ON CONFLICT(task_id,work_date) DO UPDATE SET
               state=excluded.state,instance_task_id=excluded.instance_task_id,
               updated_at=excluded.updated_at,version=task_day_plans.version+1",
            params![workspace_id, source_task_id, work_date, state, instance_task_id, now],
        )
        .map_err(|error| error.to_string())?;
    let version: i64 = transaction
        .query_row(
            "SELECT version FROM task_day_plans WHERE workspace_id=?1 AND task_id=?2 AND work_date=?3",
            params![workspace_id, source_task_id, work_date],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if let Some(storage) = cloud_state {
        crate::cloud_sync::enqueue_payload_in_transaction(
            transaction,
            storage,
            "task_day_plan_materialize",
            "task_day_plan",
            &format!("{source_task_id}|{work_date}"),
            previous_version,
            &json!({
                "task_id":source_task_id,
                "work_date":work_date,
                "state":state,
                "instance_task_id":instance_task_id,
                "created_at":created_at,
                "updated_at":now,
                "version":version
            }),
            None,
            dependency_operation_id,
        )?;
    }
    Ok(TaskDayPlanDto {
        task_id: source_task_id.to_string(),
        work_date: work_date.to_string(),
        state: state.to_string(),
        version,
        instance_task_id: instance_task_id.map(str::to_string),
    })
}

fn move_source_estimate_to_instance(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    source_task_id: &str,
    instance_task_id: &str,
    work_date: &str,
    now: i64,
    cloud_state: Option<&crate::supabase::StorageModeSnapshot>,
    dependency_operation_id: Option<&str>,
) -> Result<(), String> {
    let previous: Option<(i64, i64, i64)> = transaction
        .query_row(
            "SELECT estimate_minutes,created_at,version FROM task_daily_estimates
             WHERE workspace_id=?1 AND task_id=?2 AND work_date=?3",
            params![workspace_id, source_task_id, work_date],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some((minutes, created_at, version)) = previous else {
        return Ok(());
    };
    transaction
        .execute(
            "DELETE FROM task_daily_estimates WHERE workspace_id=?1 AND task_id=?2 AND work_date=?3",
            params![workspace_id, source_task_id, work_date],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO task_daily_estimates(workspace_id,task_id,work_date,estimate_minutes,created_at,updated_at,version)
             VALUES(?1,?2,?3,?4,?5,?5,1)
             ON CONFLICT(task_id,work_date) DO UPDATE SET estimate_minutes=excluded.estimate_minutes,
               updated_at=excluded.updated_at,version=task_daily_estimates.version+1",
            params![workspace_id, instance_task_id, work_date, minutes, now],
        )
        .map_err(|error| error.to_string())?;
    if let Some(storage) = cloud_state {
        crate::cloud_sync::enqueue_payload_in_transaction(
            transaction,
            storage,
            "task_daily_estimate_clear_for_today_instance",
            "task_daily_estimate",
            &format!("{source_task_id}|{work_date}"),
            Some(version),
            &json!({"task_id":source_task_id,"work_date":work_date,"deleted":true,"created_at":created_at,"updated_at":now,"version":version+1}),
            None,
            dependency_operation_id,
        )?;
        crate::cloud_sync::enqueue_payload_in_transaction(
            transaction,
            storage,
            "task_daily_estimate_set_for_today_instance",
            "task_daily_estimate",
            &format!("{instance_task_id}|{work_date}"),
            None,
            &json!({"task_id":instance_task_id,"work_date":work_date,"estimate_minutes":minutes,"created_at":now,"updated_at":now,"version":1}),
            None,
            dependency_operation_id,
        )?;
    }
    Ok(())
}

fn initialize_recurring_instance_estimate(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    source_task_id: &str,
    instance_task_id: &str,
    work_date: &str,
    source_estimate_minutes: Option<i64>,
    now: i64,
    cloud_state: Option<&crate::supabase::StorageModeSnapshot>,
    dependency_operation_id: Option<&str>,
) -> Result<(), String> {
    let Some(minutes) = source_estimate_minutes.filter(|minutes| *minutes > 0) else {
        return Ok(());
    };
    if crate::recurring::get_rule_for_date(transaction, workspace_id, source_task_id, work_date)?
        .is_none()
    {
        return Ok(());
    }
    let has_instance_estimate: bool = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM task_daily_estimates
             WHERE workspace_id=?1 AND task_id=?2 AND work_date=?3)",
            params![workspace_id, instance_task_id, work_date],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if has_instance_estimate {
        return Ok(());
    }
    transaction
        .execute(
            "INSERT INTO task_daily_estimates(
               workspace_id,task_id,work_date,estimate_minutes,created_at,updated_at,version
             ) VALUES(?1,?2,?3,?4,?5,?5,1)",
            params![workspace_id, instance_task_id, work_date, minutes, now],
        )
        .map_err(|error| error.to_string())?;
    if let Some(storage) = cloud_state {
        crate::cloud_sync::enqueue_payload_in_transaction(
            transaction,
            storage,
            "task_daily_estimate_initialize_from_recurring_source",
            "task_daily_estimate",
            &format!("{instance_task_id}|{work_date}"),
            None,
            &json!({
                "task_id":instance_task_id,
                "work_date":work_date,
                "estimate_minutes":minutes,
                "created_at":now,
                "updated_at":now,
                "version":1
            }),
            None,
            dependency_operation_id,
        )?;
    }
    Ok(())
}

fn ensure_instance_recursive(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    source_task_id: &str,
    work_date: &str,
    expected_kind: &str,
    now: i64,
    cloud_state: Option<&crate::supabase::StorageModeSnapshot>,
    dependency_operation_id: Option<&str>,
) -> Result<String, String> {
    let (source, existing_date, existing_kind, existing_source) =
        load_source_task(transaction, workspace_id, source_task_id)?;
    if let Some(date) = existing_date {
        if date != work_date || existing_source.is_none() {
            return Err("VALIDATION_ERROR: 今日实例日期或来源无效".to_string());
        }
        if expected_kind == "member" && existing_kind.as_deref() == Some("context") {
            let version: i64 = transaction
                .query_row(
                    "SELECT version FROM tasks WHERE id=?1",
                    [source_task_id],
                    |row| row.get(0),
                )
                .map_err(|error| error.to_string())?;
            transaction
                .execute(
                    "UPDATE tasks SET day_instance_kind='member',title=?1,note=?2,project_name=?3,
                       solution_name=?4,estimate_minutes=?5,planned_date=?6,updated_at=?7,version=version+1
                     WHERE id=?8 AND workspace_id=?9 AND deleted_at IS NULL",
                    params![source.title,source.note,source.project_name,source.solution_name,source.estimate_minutes,source.planned_date,now,source_task_id,workspace_id],
                )
                .map_err(|error| error.to_string())?;
            if let Some(storage) = cloud_state {
                crate::cloud_sync::enqueue_entity_in_transaction(
                    transaction,
                    storage,
                    "task_today_context_promote",
                    "task",
                    Some(source_task_id),
                    Some(version),
                    dependency_operation_id,
                )?;
            }
        }
        return Ok(source_task_id.to_string());
    }

    if let Some((instance_id, kind)) =
        find_active_instance(transaction, workspace_id, source_task_id, work_date)?
    {
        if expected_kind == "member" && kind == "context" {
            let version: i64 = transaction
                .query_row(
                    "SELECT version FROM tasks WHERE id=?1",
                    [&instance_id],
                    |row| row.get(0),
                )
                .map_err(|error| error.to_string())?;
            transaction
                .execute(
                    "UPDATE tasks SET day_instance_kind='member',title=?1,note=?2,project_name=?3,
                       solution_name=?4,estimate_minutes=?5,planned_date=?6,updated_at=?7,version=version+1
                     WHERE id=?8 AND workspace_id=?9 AND deleted_at IS NULL",
                    params![source.title,source.note,source.project_name,source.solution_name,source.estimate_minutes,source.planned_date,now,instance_id,workspace_id],
                )
                .map_err(|error| error.to_string())?;
            if let Some(storage) = cloud_state {
                crate::cloud_sync::enqueue_entity_in_transaction(
                    transaction,
                    storage,
                    "task_today_context_promote",
                    "task",
                    Some(&instance_id),
                    Some(version),
                    dependency_operation_id,
                )?;
            }
        }
        if expected_kind == "member" {
            move_source_estimate_to_instance(
                transaction,
                workspace_id,
                source_task_id,
                &instance_id,
                work_date,
                now,
                cloud_state,
                dependency_operation_id,
            )?;
            initialize_recurring_instance_estimate(
                transaction,
                workspace_id,
                source_task_id,
                &instance_id,
                work_date,
                source.estimate_minutes,
                now,
                cloud_state,
                dependency_operation_id,
            )?;
        }
        if expected_kind == "member" {
            upsert_plan_mapping(
                transaction,
                workspace_id,
                source_task_id,
                work_date,
                INCLUDED,
                Some(&instance_id),
                now,
                cloud_state,
                dependency_operation_id,
            )?;
        }
        return Ok(instance_id);
    }

    let parent_instance_id = if let Some(parent_source_id) = source.parent_id.as_deref() {
        Some(ensure_instance_recursive(
            transaction,
            workspace_id,
            parent_source_id,
            work_date,
            "context",
            now,
            cloud_state,
            dependency_operation_id,
        )?)
    } else {
        None
    };
    let id = uuid::Uuid::now_v7().to_string();
    let occurrence = crate::recurring::occurrence_for_date(
        transaction,
        workspace_id,
        source_task_id,
        work_date,
    )?;
    let is_done = expected_kind == "member"
        && occurrence
            .as_ref()
            .is_some_and(|item| item.status == "done");
    let completed_at = if is_done {
        occurrence
            .as_ref()
            .and_then(|item| item.completed_at)
            .or(Some(now))
    } else {
        None
    };
    transaction
        .execute(
            "INSERT INTO tasks(
               id,workspace_id,subject_id,parent_id,title,status,planned_date,estimate_minutes,note,
               project_name,solution_name,source_type,source_ref,sort_order,completed_at,created_at,updated_at,version,
               day_instance_date,day_instance_kind,day_instance_source_task_id
             ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,NULL,?13,?14,?15,?15,1,?16,?17,?18)",
            params![id,workspace_id,source.subject_id,parent_instance_id,source.title,
                if is_done { "done" } else { "open" },source.planned_date,source.estimate_minutes,
                source.note,source.project_name,source.solution_name,source.source_type,
                source.sort_order,completed_at,now,work_date,expected_kind,source_task_id],
        )
        .map_err(|error| error.to_string())?;
    let task_operation_id = if let Some(storage) = cloud_state {
        crate::cloud_sync::enqueue_entity_in_transaction(
            transaction,
            storage,
            "task_today_instance_create",
            "task",
            Some(&id),
            None,
            dependency_operation_id,
        )?
    } else {
        None
    };
    if expected_kind == "member" {
        move_source_estimate_to_instance(
            transaction,
            workspace_id,
            source_task_id,
            &id,
            work_date,
            now,
            cloud_state,
            task_operation_id.as_deref().or(dependency_operation_id),
        )?;
        initialize_recurring_instance_estimate(
            transaction,
            workspace_id,
            source_task_id,
            &id,
            work_date,
            source.estimate_minutes,
            now,
            cloud_state,
            task_operation_id.as_deref().or(dependency_operation_id),
        )?;
    }
    if expected_kind == "member" {
        upsert_plan_mapping(
            transaction,
            workspace_id,
            source_task_id,
            work_date,
            INCLUDED,
            Some(&id),
            now,
            cloud_state,
            task_operation_id.as_deref().or(dependency_operation_id),
        )?;
    }
    Ok(id)
}

fn materialize_today_instances(
    database: &Database,
    workspace_id: &str,
    subject_id: &str,
    work_date: &str,
    cloud_state: Option<&crate::supabase::StorageModeSnapshot>,
) -> Result<(), String> {
    let mut connection = database.open()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let mut source_ids = HashSet::<String>::new();
    {
        let mut statement = transaction
            .prepare(
                "SELECT p.task_id FROM task_day_plans p JOIN tasks t ON t.id=p.task_id
                 WHERE p.workspace_id=?1 AND p.work_date=?2 AND t.subject_id=?3
                   AND p.state='included'
                   AND (p.instance_task_id IS NULL OR EXISTS(
                       SELECT 1 FROM task_daily_estimates de
                       WHERE de.workspace_id=p.workspace_id AND de.task_id=p.task_id AND de.work_date=p.work_date
                   ))
                   AND t.deleted_at IS NULL AND t.day_instance_date IS NULL",
            )
            .map_err(|error| error.to_string())?;
        for id in statement
            .query_map(params![workspace_id, work_date, subject_id], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|error| error.to_string())?
        {
            source_ids.insert(id.map_err(|error| error.to_string())?);
        }
    }
    {
        let mut statement = transaction
            .prepare(
                "SELECT de.task_id FROM task_daily_estimates de JOIN tasks t ON t.id=de.task_id
                 LEFT JOIN task_day_plans p ON p.workspace_id=de.workspace_id AND p.task_id=de.task_id AND p.work_date=de.work_date
                 WHERE de.workspace_id=?1 AND de.work_date=?2 AND t.subject_id=?3
                   AND p.task_id IS NULL AND t.deleted_at IS NULL AND t.day_instance_date IS NULL",
            )
            .map_err(|error| error.to_string())?;
        for id in statement
            .query_map(params![workspace_id, work_date, subject_id], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|error| error.to_string())?
        {
            source_ids.insert(id.map_err(|error| error.to_string())?);
        }
    }
    let recurring_candidates = {
        let mut statement = transaction
            .prepare(
                "SELECT DISTINCT t.id FROM tasks t JOIN task_recurrence_rules r ON r.task_id=t.id
                 WHERE t.workspace_id=?1 AND t.subject_id=?2 AND t.deleted_at IS NULL
                   AND t.day_instance_date IS NULL AND r.effective_start<=?3
                   AND (r.effective_end IS NULL OR r.effective_end>=?3)",
            )
            .map_err(|error| error.to_string())?;
        let result = statement
            .query_map(params![workspace_id, subject_id, work_date], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        result
    };
    for id in recurring_candidates {
        if crate::recurring::get_rule_for_date(&transaction, workspace_id, &id, work_date)?
            .as_ref()
            .is_some_and(|rule| {
                crate::recurring::rule_matches_date(rule, work_date).unwrap_or(false)
            })
        {
            let mapped: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM task_day_plans WHERE workspace_id=?1 AND task_id=?2 AND work_date=?3)",
                params![workspace_id,id,work_date], |row| row.get(0),
            ).map_err(|error|error.to_string())?;
            if !mapped {
                source_ids.insert(id);
            }
        }
    }
    let now = now_millis();
    let mut changed = false;
    for source_id in source_ids {
        ensure_instance_recursive(
            &transaction,
            workspace_id,
            &source_id,
            work_date,
            "member",
            now,
            cloud_state,
            None,
        )?;
        changed = true;
    }
    if changed {
        bump_revision(&transaction)?;
    }
    transaction.commit().map_err(|error| error.to_string())
}

pub(crate) fn archive_day_instance_subtree_in_transaction(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    instance_id: &str,
    work_date: &str,
    now: i64,
    cloud_state: Option<&crate::supabase::StorageModeSnapshot>,
) -> Result<Vec<(String, i64)>, String> {
    let rows = {
        let mut statement = transaction
            .prepare(
                "WITH RECURSIVE subtree(id,version,parent_id,source_id) AS (
                   SELECT id,version,parent_id,day_instance_source_task_id FROM tasks
                   WHERE workspace_id=?1 AND id=?2 AND day_instance_date=?3 AND deleted_at IS NULL
                   UNION ALL
                   SELECT child.id,child.version,child.parent_id,child.day_instance_source_task_id
                   FROM tasks child JOIN subtree parent ON child.parent_id=parent.id
                   WHERE child.workspace_id=?1 AND child.day_instance_date=?3 AND child.deleted_at IS NULL
                 ) SELECT id,version,parent_id,source_id FROM subtree",
            )
            .map_err(|error| error.to_string())?;
        let result = statement
            .query_map(params![workspace_id, instance_id, work_date], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })
            .map_err(|error| error.to_string())?;
        result
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?
    };
    for (id, version, _, source_id) in &rows {
        transaction
            .execute(
                "UPDATE tasks SET deleted_at=?1,updated_at=?1,version=version+1
                 WHERE workspace_id=?2 AND id=?3 AND version=?4 AND deleted_at IS NULL",
                params![now, workspace_id, id, version],
            )
            .map_err(|error| error.to_string())?;
        crate::tasks::clear_daily_estimate_for_day_plan_in_transaction(
            transaction,
            workspace_id,
            id,
            work_date,
            now,
            cloud_state,
            None,
        )?;
        if let Some(storage) = cloud_state {
            crate::cloud_sync::enqueue_entity_in_transaction(
                transaction,
                storage,
                "task_today_instance_archive",
                "task",
                Some(id),
                Some(*version),
                None,
            )?;
        }
        if let Some(source_id) = source_id {
            upsert_plan_mapping(
                transaction,
                workspace_id,
                source_id,
                work_date,
                EXCLUDED,
                Some(id),
                now,
                cloud_state,
                None,
            )?;
        }
    }
    let mut context_parent = rows
        .iter()
        .find(|row| row.0 == instance_id)
        .and_then(|row| row.2.clone());
    while let Some(context_id) = context_parent {
        let context: Option<(String, Option<String>, Option<String>, i64)> = transaction
            .query_row(
                "SELECT day_instance_kind,parent_id,day_instance_source_task_id,version
                 FROM tasks WHERE workspace_id=?1 AND id=?2 AND day_instance_date=?3 AND deleted_at IS NULL",
                params![workspace_id, context_id, work_date],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        let Some((kind, parent_id, source_id, version)) = context else {
            break;
        };
        if kind != "context" {
            break;
        }
        let has_children: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM tasks WHERE workspace_id=?1 AND parent_id=?2 AND day_instance_date=?3 AND deleted_at IS NULL)",
                params![workspace_id, context_id, work_date], |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if has_children {
            break;
        }
        transaction
            .execute(
                "UPDATE tasks SET deleted_at=?1,updated_at=?1,version=version+1 WHERE id=?2 AND version=?3",
                params![now, context_id, version],
            )
            .map_err(|error| error.to_string())?;
        if let Some(storage) = cloud_state {
            crate::cloud_sync::enqueue_entity_in_transaction(
                transaction,
                storage,
                "task_today_context_archive",
                "task",
                Some(&context_id),
                Some(version),
                None,
            )?;
        }
        if let Some(source_id) = source_id {
            upsert_plan_mapping(
                transaction,
                workspace_id,
                &source_id,
                work_date,
                EXCLUDED,
                Some(&context_id),
                now,
                cloud_state,
                None,
            )?;
        }
        context_parent = parent_id;
    }
    Ok(rows
        .into_iter()
        .map(|(id, version, _, _)| (id, version))
        .collect())
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
    let (_, instance_date, instance_kind, instance_source_id) =
        load_source_task(transaction, workspace_id, task_id)?;
    let source_task_id = instance_source_id.as_deref().unwrap_or(task_id);
    let current_version: Option<i64> = transaction
        .query_row(
            "SELECT version FROM task_day_plans WHERE workspace_id=?1 AND task_id=?2 AND work_date=?3",
            params![workspace_id, source_task_id, work_date],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if current_version != expected_version {
        return Err("VERSION_CONFLICT: 今日计划已被其他窗口或设备更新".to_string());
    }
    if state == INCLUDED {
        let instance_id = if instance_date.is_some() {
            if instance_date.as_deref() != Some(work_date)
                || instance_kind.as_deref() != Some("member")
            {
                return Err("VALIDATION_ERROR: 今日实例不属于目标日期或不可作为成员".to_string());
            }
            task_id.to_string()
        } else {
            ensure_instance_recursive(
                transaction,
                workspace_id,
                task_id,
                work_date,
                "member",
                now,
                cloud_state,
                dependency_operation_id,
            )?
        };
        let plan = upsert_plan_mapping(
            transaction,
            workspace_id,
            source_task_id,
            work_date,
            INCLUDED,
            Some(&instance_id),
            now,
            cloud_state,
            dependency_operation_id,
        )?;
        return Ok(TaskDayPlanWriteResult { plan });
    }
    let instance_id = if instance_date.is_some() {
        Some(task_id.to_string())
    } else {
        transaction
            .query_row(
                "SELECT instance_task_id FROM task_day_plans WHERE workspace_id=?1 AND task_id=?2 AND work_date=?3 AND state=?4",
                params![workspace_id, source_task_id, work_date, INCLUDED],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(|error| error.to_string())?
            .flatten()
    };
    if let Some(instance_id) = instance_id.as_deref() {
        archive_day_instance_subtree_in_transaction(
            transaction,
            workspace_id,
            instance_id,
            work_date,
            now,
            cloud_state,
        )?;
    }
    let plan = upsert_plan_mapping(
        transaction,
        workspace_id,
        source_task_id,
        work_date,
        EXCLUDED,
        instance_id.as_deref(),
        now,
        cloud_state,
        dependency_operation_id,
    )?;
    if instance_date.is_none() {
        crate::tasks::clear_daily_estimate_for_day_plan_in_transaction(
            transaction,
            workspace_id,
            source_task_id,
            work_date,
            now,
            cloud_state,
            dependency_operation_id,
        )?;
    }
    Ok(TaskDayPlanWriteResult { plan })
}

pub(crate) fn ensure_included_in_transaction(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    task_id: &str,
    work_date: &str,
    now: i64,
    cloud_state: Option<&crate::supabase::StorageModeSnapshot>,
) -> Result<TaskDayPlanWriteResult, String> {
    let (_, instance_date, instance_kind, instance_source) =
        load_source_task(transaction, workspace_id, task_id)?;
    let source_task_id = instance_source.as_deref().unwrap_or(task_id);
    let current: Option<(String, Option<String>, i64)> = transaction
        .query_row(
            "SELECT state,instance_task_id,version FROM task_day_plans WHERE workspace_id=?1 AND task_id=?2 AND work_date=?3",
            params![workspace_id, source_task_id, work_date],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if let Some((state, Some(instance_id), version)) = current.as_ref() {
        if state == INCLUDED {
            if let Some(date) = instance_date.as_deref() {
                if date != work_date || instance_kind.as_deref() != Some("member") {
                    return Err(
                        "VALIDATION_ERROR: 今日实例不属于目标日期或不可作为成员".to_string()
                    );
                }
            }
            return Ok(TaskDayPlanWriteResult {
                plan: TaskDayPlanDto {
                    task_id: source_task_id.to_string(),
                    work_date: work_date.to_string(),
                    state: state.clone(),
                    version: *version,
                    instance_task_id: Some(instance_id.clone()),
                },
            });
        }
    }
    let expected = current.as_ref().map(|value| value.2);
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

#[cfg(test)]
pub(crate) fn set_day_plan_for_test(
    database: &Database,
    request: TaskDayPlanSetRequest,
) -> Result<TaskDayPlanDto, String> {
    set_day_plan(database, request, None)
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
    let direct_request = request.clone();
    crate::storage_contract::route(
        &database,
        crate::storage_contract::BusinessDomain::Planning,
        crate::storage_contract::StorageOperation::Query,
        |database| snapshot(database, request),
        |database| {
            crate::supabase::direct_cloud_day_plan_snapshot_for_command(database, direct_request)
        },
    )
}

#[tauri::command]
pub fn task_day_plan_set(
    database: tauri::State<'_, Database>,
    request: TaskDayPlanSetRequest,
) -> Result<TaskDayPlanDto, String> {
    let direct_request = request.clone();
    crate::storage_contract::route(
        &database,
        crate::storage_contract::BusinessDomain::Planning,
        crate::storage_contract::StorageOperation::Mutation,
        |database| {
            crate::supabase::ensure_repository_write_mode(database)?;
            let state = crate::supabase::storage_mode(database)?;
            let result = set_day_plan(database, request, Some(&state))?;
            Ok(result)
        },
        |database| crate::supabase::direct_cloud_day_plan_set_for_command(database, direct_request),
    )
}

#[tauri::command]
pub fn task_day_plan_batch_set(
    database: tauri::State<'_, Database>,
    request: TaskDayPlanBatchSetRequest,
) -> Result<TaskDayPlanBatchSetResult, String> {
    let direct_request = request.clone();
    crate::storage_contract::route(
        &database,
        crate::storage_contract::BusinessDomain::Planning,
        crate::storage_contract::StorageOperation::Mutation,
        |database| {
            crate::supabase::ensure_repository_write_mode(database)?;
            let state = crate::supabase::storage_mode(database)?;
            let result = set_day_plan_batch(database, request, Some(&state))?;
            Ok(result)
        },
        |database| {
            crate::supabase::direct_cloud_day_plan_batch_set_for_command(database, direct_request)
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subjects::{create_subject, SubjectCreateRequest};
    use crate::tasks::{TaskCreateRequest, TaskDailyEstimateSetRequest};
    use rusqlite::params;
    use std::sync::{Arc, Barrier};
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
                day_instance_date: None,
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
            .find(|task| task.day_instance_source_task_id.as_deref() == Some(parent.id.as_str()))
            .unwrap();
        assert_ne!(parent_row.id, parent.id);
        assert!(!parent_row.today_member);
        assert!(parent_row.today_context);
        let child_row = result
            .tasks
            .iter()
            .find(|task| task.day_instance_source_task_id.as_deref() == Some(child.id.as_str()))
            .unwrap();
        assert_ne!(child_row.id, child.id);
        assert!(child_row.today_member);
        assert!(!child_row.today_context);
    }

    #[test]
    fn cloud_materialization_does_not_enqueue_day_plan_for_context_parent() {
        let database = database();
        let date = "2026-10-22";
        let parent = create_task(&database, "云端上下文父事项", None);
        let child = create_task(&database, "云端今日成员子事项", Some(parent.id.clone()));
        let cloud_state = crate::supabase::StorageModeSnapshot {
            mode: "cloud".to_string(),
            storage_protocol: crate::storage_contract::LEGACY_SYNC_PROTOCOL.to_string(),
            configured_cloud_protocol: crate::storage_contract::LEGACY_SYNC_PROTOCOL.to_string(),
            storage_backend: "legacy_sync_sqlite".to_string(),
            cloud_access_state: "legacy_sync".to_string(),
            cloud_writable: true,
            read_only_reason: None,
            workspace_id: Uuid::now_v7().to_string(),
            device_id: Uuid::now_v7().to_string(),
            online: true,
            sync_state: "idle".to_string(),
            pending_operations: 0,
            conflict_count: 0,
            last_change_seq: 0,
            last_synced_at: None,
            last_error: None,
            auth_blocked: false,
            auth_blocked_reason: None,
            workspace_epoch: 1,
            epoch_blocked: false,
            epoch_blocked_reason: None,
        };

        set_day_plan_for_cloud_test(
            &database,
            TaskDayPlanSetRequest {
                task_id: child.id.clone(),
                work_date: date.to_string(),
                state: INCLUDED.to_string(),
                expected_version: None,
            },
            &cloud_state,
        )
        .unwrap();

        let connection = database.open().unwrap();
        let parent_plan_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM task_day_plans WHERE task_id=?1 AND work_date=?2",
                params![parent.id, date],
                |row| row.get(0),
            )
            .unwrap();
        let parent_outbox_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sync_outbox
                 WHERE entity_type='task_day_plan' AND entity_id=?1",
                [format!("{}|{date}", parent.id)],
                |row| row.get(0),
            )
            .unwrap();
        let child_plan_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM task_day_plans WHERE task_id=?1 AND work_date=?2 AND state='included'",
                params![child.id, date],
                |row| row.get(0),
            )
            .unwrap();
        let child_outbox_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sync_outbox
                 WHERE entity_type='task_day_plan' AND entity_id=?1",
                [format!("{}|{date}", child.id)],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(parent_plan_count, 0);
        assert_eq!(parent_outbox_count, 0);
        assert_eq!(child_plan_count, 1);
        assert_eq!(child_outbox_count, 1);
    }

    #[test]
    fn materialization_copies_source_fields_moves_today_estimate_and_keeps_prior_actual_on_source()
    {
        let database = database();
        let date = "2026-10-19";
        let subject_id = subject_id(&database);
        let parent = crate::tasks::create_task(
            &database,
            TaskCreateRequest {
                subject_id: subject_id.clone(),
                day_instance_date: None,
                parent_id: None,
                title: "来源父事项".to_string(),
                planned_date: Some("2026-10-18".to_string()),
                estimate_minutes: Some(180),
                note: Some("父事项备注".to_string()),
                project_name: Some("项目甲".to_string()),
                solution_name: Some("方案甲".to_string()),
            },
        )
        .unwrap();
        let child = crate::tasks::create_task(
            &database,
            TaskCreateRequest {
                subject_id: subject_id.clone(),
                day_instance_date: None,
                parent_id: Some(parent.id.clone()),
                title: "来源子事项".to_string(),
                planned_date: Some(date.to_string()),
                estimate_minutes: Some(75),
                note: Some("子事项备注".to_string()),
                project_name: Some("项目乙".to_string()),
                solution_name: Some("方案乙".to_string()),
            },
        )
        .unwrap();
        crate::tasks::set_task_daily_estimate(
            &database,
            TaskDailyEstimateSetRequest {
                task_id: child.id.clone(),
                work_date: date.to_string(),
                estimate_minutes: Some(45),
            },
        )
        .unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let entry_id = Uuid::now_v7().to_string();
        connection
            .execute(
                "INSERT INTO time_entries(
                   id,workspace_id,work_date,kind,source_type,state,default_task_id,label_snapshot,
                   started_at,ended_at,duration_seconds,created_at,updated_at,version
                 ) VALUES(?1,?2,?3,'work','manual','ended',?4,?5,1000,2000,900,1000,2000,1)",
                params![entry_id, workspace_id, date, child.id, child.title],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO time_allocations(
                   id,workspace_id,entry_id,task_id,minutes,created_at,updated_at,version
                 ) VALUES(?1,?2,?3,?4,15,1000,1000,1)",
                params![Uuid::now_v7().to_string(), workspace_id, entry_id, child.id],
            )
            .unwrap();
        drop(connection);

        let plan = plan_set(&database, &child.id, date, INCLUDED, None).unwrap();
        let snapshot = snapshot(
            &database,
            TaskDayPlanSnapshotRequest {
                subject_id,
                work_date: date.to_string(),
            },
        )
        .unwrap();
        let instance = snapshot
            .tasks
            .iter()
            .find(|task| task.id == plan.instance_task_id.as_deref().unwrap())
            .unwrap();
        let context = snapshot
            .tasks
            .iter()
            .find(|task| task.id == instance.parent_id.as_deref().unwrap())
            .unwrap();

        assert_eq!(instance.day_instance_kind.as_deref(), Some("member"));
        assert_eq!(
            instance.day_instance_source_task_id.as_deref(),
            Some(child.id.as_str())
        );
        assert_eq!(instance.title, child.title);
        assert_eq!(instance.planned_date, child.planned_date);
        assert_eq!(instance.estimate_minutes, child.estimate_minutes);
        assert_eq!(instance.note, child.note);
        assert_eq!(instance.project_name, child.project_name);
        assert_eq!(instance.solution_name, child.solution_name);
        assert_eq!(instance.sort_order, child.sort_order);
        assert_eq!(instance.today_estimate_minutes, Some(45));
        assert_eq!(instance.direct_minutes, 0);
        assert_eq!(context.day_instance_kind.as_deref(), Some("context"));
        assert_eq!(
            context.day_instance_source_task_id.as_deref(),
            Some(parent.id.as_str())
        );
        assert_eq!(context.title, parent.title);

        let connection = database.open().unwrap();
        let source_estimate_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM task_daily_estimates WHERE task_id=?1 AND work_date=?2",
                params![child.id, date],
                |row| row.get(0),
            )
            .unwrap();
        let source_allocated_minutes: i64 = connection
            .query_row(
                "SELECT COALESCE(SUM(minutes),0) FROM time_allocations WHERE task_id=?1",
                [&child.id],
                |row| row.get(0),
            )
            .unwrap();
        let instance_allocation_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM time_allocations WHERE task_id=?1",
                [&instance.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(source_estimate_count, 0);
        assert_eq!(source_allocated_minutes, 15);
        assert_eq!(instance_allocation_count, 0);
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
        let first = chrono::Local::now().date_naive();
        let second = first + chrono::Duration::days(1);
        let first = first.format("%Y-%m-%d").to_string();
        let second = second.format("%Y-%m-%d").to_string();
        let task = create_task(&database, "每日重复", None);
        crate::recurring::save_rule(
            &database,
            crate::recurring::RecurrenceSaveRequest {
                task_id: task.id.clone(),
                task_expected_version: task.version,
                frequency: "daily".to_string(),
                weekdays_mask: None,
                effective_start: first.clone(),
                rule_expected_version: None,
            },
        )
        .unwrap();
        plan_set(&database, &task.id, &first, EXCLUDED, None).unwrap();

        let first_result = snapshot(
            &database,
            TaskDayPlanSnapshotRequest {
                subject_id: subject_id(&database),
                work_date: first.clone(),
            },
        )
        .unwrap();
        assert!(first_result.tasks.iter().all(|row| row.id != task.id));
        let second_result = snapshot(
            &database,
            TaskDayPlanSnapshotRequest {
                subject_id: subject_id(&database),
                work_date: second,
            },
        )
        .unwrap();
        assert!(second_result
            .tasks
            .iter()
            .any(
                |row| row.day_instance_source_task_id.as_deref() == Some(task.id.as_str())
                    && row.today_member
            ));
    }

    #[test]
    fn recurring_materialization_is_idempotent_across_repeated_and_concurrent_snapshots() {
        let database = database();
        let subject_id = subject_id(&database);
        let date = chrono::Local::now()
            .date_naive()
            .format("%Y-%m-%d")
            .to_string();
        let source = create_task(&database, "并发重复事项", None);
        crate::recurring::save_rule(
            &database,
            crate::recurring::RecurrenceSaveRequest {
                task_id: source.id.clone(),
                task_expected_version: source.version,
                frequency: "daily".to_string(),
                weekdays_mask: None,
                effective_start: date.clone(),
                rule_expected_version: None,
            },
        )
        .unwrap();

        let barrier = Arc::new(Barrier::new(3));
        let handles = (0..2)
            .map(|_| {
                let database = database.clone();
                let subject_id = subject_id.clone();
                let date = date.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    snapshot(
                        &database,
                        TaskDayPlanSnapshotRequest {
                            subject_id,
                            work_date: date,
                        },
                    )
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        for handle in handles {
            handle.join().unwrap().unwrap();
        }
        snapshot(
            &database,
            TaskDayPlanSnapshotRequest {
                subject_id,
                work_date: date.clone(),
            },
        )
        .unwrap();

        let connection = database.open().unwrap();
        let active_instances: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM tasks
                 WHERE day_instance_source_task_id=?1 AND day_instance_date=?2 AND deleted_at IS NULL",
                params![source.id, date],
                |row| row.get(0),
            )
            .unwrap();
        let mappings: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM task_day_plans WHERE task_id=?1 AND work_date=?2",
                params![source.id, date],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(active_instances, 1);
        assert_eq!(mappings, 1);
    }

    #[test]
    fn recurring_estimate_initializes_today_estimate_without_overriding_explicit_daily_estimate() {
        let database = database();
        let subject_id = subject_id(&database);
        let date = chrono::Local::now()
            .date_naive()
            .format("%Y-%m-%d")
            .to_string();
        let fallback = create_task(&database, "使用每次预计", None);
        let explicit = create_task(&database, "优先单独今日预计", None);
        {
            let connection = database.open().unwrap();
            connection
                .execute(
                    "UPDATE tasks SET estimate_minutes=40 WHERE id=?1",
                    [&fallback.id],
                )
                .unwrap();
            connection
                .execute(
                    "UPDATE tasks SET estimate_minutes=60 WHERE id=?1",
                    [&explicit.id],
                )
                .unwrap();
        }
        for task in [&fallback, &explicit] {
            crate::recurring::save_rule(
                &database,
                crate::recurring::RecurrenceSaveRequest {
                    task_id: task.id.clone(),
                    task_expected_version: task.version,
                    frequency: "daily".to_string(),
                    weekdays_mask: None,
                    effective_start: date.clone(),
                    rule_expected_version: None,
                },
            )
            .unwrap();
        }
        crate::tasks::set_task_daily_estimate(
            &database,
            TaskDailyEstimateSetRequest {
                task_id: explicit.id.clone(),
                work_date: date.clone(),
                estimate_minutes: Some(25),
            },
        )
        .unwrap();

        let first = snapshot(
            &database,
            TaskDayPlanSnapshotRequest {
                subject_id: subject_id.clone(),
                work_date: date.clone(),
            },
        )
        .unwrap();
        let fallback_instance = first
            .tasks
            .iter()
            .find(|task| task.day_instance_source_task_id.as_deref() == Some(fallback.id.as_str()))
            .unwrap();
        let explicit_instance = first
            .tasks
            .iter()
            .find(|task| task.day_instance_source_task_id.as_deref() == Some(explicit.id.as_str()))
            .unwrap();
        assert_eq!(fallback_instance.today_estimate_minutes, Some(40));
        assert_eq!(explicit_instance.today_estimate_minutes, Some(25));

        database
            .open()
            .unwrap()
            .execute(
                "UPDATE tasks SET estimate_minutes=55 WHERE id=?1",
                [&fallback.id],
            )
            .unwrap();
        let second = snapshot(
            &database,
            TaskDayPlanSnapshotRequest {
                subject_id,
                work_date: date,
            },
        )
        .unwrap();
        let fallback_instance = second
            .tasks
            .iter()
            .find(|task| task.day_instance_source_task_id.as_deref() == Some(fallback.id.as_str()))
            .unwrap();
        assert_eq!(fallback_instance.today_estimate_minutes, Some(40));
    }

    #[test]
    fn estimate_does_not_add_membership_and_explicit_plan_controls_instance_lifecycle() {
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
        let connection = database.open().unwrap();
        let mapping_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM task_day_plans WHERE task_id=?1 AND work_date=?2",
                params![task.id, date],
                |row| row.get(0),
            )
            .unwrap();
        let instance_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE day_instance_source_task_id=?1 AND day_instance_date=?2",
                params![task.id, date],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(mapping_count, 0);
        assert_eq!(instance_count, 0);
        drop(connection);
        crate::tasks::set_task_daily_estimate(
            &database,
            TaskDailyEstimateSetRequest {
                task_id: task.id.clone(),
                work_date: date.to_string(),
                estimate_minutes: None,
            },
        )
        .unwrap();
        plan_set(&database, &task.id, date, INCLUDED, None).unwrap();
        let kept = snapshot(
            &database,
            TaskDayPlanSnapshotRequest {
                subject_id: subject_id(&database),
                work_date: date.to_string(),
            },
        )
        .unwrap();
        let kept_task = kept
            .tasks
            .iter()
            .find(|row| row.day_instance_source_task_id.as_deref() == Some(task.id.as_str()))
            .unwrap()
            .clone();
        assert!(kept_task.today_member);
        assert_ne!(kept_task.id, task.id);
        assert_eq!(kept_task.today_estimate_minutes, None);

        plan_set(
            &database,
            &task.id,
            date,
            EXCLUDED,
            Some(kept_task.day_plan_version.unwrap()),
        )
        .unwrap();
        let connection = database.open().unwrap();
        let estimate_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM task_daily_estimates WHERE task_id=?1 AND work_date=?2",
                params![kept_task.id, date],
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
        assert!(removed
            .tasks
            .iter()
            .all(|row| { row.day_instance_source_task_id.as_deref() != Some(task.id.as_str()) }));
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
                day_instance_date: None,
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
        let result = snapshot(
            &database,
            TaskDayPlanSnapshotRequest {
                subject_id: subject_id(&database),
                work_date: date.to_string(),
            },
        )
        .unwrap();
        let child_row = result
            .tasks
            .iter()
            .find(|task| task.day_instance_source_task_id.as_deref() == Some(child.id.as_str()))
            .unwrap()
            .clone();
        database
            .open()
            .unwrap()
            .execute(
                "UPDATE tasks SET status='done',completed_at=1,updated_at=1,version=version+1 WHERE id=?1",
                [&child_row.id],
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
            .find(|task| task.day_instance_source_task_id.as_deref() == Some(parent.id.as_str()))
            .unwrap();
        let child_row = result
            .tasks
            .iter()
            .find(|task| task.id == child_row.id)
            .unwrap();
        assert!(parent_row.today_context);
        assert!(!parent_row.selectable);
        assert!(child_row.today_member);
        assert_eq!(child_row.status, "done");
    }

    #[test]
    fn today_instances_are_content_and_lifecycle_independent_from_source_tasks() {
        let database = database();
        let date = "2026-10-19";
        let parent = create_task(&database, "来源父项", None);
        let child = create_task(&database, "来源子项", Some(parent.id.clone()));
        plan_set(&database, &child.id, date, INCLUDED, None).unwrap();

        let first_snapshot = snapshot(
            &database,
            TaskDayPlanSnapshotRequest {
                subject_id: subject_id(&database),
                work_date: date.to_string(),
            },
        )
        .unwrap();
        let first_instance = first_snapshot
            .tasks
            .iter()
            .find(|row| row.day_instance_source_task_id.as_deref() == Some(child.id.as_str()))
            .unwrap()
            .clone();
        let first_context_id = first_instance.parent_id.clone().unwrap();
        assert_ne!(first_instance.id, child.id);
        assert_eq!(
            first_instance
                .parent_id
                .as_deref()
                .map(|id| id != parent.id),
            Some(true)
        );

        crate::tasks::update_task(
            &database,
            crate::tasks::TaskUpdateRequest {
                id: first_instance.id.clone(),
                expected_version: first_instance.version,
                title: Some("今日改名".to_string()),
                parent_id: None,
                planned_date: None,
                estimate_minutes: None,
                note: Some(Some("今日备注".to_string())),
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        let current_instance = crate::tasks::update_task(
            &database,
            crate::tasks::TaskUpdateRequest {
                id: first_instance.id.clone(),
                expected_version: first_instance.version + 1,
                title: None,
                parent_id: None,
                planned_date: None,
                estimate_minutes: Some(Some(55)),
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        crate::tasks::set_task_daily_estimate(
            &database,
            TaskDailyEstimateSetRequest {
                task_id: first_instance.id.clone(),
                work_date: date.to_string(),
                estimate_minutes: Some(25),
            },
        )
        .unwrap();
        let connection = database.open().unwrap();
        let (source_title, source_estimate): (String, Option<i64>) = connection
            .query_row(
                "SELECT title,estimate_minutes FROM tasks WHERE id=?1",
                [&child.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let source_note: Option<String> = connection
            .query_row("SELECT note FROM tasks WHERE id=?1", [&child.id], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(source_title, "来源子项");
        assert_eq!(source_estimate, None);
        assert_eq!(source_note, None);
        let source_daily_estimate_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM task_daily_estimates WHERE task_id=?1 AND work_date=?2",
                params![child.id, date],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(source_daily_estimate_count, 0);
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let entry_id = Uuid::now_v7().to_string();
        connection.execute(
            "INSERT INTO time_entries(id,workspace_id,work_date,kind,source_type,state,default_task_id,label_snapshot,started_at,ended_at,duration_seconds,created_at,updated_at,version) VALUES(?1,?2,?3,'work','manual','ended',?4,'今日实例历史',1000,2000,600,1000,2000,1)",
            params![entry_id, workspace_id, date, first_instance.id],
        ).unwrap();
        connection.execute(
            "INSERT INTO time_allocations(id,workspace_id,entry_id,task_id,minutes,created_at,updated_at,version) VALUES(?1,?2,?3,?4,10,1000,1000,1)",
            params![Uuid::now_v7().to_string(), workspace_id, entry_id, first_instance.id],
        ).unwrap();
        drop(connection);

        let source = crate::tasks::update_task(
            &database,
            crate::tasks::TaskUpdateRequest {
                id: child.id.clone(),
                expected_version: child.version,
                title: Some("来源改名".to_string()),
                parent_id: None,
                planned_date: None,
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        let current_snapshot = snapshot(
            &database,
            TaskDayPlanSnapshotRequest {
                subject_id: subject_id(&database),
                work_date: date.to_string(),
            },
        )
        .unwrap();
        let current_instance_snapshot = current_snapshot
            .tasks
            .iter()
            .find(|row| row.id == first_instance.id)
            .unwrap();
        assert_eq!(current_instance_snapshot.title, "今日改名");
        assert_eq!(current_instance_snapshot.note.as_deref(), Some("今日备注"));
        assert_eq!(current_instance_snapshot.estimate_minutes, Some(55));
        assert_eq!(current_instance_snapshot.today_estimate_minutes, Some(25));

        crate::tasks::set_task_status(
            &database,
            crate::tasks::TaskStatusRequest {
                id: first_instance.id.clone(),
                expected_version: current_instance.version,
                done: true,
                occurrence_date: None,
                occurrence_expected_version: None,
            },
        )
        .unwrap();
        let source_status: String = database
            .open()
            .unwrap()
            .query_row(
                "SELECT status FROM tasks WHERE id=?1",
                [&source.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(source_status, "open");

        let plan = plan_set(
            &database,
            &child.id,
            date,
            EXCLUDED,
            current_instance_snapshot.day_plan_version,
        )
        .unwrap();
        assert_eq!(
            plan.instance_task_id.as_deref(),
            Some(first_instance.id.as_str())
        );
        let archived_at: Option<i64> = database
            .open()
            .unwrap()
            .query_row(
                "SELECT deleted_at FROM tasks WHERE id=?1",
                [&first_instance.id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(archived_at.is_some());
        let connection = database.open().unwrap();
        let archived_context_at: Option<i64> = connection
            .query_row(
                "SELECT deleted_at FROM tasks WHERE id=?1",
                [&first_context_id],
                |row| row.get(0),
            )
            .unwrap();
        let kept_historical_minutes: i64 = connection
            .query_row(
                "SELECT COALESCE(SUM(minutes),0) FROM time_allocations WHERE task_id=?1",
                [&first_instance.id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(archived_context_at.is_some());
        assert_eq!(kept_historical_minutes, 10);
        drop(connection);

        plan_set(&database, &child.id, date, INCLUDED, Some(plan.version)).unwrap();
        let second_snapshot = snapshot(
            &database,
            TaskDayPlanSnapshotRequest {
                subject_id: subject_id(&database),
                work_date: date.to_string(),
            },
        )
        .unwrap();
        let second_instance = second_snapshot
            .tasks
            .iter()
            .find(|row| row.day_instance_source_task_id.as_deref() == Some(child.id.as_str()))
            .unwrap();
        assert_ne!(second_instance.id, first_instance.id);
        assert_eq!(second_instance.title, "来源改名");
        assert_eq!(second_instance.status, "open");

        crate::tasks::delete_task(
            &database,
            crate::tasks::TaskDeleteRequest {
                id: source.id.clone(),
                expected_version: source.version,
            },
        )
        .unwrap();
        let instance_alive: bool = database
            .open()
            .unwrap()
            .query_row(
                "SELECT deleted_at IS NULL FROM tasks WHERE id=?1",
                [&second_instance.id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(instance_alive);
    }
}
