use std::collections::{HashMap, HashSet};

use chrono::{Datelike, Duration, NaiveDate, TimeZone};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::database::Database;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ReportDto {
    pub id: String,
    pub report_type: String,
    pub subject_id: String,
    pub subject_name: String,
    pub period_start: String,
    pub period_end: String,
    pub period: String,
    pub reference_date: String,
    pub task_ids: Vec<String>,
    pub template_id: Option<String>,
    pub template_name: Option<String>,
    pub template_archived: bool,
    pub markdown: String,
    pub content_source: String,
    pub generated_count: i64,
    pub updated_at: i64,
    pub version: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportListResult {
    pub reports: Vec<ReportDto>,
    pub revision: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportListRequest {
    pub report_type: Option<String>,
    pub subject_id: Option<String>,
    pub query: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportTaskSuggestionRequest {
    pub report_type: String,
    pub reference_date: String,
    pub subject_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportTaskSuggestionDto {
    pub task_id: String,
    pub actual_minutes: i64,
    pub daily_estimate_minutes: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportWorkPeriodPreviewRequest {
    pub subject_id: String,
    pub reference_date: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportCreateRequest {
    pub report_type: String,
    pub reference_date: String,
    pub subject_id: String,
    pub task_ids: Vec<String>,
    #[serde(default)]
    pub template_id: Option<String>,
    pub client_request_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportScopeUpdateRequest {
    pub report_id: String,
    pub report_type: String,
    pub reference_date: String,
    pub subject_id: String,
    pub task_ids: Vec<String>,
    #[serde(default)]
    pub template_id: Option<String>,
    pub expected_version: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportContentSaveRequest {
    pub report_id: String,
    pub markdown: String,
    pub expected_version: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportVersionRequest {
    pub report_id: String,
    pub expected_version: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicReportDto {
    pub report_id: String,
    pub markdown: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ReportTemplateDto {
    pub id: String,
    pub report_type: String,
    pub name: String,
    pub description: Option<String>,
    pub content: String,
    pub is_default: bool,
    pub is_builtin: bool,
    pub sort_order: i64,
    pub deleted_at: Option<i64>,
    pub version: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportTemplateGetRequest {
    pub report_type: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub subject_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportTemplateSaveRequest {
    pub report_type: String,
    #[allow(dead_code)]
    pub subject_id: Option<String>,
    pub content: String,
    pub expected_version: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportTemplateListRequest {
    pub report_type: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportTemplateIdRequest {
    pub template_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportTemplateCreateRequest {
    pub report_type: String,
    pub name: String,
    pub description: Option<String>,
    pub content: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportTemplateUpdateRequest {
    pub template_id: String,
    pub name: String,
    pub description: Option<String>,
    pub content: String,
    pub sort_order: i64,
    pub expected_version: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportTemplateDuplicateRequest {
    pub template_id: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportTemplateSetDefaultRequest {
    pub template_id: String,
    pub expected_version: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportTemplateDeleteRequest {
    pub template_id: String,
    pub replacement_template_id: Option<String>,
    pub expected_version: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportTemplateReorderRequest {
    pub report_type: String,
    pub template_ids: Vec<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ReportTemplatePlaceholderDto {
    pub token: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub example: &'static str,
    pub report_types: &'static [&'static str],
}

#[derive(Debug, Clone)]
struct TaskRow {
    id: String,
    parent_id: Option<String>,
    title: String,
    status: String,
    estimate_minutes: Option<i64>,
    daily_estimate_minutes: Option<i64>,
    sort_order: i64,
    recurrence: Option<crate::recurring::RecurrenceRuleDto>,
    occurrence_status: Option<String>,
    occurrence_origin: Option<String>,
}

pub fn list_reports(
    database: &Database,
    request: ReportListRequest,
) -> Result<ReportListResult, String> {
    let connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let mut statement = connection
        .prepare(
            "SELECT r.id FROM reports r JOIN subjects s ON s.id = r.subject_id
             WHERE r.workspace_id = ?1 AND r.deleted_at IS NULL
               AND (?2 IS NULL OR ?2 = 'all' OR r.report_type = ?2)
               AND (?3 IS NULL OR r.subject_id = ?3)
               AND (?4 IS NULL OR ?4 = '' OR r.period_start LIKE '%' || ?4 || '%'
                    OR r.period_end LIKE '%' || ?4 || '%' OR s.name LIKE '%' || ?4 || '%')
             ORDER BY r.updated_at DESC",
        )
        .map_err(|error| error.to_string())?;
    let ids = statement
        .query_map(
            params![
                workspace_id,
                request.report_type,
                request.subject_id,
                request.query
            ],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let reports = ids
        .iter()
        .map(|id| load_report(&connection, id))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ReportListResult {
        reports,
        revision: current_revision(&connection)?,
    })
}

pub fn suggest_report_tasks(
    database: &Database,
    request: ReportTaskSuggestionRequest,
) -> Result<Vec<ReportTaskSuggestionDto>, String> {
    let (period_start, period_end) =
        normalize_period(&request.report_type, &request.reference_date)?;
    let connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    ensure_subject(&connection, &workspace_id, &request.subject_id)?;
    let daily_estimate_date = (request.report_type == "daily").then_some(period_start.as_str());
    let tasks = load_tasks(
        &connection,
        &workspace_id,
        &request.subject_id,
        daily_estimate_date,
    )?;
    let actuals = task_actual_minutes(&connection, &workspace_id, &period_start, &period_end)?;
    Ok(tasks
        .into_iter()
        .filter_map(|task| {
            let actual_minutes = actuals.get(&task.id).copied().unwrap_or(0);
            (actual_minutes > 0).then_some(ReportTaskSuggestionDto {
                task_id: task.id,
                actual_minutes,
                daily_estimate_minutes: task.daily_estimate_minutes,
            })
        })
        .collect())
}

pub fn preview_report_work_period(
    database: &Database,
    request: ReportWorkPeriodPreviewRequest,
) -> Result<String, String> {
    let date = NaiveDate::parse_from_str(&request.reference_date, "%Y-%m-%d")
        .map_err(|_| "VALIDATION_ERROR: 报告日期格式无效".to_string())?;
    let connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    ensure_subject(&connection, &workspace_id, &request.subject_id)?;
    daily_work_period_text(
        &connection,
        &workspace_id,
        &request.subject_id,
        &request.reference_date,
        date.weekday().num_days_from_monday() as i64,
    )
}

#[cfg(test)]
pub fn create_report(
    database: &Database,
    request: ReportCreateRequest,
) -> Result<ReportDto, String> {
    create_report_with_cloud_operation(database, request, None)
}

fn create_report_with_cloud_operation(
    database: &Database,
    request: ReportCreateRequest,
    cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str, Option<&str>)>,
) -> Result<ReportDto, String> {
    validate_operation_id(&request.client_request_id)?;
    let (period_start, period_end) =
        normalize_period(&request.report_type, &request.reference_date)?;
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let device_id = device_id(&connection, &workspace_id)?;
    if let Some(result) =
        processed_result::<ReportDto>(&connection, &workspace_id, &request.client_request_id)?
    {
        return Ok(result);
    }
    ensure_subject(&connection, &workspace_id, &request.subject_id)?;
    if let Some(template_id) = request.template_id.as_deref() {
        load_selectable_template(
            &connection,
            &workspace_id,
            template_id,
            &request.report_type,
        )?;
    }
    let existing: Option<String> = connection
        .query_row(
            "SELECT id FROM reports WHERE workspace_id = ?1 AND report_type = ?2 AND subject_id = ?3
             AND period_start = ?4 AND period_end = ?5 AND deleted_at IS NULL",
            params![workspace_id, request.report_type, request.subject_id, period_start, period_end],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if let Some(id) = existing {
        return Err(format!(
            "REPORT_ALREADY_EXISTS: 当前日期或周期已有报告|{id}"
        ));
    }
    let id = Uuid::now_v7().to_string();
    let now = now_millis();
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let (template_id, markdown) = render_report(
        &transaction,
        &workspace_id,
        &request.report_type,
        request.template_id.as_deref(),
        &request.subject_id,
        &period_start,
        &period_end,
        &request.task_ids,
    )?;
    transaction
        .execute(
            "INSERT INTO reports(
               id, workspace_id, report_type, subject_id, period_start, period_end, reference_date,
               template_id, markdown_content, content_source, generation_count, generated_at,
               created_at, updated_at, version, created_by_device_id, updated_by_device_id
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'generated', 1, ?10, ?10, ?10, 1, ?11, ?11)",
            params![id, workspace_id, request.report_type, request.subject_id, period_start, period_end, request.reference_date, template_id, markdown, now, device_id],
        )
        .map_err(|error| error.to_string())?;
    replace_report_tasks(
        &transaction,
        &workspace_id,
        &id,
        &request.subject_id,
        &request.task_ids,
    )?;
    bump_revision(&transaction)?;
    let result = load_report(&transaction, &id)?;
    save_processed_result(
        &transaction,
        &workspace_id,
        &device_id,
        &request.client_request_id,
        "report_create",
        &result,
    )?;
    if let Some((state, operation_type, operation_id)) = cloud_operation {
        crate::cloud_sync::enqueue_entity_in_transaction(
            &transaction,
            state,
            operation_type,
            "report",
            Some(&id),
            None,
            operation_id,
        )?;
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

fn update_report_scope_with_cloud_operation(
    database: &Database,
    request: ReportScopeUpdateRequest,
    cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str)>,
) -> Result<ReportDto, String> {
    let (period_start, period_end) =
        normalize_period(&request.report_type, &request.reference_date)?;
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    ensure_subject(&connection, &workspace_id, &request.subject_id)?;
    let duplicate: Option<String> = connection
        .query_row(
            "SELECT id FROM reports WHERE workspace_id = ?1 AND report_type = ?2 AND subject_id = ?3
             AND period_start = ?4 AND period_end = ?5 AND id != ?6 AND deleted_at IS NULL",
            params![workspace_id, request.report_type, request.subject_id, period_start, period_end, request.report_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if let Some(id) = duplicate {
        return Err(format!(
            "REPORT_ALREADY_EXISTS: 当前日期或周期已有报告|{id}"
        ));
    }
    let now = now_millis();
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let current = load_report(&transaction, &request.report_id)?;
    let template_id = match request.template_id.as_deref() {
        Some(id) => {
            load_selectable_template(&transaction, &workspace_id, id, &request.report_type)?;
            Some(id.to_string())
        }
        None if current.report_type == request.report_type => current.template_id,
        None => {
            return Err("VALIDATION_ERROR: 切换报告类型时请选择对应类型的模板".to_string());
        }
    };
    let changed = transaction
        .execute(
            "UPDATE reports SET report_type = ?1, subject_id = ?2, period_start = ?3, period_end = ?4,
                    reference_date = ?5, template_id = ?6, updated_at = ?7, version = version + 1
             WHERE id = ?8 AND workspace_id = ?9 AND version = ?10 AND deleted_at IS NULL",
            params![request.report_type, request.subject_id, period_start, period_end, request.reference_date, template_id, now, request.report_id, workspace_id, request.expected_version],
        )
        .map_err(|error| error.to_string())?;
    if changed == 0 {
        return Err("VERSION_CONFLICT: 报告范围已变化".to_string());
    }
    transaction
        .execute(
            "DELETE FROM report_tasks WHERE report_id = ?1",
            [&request.report_id],
        )
        .map_err(|error| error.to_string())?;
    replace_report_tasks(
        &transaction,
        &workspace_id,
        &request.report_id,
        &request.subject_id,
        &request.task_ids,
    )?;
    bump_revision(&transaction)?;
    let result = load_report(&transaction, &request.report_id)?;
    if let Some((state, operation_type)) = cloud_operation {
        crate::cloud_sync::enqueue_entity_in_transaction(
            &transaction,
            state,
            operation_type,
            "report",
            Some(&request.report_id),
            Some(request.expected_version),
            None,
        )?;
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

#[cfg(test)]
pub fn save_report_content(
    database: &Database,
    request: ReportContentSaveRequest,
) -> Result<ReportDto, String> {
    save_report_content_with_cloud_operation(database, request, None)
}

fn save_report_content_with_cloud_operation(
    database: &Database,
    request: ReportContentSaveRequest,
    cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str)>,
) -> Result<ReportDto, String> {
    if request.markdown.trim().is_empty() {
        return Err("VALIDATION_ERROR: 报告内容不能为空".to_string());
    }
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let now = now_millis();
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let changed = transaction
        .execute(
            "UPDATE reports SET markdown_content = ?1, content_source = 'edited', updated_at = ?2, version = version + 1
             WHERE id = ?3 AND workspace_id = ?4 AND version = ?5 AND deleted_at IS NULL",
            params![request.markdown, now, request.report_id, workspace_id, request.expected_version],
        )
        .map_err(|error| error.to_string())?;
    if changed == 0 {
        return Err("VERSION_CONFLICT: 报告内容已变化".to_string());
    }
    bump_revision(&transaction)?;
    let result = load_report(&transaction, &request.report_id)?;
    if let Some((state, operation_type)) = cloud_operation {
        crate::cloud_sync::enqueue_entity_in_transaction(
            &transaction,
            state,
            operation_type,
            "report",
            Some(&request.report_id),
            Some(request.expected_version),
            None,
        )?;
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

#[cfg(test)]
pub fn regenerate_report(
    database: &Database,
    request: ReportVersionRequest,
) -> Result<ReportDto, String> {
    regenerate_report_with_cloud_operation(database, request, None)
}

fn regenerate_report_with_cloud_operation(
    database: &Database,
    request: ReportVersionRequest,
    cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str)>,
) -> Result<ReportDto, String> {
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let current = load_report(&connection, &request.report_id)?;
    if current.version != request.expected_version {
        return Err("VERSION_CONFLICT: 报告已变化".to_string());
    }
    let now = now_millis();
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let (template_id, markdown) = render_report(
        &transaction,
        &workspace_id,
        &current.report_type,
        current.template_id.as_deref(),
        &current.subject_id,
        &current.period_start,
        &current.period_end,
        &current.task_ids,
    )?;
    let changed = transaction
        .execute(
            "UPDATE reports SET template_id = ?1, markdown_content = ?2, content_source = 'generated',
                    generation_count = generation_count + 1, generated_at = ?3, updated_at = ?3, version = version + 1
             WHERE id = ?4 AND version = ?5 AND deleted_at IS NULL",
            params![template_id, markdown, now, request.report_id, request.expected_version],
        )
        .map_err(|error| error.to_string())?;
    if changed == 0 {
        return Err("VERSION_CONFLICT: 报告已变化".to_string());
    }
    bump_revision(&transaction)?;
    let result = load_report(&transaction, &request.report_id)?;
    if let Some((state, operation_type)) = cloud_operation {
        crate::cloud_sync::enqueue_entity_in_transaction(
            &transaction,
            state,
            operation_type,
            "report",
            Some(&request.report_id),
            Some(request.expected_version),
            None,
        )?;
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

fn delete_report_with_cloud_operation(
    database: &Database,
    request: ReportVersionRequest,
    cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str)>,
) -> Result<(), String> {
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let now = now_millis();
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let changed = transaction
        .execute(
            "UPDATE reports SET deleted_at = ?1, updated_at = ?1, version = version + 1
             WHERE id = ?2 AND workspace_id = ?3 AND version = ?4 AND deleted_at IS NULL",
            params![
                now,
                request.report_id,
                workspace_id,
                request.expected_version
            ],
        )
        .map_err(|error| error.to_string())?;
    if changed == 0 {
        return Err("VERSION_CONFLICT: 报告已变化或已删除".to_string());
    }
    bump_revision(&transaction)?;
    if let Some((state, operation_type)) = cloud_operation {
        crate::cloud_sync::enqueue_entity_in_transaction(
            &transaction,
            state,
            operation_type,
            "report",
            Some(&request.report_id),
            Some(request.expected_version),
            None,
        )?;
    }
    transaction.commit().map_err(|error| error.to_string())
}

pub fn public_report(
    database: &Database,
    request: ReportVersionRequest,
) -> Result<PublicReportDto, String> {
    let connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let report = load_report(&connection, &request.report_id)?;
    if report.version != request.expected_version {
        return Err("VERSION_CONFLICT: 报告已变化".to_string());
    }
    let (_, markdown) = render_report(
        &connection,
        &workspace_id,
        &report.report_type,
        report.template_id.as_deref(),
        &report.subject_id,
        &report.period_start,
        &report.period_end,
        &report.task_ids,
    )?;
    Ok(PublicReportDto {
        report_id: report.id,
        markdown,
    })
}

pub fn get_template(
    database: &Database,
    request: ReportTemplateGetRequest,
) -> Result<ReportTemplateDto, String> {
    validate_report_type(&request.report_type)?;
    let connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    load_default_template(&connection, &workspace_id, &request.report_type)
}

pub fn list_templates(
    database: &Database,
    request: ReportTemplateListRequest,
) -> Result<Vec<ReportTemplateDto>, String> {
    if let Some(report_type) = request.report_type.as_deref() {
        validate_report_type(report_type)?;
    }
    let connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let mut statement = connection
        .prepare(
            "SELECT id FROM report_templates
             WHERE workspace_id = ?1 AND deleted_at IS NULL
               AND (?2 IS NULL OR report_type = ?2)
             ORDER BY CASE report_type WHEN 'daily' THEN 1 WHEN 'weekly' THEN 2 ELSE 3 END,
                      sort_order, lower(name), id",
        )
        .map_err(|error| error.to_string())?;
    let ids = statement
        .query_map(params![workspace_id, request.report_type], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    ids.iter()
        .map(|id| load_template_by_id(&connection, id, false))
        .collect()
}

pub fn get_template_by_id(
    database: &Database,
    request: ReportTemplateIdRequest,
) -> Result<ReportTemplateDto, String> {
    let connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    load_template_in_workspace(&connection, &workspace_id, &request.template_id, true)
}

fn create_template_with_cloud_operation(
    database: &Database,
    request: ReportTemplateCreateRequest,
    cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str)>,
) -> Result<ReportTemplateDto, String> {
    validate_report_type(&request.report_type)?;
    let name = validate_template_name(&request.name)?;
    validate_template(&request.report_type, &request.content)?;
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    ensure_template_name_available(
        &connection,
        &workspace_id,
        &request.report_type,
        &name,
        None,
    )?;
    let id = Uuid::now_v7().to_string();
    let now = now_millis();
    let sort_order: i64 = connection
        .query_row(
            "SELECT COALESCE(MAX(sort_order), 0) + 10 FROM report_templates
             WHERE workspace_id = ?1 AND report_type = ?2 AND deleted_at IS NULL",
            params![workspace_id, request.report_type],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    transaction.execute(
        "INSERT INTO report_templates(
           id,workspace_id,report_type,subject_id,name,description,content,is_default,is_builtin,sort_order,created_at,updated_at,version
         ) VALUES (?1,?2,?3,NULL,?4,?5,?6,0,0,?7,?8,?8,1)",
        params![id, workspace_id, request.report_type, name, request.description.and_then(normalize_optional_text), request.content, sort_order, now],
    ).map_err(map_template_write_error)?;
    bump_revision(&transaction)?;
    let result = load_template_by_id(&transaction, &id, false)?;
    enqueue_template_change(&transaction, cloud_operation, "report_template", &id, None)?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

#[cfg(test)]
pub fn create_template_for_cloud_test(
    database: &Database,
    request: ReportTemplateCreateRequest,
) -> Result<ReportTemplateDto, String> {
    let state = crate::supabase::storage_mode(database)?;
    create_template_with_cloud_operation(
        database,
        request,
        Some((&state, "report_template_create")),
    )
}

fn update_template_with_cloud_operation(
    database: &Database,
    request: ReportTemplateUpdateRequest,
    cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str)>,
) -> Result<ReportTemplateDto, String> {
    let name = validate_template_name(&request.name)?;
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let current =
        load_template_in_workspace(&connection, &workspace_id, &request.template_id, false)?;
    validate_template(&current.report_type, &request.content)?;
    ensure_template_name_available(
        &connection,
        &workspace_id,
        &current.report_type,
        &name,
        Some(&current.id),
    )?;
    let now = now_millis();
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let changed = transaction.execute(
        "UPDATE report_templates SET name=?1,description=?2,content=?3,sort_order=?4,updated_at=?5,version=version+1
         WHERE id=?6 AND workspace_id=?7 AND version=?8 AND deleted_at IS NULL",
        params![name, request.description.and_then(normalize_optional_text), request.content, request.sort_order, now, request.template_id, workspace_id, request.expected_version],
    ).map_err(map_template_write_error)?;
    if changed == 0 {
        return Err("VERSION_CONFLICT: 报告模板已变化".to_string());
    }
    bump_revision(&transaction)?;
    let result = load_template_by_id(&transaction, &request.template_id, false)?;
    enqueue_template_change(
        &transaction,
        cloud_operation,
        "report_template",
        &request.template_id,
        Some(request.expected_version),
    )?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

fn duplicate_template_with_cloud_operation(
    database: &Database,
    request: ReportTemplateDuplicateRequest,
    cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str)>,
) -> Result<ReportTemplateDto, String> {
    let connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let source =
        load_template_in_workspace(&connection, &workspace_id, &request.template_id, false)?;
    drop(connection);
    create_template_with_cloud_operation(
        database,
        ReportTemplateCreateRequest {
            report_type: source.report_type,
            name: request.name,
            description: request.description.or(source.description),
            content: source.content,
        },
        cloud_operation,
    )
}

fn set_default_template_with_cloud_operation(
    database: &Database,
    request: ReportTemplateSetDefaultRequest,
    cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str)>,
) -> Result<ReportTemplateDto, String> {
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let target =
        load_template_in_workspace(&connection, &workspace_id, &request.template_id, false)?;
    if target.version != request.expected_version {
        return Err("VERSION_CONFLICT: 报告模板已变化".to_string());
    }
    let now = now_millis();
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    transaction.execute(
        "UPDATE report_templates SET is_default=0,updated_at=?1,version=version+1
         WHERE workspace_id=?2 AND report_type=?3 AND is_default=1 AND id<>?4 AND deleted_at IS NULL",
        params![now, workspace_id, target.report_type, target.id],
    ).map_err(|error| error.to_string())?;
    let changed = transaction
        .execute(
            "UPDATE report_templates SET is_default=1,updated_at=?1,version=version+1
         WHERE id=?2 AND workspace_id=?3 AND version=?4 AND deleted_at IS NULL",
            params![now, target.id, workspace_id, request.expected_version],
        )
        .map_err(|error| error.to_string())?;
    if changed == 0 {
        return Err("VERSION_CONFLICT: 报告模板已变化".to_string());
    }
    bump_revision(&transaction)?;
    let result = load_template_by_id(&transaction, &target.id, false)?;
    enqueue_template_change(
        &transaction,
        cloud_operation,
        "report_template_set_default",
        &target.id,
        Some(request.expected_version),
    )?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

#[cfg(test)]
pub fn set_default_template_for_cloud_test(
    database: &Database,
    request: ReportTemplateSetDefaultRequest,
) -> Result<ReportTemplateDto, String> {
    let state = crate::supabase::storage_mode(database)?;
    set_default_template_with_cloud_operation(
        database,
        request,
        Some((&state, "report_template_set_default")),
    )
}

fn delete_template_with_cloud_operation(
    database: &Database,
    request: ReportTemplateDeleteRequest,
    cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str)>,
) -> Result<(), String> {
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let target =
        load_template_in_workspace(&connection, &workspace_id, &request.template_id, false)?;
    if target.version != request.expected_version {
        return Err("VERSION_CONFLICT: 报告模板已变化".to_string());
    }
    let active_count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM report_templates WHERE workspace_id=?1 AND report_type=?2 AND deleted_at IS NULL",
        params![workspace_id, target.report_type], |row| row.get(0),
    ).map_err(|error| error.to_string())?;
    if active_count <= 1 {
        return Err("VALIDATION_ERROR: 每种报告类型至少需要保留一份模板".to_string());
    }
    let replacement = if target.is_default {
        let id = request
            .replacement_template_id
            .as_deref()
            .ok_or_else(|| "VALIDATION_ERROR: 删除默认模板时请选择替代默认模板".to_string())?;
        Some(load_selectable_template(
            &connection,
            &workspace_id,
            id,
            &target.report_type,
        )?)
    } else {
        None
    };
    let now = now_millis();
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let changed = transaction
        .execute(
            "UPDATE report_templates SET is_default=0,deleted_at=?1,updated_at=?1,version=version+1
         WHERE id=?2 AND workspace_id=?3 AND version=?4 AND deleted_at IS NULL",
            params![now, target.id, workspace_id, request.expected_version],
        )
        .map_err(|error| error.to_string())?;
    if changed == 0 {
        return Err("VERSION_CONFLICT: 报告模板已变化".to_string());
    }
    if let Some(replacement) = replacement.as_ref() {
        transaction.execute(
            "UPDATE report_templates SET is_default=1,updated_at=?1,version=version+1 WHERE id=?2",
            params![now, replacement.id],
        ).map_err(|error| error.to_string())?;
    }
    bump_revision(&transaction)?;
    if let Some((state, operation_type)) = cloud_operation {
        let mut payload = crate::cloud_sync::entity_payload_from_connection(
            &transaction,
            "report_template",
            Some(&target.id),
        )?;
        if let Some(replacement) = replacement.as_ref() {
            payload
                .as_object_mut()
                .ok_or_else(|| "CACHE_ERROR: 报告模板同步快照不是对象".to_string())?
                .insert(
                    "replacement_template_id".to_string(),
                    serde_json::Value::String(replacement.id.clone()),
                );
        }
        crate::cloud_sync::enqueue_payload_in_transaction(
            &transaction,
            state,
            operation_type,
            "report_template",
            &target.id,
            Some(request.expected_version),
            &payload,
            None,
            None,
        )?;
    }
    transaction.commit().map_err(|error| error.to_string())
}

#[cfg(test)]
pub fn delete_template_for_cloud_test(
    database: &Database,
    request: ReportTemplateDeleteRequest,
) -> Result<(), String> {
    let state = crate::supabase::storage_mode(database)?;
    delete_template_with_cloud_operation(
        database,
        request,
        Some((&state, "report_template_delete")),
    )
}

fn reorder_templates_with_cloud_operation(
    database: &Database,
    request: ReportTemplateReorderRequest,
    cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str)>,
) -> Result<Vec<ReportTemplateDto>, String> {
    validate_report_type(&request.report_type)?;
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let current = list_templates(
        database,
        ReportTemplateListRequest {
            report_type: Some(request.report_type.clone()),
        },
    )?;
    let current_ids: HashSet<_> = current.iter().map(|item| item.id.as_str()).collect();
    let request_ids: HashSet<_> = request.template_ids.iter().map(String::as_str).collect();
    if current_ids != request_ids {
        return Err("VALIDATION_ERROR: 模板排序列表不完整".to_string());
    }
    let now = now_millis();
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    for (index, id) in request.template_ids.iter().enumerate() {
        let base_version = current
            .iter()
            .find(|item| item.id == *id)
            .map(|item| item.version);
        transaction.execute(
            "UPDATE report_templates SET sort_order=?1,updated_at=?2,version=version+1 WHERE id=?3 AND workspace_id=?4",
            params![(index as i64 + 1) * 10, now, id, workspace_id],
        ).map_err(|error| error.to_string())?;
        enqueue_template_change(
            &transaction,
            cloud_operation,
            "report_template_reorder",
            id,
            base_version,
        )?;
    }
    bump_revision(&transaction)?;
    transaction.commit().map_err(|error| error.to_string())?;
    list_templates(
        database,
        ReportTemplateListRequest {
            report_type: Some(request.report_type),
        },
    )
}

fn save_template_with_cloud_operation(
    database: &Database,
    request: ReportTemplateSaveRequest,
    cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str)>,
) -> Result<ReportTemplateDto, String> {
    validate_report_type(&request.report_type)?;
    validate_template(&request.report_type, &request.content)?;
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let existing: Option<(String, i64)> = connection
        .query_row(
            "SELECT id, version FROM report_templates
             WHERE workspace_id = ?1 AND report_type = ?2 AND is_default = 1 AND deleted_at IS NULL",
            params![workspace_id, request.report_type],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let base_version = existing.as_ref().map(|(_, version)| *version);
    let now = now_millis();
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let id = if let Some((id, version)) = existing {
        if request
            .expected_version
            .is_some_and(|expected| expected != version)
        {
            return Err("VERSION_CONFLICT: 报告模板已变化".to_string());
        }
        transaction
            .execute(
                "UPDATE report_templates SET content = ?1, updated_at = ?2, version = version + 1 WHERE id = ?3",
                params![request.content, now, id],
            )
            .map_err(|error| error.to_string())?;
        id
    } else {
        let id = Uuid::now_v7().to_string();
        transaction
            .execute(
                "INSERT INTO report_templates(id, workspace_id, report_type, subject_id, name, content, is_default, is_builtin, sort_order, created_at, updated_at, version)
                 VALUES (?1, ?2, ?3, NULL, ?4, ?5, 1, 0, 10, ?6, ?6, 1)",
                params![id, workspace_id, request.report_type, format!("标准{}", report_type_label(&request.report_type)), request.content, now],
            )
            .map_err(|error| error.to_string())?;
        id
    };
    bump_revision(&transaction)?;
    let result = load_template_by_id(&transaction, &id, false)?;
    if let Some((state, operation_type)) = cloud_operation {
        crate::cloud_sync::enqueue_entity_in_transaction(
            &transaction,
            state,
            operation_type,
            "report_template",
            Some(&id),
            request.expected_version.or(base_version),
            None,
        )?;
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

fn load_report(connection: &rusqlite::Connection, report_id: &str) -> Result<ReportDto, String> {
    let mut report: ReportDto = connection
        .query_row(
            "SELECT r.id, r.report_type, r.subject_id, s.name, r.period_start, r.period_end,
                    r.reference_date, r.markdown_content, r.content_source, r.generation_count,
                    r.updated_at, r.version, r.template_id, template.name,
                    CASE WHEN template.deleted_at IS NULL THEN 0 ELSE 1 END
             FROM reports r JOIN subjects s ON s.id = r.subject_id
             LEFT JOIN report_templates template ON template.id = r.template_id
             WHERE r.id = ?1 AND r.deleted_at IS NULL",
            [report_id],
            |row| {
                let report_type: String = row.get(1)?;
                let start: String = row.get(4)?;
                let end: String = row.get(5)?;
                Ok(ReportDto {
                    id: row.get(0)?,
                    report_type,
                    subject_id: row.get(2)?,
                    subject_name: row.get(3)?,
                    period: period_label(&start, &end),
                    period_start: start,
                    period_end: end,
                    reference_date: row.get(6)?,
                    task_ids: Vec::new(),
                    template_id: row.get(12)?,
                    template_name: row.get(13)?,
                    template_archived: row.get::<_, i64>(14)? == 1,
                    markdown: row.get(7)?,
                    content_source: row.get(8)?,
                    generated_count: row.get(9)?,
                    updated_at: row.get(10)?,
                    version: row.get(11)?,
                })
            },
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "NOT_FOUND: 报告不存在".to_string())?;
    let mut statement = connection
        .prepare("SELECT task_id FROM report_tasks WHERE report_id = ?1 ORDER BY sort_order")
        .map_err(|error| error.to_string())?;
    report.task_ids = statement
        .query_map([report_id], |row| row.get(0))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(report)
}

fn replace_report_tasks(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    report_id: &str,
    subject_id: &str,
    task_ids: &[String],
) -> Result<(), String> {
    let tasks = load_tasks(transaction, workspace_id, subject_id, None)?;
    let task_map: HashMap<_, _> = tasks.iter().map(|task| (task.id.clone(), task)).collect();
    let mut unique = HashSet::new();
    for (index, task_id) in task_ids
        .iter()
        .filter(|id| unique.insert((*id).clone()))
        .enumerate()
    {
        let task = task_map
            .get(task_id)
            .ok_or_else(|| format!("NOT_FOUND: 报告事项不存在 {task_id}"))?;
        let path = task_path(task, &task_map);
        transaction
            .execute(
                "INSERT INTO report_tasks(report_id, task_id, workspace_id, sort_order, title_snapshot, path_snapshot)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![report_id, task_id, workspace_id, (index as i64 + 1) * 10, task.title, path],
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn render_report(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    report_type: &str,
    template_id: Option<&str>,
    subject_id: &str,
    period_start: &str,
    period_end: &str,
    task_ids: &[String],
) -> Result<(String, String), String> {
    validate_report_type(report_type)?;
    let template = if let Some(template_id) = template_id {
        load_bound_template(connection, workspace_id, template_id, report_type)?
    } else {
        load_default_template(connection, workspace_id, report_type)?
    };
    let user_name = setting_string(connection, workspace_id, "user_name", "晓健")?;
    let fallback_work_period =
        setting_string(connection, workspace_id, "default_work_period_text", "")?;
    let salary_hourly_rate = setting_number(connection, workspace_id, "salary_hourly_rate", 0.0)?;
    let duration_format = setting_string(
        connection,
        workspace_id,
        "report_duration_format",
        "minutes",
    )?;
    let duration_format = normalize_duration_format(&duration_format);
    let tasks = load_tasks(
        connection,
        workspace_id,
        subject_id,
        (report_type == "daily").then_some(period_start),
    )?;
    let actuals = task_actual_minutes(connection, workspace_id, period_start, period_end)?;
    let selected: HashSet<String> = task_ids.iter().cloned().collect();
    let total_minutes: i64 = actuals
        .iter()
        .filter(|(task_id, _)| selected.contains(*task_id))
        .map(|(_, minutes)| *minutes)
        .sum();
    let (today_items, tomorrow_plan) = if report_type == "daily" {
        let tomorrow = (NaiveDate::parse_from_str(period_start, "%Y-%m-%d")
            .map_err(|error| error.to_string())?
            + Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
        let tomorrow_tasks = load_tasks(connection, workspace_id, subject_id, Some(&tomorrow))?;
        (
            render_daily_items(&tasks, &selected, &actuals, duration_format),
            render_open_plan(&tomorrow_tasks, &tomorrow),
        )
    } else {
        (String::new(), String::new())
    };
    let daily_situation = if report_type == "daily" {
        String::new()
    } else {
        render_daily_situation(
            connection,
            workspace_id,
            subject_id,
            period_start,
            period_end,
            duration_format,
        )?
    };
    let salary_amount = total_minutes as f64 / 60.0 * salary_hourly_rate;
    let reimbursement_summary = if report_type == "daily" {
        String::new()
    } else {
        latest_reimbursement_summary(
            connection,
            workspace_id,
            subject_id,
            period_start,
            period_end,
        )?
    };
    let statistics = format!(
        "- 累计用时：{}\n- 选中事项：{} 项\n- 已完成事项：{} 项\n- 工资估算：¥{salary_amount:.2}{}",
        format_report_duration(total_minutes, duration_format),
        task_ids.len(),
        completed_count(
            connection,
            workspace_id,
            &tasks,
            &selected,
            period_start,
            period_end,
        )?,
        reimbursement_summary
    );
    let date =
        NaiveDate::parse_from_str(period_start, "%Y-%m-%d").map_err(|error| error.to_string())?;
    let weekday = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"]
        [date.weekday().num_days_from_monday() as usize];
    let work_period = if report_type == "daily" {
        daily_work_period_text(
            connection,
            workspace_id,
            subject_id,
            period_start,
            date.weekday().num_days_from_monday() as i64,
        )?
    } else {
        fallback_work_period
    };
    let values = [
        ("{{姓名}}", user_name),
        ("{{日期}}", period_start.to_string()),
        ("{{星期}}", weekday.to_string()),
        ("{{日期范围}}", period_label(period_start, period_end)),
        ("{{工作时段}}", work_period),
        (
            "{{总工时}}",
            format_report_duration(total_minutes, duration_format),
        ),
        ("{{今日事项}}", today_items),
        ("{{明日计划}}", tomorrow_plan),
        ("{{每日情况}}", daily_situation),
        ("{{统计信息}}", statistics),
        ("{{总结}}", "- 在这里补充本期总结。".to_string()),
    ];
    let mut markdown = template.content.clone();
    for (token, value) in values {
        markdown = markdown.replace(token, &value);
    }
    Ok((template.id, format!("{}\n", markdown.trim_end())))
}

fn daily_work_period_text(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    subject_id: &str,
    work_date: &str,
    weekday: i64,
) -> Result<String, String> {
    let configured = crate::subject_work_schedules::work_period_text(
        connection,
        workspace_id,
        subject_id,
        weekday,
    )?;
    if let Some(value) = configured.as_deref().filter(|value| *value != "休息") {
        return Ok(value.to_string());
    }

    let (started_at, ended_at): (Option<i64>, Option<i64>) = connection
        .query_row(
            "SELECT MIN(e.started_at), MAX(e.ended_at)
             FROM time_entries e
             WHERE e.workspace_id = ?1
               AND e.work_date = ?2
               AND e.kind = 'work'
               AND e.state = 'ended'
               AND e.ended_at IS NOT NULL
               AND e.deleted_at IS NULL
               AND EXISTS (
                 SELECT 1
                 FROM time_allocations a
                 JOIN tasks t ON t.id = a.task_id
                 WHERE a.workspace_id = e.workspace_id
                   AND a.entry_id = e.id
                   AND a.minutes > 0
                   AND t.workspace_id = e.workspace_id
                   AND t.subject_id = ?3
                   AND t.deleted_at IS NULL
               )",
            params![workspace_id, work_date, subject_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| error.to_string())?;
    let (Some(started_at), Some(ended_at)) = (started_at, ended_at) else {
        return Ok("休息".to_string());
    };
    let timezone = crate::work_calendar::workspace_timezone(connection, workspace_id)?;
    let started = timezone
        .timestamp_millis_opt(started_at)
        .single()
        .ok_or_else(|| "VALIDATION_ERROR: 工作开始时间无效".to_string())?;
    let ended = timezone
        .timestamp_millis_opt(ended_at)
        .single()
        .ok_or_else(|| "VALIDATION_ERROR: 工作结束时间无效".to_string())?;
    Ok(format!(
        "{}-{}",
        started.format("%H:%M"),
        ended.format("%H:%M")
    ))
}

fn latest_reimbursement_summary(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    subject_id: &str,
    period_start: &str,
    period_end: &str,
) -> Result<String, String> {
    let mut statement = connection
        .prepare(
            "SELECT preview_json FROM sync_runs
             WHERE workspace_id = ?1 AND provider = 'seatable'
               AND operation_type = 'reimbursements_query' AND state = 'succeeded'
             ORDER BY completed_at DESC, created_at DESC",
        )
        .map_err(|error| error.to_string())?;
    let snapshots = statement
        .query_map([workspace_id], |row| row.get::<_, Option<String>>(0))
        .map_err(|error| error.to_string())?;
    for snapshot in snapshots {
        let Some(snapshot) = snapshot.map_err(|error| error.to_string())? else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&snapshot) else {
            continue;
        };
        if value.get("subjectId").and_then(serde_json::Value::as_str) != Some(subject_id)
            || value.get("periodStart").and_then(serde_json::Value::as_str) != Some(period_start)
            || value.get("periodEnd").and_then(serde_json::Value::as_str) != Some(period_end)
        {
            continue;
        }
        let total = value
            .get("total")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or_default();
        let count = value
            .get("items")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len)
            .unwrap_or_default();
        return Ok(format!("\n- 待报销：¥{total:.2}（{count} 项）"));
    }
    Ok("\n- 待报销：尚未查询（不影响报告生成）".to_string())
}

fn render_daily_items(
    tasks: &[TaskRow],
    selected: &HashSet<String>,
    actuals: &HashMap<String, i64>,
    duration_format: &str,
) -> String {
    let task_map: HashMap<_, _> = tasks.iter().map(|task| (task.id.clone(), task)).collect();
    let children = child_map(tasks);
    let mut lines = Vec::new();
    let roots = ordered_children(&children, None);
    for root_id in roots {
        if subtree_included(&root_id, selected, actuals, &children, &task_map) {
            let index = lines.len() + 1;
            render_daily_task(
                &root_id,
                0,
                Some(index),
                selected,
                actuals,
                &children,
                &task_map,
                duration_format,
                &mut lines,
            );
        }
    }
    if lines.is_empty() {
        "暂无今日工作记录".to_string()
    } else {
        lines.join("\n")
    }
}

#[allow(clippy::too_many_arguments)]
fn render_daily_task(
    task_id: &str,
    depth: usize,
    root_index: Option<usize>,
    selected: &HashSet<String>,
    actuals: &HashMap<String, i64>,
    children: &HashMap<Option<String>, Vec<String>>,
    tasks: &HashMap<String, &TaskRow>,
    duration_format: &str,
    lines: &mut Vec<String>,
) {
    let Some(task) = tasks.get(task_id) else {
        return;
    };
    let child_ids = ordered_children(children, Some(task_id));
    let included_children: Vec<_> = child_ids
        .into_iter()
        .filter(|id| subtree_included(id, selected, actuals, children, tasks))
        .collect();
    let prefix = if depth == 0 {
        format!("{}. ", root_index.unwrap_or(1))
    } else {
        format!("{}- ", "   ".repeat(depth))
    };
    if !included_children.is_empty() {
        lines.push(format!("{prefix}{}", task.title));
        for child_id in included_children {
            render_daily_task(
                &child_id,
                depth + 1,
                None,
                selected,
                actuals,
                children,
                tasks,
                duration_format,
                lines,
            );
        }
        return;
    }
    if !selected.contains(task_id) {
        return;
    }
    let actual = actuals.get(task_id).copied().unwrap_or(0);
    if actual == 0 && task_effective_status(task) != "done" {
        return;
    }
    let icon = if task_effective_status(task) == "done" {
        "🟢"
    } else {
        "🔴"
    };
    let mut metrics = Vec::new();
    if let Some(estimate) = task.daily_estimate_minutes.or(task.estimate_minutes) {
        metrics.push(format!(
            "预计：{}",
            format_report_duration(estimate, duration_format)
        ));
    }
    if actual > 0 {
        metrics.push(format!(
            "实际：{}",
            format_report_duration(actual, duration_format)
        ));
    }
    let metric = if metrics.is_empty() {
        String::new()
    } else {
        format!(" <{}>", metrics.join(" "))
    };
    lines.push(format!("{prefix}{icon} {}{metric}", task.title));
}

fn render_open_plan(tasks: &[TaskRow], plan_date: &str) -> String {
    let children = child_map(tasks);
    let task_map: HashMap<_, _> = tasks.iter().map(|task| (task.id.clone(), task)).collect();
    let mut lines = Vec::new();
    for root_id in ordered_children(&children, None) {
        if !subtree_open(&root_id, plan_date, &children, &task_map) {
            continue;
        }
        let index = lines.len() + 1;
        render_open_task(
            &root_id,
            0,
            Some(index),
            plan_date,
            &children,
            &task_map,
            &mut lines,
        );
    }
    if lines.is_empty() {
        "暂无明日计划".to_string()
    } else {
        lines.join("\n")
    }
}

fn render_open_task(
    task_id: &str,
    depth: usize,
    root_index: Option<usize>,
    plan_date: &str,
    children: &HashMap<Option<String>, Vec<String>>,
    tasks: &HashMap<String, &TaskRow>,
    lines: &mut Vec<String>,
) {
    let Some(task) = tasks.get(task_id) else {
        return;
    };
    if !subtree_open(task_id, plan_date, children, tasks) {
        return;
    }
    let prefix = if depth == 0 {
        format!("{}. ", root_index.unwrap_or(1))
    } else {
        format!("{}- ", "   ".repeat(depth))
    };
    lines.push(format!("{prefix}{}", task.title));
    for child_id in ordered_children(children, Some(task_id)) {
        render_open_task(
            &child_id,
            depth + 1,
            None,
            plan_date,
            children,
            tasks,
            lines,
        );
    }
}

fn render_daily_situation(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    subject_id: &str,
    period_start: &str,
    period_end: &str,
    duration_format: &str,
) -> Result<String, String> {
    let start =
        NaiveDate::parse_from_str(period_start, "%Y-%m-%d").map_err(|error| error.to_string())?;
    let end =
        NaiveDate::parse_from_str(period_end, "%Y-%m-%d").map_err(|error| error.to_string())?;
    let mut lines = Vec::new();
    let mut date = start;
    while date <= end {
        let day = date.format("%Y-%m-%d").to_string();
        let minutes: i64 = connection
            .query_row(
                "SELECT COALESCE(SUM(a.minutes), 0)
                 FROM time_allocations a JOIN time_entries e ON e.id = a.entry_id
                 JOIN tasks t ON t.id = a.task_id
                 WHERE a.workspace_id = ?1 AND t.subject_id = ?2 AND e.work_date = ?3 AND e.deleted_at IS NULL",
                params![workspace_id, subject_id, day],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let report_exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM reports WHERE workspace_id = ?1 AND subject_id = ?2
                 AND report_type = 'daily' AND period_start = ?3 AND deleted_at IS NULL)",
                params![workspace_id, subject_id, day],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|error| error.to_string())?
            == 1;
        if minutes == 0 && !report_exists {
            lines.push(format!("- {day}：缺少日报或结算数据"));
        } else {
            let mut statement = connection
                .prepare(
                    "SELECT DISTINCT t.title
                     FROM time_allocations a JOIN time_entries e ON e.id = a.entry_id
                     JOIN tasks t ON t.id = a.task_id
                     WHERE a.workspace_id = ?1 AND t.subject_id = ?2 AND e.work_date = ?3
                       AND e.deleted_at IS NULL AND a.minutes > 0
                     ORDER BY t.sort_order, t.created_at LIMIT 5",
                )
                .map_err(|error| error.to_string())?;
            let titles = statement
                .query_map(params![workspace_id, subject_id, day], |row| {
                    row.get::<_, String>(0)
                })
                .map_err(|error| error.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?;
            let summary = if titles.is_empty() {
                "暂无已分配事项".to_string()
            } else {
                titles.join("、")
            };
            lines.push(format!(
                "- {day}：{} · {summary}",
                format_report_duration(minutes, duration_format)
            ));
        }
        date += Duration::days(1);
    }
    Ok(lines.join("\n"))
}

fn load_tasks(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    subject_id: &str,
    daily_estimate_date: Option<&str>,
) -> Result<Vec<TaskRow>, String> {
    let mut statement = connection
        .prepare(
            "SELECT t.id, t.parent_id, t.title, t.status, t.estimate_minutes,
                    de.estimate_minutes, t.sort_order
             FROM tasks t
             LEFT JOIN task_daily_estimates de
               ON de.task_id = t.id AND de.workspace_id = t.workspace_id AND de.work_date = ?3
             WHERE t.workspace_id = ?1 AND t.subject_id = ?2 AND t.deleted_at IS NULL
             ORDER BY t.sort_order, t.created_at",
        )
        .map_err(|error| error.to_string())?;
    let mut tasks = statement
        .query_map(
            params![workspace_id, subject_id, daily_estimate_date],
            |row| {
                Ok(TaskRow {
                    id: row.get(0)?,
                    parent_id: row.get(1)?,
                    title: row.get(2)?,
                    status: row.get(3)?,
                    estimate_minutes: row.get(4)?,
                    daily_estimate_minutes: row.get(5)?,
                    sort_order: row.get(6)?,
                    recurrence: None,
                    occurrence_status: None,
                    occurrence_origin: None,
                })
            },
        )
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    if let Some(date) = daily_estimate_date {
        for task in &mut tasks {
            task.recurrence =
                crate::recurring::get_rule_for_date(connection, workspace_id, &task.id, date)?.or(
                    crate::recurring::get_active_rule(connection, workspace_id, &task.id)?,
                );
            if let Some(occurrence) =
                crate::recurring::occurrence_for_date(connection, workspace_id, &task.id, date)?
            {
                task.occurrence_status = Some(occurrence.status);
                task.occurrence_origin = Some(occurrence.origin);
            }
        }
    }
    if daily_estimate_date.is_none() {
        for task in &mut tasks {
            task.recurrence =
                crate::recurring::get_latest_rule(connection, workspace_id, &task.id)?;
        }
    }
    Ok(tasks)
}

fn task_actual_minutes(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    period_start: &str,
    period_end: &str,
) -> Result<HashMap<String, i64>, String> {
    let mut statement = connection
        .prepare(
            "SELECT a.task_id, COALESCE(SUM(a.minutes), 0)
             FROM time_allocations a JOIN time_entries e ON e.id = a.entry_id
             WHERE a.workspace_id = ?1 AND e.work_date BETWEEN ?2 AND ?3 AND e.deleted_at IS NULL
             GROUP BY a.task_id",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![workspace_id, period_start, period_end], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(rows.into_iter().collect())
}

fn child_map(tasks: &[TaskRow]) -> HashMap<Option<String>, Vec<String>> {
    let mut map = HashMap::<Option<String>, Vec<(i64, String)>>::new();
    for task in tasks {
        map.entry(task.parent_id.clone())
            .or_default()
            .push((task.sort_order, task.id.clone()));
    }
    map.into_iter()
        .map(|(parent, mut items)| {
            items.sort_by_key(|(order, _)| *order);
            (parent, items.into_iter().map(|(_, id)| id).collect())
        })
        .collect()
}

fn ordered_children(
    children: &HashMap<Option<String>, Vec<String>>,
    parent_id: Option<&str>,
) -> Vec<String> {
    children
        .get(&parent_id.map(str::to_string))
        .cloned()
        .unwrap_or_default()
}

fn subtree_included(
    task_id: &str,
    selected: &HashSet<String>,
    actuals: &HashMap<String, i64>,
    children: &HashMap<Option<String>, Vec<String>>,
    tasks: &HashMap<String, &TaskRow>,
) -> bool {
    if let Some(task) = tasks.get(task_id) {
        if selected.contains(task_id)
            && (task_effective_status(task) == "done"
                || actuals.get(task_id).copied().unwrap_or(0) > 0)
        {
            return true;
        }
    }
    ordered_children(children, Some(task_id))
        .iter()
        .any(|child| subtree_included(child, selected, actuals, children, tasks))
}

fn subtree_open(
    task_id: &str,
    plan_date: &str,
    children: &HashMap<Option<String>, Vec<String>>,
    tasks: &HashMap<String, &TaskRow>,
) -> bool {
    let child_ids = ordered_children(children, Some(task_id));
    if !child_ids.is_empty() {
        return child_ids
            .iter()
            .any(|child| subtree_open(child, plan_date, children, tasks));
    }
    tasks
        .get(task_id)
        .is_some_and(|task| task_open_on_date(task, plan_date))
}

fn task_effective_status(task: &TaskRow) -> &str {
    task.occurrence_status.as_deref().unwrap_or(&task.status)
}

fn task_open_on_date(task: &TaskRow, date: &str) -> bool {
    if let Some(rule) = task.recurrence.as_ref() {
        return crate::recurring::rule_matches_date(rule, date).unwrap_or(false)
            && task_effective_status(task) == "open";
    }
    task.status == "open"
}

fn completed_count(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    tasks: &[TaskRow],
    selected: &HashSet<String>,
    period_start: &str,
    period_end: &str,
) -> Result<i64, String> {
    let recurring_task_ids = tasks
        .iter()
        .filter(|task| task.recurrence.is_some())
        .map(|task| task.id.as_str())
        .collect::<HashSet<_>>();
    let ordinary = tasks
        .iter()
        .filter(|task| {
            selected.contains(&task.id)
                && !recurring_task_ids.contains(task.id.as_str())
                && task.status == "done"
        })
        .count() as i64;
    let mut recurring = 0;
    for task_id in selected {
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM task_occurrences
                 WHERE workspace_id = ?1 AND task_id = ?2 AND status = 'done'
                   AND occurrence_date BETWEEN ?3 AND ?4",
                params![workspace_id, task_id, period_start, period_end],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        recurring += count;
    }
    Ok(ordinary + recurring)
}

fn task_path(task: &TaskRow, tasks: &HashMap<String, &TaskRow>) -> String {
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

fn normalize_period(report_type: &str, reference_date: &str) -> Result<(String, String), String> {
    validate_report_type(report_type)?;
    let date = NaiveDate::parse_from_str(reference_date, "%Y-%m-%d")
        .map_err(|_| "VALIDATION_ERROR: 非法报告日期".to_string())?;
    let (start, end) = match report_type {
        "daily" => (date, date),
        "weekly" => {
            let start = date - Duration::days(date.weekday().num_days_from_monday() as i64);
            (start, start + Duration::days(6))
        }
        "monthly" => {
            let start = NaiveDate::from_ymd_opt(date.year(), date.month(), 1).expect("valid month");
            let (year, month) = if date.month() == 12 {
                (date.year() + 1, 1)
            } else {
                (date.year(), date.month() + 1)
            };
            let next = NaiveDate::from_ymd_opt(year, month, 1).expect("valid next month");
            (start, next - Duration::days(1))
        }
        _ => unreachable!(),
    };
    Ok((
        start.format("%Y-%m-%d").to_string(),
        end.format("%Y-%m-%d").to_string(),
    ))
}

fn period_label(start: &str, end: &str) -> String {
    if start == end {
        start.to_string()
    } else {
        format!("{start} 至 {end}")
    }
}

fn format_decimal_hours(minutes: i64) -> String {
    format!("{:.1}h", minutes as f64 / 60.0)
}

fn normalize_duration_format(value: &str) -> &str {
    if value == "hours" {
        "hours"
    } else {
        "minutes"
    }
}

fn format_report_duration(minutes: i64, format: &str) -> String {
    if normalize_duration_format(format) == "minutes" {
        format!("{minutes}min")
    } else {
        format_decimal_hours(minutes)
    }
}

fn load_default_template(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    report_type: &str,
) -> Result<ReportTemplateDto, String> {
    let id: String = connection
        .query_row(
            "SELECT id FROM report_templates WHERE workspace_id = ?1 AND report_type = ?2
             AND is_default = 1 AND deleted_at IS NULL",
            params![workspace_id, report_type],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    load_template_by_id(connection, &id, false)
}

fn load_template_by_id(
    connection: &rusqlite::Connection,
    id: &str,
    include_deleted: bool,
) -> Result<ReportTemplateDto, String> {
    connection
        .query_row(
            "SELECT id, report_type, name, description, content, is_default, is_builtin, sort_order, deleted_at, version
             FROM report_templates WHERE id = ?1 AND (?2 = 1 OR deleted_at IS NULL)",
            params![id, include_deleted as i64],
            |row| Ok(ReportTemplateDto {
                id: row.get(0)?, report_type: row.get(1)?, name: row.get(2)?, description: row.get(3)?,
                content: row.get(4)?, is_default: row.get::<_, i64>(5)? == 1,
                is_builtin: row.get::<_, i64>(6)? == 1, sort_order: row.get(7)?,
                deleted_at: row.get(8)?, version: row.get(9)?,
            }),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "NOT_FOUND: 报告模板不存在".to_string())
}

fn load_template_in_workspace(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    id: &str,
    include_deleted: bool,
) -> Result<ReportTemplateDto, String> {
    let belongs: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM report_templates WHERE id=?1 AND workspace_id=?2 AND (?3=1 OR deleted_at IS NULL))",
        params![id, workspace_id, include_deleted as i64], |row| row.get::<_, i64>(0),
    ).map_err(|error| error.to_string())? == 1;
    if !belongs {
        return Err("NOT_FOUND: 报告模板不存在".to_string());
    }
    load_template_by_id(connection, id, include_deleted)
}

fn load_selectable_template(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    id: &str,
    report_type: &str,
) -> Result<ReportTemplateDto, String> {
    let template = load_template_in_workspace(connection, workspace_id, id, false)?;
    if template.report_type != report_type {
        return Err("VALIDATION_ERROR: 报告模板类型与报告类型不一致".to_string());
    }
    Ok(template)
}

fn load_bound_template(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    id: &str,
    report_type: &str,
) -> Result<ReportTemplateDto, String> {
    let template = load_template_in_workspace(connection, workspace_id, id, true)?;
    if template.report_type != report_type {
        return Err("VALIDATION_ERROR: 报告绑定的模板类型不匹配".to_string());
    }
    Ok(template)
}

fn validate_template_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("VALIDATION_ERROR: 模板名称不能为空".to_string());
    }
    if name.chars().count() > 80 {
        return Err("VALIDATION_ERROR: 模板名称不能超过 80 个字符".to_string());
    }
    Ok(name.to_string())
}

fn normalize_optional_text(value: String) -> Option<String> {
    let value = value.trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn ensure_template_name_available(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    report_type: &str,
    name: &str,
    exclude_id: Option<&str>,
) -> Result<(), String> {
    let duplicate: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM report_templates WHERE workspace_id=?1 AND report_type=?2
         AND lower(trim(name))=lower(trim(?3)) AND deleted_at IS NULL AND (?4 IS NULL OR id<>?4))",
            params![workspace_id, report_type, name, exclude_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| error.to_string())?
        == 1;
    if duplicate {
        Err("VALIDATION_ERROR: 同类型模板名称已存在".to_string())
    } else {
        Ok(())
    }
}

fn map_template_write_error(error: rusqlite::Error) -> String {
    let message = error.to_string();
    if message.contains("uq_report_templates_active_name")
        || message.contains("report_templates.workspace_id")
    {
        "VALIDATION_ERROR: 同类型模板名称已存在".to_string()
    } else {
        message
    }
}

fn enqueue_template_change(
    transaction: &Transaction<'_>,
    cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str)>,
    fallback_operation: &str,
    id: &str,
    base_version: Option<i64>,
) -> Result<(), String> {
    if let Some((state, operation_type)) = cloud_operation {
        crate::cloud_sync::enqueue_entity_in_transaction(
            transaction,
            state,
            operation_type,
            "report_template",
            Some(id),
            base_version,
            None,
        )
        .map(|_| ())
    } else {
        let _ = fallback_operation;
        Ok(())
    }
}

fn report_type_label(report_type: &str) -> &'static str {
    match report_type {
        "daily" => "日报",
        "weekly" => "周报",
        _ => "月报",
    }
}

const TEMPLATE_PLACEHOLDERS: &[ReportTemplatePlaceholderDto] = &[
    ReportTemplatePlaceholderDto {
        token: "{{姓名}}",
        label: "姓名",
        description: "设置中的用户姓名",
        example: "晓健",
        report_types: &["daily", "weekly", "monthly"],
    },
    ReportTemplatePlaceholderDto {
        token: "{{日期}}",
        label: "日期",
        description: "报告周期起始日期",
        example: "2026-10-11",
        report_types: &["daily"],
    },
    ReportTemplatePlaceholderDto {
        token: "{{星期}}",
        label: "星期",
        description: "日报日期对应的星期",
        example: "周日",
        report_types: &["daily"],
    },
    ReportTemplatePlaceholderDto {
        token: "{{日期范围}}",
        label: "日期范围",
        description: "周报或月报覆盖范围",
        example: "2026-10-05 至 2026-10-11",
        report_types: &["weekly", "monthly"],
    },
    ReportTemplatePlaceholderDto {
        token: "{{工作时段}}",
        label: "工作时段",
        description: "报告主体当天的工作时段",
        example: "10:00-12:00 13:30-17:30",
        report_types: &["daily"],
    },
    ReportTemplatePlaceholderDto {
        token: "{{总工时}}",
        label: "总工时",
        description: "所选事项累计用时",
        example: "360min",
        report_types: &["daily", "weekly", "monthly"],
    },
    ReportTemplatePlaceholderDto {
        token: "{{今日事项}}",
        label: "今日事项",
        description: "日报所选事项明细",
        example: "1. 完成登录优化",
        report_types: &["daily"],
    },
    ReportTemplatePlaceholderDto {
        token: "{{明日计划}}",
        label: "明日计划",
        description: "下一日待办事项",
        example: "1. 联调同步",
        report_types: &["daily"],
    },
    ReportTemplatePlaceholderDto {
        token: "{{每日情况}}",
        label: "每日情况",
        description: "周期内按日汇总",
        example: "- 2026-10-11：6h",
        report_types: &["weekly", "monthly"],
    },
    ReportTemplatePlaceholderDto {
        token: "{{统计信息}}",
        label: "统计信息",
        description: "工时与事项统计",
        example: "- 累计用时：6h",
        report_types: &["weekly", "monthly"],
    },
    ReportTemplatePlaceholderDto {
        token: "{{总结}}",
        label: "总结",
        description: "报告总结占位内容",
        example: "- 本期总结",
        report_types: &["weekly", "monthly"],
    },
];

pub fn template_placeholders(
    report_type: &str,
) -> Result<Vec<ReportTemplatePlaceholderDto>, String> {
    validate_report_type(report_type)?;
    Ok(TEMPLATE_PLACEHOLDERS
        .iter()
        .filter(|item| item.report_types.contains(&report_type))
        .cloned()
        .collect())
}

fn validate_template(report_type: &str, content: &str) -> Result<(), String> {
    if content.trim().is_empty() {
        return Err("VALIDATION_ERROR: 报告模板不能为空".to_string());
    }
    let allowed: HashSet<_> = TEMPLATE_PLACEHOLDERS
        .iter()
        .filter(|item| item.report_types.contains(&report_type))
        .map(|item| item.token)
        .collect();
    let mut rest = content;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start..];
        let Some(end) = after.find("}}") else {
            return Err("VALIDATION_ERROR: 模板占位符未闭合".to_string());
        };
        let token = &after[..end + 2];
        if !TEMPLATE_PLACEHOLDERS.iter().any(|item| item.token == token) {
            return Err(format!("VALIDATION_ERROR: 未知占位符 {token}"));
        }
        if !allowed.contains(token) {
            return Err(format!(
                "VALIDATION_ERROR: 占位符 {token} 不适用于{}",
                report_type_label(report_type)
            ));
        }
        rest = &after[end + 2..];
    }
    Ok(())
}

fn setting_string(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    key: &str,
    fallback: &str,
) -> Result<String, String> {
    let value: Option<String> = connection
        .query_row(
            "SELECT value_json FROM app_settings WHERE workspace_id = ?1 AND key = ?2",
            params![workspace_id, key],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    Ok(value
        .and_then(|json| serde_json::from_str::<String>(&json).ok())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback.to_string()))
}

fn setting_number(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    key: &str,
    fallback: f64,
) -> Result<f64, String> {
    let value: Option<String> = connection
        .query_row(
            "SELECT value_json FROM app_settings WHERE workspace_id = ?1 AND key = ?2",
            params![workspace_id, key],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    Ok(value
        .and_then(|json| serde_json::from_str::<f64>(&json).ok())
        .unwrap_or(fallback))
}

fn validate_report_type(value: &str) -> Result<(), String> {
    if matches!(value, "daily" | "weekly" | "monthly") {
        Ok(())
    } else {
        Err("VALIDATION_ERROR: 非法报告类型".to_string())
    }
}

fn ensure_subject(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    subject_id: &str,
) -> Result<(), String> {
    let exists: bool = connection
        .query_row("SELECT EXISTS(SELECT 1 FROM subjects WHERE id = ?1 AND workspace_id = ?2 AND deleted_at IS NULL)", params![subject_id, workspace_id], |row| row.get::<_, i64>(0))
        .map_err(|error| error.to_string())? == 1;
    if exists {
        Ok(())
    } else {
        Err("NOT_FOUND: 主体不存在".to_string())
    }
}

fn processed_result<T: for<'de> Deserialize<'de>>(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    operation_id: &str,
) -> Result<Option<T>, String> {
    let json: Option<String> = connection
        .query_row("SELECT result_json FROM processed_operations WHERE workspace_id = ?1 AND operation_id = ?2", params![workspace_id, operation_id], |row| row.get(0))
        .optional().map_err(|error| error.to_string())?;
    json.map(|value| serde_json::from_str(&value).map_err(|error| error.to_string()))
        .transpose()
}

fn save_processed_result<T: Serialize>(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    device_id: &str,
    operation_id: &str,
    operation_type: &str,
    result: &T,
) -> Result<(), String> {
    transaction.execute(
        "INSERT INTO processed_operations(workspace_id, operation_id, device_id, operation_type, result_json, processed_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![workspace_id, operation_id, device_id, operation_type, serde_json::to_string(result).map_err(|error| error.to_string())?, now_millis()],
    ).map(|_| ()).map_err(|error| error.to_string())
}

fn validate_operation_id(value: &str) -> Result<(), String> {
    Uuid::parse_str(value)
        .map(|_| ())
        .map_err(|_| "VALIDATION_ERROR: operationId 必须是 UUID".to_string())
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

fn bump_revision(connection: &rusqlite::Connection) -> Result<(), String> {
    connection.execute("UPDATE app_metadata SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT) WHERE key = 'global_revision'", []).map(|_| ()).map_err(|error| error.to_string())
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

fn device_id(connection: &rusqlite::Connection, workspace_id: &str) -> Result<String, String> {
    connection.query_row("SELECT id FROM devices WHERE workspace_id = ?1 AND revoked_at IS NULL ORDER BY created_at LIMIT 1", [workspace_id], |row| row.get(0)).map_err(|error| error.to_string())
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[tauri::command]
pub fn report_list(
    database: tauri::State<'_, Database>,
    request: ReportListRequest,
) -> Result<ReportListResult, String> {
    list_reports(&database, request)
}
#[tauri::command]
pub fn report_task_suggestions(
    database: tauri::State<'_, Database>,
    request: ReportTaskSuggestionRequest,
) -> Result<Vec<ReportTaskSuggestionDto>, String> {
    suggest_report_tasks(&database, request)
}
#[tauri::command]
pub fn report_work_period_preview(
    database: tauri::State<'_, Database>,
    request: ReportWorkPeriodPreviewRequest,
) -> Result<String, String> {
    preview_report_work_period(&database, request)
}
#[tauri::command]
pub fn report_create(
    database: tauri::State<'_, Database>,
    request: ReportCreateRequest,
) -> Result<ReportDto, String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let operation_id = request.client_request_id.clone();
    let state = crate::supabase::storage_mode(&database)?;
    let result = create_report_with_cloud_operation(
        &database,
        request,
        Some((&state, "report_create", Some(operation_id.as_str()))),
    )?;
    crate::cloud_sync::flush_if_online(&database)?;
    Ok(result)
}
#[tauri::command]
pub fn report_update_scope(
    database: tauri::State<'_, Database>,
    request: ReportScopeUpdateRequest,
) -> Result<ReportDto, String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let state = crate::supabase::storage_mode(&database)?;
    let result = update_report_scope_with_cloud_operation(
        &database,
        request,
        Some((&state, "report_update_scope")),
    )?;
    crate::cloud_sync::flush_if_online(&database)?;
    Ok(result)
}
#[tauri::command]
pub fn report_save_content(
    database: tauri::State<'_, Database>,
    request: ReportContentSaveRequest,
) -> Result<ReportDto, String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let state = crate::supabase::storage_mode(&database)?;
    let result = save_report_content_with_cloud_operation(
        &database,
        request,
        Some((&state, "report_save_content")),
    )?;
    crate::cloud_sync::flush_if_online(&database)?;
    Ok(result)
}
#[tauri::command]
pub fn report_regenerate(
    database: tauri::State<'_, Database>,
    request: ReportVersionRequest,
) -> Result<ReportDto, String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let state = crate::supabase::storage_mode(&database)?;
    let result = regenerate_report_with_cloud_operation(
        &database,
        request,
        Some((&state, "report_regenerate")),
    )?;
    crate::cloud_sync::flush_if_online(&database)?;
    Ok(result)
}
#[tauri::command]
pub fn report_delete(
    database: tauri::State<'_, Database>,
    request: ReportVersionRequest,
) -> Result<(), String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let state = crate::supabase::storage_mode(&database)?;
    delete_report_with_cloud_operation(&database, request, Some((&state, "report_delete")))?;
    crate::cloud_sync::flush_if_online(&database)
}
#[tauri::command]
pub fn report_public_text(
    database: tauri::State<'_, Database>,
    request: ReportVersionRequest,
) -> Result<PublicReportDto, String> {
    public_report(&database, request)
}
#[tauri::command]
pub fn report_template_get(
    database: tauri::State<'_, Database>,
    request: ReportTemplateGetRequest,
) -> Result<ReportTemplateDto, String> {
    get_template(&database, request)
}
#[tauri::command]
pub fn report_template_save(
    database: tauri::State<'_, Database>,
    request: ReportTemplateSaveRequest,
) -> Result<ReportTemplateDto, String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let state = crate::supabase::storage_mode(&database)?;
    let result = save_template_with_cloud_operation(
        &database,
        request,
        Some((&state, "report_template_save")),
    )?;
    crate::cloud_sync::flush_if_online(&database)?;
    Ok(result)
}
#[tauri::command]
pub fn report_template_list(
    database: tauri::State<'_, Database>,
    request: ReportTemplateListRequest,
) -> Result<Vec<ReportTemplateDto>, String> {
    list_templates(&database, request)
}
#[tauri::command]
pub fn report_template_get_by_id(
    database: tauri::State<'_, Database>,
    request: ReportTemplateIdRequest,
) -> Result<ReportTemplateDto, String> {
    get_template_by_id(&database, request)
}
#[tauri::command]
pub fn report_template_create(
    database: tauri::State<'_, Database>,
    request: ReportTemplateCreateRequest,
) -> Result<ReportTemplateDto, String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let state = crate::supabase::storage_mode(&database)?;
    let result = create_template_with_cloud_operation(
        &database,
        request,
        Some((&state, "report_template_create")),
    )?;
    crate::cloud_sync::flush_if_online(&database)?;
    Ok(result)
}
#[tauri::command]
pub fn report_template_update(
    database: tauri::State<'_, Database>,
    request: ReportTemplateUpdateRequest,
) -> Result<ReportTemplateDto, String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let state = crate::supabase::storage_mode(&database)?;
    let result = update_template_with_cloud_operation(
        &database,
        request,
        Some((&state, "report_template_update")),
    )?;
    crate::cloud_sync::flush_if_online(&database)?;
    Ok(result)
}
#[tauri::command]
pub fn report_template_duplicate(
    database: tauri::State<'_, Database>,
    request: ReportTemplateDuplicateRequest,
) -> Result<ReportTemplateDto, String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let state = crate::supabase::storage_mode(&database)?;
    let result = duplicate_template_with_cloud_operation(
        &database,
        request,
        Some((&state, "report_template_duplicate")),
    )?;
    crate::cloud_sync::flush_if_online(&database)?;
    Ok(result)
}
#[tauri::command]
pub fn report_template_set_default(
    database: tauri::State<'_, Database>,
    request: ReportTemplateSetDefaultRequest,
) -> Result<ReportTemplateDto, String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let state = crate::supabase::storage_mode(&database)?;
    let result = set_default_template_with_cloud_operation(
        &database,
        request,
        Some((&state, "report_template_set_default")),
    )?;
    crate::cloud_sync::flush_if_online(&database)?;
    Ok(result)
}
#[tauri::command]
pub fn report_template_delete(
    database: tauri::State<'_, Database>,
    request: ReportTemplateDeleteRequest,
) -> Result<(), String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let state = crate::supabase::storage_mode(&database)?;
    delete_template_with_cloud_operation(
        &database,
        request,
        Some((&state, "report_template_delete")),
    )?;
    crate::cloud_sync::flush_if_online(&database)
}
#[tauri::command]
pub fn report_template_reorder(
    database: tauri::State<'_, Database>,
    request: ReportTemplateReorderRequest,
) -> Result<Vec<ReportTemplateDto>, String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let state = crate::supabase::storage_mode(&database)?;
    let result = reorder_templates_with_cloud_operation(
        &database,
        request,
        Some((&state, "report_template_reorder")),
    )?;
    crate::cloud_sync::flush_if_online(&database)?;
    Ok(result)
}
#[tauri::command]
pub fn report_template_placeholders(
    report_type: String,
) -> Result<Vec<ReportTemplatePlaceholderDto>, String> {
    template_placeholders(&report_type)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recurring::{save_rule, RecurrenceSaveRequest};
    use crate::settings::{self, SettingsScope, SettingsUpdate};
    use crate::tasks::{
        create_task, set_task_daily_estimate, set_task_status, TaskCreateRequest,
        TaskDailyEstimateSetRequest, TaskStatusRequest,
    };
    use crate::time_tracking::{create_manual_entry, ManualEntryRequest};
    use crate::today_overview::{get_today_work_overview, TodayWorkOverviewRequest};
    use chrono::NaiveDateTime;
    use serde_json::{json, Value};
    use tempfile::tempdir;

    fn setup() -> (Database, String, String) {
        let directory = tempdir().unwrap();
        let path = directory.path().join("reports.sqlite3");
        let database = Database::initialize_at(path).unwrap();
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
                title: "生成日报事项".to_string(),
                planned_date: None,
                estimate_minutes: Some(30),
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
                work_date: chrono::Local::now().format("%Y-%m-%d").to_string(),
                estimate_minutes: Some(20),
            },
        )
        .unwrap();
        set_task_status(
            &database,
            TaskStatusRequest {
                id: task.id.clone(),
                expected_version: task.version,
                done: true,
                occurrence_date: None,
                occurrence_expected_version: None,
            },
        )
        .unwrap();
        create_manual_entry(
            &database,
            ManualEntryRequest {
                task_id: Some(task.id.clone()),
                work_date: chrono::Local::now().format("%Y-%m-%d").to_string(),
                started_at: None,
                ended_at: None,
                minutes: Some(30),
                note: None,
                complete_task: false,
                task_expected_version: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        (database, subject_id, task.id)
    }

    fn local_millis(value: &str) -> i64 {
        let timezone: chrono_tz::Tz = "Asia/Shanghai".parse().unwrap();
        timezone
            .from_local_datetime(
                &NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S").unwrap(),
            )
            .single()
            .unwrap()
            .timestamp_millis()
    }

    fn insert_report_work_entry(
        database: &Database,
        workspace_id: &str,
        task_id: Option<&str>,
        entry_id: &str,
        work_date: &str,
        start: &str,
        end: &str,
        kind: &str,
        deleted: bool,
    ) {
        let connection = database.open().unwrap();
        let started_at = local_millis(start);
        let ended_at = local_millis(end);
        connection
            .execute(
                "INSERT INTO time_entries(
                   id,workspace_id,work_date,kind,source_type,state,label_snapshot,started_at,ended_at,
                   duration_seconds,created_at,updated_at,version,deleted_at
                 ) VALUES (?1,?2,?3,?4,'manual','ended','日报时段测试',?5,?6,?7,?5,?6,1,?8)",
                params![
                    entry_id,
                    workspace_id,
                    work_date,
                    kind,
                    started_at,
                    ended_at,
                    (ended_at - started_at) / 1000,
                    deleted.then_some(ended_at)
                ],
            )
            .unwrap();
        if let Some(task_id) = task_id {
            connection
                .execute(
                    "INSERT INTO time_allocations(
                       id,workspace_id,entry_id,task_id,minutes,created_at,updated_at,version
                     ) VALUES (?1,?2,?3,?4,1,?5,?6,1)",
                    params![
                        format!("allocation-{entry_id}"),
                        workspace_id,
                        entry_id,
                        task_id,
                        started_at,
                        ended_at
                    ],
                )
                .unwrap();
        }
    }

    #[test]
    fn creates_edits_and_regenerates_daily_report() {
        let (database, subject_id, task_id) = setup();
        let date = chrono::Local::now().format("%Y-%m-%d").to_string();
        let created = create_report(
            &database,
            ReportCreateRequest {
                report_type: "daily".to_string(),
                reference_date: date.clone(),
                subject_id,
                task_ids: vec![task_id],
                template_id: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        assert!(created.markdown.contains("🟢"));
        assert!(created.markdown.contains("预计：20min 实际：30min"));
        assert!(!created.markdown.contains("预计：30min"));
        let edited = save_report_content(
            &database,
            ReportContentSaveRequest {
                report_id: created.id.clone(),
                markdown: "# 手工修改".to_string(),
                expected_version: created.version,
            },
        )
        .unwrap();
        assert_eq!(edited.content_source, "edited");
        let regenerated = regenerate_report(
            &database,
            ReportVersionRequest {
                report_id: created.id,
                expected_version: edited.version,
            },
        )
        .unwrap();
        assert_eq!(regenerated.generated_count, 2);
        assert_eq!(regenerated.content_source, "generated");
        assert!(!regenerated.markdown.contains("# 手工修改"));
        assert!(regenerated.markdown.contains("预计：20min 实际：30min"));
    }

    #[test]
    fn cloud_report_writes_persist_outbox_in_the_same_transaction() {
        let (database, subject_id, task_id) = setup();
        let cloud_workspace_id = Uuid::now_v7().to_string();
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "cloud_workspace_id".to_string(),
                value: json!(cloud_workspace_id),
            },
        )
        .unwrap();
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "storage_mode".to_string(),
                value: json!("cloud"),
            },
        )
        .unwrap();
        let state = crate::supabase::storage_mode(&database).unwrap();
        let date = chrono::Local::now().format("%Y-%m-%d").to_string();
        let create_operation_id = Uuid::now_v7().to_string();

        let created = create_report_with_cloud_operation(
            &database,
            ReportCreateRequest {
                report_type: "daily".to_string(),
                reference_date: date,
                subject_id: subject_id.clone(),
                task_ids: vec![task_id],
                template_id: None,
                client_request_id: create_operation_id.clone(),
            },
            Some((&state, "report_create", Some(create_operation_id.as_str()))),
        )
        .unwrap();
        let saved = save_report_content_with_cloud_operation(
            &database,
            ReportContentSaveRequest {
                report_id: created.id.clone(),
                markdown: "# 云端事务报告\n\n本地已保存。".to_string(),
                expected_version: created.version,
            },
            Some((&state, "report_save_content")),
        )
        .unwrap();
        let template = save_template_with_cloud_operation(
            &database,
            ReportTemplateSaveRequest {
                report_type: "daily".to_string(),
                subject_id: Some(subject_id),
                content: "# {{日期}}\n{{今日事项}}".to_string(),
                expected_version: None,
            },
            Some((&state, "report_template_save")),
        )
        .unwrap();
        delete_report_with_cloud_operation(
            &database,
            ReportVersionRequest {
                report_id: created.id.clone(),
                expected_version: saved.version,
            },
            Some((&state, "report_delete")),
        )
        .unwrap();

        let connection = database.open().unwrap();
        let outbox = connection
            .prepare(
                "SELECT operation_id, operation_type, entity_type, entity_id, base_version, payload_json FROM sync_outbox ORDER BY created_at, operation_type",
            )
            .unwrap()
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert!(outbox.iter().any(
            |(operation_id, operation, entity_type, entity_id, base_version, payload)| {
                let value: Value = serde_json::from_str(payload).unwrap();
                operation_id == &create_operation_id
                    && operation == "report_create"
                    && entity_type == "report"
                    && entity_id.as_deref() == Some(&created.id)
                    && base_version.is_none()
                    && value["report_tasks"]
                        .as_array()
                        .is_some_and(|tasks| tasks.len() == 1)
            }
        ));
        assert!(outbox.iter().any(
            |(_, operation, entity_type, entity_id, base_version, payload)| {
                let value: Value = serde_json::from_str(payload).unwrap();
                operation == "report_save_content"
                    && entity_type == "report"
                    && entity_id.as_deref() == Some(&created.id)
                    && *base_version == Some(created.version)
                    && value["markdown_content"] == "# 云端事务报告\n\n本地已保存。"
            }
        ));
        assert!(outbox.iter().any(
            |(_, operation, entity_type, entity_id, base_version, payload)| {
                let value: Value = serde_json::from_str(payload).unwrap();
                operation == "report_template_save"
                    && entity_type == "report_template"
                    && entity_id.as_deref() == Some(&template.id)
                    && *base_version == Some(template.version - 1)
                    && value["content"] == "# {{日期}}\n{{今日事项}}"
                    && value["subject_id"].is_null()
                    && value["name"] == "标准日报"
            }
        ));
        assert!(outbox.iter().any(
            |(_, operation, entity_type, entity_id, base_version, payload)| {
                let value: Value = serde_json::from_str(payload).unwrap();
                operation == "report_delete"
                    && entity_type == "report"
                    && entity_id.as_deref() == Some(&created.id)
                    && *base_version == Some(saved.version)
                    && !value["deleted_at"].is_null()
            }
        ));
    }

    #[test]
    fn report_suggestions_use_actual_time_and_daily_plan_keeps_all_open_tasks() {
        let (database, subject_id, timed_task_id) = setup();
        let open_task = create_task(
            &database,
            TaskCreateRequest {
                subject_id: subject_id.clone(),
                parent_id: None,
                title: "尚未执行的后续计划".to_string(),
                planned_date: None,
                estimate_minutes: Some(45),
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        let date = chrono::Local::now().format("%Y-%m-%d").to_string();

        let suggestions = suggest_report_tasks(
            &database,
            ReportTaskSuggestionRequest {
                report_type: "daily".to_string(),
                reference_date: date.clone(),
                subject_id: subject_id.clone(),
            },
        )
        .unwrap();
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].task_id, timed_task_id);
        assert_eq!(suggestions[0].actual_minutes, 30);
        assert_eq!(suggestions[0].daily_estimate_minutes, Some(20));

        let report = create_report(
            &database,
            ReportCreateRequest {
                report_type: "daily".to_string(),
                reference_date: date,
                subject_id,
                task_ids: vec![timed_task_id],
                template_id: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        assert!(report.markdown.contains(&open_task.title));
        assert!(report.markdown.contains("## 明日计划"));
    }

    #[test]
    fn daily_report_keeps_overall_estimate_and_uses_only_reference_date_actual() {
        let (database, subject_id, task_id) = setup();
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let yesterday = (chrono::Local::now() - chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        let connection = database.open().unwrap();
        connection
            .execute(
                "UPDATE tasks SET estimate_minutes = 120 WHERE id = ?1",
                [&task_id],
            )
            .unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        connection
            .execute(
                "DELETE FROM task_daily_estimates WHERE task_id = ?1 AND work_date = ?2",
                params![task_id, today],
            )
            .unwrap();
        let now = chrono::Local::now().timestamp_millis();
        let entry_id = Uuid::now_v7().to_string();
        connection
            .execute(
                "INSERT INTO time_entries(id, workspace_id, work_date, kind, source_type, state, label_snapshot, started_at, ended_at, duration_seconds, created_at, updated_at, version)
                 VALUES (?1, ?2, ?3, 'work', 'manual', 'ended', '昨日投入', ?4, ?4, 3600, ?4, ?4, 1)",
                params![entry_id, workspace_id, yesterday, now],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO time_allocations(id, workspace_id, entry_id, task_id, minutes, created_at, updated_at, version)
                 VALUES (?1, ?2, ?3, ?4, 60, ?5, ?5, 1)",
                params![Uuid::now_v7().to_string(), workspace_id, entry_id, task_id, now],
            )
            .unwrap();
        drop(connection);

        let report = create_report(
            &database,
            ReportCreateRequest {
                report_type: "daily".to_string(),
                reference_date: today,
                subject_id,
                task_ids: vec![task_id],
                template_id: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        assert!(report.markdown.contains("预计：120min 实际：30min"));
        assert!(!report.markdown.contains("实际：90min"));
    }

    #[test]
    fn daily_report_and_today_overview_use_the_same_daily_actual_minutes() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("daily-consistency.sqlite3")).unwrap();
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
                title: "同日口径事项".to_string(),
                planned_date: Some("2026-09-29".to_string()),
                estimate_minutes: Some(60),
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        create_manual_entry(
            &database,
            ManualEntryRequest {
                task_id: Some(task.id.clone()),
                work_date: "2026-09-29".to_string(),
                started_at: None,
                ended_at: None,
                minutes: Some(37),
                note: None,
                complete_task: false,
                task_expected_version: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        let overview = get_today_work_overview(
            &database,
            TodayWorkOverviewRequest {
                subject_id: Some(subject_id.clone()),
                today_date: "2026-09-29".to_string(),
                history_days: Some(1),
                trend_days: Some(1),
            },
        )
        .unwrap();
        let report = create_report(
            &database,
            ReportCreateRequest {
                report_type: "daily".to_string(),
                reference_date: "2026-09-29".to_string(),
                subject_id,
                task_ids: vec![task.id],
                template_id: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        assert_eq!(overview.summary.actual_minutes, 37);
        assert!(report.markdown.contains("实际：37min"));
    }

    #[test]
    fn recurring_daily_report_uses_occurrence_status_and_tomorrow_rule() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("recurring-report.sqlite3")).unwrap();
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
                title: "每日方案预备".to_string(),
                planned_date: None,
                estimate_minutes: Some(20),
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        let today = chrono::Local::now().date_naive();
        let today_text = today.format("%Y-%m-%d").to_string();
        save_rule(
            &database,
            RecurrenceSaveRequest {
                task_id: task.id.clone(),
                task_expected_version: task.version,
                frequency: "daily".to_string(),
                weekdays_mask: None,
                effective_start: today_text.clone(),
                rule_expected_version: None,
            },
        )
        .unwrap();
        set_task_daily_estimate(
            &database,
            TaskDailyEstimateSetRequest {
                task_id: task.id.clone(),
                work_date: today_text.clone(),
                estimate_minutes: Some(25),
            },
        )
        .unwrap();
        create_manual_entry(
            &database,
            ManualEntryRequest {
                task_id: Some(task.id.clone()),
                work_date: today_text.clone(),
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
        let current_version: i64 = database
            .open()
            .unwrap()
            .query_row(
                "SELECT version FROM tasks WHERE id = ?1",
                [&task.id],
                |row| row.get(0),
            )
            .unwrap();
        set_task_status(
            &database,
            TaskStatusRequest {
                id: task.id.clone(),
                expected_version: current_version,
                done: true,
                occurrence_date: Some(today_text.clone()),
                occurrence_expected_version: Some(1),
            },
        )
        .unwrap();

        let report = create_report(
            &database,
            ReportCreateRequest {
                report_type: "daily".to_string(),
                reference_date: today_text,
                subject_id,
                task_ids: vec![task.id.clone()],
                template_id: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        assert!(report
            .markdown
            .contains("🟢 每日方案预备 <预计：25min 实际：15min>"));
        assert!(report.markdown.matches("每日方案预备").count() >= 2);
        let base_status: String = database
            .open()
            .unwrap()
            .query_row(
                "SELECT status FROM tasks WHERE id = ?1",
                [&task.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(base_status, "open");
    }

    #[test]
    fn recurring_workday_item_is_not_in_weekend_tomorrow_plan() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("recurring-weekend.sqlite3")).unwrap();
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
                title: "仅工作日事项".to_string(),
                planned_date: None,
                estimate_minutes: Some(20),
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        save_rule(
            &database,
            RecurrenceSaveRequest {
                task_id: task.id.clone(),
                task_expected_version: task.version,
                frequency: "weekdays".to_string(),
                weekdays_mask: None,
                effective_start: chrono::Local::now().format("%Y-%m-%d").to_string(),
                rule_expected_version: None,
            },
        )
        .unwrap();
        let friday = "2026-10-02";
        let (_, markdown) = render_report(
            &database.open().unwrap(),
            &database
                .open()
                .unwrap()
                .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| {
                    row.get::<_, String>(0)
                })
                .unwrap(),
            "daily",
            None,
            &subject_id,
            friday,
            friday,
            &[],
        )
        .unwrap();
        let tomorrow_section = markdown.split("## 明日计划：").nth(1).unwrap_or_default();
        assert!(!tomorrow_section.contains("仅工作日事项"));
    }

    #[test]
    fn daily_report_uses_subject_work_schedule_for_the_report_weekday() {
        let (database, subject_id, task_id) = setup();
        let mut days = crate::subject_work_schedules::get_schedule(&database, &subject_id).unwrap();
        days[0].segments = vec![crate::subject_work_schedules::WorkScheduleSegmentDto {
            start: "09:15".to_string(),
            end: "11:45".to_string(),
        }];
        crate::subject_work_schedules::save_schedule_for_test(
            &database,
            crate::subject_work_schedules::WorkScheduleSaveRequest {
                subject_id: subject_id.clone(),
                days,
            },
        )
        .unwrap();
        let monday = "2026-10-05";
        let workspace_id: String = database
            .open()
            .unwrap()
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        insert_report_work_entry(
            &database,
            &workspace_id,
            Some(&task_id),
            "configured-day-actual",
            monday,
            "2026-10-05 07:00:00",
            "2026-10-05 20:00:00",
            "work",
            false,
        );
        let (_, markdown) = render_report(
            &database.open().unwrap(),
            &workspace_id,
            "daily",
            None,
            &subject_id,
            monday,
            monday,
            &[],
        )
        .unwrap();
        assert!(markdown.contains("09:15-11:45"));
        assert!(!markdown.contains("07:00-20:00"));

        let sunday = "2026-10-11";
        let (_, markdown) = render_report(
            &database.open().unwrap(),
            &workspace_id,
            "daily",
            None,
            &subject_id,
            sunday,
            sunday,
            &[],
        )
        .unwrap();
        assert!(markdown.contains("休息"));
    }

    #[test]
    fn daily_report_uses_one_actual_range_on_rest_day_and_filters_other_entries() {
        let (database, subject_id, task_id) = setup();
        let workspace_id: String = database
            .open()
            .unwrap()
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let sunday_schedule_id: String = database
            .open()
            .unwrap()
            .query_row(
                "SELECT id FROM subject_work_schedules WHERE subject_id=?1 AND weekday=6",
                [&subject_id],
                |row| row.get(0),
            )
            .unwrap();
        let connection = database.open().unwrap();
        connection
            .execute(
                "DELETE FROM subject_work_schedule_segments WHERE schedule_id=?1",
                [&sunday_schedule_id],
            )
            .unwrap();
        connection
            .execute(
                "DELETE FROM subject_work_schedules WHERE id=?1",
                [&sunday_schedule_id],
            )
            .unwrap();
        let other_subject = Uuid::now_v7().to_string();
        let other_task = Uuid::now_v7().to_string();
        let now = local_millis("2026-10-11 07:00:00");
        database
            .open()
            .unwrap()
            .execute_batch(&format!(
                "INSERT INTO subjects(id,workspace_id,name,sort_order,created_at,updated_at,version)
                 VALUES ('{other_subject}','{workspace_id}','其他主体',20,{now},{now},1);
                 INSERT INTO tasks(id,workspace_id,subject_id,title,status,source_type,sort_order,created_at,updated_at,version)
                 VALUES ('{other_task}','{workspace_id}','{other_subject}','其他事项','open','manual',10,{now},{now},1);"
            ))
            .unwrap();

        insert_report_work_entry(
            &database,
            &workspace_id,
            Some(&task_id),
            "target-morning",
            "2026-10-11",
            "2026-10-11 08:00:00",
            "2026-10-11 10:00:00",
            "work",
            false,
        );
        insert_report_work_entry(
            &database,
            &workspace_id,
            Some(&task_id),
            "target-afternoon",
            "2026-10-11",
            "2026-10-11 14:00:00",
            "2026-10-11 18:00:00",
            "work",
            false,
        );
        insert_report_work_entry(
            &database,
            &workspace_id,
            Some(&other_task),
            "other-subject",
            "2026-10-11",
            "2026-10-11 06:00:00",
            "2026-10-11 22:00:00",
            "work",
            false,
        );
        insert_report_work_entry(
            &database,
            &workspace_id,
            Some(&task_id),
            "target-break",
            "2026-10-11",
            "2026-10-11 05:00:00",
            "2026-10-11 23:00:00",
            "break",
            false,
        );
        insert_report_work_entry(
            &database,
            &workspace_id,
            Some(&task_id),
            "target-deleted",
            "2026-10-11",
            "2026-10-11 04:00:00",
            "2026-10-11 23:30:00",
            "work",
            true,
        );
        insert_report_work_entry(
            &database,
            &workspace_id,
            None,
            "unallocated",
            "2026-10-11",
            "2026-10-11 03:00:00",
            "2026-10-11 23:45:00",
            "work",
            false,
        );
        let running_started = local_millis("2026-10-11 02:00:00");
        database.open().unwrap().execute(
            "INSERT INTO time_entries(
               id,workspace_id,work_date,kind,source_type,state,default_task_id,label_snapshot,started_at,
               duration_seconds,created_at,updated_at,version
             ) VALUES ('running',?1,'2026-10-11','work','timer','running',?2,'运行中',?3,0,?3,?3,1)",
            params![workspace_id, task_id, running_started],
        ).unwrap();

        let (_, markdown) = render_report(
            &database.open().unwrap(),
            &workspace_id,
            "daily",
            None,
            &subject_id,
            "2026-10-11",
            "2026-10-11",
            &[],
        )
        .unwrap();
        assert!(markdown.contains("08:00-18:00"));
        assert!(!markdown.contains("08:00-10:00 14:00-18:00"));
        assert_eq!(
            preview_report_work_period(
                &database,
                ReportWorkPeriodPreviewRequest {
                    subject_id,
                    reference_date: "2026-10-11".to_string(),
                },
            )
            .unwrap(),
            "08:00-18:00"
        );
    }

    #[test]
    fn daily_report_rest_day_without_actual_work_stays_rest_and_regeneration_refreshes_range() {
        let (database, subject_id, task_id) = setup();
        let workspace_id: String = database
            .open()
            .unwrap()
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            daily_work_period_text(
                &database.open().unwrap(),
                &workspace_id,
                &subject_id,
                "2026-10-11",
                6,
            )
            .unwrap(),
            "休息"
        );
        insert_report_work_entry(
            &database,
            &workspace_id,
            Some(&task_id),
            "initial-overtime",
            "2026-10-11",
            "2026-10-11 10:00:00",
            "2026-10-11 12:00:00",
            "work",
            false,
        );
        let created = create_report(
            &database,
            ReportCreateRequest {
                report_type: "daily".to_string(),
                reference_date: "2026-10-11".to_string(),
                subject_id: subject_id.clone(),
                task_ids: vec![task_id.clone()],
                template_id: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        assert!(created.markdown.contains("10:00-12:00"));
        let edited = save_report_content(
            &database,
            ReportContentSaveRequest {
                report_id: created.id.clone(),
                markdown: "# 手工工作时段 11:00-11:30".to_string(),
                expected_version: created.version,
            },
        )
        .unwrap();
        insert_report_work_entry(
            &database,
            &workspace_id,
            Some(&task_id),
            "later-overtime",
            "2026-10-11",
            "2026-10-11 08:00:00",
            "2026-10-11 18:00:00",
            "work",
            false,
        );
        let regenerated = regenerate_report(
            &database,
            ReportVersionRequest {
                report_id: created.id,
                expected_version: edited.version,
            },
        )
        .unwrap();
        assert!(regenerated.markdown.contains("08:00-18:00"));
        assert!(!regenerated.markdown.contains("11:00-11:30"));
    }

    #[test]
    fn repeated_daily_report_creation_returns_existing_report() {
        let (database, subject_id, task_id) = setup();
        let date = chrono::Local::now().format("%Y-%m-%d").to_string();
        let first = create_report(
            &database,
            ReportCreateRequest {
                report_type: "daily".to_string(),
                reference_date: date.clone(),
                subject_id: subject_id.clone(),
                task_ids: vec![task_id.clone()],
                template_id: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        let error = create_report(
            &database,
            ReportCreateRequest {
                report_type: "daily".to_string(),
                reference_date: date,
                subject_id,
                task_ids: vec![task_id],
                template_id: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap_err();
        assert!(error.starts_with("REPORT_ALREADY_EXISTS:"));
        assert!(error.contains(&first.id));
    }

    #[test]
    fn normalizes_week_and_month_ranges_and_validates_template() {
        assert_eq!(
            normalize_period("weekly", "2026-09-24").unwrap(),
            ("2026-09-21".to_string(), "2026-09-27".to_string())
        );
        assert_eq!(
            normalize_period("monthly", "2026-09-24").unwrap(),
            ("2026-09-01".to_string(), "2026-09-30".to_string())
        );
        assert!(validate_template("daily", "{{未知}} ").is_err());
        assert!(validate_template("daily", "{{日期范围}}").is_err());
        assert!(validate_template("daily", "{{日期").is_err());
        assert!(validate_template("daily", "   ").is_err());
        assert_eq!(format_decimal_hours(12), "0.2h");
        assert_eq!(format_decimal_hours(90), "1.5h");
        assert_eq!(normalize_duration_format("minutes"), "minutes");
        assert_eq!(normalize_duration_format("hours"), "hours");
        assert_eq!(normalize_duration_format("unknown"), "minutes");
        assert_eq!(format_report_duration(90, "minutes"), "90min");
        assert_eq!(format_report_duration(90, "hours"), "1.5h");
    }

    #[test]
    fn named_templates_are_global_selectable_and_keep_report_binding() {
        let (database, subject_id, task_id) = setup();
        let created = create_template_with_cloud_operation(
            &database,
            ReportTemplateCreateRequest {
                report_type: "daily".to_string(),
                name: "精简日报".to_string(),
                description: Some("全部主体通用".to_string()),
                content: "# {{日期}}\n\n{{今日事项}}".to_string(),
            },
            None,
        )
        .unwrap();
        let list = list_templates(
            &database,
            ReportTemplateListRequest {
                report_type: Some("daily".to_string()),
            },
        )
        .unwrap();
        assert!(list.iter().any(|item| item.id == created.id));
        assert_eq!(list.iter().filter(|item| item.is_default).count(), 1);

        let report = create_report(
            &database,
            ReportCreateRequest {
                report_type: "daily".to_string(),
                reference_date: chrono::Local::now().format("%Y-%m-%d").to_string(),
                subject_id,
                task_ids: vec![task_id],
                template_id: Some(created.id.clone()),
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        assert_eq!(report.template_id.as_deref(), Some(created.id.as_str()));
        assert_eq!(report.template_name.as_deref(), Some("精简日报"));

        let default = set_default_template_with_cloud_operation(
            &database,
            ReportTemplateSetDefaultRequest {
                template_id: created.id.clone(),
                expected_version: created.version,
            },
            None,
        )
        .unwrap();
        assert!(default.is_default);
        let regenerated = regenerate_report(
            &database,
            ReportVersionRequest {
                report_id: report.id,
                expected_version: report.version,
            },
        )
        .unwrap();
        assert_eq!(
            regenerated.template_id.as_deref(),
            Some(created.id.as_str())
        );
    }

    #[test]
    fn template_delete_protects_last_item_and_archived_binding_remains_readable() {
        let (database, _, _) = setup();
        let daily = list_templates(
            &database,
            ReportTemplateListRequest {
                report_type: Some("daily".to_string()),
            },
        )
        .unwrap();
        let error = delete_template_with_cloud_operation(
            &database,
            ReportTemplateDeleteRequest {
                template_id: daily[0].id.clone(),
                replacement_template_id: None,
                expected_version: daily[0].version,
            },
            None,
        )
        .unwrap_err();
        assert!(error.contains("至少需要保留"));

        let second = create_template_with_cloud_operation(
            &database,
            ReportTemplateCreateRequest {
                report_type: "daily".to_string(),
                name: "临时日报".to_string(),
                description: None,
                content: "{{今日事项}}".to_string(),
            },
            None,
        )
        .unwrap();
        delete_template_with_cloud_operation(
            &database,
            ReportTemplateDeleteRequest {
                template_id: second.id.clone(),
                replacement_template_id: None,
                expected_version: second.version,
            },
            None,
        )
        .unwrap();
        let connection = database.open().unwrap();
        let archived = load_template_in_workspace(
            &connection,
            &workspace_id(&connection).unwrap(),
            &second.id,
            true,
        )
        .unwrap();
        assert!(archived.deleted_at.is_some());
        assert!(!list_templates(
            &database,
            ReportTemplateListRequest {
                report_type: Some("daily".to_string())
            }
        )
        .unwrap()
        .iter()
        .any(|item| item.id == second.id));
    }

    #[test]
    fn named_template_rules_cover_names_versions_defaults_and_type_binding() {
        let (database, subject_id, task_id) = setup();
        let daily = create_template_with_cloud_operation(
            &database,
            ReportTemplateCreateRequest {
                report_type: "daily".to_string(),
                name: "通用模板".to_string(),
                description: None,
                content: "{{今日事项}}".to_string(),
            },
            None,
        )
        .unwrap();
        let duplicate_error = create_template_with_cloud_operation(
            &database,
            ReportTemplateCreateRequest {
                report_type: "daily".to_string(),
                name: " 通用模板 ".to_string(),
                description: None,
                content: "{{今日事项}}".to_string(),
            },
            None,
        )
        .unwrap_err();
        assert!(duplicate_error.contains("名称已存在"));
        let weekly = create_template_with_cloud_operation(
            &database,
            ReportTemplateCreateRequest {
                report_type: "weekly".to_string(),
                name: "通用模板".to_string(),
                description: None,
                content: "{{统计信息}}".to_string(),
            },
            None,
        )
        .unwrap();
        let copy = duplicate_template_with_cloud_operation(
            &database,
            ReportTemplateDuplicateRequest {
                template_id: daily.id.clone(),
                name: "通用模板副本".to_string(),
                description: None,
            },
            None,
        )
        .unwrap();
        assert!(!copy.is_default);
        let stale_error = update_template_with_cloud_operation(
            &database,
            ReportTemplateUpdateRequest {
                template_id: daily.id.clone(),
                name: daily.name.clone(),
                description: None,
                content: daily.content.clone(),
                sort_order: daily.sort_order,
                expected_version: daily.version + 9,
            },
            None,
        )
        .unwrap_err();
        assert!(stale_error.contains("VERSION_CONFLICT"));
        let default = set_default_template_with_cloud_operation(
            &database,
            ReportTemplateSetDefaultRequest {
                template_id: daily.id.clone(),
                expected_version: daily.version,
            },
            None,
        )
        .unwrap();
        assert!(default.is_default);
        assert_eq!(
            list_templates(
                &database,
                ReportTemplateListRequest {
                    report_type: Some("daily".to_string())
                }
            )
            .unwrap()
            .iter()
            .filter(|item| item.is_default)
            .count(),
            1
        );
        let cross_type = create_report(
            &database,
            ReportCreateRequest {
                report_type: "daily".to_string(),
                reference_date: chrono::Local::now().format("%Y-%m-%d").to_string(),
                subject_id,
                task_ids: vec![task_id],
                template_id: Some(weekly.id),
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap_err();
        assert!(cross_type.contains("类型"));
    }

    #[test]
    fn report_duration_setting_changes_task_metrics() {
        let (database, subject_id, task_id) = setup();
        crate::settings::update_setting(
            &database,
            crate::settings::SettingsUpdate {
                scope: crate::settings::SettingsScope::Shared,
                key: "report_duration_format".to_string(),
                value: serde_json::json!("hours"),
            },
        )
        .unwrap();
        let report = create_report(
            &database,
            ReportCreateRequest {
                report_type: "daily".to_string(),
                reference_date: chrono::Local::now().format("%Y-%m-%d").to_string(),
                subject_id,
                task_ids: vec![task_id],
                template_id: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        assert!(report.markdown.contains("预计：0.3h 实际：0.5h"));
        assert!(!report.markdown.contains("预计：0.5h"));
    }

    #[test]
    fn weekly_report_contains_daily_items_and_salary_summary() {
        let (database, subject_id, task_id) = setup();
        crate::settings::update_setting(
            &database,
            crate::settings::SettingsUpdate {
                scope: crate::settings::SettingsScope::Shared,
                key: "salary_hourly_rate".to_string(),
                value: serde_json::json!(100),
            },
        )
        .unwrap();
        let report = create_report(
            &database,
            ReportCreateRequest {
                report_type: "weekly".to_string(),
                reference_date: chrono::Local::now().format("%Y-%m-%d").to_string(),
                subject_id,
                task_ids: vec![task_id],
                template_id: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        assert!(report.markdown.contains("每日情况"));
        assert!(report.markdown.contains("工资估算：¥"));
        assert!(report.markdown.contains("待报销：尚未查询"));
    }

    #[test]
    fn weekly_report_uses_latest_reimbursement_snapshot_when_available() {
        let (database, subject_id, task_id) = setup();
        let reference_date = chrono::Local::now().format("%Y-%m-%d").to_string();
        let (period_start, period_end) = normalize_period("weekly", &reference_date).unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        connection.execute(
            "INSERT INTO sync_runs(id, workspace_id, provider, operation_type, state, request_json, preview_json, success_count, created_at, started_at, completed_at)
             VALUES (?1, ?2, 'seatable', 'reimbursements_query', 'succeeded', '{}', ?3, 2, 1, 1, 1)",
            params![
                Uuid::now_v7().to_string(),
                workspace_id,
                serde_json::json!({
                    "subjectId": subject_id,
                    "periodStart": period_start,
                    "periodEnd": period_end,
                    "total": 128.5,
                    "items": [{"description": "交通", "amount": 28.5}, {"description": "软件", "amount": 100}]
                }).to_string()
            ],
        ).unwrap();
        drop(connection);
        let report = create_report(
            &database,
            ReportCreateRequest {
                report_type: "weekly".to_string(),
                reference_date,
                subject_id,
                task_ids: vec![task_id],
                template_id: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        assert!(report.markdown.contains("待报销：¥128.50（2 项）"));
    }

    #[test]
    fn public_report_is_regenerated_without_manual_internal_notes() {
        let (database, subject_id, task_id) = setup();
        let date = chrono::Local::now().format("%Y-%m-%d").to_string();
        let created = create_report(
            &database,
            ReportCreateRequest {
                report_type: "daily".to_string(),
                reference_date: date,
                subject_id,
                task_ids: vec![task_id],
                template_id: None,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
        let edited = save_report_content(
            &database,
            ReportContentSaveRequest {
                report_id: created.id.clone(),
                markdown: "内部备注：不要上报".to_string(),
                expected_version: created.version,
            },
        )
        .unwrap();
        let public = public_report(
            &database,
            ReportVersionRequest {
                report_id: edited.id,
                expected_version: edited.version,
            },
        )
        .unwrap();
        assert!(!public.markdown.contains("内部备注：不要上报"));
        assert!(public.markdown.contains("生成日报事项"));
    }
}
