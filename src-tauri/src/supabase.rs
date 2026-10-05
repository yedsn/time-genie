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
pub(crate) const CLOUD_SCHEMA: &str = "timegenie";
const CLOUD_BUSINESS_ENTITIES: [&str; 8] = [
    "tasks",
    "task_daily_estimates",
    "task_recurrence_rules",
    "task_occurrences",
    "work_days",
    "time_entries",
    "unassigned_sessions",
    "reports",
];
static SESSION_CACHE: OnceLock<Mutex<HashMap<String, CloudSession>>> = OnceLock::new();
static SESSION_REFRESH_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
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
    pub auth_blocked: bool,
    pub auth_blocked_reason: Option<String>,
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
    Ok(StorageModeSnapshot {
        online: mode == "local" || current_session(database).is_ok(),
        sync_state: if conflicts > 0 {
            "conflict".to_string()
        } else if pending > 0 {
            "pending".to_string()
        } else if last_error.is_some() {
            "error".to_string()
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
        auth_blocked,
        auth_blocked_reason,
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
    let _ = client.rpc_for_database(
        database,
        "workspace_initialize_defaults",
        json!({ "p_workspace_id": workspace.id.clone() }),
        session,
    )?;
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
        (
            "work_days",
            "SELECT COUNT(*) FROM work_days WHERE workspace_id = ?1",
        ),
        (
            "unassigned_sessions",
            "SELECT COUNT(*) FROM unassigned_sessions WHERE workspace_id = ?1",
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
    let workspace = workspace_preview(database)?;
    let snapshot = if let Some(workspace) = &workspace {
        let session = current_session(database)?;
        client(database)?.rpc_for_database(
            database,
            "cloud_snapshot_get",
            json!({ "p_workspace_id": workspace.id.clone() }),
            &session,
        )?
    } else {
        json!({})
    };
    let cloud_counts = [
        "subjects",
        "tasks",
        "time_entries",
        "time_allocations",
        "reports",
        "task_daily_estimates",
        "task_recurrence_rules",
        "task_occurrences",
        "work_days",
        "unassigned_sessions",
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
    let cloud_has_business_data = cloud_snapshot_has_business_data(&cloud_counts);
    let conflicts = migration_conflicts(
        &request.direction,
        workspace.is_some(),
        cloud_has_business_data,
    );
    Ok(MigrationPreview {
        direction: request.direction,
        source_workspace_id,
        target_workspace_id: workspace.map(|workspace| workspace.id),
        entities: local_counts,
        can_execute: conflicts.is_empty(),
        conflicts,
    })
}

fn migration_conflicts(
    direction: &str,
    cloud_workspace_exists: bool,
    cloud_has_business_data: bool,
) -> Vec<String> {
    let mut conflicts = Vec::new();
    if direction == "local_to_cloud" && cloud_has_business_data {
        conflicts.push("云端工作空间已有业务数据，请使用现有云端数据，不能覆盖上传".to_string());
    }
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

fn cloud_snapshot_has_business_data(cloud_counts: &HashMap<&str, i64>) -> bool {
    CLOUD_BUSINESS_ENTITIES
        .iter()
        .any(|entity| cloud_counts.get(entity).copied().unwrap_or(0) > 0)
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
            .map_err(|error| format!("NETWORK_ERROR: Supabase 请求失败: {error}"))?;
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
        self.request_for_database(
            database,
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
    current_session_with_refresh(database, false, true)
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
    let lower_message = message.to_ascii_lowercase();
    let planning_operation_missing = lower_message.contains("unsupported_operation")
        && (lower_message.contains("task_daily_estimate")
            || lower_message.contains("task_recurrence_rule"));
    if planning_operation_missing {
        return "CLOUD_SCHEMA_MISSING: 云端数据库版本过旧，请部署 TimeGenie 事项规划同步补丁"
            .to_string();
    }
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
    } else if status.is_server_error()
        || matches!(
            status,
            StatusCode::REQUEST_TIMEOUT | StatusCode::TOO_MANY_REQUESTS
        )
    {
        "NETWORK_ERROR"
    } else {
        fallback
    };
    format!("{code}: {message}")
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
                    let first_line = String::from_utf8_lossy(&request)
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .to_string();
                    assert!(
                        first_line.contains(expected.path),
                        "expected request path {}, got {first_line}",
                        expected.path
                    );
                    thread_hits.lock().unwrap().push(first_line);
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
    fn migration_conflicts_block_upload_when_cloud_has_business_data() {
        let conflicts = migration_conflicts("local_to_cloud", true, true);
        assert_eq!(conflicts.len(), 1);
        assert!(conflicts[0].contains("云端工作空间已有业务数据"));

        let conflicts = migration_conflicts("local_to_cloud", false, false);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn cloud_business_data_detection_includes_local_first_support_entities() {
        for entity in [
            "tasks",
            "task_daily_estimates",
            "task_recurrence_rules",
            "task_occurrences",
            "work_days",
            "time_entries",
            "unassigned_sessions",
            "reports",
        ] {
            let mut counts = HashMap::new();
            counts.insert(entity, 1);
            assert!(
                cloud_snapshot_has_business_data(&counts),
                "{entity} must block migrations that assume an empty cloud workspace"
            );
        }

        let mut non_business_counts = HashMap::new();
        non_business_counts.insert("subjects", 1);
        non_business_counts.insert("report_templates", 1);
        assert!(!cloud_snapshot_has_business_data(&non_business_counts));
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
        assert!(preview.can_execute);
        assert!(preview.conflicts.is_empty());
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
