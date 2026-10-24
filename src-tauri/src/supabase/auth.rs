use reqwest::blocking::Client;
use reqwest::StatusCode;
#[cfg(debug_assertions)]
use rusqlite::params;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;

use crate::database::Database;
use crate::settings::{SettingsScope, SettingsUpdate};

use super::*;

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

pub(crate) fn classify_probe_response(status: StatusCode, body: &str) -> CloudProjectProbe {
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
    if let Some(cloud_workspace_id) = cloud_workspace_id.as_deref() {
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
    if let Ok(cache) = crate::cloud_read_cache::CloudReadCache::for_database(database) {
        if let Some(workspace_id) = cloud_workspace_id.as_deref() {
            let _ = cache.clear_identity(&session.user_id, workspace_id);
        }
    }
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

pub(crate) fn cloud_configuration_is_changing(
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
    let connection = database.open()?;
    let device_id: String = connection
        .query_row(
            "SELECT json_extract(value_json, '$') FROM device_settings WHERE key = 'device_id'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let mut checkpoint = read_direct_cloud_checkpoint(&connection, &workspace.id);
    drop(connection);
    checkpoint.device_id = device_id;
    checkpoint.workspace_epoch = workspace.workspace_epoch.max(1);
    write_direct_cloud_checkpoint(database, &checkpoint)?;
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
    let connection = database.open()?;
    let mut checkpoint = read_direct_cloud_checkpoint(&connection, &workspace.id);
    drop(connection);
    checkpoint.change_seq = latest_change_seq;
    write_direct_cloud_checkpoint(database, &checkpoint)?;
    Ok(CloudWorkspace {
        id: workspace.id,
        name: workspace.name,
        timezone: workspace.timezone,
        latest_change_seq,
    })
}

pub(crate) fn is_legacy_report_template_bootstrap_error(error: &str) -> bool {
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
                    "{}/rest/v1/workspaces?owner_user_id=eq.{}&deleted_at=is.null&select=id,name,timezone,workspace_epoch&limit=1",
                    client.project_url, session.user_id
                )),
            session,
        )?
        .json()
        .map_err(|error| format!("NETWORK_ERROR: 读取 Supabase 工作空间失败: {error}"))?;
    Ok(workspaces.into_iter().next())
}

pub(crate) fn workspace_preview(database: &Database) -> Result<Option<CloudWorkspace>, String> {
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
    let workspace = workspace_bootstrap(database)?;
    register_device_for_workspace(database, &workspace.id, request)
}

