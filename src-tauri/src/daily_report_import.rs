use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::database::Database;
use crate::tasks::{TaskCreateRequest, TaskDto};

const BACKLOG: &str = "backlog";
const DAY_INSTANCE: &str = "day_instance";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyReportImportPreviewRequest {
    pub subject_id: String,
    pub source_date: String,
    pub target_kind: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyReportImportPreviewItem {
    pub id: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub status: String,
    pub estimate_minutes: Option<i64>,
    pub raw_actual_minutes: Option<i64>,
    pub import_actual_minutes: Option<i64>,
    pub sort_order: i64,
    pub source_line_no: usize,
    pub source_text: String,
    pub warning: Option<String>,
    pub already_imported: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyReportImportPreviewResult {
    pub batch_id: String,
    pub subject_id: String,
    pub source_date: String,
    pub target_kind: String,
    pub source_path: String,
    pub source_mtime: i64,
    pub source_content_hash: String,
    pub items: Vec<DailyReportImportPreviewItem>,
    pub warnings: Vec<String>,
    pub version: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyReportImportConfirmRequest {
    pub batch_id: String,
    pub selected_item_ids: Vec<String>,
    pub expected_version: i64,
    pub client_request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyReportImportConfirmResult {
    pub batch_id: String,
    pub target_kind: String,
    pub source_date: String,
    pub tasks: Vec<TaskDto>,
    pub created_count: usize,
    pub skipped_count: usize,
    pub adjustment_count: usize,
    pub already_confirmed: bool,
}

#[derive(Debug, Clone)]
struct ParsedItem {
    client_id: String,
    parent_client_id: Option<String>,
    title: String,
    status: String,
    estimate_minutes: Option<i64>,
    raw_actual_minutes: Option<i64>,
    import_actual_minutes: Option<i64>,
    sort_order: i64,
    source_line_no: usize,
    source_text: String,
    warning: Option<String>,
    source_fingerprint: String,
}

#[derive(Debug)]
struct ParsedReport {
    items: Vec<ParsedItem>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone)]
struct ImportItemRow {
    id: String,
    parent_item_id: Option<String>,
    title: String,
    status: String,
    estimate_minutes: Option<i64>,
    import_actual_minutes: Option<i64>,
    source_fingerprint: String,
    already_imported: bool,
}

#[derive(Debug, Clone)]
struct DirectDailyReportImportState {
    preview: DailyReportImportPreviewResult,
    source_path_hash: String,
    fingerprints: HashMap<String, String>,
    confirmed: Option<DailyReportImportConfirmResult>,
}

fn direct_daily_report_imports() -> &'static Mutex<HashMap<String, DirectDailyReportImportState>> {
    static IMPORTS: OnceLock<Mutex<HashMap<String, DirectDailyReportImportState>>> =
        OnceLock::new();
    IMPORTS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn preview(
    database: &Database,
    request: DailyReportImportPreviewRequest,
) -> Result<DailyReportImportPreviewResult, String> {
    validate_date(&request.source_date)?;
    validate_target_kind(&request.target_kind)?;
    let connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    ensure_subject(&connection, &workspace_id, &request.subject_id)?;
    let device_id = device_id(&connection, &workspace_id)?;
    let directory = subject_directory(&connection, &request.subject_id)?
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "INTEGRATION_NOT_CONFIGURED: 请先在主体设置中配置日报目录".to_string())?;
    let path = report_path(&directory, &request.source_date);
    if !path.is_file() {
        return Err(format!(
            "SOURCE_NOT_FOUND: 日报文件不存在：{}",
            path.display()
        ));
    }
    let metadata = fs::metadata(&path)
        .map_err(|error| format!("无法读取日报文件信息 {}: {error}", path.display()))?;
    let raw_markdown = fs::read_to_string(&path)
        .map_err(|error| format!("无法读取日报文件 {}: {error}", path.display()))?;
    let source_mtime = metadata
        .modified()
        .ok()
        .map(system_time_millis)
        .unwrap_or(0);
    let source_content_hash = content_hash(&raw_markdown);
    let normalized_path = normalize_path(&path);
    let source_path_hash = content_hash(&normalized_path);
    let parsed = parse_daily_report(&raw_markdown);
    let batch_id = Uuid::now_v7().to_string();
    let now = now_millis();
    let warnings_json =
        serde_json::to_string(&parsed.warnings).map_err(|error| error.to_string())?;
    connection.execute(
        "INSERT INTO daily_report_import_batches(
           id,workspace_id,device_id,subject_id,source_date,target_kind,source_path,source_path_hash,
           source_mtime,source_content_hash,raw_markdown,warnings_json,state,created_at,updated_at,version
         ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,'preview',?13,?13,1)",
        params![batch_id,workspace_id,device_id,request.subject_id,request.source_date,request.target_kind,
            path.display().to_string(),source_path_hash,source_mtime,source_content_hash,raw_markdown,warnings_json,now],
    ).map_err(|error| error.to_string())?;

    let mut client_to_id = HashMap::new();
    let mut items = Vec::new();
    for parsed_item in parsed.items {
        let id = Uuid::now_v7().to_string();
        let parent_id = parsed_item
            .parent_client_id
            .as_ref()
            .and_then(|value| client_to_id.get(value))
            .cloned();
        let already_imported: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM daily_report_import_sources
             WHERE workspace_id=?1 AND subject_id=?2 AND source_device_id=?3 AND source_path_hash=?4 AND source_date=?5
               AND target_kind=?6 AND source_fingerprint=?7)",
                params![
                    workspace_id,
                    request.subject_id,
                    device_id,
                    source_path_hash,
                    request.source_date,
                    request.target_kind,
                    parsed_item.source_fingerprint
                ],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        connection.execute(
            "INSERT INTO daily_report_import_items(
               id,workspace_id,batch_id,parent_item_id,title,item_status,estimate_minutes,raw_actual_minutes,
               import_actual_minutes,sort_order,source_line_no,source_text,source_fingerprint,warning,already_imported
             ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
            params![id,workspace_id,batch_id,parent_id,parsed_item.title,parsed_item.status,parsed_item.estimate_minutes,
                parsed_item.raw_actual_minutes,parsed_item.import_actual_minutes,parsed_item.sort_order,parsed_item.source_line_no as i64,
                parsed_item.source_text,parsed_item.source_fingerprint,parsed_item.warning,already_imported as i64],
        ).map_err(|error| error.to_string())?;
        client_to_id.insert(parsed_item.client_id, id.clone());
        items.push(DailyReportImportPreviewItem {
            id,
            parent_id,
            title: parsed_item.title,
            status: parsed_item.status,
            estimate_minutes: parsed_item.estimate_minutes,
            raw_actual_minutes: parsed_item.raw_actual_minutes,
            import_actual_minutes: parsed_item.import_actual_minutes,
            sort_order: parsed_item.sort_order,
            source_line_no: parsed_item.source_line_no,
            source_text: parsed_item.source_text,
            warning: parsed_item.warning,
            already_imported,
        });
    }
    Ok(DailyReportImportPreviewResult {
        batch_id,
        subject_id: request.subject_id,
        source_date: request.source_date,
        target_kind: request.target_kind,
        source_path: path.display().to_string(),
        source_mtime,
        source_content_hash,
        items,
        warnings: parsed.warnings,
        version: 1,
    })
}

