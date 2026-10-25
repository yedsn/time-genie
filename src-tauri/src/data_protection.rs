use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::{Cursor, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use chrono::{SecondsFormat, Utc};
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

pub const DATA_PACKAGE_FORMAT_VERSION: u32 = 1;
pub const BACKUP_FORMAT_VERSION: u32 = DATA_PACKAGE_FORMAT_VERSION;
pub const DATA_PACKAGE_CATALOG_JSON: &str = include_str!("../data-package-catalog.json");
pub const MAX_ARCHIVE_MEMBERS: usize = 16;
pub const MAX_ARCHIVE_MEMBER_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_ARCHIVE_TOTAL_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DataPackageCatalogScope {
    entities: Vec<DataPackageCatalogScopeEntity>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DataPackageCatalogScopeEntity {
    name: String,
    copy: String,
}

pub fn data_package_copy_entity_names() -> Result<Vec<String>, String> {
    let catalog: DataPackageCatalogScope = serde_json::from_str(DATA_PACKAGE_CATALOG_JSON)
        .map_err(|error| format!("DATA_PACKAGE_CATALOG_INVALID: {error}"))?;
    Ok(catalog
        .entities
        .into_iter()
        .filter(|entity| entity.copy == "include")
        .map(|entity| entity.name)
        .collect())
}

pub const ARCHIVE_MEMBERS: [&str; 7] = [
    "manifest.json",
    "workspace.json",
    "data/planning.json",
    "data/time-tracking.json",
    "data/reports.json",
    "data/shared-settings.json",
    "checksums.json",
];
pub const LEGACY_SYNC_ARCHIVE_MEMBER: &str = "data/legacy-sync.json";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtectionGroup {
    Planning,
    TimeTracking,
    Reports,
    SharedSettings,
}

impl ProtectionGroup {
    pub const fn archive_member(self) -> &'static str {
        match self {
            Self::Planning => "data/planning.json",
            Self::TimeTracking => "data/time-tracking.json",
            Self::Reports => "data/reports.json",
            Self::SharedSettings => "data/shared-settings.json",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ProtectedEntity {
    pub table: &'static str,
    #[cfg_attr(not(test), allow(dead_code))]
    pub sync_entity: Option<&'static str>,
    pub group: ProtectionGroup,
    pub stable_order: &'static str,
    #[cfg_attr(not(test), allow(dead_code))]
    pub import_order: u16,
    #[allow(dead_code)]
    pub label_field: Option<&'static str>,
    #[allow(dead_code)]
    pub log_fields: &'static [&'static str],
}

pub const PROTECTED_ENTITIES: &[ProtectedEntity] = &[
    ProtectedEntity {
        table: "subjects",
        sync_entity: Some("subjects"),
        group: ProtectionGroup::Planning,
        stable_order: "sort_order,id",
        import_order: 10,
        label_field: Some("name"),
        log_fields: &["name", "sort_order", "deleted_at"],
    },
    ProtectedEntity {
        table: "subject_work_schedules",
        sync_entity: Some("subject_work_schedules"),
        group: ProtectionGroup::Planning,
        stable_order: "subject_id,weekday,id",
        import_order: 20,
        label_field: None,
        log_fields: &["weekday", "is_workday"],
    },
    ProtectedEntity {
        table: "subject_work_schedule_segments",
        sync_entity: None,
        group: ProtectionGroup::Planning,
        stable_order: "schedule_id,sequence_no,id",
        import_order: 30,
        label_field: None,
        log_fields: &["sequence_no", "start_minute", "end_minute"],
    },
    ProtectedEntity {
        table: "work_days",
        sync_entity: Some("work_days"),
        group: ProtectionGroup::Planning,
        stable_order: "work_date",
        import_order: 40,
        label_field: Some("work_date"),
        log_fields: &["timezone", "work_period_text", "note", "settled_at"],
    },
    ProtectedEntity {
        table: "tasks",
        sync_entity: Some("tasks"),
        group: ProtectionGroup::Planning,
        stable_order: "subject_id,parent_id,sort_order,id",
        import_order: 50,
        label_field: Some("title"),
        log_fields: &[
            "title",
            "status",
            "parent_id",
            "planned_date",
            "planned_time",
            "estimate_minutes",
            "note",
            "project_name",
            "solution_name",
            "sort_order",
            "completed_at",
            "deleted_at",
        ],
    },
    ProtectedEntity {
        table: "task_status_events",
        sync_entity: Some("task_status_events"),
        group: ProtectionGroup::Planning,
        stable_order: "occurred_at,id",
        import_order: 60,
        label_field: None,
        log_fields: &["status", "occurred_at", "source_type"],
    },
    ProtectedEntity {
        table: "task_day_plans",
        sync_entity: Some("task_day_plans"),
        group: ProtectionGroup::Planning,
        stable_order: "task_id,work_date",
        import_order: 65,
        label_field: None,
        log_fields: &["work_date", "state"],
    },
    ProtectedEntity {
        table: "task_daily_estimates",
        sync_entity: Some("task_daily_estimates"),
        group: ProtectionGroup::Planning,
        stable_order: "task_id,work_date",
        import_order: 70,
        label_field: None,
        log_fields: &["work_date", "estimate_minutes"],
    },
    ProtectedEntity {
        table: "task_recurrence_rules",
        sync_entity: Some("task_recurrence_rules"),
        group: ProtectionGroup::Planning,
        stable_order: "task_id,effective_start,id",
        import_order: 80,
        label_field: None,
        log_fields: &[
            "frequency",
            "weekdays_mask",
            "effective_start",
            "effective_end",
        ],
    },
    ProtectedEntity {
        table: "task_occurrences",
        sync_entity: Some("task_occurrences"),
        group: ProtectionGroup::Planning,
        stable_order: "task_id,occurrence_date",
        import_order: 90,
        label_field: None,
        log_fields: &["occurrence_date", "origin", "status", "completed_at"],
    },
    ProtectedEntity {
        table: "time_entries",
        sync_entity: Some("time_entries"),
        group: ProtectionGroup::TimeTracking,
        stable_order: "started_at,id",
        import_order: 100,
        label_field: Some("label_snapshot"),
        log_fields: &[
            "work_date",
            "kind",
            "source_type",
            "state",
            "default_task_id",
            "label_snapshot",
            "started_at",
            "ended_at",
            "duration_seconds",
            "note",
            "deleted_at",
        ],
    },
    ProtectedEntity {
        table: "time_segments",
        sync_entity: Some("time_segments"),
        group: ProtectionGroup::TimeTracking,
        stable_order: "entry_id,sequence_no,id",
        import_order: 110,
        label_field: None,
        log_fields: &["sequence_no", "started_at", "ended_at", "duration_seconds"],
    },
    ProtectedEntity {
        table: "time_allocations",
        sync_entity: Some("time_allocations"),
        group: ProtectionGroup::TimeTracking,
        stable_order: "entry_id,task_id,id",
        import_order: 120,
        label_field: None,
        log_fields: &["task_id", "minutes", "note"],
    },
    ProtectedEntity {
        table: "unassigned_sessions",
        sync_entity: Some("unassigned_sessions"),
        group: ProtectionGroup::TimeTracking,
        stable_order: "first_started_at,id",
        import_order: 130,
        label_field: Some("work_date"),
        log_fields: &[
            "work_date",
            "state",
            "duration_seconds",
            "resolution_type",
            "resolved_at",
        ],
    },
    ProtectedEntity {
        table: "unassigned_segments",
        sync_entity: None,
        group: ProtectionGroup::TimeTracking,
        stable_order: "session_id,sequence_no,id",
        import_order: 140,
        label_field: None,
        log_fields: &["sequence_no", "started_at", "ended_at", "duration_seconds"],
    },
    ProtectedEntity {
        table: "actual_time_adjustments",
        sync_entity: Some("actual_time_adjustments"),
        group: ProtectionGroup::TimeTracking,
        stable_order: "work_date,created_at,id",
        import_order: 150,
        label_field: Some("work_date"),
        log_fields: &[
            "task_id",
            "work_date",
            "delta_minutes",
            "baseline_minutes",
            "target_minutes",
            "note",
            "deleted_at",
        ],
    },
    ProtectedEntity {
        table: "daily_report_import_sources",
        sync_entity: None,
        group: ProtectionGroup::Planning,
        stable_order: "source_date,target_kind,source_fingerprint,id",
        import_order: 155,
        label_field: Some("source_date"),
        log_fields: &["subject_id", "source_date", "target_kind", "task_id"],
    },
    ProtectedEntity {
        table: "report_templates",
        sync_entity: Some("report_templates"),
        group: ProtectionGroup::Reports,
        stable_order: "report_type,sort_order,id",
        import_order: 160,
        label_field: Some("name"),
        log_fields: &[
            "report_type",
            "name",
            "description",
            "content",
            "is_default",
            "sort_order",
            "deleted_at",
        ],
    },
    ProtectedEntity {
        table: "reports",
        sync_entity: Some("reports"),
        group: ProtectionGroup::Reports,
        stable_order: "period_start,report_type,id",
        import_order: 170,
        label_field: Some("reference_date"),
        log_fields: &[
            "report_type",
            "period_start",
            "period_end",
            "reference_date",
            "markdown_content",
            "content_source",
            "deleted_at",
        ],
    },
    ProtectedEntity {
        table: "report_tasks",
        sync_entity: Some("report_tasks"),
        group: ProtectionGroup::Reports,
        stable_order: "report_id,sort_order,task_id",
        import_order: 180,
        label_field: Some("title_snapshot"),
        log_fields: &["title_snapshot", "path_snapshot", "sort_order"],
    },
    ProtectedEntity {
        table: "app_settings",
        sync_entity: Some("app_settings"),
        group: ProtectionGroup::SharedSettings,
        stable_order: "key",
        import_order: 190,
        label_field: Some("key"),
        log_fields: &["key", "value_json"],
    },
    ProtectedEntity {
        table: "integration_configs",
        sync_entity: Some("integration_configs"),
        group: ProtectionGroup::SharedSettings,
        stable_order: "provider",
        import_order: 200,
        label_field: Some("provider"),
        log_fields: &["provider", "enabled"],
    },
    ProtectedEntity {
        table: "external_bindings",
        sync_entity: Some("external_bindings"),
        group: ProtectionGroup::SharedSettings,
        stable_order: "provider,entity_type,entity_id,id",
        import_order: 210,
        label_field: Some("provider"),
        log_fields: &[
            "provider",
            "entity_type",
            "external_id",
            "external_revision",
            "last_synced_at",
        ],
    },
];

#[cfg(test)]
pub const EXCLUDED_TABLES: &[&str] = &[
    "devices",
    "device_settings",
    "legacy_sync_cleanup_diagnostics",
    "local_request_results",
    "tracking_runtime_state",
    "device_hooks",
    "device_hook_runs",
    "local_report_outputs",
    "integration_runs",
    "integration_run_items",
    "actual_adjustment_batches",
    "plan_import_batches",
    "plan_import_items",
    "daily_report_import_batches",
    "daily_report_import_items",
];

pub const EXCLUDED_FIELDS: &[&str] = &[
    "secret_ref",
    "access_token",
    "refresh_token",
    "anon_key",
    "service_role",
    "lease_token",
    "auth_session_id",
    "created_by_device_id",
    "updated_by_device_id",
];

const BACKUP_KIND_STANDARD: &str = "standard";
const BACKUP_KIND_RESCUE: &str = "stale_epoch_rescue";
const EXCLUDED_CATEGORIES: &[&str] = &[
    "系统凭据与 Token",
    "Supabase 项目连接与登录会话",
    "设备授权身份",
    "同步操作 payload、设备身份与同步游标",
    "自动化命令及执行记录",
    "运行时租约与本机路径",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupManifest {
    format_version: u32,
    app_version: String,
    created_at: String,
    frozen_at: i64,
    backup_kind: String,
    source_mode: String,
    source_workspace_marker: String,
    source_workspace_epoch: i64,
    cloud_consistent: bool,
    pending_operation_count: i64,
    entity_counts: BTreeMap<String, usize>,
    excluded_categories: Vec<String>,
    #[serde(default)]
    legacy_sync_diagnostics: Option<LegacySyncDiagnostics>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LegacySyncQueueCounts {
    pub pending: i64,
    pub sending: i64,
    pub failed: i64,
    pub conflict: i64,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LegacySyncOperationDiagnostic {
    pub sequence: i64,
    pub operation_type: String,
    pub entity_type: String,
    pub state: String,
    pub attempt_count: i64,
    pub base_version: Option<i64>,
    pub payload_version: Option<i64>,
    pub workspace_epoch: i64,
    pub error_code: Option<String>,
    pub created_at: i64,
    pub last_attempt_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LegacySyncDiagnostics {
    pub disposition_reason: String,
    pub queue_counts: LegacySyncQueueCounts,
    pub operations: Vec<LegacySyncOperationDiagnostic>,
    pub operations_truncated: bool,
    pub business_data_included: bool,
    pub raw_payload_excluded: bool,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceMetadata {
    name: String,
    timezone: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupExportRequest {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupExportResult {
    pub path: String,
    pub created_at: String,
    pub backup_kind: String,
    pub source_mode: String,
    pub cloud_consistent: bool,
    pub entity_counts: BTreeMap<String, usize>,
    pub excluded_categories: Vec<String>,
    pub checksum: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupHistoryItem {
    pub id: String,
    pub path: String,
    pub file_name: String,
    pub created_at: String,
    pub backup_kind: String,
    pub source_mode: String,
    pub cloud_consistent: bool,
    pub entity_count: i64,
    pub size_bytes: i64,
    pub checksum: String,
    pub origin: String,
    pub file_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupOpenRequest {
    pub path: String,
    pub reveal: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupPreviewRequest {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupPreview {
    pub path: String,
    pub format_version: u32,
    pub app_version: String,
    pub created_at: String,
    pub frozen_at: i64,
    pub backup_kind: String,
    pub source_mode: String,
    pub cloud_consistent: bool,
    pub pending_operation_count: i64,
    pub workspace_name: String,
    pub timezone: String,
    pub entity_counts: BTreeMap<String, usize>,
    pub excluded_categories: Vec<String>,
    pub replaced_entity_counts: BTreeMap<String, usize>,
    pub legacy_sync_diagnostics: Option<LegacySyncDiagnostics>,
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct ValidatedRescueBackup {
    pub(crate) pending_operation_count: i64,
    pub(crate) legacy_queue_operation_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupImportRequest {
    pub path: String,
    pub confirmed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupImportResult {
    pub workspace_id: String,
    pub safety_backup_path: String,
    pub imported_entity_counts: BTreeMap<String, usize>,
}

#[derive(Clone, Debug)]
struct ValidatedBackup {
    manifest: BackupManifest,
    workspace: WorkspaceMetadata,
    tables: BTreeMap<String, Vec<Value>>,
}

pub fn export_backup(
    database: &crate::database::Database,
    request: BackupExportRequest,
) -> Result<BackupExportResult, String> {
    export_backup_with_policy(database, request, false)
}

fn export_backup_with_policy(
    database: &crate::database::Database,
    request: BackupExportRequest,
    safety_backup: bool,
) -> Result<BackupExportResult, String> {
    export_backup_with_policy_for_workspace(database, request, safety_backup, None, None)
}

fn export_backup_with_policy_for_workspace(
    database: &crate::database::Database,
    request: BackupExportRequest,
    safety_backup: bool,
    disposition_reason: Option<String>,
    legacy_workspace_override: Option<&str>,
) -> Result<BackupExportResult, String> {
    let target = validated_backup_target(&request.path)?;
    let frozen_at = now_millis();
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
    let source_mode: String = transaction
        .query_row(
            "SELECT COALESCE(json_extract(value_json,'$'),'local') FROM device_settings WHERE key='storage_mode'",
            [],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| "local".to_string());
    let cloud_workspace_id = transaction
        .query_row(
            "SELECT json_extract(value_json,'$') FROM device_settings WHERE key='cloud_workspace_id'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let legacy_workspace_id = legacy_workspace_override
        .filter(|value| !value.trim().is_empty())
        .or(cloud_workspace_id.as_deref());
    let legacy_rescue = legacy_workspace_override.is_some();
    let (source_workspace_epoch, pending_operation_count, legacy_sync_diagnostics) =
        if legacy_rescue {
            let workspace_id = legacy_workspace_id
                .ok_or_else(|| "RESCUE_BACKUP_NOT_AVAILABLE: 缺少旧云端工作空间标记".to_string())?;
            let source_workspace_epoch = transaction
                .query_row(
                    "SELECT workspace_epoch FROM local_sync_state WHERE workspace_id=?1",
                    [workspace_id],
                    |row| row.get(0),
                )
                .unwrap_or(1);
            let diagnostics = build_legacy_sync_diagnostics(
                &transaction,
                Some(workspace_id),
                disposition_reason.as_deref(),
            )?;
            (
                source_workspace_epoch,
                diagnostics.queue_counts.total,
                Some(diagnostics),
            )
        } else {
            (1, 0, None)
        };
    let workspace: WorkspaceMetadata = transaction
        .query_row(
            "SELECT name,timezone FROM workspaces WHERE id=?1",
            [&local_workspace_id],
            |row| {
                Ok(WorkspaceMetadata {
                    name: row.get(0)?,
                    timezone: row.get(1)?,
                })
            },
        )
        .map_err(|error| error.to_string())?;
    let mut tables = export_tables(&transaction, &local_workspace_id)?;
    freeze_active_tracking(&mut tables, frozen_at)?;
    transaction.commit().map_err(|error| error.to_string())?;
    let entity_counts = tables
        .iter()
        .map(|(table, rows)| (table.clone(), rows.len()))
        .collect::<BTreeMap<_, _>>();
    let backup_kind = if legacy_rescue {
        BACKUP_KIND_RESCUE
    } else {
        BACKUP_KIND_STANDARD
    };
    let manifest = BackupManifest {
        format_version: BACKUP_FORMAT_VERSION,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        created_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        frozen_at,
        backup_kind: backup_kind.to_string(),
        source_mode: source_mode.clone(),
        source_workspace_marker: sha256_hex(
            legacy_workspace_id
                .unwrap_or(&local_workspace_id)
                .as_bytes(),
        ),
        source_workspace_epoch,
        cloud_consistent: pending_operation_count == 0,
        pending_operation_count,
        entity_counts: entity_counts.clone(),
        excluded_categories: EXCLUDED_CATEGORIES
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        legacy_sync_diagnostics,
    };
    let archive_entries = backup_entries(&manifest, &workspace, &tables)?;
    let bytes = create_json_archive(&archive_entries)?;
    write_atomic(&target, &bytes)?;
    let validated = validate_backup_path(&target)?;
    let checksum = sha256_hex(&bytes);
    let result = BackupExportResult {
        path: target.display().to_string(),
        created_at: validated.manifest.created_at,
        backup_kind: validated.manifest.backup_kind,
        source_mode,
        cloud_consistent: validated.manifest.cloud_consistent,
        entity_counts,
        excluded_categories: validated.manifest.excluded_categories,
        checksum,
    };
    record_backup_history(
        database,
        &result,
        bytes.len() as i64,
        if safety_backup { "safety" } else { "manual" },
    )?;
    Ok(result)
}

pub(crate) fn export_automatic_safety_backup(
    database: &crate::database::Database,
    path: String,
) -> Result<BackupExportResult, String> {
    export_backup_with_policy(database, BackupExportRequest { path }, true)
}

pub(crate) fn export_legacy_sync_retirement_rescue_if_needed(
    database: &crate::database::Database,
) -> Result<Option<BackupExportResult>, String> {
    const MARKER_KEY: &str = "legacy_sync_retirement_rescue_v1";
    let connection = database.open()?;
    let outbox_exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='sync_outbox')",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if !outbox_exists {
        return Ok(None);
    }
    let legacy_workspace_id: Option<String> = connection
        .query_row(
            "SELECT workspace_id FROM sync_outbox
             WHERE state IN ('pending','sending','failed','conflict')
             GROUP BY workspace_id ORDER BY MIN(created_at),workspace_id LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some(legacy_workspace_id) = legacy_workspace_id else {
        return Ok(None);
    };
    let existing_marker: Option<String> = connection
        .query_row(
            "SELECT value_json FROM device_settings WHERE key=?1",
            [MARKER_KEY],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    drop(connection);
    if existing_marker
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .and_then(|value| value.get("path").and_then(Value::as_str).map(PathBuf::from))
        .is_some_and(|path| path.is_file())
    {
        return Ok(None);
    }

    let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
    let path = database.data_directory().join("backups").join(format!(
        "timegenie-before-legacy-sync-retirement-{timestamp}.timegenie-backup"
    ));
    let result = export_backup_with_policy_for_workspace(
        database,
        BackupExportRequest {
            path: path.display().to_string(),
        },
        true,
        Some("升级为双模式架构，旧队列仅留存本机救援备份且不再上传".to_string()),
        Some(&legacy_workspace_id),
    )?;
    database
        .open()?
        .execute(
            "INSERT INTO device_settings(key,value_json,updated_at) VALUES(?1,?2,?3)
             ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json,updated_at=excluded.updated_at",
            params![
                MARKER_KEY,
                json!({
                    "path": result.path,
                    "checksum": result.checksum,
                    "createdAt": result.created_at,
                })
                .to_string(),
                now_millis(),
            ],
        )
        .map_err(|error| format!("无法记录旧同步退役救援备份: {error}"))?;
    Ok(Some(result))
}

pub fn preview_backup(
    database: &crate::database::Database,
    request: BackupPreviewRequest,
) -> Result<BackupPreview, String> {
    let path = PathBuf::from(&request.path);
    let validated = validate_backup_path(&path)?;
    let local_workspace_id: String = database
        .open()?
        .query_row(
            "SELECT id FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let replaced_entity_counts = table_counts(&database.open()?, &local_workspace_id)?;
    Ok(BackupPreview {
        path: path.display().to_string(),
        format_version: validated.manifest.format_version,
        app_version: validated.manifest.app_version,
        created_at: validated.manifest.created_at,
        frozen_at: validated.manifest.frozen_at,
        backup_kind: validated.manifest.backup_kind,
        source_mode: validated.manifest.source_mode,
        cloud_consistent: validated.manifest.cloud_consistent,
        pending_operation_count: validated.manifest.pending_operation_count,
        workspace_name: validated.workspace.name,
        timezone: validated.workspace.timezone,
        entity_counts: validated.manifest.entity_counts,
        excluded_categories: validated.manifest.excluded_categories,
        replaced_entity_counts,
        legacy_sync_diagnostics: validated.manifest.legacy_sync_diagnostics,
    })
}

pub fn import_backup(
    database: &crate::database::Database,
    request: BackupImportRequest,
) -> Result<BackupImportResult, String> {
    let source_path = PathBuf::from(&request.path);
    let safety_path = safety_backup_path(&source_path);
    import_backup_with_safety_path(database, request, safety_path)
}

fn import_backup_with_safety_path(
    database: &crate::database::Database,
    request: BackupImportRequest,
    safety_path: PathBuf,
) -> Result<BackupImportResult, String> {
    if !request.confirmed {
        return Err("BACKUP_IMPORT_NOT_CONFIRMED: 导入必须明确确认".to_string());
    }
    let source_path = PathBuf::from(&request.path);
    let validated = validate_backup_path(&source_path)?;
    export_backup_with_policy(
        database,
        BackupExportRequest {
            path: safety_path.display().to_string(),
        },
        true,
    )?;
    let workspace_id = restore_validated_backup(database, &validated)?;
    Ok(BackupImportResult {
        workspace_id,
        safety_backup_path: safety_path.display().to_string(),
        imported_entity_counts: validated.manifest.entity_counts,
    })
}

#[tauri::command]
pub fn data_backup_export(
    database: tauri::State<'_, crate::database::Database>,
    request: BackupExportRequest,
) -> Result<BackupExportResult, String> {
    export_backup(&database, request)
}

#[tauri::command]
pub fn data_backup_preview(
    database: tauri::State<'_, crate::database::Database>,
    request: BackupPreviewRequest,
) -> Result<BackupPreview, String> {
    preview_backup(&database, request)
}

#[tauri::command]
pub fn data_backup_import(
    database: tauri::State<'_, crate::database::Database>,
    request: BackupImportRequest,
) -> Result<BackupImportResult, String> {
    import_backup(&database, request)
}

#[tauri::command]
pub fn data_backup_history_list(
    database: tauri::State<'_, crate::database::Database>,
) -> Result<Vec<BackupHistoryItem>, String> {
    backup_history_list(&database)
}

#[tauri::command]
pub fn data_backup_open(
    database: tauri::State<'_, crate::database::Database>,
    request: BackupOpenRequest,
) -> Result<(), String> {
    let path: String = database
        .open()?
        .query_row(
            "SELECT path FROM local_backup_history WHERE path=?1",
            [&request.path],
            |row| row.get(0),
        )
        .map_err(|_| "BACKUP_NOT_FOUND: 这条备份记录不存在".to_string())?;
    let path = PathBuf::from(path);
    if !path.is_file() {
        return Err("BACKUP_FILE_MISSING: 备份文件已被移动或删除".to_string());
    }
    open_backup_path(&path, request.reveal)
}

fn record_backup_history(
    database: &crate::database::Database,
    result: &BackupExportResult,
    size_bytes: i64,
    origin: &str,
) -> Result<(), String> {
    let path = PathBuf::from(&result.path);
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("TimeGenie 备份");
    let entity_count = result.entity_counts.values().sum::<usize>() as i64;
    database
        .open()?
        .execute(
            "INSERT INTO local_backup_history(
                 id,path,file_name,created_at,recorded_at,backup_kind,source_mode,
                 cloud_consistent,entity_count,size_bytes,checksum,origin
             ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
             ON CONFLICT(path) DO UPDATE SET
                 id=excluded.id,file_name=excluded.file_name,created_at=excluded.created_at,
                 recorded_at=excluded.recorded_at,backup_kind=excluded.backup_kind,
                 source_mode=excluded.source_mode,cloud_consistent=excluded.cloud_consistent,
                 entity_count=excluded.entity_count,size_bytes=excluded.size_bytes,
                 checksum=excluded.checksum,origin=excluded.origin",
            params![
                Uuid::now_v7().to_string(),
                result.path,
                file_name,
                result.created_at,
                now_millis(),
                result.backup_kind,
                result.source_mode,
                result.cloud_consistent,
                entity_count,
                size_bytes,
                result.checksum,
                origin,
            ],
        )
        .map_err(|error| format!("无法记录本地备份历史: {error}"))?;
    Ok(())
}

fn backup_history_list(
    database: &crate::database::Database,
) -> Result<Vec<BackupHistoryItem>, String> {
    let connection = database.open()?;
    let mut statement = connection
        .prepare(
            "SELECT id,path,file_name,created_at,backup_kind,source_mode,cloud_consistent,
                    entity_count,size_bytes,checksum,origin
             FROM local_backup_history ORDER BY recorded_at DESC,id DESC LIMIT 100",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            let path: String = row.get(1)?;
            Ok(BackupHistoryItem {
                id: row.get(0)?,
                file_exists: Path::new(&path).is_file(),
                path,
                file_name: row.get(2)?,
                created_at: row.get(3)?,
                backup_kind: row.get(4)?,
                source_mode: row.get(5)?,
                cloud_consistent: row.get(6)?,
                entity_count: row.get(7)?,
                size_bytes: row.get(8)?,
                checksum: row.get(9)?,
                origin: row.get(10)?,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "windows")]
fn open_backup_path(path: &Path, reveal: bool) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    if reveal {
        std::process::Command::new("explorer.exe")
            .arg(format!("/select,{}", path.display()))
            .spawn()
            .map_err(|error| format!("无法打开备份所在位置: {error}"))?;
        return Ok(());
    }
    let verb = std::ffi::OsStr::new("open")
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let file = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if result as isize <= 32 {
        return Err("BACKUP_OPEN_FAILED: 系统没有可打开此备份文件的程序".to_string());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn open_backup_path(path: &Path, reveal: bool) -> Result<(), String> {
    let mut command = std::process::Command::new("open");
    if reveal {
        command.arg("-R");
    }
    command
        .arg(path)
        .spawn()
        .map_err(|error| format!("无法打开备份文件: {error}"))?;
    Ok(())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn open_backup_path(path: &Path, reveal: bool) -> Result<(), String> {
    let target = if reveal {
        path.parent().unwrap_or(path)
    } else {
        path
    };
    std::process::Command::new("xdg-open")
        .arg(target)
        .spawn()
        .map_err(|error| format!("无法打开备份文件: {error}"))?;
    Ok(())
}

fn validated_backup_target(path: &str) -> Result<PathBuf, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("VALIDATION_ERROR: 请选择备份保存路径".to_string());
    }
    let mut target = PathBuf::from(trimmed);
    if target.extension().and_then(|value| value.to_str()) != Some("timegenie-backup") {
        target.set_extension("timegenie-backup");
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("无法创建备份目录: {error}"))?;
    }
    Ok(target)
}

fn safety_backup_path(source_path: &Path) -> PathBuf {
    let directory = source_path.parent().unwrap_or_else(|| Path::new("."));
    let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
    directory.join(format!(
        "timegenie-before-import-{timestamp}.timegenie-backup"
    ))
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("timegenie-backup"),
        Uuid::now_v7()
    ));
    fs::write(&temporary, bytes).map_err(|error| format!("无法写入临时备份: {error}"))?;
    if path.exists() {
        fs::remove_file(path).map_err(|error| format!("无法覆盖备份文件: {error}"))?;
    }
    fs::rename(&temporary, path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        format!("无法完成备份文件写入: {error}")
    })
}

fn backup_entries(
    manifest: &BackupManifest,
    workspace: &WorkspaceMetadata,
    tables: &BTreeMap<String, Vec<Value>>,
) -> Result<BTreeMap<String, Value>, String> {
    let mut entries = BTreeMap::new();
    entries.insert("manifest.json".to_string(), to_json_value(manifest)?);
    entries.insert("workspace.json".to_string(), to_json_value(workspace)?);
    for group in [
        ProtectionGroup::Planning,
        ProtectionGroup::TimeTracking,
        ProtectionGroup::Reports,
        ProtectionGroup::SharedSettings,
    ] {
        let group_tables = PROTECTED_ENTITIES
            .iter()
            .filter(|entity| entity.group == group)
            .map(|entity| {
                (
                    entity.table.to_string(),
                    Value::Array(tables.get(entity.table).cloned().unwrap_or_default()),
                )
            })
            .collect::<Map<_, _>>();
        entries.insert(
            group.archive_member().to_string(),
            Value::Object(group_tables),
        );
    }
    if let Some(diagnostics) = manifest.legacy_sync_diagnostics.as_ref() {
        entries.insert(
            LEGACY_SYNC_ARCHIVE_MEMBER.to_string(),
            to_json_value(diagnostics)?,
        );
    }
    let checksums = checksums_for(&entries)?;
    entries.insert("checksums.json".to_string(), to_json_value(checksums)?);
    Ok(entries)
}

fn validate_backup_path(path: &Path) -> Result<ValidatedBackup, String> {
    let metadata = fs::metadata(path).map_err(|error| format!("无法读取备份文件: {error}"))?;
    if metadata.len() > MAX_ARCHIVE_TOTAL_BYTES {
        return Err("BACKUP_TOO_LARGE: 备份文件超出大小限制".to_string());
    }
    let file = fs::File::open(path).map_err(|error| format!("无法打开备份文件: {error}"))?;
    let entries = read_json_archive(file)?;
    let names = entries.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let required = ARCHIVE_MEMBERS.into_iter().collect::<BTreeSet<_>>();
    let mut allowed = required.clone();
    allowed.insert(LEGACY_SYNC_ARCHIVE_MEMBER);
    if !required.is_subset(&names) || !names.is_subset(&allowed) {
        return Err(format!(
            "INVALID_BACKUP: 备份成员不完整，期望 {required:?}，实际 {names:?}"
        ));
    }
    let checksums: BTreeMap<String, String> =
        serde_json::from_value(entries["checksums.json"].clone())
            .map_err(|error| format!("INVALID_BACKUP: 校验清单无效: {error}"))?;
    let content = entries
        .iter()
        .filter(|(name, _)| name.as_str() != "checksums.json")
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect::<BTreeMap<_, _>>();
    let actual = checksums_for(&content)?;
    if checksums != actual {
        return Err("INVALID_BACKUP: 文件校验和不匹配".to_string());
    }
    let manifest: BackupManifest = serde_json::from_value(entries["manifest.json"].clone())
        .map_err(|error| format!("INVALID_BACKUP: manifest 无效: {error}"))?;
    if manifest.format_version != BACKUP_FORMAT_VERSION {
        return Err(format!(
            "UNSUPPORTED_BACKUP_VERSION: 当前支持版本 {BACKUP_FORMAT_VERSION}，备份版本 {}",
            manifest.format_version
        ));
    }
    if !matches!(
        manifest.backup_kind.as_str(),
        BACKUP_KIND_STANDARD | BACKUP_KIND_RESCUE
    ) {
        return Err("INVALID_BACKUP: 不支持的备份种类".to_string());
    }
    let has_legacy_diagnostics = entries.contains_key(LEGACY_SYNC_ARCHIVE_MEMBER);
    if manifest.legacy_sync_diagnostics.is_some() != has_legacy_diagnostics {
        return Err("INVALID_BACKUP: 旧同步诊断清单与备份成员不一致".to_string());
    }
    if manifest.backup_kind == BACKUP_KIND_STANDARD && has_legacy_diagnostics {
        return Err("INVALID_BACKUP: 标准备份不能包含旧同步诊断".to_string());
    }
    if let Some(diagnostics) = manifest.legacy_sync_diagnostics.as_ref() {
        validate_legacy_sync_diagnostics(diagnostics, manifest.pending_operation_count)?;
    }
    let workspace: WorkspaceMetadata = serde_json::from_value(entries["workspace.json"].clone())
        .map_err(|error| format!("INVALID_BACKUP: workspace 无效: {error}"))?;
    if workspace.name.trim().is_empty() || chrono_tz::Tz::from_str(&workspace.timezone).is_err() {
        return Err("INVALID_BACKUP: 工作空间名称或时区无效".to_string());
    }
    let mut tables = BTreeMap::new();
    for group in [
        ProtectionGroup::Planning,
        ProtectionGroup::TimeTracking,
        ProtectionGroup::Reports,
        ProtectionGroup::SharedSettings,
    ] {
        let object = entries[group.archive_member()]
            .as_object()
            .ok_or_else(|| format!("INVALID_BACKUP: {} 必须是对象", group.archive_member()))?;
        let expected = PROTECTED_ENTITIES
            .iter()
            .filter(|entity| entity.group == group)
            .map(|entity| entity.table)
            .collect::<BTreeSet<_>>();
        let actual_names = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
        if actual_names != expected {
            return Err(format!(
                "INVALID_BACKUP: {} 实体清单不匹配",
                group.archive_member()
            ));
        }
        for (table, value) in object {
            let rows = value
                .as_array()
                .ok_or_else(|| format!("INVALID_BACKUP: {table} 必须是数组"))?;
            tables.insert(table.clone(), rows.clone());
        }
    }
    let actual_counts = tables
        .iter()
        .map(|(table, rows)| (table.clone(), rows.len()))
        .collect::<BTreeMap<_, _>>();
    if actual_counts != manifest.entity_counts {
        return Err("INVALID_BACKUP: 实体数量与清单不一致".to_string());
    }
    validate_references(&tables)?;
    Ok(ValidatedBackup {
        manifest,
        workspace,
        tables,
    })
}

fn export_tables(
    connection: &Connection,
    workspace_id: &str,
) -> Result<BTreeMap<String, Vec<Value>>, String> {
    let mut tables = BTreeMap::new();
    for entity in PROTECTED_ENTITIES {
        let select = if entity.table == "integration_configs" {
            "workspace_id,provider,enabled,config_json,updated_at,version".to_string()
        } else if entity.table == "unassigned_segments" {
            "id,workspace_id,session_id,sequence_no,started_at,ended_at,duration_seconds,NULL AS lease_token".to_string()
        } else {
            "*".to_string()
        };
        let sql = format!(
            "SELECT {select} FROM {} WHERE workspace_id=?1 ORDER BY {}",
            entity.table, entity.stable_order
        );
        let mut statement = connection
            .prepare(&sql)
            .map_err(|error| error.to_string())?;
        let column_names = statement
            .column_names()
            .iter()
            .map(|value| (*value).to_string())
            .collect::<Vec<_>>();
        let rows = statement
            .query_map([workspace_id], |row| {
                let mut object = Map::new();
                for (index, name) in column_names.iter().enumerate() {
                    object.insert(name.clone(), sqlite_value_to_json(row.get(index)?));
                }
                for field in ["value_json", "config_json"] {
                    parse_and_redact_json_field(&mut object, field);
                }
                for field in EXCLUDED_FIELDS {
                    object.remove(*field);
                }
                Ok(Value::Object(object))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        tables.insert(entity.table.to_string(), rows);
    }
    Ok(tables)
}

fn table_counts(
    connection: &Connection,
    workspace_id: &str,
) -> Result<BTreeMap<String, usize>, String> {
    PROTECTED_ENTITIES
        .iter()
        .map(|entity| {
            connection
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM {} WHERE workspace_id=?1",
                        entity.table
                    ),
                    [workspace_id],
                    |row| row.get::<_, i64>(0),
                )
                .map(|count| (entity.table.to_string(), count.max(0) as usize))
                .map_err(|error| error.to_string())
        })
        .collect()
}

fn freeze_active_tracking(
    tables: &mut BTreeMap<String, Vec<Value>>,
    frozen_at: i64,
) -> Result<(), String> {
    if let Some(segments) = tables.get_mut("time_segments") {
        for row in segments {
            let Some(object) = row.as_object_mut() else {
                continue;
            };
            if object.get("ended_at").is_some_and(Value::is_null) {
                let started_at = object
                    .get("started_at")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| "INVALID_BACKUP: 时间段缺少 started_at".to_string())?;
                let ended_at = frozen_at.max(started_at);
                object.insert("ended_at".to_string(), json!(ended_at));
                object.insert(
                    "duration_seconds".to_string(),
                    json!((ended_at - started_at) / 1000),
                );
            }
        }
    }
    let durations = tables
        .get("time_segments")
        .into_iter()
        .flatten()
        .filter_map(|row| {
            Some((
                row.get("entry_id")?.as_str()?.to_string(),
                row.get("duration_seconds")?.as_i64()?,
            ))
        })
        .fold(
            HashMap::<String, i64>::new(),
            |mut totals, (entry_id, duration)| {
                *totals.entry(entry_id).or_default() += duration;
                totals
            },
        );
    if let Some(entries) = tables.get_mut("time_entries") {
        for row in entries {
            let Some(object) = row.as_object_mut() else {
                continue;
            };
            if object.get("state").and_then(Value::as_str) == Some("running") {
                let id = object
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                object.insert("state".to_string(), json!("paused"));
                object.insert("ended_at".to_string(), Value::Null);
                object.insert(
                    "duration_seconds".to_string(),
                    json!(durations.get(&id).copied().unwrap_or_default()),
                );
                object.insert("last_continuous_at".to_string(), json!(frozen_at));
                object.insert("updated_at".to_string(), json!(frozen_at));
            }
        }
    }
    if let Some(sessions) = tables.get_mut("unassigned_sessions") {
        for row in sessions {
            let Some(object) = row.as_object_mut() else {
                continue;
            };
            if matches!(
                object.get("state").and_then(Value::as_str),
                Some("collecting" | "awaiting_resolution")
            ) {
                object.insert("state".to_string(), json!("discarded"));
                object.insert("resolution_type".to_string(), json!("discard"));
                object.insert("resolved_at".to_string(), json!(frozen_at));
                object.insert("updated_at".to_string(), json!(frozen_at));
            }
        }
    }
    if let Some(segments) = tables.get_mut("unassigned_segments") {
        for row in segments {
            let Some(object) = row.as_object_mut() else {
                continue;
            };
            if object.get("ended_at").is_some_and(Value::is_null) {
                let started_at = object
                    .get("started_at")
                    .and_then(Value::as_i64)
                    .unwrap_or(frozen_at);
                let ended_at = frozen_at.max(started_at);
                object.insert("ended_at".to_string(), json!(ended_at));
                object.insert(
                    "duration_seconds".to_string(),
                    json!((ended_at - started_at) / 1000),
                );
            }
        }
    }
    Ok(())
}

fn build_legacy_sync_diagnostics(
    connection: &Connection,
    cloud_workspace_id: Option<&str>,
    disposition_reason: Option<&str>,
) -> Result<LegacySyncDiagnostics, String> {
    let workspace_id = cloud_workspace_id
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "RESCUE_BACKUP_NOT_AVAILABLE: 缺少云端工作空间标记".to_string())?;
    let queue_counts = connection
        .query_row(
            "SELECT
               COALESCE(SUM(CASE WHEN state='pending' THEN 1 ELSE 0 END),0),
               COALESCE(SUM(CASE WHEN state='sending' THEN 1 ELSE 0 END),0),
               COALESCE(SUM(CASE WHEN state='failed' THEN 1 ELSE 0 END),0),
               COALESCE(SUM(CASE WHEN state='conflict' THEN 1 ELSE 0 END),0),
               COUNT(*)
             FROM sync_outbox
             WHERE workspace_id=?1 AND state IN ('pending','sending','failed','conflict')",
            [workspace_id],
            |row| {
                Ok(LegacySyncQueueCounts {
                    pending: row.get(0)?,
                    sending: row.get(1)?,
                    failed: row.get(2)?,
                    conflict: row.get(3)?,
                    total: row.get(4)?,
                })
            },
        )
        .map_err(|error| error.to_string())?;
    const DIAGNOSTIC_LIMIT: usize = 5_000;
    let mut statement = connection
        .prepare(
            "SELECT operation_type,entity_type,state,attempt_count,base_version,
                    payload_version,workspace_epoch,created_at,last_attempt_at,error_json
             FROM sync_outbox
             WHERE workspace_id=?1 AND state IN ('pending','sending','failed','conflict')
             ORDER BY created_at,operation_type,entity_type
             LIMIT ?2",
        )
        .map_err(|error| error.to_string())?;
    let operations = statement
        .query_map(
            params![workspace_id, (DIAGNOSTIC_LIMIT + 1) as i64],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, Option<i64>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                ))
            },
        )
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let operations_truncated = operations.len() > DIAGNOSTIC_LIMIT;
    let operations = operations
        .into_iter()
        .take(DIAGNOSTIC_LIMIT)
        .enumerate()
        .map(
            |(
                index,
                (
                    operation_type,
                    entity_type,
                    state,
                    attempt_count,
                    base_version,
                    payload_version,
                    workspace_epoch,
                    created_at,
                    last_attempt_at,
                    error_json,
                ),
            )| {
                LegacySyncOperationDiagnostic {
                    sequence: index as i64 + 1,
                    operation_type,
                    entity_type,
                    state,
                    attempt_count,
                    base_version,
                    payload_version,
                    workspace_epoch,
                    error_code: diagnostic_error_code(error_json.as_deref()),
                    created_at,
                    last_attempt_at,
                }
            },
        )
        .collect::<Vec<_>>();
    let reason = disposition_reason
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("升级到直连云端存储前导出旧同步队列并明确放弃")
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .take(500)
        .collect::<String>();
    Ok(LegacySyncDiagnostics {
        disposition_reason: reason,
        queue_counts,
        operations,
        operations_truncated,
        business_data_included: true,
        raw_payload_excluded: true,
        note: "包含旧云端本地业务快照与队列诊断；不包含操作 payload、设备标识、访问令牌或密码"
            .to_string(),
    })
}

fn diagnostic_error_code(raw: Option<&str>) -> Option<String> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    let message = serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|value| {
            value
                .get("message")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .or_else(|| value.as_str().map(ToOwned::to_owned))
        })
        .unwrap_or_else(|| raw.to_string());
    let candidate = message
        .split_once(':')
        .map(|(code, _)| code.trim())
        .unwrap_or("");
    const SAFE_CODES: &[&str] = &[
        "AUTH_REQUIRED",
        "CLOUD_PROTOCOL_UNSUPPORTED",
        "DEVICE_REVOKED",
        "ENTITY_DELETED",
        "IDEMPOTENCY_CONFLICT",
        "LEASE_LOST",
        "NETWORK_ERROR",
        "OFFLINE_RESTRICTED",
        "VALIDATION_ERROR",
        "VERSION_CONFLICT",
        "WORKSPACE_EPOCH_STALE",
    ];
    Some(if SAFE_CODES.contains(&candidate) {
        candidate.to_string()
    } else {
        "REMOTE_ERROR".to_string()
    })
}

fn validate_legacy_sync_diagnostics(
    diagnostics: &LegacySyncDiagnostics,
    pending_operation_count: i64,
) -> Result<(), String> {
    if diagnostics.queue_counts.total
        != diagnostics.queue_counts.pending
            + diagnostics.queue_counts.sending
            + diagnostics.queue_counts.failed
            + diagnostics.queue_counts.conflict
    {
        return Err("INVALID_BACKUP: 旧同步队列汇总不一致".to_string());
    }
    if diagnostics.queue_counts.total != pending_operation_count {
        return Err("INVALID_BACKUP: 旧同步队列数量与清单不一致".to_string());
    }
    if !diagnostics.business_data_included || !diagnostics.raw_payload_excluded {
        return Err("INVALID_BACKUP: 旧同步诊断范围标记无效".to_string());
    }
    if diagnostics.disposition_reason.trim().is_empty() {
        return Err("INVALID_BACKUP: 旧同步处置原因不能为空".to_string());
    }
    if !diagnostics.operations_truncated
        && diagnostics.operations.len() as i64 != diagnostics.queue_counts.total
    {
        return Err("INVALID_BACKUP: 旧同步操作明细数量不一致".to_string());
    }
    let mut states = BTreeMap::<&str, i64>::new();
    for (index, operation) in diagnostics.operations.iter().enumerate() {
        if operation.sequence != index as i64 + 1 {
            return Err("INVALID_BACKUP: 旧同步操作序号不连续".to_string());
        }
        if !matches!(
            operation.state.as_str(),
            "pending" | "sending" | "failed" | "conflict"
        ) {
            return Err("INVALID_BACKUP: 旧同步操作状态无效".to_string());
        }
        *states.entry(operation.state.as_str()).or_default() += 1;
        if operation.attempt_count < 0 {
            return Err("INVALID_BACKUP: 旧同步重试次数无效".to_string());
        }
        if operation.error_code.as_deref().is_some_and(|code| {
            code.is_empty()
                || !code
                    .chars()
                    .all(|character| character.is_ascii_uppercase() || character == '_')
        }) {
            return Err("INVALID_BACKUP: 旧同步错误码无效".to_string());
        }
    }
    if !diagnostics.operations_truncated
        && (states.get("pending").copied().unwrap_or(0) != diagnostics.queue_counts.pending
            || states.get("sending").copied().unwrap_or(0) != diagnostics.queue_counts.sending
            || states.get("failed").copied().unwrap_or(0) != diagnostics.queue_counts.failed
            || states.get("conflict").copied().unwrap_or(0) != diagnostics.queue_counts.conflict)
    {
        return Err("INVALID_BACKUP: 旧同步状态计数不一致".to_string());
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn validate_rescue_backup_for_abandon(
    path: &Path,
) -> Result<ValidatedRescueBackup, String> {
    let validated = validate_backup_path(path)?;
    if validated.manifest.backup_kind != BACKUP_KIND_RESCUE {
        return Err("RESCUE_BACKUP_REQUIRED: 必须使用带旧同步诊断的救援备份".to_string());
    }
    let diagnostics = validated
        .manifest
        .legacy_sync_diagnostics
        .as_ref()
        .ok_or_else(|| "RESCUE_BACKUP_INCOMPLETE: 救援备份缺少旧同步诊断".to_string())?;
    Ok(ValidatedRescueBackup {
        pending_operation_count: validated.manifest.pending_operation_count,
        legacy_queue_operation_count: diagnostics.operations.len(),
    })
}

fn restore_validated_backup(
    database: &crate::database::Database,
    backup: &ValidatedBackup,
) -> Result<String, String> {
    let mut connection = database.open()?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let old_workspace_id: String = transaction
        .query_row(
            "SELECT id FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let new_workspace_id = Uuid::now_v7().to_string();
    let new_device_id = Uuid::now_v7().to_string();
    let now = now_millis();
    delete_workspace_data(&transaction, &old_workspace_id)?;
    transaction.execute(
        "INSERT INTO workspaces(id,owner_user_id,name,timezone,storage_mode,created_at,updated_at,version) VALUES(?1,NULL,?2,?3,'local',?4,?4,1)",
        params![new_workspace_id, backup.workspace.name, backup.workspace.timezone, now],
    ).map_err(|error| error.to_string())?;
    transaction.execute(
        "INSERT INTO devices(id,workspace_id,device_name,platform,app_version,last_seen_at,created_at) VALUES(?1,?2,'当前设备',?3,?4,?5,?5)",
        params![new_device_id,new_workspace_id,std::env::consts::OS,env!("CARGO_PKG_VERSION"),now],
    ).map_err(|error| error.to_string())?;
    for entity in PROTECTED_ENTITIES {
        let rows = backup
            .tables
            .get(entity.table)
            .ok_or_else(|| format!("INVALID_BACKUP: 缺少 {}", entity.table))?;
        insert_table_rows(&transaction, entity.table, rows, &new_workspace_id)?;
    }
    restore_deferred_references(&transaction, &backup.tables)?;
    for (key, value) in [
        ("storage_mode", json!("local")),
        ("device_id", json!(new_device_id)),
        ("device_name", json!("当前设备")),
    ] {
        transaction.execute(
            "INSERT INTO device_settings(key,value_json,updated_at) VALUES(?1,?2,?3) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json,updated_at=excluded.updated_at",
            params![key,value.to_string(),now],
        ).map_err(|error| error.to_string())?;
    }
    transaction.execute(
        "DELETE FROM device_settings WHERE key IN ('cloud_workspace_id','supabase_project_url','supabase_anon_key','supabase_session_metadata')",
        [],
    ).map_err(|error| error.to_string())?;
    transaction
        .execute(
            "UPDATE app_metadata SET value=CAST(value AS INTEGER)+1 WHERE key='global_revision'",
            [],
        )
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(new_workspace_id)
}

pub(crate) fn replace_local_business_data_from_copy(
    database: &crate::database::Database,
    snapshot: &Value,
) -> Result<String, String> {
    let expected_entities = data_package_copy_entity_names()?;
    let mut tables = BTreeMap::new();
    for entity_name in expected_entities {
        let rows = snapshot
            .get(&entity_name)
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| format!("DATA_PACKAGE_ENTITY_MISSING: {entity_name}"))?;
        tables.insert(entity_name, rows);
    }
    validate_references(&tables)?;

    let mut connection = database.open()?;
    let transaction = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let workspace_id: String = transaction
        .query_row(
            "SELECT id FROM workspaces WHERE storage_mode='local' AND deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let preserved_path_settings = preserved_local_path_settings(&transaction, &workspace_id)?;
    let preserved_secret_refs = preserved_integration_secret_refs(&transaction, &workspace_id)?;
    clear_local_copy_target(&transaction, &workspace_id)?;
    for entity in PROTECTED_ENTITIES
        .iter()
        .filter(|entity| tables.contains_key(entity.table))
    {
        let rows = tables
            .get(entity.table)
            .ok_or_else(|| format!("DATA_PACKAGE_ENTITY_MISSING: {}", entity.table))?;
        insert_table_rows(&transaction, entity.table, rows, &workspace_id)?;
    }
    restore_deferred_references(&transaction, &tables)?;
    restore_preserved_local_metadata(
        &transaction,
        &workspace_id,
        &preserved_path_settings,
        &preserved_secret_refs,
    )?;
    transaction
        .execute(
            "UPDATE app_metadata SET value=CAST(CAST(value AS INTEGER)+1 AS TEXT) WHERE key='global_revision'",
            [],
        )
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(workspace_id)
}

fn preserved_local_path_settings(
    transaction: &Transaction<'_>,
    workspace_id: &str,
) -> Result<Vec<(String, String, i64, i64)>, String> {
    let mut statement = transaction
        .prepare("SELECT key,value_json,updated_at,version FROM app_settings WHERE workspace_id=?1")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([workspace_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .filter_map(|row| match row {
            Ok((key, value, updated_at, version)) if is_local_path_setting_key(&key) => {
                Some(Ok((key, value, updated_at, version)))
            }
            Ok(_) => None,
            Err(error) => Some(Err(error.to_string())),
        })
        .collect();
    rows
}

fn preserved_integration_secret_refs(
    transaction: &Transaction<'_>,
    workspace_id: &str,
) -> Result<HashMap<String, String>, String> {
    let mut statement = transaction
        .prepare(
            "SELECT provider,secret_ref FROM integration_configs WHERE workspace_id=?1 AND secret_ref IS NOT NULL AND secret_ref<>''",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([workspace_id], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|error| error.to_string())?
        .collect::<Result<HashMap<_, _>, _>>()
        .map_err(|error| error.to_string());
    rows
}

fn restore_preserved_local_metadata(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    path_settings: &[(String, String, i64, i64)],
    secret_refs: &HashMap<String, String>,
) -> Result<(), String> {
    for (key, value_json, updated_at, version) in path_settings {
        transaction
            .execute(
                "INSERT INTO app_settings(workspace_id,key,value_json,updated_at,version) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(workspace_id,key) DO NOTHING",
                params![workspace_id,key,value_json,updated_at,version],
            )
            .map_err(|error| error.to_string())?;
    }
    for (provider, secret_ref) in secret_refs {
        transaction
            .execute(
                "UPDATE integration_configs SET secret_ref=?1 WHERE workspace_id=?2 AND provider=?3",
                params![secret_ref,workspace_id,provider],
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn is_local_path_setting_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace(['-', '_', ' '], "");
    [
        "localpath",
        "directory",
        "filepath",
        "sourcepath",
        "outputpath",
        "vaultpath",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}

fn clear_local_copy_target(
    transaction: &Transaction<'_>,
    workspace_id: &str,
) -> Result<(), String> {
    transaction
        .execute(
            "DELETE FROM local_report_outputs WHERE report_id IN (SELECT id FROM reports WHERE workspace_id=?1)",
            [workspace_id],
        )
        .map_err(|error| error.to_string())?;
    for (table, field) in [
        ("time_entries", "origin_unassigned_session_id"),
        ("time_entries", "previous_entry_id"),
        ("unassigned_sessions", "generated_entry_id"),
        ("tasks", "parent_id"),
    ] {
        transaction
            .execute(
                &format!("UPDATE {table} SET {field}=NULL WHERE workspace_id=?1"),
                [workspace_id],
            )
            .map_err(|error| error.to_string())?;
    }
    for table in [
        "daily_report_import_items",
        "daily_report_import_sources",
        "daily_report_import_batches",
        "plan_import_items",
        "plan_import_batches",
        "actual_adjustment_batches",
        "tracking_runtime_state",
        "external_bindings",
        "integration_configs",
        "subject_work_schedule_segments",
        "subject_work_schedules",
        "report_tasks",
        "task_occurrences",
        "task_recurrence_rules",
        "task_day_plans",
        "task_daily_estimates",
        "time_allocations",
        "actual_time_adjustments",
        "time_segments",
        "unassigned_segments",
        "time_entries",
        "unassigned_sessions",
        "task_status_events",
        "reports",
        "report_templates",
        "tasks",
        "work_days",
        "app_settings",
        "subjects",
    ] {
        transaction
            .execute(
                &format!("DELETE FROM {table} WHERE workspace_id=?1"),
                [workspace_id],
            )
            .map_err(|error| {
                format!("LOCAL_DATA_COPY_REPLACE_FAILED: 清理 {table} 失败: {error}")
            })?;
    }
    Ok(())
}

fn delete_workspace_data(transaction: &Transaction<'_>, workspace_id: &str) -> Result<(), String> {
    transaction
        .execute(
            "DELETE FROM local_report_outputs WHERE report_id IN (SELECT id FROM reports WHERE workspace_id=?1)",
            [workspace_id],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "UPDATE time_entries SET origin_unassigned_session_id=NULL,previous_entry_id=NULL WHERE workspace_id=?1",
            [workspace_id],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "UPDATE unassigned_sessions SET generated_entry_id=NULL WHERE workspace_id=?1",
            [workspace_id],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "UPDATE tasks SET parent_id=NULL WHERE workspace_id=?1",
            [workspace_id],
        )
        .map_err(|error| error.to_string())?;
    for table in [
        "integration_run_items",
        "plan_import_items",
        "daily_report_import_items",
        "daily_report_import_sources",
        "subject_work_schedule_segments",
        "report_tasks",
        "time_allocations",
        "time_segments",
        "unassigned_segments",
        "actual_time_adjustments",
        "actual_adjustment_batches",
        "task_status_events",
        "task_day_plans",
        "task_daily_estimates",
        "task_recurrence_rules",
        "task_occurrences",
        "integration_runs",
        "plan_import_batches",
        "daily_report_import_batches",
        "reports",
        "report_templates",
        "unassigned_sessions",
        "time_entries",
        "tasks",
        "subject_work_schedules",
        "work_days",
        "app_settings",
        "integration_configs",
        "external_bindings",
        "local_request_results",
        "tracking_runtime_state",
        "subjects",
        "devices",
    ] {
        transaction
            .execute(
                &format!("DELETE FROM {table} WHERE workspace_id=?1"),
                [workspace_id],
            )
            .map_err(|error| format!("清理 {table} 失败: {error}"))?;
    }
    transaction
        .execute("DELETE FROM workspaces WHERE id=?1", [workspace_id])
        .map_err(|error| format!("清理旧工作空间失败: {error}"))?;
    Ok(())
}

fn insert_table_rows(
    transaction: &Transaction<'_>,
    table: &str,
    rows: &[Value],
    workspace_id: &str,
) -> Result<(), String> {
    let columns = table_columns(transaction, table)?;
    for row in rows {
        let object = row
            .as_object()
            .ok_or_else(|| format!("INVALID_BACKUP: {table} 行不是对象"))?;
        let mut insert_columns = Vec::new();
        let mut values = Vec::new();
        for column in &columns {
            let value = if column == "workspace_id" {
                Some(json!(workspace_id))
            } else if matches!(
                (table, column.as_str()),
                ("tasks", "parent_id")
                    | ("tasks", "day_instance_source_task_id")
                    | ("time_entries", "origin_unassigned_session_id")
                    | ("time_entries", "previous_entry_id")
                    | ("unassigned_sessions", "predecessor_session_id")
            ) {
                Some(Value::Null)
            } else {
                object.get(column).cloned()
            };
            if let Some(value) = value {
                insert_columns.push(column.clone());
                values.push(json_to_sql_value(table, column, &value)?);
            }
        }
        let placeholders = (1..=insert_columns.len())
            .map(|index| format!("?{index}"))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "INSERT INTO {table}({}) VALUES({placeholders})",
            insert_columns.join(",")
        );
        transaction
            .execute(&sql, params_from_iter(values))
            .map_err(|error| format!("导入 {table} 失败: {error}"))?;
    }
    Ok(())
}

fn restore_deferred_references(
    transaction: &Transaction<'_>,
    tables: &BTreeMap<String, Vec<Value>>,
) -> Result<(), String> {
    let task_ids = backup_row_ids(tables, "tasks");
    let entry_ids = backup_row_ids(tables, "time_entries");
    let session_ids = backup_row_ids(tables, "unassigned_sessions");
    for (table, field, targets) in [
        ("tasks", "parent_id", &task_ids),
        ("tasks", "day_instance_source_task_id", &task_ids),
        ("time_entries", "previous_entry_id", &entry_ids),
        // 来源会话可能已被清理：只有备份里仍然存在该会话时才回填，否则保持 NULL。
        ("time_entries", "origin_unassigned_session_id", &session_ids),
        (
            "unassigned_sessions",
            "predecessor_session_id",
            &session_ids,
        ),
    ] {
        for row in tables.get(table).into_iter().flatten() {
            let Some(value) = row.get(field).and_then(Value::as_str) else {
                continue;
            };
            if !targets.contains(value) {
                continue;
            }
            let id = row
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("INVALID_BACKUP: {table} 缺少 id"))?;
            transaction
                .execute(
                    &format!("UPDATE {table} SET {field}=?1 WHERE id=?2"),
                    params![value, id],
                )
                .map_err(|error| format!("恢复 {table}.{field} 失败: {error}"))?;
        }
    }
    Ok(())
}

fn backup_row_ids<'a>(tables: &'a BTreeMap<String, Vec<Value>>, table: &str) -> HashSet<&'a str> {
    tables
        .get(table)
        .into_iter()
        .flatten()
        .filter_map(|row| row.get("id").and_then(Value::as_str))
        .collect()
}

fn table_columns(transaction: &Transaction<'_>, table: &str) -> Result<Vec<String>, String> {
    transaction
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|error| error.to_string())?
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

fn json_to_sql_value(table: &str, column: &str, value: &Value) -> Result<SqlValue, String> {
    if value.is_null() {
        return Ok(SqlValue::Null);
    }
    if matches!(
        (table, column),
        ("app_settings", "value_json") | ("integration_configs", "config_json")
    ) {
        return canonical_json_bytes(value)
            .and_then(|bytes| String::from_utf8(bytes).map_err(|error| error.to_string()))
            .map(SqlValue::Text);
    }
    match value {
        Value::Bool(value) => Ok(SqlValue::Integer(i64::from(*value))),
        Value::Number(value) => value
            .as_i64()
            .map(SqlValue::Integer)
            .or_else(|| value.as_f64().map(SqlValue::Real))
            .ok_or_else(|| "INVALID_BACKUP: 数字超出 SQLite 范围".to_string()),
        Value::String(value) if column.ends_with("_at") => {
            Ok(chrono::DateTime::parse_from_rfc3339(value)
                .map(|timestamp| SqlValue::Integer(timestamp.timestamp_millis()))
                .unwrap_or_else(|_| SqlValue::Text(value.clone())))
        }
        Value::String(value) => Ok(SqlValue::Text(value.clone())),
        Value::Array(_) | Value::Object(_) => canonical_json_bytes(value)
            .and_then(|bytes| String::from_utf8(bytes).map_err(|error| error.to_string()))
            .map(SqlValue::Text),
        Value::Null => Ok(SqlValue::Null),
    }
}

fn validate_references(tables: &BTreeMap<String, Vec<Value>>) -> Result<(), String> {
    let ids = |table: &str, field: &str| -> HashSet<String> {
        tables
            .get(table)
            .into_iter()
            .flatten()
            .filter_map(|row| row.get(field)?.as_str().map(str::to_string))
            .collect()
    };
    let subjects = ids("subjects", "id");
    let tasks = ids("tasks", "id");
    let entries = ids("time_entries", "id");
    let sessions = ids("unassigned_sessions", "id");
    let schedules = ids("subject_work_schedules", "id");
    let templates = ids("report_templates", "id");
    let reports = ids("reports", "id");
    let mut workspace_ids = HashSet::new();
    for entity in PROTECTED_ENTITIES {
        let mut row_ids = HashSet::new();
        for row in tables.get(entity.table).into_iter().flatten() {
            let object = row
                .as_object()
                .ok_or_else(|| format!("INVALID_BACKUP: {} 行不是对象", entity.table))?;
            let workspace_id = object
                .get("workspace_id")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("INVALID_BACKUP: {} 缺少 workspace_id", entity.table))?;
            workspace_ids.insert(workspace_id.to_string());
            if let Some(id) = object.get("id") {
                let id = id
                    .as_str()
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| format!("INVALID_BACKUP: {} id 无效", entity.table))?;
                if !row_ids.insert(id.to_string()) {
                    return Err(format!("INVALID_BACKUP: {} 存在重复 id={id}", entity.table));
                }
            }
        }
    }
    if workspace_ids.len() > 1 {
        return Err("INVALID_BACKUP: 业务实体来自多个工作空间".to_string());
    }
    for row in tables.get("tasks").into_iter().flatten() {
        ensure_ref(row, "subject_id", &subjects, "事项主体")?;
        ensure_optional_ref(row, "parent_id", &tasks, "父事项")?;
    }
    for table in [
        "task_status_events",
        "task_day_plans",
        "task_daily_estimates",
        "task_recurrence_rules",
        "task_occurrences",
        "time_allocations",
        "actual_time_adjustments",
        "report_tasks",
    ] {
        for row in tables.get(table).into_iter().flatten() {
            ensure_ref(row, "task_id", &tasks, table)?;
        }
    }
    for row in tables.get("time_segments").into_iter().flatten() {
        ensure_ref(row, "entry_id", &entries, "时间段")?;
    }
    for row in tables.get("time_entries").into_iter().flatten() {
        ensure_optional_ref(row, "default_task_id", &tasks, "计时条目")?;
        ensure_optional_ref(row, "previous_entry_id", &entries, "计时条目")?;
        // 来源未归属会话只是一条溯源线索：会话记录本身可能已被正常清理
        // （例如云端删除同步回本地），此时计时条目依旧有效，不能因此拒绝备份。
        ensure_optional_soft_ref(row, "origin_unassigned_session_id", "计时条目")?;
    }
    for row in tables.get("time_allocations").into_iter().flatten() {
        ensure_ref(row, "entry_id", &entries, "工时分配")?;
    }
    for row in tables.get("subject_work_schedules").into_iter().flatten() {
        ensure_ref(row, "subject_id", &subjects, "工作时间")?;
    }
    for row in tables
        .get("subject_work_schedule_segments")
        .into_iter()
        .flatten()
    {
        ensure_ref(row, "schedule_id", &schedules, "工作时间段")?;
    }
    for row in tables.get("unassigned_sessions").into_iter().flatten() {
        ensure_optional_ref(row, "generated_entry_id", &entries, "未归属会话")?;
    }
    for row in tables.get("unassigned_segments").into_iter().flatten() {
        ensure_ref(row, "session_id", &sessions, "未归属时间段")?;
    }
    for row in tables.get("report_templates").into_iter().flatten() {
        ensure_optional_ref(row, "subject_id", &subjects, "报告模板")?;
    }
    for row in tables.get("reports").into_iter().flatten() {
        ensure_ref(row, "subject_id", &subjects, "报告主体")?;
        ensure_optional_ref(row, "template_id", &templates, "报告模板")?;
    }
    for row in tables.get("report_tasks").into_iter().flatten() {
        ensure_ref(row, "report_id", &reports, "报告事项")?;
    }
    for row in tables.get("actual_time_adjustments").into_iter().flatten() {
        ensure_ref(row, "subject_id", &subjects, "实际工时调整")?;
    }
    for row in tables
        .get("daily_report_import_sources")
        .into_iter()
        .flatten()
    {
        ensure_ref(row, "subject_id", &subjects, "日报导入来源")?;
        ensure_ref(row, "task_id", &tasks, "日报导入来源")?;
    }
    Ok(())
}

fn ensure_ref(
    row: &Value,
    field: &str,
    values: &HashSet<String>,
    label: &str,
) -> Result<(), String> {
    let value = row
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("INVALID_BACKUP: {label} 缺少 {field}"))?;
    if values.contains(value) {
        Ok(())
    } else {
        Err(format!(
            "INVALID_BACKUP: {label} 引用不存在 {field}={value}"
        ))
    }
}

fn ensure_optional_ref(
    row: &Value,
    field: &str,
    values: &HashSet<String>,
    label: &str,
) -> Result<(), String> {
    match row.get(field) {
        None | Some(Value::Null) => Ok(()),
        Some(Value::String(value)) if values.contains(value) => Ok(()),
        Some(Value::String(value)) => Err(format!(
            "INVALID_BACKUP: {label} 引用不存在 {field}={value}"
        )),
        _ => Err(format!("INVALID_BACKUP: {label} 的 {field} 类型无效")),
    }
}

/// 软溯源引用：目标记录允许缺失（历史数据里会话可能已被清理），因此只校验字段类型。
fn ensure_optional_soft_ref(row: &Value, field: &str, label: &str) -> Result<(), String> {
    match row.get(field) {
        None | Some(Value::Null) => Ok(()),
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(()),
        _ => Err(format!("INVALID_BACKUP: {label} 的 {field} 类型无效")),
    }
}

/// Parse a JSON text column for export, strip secret-bearing keys, and never
/// let unparsable free text survive: a raw string column could itself be a
/// credential.
fn parse_and_redact_json_field(object: &mut Map<String, Value>, field: &str) {
    let Some(raw) = object.get(field).and_then(Value::as_str) else {
        return;
    };
    match serde_json::from_str::<Value>(raw) {
        Ok(mut parsed) => {
            redact_secret_values(&mut parsed);
            object.insert(field.to_string(), parsed);
        }
        Err(_) => {
            object.insert(field.to_string(), Value::Null);
        }
    }
}

fn redact_secret_values(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.retain(|key, _| !is_secret_key(key));
            for nested in map.values_mut() {
                redact_secret_values(nested);
            }
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                redact_secret_values(item);
            }
        }
        _ => {}
    }
}

fn is_secret_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace(['-', ' '], "_");
    const MARKERS: &[&str] = &[
        "token",
        "secret",
        "password",
        "passwd",
        "api_key",
        "apikey",
        "authorization",
        "cookie",
        "credential",
        "private_key",
        "auth_session",
    ];
    MARKERS.iter().any(|marker| normalized.contains(marker))
}

fn sqlite_value_to_json(value: SqlValue) -> Value {
    match value {
        SqlValue::Null => Value::Null,
        SqlValue::Integer(value) => json!(value),
        SqlValue::Real(value) => json!(value),
        SqlValue::Text(value) => json!(value),
        SqlValue::Blob(value) => json!(String::from_utf8_lossy(&value)),
    }
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

pub fn canonical_json_bytes(value: &Value) -> Result<Vec<u8>, String> {
    serde_json::to_vec(&canonicalize_json(value)).map_err(|error| error.to_string())
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn create_json_archive(entries: &BTreeMap<String, Value>) -> Result<Vec<u8>, String> {
    validate_member_names(entries.keys().map(String::as_str))?;
    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o600);
    for (name, value) in entries {
        let bytes = canonical_json_bytes(value)?;
        if bytes.len() as u64 > MAX_ARCHIVE_MEMBER_BYTES {
            return Err(format!("BACKUP_TOO_LARGE: {name} 超出单文件限制"));
        }
        writer
            .start_file(name, options)
            .map_err(|error| error.to_string())?;
        writer
            .write_all(&bytes)
            .map_err(|error| error.to_string())?;
    }
    writer
        .finish()
        .map(|cursor| cursor.into_inner())
        .map_err(|error| error.to_string())
}

pub fn read_json_archive<R: Read + Seek>(reader: R) -> Result<BTreeMap<String, Value>, String> {
    let mut archive =
        ZipArchive::new(reader).map_err(|error| format!("INVALID_BACKUP: {error}"))?;
    if archive.len() > MAX_ARCHIVE_MEMBERS {
        return Err("INVALID_BACKUP: 归档成员数量超出限制".to_string());
    }
    let mut values = BTreeMap::new();
    let mut total_bytes = 0u64;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|error| error.to_string())?;
        if file.is_dir() {
            return Err("INVALID_BACKUP: 不允许目录成员".to_string());
        }
        let name = file.name().to_string();
        validate_member_names(std::iter::once(name.as_str()))?;
        if file.size() > MAX_ARCHIVE_MEMBER_BYTES {
            return Err(format!("BACKUP_TOO_LARGE: {name} 超出单文件限制"));
        }
        total_bytes = total_bytes.saturating_add(file.size());
        if total_bytes > MAX_ARCHIVE_TOTAL_BYTES {
            return Err("BACKUP_TOO_LARGE: 归档解压总大小超出限制".to_string());
        }
        let mut bytes = Vec::with_capacity(file.size() as usize);
        file.read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        let value = serde_json::from_slice(&bytes)
            .map_err(|error| format!("INVALID_BACKUP: {name} 不是有效 JSON: {error}"))?;
        if values.insert(name.clone(), value).is_some() {
            return Err(format!("INVALID_BACKUP: 归档成员重复 {name}"));
        }
    }
    Ok(values)
}

pub fn checksums_for(
    entries: &BTreeMap<String, Value>,
) -> Result<BTreeMap<String, String>, String> {
    entries
        .iter()
        .map(|(name, value)| Ok((name.clone(), sha256_hex(&canonical_json_bytes(value)?))))
        .collect()
}

fn validate_member_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Result<(), String> {
    let mut allowed = ARCHIVE_MEMBERS.into_iter().collect::<BTreeSet<_>>();
    allowed.insert(LEGACY_SYNC_ARCHIVE_MEMBER);
    for name in names {
        if !allowed.contains(name)
            || name.starts_with('/')
            || name.starts_with('\\')
            || name.contains("..")
            || name.contains('\\')
            || name.contains(':')
        {
            return Err(format!("INVALID_BACKUP: 不允许的归档成员 {name}"));
        }
    }
    Ok(())
}

fn canonicalize_json(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let ordered = object
                .iter()
                .map(|(key, value)| (key.clone(), canonicalize_json(value)))
                .collect::<BTreeMap<_, _>>();
            Value::Object(ordered.into_iter().collect::<Map<_, _>>())
        }
        Value::Array(values) => Value::Array(values.iter().map(canonicalize_json).collect()),
        other => other.clone(),
    }
}

pub fn to_json_value<T: Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::{tempdir, TempDir};

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct DataPackageCatalog {
        package_version: u32,
        groups: Vec<String>,
        entities: Vec<DataPackageCatalogEntity>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct DataPackageCatalogEntity {
        name: String,
        group: String,
        local_backup: String,
        cloud_snapshot: String,
        copy: String,
        exclusion_reason: Option<String>,
    }

    const SYNC_SOURCES: &[&str] = &[
        "subjects",
        "subject_work_schedules",
        "work_days",
        "app_settings",
        "tasks",
        "task_status_events",
        "task_day_plans",
        "task_daily_estimates",
        "task_recurrence_rules",
        "task_occurrences",
        "unassigned_sessions",
        "time_entries",
        "time_segments",
        "time_allocations",
        "actual_time_adjustments",
        "report_templates",
        "reports",
        "report_tasks",
        "integration_configs",
        "external_bindings",
    ];

    #[test]
    fn catalog_covers_every_synchronized_entity_and_excludes_device_state() {
        let catalog = PROTECTED_ENTITIES
            .iter()
            .filter_map(|entity| entity.sync_entity)
            .collect::<BTreeSet<_>>();
        assert_eq!(catalog, SYNC_SOURCES.iter().copied().collect());
        assert!(PROTECTED_ENTITIES
            .iter()
            .all(|entity| !entity.stable_order.is_empty()));
        assert!(PROTECTED_ENTITIES
            .windows(2)
            .all(|pair| pair[0].import_order < pair[1].import_order));
        for excluded in [
            "devices",
            "local_request_results",
            "device_hooks",
            "integration_runs",
            "integration_run_items",
        ] {
            assert!(EXCLUDED_TABLES.contains(&excluded));
            assert!(!PROTECTED_ENTITIES
                .iter()
                .any(|entity| entity.table == excluded));
        }
        assert!(EXCLUDED_FIELDS.contains(&"secret_ref"));
        assert!(EXCLUDED_FIELDS.contains(&"refresh_token"));
    }

    #[test]
    fn data_package_catalog_unifies_backup_snapshot_and_copy_scope() {
        let catalog: DataPackageCatalog = serde_json::from_str(DATA_PACKAGE_CATALOG_JSON).unwrap();
        assert_eq!(catalog.package_version, DATA_PACKAGE_FORMAT_VERSION);
        assert_eq!(
            catalog.groups,
            ["planning", "time_tracking", "reports", "shared_settings"]
        );

        let names = catalog
            .entities
            .iter()
            .map(|entity| entity.name.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            names.len(),
            catalog.entities.len(),
            "数据包目录存在重复实体"
        );

        let storage_boundaries: Value =
            serde_json::from_str(include_str!("../storage-boundaries.json")).unwrap();
        let local_business_tables = storage_boundaries["sqliteTables"]["localBusiness"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            names, local_business_tables,
            "新增本地业务表必须在数据包目录中显式分类"
        );

        let local_backup_entities = catalog
            .entities
            .iter()
            .filter(|entity| entity.local_backup == "include")
            .map(|entity| entity.name.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            local_backup_entities,
            PROTECTED_ENTITIES
                .iter()
                .map(|entity| entity.table)
                .collect::<BTreeSet<_>>(),
            "本地备份范围必须来自统一数据包目录"
        );

        let snapshot_sql =
            include_str!("../../supabase/20261018c_timegenie_workspace_snapshots.sql");
        assert!(snapshot_sql.contains(&format!(
            "format_version integer not null default {}",
            DATA_PACKAGE_FORMAT_VERSION
        )));
        let catalog_body = snapshot_sql
            .split("create or replace function timegenie.workspace_snapshot_entity_catalog()")
            .nth(1)
            .unwrap()
            .split("$$;")
            .next()
            .unwrap();
        let cloud_snapshot_entities = catalog_body
            .split("('")
            .skip(1)
            .filter_map(|value| value.split("','").next())
            .collect::<BTreeSet<_>>();
        let declared_cloud_snapshot_entities = catalog
            .entities
            .iter()
            .filter(|entity| entity.cloud_snapshot == "include")
            .map(|entity| entity.name.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            declared_cloud_snapshot_entities, cloud_snapshot_entities,
            "云端快照 SQL 范围必须来自统一数据包目录"
        );

        let copy_entities = catalog
            .entities
            .iter()
            .filter(|entity| entity.copy == "include")
            .map(|entity| entity.name.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(copy_entities, cloud_snapshot_entities);
        assert!(copy_entities.is_subset(&local_backup_entities));

        for entity in &catalog.entities {
            assert!(matches!(
                entity.local_backup.as_str(),
                "include" | "exclude" | "metadata"
            ));
            assert!(matches!(
                entity.cloud_snapshot.as_str(),
                "include" | "exclude" | "metadata"
            ));
            assert!(matches!(
                entity.copy.as_str(),
                "include" | "exclude" | "metadata"
            ));
            if entity.local_backup != "include"
                || entity.cloud_snapshot != "include"
                || entity.copy != "include"
            {
                assert!(
                    entity
                        .exclusion_reason
                        .as_deref()
                        .is_some_and(|reason| !reason.trim().is_empty()),
                    "排除或元数据实体 {} 必须说明原因",
                    entity.name
                );
            }
            if entity.copy == "include" {
                assert!(catalog.groups.contains(&entity.group));
            }
        }
    }

    #[test]
    fn canonical_json_is_stable_for_object_key_order() {
        let left = json!({"z": 1, "a": {"d": 4, "b": 2}});
        let right: Value = serde_json::from_str(r#"{"a":{"b":2,"d":4},"z":1}"#).unwrap();
        assert_eq!(
            canonical_json_bytes(&left).unwrap(),
            canonical_json_bytes(&right).unwrap()
        );
        assert_eq!(
            sha256_hex(&canonical_json_bytes(&left).unwrap()),
            sha256_hex(&canonical_json_bytes(&right).unwrap())
        );
    }

    #[test]
    fn archive_round_trip_preserves_allowed_json_members() {
        let entries = BTreeMap::from([
            (
                "manifest.json".to_string(),
                json!({"formatVersion": BACKUP_FORMAT_VERSION}),
            ),
            (
                "workspace.json".to_string(),
                json!({"name": "本地工作空间"}),
            ),
        ]);
        let bytes = create_json_archive(&entries).unwrap();
        assert_eq!(read_json_archive(Cursor::new(bytes)).unwrap(), entries);
    }

    #[test]
    fn archive_rejects_path_traversal_and_unknown_members() {
        for name in ["../manifest.json", "data\\planning.json", "secrets.json"] {
            let entries = BTreeMap::from([(name.to_string(), json!({}))]);
            assert!(create_json_archive(&entries)
                .unwrap_err()
                .contains("不允许"));
        }
    }

    #[test]
    fn archive_rejects_declared_oversized_member_before_allocation() {
        let cursor = Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(cursor);
        writer
            .start_file("manifest.json", SimpleFileOptions::default())
            .unwrap();
        let oversized = vec![b' '; (MAX_ARCHIVE_MEMBER_BYTES + 1) as usize];
        writer.write_all(&oversized).unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        assert!(read_json_archive(Cursor::new(bytes))
            .unwrap_err()
            .contains("超出单文件限制"));
    }

    #[test]
    fn exported_backup_is_recorded_and_missing_files_remain_visible() {
        let (directory, database, _, _) = test_database();
        let path = directory.path().join("history.timegenie-backup");
        let result = export_backup(
            &database,
            BackupExportRequest {
                path: path.display().to_string(),
            },
        )
        .unwrap();

        let history = backup_history_list(&database).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].path, result.path);
        assert_eq!(history[0].origin, "manual");
        assert!(history[0].file_exists);
        assert!(history[0].size_bytes > 0);

        fs::remove_file(path).unwrap();
        let history = backup_history_list(&database).unwrap();
        assert!(!history[0].file_exists);
    }

    fn test_database() -> (TempDir, crate::database::Database, String, String) {
        let directory = tempdir().unwrap();
        let database = crate::database::Database::initialize_at(
            directory.path().join("data-protection.sqlite3"),
        )
        .unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let subject_id: String = connection
            .query_row(
                "SELECT id FROM subjects WHERE workspace_id=?1 ORDER BY sort_order LIMIT 1",
                [&workspace_id],
                |row| row.get(0),
            )
            .unwrap();
        (directory, database, workspace_id, subject_id)
    }

    fn set_device_setting(database: &crate::database::Database, key: &str, value: Value) {
        database
            .open()
            .unwrap()
            .execute(
                "INSERT INTO device_settings(key,value_json,updated_at) VALUES(?1,?2,1) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json",
                params![key, value.to_string()],
            )
            .unwrap();
    }

    fn rewrite_archive(
        source: &Path,
        target: &Path,
        mutate: impl FnOnce(&mut BTreeMap<String, Value>),
    ) {
        let mut entries = read_json_archive(fs::File::open(source).unwrap()).unwrap();
        mutate(&mut entries);
        let content = entries
            .iter()
            .filter(|(name, _)| name.as_str() != "checksums.json")
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect::<BTreeMap<_, _>>();
        entries.insert(
            "checksums.json".to_string(),
            to_json_value(checksums_for(&content).unwrap()).unwrap(),
        );
        fs::write(target, create_json_archive(&entries).unwrap()).unwrap();
    }

    #[test]
    fn standard_export_excludes_secrets_device_state_automation_and_outbox() {
        let (directory, database, workspace_id, subject_id) = test_database();
        let connection = database.open().unwrap();
        connection
            .execute(
                "INSERT INTO integration_configs(workspace_id,provider,enabled,config_json,secret_ref,updated_at,version) VALUES(?1,'seatable',1,'{\"serverUrl\":\"https://example.invalid\"}','keyring://real-secret',1,1)",
                [&workspace_id],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO device_hooks(id,name,event_type,action_type,action_config_json,timeout_seconds,enabled,sort_order,created_at,updated_at) VALUES('hook-secret','危险命令','timer.started','process','{\"program\":\"secret.exe\",\"args\":[\"token-value\"]}',10,1,10,1,1)",
                [],
            )
            .unwrap();
        set_device_setting(
            &database,
            "supabase_project_url",
            json!("https://secret.supabase.co"),
        );
        set_device_setting(&database, "supabase_anon_key", json!("actual-anon-secret"));
        set_device_setting(
            &database,
            &format!("subject_daily_report_directory:{subject_id}"),
            json!("D:/secret/daily-report-directory"),
        );
        connection
            .execute(
                "UPDATE legacy_sync_cleanup_diagnostics SET cleanup_reason='password=cleanup-diagnostic-secret' WHERE table_name='tracking_leases'",
                [],
            )
            .unwrap();
        drop(connection);

        let path = directory.path().join("standard.timegenie-backup");
        export_backup(
            &database,
            BackupExportRequest {
                path: path.display().to_string(),
            },
        )
        .unwrap();
        let bytes = fs::read(&path).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        for forbidden in [
            "actual-anon-secret",
            "secret.supabase.co",
            "keyring://real-secret",
            "secret.exe",
            "hook-secret",
            "cleanup-diagnostic-secret",
            "D:/secret/daily-report-directory",
        ] {
            assert!(!text.contains(forbidden), "archive leaked {forbidden}");
        }
        let validated = validate_backup_path(&path).unwrap();
        let integration = &validated.tables["integration_configs"][0];
        assert!(integration.get("secret_ref").is_none());
        assert!(!validated.tables.contains_key("devices"));
        assert!(!validated.tables.contains_key("sync_outbox"));
        assert!(!validated
            .tables
            .contains_key("legacy_sync_cleanup_diagnostics"));
    }

    #[test]
    fn daily_report_import_source_is_backed_up_without_absolute_path() {
        let (directory, database, workspace_id, subject_id) = test_database();
        let connection = database.open().unwrap();
        let task_id: String = connection
            .query_row(
                "SELECT id FROM tasks WHERE workspace_id=?1 AND subject_id=?2 LIMIT 1",
                params![workspace_id, subject_id],
                |row| row.get(0),
            )
            .unwrap();
        connection.execute(
            "INSERT INTO daily_report_import_sources(id,workspace_id,subject_id,source_device_id,source_path_hash,source_date,target_kind,source_fingerprint,task_id,batch_id,created_at) VALUES('daily-source',?1,?2,'old-device','path-hash','2026-10-22','backlog','item-hash',?3,'batch',1)",
            params![workspace_id, subject_id, task_id],
        ).unwrap();
        drop(connection);
        set_device_setting(
            &database,
            &format!("subject_daily_report_directory:{subject_id}"),
            json!("D:/secret/daily-report-directory"),
        );
        let path = directory.path().join("daily-source.timegenie-backup");
        export_backup(
            &database,
            BackupExportRequest {
                path: path.display().to_string(),
            },
        )
        .unwrap();
        assert!(!String::from_utf8_lossy(&fs::read(&path).unwrap())
            .contains("D:/secret/daily-report-directory"));
        let validated = validate_backup_path(&path).unwrap();
        let source = &validated.tables["daily_report_import_sources"][0];
        assert_eq!(source["task_id"], json!(task_id));
        assert_eq!(source["source_path_hash"], json!("path-hash"));
    }

    #[test]
    fn backup_audit_scans_standard_archive_for_secrets_and_runtime_metadata() {
        let (directory, database, workspace_id, _) = test_database();
        let device_id: String;
        {
            let connection = database.open().unwrap();
            device_id = connection
                .query_row(
                    "SELECT id FROM devices WHERE workspace_id=?1",
                    [&workspace_id],
                    |row| row.get(0),
                )
                .unwrap();
            connection
                .execute(
                    "UPDATE devices SET device_name='audit-device-name-5c7',platform='audit-platform',app_version='9.9.9' WHERE id=?1",
                    [&device_id],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO integration_configs(workspace_id,provider,enabled,config_json,secret_ref,updated_at,version) VALUES(?1,'seatable',1,?2,?3,1,1)",
                    params![
                        workspace_id,
                        json!({
                            "serverUrl": "https://audit-seatable.example.invalid",
                            "token": "seatable-real-token-7f3a",
                            "tableId": "tbl-audit"
                        })
                        .to_string(),
                        "keyring://seatable-secret-ref-9c1"
                    ],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO device_hooks(id,name,event_type,action_type,action_config_json,timeout_seconds,enabled,sort_order,created_at,updated_at) VALUES('hook-audit','审计自动化','timer.started','process',?1,10,1,10,1,1)",
                    [json!({
                        "program": "audit-secret-program.exe",
                        "args": ["--token", "automation-command-4b2e"]
                    })
                    .to_string()],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO app_settings(workspace_id,key,value_json,updated_at,version) VALUES(?1,'audit_safe_setting',?2,1,1)",
                    params![workspace_id, json!({ "value": "safe-shared-setting" }).to_string()],
                )
                .unwrap();
        }
        set_device_setting(
            &database,
            "supabase_project_url",
            json!("https://audit-secret-project.supabase.co"),
        );
        set_device_setting(
            &database,
            "supabase_anon_key",
            json!("anon-audit-secret-1d9"),
        );
        set_device_setting(&database, "storage_mode", json!("cloud"));
        set_device_setting(
            &database,
            "cloud_workspace_id",
            json!("audit-cloud-workspace"),
        );
        crate::supabase::install_test_session_for_database(&database, "audit-user-secret-2f81")
            .unwrap();

        let standard = directory.path().join("audit-standard.timegenie-backup");
        let standard_result = export_backup(
            &database,
            BackupExportRequest {
                path: standard.display().to_string(),
            },
        )
        .unwrap();
        assert_eq!(standard_result.backup_kind, BACKUP_KIND_STANDARD);
        assert!(standard_result.cloud_consistent);

        let forbidden = [
            "seatable-real-token-7f3a",
            "keyring://seatable-secret-ref-9c1",
            "audit-secret-program.exe",
            "automation-command-4b2e",
            "hook-audit",
            "anon-audit-secret-1d9",
            "audit-secret-project.supabase.co",
            "audit-user-secret-2f81",
            "access-audit-user-secret-2f81",
            "refresh-audit-user-secret-2f81",
            "audit-device-name-5c7",
        ];
        for (label, path) in [("standard", &standard)] {
            let entries = read_json_archive(fs::File::open(path).unwrap()).unwrap();
            let text = entries
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<_>>()
                .join("\n");
            let lower = text.to_lowercase();
            for needle in forbidden {
                assert!(
                    !lower.contains(&needle.to_lowercase()),
                    "{label} 备份泄露 {needle}"
                );
            }
            assert!(
                !lower.contains(device_id.to_lowercase().as_str()),
                "{label} 备份泄露设备身份"
            );
            let validated = validate_backup_path(path).unwrap();
            for excluded in [
                "devices",
                "device_settings",
                "device_hooks",
                "local_sync_state",
                "sync_outbox",
                "legacy_sync_cleanup_diagnostics",
            ] {
                assert!(
                    !validated.tables.contains_key(excluded),
                    "{label} 备份包含排除表 {excluded}"
                );
            }
            for row in validated
                .tables
                .get("integration_configs")
                .into_iter()
                .flatten()
            {
                assert!(row.get("secret_ref").is_none());
                assert!(row.get("token").is_none());
            }
            assert!(validated
                .manifest
                .excluded_categories
                .contains(&"系统凭据与 Token".to_string()));
            assert!(validated
                .manifest
                .excluded_categories
                .contains(&"同步操作 payload、设备身份与同步游标".to_string()));
        }
        let validated_standard = validate_backup_path(&standard).unwrap();
        assert_eq!(validated_standard.manifest.pending_operation_count, 0);
    }

    #[test]
    fn preview_rejects_tampering_newer_version_and_missing_reference_without_mutation() {
        let (directory, database, workspace_id, _) = test_database();
        let started_at = now_millis() - 180_000;
        database
            .open()
            .unwrap()
            .execute(
                "INSERT INTO time_entries(id,workspace_id,work_date,kind,source_type,state,label_snapshot,started_at,ended_at,duration_seconds,created_at,updated_at,version)\
                 VALUES('tamper-target',?1,'1999-01-04','work','manual','ended','待校验',?2,?3,180,?2,?2,1)",
                params![workspace_id, started_at, started_at + 180_000],
            )
            .unwrap();
        let source = directory.path().join("valid.timegenie-backup");
        export_backup(
            &database,
            BackupExportRequest {
                path: source.display().to_string(),
            },
        )
        .unwrap();
        let before = table_counts(&database.open().unwrap(), &workspace_id).unwrap();

        let mut entries = read_json_archive(fs::File::open(&source).unwrap()).unwrap();
        entries.get_mut("workspace.json").unwrap()["name"] = json!("篡改但不更新校验和");
        let tampered = directory.path().join("tampered.timegenie-backup");
        fs::write(&tampered, create_json_archive(&entries).unwrap()).unwrap();
        assert!(validate_backup_path(&tampered)
            .unwrap_err()
            .contains("校验和"));

        let newer = directory.path().join("newer.timegenie-backup");
        rewrite_archive(&source, &newer, |entries| {
            entries.get_mut("manifest.json").unwrap()["formatVersion"] =
                json!(BACKUP_FORMAT_VERSION + 1);
        });
        assert!(validate_backup_path(&newer)
            .unwrap_err()
            .contains("UNSUPPORTED_BACKUP_VERSION"));

        let missing = directory.path().join("missing-reference.timegenie-backup");
        rewrite_archive(&source, &missing, |entries| {
            let planning = entries
                .get_mut("data/planning.json")
                .unwrap()
                .as_object_mut()
                .unwrap();
            let tasks = planning.get_mut("tasks").unwrap().as_array_mut().unwrap();
            tasks[0]["subject_id"] = json!("missing-subject");
        });
        assert!(validate_backup_path(&missing)
            .unwrap_err()
            .contains("引用不存在"));

        let dangling_previous = directory.path().join("dangling-previous.timegenie-backup");
        rewrite_archive(&source, &dangling_previous, |entries| {
            let tracking = entries
                .get_mut("data/time-tracking.json")
                .unwrap()
                .as_object_mut()
                .unwrap();
            let rows = tracking
                .get_mut("time_entries")
                .unwrap()
                .as_array_mut()
                .unwrap();
            let row = rows
                .iter_mut()
                .find(|row| row["id"] == json!("tamper-target"))
                .unwrap();
            row["previous_entry_id"] = json!("missing-entry");
        });
        assert!(validate_backup_path(&dangling_previous)
            .unwrap_err()
            .contains("引用不存在"));
        assert_eq!(
            table_counts(&database.open().unwrap(), &workspace_id).unwrap(),
            before
        );
    }

    #[test]
    fn import_round_trip_remaps_identity_and_freezes_active_tracking() {
        let (directory, database, old_workspace_id, subject_id) = test_database();
        let connection = database.open().unwrap();
        let task_id: String = connection
            .query_row(
                "SELECT id FROM tasks WHERE workspace_id=?1 AND parent_id IS NULL LIMIT 1",
                [&old_workspace_id],
                |row| row.get(0),
            )
            .unwrap();
        let started_at = now_millis() - 60_000;
        connection.execute(
            "INSERT INTO task_day_plans(workspace_id,task_id,work_date,state,created_at,updated_at,version)
             VALUES(?1,?2,'1999-01-02','included',?3,?3,4),
                   (?1,?2,'1999-01-03','excluded',?3,?3,2)",
            params![old_workspace_id, task_id, started_at],
        ).unwrap();
        connection.execute("INSERT INTO time_entries(id,workspace_id,work_date,kind,source_type,state,default_task_id,label_snapshot,started_at,duration_seconds,created_at,updated_at,version,origin_unassigned_session_id) VALUES('running-entry',?1,'1999-01-02','work','timer','running',?2,'运行中',?3,0,?3,?3,1,'active-unassigned')",params![old_workspace_id,task_id,started_at]).unwrap();
        connection.execute("INSERT INTO time_segments(id,workspace_id,entry_id,sequence_no,started_at,duration_seconds) VALUES('open-segment',?1,'running-entry',1,?2,0)",params![old_workspace_id,started_at]).unwrap();
        connection.execute("INSERT INTO unassigned_sessions(id,workspace_id,work_date,state,threshold_seconds,duration_seconds,first_started_at,generated_entry_id,created_at,updated_at,version) VALUES('active-unassigned',?1,'1999-01-02','collecting',300,0,?2,'running-entry',?2,?2,1)",params![old_workspace_id,started_at]).unwrap();
        connection.execute("INSERT INTO unassigned_segments(id,workspace_id,session_id,sequence_no,started_at,duration_seconds,lease_token) VALUES('open-unassigned-segment',?1,'active-unassigned',1,?2,0,'lease-secret')",params![old_workspace_id,started_at]).unwrap();
        connection.execute("INSERT INTO integration_configs(workspace_id,provider,enabled,config_json,secret_ref,updated_at,version) VALUES(?1,'seatable',1,'{}','keyring://secret',1,1)",[&old_workspace_id]).unwrap();
        drop(connection);

        let source = directory.path().join("round-trip.timegenie-backup");
        let exported = export_backup(
            &database,
            BackupExportRequest {
                path: source.display().to_string(),
            },
        )
        .unwrap();
        let safety = directory.path().join("before-import.timegenie-backup");
        let imported = import_backup_with_safety_path(
            &database,
            BackupImportRequest {
                path: source.display().to_string(),
                confirmed: true,
            },
            safety.clone(),
        )
        .unwrap();
        assert_ne!(imported.workspace_id, old_workspace_id);
        assert!(safety.exists());
        assert_eq!(imported.imported_entity_counts, exported.entity_counts);
        let connection = database.open().unwrap();
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM workspaces", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT storage_mode FROM workspaces WHERE id=?1",
                    [&imported.workspace_id],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "local"
        );
        let restored_mode: String = connection
            .query_row(
                "SELECT json_extract(value_json,'$') FROM device_settings WHERE key='storage_mode'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(restored_mode, "local");
        let legacy_runtime_tables: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN (
                   'sync_outbox','local_sync_state','cloud_deferred_entities',
                   'legacy_abandoned_outbox','processed_operations','sync_items','sync_runs'
                 )",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(legacy_runtime_tables, 0);
        let (state, ended_at, duration, origin): (String, Option<i64>, i64, Option<String>) = connection.query_row("SELECT state,ended_at,duration_seconds,origin_unassigned_session_id FROM time_entries WHERE id='running-entry'",[],|row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).unwrap();
        assert_eq!(state, "paused");
        assert!(ended_at.is_none());
        assert!(duration >= 60);
        assert_eq!(origin.as_deref(), Some("active-unassigned"));
        let (session_state, resolution, generated): (String, Option<String>, Option<String>) = connection.query_row("SELECT state,resolution_type,generated_entry_id FROM unassigned_sessions WHERE id='active-unassigned'",[],|row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).unwrap();
        assert_eq!(session_state, "discarded");
        assert_eq!(resolution.as_deref(), Some("discard"));
        assert_eq!(generated.as_deref(), Some("running-entry"));
        assert_eq!(connection.query_row("SELECT COUNT(*) FROM integration_configs WHERE provider='seatable' AND secret_ref IS NULL", [], |row| row.get::<_, i64>(0)).unwrap(), 1);
        let restored_subject_id: String = connection
            .query_row(
                "SELECT id FROM subjects WHERE id=?1",
                [&subject_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(restored_subject_id, subject_id);
        let restored_plans = connection
            .prepare("SELECT work_date,state,version FROM task_day_plans WHERE task_id=?1 ORDER BY work_date")
            .unwrap()
            .query_map([&task_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            restored_plans,
            vec![
                ("1999-01-02".to_string(), "included".to_string(), 4),
                ("1999-01-03".to_string(), "excluded".to_string(), 2),
            ]
        );
        let estimate_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM task_daily_estimates WHERE task_id=?1",
                [&task_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(estimate_count, 0);
    }

    #[test]
    fn stale_origin_unassigned_session_reference_survives_backup_round_trip() {
        let (directory, database, workspace_id, _) = test_database();
        let started_at = now_millis() - 120_000;
        database
            .open()
            .unwrap()
            .execute(
                "INSERT INTO time_entries(id,workspace_id,work_date,kind,source_type,state,label_snapshot,started_at,ended_at,duration_seconds,created_at,updated_at,version,origin_unassigned_session_id)\
                 VALUES('stale-origin-entry',?1,'1999-01-03','work','unassigned','ended','来源会话已清理',?2,?3,120,?2,?2,1,'01a10260-c665-74c3-86a5-f846c5a50974')",
                params![workspace_id, started_at, started_at + 120_000],
            )
            .unwrap();

        let source = directory.path().join("stale-origin.timegenie-backup");
        let exported = export_backup(
            &database,
            BackupExportRequest {
                path: source.display().to_string(),
            },
        )
        .unwrap();
        assert_eq!(exported.entity_counts["time_entries"], 1);
        validate_backup_path(&source).unwrap();

        let imported = import_backup_with_safety_path(
            &database,
            BackupImportRequest {
                path: source.display().to_string(),
                confirmed: true,
            },
            directory
                .path()
                .join("stale-origin-safety.timegenie-backup"),
        )
        .unwrap();
        assert_eq!(imported.imported_entity_counts["time_entries"], 1);

        let connection = database.open().unwrap();
        let (label, duration, origin): (String, i64, Option<String>) = connection
            .query_row(
                "SELECT label_snapshot,duration_seconds,origin_unassigned_session_id FROM time_entries WHERE id='stale-origin-entry'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(label, "来源会话已清理");
        assert_eq!(duration, 120);
        assert!(origin.is_none());
    }

    #[test]
    fn safety_backup_failure_and_import_constraint_error_leave_current_workspace_unchanged() {
        let (directory, database, workspace_id, _) = test_database();
        let source = directory.path().join("source.timegenie-backup");
        export_backup(
            &database,
            BackupExportRequest {
                path: source.display().to_string(),
            },
        )
        .unwrap();
        let before = table_counts(&database.open().unwrap(), &workspace_id).unwrap();
        let blocked_parent = directory.path().join("not-a-directory");
        fs::write(&blocked_parent, b"file").unwrap();
        let error = import_backup_with_safety_path(
            &database,
            BackupImportRequest {
                path: source.display().to_string(),
                confirmed: true,
            },
            blocked_parent.join("safety.timegenie-backup"),
        )
        .unwrap_err();
        assert!(error.contains("无法创建备份目录"));
        assert_eq!(
            table_counts(&database.open().unwrap(), &workspace_id).unwrap(),
            before
        );

        let mut validated = validate_backup_path(&source).unwrap();
        let duplicate_subject = validated.tables["subjects"][0].clone();
        validated
            .tables
            .get_mut("subjects")
            .unwrap()
            .push(duplicate_subject);
        let error = restore_validated_backup(&database, &validated).unwrap_err();
        assert!(error.contains("UNIQUE constraint failed") || error.contains("导入 subjects 失败"));
        let connection = database.open().unwrap();
        assert_eq!(
            connection
                .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            workspace_id
        );
        assert_eq!(table_counts(&connection, &workspace_id).unwrap(), before);
    }
}
