use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::database::Database;
use crate::tasks::{parse_plan_markdown, TaskDto};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportWritePreviewRequest {
    pub report_id: String,
    pub expected_version: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ReportWritePreviewResult {
    pub run_id: String,
    pub report_id: String,
    pub report_version: i64,
    pub target_path: String,
    pub file_exists: bool,
    pub existing_content: String,
    pub existing_content_hash: Option<String>,
    pub existing_file_mtime: Option<i64>,
    pub report_markdown: String,
    pub action: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportWriteExecuteRequest {
    pub run_id: String,
    pub strategy: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportWriteExecuteResult {
    pub run_id: String,
    pub report_id: String,
    pub target_path: String,
    pub strategy: String,
    pub written_at: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanImportPreviewRequest {
    pub source_date: String,
    pub target_date: String,
    pub subject_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanImportPreviewResult {
    pub batch_id: String,
    pub source_path: String,
    pub source_exists: bool,
    pub source_mtime: Option<i64>,
    pub source_content_hash: Option<String>,
    pub raw_markdown: String,
    pub items: Vec<PlanImportPreviewItem>,
    pub warnings: Vec<String>,
    pub version: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanImportPreviewItem {
    pub id: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub estimate_minutes: Option<i64>,
    pub sort_order: i64,
    pub source_line_no: usize,
    pub source_text: String,
    pub parse_status: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanImportConfirmRequest {
    pub batch_id: String,
    pub items: Vec<PlanImportConfirmItem>,
    pub expected_version: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanImportConfirmItem {
    pub import_item_id: String,
    pub selected: bool,
    pub title: String,
    pub estimate_minutes: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanImportConfirmResult {
    pub batch_id: String,
    pub tasks: Vec<TaskDto>,
    pub already_confirmed: bool,
}

pub fn preview_report_write(
    database: &Database,
    request: ReportWritePreviewRequest,
) -> Result<ReportWritePreviewResult, String> {
    let connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let (report_type, period_start, period_end, markdown, version): (
        String,
        String,
        String,
        String,
        i64,
    ) = connection
        .query_row(
            "SELECT report_type, period_start, period_end, markdown_content, version
             FROM reports WHERE id = ?1 AND workspace_id = ?2 AND deleted_at IS NULL",
            params![request.report_id, workspace_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "NOT_FOUND: 报告不存在".to_string())?;
    if version != request.expected_version {
        return Err("VERSION_CONFLICT: 报告内容已变化，请保存后重试".to_string());
    }
    let target_path = resolve_report_path(&connection, &report_type, &period_start, &period_end)?;
    let (file_exists, existing_content, existing_file_mtime) = if target_path.exists() {
        let metadata = fs::metadata(&target_path)
            .map_err(|error| format!("无法读取目标文件信息 {}: {error}", target_path.display()))?;
        let content = fs::read_to_string(&target_path)
            .map_err(|error| format!("无法读取目标文件 {}: {error}", target_path.display()))?;
        (
            true,
            content,
            metadata.modified().ok().map(system_time_millis),
        )
    } else {
        (false, String::new(), None)
    };
    let run_id = Uuid::now_v7().to_string();
    let now = now_millis();
    let result = ReportWritePreviewResult {
        run_id: run_id.clone(),
        report_id: request.report_id.clone(),
        report_version: version,
        target_path: target_path.display().to_string(),
        file_exists,
        existing_content_hash: file_exists.then(|| content_hash(&existing_content)),
        existing_file_mtime,
        existing_content,
        report_markdown: markdown,
        action: if file_exists { "update" } else { "create" }.to_string(),
    };
    connection
        .execute(
            "INSERT INTO sync_runs(
               id, workspace_id, provider, operation_type, state, request_json, preview_json, created_at
             ) VALUES (?1, ?2, 'obsidian', 'report_write', 'preview', ?3, ?4, ?5)",
            params![
                run_id,
                workspace_id,
                serde_json::to_string(&request).map_err(|error| error.to_string())?,
                serde_json::to_string(&result).map_err(|error| error.to_string())?,
                now
            ],
        )
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "INSERT INTO sync_items(
               id, workspace_id, run_id, entity_type, entity_id, action, state, before_json, after_json
             ) VALUES (?1, ?2, ?3, 'report', ?4, 'write_file', 'pending', ?5, ?6)",
            params![
                Uuid::now_v7().to_string(),
                workspace_id,
                result.run_id,
                result.report_id,
                serde_json::to_string(&result.existing_content).map_err(|error| error.to_string())?,
                serde_json::to_string(&result.report_markdown).map_err(|error| error.to_string())?
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(result)
}

pub fn execute_report_write(
    database: &Database,
    request: ReportWriteExecuteRequest,
) -> Result<ReportWriteExecuteResult, String> {
    if !matches!(request.strategy.as_str(), "overwrite" | "append") {
        return Err("VALIDATION_ERROR: 写入方式必须是 overwrite 或 append".to_string());
    }
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let device_id: String = connection
        .query_row(
            "SELECT id FROM devices WHERE workspace_id = ?1 AND revoked_at IS NULL ORDER BY created_at LIMIT 1",
            [&workspace_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let preview_json: String = connection
        .query_row(
            "SELECT preview_json FROM sync_runs
             WHERE id = ?1 AND workspace_id = ?2 AND provider = 'obsidian'
               AND operation_type = 'report_write' AND state = 'preview'",
            params![request.run_id, workspace_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "NOT_FOUND: 写入预览不存在或已经执行".to_string())?;
    let preview: ReportWritePreviewResult =
        serde_json::from_str(&preview_json).map_err(|error| error.to_string())?;
    let current_report: (String, i64) = connection
        .query_row(
            "SELECT markdown_content, version FROM reports
             WHERE id = ?1 AND workspace_id = ?2 AND deleted_at IS NULL",
            params![preview.report_id, workspace_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "NOT_FOUND: 报告不存在".to_string())?;
    if current_report.1 != preview.report_version || current_report.0 != preview.report_markdown {
        return Err("REPORT_CHANGED: 报告在预览后已变化，请重新预览".to_string());
    }
    let target_path = PathBuf::from(&preview.target_path);
    let current_file = if target_path.exists() {
        let metadata = fs::metadata(&target_path)
            .map_err(|error| format!("无法读取目标文件信息 {}: {error}", target_path.display()))?;
        let content = fs::read_to_string(&target_path)
            .map_err(|error| format!("无法读取目标文件 {}: {error}", target_path.display()))?;
        (
            true,
            Some(content_hash(&content)),
            metadata.modified().ok().map(system_time_millis),
            content,
        )
    } else {
        (false, None, None, String::new())
    };
    if current_file.0 != preview.file_exists
        || current_file.1 != preview.existing_content_hash
        || current_file.2 != preview.existing_file_mtime
    {
        return Err("EXTERNAL_FILE_CHANGED: Obsidian 文件在预览后已变化，请重新预览".to_string());
    }
    let output =
        if request.strategy == "append" && current_file.0 && !current_file.3.trim().is_empty() {
            format!(
                "{}\n\n---\n\n{}",
                current_file.3.trim_end(),
                preview.report_markdown.trim_start()
            )
        } else {
            preview.report_markdown.clone()
        };
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("无法创建 Obsidian 目录 {}: {error}", parent.display()))?;
    }
    fs::write(&target_path, &output)
        .map_err(|error| format!("无法写入 Obsidian 文件 {}: {error}", target_path.display()))?;
    let metadata = fs::metadata(&target_path)
        .map_err(|error| format!("无法读取写入结果 {}: {error}", target_path.display()))?;
    let written_at = now_millis();
    let output_mtime = metadata
        .modified()
        .ok()
        .map(system_time_millis)
        .unwrap_or(written_at);
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "UPDATE sync_runs SET state = 'succeeded', success_count = 1, started_at = ?1, completed_at = ?1 WHERE id = ?2",
            params![written_at, request.run_id],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "UPDATE sync_items SET state = 'succeeded' WHERE run_id = ?1",
            [&request.run_id],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO local_report_outputs(report_id, device_id, output_path, output_content_hash, output_file_mtime, written_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(report_id, device_id) DO UPDATE SET
               output_path = excluded.output_path,
               output_content_hash = excluded.output_content_hash,
               output_file_mtime = excluded.output_file_mtime,
               written_at = excluded.written_at",
            params![
                preview.report_id,
                device_id,
                preview.target_path,
                content_hash(&output),
                output_mtime,
                written_at
            ],
        )
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(ReportWriteExecuteResult {
        run_id: request.run_id,
        report_id: preview.report_id,
        target_path: preview.target_path,
        strategy: request.strategy,
        written_at,
    })
}

pub fn preview_plan_from_obsidian(
    database: &Database,
    request: PlanImportPreviewRequest,
) -> Result<PlanImportPreviewResult, String> {
    validate_date(&request.source_date)?;
    validate_date(&request.target_date)?;
    let connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    ensure_subject(&connection, &workspace_id, &request.subject_id)?;
    let (root_path, pattern) = obsidian_path_settings(&connection)?;
    if root_path.trim().is_empty() {
        return Err("INTEGRATION_NOT_CONFIGURED: 请先设置 Obsidian 根目录".to_string());
    }
    let source_path = resolve_daily_path(&root_path, &pattern, &request.source_date)?;
    let (source_exists, raw_markdown, source_mtime) = if source_path.exists() {
        let metadata = fs::metadata(&source_path).map_err(|error| {
            format!(
                "无法读取 Obsidian 文件信息 {}: {error}",
                source_path.display()
            )
        })?;
        let markdown = fs::read_to_string(&source_path).map_err(|error| {
            format!("无法读取 Obsidian 日报 {}: {error}", source_path.display())
        })?;
        (
            true,
            markdown,
            metadata.modified().ok().map(system_time_millis),
        )
    } else {
        (false, String::new(), None)
    };
    let content_hash = source_exists.then(|| content_hash(&raw_markdown));
    let mut parsed = parse_plan_markdown(&raw_markdown);
    if !source_exists {
        parsed
            .warnings
            .push(format!("源日报不存在：{}", source_path.display()));
    }
    let batch_id = Uuid::now_v7().to_string();
    let now = now_millis();
    let warnings_json =
        serde_json::to_string(&parsed.warnings).map_err(|error| error.to_string())?;
    connection
        .execute(
            "INSERT INTO plan_import_batches(
               id, workspace_id, target_date, subject_id, source_path, source_mtime,
               source_content_hash, state, raw_markdown, warnings_json, created_at, updated_at, version
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'preview', ?8, ?9, ?10, ?10, 1)",
            params![
                batch_id,
                workspace_id,
                request.target_date,
                request.subject_id,
                source_path.display().to_string(),
                source_mtime,
                content_hash,
                raw_markdown,
                warnings_json,
                now
            ],
        )
        .map_err(|error| error.to_string())?;

    let mut client_to_item = HashMap::new();
    let mut items = Vec::new();
    for item in parsed.items {
        let id = Uuid::now_v7().to_string();
        let parent_id = item
            .parent_client_id
            .as_ref()
            .and_then(|client_id| client_to_item.get(client_id).cloned());
        connection
            .execute(
                "INSERT INTO plan_import_items(
                   id, workspace_id, batch_id, parent_item_id, title, estimate_minutes,
                   sort_order, source_line_no, source_text, parse_status
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    id,
                    workspace_id,
                    batch_id,
                    parent_id,
                    item.title,
                    item.estimate_minutes,
                    item.sort_order,
                    item.source_line_no as i64,
                    item.source_text,
                    item.parse_status
                ],
            )
            .map_err(|error| error.to_string())?;
        client_to_item.insert(item.client_id, id.clone());
        items.push(PlanImportPreviewItem {
            id,
            parent_id,
            title: item.title,
            estimate_minutes: item.estimate_minutes,
            sort_order: item.sort_order,
            source_line_no: item.source_line_no,
            source_text: item.source_text,
            parse_status: item.parse_status,
        });
    }
    Ok(PlanImportPreviewResult {
        batch_id,
        source_path: source_path.display().to_string(),
        source_exists,
        source_mtime,
        source_content_hash: content_hash,
        raw_markdown,
        items,
        warnings: parsed.warnings,
        version: 1,
    })
}

pub fn confirm_plan_import(
    database: &Database,
    request: PlanImportConfirmRequest,
) -> Result<PlanImportConfirmResult, String> {
    let mut connection = database.open()?;
    let batch: Option<(String, String, String, i64)> = connection
        .query_row(
            "SELECT workspace_id, subject_id, target_date, version FROM plan_import_batches WHERE id = ?1",
            [&request.batch_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some((workspace_id, subject_id, target_date, version)) = batch else {
        return Err("NOT_FOUND: 导入批次不存在".to_string());
    };
    if version != request.expected_version {
        return Err("VERSION_CONFLICT: 导入预览已经变化".to_string());
    }
    let existing = load_confirmed_tasks(&connection, &request.batch_id)?;
    if !existing.is_empty() {
        return Ok(PlanImportConfirmResult {
            batch_id: request.batch_id,
            tasks: existing,
            already_confirmed: true,
        });
    }
    let selected = request
        .items
        .into_iter()
        .filter(|item| item.selected && !item.title.trim().is_empty())
        .map(|item| (item.import_item_id.clone(), item))
        .collect::<HashMap<_, _>>();
    let import_items = load_import_items(&connection, &request.batch_id)?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let now = now_millis();
    let mut item_to_task = HashMap::new();
    let mut created_ids = Vec::new();
    for item in import_items {
        let Some(selection) = selected.get(&item.id) else {
            continue;
        };
        if item.parse_status != "recognized" {
            continue;
        }
        if selection
            .estimate_minutes
            .is_some_and(|minutes| minutes <= 0)
        {
            return Err("VALIDATION_ERROR: 预计用时必须大于 0 分钟".to_string());
        }
        let parent_id = item
            .parent_item_id
            .as_ref()
            .and_then(|parent_item_id| item_to_task.get(parent_item_id).cloned());
        let task_id = Uuid::now_v7().to_string();
        transaction
            .execute(
                "INSERT INTO tasks(
                   id, workspace_id, subject_id, parent_id, title, status, planned_date,
                   estimate_minutes, source_type, source_ref, sort_order, created_at, updated_at, version
                 ) VALUES (?1, ?2, ?3, ?4, ?5, 'open', ?6, ?7, 'obsidian_import', ?8, ?9, ?10, ?10, 1)",
                params![
                    task_id,
                    workspace_id,
                    subject_id,
                    parent_id,
                    selection.title.trim(),
                    target_date,
                    selection.estimate_minutes,
                    item.id,
                    item.sort_order,
                    now
                ],
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "UPDATE plan_import_items SET confirmed_task_id = ?1 WHERE id = ?2",
                params![task_id, item.id],
            )
            .map_err(|error| error.to_string())?;
        item_to_task.insert(item.id, task_id.clone());
        created_ids.push(task_id);
    }
    transaction
        .execute(
            "UPDATE plan_import_batches SET state = 'confirmed', updated_at = ?1, version = version + 1
             WHERE id = ?2 AND version = ?3",
            params![now, request.batch_id, request.expected_version],
        )
        .map_err(|error| error.to_string())?;
    bump_revision(&transaction)?;
    transaction.commit().map_err(|error| error.to_string())?;
    let connection = database.open()?;
    let tasks = created_ids
        .iter()
        .map(|id| load_task(&connection, id))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PlanImportConfirmResult {
        batch_id: request.batch_id,
        tasks,
        already_confirmed: false,
    })
}

struct ImportItemRow {
    id: String,
    parent_item_id: Option<String>,
    sort_order: i64,
    parse_status: String,
}

fn load_import_items(
    connection: &rusqlite::Connection,
    batch_id: &str,
) -> Result<Vec<ImportItemRow>, String> {
    let mut statement = connection
        .prepare(
            "SELECT id, parent_item_id, sort_order, parse_status
             FROM plan_import_items WHERE batch_id = ?1 ORDER BY source_line_no, sort_order",
        )
        .map_err(|error| error.to_string())?;
    let result = statement
        .query_map([batch_id], |row| {
            Ok(ImportItemRow {
                id: row.get(0)?,
                parent_item_id: row.get(1)?,
                sort_order: row.get(2)?,
                parse_status: row.get(3)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string());
    result
}

fn load_confirmed_tasks(
    connection: &rusqlite::Connection,
    batch_id: &str,
) -> Result<Vec<TaskDto>, String> {
    let mut statement = connection
        .prepare(
            "SELECT t.id, t.subject_id, t.parent_id, t.title, t.status, t.planned_date,
                    t.estimate_minutes, t.note, t.project_name, t.solution_name, t.sort_order, t.version
             FROM plan_import_items i JOIN tasks t ON t.id = i.confirmed_task_id
             WHERE i.batch_id = ?1 ORDER BY i.source_line_no",
        )
        .map_err(|error| error.to_string())?;
    let result = statement
        .query_map([batch_id], task_from_row)
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string());
    result
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

fn load_task(connection: &rusqlite::Connection, id: &str) -> Result<TaskDto, String> {
    connection
        .query_row(
            "SELECT id, subject_id, parent_id, title, status, planned_date,
                    estimate_minutes, note, project_name, solution_name, sort_order, version
             FROM tasks WHERE id = ?1 AND deleted_at IS NULL",
            [id],
            task_from_row,
        )
        .map_err(|error| error.to_string())
}

fn obsidian_path_settings(connection: &rusqlite::Connection) -> Result<(String, String), String> {
    let root = read_device_string(connection, "obsidian_root_path")?.unwrap_or_default();
    let pattern = read_device_string(connection, "obsidian_daily_path_pattern")?
        .unwrap_or_else(|| "工作日报/{date}.md".to_string());
    Ok((root, pattern))
}

fn resolve_report_path(
    connection: &rusqlite::Connection,
    report_type: &str,
    period_start: &str,
    period_end: &str,
) -> Result<PathBuf, String> {
    let (root, daily_pattern) = obsidian_path_settings(connection)?;
    if root.trim().is_empty() {
        return Err("INTEGRATION_NOT_CONFIGURED: 请先设置 Obsidian 根目录".to_string());
    }
    let relative = match report_type {
        "daily" => daily_pattern.replace("{date}", period_start),
        "weekly" => format!("工作周报/{period_start}_{period_end}.md"),
        "monthly" => format!("工作月报/{period_start}_{period_end}.md"),
        _ => return Err("VALIDATION_ERROR: 不支持的报告类型".to_string()),
    };
    let relative_path = Path::new(&relative);
    if relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err("VALIDATION_ERROR: 报告路径必须位于 Obsidian 根目录内".to_string());
    }
    Ok(Path::new(&root).join(relative_path))
}

fn read_device_string(
    connection: &rusqlite::Connection,
    key: &str,
) -> Result<Option<String>, String> {
    let value: Option<String> = connection
        .query_row(
            "SELECT value_json FROM device_settings WHERE key = ?1",
            [key],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    value
        .map(|value| serde_json::from_str::<String>(&value).map_err(|error| error.to_string()))
        .transpose()
}

fn resolve_daily_path(root: &str, pattern: &str, date: &str) -> Result<PathBuf, String> {
    let relative = pattern.replace("{date}", date);
    let relative_path = Path::new(&relative);
    if relative_path.is_absolute() {
        return Err("VALIDATION_ERROR: 日报路径规则必须是 Obsidian 根目录下的相对路径".to_string());
    }
    if relative_path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err("VALIDATION_ERROR: 日报路径规则不能跳出 Obsidian 根目录".to_string());
    }
    Ok(Path::new(root).join(relative_path))
}

fn validate_date(value: &str) -> Result<(), String> {
    let bytes = value.as_bytes();
    if bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
    {
        Ok(())
    } else {
        Err(format!("VALIDATION_ERROR: 非法日期 {value}"))
    }
}

fn content_hash(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

fn system_time_millis(value: SystemTime) -> i64 {
    value
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn now_millis() -> i64 {
    system_time_millis(SystemTime::now())
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

fn bump_revision(connection: &rusqlite::Connection) -> Result<(), String> {
    connection
        .execute(
            "UPDATE app_metadata SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT) WHERE key = 'global_revision'",
            [],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn obsidian_plan_import_preview(
    database: tauri::State<'_, Database>,
    request: PlanImportPreviewRequest,
) -> Result<PlanImportPreviewResult, String> {
    preview_plan_from_obsidian(&database, request)
}

#[tauri::command]
pub fn obsidian_plan_import_confirm(
    database: tauri::State<'_, Database>,
    request: PlanImportConfirmRequest,
) -> Result<PlanImportConfirmResult, String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let result = confirm_plan_import(&database, request)?;
    for task in &result.tasks {
        crate::cloud_sync::enqueue_entity_deferred(
            &database,
            "obsidian_plan_import",
            "task",
            Some(&task.id),
            None,
            None,
        )?;
    }
    crate::cloud_sync::flush_if_online(&database)?;
    Ok(result)
}

#[tauri::command]
pub fn obsidian_report_write_preview(
    database: tauri::State<'_, Database>,
    request: ReportWritePreviewRequest,
) -> Result<ReportWritePreviewResult, String> {
    preview_report_write(&database, request)
}

#[tauri::command]
pub fn obsidian_report_write_execute(
    database: tauri::State<'_, Database>,
    request: ReportWriteExecuteRequest,
) -> Result<ReportWriteExecuteResult, String> {
    execute_report_write(&database, request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reports::{create_report, ReportCreateRequest};
    use crate::settings::{update_setting, SettingsScope, SettingsUpdate};
    use crate::tasks::{set_task_status, TaskStatusRequest};
    use crate::time_tracking::{start_timer, stop_timer, TimerStartRequest, TimerStopRequest};
    use chrono::Duration;
    use tempfile::tempdir;

    #[test]
    fn previews_and_confirms_obsidian_plan_idempotently() {
        let directory = tempdir().unwrap();
        let vault = directory.path().join("vault");
        fs::create_dir_all(vault.join("工作日报")).unwrap();
        fs::write(
            vault.join("工作日报/2026-09-23.md"),
            "# 日报\n\n## 明日计划：\n\n1. 运维相关\n   - 处理异常 <预计：0.5h>\n这行无法识别\n",
        )
        .unwrap();
        let database = Database::initialize_at(directory.path().join("obsidian.sqlite3")).unwrap();
        update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "obsidian_root_path".to_string(),
                value: serde_json::json!(vault.display().to_string()),
            },
        )
        .unwrap();
        let connection = database.open().unwrap();
        let subject_id: String = connection
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        drop(connection);
        let preview = preview_plan_from_obsidian(
            &database,
            PlanImportPreviewRequest {
                source_date: "2026-09-23".to_string(),
                target_date: "2026-09-24".to_string(),
                subject_id,
            },
        )
        .unwrap();
        assert!(preview.source_exists);
        assert_eq!(preview.items.len(), 3);
        assert_eq!(preview.items[1].estimate_minutes, Some(30));
        assert_eq!(preview.items[2].parse_status, "unrecognized");
        assert_eq!(preview.warnings.len(), 1);
        let selections = preview
            .items
            .iter()
            .map(|item| PlanImportConfirmItem {
                import_item_id: item.id.clone(),
                selected: true,
                title: item.title.clone(),
                estimate_minutes: item.estimate_minutes,
            })
            .collect();
        let confirmed = confirm_plan_import(
            &database,
            PlanImportConfirmRequest {
                batch_id: preview.batch_id.clone(),
                items: selections,
                expected_version: 1,
            },
        )
        .unwrap();
        assert_eq!(confirmed.tasks.len(), 2);
        let repeated = confirm_plan_import(
            &database,
            PlanImportConfirmRequest {
                batch_id: preview.batch_id,
                items: Vec::new(),
                expected_version: 2,
            },
        )
        .unwrap();
        assert!(repeated.already_confirmed);
        assert_eq!(repeated.tasks.len(), 2);
    }

    #[test]
    fn missing_source_returns_empty_preview() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(directory.path().join("missing.sqlite3")).unwrap();
        update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "obsidian_root_path".to_string(),
                value: serde_json::json!(directory.path().display().to_string()),
            },
        )
        .unwrap();
        let connection = database.open().unwrap();
        let subject_id: String = connection
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        drop(connection);
        let preview = preview_plan_from_obsidian(
            &database,
            PlanImportPreviewRequest {
                source_date: "2026-09-22".to_string(),
                target_date: "2026-09-23".to_string(),
                subject_id,
            },
        )
        .unwrap();
        assert!(!preview.source_exists);
        assert!(preview.items.is_empty());
        assert_eq!(preview.warnings.len(), 1);
    }

    #[test]
    fn writes_report_after_preview_and_rejects_external_file_changes() {
        let directory = tempdir().unwrap();
        let vault = directory.path().join("vault");
        fs::create_dir_all(&vault).unwrap();
        let database = Database::initialize_at(directory.path().join("write.sqlite3")).unwrap();
        update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "obsidian_root_path".to_string(),
                value: serde_json::json!(vault.display().to_string()),
            },
        )
        .unwrap();
        let connection = database.open().unwrap();
        let subject_id: String = connection
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let task_id: String = connection
            .query_row(
                "SELECT id FROM tasks WHERE parent_id IS NOT NULL LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        drop(connection);
        let report = create_report(
            &database,
            ReportCreateRequest {
                report_type: "daily".to_string(),
                reference_date: "2026-09-24".to_string(),
                subject_id,
                task_ids: vec![task_id],
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        let preview = preview_report_write(
            &database,
            ReportWritePreviewRequest {
                report_id: report.id.clone(),
                expected_version: report.version,
            },
        )
        .unwrap();
        assert!(!preview.file_exists);
        let written = execute_report_write(
            &database,
            ReportWriteExecuteRequest {
                run_id: preview.run_id,
                strategy: "overwrite".to_string(),
            },
        )
        .unwrap();
        assert!(Path::new(&written.target_path).exists());

        let changed_preview = preview_report_write(
            &database,
            ReportWritePreviewRequest {
                report_id: report.id,
                expected_version: report.version,
            },
        )
        .unwrap();
        fs::write(&changed_preview.target_path, "外部修改").unwrap();
        let error = execute_report_write(
            &database,
            ReportWriteExecuteRequest {
                run_id: changed_preview.run_id,
                strategy: "append".to_string(),
            },
        )
        .unwrap_err();
        assert!(error.starts_with("EXTERNAL_FILE_CHANGED:"));
    }

    #[test]
    fn completes_import_timer_allocation_and_daily_report_flow() {
        let directory = tempdir().unwrap();
        let vault = directory.path().join("vault");
        fs::create_dir_all(vault.join("工作日报")).unwrap();
        let target_date = chrono::Local::now().date_naive();
        let source_date = target_date - Duration::days(1);
        let source_date_text = source_date.format("%Y-%m-%d").to_string();
        let target_date_text = target_date.format("%Y-%m-%d").to_string();
        fs::write(
            vault
                .join("工作日报")
                .join(format!("{source_date_text}.md")),
            "【晓健】昨日工作记录\n\n## 明日计划：\n\n1. 运维相关\n   - 处理备份异常 <预计：0.5h>\n   - 整理同步方案\n",
        )
        .unwrap();
        let database = Database::initialize_at(directory.path().join("flow.sqlite3")).unwrap();
        update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "obsidian_root_path".to_string(),
                value: serde_json::json!(vault.display().to_string()),
            },
        )
        .unwrap();
        let connection = database.open().unwrap();
        let subject_id: String = connection
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        drop(connection);

        let preview = preview_plan_from_obsidian(
            &database,
            PlanImportPreviewRequest {
                source_date: source_date_text,
                target_date: target_date_text.clone(),
                subject_id: subject_id.clone(),
            },
        )
        .unwrap();
        let confirmed = confirm_plan_import(
            &database,
            PlanImportConfirmRequest {
                batch_id: preview.batch_id,
                items: preview
                    .items
                    .iter()
                    .map(|item| PlanImportConfirmItem {
                        import_item_id: item.id.clone(),
                        selected: item.parse_status == "recognized",
                        title: item.title.clone(),
                        estimate_minutes: item.estimate_minutes,
                    })
                    .collect(),
                expected_version: preview.version,
            },
        )
        .unwrap();
        let leaf = confirmed
            .tasks
            .iter()
            .find(|task| task.parent_id.is_some())
            .unwrap();
        let started = start_timer(
            &database,
            TimerStartRequest {
                task_id: Some(leaf.id.clone()),
                note: Some("白天处理备份异常".to_string()),
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        let connection = database.open().unwrap();
        connection
            .execute(
                "UPDATE time_segments SET started_at = started_at - 1800000 WHERE entry_id = ?1",
                [&started.id],
            )
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
        assert_eq!(stopped.allocated_minutes, stopped.settlement_minutes);
        let completed = set_task_status(
            &database,
            TaskStatusRequest {
                id: leaf.id.clone(),
                expected_version: leaf.version,
                done: true,
                occurrence_date: None,
                occurrence_expected_version: None,
            },
        )
        .unwrap();
        assert_eq!(completed.status, "done");
        let report = create_report(
            &database,
            ReportCreateRequest {
                report_type: "daily".to_string(),
                reference_date: target_date_text,
                subject_id,
                task_ids: confirmed.tasks.iter().map(|task| task.id.clone()).collect(),
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        assert!(report.markdown.contains("运维相关"));
        assert!(report.markdown.contains("🟢 处理备份异常"));
        assert!(report.markdown.contains("实际：30min"));
    }
}
