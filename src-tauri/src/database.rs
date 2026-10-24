use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::Serialize;
use uuid::Uuid;

const DATABASE_FILE_NAME: &str = "timegenie.sqlite3";
const LEGACY_SYNC_CLEANUP_MIGRATION_VERSION: i64 = 32;

struct Migration {
    version: i64,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial_local_schema",
        sql: include_str!("../migrations/0001_initial_local_schema.sql"),
    },
    Migration {
        version: 2,
        name: "task_daily_estimates",
        sql: include_str!("../migrations/0002_task_daily_estimates.sql"),
    },
    Migration {
        version: 3,
        name: "recurring_tasks",
        sql: include_str!("../migrations/0003_recurring_tasks.sql"),
    },
    Migration {
        version: 4,
        name: "automation_hooks",
        sql: include_str!("../migrations/0004_automation_hooks.sql"),
    },
    Migration {
        version: 5,
        name: "cloud_session_lifecycle",
        sql: include_str!("../migrations/0005_cloud_session_lifecycle.sql"),
    },
    Migration {
        version: 6,
        name: "outbox_coordination",
        sql: include_str!("../migrations/0006_outbox_coordination.sql"),
    },
    Migration {
        version: 7,
        name: "local_only_unassigned_sessions",
        sql: include_str!("../migrations/0007_local_only_unassigned_sessions.sql"),
    },
    Migration {
        version: 8,
        name: "event_driven_cloud_sync",
        sql: include_str!("../migrations/0008_event_driven_cloud_sync.sql"),
    },
    Migration {
        version: 9,
        name: "shared_unassigned_sessions",
        sql: include_str!("../migrations/0009_shared_unassigned_sessions.sql"),
    },
    Migration {
        version: 10,
        name: "calendar_day_time_accounting",
        sql: include_str!("../migrations/0010_calendar_day_time_accounting.sql"),
    },
    Migration {
        version: 11,
        name: "simplified_unassigned_anchor",
        sql: include_str!("../migrations/0011_simplified_unassigned_anchor.sql"),
    },
    Migration {
        version: 12,
        name: "repair_unassigned_active_index",
        sql: include_str!("../migrations/0012_repair_unassigned_active_index.sql"),
    },
    Migration {
        version: 13,
        name: "supersede_historical_unassigned",
        sql: include_str!("../migrations/0013_supersede_historical_unassigned.sql"),
    },
    Migration {
        version: 14,
        name: "named_report_templates",
        sql: include_str!("../migrations/0014_named_report_templates.sql"),
    },
    Migration {
        version: 15,
        name: "subject_work_schedules",
        sql: include_str!("../migrations/0015_subject_work_schedules.sql"),
    },
    Migration {
        version: 16,
        name: "timer_sync_recovery",
        sql: include_str!("../migrations/0016_timer_sync_recovery.sql"),
    },
    Migration {
        version: 17,
        name: "actual_time_adjustments",
        sql: include_str!("../migrations/0017_actual_time_adjustments.sql"),
    },
    Migration {
        version: 18,
        name: "workspace_epoch",
        sql: include_str!("../migrations/0018_workspace_epoch.sql"),
    },
    Migration {
        version: 19,
        name: "local_backup_history",
        sql: include_str!("../migrations/0019_local_backup_history.sql"),
    },
    Migration {
        version: 20,
        name: "task_day_plans",
        sql: include_str!("../migrations/0020_task_day_plans.sql"),
    },
    Migration {
        version: 21,
        name: "actual_adjustment_conflict_classification",
        sql: include_str!("../migrations/0021_actual_adjustment_conflict_classification.sql"),
    },
    Migration {
        version: 22,
        name: "retry_actual_adjustment_target_conflicts",
        sql: include_str!("../migrations/0022_retry_actual_adjustment_target_conflicts.sql"),
    },
    Migration {
        version: 23,
        name: "today_task_instances",
        sql: include_str!("../migrations/0023_today_task_instances.sql"),
    },
    Migration {
        version: 24,
        name: "task_day_plan_instances",
        sql: include_str!("../migrations/0024_task_day_plan_instances.sql"),
    },
    Migration {
        version: 25,
        name: "daily_report_collection_templates",
        sql: include_str!("../migrations/0025_daily_report_collection_templates.sql"),
    },
    Migration {
        version: 26,
        name: "report_list_pagination_indexes",
        sql: include_str!("../migrations/0026_report_list_pagination_indexes.sql"),
    },
    Migration {
        version: 27,
        name: "archived_task_parent_scope",
        sql: include_str!("../migrations/0027_archived_task_parent_scope.sql"),
    },
    Migration {
        version: 28,
        name: "remove_context_day_plan_outbox",
        sql: include_str!("../migrations/0028_remove_context_day_plan_outbox.sql"),
    },
    Migration {
        version: 29,
        name: "daily_report_task_import",
        sql: include_str!("../migrations/0029_daily_report_task_import.sql"),
    },
    Migration {
        version: 30,
        name: "legacy_abandoned_outbox",
        sql: include_str!("../migrations/0030_legacy_abandoned_outbox.sql"),
    },
    Migration {
        version: 31,
        name: "remove_unused_legacy_cloud_mirrors",
        sql: include_str!("../migrations/0031_remove_unused_legacy_cloud_mirrors.sql"),
    },
    Migration {
        version: 32,
        name: "remove_legacy_sync_runtime",
        sql: include_str!("../migrations/0032_remove_legacy_sync_runtime.sql"),
    },
];

#[derive(Clone, Debug)]
pub struct Database {
    path: PathBuf,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseStatus {
    pub path: String,
    pub schema_version: i64,
    pub applied_migrations: Vec<String>,
    pub tables: Vec<String>,
}

impl Database {
    pub fn initialize(app_data_dir: &Path) -> Result<Self, String> {
        fs::create_dir_all(app_data_dir)
            .map_err(|error| format!("无法创建应用数据目录 {}: {error}", app_data_dir.display()))?;
        let database = Self {
            path: app_data_dir.join(DATABASE_FILE_NAME),
        };
        let mut connection = database.open()?;
        apply_migrations_before_legacy_cleanup(&mut connection)?;
        drop(connection);
        crate::data_protection::export_legacy_sync_retirement_rescue_if_needed(&database)?;
        let mut connection = database.open()?;
        apply_migrations(&mut connection)?;
        ensure_initial_data(&mut connection)?;
        crate::work_calendar::validate_all_workspace_timezones(&connection)?;
        drop(connection);
        recover_local_tracking_state(&database)?;
        Ok(database)
    }

    #[cfg(test)]
    pub(crate) fn initialize_at(path: PathBuf) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let database = Self { path };
        let mut connection = database.open()?;
        apply_migrations_before_legacy_cleanup(&mut connection)?;
        drop(connection);
        crate::data_protection::export_legacy_sync_retirement_rescue_if_needed(&database)?;
        let mut connection = database.open()?;
        apply_migrations(&mut connection)?;
        ensure_initial_data(&mut connection)?;
        crate::work_calendar::validate_all_workspace_timezones(&connection)?;
        drop(connection);
        recover_local_tracking_state(&database)?;
        Ok(database)
    }

    pub fn open(&self) -> Result<Connection, String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("无法创建数据库目录 {}: {error}", parent.display()))?;
        }
        let connection = Connection::open(&self.path)
            .map_err(|error| format!("无法打开数据库 {}: {error}", self.path.display()))?;
        configure_connection(&connection)
            .map_err(|error| format!("无法配置 SQLite {}: {error}", self.path.display()))?;
        Ok(connection)
    }

    pub(crate) fn data_directory(&self) -> PathBuf {
        self.path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    }

    pub fn status(&self) -> Result<DatabaseStatus, String> {
        let connection = self.open()?;
        let schema_version = connection
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let applied_migrations = collect_strings(
            &connection,
            "SELECT printf('%d:%s', version, name) FROM schema_migrations ORDER BY version",
        )?;
        let tables = collect_strings(
            &connection,
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )?;
        Ok(DatabaseStatus {
            path: self.path.display().to_string(),
            schema_version,
            applied_migrations,
            tables,
        })
    }
}

fn recover_local_tracking_state(database: &Database) -> Result<(), String> {
    recover_local_tracking_state_at(database, now_millis())
}

fn recover_local_tracking_state_at(database: &Database, now: i64) -> Result<(), String> {
    let connection = database.open()?;
    let storage_mode: String = connection
        .query_row(
            "SELECT value_json FROM device_settings WHERE key='storage_mode'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .and_then(|value| serde_json::from_str::<String>(&value).ok())
        .unwrap_or_else(|| "local".to_string());
    drop(connection);
    if storage_mode != "local" {
        return Ok(());
    }
    crate::time_tracking::coordinate_active_timer(database, now)?;
    crate::unassigned::coordinate_sessions(database, now)
}

fn configure_connection(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA busy_timeout = 5000;",
        )
        .map_err(|error| format!("无法配置 SQLite: {error}"))
}

fn apply_migrations(connection: &mut Connection) -> Result<(), String> {
    apply_migrations_through(connection, None)
}

fn apply_migrations_before_legacy_cleanup(connection: &mut Connection) -> Result<(), String> {
    apply_migrations_through(connection, Some(LEGACY_SYNC_CLEANUP_MIGRATION_VERSION - 1))
}

