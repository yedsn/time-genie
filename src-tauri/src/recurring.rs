use chrono::{Datelike, Duration, Local, NaiveDate};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::database::Database;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecurrenceRuleDto {
    pub id: String,
    pub task_id: String,
    pub frequency: String,
    pub weekdays_mask: Option<i64>,
    pub effective_start: String,
    pub effective_end: Option<String>,
    pub summary: String,
    pub version: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TaskOccurrenceDto {
    pub task_id: String,
    pub occurrence_date: String,
    pub origin: String,
    pub status: String,
    pub completed_at: Option<i64>,
    pub version: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecurrenceSaveRequest {
    pub task_id: String,
    pub task_expected_version: i64,
    pub frequency: String,
    pub weekdays_mask: Option<i64>,
    pub effective_start: String,
    pub rule_expected_version: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecurrenceCloseRequest {
    pub task_id: String,
    pub task_expected_version: i64,
    pub effective_end: String,
    pub rule_expected_version: i64,
}

pub fn get_active_rule(
    connection: &Connection,
    workspace_id: &str,
    task_id: &str,
) -> Result<Option<RecurrenceRuleDto>, String> {
    connection
        .query_row(
            "SELECT id, task_id, frequency, weekdays_mask, effective_start, effective_end, version
             FROM task_recurrence_rules
             WHERE workspace_id = ?1 AND task_id = ?2 AND effective_end IS NULL
             ORDER BY effective_start DESC LIMIT 1",
            params![workspace_id, task_id],
            rule_from_row,
        )
        .optional()
        .map_err(|error| error.to_string())
}

pub fn get_rule_for_date(
    connection: &Connection,
    workspace_id: &str,
    task_id: &str,
    date: &str,
) -> Result<Option<RecurrenceRuleDto>, String> {
    validate_date(date)?;
    connection
        .query_row(
            "SELECT id, task_id, frequency, weekdays_mask, effective_start, effective_end, version
             FROM task_recurrence_rules
             WHERE workspace_id = ?1 AND task_id = ?2
               AND effective_start <= ?3
               AND (effective_end IS NULL OR effective_end >= ?3)
             ORDER BY effective_start DESC LIMIT 1",
            params![workspace_id, task_id, date],
            rule_from_row,
        )
        .optional()
        .map_err(|error| error.to_string())
}

pub fn get_latest_rule(
    connection: &Connection,
    workspace_id: &str,
    task_id: &str,
) -> Result<Option<RecurrenceRuleDto>, String> {
    connection
        .query_row(
            "SELECT id, task_id, frequency, weekdays_mask, effective_start, effective_end, version
             FROM task_recurrence_rules
             WHERE workspace_id = ?1 AND task_id = ?2
             ORDER BY effective_start DESC LIMIT 1",
            params![workspace_id, task_id],
            rule_from_row,
        )
        .optional()
        .map_err(|error| error.to_string())
}

pub fn occurrence_for_date(
    connection: &Connection,
    workspace_id: &str,
    task_id: &str,
    date: &str,
) -> Result<Option<TaskOccurrenceDto>, String> {
    validate_date(date)?;
    connection
        .query_row(
            "SELECT task_id, occurrence_date, origin, status, completed_at, version
             FROM task_occurrences
             WHERE workspace_id = ?1 AND task_id = ?2 AND occurrence_date = ?3",
            params![workspace_id, task_id, date],
            |row| {
                Ok(TaskOccurrenceDto {
                    task_id: row.get(0)?,
                    occurrence_date: row.get(1)?,
                    origin: row.get(2)?,
                    status: row.get(3)?,
                    completed_at: row.get(4)?,
                    version: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(|error| error.to_string())
}

pub fn rule_matches_date(rule: &RecurrenceRuleDto, date: &str) -> Result<bool, String> {
    let date = parse_date(date)?;
    let start = parse_date(&rule.effective_start)?;
    if date < start {
        return Ok(false);
    }
    if let Some(end) = rule.effective_end.as_deref() {
        if date > parse_date(end)? {
            return Ok(false);
        }
    }
    let weekday = date.weekday().num_days_from_monday();
    match rule.frequency.as_str() {
        "daily" => Ok(true),
        "weekdays" => Ok(weekday < 5),
        "weekly" => Ok(rule
            .weekdays_mask
            .is_some_and(|mask| mask & (1_i64 << weekday) != 0)),
        _ => Err("VALIDATION_ERROR: 不支持的重复频率".to_string()),
    }
}

pub fn ensure_occurrence(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    task_id: &str,
    date: &str,
    now: i64,
) -> Result<TaskOccurrenceDto, String> {
    validate_date(date)?;
    let active_definition = get_active_rule(transaction, workspace_id, task_id)?;
    let dated_rule = get_rule_for_date(transaction, workspace_id, task_id, date)?;
    if active_definition.is_none() && dated_rule.is_none() {
        return Err("TASK_NOT_RECURRING: 事项未启用重复".to_string());
    }
    let origin = if let Some(rule) = dated_rule.as_ref() {
        if rule_matches_date(rule, date)? {
            "scheduled"
        } else {
            "manual"
        }
    } else {
        "manual"
    };
    transaction
        .execute(
            "INSERT INTO task_occurrences(
               workspace_id, task_id, occurrence_date, origin, status,
               created_at, updated_at, version
             ) VALUES (?1, ?2, ?3, ?4, 'open', ?5, ?5, 1)
             ON CONFLICT(task_id, occurrence_date) DO NOTHING",
            params![workspace_id, task_id, date, origin, now],
        )
        .map_err(|error| error.to_string())?;
    occurrence_for_date(transaction, workspace_id, task_id, date)?
        .ok_or_else(|| "INTERNAL_ERROR: 无法创建重复事项轮次".to_string())
}

pub fn ensure_occurrence_if_recurring(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    task_id: &str,
    date: &str,
    now: i64,
) -> Result<Option<TaskOccurrenceDto>, String> {
    if get_active_rule(transaction, workspace_id, task_id)?.is_none()
        && get_rule_for_date(transaction, workspace_id, task_id, date)?.is_none()
    {
        return Ok(None);
    }
    ensure_occurrence(transaction, workspace_id, task_id, date, now).map(Some)
}

pub fn set_occurrence_status(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    task_id: &str,
    date: &str,
    done: bool,
    expected_version: Option<i64>,
    now: i64,
) -> Result<TaskOccurrenceDto, String> {
    let occurrence = ensure_occurrence(transaction, workspace_id, task_id, date, now)?;
    let target_status = if done { "done" } else { "open" };
    if occurrence.status == target_status {
        return Ok(occurrence);
    }
    if expected_version.is_some_and(|expected| expected != 0 && expected != occurrence.version) {
        return Err("VERSION_CONFLICT: 当日重复事项状态已变化".to_string());
    }
    let changed = transaction
        .execute(
            "UPDATE task_occurrences
             SET status = ?1, completed_at = ?2, updated_at = ?3, version = version + 1
             WHERE workspace_id = ?4 AND task_id = ?5 AND occurrence_date = ?6 AND version = ?7",
            params![
                target_status,
                if done { Some(now) } else { None },
                now,
                workspace_id,
                task_id,
                date,
                occurrence.version
            ],
        )
        .map_err(|error| error.to_string())?;
    if changed == 0 {
        return Err("VERSION_CONFLICT: 当日重复事项状态已变化".to_string());
    }
    occurrence_for_date(transaction, workspace_id, task_id, date)?
        .ok_or_else(|| "INTERNAL_ERROR: 重复事项轮次不存在".to_string())
}

pub fn save_rule(
    database: &Database,
    request: RecurrenceSaveRequest,
) -> Result<RecurrenceRuleDto, String> {
    validate_frequency(&request.frequency, request.weekdays_mask)?;
    let start = parse_date(&request.effective_start)?;
    if start < Local::now().date_naive() {
        return Err("VALIDATION_ERROR: 重复规则开始日期不能早于今天".to_string());
    }
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    ensure_leaf_task(
        &transaction,
        &workspace_id,
        &request.task_id,
        request.task_expected_version,
    )?;
    let current = get_active_rule(&transaction, &workspace_id, &request.task_id)?;
    let now = now_millis();
    let rule_id = if let Some(current) = current {
        if request.rule_expected_version != Some(current.version) {
            return Err("VERSION_CONFLICT: 重复规则已变化".to_string());
        }
        if current.effective_start == request.effective_start
            || parse_date(&current.effective_start)? > Local::now().date_naive()
        {
            let changed = transaction
                .execute(
                    "UPDATE task_recurrence_rules
                     SET frequency = ?1, weekdays_mask = ?2, effective_start = ?3,
                         updated_at = ?4, version = version + 1
                     WHERE id = ?5 AND version = ?6",
                    params![
                        request.frequency,
                        request.weekdays_mask,
                        request.effective_start,
                        now,
                        current.id,
                        current.version
                    ],
                )
                .map_err(|error| error.to_string())?;
            if changed == 0 {
                return Err("VERSION_CONFLICT: 重复规则已变化".to_string());
            }
            current.id
        } else {
            let previous_day = (start - Duration::days(1)).format("%Y-%m-%d").to_string();
            let changed = transaction
                .execute(
                    "UPDATE task_recurrence_rules
                     SET effective_end = ?1, updated_at = ?2, version = version + 1
                     WHERE id = ?3 AND version = ?4 AND effective_end IS NULL",
                    params![previous_day, now, current.id, current.version],
                )
                .map_err(|error| error.to_string())?;
            if changed == 0 {
                return Err("VERSION_CONFLICT: 重复规则已变化".to_string());
            }
            insert_rule(&transaction, &workspace_id, &request, now)?
        }
    } else {
        if request.rule_expected_version.is_some() {
            return Err("VERSION_CONFLICT: 重复规则已关闭".to_string());
        }
        insert_rule(&transaction, &workspace_id, &request, now)?
    };
    touch_task(
        &transaction,
        &workspace_id,
        &request.task_id,
        request.task_expected_version,
        now,
    )?;
    bump_revision(&transaction)?;
    let result = load_rule(&transaction, &rule_id)?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

pub fn close_rule(database: &Database, request: RecurrenceCloseRequest) -> Result<(), String> {
    validate_date(&request.effective_end)?;
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let current = get_active_rule(&transaction, &workspace_id, &request.task_id)?
        .ok_or_else(|| "NOT_FOUND: 事项没有启用重复".to_string())?;
    if current.version != request.rule_expected_version {
        return Err("VERSION_CONFLICT: 重复规则已变化".to_string());
    }
    let now = now_millis();
    if parse_date(&request.effective_end)? < parse_date(&current.effective_start)? {
        let changed = transaction
            .execute(
                "DELETE FROM task_recurrence_rules WHERE id = ?1 AND version = ?2",
                params![current.id, current.version],
            )
            .map_err(|error| error.to_string())?;
        if changed == 0 {
            return Err("VERSION_CONFLICT: 重复规则已变化".to_string());
        }
        touch_task(
            &transaction,
            &workspace_id,
            &request.task_id,
            request.task_expected_version,
            now,
        )?;
        bump_revision(&transaction)?;
        return transaction.commit().map_err(|error| error.to_string());
    }
    let changed = transaction
        .execute(
            "UPDATE task_recurrence_rules
             SET effective_end = ?1, updated_at = ?2, version = version + 1
             WHERE id = ?3 AND version = ?4 AND effective_end IS NULL",
            params![request.effective_end, now, current.id, current.version],
        )
        .map_err(|error| error.to_string())?;
    if changed == 0 {
        return Err("VERSION_CONFLICT: 重复规则已变化".to_string());
    }
    touch_task(
        &transaction,
        &workspace_id,
        &request.task_id,
        request.task_expected_version,
        now,
    )?;
    bump_revision(&transaction)?;
    transaction.commit().map_err(|error| error.to_string())
}

pub fn task_has_active_rule(
    connection: &Connection,
    workspace_id: &str,
    task_id: &str,
) -> Result<bool, String> {
    Ok(get_active_rule(connection, workspace_id, task_id)?.is_some())
}

fn insert_rule(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    request: &RecurrenceSaveRequest,
    now: i64,
) -> Result<String, String> {
    let id = Uuid::now_v7().to_string();
    transaction
        .execute(
            "INSERT INTO task_recurrence_rules(
               id, workspace_id, task_id, frequency, weekdays_mask, effective_start,
               created_at, updated_at, version
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, 1)",
            params![
                id,
                workspace_id,
                request.task_id,
                request.frequency,
                request.weekdays_mask,
                request.effective_start,
                now
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(id)
}

fn ensure_leaf_task(
    connection: &Connection,
    workspace_id: &str,
    task_id: &str,
    expected_version: i64,
) -> Result<(), String> {
    let row: Option<(String, i64, i64)> = connection
        .query_row(
            "SELECT status, version, EXISTS(
               SELECT 1 FROM tasks child
               WHERE child.parent_id = task.id AND child.deleted_at IS NULL
             )
             FROM tasks task
             WHERE task.id = ?1 AND task.workspace_id = ?2 AND task.deleted_at IS NULL",
            params![task_id, workspace_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some((status, version, has_children)) = row else {
        return Err("NOT_FOUND: 事项不存在".to_string());
    };
    if version != expected_version {
        return Err("VERSION_CONFLICT: 事项已变化".to_string());
    }
    if has_children != 0 {
        return Err("TASK_NOT_LEAF: 只有无子事项可以启用重复".to_string());
    }
    if status == "done" {
        return Err("TASK_COMPLETED: 请先将事项恢复为未完成再启用重复".to_string());
    }
    let completed_ancestor: Option<String> = connection
        .query_row(
            "WITH RECURSIVE ancestors(id, parent_id, status) AS (
               SELECT id, parent_id, status FROM tasks WHERE id = ?1
               UNION ALL
               SELECT parent.id, parent.parent_id, parent.status
               FROM tasks parent JOIN ancestors child ON child.parent_id = parent.id
               WHERE parent.deleted_at IS NULL
             )
             SELECT id FROM ancestors WHERE id != ?1 AND status = 'done' LIMIT 1",
            [task_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if completed_ancestor.is_some() {
        return Err("TASK_COMPLETED_ANCESTOR: 请先恢复已完成的上级事项".to_string());
    }
    Ok(())
}

fn touch_task(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    task_id: &str,
    expected_version: i64,
    now: i64,
) -> Result<(), String> {
    let changed = transaction
        .execute(
            "UPDATE tasks SET status = 'open', completed_at = NULL,
                    updated_at = ?1, version = version + 1
             WHERE id = ?2 AND workspace_id = ?3 AND version = ?4 AND deleted_at IS NULL",
            params![now, task_id, workspace_id, expected_version],
        )
        .map_err(|error| error.to_string())?;
    if changed == 0 {
        Err("VERSION_CONFLICT: 事项已变化".to_string())
    } else {
        Ok(())
    }
}

fn load_rule(connection: &Connection, id: &str) -> Result<RecurrenceRuleDto, String> {
    connection
        .query_row(
            "SELECT id, task_id, frequency, weekdays_mask, effective_start, effective_end, version
             FROM task_recurrence_rules WHERE id = ?1",
            [id],
            rule_from_row,
        )
        .map_err(|error| error.to_string())
}

fn rule_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RecurrenceRuleDto> {
    let frequency: String = row.get(2)?;
    let weekdays_mask: Option<i64> = row.get(3)?;
    Ok(RecurrenceRuleDto {
        id: row.get(0)?,
        task_id: row.get(1)?,
        summary: rule_summary(&frequency, weekdays_mask),
        frequency,
        weekdays_mask,
        effective_start: row.get(4)?,
        effective_end: row.get(5)?,
        version: row.get(6)?,
    })
}

fn rule_summary(frequency: &str, weekdays_mask: Option<i64>) -> String {
    match frequency {
        "daily" => "每天".to_string(),
        "weekdays" => "工作日".to_string(),
        "weekly" => {
            let labels = ["一", "二", "三", "四", "五", "六", "日"];
            let selected = labels
                .iter()
                .enumerate()
                .filter_map(|(index, label)| {
                    weekdays_mask
                        .is_some_and(|mask| mask & (1_i64 << index) != 0)
                        .then_some(*label)
                })
                .collect::<Vec<_>>();
            format!("每周{}", selected.join("、"))
        }
        _ => frequency.to_string(),
    }
}

fn validate_frequency(frequency: &str, weekdays_mask: Option<i64>) -> Result<(), String> {
    match frequency {
        "daily" | "weekdays" if weekdays_mask.is_none() => Ok(()),
        "weekly" if weekdays_mask.is_some_and(|mask| (1..=127).contains(&mask)) => Ok(()),
        "weekly" => Err("VALIDATION_ERROR: 每周重复至少选择一个星期".to_string()),
        "daily" | "weekdays" => Err("VALIDATION_ERROR: 当前重复类型不能设置星期".to_string()),
        _ => Err("VALIDATION_ERROR: 不支持的重复频率".to_string()),
    }
}

fn parse_date(value: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| "VALIDATION_ERROR: 日期格式必须为 YYYY-MM-DD".to_string())
}

fn validate_date(value: &str) -> Result<(), String> {
    parse_date(value).map(|_| ())
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

fn bump_revision(connection: &Connection) -> Result<(), String> {
    connection
        .execute(
            "UPDATE app_metadata SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT)
             WHERE key = 'global_revision'",
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
pub fn task_recurrence_save(
    database: tauri::State<'_, Database>,
    request: RecurrenceSaveRequest,
) -> Result<RecurrenceRuleDto, String> {
    crate::supabase::ensure_local_mode(&database)?;
    save_rule(&database, request)
}

#[tauri::command]
pub fn task_recurrence_close(
    database: tauri::State<'_, Database>,
    request: RecurrenceCloseRequest,
) -> Result<(), String> {
    crate::supabase::ensure_local_mode(&database)?;
    close_rule(&database, request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::{
        create_task, list_tasks, set_task_status, TaskCreateRequest, TaskListRequest,
        TaskStatusRequest,
    };
    use crate::time_tracking::{create_manual_entry, ManualEntryRequest};
    use tempfile::tempdir;

    fn setup_task() -> (Database, String, String, i64) {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(directory.path().join("recurring.sqlite3")).unwrap();
        std::mem::forget(directory);
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
                title: "重复事项".to_string(),
                planned_date: None,
                estimate_minutes: Some(20),
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        (database, subject_id, task.id, task.version)
    }

    #[test]
    fn matches_daily_weekdays_and_weekly_rules() {
        let rule = RecurrenceRuleDto {
            id: "r".to_string(),
            task_id: "t".to_string(),
            frequency: "daily".to_string(),
            weekdays_mask: None,
            effective_start: "2026-09-28".to_string(),
            effective_end: Some("2026-10-02".to_string()),
            summary: String::new(),
            version: 1,
        };
        assert!(rule_matches_date(&rule, "2026-09-29").unwrap());
        assert!(!rule_matches_date(&rule, "2026-10-03").unwrap());

        let weekdays = RecurrenceRuleDto {
            frequency: "weekdays".to_string(),
            effective_end: None,
            ..rule.clone()
        };
        assert!(rule_matches_date(&weekdays, "2026-10-02").unwrap());
        assert!(!rule_matches_date(&weekdays, "2026-10-03").unwrap());

        let weekly = RecurrenceRuleDto {
            frequency: "weekly".to_string(),
            weekdays_mask: Some((1 << 0) | (1 << 2) | (1 << 4)),
            effective_end: None,
            ..rule
        };
        assert!(rule_matches_date(&weekly, "2026-09-30").unwrap());
        assert!(!rule_matches_date(&weekly, "2026-10-01").unwrap());
    }

    #[test]
    fn top_level_and_nested_leaf_tasks_can_repeat() {
        for nested in [false, true] {
            let (database, subject_id, parent_task_id, parent_version) = setup_task();
            let (task_id, version) = if nested {
                let child = create_task(
                    &database,
                    TaskCreateRequest {
                        subject_id,
                        parent_id: Some(parent_task_id),
                        title: "嵌套重复事项".to_string(),
                        planned_date: None,
                        estimate_minutes: None,
                        note: None,
                        project_name: None,
                        solution_name: None,
                    },
                )
                .unwrap();
                (child.id, child.version)
            } else {
                (parent_task_id, parent_version)
            };
            let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
            let rule = save_rule(
                &database,
                RecurrenceSaveRequest {
                    task_id,
                    task_expected_version: version,
                    frequency: "daily".to_string(),
                    weekdays_mask: None,
                    effective_start: today,
                    rule_expected_version: None,
                },
            )
            .unwrap();
            assert_eq!(rule.frequency, "daily");
        }
    }

    #[test]
    fn task_with_children_cannot_repeat() {
        let (database, subject_id, task_id, task_version) = setup_task();
        create_task(
            &database,
            TaskCreateRequest {
                subject_id,
                parent_id: Some(task_id.clone()),
                title: "子事项".to_string(),
                planned_date: None,
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
        let error = save_rule(
            &database,
            RecurrenceSaveRequest {
                task_id,
                task_expected_version: task_version,
                frequency: "daily".to_string(),
                weekdays_mask: None,
                effective_start: today,
                rule_expected_version: None,
            },
        )
        .unwrap_err();
        assert!(error.starts_with("TASK_NOT_LEAF:"));
    }

    #[test]
    fn occurrence_creation_is_idempotent_and_non_plan_day_is_manual() {
        let (database, _, task_id, version) = setup_task();
        save_rule(
            &database,
            RecurrenceSaveRequest {
                task_id: task_id.clone(),
                task_expected_version: version,
                frequency: "weekly".to_string(),
                weekdays_mask: Some(1),
                effective_start: "2099-01-01".to_string(),
                rule_expected_version: None,
            },
        )
        .unwrap();
        let mut connection = database.open().unwrap();
        let workspace_id = workspace_id(&connection).unwrap();
        let transaction = connection.transaction().unwrap();
        let first = ensure_occurrence(
            &transaction,
            &workspace_id,
            &task_id,
            "2099-01-02",
            now_millis(),
        )
        .unwrap();
        let second = ensure_occurrence(
            &transaction,
            &workspace_id,
            &task_id,
            "2099-01-02",
            now_millis(),
        )
        .unwrap();
        assert_eq!(first, second);
        assert_eq!(first.origin, "manual");
        let count: i64 = transaction
            .query_row(
                "SELECT COUNT(*) FROM task_occurrences
                 WHERE task_id = ?1 AND occurrence_date = '2099-01-02'",
                [&task_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn daily_occurrences_are_date_scoped_and_time_creates_the_current_occurrence() {
        let (database, subject_id, task_id, task_version) = setup_task();
        let today = Local::now().date_naive();
        let today_text = today.format("%Y-%m-%d").to_string();
        let tomorrow_text = (today + Duration::days(1)).format("%Y-%m-%d").to_string();
        save_rule(
            &database,
            RecurrenceSaveRequest {
                task_id: task_id.clone(),
                task_expected_version: task_version,
                frequency: "daily".to_string(),
                weekdays_mask: None,
                effective_start: today_text.clone(),
                rule_expected_version: None,
            },
        )
        .unwrap();

        let today_tasks = list_tasks(
            &database,
            TaskListRequest {
                subject_id: Some(subject_id.clone()),
                include_completed: Some(true),
                planned_date: None,
                today_date: Some(today_text.clone()),
                today_only: Some(true),
                daily_estimate_date: Some(today_text.clone()),
                query: None,
            },
        )
        .unwrap();
        let today_task = today_tasks
            .tasks
            .iter()
            .find(|task| task.id == task_id)
            .unwrap();
        assert_eq!(today_task.status, "open");
        assert_eq!(
            today_task.occurrence_date.as_deref(),
            Some(today_text.as_str())
        );
        assert_eq!(today_task.occurrence_version, Some(0));

        let completed = set_task_status(
            &database,
            TaskStatusRequest {
                id: task_id.clone(),
                expected_version: today_task.version,
                done: true,
                occurrence_date: Some(today_text.clone()),
                occurrence_expected_version: Some(0),
            },
        )
        .unwrap();
        assert_eq!(completed.status, "done");
        let today_tasks_after_completion = list_tasks(
            &database,
            TaskListRequest {
                subject_id: Some(subject_id.clone()),
                include_completed: Some(true),
                planned_date: None,
                today_date: Some(today_text.clone()),
                today_only: Some(true),
                daily_estimate_date: Some(today_text.clone()),
                query: None,
            },
        )
        .unwrap();
        let today_task_after_completion = today_tasks_after_completion
            .tasks
            .iter()
            .find(|task| task.id == task_id)
            .unwrap();
        assert_eq!(today_task_after_completion.status, "done");
        assert_eq!(
            today_task_after_completion.occurrence_date.as_deref(),
            Some(today_text.as_str())
        );
        let base_status: String = database
            .open()
            .unwrap()
            .query_row(
                "SELECT status FROM tasks WHERE id = ?1",
                [&task_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(base_status, "open");

        let tomorrow_tasks = list_tasks(
            &database,
            TaskListRequest {
                subject_id: Some(subject_id),
                include_completed: Some(true),
                planned_date: None,
                today_date: Some(tomorrow_text.clone()),
                today_only: Some(true),
                daily_estimate_date: Some(tomorrow_text.clone()),
                query: None,
            },
        )
        .unwrap();
        let tomorrow_task = tomorrow_tasks
            .tasks
            .iter()
            .find(|task| task.id == task_id)
            .unwrap();
        assert_eq!(tomorrow_task.status, "open");
        assert_eq!(tomorrow_task.occurrence_version, Some(0));

        create_manual_entry(
            &database,
            ManualEntryRequest {
                task_id: Some(task_id.clone()),
                work_date: tomorrow_text.clone(),
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
        let occurrence = database
            .open()
            .unwrap()
            .query_row(
                "SELECT origin, status FROM task_occurrences WHERE task_id = ?1 AND occurrence_date = ?2",
                params![task_id, tomorrow_text],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .unwrap();
        assert_eq!(occurrence, ("scheduled".to_string(), "open".to_string()));
    }

    #[test]
    fn closing_a_rule_keeps_today_and_stops_future_occurrences() {
        let (database, _, task_id, task_version) = setup_task();
        let today = Local::now().date_naive();
        let today_text = today.format("%Y-%m-%d").to_string();
        let tomorrow_text = (today + Duration::days(1)).format("%Y-%m-%d").to_string();
        let rule = save_rule(
            &database,
            RecurrenceSaveRequest {
                task_id: task_id.clone(),
                task_expected_version: task_version,
                frequency: "daily".to_string(),
                weekdays_mask: None,
                effective_start: today_text.clone(),
                rule_expected_version: None,
            },
        )
        .unwrap();
        close_rule(
            &database,
            RecurrenceCloseRequest {
                task_id: task_id.clone(),
                task_expected_version: task_version + 1,
                effective_end: today_text.clone(),
                rule_expected_version: rule.version,
            },
        )
        .unwrap();
        let connection = database.open().unwrap();
        let workspace_id = workspace_id(&connection).unwrap();
        assert!(
            get_rule_for_date(&connection, &workspace_id, &task_id, &today_text)
                .unwrap()
                .is_some()
        );
        assert!(
            get_rule_for_date(&connection, &workspace_id, &task_id, &tomorrow_text)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn full_recurring_lifecycle_preserves_history_and_skips_untouched_days() {
        let (database, subject_id, task_id, task_version) = setup_task();
        let today = Local::now().date_naive();
        let yesterday_text = (today - Duration::days(1)).format("%Y-%m-%d").to_string();
        let today_text = today.format("%Y-%m-%d").to_string();
        let tomorrow_text = (today + Duration::days(1)).format("%Y-%m-%d").to_string();
        let daily_rule = save_rule(
            &database,
            RecurrenceSaveRequest {
                task_id: task_id.clone(),
                task_expected_version: task_version,
                frequency: "daily".to_string(),
                weekdays_mask: None,
                effective_start: today_text.clone(),
                rule_expected_version: None,
            },
        )
        .unwrap();
        let connection = database.open().unwrap();
        let workspace_id = workspace_id(&connection).unwrap();
        let yesterday_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM task_occurrences WHERE task_id = ?1 AND occurrence_date = ?2",
                params![task_id, yesterday_text],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(yesterday_count, 0);
        drop(connection);

        create_manual_entry(
            &database,
            ManualEntryRequest {
                task_id: Some(task_id.clone()),
                work_date: today_text.clone(),
                started_at: None,
                ended_at: None,
                minutes: Some(18),
                note: None,
                complete_task: false,
                task_expected_version: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        set_task_status(
            &database,
            TaskStatusRequest {
                id: task_id.clone(),
                expected_version: task_version + 1,
                done: true,
                occurrence_date: Some(today_text.clone()),
                occurrence_expected_version: Some(1),
            },
        )
        .unwrap();

        let tomorrow_tasks = list_tasks(
            &database,
            TaskListRequest {
                subject_id: Some(subject_id),
                include_completed: Some(true),
                planned_date: None,
                today_date: Some(tomorrow_text.clone()),
                today_only: Some(true),
                daily_estimate_date: Some(tomorrow_text.clone()),
                query: None,
            },
        )
        .unwrap();
        assert_eq!(
            tomorrow_tasks
                .tasks
                .iter()
                .find(|task| task.id == task_id)
                .map(|task| task.status.as_str()),
            Some("open")
        );

        let tomorrow_weekday = (today + Duration::days(1)).weekday().num_days_from_monday();
        let weekly_rule = save_rule(
            &database,
            RecurrenceSaveRequest {
                task_id: task_id.clone(),
                task_expected_version: task_version + 1,
                frequency: "weekly".to_string(),
                weekdays_mask: Some(1_i64 << tomorrow_weekday),
                effective_start: tomorrow_text.clone(),
                rule_expected_version: Some(daily_rule.version),
            },
        )
        .unwrap();
        close_rule(
            &database,
            RecurrenceCloseRequest {
                task_id: task_id.clone(),
                task_expected_version: task_version + 2,
                effective_end: tomorrow_text,
                rule_expected_version: weekly_rule.version,
            },
        )
        .unwrap();

        let connection = database.open().unwrap();
        let historical_rule = get_rule_for_date(&connection, &workspace_id, &task_id, &today_text)
            .unwrap()
            .unwrap();
        assert_eq!(historical_rule.frequency, "daily");
        let today_occurrence =
            occurrence_for_date(&connection, &workspace_id, &task_id, &today_text)
                .unwrap()
                .unwrap();
        assert_eq!(today_occurrence.status, "done");
        let actual_minutes: i64 = connection
            .query_row(
                "SELECT COALESCE(SUM(a.minutes), 0)
                 FROM time_allocations a JOIN time_entries e ON e.id = a.entry_id
                 WHERE a.task_id = ?1 AND e.work_date = ?2",
                params![task_id, today_text],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(actual_minutes, 18);
        assert!(get_active_rule(&connection, &workspace_id, &task_id)
            .unwrap()
            .is_none());
    }
}
