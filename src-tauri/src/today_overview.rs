use std::collections::{HashMap, HashSet};

use chrono::{Duration, NaiveDate};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::database::Database;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodayWorkOverviewRequest {
    pub subject_id: Option<String>,
    pub today_date: String,
    pub history_days: Option<i64>,
    pub trend_days: Option<i64>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TodayWorkOverviewScopeDto {
    pub subject_id: Option<String>,
    pub subject_name: String,
    pub today_date: String,
}

#[derive(Debug, Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct TodayWorkOverviewSummaryDto {
    pub actual_minutes: i64,
    pub estimated_minutes: i64,
    pub completed_count: i64,
    pub active_count: i64,
    pub not_started_count: i64,
    pub unassigned_minutes: i64,
    pub completion_rate: f64,
}

#[derive(Debug, Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct TodayWorkOverviewDayDto {
    pub date: String,
    pub actual_minutes: i64,
    pub estimated_minutes: i64,
    pub completed_count: i64,
    pub total_count: i64,
    pub unassigned_minutes: i64,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TodayWorkOverviewTimelineItemDto {
    pub id: String,
    pub item_type: String,
    pub task_id: Option<String>,
    pub task_title: String,
    pub subject_id: Option<String>,
    pub subject_name: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub minutes: i64,
    pub state: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TodayWorkOverviewTaskDto {
    pub task_id: String,
    pub title: String,
    pub path_label: String,
    pub subject_id: String,
    pub subject_name: String,
    pub status: String,
    pub actual_minutes: i64,
    pub today_estimate_minutes: Option<i64>,
    pub estimate_minutes: Option<i64>,
    pub last_entry_at: Option<i64>,
    pub selectable: bool,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TodayWorkOverviewDto {
    pub scope: TodayWorkOverviewScopeDto,
    pub summary: TodayWorkOverviewSummaryDto,
    pub days: Vec<TodayWorkOverviewDayDto>,
    pub timeline: Vec<TodayWorkOverviewTimelineItemDto>,
    pub tasks: Vec<TodayWorkOverviewTaskDto>,
    pub revision: i64,
}

#[derive(Debug, Clone)]
struct TaskRow {
    id: String,
    subject_id: String,
    subject_name: String,
    parent_id: Option<String>,
    title: String,
    status: String,
    planned_date: Option<String>,
    estimate_minutes: Option<i64>,
    sort_order: i64,
    created_at: i64,
    occurrence_status: Option<String>,
    occurrence_date: Option<String>,
}

#[derive(Debug, Default, Clone)]
struct TaskDayStats {
    actual_minutes: i64,
    last_entry_at: Option<i64>,
}

pub fn get_today_work_overview(
    database: &Database,
    request: TodayWorkOverviewRequest,
) -> Result<TodayWorkOverviewDto, String> {
    validate_date(&request.today_date)?;
    let history_days = request.history_days.unwrap_or(30).clamp(1, 90);
    let _trend_days = request.trend_days.unwrap_or(14).clamp(1, history_days);
    let today = NaiveDate::parse_from_str(&request.today_date, "%Y-%m-%d")
        .map_err(|error| error.to_string())?;
    let start_date = today - Duration::days(history_days - 1);
    let start_text = start_date.format("%Y-%m-%d").to_string();
    let connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let subject = resolve_subject(&connection, &workspace_id, request.subject_id.as_deref())?;
    let scope = TodayWorkOverviewScopeDto {
        subject_id: subject.clone(),
        subject_name: subject
            .as_ref()
            .map(|id| subject_name(&connection, &workspace_id, id))
            .transpose()?
            .unwrap_or_else(|| "全部主体".to_string()),
        today_date: request.today_date.clone(),
    };

    let tasks = load_tasks(
        &connection,
        &workspace_id,
        subject.as_deref(),
        &request.today_date,
    )?;
    let children = build_children(&tasks);
    let path_labels = build_path_labels(&tasks);
    let task_stats = load_task_day_stats(
        &connection,
        &workspace_id,
        subject.as_deref(),
        &start_text,
        &request.today_date,
    )?;
    let unassigned_by_date =
        load_unassigned_minutes(&connection, &workspace_id, &start_text, &request.today_date)?;
    let mut days = date_range(start_date, today)
        .into_iter()
        .map(|date| TodayWorkOverviewDayDto {
            date,
            ..Default::default()
        })
        .collect::<Vec<_>>();

    let today_tasks = today_task_ids(&tasks, &task_stats, &request.today_date);
    let mut today_summary = TodayWorkOverviewSummaryDto::default();
    let mut overview_tasks = Vec::new();

    for day in &mut days {
        day.unassigned_minutes = unassigned_by_date.get(&day.date).copied().unwrap_or(0);
        let day_task_ids = task_ids_for_day(&tasks, &task_stats, &day.date);
        for task_id in &day_task_ids {
            let Some(task) = tasks.iter().find(|item| &item.id == task_id) else {
                continue;
            };
            let stats = task_stats.get(&(task.id.clone(), day.date.clone()));
            day.actual_minutes += stats.map(|item| item.actual_minutes).unwrap_or(0);
            day.estimated_minutes += estimate_for_day(&connection, &workspace_id, task, &day.date)?;
            day.total_count += 1;
            if effective_status(task, &day.date) == "done" {
                day.completed_count += 1;
            }
        }
    }

    for task_id in &today_tasks {
        let Some(task) = tasks.iter().find(|item| &item.id == task_id) else {
            continue;
        };
        let stats = task_stats.get(&(task.id.clone(), request.today_date.clone()));
        let actual_minutes = stats.map(|item| item.actual_minutes).unwrap_or(0);
        let today_estimate =
            daily_estimate(&connection, &workspace_id, &task.id, &request.today_date)?;
        let status = task_execution_status(task, actual_minutes, &request.today_date);
        today_summary.actual_minutes += actual_minutes;
        today_summary.estimated_minutes += today_estimate.or(task.estimate_minutes).unwrap_or(0);
        match status.as_str() {
            "done" => today_summary.completed_count += 1,
            "active" | "started" => today_summary.active_count += 1,
            _ => today_summary.not_started_count += 1,
        }
        overview_tasks.push(TodayWorkOverviewTaskDto {
            task_id: task.id.clone(),
            title: task.title.clone(),
            path_label: path_labels
                .get(&task.id)
                .cloned()
                .unwrap_or_else(|| task.title.clone()),
            subject_id: task.subject_id.clone(),
            subject_name: task.subject_name.clone(),
            status,
            actual_minutes,
            today_estimate_minutes: today_estimate,
            estimate_minutes: task.estimate_minutes,
            last_entry_at: stats.and_then(|item| item.last_entry_at),
            selectable: !children
                .get(&Some(task.id.clone()))
                .is_some_and(|items| !items.is_empty()),
        });
    }
    today_summary.unassigned_minutes = unassigned_by_date
        .get(&request.today_date)
        .copied()
        .unwrap_or(0);
    let total_count = today_summary.completed_count
        + today_summary.active_count
        + today_summary.not_started_count;
    today_summary.completion_rate = if total_count == 0 {
        0.0
    } else {
        today_summary.completed_count as f64 / total_count as f64
    };

    overview_tasks.sort_by_key(|task| {
        tasks
            .iter()
            .find(|item| item.id == task.task_id)
            .map(|item| (item.sort_order, item.created_at, item.id.clone()))
            .unwrap_or_default()
    });

    let timeline = load_timeline(
        &connection,
        &workspace_id,
        subject.as_deref(),
        &request.today_date,
    )?;
    let revision = current_revision(&connection)?;

    Ok(TodayWorkOverviewDto {
        scope,
        summary: today_summary,
        days,
        timeline,
        tasks: overview_tasks,
        revision,
    })
}

fn load_tasks(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    subject_id: Option<&str>,
    today_date: &str,
) -> Result<Vec<TaskRow>, String> {
    let mut statement = connection
        .prepare(
            "SELECT t.id, t.subject_id, s.name, t.parent_id, t.title, t.status, t.planned_date,
                    t.estimate_minutes, t.sort_order, t.created_at,
                    o.status, o.occurrence_date
             FROM tasks t
             JOIN subjects s ON s.id = t.subject_id
             LEFT JOIN task_occurrences o
               ON o.task_id = t.id AND o.workspace_id = t.workspace_id AND o.occurrence_date = ?3
             WHERE t.workspace_id = ?1 AND t.deleted_at IS NULL
               AND (?2 IS NULL OR t.subject_id = ?2)
             ORDER BY t.subject_id, t.sort_order, t.created_at",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![workspace_id, subject_id, today_date], |row| {
            Ok(TaskRow {
                id: row.get(0)?,
                subject_id: row.get(1)?,
                subject_name: row.get(2)?,
                parent_id: row.get(3)?,
                title: row.get(4)?,
                status: row.get(5)?,
                planned_date: row.get(6)?,
                estimate_minutes: row.get(7)?,
                sort_order: row.get(8)?,
                created_at: row.get(9)?,
                occurrence_status: row.get(10)?,
                occurrence_date: row.get(11)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(rows)
}

fn load_task_day_stats(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    subject_id: Option<&str>,
    start_date: &str,
    end_date: &str,
) -> Result<HashMap<(String, String), TaskDayStats>, String> {
    let mut statement = connection
        .prepare(
            "SELECT a.task_id, e.work_date, COALESCE(SUM(a.minutes), 0), MAX(e.started_at)
             FROM time_allocations a
             JOIN time_entries e ON e.id = a.entry_id
             JOIN tasks t ON t.id = a.task_id
             WHERE a.workspace_id = ?1 AND e.deleted_at IS NULL AND t.deleted_at IS NULL
               AND e.work_date BETWEEN ?2 AND ?3
               AND (?4 IS NULL OR t.subject_id = ?4)
             GROUP BY a.task_id, e.work_date",
        )
        .map_err(|error| error.to_string())?;
    let mut result = HashMap::new();
    for row in statement
        .query_map(
            params![workspace_id, start_date, end_date, subject_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                ))
            },
        )
        .map_err(|error| error.to_string())?
    {
        let (task_id, date, actual_minutes, last_entry_at) =
            row.map_err(|error| error.to_string())?;
        result.insert(
            (task_id, date),
            TaskDayStats {
                actual_minutes,
                last_entry_at,
            },
        );
    }
    Ok(result)
}

fn load_unassigned_minutes(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    start_date: &str,
    end_date: &str,
) -> Result<HashMap<String, i64>, String> {
    let mut statement = connection
        .prepare(
            "SELECT work_date, COALESCE(SUM(duration_seconds), 0)
             FROM unassigned_sessions
             WHERE workspace_id = ?1 AND work_date BETWEEN ?2 AND ?3
               AND state IN ('collecting', 'awaiting_resolution')
             GROUP BY work_date",
        )
        .map_err(|error| error.to_string())?;
    let mut result = HashMap::new();
    for row in statement
        .query_map(params![workspace_id, start_date, end_date], |row| {
            Ok((
                row.get::<_, String>(0)?,
                settlement_minutes(row.get::<_, i64>(1)?),
            ))
        })
        .map_err(|error| error.to_string())?
    {
        let (date, minutes) = row.map_err(|error| error.to_string())?;
        result.insert(date, minutes);
    }
    Ok(result)
}

fn load_timeline(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    subject_id: Option<&str>,
    today_date: &str,
) -> Result<Vec<TodayWorkOverviewTimelineItemDto>, String> {
    let current_time = now_millis();
    let mut timeline = Vec::new();
    let mut statement = connection
        .prepare(
            "SELECT e.id, e.kind, e.default_task_id, e.label_snapshot, e.started_at, e.ended_at,
                    e.duration_seconds, e.state, t.subject_id, s.name
             FROM time_entries e
             LEFT JOIN tasks t ON t.id = e.default_task_id
             LEFT JOIN subjects s ON s.id = t.subject_id
             WHERE e.workspace_id = ?1 AND e.work_date = ?2 AND e.deleted_at IS NULL
               AND (?3 IS NULL OR t.subject_id = ?3)
             ORDER BY e.started_at, e.id",
        )
        .map_err(|error| error.to_string())?;
    for row in statement
        .query_map(params![workspace_id, today_date, subject_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, Option<i64>>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, Option<String>>(9)?,
            ))
        })
        .map_err(|error| error.to_string())?
    {
        let (
            id,
            kind,
            task_id,
            label,
            started_at,
            ended_at,
            mut duration_seconds,
            state,
            row_subject_id,
            subject_name,
        ) = row.map_err(|error| error.to_string())?;
        if state == "running" {
            duration_seconds += (current_time - started_at).max(0) / 1_000;
        }
        timeline.push(TodayWorkOverviewTimelineItemDto {
            id,
            item_type: kind,
            task_id,
            task_title: label,
            subject_id: row_subject_id,
            subject_name,
            started_at,
            ended_at,
            minutes: settlement_minutes(duration_seconds),
            state: Some(state),
        });
    }
    if subject_id.is_none() {
        append_unassigned_timeline(connection, workspace_id, today_date, &mut timeline)?;
    }
    timeline.sort_by_key(|item| (item.started_at, item.id.clone()));
    Ok(timeline)
}

fn append_unassigned_timeline(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    today_date: &str,
    timeline: &mut Vec<TodayWorkOverviewTimelineItemDto>,
) -> Result<(), String> {
    let current_time = now_millis();
    let mut statement = connection
        .prepare(
            "SELECT id, first_started_at, last_ended_at, duration_seconds, state
             FROM unassigned_sessions
             WHERE workspace_id = ?1 AND work_date = ?2 AND state IN ('collecting', 'awaiting_resolution')",
        )
        .map_err(|error| error.to_string())?;
    for row in statement
        .query_map(params![workspace_id, today_date], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|error| error.to_string())?
    {
        let (id, started_at, ended_at, duration_seconds, state) =
            row.map_err(|error| error.to_string())?;
        let live_seconds = if state == "collecting" && ended_at.is_none() {
            (current_time - started_at).max(0) / 1_000
        } else {
            0
        };
        timeline.push(TodayWorkOverviewTimelineItemDto {
            id,
            item_type: "unassigned".to_string(),
            task_id: None,
            task_title: "未归属时间".to_string(),
            subject_id: None,
            subject_name: None,
            started_at,
            ended_at,
            minutes: settlement_minutes(duration_seconds + live_seconds),
            state: Some(state),
        });
    }
    Ok(())
}

fn today_task_ids(
    tasks: &[TaskRow],
    stats: &HashMap<(String, String), TaskDayStats>,
    today_date: &str,
) -> HashSet<String> {
    task_ids_for_day(tasks, stats, today_date)
}

fn task_ids_for_day(
    tasks: &[TaskRow],
    stats: &HashMap<(String, String), TaskDayStats>,
    date: &str,
) -> HashSet<String> {
    let mut ids = HashSet::new();
    for task in tasks {
        if stats.contains_key(&(task.id.clone(), date.to_string())) {
            ids.insert(task.id.clone());
            continue;
        }
        if task.occurrence_date.as_deref() == Some(date)
            || task
                .planned_date
                .as_deref()
                .is_some_and(|planned| planned <= date)
            || effective_status(task, date) == "done"
        {
            ids.insert(task.id.clone());
        }
    }
    ids
}

fn task_execution_status(task: &TaskRow, actual_minutes: i64, date: &str) -> String {
    if effective_status(task, date) == "done" {
        "done".to_string()
    } else if actual_minutes > 0 {
        "started".to_string()
    } else {
        "not_started".to_string()
    }
}

fn effective_status(task: &TaskRow, date: &str) -> String {
    if task.occurrence_date.as_deref() == Some(date) {
        task.occurrence_status
            .clone()
            .unwrap_or_else(|| task.status.clone())
    } else {
        task.status.clone()
    }
}

fn estimate_for_day(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    task: &TaskRow,
    date: &str,
) -> Result<i64, String> {
    Ok(daily_estimate(connection, workspace_id, &task.id, date)?
        .or(task.estimate_minutes)
        .unwrap_or(0))
}

fn daily_estimate(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    task_id: &str,
    date: &str,
) -> Result<Option<i64>, String> {
    connection
        .query_row(
            "SELECT estimate_minutes FROM task_daily_estimates
             WHERE workspace_id = ?1 AND task_id = ?2 AND work_date = ?3",
            params![workspace_id, task_id, date],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())
}

fn build_children(tasks: &[TaskRow]) -> HashMap<Option<String>, Vec<String>> {
    let ids = tasks
        .iter()
        .map(|task| task.id.clone())
        .collect::<HashSet<_>>();
    let mut children = HashMap::<Option<String>, Vec<String>>::new();
    for task in tasks {
        let parent = task
            .parent_id
            .as_ref()
            .filter(|id| ids.contains(*id))
            .cloned();
        children.entry(parent).or_default().push(task.id.clone());
    }
    children
}

fn build_path_labels(tasks: &[TaskRow]) -> HashMap<String, String> {
    let by_id = tasks
        .iter()
        .map(|task| (task.id.clone(), task))
        .collect::<HashMap<_, _>>();
    let mut result = HashMap::new();
    for task in tasks {
        let mut titles = Vec::new();
        let mut current = Some(task.id.clone());
        let mut visited = HashSet::new();
        while let Some(id) = current {
            if !visited.insert(id.clone()) {
                break;
            }
            let Some(item) = by_id.get(&id) else { break };
            titles.push(item.title.clone());
            current = item.parent_id.clone();
        }
        titles.reverse();
        result.insert(task.id.clone(), titles.join(" / "));
    }
    result
}

fn date_range(start: NaiveDate, end: NaiveDate) -> Vec<String> {
    let mut current = start;
    let mut result = Vec::new();
    while current <= end {
        result.push(current.format("%Y-%m-%d").to_string());
        current += Duration::days(1);
    }
    result
}

fn resolve_subject(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    subject_id: Option<&str>,
) -> Result<Option<String>, String> {
    let Some(subject_id) = subject_id.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };
    subject_name(connection, workspace_id, subject_id)?;
    Ok(Some(subject_id.to_string()))
}

