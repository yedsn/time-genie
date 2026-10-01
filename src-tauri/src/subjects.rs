use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::database::Database;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubjectDto {
    pub id: String,
    pub name: String,
    pub sort_order: i64,
    pub open_task_count: i64,
    pub version: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubjectCreateResult {
    pub subject: SubjectDto,
    pub already_exists: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubjectCreateRequest {
    pub name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubjectRenameRequest {
    pub subject_id: String,
    pub name: String,
    pub expected_version: i64,
}

pub fn list_subjects(database: &Database) -> Result<Vec<SubjectDto>, String> {
    let connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    let mut statement = connection
        .prepare(
            "SELECT s.id, s.name, s.sort_order,
                    (SELECT COUNT(*) FROM tasks t WHERE t.subject_id = s.id AND t.status = 'open' AND t.deleted_at IS NULL),
                    s.version
             FROM subjects s
             WHERE s.workspace_id = ?1 AND s.deleted_at IS NULL
             ORDER BY s.sort_order, s.created_at",
        )
        .map_err(|error| error.to_string())?;
    let result = statement
        .query_map([workspace_id], subject_from_row)
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string());
    result
}

pub fn create_subject(
    database: &Database,
    request: SubjectCreateRequest,
) -> Result<SubjectCreateResult, String> {
    let name = request.name.trim();
    if name.is_empty() {
        return Err("VALIDATION_ERROR: 主体名称不能为空".to_string());
    }
    let connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    if let Some(subject) = find_subject_by_name(&connection, &workspace_id, name)? {
        return Ok(SubjectCreateResult {
            subject,
            already_exists: true,
        });
    }
    let id = Uuid::now_v7().to_string();
    let sort_order: i64 = connection
        .query_row(
            "SELECT COALESCE(MAX(sort_order), 0) + 10 FROM subjects WHERE workspace_id = ?1 AND deleted_at IS NULL",
            [&workspace_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let now = now_millis();
    connection
        .execute(
            "INSERT INTO subjects(id, workspace_id, name, sort_order, created_at, updated_at, version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5, 1)",
            params![id, workspace_id, name, sort_order, now],
        )
        .map_err(|error| error.to_string())?;
    bump_revision(&connection)?;
    Ok(SubjectCreateResult {
        subject: load_subject(&connection, &id)?,
        already_exists: false,
    })
}

pub fn rename_subject(
    database: &Database,
    request: SubjectRenameRequest,
) -> Result<SubjectDto, String> {
    let name = request.name.trim();
    if name.is_empty() {
        return Err("VALIDATION_ERROR: 主体名称不能为空".to_string());
    }
    let connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    if let Some(existing) = find_subject_by_name(&connection, &workspace_id, name)? {
        if existing.id != request.subject_id {
            return Err("VALIDATION_ERROR: 主体名称已存在".to_string());
        }
    }
    let changed = connection
        .execute(
            "UPDATE subjects SET name = ?1, updated_at = ?2, version = version + 1
             WHERE id = ?3 AND workspace_id = ?4 AND version = ?5 AND deleted_at IS NULL",
            params![
                name,
                now_millis(),
                request.subject_id,
                workspace_id,
                request.expected_version
            ],
        )
        .map_err(|error| error.to_string())?;
    if changed == 0 {
        return Err("VERSION_CONFLICT: 主体已被其他窗口或设备更新".to_string());
    }
    bump_revision(&connection)?;
    load_subject(&connection, &request.subject_id)
}

fn subject_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SubjectDto> {
    Ok(SubjectDto {
        id: row.get(0)?,
        name: row.get(1)?,
        sort_order: row.get(2)?,
        open_task_count: row.get(3)?,
        version: row.get(4)?,
    })
}

fn load_subject(connection: &rusqlite::Connection, id: &str) -> Result<SubjectDto, String> {
    connection
        .query_row(
            "SELECT s.id, s.name, s.sort_order,
                    (SELECT COUNT(*) FROM tasks t WHERE t.subject_id = s.id AND t.status = 'open' AND t.deleted_at IS NULL),
                    s.version
             FROM subjects s WHERE s.id = ?1 AND s.deleted_at IS NULL",
            [id],
            subject_from_row,
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "NOT_FOUND: 主体不存在".to_string())
}

fn find_subject_by_name(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    name: &str,
) -> Result<Option<SubjectDto>, String> {
    connection
        .query_row(
            "SELECT s.id, s.name, s.sort_order,
                    (SELECT COUNT(*) FROM tasks t WHERE t.subject_id = s.id AND t.status = 'open' AND t.deleted_at IS NULL),
                    s.version
             FROM subjects s
             WHERE s.workspace_id = ?1 AND lower(s.name) = lower(?2) AND s.deleted_at IS NULL",
            params![workspace_id, name],
            subject_from_row,
        )
        .optional()
        .map_err(|error| error.to_string())
}

fn workspace_id(connection: &rusqlite::Connection) -> Result<String, String> {
    connection
        .query_row(
            "SELECT id FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())
}

fn bump_revision(connection: &rusqlite::Connection) -> Result<(), String> {
    connection
        .execute(
            "UPDATE app_metadata SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT) WHERE key = 'global_revision'",
            [],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[tauri::command]
pub fn subject_list(database: tauri::State<'_, Database>) -> Result<Vec<SubjectDto>, String> {
    list_subjects(&database)
}

#[tauri::command]
pub fn subject_create(
    database: tauri::State<'_, Database>,
    request: SubjectCreateRequest,
) -> Result<SubjectCreateResult, String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let result = create_subject(&database, request)?;
    if !result.already_exists {
        crate::cloud_sync::enqueue_entity(
            &database,
            "subject_create",
            "subject",
            Some(&result.subject.id),
            None,
            None,
        )?;
    }
    Ok(result)
}

#[tauri::command]
pub fn subject_rename(
    database: tauri::State<'_, Database>,
    request: SubjectRenameRequest,
) -> Result<SubjectDto, String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let base_version = request.expected_version;
    let result = rename_subject(&database, request)?;
    crate::cloud_sync::enqueue_entity(
        &database,
        "subject_rename",
        "subject",
        Some(&result.id),
        Some(base_version),
        None,
    )?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn creates_existing_subject_idempotently_and_checks_rename_version() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(directory.path().join("subjects.sqlite3")).unwrap();
        let created = create_subject(
            &database,
            SubjectCreateRequest {
                name: "研发支持".to_string(),
            },
        )
        .unwrap();
        assert!(!created.already_exists);
        let existing = create_subject(
            &database,
            SubjectCreateRequest {
                name: " 研发支持 ".to_string(),
            },
        )
        .unwrap();
        assert!(existing.already_exists);
        assert_eq!(created.subject.id, existing.subject.id);
        assert!(rename_subject(
            &database,
            SubjectRenameRequest {
                subject_id: created.subject.id,
                name: "新名称".to_string(),
                expected_version: 99,
            }
        )
        .is_err());
    }
}
