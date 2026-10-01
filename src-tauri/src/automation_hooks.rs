use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::{SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use serde_json::json;
use serde_json::Value;
use url::Url;
use uuid::Uuid;

use crate::database::Database;
use crate::tasks::TaskDto;
use crate::time_tracking::TimeEntryDto;

const OUTPUT_LIMIT_BYTES: usize = 8 * 1024;
const RUN_RETENTION_MILLIS: i64 = 30 * 24 * 60 * 60 * 1000;
const RUN_RETENTION_COUNT: i64 = 100;
const SUPPORTED_EVENTS: &[&str] = &["timer.started", "timer.stopped", "task.completed"];
pub const TEMPLATE_VARIABLES: &[&str] = &[
    "event.type",
    "event.id",
    "task.id",
    "task.title",
    "task.path",
    "task.occurrenceDate",
    "subject.id",
    "subject.name",
    "timer.id",
    "timer.durationSeconds",
    "timer.durationMinutes",
    "timer.pendingMinutes",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookRuleDto {
    pub id: String,
    pub name: String,
    pub event_type: String,
    pub action_type: String,
    pub action_config: Value,
    pub timeout_seconds: i64,
    pub enabled: bool,
    pub sort_order: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub recent_run: Option<HookRunDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookDraftRequest {
    pub name: String,
    pub event_type: String,
    pub action_type: String,
    pub action_config: Value,
    pub timeout_seconds: i64,
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookUpdateRequest {
    pub id: String,
    #[serde(flatten)]
    pub draft: HookDraftRequest,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookEnabledRequest {
    pub id: String,
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookReorderRequest {
    pub hook_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookIdRequest {
    pub id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookRunListRequest {
    pub hook_id: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookRunDto {
    pub id: String,
    pub hook_id: Option<String>,
    pub hook_name_snapshot: String,
    pub event_id: String,
    pub event_type: String,
    pub is_test: bool,
    pub status: String,
    pub exit_code: Option<i64>,
    pub duration_ms: Option<i64>,
    pub stdout_tail: Option<String>,
    pub stderr_tail: Option<String>,
    pub error_message: Option<String>,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookTemplatePreview {
    pub rendered: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookEvent {
    pub schema_version: u32,
    pub event: String,
    pub event_id: String,
    pub occurred_at: String,
    pub test: bool,
    pub device: HookDeviceSnapshot,
    pub subject: Option<HookSubjectSnapshot>,
    pub task: Option<HookTaskSnapshot>,
    pub task_snapshot: Option<HookTaskNameSnapshot>,
    pub timer: Option<HookTimerSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookDeviceSnapshot {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookSubjectSnapshot {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookTaskSnapshot {
    pub id: String,
    pub title: String,
    pub path: String,
    pub occurrence_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookTaskNameSnapshot {
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookTimerSnapshot {
    pub id: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub duration_seconds: i64,
    pub duration_minutes: i64,
    pub settlement_minutes: i64,
    pub allocated_minutes: i64,
    pub pending_minutes: i64,
}

#[derive(Debug, Clone)]
struct ExecutionResult {
    status: String,
    exit_code: Option<i64>,
    duration_ms: i64,
    stdout_tail: Option<String>,
    stderr_tail: Option<String>,
    error_message: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UriActionConfig {
    uri_template: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProcessActionConfig {
    program: String,
    #[serde(default)]
    args: Vec<String>,
    working_directory: Option<String>,
}

pub fn list_hooks(database: &Database) -> Result<Vec<HookRuleDto>, String> {
    let connection = database.open()?;
    let mut statement = connection
        .prepare(
            "SELECT id, name, event_type, action_type, action_config_json, timeout_seconds,
                    enabled, sort_order, created_at, updated_at
             FROM device_hooks ORDER BY sort_order, created_at",
        )
        .map_err(|error| error.to_string())?;
    let hooks = statement
        .query_map([], |row| hook_from_row(&connection, row))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(hooks)
}

pub fn create_hook(database: &Database, draft: HookDraftRequest) -> Result<HookRuleDto, String> {
    validate_draft(&draft)?;
    let connection = database.open()?;
    let now = now_millis();
    let id = Uuid::now_v7().to_string();
    let sort_order: i64 = connection
        .query_row(
            "SELECT COALESCE(MAX(sort_order), 0) + 10 FROM device_hooks",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "INSERT INTO device_hooks(
               id, name, event_type, action_type, action_config_json, timeout_seconds,
               enabled, sort_order, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
            params![
                id,
                draft.name.trim(),
                draft.event_type,
                draft.action_type,
                serde_json::to_string(&draft.action_config).map_err(|error| error.to_string())?,
                draft.timeout_seconds,
                draft.enabled,
                sort_order,
                now
            ],
        )
        .map_err(|error| error.to_string())?;
    load_hook(&connection, &id)
}

pub fn update_hook(database: &Database, request: HookUpdateRequest) -> Result<HookRuleDto, String> {
    validate_draft(&request.draft)?;
    let connection = database.open()?;
    let changed = connection
        .execute(
            "UPDATE device_hooks SET name = ?1, event_type = ?2, action_type = ?3,
                    action_config_json = ?4, timeout_seconds = ?5, enabled = ?6, updated_at = ?7
             WHERE id = ?8",
            params![
                request.draft.name.trim(),
                request.draft.event_type,
                request.draft.action_type,
                serde_json::to_string(&request.draft.action_config)
                    .map_err(|error| error.to_string())?,
                request.draft.timeout_seconds,
                request.draft.enabled,
                now_millis(),
                request.id
            ],
        )
        .map_err(|error| error.to_string())?;
    if changed == 0 {
        return Err("NOT_FOUND: Hook 不存在".to_string());
    }
    load_hook(&connection, &request.id)
}

pub fn set_hook_enabled(
    database: &Database,
    request: HookEnabledRequest,
) -> Result<HookRuleDto, String> {
    let connection = database.open()?;
    let changed = connection
        .execute(
            "UPDATE device_hooks SET enabled = ?1, updated_at = ?2 WHERE id = ?3",
            params![request.enabled, now_millis(), request.id],
        )
        .map_err(|error| error.to_string())?;
    if changed == 0 {
        return Err("NOT_FOUND: Hook 不存在".to_string());
    }
    load_hook(&connection, &request.id)
}

pub fn reorder_hooks(
    database: &Database,
    request: HookReorderRequest,
) -> Result<Vec<HookRuleDto>, String> {
    let mut connection = database.open()?;
    let total: i64 = connection
        .query_row("SELECT COUNT(*) FROM device_hooks", [], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    if request.hook_ids.len() as i64 != total {
        return Err("VALIDATION_ERROR: 排序列表必须包含全部 Hook".to_string());
    }
    let mut unique = request.hook_ids.clone();
    unique.sort();
    unique.dedup();
    if unique.len() != request.hook_ids.len() {
        return Err("VALIDATION_ERROR: 排序列表包含重复 Hook".to_string());
    }
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    for (index, id) in request.hook_ids.iter().enumerate() {
        let changed = transaction
            .execute(
                "UPDATE device_hooks SET sort_order = ?1, updated_at = ?2 WHERE id = ?3",
                params![(index as i64 + 1) * 10, now_millis(), id],
            )
            .map_err(|error| error.to_string())?;
        if changed == 0 {
            return Err("VALIDATION_ERROR: 排序列表包含不存在的 Hook".to_string());
        }
    }
    transaction.commit().map_err(|error| error.to_string())?;
    list_hooks(database)
}

pub fn delete_hook(database: &Database, id: &str) -> Result<(), String> {
    let connection = database.open()?;
    let changed = connection
        .execute("DELETE FROM device_hooks WHERE id = ?1", [id])
        .map_err(|error| error.to_string())?;
    if changed == 0 {
        return Err("NOT_FOUND: Hook 不存在".to_string());
    }
    Ok(())
}

pub fn list_runs(
    database: &Database,
    request: HookRunListRequest,
) -> Result<Vec<HookRunDto>, String> {
    let connection = database.open()?;
    let limit = request.limit.unwrap_or(50).clamp(1, 100);
    let mut statement = connection
        .prepare(
            "SELECT id, hook_id, hook_name_snapshot, event_id, event_type, is_test, status,
                    exit_code, duration_ms, stdout_tail, stderr_tail, error_message,
                    started_at, finished_at, created_at
             FROM device_hook_runs
             WHERE (?1 IS NULL OR hook_id = ?1)
             ORDER BY created_at DESC LIMIT ?2",
        )
        .map_err(|error| error.to_string())?;
    let runs = statement
        .query_map(params![request.hook_id, limit], run_from_row)
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(runs)
}

pub fn get_run(database: &Database, id: &str) -> Result<HookRunDto, String> {
    let connection = database.open()?;
    connection
        .query_row(
            "SELECT id, hook_id, hook_name_snapshot, event_id, event_type, is_test, status,
                    exit_code, duration_ms, stdout_tail, stderr_tail, error_message,
                    started_at, finished_at, created_at
             FROM device_hook_runs WHERE id = ?1",
            [id],
            run_from_row,
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "NOT_FOUND: Hook 执行记录不存在".to_string())
}

pub fn recover_interrupted_runs(database: &Database) -> Result<usize, String> {
    let connection = database.open()?;
    let now = now_millis();
    let changed = connection
        .execute(
            "UPDATE device_hook_runs
             SET status = 'failed', finished_at = ?1,
                 duration_ms = CASE WHEN started_at IS NULL THEN NULL ELSE MAX(0, ?1 - started_at) END,
                 error_message = '应用上次退出时执行被中断'
             WHERE status IN ('queued', 'running')",
            [now],
        )
        .map_err(|error| error.to_string())?;
    cleanup_runs(database)?;
    Ok(changed)
}

pub fn cleanup_runs(database: &Database) -> Result<usize, String> {
    let connection = database.open()?;
    connection
        .execute(
            "DELETE FROM device_hook_runs
             WHERE created_at < ?1
               AND id NOT IN (
                 SELECT id FROM device_hook_runs ORDER BY created_at DESC LIMIT ?2
               )",
            params![now_millis() - RUN_RETENTION_MILLIS, RUN_RETENTION_COUNT],
        )
        .map_err(|error| error.to_string())
}

pub fn timer_event(
    database: &Database,
    event_type: &str,
    event_id: String,
    entry: &TimeEntryDto,
) -> Result<HookEvent, String> {
    if !matches!(event_type, "timer.started" | "timer.stopped") {
        return Err("VALIDATION_ERROR: 非法计时 Hook 事件".to_string());
    }
    let connection = database.open()?;
    let device = load_device(&connection)?;
    let (task, subject) = match entry.default_task_id.as_deref() {
        Some(task_id) => load_task_context(&connection, task_id, None)?.unwrap_or((None, None)),
        None => (None, None),
    };
    Ok(HookEvent {
        schema_version: 1,
        event: event_type.to_string(),
        event_id,
        occurred_at: now_rfc3339(),
        test: false,
        device,
        subject,
        task,
        task_snapshot: Some(HookTaskNameSnapshot {
            title: entry.label.clone(),
        }),
        timer: Some(HookTimerSnapshot {
            id: entry.id.clone(),
            started_at: entry.started_at,
            ended_at: entry.ended_at,
            duration_seconds: entry.duration_seconds,
            duration_minutes: (entry.duration_seconds + 59) / 60,
            settlement_minutes: entry.settlement_minutes,
            allocated_minutes: entry.allocated_minutes,
            pending_minutes: (entry.settlement_minutes - entry.allocated_minutes).max(0),
        }),
    })
}

pub fn task_completed_event(
    database: &Database,
    event_id: String,
    task: &TaskDto,
) -> Result<HookEvent, String> {
    let connection = database.open()?;
    let device = load_device(&connection)?;
    let subject = connection
        .query_row(
            "SELECT id, name FROM subjects WHERE id = ?1 AND deleted_at IS NULL",
            [&task.subject_id],
            |row| {
                Ok(HookSubjectSnapshot {
                    id: row.get(0)?,
                    name: row.get(1)?,
                })
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;
    Ok(HookEvent {
        schema_version: 1,
        event: "task.completed".to_string(),
        event_id,
        occurred_at: now_rfc3339(),
        test: false,
        device,
        subject,
        task: Some(HookTaskSnapshot {
            id: task.id.clone(),
            title: task.title.clone(),
            path: task.path_label.clone(),
            occurrence_date: task.occurrence_date.clone(),
        }),
        task_snapshot: Some(HookTaskNameSnapshot {
            title: task.title.clone(),
        }),
        timer: None,
    })
}

pub fn publish(database: Database, event: HookEvent) -> Result<usize, String> {
    validate_event(&event.event)?;
    let connection = database.open()?;
    let mut statement = connection
        .prepare(
            "SELECT id, name, event_type, action_type, action_config_json, timeout_seconds,
                    enabled, sort_order, created_at, updated_at
             FROM device_hooks
             WHERE event_type = ?1 AND enabled = 1
             ORDER BY sort_order, created_at",
        )
        .map_err(|error| error.to_string())?;
    let hooks = statement
        .query_map([&event.event], |row| hook_from_row(&connection, row))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(statement);
    drop(connection);
    let mut queued = Vec::new();
    for hook in hooks {
        if let Some(run_id) = queue_run(&database, &hook, &event)? {
            queued.push((hook, run_id));
        }
    }
    let count = queued.len();
    if count > 0 {
        tauri::async_runtime::spawn(async move {
            for (hook, run_id) in queued {
                let database_for_execution = database.clone();
                let event_for_execution = event.clone();
                let result = tauri::async_runtime::spawn_blocking(move || {
                    execute_queued_run(
                        &database_for_execution,
                        &run_id,
                        &hook,
                        &event_for_execution,
                    )
                })
                .await;
                if let Err(error) = result {
                    eprintln!("Hook 后台任务异常: {error}");
                }
            }
            let _ = cleanup_runs(&database);
        });
    }
    Ok(count)
}

pub fn test_draft(database: &Database, draft: HookDraftRequest) -> Result<HookRunDto, String> {
    validate_draft(&draft)?;
    let event = sample_event(database, &draft.event_type)?;
    let hook = HookRuleDto {
        id: Uuid::now_v7().to_string(),
        name: draft.name.trim().to_string(),
        event_type: draft.event_type,
        action_type: draft.action_type,
        action_config: draft.action_config,
        timeout_seconds: draft.timeout_seconds,
        enabled: draft.enabled,
        sort_order: 0,
        created_at: now_millis(),
        updated_at: now_millis(),
        recent_run: None,
    };
    let run_id = Uuid::now_v7().to_string();
    insert_test_run(database, &run_id, &hook, &event)?;
    execute_queued_run(database, &run_id, &hook, &event)?;
    get_run(database, &run_id)
}

pub fn preview_template(
    event: &HookEvent,
    template: &str,
    uri_encode: bool,
) -> Result<HookTemplatePreview, String> {
    let variables = template_values(event);
    render_template(template, &variables, uri_encode)
}

fn validate_draft(draft: &HookDraftRequest) -> Result<(), String> {
    if draft.name.trim().is_empty() || draft.name.trim().chars().count() > 80 {
        return Err("VALIDATION_ERROR: Hook 名称需为 1 至 80 个字符".to_string());
    }
    validate_event(&draft.event_type)?;
    if !(1..=60).contains(&draft.timeout_seconds) {
        return Err("VALIDATION_ERROR: 超时时间必须为 1 至 60 秒".to_string());
    }
    match draft.action_type.as_str() {
        "uri" => {
            let config: UriActionConfig = serde_json::from_value(draft.action_config.clone())
                .map_err(|_| "VALIDATION_ERROR: URI 动作配置不完整".to_string())?;
            validate_template(&config.uri_template)?;
            validate_uri_template(&config.uri_template)?;
        }
        "process" => {
            let config: ProcessActionConfig =
                serde_json::from_value(draft.action_config.clone())
                    .map_err(|_| "VALIDATION_ERROR: 本地程序动作配置不完整".to_string())?;
            if config.program.trim().is_empty() {
                return Err("VALIDATION_ERROR: 本地程序不能为空".to_string());
            }
            if contains_control_char(&config.program)
                || config
                    .working_directory
                    .as_deref()
                    .is_some_and(contains_control_char)
            {
                return Err("VALIDATION_ERROR: 程序或工作目录包含控制字符".to_string());
            }
            for argument in &config.args {
                validate_template(argument)?;
                if contains_control_char(argument) {
                    return Err("VALIDATION_ERROR: 程序参数包含控制字符".to_string());
                }
            }
        }
        _ => return Err("VALIDATION_ERROR: 不支持的 Hook 动作类型".to_string()),
    }
    Ok(())
}

fn validate_event(event_type: &str) -> Result<(), String> {
    if SUPPORTED_EVENTS.contains(&event_type) {
        Ok(())
    } else {
        Err("VALIDATION_ERROR: 不支持的 Hook 事件".to_string())
    }
}

fn validate_template(template: &str) -> Result<(), String> {
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let end = after
            .find("}}")
            .ok_or_else(|| "VALIDATION_ERROR: 模板变量缺少结束符 }}".to_string())?;
        let name = after[..end].trim();
        if !TEMPLATE_VARIABLES.contains(&name) {
            return Err(format!("VALIDATION_ERROR: 未知模板变量 {{{{{name}}}}}"));
        }
        rest = &after[end + 2..];
    }
    if rest.contains("}}") {
        return Err("VALIDATION_ERROR: 模板变量缺少开始符 {{".to_string());
    }
    Ok(())
}

fn validate_uri_template(template: &str) -> Result<(), String> {
    if template.trim().is_empty() || contains_control_char(template) {
        return Err("VALIDATION_ERROR: URI 模板不能为空或包含控制字符".to_string());
    }
    let sample = TEMPLATE_VARIABLES
        .iter()
        .fold(template.to_string(), |value, name| {
            value.replace(&format!("{{{{{name}}}}}"), "sample")
        });
    validate_final_uri(&sample).map(|_| ())
}

fn validate_final_uri(uri: &str) -> Result<Url, String> {
    let parsed = Url::parse(uri).map_err(|error| format!("VALIDATION_ERROR: URI 无效: {error}"))?;
    let scheme = parsed.scheme().to_ascii_lowercase();
    if scheme.is_empty() || matches!(scheme.as_str(), "file" | "javascript" | "data") {
        return Err("VALIDATION_ERROR: URI scheme 不允许".to_string());
    }
    if !scheme.chars().enumerate().all(|(index, value)| {
        value.is_ascii_alphabetic()
            || (index > 0 && (value.is_ascii_digit() || matches!(value, '+' | '-' | '.')))
    }) {
        return Err("VALIDATION_ERROR: URI scheme 无效".to_string());
    }
    Ok(parsed)
}

fn render_template(
    template: &str,
    values: &BTreeMap<&'static str, Option<String>>,
    uri_encode: bool,
) -> Result<HookTemplatePreview, String> {
    validate_template(template)?;
    let mut rendered = String::with_capacity(template.len());
    let mut warnings = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        rendered.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after.find("}}").expect("validated template");
        let name = after[..end].trim();
        let value = values.get(name).and_then(|value| value.clone());
        if value.is_none() {
            warnings.push(format!(
                "模板变量 {{{{{name}}}}} 当前没有值，已渲染为空字符串"
            ));
        }
        let value = value.unwrap_or_default();
        if uri_encode {
            let encoded = url::form_urlencoded::byte_serialize(value.as_bytes())
                .collect::<String>()
                .replace('+', "%20");
            rendered.push_str(&encoded);
        } else {
            rendered.push_str(&value);
        }
        rest = &after[end + 2..];
    }
    rendered.push_str(rest);
    Ok(HookTemplatePreview { rendered, warnings })
}

fn template_values(event: &HookEvent) -> BTreeMap<&'static str, Option<String>> {
    BTreeMap::from([
        ("event.type", Some(event.event.clone())),
        ("event.id", Some(event.event_id.clone())),
        ("task.id", event.task.as_ref().map(|value| value.id.clone())),
        (
            "task.title",
            event
                .task
                .as_ref()
                .map(|value| value.title.clone())
                .or_else(|| {
                    event
                        .task_snapshot
                        .as_ref()
                        .map(|value| value.title.clone())
                }),
        ),
        (
            "task.path",
            event.task.as_ref().map(|value| value.path.clone()),
        ),
        (
            "task.occurrenceDate",
            event
                .task
                .as_ref()
                .and_then(|value| value.occurrence_date.clone()),
        ),
        (
            "subject.id",
            event.subject.as_ref().map(|value| value.id.clone()),
        ),
        (
            "subject.name",
            event.subject.as_ref().map(|value| value.name.clone()),
        ),
        (
            "timer.id",
            event.timer.as_ref().map(|value| value.id.clone()),
        ),
        (
            "timer.durationSeconds",
            event
                .timer
                .as_ref()
                .map(|value| value.duration_seconds.to_string()),
        ),
        (
            "timer.durationMinutes",
            event
                .timer
                .as_ref()
                .map(|value| value.duration_minutes.to_string()),
        ),
        (
            "timer.pendingMinutes",
            event
                .timer
                .as_ref()
                .map(|value| value.pending_minutes.to_string()),
        ),
    ])
}

fn queue_run(
    database: &Database,
    hook: &HookRuleDto,
    event: &HookEvent,
) -> Result<Option<String>, String> {
    let connection = database.open()?;
    let id = Uuid::now_v7().to_string();
    let inserted = connection
        .execute(
            "INSERT OR IGNORE INTO device_hook_runs(
               id, hook_id, hook_name_snapshot, event_id, event_type, is_test, status, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, 0, 'queued', ?6)",
            params![
                id,
                hook.id,
                hook.name,
                event.event_id,
                event.event,
                now_millis()
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok((inserted > 0).then_some(id))
}

fn insert_test_run(
    database: &Database,
    id: &str,
    hook: &HookRuleDto,
    event: &HookEvent,
) -> Result<(), String> {
    let connection = database.open()?;
    connection
        .execute(
            "INSERT INTO device_hook_runs(
               id, hook_id, hook_name_snapshot, event_id, event_type, is_test, status, created_at
             ) VALUES (?1, NULL, ?2, ?3, ?4, 1, 'queued', ?5)",
            params![id, hook.name, event.event_id, event.event, now_millis()],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn execute_queued_run(
    database: &Database,
    run_id: &str,
    hook: &HookRuleDto,
    event: &HookEvent,
) -> Result<(), String> {
    let started_at = now_millis();
    let connection = database.open()?;
    connection
        .execute(
            "UPDATE device_hook_runs SET status = 'running', started_at = ?1 WHERE id = ?2 AND status = 'queued'",
            params![started_at, run_id],
        )
        .map_err(|error| error.to_string())?;
    drop(connection);
    let result = execute_action(hook, event);
    let connection = database.open()?;
    connection
        .execute(
            "UPDATE device_hook_runs
             SET status = ?1, exit_code = ?2, duration_ms = ?3, stdout_tail = ?4,
                 stderr_tail = ?5, error_message = ?6, finished_at = ?7
             WHERE id = ?8",
            params![
                result.status,
                result.exit_code,
                result.duration_ms,
                result.stdout_tail,
                result.stderr_tail,
                result.error_message,
                now_millis(),
                run_id
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn execute_action(hook: &HookRuleDto, event: &HookEvent) -> ExecutionResult {
    let started = Instant::now();
    let result = match hook.action_type.as_str() {
        "uri" => execute_uri(&hook.action_config, event),
        "process" => execute_process(&hook.action_config, event, hook.timeout_seconds),
        _ => Err("不支持的 Hook 动作类型".to_string()),
    };
    match result {
        Ok(mut result) => {
            result.duration_ms = started.elapsed().as_millis() as i64;
            result
        }
        Err(error) => ExecutionResult {
            status: "failed".to_string(),
            exit_code: None,
            duration_ms: started.elapsed().as_millis() as i64,
            stdout_tail: None,
            stderr_tail: None,
            error_message: Some(error),
        },
    }
}

fn execute_uri(config: &Value, event: &HookEvent) -> Result<ExecutionResult, String> {
    let config: UriActionConfig =
        serde_json::from_value(config.clone()).map_err(|_| "URI 动作配置不完整".to_string())?;
    let preview = preview_template(event, &config.uri_template, true)?;
    validate_final_uri(&preview.rendered)?;
    open_uri(&preview.rendered)?;
    Ok(ExecutionResult {
        status: "succeeded".to_string(),
        exit_code: None,
        duration_ms: 0,
        stdout_tail: None,
        stderr_tail: None,
        error_message: preview.warnings.first().cloned(),
    })
}

fn execute_process(
    config: &Value,
    event: &HookEvent,
    timeout_seconds: i64,
) -> Result<ExecutionResult, String> {
    let config: ProcessActionConfig =
        serde_json::from_value(config.clone()).map_err(|_| "本地程序动作配置不完整".to_string())?;
    let values = template_values(event);
    let args = config
        .args
        .iter()
        .map(|argument| render_template(argument, &values, false).map(|value| value.rendered))
        .collect::<Result<Vec<_>, _>>()?;
    let mut command = Command::new(config.program.trim());
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(directory) = config
        .working_directory
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        command.current_dir(directory);
    }
    configure_hidden_process(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("无法启动本地程序 {}: {error}", config.program))?;
    let stdin_json = serde_json::to_vec(event).map_err(|error| error.to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(&stdin_json)
            .map_err(|error| format!("写入 Hook JSON 失败: {error}"))?;
    }
    let stdout = child.stdout.take().map(read_pipe);
    let stderr = child.stderr.take().map(read_pipe);
    let deadline = Instant::now() + Duration::from_secs(timeout_seconds as u64);
    let (status, timed_out) = loop {
        match child.try_wait().map_err(|error| error.to_string())? {
            Some(status) => break (status, false),
            None if Instant::now() >= deadline => {
                child
                    .kill()
                    .map_err(|error| format!("终止超时程序失败: {error}"))?;
                break (
                    child
                        .wait()
                        .map_err(|error| format!("等待超时程序退出失败: {error}"))?,
                    true,
                );
            }
            None => thread::sleep(Duration::from_millis(25)),
        }
    };
    let (stdout, stdout_truncated) = stdout
        .map(|handle| handle.join().unwrap_or_default())
        .unwrap_or_default();
    let (stderr, stderr_truncated) = stderr
        .map(|handle| handle.join().unwrap_or_default())
        .unwrap_or_default();
    let stdout_tail = output_tail(&stdout, stdout_truncated);
    let stderr_tail = output_tail(&stderr, stderr_truncated);
    if timed_out {
        return Ok(ExecutionResult {
            status: "timed_out".to_string(),
            exit_code: status.code().map(i64::from),
            duration_ms: 0,
            stdout_tail,
            stderr_tail,
            error_message: Some(format!("本地程序执行超过 {timeout_seconds} 秒，已终止")),
        });
    }
    let succeeded = status.success();
    Ok(ExecutionResult {
        status: if succeeded { "succeeded" } else { "failed" }.to_string(),
        exit_code: status.code().map(i64::from),
        duration_ms: 0,
        stdout_tail,
        stderr_tail,
        error_message: (!succeeded).then(|| format!("本地程序退出码为 {:?}", status.code())),
    })
}

fn read_pipe<R: Read + Send + 'static>(mut pipe: R) -> thread::JoinHandle<(Vec<u8>, bool)> {
    thread::spawn(move || {
        let mut tail = Vec::with_capacity(OUTPUT_LIMIT_BYTES);
        let mut buffer = [0_u8; 4096];
        let mut truncated = false;
        loop {
            let read = match pipe.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => read,
            };
            if tail.len() + read > OUTPUT_LIMIT_BYTES {
                let overflow = tail.len() + read - OUTPUT_LIMIT_BYTES;
                if overflow >= tail.len() {
                    tail.clear();
                } else {
                    tail.drain(..overflow);
                }
                truncated = true;
            }
            let start = read.saturating_sub(OUTPUT_LIMIT_BYTES - tail.len());
            if start > 0 {
                truncated = true;
            }
            tail.extend_from_slice(&buffer[start..read]);
        }
        (tail, truncated)
    })
}

fn output_tail(output: &[u8], truncated: bool) -> Option<String> {
    if output.is_empty() {
        return None;
    }
    let text = String::from_utf8_lossy(output);
    Some(if truncated {
        format!("[输出已截断，仅保留末尾 8 KiB]\n{text}")
    } else {
        text.into_owned()
    })
}

#[cfg(target_os = "windows")]
fn configure_hidden_process(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x08000000);
}

#[cfg(not(target_os = "windows"))]
fn configure_hidden_process(_command: &mut Command) {}

#[cfg(target_os = "windows")]
fn open_uri(uri: &str) -> Result<(), String> {
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let operation = std::ffi::OsStr::new("open")
        .encode_wide()
        .chain(once(0))
        .collect::<Vec<_>>();
    let target = std::ffi::OsStr::new(uri)
        .encode_wide()
        .chain(once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            operation.as_ptr(),
            target.as_ptr(),
            ptr::null(),
            ptr::null(),
            SW_SHOWNORMAL,
        )
    } as isize;
    if result <= 32 {
        Err(format!(
            "系统无法打开 URI，协议可能未注册（ShellExecute 错误 {result}）"
        ))
    } else {
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
fn open_uri(uri: &str) -> Result<(), String> {
    let (program, argument) = if cfg!(target_os = "macos") {
        ("open", uri)
    } else {
        ("xdg-open", uri)
    };
    let mut command = Command::new(program);
    command
        .arg(argument)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let status = command
        .status()
        .map_err(|error| format!("无法调用系统 URI 处理器: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("系统 URI 处理器退出码为 {:?}", status.code()))
    }
}

fn sample_event(database: &Database, event_type: &str) -> Result<HookEvent, String> {
    let connection = database.open()?;
    let device = load_device(&connection)?;
    Ok(HookEvent {
        schema_version: 1,
        event: event_type.to_string(),
        event_id: format!("test:{}:{}", event_type, Uuid::now_v7()),
        occurred_at: now_rfc3339(),
        test: true,
        device,
        subject: Some(HookSubjectSnapshot {
            id: "test-subject".to_string(),
            name: "测试主体".to_string(),
        }),
        task: Some(HookTaskSnapshot {
            id: "test-task".to_string(),
            title: "Hook 测试事项".to_string(),
            path: "测试计划 / Hook 测试事项".to_string(),
            occurrence_date: Some("2026-10-01".to_string()),
        }),
        task_snapshot: Some(HookTaskNameSnapshot {
            title: "Hook 测试事项".to_string(),
        }),
        timer: Some(HookTimerSnapshot {
            id: "test-timer".to_string(),
            started_at: now_millis() - 1_800_000,
            ended_at: Some(now_millis()),
            duration_seconds: 1800,
            duration_minutes: 30,
            settlement_minutes: 30,
            allocated_minutes: 20,
            pending_minutes: 10,
        }),
    })
}

fn load_device(connection: &Connection) -> Result<HookDeviceSnapshot, String> {
    connection
        .query_row(
            "SELECT id FROM devices WHERE revoked_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| Ok(HookDeviceSnapshot { id: row.get(0)? }),
        )
        .map_err(|error| error.to_string())
}

fn load_task_context(
    connection: &Connection,
    task_id: &str,
    occurrence_date: Option<String>,
) -> Result<Option<(Option<HookTaskSnapshot>, Option<HookSubjectSnapshot>)>, String> {
    let row = connection
        .query_row(
            "SELECT t.id, t.title, t.parent_id, t.subject_id, s.name
             FROM tasks t JOIN subjects s ON s.id = t.subject_id
             WHERE t.id = ?1 AND t.deleted_at IS NULL AND s.deleted_at IS NULL",
            [task_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some((id, title, parent_id, subject_id, subject_name)) = row else {
        return Ok(None);
    };
    let mut path = vec![title.clone()];
    let mut current = parent_id;
    while let Some(parent_id) = current {
        let parent = connection
            .query_row(
                "SELECT title, parent_id FROM tasks WHERE id = ?1 AND deleted_at IS NULL",
                [&parent_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        let Some((title, parent_id)) = parent else {
            break;
        };
        path.push(title);
        current = parent_id;
    }
    path.reverse();
    Ok(Some((
        Some(HookTaskSnapshot {
            id,
            title,
            path: path.join(" / "),
            occurrence_date,
        }),
        Some(HookSubjectSnapshot {
            id: subject_id,
            name: subject_name,
        }),
    )))
}

fn load_hook(connection: &Connection, id: &str) -> Result<HookRuleDto, String> {
    connection
        .query_row(
            "SELECT id, name, event_type, action_type, action_config_json, timeout_seconds,
                    enabled, sort_order, created_at, updated_at
             FROM device_hooks WHERE id = ?1",
            [id],
            |row| hook_from_row(connection, row),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "NOT_FOUND: Hook 不存在".to_string())
}

fn hook_from_row(
    connection: &Connection,
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<HookRuleDto> {
    let id: String = row.get(0)?;
    let action_config_json: String = row.get(4)?;
    let recent_run = connection
        .query_row(
            "SELECT id, hook_id, hook_name_snapshot, event_id, event_type, is_test, status,
                    exit_code, duration_ms, stdout_tail, stderr_tail, error_message,
                    started_at, finished_at, created_at
             FROM device_hook_runs WHERE hook_id = ?1 ORDER BY created_at DESC LIMIT 1",
            [&id],
            run_from_row,
        )
        .optional()?;
    Ok(HookRuleDto {
        id,
        name: row.get(1)?,
        event_type: row.get(2)?,
        action_type: row.get(3)?,
        action_config: serde_json::from_str(&action_config_json).unwrap_or(Value::Null),
        timeout_seconds: row.get(5)?,
        enabled: row.get(6)?,
        sort_order: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        recent_run,
    })
}

fn run_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<HookRunDto> {
    Ok(HookRunDto {
        id: row.get(0)?,
        hook_id: row.get(1)?,
        hook_name_snapshot: row.get(2)?,
        event_id: row.get(3)?,
        event_type: row.get(4)?,
        is_test: row.get(5)?,
        status: row.get(6)?,
        exit_code: row.get(7)?,
        duration_ms: row.get(8)?,
        stdout_tail: row.get(9)?,
        stderr_tail: row.get(10)?,
        error_message: row.get(11)?,
        started_at: row.get(12)?,
        finished_at: row.get(13)?,
        created_at: row.get(14)?,
    })
}

fn contains_control_char(value: &str) -> bool {
    value.chars().any(|character| character.is_control())
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[tauri::command]
pub fn automation_hook_list(
    database: tauri::State<'_, Database>,
) -> Result<Vec<HookRuleDto>, String> {
    list_hooks(&database)
}

#[tauri::command]
pub fn automation_hook_create(
    database: tauri::State<'_, Database>,
    request: HookDraftRequest,
) -> Result<HookRuleDto, String> {
    create_hook(&database, request)
}

#[tauri::command]
pub fn automation_hook_update(
    database: tauri::State<'_, Database>,
    request: HookUpdateRequest,
) -> Result<HookRuleDto, String> {
    update_hook(&database, request)
}

#[tauri::command]
pub fn automation_hook_set_enabled(
    database: tauri::State<'_, Database>,
    request: HookEnabledRequest,
) -> Result<HookRuleDto, String> {
    set_hook_enabled(&database, request)
}

#[tauri::command]
pub fn automation_hook_reorder(
    database: tauri::State<'_, Database>,
    request: HookReorderRequest,
) -> Result<Vec<HookRuleDto>, String> {
    reorder_hooks(&database, request)
}

#[tauri::command]
pub fn automation_hook_delete(
    database: tauri::State<'_, Database>,
    request: HookIdRequest,
) -> Result<(), String> {
    delete_hook(&database, &request.id)
}

#[tauri::command]
pub fn automation_hook_run_list(
    database: tauri::State<'_, Database>,
    request: HookRunListRequest,
) -> Result<Vec<HookRunDto>, String> {
    list_runs(&database, request)
}

#[tauri::command]
pub fn automation_hook_run_get(
    database: tauri::State<'_, Database>,
    request: HookIdRequest,
) -> Result<HookRunDto, String> {
    get_run(&database, &request.id)
}

#[tauri::command]
pub async fn automation_hook_test(
    database: tauri::State<'_, Database>,
    request: HookDraftRequest,
) -> Result<HookRunDto, String> {
    let database = database.inner().clone();
    tauri::async_runtime::spawn_blocking(move || test_draft(&database, request))
        .await
        .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn database() -> Database {
        let directory = tempdir().unwrap();
        let path = directory.keep().join("hooks.sqlite3");
        Database::initialize_at(path).unwrap()
    }

    fn uri_draft(name: &str) -> HookDraftRequest {
        HookDraftRequest {
            name: name.to_string(),
            event_type: "task.completed".to_string(),
            action_type: "uri".to_string(),
            action_config: json!({ "uriTemplate": "motioncue://play/{{task.title}}" }),
            timeout_seconds: 10,
            enabled: true,
        }
    }

    #[test]
    fn hook_crud_validates_and_preserves_run_snapshot_after_delete() {
        let database = database();
        let created = create_hook(&database, uri_draft("完成反馈")).unwrap();
        assert_eq!(list_hooks(&database).unwrap().len(), 1);
        let updated = update_hook(
            &database,
            HookUpdateRequest {
                id: created.id.clone(),
                draft: HookDraftRequest {
                    name: "完成反馈 2".to_string(),
                    ..uri_draft("unused")
                },
            },
        )
        .unwrap();
        assert_eq!(updated.name, "完成反馈 2");
        assert!(
            !set_hook_enabled(
                &database,
                HookEnabledRequest {
                    id: created.id.clone(),
                    enabled: false,
                },
            )
            .unwrap()
            .enabled
        );
        let event = sample_event(&database, "task.completed").unwrap();
        let run_id = Uuid::now_v7().to_string();
        insert_test_run(&database, &run_id, &updated, &event).unwrap();
        delete_hook(&database, &created.id).unwrap();
        let run = get_run(&database, &run_id).unwrap();
        assert_eq!(run.hook_name_snapshot, "完成反馈 2");
        assert!(run.hook_id.is_none());
    }

    #[test]
    fn invalid_hook_configs_are_rejected() {
        let database = database();
        let mut draft = uri_draft("bad");
        draft.timeout_seconds = 0;
        assert!(create_hook(&database, draft).is_err());
        let mut draft = uri_draft("bad");
        draft.action_config = json!({ "uriTemplate": "file:///tmp/a" });
        assert!(create_hook(&database, draft).is_err());
        let mut draft = uri_draft("bad");
        draft.action_config = json!({ "uriTemplate": "test://{{unknown.value}}" });
        assert!(create_hook(&database, draft).is_err());
    }

    #[test]
    fn template_encoding_handles_chinese_reserved_and_missing_values() {
        let database = database();
        let mut event = sample_event(&database, "task.completed").unwrap();
        event.task.as_mut().unwrap().title = "中文 空格 &/?".to_string();
        event.timer = None;
        let preview = preview_template(
            &event,
            "motioncue://play?task={{task.title}}&minutes={{timer.pendingMinutes}}",
            true,
        )
        .unwrap();
        assert!(preview
            .rendered
            .contains("%E4%B8%AD%E6%96%87%20%E7%A9%BA%E6%A0%BC%20%26%2F%3F"));
        assert!(preview.rendered.ends_with("minutes="));
        assert_eq!(preview.warnings.len(), 1);
    }

    #[test]
    fn event_serialization_handles_deleted_task_and_occurrence() {
        let database = database();
        let mut deleted = sample_event(&database, "timer.stopped").unwrap();
        deleted.task = None;
        deleted.task_snapshot = Some(HookTaskNameSnapshot {
            title: "已删除事项".to_string(),
        });
        let value = serde_json::to_value(&deleted).unwrap();
        assert!(value["task"].is_null());
        assert_eq!(value["taskSnapshot"]["title"], "已删除事项");
        let recurring = sample_event(&database, "task.completed").unwrap();
        assert_eq!(
            recurring.task.unwrap().occurrence_date.as_deref(),
            Some("2026-10-01")
        );
    }

    #[test]
    fn duplicate_publish_queues_each_hook_only_once() {
        let database = database();
        let hook = create_hook(&database, uri_draft("one")).unwrap();
        let event = sample_event(&database, "task.completed").unwrap();
        assert!(queue_run(&database, &hook, &event).unwrap().is_some());
        assert!(queue_run(&database, &hook, &event).unwrap().is_none());
    }

    #[test]
    fn test_draft_uses_sample_event_without_saving_rule() {
        let database = database();
        let executable = std::env::current_exe().unwrap();
        let run = test_draft(
            &database,
            HookDraftRequest {
                name: "draft test".to_string(),
                event_type: "timer.stopped".to_string(),
                action_type: "process".to_string(),
                action_config: json!({
                    "program": executable,
                    "args": ["--exact", "automation_hooks::tests::hook_helper", "--nocapture"],
                    "workingDirectory": null
                }),
                timeout_seconds: 10,
                enabled: true,
            },
        )
        .unwrap();
        assert!(run.is_test);
        assert_eq!(run.status, "succeeded");
        assert!(run.event_id.starts_with("test:timer.stopped:"));
        assert!(run.stdout_tail.unwrap_or_default().contains("hook helper"));
        assert!(list_hooks(&database).unwrap().is_empty());
    }

    #[test]
    fn recovery_and_cleanup_keep_recent_records() {
        let database = database();
        let hook = create_hook(&database, uri_draft("cleanup")).unwrap();
        let connection = database.open().unwrap();
        for index in 0..105 {
            connection.execute(
                "INSERT INTO device_hook_runs(id, hook_id, hook_name_snapshot, event_id, event_type, is_test, status, created_at)
                 VALUES (?1, ?2, 'cleanup', ?3, 'task.completed', 1, ?4, ?5)",
                params![
                    Uuid::now_v7().to_string(),
                    hook.id,
                    format!("test:{index}"),
                    if index == 104 { "running" } else { "succeeded" },
                    now_millis() - RUN_RETENTION_MILLIS - 10_000 + index
                ],
            ).unwrap();
        }
        drop(connection);
        assert_eq!(recover_interrupted_runs(&database).unwrap(), 1);
        let runs = list_runs(
            &database,
            HookRunListRequest {
                hook_id: None,
                limit: Some(100),
            },
        )
        .unwrap();
        assert_eq!(runs.len(), 100);
        assert_eq!(runs[0].status, "failed");
    }

    #[test]
    fn process_executor_captures_stdin_nonzero_output_and_timeout() {
        let database = database();
        let event = sample_event(&database, "timer.stopped").unwrap();
        let executable = std::env::current_exe().unwrap();
        let config = json!({
            "program": executable,
            "args": ["--exact", "automation_hooks::tests::hook_helper", "--nocapture"],
            "workingDirectory": null
        });
        let result = execute_process(&config, &event, 10).unwrap();
        assert_eq!(result.status, "succeeded");
        assert!(result
            .stdout_tail
            .unwrap_or_default()
            .contains("hook helper"));

        let failed = execute_process(
            &json!({
                "program": executable,
                "args": ["--exact", "automation_hooks::tests::hook_helper", "--nocapture", "--", "fail"],
                "workingDirectory": null
            }),
            &event,
            10,
        )
        .unwrap();
        assert_eq!(failed.status, "failed");
        assert_eq!(failed.exit_code, Some(7));

        let large = execute_process(
            &json!({
                "program": executable,
                "args": ["--exact", "automation_hooks::tests::hook_helper", "--nocapture", "--", "large"],
                "workingDirectory": null
            }),
            &event,
            10,
        )
        .unwrap();
        let output = large.stdout_tail.unwrap();
        assert!(output.starts_with("[输出已截断"));
        assert!(output.len() <= OUTPUT_LIMIT_BYTES + 80);

        let timed_out = execute_process(
            &json!({
                "program": executable,
                "args": ["--exact", "automation_hooks::tests::hook_helper", "--nocapture", "--", "sleep"],
                "workingDirectory": null
            }),
            &event,
            1,
        )
        .unwrap();
        assert_eq!(timed_out.status, "timed_out");
    }

    #[test]
    fn hook_helper() {
        if std::env::args().any(|value| value == "--exact") {
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input).unwrap();
            assert!(input.contains("schemaVersion"));
            let mode = std::env::args().last().unwrap_or_default();
            if mode == "fail" {
                eprintln!("hook helper failed");
                std::process::exit(7);
            }
            if mode == "large" {
                println!("{}TAIL", "x".repeat(OUTPUT_LIMIT_BYTES + 1024));
                return;
            }
            if mode == "sleep" {
                thread::sleep(Duration::from_secs(2));
                return;
            }
            println!("hook helper");
        }
    }
}