fn subject_name(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    subject_id: &str,
) -> Result<String, String> {
    connection
        .query_row(
            "SELECT name FROM subjects WHERE id = ?1 AND workspace_id = ?2 AND deleted_at IS NULL",
            params![subject_id, workspace_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "NOT_FOUND: 主体不存在".to_string())
}

fn validate_date(value: &str) -> Result<(), String> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
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

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
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

fn current_revision(connection: &rusqlite::Connection) -> Result<i64, String> {
    connection
        .query_row(
            "SELECT CAST(value AS INTEGER) FROM app_metadata WHERE key = 'global_revision'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn today_work_overview_get(
    database: tauri::State<'_, Database>,
    request: TodayWorkOverviewRequest,
) -> Result<TodayWorkOverviewDto, String> {
    get_today_work_overview(&database, request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::{create_task, TaskCreateRequest};
    use uuid::Uuid;

    fn setup() -> (Database, String, String) {
        let directory = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("today-overview-tests");
        std::fs::create_dir_all(&directory).unwrap();
        let database =
            Database::initialize_at(directory.join(format!("{}.sqlite3", Uuid::now_v7()))).unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let subject_id: String = connection
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        (database, workspace_id, subject_id)
    }

    #[test]
    fn overview_filters_subject_and_summarizes_days() {
        let (database, workspace_id, subject_id) = setup();
        let other_subject = Uuid::now_v7().to_string();
        let now = 1_800_000_000_000_i64;
        let connection = database.open().unwrap();
        connection
            .execute(
                "INSERT INTO subjects(id, workspace_id, name, sort_order, created_at, updated_at, version)
                 VALUES (?1, ?2, '其他', 20, ?3, ?3, 1)",
                params![other_subject, workspace_id, now],
            )
            .unwrap();
        drop(connection);
        let first = create_task(
            &database,
            TaskCreateRequest {
                subject_id: subject_id.clone(),
                parent_id: None,
                title: "主体事项".to_string(),
                planned_date: Some("2026-09-29".to_string()),
                estimate_minutes: Some(60),
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        let second = create_task(
            &database,
            TaskCreateRequest {
                subject_id: other_subject.clone(),
                parent_id: None,
                title: "其他事项".to_string(),
                planned_date: Some("2026-09-29".to_string()),
                estimate_minutes: Some(30),
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        let connection = database.open().unwrap();
        insert_entry(&connection, &workspace_id, &first.id, "2026-09-29", 45, now);
        insert_entry(
            &connection,
            &workspace_id,
            &second.id,
            "2026-09-29",
            25,
            now + 1_000,
        );
        insert_unassigned(&connection, &workspace_id, "2026-09-29", 600, now + 2_000);
        drop(connection);

        let all = get_today_work_overview(&database, request(None)).unwrap();
        assert_eq!(all.summary.actual_minutes, 70);
        assert_eq!(all.summary.unassigned_minutes, 10);
        assert_eq!(all.tasks.len(), 2);

        let scoped = get_today_work_overview(&database, request(Some(subject_id))).unwrap();
        assert_eq!(scoped.summary.actual_minutes, 45);
        assert_eq!(scoped.summary.estimated_minutes, 60);
        assert_eq!(scoped.tasks.len(), 1);
        assert_eq!(scoped.tasks[0].task_id, first.id);
    }

    #[test]
    fn overview_timeline_orders_work_break_unassigned_and_running() {
        let (database, workspace_id, subject_id) = setup();
        let task = create_task(
            &database,
            TaskCreateRequest {
                subject_id,
                parent_id: None,
                title: "时间轴事项".to_string(),
                planned_date: Some("2026-09-29".to_string()),
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        let now = 1_800_000_000_000_i64;
        let connection = database.open().unwrap();
        insert_entry(&connection, &workspace_id, &task.id, "2026-09-29", 30, now);
        insert_break(&connection, &workspace_id, "2026-09-29", 12, now + 1_000);
        insert_running_entry(
            &connection,
            &workspace_id,
            &task.id,
            "2026-09-29",
            now + 2_000,
        );
        insert_unassigned(&connection, &workspace_id, "2026-09-29", 300, now + 3_000);
        drop(connection);

        let overview = get_today_work_overview(&database, request(None)).unwrap();
        assert!(overview
            .timeline
            .iter()
            .any(|item| item.item_type == "work"));
        assert!(overview
            .timeline
            .iter()
            .any(|item| item.item_type == "break"));
        assert!(overview
            .timeline
            .iter()
            .any(|item| item.item_type == "unassigned"));
        assert!(overview
            .timeline
            .iter()
            .any(|item| item.state.as_deref() == Some("running")));
        let starts = overview
            .timeline
            .iter()
            .map(|item| item.started_at)
            .collect::<Vec<_>>();
        let mut sorted = starts.clone();
        sorted.sort();
        assert_eq!(starts, sorted);
    }

    #[test]
    fn overview_task_summary_keeps_done_task_available_after_more_time() {
        let (database, workspace_id, subject_id) = setup();
        let task = create_task(
            &database,
            TaskCreateRequest {
                subject_id,
                parent_id: None,
                title: "已完成后继续".to_string(),
                planned_date: Some("2026-09-29".to_string()),
                estimate_minutes: Some(20),
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        let now = 1_800_000_000_000_i64;
        let connection = database.open().unwrap();
        connection
            .execute(
                "UPDATE tasks SET status = 'done', completed_at = ?1 WHERE id = ?2",
                params![now, task.id],
            )
            .unwrap();
        insert_entry(
            &connection,
            &workspace_id,
            &task.id,
            "2026-09-29",
            15,
            now + 1_000,
        );
        drop(connection);

        let overview = get_today_work_overview(&database, request(None)).unwrap();
        let item = overview
            .tasks
            .iter()
            .find(|item| item.task_id == task.id)
            .unwrap();
        assert_eq!(item.status, "done");
        assert_eq!(item.actual_minutes, 15);
        assert!(item.selectable);
    }

    fn request(subject_id: Option<String>) -> TodayWorkOverviewRequest {
        TodayWorkOverviewRequest {
            subject_id,
            today_date: "2026-09-29".to_string(),
            history_days: Some(30),
            trend_days: Some(14),
        }
    }

    fn insert_entry(
        connection: &rusqlite::Connection,
        workspace_id: &str,
        task_id: &str,
        work_date: &str,
        minutes: i64,
        started_at: i64,
    ) {
        let entry_id = Uuid::now_v7().to_string();
        connection
            .execute(
                "INSERT INTO time_entries(id, workspace_id, work_date, kind, source_type, state, default_task_id, label_snapshot, started_at, ended_at, duration_seconds, created_at, updated_at, version)
                 VALUES (?1, ?2, ?3, 'work', 'manual', 'ended', ?4, '事项', ?5, ?6, ?7, ?5, ?5, 1)",
                params![entry_id, workspace_id, work_date, task_id, started_at, started_at + minutes * 60_000, minutes * 60],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO time_allocations(id, workspace_id, entry_id, task_id, minutes, created_at, updated_at, version)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, 1)",
                params![Uuid::now_v7().to_string(), workspace_id, entry_id, task_id, minutes, started_at],
            )
            .unwrap();
    }

    fn insert_break(
        connection: &rusqlite::Connection,
        workspace_id: &str,
        work_date: &str,
        minutes: i64,
        started_at: i64,
    ) {
        connection
            .execute(
                "INSERT INTO time_entries(id, workspace_id, work_date, kind, source_type, state, label_snapshot, started_at, ended_at, duration_seconds, created_at, updated_at, version)
                 VALUES (?1, ?2, ?3, 'break', 'manual', 'ended', '休息', ?4, ?5, ?6, ?4, ?4, 1)",
                params![Uuid::now_v7().to_string(), workspace_id, work_date, started_at, started_at + minutes * 60_000, minutes * 60],
            )
            .unwrap();
    }

    fn insert_running_entry(
        connection: &rusqlite::Connection,
        workspace_id: &str,
        task_id: &str,
        work_date: &str,
        started_at: i64,
    ) {
        connection
            .execute(
                "INSERT INTO time_entries(id, workspace_id, work_date, kind, source_type, state, default_task_id, label_snapshot, started_at, duration_seconds, created_at, updated_at, version)
                 VALUES (?1, ?2, ?3, 'work', 'timer', 'running', ?4, '运行中', ?5, 0, ?5, ?5, 1)",
                params![Uuid::now_v7().to_string(), workspace_id, work_date, task_id, started_at],
            )
            .unwrap();
    }

    fn insert_unassigned(
        connection: &rusqlite::Connection,
        workspace_id: &str,
        work_date: &str,
        seconds: i64,
        started_at: i64,
    ) {
        connection
            .execute(
                "INSERT INTO unassigned_sessions(id, workspace_id, work_date, state, threshold_seconds, duration_seconds, first_started_at, created_at, updated_at, version)
                 VALUES (?1, ?2, ?3, 'awaiting_resolution', 300, ?4, ?5, ?5, ?5, 1)",
                params![Uuid::now_v7().to_string(), workspace_id, work_date, seconds, started_at],
            )
            .unwrap();
    }
}
