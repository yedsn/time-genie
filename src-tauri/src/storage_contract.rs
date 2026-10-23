use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::database::Database;

pub const STORAGE_BOUNDARIES_JSON: &str = include_str!("../storage-boundaries.json");
pub const LEGACY_SYNC_PROTOCOL: &str = "sync_v1";
pub const DIRECT_CLOUD_PROTOCOL: &str = "direct_v2";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageBackend {
    LocalSqlite,
    LegacySyncSqlite,
    DirectSupabase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BusinessDomain {
    Tasks,
    Planning,
    Time,
    Reports,
    SharedSettingsIntegrations,
    DataProtection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageOperation {
    Query,
    Mutation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainStorageError {
    pub code: String,
    pub message: String,
    pub domain: BusinessDomain,
    pub operation: String,
}

impl DomainStorageError {
    pub fn direct_adapter_pending(domain: BusinessDomain, operation: StorageOperation) -> Self {
        Self {
            code: "DIRECT_CLOUD_ADAPTER_PENDING".to_string(),
            message: "当前业务领域尚未完成 Supabase 直连适配".to_string(),
            domain,
            operation: operation_name(operation).to_string(),
        }
    }
}

impl std::fmt::Display for DomainStorageError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

fn operation_name(operation: StorageOperation) -> &'static str {
    match operation {
        StorageOperation::Query => "query",
        StorageOperation::Mutation => "mutation",
    }
}

pub fn storage_backend(database: &Database) -> Result<StorageBackend, String> {
    storage_backend_from_connection(&database.open()?)
}

pub fn storage_backend_from_connection(connection: &Connection) -> Result<StorageBackend, String> {
    let mode =
        read_device_string(connection, "storage_mode")?.unwrap_or_else(|| "local".to_string());
    if mode != "cloud" {
        return Ok(StorageBackend::LocalSqlite);
    }
    let protocol = read_device_string(connection, "cloud_storage_protocol")?
        .unwrap_or_else(|| LEGACY_SYNC_PROTOCOL.to_string());
    match protocol.as_str() {
        LEGACY_SYNC_PROTOCOL => Ok(StorageBackend::LegacySyncSqlite),
        DIRECT_CLOUD_PROTOCOL => Ok(StorageBackend::DirectSupabase),
        _ => Err(format!(
            "CLOUD_PROTOCOL_UNSUPPORTED: 不支持的云端存储协议 {protocol}"
        )),
    }
}

pub fn route<T>(
    database: &Database,
    domain: BusinessDomain,
    operation: StorageOperation,
    sqlite: impl FnOnce(&Database) -> Result<T, String>,
    direct: impl FnOnce(&Database) -> Result<T, String>,
) -> Result<T, String> {
    match storage_backend(database)? {
        StorageBackend::LocalSqlite | StorageBackend::LegacySyncSqlite => sqlite(database),
        StorageBackend::DirectSupabase => direct(database).map_err(|error| {
            if error.trim().is_empty() {
                DomainStorageError::direct_adapter_pending(domain, operation).to_string()
            } else {
                error
            }
        }),
    }
}

pub fn direct_adapter_pending<T>(
    domain: BusinessDomain,
    operation: StorageOperation,
) -> Result<T, String> {
    Err(DomainStorageError::direct_adapter_pending(domain, operation).to_string())
}

pub fn business_data_fingerprint(database: &Database) -> Result<String, String> {
    let manifest: Value = serde_json::from_str(STORAGE_BOUNDARIES_JSON)
        .map_err(|error| format!("存储边界清单无效: {error}"))?;
    let tables = manifest
        .pointer("/sqliteTables/localBusiness")
        .and_then(Value::as_array)
        .ok_or_else(|| "存储边界清单缺少 localBusiness".to_string())?;
    let connection = database.open()?;
    let mut parts = Vec::with_capacity(tables.len() + 2);
    for table in tables {
        let table = table
            .as_str()
            .ok_or_else(|| "localBusiness 表名无效".to_string())?;
        let count: i64 = connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .map_err(|error| format!("读取 {table} 失败: {error}"))?;
        parts.push(format!("{table}:{count}"));
    }
    for table in ["sync_outbox", "local_sync_state"] {
        let count: i64 = connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .map_err(|error| format!("读取 {table} 失败: {error}"))?;
        parts.push(format!("{table}:{count}"));
    }
    Ok(parts.join("|"))
}

fn read_device_string(connection: &Connection, key: &str) -> Result<Option<String>, String> {
    use rusqlite::OptionalExtension;
    connection
        .query_row(
            "SELECT json_extract(value_json, '$') FROM device_settings WHERE key=?1",
            [key],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use tempfile::tempdir;

    fn test_database() -> Database {
        let directory = tempdir().unwrap();
        let path = directory.path().join("storage-contract.sqlite3");
        std::mem::forget(directory);
        Database::initialize_at(path).unwrap()
    }

    fn set_device_value(database: &Database, key: &str, value: &str) {
        database
            .open()
            .unwrap()
            .execute(
                "INSERT INTO device_settings(key,value_json,updated_at) VALUES(?1,?2,1) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json",
                params![key, serde_json::to_string(value).unwrap()],
            )
            .unwrap();
    }

    #[test]
    fn routes_local_and_legacy_cloud_to_sqlite_adapter() {
        let database = test_database();
        let local = route(
            &database,
            BusinessDomain::Tasks,
            StorageOperation::Query,
            |_| Ok("sqlite"),
            |_| Ok("cloud"),
        )
        .unwrap();
        assert_eq!(local, "sqlite");
        set_device_value(&database, "storage_mode", "cloud");
        let legacy = route(
            &database,
            BusinessDomain::Tasks,
            StorageOperation::Mutation,
            |_| Ok("sqlite"),
            |_| Ok("cloud"),
        )
        .unwrap();
        assert_eq!(legacy, "sqlite");
    }

    #[test]
    fn direct_cloud_route_does_not_execute_sqlite_adapter_or_change_local_tables() {
        let database = test_database();
        set_device_value(&database, "storage_mode", "cloud");
        set_device_value(&database, "cloud_storage_protocol", DIRECT_CLOUD_PROTOCOL);
        let before = business_data_fingerprint(&database).unwrap();
        let result = route(
            &database,
            BusinessDomain::Tasks,
            StorageOperation::Mutation,
            |database| {
                database
                    .open()?
                    .execute("DELETE FROM tasks", [])
                    .map_err(|error| error.to_string())?;
                Ok(())
            },
            |_| direct_adapter_pending(BusinessDomain::Tasks, StorageOperation::Mutation),
        );
        assert!(result
            .unwrap_err()
            .starts_with("DIRECT_CLOUD_ADAPTER_PENDING:"));
        assert_eq!(before, business_data_fingerprint(&database).unwrap());
    }

    #[test]
    fn direct_cloud_query_does_not_execute_sqlite_adapter() {
        let database = test_database();
        set_device_value(&database, "storage_mode", "cloud");
        set_device_value(&database, "cloud_storage_protocol", DIRECT_CLOUD_PROTOCOL);
        let before = business_data_fingerprint(&database).unwrap();
        let result = route(
            &database,
            BusinessDomain::Reports,
            StorageOperation::Query,
            |database| {
                database
                    .open()?
                    .execute("DELETE FROM reports", [])
                    .map_err(|error| error.to_string())?;
                Ok(())
            },
            |_| direct_adapter_pending(BusinessDomain::Reports, StorageOperation::Query),
        );
        assert!(result
            .unwrap_err()
            .starts_with("DIRECT_CLOUD_ADAPTER_PENDING:"));
        assert_eq!(before, business_data_fingerprint(&database).unwrap());
    }

    fn configure_cloud_context(database: &Database, protocol: &str) -> (String, String) {
        set_device_value(database, "storage_mode", "cloud");
        set_device_value(database, "cloud_storage_protocol", protocol);
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        drop(connection);
        set_device_value(database, "cloud_workspace_id", &workspace_id);
        (workspace_id, device_id)
    }

    #[test]
    fn storage_snapshot_maps_local_and_direct_cloud_access_states() {
        let database = test_database();
        let local = crate::supabase::storage_mode_from_connection(&database.open().unwrap(), false)
            .unwrap();
        assert_eq!(local.cloud_access_state, "local_writable");
        assert!(local.cloud_writable);
        assert_eq!(local.storage_backend, "local_sqlite");

        configure_cloud_context(&database, DIRECT_CLOUD_PROTOCOL);
        let online =
            crate::supabase::storage_mode_from_connection(&database.open().unwrap(), true).unwrap();
        assert_eq!(online.cloud_access_state, "connecting");
        assert!(!online.cloud_writable);
        assert_eq!(online.storage_backend, "direct_supabase");

        let offline =
            crate::supabase::storage_mode_from_connection(&database.open().unwrap(), false)
                .unwrap();
        assert_eq!(offline.cloud_access_state, "offline_readonly");
        assert!(!offline.cloud_writable);
        assert!(offline.read_only_reason.is_some());
    }

    #[test]
    fn storage_snapshot_maps_reauthentication_and_protocol_incompatibility() {
        let database = test_database();
        let (workspace_id, device_id) = configure_cloud_context(&database, DIRECT_CLOUD_PROTOCOL);
        database
            .open()
            .unwrap()
            .execute(
                "INSERT INTO local_sync_state(workspace_id,device_id,last_change_seq,auth_blocked,auth_blocked_reason,workspace_epoch) VALUES(?1,?2,0,1,'device_revoked',1)",
                params![workspace_id, device_id],
            )
            .unwrap();
        let reauth =
            crate::supabase::storage_mode_from_connection(&database.open().unwrap(), true).unwrap();
        assert_eq!(reauth.cloud_access_state, "reauth_required");
        assert!(!reauth.cloud_writable);

        set_device_value(&database, "cloud_storage_protocol", "future_v9");
        let incompatible =
            crate::supabase::storage_mode_from_connection(&database.open().unwrap(), true).unwrap();
        assert_eq!(incompatible.cloud_access_state, "protocol_incompatible");
        assert!(!incompatible.cloud_writable);
    }

    #[test]
    fn manifest_covers_initialized_sqlite_tables() {
        let database = test_database();
        let manifest: Value = serde_json::from_str(STORAGE_BOUNDARIES_JSON).unwrap();
        let mut classified = manifest["sqliteTables"]
            .as_object()
            .unwrap()
            .values()
            .flat_map(|tables| tables.as_array().unwrap())
            .map(|table| table.as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        classified.sort();
        let connection = database.open().unwrap();
        let mut statement = connection
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .unwrap();
        let actual = statement
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(classified, actual);
        assert!(manifest["sqliteTables"]["cloudBusiness"]
            .as_array()
            .unwrap()
            .is_empty());
    }
}
