use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::database::Database;

use super::*;

pub fn direct_cloud_workbench_bootstrap(
    database: &Database,
) -> Result<DirectCloudWorkbenchResponse, String> {
    let context = direct_cloud_read_context(database)?;
    direct_cloud_query(
        database,
        "direct_cloud_workbench_bootstrap",
        &context,
        json!({}),
    )
}

pub fn direct_cloud_subject_list(
    database: &Database,
) -> Result<Vec<crate::subjects::SubjectDto>, String> {
    Ok(direct_cloud_workbench_bootstrap(database)?
        .data
        .subjects
        .into_iter()
        .map(|subject| crate::subjects::SubjectDto {
            id: subject.id,
            name: subject.name,
            sort_order: subject.sort_order,
            open_task_count: subject.open_task_count,
            version: subject.version,
        })
        .collect())
}

pub fn direct_cloud_subject_create(
    database: &Database,
    request: crate::subjects::SubjectCreateRequest,
) -> Result<crate::subjects::SubjectCreateResult, String> {
    let response = direct_cloud_mutation::<crate::subjects::SubjectCreateResult>(
        database,
        "direct_cloud_subject_create",
        None,
        json!({
            "id": Uuid::now_v7().to_string(),
            "name": request.name,
        }),
    )?;
    Ok(response.data)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectCloudSubjectData {
    subject: crate::subjects::SubjectDto,
}

pub fn direct_cloud_subject_rename(
    database: &Database,
    request: crate::subjects::SubjectRenameRequest,
) -> Result<crate::subjects::SubjectDto, String> {
    let response = direct_cloud_mutation::<DirectCloudSubjectData>(
        database,
        "direct_cloud_subject_update",
        None,
        json!({
            "id": request.subject_id,
            "name": request.name,
            "expectedVersion": request.expected_version,
        }),
    )?;
    Ok(response.data.subject)
}

pub fn direct_cloud_task_list(
    database: &Database,
    request: DirectCloudTaskQuery,
) -> Result<DirectCloudTaskListResponse, String> {
    let context = direct_cloud_read_context(database)?;
    direct_cloud_query(
        database,
        "direct_cloud_task_list",
        &context,
        json!({
            "p_subject_id": request.subject_id,
            "p_include_completed": request.include_completed.unwrap_or(true),
            "p_planned_date": request.planned_date,
            "p_query": request.query,
        }),
    )
}

pub fn direct_cloud_completed_tasks(
    database: &Database,
    request: DirectCloudCompletedTasksQuery,
) -> Result<DirectCloudCompletedTasksResponse, String> {
    let context = direct_cloud_read_context(database)?;
    direct_cloud_query(
        database,
        "direct_cloud_completed_tasks",
        &context,
        json!({
            "p_subject_id": request.subject_id,
            "p_cursor_completed_at": request.cursor_completed_at,
            "p_cursor_id": request.cursor_id,
            "p_limit": request.limit.unwrap_or(20).clamp(1,100),
        }),
    )
}

pub fn direct_cloud_task_detail(
    database: &Database,
    task_id: String,
) -> Result<DirectCloudTaskDetailResponse, String> {
    let context = direct_cloud_read_context(database)?;
    direct_cloud_query(
        database,
        "direct_cloud_task_detail",
        &context,
        json!({"p_task_id": task_id}),
    )
}

pub fn direct_cloud_today_plan(
    database: &Database,
    subject_id: String,
    work_date: String,
) -> Result<DirectCloudTodayPlanResponse, String> {
    let context = direct_cloud_read_context(database)?;
    direct_cloud_query(
        database,
        "direct_cloud_today_plan",
        &context,
        json!({"p_subject_id": subject_id, "p_work_date": work_date}),
    )
}

pub fn direct_cloud_today_overview(
    database: &Database,
    request: crate::today_overview::TodayWorkOverviewRequest,
) -> Result<crate::today_overview::TodayWorkOverviewDto, String> {
    let context = direct_cloud_read_context(database)?;
    let history_days = request.history_days.unwrap_or(30).clamp(1, 90);
    let response = direct_cloud_query::<crate::today_overview::TodayWorkOverviewDto>(
        database,
        "direct_cloud_today_overview",
        &context,
        json!({
            "p_subject_id": request.subject_id,
            "p_today_date": request.today_date,
            "p_history_days": history_days,
            "p_trend_days": request.trend_days.unwrap_or(14).clamp(1, history_days),
        }),
    )?;
    Ok(response.data)
}

pub fn direct_cloud_time_state(
    database: &Database,
    work_date: String,
) -> Result<DirectCloudTimeStateResponse, String> {
    let context = direct_cloud_read_context(database)?;
    direct_cloud_query(
        database,
        "direct_cloud_time_state",
        &context,
        json!({"p_work_date": work_date}),
    )
}

pub fn direct_cloud_time_entry_list_for_command(
    database: &Database,
    request: crate::time_tracking::TimeEntryListRequest,
) -> Result<crate::time_tracking::TimeEntryListResult, String> {
    let response = direct_cloud_time_state(database, request.work_date)?;
    let entries = response
        .data
        .entries
        .into_iter()
        .filter(|entry| request.include_breaks || entry.kind != "break")
        .collect::<Vec<_>>();
    let raw_minutes = entries
        .iter()
        .filter(|entry| entry.kind == "work")
        .map(|entry| entry.settlement_minutes)
        .sum();
    let allocated_minutes = entries
        .iter()
        .filter(|entry| entry.kind == "work")
        .map(|entry| entry.allocated_minutes)
        .sum();
    let break_minutes = entries
        .iter()
        .filter(|entry| entry.kind == "break")
        .map(|entry| entry.settlement_minutes)
        .sum();
    Ok(crate::time_tracking::TimeEntryListResult {
        entries,
        raw_minutes,
        allocated_minutes,
        pending_minutes: (raw_minutes - allocated_minutes).max(0),
        break_minutes,
        revision: response.metadata.change_seq,
    })
}

pub fn direct_cloud_active_timer(
    database: &Database,
) -> Result<Option<crate::time_tracking::TimeEntryDto>, String> {
    let context = direct_cloud_read_context(database)?;
    let response = direct_cloud_query::<DirectCloudActiveTimerData>(
        database,
        "direct_cloud_active_timer",
        &context,
        json!({}),
    )?;
    Ok(response.data.active_entry)
}

pub fn direct_cloud_time_history_for_command(
    database: &Database,
    request: crate::time_tracking::TimeEntryHistoryRequest,
) -> Result<crate::time_tracking::TimeEntryHistoryResult, String> {
    let context = direct_cloud_read_context(database)?;
    let response = direct_cloud_query::<DirectCloudTimeHistoryPageData>(
        database,
        "direct_cloud_time_history_page",
        &context,
        json!({
            "p_page": request.page,
            "p_include_breaks": request.include_breaks,
        }),
    )?;
    let groups = response
        .data
        .groups
        .into_iter()
        .map(|group| {
            let raw_minutes = group
                .entries
                .iter()
                .filter(|entry| entry.kind == "work")
                .map(|entry| entry.settlement_minutes)
                .sum();
            let allocated_minutes = group
                .entries
                .iter()
                .filter(|entry| entry.kind == "work")
                .map(|entry| entry.allocated_minutes)
                .sum();
            let break_minutes = group
                .entries
                .iter()
                .filter(|entry| entry.kind == "break")
                .map(|entry| entry.settlement_minutes)
                .sum();
            let adjustment_minutes = group
                .adjustments
                .iter()
                .map(|adjustment| adjustment.delta_minutes)
                .sum::<i64>();
            crate::time_tracking::TimeEntryHistoryDayDto {
                work_date: group.work_date,
                entry_count: group.entries.len() as i64,
                raw_minutes,
                allocated_minutes,
                pending_minutes: (raw_minutes - allocated_minutes).max(0),
                break_minutes,
                entries: group.entries,
                adjustment_minutes,
                final_actual_minutes: (allocated_minutes + adjustment_minutes).max(0),
                adjustments: group.adjustments,
            }
        })
        .collect();
    Ok(crate::time_tracking::TimeEntryHistoryResult {
        page: response.data.page,
        page_size: response.data.page_size,
        total_days: response.data.total_days,
        total_pages: response.data.total_pages,
        groups,
        revision: response.metadata.change_seq,
    })
}

pub fn direct_cloud_unassigned_state(
    database: &Database,
    session_id: Option<String>,
) -> Result<Option<crate::unassigned::UnassignedStateDto>, String> {
    let context = direct_cloud_read_context(database)?;
    let response = direct_cloud_query::<DirectCloudUnassignedStateData>(
        database,
        "direct_cloud_unassigned_state",
        &context,
        json!({"p_session_id": session_id}),
    )?;
    let _ = response.data.work_date;
    Ok(response.data.anchor)
}

pub fn direct_cloud_unassigned_ensure(
    database: &Database,
) -> Result<Option<crate::unassigned::UnassignedStateDto>, String> {
    let context = direct_cloud_read_context(database)?;
    let state_response = direct_cloud_query::<DirectCloudUnassignedStateData>(
        database,
        "direct_cloud_unassigned_state",
        &context,
        json!({"p_session_id": null}),
    )?;
    if state_response.data.anchor.is_some() {
        return Ok(state_response.data.anchor);
    }
    if state_response.data.active_timer {
        return Ok(None);
    }
    let operation_id = direct_timer_operation_id(
        "direct_cloud_unassigned_ensure",
        state_response.data.work_date.as_bytes(),
    );
    let response = direct_cloud_mutation_with_context::<DirectCloudUnassignedEnsureData>(
        database,
        &context,
        "direct_cloud_unassigned_ensure",
        Some(operation_id),
        json!({}),
    )?;
    let _ = response.data.server_time;
    Ok(response.data.anchor)
}

pub fn direct_cloud_unassigned_resolve_work(
    database: &Database,
    request: crate::unassigned::ResolveWorkRequest,
) -> Result<crate::unassigned::UnassignedResolveResult, String> {
    direct_cloud_unassigned_resolve(
        database,
        request.operation_id,
        "work",
        json!({
            "sessionId": request.session_id,
            "expectedVersion": request.expected_version,
            "resolutionType": "work",
            "allocations": request.allocations,
        }),
    )
}

pub fn direct_cloud_unassigned_resolve_session(
    database: &Database,
    request: crate::unassigned::ResolveSessionRequest,
    resolution_type: &str,
) -> Result<crate::unassigned::UnassignedResolveResult, String> {
    direct_cloud_unassigned_resolve(
        database,
        request.operation_id,
        resolution_type,
        json!({
            "sessionId": request.session_id,
            "expectedVersion": request.expected_version,
            "resolutionType": resolution_type,
            "allocations": [],
        }),
    )
}

fn direct_cloud_unassigned_resolve(
    database: &Database,
    operation_id: String,
    resolution_type: &str,
    payload: Value,
) -> Result<crate::unassigned::UnassignedResolveResult, String> {
    let response = direct_cloud_mutation::<DirectCloudUnassignedResolveData>(
        database,
        "direct_cloud_unassigned_resolve",
        Some(operation_id),
        payload,
    )?;
    let generated_entry_id = response
        .data
        .generated_entry
        .as_ref()
        .map(|entry| entry.id.clone());
    let _ = response.data.server_time;
    Ok(crate::unassigned::UnassignedResolveResult {
        session_id: response.data.session.session_id,
        generated_entry_id,
        resolution_type: resolution_type.to_string(),
        elapsed_seconds: response.data.session.elapsed_seconds,
        required_minutes: response.data.session.required_minutes,
        next_session: response.data.next_session,
    })
}

pub fn direct_cloud_actual_adjustment_snapshot(
    database: &Database,
    request: crate::actual_time_adjustments::ActualAdjustmentSnapshotRequest,
) -> Result<crate::actual_time_adjustments::ActualAdjustmentSnapshotDto, String> {
    let context = direct_cloud_read_context(database)?;
    let response = direct_cloud_query(
        database,
        "direct_cloud_actual_adjustment_snapshot",
        &context,
        json!({
            "p_subject_id": request.subject_id,
            "p_work_date": request.work_date,
            "p_execution_only": request.execution_only.unwrap_or(false),
        }),
    )?;
    Ok(response.data)
}

pub fn direct_cloud_report_list(
    database: &Database,
    request: DirectCloudReportListQuery,
) -> Result<DirectCloudReportListResponse, String> {
    let context = direct_cloud_read_context(database)?;
    direct_cloud_query(
        database,
        "direct_cloud_report_list",
        &context,
        json!({
            "p_report_type": request.report_type,
            "p_subject_id": request.subject_id,
            "p_query": request.query,
            "p_cursor_updated_at": request.cursor_updated_at,
            "p_cursor_id": request.cursor_id,
            "p_limit": request.limit.unwrap_or(20).clamp(1,100),
        }),
    )
}

pub fn direct_cloud_report_detail(
    database: &Database,
    report_id: String,
) -> Result<DirectCloudReportDetailResponse, String> {
    let context = direct_cloud_read_context(database)?;
    direct_cloud_query(
        database,
        "direct_cloud_report_detail",
        &context,
        json!({"p_report_id": report_id}),
    )
}

pub fn direct_cloud_report_templates(
    database: &Database,
    report_type: Option<String>,
) -> Result<DirectCloudReportTemplatesResponse, String> {
    let context = direct_cloud_read_context(database)?;
    direct_cloud_query(
        database,
        "direct_cloud_report_templates",
        &context,
        json!({"p_report_type": report_type}),
    )
}

pub fn direct_cloud_report_preview(
    database: &Database,
    request: DirectCloudReportPreviewQuery,
) -> Result<DirectCloudReportPreviewResponse, String> {
    let context = direct_cloud_read_context(database)?;
    direct_cloud_query(
        database,
        "direct_cloud_report_preview",
        &context,
        json!({
            "p_report_type": request.report_type,
            "p_reference_date": request.reference_date,
            "p_subject_id": request.subject_id,
            "p_template_id": request.template_id,
        }),
    )
}

fn direct_cloud_json_mutation(
    database: &Database,
    function: &str,
    operation_id: String,
    payload: Value,
) -> Result<Value, String> {
    Ok(direct_cloud_mutation::<Value>(database, function, Some(operation_id), payload)?.data)
}

pub fn direct_cloud_report_create_mutation(
    database: &Database,
    operation_id: String,
    payload: Value,
) -> Result<Value, String> {
    direct_cloud_json_mutation(
        database,
        "direct_cloud_report_create",
        operation_id,
        payload,
    )
}

pub fn direct_cloud_report_scope_update_mutation(
    database: &Database,
    operation_id: String,
    payload: Value,
) -> Result<Value, String> {
    direct_cloud_json_mutation(
        database,
        "direct_cloud_report_scope_update",
        operation_id,
        payload,
    )
}

pub fn direct_cloud_report_content_save_mutation(
    database: &Database,
    operation_id: String,
    payload: Value,
) -> Result<Value, String> {
    direct_cloud_json_mutation(
        database,
        "direct_cloud_report_content_save",
        operation_id,
        payload,
    )
}

pub fn direct_cloud_report_regenerate_mutation(
    database: &Database,
    operation_id: String,
    payload: Value,
) -> Result<Value, String> {
    direct_cloud_json_mutation(
        database,
        "direct_cloud_report_regenerate",
        operation_id,
        payload,
    )
}

pub fn direct_cloud_report_delete_mutation(
    database: &Database,
    operation_id: String,
    payload: Value,
) -> Result<Value, String> {
    direct_cloud_json_mutation(
        database,
        "direct_cloud_report_delete",
        operation_id,
        payload,
    )
}

pub fn direct_cloud_report_template_create_mutation(
    database: &Database,
    operation_id: String,
    payload: Value,
) -> Result<Value, String> {
    direct_cloud_json_mutation(
        database,
        "direct_cloud_report_template_create",
        operation_id,
        payload,
    )
}

pub fn direct_cloud_report_template_duplicate_mutation(
    database: &Database,
    operation_id: String,
    payload: Value,
) -> Result<Value, String> {
    direct_cloud_json_mutation(
        database,
        "direct_cloud_report_template_duplicate",
        operation_id,
        payload,
    )
}

pub fn direct_cloud_report_template_update_mutation(
    database: &Database,
    operation_id: String,
    payload: Value,
) -> Result<Value, String> {
    direct_cloud_json_mutation(
        database,
        "direct_cloud_report_template_update",
        operation_id,
        payload,
    )
}

pub fn direct_cloud_report_template_set_default_mutation(
    database: &Database,
    operation_id: String,
    payload: Value,
) -> Result<Value, String> {
    direct_cloud_json_mutation(
        database,
        "direct_cloud_report_template_set_default",
        operation_id,
        payload,
    )
}

pub fn direct_cloud_report_template_reorder_mutation(
    database: &Database,
    operation_id: String,
    payload: Value,
) -> Result<Value, String> {
    direct_cloud_json_mutation(
        database,
        "direct_cloud_report_template_reorder",
        operation_id,
        payload,
    )
}

pub fn direct_cloud_report_template_delete_mutation(
    database: &Database,
    operation_id: String,
    payload: Value,
) -> Result<Value, String> {
    direct_cloud_json_mutation(
        database,
        "direct_cloud_report_template_delete",
        operation_id,
        payload,
    )
}

pub fn direct_cloud_shared_setting_update_mutation(
    database: &Database,
    operation_id: String,
    payload: Value,
) -> Result<Value, String> {
    direct_cloud_json_mutation(
        database,
        "direct_cloud_shared_setting_update",
        operation_id,
        payload,
    )
}

pub fn direct_cloud_subject_schedule_save_mutation(
    database: &Database,
    operation_id: String,
    payload: Value,
) -> Result<Value, String> {
    direct_cloud_json_mutation(
        database,
        "direct_cloud_subject_schedule_save",
        operation_id,
        payload,
    )
}

pub fn direct_cloud_integration_config_save_mutation(
    database: &Database,
    operation_id: String,
    payload: Value,
) -> Result<Value, String> {
    direct_cloud_json_mutation(
        database,
        "direct_cloud_integration_config_save",
        operation_id,
        payload,
    )
}

pub fn direct_cloud_external_binding_upsert_mutation(
    database: &Database,
    operation_id: String,
    payload: Value,
) -> Result<Value, String> {
    direct_cloud_json_mutation(
        database,
        "direct_cloud_external_binding_upsert",
        operation_id,
        payload,
    )
}

pub fn direct_cloud_plan_import_confirm_mutation(
    database: &Database,
    operation_id: String,
    payload: Value,
) -> Result<Value, String> {
    direct_cloud_json_mutation(
        database,
        "direct_cloud_plan_import_confirm",
        operation_id,
        payload,
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectCloudDailyReportImportedFingerprints {
    fingerprints: Vec<String>,
}

pub fn direct_cloud_daily_report_imported_fingerprints(
    database: &Database,
    subject_id: String,
    source_date: String,
    target_kind: String,
    source_path_hash: String,
    fingerprints: Vec<String>,
) -> Result<Vec<String>, String> {
    let context = direct_cloud_read_context(database)?;
    let response = direct_cloud_query::<DirectCloudDailyReportImportedFingerprints>(
        database,
        "direct_cloud_daily_report_imported_fingerprints",
        &context,
        json!({
            "p_subject_id": subject_id,
            "p_source_date": source_date,
            "p_target_kind": target_kind,
            "p_source_path_hash": source_path_hash,
            "p_fingerprints": fingerprints,
        }),
    )?;
    Ok(response.data.fingerprints)
}

pub fn direct_cloud_daily_report_import_confirm_mutation(
    database: &Database,
    operation_id: String,
    payload: Value,
) -> Result<Value, String> {
    direct_cloud_json_mutation(
        database,
        "direct_cloud_daily_report_import_confirm",
        operation_id,
        payload,
    )
}

pub fn direct_cloud_time_allocation_export(
    database: &Database,
    request: crate::time_allocation_export::TimeAllocationExportRequest,
) -> Result<Vec<crate::time_allocation_export::TimeAllocationExportRow>, String> {
    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ExportData {
        rows: Vec<crate::time_allocation_export::TimeAllocationExportRow>,
    }
    let context = direct_cloud_read_context(database)?;
    let response = direct_cloud_query::<ExportData>(
        database,
        "direct_cloud_time_allocation_export",
        &context,
        json!({
            "p_start_date": request.start_date,
            "p_end_date": request.end_date,
            "p_include_special_time": request.include_special_time,
        }),
    )?;
    Ok(response.data.rows)
}

pub fn direct_cloud_shared_configuration(
    database: &Database,
) -> Result<DirectCloudSharedConfigurationResponse, String> {
    let context = direct_cloud_read_context(database)?;
    direct_cloud_query(
        database,
        "direct_cloud_shared_configuration",
        &context,
        json!({}),
    )
}