fn apply_migrations_through(
    connection: &mut Connection,
    maximum_version: Option<i64>,
) -> Result<(), String> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                applied_at INTEGER NOT NULL
            );",
        )
        .map_err(|error| format!("无法创建迁移记录表: {error}"))?;

    for migration in MIGRATIONS {
        if maximum_version.is_some_and(|maximum| migration.version > maximum) {
            break;
        }
        let applied = connection
            .query_row(
                "SELECT 1 FROM schema_migrations WHERE version = ?1",
                [migration.version],
                |_| Ok(()),
            )
            .optional()
            .map_err(|error| error.to_string())?
            .is_some();
        if applied {
            continue;
        }

        let transaction = connection
            .transaction()
            .map_err(|error| error.to_string())?;
        transaction.execute_batch(migration.sql).map_err(|error| {
            format!(
                "执行数据库迁移 {} ({}) 失败: {error}",
                migration.version, migration.name
            )
        })?;
        transaction
            .execute(
                "INSERT INTO schema_migrations(version, name, applied_at) VALUES (?1, ?2, ?3)",
                params![migration.version, migration.name, now_millis()],
            )
            .map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn ensure_initial_data(connection: &mut Connection) -> Result<(), String> {
    let existing_workspace: Option<String> = connection
        .query_row(
            "SELECT id FROM workspaces WHERE storage_mode = 'local' AND deleted_at IS NULL LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if let Some(workspace_id) = existing_workspace {
        ensure_demo_seed(connection, &workspace_id)?;
        return Ok(());
    }

    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    seed_local_workspace(&transaction)?;
    transaction.commit().map_err(|error| error.to_string())
}

fn ensure_demo_seed(connection: &mut Connection, workspace_id: &str) -> Result<(), String> {
    let seeded: Option<String> = connection
        .query_row(
            "SELECT value FROM app_metadata WHERE key = 'initial_demo_tasks_seeded'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if seeded.is_some() {
        return Ok(());
    }
    let task_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM tasks WHERE workspace_id = ?1 AND deleted_at IS NULL",
            [workspace_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let subject_id: String = connection
        .query_row(
            "SELECT id FROM subjects WHERE workspace_id = ?1 AND deleted_at IS NULL ORDER BY sort_order LIMIT 1",
            [workspace_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    if task_count == 0 {
        seed_demo_tasks(&transaction, workspace_id, &subject_id, now_millis())?;
    }
    transaction
        .execute(
            "INSERT INTO app_metadata(key, value) VALUES ('initial_demo_tasks_seeded', '1')",
            [],
        )
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())
}

fn seed_local_workspace(transaction: &Transaction<'_>) -> Result<(), String> {
    let now = now_millis();
    let workspace_id = Uuid::now_v7().to_string();
    let device_id = Uuid::now_v7().to_string();
    let subject_id = Uuid::now_v7().to_string();
    transaction
        .execute(
            "INSERT INTO workspaces(id, owner_user_id, name, timezone, storage_mode, created_at, updated_at, version)
             VALUES (?1, NULL, '本地工作台', 'Asia/Shanghai', 'local', ?2, ?2, 1)",
            params![workspace_id, now],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO devices(id, workspace_id, device_name, platform, app_version, last_seen_at, created_at)
             VALUES (?1, ?2, '当前设备', 'windows', ?3, ?4, ?4)",
            params![device_id, workspace_id, env!("CARGO_PKG_VERSION"), now],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO subjects(id, workspace_id, name, sort_order, created_at, updated_at, version)
             VALUES (?1, ?2, '默认', 10, ?3, ?3, 1)",
            params![subject_id, workspace_id, now],
        )
        .map_err(|error| error.to_string())?;
    crate::subject_work_schedules::insert_default_schedule(
        transaction,
        &workspace_id,
        &subject_id,
        now,
    )?;

    for (key, value) in [
        ("default_subject_id", serde_json::json!(subject_id)),
        ("timezone", serde_json::json!("Asia/Shanghai")),
        ("unassigned_prompt_seconds", serde_json::json!(300)),
        ("default_work_period_text", serde_json::json!("")),
        ("salary_hourly_rate", serde_json::json!(0)),
    ] {
        transaction
            .execute(
                "INSERT INTO app_settings(workspace_id, key, value_json, updated_at, version)
                 VALUES (?1, ?2, ?3, ?4, 1)",
                params![workspace_id, key, value.to_string(), now],
            )
            .map_err(|error| error.to_string())?;
    }

    for (key, value) in [
        ("storage_mode", serde_json::json!("local")),
        ("device_id", serde_json::json!(device_id)),
        ("device_name", serde_json::json!("当前设备")),
        ("tray_hover_enabled", serde_json::json!(true)),
        ("tray_menu_suppress_hover", serde_json::json!(true)),
        (
            "tray_icon_style",
            serde_json::json!(crate::tray_presentation::DEFAULT_TRAY_ICON_STYLE),
        ),
        ("start_minimized", serde_json::json!(false)),
        ("obsidian_root_path", serde_json::json!("")),
        (
            "obsidian_daily_path_pattern",
            serde_json::json!("工作日报/{date}.md"),
        ),
        ("supabase_project_url", serde_json::json!("")),
        ("supabase_anon_key", serde_json::json!("")),
    ] {
        transaction
            .execute(
                "INSERT INTO device_settings(key, value_json, updated_at)
                 VALUES (?1, ?2, ?3)",
                params![key, value.to_string(), now],
            )
            .map_err(|error| error.to_string())?;
    }

    let templates = [
        (
            "daily",
            "标准日报",
            "【{{姓名}}】{{日期}} {{星期}} {{工作时段}} {{总工时}}\n\n{{今日事项}}\n\n## 明日计划：\n\n{{明日计划}}\n",
        ),
        (
            "weekly",
            "标准周报",
            "{{统计信息}}\n\n{{日报合集}}\n",
        ),
        (
            "monthly",
            "标准月报",
            "{{统计信息}}\n\n{{日报合集}}\n",
        ),
    ];
    for (index, (report_type, name, content)) in templates.into_iter().enumerate() {
        transaction
            .execute(
                "INSERT INTO report_templates(id, workspace_id, report_type, subject_id, content, is_builtin, created_at, updated_at, version, name, description, is_default, sort_order)
                 VALUES (?1, ?2, ?3, NULL, ?4, 1, ?5, ?5, 1, ?6, '系统内置模板', 1, ?7)",
                params![Uuid::now_v7().to_string(), workspace_id, report_type, content, now, name, (index as i64 + 1) * 10],
            )
            .map_err(|error| error.to_string())?;
    }
    seed_demo_tasks(transaction, &workspace_id, &subject_id, now)?;
    transaction
        .execute(
            "INSERT INTO app_metadata(key, value) VALUES ('initial_demo_tasks_seeded', '1')",
            [],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn seed_demo_tasks(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    subject_id: &str,
    now: i64,
) -> Result<(), String> {
    let roots = [("今日计划梳理与方案预备", "今日计划"), ("运维相关", "运维")];
    let mut root_ids = Vec::new();
    for (index, (title, project)) in roots.into_iter().enumerate() {
        let id = Uuid::now_v7().to_string();
        transaction
            .execute(
                "INSERT INTO tasks(id, workspace_id, subject_id, title, status, project_name, source_type, sort_order, created_at, updated_at, version)
                 VALUES (?1, ?2, ?3, ?4, 'open', ?5, 'manual', ?6, ?7, ?7, 1)",
                params![id, workspace_id, subject_id, title, project, (index as i64 + 1) * 10, now],
            )
            .map_err(|error| error.to_string())?;
        root_ids.push(id);
    }
    let children = [
        "开发江湖数据同步v3功能",
        "跟老师讨论江湖数据同步v3方案（UI、数据结构、接口设计）",
        "按照规则重新分配PVE虚拟机ID，开启虚拟机备份",
        "整理工作记录文档到Teable",
        "加一个模板虚拟机，面板有什么更新都同步过去，然后更新完存一个虚拟机的template，名称带版本号",
        "梳理数据分析相关",
        "调研自动化运维工具（试用Jpom、Openocta，梳理有用的自动化运维功能）",
    ];
    for (index, title) in children.into_iter().enumerate() {
        transaction
            .execute(
                "INSERT INTO tasks(id, workspace_id, subject_id, parent_id, title, status, project_name, source_type, sort_order, created_at, updated_at, version)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'open', '运维', 'manual', ?6, ?7, ?7, 1)",
                params![Uuid::now_v7().to_string(), workspace_id, subject_id, root_ids[1], title, (index as i64 + 1) * 10, now],
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn collect_strings(connection: &Connection, sql: &str) -> Result<Vec<String>, String> {
    let mut statement = connection.prepare(sql).map_err(|error| error.to_string())?;
    let values = statement
        .query_map([], |row| row.get(0))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<String>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(values)
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[tauri::command]
pub fn database_status(database: tauri::State<'_, Database>) -> Result<DatabaseStatus, String> {
    database.status()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const REQUIRED_TABLES: &[&str] = &[
        "workspaces",
        "devices",
        "subjects",
        "subject_work_schedules",
        "subject_work_schedule_segments",
        "work_days",
        "tasks",
        "task_daily_estimates",
        "task_day_plans",
        "task_recurrence_rules",
        "task_occurrences",
        "task_status_events",
        "time_entries",
        "time_segments",
        "time_allocations",
        "unassigned_sessions",
        "unassigned_segments",
        "report_templates",
        "reports",
        "report_tasks",
        "integration_runs",
        "integration_run_items",
        "app_settings",
        "device_settings",
        "integration_configs",
        "local_request_results",
        "local_report_outputs",
        "device_hooks",
        "device_hook_runs",
        "tracking_runtime_state",
        "actual_time_adjustments",
        "daily_report_import_batches",
        "daily_report_import_items",
        "daily_report_import_sources",
        "legacy_sync_cleanup_diagnostics",
    ];

    fn latest_migration_version() -> i64 {
        MIGRATIONS
            .last()
            .map(|migration| migration.version)
            .unwrap_or(0)
    }

    fn migrate_through_legacy_cleanup(path: &std::path::Path) -> Connection {
        let mut connection = Connection::open(path).unwrap();
        configure_connection(&connection).unwrap();
        apply_migrations_before_legacy_cleanup(&mut connection).unwrap();
        connection
    }

    #[test]
    fn first_start_applies_migration_and_seeds_local_workspace() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(directory.path().join("test.sqlite3")).unwrap();
        let status = database.status().unwrap();
        assert_eq!(status.schema_version, latest_migration_version());
        for table in REQUIRED_TABLES {
            assert!(
                status.tables.iter().any(|value| value == table),
                "missing {table}"
            );
        }

        let connection = database.open().unwrap();
        let workspace_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM workspaces", [], |row| row.get(0))
            .unwrap();
        let subject_name: String = connection
            .query_row("SELECT name FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let template_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM report_templates", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(workspace_count, 1);
        assert_eq!(subject_name, "默认");
        assert_eq!(template_count, 3);
    }

    #[test]
    fn migrations_and_seed_are_idempotent() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("test.sqlite3");
        Database::initialize_at(path.clone()).unwrap();
        let database = Database::initialize_at(path).unwrap();
        let connection = database.open().unwrap();
        let migration_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        let workspace_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM workspaces", [], |row| row.get(0))
            .unwrap();
        assert_eq!(migration_count, latest_migration_version());
        assert_eq!(workspace_count, 1);
    }

    #[test]
    fn upgrade_with_legacy_queue_creates_one_rescue_backup_and_continues() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("legacy-queue-upgrade.sqlite3");
        let database = Database { path: path.clone() };
        let mut connection = database.open().unwrap();
        apply_migrations_before_legacy_cleanup(&mut connection).unwrap();
        ensure_initial_data(&mut connection).unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        connection.execute(
            "INSERT INTO sync_outbox(
                 operation_id,workspace_id,device_id,operation_type,entity_type,payload_json,
                 state,attempt_count,created_at,error_json
             ) VALUES
                 ('pending-upgrade',?1,?2,'task_update','task','{\"secret\":\"payload-token\"}','pending',0,1,NULL),
                 ('conflict-upgrade',?1,?2,'task_update','task','{}','conflict',1,2,
                  '{\"message\":\"VERSION_CONFLICT: bearer-secret\"}')",
            params![workspace_id, device_id],
        ).unwrap();
        drop(connection);

        let upgraded = Database::initialize_at(path.clone()).unwrap();
        assert_eq!(
            upgraded.status().unwrap().schema_version,
            latest_migration_version()
        );
        let history = upgraded.open().unwrap().query_row(
            "SELECT path,backup_kind,origin FROM local_backup_history ORDER BY recorded_at DESC LIMIT 1",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
        ).unwrap();
        assert_eq!(history.1, "stale_epoch_rescue");
        assert_eq!(history.2, "safety");
        let validated =
            crate::data_protection::validate_rescue_backup_for_abandon(Path::new(&history.0))
                .unwrap();
        assert_eq!(validated.pending_operation_count, 2);
        assert_eq!(validated.legacy_queue_operation_count, 2);
        let archive_text =
            crate::data_protection::read_json_archive(fs::File::open(&history.0).unwrap())
                .unwrap()
                .values()
                .map(serde_json::Value::to_string)
                .collect::<Vec<_>>()
                .join("\n");
        assert!(!archive_text.contains("payload-token"));
        assert!(!archive_text.contains("bearer-secret"));
        let connection = upgraded.open().unwrap();
        for removed in [
            "sync_outbox",
            "local_sync_state",
            "cloud_deferred_entities",
            "legacy_abandoned_outbox",
            "processed_operations",
            "sync_items",
            "sync_runs",
        ] {
            let exists: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [removed],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(exists, 0, "legacy table {removed} should be removed");
        }
        drop(connection);

        Database::initialize_at(path).unwrap();
        let backup_count: i64 = upgraded
            .open()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM local_backup_history WHERE backup_kind='stale_epoch_rescue'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(backup_count, 1);
    }

    #[test]
    fn final_cleanup_preserves_local_business_and_renames_non_sync_runtime_data() {
        let directory = tempdir().unwrap();
        let path = directory
            .path()
            .join("final-cleanup-preserves-local.sqlite3");
        let database = Database { path: path.clone() };
        let mut connection = database.open().unwrap();
        apply_migrations_before_legacy_cleanup(&mut connection).unwrap();
        ensure_initial_data(&mut connection).unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let subject_id: String = connection
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        connection.execute(
            "INSERT INTO tasks(id,workspace_id,subject_id,title,status,source_type,sort_order,created_at,updated_at,version)
             VALUES('preserved-task',?1,?2,'保留本地事项','open','manual',99,1,1,1)",
            params![workspace_id, subject_id],
        ).unwrap();
        connection.execute(
            "INSERT INTO processed_operations(workspace_id,operation_id,device_id,operation_type,result_json,processed_at)
             VALUES(?1,'local-request',?2,'timer_start','{\"ok\":true}',1)",
            params![workspace_id, device_id],
        ).unwrap();
        connection.execute(
            "INSERT INTO sync_runs(id,workspace_id,provider,operation_type,state,request_json,preview_json,created_at)
             VALUES('integration-run',?1,'seatable','task_sync','preview','{}','{}',1)",
            [&workspace_id],
        ).unwrap();
        connection.execute(
            "INSERT INTO sync_items(id,workspace_id,run_id,entity_type,entity_id,action,state)
             VALUES('integration-item',?1,'integration-run','task','preserved-task','update','pending')",
            [&workspace_id],
        ).unwrap();
        drop(connection);

        let upgraded = Database::initialize_at(path).unwrap();
        let connection = upgraded.open().unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT title FROM tasks WHERE id='preserved-task'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "保留本地事项"
        );
        assert_eq!(connection.query_row("SELECT request_type FROM local_request_results WHERE request_id='local-request'", [], |row| row.get::<_, String>(0)).unwrap(), "timer_start");
        assert_eq!(
            connection
                .query_row(
                    "SELECT provider FROM integration_runs WHERE id='integration-run'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "seatable"
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT action FROM integration_run_items WHERE id='integration-item'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "update"
        );
    }

    #[test]
    fn legacy_cloud_mirror_cleanup_preserves_business_and_device_data() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("legacy-cloud-mirror-cleanup.sqlite3");
        let mut connection = Connection::open(&path).unwrap();
        configure_connection(&connection).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                   version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at INTEGER NOT NULL
                 );",
            )
            .unwrap();
        for migration in &MIGRATIONS[..30] {
            let transaction = connection.transaction().unwrap();
            transaction.execute_batch(migration.sql).unwrap();
            transaction
                .execute(
                    "INSERT INTO schema_migrations(version,name,applied_at) VALUES (?1,?2,1)",
                    params![migration.version, migration.name],
                )
                .unwrap();
            transaction.commit().unwrap();
        }
        connection
            .execute_batch(
                r#"INSERT INTO workspaces(id,name,timezone,storage_mode,created_at,updated_at,version)
                     VALUES('workspace','保留工作区','Asia/Shanghai','local',1,1,1);
                   INSERT INTO devices(id,workspace_id,device_name,platform,app_version,last_seen_at,created_at)
                     VALUES('device','workspace','保留设备','windows','1',1,1);
                   INSERT INTO subjects(id,workspace_id,name,sort_order,created_at,updated_at,version)
                     VALUES('subject','workspace','保留主体',10,1,1,1);
                   INSERT INTO device_settings(key,value_json,updated_at)
                     VALUES('tray_hover_enabled','true',1);
                   INSERT INTO tracking_leases(workspace_id,holder_device_id,lease_token,expires_at,updated_at,version)
                     VALUES('workspace','device','legacy-token',2,1,1);
                   INSERT INTO workspace_changes(workspace_id,entity_type,entity_id,operation,entity_version,changed_by_device_id,changed_at)
                     VALUES('workspace','task','legacy-task','update',2,'device',1);"#,
            )
            .unwrap();
        drop(connection);

        let database = Database::initialize_at(path).unwrap();
        let connection = database.open().unwrap();
        assert_eq!(
            connection
                .query_row("SELECT name FROM subjects WHERE id='subject'", [], |row| {
                    row.get::<_, String>(0)
                })
                .unwrap(),
            "保留主体"
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT value_json FROM device_settings WHERE key='tray_hover_enabled'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "true"
        );
        for removed in ["tracking_leases", "workspace_changes"] {
            let exists: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [removed],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(exists, 0, "legacy table {removed} should be removed");
            let recorded: i64 = connection
                .query_row(
                    "SELECT row_count FROM legacy_sync_cleanup_diagnostics WHERE table_name=?1",
                    [removed],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(recorded, 1);
        }
    }

    #[test]
    fn daily_report_import_migration_preserves_legacy_import_tables_and_adjustments() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("daily-report-import-upgrade.sqlite3");
        let mut connection = Connection::open(&path).unwrap();
        configure_connection(&connection).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                   version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at INTEGER NOT NULL
                 );",
            )
            .unwrap();
        for migration in &MIGRATIONS[..28] {
            let transaction = connection.transaction().unwrap();
            transaction.execute_batch(migration.sql).unwrap();
            transaction
                .execute(
                    "INSERT INTO schema_migrations(version,name,applied_at) VALUES (?1,?2,1)",
                    params![migration.version, migration.name],
                )
                .unwrap();
            transaction.commit().unwrap();
        }
        connection.execute_batch(
            r#"INSERT INTO workspaces(id,name,timezone,storage_mode,created_at,updated_at,version)
                 VALUES('ws','迁移','Asia/Shanghai','local',1,1,1);
               INSERT INTO devices(id,workspace_id,device_name,platform,app_version,last_seen_at,created_at)
                 VALUES('device','ws','设备','windows','1',1,1);
               INSERT INTO subjects(id,workspace_id,name,sort_order,created_at,updated_at,version)
                 VALUES('subject','ws','主体',10,1,1,1);
               INSERT INTO tasks(id,workspace_id,subject_id,title,status,source_type,sort_order,created_at,updated_at,version)
                 VALUES('task','ws','subject','事项','open','manual',10,1,1,1);
               INSERT INTO plan_import_batches(id,workspace_id,target_date,subject_id,source_path,source_mtime,source_content_hash,state,raw_markdown,warnings_json,created_at,updated_at,version)
                 VALUES('old-batch','ws','2026-10-22','subject','D:/old.md',1,'hash','preview','# 旧','[]',1,1,1);
               INSERT INTO plan_import_items(id,workspace_id,batch_id,title,sort_order,source_line_no,source_text,parse_status)
                 VALUES('old-item','ws','old-batch','旧事项',10,1,'1. 旧事项','recognized');
               INSERT INTO actual_time_adjustments(id,workspace_id,subject_id,task_id,work_date,delta_minutes,baseline_minutes,target_minutes,note,source_type,batch_id,created_at,updated_at,version)
                 VALUES('adjustment','ws','subject','task','2026-10-22',15,0,15,'旧调整','quick_actual_adjustment','adjustment-batch',1,1,1);"#,
        ).unwrap();
        drop(connection);

        let database = Database::initialize_at(path).unwrap();
        let connection = database.open().unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM plan_import_batches WHERE id='old-batch'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM plan_import_items WHERE id='old-item'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT source_type FROM actual_time_adjustments WHERE id='adjustment'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "quick_actual_adjustment"
        );
        connection.execute(
            "INSERT INTO actual_time_adjustments(id,workspace_id,subject_id,task_id,work_date,delta_minutes,baseline_minutes,target_minutes,source_type,batch_id,created_at,updated_at,version) VALUES('daily-adjustment','ws','subject','task','2026-10-22',5,15,20,'daily_report_import','daily-batch',2,2,1)",
            [],
        ).unwrap();
    }

    #[test]
    fn report_pagination_index_migration_preserves_existing_reports() {
        let directory = tempdir().unwrap();
        let path = directory
            .path()
            .join("report-pagination-index-upgrade.sqlite3");
        let mut connection = Connection::open(&path).unwrap();
        configure_connection(&connection).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                   version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at INTEGER NOT NULL
                 );",
            )
            .unwrap();
        for migration in &MIGRATIONS[..25] {
            let transaction = connection.transaction().unwrap();
            transaction.execute_batch(migration.sql).unwrap();
            transaction
                .execute(
                    "INSERT INTO schema_migrations(version,name,applied_at) VALUES (?1,?2,1)",
                    params![migration.version, migration.name],
                )
                .unwrap();
            transaction.commit().unwrap();
        }
        connection
            .execute_batch(
                "INSERT INTO workspaces(id,name,timezone,storage_mode,created_at,updated_at,version)
                   VALUES ('pagination-ws','分页迁移','Asia/Shanghai','local',1,1,1);
                 INSERT INTO devices(id,workspace_id,device_name,platform,app_version,last_seen_at,created_at)
                   VALUES ('pagination-device','pagination-ws','旧设备','test','1',1,1);
                 INSERT INTO subjects(id,workspace_id,name,sort_order,created_at,updated_at,version)
                   VALUES ('pagination-subject','pagination-ws','旧主体',10,1,1,1);
                 INSERT INTO reports(
                   id,workspace_id,report_type,subject_id,period_start,period_end,reference_date,
                   markdown_content,content_source,generation_count,created_at,updated_at,version,
                   created_by_device_id,updated_by_device_id
                 ) VALUES (
                   'pagination-report','pagination-ws','daily','pagination-subject',
                   '2026-10-20','2026-10-20','2026-10-20','# 历史正文','edited',3,2,3,4,
                   'pagination-device','pagination-device'
                 );
                 INSERT INTO app_metadata(key,value) VALUES ('initial_demo_tasks_seeded','1');",
            )
            .unwrap();
        drop(connection);

        let database = Database::initialize_at(path).unwrap();
        assert_eq!(
            database.status().unwrap().schema_version,
            latest_migration_version()
        );
        let connection = database.open().unwrap();
        let report: (String, String, i64, i64) = connection
            .query_row(
                "SELECT markdown_content,content_source,generation_count,version
                 FROM reports WHERE id='pagination-report'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            report,
            ("# 历史正文".to_string(), "edited".to_string(), 3, 4)
        );
        let indexes = connection
            .prepare(
                "SELECT name FROM sqlite_master
                 WHERE type='index' AND name LIKE 'idx_reports_list_%' ORDER BY name",
            )
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            indexes,
            vec![
                "idx_reports_list_subject_type_updated".to_string(),
                "idx_reports_list_subject_updated".to_string(),
            ]
        );
    }

    #[test]
    fn actual_adjustment_target_migration_retries_old_baseline_conflicts() {
        let directory = tempdir().unwrap();
        let path = directory
            .path()
            .join("actual-adjustment-conflict-upgrade.sqlite3");
        let mut connection = Connection::open(&path).unwrap();
        configure_connection(&connection).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                   version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at INTEGER NOT NULL
                 );",
            )
            .unwrap();
        for migration in &MIGRATIONS[..20] {
            let transaction = connection.transaction().unwrap();
            transaction.execute_batch(migration.sql).unwrap();
            transaction
                .execute(
                    "INSERT INTO schema_migrations(version,name,applied_at) VALUES (?1,?2,1)",
                    params![migration.version, migration.name],
                )
                .unwrap();
            transaction.commit().unwrap();
        }
        connection
            .execute(
                "INSERT INTO sync_outbox(operation_id,workspace_id,device_id,operation_type,entity_type,entity_id,payload_json,state,attempt_count,created_at,last_attempt_at,error_json)
                 VALUES('adjustment-op','workspace','device','actual_adjustment_batch_apply','actual_adjustment_batch','batch','{}','pending',7,100,200,?1)",
                [serde_json::json!({ "message": "CLOUD_REQUEST_FAILED: ADJUSTMENT_BASELINE_CONFLICT: actual adjustment baseline changed" }).to_string()],
            )
            .unwrap();
        drop(connection);

        let connection = migrate_through_legacy_cleanup(&path);
        let state: String = connection
            .query_row(
                "SELECT state FROM sync_outbox WHERE operation_id='adjustment-op'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(state, "pending");
        let error_json: Option<String> = connection
            .query_row(
                "SELECT error_json FROM sync_outbox WHERE operation_id='adjustment-op'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(error_json.is_none());
    }

    #[test]
    fn task_day_plan_migration_backfills_estimates_with_versions_idempotently() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("task-day-plan-upgrade.sqlite3");
        let mut connection = Connection::open(&path).unwrap();
        configure_connection(&connection).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                   version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at INTEGER NOT NULL
                 );",
            )
            .unwrap();
        for migration in &MIGRATIONS[..19] {
            let transaction = connection.transaction().unwrap();
            transaction.execute_batch(migration.sql).unwrap();
            transaction
                .execute(
                    "INSERT INTO schema_migrations(version,name,applied_at) VALUES (?1,?2,1)",
                    params![migration.version, migration.name],
                )
                .unwrap();
            transaction.commit().unwrap();
        }
        connection
            .execute_batch(
                "INSERT INTO workspaces(id,name,timezone,storage_mode,created_at,updated_at,version)
                   VALUES ('plan-ws','Plan','Asia/Shanghai','local',1,1,1);
                 INSERT INTO subjects(id,workspace_id,name,sort_order,created_at,updated_at,version)
                   VALUES ('plan-subject','plan-ws','Plan',10,1,1,1);
                 INSERT INTO tasks(id,workspace_id,subject_id,title,status,source_type,sort_order,created_at,updated_at,version)
                   VALUES ('plan-task-a','plan-ws','plan-subject','A','open','manual',10,1,1,1),
                          ('plan-task-b','plan-ws','plan-subject','B','open','manual',20,1,1,1);
                 INSERT INTO task_daily_estimates(workspace_id,task_id,work_date,estimate_minutes,created_at,updated_at,version)
                   VALUES ('plan-ws','plan-task-a','2026-10-19',30,11,12,7),
                          ('plan-ws','plan-task-b','2026-10-20',45,21,22,3);",
            )
            .unwrap();
        drop(connection);

        Database::initialize_at(path.clone()).unwrap();
        Database::initialize_at(path.clone()).unwrap();
        let connection = Connection::open(path).unwrap();
        let rows: Vec<(String, String, String, i64, i64, i64)> = connection
            .prepare(
                "SELECT task_id,work_date,state,created_at,updated_at,version
                 FROM task_day_plans WHERE workspace_id='plan-ws' ORDER BY task_id",
            )
            .unwrap()
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            rows,
            vec![
                (
                    "plan-task-a".to_string(),
                    "2026-10-19".to_string(),
                    "included".to_string(),
                    11,
                    12,
                    7,
                ),
                (
                    "plan-task-b".to_string(),
                    "2026-10-20".to_string(),
                    "included".to_string(),
                    21,
                    22,
                    3,
                ),
            ]
        );
        let migration_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version=20",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(migration_count, 1);
    }

    #[test]
    fn today_instance_migrations_enforce_scope_and_allow_readd_after_archive() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("today-instance-upgrade.sqlite3");
        let mut connection = Connection::open(&path).unwrap();
        configure_connection(&connection).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                   version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at INTEGER NOT NULL
                 );",
            )
            .unwrap();
        for migration in &MIGRATIONS[..22] {
            let transaction = connection.transaction().unwrap();
            transaction.execute_batch(migration.sql).unwrap();
            transaction
                .execute(
                    "INSERT INTO schema_migrations(version,name,applied_at) VALUES (?1,?2,1)",
                    params![migration.version, migration.name],
                )
                .unwrap();
            transaction.commit().unwrap();
        }
        connection
            .execute_batch(
                "INSERT INTO workspaces(id,name,timezone,storage_mode,created_at,updated_at,version)
                   VALUES ('instance-ws','Instance','Asia/Shanghai','local',1,1,1);
                 INSERT INTO subjects(id,workspace_id,name,sort_order,created_at,updated_at,version)
                   VALUES ('instance-subject','instance-ws','Instance',10,1,1,1);
                 INSERT INTO tasks(id,workspace_id,subject_id,title,status,source_type,sort_order,created_at,updated_at,version)
                   VALUES ('source-parent','instance-ws','instance-subject','Parent','open','manual',10,1,1,1),
                          ('source-child','instance-ws','instance-subject','Child','open','manual',20,1,1,1);",
            )
            .unwrap();
        drop(connection);

        Database::initialize_at(path.clone()).unwrap();
        Database::initialize_at(path.clone()).unwrap();
        let connection = Connection::open(path).unwrap();
        configure_connection(&connection).unwrap();

        let ordinary_scope: (Option<String>, Option<String>, Option<String>) = connection
            .query_row(
                "SELECT day_instance_date,day_instance_kind,day_instance_source_task_id
                 FROM tasks WHERE id='source-child'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(ordinary_scope, (None, None, None));

        connection
            .execute_batch(
                "INSERT INTO tasks(
                     id,workspace_id,subject_id,title,status,source_type,sort_order,created_at,updated_at,version,
                     day_instance_date,day_instance_kind,day_instance_source_task_id
                   ) VALUES (
                     'context-1','instance-ws','instance-subject','Parent','open','manual',10,2,2,1,
                     '2026-10-21','context','source-parent'
                   );
                 INSERT INTO tasks(
                     id,workspace_id,subject_id,parent_id,title,status,source_type,sort_order,created_at,updated_at,version,
                     day_instance_date,day_instance_kind,day_instance_source_task_id
                   ) VALUES (
                     'member-1','instance-ws','instance-subject','context-1','Child','open','manual',20,2,2,1,
                     '2026-10-21','member','source-child'
                   );
                 INSERT INTO task_day_plans(
                     workspace_id,task_id,work_date,state,created_at,updated_at,version,instance_task_id
                   ) VALUES (
                     'instance-ws','source-child','2026-10-21','included',2,2,1,'member-1'
                   );",
            )
            .unwrap();

        let invalid_field_pair = connection.execute(
            "INSERT INTO tasks(
               id,workspace_id,subject_id,title,status,source_type,sort_order,created_at,updated_at,version,
               day_instance_date
             ) VALUES (
               'invalid-fields','instance-ws','instance-subject','Invalid','open','manual',30,2,2,1,
               '2026-10-21'
             )",
            [],
        );
        assert!(invalid_field_pair.is_err());

        let cross_date_parent = connection.execute(
            "INSERT INTO tasks(
               id,workspace_id,subject_id,parent_id,title,status,source_type,sort_order,created_at,updated_at,version,
               day_instance_date,day_instance_kind
             ) VALUES (
               'cross-date-member','instance-ws','instance-subject','context-1','Cross date','open','manual',30,2,2,1,
               '2026-10-22','member'
             )",
            [],
        );
        assert!(cross_date_parent.is_err());

        let duplicate_active_source = connection.execute(
            "INSERT INTO tasks(
               id,workspace_id,subject_id,parent_id,title,status,source_type,sort_order,created_at,updated_at,version,
               day_instance_date,day_instance_kind,day_instance_source_task_id
             ) VALUES (
               'member-duplicate','instance-ws','instance-subject','context-1','Duplicate','open','manual',30,2,2,1,
               '2026-10-21','member','source-child'
             )",
            [],
        );
        assert!(duplicate_active_source.is_err());

        connection
            .execute(
                "UPDATE tasks SET deleted_at=3,updated_at=3,version=version+1 WHERE id='member-1'",
                [],
            )
            .unwrap();
        connection
            .execute_batch(
                "INSERT INTO tasks(
                   id,workspace_id,subject_id,parent_id,title,status,source_type,sort_order,created_at,updated_at,version,
                   day_instance_date,day_instance_kind,day_instance_source_task_id
                 ) VALUES (
                   'member-2','instance-ws','instance-subject','context-1','Child again','open','manual',20,4,4,1,
                   '2026-10-21','member','source-child'
                 );
                 UPDATE task_day_plans
                    SET state='included',instance_task_id='member-2',updated_at=4,version=version+1
                  WHERE task_id='source-child' AND work_date='2026-10-21';",
            )
            .unwrap();

        let plan: (String, String) = connection
            .query_row(
                "SELECT state,instance_task_id FROM task_day_plans
                 WHERE task_id='source-child' AND work_date='2026-10-21'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(plan, ("included".to_string(), "member-2".to_string()));
        let archived_old_instance: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE id='member-1' AND deleted_at IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(archived_old_instance, 1);
        connection
            .execute(
                "UPDATE tasks SET deleted_at=5,updated_at=5,version=version+1 WHERE id='context-1'",
                [],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE tasks SET parent_id='context-1' WHERE id='member-1'",
                [],
            )
            .unwrap();
        let restore_child_under_archived_parent =
            connection.execute("UPDATE tasks SET deleted_at=NULL WHERE id='member-1'", []);
        assert!(restore_child_under_archived_parent.is_err());
        let migration_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version IN (23,24,27)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(migration_count, 3);
    }

    #[test]
    fn context_day_plan_cleanup_migration_removes_only_invalid_mapping() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("context-day-plan-cleanup.sqlite3");
        let mut connection = Connection::open(&path).unwrap();
        configure_connection(&connection).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                   version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at INTEGER NOT NULL
                 );",
            )
            .unwrap();
        for migration in &MIGRATIONS[..27] {
            let transaction = connection.transaction().unwrap();
            transaction.execute_batch(migration.sql).unwrap();
            transaction
                .execute(
                    "INSERT INTO schema_migrations(version,name,applied_at) VALUES (?1,?2,1)",
                    params![migration.version, migration.name],
                )
                .unwrap();
            transaction.commit().unwrap();
        }
        connection.execute_batch(
            r#"INSERT INTO workspaces(id,name,storage_mode,created_at,updated_at) VALUES('ws','测试','cloud',1,1);
             INSERT INTO devices(id,workspace_id,device_name,platform,app_version,last_seen_at,created_at)
               VALUES('device','ws','测试设备','windows','0.4.6',1,1);
             INSERT INTO subjects(id,workspace_id,name,sort_order,created_at,updated_at,version)
               VALUES('subject','ws','测试主体',1,1,1,1);
             INSERT INTO tasks(id,workspace_id,subject_id,title,status,source_type,sort_order,created_at,updated_at,version)
               VALUES('source-context','ws','subject','上下文来源','open','manual',1,1,1,1),
                     ('source-member','ws','subject','成员来源','open','manual',2,1,1,1);
             INSERT INTO tasks(id,workspace_id,subject_id,title,status,source_type,sort_order,created_at,updated_at,version,day_instance_date,day_instance_kind,day_instance_source_task_id)
               VALUES('context-instance','ws','subject','上下文实例','open','manual',1,1,1,1,'2026-10-22','context','source-context'),
                     ('member-instance','ws','subject','成员实例','open','manual',2,1,1,1,'2026-10-22','member','source-member');
             INSERT INTO task_day_plans(workspace_id,task_id,work_date,state,created_at,updated_at,version,instance_task_id)
               VALUES('ws','source-context','2026-10-22','included',1,1,1,'context-instance'),
                     ('ws','source-member','2026-10-22','included',1,1,1,'member-instance');
             INSERT INTO sync_outbox(operation_id,workspace_id,device_id,operation_type,entity_type,entity_id,payload_json,state,attempt_count,created_at)
               VALUES('context-plan-op','ws','device','task_day_plan_materialize','task_day_plan','source-context|2026-10-22','{"task_id":"source-context","work_date":"2026-10-22","state":"included","instance_task_id":"context-instance","version":1}','failed',9,1),
                     ('member-plan-op','ws','device','task_day_plan_materialize','task_day_plan','source-member|2026-10-22','{"task_id":"source-member","work_date":"2026-10-22","state":"included","instance_task_id":"member-instance","version":1}','pending',0,1);"#,
        ).unwrap();
        drop(connection);

        let connection = migrate_through_legacy_cleanup(&path);
        let context_plan_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM task_day_plans WHERE task_id='source-context'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let context_queue_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sync_outbox WHERE operation_id='context-plan-op'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let member_plan_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM task_day_plans WHERE task_id='source-member'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let member_queue_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sync_outbox WHERE operation_id='member-plan-op'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(context_plan_count, 0);
        assert_eq!(context_queue_count, 0);
        assert_eq!(member_plan_count, 1);
        assert_eq!(member_queue_count, 1);
    }

    #[test]
    fn workspace_epoch_migration_defaults_existing_sync_state_and_outbox() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("workspace-epoch-upgrade.sqlite3");
        let mut connection = Connection::open(&path).unwrap();
        configure_connection(&connection).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                   version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at INTEGER NOT NULL
                 );",
            )
            .unwrap();
        for migration in &MIGRATIONS[..17] {
            let transaction = connection.transaction().unwrap();
            transaction.execute_batch(migration.sql).unwrap();
            transaction
                .execute(
                    "INSERT INTO schema_migrations(version,name,applied_at) VALUES (?1,?2,1)",
                    params![migration.version, migration.name],
                )
                .unwrap();
            transaction.commit().unwrap();
        }
        connection.execute_batch(
            "INSERT INTO workspaces(id,name,timezone,storage_mode,created_at,updated_at,version)
               VALUES ('epoch-ws','Epoch','Asia/Shanghai','cloud',1,1,1);
             INSERT INTO devices(id,workspace_id,device_name,platform,app_version,last_seen_at,created_at)
               VALUES ('epoch-device','epoch-ws','Device','test','1',1,1);
             INSERT INTO local_sync_state(workspace_id,device_id,last_change_seq)
               VALUES ('epoch-ws','epoch-device',5);
             INSERT INTO sync_outbox(operation_id,workspace_id,device_id,operation_type,entity_type,entity_id,payload_json,state,created_at)
               VALUES ('epoch-operation','epoch-ws','epoch-device','task_update','task',NULL,'{}','pending',1);",
        ).unwrap();
        drop(connection);

        let connection = migrate_through_legacy_cleanup(&path);
        let sync_state: (i64, bool, Option<String>, Option<i64>) = connection
            .query_row(
                "SELECT workspace_epoch,epoch_blocked,epoch_blocked_reason,epoch_blocked_at FROM local_sync_state WHERE workspace_id='epoch-ws'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(sync_state, (1, false, None, None));
        let outbox_epoch: i64 = connection
            .query_row(
                "SELECT workspace_epoch FROM sync_outbox WHERE operation_id='epoch-operation'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(outbox_epoch, 1);
        assert_eq!(
            connection
                .query_row(
                    "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            LEGACY_SYNC_CLEANUP_MIGRATION_VERSION - 1
        );
    }

    #[test]
    fn timer_sync_recovery_upgrade_marks_legacy_conflict_without_mutating_local_history() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("timer-sync-recovery-upgrade.sqlite3");
        let mut connection = Connection::open(&path).unwrap();
        configure_connection(&connection).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
               version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at INTEGER NOT NULL
             );",
            )
            .unwrap();
        for migration in &MIGRATIONS[..15] {
            let transaction = connection.transaction().unwrap();
            transaction.execute_batch(migration.sql).unwrap();
            transaction
                .execute(
                    "INSERT INTO schema_migrations(version,name,applied_at) VALUES (?1,?2,1)",
                    params![migration.version, migration.name],
                )
                .unwrap();
            transaction.commit().unwrap();
        }
        connection.execute_batch(
            "INSERT INTO workspaces(id,name,timezone,storage_mode,created_at,updated_at,version)
               VALUES ('timer-ws','计时恢复','Asia/Shanghai','cloud',1,1,1);
             INSERT INTO devices(id,workspace_id,device_name,platform,app_version,last_seen_at,created_at)
               VALUES ('timer-device','timer-ws','旧设备','test','1',1,1);
             INSERT INTO subjects(id,workspace_id,name,sort_order,created_at,updated_at,version)
               VALUES ('timer-subject','timer-ws','默认',10,1,1,1);
             INSERT INTO tasks(id,workspace_id,subject_id,title,status,source_type,sort_order,created_at,updated_at,version)
               VALUES ('timer-task','timer-ws','timer-subject','故障事项','open','manual',10,1,1,1);
             INSERT INTO time_entries(id,workspace_id,work_date,kind,source_type,state,default_task_id,label_snapshot,started_at,ended_at,duration_seconds,note,created_at,updated_at,version,created_by_device_id,updated_by_device_id,timer_chain_id,last_continuous_at)
               VALUES ('timer-entry','timer-ws','2026-10-14','work','timer','ended','timer-task','故障事项',1000,361000,360,'必须保留',1000,361000,8,'timer-device','timer-device','timer-entry',361000);
             INSERT INTO time_segments(id,workspace_id,entry_id,sequence_no,started_at,ended_at,duration_seconds) VALUES
               ('segment-1','timer-ws','timer-entry',1,1000,61000,60),
               ('segment-2','timer-ws','timer-entry',2,61000,121000,60),
               ('segment-3','timer-ws','timer-entry',3,121000,181000,60),
               ('segment-4','timer-ws','timer-entry',4,181000,241000,60),
               ('segment-5','timer-ws','timer-entry',5,241000,301000,60),
               ('segment-6','timer-ws','timer-entry',6,301000,361000,60);
             INSERT INTO time_allocations(id,workspace_id,entry_id,task_id,minutes,note,created_at,updated_at,version)
               VALUES ('allocation-1','timer-ws','timer-entry','timer-task',6,'最终分配',361000,361000,1);",
        ).unwrap();
        let payload = serde_json::json!({
            "id": "timer-entry",
            "source_type": "timer",
            "state": "ended",
            "ended_at": 361000,
            "duration_seconds": 360,
            "version": 8,
            "segments": [1, 2, 3, 4, 5, 6],
            "allocations": [{"task_id": "timer-task", "minutes": 6}]
        })
        .to_string();
        let error = serde_json::json!({
            "message": "CLOUD_REQUEST_FAILED: IDEMPOTENCY_CONFLICT: operation_id was already used for different content"
        }).to_string();
        connection.execute(
            "INSERT INTO sync_outbox(operation_id,workspace_id,device_id,operation_type,entity_type,entity_id,base_version,payload_json,payload_version,depends_on_operation_id,state,attempt_count,created_at,last_attempt_at,error_json,coalesced_count)
             VALUES ('legacy-timer-op','cloud-timer-ws','timer-device','timer_continuity_close','time_entry','timer-entry',4,?1,8,NULL,'conflict',28,10,20,?2,4)",
            params![payload, error],
        ).unwrap();
        drop(connection);

        let connection = migrate_through_legacy_cleanup(&path);
        assert_eq!(
            connection
                .query_row(
                    "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            LEGACY_SYNC_CLEANUP_MIGRATION_VERSION - 1
        );
        let queue: (String,String,i64,String,String,i64,Option<String>,String,Option<String>) = connection.query_row(
            "SELECT operation_id,state,attempt_count,payload_json,error_json,coalesced_count,depends_on_operation_id,recovery_kind,recovery_context_json FROM sync_outbox WHERE operation_id='legacy-timer-op'",
            [],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?,row.get(7)?,row.get(8)?)),
        ).unwrap();
        assert_eq!(queue.0, "legacy-timer-op");
        assert_eq!(queue.1, "conflict");
        assert_eq!(queue.2, 28);
        assert_eq!(queue.3, payload);
        assert_eq!(queue.4, error);
        assert_eq!(queue.5, 4);
        assert_eq!(queue.6, None);
        assert_eq!(queue.7, "historical_candidate");
        assert_eq!(queue.8.as_deref(), Some(queue.4.as_str()));
        let aggregate: (String,i64,i64,i64,i64) = connection.query_row(
            "SELECT state,ended_at,duration_seconds,(SELECT COUNT(*) FROM time_segments WHERE entry_id='timer-entry'),(SELECT minutes FROM time_allocations WHERE entry_id='timer-entry') FROM time_entries WHERE id='timer-entry'",
            [],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?)),
        ).unwrap();
        assert_eq!(aggregate, ("ended".to_string(), 361000, 360, 6, 6));
    }

    #[test]
    fn named_template_migration_preserves_legacy_templates_and_report_content() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("named-template-upgrade.sqlite3");
        let mut connection = Connection::open(&path).unwrap();
        configure_connection(&connection).unwrap();
        connection.execute_batch("CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at INTEGER NOT NULL);").unwrap();
        for migration in &MIGRATIONS[..13] {
            let transaction = connection.transaction().unwrap();
            transaction.execute_batch(migration.sql).unwrap();
            transaction
                .execute(
                    "INSERT INTO schema_migrations(version,name,applied_at) VALUES (?1,?2,1)",
                    params![migration.version, migration.name],
                )
                .unwrap();
            transaction.commit().unwrap();
        }
        connection.execute_batch(
            "INSERT INTO workspaces(id,name,timezone,storage_mode,created_at,updated_at,version) VALUES ('ws','迁移','Asia/Shanghai','local',1,1,1);
             INSERT INTO subjects(id,workspace_id,name,sort_order,created_at,updated_at,version) VALUES ('subject-a','ws','客户',10,1,1,1),('subject-b','ws','客户 ',20,1,1,1);
             INSERT INTO report_templates(id,workspace_id,report_type,subject_id,content,is_builtin,created_at,updated_at,version) VALUES
               ('default-daily','ws','daily',NULL,'默认内容',1,1,1,3),
               ('subject-template-a','ws','daily','subject-a','客户 A 内容',0,2,2,4),
               ('subject-template-b','ws','daily','subject-b','客户 B 内容',0,3,3,5);
             INSERT INTO reports(id,workspace_id,report_type,subject_id,period_start,period_end,reference_date,template_id,markdown_content,content_source,generation_count,created_at,updated_at,version)
               VALUES ('report','ws','daily','subject-a','2026-10-11','2026-10-11','2026-10-11','subject-template-a','# 手工内容','edited',7,4,4,9);
             INSERT INTO app_metadata(key,value) VALUES ('initial_demo_tasks_seeded','1');",
        ).unwrap();
        drop(connection);

        let database = Database::initialize_at(path.clone()).unwrap();
        let connection = database.open().unwrap();
        let templates = connection.prepare("SELECT id,name,subject_id,content,is_default FROM report_templates WHERE workspace_id='ws' ORDER BY id").unwrap()
            .query_map([], |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,Option<String>>(2)?,row.get::<_,String>(3)?,row.get::<_,i64>(4)?))).unwrap()
            .collect::<Result<Vec<_>,_>>().unwrap();
        assert_eq!(templates.len(), 5);
        assert!(templates.iter().all(|item| item.2.is_none()));
        assert!(templates.iter().any(|item| item.0 == "subject-template-a"
            && item.1 == "客户 - 日报"
            && item.3 == "客户 A 内容"));
        assert!(templates.iter().any(|item| item.0 == "subject-template-b"
            && item.1 == "客户 - 日报 (2)"
            && item.3 == "客户 B 内容"));
        assert_eq!(
            templates
                .iter()
                .filter(|item| item.4 == 1 && item.0.contains("daily"))
                .count(),
            1
        );
        let report: (Option<String>,String,i64,i64) = connection.query_row("SELECT template_id,markdown_content,generation_count,version FROM reports WHERE id='report'",[],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).unwrap();
        assert_eq!(
            report,
            (
                Some("subject-template-a".to_string()),
                "# 手工内容".to_string(),
                7,
                9
            )
        );

        // Rollback drill: the previous application selected one effective
        // template per type. After migration that compatible read must resolve
        // to the new default without deleting the additional global templates
        // or changing historical report bindings/content.
        let legacy_effective: (String, String) = connection
            .query_row(
                "SELECT id,content FROM report_templates
                 WHERE workspace_id='ws' AND report_type='daily' AND subject_id IS NULL
                   AND deleted_at IS NULL
                 ORDER BY is_default DESC,is_builtin DESC,created_at,id LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(legacy_effective.0, "default-daily");
        assert_eq!(legacy_effective.1, "默认内容");
        let counts_before_rollback_read: (i64, i64) = connection
            .query_row(
                "SELECT (SELECT COUNT(*) FROM report_templates WHERE workspace_id='ws'),
                        (SELECT COUNT(*) FROM reports WHERE workspace_id='ws')",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let report_after_rollback_read: (Option<String>, String, i64, i64) = connection
            .query_row(
                "SELECT template_id,markdown_content,generation_count,version FROM reports WHERE id='report'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(counts_before_rollback_read, (5, 1));
        assert_eq!(report_after_rollback_read, report);
        drop(connection);
        Database::initialize_at(path.clone()).unwrap();
        let reopened = Connection::open(path).unwrap();
        assert_eq!(
            reopened
                .query_row(
                    "SELECT COUNT(*) FROM report_templates WHERE workspace_id='ws'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            5
        );
        assert_eq!(
            reopened
                .query_row(
                    "SELECT template_id FROM reports WHERE id='report'",
                    [],
                    |row| row.get::<_, Option<String>>(0),
                )
                .unwrap(),
            Some("subject-template-a".to_string())
        );
    }

    #[test]
    fn event_driven_sync_migration_preserves_business_data_and_outbox() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("event-driven-upgrade.sqlite3");
        let mut connection = Connection::open(&path).unwrap();
        configure_connection(&connection).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                   version INTEGER PRIMARY KEY,
                   name TEXT NOT NULL,
                   applied_at INTEGER NOT NULL
                 );",
            )
            .unwrap();
        for migration in &MIGRATIONS[..7] {
            let transaction = connection.transaction().unwrap();
            transaction.execute_batch(migration.sql).unwrap();
            transaction
                .execute(
                    "INSERT INTO schema_migrations(version, name, applied_at) VALUES (?1, ?2, ?3)",
                    params![migration.version, migration.name, now_millis()],
                )
                .unwrap();
            transaction.commit().unwrap();
        }
        connection
            .execute(
                "INSERT INTO workspaces(id, name, timezone, storage_mode, created_at, updated_at, version)
                 VALUES ('upgrade-workspace', '升级工作空间', 'Asia/Shanghai', 'cloud', 1, 1, 1)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO subjects(id, workspace_id, name, sort_order, created_at, updated_at, version)
                 VALUES ('upgrade-subject', 'upgrade-workspace', '升级主体', 1, 1, 1, 1)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, payload_json, state, attempt_count, created_at)
                 VALUES ('upgrade-operation', 'upgrade-workspace', 'upgrade-device', 'task.update', 'task', 'upgrade-task', '{}', 'pending', 0, 1)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO tasks(id, workspace_id, subject_id, title, status, source_type, sort_order, created_at, updated_at, version)
                 VALUES ('upgrade-task', 'upgrade-workspace', 'upgrade-subject', '升级前事项', 'open', 'manual', 99, 1, 1, 1)",
                [],
            )
            .unwrap();
        drop(connection);

        let connection = migrate_through_legacy_cleanup(&path);
        let task_title: String = connection
            .query_row(
                "SELECT title FROM tasks WHERE id = 'upgrade-task'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let queued: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sync_outbox WHERE operation_id = 'upgrade-operation'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let deferred_table: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('cloud_deferred_entities')",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(task_title, "升级前事项");
        assert_eq!(queued, 1);
        assert_eq!(deferred_table, 6);
        assert_eq!(
            connection
                .query_row(
                    "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            LEGACY_SYNC_CLEANUP_MIGRATION_VERSION - 1
        );
    }

    #[test]
    fn outbox_coordination_migration_preserves_legacy_rows() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("legacy-outbox.sqlite3");
        let mut connection = Connection::open(&path).unwrap();
        configure_connection(&connection).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                   version INTEGER PRIMARY KEY,
                   name TEXT NOT NULL,
                   applied_at INTEGER NOT NULL
                 );",
            )
            .unwrap();
        for migration in &MIGRATIONS[..5] {
            let transaction = connection.transaction().unwrap();
            transaction.execute_batch(migration.sql).unwrap();
            transaction
                .execute(
                    "INSERT INTO schema_migrations(version, name, applied_at) VALUES (?1, ?2, ?3)",
                    params![migration.version, migration.name, now_millis()],
                )
                .unwrap();
            transaction.commit().unwrap();
        }
        for (index, state) in ["pending", "sending", "failed", "conflict"]
            .into_iter()
            .enumerate()
        {
            connection
                .execute(
                    "INSERT INTO sync_outbox(
                       operation_id, workspace_id, device_id, operation_type, entity_type,
                       entity_id, base_version, payload_json, state, attempt_count, created_at
                     ) VALUES (?1, 'legacy-workspace', 'legacy-device', 'timer_update',
                       'time_entry', ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        format!("legacy-{state}"),
                        format!("entry-{index}"),
                        index as i64,
                        serde_json::json!({ "version": index as i64 + 1 }).to_string(),
                        state,
                        index as i64,
                        now_millis() + index as i64,
                    ],
                )
                .unwrap();
        }
        drop(connection);

        let connection = migrate_through_legacy_cleanup(&path);
        let rows = connection
            .prepare(
                "SELECT state, payload_version, depends_on_operation_id, coalesced_count
                 FROM sync_outbox WHERE workspace_id = 'legacy-workspace' ORDER BY created_at",
            )
            .unwrap()
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(rows.len(), 4);
        for (index, (state, payload_version, dependency, coalesced_count)) in
            rows.iter().enumerate()
        {
            assert_eq!(state, ["pending", "sending", "failed", "conflict"][index]);
            assert_eq!(*payload_version, Some(index as i64 + 1));
            assert_eq!(*dependency, None);
            assert_eq!(*coalesced_count, 1);
        }
    }

    #[test]
    fn local_only_unassigned_migration_removes_old_session_operations_only() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("legacy-unassigned-outbox.sqlite3");
        let mut connection = Connection::open(&path).unwrap();
        configure_connection(&connection).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                   version INTEGER PRIMARY KEY,
                   name TEXT NOT NULL,
                   applied_at INTEGER NOT NULL
                 );",
            )
            .unwrap();
        for migration in &MIGRATIONS[..6] {
            let transaction = connection.transaction().unwrap();
            transaction.execute_batch(migration.sql).unwrap();
            transaction
                .execute(
                    "INSERT INTO schema_migrations(version, name, applied_at) VALUES (?1, ?2, ?3)",
                    params![migration.version, migration.name, now_millis()],
                )
                .unwrap();
            transaction.commit().unwrap();
        }
        for (operation_id, entity_type, state, payload) in [
            ("old-unassigned", "unassigned_session", "conflict", "{}"),
            (
                "keep-time-entry",
                "time_entry",
                "pending",
                r#"{"origin_unassigned_session_id":"local-session","title":"保留"}"#,
            ),
        ] {
            connection
                .execute(
                    "INSERT INTO sync_outbox(operation_id, workspace_id, device_id, operation_type, entity_type, entity_id, payload_json, state, attempt_count, created_at)
                     VALUES (?1, 'legacy-workspace', 'legacy-device', 'update', ?2, ?1, ?4, ?3, 1, 1)",
                    params![operation_id, entity_type, state, payload],
                )
                .unwrap();
        }
        drop(connection);

        let connection = migrate_through_legacy_cleanup(&path);
        let rows = collect_strings(
            &connection,
            "SELECT operation_id FROM sync_outbox ORDER BY operation_id",
        )
        .unwrap();
        assert_eq!(rows, vec!["keep-time-entry"]);
        let payload: String = connection
            .query_row(
                "SELECT payload_json FROM sync_outbox WHERE operation_id = 'keep-time-entry'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let payload: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(
            payload["origin_unassigned_session_id"],
            serde_json::Value::Null
        );
        assert_eq!(payload["title"], "保留");
    }

    #[test]
    fn shared_unassigned_migration_preserves_history_and_unrelated_outbox() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("shared-unassigned-upgrade.sqlite3");
        let mut connection = Connection::open(&path).unwrap();
        configure_connection(&connection).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                   version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at INTEGER NOT NULL
                 );",
            )
            .unwrap();
        for migration in &MIGRATIONS[..8] {
            let transaction = connection.transaction().unwrap();
            transaction.execute_batch(migration.sql).unwrap();
            transaction
                .execute(
                    "INSERT INTO schema_migrations(version,name,applied_at) VALUES (?1,?2,?3)",
                    params![migration.version, migration.name, now_millis()],
                )
                .unwrap();
            transaction.commit().unwrap();
        }
        let workspace_id = "shared-upgrade-workspace";
        let device_id = "shared-upgrade-device";
        connection.execute("INSERT INTO workspaces(id,name,timezone,storage_mode,created_at,updated_at,version) VALUES (?1,'共享升级','Asia/Shanghai','cloud',1,1,1)",[workspace_id]).unwrap();
        connection.execute("INSERT INTO devices(id,workspace_id,device_name,platform,app_version,last_seen_at,created_at) VALUES (?1,?2,'测试设备','test','1',1,1)",params![device_id,workspace_id]).unwrap();
        connection.execute("INSERT INTO unassigned_sessions(id,workspace_id,work_date,state,threshold_seconds,duration_seconds,first_started_at,created_at,updated_at,version) VALUES ('active-session',?1,'2026-10-06','collecting',300,60,1,1,1,2)",[workspace_id]).unwrap();
        connection.execute("INSERT INTO unassigned_sessions(id,workspace_id,work_date,state,threshold_seconds,duration_seconds,first_started_at,last_ended_at,resolution_type,resolved_at,created_at,updated_at,version) VALUES ('resolved-session',?1,'2026-10-06','resolved',300,120,1,121000,'work',121000,1,121000,3)",[workspace_id]).unwrap();
        connection.execute("INSERT INTO time_entries(id,workspace_id,work_date,kind,source_type,state,label_snapshot,started_at,ended_at,duration_seconds,origin_unassigned_session_id,created_at,updated_at,version) VALUES ('history-entry',?1,'2026-10-06','work','unassigned','ended','历史',1,121000,120,'resolved-session',1,121000,1)",[workspace_id]).unwrap();
        connection.execute("UPDATE unassigned_sessions SET generated_entry_id='history-entry' WHERE id='resolved-session'",[]).unwrap();
        connection.execute("INSERT INTO sync_outbox(operation_id,workspace_id,device_id,operation_type,entity_type,entity_id,payload_json,state,attempt_count,created_at) VALUES ('keep-task-op',?1,?2,'task.update','task','task-1','{}','pending',0,1)",params![workspace_id,device_id]).unwrap();
        drop(connection);

        let connection = migrate_through_legacy_cleanup(&path);
        drop(connection);
        // Startup coordination runs after the final migration; emulate it here so
        // the assertion reflects the state a real launch observes.
        recover_local_tracking_state(&Database { path: path.clone() }).unwrap();
        let connection = Connection::open(&path).unwrap();
        let active: (String, String) = connection.query_row("SELECT shared_source,migration_state FROM unassigned_sessions WHERE id='active-session'",[],|row| Ok((row.get(0)?,row.get(1)?))).unwrap();
        let history: (String, Option<String>) = connection.query_row("SELECT state,generated_entry_id FROM unassigned_sessions WHERE id='resolved-session'",[],|row| Ok((row.get(0)?,row.get(1)?))).unwrap();
        let kept: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sync_outbox WHERE operation_id='keep-task-op'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            active,
            ("legacy_local".to_string(), "superseded".to_string())
        );
        assert_eq!(
            history,
            ("resolved".to_string(), Some("history-entry".to_string()))
        );
        assert_eq!(kept, 1);
    }

    #[test]
    fn calendar_day_migration_preserves_history_and_initializes_chain_metadata() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("calendar-day-upgrade.sqlite3");
        let mut connection = Connection::open(&path).unwrap();
        configure_connection(&connection).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                   version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at INTEGER NOT NULL
                 );",
            )
            .unwrap();
        for migration in &MIGRATIONS[..9] {
            let transaction = connection.transaction().unwrap();
            transaction.execute_batch(migration.sql).unwrap();
            transaction
                .execute(
                    "INSERT INTO schema_migrations(version,name,applied_at) VALUES (?1,?2,?3)",
                    params![migration.version, migration.name, now_millis()],
                )
                .unwrap();
            transaction.commit().unwrap();
        }
        connection.execute("INSERT INTO workspaces(id,name,timezone,storage_mode,created_at,updated_at,version) VALUES ('calendar-workspace','日期升级','Asia/Shanghai','local',1,1,1)",[]).unwrap();
        connection.execute("INSERT INTO devices(id,workspace_id,device_name,platform,app_version,last_seen_at,created_at) VALUES ('calendar-device','calendar-workspace','设备','test','1',1,1)",[]).unwrap();
        connection.execute("INSERT INTO device_settings(key,value_json,updated_at) VALUES ('storage_mode','\"cloud\"',1)",[]).unwrap();
        connection.execute("INSERT INTO subjects(id,workspace_id,name,sort_order,created_at,updated_at,version) VALUES ('calendar-subject','calendar-workspace','默认',10,1,1,1)",[]).unwrap();
        connection.execute("INSERT INTO tasks(id,workspace_id,subject_id,title,status,source_type,sort_order,created_at,updated_at,version) VALUES ('calendar-task','calendar-workspace','calendar-subject','历史事项','open','manual',10,1,1,1)",[]).unwrap();
        connection.execute("INSERT INTO time_entries(id,workspace_id,work_date,kind,source_type,state,label_snapshot,started_at,ended_at,duration_seconds,created_at,updated_at,version) VALUES ('history','calendar-workspace','2026-10-05','work','manual','ended','历史',1000,61000,60,1000,61000,1)",[]).unwrap();
        connection.execute("INSERT INTO time_allocations(id,workspace_id,entry_id,task_id,minutes,created_at,updated_at,version) VALUES ('history-allocation','calendar-workspace','history','calendar-task',1,1000,61000,1)",[]).unwrap();
        connection.execute("INSERT INTO time_entries(id,workspace_id,work_date,kind,source_type,state,label_snapshot,started_at,duration_seconds,created_at,updated_at,version) VALUES ('active','calendar-workspace','2026-10-06','work','timer','paused','活动',100000,60,100000,160000,2)",[]).unwrap();
        connection.execute("INSERT INTO unassigned_sessions(id,workspace_id,work_date,state,threshold_seconds,duration_seconds,first_started_at,created_at,updated_at,version) VALUES ('unassigned','calendar-workspace','2026-10-06','awaiting_resolution',300,600,100000,100000,700000,2)",[]).unwrap();
        drop(connection);

        let database = Database::initialize_at(path).unwrap();
        let connection = database.open().unwrap();
        let history: (String, i64) = connection
            .query_row(
                "SELECT work_date,duration_seconds FROM time_entries WHERE id='history'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let active: (String, i64) = connection
            .query_row(
                "SELECT timer_chain_id,last_continuous_at FROM time_entries WHERE id='active'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let unassigned_last: i64 = connection
            .query_row(
                "SELECT last_continuous_at FROM unassigned_sessions WHERE id='unassigned'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let allocation: (String, String, i64) = connection.query_row("SELECT entry_id,task_id,minutes FROM time_allocations WHERE id='history-allocation'",[],|row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).unwrap();
        assert_eq!(history, ("2026-10-05".to_string(), 60));
        assert_eq!(active, ("active".to_string(), 160000));
        assert_eq!(unassigned_last, 700000);
        assert_eq!(
            allocation,
            ("history".to_string(), "calendar-task".to_string(), 1)
        );
    }

    #[test]
    fn simplified_anchor_migration_preserves_history_and_collapses_legacy_lifecycle() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("simplified-anchor-upgrade.sqlite3");
        let mut connection = Connection::open(&path).unwrap();
        configure_connection(&connection).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                   version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at INTEGER NOT NULL
                 );",
            )
            .unwrap();
        for migration in &MIGRATIONS[..10] {
            let transaction = connection.transaction().unwrap();
            transaction.execute_batch(migration.sql).unwrap();
            transaction
                .execute(
                    "INSERT INTO schema_migrations(version,name,applied_at) VALUES (?1,?2,?3)",
                    params![migration.version, migration.name, now_millis()],
                )
                .unwrap();
            transaction.commit().unwrap();
        }

        connection.execute_batch(
            "INSERT INTO workspaces(id,name,timezone,storage_mode,created_at,updated_at,version)
               VALUES ('anchor-workspace','锚点升级','America/Los_Angeles','cloud',1,1,1),
                      ('timer-workspace','计时升级','Pacific/Auckland','cloud',1,1,1);
             INSERT INTO devices(id,workspace_id,device_name,platform,app_version,last_seen_at,created_at)
               VALUES ('anchor-device','anchor-workspace','设备 A','test','1',1,1),
                      ('timer-device','timer-workspace','设备 B','test','1',1,1);
             INSERT INTO subjects(id,workspace_id,name,sort_order,created_at,updated_at,version)
               VALUES ('anchor-subject','anchor-workspace','默认',10,1,1,1);
             INSERT INTO tasks(id,workspace_id,subject_id,title,status,source_type,sort_order,created_at,updated_at,version)
               VALUES ('anchor-task','anchor-workspace','anchor-subject','历史事项','open','manual',10,1,1,1);
             INSERT INTO time_entries(id,workspace_id,work_date,kind,source_type,state,label_snapshot,started_at,ended_at,duration_seconds,created_at,updated_at,version)
               VALUES ('history-entry','anchor-workspace','2026-10-04','work','unassigned','ended','历史工时',1000,61000,60,1000,61000,1);
             INSERT INTO time_allocations(id,workspace_id,entry_id,task_id,minutes,created_at,updated_at,version)
               VALUES ('history-allocation','anchor-workspace','history-entry','anchor-task',1,1000,61000,1);
             INSERT INTO unassigned_sessions(id,workspace_id,work_date,state,threshold_seconds,duration_seconds,first_started_at,last_ended_at,resolution_type,generated_entry_id,resolved_at,created_at,updated_at,version,shared_source,migration_state,last_continuous_at)
               VALUES ('resolved-session','anchor-workspace','2026-10-04','resolved',300,60,1000,61000,'work','history-entry',61000,1000,61000,3,'cloud','ready',61000),
                      ('historical-pending','anchor-workspace','2026-10-05','awaiting_resolution',300,600,100000,700000,NULL,NULL,NULL,100000,700000,2,'cloud','ready',700000),
                      ('active-anchor','anchor-workspace','2026-10-06','collecting',300,900,800000,900000,NULL,NULL,NULL,800000,950000,4,'cloud','ready',950000),
                      ('timer-cleared','timer-workspace','2026-10-07','awaiting_resolution',300,300,1000000,1300000,NULL,NULL,NULL,1000000,1300000,5,'cloud','ready',1300000);
             INSERT INTO unassigned_segments(id,workspace_id,session_id,sequence_no,started_at,ended_at,duration_seconds)
               VALUES ('historical-segment','anchor-workspace','historical-pending',1,100000,700000,600),
                      ('old-closed-segment','anchor-workspace','active-anchor',1,800000,900000,100),
                      ('active-open-segment','anchor-workspace','active-anchor',2,900000,NULL,800);
             INSERT INTO time_entries(id,workspace_id,work_date,kind,source_type,state,label_snapshot,started_at,duration_seconds,created_at,updated_at,version,timer_chain_id,last_continuous_at)
               VALUES ('running-timer','timer-workspace','2026-10-07','work','timer','running','活动事项',1400000,0,1400000,1400000,1,'running-timer',1400000);
             INSERT INTO sync_outbox(operation_id,workspace_id,device_id,operation_type,entity_type,entity_id,base_version,payload_json,state,attempt_count,created_at,payload_version,coalesced_count)
               VALUES ('pause-op','anchor-workspace','anchor-device','unassigned_session_pause','unassigned_session','active-anchor',1,'{}','pending',0,1,2,1),
                      ('resume-op','anchor-workspace','anchor-device','unassigned_session_resume','unassigned_session','active-anchor',2,'{}','failed',1,2,3,1),
                      ('await-op','anchor-workspace','anchor-device','unassigned_session_awaiting_resolution','unassigned_session','active-anchor',3,'{}','conflict',1,3,4,1),
                      ('sending-op','anchor-workspace','anchor-device','unassigned_session_pause','unassigned_session','active-anchor',4,'{}','sending',1,4,5,1),
                      ('keep-op','anchor-workspace','anchor-device','task.update','task','anchor-task',1,'{}','pending',0,5,2,1);"
        ).unwrap();
        drop(connection);

        let mut upgrade_connection = Connection::open(&path).unwrap();
        configure_connection(&upgrade_connection).unwrap();
        apply_migrations_before_legacy_cleanup(&mut upgrade_connection).unwrap();
        drop(upgrade_connection);
        let database = Database { path };
        let connection = database.open().unwrap();

        let resolved: (String, i64, Option<String>) = connection.query_row(
            "SELECT state,duration_seconds,generated_entry_id FROM unassigned_sessions WHERE id='resolved-session'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).unwrap();
        assert_eq!(
            resolved,
            (
                "resolved".to_string(),
                60,
                Some("history-entry".to_string())
            )
        );
        let history_entry: (i64, i64) = connection
            .query_row(
                "SELECT duration_seconds,ended_at FROM time_entries WHERE id='history-entry'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(history_entry, (60, 61000));
        let history_allocation: i64 = connection
            .query_row(
                "SELECT minutes FROM time_allocations WHERE id='history-allocation'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(history_allocation, 1);

        let historical: (String, i64, Option<String>, String) = connection.query_row(
            "SELECT state,duration_seconds,resolution_type,migration_state FROM unassigned_sessions WHERE id='historical-pending'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).unwrap();
        assert_eq!(
            historical,
            (
                "discarded".to_string(),
                0,
                Some("discard".to_string()),
                "superseded".to_string()
            )
        );
        let historical_segments: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM unassigned_segments WHERE session_id='historical-pending'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(historical_segments, 1);
        let generated_from_historical: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM time_entries WHERE origin_unassigned_session_id='historical-pending'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(generated_from_historical, 0);

        let active: (String, i64, i64, i64, Option<i64>) = connection.query_row(
            "SELECT state,duration_seconds,first_started_at,version,last_ended_at FROM unassigned_sessions WHERE id='active-anchor'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        ).unwrap();
        assert_eq!(active, ("collecting".to_string(), 0, 900000, 4, None));
        let active_segments: Vec<(i64, i64, Option<i64>, i64)> = connection.prepare(
            "SELECT sequence_no,started_at,ended_at,duration_seconds FROM unassigned_segments WHERE session_id='active-anchor' ORDER BY sequence_no",
        ).unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
            .unwrap().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(active_segments, vec![(1, 900000, None, 0)]);

        let cleared: (String, Option<String>, i64, String) = connection.query_row(
            "SELECT state,resolution_type,duration_seconds,migration_state FROM unassigned_sessions WHERE id='timer-cleared'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).unwrap();
        assert_eq!(
            cleared,
            (
                "discarded".to_string(),
                Some("discard".to_string()),
                0,
                "superseded".to_string()
            )
        );

        let outbox_operations = collect_strings(
            &connection,
            "SELECT operation_id FROM sync_outbox ORDER BY operation_id",
        )
        .unwrap();
        assert_eq!(outbox_operations, vec!["keep-op", "sending-op"]);
        assert_eq!(
            connection
                .query_row(
                    "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            LEGACY_SYNC_CLEANUP_MIGRATION_VERSION - 1
        );

        // Deployment rollback keeps the migrated database in place. Rehearse
        // the previous client's read path against that database and verify
        // that final work and its allocation remain intact while the old
        // lifecycle operations stay retired.
        let previous_client_entry: (String, i64, i64) = connection
            .query_row(
                "SELECT state,duration_seconds,ended_at FROM time_entries WHERE id='history-entry'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(previous_client_entry, ("ended".to_string(), 60, 61000));
        let previous_client_allocation: i64 = connection
            .query_row(
                "SELECT minutes FROM time_allocations WHERE entry_id='history-entry'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(previous_client_allocation, 1);
        let retired_lifecycle_operations: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sync_outbox
                 WHERE operation_type IN (
                   'unassigned_session_pause',
                   'unassigned_session_resume',
                   'unassigned_session_awaiting_resolution'
                 ) AND state IN ('pending','failed','conflict')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(retired_lifecycle_operations, 0);
    }

    #[test]
    fn repair_unassigned_active_index_removes_stale_workspace_constraint() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("repair-unassigned-index.sqlite3");
        let mut connection = Connection::open(&path).unwrap();
        configure_connection(&connection).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                   version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at INTEGER NOT NULL
                 );",
            )
            .unwrap();
        for migration in &MIGRATIONS[..11] {
            let transaction = connection.transaction().unwrap();
            transaction.execute_batch(migration.sql).unwrap();
            transaction
                .execute(
                    "INSERT INTO schema_migrations(version,name,applied_at) VALUES (?1,?2,?3)",
                    params![migration.version, migration.name, now_millis()],
                )
                .unwrap();
            transaction.commit().unwrap();
        }

        connection
            .execute_batch(
                "DROP INDEX IF EXISTS uq_unassigned_sessions_active_date;
                 CREATE UNIQUE INDEX uq_unassigned_sessions_active
                   ON unassigned_sessions(workspace_id)
                   WHERE state IN ('collecting', 'awaiting_resolution');
                 INSERT INTO workspaces(
                   id,name,timezone,storage_mode,created_at,updated_at,version
                 ) VALUES (
                   'repair-workspace','索引修复','Asia/Shanghai','local',1,1,1
                 );
                 INSERT INTO unassigned_sessions(
                   id,workspace_id,work_date,state,threshold_seconds,duration_seconds,
                   first_started_at,created_at,updated_at,version
                 ) VALUES (
                   'old-day','repair-workspace','2000-01-01','awaiting_resolution',
                   300,60,1,1,60001,1
                 );
                 INSERT INTO app_metadata(key,value)
                   VALUES ('initial_demo_tasks_seeded','1');",
            )
            .unwrap();
        drop(connection);

        let database = Database::initialize_at(path).unwrap();
        assert_eq!(
            database.status().unwrap().schema_version,
            latest_migration_version()
        );
        let connection = database.open().unwrap();

        let stale_index_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type='index' AND name='uq_unassigned_sessions_active'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let date_index_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type='index' AND name='uq_unassigned_sessions_active_date'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stale_index_count, 0);
        assert_eq!(date_index_count, 1);

        connection
            .execute(
                "INSERT INTO unassigned_sessions(
                   id,workspace_id,work_date,state,threshold_seconds,duration_seconds,
                   first_started_at,created_at,updated_at,version
                 ) VALUES (
                   'new-day','repair-workspace','2000-01-02','collecting',
                   300,0,70000,70000,70000,1
                 )",
                [],
            )
            .unwrap();
        let duplicate_result = connection.execute(
            "INSERT INTO unassigned_sessions(
               id,workspace_id,work_date,state,threshold_seconds,duration_seconds,
               first_started_at,created_at,updated_at,version
             ) VALUES (
               'same-day','repair-workspace','2000-01-02','awaiting_resolution',
               300,10,80000,80000,90000,1
             )",
            [],
        );
        assert!(duplicate_result.is_err());
    }

    #[test]
    fn calendar_day_migration_defaults_blank_timezone_and_rejects_invalid_timezone() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("calendar-timezone-upgrade.sqlite3");
        let mut connection = Connection::open(&path).unwrap();
        configure_connection(&connection).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                   version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at INTEGER NOT NULL
                 );",
            )
            .unwrap();
        for migration in &MIGRATIONS[..9] {
            let transaction = connection.transaction().unwrap();
            transaction.execute_batch(migration.sql).unwrap();
            transaction
                .execute(
                    "INSERT INTO schema_migrations(version,name,applied_at) VALUES (?1,?2,?3)",
                    params![migration.version, migration.name, now_millis()],
                )
                .unwrap();
            transaction.commit().unwrap();
        }
        connection.execute("INSERT INTO workspaces(id,name,timezone,storage_mode,created_at,updated_at,version) VALUES ('blank-workspace','空时区','','local',1,1,1)",[]).unwrap();
        connection.execute("INSERT INTO devices(id,workspace_id,device_name,platform,app_version,last_seen_at,created_at) VALUES ('blank-device','blank-workspace','设备','test','1',1,1)",[]).unwrap();
        connection.execute("INSERT INTO subjects(id,workspace_id,name,sort_order,created_at,updated_at,version) VALUES ('blank-subject','blank-workspace','默认',10,1,1,1)",[]).unwrap();
        drop(connection);

        let database = Database::initialize_at(path).unwrap();
        let timezone: String = database
            .open()
            .unwrap()
            .query_row(
                "SELECT timezone FROM workspaces WHERE id='blank-workspace'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(timezone, crate::work_calendar::DEFAULT_TIMEZONE);

        let invalid_path = directory.path().join("calendar-invalid-timezone.sqlite3");
        let invalid = Database::initialize_at(invalid_path.clone()).unwrap();
        invalid
            .open()
            .unwrap()
            .execute("UPDATE workspaces SET timezone='Mars/Olympus'", [])
            .unwrap();
        drop(invalid);
        let error = Database::initialize_at(invalid_path).unwrap_err();
        assert!(error.contains("无效的工作空间时区"));
    }

    #[test]
    fn local_startup_recovery_preserves_history_and_recovers_active_states_at_confirmed_boundaries()
    {
        use chrono::{NaiveDateTime, TimeZone};
        let timezone: chrono_tz::Tz = "Asia/Shanghai".parse().unwrap();
        let millis = |value: &str| {
            timezone
                .from_local_datetime(
                    &NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S").unwrap(),
                )
                .single()
                .unwrap()
                .timestamp_millis()
        };
        let directory = tempdir().unwrap();
        let database =
            Database::initialize_at(directory.path().join("startup-recovery.sqlite3")).unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let task_id: String = connection
            .query_row(
                "SELECT id FROM tasks WHERE parent_id IS NULL LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        connection
            .execute("DELETE FROM unassigned_segments", [])
            .unwrap();
        connection
            .execute("DELETE FROM unassigned_sessions", [])
            .unwrap();
        connection.execute("DELETE FROM time_segments", []).unwrap();
        connection
            .execute("DELETE FROM time_allocations", [])
            .unwrap();
        connection.execute("DELETE FROM time_entries", []).unwrap();
        let history_started = millis("2026-10-04 10:00:00");
        let history_ended = millis("2026-10-04 10:10:00");
        connection.execute("INSERT INTO time_entries(id,workspace_id,work_date,kind,source_type,state,label_snapshot,started_at,ended_at,duration_seconds,created_at,updated_at,version) VALUES ('history',?1,'2026-10-04','work','manual','ended','历史',?2,?3,600,?2,?3,1)",params![workspace_id,history_started,history_ended]).unwrap();
        let paused_started = millis("2026-10-05 23:00:00");
        connection.execute("INSERT INTO time_entries(id,workspace_id,work_date,kind,source_type,state,default_task_id,label_snapshot,started_at,duration_seconds,created_at,updated_at,version,timer_chain_id,last_continuous_at) VALUES ('paused',?1,'2026-10-05','work','timer','paused',?2,'暂停',?3,300,?3,?4,1,'paused',?4)",params![workspace_id,task_id,paused_started,millis("2026-10-05 23:05:00")]).unwrap();
        let pending_started = millis("2026-10-05 22:00:00");
        let pending_ended = millis("2026-10-05 22:10:00");
        connection.execute("INSERT INTO unassigned_sessions(id,workspace_id,work_date,state,threshold_seconds,duration_seconds,first_started_at,last_ended_at,prompted_at,created_at,updated_at,version,shared_source,migration_state,last_continuous_at) VALUES ('pending',?1,'2026-10-05','awaiting_resolution',300,600,?2,?3,?3,?2,?3,1,'local','ready',?3)",params![workspace_id,pending_started,pending_ended]).unwrap();
        connection.execute("INSERT INTO unassigned_segments(id,workspace_id,session_id,sequence_no,started_at,ended_at,duration_seconds) VALUES ('pending-segment',?1,'pending',1,?2,?3,600)",params![workspace_id,pending_started,pending_ended]).unwrap();
        drop(connection);

        let now = millis("2026-10-06 08:00:00");
        recover_local_tracking_state_at(&database, now).unwrap();
        let connection = database.open().unwrap();
        let history: (String, i64, i64) = connection
            .query_row(
                "SELECT work_date,duration_seconds,ended_at FROM time_entries WHERE id='history'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(history, ("2026-10-04".to_string(), 600, history_ended));
        let timer_rows = connection.prepare("SELECT work_date,state,duration_seconds,started_at FROM time_entries WHERE timer_chain_id='paused' ORDER BY work_date").unwrap().query_map([],|row|Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,i64>(2)?,row.get::<_,i64>(3)?))).unwrap().collect::<Result<Vec<_>,_>>().unwrap();
        assert_eq!(
            timer_rows,
            vec![
                (
                    "2026-10-05".to_string(),
                    "ended".to_string(),
                    300,
                    paused_started
                ),
                ("2026-10-06".to_string(), "paused".to_string(), 0, now)
            ]
        );
        let pending: (String,i64,String) = connection.query_row("SELECT work_date,duration_seconds,state FROM unassigned_sessions WHERE id='pending'",[],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).unwrap();
        assert_eq!(
            pending,
            ("2026-10-05".to_string(), 0, "discarded".to_string())
        );
        let today_sessions: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM unassigned_sessions WHERE work_date='2026-10-06'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(today_sessions, 0);
    }

    #[test]
    fn recurring_task_constraints_and_date_index_are_available() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(directory.path().join("test.sqlite3")).unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let task_id: String = connection
            .query_row(
                "SELECT id FROM tasks WHERE parent_id IS NULL ORDER BY sort_order LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let now = now_millis();
        connection
            .execute(
                "INSERT INTO task_recurrence_rules(
                   id, workspace_id, task_id, frequency, weekdays_mask, effective_start,
                   created_at, updated_at, version
                 ) VALUES (?1, ?2, ?3, 'daily', NULL, '2026-09-29', ?4, ?4, 1)",
                params![Uuid::now_v7().to_string(), workspace_id, task_id, now],
            )
            .unwrap();
        let duplicate_open = connection.execute(
            "INSERT INTO task_recurrence_rules(
               id, workspace_id, task_id, frequency, weekdays_mask, effective_start,
               created_at, updated_at, version
             ) VALUES (?1, ?2, ?3, 'weekdays', NULL, '2026-09-30', ?4, ?4, 1)",
            params![Uuid::now_v7().to_string(), workspace_id, task_id, now],
        );
        assert!(duplicate_open.is_err());

        connection
            .execute(
                "INSERT INTO task_occurrences(
                   workspace_id, task_id, occurrence_date, origin, status,
                   created_at, updated_at, version
                 ) VALUES (?1, ?2, '2026-09-29', 'scheduled', 'open', ?3, ?3, 1)",
                params![workspace_id, task_id, now],
            )
            .unwrap();
        let duplicate_occurrence = connection.execute(
            "INSERT INTO task_occurrences(
               workspace_id, task_id, occurrence_date, origin, status,
               created_at, updated_at, version
             ) VALUES (?1, ?2, '2026-09-29', 'manual', 'open', ?3, ?3, 1)",
            params![workspace_id, task_id, now],
        );
        assert!(duplicate_occurrence.is_err());

        let query_plan: String = connection
            .query_row(
                "EXPLAIN QUERY PLAN SELECT task_id FROM task_occurrences
                 WHERE workspace_id = ?1 AND occurrence_date = '2026-09-29'",
                [&workspace_id],
                |row| row.get(3),
            )
            .unwrap();
        assert!(query_plan.contains("idx_task_occurrences_workspace_date"));
    }

    #[test]
    fn database_constraints_prevent_parallel_active_timers() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(directory.path().join("test.sqlite3")).unwrap();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let now = now_millis();
        connection
            .execute(
                "INSERT INTO time_entries(id, workspace_id, work_date, kind, source_type, state, label_snapshot, started_at, duration_seconds, created_at, updated_at, version)
                 VALUES (?1, ?2, '2026-09-24', 'work', 'timer', 'running', 'A', ?3, 0, ?3, ?3, 1)",
                params![Uuid::now_v7().to_string(), workspace_id, now],
            )
            .unwrap();
        let duplicate = connection.execute(
            "INSERT INTO time_entries(id, workspace_id, work_date, kind, source_type, state, label_snapshot, started_at, duration_seconds, created_at, updated_at, version)
             VALUES (?1, ?2, '2026-09-24', 'work', 'timer', 'paused', 'B', ?3, 0, ?3, ?3, 1)",
            params![Uuid::now_v7().to_string(), workspace_id, now],
        );
        assert!(duplicate.is_err());
    }

    #[test]
    fn automation_hook_migration_enforces_actual_event_uniqueness() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(directory.path().join("test.sqlite3")).unwrap();
        let connection = database.open().unwrap();
        let hook_id = Uuid::now_v7().to_string();
        let now = now_millis();
        connection
            .execute(
                "INSERT INTO device_hooks(
                   id, name, event_type, action_type, action_config_json, timeout_seconds,
                   enabled, sort_order, created_at, updated_at
                 ) VALUES (?1, 'Test', 'timer.started', 'uri', '{\"uriTemplate\":\"test://open\"}', 10, 1, 10, ?2, ?2)",
                params![hook_id, now],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO device_hook_runs(
                   id, hook_id, hook_name_snapshot, event_id, event_type, is_test, status, created_at
                 ) VALUES (?1, ?2, 'Test', 'timer.started:one', 'timer.started', 0, 'queued', ?3)",
                params![Uuid::now_v7().to_string(), hook_id, now],
            )
            .unwrap();
        let duplicate = connection.execute(
            "INSERT INTO device_hook_runs(
               id, hook_id, hook_name_snapshot, event_id, event_type, is_test, status, created_at
             ) VALUES (?1, ?2, 'Test', 'timer.started:one', 'timer.started', 0, 'queued', ?3)",
            params![Uuid::now_v7().to_string(), hook_id, now],
        );
        assert!(duplicate.is_err());

        connection
            .execute(
                "INSERT INTO device_hook_runs(
                   id, hook_id, hook_name_snapshot, event_id, event_type, is_test, status, created_at
                 ) VALUES (?1, ?2, 'Test', 'timer.started:one', 'timer.started', 1, 'queued', ?3)",
                params![Uuid::now_v7().to_string(), hook_id, now],
            )
            .unwrap();
    }

    #[test]
    fn automation_hook_migration_upgrades_version_three_database() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("test.sqlite3");
        let mut connection = Connection::open(&path).unwrap();
        configure_connection(&connection).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                   version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at INTEGER NOT NULL
                 );",
            )
            .unwrap();
        for migration in &MIGRATIONS[..3] {
            let transaction = connection.transaction().unwrap();
            transaction.execute_batch(migration.sql).unwrap();
            transaction
                .execute(
                    "INSERT INTO schema_migrations(version, name, applied_at) VALUES (?1, ?2, ?3)",
                    params![migration.version, migration.name, now_millis()],
                )
                .unwrap();
            transaction.commit().unwrap();
        }
        drop(connection);

        let database = Database::initialize_at(path).unwrap();
        let status = database.status().unwrap();
        assert_eq!(status.schema_version, latest_migration_version());
        assert!(status.tables.iter().any(|table| table == "device_hooks"));
        assert!(status
            .tables
            .iter()
            .any(|table| table == "device_hook_runs"));
    }
}
