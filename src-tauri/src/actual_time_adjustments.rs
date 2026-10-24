use std::collections::{HashMap, HashSet};

use chrono::NaiveDate;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::database::Database;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActualTimeAdjustmentDto {
    pub id: String,
    pub subject_id: String,
    pub task_id: String,
    pub task_title: String,
    pub work_date: String,
    pub delta_minutes: i64,
    pub baseline_minutes: i64,
    pub target_minutes: i64,
    pub note: Option<String>,
    pub batch_id: String,
    pub created_at: i64,
    pub version: i64,
    pub deleted_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActualAdjustmentSnapshotTaskDto {
    pub id: String,
    pub subject_id: String,
    pub parent_id: Option<String>,
    pub day_instance_date: Option<String>,
    pub day_instance_kind: Option<String>,
    pub day_instance_source_task_id: Option<String>,
    pub title: String,
    pub note: Option<String>,
    pub status: String,
    pub completed_at: Option<i64>,
    pub completed_today: bool,
    pub planned_date: Option<String>,
    pub estimate_minutes: Option<i64>,
    pub today_estimate_minutes: Option<i64>,
    pub daily_estimate_version: Option<i64>,
    pub sort_order: i64,
    pub version: i64,
    pub is_leaf: bool,
    pub selectable: bool,
    pub direct_minutes: i64,
    pub total_minutes: i64,
    pub adjustment_fingerprint: String,
    pub timer_state: Option<String>,
    pub can_accept_children: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActualAdjustmentSnapshotDto {
    pub subject_id: String,
    pub work_date: String,
    pub tasks: Vec<ActualAdjustmentSnapshotTaskDto>,
    pub adjustments: Vec<ActualTimeAdjustmentDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActualAdjustmentSnapshotRequest {
    pub subject_id: String,
    pub work_date: String,
    #[serde(default)]
    pub execution_only: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActualAdjustmentBatchItem {
    pub task_id: String,
    pub expected_task_version: i64,
    pub baseline_minutes: i64,
    pub target_minutes: i64,
    pub delta_minutes: i64,
    pub adjustment_fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActualAdjustmentNewTaskInput {
    pub client_task_id: String,
    pub title: String,
    pub parent_id: Option<String>,
    pub expected_parent_version: Option<i64>,
    pub target_minutes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActualAdjustmentBatchRequest {
    pub subject_id: String,
    pub work_date: String,
    pub client_request_id: String,
    pub batch_id: String,
    pub note: Option<String>,
    #[serde(default)]
    pub day_instance_date: Option<String>,
    #[serde(default)]
    pub items: Vec<ActualAdjustmentBatchItem>,
    #[serde(default)]
    pub new_tasks: Vec<ActualAdjustmentNewTaskInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActualAdjustmentCreatedTaskDto {
    pub client_task_id: String,
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActualAdjustmentBatchResult {
    pub batch_id: String,
    pub created_adjustment_count: usize,
    pub created_task_count: usize,
    pub created_tasks: Vec<ActualAdjustmentCreatedTaskDto>,
    pub adjustments: Vec<ActualTimeAdjustmentDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActualAdjustmentDeleteRequest {
    pub adjustment_id: String,
    pub expected_version: i64,
    pub client_request_id: String,
}

fn validate_date(value: &str) -> Result<(), String> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map(|_| ())
        .map_err(|_| "VALIDATION_ERROR: 日期格式必须为 YYYY-MM-DD".to_string())
}

fn validate_request_id(value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 200 {
        Err("VALIDATION_ERROR: 客户端请求标识无效".to_string())
    } else {
        Ok(())
    }
}

fn workspace_id(connection: &Connection) -> Result<String, String> {
    connection
        .query_row(
            "SELECT id FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
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

fn bump_revision(connection: &Connection) -> Result<(), String> {
    connection
        .execute("UPDATE app_metadata SET value=CAST(CAST(value AS INTEGER)+1 AS TEXT) WHERE key='global_revision'", [])
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub(crate) fn direct_actual_minutes(
    connection: &Connection,
    workspace_id: &str,
    task_id: &str,
    work_date: Option<&str>,
) -> Result<i64, String> {
    let allocated: i64 = connection
        .query_row(
            "SELECT COALESCE(SUM(a.minutes),0)
             FROM time_allocations a JOIN time_entries e ON e.id=a.entry_id
             WHERE a.workspace_id=?1 AND a.task_id=?2 AND e.deleted_at IS NULL
               AND (?3 IS NULL OR e.work_date=?3)",
            params![workspace_id, task_id, work_date],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let adjusted: i64 = connection
        .query_row(
            "SELECT COALESCE(SUM(delta_minutes),0) FROM actual_time_adjustments
             WHERE workspace_id=?1 AND task_id=?2 AND deleted_at IS NULL
               AND (?3 IS NULL OR work_date=?3)",
            params![workspace_id, task_id, work_date],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    Ok((allocated + adjusted).max(0))
}

pub(crate) fn direct_actual_minutes_map(
    connection: &Connection,
    workspace_id: &str,
    work_date: Option<&str>,
) -> Result<HashMap<String, i64>, String> {
    let mut result = HashMap::new();
    let mut statement = connection
        .prepare(
            "SELECT task_id,SUM(minutes) FROM (
               SELECT a.task_id AS task_id,a.minutes AS minutes
               FROM time_allocations a JOIN time_entries e ON e.id=a.entry_id
               WHERE a.workspace_id=?1 AND e.deleted_at IS NULL AND (?2 IS NULL OR e.work_date=?2)
               UNION ALL
               SELECT task_id,delta_minutes FROM actual_time_adjustments
               WHERE workspace_id=?1 AND deleted_at IS NULL AND (?2 IS NULL OR work_date=?2)
             ) GROUP BY task_id",
        )
        .map_err(|error| error.to_string())?;
    for row in statement
        .query_map(params![workspace_id, work_date], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|error| error.to_string())?
    {
        let (task_id, minutes) = row.map_err(|error| error.to_string())?;
        result.insert(task_id, minutes.max(0));
    }
    Ok(result)
}

fn adjustment_fingerprint(
    connection: &Connection,
    workspace_id: &str,
    task_id: &str,
    work_date: &str,
) -> Result<String, String> {
    let mut statement = connection
        .prepare(
            "SELECT id,version,COALESCE(deleted_at,0) FROM actual_time_adjustments
             WHERE workspace_id=?1 AND task_id=?2 AND work_date=?3 ORDER BY id",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![workspace_id, task_id, work_date], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let canonical = rows
        .into_iter()
        .map(|(id, version, deleted_at)| format!("{id}:{version}:{deleted_at}"))
        .collect::<Vec<_>>()
        .join("|");
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    Ok(format!("{:x}", hasher.finalize()))
}

fn load_adjustment(connection: &Connection, id: &str) -> Result<ActualTimeAdjustmentDto, String> {
    connection
        .query_row(
            "SELECT a.id,a.subject_id,a.task_id,COALESCE(t.title,'已删除事项'),a.work_date,
                    a.delta_minutes,a.baseline_minutes,a.target_minutes,a.note,a.batch_id,
                    a.created_at,a.version,a.deleted_at
             FROM actual_time_adjustments a LEFT JOIN tasks t ON t.id=a.task_id WHERE a.id=?1",
            [id],
            |row| {
                Ok(ActualTimeAdjustmentDto {
                    id: row.get(0)?,
                    subject_id: row.get(1)?,
                    task_id: row.get(2)?,
                    task_title: row.get(3)?,
                    work_date: row.get(4)?,
                    delta_minutes: row.get(5)?,
                    baseline_minutes: row.get(6)?,
                    target_minutes: row.get(7)?,
                    note: row.get(8)?,
                    batch_id: row.get(9)?,
                    created_at: row.get(10)?,
                    version: row.get(11)?,
                    deleted_at: row.get(12)?,
                })
            },
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "NOT_FOUND: 实际调整记录不存在".to_string())
}

pub(crate) fn load_adjustment_for_history(
    connection: &Connection,
    id: &str,
) -> Result<ActualTimeAdjustmentDto, String> {
    load_adjustment(connection, id)
}

pub fn snapshot(
    database: &Database,
    request: ActualAdjustmentSnapshotRequest,
) -> Result<ActualAdjustmentSnapshotDto, String> {
    validate_date(&request.work_date)?;
    let connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let subject_exists: bool = connection
        .query_row("SELECT EXISTS(SELECT 1 FROM subjects WHERE id=?1 AND workspace_id=?2 AND deleted_at IS NULL)", params![request.subject_id,workspace_id], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    if !subject_exists {
        return Err("NOT_FOUND: 主体不存在".to_string());
    }
    let direct = direct_actual_minutes_map(&connection, &workspace_id, Some(&request.work_date))?;
    let timezone = crate::work_calendar::workspace_timezone(&connection, &workspace_id)?;
    let workday = crate::work_calendar::day_bounds(timezone, &request.work_date)?;
    let execution_only = request.execution_only.unwrap_or(false);
    let mut statement = connection.prepare(
        "SELECT t.id,t.subject_id,t.parent_id,t.title,t.note,t.status,t.completed_at,
                t.planned_date,t.estimate_minutes,de.estimate_minutes,de.version,t.sort_order,t.version,
                NOT EXISTS(SELECT 1 FROM tasks c WHERE c.parent_id=t.id AND c.deleted_at IS NULL),
                (SELECT state FROM time_entries e WHERE e.default_task_id=t.id AND e.deleted_at IS NULL AND e.state IN ('running','paused') LIMIT 1),
                t.parent_id IS NULL AND t.status='open' AND NOT EXISTS(SELECT 1 FROM task_recurrence_rules r WHERE r.task_id=t.id AND r.effective_end IS NULL),
                t.day_instance_date,t.day_instance_kind,t.day_instance_source_task_id
         FROM tasks t
         LEFT JOIN task_daily_estimates de ON de.workspace_id=t.workspace_id AND de.task_id=t.id AND de.work_date=?3
         WHERE t.workspace_id=?1 AND t.subject_id=?2 AND t.deleted_at IS NULL
           AND (t.day_instance_date IS NULL OR t.day_instance_date=?3)
           AND (?6=0 OR t.day_instance_date=?3)
           AND (t.status='open' OR (t.completed_at>=?4 AND t.completed_at<?5)
                OR EXISTS(SELECT 1 FROM time_allocations a JOIN time_entries e ON e.id=a.entry_id WHERE a.task_id=t.id AND e.work_date=?3 AND e.deleted_at IS NULL)
                OR EXISTS(SELECT 1 FROM actual_time_adjustments aa WHERE aa.task_id=t.id AND aa.work_date=?3 AND aa.deleted_at IS NULL)
                OR EXISTS(
                    SELECT 1 FROM tasks c WHERE c.parent_id=t.id AND c.deleted_at IS NULL
                      AND (c.status='open' OR (c.completed_at>=?4 AND c.completed_at<?5)
                           OR EXISTS(SELECT 1 FROM time_allocations ca JOIN time_entries ce ON ce.id=ca.entry_id WHERE ca.task_id=c.id AND ce.work_date=?3 AND ce.deleted_at IS NULL)
                           OR EXISTS(SELECT 1 FROM actual_time_adjustments cad WHERE cad.task_id=c.id AND cad.work_date=?3 AND cad.deleted_at IS NULL))
                ))
         ORDER BY t.sort_order,t.created_at,t.id"
    ).map_err(|error| error.to_string())?;
    let raw = statement
        .query_map(
            params![
                workspace_id,
                request.subject_id,
                request.work_date,
                workday.start_at,
                workday.end_at,
                execution_only as i64,
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<i64>>(8)?,
                    row.get::<_, Option<i64>>(9)?,
                    row.get::<_, Option<i64>>(10)?,
                    row.get::<_, i64>(11)?,
                    row.get::<_, i64>(12)?,
                    row.get::<_, bool>(13)?,
                    row.get::<_, Option<String>>(14)?,
                    row.get::<_, bool>(15)?,
                    row.get::<_, Option<String>>(16)?,
                    row.get::<_, Option<String>>(17)?,
                    row.get::<_, Option<String>>(18)?,
                ))
            },
        )
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let visible_ids = raw.iter().map(|row| row.0.clone()).collect::<HashSet<_>>();
    let mut children = HashMap::<String, Vec<String>>::new();
    for row in &raw {
        if let Some(parent) = &row.2 {
            if visible_ids.contains(parent) {
                children
                    .entry(parent.clone())
                    .or_default()
                    .push(row.0.clone());
            }
        }
    }
    fn total(
        id: &str,
        children: &HashMap<String, Vec<String>>,
        direct: &HashMap<String, i64>,
    ) -> i64 {
        children
            .get(id)
            .filter(|v| !v.is_empty())
            .map(|v| v.iter().map(|child| total(child, children, direct)).sum())
            .unwrap_or_else(|| *direct.get(id).unwrap_or(&0))
    }
    let mut tasks = Vec::new();
    for (
        id,
        subject_id,
        parent_id,
        title,
        note,
        status,
        completed_at,
        planned_date,
        estimate_minutes,
        today_estimate_minutes,
        daily_estimate_version,
        sort_order,
        version,
        is_leaf,
        timer_state,
        parent_candidate,
        day_instance_date,
        day_instance_kind,
        day_instance_source_task_id,
    ) in raw
    {
        let direct_minutes = *direct.get(&id).unwrap_or(&0);
        let can_accept_children =
            parent_candidate && direct_actual_minutes(&connection, &workspace_id, &id, None)? == 0;
        let completed_today =
            completed_at.is_some_and(|value| value >= workday.start_at && value < workday.end_at);
        tasks.push(ActualAdjustmentSnapshotTaskDto {
            id: id.clone(),
            subject_id,
            parent_id,
            day_instance_date,
            day_instance_kind,
            day_instance_source_task_id,
            title,
            note,
            status,
            completed_at,
            completed_today,
            planned_date,
            estimate_minutes,
            today_estimate_minutes,
            daily_estimate_version,
            sort_order,
            version,
            is_leaf,
            selectable: is_leaf,
            direct_minutes,
            total_minutes: total(&id, &children, &direct),
            adjustment_fingerprint: adjustment_fingerprint(
                &connection,
                &workspace_id,
                &id,
                &request.work_date,
            )?,
            timer_state,
            can_accept_children,
        });
    }
    let mut statement=connection.prepare("SELECT id FROM actual_time_adjustments WHERE workspace_id=?1 AND subject_id=?2 AND work_date=?3 AND deleted_at IS NULL ORDER BY created_at DESC,id DESC").map_err(|error|error.to_string())?;
    let ids = statement
        .query_map(
            params![workspace_id, request.subject_id, request.work_date],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let adjustments = ids
        .iter()
        .map(|id| load_adjustment(&connection, id))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ActualAdjustmentSnapshotDto {
        subject_id: request.subject_id,
        work_date: request.work_date,
        tasks,
        adjustments,
    })
}

fn request_hash(request: &ActualAdjustmentBatchRequest) -> Result<String, String> {
    let bytes = serde_json::to_vec(request).map_err(|error| error.to_string())?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn validate_parent(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    subject_id: &str,
    parent_id: &str,
    expected_version: Option<i64>,
) -> Result<(), String> {
    crate::tasks::validate_quick_child_parent(
        transaction,
        workspace_id,
        subject_id,
        parent_id,
        expected_version,
    )
}

pub fn apply_batch(
    database: &Database,
    request: ActualAdjustmentBatchRequest,
) -> Result<ActualAdjustmentBatchResult, String> {
    validate_date(&request.work_date)?;
    validate_request_id(&request.client_request_id)?;
    validate_request_id(&request.batch_id)?;
    if request.items.is_empty() && request.new_tasks.is_empty() {
        return Err("VALIDATION_ERROR: 调整批次不能为空".to_string());
    }
    let request_hash = request_hash(&request)?;
    let state = crate::supabase::storage_mode(database)?;
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    if let Some((existing_hash,result_json))=connection.query_row("SELECT request_hash,result_json FROM actual_adjustment_batches WHERE workspace_id=?1 AND client_request_id=?2",params![workspace_id,request.client_request_id],|row|Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?))).optional().map_err(|error|error.to_string())?{
        if existing_hash!=request_hash{return Err("IDEMPOTENCY_CONFLICT: 同一请求标识对应不同内容".to_string())}
        return serde_json::from_str(&result_json).map_err(|error|error.to_string());
    }
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    crate::work_calendar::ensure_current_work_date(&transaction, &request.work_date, now_millis())?;
    let subject_exists:bool=transaction.query_row("SELECT EXISTS(SELECT 1 FROM subjects WHERE id=?1 AND workspace_id=?2 AND deleted_at IS NULL)",params![request.subject_id,workspace_id],|row|row.get(0)).map_err(|error|error.to_string())?;
    if !subject_exists {
        return Err("NOT_FOUND: 主体不存在".to_string());
    }
    let note = request
        .note
        .as_ref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let mut adjustments = Vec::new();
    for item in &request.items {
        if !(0..=1440).contains(&item.target_minutes)
            || item.baseline_minutes < 0
            || item.target_minutes - item.baseline_minutes != item.delta_minutes
        {
            return Err("VALIDATION_ERROR: 目标分钟或固定差额无效".to_string());
        }
        if item.delta_minutes == 0 {
            continue;
        }
        let version:Option<i64>=transaction.query_row("SELECT version FROM tasks WHERE id=?1 AND workspace_id=?2 AND subject_id=?3 AND deleted_at IS NULL AND NOT EXISTS(SELECT 1 FROM tasks c WHERE c.parent_id=tasks.id AND c.deleted_at IS NULL)",params![item.task_id,workspace_id,request.subject_id],|row|row.get(0)).optional().map_err(|error|error.to_string())?;
        if version != Some(item.expected_task_version) {
            return Err("VERSION_CONFLICT: 事项已变化，请重新加载".to_string());
        }
        if adjustment_fingerprint(
            &transaction,
            &workspace_id,
            &item.task_id,
            &request.work_date,
        )? != item.adjustment_fingerprint
        {
            return Err("ADJUSTMENT_BASELINE_CONFLICT: 实际调整记录已变化，请重新加载".to_string());
        }
        let current = direct_actual_minutes(
            &transaction,
            &workspace_id,
            &item.task_id,
            Some(&request.work_date),
        )?;
        if current + item.delta_minutes < 0 {
            return Err("VALIDATION_ERROR: 调整后实际时间不能小于 0".to_string());
        }
        let id = Uuid::now_v7().to_string();
        let now = now_millis();
        transaction.execute("INSERT INTO actual_time_adjustments(id,workspace_id,subject_id,task_id,work_date,delta_minutes,baseline_minutes,target_minutes,note,batch_id,created_at,updated_at,version,created_by_device_id,updated_by_device_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?11,1,?12,?12)",params![id,workspace_id,request.subject_id,item.task_id,request.work_date,item.delta_minutes,item.baseline_minutes,item.target_minutes,note,request.batch_id,now,state.device_id]).map_err(|error|error.to_string())?;
        adjustments.push(load_adjustment(&transaction, &id)?);
    }
    let mut created_tasks = Vec::new();
    let mut seen = HashSet::new();
    for draft in &request.new_tasks {
        if !seen.insert(draft.client_task_id.clone()) || draft.client_task_id.trim().is_empty() {
            return Err("VALIDATION_ERROR: 新事项草稿标识无效或重复".to_string());
        }
        crate::tasks::validate_title(&draft.title)?;
        if !(0..=1440).contains(&draft.target_minutes) {
            return Err("VALIDATION_ERROR: 新事项目标分钟无效".to_string());
        }
        if let Some(parent_id) = draft.parent_id.as_deref() {
            validate_parent(
                &transaction,
                &workspace_id,
                &request.subject_id,
                parent_id,
                draft.expected_parent_version,
            )?
        } else if draft.expected_parent_version.is_some() {
            return Err("VALIDATION_ERROR: 顶层事项不应携带父项版本".to_string());
        }
        let now = now_millis();
        let (task, _) = crate::tasks::create_task_in_transaction(
            &transaction,
            &workspace_id,
            &crate::tasks::TaskCreateRequest {
                subject_id: request.subject_id.clone(),
                parent_id: draft.parent_id.clone(),
                day_instance_date: request.day_instance_date.clone(),
                title: draft.title.clone(),
                planned_date: request
                    .day_instance_date
                    .is_none()
                    .then(|| request.work_date.clone()),
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
            "manual",
            draft.expected_parent_version,
            now,
            None,
            None,
            None,
        )?;
        let id = task.id;
        created_tasks.push(ActualAdjustmentCreatedTaskDto {
            client_task_id: draft.client_task_id.clone(),
            task_id: id.clone(),
        });
        if draft.target_minutes > 0 {
            let adjustment_id = Uuid::now_v7().to_string();
            transaction.execute("INSERT INTO actual_time_adjustments(id,workspace_id,subject_id,task_id,work_date,delta_minutes,baseline_minutes,target_minutes,note,batch_id,created_at,updated_at,version,created_by_device_id,updated_by_device_id) VALUES(?1,?2,?3,?4,?5,?6,0,?6,?7,?8,?9,?9,1,?10,?10)",params![adjustment_id,workspace_id,request.subject_id,id,request.work_date,draft.target_minutes,note,request.batch_id,now,state.device_id]).map_err(|error|error.to_string())?;
            adjustments.push(load_adjustment(&transaction, &adjustment_id)?);
        }
    }
    bump_revision(&transaction)?;
    let result = ActualAdjustmentBatchResult {
        batch_id: request.batch_id.clone(),
        created_adjustment_count: adjustments.len(),
        created_task_count: created_tasks.len(),
        created_tasks,
        adjustments,
    };
    let result_json = serde_json::to_string(&result).map_err(|error| error.to_string())?;
    transaction.execute("INSERT INTO actual_adjustment_batches(workspace_id,client_request_id,batch_id,request_hash,result_json,created_at) VALUES(?1,?2,?3,?4,?5,?6)",params![workspace_id,request.client_request_id,request.batch_id,request_hash,result_json,now_millis()]).map_err(|error|error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

pub fn delete_adjustment(
    database: &Database,
    request: ActualAdjustmentDeleteRequest,
) -> Result<ActualTimeAdjustmentDto, String> {
    validate_request_id(&request.client_request_id)?;
    let state = crate::supabase::storage_mode(database)?;
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let current = load_adjustment(&transaction, &request.adjustment_id)?;
    let now = now_millis();
    crate::work_calendar::ensure_current_work_date(&transaction, &current.work_date, now)?;
    if current.deleted_at.is_some() {
        return Ok(current);
    }
    if current.version != request.expected_version {
        return Err("VERSION_CONFLICT: 实际调整记录已变化".to_string());
    }
    transaction.execute("UPDATE actual_time_adjustments SET deleted_at=?1,updated_at=?1,version=version+1,updated_by_device_id=?2 WHERE id=?3 AND workspace_id=?4",params![now,state.device_id,request.adjustment_id,workspace_id]).map_err(|error|error.to_string())?;
    bump_revision(&transaction)?;
    let result = load_adjustment(&transaction, &request.adjustment_id)?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

#[tauri::command]
pub fn actual_adjustment_snapshot(
    database: tauri::State<'_, Database>,
    request: ActualAdjustmentSnapshotRequest,
) -> Result<ActualAdjustmentSnapshotDto, String> {
    let direct_request = request.clone();
    crate::storage_contract::route(
        &database,
        crate::storage_contract::BusinessDomain::Time,
        crate::storage_contract::StorageOperation::Query,
        |database| snapshot(database, request),
        |database| {
            crate::supabase::direct_cloud_actual_adjustment_snapshot(database, direct_request)
        },
    )
}

#[tauri::command]
pub fn actual_adjustment_batch_apply(
    database: tauri::State<'_, Database>,
    request: ActualAdjustmentBatchRequest,
) -> Result<ActualAdjustmentBatchResult, String> {
    let direct_request = request.clone();
    crate::storage_contract::route(
        &database,
        crate::storage_contract::BusinessDomain::Time,
        crate::storage_contract::StorageOperation::Mutation,
        |database| {
            crate::supabase::ensure_repository_write_mode(database)?;
            apply_batch(database, request)
        },
        |database| {
            crate::supabase::direct_cloud_actual_adjustment_batch_apply(database, direct_request)
        },
    )
}

#[tauri::command]
pub fn actual_adjustment_delete(
    database: tauri::State<'_, Database>,
    request: ActualAdjustmentDeleteRequest,
) -> Result<ActualTimeAdjustmentDto, String> {
    let direct_request = request.clone();
    crate::storage_contract::route(
        &database,
        crate::storage_contract::BusinessDomain::Time,
        crate::storage_contract::StorageOperation::Mutation,
        |database| {
            crate::supabase::ensure_repository_write_mode(database)?;
            delete_adjustment(database, request)
        },
        |database| crate::supabase::direct_cloud_actual_adjustment_delete(database, direct_request),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn fixture() -> (Database, String, String) {
        let dir = tempdir().unwrap();
        let path = dir.keep();
        let database = Database::initialize(&path).unwrap();
        let connection = database.open().unwrap();
        let workspace: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let subject: String = connection
            .query_row(
                "SELECT id FROM subjects WHERE workspace_id=?1 LIMIT 1",
                [&workspace],
                |row| row.get(0),
            )
            .unwrap();
        (database, workspace, subject)
    }

    fn current_work_date(database: &Database) -> String {
        crate::work_calendar::current_work_date(&database.open().unwrap(), now_millis()).unwrap()
    }

    #[test]
    fn creates_new_task_and_adjustment_atomically_and_is_idempotent() {
        let (database, _workspace, subject) = fixture();
        let work_date = current_work_date(&database);
        let request = ActualAdjustmentBatchRequest {
            subject_id: subject.clone(),
            work_date,
            client_request_id: "request-1".into(),
            batch_id: "batch-1".into(),
            note: Some("核对".into()),
            day_instance_date: None,
            items: vec![],
            new_tasks: vec![ActualAdjustmentNewTaskInput {
                client_task_id: "draft-1".into(),
                title: "临时工作".into(),
                parent_id: None,
                expected_parent_version: None,
                target_minutes: 45,
            }],
        };
        let first = apply_batch(&database, request.clone()).unwrap();
        let second = apply_batch(&database, request).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.created_task_count, 1);
        assert_eq!(first.created_adjustment_count, 1);
        let connection = database.open().unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM tasks WHERE title='临时工作'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
    }

    #[test]
    fn today_instance_actual_adjustment_does_not_increase_source_minutes() {
        let (database, workspace, subject) = fixture();
        let work_date = current_work_date(&database);
        let source = insert_task(&database, &workspace, &subject, "长期来源", None);
        let instance = insert_task(&database, &workspace, &subject, "今日副本", None);
        database
            .open()
            .unwrap()
            .execute(
                "UPDATE tasks SET day_instance_date=?1,day_instance_kind='member',day_instance_source_task_id=?2 WHERE id=?3",
                params![work_date, source, instance],
            )
            .unwrap();
        let fingerprint =
            adjustment_fingerprint(&database.open().unwrap(), &workspace, &instance, &work_date)
                .unwrap();

        let result = apply_batch(
            &database,
            ActualAdjustmentBatchRequest {
                subject_id: subject,
                work_date: work_date.clone(),
                client_request_id: "instance-actual-request".into(),
                batch_id: "instance-actual-batch".into(),
                note: None,
                day_instance_date: Some(work_date.clone()),
                items: vec![ActualAdjustmentBatchItem {
                    task_id: instance.clone(),
                    expected_task_version: 1,
                    baseline_minutes: 0,
                    target_minutes: 35,
                    delta_minutes: 35,
                    adjustment_fingerprint: fingerprint,
                }],
                new_tasks: vec![],
            },
        )
        .unwrap();

        assert_eq!(result.adjustments.len(), 1);
        assert_eq!(result.adjustments[0].task_id, instance);
        let connection = database.open().unwrap();
        assert_eq!(
            direct_actual_minutes(&connection, &workspace, &source, Some(&work_date)).unwrap(),
            0
        );
        let outbox_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM sync_outbox", [], |row| row.get(0))
            .unwrap();
        assert_eq!(outbox_count, 0);
        assert_eq!(
            direct_actual_minutes(&connection, &workspace, &instance, Some(&work_date)).unwrap(),
            35
        );
    }

    #[test]
    fn zero_target_creates_task_without_adjustment_and_invalid_draft_rolls_back() {
        let (database, _workspace, subject) = fixture();
        let work_date = current_work_date(&database);
        let zero = apply_batch(
            &database,
            ActualAdjustmentBatchRequest {
                subject_id: subject.clone(),
                work_date: work_date.clone(),
                client_request_id: "zero".into(),
                batch_id: "zero-batch".into(),
                note: None,
                day_instance_date: None,
                items: vec![],
                new_tasks: vec![ActualAdjustmentNewTaskInput {
                    client_task_id: "zero-task".into(),
                    title: "只建事项".into(),
                    parent_id: None,
                    expected_parent_version: None,
                    target_minutes: 0,
                }],
            },
        )
        .unwrap();
        assert_eq!(zero.created_task_count, 1);
        assert_eq!(zero.created_adjustment_count, 0);
        assert!(apply_batch(
            &database,
            ActualAdjustmentBatchRequest {
                subject_id: subject,
                work_date,
                client_request_id: "bad".into(),
                batch_id: "bad-batch".into(),
                note: None,
                day_instance_date: None,
                items: vec![],
                new_tasks: vec![ActualAdjustmentNewTaskInput {
                    client_task_id: "bad-task".into(),
                    title: " ".into(),
                    parent_id: None,
                    expected_parent_version: None,
                    target_minutes: 10
                }]
            }
        )
        .is_err());
        let connection = database.open().unwrap();
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM tasks WHERE title=''", [], |row| row
                    .get::<_, i64>(
                    0
                ))
                .unwrap(),
            0
        );
    }

    fn insert_task(
        database: &Database,
        workspace: &str,
        subject: &str,
        title: &str,
        parent_id: Option<&str>,
    ) -> String {
        let id = Uuid::now_v7().to_string();
        database.open().unwrap().execute(
            "INSERT INTO tasks(id,workspace_id,subject_id,parent_id,title,status,source_type,sort_order,created_at,updated_at,version)
             VALUES(?1,?2,?3,?4,?5,'open','manual',10,1,1,1)",
            params![id, workspace, subject, parent_id, title],
        ).unwrap();
        id
    }

    #[test]
    fn batch_contract_defaults_new_tasks_and_round_trips_created_mapping() {
        let legacy: ActualAdjustmentBatchRequest = serde_json::from_value(json!({
            "subjectId": "subject",
            "workDate": "2026-10-15",
            "clientRequestId": "request",
            "batchId": "batch"
        }))
        .unwrap();
        assert!(legacy.items.is_empty());
        assert!(legacy.new_tasks.is_empty());

        let result = ActualAdjustmentBatchResult {
            batch_id: "batch".into(),
            created_adjustment_count: 0,
            created_task_count: 1,
            created_tasks: vec![ActualAdjustmentCreatedTaskDto {
                client_task_id: "draft".into(),
                task_id: "task".into(),
            }],
            adjustments: vec![],
        };
        let decoded: ActualAdjustmentBatchResult =
            serde_json::from_value(serde_json::to_value(&result).unwrap()).unwrap();
        assert_eq!(decoded, result);
    }

    #[test]
    fn adjustment_fingerprint_uses_the_cross_database_canonical_format() {
        let (database, workspace, subject) = fixture();
        let task = insert_task(&database, &workspace, &subject, "指纹事项", None);
        let connection = database.open().unwrap();
        let first_id = "81000000-0000-0000-0000-000000000001";
        let second_id = "81000000-0000-0000-0000-000000000002";
        connection.execute("INSERT INTO actual_time_adjustments(id,workspace_id,subject_id,task_id,work_date,delta_minutes,baseline_minutes,target_minutes,batch_id,created_at,updated_at,version,deleted_at) VALUES(?1,?2,?3,?4,'2026-10-15',5,0,5,?5,1,1,2,NULL)",params![first_id,workspace,subject,task,Uuid::now_v7().to_string()]).unwrap();
        connection.execute("INSERT INTO actual_time_adjustments(id,workspace_id,subject_id,task_id,work_date,delta_minutes,baseline_minutes,target_minutes,batch_id,created_at,updated_at,version,deleted_at) VALUES(?1,?2,?3,?4,'2026-10-15',-2,5,3,?5,1,1,4,1791997200123)",params![second_id,workspace,subject,task,Uuid::now_v7().to_string()]).unwrap();
        let canonical = format!("{first_id}:2:0|{second_id}:4:1791997200123");
        assert_eq!(
            adjustment_fingerprint(&connection, &workspace, &task, "2026-10-15").unwrap(),
            format!("{:x}", Sha256::digest(canonical.as_bytes()))
        );
    }

    #[test]
    fn execution_snapshot_keeps_original_tree_order_and_workspace_today_context() {
        let (database, workspace, subject) = fixture();
        database
            .open()
            .unwrap()
            .execute(
                "UPDATE workspaces SET timezone='Asia/Shanghai' WHERE id=?1",
                [&workspace],
            )
            .unwrap();
        let timezone: chrono_tz::Tz = "Asia/Shanghai".parse().unwrap();
        let work_date = "2026-10-15";
        let bounds = crate::work_calendar::day_bounds(timezone, work_date).unwrap();
        let parent = insert_task(&database, &workspace, &subject, "已完成父项", None);
        let open_child = insert_task(&database, &workspace, &subject, "进行中子项", Some(&parent));
        let today_child = insert_task(&database, &workspace, &subject, "今天完成", Some(&parent));
        let old_done = insert_task(&database, &workspace, &subject, "历史完成", None);
        let connection = database.open().unwrap();
        connection.execute(
            "UPDATE tasks SET status='done',completed_at=?1,sort_order=10,note='父备注' WHERE id=?2",
            params![bounds.start_at + 1000, parent],
        ).unwrap();
        connection
            .execute("UPDATE tasks SET sort_order=20 WHERE id=?1", [&open_child])
            .unwrap();
        connection
            .execute(
                "UPDATE tasks SET status='done',completed_at=?1,sort_order=30 WHERE id=?2",
                params![bounds.end_at - 1000, today_child],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE tasks SET status='done',completed_at=?1,sort_order=40 WHERE id=?2",
                params![bounds.start_at - 1000, old_done],
            )
            .unwrap();
        connection.execute(
            "INSERT INTO task_daily_estimates(workspace_id,task_id,work_date,estimate_minutes,created_at,updated_at,version) VALUES(?1,?2,?3,45,1,1,2)",
            params![workspace, open_child, work_date],
        ).unwrap();
        let entry = Uuid::now_v7().to_string();
        connection.execute(
            "INSERT INTO time_entries(id,workspace_id,work_date,kind,source_type,state,default_task_id,label_snapshot,started_at,duration_seconds,created_at,updated_at,version) VALUES(?1,?2,?3,'work','timer','running',?4,'进行中子项',?5,0,?5,?5,1)",
            params![entry, workspace, work_date, open_child, bounds.start_at + 2000],
        ).unwrap();
        drop(connection);

        let result = snapshot(
            &database,
            ActualAdjustmentSnapshotRequest {
                subject_id: subject,
                work_date: work_date.into(),
                execution_only: None,
            },
        )
        .unwrap();
        let visible_ids = result
            .tasks
            .iter()
            .map(|task| task.id.as_str())
            .collect::<Vec<_>>();
        let parent_index = visible_ids.iter().position(|id| **id == parent).unwrap();
        let open_index = visible_ids
            .iter()
            .position(|id| **id == open_child)
            .unwrap();
        let today_index = visible_ids
            .iter()
            .position(|id| **id == today_child)
            .unwrap();
        assert!(parent_index < open_index && open_index < today_index);
        let parent_row = result.tasks.iter().find(|task| task.id == parent).unwrap();
        assert!(parent_row.completed_today);
        assert!(!parent_row.is_leaf);
        assert_eq!(parent_row.note.as_deref(), Some("父备注"));
        let open_row = result
            .tasks
            .iter()
            .find(|task| task.id == open_child)
            .unwrap();
        assert_eq!(open_row.today_estimate_minutes, Some(45));
        assert_eq!(open_row.daily_estimate_version, Some(2));
        assert_eq!(open_row.timer_state.as_deref(), Some("running"));
        assert!(!result.tasks.iter().any(|task| task.id == old_done));
    }

    #[test]
    fn mixed_batch_rolls_back_everything_when_a_later_draft_is_invalid() {
        let (database, workspace, subject) = fixture();
        let leaf = insert_task(&database, &workspace, &subject, "现有事项", None);
        let work_date = current_work_date(&database);
        let request = ActualAdjustmentBatchRequest {
            subject_id: subject,
            work_date: work_date.clone(),
            client_request_id: "mixed-rollback".into(),
            batch_id: "mixed-rollback-batch".into(),
            note: None,
            day_instance_date: None,
            items: vec![ActualAdjustmentBatchItem {
                task_id: leaf.clone(),
                expected_task_version: 1,
                baseline_minutes: 0,
                target_minutes: 20,
                delta_minutes: 20,
                adjustment_fingerprint: adjustment_fingerprint(
                    &database.open().unwrap(),
                    &workspace,
                    &leaf,
                    &work_date,
                )
                .unwrap(),
            }],
            new_tasks: vec![
                ActualAdjustmentNewTaskInput {
                    client_task_id: "valid-draft".into(),
                    title: "本应回滚".into(),
                    parent_id: None,
                    expected_parent_version: None,
                    target_minutes: 10,
                },
                ActualAdjustmentNewTaskInput {
                    client_task_id: "invalid-draft".into(),
                    title: " ".into(),
                    parent_id: None,
                    expected_parent_version: None,
                    target_minutes: 5,
                },
            ],
        };
        assert!(apply_batch(&database, request).is_err());
        let connection = database.open().unwrap();
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM actual_time_adjustments", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM tasks WHERE title='本应回滚'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM actual_adjustment_batches",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn parent_eligibility_and_version_are_revalidated_at_commit_time() {
        let (database, workspace, subject) = fixture();
        let parent = insert_task(&database, &workspace, &subject, "父事项", None);
        let work_date = current_work_date(&database);
        let make_request = |request_id: &str, version: i64| ActualAdjustmentBatchRequest {
            subject_id: subject.clone(),
            work_date: work_date.clone(),
            client_request_id: request_id.into(),
            batch_id: format!("{request_id}-batch"),
            note: None,
            day_instance_date: None,
            items: vec![],
            new_tasks: vec![ActualAdjustmentNewTaskInput {
                client_task_id: format!("{request_id}-draft"),
                title: "新子项".into(),
                parent_id: Some(parent.clone()),
                expected_parent_version: Some(version),
                target_minutes: 5,
            }],
        };
        database
            .open()
            .unwrap()
            .execute("UPDATE tasks SET version=2 WHERE id=?1", [&parent])
            .unwrap();
        assert!(apply_batch(&database, make_request("stale-parent", 1))
            .unwrap_err()
            .contains("VERSION_CONFLICT"));

        let entry = Uuid::now_v7().to_string();
        let connection = database.open().unwrap();
        connection.execute("INSERT INTO time_entries(id,workspace_id,work_date,kind,source_type,state,label_snapshot,started_at,ended_at,duration_seconds,created_at,updated_at,version) VALUES(?1,?2,?3,'work','manual','ended','父事项计时',1,61,60,1,1,1)", params![entry, workspace, work_date]).unwrap();
        connection.execute("INSERT INTO time_allocations(id,workspace_id,entry_id,task_id,minutes,created_at,updated_at,version) VALUES(?1,?2,?3,?4,1,1,1,1)", params![Uuid::now_v7().to_string(), workspace, entry, parent]).unwrap();
        drop(connection);
        assert!(apply_batch(&database, make_request("timed-parent", 2))
            .unwrap_err()
            .contains("TASK_HAS_DIRECT_TIME"));
    }

    #[test]
    fn cloud_compatible_batch_is_idempotent_without_creating_outbox() {
        use crate::settings::{self, SettingsScope, SettingsUpdate};
        let (database, _workspace, subject) = fixture();
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "storage_mode".into(),
                value: json!("cloud"),
            },
        )
        .unwrap();
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "cloud_workspace_id".into(),
                value: json!(Uuid::now_v7().to_string()),
            },
        )
        .unwrap();
        let work_date = current_work_date(&database);
        let request = ActualAdjustmentBatchRequest {
            subject_id: subject,
            work_date,
            client_request_id: "40000000-0000-0000-0000-000000000001".into(),
            batch_id: "41000000-0000-0000-0000-000000000001".into(),
            note: None,
            day_instance_date: None,
            items: vec![],
            new_tasks: vec![ActualAdjustmentNewTaskInput {
                client_task_id: "draft".into(),
                title: "云端兼容事项".into(),
                parent_id: None,
                expected_parent_version: None,
                target_minutes: 30,
            }],
        };
        let first = apply_batch(&database, request.clone()).unwrap();
        let repeated = apply_batch(&database, request).unwrap();
        assert_eq!(repeated, first);
        assert_eq!(first.created_task_count, 1);
        assert_eq!(first.created_adjustment_count, 1);
        assert_eq!(first.created_tasks[0].client_task_id, "draft");
        assert_eq!(first.adjustments[0].task_id, first.created_tasks[0].task_id);
        let connection = database.open().unwrap();
        let outbox_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM sync_outbox", [], |row| row.get(0))
            .unwrap();
        assert_eq!(outbox_count, 0);
    }

    #[test]
    fn deleting_adjustment_is_idempotent_and_removes_it_from_effective_actual() {
        let (database, workspace, subject) = fixture();
        let task = insert_task(&database, &workspace, &subject, "可撤销事项", None);
        let work_date = current_work_date(&database);
        let created = apply_batch(
            &database,
            ActualAdjustmentBatchRequest {
                subject_id: subject,
                work_date: work_date.clone(),
                client_request_id: "delete-adjustment".into(),
                batch_id: "delete-adjustment-batch".into(),
                note: None,
                day_instance_date: None,
                items: vec![ActualAdjustmentBatchItem {
                    task_id: task.clone(),
                    expected_task_version: 1,
                    baseline_minutes: 0,
                    target_minutes: 25,
                    delta_minutes: 25,
                    adjustment_fingerprint: adjustment_fingerprint(
                        &database.open().unwrap(),
                        &workspace,
                        &task,
                        &work_date,
                    )
                    .unwrap(),
                }],
                new_tasks: vec![],
            },
        )
        .unwrap();
        let adjustment = created.adjustments.first().unwrap().clone();
        let deleted = delete_adjustment(
            &database,
            ActualAdjustmentDeleteRequest {
                adjustment_id: adjustment.id.clone(),
                expected_version: adjustment.version,
                client_request_id: "delete-1".into(),
            },
        )
        .unwrap();
        assert!(deleted.deleted_at.is_some());
        assert_eq!(
            direct_actual_minutes(
                &database.open().unwrap(),
                &workspace,
                &task,
                Some(&work_date)
            )
            .unwrap(),
            0
        );
        let repeated = delete_adjustment(
            &database,
            ActualAdjustmentDeleteRequest {
                adjustment_id: adjustment.id,
                expected_version: adjustment.version,
                client_request_id: "delete-2".into(),
            },
        )
        .unwrap();
        assert!(repeated.deleted_at.is_some());
    }

    #[test]
    fn historical_adjustment_delete_is_rejected_without_mutation() {
        let (database, workspace, subject) = fixture();
        let task = insert_task(&database, &workspace, &subject, "历史调整", None);
        let work_date = "2000-01-01";
        let adjustment_id = Uuid::now_v7().to_string();
        let batch_id = Uuid::now_v7().to_string();
        database.open().unwrap().execute(
            "INSERT INTO actual_time_adjustments(id,workspace_id,subject_id,task_id,work_date,delta_minutes,baseline_minutes,target_minutes,batch_id,created_at,updated_at,version,deleted_at) VALUES(?1,?2,?3,?4,?5,10,0,10,?6,1,1,1,NULL)",
            params![adjustment_id, workspace, subject, task, work_date, batch_id],
        ).unwrap();
        let error = delete_adjustment(
            &database,
            ActualAdjustmentDeleteRequest {
                adjustment_id: adjustment_id.clone(),
                expected_version: 1,
                client_request_id: "historical-delete".into(),
            },
        )
        .unwrap_err();
        assert!(error.starts_with("HISTORICAL_RECORD_READ_ONLY:"));
        let stored = load_adjustment(&database.open().unwrap(), &adjustment_id).unwrap();
        assert!(stored.deleted_at.is_none());
        assert_eq!(stored.version, 1);
    }

    #[test]
    fn historical_adjustment_batch_is_rejected_without_mutation() {
        let (database, workspace, subject) = fixture();
        let task = insert_task(&database, &workspace, &subject, "历史创建保护", None);
        let error = apply_batch(
            &database,
            ActualAdjustmentBatchRequest {
                subject_id: subject,
                work_date: "2000-01-01".into(),
                client_request_id: "historical-create".into(),
                batch_id: "historical-create-batch".into(),
                note: None,
                day_instance_date: None,
                items: vec![ActualAdjustmentBatchItem {
                    task_id: task.clone(),
                    expected_task_version: 1,
                    baseline_minutes: 0,
                    target_minutes: 10,
                    delta_minutes: 10,
                    adjustment_fingerprint: adjustment_fingerprint(
                        &database.open().unwrap(),
                        &workspace,
                        &task,
                        "2000-01-01",
                    )
                    .unwrap(),
                }],
                new_tasks: vec![ActualAdjustmentNewTaskInput {
                    client_task_id: "historical-draft".into(),
                    title: "不应创建".into(),
                    parent_id: None,
                    expected_parent_version: None,
                    target_minutes: 5,
                }],
            },
        )
        .unwrap_err();
        assert!(error.starts_with("HISTORICAL_RECORD_READ_ONLY:"));
        let connection = database.open().unwrap();
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM actual_time_adjustments", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM actual_adjustment_batches",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM tasks WHERE title='不应创建'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
    }
}
