use base64::Engine;
use reqwest::blocking::{Client, RequestBuilder};
use reqwest::StatusCode;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[cfg(test)]
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;
use url::Url;
use uuid::Uuid;

use crate::database::Database;
use crate::settings::{self, SettingsScope, SettingsUpdate};

const SESSION_PROVIDER: &str = "supabase";
const SESSION_ACCOUNT: &str = "supabase:session";
#[cfg(not(test))]
const SESSION_CREDENTIAL_SERVICE: &str = "timegenie";
#[cfg(all(not(test), windows))]
const SESSION_CREDENTIAL_TARGET_PREFIX: &str = "com.timegenie.desktop";
const SESSION_METADATA_SETTING: &str = "supabase_session_metadata";
const DEFAULT_TIMEOUT_SECONDS: u64 = 12;
const SNAPSHOT_TIMEOUT_SECONDS: u64 = 150;
const RESTORE_TIMEOUT_SECONDS: u64 = 630;
pub(crate) const CLOUD_SCHEMA: &str = "timegenie";
static SESSION_CACHE: OnceLock<Mutex<HashMap<String, CloudSession>>> = OnceLock::new();
static SESSION_REFRESH_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static DIRECT_TRACKING_LEASE_TOKENS: OnceLock<Mutex<HashMap<(String, String), String>>> =
    OnceLock::new();
static DIRECT_CLOUD_ACCESS_STATES: OnceLock<Mutex<HashMap<String, DirectCloudAccessRuntime>>> =
    OnceLock::new();
#[cfg(test)]
static TEST_SESSION_STATE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudSession {
    pub user_id: String,
    pub email: Option<String>,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudSessionSnapshot {
    pub signed_in: bool,
    pub status: String,
    pub reason: Option<String>,
    pub user_id: Option<String>,
    pub email: Option<String>,
    pub expires_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CloudSessionMetadata {
    user_id: String,
    email: Option<String>,
    expires_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudWorkspace {
    pub id: String,
    pub name: String,
    pub timezone: String,
    pub latest_change_seq: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudDevice {
    pub id: String,
    pub workspace_id: String,
    pub device_name: String,
    pub platform: String,
    pub app_version: String,
    pub last_seen_at: String,
    pub authorized_at: String,
    pub reauthorized_at: Option<String>,
    pub revoked_at: Option<String>,
    pub current: bool,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LegacyQueueCounts {
    pub pending: i64,
    pub sending: i64,
    pub failed: i64,
    pub conflict: i64,
    pub total: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StorageProtocolUpgradeServerPreflight {
    workspace_id: String,
    device_id: String,
    storage_protocol: String,
    workspace_epoch: i64,
    server_upgrade_allowed: bool,
    already_direct: bool,
    requires_local_queue_disposition: bool,
    allowed_queue_dispositions: Vec<String>,
    server_time: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageProtocolUpgradePreflight {
    pub workspace_id: String,
    pub device_id: String,
    pub storage_protocol: String,
    pub local_protocol: String,
    pub workspace_epoch: i64,
    pub local_workspace_epoch: i64,
    pub server_upgrade_allowed: bool,
    pub already_direct: bool,
    pub requires_local_queue_disposition: bool,
    pub allowed_queue_dispositions: Vec<String>,
    pub devices: Vec<CloudDevice>,
    pub queue: LegacyQueueCounts,
    pub can_upgrade_with_drained_queue: bool,
    pub blocked_reasons: Vec<String>,
    pub server_time: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageProtocolUpgradeRequest {
    pub operation_id: String,
    #[serde(default)]
    pub create_safety_snapshot: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageProtocolUpgradeResult {
    pub upgraded: bool,
    pub already_direct: bool,
    pub workspace_epoch: i64,
    pub storage_protocol: String,
    pub change_seq: Option<i64>,
    pub safety_snapshot_id: Option<String>,
    pub pushed_operations: i64,
    pub last_change_seq: i64,
    pub refreshed_domains: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageProtocolAbandonRequest {
    pub operation_id: String,
    pub rescue_backup_path: String,
    pub confirmed: bool,
    pub confirmation_text: String,
    #[serde(default)]
    pub create_safety_snapshot: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageProtocolAuthorityResetRequest {
    pub operation_id: String,
    pub confirmed: bool,
    #[serde(default)]
    pub create_safety_snapshot: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageProtocolAbandonResult {
    pub upgraded: bool,
    pub already_direct: bool,
    pub workspace_epoch: i64,
    pub storage_protocol: String,
    pub change_seq: Option<i64>,
    pub safety_snapshot_id: Option<String>,
    pub quarantined_operations: i64,
    pub rescue_backup_path: String,
    pub rescue_backup_checksum: String,
    pub last_change_seq: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StorageProtocolUpgradeServerResult {
    #[serde(default)]
    upgraded: bool,
    #[serde(default)]
    already_direct: bool,
    workspace_epoch: i64,
    #[serde(default)]
    storage_protocol: Option<String>,
    #[serde(default)]
    change_seq: Option<i64>,
    #[serde(default)]
    safety_snapshot_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageModeSnapshot {
    pub mode: String,
    pub storage_protocol: String,
    pub configured_cloud_protocol: String,
    pub storage_backend: String,
    pub cloud_access_state: String,
    pub cloud_writable: bool,
    pub read_only_reason: Option<String>,
    pub workspace_id: String,
    pub device_id: String,
    pub online: bool,
    pub sync_state: String,
    pub pending_operations: i64,
    pub conflict_count: i64,
    pub last_change_seq: i64,
    pub last_synced_at: Option<i64>,
    pub last_error: Option<String>,
    pub auth_blocked: bool,
    pub auth_blocked_reason: Option<String>,
    pub workspace_epoch: i64,
    pub epoch_blocked: bool,
    pub epoch_blocked_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirectCloudAccessState {
    Connecting,
    OnlineWritable,
    OfflineReadonly,
    DegradedReadonly,
    ReauthRequired,
}

impl DirectCloudAccessState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Connecting => "connecting",
            Self::OnlineWritable => "online_writable",
            Self::OfflineReadonly => "offline_readonly",
            Self::DegradedReadonly => "degraded_readonly",
            Self::ReauthRequired => "reauth_required",
        }
    }

    fn writable(self) -> bool {
        self == Self::OnlineWritable
    }
}

#[derive(Debug, Clone)]
struct DirectCloudAccessRuntime {
    state: DirectCloudAccessState,
    reason: Option<String>,
}

#[cfg(test)]
#[derive(Default)]
struct TestCredentialStore {
    values: HashMap<String, String>,
    fail_reads: bool,
    fail_writes: bool,
    fail_deletes: bool,
}

#[cfg(test)]
thread_local! {
    static TEST_CREDENTIAL_STORE: RefCell<TestCredentialStore> = RefCell::new(TestCredentialStore::default());
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationEntityCount {
    pub entity: String,
    pub local_count: i64,
    pub cloud_count: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationPreview {
    pub direction: String,
    pub source_workspace_id: String,
    pub target_workspace_id: Option<String>,
    pub package_version: u32,
    pub entities: Vec<MigrationEntityCount>,
    pub source_entity_count: i64,
    pub target_entity_count: i64,
    pub target_is_empty: bool,
    pub replace_entity_count: i64,
    pub safety_snapshot_required: bool,
    pub conflicts: Vec<String>,
    pub can_execute: bool,
}

const DATA_COPY_CHUNK_MAX_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataCopyStageResult {
    pub package_id: String,
    pub status: String,
    pub entity_counts: HashMap<String, i64>,
    pub content_hash: String,
    pub chunk_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataCopyReplaceRequest {
    pub package_id: String,
    pub content_hash: String,
    pub operation_id: String,
    pub confirmed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataCopyReplaceResult {
    pub replaced: bool,
    pub package_id: String,
    pub content_hash: String,
    pub safety_snapshot_id: String,
    pub previous_epoch: i64,
    pub workspace_epoch: i64,
    pub reset_change_seq: i64,
    pub entity_counts: HashMap<String, i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataCopyExportManifest {
    pub package_id: String,
    pub package_version: u32,
    pub workspace_epoch: i64,
    pub change_seq: i64,
    pub entity_counts: HashMap<String, i64>,
    pub content_hash: String,
    pub chunk_count: usize,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataCopyExportChunk {
    pub package_id: String,
    pub chunk_index: usize,
    pub entity_name: String,
    pub content_text: String,
    pub content_hash: String,
    pub row_count: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataCopyExportChunkRequest {
    pub package_id: String,
    pub chunk_index: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataCopyExportDiscardRequest {
    pub package_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataCopyReplaceLocalRequest {
    pub confirmed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataCopyReplaceLocalResult {
    pub replaced: bool,
    pub package_id: String,
    pub content_hash: String,
    pub safety_backup_path: String,
    pub workspace_id: String,
    pub entity_counts: HashMap<String, i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DataCopyExportPreviewResponse {
    workspace_id: String,
    package_version: u32,
    workspace_epoch: i64,
    change_seq: i64,
    entity_counts: HashMap<String, i64>,
}

#[derive(Debug, Clone)]
struct DataCopyChunk {
    chunk_index: usize,
    entity_name: String,
    content_text: String,
    content_hash: String,
    row_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudOperationHistoryCursor {
    pub occurred_at: String,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudOperationHistoryItem {
    pub id: String,
    pub operation_id: String,
    pub business_type: String,
    pub source: String,
    pub device_id: Option<String>,
    pub device_name: String,
    pub summary: String,
    pub affected_entity_count: i64,
    pub occurred_at: String,
    pub expires_at: String,
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudOperationHistoryPage {
    pub items: Vec<CloudOperationHistoryItem>,
    pub next_cursor: Option<CloudOperationHistoryCursor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudOperationHistoryDetailItem {
    pub id: i64,
    pub entity_type: String,
    pub entity_id: String,
    pub entity_label: String,
    pub action: String,
    pub before: Option<Value>,
    pub after: Option<Value>,
    pub changed_fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudOperationHistoryDetail {
    pub id: String,
    pub operation_id: String,
    pub business_type: String,
    pub source: String,
    pub device_id: Option<String>,
    pub device_name_snapshot: String,
    pub summary: String,
    pub affected_entity_count: i64,
    pub metadata: Value,
    pub occurred_at: String,
    pub expires_at: String,
    pub items: Vec<CloudOperationHistoryDetailItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudOperationHistoryListRequest {
    pub limit: Option<i64>,
    pub cursor: Option<CloudOperationHistoryCursor>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub device_id: Option<String>,
    pub source: Option<String>,
    pub business_type: Option<String>,
    pub query: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudOperationHistoryDetailRequest {
    pub log_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudWorkspaceSnapshot {
    pub id: String,
    pub snapshot_type: String,
    pub name: String,
    pub note: Option<String>,
    pub status: String,
    pub format_version: i64,
    pub base_change_seq: i64,
    pub workspace_epoch: i64,
    pub entity_counts: Value,
    pub group_hashes: Value,
    pub content_hash: Option<String>,
    pub size_bytes: i64,
    pub chunk_count: i64,
    pub device_name: String,
    pub protected: bool,
    pub protected_at: Option<String>,
    pub expires_at: Option<String>,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudWorkspaceSnapshotPreview {
    pub snapshot_id: String,
    pub restorable: bool,
    pub error_code: Option<String>,
    pub entity_counts: Option<Value>,
    pub group_counts: Option<Value>,
    pub group_hashes: Option<Value>,
    pub content_hash: Option<String>,
    pub size_bytes: Option<i64>,
    pub chunk_count: Option<i64>,
    pub base_change_seq: Option<i64>,
    pub workspace_epoch: Option<i64>,
    pub created_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudStorageError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub requires_refresh: bool,
}

impl CloudStorageError {
    fn from_http_body(body: &str) -> Option<Self> {
        let parsed = serde_json::from_str::<Value>(body).ok()?;
        let message = parsed.get("message")?.as_str()?.trim();
        let explicit_code = parsed
            .get("detail")
            .and_then(Value::as_str)
            .and_then(|detail| serde_json::from_str::<Value>(detail).ok())
            .and_then(|detail| {
                detail
                    .get("code")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .or_else(|| {
                parsed
                    .get("error_code")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            });
        let prefix_code = message
            .split_once(':')
            .map(|(code, _)| code.trim().to_string());
        let code = explicit_code.or(prefix_code).unwrap_or_default();
        if !matches!(
            code.as_str(),
            "VERSION_CONFLICT"
                | "ENTITY_DELETED"
                | "LEASE_LOST"
                | "WORKSPACE_EPOCH_STALE"
                | "DEVICE_REVOKED"
                | "CLOUD_PROTOCOL_UNSUPPORTED"
                | "EMERGENCY_READ_ONLY"
                | "IDEMPOTENCY_CONFLICT"
        ) {
            return None;
        }
        let detail = message
            .strip_prefix(code.as_str())
            .and_then(|rest| rest.strip_prefix(':'))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(message)
            .to_string();
        Some(Self {
            code: code.to_string(),
            message: sanitize_remote_error(&detail),
            retryable: false,
            requires_refresh: matches!(
                code.as_str(),
                "VERSION_CONFLICT"
                    | "ENTITY_DELETED"
                    | "LEASE_LOST"
                    | "WORKSPACE_EPOCH_STALE"
                    | "DEVICE_REVOKED"
            ),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectCloudQueryMetadata {
    pub workspace_id: String,
    pub device_id: String,
    pub storage_protocol: String,
    pub workspace_epoch: i64,
    pub change_seq: i64,
    pub server_time: String,
    pub domains: Vec<String>,
    #[serde(default)]
    pub writable: Option<bool>,
    #[serde(default)]
    pub read_only_reason: Option<String>,
    #[serde(default)]
    pub diagnostics: Option<DirectCloudQueryDiagnostics>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectCloudQueryDiagnostics {
    pub response_bytes: i64,
    pub rpc_count: i64,
    pub duration_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectCloudQueryResponse<T> {
    pub metadata: DirectCloudQueryMetadata,
    pub data: T,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectCloudMutationResponse<T> {
    pub operation_id: String,
    pub metadata: DirectCloudQueryMetadata,
    pub data: T,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectCloudWorkbenchSubject {
    pub id: String,
    pub name: String,
    pub sort_order: i64,
    pub open_task_count: i64,
    pub version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectCloudWorkbenchSummary {
    pub subject_count: i64,
    pub open_task_count: i64,
    pub completed_task_count: i64,
    pub task_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectCloudWorkbenchDevice {
    pub id: String,
    pub device_name: String,
    pub platform: String,
    pub app_version: String,
    pub last_seen_at: String,
    pub authorized_at: String,
    pub reauthorized_at: Option<String>,
    pub writable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectCloudWorkbenchData {
    pub workspace_id: String,
    pub workspace_name: String,
    pub timezone: String,
    pub current_work_date: String,
    pub current_day_start_at: i64,
    pub current_day_end_at: i64,
    pub subjects: Vec<DirectCloudWorkbenchSubject>,
    pub summary: DirectCloudWorkbenchSummary,
    pub device: DirectCloudWorkbenchDevice,
}

pub type DirectCloudWorkbenchResponse = DirectCloudQueryResponse<DirectCloudWorkbenchData>;
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectCloudTaskQuery {
    pub subject_id: Option<String>,
    pub include_completed: Option<bool>,
    pub planned_date: Option<String>,
    pub query: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectCloudCompletedTasksQuery {
    pub subject_id: Option<String>,
    pub cursor_completed_at: Option<String>,
    pub cursor_id: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectCloudReportListQuery {
    pub report_type: Option<String>,
    pub subject_id: Option<String>,
    pub query: Option<String>,
    pub cursor_updated_at: Option<String>,
    pub cursor_id: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectCloudReportPreviewQuery {
    pub report_type: String,
    pub reference_date: String,
    pub subject_id: String,
    pub template_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectCloudTaskData {
    pub id: String,
    pub subject_id: String,
    pub parent_id: Option<String>,
    pub day_instance_date: Option<String>,
    pub day_instance_kind: Option<String>,
    pub day_instance_source_task_id: Option<String>,
    pub title: String,
    pub status: String,
    pub planned_date: Option<String>,
    pub estimate_minutes: Option<i64>,
    pub note: Option<String>,
    pub project_name: Option<String>,
    pub solution_name: Option<String>,
    pub sort_order: i64,
    pub completed_at: Option<i64>,
    pub depth: i64,
    pub path_label: String,
    pub selectable: bool,
    pub direct_minutes: i64,
    pub total_minutes: i64,
    pub execution_state: String,
    #[serde(default)]
    pub today_estimate_minutes: Option<i64>,
    #[serde(default)]
    pub daily_estimate_version: Option<i64>,
    #[serde(default)]
    pub day_plan_state: Option<String>,
    #[serde(default)]
    pub day_plan_version: Option<i64>,
    #[serde(default)]
    pub today_member: bool,
    #[serde(default)]
    pub today_context: bool,
    #[serde(default)]
    pub recurrence: Option<crate::recurring::RecurrenceRuleDto>,
    #[serde(default)]
    pub occurrence_date: Option<String>,
    #[serde(default)]
    pub occurrence_origin: Option<String>,
    #[serde(default)]
    pub occurrence_version: Option<i64>,
    pub version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectCloudTaskListData {
    pub tasks: Vec<DirectCloudTaskData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectCloudCompletedTaskCursor {
    pub completed_at: String,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectCloudCompletedTasksData {
    pub tasks: Vec<DirectCloudTaskData>,
    #[serde(default)]
    pub total: i64,
    pub has_more: bool,
    pub next_cursor: Option<DirectCloudCompletedTaskCursor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectCloudTaskDetailData {
    pub task: DirectCloudTaskData,
    pub children: Vec<DirectCloudTaskData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectCloudTodayPlanData {
    pub subject_id: String,
    pub work_date: String,
    pub members: Vec<DirectCloudTaskData>,
    pub contexts: Vec<DirectCloudTaskData>,
    pub plans: Vec<DirectCloudTodayPlanRecord>,
    pub estimates: Vec<DirectCloudTodayEstimateRecord>,
    pub recurrence_rules: Vec<DirectCloudTodayRecurrenceRule>,
    pub occurrences: Vec<crate::recurring::TaskOccurrenceDto>,
    pub instances: Vec<DirectCloudTaskData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectCloudTodayPlanRecord {
    pub task_id: String,
    pub work_date: String,
    pub state: String,
    pub instance_task_id: Option<String>,
    pub version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectCloudTodayEstimateRecord {
    pub task_id: String,
    pub estimate_minutes: i64,
    pub version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectCloudTodayRecurrenceRule {
    pub id: String,
    pub task_id: String,
    pub frequency: String,
    pub weekdays_mask: Option<i64>,
    pub effective_start: String,
    pub effective_end: Option<String>,
    pub summary: String,
    pub version: i64,
    pub matches_date: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectCloudTimeStateData {
    pub work_date: String,
    pub active_entry: Option<crate::time_tracking::TimeEntryDto>,
    pub entries: Vec<crate::time_tracking::TimeEntryDto>,
    pub unassigned: Option<crate::unassigned::UnassignedStateDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectCloudTimeHistoryGroupData {
    pub work_date: String,
    pub entries: Vec<crate::time_tracking::TimeEntryDto>,
    pub adjustments: Vec<crate::actual_time_adjustments::ActualTimeAdjustmentDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectCloudTimeHistoryPageData {
    page: i64,
    page_size: i64,
    total_days: i64,
    total_pages: i64,
    groups: Vec<DirectCloudTimeHistoryGroupData>,
    has_more: bool,
    next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectCloudReportListData {
    pub reports: Vec<Value>,
    pub has_more: bool,
    pub next_cursor: Option<Value>,
    pub type_counts: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectCloudReportDetailData {
    pub report: Value,
    pub tasks: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectCloudReportTemplatesData {
    pub templates: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectCloudReportPreviewData {
    pub report_type: String,
    pub reference_date: String,
    pub period_start: String,
    pub period_end: String,
    pub subject_id: String,
    pub subject_name: String,
    pub template: Value,
    pub context: Value,
    pub markdown: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectCloudSharedConfigurationData {
    pub workspace: Value,
    pub settings: Value,
    pub setting_versions: Value,
    pub work_days: Vec<Value>,
    pub subject_work_schedules: Vec<Value>,
    pub integrations: Vec<Value>,
    pub bindings: Vec<Value>,
}

pub type DirectCloudTaskListResponse = DirectCloudQueryResponse<DirectCloudTaskListData>;
pub type DirectCloudCompletedTasksResponse =
    DirectCloudQueryResponse<DirectCloudCompletedTasksData>;
pub type DirectCloudTaskDetailResponse = DirectCloudQueryResponse<DirectCloudTaskDetailData>;
pub type DirectCloudTodayPlanResponse = DirectCloudQueryResponse<DirectCloudTodayPlanData>;
pub type DirectCloudTimeStateResponse = DirectCloudQueryResponse<DirectCloudTimeStateData>;
pub type DirectCloudReportListResponse = DirectCloudQueryResponse<DirectCloudReportListData>;
pub type DirectCloudReportDetailResponse = DirectCloudQueryResponse<DirectCloudReportDetailData>;
pub type DirectCloudReportTemplatesResponse =
    DirectCloudQueryResponse<DirectCloudReportTemplatesData>;
pub type DirectCloudReportPreviewResponse = DirectCloudQueryResponse<DirectCloudReportPreviewData>;
pub type DirectCloudSharedConfigurationResponse =
    DirectCloudQueryResponse<DirectCloudSharedConfigurationData>;

impl From<DirectCloudTaskData> for crate::tasks::TaskDto {
    fn from(value: DirectCloudTaskData) -> Self {
        Self {
            id: value.id,
            subject_id: value.subject_id,
            parent_id: value.parent_id,
            day_instance_date: value.day_instance_date,
            day_instance_kind: value.day_instance_kind,
            day_instance_source_task_id: value.day_instance_source_task_id,
            title: value.title,
            status: value.status,
            planned_date: value.planned_date,
            estimate_minutes: value.estimate_minutes,
            today_estimate_minutes: value.today_estimate_minutes,
            daily_estimate_version: value.daily_estimate_version,
            day_plan_state: value.day_plan_state,
            day_plan_version: value.day_plan_version,
            today_member: value.today_member,
            today_context: value.today_context,
            note: value.note,
            project_name: value.project_name,
            solution_name: value.solution_name,
            sort_order: value.sort_order,
            completed_at: value.completed_at,
            depth: value.depth.max(0) as usize,
            path_label: value.path_label,
            selectable: value.selectable,
            direct_minutes: value.direct_minutes,
            total_minutes: value.total_minutes,
            execution_state: value.execution_state,
            recurrence: value.recurrence,
            occurrence_date: value.occurrence_date,
            occurrence_origin: value.occurrence_origin,
            occurrence_version: value.occurrence_version,
            version: value.version,
        }
    }
}

impl DirectCloudTaskListResponse {
    pub fn into_task_list_result(self) -> crate::tasks::TaskListResult {
        crate::tasks::TaskListResult {
            tasks: self.data.tasks.into_iter().map(Into::into).collect(),
            revision: self.metadata.change_seq,
        }
    }
}

fn direct_cloud_recurrence_rule(
    value: DirectCloudTodayRecurrenceRule,
) -> crate::recurring::RecurrenceRuleDto {
    crate::recurring::RecurrenceRuleDto {
        id: value.id,
        task_id: value.task_id,
        frequency: value.frequency,
        weekdays_mask: value.weekdays_mask,
        effective_start: value.effective_start,
        effective_end: value.effective_end,
        summary: value.summary,
        version: value.version,
    }
}

fn direct_cloud_enrich_today_plan_data(data: &mut DirectCloudTodayPlanData) {
    let plans = data
        .plans
        .iter()
        .map(|plan| (plan.task_id.clone(), plan.clone()))
        .collect::<HashMap<_, _>>();
    let estimates = data
        .estimates
        .iter()
        .map(|estimate| (estimate.task_id.clone(), estimate.clone()))
        .collect::<HashMap<_, _>>();
    let rules = data
        .recurrence_rules
        .iter()
        .filter(|rule| rule.matches_date)
        .map(|rule| (rule.task_id.clone(), rule.clone()))
        .collect::<HashMap<_, _>>();
    let occurrences = data
        .occurrences
        .iter()
        .map(|occurrence| (occurrence.task_id.clone(), occurrence.clone()))
        .collect::<HashMap<_, _>>();
    let source_by_instance = data
        .instances
        .iter()
        .filter_map(|instance| {
            instance
                .day_instance_source_task_id
                .as_ref()
                .map(|source_id| (instance.id.clone(), source_id.clone()))
        })
        .collect::<HashMap<_, _>>();

    let enrich = |task: &mut DirectCloudTaskData| {
        let source_id = task
            .day_instance_source_task_id
            .as_deref()
            .or_else(|| source_by_instance.get(&task.id).map(String::as_str))
            .unwrap_or(task.id.as_str());
        if let Some(plan) = plans.get(source_id) {
            task.day_plan_state = Some(plan.state.clone());
            task.day_plan_version = Some(plan.version);
        }
        if let Some(estimate) = estimates.get(&task.id).or_else(|| estimates.get(source_id)) {
            task.today_estimate_minutes = Some(estimate.estimate_minutes);
            task.daily_estimate_version = Some(estimate.version);
        }
        if let Some(rule) = rules.get(source_id) {
            task.recurrence = Some(direct_cloud_recurrence_rule(rule.clone()));
            task.occurrence_date = Some(data.work_date.clone());
            if let Some(occurrence) = occurrences.get(source_id) {
                task.status = occurrence.status.clone();
                task.occurrence_origin = Some(occurrence.origin.clone());
                task.occurrence_version = Some(occurrence.version);
            } else {
                task.status = "open".to_string();
                task.occurrence_origin = Some("scheduled".to_string());
                task.occurrence_version = Some(0);
            }
        }
    };
    for task in &mut data.members {
        enrich(task);
    }
    for task in &mut data.contexts {
        enrich(task);
    }
    for task in &mut data.instances {
        enrich(task);
    }
}

fn direct_cloud_task_query_for_request(
    database: &Database,
    request: &crate::tasks::TaskListRequest,
) -> Result<DirectCloudTaskListResponse, String> {
    direct_cloud_task_list(
        database,
        DirectCloudTaskQuery {
            subject_id: request.subject_id.clone(),
            include_completed: request.include_completed,
            planned_date: request.planned_date.clone(),
            query: request.query.clone(),
        },
    )
}

pub fn direct_cloud_task_list_for_command(
    database: &Database,
    request: crate::tasks::TaskListRequest,
) -> Result<crate::tasks::TaskListResult, String> {
    if request.today_only.unwrap_or(false) {
        let work_date = request
            .today_date
            .ok_or_else(|| "VALIDATION_ERROR: 今日事项筛选缺少日期".to_string())?;
        let mut tasks = Vec::new();
        if let Some(subject_id) = request.subject_id {
            let mut response = direct_cloud_today_plan(database, subject_id, work_date)?;
            direct_cloud_enrich_today_plan_data(&mut response.data);
            tasks.extend(response.data.contexts.into_iter().map(Into::into));
            tasks.extend(response.data.members.into_iter().map(Into::into));
            return Ok(crate::tasks::TaskListResult {
                tasks,
                revision: response.metadata.change_seq,
            });
        }
        let bootstrap = direct_cloud_workbench_bootstrap(database)?;
        let revision = bootstrap.metadata.change_seq;
        for subject in bootstrap.data.subjects {
            let mut response = direct_cloud_today_plan(database, subject.id, work_date.clone())?;
            direct_cloud_enrich_today_plan_data(&mut response.data);
            tasks.extend(response.data.contexts.into_iter().map(Into::into));
            tasks.extend(response.data.members.into_iter().map(Into::into));
        }
        return Ok(crate::tasks::TaskListResult { tasks, revision });
    }
    let mut response = direct_cloud_task_query_for_request(database, &request)?;
    if let Some(work_date) = request.daily_estimate_date.as_deref() {
        let subject_ids = if let Some(subject_id) = request.subject_id.clone() {
            vec![subject_id]
        } else {
            direct_cloud_workbench_bootstrap(database)?
                .data
                .subjects
                .into_iter()
                .map(|subject| subject.id)
                .collect()
        };
        let mut daily_by_id = HashMap::new();
        for subject_id in subject_ids {
            let mut today = direct_cloud_today_plan(database, subject_id, work_date.to_string())?;
            direct_cloud_enrich_today_plan_data(&mut today.data);
            for task in today.data.members.into_iter().chain(today.data.contexts) {
                daily_by_id.insert(task.id.clone(), task);
            }
        }
        for task in &mut response.data.tasks {
            if let Some(daily) = daily_by_id.remove(&task.id) {
                task.today_estimate_minutes = daily.today_estimate_minutes;
                task.daily_estimate_version = daily.daily_estimate_version;
                task.day_plan_state = daily.day_plan_state;
                task.day_plan_version = daily.day_plan_version;
                task.today_member = daily.today_member;
                task.today_context = daily.today_context;
                task.recurrence = daily.recurrence;
                task.occurrence_date = daily.occurrence_date;
                task.occurrence_origin = daily.occurrence_origin;
                task.occurrence_version = daily.occurrence_version;
            }
        }
    }
    Ok(response.into_task_list_result())
}

pub fn direct_cloud_completed_task_page_for_command(
    database: &Database,
    request: crate::tasks::CompletedTaskPageRequest,
) -> Result<crate::tasks::CompletedTaskPageResult, String> {
    let page = request.page.unwrap_or(1);
    let page_size = request.page_size.unwrap_or(20);
    if page < 1 || !(1..=100).contains(&page_size) {
        return Err("VALIDATION_ERROR: 页码或每页条数无效".to_string());
    }
    let mut cursor_completed_at = None;
    let mut cursor_id = None;
    let mut response = None;
    for _ in 1..=page {
        let current = direct_cloud_completed_tasks(
            database,
            DirectCloudCompletedTasksQuery {
                subject_id: Some(request.subject_id.clone()),
                cursor_completed_at,
                cursor_id,
                limit: Some(page_size),
            },
        )?;
        cursor_completed_at = current
            .data
            .next_cursor
            .as_ref()
            .map(|cursor| cursor.completed_at.clone());
        cursor_id = current
            .data
            .next_cursor
            .as_ref()
            .map(|cursor| cursor.id.clone());
        response = Some(current);
        if cursor_id.is_none() {
            break;
        }
    }
    let response =
        response.ok_or_else(|| "CLOUD_RESPONSE_INVALID: 已完成事项响应为空".to_string())?;
    let total = response.data.total;
    Ok(crate::tasks::CompletedTaskPageResult {
        tasks: response.data.tasks.into_iter().map(Into::into).collect(),
        page,
        page_size,
        total,
        total_pages: if total == 0 {
            0
        } else {
            (total + page_size - 1) / page_size
        },
    })
}

pub fn direct_cloud_day_plan_snapshot_for_command(
    database: &Database,
    request: crate::today_plans::TaskDayPlanSnapshotRequest,
) -> Result<crate::today_plans::TaskDayPlanSnapshotDto, String> {
    let mut response = direct_cloud_today_plan(
        database,
        request.subject_id.clone(),
        request.work_date.clone(),
    )?;
    direct_cloud_enrich_today_plan_data(&mut response.data);
    let mut tasks = response
        .data
        .instances
        .into_iter()
        .map(crate::tasks::TaskDto::from)
        .collect::<Vec<_>>();
    for task in &mut tasks {
        task.today_member = task.day_instance_kind.as_deref() == Some("member");
        task.today_context = task.day_instance_kind.as_deref() == Some("context");
    }
    Ok(crate::today_plans::TaskDayPlanSnapshotDto {
        subject_id: request.subject_id,
        work_date: request.work_date,
        tasks,
        revision: response.metadata.change_seq,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectCloudTaskMutationData {
    task: DirectCloudTaskData,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectCloudDailyEstimateData {
    task_id: String,
    work_date: String,
    daily_estimate: Option<DirectCloudDailyEstimateRecord>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectCloudDailyEstimateRecord {
    estimate_minutes: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectCloudChangedCountData {
    changed_count: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectCloudDayPlanData {
    plan: crate::today_plans::TaskDayPlanDto,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectCloudRecurrenceData {
    rule: crate::recurring::RecurrenceRuleDto,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectCloudOccurrenceStatusData {
    task: DirectCloudTaskData,
    instance: Option<DirectCloudTaskData>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectCloudTodayRecurringUpdateData {
    instance: DirectCloudTaskData,
    source: DirectCloudTaskData,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectCloudTaskDuplicateData {
    root_id: String,
    tasks: Vec<DirectCloudTaskData>,
}

#[derive(Debug, Deserialize)]
struct DirectCloudTimerEntryData {
    entry: crate::time_tracking::TimeEntryDto,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectCloudTimerStopData {
    current_entry: crate::time_tracking::TimeEntryDto,
    slices: Vec<crate::time_tracking::TimeEntryDto>,
    total_duration_seconds: i64,
    total_settlement_minutes: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectCloudTimerTransitionData {
    created_task: Option<crate::tasks::TaskDto>,
    previous_stop_result: Option<crate::time_tracking::TimerStopResultDto>,
    active_entry: crate::time_tracking::TimeEntryDto,
    transition_at: i64,
    no_op: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectCloudTimerRecoveryData {
    active_entry: crate::time_tracking::TimeEntryDto,
    slices: Vec<crate::time_tracking::TimeEntryDto>,
    recovered: bool,
    calendar_rollover: bool,
    continuity_interrupted: bool,
    created_slice_count: i64,
    observed_at: i64,
    no_op: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectCloudActiveTimerData {
    active_entry: Option<crate::time_tracking::TimeEntryDto>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectCloudUnassignedStateData {
    anchor: Option<crate::unassigned::UnassignedStateDto>,
    work_date: String,
    active_timer: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectCloudUnassignedEnsureData {
    anchor: Option<crate::unassigned::UnassignedStateDto>,
    server_time: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectCloudUnassignedResolveData {
    session: crate::unassigned::UnassignedStateDto,
    generated_entry: Option<crate::time_tracking::TimeEntryDto>,
    server_time: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectCloudActualAdjustmentData {
    adjustment: crate::actual_time_adjustments::ActualTimeAdjustmentDto,
}

fn direct_timer_operation_id(function: &str, request: &[u8]) -> String {
    let mut material = Vec::with_capacity(function.len() + request.len() + 1);
    material.extend_from_slice(function.as_bytes());
    material.push(b':');
    material.extend_from_slice(request);
    Uuid::new_v5(&Uuid::NAMESPACE_URL, &material).to_string()
}

pub fn direct_cloud_task_create_for_command(
    database: &Database,
    request: crate::tasks::TaskCreateRequest,
    source_type: &str,
) -> Result<crate::tasks::TaskDto, String> {
    let response = direct_cloud_mutation::<DirectCloudTaskMutationData>(
        database,
        "direct_cloud_task_create",
        None,
        json!({
            "id": Uuid::now_v7().to_string(),
            "subjectId": request.subject_id,
            "parentId": request.parent_id,
            "dayInstanceDate": request.day_instance_date,
            "title": request.title,
            "plannedDate": request.planned_date,
            "estimateMinutes": request.estimate_minutes,
            "note": request.note,
            "projectName": request.project_name,
            "solutionName": request.solution_name,
            "sourceType": source_type,
        }),
    )?;
    Ok(response.data.task.into())
}

pub fn direct_cloud_task_create_positioned_for_command(
    database: &Database,
    request: crate::tasks::TaskCreatePositionedRequest,
) -> Result<crate::tasks::TaskDto, String> {
    let response = direct_cloud_mutation::<DirectCloudTaskMutationData>(
        database,
        "direct_cloud_task_create",
        None,
        json!({
            "id": Uuid::now_v7().to_string(),
            "subjectId": request.subject_id,
            "parentId": request.parent_id,
            "dayInstanceDate": request.day_instance_date,
            "title": request.title,
            "plannedDate": request.planned_date,
            "estimateMinutes": request.estimate_minutes,
            "note": request.note,
            "projectName": request.project_name,
            "solutionName": request.solution_name,
            "sourceType": "manual",
            "previousSiblingId": request.previous_sibling_id,
            "nextSiblingId": request.next_sibling_id,
        }),
    )?;
    Ok(response.data.task.into())
}

pub fn direct_cloud_task_update_for_command(
    database: &Database,
    request: crate::tasks::TaskUpdateRequest,
) -> Result<crate::tasks::TaskDto, String> {
    let mut payload = serde_json::Map::new();
    payload.insert("id".to_string(), json!(request.id));
    payload.insert(
        "expectedVersion".to_string(),
        json!(request.expected_version),
    );
    if let Some(value) = request.title {
        payload.insert("title".to_string(), json!(value));
    }
    if let Some(value) = request.parent_id {
        payload.insert("parentId".to_string(), json!(value));
    }
    if let Some(value) = request.planned_date {
        payload.insert("plannedDate".to_string(), json!(value));
    }
    if let Some(value) = request.estimate_minutes {
        payload.insert("estimateMinutes".to_string(), json!(value));
    }
    if let Some(value) = request.note {
        payload.insert("note".to_string(), json!(value));
    }
    if let Some(value) = request.project_name {
        payload.insert("projectName".to_string(), json!(value));
    }
    if let Some(value) = request.solution_name {
        payload.insert("solutionName".to_string(), json!(value));
    }
    let response = direct_cloud_mutation::<DirectCloudTaskMutationData>(
        database,
        "direct_cloud_task_update",
        None,
        Value::Object(payload),
    )?;
    Ok(response.data.task.into())
}

pub fn direct_cloud_task_move_for_command(
    database: &Database,
    request: crate::tasks::TaskMoveRequest,
) -> Result<crate::tasks::TaskDto, String> {
    let response = direct_cloud_mutation::<DirectCloudTaskMutationData>(
        database,
        "direct_cloud_task_move",
        None,
        json!({
            "id": request.id,
            "expectedVersion": request.expected_version,
            "parentId": request.parent_id,
            "previousSiblingId": request.previous_sibling_id,
            "nextSiblingId": request.next_sibling_id,
        }),
    )?;
    Ok(response.data.task.into())
}

pub fn direct_cloud_task_change_parent_for_command(
    database: &Database,
    request: crate::tasks::TaskChangeParentRequest,
) -> Result<crate::tasks::TaskDto, String> {
    let response = direct_cloud_mutation::<DirectCloudTaskMutationData>(
        database,
        "direct_cloud_task_move",
        None,
        json!({
            "id": request.id,
            "expectedVersion": request.expected_version,
            "parentId": request.new_parent_id,
            "sortOrder": request.sort_order,
        }),
    )?;
    Ok(response.data.task.into())
}

pub fn direct_cloud_task_reorder_for_command(
    database: &Database,
    request: crate::tasks::TaskReorderRequest,
) -> Result<crate::tasks::TaskDto, String> {
    let response = direct_cloud_mutation::<DirectCloudTaskMutationData>(
        database,
        "direct_cloud_task_move",
        None,
        json!({
            "id": request.id,
            "expectedVersion": request.expected_version,
            "parentId": request.parent_id,
            "sortOrder": request.sort_order,
        }),
    )?;
    Ok(response.data.task.into())
}

pub fn direct_cloud_task_delete_for_command(
    database: &Database,
    request: crate::tasks::TaskDeleteRequest,
) -> Result<(), String> {
    direct_cloud_mutation::<Value>(
        database,
        "direct_cloud_task_archive_subtree",
        None,
        json!({"id":request.id,"expectedVersion":request.expected_version}),
    )?;
    Ok(())
}

pub fn direct_cloud_task_status_for_command(
    database: &Database,
    request: crate::tasks::TaskStatusRequest,
) -> Result<crate::tasks::TaskDto, String> {
    if let Some(occurrence_date) = request.occurrence_date.clone() {
        let response = direct_cloud_mutation::<DirectCloudOccurrenceStatusData>(
            database,
            "direct_cloud_recurrence_occurrence_set_status",
            None,
            json!({
                "taskId":request.id,
                "occurrenceDate":occurrence_date,
                "done":request.done,
                "taskExpectedVersion":request.expected_version,
                "occurrenceExpectedVersion":request.occurrence_expected_version,
            }),
        )?;
        // Match the local command: recurring completion returns the source task
        // with occurrence context rather than replacing it with the member instance.
        return Ok(response.data.task.into());
    }
    let response = direct_cloud_mutation::<DirectCloudTaskMutationData>(
        database,
        "direct_cloud_task_set_status",
        None,
        json!({"id":request.id,"expectedVersion":request.expected_version,"done":request.done}),
    )?;
    Ok(response.data.task.into())
}

pub fn direct_cloud_today_recurring_update_for_command(
    database: &Database,
    request: crate::tasks::TodayRecurringInstanceUpdateRequest,
) -> Result<crate::tasks::TodayRecurringInstanceUpdateResult, String> {
    let response = direct_cloud_mutation::<DirectCloudTodayRecurringUpdateData>(
        database,
        "direct_cloud_today_recurring_update",
        None,
        serde_json::to_value(request).map_err(|error| format!("CLOUD_REQUEST_INVALID: {error}"))?,
    )?;
    Ok(crate::tasks::TodayRecurringInstanceUpdateResult {
        instance: response.data.instance.into(),
        source: response.data.source.into(),
    })
}

pub fn direct_cloud_task_duplicate_for_command(
    database: &Database,
    request: crate::tasks::TaskDuplicateRequest,
) -> Result<crate::tasks::TaskDuplicateResult, String> {
    let response = direct_cloud_mutation::<DirectCloudTaskDuplicateData>(
        database,
        "direct_cloud_task_duplicate_subtree",
        None,
        json!({"id":request.id,"expectedVersion":request.expected_version}),
    )?;
    Ok(crate::tasks::TaskDuplicateResult {
        root_id: response.data.root_id,
        tasks: response.data.tasks.into_iter().map(Into::into).collect(),
    })
}

pub fn direct_cloud_task_plan_batch_apply_for_command(
    database: &Database,
    request: crate::tasks::TaskPlanBatchApplyRequest,
) -> Result<crate::tasks::TaskPlanBatchApplyResult, String> {
    let response = direct_cloud_mutation::<crate::tasks::TaskPlanBatchApplyResult>(
        database,
        "direct_cloud_task_plan_batch_apply",
        None,
        serde_json::to_value(request).map_err(|error| format!("CLOUD_REQUEST_INVALID: {error}"))?,
    )?;
    Ok(response.data)
}

pub fn direct_cloud_daily_estimate_set_for_command(
    database: &Database,
    request: crate::tasks::TaskDailyEstimateSetRequest,
    expected_daily_estimate_version: Option<i64>,
) -> Result<crate::tasks::TaskDailyEstimateDto, String> {
    let response = direct_cloud_mutation::<DirectCloudDailyEstimateData>(
        database,
        "direct_cloud_task_daily_estimate_set",
        None,
        json!({
            "taskId":request.task_id,
            "workDate":request.work_date,
            "estimateMinutes":request.estimate_minutes,
            "expectedVersion":expected_daily_estimate_version,
        }),
    )?;
    Ok(crate::tasks::TaskDailyEstimateDto {
        task_id: response.data.task_id,
        work_date: response.data.work_date,
        estimate_minutes: response
            .data
            .daily_estimate
            .map(|value| value.estimate_minutes),
    })
}

pub fn direct_cloud_daily_estimate_batch_set_for_command(
    database: &Database,
    request: crate::tasks::TaskDailyEstimateBatchSetRequest,
) -> Result<crate::tasks::TaskDailyEstimateBatchSetResult, String> {
    let response = direct_cloud_mutation::<DirectCloudChangedCountData>(
        database,
        "direct_cloud_task_daily_estimate_batch_set",
        None,
        serde_json::to_value(request).map_err(|error| format!("CLOUD_REQUEST_INVALID: {error}"))?,
    )?;
    Ok(crate::tasks::TaskDailyEstimateBatchSetResult {
        changed_count: response.data.changed_count,
    })
}

pub fn direct_cloud_recurring_estimate_set_for_command(
    database: &Database,
    request: crate::tasks::RecurringTaskEstimateSetRequest,
) -> Result<crate::tasks::TaskDto, String> {
    let response = direct_cloud_mutation::<DirectCloudDailyEstimateData>(
        database,
        "direct_cloud_task_daily_estimate_set",
        None,
        json!({
            "taskId":request.task_id,
            "workDate":request.work_date,
            "estimateMinutes":request.estimate_minutes,
            "expectedVersion":Value::Null,
            "expectedTaskVersion":request.expected_version,
            "updateRecurringEstimate":true,
        }),
    )?;
    let task_id = response.data.task_id;
    Ok(direct_cloud_task_detail(database, task_id)?
        .data
        .task
        .into())
}

pub fn direct_cloud_day_plan_set_for_command(
    database: &Database,
    request: crate::today_plans::TaskDayPlanSetRequest,
) -> Result<crate::today_plans::TaskDayPlanDto, String> {
    let response = direct_cloud_mutation::<DirectCloudDayPlanData>(
        database,
        "direct_cloud_task_day_plan_set",
        None,
        serde_json::to_value(request).map_err(|error| format!("CLOUD_REQUEST_INVALID: {error}"))?,
    )?;
    Ok(response.data.plan)
}

pub fn direct_cloud_day_plan_batch_set_for_command(
    database: &Database,
    request: crate::today_plans::TaskDayPlanBatchSetRequest,
) -> Result<crate::today_plans::TaskDayPlanBatchSetResult, String> {
    let response = direct_cloud_mutation::<DirectCloudChangedCountData>(
        database,
        "direct_cloud_task_day_plan_batch_set",
        None,
        serde_json::to_value(request).map_err(|error| format!("CLOUD_REQUEST_INVALID: {error}"))?,
    )?;
    Ok(crate::today_plans::TaskDayPlanBatchSetResult {
        changed_count: response.data.changed_count,
    })
}

pub fn direct_cloud_recurrence_save_for_command(
    database: &Database,
    request: crate::recurring::RecurrenceSaveRequest,
) -> Result<crate::recurring::RecurrenceRuleDto, String> {
    let response = direct_cloud_mutation::<DirectCloudRecurrenceData>(
        database,
        "direct_cloud_recurrence_save",
        None,
        serde_json::to_value(request).map_err(|error| format!("CLOUD_REQUEST_INVALID: {error}"))?,
    )?;
    Ok(response.data.rule)
}

pub fn direct_cloud_recurrence_close_for_command(
    database: &Database,
    request: crate::recurring::RecurrenceCloseRequest,
) -> Result<(), String> {
    direct_cloud_mutation::<Value>(
        database,
        "direct_cloud_recurrence_close",
        None,
        serde_json::to_value(request).map_err(|error| format!("CLOUD_REQUEST_INVALID: {error}"))?,
    )?;
    Ok(())
}

fn direct_cloud_timer_mutation_error(database: &Database, error: String) -> String {
    if !error.starts_with("LEASE_LOST:") {
        return error;
    }
    match direct_cloud_active_timer(database) {
        Ok(Some(entry)) => format!(
            "{error}（当前活动计时 {}，版本 {}，状态 {}）",
            entry.id, entry.version, entry.state
        ),
        Ok(None) => format!("{error}（当前没有活动计时）"),
        Err(_) => error,
    }
}

pub fn direct_cloud_timer_start(
    database: &Database,
    request: crate::time_tracking::TimerStartRequest,
) -> Result<crate::time_tracking::TimeEntryDto, String> {
    let context = direct_cloud_read_context(database)?;
    let lease_token = direct_tracking_lease_token(&context);
    let response = direct_cloud_mutation_with_context::<DirectCloudTimerEntryData>(
        database,
        &context,
        "direct_cloud_timer_start",
        Some(request.client_request_id.clone()),
        json!({
            "taskId": request.task_id,
            "note": request.note,
            "leaseToken": lease_token,
        }),
    )
    .map_err(|error| direct_cloud_timer_mutation_error(database, error))?;
    Ok(response.data.entry)
}

pub fn direct_cloud_timer_pause(
    database: &Database,
    request: crate::time_tracking::TimerVersionRequest,
) -> Result<crate::time_tracking::TimeEntryDto, String> {
    let context = direct_cloud_read_context(database)?;
    let lease_token = direct_tracking_lease_token(&context);
    let operation_id = direct_timer_operation_id(
        "direct_cloud_timer_pause",
        format!("{}:{}", request.entry_id, request.expected_version).as_bytes(),
    );
    let response = direct_cloud_mutation_with_context::<DirectCloudTimerEntryData>(
        database,
        &context,
        "direct_cloud_timer_pause",
        Some(operation_id),
        json!({
            "entryId": request.entry_id,
            "expectedVersion": request.expected_version,
            "leaseToken": lease_token,
        }),
    )
    .map_err(|error| direct_cloud_timer_mutation_error(database, error))?;
    Ok(response.data.entry)
}

pub fn direct_cloud_timer_resume(
    database: &Database,
    request: crate::time_tracking::TimerResumeRequest,
) -> Result<crate::time_tracking::TimeEntryDto, String> {
    let context = direct_cloud_read_context(database)?;
    let lease_token = direct_tracking_lease_token(&context);
    let response = direct_cloud_mutation_with_context::<DirectCloudTimerEntryData>(
        database,
        &context,
        "direct_cloud_timer_resume",
        Some(request.operation_id.clone()),
        json!({
            "entryId": request.entry_id,
            "expectedVersion": request.expected_version,
            "leaseToken": lease_token,
        }),
    )
    .map_err(|error| direct_cloud_timer_mutation_error(database, error))?;
    Ok(response.data.entry)
}

pub fn direct_cloud_timer_stop(
    database: &Database,
    request: crate::time_tracking::TimerStopRequest,
) -> Result<crate::time_tracking::TimerStopResultDto, String> {
    let context = direct_cloud_read_context(database)?;
    let lease_token = direct_tracking_lease_token(&context);
    let operation_id = direct_timer_operation_id(
        "direct_cloud_timer_stop",
        format!(
            "{}:{}:{}",
            request.entry_id, request.expected_version, request.create_default_allocation
        )
        .as_bytes(),
    );
    let response = direct_cloud_mutation_with_context::<DirectCloudTimerStopData>(
        database,
        &context,
        "direct_cloud_timer_stop",
        Some(operation_id),
        json!({
            "entryId": request.entry_id,
            "expectedVersion": request.expected_version,
            "createDefaultAllocation": request.create_default_allocation,
            "leaseToken": lease_token,
        }),
    )
    .map_err(|error| direct_cloud_timer_mutation_error(database, error))?;
    Ok(crate::time_tracking::TimerStopResultDto {
        current_entry: response.data.current_entry,
        slices: response.data.slices,
        total_duration_seconds: response.data.total_duration_seconds,
        total_settlement_minutes: response.data.total_settlement_minutes,
    })
}

pub fn direct_cloud_time_allocation_replace(
    database: &Database,
    request: crate::time_tracking::AllocationReplaceRequest,
) -> Result<crate::time_tracking::TimeEntryDto, String> {
    let payload = serde_json::to_value(&request)
        .map_err(|error| format!("CLOUD_REQUEST_INVALID: {error}"))?;
    let operation_id = direct_timer_operation_id(
        "direct_cloud_time_allocation_replace",
        payload.to_string().as_bytes(),
    );
    let response = direct_cloud_mutation::<DirectCloudTimerEntryData>(
        database,
        "direct_cloud_time_allocation_replace",
        Some(operation_id),
        payload,
    )?;
    Ok(response.data.entry)
}

pub fn direct_cloud_timer_transition(
    database: &Database,
    request: crate::time_tracking::TimerTransitionRequest,
) -> Result<crate::time_tracking::TimerTransitionResultDto, String> {
    let context = direct_cloud_read_context(database)?;
    let lease_token = direct_tracking_lease_token(&context);
    let mut payload = serde_json::to_value(&request)
        .map_err(|error| format!("CLOUD_REQUEST_INVALID: {error}"))?;
    payload["leaseToken"] = json!(lease_token);
    let response = direct_cloud_mutation_with_context::<DirectCloudTimerTransitionData>(
        database,
        &context,
        "direct_cloud_timer_transition",
        Some(request.client_request_id.clone()),
        payload,
    )
    .map_err(|error| direct_cloud_timer_mutation_error(database, error))?;
    Ok(crate::time_tracking::TimerTransitionResultDto {
        created_task: response.data.created_task,
        previous_stop_result: response.data.previous_stop_result,
        active_entry: response.data.active_entry,
        transition_at: response.data.transition_at,
        no_op: response.data.no_op,
    })
}

pub fn direct_cloud_timer_recover(
    database: &Database,
    request: crate::time_tracking::TimerRecoveryRequest,
) -> Result<crate::time_tracking::TimerRecoveryResultDto, String> {
    let context = direct_cloud_read_context(database)?;
    let lease_token = direct_tracking_lease_token(&context);
    let response = direct_cloud_mutation_with_context::<DirectCloudTimerRecoveryData>(
        database,
        &context,
        "direct_cloud_timer_recover",
        Some(request.client_request_id.clone()),
        json!({
            "entryId": request.entry_id,
            "expectedVersion": request.expected_version,
            "leaseToken": lease_token,
        }),
    )
    .map_err(|error| direct_cloud_timer_mutation_error(database, error))?;
    Ok(crate::time_tracking::TimerRecoveryResultDto {
        active_entry: response.data.active_entry,
        slices: response.data.slices,
        recovered: response.data.recovered,
        calendar_rollover: response.data.calendar_rollover,
        continuity_interrupted: response.data.continuity_interrupted,
        created_slice_count: response.data.created_slice_count,
        observed_at: response.data.observed_at,
        no_op: response.data.no_op,
    })
}

pub fn direct_cloud_timer_takeover(
    database: &Database,
    request: crate::time_tracking::TimerTakeoverRequest,
) -> Result<crate::time_tracking::TimerRecoveryResultDto, String> {
    let context = direct_cloud_read_context(database)?;
    let response = direct_cloud_mutation_with_context::<DirectCloudTimerRecoveryData>(
        database,
        &context,
        "direct_cloud_timer_takeover",
        Some(request.client_request_id.clone()),
        json!({
            "entryId": request.entry_id,
            "expectedVersion": request.expected_version,
            "leaseToken": request.lease_token,
        }),
    )
    .map_err(|error| direct_cloud_timer_mutation_error(database, error))?;
    store_direct_tracking_lease_token(&context, &request.lease_token);
    Ok(crate::time_tracking::TimerRecoveryResultDto {
        active_entry: response.data.active_entry,
        slices: response.data.slices,
        recovered: response.data.recovered,
        calendar_rollover: response.data.calendar_rollover,
        continuity_interrupted: response.data.continuity_interrupted,
        created_slice_count: response.data.created_slice_count,
        observed_at: response.data.observed_at,
        no_op: response.data.no_op,
    })
}

pub fn direct_cloud_manual_time_create(
    database: &Database,
    request: crate::time_tracking::ManualEntryRequest,
) -> Result<crate::time_tracking::TimeEntryDto, String> {
    let operation_id = request.client_request_id.clone();
    let response = direct_cloud_mutation::<DirectCloudTimerEntryData>(
        database,
        "direct_cloud_manual_time_create",
        Some(operation_id),
        serde_json::to_value(request).map_err(|error| format!("CLOUD_REQUEST_INVALID: {error}"))?,
    )?;
    Ok(response.data.entry)
}

pub fn direct_cloud_time_entry_update(
    database: &Database,
    request: crate::time_tracking::TimeEntryUpdateRequest,
) -> Result<crate::time_tracking::TimeEntryDto, String> {
    let payload = serde_json::to_value(&request)
        .map_err(|error| format!("CLOUD_REQUEST_INVALID: {error}"))?;
    let operation_id = direct_timer_operation_id(
        "direct_cloud_time_entry_update",
        payload.to_string().as_bytes(),
    );
    let response = direct_cloud_mutation::<DirectCloudTimerEntryData>(
        database,
        "direct_cloud_time_entry_update",
        Some(operation_id),
        payload,
    )?;
    Ok(response.data.entry)
}

pub fn direct_cloud_time_entry_disposition(
    database: &Database,
    request: crate::time_tracking::TimeEntryDispositionRequest,
) -> Result<crate::time_tracking::TimeEntryDto, String> {
    let payload = serde_json::to_value(&request)
        .map_err(|error| format!("CLOUD_REQUEST_INVALID: {error}"))?;
    let operation_id = direct_timer_operation_id(
        "direct_cloud_time_entry_disposition",
        payload.to_string().as_bytes(),
    );
    let response = direct_cloud_mutation::<DirectCloudTimerEntryData>(
        database,
        "direct_cloud_time_entry_disposition",
        Some(operation_id),
        payload,
    )?;
    Ok(response.data.entry)
}

pub fn direct_cloud_actual_adjustment_batch_apply(
    database: &Database,
    request: crate::actual_time_adjustments::ActualAdjustmentBatchRequest,
) -> Result<crate::actual_time_adjustments::ActualAdjustmentBatchResult, String> {
    let operation_id = request.client_request_id.clone();
    let response = direct_cloud_mutation::<
        crate::actual_time_adjustments::ActualAdjustmentBatchResult,
    >(
        database,
        "direct_cloud_actual_adjustment_batch_apply",
        Some(operation_id),
        serde_json::to_value(request).map_err(|error| format!("CLOUD_REQUEST_INVALID: {error}"))?,
    )?;
    Ok(response.data)
}

pub fn direct_cloud_actual_adjustment_delete(
    database: &Database,
    request: crate::actual_time_adjustments::ActualAdjustmentDeleteRequest,
) -> Result<crate::actual_time_adjustments::ActualTimeAdjustmentDto, String> {
    let operation_id = request.client_request_id.clone();
    let response = direct_cloud_mutation::<DirectCloudActualAdjustmentData>(
        database,
        "direct_cloud_actual_adjustment_delete",
        Some(operation_id),
        serde_json::to_value(request).map_err(|error| format!("CLOUD_REQUEST_INVALID: {error}"))?,
    )?;
    Ok(response.data.adjustment)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudWorkspaceSnapshotSettings {
    pub automatic_snapshot_enabled: bool,
    pub automatic_snapshot_time: String,
    pub automatic_snapshot_timezone: String,
    pub snapshot_retention_days: i64,
    pub next_automatic_snapshot_at: Option<String>,
    pub last_automatic_snapshot_at: Option<String>,
    pub scheduler_active: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudWorkspaceSnapshotSettingsUpdateRequest {
    pub automatic_snapshot_enabled: bool,
    pub automatic_snapshot_time: String,
    pub snapshot_retention_days: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudWorkspaceSnapshotCreateRequest {
    pub name: String,
    pub note: Option<String>,
    pub snapshot_type: Option<String>,
    pub lazy: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudWorkspaceSnapshotIdRequest {
    pub snapshot_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudWorkspaceSnapshotProtectRequest {
    pub snapshot_id: String,
    pub protected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudWorkspaceSnapshotDeleteResult {
    pub deleted: bool,
    pub snapshot_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudWorkspaceRestorePrepareResult {
    pub lock_token: String,
    pub target_snapshot_id: String,
    pub target_content_hash: String,
    pub prepared_epoch: i64,
    pub safety_snapshot_id: Option<String>,
    pub expires_at: String,
    pub preview: CloudWorkspaceSnapshotPreview,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudWorkspaceRestoreResult {
    pub restored: bool,
    pub target_snapshot_id: String,
    pub safety_snapshot_id: Option<String>,
    pub previous_epoch: i64,
    pub workspace_epoch: i64,
    pub reset_change_seq: i64,
    #[serde(default)]
    pub local_cache_rebuilt: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudWorkspaceRestoreCommitRequest {
    pub lock_token: String,
    pub target_snapshot_id: String,
    pub target_content_hash: String,
    pub prepared_epoch: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudWorkspaceRestorePrepareRequest {
    pub snapshot_id: String,
    #[serde(default)]
    pub create_safety_snapshot: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudWorkspaceRestoreCancelRequest {
    pub lock_token: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudConfigureRequest {
    pub project_url: String,
    pub anon_key: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CloudProjectProbe {
    pub state: String,
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudSignInRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudDeviceRegisterRequest {
    pub device_name: String,
    pub platform: String,
    pub app_version: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudDeviceRevokeRequest {
    pub device_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationPreviewRequest {
    pub direction: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationExecuteRequest {
    pub direction: String,
    pub confirmed: bool,
}

#[derive(Debug, Deserialize)]
struct AuthResponse {
    access_token: String,
    refresh_token: String,
    expires_in: Option<i64>,
    user: Option<AuthUser>,
}

#[derive(Debug, Deserialize)]
struct AuthUser {
    id: String,
    email: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct WorkspaceRow {
    id: String,
    name: String,
    timezone: String,
}

#[derive(Debug, Deserialize)]
struct ChangeSeqRow {
    change_seq: i64,
}

#[derive(Debug, Deserialize)]
struct DeviceRow {
    id: String,
    workspace_id: String,
    device_name: String,
    platform: String,
    app_version: String,
    last_seen_at: String,
    authorized_at: String,
    reauthorized_at: Option<String>,
    revoked_at: Option<String>,
}

/// Ordinary business edits always target the local SQLite cache first. In
/// cloud mode they are appended to `sync_outbox` by the owning command and
/// become authoritative only after Supabase accepts the operation.
pub fn ensure_repository_write_mode(database: &Database) -> Result<(), String> {
    let connection = database.open()?;
    let mode: String = connection
        .query_row(
            "SELECT COALESCE(json_extract(value_json, '$'), 'local') FROM device_settings WHERE key = 'storage_mode'",
            [],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| "local".to_string());
    if mode != "cloud" {
        return Ok(());
    }
    let workspace_id: Option<String> = connection
        .query_row(
            "SELECT json_extract(value_json, '$') FROM device_settings WHERE key = 'cloud_workspace_id'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let blocked = workspace_id
        .as_deref()
        .and_then(|workspace_id| {
            connection
                .query_row(
                    "SELECT auth_blocked, auth_blocked_reason FROM local_sync_state WHERE workspace_id = ?1",
                    [workspace_id],
                    |row| Ok((row.get::<_, bool>(0)?, row.get::<_, Option<String>>(1)?)),
                )
                .optional()
                .ok()
                .flatten()
        });
    if let Some((true, reason)) = blocked {
        return Err(format!(
            "AUTH_BLOCKED: {}",
            reason.unwrap_or_else(|| "云端授权已失效，请重新登录并确认待同步修改".to_string())
        ));
    }
    Ok(())
}

pub fn storage_mode(database: &Database) -> Result<StorageModeSnapshot, String> {
    let connection = database.open()?;
    storage_mode_from_connection(&connection, current_session(database).is_ok())
}

#[tauri::command]
pub fn direct_cloud_release_metrics_get(
    database: tauri::State<'_, Database>,
) -> Result<crate::direct_cloud_observability::DirectCloudReleaseMetricsSnapshot, String> {
    let state = storage_mode(&database)?;
    Ok(crate::direct_cloud_observability::snapshot(
        &state.workspace_id,
    ))
}

pub fn operation_history_list(
    database: &Database,
    request: CloudOperationHistoryListRequest,
) -> Result<CloudOperationHistoryPage, String> {
    let state = storage_mode(database)?;
    if state.mode != "cloud" {
        return Err("CLOUD_MODE_REQUIRED: 操作记录仅在云端模式可用".to_string());
    }
    let session = current_session(database)?;
    let value = client(database)?.rpc_for_database(
        database,
        "cloud_operation_history_list",
        json!({
            "p_workspace_id": state.workspace_id,
            "p_device_id": state.device_id,
            "p_limit": request.limit.unwrap_or(50).clamp(1, 100),
            "p_cursor_time": request.cursor.as_ref().map(|cursor| cursor.occurred_at.clone()),
            "p_cursor_id": request.cursor.as_ref().map(|cursor| cursor.id.clone()),
            "p_from": request.from,
            "p_to": request.to,
            "p_filter_device_id": request.device_id,
            "p_source": request.source,
            "p_business_type": request.business_type,
            "p_query": request.query,
        }),
        &session,
    )?;
    serde_json::from_value(value)
        .map_err(|error| format!("CLOUD_RESPONSE_INVALID: 操作记录响应无效: {error}"))
}

pub fn operation_history_detail(
    database: &Database,
    request: CloudOperationHistoryDetailRequest,
) -> Result<CloudOperationHistoryDetail, String> {
    let state = storage_mode(database)?;
    if state.mode != "cloud" {
        return Err("CLOUD_MODE_REQUIRED: 操作记录仅在云端模式可用".to_string());
    }
    let session = current_session(database)?;
    let value = client(database)?.rpc_for_database(
        database,
        "cloud_operation_history_detail",
        json!({
            "p_workspace_id": state.workspace_id,
            "p_device_id": state.device_id,
            "p_log_id": request.log_id,
        }),
        &session,
    )?;
    serde_json::from_value(value)
        .map_err(|error| format!("CLOUD_RESPONSE_INVALID: 操作记录详情无效: {error}"))
}

#[derive(Debug, Clone)]
struct CloudWorkspaceContext {
    workspace_id: String,
    device_id: String,
    workspace_epoch: i64,
}

fn cloud_workspace_context(
    database: &Database,
    capability: &str,
) -> Result<(CloudWorkspaceContext, CloudSession), String> {
    let connection = database.open()?;
    let mode = read_device_setting_string(&connection, "storage_mode")?
        .unwrap_or_else(|| "local".to_string());
    if mode != "cloud" {
        return Err(format!("CLOUD_MODE_REQUIRED: {capability}仅在云端模式可用"));
    }
    let workspace_id = read_device_setting_string(&connection, "cloud_workspace_id")?
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "CLOUD_DEVICE_REQUIRED: 请先完成云端设备注册".to_string())?;
    let device_id = read_device_setting_string(&connection, "device_id")?
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "CLOUD_DEVICE_REQUIRED: 请先完成云端设备注册".to_string())?;
    if workspace_id.trim().is_empty() || device_id.trim().is_empty() {
        return Err("CLOUD_DEVICE_REQUIRED: 请先完成云端设备注册".to_string());
    }
    let workspace_epoch = connection
        .query_row(
            "SELECT workspace_epoch FROM local_sync_state WHERE workspace_id=?1",
            [&workspace_id],
            |row| row.get(0),
        )
        .unwrap_or(1);
    drop(connection);
    let session = session(&workspace_id)?;
    Ok((
        CloudWorkspaceContext {
            workspace_id,
            device_id,
            workspace_epoch,
        },
        session,
    ))
}

#[derive(Debug, Clone)]
struct DirectCloudReadContext {
    workspace_id: String,
    device_id: String,
    workspace_epoch: i64,
    session: CloudSession,
}

fn read_device_setting_string(
    connection: &rusqlite::Connection,
    key: &str,
) -> Result<Option<String>, String> {
    connection
        .query_row(
            "SELECT json_extract(value_json, '$') FROM device_settings WHERE key=?1",
            [key],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())
}

fn direct_cloud_read_context(database: &Database) -> Result<DirectCloudReadContext, String> {
    let connection = database.open()?;
    let mode = read_device_setting_string(&connection, "storage_mode")?
        .unwrap_or_else(|| "local".to_string());
    if mode != "cloud" {
        return Err("CLOUD_MODE_REQUIRED: 云端查询仅在云端模式可用".to_string());
    }
    let protocol = read_device_setting_string(&connection, "cloud_storage_protocol")?
        .unwrap_or_else(|| crate::storage_contract::LEGACY_SYNC_PROTOCOL.to_string());
    if protocol != crate::storage_contract::DIRECT_CLOUD_PROTOCOL {
        return Err("CLOUD_PROTOCOL_UNSUPPORTED: 当前工作空间尚未启用直连云端存储".to_string());
    }
    let workspace_id = read_device_setting_string(&connection, "cloud_workspace_id")?
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "CLOUD_NOT_CONFIGURED: 尚未初始化 Supabase 工作空间".to_string())?;
    let device_id = connection
        .query_row(
            "SELECT id FROM devices WHERE revoked_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|_| "CLOUD_DEVICE_REQUIRED: 请先完成云端设备注册".to_string())?;
    let workspace_epoch = connection
        .query_row(
            "SELECT workspace_epoch FROM local_sync_state WHERE workspace_id=?1",
            [&workspace_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .unwrap_or(1);
    drop(connection);
    let session = match session(&workspace_id) {
        Ok(session) => session,
        Err(error) => {
            record_direct_cloud_access_error(&workspace_id, &error);
            return Err(error);
        }
    };
    Ok(DirectCloudReadContext {
        workspace_id: workspace_id.clone(),
        device_id,
        workspace_epoch,
        session,
    })
}

fn data_copy_target_context(database: &Database) -> Result<DirectCloudReadContext, String> {
    let connection = database.open()?;
    let protocol = read_device_setting_string(&connection, "cloud_storage_protocol")?
        .unwrap_or_else(|| crate::storage_contract::LEGACY_SYNC_PROTOCOL.to_string());
    if protocol != crate::storage_contract::DIRECT_CLOUD_PROTOCOL {
        return Err("CLOUD_PROTOCOL_UNSUPPORTED: 云端目标尚未启用直连存储协议".to_string());
    }
    let workspace_id = read_device_setting_string(&connection, "cloud_workspace_id")?
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "CLOUD_NOT_CONFIGURED: 尚未初始化 Supabase 工作空间".to_string())?;
    let device_id = read_device_setting_string(&connection, "device_id")?
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "CLOUD_DEVICE_REQUIRED: 请先完成云端设备注册".to_string())?;
    let workspace_epoch = connection
        .query_row(
            "SELECT workspace_epoch FROM local_sync_state WHERE workspace_id=?1",
            [&workspace_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .unwrap_or(1);
    drop(connection);
    let session = match session(&workspace_id) {
        Ok(session) => session,
        Err(error) => {
            record_direct_cloud_access_error(&workspace_id, &error);
            return Err(error);
        }
    };
    Ok(DirectCloudReadContext {
        workspace_id,
        device_id,
        workspace_epoch,
        session,
    })
}

fn direct_cloud_access_states() -> &'static Mutex<HashMap<String, DirectCloudAccessRuntime>> {
    DIRECT_CLOUD_ACCESS_STATES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn direct_cloud_access_runtime(workspace_id: &str) -> Option<DirectCloudAccessRuntime> {
    direct_cloud_access_states()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(workspace_id)
        .cloned()
}

fn set_direct_cloud_access_runtime(
    workspace_id: &str,
    state: DirectCloudAccessState,
    reason: Option<String>,
) {
    direct_cloud_access_states()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(
            workspace_id.to_string(),
            DirectCloudAccessRuntime { state, reason },
        );
}

fn record_direct_cloud_access_success(workspace_id: &str) {
    set_direct_cloud_access_runtime(workspace_id, DirectCloudAccessState::OnlineWritable, None);
}

fn record_direct_cloud_access_error(workspace_id: &str, error: &str) {
    let (state, reason) = direct_cloud_access_state_for_error(error);
    set_direct_cloud_access_runtime(workspace_id, state, Some(reason));
}

fn direct_cloud_access_state_for_error(error: &str) -> (DirectCloudAccessState, String) {
    if error.starts_with("AUTH_")
        || error.starts_with("DEVICE_REVOKED:")
        || error.starts_with("CREDENTIAL_UNAVAILABLE:")
    {
        return (
            DirectCloudAccessState::ReauthRequired,
            "云端授权已失效，请重新登录".to_string(),
        );
    }
    if error.starts_with("CLOUD_TIMEOUT:")
        || error.starts_with("EMERGENCY_READ_ONLY:")
        || error.starts_with("CLOUD_SERVICE_UNAVAILABLE:")
        || error.starts_with("CLOUD_REQUEST_FAILED:")
        || error.starts_with("CLOUD_RESPONSE_INVALID:")
        || error.starts_with("CLOUD_SCHEMA_")
        || error.starts_with("CLOUD_PERMISSION_DENIED:")
    {
        let code = error
            .split_once(':')
            .map(|(code, _)| code)
            .unwrap_or("CLOUD_ERROR");
        return (
            DirectCloudAccessState::DegradedReadonly,
            format!("云端请求异常，当前暂时只读（{code}）；刷新云端数据后会自动恢复"),
        );
    }
    (
        DirectCloudAccessState::OfflineReadonly,
        "当前无法连接 Supabase，云端数据暂时只读".to_string(),
    )
}

fn direct_cloud_query<T: for<'de> Deserialize<'de>>(
    database: &Database,
    function: &str,
    context: &DirectCloudReadContext,
    mut body: Value,
) -> Result<DirectCloudQueryResponse<T>, String> {
    let started_at = Instant::now();
    let body_object = body
        .as_object_mut()
        .ok_or_else(|| "CLOUD_REQUEST_INVALID: 云端查询参数必须是对象".to_string())?;
    body_object.insert("p_workspace_id".to_string(), json!(context.workspace_id));
    body_object.insert("p_device_id".to_string(), json!(context.device_id));
    body_object.insert(
        "p_expected_epoch".to_string(),
        json!(context.workspace_epoch),
    );
    let value = match client(database)?.rpc_for_database_with_epoch(
        database,
        function,
        body,
        &context.session,
        context.workspace_epoch,
    ) {
        Ok(value) => value,
        Err(error) => {
            record_direct_cloud_access_error(&context.workspace_id, &error);
            crate::direct_cloud_observability::record_query_error(&context.workspace_id, &error);
            return Err(error);
        }
    };
    let response: DirectCloudQueryResponse<T> = serde_json::from_value(value).map_err(|error| {
        let error = format!("CLOUD_RESPONSE_INVALID: {function} 响应无效: {error}");
        record_direct_cloud_access_error(&context.workspace_id, &error);
        crate::direct_cloud_observability::record_query_error(&context.workspace_id, &error);
        error
    })?;
    record_direct_cloud_checkpoint(database, &response.metadata)?;
    if response.metadata.writable == Some(false) {
        set_direct_cloud_access_runtime(
            &context.workspace_id,
            DirectCloudAccessState::DegradedReadonly,
            response
                .metadata
                .read_only_reason
                .clone()
                .or_else(|| Some("Supabase 已启用紧急只读，业务修改暂时停用".to_string())),
        );
    } else {
        record_direct_cloud_access_success(&context.workspace_id);
    }
    crate::direct_cloud_observability::record_query(
        &context.workspace_id,
        started_at.elapsed().as_secs_f64() * 1000.0,
    );
    Ok(response)
}

pub fn direct_cloud_mutation<T: for<'de> Deserialize<'de>>(
    database: &Database,
    function: &str,
    operation_id: Option<String>,
    payload: Value,
) -> Result<DirectCloudMutationResponse<T>, String> {
    let context = direct_cloud_read_context(database)?;
    direct_cloud_mutation_with_context(database, &context, function, operation_id, payload)
}

fn ensure_direct_cloud_writable(database: &Database, workspace_id: &str) -> Result<(), String> {
    let connection = database.open()?;
    let mode = read_device_setting_string(&connection, "storage_mode")?
        .unwrap_or_else(|| "local".to_string());
    let protocol = read_device_setting_string(&connection, "cloud_storage_protocol")?
        .unwrap_or_else(|| crate::storage_contract::LEGACY_SYNC_PROTOCOL.to_string());
    drop(connection);
    if mode != "cloud" || protocol != crate::storage_contract::DIRECT_CLOUD_PROTOCOL {
        return Err("CLOUD_MODE_REQUIRED: 当前操作需要直连云端存储".to_string());
    }
    let runtime = direct_cloud_access_runtime(workspace_id).unwrap_or(DirectCloudAccessRuntime {
        state: DirectCloudAccessState::Connecting,
        reason: Some("正在连接 Supabase 并加载云端数据".to_string()),
    });
    if runtime.state.writable() {
        return Ok(());
    }
    Err(format!(
        "CLOUD_READ_ONLY: {}",
        runtime
            .reason
            .unwrap_or_else(|| "Supabase 当前不可写，请刷新云端数据后重试".to_string())
    ))
}

fn direct_cloud_mutation_with_context<T: for<'de> Deserialize<'de>>(
    database: &Database,
    context: &DirectCloudReadContext,
    function: &str,
    operation_id: Option<String>,
    payload: Value,
) -> Result<DirectCloudMutationResponse<T>, String> {
    if let Err(error) = ensure_direct_cloud_writable(database, &context.workspace_id) {
        crate::direct_cloud_observability::record_mutation_error(&context.workspace_id, &error);
        return Err(error);
    }
    let operation_id = operation_id
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| Uuid::now_v7().to_string());
    Uuid::parse_str(&operation_id)
        .map_err(|_| "VALIDATION_ERROR: operationId 必须是 UUID".to_string())?;
    let body = json!({
        "p_workspace_id": context.workspace_id,
        "p_device_id": context.device_id,
        "p_operation_id": operation_id,
        "p_expected_epoch": context.workspace_epoch,
        "p_payload": payload,
    });
    let api = client(database)?;
    let value = match api.rpc_for_database_with_epoch(
        database,
        function,
        body.clone(),
        &context.session,
        context.workspace_epoch,
    ) {
        Ok(value) => value,
        Err(error)
            if error.starts_with("NETWORK_ERROR:")
                || error.starts_with("CLOUD_TIMEOUT:")
                || error.starts_with("CLOUD_SERVICE_UNAVAILABLE:") =>
        {
            match api.rpc_for_database_with_epoch(
                database,
                function,
                body,
                &context.session,
                context.workspace_epoch,
            ) {
                Ok(value) => value,
                Err(retry_error) => {
                    record_direct_cloud_access_error(&context.workspace_id, &retry_error);
                    crate::direct_cloud_observability::record_mutation_error(
                        &context.workspace_id,
                        &retry_error,
                    );
                    return Err(retry_error);
                }
            }
        }
        Err(error) => {
            record_direct_cloud_access_error(&context.workspace_id, &error);
            crate::direct_cloud_observability::record_mutation_error(&context.workspace_id, &error);
            return Err(error);
        }
    };
    let response: DirectCloudMutationResponse<T> =
        serde_json::from_value(value).map_err(|error| {
            let error = format!("CLOUD_RESPONSE_INVALID: {function} 响应无效: {error}");
            record_direct_cloud_access_error(&context.workspace_id, &error);
            crate::direct_cloud_observability::record_mutation_error(&context.workspace_id, &error);
            error
        })?;
    record_direct_cloud_checkpoint(database, &response.metadata)?;
    record_direct_cloud_access_success(&context.workspace_id);
    crate::direct_cloud_observability::record_mutation(&context.workspace_id);
    Ok(response)
}

fn record_direct_cloud_checkpoint(
    database: &Database,
    metadata: &DirectCloudQueryMetadata,
) -> Result<(), String> {
    if metadata.workspace_id.trim().is_empty() {
        return Ok(());
    }
    database
        .open()?
        .execute(
            "INSERT INTO local_sync_state(workspace_id, device_id, last_change_seq, last_full_sync_at, workspace_epoch)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(workspace_id) DO UPDATE SET
               device_id=excluded.device_id,
               last_change_seq=CASE
                 WHEN local_sync_state.workspace_epoch <> excluded.workspace_epoch THEN excluded.last_change_seq
                 ELSE MAX(local_sync_state.last_change_seq, excluded.last_change_seq)
               END,
               last_full_sync_at=excluded.last_full_sync_at,
               last_error=NULL,
               workspace_epoch=excluded.workspace_epoch,
               epoch_blocked=0,
               epoch_blocked_reason=NULL,
               epoch_blocked_at=NULL",
            params![
                metadata.workspace_id,
                metadata.device_id,
                metadata.change_seq,
                now_seconds() * 1_000,
                metadata.workspace_epoch
            ],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn direct_tracking_lease_key(context: &DirectCloudReadContext) -> (String, String) {
    (context.workspace_id.clone(), context.device_id.clone())
}

fn direct_tracking_lease_token(context: &DirectCloudReadContext) -> String {
    let tokens = DIRECT_TRACKING_LEASE_TOKENS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = tokens
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard
        .entry(direct_tracking_lease_key(context))
        .or_insert_with(|| Uuid::now_v7().to_string())
        .clone()
}

fn store_direct_tracking_lease_token(context: &DirectCloudReadContext, token: &str) {
    let tokens = DIRECT_TRACKING_LEASE_TOKENS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = tokens
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.insert(direct_tracking_lease_key(context), token.to_string());
}

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

pub fn direct_cloud_external_binding_delete_mutation(
    database: &Database,
    operation_id: String,
    payload: Value,
) -> Result<Value, String> {
    direct_cloud_json_mutation(
        database,
        "direct_cloud_external_binding_delete",
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
    #[derive(Deserialize)]
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

pub fn workspace_snapshot_create(
    database: &Database,
    request: CloudWorkspaceSnapshotCreateRequest,
) -> Result<CloudWorkspaceSnapshot, String> {
    let (state, session) = cloud_workspace_context(database, "云端快照")?;
    let snapshot_type = request
        .snapshot_type
        .unwrap_or_else(|| "manual".to_string());
    if !matches!(
        snapshot_type.as_str(),
        "manual" | "automatic" | "pre_restore"
    ) {
        return Err("VALIDATION_ERROR: 不支持的镜像类型".to_string());
    }
    let name = request.name.trim();
    if name.is_empty() || name.chars().count() > 120 {
        return Err("VALIDATION_ERROR: 镜像名称应为 1 到 120 个字符".to_string());
    }
    let value = client(database)?.rpc_for_database_with_timeout(
        database,
        "workspace_snapshot_create",
        json!({
            "p_workspace_id": state.workspace_id,
            "p_device_id": state.device_id,
            "p_operation_id": Uuid::now_v7().to_string(),
            "p_snapshot_type": snapshot_type,
            "p_name": name,
            "p_note": request.note.and_then(|note| {
                let note = note.trim().to_string();
                (!note.is_empty()).then_some(note)
            }),
            "p_lazy": request.lazy.unwrap_or(false),
        }),
        &session,
        SNAPSHOT_TIMEOUT_SECONDS,
    )?;
    serde_json::from_value(value)
        .map_err(|error| format!("CLOUD_RESPONSE_INVALID: 镜像创建响应无效: {error}"))
}

pub fn workspace_snapshot_list(database: &Database) -> Result<Vec<CloudWorkspaceSnapshot>, String> {
    let (state, session) = cloud_workspace_context(database, "云端快照")?;
    let value = client(database)?.rpc_for_database(
        database,
        "workspace_snapshot_list",
        json!({"p_workspace_id": state.workspace_id, "p_device_id": state.device_id}),
        &session,
    )?;
    serde_json::from_value(value)
        .map_err(|error| format!("CLOUD_RESPONSE_INVALID: 镜像列表响应无效: {error}"))
}

pub fn workspace_snapshot_set_protected(
    database: &Database,
    request: CloudWorkspaceSnapshotProtectRequest,
) -> Result<CloudWorkspaceSnapshot, String> {
    let (state, session) = cloud_workspace_context(database, "云端快照")?;
    let value = client(database)?.rpc_for_database(
        database,
        "workspace_snapshot_set_protected",
        json!({
            "p_workspace_id": state.workspace_id,
            "p_device_id": state.device_id,
            "p_snapshot_id": request.snapshot_id,
            "p_protected": request.protected,
        }),
        &session,
    )?;
    serde_json::from_value(value)
        .map_err(|error| format!("CLOUD_RESPONSE_INVALID: 镜像保护响应无效: {error}"))
}

pub fn workspace_snapshot_delete(
    database: &Database,
    request: CloudWorkspaceSnapshotIdRequest,
) -> Result<CloudWorkspaceSnapshotDeleteResult, String> {
    let (state, session) = cloud_workspace_context(database, "云端快照")?;
    let value = client(database)?.rpc_for_database(
        database,
        "workspace_snapshot_delete",
        json!({
            "p_workspace_id": state.workspace_id,
            "p_device_id": state.device_id,
            "p_snapshot_id": request.snapshot_id,
        }),
        &session,
    )?;
    serde_json::from_value(value)
        .map_err(|error| format!("CLOUD_RESPONSE_INVALID: 镜像删除响应无效: {error}"))
}

pub fn workspace_snapshot_preview(
    database: &Database,
    request: CloudWorkspaceSnapshotIdRequest,
) -> Result<CloudWorkspaceSnapshotPreview, String> {
    let (state, session) = cloud_workspace_context(database, "云端快照")?;
    let value = client(database)?.rpc_for_database(
        database,
        "workspace_snapshot_preview",
        json!({
            "p_workspace_id": state.workspace_id,
            "p_device_id": state.device_id,
            "p_snapshot_id": request.snapshot_id,
        }),
        &session,
    )?;
    serde_json::from_value(value)
        .map_err(|error| format!("CLOUD_RESPONSE_INVALID: 快照详情响应无效: {error}"))
}

fn validate_automatic_snapshot_time(value: &str) -> Result<&str, String> {
    let trimmed = value.trim();
    let mut parts = trimmed.split(':');
    let hour = parts.next().and_then(|part| part.parse::<u32>().ok());
    let minute = parts.next().and_then(|part| part.parse::<u32>().ok());
    let valid = trimmed.len() == 5
        && trimmed.as_bytes()[2] == b':'
        && parts.next().is_none()
        && hour.is_some_and(|hour| hour < 24)
        && minute.is_some_and(|minute| minute < 60);
    if valid {
        Ok(trimmed)
    } else {
        Err("VALIDATION_ERROR: 自动快照时间格式必须为 HH:MM".to_string())
    }
}

fn validate_snapshot_retention_days(value: i64) -> Result<i64, String> {
    if (1..=365).contains(&value) {
        Ok(value)
    } else {
        Err("VALIDATION_ERROR: 快照最长保留天数必须在 1 到 365 天之间".to_string())
    }
}

#[cfg(test)]
mod snapshot_setting_tests {
    use super::{validate_automatic_snapshot_time, validate_snapshot_retention_days};

    #[test]
    fn accepts_only_24_hour_clock_times() {
        assert_eq!(validate_automatic_snapshot_time("00:00").unwrap(), "00:00");
        assert_eq!(validate_automatic_snapshot_time("03:00").unwrap(), "03:00");
        assert_eq!(validate_automatic_snapshot_time("23:59").unwrap(), "23:59");
        assert!(validate_automatic_snapshot_time("24:00").is_err());
        assert!(validate_automatic_snapshot_time("03:60").is_err());
        assert!(validate_automatic_snapshot_time("3:00").is_err());
        assert!(validate_automatic_snapshot_time("03:00:00").is_err());
        assert!(validate_automatic_snapshot_time("daily").is_err());
    }

    #[test]
    fn accepts_snapshot_retention_between_one_and_365_days() {
        assert_eq!(validate_snapshot_retention_days(1).unwrap(), 1);
        assert_eq!(validate_snapshot_retention_days(7).unwrap(), 7);
        assert_eq!(validate_snapshot_retention_days(365).unwrap(), 365);
        assert!(validate_snapshot_retention_days(0).is_err());
        assert!(validate_snapshot_retention_days(366).is_err());
    }
}

pub fn workspace_snapshot_settings_get(
    database: &Database,
) -> Result<CloudWorkspaceSnapshotSettings, String> {
    let (state, session) = cloud_workspace_context(database, "云端快照")?;
    let value = client(database)?.rpc_for_database(
        database,
        "workspace_snapshot_settings_get",
        json!({"p_workspace_id": state.workspace_id, "p_device_id": state.device_id}),
        &session,
    )?;
    serde_json::from_value(value)
        .map_err(|error| format!("CLOUD_RESPONSE_INVALID: 快照设置响应无效: {error}"))
}

pub fn workspace_snapshot_settings_update(
    database: &Database,
    request: CloudWorkspaceSnapshotSettingsUpdateRequest,
) -> Result<CloudWorkspaceSnapshotSettings, String> {
    let (state, session) = cloud_workspace_context(database, "云端快照")?;
    let time = validate_automatic_snapshot_time(&request.automatic_snapshot_time)?;
    let retention_days = validate_snapshot_retention_days(request.snapshot_retention_days)?;
    let value = client(database)?.rpc_for_database(
        database,
        "workspace_snapshot_settings_update",
        json!({
            "p_workspace_id": state.workspace_id,
            "p_device_id": state.device_id,
            "p_operation_id": Uuid::now_v7().to_string(),
            "p_automatic_snapshot_enabled": request.automatic_snapshot_enabled,
            "p_automatic_snapshot_time": time,
            "p_snapshot_retention_days": retention_days,
        }),
        &session,
    )?;
    serde_json::from_value(value)
        .map_err(|error| format!("CLOUD_RESPONSE_INVALID: 快照设置保存响应无效: {error}"))
}

fn ensure_local_restore_gate(database: &Database, workspace_id: &str) -> Result<(), String> {
    let connection = database.open()?;
    let dirty: i64 = connection.query_row(
        "SELECT COUNT(*) FROM sync_outbox WHERE workspace_id=?1 AND state IN ('pending','sending','failed','conflict')",
        [workspace_id], |row| row.get(0),
    ).map_err(|error| error.to_string())?;
    if dirty > 0 {
        return Err(
            "SYNC_PENDING: 当前设备仍有待同步、失败或冲突操作，不能开始云端回滚".to_string(),
        );
    }
    let active_timer: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM time_entries WHERE state IN ('running','paused') AND deleted_at IS NULL)", [], |row| row.get(0),
    ).map_err(|error| error.to_string())?;
    if active_timer {
        return Err("RESTORE_ACTIVE_TIMER: 请先结束当前活动计时".to_string());
    }
    Ok(())
}

fn uses_direct_cloud_protocol(database: &Database) -> Result<bool, String> {
    let connection = database.open()?;
    Ok(
        read_device_setting_string(&connection, "cloud_storage_protocol")?.as_deref()
            == Some(crate::storage_contract::DIRECT_CLOUD_PROTOCOL),
    )
}

fn record_direct_restore_checkpoint(
    database: &Database,
    workspace_id: &str,
    device_id: &str,
    workspace_epoch: i64,
    reset_change_seq: i64,
) -> Result<(), String> {
    let connection = database.open()?;
    connection
        .execute(
            "INSERT INTO local_sync_state(
                 workspace_id,device_id,last_change_seq,last_full_sync_at,last_error,
                 auth_blocked,auth_blocked_reason,auth_blocked_at,workspace_epoch,
                 epoch_blocked,epoch_blocked_reason,epoch_blocked_at
               ) VALUES(?1,?2,?3,NULL,NULL,0,NULL,NULL,?4,0,NULL,NULL)
               ON CONFLICT(workspace_id) DO UPDATE SET
                 device_id=excluded.device_id,
                 last_change_seq=excluded.last_change_seq,
                 last_full_sync_at=NULL,
                 last_error=NULL,
                 auth_blocked=0,
                 auth_blocked_reason=NULL,
                 auth_blocked_at=NULL,
                 workspace_epoch=excluded.workspace_epoch,
                 epoch_blocked=0,
                 epoch_blocked_reason=NULL,
                 epoch_blocked_at=NULL",
            params![workspace_id, device_id, reset_change_seq, workspace_epoch],
        )
        .map_err(|error| {
            format!("RESTORE_CHECKPOINT_UPDATE_FAILED: 无法保存恢复后的云端世代: {error}")
        })?;
    record_direct_cloud_access_success(workspace_id);
    Ok(())
}

pub fn workspace_restore_prepare(
    database: &Database,
    request: CloudWorkspaceRestorePrepareRequest,
) -> Result<CloudWorkspaceRestorePrepareResult, String> {
    let (state, session) = cloud_workspace_context(database, "云端回滚")?;
    if !uses_direct_cloud_protocol(database)? {
        ensure_local_restore_gate(database, &state.workspace_id)?;
    }
    let direct_cloud = uses_direct_cloud_protocol(database)?;
    let function = if direct_cloud {
        "direct_cloud_workspace_restore_prepare"
    } else {
        "workspace_restore_prepare"
    };
    let value = client(database)?.rpc_for_database_with_epoch_and_timeout(database, function, json!({
        "p_workspace_id":state.workspace_id,"p_device_id":state.device_id,"p_target_snapshot_id":request.snapshot_id,"p_expected_epoch":state.workspace_epoch,
        "p_create_safety_snapshot":request.create_safety_snapshot
    }), &session, state.workspace_epoch, RESTORE_TIMEOUT_SECONDS)?;
    serde_json::from_value(value)
        .map_err(|error| format!("CLOUD_RESPONSE_INVALID: 回滚准备响应无效: {error}"))
}

pub fn workspace_restore_commit(
    database: &Database,
    request: CloudWorkspaceRestoreCommitRequest,
) -> Result<CloudWorkspaceRestoreResult, String> {
    crate::cloud_sync::with_cloud_sync_operation_lock(|| {
        workspace_restore_commit_locked(database, request)
    })
}

fn workspace_restore_commit_locked(
    database: &Database,
    request: CloudWorkspaceRestoreCommitRequest,
) -> Result<CloudWorkspaceRestoreResult, String> {
    let (state, session) = cloud_workspace_context(database, "云端回滚")?;
    let direct_cloud = uses_direct_cloud_protocol(database)?;
    if !direct_cloud {
        ensure_local_restore_gate(database, &state.workspace_id)?;
    }
    let function = if direct_cloud {
        "direct_cloud_workspace_restore_commit"
    } else {
        "workspace_restore_commit"
    };
    let value = client(database)?.rpc_for_database_with_epoch_and_timeout(database, function, json!({
        "p_workspace_id":state.workspace_id,"p_device_id":state.device_id,"p_lock_token":request.lock_token,
        "p_target_snapshot_id":request.target_snapshot_id,"p_target_content_hash":request.target_content_hash,
        "p_prepared_epoch":request.prepared_epoch,"p_operation_id":Uuid::now_v7().to_string(),"p_confirmed":true
    }), &session, request.prepared_epoch, RESTORE_TIMEOUT_SECONDS)?;
    let mut restored: CloudWorkspaceRestoreResult = serde_json::from_value(value)
        .map_err(|error| format!("CLOUD_RESPONSE_INVALID: 回滚提交响应无效: {error}"))?;
    if !restored.restored {
        return Err("CLOUD_RESPONSE_INVALID: 云端没有确认恢复成功".to_string());
    }
    if direct_cloud {
        record_direct_restore_checkpoint(
            database,
            &state.workspace_id,
            &state.device_id,
            restored.workspace_epoch,
            restored.reset_change_seq,
        )?;
        restored.local_cache_rebuilt = false;
        return Ok(restored);
    }
    crate::cloud_sync::rebuild_local_cache_after_workspace_restore_locked(
        database,
        restored.workspace_epoch,
        restored.reset_change_seq,
    )
    .map_err(|error| {
        format!("RESTORE_LOCAL_REBUILD_FAILED: 云端已恢复成功，但当前设备重新下载失败：{error}")
    })?;
    restored.local_cache_rebuilt = true;
    Ok(restored)
}

pub fn workspace_restore_cancel(
    database: &Database,
    request: CloudWorkspaceRestoreCancelRequest,
) -> Result<Value, String> {
    let (state, session) = cloud_workspace_context(database, "云端回滚")?;
    client(database)?.rpc_for_database_with_epoch(database, "workspace_restore_cancel", json!({
        "p_workspace_id":state.workspace_id,"p_device_id":state.device_id,"p_lock_token":request.lock_token
    }), &session, state.workspace_epoch)
}

pub(crate) fn storage_mode_from_connection(
    connection: &rusqlite::Connection,
    online: bool,
) -> Result<StorageModeSnapshot, String> {
    let local_workspace_id: String = connection
        .query_row(
            "SELECT id FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let device_id: String = connection
        .query_row(
            "SELECT id FROM devices WHERE workspace_id = ?1 AND revoked_at IS NULL ORDER BY created_at LIMIT 1",
            [&local_workspace_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let mode: String = connection
        .query_row(
            "SELECT json_extract(value_json, '$') FROM device_settings WHERE key = 'storage_mode'",
            [],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| "local".to_string());
    let configured_cloud_protocol = connection
        .query_row(
            "SELECT json_extract(value_json, '$') FROM device_settings WHERE key = 'cloud_storage_protocol'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap_or_else(|_| crate::storage_contract::LEGACY_SYNC_PROTOCOL.to_string());
    let storage_protocol = if mode == "cloud" {
        configured_cloud_protocol.clone()
    } else {
        "local".to_string()
    };
    let workspace_id = if mode == "cloud" {
        connection
            .query_row(
                "SELECT json_extract(value_json, '$') FROM device_settings WHERE key = 'cloud_workspace_id'",
                [],
                |row| row.get(0),
            )
            .map_err(|_| "CLOUD_NOT_CONFIGURED: 尚未初始化 Supabase 工作空间".to_string())?
    } else {
        local_workspace_id.clone()
    };
    let sync_workspace_id = workspace_id.clone();
    let (pending, conflicts): (i64, i64) = connection
        .query_row(
            "SELECT
                (SELECT COUNT(*) FROM sync_outbox WHERE workspace_id = ?1 AND state IN ('pending', 'sending', 'failed')),
                (SELECT COUNT(*) FROM sync_outbox WHERE workspace_id = ?1 AND state = 'conflict')",
            [&sync_workspace_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| error.to_string())?;
    let (last_change_seq, last_synced_at, last_error, workspace_epoch, epoch_blocked, epoch_blocked_reason): (i64, Option<i64>, Option<String>, i64, bool, Option<String>) = connection
        .query_row(
            "SELECT last_change_seq, last_full_sync_at, last_error, workspace_epoch, epoch_blocked, epoch_blocked_reason FROM local_sync_state WHERE workspace_id = ?1",
            [&sync_workspace_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
        )
        .unwrap_or((0, None, None, 1, false, None));
    let cloud_workspace_id: Option<String> = connection
        .query_row(
            "SELECT json_extract(value_json, '$') FROM device_settings WHERE key = 'cloud_workspace_id'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let (auth_blocked, auth_blocked_reason) = cloud_workspace_id
        .as_deref()
        .and_then(|cloud_workspace_id| {
            connection
                .query_row(
                    "SELECT auth_blocked, auth_blocked_reason FROM local_sync_state WHERE workspace_id = ?1",
                    [cloud_workspace_id],
                    |row| Ok((row.get::<_, bool>(0)?, row.get::<_, Option<String>>(1)?)),
                )
                .optional()
                .ok()
                .flatten()
        })
        .unwrap_or((false, None));
    let protocol_supported = matches!(
        storage_protocol.as_str(),
        "local"
            | crate::storage_contract::LEGACY_SYNC_PROTOCOL
            | crate::storage_contract::DIRECT_CLOUD_PROTOCOL
    );
    let direct_cloud =
        mode == "cloud" && storage_protocol == crate::storage_contract::DIRECT_CLOUD_PROTOCOL;
    let runtime_access = direct_cloud
        .then(|| direct_cloud_access_runtime(&workspace_id))
        .flatten();
    let (cloud_access_state, cloud_writable, read_only_reason) = if mode == "local" {
        ("local_writable".to_string(), true, None)
    } else if !protocol_supported {
        (
            "protocol_incompatible".to_string(),
            false,
            Some("当前云端存储协议不受支持，请升级应用".to_string()),
        )
    } else if auth_blocked {
        (
            "reauth_required".to_string(),
            false,
            Some(
                auth_blocked_reason
                    .clone()
                    .unwrap_or_else(|| "云端授权已失效，请重新登录".to_string()),
            ),
        )
    } else if epoch_blocked {
        (
            "degraded_readonly".to_string(),
            false,
            Some(
                epoch_blocked_reason
                    .clone()
                    .unwrap_or_else(|| "云端工作空间世代已变化".to_string()),
            ),
        )
    } else if direct_cloud && !online {
        let state = runtime_access
            .as_ref()
            .map(|runtime| runtime.state)
            .filter(|state| *state == DirectCloudAccessState::ReauthRequired)
            .unwrap_or(DirectCloudAccessState::OfflineReadonly);
        let reason = runtime_access
            .as_ref()
            .and_then(|runtime| runtime.reason.clone())
            .or_else(|| Some("当前无法连接 Supabase，云端数据暂时只读".to_string()));
        (state.as_str().to_string(), state.writable(), reason)
    } else if direct_cloud {
        let runtime = runtime_access.unwrap_or(DirectCloudAccessRuntime {
            state: DirectCloudAccessState::Connecting,
            reason: Some("正在连接 Supabase 并加载云端数据".to_string()),
        });
        (
            runtime.state.as_str().to_string(),
            runtime.state.writable(),
            runtime.reason,
        )
    } else if online {
        ("legacy_sync".to_string(), true, None)
    } else {
        (
            "legacy_sync_offline".to_string(),
            true,
            Some("当前使用兼容同步协议，修改会保存在本机等待同步".to_string()),
        )
    };
    Ok(StorageModeSnapshot {
        online: mode == "local" || online,
        sync_state: if epoch_blocked {
            "epoch_blocked".to_string()
        } else if conflicts > 0 {
            "conflict".to_string()
        } else if pending > 0 {
            "pending".to_string()
        } else if last_error.is_some() {
            "error".to_string()
        } else {
            "synced".to_string()
        },
        storage_backend: if mode == "local" {
            "local_sqlite".to_string()
        } else if direct_cloud {
            "direct_supabase".to_string()
        } else {
            "legacy_sync_sqlite".to_string()
        },
        mode,
        storage_protocol,
        configured_cloud_protocol,
        cloud_access_state,
        cloud_writable,
        read_only_reason,
        workspace_id,
        device_id,
        pending_operations: pending,
        conflict_count: conflicts,
        last_change_seq,
        last_synced_at,
        last_error,
        auth_blocked,
        auth_blocked_reason,
        workspace_epoch,
        epoch_blocked,
        epoch_blocked_reason,
    })
}

pub fn configure(database: &Database, request: CloudConfigureRequest) -> Result<(), String> {
    let project_url = validate_project_url(&request.project_url)?;
    let anon_key = request.anon_key.trim();
    if anon_key.is_empty() {
        return Err("VALIDATION_ERROR: Supabase anon key 不能为空".to_string());
    }
    if is_service_role_key(anon_key) {
        return Err("SECURITY_ERROR: 不允许把 Supabase service_role key 放入桌面应用".to_string());
    }
    let replacing_existing_connection =
        cloud_configuration_is_changing(database, &project_url, anon_key)?;
    ensure_can_replace_cloud_configuration(database, &project_url, anon_key)?;
    let http = Client::builder()
        .timeout(std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECONDS))
        .build()
        .map_err(|error| format!("NETWORK_ERROR: 无法初始化 Supabase 客户端: {error}"))?;
    let response = send_auth_settings_request(&http, &project_url, anon_key)?;
    if !response.status().is_success() {
        return Err(map_http_error(response, "CLOUD_NOT_CONFIGURED"));
    }
    settings::update_setting(
        database,
        SettingsUpdate {
            scope: SettingsScope::Device,
            key: "supabase_project_url".to_string(),
            value: json!(project_url),
        },
    )?;
    settings::update_setting(
        database,
        SettingsUpdate {
            scope: SettingsScope::Device,
            key: "supabase_anon_key".to_string(),
            value: json!(anon_key),
        },
    )?;
    if replacing_existing_connection {
        clear_all_saved_sessions(database)?;
    }
    Ok(())
}

pub const CLOUD_PROJECT_READY: &str = "ready";
pub const CLOUD_PROJECT_UNCONFIGURED: &str = "unconfigured";
pub const CLOUD_PROJECT_SCHEMA_MISSING: &str = "schema_missing";
pub const CLOUD_PROJECT_SCHEMA_NOT_EXPOSED: &str = "schema_not_exposed";
pub const CLOUD_PROJECT_UNREACHABLE: &str = "unreachable";

/// Checks whether the connected Supabase project already has the TimeGenie
/// schema, using only the anon key. The desktop client never holds DDL rights,
/// so an uninitialized project can only be reported to the user, not repaired by
/// the app itself.
pub fn probe_cloud_project(database: &Database) -> Result<CloudProjectProbe, String> {
    let client = match client(database) {
        Ok(client) => client,
        Err(error) if error.starts_with("CLOUD_NOT_CONFIGURED") => {
            return Ok(CloudProjectProbe {
                state: CLOUD_PROJECT_UNCONFIGURED.to_string(),
                message: "尚未配置 Supabase 项目地址和 anon key".to_string(),
            });
        }
        Err(error) => return Err(error),
    };
    let url = format!(
        "{}/rest/v1/workspaces?select=id&limit=1",
        client.project_url
    );
    let mut request = client
        .http
        .get(url)
        .header("apikey", &client.anon_key)
        .bearer_auth(&client.anon_key)
        .header("Accept-Profile", CLOUD_SCHEMA);
    if let Ok(session) = current_session(database) {
        request = request.bearer_auth(session.access_token);
    }
    let response = request.send();
    match response {
        Ok(response) => {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            Ok(classify_probe_response(status, &body))
        }
        Err(error) => Ok(CloudProjectProbe {
            state: CLOUD_PROJECT_UNREACHABLE.to_string(),
            message: map_supabase_request_error(error),
        }),
    }
}

fn classify_probe_response(status: StatusCode, body: &str) -> CloudProjectProbe {
    let classified = classify_http_error(status, body, "CLOUD_REQUEST_FAILED");
    let state = if classified.starts_with("CLOUD_SCHEMA_MISSING") {
        CLOUD_PROJECT_SCHEMA_MISSING
    } else if classified.starts_with("CLOUD_SCHEMA_NOT_EXPOSED") {
        CLOUD_PROJECT_SCHEMA_NOT_EXPOSED
    } else if classified.starts_with("NETWORK_ERROR")
        || classified.starts_with("CLOUD_TIMEOUT")
        || classified.starts_with("CLOUD_SERVICE_UNAVAILABLE")
    {
        CLOUD_PROJECT_UNREACHABLE
    } else {
        CLOUD_PROJECT_READY
    };
    let message = if state == CLOUD_PROJECT_READY {
        "云端项目已初始化，可以登录并同步".to_string()
    } else {
        classified
    };
    CloudProjectProbe {
        state: state.to_string(),
        message,
    }
}

pub fn sign_in_password(
    database: &Database,
    request: CloudSignInRequest,
) -> Result<CloudSessionSnapshot, String> {
    if request.email.trim().is_empty() || request.password.is_empty() {
        return Err("AUTH_FAILED: 邮箱和密码不能为空".to_string());
    }
    let client = client(database)?;
    let response = client.auth_token(
        "password",
        json!({ "email": request.email.trim(), "password": request.password }),
    )?;
    if !response.status().is_success() {
        return Err(map_auth_error(response, "AUTH_FAILED"));
    }
    let auth: AuthResponse = response
        .json()
        .map_err(|error| format!("AUTH_FAILED: Supabase 登录响应无效: {error}"))?;
    let user = auth
        .user
        .ok_or_else(|| "AUTH_FAILED: 登录响应缺少用户信息".to_string())?;
    let session = CloudSession {
        user_id: user.id,
        email: user.email,
        access_token: auth.access_token,
        refresh_token: auth.refresh_token,
        expires_at: auth.expires_in.map(|seconds| now_seconds() + seconds),
    };
    save_session(database, &session)?;
    let _ = workspace_bootstrap_with_session(database, &client, &session)?;
    crate::cloud_sync::wake_after_connection_ready();
    Ok(session_snapshot(&session))
}

pub fn sign_out(database: &Database) -> Result<(), String> {
    sign_out_with_scope(database, false)
}

pub fn sign_out_all(database: &Database) -> Result<(), String> {
    sign_out_with_scope(database, true)
}

fn sign_out_with_scope(database: &Database, all_devices: bool) -> Result<(), String> {
    let session = current_session(database)?;
    let state = storage_mode(database)?;
    let api = client(database)?;
    let cloud_workspace_id: Option<String> = database
        .open()?
        .query_row(
            "SELECT json_extract(value_json, '$') FROM device_settings WHERE key = 'cloud_workspace_id'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if let Some(cloud_workspace_id) = cloud_workspace_id {
        if all_devices {
            api.rpc_for_database(
                database,
                "device_revoke_all",
                json!({ "p_workspace_id": cloud_workspace_id }),
                &session,
            )?;
        } else {
            let authorization = api.rpc_for_database(
                database,
                "device_authorization_get",
                json!({ "p_workspace_id": cloud_workspace_id, "p_device_id": state.device_id }),
                &session,
            )?;
            if authorization.get("authorized").and_then(Value::as_bool) == Some(true) {
                api.rpc_for_database(
                    database,
                    "device_revoke",
                    json!({ "p_workspace_id": cloud_workspace_id, "p_device_id": state.device_id }),
                    &session,
                )?;
            }
        }
    }
    let logout_result = api.auth_logout(&session, if all_devices { "global" } else { "local" });
    clear_saved_session_for_workspace(database, &local_workspace_id(database)?)?;
    mark_auth_blocked(
        database,
        if all_devices {
            "all_devices_signed_out"
        } else {
            "current_device_signed_out"
        },
    )?;
    settings::update_setting(
        database,
        SettingsUpdate {
            scope: SettingsScope::Device,
            key: "storage_mode".to_string(),
            value: json!("local"),
        },
    )?;
    logout_result
}

fn ensure_no_pending_cloud_work(database: &Database) -> Result<(), String> {
    let state = storage_mode(database)?;
    if state.mode == "cloud" && (state.pending_operations > 0 || state.conflict_count > 0) {
        return Err(
            "OFFLINE_RESTRICTED: 请先同步或处理云端冲突，再退出账号或切换到本地模式".to_string(),
        );
    }
    Ok(())
}

fn ensure_can_replace_cloud_configuration(
    database: &Database,
    project_url: &str,
    anon_key: &str,
) -> Result<(), String> {
    let connection = database.open()?;
    let mode: String = connection
        .query_row(
            "SELECT json_extract(value_json, '$') FROM device_settings WHERE key = 'storage_mode'",
            [],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| "local".to_string());
    if mode != "cloud" {
        return Ok(());
    }
    drop(connection);

    if cloud_configuration_is_changing(database, project_url, anon_key)? {
        ensure_no_pending_cloud_work(database).map_err(|error| {
            if error.starts_with("OFFLINE_RESTRICTED:") {
                "OFFLINE_RESTRICTED: 请先同步或处理云端冲突，再更换 Supabase 项目或公开 key"
                    .to_string()
            } else {
                error
            }
        })?;
    }
    Ok(())
}

fn cloud_configuration_is_changing(
    database: &Database,
    project_url: &str,
    anon_key: &str,
) -> Result<bool, String> {
    let connection = database.open()?;
    let current_project_url: Option<String> = connection
        .query_row(
            "SELECT json_extract(value_json, '$') FROM device_settings WHERE key = 'supabase_project_url'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let current_anon_key: Option<String> = connection
        .query_row(
            "SELECT json_extract(value_json, '$') FROM device_settings WHERE key = 'supabase_anon_key'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    Ok(current_project_url
        .as_deref()
        .is_some_and(|current| current != project_url)
        || current_anon_key
            .as_deref()
            .is_some_and(|current| current != anon_key))
}

fn send_auth_settings_request(
    http: &Client,
    project_url: &str,
    anon_key: &str,
) -> Result<reqwest::blocking::Response, String> {
    let url = format!("{project_url}/auth/v1/settings");
    let response = http
        .get(&url)
        .header("apikey", anon_key)
        .send()
        .map_err(|error| format!("NETWORK_ERROR: 无法连接 Supabase 项目: {error}"))?;
    if matches!(
        response.status(),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
    ) {
        return http
            .get(url)
            .header("apikey", anon_key)
            .bearer_auth(anon_key)
            .send()
            .map_err(|error| format!("NETWORK_ERROR: 无法连接 Supabase 项目: {error}"));
    }
    Ok(response)
}

pub fn session_snapshot_for_database(database: &Database) -> Result<CloudSessionSnapshot, String> {
    match current_session(database) {
        Ok(session) => match validate_remote_session(database, &session) {
            Ok(()) => match validate_current_device_authorization(database, &session) {
                Ok(()) => Ok(session_snapshot(&session)),
                Err(error) if should_preserve_saved_session(&error) => {
                    offline_session_snapshot(database, session_reason_code(&error))
                }
                Err(error) => Ok(reauth_required_snapshot(session_reason_code(&error))),
            },
            Err(error) if should_preserve_saved_session(&error) => {
                offline_session_snapshot(database, session_reason_code(&error))
            }
            Err(error) => Ok(reauth_required_snapshot(session_reason_code(&error))),
        },
        Err(error) if should_preserve_saved_session(&error) => Ok(offline_session_snapshot(
            database,
            session_reason_code(&error),
        )?),
        Err(error) => Ok(reauth_required_snapshot(session_reason_code(&error))),
    }
}

fn validate_current_device_authorization(
    database: &Database,
    session: &CloudSession,
) -> Result<(), String> {
    let state = storage_mode(database)?;
    if state.mode != "cloud" {
        return Ok(());
    }
    let api = client(database)?;
    let response = api
        .post(format!(
            "{}/rest/v1/rpc/device_authorization_get",
            api.project_url
        ))
        .bearer_auth(&session.access_token)
        .json(&json!({ "p_workspace_id": state.workspace_id, "p_device_id": state.device_id }))
        .send()
        .map_err(|error| format!("NETWORK_ERROR: Supabase 设备授权校验失败: {error}"))?;
    if !response.status().is_success() {
        let error = map_http_error(response, "CLOUD_REQUEST_FAILED");
        return Err(if error.starts_with("AUTH_FAILED:") {
            error.replacen("AUTH_FAILED:", "AUTH_UNVERIFIED:", 1)
        } else {
            error
        });
    }
    let value: Value = response
        .json()
        .map_err(|error| format!("NETWORK_ERROR: 设备授权响应无效: {error}"))?;
    if value.get("authorized").and_then(Value::as_bool) == Some(true) {
        return Ok(());
    }
    clear_saved_session_for_workspace(database, &local_workspace_id(database)?)?;
    mark_auth_blocked(database, "device_revoked")?;
    Err("DEVICE_REVOKED: 当前设备的云端授权已被撤销，请重新登录".to_string())
}

fn validate_remote_session(database: &Database, session: &CloudSession) -> Result<(), String> {
    let api = client(database)?;
    let response = api
        .http
        .get(format!("{}/auth/v1/user", api.project_url))
        .header("apikey", api.anon_key())
        .bearer_auth(&session.access_token)
        .send()
        .map_err(|error| format!("NETWORK_ERROR: Supabase 会话校验失败: {error}"))?;
    if response.status().is_success() {
        return Ok(());
    }
    if matches!(
        response.status(),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
    ) {
        return refresh_current_session(database).map(|_| ());
    }
    Err(map_http_error(response, "AUTH_UNVERIFIED"))
}

pub fn workspace_bootstrap(database: &Database) -> Result<CloudWorkspace, String> {
    let session = current_session(database)?;
    let client = client(database)?;
    workspace_bootstrap_with_session(database, &client, &session)
}

fn workspace_bootstrap_with_session(
    database: &Database,
    client: &SupabaseClient,
    session: &CloudSession,
) -> Result<CloudWorkspace, String> {
    let mut workspace = read_workspace_with_session(database, client, session)?;
    let workspace_already_existed = workspace.is_some();
    if workspace.is_none() {
        let response = client.request_for_database(
            database,
            client
                .post(format!("{}/rest/v1/workspaces", client.project_url))
                .header("Prefer", "return=representation")
                .json(&json!({
                    "owner_user_id": session.user_id,
                    "name": "我的工作台",
                    "timezone": "Asia/Shanghai"
                })),
            session,
        )?;
        let created: Vec<WorkspaceRow> = response
            .json::<Vec<WorkspaceRow>>()
            .map_err(|error| format!("NETWORK_ERROR: 创建 Supabase 工作空间失败: {error}"))?;
        workspace = created.into_iter().next();
    }
    let workspace =
        workspace.ok_or_else(|| "VERSION_CONFLICT: Supabase 未返回可用工作空间".to_string())?;
    let initialize_result = client.rpc_for_database(
        database,
        "workspace_initialize_defaults",
        json!({ "p_workspace_id": workspace.id.clone() }),
        session,
    );
    if let Err(error) = initialize_result {
        // Older cloud projects can have the named report-template columns but
        // still expose a pre-upgrade bootstrap function that inserts templates
        // without `name`. Existing workspaces already have their business data,
        // so allow them to continue to snapshot download until the cloud SQL
        // patch is deployed. A newly-created workspace must still initialize
        // successfully or it would be left incomplete.
        if !(workspace_already_existed && is_legacy_report_template_bootstrap_error(&error)) {
            return Err(error);
        }
    }
    settings::update_setting(
        database,
        SettingsUpdate {
            scope: SettingsScope::Device,
            key: "cloud_workspace_id".to_string(),
            value: json!(workspace.id),
        },
    )?;
    let latest_change_seq = latest_change_seq(database, client, session, &workspace.id)?;
    Ok(CloudWorkspace {
        id: workspace.id,
        name: workspace.name,
        timezone: workspace.timezone,
        latest_change_seq,
    })
}

fn is_legacy_report_template_bootstrap_error(error: &str) -> bool {
    let normalized = error.to_ascii_lowercase();
    normalized.contains("null value in column \"name\"")
        && normalized.contains("report_templates")
        && normalized.contains("not-null constraint")
}

fn read_workspace_with_session(
    database: &Database,
    client: &SupabaseClient,
    session: &CloudSession,
) -> Result<Option<WorkspaceRow>, String> {
    let workspaces: Vec<WorkspaceRow> = client
        .request_for_database(
            database,
            client
                .get(format!(
                    "{}/rest/v1/workspaces?owner_user_id=eq.{}&deleted_at=is.null&select=id,name,timezone&limit=1",
                    client.project_url, session.user_id
                )),
            session,
        )?
        .json()
        .map_err(|error| format!("NETWORK_ERROR: 读取 Supabase 工作空间失败: {error}"))?;
    Ok(workspaces.into_iter().next())
}

fn workspace_preview(database: &Database) -> Result<Option<CloudWorkspace>, String> {
    let session = current_session(database)?;
    let client = client(database)?;
    let Some(workspace) = read_workspace_with_session(database, &client, &session)? else {
        return Ok(None);
    };
    let latest_change_seq = latest_change_seq(database, &client, &session, &workspace.id)?;
    Ok(Some(CloudWorkspace {
        id: workspace.id,
        name: workspace.name,
        timezone: workspace.timezone,
        latest_change_seq,
    }))
}

pub fn register_device(
    database: &Database,
    request: CloudDeviceRegisterRequest,
) -> Result<CloudDevice, String> {
    let session = current_session(database)?;
    let workspace = workspace_bootstrap(database)?;
    let connection = database.open()?;
    let device_id: String = connection
        .query_row(
            "SELECT json_extract(value_json, '$') FROM device_settings WHERE key = 'device_id'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let client = client(database)?;
    let value = client.rpc_for_database(
        database,
        "device_authorize",
        json!({
            "p_workspace_id": workspace.id,
            "p_device_id": device_id,
            "p_device_name": request.device_name.trim(),
            "p_platform": request.platform,
            "p_app_version": request.app_version
        }),
        &session,
    )?;
    let device: DeviceRow = serde_json::from_value(value)
        .map_err(|error| format!("NETWORK_ERROR: 注册设备响应无效: {error}"))?;
    Ok(CloudDevice {
        id: device.id,
        workspace_id: device.workspace_id,
        device_name: device.device_name,
        platform: device.platform,
        app_version: device.app_version,
        last_seen_at: device.last_seen_at,
        authorized_at: device.authorized_at,
        reauthorized_at: device.reauthorized_at,
        revoked_at: device.revoked_at,
        current: true,
    })
}

pub fn list_devices(database: &Database) -> Result<Vec<CloudDevice>, String> {
    let session = current_session(database)?;
    let state = storage_mode(database)?;
    let value = client(database)?.rpc_for_database(
        database,
        "device_list",
        json!({ "p_workspace_id": state.workspace_id }),
        &session,
    )?;
    let rows: Vec<DeviceRow> = serde_json::from_value(value)
        .map_err(|error| format!("NETWORK_ERROR: 设备列表响应无效: {error}"))?;
    Ok(rows
        .into_iter()
        .map(|row| cloud_device_from_row(row, &state.device_id))
        .collect())
}

fn cloud_device_from_row(row: DeviceRow, current_device_id: &str) -> CloudDevice {
    CloudDevice {
        current: row.id == current_device_id,
        id: row.id,
        workspace_id: row.workspace_id,
        device_name: row.device_name,
        platform: row.platform,
        app_version: row.app_version,
        last_seen_at: row.last_seen_at,
        authorized_at: row.authorized_at,
        reauthorized_at: row.reauthorized_at,
        revoked_at: row.revoked_at,
    }
}

pub fn revoke_device(database: &Database, device_id: &str) -> Result<Vec<CloudDevice>, String> {
    let session = current_session(database)?;
    let state = storage_mode(database)?;
    client(database)?.rpc_for_database(
        database,
        "device_revoke",
        json!({ "p_workspace_id": state.workspace_id, "p_device_id": device_id }),
        &session,
    )?;
    if device_id == state.device_id {
        let _ = client(database)?.auth_logout(&session, "local");
        clear_saved_session_for_workspace(database, &local_workspace_id(database)?)?;
        mark_auth_blocked(database, "device_revoked")?;
        settings::update_setting(
            database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "storage_mode".to_string(),
                value: json!("local"),
            },
        )?;
        return Ok(Vec::new());
    }
    list_devices(database)
}

pub(crate) fn ensure_current_device_authorized(database: &Database) -> Result<(), String> {
    let state = storage_mode(database)?;
    if state.mode != "cloud" {
        return Ok(());
    }
    let session = current_session(database)?;
    let value = client(database)?.rpc_for_database(
        database,
        "device_authorization_get",
        json!({ "p_workspace_id": state.workspace_id, "p_device_id": state.device_id }),
        &session,
    )?;
    if value.get("authorized").and_then(Value::as_bool) == Some(true) {
        if auth_blocked_reason(database)?.is_some() {
            return Err(
                "AUTH_RESUME_REQUIRED: 登录已恢复，请确认如何处理此前保留的待同步修改".to_string(),
            );
        }
        return Ok(());
    }
    let _ = client(database)?.auth_logout(&session, "local");
    clear_saved_session_for_workspace(database, &local_workspace_id(database)?)?;
    mark_auth_blocked(database, "device_revoked")?;
    Err("DEVICE_REVOKED: 当前设备的云端授权已被撤销，请重新登录".to_string())
}

fn legacy_queue_counts(
    connection: &rusqlite::Connection,
    workspace_id: &str,
) -> Result<LegacyQueueCounts, String> {
    connection
        .query_row(
            "SELECT
               SUM(CASE WHEN state='pending' THEN 1 ELSE 0 END),
               SUM(CASE WHEN state='sending' THEN 1 ELSE 0 END),
               SUM(CASE WHEN state='failed' THEN 1 ELSE 0 END),
               SUM(CASE WHEN state='conflict' THEN 1 ELSE 0 END),
               COUNT(*)
             FROM sync_outbox
             WHERE workspace_id=?1",
            [workspace_id],
            |row| {
                Ok(LegacyQueueCounts {
                    pending: row.get::<_, Option<i64>>(0)?.unwrap_or(0),
                    sending: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                    failed: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    conflict: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                    total: row.get(4)?,
                })
            },
        )
        .map_err(|error| error.to_string())
}

pub fn storage_protocol_upgrade_preflight(
    database: &Database,
) -> Result<StorageProtocolUpgradePreflight, String> {
    let connection = database.open()?;
    let workspace_id = read_device_setting_string(&connection, "cloud_workspace_id")?
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "CLOUD_NOT_CONFIGURED: 尚未初始化 Supabase 工作空间".to_string())?;
    let local_protocol = read_device_setting_string(&connection, "cloud_storage_protocol")?
        .unwrap_or_else(|| crate::storage_contract::LEGACY_SYNC_PROTOCOL.to_string());
    let device_id = read_device_setting_string(&connection, "device_id")?
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "CLOUD_DEVICE_REQUIRED: 请先完成云端设备注册".to_string())?;
    let local_workspace_epoch = connection
        .query_row(
            "SELECT workspace_epoch FROM local_sync_state WHERE workspace_id=?1",
            [&workspace_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .unwrap_or(1);
    let queue = legacy_queue_counts(&connection, &workspace_id)?;
    drop(connection);

    let session = current_session(database)?;
    let server: StorageProtocolUpgradeServerPreflight =
        serde_json::from_value(client(database)?.rpc_for_database(
            database,
            "storage_protocol_upgrade_preflight",
            json!({ "p_workspace_id": workspace_id, "p_device_id": device_id }),
            &session,
        )?)
        .map_err(|error| format!("CLOUD_RESPONSE_INVALID: 协议升级预检响应无效: {error}"))?;
    if server.workspace_id != workspace_id || server.device_id != device_id {
        return Err("CLOUD_RESPONSE_INVALID: 协议升级预检工作空间或设备不一致".to_string());
    }
    let devices = list_devices(database)?;
    Ok(build_storage_protocol_upgrade_preflight(
        server,
        local_protocol,
        local_workspace_epoch,
        devices,
        queue,
    ))
}

fn build_storage_protocol_upgrade_preflight(
    server: StorageProtocolUpgradeServerPreflight,
    local_protocol: String,
    local_workspace_epoch: i64,
    devices: Vec<CloudDevice>,
    queue: LegacyQueueCounts,
) -> StorageProtocolUpgradePreflight {
    let mut blocked_reasons = Vec::new();
    if server.workspace_epoch != local_workspace_epoch {
        blocked_reasons.push(format!(
            "本机工作空间世代为 {local_workspace_epoch}，云端为 {}，请先完成最后一次旧协议刷新",
            server.workspace_epoch
        ));
    }
    if queue.total > 0 {
        blocked_reasons.push(format!(
            "当前设备仍有 {} 条旧同步操作（等待 {}、发送中 {}、失败 {}、冲突 {}）",
            queue.total, queue.pending, queue.sending, queue.failed, queue.conflict
        ));
    }
    if !server.server_upgrade_allowed && !server.already_direct {
        blocked_reasons.push("云端当前协议不允许升级到 direct_v2".to_string());
    }
    let can_upgrade_with_drained_queue = server.server_upgrade_allowed
        && server.workspace_epoch == local_workspace_epoch
        && queue.total == 0;
    StorageProtocolUpgradePreflight {
        workspace_id: server.workspace_id,
        device_id: server.device_id,
        storage_protocol: server.storage_protocol,
        local_protocol,
        workspace_epoch: server.workspace_epoch,
        local_workspace_epoch,
        server_upgrade_allowed: server.server_upgrade_allowed,
        already_direct: server.already_direct,
        requires_local_queue_disposition: server.requires_local_queue_disposition,
        allowed_queue_dispositions: server.allowed_queue_dispositions,
        devices,
        queue,
        can_upgrade_with_drained_queue,
        blocked_reasons,
        server_time: server.server_time,
    }
}

fn ensure_upgrade_preflight_can_drain(
    preflight: &StorageProtocolUpgradePreflight,
) -> Result<(), String> {
    if preflight.already_direct {
        return Err(
            "CLOUD_PROTOCOL_UNSUPPORTED: 云端工作空间已启用直连协议，请重新打开应用加载直连数据"
                .to_string(),
        );
    }
    if preflight.storage_protocol != crate::storage_contract::LEGACY_SYNC_PROTOCOL
        || !preflight.server_upgrade_allowed
    {
        return Err("CLOUD_PROTOCOL_UNSUPPORTED: 云端当前协议不允许执行旧队列升级".to_string());
    }
    if preflight.local_protocol != crate::storage_contract::LEGACY_SYNC_PROTOCOL {
        return Err("CLOUD_PROTOCOL_UNSUPPORTED: 本机当前协议不允许执行旧队列升级".to_string());
    }
    Ok(())
}

fn run_legacy_queue_upgrade_flow<D, R, S>(
    initial: &StorageProtocolUpgradePreflight,
    drain: D,
    recheck: R,
    switch: S,
) -> Result<
    (
        StorageProtocolUpgradeServerResult,
        StorageProtocolUpgradePreflight,
        crate::cloud_sync::LegacyQueueDrainResult,
    ),
    String,
>
where
    D: FnOnce() -> Result<crate::cloud_sync::LegacyQueueDrainResult, String>,
    R: FnOnce() -> Result<StorageProtocolUpgradePreflight, String>,
    S: FnOnce(
        &StorageProtocolUpgradePreflight,
    ) -> Result<StorageProtocolUpgradeServerResult, String>,
{
    ensure_upgrade_preflight_can_drain(initial)?;
    let drained = drain()?;
    let refreshed = recheck()?;
    ensure_upgrade_preflight_can_drain(&refreshed)?;
    if refreshed.workspace_id != initial.workspace_id || refreshed.device_id != initial.device_id {
        return Err("CLOUD_RESPONSE_INVALID: 升级预检工作空间或设备发生变化".to_string());
    }
    if refreshed.queue.total != 0 {
        return Err(format!(
            "LEGACY_QUEUE_BLOCKED: 最终刷新后仍有 {} 条旧同步操作，协议未切换",
            refreshed.queue.total
        ));
    }
    if refreshed.workspace_epoch != refreshed.local_workspace_epoch {
        return Err(format!(
            "WORKSPACE_EPOCH_STALE: 最终刷新后本机世代 {} 与云端 {} 不一致，协议未切换",
            refreshed.local_workspace_epoch, refreshed.workspace_epoch
        ));
    }
    let switched = switch(&refreshed)?;
    let reported_protocol = switched.storage_protocol.as_deref().unwrap_or_else(|| {
        if switched.already_direct {
            crate::storage_contract::DIRECT_CLOUD_PROTOCOL
        } else {
            ""
        }
    });
    if !switched.upgraded && !switched.already_direct {
        return Err("CLOUD_RESPONSE_INVALID: 云端未确认存储协议切换".to_string());
    }
    if reported_protocol != crate::storage_contract::DIRECT_CLOUD_PROTOCOL {
        return Err("CLOUD_RESPONSE_INVALID: 云端返回的存储协议不是 direct_v2".to_string());
    }
    if switched.workspace_epoch != refreshed.workspace_epoch {
        return Err(format!(
            "WORKSPACE_EPOCH_STALE: 切换期间云端世代从 {} 变为 {}，请重新预检",
            refreshed.workspace_epoch, switched.workspace_epoch
        ));
    }
    Ok((switched, refreshed, drained))
}

fn apply_legacy_queue_upgrade_result(
    database: &Database,
    result: &StorageProtocolUpgradeServerResult,
    drained: &crate::cloud_sync::LegacyQueueDrainResult,
    preflight: &StorageProtocolUpgradePreflight,
) -> Result<StorageProtocolUpgradeResult, String> {
    let final_change_seq = result
        .change_seq
        .unwrap_or(drained.last_change_seq)
        .max(drained.last_change_seq);
    persist_direct_protocol_state(
        database,
        &preflight.workspace_id,
        result.workspace_epoch,
        final_change_seq,
    )?;
    Ok(StorageProtocolUpgradeResult {
        upgraded: result.upgraded,
        already_direct: result.already_direct,
        workspace_epoch: result.workspace_epoch,
        storage_protocol: crate::storage_contract::DIRECT_CLOUD_PROTOCOL.to_string(),
        change_seq: result.change_seq,
        safety_snapshot_id: result.safety_snapshot_id.clone(),
        pushed_operations: drained.pushed,
        last_change_seq: final_change_seq,
        refreshed_domains: drained.domains.clone(),
    })
}

fn persist_direct_protocol_state(
    database: &Database,
    workspace_id: &str,
    workspace_epoch: i64,
    final_change_seq: i64,
) -> Result<(), String> {
    let connection = database.open()?;
    connection
        .execute(
            "INSERT INTO device_settings(key,value_json,updated_at) VALUES(?1,?2,1)
             ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json,updated_at=excluded.updated_at",
            params![
                "cloud_storage_protocol",
                serde_json::to_string(crate::storage_contract::DIRECT_CLOUD_PROTOCOL)
                    .map_err(|error| error.to_string())?
            ],
        )
        .map_err(|error| format!("LOCAL_PROTOCOL_UPDATE_FAILED: 无法保存直连协议: {error}"))?;
    connection
        .execute(
            "UPDATE local_sync_state
             SET workspace_epoch=?2, last_change_seq=MAX(last_change_seq, ?3),
                 epoch_blocked=0, epoch_blocked_reason=NULL, last_error=NULL
             WHERE workspace_id=?1",
            params![workspace_id, workspace_epoch, final_change_seq],
        )
        .map_err(|error| format!("LOCAL_PROTOCOL_UPDATE_FAILED: 无法保存直连协议状态: {error}"))?;
    set_direct_cloud_access_runtime(
        workspace_id,
        DirectCloudAccessState::Connecting,
        Some("存储协议已切换，正在加载云端权威数据".to_string()),
    );
    crate::cloud_sync::enter_direct_cloud_mode();
    Ok(())
}

pub fn storage_protocol_upgrade_with_drained_queue(
    database: &Database,
    request: StorageProtocolUpgradeRequest,
) -> Result<StorageProtocolUpgradeResult, String> {
    Uuid::parse_str(request.operation_id.trim())
        .map_err(|_| "VALIDATION_ERROR: 协议升级操作标识无效".to_string())?;
    let initial = storage_protocol_upgrade_preflight(database)?;
    ensure_upgrade_preflight_can_drain(&initial)?;
    let operation_id = request.operation_id;
    let create_safety_snapshot = request.create_safety_snapshot.unwrap_or(true);
    crate::cloud_sync::drain_legacy_queue_then(database, |drained| {
        let (switched, refreshed, drained) = run_legacy_queue_upgrade_flow(
            &initial,
            || Ok(drained.clone()),
            || storage_protocol_upgrade_preflight(database),
            |preflight| {
                let session = current_session(database)?;
                let value = client(database)?.rpc_for_database(
                    database,
                    "upgrade_storage_protocol",
                    json!({
                        "p_workspace_id": preflight.workspace_id,
                        "p_device_id": preflight.device_id,
                        "p_operation_id": operation_id,
                        "p_expected_epoch": preflight.workspace_epoch,
                        "p_old_sync_disposition": "queue_drained",
                        "p_create_safety_snapshot": create_safety_snapshot,
                    }),
                    &session,
                )?;
                serde_json::from_value(value)
                    .map_err(|error| format!("CLOUD_RESPONSE_INVALID: 协议升级响应无效: {error}"))
            },
        )?;
        apply_legacy_queue_upgrade_result(database, &switched, &drained, &refreshed)
    })
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn quarantine_legacy_queue(
    database: &Database,
    workspace_id: &str,
    rescue_backup_checksum: &str,
    reason: &str,
) -> Result<(String, i64), String> {
    let mut connection = database.open()?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let count: i64 = transaction
        .query_row(
            "SELECT COUNT(*) FROM sync_outbox WHERE workspace_id=?1 AND state IN ('pending','sending','failed','conflict')",
            [workspace_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if count == 0 {
        return Err("LEGACY_QUEUE_EMPTY: 当前设备没有可放弃的旧同步操作".to_string());
    }
    let batch_id = Uuid::now_v7().to_string();
    let quarantined_at = now_millis();
    let inserted = transaction
        .execute(
            "INSERT INTO legacy_abandoned_outbox(
                operation_id,workspace_id,device_id,operation_type,entity_type,entity_id,
                base_version,payload_json,state,attempt_count,created_at,last_attempt_at,
                error_json,payload_version,depends_on_operation_id,coalesced_count,
                recovery_kind,recovery_context_json,recovery_updated_at,workspace_epoch,
                quarantine_batch_id,quarantine_reason,rescue_backup_checksum,quarantined_at
             )
             SELECT operation_id,workspace_id,device_id,operation_type,entity_type,entity_id,
                    base_version,payload_json,state,attempt_count,created_at,last_attempt_at,
                    error_json,payload_version,depends_on_operation_id,coalesced_count,
                    recovery_kind,recovery_context_json,recovery_updated_at,workspace_epoch,
                    ?2,?3,?4,?5
             FROM sync_outbox
             WHERE workspace_id=?1 AND state IN ('pending','sending','failed','conflict')",
            params![
                workspace_id,
                batch_id,
                reason,
                rescue_backup_checksum,
                quarantined_at
            ],
        )
        .map_err(|error| format!("LEGACY_QUEUE_ISOLATION_FAILED: {error}"))?;
    if inserted as i64 != count {
        return Err(format!(
            "LEGACY_QUEUE_ISOLATION_FAILED: 预期隔离 {count} 条，实际 {inserted} 条"
        ));
    }
    transaction
        .execute(
            "DELETE FROM sync_outbox WHERE workspace_id=?1 AND state IN ('pending','sending','failed','conflict')",
            [workspace_id],
        )
        .map_err(|error| format!("LEGACY_QUEUE_ISOLATION_FAILED: {error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("LEGACY_QUEUE_ISOLATION_FAILED: {error}"))?;
    Ok((batch_id, count))
}

fn restore_quarantined_legacy_queue(database: &Database, batch_id: &str) -> Result<(), String> {
    let mut connection = database.open()?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO sync_outbox(
                operation_id,workspace_id,device_id,operation_type,entity_type,entity_id,
                base_version,payload_json,state,attempt_count,created_at,last_attempt_at,
                error_json,payload_version,depends_on_operation_id,coalesced_count,
                recovery_kind,recovery_context_json,recovery_updated_at,workspace_epoch
             )
             SELECT operation_id,workspace_id,device_id,operation_type,entity_type,entity_id,
                    base_version,payload_json,state,attempt_count,created_at,last_attempt_at,
                    error_json,payload_version,depends_on_operation_id,coalesced_count,
                    recovery_kind,recovery_context_json,recovery_updated_at,workspace_epoch
             FROM legacy_abandoned_outbox
             WHERE quarantine_batch_id=?1
             ORDER BY created_at,operation_type,entity_type",
            [batch_id],
        )
        .map_err(|error| format!("LEGACY_QUEUE_RESTORE_FAILED: {error}"))?;
    transaction
        .execute(
            "DELETE FROM legacy_abandoned_outbox WHERE quarantine_batch_id=?1",
            [batch_id],
        )
        .map_err(|error| format!("LEGACY_QUEUE_RESTORE_FAILED: {error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("LEGACY_QUEUE_RESTORE_FAILED: {error}"))?;
    Ok(())
}

fn validate_rescue_backup_matches_upgrade(
    backup: &crate::data_protection::ValidatedRescueBackup,
    preflight: &StorageProtocolUpgradePreflight,
) -> Result<(), String> {
    if backup.source_workspace_marker
        != crate::data_protection::sha256_hex(preflight.workspace_id.as_bytes())
    {
        return Err("RESCUE_BACKUP_WORKSPACE_MISMATCH: 救援备份不属于当前云端工作空间".to_string());
    }
    if backup.pending_operation_count != preflight.queue.total {
        return Err(format!(
            "RESCUE_BACKUP_STALE: 备份包含 {} 条旧操作，当前队列为 {} 条，请重新导出救援备份",
            backup.pending_operation_count, preflight.queue.total
        ));
    }
    if preflight.queue.total == 0 {
        return Err("LEGACY_QUEUE_EMPTY: 旧同步队列为空，请改用完成队列后升级".to_string());
    }
    Ok(())
}

fn run_abandon_queue_upgrade_flow<Q, R, S, C>(
    preflight: &StorageProtocolUpgradePreflight,
    backup: &crate::data_protection::ValidatedRescueBackup,
    quarantine: Q,
    restore: R,
    switch: S,
    server_still_legacy: C,
) -> Result<(StorageProtocolUpgradeServerResult, String, i64), String>
where
    Q: FnOnce() -> Result<(String, i64), String>,
    R: FnOnce(&str) -> Result<(), String>,
    S: FnOnce(
        &StorageProtocolUpgradePreflight,
    ) -> Result<StorageProtocolUpgradeServerResult, String>,
    C: FnOnce() -> Result<bool, String>,
{
    ensure_upgrade_preflight_can_drain(preflight)?;
    validate_rescue_backup_matches_upgrade(backup, preflight)?;
    let (batch_id, quarantined) = quarantine()?;
    if quarantined != preflight.queue.total {
        let _ = restore(&batch_id);
        return Err(format!(
            "LEGACY_QUEUE_CHANGED: 隔离时发现 {} 条旧操作，预检为 {} 条，旧队列已恢复",
            quarantined, preflight.queue.total
        ));
    }
    let switched = match switch(preflight) {
        Ok(switched) => switched,
        Err(error) => {
            let still_legacy = server_still_legacy().unwrap_or(false);
            if still_legacy {
                return match restore(&batch_id) {
                    Ok(()) => Err(format!("{error}；旧队列已恢复到 sync_v1")),
                    Err(restore_error) => Err(format!(
                        "{error}；旧队列恢复失败，已保留隔离区 {batch_id}: {restore_error}"
                    )),
                };
            }
            return Err(format!(
                "{error}；无法确认云端仍为 sync_v1，旧队列保留在隔离区 {batch_id}"
            ));
        }
    };
    let protocol = switched.storage_protocol.as_deref().unwrap_or_else(|| {
        if switched.already_direct {
            crate::storage_contract::DIRECT_CLOUD_PROTOCOL
        } else {
            ""
        }
    });
    if (!switched.upgraded && !switched.already_direct)
        || protocol != crate::storage_contract::DIRECT_CLOUD_PROTOCOL
    {
        return Err(format!(
            "CLOUD_RESPONSE_INVALID: 云端未确认 direct_v2；旧队列保留在隔离区 {batch_id}"
        ));
    }
    if switched.workspace_epoch != preflight.workspace_epoch {
        return Err(format!(
            "WORKSPACE_EPOCH_STALE: 切换期间云端世代从 {} 变为 {}；旧队列保留在隔离区 {batch_id}",
            preflight.workspace_epoch, switched.workspace_epoch
        ));
    }
    Ok((switched, batch_id, quarantined))
}

pub fn storage_protocol_upgrade_after_rescue(
    database: &Database,
    request: StorageProtocolAbandonRequest,
) -> Result<StorageProtocolAbandonResult, String> {
    if !request.confirmed {
        return Err("ABANDON_NOT_CONFIRMED: 放弃旧修改需要明确确认".to_string());
    }
    if request.confirmation_text.trim() != "放弃旧修改" {
        return Err("ABANDON_CONFIRMATION_MISMATCH: 请输入“放弃旧修改”完成二次确认".to_string());
    }
    Uuid::parse_str(request.operation_id.trim())
        .map_err(|_| "VALIDATION_ERROR: 协议升级操作标识无效".to_string())?;
    let backup_path = std::path::PathBuf::from(&request.rescue_backup_path);
    let backup = crate::data_protection::validate_rescue_backup_for_abandon(&backup_path)?;
    let initial = storage_protocol_upgrade_preflight(database)?;
    ensure_upgrade_preflight_can_drain(&initial)?;
    validate_rescue_backup_matches_upgrade(&backup, &initial)?;
    let operation_id = request.operation_id;
    let create_safety_snapshot = request.create_safety_snapshot.unwrap_or(true);
    let reason = format!(
        "用户二次确认放弃旧修改；救援备份校验通过（{}）",
        backup.created_at
    );
    crate::cloud_sync::with_cloud_sync_operation_lock(|| {
        let refreshed = storage_protocol_upgrade_preflight(database)?;
        ensure_upgrade_preflight_can_drain(&refreshed)?;
        validate_rescue_backup_matches_upgrade(&backup, &refreshed)?;
        let (switched, _batch_id, quarantined) = run_abandon_queue_upgrade_flow(
            &refreshed,
            &backup,
            || {
                quarantine_legacy_queue(
                    database,
                    &refreshed.workspace_id,
                    &backup.checksum,
                    &reason,
                )
            },
            |batch_id| restore_quarantined_legacy_queue(database, batch_id),
            |preflight| {
                let session = current_session(database)?;
                let value = client(database)?.rpc_for_database(
                    database,
                    "upgrade_storage_protocol",
                    json!({
                        "p_workspace_id": preflight.workspace_id,
                        "p_device_id": preflight.device_id,
                        "p_operation_id": operation_id,
                        "p_expected_epoch": preflight.workspace_epoch,
                        "p_old_sync_disposition": "abandoned_with_rescue",
                        "p_create_safety_snapshot": create_safety_snapshot,
                    }),
                    &session,
                )?;
                serde_json::from_value(value)
                    .map_err(|error| format!("CLOUD_RESPONSE_INVALID: 协议升级响应无效: {error}"))
            },
            || {
                Ok(
                    storage_protocol_upgrade_preflight(database)?.storage_protocol
                        == crate::storage_contract::LEGACY_SYNC_PROTOCOL,
                )
            },
        )?;
        let final_change_seq = switched
            .change_seq
            .unwrap_or(0)
            .max(storage_mode(database)?.last_change_seq);
        persist_direct_protocol_state(
            database,
            &refreshed.workspace_id,
            switched.workspace_epoch,
            final_change_seq,
        )?;
        Ok(StorageProtocolAbandonResult {
            upgraded: switched.upgraded,
            already_direct: switched.already_direct,
            workspace_epoch: switched.workspace_epoch,
            storage_protocol: crate::storage_contract::DIRECT_CLOUD_PROTOCOL.to_string(),
            change_seq: switched.change_seq,
            safety_snapshot_id: switched.safety_snapshot_id,
            quarantined_operations: quarantined,
            rescue_backup_path: backup_path.display().to_string(),
            rescue_backup_checksum: backup.checksum,
            last_change_seq: final_change_seq,
        })
    })
}

pub fn storage_protocol_reset_to_cloud_authority(
    database: &Database,
    request: StorageProtocolAuthorityResetRequest,
) -> Result<StorageProtocolAbandonResult, String> {
    if !request.confirmed {
        return Err("CLOUD_AUTHORITY_RESET_NOT_CONFIRMED: 必须确认放弃本机旧云端修改".to_string());
    }
    Uuid::parse_str(request.operation_id.trim())
        .map_err(|_| "VALIDATION_ERROR: 云端切换操作标识无效".to_string())?;
    let preflight = storage_protocol_upgrade_preflight(database)?;
    let create_safety_snapshot = request.create_safety_snapshot.unwrap_or(true);
    if preflight.queue.total == 0 {
        if preflight.local_protocol != crate::storage_contract::LEGACY_SYNC_PROTOCOL {
            return Err("CLOUD_PROTOCOL_UNSUPPORTED: 本机当前协议不允许重置旧云端数据".to_string());
        }
        let switched = if preflight.already_direct {
            StorageProtocolUpgradeServerResult {
                upgraded: false,
                already_direct: true,
                workspace_epoch: preflight.workspace_epoch,
                storage_protocol: Some(crate::storage_contract::DIRECT_CLOUD_PROTOCOL.to_string()),
                change_seq: Some(storage_mode(database)?.last_change_seq),
                safety_snapshot_id: None,
            }
        } else {
            if preflight.storage_protocol != crate::storage_contract::LEGACY_SYNC_PROTOCOL
                || !preflight.server_upgrade_allowed
            {
                return Err(
                    "CLOUD_PROTOCOL_UNSUPPORTED: 云端当前协议不允许切换为直连模式".to_string(),
                );
            }
            let session = current_session(database)?;
            let value = client(database)?.rpc_for_database(
                database,
                "upgrade_storage_protocol",
                json!({
                    "p_workspace_id": preflight.workspace_id,
                    "p_device_id": preflight.device_id,
                    "p_operation_id": request.operation_id,
                    "p_expected_epoch": preflight.workspace_epoch,
                    "p_old_sync_disposition": "queue_drained",
                    "p_create_safety_snapshot": create_safety_snapshot,
                }),
                &session,
            )?;
            serde_json::from_value(value)
                .map_err(|error| format!("CLOUD_RESPONSE_INVALID: 协议切换响应无效: {error}"))?
        };
        let protocol = switched.storage_protocol.as_deref().unwrap_or_else(|| {
            if switched.already_direct {
                crate::storage_contract::DIRECT_CLOUD_PROTOCOL
            } else {
                ""
            }
        });
        if (!switched.upgraded && !switched.already_direct)
            || protocol != crate::storage_contract::DIRECT_CLOUD_PROTOCOL
        {
            return Err("CLOUD_RESPONSE_INVALID: 云端未确认切换为直连模式".to_string());
        }
        let final_change_seq = switched
            .change_seq
            .unwrap_or(0)
            .max(storage_mode(database)?.last_change_seq);
        persist_direct_protocol_state(
            database,
            &preflight.workspace_id,
            switched.workspace_epoch,
            final_change_seq,
        )?;
        return Ok(StorageProtocolAbandonResult {
            upgraded: switched.upgraded,
            already_direct: switched.already_direct,
            workspace_epoch: switched.workspace_epoch,
            storage_protocol: crate::storage_contract::DIRECT_CLOUD_PROTOCOL.to_string(),
            change_seq: switched.change_seq,
            safety_snapshot_id: switched.safety_snapshot_id,
            quarantined_operations: 0,
            rescue_backup_path: String::new(),
            rescue_backup_checksum: String::new(),
            last_change_seq: final_change_seq,
        });
    }

    let backup = crate::data_protection::export_automatic_legacy_rescue_backup(
        database,
        "用户确认放弃本机旧云端数据并直接使用 Supabase 权威数据",
    )?;
    if preflight.already_direct {
        let backup_path = std::path::PathBuf::from(&backup.path);
        let validated = crate::data_protection::validate_rescue_backup_for_abandon(&backup_path)?;
        return crate::cloud_sync::with_cloud_sync_operation_lock(|| {
            let refreshed = storage_protocol_upgrade_preflight(database)?;
            if !refreshed.already_direct {
                return Err("CLOUD_PROTOCOL_CHANGED: 云端协议状态发生变化，请重新执行".to_string());
            }
            validate_rescue_backup_matches_upgrade(&validated, &refreshed)?;
            let reason = format!(
                "云端已是 direct_v2；用户确认放弃本机旧修改；救援备份校验通过（{}）",
                validated.created_at
            );
            let (batch_id, quarantined) = quarantine_legacy_queue(
                database,
                &refreshed.workspace_id,
                &validated.checksum,
                &reason,
            )?;
            let final_change_seq = storage_mode(database)?.last_change_seq;
            if let Err(error) = persist_direct_protocol_state(
                database,
                &refreshed.workspace_id,
                refreshed.workspace_epoch,
                final_change_seq,
            ) {
                return match restore_quarantined_legacy_queue(database, &batch_id) {
                    Ok(()) => Err(format!("{error}；本机旧队列已恢复")),
                    Err(restore_error) => Err(format!(
                        "{error}；本机旧队列恢复失败，已保留隔离区 {batch_id}: {restore_error}"
                    )),
                };
            }
            Ok(StorageProtocolAbandonResult {
                upgraded: false,
                already_direct: true,
                workspace_epoch: refreshed.workspace_epoch,
                storage_protocol: crate::storage_contract::DIRECT_CLOUD_PROTOCOL.to_string(),
                change_seq: Some(final_change_seq),
                safety_snapshot_id: None,
                quarantined_operations: quarantined,
                rescue_backup_path: backup_path.display().to_string(),
                rescue_backup_checksum: validated.checksum,
                last_change_seq: final_change_seq,
            })
        });
    }
    storage_protocol_upgrade_after_rescue(
        database,
        StorageProtocolAbandonRequest {
            operation_id: request.operation_id,
            rescue_backup_path: backup.path,
            confirmed: true,
            confirmation_text: "放弃旧修改".to_string(),
            create_safety_snapshot: Some(create_safety_snapshot),
        },
    )
}

fn auth_blocked_reason(database: &Database) -> Result<Option<String>, String> {
    let state = storage_mode(database)?;
    if state.mode != "cloud" {
        return Ok(None);
    }
    database
        .open()?
        .query_row(
            "SELECT auth_blocked_reason FROM local_sync_state WHERE workspace_id = ?1 AND auth_blocked <> 0",
            [state.workspace_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())
}

pub fn resume_pending_sync(database: &Database) -> Result<StorageModeSnapshot, String> {
    let connection = database.open()?;
    let workspace_id: String = connection
        .query_row(
            "SELECT json_extract(value_json, '$') FROM device_settings WHERE key = 'cloud_workspace_id'",
            [],
            |row| row.get(0),
        )
        .map_err(|_| "CLOUD_NOT_CONFIGURED: 尚未初始化 Supabase 工作空间".to_string())?;
    let device_id: String = connection
        .query_row(
            "SELECT json_extract(value_json, '$') FROM device_settings WHERE key = 'device_id'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    drop(connection);
    let session = current_session(database)?;
    let value = client(database)?.rpc_for_database(
        database,
        "device_authorization_get",
        json!({ "p_workspace_id": workspace_id, "p_device_id": device_id }),
        &session,
    )?;
    if value.get("authorized").and_then(Value::as_bool) != Some(true) {
        return Err("DEVICE_REVOKED: 当前设备尚未重新授权".to_string());
    }
    clear_auth_blocked(database)?;
    settings::update_setting(
        database,
        SettingsUpdate {
            scope: SettingsScope::Device,
            key: "storage_mode".to_string(),
            value: json!("cloud"),
        },
    )?;
    crate::cloud_sync::wake_after_connection_ready();
    storage_mode(database)
}

pub fn migration_preview(
    database: &Database,
    request: MigrationPreviewRequest,
) -> Result<MigrationPreview, String> {
    if request.direction != "local_to_cloud" && request.direction != "cloud_to_local_snapshot" {
        return Err("VALIDATION_ERROR: 不支持的迁移方向".to_string());
    }
    let connection = database.open()?;
    let source_workspace_id: String = connection
        .query_row(
            "SELECT id FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    drop(connection);
    let entity_names = crate::data_protection::data_package_copy_entity_names()?;
    let local = local_snapshot(database, &source_workspace_id)?;
    let workspace = workspace_preview(database)?;
    let cloud_counts = if let Some(workspace) = &workspace {
        if request.direction == "cloud_to_local_snapshot" {
            let context = data_copy_target_context(database)?;
            let value = client(database)?.rpc_for_database_with_epoch(
                database,
                "data_copy_export_preview",
                json!({
                    "p_workspace_id": context.workspace_id,
                    "p_device_id": context.device_id,
                    "p_expected_epoch": context.workspace_epoch,
                }),
                &context.session,
                context.workspace_epoch,
            )?;
            let preview: DataCopyExportPreviewResponse = serde_json::from_value(value)
                .map_err(|error| format!("CLOUD_RESPONSE_INVALID: 云端复制预览无效: {error}"))?;
            if preview.workspace_id != workspace.id
                || preview.package_version != crate::data_protection::DATA_PACKAGE_FORMAT_VERSION
                || preview.workspace_epoch != context.workspace_epoch
                || preview.change_seq < 0
            {
                return Err("CLOUD_RESPONSE_INVALID: 云端复制预览元数据不一致".to_string());
            }
            preview.entity_counts
        } else {
            let session = current_session(database)?;
            let snapshot = client(database)?.rpc_for_database(
                database,
                "cloud_snapshot_get",
                json!({ "p_workspace_id": workspace.id.clone() }),
                &session,
            )?;
            entity_names
                .iter()
                .map(|entity| {
                    (
                        entity.clone(),
                        snapshot
                            .get(entity)
                            .and_then(Value::as_array)
                            .map(|rows| rows.len() as i64)
                            .unwrap_or(0),
                    )
                })
                .collect()
        }
    } else {
        HashMap::new()
    };
    let entities = entity_names
        .iter()
        .map(|entity| MigrationEntityCount {
            entity: entity.clone(),
            local_count: local
                .get(entity)
                .and_then(Value::as_array)
                .map(|rows| rows.len() as i64)
                .unwrap_or(0),
            cloud_count: cloud_counts.get(entity).copied(),
        })
        .collect::<Vec<_>>();
    let (
        source_entity_count,
        target_entity_count,
        target_is_empty,
        replace_entity_count,
        safety_snapshot_required,
    ) = migration_preview_impact(&request.direction, &entities);
    let cloud_has_business_data = entities
        .iter()
        .map(|entity| entity.cloud_count.unwrap_or(0))
        .sum::<i64>()
        > 0;
    let conflicts = migration_conflicts(
        &request.direction,
        workspace.is_some(),
        cloud_has_business_data,
    );
    let (preview_source_workspace_id, preview_target_workspace_id) =
        if request.direction == "cloud_to_local_snapshot" {
            (
                workspace
                    .as_ref()
                    .map(|workspace| workspace.id.clone())
                    .unwrap_or_default(),
                Some(source_workspace_id),
            )
        } else {
            (
                source_workspace_id,
                workspace.as_ref().map(|workspace| workspace.id.clone()),
            )
        };
    Ok(MigrationPreview {
        direction: request.direction,
        source_workspace_id: preview_source_workspace_id,
        target_workspace_id: preview_target_workspace_id,
        package_version: crate::data_protection::DATA_PACKAGE_FORMAT_VERSION,
        entities,
        source_entity_count,
        target_entity_count,
        target_is_empty,
        replace_entity_count,
        safety_snapshot_required,
        can_execute: conflicts.is_empty(),
        conflicts,
    })
}

fn migration_preview_impact(
    direction: &str,
    entities: &[MigrationEntityCount],
) -> (i64, i64, bool, i64, bool) {
    let local_entity_count = entities.iter().map(|entity| entity.local_count).sum();
    let cloud_entity_count = entities
        .iter()
        .map(|entity| entity.cloud_count.unwrap_or(0))
        .sum();
    let (source_entity_count, target_entity_count) = if direction == "cloud_to_local_snapshot" {
        (cloud_entity_count, local_entity_count)
    } else {
        (local_entity_count, cloud_entity_count)
    };
    let target_is_empty = target_entity_count == 0;
    (
        source_entity_count,
        target_entity_count,
        target_is_empty,
        target_entity_count,
        direction == "local_to_cloud" && !target_is_empty,
    )
}

fn build_data_copy_chunks(snapshot: &Value) -> Result<(Vec<DataCopyChunk>, String), String> {
    let mut chunks = Vec::new();
    for entity_name in crate::data_protection::data_package_copy_entity_names()? {
        let rows = snapshot
            .get(&entity_name)
            .and_then(Value::as_array)
            .ok_or_else(|| format!("DATA_PACKAGE_ENTITY_MISSING: {entity_name}"))?;
        let mut pending = Vec::<Value>::new();
        for row in rows {
            let mut candidate = pending.clone();
            candidate.push(row.clone());
            let candidate_value = Value::Array(candidate);
            let candidate_bytes = crate::data_protection::canonical_json_bytes(&candidate_value)?;
            if candidate_bytes.len() > DATA_COPY_CHUNK_MAX_BYTES {
                if pending.is_empty() {
                    return Err(format!("DATA_PACKAGE_ROW_TOO_LARGE: {entity_name}"));
                }
                push_data_copy_chunk(&mut chunks, &entity_name, std::mem::take(&mut pending))?;
                pending.push(row.clone());
            } else {
                pending = candidate_value.as_array().cloned().unwrap_or_default();
            }
        }
        push_data_copy_chunk(&mut chunks, &entity_name, pending)?;
    }
    if chunks.len() > 256 {
        return Err("DATA_PACKAGE_TOO_MANY_CHUNKS: 数据包分块超过 256 个".to_string());
    }
    let hash_chain = chunks
        .iter()
        .map(|chunk| chunk.content_hash.as_str())
        .collect::<String>();
    Ok((
        chunks,
        crate::data_protection::sha256_hex(hash_chain.as_bytes()),
    ))
}

fn push_data_copy_chunk(
    chunks: &mut Vec<DataCopyChunk>,
    entity_name: &str,
    rows: Vec<Value>,
) -> Result<(), String> {
    let value = Value::Array(rows);
    let bytes = crate::data_protection::canonical_json_bytes(&value)?;
    if bytes.len() > DATA_COPY_CHUNK_MAX_BYTES {
        return Err(format!("DATA_PACKAGE_CHUNK_TOO_LARGE: {entity_name}"));
    }
    let content_text = String::from_utf8(bytes.clone())
        .map_err(|error| format!("DATA_PACKAGE_ENCODING_INVALID: {error}"))?;
    chunks.push(DataCopyChunk {
        chunk_index: chunks.len(),
        entity_name: entity_name.to_string(),
        content_text,
        content_hash: crate::data_protection::sha256_hex(&bytes),
        row_count: value.as_array().map(Vec::len).unwrap_or(0),
    });
    Ok(())
}

pub fn stage_local_data_copy(database: &Database) -> Result<DataCopyStageResult, String> {
    let context = data_copy_target_context(database)?;
    let connection = database.open()?;
    let source_workspace_id: String = connection
        .query_row(
            "SELECT id FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    drop(connection);

    let snapshot = local_snapshot(database, &source_workspace_id)?;
    let (chunks, content_hash) = build_data_copy_chunks(&snapshot)?;
    let api = client(database)?;
    let begin = api.rpc_for_database(
        database,
        "data_copy_stage_begin",
        json!({
            "p_workspace_id": context.workspace_id,
            "p_device_id": context.device_id,
            "p_expected_epoch": context.workspace_epoch,
            "p_package_version": crate::data_protection::DATA_PACKAGE_FORMAT_VERSION,
            "p_expected_chunk_count": chunks.len(),
            "p_expected_content_hash": content_hash,
        }),
        &context.session,
    )?;
    let package_id = begin
        .get("packageId")
        .and_then(Value::as_str)
        .ok_or_else(|| "CLOUD_RESPONSE_INVALID: staging begin 缺少 packageId".to_string())?
        .to_string();

    let upload_result = (|| {
        for chunk in &chunks {
            api.rpc_for_database(
                database,
                "data_copy_stage_chunk_put",
                json!({
                    "p_workspace_id": context.workspace_id,
                    "p_device_id": context.device_id,
                    "p_expected_epoch": context.workspace_epoch,
                    "p_package_id": package_id,
                    "p_chunk_index": chunk.chunk_index,
                    "p_entity_name": chunk.entity_name,
                    "p_content_text": chunk.content_text,
                    "p_content_hash": chunk.content_hash,
                    "p_row_count": chunk.row_count,
                }),
                &context.session,
            )?;
        }
        api.rpc_for_database(
            database,
            "data_copy_stage_validate",
            json!({
                "p_workspace_id": context.workspace_id,
                "p_device_id": context.device_id,
                "p_expected_epoch": context.workspace_epoch,
                "p_package_id": package_id,
            }),
            &context.session,
        )
    })();
    let validated = match upload_result {
        Ok(value) => value,
        Err(error) => {
            let _ = api.rpc_for_database(
                database,
                "data_copy_stage_discard",
                json!({
                    "p_workspace_id": context.workspace_id,
                    "p_device_id": context.device_id,
                    "p_expected_epoch": context.workspace_epoch,
                    "p_package_id": package_id,
                }),
                &context.session,
            );
            return Err(error);
        }
    };
    let entity_counts = serde_json::from_value(
        validated
            .get("entityCounts")
            .cloned()
            .unwrap_or_else(|| json!({})),
    )
    .map_err(|error| format!("CLOUD_RESPONSE_INVALID: staging entityCounts 无效: {error}"))?;
    Ok(DataCopyStageResult {
        package_id,
        status: validated
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("validated")
            .to_string(),
        entity_counts,
        content_hash,
        chunk_count: chunks.len(),
    })
}

pub fn replace_staged_local_data_copy(
    database: &Database,
    request: DataCopyReplaceRequest,
) -> Result<DataCopyReplaceResult, String> {
    crate::cloud_sync::with_cloud_sync_operation_lock(|| {
        replace_staged_local_data_copy_locked(database, request)
    })
}

fn replace_staged_local_data_copy_locked(
    database: &Database,
    request: DataCopyReplaceRequest,
) -> Result<DataCopyReplaceResult, String> {
    if !request.confirmed {
        return Err(
            "DATA_COPY_REPLACE_CONFIRMATION_REQUIRED: 完整替换云端数据必须明确确认".to_string(),
        );
    }
    let package_id = Uuid::parse_str(request.package_id.trim())
        .map_err(|_| "VALIDATION_ERROR: packageId 必须是 UUID".to_string())?;
    let operation_id = Uuid::parse_str(request.operation_id.trim())
        .map_err(|_| "VALIDATION_ERROR: operationId 必须是 UUID".to_string())?;
    let content_hash = request.content_hash.trim().to_ascii_lowercase();
    if content_hash.len() != 64 || !content_hash.bytes().all(|value| value.is_ascii_hexdigit()) {
        return Err("VALIDATION_ERROR: contentHash 必须是 SHA-256 十六进制值".to_string());
    }

    let context = data_copy_target_context(database)?;
    let body = json!({
        "p_workspace_id": context.workspace_id,
        "p_device_id": context.device_id,
        "p_expected_epoch": context.workspace_epoch,
        "p_package_id": package_id,
        "p_expected_content_hash": content_hash,
        "p_operation_id": operation_id,
        "p_confirmed": true,
    });
    let api = client(database)?;
    let value = match api.rpc_for_database_with_epoch_and_timeout(
        database,
        "data_copy_replace_commit",
        body.clone(),
        &context.session,
        context.workspace_epoch,
        RESTORE_TIMEOUT_SECONDS,
    ) {
        Ok(value) => value,
        Err(error)
            if error.starts_with("NETWORK_ERROR:")
                || error.starts_with("CLOUD_TIMEOUT:")
                || error.starts_with("CLOUD_SERVICE_UNAVAILABLE:") =>
        {
            api.rpc_for_database_with_epoch_and_timeout(
                database,
                "data_copy_replace_commit",
                body,
                &context.session,
                context.workspace_epoch,
                RESTORE_TIMEOUT_SECONDS,
            )
            .map_err(|retry_error| {
                record_direct_cloud_access_error(&context.workspace_id, &retry_error);
                retry_error
            })?
        }
        Err(error) => {
            record_direct_cloud_access_error(&context.workspace_id, &error);
            return Err(error);
        }
    };
    let replaced: DataCopyReplaceResult = serde_json::from_value(value).map_err(|error| {
        let error = format!("CLOUD_RESPONSE_INVALID: data_copy_replace_commit 响应无效: {error}");
        record_direct_cloud_access_error(&context.workspace_id, &error);
        error
    })?;
    if !replaced.replaced
        || replaced.package_id != package_id.to_string()
        || replaced.content_hash != content_hash
        || replaced.previous_epoch != context.workspace_epoch
        || replaced.workspace_epoch <= replaced.previous_epoch
    {
        let error = "CLOUD_RESPONSE_INVALID: 云端没有返回一致的数据复制替换结果".to_string();
        record_direct_cloud_access_error(&context.workspace_id, &error);
        return Err(error);
    }
    record_data_copy_replace_checkpoint(database, &context, &replaced)?;
    record_direct_cloud_access_success(&context.workspace_id);
    Ok(replaced)
}

fn record_data_copy_replace_checkpoint(
    database: &Database,
    context: &DirectCloudReadContext,
    result: &DataCopyReplaceResult,
) -> Result<(), String> {
    database
        .open()?
        .execute(
            "INSERT INTO local_sync_state(workspace_id, device_id, last_change_seq, last_full_sync_at, workspace_epoch)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(workspace_id) DO UPDATE SET
               device_id=excluded.device_id,
               last_change_seq=excluded.last_change_seq,
               last_full_sync_at=excluded.last_full_sync_at,
               last_error=NULL,
               workspace_epoch=excluded.workspace_epoch,
               epoch_blocked=0,
               epoch_blocked_reason=NULL,
               epoch_blocked_at=NULL",
            params![
                context.workspace_id,
                context.device_id,
                result.reset_change_seq,
                now_seconds() * 1_000,
                result.workspace_epoch,
            ],
        )
        .map_err(|error| error.to_string())?;
    DIRECT_TRACKING_LEASE_TOKENS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(&direct_tracking_lease_key(context));
    Ok(())
}

pub fn begin_cloud_data_copy_export(database: &Database) -> Result<DataCopyExportManifest, String> {
    let context = data_copy_target_context(database)?;
    let value = client(database)?.rpc_for_database_with_epoch_and_timeout(
        database,
        "data_copy_export_begin",
        json!({
            "p_workspace_id": context.workspace_id,
            "p_device_id": context.device_id,
            "p_expected_epoch": context.workspace_epoch,
        }),
        &context.session,
        context.workspace_epoch,
        SNAPSHOT_TIMEOUT_SECONDS,
    )?;
    let manifest: DataCopyExportManifest = serde_json::from_value(value)
        .map_err(|error| format!("CLOUD_RESPONSE_INVALID: 云端导出清单无效: {error}"))?;
    validate_data_copy_export_manifest(&manifest, context.workspace_epoch)?;
    Ok(manifest)
}

pub fn get_cloud_data_copy_export_chunk(
    database: &Database,
    request: DataCopyExportChunkRequest,
) -> Result<DataCopyExportChunk, String> {
    let package_id = Uuid::parse_str(request.package_id.trim())
        .map_err(|_| "VALIDATION_ERROR: packageId 必须是 UUID".to_string())?;
    if request.chunk_index >= 256 {
        return Err("VALIDATION_ERROR: chunkIndex 超出范围".to_string());
    }
    let context = data_copy_target_context(database)?;
    let value = client(database)?.rpc_for_database_with_epoch(
        database,
        "data_copy_export_chunk_get",
        json!({
            "p_workspace_id": context.workspace_id,
            "p_device_id": context.device_id,
            "p_package_id": package_id,
            "p_chunk_index": request.chunk_index,
        }),
        &context.session,
        context.workspace_epoch,
    )?;
    let chunk: DataCopyExportChunk = serde_json::from_value(value)
        .map_err(|error| format!("CLOUD_RESPONSE_INVALID: 云端导出分块无效: {error}"))?;
    validate_data_copy_export_chunk(&chunk, &package_id.to_string(), request.chunk_index)?;
    Ok(chunk)
}

pub fn discard_cloud_data_copy_export(
    database: &Database,
    request: DataCopyExportDiscardRequest,
) -> Result<Value, String> {
    let package_id = Uuid::parse_str(request.package_id.trim())
        .map_err(|_| "VALIDATION_ERROR: packageId 必须是 UUID".to_string())?;
    let context = data_copy_target_context(database)?;
    client(database)?.rpc_for_database_with_epoch(
        database,
        "data_copy_export_discard",
        json!({
            "p_workspace_id": context.workspace_id,
            "p_device_id": context.device_id,
            "p_package_id": package_id,
        }),
        &context.session,
        context.workspace_epoch,
    )
}

pub fn replace_local_with_cloud_data_copy(
    database: &Database,
    request: DataCopyReplaceLocalRequest,
) -> Result<DataCopyReplaceLocalResult, String> {
    crate::cloud_sync::with_cloud_sync_operation_lock(|| {
        replace_local_with_cloud_data_copy_locked(database, request)
    })
}

fn replace_local_with_cloud_data_copy_locked(
    database: &Database,
    request: DataCopyReplaceLocalRequest,
) -> Result<DataCopyReplaceLocalResult, String> {
    if !request.confirmed {
        return Err(
            "DATA_COPY_REPLACE_CONFIRMATION_REQUIRED: 完整替换本地数据必须明确确认".to_string(),
        );
    }
    let manifest = begin_cloud_data_copy_export(database)?;
    let download_result = download_cloud_data_copy(database, &manifest);
    let _ = discard_cloud_data_copy_export(
        database,
        DataCopyExportDiscardRequest {
            package_id: manifest.package_id.clone(),
        },
    );
    let snapshot = download_result?;
    replace_local_with_downloaded_data_copy(database, &manifest, snapshot)
}

fn replace_local_with_downloaded_data_copy(
    database: &Database,
    manifest: &DataCopyExportManifest,
    snapshot: Value,
) -> Result<DataCopyReplaceLocalResult, String> {
    let safety_directory = database.data_directory().join("backups");
    let safety_path = safety_directory.join(format!(
        "timegenie-before-cloud-copy-{}-{}.timegenie-backup",
        now_seconds(),
        Uuid::now_v7()
    ));
    let safety_backup = crate::data_protection::export_automatic_safety_backup(
        database,
        safety_path.display().to_string(),
    )?;

    let workspace_id =
        crate::data_protection::replace_local_business_data_from_copy(database, &snapshot)?;
    Ok(DataCopyReplaceLocalResult {
        replaced: true,
        package_id: manifest.package_id.clone(),
        content_hash: manifest.content_hash.clone(),
        safety_backup_path: safety_backup.path,
        workspace_id,
        entity_counts: manifest.entity_counts.clone(),
    })
}

fn download_cloud_data_copy(
    database: &Database,
    manifest: &DataCopyExportManifest,
) -> Result<Value, String> {
    download_data_copy_from_fetch(manifest, |chunk_index| {
        get_cloud_data_copy_export_chunk(
            database,
            DataCopyExportChunkRequest {
                package_id: manifest.package_id.clone(),
                chunk_index,
            },
        )
    })
}

fn download_data_copy_from_fetch<F>(
    manifest: &DataCopyExportManifest,
    mut fetch: F,
) -> Result<Value, String>
where
    F: FnMut(usize) -> Result<DataCopyExportChunk, String>,
{
    let entity_names = crate::data_protection::data_package_copy_entity_names()?;
    let mut snapshot = entity_names
        .iter()
        .map(|entity| (entity.clone(), Value::Array(Vec::new())))
        .collect::<serde_json::Map<_, _>>();
    let mut hash_chain = String::with_capacity(manifest.chunk_count * 64);
    for chunk_index in 0..manifest.chunk_count {
        let chunk = fetch(chunk_index)?;
        validate_data_copy_export_chunk(&chunk, &manifest.package_id, chunk_index)?;
        let rows: Vec<Value> = serde_json::from_str(&chunk.content_text).map_err(|_| {
            "DATA_PACKAGE_CHUNK_JSON_INVALID: 云端导出分块不是有效 JSON".to_string()
        })?;
        snapshot
            .get_mut(&chunk.entity_name)
            .and_then(Value::as_array_mut)
            .ok_or_else(|| {
                format!(
                    "DATA_PACKAGE_ENTITY_MISSING: 云端导出包含未知实体 {}",
                    chunk.entity_name
                )
            })?
            .extend(rows);
        hash_chain.push_str(&chunk.content_hash);
    }
    if crate::data_protection::sha256_hex(hash_chain.as_bytes()) != manifest.content_hash {
        return Err("DATA_PACKAGE_CONTENT_HASH_MISMATCH: 云端导出整包哈希校验失败".to_string());
    }
    for entity_name in entity_names {
        let actual_count = snapshot
            .get(&entity_name)
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or_default() as i64;
        if manifest.entity_counts.get(&entity_name).copied() != Some(actual_count) {
            return Err(format!(
                "DATA_PACKAGE_ENTITY_COUNT_MISMATCH: {entity_name} 实体数量与清单不一致"
            ));
        }
    }
    Ok(Value::Object(snapshot))
}

fn validate_data_copy_export_manifest(
    manifest: &DataCopyExportManifest,
    expected_epoch: i64,
) -> Result<(), String> {
    Uuid::parse_str(&manifest.package_id)
        .map_err(|_| "CLOUD_RESPONSE_INVALID: 云端导出 packageId 无效".to_string())?;
    if manifest.package_version != crate::data_protection::DATA_PACKAGE_FORMAT_VERSION
        || manifest.workspace_epoch != expected_epoch
        || manifest.change_seq < 0
        || !(22..=256).contains(&manifest.chunk_count)
        || manifest.content_hash.len() != 64
        || !manifest
            .content_hash
            .bytes()
            .all(|value| value.is_ascii_hexdigit())
    {
        return Err("CLOUD_RESPONSE_INVALID: 云端导出清单元数据不一致".to_string());
    }
    let expected_entities = crate::data_protection::data_package_copy_entity_names()?;
    if manifest.entity_counts.len() != expected_entities.len()
        || expected_entities.iter().any(|entity| {
            manifest
                .entity_counts
                .get(entity)
                .is_none_or(|count| *count < 0)
        })
    {
        return Err("CLOUD_RESPONSE_INVALID: 云端导出实体目录不完整".to_string());
    }
    Ok(())
}

fn validate_data_copy_export_chunk(
    chunk: &DataCopyExportChunk,
    package_id: &str,
    chunk_index: usize,
) -> Result<(), String> {
    if chunk.package_id != package_id
        || chunk.chunk_index != chunk_index
        || !crate::data_protection::data_package_copy_entity_names()?.contains(&chunk.entity_name)
        || chunk.content_text.as_bytes().len() > DATA_COPY_CHUNK_MAX_BYTES
        || crate::data_protection::sha256_hex(chunk.content_text.as_bytes()) != chunk.content_hash
    {
        return Err("DATA_PACKAGE_CHUNK_HASH_MISMATCH: 云端导出分块校验失败".to_string());
    }
    let rows: Value = serde_json::from_str(&chunk.content_text)
        .map_err(|_| "DATA_PACKAGE_CHUNK_JSON_INVALID: 云端导出分块不是有效 JSON".to_string())?;
    if rows.as_array().map(Vec::len) != Some(chunk.row_count) {
        return Err("DATA_PACKAGE_ROW_COUNT_MISMATCH: 云端导出分块行数不一致".to_string());
    }
    Ok(())
}

fn migration_conflicts(
    direction: &str,
    cloud_workspace_exists: bool,
    cloud_has_business_data: bool,
) -> Vec<String> {
    let mut conflicts = Vec::new();
    if direction == "cloud_to_local_snapshot" && !cloud_workspace_exists {
        conflicts
            .push("当前账号还没有可使用的云端工作空间，不能用空云端快照覆盖本机数据".to_string());
    }
    if direction == "cloud_to_local_snapshot" && cloud_workspace_exists && !cloud_has_business_data
    {
        conflicts.push(
            "云端工作空间还没有事项、计时或报告数据，不能用空云端快照覆盖本机数据".to_string(),
        );
    }
    conflicts
}

pub fn migration_execute(
    database: &Database,
    request: MigrationExecuteRequest,
) -> Result<StorageModeSnapshot, String> {
    if !request.confirmed {
        return Err("MIGRATION_NOT_CONFIRMED: 迁移必须明确确认后执行".to_string());
    }
    ensure_no_pending_cloud_work(database)?;
    let preview = migration_preview(
        database,
        MigrationPreviewRequest {
            direction: request.direction.clone(),
        },
    )?;
    if !preview.can_execute {
        return Err("MIGRATION_BLOCKED: 当前数据存在未处理冲突".to_string());
    }
    if request.direction == "local_to_cloud" {
        let session = current_session(database)?;
        let workspace = workspace_bootstrap(database)?;
        let connection = database.open()?;
        let device_name: String = connection
            .query_row(
                "SELECT json_extract(value_json, '$') FROM device_settings WHERE key = 'device_name'",
                [],
                |row| row.get(0),
            )
            .unwrap_or_else(|_| "当前设备".to_string());
        drop(connection);
        register_device(
            database,
            CloudDeviceRegisterRequest {
                device_name,
                platform: std::env::consts::OS.to_string(),
                app_version: env!("CARGO_PKG_VERSION").to_string(),
            },
        )?;
        let snapshot = local_snapshot(database, &preview.source_workspace_id)?;
        let client = client(database)?;
        client.rpc_for_database(
            database,
            "migration_import_snapshot",
            json!({
                "p_workspace_id": workspace.id,
                "p_operation_id": Uuid::now_v7().to_string(),
                "p_device_id": storage_mode(database)?.device_id,
                "p_snapshot": snapshot
            }),
            &session,
        )?;
        let cloud_workspace_id = workspace.id.clone();
        settings::update_setting(
            database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "storage_mode".to_string(),
                value: json!("cloud"),
            },
        )?;
        initialize_sync_state(
            database,
            &cloud_workspace_id,
            &storage_mode(database)?.device_id,
        )?;
        crate::cloud_sync::pull_snapshot(database)?;
        crate::cloud_sync::wake_after_connection_ready();
    } else if request.direction == "cloud_to_local_snapshot" {
        let workspace = workspace_bootstrap(database)?;
        let connection = database.open()?;
        let device_name: String = connection
            .query_row(
                "SELECT json_extract(value_json, '$') FROM device_settings WHERE key = 'device_name'",
                [],
                |row| row.get(0),
            )
            .unwrap_or_else(|_| "当前设备".to_string());
        drop(connection);
        register_device(
            database,
            CloudDeviceRegisterRequest {
                device_name,
                platform: std::env::consts::OS.to_string(),
                app_version: env!("CARGO_PKG_VERSION").to_string(),
            },
        )?;
        settings::update_setting(
            database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "cloud_workspace_id".to_string(),
                value: json!(workspace.id),
            },
        )?;
        settings::update_setting(
            database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "storage_mode".to_string(),
                value: json!("cloud"),
            },
        )?;
        initialize_sync_state(database, &workspace.id, &storage_mode(database)?.device_id)?;
        crate::cloud_sync::pull_snapshot(database)?;
        crate::cloud_sync::wake_after_connection_ready();
    }
    storage_mode(database)
}

fn initialize_sync_state(
    database: &Database,
    workspace_id: &str,
    device_id: &str,
) -> Result<(), String> {
    database
        .open()?
        .execute(
            "INSERT INTO local_sync_state(workspace_id, device_id, last_change_seq, last_full_sync_at, last_error)
             VALUES (?1, ?2, 0, NULL, NULL)
             ON CONFLICT(workspace_id) DO UPDATE SET device_id = excluded.device_id, last_error = NULL",
            params![workspace_id, device_id],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub fn latest_change_seq(
    database: &Database,
    client: &SupabaseClient,
    session: &CloudSession,
    workspace_id: &str,
) -> Result<i64, String> {
    let rows: Vec<ChangeSeqRow> = client
        .request_for_database(
            database,
            client
                .get(format!(
                    "{}/rest/v1/workspace_changes?workspace_id=eq.{}&select=change_seq&order=change_seq.desc&limit=1",
                    client.project_url, workspace_id
                )),
            session,
        )?
        .json()
        .map_err(|error| format!("NETWORK_ERROR: 读取云端修订号失败: {error}"))?;
    Ok(rows.first().map(|row| row.change_seq).unwrap_or(0))
}

pub(crate) struct SupabaseClient {
    pub(crate) project_url: String,
    anon_key: String,
    http: Client,
}

impl SupabaseClient {
    pub(crate) fn anon_key(&self) -> &str {
        &self.anon_key
    }

    pub(crate) fn get(&self, url: String) -> RequestBuilder {
        self.http
            .get(url)
            .header("apikey", &self.anon_key)
            .header("Accept-Profile", CLOUD_SCHEMA)
            .header("Content-Profile", CLOUD_SCHEMA)
    }

    pub(crate) fn post(&self, url: String) -> RequestBuilder {
        self.http
            .post(url)
            .header("apikey", &self.anon_key)
            .header("Accept-Profile", CLOUD_SCHEMA)
            .header("Content-Profile", CLOUD_SCHEMA)
    }

    fn auth_token(
        &self,
        grant_type: &str,
        body: Value,
    ) -> Result<reqwest::blocking::Response, String> {
        let url = format!("{}/auth/v1/token?grant_type={grant_type}", self.project_url);
        let response = self
            .http
            .post(&url)
            .header("apikey", &self.anon_key)
            .json(&body)
            .send()
            .map_err(|error| format!("NETWORK_ERROR: Supabase 登录请求失败: {error}"))?;
        if matches!(
            response.status(),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ) {
            return self
                .http
                .post(url)
                .header("apikey", &self.anon_key)
                .bearer_auth(&self.anon_key)
                .json(&body)
                .send()
                .map_err(|error| format!("NETWORK_ERROR: Supabase 登录请求失败: {error}"));
        }
        Ok(response)
    }

    fn auth_logout(&self, session: &CloudSession, scope: &str) -> Result<(), String> {
        let response = self
            .http
            .post(format!("{}/auth/v1/logout?scope={scope}", self.project_url))
            .header("apikey", &self.anon_key)
            .bearer_auth(&session.access_token)
            .send()
            .map_err(|error| format!("NETWORK_ERROR: Supabase 退出请求失败: {error}"))?;
        if response.status().is_success()
            || matches!(
                response.status(),
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
            )
        {
            return Ok(());
        }
        Err(map_http_error(response, "AUTH_LOGOUT_FAILED"))
    }

    pub(crate) fn request(
        &self,
        request: RequestBuilder,
        session: &CloudSession,
    ) -> Result<reqwest::blocking::Response, String> {
        let response = request
            .bearer_auth(&session.access_token)
            .send()
            .map_err(map_supabase_request_error)?;
        if !response.status().is_success() {
            return Err(map_http_error(response, "CLOUD_REQUEST_FAILED"));
        }
        Ok(response)
    }

    pub(crate) fn request_for_database(
        &self,
        database: &Database,
        request: RequestBuilder,
        session: &CloudSession,
    ) -> Result<reqwest::blocking::Response, String> {
        let retry_request = request.try_clone();
        match self.request(request, session) {
            Ok(response) => Ok(response),
            Err(error) if is_business_request_auth_error(&error) => {
                let refreshed = refresh_current_session(database)?;
                if let Some(retry_request) = retry_request {
                    self.request(retry_request, &refreshed)
                } else {
                    Err("AUTH_RETRY_REQUIRED: 登录状态已恢复，请重试刚才的操作".to_string())
                }
            }
            Err(error) => Err(error),
        }
    }

    pub(crate) fn rpc_for_database(
        &self,
        database: &Database,
        function: &str,
        body: Value,
        session: &CloudSession,
    ) -> Result<Value, String> {
        let workspace_epoch = body
            .get("p_workspace_id")
            .and_then(Value::as_str)
            .and_then(|workspace_id| {
                database
                    .open()
                    .ok()?
                    .query_row(
                        "SELECT workspace_epoch FROM local_sync_state WHERE workspace_id=?1",
                        [workspace_id],
                        |row| row.get::<_, i64>(0),
                    )
                    .optional()
                    .ok()
                    .flatten()
            })
            .unwrap_or(1);
        self.request_for_database(
            database,
            self.post(format!("{}/rest/v1/rpc/{function}", self.project_url))
                .header("x-timegenie-workspace-epoch", workspace_epoch.to_string())
                .json(&body),
            session,
        )?
        .json()
        .map_err(|error| format!("NETWORK_ERROR: Supabase RPC 响应无效: {error}"))
    }

    pub(crate) fn rpc_for_database_with_timeout(
        &self,
        database: &Database,
        function: &str,
        body: Value,
        session: &CloudSession,
        timeout_seconds: u64,
    ) -> Result<Value, String> {
        let workspace_epoch = body
            .get("p_workspace_id")
            .and_then(Value::as_str)
            .and_then(|workspace_id| {
                database
                    .open()
                    .ok()?
                    .query_row(
                        "SELECT workspace_epoch FROM local_sync_state WHERE workspace_id=?1",
                        [workspace_id],
                        |row| row.get::<_, i64>(0),
                    )
                    .optional()
                    .ok()
                    .flatten()
            })
            .unwrap_or(1);
        self.request_for_database(
            database,
            self.post(format!("{}/rest/v1/rpc/{function}", self.project_url))
                .timeout(std::time::Duration::from_secs(timeout_seconds))
                .header("x-timegenie-workspace-epoch", workspace_epoch.to_string())
                .json(&body),
            session,
        )?
        .json()
        .map_err(|error| format!("NETWORK_ERROR: Supabase RPC 响应无效: {error}"))
    }

    pub(crate) fn rpc_for_database_with_epoch(
        &self,
        database: &Database,
        function: &str,
        body: Value,
        session: &CloudSession,
        workspace_epoch: i64,
    ) -> Result<Value, String> {
        self.request_for_database(
            database,
            self.post(format!("{}/rest/v1/rpc/{function}", self.project_url))
                .header("x-timegenie-workspace-epoch", workspace_epoch.to_string())
                .json(&body),
            session,
        )?
        .json()
        .map_err(|error| format!("NETWORK_ERROR: Supabase RPC 响应无效: {error}"))
    }

    pub(crate) fn rpc_for_database_with_epoch_and_timeout(
        &self,
        database: &Database,
        function: &str,
        body: Value,
        session: &CloudSession,
        workspace_epoch: i64,
        timeout_seconds: u64,
    ) -> Result<Value, String> {
        self.request_for_database(
            database,
            self.post(format!("{}/rest/v1/rpc/{function}", self.project_url))
                .timeout(std::time::Duration::from_secs(timeout_seconds))
                .header("x-timegenie-workspace-epoch", workspace_epoch.to_string())
                .json(&body),
            session,
        )?
        .json()
        .map_err(|error| format!("NETWORK_ERROR: Supabase RPC 响应无效: {error}"))
    }
}

fn map_supabase_request_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        return "CLOUD_TIMEOUT: 云端操作处理时间较长，请稍后刷新确认结果后再重试".to_string();
    }
    format!("NETWORK_ERROR: Supabase 请求失败: {error}")
}

pub(crate) fn client(database: &Database) -> Result<SupabaseClient, String> {
    let connection = database.open()?;
    let project_url: String = connection
        .query_row(
            "SELECT json_extract(value_json, '$') FROM device_settings WHERE key = 'supabase_project_url'",
            [],
            |row| row.get(0),
        )
        .map_err(|_| "CLOUD_NOT_CONFIGURED: 尚未配置 Supabase Project URL".to_string())?;
    let anon_key: String = connection
        .query_row(
            "SELECT json_extract(value_json, '$') FROM device_settings WHERE key = 'supabase_anon_key'",
            [],
            |row| row.get(0),
        )
        .map_err(|_| "CLOUD_NOT_CONFIGURED: 尚未配置 Supabase anon key".to_string())?;
    let project_url = validate_project_url(&project_url)?;
    if anon_key.trim().is_empty() {
        return Err("CLOUD_NOT_CONFIGURED: 尚未配置 Supabase anon key".to_string());
    }
    let http = Client::builder()
        .timeout(std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECONDS))
        .build()
        .map_err(|error| format!("NETWORK_ERROR: 无法初始化 Supabase 客户端: {error}"))?;
    Ok(SupabaseClient {
        project_url,
        anon_key,
        http,
    })
}

pub(crate) fn current_session(database: &Database) -> Result<CloudSession, String> {
    #[cfg(debug_assertions)]
    if let Some(session) = dev_cloud_session_from_env() {
        return Ok(session);
    }
    current_session_with_refresh(database, false, true)
}

#[cfg(debug_assertions)]
fn dev_cloud_session_from_env() -> Option<CloudSession> {
    if std::env::var_os("TG_DEV_CLOUD_SESSION").is_none() {
        return None;
    }
    let user_id = std::env::var("TG_DEV_CLOUD_USER_ID").ok()?;
    let access_token =
        std::env::var("TG_DEV_CLOUD_ACCESS_TOKEN").unwrap_or_else(|_| format!("access-{user_id}"));
    Some(CloudSession {
        user_id: user_id.clone(),
        email: Some(format!("{user_id}@timegenie.test")),
        access_token,
        refresh_token: format!("refresh-{user_id}"),
        expires_at: Some(now_seconds() + 24 * 60 * 60),
    })
}

#[cfg(debug_assertions)]
pub(crate) fn configure_dev_cloud_from_env(database: &Database) -> Result<(), String> {
    if std::env::var_os("TG_DEV_CLOUD_SESSION").is_none() {
        return Ok(());
    }
    let project_url = std::env::var("TG_DEV_CLOUD_URL")
        .map_err(|_| "TG_DEV_CLOUD_URL is required".to_string())?;
    let anon_key =
        std::env::var("TG_DEV_CLOUD_ANON_KEY").unwrap_or_else(|_| "dev-anon-key".to_string());
    let workspace_id = std::env::var("TG_DEV_CLOUD_WORKSPACE_ID")
        .map_err(|_| "TG_DEV_CLOUD_WORKSPACE_ID is required".to_string())?;
    let subject_id = std::env::var("TG_DEV_CLOUD_SUBJECT_ID")
        .map_err(|_| "TG_DEV_CLOUD_SUBJECT_ID is required".to_string())?;
    let template_prefix = std::env::var("TG_DEV_CLOUD_TEMPLATE_PREFIX")
        .unwrap_or_else(|_| "91000000-0000-0000-0000-00000000000".to_string());
    let mut connection = database.open()?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let local_workspace_id: String = transaction
        .query_row(
            "SELECT id FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let now = chrono::Utc::now().timestamp_millis();
    if std::env::var_os("TG_DEV_CLOUD_RESET_DATA").is_some() {
        for table in [
            "report_tasks",
            "reports",
            "time_allocations",
            "time_segments",
            "time_entries",
            "unassigned_segments",
            "unassigned_sessions",
            "task_occurrences",
            "task_recurrence_rules",
            "task_daily_estimates",
            "task_status_events",
            "tasks",
            "external_bindings",
            "subjects",
            "report_templates",
        ] {
            transaction
                .execute(
                    &format!("DELETE FROM {table} WHERE workspace_id = ?1"),
                    [&local_workspace_id],
                )
                .map_err(|error| error.to_string())?;
        }
        transaction
            .execute(
                "INSERT INTO subjects(id,workspace_id,name,sort_order,created_at,updated_at,version) VALUES (?1,?2,'验收主体',10,?3,?3,1)",
                params![subject_id, local_workspace_id, now],
            )
            .map_err(|error| error.to_string())?;
        for (suffix, report_type, content) in [
            ("1", "daily", "# {{日期}}\n\n{{今日事项}}\n"),
            ("2", "weekly", "# {{日期范围}}\n\n{{每日情况}}\n"),
            ("3", "monthly", "# {{日期范围}}\n\n{{每日情况}}\n"),
        ] {
            transaction
                .execute(
                    "INSERT INTO report_templates(id,workspace_id,report_type,subject_id,content,is_builtin,created_at,updated_at,version) VALUES (?1,?2,?3,NULL,?4,1,?5,?5,1)",
                    params![format!("{template_prefix}{suffix}"), local_workspace_id, report_type, content, now],
                )
                .map_err(|error| error.to_string())?;
        }
    }
    for (key, value) in [
        ("supabase_project_url", json!(project_url)),
        ("supabase_anon_key", json!(anon_key)),
        ("cloud_workspace_id", json!(workspace_id)),
        ("storage_mode", json!("cloud")),
    ] {
        transaction
            .execute(
                "INSERT INTO device_settings(key,value_json,updated_at) VALUES (?1,?2,?3) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json,updated_at=excluded.updated_at",
                params![key, value.to_string(), now],
            )
            .map_err(|error| error.to_string())?;
    }
    let device_id: String = transaction
        .query_row(
            "SELECT id FROM devices WHERE workspace_id=?1 AND revoked_at IS NULL ORDER BY created_at LIMIT 1",
            [&local_workspace_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO local_sync_state(workspace_id,device_id,last_change_seq,last_error,auth_blocked) VALUES (?1,?2,0,NULL,0) ON CONFLICT(workspace_id) DO UPDATE SET device_id=excluded.device_id,last_error=NULL,auth_blocked=0,auth_blocked_reason=NULL,auth_blocked_at=NULL",
            params![workspace_id, device_id],
        )
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())
}

fn local_workspace_id(database: &Database) -> Result<String, String> {
    database
        .open()?
        .query_row(
            "SELECT id FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())
}

fn refresh_current_session(database: &Database) -> Result<CloudSession, String> {
    current_session_with_refresh(database, true, false)
}

fn current_session_with_refresh(
    database: &Database,
    force_refresh: bool,
    keep_existing_after_refresh_error: bool,
) -> Result<CloudSession, String> {
    let connection = database.open()?;
    let workspace_id: String = connection
        .query_row(
            "SELECT id FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let existing = session(&workspace_id)?;
    if !force_refresh
        && existing
            .expires_at
            .is_none_or(|expires_at| expires_at > now_seconds() + 60)
    {
        return Ok(existing);
    }
    let _refresh_guard = SESSION_REFRESH_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "AUTH_REQUIRED: Supabase 会话刷新锁不可用".to_string())?;
    let existing = session(&workspace_id)?;
    if !force_refresh
        && existing
            .expires_at
            .is_none_or(|expires_at| expires_at > now_seconds() + 60)
    {
        return Ok(existing);
    }
    let api = client(database)?;
    let response = match api.auth_token(
        "refresh_token",
        json!({ "refresh_token": existing.refresh_token }),
    ) {
        Ok(response) => response,
        Err(error)
            if keep_existing_after_refresh_error
                && should_keep_existing_session_after_refresh_error(&error) =>
        {
            return Ok(existing)
        }
        Err(error) => return Err(error),
    };
    if !response.status().is_success() {
        let error = map_refresh_auth_error(response);
        if keep_existing_after_refresh_error
            && should_keep_existing_session_after_refresh_error(&error)
        {
            return Ok(existing);
        }
        if should_keep_existing_session_after_refresh_error(&error) {
            return Err(error);
        }
        if is_definitive_auth_invalid(&error) {
            clear_saved_session_for_workspace(database, &workspace_id)?;
            mark_auth_blocked(database, "auth_session_revoked")?;
        }
        return Err(error);
    }
    let auth: AuthResponse = response
        .json()
        .map_err(|error| format!("NETWORK_ERROR: Supabase 刷新响应无效: {error}"))?;
    let refresh_token = if auth.refresh_token.is_empty() {
        existing.refresh_token.clone()
    } else {
        auth.refresh_token
    };
    let refreshed = CloudSession {
        user_id: auth
            .user
            .as_ref()
            .map(|user| user.id.clone())
            .unwrap_or(existing.user_id),
        email: auth.user.and_then(|user| user.email).or(existing.email),
        access_token: auth.access_token,
        refresh_token,
        expires_at: auth.expires_in.map(|seconds| now_seconds() + seconds),
    };
    save_session(database, &refreshed)?;
    validate_current_device_authorization(database, &refreshed)?;
    Ok(refreshed)
}

fn should_keep_existing_session_after_refresh_error(error: &str) -> bool {
    should_preserve_saved_session(error)
}

fn should_preserve_saved_session(error: &str) -> bool {
    error.starts_with("NETWORK_ERROR:")
        || error.starts_with("CREDENTIAL_UNAVAILABLE:")
        || error.starts_with("AUTH_UNVERIFIED:")
}

fn is_business_request_auth_error(error: &str) -> bool {
    error.starts_with("AUTH_FAILED:")
}

fn is_definitive_auth_invalid(error: &str) -> bool {
    error.starts_with("AUTH_INVALID:") || error.starts_with("DEVICE_REVOKED:")
}

fn session(workspace_id: &str) -> Result<CloudSession, String> {
    if let Some(cached) = cached_session(workspace_id) {
        return Ok(cached);
    }
    let accounts = session_account_candidates(workspace_id);
    let (serialized, from_legacy) = match read_session_credential(&accounts[0]) {
        Ok(serialized) => (serialized, false),
        Err(stable_error) if stable_error == "NO_ENTRY" => {
            (read_session_credential(&accounts[1])?, true)
        }
        Err(error) => return Err(error),
    };
    let session: CloudSession = serde_json::from_str(&serialized)
        .map_err(|error| format!("AUTH_INVALID: Supabase 会话损坏，请重新登录: {error}"))?;
    if from_legacy {
        write_session_credential(SESSION_ACCOUNT, &serialized)
            .map_err(|error| format!("CREDENTIAL_UNAVAILABLE: 无法迁移 Supabase 会话: {error}"))?;
    }
    cache_session(workspace_id, &session)?;
    Ok(session)
}

fn save_session(database: &Database, session: &CloudSession) -> Result<(), String> {
    let connection = database.open()?;
    let workspace_id: String = connection
        .query_row(
            "SELECT id FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let serialized = serde_json::to_string(session).map_err(|error| error.to_string())?;
    write_session_credential(SESSION_ACCOUNT, &serialized)
        .map_err(|error| format!("保存 Supabase 会话失败: {error}"))?;
    // Keep the legacy workspace-scoped entry so older settings views still report
    // that a session exists and existing installations can migrate gradually.
    let _ = write_session_credential(&format!("{SESSION_PROVIDER}:{workspace_id}"), &serialized);
    save_session_metadata(database, session)?;
    cache_session(&workspace_id, session)
}

#[cfg(all(not(test), windows))]
fn session_entry(account: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new_with_target(
        &format!("{SESSION_CREDENTIAL_TARGET_PREFIX}:{account}"),
        SESSION_CREDENTIAL_SERVICE,
        account,
    )
    .map_err(|error| format!("无法访问系统凭据库: {error}"))
}

#[cfg(all(not(test), not(windows)))]
fn session_entry(account: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SESSION_CREDENTIAL_SERVICE, account)
        .map_err(|error| format!("无法访问系统凭据库: {error}"))
}

#[cfg(not(test))]
fn legacy_session_entry(account: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SESSION_CREDENTIAL_SERVICE, account)
        .map_err(|error| format!("无法访问系统凭据库: {error}"))
}

fn session_account_candidates(workspace_id: &str) -> [String; 2] {
    [
        SESSION_ACCOUNT.to_string(),
        format!("{SESSION_PROVIDER}:{workspace_id}"),
    ]
}

fn read_session_credential(account: &str) -> Result<String, String> {
    #[cfg(test)]
    {
        return TEST_CREDENTIAL_STORE.with(|store| {
            let store = store.borrow();
            if store.fail_reads {
                return Err(
                    "CREDENTIAL_UNAVAILABLE: 本机凭据暂时无法读取: test failure".to_string()
                );
            }
            store
                .values
                .get(account)
                .cloned()
                .ok_or_else(|| "NO_ENTRY".to_string())
        });
    }
    #[cfg(not(test))]
    {
        let entry = session_entry(account)?;
        match entry.get_password() {
            Ok(serialized) => Ok(serialized),
            Err(keyring::Error::NoEntry) => {
                let legacy = legacy_session_entry(account)?;
                let serialized = legacy.get_password().map_err(|error| match error {
                    keyring::Error::NoEntry => "NO_ENTRY".to_string(),
                    _ => format!("CREDENTIAL_UNAVAILABLE: 本机凭据暂时无法读取: {error}"),
                })?;
                entry.set_password(&serialized).map_err(|error| {
                    format!("CREDENTIAL_UNAVAILABLE: 无法迁移 Supabase 会话: {error}")
                })?;
                let _ = legacy.delete_credential();
                Ok(serialized)
            }
            Err(error) => Err(format!(
                "CREDENTIAL_UNAVAILABLE: 本机凭据暂时无法读取: {error}"
            )),
        }
    }
}

fn write_session_credential(account: &str, serialized: &str) -> Result<(), String> {
    #[cfg(test)]
    {
        return TEST_CREDENTIAL_STORE.with(|store| {
            let mut store = store.borrow_mut();
            if store.fail_writes {
                return Err("test credential write failure".to_string());
            }
            store
                .values
                .insert(account.to_string(), serialized.to_string());
            Ok(())
        });
    }
    #[cfg(not(test))]
    {
        let entry = session_entry(account)?;
        entry
            .set_password(serialized)
            .map_err(|error| error.to_string())?;
        let persisted = entry
            .get_password()
            .map_err(|error| format!("写入后无法读取系统凭据: {error}"))?;
        if persisted != serialized {
            return Err("系统凭据写入校验失败".to_string());
        }
        Ok(())
    }
}

fn delete_session_credential(account: &str) -> Result<(), String> {
    #[cfg(test)]
    {
        return TEST_CREDENTIAL_STORE.with(|store| {
            let mut store = store.borrow_mut();
            if store.fail_deletes {
                return Err("test credential delete failure".to_string());
            }
            store.values.remove(account);
            Ok(())
        });
    }
    #[cfg(not(test))]
    {
        let mut first_error = None;
        for entry in [session_entry(account)?, legacy_session_entry(account)?] {
            match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => {}
                Err(error) if first_error.is_none() => first_error = Some(error.to_string()),
                Err(_) => {}
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

fn clear_saved_session_for_workspace(
    database: &Database,
    workspace_id: &str,
) -> Result<(), String> {
    for account in session_account_candidates(workspace_id) {
        delete_session_credential(&account)
            .map_err(|error| format!("删除 Supabase 会话失败: {error}"))?;
    }
    remove_cached_session(workspace_id);
    clear_session_metadata(database)?;
    Ok(())
}

fn clear_all_saved_sessions(database: &Database) -> Result<(), String> {
    let connection = database.open()?;
    let workspaces = connection
        .prepare("SELECT id FROM workspaces WHERE deleted_at IS NULL")
        .map_err(|error| error.to_string())?
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(connection);

    delete_session_credential(SESSION_ACCOUNT)
        .map_err(|error| format!("删除 Supabase 会话失败: {error}"))?;
    if let Some(cache) = SESSION_CACHE.get() {
        if let Ok(mut cache) = cache.lock() {
            cache.clear();
        }
    }
    for workspace_id in workspaces {
        let legacy_account = format!("{SESSION_PROVIDER}:{workspace_id}");
        delete_session_credential(&legacy_account)
            .map_err(|error| format!("删除 Supabase 会话失败: {error}"))?;
    }
    Ok(())
}

fn cached_session(workspace_id: &str) -> Option<CloudSession> {
    SESSION_CACHE
        .get()
        .and_then(|cache| cache.lock().ok())
        .and_then(|cache| cache.get(workspace_id).cloned())
}

fn cache_session(workspace_id: &str, session: &CloudSession) -> Result<(), String> {
    SESSION_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| "AUTH_REQUIRED: Supabase 会话缓存不可用".to_string())?
        .insert(workspace_id.to_string(), session.clone());
    Ok(())
}

fn remove_cached_session(workspace_id: &str) {
    if let Some(cache) = SESSION_CACHE.get() {
        if let Ok(mut cache) = cache.lock() {
            cache.remove(workspace_id);
        }
    }
}

#[cfg(test)]
pub(crate) fn clear_cached_session_for_test(database: &Database) {
    if let Ok(workspace_id) = local_workspace_id(database) {
        remove_cached_session(&workspace_id);
    }
}

#[cfg(test)]
pub(crate) fn force_cached_session_expiry_for_test(database: &Database) -> Result<(), String> {
    let workspace_id = local_workspace_id(database)?;
    let mut session = session(&workspace_id)?;
    session.expires_at = Some(0);
    cache_session(&workspace_id, &session)
}

#[cfg(test)]
pub(crate) fn install_test_session_for_database(
    database: &Database,
    user_id: &str,
) -> Result<(), String> {
    save_session(
        database,
        &CloudSession {
            user_id: user_id.to_string(),
            email: Some(format!("{user_id}@example.com")),
            access_token: format!("access-{user_id}"),
            refresh_token: format!("refresh-{user_id}"),
            expires_at: Some(now_seconds() + 3600),
        },
    )
}

fn session_snapshot(session: &CloudSession) -> CloudSessionSnapshot {
    CloudSessionSnapshot {
        signed_in: true,
        status: "authenticated".to_string(),
        reason: None,
        user_id: Some(session.user_id.clone()),
        email: session.email.clone(),
        expires_at: session.expires_at,
    }
}

fn offline_session_snapshot(
    database: &Database,
    reason: Option<String>,
) -> Result<CloudSessionSnapshot, String> {
    let metadata = session_metadata(database)?;
    Ok(CloudSessionSnapshot {
        signed_in: metadata.is_some(),
        status: if metadata.is_some() {
            "offline_saved".to_string()
        } else {
            "reauth_required".to_string()
        },
        reason,
        user_id: metadata.as_ref().map(|value| value.user_id.clone()),
        email: metadata.as_ref().and_then(|value| value.email.clone()),
        expires_at: metadata.and_then(|value| value.expires_at),
    })
}

fn reauth_required_snapshot(reason: Option<String>) -> CloudSessionSnapshot {
    CloudSessionSnapshot {
        signed_in: false,
        status: "reauth_required".to_string(),
        reason,
        user_id: None,
        email: None,
        expires_at: None,
    }
}

fn session_reason_code(error: &str) -> Option<String> {
    error
        .split_once(':')
        .map(|(code, _)| code.to_ascii_lowercase())
}

fn save_session_metadata(database: &Database, session: &CloudSession) -> Result<(), String> {
    let metadata = CloudSessionMetadata {
        user_id: session.user_id.clone(),
        email: session.email.clone(),
        expires_at: session.expires_at,
    };
    settings::update_setting(
        database,
        SettingsUpdate {
            scope: SettingsScope::Device,
            key: SESSION_METADATA_SETTING.to_string(),
            value: serde_json::to_value(metadata).map_err(|error| error.to_string())?,
        },
    )
    .map(|_| ())
}

fn session_metadata(database: &Database) -> Result<Option<CloudSessionMetadata>, String> {
    let connection = database.open()?;
    let serialized: Option<String> = connection
        .query_row(
            "SELECT value_json FROM device_settings WHERE key = ?1",
            [SESSION_METADATA_SETTING],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    serialized
        .map(|value| serde_json::from_str(&value).map_err(|error| error.to_string()))
        .transpose()
}

fn clear_session_metadata(database: &Database) -> Result<(), String> {
    database
        .open()?
        .execute(
            "DELETE FROM device_settings WHERE key = ?1",
            [SESSION_METADATA_SETTING],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub(crate) fn mark_auth_blocked(database: &Database, reason: &str) -> Result<(), String> {
    let state = storage_mode(database)?;
    let connection = database.open()?;
    let cloud_workspace_id: Option<String> = connection
        .query_row(
            "SELECT json_extract(value_json, '$') FROM device_settings WHERE key = 'cloud_workspace_id'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some(cloud_workspace_id) = cloud_workspace_id else {
        return Ok(());
    };
    let last_change_seq = connection
        .query_row(
            "SELECT last_change_seq FROM local_sync_state WHERE workspace_id = ?1",
            [&cloud_workspace_id],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(state.last_change_seq);
    connection
        .execute(
            "INSERT INTO local_sync_state(workspace_id, device_id, last_change_seq, last_error, auth_blocked, auth_blocked_reason, auth_blocked_at)
             VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6)
             ON CONFLICT(workspace_id) DO UPDATE SET auth_blocked = 1, auth_blocked_reason = excluded.auth_blocked_reason, auth_blocked_at = excluded.auth_blocked_at, last_error = excluded.last_error",
            params![cloud_workspace_id, state.device_id, last_change_seq, format!("AUTH_BLOCKED: {reason}"), reason, now_seconds() * 1000],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub(crate) fn clear_auth_blocked(database: &Database) -> Result<(), String> {
    database
        .open()?
        .execute(
            "UPDATE local_sync_state SET auth_blocked = 0, auth_blocked_reason = NULL, auth_blocked_at = NULL WHERE auth_blocked <> 0",
            [],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn validate_project_url(value: &str) -> Result<String, String> {
    let value = value.trim().trim_end_matches('/');
    let url =
        Url::parse(value).map_err(|_| "VALIDATION_ERROR: Supabase Project URL 无效".to_string())?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err("VALIDATION_ERROR: Supabase Project URL 不能包含用户名或密码".to_string());
    }
    url.host_str()
        .ok_or_else(|| "VALIDATION_ERROR: Supabase Project URL 无效".to_string())?;
    if !matches!(url.scheme(), "https" | "http") {
        return Err("VALIDATION_ERROR: Supabase Project URL 只支持 HTTP 或 HTTPS 地址".to_string());
    }
    if url.path() != "" && url.path() != "/" {
        return Err(
            "VALIDATION_ERROR: Supabase Project URL 必须填写项目根地址，不能包含路径".to_string(),
        );
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err("VALIDATION_ERROR: Supabase Project URL 不能包含查询参数或片段".to_string());
    }
    Ok(value.to_string())
}

fn is_service_role_key(value: &str) -> bool {
    if value.to_ascii_lowercase().contains("service_role") {
        return true;
    }
    let Some(payload) = value.split('.').nth(1) else {
        return false;
    };
    let Ok(decoded) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload) else {
        return false;
    };
    serde_json::from_slice::<Value>(&decoded)
        .ok()
        .and_then(|json| {
            json.get("role")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .is_some_and(|role| role == "service_role")
}

fn map_http_error(response: reqwest::blocking::Response, fallback: &str) -> String {
    let status = response.status();
    let body = response.text().unwrap_or_default();
    classify_http_error(status, &body, fallback)
}

fn map_auth_error(response: reqwest::blocking::Response, fallback: &str) -> String {
    let status = response.status();
    let body = response.text().unwrap_or_default();
    let classified = classify_http_error(status, &body, fallback);
    if (status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN)
        && classified == format!("{fallback}: Unauthorized")
    {
        return format!(
            "{fallback}: Supabase Auth 拒绝了登录请求，请确认 Project URL 是 API 根地址，anon/publishable key 属于同一个项目，且不是 Studio 地址或 service_role key"
        );
    }
    classified
}

fn map_refresh_auth_error(response: reqwest::blocking::Response) -> String {
    let status = response.status();
    let body = response.text().unwrap_or_default();
    classify_refresh_auth_error(status, &body)
}

fn classify_refresh_auth_error(status: StatusCode, body: &str) -> String {
    let parsed = serde_json::from_str::<Value>(body).ok();
    let message = sanitize_remote_error(
        &parsed
            .as_ref()
            .and_then(|json| {
                json.get("msg")
                    .or_else(|| json.get("message"))
                    .or_else(|| json.get("error_description"))
                    .and_then(Value::as_str)
            })
            .unwrap_or(body)
            .chars()
            .take(240)
            .collect::<String>(),
    );
    let code = parsed
        .as_ref()
        .and_then(|json| json.get("code").or_else(|| json.get("error_code")))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let lower = message.to_ascii_lowercase();
    let definitive = matches!(
        code.as_str(),
        "refresh_token_not_found"
            | "refresh_token_already_used"
            | "user_banned"
            | "user_not_found"
            | "session_not_found"
            | "invalid_grant"
    ) || lower.contains("invalid refresh token")
        || lower.contains("refresh token not found")
        || lower.contains("refresh token has been revoked")
        || lower.contains("user is banned")
        || lower.contains("session not found");
    if definitive {
        format!("AUTH_INVALID: {message}")
    } else if status.is_server_error()
        || matches!(
            status,
            StatusCode::REQUEST_TIMEOUT | StatusCode::TOO_MANY_REQUESTS
        )
    {
        format!("NETWORK_ERROR: {message}")
    } else {
        format!("AUTH_UNVERIFIED: {message}")
    }
}

fn classify_http_error(status: StatusCode, body: &str, fallback: &str) -> String {
    let parsed = serde_json::from_str::<Value>(body).ok();
    let message = sanitize_remote_error(
        &serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|json| {
                json.get("msg")
                    .or_else(|| json.get("message"))
                    .or_else(|| json.get("error_description"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_else(|| body.chars().take(240).collect::<String>()),
    );
    let remote_code = parsed
        .as_ref()
        .and_then(|json| json.get("code"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if let Some(storage_error) = CloudStorageError::from_http_body(body) {
        return format!("{}: {}", storage_error.code, storage_error.message);
    }
    let lower_message = message.to_ascii_lowercase();
    let planning_operation_missing = lower_message.contains("unsupported_operation")
        && (lower_message.contains("task_daily_estimate")
            || lower_message.contains("task_recurrence_rule"));
    if planning_operation_missing {
        return "CLOUD_SCHEMA_MISSING: 云端数据库版本过旧，请部署 TimeGenie 事项规划同步补丁"
            .to_string();
    }
    let code = if matches!(
        remote_code,
        "PGRST202" | "PGRST205" | "42P01" | "42703" | "42883"
    ) || lower_message.contains("could not find the function")
        || lower_message.contains("could not find the table")
        || lower_message.contains("does not exist")
    {
        "CLOUD_SCHEMA_MISSING"
    } else if remote_code == "PGRST106"
        || lower_message.contains("the schema must be one of the following")
    {
        "CLOUD_SCHEMA_NOT_EXPOSED"
    } else if remote_code == "42501" || lower_message.contains("permission denied") {
        "CLOUD_PERMISSION_DENIED"
    } else if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        "AUTH_FAILED"
    } else if status == StatusCode::CONFLICT || status == StatusCode::PRECONDITION_FAILED {
        "VERSION_CONFLICT"
    } else if status == StatusCode::REQUEST_TIMEOUT {
        "CLOUD_TIMEOUT"
    } else if status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS {
        if fallback.starts_with("AUTH_") {
            "NETWORK_ERROR"
        } else {
            "CLOUD_SERVICE_UNAVAILABLE"
        }
    } else {
        fallback
    };
    if let Some(guidance) = cloud_schema_guidance(code) {
        return format!("{code}: {guidance}");
    }
    format!("{code}: {message}")
}

fn cloud_schema_guidance(code: &str) -> Option<&'static str> {
    match code {
        "CLOUD_SCHEMA_MISSING" => Some(
            "这个 Supabase 项目还没有初始化 TimeGenie 数据结构。请在 Supabase SQL Editor 中执行仓库里的 supabase/schema.sql，然后回到这里点“重新检测”。",
        ),
        "CLOUD_SCHEMA_NOT_EXPOSED" => Some(
            "这个 Supabase 项目还没有把 timegenie schema 暴露给 API。请在项目的 API 设置里把 timegenie 加入暴露的 schema（自托管请配置 PGRST_DB_SCHEMAS 并重启 REST 服务），然后点“重新检测”。",
        ),
        _ => None,
    }
}

fn sanitize_remote_error(message: &str) -> String {
    let lower = message.to_ascii_lowercase();
    if [
        "access_token",
        "refresh_token",
        "authorization",
        "password",
        "service_role",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
        || message
            .split_whitespace()
            .any(|part| part.starts_with("eyJ") && part.matches('.').count() >= 2)
    {
        return "远程服务返回了已脱敏错误".to_string();
    }
    message.chars().take(240).collect()
}

fn local_snapshot(database: &Database, workspace_id: &str) -> Result<Value, String> {
    let mut connection = database.open()?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let snapshot = local_snapshot_from_connection(&transaction, workspace_id)?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(snapshot)
}

fn local_snapshot_from_connection(
    connection: &rusqlite::Connection,
    workspace_id: &str,
) -> Result<Value, String> {
    let mut result = serde_json::Map::new();
    for (table, sql) in [
        ("subjects", "SELECT * FROM subjects WHERE workspace_id = ?1"),
        (
            "subject_work_schedules",
            "SELECT * FROM subject_work_schedules WHERE workspace_id = ?1 ORDER BY subject_id, weekday",
        ),
        (
            "subject_work_schedule_segments",
            "SELECT * FROM subject_work_schedule_segments WHERE workspace_id = ?1 ORDER BY schedule_id, sequence_no",
        ),
        ("tasks", "SELECT * FROM tasks WHERE workspace_id = ?1 ORDER BY parent_id IS NOT NULL, created_at"),
        ("task_status_events", "SELECT * FROM task_status_events WHERE workspace_id = ?1"),
        ("task_day_plans", "SELECT * FROM task_day_plans WHERE workspace_id = ?1"),
        ("task_daily_estimates", "SELECT * FROM task_daily_estimates WHERE workspace_id = ?1"),
        ("task_recurrence_rules", "SELECT * FROM task_recurrence_rules WHERE workspace_id = ?1"),
        ("task_occurrences", "SELECT * FROM task_occurrences WHERE workspace_id = ?1"),
        ("work_days", "SELECT * FROM work_days WHERE workspace_id = ?1"),
        ("time_entries", "SELECT * FROM time_entries WHERE workspace_id = ?1"),
        ("time_segments", "SELECT * FROM time_segments WHERE workspace_id = ?1"),
        ("time_allocations", "SELECT * FROM time_allocations WHERE workspace_id = ?1"),
        ("unassigned_sessions", "SELECT * FROM unassigned_sessions WHERE workspace_id = ?1"),
        ("unassigned_segments", "SELECT * FROM unassigned_segments WHERE workspace_id = ?1"),
        ("actual_time_adjustments", "SELECT * FROM actual_time_adjustments WHERE workspace_id = ?1"),
        ("report_templates", "SELECT * FROM report_templates WHERE workspace_id = ?1"),
        ("reports", "SELECT * FROM reports WHERE workspace_id = ?1"),
        ("report_tasks", "SELECT * FROM report_tasks WHERE workspace_id = ?1"),
        ("app_settings", "SELECT * FROM app_settings WHERE workspace_id = ?1"),
        ("integration_configs", "SELECT workspace_id, provider, enabled, config_json, updated_at, version FROM integration_configs WHERE workspace_id = ?1"),
        ("external_bindings", "SELECT * FROM external_bindings WHERE workspace_id = ?1"),
    ] {
        let mut statement = connection.prepare(sql).map_err(|error| error.to_string())?;
        let column_names = statement
            .column_names()
            .into_iter()
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        let rows = statement
            .query_map([workspace_id], |row| {
                let mut object = serde_json::Map::new();
                for (index, name) in column_names.iter().enumerate() {
                    let value: rusqlite::types::Value = row.get(index)?;
                    object.insert(name.clone(), sqlite_value_to_json(value));
                }
                if table == "app_settings" {
                    parse_json_text_field(&mut object, "value_json");
                } else if table == "integration_configs" {
                    parse_json_text_field(&mut object, "config_json");
                }
                Ok(Value::Object(object))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        result.insert(table.to_string(), Value::Array(rows));
    }
    Ok(Value::Object(result))
}

fn parse_json_text_field(object: &mut serde_json::Map<String, Value>, field: &str) {
    let Some(raw) = object.get(field).and_then(Value::as_str) else {
        return;
    };
    if let Ok(parsed) = serde_json::from_str::<Value>(raw) {
        object.insert(field.to_string(), parsed);
    }
}

fn sqlite_value_to_json(value: rusqlite::types::Value) -> Value {
    match value {
        rusqlite::types::Value::Null => Value::Null,
        rusqlite::types::Value::Integer(value) => json!(value),
        rusqlite::types::Value::Real(value) => json!(value),
        rusqlite::types::Value::Text(value) => json!(value),
        rusqlite::types::Value::Blob(value) => json!(String::from_utf8_lossy(&value)),
    }
}

fn now_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[tauri::command]
pub fn storage_mode_get(
    database: tauri::State<'_, Database>,
) -> Result<StorageModeSnapshot, String> {
    storage_mode(&database)
}

#[tauri::command]
pub fn storage_mode_set_local(
    database: tauri::State<'_, Database>,
) -> Result<StorageModeSnapshot, String> {
    ensure_no_pending_cloud_work(&database)?;
    settings::update_setting(
        &database,
        SettingsUpdate {
            scope: SettingsScope::Device,
            key: "storage_mode".to_string(),
            value: json!("local"),
        },
    )?;
    crate::cloud_sync::disable_cloud_sync();
    storage_mode(&database)
}

#[tauri::command]
pub fn storage_mode_set_cloud(
    database: tauri::State<'_, Database>,
) -> Result<StorageModeSnapshot, String> {
    let connection = database.open()?;
    let protocol = read_device_setting_string(&connection, "cloud_storage_protocol")?
        .unwrap_or_else(|| crate::storage_contract::LEGACY_SYNC_PROTOCOL.to_string());
    let workspace_id = read_device_setting_string(&connection, "cloud_workspace_id")?
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "CLOUD_NOT_CONFIGURED: 尚未初始化 Supabase 工作空间".to_string())?;
    drop(connection);
    if protocol != crate::storage_contract::DIRECT_CLOUD_PROTOCOL {
        return Err("CLOUD_PROTOCOL_UNSUPPORTED: 当前云端工作空间尚未启用直连存储协议".to_string());
    }
    current_session(&database)?;
    ensure_no_pending_cloud_work(&database)?;
    settings::update_setting(
        &database,
        SettingsUpdate {
            scope: SettingsScope::Device,
            key: "storage_mode".to_string(),
            value: json!("cloud"),
        },
    )?;
    crate::cloud_sync::enter_direct_cloud_mode();
    set_direct_cloud_access_runtime(
        &workspace_id,
        DirectCloudAccessState::Connecting,
        Some("正在连接 Supabase 并加载云端数据".to_string()),
    );
    if let Err(error) = direct_cloud_workbench_bootstrap(&database) {
        let _ = settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "storage_mode".to_string(),
                value: json!("local"),
            },
        );
        crate::cloud_sync::disable_cloud_sync();
        return Err(error);
    }
    storage_mode(&database)
}

#[tauri::command]
pub fn cloud_configure(
    database: tauri::State<'_, Database>,
    request: CloudConfigureRequest,
) -> Result<StorageModeSnapshot, String> {
    configure(&database, request)?;
    storage_mode(&database)
}

#[tauri::command]
pub fn cloud_project_probe(
    database: tauri::State<'_, Database>,
) -> Result<CloudProjectProbe, String> {
    probe_cloud_project(&database)
}

#[tauri::command]
pub fn cloud_sign_in_password(
    database: tauri::State<'_, Database>,
    request: CloudSignInRequest,
) -> Result<CloudSessionSnapshot, String> {
    sign_in_password(&database, request)
}

#[tauri::command]
pub fn cloud_sign_out(database: tauri::State<'_, Database>) -> Result<StorageModeSnapshot, String> {
    sign_out(&database)?;
    settings::update_setting(
        &database,
        SettingsUpdate {
            scope: SettingsScope::Device,
            key: "storage_mode".to_string(),
            value: json!("local"),
        },
    )?;
    crate::cloud_sync::disable_cloud_sync();
    storage_mode(&database)
}

#[tauri::command]
pub fn cloud_sign_out_all(
    database: tauri::State<'_, Database>,
) -> Result<StorageModeSnapshot, String> {
    sign_out_all(&database)?;
    settings::update_setting(
        &database,
        SettingsUpdate {
            scope: SettingsScope::Device,
            key: "storage_mode".to_string(),
            value: json!("local"),
        },
    )?;
    crate::cloud_sync::disable_cloud_sync();
    storage_mode(&database)
}

#[tauri::command]
pub fn cloud_session_get(
    database: tauri::State<'_, Database>,
) -> Result<CloudSessionSnapshot, String> {
    session_snapshot_for_database(&database)
}

#[tauri::command]
pub fn cloud_workspace_bootstrap(
    database: tauri::State<'_, Database>,
) -> Result<CloudWorkspace, String> {
    workspace_bootstrap(&database)
}

#[tauri::command]
pub fn cloud_device_register(
    database: tauri::State<'_, Database>,
    request: CloudDeviceRegisterRequest,
) -> Result<CloudDevice, String> {
    register_device(&database, request)
}

#[tauri::command]
pub fn cloud_device_list(database: tauri::State<'_, Database>) -> Result<Vec<CloudDevice>, String> {
    list_devices(&database)
}

#[tauri::command]
pub fn cloud_device_revoke(
    database: tauri::State<'_, Database>,
    request: CloudDeviceRevokeRequest,
) -> Result<Vec<CloudDevice>, String> {
    revoke_device(&database, &request.device_id)
}

#[tauri::command]
pub fn cloud_sync_resume_after_reauth(
    database: tauri::State<'_, Database>,
) -> Result<StorageModeSnapshot, String> {
    resume_pending_sync(&database)
}

#[tauri::command]
pub fn storage_protocol_upgrade_preflight_get(
    database: tauri::State<'_, Database>,
) -> Result<StorageProtocolUpgradePreflight, String> {
    storage_protocol_upgrade_preflight(&database)
}

#[tauri::command]
pub fn storage_protocol_upgrade_with_drained_queue_command(
    database: tauri::State<'_, Database>,
    request: StorageProtocolUpgradeRequest,
) -> Result<StorageProtocolUpgradeResult, String> {
    storage_protocol_upgrade_with_drained_queue(&database, request)
}

#[tauri::command]
pub fn storage_protocol_upgrade_after_rescue_command(
    database: tauri::State<'_, Database>,
    request: StorageProtocolAbandonRequest,
) -> Result<StorageProtocolAbandonResult, String> {
    storage_protocol_upgrade_after_rescue(&database, request)
}

#[tauri::command]
pub fn storage_protocol_reset_to_cloud_authority_command(
    database: tauri::State<'_, Database>,
    request: StorageProtocolAuthorityResetRequest,
) -> Result<StorageProtocolAbandonResult, String> {
    storage_protocol_reset_to_cloud_authority(&database, request)
}

#[tauri::command]
pub fn cloud_operation_history_list(
    database: tauri::State<'_, Database>,
    request: CloudOperationHistoryListRequest,
) -> Result<CloudOperationHistoryPage, String> {
    operation_history_list(&database, request)
}

#[tauri::command]
pub fn cloud_operation_history_detail(
    database: tauri::State<'_, Database>,
    request: CloudOperationHistoryDetailRequest,
) -> Result<CloudOperationHistoryDetail, String> {
    operation_history_detail(&database, request)
}

#[tauri::command]
pub fn cloud_workspace_snapshot_create(
    database: tauri::State<'_, Database>,
    request: CloudWorkspaceSnapshotCreateRequest,
) -> Result<CloudWorkspaceSnapshot, String> {
    workspace_snapshot_create(&database, request)
}

#[tauri::command]
pub fn cloud_workspace_snapshot_list(
    database: tauri::State<'_, Database>,
) -> Result<Vec<CloudWorkspaceSnapshot>, String> {
    workspace_snapshot_list(&database)
}

#[tauri::command]
pub fn cloud_workspace_snapshot_set_protected(
    database: tauri::State<'_, Database>,
    request: CloudWorkspaceSnapshotProtectRequest,
) -> Result<CloudWorkspaceSnapshot, String> {
    workspace_snapshot_set_protected(&database, request)
}

#[tauri::command]
pub fn cloud_workspace_snapshot_delete(
    database: tauri::State<'_, Database>,
    request: CloudWorkspaceSnapshotIdRequest,
) -> Result<CloudWorkspaceSnapshotDeleteResult, String> {
    workspace_snapshot_delete(&database, request)
}

#[tauri::command]
pub fn cloud_workspace_snapshot_preview(
    database: tauri::State<'_, Database>,
    request: CloudWorkspaceSnapshotIdRequest,
) -> Result<CloudWorkspaceSnapshotPreview, String> {
    workspace_snapshot_preview(&database, request)
}

#[tauri::command]
pub fn cloud_workspace_snapshot_settings_get(
    database: tauri::State<'_, Database>,
) -> Result<CloudWorkspaceSnapshotSettings, String> {
    workspace_snapshot_settings_get(&database)
}

#[tauri::command]
pub fn cloud_workspace_snapshot_settings_update(
    database: tauri::State<'_, Database>,
    request: CloudWorkspaceSnapshotSettingsUpdateRequest,
) -> Result<CloudWorkspaceSnapshotSettings, String> {
    workspace_snapshot_settings_update(&database, request)
}

#[tauri::command]
pub fn cloud_workspace_restore_prepare(
    database: tauri::State<'_, Database>,
    request: CloudWorkspaceRestorePrepareRequest,
) -> Result<CloudWorkspaceRestorePrepareResult, String> {
    workspace_restore_prepare(&database, request)
}

#[tauri::command]
pub fn cloud_workspace_restore_commit(
    database: tauri::State<'_, Database>,
    request: CloudWorkspaceRestoreCommitRequest,
) -> Result<CloudWorkspaceRestoreResult, String> {
    workspace_restore_commit(&database, request)
}

#[tauri::command]
pub fn cloud_workspace_restore_cancel(
    database: tauri::State<'_, Database>,
    request: CloudWorkspaceRestoreCancelRequest,
) -> Result<Value, String> {
    workspace_restore_cancel(&database, request)
}

#[tauri::command]
pub fn storage_migration_preview(
    database: tauri::State<'_, Database>,
    request: MigrationPreviewRequest,
) -> Result<MigrationPreview, String> {
    migration_preview(&database, request)
}

#[tauri::command]
pub fn storage_copy_stage_local_to_cloud(
    database: tauri::State<'_, Database>,
) -> Result<DataCopyStageResult, String> {
    let workspace_id = storage_mode(&database)?.workspace_id;
    stage_local_data_copy(&database).map_err(|error| {
        crate::direct_cloud_observability::record_copy_failure(&workspace_id, &error);
        error
    })
}

#[tauri::command]
pub fn storage_copy_replace_cloud(
    database: tauri::State<'_, Database>,
    request: DataCopyReplaceRequest,
) -> Result<DataCopyReplaceResult, String> {
    let workspace_id = storage_mode(&database)?.workspace_id;
    replace_staged_local_data_copy(&database, request).map_err(|error| {
        crate::direct_cloud_observability::record_copy_failure(&workspace_id, &error);
        error
    })
}

#[tauri::command]
pub fn storage_copy_export_cloud_begin(
    database: tauri::State<'_, Database>,
) -> Result<DataCopyExportManifest, String> {
    let workspace_id = storage_mode(&database)?.workspace_id;
    begin_cloud_data_copy_export(&database).map_err(|error| {
        crate::direct_cloud_observability::record_copy_failure(&workspace_id, &error);
        error
    })
}

#[tauri::command]
pub fn storage_copy_export_cloud_chunk(
    database: tauri::State<'_, Database>,
    request: DataCopyExportChunkRequest,
) -> Result<DataCopyExportChunk, String> {
    get_cloud_data_copy_export_chunk(&database, request)
}

#[tauri::command]
pub fn storage_copy_export_cloud_discard(
    database: tauri::State<'_, Database>,
    request: DataCopyExportDiscardRequest,
) -> Result<Value, String> {
    discard_cloud_data_copy_export(&database, request)
}

#[tauri::command]
pub fn storage_copy_replace_local(
    database: tauri::State<'_, Database>,
    request: DataCopyReplaceLocalRequest,
) -> Result<DataCopyReplaceLocalResult, String> {
    let workspace_id = storage_mode(&database)?.workspace_id;
    replace_local_with_cloud_data_copy(&database, request).map_err(|error| {
        crate::direct_cloud_observability::record_copy_failure(&workspace_id, &error);
        error
    })
}

#[tauri::command]
pub fn storage_migration_execute(
    database: tauri::State<'_, Database>,
    request: MigrationExecuteRequest,
) -> Result<StorageModeSnapshot, String> {
    migration_execute(&database, request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recurring::{save_rule, RecurrenceSaveRequest};
    use crate::tasks::{
        create_task, set_task_daily_estimate, TaskCreateRequest, TaskDailyEstimateSetRequest,
    };
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::thread;
    use tempfile::tempdir;

    struct TestHttpResponse {
        path: &'static str,
        status: u16,
        body: &'static str,
    }

    struct TestHttpDisconnect {
        path: &'static str,
    }

    struct TestHttpServer {
        base_url: String,
        hits: Arc<Mutex<Vec<String>>>,
        handle: Option<thread::JoinHandle<()>>,
    }

    impl TestHttpServer {
        fn start(responses: Vec<TestHttpResponse>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let hits = Arc::new(Mutex::new(Vec::new()));
            let thread_hits = Arc::clone(&hits);
            let handle = thread::spawn(move || {
                for expected in responses {
                    let (mut stream, _) = listener.accept().unwrap();
                    let mut request = Vec::new();
                    let mut buffer = [0_u8; 1024];
                    loop {
                        let read = stream.read(&mut buffer).unwrap();
                        if read == 0 {
                            break;
                        }
                        request.extend_from_slice(&buffer[..read]);
                        if let Some(header_end) =
                            request.windows(4).position(|window| window == b"\r\n\r\n")
                        {
                            let headers = String::from_utf8_lossy(&request[..header_end]);
                            let chunked = headers.lines().any(|line| {
                                line.split_once(':').is_some_and(|(name, value)| {
                                    name.eq_ignore_ascii_case("transfer-encoding")
                                        && value.to_ascii_lowercase().contains("chunked")
                                })
                            });
                            let content_length = headers
                                .lines()
                                .find_map(|line| {
                                    let (name, value) = line.split_once(':')?;
                                    name.eq_ignore_ascii_case("content-length")
                                        .then(|| value.trim().parse::<usize>().ok())
                                        .flatten()
                                })
                                .unwrap_or(0);
                            let body = &request[header_end + 4..];
                            if (chunked && body.windows(5).any(|window| window == b"0\r\n\r\n"))
                                || (!chunked && body.len() >= content_length)
                            {
                                break;
                            }
                        }
                    }
                    let request_text = String::from_utf8_lossy(&request).to_string();
                    let first_line = request_text.lines().next().unwrap_or_default().to_string();
                    assert!(
                        first_line.contains(expected.path),
                        "expected request path {}, got {first_line}",
                        expected.path
                    );
                    thread_hits.lock().unwrap().push(request_text);
                    let reason = match expected.status {
                        200 => "OK",
                        204 => "No Content",
                        400 => "Bad Request",
                        401 => "Unauthorized",
                        500 => "Internal Server Error",
                        503 => "Service Unavailable",
                        _ => "Test",
                    };
                    let response = format!(
                        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        expected.status,
                        reason,
                        expected.body.len(),
                        expected.body
                    );
                    stream.write_all(response.as_bytes()).unwrap();
                }
            });
            Self {
                base_url: format!("http://{address}"),
                hits,
                handle: Some(handle),
            }
        }

        fn start_with_disconnect_then_response(
            disconnect: TestHttpDisconnect,
            response: TestHttpResponse,
        ) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let hits = Arc::new(Mutex::new(Vec::new()));
            let thread_hits = Arc::clone(&hits);
            let handle = thread::spawn(move || {
                for (index, expected_path) in
                    [disconnect.path, response.path].into_iter().enumerate()
                {
                    let (mut stream, _) = listener.accept().unwrap();
                    let mut request = Vec::new();
                    let mut buffer = [0_u8; 1024];
                    loop {
                        let read = stream.read(&mut buffer).unwrap();
                        if read == 0 {
                            break;
                        }
                        request.extend_from_slice(&buffer[..read]);
                        if let Some(header_end) =
                            request.windows(4).position(|window| window == b"\r\n\r\n")
                        {
                            let headers = String::from_utf8_lossy(&request[..header_end]);
                            let content_length = headers
                                .lines()
                                .find_map(|line| {
                                    let (name, value) = line.split_once(':')?;
                                    name.eq_ignore_ascii_case("content-length")
                                        .then(|| value.trim().parse::<usize>().ok())
                                        .flatten()
                                })
                                .unwrap_or(0);
                            if request.len() >= header_end + 4 + content_length {
                                break;
                            }
                        }
                    }
                    let request_text = String::from_utf8_lossy(&request).to_string();
                    assert!(
                        request_text
                            .lines()
                            .next()
                            .unwrap_or_default()
                            .contains(expected_path),
                        "expected request path {expected_path}"
                    );
                    thread_hits.lock().unwrap().push(request_text);
                    if index == 0 {
                        continue;
                    }
                    let body = response.body;
                    let response_text = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream.write_all(response_text.as_bytes()).unwrap();
                }
            });
            Self {
                base_url: format!("http://{address}"),
                hits,
                handle: Some(handle),
            }
        }

        fn base_url(&self) -> String {
            self.base_url.clone()
        }

        fn finish(mut self) -> Vec<String> {
            if let Some(handle) = self.handle.take() {
                handle.join().unwrap();
            }
            self.hits.lock().unwrap().clone()
        }
    }

    fn reset_test_credentials() {
        TEST_CREDENTIAL_STORE.with(|store| *store.borrow_mut() = TestCredentialStore::default());
    }

    #[test]
    fn supabase_business_requests_do_not_advertise_client_schema_generation() {
        let server = TestHttpServer::start(vec![TestHttpResponse {
            path: "/rest/v1/rpc/handshake_probe",
            status: 200,
            body: "{}",
        }]);
        let api = SupabaseClient {
            project_url: server.base_url(),
            anon_key: "anon-test-key".to_string(),
            http: Client::builder().build().unwrap(),
        };
        api.request(
            api.post(format!("{}/rest/v1/rpc/handshake_probe", api.project_url))
                .json(&json!({})),
            &test_session("schema-generation", Some(now_seconds() + 3600)),
        )
        .unwrap();
        let requests = server.finish();
        assert!(!requests[0].lines().any(|line| line
            .to_ascii_lowercase()
            .starts_with("x-timegenie-client-schema-generation:")));
    }

    fn lock_test_session_state() -> std::sync::MutexGuard<'static, ()> {
        TEST_SESSION_STATE_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn test_workspace_id(database: &Database) -> String {
        database
            .open()
            .unwrap()
            .query_row(
                "SELECT id FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn test_session(suffix: &str, expires_at: Option<i64>) -> CloudSession {
        CloudSession {
            user_id: format!("user-{suffix}"),
            email: Some(format!("{suffix}@example.com")),
            access_token: format!("access-{suffix}"),
            refresh_token: format!("refresh-{suffix}"),
            expires_at,
        }
    }

    fn configure_test_cloud_state(
        database: &Database,
        project_url: &str,
        cloud_workspace_id: &str,
    ) -> (String, String) {
        for (key, value) in [
            ("supabase_project_url", json!(project_url)),
            ("supabase_anon_key", json!("anon-test-key")),
            ("cloud_workspace_id", json!(cloud_workspace_id)),
            ("storage_mode", json!("cloud")),
        ] {
            settings::update_setting(
                database,
                SettingsUpdate {
                    scope: SettingsScope::Device,
                    key: key.to_string(),
                    value,
                },
            )
            .unwrap();
        }
        let state = storage_mode(database).unwrap();
        database
            .open()
            .unwrap()
            .execute(
                "INSERT INTO local_sync_state(workspace_id, device_id, last_change_seq) VALUES (?1, ?2, 0)",
                params![cloud_workspace_id, state.device_id],
            )
            .unwrap();
        record_direct_cloud_access_success(cloud_workspace_id);
        (state.device_id, test_workspace_id(database))
    }

    #[test]
    fn auth_logout_supports_local_global_rejection_and_network_uncertainty() {
        let _guard = lock_test_session_state();
        let server = TestHttpServer::start(vec![
            TestHttpResponse {
                path: "/auth/v1/logout?scope=local",
                status: 204,
                body: "",
            },
            TestHttpResponse {
                path: "/auth/v1/logout?scope=global",
                status: 401,
                body: "{}",
            },
            TestHttpResponse {
                path: "/auth/v1/logout?scope=others",
                status: 400,
                body: r#"{"message":"invalid scope"}"#,
            },
        ]);
        let session = test_session("logout", Some(now_seconds() + 3600));
        let api = SupabaseClient {
            project_url: server.base_url(),
            anon_key: "anon-test-key".to_string(),
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(1))
                .build()
                .unwrap(),
        };
        api.auth_logout(&session, "local").unwrap();
        api.auth_logout(&session, "global").unwrap();
        assert!(api
            .auth_logout(&session, "others")
            .unwrap_err()
            .starts_with("AUTH_LOGOUT_FAILED:"));
        assert_eq!(server.finish().len(), 3);

        let offline_api = SupabaseClient {
            project_url: "http://127.0.0.1:9".to_string(),
            anon_key: "anon-test-key".to_string(),
            http: Client::builder()
                .timeout(std::time::Duration::from_millis(200))
                .build()
                .unwrap(),
        };
        assert!(offline_api
            .auth_logout(&session, "local")
            .unwrap_err()
            .starts_with("NETWORK_ERROR:"));
    }

    #[test]
    fn business_auth_errors_retry_safe_requests_once_and_do_not_replay_streaming_bodies() {
        let _guard = lock_test_session_state();
        reset_test_credentials();
        let server = TestHttpServer::start(vec![
            TestHttpResponse {
                path: "/rest/v1/safe",
                status: 401,
                body: r#"{"message":"jwt expired"}"#,
            },
            TestHttpResponse {
                path: "/auth/v1/token?grant_type=refresh_token",
                status: 200,
                body: r#"{"access_token":"access-refreshed","refresh_token":"refresh-refreshed","expires_in":3600,"user":{"id":"user-safe","email":"safe@example.com"}}"#,
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/device_authorization_get",
                status: 200,
                body: r#"{"authorized":true}"#,
            },
            TestHttpResponse {
                path: "/rest/v1/safe",
                status: 200,
                body: "{}",
            },
            TestHttpResponse {
                path: "/rest/v1/streaming",
                status: 401,
                body: r#"{"message":"jwt expired"}"#,
            },
            TestHttpResponse {
                path: "/auth/v1/token?grant_type=refresh_token",
                status: 200,
                body: r#"{"access_token":"access-refreshed-again","refresh_token":"refresh-refreshed-again","expires_in":3600,"user":{"id":"user-safe","email":"safe@example.com"}}"#,
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/device_authorization_get",
                status: 200,
                body: r#"{"authorized":true}"#,
            },
        ]);
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("request-retry.sqlite3")).unwrap();
        let cloud_workspace_id = Uuid::now_v7().to_string();
        configure_test_cloud_state(&database, &server.base_url(), &cloud_workspace_id);
        let original = CloudSession {
            user_id: "user-safe".to_string(),
            email: Some("safe@example.com".to_string()),
            access_token: "access-original".to_string(),
            refresh_token: "refresh-original".to_string(),
            expires_at: Some(now_seconds() + 3600),
        };
        save_session(&database, &original).unwrap();
        let api = client(&database).unwrap();
        api.request_for_database(
            &database,
            api.post(format!("{}/rest/v1/safe", api.project_url))
                .json(&json!({ "value": 1 })),
            &original,
        )
        .unwrap();
        assert_eq!(
            current_session(&database).unwrap().refresh_token,
            "refresh-refreshed"
        );

        let refreshed = current_session(&database).unwrap();
        let streaming_request = api
            .post(format!("{}/rest/v1/streaming", api.project_url))
            .body(reqwest::blocking::Body::new(std::io::Cursor::new(
                b"payload".to_vec(),
            )));
        let error = api
            .request_for_database(&database, streaming_request, &refreshed)
            .unwrap_err();
        assert!(
            error.starts_with("AUTH_RETRY_REQUIRED:"),
            "unexpected streaming request error: {error}"
        );
        assert_eq!(
            current_session(&database).unwrap().refresh_token,
            "refresh-refreshed-again"
        );
        assert_eq!(server.finish().len(), 7);
    }

    #[test]
    fn business_auth_error_clears_session_only_after_definitive_refresh_rejection() {
        let _guard = lock_test_session_state();
        reset_test_credentials();
        let server = TestHttpServer::start(vec![
            TestHttpResponse {
                path: "/rest/v1/invalid",
                status: 401,
                body: r#"{"message":"jwt expired"}"#,
            },
            TestHttpResponse {
                path: "/auth/v1/token?grant_type=refresh_token",
                status: 400,
                body: r#"{"error_code":"invalid_grant","message":"session revoked"}"#,
            },
        ]);
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("request-invalid.sqlite3")).unwrap();
        let cloud_workspace_id = Uuid::now_v7().to_string();
        configure_test_cloud_state(&database, &server.base_url(), &cloud_workspace_id);
        let session = test_session("invalid", Some(now_seconds() + 3600));
        save_session(&database, &session).unwrap();
        let api = client(&database).unwrap();
        let error = api
            .request_for_database(
                &database,
                api.get(format!("{}/rest/v1/invalid", api.project_url)),
                &session,
            )
            .unwrap_err();
        assert!(error.starts_with("AUTH_INVALID:"));
        assert_eq!(
            read_session_credential(SESSION_ACCOUNT).unwrap_err(),
            "NO_ENTRY"
        );
        assert!(storage_mode(&database).unwrap().auth_blocked);
        assert_eq!(server.finish().len(), 2);
    }

    #[test]
    fn current_device_logout_preserves_local_data_and_outbox_even_when_auth_logout_fails() {
        let _guard = lock_test_session_state();
        reset_test_credentials();
        let server = TestHttpServer::start(vec![
            TestHttpResponse {
                path: "/rest/v1/rpc/device_authorization_get",
                status: 200,
                body: r#"{"authorized":true}"#,
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/device_revoke",
                status: 200,
                body: r#"{"revoked":true}"#,
            },
            TestHttpResponse {
                path: "/auth/v1/logout?scope=local",
                status: 500,
                body: r#"{"message":"response lost"}"#,
            },
        ]);
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("current-logout.sqlite3")).unwrap();
        let cloud_workspace_id = Uuid::now_v7().to_string();
        let (device_id, local_workspace_id) =
            configure_test_cloud_state(&database, &server.base_url(), &cloud_workspace_id);
        save_session(
            &database,
            &test_session("current-logout", Some(now_seconds() + 3600)),
        )
        .unwrap();
        let task_id = Uuid::now_v7().to_string();
        let operation_id = Uuid::now_v7().to_string();
        {
            let connection = database.open().unwrap();
            let subject_id: String = connection
                .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
                .unwrap();
            connection.execute(
                "INSERT INTO tasks(id, workspace_id, subject_id, title, status, source_type, sort_order, created_at, updated_at, version) VALUES (?1, ?2, ?3, '保留事项', 'open', 'manual', 10, 1, 1, 1)",
                params![task_id, local_workspace_id, subject_id],
            ).unwrap();
            connection.execute(
                "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, payload_json, state, created_at) VALUES (?1, ?2, ?3, 'task_create', 'task', ?4, '{}', 'pending', 1)",
                params![operation_id, cloud_workspace_id, device_id, task_id],
            ).unwrap();
        }
        let error = sign_out(&database).unwrap_err();
        assert!(error.starts_with("NETWORK_ERROR:"));
        assert_eq!(server.finish().len(), 3);
        assert_eq!(
            read_session_credential(SESSION_ACCOUNT).unwrap_err(),
            "NO_ENTRY"
        );
        assert_eq!(storage_mode(&database).unwrap().mode, "local");
        let connection = database.open().unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM tasks WHERE id = ?1",
                    [&task_id],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM sync_outbox WHERE operation_id = ?1",
                    [&operation_id],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT auth_blocked FROM local_sync_state WHERE workspace_id = ?1",
                    [&cloud_workspace_id],
                    |row| row.get::<_, bool>(0)
                )
                .unwrap(),
            true
        );
    }

    #[test]
    fn current_device_logout_revokes_cloud_authorization_after_switching_to_local_mode() {
        let _guard = lock_test_session_state();
        reset_test_credentials();
        let server = TestHttpServer::start(vec![
            TestHttpResponse {
                path: "/rest/v1/rpc/device_authorization_get",
                status: 200,
                body: r#"{"authorized":true}"#,
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/device_revoke",
                status: 200,
                body: r#"{"revoked":true}"#,
            },
            TestHttpResponse {
                path: "/auth/v1/logout?scope=local",
                status: 204,
                body: "",
            },
        ]);
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(
            directory
                .path()
                .join("local-mode-current-device-logout.sqlite3"),
        )
        .unwrap();
        let cloud_workspace_id = Uuid::now_v7().to_string();
        configure_test_cloud_state(&database, &server.base_url(), &cloud_workspace_id);
        save_session(
            &database,
            &test_session("local-mode-logout", Some(now_seconds() + 3600)),
        )
        .unwrap();
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "storage_mode".to_string(),
                value: json!("local"),
            },
        )
        .unwrap();

        sign_out(&database).unwrap();

        assert_eq!(server.finish().len(), 3);
        assert_eq!(
            read_session_credential(SESSION_ACCOUNT).unwrap_err(),
            "NO_ENTRY"
        );
        assert!(storage_mode(&database).unwrap().auth_blocked);
    }

    #[test]
    fn all_device_logout_stops_before_auth_when_revoke_fails_and_clears_locally_after_revoke() {
        let _guard = lock_test_session_state();
        reset_test_credentials();
        let failure_server = TestHttpServer::start(vec![TestHttpResponse {
            path: "/rest/v1/rpc/device_revoke_all",
            status: 500,
            body: r#"{"message":"transaction failed"}"#,
        }]);
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("all-logout-failure.sqlite3")).unwrap();
        let cloud_workspace_id = Uuid::now_v7().to_string();
        configure_test_cloud_state(&database, &failure_server.base_url(), &cloud_workspace_id);
        save_session(
            &database,
            &test_session("all-failure", Some(now_seconds() + 3600)),
        )
        .unwrap();
        assert!(sign_out_all(&database).is_err());
        assert_eq!(failure_server.finish().len(), 1);
        assert!(read_session_credential(SESSION_ACCOUNT).is_ok());

        reset_test_credentials();
        let success_server = TestHttpServer::start(vec![
            TestHttpResponse {
                path: "/rest/v1/rpc/device_revoke_all",
                status: 200,
                body: r#"{"revokedCount":2}"#,
            },
            TestHttpResponse {
                path: "/auth/v1/logout?scope=global",
                status: 503,
                body: r#"{"message":"response lost"}"#,
            },
        ]);
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("all-logout-success.sqlite3")).unwrap();
        let cloud_workspace_id = Uuid::now_v7().to_string();
        configure_test_cloud_state(&database, &success_server.base_url(), &cloud_workspace_id);
        save_session(
            &database,
            &test_session("all-success", Some(now_seconds() + 3600)),
        )
        .unwrap();
        assert!(sign_out_all(&database).is_err());
        assert_eq!(success_server.finish().len(), 2);
        assert_eq!(
            read_session_credential(SESSION_ACCOUNT).unwrap_err(),
            "NO_ENTRY"
        );
        let state = storage_mode(&database).unwrap();
        assert_eq!(state.mode, "local");
        assert!(state.auth_blocked);
    }

    #[test]
    fn auth_blocked_cloud_mode_rejects_new_writes_until_explicit_resume() {
        let server = TestHttpServer::start(Vec::new());
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("auth-blocked.sqlite3")).unwrap();
        let cloud_workspace_id = Uuid::now_v7().to_string();
        configure_test_cloud_state(&database, &server.base_url(), &cloud_workspace_id);
        mark_auth_blocked(&database, "device_revoked").unwrap();
        let blocked = ensure_repository_write_mode(&database).unwrap_err();
        assert!(blocked.starts_with("AUTH_BLOCKED:"));

        clear_auth_blocked(&database).unwrap();
        ensure_repository_write_mode(&database).unwrap();
        let state = storage_mode(&database).unwrap();
        assert!(!state.auth_blocked);
        assert_eq!(state.mode, "cloud");
    }

    #[test]
    fn session_snapshots_map_authenticated_offline_missing_and_credential_unavailable() {
        let _guard = lock_test_session_state();
        reset_test_credentials();
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("session-state.sqlite3")).unwrap();
        let session = test_session("snapshot", Some(now_seconds() + 3600));

        let authenticated = session_snapshot(&session);
        assert!(authenticated.signed_in);
        assert_eq!(authenticated.status, "authenticated");
        assert_eq!(authenticated.reason, None);

        save_session_metadata(&database, &session).unwrap();
        let offline =
            offline_session_snapshot(&database, Some("network_error".to_string())).unwrap();
        assert!(offline.signed_in);
        assert_eq!(offline.status, "offline_saved");
        assert_eq!(offline.email.as_deref(), Some("snapshot@example.com"));

        let missing = reauth_required_snapshot(Some("no_entry".to_string()));
        assert!(!missing.signed_in);
        assert_eq!(missing.status, "reauth_required");

        TEST_CREDENTIAL_STORE.with(|store| store.borrow_mut().fail_reads = true);
        let unavailable = session_snapshot_for_database(&database).unwrap();
        assert!(unavailable.signed_in);
        assert_eq!(unavailable.status, "offline_saved");
        assert_eq!(
            unavailable.reason.as_deref(),
            Some("credential_unavailable")
        );
    }

    #[test]
    fn credential_store_restores_migrates_rotates_and_preserves_old_record_on_write_failure() {
        let _guard = lock_test_session_state();
        reset_test_credentials();
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("credential-state.sqlite3")).unwrap();
        let workspace_id = test_workspace_id(&database);
        let original = test_session("original", Some(now_seconds() + 3600));

        save_session(&database, &original).unwrap();
        remove_cached_session(&workspace_id);
        assert_eq!(
            session(&workspace_id).unwrap().refresh_token,
            "refresh-original"
        );

        reset_test_credentials();
        remove_cached_session(&workspace_id);
        let serialized = serde_json::to_string(&original).unwrap();
        write_session_credential(&format!("{SESSION_PROVIDER}:{workspace_id}"), &serialized)
            .unwrap();
        assert_eq!(
            session(&workspace_id).unwrap().access_token,
            "access-original"
        );
        assert_eq!(
            read_session_credential(SESSION_ACCOUNT).unwrap(),
            serialized
        );

        let rotated = test_session("rotated", Some(now_seconds() + 7200));
        save_session(&database, &rotated).unwrap();
        assert_eq!(
            cached_session(&workspace_id).unwrap().refresh_token,
            "refresh-rotated"
        );
        let saved: CloudSession =
            serde_json::from_str(&read_session_credential(SESSION_ACCOUNT).unwrap()).unwrap();
        assert_eq!(saved.access_token, "access-rotated");
        assert_eq!(saved.refresh_token, "refresh-rotated");

        TEST_CREDENTIAL_STORE.with(|store| store.borrow_mut().fail_writes = true);
        let rejected = test_session("rejected", Some(now_seconds() + 10_800));
        assert!(save_session(&database, &rejected).is_err());
        TEST_CREDENTIAL_STORE.with(|store| store.borrow_mut().fail_writes = false);
        let retained: CloudSession =
            serde_json::from_str(&read_session_credential(SESSION_ACCOUNT).unwrap()).unwrap();
        assert_eq!(retained.refresh_token, "refresh-rotated");
        assert_eq!(
            cached_session(&workspace_id).unwrap().refresh_token,
            "refresh-rotated"
        );
    }

    #[test]
    fn rejects_non_https_and_service_role_key() {
        assert!(validate_project_url("ftp://example.supabase.co").is_err());
        assert!(validate_project_url("https://user:pass@example.supabase.co").is_err());
        assert!(validate_project_url("https://example.supabase.co/rest/v1").is_err());
        assert!(validate_project_url("https://example.supabase.co?apikey=abc").is_err());
        assert!(validate_project_url("https://example.supabase.co#settings").is_err());
        assert_eq!(
            validate_project_url("https://example.supabase.co/").unwrap(),
            "https://example.supabase.co"
        );
        assert_eq!(
            validate_project_url("http://example.supabase.co").unwrap(),
            "http://example.supabase.co"
        );
        assert_eq!(
            validate_project_url("http://127.0.0.1:54321/").unwrap(),
            "http://127.0.0.1:54321"
        );
        assert_eq!(
            validate_project_url("http://localhost:54321").unwrap(),
            "http://localhost:54321"
        );
        assert_eq!(
            validate_project_url("http://192.168.1.20:54321/").unwrap(),
            "http://192.168.1.20:54321"
        );
        assert_eq!(
            validate_project_url("http://10.0.0.8:54321").unwrap(),
            "http://10.0.0.8:54321"
        );
        assert_eq!(
            validate_project_url("http://supabase.local:54321").unwrap(),
            "http://supabase.local:54321"
        );
        assert!(is_service_role_key("service_role-secret"));
        let payload =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(br#"{"role":"service_role"}"#);
        assert!(is_service_role_key(&format!("header.{payload}.signature")));
    }

    #[test]
    fn cloud_schema_errors_guide_the_user_to_the_initialization_script() {
        let missing = classify_http_error(
            StatusCode::NOT_FOUND,
            r#"{"code":"PGRST205","message":"Could not find the table 'timegenie.workspaces' in the schema cache"}"#,
            "CLOUD_REQUEST_FAILED",
        );
        assert!(missing.starts_with("CLOUD_SCHEMA_MISSING:"));
        assert!(missing.contains("supabase/schema.sql"));
        assert!(!missing.contains("schema cache"));

        let invalid_function_column = classify_http_error(
            StatusCode::BAD_REQUEST,
            r#"{"code":"42703","message":"column change_seq does not exist"}"#,
            "CLOUD_REQUEST_FAILED",
        );
        assert!(invalid_function_column.starts_with("CLOUD_SCHEMA_MISSING:"));

        let unexposed = classify_http_error(
            StatusCode::NOT_ACCEPTABLE,
            r#"{"code":"PGRST106","message":"The schema must be one of the following: public, graphql_public"}"#,
            "CLOUD_REQUEST_FAILED",
        );
        assert!(unexposed.starts_with("CLOUD_SCHEMA_NOT_EXPOSED:"));
        assert!(unexposed.contains("timegenie"));
    }

    #[test]
    fn cloud_project_probe_distinguishes_uninitialized_projects() {
        let missing = classify_probe_response(
            StatusCode::NOT_FOUND,
            r#"{"code":"PGRST205","message":"Could not find the table 'timegenie.workspaces' in the schema cache"}"#,
        );
        assert_eq!(missing.state, CLOUD_PROJECT_SCHEMA_MISSING);
        assert!(missing.message.contains("supabase/schema.sql"));

        let unexposed = classify_probe_response(
            StatusCode::NOT_ACCEPTABLE,
            r#"{"code":"PGRST106","message":"The schema must be one of the following: public, graphql_public"}"#,
        );
        assert_eq!(unexposed.state, CLOUD_PROJECT_SCHEMA_NOT_EXPOSED);

        let ready = classify_probe_response(StatusCode::OK, "[]");
        assert_eq!(ready.state, CLOUD_PROJECT_READY);
        assert!(!ready.message.is_empty());

        let denied = classify_probe_response(
            StatusCode::FORBIDDEN,
            r#"{"code":"42501","message":"permission denied for table workspaces"}"#,
        );
        assert_eq!(denied.state, CLOUD_PROJECT_READY);

        let offline = classify_probe_response(
            StatusCode::GATEWAY_TIMEOUT,
            r#"{"message":"upstream timeout"}"#,
        );
        assert_eq!(offline.state, CLOUD_PROJECT_UNREACHABLE);
    }

    #[test]
    fn supabase_http_errors_distinguish_schema_and_permission_failures() {
        let missing = classify_http_error(
            StatusCode::NOT_FOUND,
            r#"{"code":"PGRST202","message":"Could not find the function timegenie.cloud_snapshot_get"}"#,
            "CLOUD_REQUEST_FAILED",
        );
        assert!(missing.starts_with("CLOUD_SCHEMA_MISSING:"));

        for entity_type in ["task_daily_estimate", "task_recurrence_rule"] {
            let planning_missing = classify_http_error(
                StatusCode::BAD_REQUEST,
                &format!(r#"{{"code":"P0001","message":"UNSUPPORTED_OPERATION: {entity_type}"}}"#),
                "CLOUD_REQUEST_FAILED",
            );
            assert!(planning_missing.starts_with("CLOUD_SCHEMA_MISSING:"));
            assert!(planning_missing.contains("事项规划同步补丁"));
            assert!(!planning_missing.starts_with("AUTH_FAILED:"));
        }

        let denied = classify_http_error(
            StatusCode::BAD_REQUEST,
            r#"{"code":"42501","message":"permission denied for table workspaces"}"#,
            "CLOUD_REQUEST_FAILED",
        );
        assert!(denied.starts_with("CLOUD_PERMISSION_DENIED:"));

        let sensitive = classify_http_error(
            StatusCode::BAD_REQUEST,
            r#"{"message":"refresh_token=secret-value"}"#,
            "CLOUD_REQUEST_FAILED",
        );
        assert!(!sensitive.contains("secret-value"));
        assert!(sensitive.contains("已脱敏"));
    }

    #[test]
    fn direct_storage_errors_keep_stable_codes_without_english_messages() {
        for code in [
            "VERSION_CONFLICT",
            "ENTITY_DELETED",
            "LEASE_LOST",
            "WORKSPACE_EPOCH_STALE",
            "DEVICE_REVOKED",
            "CLOUD_PROTOCOL_UNSUPPORTED",
        ] {
            let body = format!(r#"{{"code":"P0001","message":"{}: 中文错误说明"}}"#, code);
            let parsed = CloudStorageError::from_http_body(&body).expect("structured error");
            assert_eq!(parsed.code, code);
            assert_eq!(parsed.message, "中文错误说明");
            assert!(!parsed.retryable);
            if code == "LEASE_LOST" {
                assert!(parsed.requires_refresh);
            }

            let classified =
                classify_http_error(StatusCode::BAD_REQUEST, &body, "CLOUD_REQUEST_FAILED");
            assert_eq!(classified, format!("{code}: 中文错误说明"));
        }

        let explicit_detail = CloudStorageError::from_http_body(
            r#"{"code":"P0001","message":"请重新加载","detail":"{\"code\":\"VERSION_CONFLICT\"}"}"#,
        )
        .expect("detail code");
        assert_eq!(explicit_detail.code, "VERSION_CONFLICT");
        assert!(explicit_detail.requires_refresh);
    }

    #[test]
    fn direct_cloud_query_metadata_contract_is_stable() {
        let value = serde_json::json!({
            "metadata": {
                "workspaceId": "20000000-0000-0000-0000-000000000001",
                "deviceId": "30000000-0000-0000-0000-000000000001",
                "storageProtocol": "direct_v2",
                "workspaceEpoch": 3,
                "changeSeq": 42,
                "serverTime": "2026-10-22T18:00:00+08:00",
                "domains": ["tasks", "time"],
                "diagnostics": {"responseBytes": 512, "rpcCount": 1, "durationMs": 2.5}
            },
            "data": {"probe": "中文查询结果"}
        });
        let response: DirectCloudQueryResponse<Value> =
            serde_json::from_value(value).expect("direct query response");
        assert_eq!(response.metadata.storage_protocol, "direct_v2");
        assert_eq!(response.metadata.workspace_epoch, 3);
        assert_eq!(response.metadata.change_seq, 42);
        assert_eq!(response.metadata.domains, vec!["tasks", "time"]);
        assert_eq!(response.metadata.diagnostics.as_ref().unwrap().rpc_count, 1);
        assert_eq!(
            response
                .metadata
                .diagnostics
                .as_ref()
                .unwrap()
                .response_bytes,
            512
        );
        assert_eq!(response.data["probe"], "中文查询结果");
    }

    #[test]
    fn direct_cloud_checkpoint_tracks_latest_sequence_and_epoch_without_business_data() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("direct-checkpoint.sqlite3")).unwrap();
        let workspace_id = "20000000-0000-0000-0000-0000000000c1";
        let metadata = |change_seq, workspace_epoch| DirectCloudQueryMetadata {
            workspace_id: workspace_id.to_string(),
            device_id: "30000000-0000-0000-0000-0000000000c1".to_string(),
            storage_protocol: crate::storage_contract::DIRECT_CLOUD_PROTOCOL.to_string(),
            workspace_epoch,
            change_seq,
            server_time: "2026-10-23T12:00:00+08:00".to_string(),
            domains: vec!["tasks".to_string()],
            writable: None,
            read_only_reason: None,
            diagnostics: None,
        };

        record_direct_cloud_checkpoint(&database, &metadata(12, 1)).unwrap();
        record_direct_cloud_checkpoint(&database, &metadata(9, 1)).unwrap();
        let same_epoch: (i64, i64) = database
            .open()
            .unwrap()
            .query_row(
                "SELECT last_change_seq, workspace_epoch FROM local_sync_state WHERE workspace_id=?1",
                [workspace_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(same_epoch, (12, 1));

        record_direct_cloud_checkpoint(&database, &metadata(3, 2)).unwrap();
        let restored_epoch: (i64, i64) = database
            .open()
            .unwrap()
            .query_row(
                "SELECT last_change_seq, workspace_epoch FROM local_sync_state WHERE workspace_id=?1",
                [workspace_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(restored_epoch, (3, 2));
    }

    #[test]
    fn direct_cloud_snapshot_creation_uses_only_the_authoritative_cloud_rpc() {
        let _guard = lock_test_session_state();
        reset_test_credentials();
        let snapshot_id = "90000000-0000-0000-0000-000000000201";
        let response_body = serde_json::to_string(&json!({
            "id": snapshot_id,
            "snapshotType": "manual",
            "name": "权威云端快照",
            "note": "不依赖本机业务缓存",
            "status": "ready",
            "formatVersion": 1,
            "baseChangeSeq": 88,
            "workspaceEpoch": 3,
            "entityCounts": {"tasks": 4},
            "groupHashes": {"work": "cloud-work-hash"},
            "contentHash": "cloud-content-hash",
            "sizeBytes": 512,
            "chunkCount": 4,
            "deviceName": "快照设备",
            "protected": false,
            "protectedAt": null,
            "expiresAt": null,
            "createdAt": "2026-10-23T12:00:00+08:00",
            "completedAt": "2026-10-23T12:00:01+08:00",
            "failureReason": null
        }))
        .unwrap();
        let server = TestHttpServer::start(vec![TestHttpResponse {
            path: "/rest/v1/rpc/workspace_snapshot_create",
            status: 200,
            body: Box::leak(response_body.into_boxed_str()),
        }]);
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("direct-snapshot.sqlite3")).unwrap();
        let cloud_workspace_id = "20000000-0000-0000-0000-0000000000f2";
        configure_test_cloud_state(&database, &server.base_url(), cloud_workspace_id);
        database
            .open()
            .unwrap()
            .execute_batch(
                "UPDATE local_sync_state
                   SET last_error='旧同步失败'
                 WHERE workspace_id='20000000-0000-0000-0000-0000000000f2';
                 INSERT INTO sync_outbox(
                   operation_id,workspace_id,device_id,operation_type,entity_type,entity_id,
                   payload_json,state,created_at,error_json
                 )
                 SELECT
                   '40000000-0000-0000-0000-0000000000f2',
                   '20000000-0000-0000-0000-0000000000f2',
                   json_extract(value_json,'$'),
                   'legacy_probe','task','60000000-0000-0000-0000-0000000000f2',
                   '{}','failed',1,'{\"message\":\"旧同步队列故障\"}'
                 FROM device_settings WHERE key='device_id';
                 INSERT INTO device_settings(key,value_json,updated_at)
                 VALUES('cloud_storage_protocol','\"direct_v2\"',1)
                 ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json;",
            )
            .unwrap();
        cache_session(
            cloud_workspace_id,
            &test_session("direct-snapshot", Some(now_seconds() + 3600)),
        )
        .unwrap();

        let manifest: Value =
            serde_json::from_str(crate::storage_contract::STORAGE_BOUNDARIES_JSON).unwrap();
        let connection = database.open().unwrap();
        for table in manifest["sqliteTables"]["localBusiness"]
            .as_array()
            .unwrap()
        {
            let table = table.as_str().unwrap();
            connection
                .execute_batch(&format!("ALTER TABLE {table} RENAME TO blocked_{table}"))
                .unwrap();
        }
        drop(connection);

        let snapshot = workspace_snapshot_create(
            &database,
            CloudWorkspaceSnapshotCreateRequest {
                name: "权威云端快照".to_string(),
                note: Some("不依赖本机业务缓存".to_string()),
                snapshot_type: Some("manual".to_string()),
                lazy: Some(false),
            },
        )
        .unwrap();
        assert_eq!(snapshot.id, snapshot_id);
        assert_eq!(snapshot.base_change_seq, 88);
        assert_eq!(snapshot.workspace_epoch, 3);
        assert_eq!(snapshot.entity_counts["tasks"], 4);

        let requests = server.finish();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].contains("workspace_snapshot_create"));
        assert!(requests[0].contains(cloud_workspace_id));
        assert!(requests[0].contains("权威云端快照"));
    }

    #[test]
    fn direct_cloud_restore_updates_only_device_checkpoint_and_skips_legacy_local_state() {
        let _guard = lock_test_session_state();
        reset_test_credentials();
        let snapshot_id = "90000000-0000-0000-0000-000000000301";
        let lock_token = "90000000-0000-0000-0000-000000000302";
        let prepare_body = serde_json::to_string(&json!({
            "lockToken": lock_token,
            "targetSnapshotId": snapshot_id,
            "targetContentHash": "restore-content-hash",
            "preparedEpoch": 4,
            "safetySnapshotId": null,
            "expiresAt": "2026-10-23T12:10:00+08:00",
            "preview": {
                "snapshotId": snapshot_id,
                "restorable": true,
                "errorCode": null
            }
        }))
        .unwrap();
        let commit_body = serde_json::to_string(&json!({
            "restored": true,
            "targetSnapshotId": snapshot_id,
            "safetySnapshotId": null,
            "previousEpoch": 4,
            "workspaceEpoch": 5,
            "resetChangeSeq": 99,
            "localCacheRebuilt": false
        }))
        .unwrap();
        let server = TestHttpServer::start(vec![
            TestHttpResponse {
                path: "/rest/v1/rpc/workspace_restore_prepare",
                status: 200,
                body: Box::leak(prepare_body.into_boxed_str()),
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/workspace_restore_commit",
                status: 200,
                body: Box::leak(commit_body.into_boxed_str()),
            },
        ]);
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("direct-restore.sqlite3")).unwrap();
        let cloud_workspace_id = "20000000-0000-0000-0000-0000000000f3";
        configure_test_cloud_state(&database, &server.base_url(), cloud_workspace_id);
        let device_id = read_device_setting_string(&database.open().unwrap(), "device_id")
            .unwrap()
            .unwrap();
        database
            .open()
            .unwrap()
            .execute_batch(&format!(
                "UPDATE local_sync_state SET workspace_epoch=4,last_change_seq=41,last_error='旧同步故障'
                 WHERE workspace_id='{cloud_workspace_id}';
                 INSERT INTO sync_outbox(
                   operation_id,workspace_id,device_id,operation_type,entity_type,entity_id,
                   payload_json,state,created_at,error_json,workspace_epoch
                 ) VALUES(
                   '40000000-0000-0000-0000-0000000000f3','{cloud_workspace_id}','{device_id}',
                   'legacy_probe','task','60000000-0000-0000-0000-0000000000f3',
                   '{{}}','failed',1,'{{\"message\":\"旧同步队列故障\"}}',4
                 );
                 INSERT INTO device_settings(key,value_json,updated_at)
                 VALUES('cloud_storage_protocol','\"direct_v2\"',1)
                 ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json;"
            ))
            .unwrap();
        cache_session(
            cloud_workspace_id,
            &test_session("direct-restore", Some(now_seconds() + 3600)),
        )
        .unwrap();

        let manifest: Value =
            serde_json::from_str(crate::storage_contract::STORAGE_BOUNDARIES_JSON).unwrap();
        let connection = database.open().unwrap();
        for table in manifest["sqliteTables"]["localBusiness"]
            .as_array()
            .unwrap()
        {
            let table = table.as_str().unwrap();
            connection
                .execute_batch(&format!("ALTER TABLE {table} RENAME TO blocked_{table}"))
                .unwrap();
        }
        drop(connection);

        let prepared = workspace_restore_prepare(
            &database,
            CloudWorkspaceRestorePrepareRequest {
                snapshot_id: snapshot_id.to_string(),
                create_safety_snapshot: false,
            },
        )
        .unwrap();
        let restored = workspace_restore_commit(
            &database,
            CloudWorkspaceRestoreCommitRequest {
                lock_token: prepared.lock_token,
                target_snapshot_id: prepared.target_snapshot_id,
                target_content_hash: prepared.target_content_hash,
                prepared_epoch: prepared.prepared_epoch,
            },
        )
        .unwrap();
        assert!(restored.restored);
        assert!(!restored.local_cache_rebuilt);
        let connection = database.open().unwrap();
        let checkpoint: (i64, i64, Option<String>) = connection
            .query_row(
                "SELECT workspace_epoch,last_change_seq,last_error FROM local_sync_state WHERE workspace_id=?1",
                [cloud_workspace_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(checkpoint, (5, 99, None));
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM sync_outbox WHERE workspace_id=?1 AND state='failed'",
                    [cloud_workspace_id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
        drop(connection);

        let requests = server.finish();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].contains("workspace_restore_prepare"));
        assert!(requests[1].contains("workspace_restore_commit"));
        assert!(!requests
            .iter()
            .any(|request| request.contains("cloud_snapshot_get")));
    }

    #[test]
    fn direct_cloud_access_state_machine_classifies_failures_and_recovers() {
        let workspace_id = format!("workspace-access-{}", Uuid::now_v7());
        assert_eq!(
            direct_cloud_access_state_for_error("NETWORK_ERROR: dns failed").0,
            DirectCloudAccessState::OfflineReadonly
        );
        assert_eq!(
            direct_cloud_access_state_for_error("CLOUD_TIMEOUT: request timed out").0,
            DirectCloudAccessState::DegradedReadonly
        );
        assert_eq!(
            direct_cloud_access_state_for_error("CLOUD_SERVICE_UNAVAILABLE: upstream failed").0,
            DirectCloudAccessState::DegradedReadonly
        );
        assert_eq!(
            direct_cloud_access_state_for_error("DEVICE_REVOKED: revoked").0,
            DirectCloudAccessState::ReauthRequired
        );
        assert!(
            direct_cloud_access_state_for_error("CLOUD_SCHEMA_MISSING: missing")
                .1
                .contains("CLOUD_SCHEMA_MISSING")
        );

        record_direct_cloud_access_error(&workspace_id, "NETWORK_ERROR: dns failed");
        assert_eq!(
            direct_cloud_access_runtime(&workspace_id).unwrap().state,
            DirectCloudAccessState::OfflineReadonly
        );
        record_direct_cloud_access_error(&workspace_id, "CLOUD_TIMEOUT: request timed out");
        assert_eq!(
            direct_cloud_access_runtime(&workspace_id).unwrap().state,
            DirectCloudAccessState::DegradedReadonly
        );
        record_direct_cloud_access_success(&workspace_id);
        assert_eq!(
            direct_cloud_access_runtime(&workspace_id).unwrap().state,
            DirectCloudAccessState::OnlineWritable
        );
    }

    #[test]
    fn direct_cloud_mutation_guard_blocks_readonly_states_without_local_writes() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("direct-readonly-guard.sqlite3"))
                .unwrap();
        let workspace_id = format!("workspace-guard-{}", Uuid::now_v7());
        for (key, value) in [
            ("storage_mode", json!("cloud")),
            ("cloud_workspace_id", json!(workspace_id.clone())),
        ] {
            settings::update_setting(
                &database,
                SettingsUpdate {
                    scope: SettingsScope::Device,
                    key: key.to_string(),
                    value,
                },
            )
            .unwrap();
        }
        database
            .open()
            .unwrap()
            .execute(
                "INSERT INTO device_settings(key,value_json,updated_at) VALUES('cloud_storage_protocol',?1,1) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json",
                [json!(crate::storage_contract::DIRECT_CLOUD_PROTOCOL).to_string()],
            )
            .unwrap();
        let before = crate::storage_contract::business_data_fingerprint(&database).unwrap();

        for (state, expected_reason) in [
            (DirectCloudAccessState::Connecting, "正在连接"),
            (DirectCloudAccessState::OfflineReadonly, "网络不可达"),
            (DirectCloudAccessState::DegradedReadonly, "服务异常"),
            (DirectCloudAccessState::ReauthRequired, "重新登录"),
        ] {
            set_direct_cloud_access_runtime(
                &workspace_id,
                state,
                Some(expected_reason.to_string()),
            );
            let error = ensure_direct_cloud_writable(&database, &workspace_id).unwrap_err();
            assert!(error.starts_with("CLOUD_READ_ONLY:"));
            assert!(error.contains(expected_reason));
            assert_eq!(
                before,
                crate::storage_contract::business_data_fingerprint(&database).unwrap()
            );
        }

        record_direct_cloud_access_success(&workspace_id);
        ensure_direct_cloud_writable(&database, &workspace_id).unwrap();
    }

    #[test]
    fn server_and_timeout_http_errors_keep_distinct_access_semantics() {
        assert!(classify_http_error(
            StatusCode::SERVICE_UNAVAILABLE,
            r#"{"message":"upstream unavailable"}"#,
            "CLOUD_REQUEST_FAILED",
        )
        .starts_with("CLOUD_SERVICE_UNAVAILABLE:"));
        assert!(classify_http_error(
            StatusCode::REQUEST_TIMEOUT,
            r#"{"message":"request timeout"}"#,
            "CLOUD_REQUEST_FAILED",
        )
        .starts_with("CLOUD_TIMEOUT:"));
    }

    #[test]
    fn direct_cloud_faults_never_create_pending_work_and_only_requery_restores_writes() {
        let _guard = lock_test_session_state();
        reset_test_credentials();
        let cloud_workspace_id = Uuid::now_v7().to_string();
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("direct-fault-injection.sqlite3"))
                .unwrap();

        let unavailable = TestHttpServer::start(vec![TestHttpResponse {
            path: "/rest/v1/rpc/direct_cloud_workbench_bootstrap",
            status: 503,
            body: r#"{"message":"upstream unavailable"}"#,
        }]);
        let (device_id, _) =
            configure_test_cloud_state(&database, &unavailable.base_url(), &cloud_workspace_id);
        database
            .open()
            .unwrap()
            .execute(
                "INSERT INTO device_settings(key,value_json,updated_at) VALUES('cloud_storage_protocol',?1,1) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json",
                [serde_json::to_string(crate::storage_contract::DIRECT_CLOUD_PROTOCOL).unwrap()],
            )
            .unwrap();
        save_session(
            &database,
            &test_session("direct-faults", Some(now_seconds() + 3600)),
        )
        .unwrap();
        let context = direct_cloud_read_context(&database).unwrap();
        let before = crate::storage_contract::business_data_fingerprint(&database).unwrap();

        let error = direct_cloud_query::<Value>(
            &database,
            "direct_cloud_workbench_bootstrap",
            &context,
            json!({}),
        )
        .unwrap_err();
        assert!(error.starts_with("CLOUD_SERVICE_UNAVAILABLE:"));
        assert_eq!(
            direct_cloud_access_runtime(&cloud_workspace_id)
                .unwrap()
                .state,
            DirectCloudAccessState::DegradedReadonly
        );
        assert!(direct_cloud_mutation_with_context::<Value>(
            &database,
            &context,
            "direct_cloud_subject_create",
            Some(Uuid::now_v7().to_string()),
            json!({"id": Uuid::now_v7().to_string(), "name": "故障期间不应写入"}),
        )
        .unwrap_err()
        .starts_with("CLOUD_READ_ONLY:"));
        assert_eq!(
            before,
            crate::storage_contract::business_data_fingerprint(&database).unwrap()
        );
        unavailable.finish();

        let timeout = TestHttpServer::start(vec![TestHttpResponse {
            path: "/rest/v1/rpc/direct_cloud_workbench_bootstrap",
            status: 408,
            body: r#"{"message":"request timeout"}"#,
        }]);
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "supabase_project_url".to_string(),
                value: json!(timeout.base_url()),
            },
        )
        .unwrap();
        assert!(direct_cloud_query::<Value>(
            &database,
            "direct_cloud_workbench_bootstrap",
            &context,
            json!({}),
        )
        .unwrap_err()
        .starts_with("CLOUD_TIMEOUT:"));
        assert_eq!(
            direct_cloud_access_runtime(&cloud_workspace_id)
                .unwrap()
                .state,
            DirectCloudAccessState::DegradedReadonly
        );
        assert!(direct_cloud_mutation_with_context::<Value>(
            &database,
            &context,
            "direct_cloud_subject_create",
            Some(Uuid::now_v7().to_string()),
            json!({"id": Uuid::now_v7().to_string(), "name": "超时期间不应写入"}),
        )
        .unwrap_err()
        .starts_with("CLOUD_READ_ONLY:"));
        assert_eq!(
            before,
            crate::storage_contract::business_data_fingerprint(&database).unwrap()
        );
        timeout.finish();

        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "supabase_project_url".to_string(),
                value: json!("http://127.0.0.1:9"),
            },
        )
        .unwrap();
        assert!(direct_cloud_query::<Value>(
            &database,
            "direct_cloud_workbench_bootstrap",
            &context,
            json!({}),
        )
        .unwrap_err()
        .starts_with("NETWORK_ERROR:"));
        assert_eq!(
            direct_cloud_access_runtime(&cloud_workspace_id)
                .unwrap()
                .state,
            DirectCloudAccessState::OfflineReadonly
        );
        assert!(direct_cloud_mutation_with_context::<Value>(
            &database,
            &context,
            "direct_cloud_subject_create",
            Some(Uuid::now_v7().to_string()),
            json!({"id": Uuid::now_v7().to_string(), "name": "断网期间不应写入"}),
        )
        .unwrap_err()
        .starts_with("CLOUD_READ_ONLY:"));
        assert_eq!(
            before,
            crate::storage_contract::business_data_fingerprint(&database).unwrap()
        );

        assert_eq!(
            direct_cloud_access_state_for_error("NETWORK_ERROR: dns lookup failed").0,
            DirectCloudAccessState::OfflineReadonly
        );
        record_direct_cloud_access_error(
            &cloud_workspace_id,
            "AUTH_INVALID: refresh token revoked",
        );
        assert_eq!(
            direct_cloud_access_runtime(&cloud_workspace_id)
                .unwrap()
                .state,
            DirectCloudAccessState::ReauthRequired
        );
        assert!(direct_cloud_mutation_with_context::<Value>(
            &database,
            &context,
            "direct_cloud_subject_create",
            Some(Uuid::now_v7().to_string()),
            json!({"id": Uuid::now_v7().to_string(), "name": "会话失效期间不应写入"}),
        )
        .unwrap_err()
        .starts_with("CLOUD_READ_ONLY:"));
        assert_eq!(
            before,
            crate::storage_contract::business_data_fingerprint(&database).unwrap()
        );

        let recovery_value = json!({
            "metadata": {
                "workspaceId": cloud_workspace_id,
                "deviceId": device_id,
                "storageProtocol": crate::storage_contract::DIRECT_CLOUD_PROTOCOL,
                "workspaceEpoch": context.workspace_epoch,
                "changeSeq": 7,
                "serverTime": "2026-10-23T10:00:00Z",
                "domains": []
            },
            "data": {}
        });
        let mut emergency_value = recovery_value.clone();
        emergency_value["metadata"]["writable"] = json!(false);
        emergency_value["metadata"]["readOnlyReason"] = json!("发布保护演练");
        let emergency_body = Box::leak(emergency_value.to_string().into_boxed_str());
        let recovery_body = Box::leak(recovery_value.to_string().into_boxed_str());
        let recovery = TestHttpServer::start(vec![
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_workbench_bootstrap",
                status: 200,
                body: emergency_body,
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_workbench_bootstrap",
                status: 200,
                body: recovery_body,
            },
        ]);
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "supabase_project_url".to_string(),
                value: json!(recovery.base_url()),
            },
        )
        .unwrap();
        assert!(direct_cloud_mutation_with_context::<Value>(
            &database,
            &context,
            "direct_cloud_subject_create",
            Some(Uuid::now_v7().to_string()),
            json!({"id": Uuid::now_v7().to_string(), "name": "重查前仍不应写入"}),
        )
        .is_err());
        assert_eq!(
            before,
            crate::storage_contract::business_data_fingerprint(&database).unwrap()
        );
        direct_cloud_query::<Value>(
            &database,
            "direct_cloud_workbench_bootstrap",
            &context,
            json!({}),
        )
        .unwrap();
        let emergency = direct_cloud_access_runtime(&cloud_workspace_id).unwrap();
        assert_eq!(emergency.state, DirectCloudAccessState::DegradedReadonly);
        assert_eq!(emergency.reason.as_deref(), Some("发布保护演练"));
        assert!(direct_cloud_mutation_with_context::<Value>(
            &database,
            &context,
            "direct_cloud_subject_create",
            Some(Uuid::now_v7().to_string()),
            json!({"id": Uuid::now_v7().to_string(), "name": "紧急停写期间不应写入"}),
        )
        .unwrap_err()
        .contains("发布保护演练"));
        assert_eq!(
            before,
            crate::storage_contract::business_data_fingerprint(&database).unwrap()
        );
        direct_cloud_query::<Value>(
            &database,
            "direct_cloud_workbench_bootstrap",
            &context,
            json!({}),
        )
        .unwrap();
        recovery.finish();
        assert_eq!(
            direct_cloud_access_runtime(&cloud_workspace_id)
                .unwrap()
                .state,
            DirectCloudAccessState::OnlineWritable
        );
        ensure_direct_cloud_writable(&database, &cloud_workspace_id).unwrap();
        assert_eq!(
            before,
            crate::storage_contract::business_data_fingerprint(&database).unwrap()
        );

        let metrics = crate::direct_cloud_observability::snapshot(&cloud_workspace_id);
        assert_eq!(metrics.metrics.query_count, 5);
        assert_eq!(metrics.metrics.query_error_count, 3);
        assert_eq!(metrics.metrics.mutation_error_count, 6);
        assert!(metrics.metrics.query_latency_max_ms > 0.0);
        assert!(metrics.average_query_latency_ms > 0.0);
    }

    #[test]
    fn direct_cloud_workbench_contract_matches_the_rpc_payload() {
        let value = serde_json::json!({
            "metadata": {
                "workspaceId": "20000000-0000-0000-0000-00000000000a",
                "deviceId": "30000000-0000-0000-0000-00000000000a",
                "storageProtocol": "direct_v2",
                "workspaceEpoch": 4,
                "changeSeq": 101,
                "serverTime": "2026-10-23T08:00:00+14:00",
                "domains": ["subjects", "tasks"],
                "diagnostics": {"responseBytes": 2048, "rpcCount": 1, "durationMs": 4.0}
            },
            "data": {
                "workspaceId": "20000000-0000-0000-0000-00000000000a",
                "workspaceName": "直连协议测试工作空间",
                "timezone": "Pacific/Kiritimati",
                "currentWorkDate": "2026-10-23",
                "currentDayStartAt": 1792663200000_i64,
                "currentDayEndAt": 1792749600000_i64,
                "subjects": [{
                    "id": "51000000-0000-0000-0000-0000000000a1",
                    "name": "直连主体一",
                    "sortOrder": 10,
                    "openTaskCount": 1,
                    "version": 2
                }],
                "summary": {
                    "subjectCount": 1,
                    "openTaskCount": 1,
                    "completedTaskCount": 0,
                    "taskCount": 1
                },
                "device": {
                    "id": "30000000-0000-0000-0000-00000000000a",
                    "deviceName": "直连协议测试设备",
                    "platform": "windows",
                    "appVersion": "0.4.6",
                    "lastSeenAt": "2026-10-23T08:00:00+14:00",
                    "authorizedAt": "2026-10-22T08:00:00+14:00",
                    "reauthorizedAt": null,
                    "writable": true
                }
            }
        });
        let response: DirectCloudWorkbenchResponse =
            serde_json::from_value(value).expect("direct workbench response");
        assert_eq!(response.data.current_work_date, "2026-10-23");
        assert_eq!(response.data.subjects[0].open_task_count, 1);
        assert_eq!(response.data.summary.task_count, 1);
        assert!(response.data.device.writable);
    }

    #[test]
    fn direct_cloud_query_adapters_match_local_fixture_without_business_table_reads() {
        let _guard = lock_test_session_state();
        reset_test_credentials();
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("direct-query-adapter.sqlite3")).unwrap();
        let subject_id: String = database
            .open()
            .unwrap()
            .query_row(
                "SELECT id FROM subjects WHERE deleted_at IS NULL ORDER BY sort_order LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let created = crate::tasks::create_task(
            &database,
            crate::tasks::TaskCreateRequest {
                subject_id: subject_id.clone(),
                parent_id: None,
                day_instance_date: None,
                title: "固定契约事项".to_string(),
                planned_date: Some("2026-10-22".to_string()),
                estimate_minutes: Some(45),
                note: Some("本地与云端一致".to_string()),
                project_name: Some("直连迁移".to_string()),
                solution_name: Some("读取适配器".to_string()),
            },
        )
        .unwrap();
        let local_fixture = crate::tasks::list_tasks(
            &database,
            crate::tasks::TaskListRequest {
                subject_id: Some(subject_id.clone()),
                include_completed: Some(false),
                planned_date: None,
                today_date: None,
                today_only: None,
                daily_estimate_date: None,
                query: Some("固定契约".to_string()),
            },
        )
        .unwrap();
        assert_eq!(local_fixture.tasks.len(), 1);
        assert_eq!(local_fixture.tasks[0].id, created.id);

        let cloud_workspace_id = "20000000-0000-0000-0000-0000000000f8";
        let metadata = |domains: Value| {
            json!({
                "workspaceId": cloud_workspace_id,
                "deviceId": "device-placeholder",
                "storageProtocol": "direct_v2",
                "workspaceEpoch": 1,
                "changeSeq": local_fixture.revision,
                "serverTime": "2026-10-22T18:00:00+08:00",
                "domains": domains,
                "diagnostics": {"responseBytes": 1024, "rpcCount": 1, "durationMs": 1.0},
            })
        };
        let leak_response = |data: Value, domains: Value| -> &'static str {
            Box::leak(
                serde_json::to_string(&json!({"metadata": metadata(domains), "data": data}))
                    .unwrap()
                    .into_boxed_str(),
            )
        };
        let task_json = serde_json::to_value(&local_fixture.tasks[0]).unwrap();
        let server = TestHttpServer::start(vec![
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_task_list",
                status: 200,
                body: leak_response(json!({"tasks": [task_json.clone()]}), json!(["tasks"])),
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_workbench_bootstrap",
                status: 200,
                body: leak_response(
                    json!({
                        "workspaceId": cloud_workspace_id,
                        "workspaceName": "直连工作空间",
                        "timezone": "Asia/Shanghai",
                        "currentWorkDate": "2026-10-22",
                        "currentDayStartAt": 1792598400000_i64,
                        "currentDayEndAt": 1792684800000_i64,
                        "subjects": [],
                        "summary": {"subjectCount":0,"openTaskCount":0,"completedTaskCount":0,"taskCount":0},
                        "device": {"id":"device-placeholder","deviceName":"测试设备","platform":"windows","appVersion":"0.4.6","lastSeenAt":"2026-10-22T18:00:00+08:00","authorizedAt":"2026-10-22T17:00:00+08:00","reauthorizedAt":null,"writable":true}
                    }),
                    json!(["subjects", "tasks"]),
                ),
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_completed_tasks",
                status: 200,
                body: leak_response(
                    json!({"tasks":[],"hasMore":false,"nextCursor":null}),
                    json!(["tasks"]),
                ),
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_task_detail",
                status: 200,
                body: leak_response(json!({"task":task_json,"children":[]}), json!(["tasks"])),
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_today_plan",
                status: 200,
                body: leak_response(
                    json!({"subjectId":subject_id,"workDate":"2026-10-22","members":[],"contexts":[],"plans":[],"estimates":[],"recurrenceRules":[],"occurrences":[],"instances":[]}),
                    json!(["planning", "tasks"]),
                ),
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_time_state",
                status: 200,
                body: leak_response(
                    json!({"workDate":"2026-10-22","activeEntry":null,"entries":[],"unassigned":null}),
                    json!(["time", "unassigned"]),
                ),
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_time_history_page",
                status: 200,
                body: leak_response(
                    json!({"page":1,"pageSize":7,"totalDays":0,"totalPages":0,"groups":[],"hasMore":false,"nextCursor":null}),
                    json!(["time", "unassigned"]),
                ),
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_report_list",
                status: 200,
                body: leak_response(
                    json!({"reports":[],"hasMore":false,"nextCursor":null,"typeCounts":{"all":0,"daily":0,"weekly":0,"monthly":0}}),
                    json!(["reports"]),
                ),
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_report_detail",
                status: 200,
                body: leak_response(
                    json!({"report":{"id":"report-1"},"tasks":[]}),
                    json!(["reports"]),
                ),
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_report_templates",
                status: 200,
                body: leak_response(json!({"templates":[]}), json!(["reports"])),
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_report_preview",
                status: 200,
                body: leak_response(
                    json!({"reportType":"daily","referenceDate":"2026-10-22","periodStart":"2026-10-22","periodEnd":"2026-10-22","subjectId":subject_id,"subjectName":"主体","template":{},"context":{},"markdown":"# 日报"}),
                    json!(["reports", "tasks", "time"]),
                ),
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_shared_configuration",
                status: 200,
                body: leak_response(
                    json!({"workspace":{},"settings":{},"settingVersions":{},"workDays":[],"subjectWorkSchedules":[],"integrations":[],"bindings":[]}),
                    json!(["settings", "calendar", "integrations"]),
                ),
            },
        ]);
        let (device_id, _) =
            configure_test_cloud_state(&database, &server.base_url(), cloud_workspace_id);
        database
            .open()
            .unwrap()
            .execute(
                "INSERT INTO device_settings(key,value_json,updated_at) VALUES('cloud_storage_protocol','\"direct_v2\"',1) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json",
                [],
            )
            .unwrap();
        cache_session(
            cloud_workspace_id,
            &test_session("direct-query-adapter", Some(now_seconds() + 3600)),
        )
        .unwrap();

        let manifest: Value =
            serde_json::from_str(crate::storage_contract::STORAGE_BOUNDARIES_JSON).unwrap();
        let connection = database.open().unwrap();
        for table in manifest["sqliteTables"]["localBusiness"]
            .as_array()
            .unwrap()
        {
            let table = table.as_str().unwrap();
            connection
                .execute_batch(&format!("ALTER TABLE {table} RENAME TO blocked_{table}"))
                .unwrap();
        }
        drop(connection);

        let cloud_fixture = direct_cloud_task_list(
            &database,
            DirectCloudTaskQuery {
                subject_id: Some(subject_id.clone()),
                include_completed: Some(false),
                planned_date: None,
                query: Some("固定契约".to_string()),
            },
        )
        .unwrap()
        .into_task_list_result();
        assert_eq!(
            serde_json::to_value(&cloud_fixture.tasks).unwrap(),
            serde_json::to_value(&local_fixture.tasks).unwrap()
        );
        assert_eq!(cloud_fixture.revision, local_fixture.revision);

        let workbench = direct_cloud_workbench_bootstrap(&database).unwrap();
        assert_eq!(workbench.data.workspace_name, "直连工作空间");
        assert!(
            direct_cloud_completed_tasks(&database, DirectCloudCompletedTasksQuery::default())
                .unwrap()
                .data
                .tasks
                .is_empty()
        );
        assert_eq!(
            direct_cloud_task_detail(&database, created.id.clone())
                .unwrap()
                .data
                .task
                .id,
            created.id
        );
        assert_eq!(
            direct_cloud_today_plan(&database, subject_id.clone(), "2026-10-22".to_string())
                .unwrap()
                .data
                .work_date,
            "2026-10-22"
        );
        assert!(direct_cloud_time_state(&database, "2026-10-22".to_string())
            .unwrap()
            .data
            .entries
            .is_empty());
        assert!(direct_cloud_time_history_for_command(
            &database,
            crate::time_tracking::TimeEntryHistoryRequest {
                page: 1,
                include_breaks: true,
            },
        )
        .unwrap()
        .groups
        .is_empty());
        assert!(
            direct_cloud_report_list(&database, DirectCloudReportListQuery::default())
                .unwrap()
                .data
                .reports
                .is_empty()
        );
        assert_eq!(
            direct_cloud_report_detail(&database, "report-1".to_string())
                .unwrap()
                .data
                .report["id"],
            "report-1"
        );
        assert!(direct_cloud_report_templates(&database, None)
            .unwrap()
            .data
            .templates
            .is_empty());
        assert_eq!(
            direct_cloud_report_preview(
                &database,
                DirectCloudReportPreviewQuery {
                    report_type: "daily".to_string(),
                    reference_date: "2026-10-22".to_string(),
                    subject_id,
                    template_id: None
                }
            )
            .unwrap()
            .data
            .markdown,
            "# 日报"
        );
        assert!(direct_cloud_shared_configuration(&database)
            .unwrap()
            .data
            .integrations
            .is_empty());

        let requests = server.finish();
        assert_eq!(requests.len(), 12);
        for request in requests {
            assert!(request.contains(&format!("\"p_workspace_id\":\"{cloud_workspace_id}\"")));
            assert!(request.contains(&format!("\"p_device_id\":\"{device_id}\"")));
            assert!(request.contains("\"p_expected_epoch\":1"));
        }
    }

    #[test]
    fn direct_report_and_settings_routes_skip_local_business_tables_and_outbox() {
        let _guard = lock_test_session_state();
        reset_test_credentials();
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(
            directory
                .path()
                .join("direct-report-settings-routing.sqlite3"),
        )
        .unwrap();
        let cloud_workspace_id = "20000000-0000-0000-0000-0000000000e8";
        let subject_id = "51000000-0000-0000-0000-0000000000e8";
        let report_id = "72000000-0000-0000-0000-0000000000e8";
        let operation_id = "4f000000-0000-0000-0000-0000000000e8";
        let metadata = |domains: Value| {
            json!({
                "workspaceId": cloud_workspace_id,
                "deviceId": "device-placeholder",
                "storageProtocol": "direct_v2",
                "workspaceEpoch": 1,
                "changeSeq": 21,
                "serverTime": "2026-10-23T18:00:00+08:00",
                "domains": domains,
                "diagnostics": {"responseBytes": 512, "rpcCount": 1, "durationMs": 1.0},
            })
        };
        let query_response = |data: Value, domains: Value| -> &'static str {
            Box::leak(
                serde_json::to_string(&json!({"metadata": metadata(domains), "data": data}))
                    .unwrap()
                    .into_boxed_str(),
            )
        };
        let report = json!({
            "id": report_id,
            "reportType": "daily",
            "subjectId": subject_id,
            "subjectName": "直连主体",
            "periodStart": "2026-10-23",
            "periodEnd": "2026-10-23",
            "referenceDate": "2026-10-23",
            "taskIds": [],
            "templateId": null,
            "templateName": null,
            "templateArchived": false,
            "markdown": "# 直连日报",
            "contentSource": "generated",
            "generatedCount": 1,
            "updatedAt": 1792778400000_i64,
            "version": 1
        });
        let shared_data = json!({
            "workspace": {"id": cloud_workspace_id, "name": "直连工作空间", "timezone": "Asia/Shanghai", "version": 3},
            "settings": {"user_name": "晓健", "timezone": "Asia/Shanghai"},
            "settingVersions": {"user_name": 2, "timezone": 1},
            "workDays": [],
            "subjectWorkSchedules": [],
            "integrations": [],
            "bindings": []
        });
        let server = TestHttpServer::start(vec![
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_report_create",
                status: 200,
                body: Box::leak(
                    serde_json::to_string(&json!({
                        "operationId": operation_id,
                        "metadata": metadata(json!(["reports"])),
                        "data": {"report": report}
                    }))
                    .unwrap()
                    .into_boxed_str(),
                ),
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_shared_configuration",
                status: 200,
                body: query_response(shared_data, json!(["settings", "calendar", "integrations"])),
            },
        ]);
        configure_test_cloud_state(&database, &server.base_url(), cloud_workspace_id);
        database
            .open()
            .unwrap()
            .execute(
                "INSERT INTO device_settings(key,value_json,updated_at) VALUES('cloud_storage_protocol','\"direct_v2\"',1) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json",
                [],
            )
            .unwrap();
        cache_session(
            cloud_workspace_id,
            &test_session("direct-report-settings", Some(now_seconds() + 3600)),
        )
        .unwrap();
        let outbox_before: i64 = database
            .open()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM sync_outbox", [], |row| row.get(0))
            .unwrap();

        let manifest: Value =
            serde_json::from_str(crate::storage_contract::STORAGE_BOUNDARIES_JSON).unwrap();
        let connection = database.open().unwrap();
        for table in manifest["sqliteTables"]["localBusiness"]
            .as_array()
            .unwrap()
        {
            let table = table.as_str().unwrap();
            connection
                .execute_batch(&format!("ALTER TABLE {table} RENAME TO blocked_{table}"))
                .unwrap();
        }
        drop(connection);

        let created = crate::storage_contract::route(
            &database,
            crate::storage_contract::BusinessDomain::Reports,
            crate::storage_contract::StorageOperation::Mutation,
            |_| panic!("sqlite report adapter must not run for direct_v2"),
            |database| {
                crate::reports::direct_report_create_for_command(
                    database,
                    crate::reports::ReportCreateRequest {
                        report_type: "daily".to_string(),
                        reference_date: "2026-10-23".to_string(),
                        subject_id: subject_id.to_string(),
                        task_ids: vec![],
                        template_id: None,
                        client_request_id: operation_id.to_string(),
                    },
                )
            },
        )
        .unwrap();
        assert_eq!(created.markdown, "# 直连日报");

        let settings = crate::storage_contract::route(
            &database,
            crate::storage_contract::BusinessDomain::SharedSettingsIntegrations,
            crate::storage_contract::StorageOperation::Query,
            |_| panic!("sqlite settings adapter must not run for direct_v2"),
            crate::settings::direct_settings_snapshot,
        )
        .unwrap();
        assert_eq!(settings.shared["user_name"], json!("晓健"));
        assert_eq!(settings.workspace_id, cloud_workspace_id);
        assert_eq!(settings.device_id, "device-placeholder");
        let outbox_after: i64 = database
            .open()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM sync_outbox", [], |row| row.get(0))
            .unwrap();
        assert_eq!(outbox_before, outbox_after);

        let requests = server.finish();
        assert_eq!(requests.len(), 2);
        assert!(!requests.iter().any(|request| request.contains("blocked_")));
    }

    #[test]
    fn direct_schedule_and_integration_writes_do_not_touch_local_business_tables() {
        let _guard = lock_test_session_state();
        reset_test_credentials();
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(
            directory
                .path()
                .join("direct-schedule-integration-routing.sqlite3"),
        )
        .unwrap();
        let cloud_workspace_id = "20000000-0000-0000-0000-0000000000e7";
        let subject_id = "51000000-0000-0000-0000-0000000000e7";
        let days = json!([
            {"id":"83000000-0000-0000-0000-000000000001","weekday":0,"enabled":true,"segments":[{"start":"09:00","end":"12:00"}],"version":2},
            {"id":"83000000-0000-0000-0000-000000000002","weekday":1,"enabled":true,"segments":[{"start":"09:00","end":"12:00"}],"version":1},
            {"id":"83000000-0000-0000-0000-000000000003","weekday":2,"enabled":true,"segments":[{"start":"09:00","end":"12:00"}],"version":1},
            {"id":"83000000-0000-0000-0000-000000000004","weekday":3,"enabled":true,"segments":[{"start":"09:00","end":"12:00"}],"version":1},
            {"id":"83000000-0000-0000-0000-000000000005","weekday":4,"enabled":true,"segments":[{"start":"09:00","end":"12:00"}],"version":1},
            {"id":"83000000-0000-0000-0000-000000000006","weekday":5,"enabled":false,"segments":[],"version":1},
            {"id":"83000000-0000-0000-0000-000000000007","weekday":6,"enabled":false,"segments":[],"version":1}
        ]);
        let metadata = |domains: Value| {
            json!({
                "workspaceId": cloud_workspace_id,
                "deviceId": "device-placeholder",
                "storageProtocol": "direct_v2",
                "workspaceEpoch": 1,
                "changeSeq": 22,
                "serverTime": "2026-10-23T18:00:00+08:00",
                "domains": domains,
                "diagnostics": {"responseBytes": 512, "rpcCount": 1, "durationMs": 1.0},
            })
        };
        let query_response = |data: Value, domains: Value| -> &'static str {
            Box::leak(
                serde_json::to_string(&json!({"metadata": metadata(domains), "data": data}))
                    .unwrap()
                    .into_boxed_str(),
            )
        };
        let mutation_response = |data: Value, domains: Value| -> &'static str {
            Box::leak(
                serde_json::to_string(&json!({
                    "operationId": "4f000000-0000-0000-0000-0000000000e7",
                    "metadata": metadata(domains),
                    "data": data,
                }))
                .unwrap()
                .into_boxed_str(),
            )
        };
        let shared_data = |version: i64| {
            json!({
                "workspace": {"id": cloud_workspace_id, "timezone": "Asia/Shanghai", "version": 2},
                "settings": {"user_name": "晓健"},
                "settingVersions": {"user_name": 1},
                "workDays": [],
                "subjectWorkSchedules": [{"subjectId": subject_id, "days": days.clone()}],
                "integrations": [{"provider": "seatable", "enabled": true, "config": {"serverUrl": "https://cloud.seatable.cn"}, "version": version, "requiresDeviceCredential": true}],
                "bindings": []
            })
        };
        let server = TestHttpServer::start(vec![
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_subject_schedule_save",
                status: 200,
                body: mutation_response(
                    json!({"schedule": {"subjectId": subject_id, "days": days.clone()}, "batch": true}),
                    json!(["settings", "calendar"]),
                ),
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_shared_configuration",
                status: 200,
                body: query_response(
                    shared_data(3),
                    json!(["settings", "calendar", "integrations"]),
                ),
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_integration_config_save",
                status: 200,
                body: mutation_response(
                    json!({
                        "changed": true,
                        "integration": {
                            "provider": "seatable", "enabled": true,
                            "config": {"serverUrl": "https://example.invalid"},
                            "version": 4, "requiresDeviceCredential": true
                        }
                    }),
                    json!(["settings", "integrations"]),
                ),
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_shared_configuration",
                status: 200,
                body: query_response(
                    shared_data(4),
                    json!(["settings", "calendar", "integrations"]),
                ),
            },
        ]);
        configure_test_cloud_state(&database, &server.base_url(), cloud_workspace_id);
        database
            .open()
            .unwrap()
            .execute(
                "INSERT INTO device_settings(key,value_json,updated_at) VALUES('cloud_storage_protocol','\"direct_v2\"',1) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json",
                [],
            )
            .unwrap();
        cache_session(
            cloud_workspace_id,
            &test_session("direct-schedule-integration", Some(now_seconds() + 3600)),
        )
        .unwrap();
        let outbox_before: i64 = database
            .open()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM sync_outbox", [], |row| row.get(0))
            .unwrap();
        let manifest: Value =
            serde_json::from_str(crate::storage_contract::STORAGE_BOUNDARIES_JSON).unwrap();
        let connection = database.open().unwrap();
        for table in manifest["sqliteTables"]["localBusiness"]
            .as_array()
            .unwrap()
        {
            let table = table.as_str().unwrap();
            connection
                .execute_batch(&format!("ALTER TABLE {table} RENAME TO blocked_{table}"))
                .unwrap();
        }
        drop(connection);

        let saved = crate::storage_contract::route(
            &database,
            crate::storage_contract::BusinessDomain::SharedSettingsIntegrations,
            crate::storage_contract::StorageOperation::Mutation,
            |_| panic!("sqlite schedule adapter must not run for direct_v2"),
            |database| {
                crate::subject_work_schedules::direct_schedule_save_for_command(
                    database,
                    crate::subject_work_schedules::WorkScheduleSaveRequest {
                        subject_id: subject_id.to_string(),
                        days: serde_json::from_value(days.clone()).unwrap(),
                    },
                )
            },
        )
        .unwrap();
        assert_eq!(saved.len(), 7);
        assert_eq!(saved[0].segments[0].start, "09:00");

        let settings = crate::settings::direct_update_integration_config_for_command(
            &database,
            crate::settings::IntegrationConfigRequest {
                provider: "seatable".to_string(),
                enabled: true,
                config: json!({"serverUrl": "https://example.invalid"}),
                expected_version: Some(3),
            },
        )
        .unwrap();
        assert_eq!(settings.integrations["seatable"].version, 4);
        let outbox_after: i64 = database
            .open()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM sync_outbox", [], |row| row.get(0))
            .unwrap();
        assert_eq!(outbox_before, outbox_after);
        assert_eq!(server.finish().len(), 4);
    }

    #[test]
    fn second_device_stale_report_template_calendar_and_binding_writes_reload_authority() {
        let _guard = lock_test_session_state();
        reset_test_credentials();
        let directory = tempdir().unwrap();
        let device_a =
            Database::initialize_at(directory.path().join("direct-integration-a.sqlite3")).unwrap();
        let device_b =
            Database::initialize_at(directory.path().join("direct-integration-b.sqlite3")).unwrap();
        let cloud_workspace_id = "20000000-0000-0000-0000-0000000000ea";
        let report_id = "a1000000-0000-0000-0000-0000000000ea";
        let template_id = "a2000000-0000-0000-0000-0000000000ea";
        let subject_id = "51000000-0000-0000-0000-0000000000ea";
        let binding_task_id = "61000000-0000-0000-0000-0000000000ea";
        let metadata = |domains: Value| {
            json!({
                "workspaceId": cloud_workspace_id,
                "deviceId": "device-placeholder",
                "storageProtocol": "direct_v2",
                "workspaceEpoch": 1,
                "changeSeq": 12,
                "serverTime": "2026-10-23T18:00:00+08:00",
                "domains": domains,
                "diagnostics": {"responseBytes": 512, "rpcCount": 1, "durationMs": 1.0},
            })
        };
        let query_body = |data: Value, domains: Value| -> &'static str {
            Box::leak(
                serde_json::to_string(&json!({"metadata": metadata(domains), "data": data}))
                    .unwrap()
                    .into_boxed_str(),
            )
        };
        let mutation_body = |data: Value, domains: Value| -> &'static str {
            Box::leak(
                serde_json::to_string(&json!({
                    "operationId": "70000000-0000-0000-0000-0000000000ea",
                    "metadata": metadata(domains),
                    "data": data,
                }))
                .unwrap()
                .into_boxed_str(),
            )
        };
        let report_record = |markdown: &str, version: i64| {
            json!({
                "id": report_id,
                "reportType": "daily",
                "subjectId": subject_id,
                "subjectName": "晓健",
                "periodStart": "2026-10-23",
                "periodEnd": "2026-10-23",
                "referenceDate": "2026-10-23",
                "taskIds": [],
                "templateId": null,
                "templateName": null,
                "templateArchived": false,
                "markdown": markdown,
                "contentSource": "manual",
                "generatedCount": 0,
                "updatedAt": 1792622400000i64,
                "version": version,
            })
        };
        let template_record = |version: i64| {
            json!({
                "id": template_id,
                "reportType": "daily",
                "name": "标准日报",
                "description": null,
                "content": "# 日报",
                "isDefault": true,
                "isBuiltin": false,
                "sortOrder": 10,
                "deletedAt": null,
                "version": version,
            })
        };
        let schedule_days = |first_version: i64| {
            (0..7)
                .map(|weekday| {
                    json!({
                        "id": format!("83000000-0000-0000-0000-0000000000{:02}", weekday),
                        "weekday": weekday,
                        "enabled": weekday < 5,
                        "segments": if weekday < 5 {
                            json!([{"start": "09:00", "end": "12:00"}])
                        } else {
                            json!([])
                        },
                        "version": if weekday == 0 { first_version } else { 1 },
                    })
                })
                .collect::<Vec<_>>()
        };
        let binding_record = |version: i64| {
            json!({
                "provider": "seatable",
                "entityType": "task",
                "entityId": binding_task_id,
                "externalId": "row-device-a",
                "version": version,
            })
        };
        let version_conflict =
            r#"{"code":"P0001","message":"VERSION_CONFLICT: 数据已被其他设备修改"}"#;
        let server = TestHttpServer::start(vec![
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_report_content_save",
                status: 200,
                body: mutation_body(
                    json!({"report": report_record("设备 A 报告", 2)}),
                    json!(["reports"]),
                ),
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_report_content_save",
                status: 400,
                body: version_conflict,
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_report_detail",
                status: 200,
                body: query_body(
                    json!({"report": report_record("设备 A 报告", 2), "tasks": []}),
                    json!(["reports"]),
                ),
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_report_template_set_default",
                status: 200,
                body: mutation_body(json!({"template": template_record(2)}), json!(["reports"])),
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_report_template_set_default",
                status: 400,
                body: version_conflict,
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_report_templates",
                status: 200,
                body: query_body(
                    json!({"templates": [template_record(2)]}),
                    json!(["reports"]),
                ),
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_subject_schedule_save",
                status: 200,
                body: mutation_body(
                    json!({"schedule": {"subjectId": subject_id, "days": schedule_days(2)}}),
                    json!(["settings", "calendar"]),
                ),
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_subject_schedule_save",
                status: 400,
                body: version_conflict,
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_shared_configuration",
                status: 200,
                body: query_body(
                    json!({
                        "workspace": {"id": cloud_workspace_id, "timezone": "Asia/Shanghai", "version": 2},
                        "settings": {},
                        "settingVersions": {},
                        "workDays": [],
                        "subjectWorkSchedules": [{"subjectId": subject_id, "days": schedule_days(2)}],
                        "integrations": [],
                        "bindings": [],
                    }),
                    json!(["settings", "calendar"]),
                ),
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_external_binding_upsert",
                status: 200,
                body: mutation_body(
                    json!({"changed": true, "binding": binding_record(2)}),
                    json!(["integrations"]),
                ),
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_external_binding_upsert",
                status: 400,
                body: version_conflict,
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_shared_configuration",
                status: 200,
                body: query_body(
                    json!({
                        "workspace": {"id": cloud_workspace_id, "timezone": "Asia/Shanghai", "version": 2},
                        "settings": {},
                        "settingVersions": {},
                        "workDays": [],
                        "subjectWorkSchedules": [],
                        "integrations": [],
                        "bindings": [binding_record(2)],
                    }),
                    json!(["integrations"]),
                ),
            },
        ]);
        for database in [&device_a, &device_b] {
            configure_test_cloud_state(database, &server.base_url(), cloud_workspace_id);
            database
                .open()
                .unwrap()
                .execute(
                    "INSERT INTO device_settings(key,value_json,updated_at) VALUES('cloud_storage_protocol',?1,1) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json",
                    [serde_json::to_string("direct_v2").unwrap()],
                )
                .unwrap();
            cache_session(
                cloud_workspace_id,
                &test_session("direct-integration-race", Some(now_seconds() + 3600)),
            )
            .unwrap();
        }
        let before_a = crate::storage_contract::business_data_fingerprint(&device_a).unwrap();
        let before_b = crate::storage_contract::business_data_fingerprint(&device_b).unwrap();

        let saved = crate::reports::direct_report_content_save_for_command(
            &device_a,
            crate::reports::ReportContentSaveRequest {
                report_id: report_id.to_string(),
                markdown: "设备 A 报告".to_string(),
                expected_version: 1,
            },
        )
        .unwrap();
        assert_eq!(saved.version, 2);
        let stale_report = crate::reports::direct_report_content_save_for_command(
            &device_b,
            crate::reports::ReportContentSaveRequest {
                report_id: report_id.to_string(),
                markdown: "设备 B 陈旧报告".to_string(),
                expected_version: 1,
            },
        )
        .unwrap_err();
        assert!(stale_report.starts_with("VERSION_CONFLICT:"));
        let latest_report = crate::reports::direct_report_from_value(
            direct_cloud_report_detail(&device_b, report_id.to_string())
                .unwrap()
                .data
                .report,
        )
        .unwrap();
        assert_eq!(latest_report.markdown, "设备 A 报告");
        assert_eq!(latest_report.version, 2);

        let template = crate::reports::direct_template_set_default_for_command(
            &device_a,
            crate::reports::ReportTemplateSetDefaultRequest {
                template_id: template_id.to_string(),
                expected_version: 1,
            },
        )
        .unwrap();
        assert_eq!(template.version, 2);
        let stale_template = crate::reports::direct_template_set_default_for_command(
            &device_b,
            crate::reports::ReportTemplateSetDefaultRequest {
                template_id: template_id.to_string(),
                expected_version: 1,
            },
        )
        .unwrap_err();
        assert!(stale_template.starts_with("VERSION_CONFLICT:"));
        let latest_templates =
            crate::reports::direct_template_list_for_command(&device_b, Some("daily".to_string()))
                .unwrap();
        assert!(latest_templates[0].is_default);
        assert_eq!(latest_templates[0].version, 2);

        let days = schedule_days(1)
            .into_iter()
            .map(|day| serde_json::from_value(day).unwrap())
            .collect::<Vec<crate::subject_work_schedules::WorkScheduleDayDto>>();
        let saved_days = crate::subject_work_schedules::direct_schedule_save_for_command(
            &device_a,
            crate::subject_work_schedules::WorkScheduleSaveRequest {
                subject_id: subject_id.to_string(),
                days: days.clone(),
            },
        )
        .unwrap();
        assert_eq!(saved_days[0].version, 2);
        let stale_schedule = crate::subject_work_schedules::direct_schedule_save_for_command(
            &device_b,
            crate::subject_work_schedules::WorkScheduleSaveRequest {
                subject_id: subject_id.to_string(),
                days,
            },
        )
        .unwrap_err();
        assert!(stale_schedule.starts_with("VERSION_CONFLICT:"));
        let latest_days =
            crate::subject_work_schedules::direct_schedule_get_for_command(&device_b, subject_id)
                .unwrap();
        assert_eq!(latest_days[0].version, 2);
        assert!(latest_days[0].enabled);

        let binding_payload = |expected_version: i64| {
            json!({
                "provider": "seatable",
                "entityType": "task",
                "entityId": binding_task_id,
                "externalId": "row-device-a",
                "expectedVersion": expected_version,
            })
        };
        direct_cloud_external_binding_upsert_mutation(
            &device_a,
            "71000000-0000-0000-0000-0000000000ea".to_string(),
            binding_payload(1),
        )
        .unwrap();
        let stale_binding = direct_cloud_external_binding_upsert_mutation(
            &device_b,
            "71000000-0000-0000-0000-0000000000eb".to_string(),
            binding_payload(1),
        )
        .unwrap_err();
        assert!(stale_binding.starts_with("VERSION_CONFLICT:"));
        let latest_configuration = direct_cloud_shared_configuration(&device_b).unwrap();
        assert_eq!(latest_configuration.data.bindings[0]["version"], json!(2));
        assert_eq!(
            latest_configuration.data.bindings[0]["externalId"],
            json!("row-device-a")
        );

        for database in [&device_a, &device_b] {
            let pending: i64 = database
                .open()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM sync_outbox WHERE state IN ('pending','sending','failed','conflict')",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(pending, 0);
        }
        assert_eq!(
            before_a,
            crate::storage_contract::business_data_fingerprint(&device_a).unwrap()
        );
        assert_eq!(
            before_b,
            crate::storage_contract::business_data_fingerprint(&device_b).unwrap()
        );
        assert_eq!(server.finish().len(), 12);
    }

    #[test]
    fn direct_timer_lease_conflict_loads_authority_without_local_recovery() {
        let _guard = lock_test_session_state();
        reset_test_credentials();
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("direct-timer-lease.sqlite3")).unwrap();
        let cloud_workspace_id = "20000000-0000-0000-0000-0000000000f9";
        let active_entry_id = "62000000-0000-0000-0000-0000000000fa";
        let metadata = json!({
            "workspaceId": cloud_workspace_id,
            "deviceId": "device-placeholder",
            "storageProtocol": "direct_v2",
            "workspaceEpoch": 1,
            "changeSeq": 7,
            "serverTime": "2026-10-22T18:00:00+08:00",
            "domains": ["time"],
            "diagnostics": {"responseBytes": 512, "rpcCount": 1, "durationMs": 1.0},
        });
        let active_timer_body: &'static str = Box::leak(
            serde_json::to_string(&json!({
                "metadata": metadata,
                "data": {
                    "activeEntry": {
                        "id": active_entry_id,
                        "workDate": "2026-10-22",
                        "label": "租赁冲突计时",
                        "startedAt": 1792650000000_i64,
                        "endedAt": null,
                        "durationSeconds": 120,
                        "settlementMinutes": 2,
                        "allocatedMinutes": 0,
                        "defaultTaskId": null,
                        "note": null,
                        "kind": "work",
                        "sourceType": "timer",
                        "state": "running",
                        "version": 4,
                        "timerChainId": null,
                        "previousEntryId": null,
                        "splitBoundaryAt": null,
                        "allocations": []
                    }
                }
            }))
            .unwrap()
            .into_boxed_str(),
        );
        let server = TestHttpServer::start(vec![
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_timer_pause",
                status: 400,
                body: r#"{"code":"P0001","message":"LEASE_LOST: 当前计时由其他设备持有"}"#,
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_active_timer",
                status: 200,
                body: active_timer_body,
            },
        ]);
        configure_test_cloud_state(&database, &server.base_url(), cloud_workspace_id);
        database
            .open()
            .unwrap()
            .execute(
                "INSERT INTO device_settings(key,value_json,updated_at) VALUES('cloud_storage_protocol','\"direct_v2\"',1) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json",
                [],
            )
            .unwrap();
        cache_session(
            cloud_workspace_id,
            &test_session("direct-timer-lease", Some(now_seconds() + 3600)),
        )
        .unwrap();
        let local_counts = || -> (i64, i64) {
            database
                .open()
                .unwrap()
                .query_row(
                    "SELECT (SELECT COUNT(*) FROM sync_outbox),(SELECT COUNT(*) FROM time_entries)",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap()
        };
        let before = local_counts();
        let error = direct_cloud_timer_pause(
            &database,
            crate::time_tracking::TimerVersionRequest {
                entry_id: active_entry_id.to_string(),
                expected_version: 3,
            },
        )
        .unwrap_err();
        assert!(
            error.starts_with("LEASE_LOST:"),
            "unexpected error: {error}"
        );
        assert!(
            error.contains(active_entry_id),
            "missing active timer: {error}"
        );
        assert!(error.contains("版本 4"), "missing active version: {error}");
        assert_eq!(local_counts(), before);
        let requests = server.finish();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].contains("\"leaseToken\":\""));
        assert!(requests[1].contains("\"p_workspace_id\":\"20000000-0000-0000-0000-0000000000f9\""));
    }

    #[test]
    fn direct_cloud_task_domain_commands_route_to_supabase_without_local_business_writes() {
        let _guard = lock_test_session_state();
        reset_test_credentials();
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("direct-task-routing.sqlite3")).unwrap();
        let cloud_workspace_id = "20000000-0000-0000-0000-0000000000e8";
        let task_id = "60000000-0000-0000-0000-0000000000e1";
        let metadata = |domains: Value| {
            json!({
                "workspaceId": cloud_workspace_id,
                "deviceId": "device-placeholder",
                "storageProtocol": "direct_v2",
                "workspaceEpoch": 1,
                "changeSeq": 7,
                "serverTime": "2026-10-22T18:00:00+08:00",
                "domains": domains,
                "diagnostics": {"responseBytes": 512, "rpcCount": 1, "durationMs": 1.0},
            })
        };
        let task_json = json!({
            "id": task_id,
            "subjectId": "50000000-0000-0000-0000-0000000000e1",
            "parentId": null,
            "dayInstanceDate": null,
            "dayInstanceKind": null,
            "dayInstanceSourceTaskId": null,
            "title": "云端路由事项",
            "status": "done",
            "plannedDate": "2026-10-22",
            "estimateMinutes": 30,
            "note": null,
            "projectName": null,
            "solutionName": null,
            "sortOrder": 10,
            "completedAt": 1792651200000_i64,
            "depth": 0,
            "pathLabel": "云端路由事项",
            "selectable": true,
            "directMinutes": 0,
            "totalMinutes": 0,
            "executionState": "done",
            "version": 2,
        });
        let query_body = Box::leak(
            serde_json::to_string(&json!({
                "metadata": metadata(json!(["tasks"])),
                "data": {"tasks": [task_json.clone()]},
            }))
            .unwrap()
            .into_boxed_str(),
        );
        let status_body = Box::leak(
            serde_json::to_string(&json!({
                "operationId": "70000000-0000-0000-0000-0000000000e1",
                "metadata": metadata(json!(["tasks", "planning"])),
                "data": {"task": task_json},
            }))
            .unwrap()
            .into_boxed_str(),
        );
        let recurrence_body = Box::leak(
            serde_json::to_string(&json!({
                "operationId": "70000000-0000-0000-0000-0000000000e2",
                "metadata": metadata(json!(["recurrence", "tasks"])),
                "data": {"rule": {
                    "id": "71000000-0000-0000-0000-0000000000e1",
                    "taskId": task_id,
                    "frequency": "daily",
                    "weekdaysMask": null,
                    "effectiveStart": "2026-10-22",
                    "effectiveEnd": null,
                    "summary": "每天",
                    "version": 1
                }},
            }))
            .unwrap()
            .into_boxed_str(),
        );
        let batch_body = Box::leak(
            serde_json::to_string(&json!({
                "operationId": "70000000-0000-0000-0000-0000000000e3",
                "metadata": metadata(json!(["tasks", "planning", "recurrence"])),
                "data": {
                    "createdCount": 1,
                    "updatedCount": 0,
                    "deletedCount": 0,
                    "todayPlanChangedCount": 1,
                    "createdItems": [{
                        "lineKey": "new",
                        "taskId": "60000000-0000-0000-0000-0000000000e2"
                    }]
                },
            }))
            .unwrap()
            .into_boxed_str(),
        );
        let server = TestHttpServer::start(vec![
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_task_list",
                status: 200,
                body: query_body,
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_task_set_status",
                status: 200,
                body: status_body,
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_recurrence_save",
                status: 200,
                body: recurrence_body,
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_task_plan_batch_apply",
                status: 200,
                body: batch_body,
            },
        ]);
        configure_test_cloud_state(&database, &server.base_url(), cloud_workspace_id);
        database
            .open()
            .unwrap()
            .execute(
                "INSERT INTO device_settings(key,value_json,updated_at) VALUES('cloud_storage_protocol',?1,1) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json",
                [serde_json::to_string("direct_v2").unwrap()],
            )
            .unwrap();
        cache_session(
            cloud_workspace_id,
            &test_session("direct-task-routing", Some(now_seconds() + 3600)),
        )
        .unwrap();
        let before = crate::storage_contract::business_data_fingerprint(&database).unwrap();
        let sqlite_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        let listed = crate::storage_contract::route(
            &database,
            crate::storage_contract::BusinessDomain::Tasks,
            crate::storage_contract::StorageOperation::Query,
            |_| -> Result<crate::tasks::TaskListResult, String> {
                sqlite_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(crate::tasks::TaskListResult {
                    tasks: Vec::new(),
                    revision: 0,
                })
            },
            |database| {
                direct_cloud_task_list_for_command(
                    database,
                    crate::tasks::TaskListRequest {
                        subject_id: None,
                        include_completed: Some(true),
                        planned_date: Some("2026-10-22".to_string()),
                        today_date: None,
                        today_only: None,
                        daily_estimate_date: None,
                        query: None,
                    },
                )
            },
        )
        .unwrap();
        assert_eq!(listed.tasks.len(), 1);

        let status = crate::storage_contract::route(
            &database,
            crate::storage_contract::BusinessDomain::Planning,
            crate::storage_contract::StorageOperation::Mutation,
            |_| -> Result<crate::tasks::TaskDto, String> {
                sqlite_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Err("sqlite adapter must not run".to_string())
            },
            |database| {
                direct_cloud_task_status_for_command(
                    database,
                    crate::tasks::TaskStatusRequest {
                        id: task_id.to_string(),
                        expected_version: 1,
                        done: true,
                        occurrence_date: None,
                        occurrence_expected_version: None,
                    },
                )
            },
        )
        .unwrap();
        assert_eq!(status.status, "done");

        let rule = crate::storage_contract::route(
            &database,
            crate::storage_contract::BusinessDomain::Planning,
            crate::storage_contract::StorageOperation::Mutation,
            |_| -> Result<crate::recurring::RecurrenceRuleDto, String> {
                sqlite_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Err("sqlite adapter must not run".to_string())
            },
            |database| {
                direct_cloud_recurrence_save_for_command(
                    database,
                    crate::recurring::RecurrenceSaveRequest {
                        task_id: task_id.to_string(),
                        task_expected_version: 2,
                        frequency: "daily".to_string(),
                        weekdays_mask: None,
                        effective_start: "2026-10-22".to_string(),
                        rule_expected_version: None,
                    },
                )
            },
        )
        .unwrap();
        assert_eq!(rule.frequency, "daily");

        let batch = crate::storage_contract::route(
            &database,
            crate::storage_contract::BusinessDomain::Planning,
            crate::storage_contract::StorageOperation::Mutation,
            |_| -> Result<crate::tasks::TaskPlanBatchApplyResult, String> {
                sqlite_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Err("sqlite adapter must not run".to_string())
            },
            |database| {
                direct_cloud_task_plan_batch_apply_for_command(
                    database,
                    crate::tasks::TaskPlanBatchApplyRequest {
                        subject_id: "50000000-0000-0000-0000-0000000000e1".to_string(),
                        day_instance_date: None,
                        baseline_tasks: vec![crate::tasks::TaskPlanBatchBaselineItem {
                            task_id: task_id.to_string(),
                            expected_version: 2,
                        }],
                        final_items: vec![crate::tasks::TaskPlanBatchFinalItem {
                            line_key: "new".to_string(),
                            task_id: None,
                            title: "批量新增".to_string(),
                            parent_line_key: None,
                        }],
                        confirmed_delete_task_ids: Vec::new(),
                        today_plan_change: Some(crate::tasks::TaskPlanBatchTodayPlanChange {
                            work_date: "2026-10-22".to_string(),
                            include_line_keys: vec!["new".to_string()],
                            exclude_items: Vec::new(),
                        }),
                    },
                )
            },
        )
        .unwrap();
        assert_eq!(batch.created_count, 1);
        assert_eq!(sqlite_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(
            before,
            crate::storage_contract::business_data_fingerprint(&database).unwrap()
        );

        let requests = server.finish();
        assert_eq!(requests.len(), 4);
        assert!(requests[0].contains("\"p_planned_date\":\"2026-10-22\""));
        assert!(requests[1].contains("\"p_operation_id\""));
        assert!(requests[1].contains("\"expectedVersion\":1"));
        assert!(requests[2].contains("\"frequency\":\"daily\""));
        assert!(requests[3].contains("\"confirmedDeleteTaskIds\":[]"));

        database
            .open()
            .unwrap()
            .execute(
                "UPDATE device_settings SET value_json='\"sync_v1\"' WHERE key='cloud_storage_protocol'",
                [],
            )
            .unwrap();
        let legacy = crate::storage_contract::route(
            &database,
            crate::storage_contract::BusinessDomain::Tasks,
            crate::storage_contract::StorageOperation::Mutation,
            |_| Ok("sqlite"),
            |_| -> Result<&str, String> { panic!("legacy sync must use the SQLite adapter") },
        )
        .unwrap();
        assert_eq!(legacy, "sqlite");
    }

    #[test]
    fn direct_cloud_time_domain_adapters_use_authoritative_server_results_without_local_writes() {
        let _guard = lock_test_session_state();
        reset_test_credentials();
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("direct-time-routing.sqlite3")).unwrap();
        let cloud_workspace_id = "20000000-0000-0000-0000-0000000000d8";
        let task_id = "60000000-0000-0000-0000-0000000000d8";
        let entry_id = "90000000-0000-0000-0000-0000000000d8";
        let session_id = "94000000-0000-0000-0000-0000000000d8";
        let server_started_at = 1_792_656_000_000_i64;
        let metadata = |domains: Value| {
            json!({
                "workspaceId": cloud_workspace_id,
                "deviceId": "device-placeholder",
                "storageProtocol": "direct_v2",
                "workspaceEpoch": 1,
                "changeSeq": 27,
                "serverTime": "2026-10-22T18:00:00+08:00",
                "domains": domains,
                "diagnostics": {"responseBytes": 512, "rpcCount": 1, "durationMs": 1.0},
            })
        };
        let entry = json!({
            "id": entry_id,
            "workDate": "2026-10-22",
            "label": "服务器计时",
            "startedAt": server_started_at,
            "endedAt": null,
            "durationSeconds": 7200,
            "settlementMinutes": 120,
            "allocatedMinutes": 0,
            "defaultTaskId": task_id,
            "note": null,
            "kind": "work",
            "sourceType": "timer",
            "state": "running",
            "version": 4,
            "timerChainId": entry_id,
            "previousEntryId": null,
            "splitBoundaryAt": null,
            "allocations": [],
        });
        let anchor = json!({
            "sessionId": session_id,
            "workDate": "2026-10-22",
            "state": "collecting",
            "firstStartedAt": server_started_at + 7_200_000,
            "lastEndedAt": null,
            "currentSegmentStartedAt": server_started_at + 7_200_000,
            "elapsedSeconds": 600,
            "requiredMinutes": 10,
            "thresholdSeconds": 300,
            "mustResolve": true,
            "version": 2,
        });
        let list_body = Box::leak(
            serde_json::to_string(&json!({
                "metadata": metadata(json!(["time", "unassigned"])),
                "data": {"workDate":"2026-10-22","activeEntry":entry,"entries":[entry],"unassigned":null},
            }))
            .unwrap()
            .into_boxed_str(),
        );
        let history_body = Box::leak(
            serde_json::to_string(&json!({
                "metadata": metadata(json!(["time", "unassigned"])),
                "data": {"page":1,"pageSize":7,"totalDays":1,"totalPages":1,"groups":[{
                    "workDate":"2026-10-22","entries":[entry],"adjustments":[]
                }],"hasMore":false,"nextCursor":null},
            }))
            .unwrap()
            .into_boxed_str(),
        );
        let unassigned_state_body = Box::leak(
            serde_json::to_string(&json!({
                "metadata": metadata(json!(["time", "unassigned"])),
                "data": {"anchor":null,"workDate":"2026-10-22","activeTimer":false,"daySummary":{}},
            }))
            .unwrap()
            .into_boxed_str(),
        );
        let unassigned_ensure_body = Box::leak(
            serde_json::to_string(&json!({
                "operationId":"70000000-0000-0000-0000-0000000000d8",
                "metadata":metadata(json!(["time","unassigned"])),
                "data":{"created":true,"anchor":anchor,"daySummary":{},"serverTime":server_started_at + 7_800_000},
            }))
            .unwrap()
            .into_boxed_str(),
        );
        let manual_entry = json!({
            "id": entry_id, "workDate": "2026-10-22", "label": "服务器手工时间",
            "startedAt": server_started_at + 8_000_000, "endedAt": server_started_at + 8_600_000,
            "durationSeconds": 600, "settlementMinutes": 10, "allocatedMinutes": 10,
            "defaultTaskId": task_id, "note": "云端创建", "kind": "work",
            "sourceType": "manual", "state": "stopped", "version": 1,
            "timerChainId": null, "previousEntryId": null, "splitBoundaryAt": null,
            "allocations": [{
                "id": "91000000-0000-0000-0000-0000000000d8", "taskId": task_id,
                "taskTitle": "云端事项", "subjectId": "50000000-0000-0000-0000-0000000000d8",
                "minutes": 10, "note": null, "version": 1
            }],
        });
        let corrected_entry = json!({
            "id": entry_id, "workDate": "2026-10-22", "label": "服务器手工时间",
            "startedAt": server_started_at + 8_100_000, "endedAt": server_started_at + 8_700_000,
            "durationSeconds": 600, "settlementMinutes": 10, "allocatedMinutes": 10,
            "defaultTaskId": task_id, "note": "云端修正", "kind": "work",
            "sourceType": "manual", "state": "stopped", "version": 2,
            "timerChainId": null, "previousEntryId": null, "splitBoundaryAt": null,
            "allocations": [],
        });
        let break_entry = json!({
            "id": entry_id, "workDate": "2026-10-22", "label": "休息",
            "startedAt": server_started_at + 8_100_000, "endedAt": server_started_at + 8_700_000,
            "durationSeconds": 600, "settlementMinutes": 10, "allocatedMinutes": 0,
            "defaultTaskId": null, "note": "云端修正", "kind": "break",
            "sourceType": "manual", "state": "stopped", "version": 3,
            "timerChainId": null, "previousEntryId": null, "splitBoundaryAt": null,
            "allocations": [],
        });
        let adjustment_id = "95000000-0000-0000-0000-0000000000d8";
        let adjustment = json!({
            "id": adjustment_id, "subjectId": "50000000-0000-0000-0000-0000000000d8",
            "taskId": task_id, "taskTitle": "云端事项", "workDate": "2026-10-22",
            "deltaMinutes": 3, "baselineMinutes": 0, "targetMinutes": 3,
            "note": "云端校准", "batchId": "96000000-0000-0000-0000-0000000000d8",
            "createdAt": server_started_at + 9_000_000, "version": 1, "deletedAt": null,
        });
        let deleted_adjustment = json!({
            "id": adjustment_id, "subjectId": "50000000-0000-0000-0000-0000000000d8",
            "taskId": task_id, "taskTitle": "云端事项", "workDate": "2026-10-22",
            "deltaMinutes": 3, "baselineMinutes": 0, "targetMinutes": 3,
            "note": "云端校准", "batchId": "96000000-0000-0000-0000-0000000000d8",
            "createdAt": server_started_at + 9_000_000, "version": 2,
            "deletedAt": server_started_at + 9_600_000,
        });
        let mutation_body = |operation_id: &str, data: Value| {
            Box::leak(
                serde_json::to_string(&json!({
                    "operationId": operation_id,
                    "metadata": metadata(json!(["time", "tasks"])),
                    "data": data,
                }))
                .unwrap()
                .into_boxed_str(),
            ) as &'static str
        };
        let actual_snapshot_body = Box::leak(
            serde_json::to_string(&json!({
                "metadata": metadata(json!(["time", "tasks"])),
                "data": {
                    "subjectId": "50000000-0000-0000-0000-0000000000d8",
                    "workDate": "2026-10-22", "tasks": [], "adjustments": []
                },
            }))
            .unwrap()
            .into_boxed_str(),
        );
        let server = TestHttpServer::start(vec![
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_time_state",
                status: 200,
                body: list_body,
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_time_history_page",
                status: 200,
                body: history_body,
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_unassigned_state",
                status: 200,
                body: unassigned_state_body,
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_unassigned_ensure",
                status: 200,
                body: unassigned_ensure_body,
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_manual_time_create",
                status: 200,
                body: mutation_body(
                    "70000000-0000-0000-0000-0000000000d9",
                    json!({"entry":manual_entry}),
                ),
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_time_entry_update",
                status: 200,
                body: mutation_body(
                    "70000000-0000-0000-0000-0000000000da",
                    json!({"entry":corrected_entry}),
                ),
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_time_entry_disposition",
                status: 200,
                body: mutation_body(
                    "70000000-0000-0000-0000-0000000000db",
                    json!({"entry":break_entry}),
                ),
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_unassigned_resolve",
                status: 200,
                body: mutation_body(
                    "70000000-0000-0000-0000-0000000000dc",
                    json!({"session":anchor,"generatedEntry":null,"serverTime":server_started_at + 8_000_000}),
                ),
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_actual_adjustment_snapshot",
                status: 200,
                body: actual_snapshot_body,
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_actual_adjustment_batch_apply",
                status: 200,
                body: mutation_body(
                    "70000000-0000-0000-0000-0000000000dd",
                    json!({"batchId":"96000000-0000-0000-0000-0000000000d8","createdAdjustmentCount":1,"createdTaskCount":0,"createdTasks":[],"adjustments":[adjustment]}),
                ),
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_actual_adjustment_delete",
                status: 200,
                body: mutation_body(
                    "70000000-0000-0000-0000-0000000000de",
                    json!({"adjustment":deleted_adjustment}),
                ),
            },
        ]);
        configure_test_cloud_state(&database, &server.base_url(), cloud_workspace_id);
        database
            .open()
            .unwrap()
            .execute(
                "INSERT INTO device_settings(key,value_json,updated_at) VALUES('cloud_storage_protocol',?1,1) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json",
                [serde_json::to_string("direct_v2").unwrap()],
            )
            .unwrap();
        cache_session(
            cloud_workspace_id,
            &test_session("direct-time-routing", Some(now_seconds() + 3600)),
        )
        .unwrap();
        let before = crate::storage_contract::business_data_fingerprint(&database).unwrap();

        let listed = direct_cloud_time_entry_list_for_command(
            &database,
            crate::time_tracking::TimeEntryListRequest {
                work_date: "2026-10-22".to_string(),
                include_breaks: true,
            },
        )
        .unwrap();
        assert_eq!(listed.entries[0].started_at, server_started_at);
        assert_eq!(listed.entries[0].duration_seconds, 7200);
        assert_eq!(listed.revision, 27);

        let history = direct_cloud_time_history_for_command(
            &database,
            crate::time_tracking::TimeEntryHistoryRequest {
                page: 1,
                include_breaks: true,
            },
        )
        .unwrap();
        assert_eq!(history.groups[0].entries[0].started_at, server_started_at);
        assert_eq!(history.revision, 27);

        let ensured = direct_cloud_unassigned_ensure(&database).unwrap().unwrap();
        assert_eq!(ensured.session_id, session_id);
        assert_eq!(ensured.elapsed_seconds, 600);
        assert_eq!(ensured.first_started_at, server_started_at + 7_200_000);

        let created = direct_cloud_manual_time_create(
            &database,
            crate::time_tracking::ManualEntryRequest {
                task_id: Some(task_id.to_string()),
                work_date: "2026-10-22".to_string(),
                started_at: None,
                ended_at: None,
                minutes: Some(10),
                note: Some("本地请求".to_string()),
                complete_task: false,
                task_expected_version: None,
                client_request_id: "70000000-0000-0000-0000-0000000000d9".to_string(),
            },
        )
        .unwrap();
        assert_eq!(created.started_at, server_started_at + 8_000_000);
        assert_eq!(created.duration_seconds, 600);

        let corrected = direct_cloud_time_entry_update(
            &database,
            crate::time_tracking::TimeEntryUpdateRequest {
                entry_id: entry_id.to_string(),
                started_at: server_started_at + 8_100_000,
                ended_at: server_started_at + 8_700_000,
                note: Some("本地修正".to_string()),
                expected_version: 1,
            },
        )
        .unwrap();
        assert_eq!(corrected.note.as_deref(), Some("云端修正"));
        assert_eq!(corrected.version, 2);

        let disposed = direct_cloud_time_entry_disposition(
            &database,
            crate::time_tracking::TimeEntryDispositionRequest {
                entry_id: entry_id.to_string(),
                expected_version: 2,
                disposition: "break".to_string(),
            },
        )
        .unwrap();
        assert_eq!(disposed.kind, "break");
        assert_eq!(disposed.allocated_minutes, 0);

        let resolved = direct_cloud_unassigned_resolve_session(
            &database,
            crate::unassigned::ResolveSessionRequest {
                session_id: session_id.to_string(),
                expected_version: 2,
                operation_id: "70000000-0000-0000-0000-0000000000dc".to_string(),
            },
            "discard",
        )
        .unwrap();
        assert_eq!(resolved.resolution_type, "discard");
        assert_eq!(resolved.elapsed_seconds, 600);

        let snapshot = direct_cloud_actual_adjustment_snapshot(
            &database,
            crate::actual_time_adjustments::ActualAdjustmentSnapshotRequest {
                subject_id: "50000000-0000-0000-0000-0000000000d8".to_string(),
                work_date: "2026-10-22".to_string(),
                execution_only: Some(false),
            },
        )
        .unwrap();
        assert_eq!(snapshot.work_date, "2026-10-22");

        let applied = direct_cloud_actual_adjustment_batch_apply(
            &database,
            crate::actual_time_adjustments::ActualAdjustmentBatchRequest {
                subject_id: "50000000-0000-0000-0000-0000000000d8".to_string(),
                work_date: "2026-10-22".to_string(),
                client_request_id: "70000000-0000-0000-0000-0000000000dd".to_string(),
                batch_id: "96000000-0000-0000-0000-0000000000d8".to_string(),
                note: Some("本地校准请求".to_string()),
                day_instance_date: None,
                items: vec![],
                new_tasks: vec![],
            },
        )
        .unwrap();
        assert_eq!(applied.created_adjustment_count, 1);
        assert_eq!(
            applied.adjustments[0].created_at,
            server_started_at + 9_000_000
        );

        let deleted = direct_cloud_actual_adjustment_delete(
            &database,
            crate::actual_time_adjustments::ActualAdjustmentDeleteRequest {
                adjustment_id: adjustment_id.to_string(),
                expected_version: 1,
                client_request_id: "70000000-0000-0000-0000-0000000000de".to_string(),
            },
        )
        .unwrap();
        assert_eq!(deleted.version, 2);
        assert_eq!(deleted.deleted_at, Some(server_started_at + 9_600_000));
        assert_eq!(
            before,
            crate::storage_contract::business_data_fingerprint(&database).unwrap()
        );
        let requests = server.finish();
        assert_eq!(requests.len(), 11);
        assert!(requests[0].contains("\"p_work_date\":\"2026-10-22\""));
        assert!(requests[1].contains("\"p_page\":1"));
        assert!(requests[3].contains("\"p_operation_id\""));
        assert!(requests[4].contains("\"minutes\":10"));
        assert!(requests[5].contains("\"expectedVersion\":1"));
        assert!(requests[6].contains("\"disposition\":\"break\""));
        assert!(requests[7].contains("\"resolutionType\":\"discard\""));
        assert!(requests[8].contains("\"p_execution_only\":false"));
    }

    #[test]
    fn direct_cloud_timer_response_loss_retries_the_same_operation_without_local_conflicts() {
        let _guard = lock_test_session_state();
        reset_test_credentials();
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("timer-retry.sqlite3")).unwrap();
        let workspace_id = "20000000-0000-0000-0000-0000000000fa";
        let operation_id = "70000000-0000-0000-0000-0000000000fa";
        let body = Box::leak(
            serde_json::to_string(&json!({
                "operationId": operation_id,
                "metadata": {
                    "workspaceId":workspace_id,"deviceId":"device-placeholder",
                    "storageProtocol":"direct_v2","workspaceEpoch":1,"changeSeq":99,
                    "serverTime":"2026-10-23T09:00:00+08:00",
                    "domains":["time"],"diagnostics":{"responseBytes":256,"rpcCount":1,"durationMs":1.0}
                },
                "data": {"entry": {
                    "id":"90000000-0000-0000-0000-0000000000fa","workDate":"2026-10-23",
                    "label":"幂等重试计时","startedAt":1792726800000_i64,"endedAt":null,
                    "durationSeconds":0,"settlementMinutes":0,"allocatedMinutes":0,
                    "defaultTaskId":null,"note":null,"kind":"work","sourceType":"timer",
                    "state":"running","version":1,"timerChainId":"90000000-0000-0000-0000-0000000000fa",
                    "previousEntryId":null,"splitBoundaryAt":null,"allocations":[]
                }}
            }))
            .unwrap()
            .into_boxed_str(),
        );
        let server = TestHttpServer::start_with_disconnect_then_response(
            TestHttpDisconnect {
                path: "/rest/v1/rpc/direct_cloud_timer_start",
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_timer_start",
                status: 200,
                body,
            },
        );
        configure_test_cloud_state(&database, &server.base_url(), workspace_id);
        database
            .open()
            .unwrap()
            .execute(
                "INSERT INTO device_settings(key,value_json,updated_at) VALUES('cloud_storage_protocol',?1,1) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json",
                [serde_json::to_string("direct_v2").unwrap()],
            )
            .unwrap();
        cache_session(
            workspace_id,
            &test_session("timer-response-loss", Some(now_seconds() + 3600)),
        )
        .unwrap();
        let before = crate::storage_contract::business_data_fingerprint(&database).unwrap();

        let entry = direct_cloud_timer_start(
            &database,
            crate::time_tracking::TimerStartRequest {
                task_id: None,
                note: None,
                client_request_id: operation_id.to_string(),
            },
        )
        .unwrap();
        assert_eq!(entry.started_at, 1_792_726_800_000);
        assert_eq!(
            before,
            crate::storage_contract::business_data_fingerprint(&database).unwrap()
        );
        let requests = server.finish();
        assert_eq!(requests.len(), 2);
        assert!(requests
            .iter()
            .all(|request| request.contains(operation_id)));
        assert_eq!(requests[0], requests[1]);
        let conflict_count: i64 = database
            .open()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM sync_outbox WHERE state='conflict'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(conflict_count, 0);
    }

    #[test]
    fn direct_cloud_tray_start_uses_the_storage_router_and_authoritative_timer() {
        let _guard = lock_test_session_state();
        reset_test_credentials();
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("direct-tray.sqlite3")).unwrap();
        let workspace_id = "20000000-0000-0000-0000-0000000000fb";
        let server_started_at = 1_792_730_400_000_i64;
        let metadata = json!({
            "workspaceId":workspace_id,"deviceId":"device-placeholder",
            "storageProtocol":"direct_v2","workspaceEpoch":1,"changeSeq":101,
            "serverTime":"2026-10-23T10:00:00+08:00",
            "domains":["time"],"diagnostics":{"responseBytes":256,"rpcCount":1,"durationMs":1.0}
        });
        let state_body = Box::leak(
            serde_json::to_string(&json!({
                "metadata":metadata,
                "data":{"workDate":"2026-10-23","activeEntry":null,"entries":[],"unassigned":null}
            }))
            .unwrap()
            .into_boxed_str(),
        );
        let started_body = Box::leak(
            serde_json::to_string(&json!({
                "operationId":"70000000-0000-0000-0000-0000000000fb",
                "metadata":metadata,
                "data":{"entry":{
                    "id":"90000000-0000-0000-0000-0000000000fb","workDate":"2026-10-23",
                    "label":"托盘云端计时","startedAt":server_started_at,"endedAt":null,
                    "durationSeconds":0,"settlementMinutes":0,"allocatedMinutes":0,
                    "defaultTaskId":null,"note":null,"kind":"work","sourceType":"timer",
                    "state":"running","version":1,"timerChainId":"90000000-0000-0000-0000-0000000000fb",
                    "previousEntryId":null,"splitBoundaryAt":null,"allocations":[]
                }}
            }))
            .unwrap()
            .into_boxed_str(),
        );
        let server = TestHttpServer::start(vec![
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_active_timer",
                status: 200,
                body: state_body,
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_timer_start",
                status: 200,
                body: started_body,
            },
        ]);
        configure_test_cloud_state(&database, &server.base_url(), workspace_id);
        database
            .open()
            .unwrap()
            .execute(
                "INSERT INTO device_settings(key,value_json,updated_at) VALUES('cloud_storage_protocol',?1,1) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json",
                [serde_json::to_string("direct_v2").unwrap()],
            )
            .unwrap();
        cache_session(
            workspace_id,
            &test_session("direct-tray", Some(now_seconds() + 3600)),
        )
        .unwrap();
        let before = crate::storage_contract::business_data_fingerprint(&database).unwrap();

        let result =
            crate::execute_tray_timer_action(&database, crate::TrayTimerAction::Start).unwrap();
        assert_eq!(result["startedAt"], server_started_at);
        assert_eq!(result["label"], "托盘云端计时");
        assert_eq!(
            before,
            crate::storage_contract::business_data_fingerprint(&database).unwrap()
        );
        let requests = server.finish();
        assert_eq!(requests.len(), 2);
        assert!(requests[1].contains("/rest/v1/rpc/direct_cloud_timer_start"));
        assert!(requests[1].contains("\"p_operation_id\""));
    }

    #[test]
    fn second_device_stale_submission_reloads_authority_without_local_conflict_rows() {
        let _guard = lock_test_session_state();
        reset_test_credentials();
        let directory = tempdir().unwrap();
        let device_a =
            Database::initialize_at(directory.path().join("direct-device-a.sqlite3")).unwrap();
        let device_b =
            Database::initialize_at(directory.path().join("direct-device-b.sqlite3")).unwrap();
        let cloud_workspace_id = "20000000-0000-0000-0000-0000000000e9";
        let task_id = "60000000-0000-0000-0000-0000000000e9";
        let metadata = |domains: Value| {
            json!({
                "workspaceId": cloud_workspace_id,
                "deviceId": "device-placeholder",
                "storageProtocol": "direct_v2",
                "workspaceEpoch": 1,
                "changeSeq": 9,
                "serverTime": "2026-10-22T18:00:00+08:00",
                "domains": domains,
                "diagnostics": {"responseBytes": 512, "rpcCount": 1, "durationMs": 1.0},
            })
        };
        let authoritative_task = json!({
            "id": task_id,
            "subjectId": "50000000-0000-0000-0000-0000000000e9",
            "parentId": null,
            "dayInstanceDate": null,
            "dayInstanceKind": null,
            "dayInstanceSourceTaskId": null,
            "title": "设备 A 已保存",
            "status": "open",
            "plannedDate": null,
            "estimateMinutes": null,
            "note": null,
            "projectName": null,
            "solutionName": null,
            "sortOrder": 10,
            "completedAt": null,
            "depth": 0,
            "pathLabel": "设备 A 已保存",
            "selectable": true,
            "directMinutes": 0,
            "totalMinutes": 0,
            "executionState": "not_started",
            "version": 2,
        });
        let update_body = Box::leak(
            serde_json::to_string(&json!({
                "operationId": "70000000-0000-0000-0000-0000000000e9",
                "metadata": metadata(json!(["tasks"])),
                "data": {"task": authoritative_task},
            }))
            .unwrap()
            .into_boxed_str(),
        );
        let reload_body = Box::leak(
            serde_json::to_string(&json!({
                "metadata": metadata(json!(["tasks"])),
                "data": {"tasks": [authoritative_task]},
            }))
            .unwrap()
            .into_boxed_str(),
        );
        let server = TestHttpServer::start(vec![
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_task_update",
                status: 200,
                body: update_body,
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_task_update",
                status: 400,
                body: r#"{"code":"P0001","message":"VERSION_CONFLICT: 事项已变化"}"#,
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/direct_cloud_task_list",
                status: 200,
                body: reload_body,
            },
        ]);
        for database in [&device_a, &device_b] {
            configure_test_cloud_state(database, &server.base_url(), cloud_workspace_id);
            database
                .open()
                .unwrap()
                .execute(
                    "INSERT INTO device_settings(key,value_json,updated_at) VALUES('cloud_storage_protocol',?1,1) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json",
                    [serde_json::to_string("direct_v2").unwrap()],
                )
                .unwrap();
            cache_session(
                cloud_workspace_id,
                &test_session("direct-device-race", Some(now_seconds() + 3600)),
            )
            .unwrap();
        }
        let before_a = crate::storage_contract::business_data_fingerprint(&device_a).unwrap();
        let before_b = crate::storage_contract::business_data_fingerprint(&device_b).unwrap();

        let saved = direct_cloud_task_update_for_command(
            &device_a,
            crate::tasks::TaskUpdateRequest {
                id: task_id.to_string(),
                expected_version: 1,
                title: Some("设备 A 已保存".to_string()),
                parent_id: None,
                planned_date: None,
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        assert_eq!(saved.version, 2);

        let stale = crate::storage_contract::route(
            &device_b,
            crate::storage_contract::BusinessDomain::Tasks,
            crate::storage_contract::StorageOperation::Mutation,
            |_| -> Result<crate::tasks::TaskDto, String> {
                Err("sqlite adapter must not run for direct_v2".to_string())
            },
            |database| {
                direct_cloud_task_update_for_command(
                    database,
                    crate::tasks::TaskUpdateRequest {
                        id: task_id.to_string(),
                        expected_version: 1,
                        title: Some("设备 B 陈旧标题".to_string()),
                        parent_id: None,
                        planned_date: None,
                        estimate_minutes: None,
                        note: None,
                        project_name: None,
                        solution_name: None,
                    },
                )
            },
        )
        .unwrap_err();
        assert!(stale.starts_with("VERSION_CONFLICT:"));

        let reloaded = direct_cloud_task_list_for_command(
            &device_b,
            crate::tasks::TaskListRequest {
                subject_id: None,
                include_completed: Some(true),
                planned_date: None,
                today_date: None,
                today_only: None,
                daily_estimate_date: None,
                query: None,
            },
        )
        .unwrap();
        assert_eq!(reloaded.tasks[0].title, "设备 A 已保存");
        assert_eq!(reloaded.tasks[0].version, 2);

        for database in [&device_a, &device_b] {
            let pending: i64 = database
                .open()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM sync_outbox WHERE state IN ('pending','sending','failed','conflict')",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(pending, 0);
        }
        assert_eq!(
            before_a,
            crate::storage_contract::business_data_fingerprint(&device_a).unwrap()
        );
        assert_eq!(
            before_b,
            crate::storage_contract::business_data_fingerprint(&device_b).unwrap()
        );

        let requests = server.finish();
        assert_eq!(requests.len(), 3);
        assert!(requests[1].contains("\"expectedVersion\":1"));
        assert!(requests[1].contains("设备 B 陈旧标题"));
        assert!(requests[2].contains("\"p_workspace_id\":\"20000000-0000-0000-0000-0000000000e9\""));
    }

    #[test]
    fn recognizes_only_the_legacy_named_report_template_bootstrap_failure() {
        assert!(is_legacy_report_template_bootstrap_error(
            "CLOUD_REQUEST_FAILED: null value in column \"name\" of relation \"report_templates\" violates not-null constraint"
        ));
        assert!(!is_legacy_report_template_bootstrap_error(
            "CLOUD_REQUEST_FAILED: null value in column \"content\" of relation \"report_templates\" violates not-null constraint"
        ));
        assert!(!is_legacy_report_template_bootstrap_error(
            "CLOUD_REQUEST_FAILED: null value in column \"name\" of relation \"subjects\" violates not-null constraint"
        ));
    }

    #[test]
    fn device_rows_mark_current_and_preserve_revocation_state() {
        let current = cloud_device_from_row(
            DeviceRow {
                id: "device-a".to_string(),
                workspace_id: "workspace".to_string(),
                device_name: "设备 A".to_string(),
                platform: "windows".to_string(),
                app_version: "0.2.1".to_string(),
                last_seen_at: "2026-10-04T10:00:00Z".to_string(),
                authorized_at: "2026-10-01T10:00:00Z".to_string(),
                reauthorized_at: None,
                revoked_at: None,
            },
            "device-a",
        );
        assert!(current.current);
        assert!(current.revoked_at.is_none());

        let revoked = cloud_device_from_row(
            DeviceRow {
                id: "device-b".to_string(),
                workspace_id: "workspace".to_string(),
                device_name: "设备 B".to_string(),
                platform: "linux".to_string(),
                app_version: "0.2.0".to_string(),
                last_seen_at: "2026-10-03T10:00:00Z".to_string(),
                authorized_at: "2026-10-01T10:00:00Z".to_string(),
                reauthorized_at: Some("2026-10-02T10:00:00Z".to_string()),
                revoked_at: Some("2026-10-04T10:00:00Z".to_string()),
            },
            "device-a",
        );
        assert!(!revoked.current);
        assert!(revoked.revoked_at.is_some());
    }

    #[test]
    fn temporary_supabase_auth_failures_keep_saved_session_visible() {
        let error = classify_http_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            r#"{"message":"temporary auth service failure"}"#,
            "AUTH_REQUIRED",
        );
        assert!(error.starts_with("NETWORK_ERROR:"));
    }

    #[test]
    fn refresh_network_failures_keep_the_existing_session_but_auth_failures_do_not() {
        assert!(should_keep_existing_session_after_refresh_error(
            "NETWORK_ERROR: Supabase 登录请求失败: timeout"
        ));
        assert!(!should_keep_existing_session_after_refresh_error(
            "AUTH_INVALID: refresh token 已失效"
        ));
        assert!(should_keep_existing_session_after_refresh_error(
            "AUTH_UNVERIFIED: gateway returned unauthorized"
        ));
    }

    #[test]
    fn only_definitive_refresh_failures_clear_saved_sessions() {
        assert!(is_definitive_auth_invalid("AUTH_INVALID: invalid_grant"));
        assert!(is_definitive_auth_invalid("DEVICE_REVOKED: revoked"));
        assert!(!is_definitive_auth_invalid("AUTH_FAILED: JWT expired"));
        assert!(!is_definitive_auth_invalid("AUTH_UNVERIFIED: unauthorized"));
        assert!(!is_definitive_auth_invalid("NETWORK_ERROR: timeout"));
    }

    #[test]
    fn business_request_auth_errors_refresh_without_direct_session_deletion() {
        assert!(is_business_request_auth_error("AUTH_FAILED: JWT expired"));
        assert!(!is_business_request_auth_error("AUTH_REQUIRED: 尚未登录"));
        assert!(!is_business_request_auth_error("NETWORK_ERROR: timeout"));
        assert!(!is_business_request_auth_error(
            "CLOUD_SCHEMA_MISSING: missing"
        ));
    }

    #[test]
    fn refresh_error_classification_distinguishes_definitive_and_temporary_failures() {
        assert!(classify_refresh_auth_error(
            StatusCode::BAD_REQUEST,
            r#"{"error_code":"refresh_token_not_found","message":"Invalid Refresh Token"}"#
        )
        .starts_with("AUTH_INVALID:"));
        assert!(classify_refresh_auth_error(
            StatusCode::UNAUTHORIZED,
            r#"{"message":"upstream unauthorized"}"#
        )
        .starts_with("AUTH_UNVERIFIED:"));
        assert!(classify_refresh_auth_error(
            StatusCode::TOO_MANY_REQUESTS,
            r#"{"message":"rate limited"}"#
        )
        .starts_with("NETWORK_ERROR:"));
        assert!(classify_refresh_auth_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            r#"{"message":"temporary failure"}"#
        )
        .starts_with("NETWORK_ERROR:"));
        assert!(classify_refresh_auth_error(
            StatusCode::BAD_GATEWAY,
            "upstream returned malformed html"
        )
        .starts_with("NETWORK_ERROR:"));
        assert!(classify_refresh_auth_error(
            StatusCode::FORBIDDEN,
            r#"{"message":"unknown gateway denial"}"#
        )
        .starts_with("AUTH_UNVERIFIED:"));
        assert!(classify_refresh_auth_error(
            StatusCode::BAD_REQUEST,
            r#"{"error_code":"invalid_grant","message":"session revoked"}"#
        )
        .starts_with("AUTH_INVALID:"));
        assert!(classify_refresh_auth_error(
            StatusCode::FORBIDDEN,
            r#"{"error_code":"user_banned","message":"user is banned"}"#
        )
        .starts_with("AUTH_INVALID:"));
        assert!(classify_refresh_auth_error(
            StatusCode::UNAUTHORIZED,
            r#"{"error_code":"session_not_found","message":"session not found"}"#
        )
        .starts_with("AUTH_INVALID:"));
    }

    #[test]
    fn refresh_response_parse_failure_preserves_saved_session() {
        let _guard = lock_test_session_state();
        reset_test_credentials();
        let server = TestHttpServer::start(vec![TestHttpResponse {
            path: "/auth/v1/token?grant_type=refresh_token",
            status: 200,
            body: "not-json",
        }]);
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("refresh-parse-failure.sqlite3"))
                .unwrap();
        let cloud_workspace_id = Uuid::now_v7().to_string();
        configure_test_cloud_state(&database, &server.base_url(), &cloud_workspace_id);
        save_session(&database, &test_session("parse-failure", Some(0))).unwrap();

        let error = current_session(&database).unwrap_err();
        assert!(error.starts_with("NETWORK_ERROR:"));
        assert!(read_session_credential(SESSION_ACCOUNT).is_ok());
        assert!(cached_session(&test_workspace_id(&database)).is_some());
        assert_eq!(server.finish().len(), 1);
    }

    #[test]
    fn successful_token_refresh_detects_revoked_device_before_business_requests_resume() {
        let _guard = lock_test_session_state();
        reset_test_credentials();
        let server = TestHttpServer::start(vec![
            TestHttpResponse {
                path: "/auth/v1/token?grant_type=refresh_token",
                status: 200,
                body: r#"{"access_token":"access-refreshed","refresh_token":"refresh-refreshed","expires_in":3600,"user":{"id":"user-revoked","email":"revoked@example.com"}}"#,
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/device_authorization_get",
                status: 200,
                body: r#"{"authorized":false,"reason":"DEVICE_REVOKED"}"#,
            },
        ]);
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("refresh-device-revoked.sqlite3"))
                .unwrap();
        let cloud_workspace_id = Uuid::now_v7().to_string();
        configure_test_cloud_state(&database, &server.base_url(), &cloud_workspace_id);
        save_session(&database, &test_session("revoked", Some(0))).unwrap();

        let error = current_session(&database).unwrap_err();
        assert!(error.starts_with("DEVICE_REVOKED:"));
        assert_eq!(
            read_session_credential(SESSION_ACCOUNT).unwrap_err(),
            "NO_ENTRY"
        );
        assert!(storage_mode(&database).unwrap().auth_blocked);
        assert_eq!(server.finish().len(), 2);
    }

    #[test]
    fn device_api_errors_keep_cross_account_rejection_and_redact_sensitive_details() {
        let _guard = lock_test_session_state();
        reset_test_credentials();
        let server = TestHttpServer::start(vec![
            TestHttpResponse {
                path: "/rest/v1/rpc/device_list",
                status: 400,
                body: r#"{"message":"AUTH_REQUIRED: workspace is not owned by current user"}"#,
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/device_revoke",
                status: 400,
                body: r#"{"message":"authorization=Bearer secret-device-token"}"#,
            },
        ]);
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("device-api-errors.sqlite3")).unwrap();
        let cloud_workspace_id = Uuid::now_v7().to_string();
        configure_test_cloud_state(&database, &server.base_url(), &cloud_workspace_id);
        save_session(
            &database,
            &test_session("device-api-errors", Some(now_seconds() + 3600)),
        )
        .unwrap();

        let cross_account = list_devices(&database).unwrap_err();
        assert!(cross_account.starts_with("CLOUD_REQUEST_FAILED:"));
        assert!(cross_account.contains("AUTH_REQUIRED"));

        let sensitive = revoke_device(&database, &Uuid::now_v7().to_string()).unwrap_err();
        assert!(sensitive.contains("已脱敏"));
        assert!(!sensitive.contains("secret-device-token"));
        assert_eq!(server.finish().len(), 2);
    }

    #[test]
    fn session_candidates_prefer_stable_account_before_workspace_legacy_key() {
        let workspace_id = Uuid::now_v7().to_string();
        let candidates = session_account_candidates(&workspace_id);
        assert_eq!(candidates[0], SESSION_ACCOUNT);
        assert_eq!(candidates[1], format!("{SESSION_PROVIDER}:{workspace_id}"));
    }

    #[test]
    fn migration_conflicts_block_empty_cloud_snapshot_import() {
        let conflicts = migration_conflicts("cloud_to_local_snapshot", false, false);
        assert_eq!(conflicts.len(), 1);
        assert!(conflicts[0].contains("不能用空云端快照覆盖本机数据"));

        let conflicts = migration_conflicts("cloud_to_local_snapshot", true, false);
        assert_eq!(conflicts.len(), 1);
        assert!(conflicts[0].contains("还没有事项、计时或报告数据"));

        let conflicts = migration_conflicts("cloud_to_local_snapshot", true, true);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn migration_conflicts_allow_atomic_upload_when_cloud_has_business_data() {
        let conflicts = migration_conflicts("local_to_cloud", true, true);
        assert!(conflicts.is_empty());

        let conflicts = migration_conflicts("local_to_cloud", false, false);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn local_mode_snapshot_has_no_pending_operations() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(directory.path().join("test.sqlite3")).unwrap();
        let snapshot = storage_mode(&database).unwrap();
        assert_eq!(snapshot.mode, "local");
        assert_eq!(snapshot.pending_operations, 0);
        assert!(!snapshot.online || snapshot.sync_state == "synced");
    }

    #[test]
    fn failed_outbox_rows_are_reported_as_pending_operations() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("failed-outbox.sqlite3")).unwrap();
        let cloud_workspace_id = "cloud-workspace";
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
        let device_id = storage_mode(&database).unwrap().device_id;
        database
            .open()
            .unwrap()
            .execute(
                "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, state, attempt_count, created_at, error_json)
                 VALUES (?1, ?2, ?3, 'upsert', 'task', 'task-1', 1, '{}', 'failed', 1, 1, ?4)",
                rusqlite::params![
                    uuid::Uuid::now_v7().to_string(),
                    cloud_workspace_id,
                    device_id,
                    json!({ "message": "temporary network failure" }).to_string()
                ],
            )
            .unwrap();

        let snapshot = storage_mode(&database).unwrap();
        assert_eq!(snapshot.mode, "cloud");
        assert_eq!(snapshot.pending_operations, 1);
        assert_eq!(snapshot.sync_state, "pending");
    }

    #[test]
    fn pending_outbox_rows_take_priority_over_previous_sync_error() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("pending-priority-over-error.sqlite3"))
                .unwrap();
        let cloud_workspace_id = "cloud-workspace";
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
        let device_id = storage_mode(&database).unwrap().device_id;
        let connection = database.open().unwrap();
        connection
            .execute(
                "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, state, attempt_count, created_at, error_json)
                 VALUES (?1, ?2, ?3, 'upsert', 'task', 'task-1', 1, '{}', 'pending', 0, 1, NULL)",
                rusqlite::params![uuid::Uuid::now_v7().to_string(), cloud_workspace_id, device_id],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO local_sync_state(workspace_id, device_id, last_change_seq, last_full_sync_at, last_error)
                 VALUES (?1, ?2, 42, 1700000000000, 'network timeout')",
                rusqlite::params![cloud_workspace_id, device_id],
            )
            .unwrap();

        let snapshot = storage_mode(&database).unwrap();
        assert_eq!(snapshot.mode, "cloud");
        assert_eq!(snapshot.pending_operations, 1);
        assert_eq!(snapshot.sync_state, "pending");
        assert_eq!(snapshot.last_error.as_deref(), Some("network timeout"));
    }

    #[test]
    fn conflict_outbox_rows_are_reported_as_conflict_state() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("conflict-outbox.sqlite3")).unwrap();
        let cloud_workspace_id = "cloud-workspace";
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
        let device_id = storage_mode(&database).unwrap().device_id;
        database
            .open()
            .unwrap()
            .execute(
                "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, state, attempt_count, created_at, error_json)
                 VALUES (?1, ?2, ?3, 'upsert', 'task', 'task-1', 1, '{}', 'conflict', 1, 1, ?4)",
                rusqlite::params![
                    uuid::Uuid::now_v7().to_string(),
                    cloud_workspace_id,
                    device_id,
                    json!({ "message": "version conflict" }).to_string()
                ],
            )
            .unwrap();
        database
            .open()
            .unwrap()
            .execute(
                "INSERT INTO local_sync_state(workspace_id, device_id, last_change_seq, last_full_sync_at, last_error)
                 VALUES (?1, ?2, 0, 1, 'previous network timeout')",
                rusqlite::params![cloud_workspace_id, device_id],
            )
            .unwrap();

        let snapshot = storage_mode(&database).unwrap();
        assert_eq!(snapshot.mode, "cloud");
        assert_eq!(snapshot.pending_operations, 0);
        assert_eq!(snapshot.conflict_count, 1);
        assert_eq!(snapshot.sync_state, "conflict");
        assert_eq!(
            snapshot.last_error.as_deref(),
            Some("previous network timeout")
        );
    }

    #[test]
    fn cloud_sync_state_exposes_last_success_and_error_details() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("sync-state-details.sqlite3")).unwrap();
        let cloud_workspace_id = "cloud-workspace";
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
        database
            .open()
            .unwrap()
            .execute(
                "INSERT INTO local_sync_state(workspace_id, device_id, last_change_seq, last_full_sync_at, last_error)
                 VALUES (?1, 'device-1', 42, 1700000000000, 'network timeout')",
                [cloud_workspace_id],
            )
            .unwrap();

        let snapshot = storage_mode(&database).unwrap();
        assert_eq!(snapshot.sync_state, "error");
        assert_eq!(snapshot.last_change_seq, 42);
        assert_eq!(snapshot.last_synced_at, Some(1700000000000));
        assert_eq!(snapshot.last_error.as_deref(), Some("network timeout"));
    }

    #[test]
    fn local_mode_switch_is_blocked_until_cloud_outbox_is_clean() {
        for dirty_state in ["pending", "failed", "sending", "conflict"] {
            let (_directory, database, _) = cloud_database_with_outbox_state(
                &format!("cloud-switch-{dirty_state}.sqlite3"),
                dirty_state,
            );

            let error = ensure_no_pending_cloud_work(&database).unwrap_err();
            assert!(
                error.starts_with("OFFLINE_RESTRICTED:"),
                "dirty outbox state {dirty_state} should block local mode switch"
            );
            let snapshot = storage_mode(&database).unwrap();
            assert_eq!(snapshot.mode, "cloud");
            if dirty_state == "conflict" {
                assert_eq!(snapshot.conflict_count, 1);
            } else {
                assert_eq!(snapshot.pending_operations, 1);
            }
        }
    }

    #[test]
    fn cloud_configuration_change_is_blocked_until_outbox_is_clean() {
        for dirty_state in ["pending", "failed", "sending", "conflict"] {
            let (_directory, database, _) = cloud_database_with_outbox_state(
                &format!("config-switch-{dirty_state}.sqlite3"),
                dirty_state,
            );

            ensure_can_replace_cloud_configuration(
                &database,
                "https://old-project.supabase.co",
                "old-anon-key",
            )
            .unwrap();
            let error = ensure_can_replace_cloud_configuration(
                &database,
                "https://new-project.supabase.co",
                "old-anon-key",
            )
            .unwrap_err();
            assert!(
                error.starts_with("OFFLINE_RESTRICTED:"),
                "dirty outbox state {dirty_state} should block project change"
            );
            let error = ensure_can_replace_cloud_configuration(
                &database,
                "https://old-project.supabase.co",
                "new-anon-key",
            )
            .unwrap_err();
            assert!(
                error.starts_with("OFFLINE_RESTRICTED:"),
                "dirty outbox state {dirty_state} should block key change"
            );
        }
    }

    #[test]
    fn cloud_configuration_change_detection_only_changes_for_project_or_key() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("config-change.sqlite3")).unwrap();
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "supabase_project_url".to_string(),
                value: json!("https://old-project.supabase.co"),
            },
        )
        .unwrap();
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "supabase_anon_key".to_string(),
                value: json!("old-anon-key"),
            },
        )
        .unwrap();

        assert!(!cloud_configuration_is_changing(
            &database,
            "https://old-project.supabase.co",
            "old-anon-key"
        )
        .unwrap());
        assert!(cloud_configuration_is_changing(
            &database,
            "https://new-project.supabase.co",
            "old-anon-key"
        )
        .unwrap());
        assert!(cloud_configuration_is_changing(
            &database,
            "https://old-project.supabase.co",
            "new-anon-key"
        )
        .unwrap());
    }

    #[test]
    fn cloud_snapshot_migration_is_blocked_until_outbox_is_clean() {
        for dirty_state in ["pending", "failed", "sending", "conflict"] {
            let (_directory, database, _) = cloud_database_with_outbox_state(
                &format!("migration-blocked-{dirty_state}.sqlite3"),
                dirty_state,
            );

            let error = migration_execute(
                &database,
                MigrationExecuteRequest {
                    direction: "cloud_to_local_snapshot".to_string(),
                    confirmed: true,
                },
            )
            .unwrap_err();
            assert!(
                error.starts_with("OFFLINE_RESTRICTED:"),
                "dirty outbox state {dirty_state} should block cloud snapshot migration"
            );
        }
    }

    fn cloud_database_with_outbox_state(
        file_name: &str,
        outbox_state: &str,
    ) -> (tempfile::TempDir, Database, String) {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(directory.path().join(file_name)).unwrap();
        let cloud_workspace_id = Uuid::now_v7().to_string();
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "cloud_workspace_id".to_string(),
                value: json!(cloud_workspace_id.clone()),
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
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "supabase_project_url".to_string(),
                value: json!("https://old-project.supabase.co"),
            },
        )
        .unwrap();
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "supabase_anon_key".to_string(),
                value: json!("old-anon-key"),
            },
        )
        .unwrap();
        let device_id = storage_mode(&database).unwrap().device_id;
        database
            .open()
            .unwrap()
            .execute(
                "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, state, attempt_count, created_at, error_json)
                 VALUES (?1, ?2, ?3, 'task_update', 'task', 'task-1', 1, '{}', ?4, 1, 1, '{}')",
                rusqlite::params![
                    Uuid::now_v7().to_string(),
                    &cloud_workspace_id,
                    device_id,
                    outbox_state
                ],
            )
            .unwrap();
        (directory, database, cloud_workspace_id)
    }

    #[test]
    fn local_snapshot_includes_recurring_and_daily_estimate_data() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("recurring-migration.sqlite3")).unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let subject_id: String = connection
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        drop(connection);
        let task = create_task(
            &database,
            TaskCreateRequest {
                subject_id,
                day_instance_date: None,
                parent_id: None,
                title: "不支持迁移的重复事项".to_string(),
                planned_date: None,
                estimate_minutes: None,
                note: None,
                project_name: None,
                solution_name: None,
            },
        )
        .unwrap();
        let task_id = task.id.clone();
        set_task_daily_estimate(
            &database,
            TaskDailyEstimateSetRequest {
                task_id: task_id.clone(),
                work_date: "2026-09-29".to_string(),
                estimate_minutes: Some(60),
            },
        )
        .unwrap();
        save_rule(
            &database,
            RecurrenceSaveRequest {
                task_id: task_id.clone(),
                task_expected_version: task.version,
                frequency: "daily".to_string(),
                weekdays_mask: None,
                effective_start: chrono::Local::now().format("%Y-%m-%d").to_string(),
                rule_expected_version: None,
            },
        )
        .unwrap();
        let connection = database.open().unwrap();
        let now = now_seconds() * 1000;
        connection
            .execute(
                "INSERT INTO task_occurrences(workspace_id,task_id,occurrence_date,origin,status,created_at,updated_at,version)
                 VALUES (?1,?2,'2026-09-29','scheduled','open',?3,?3,1)",
                params![workspace_id, task_id, now],
            )
            .unwrap();
        drop(connection);

        let snapshot = local_snapshot(&database, &workspace_id).unwrap();
        assert_eq!(
            snapshot["task_daily_estimates"].as_array().unwrap().len(),
            1
        );
        assert_eq!(
            snapshot["task_recurrence_rules"].as_array().unwrap().len(),
            1
        );
        assert_eq!(snapshot["task_occurrences"].as_array().unwrap().len(), 1);
        assert_eq!(
            snapshot["task_daily_estimates"][0]["estimate_minutes"],
            json!(60)
        );
    }

    #[test]
    fn local_snapshot_includes_subject_work_schedules() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("schedule-migration.sqlite3")).unwrap();
        let workspace_id: String = database
            .open()
            .unwrap()
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let snapshot = local_snapshot(&database, &workspace_id).unwrap();
        assert_eq!(
            snapshot["subject_work_schedules"].as_array().unwrap().len(),
            7
        );
        assert_eq!(
            snapshot["subject_work_schedule_segments"]
                .as_array()
                .unwrap()
                .len(),
            10
        );
    }

    #[test]
    fn migration_preview_counts_planning_entities() {
        let _guard = lock_test_session_state();
        reset_test_credentials();
        let cloud_workspace_id = Uuid::now_v7().to_string();
        let server = TestHttpServer::start(vec![
            TestHttpResponse {
                path: "/rest/v1/workspaces",
                status: 200,
                body: Box::leak(
                    format!(
                        r#"[{{"id":"{cloud_workspace_id}","name":"云端工作空间","timezone":"Asia/Shanghai"}}]"#
                    )
                    .into_boxed_str(),
                ),
            },
            TestHttpResponse {
                path: "/rest/v1/workspace_changes",
                status: 200,
                body: "[]",
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/cloud_snapshot_get",
                status: 200,
                body: r#"{"subjects":[],"tasks":[],"time_entries":[],"time_allocations":[],"reports":[],"task_daily_estimates":[],"task_recurrence_rules":[],"task_occurrences":[],"work_days":[],"unassigned_sessions":[]}"#,
            },
        ]);
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("planning-preview.sqlite3")).unwrap();
        configure_test_cloud_state(&database, &server.base_url(), &cloud_workspace_id);
        save_session(
            &database,
            &test_session("planning-preview", Some(now_seconds() + 3600)),
        )
        .unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let subject_id: String = connection
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        drop(connection);
        let task = create_task(
            &database,
            TaskCreateRequest {
                subject_id,
                day_instance_date: None,
                parent_id: None,
                title: "迁移预览规划事项".to_string(),
                planned_date: None,
                estimate_minutes: None,
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
                estimate_minutes: Some(30),
            },
        )
        .unwrap();
        let rule = save_rule(
            &database,
            RecurrenceSaveRequest {
                task_id: task.id.clone(),
                task_expected_version: task.version,
                frequency: "daily".to_string(),
                weekdays_mask: None,
                effective_start: chrono::Local::now().format("%Y-%m-%d").to_string(),
                rule_expected_version: None,
            },
        )
        .unwrap();
        let now = now_seconds() * 1000;
        database
            .open()
            .unwrap()
            .execute(
                "INSERT INTO task_occurrences(workspace_id,task_id,occurrence_date,origin,status,created_at,updated_at,version) VALUES (?1,?2,?3,'scheduled','open',?4,?4,1)",
                params![workspace_id, task.id, rule.effective_start, now],
            )
            .unwrap();

        let preview = migration_preview(
            &database,
            MigrationPreviewRequest {
                direction: "local_to_cloud".to_string(),
            },
        )
        .unwrap();
        let count = |entity: &str| {
            preview
                .entities
                .iter()
                .find(|item| item.entity == entity)
                .map(|item| item.local_count)
                .unwrap()
        };
        assert_eq!(count("task_daily_estimates"), 1);
        assert_eq!(count("task_recurrence_rules"), 1);
        assert_eq!(count("task_occurrences"), 1);
        assert_eq!(preview.entities.len(), 22);
        assert_eq!(
            preview.package_version,
            crate::data_protection::DATA_PACKAGE_FORMAT_VERSION
        );
        assert!(preview.source_entity_count > 0);
        assert_eq!(preview.target_entity_count, 0);
        assert!(preview.target_is_empty);
        assert_eq!(preview.replace_entity_count, 0);
        assert!(!preview.safety_snapshot_required);
        assert!(preview.can_execute);
        assert!(preview.conflicts.is_empty());
        assert_eq!(server.finish().len(), 3);
        reset_test_credentials();
    }

    #[test]
    fn local_to_cloud_preview_marks_nonempty_targets_for_snapshot_and_replace() {
        let entities = vec![
            MigrationEntityCount {
                entity: "tasks".to_string(),
                local_count: 3,
                cloud_count: Some(2),
            },
            MigrationEntityCount {
                entity: "app_settings".to_string(),
                local_count: 1,
                cloud_count: Some(1),
            },
        ];
        assert_eq!(
            migration_preview_impact("local_to_cloud", &entities),
            (4, 3, false, 3, true)
        );
        assert_eq!(
            migration_preview_impact("cloud_to_local_snapshot", &entities),
            (3, 4, false, 4, false)
        );
    }

    #[test]
    fn cloud_to_local_preview_uses_sanitized_cloud_counts_and_reverses_source_target() {
        let _guard = lock_test_session_state();
        reset_test_credentials();
        let cloud_workspace_id = Uuid::now_v7().to_string();
        let entity_counts = crate::data_protection::data_package_copy_entity_names()
            .unwrap()
            .into_iter()
            .map(|entity| {
                let count = if entity == "subjects" { 2 } else { 0 };
                (entity, json!(count))
            })
            .collect::<serde_json::Map<String, Value>>();
        let export_preview_body = Box::leak(
            json!({
                "workspaceId": cloud_workspace_id,
                "packageVersion": crate::data_protection::DATA_PACKAGE_FORMAT_VERSION,
                "workspaceEpoch": 1,
                "changeSeq": 17,
                "entityCounts": entity_counts
            })
            .to_string()
            .into_boxed_str(),
        );
        let server = TestHttpServer::start(vec![
            TestHttpResponse {
                path: "/rest/v1/workspaces",
                status: 200,
                body: Box::leak(
                    format!(
                        r#"[{{"id":"{cloud_workspace_id}","name":"导出云端","timezone":"Asia/Shanghai"}}]"#
                    )
                    .into_boxed_str(),
                ),
            },
            TestHttpResponse {
                path: "/rest/v1/workspace_changes",
                status: 200,
                body: "[]",
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/data_copy_export_preview",
                status: 200,
                body: export_preview_body,
            },
        ]);
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("cloud-to-local-copy-preview.sqlite3"))
                .unwrap();
        let (device_id, local_workspace_id) =
            configure_test_cloud_state(&database, &server.base_url(), &cloud_workspace_id);
        database
            .open()
            .unwrap()
            .execute(
                "INSERT INTO device_settings(key,value_json,updated_at) VALUES('cloud_storage_protocol',?1,1) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json",
                [serde_json::to_string(crate::storage_contract::DIRECT_CLOUD_PROTOCOL).unwrap()],
            )
            .unwrap();
        save_session(
            &database,
            &test_session("cloud-to-local-copy-preview", Some(now_seconds() + 3600)),
        )
        .unwrap();

        let preview = migration_preview(
            &database,
            MigrationPreviewRequest {
                direction: "cloud_to_local_snapshot".to_string(),
            },
        )
        .unwrap();

        assert_eq!(preview.source_workspace_id, cloud_workspace_id);
        assert_eq!(
            preview.target_workspace_id.as_deref(),
            Some(local_workspace_id.as_str())
        );
        assert_eq!(preview.source_entity_count, 2);
        assert_eq!(
            preview.target_entity_count,
            preview
                .entities
                .iter()
                .map(|entity| entity.local_count)
                .sum::<i64>()
        );
        assert_eq!(preview.replace_entity_count, preview.target_entity_count);
        assert!(!preview.safety_snapshot_required);
        assert!(preview.can_execute);
        assert_eq!(
            preview
                .entities
                .iter()
                .find(|entity| entity.entity == "subjects")
                .and_then(|entity| entity.cloud_count),
            Some(2)
        );
        let requests = server.finish();
        assert_eq!(requests.len(), 3);
        assert!(requests[2].contains("/rest/v1/rpc/data_copy_export_preview"));
        assert!(requests[2].contains(&device_id));
        assert!(!requests[2].contains("cloud_snapshot_get"));
        reset_test_credentials();
    }

    fn empty_copy_snapshot() -> Value {
        Value::Object(
            crate::data_protection::data_package_copy_entity_names()
                .unwrap()
                .into_iter()
                .map(|entity| (entity, json!([])))
                .collect(),
        )
    }

    #[test]
    fn data_copy_chunks_cover_every_entity_and_have_stable_hashes() {
        let mut snapshot = empty_copy_snapshot();
        snapshot["subjects"] = json!([{
            "id": "10000000-0000-0000-0000-000000000001",
            "name": "主体"
        }]);
        let (first, first_hash) = build_data_copy_chunks(&snapshot).unwrap();
        let (second, second_hash) = build_data_copy_chunks(&snapshot).unwrap();
        assert_eq!(first.len(), 22);
        assert_eq!(first_hash, second_hash);
        assert_eq!(
            first
                .iter()
                .map(|chunk| chunk.content_hash.as_str())
                .collect::<Vec<_>>(),
            second
                .iter()
                .map(|chunk| chunk.content_hash.as_str())
                .collect::<Vec<_>>()
        );
        assert!(first
            .iter()
            .all(|chunk| chunk.content_text.len() <= DATA_COPY_CHUNK_MAX_BYTES));

        snapshot["subjects"][0]["name"] = json!("已修改主体");
        let (_, changed_hash) = build_data_copy_chunks(&snapshot).unwrap();
        assert_ne!(first_hash, changed_hash);
    }

    #[test]
    fn data_copy_chunks_reject_a_single_oversized_row() {
        let mut snapshot = empty_copy_snapshot();
        snapshot["subjects"] = json!([{
            "id": "10000000-0000-0000-0000-000000000001",
            "name": "x".repeat(DATA_COPY_CHUNK_MAX_BYTES)
        }]);
        assert!(build_data_copy_chunks(&snapshot)
            .unwrap_err()
            .starts_with("DATA_PACKAGE_ROW_TOO_LARGE:"));
    }

    #[test]
    fn data_copy_export_validation_rejects_tampered_chunks_and_incomplete_manifests() {
        let entities = crate::data_protection::data_package_copy_entity_names().unwrap();
        let manifest = DataCopyExportManifest {
            package_id: Uuid::now_v7().to_string(),
            package_version: crate::data_protection::DATA_PACKAGE_FORMAT_VERSION,
            workspace_epoch: 3,
            change_seq: 9,
            entity_counts: entities.iter().map(|entity| (entity.clone(), 0)).collect(),
            content_hash: "ab".repeat(32),
            chunk_count: entities.len(),
            expires_at: "2026-10-24T00:00:00Z".to_string(),
        };
        validate_data_copy_export_manifest(&manifest, 3).unwrap();
        let mut incomplete = manifest.clone();
        incomplete.entity_counts.remove("subjects");
        assert!(validate_data_copy_export_manifest(&incomplete, 3).is_err());

        let content_text = "[]".to_string();
        let chunk = DataCopyExportChunk {
            package_id: manifest.package_id.clone(),
            chunk_index: 0,
            entity_name: "subjects".to_string(),
            content_hash: crate::data_protection::sha256_hex(content_text.as_bytes()),
            content_text,
            row_count: 0,
        };
        validate_data_copy_export_chunk(&chunk, &manifest.package_id, 0).unwrap();
        let mut tampered = chunk;
        tampered.content_text = "[{}]".to_string();
        assert!(
            validate_data_copy_export_chunk(&tampered, &manifest.package_id, 0)
                .unwrap_err()
                .starts_with("DATA_PACKAGE_CHUNK_HASH_MISMATCH:")
        );
    }

    fn test_export_package(snapshot: &Value) -> (DataCopyExportManifest, Vec<DataCopyExportChunk>) {
        let (chunks, content_hash) = build_data_copy_chunks(snapshot).unwrap();
        let package_id = Uuid::now_v7().to_string();
        let entity_counts = crate::data_protection::data_package_copy_entity_names()
            .unwrap()
            .into_iter()
            .map(|entity| {
                let count = snapshot
                    .get(&entity)
                    .and_then(Value::as_array)
                    .map(Vec::len)
                    .unwrap_or_default() as i64;
                (entity, count)
            })
            .collect();
        let exported_chunks = chunks
            .into_iter()
            .map(|chunk| DataCopyExportChunk {
                package_id: package_id.clone(),
                chunk_index: chunk.chunk_index,
                entity_name: chunk.entity_name,
                content_text: chunk.content_text,
                content_hash: chunk.content_hash,
                row_count: chunk.row_count,
            })
            .collect::<Vec<_>>();
        (
            DataCopyExportManifest {
                package_id,
                package_version: crate::data_protection::DATA_PACKAGE_FORMAT_VERSION,
                workspace_epoch: 1,
                change_seq: 9,
                entity_counts,
                content_hash,
                chunk_count: exported_chunks.len(),
                expires_at: "2026-10-24T00:00:00Z".to_string(),
            },
            exported_chunks,
        )
    }

    #[test]
    fn cloud_data_copy_download_interruption_and_hash_failure_never_produce_a_snapshot() {
        let snapshot = empty_copy_snapshot();
        let (manifest, chunks) = test_export_package(&snapshot);
        let interrupted = download_data_copy_from_fetch(&manifest, |chunk_index| {
            if chunk_index == 1 {
                Err("NETWORK_ERROR: simulated interrupted download".to_string())
            } else {
                Ok(chunks[chunk_index].clone())
            }
        })
        .unwrap_err();
        assert!(interrupted.starts_with("NETWORK_ERROR:"));

        let mut corrupt_chunks = chunks;
        corrupt_chunks[0].content_text = "[{}]".to_string();
        let corrupt = download_data_copy_from_fetch(&manifest, |chunk_index| {
            Ok(corrupt_chunks[chunk_index].clone())
        })
        .unwrap_err();
        assert!(corrupt.starts_with("DATA_PACKAGE_CHUNK_HASH_MISMATCH:"));
    }

    #[test]
    fn local_data_copy_replace_creates_safety_backup_and_rolls_back_failed_sqlite_write() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("copy-local-rollback.sqlite3")).unwrap();
        let workspace_id = test_workspace_id(&database);
        let before = local_snapshot(&database, &workspace_id).unwrap();
        let before_hash = crate::data_protection::sha256_hex(
            &crate::data_protection::canonical_json_bytes(&before).unwrap(),
        );
        let mut valid = before.clone();
        valid["subjects"][0]["name"] = json!("模拟写入失败的新主体");
        let (manifest, _) = test_export_package(&valid);
        database
            .open()
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER test_fail_data_copy_subject_insert BEFORE INSERT ON subjects BEGIN SELECT RAISE(ABORT,'simulated disk write failure'); END;",
            )
            .unwrap();

        let error =
            replace_local_with_downloaded_data_copy(&database, &manifest, valid).unwrap_err();
        assert!(
            error.contains("simulated disk write failure") || error.contains("导入 subjects 失败"),
            "unexpected simulated SQLite write failure: {error}"
        );
        let after = local_snapshot(&database, &workspace_id).unwrap();
        let after_hash = crate::data_protection::sha256_hex(
            &crate::data_protection::canonical_json_bytes(&after).unwrap(),
        );
        assert_eq!(after_hash, before_hash);
        let backups = std::fs::read_dir(database.data_directory().join("backups"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.path().extension().and_then(|value| value.to_str())
                    == Some("timegenie-backup")
            })
            .count();
        assert_eq!(backups, 1);
    }

    #[test]
    fn local_data_copy_replace_commits_all_entities_without_switching_storage_mode() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("copy-local-success.sqlite3")).unwrap();
        let workspace_id = test_workspace_id(&database);
        database
            .open()
            .unwrap()
            .execute(
                "INSERT INTO app_settings(workspace_id,key,value_json,updated_at,version) VALUES(?1,'daily_report_directory',?2,1,1)",
                params![workspace_id, json!("C:/local/reports").to_string()],
            )
            .unwrap();
        settings::update_integration_config(
            &database,
            settings::IntegrationConfigRequest {
                provider: "seatable".to_string(),
                enabled: true,
                config: json!({"serverUrl": "https://cloud.example"}),
                expected_version: None,
            },
        )
        .unwrap();
        database
            .open()
            .unwrap()
            .execute(
                "UPDATE integration_configs SET secret_ref='keyring://seatable-local' WHERE workspace_id=?1 AND provider='seatable'",
                [&workspace_id],
            )
            .unwrap();
        let mut snapshot = local_snapshot(&database, &workspace_id).unwrap();
        snapshot["subjects"][0]["name"] = json!("来自云端复制的主体");
        snapshot["app_settings"]
            .as_array_mut()
            .unwrap()
            .retain(|row| row.get("key").and_then(Value::as_str) != Some("daily_report_directory"));
        snapshot["integration_configs"][0]
            .as_object_mut()
            .unwrap()
            .remove("secret_ref");
        let (manifest, chunks) = test_export_package(&snapshot);
        let downloaded =
            download_data_copy_from_fetch(&manifest, |chunk_index| Ok(chunks[chunk_index].clone()))
                .unwrap();

        let result =
            replace_local_with_downloaded_data_copy(&database, &manifest, downloaded).unwrap();

        assert!(result.replaced);
        assert!(std::path::Path::new(&result.safety_backup_path).exists());
        assert_eq!(storage_mode(&database).unwrap().mode, "local");
        let connection = database.open().unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT name FROM subjects WHERE workspace_id=?1 ORDER BY sort_order,id LIMIT 1",
                    [&workspace_id],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "来自云端复制的主体"
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT json_extract(value_json,'$') FROM app_settings WHERE workspace_id=?1 AND key='daily_report_directory'",
                    [&workspace_id],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "C:/local/reports"
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT secret_ref FROM integration_configs WHERE workspace_id=?1 AND provider='seatable'",
                    [&workspace_id],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "keyring://seatable-local"
        );
        for (entity, expected_count) in manifest.entity_counts {
            assert_eq!(
                result.entity_counts.get(&entity).copied(),
                Some(expected_count)
            );
        }
    }

    fn local_snapshot_content_hash(database: &Database, workspace_id: &str) -> String {
        let snapshot = local_snapshot(database, workspace_id).unwrap();
        crate::data_protection::sha256_hex(
            &crate::data_protection::canonical_json_bytes(&snapshot).unwrap(),
        )
    }

    fn local_subject_name(database: &Database, workspace_id: &str) -> String {
        database
            .open()
            .unwrap()
            .query_row(
                "SELECT name FROM subjects WHERE workspace_id=?1 ORDER BY sort_order,id LIMIT 1",
                [workspace_id],
                |row| row.get(0),
            )
            .unwrap()
    }

    #[test]
    fn storage_switches_and_full_copies_keep_local_and_cloud_spaces_independent() {
        let local_directory = tempdir().unwrap();
        let cloud_directory = tempdir().unwrap();
        let local =
            Database::initialize_at(local_directory.path().join("independent-local.sqlite3"))
                .unwrap();
        let cloud =
            Database::initialize_at(cloud_directory.path().join("independent-cloud.sqlite3"))
                .unwrap();
        let local_workspace_id = test_workspace_id(&local);
        let cloud_workspace_id = test_workspace_id(&cloud);
        local
            .open()
            .unwrap()
            .execute(
                "UPDATE subjects SET name='本地空间保留事项' WHERE workspace_id=?1",
                [&local_workspace_id],
            )
            .unwrap();
        cloud
            .open()
            .unwrap()
            .execute(
                "UPDATE subjects SET name='云端空间保留事项' WHERE workspace_id=?1",
                [&cloud_workspace_id],
            )
            .unwrap();
        let local_before = local_snapshot_content_hash(&local, &local_workspace_id);
        let cloud_before = local_snapshot_content_hash(&cloud, &cloud_workspace_id);

        for mode in ["cloud", "local", "cloud", "local"] {
            settings::update_setting(
                &local,
                SettingsUpdate {
                    scope: SettingsScope::Device,
                    key: "storage_mode".to_string(),
                    value: json!(mode),
                },
            )
            .unwrap();
            assert_eq!(
                local_before,
                local_snapshot_content_hash(&local, &local_workspace_id)
            );
            assert_eq!(
                cloud_before,
                local_snapshot_content_hash(&cloud, &cloud_workspace_id)
            );
        }

        let local_snapshot_value = local_snapshot(&local, &local_workspace_id).unwrap();
        let (local_manifest, local_chunks) = test_export_package(&local_snapshot_value);
        let downloaded_local =
            download_data_copy_from_fetch(&local_manifest, |index| Ok(local_chunks[index].clone()))
                .unwrap();
        replace_local_with_downloaded_data_copy(&cloud, &local_manifest, downloaded_local).unwrap();
        let local_after_copy = local_snapshot_content_hash(&local, &local_workspace_id);
        assert_eq!(
            local_subject_name(&cloud, &cloud_workspace_id),
            "本地空间保留事项"
        );
        assert_eq!(
            local_after_copy,
            local_snapshot_content_hash(&local, &local_workspace_id)
        );

        cloud
            .open()
            .unwrap()
            .execute(
                "UPDATE subjects SET name='复制后云端独立修改' WHERE workspace_id=?1",
                [&cloud_workspace_id],
            )
            .unwrap();
        assert_eq!(
            local_after_copy,
            local_snapshot_content_hash(&local, &local_workspace_id)
        );
        let cloud_after_edit = local_snapshot_content_hash(&cloud, &cloud_workspace_id);
        assert_eq!(
            local_subject_name(&cloud, &cloud_workspace_id),
            "复制后云端独立修改"
        );
        local
            .open()
            .unwrap()
            .execute(
                "UPDATE subjects SET name='复制后本地独立修改' WHERE workspace_id=?1",
                [&local_workspace_id],
            )
            .unwrap();
        assert_eq!(
            local_subject_name(&cloud, &cloud_workspace_id),
            "复制后云端独立修改"
        );
        assert_eq!(
            cloud_after_edit,
            local_snapshot_content_hash(&cloud, &cloud_workspace_id)
        );

        let cloud_snapshot_value = local_snapshot(&cloud, &cloud_workspace_id).unwrap();
        let (cloud_manifest, cloud_chunks) = test_export_package(&cloud_snapshot_value);
        let downloaded_cloud =
            download_data_copy_from_fetch(&cloud_manifest, |index| Ok(cloud_chunks[index].clone()))
                .unwrap();
        replace_local_with_downloaded_data_copy(&local, &cloud_manifest, downloaded_cloud).unwrap();
        assert_eq!(
            local_subject_name(&local, &local_workspace_id),
            "复制后云端独立修改"
        );
        assert_eq!(
            local_subject_name(&cloud, &cloud_workspace_id),
            "复制后云端独立修改"
        );
    }

    #[test]
    fn both_copy_directions_require_explicit_confirmation() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("copy-confirmation.sqlite3")).unwrap();
        let local_error = replace_staged_local_data_copy(
            &database,
            DataCopyReplaceRequest {
                package_id: Uuid::now_v7().to_string(),
                content_hash: "ab".repeat(32),
                operation_id: Uuid::now_v7().to_string(),
                confirmed: false,
            },
        )
        .unwrap_err();
        assert!(local_error.starts_with("DATA_COPY_REPLACE_CONFIRMATION_REQUIRED:"));

        let cloud_error = replace_local_with_cloud_data_copy(
            &database,
            DataCopyReplaceLocalRequest { confirmed: false },
        )
        .unwrap_err();
        assert!(cloud_error.starts_with("DATA_COPY_REPLACE_CONFIRMATION_REQUIRED:"));
    }

    #[test]
    fn local_to_cloud_copy_preserves_the_local_space_and_creates_no_sync_work() {
        let _guard = lock_test_session_state();
        reset_test_credentials();
        let cloud_workspace_id = Uuid::now_v7().to_string();
        let package_id = Uuid::now_v7().to_string();
        let operation_id = Uuid::now_v7().to_string();
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("local-to-cloud-copy.sqlite3")).unwrap();
        let source_workspace_id = test_workspace_id(&database);
        database
            .open()
            .unwrap()
            .execute(
                "UPDATE subjects SET name='本地复制源事项' WHERE workspace_id=?1",
                [&source_workspace_id],
            )
            .unwrap();
        let source_snapshot = local_snapshot(&database, &source_workspace_id).unwrap();
        let (chunks, content_hash) = build_data_copy_chunks(&source_snapshot).unwrap();
        let entity_counts = source_snapshot
            .as_object()
            .unwrap()
            .iter()
            .map(|(entity, rows)| {
                (
                    entity.clone(),
                    json!(rows.as_array().map(Vec::len).unwrap_or(0)),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        let begin_body = Box::leak(
            json!({"packageId": package_id})
                .to_string()
                .into_boxed_str(),
        );
        let validated_body = Box::leak(
            json!({"status": "validated", "entityCounts": entity_counts})
                .to_string()
                .into_boxed_str(),
        );
        let commit_body = Box::leak(
            json!({
                "replaced": true,
                "packageId": package_id,
                "contentHash": content_hash,
                "safetySnapshotId": Uuid::now_v7().to_string(),
                "previousEpoch": 1,
                "workspaceEpoch": 2,
                "resetChangeSeq": 41,
                "entityCounts": entity_counts
            })
            .to_string()
            .into_boxed_str(),
        );
        let mut responses = vec![TestHttpResponse {
            path: "/rest/v1/rpc/data_copy_stage_begin",
            status: 200,
            body: begin_body,
        }];
        for _ in &chunks {
            responses.push(TestHttpResponse {
                path: "/rest/v1/rpc/data_copy_stage_chunk_put",
                status: 200,
                body: "{}",
            });
        }
        responses.push(TestHttpResponse {
            path: "/rest/v1/rpc/data_copy_stage_validate",
            status: 200,
            body: validated_body,
        });
        responses.push(TestHttpResponse {
            path: "/rest/v1/rpc/data_copy_replace_commit",
            status: 200,
            body: commit_body,
        });
        let server = TestHttpServer::start(responses);
        configure_test_cloud_state(&database, &server.base_url(), &cloud_workspace_id);
        database
            .open()
            .unwrap()
            .execute(
                "INSERT INTO device_settings(key,value_json,updated_at) VALUES('cloud_storage_protocol',?1,1) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json",
                [serde_json::to_string(crate::storage_contract::DIRECT_CLOUD_PROTOCOL).unwrap()],
            )
            .unwrap();
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "storage_mode".to_string(),
                value: json!("local"),
            },
        )
        .unwrap();
        save_session(
            &database,
            &test_session("local-to-cloud-copy", Some(now_seconds() + 3600)),
        )
        .unwrap();
        let before_hash = local_snapshot_content_hash(&database, &source_workspace_id);

        let staged = stage_local_data_copy(&database).unwrap();
        assert_eq!(staged.package_id, package_id);
        assert_eq!(staged.content_hash, content_hash);
        let replaced = replace_staged_local_data_copy(
            &database,
            DataCopyReplaceRequest {
                package_id,
                content_hash,
                operation_id,
                confirmed: true,
            },
        )
        .unwrap();
        assert!(replaced.replaced);
        assert_eq!(storage_mode(&database).unwrap().mode, "local");
        assert_eq!(
            before_hash,
            local_snapshot_content_hash(&database, &source_workspace_id)
        );
        assert_eq!(
            database
                .open()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM sync_outbox", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        let requests = server.finish();
        assert_eq!(requests.len(), 3 + chunks.len());
    }

    #[test]
    fn local_cloud_local_data_copy_round_trip_preserves_counts_and_content_hash() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("copy-round-trip.sqlite3")).unwrap();
        let workspace_id = test_workspace_id(&database);
        let source = local_snapshot(&database, &workspace_id).unwrap();
        let (manifest, chunks) = test_export_package(&source);
        let source_counts = manifest.entity_counts.clone();
        let source_hash = manifest.content_hash.clone();
        let downloaded =
            download_data_copy_from_fetch(&manifest, |chunk_index| Ok(chunks[chunk_index].clone()))
                .unwrap();

        replace_local_with_downloaded_data_copy(&database, &manifest, downloaded).unwrap();

        let restored = local_snapshot(&database, &workspace_id).unwrap();
        let (restored_manifest, _) = test_export_package(&restored);
        assert_eq!(restored_manifest.entity_counts, source_counts);
        assert_eq!(restored_manifest.content_hash, source_hash);
    }

    #[test]
    fn legacy_upgrade_queue_preflight_counts_every_blocking_state() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("upgrade-preflight.sqlite3")).unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        for (index, state) in ["pending", "sending", "failed", "conflict"]
            .iter()
            .enumerate()
        {
            connection.execute(
                "INSERT INTO sync_outbox(operation_id,workspace_id,device_id,operation_type,entity_type,payload_json,state,created_at) VALUES(?1,?2,?3,'test','task','{}',?4,?5)",
                params![Uuid::now_v7().to_string(), workspace_id, device_id, state, index as i64],
            ).unwrap();
        }
        let counts = legacy_queue_counts(&connection, &workspace_id).unwrap();
        assert_eq!(counts.pending, 1);
        assert_eq!(counts.sending, 1);
        assert_eq!(counts.failed, 1);
        assert_eq!(counts.conflict, 1);
        assert_eq!(counts.total, 4);
    }

    #[test]
    fn legacy_upgrade_queue_preflight_allows_an_empty_queue() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("upgrade-empty.sqlite3")).unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            legacy_queue_counts(&connection, &workspace_id).unwrap(),
            LegacyQueueCounts::default()
        );
    }

    #[test]
    fn legacy_upgrade_preflight_decision_covers_empty_blocked_and_stale_cases() {
        let server = |workspace_epoch| StorageProtocolUpgradeServerPreflight {
            workspace_id: "20000000-0000-0000-0000-000000000001".to_string(),
            device_id: "30000000-0000-0000-0000-000000000001".to_string(),
            storage_protocol: "sync_v1".to_string(),
            workspace_epoch,
            server_upgrade_allowed: true,
            already_direct: false,
            requires_local_queue_disposition: true,
            allowed_queue_dispositions: vec![
                "queue_drained".to_string(),
                "abandoned_with_rescue".to_string(),
            ],
            server_time: "2026-10-23T20:00:00Z".to_string(),
        };
        let clean = build_storage_protocol_upgrade_preflight(
            server(4),
            "sync_v1".to_string(),
            4,
            Vec::new(),
            LegacyQueueCounts::default(),
        );
        assert!(clean.can_upgrade_with_drained_queue);
        assert!(clean.blocked_reasons.is_empty());

        let blocked = build_storage_protocol_upgrade_preflight(
            server(4),
            "sync_v1".to_string(),
            4,
            Vec::new(),
            LegacyQueueCounts {
                pending: 1,
                sending: 1,
                failed: 1,
                conflict: 1,
                total: 4,
            },
        );
        assert!(!blocked.can_upgrade_with_drained_queue);
        assert!(blocked.blocked_reasons[0].contains("等待 1、发送中 1、失败 1、冲突 1"));

        let stale = build_storage_protocol_upgrade_preflight(
            server(5),
            "sync_v1".to_string(),
            4,
            Vec::new(),
            LegacyQueueCounts::default(),
        );
        assert!(!stale.can_upgrade_with_drained_queue);
        assert!(stale.blocked_reasons[0].contains("本机工作空间世代为 4，云端为 5"));
    }

    fn upgrade_preflight_for_test(queue_total: i64) -> StorageProtocolUpgradePreflight {
        StorageProtocolUpgradePreflight {
            workspace_id: "20000000-0000-0000-0000-000000000001".to_string(),
            device_id: "30000000-0000-0000-0000-000000000001".to_string(),
            storage_protocol: "sync_v1".to_string(),
            local_protocol: "sync_v1".to_string(),
            workspace_epoch: 4,
            local_workspace_epoch: 4,
            server_upgrade_allowed: true,
            already_direct: false,
            requires_local_queue_disposition: true,
            allowed_queue_dispositions: vec![
                "queue_drained".to_string(),
                "abandoned_with_rescue".to_string(),
            ],
            devices: Vec::new(),
            queue: LegacyQueueCounts {
                pending: queue_total,
                sending: 0,
                failed: 0,
                conflict: 0,
                total: queue_total,
            },
            can_upgrade_with_drained_queue: queue_total == 0,
            blocked_reasons: Vec::new(),
            server_time: "2026-10-23T20:00:00Z".to_string(),
        }
    }

    fn drained_queue_for_test() -> crate::cloud_sync::LegacyQueueDrainResult {
        crate::cloud_sync::LegacyQueueDrainResult {
            pushed: 3,
            last_change_seq: 19,
            domains: vec!["tasks".to_string(), "time".to_string()],
        }
    }

    #[test]
    fn legacy_queue_upgrade_switches_only_after_drain_and_authoritative_refresh() {
        use std::cell::Cell;

        let initial = upgrade_preflight_for_test(2);
        let drain_calls = Cell::new(0);
        let switch_calls = Cell::new(0);
        let (switched, refreshed, drained) = run_legacy_queue_upgrade_flow(
            &initial,
            || {
                drain_calls.set(drain_calls.get() + 1);
                Ok(drained_queue_for_test())
            },
            || Ok(upgrade_preflight_for_test(0)),
            |_| {
                switch_calls.set(switch_calls.get() + 1);
                Ok(StorageProtocolUpgradeServerResult {
                    upgraded: true,
                    already_direct: false,
                    workspace_epoch: 4,
                    storage_protocol: Some("direct_v2".to_string()),
                    change_seq: Some(20),
                    safety_snapshot_id: None,
                })
            },
        )
        .unwrap();
        assert_eq!(drain_calls.get(), 1);
        assert_eq!(switch_calls.get(), 1);
        assert!(switched.upgraded);
        assert_eq!(refreshed.queue.total, 0);
        assert_eq!(drained.pushed, 3);

        let blocked = run_legacy_queue_upgrade_flow(
            &upgrade_preflight_for_test(1),
            || Ok(drained_queue_for_test()),
            || Ok(upgrade_preflight_for_test(1)),
            |_| {
                switch_calls.set(switch_calls.get() + 1);
                panic!("blocked queue must not switch protocol");
            },
        )
        .unwrap_err();
        assert!(blocked.starts_with("LEGACY_QUEUE_BLOCKED:"));
        assert_eq!(switch_calls.get(), 1);

        let drain_failed = run_legacy_queue_upgrade_flow(
            &upgrade_preflight_for_test(1),
            || Err("NETWORK_ERROR: simulated push failure".to_string()),
            || Ok(upgrade_preflight_for_test(0)),
            |_| panic!("failed drain must not switch protocol"),
        )
        .unwrap_err();
        assert!(drain_failed.starts_with("NETWORK_ERROR:"));
    }

    #[test]
    fn drained_upgrade_persists_direct_protocol_without_writing_business_tables() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("upgrade-apply.sqlite3")).unwrap();
        let workspace_id = Uuid::now_v7().to_string();
        let device_id = storage_mode(&database).unwrap().device_id;
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "cloud_workspace_id".to_string(),
                value: json!(workspace_id),
            },
        )
        .unwrap();
        database
            .open()
            .unwrap()
            .execute(
                "INSERT INTO local_sync_state(workspace_id,device_id,last_change_seq,workspace_epoch) VALUES(?1,?2,12,4)",
                params![workspace_id, device_id],
            )
            .unwrap();
        let preflight = upgrade_preflight_for_test(0);
        let server = StorageProtocolUpgradeServerResult {
            upgraded: true,
            already_direct: false,
            workspace_epoch: 4,
            storage_protocol: Some("direct_v2".to_string()),
            change_seq: Some(20),
            safety_snapshot_id: Some(Uuid::now_v7().to_string()),
        };
        let before = crate::storage_contract::business_data_fingerprint(&database).unwrap();
        let applied = apply_legacy_queue_upgrade_result(
            &database,
            &server,
            &drained_queue_for_test(),
            &preflight,
        )
        .unwrap();
        assert_eq!(applied.storage_protocol, "direct_v2");
        assert_eq!(applied.pushed_operations, 3);
        assert_eq!(applied.last_change_seq, 20);
        assert_eq!(
            storage_mode(&database).unwrap().configured_cloud_protocol,
            "direct_v2"
        );
        assert_eq!(
            before,
            crate::storage_contract::business_data_fingerprint(&database).unwrap()
        );
    }

    fn insert_legacy_outbox_for_test(database: &Database, workspace_id: &str, count: i64) {
        let device_id = storage_mode(database).unwrap().device_id;
        let connection = database.open().unwrap();
        for index in 0..count {
            connection
                .execute(
                    "INSERT INTO sync_outbox(operation_id,workspace_id,device_id,operation_type,entity_type,payload_json,state,attempt_count,created_at,workspace_epoch,error_json) VALUES(?1,?2,?3,'task_update','task',?4,?5,1,?6,4,?7)",
                    params![
                        Uuid::now_v7().to_string(),
                        workspace_id,
                        device_id,
                        json!({"title": format!("旧修改 {index}")}).to_string(),
                        if index % 2 == 0 { "pending" } else { "failed" },
                        index,
                        json!({"message": "VERSION_CONFLICT: 本地保留"}).to_string()
                    ],
                )
                .unwrap();
        }
    }

    fn test_rescue_backup_for_preflight(
        preflight: &StorageProtocolUpgradePreflight,
    ) -> crate::data_protection::ValidatedRescueBackup {
        crate::data_protection::ValidatedRescueBackup {
            created_at: "2026-10-23T20:00:00.000Z".to_string(),
            checksum: "a".repeat(64),
            source_workspace_marker: crate::data_protection::sha256_hex(
                preflight.workspace_id.as_bytes(),
            ),
            pending_operation_count: preflight.queue.total,
            legacy_queue_operation_count: preflight.queue.total as usize,
        }
    }

    #[test]
    fn rescue_abandon_requires_explicit_second_confirmation() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("abandon-confirmation.sqlite3")).unwrap();
        let request = |confirmed, text: &str| StorageProtocolAbandonRequest {
            operation_id: Uuid::now_v7().to_string(),
            rescue_backup_path: "missing.timegenie-backup".to_string(),
            confirmed,
            confirmation_text: text.to_string(),
            create_safety_snapshot: Some(true),
        };
        let unconfirmed =
            storage_protocol_upgrade_after_rescue(&database, request(false, "放弃旧修改"))
                .unwrap_err();
        assert!(unconfirmed.starts_with("ABANDON_NOT_CONFIRMED:"));
        let wrong_text =
            storage_protocol_upgrade_after_rescue(&database, request(true, "确认")).unwrap_err();
        assert!(wrong_text.starts_with("ABANDON_CONFIRMATION_MISMATCH:"));
    }

    #[test]
    fn cloud_authority_reset_requires_confirmation_before_any_remote_check() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(
            directory
                .path()
                .join("authority-reset-confirmation.sqlite3"),
        )
        .unwrap();
        let error = storage_protocol_reset_to_cloud_authority(
            &database,
            StorageProtocolAuthorityResetRequest {
                operation_id: Uuid::now_v7().to_string(),
                confirmed: false,
                create_safety_snapshot: Some(true),
            },
        )
        .unwrap_err();
        assert!(error.starts_with("CLOUD_AUTHORITY_RESET_NOT_CONFIRMED:"));
    }

    #[test]
    fn rescue_abandon_isolates_queue_and_restores_it_when_protocol_stays_legacy() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("abandon-queue.sqlite3")).unwrap();
        let preflight = upgrade_preflight_for_test(2);
        insert_legacy_outbox_for_test(&database, &preflight.workspace_id, 2);
        let backup = test_rescue_backup_for_preflight(&preflight);
        let failure = run_abandon_queue_upgrade_flow(
            &preflight,
            &backup,
            || {
                quarantine_legacy_queue(
                    &database,
                    &preflight.workspace_id,
                    &backup.checksum,
                    "测试放弃原因",
                )
            },
            |batch_id| restore_quarantined_legacy_queue(&database, batch_id),
            |_| Err("NETWORK_ERROR: simulated switch failure".to_string()),
            || Ok(true),
        )
        .unwrap_err();
        assert!(failure.contains("旧队列已恢复到 sync_v1"));
        let connection = database.open().unwrap();
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM sync_outbox", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM legacy_abandoned_outbox", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            0
        );
        drop(connection);

        let (switched, batch_id, quarantined) = run_abandon_queue_upgrade_flow(
            &preflight,
            &backup,
            || {
                quarantine_legacy_queue(
                    &database,
                    &preflight.workspace_id,
                    &backup.checksum,
                    "用户确认导出后放弃旧修改",
                )
            },
            |batch_id| restore_quarantined_legacy_queue(&database, batch_id),
            |_| {
                Ok(StorageProtocolUpgradeServerResult {
                    upgraded: true,
                    already_direct: false,
                    workspace_epoch: 4,
                    storage_protocol: Some("direct_v2".to_string()),
                    change_seq: Some(30),
                    safety_snapshot_id: Some(Uuid::now_v7().to_string()),
                })
            },
            || Ok(true),
        )
        .unwrap();
        assert!(switched.upgraded);
        assert_eq!(quarantined, 2);
        let connection = database.open().unwrap();
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM sync_outbox", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        let (isolated, checksum, reason): (i64, String, String) = connection
            .query_row(
                "SELECT COUNT(*),MIN(rescue_backup_checksum),MIN(quarantine_reason) FROM legacy_abandoned_outbox WHERE quarantine_batch_id=?1",
                [&batch_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(isolated, 2);
        assert_eq!(checksum, backup.checksum);
        assert!(reason.contains("放弃旧修改"));
    }

    #[test]
    fn data_copy_replace_retries_lost_response_without_switching_or_writing_local_business_data() {
        let _guard = lock_test_session_state();
        reset_test_credentials();
        let cloud_workspace_id = Uuid::now_v7().to_string();
        let package_id = Uuid::now_v7().to_string();
        let operation_id = Uuid::now_v7().to_string();
        let safety_snapshot_id = Uuid::now_v7().to_string();
        let content_hash = "ab".repeat(32);
        let response_body = Box::leak(
            json!({
                "replaced": true,
                "packageId": package_id,
                "contentHash": content_hash,
                "safetySnapshotId": safety_snapshot_id,
                "previousEpoch": 1,
                "workspaceEpoch": 2,
                "resetChangeSeq": 42,
                "entityCounts": {"subjects": 1}
            })
            .to_string()
            .into_boxed_str(),
        );
        let server = TestHttpServer::start_with_disconnect_then_response(
            TestHttpDisconnect {
                path: "/rest/v1/rpc/data_copy_replace_commit",
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/data_copy_replace_commit",
                status: 200,
                body: response_body,
            },
        );
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("copy-replace-retry.sqlite3")).unwrap();
        let (device_id, local_workspace_id) =
            configure_test_cloud_state(&database, &server.base_url(), &cloud_workspace_id);
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "storage_mode".to_string(),
                value: json!("local"),
            },
        )
        .unwrap();
        database
            .open()
            .unwrap()
            .execute(
                "INSERT INTO device_settings(key,value_json,updated_at) VALUES('cloud_storage_protocol',?1,1) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json",
                [serde_json::to_string(crate::storage_contract::DIRECT_CLOUD_PROTOCOL).unwrap()],
            )
            .unwrap();
        save_session(
            &database,
            &test_session("copy-replace-retry", Some(now_seconds() + 3600)),
        )
        .unwrap();
        let subjects_before: i64 = database
            .open()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM subjects WHERE workspace_id=?1",
                [&local_workspace_id],
                |row| row.get(0),
            )
            .unwrap();

        let result = replace_staged_local_data_copy(
            &database,
            DataCopyReplaceRequest {
                package_id: package_id.clone(),
                content_hash: content_hash.clone(),
                operation_id: operation_id.clone(),
                confirmed: true,
            },
        )
        .unwrap();

        assert!(result.replaced);
        assert_eq!(result.workspace_epoch, 2);
        assert_eq!(storage_mode(&database).unwrap().mode, "local");
        assert_eq!(
            database
                .open()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM subjects WHERE workspace_id=?1",
                    [&local_workspace_id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            subjects_before
        );
        assert_eq!(
            database
                .open()
                .unwrap()
                .query_row(
                    "SELECT device_id,last_change_seq,workspace_epoch FROM local_sync_state WHERE workspace_id=?1",
                    [&cloud_workspace_id],
                    |row| Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    )),
                )
                .unwrap(),
            (device_id, 42, 2)
        );
        let requests = server.finish();
        assert_eq!(requests.len(), 2);
        assert!(requests
            .iter()
            .all(|request| request.contains(&operation_id)));
        assert!(requests.iter().all(|request| request.contains(&package_id)));
        reset_test_credentials();
    }

    #[test]
    fn local_to_cloud_preview_reports_nonempty_cloud_replace_impact() {
        let _guard = lock_test_session_state();
        reset_test_credentials();
        let cloud_workspace_id = Uuid::now_v7().to_string();
        let server = TestHttpServer::start(vec![
            TestHttpResponse {
                path: "/rest/v1/workspaces",
                status: 200,
                body: Box::leak(
                    format!(
                        r#"[{{"id":"{cloud_workspace_id}","name":"非空云端","timezone":"Asia/Shanghai"}}]"#
                    )
                    .into_boxed_str(),
                ),
            },
            TestHttpResponse {
                path: "/rest/v1/workspace_changes",
                status: 200,
                body: "[]",
            },
            TestHttpResponse {
                path: "/rest/v1/rpc/cloud_snapshot_get",
                status: 200,
                body: r#"{"app_settings":[{"workspace_id":"cloud","key":"timezone","value_json":"Asia/Shanghai","version":1}]}"#,
            },
        ]);
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("nonempty-copy-preview.sqlite3"))
                .unwrap();
        configure_test_cloud_state(&database, &server.base_url(), &cloud_workspace_id);
        save_session(
            &database,
            &test_session("nonempty-copy-preview", Some(now_seconds() + 3600)),
        )
        .unwrap();

        let preview = migration_preview(
            &database,
            MigrationPreviewRequest {
                direction: "local_to_cloud".to_string(),
            },
        )
        .unwrap();
        assert!(!preview.target_is_empty);
        assert_eq!(preview.target_entity_count, 1);
        assert_eq!(preview.replace_entity_count, 1);
        assert!(preview.safety_snapshot_required);
        assert!(preview.can_execute);
        assert!(preview.conflicts.is_empty());
        assert_eq!(
            preview
                .entities
                .iter()
                .find(|entity| entity.entity == "app_settings")
                .and_then(|entity| entity.cloud_count),
            Some(1)
        );
        assert_eq!(server.finish().len(), 3);
        reset_test_credentials();
    }

    #[test]
    fn local_snapshot_preserves_json_setting_and_config_types() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("snapshot-json.sqlite3")).unwrap();
        let workspace_id: String = database
            .open()
            .unwrap()
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();

        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Shared,
                key: "salary_hourly_rate".to_string(),
                value: json!(88.5),
            },
        )
        .unwrap();
        settings::update_integration_config(
            &database,
            settings::IntegrationConfigRequest {
                provider: "seatable".to_string(),
                enabled: true,
                config: json!({"serverUrl": "https://example.test", "batchSize": 100}),
                expected_version: None,
            },
        )
        .unwrap();
        database
            .open()
            .unwrap()
            .execute(
                "UPDATE integration_configs SET secret_ref = 'keyring://seatable' WHERE workspace_id = ?1 AND provider = 'seatable'",
                [&workspace_id],
            )
            .unwrap();

        let snapshot = local_snapshot(&database, &workspace_id).unwrap();
        let setting = snapshot["app_settings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["key"] == "salary_hourly_rate")
            .unwrap();
        assert_eq!(setting["value_json"], json!(88.5));
        assert!(setting["value_json"].is_number());

        let config = snapshot["integration_configs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["provider"] == "seatable")
            .unwrap();
        assert_eq!(
            config["config_json"],
            json!({"serverUrl": "https://example.test", "batchSize": 100})
        );
        assert!(config["config_json"].is_object());
        assert!(config.get("secret_ref").is_none());
        assert!(!snapshot.to_string().contains("keyring://"));
    }

    #[test]
    fn local_snapshot_excludes_device_hook_rules_and_runs() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("snapshot-hooks.sqlite3")).unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let hook_id = uuid::Uuid::now_v7().to_string();
        let now = now_seconds() * 1000;
        connection
            .execute(
                "INSERT INTO device_hooks(id,name,event_type,action_type,action_config_json,timeout_seconds,enabled,sort_order,created_at,updated_at)
                 VALUES (?1,'local only','task.completed','uri','{\"uriTemplate\":\"test://done\"}',10,1,10,?2,?2)",
                params![hook_id, now],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO device_hook_runs(id,hook_id,hook_name_snapshot,event_id,event_type,is_test,status,created_at)
                 VALUES (?1,?2,'local only','task.completed:test','task.completed',1,'succeeded',?3)",
                params![uuid::Uuid::now_v7().to_string(), hook_id, now],
            )
            .unwrap();
        drop(connection);

        let snapshot = local_snapshot(&database, &workspace_id).unwrap();
        assert!(snapshot.get("device_hooks").is_none());
        assert!(snapshot.get("device_hook_runs").is_none());
        assert!(snapshot.get("device_settings").is_none());
    }
}