pub(crate) fn register_device_for_workspace(
    database: &Database,
    workspace_id: &str,
    request: CloudDeviceRegisterRequest,
) -> Result<CloudDevice, String> {
    let session = current_session(database)?;
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
            "p_workspace_id": workspace_id,
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

pub(crate) fn cloud_device_from_row(row: DeviceRow, current_device_id: &str) -> CloudDevice {
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

#[cfg(debug_assertions)]
pub(crate) fn dev_cloud_session_from_env() -> Option<CloudSession> {
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
    transaction.commit().map_err(|error| error.to_string())?;
    write_direct_cloud_checkpoint(
        database,
        &DirectCloudCheckpoint {
            workspace_id: workspace_id.to_string(),
            device_id,
            workspace_epoch: 1,
            ..DirectCloudCheckpoint::default()
        },
    )
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

pub(crate) fn refresh_current_session(database: &Database) -> Result<CloudSession, String> {
    current_session_with_refresh(database, true, false)
}

pub(crate) fn current_session_with_refresh(
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

pub(crate) fn should_keep_existing_session_after_refresh_error(error: &str) -> bool {
    should_preserve_saved_session(error)
}

fn should_preserve_saved_session(error: &str) -> bool {
    error.starts_with("NETWORK_ERROR:")
        || error.starts_with("CREDENTIAL_UNAVAILABLE:")
        || error.starts_with("AUTH_UNVERIFIED:")
}

pub(crate) fn is_business_request_auth_error(error: &str) -> bool {
    error.starts_with("AUTH_FAILED:")
}

pub(crate) fn is_definitive_auth_invalid(error: &str) -> bool {
    error.starts_with("AUTH_INVALID:") || error.starts_with("DEVICE_REVOKED:")
}

pub(crate) fn session(workspace_id: &str) -> Result<CloudSession, String> {
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

pub(crate) fn save_session(database: &Database, session: &CloudSession) -> Result<(), String> {
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

pub(crate) fn session_account_candidates(workspace_id: &str) -> [String; 2] {
    [
        SESSION_ACCOUNT.to_string(),
        format!("{SESSION_PROVIDER}:{workspace_id}"),
    ]
}

pub(crate) fn read_session_credential(account: &str) -> Result<String, String> {
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

pub(crate) fn write_session_credential(account: &str, serialized: &str) -> Result<(), String> {
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

pub(crate) fn cached_session(workspace_id: &str) -> Option<CloudSession> {
    SESSION_CACHE
        .get()
        .and_then(|cache| cache.lock().ok())
        .and_then(|cache| cache.get(workspace_id).cloned())
}

pub(crate) fn cache_session(workspace_id: &str, session: &CloudSession) -> Result<(), String> {
    SESSION_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| "AUTH_REQUIRED: Supabase 会话缓存不可用".to_string())?
        .insert(workspace_id.to_string(), session.clone());
    Ok(())
}

pub(crate) fn remove_cached_session(workspace_id: &str) {
    if let Some(cache) = SESSION_CACHE.get() {
        if let Ok(mut cache) = cache.lock() {
            cache.remove(workspace_id);
        }
    }
}

#[cfg(test)]
#[cfg(test)]
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

#[cfg(test)]
pub(crate) fn install_serialized_test_session_for_database(
    database: &Database,
    serialized: &str,
) -> Result<(), String> {
    let session: CloudSession = serde_json::from_str(serialized)
        .map_err(|error| format!("AUTH_INVALID: Supabase 测试会话损坏: {error}"))?;
    if session.user_id.trim().is_empty()
        || session.access_token.trim().is_empty()
        || session.refresh_token.trim().is_empty()
    {
        return Err("AUTH_INVALID: Supabase 测试会话缺少必要字段".to_string());
    }
    save_session(database, &session)
}

pub(crate) fn session_snapshot(session: &CloudSession) -> CloudSessionSnapshot {
    CloudSessionSnapshot {
        signed_in: true,
        status: "authenticated".to_string(),
        reason: None,
        user_id: Some(session.user_id.clone()),
        email: session.email.clone(),
        expires_at: session.expires_at,
    }
}

pub(crate) fn offline_session_snapshot(
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

pub(crate) fn reauth_required_snapshot(reason: Option<String>) -> CloudSessionSnapshot {
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

pub(crate) fn save_session_metadata(
    database: &Database,
    session: &CloudSession,
) -> Result<(), String> {
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
    let mut checkpoint = read_direct_cloud_checkpoint(&connection, &cloud_workspace_id);
    checkpoint.device_id = state.device_id;
    checkpoint.change_seq = checkpoint.change_seq.max(state.last_change_seq);
    checkpoint.last_error = Some(format!("AUTH_BLOCKED: {reason}"));
    checkpoint.auth_blocked_reason = Some(reason.to_string());
    drop(connection);
    write_direct_cloud_checkpoint(database, &checkpoint)
}

pub(crate) fn clear_auth_blocked(database: &Database) -> Result<(), String> {
    let connection = database.open()?;
    let workspace_id = read_device_setting_string(&connection, "cloud_workspace_id")?;
    let Some(workspace_id) = workspace_id else {
        return Ok(());
    };
    let mut checkpoint = read_direct_cloud_checkpoint(&connection, &workspace_id);
    checkpoint.auth_blocked_reason = None;
    checkpoint.last_error = None;
    drop(connection);
    write_direct_cloud_checkpoint(database, &checkpoint)
}
