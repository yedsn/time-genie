use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{Datelike, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use rusqlite::{params, Connection};
use rust_xlsxwriter::{Color, ExcelDateTime, Format, FormatAlign, Workbook};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::database::Database;

const WORKSHEET_NAME: &str = "工时归属";
const EXPORT_HEADERS: [&str; 6] = ["工作日期", "主体", "事项", "用时（分钟）", "时间段", "备注"];

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TimeAllocationExportRequest {
    pub start_date: String,
    pub end_date: String,
    #[serde(default)]
    pub include_special_time: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeAllocationExportExecuteRequest {
    pub start_date: String,
    pub end_date: String,
    #[serde(default)]
    pub include_special_time: bool,
    pub target_path: String,
}

impl From<&TimeAllocationExportExecuteRequest> for TimeAllocationExportRequest {
    fn from(value: &TimeAllocationExportExecuteRequest) -> Self {
        Self {
            start_date: value.start_date.clone(),
            end_date: value.end_date.clone(),
            include_special_time: value.include_special_time,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TimeAllocationExportRow {
    pub work_date: String,
    pub subject: String,
    pub task: String,
    pub minutes: i64,
    pub time_range: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TimeAllocationExportPreview {
    pub row_count: usize,
    pub total_minutes: i64,
    pub default_file_name: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TimeAllocationExportResult {
    pub row_count: usize,
    pub total_minutes: i64,
    pub path: String,
}

#[derive(Debug, Clone)]
struct TaskSnapshot {
    parent_id: Option<String>,
    title: String,
    subject: String,
    deleted: bool,
}

#[derive(Debug)]
struct BuiltExport {
    rows: Vec<TimeAllocationExportRow>,
    total_minutes: i64,
}

fn validate_request(
    request: &TimeAllocationExportRequest,
) -> Result<(NaiveDate, NaiveDate), String> {
    let start = NaiveDate::parse_from_str(request.start_date.trim(), "%Y-%m-%d")
        .map_err(|_| "VALIDATION_ERROR: 开始日期格式无效".to_string())?;
    let end = NaiveDate::parse_from_str(request.end_date.trim(), "%Y-%m-%d")
        .map_err(|_| "VALIDATION_ERROR: 结束日期格式无效".to_string())?;
    if start > end {
        return Err("VALIDATION_ERROR: 开始日期不能晚于结束日期".to_string());
    }
    Ok((start, end))
}

fn current_workspace(connection: &Connection) -> Result<(String, Tz), String> {
    let (workspace_id, timezone): (String, String) = connection
        .query_row(
            "SELECT id, timezone FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| error.to_string())?;
    Ok((
        workspace_id,
        crate::work_calendar::validate_timezone(&timezone)?,
    ))
}

fn load_task_snapshots(
    connection: &Connection,
    workspace_id: &str,
) -> Result<HashMap<String, TaskSnapshot>, String> {
    let mut statement = connection
        .prepare(
            "SELECT t.id, t.parent_id,
                    CASE WHEN t.deleted_at IS NULL THEN t.title ELSE '已删除事项' END,
                    CASE WHEN s.id IS NULL OR s.deleted_at IS NOT NULL THEN '已删除主体' ELSE s.name END,
                    t.deleted_at IS NOT NULL
             FROM tasks t
             LEFT JOIN subjects s ON s.id = t.subject_id
             WHERE t.workspace_id = ?1",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([workspace_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                TaskSnapshot {
                    parent_id: row.get(1)?,
                    title: row.get(2)?,
                    subject: row.get(3)?,
                    deleted: row.get(4)?,
                },
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<HashMap<_, _>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(rows)
}

fn task_path(task_id: &str, tasks: &HashMap<String, TaskSnapshot>) -> String {
    let Some(task) = tasks.get(task_id) else {
        return "已删除事项".to_string();
    };
    if task.deleted {
        return "已删除事项".to_string();
    }
    let mut parts = vec![task.title.clone()];
    let mut parent_id = task.parent_id.clone();
    let mut guard = 0;
    while let Some(id) = parent_id {
        guard += 1;
        if guard > 64 {
            break;
        }
        let Some(parent) = tasks.get(&id) else {
            break;
        };
        parts.push(parent.title.clone());
        parent_id = parent.parent_id.clone();
    }
    parts.reverse();
    parts.join(" / ")
}

fn task_subject(task_id: &str, tasks: &HashMap<String, TaskSnapshot>) -> String {
    tasks
        .get(task_id)
        .map(|task| task.subject.clone())
        .unwrap_or_else(|| "已删除主体".to_string())
}

fn format_time(timezone: Tz, timestamp_millis: i64) -> Result<String, String> {
    let value = Utc
        .timestamp_millis_opt(timestamp_millis)
        .single()
        .ok_or_else(|| "VALIDATION_ERROR: 时间记录包含无效时间戳".to_string())?;
    Ok(value.with_timezone(&timezone).format("%H:%M").to_string())
}

fn display_end_at(
    connection: &Connection,
    entry: &crate::time_tracking::TimeEntryDto,
    snapshot_at: i64,
) -> Result<i64, String> {
    match entry.state.as_str() {
        "running" => Ok(snapshot_at.max(entry.started_at)),
        "paused" => connection
            .query_row(
                "SELECT MAX(ended_at) FROM time_segments WHERE entry_id = ?1 AND ended_at IS NOT NULL",
                [&entry.id],
                |row| row.get::<_, Option<i64>>(0),
            )
            .map_err(|error| error.to_string())
            .map(|value| {
                value.unwrap_or_else(|| {
                    entry
                        .started_at
                        .saturating_add(entry.duration_seconds.saturating_mul(1_000))
                })
            }),
        _ => Ok(entry.ended_at.unwrap_or_else(|| {
            entry
                .started_at
                .saturating_add(entry.duration_seconds.saturating_mul(1_000))
        })),
    }
}

fn entry_time_range(
    connection: &Connection,
    timezone: Tz,
    entry: &crate::time_tracking::TimeEntryDto,
    snapshot_at: i64,
) -> Result<String, String> {
    let started = format_time(timezone, entry.started_at)?;
    let ended = format_time(timezone, display_end_at(connection, entry, snapshot_at)?)?;
    Ok(format!("{started}-{ended}"))
}

fn combine_notes(entry_note: Option<&str>, allocation_note: Option<&str>) -> String {
    let mut parts = Vec::new();
    if let Some(note) = entry_note.map(str::trim).filter(|note| !note.is_empty()) {
        parts.push(format!("计时：{note}"));
    }
    if let Some(note) = allocation_note
        .map(str::trim)
        .filter(|note| !note.is_empty())
    {
        parts.push(format!("归属：{note}"));
    }
    parts.join("；")
}

fn special_note(entry_note: Option<&str>, kind: &str, state: &str) -> String {
    let mut parts = Vec::new();
    if let Some(note) = entry_note.map(str::trim).filter(|note| !note.is_empty()) {
        parts.push(format!("计时：{note}"));
    }
    parts.push(kind.to_string());
    match state {
        "running" => parts.push("进行中".to_string()),
        "paused" => parts.push("已暂停".to_string()),
        _ => {}
    }
    parts.join("；")
}

fn build_export(
    connection: &Connection,
    request: &TimeAllocationExportRequest,
    snapshot_at: i64,
) -> Result<BuiltExport, String> {
    validate_request(request)?;
    let (workspace_id, timezone) = current_workspace(connection)?;
    let tasks = load_task_snapshots(connection, &workspace_id)?;
    let mut statement = connection
        .prepare(
            "SELECT id FROM time_entries
             WHERE workspace_id = ?1 AND work_date BETWEEN ?2 AND ?3 AND deleted_at IS NULL
             ORDER BY work_date, started_at, id",
        )
        .map_err(|error| error.to_string())?;
    let entry_ids = statement
        .query_map(
            params![workspace_id, request.start_date, request.end_date],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;

    let mut rows = Vec::new();
    for entry_id in entry_ids {
        let entry = crate::time_tracking::load_entry_at(connection, &entry_id, snapshot_at)?;
        let time_range = entry_time_range(connection, timezone, &entry, snapshot_at)?;

        if entry.kind != "break" && entry.state == "ended" {
            for allocation in entry
                .allocations
                .iter()
                .filter(|allocation| allocation.minutes > 0)
            {
                rows.push(TimeAllocationExportRow {
                    work_date: entry.work_date.clone(),
                    subject: task_subject(&allocation.task_id, &tasks),
                    task: task_path(&allocation.task_id, &tasks),
                    minutes: allocation.minutes,
                    time_range: time_range.clone(),
                    note: combine_notes(entry.note.as_deref(), allocation.note.as_deref()),
                });
            }
        }

        if !request.include_special_time {
            continue;
        }
        if entry.kind == "break" {
            if entry.settlement_minutes > 0 {
                rows.push(TimeAllocationExportRow {
                    work_date: entry.work_date.clone(),
                    subject: "特殊时间".to_string(),
                    task: "休息".to_string(),
                    minutes: entry.settlement_minutes,
                    time_range,
                    note: special_note(entry.note.as_deref(), "休息", &entry.state),
                });
            }
            continue;
        }
        let allocated_minutes = if entry.state == "ended" {
            entry
                .allocations
                .iter()
                .filter(|allocation| allocation.minutes > 0)
                .map(|allocation| allocation.minutes)
                .sum::<i64>()
        } else {
            0
        };
        let remaining = (entry.settlement_minutes - allocated_minutes).max(0);
        if remaining > 0 {
            rows.push(TimeAllocationExportRow {
                work_date: entry.work_date.clone(),
                subject: "特殊时间".to_string(),
                task: "未归属".to_string(),
                minutes: remaining,
                time_range,
                note: special_note(entry.note.as_deref(), "未归属", &entry.state),
            });
        }
    }
    let mut adjustment_statement = connection
        .prepare(
            "SELECT a.work_date,a.task_id,a.delta_minutes,a.note
             FROM actual_time_adjustments a
             WHERE a.workspace_id=?1 AND a.work_date BETWEEN ?2 AND ?3 AND a.deleted_at IS NULL
             ORDER BY a.work_date,a.created_at,a.id",
        )
        .map_err(|error| error.to_string())?;
    for row in adjustment_statement
        .query_map(
            params![workspace_id, request.start_date, request.end_date],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .map_err(|error| error.to_string())?
    {
        let (work_date, task_id, minutes, note) = row.map_err(|error| error.to_string())?;
        let note = note
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        rows.push(TimeAllocationExportRow {
            work_date,
            subject: task_subject(&task_id, &tasks),
            task: task_path(&task_id, &tasks),
            minutes,
            time_range: String::new(),
            note: note
                .map(|value| format!("实际调整：{value}"))
                .unwrap_or_else(|| "实际调整".to_string()),
        });
    }
    let total_minutes = rows.iter().map(|row| row.minutes).sum();
    Ok(BuiltExport {
        rows,
        total_minutes,
    })
}

fn default_file_name(request: &TimeAllocationExportRequest) -> String {
    format!(
        "TimeGenie_工时归属_{}_{}.xlsx",
        request.start_date, request.end_date
    )
}

fn preview_with_snapshot(
    database: &Database,
    request: &TimeAllocationExportRequest,
    snapshot_at: i64,
) -> Result<TimeAllocationExportPreview, String> {
    let connection = database.open()?;
    let built = build_export(&connection, request, snapshot_at)?;
    Ok(TimeAllocationExportPreview {
        row_count: built.rows.len(),
        total_minutes: built.total_minutes,
        default_file_name: default_file_name(request),
    })
}

fn preview_direct(
    database: &Database,
    request: &TimeAllocationExportRequest,
) -> Result<TimeAllocationExportPreview, String> {
    validate_request(request)?;
    let rows = crate::supabase::direct_cloud_time_allocation_export(database, request.clone())?;
    Ok(TimeAllocationExportPreview {
        row_count: rows.len(),
        total_minutes: rows.iter().map(|row| row.minutes).sum(),
        default_file_name: default_file_name(request),
    })
}

fn workbook_bytes(rows: &[TimeAllocationExportRow], total_minutes: i64) -> Result<Vec<u8>, String> {
    let mut workbook = Workbook::new();
    let header_format = Format::new()
        .set_bold()
        .set_font_color(Color::White)
        .set_background_color(Color::RGB(0x3F5E4F))
        .set_align(FormatAlign::Center)
        .set_align(FormatAlign::VerticalCenter);
    let date_format = Format::new().set_num_format("yyyy-mm-dd");
    let minutes_format = Format::new()
        .set_num_format("0")
        .set_align(FormatAlign::Center);
    let wrap_format = Format::new()
        .set_text_wrap()
        .set_align(FormatAlign::VerticalCenter);
    let total_format = Format::new()
        .set_bold()
        .set_background_color(Color::RGB(0xE8EFEA));

    {
        let worksheet = workbook.add_worksheet();
        worksheet
            .set_name(WORKSHEET_NAME)
            .map_err(|error| error.to_string())?;
        worksheet
            .set_freeze_panes(1, 0)
            .map_err(|error| error.to_string())?;
        for (column, header) in EXPORT_HEADERS.iter().enumerate() {
            worksheet
                .write_string_with_format(0, column as u16, *header, &header_format)
                .map_err(|error| error.to_string())?;
        }
        worksheet
            .set_row_height(0, 24)
            .map_err(|error| error.to_string())?;
        worksheet
            .set_column_width(0, 13)
            .map_err(|error| error.to_string())?;
        worksheet
            .set_column_width(1, 18)
            .map_err(|error| error.to_string())?;
        worksheet
            .set_column_width(2, 34)
            .map_err(|error| error.to_string())?;
        worksheet
            .set_column_width(3, 14)
            .map_err(|error| error.to_string())?;
        worksheet
            .set_column_width(4, 18)
            .map_err(|error| error.to_string())?;
        worksheet
            .set_column_width(5, 46)
            .map_err(|error| error.to_string())?;

        for (index, item) in rows.iter().enumerate() {
            let row = index as u32 + 1;
            let date = NaiveDate::parse_from_str(&item.work_date, "%Y-%m-%d")
                .map_err(|_| "VALIDATION_ERROR: 导出行包含无效日期".to_string())?;
            let excel_date =
                ExcelDateTime::from_ymd(date.year() as u16, date.month() as u8, date.day() as u8)
                    .map_err(|error| error.to_string())?;
            worksheet
                .write_datetime_with_format(row, 0, &excel_date, &date_format)
                .map_err(|error| error.to_string())?;
            worksheet
                .write_string_with_format(row, 1, &item.subject, &wrap_format)
                .map_err(|error| error.to_string())?;
            worksheet
                .write_string_with_format(row, 2, &item.task, &wrap_format)
                .map_err(|error| error.to_string())?;
            worksheet
                .write_number_with_format(row, 3, item.minutes as f64, &minutes_format)
                .map_err(|error| error.to_string())?;
            worksheet
                .write_string_with_format(row, 4, &item.time_range, &wrap_format)
                .map_err(|error| error.to_string())?;
            worksheet
                .write_string_with_format(row, 5, &item.note, &wrap_format)
                .map_err(|error| error.to_string())?;
        }

        let total_row = rows.len() as u32 + 1;
        worksheet
            .write_string_with_format(total_row, 2, "合计", &total_format)
            .map_err(|error| error.to_string())?;
        worksheet
            .write_number_with_format(total_row, 3, total_minutes as f64, &total_format)
            .map_err(|error| error.to_string())?;
        worksheet
            .autofilter(0, 0, rows.len() as u32, 5)
            .map_err(|error| error.to_string())?;
    }
    workbook.save_to_buffer().map_err(|error| error.to_string())
}

fn unique_sibling(path: &Path, suffix: &str) -> Result<PathBuf, String> {
    let parent = path
        .parent()
        .ok_or_else(|| "FILE_ERROR: 保存路径没有父目录".to_string())?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "FILE_ERROR: 保存文件名无效".to_string())?;
    Ok(parent.join(format!(".{file_name}.{}.{suffix}", Uuid::now_v7())))
}

fn safe_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "FILE_ERROR: 保存路径没有父目录".to_string())?;
    if !parent.is_dir() {
        return Err("FILE_ERROR: 保存目录不存在".to_string());
    }
    if path.is_dir() {
        return Err("FILE_ERROR: 保存位置是文件夹，无法写入 Excel".to_string());
    }
    let temporary = unique_sibling(path, "tmp")?;
    let backup = unique_sibling(path, "bak")?;
    let write_result = (|| -> Result<(), String> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| format!("FILE_ERROR: 无法创建临时导出文件: {error}"))?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("FILE_ERROR: 无法写入导出文件: {error}"))?;
        drop(file);

        let existed = path.exists();
        if existed {
            fs::rename(path, &backup)
                .map_err(|error| format!("FILE_ERROR: 目标文件正在使用或无法覆盖: {error}"))?;
        }
        if let Err(error) = fs::rename(&temporary, path) {
            if existed {
                let _ = fs::rename(&backup, path);
            }
            return Err(format!("FILE_ERROR: 无法保存 Excel: {error}"));
        }
        if existed {
            let _ = fs::remove_file(&backup);
        }
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
        if backup.exists() && !path.exists() {
            let _ = fs::rename(&backup, path);
        }
    }
    write_result
}

fn execute_with_snapshot(
    database: &Database,
    request: &TimeAllocationExportExecuteRequest,
    snapshot_at: i64,
) -> Result<TimeAllocationExportResult, String> {
    let export_request = TimeAllocationExportRequest::from(request);
    validate_request(&export_request)?;
    let path = PathBuf::from(request.target_path.trim());
    if !path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("xlsx"))
    {
        return Err("VALIDATION_ERROR: 保存文件必须使用 .xlsx 扩展名".to_string());
    }
    let connection = database.open()?;
    let built = build_export(&connection, &export_request, snapshot_at)?;
    if built.rows.is_empty() {
        return Err("NO_EXPORT_ROWS: 所选范围没有可导出的工时".to_string());
    }
    let bytes = workbook_bytes(&built.rows, built.total_minutes)?;
    safe_write(&path, &bytes)?;
    Ok(TimeAllocationExportResult {
        row_count: built.rows.len(),
        total_minutes: built.total_minutes,
        path: path.to_string_lossy().into_owned(),
    })
}

fn execute_direct(
    database: &Database,
    request: &TimeAllocationExportExecuteRequest,
) -> Result<TimeAllocationExportResult, String> {
    let export_request = TimeAllocationExportRequest::from(request);
    validate_request(&export_request)?;
    let path = PathBuf::from(request.target_path.trim());
    if !path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("xlsx"))
    {
        return Err("VALIDATION_ERROR: 保存文件必须使用 .xlsx 扩展名".to_string());
    }
    let rows = crate::supabase::direct_cloud_time_allocation_export(database, export_request)?;
    if rows.is_empty() {
        return Err("NO_EXPORT_ROWS: 所选范围没有可导出的工时".to_string());
    }
    let total_minutes = rows.iter().map(|row| row.minutes).sum();
    safe_write(&path, &workbook_bytes(&rows, total_minutes)?)?;
    Ok(TimeAllocationExportResult {
        row_count: rows.len(),
        total_minutes,
        path: path.to_string_lossy().into_owned(),
    })
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[tauri::command]
pub async fn time_allocation_export_preview(
    database: tauri::State<'_, Database>,
    request: TimeAllocationExportRequest,
) -> Result<TimeAllocationExportPreview, String> {
    let database = database.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let direct_request = request.clone();
        crate::storage_contract::route(
            &database,
            crate::storage_contract::BusinessDomain::Time,
            crate::storage_contract::StorageOperation::Query,
            |database| preview_with_snapshot(database, &request, now_millis()),
            |database| preview_direct(database, &direct_request),
        )
    })
    .await
    .map_err(|error| format!("导出预览任务失败: {error}"))?
}

#[tauri::command]
pub async fn time_allocation_export_execute(
    database: tauri::State<'_, Database>,
    request: TimeAllocationExportExecuteRequest,
) -> Result<TimeAllocationExportResult, String> {
    let database = database.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let direct_request = TimeAllocationExportExecuteRequest {
            start_date: request.start_date.clone(),
            end_date: request.end_date.clone(),
            include_special_time: request.include_special_time,
            target_path: request.target_path.clone(),
        };
        crate::storage_contract::route(
            &database,
            crate::storage_contract::BusinessDomain::Time,
            crate::storage_contract::StorageOperation::Query,
            |database| execute_with_snapshot(database, &request, now_millis()),
            |database| execute_direct(database, &direct_request),
        )
    })
    .await
    .map_err(|error| format!("Excel 导出任务失败: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use std::io::Read;
    use tempfile::tempdir;
    use zip::ZipArchive;

    fn millis(value: &str) -> i64 {
        let timezone: Tz = "Asia/Shanghai".parse().unwrap();
        chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")
            .unwrap()
            .and_local_timezone(timezone)
            .single()
            .unwrap()
            .timestamp_millis()
    }

    struct Fixture {
        _directory: tempfile::TempDir,
        database: Database,
        workspace_id: String,
        task_id: String,
        child_task_id: String,
    }

    fn fixture(storage_mode: &str) -> Fixture {
        let directory = tempdir().unwrap();
        let path = directory.path().join("export.sqlite3");
        let database = Database::initialize_at(path).unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row(
                "SELECT id FROM workspaces WHERE deleted_at IS NULL LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        connection
            .execute(
                "UPDATE workspaces SET storage_mode=?1 WHERE id=?2",
                params![storage_mode, workspace_id],
            )
            .unwrap();
        let subject_id: String = connection
            .query_row(
                "SELECT id FROM subjects WHERE workspace_id=?1 AND deleted_at IS NULL LIMIT 1",
                [&workspace_id],
                |row| row.get(0),
            )
            .unwrap();
        connection
            .execute("DELETE FROM time_allocations", [])
            .unwrap();
        connection.execute("DELETE FROM time_segments", []).unwrap();
        connection.execute("DELETE FROM time_entries", []).unwrap();
        connection.execute("DELETE FROM tasks", []).unwrap();
        let task_id = Uuid::now_v7().to_string();
        let child_task_id = Uuid::now_v7().to_string();
        let now = millis("2026-10-01 00:00:00");
        connection.execute("INSERT INTO tasks(id,workspace_id,subject_id,parent_id,title,status,source_type,sort_order,created_at,updated_at,version) VALUES (?1,?2,?3,NULL,'项目 A','open','manual',10,?4,?4,1)",params![task_id,workspace_id,subject_id,now]).unwrap();
        connection.execute("INSERT INTO tasks(id,workspace_id,subject_id,parent_id,title,status,source_type,sort_order,created_at,updated_at,version) VALUES (?1,?2,?3,?4,'需求确认','open','manual',20,?5,?5,1)",params![child_task_id,workspace_id,subject_id,task_id,now]).unwrap();
        Fixture {
            _directory: directory,
            database,
            workspace_id,
            task_id,
            child_task_id,
        }
    }

    fn insert_entry(
        fixture: &Fixture,
        id: &str,
        work_date: &str,
        kind: &str,
        state: &str,
        started_at: i64,
        ended_at: Option<i64>,
        duration_seconds: i64,
        note: Option<&str>,
    ) {
        let connection = fixture.database.open().unwrap();
        connection.execute("INSERT INTO time_entries(id,workspace_id,work_date,kind,source_type,state,label_snapshot,started_at,ended_at,duration_seconds,note,created_at,updated_at,version,timer_chain_id,last_continuous_at) VALUES (?1,?2,?3,?4,'manual',?5,'测试',?6,?7,?8,?9,?6,?6,1,NULL,COALESCE(?7,?6))",params![id,fixture.workspace_id,work_date,kind,state,started_at,ended_at,duration_seconds,note]).unwrap();
    }

    fn insert_allocation(
        fixture: &Fixture,
        id: &str,
        entry_id: &str,
        task_id: &str,
        minutes: i64,
        note: Option<&str>,
    ) {
        let connection = fixture.database.open().unwrap();
        connection.execute("INSERT INTO time_allocations(id,workspace_id,entry_id,task_id,minutes,note,created_at,updated_at,version) VALUES (?1,?2,?3,?4,?5,?6,?7,?7,1)",params![id,fixture.workspace_id,entry_id,task_id,minutes,note,millis("2026-10-06 18:00:00")]).unwrap();
    }

    fn request(include_special_time: bool) -> TimeAllocationExportRequest {
        TimeAllocationExportRequest {
            start_date: "2026-10-01".to_string(),
            end_date: "2026-10-31".to_string(),
            include_special_time,
        }
    }

    #[test]
    fn time_allocation_export_request_serializes_as_camel_case_and_validates_range() {
        let value = serde_json::to_value(request(true)).unwrap();
        assert_eq!(value["startDate"], "2026-10-01");
        assert_eq!(value["endDate"], "2026-10-31");
        assert_eq!(value["includeSpecialTime"], true);
        assert!(validate_request(&request(false)).is_ok());
        let invalid = TimeAllocationExportRequest {
            start_date: "2026-11-01".to_string(),
            end_date: "2026-10-31".to_string(),
            include_special_time: false,
        };
        assert!(validate_request(&invalid)
            .unwrap_err()
            .contains("开始日期不能晚于结束日期"));
    }

    #[test]
    fn time_allocation_export_builds_task_rows_and_optional_special_rows() {
        let fixture = fixture("local");
        let started = millis("2026-10-06 09:00:00");
        insert_entry(
            &fixture,
            "work-entry",
            "2026-10-06",
            "work",
            "ended",
            started,
            Some(started + 3_600_000),
            3_600,
            Some("客户沟通"),
        );
        insert_allocation(
            &fixture,
            "allocation-a",
            "work-entry",
            &fixture.child_task_id,
            40,
            Some("需求确认"),
        );
        insert_entry(
            &fixture,
            "break-entry",
            "2026-10-07",
            "break",
            "ended",
            started + 86_400_000,
            Some(started + 86_400_000 + 600_000),
            600,
            None,
        );

        let connection = fixture.database.open().unwrap();
        let normal = build_export(&connection, &request(false), started + 4_000_000).unwrap();
        assert_eq!(normal.rows.len(), 1);
        assert_eq!(normal.total_minutes, 40);
        assert_eq!(normal.rows[0].task, "项目 A / 需求确认");
        assert_eq!(normal.rows[0].note, "计时：客户沟通；归属：需求确认");

        let special = build_export(&connection, &request(true), started + 4_000_000).unwrap();
        assert_eq!(special.rows.len(), 3);
        assert_eq!(special.total_minutes, 70);
        assert!(special
            .rows
            .iter()
            .any(|row| row.task == "未归属" && row.minutes == 20));
        assert!(special
            .rows
            .iter()
            .any(|row| row.task == "休息" && row.minutes == 10));
    }

    #[test]
    fn time_allocation_export_handles_multiple_allocations_cross_day_deleted_and_legacy_zero_rows()
    {
        let fixture = fixture("local");
        let first = millis("2026-10-06 23:30:00");
        insert_entry(
            &fixture,
            "day-one",
            "2026-10-06",
            "work",
            "ended",
            first,
            Some(first + 1_800_000),
            1_800,
            None,
        );
        insert_entry(
            &fixture,
            "day-two",
            "2026-10-07",
            "work",
            "ended",
            first + 1_800_000,
            Some(first + 3_600_000),
            1_800,
            None,
        );
        insert_allocation(&fixture, "first-a", "day-one", &fixture.task_id, 10, None);
        insert_allocation(
            &fixture,
            "first-b",
            "day-one",
            &fixture.child_task_id,
            20,
            None,
        );
        insert_allocation(
            &fixture,
            "second-a",
            "day-two",
            &fixture.child_task_id,
            30,
            None,
        );
        let connection = fixture.database.open().unwrap();
        connection
            .execute(
                "UPDATE tasks SET deleted_at=?1 WHERE id=?2",
                params![first + 4_000_000, fixture.task_id],
            )
            .unwrap();
        connection
            .execute("PRAGMA ignore_check_constraints=ON", [])
            .unwrap();
        connection.execute("INSERT INTO time_allocations(id,workspace_id,entry_id,task_id,minutes,created_at,updated_at,version) VALUES ('legacy-zero',?1,'day-two',?2,0,?3,?3,1)",params![fixture.workspace_id,fixture.task_id,first]).unwrap();
        connection
            .execute(
                "UPDATE time_entries SET deleted_at=?1 WHERE id='day-two'",
                [first + 5_000_000],
            )
            .unwrap();

        let built = build_export(&connection, &request(false), first + 6_000_000).unwrap();
        assert_eq!(built.rows.len(), 2);
        assert_eq!(built.total_minutes, 30);
        assert!(built.rows.iter().all(|row| row.work_date == "2026-10-06"));
        assert!(built.rows.iter().any(|row| row.task == "已删除事项"));
    }

    #[test]
    fn time_allocation_export_clamps_overallocated_special_time_and_formats_workspace_timezone() {
        let fixture = fixture("local");
        let started = millis("2026-10-06 09:00:00");
        insert_entry(
            &fixture,
            "overallocated",
            "2026-10-06",
            "work",
            "ended",
            started,
            Some(started + 1_800_000),
            1_800,
            None,
        );
        insert_allocation(
            &fixture,
            "too-much",
            "overallocated",
            &fixture.task_id,
            40,
            None,
        );
        let connection = fixture.database.open().unwrap();
        connection
            .execute(
                "UPDATE workspaces SET timezone='America/New_York' WHERE id=?1",
                [&fixture.workspace_id],
            )
            .unwrap();
        let built = build_export(&connection, &request(true), started + 2_000_000).unwrap();
        assert_eq!(built.rows.len(), 1);
        assert_eq!(built.total_minutes, 40);
        assert_eq!(built.rows[0].time_range, "21:00-21:30");
        assert!(!built.rows.iter().any(|row| row.task == "未归属"));
    }

    #[test]
    fn time_allocation_export_preview_matches_built_rows_and_empty_result() {
        let fixture = fixture("local");
        let preview = preview_with_snapshot(
            &fixture.database,
            &request(false),
            millis("2026-10-10 12:00:00"),
        )
        .unwrap();
        assert_eq!(preview.row_count, 0);
        assert_eq!(preview.total_minutes, 0);
        assert_eq!(
            preview.default_file_name,
            "TimeGenie_工时归属_2026-10-01_2026-10-31.xlsx"
        );
    }

    #[test]
    fn time_allocation_export_marks_running_and_paused_snapshots_without_pause_growth() {
        let fixture = fixture("local");
        let started = millis("2026-10-08 10:00:00");
        let snapshot = started + 125_000;
        insert_entry(
            &fixture,
            "running-entry",
            "2026-10-08",
            "work",
            "running",
            started,
            None,
            0,
            None,
        );
        let connection = fixture.database.open().unwrap();
        connection.execute("INSERT INTO time_segments(id,workspace_id,entry_id,sequence_no,started_at,ended_at,duration_seconds) VALUES ('running-segment',?1,'running-entry',1,?2,NULL,0)",params![fixture.workspace_id,started]).unwrap();
        let running = build_export(&connection, &request(true), snapshot).unwrap();
        assert_eq!(running.total_minutes, 3);
        assert!(running.rows[0].note.contains("进行中"));
        assert_eq!(running.rows[0].time_range, "10:00-10:02");

        connection.execute("DELETE FROM time_segments", []).unwrap();
        connection.execute("DELETE FROM time_entries", []).unwrap();
        insert_entry(
            &fixture,
            "paused-entry",
            "2026-10-08",
            "work",
            "paused",
            started,
            None,
            120,
            None,
        );
        connection.execute("INSERT INTO time_segments(id,workspace_id,entry_id,sequence_no,started_at,ended_at,duration_seconds) VALUES ('paused-segment',?1,'paused-entry',1,?2,?3,120)",params![fixture.workspace_id,started,started+120_000]).unwrap();
        let paused = build_export(&connection, &request(true), snapshot + 86_400_000).unwrap();
        assert_eq!(paused.total_minutes, 2);
        assert!(paused.rows[0].note.contains("已暂停"));
        assert_eq!(paused.rows[0].time_range, "10:00-10:02");
    }

    #[test]
    fn time_allocation_export_generates_single_sheet_six_columns_and_total() {
        let rows = vec![TimeAllocationExportRow {
            work_date: "2026-10-06".to_string(),
            subject: "默认".to_string(),
            task: "项目 A / 需求确认".to_string(),
            minutes: 40,
            time_range: "09:00-10:00".to_string(),
            note: "计时：客户沟通".to_string(),
        }];
        let bytes = workbook_bytes(&rows, 40).unwrap();
        let cursor = std::io::Cursor::new(bytes);
        let mut archive = ZipArchive::new(cursor).unwrap();
        let workbook_xml = {
            let mut value = String::new();
            archive
                .by_name("xl/workbook.xml")
                .unwrap()
                .read_to_string(&mut value)
                .unwrap();
            value
        };
        assert!(workbook_xml.contains("name=\"工时归属\""));
        assert_eq!(workbook_xml.matches("<sheet ").count(), 1);
        let sheet_xml = {
            let mut value = String::new();
            archive
                .by_name("xl/worksheets/sheet1.xml")
                .unwrap()
                .read_to_string(&mut value)
                .unwrap();
            value
        };
        assert!(sheet_xml.contains("ref=\"A1:F2\""));
        assert!(sheet_xml.contains("<v>40</v>"));
        assert!(sheet_xml.contains("<pane ySplit=\"1\""));
        let shared_strings = {
            let mut value = String::new();
            archive
                .by_name("xl/sharedStrings.xml")
                .unwrap()
                .read_to_string(&mut value)
                .unwrap();
            value
        };
        let mut last_index = 0;
        for header in EXPORT_HEADERS {
            let index = shared_strings[last_index..].find(header).unwrap() + last_index;
            assert!(index >= last_index);
            last_index = index + header.len();
        }
    }

    #[test]
    fn time_allocation_export_execute_is_safe_and_read_only_in_local_and_cloud_modes() {
        for mode in ["local", "cloud"] {
            let fixture = fixture(mode);
            let started = millis("2026-10-09 09:00:00");
            insert_entry(
                &fixture,
                "entry",
                "2026-10-09",
                "work",
                "ended",
                started,
                Some(started + 1_800_000),
                1_800,
                None,
            );
            insert_allocation(&fixture, "allocation", "entry", &fixture.task_id, 30, None);
            let connection = fixture.database.open().unwrap();
            let before: (i64, i64, i64) = connection
                .query_row("SELECT (SELECT version FROM time_entries WHERE id='entry'), (SELECT version FROM time_allocations WHERE id='allocation'), (SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='sync_outbox')",[],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?)))
                .unwrap();
            drop(connection);
            let directory = tempdir().unwrap();
            let target = directory.path().join(format!("{mode}.xlsx"));
            let execute_request = TimeAllocationExportExecuteRequest {
                start_date: "2026-10-01".to_string(),
                end_date: "2026-10-31".to_string(),
                include_special_time: false,
                target_path: target.to_string_lossy().into_owned(),
            };
            let result =
                execute_with_snapshot(&fixture.database, &execute_request, started + 2_000_000)
                    .unwrap();
            assert_eq!(result.row_count, 1);
            assert_eq!(result.total_minutes, 30);
            assert!(target.exists());
            let connection = fixture.database.open().unwrap();
            let after: (i64, i64, i64) = connection
                .query_row("SELECT (SELECT version FROM time_entries WHERE id='entry'), (SELECT version FROM time_allocations WHERE id='allocation'), (SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='sync_outbox')",[],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?)))
                .unwrap();
            assert_eq!(before, after);
        }
    }

    #[test]
    fn time_allocation_export_integration_writes_cross_subject_special_time_files() {
        let fixture = fixture("local");
        let connection = fixture.database.open().unwrap();
        let second_subject_id = Uuid::now_v7().to_string();
        let second_task_id = Uuid::now_v7().to_string();
        let now = millis("2026-10-01 00:00:00");
        connection.execute("INSERT INTO subjects(id,workspace_id,name,sort_order,created_at,updated_at,version) VALUES (?1,?2,'客户 B',20,?3,?3,1)",params![second_subject_id,fixture.workspace_id,now]).unwrap();
        connection.execute("INSERT INTO tasks(id,workspace_id,subject_id,parent_id,title,status,source_type,sort_order,created_at,updated_at,version) VALUES (?1,?2,?3,NULL,'客户回访','open','manual',10,?4,?4,1)",params![second_task_id,fixture.workspace_id,second_subject_id,now]).unwrap();
        drop(connection);

        let started = millis("2026-10-05 09:00:00");
        insert_entry(
            &fixture,
            "mixed-work",
            "2026-10-05",
            "work",
            "ended",
            started,
            Some(started + 3_600_000),
            3_600,
            Some("跨主体协作"),
        );
        insert_allocation(
            &fixture,
            "mixed-a",
            "mixed-work",
            &fixture.child_task_id,
            40,
            Some("需求确认"),
        );
        insert_allocation(
            &fixture,
            "mixed-b",
            "mixed-work",
            &second_task_id,
            10,
            Some("电话回访"),
        );
        insert_entry(
            &fixture,
            "mixed-break",
            "2026-10-06",
            "break",
            "ended",
            started + 86_400_000,
            Some(started + 87_000_000),
            600,
            Some("午间休息"),
        );
        insert_entry(
            &fixture,
            "mixed-invalid",
            "2026-10-07",
            "work",
            "ended",
            started + 172_800_000,
            Some(started + 174_600_000),
            1_800,
            Some("应排除"),
        );
        let paused_started = millis("2026-10-08 10:00:00");
        insert_entry(
            &fixture,
            "mixed-paused",
            "2026-10-08",
            "work",
            "paused",
            paused_started,
            None,
            120,
            Some("暂停等待"),
        );
        let connection = fixture.database.open().unwrap();
        connection
            .execute(
                "UPDATE time_entries SET deleted_at=?1 WHERE id='mixed-invalid'",
                [started + 200_000_000],
            )
            .unwrap();
        connection.execute("INSERT INTO time_segments(id,workspace_id,entry_id,sequence_no,started_at,ended_at,duration_seconds) VALUES ('mixed-paused-segment',?1,'mixed-paused',1,?2,?3,120)",params![fixture.workspace_id,paused_started,paused_started+120_000]).unwrap();
        let normal =
            build_export(&connection, &request(false), paused_started + 7_200_000).unwrap();
        assert_eq!(normal.rows.len(), 2);
        assert_eq!(normal.total_minutes, 50);
        assert!(normal
            .rows
            .iter()
            .any(|row| row.subject == "默认" && row.minutes == 40));
        assert!(normal
            .rows
            .iter()
            .any(|row| row.subject == "客户 B" && row.minutes == 10));
        assert!(!normal.rows.iter().any(|row| row.note.contains("应排除")));
        let special =
            build_export(&connection, &request(true), paused_started + 7_200_000).unwrap();
        assert_eq!(special.rows.len(), 5);
        assert_eq!(special.total_minutes, 72);
        assert!(special
            .rows
            .iter()
            .any(|row| row.task == "未归属" && row.minutes == 10));
        assert!(special
            .rows
            .iter()
            .any(|row| row.task == "休息" && row.minutes == 10));
        assert!(special
            .rows
            .iter()
            .any(|row| row.task == "未归属" && row.minutes == 2 && row.note.contains("已暂停")));
        drop(connection);

        let directory = tempdir().unwrap();
        for (name, include_special_time, expected_rows, expected_minutes) in
            [("normal.XLSX", false, 2, 50), ("special.xlsx", true, 5, 72)]
        {
            let target = directory.path().join(name);
            let result = execute_with_snapshot(
                &fixture.database,
                &TimeAllocationExportExecuteRequest {
                    start_date: "2026-10-01".to_string(),
                    end_date: "2026-10-31".to_string(),
                    include_special_time,
                    target_path: target.to_string_lossy().into_owned(),
                },
                paused_started + 7_200_000,
            )
            .unwrap();
            assert_eq!(result.row_count, expected_rows);
            assert_eq!(result.total_minutes, expected_minutes);
            let archive = ZipArchive::new(std::io::Cursor::new(fs::read(target).unwrap())).unwrap();
            assert_eq!(archive.len() > 5, true);
        }
    }

    #[test]
    fn time_allocation_export_cloud_offline_cross_month_does_not_touch_sync_state() {
        let fixture = fixture("cloud");
        for (id, work_date, started, minutes) in [
            ("september-entry", "2026-09-30", "2026-09-30 23:00:00", 30),
            ("october-entry", "2026-10-01", "2026-10-01 09:00:00", 45),
        ] {
            let started_at = millis(started);
            insert_entry(
                &fixture,
                id,
                work_date,
                "work",
                "ended",
                started_at,
                Some(started_at + minutes * 60_000),
                minutes * 60,
                None,
            );
            insert_allocation(
                &fixture,
                &format!("{id}-allocation"),
                id,
                &fixture.task_id,
                minutes,
                None,
            );
        }
        let connection = fixture.database.open().unwrap();
        let before: (String, i64, i64) = connection
            .query_row(
                "SELECT storage_mode, (SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='sync_outbox'), (SELECT CAST(value AS INTEGER) FROM app_metadata WHERE key='global_revision') FROM workspaces WHERE id=?1",
                [&fixture.workspace_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(before.0, "cloud");
        drop(connection);
        let directory = tempdir().unwrap();
        let target = directory.path().join("cloud-offline-cross-month.xlsx");
        let result = execute_with_snapshot(
            &fixture.database,
            &TimeAllocationExportExecuteRequest {
                start_date: "2026-09-01".to_string(),
                end_date: "2026-10-31".to_string(),
                include_special_time: false,
                target_path: target.to_string_lossy().into_owned(),
            },
            millis("2026-10-13 12:00:00"),
        )
        .unwrap();
        assert_eq!(result.row_count, 2);
        assert_eq!(result.total_minutes, 75);
        let connection = fixture.database.open().unwrap();
        let after: (String, i64, i64) = connection
            .query_row(
                "SELECT storage_mode, (SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='sync_outbox'), (SELECT CAST(value AS INTEGER) FROM app_metadata WHERE key='global_revision') FROM workspaces WHERE id=?1",
                [&fixture.workspace_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(before, after);
        let archive = ZipArchive::new(std::io::Cursor::new(fs::read(target).unwrap())).unwrap();
        assert!(archive.len() > 5);
    }

    #[test]
    fn time_allocation_export_does_not_create_empty_or_damage_existing_file_on_error() {
        let fixture = fixture("local");
        let directory = tempdir().unwrap();
        let empty_target = directory.path().join("empty.xlsx");
        let empty_request = TimeAllocationExportExecuteRequest {
            start_date: "2026-10-01".to_string(),
            end_date: "2026-10-31".to_string(),
            include_special_time: false,
            target_path: empty_target.to_string_lossy().into_owned(),
        };
        assert!(execute_with_snapshot(
            &fixture.database,
            &empty_request,
            millis("2026-10-09 10:00:00")
        )
        .unwrap_err()
        .contains("NO_EXPORT_ROWS"));
        assert!(!empty_target.exists());

        let existing = directory.path().join("existing.xlsx");
        fs::write(&existing, b"original").unwrap();
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            let locked = OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(&existing)
                .unwrap();
            assert!(safe_write(&existing, b"replacement").is_err());
            drop(locked);
            assert_eq!(fs::read(&existing).unwrap(), b"original");
        }
        let invalid_target = directory.path().join("missing").join("bad.xlsx");
        assert!(safe_write(&invalid_target, b"new").is_err());
        assert!(!invalid_target.exists());
        assert_eq!(fs::read(&existing).unwrap(), b"original");
        let directory_target = directory.path().join("folder.xlsx");
        fs::create_dir(&directory_target).unwrap();
        assert!(safe_write(&directory_target, b"new").is_err());
        assert!(directory_target.is_dir());
    }
}
