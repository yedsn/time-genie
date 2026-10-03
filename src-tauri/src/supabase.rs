use base64::Engine;
use reqwest::blocking::{Client, RequestBuilder};
use reqwest::StatusCode;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use url::Url;
use uuid::Uuid;

use crate::database::Database;
use crate::settings::{self, SettingsScope, SettingsUpdate};

const SESSION_PROVIDER: &str = "supabase";
const DEFAULT_TIMEOUT_SECONDS: u64 = 12;
pub(crate) const CLOUD_SCHEMA: &str = "timegenie";
static SESSION_CACHE: OnceLock<Mutex<HashMap<String, CloudSession>>> = OnceLock::new();

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
    pub user_id: Option<String>,
    pub email: Option<String>,
    pub expires_at: Option<i64>,
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
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageModeSnapshot {
    pub mode: String,
    pub workspace_id: String,
    pub device_id: String,
    pub online: bool,
    pub sync_state: String,
    pub pending_operations: i64,
    pub conflict_count: i64,
    pub last_change_seq: i64,
    pub last_synced_at: Option<i64>,
    pub last_error: Option<String>,
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
    pub entities: Vec<MigrationEntityCount>,
    pub conflicts: Vec<String>,
    pub can_execute: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudConfigureRequest {
    pub project_url: String,
    pub anon_key: String,
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
}

pub fn ensure_local_mode(database: &Database) -> Result<(), String> {
    let connection = database.open()?;
    let mode: String = connection
        .query_row(
            "SELECT COALESCE(json_extract(value_json, '$'), 'local') FROM device_settings WHERE key = 'storage_mode'",
            [],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| "local".to_string());
    if mode == "cloud" {
        return Err(
            "OFFLINE_RESTRICTED: 当前是 Supabase 云端模式，该本地命令尚未接入云端 Repository"
                .to_string(),
        );
    }
    Ok(())
}

/// Ordinary business edits always target the local SQLite cache first. In
/// cloud mode they are appended to `sync_outbox` by the owning command and
/// become authoritative only after Supabase accepts the operation.
pub fn ensure_repository_write_mode(_database: &Database) -> Result<(), String> {
    Ok(())
}

pub fn storage_mode(database: &Database) -> Result<StorageModeSnapshot, String> {
    let connection = database.open()?;
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
    let (last_change_seq, last_synced_at, last_error): (i64, Option<i64>, Option<String>) = connection
        .query_row(
            "SELECT last_change_seq, last_full_sync_at, last_error FROM local_sync_state WHERE workspace_id = ?1",
            [&sync_workspace_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap_or((0, None, None));
    Ok(StorageModeSnapshot {
        online: mode == "local" || current_session(database).is_ok(),
        sync_state: if last_error.is_some() {
            "error".to_string()
        } else if pending > 0 || conflicts > 0 {
            "pending".to_string()
        } else {
            "synced".to_string()
        },
        mode,
        workspace_id,
        device_id,
        pending_operations: pending,
        conflict_count: conflicts,
        last_change_seq,
        last_synced_at,
        last_error,
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
    Ok(())
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
    Ok(session_snapshot(&session))
}

pub fn sign_out(database: &Database) -> Result<(), String> {
    ensure_no_pending_cloud_work(database)?;
    let connection = database.open()?;
    let workspace_id: String = connection
        .query_row(
            "SELECT id FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let entry = keyring::Entry::new("timegenie", &format!("{SESSION_PROVIDER}:{workspace_id}"))
        .map_err(|error| format!("无法访问系统凭据库: {error}"))?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {
            remove_cached_session(&workspace_id);
            Ok(())
        }
        Err(error) => Err(format!("删除 Supabase 会话失败: {error}")),
    }
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
    let is_changing = current_project_url
        .as_deref()
        .is_some_and(|current| current != project_url)
        || current_anon_key
            .as_deref()
            .is_some_and(|current| current != anon_key);
    drop(connection);

    if is_changing {
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
        Ok(session) => Ok(session_snapshot(&session)),
        Err(_) => Ok(CloudSessionSnapshot {
            signed_in: false,
            user_id: None,
            email: None,
            expires_at: None,
        }),
    }
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
    let mut workspaces: Vec<WorkspaceRow> = client
        .request(
            client
                .get(format!(
                    "{}/rest/v1/workspaces?owner_user_id=eq.{}&deleted_at=is.null&select=id,name,timezone&limit=1",
                    client.project_url, session.user_id
                )),
            &session,
        )?
        .json()
        .map_err(|error| format!("NETWORK_ERROR: 读取 Supabase 工作空间失败: {error}"))?;
    if workspaces.is_empty() {
        let response = client.request(
            client
                .post(format!("{}/rest/v1/workspaces", client.project_url))
                .header("Prefer", "return=representation")
                .json(&json!({
                    "owner_user_id": session.user_id,
                    "name": "我的工作台",
                    "timezone": "Asia/Shanghai"
                })),
            &session,
        )?;
        workspaces = response
            .json()
            .map_err(|error| format!("NETWORK_ERROR: 创建 Supabase 工作空间失败: {error}"))?;
    }
    let workspace = workspaces
        .into_iter()
        .next()
        .ok_or_else(|| "VERSION_CONFLICT: Supabase 未返回可用工作空间".to_string())?;
    let _ = client.rpc(
        "workspace_initialize_defaults",
        json!({ "p_workspace_id": workspace.id.clone() }),
        &session,
    )?;
    settings::update_setting(
        database,
        SettingsUpdate {
            scope: SettingsScope::Device,
            key: "cloud_workspace_id".to_string(),
            value: json!(workspace.id),
        },
    )?;
    let latest_change_seq = latest_change_seq(&client, &session, &workspace.id)?;
    Ok(CloudWorkspace {
        id: workspace.id,
        name: workspace.name,
        timezone: workspace.timezone,
        latest_change_seq,
    })
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
    let response = client.request(
        client
            .post(format!(
                "{}/rest/v1/devices?on_conflict=id",
                client.project_url
            ))
            .header(
                "Prefer",
                "resolution=merge-duplicates,return=representation",
            )
            .json(&json!({
                "id": device_id,
                "workspace_id": workspace.id,
                "device_name": request.device_name.trim(),
                "platform": request.platform,
                "app_version": request.app_version,
                "last_seen_at": chrono::Utc::now().to_rfc3339()
            })),
        &session,
    )?;
    let device: DeviceRow = response
        .json::<Vec<DeviceRow>>()
        .map_err(|error| format!("NETWORK_ERROR: 注册设备响应无效: {error}"))?
        .into_iter()
        .next()
        .ok_or_else(|| "NETWORK_ERROR: Supabase 未返回设备".to_string())?;
    Ok(CloudDevice {
        id: device.id,
        workspace_id: device.workspace_id,
        device_name: device.device_name,
        platform: device.platform,
        app_version: device.app_version,
        last_seen_at: device.last_seen_at,
    })
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
    let entities = [
        (
            "subjects",
            "SELECT COUNT(*) FROM subjects WHERE workspace_id = ?1 AND deleted_at IS NULL",
        ),
        (
            "tasks",
            "SELECT COUNT(*) FROM tasks WHERE workspace_id = ?1 AND deleted_at IS NULL",
        ),
        (
            "time_entries",
            "SELECT COUNT(*) FROM time_entries WHERE workspace_id = ?1 AND deleted_at IS NULL",
        ),
        (
            "time_allocations",
            "SELECT COUNT(*) FROM time_allocations WHERE workspace_id = ?1",
        ),
        (
            "reports",
            "SELECT COUNT(*) FROM reports WHERE workspace_id = ?1 AND deleted_at IS NULL",
        ),
        (
            "task_daily_estimates",
            "SELECT COUNT(*) FROM task_daily_estimates WHERE workspace_id = ?1",
        ),
        (
            "task_recurrence_rules",
            "SELECT COUNT(*) FROM task_recurrence_rules WHERE workspace_id = ?1",
        ),
        (
            "task_occurrences",
            "SELECT COUNT(*) FROM task_occurrences WHERE workspace_id = ?1",
        ),
    ];
    let local_counts = entities
        .iter()
        .map(|(entity, sql)| {
            connection
                .query_row(sql, [source_workspace_id.as_str()], |row| row.get(0))
                .map(|count| MigrationEntityCount {
                    entity: (*entity).to_string(),
                    local_count: count,
                    cloud_count: None,
                })
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let workspace = workspace_bootstrap(database)?;
    let session = current_session(database)?;
    let snapshot = client(database)?.rpc(
        "cloud_snapshot_get",
        json!({ "p_workspace_id": workspace.id.clone() }),
        &session,
    )?;
    let cloud_counts = [
        "subjects",
        "tasks",
        "time_entries",
        "time_allocations",
        "reports",
        "task_daily_estimates",
        "task_recurrence_rules",
        "task_occurrences",
    ]
    .into_iter()
    .map(|entity| {
        (
            entity,
            snapshot
                .get(entity)
                .and_then(Value::as_array)
                .map(|rows| rows.len() as i64)
                .unwrap_or(0),
        )
    })
    .collect::<std::collections::HashMap<_, _>>();
    let local_counts = local_counts
        .into_iter()
        .map(|mut count| {
            count.cloud_count = cloud_counts.get(count.entity.as_str()).copied();
            count
        })
        .collect::<Vec<_>>();
    let cloud_has_business_data = cloud_counts.get("tasks").copied().unwrap_or(0) > 0
        || cloud_counts.get("time_entries").copied().unwrap_or(0) > 0
        || cloud_counts.get("reports").copied().unwrap_or(0) > 0;
    let mut conflicts = Vec::new();
    if request.direction == "local_to_cloud" && cloud_has_business_data {
        conflicts.push("云端工作空间已有业务数据，请使用现有云端数据，不能覆盖上传".to_string());
    }
    Ok(MigrationPreview {
        direction: request.direction,
        source_workspace_id,
        target_workspace_id: Some(workspace.id),
        entities: local_counts,
        can_execute: conflicts.is_empty(),
        conflicts,
    })
}

pub fn migration_execute(
    database: &Database,
    request: MigrationExecuteRequest,
) -> Result<StorageModeSnapshot, String> {
    if !request.confirmed {
        return Err("MIGRATION_NOT_CONFIRMED: 迁移必须明确确认后执行".to_string());
    }
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
        client.rpc(
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
    client: &SupabaseClient,
    session: &CloudSession,
    workspace_id: &str,
) -> Result<i64, String> {
    let rows: Vec<ChangeSeqRow> = client
        .request(
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

    pub(crate) fn request(
        &self,
        request: RequestBuilder,
        session: &CloudSession,
    ) -> Result<reqwest::blocking::Response, String> {
        let response = request
            .bearer_auth(&session.access_token)
            .send()
            .map_err(|error| format!("NETWORK_ERROR: Supabase 请求失败: {error}"))?;
        if !response.status().is_success() {
            return Err(map_http_error(response, "CLOUD_REQUEST_FAILED"));
        }
        Ok(response)
    }

    pub(crate) fn rpc(
        &self,
        function: &str,
        body: Value,
        session: &CloudSession,
    ) -> Result<Value, String> {
        self.request(
            self.post(format!("{}/rest/v1/rpc/{function}", self.project_url))
                .json(&body),
            session,
        )?
        .json()
        .map_err(|error| format!("NETWORK_ERROR: Supabase RPC 响应无效: {error}"))
    }
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
    let connection = database.open()?;
    let workspace_id: String = connection
        .query_row(
            "SELECT id FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let existing = session(&workspace_id)?;
    if existing
        .expires_at
        .is_none_or(|expires_at| expires_at > now_seconds() + 60)
    {
        return Ok(existing);
    }
    let api = client(database)?;
    let response = api.auth_token(
        "refresh_token",
        json!({ "refresh_token": existing.refresh_token }),
    )?;
    if !response.status().is_success() {
        return Err(map_auth_error(response, "AUTH_REQUIRED"));
    }
    let auth: AuthResponse = response
        .json()
        .map_err(|error| format!("AUTH_REQUIRED: Supabase 刷新响应无效: {error}"))?;
    let refreshed = CloudSession {
        user_id: auth
            .user
            .as_ref()
            .map(|user| user.id.clone())
            .unwrap_or(existing.user_id),
        email: auth.user.and_then(|user| user.email).or(existing.email),
        access_token: auth.access_token,
        refresh_token: auth.refresh_token,
        expires_at: auth.expires_in.map(|seconds| now_seconds() + seconds),
    };
    save_session(database, &refreshed)?;
    Ok(refreshed)
}

fn session(workspace_id: &str) -> Result<CloudSession, String> {
    if let Some(cached) = cached_session(workspace_id) {
        return Ok(cached);
    }
    let entry = keyring::Entry::new("timegenie", &format!("{SESSION_PROVIDER}:{workspace_id}"))
        .map_err(|error| format!("AUTH_REQUIRED: 无法访问系统凭据库: {error}"))?;
    let serialized = entry.get_password().map_err(|error| {
        format!("AUTH_REQUIRED: 尚未登录 Supabase（本机凭据读取失败: {error}）")
    })?;
    let session: CloudSession = serde_json::from_str(&serialized)
        .map_err(|error| format!("AUTH_REQUIRED: Supabase 会话损坏: {error}"))?;
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
    let entry = keyring::Entry::new("timegenie", &format!("{SESSION_PROVIDER}:{workspace_id}"))
        .map_err(|error| format!("无法访问系统凭据库: {error}"))?;
    let serialized = serde_json::to_string(session).map_err(|error| error.to_string())?;
    entry
        .set_password(&serialized)
        .map_err(|error| format!("保存 Supabase 会话失败: {error}"))?;
    cache_session(&workspace_id, session)
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

fn session_snapshot(session: &CloudSession) -> CloudSessionSnapshot {
    CloudSessionSnapshot {
        signed_in: true,
        user_id: Some(session.user_id.clone()),
        email: session.email.clone(),
        expires_at: session.expires_at,
    }
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

fn classify_http_error(status: StatusCode, body: &str, fallback: &str) -> String {
    let parsed = serde_json::from_str::<Value>(body).ok();
    let message = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|json| {
            json.get("msg")
                .or_else(|| json.get("message"))
                .or_else(|| json.get("error_description"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| body.chars().take(240).collect::<String>());
    let remote_code = parsed
        .as_ref()
        .and_then(|json| json.get("code"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let lower_message = message.to_ascii_lowercase();
    let code = if matches!(remote_code, "PGRST202" | "PGRST205" | "42P01" | "42883")
        || lower_message.contains("could not find the function")
        || lower_message.contains("could not find the table")
        || lower_message.contains("does not exist")
    {
        "CLOUD_SCHEMA_MISSING"
    } else if remote_code == "42501" || lower_message.contains("permission denied") {
        "CLOUD_PERMISSION_DENIED"
    } else if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        "AUTH_FAILED"
    } else if status == StatusCode::CONFLICT || status == StatusCode::PRECONDITION_FAILED {
        "VERSION_CONFLICT"
    } else {
        fallback
    };
    format!("{code}: {message}")
}

fn local_snapshot(database: &Database, workspace_id: &str) -> Result<Value, String> {
    let connection = database.open()?;
    let mut result = serde_json::Map::new();
    for (table, sql) in [
        ("subjects", "SELECT * FROM subjects WHERE workspace_id = ?1"),
        ("tasks", "SELECT * FROM tasks WHERE workspace_id = ?1 ORDER BY parent_id IS NOT NULL, created_at"),
        ("task_status_events", "SELECT * FROM task_status_events WHERE workspace_id = ?1"),
        ("task_daily_estimates", "SELECT * FROM task_daily_estimates WHERE workspace_id = ?1"),
        ("task_recurrence_rules", "SELECT * FROM task_recurrence_rules WHERE workspace_id = ?1"),
        ("task_occurrences", "SELECT * FROM task_occurrences WHERE workspace_id = ?1"),
        ("work_days", "SELECT * FROM work_days WHERE workspace_id = ?1"),
        ("time_entries", "SELECT * FROM time_entries WHERE workspace_id = ?1"),
        ("time_segments", "SELECT * FROM time_segments WHERE workspace_id = ?1"),
        ("time_allocations", "SELECT * FROM time_allocations WHERE workspace_id = ?1"),
        ("unassigned_sessions", "SELECT * FROM unassigned_sessions WHERE workspace_id = ?1"),
        ("unassigned_segments", "SELECT * FROM unassigned_segments WHERE workspace_id = ?1"),
        ("report_templates", "SELECT * FROM report_templates WHERE workspace_id = ?1"),
        ("reports", "SELECT * FROM reports WHERE workspace_id = ?1"),
        ("report_tasks", "SELECT * FROM report_tasks WHERE workspace_id = ?1"),
        ("app_settings", "SELECT * FROM app_settings WHERE workspace_id = ?1"),
        ("integration_configs", "SELECT * FROM integration_configs WHERE workspace_id = ?1"),
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
pub fn storage_migration_preview(
    database: tauri::State<'_, Database>,
    request: MigrationPreviewRequest,
) -> Result<MigrationPreview, String> {
    migration_preview(&database, request)
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
    use tempfile::tempdir;

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
    fn supabase_http_errors_distinguish_schema_and_permission_failures() {
        let missing = classify_http_error(
            StatusCode::NOT_FOUND,
            r#"{"code":"PGRST202","message":"Could not find the function timegenie.cloud_snapshot_get"}"#,
            "CLOUD_REQUEST_FAILED",
        );
        assert!(missing.starts_with("CLOUD_SCHEMA_MISSING:"));

        let denied = classify_http_error(
            StatusCode::BAD_REQUEST,
            r#"{"code":"42501","message":"permission denied for table workspaces"}"#,
            "CLOUD_REQUEST_FAILED",
        );
        assert!(denied.starts_with("CLOUD_PERMISSION_DENIED:"));
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
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("pending-cloud-switch.sqlite3")).unwrap();
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
                "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, state, attempt_count, created_at)
                 VALUES (?1, ?2, ?3, 'task_update', 'task', 'task-1', 1, '{}', 'pending', 0, 1)",
                rusqlite::params![Uuid::now_v7().to_string(), cloud_workspace_id, device_id],
            )
            .unwrap();

        let error = ensure_no_pending_cloud_work(&database).unwrap_err();
        assert!(error.starts_with("OFFLINE_RESTRICTED:"));
        let snapshot = storage_mode(&database).unwrap();
        assert_eq!(snapshot.mode, "cloud");
        assert_eq!(snapshot.pending_operations, 1);
    }

    #[test]
    fn cloud_configuration_change_is_blocked_until_outbox_is_clean() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("pending-config-switch.sqlite3"))
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
                "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, base_version, payload_json, state, attempt_count, created_at)
                 VALUES (?1, ?2, ?3, 'task_update', 'task', 'task-1', 1, '{}', 'pending', 0, 1)",
                rusqlite::params![Uuid::now_v7().to_string(), cloud_workspace_id, device_id],
            )
            .unwrap();

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
        assert!(error.starts_with("OFFLINE_RESTRICTED:"));
        let error = ensure_can_replace_cloud_configuration(
            &database,
            "https://old-project.supabase.co",
            "new-anon-key",
        )
        .unwrap_err();
        assert!(error.starts_with("OFFLINE_RESTRICTED:"));
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
