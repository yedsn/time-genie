use std::collections::BTreeMap;

use keyring::Entry;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::database::Database;

const KEYRING_SERVICE: &str = "timegenie";
const SUPABASE_SESSION_ACCOUNT: &str = "supabase:session";

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SettingsUpdate {
    pub scope: SettingsScope,
    pub key: String,
    pub value: Value,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum SettingsScope {
    Shared,
    Device,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    pub storage_mode: String,
    pub workspace_id: String,
    pub device_id: String,
    pub shared: BTreeMap<String, Value>,
    pub device: BTreeMap<String, Value>,
    pub integrations: BTreeMap<String, IntegrationConfigSnapshot>,
    pub secrets: SecretPresence,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationConfigSnapshot {
    pub enabled: bool,
    pub config: Value,
    pub has_secret: bool,
    pub version: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationConfigRequest {
    pub provider: String,
    pub enabled: bool,
    pub config: Value,
    pub expected_version: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretPresence {
    pub seatable_token_set: bool,
    pub supabase_session_set: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationSecretRequest {
    pub provider: SecretProvider,
    pub value: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SecretProvider {
    Seatable,
    Supabase,
}

impl SecretProvider {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Seatable => "seatable",
            Self::Supabase => "supabase",
        }
    }
}

pub fn get_settings(database: &Database) -> Result<SettingsSnapshot, String> {
    let connection = database.open()?;
    let workspace_id: String = connection
        .query_row(
            "SELECT id FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let device_id: String = connection
        .query_row(
            "SELECT id FROM devices WHERE workspace_id = ?1 AND revoked_at IS NULL ORDER BY created_at LIMIT 1",
            [&workspace_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let storage_mode = read_device_value(&connection, "storage_mode")?
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| "local".to_string());
    let shared = read_settings(&connection, "app_settings", Some(&workspace_id))?;
    let device = read_settings(&connection, "device_settings", None)?;
    let integrations = read_integration_configs(&connection, &workspace_id)?;
    let secrets = SecretPresence {
        seatable_token_set: secret_exists("seatable", &workspace_id),
        supabase_session_set: secret_exists("supabase", &workspace_id)
            || credential_account_exists(SUPABASE_SESSION_ACCOUNT),
    };
    Ok(SettingsSnapshot {
        storage_mode,
        workspace_id,
        device_id,
        shared,
        device,
        integrations,
        secrets,
    })
}

pub fn update_setting(
    database: &Database,
    request: SettingsUpdate,
) -> Result<SettingsSnapshot, String> {
    update_setting_with_cloud_operation(database, request, None)
}

fn update_setting_with_cloud_operation(
    database: &Database,
    request: SettingsUpdate,
    cloud_state: Option<&crate::supabase::StorageModeSnapshot>,
) -> Result<SettingsSnapshot, String> {
    validate_setting_key(&request.scope, &request.key)?;
    validate_setting_value(&request.key, &request.value)?;
    let mut connection = database.open()?;
    let workspace_id: String = connection
        .query_row(
            "SELECT id FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let serialized = serde_json::to_string(&request.value).map_err(|error| error.to_string())?;
    let now = now_millis();
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    match request.scope {
        SettingsScope::Shared => {
            let base_version: Option<i64> = transaction
                .query_row(
                    "SELECT version FROM app_settings WHERE workspace_id = ?1 AND key = ?2",
                    params![workspace_id, request.key],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| error.to_string())?;
            transaction
                .execute(
                    "INSERT INTO app_settings(workspace_id, key, value_json, updated_at, version)
                     VALUES (?1, ?2, ?3, ?4, 1)
                     ON CONFLICT(workspace_id, key) DO UPDATE SET value_json = excluded.value_json, updated_at = excluded.updated_at, version = app_settings.version + 1",
                    params![workspace_id, request.key, serialized, now],
                )
                .map_err(|error| error.to_string())?;
            if request.key == "timezone" {
                let timezone = request
                    .value
                    .as_str()
                    .ok_or_else(|| "工作空间时区必须是 IANA 时区名称".to_string())?;
                transaction
                    .execute(
                        "UPDATE workspaces SET timezone=?1,updated_at=?2,version=version+1 WHERE id=?3",
                        params![timezone, now, workspace_id],
                    )
                    .map_err(|error| error.to_string())?;
            }
            if let Some(state) = cloud_state {
                crate::cloud_sync::enqueue_entity_in_transaction(
                    &transaction,
                    state,
                    "app_setting_update",
                    "app_setting",
                    Some(&request.key),
                    base_version,
                    None,
                )?;
            }
        }
        SettingsScope::Device => {
            transaction
                .execute(
                    "INSERT INTO device_settings(key, value_json, updated_at)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json, updated_at = excluded.updated_at",
                    params![request.key, serialized, now],
                )
                .map_err(|error| error.to_string())?;
        }
    }
    transaction.commit().map_err(|error| error.to_string())?;
    get_settings(database)
}

fn set_integration_secret_with_cloud_operation(
    database: &Database,
    request: IntegrationSecretRequest,
    cloud_state: Option<&crate::supabase::StorageModeSnapshot>,
) -> Result<SettingsSnapshot, String> {
    if request.value.trim().is_empty() {
        return Err("凭据不能为空".to_string());
    }
    let mut connection = database.open()?;
    let workspace_id: String = connection
        .query_row(
            "SELECT id FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let provider = request.provider.as_str();
    let entry = credential_entry(provider, &workspace_id)?;
    entry
        .set_password(request.value.trim())
        .map_err(|error| format!("保存 {provider} 凭据失败: {error}"))?;
    let now = now_millis();
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let base_version: Option<i64> = transaction
        .query_row(
            "SELECT version FROM integration_configs WHERE workspace_id = ?1 AND provider = ?2",
            params![workspace_id, provider],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO integration_configs(workspace_id, provider, enabled, config_json, secret_ref, updated_at, version)
             VALUES (?1, ?2, 1, '{}', ?3, ?4, 1)
             ON CONFLICT(workspace_id, provider) DO UPDATE SET enabled = 1, secret_ref = excluded.secret_ref, updated_at = excluded.updated_at, version = integration_configs.version + 1",
            params![workspace_id, provider, format!("keyring://{provider}"), now],
        )
        .map_err(|error| error.to_string())?;
    if let Some(state) = cloud_state {
        crate::cloud_sync::enqueue_entity_in_transaction(
            &transaction,
            state,
            "integration_secret_set",
            "integration_config",
            Some(provider),
            base_version,
            None,
        )?;
    }
    transaction.commit().map_err(|error| error.to_string())?;
    get_settings(database)
}

#[cfg(test)]
pub fn update_integration_config(
    database: &Database,
    request: IntegrationConfigRequest,
) -> Result<SettingsSnapshot, String> {
    update_integration_config_with_cloud_operation(database, request, None)
}

fn update_integration_config_with_cloud_operation(
    database: &Database,
    request: IntegrationConfigRequest,
    cloud_state: Option<&crate::supabase::StorageModeSnapshot>,
) -> Result<SettingsSnapshot, String> {
    if !matches!(request.provider.as_str(), "seatable") {
        return Err(format!("不支持的共享集成: {}", request.provider));
    }
    reject_sensitive_json(&request.config, "config")?;
    let serialized = serde_json::to_string(&request.config).map_err(|error| error.to_string())?;
    let mut connection = database.open()?;
    let workspace_id: String = connection
        .query_row(
            "SELECT id FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let base_version: Option<i64> = transaction
        .query_row(
            "SELECT version FROM integration_configs WHERE workspace_id = ?1 AND provider = ?2",
            params![workspace_id, request.provider],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if let Some(expected_version) = request.expected_version {
        let changed = transaction
            .execute(
                "UPDATE integration_configs
                 SET enabled = ?1, config_json = ?2, updated_at = ?3, version = version + 1
                 WHERE workspace_id = ?4 AND provider = ?5 AND version = ?6",
                params![
                    request.enabled as i64,
                    serialized,
                    now_millis(),
                    workspace_id,
                    request.provider,
                    expected_version
                ],
            )
            .map_err(|error| error.to_string())?;
        if changed == 0 {
            return Err("VERSION_CONFLICT: 集成配置已在其他窗口或设备更新".to_string());
        }
    } else {
        transaction
            .execute(
                "INSERT INTO integration_configs(workspace_id, provider, enabled, config_json, secret_ref, updated_at, version)
                 VALUES (?1, ?2, ?3, ?4, NULL, ?5, 1)
                 ON CONFLICT(workspace_id, provider) DO UPDATE SET enabled = excluded.enabled, config_json = excluded.config_json, updated_at = excluded.updated_at, version = integration_configs.version + 1",
                params![workspace_id, request.provider, request.enabled as i64, serialized, now_millis()],
            )
            .map_err(|error| error.to_string())?;
    }
    if let Some(state) = cloud_state {
        crate::cloud_sync::enqueue_entity_in_transaction(
            &transaction,
            state,
            "integration_config_update",
            "integration_config",
            Some(&request.provider),
            request.expected_version.or(base_version),
            None,
        )?;
    }
    transaction.commit().map_err(|error| error.to_string())?;
    get_settings(database)
}

fn clear_integration_secret_with_cloud_operation(
    database: &Database,
    provider: SecretProvider,
    cloud_state: Option<&crate::supabase::StorageModeSnapshot>,
) -> Result<SettingsSnapshot, String> {
    let mut connection = database.open()?;
    let workspace_id: String = connection
        .query_row(
            "SELECT id FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let provider_name = provider.as_str();
    let entry = credential_entry(provider_name, &workspace_id)?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(error) => return Err(format!("删除 {provider_name} 凭据失败: {error}")),
    }
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let base_version: Option<i64> = transaction
        .query_row(
            "SELECT version FROM integration_configs WHERE workspace_id = ?1 AND provider = ?2",
            params![workspace_id, provider_name],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let changed = transaction
        .execute(
            "UPDATE integration_configs SET secret_ref = NULL, enabled = 0, updated_at = ?1, version = version + 1
             WHERE workspace_id = ?2 AND provider = ?3",
            params![now_millis(), workspace_id, provider_name],
        )
        .map_err(|error| error.to_string())?;
    if changed > 0 {
        if let Some(state) = cloud_state {
            crate::cloud_sync::enqueue_entity_in_transaction(
                &transaction,
                state,
                "integration_secret_clear",
                "integration_config",
                Some(provider_name),
                base_version,
                None,
            )?;
        }
    }
    transaction.commit().map_err(|error| error.to_string())?;
    get_settings(database)
}

fn read_settings(
    connection: &rusqlite::Connection,
    table: &str,
    workspace_id: Option<&str>,
) -> Result<BTreeMap<String, Value>, String> {
    let mut settings = BTreeMap::new();
    let sql = match table {
        "app_settings" => {
            "SELECT key, value_json FROM app_settings WHERE workspace_id = ?1 ORDER BY key"
        }
        "device_settings" => "SELECT key, value_json FROM device_settings ORDER BY key",
        _ => return Err("不支持的设置范围".to_string()),
    };
    let mut statement = connection.prepare(sql).map_err(|error| error.to_string())?;
    let rows = if let Some(workspace_id) = workspace_id {
        statement
            .query_map([workspace_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?
    } else {
        statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?
    };
    for (key, value) in rows {
        let parsed = serde_json::from_str(&value)
            .map_err(|error| format!("设置 {key} 不是合法 JSON: {error}"))?;
        settings.insert(key, parsed);
    }
    Ok(settings)
}

fn read_device_value(
    connection: &rusqlite::Connection,
    key: &str,
) -> Result<Option<Value>, String> {
    let value: Option<String> = connection
        .query_row(
            "SELECT value_json FROM device_settings WHERE key = ?1",
            [key],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    value
        .map(|value| serde_json::from_str(&value).map_err(|error| error.to_string()))
        .transpose()
}

fn read_integration_configs(
    connection: &rusqlite::Connection,
    workspace_id: &str,
) -> Result<BTreeMap<String, IntegrationConfigSnapshot>, String> {
    let mut statement = connection
        .prepare(
            "SELECT provider, enabled, config_json, secret_ref IS NOT NULL, version
             FROM integration_configs WHERE workspace_id = ?1 ORDER BY provider",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([workspace_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)? != 0,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)? != 0,
                row.get::<_, i64>(4)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let mut integrations = BTreeMap::new();
    for (provider, enabled, config_json, has_secret, version) in rows {
        integrations.insert(
            provider,
            IntegrationConfigSnapshot {
                enabled,
                config: serde_json::from_str(&config_json).map_err(|error| error.to_string())?,
                has_secret,
                version,
            },
        );
    }
    Ok(integrations)
}

fn reject_sensitive_json(value: &Value, path: &str) -> Result<(), String> {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let normalized = key.to_ascii_lowercase();
                if [
                    "token",
                    "password",
                    "secret",
                    "authorization",
                    "apikey",
                    "api_key",
                ]
                .iter()
                .any(|sensitive| normalized.contains(sensitive))
                {
                    return Err(format!("敏感字段 {path}.{key} 必须保存到系统凭据库"));
                }
                reject_sensitive_json(child, &format!("{path}.{key}"))?;
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                reject_sensitive_json(child, &format!("{path}[{index}]"))?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_setting_key(scope: &SettingsScope, key: &str) -> Result<(), String> {
    let allowed = match scope {
        SettingsScope::Shared => [
            "user_name",
            "timezone",
            "default_subject_id",
            "unassigned_prompt_seconds",
            "default_work_period_text",
            "salary_hourly_rate",
            "report_duration_format",
        ]
        .as_slice(),
        SettingsScope::Device => [
            "tray_hover_enabled",
            "tray_menu_suppress_hover",
            "start_minimized",
            "completion_feedback_enabled",
            "completion_feedback_sound_enabled",
            "app_theme",
            "obsidian_root_path",
            "obsidian_daily_path_pattern",
            "device_name",
            "storage_mode",
            "supabase_project_url",
            "supabase_anon_key",
            "supabase_session_metadata",
            "cloud_workspace_id",
        ]
        .as_slice(),
    };
    if allowed.contains(&key) {
        Ok(())
    } else {
        Err(format!("不允许写入设置键: {key}"))
    }
}

fn validate_setting_value(key: &str, value: &Value) -> Result<(), String> {
    if key == "timezone" {
        let timezone = value
            .as_str()
            .ok_or_else(|| "工作空间时区必须是 IANA 时区名称".to_string())?;
        crate::work_calendar::validate_timezone(timezone)?;
    }
    if matches!(
        key,
        "completion_feedback_enabled" | "completion_feedback_sound_enabled"
    ) && !value.is_boolean()
    {
        return Err("激励反馈设置必须是布尔值".to_string());
    }
    if key == "report_duration_format" && !matches!(value.as_str(), Some("minutes" | "hours")) {
        return Err("报告时长格式只能是 minutes 或 hours".to_string());
    }
    if key == "app_theme"
        && !matches!(
            value.as_str(),
            Some("forest" | "graphite" | "ocean" | "ember" | "paper" | "mist")
        )
    {
        return Err("主题只能是 forest、graphite、ocean、ember、paper 或 mist".to_string());
    }
    Ok(())
}

fn credential_entry(provider: &str, workspace_id: &str) -> Result<Entry, String> {
    Entry::new(KEYRING_SERVICE, &format!("{provider}:{workspace_id}"))
        .map_err(|error| format!("无法访问系统凭据库: {error}"))
}

fn credential_account_exists(account: &str) -> bool {
    let Ok(entry) = Entry::new(KEYRING_SERVICE, account) else {
        return false;
    };
    entry.get_password().is_ok()
}

pub(crate) fn integration_secret(provider: &str, workspace_id: &str) -> Result<String, String> {
    credential_entry(provider, workspace_id)?
        .get_password()
        .map_err(|error| match error {
            keyring::Error::NoEntry => {
                format!("INTEGRATION_NOT_CONFIGURED: {provider} 凭据尚未配置")
            }
            _ => format!("读取 {provider} 凭据失败: {error}"),
        })
}

fn secret_exists(provider: &str, workspace_id: &str) -> bool {
    let Ok(entry) = credential_entry(provider, workspace_id) else {
        return false;
    };
    entry.get_password().is_ok()
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[tauri::command]
pub fn settings_get(database: tauri::State<'_, Database>) -> Result<SettingsSnapshot, String> {
    get_settings(&database)
}

#[tauri::command]
pub fn settings_update(
    database: tauri::State<'_, Database>,
    request: SettingsUpdate,
) -> Result<SettingsSnapshot, String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let state = crate::supabase::storage_mode(&database)?;
    let is_shared = matches!(&request.scope, SettingsScope::Shared);
    let result = update_setting_with_cloud_operation(&database, request, Some(&state))?;
    if is_shared {
        crate::cloud_sync::flush_if_online(&database)?;
    }
    Ok(result)
}

#[tauri::command]
pub fn integration_secret_set(
    database: tauri::State<'_, Database>,
    request: IntegrationSecretRequest,
) -> Result<SettingsSnapshot, String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let state = crate::supabase::storage_mode(&database)?;
    let result = set_integration_secret_with_cloud_operation(&database, request, Some(&state))?;
    crate::cloud_sync::flush_if_online(&database)?;
    Ok(result)
}

#[tauri::command]
pub fn integration_config_update(
    database: tauri::State<'_, Database>,
    request: IntegrationConfigRequest,
) -> Result<SettingsSnapshot, String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let state = crate::supabase::storage_mode(&database)?;
    let result = update_integration_config_with_cloud_operation(&database, request, Some(&state))?;
    crate::cloud_sync::flush_if_online(&database)?;
    Ok(result)
}

#[tauri::command]
pub fn integration_secret_clear(
    database: tauri::State<'_, Database>,
    provider: SecretProvider,
) -> Result<SettingsSnapshot, String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let state = crate::supabase::storage_mode(&database)?;
    let result = clear_integration_secret_with_cloud_operation(&database, provider, Some(&state))?;
    crate::cloud_sync::flush_if_online(&database)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use uuid::Uuid;

    #[test]
    fn workspace_timezone_update_is_validated_and_updates_workspace_metadata() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(directory.path().join("timezone.sqlite3")).unwrap();
        let before: (String, i64) = database
            .open()
            .unwrap()
            .query_row(
                "SELECT timezone, version FROM workspaces WHERE deleted_at IS NULL LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Shared,
                key: "timezone".to_string(),
                value: serde_json::json!("America/New_York"),
            },
        )
        .unwrap();
        let after: (String, i64) = database
            .open()
            .unwrap()
            .query_row(
                "SELECT timezone, version FROM workspaces WHERE deleted_at IS NULL LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(before.0, "Asia/Shanghai");
        assert_eq!(after.0, "America/New_York");
        assert_eq!(after.1, before.1 + 1);

        let error = update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Shared,
                key: "timezone".to_string(),
                value: serde_json::json!("Mars/Olympus"),
            },
        )
        .unwrap_err();
        assert!(error.contains("无效的工作空间时区"));
        let unchanged: String = database
            .open()
            .unwrap()
            .query_row(
                "SELECT timezone FROM workspaces WHERE deleted_at IS NULL LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(unchanged, "America/New_York");
    }

    #[test]
    fn shared_and_device_settings_persist_without_exposing_secrets() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(directory.path().join("test.sqlite3")).unwrap();
        update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Shared,
                key: "salary_hourly_rate".to_string(),
                value: serde_json::json!(88.5),
            },
        )
        .unwrap();
        update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "completion_feedback_sound_enabled".to_string(),
                value: serde_json::json!(false),
            },
        )
        .unwrap();
        update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "obsidian_root_path".to_string(),
                value: serde_json::json!("D:/Obsidian"),
            },
        )
        .unwrap();
        update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "completion_feedback_enabled".to_string(),
                value: serde_json::json!(false),
            },
        )
        .unwrap();
        update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "app_theme".to_string(),
                value: serde_json::json!("ocean"),
            },
        )
        .unwrap();
        let snapshot = get_settings(&database).unwrap();
        assert_eq!(
            snapshot.shared["salary_hourly_rate"],
            serde_json::json!(88.5)
        );
        assert_eq!(
            snapshot.device["obsidian_root_path"],
            serde_json::json!("D:/Obsidian")
        );
        assert_eq!(
            snapshot.device["completion_feedback_enabled"],
            serde_json::json!(false)
        );
        assert_eq!(
            snapshot.device["completion_feedback_sound_enabled"],
            serde_json::json!(false)
        );
        assert_eq!(snapshot.device["app_theme"], serde_json::json!("ocean"));
        update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Shared,
                key: "report_duration_format".to_string(),
                value: serde_json::json!("hours"),
            },
        )
        .unwrap();
        assert_eq!(
            get_settings(&database).unwrap().shared["report_duration_format"],
            serde_json::json!("hours")
        );
        assert!(update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Shared,
                key: "report_duration_format".to_string(),
                value: serde_json::json!("invalid"),
            },
        )
        .is_err());
        assert!(update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "completion_feedback_sound_enabled".to_string(),
                value: serde_json::json!("yes"),
            },
        )
        .is_err());
        assert!(update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "app_theme".to_string(),
                value: serde_json::json!("unknown"),
            },
        )
        .is_err());
        let serialized = serde_json::to_string(&snapshot).unwrap();
        assert!(!serialized.to_ascii_lowercase().contains("refresh_token"));
        assert!(!serialized.to_ascii_lowercase().contains("base_api_token"));
    }

    #[test]
    fn integration_config_rejects_sensitive_values() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(directory.path().join("test.sqlite3")).unwrap();
        let result = update_integration_config(
            &database,
            IntegrationConfigRequest {
                provider: "seatable".to_string(),
                enabled: true,
                config: serde_json::json!({
                    "serverUrl": "https://cloud.seatable.cn",
                    "baseApiToken": "must-not-be-here"
                }),
                expected_version: None,
            },
        );
        assert!(result.is_err());
    }

    #[test]
    fn integration_config_uses_version_check() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(directory.path().join("test.sqlite3")).unwrap();
        let created = update_integration_config(
            &database,
            IntegrationConfigRequest {
                provider: "seatable".to_string(),
                enabled: true,
                config: serde_json::json!({ "serverUrl": "https://cloud.seatable.cn" }),
                expected_version: None,
            },
        )
        .unwrap();
        assert_eq!(created.integrations["seatable"].version, 1);
        let conflict = update_integration_config(
            &database,
            IntegrationConfigRequest {
                provider: "seatable".to_string(),
                enabled: false,
                config: serde_json::json!({ "serverUrl": "https://example.invalid" }),
                expected_version: Some(99),
            },
        );
        assert!(conflict.is_err());
    }

    #[test]
    fn cloud_shared_settings_and_integration_config_are_queued_transactionally() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("settings-cloud.sqlite3")).unwrap();
        let cloud_workspace_id = Uuid::now_v7().to_string();
        update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "cloud_workspace_id".to_string(),
                value: serde_json::json!(cloud_workspace_id),
            },
        )
        .unwrap();
        update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "storage_mode".to_string(),
                value: serde_json::json!("cloud"),
            },
        )
        .unwrap();
        let state = crate::supabase::storage_mode(&database).unwrap();

        update_setting_with_cloud_operation(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Shared,
                key: "salary_hourly_rate".to_string(),
                value: serde_json::json!(120),
            },
            Some(&state),
        )
        .unwrap();
        update_setting_with_cloud_operation(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "obsidian_root_path".to_string(),
                value: serde_json::json!("D:/private-vault"),
            },
            Some(&state),
        )
        .unwrap();
        for (key, value) in [
            ("tray_hover_enabled", serde_json::json!(false)),
            ("tray_menu_suppress_hover", serde_json::json!(false)),
            ("start_minimized", serde_json::json!(true)),
            ("device_name", serde_json::json!("仅本机设备名")),
        ] {
            update_setting_with_cloud_operation(
                &database,
                SettingsUpdate {
                    scope: SettingsScope::Device,
                    key: key.to_string(),
                    value,
                },
                Some(&state),
            )
            .unwrap();
        }
        update_integration_config_with_cloud_operation(
            &database,
            IntegrationConfigRequest {
                provider: "seatable".to_string(),
                enabled: true,
                config: serde_json::json!({ "serverUrl": "https://example.invalid" }),
                expected_version: None,
            },
            Some(&state),
        )
        .unwrap();

        let connection = database.open().unwrap();
        let mut statement = connection
            .prepare(
                "SELECT operation_type, entity_type, entity_id, payload_json FROM sync_outbox ORDER BY created_at, operation_type",
            )
            .unwrap();
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(rows.len(), 2);
        assert!(rows
            .iter()
            .any(|(operation_type, entity_type, entity_id, payload_json)| {
                let payload: Value = serde_json::from_str(payload_json).unwrap();
                operation_type == "app_setting_update"
                    && entity_type == "app_setting"
                    && entity_id.as_deref() == Some("salary_hourly_rate")
                    && payload["value_json"] == serde_json::json!(120)
            }));
        assert!(rows
            .iter()
            .any(|(operation_type, entity_type, entity_id, payload_json)| {
                let payload: Value = serde_json::from_str(payload_json).unwrap();
                operation_type == "integration_config_update"
                    && entity_type == "integration_config"
                    && entity_id.as_deref() == Some("seatable")
                    && payload["config_json"]
                        == serde_json::json!({ "serverUrl": "https://example.invalid" })
            }));
        assert!(!rows
            .iter()
            .any(|(_, _, entity_id, _)| entity_id.as_deref() == Some("obsidian_root_path")));
        for local_key in [
            "tray_hover_enabled",
            "tray_menu_suppress_hover",
            "start_minimized",
            "device_name",
        ] {
            assert!(!rows
                .iter()
                .any(|(_, _, entity_id, _)| entity_id.as_deref() == Some(local_key)));
        }
    }

    #[test]
    fn cloud_integration_secret_state_is_queued_without_secret_material() {
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("settings-cloud-secret.sqlite3"))
                .unwrap();
        let cloud_workspace_id = Uuid::now_v7().to_string();
        update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "cloud_workspace_id".to_string(),
                value: serde_json::json!(cloud_workspace_id),
            },
        )
        .unwrap();
        update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "storage_mode".to_string(),
                value: serde_json::json!("cloud"),
            },
        )
        .unwrap();
        let state = crate::supabase::storage_mode(&database).unwrap();

        set_integration_secret_with_cloud_operation(
            &database,
            IntegrationSecretRequest {
                provider: SecretProvider::Seatable,
                value: "secret-token-that-must-not-sync".to_string(),
            },
            Some(&state),
        )
        .unwrap();
        clear_integration_secret_with_cloud_operation(
            &database,
            SecretProvider::Seatable,
            Some(&state),
        )
        .unwrap();

        let connection = database.open().unwrap();
        let rows = connection
            .prepare(
                "SELECT operation_type, entity_type, entity_id, base_version, payload_json FROM sync_outbox ORDER BY created_at, rowid",
            )
            .unwrap()
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, "integration_secret_set");
        assert_eq!(rows[0].1, "integration_config");
        assert_eq!(rows[0].2.as_deref(), Some("seatable"));
        assert_eq!(rows[0].3, None);
        assert_eq!(rows[1].0, "integration_secret_clear");
        assert_eq!(rows[1].1, "integration_config");
        assert_eq!(rows[1].2.as_deref(), Some("seatable"));
        assert_eq!(rows[1].3, Some(1));

        for (_, _, _, _, payload_json) in rows {
            let payload: Value = serde_json::from_str(&payload_json).unwrap();
            assert_eq!(payload["provider"], "seatable");
            assert!(payload.get("secret_ref").is_none());
            assert!(!payload_json.contains("secret-token-that-must-not-sync"));
            assert!(!payload_json.contains("keyring://"));
        }
    }
}
