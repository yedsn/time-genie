use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use url::Url;

const REALTIME_RECONNECT_SECONDS: u64 = 5;
pub(crate) const REALTIME_RECONNECT_MAX_SECONDS: u64 = 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceInvalidation {
    pub(crate) workspace_id: String,
    pub(crate) change_seq: i64,
    pub(crate) workspace_epoch: i64,
    pub(crate) domains: Vec<String>,
    pub(crate) source: String,
}

pub(crate) fn realtime_change_seq(message: &Value) -> Option<i64> {
    message
        .pointer("/payload/data/record/change_seq")
        .or_else(|| message.pointer("/payload/record/change_seq"))
        .or_else(|| message.pointer("/payload/data/new/change_seq"))
        .and_then(|value| value.as_i64().or_else(|| value.as_str()?.parse().ok()))
}

pub(crate) fn realtime_workspace_invalidation(
    message: &Value,
    expected_workspace_id: &str,
    workspace_epoch: i64,
) -> Option<WorkspaceInvalidation> {
    let record = message
        .pointer("/payload/data/record")
        .or_else(|| message.pointer("/payload/record"))
        .or_else(|| message.pointer("/payload/data/new"))?;
    let change_seq = record
        .get("change_seq")
        .and_then(|value| value.as_i64().or_else(|| value.as_str()?.parse().ok()))?;
    let workspace_id = record
        .get("workspace_id")
        .and_then(Value::as_str)
        .filter(|value| *value == expected_workspace_id)?;
    let domains = record
        .get("entity_type")
        .and_then(Value::as_str)
        .map(realtime_refresh_domains)
        .unwrap_or_default();
    if domains.is_empty() {
        return None;
    }
    Some(WorkspaceInvalidation {
        workspace_id: workspace_id.to_string(),
        change_seq,
        workspace_epoch,
        domains: domains.into_iter().map(str::to_string).collect(),
        source: "realtime".to_string(),
    })
}

pub(crate) fn realtime_refresh_domains(entity_type: &str) -> Vec<&'static str> {
    match entity_type {
        "subjects" | "subject_work_schedules" => vec!["subjects"],
        "tasks"
        | "task_status_events"
        | "task_daily_estimates"
        | "task_day_plans"
        | "task_recurrence_rules"
        | "task_occurrences" => vec!["tasks"],
        "work_days"
        | "time_entries"
        | "time_segments"
        | "time_allocations"
        | "actual_time_adjustments"
        | "actual_adjustment_batches" => vec!["time"],
        "unassigned_sessions" | "unassigned_segments" => vec!["unassigned"],
        "report_templates" | "reports" | "report_tasks" => vec!["reports"],
        "app_settings" | "integration_configs" | "external_bindings" => vec!["settings"],
        _ => Vec::new(),
    }
}

pub(crate) fn realtime_url(project_url: &str, anon_key: &str) -> Result<Url, String> {
    let mut url = Url::parse(project_url)
        .map_err(|_| "CLOUD_NOT_CONFIGURED: Supabase Project URL 无效".to_string())?;
    let websocket_scheme = match url.scheme() {
        "https" => "wss",
        "http" => "ws",
        _ => return Err("CLOUD_NOT_CONFIGURED: Supabase Realtime 地址无效".to_string()),
    };
    url.set_scheme(websocket_scheme)
        .map_err(|_| "CLOUD_NOT_CONFIGURED: Supabase Realtime 地址无效".to_string())?;
    url.set_path("/realtime/v1/websocket");
    url.set_query(None);
    url.query_pairs_mut()
        .append_pair("apikey", anon_key)
        .append_pair("vsn", "1.0.0");
    Ok(url)
}

pub(crate) fn realtime_reconnect_delay(attempt: u32, jitter_seed: u64) -> Duration {
    let exponent = attempt.min(4);
    let base_millis = REALTIME_RECONNECT_SECONDS
        .saturating_mul(1_u64 << exponent)
        .saturating_mul(1_000)
        .min(REALTIME_RECONNECT_MAX_SECONDS * 1_000);
    let jitter_window = (base_millis / 5).max(1);
    Duration::from_millis(
        (base_millis + jitter_seed % jitter_window).min(REALTIME_RECONNECT_MAX_SECONDS * 1_000),
    )
}
