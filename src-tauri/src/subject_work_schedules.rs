use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::database::Database;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkScheduleSegmentDto {
    pub start: String,
    pub end: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkScheduleDayDto {
    pub id: String,
    pub weekday: i64,
    pub enabled: bool,
    pub segments: Vec<WorkScheduleSegmentDto>,
    pub version: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkScheduleGetRequest {
    pub subject_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkScheduleSaveRequest {
    pub subject_id: String,
    pub days: Vec<WorkScheduleDayDto>,
}

pub(crate) fn insert_default_schedule(
    transaction: &Transaction<'_>,
    workspace_id: &str,
    subject_id: &str,
    now: i64,
) -> Result<Vec<String>, String> {
    let mut schedule_ids = Vec::with_capacity(7);
    for weekday in 0..7 {
        let schedule_id = Uuid::now_v7().to_string();
        transaction
            .execute(
                "INSERT INTO subject_work_schedules(id,workspace_id,subject_id,weekday,is_workday,created_at,updated_at,version)
                 VALUES (?1,?2,?3,?4,?5,?6,?6,1)",
                params![schedule_id, workspace_id, subject_id, weekday, (weekday < 5) as i64, now],
            )
            .map_err(|error| error.to_string())?;
        if weekday < 5 {
            for (sequence_no, start_minute, end_minute) in [(1, 600, 720), (2, 810, 1050)] {
                transaction
                    .execute(
                        "INSERT INTO subject_work_schedule_segments(id,workspace_id,schedule_id,sequence_no,start_minute,end_minute)
                         VALUES (?1,?2,?3,?4,?5,?6)",
                        params![Uuid::now_v7().to_string(), workspace_id, schedule_id, sequence_no, start_minute, end_minute],
                    )
                    .map_err(|error| error.to_string())?;
            }
        }
        schedule_ids.push(schedule_id);
    }
    Ok(schedule_ids)
}

pub fn get_schedule(
    database: &Database,
    subject_id: &str,
) -> Result<Vec<WorkScheduleDayDto>, String> {
    let connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    ensure_subject(&connection, &workspace_id, subject_id)?;
    load_schedule(&connection, &workspace_id, subject_id)
}

fn save_schedule_with_cloud_operations(
    database: &Database,
    request: WorkScheduleSaveRequest,
    state: Option<&crate::supabase::StorageModeSnapshot>,
) -> Result<Vec<WorkScheduleDayDto>, String> {
    validate_days(&request.days)?;
    let mut connection = database.open()?;
    let workspace_id = workspace_id(&connection)?;
    ensure_subject(&connection, &workspace_id, &request.subject_id)?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let now = now_millis();
    let mut changed = false;
    for day in request.days {
        let current = load_day(
            &transaction,
            &workspace_id,
            &request.subject_id,
            day.weekday,
        )?
        .ok_or_else(|| format!("NOT_FOUND: 周{}工作时段不存在", day.weekday + 1))?;
        if current.id != day.id || current.version != day.version {
            return Err("VERSION_CONFLICT: 主体工作时段已被其他窗口或设备更新".to_string());
        }
        if current.enabled == day.enabled && current.segments == day.segments {
            continue;
        }
        transaction
            .execute(
                "UPDATE subject_work_schedules SET is_workday=?1,updated_at=?2,version=version+1
                 WHERE id=?3 AND workspace_id=?4 AND subject_id=?5 AND version=?6",
                params![
                    day.enabled as i64,
                    now,
                    day.id,
                    workspace_id,
                    request.subject_id,
                    day.version
                ],
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "DELETE FROM subject_work_schedule_segments WHERE schedule_id=?1",
                [&day.id],
            )
            .map_err(|error| error.to_string())?;
        for (index, segment) in day.segments.iter().enumerate() {
            transaction
                .execute(
                    "INSERT INTO subject_work_schedule_segments(id,workspace_id,schedule_id,sequence_no,start_minute,end_minute)
                     VALUES (?1,?2,?3,?4,?5,?6)",
                    params![Uuid::now_v7().to_string(), workspace_id, day.id, index as i64 + 1, parse_time(&segment.start)?, parse_time(&segment.end)?,],
                )
                .map_err(|error| error.to_string())?;
        }
        if let Some(state) = state {
            crate::cloud_sync::enqueue_entity_in_transaction(
                &transaction,
                state,
                "subject_work_schedule_save",
                "subject_work_schedule",
                Some(&day.id),
                Some(day.version),
                None,
            )?;
        }
        changed = true;
    }
    if changed {
        bump_revision(&transaction)?;
    }
    let result = load_schedule(&transaction, &workspace_id, &request.subject_id)?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}

#[cfg(test)]
pub(crate) fn save_schedule_for_test(
    database: &Database,
    request: WorkScheduleSaveRequest,
) -> Result<Vec<WorkScheduleDayDto>, String> {
    save_schedule_with_cloud_operations(database, request, None)
}

pub(crate) fn work_period_text(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    subject_id: &str,
    weekday: i64,
) -> Result<Option<String>, String> {
    let Some(day) = load_day(connection, workspace_id, subject_id, weekday)? else {
        return Ok(None);
    };
    if !day.enabled {
        return Ok(Some("休息".to_string()));
    }
    Ok(Some(
        day.segments
            .iter()
            .map(|segment| format!("{}-{}", segment.start, segment.end))
            .collect::<Vec<_>>()
            .join(" "),
    ))
}

fn load_schedule(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    subject_id: &str,
) -> Result<Vec<WorkScheduleDayDto>, String> {
    (0..7)
        .map(|weekday| {
            load_day(connection, workspace_id, subject_id, weekday)?
                .ok_or_else(|| "CACHE_ERROR: 主体工作时段不完整".to_string())
        })
        .collect()
}

fn load_day(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    subject_id: &str,
    weekday: i64,
) -> Result<Option<WorkScheduleDayDto>, String> {
    let row = connection
        .query_row(
            "SELECT id,is_workday,version FROM subject_work_schedules
             WHERE workspace_id=?1 AND subject_id=?2 AND weekday=?3",
            params![workspace_id, subject_id, weekday],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, bool>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some((id, enabled, version)) = row else {
        return Ok(None);
    };
    let mut statement = connection
        .prepare("SELECT start_minute,end_minute FROM subject_work_schedule_segments WHERE schedule_id=?1 ORDER BY sequence_no")
        .map_err(|error| error.to_string())?;
    let segments = statement
        .query_map([&id], |row| {
            Ok(WorkScheduleSegmentDto {
                start: format_time(row.get(0)?),
                end: format_time(row.get(1)?),
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(Some(WorkScheduleDayDto {
        id,
        weekday,
        enabled,
        segments,
        version,
    }))
}

fn validate_days(days: &[WorkScheduleDayDto]) -> Result<(), String> {
    if days.len() != 7 {
        return Err("VALIDATION_ERROR: 主体工作时段必须包含周一至周日".to_string());
    }
    let mut weekdays = days.iter().map(|day| day.weekday).collect::<Vec<_>>();
    weekdays.sort_unstable();
    if weekdays != (0..7).collect::<Vec<_>>() {
        return Err("VALIDATION_ERROR: 主体工作时段星期无效".to_string());
    }
    for day in days {
        if day.enabled && day.segments.is_empty() {
            return Err(format!(
                "VALIDATION_ERROR: 周{}至少需要一个工作时段",
                day.weekday + 1
            ));
        }
        if !day.enabled && !day.segments.is_empty() {
            return Err(format!(
                "VALIDATION_ERROR: 周{}休息日不能包含工作时段",
                day.weekday + 1
            ));
        }
        let mut previous_end = None;
        for segment in &day.segments {
            let start = parse_time(&segment.start)?;
            let end = parse_time(&segment.end)?;
            if start >= end {
                return Err(format!(
                    "VALIDATION_ERROR: 周{}结束时间必须晚于开始时间",
                    day.weekday + 1
                ));
            }
            if previous_end.is_some_and(|value| value > start) {
                return Err(format!(
                    "VALIDATION_ERROR: 周{}存在重叠的工作时段",
                    day.weekday + 1
                ));
            }
            previous_end = Some(end);
        }
    }
    Ok(())
}

fn parse_time(value: &str) -> Result<i64, String> {
    let (hours, minutes) = value
        .split_once(':')
        .ok_or_else(|| "VALIDATION_ERROR: 工作时段时间格式无效".to_string())?;
    let hours = hours
        .parse::<i64>()
        .map_err(|_| "VALIDATION_ERROR: 工作时段时间格式无效".to_string())?;
    let minutes = minutes
        .parse::<i64>()
        .map_err(|_| "VALIDATION_ERROR: 工作时段时间格式无效".to_string())?;
    if !(0..=23).contains(&hours) || !(0..=59).contains(&minutes) {
        return Err("VALIDATION_ERROR: 工作时段时间格式无效".to_string());
    }
    Ok(hours * 60 + minutes)
}

fn format_time(minutes: i64) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
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

fn ensure_subject(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    subject_id: &str,
) -> Result<(), String> {
    let exists: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM subjects WHERE workspace_id=?1 AND id=?2 AND deleted_at IS NULL)", params![workspace_id, subject_id], |row| row.get(0)).map_err(|error| error.to_string())?;
    if exists {
        Ok(())
    } else {
        Err("NOT_FOUND: 主体不存在".to_string())
    }
}

fn bump_revision(connection: &rusqlite::Connection) -> Result<(), String> {
    connection.execute("UPDATE app_metadata SET value=CAST(CAST(value AS INTEGER)+1 AS TEXT) WHERE key='global_revision'", []).map(|_| ()).map_err(|error| error.to_string())
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[tauri::command]
pub fn subject_work_schedule_get(
    database: tauri::State<'_, Database>,
    request: WorkScheduleGetRequest,
) -> Result<Vec<WorkScheduleDayDto>, String> {
    get_schedule(&database, &request.subject_id)
}

#[tauri::command]
pub fn subject_work_schedule_save(
    database: tauri::State<'_, Database>,
    request: WorkScheduleSaveRequest,
) -> Result<Vec<WorkScheduleDayDto>, String> {
    crate::supabase::ensure_repository_write_mode(&database)?;
    let state = crate::supabase::storage_mode(&database)?;
    let result = save_schedule_with_cloud_operations(&database, request, Some(&state))?;
    crate::cloud_sync::flush_if_online(&database)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn database() -> Database {
        Database::initialize_at(tempdir().unwrap().keep().join("schedules.sqlite3")).unwrap()
    }

    #[test]
    fn defaults_and_valid_save_persist() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("schedules.sqlite3");
        let database = Database::initialize_at(path.clone()).unwrap();
        let subject_id: String = database
            .open()
            .unwrap()
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let mut days = get_schedule(&database, &subject_id).unwrap();
        assert_eq!(days[0].segments.len(), 2);
        assert!(!days[6].enabled);
        days[0].segments = vec![WorkScheduleSegmentDto {
            start: "09:00".into(),
            end: "11:30".into(),
        }];
        let saved = save_schedule_with_cloud_operations(
            &database,
            WorkScheduleSaveRequest {
                subject_id: subject_id.clone(),
                days,
            },
            None,
        )
        .unwrap();
        assert_eq!(saved[0].segments[0].start, "09:00");
        drop(database);
        let reopened = Database::initialize_at(path).unwrap();
        assert_eq!(get_schedule(&reopened, &subject_id).unwrap()[0], saved[0]);
    }

    #[test]
    fn rejects_invalid_and_overlapping_segments() {
        let database = database();
        let subject_id: String = database
            .open()
            .unwrap()
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let mut days = get_schedule(&database, &subject_id).unwrap();
        days[0].segments = vec![WorkScheduleSegmentDto {
            start: "12:00".into(),
            end: "10:00".into(),
        }];
        assert!(save_schedule_with_cloud_operations(
            &database,
            WorkScheduleSaveRequest {
                subject_id: subject_id.clone(),
                days
            },
            None
        )
        .unwrap_err()
        .contains("结束时间"));
        let mut days = get_schedule(&database, &subject_id).unwrap();
        days[0].segments = vec![
            WorkScheduleSegmentDto {
                start: "10:00".into(),
                end: "12:00".into(),
            },
            WorkScheduleSegmentDto {
                start: "11:30".into(),
                end: "13:00".into(),
            },
        ];
        assert!(save_schedule_with_cloud_operations(
            &database,
            WorkScheduleSaveRequest { subject_id, days },
            None
        )
        .unwrap_err()
        .contains("重叠"));
    }

    #[test]
    fn cloud_mode_queues_only_real_schedule_changes() {
        let database = database();
        let connection = database.open().unwrap();
        let workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let device_id: String = connection
            .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let subject_id: String = connection
            .query_row("SELECT id FROM subjects LIMIT 1", [], |row| row.get(0))
            .unwrap();
        drop(connection);
        let state = crate::supabase::StorageModeSnapshot {
            mode: "cloud".to_string(),
            workspace_id,
            device_id,
            online: false,
            sync_state: "waiting".to_string(),
            pending_operations: 0,
            conflict_count: 0,
            last_change_seq: 0,
            last_synced_at: None,
            last_error: None,
            auth_blocked: false,
            auth_blocked_reason: None,
        };
        let mut days = get_schedule(&database, &subject_id).unwrap();
        assert_eq!(
            database
                .open()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM sync_outbox", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        save_schedule_with_cloud_operations(
            &database,
            WorkScheduleSaveRequest {
                subject_id: subject_id.clone(),
                days: days.clone(),
            },
            Some(&state),
        )
        .unwrap();
        assert_eq!(
            database
                .open()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM sync_outbox", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        days[0].segments[0].start = "09:30".to_string();
        save_schedule_with_cloud_operations(
            &database,
            WorkScheduleSaveRequest { subject_id, days },
            Some(&state),
        )
        .unwrap();
        assert_eq!(
            database
                .open()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM sync_outbox WHERE entity_type='subject_work_schedule'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
    }
}
