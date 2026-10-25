use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::database::Database;

pub const STORAGE_BOUNDARIES_JSON: &str = include_str!("../storage-boundaries.json");
// Kept temporarily while the server-side retirement migration removes legacy protocol fields.
pub const LEGACY_SYNC_PROTOCOL: &str = "sync_v1";
pub const DIRECT_CLOUD_PROTOCOL: &str = "direct_v2";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageBackend {
    LocalSqlite,
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
    // Cloud mode is direct Supabase. Legacy and unknown persisted protocol markers are
    // ignored so an old local setting can never route business work back to SQLite.
    Ok(StorageBackend::DirectSupabase)
}

pub fn route<T>(
    database: &Database,
    domain: BusinessDomain,
    operation: StorageOperation,
    sqlite: impl FnOnce(&Database) -> Result<T, String>,
    direct: impl FnOnce(&Database) -> Result<T, String>,
) -> Result<T, String> {
    match storage_backend(database)? {
        StorageBackend::LocalSqlite => sqlite(database),
        StorageBackend::DirectSupabase => direct(database).map_err(|error| {
            if error.trim().is_empty() {
                DomainStorageError::direct_adapter_pending(domain, operation).to_string()
            } else {
                error
            }
        }),
    }
}

pub async fn route_async<T, Local, Direct>(
    database: Database,
    domain: BusinessDomain,
    operation: StorageOperation,
    sqlite: Local,
    direct: Direct,
) -> Result<T, String>
where
    T: Send + 'static,
    Local: FnOnce(&Database) -> Result<T, String>,
    Direct: FnOnce(&Database) -> Result<T, String> + Send + 'static,
{
    match storage_backend(&database)? {
        StorageBackend::LocalSqlite => sqlite(&database),
        StorageBackend::DirectSupabase => {
            crate::cloud_executor::run(move || {
                direct(&database).map_err(|error| {
                    if error.trim().is_empty() {
                        DomainStorageError::direct_adapter_pending(domain, operation).to_string()
                    } else {
                        error
                    }
                })
            })
            .await
        }
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
    let mut parts = Vec::with_capacity(tables.len());
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
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
    fn routes_local_to_sqlite_and_any_cloud_marker_to_direct_adapter() {
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
        let legacy_marker = route(
            &database,
            BusinessDomain::Tasks,
            StorageOperation::Mutation,
            |_| Ok("sqlite"),
            |_| Ok("cloud"),
        )
        .unwrap();
        assert_eq!(legacy_marker, "cloud");
        set_device_value(&database, "cloud_storage_protocol", "future_v9");
        let unknown_marker = route(
            &database,
            BusinessDomain::Tasks,
            StorageOperation::Mutation,
            |_| Ok("sqlite"),
            |_| Ok("cloud"),
        )
        .unwrap();
        assert_eq!(unknown_marker, "cloud");
    }

    #[tokio::test]
    async fn async_route_keeps_local_work_inline_and_runs_cloud_work_in_executor() {
        let database = test_database();
        let before = crate::cloud_executor::metrics().started_count;
        let local = route_async(
            database.clone(),
            BusinessDomain::Tasks,
            StorageOperation::Query,
            |_| Ok("sqlite"),
            |_| Ok("cloud"),
        )
        .await
        .unwrap();
        assert_eq!(local, "sqlite");
        assert_eq!(crate::cloud_executor::metrics().started_count, before);

        set_device_value(&database, "storage_mode", "cloud");
        let cloud = route_async(
            database,
            BusinessDomain::Tasks,
            StorageOperation::Query,
            |_| Ok("sqlite"),
            |_| Ok("cloud"),
        )
        .await
        .unwrap();
        assert_eq!(cloud, "cloud");
        assert_eq!(crate::cloud_executor::metrics().started_count, before + 1);
    }

    #[test]
    fn local_mode_never_dispatches_any_business_domain_to_supabase() {
        let database = test_database();
        let cloud_requests = Arc::new(AtomicUsize::new(0));
        for domain in [
            BusinessDomain::Tasks,
            BusinessDomain::Planning,
            BusinessDomain::Time,
            BusinessDomain::Reports,
            BusinessDomain::SharedSettingsIntegrations,
            BusinessDomain::DataProtection,
        ] {
            for operation in [StorageOperation::Query, StorageOperation::Mutation] {
                let cloud_requests_for_route = Arc::clone(&cloud_requests);
                let result = route(
                    &database,
                    domain,
                    operation,
                    |_| Ok("sqlite"),
                    |_| {
                        cloud_requests_for_route.fetch_add(1, Ordering::SeqCst);
                        Ok("supabase")
                    },
                )
                .unwrap();
                assert_eq!(result, "sqlite");
            }
        }
        assert_eq!(cloud_requests.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn direct_cloud_never_executes_sqlite_adapter_for_any_business_domain_or_operation() {
        let database = test_database();
        set_device_value(&database, "storage_mode", "cloud");
        set_device_value(&database, "cloud_storage_protocol", DIRECT_CLOUD_PROTOCOL);
        let before = business_data_fingerprint(&database).unwrap();
        for domain in [
            BusinessDomain::Tasks,
            BusinessDomain::Planning,
            BusinessDomain::Time,
            BusinessDomain::Reports,
            BusinessDomain::SharedSettingsIntegrations,
            BusinessDomain::DataProtection,
        ] {
            for operation in [StorageOperation::Query, StorageOperation::Mutation] {
                let result = route(
                    &database,
                    domain,
                    operation,
                    |database| {
                        database
                            .open()?
                            .execute("DELETE FROM tasks", [])
                            .map_err(|error| error.to_string())?;
                        Ok("sqlite")
                    },
                    |_| Ok("supabase"),
                )
                .unwrap();
                assert_eq!(result, "supabase");
            }
        }
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
    fn storage_snapshot_ignores_legacy_protocol_marker_without_runtime_tables() {
        let database = test_database();
        configure_cloud_context(&database, DIRECT_CLOUD_PROTOCOL);
        set_device_value(&database, "cloud_storage_protocol", "future_v9");
        assert_eq!(
            storage_backend(&database).unwrap(),
            StorageBackend::DirectSupabase
        );
        let connection = database.open().unwrap();
        let legacy_table_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('sync_outbox','local_sync_state')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(legacy_table_count, 0);
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