fn direct_preview(
    database: &Database,
    request: DailyReportImportPreviewRequest,
) -> Result<DailyReportImportPreviewResult, String> {
    validate_date(&request.source_date)?;
    validate_target_kind(&request.target_kind)?;
    crate::supabase::direct_cloud_subject_list(database)?
        .into_iter()
        .find(|subject| subject.id == request.subject_id)
        .ok_or_else(|| "NOT_FOUND: 主体不存在".to_string())?;
    let connection = database.open()?;
    let directory = subject_directory(&connection, &request.subject_id)?
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "INTEGRATION_NOT_CONFIGURED: 请先在主体设置中配置日报目录".to_string())?;
    let path = report_path(&directory, &request.source_date);
    if !path.is_file() {
        return Err(format!(
            "SOURCE_NOT_FOUND: 日报文件不存在：{}",
            path.display()
        ));
    }
    let metadata = fs::metadata(&path)
        .map_err(|error| format!("无法读取日报文件信息 {}: {error}", path.display()))?;
    let raw_markdown = fs::read_to_string(&path)
        .map_err(|error| format!("无法读取日报文件 {}: {error}", path.display()))?;
    let source_mtime = metadata
        .modified()
        .ok()
        .map(system_time_millis)
        .unwrap_or(0);
    let source_content_hash = content_hash(&raw_markdown);
    let source_path_hash = content_hash(&normalize_path(&path));
    let parsed = parse_daily_report(&raw_markdown);
    let fingerprints = parsed
        .items
        .iter()
        .map(|item| item.source_fingerprint.clone())
        .collect::<Vec<_>>();
    let imported = crate::supabase::direct_cloud_daily_report_imported_fingerprints(
        database,
        request.subject_id.clone(),
        request.source_date.clone(),
        request.target_kind.clone(),
        source_path_hash.clone(),
        fingerprints,
    )?
    .into_iter()
    .collect::<HashSet<_>>();
    let batch_id = Uuid::now_v7().to_string();
    let mut client_to_id = HashMap::new();
    let mut item_fingerprints = HashMap::new();
    let mut items = Vec::new();
    for parsed_item in parsed.items {
        let id = Uuid::now_v7().to_string();
        let parent_id = parsed_item
            .parent_client_id
            .as_ref()
            .and_then(|value| client_to_id.get(value))
            .cloned();
        client_to_id.insert(parsed_item.client_id, id.clone());
        item_fingerprints.insert(id.clone(), parsed_item.source_fingerprint.clone());
        items.push(DailyReportImportPreviewItem {
            id,
            parent_id,
            title: parsed_item.title,
            status: parsed_item.status,
            estimate_minutes: parsed_item.estimate_minutes,
            raw_actual_minutes: parsed_item.raw_actual_minutes,
            import_actual_minutes: parsed_item.import_actual_minutes,
            sort_order: parsed_item.sort_order,
            source_line_no: parsed_item.source_line_no,
            source_text: parsed_item.source_text,
            warning: parsed_item.warning,
            already_imported: imported.contains(&parsed_item.source_fingerprint),
        });
    }
    let preview = DailyReportImportPreviewResult {
        batch_id: batch_id.clone(),
        subject_id: request.subject_id,
        source_date: request.source_date,
        target_kind: request.target_kind,
        source_path: path.display().to_string(),
        source_mtime,
        source_content_hash,
        items,
        warnings: parsed.warnings,
        version: 1,
    };
    let mut registry = direct_daily_report_imports()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    while registry.len() >= 32 {
        let Some(key) = registry.keys().next().cloned() else {
            break;
        };
        registry.remove(&key);
    }
    registry.insert(
        batch_id,
        DirectDailyReportImportState {
            preview: preview.clone(),
            source_path_hash,
            fingerprints: item_fingerprints,
            confirmed: None,
        },
    );
    Ok(preview)
}

