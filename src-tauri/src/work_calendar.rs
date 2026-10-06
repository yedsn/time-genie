use chrono::{DateTime, LocalResult, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;

use crate::database::Database;

pub const DEFAULT_TIMEZONE: &str = "Asia/Shanghai";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayBounds {
    pub work_date: String,
    pub start_at: i64,
    pub end_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarSlice {
    pub work_date: String,
    pub started_at: i64,
    pub ended_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceCalendarContextDto {
    pub timezone: String,
    pub current_work_date: String,
    pub current_day_start_at: i64,
    pub current_day_end_at: i64,
}

pub fn validate_timezone(value: &str) -> Result<Tz, String> {
    value
        .trim()
        .parse::<Tz>()
        .map_err(|_| format!("VALIDATION_ERROR: 无效的工作空间时区: {value}"))
}

pub fn workspace_timezone(connection: &Connection, workspace_id: &str) -> Result<Tz, String> {
    let value: String = connection
        .query_row(
            "SELECT timezone FROM workspaces WHERE id = ?1 AND deleted_at IS NULL",
            [workspace_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    validate_timezone(&value)
}

pub fn current_workspace_timezone(connection: &Connection) -> Result<Tz, String> {
    let value: Option<String> = connection
        .query_row(
            "SELECT timezone FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    validate_timezone(
        value
            .as_deref()
            .ok_or_else(|| "NOT_FOUND: 工作空间不存在".to_string())?,
    )
}

pub fn validate_all_workspace_timezones(connection: &Connection) -> Result<(), String> {
    let mut statement = connection
        .prepare("SELECT id, timezone FROM workspaces WHERE deleted_at IS NULL")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| error.to_string())?;
    for row in rows {
        let (workspace_id, timezone) = row.map_err(|error| error.to_string())?;
        validate_timezone(&timezone)
            .map_err(|error| format!("{error}（工作空间 {workspace_id}），请修正后重新启动"))?;
    }
    Ok(())
}

pub fn work_date_at(timezone: Tz, timestamp_millis: i64) -> Result<String, String> {
    Ok(timestamp_at(timezone, timestamp_millis)?
        .date_naive()
        .format("%Y-%m-%d")
        .to_string())
}

pub fn current_work_date(connection: &Connection, now_millis: i64) -> Result<String, String> {
    work_date_at(current_workspace_timezone(connection)?, now_millis)
}

pub fn calendar_context_at(
    connection: &Connection,
    now_millis: i64,
) -> Result<WorkspaceCalendarContextDto, String> {
    let timezone = current_workspace_timezone(connection)?;
    let current_work_date = work_date_at(timezone, now_millis)?;
    let bounds = day_bounds(timezone, &current_work_date)?;
    Ok(WorkspaceCalendarContextDto {
        timezone: timezone.name().to_string(),
        current_work_date,
        current_day_start_at: bounds.start_at,
        current_day_end_at: bounds.end_at,
    })
}

#[tauri::command]
pub fn work_calendar_get_context(
    database: tauri::State<'_, Database>,
) -> Result<WorkspaceCalendarContextDto, String> {
    calendar_context_at(&database.open()?, now_millis())
}

pub fn day_bounds(timezone: Tz, work_date: &str) -> Result<DayBounds, String> {
    let date = NaiveDate::parse_from_str(work_date, "%Y-%m-%d")
        .map_err(|_| format!("VALIDATION_ERROR: 无效日期: {work_date}"))?;
    let next = date
        .succ_opt()
        .ok_or_else(|| "VALIDATION_ERROR: 日期超出支持范围".to_string())?;
    Ok(DayBounds {
        work_date: work_date.to_string(),
        start_at: local_midnight(timezone, date)?.timestamp_millis(),
        end_at: local_midnight(timezone, next)?.timestamp_millis(),
    })
}

pub fn boundaries_between(
    timezone: Tz,
    started_at: i64,
    ended_at: i64,
) -> Result<Vec<i64>, String> {
    if ended_at <= started_at {
        return Ok(Vec::new());
    }
    let mut date = timestamp_at(timezone, started_at)?.date_naive();
    let mut result = Vec::new();
    loop {
        date = date
            .succ_opt()
            .ok_or_else(|| "VALIDATION_ERROR: 日期超出支持范围".to_string())?;
        let boundary = local_midnight(timezone, date)?.timestamp_millis();
        if boundary >= ended_at {
            break;
        }
        if boundary > started_at {
            result.push(boundary);
        }
    }
    Ok(result)
}

pub fn split_interval_by_day(
    timezone: Tz,
    started_at: i64,
    ended_at: i64,
) -> Result<Vec<CalendarSlice>, String> {
    if ended_at <= started_at {
        return Ok(Vec::new());
    }
    let mut points = Vec::with_capacity(4);
    points.push(started_at);
    points.extend(boundaries_between(timezone, started_at, ended_at)?);
    points.push(ended_at);
    points
        .windows(2)
        .map(|window| {
            Ok(CalendarSlice {
                work_date: work_date_at(timezone, window[0])?,
                started_at: window[0],
                ended_at: window[1],
            })
        })
        .collect()
}

fn timestamp_at(timezone: Tz, timestamp_millis: i64) -> Result<DateTime<Tz>, String> {
    let utc = Utc
        .timestamp_millis_opt(timestamp_millis)
        .single()
        .ok_or_else(|| "VALIDATION_ERROR: 时间戳超出支持范围".to_string())?;
    Ok(utc.with_timezone(&timezone))
}

fn local_midnight(timezone: Tz, date: NaiveDate) -> Result<DateTime<Tz>, String> {
    let midnight = date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| "VALIDATION_ERROR: 无法构造自然日边界".to_string())?;
    match timezone.from_local_datetime(&midnight) {
        LocalResult::Single(value) => Ok(value),
        LocalResult::Ambiguous(first, second) => Ok(first.min(second)),
        LocalResult::None => {
            for minutes in 1..=180 {
                let candidate = midnight + chrono::Duration::minutes(minutes);
                match timezone.from_local_datetime(&candidate) {
                    LocalResult::Single(value) => return Ok(value),
                    LocalResult::Ambiguous(first, second) => return Ok(first.min(second)),
                    LocalResult::None => {}
                }
            }
            Err("VALIDATION_ERROR: 无法解析工作空间自然日边界".to_string())
        }
    }
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;

    fn millis(timezone: Tz, value: &str) -> i64 {
        NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")
            .unwrap()
            .and_local_timezone(timezone)
            .single()
            .unwrap()
            .timestamp_millis()
    }

    #[test]
    fn shanghai_calendar_uses_workspace_timezone_instead_of_device_timezone() {
        let timezone: Tz = "Asia/Shanghai".parse().unwrap();
        let timestamp = Utc
            .with_ymd_and_hms(2026, 10, 5, 16, 30, 0)
            .unwrap()
            .timestamp_millis();
        assert_eq!(work_date_at(timezone, timestamp).unwrap(), "2026-10-06");
        let bounds = day_bounds(timezone, "2026-10-06").unwrap();
        assert_eq!(bounds.start_at, millis(timezone, "2026-10-06 00:00:00"));
        assert_eq!(bounds.end_at, millis(timezone, "2026-10-07 00:00:00"));
    }

    #[test]
    fn splits_interval_at_every_workspace_midnight() {
        let timezone: Tz = "Asia/Shanghai".parse().unwrap();
        let slices = split_interval_by_day(
            timezone,
            millis(timezone, "2026-10-05 23:59:30"),
            millis(timezone, "2026-10-07 00:00:30"),
        )
        .unwrap();
        assert_eq!(
            slices
                .iter()
                .map(|slice| slice.work_date.as_str())
                .collect::<Vec<_>>(),
            vec!["2026-10-05", "2026-10-06", "2026-10-07"]
        );
        assert_eq!(
            slices
                .iter()
                .map(|slice| slice.ended_at - slice.started_at)
                .collect::<Vec<_>>(),
            vec![30_000, 86_400_000, 30_000]
        );
    }

    #[test]
    fn daylight_saving_days_are_not_forced_to_twenty_four_hours() {
        let timezone: Tz = "America/New_York".parse().unwrap();
        let spring = day_bounds(timezone, "2026-03-08").unwrap();
        let autumn = day_bounds(timezone, "2026-11-01").unwrap();
        assert_eq!(spring.end_at - spring.start_at, 23 * 60 * 60 * 1_000);
        assert_eq!(autumn.end_at - autumn.start_at, 25 * 60 * 60 * 1_000);
    }

    #[test]
    fn invalid_timezone_is_rejected_explicitly() {
        let error = validate_timezone("Mars/Olympus").unwrap_err();
        assert!(error.contains("无效的工作空间时区"));
    }

    #[test]
    fn calendar_context_uses_the_same_workspace_date_and_day_bounds() {
        let directory = tempfile::tempdir().unwrap();
        let database = crate::database::Database::initialize_at(
            directory.path().join("calendar-context.sqlite3"),
        )
        .unwrap();
        let connection = database.open().unwrap();
        connection
            .execute(
                "UPDATE workspaces SET timezone = 'Asia/Shanghai' WHERE deleted_at IS NULL",
                [],
            )
            .unwrap();
        let timestamp = Utc
            .with_ymd_and_hms(2026, 10, 5, 16, 30, 0)
            .unwrap()
            .timestamp_millis();
        let context = calendar_context_at(&connection, timestamp).unwrap();
        assert_eq!(context.timezone, "Asia/Shanghai");
        assert_eq!(context.current_work_date, "2026-10-06");
        assert_eq!(
            context.current_day_start_at,
            millis("Asia/Shanghai".parse().unwrap(), "2026-10-06 00:00:00")
        );
        assert_eq!(
            context.current_day_end_at,
            millis("Asia/Shanghai".parse().unwrap(), "2026-10-07 00:00:00")
        );
    }
}
