use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::database::Database;

const CACHE_FILE_NAME: &str = "cloud-read-cache.sqlite3";
const CACHE_SCHEMA_VERSION: i64 = 1;
const MAX_CACHE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CACHE_AGE_MILLIS: i64 = 30 * 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudReadCacheKey<'a> {
    pub account_id: &'a str,
    pub workspace_id: &'a str,
    pub domain: &'a str,
    pub query_key: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CloudReadCacheSnapshot {
    pub payload: Value,
    pub workspace_epoch: i64,
    pub change_seq: i64,
    pub fetched_at: i64,
}

#[derive(Debug, Clone)]
pub struct CloudReadCache {
    path: PathBuf,
}

impl CloudReadCache {
    pub fn for_database(database: &Database) -> Result<Self, String> {
        Self::at_data_directory(&database.data_directory())
    }

    pub fn at_data_directory(directory: &Path) -> Result<Self, String> {
        Self::initialize_at(directory.join(CACHE_FILE_NAME))
    }

    fn initialize_at(path: PathBuf) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let cache = Self { path };
        if let Err(error) = cache.initialize_schema() {
            cache.rebuild_corrupt_file(&error)?;
            cache.initialize_schema()?;
        }
        Ok(cache)
    }

    pub fn read(
        &self,
        key: &CloudReadCacheKey<'_>,
        now: i64,
    ) -> Result<Option<CloudReadCacheSnapshot>, String> {
        let connection = self.open()?;
        let row: Option<(String, i64, i64, i64, i64)> = connection
            .query_row(
                "SELECT payload_json,workspace_epoch,change_seq,fetched_at,cache_schema_version
                 FROM query_snapshots
                 WHERE account_id=?1 AND workspace_id=?2 AND domain=?3 AND query_key=?4",
                params![key.account_id, key.workspace_id, key.domain, key.query_key],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| error.to_string())?;
        let Some((payload_json, workspace_epoch, change_seq, fetched_at, schema_version)) = row
        else {
            return Ok(None);
        };
        if schema_version != CACHE_SCHEMA_VERSION
            || now.saturating_sub(fetched_at) > MAX_CACHE_AGE_MILLIS
        {
            self.delete_key(&connection, key)?;
            return Ok(None);
        }
        let payload = match serde_json::from_str(&payload_json) {
            Ok(payload) => payload,
            Err(_) => {
                self.delete_key(&connection, key)?;
                return Ok(None);
            }
        };
        connection
            .execute(
                "UPDATE query_snapshots SET last_accessed_at=?5
                 WHERE account_id=?1 AND workspace_id=?2 AND domain=?3 AND query_key=?4",
                params![
                    key.account_id,
                    key.workspace_id,
                    key.domain,
                    key.query_key,
                    now
                ],
            )
            .map_err(|error| error.to_string())?;
        Ok(Some(CloudReadCacheSnapshot {
            payload,
            workspace_epoch,
            change_seq,
            fetched_at,
        }))
    }

    pub fn save(
        &self,
        key: &CloudReadCacheKey<'_>,
        snapshot: &CloudReadCacheSnapshot,
        now: i64,
    ) -> Result<(), String> {
        let sanitized = sanitize_payload(snapshot.payload.clone());
        let payload_json = serde_json::to_string(&sanitized).map_err(|error| error.to_string())?;
        let connection = self.open()?;
        connection
            .execute(
                "INSERT INTO query_snapshots(
                   account_id,workspace_id,domain,query_key,payload_json,workspace_epoch,
                   change_seq,fetched_at,last_accessed_at,cache_schema_version
                 ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
                 ON CONFLICT(account_id,workspace_id,domain,query_key) DO UPDATE SET
                   payload_json=excluded.payload_json,workspace_epoch=excluded.workspace_epoch,
                   change_seq=excluded.change_seq,fetched_at=excluded.fetched_at,
                   last_accessed_at=excluded.last_accessed_at,
                   cache_schema_version=excluded.cache_schema_version",
                params![
                    key.account_id,
                    key.workspace_id,
                    key.domain,
                    key.query_key,
                    payload_json,
                    snapshot.workspace_epoch,
                    snapshot.change_seq,
                    snapshot.fetched_at,
                    now,
                    CACHE_SCHEMA_VERSION,
                ],
            )
            .map_err(|error| error.to_string())?;
        self.prune(&connection, now)
    }

    pub fn invalidate_domain(
        &self,
        account_id: &str,
        workspace_id: &str,
        domain: &str,
    ) -> Result<(), String> {
        self.open()?
            .execute(
                "DELETE FROM query_snapshots WHERE account_id=?1 AND workspace_id=?2 AND domain=?3",
                params![account_id, workspace_id, domain],
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    pub fn clear_identity(&self, account_id: &str, workspace_id: &str) -> Result<(), String> {
        self.open()?
            .execute(
                "DELETE FROM query_snapshots WHERE account_id=?1 AND workspace_id=?2",
                params![account_id, workspace_id],
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    pub fn clear_workspace(&self, workspace_id: &str) -> Result<(), String> {
        self.open()?
            .execute(
                "DELETE FROM query_snapshots WHERE workspace_id=?1",
                [workspace_id],
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    pub fn clear_all(&self) -> Result<(), String> {
        self.open()?
            .execute("DELETE FROM query_snapshots", [])
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    pub fn latest_for_workspace(
        &self,
        workspace_id: &str,
        now: i64,
    ) -> Result<Option<(String, i64)>, String> {
        let connection = self.open()?;
        let fetched_at: Option<i64> = connection
            .query_row(
                "SELECT MAX(fetched_at) FROM query_snapshots
                 WHERE workspace_id=?1 AND cache_schema_version=?2 AND fetched_at>=?3",
                params![
                    workspace_id,
                    CACHE_SCHEMA_VERSION,
                    now.saturating_sub(MAX_CACHE_AGE_MILLIS)
                ],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        Ok(fetched_at.map(|fetched_at| ("persistent".to_string(), fetched_at)))
    }

    fn initialize_schema(&self) -> Result<(), String> {
        self.open()?
            .execute_batch(
                "PRAGMA journal_mode=WAL;
                 PRAGMA busy_timeout=5000;
                 CREATE TABLE IF NOT EXISTS cache_metadata(
                   key TEXT PRIMARY KEY,value_integer INTEGER NOT NULL
                 );
                 INSERT INTO cache_metadata(key,value_integer) VALUES('schema_version',1)
                   ON CONFLICT(key) DO UPDATE SET value_integer=excluded.value_integer;
                 CREATE TABLE IF NOT EXISTS query_snapshots(
                   account_id TEXT NOT NULL,
                   workspace_id TEXT NOT NULL,
                   domain TEXT NOT NULL,
                   query_key TEXT NOT NULL,
                   payload_json TEXT NOT NULL,
                   workspace_epoch INTEGER NOT NULL,
                   change_seq INTEGER NOT NULL,
                   fetched_at INTEGER NOT NULL,
                   last_accessed_at INTEGER NOT NULL,
                   cache_schema_version INTEGER NOT NULL,
                   PRIMARY KEY(account_id,workspace_id,domain,query_key)
                 );
                 CREATE INDEX IF NOT EXISTS idx_cloud_read_cache_lru
                   ON query_snapshots(last_accessed_at);",
            )
            .map_err(|error| error.to_string())
    }

    fn open(&self) -> Result<Connection, String> {
        Connection::open(&self.path).map_err(|error| error.to_string())
    }

    fn rebuild_corrupt_file(&self, cause: &str) -> Result<(), String> {
        if self.path.exists() {
            fs::remove_file(&self.path)
                .map_err(|error| format!("无法重建损坏的云端只读缓存（{cause}）: {error}"))?;
        }
        for suffix in ["-wal", "-shm"] {
            let sidecar = PathBuf::from(format!("{}{suffix}", self.path.display()));
            if sidecar.exists() {
                let _ = fs::remove_file(sidecar);
            }
        }
        Ok(())
    }

    fn delete_key(
        &self,
        connection: &Connection,
        key: &CloudReadCacheKey<'_>,
    ) -> Result<(), String> {
        connection
            .execute(
                "DELETE FROM query_snapshots
                 WHERE account_id=?1 AND workspace_id=?2 AND domain=?3 AND query_key=?4",
                params![key.account_id, key.workspace_id, key.domain, key.query_key],
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn prune(&self, connection: &Connection, now: i64) -> Result<(), String> {
        connection
            .execute(
                "DELETE FROM query_snapshots WHERE fetched_at < ?1",
                [now.saturating_sub(MAX_CACHE_AGE_MILLIS)],
            )
            .map_err(|error| error.to_string())?;
        while fs::metadata(&self.path)
            .map(|metadata| metadata.len() > MAX_CACHE_BYTES)
            .unwrap_or(false)
        {
            if connection
                .execute(
                    "DELETE FROM query_snapshots WHERE rowid IN (
                       SELECT rowid FROM query_snapshots ORDER BY last_accessed_at LIMIT 32
                     )",
                    [],
                )
                .map_err(|error| error.to_string())?
                == 0
            {
                break;
            }
        }
        Ok(())
    }

    #[cfg(test)]
    fn path(&self) -> &Path {
        &self.path
    }
}

fn sanitize_payload(value: Value) -> Value {
    match value {
        Value::Object(mut object) => {
            object.retain(|key, _| !is_secret_key(key));
            for value in object.values_mut() {
                *value = sanitize_payload(value.take());
            }
            Value::Object(object)
        }
        Value::Array(values) => Value::Array(values.into_iter().map(sanitize_payload).collect()),
        other => other,
    }
}

fn is_secret_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace(['-', '_'], "");
    [
        "accesstoken",
        "refreshtoken",
        "authorization",
        "apikey",
        "anonkey",
        "servicerolekey",
        "password",
        "secret",
        "integrationkey",
    ]
    .iter()
    .any(|secret| normalized.contains(secret))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn key<'a>(account_id: &'a str, workspace_id: &'a str) -> CloudReadCacheKey<'a> {
        CloudReadCacheKey {
            account_id,
            workspace_id,
            domain: "tasks",
            query_key: "list:open",
        }
    }

    #[test]
    fn cache_is_a_separate_file_and_isolated_by_identity_and_query_key() {
        let directory = tempfile::tempdir().unwrap();
        let business_path = directory.path().join("timegenie.sqlite3");
        let database = Database::initialize_at(business_path.clone()).unwrap();
        let cache = CloudReadCache::for_database(&database).unwrap();
        assert_ne!(cache.path(), business_path.as_path());
        assert_eq!(cache.path().file_name().unwrap(), CACHE_FILE_NAME);

        let snapshot = CloudReadCacheSnapshot {
            payload: json!({"tasks":[{"id":"task-1"}]}),
            workspace_epoch: 2,
            change_seq: 9,
            fetched_at: 1_000,
        };
        cache
            .save(&key("account-a", "workspace-a"), &snapshot, 1_000)
            .unwrap();
        assert_eq!(
            cache.read(&key("account-a", "workspace-a"), 1_001).unwrap(),
            Some(snapshot)
        );
        assert!(cache
            .read(&key("account-b", "workspace-a"), 1_001)
            .unwrap()
            .is_none());
        assert!(cache
            .read(&key("account-a", "workspace-b"), 1_001)
            .unwrap()
            .is_none());
    }

    #[test]
    fn cache_sanitizes_secrets_expires_old_rows_and_has_no_business_tables() {
        let directory = tempfile::tempdir().unwrap();
        let cache = CloudReadCache::initialize_at(directory.path().join(CACHE_FILE_NAME)).unwrap();
        let snapshot = CloudReadCacheSnapshot {
            payload: json!({
                "accessToken":"never-write",
                "nested":{"authorization":"Bearer secret","title":"safe"},
                "integration_key":"private",
            }),
            workspace_epoch: 1,
            change_seq: 1,
            fetched_at: 10,
        };
        cache
            .save(&key("account", "workspace"), &snapshot, 10)
            .unwrap();
        let stored = cache
            .read(&key("account", "workspace"), 11)
            .unwrap()
            .unwrap();
        assert_eq!(stored.payload, json!({"nested":{"title":"safe"}}));
        let connection = cache.open().unwrap();
        let business_tables: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('tasks','subjects','time_entries','sync_outbox')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(business_tables, 0);
        drop(connection);
        assert!(cache
            .read(&key("account", "workspace"), MAX_CACHE_AGE_MILLIS + 11)
            .unwrap()
            .is_none());
    }

    #[test]
    fn corrupt_cache_is_rebuilt_without_touching_the_business_database() {
        let directory = tempfile::tempdir().unwrap();
        let business_path = directory.path().join("timegenie.sqlite3");
        let database = Database::initialize_at(business_path.clone()).unwrap();
        let workspace_count_before: i64 = database
            .open()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM workspaces", [], |row| row.get(0))
            .unwrap();
        let cache_path = directory.path().join(CACHE_FILE_NAME);
        fs::write(&cache_path, b"not a sqlite database").unwrap();

        let cache = CloudReadCache::for_database(&database).unwrap();
        assert!(cache
            .read(&key("account", "workspace"), 1)
            .unwrap()
            .is_none());
        let workspace_count_after: i64 = database
            .open()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM workspaces", [], |row| row.get(0))
            .unwrap();
        assert_eq!(workspace_count_before, workspace_count_after);
    }

    #[test]
    fn clearing_cache_never_changes_local_business_data() {
        let directory = tempfile::tempdir().unwrap();
        let database = Database::initialize_at(directory.path().join("timegenie.sqlite3")).unwrap();
        let workspace_id: String = database
            .open()
            .unwrap()
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let subjects_before: i64 = database
            .open()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM subjects", [], |row| row.get(0))
            .unwrap();
        let cache = CloudReadCache::for_database(&database).unwrap();
        cache
            .save(
                &key("account", &workspace_id),
                &CloudReadCacheSnapshot {
                    payload: json!({"tasks":[]}),
                    workspace_epoch: 1,
                    change_seq: 1,
                    fetched_at: 1,
                },
                1,
            )
            .unwrap();
        cache.clear_all().unwrap();
        assert!(cache
            .read(&key("account", &workspace_id), 2)
            .unwrap()
            .is_none());
        let subjects_after: i64 = database
            .open()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM subjects", [], |row| row.get(0))
            .unwrap();
        assert_eq!(subjects_before, subjects_after);
    }

    #[test]
    fn domain_and_epoch_invalidation_only_remove_the_intended_snapshots() {
        let directory = tempfile::tempdir().unwrap();
        let cache = CloudReadCache::initialize_at(directory.path().join(CACHE_FILE_NAME)).unwrap();
        for (domain, query_key) in [("tasks", "open"), ("reports", "list")] {
            cache
                .save(
                    &CloudReadCacheKey {
                        account_id: "account",
                        workspace_id: "workspace-a",
                        domain,
                        query_key,
                    },
                    &CloudReadCacheSnapshot {
                        payload: json!({"domain":domain}),
                        workspace_epoch: 1,
                        change_seq: 1,
                        fetched_at: 1,
                    },
                    1,
                )
                .unwrap();
        }
        cache
            .invalidate_domain("account", "workspace-a", "tasks")
            .unwrap();
        assert!(cache
            .read(
                &CloudReadCacheKey {
                    account_id: "account",
                    workspace_id: "workspace-a",
                    domain: "tasks",
                    query_key: "open",
                },
                2,
            )
            .unwrap()
            .is_none());
        assert!(cache
            .read(
                &CloudReadCacheKey {
                    account_id: "account",
                    workspace_id: "workspace-a",
                    domain: "reports",
                    query_key: "list",
                },
                2,
            )
            .unwrap()
            .is_some());
        cache.clear_workspace("workspace-a").unwrap();
        assert!(cache
            .latest_for_workspace("workspace-a", 3)
            .unwrap()
            .is_none());
    }

    #[test]
    fn clearing_one_identity_cannot_expose_or_remove_another_identity() {
        let directory = tempfile::tempdir().unwrap();
        let cache = CloudReadCache::initialize_at(directory.path().join(CACHE_FILE_NAME)).unwrap();
        for account_id in ["account-a", "account-b"] {
            cache
                .save(
                    &key(account_id, "workspace"),
                    &CloudReadCacheSnapshot {
                        payload: json!({"owner":account_id}),
                        workspace_epoch: 1,
                        change_seq: 1,
                        fetched_at: 1,
                    },
                    1,
                )
                .unwrap();
        }
        cache.clear_identity("account-a", "workspace").unwrap();
        assert!(cache
            .read(&key("account-a", "workspace"), 2)
            .unwrap()
            .is_none());
        assert_eq!(
            cache
                .read(&key("account-b", "workspace"), 2)
                .unwrap()
                .unwrap()
                .payload,
            json!({"owner":"account-b"})
        );
    }
}