pub fn confirm(
    database: &Database,
    request: DailyReportImportConfirmRequest,
) -> Result<DailyReportImportConfirmResult, String> {
    Uuid::parse_str(&request.client_request_id)
        .map_err(|_| "VALIDATION_ERROR: 请求标识必须是 UUID".to_string())?;
    crate::supabase::ensure_repository_write_mode(database)?;
    let state = crate::supabase::storage_mode(database)?;
    let mut connection = database.open()?;
    if let Some(result_json) = connection.query_row(
        "SELECT result_json FROM daily_report_import_batches WHERE workspace_id=(SELECT id FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1) AND confirm_client_request_id=?1",
        [&request.client_request_id], |row| row.get::<_, Option<String>>(0),
    ).optional().map_err(|error| error.to_string())?.flatten() {
        let mut result: DailyReportImportConfirmResult = serde_json::from_str(&result_json).map_err(|error| error.to_string())?;
        result.already_confirmed = true;
        return Ok(result);
    }

    let batch: Option<(String,String,String,String,String,String,i64,i64,String,Option<String>)> = connection.query_row(
        "SELECT workspace_id,device_id,subject_id,source_date,target_kind,source_path,source_mtime,version,state,result_json
         FROM daily_report_import_batches WHERE id=?1", [&request.batch_id],
        |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?,row.get(7)?,row.get(8)?,row.get(9)?)),
    ).optional().map_err(|error| error.to_string())?;
    let Some((
        workspace_id,
        source_device_id,
        subject_id,
        source_date,
        target_kind,
        source_path,
        source_mtime,
        version,
        batch_state,
        saved_result_json,
    )) = batch
    else {
        return Err("NOT_FOUND: 日报导入预览不存在".to_string());
    };
    if batch_state == "confirmed" {
        let result_json =
            saved_result_json.ok_or_else(|| "INVALID_STATE: 已确认批次缺少结果".to_string())?;
        let mut result: DailyReportImportConfirmResult =
            serde_json::from_str(&result_json).map_err(|error| error.to_string())?;
        result.already_confirmed = true;
        return Ok(result);
    }
    if version != request.expected_version {
        return Err("VERSION_CONFLICT: 日报导入预览已变化".to_string());
    }
    let current_metadata = fs::metadata(&source_path)
        .map_err(|_| "EXTERNAL_FILE_CHANGED: 日报文件已被移动或删除，请重新预览".to_string())?;
    let current_markdown = fs::read_to_string(&source_path)
        .map_err(|_| "EXTERNAL_FILE_CHANGED: 日报文件无法读取，请重新预览".to_string())?;
    let current_mtime = current_metadata
        .modified()
        .ok()
        .map(system_time_millis)
        .unwrap_or(0);
    let expected_hash: String = connection
        .query_row(
            "SELECT source_content_hash FROM daily_report_import_batches WHERE id=?1",
            [&request.batch_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if current_mtime != source_mtime || content_hash(&current_markdown) != expected_hash {
        return Err("EXTERNAL_FILE_CHANGED: 日报在预览后已变化，请重新预览".to_string());
    }
    let selected = request
        .selected_item_ids
        .into_iter()
        .collect::<HashSet<_>>();
    if selected.is_empty() {
        return Err("VALIDATION_ERROR: 请至少选择一条可导入事项".to_string());
    }
    let all_items = load_items(&connection, &request.batch_id)?;
    let item_by_id = all_items
        .iter()
        .map(|item| (item.id.clone(), item))
        .collect::<HashMap<_, _>>();
    if selected.iter().any(|id| !item_by_id.contains_key(id)) {
        return Err("VALIDATION_ERROR: 选择中包含无效候选".to_string());
    }
    let mut required = selected.clone();
    for id in selected.iter() {
        let mut cursor = item_by_id
            .get(id)
            .and_then(|item| item.parent_item_id.clone());
        while let Some(parent_id) = cursor {
            required.insert(parent_id.clone());
            cursor = item_by_id
                .get(&parent_id)
                .and_then(|item| item.parent_item_id.clone());
        }
    }
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let now = now_millis();
    let completed_at = crate::work_calendar::local_noon_millis(
        crate::work_calendar::workspace_timezone(&transaction, &workspace_id)?,
        &source_date,
    )?;
    let source_path_hash: String = transaction
        .query_row(
            "SELECT source_path_hash FROM daily_report_import_batches WHERE id=?1",
            [&request.batch_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let mut item_to_task = HashMap::new();
    let mut created_ids = Vec::new();
    let mut pending_adjustments = Vec::<(String, Option<String>, i64)>::new();
    let mut adjustment_count = 0usize;
    let mut skipped_count = 0usize;
    for item in all_items.iter().filter(|item| required.contains(&item.id)) {
        if item.already_imported {
            let existing_task_id: String = transaction.query_row(
                "SELECT s.task_id
                   FROM daily_report_import_sources s
                   JOIN tasks t ON t.id=s.task_id AND t.workspace_id=s.workspace_id AND t.deleted_at IS NULL
                  WHERE s.workspace_id=?1 AND s.subject_id=?2 AND s.source_device_id=?3 AND s.source_path_hash=?4
                    AND s.source_date=?5 AND s.target_kind=?6 AND s.source_fingerprint=?7",
                params![workspace_id,subject_id,source_device_id,source_path_hash,source_date,target_kind,item.source_fingerprint],
                |row| row.get(0),
            ).map_err(|_| "IMPORT_SOURCE_UNAVAILABLE: 已导入的父事项已不可用，请恢复该事项或修改日报后重新预览".to_string())?;
            item_to_task.insert(item.id.clone(), existing_task_id);
            skipped_count += 1;
            continue;
        }
        let explicitly_selected = selected.contains(&item.id);
        let parent_id = item
            .parent_item_id
            .as_ref()
            .and_then(|id| item_to_task.get(id))
            .cloned();
        let is_context = target_kind == DAY_INSTANCE && !explicitly_selected;
        let estimate = explicitly_selected
            .then_some(item.estimate_minutes)
            .flatten();
        let (task, _) = crate::tasks::create_task_in_transaction(
            &transaction,
            &workspace_id,
            &TaskCreateRequest {
                subject_id: subject_id.clone(),
                parent_id,
                day_instance_date: (target_kind == DAY_INSTANCE).then(|| source_date.clone()),
                title: item.title.clone(),
                planned_date: None,
                estimate_minutes: estimate,
                note: None,
                project_name: None,
                solution_name: None,
            },
            "obsidian_import",
            None,
            now,
            None,
            None,
            None,
        )?;
        if is_context {
            transaction
                .execute(
                    "UPDATE tasks SET day_instance_kind='context' WHERE id=?1",
                    [&task.id],
                )
                .map_err(|error| error.to_string())?;
        }
        if explicitly_selected && item.status == "done" && !is_context {
            transaction
                .execute(
                    "UPDATE tasks SET status='done',completed_at=?1,updated_at=?2 WHERE id=?3",
                    params![completed_at, now, task.id],
                )
                .map_err(|error| error.to_string())?;
            transaction.execute(
                "INSERT INTO task_status_events(id,workspace_id,task_id,status,occurred_at,source_type,created_at)
                 VALUES(?1,?2,?3,'done',?4,'import',?4)",
                params![Uuid::now_v7().to_string(),workspace_id,task.id,completed_at],
            ).map_err(|error| error.to_string())?;
        }
        let task_operation_id = crate::cloud_sync::enqueue_entity_in_transaction(
            &transaction,
            &state,
            "daily_report_task_import",
            "task",
            Some(&task.id),
            None,
            None,
        )?;
        if target_kind == DAY_INSTANCE && estimate.is_some() && !is_context {
            crate::tasks::write_daily_estimate_in_transaction(
                &transaction,
                &workspace_id,
                &task.id,
                &source_date,
                estimate,
                now,
                Some(&state),
                task_operation_id.as_deref(),
                false,
            )?;
        }
        if explicitly_selected && item.status == "done" {
            if let Some(target_minutes) = item.import_actual_minutes {
                pending_adjustments.push((
                    task.id.clone(),
                    task_operation_id.clone(),
                    target_minutes,
                ));
            }
        }
        transaction.execute(
            "INSERT INTO daily_report_import_sources(
               id,workspace_id,subject_id,source_device_id,source_path_hash,source_date,target_kind,source_fingerprint,task_id,batch_id,created_at
             ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![Uuid::now_v7().to_string(),workspace_id,subject_id,source_device_id,source_path_hash,source_date,target_kind,item.source_fingerprint,task.id,request.batch_id,now],
        ).map_err(|error| error.to_string())?;
        transaction
            .execute(
                "UPDATE daily_report_import_items SET confirmed_task_id=?1 WHERE id=?2",
                params![task.id, item.id],
            )
            .map_err(|error| error.to_string())?;
        item_to_task.insert(item.id.clone(), task.id.clone());
        created_ids.push(task.id);
    }
    for (task_id, task_operation_id, target_minutes) in pending_adjustments {
        let baseline = crate::actual_time_adjustments::direct_actual_minutes(
            &transaction,
            &workspace_id,
            &task_id,
            Some(&source_date),
        )?;
        let Some(delta) = adjustment_delta(target_minutes, baseline) else {
            continue;
        };
        let adjustment_id = Uuid::now_v7().to_string();
        transaction.execute(
            "INSERT INTO actual_time_adjustments(
               id,workspace_id,subject_id,task_id,work_date,delta_minutes,baseline_minutes,target_minutes,
               note,source_type,batch_id,created_at,updated_at,version,created_by_device_id,updated_by_device_id
             ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'从 Obsidian 日报导入','daily_report_import',?9,?10,?10,1,?11,?11)",
            params![adjustment_id,workspace_id,subject_id,task_id,source_date,delta,baseline,target_minutes,request.batch_id,now,state.device_id],
        ).map_err(|error| error.to_string())?;
        crate::cloud_sync::enqueue_entity_in_transaction(
            &transaction,
            &state,
            "daily_report_actual_import",
            "actual_time_adjustment",
            Some(&adjustment_id),
            None,
            task_operation_id.as_deref(),
        )?;
        adjustment_count += 1;
    }
    crate::tasks::bump_revision_for_import(&transaction)?;
    let tasks = created_ids
        .iter()
        .map(|id| crate::tasks::load_task_for_import(&transaction, id))
        .collect::<Result<Vec<_>, _>>()?;
    let mut result = DailyReportImportConfirmResult {
        batch_id: request.batch_id.clone(),
        target_kind: target_kind.clone(),
        source_date: source_date.clone(),
        created_count: tasks.len(),
        skipped_count,
        adjustment_count,
        tasks,
        already_confirmed: false,
    };
    let result_json = serde_json::to_string(&result).map_err(|error| error.to_string())?;
    let changed = transaction.execute(
        "UPDATE daily_report_import_batches SET state='confirmed',confirm_client_request_id=?1,result_json=?2,updated_at=?3,version=version+1
         WHERE id=?4 AND version=?5",
        params![request.client_request_id,result_json,now,request.batch_id,request.expected_version],
    ).map_err(|error| error.to_string())?;
    if changed == 0 {
        return Err("VERSION_CONFLICT: 日报导入预览已变化".to_string());
    }
    transaction.commit().map_err(|error| error.to_string())?;
    crate::cloud_sync::flush_if_online(database)?;
    result.already_confirmed = false;
    Ok(result)
}

fn direct_confirm(
    database: &Database,
    request: DailyReportImportConfirmRequest,
) -> Result<DailyReportImportConfirmResult, String> {
    Uuid::parse_str(&request.client_request_id)
        .map_err(|_| "VALIDATION_ERROR: 请求标识必须是 UUID".to_string())?;
    let (preview, source_path_hash, fingerprints) = {
        let registry = direct_daily_report_imports()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let state = registry
            .get(&request.batch_id)
            .ok_or_else(|| "NOT_FOUND: 日报导入预览不存在或应用已重启，请重新预览".to_string())?;
        if let Some(result) = &state.confirmed {
            let mut result = result.clone();
            result.already_confirmed = true;
            return Ok(result);
        }
        if state.preview.version != request.expected_version {
            return Err("VERSION_CONFLICT: 日报导入预览已变化".to_string());
        }
        (
            state.preview.clone(),
            state.source_path_hash.clone(),
            state.fingerprints.clone(),
        )
    };
    let current_metadata = fs::metadata(&preview.source_path)
        .map_err(|_| "EXTERNAL_FILE_CHANGED: 日报文件已被移动或删除，请重新预览".to_string())?;
    let current_markdown = fs::read_to_string(&preview.source_path)
        .map_err(|_| "EXTERNAL_FILE_CHANGED: 日报文件无法读取，请重新预览".to_string())?;
    let current_mtime = current_metadata
        .modified()
        .ok()
        .map(system_time_millis)
        .unwrap_or(0);
    if current_mtime != preview.source_mtime
        || content_hash(&current_markdown) != preview.source_content_hash
    {
        return Err("EXTERNAL_FILE_CHANGED: 日报在预览后已变化，请重新预览".to_string());
    }
    let selected = request
        .selected_item_ids
        .into_iter()
        .collect::<HashSet<_>>();
    if selected.is_empty() {
        return Err("VALIDATION_ERROR: 请至少选择一条可导入事项".to_string());
    }
    let item_by_id = preview
        .items
        .iter()
        .map(|item| (item.id.clone(), item))
        .collect::<HashMap<_, _>>();
    if selected.iter().any(|id| !item_by_id.contains_key(id)) {
        return Err("VALIDATION_ERROR: 选择中包含无效候选".to_string());
    }
    let mut required = selected.clone();
    for id in &selected {
        let mut cursor = item_by_id.get(id).and_then(|item| item.parent_id.clone());
        while let Some(parent_id) = cursor {
            required.insert(parent_id.clone());
            cursor = item_by_id
                .get(&parent_id)
                .and_then(|item| item.parent_id.clone());
        }
    }
    let items = preview
        .items
        .iter()
        .filter(|item| required.contains(&item.id))
        .map(|item| {
            let explicitly_selected = selected.contains(&item.id);
            serde_json::json!({
                "id": item.id,
                "parentId": item.parent_id,
                "title": item.title,
                "status": item.status,
                "estimateMinutes": explicitly_selected.then_some(item.estimate_minutes).flatten(),
                "importActualMinutes": explicitly_selected.then_some(item.import_actual_minutes).flatten(),
                "sortOrder": item.sort_order,
                "sourceFingerprint": fingerprints.get(&item.id),
                "selected": explicitly_selected,
                "alreadyImported": item.already_imported,
            })
        })
        .collect::<Vec<_>>();
    let response = crate::supabase::direct_cloud_daily_report_import_confirm_mutation(
        database,
        request.client_request_id,
        serde_json::json!({
            "batchId": request.batch_id,
            "subjectId": preview.subject_id,
            "sourceDate": preview.source_date,
            "targetKind": preview.target_kind,
            "sourcePathHash": source_path_hash,
            "items": items,
        }),
    )?;
    let tasks = response
        .get("tasks")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|value| {
            serde_json::from_value::<crate::supabase::DirectCloudTaskData>(value)
                .map(TaskDto::from)
                .map_err(|error| format!("CLOUD_RESPONSE_INVALID: 日报导入事项响应无效: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let result = DailyReportImportConfirmResult {
        batch_id: preview.batch_id.clone(),
        target_kind: preview.target_kind,
        source_date: preview.source_date,
        created_count: response
            .get("createdCount")
            .and_then(|value| value.as_u64())
            .unwrap_or(tasks.len() as u64) as usize,
        skipped_count: response
            .get("skippedCount")
            .and_then(|value| value.as_u64())
            .unwrap_or(0) as usize,
        adjustment_count: response
            .get("adjustmentCount")
            .and_then(|value| value.as_u64())
            .unwrap_or(0) as usize,
        tasks,
        already_confirmed: false,
    };
    let mut registry = direct_daily_report_imports()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(state) = registry.get_mut(&preview.batch_id) {
        state.confirmed = Some(result.clone());
    }
    Ok(result)
}

fn load_items(
    connection: &rusqlite::Connection,
    batch_id: &str,
) -> Result<Vec<ImportItemRow>, String> {
    let mut statement = connection.prepare(
        "SELECT id,parent_item_id,title,item_status,estimate_minutes,import_actual_minutes,sort_order,source_fingerprint,already_imported
         FROM daily_report_import_items WHERE batch_id=?1 ORDER BY source_line_no,sort_order",
    ).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([batch_id], |row| {
            Ok(ImportItemRow {
                id: row.get(0)?,
                parent_item_id: row.get(1)?,
                title: row.get(2)?,
                status: row.get(3)?,
                estimate_minutes: row.get(4)?,
                import_actual_minutes: row.get(5)?,
                source_fingerprint: row.get(7)?,
                already_imported: row.get::<_, i64>(8)? != 0,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(rows)
}

fn parse_daily_report(markdown: &str) -> ParsedReport {
    let mut items = Vec::new();
    let mut warnings = Vec::new();
    let mut found_heading = false;
    let mut stack: Vec<(usize, String, String)> = Vec::new();
    let mut root_order = 0i64;
    let mut child_orders = HashMap::<String, i64>::new();
    for (index, raw_line) in markdown.lines().enumerate() {
        let trimmed = raw_line.trim();
        if trimmed.starts_with('#') {
            let heading = trimmed.trim_start_matches('#').trim();
            if !found_heading {
                found_heading = true;
                continue;
            }
            if is_plan_heading(heading) {
                break;
            }
            continue;
        }
        if !found_heading || trimmed.is_empty() {
            continue;
        }
        let Some(raw_title) = strip_list_marker(trimmed) else {
            warnings.push(format!("第 {} 行未识别为事项：{}", index + 1, trimmed));
            continue;
        };
        let indent = normalized_indent(raw_line);
        while stack
            .last()
            .is_some_and(|(previous, _, _)| *previous >= indent)
        {
            stack.pop();
        }
        let indentation_warning = if indent > 0 && stack.is_empty() {
            Some("缩进事项缺少可识别的父项，已按顶层事项处理".to_string())
        } else if let Some((parent_indent, _, _)) = stack.last() {
            (indent > *parent_indent + 4)
                .then(|| "缩进层级跳跃，已挂到最近的可识别父项".to_string())
        } else {
            None
        };
        let parent_client_id = stack.last().map(|(_, id, _)| id.clone());
        let parent_path = stack
            .last()
            .map(|(_, _, path)| path.clone())
            .unwrap_or_default();
        let sort_order = if let Some(parent) = parent_client_id.as_ref() {
            let order = child_orders.entry(parent.clone()).or_insert(0);
            *order += 10;
            *order
        } else {
            root_order += 10;
            root_order
        };
        let (title, status, estimate, raw_actual, import_actual, metadata_warning) =
            parse_title_metadata(&raw_title);
        let warning = [indentation_warning, metadata_warning]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let warning = (!warning.is_empty()).then(|| warning.join("；"));
        let client_id = format!("item-{}", items.len() + 1);
        let ordinal_path = if parent_path.is_empty() {
            sort_order.to_string()
        } else {
            format!("{parent_path}/{sort_order}")
        };
        let canonical = format!(
            "{ordinal_path}|{title}|{status}|{}|{}",
            estimate.map_or(String::new(), |v| v.to_string()),
            raw_actual.map_or(String::new(), |v| v.to_string())
        );
        items.push(ParsedItem {
            client_id: client_id.clone(),
            parent_client_id,
            title,
            status,
            estimate_minutes: estimate,
            raw_actual_minutes: raw_actual,
            import_actual_minutes: import_actual,
            sort_order,
            source_line_no: index + 1,
            source_text: raw_line.to_string(),
            warning,
            source_fingerprint: content_hash(&canonical),
        });
        stack.push((indent, client_id, ordinal_path));
    }
    if !found_heading {
        warnings.push("日报中没有找到 Markdown 标题".to_string());
    }
    if found_heading && items.is_empty() {
        warnings.push("日报顶部没有识别到可导入事项".to_string());
    }
    ParsedReport { items, warnings }
}

fn parse_title_metadata(
    value: &str,
) -> (
    String,
    String,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<String>,
) {
    let mut value = value.trim().to_string();
    let status = if let Some(rest) = value.strip_prefix('🟢') {
        value = rest.trim_start().to_string();
        "done"
    } else if let Some(rest) = value.strip_prefix('🔴') {
        value = rest.trim_start().to_string();
        "open"
    } else {
        "open"
    }
    .to_string();
    let mut estimate = None;
    let mut actual = None;
    let mut warnings = Vec::new();
    if let Some(start) = value.rfind('<') {
        if value.ends_with('>') {
            let metadata = value[start + 1..value.len() - 1].replace('：', ":");
            for (label, target) in [("预计", &mut estimate), ("实际", &mut actual)] {
                if let Some(position) = metadata.find(&format!("{label}:")) {
                    let rest = &metadata[position + label.len() + 1..];
                    let token = rest.split_whitespace().next().unwrap_or_default();
                    match parse_duration(token) {
                        Ok(minutes) => *target = Some(minutes),
                        Err(()) => warnings.push(format!("{label}时长无法识别")),
                    }
                }
            }
            value = value[..start].trim_end().to_string();
        }
    }
    let import_actual = (status == "done").then_some(actual).flatten();
    (
        value,
        status,
        estimate,
        actual,
        import_actual,
        (!warnings.is_empty()).then(|| warnings.join("；")),
    )
}

fn parse_duration(value: &str) -> Result<i64, ()> {
    let normalized = value.trim().to_ascii_lowercase();
    let minutes = if let Some(hours) = normalized.strip_suffix('h') {
        hours.parse::<f64>().ok().map(|v| (v * 60.0).round() as i64)
    } else if let Some(value) = normalized.strip_suffix("min") {
        value.parse::<f64>().ok().map(|v| v.round() as i64)
    } else {
        None
    };
    minutes.filter(|value| *value > 0).ok_or(())
}
fn is_plan_heading(value: &str) -> bool {
    value.contains("明日计划")
        || (value.contains("下周") && value.contains("计划"))
        || (value.contains("下一工作日") && value.contains("计划"))
}
fn normalized_indent(value: &str) -> usize {
    value
        .chars()
        .take_while(|c| c.is_whitespace())
        .map(|c| if c == '\t' { 4 } else { 1 })
        .sum()
}
fn strip_list_marker(value: &str) -> Option<String> {
    let value = value.trim_start();
    if let Some((number, title)) = value.split_once('.') {
        if !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()) {
            return non_empty(title);
        }
    }
    for marker in ["- ", "* ", "+ "] {
        if let Some(title) = value.strip_prefix(marker) {
            return non_empty(title);
        }
    }
    None
}
fn non_empty(value: &str) -> Option<String> {
    let value = value.trim().to_string();
    (!value.is_empty()).then_some(value)
}
fn report_path(directory: &str, date: &str) -> PathBuf {
    Path::new(directory).join(format!("{}.md", date.replace('-', "")))
}
fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/").to_lowercase()
}
fn subject_directory(
    connection: &rusqlite::Connection,
    subject_id: &str,
) -> Result<Option<String>, String> {
    let key = format!("subject_daily_report_directory:{subject_id}");
    let value: Option<String> = connection
        .query_row(
            "SELECT value_json FROM device_settings WHERE key=?1",
            [key],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    value
        .map(|v| serde_json::from_str::<String>(&v).map_err(|e| e.to_string()))
        .transpose()
}
fn adjustment_delta(target_minutes: i64, baseline_minutes: i64) -> Option<i64> {
    let delta = target_minutes - baseline_minutes;
    (delta != 0).then_some(delta)
}
fn workspace_id(c: &rusqlite::Connection) -> Result<String, String> {
    c.query_row(
        "SELECT id FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
        [],
        |r| r.get(0),
    )
    .map_err(|e| e.to_string())
}
fn device_id(c: &rusqlite::Connection, w: &str) -> Result<String, String> {
    c.query_row("SELECT id FROM devices WHERE workspace_id=?1 AND revoked_at IS NULL ORDER BY created_at LIMIT 1",[w],|r|r.get(0)).map_err(|e|e.to_string())
}
fn ensure_subject(c: &rusqlite::Connection, w: &str, s: &str) -> Result<(), String> {
    let exists:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM subjects WHERE workspace_id=?1 AND id=?2 AND deleted_at IS NULL)",params![w,s],|r|r.get(0)).map_err(|e|e.to_string())?;
    if exists {
        Ok(())
    } else {
        Err("NOT_FOUND: 主体不存在".to_string())
    }
}
fn validate_target_kind(v: &str) -> Result<(), String> {
    if matches!(v, BACKLOG | DAY_INSTANCE) {
        Ok(())
    } else {
        Err("VALIDATION_ERROR: 导入目标无效".to_string())
    }
}
fn validate_date(v: &str) -> Result<(), String> {
    chrono::NaiveDate::parse_from_str(v, "%Y-%m-%d")
        .map(|_| ())
        .map_err(|_| format!("VALIDATION_ERROR: 非法日期 {v}"))
}
fn content_hash(v: &str) -> String {
    format!("{:x}", Sha256::digest(v.as_bytes()))
}
fn system_time_millis(v: SystemTime) -> i64 {
    v.duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64
}
fn now_millis() -> i64 {
    system_time_millis(SystemTime::now())
}

#[tauri::command]
pub fn daily_report_import_preview(
    database: tauri::State<'_, Database>,
    request: DailyReportImportPreviewRequest,
) -> Result<DailyReportImportPreviewResult, String> {
    let direct_request = request.clone();
    crate::storage_contract::route(
        &database,
        crate::storage_contract::BusinessDomain::Reports,
        crate::storage_contract::StorageOperation::Query,
        |database| preview(database, request),
        |database| direct_preview(database, direct_request),
    )
}
#[tauri::command]
pub fn daily_report_import_confirm(
    database: tauri::State<'_, Database>,
    request: DailyReportImportConfirmRequest,
) -> Result<DailyReportImportConfirmResult, String> {
    let direct_request = request.clone();
    crate::storage_contract::route(
        &database,
        crate::storage_contract::BusinessDomain::Reports,
        crate::storage_contract::StorageOperation::Mutation,
        |database| confirm(database, request),
        |database| direct_confirm(database, direct_request),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parses_top_items_status_durations_and_stops_before_tomorrow() {
        let parsed=parse_daily_report("## 【晓健】2026-10-22\n\n1. 🟢 完成项 <预计：0.2h 实际：0.2h>\n2. 🔴 未完成 <预计: 2h 实际: 1.5h>\n3. 父项\n\t- 🟢 子项 <实际：20min>\n\n## 明日计划：\n1. 不应导入\n");
        assert_eq!(parsed.items.len(), 4);
        assert_eq!(parsed.items[0].title, "完成项");
        assert_eq!(parsed.items[0].estimate_minutes, Some(12));
        assert_eq!(parsed.items[0].import_actual_minutes, Some(12));
        assert_eq!(parsed.items[1].import_actual_minutes, None);
        assert_eq!(parsed.items[1].warning, None);
        assert_eq!(parsed.items[3].parent_client_id.as_deref(), Some("item-3"));
    }

    #[test]
    fn parses_markers_indent_warnings_and_duration_errors() {
        let parsed = parse_daily_report(
            "前言不会解析\n# 标题\n- 🟢 小时 <预计：2H 实际：20min>\n    * 普通子项\n            + 跳级子项\n\t- Tab 子项 <预计：-1h 实际：bad>\n这行无法识别\n## 下周一计划\n1. 不应出现\n",
        );
        assert_eq!(
            parsed
                .items
                .iter()
                .map(|item| item.title.as_str())
                .collect::<Vec<_>>(),
            vec!["小时", "普通子项", "跳级子项", "Tab 子项"]
        );
        assert_eq!(parsed.items[0].estimate_minutes, Some(120));
        assert_eq!(parsed.items[0].import_actual_minutes, Some(20));
        assert_eq!(parsed.items[1].parent_client_id.as_deref(), Some("item-1"));
        assert!(parsed.items[2]
            .warning
            .as_deref()
            .unwrap()
            .contains("缩进层级跳跃"));
        assert!(parsed.items[3]
            .warning
            .as_deref()
            .unwrap()
            .contains("预计时长无法识别"));
        assert!(parsed
            .warnings
            .iter()
            .any(|warning| warning.contains("这行无法识别")));
        assert!(!parsed
            .items
            .iter()
            .any(|item| item.title.contains("不应出现")));
    }

    #[test]
    fn backlog_and_day_instance_import_are_independently_idempotent() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("daily-import.sqlite3")).unwrap();
        let subject = crate::subjects::create_subject(
            &database,
            crate::subjects::SubjectCreateRequest {
                name: "晓健".to_string(),
            },
        )
        .unwrap()
        .subject;
        crate::settings::update_setting(
            &database,
            crate::settings::SettingsUpdate {
                scope: crate::settings::SettingsScope::Device,
                key: format!("subject_daily_report_directory:{}", subject.id),
                value: serde_json::json!(directory.path().display().to_string()),
            },
        )
        .unwrap();
        fs::write(
            directory.path().join("20261022.md"),
            "## 【晓健】2026-10-22\n\n1. 运维相关\n    - 🟢 已完成子项 <预计：0.2h 实际：20min>\n2. 🔴 调研方案 <预计：2h 实际：1.5h>\n\n## 明日计划：\n1. 不应导入\n",
        ).unwrap();

        let backlog = preview(
            &database,
            DailyReportImportPreviewRequest {
                subject_id: subject.id.clone(),
                source_date: "2026-10-22".to_string(),
                target_kind: BACKLOG.to_string(),
            },
        )
        .unwrap();
        assert_eq!(backlog.items.len(), 3);
        let backlog_result = confirm(
            &database,
            DailyReportImportConfirmRequest {
                batch_id: backlog.batch_id.clone(),
                selected_item_ids: vec![backlog.items[1].id.clone(), backlog.items[2].id.clone()],
                expected_version: backlog.version,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        assert_eq!(backlog_result.created_count, 3);
        assert_eq!(backlog_result.adjustment_count, 1);
        let replay = confirm(
            &database,
            DailyReportImportConfirmRequest {
                batch_id: backlog.batch_id.clone(),
                selected_item_ids: vec![backlog.items[1].id.clone()],
                expected_version: backlog.version,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        assert!(replay.already_confirmed);
        assert_eq!(replay.created_count, backlog_result.created_count);

        let repeated_backlog = preview(
            &database,
            DailyReportImportPreviewRequest {
                subject_id: subject.id.clone(),
                source_date: "2026-10-22".to_string(),
                target_kind: BACKLOG.to_string(),
            },
        )
        .unwrap();
        assert!(repeated_backlog
            .items
            .iter()
            .all(|item| item.already_imported));
        let day_preview = preview(
            &database,
            DailyReportImportPreviewRequest {
                subject_id: subject.id.clone(),
                source_date: "2026-10-22".to_string(),
                target_kind: DAY_INSTANCE.to_string(),
            },
        )
        .unwrap();
        assert!(day_preview.items.iter().all(|item| !item.already_imported));
        let day_result = confirm(
            &database,
            DailyReportImportConfirmRequest {
                batch_id: day_preview.batch_id.clone(),
                selected_item_ids: vec![day_preview.items[1].id.clone()],
                expected_version: day_preview.version,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        assert_eq!(day_result.created_count, 2);

        let connection = database.open().unwrap();
        let backlog_count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM tasks WHERE subject_id=?1 AND day_instance_date IS NULL AND deleted_at IS NULL",
            [&subject.id], |row| row.get(0),
        ).unwrap();
        assert_eq!(backlog_count, 3);
        let day_rows: Vec<(String, String, Option<i64>, Option<String>)> = {
            let mut statement = connection.prepare(
                "SELECT title,day_instance_kind,estimate_minutes,parent_id FROM tasks WHERE subject_id=?1 AND day_instance_date='2026-10-22' ORDER BY sort_order",
            ).unwrap();
            statement
                .query_map([&subject.id], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert_eq!(day_rows.len(), 2);
        assert_eq!(day_rows[0].1, "context");
        assert_eq!(day_rows[0].2, None);
        assert_eq!(day_rows[1].1, "member");
        assert_eq!(day_rows[1].2, Some(12));
        assert!(day_rows[1].3.is_some());
        let daily_estimate: i64 = connection.query_row(
            "SELECT estimate_minutes FROM task_daily_estimates WHERE task_id=?1 AND work_date='2026-10-22'",
            [&day_result.tasks.iter().find(|task| task.title == "已完成子项").unwrap().id], |row| row.get(0),
        ).unwrap();
        assert_eq!(daily_estimate, 12);
        let adjustments: Vec<(String, i64, String)> = {
            let mut statement = connection.prepare(
                "SELECT source_type,target_minutes,note FROM actual_time_adjustments ORDER BY created_at",
            ).unwrap();
            statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert_eq!(adjustments.len(), 2);
        assert!(adjustments
            .iter()
            .all(|(source, target, note)| source == "daily_report_import"
                && *target == 20
                && note.contains("Obsidian 日报")));
        let red_adjustments: i64 = connection.query_row(
            "SELECT COUNT(*) FROM actual_time_adjustments a JOIN tasks t ON t.id=a.task_id WHERE t.title='调研方案'",
            [], |row| row.get(0),
        ).unwrap();
        assert_eq!(red_adjustments, 0);
    }

    #[test]
    fn confirm_rejects_changed_source_file() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("changed-source.sqlite3")).unwrap();
        let subject = crate::subjects::create_subject(
            &database,
            crate::subjects::SubjectCreateRequest {
                name: "变更测试".to_string(),
            },
        )
        .unwrap()
        .subject;
        crate::settings::update_setting(
            &database,
            crate::settings::SettingsUpdate {
                scope: crate::settings::SettingsScope::Device,
                key: format!("subject_daily_report_directory:{}", subject.id),
                value: serde_json::json!(directory.path().display().to_string()),
            },
        )
        .unwrap();
        let path = directory.path().join("20261022.md");
        fs::write(&path, "# 日报\n1. 原事项\n").unwrap();
        let initial_task_count: i64 = database
            .open()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get(0))
            .unwrap();
        let preview_result = preview(
            &database,
            DailyReportImportPreviewRequest {
                subject_id: subject.id,
                source_date: "2026-10-22".to_string(),
                target_kind: BACKLOG.to_string(),
            },
        )
        .unwrap();
        fs::write(&path, "# 日报\n1. 已变化事项\n").unwrap();
        let error = confirm(
            &database,
            DailyReportImportConfirmRequest {
                batch_id: preview_result.batch_id,
                selected_item_ids: vec![preview_result.items[0].id.clone()],
                expected_version: preview_result.version,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap_err();
        assert!(error.contains("EXTERNAL_FILE_CHANGED"));
        assert_eq!(
            database
                .open()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            initial_task_count
        );
    }

    #[test]
    fn confirm_rolls_back_earlier_items_when_a_later_item_fails() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(directory.path().join("rollback.sqlite3")).unwrap();
        let subject = crate::subjects::create_subject(
            &database,
            crate::subjects::SubjectCreateRequest {
                name: "回滚测试".to_string(),
            },
        )
        .unwrap()
        .subject;
        crate::settings::update_setting(
            &database,
            crate::settings::SettingsUpdate {
                scope: crate::settings::SettingsScope::Device,
                key: format!("subject_daily_report_directory:{}", subject.id),
                value: serde_json::json!(directory.path().display().to_string()),
            },
        )
        .unwrap();
        fs::write(
            directory.path().join("20261022.md"),
            "# 日报\n1. 第一项\n2. 第二项\n",
        )
        .unwrap();
        let preview_result = preview(
            &database,
            DailyReportImportPreviewRequest {
                subject_id: subject.id.clone(),
                source_date: "2026-10-22".to_string(),
                target_kind: BACKLOG.to_string(),
            },
        )
        .unwrap();
        let connection = database.open().unwrap();
        connection
            .execute(
                "UPDATE daily_report_import_items SET title='' WHERE id=?1",
                [&preview_result.items[1].id],
            )
            .unwrap();
        let initial_task_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get(0))
            .unwrap();
        drop(connection);
        assert!(confirm(
            &database,
            DailyReportImportConfirmRequest {
                batch_id: preview_result.batch_id,
                selected_item_ids: preview_result
                    .items
                    .iter()
                    .map(|item| item.id.clone())
                    .collect(),
                expected_version: preview_result.version,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .is_err());
        let connection = database.open().unwrap();
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            initial_task_count
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM daily_report_import_sources",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        assert_eq!(connection.query_row("SELECT COUNT(*) FROM actual_time_adjustments WHERE source_type='daily_report_import'", [], |row| row.get::<_,i64>(0)).unwrap(), 0);
    }

    #[test]
    fn preview_reports_configuration_missing_file_and_invalid_utf8() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("preview-errors.sqlite3")).unwrap();
        let subject = crate::subjects::create_subject(
            &database,
            crate::subjects::SubjectCreateRequest {
                name: "预览错误".to_string(),
            },
        )
        .unwrap()
        .subject;
        let request = || DailyReportImportPreviewRequest {
            subject_id: subject.id.clone(),
            source_date: "2026-10-22".to_string(),
            target_kind: BACKLOG.to_string(),
        };
        assert!(preview(&database, request())
            .unwrap_err()
            .contains("INTEGRATION_NOT_CONFIGURED"));
        crate::settings::update_setting(
            &database,
            crate::settings::SettingsUpdate {
                scope: crate::settings::SettingsScope::Device,
                key: format!("subject_daily_report_directory:{}", subject.id),
                value: serde_json::json!(directory.path().display().to_string()),
            },
        )
        .unwrap();
        assert!(preview(&database, request())
            .unwrap_err()
            .contains("SOURCE_NOT_FOUND"));
        fs::write(directory.path().join("20261022.md"), [0xff, 0xfe, 0xfd]).unwrap();
        assert!(preview(&database, request())
            .unwrap_err()
            .contains("无法读取日报文件"));
    }

    #[test]
    fn fingerprint_changes_with_source_content() {
        let first = parse_daily_report("# 日报\n1. 🟢 同名事项 <预计：1h 实际：30min>\n");
        let second = parse_daily_report("# 日报\n1. 🟢 同名事项 <预计：2h 实际：30min>\n");
        assert_ne!(
            first.items[0].source_fingerprint,
            second.items[0].source_fingerprint
        );
    }

    #[test]
    fn actual_target_delta_covers_zero_existing_zero_and_negative_differences() {
        assert_eq!(adjustment_delta(90, 0), Some(90));
        assert_eq!(adjustment_delta(90, 40), Some(50));
        assert_eq!(adjustment_delta(90, 90), None);
        assert_eq!(adjustment_delta(30, 90), Some(-60));
    }
}
