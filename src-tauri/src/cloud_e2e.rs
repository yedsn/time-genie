use std::collections::HashMap;
use std::env;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;
use uuid::Uuid;

use crate::actual_time_adjustments::{
    self, ActualAdjustmentBatchItem, ActualAdjustmentBatchRequest, ActualAdjustmentDeleteRequest,
    ActualAdjustmentNewTaskInput, ActualAdjustmentSnapshotRequest,
};
use crate::cloud_sync::{
    self, CloudConflictResolveRequest, CloudSyncPullRequest, CloudTimerStartRequest,
    CloudTimerVersionRequest, TrackingLeaseRequest,
};
use crate::data_protection::{self, BackupExportRequest};
use crate::database::Database;
use crate::recurring::{self, RecurrenceSaveRequest};
use crate::reports::{
    self, ReportCreateRequest, ReportListRequest, ReportTemplateCreateRequest,
    ReportTemplateDeleteRequest, ReportTemplateListRequest, ReportTemplateSetDefaultRequest,
    ReportVersionRequest,
};
use crate::subjects;
use crate::supabase::{
    self, CloudConfigureRequest, CloudDeviceRegisterRequest, CloudOperationHistoryListRequest,
    CloudSignInRequest, CloudWorkspaceRestoreCommitRequest, CloudWorkspaceRestorePrepareRequest,
    CloudWorkspaceSnapshotCreateRequest, CloudWorkspaceSnapshotSettingsUpdateRequest,
    MigrationExecuteRequest,
};
use crate::tasks::{
    self, TaskCreateRequest, TaskDailyEstimateBatchItem, TaskDailyEstimateBatchSetRequest,
    TaskDailyEstimateSetRequest, TaskListRequest, TaskUpdateRequest,
};
use crate::time_tracking::{
    self, ManualEntryRequest, TimeEntryListRequest, TimerTransitionNewTaskRequest,
    TimerTransitionRequest,
};
use crate::today_plans::{self, TaskDayPlanSetRequest, TaskDayPlanSnapshotRequest};
use crate::unassigned::{
    self, ResolveSessionRequest, ResolveWorkRequest, UnassignedAllocationInput,
};

struct TestConfig {
    project_url: String,
    anon_key: String,
    email: String,
    password: String,
}

impl TestConfig {
    fn from_environment() -> Self {
        assert_eq!(
            env::var("TG_SUPABASE_E2E_ALLOW_RESET").as_deref(),
            Ok("1"),
            "该测试会删除专用测试账号的工作空间；请显式设置 TG_SUPABASE_E2E_ALLOW_RESET=1"
        );
        Self {
            project_url: required_env("TG_SUPABASE_URL")
                .trim_end_matches('/')
                .to_string(),
            anon_key: required_env("TG_SUPABASE_ANON_KEY"),
            email: required_env("TG_SUPABASE_EMAIL"),
            password: required_env("TG_SUPABASE_PASSWORD"),
        }
    }
}

fn required_env(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("缺少环境变量 {name}"))
}

fn configure_and_sign_in(database: &Database, config: &TestConfig) {
    supabase::configure(
        database,
        CloudConfigureRequest {
            project_url: config.project_url.clone(),
            anon_key: config.anon_key.clone(),
        },
    )
    .unwrap();
    supabase::sign_in_password(
        database,
        CloudSignInRequest {
            email: config.email.clone(),
            password: config.password.clone(),
        },
    )
    .unwrap();
}

fn reset_owned_workspaces(database: &Database) {
    cleanup_owned_workspaces(database).unwrap();
}

fn cleanup_owned_workspaces(database: &Database) -> Result<(), String> {
    let session = supabase::current_session(database)?;
    let api = supabase::client(database)?;
    let response = reqwest::blocking::Client::new()
        .delete(format!(
            "{}/rest/v1/workspaces?owner_user_id=eq.{}",
            api.project_url, session.user_id
        ))
        .header("apikey", api.anon_key())
        .header("Accept-Profile", supabase::CLOUD_SCHEMA)
        .header("Content-Profile", supabase::CLOUD_SCHEMA)
        .bearer_auth(&session.access_token)
        .send()
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "无法清理专用 Supabase 测试工作空间: {}",
            response.text().unwrap_or_default()
        ));
    }
    Ok(())
}

struct CleanupGuard {
    database: Database,
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        if supabase::current_session(&self.database).is_ok() {
            let _ = cleanup_owned_workspaces(&self.database);
            let _ = supabase::sign_out(&self.database);
        }
    }
}

fn register_device(database: &Database, name: &str) {
    supabase::register_device(
        database,
        CloudDeviceRegisterRequest {
            device_name: name.to_string(),
            platform: "windows-e2e".to_string(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
        },
    )
    .unwrap();
}

fn migrate(database: &Database, direction: &str) {
    supabase::migration_execute(
        database,
        MigrationExecuteRequest {
            direction: direction.to_string(),
            confirmed: true,
        },
    )
    .unwrap();
}

fn create_task(database: &Database, subject_id: &str, title: &str) -> tasks::TaskDto {
    tasks::create_task(
        database,
        TaskCreateRequest {
            subject_id: subject_id.to_string(),
            day_instance_date: None,
            parent_id: None,
            title: title.to_string(),
            planned_date: None,
            estimate_minutes: None,
            note: None,
            project_name: None,
            solution_name: None,
        },
    )
    .unwrap()
}

fn update_task_title(database: &Database, task: &tasks::TaskDto, title: &str) -> tasks::TaskDto {
    tasks::update_task(
        database,
        TaskUpdateRequest {
            id: task.id.clone(),
            expected_version: task.version,
            title: Some(title.to_string()),
            parent_id: None,
            planned_date: None,
            estimate_minutes: None,
            note: None,
            project_name: None,
            solution_name: None,
        },
    )
    .unwrap()
}

fn list_tasks(database: &Database) -> Vec<tasks::TaskDto> {
    tasks::list_tasks(
        database,
        TaskListRequest {
            subject_id: None,
            include_completed: Some(true),
            planned_date: None,
            today_date: None,
            today_only: Some(false),
            daily_estimate_date: None,
            query: None,
        },
    )
    .unwrap()
    .tasks
}

fn task_version(database: &Database, task_id: &str) -> i64 {
    database
        .open()
        .unwrap()
        .query_row(
            "SELECT version FROM tasks WHERE id = ?1",
            [task_id],
            |row| row.get(0),
        )
        .unwrap()
}

fn settle_local_restore_gates(database: &Database) {
    let connection = database.open().unwrap();
    connection
        .execute(
            "UPDATE time_entries SET state='ended',ended_at=COALESCE(ended_at,started_at),updated_at=MAX(updated_at,started_at) WHERE state IN ('running','paused')",
            [],
        )
        .unwrap();
}

fn daily_estimate(database: &Database, task_id: &str, work_date: &str) -> Option<i64> {
    use rusqlite::OptionalExtension;
    database
        .open()
        .unwrap()
        .query_row(
            "SELECT estimate_minutes FROM task_daily_estimates WHERE task_id = ?1 AND work_date = ?2",
            rusqlite::params![task_id, work_date],
            |row| row.get(0),
        )
        .optional()
        .unwrap()
}

fn day_plan_state(database: &Database, task_id: &str, work_date: &str) -> Option<String> {
    use rusqlite::OptionalExtension;
    database
        .open()
        .unwrap()
        .query_row(
            "SELECT state FROM task_day_plans WHERE task_id = ?1 AND work_date = ?2",
            rusqlite::params![task_id, work_date],
            |row| row.get(0),
        )
        .optional()
        .unwrap()
}

fn is_today_member(database: &Database, subject_id: &str, work_date: &str, task_id: &str) -> bool {
    today_plans::snapshot(
        database,
        TaskDayPlanSnapshotRequest {
            subject_id: subject_id.to_string(),
            work_date: work_date.to_string(),
        },
    )
    .unwrap()
    .tasks
    .iter()
    .any(|task| {
        task.today_member
            && (task.id == task_id || task.day_instance_source_task_id.as_deref() == Some(task_id))
    })
}

fn today_instance(
    database: &Database,
    subject_id: &str,
    work_date: &str,
    source_task_id: &str,
) -> tasks::TaskDto {
    today_plans::snapshot(
        database,
        TaskDayPlanSnapshotRequest {
            subject_id: subject_id.to_string(),
            work_date: work_date.to_string(),
        },
    )
    .unwrap()
    .tasks
    .into_iter()
    .find(|task| {
        task.today_member && task.day_instance_source_task_id.as_deref() == Some(source_task_id)
    })
    .unwrap()
}

fn task_title(database: &Database, task_id: &str) -> String {
    database
        .open()
        .unwrap()
        .query_row("SELECT title FROM tasks WHERE id=?1", [task_id], |row| {
            row.get(0)
        })
        .unwrap()
}

fn running_entry_count(database: &Database, work_date: &str) -> usize {
    time_tracking::list_time_entries(
        database,
        TimeEntryListRequest {
            work_date: work_date.to_string(),
            include_breaks: true,
        },
    )
    .unwrap()
    .entries
    .iter()
    .filter(|entry| entry.state == "running")
    .count()
}

fn task_estimate(database: &Database, task_id: &str) -> Option<i64> {
    database
        .open()
        .unwrap()
        .query_row(
            "SELECT estimate_minutes FROM tasks WHERE id=?1",
            [task_id],
            |row| row.get(0),
        )
        .unwrap()
}

fn push_all(database: &Database) -> cloud_sync::CloudSyncPushResult {
    let mut total_pushed = 0;
    let mut stalled_rounds = 0;
    for _ in 0..20 {
        let result = cloud_sync::push_outbox(database).unwrap();
        total_pushed += result.pushed;
        if result.conflicts > 0 || result.pending == 0 {
            return cloud_sync::CloudSyncPushResult {
                pushed: total_pushed,
                pending: result.pending,
                conflicts: result.conflicts,
            };
        }
        stalled_rounds = if result.pushed > 0 {
            0
        } else {
            stalled_rounds + 1
        };
        assert!(
            stalled_rounds < 2,
            "同步队列仍有待处理操作，但本轮没有取得进展: {}",
            database
                .open()
                .unwrap()
                .prepare(
                    "SELECT operation_type||':'||entity_type||':'||state||':'||
                            COALESCE(depends_on_operation_id,'-')||':'||COALESCE(error_json,'-')
                     FROM sync_outbox ORDER BY created_at,rowid",
                )
                .unwrap()
                .query_map([], |row| row.get::<_, String>(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
                .join(" | ")
        );
    }
    panic!("同步队列在 20 轮后仍未排空")
}

fn pull_all(database: &Database) -> cloud_sync::CloudSyncPullResult {
    let mut combined = cloud_sync::CloudSyncPullResult {
        changes: Vec::new(),
        last_change_seq: 0,
        has_more: false,
        domains: Vec::new(),
    };
    for _ in 0..20 {
        let result = cloud_sync::pull(
            database,
            CloudSyncPullRequest {
                after_change_seq: None,
                limit: None,
            },
        )
        .unwrap();
        combined.changes.extend(result.changes);
        combined.last_change_seq = result.last_change_seq;
        combined.has_more = result.has_more;
        for domain in result.domains {
            if !combined.domains.contains(&domain) {
                combined.domains.push(domain);
            }
        }
        if !combined.has_more {
            return combined;
        }
    }
    panic!("Supabase 增量拉取在 20 轮后仍未完成")
}

fn reset_shared_unassigned_anchor(database: &Database, resumed_at_millis: i64) -> Value {
    let local = unassigned::get_state(database)
        .unwrap()
        .expect("重置共享未归属锚点前不存在活动会话");
    let storage = supabase::storage_mode(database).unwrap();
    let session = supabase::current_session(database).unwrap();
    let value = supabase::client(database)
        .unwrap()
        .rpc_for_database(
            database,
            "unassigned_anchor_reset",
            json!({
                "p_workspace_id": storage.workspace_id,
                "p_device_id": storage.device_id,
                "p_operation_id": Uuid::now_v7().to_string(),
                "p_session_id": local.session_id,
                "p_expected_version": local.version,
                "p_resumed_at_millis": resumed_at_millis,
            }),
            &session,
        )
        .unwrap();
    assert_eq!(value["accepted"], true, "共享未归属锚点重置未被接受");
    cloud_sync::apply_remote_unassigned_session_for_test(database, &value["session"]).unwrap();
    value
}

fn remote_unassigned_state(database: &Database) -> Value {
    let storage = supabase::storage_mode(database).unwrap();
    let session = supabase::current_session(database).unwrap();
    supabase::client(database)
        .unwrap()
        .rpc_for_database(
            database,
            "unassigned_get_state",
            json!({ "p_workspace_id": storage.workspace_id }),
            &session,
        )
        .unwrap()
}

fn outbox_debug(database: &Database) -> Vec<String> {
    database
        .open()
        .unwrap()
        .prepare(
            "SELECT operation_type||':'||entity_type||':'||COALESCE(entity_id,'-')||':'||state
             FROM sync_outbox ORDER BY created_at,rowid",
        )
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn generated_entries_for_session(database: &Database, session_id: &str) -> Vec<(String, String)> {
    database
        .open()
        .unwrap()
        .prepare(
            "SELECT id,kind FROM time_entries
             WHERE origin_unassigned_session_id=?1 ORDER BY id",
        )
        .unwrap()
        .query_map([session_id], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn unassigned_terminal_result(
    database: &Database,
    session_id: &str,
) -> (String, String, Option<String>) {
    database
        .open()
        .unwrap()
        .query_row(
            "SELECT state,COALESCE(resolution_type,''),generated_entry_id
             FROM unassigned_sessions WHERE id=?1",
            [session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap()
}

fn report_unassigned_e2e(
    scenario: &str,
    state: Option<&unassigned::UnassignedStateDto>,
    pending: i64,
    final_result: &str,
) {
    let anchor = state
        .map(|item| format!("{}@{}", item.session_id, item.first_started_at))
        .unwrap_or_else(|| "none".to_string());
    eprintln!(
        "[unassigned-e2e] scenario={scenario} anchor={anchor} pending={pending} final={final_result} result=PASS"
    );
}

#[derive(Clone)]
struct PlanningSnapshotState {
    subjects: HashMap<String, Value>,
    tasks: HashMap<String, Value>,
    occurrences: HashMap<String, Value>,
    day_plans: HashMap<String, Value>,
    daily_estimates: HashMap<String, Value>,
    recurrence_rules: HashMap<String, Vec<Value>>,
    time_entries: HashMap<String, Value>,
    actual_time_adjustments: HashMap<String, Value>,
    report_templates: HashMap<String, Value>,
    reports: HashMap<String, Value>,
    base_change_seq: i64,
    workspace_epoch: i64,
}

struct PlanningCloudState {
    latest_change_seq: i64,
    workspace_epoch: i64,
    subjects: HashMap<String, Value>,
    tasks: HashMap<String, Value>,
    occurrences: HashMap<String, Value>,
    day_plans: HashMap<String, Value>,
    daily_estimates: HashMap<String, Value>,
    recurrence_rules: HashMap<String, Vec<Value>>,
    time_entries: HashMap<String, Value>,
    actual_time_adjustments: HashMap<String, Value>,
    report_templates: HashMap<String, Value>,
    reports: HashMap<String, Value>,
    changes: Vec<Value>,
    incremental_pull_count: usize,
    snapshot_get_count: usize,
    force_reset_required_once: bool,
    snapshots: HashMap<String, PlanningSnapshotState>,
    automatic_snapshot_enabled: bool,
    automatic_snapshot_time: String,
    automatic_snapshot_retention_days: i64,
    requested_rpc_paths: Vec<String>,
    restore_target_snapshot_id: Option<String>,
    restore_safety_snapshot_id: Option<String>,
    restore_lock_token: Option<String>,
    restore_operation_count: usize,
    operation_history_item_count: usize,
    tracking_lease_holder_device_id: Option<String>,
    tracking_lease_token: Option<String>,
    tracking_lease_version: i64,
    tracking_lease_expired: bool,
}

impl Default for PlanningCloudState {
    fn default() -> Self {
        Self {
            latest_change_seq: 0,
            workspace_epoch: 1,
            subjects: HashMap::new(),
            tasks: HashMap::new(),
            occurrences: HashMap::new(),
            day_plans: HashMap::new(),
            daily_estimates: HashMap::new(),
            recurrence_rules: HashMap::new(),
            time_entries: HashMap::new(),
            actual_time_adjustments: HashMap::new(),
            report_templates: HashMap::new(),
            reports: HashMap::new(),
            changes: Vec::new(),
            incremental_pull_count: 0,
            snapshot_get_count: 0,
            force_reset_required_once: false,
            snapshots: HashMap::new(),
            automatic_snapshot_enabled: true,
            automatic_snapshot_time: "03:00".to_string(),
            automatic_snapshot_retention_days: 30,
            requested_rpc_paths: Vec::new(),
            restore_target_snapshot_id: None,
            restore_safety_snapshot_id: None,
            restore_lock_token: None,
            restore_operation_count: 0,
            operation_history_item_count: 0,
            tracking_lease_holder_device_id: None,
            tracking_lease_token: None,
            tracking_lease_version: 0,
            tracking_lease_expired: false,
        }
    }
}

struct PlanningCloudServer {
    base_url: String,
    state: Arc<Mutex<PlanningCloudState>>,
    shutdown: Arc<std::sync::atomic::AtomicBool>,
    apply_patch_delay_millis: Arc<std::sync::atomic::AtomicU64>,
    apply_patch_started: Arc<std::sync::atomic::AtomicBool>,
    fail_apply_once: Arc<std::sync::atomic::AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl PlanningCloudServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let state = Arc::new(Mutex::new(PlanningCloudState::default()));
        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let apply_patch_delay_millis = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let apply_patch_started = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let fail_apply_once = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let thread_state = Arc::clone(&state);
        let thread_shutdown = Arc::clone(&shutdown);
        let thread_apply_patch_delay_millis = Arc::clone(&apply_patch_delay_millis);
        let thread_apply_patch_started = Arc::clone(&apply_patch_started);
        let thread_fail_apply_once = Arc::clone(&fail_apply_once);
        let handle = thread::spawn(move || {
            while !thread_shutdown.load(std::sync::atomic::Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        handle_planning_cloud_request(
                            stream,
                            &thread_state,
                            &thread_apply_patch_delay_millis,
                            &thread_apply_patch_started,
                            &thread_fail_apply_once,
                        );
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("隔离 Supabase 测试服务异常: {error}"),
                }
            }
        });
        Self {
            base_url: format!("http://{address}"),
            state,
            shutdown,
            apply_patch_delay_millis,
            apply_patch_started,
            fail_apply_once,
            handle: Some(handle),
        }
    }

    fn seed(&self, database: &Database, subject: &subjects::SubjectDto, task: &tasks::TaskDto) {
        let now = chrono::Utc::now().timestamp_millis();
        let mut state = self.state.lock().unwrap();
        state.subjects.insert(
            subject.id.clone(),
            json!({
                "id": subject.id,
                "name": subject.name,
                "sort_order": subject.sort_order,
                "created_at": now,
                "updated_at": now,
                "version": subject.version,
                "deleted_at": Value::Null
            }),
        );
        state.tasks.insert(task.id.clone(), task_payload(task));
        let connection = database.open().unwrap();
        let mut statement = connection
            .prepare(
                "SELECT id,report_type,name,description,content,is_default,is_builtin,sort_order,created_at,updated_at,version,deleted_at FROM report_templates",
            )
            .unwrap();
        let templates = statement
            .query_map([], |row| {
                Ok(json!({
                    "id": row.get::<_, String>(0)?,
                    "report_type": row.get::<_, String>(1)?,
                    "subject_id": Value::Null,
                    "name": row.get::<_, String>(2)?,
                    "description": row.get::<_, Option<String>>(3)?,
                    "content": row.get::<_, String>(4)?,
                    "is_default": row.get::<_, i64>(5)? == 1,
                    "is_builtin": row.get::<_, i64>(6)? == 1,
                    "sort_order": row.get::<_, i64>(7)?,
                    "created_at": row.get::<_, i64>(8)?,
                    "updated_at": row.get::<_, i64>(9)?,
                    "version": row.get::<_, i64>(10)?,
                    "deleted_at": row.get::<_, Option<i64>>(11)?,
                }))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        for template in templates {
            state
                .report_templates
                .insert(template["id"].as_str().unwrap().to_string(), template);
        }
    }

    fn request_counts(&self) -> (usize, usize) {
        let state = self.state.lock().unwrap();
        (state.incremental_pull_count, state.snapshot_get_count)
    }

    fn require_snapshot_reset_once(&self) {
        self.state.lock().unwrap().force_reset_required_once = true;
    }

    fn capture_snapshot(&self) -> String {
        let mut state = self.state.lock().unwrap();
        let snapshot_id = Uuid::now_v7().to_string();
        let snapshot = PlanningSnapshotState {
            subjects: state.subjects.clone(),
            tasks: state.tasks.clone(),
            occurrences: state.occurrences.clone(),
            day_plans: state.day_plans.clone(),
            daily_estimates: state.daily_estimates.clone(),
            recurrence_rules: state.recurrence_rules.clone(),
            time_entries: state.time_entries.clone(),
            actual_time_adjustments: state.actual_time_adjustments.clone(),
            report_templates: state.report_templates.clone(),
            reports: state.reports.clone(),
            base_change_seq: state.latest_change_seq,
            workspace_epoch: state.workspace_epoch,
        };
        state.snapshots.insert(snapshot_id.clone(), snapshot);
        snapshot_id
    }

    fn restore_audit(&self) -> (i64, i64, Option<String>, usize) {
        let state = self.state.lock().unwrap();
        (
            state.workspace_epoch,
            state.latest_change_seq,
            state.restore_safety_snapshot_id.clone(),
            state.restore_operation_count,
        )
    }

    fn expire_tracking_lease(&self) {
        let mut state = self.state.lock().unwrap();
        if state.tracking_lease_holder_device_id.is_some() {
            state.tracking_lease_expired = true;
        }
    }

    fn requested_rpc_paths(&self) -> Vec<String> {
        self.state.lock().unwrap().requested_rpc_paths.clone()
    }

    fn delay_next_apply_patch(&self, delay: Duration) {
        self.apply_patch_started
            .store(false, std::sync::atomic::Ordering::Release);
        self.apply_patch_delay_millis.store(
            delay.as_millis() as u64,
            std::sync::atomic::Ordering::Release,
        );
    }

    fn fail_next_apply_once(&self) {
        self.fail_apply_once
            .store(true, std::sync::atomic::Ordering::Release);
    }

    fn wait_for_delayed_apply_patch(&self) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !self
            .apply_patch_started
            .load(std::sync::atomic::Ordering::Acquire)
        {
            assert!(
                std::time::Instant::now() < deadline,
                "慢网测试没有观察到上传请求"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for PlanningCloudServer {
    fn drop(&mut self) {
        self.shutdown
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = TcpStream::connect(self.base_url.trim_start_matches("http://"));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn task_payload(task: &tasks::TaskDto) -> Value {
    let now = chrono::Utc::now().timestamp_millis();
    json!({
        "id": task.id,
        "subject_id": task.subject_id,
        "parent_id": task.parent_id,
        "title": task.title,
        "status": task.status,
        "planned_date": task.planned_date,
        "planned_time": Value::Null,
        "estimate_minutes": task.estimate_minutes,
        "note": task.note,
        "project_name": task.project_name,
        "solution_name": task.solution_name,
        "source_type": "manual",
        "source_ref": Value::Null,
        "sort_order": task.sort_order,
        "completed_at": task.completed_at,
        "created_at": now,
        "updated_at": now,
        "version": task.version,
        "deleted_at": Value::Null
    })
}

fn read_http_request(stream: &mut TcpStream) -> (String, Value) {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let read = stream.read(&mut buffer).unwrap();
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
        let Some(header_end) = request.windows(4).position(|value| value == b"\r\n\r\n") else {
            continue;
        };
        let headers = String::from_utf8_lossy(&request[..header_end]);
        let length = headers
            .lines()
            .find_map(|line| line.split_once(':'))
            .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            .and_then(|(_, value)| value.trim().parse::<usize>().ok())
            .unwrap_or_default();
        if request.len() >= header_end + 4 + length {
            break;
        }
    }
    let header_end = request
        .windows(4)
        .position(|value| value == b"\r\n\r\n")
        .unwrap_or(request.len());
    let request_head = String::from_utf8_lossy(&request[..header_end]).to_string();
    let body = request.get(header_end + 4..).unwrap_or_default();
    let json = serde_json::from_slice(body).unwrap_or(Value::Null);
    (request_head, json)
}

fn write_http_json(stream: &mut TcpStream, status: u16, body: Value) {
    let body = body.to_string();
    let reason = if status == 200 { "OK" } else { "Conflict" };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).unwrap();
}

fn request_workspace_epoch(request_line: &str) -> i64 {
    request_line
        .split("\r\n")
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("x-timegenie-workspace-epoch"))
        .and_then(|(_, value)| value.trim().parse().ok())
        .unwrap_or(1)
}

fn planning_snapshot_settings(state: &PlanningCloudState) -> Value {
    json!({
        "automaticSnapshotEnabled": state.automatic_snapshot_enabled,
        "automaticSnapshotTime": state.automatic_snapshot_time,
        "automaticSnapshotTimezone": "Asia/Shanghai",
        "snapshotRetentionDays": state.automatic_snapshot_retention_days,
        "nextAutomaticSnapshotAt": "2026-10-19T03:00:00+08:00",
        "lastAutomaticSnapshotAt": Value::Null,
        "schedulerActive": true
    })
}

fn planning_snapshot_record(state: &PlanningCloudState) -> PlanningSnapshotState {
    PlanningSnapshotState {
        subjects: state.subjects.clone(),
        tasks: state.tasks.clone(),
        occurrences: state.occurrences.clone(),
        day_plans: state.day_plans.clone(),
        daily_estimates: state.daily_estimates.clone(),
        recurrence_rules: state.recurrence_rules.clone(),
        time_entries: state.time_entries.clone(),
        actual_time_adjustments: state.actual_time_adjustments.clone(),
        report_templates: state.report_templates.clone(),
        reports: state.reports.clone(),
        base_change_seq: state.latest_change_seq,
        workspace_epoch: state.workspace_epoch,
    }
}

fn restore_planning_snapshot(state: &mut PlanningCloudState, snapshot: &PlanningSnapshotState) {
    state.subjects = snapshot.subjects.clone();
    state.tasks = snapshot.tasks.clone();
    state.occurrences = snapshot.occurrences.clone();
    state.day_plans = snapshot.day_plans.clone();
    state.daily_estimates = snapshot.daily_estimates.clone();
    state.recurrence_rules = snapshot.recurrence_rules.clone();
    state.time_entries = snapshot.time_entries.clone();
    state.actual_time_adjustments = snapshot.actual_time_adjustments.clone();
    state.report_templates = snapshot.report_templates.clone();
    state.reports = snapshot.reports.clone();
}

fn record_change(
    state: &mut PlanningCloudState,
    workspace_id: &str,
    entity_type: &str,
    entity_id: &str,
    operation: &str,
    version: i64,
) {
    state.latest_change_seq += 1;
    let change_seq = state.latest_change_seq;
    state.changes.push(json!({
        "change_seq": change_seq,
        "workspace_id": workspace_id,
        "entity_type": entity_type,
        "entity_id": entity_id,
        "operation": operation,
        "entity_version": version,
        "changed_at": "2026-10-05T00:00:00Z"
    }));
}

fn planning_snapshot(state: &PlanningCloudState) -> Value {
    json!({
        "latest_change_seq": state.latest_change_seq,
        "workspace_epoch": state.workspace_epoch,
        "subjects": state.subjects.values().cloned().collect::<Vec<_>>(),
        "tasks": state.tasks.values().cloned().collect::<Vec<_>>(),
        "task_status_events": [],
        "task_day_plans": state.day_plans.values().cloned().collect::<Vec<_>>(),
        "task_daily_estimates": state.daily_estimates.values().cloned().collect::<Vec<_>>(),
        "task_recurrence_rules": state.recurrence_rules.values().flatten().cloned().collect::<Vec<_>>(),
        "task_occurrences": state.occurrences.values().cloned().collect::<Vec<_>>(),
        "work_days": [], "time_entries": state.time_entries.values().cloned().collect::<Vec<_>>(), "time_segments": [], "time_allocations": [],
        "actual_time_adjustments": state.actual_time_adjustments.values().cloned().collect::<Vec<_>>(),
        "unassigned_sessions": [], "unassigned_segments": [], "report_templates": state.report_templates.values().cloned().collect::<Vec<_>>(),
        "reports": state.reports.values().cloned().collect::<Vec<_>>(), "report_tasks": [], "app_settings": [], "integration_configs": [],
        "external_bindings": []
    })
}

fn planning_incremental_entity(
    state: &PlanningCloudState,
    source_entity_type: &str,
    entity_id: &str,
    change_seq: i64,
) -> Value {
    let (entity_type, data) = match source_entity_type {
        "subjects" => ("subjects", state.subjects.get(entity_id).cloned()),
        "tasks" => ("tasks", state.tasks.get(entity_id).cloned()),
        "task_day_plans" => ("task_day_plans", state.day_plans.get(entity_id).cloned()),
        "task_daily_estimates" => (
            "task_daily_estimates",
            state.daily_estimates.get(entity_id).cloned(),
        ),
        "task_recurrence_rules" => (
            "task_recurrence_rules",
            Some(json!({
                "task_id": entity_id,
                "rules": state
                    .recurrence_rules
                    .get(entity_id)
                    .cloned()
                    .unwrap_or_default()
            })),
        ),
        "task_occurrences" => (
            "task_occurrences",
            state.occurrences.get(entity_id).cloned(),
        ),
        "time_entries" | "time_segments" | "time_allocations" => {
            ("time_entries", state.time_entries.get(entity_id).cloned())
        }
        "actual_time_adjustments" => (
            "actual_time_adjustments",
            state.actual_time_adjustments.get(entity_id).cloned(),
        ),
        "reports" | "report_tasks" => ("reports", state.reports.get(entity_id).cloned()),
        "report_templates" => (
            "report_templates",
            state.report_templates.get(entity_id).cloned(),
        ),
        other => (other, None),
    };
    json!({
        "entity_type": entity_type,
        "source_entity_type": source_entity_type,
        "entity_id": entity_id,
        "change_seq": change_seq,
        "deleted": data.is_none(),
        "data": data
    })
}

fn planning_incremental_pull(state: &PlanningCloudState, body: &Value) -> Value {
    let after = body["p_after_change_seq"]
        .as_i64()
        .unwrap_or_default()
        .max(0);
    let limit = body["p_limit"].as_i64().unwrap_or(200).clamp(1, 1_000) as usize;
    let changes = state
        .changes
        .iter()
        .filter(|change| change["change_seq"].as_i64().unwrap_or_default() > after)
        .take(limit)
        .cloned()
        .collect::<Vec<_>>();
    let covered_change_seq = changes
        .last()
        .and_then(|change| change["change_seq"].as_i64())
        .unwrap_or(after);
    let mut latest_entities = HashMap::<(String, String), Value>::new();
    for change in &changes {
        let source_entity_type = change["entity_type"].as_str().unwrap().to_string();
        let entity_id = change["entity_id"].as_str().unwrap().to_string();
        latest_entities.insert((source_entity_type, entity_id), change.clone());
    }
    let mut latest_entities = latest_entities.into_values().collect::<Vec<_>>();
    latest_entities.sort_by_key(|change| change["change_seq"].as_i64().unwrap_or_default());
    let entities = latest_entities
        .iter()
        .map(|change| {
            planning_incremental_entity(
                state,
                change["entity_type"].as_str().unwrap(),
                change["entity_id"].as_str().unwrap(),
                change["change_seq"].as_i64().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    json!({
        "changes": changes,
        "entities": entities,
        "covered_change_seq": covered_change_seq,
        "latest_change_seq": state.latest_change_seq,
        "has_more": covered_change_seq < state.latest_change_seq,
        "reset_required": false,
        "workspace_epoch": state.workspace_epoch
    })
}

fn planning_actual_adjustment_fingerprint(
    state: &PlanningCloudState,
    task_id: &str,
    work_date: &str,
) -> String {
    let mut rows = state
        .actual_time_adjustments
        .values()
        .filter(|adjustment| {
            adjustment["task_id"].as_str() == Some(task_id)
                && adjustment["work_date"].as_str() == Some(work_date)
        })
        .map(|adjustment| {
            format!(
                "{}:{}:{}",
                adjustment["id"].as_str().unwrap_or_default(),
                adjustment["version"].as_i64().unwrap_or_default(),
                adjustment["deleted_at"].as_i64().unwrap_or_default()
            )
        })
        .collect::<Vec<_>>();
    rows.sort();
    format!("{:x}", Sha256::digest(rows.join("|").as_bytes()))
}

fn planning_apply_actual_adjustment_batch(
    state: &mut PlanningCloudState,
    workspace_id: &str,
    device_id: &str,
    payload: &Value,
) -> Result<Value, String> {
    let subject_id = payload["subjectId"]
        .as_str()
        .ok_or_else(|| "VALIDATION_ERROR: missing subject".to_string())?;
    let work_date = payload["workDate"]
        .as_str()
        .ok_or_else(|| "VALIDATION_ERROR: missing work date".to_string())?;
    let adjustments = payload["adjustments"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    for item in payload["items"].as_array().into_iter().flatten() {
        let task_id = item["taskId"]
            .as_str()
            .ok_or_else(|| "VALIDATION_ERROR: missing task".to_string())?;
        let task = state
            .tasks
            .get(task_id)
            .ok_or_else(|| "VERSION_CONFLICT: actual adjustment task missing".to_string())?;
        if task["subject_id"].as_str() != Some(subject_id)
            || !task["deleted_at"].is_null()
            || task["version"].as_i64() != item["expectedTaskVersion"].as_i64()
        {
            return Err("VERSION_CONFLICT: actual adjustment task version changed".to_string());
        }
        if planning_actual_adjustment_fingerprint(state, task_id, work_date)
            != item["adjustmentFingerprint"].as_str().unwrap_or_default()
        {
            return Err(
                "ADJUSTMENT_BASELINE_CONFLICT: actual adjustment baseline changed".to_string(),
            );
        }
    }

    let mut created_task_count = 0;
    for wrapper in payload["createdTasks"].as_array().into_iter().flatten() {
        let task = wrapper["task"].clone();
        let task_id = task["id"]
            .as_str()
            .ok_or_else(|| "VALIDATION_ERROR: created task image missing".to_string())?
            .to_string();
        let version = task["version"].as_i64().unwrap_or(1);
        state.tasks.insert(task_id.clone(), task);
        record_change(state, workspace_id, "tasks", &task_id, "insert", version);
        created_task_count += 1;
    }

    let mut applied = Vec::new();
    let mut discarded = Vec::new();
    for mut adjustment in adjustments {
        let adjustment_id = adjustment["id"]
            .as_str()
            .ok_or_else(|| "VALIDATION_ERROR: adjustment image missing".to_string())?
            .to_string();
        let task_id = adjustment["task_id"]
            .as_str()
            .ok_or_else(|| "VALIDATION_ERROR: adjustment task missing".to_string())?
            .to_string();
        if let Some(item) = payload["items"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|item| item["taskId"].as_str() == Some(task_id.as_str()))
        {
            let current_actual = state
                .actual_time_adjustments
                .values()
                .filter(|existing| {
                    existing["task_id"].as_str() == Some(task_id.as_str())
                        && existing["work_date"].as_str() == Some(work_date)
                        && existing["deleted_at"].is_null()
                })
                .map(|existing| existing["delta_minutes"].as_i64().unwrap_or_default())
                .sum::<i64>();
            let target = item["targetMinutes"].as_i64().unwrap_or_default();
            let effective_delta = target - current_actual;
            if effective_delta == 0 {
                discarded.push(Value::String(adjustment_id));
                continue;
            }
            adjustment["delta_minutes"] = Value::Number(effective_delta.into());
            adjustment["baseline_minutes"] = Value::Number(current_actual.into());
            adjustment["target_minutes"] = Value::Number(target.into());
        }
        adjustment["workspace_id"] = Value::String(workspace_id.to_string());
        adjustment["updated_by_device_id"] = Value::String(device_id.to_string());
        let version = adjustment["version"].as_i64().unwrap_or(1);
        state
            .actual_time_adjustments
            .insert(adjustment_id.clone(), adjustment.clone());
        record_change(
            state,
            workspace_id,
            "actual_time_adjustments",
            &adjustment_id,
            "insert",
            version,
        );
        applied.push(adjustment);
    }

    Ok(json!({
        "batchId": payload["batchId"],
        "createdTaskCount": created_task_count,
        "createdAdjustmentCount": applied.len(),
        "adjustments": applied,
        "discardedAdjustmentIds": discarded
    }))
}

fn handle_planning_cloud_request(
    mut stream: TcpStream,
    state: &Arc<Mutex<PlanningCloudState>>,
    apply_patch_delay_millis: &std::sync::atomic::AtomicU64,
    apply_patch_started: &std::sync::atomic::AtomicBool,
    fail_apply_once: &std::sync::atomic::AtomicBool,
) {
    let (request_line, body) = read_http_request(&mut stream);
    let mut state = state.lock().unwrap();
    let request_epoch = request_workspace_epoch(&request_line);
    state.requested_rpc_paths.push(request_line.clone());
    if request_line.contains("/rest/v1/rpc/device_authorization_get") {
        return write_http_json(&mut stream, 200, json!({ "authorized": true }));
    }
    if request_line.contains("/rest/v1/rpc/workspace_snapshot_settings_get") {
        let settings = planning_snapshot_settings(&state);
        return write_http_json(&mut stream, 200, settings);
    }
    if request_line.contains("/rest/v1/rpc/workspace_snapshot_settings_update") {
        state.automatic_snapshot_enabled = body["p_automatic_snapshot_enabled"]
            .as_bool()
            .unwrap_or(true);
        state.automatic_snapshot_time = body["p_automatic_snapshot_time"]
            .as_str()
            .unwrap_or("03:00")
            .to_string();
        if let Some(days) = body["p_snapshot_retention_days"].as_i64() {
            state.automatic_snapshot_retention_days = days;
        }
        let settings = planning_snapshot_settings(&state);
        return write_http_json(&mut stream, 200, settings);
    }
    if request_line.contains("/rest/v1/rpc/workspace_snapshot_create") {
        let snapshot_id = Uuid::now_v7().to_string();
        let snapshot = planning_snapshot_record(&state);
        let name = body["p_name"].as_str().unwrap_or("测试镜像");
        let content_hash = format!("hash-{snapshot_id}");
        let workspace_epoch = snapshot.workspace_epoch;
        let base_change_seq = snapshot.base_change_seq;
        state.snapshots.insert(snapshot_id.clone(), snapshot);
        return write_http_json(
            &mut stream,
            200,
            json!({
                "id": snapshot_id, "snapshotType": body["p_snapshot_type"].as_str().unwrap_or("manual"),
                "name": name, "note": body["p_note"], "status": "ready", "formatVersion": 1,
                "baseChangeSeq": base_change_seq, "workspaceEpoch": workspace_epoch,
                "entityCounts": {"tasks": state.tasks.len()}, "groupHashes": {}, "contentHash": content_hash,
                "sizeBytes": 1, "chunkCount": 1, "deviceName": "隔离设备", "protected": false,
                "protectedAt": null, "expiresAt": null, "createdAt": "2026-10-18T00:00:00Z",
                "completedAt": "2026-10-18T00:00:00Z", "failureReason": null
            }),
        );
    }
    if request_line.contains("/rest/v1/rpc/workspace_restore_prepare") {
        if request_epoch != state.workspace_epoch {
            return write_http_json(
                &mut stream,
                409,
                json!({"message": format!("WORKSPACE_EPOCH_STALE: server={}", state.workspace_epoch)}),
            );
        }
        let target_snapshot_id = body["p_target_snapshot_id"].as_str().unwrap().to_string();
        if !state.snapshots.contains_key(&target_snapshot_id) {
            return write_http_json(&mut stream, 409, json!({"message": "SNAPSHOT_NOT_FOUND"}));
        }
        let safety_snapshot_id = body["p_create_safety_snapshot"]
            .as_bool()
            .unwrap_or(false)
            .then(|| Uuid::now_v7().to_string());
        if let Some(safety_snapshot_id) = safety_snapshot_id.as_ref() {
            let safety_snapshot = planning_snapshot_record(&state);
            state
                .snapshots
                .insert(safety_snapshot_id.clone(), safety_snapshot);
        }
        let lock_token = Uuid::now_v7().to_string();
        state.restore_target_snapshot_id = Some(target_snapshot_id.clone());
        state.restore_safety_snapshot_id = safety_snapshot_id.clone();
        state.restore_lock_token = Some(lock_token.clone());
        return write_http_json(
            &mut stream,
            200,
            json!({
                "lockToken": lock_token, "targetSnapshotId": target_snapshot_id.clone(),
                "targetContentHash": format!("hash-{target_snapshot_id}"), "preparedEpoch": state.workspace_epoch,
                "safetySnapshotId": safety_snapshot_id, "expiresAt": "2099-01-01T00:10:00Z",
                "preview": {"snapshotId": target_snapshot_id, "restorable": true, "errorCode": null,
                    "entityCounts": {"tasks": state.tasks.len()}, "groupCounts": {"planning": state.tasks.len()},
                    "groupHashes": {"planning": "planning-hash"}, "contentHash": format!("hash-{}", body["p_target_snapshot_id"].as_str().unwrap()),
                    "sizeBytes": 128, "chunkCount": 1, "baseChangeSeq": state.latest_change_seq, "workspaceEpoch": state.workspace_epoch}
            }),
        );
    }
    if request_line.contains("/rest/v1/rpc/workspace_restore_commit") {
        let lock_token = body["p_lock_token"].as_str().unwrap_or_default();
        if state.restore_lock_token.as_deref() != Some(lock_token) {
            return write_http_json(&mut stream, 409, json!({"message": "RESTORE_LOCK_INVALID"}));
        }
        let target_snapshot_id = body["p_target_snapshot_id"].as_str().unwrap().to_string();
        let target = state.snapshots.get(&target_snapshot_id).cloned().unwrap();
        let safety_snapshot_id = state.restore_safety_snapshot_id.clone();
        let previous_epoch = state.workspace_epoch;
        restore_planning_snapshot(&mut state, &target);
        state.workspace_epoch += 1;
        state.latest_change_seq += 1;
        let reset_change_seq = state.latest_change_seq;
        let workspace_epoch = state.workspace_epoch;
        state.changes.push(json!({
            "change_seq": reset_change_seq, "workspace_id": body["p_workspace_id"],
            "entity_type": "workspace_reset", "entity_id": body["p_workspace_id"], "operation": "reset",
            "entity_version": workspace_epoch, "changed_at": "2026-10-18T00:00:00Z"
        }));
        state.restore_operation_count += 1;
        state.restore_lock_token = None;
        return write_http_json(
            &mut stream,
            200,
            json!({
                "restored": true, "targetSnapshotId": target_snapshot_id, "safetySnapshotId": safety_snapshot_id,
                "previousEpoch": previous_epoch, "workspaceEpoch": state.workspace_epoch, "resetChangeSeq": reset_change_seq
            }),
        );
    }
    if request_line.contains("/rest/v1/rpc/cloud_snapshot_get") {
        state.snapshot_get_count += 1;
        return write_http_json(&mut stream, 200, planning_snapshot(&state));
    }
    if request_line.contains("/rest/v1/rpc/cloud_operation_history_list") {
        let history_count = if state.operation_history_item_count > 0 {
            state.operation_history_item_count
        } else if state.restore_operation_count > 0 {
            1
        } else {
            0
        };
        let all_items = if history_count > 0 {
            (0..history_count).map(|index| json!({
                "id": format!("restore-log-{index}"), "operationId": format!("restore-operation-{index}"), "businessType": "restore",
                "source": "restore", "deviceId": "device-a", "deviceName": "设备 A",
                "summary": "将工作空间回滚到历史镜像", "affectedEntityCount": 1,
                "occurredAt": format!("2026-10-{:02}T00:00:00Z", 18 - (index / 24)), "expiresAt": "2027-01-16T00:00:00Z", "metadata": {}
            })).collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let limit = body["p_limit"].as_u64().unwrap_or(50) as usize;
        let offset = body["p_cursor_id"]
            .as_str()
            .and_then(|value| value.strip_prefix("cursor-"))
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        let items = all_items
            .iter()
            .skip(offset)
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        let next_offset = offset + items.len();
        let next_cursor = (next_offset < all_items.len()).then(
            || json!({"occurredAt": "2026-10-01T00:00:00Z", "id": format!("cursor-{next_offset}")}),
        );
        return write_http_json(
            &mut stream,
            200,
            json!({ "items": items, "nextCursor": next_cursor }),
        );
    }
    if request_line.contains("/rest/v1/rpc/cloud_incremental_pull") {
        state.incremental_pull_count += 1;
        if request_epoch != state.workspace_epoch {
            return write_http_json(
                &mut stream,
                409,
                json!({ "message": format!("WORKSPACE_EPOCH_STALE: client={request_epoch}, server={}", state.workspace_epoch) }),
            );
        }
        if state.force_reset_required_once {
            state.force_reset_required_once = false;
            return write_http_json(
                &mut stream,
                200,
                json!({
                    "changes": [],
                    "entities": [],
                    "covered_change_seq": body["p_after_change_seq"].as_i64().unwrap_or_default(),
                    "latest_change_seq": state.latest_change_seq,
                    "has_more": false,
                    "reset_required": true,
                    "workspace_epoch": state.workspace_epoch
                }),
            );
        }
        return write_http_json(&mut stream, 200, planning_incremental_pull(&state, &body));
    }
    if request_line.contains("/rest/v1/workspace_changes?") {
        let after = request_line
            .split("change_seq=gt.")
            .nth(1)
            .and_then(|value| value.split('&').next())
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or_default();
        let changes = state
            .changes
            .iter()
            .filter(|change| change["change_seq"].as_i64().unwrap_or_default() > after)
            .cloned()
            .collect::<Vec<_>>();
        return write_http_json(&mut stream, 200, Value::Array(changes));
    }
    if request_line.contains("/rest/v1/rpc/timer_sync_context_get") {
        let entity_id = body["p_entity_id"].as_str();
        let authoritative = entity_id
            .and_then(|id| state.time_entries.get(id).cloned())
            .or_else(|| {
                state
                    .time_entries
                    .values()
                    .find(|entry| {
                        entry["source_type"].as_str() == Some("timer")
                            && matches!(entry["state"].as_str(), Some("running" | "paused"))
                            && entry["deleted_at"].is_null()
                    })
                    .cloned()
            });
        return write_http_json(
            &mut stream,
            200,
            json!({
                "processedOperation": null,
                "entityId": entity_id,
                "currentVersion": authoritative.as_ref().and_then(|entry| entry["version"].as_i64()),
                "authoritative": authoritative
            }),
        );
    }
    if request_line.contains("/rest/v1/rpc/tracking_lease_takeover_for_timer_write") {
        let device_id = body["p_device_id"].as_str().unwrap();
        let lease_token = body["p_lease_token"].as_str().unwrap();
        let previous_holder = state.tracking_lease_holder_device_id.clone();
        let takeover = previous_holder
            .as_deref()
            .is_some_and(|holder| holder != device_id);
        state.tracking_lease_holder_device_id = Some(device_id.to_string());
        state.tracking_lease_token = Some(lease_token.to_string());
        state.tracking_lease_version += 1;
        state.tracking_lease_expired = false;
        let authoritative_timer = takeover.then(|| {
            state
                .time_entries
                .values()
                .filter(|entry| {
                    entry["source_type"].as_str() == Some("timer")
                        && matches!(entry["state"].as_str(), Some("running" | "paused"))
                        && entry["deleted_at"].is_null()
                })
                .max_by_key(|entry| entry["started_at"].as_i64().unwrap_or_default())
                .cloned()
        });
        return write_http_json(
            &mut stream,
            200,
            json!({
                "acquired": true,
                "leaseToken": lease_token,
                "expiresAt": "2099-01-01T00:00:45Z",
                "version": state.tracking_lease_version,
                "takeover": takeover,
                "previousHolderDeviceId": previous_holder,
                "authoritativeTimer": authoritative_timer.flatten()
            }),
        );
    }
    if request_line.contains("/rest/v1/rpc/tracking_lease_acquire") {
        let device_id = body["p_device_id"].as_str().unwrap();
        let lease_token = body["p_lease_token"].as_str().unwrap();
        let previous_holder = state.tracking_lease_holder_device_id.clone();
        let takeover = state.tracking_lease_expired
            && previous_holder
                .as_deref()
                .is_some_and(|holder| holder != device_id);
        if !state.tracking_lease_expired
            && previous_holder
                .as_deref()
                .is_some_and(|holder| holder != device_id)
        {
            return write_http_json(
                &mut stream,
                200,
                json!({
                    "acquired": false,
                    "holderDeviceId": previous_holder,
                    "expiresAt": "2099-01-01T00:00:45Z",
                    "takeover": false
                }),
            );
        }
        state.tracking_lease_holder_device_id = Some(device_id.to_string());
        state.tracking_lease_token = Some(lease_token.to_string());
        state.tracking_lease_version += 1;
        state.tracking_lease_expired = false;
        let authoritative_timer = takeover.then(|| {
            state
                .time_entries
                .values()
                .filter(|entry| {
                    entry["source_type"].as_str() == Some("timer")
                        && matches!(entry["state"].as_str(), Some("running" | "paused"))
                        && entry["deleted_at"].is_null()
                })
                .max_by_key(|entry| entry["started_at"].as_i64().unwrap_or_default())
                .cloned()
        });
        return write_http_json(
            &mut stream,
            200,
            json!({
                "acquired": true,
                "leaseToken": lease_token,
                "expiresAt": "2099-01-01T00:00:45Z",
                "version": state.tracking_lease_version,
                "takeover": takeover,
                "previousHolderDeviceId": previous_holder,
                "authoritativeTimer": authoritative_timer.flatten()
            }),
        );
    }
    if request_line.contains("/rest/v1/rpc/tracking_lease_renew") {
        let device_id = body["p_device_id"].as_str().unwrap();
        let lease_token = body["p_lease_token"].as_str().unwrap();
        if state.tracking_lease_expired
            || state.tracking_lease_holder_device_id.as_deref() != Some(device_id)
            || state.tracking_lease_token.as_deref() != Some(lease_token)
        {
            return write_http_json(&mut stream, 409, json!({ "message": "LEASE_EXPIRED" }));
        }
        state.tracking_lease_version += 1;
        return write_http_json(
            &mut stream,
            200,
            json!({
                "renewed": true,
                "leaseToken": lease_token,
                "expiresAt": "2099-01-01T00:00:45Z",
                "version": state.tracking_lease_version,
                "takeover": false
            }),
        );
    }
    if request_line.contains("/rest/v1/rpc/tracking_lease_release") {
        let device_id = body["p_device_id"].as_str().unwrap();
        let lease_token = body["p_lease_token"].as_str().unwrap();
        let released = state.tracking_lease_holder_device_id.as_deref() == Some(device_id)
            && state.tracking_lease_token.as_deref() == Some(lease_token);
        if released {
            state.tracking_lease_holder_device_id = None;
            state.tracking_lease_token = None;
            state.tracking_lease_version += 1;
            state.tracking_lease_expired = false;
        }
        return write_http_json(&mut stream, 200, json!(released));
    }
    if request_line.contains("/rest/v1/rpc/timer_transition_apply") {
        let workspace_id = body["p_workspace_id"].as_str().unwrap();
        let operation_id = body["p_operation_id"].as_str().unwrap();
        let payload = &body["p_payload"];
        let previous = payload
            .get("previous_entry")
            .filter(|value| !value.is_null());
        let task = payload.get("task").filter(|value| !value.is_null());
        let occurrence = payload.get("occurrence").filter(|value| !value.is_null());
        let reopen_base_version = payload
            .get("reopen_task_base_version")
            .and_then(Value::as_i64);
        let active = payload
            .get("active_entry")
            .filter(|value| !value.is_null())
            .unwrap();
        let remote_active = state
            .time_entries
            .values()
            .find(|entry| {
                matches!(entry["state"].as_str(), Some("running" | "paused"))
                    && entry["deleted_at"].is_null()
            })
            .cloned();
        match (previous, remote_active.as_ref()) {
            (None, Some(_)) => {
                return write_http_json(
                    &mut stream,
                    409,
                    json!({ "message": "VERSION_CONFLICT: remote timer is active" }),
                )
            }
            (Some(previous), Some(remote))
                if remote["id"].as_str() != previous["id"].as_str()
                    || remote["version"].as_i64() != payload["previous_base_version"].as_i64() =>
            {
                return write_http_json(
                    &mut stream,
                    409,
                    json!({ "message": "VERSION_CONFLICT: remote timer changed" }),
                );
            }
            (Some(_), None) => {
                return write_http_json(
                    &mut stream,
                    409,
                    json!({ "message": "VERSION_CONFLICT: remote timer changed" }),
                )
            }
            _ => {}
        }
        if let Some(expected_version) = reopen_base_version {
            let Some(task) = task else {
                return write_http_json(
                    &mut stream,
                    409,
                    json!({ "message": "VALIDATION_ERROR: missing reopen task" }),
                );
            };
            let Some(remote_task) = state.tasks.get(task["id"].as_str().unwrap()) else {
                return write_http_json(
                    &mut stream,
                    409,
                    json!({ "message": "VERSION_CONFLICT: completed task changed" }),
                );
            };
            if remote_task["status"].as_str() != Some("done")
                || remote_task["version"].as_i64() != Some(expected_version)
            {
                return write_http_json(
                    &mut stream,
                    409,
                    json!({ "message": "VERSION_CONFLICT: completed task changed" }),
                );
            }
        }
        if let Some(previous) = previous {
            state.time_entries.insert(
                previous["id"].as_str().unwrap().to_string(),
                previous.clone(),
            );
            record_change(
                &mut state,
                workspace_id,
                "time_entries",
                previous["id"].as_str().unwrap(),
                "update",
                previous["version"].as_i64().unwrap_or(1),
            );
        }
        if let Some(task) = task {
            state
                .tasks
                .insert(task["id"].as_str().unwrap().to_string(), task.clone());
            record_change(
                &mut state,
                workspace_id,
                "tasks",
                task["id"].as_str().unwrap(),
                if reopen_base_version.is_some() {
                    "update"
                } else {
                    "insert"
                },
                task["version"].as_i64().unwrap_or(1),
            );
        }
        let authoritative_occurrence = if let Some(occurrence) = occurrence {
            if occurrence["task_id"] != active["default_task_id"]
                || occurrence["occurrence_date"] != active["work_date"]
                || occurrence["version"].as_i64() != Some(1)
                || occurrence["status"].as_str() != Some("open")
            {
                return write_http_json(
                    &mut stream,
                    409,
                    json!({ "message": "VALIDATION_ERROR: timer occurrence identity mismatch" }),
                );
            }
            let key = format!(
                "{}|{}",
                occurrence["task_id"].as_str().unwrap(),
                occurrence["occurrence_date"].as_str().unwrap()
            );
            if !state.occurrences.contains_key(&key) {
                state.occurrences.insert(key.clone(), occurrence.clone());
                record_change(
                    &mut state,
                    workspace_id,
                    "task_occurrences",
                    &key,
                    "insert",
                    1,
                );
            }
            state.occurrences.get(&key).cloned()
        } else {
            None
        };
        state
            .time_entries
            .insert(active["id"].as_str().unwrap().to_string(), active.clone());
        record_change(
            &mut state,
            workspace_id,
            "time_entries",
            active["id"].as_str().unwrap(),
            "insert",
            active["version"].as_i64().unwrap_or(1),
        );
        return write_http_json(
            &mut stream,
            200,
            json!({
                "accepted": true,
                "operationId": operation_id,
                "activeEntry": active,
                "occurrence": authoritative_occurrence
            }),
        );
    }
    if request_line.contains("/rest/v1/rpc/actual_adjustment_batch_apply") {
        if request_epoch != state.workspace_epoch {
            return write_http_json(
                &mut stream,
                409,
                json!({ "message": format!("WORKSPACE_EPOCH_STALE: server={}", state.workspace_epoch) }),
            );
        }
        let workspace_id = body["p_workspace_id"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let device_id = body["p_device_id"].as_str().unwrap_or_default().to_string();
        let payload = body["p_payload"].clone();
        return match planning_apply_actual_adjustment_batch(
            &mut state,
            &workspace_id,
            &device_id,
            &payload,
        ) {
            Ok(result) => write_http_json(&mut stream, 200, result),
            Err(error) => write_http_json(&mut stream, 409, json!({ "message": error })),
        };
    }
    if !request_line.contains("/rest/v1/rpc/cloud_apply_patch") {
        return write_http_json(
            &mut stream,
            409,
            json!({ "message": format!("unexpected request: {request_line}") }),
        );
    }
    if request_epoch != state.workspace_epoch {
        return write_http_json(
            &mut stream,
            409,
            json!({ "message": format!("WORKSPACE_EPOCH_STALE: server={}", state.workspace_epoch) }),
        );
    }
    apply_patch_started.store(true, std::sync::atomic::Ordering::Release);
    if fail_apply_once.swap(false, std::sync::atomic::Ordering::AcqRel) {
        return write_http_json(
            &mut stream,
            503,
            json!({ "message": "SERVICE_UNAVAILABLE: 测试注入的暂时离线" }),
        );
    }
    let delay = apply_patch_delay_millis.swap(0, std::sync::atomic::Ordering::AcqRel);
    if delay > 0 {
        thread::sleep(Duration::from_millis(delay));
    }
    let workspace_id = body["p_workspace_id"].as_str().unwrap();
    let entity_type = body["p_entity_type"].as_str().unwrap();
    let entity_id = body["p_entity_id"].as_str().unwrap();
    let base_version = body["p_base_version"].as_i64();
    let payload = body["p_payload"].clone();
    match entity_type {
        "task" => {
            let current = state
                .tasks
                .get(entity_id)
                .and_then(|row| row["version"].as_i64());
            if current != base_version {
                return write_http_json(
                    &mut stream,
                    409,
                    json!({ "message": "SYNC_CONFLICT: task version changed" }),
                );
            }
            let version = payload["version"].as_i64().unwrap_or(1);
            state.tasks.insert(entity_id.to_string(), payload);
            record_change(
                &mut state,
                workspace_id,
                "tasks",
                entity_id,
                "update",
                version,
            );
        }
        "time_entry" => {
            if body["p_operation_type"].as_str() == Some("timer_rollover_create") {
                let chain_id = payload["timer_chain_id"].as_str();
                let work_date = payload["work_date"].as_str();
                let authoritative = state.time_entries.values().find(|entry| {
                    entry["source_type"].as_str() == Some("timer")
                        && entry["timer_chain_id"].as_str() == chain_id
                        && entry["work_date"].as_str() == work_date
                        && entry["deleted_at"].is_null()
                });
                if let Some(authoritative) = authoritative.cloned() {
                    if authoritative["id"].as_str() != Some(entity_id) {
                        return write_http_json(
                            &mut stream,
                            200,
                            json!({
                                "superseded": true,
                                "candidateEntityId": entity_id,
                                "authoritative": authoritative
                            }),
                        );
                    }
                }
            }
            let current = state
                .time_entries
                .get(entity_id)
                .and_then(|row| row["version"].as_i64());
            if current != base_version {
                return write_http_json(
                    &mut stream,
                    409,
                    json!({ "message": "SYNC_CONFLICT: time entry version changed" }),
                );
            }
            let version = payload["version"].as_i64().unwrap_or(1);
            state.time_entries.insert(entity_id.to_string(), payload);
            record_change(
                &mut state,
                workspace_id,
                "time_entries",
                entity_id,
                "update",
                version,
            );
        }
        "report" => {
            let current = state
                .reports
                .get(entity_id)
                .and_then(|row| row["version"].as_i64());
            if current != base_version {
                return write_http_json(
                    &mut stream,
                    409,
                    json!({ "message": "SYNC_CONFLICT: report version changed" }),
                );
            }
            let version = payload["version"].as_i64().unwrap_or(1);
            state.reports.insert(entity_id.to_string(), payload);
            record_change(
                &mut state,
                workspace_id,
                "reports",
                entity_id,
                "update",
                version,
            );
        }
        "report_template" => {
            let current = state
                .report_templates
                .get(entity_id)
                .and_then(|row| row["version"].as_i64());
            if current != base_version {
                return write_http_json(
                    &mut stream,
                    409,
                    json!({ "message": "SYNC_CONFLICT: report template version changed" }),
                );
            }
            let version = payload["version"].as_i64().unwrap_or(1);
            let report_type = payload["report_type"].as_str().map(str::to_string);
            let mut secondary_changes = Vec::new();
            if (payload["is_default"].as_bool() == Some(true)
                || payload["is_default"].as_i64() == Some(1))
                && payload["deleted_at"].is_null()
            {
                for (other_id, template) in &mut state.report_templates {
                    if other_id != entity_id
                        && template["report_type"].as_str() == report_type.as_deref()
                        && template["deleted_at"].is_null()
                        && (template["is_default"].as_bool() == Some(true)
                            || template["is_default"].as_i64() == Some(1))
                    {
                        template["is_default"] = Value::Bool(false);
                        let next_version = template["version"].as_i64().unwrap_or_default() + 1;
                        template["version"] = Value::Number(next_version.into());
                        secondary_changes.push((other_id.clone(), next_version));
                    }
                }
            }
            state
                .report_templates
                .insert(entity_id.to_string(), payload.clone());
            if let Some(replacement_id) = payload["replacement_template_id"].as_str() {
                if let Some(replacement) = state.report_templates.get_mut(replacement_id) {
                    replacement["is_default"] = Value::Bool(true);
                    let next_version = replacement["version"].as_i64().unwrap_or_default() + 1;
                    replacement["version"] = Value::Number(next_version.into());
                    secondary_changes.push((replacement_id.to_string(), next_version));
                }
            }
            for (changed_id, changed_version) in secondary_changes {
                record_change(
                    &mut state,
                    workspace_id,
                    "report_templates",
                    &changed_id,
                    "update",
                    changed_version,
                );
            }
            record_change(
                &mut state,
                workspace_id,
                "report_templates",
                entity_id,
                if payload["deleted_at"].is_null() {
                    "update"
                } else {
                    "delete"
                },
                version,
            );
        }
        "task_day_plan" => {
            let current = state
                .day_plans
                .get(entity_id)
                .and_then(|row| row["version"].as_i64());
            if current != base_version {
                return write_http_json(
                    &mut stream,
                    409,
                    json!({
                        "message": format!("SYNC_CONFLICT: expected version {base_version:?}, current version {current:?}")
                    }),
                );
            }
            let version = payload["version"].as_i64().unwrap_or(1);
            state.day_plans.insert(entity_id.to_string(), payload);
            record_change(
                &mut state,
                workspace_id,
                "task_day_plans",
                entity_id,
                "update",
                version,
            );
        }
        "task_occurrence" => {
            let current = state
                .occurrences
                .get(entity_id)
                .and_then(|row| row["version"].as_i64());
            if current != base_version {
                return write_http_json(
                    &mut stream,
                    409,
                    json!({
                        "message": format!(
                            "SYNC_CONFLICT: expected version {}, current version {}",
                            base_version.map(|value| value.to_string()).unwrap_or_else(|| "<NULL>".to_string()),
                            current.map(|value| value.to_string()).unwrap_or_else(|| "<NULL>".to_string())
                        )
                    }),
                );
            }
            let version = payload["version"].as_i64().unwrap_or(1);
            state.occurrences.insert(entity_id.to_string(), payload);
            record_change(
                &mut state,
                workspace_id,
                "task_occurrences",
                entity_id,
                "update",
                version,
            );
        }
        "task_daily_estimate" => {
            let current = state
                .daily_estimates
                .get(entity_id)
                .and_then(|row| row["version"].as_i64());
            if current != base_version
                && !(payload["deleted"].as_bool() == Some(true) && current.is_none())
            {
                return write_http_json(
                    &mut stream,
                    409,
                    json!({ "message": format!("SYNC_CONFLICT: expected version {base_version:?}, current version {current:?}") }),
                );
            }
            let version = payload["version"].as_i64().unwrap_or(1);
            let operation = if payload["deleted"].as_bool() == Some(true) {
                state.daily_estimates.remove(entity_id);
                "delete"
            } else {
                state.daily_estimates.insert(entity_id.to_string(), payload);
                "update"
            };
            record_change(
                &mut state,
                workspace_id,
                "task_daily_estimates",
                entity_id,
                operation,
                version,
            );
        }
        "task_recurrence_rule" => {
            let task_id = payload["task_id"].as_str().unwrap();
            let task_version = state.tasks[task_id]["version"].as_i64().unwrap();
            let current_rule = state
                .recurrence_rules
                .get(task_id)
                .and_then(|rules| rules.iter().find(|rule| rule["effective_end"].is_null()))
                .and_then(|rule| rule["version"].as_i64());
            if current_rule != base_version
                || payload["task_expected_version"].as_i64() != Some(task_version)
            {
                return write_http_json(
                    &mut stream,
                    409,
                    json!({ "message": "SYNC_CONFLICT: recurrence version changed" }),
                );
            }
            state.recurrence_rules.insert(
                task_id.to_string(),
                payload["rules"].as_array().unwrap().clone(),
            );
            state.tasks.get_mut(task_id).unwrap()["version"] = payload["task_version"].clone();
            let version = payload["version"].as_i64().unwrap_or_default();
            record_change(
                &mut state,
                workspace_id,
                "task_recurrence_rules",
                task_id,
                "update",
                version,
            );
            record_change(
                &mut state,
                workspace_id,
                "tasks",
                task_id,
                "update",
                payload["task_version"].as_i64().unwrap(),
            );
        }
        _ => {
            return write_http_json(
                &mut stream,
                409,
                json!({ "message": format!("UNSUPPORTED_OPERATION: {entity_type}") }),
            );
        }
    }
    let authoritative = match entity_type {
        "report_template" => state.report_templates.get(entity_id).cloned(),
        _ => None,
    };
    write_http_json(
        &mut stream,
        200,
        json!({
            "entityType": entity_type,
            "entityId": entity_id,
            "version": authoritative.as_ref().and_then(|row| row["version"].as_i64()),
            "authoritative": authoritative
        }),
    );
}

fn today_entries(database: &Database) -> time_tracking::TimeEntryListResult {
    time_tracking::list_time_entries(
        database,
        TimeEntryListRequest {
            work_date: chrono::Local::now().format("%Y-%m-%d").to_string(),
            include_breaks: true,
        },
    )
    .unwrap()
}

struct RealtimeTestConfig {
    project_url: String,
    anon_key: String,
    access_token: String,
}

fn realtime_test_config(database: &Database) -> RealtimeTestConfig {
    let session = supabase::current_session(database).unwrap();
    let api = supabase::client(database).unwrap();
    RealtimeTestConfig {
        project_url: api.project_url.clone(),
        anon_key: api.anon_key().to_string(),
        access_token: session.access_token,
    }
}

async fn subscribe_to_workspace_changes(
    config: RealtimeTestConfig,
    workspace_id: &str,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let mut url = Url::parse(&config.project_url).unwrap();
    let websocket_scheme = if url.scheme() == "https" { "wss" } else { "ws" };
    url.set_scheme(websocket_scheme).unwrap();
    url.set_path("/realtime/v1/websocket");
    url.set_query(None);
    url.query_pairs_mut()
        .append_pair("apikey", &config.anon_key)
        .append_pair("vsn", "1.0.0");
    let (mut socket, _) = connect_async(url.as_str()).await.unwrap();
    let topic = format!(
        "realtime:{}:workspace_changes:{workspace_id}",
        supabase::CLOUD_SCHEMA
    );
    socket
        .send(Message::Text(
            json!({
                "topic": topic,
                "event": "phx_join",
                "payload": {
                    "config": {
                        "broadcast": { "ack": false, "self": false },
                        "presence": { "key": "" },
                        "postgres_changes": [{
                            "event": "*",
                            "schema": supabase::CLOUD_SCHEMA,
                            "table": "workspace_changes",
                            "filter": format!("workspace_id=eq.{workspace_id}")
                        }],
                        "private": false
                    },
                    "access_token": config.access_token
                },
                "ref": "1",
                "join_ref": "1"
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();

    let joined = tokio::time::timeout(Duration::from_secs(15), async {
        while let Some(message) = socket.next().await {
            let message = message.unwrap();
            if let Message::Text(text) = message {
                let payload: Value = serde_json::from_str(text.as_ref()).unwrap_or(Value::Null);
                if payload.get("event").and_then(Value::as_str) == Some("phx_reply")
                    && payload.get("ref").and_then(Value::as_str) == Some("1")
                {
                    return payload.pointer("/payload/status").and_then(Value::as_str)
                        == Some("ok");
                }
            }
        }
        false
    })
    .await
    .expect("Supabase Realtime 订阅超时");
    assert!(joined, "Supabase Realtime 拒绝了 workspace_changes 订阅");
    socket
}

async fn wait_for_postgres_change(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) {
    let received = tokio::time::timeout(Duration::from_secs(15), async {
        while let Some(message) = socket.next().await {
            let message = message.unwrap();
            if let Message::Text(text) = message {
                let payload: Value = serde_json::from_str(text.as_ref()).unwrap_or(Value::Null);
                if payload.get("event").and_then(Value::as_str) == Some("postgres_changes") {
                    return true;
                }
            }
        }
        false
    })
    .await
    .expect("等待 Supabase Realtime 变更超时");
    assert!(received, "没有收到 workspace_changes 的 Realtime 通知");
}

fn configure_isolated_planning_device(
    database: &Database,
    base_url: &str,
    workspace_id: &str,
    user_id: &str,
) -> supabase::StorageModeSnapshot {
    for (key, value) in [
        ("supabase_project_url", json!(base_url)),
        ("supabase_anon_key", json!("anon-isolated-planning-test")),
        ("cloud_workspace_id", json!(workspace_id)),
        ("storage_mode", json!("cloud")),
    ] {
        crate::settings::update_setting(
            database,
            crate::settings::SettingsUpdate {
                scope: crate::settings::SettingsScope::Device,
                key: key.to_string(),
                value,
            },
        )
        .unwrap();
    }
    supabase::install_test_session_for_database(database, user_id).unwrap();
    let state = supabase::storage_mode(database).unwrap();
    database
        .open()
        .unwrap()
        .execute(
            "INSERT INTO local_sync_state(workspace_id, device_id, last_change_seq) VALUES (?1, ?2, 0)",
            rusqlite::params![workspace_id, state.device_id],
        )
        .unwrap();
    state
}

#[test]
fn isolated_supabase_planning_two_device_flow() {
    let directory = tempfile::tempdir().unwrap();
    let device_a = Database::initialize_at(directory.path().join("planning-a.sqlite3")).unwrap();
    let device_b = Database::initialize_at(directory.path().join("planning-b.sqlite3")).unwrap();
    let cloud = PlanningCloudServer::start();
    let workspace_id = Uuid::now_v7().to_string();
    let state_a = configure_isolated_planning_device(
        &device_a,
        &cloud.base_url,
        &workspace_id,
        "planning-user-a",
    );
    let state_b = configure_isolated_planning_device(
        &device_b,
        &cloud.base_url,
        &workspace_id,
        "planning-user-b",
    );
    let subject = subjects::list_subjects(&device_a).unwrap().remove(0);
    let task = create_task(&device_a, &subject.id, "隔离 Supabase 规划事项");
    cloud.seed(&device_a, &subject, &task);
    cloud_sync::pull_snapshot(&device_b).unwrap();
    let (_, snapshots_after_initialization) = cloud.request_counts();

    let today = chrono::Local::now().date_naive();
    let today_text = today.format("%Y-%m-%d").to_string();
    let tomorrow_text = today.succ_opt().unwrap().format("%Y-%m-%d").to_string();
    let next_text = today
        .succ_opt()
        .unwrap()
        .succ_opt()
        .unwrap()
        .format("%Y-%m-%d")
        .to_string();

    // Keep a local dirty task on device B while device A publishes that same
    // task plus unrelated time/report aggregates. The dirty task must be
    // deferred without blocking the other two entities in the same catch-up.
    let task_on_a = update_task_title(&device_a, &task, "设备 A 云端标题");
    cloud_sync::enqueue_entity(
        &device_a,
        "task_update",
        "task",
        Some(&task_on_a.id),
        Some(task.version),
        None,
    )
    .unwrap();
    let task_on_b = update_task_title(&device_b, &task, "设备 B 本地待同步标题");
    cloud_sync::enqueue_entity_deferred(
        &device_b,
        "task_update",
        "task",
        Some(&task_on_b.id),
        Some(task.version),
        Some(&Uuid::now_v7().to_string()),
    )
    .unwrap();
    let time_entry = time_tracking::create_manual_entry(
        &device_a,
        ManualEntryRequest {
            task_id: Some(task.id.clone()),
            work_date: today_text.clone(),
            started_at: None,
            ended_at: None,
            minutes: Some(30),
            note: Some("隔离双设备工时".to_string()),
            complete_task: false,
            task_expected_version: None,
            client_request_id: Uuid::now_v7().to_string(),
        },
    )
    .unwrap();
    cloud_sync::enqueue_entity(
        &device_a,
        "time_entry_create_manual",
        "time_entry",
        Some(&time_entry.id),
        None,
        None,
    )
    .unwrap();
    let report = reports::create_report(
        &device_a,
        ReportCreateRequest {
            report_type: "daily".to_string(),
            reference_date: today_text.clone(),
            subject_id: subject.id.clone(),
            task_ids: vec![task.id.clone()],
            template_id: None,
            client_request_id: Uuid::now_v7().to_string(),
        },
    )
    .unwrap();
    cloud_sync::enqueue_entity(
        &device_a,
        "report_create",
        "report",
        Some(&report.id),
        None,
        None,
    )
    .unwrap();
    assert_eq!(push_all(&device_a).conflicts, 0);
    cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: None,
        },
    )
    .unwrap();
    assert_eq!(
        list_tasks(&device_b)
            .into_iter()
            .find(|candidate| candidate.id == task.id)
            .map(|candidate| candidate.title),
        Some("设备 B 本地待同步标题".to_string())
    );
    assert!(
        today_entries(&device_b)
            .entries
            .iter()
            .any(|entry| entry.id == time_entry.id),
        "dirty 事项阻塞了无关工时聚合"
    );
    assert!(
        reports::list_reports(
            &device_b,
            ReportListRequest {
                report_type: None,
                subject_id: None,
                query: None,
            },
        )
        .unwrap()
        .reports
        .iter()
        .any(|candidate| candidate.id == report.id),
        "dirty 事项阻塞了无关报告聚合"
    );
    assert_eq!(
        cloud.request_counts().1,
        snapshots_after_initialization,
        "普通事项、工时和报告增量不应进入完整快照"
    );
    assert_eq!(push_all(&device_b).conflicts, 1);
    let task_conflict = cloud_sync::list_conflicts(&device_b)
        .unwrap()
        .into_iter()
        .find(|item| item.entity_type == "task")
        .expect("普通事项同实体 dirty 冲突没有进入冲突列表");
    cloud_sync::resolve_conflict(
        &device_b,
        CloudConflictResolveRequest {
            operation_id: task_conflict.operation_id,
            strategy: "use_cloud".to_string(),
        },
    )
    .unwrap();
    cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            // The production coordinator rewinds to the earliest deferred
            // change before this pull. The isolated test drives the pull
            // directly, so make that rewind explicit here.
            after_change_seq: Some(0),
            limit: None,
        },
    )
    .unwrap();
    assert_eq!(
        list_tasks(&device_b)
            .into_iter()
            .find(|candidate| candidate.id == task.id)
            .map(|candidate| candidate.title),
        Some("设备 A 云端标题".to_string())
    );
    let snapshots_after_business_incremental = cloud.request_counts().1;

    tasks::set_task_daily_estimate_for_cloud_test(
        &device_a,
        TaskDailyEstimateSetRequest {
            task_id: task.id.clone(),
            work_date: today_text.clone(),
            estimate_minutes: Some(45),
        },
        &state_a,
    )
    .unwrap();
    assert_eq!(
        cloud_sync::sync_status(&device_a)
            .unwrap()
            .pending_operations,
        1
    );
    let estimate_outbox: Vec<(String, String, Option<String>)> = device_a
        .open()
        .unwrap()
        .prepare(
            "SELECT operation_type,entity_type,depends_on_operation_id
             FROM sync_outbox ORDER BY created_at,rowid",
        )
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        estimate_outbox
            .iter()
            .map(|(operation, entity, _)| (operation.as_str(), entity.as_str()))
            .collect::<Vec<_>>(),
        vec![("task_daily_estimate_set", "task_daily_estimate")]
    );
    assert_eq!(estimate_outbox[0].2, None);
    assert_eq!(push_all(&device_a).conflicts, 0);
    cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: None,
        },
    )
    .unwrap();

    assert_eq!(daily_estimate(&device_b, &task.id, &today_text), Some(45));
    assert_eq!(day_plan_state(&device_b, &task.id, &today_text), None);
    let (incremental_pulls, snapshots_after_incremental) = cloud.request_counts();
    assert!(incremental_pulls > 0);
    assert_eq!(
        snapshots_after_incremental,
        snapshots_after_business_incremental
    );

    let batch_task = create_task(&device_a, &task.subject_id, "批量普通事项");
    let batch_recurring = create_task(&device_a, &task.subject_id, "批量重复事项");
    for created in [&batch_task, &batch_recurring] {
        cloud_sync::enqueue_entity(
            &device_a,
            "task_create",
            "task",
            Some(&created.id),
            None,
            None,
        )
        .unwrap();
    }
    assert_eq!(push_all(&device_a).conflicts, 0);
    cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: None,
        },
    )
    .unwrap();
    let batch_rule = recurring::save_rule_for_cloud_test(
        &device_a,
        RecurrenceSaveRequest {
            task_id: batch_recurring.id.clone(),
            task_expected_version: task_version(&device_a, &batch_recurring.id),
            frequency: "daily".to_string(),
            weekdays_mask: None,
            effective_start: today_text.clone(),
            rule_expected_version: None,
        },
        &state_a,
    )
    .unwrap();
    let batch_task = list_tasks(&device_a)
        .into_iter()
        .find(|candidate| candidate.id == batch_task.id)
        .unwrap();
    let batch_recurring = list_tasks(&device_a)
        .into_iter()
        .find(|candidate| candidate.id == batch_recurring.id)
        .unwrap();
    tasks::set_task_daily_estimate_batch_for_cloud_test(
        &device_a,
        TaskDailyEstimateBatchSetRequest {
            subject_id: batch_task.subject_id.clone(),
            work_date: today_text.clone(),
            items: vec![
                TaskDailyEstimateBatchItem {
                    task_id: batch_task.id.clone(),
                    expected_task_version: batch_task.version,
                    expected_daily_estimate_version: batch_task.daily_estimate_version,
                    estimate_minutes: Some(25),
                },
                TaskDailyEstimateBatchItem {
                    task_id: batch_recurring.id.clone(),
                    expected_task_version: batch_recurring.version,
                    expected_daily_estimate_version: batch_recurring.daily_estimate_version,
                    estimate_minutes: Some(35),
                },
            ],
        },
        &state_a,
    )
    .unwrap();
    assert_eq!(push_all(&device_a).conflicts, 0);
    cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: None,
        },
    )
    .unwrap();
    assert_eq!(
        daily_estimate(&device_b, &batch_task.id, &today_text),
        Some(25)
    );
    assert_eq!(
        daily_estimate(&device_b, &batch_recurring.id, &today_text),
        Some(35)
    );
    assert_eq!(task_estimate(&device_b, &batch_recurring.id), Some(35));
    assert!(batch_rule.version > 0);

    let rule = recurring::save_rule_for_cloud_test(
        &device_a,
        RecurrenceSaveRequest {
            task_id: task.id.clone(),
            task_expected_version: task_version(&device_a, &task.id),
            frequency: "daily".to_string(),
            weekdays_mask: None,
            effective_start: today_text.clone(),
            rule_expected_version: None,
        },
        &state_a,
    )
    .unwrap();
    assert_eq!(push_all(&device_a).conflicts, 0);
    cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: None,
        },
    )
    .unwrap();

    // Materialize and synchronize the recurring occurrence before creating
    // the estimate race, so the conflict is about the estimate identity and
    // not an earlier occurrence dependency.
    tasks::set_task_daily_estimate_for_cloud_test(
        &device_a,
        TaskDailyEstimateSetRequest {
            task_id: task.id.clone(),
            work_date: today_text.clone(),
            estimate_minutes: Some(45),
        },
        &state_a,
    )
    .unwrap();
    assert_eq!(push_all(&device_a).conflicts, 0);
    cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: None,
        },
    )
    .unwrap();

    tasks::set_task_daily_estimate_for_cloud_test(
        &device_a,
        TaskDailyEstimateSetRequest {
            task_id: task.id.clone(),
            work_date: today_text.clone(),
            estimate_minutes: Some(60),
        },
        &state_a,
    )
    .unwrap();
    assert_eq!(push_all(&device_a).conflicts, 0);
    tasks::set_task_daily_estimate_for_cloud_test(
        &device_b,
        TaskDailyEstimateSetRequest {
            task_id: task.id.clone(),
            work_date: today_text.clone(),
            estimate_minutes: Some(90),
        },
        &state_b,
    )
    .unwrap();
    assert_eq!(push_all(&device_b).conflicts, 1);
    let estimate_conflicts = cloud_sync::list_conflicts(&device_b).unwrap();
    let estimate_conflict = estimate_conflicts
        .into_iter()
        .find(|item| item.entity_type == "task_daily_estimate")
        .expect("同日期预估冲突没有进入冲突列表");
    cloud_sync::resolve_conflict(
        &device_b,
        CloudConflictResolveRequest {
            operation_id: estimate_conflict.operation_id,
            strategy: "use_cloud".to_string(),
        },
    )
    .unwrap();
    cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: None,
        },
    )
    .unwrap();
    assert_eq!(daily_estimate(&device_b, &task.id, &today_text), Some(60));

    tasks::set_task_daily_estimate_for_cloud_test(
        &device_a,
        TaskDailyEstimateSetRequest {
            task_id: task.id.clone(),
            work_date: tomorrow_text.clone(),
            estimate_minutes: Some(30),
        },
        &state_a,
    )
    .unwrap();
    tasks::set_task_daily_estimate_for_cloud_test(
        &device_b,
        TaskDailyEstimateSetRequest {
            task_id: task.id.clone(),
            work_date: next_text.clone(),
            estimate_minutes: Some(20),
        },
        &state_b,
    )
    .unwrap();
    assert_eq!(push_all(&device_a).conflicts, 0);
    let instance_push = push_all(&device_b);
    assert_eq!(
        instance_push.conflicts,
        0,
        "今日实例内容/实际同步冲突: {:?}",
        cloud_sync::list_conflicts(&device_b).unwrap()
    );
    cloud_sync::pull(
        &device_a,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: Some(1),
        },
    )
    .unwrap();
    while cloud_sync::pull(
        &device_a,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: Some(1),
        },
    )
    .unwrap()
    .has_more
    {}
    cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: Some(1),
        },
    )
    .unwrap();
    while cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: Some(1),
        },
    )
    .unwrap()
    .has_more
    {}
    for database in [&device_a, &device_b] {
        assert_eq!(daily_estimate(database, &task.id, &tomorrow_text), Some(30));
        assert_eq!(daily_estimate(database, &task.id, &next_text), Some(20));
    }

    let changed_rule = recurring::save_rule_for_cloud_test(
        &device_a,
        RecurrenceSaveRequest {
            task_id: task.id.clone(),
            task_expected_version: task_version(&device_a, &task.id),
            frequency: "weekdays".to_string(),
            weekdays_mask: None,
            effective_start: today_text.clone(),
            rule_expected_version: Some(rule.version),
        },
        &state_a,
    )
    .unwrap();
    assert_eq!(push_all(&device_a).conflicts, 0);
    recurring::save_rule_for_cloud_test(
        &device_b,
        RecurrenceSaveRequest {
            task_id: task.id.clone(),
            task_expected_version: task_version(&device_b, &task.id),
            frequency: "weekly".to_string(),
            weekdays_mask: Some(21),
            effective_start: today_text.clone(),
            rule_expected_version: Some(rule.version),
        },
        &state_b,
    )
    .unwrap();
    assert_eq!(push_all(&device_b).conflicts, 1);
    let rule_conflict = cloud_sync::list_conflicts(&device_b)
        .unwrap()
        .into_iter()
        .find(|item| item.entity_type == "task_recurrence_rule")
        .unwrap();
    cloud_sync::resolve_conflict(
        &device_b,
        CloudConflictResolveRequest {
            operation_id: rule_conflict.operation_id,
            strategy: "use_cloud".to_string(),
        },
    )
    .unwrap();
    cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: None,
        },
    )
    .unwrap();
    let local_workspace_id: String = device_b
        .open()
        .unwrap()
        .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
        .unwrap();
    let active_rule =
        recurring::get_active_rule(&device_b.open().unwrap(), &local_workspace_id, &task.id)
            .unwrap()
            .unwrap();
    assert_eq!(
        (active_rule.frequency, active_rule.version),
        ("weekdays".to_string(), changed_rule.version)
    );

    tasks::set_task_daily_estimate_for_cloud_test(
        &device_a,
        TaskDailyEstimateSetRequest {
            task_id: task.id.clone(),
            work_date: today_text.clone(),
            estimate_minutes: None,
        },
        &state_a,
    )
    .unwrap();
    assert_eq!(push_all(&device_a).conflicts, 0);
    cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: None,
        },
    )
    .unwrap();
    assert_eq!(daily_estimate(&device_b, &task.id, &today_text), None);

    let snapshots_before_reset = cloud.request_counts().1;
    cloud.require_snapshot_reset_once();
    cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: None,
        },
    )
    .unwrap();
    assert_eq!(cloud.request_counts().1, snapshots_before_reset + 1);
}

#[test]
fn isolated_supabase_today_plan_two_device_flow() {
    let directory = tempfile::tempdir().unwrap();
    let device_a = Database::initialize_at(directory.path().join("today-plan-a.sqlite3")).unwrap();
    let device_b = Database::initialize_at(directory.path().join("today-plan-b.sqlite3")).unwrap();
    let cloud = PlanningCloudServer::start();
    let workspace_id = Uuid::now_v7().to_string();
    let state_a = configure_isolated_planning_device(
        &device_a,
        &cloud.base_url,
        &workspace_id,
        "today-plan-user-a",
    );
    let state_b = configure_isolated_planning_device(
        &device_b,
        &cloud.base_url,
        &workspace_id,
        "today-plan-user-b",
    );
    let subject = subjects::list_subjects(&device_a).unwrap().remove(0);
    let today = chrono::Local::now().date_naive();
    let today_text = today.format("%Y-%m-%d").to_string();
    let tomorrow_text = today.succ_opt().unwrap().format("%Y-%m-%d").to_string();
    let manual = create_task(&device_a, &subject.id, "无预计手动加入");
    let recurring = create_task(&device_a, &subject.id, "重复事项移出");
    let concurrent = create_task(&device_a, &subject.id, "并发成员修改");
    let mixed = create_task(&device_a, &subject.id, "并发成员预计修改");
    let retriable = create_task(&device_a, &subject.id, "离线重试事项");
    for task in [&manual, &recurring, &concurrent, &mixed, &retriable] {
        cloud.seed(&device_a, &subject, task);
    }
    cloud_sync::pull_snapshot(&device_b).unwrap();

    // 无预计手动加入只同步成员，不产生当日预计。
    let manual_plan = today_plans::set_day_plan_for_cloud_test(
        &device_a,
        TaskDayPlanSetRequest {
            task_id: manual.id.clone(),
            work_date: today_text.clone(),
            state: "included".to_string(),
            expected_version: None,
        },
        &state_a,
    )
    .unwrap();
    assert_eq!(manual_plan.version, 1);
    assert_eq!(
        cloud_sync::sync_status(&device_a)
            .unwrap()
            .pending_operations,
        2
    );
    assert_eq!(push_all(&device_a).conflicts, 0);
    pull_all(&device_b);
    assert_eq!(
        day_plan_state(&device_b, &manual.id, &today_text).as_deref(),
        Some("included")
    );
    assert_eq!(daily_estimate(&device_b, &manual.id, &today_text), None);
    assert!(is_today_member(
        &device_b,
        &subject.id,
        &today_text,
        &manual.id
    ));
    let manual_instance_b = today_instance(&device_b, &subject.id, &today_text, &manual.id);
    assert_ne!(manual_instance_b.id, manual.id);
    let renamed_instance = tasks::update_task_for_cloud_test(
        &device_b,
        TaskUpdateRequest {
            id: manual_instance_b.id.clone(),
            expected_version: manual_instance_b.version,
            title: Some("设备 B 今日独立标题".to_string()),
            parent_id: None,
            planned_date: None,
            estimate_minutes: None,
            note: None,
            project_name: None,
            solution_name: None,
        },
        &state_b,
    )
    .unwrap();
    let fingerprint = actual_time_adjustments::snapshot(
        &device_b,
        ActualAdjustmentSnapshotRequest {
            subject_id: subject.id.clone(),
            work_date: today_text.clone(),
            execution_only: Some(true),
        },
    )
    .unwrap()
    .tasks
    .into_iter()
    .find(|task| task.id == renamed_instance.id)
    .unwrap()
    .adjustment_fingerprint;
    actual_time_adjustments::apply_batch(
        &device_b,
        ActualAdjustmentBatchRequest {
            subject_id: subject.id.clone(),
            work_date: today_text.clone(),
            client_request_id: Uuid::now_v7().to_string(),
            batch_id: Uuid::now_v7().to_string(),
            note: None,
            day_instance_date: Some(today_text.clone()),
            items: vec![ActualAdjustmentBatchItem {
                task_id: renamed_instance.id.clone(),
                expected_task_version: renamed_instance.version,
                baseline_minutes: 0,
                target_minutes: 25,
                delta_minutes: 25,
                adjustment_fingerprint: fingerprint,
            }],
            new_tasks: vec![],
        },
    )
    .unwrap();
    let instance_push = push_all(&device_b);
    assert_eq!(
        instance_push.conflicts,
        0,
        "今日实例内容/实际同步冲突: {:?}",
        cloud_sync::list_conflicts(&device_b).unwrap()
    );
    pull_all(&device_a);
    let manual_instance_a = today_instance(&device_a, &subject.id, &today_text, &manual.id);
    assert_eq!(manual_instance_a.id, renamed_instance.id);
    assert_eq!(manual_instance_a.title, "设备 B 今日独立标题");
    assert_eq!(task_title(&device_a, &manual.id), "无预计手动加入");
    let local_workspace_a: String = device_a
        .open()
        .unwrap()
        .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        actual_time_adjustments::direct_actual_minutes(
            &device_a.open().unwrap(),
            &local_workspace_a,
            &manual.id,
            Some(&today_text),
        )
        .unwrap(),
        0
    );
    assert_eq!(
        actual_time_adjustments::direct_actual_minutes(
            &device_a.open().unwrap(),
            &local_workspace_a,
            &manual_instance_a.id,
            Some(&today_text),
        )
        .unwrap(),
        25
    );

    // 重复规则命中的事项默认属于今日，移出后仅当天生效。
    recurring::save_rule_for_cloud_test(
        &device_a,
        RecurrenceSaveRequest {
            task_id: recurring.id.clone(),
            task_expected_version: task_version(&device_a, &recurring.id),
            frequency: "daily".to_string(),
            weekdays_mask: None,
            effective_start: today_text.clone(),
            rule_expected_version: None,
        },
        &state_a,
    )
    .unwrap();
    assert_eq!(push_all(&device_a).conflicts, 0);
    cloud_sync::pull_snapshot(&device_b).unwrap();
    assert!(
        is_today_member(&device_b, &subject.id, &today_text, &recurring.id),
        "重复规则命中的事项应默认为今日成员"
    );
    assert_eq!(push_all(&device_b).conflicts, 0);
    pull_all(&device_a);
    let recurring_plan_version: i64 = device_a
        .open()
        .unwrap()
        .query_row(
            "SELECT version FROM task_day_plans WHERE task_id=?1 AND work_date=?2",
            rusqlite::params![recurring.id, today_text],
            |row| row.get(0),
        )
        .unwrap();
    let recurring_excluded = today_plans::set_day_plan_for_cloud_test(
        &device_a,
        TaskDayPlanSetRequest {
            task_id: recurring.id.clone(),
            work_date: today_text.clone(),
            state: "excluded".to_string(),
            expected_version: Some(recurring_plan_version),
        },
        &state_a,
    )
    .unwrap();
    assert_eq!(recurring_excluded.version, recurring_plan_version + 1);
    assert_eq!(push_all(&device_a).conflicts, 0);
    pull_all(&device_b);
    assert_eq!(
        day_plan_state(&device_b, &recurring.id, &today_text).as_deref(),
        Some("excluded")
    );
    assert!(
        !is_today_member(&device_b, &subject.id, &today_text, &recurring.id),
        "显式移出应覆盖重复规则命中"
    );
    let archived_recurring_instance_id = recurring_excluded
        .instance_task_id
        .clone()
        .expect("移出映射应保留归档实例 ID");
    for _ in 0..2 {
        let _ = today_plans::snapshot(
            &device_b,
            TaskDayPlanSnapshotRequest {
                subject_id: subject.id.clone(),
                work_date: today_text.clone(),
            },
        )
        .unwrap();
    }
    assert!(
        !is_today_member(&device_b, &subject.id, &today_text, &recurring.id),
        "移出后的重复事项不得在刷新时自动重建"
    );
    let recurring_reincluded = today_plans::set_day_plan_for_cloud_test(
        &device_b,
        TaskDayPlanSetRequest {
            task_id: recurring.id.clone(),
            work_date: today_text.clone(),
            state: "included".to_string(),
            expected_version: Some(recurring_excluded.version),
        },
        &state_b,
    )
    .unwrap();
    let rejoined_instance_id = recurring_reincluded
        .instance_task_id
        .clone()
        .expect("再次加入应创建实例");
    assert_ne!(rejoined_instance_id, archived_recurring_instance_id);
    assert_eq!(push_all(&device_b).conflicts, 0);
    pull_all(&device_a);
    assert_eq!(
        today_instance(&device_a, &subject.id, &today_text, &recurring.id).id,
        rejoined_instance_id
    );
    assert!(
        is_today_member(&device_a, &subject.id, &tomorrow_text, &recurring.id),
        "移出今日不应影响其他日期"
    );

    // 同一成员在两端并发修改：后写入的云端版本获胜，落败方选择使用云端后收敛。
    let concurrent_first = today_plans::set_day_plan_for_cloud_test(
        &device_a,
        TaskDayPlanSetRequest {
            task_id: concurrent.id.clone(),
            work_date: today_text.clone(),
            state: "included".to_string(),
            expected_version: None,
        },
        &state_a,
    )
    .unwrap();
    assert_eq!(concurrent_first.version, 1);
    assert_eq!(push_all(&device_a).conflicts, 0);
    pull_all(&device_b);
    assert_eq!(
        day_plan_state(&device_b, &concurrent.id, &today_text).as_deref(),
        Some("included")
    );
    let concurrent_second = today_plans::set_day_plan_for_cloud_test(
        &device_a,
        TaskDayPlanSetRequest {
            task_id: concurrent.id.clone(),
            work_date: today_text.clone(),
            state: "excluded".to_string(),
            expected_version: Some(concurrent_first.version),
        },
        &state_a,
    )
    .unwrap();
    assert_eq!(concurrent_second.version, 2);
    assert_eq!(push_all(&device_a).conflicts, 0);
    today_plans::set_day_plan_for_cloud_test(
        &device_b,
        TaskDayPlanSetRequest {
            task_id: concurrent.id.clone(),
            work_date: today_text.clone(),
            state: "excluded".to_string(),
            expected_version: Some(concurrent_first.version),
        },
        &state_b,
    )
    .unwrap();
    let plan_push = push_all(&device_b);
    assert_eq!(plan_push.conflicts, 1, "过期版本写入应产生今日计划冲突");
    let instance_conflict = cloud_sync::list_conflicts(&device_b)
        .unwrap()
        .into_iter()
        .find(|item| item.entity_type == "task")
        .expect("今日实例冲突没有进入冲突列表");
    cloud_sync::resolve_conflict(
        &device_b,
        CloudConflictResolveRequest {
            operation_id: instance_conflict.operation_id,
            strategy: "use_cloud".to_string(),
        },
    )
    .unwrap();
    assert_eq!(push_all(&device_b).conflicts, 1);
    let plan_conflict = cloud_sync::list_conflicts(&device_b)
        .unwrap()
        .into_iter()
        .find(|item| item.entity_type == "task_day_plan")
        .expect("今日计划映射冲突没有进入冲突列表");
    cloud_sync::resolve_conflict(
        &device_b,
        CloudConflictResolveRequest {
            operation_id: plan_conflict.operation_id,
            strategy: "use_cloud".to_string(),
        },
    )
    .unwrap();
    cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            after_change_seq: Some(0),
            limit: None,
        },
    )
    .unwrap();
    assert_eq!(
        day_plan_state(&device_b, &concurrent.id, &today_text).as_deref(),
        Some("excluded")
    );
    assert!(!is_today_member(
        &device_b,
        &subject.id,
        &today_text,
        &concurrent.id
    ));

    // 来源预计同步后，设备 B 的懒物化可能已创建映射；使用最新版本显式移出。
    tasks::set_task_daily_estimate_for_cloud_test(
        &device_a,
        TaskDailyEstimateSetRequest {
            task_id: mixed.id.clone(),
            work_date: today_text.clone(),
            estimate_minutes: Some(35),
        },
        &state_a,
    )
    .unwrap();
    assert_eq!(push_all(&device_a).conflicts, 0);
    pull_all(&device_b);
    let mixed_plan_version = device_b
        .open()
        .unwrap()
        .query_row(
            "SELECT version FROM task_day_plans WHERE task_id=?1 AND work_date=?2",
            rusqlite::params![mixed.id, today_text],
            |row| row.get(0),
        )
        .unwrap_or_default();
    today_plans::set_day_plan_for_cloud_test(
        &device_b,
        TaskDayPlanSetRequest {
            task_id: mixed.id.clone(),
            work_date: today_text.clone(),
            state: "excluded".to_string(),
            expected_version: (mixed_plan_version > 0).then_some(mixed_plan_version),
        },
        &state_b,
    )
    .unwrap();
    let mixed_push = push_all(&device_b);
    assert_eq!((mixed_push.pending, mixed_push.conflicts), (0, 0));
    pull_all(&device_a);
    pull_all(&device_b);
    for database in [&device_a, &device_b] {
        assert_eq!(
            daily_estimate(database, &mixed.id, &today_text),
            None,
            "移出今日应清除并同步清除当日预计"
        );
        assert_eq!(
            day_plan_state(database, &mixed.id, &today_text).as_deref(),
            Some("excluded")
        );
        assert!(!is_today_member(
            database,
            &subject.id,
            &today_text,
            &mixed.id
        ));
    }

    // 暂时离线时成员操作保留在队列中，恢复后重试并最终收敛。
    today_plans::set_day_plan_for_cloud_test(
        &device_a,
        TaskDayPlanSetRequest {
            task_id: retriable.id.clone(),
            work_date: today_text.clone(),
            state: "included".to_string(),
            expected_version: None,
        },
        &state_a,
    )
    .unwrap();
    cloud.fail_next_apply_once();
    let offline = cloud_sync::push_outbox(&device_a).unwrap();
    assert_eq!(
        (offline.pushed, offline.pending, offline.conflicts),
        (0, 2, 0)
    );
    assert_eq!(day_plan_state(&device_b, &retriable.id, &today_text), None);
    thread::sleep(Duration::from_millis(2_100));
    assert_eq!(push_all(&device_a).conflicts, 0);
    pull_all(&device_b);
    assert_eq!(
        day_plan_state(&device_b, &retriable.id, &today_text).as_deref(),
        Some("included")
    );

    // 开始计时不改变今日成员，且两端只保留一个进行中的计时。
    cloud_sync::legacy_cloud_timer_start_local_first(
        &device_b,
        CloudTimerStartRequest {
            task_id: Some(manual.id.clone()),
            note: None,
            operation_id: Uuid::now_v7().to_string(),
        },
    )
    .unwrap();
    let timer_push = push_all(&device_b);
    assert_eq!((timer_push.pending, timer_push.conflicts), (0, 0));
    pull_all(&device_a);
    for database in [&device_a, &device_b] {
        assert_eq!(running_entry_count(database, &today_text), 1);
        assert!(is_today_member(
            database,
            &subject.id,
            &today_text,
            &manual.id
        ));
        assert!(!is_today_member(
            database,
            &subject.id,
            &today_text,
            &concurrent.id
        ));
    }

    // 新设备从完整快照恢复后，保留今日实例、来源映射和独立实际用时。
    let device_c = Database::initialize_at(directory.path().join("today-plan-c.sqlite3")).unwrap();
    configure_isolated_planning_device(
        &device_c,
        &cloud.base_url,
        &workspace_id,
        "today-plan-user-c",
    );
    cloud_sync::pull_snapshot(&device_c).unwrap();
    let manual_instance_c = today_instance(&device_c, &subject.id, &today_text, &manual.id);
    assert_eq!(manual_instance_c.id, renamed_instance.id);
    assert_eq!(manual_instance_c.title, "设备 B 今日独立标题");
    assert_eq!(
        day_plan_state(&device_c, &manual.id, &today_text).as_deref(),
        Some("included")
    );
    let local_workspace_c: String = device_c
        .open()
        .unwrap()
        .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        actual_time_adjustments::direct_actual_minutes(
            &device_c.open().unwrap(),
            &local_workspace_c,
            &manual_instance_c.id,
            Some(&today_text),
        )
        .unwrap(),
        25
    );
}

#[test]
fn isolated_automatic_snapshot_settings_are_shared_across_devices() {
    let directory = tempfile::tempdir().unwrap();
    let device_a = Database::initialize_at(directory.path().join("snapshot-a.sqlite3")).unwrap();
    let device_b = Database::initialize_at(directory.path().join("snapshot-b.sqlite3")).unwrap();
    let cloud = PlanningCloudServer::start();
    let workspace_id = Uuid::now_v7().to_string();
    configure_isolated_planning_device(
        &device_a,
        &cloud.base_url,
        &workspace_id,
        "snapshot-user-a",
    );
    configure_isolated_planning_device(
        &device_b,
        &cloud.base_url,
        &workspace_id,
        "snapshot-user-b",
    );

    let initial = supabase::workspace_snapshot_settings_get(&device_a).unwrap();
    assert!(initial.automatic_snapshot_enabled);
    assert_eq!(initial.automatic_snapshot_time, "03:00");
    assert_eq!(initial.automatic_snapshot_timezone, "Asia/Shanghai");
    assert_eq!(initial.snapshot_retention_days, 30);
    assert!(initial.scheduler_active);
    assert!(initial.next_automatic_snapshot_at.is_some());
    assert!(initial.last_automatic_snapshot_at.is_none());

    supabase::workspace_snapshot_settings_update(
        &device_a,
        CloudWorkspaceSnapshotSettingsUpdateRequest {
            automatic_snapshot_enabled: true,
            automatic_snapshot_time: "21:30".to_string(),
            snapshot_retention_days: 7,
        },
    )
    .unwrap();
    let shared = supabase::workspace_snapshot_settings_get(&device_b).unwrap();
    assert_eq!(shared.automatic_snapshot_time, "21:30");
    assert_eq!(shared.snapshot_retention_days, 7);

    supabase::workspace_snapshot_settings_update(
        &device_b,
        CloudWorkspaceSnapshotSettingsUpdateRequest {
            automatic_snapshot_enabled: false,
            automatic_snapshot_time: "21:30".to_string(),
            snapshot_retention_days: 7,
        },
    )
    .unwrap();
    assert!(
        !supabase::workspace_snapshot_settings_get(&device_a)
            .unwrap()
            .automatic_snapshot_enabled
    );

    assert!(supabase::workspace_snapshot_settings_update(
        &device_a,
        CloudWorkspaceSnapshotSettingsUpdateRequest {
            automatic_snapshot_enabled: true,
            automatic_snapshot_time: "24:00".to_string(),
            snapshot_retention_days: 7,
        },
    )
    .is_err());
    assert!(supabase::workspace_snapshot_settings_update(
        &device_a,
        CloudWorkspaceSnapshotSettingsUpdateRequest {
            automatic_snapshot_enabled: true,
            automatic_snapshot_time: "03:00".to_string(),
            snapshot_retention_days: 0,
        },
    )
    .is_err());
}

#[test]
fn isolated_sync_round_never_requests_automatic_snapshots() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::initialize_at(directory.path().join("snapshot-sync.sqlite3")).unwrap();
    let cloud = PlanningCloudServer::start();
    let workspace_id = Uuid::now_v7().to_string();
    configure_isolated_planning_device(
        &database,
        &cloud.base_url,
        &workspace_id,
        "snapshot-sync-user",
    );

    let source = cloud_sync::run_sync_coordinator_round_for_test(&database).unwrap();
    assert_ne!(source, "offline");
    let paths = cloud.requested_rpc_paths();
    assert!(
        !paths.is_empty(),
        "the sync round must reach the cloud before this assertion means anything"
    );
    assert!(
        paths
            .iter()
            .all(|path| !path.to_lowercase().contains("automatic")),
        "a sync round must never schedule automatic snapshots: {paths:?}"
    );
}
#[test]
fn isolated_operation_history_uses_fifty_row_cursor_pages_without_duplicates() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::initialize_at(directory.path().join("history-pages.sqlite3")).unwrap();
    let cloud = PlanningCloudServer::start();
    let workspace_id = Uuid::now_v7().to_string();
    configure_isolated_planning_device(&database, &cloud.base_url, &workspace_id, "history-user");
    cloud.state.lock().unwrap().operation_history_item_count = 75;
    let first = supabase::operation_history_list(
        &database,
        CloudOperationHistoryListRequest {
            limit: None,
            cursor: None,
            from: None,
            to: None,
            device_id: None,
            source: None,
            business_type: None,
            query: None,
        },
    )
    .unwrap();
    assert_eq!(first.items.len(), 50);
    assert!(first.next_cursor.is_some());
    let second = supabase::operation_history_list(
        &database,
        CloudOperationHistoryListRequest {
            limit: None,
            cursor: first.next_cursor,
            from: None,
            to: None,
            device_id: None,
            source: None,
            business_type: None,
            query: None,
        },
    )
    .unwrap();
    assert_eq!(second.items.len(), 25);
    assert!(second.next_cursor.is_none());
    let first_ids = first
        .items
        .iter()
        .map(|item| item.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    assert!(second
        .items
        .iter()
        .all(|item| !first_ids.contains(item.id.as_str())));
}

#[test]
fn isolated_workspace_restore_rebuilds_clean_second_device() {
    let directory = tempfile::tempdir().unwrap();
    let device_a =
        Database::initialize_at(directory.path().join("restore-clean-a.sqlite3")).unwrap();
    let device_b =
        Database::initialize_at(directory.path().join("restore-clean-b.sqlite3")).unwrap();
    let cloud = PlanningCloudServer::start();
    let workspace_id = Uuid::now_v7().to_string();
    configure_isolated_planning_device(
        &device_a,
        &cloud.base_url,
        &workspace_id,
        "restore-clean-user-a",
    );
    configure_isolated_planning_device(
        &device_b,
        &cloud.base_url,
        &workspace_id,
        "restore-clean-user-b",
    );
    let subject = subjects::list_subjects(&device_a).unwrap().remove(0);
    let task = create_task(&device_a, &subject.id, "回滚前标题");
    cloud.seed(&device_a, &subject, &task);
    cloud_sync::pull_snapshot(&device_b).unwrap();

    let snapshot = supabase::workspace_snapshot_create(
        &device_a,
        CloudWorkspaceSnapshotCreateRequest {
            name: "回滚目标".to_string(),
            note: Some("双设备自动重建验收".to_string()),
            snapshot_type: Some("manual".to_string()),
            lazy: Some(false),
        },
    )
    .unwrap();
    let changed = update_task_title(&device_a, &task, "回滚后应被移除的标题");
    cloud_sync::enqueue_entity(
        &device_a,
        "task_update",
        "task",
        Some(&changed.id),
        Some(task.version),
        None,
    )
    .unwrap();
    assert_eq!(push_all(&device_a).pending, 0);
    pull_all(&device_b);
    assert_eq!(
        list_tasks(&device_b)
            .into_iter()
            .find(|item| item.id == task.id)
            .unwrap()
            .title,
        "回滚后应被移除的标题"
    );
    settle_local_restore_gates(&device_a);

    let prepared = supabase::workspace_restore_prepare(
        &device_a,
        CloudWorkspaceRestorePrepareRequest {
            snapshot_id: snapshot.id.clone(),
            create_safety_snapshot: true,
        },
    )
    .unwrap();
    assert!(device_a
        .open()
        .unwrap()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM unassigned_sessions WHERE state IN ('collecting','paused','awaiting_resolution'))",
            [],
            |row| row.get::<_, bool>(0),
        )
        .unwrap());
    let restored = supabase::workspace_restore_commit(
        &device_a,
        CloudWorkspaceRestoreCommitRequest {
            lock_token: prepared.lock_token,
            target_snapshot_id: prepared.target_snapshot_id,
            target_content_hash: prepared.target_content_hash,
            prepared_epoch: prepared.prepared_epoch,
        },
    )
    .unwrap();
    assert_eq!((restored.previous_epoch, restored.workspace_epoch), (1, 2));
    assert!(restored.local_cache_rebuilt);
    let restoring_status = cloud_sync::sync_status(&device_a).unwrap();
    assert_eq!(restoring_status.workspace_epoch, 2);
    assert!(!restoring_status.epoch_blocked);
    assert_eq!(restoring_status.last_change_seq, restored.reset_change_seq);
    assert_eq!(
        list_tasks(&device_a)
            .into_iter()
            .find(|item| item.id == task.id)
            .unwrap()
            .title,
        "回滚前标题"
    );

    let pulled = cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: None,
        },
    )
    .unwrap();
    let status = cloud_sync::sync_status(&device_b).unwrap();
    assert_eq!(status.workspace_epoch, 2);
    assert!(!status.epoch_blocked);
    assert_eq!(pulled.last_change_seq, restored.reset_change_seq);
    assert_eq!(
        list_tasks(&device_b)
            .into_iter()
            .find(|item| item.id == task.id)
            .unwrap()
            .title,
        "回滚前标题"
    );
    let (epoch, latest_seq, safety_snapshot_id, restore_logs) = cloud.restore_audit();
    assert_eq!(
        (epoch, latest_seq, restore_logs),
        (2, restored.reset_change_seq, 1)
    );
    assert_eq!(
        safety_snapshot_id.as_deref(),
        restored.safety_snapshot_id.as_deref()
    );
    assert!(cloud
        .state
        .lock()
        .unwrap()
        .snapshots
        .contains_key(restored.safety_snapshot_id.as_deref().unwrap()));
    let history = supabase::operation_history_list(
        &device_a,
        CloudOperationHistoryListRequest {
            limit: Some(20),
            cursor: None,
            from: None,
            to: None,
            device_id: None,
            source: Some("restore".to_string()),
            business_type: None,
            query: None,
        },
    )
    .unwrap();
    assert_eq!(history.items.len(), 1);
    assert_eq!(history.items[0].source, "restore");
    assert_eq!(history.items[0].business_type, "restore");
}

#[test]
fn isolated_workspace_restore_blocks_old_epoch_and_allows_rescue_then_discard() {
    let directory = tempfile::tempdir().unwrap();
    let device_a =
        Database::initialize_at(directory.path().join("restore-dirty-a.sqlite3")).unwrap();
    let device_b =
        Database::initialize_at(directory.path().join("restore-dirty-b.sqlite3")).unwrap();
    let cloud = PlanningCloudServer::start();
    let workspace_id = Uuid::now_v7().to_string();
    configure_isolated_planning_device(
        &device_a,
        &cloud.base_url,
        &workspace_id,
        "restore-dirty-user-a",
    );
    configure_isolated_planning_device(
        &device_b,
        &cloud.base_url,
        &workspace_id,
        "restore-dirty-user-b",
    );
    let subject = subjects::list_subjects(&device_a).unwrap().remove(0);
    let task = create_task(&device_a, &subject.id, "云端回滚目标标题");
    cloud.seed(&device_a, &subject, &task);
    cloud_sync::pull_snapshot(&device_b).unwrap();
    let snapshot_id = cloud.capture_snapshot();

    let offline = update_task_title(&device_b, &task, "设备 B 离线旧世代标题");
    cloud_sync::enqueue_entity_deferred(
        &device_b,
        "task_update",
        "task",
        Some(&offline.id),
        Some(task.version),
        None,
    )
    .unwrap();
    settle_local_restore_gates(&device_a);

    let prepared = supabase::workspace_restore_prepare(
        &device_a,
        CloudWorkspaceRestorePrepareRequest {
            snapshot_id: snapshot_id.clone(),
            create_safety_snapshot: false,
        },
    )
    .unwrap();
    let restored = supabase::workspace_restore_commit(
        &device_a,
        CloudWorkspaceRestoreCommitRequest {
            lock_token: prepared.lock_token,
            target_snapshot_id: snapshot_id,
            target_content_hash: prepared.target_content_hash,
            prepared_epoch: prepared.prepared_epoch,
        },
    )
    .unwrap();

    let push = cloud_sync::push_outbox(&device_b).unwrap();
    assert_eq!(push.pushed, 0);
    let blocked = cloud_sync::sync_status(&device_b).unwrap();
    assert!(
        blocked.epoch_blocked,
        "status={blocked:?} push={push:?} outbox={:?}",
        outbox_debug(&device_b)
    );
    assert_eq!(blocked.workspace_epoch, restored.workspace_epoch);
    let connection = device_b.open().unwrap();
    let old_outbox: (i64, i64) = connection
        .query_row(
            "SELECT COUNT(*),MIN(workspace_epoch) FROM sync_outbox WHERE workspace_id=?1",
            [&workspace_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(old_outbox, (1, 1));
    drop(connection);
    assert_eq!(
        cloud.state.lock().unwrap().tasks[&task.id]["title"],
        "云端回滚目标标题"
    );

    let rescue_path = directory.path().join("old-epoch-rescue.timegenie-backup");
    let rescue = data_protection::export_backup(
        &device_b,
        BackupExportRequest {
            path: rescue_path.display().to_string(),
            rescue: true,
        },
    )
    .unwrap();
    assert_eq!(rescue.backup_kind, "stale_epoch_rescue");
    assert!(rescue_path.exists());

    let reset = cloud_sync::discard_old_epoch_changes_and_redownload(&device_b).unwrap();
    assert_eq!(reset.workspace_epoch, restored.workspace_epoch);
    assert!(!reset.epoch_blocked);
    assert_eq!(reset.pending_operations, 0);
    assert_eq!(
        list_tasks(&device_b)
            .into_iter()
            .find(|item| item.id == task.id)
            .unwrap()
            .title,
        "云端回滚目标标题"
    );
    let safety_backups = std::fs::read_dir(directory.path().join("backups"))
        .unwrap()
        .filter_map(Result::ok)
        .count();
    assert_eq!(safety_backups, 1);
}

#[test]
fn isolated_timer_transition_offline_retry_converges_two_devices() {
    let directory = tempfile::tempdir().unwrap();
    let device_a = Database::initialize_at(directory.path().join("transition-a.sqlite3")).unwrap();
    let device_b = Database::initialize_at(directory.path().join("transition-b.sqlite3")).unwrap();
    let cloud = PlanningCloudServer::start();
    let workspace_id = Uuid::now_v7().to_string();
    let state_a = configure_isolated_planning_device(
        &device_a,
        &cloud.base_url,
        &workspace_id,
        "transition-user-a",
    );
    configure_isolated_planning_device(
        &device_b,
        &cloud.base_url,
        &workspace_id,
        "transition-user-b",
    );
    let subject = subjects::list_subjects(&device_a).unwrap().remove(0);
    let initial_task = create_task(&device_a, &subject.id, "原计时事项");
    cloud.seed(&device_a, &subject, &initial_task);
    cloud_sync::pull_snapshot(&device_b).unwrap();

    let operation_id = Uuid::now_v7().to_string();
    let local_result = time_tracking::transition_timer_for_cloud_test(
        &device_a,
        TimerTransitionRequest {
            client_request_id: operation_id.clone(),
            current_entry_id: None,
            current_entry_expected_version: None,
            task_id: None,
            reopen_completed_task_version: None,
            new_task: Some(TimerTransitionNewTaskRequest {
                subject_id: subject.id.clone(),
                parent_id: None,
                expected_parent_version: None,
                title: "离线新事项".to_string(),
                day_instance_date: None,
            }),
            note: None,
        },
        &state_a,
    )
    .unwrap();
    let created_task_id = local_result.created_task.as_ref().unwrap().id.clone();
    assert_eq!(
        cloud_sync::sync_status(&device_a)
            .unwrap()
            .pending_operations,
        1
    );
    assert_eq!(push_all(&device_a).conflicts, 0);
    cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: None,
        },
    )
    .unwrap();
    assert!(list_tasks(&device_b)
        .iter()
        .any(|task| task.id == created_task_id));
    assert!(today_entries(&device_b)
        .entries
        .iter()
        .any(|entry| entry.id == local_result.active_entry.id && entry.state == "running"));

    let stale_operation_id = Uuid::now_v7().to_string();
    let stale = time_tracking::transition_timer_for_cloud_test(
        &device_a,
        TimerTransitionRequest {
            client_request_id: stale_operation_id.clone(),
            current_entry_id: Some(local_result.active_entry.id.clone()),
            current_entry_expected_version: Some(local_result.active_entry.version),
            task_id: Some(initial_task.id),
            reopen_completed_task_version: None,
            new_task: None,
            note: None,
        },
        &state_a,
    )
    .unwrap();
    cloud
        .state
        .lock()
        .unwrap()
        .time_entries
        .get_mut(&local_result.active_entry.id)
        .unwrap()["version"] = json!(99);
    let pushed = push_all(&device_a);
    assert_eq!((pushed.pending, pushed.conflicts), (0, 0));
    let remote = cloud.state.lock().unwrap();
    assert!(!remote.time_entries.contains_key(&stale.active_entry.id));
    assert_eq!(
        remote.time_entries[&local_result.active_entry.id]["state"].as_str(),
        Some("running")
    );
    drop(remote);
    let local_entries = today_entries(&device_a).entries;
    assert!(!local_entries
        .iter()
        .any(|entry| entry.id == stale.active_entry.id));
    assert!(local_entries
        .iter()
        .any(|entry| { entry.id == local_result.active_entry.id && entry.state == "running" }));
}

#[test]
fn competing_first_recurring_timers_share_one_occurrence_and_follow_first_timer() {
    let directory = tempfile::tempdir().unwrap();
    let device_a =
        Database::initialize_at(directory.path().join("recurring-transition-a.sqlite3")).unwrap();
    let device_b =
        Database::initialize_at(directory.path().join("recurring-transition-b.sqlite3")).unwrap();
    let cloud = PlanningCloudServer::start();
    let workspace_id = Uuid::now_v7().to_string();
    let state_a = configure_isolated_planning_device(
        &device_a,
        &cloud.base_url,
        &workspace_id,
        "recurring-transition-user-a",
    );
    let state_b = configure_isolated_planning_device(
        &device_b,
        &cloud.base_url,
        &workspace_id,
        "recurring-transition-user-b",
    );
    let subject = subjects::list_subjects(&device_a).unwrap().remove(0);
    let task = create_task(&device_a, &subject.id, "双设备首次周期计时");
    cloud.seed(&device_a, &subject, &task);
    cloud_sync::pull_snapshot(&device_b).unwrap();
    let work_date = crate::work_calendar::current_work_date(
        &device_a.open().unwrap(),
        chrono::Utc::now().timestamp_millis(),
    )
    .unwrap();
    recurring::save_rule_for_cloud_test(
        &device_a,
        RecurrenceSaveRequest {
            task_id: task.id.clone(),
            task_expected_version: task.version,
            frequency: "daily".to_string(),
            weekdays_mask: None,
            effective_start: work_date.clone(),
            rule_expected_version: None,
        },
        &state_a,
    )
    .unwrap();
    assert_eq!(push_all(&device_a).conflicts, 0);
    pull_all(&device_b);

    let timer_a = time_tracking::transition_timer_for_cloud_test(
        &device_a,
        TimerTransitionRequest {
            client_request_id: Uuid::now_v7().to_string(),
            current_entry_id: None,
            current_entry_expected_version: None,
            task_id: Some(task.id.clone()),
            reopen_completed_task_version: None,
            new_task: None,
            note: None,
        },
        &state_a,
    )
    .unwrap();
    let timer_b = time_tracking::transition_timer_for_cloud_test(
        &device_b,
        TimerTransitionRequest {
            client_request_id: Uuid::now_v7().to_string(),
            current_entry_id: None,
            current_entry_expected_version: None,
            task_id: Some(task.id.clone()),
            reopen_completed_task_version: None,
            new_task: None,
            note: None,
        },
        &state_b,
    )
    .unwrap();
    assert_ne!(timer_a.active_entry.id, timer_b.active_entry.id);

    let first = push_all(&device_a);
    let follower = push_all(&device_b);
    assert_eq!((first.pending, first.conflicts), (0, 0));
    assert_eq!((follower.pending, follower.conflicts), (0, 0));
    assert!(cloud_sync::list_conflicts(&device_b).unwrap().is_empty());
    pull_all(&device_b);

    let entity_id = format!("{}|{}", task.id, work_date);
    let remote = cloud.state.lock().unwrap();
    assert_eq!(remote.occurrences.len(), 1);
    assert_eq!(remote.occurrences[&entity_id]["version"], 1);
    assert_eq!(
        remote
            .time_entries
            .values()
            .filter(|entry| matches!(entry["state"].as_str(), Some("running" | "paused")))
            .count(),
        1
    );
    drop(remote);
    let local_occurrence: (String, i64) = device_b
        .open()
        .unwrap()
        .query_row(
            "SELECT status,version FROM task_occurrences WHERE task_id=?1 AND occurrence_date=?2",
            rusqlite::params![task.id, work_date],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(local_occurrence, ("open".to_string(), 1));
    let active_b = today_entries(&device_b)
        .entries
        .into_iter()
        .find(|entry| matches!(entry.state.as_str(), "running" | "paused"))
        .unwrap();
    assert_eq!(active_b.id, timer_a.active_entry.id);
    assert_ne!(active_b.id, timer_b.active_entry.id);
}

#[test]
fn missing_remote_occurrence_conflict_auto_rebases_and_pushes_final_local_state() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::initialize_at(
        directory
            .path()
            .join("missing-occurrence-auto-rebase.sqlite3"),
    )
    .unwrap();
    let cloud = PlanningCloudServer::start();
    let workspace_id = Uuid::now_v7().to_string();
    configure_isolated_planning_device(
        &database,
        &cloud.base_url,
        &workspace_id,
        "missing-occurrence-user",
    );
    let subject = subjects::list_subjects(&database).unwrap().remove(0);
    let task = create_task(&database, &subject.id, "历史缺失周期轮次");
    cloud.seed(&database, &subject, &task);
    let work_date = crate::work_calendar::current_work_date(
        &database.open().unwrap(),
        chrono::Utc::now().timestamp_millis(),
    )
    .unwrap();
    let entity_id = format!("{}|{}", task.id, work_date);
    let operation_id = Uuid::now_v7().to_string();
    let local_workspace_id: String = database
        .open()
        .unwrap()
        .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
        .unwrap();
    let device_id: String = database
        .open()
        .unwrap()
        .query_row("SELECT id FROM devices LIMIT 1", [], |row| row.get(0))
        .unwrap();
    database.open().unwrap().execute(
        "INSERT INTO task_occurrences(workspace_id,task_id,occurrence_date,origin,status,completed_at,created_at,updated_at,version) VALUES(?1,?2,?3,'scheduled','open',NULL,1,3,3)",
        rusqlite::params![local_workspace_id,task.id,work_date],
    ).unwrap();
    let payload = json!({
        "task_id":task.id,
        "occurrence_date":work_date,
        "origin":"scheduled",
        "status":"open",
        "completed_at":null,
        "created_at":1,
        "updated_at":3,
        "version":3
    });
    database.open().unwrap().execute(
        "INSERT INTO sync_outbox(operation_id,workspace_id,device_id,operation_type,entity_type,entity_id,base_version,payload_json,payload_version,state,attempt_count,created_at) VALUES(?1,?2,?3,'task_set_completed','task_occurrence',?4,2,?5,3,'pending',0,1)",
        rusqlite::params![operation_id,workspace_id,device_id,entity_id,payload.to_string()],
    ).unwrap();

    let pushed = push_all(&database);
    assert_eq!((pushed.pending, pushed.conflicts), (0, 0));
    assert!(cloud_sync::list_conflicts(&database).unwrap().is_empty());
    let remote = cloud.state.lock().unwrap();
    assert_eq!(remote.occurrences[&entity_id]["status"], "open");
    assert_eq!(remote.occurrences[&entity_id]["version"], 1);
    drop(remote);
    assert_eq!(
        database
            .open()
            .unwrap()
            .query_row(
                "SELECT version FROM task_occurrences WHERE task_id=?1 AND occurrence_date=?2",
                rusqlite::params![task.id, work_date],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
}

#[test]
fn timer_write_succeeds_while_another_device_holds_background_lease() {
    let directory = tempfile::tempdir().unwrap();
    let device_a = Database::initialize_at(directory.path().join("lease-holder.sqlite3")).unwrap();
    let device_b = Database::initialize_at(directory.path().join("timer-writer.sqlite3")).unwrap();
    let cloud = PlanningCloudServer::start();
    let workspace_id = Uuid::now_v7().to_string();
    let state_a = configure_isolated_planning_device(
        &device_a,
        &cloud.base_url,
        &workspace_id,
        "lease-user-a",
    );
    configure_isolated_planning_device(&device_b, &cloud.base_url, &workspace_id, "lease-user-b");
    let subject = subjects::list_subjects(&device_a).unwrap().remove(0);
    let task = create_task(&device_a, &subject.id, "主动计时接管租约");
    cloud.seed(&device_a, &subject, &task);
    cloud_sync::pull_snapshot(&device_b).unwrap();

    let holder = cloud_sync::acquire_lease(&device_a).unwrap();
    assert_eq!(holder["acquired"], true);
    let timer = cloud_sync::legacy_cloud_timer_start_local_first(
        &device_b,
        CloudTimerStartRequest {
            task_id: Some(task.id),
            note: None,
            operation_id: Uuid::now_v7().to_string(),
        },
    )
    .unwrap();

    let started = push_all(&device_b);
    assert_eq!((started.pending, started.conflicts), (0, 0));
    assert_eq!(
        cloud
            .state
            .lock()
            .unwrap()
            .tracking_lease_holder_device_id
            .as_deref(),
        Some(state_a.device_id.as_str())
    );

    cloud_sync::legacy_cloud_timer_stop_local_first(
        &device_b,
        CloudTimerVersionRequest {
            entry_id: timer.id,
            expected_version: timer.version,
            operation_id: Uuid::now_v7().to_string(),
        },
    )
    .unwrap();
    let stopped = push_all(&device_b);
    assert_eq!((stopped.pending, stopped.conflicts), (0, 0));
    assert_eq!(
        cloud
            .state
            .lock()
            .unwrap()
            .tracking_lease_holder_device_id
            .as_deref(),
        Some(state_a.device_id.as_str())
    );
}

#[test]
fn competing_timer_stops_follow_first_cloud_result_without_conflict() {
    let directory = tempfile::tempdir().unwrap();
    let device_a = Database::initialize_at(directory.path().join("stop-a.sqlite3")).unwrap();
    let device_b = Database::initialize_at(directory.path().join("stop-b.sqlite3")).unwrap();
    let cloud = PlanningCloudServer::start();
    let workspace_id = Uuid::now_v7().to_string();
    configure_isolated_planning_device(&device_a, &cloud.base_url, &workspace_id, "stop-user-a");
    configure_isolated_planning_device(&device_b, &cloud.base_url, &workspace_id, "stop-user-b");
    let subject = subjects::list_subjects(&device_a).unwrap().remove(0);
    let task = create_task(&device_a, &subject.id, "竞争结束计时");
    cloud.seed(&device_a, &subject, &task);
    cloud_sync::pull_snapshot(&device_b).unwrap();

    let timer = cloud_sync::legacy_cloud_timer_start_local_first(
        &device_a,
        CloudTimerStartRequest {
            task_id: Some(task.id),
            note: None,
            operation_id: Uuid::now_v7().to_string(),
        },
    )
    .unwrap();
    let started = push_all(&device_a);
    assert_eq!((started.pending, started.conflicts), (0, 0));
    pull_all(&device_b);

    let stop_a = cloud_sync::legacy_cloud_timer_stop_local_first(
        &device_a,
        CloudTimerVersionRequest {
            entry_id: timer.id.clone(),
            expected_version: timer.version,
            operation_id: Uuid::now_v7().to_string(),
        },
    )
    .unwrap();
    thread::sleep(Duration::from_millis(5));
    let stop_b = cloud_sync::legacy_cloud_timer_stop_local_first(
        &device_b,
        CloudTimerVersionRequest {
            entry_id: timer.id.clone(),
            expected_version: timer.version,
            operation_id: Uuid::now_v7().to_string(),
        },
    )
    .unwrap();
    assert!(stop_b.ended_at >= stop_a.ended_at);

    let first = push_all(&device_a);
    let follower = push_all(&device_b);
    assert_eq!((first.pending, first.conflicts), (0, 0));
    assert_eq!((follower.pending, follower.conflicts), (0, 0));
    assert!(cloud_sync::list_conflicts(&device_b).unwrap().is_empty());
    let cloud_entry = cloud.state.lock().unwrap().time_entries[&timer.id].clone();
    let followed = today_entries(&device_b)
        .entries
        .into_iter()
        .find(|entry| entry.id == timer.id)
        .unwrap();
    assert_eq!(followed.state, "ended");
    assert_eq!(followed.ended_at, cloud_entry["ended_at"].as_i64());
    assert_eq!(
        followed.duration_seconds,
        cloud_entry["duration_seconds"].as_i64().unwrap()
    );
}

#[test]
fn isolated_completed_task_continue_converges_two_devices_atomically() {
    let directory = tempfile::tempdir().unwrap();
    let device_a = Database::initialize_at(directory.path().join("continue-a.sqlite3")).unwrap();
    let device_b = Database::initialize_at(directory.path().join("continue-b.sqlite3")).unwrap();
    let cloud = PlanningCloudServer::start();
    let workspace_id = Uuid::now_v7().to_string();
    let state_a = configure_isolated_planning_device(
        &device_a,
        &cloud.base_url,
        &workspace_id,
        "continue-user-a",
    );
    configure_isolated_planning_device(
        &device_b,
        &cloud.base_url,
        &workspace_id,
        "continue-user-b",
    );
    let subject = subjects::list_subjects(&device_a).unwrap().remove(0);
    let task = create_task(&device_a, &subject.id, "今天完成后继续");
    let now = chrono::Utc::now().timestamp_millis();
    device_a
        .open()
        .unwrap()
        .execute(
            "UPDATE tasks SET status='done',completed_at=?1,updated_at=?1,version=2 WHERE id=?2",
            rusqlite::params![now, task.id],
        )
        .unwrap();
    let completed_task = list_tasks(&device_a)
        .into_iter()
        .find(|candidate| candidate.id == task.id)
        .unwrap();
    cloud.seed(&device_a, &subject, &completed_task);
    cloud_sync::pull_snapshot(&device_b).unwrap();

    let result = time_tracking::transition_timer_for_cloud_test(
        &device_a,
        TimerTransitionRequest {
            client_request_id: Uuid::now_v7().to_string(),
            current_entry_id: None,
            current_entry_expected_version: None,
            task_id: Some(task.id.clone()),
            reopen_completed_task_version: Some(2),
            new_task: None,
            note: None,
        },
        &state_a,
    )
    .unwrap();
    assert_eq!(push_all(&device_a).conflicts, 0);
    pull_all(&device_b);

    let task_on_b = list_tasks(&device_b)
        .into_iter()
        .find(|candidate| candidate.id == task.id)
        .unwrap();
    assert_eq!(task_on_b.status, "open");
    assert_eq!(task_on_b.version, 3);
    let active_on_b = today_entries(&device_b)
        .entries
        .into_iter()
        .find(|entry| entry.id == result.active_entry.id)
        .unwrap();
    assert_eq!(active_on_b.state, "running");
    assert_eq!(
        active_on_b.default_task_id.as_deref(),
        Some(task.id.as_str())
    );
}

#[test]
fn isolated_named_report_templates_two_device_flow() {
    let directory = tempfile::tempdir().unwrap();
    let device_a = Database::initialize_at(directory.path().join("templates-a.sqlite3")).unwrap();
    let device_b = Database::initialize_at(directory.path().join("templates-b.sqlite3")).unwrap();
    let cloud = PlanningCloudServer::start();
    let workspace_id = Uuid::now_v7().to_string();
    configure_isolated_planning_device(
        &device_a,
        &cloud.base_url,
        &workspace_id,
        "templates-user-a",
    );
    configure_isolated_planning_device(
        &device_b,
        &cloud.base_url,
        &workspace_id,
        "templates-user-b",
    );
    let subject = subjects::list_subjects(&device_a).unwrap().remove(0);
    let task = create_task(&device_a, &subject.id, "模板同步事项");
    cloud.seed(&device_a, &subject, &task);
    cloud_sync::pull_snapshot(&device_b).unwrap();

    let customer = reports::create_template_for_cloud_test(
        &device_a,
        ReportTemplateCreateRequest {
            report_type: "daily".to_string(),
            name: "客户日报".to_string(),
            description: Some("全部主体均可选择".to_string()),
            content: "# 客户日报 {{日期}}\n\n{{今日事项}}".to_string(),
        },
    )
    .unwrap();
    assert_eq!(push_all(&device_a).conflicts, 0);
    pull_all(&device_b);
    let on_b = reports::list_templates(
        &device_b,
        ReportTemplateListRequest {
            report_type: Some("daily".to_string()),
        },
    )
    .unwrap();
    assert!(on_b.iter().any(|item| {
        item.id == customer.id
            && item.name == "客户日报"
            && item.description.as_deref() == Some("全部主体均可选择")
            && item.sort_order == customer.sort_order
    }));

    let chosen = reports::set_default_template_for_cloud_test(
        &device_a,
        ReportTemplateSetDefaultRequest {
            template_id: customer.id.clone(),
            expected_version: customer.version,
        },
    )
    .unwrap();
    assert_eq!(push_all(&device_a).conflicts, 0);
    {
        let state = cloud.state.lock().unwrap();
        let cloud_target = state.report_templates.get(&chosen.id).unwrap();
        assert_eq!(
            cloud_target["is_default"]
                .as_bool()
                .unwrap_or_else(|| cloud_target["is_default"].as_i64() == Some(1)),
            true,
            "云端模拟器未保存目标默认模板: {cloud_target}"
        );
    }
    pull_all(&device_b);
    let on_b = reports::list_templates(
        &device_b,
        ReportTemplateListRequest {
            report_type: Some("daily".to_string()),
        },
    )
    .unwrap();
    assert_eq!(on_b.iter().filter(|item| item.is_default).count(), 1);
    assert!(
        on_b.iter()
            .any(|item| item.id == chosen.id && item.is_default),
        "目标模板未成为默认: {:?}",
        on_b.iter()
            .map(|item| (&item.id, &item.name, item.is_default, item.version))
            .collect::<Vec<_>>()
    );

    let compact = reports::create_template_for_cloud_test(
        &device_a,
        ReportTemplateCreateRequest {
            report_type: "daily".to_string(),
            name: "精简日报".to_string(),
            description: None,
            content: "# {{日期}}\n\n{{总工时}}".to_string(),
        },
    )
    .unwrap();
    assert_eq!(push_all(&device_a).conflicts, 0);
    pull_all(&device_b);
    let before_race_a = reports::list_templates(
        &device_a,
        ReportTemplateListRequest {
            report_type: Some("daily".to_string()),
        },
    )
    .unwrap();
    let before_race_b = reports::list_templates(
        &device_b,
        ReportTemplateListRequest {
            report_type: Some("daily".to_string()),
        },
    )
    .unwrap();
    let customer_a = before_race_a
        .iter()
        .find(|item| item.id == chosen.id)
        .unwrap();
    let compact_b = before_race_b
        .iter()
        .find(|item| item.id == compact.id)
        .unwrap();
    reports::set_default_template_for_cloud_test(
        &device_a,
        ReportTemplateSetDefaultRequest {
            template_id: customer_a.id.clone(),
            expected_version: customer_a.version,
        },
    )
    .unwrap();
    reports::set_default_template_for_cloud_test(
        &device_b,
        ReportTemplateSetDefaultRequest {
            template_id: compact_b.id.clone(),
            expected_version: compact_b.version,
        },
    )
    .unwrap();
    assert_eq!(push_all(&device_a).conflicts, 0);
    assert_eq!(push_all(&device_b).conflicts, 0);
    pull_all(&device_a);
    pull_all(&device_b);
    let defaults = [&device_a, &device_b]
        .into_iter()
        .map(|database| {
            reports::list_templates(
                database,
                ReportTemplateListRequest {
                    report_type: Some("daily".to_string()),
                },
            )
            .unwrap()
            .into_iter()
            .filter(|item| item.is_default)
            .map(|item| item.id)
            .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(defaults[0], vec![compact.id.clone()]);
    assert_eq!(defaults[1], vec![compact.id.clone()]);

    let report = reports::create_report(
        &device_a,
        ReportCreateRequest {
            report_type: "daily".to_string(),
            reference_date: chrono::Local::now()
                .date_naive()
                .format("%Y-%m-%d")
                .to_string(),
            subject_id: subject.id.clone(),
            task_ids: vec![task.id.clone()],
            template_id: Some(chosen.id.clone()),
            client_request_id: Uuid::now_v7().to_string(),
        },
    )
    .unwrap();
    cloud_sync::enqueue_entity_deferred(
        &device_a,
        "report_create",
        "report",
        Some(&report.id),
        None,
        None,
    )
    .unwrap();
    assert_eq!(push_all(&device_a).conflicts, 0);
    pull_all(&device_b);
    let report_on_b = reports::list_reports(
        &device_b,
        ReportListRequest {
            report_type: None,
            subject_id: None,
            query: None,
        },
    )
    .unwrap()
    .reports
    .into_iter()
    .find(|item| item.id == report.id)
    .unwrap();
    assert_eq!(report_on_b.template_id.as_deref(), Some(chosen.id.as_str()));

    let chosen_before_delete = reports::list_templates(
        &device_a,
        ReportTemplateListRequest {
            report_type: Some("daily".to_string()),
        },
    )
    .unwrap()
    .into_iter()
    .find(|item| item.id == chosen.id)
    .unwrap();
    reports::set_default_template_for_cloud_test(
        &device_a,
        ReportTemplateSetDefaultRequest {
            template_id: chosen.id.clone(),
            expected_version: chosen_before_delete.version,
        },
    )
    .unwrap();
    assert_eq!(push_all(&device_a).conflicts, 0);
    pull_all(&device_b);

    let current_templates = reports::list_templates(
        &device_a,
        ReportTemplateListRequest {
            report_type: Some("daily".to_string()),
        },
    )
    .unwrap();
    let chosen_version_for_delete = current_templates
        .iter()
        .find(|item| item.id == chosen.id)
        .unwrap()
        .version;
    let replacement = current_templates
        .into_iter()
        .find(|item| item.id != chosen.id)
        .unwrap();
    reports::delete_template_for_cloud_test(
        &device_a,
        ReportTemplateDeleteRequest {
            template_id: chosen.id.clone(),
            replacement_template_id: Some(replacement.id.clone()),
            expected_version: chosen_version_for_delete,
        },
    )
    .unwrap();
    assert_eq!(push_all(&device_a).conflicts, 0);
    pull_all(&device_b);
    let active_on_b = reports::list_templates(
        &device_b,
        ReportTemplateListRequest {
            report_type: Some("daily".to_string()),
        },
    )
    .unwrap();
    assert_eq!(active_on_b.iter().filter(|item| item.is_default).count(), 1);
    assert!(active_on_b
        .iter()
        .any(|item| item.id == replacement.id && item.is_default));
    let archived = reports::get_template_by_id(
        &device_b,
        reports::ReportTemplateIdRequest {
            template_id: chosen.id.clone(),
        },
    )
    .unwrap();
    assert!(archived.deleted_at.is_some());

    let regenerated = reports::regenerate_report(
        &device_b,
        ReportVersionRequest {
            report_id: report_on_b.id,
            expected_version: report_on_b.version,
        },
    )
    .unwrap();
    assert_eq!(regenerated.template_id.as_deref(), Some(chosen.id.as_str()));
    assert!(regenerated.markdown.starts_with("# 客户日报"));
}

#[test]
fn isolated_calendar_day_two_device_candidates_converge_to_one_authoritative_slice() {
    let directory = tempfile::tempdir().unwrap();
    let device_a = Database::initialize_at(directory.path().join("calendar-a.sqlite3")).unwrap();
    let device_b = Database::initialize_at(directory.path().join("calendar-b.sqlite3")).unwrap();
    let cloud = PlanningCloudServer::start();
    let cloud_workspace_id = Uuid::now_v7().to_string();
    let state_a = configure_isolated_planning_device(
        &device_a,
        &cloud.base_url,
        &cloud_workspace_id,
        "calendar-user-a",
    );
    let state_b = configure_isolated_planning_device(
        &device_b,
        &cloud.base_url,
        &cloud_workspace_id,
        "calendar-user-b",
    );
    let chain_id = Uuid::now_v7().to_string();
    let previous_entry_id = Uuid::now_v7().to_string();
    let candidate_a = Uuid::now_v7().to_string();
    let candidate_b = Uuid::now_v7().to_string();
    assert_ne!(candidate_a, candidate_b);

    for (database, candidate_id, state) in [
        (&device_a, candidate_a.as_str(), &state_a),
        (&device_b, candidate_b.as_str(), &state_b),
    ] {
        let connection = database.open().unwrap();
        let local_workspace_id: String = connection
            .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        connection
            .execute(
                "INSERT INTO time_entries(
               id,workspace_id,work_date,kind,source_type,state,label_snapshot,
               started_at,ended_at,duration_seconds,created_at,updated_at,version,
               timer_chain_id,last_continuous_at
             ) VALUES (?1,?2,'2026-10-05','work','timer','ended','跨日前一切片',
               1000,31000,30,1000,31000,1,?3,31000)",
                rusqlite::params![previous_entry_id, local_workspace_id, chain_id],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO time_entries(
               id,workspace_id,work_date,kind,source_type,state,label_snapshot,
               started_at,ended_at,duration_seconds,created_at,updated_at,version,
               timer_chain_id,previous_entry_id,split_boundary_at,last_continuous_at
             ) VALUES (?1,?2,'2026-10-06','work','timer','ended','跨日当天候选',
               31000,91000,60,31000,91000,1,?3,?4,31000,91000)",
                rusqlite::params![
                    candidate_id,
                    local_workspace_id,
                    chain_id,
                    previous_entry_id
                ],
            )
            .unwrap();
        drop(connection);
        cloud_sync::enqueue_entity_deferred(
            database,
            "timer_rollover_create",
            "time_entry",
            Some(candidate_id),
            None,
            None,
        )
        .unwrap();
        let pending = cloud_sync::sync_status(database).unwrap();
        assert_eq!(pending.pending_operations, 1);
        assert_eq!(pending.conflict_count, 0);
        assert_eq!(state.workspace_id, cloud_workspace_id);
    }

    let pushed_a = push_all(&device_a);
    assert_eq!((pushed_a.pending, pushed_a.conflicts), (0, 0));
    cloud.expire_tracking_lease();
    let pushed_b = push_all(&device_b);
    assert_eq!((pushed_b.pending, pushed_b.conflicts), (0, 0));

    let cloud_timer_ids = {
        let state = cloud.state.lock().unwrap();
        state
            .time_entries
            .values()
            .filter(|entry| {
                entry["timer_chain_id"].as_str() == Some(chain_id.as_str())
                    && entry["work_date"].as_str() == Some("2026-10-06")
            })
            .filter_map(|entry| entry["id"].as_str().map(str::to_string))
            .collect::<Vec<_>>()
    };
    assert_eq!(cloud_timer_ids, vec![candidate_a.clone()]);

    for database in [&device_a, &device_b] {
        let connection = database.open().unwrap();
        let slices = connection
            .prepare(
                "SELECT id,work_date,duration_seconds FROM time_entries
                 WHERE timer_chain_id=?1 ORDER BY work_date",
            )
            .unwrap()
            .query_map([&chain_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            slices,
            vec![
                (previous_entry_id.clone(), "2026-10-05".to_string(), 30),
                (candidate_a.clone(), "2026-10-06".to_string(), 60),
            ]
        );
        let status = cloud_sync::sync_status(database).unwrap();
        assert_eq!(status.pending_operations, 0);
        assert_eq!(status.conflict_count, 0);
    }
}

#[test]
fn isolated_unassigned_resolution_stops_the_old_clock_on_both_devices() {
    let directory = tempfile::tempdir().unwrap();
    let device_a = Database::initialize_at(directory.path().join("unassigned-a.sqlite3")).unwrap();
    let device_b = Database::initialize_at(directory.path().join("unassigned-b.sqlite3")).unwrap();
    let cloud = PlanningCloudServer::start();
    let cloud_workspace_id = Uuid::now_v7().to_string();
    configure_isolated_planning_device(
        &device_a,
        &cloud.base_url,
        &cloud_workspace_id,
        "unassigned-user-a",
    );
    configure_isolated_planning_device(
        &device_b,
        &cloud.base_url,
        &cloud_workspace_id,
        "unassigned-user-b",
    );

    let local_a = unassigned::get_state(&device_a).unwrap().unwrap();
    let started_at = chrono::Utc::now().timestamp_millis() - 120_000;
    let active_payload = json!({
        "id": local_a.session_id,
        "work_date": local_a.work_date,
        "state": "collecting",
        "threshold_seconds": 300,
        "duration_seconds": 0,
        "first_started_at": started_at,
        "last_ended_at": null,
        "prompted_at": null,
        "resolution_type": null,
        "generated_entry_id": null,
        "resolved_at": null,
        "created_at": started_at,
        "updated_at": started_at,
        "version": local_a.version,
        "last_continuous_at": started_at,
        "segments": [{
            "id": Uuid::now_v7().to_string(),
            "sequence_no": 1,
            "started_at": started_at,
            "ended_at": null,
            "duration_seconds": 0
        }]
    });
    for database in [&device_a, &device_b] {
        let connection = database.open().unwrap();
        connection
            .execute("DELETE FROM unassigned_segments", [])
            .unwrap();
        connection
            .execute("DELETE FROM unassigned_sessions", [])
            .unwrap();
        drop(connection);
        cloud_sync::apply_remote_unassigned_session_for_test(database, &active_payload).unwrap();
        let state = unassigned::get_state(database).unwrap().unwrap();
        assert_eq!(state.session_id, active_payload["id"].as_str().unwrap());
    }

    let resolved_at = chrono::Utc::now().timestamp_millis();
    let resolved_payload = json!({
        "id": active_payload["id"],
        "work_date": active_payload["work_date"],
        "state": "discarded",
        "threshold_seconds": 300,
        "duration_seconds": 120,
        "first_started_at": started_at,
        "last_ended_at": resolved_at,
        "prompted_at": null,
        "resolution_type": "discard",
        "generated_entry_id": null,
        "resolved_at": resolved_at,
        "created_at": started_at,
        "updated_at": resolved_at,
        "version": local_a.version + 1,
        "last_continuous_at": resolved_at,
        "segments": [{
            "id": active_payload["segments"][0]["id"],
            "sequence_no": 1,
            "started_at": started_at,
            "ended_at": resolved_at,
            "duration_seconds": 120
        }]
    });
    for database in [&device_a, &device_b] {
        cloud_sync::apply_remote_unassigned_session_for_test(database, &resolved_payload).unwrap();
        let connection = database.open().unwrap();
        let old: (String, i64, i64) = connection.query_row(
            "SELECT state,duration_seconds,(SELECT COUNT(*) FROM unassigned_segments WHERE session_id=?1 AND ended_at IS NULL) FROM unassigned_sessions WHERE id=?1",
            [resolved_payload["id"].as_str().unwrap()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).unwrap();
        assert_eq!(old, ("discarded".to_string(), 120, 0));
        let next: (String, i64, String) = connection.query_row(
            "SELECT id,first_started_at,predecessor_session_id FROM unassigned_sessions WHERE predecessor_session_id=?1 AND state IN ('collecting','awaiting_resolution')",
            [resolved_payload["id"].as_str().unwrap()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).unwrap();
        assert_ne!(next.0, resolved_payload["id"].as_str().unwrap());
        assert_eq!(next.1, resolved_at);
        assert_eq!(next.2, resolved_payload["id"].as_str().unwrap());
        drop(connection);
        let current = unassigned::get_state(database).unwrap().unwrap();
        assert_eq!(current.session_id, next.0);
        let status = cloud_sync::sync_status(database).unwrap();
        assert_eq!(status.pending_operations, 0);
        assert_eq!(status.conflict_count, 0);
    }
}

#[test]
fn slow_cloud_upload_does_not_block_local_sqlite_reads_or_writes() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::initialize_at(directory.path().join("slow-cloud.sqlite3")).unwrap();
    let cloud = PlanningCloudServer::start();
    let workspace_id = Uuid::now_v7().to_string();
    configure_isolated_planning_device(
        &database,
        &cloud.base_url,
        &workspace_id,
        "slow-cloud-user",
    );
    let subject = subjects::list_subjects(&database).unwrap().remove(0);
    let task = create_task(&database, &subject.id, "慢网上传事项");
    cloud.seed(&database, &subject, &task);
    let updated = update_task_title(&database, &task, "等待慢网提交");
    cloud_sync::enqueue_entity_deferred(
        &database,
        "task_update",
        "task",
        Some(&updated.id),
        Some(task.version),
        None,
    )
    .unwrap();

    cloud.delay_next_apply_patch(Duration::from_millis(700));
    let upload_database = database.clone();
    let upload = thread::spawn(move || cloud_sync::push_outbox(&upload_database).unwrap());
    cloud.wait_for_delayed_apply_patch();

    let started = std::time::Instant::now();
    let local_task = create_task(&database, &subject.id, "慢网期间仍可本地写入");
    assert!(list_tasks(&database)
        .iter()
        .any(|candidate| candidate.id == local_task.id));
    assert!(
        started.elapsed() < Duration::from_millis(250),
        "云端请求等待期间，本地 SQLite 操作耗时 {:?}",
        started.elapsed()
    );
    assert_eq!(upload.join().unwrap().conflicts, 0);
}

#[test]
#[ignore = "需要专用 Supabase 测试账号和显式的工作空间清理授权"]
fn real_supabase_unassigned_two_device_flow() {
    use chrono::TimeZone;

    let config = TestConfig::from_environment();
    let directory = tempfile::tempdir().unwrap();
    let device_a =
        Database::initialize_at(directory.path().join("unassigned-device-a.sqlite3")).unwrap();
    let device_b =
        Database::initialize_at(directory.path().join("unassigned-device-b.sqlite3")).unwrap();

    configure_and_sign_in(&device_a, &config);
    let _cleanup = CleanupGuard {
        database: device_a.clone(),
    };
    reset_owned_workspaces(&device_a);
    let workspace = supabase::workspace_bootstrap(&device_a).unwrap();
    register_device(&device_a, "未归属 E2E 设备 A");
    migrate(&device_a, "local_to_cloud");

    configure_and_sign_in(&device_b, &config);
    let workspace_b = supabase::workspace_bootstrap(&device_b).unwrap();
    assert_eq!(workspace_b.id, workspace.id, "两台设备没有进入同一工作空间");
    register_device(&device_b, "未归属 E2E 设备 B");
    migrate(&device_b, "cloud_to_local_snapshot");

    // Natural growth is derived locally from one shared anchor. It must not
    // mutate versions or enqueue per-second writes.
    let initial_a = unassigned::get_state(&device_a).unwrap().unwrap();
    let initial_b = unassigned::get_state(&device_b).unwrap().unwrap();
    assert_eq!(initial_a.session_id, initial_b.session_id);
    assert_eq!(initial_a.first_started_at, initial_b.first_started_at);
    let initial_version = initial_a.version;
    let initial_elapsed = initial_a.elapsed_seconds;
    thread::sleep(Duration::from_millis(1_100));
    let grown_a = unassigned::get_state(&device_a).unwrap().unwrap();
    assert_eq!(grown_a.session_id, initial_a.session_id);
    assert_eq!(grown_a.version, initial_version);
    assert!(grown_a.elapsed_seconds > initial_elapsed);
    let status = cloud_sync::sync_status(&device_a).unwrap();
    assert_eq!((status.pending_operations, status.conflict_count), (0, 0));
    report_unassigned_e2e(
        "natural-growth",
        Some(&grown_a),
        status.pending_operations,
        "shared-anchor-unchanged",
    );

    let subject_id = subjects::list_subjects(&device_a).unwrap()[0].id.clone();
    let task = create_task(&device_a, &subject_id, "未归属双设备验收事项");
    cloud_sync::enqueue_entity(&device_a, "task_create", "task", Some(&task.id), None, None)
        .unwrap();
    pull_all(&device_b);

    // Starting a task clears the current anchor without creating a hidden
    // final time entry. The other device observes the same terminal result.
    let cleared_session_id = grown_a.session_id.clone();
    let timer = cloud_sync::legacy_cloud_timer_start_local_first(
        &device_a,
        CloudTimerStartRequest {
            task_id: Some(task.id.clone()),
            note: Some("开始事项清零未归属".to_string()),
            operation_id: Uuid::now_v7().to_string(),
        },
    )
    .unwrap();
    assert!(unassigned::get_state(&device_a).unwrap().is_none());
    let queued_start = cloud_sync::sync_status(&device_a).unwrap();
    assert_eq!(queued_start.pending_operations, 1);
    push_all(&device_a);
    pull_all(&device_b);
    assert!(unassigned::get_state(&device_b).unwrap().is_none());
    for database in [&device_a, &device_b] {
        let terminal = unassigned_terminal_result(database, &cleared_session_id);
        assert_eq!(
            (terminal.0.as_str(), terminal.1.as_str()),
            ("discarded", "discard")
        );
        assert!(terminal.2.is_none());
        assert!(generated_entries_for_session(database, &cleared_session_id).is_empty());
    }
    report_unassigned_e2e(
        "timer-start-clears-anchor",
        None,
        cloud_sync::sync_status(&device_a)
            .unwrap()
            .pending_operations,
        "discarded-without-entry",
    );

    // Stop from the other device and verify the server-authoritative stop
    // boundary becomes the one shared anchor on both clients.
    let stopped = cloud_sync::legacy_cloud_timer_stop_local_first(
        &device_b,
        CloudTimerVersionRequest {
            entry_id: timer.id.clone(),
            expected_version: timer.version,
            operation_id: Uuid::now_v7().to_string(),
        },
    )
    .unwrap();
    push_all(&device_b);
    pull_all(&device_a);
    let stopped_anchor_a = unassigned::get_state(&device_a).unwrap().unwrap();
    let stopped_anchor_b = unassigned::get_state(&device_b).unwrap().unwrap();
    assert_eq!(stopped_anchor_a.session_id, stopped_anchor_b.session_id);
    assert_eq!(
        stopped_anchor_a.first_started_at,
        stopped_anchor_b.first_started_at
    );
    assert_eq!(stopped_anchor_a.first_started_at, stopped.ended_at.unwrap());
    report_unassigned_e2e(
        "timer-stop-restarts-anchor",
        Some(&stopped_anchor_a),
        cloud_sync::sync_status(&device_b)
            .unwrap()
            .pending_operations,
        "shared-stop-boundary",
    );

    // Work resolution. Move only the discrete anchor boundary backwards; no
    // per-second writes are produced while its displayed duration grows.
    let work_reset =
        reset_shared_unassigned_anchor(&device_a, chrono::Utc::now().timestamp_millis() - 120_000);
    cloud_sync::apply_remote_unassigned_session_for_test(&device_b, &work_reset["session"])
        .unwrap();
    let work_state = unassigned::get_state(&device_a).unwrap().unwrap();
    let work_result = unassigned::resolve_work_for_sync(
        &device_a,
        ResolveWorkRequest {
            session_id: work_state.session_id.clone(),
            expected_version: work_state.version,
            operation_id: Uuid::now_v7().to_string(),
            allocations: vec![UnassignedAllocationInput {
                task_id: task.id.clone(),
                minutes: work_state.required_minutes,
                complete_task: false,
                task_expected_version: None,
            }],
        },
    )
    .unwrap();
    assert_eq!(
        cloud_sync::sync_status(&device_a)
            .unwrap()
            .pending_operations,
        1
    );
    push_all(&device_a);
    pull_all(&device_b);
    let work_entry_id = work_result.generated_entry_id.unwrap();
    for database in [&device_a, &device_b] {
        assert_eq!(
            generated_entries_for_session(database, &work_state.session_id),
            vec![(work_entry_id.clone(), "work".to_string())]
        );
    }
    let after_work_a = unassigned::get_state(&device_a).unwrap().unwrap();
    let after_work_b = unassigned::get_state(&device_b).unwrap().unwrap();
    assert_eq!(after_work_a.session_id, after_work_b.session_id);
    report_unassigned_e2e(
        "resolve-work",
        Some(&after_work_a),
        cloud_sync::sync_status(&device_a)
            .unwrap()
            .pending_operations,
        &format!("entry={work_entry_id}"),
    );

    // Break resolution.
    let break_reset =
        reset_shared_unassigned_anchor(&device_b, chrono::Utc::now().timestamp_millis() - 60_000);
    cloud_sync::apply_remote_unassigned_session_for_test(&device_a, &break_reset["session"])
        .unwrap();
    let break_state = unassigned::get_state(&device_b).unwrap().unwrap();
    let break_result = unassigned::resolve_break_for_sync(
        &device_b,
        ResolveSessionRequest {
            session_id: break_state.session_id.clone(),
            expected_version: break_state.version,
            operation_id: Uuid::now_v7().to_string(),
        },
    )
    .unwrap();
    push_all(&device_b);
    pull_all(&device_a);
    let break_entry_id = break_result.generated_entry_id.unwrap();
    for database in [&device_a, &device_b] {
        assert_eq!(
            generated_entries_for_session(database, &break_state.session_id),
            vec![(break_entry_id.clone(), "break".to_string())]
        );
    }
    let after_break = unassigned::get_state(&device_a).unwrap().unwrap();
    report_unassigned_e2e(
        "resolve-break",
        Some(&after_break),
        cloud_sync::sync_status(&device_b)
            .unwrap()
            .pending_operations,
        &format!("entry={break_entry_id}"),
    );

    // Discard resolution is also one final invalid-time record and one next
    // anchor, not a resumable lifecycle.
    let discard_reset =
        reset_shared_unassigned_anchor(&device_a, chrono::Utc::now().timestamp_millis() - 60_000);
    cloud_sync::apply_remote_unassigned_session_for_test(&device_b, &discard_reset["session"])
        .unwrap();
    let discard_state = unassigned::get_state(&device_a).unwrap().unwrap();
    let discard_result = unassigned::discard_for_sync(
        &device_a,
        ResolveSessionRequest {
            session_id: discard_state.session_id.clone(),
            expected_version: discard_state.version,
            operation_id: Uuid::now_v7().to_string(),
        },
    )
    .unwrap();
    push_all(&device_a);
    pull_all(&device_b);
    let discard_entry_id = discard_result.generated_entry_id.unwrap();
    for database in [&device_a, &device_b] {
        assert_eq!(
            unassigned_terminal_result(database, &discard_state.session_id).1,
            "discard"
        );
        assert_eq!(
            generated_entries_for_session(database, &discard_state.session_id),
            vec![(discard_entry_id.clone(), "break".to_string())]
        );
    }
    let after_discard = unassigned::get_state(&device_b).unwrap().unwrap();
    report_unassigned_e2e(
        "resolve-discard",
        Some(&after_discard),
        cloud_sync::sync_status(&device_a)
            .unwrap()
            .pending_operations,
        &format!("entry={discard_entry_id}"),
    );

    // Both devices resolve the same version differently while offline. The
    // first accepted action wins, and the losing optimistic entry is removed.
    let race_reset =
        reset_shared_unassigned_anchor(&device_a, chrono::Utc::now().timestamp_millis() - 120_000);
    cloud_sync::apply_remote_unassigned_session_for_test(&device_b, &race_reset["session"])
        .unwrap();
    let race_state_a = unassigned::get_state(&device_a).unwrap().unwrap();
    let race_state_b = unassigned::get_state(&device_b).unwrap().unwrap();
    assert_eq!(race_state_a.session_id, race_state_b.session_id);
    assert_eq!(race_state_a.version, race_state_b.version);
    let winner = unassigned::resolve_work_for_sync(
        &device_a,
        ResolveWorkRequest {
            session_id: race_state_a.session_id.clone(),
            expected_version: race_state_a.version,
            operation_id: Uuid::now_v7().to_string(),
            allocations: vec![UnassignedAllocationInput {
                task_id: task.id.clone(),
                minutes: race_state_a.required_minutes,
                complete_task: false,
                task_expected_version: None,
            }],
        },
    )
    .unwrap();
    let losing = unassigned::discard_for_sync(
        &device_b,
        ResolveSessionRequest {
            session_id: race_state_b.session_id.clone(),
            expected_version: race_state_b.version,
            operation_id: Uuid::now_v7().to_string(),
        },
    )
    .unwrap();
    assert_ne!(winner.generated_entry_id, losing.generated_entry_id);
    push_all(&device_a);
    push_all(&device_b);
    pull_all(&device_a);
    pull_all(&device_b);
    push_all(&device_a);
    push_all(&device_b);
    pull_all(&device_a);
    pull_all(&device_b);
    let winner_entry_id = winner.generated_entry_id.unwrap();
    for database in [&device_a, &device_b] {
        let terminal = unassigned_terminal_result(database, &race_state_a.session_id);
        assert_eq!(
            (terminal.0.as_str(), terminal.1.as_str()),
            ("resolved", "work")
        );
        assert_eq!(terminal.2.as_deref(), Some(winner_entry_id.as_str()));
        assert_eq!(
            generated_entries_for_session(database, &race_state_a.session_id),
            vec![(winner_entry_id.clone(), "work".to_string())]
        );
        let status = cloud_sync::sync_status(database).unwrap();
        assert_eq!((status.pending_operations, status.conflict_count), (0, 0));
    }
    let race_anchor_a = unassigned::get_state(&device_a).unwrap().unwrap();
    let race_anchor_b = unassigned::get_state(&device_b).unwrap().unwrap();
    assert_eq!(race_anchor_a.session_id, race_anchor_b.session_id);
    report_unassigned_e2e(
        "concurrent-resolution",
        Some(&race_anchor_a),
        0,
        &format!("winner={winner_entry_id}"),
    );

    // Queue an action, make the endpoint unreachable, then restore it. The
    // pending item survives and converges after connectivity returns.
    let offline_reset =
        reset_shared_unassigned_anchor(&device_a, chrono::Utc::now().timestamp_millis() - 60_000);
    cloud_sync::apply_remote_unassigned_session_for_test(&device_b, &offline_reset["session"])
        .unwrap();
    let offline_state = unassigned::get_state(&device_a).unwrap().unwrap();
    let offline_result = unassigned::discard_for_sync(
        &device_a,
        ResolveSessionRequest {
            session_id: offline_state.session_id.clone(),
            expected_version: offline_state.version,
            operation_id: Uuid::now_v7().to_string(),
        },
    )
    .unwrap();
    device_a
        .open()
        .unwrap()
        .execute(
            "UPDATE device_settings SET value_json=?1 WHERE key='supabase_project_url'",
            [json!("http://127.0.0.1:9").to_string()],
        )
        .unwrap();
    assert!(cloud_sync::push_outbox(&device_a).is_err());
    assert_eq!(
        cloud_sync::sync_status(&device_a)
            .unwrap()
            .pending_operations,
        1
    );
    device_a
        .open()
        .unwrap()
        .execute(
            "UPDATE device_settings SET value_json=?1 WHERE key='supabase_project_url'",
            [json!(config.project_url).to_string()],
        )
        .unwrap();
    push_all(&device_a);
    pull_all(&device_b);
    let offline_entry_id = offline_result.generated_entry_id.unwrap();
    assert_eq!(
        generated_entries_for_session(&device_b, &offline_state.session_id),
        vec![(offline_entry_id.clone(), "break".to_string())]
    );
    report_unassigned_e2e(
        "offline-recovery",
        unassigned::get_state(&device_b).unwrap().as_ref(),
        cloud_sync::sync_status(&device_a)
            .unwrap()
            .pending_operations,
        &format!("entry={offline_entry_id}"),
    );

    // A stale local continuity heartbeat resets the shared anchor exactly once
    // after resume, without uploading heartbeat ticks.
    pull_all(&device_a);
    let before_resume = unassigned::get_state(&device_a).unwrap().unwrap();
    let stale_at = chrono::Utc::now().timestamp_millis() - 180_000;
    let workspace_id: String = device_a
        .open()
        .unwrap()
        .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
        .unwrap();
    device_a
        .open()
        .unwrap()
        .execute(
            "INSERT INTO tracking_runtime_state(workspace_id,last_heartbeat_at,updated_at)
             VALUES (?1,?2,?2) ON CONFLICT(workspace_id) DO UPDATE SET
             last_heartbeat_at=excluded.last_heartbeat_at,updated_at=excluded.updated_at",
            rusqlite::params![workspace_id, stale_at],
        )
        .unwrap();
    let resumed = cloud_sync::refresh_unassigned_state_for_cloud_mode(&device_a)
        .unwrap()
        .unwrap();
    assert_eq!(resumed.session_id, before_resume.session_id);
    assert_ne!(resumed.first_started_at, before_resume.first_started_at);
    assert!(resumed.version > before_resume.version);
    assert!(resumed.elapsed_seconds <= 1);
    let resume_pending = cloud_sync::sync_status(&device_a)
        .unwrap()
        .pending_operations;
    assert!(resume_pending > 0);
    let resume_operation_types = device_a
        .open()
        .unwrap()
        .prepare(
            "SELECT operation_type FROM sync_outbox
             WHERE state IN ('pending','sending','failed') ORDER BY created_at,rowid",
        )
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(resume_operation_types.iter().all(|operation| matches!(
        operation.as_str(),
        "unassigned_session_create" | "unassigned_anchor_reset"
    )));
    push_all(&device_a);
    let remote_resumed = remote_unassigned_state(&device_a);
    let pulled_a = pull_all(&device_a);
    let pulled_b = pull_all(&device_b);
    let resumed_a = unassigned::get_state(&device_a).unwrap().unwrap();
    let resumed_b = unassigned::get_state(&device_b).unwrap().unwrap();
    assert_eq!(resumed_a.session_id, resumed_b.session_id);
    assert_eq!(
        resumed_a.first_started_at,
        resumed_b.first_started_at,
        "休眠恢复未收敛: optimistic={} before={} remote={} a_version={} b_version={} a_seq={} b_seq={} a_changes={:?} b_changes={:?} a_outbox={:?} b_outbox={:?}",
        resumed.first_started_at,
        before_resume.first_started_at,
        remote_resumed,
        resumed_a.version,
        resumed_b.version,
        pulled_a.last_change_seq,
        pulled_b.last_change_seq,
        pulled_a.changes,
        pulled_b.changes,
        outbox_debug(&device_a),
        outbox_debug(&device_b)
    );
    assert_eq!(
        cloud_sync::sync_status(&device_a)
            .unwrap()
            .pending_operations,
        0
    );
    assert_eq!(
        cloud_sync::sync_status(&device_b)
            .unwrap()
            .pending_operations,
        0
    );
    report_unassigned_e2e(
        "sleep-resume-reset",
        Some(&resumed_a),
        cloud_sync::sync_status(&device_a)
            .unwrap()
            .pending_operations,
        "offline-gap-excluded",
    );

    // Seed the shared anchor just before today's workspace midnight. Lease
    // acquisition performs a real server tick; the old day closes and today's
    // one shared anchor starts exactly at midnight.
    let timezone: chrono_tz::Tz = workspace.timezone.parse().unwrap();
    let now = chrono::Utc::now().with_timezone(&timezone);
    let today_start = timezone
        .from_local_datetime(&now.date_naive().and_hms_opt(0, 0, 0).unwrap())
        .single()
        .unwrap()
        .timestamp_millis();
    let cross_day_reset = reset_shared_unassigned_anchor(&device_a, today_start - 30_000);
    cloud_sync::apply_remote_unassigned_session_for_test(&device_b, &cross_day_reset["session"])
        .unwrap();
    let old_cross_day_id = cross_day_reset["session"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let lease = cloud_sync::acquire_lease(&device_a).unwrap();
    pull_all(&device_a);
    pull_all(&device_b);
    let today_anchor_a = unassigned::get_state(&device_a).unwrap().unwrap();
    let today_anchor_b = unassigned::get_state(&device_b).unwrap().unwrap();
    assert_eq!(today_anchor_a.session_id, today_anchor_b.session_id);
    assert_eq!(today_anchor_a.first_started_at, today_start);
    for database in [&device_a, &device_b] {
        let connection = database.open().unwrap();
        let old_state: String = connection
            .query_row(
                "SELECT state FROM unassigned_sessions WHERE id=?1",
                [&old_cross_day_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(old_state, "discarded");
    }
    cloud_sync::release_lease(
        &device_a,
        TrackingLeaseRequest {
            lease_token: lease["leaseToken"].as_str().map(ToOwned::to_owned),
        },
    )
    .unwrap();
    report_unassigned_e2e(
        "calendar-rollover",
        Some(&today_anchor_a),
        cloud_sync::sync_status(&device_a)
            .unwrap()
            .pending_operations,
        &format!("previous={old_cross_day_id}:discarded"),
    );

    reset_owned_workspaces(&device_a);
    let _ = supabase::sign_out(&device_a);
    let _ = supabase::sign_out(&device_b);
}

#[test]
#[ignore = "需要专用 Supabase 测试账号和显式的工作空间清理授权"]
fn real_supabase_two_device_flow() {
    let config = TestConfig::from_environment();
    let directory = tempfile::tempdir().unwrap();
    let device_a = Database::initialize_at(directory.path().join("device-a.sqlite3")).unwrap();
    let device_b = Database::initialize_at(directory.path().join("device-b.sqlite3")).unwrap();

    configure_and_sign_in(&device_a, &config);
    supabase::clear_cached_session_for_test(&device_a);
    assert!(
        supabase::current_session(&device_a).is_ok(),
        "应用重启模拟后没有从系统凭据恢复 Supabase 会话"
    );
    let access_before_refresh = supabase::current_session(&device_a).unwrap().access_token;
    // GoTrue JWT claims use second precision. A refresh in the same second as
    // sign-in can legitimately reproduce the same signed token.
    thread::sleep(Duration::from_millis(1_100));
    supabase::force_cached_session_expiry_for_test(&device_a).unwrap();
    let access_after_refresh = supabase::current_session(&device_a).unwrap().access_token;
    assert_ne!(
        access_after_refresh, access_before_refresh,
        "access token 到期后没有自动刷新会话"
    );
    {
        let original_url = config.project_url.clone();
        device_a
            .open()
            .unwrap()
            .execute(
                "UPDATE device_settings SET value_json = ?1 WHERE key = 'supabase_project_url'",
                [json!("http://127.0.0.1:9").to_string()],
            )
            .unwrap();
        let offline = supabase::session_snapshot_for_database(&device_a).unwrap();
        assert_eq!(offline.status, "offline_saved", "断网时没有保留登录状态");
        device_a
            .open()
            .unwrap()
            .execute(
                "UPDATE device_settings SET value_json = ?1 WHERE key = 'supabase_project_url'",
                [json!(original_url).to_string()],
            )
            .unwrap();
    }
    let _cleanup = CleanupGuard {
        database: device_a.clone(),
    };
    reset_owned_workspaces(&device_a);
    let workspace = supabase::workspace_bootstrap(&device_a).unwrap();
    register_device(&device_a, "Supabase E2E 设备 A");
    migrate(&device_a, "local_to_cloud");

    configure_and_sign_in(&device_b, &config);
    let workspace_b = supabase::workspace_bootstrap(&device_b).unwrap();
    assert_eq!(workspace_b.id, workspace.id, "两台设备没有进入同一工作空间");
    register_device(&device_b, "Supabase E2E 设备 B");
    migrate(&device_b, "cloud_to_local_snapshot");

    let devices = supabase::list_devices(&device_a).unwrap();
    assert_eq!(devices.len(), 2, "云端没有注册两台独立设备");

    let subject_id = subjects::list_subjects(&device_a).unwrap()[0].id.clone();
    let realtime_config = realtime_test_config(&device_b);
    let realtime_workspace_id = workspace.id.clone();
    let (realtime_ready_tx, realtime_ready_rx) = std::sync::mpsc::sync_channel(0);
    let realtime_listener = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let mut socket =
                subscribe_to_workspace_changes(realtime_config, &realtime_workspace_id).await;
            realtime_ready_tx.send(()).unwrap();
            wait_for_postgres_change(&mut socket).await;
        });
    });
    realtime_ready_rx
        .recv_timeout(Duration::from_secs(20))
        .expect("Supabase Realtime 监听器未就绪");
    let synced_task = create_task(&device_a, &subject_id, "Supabase E2E Realtime 事项");
    cloud_sync::enqueue_entity(
        &device_a,
        "task_create",
        "task",
        Some(&synced_task.id),
        None,
        None,
    )
    .unwrap();
    realtime_listener
        .join()
        .expect("Supabase Realtime 监听线程异常退出");
    let pulled = cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: None,
        },
    )
    .unwrap();
    assert!(
        !pulled.changes.is_empty(),
        "设备 B 没有增量拉取到设备 A 的变更"
    );
    assert!(
        list_tasks(&device_b)
            .iter()
            .any(|task| task.id == synced_task.id),
        "设备 B 的 SQLite 缓存没有重建出新增事项"
    );

    let queued_task = create_task(&device_a, &subject_id, "Supabase E2E 离线队列事项");
    cloud_sync::enqueue_entity_deferred(
        &device_a,
        "task_create",
        "task",
        Some(&queued_task.id),
        None,
        Some(&Uuid::now_v7().to_string()),
    )
    .unwrap();
    let pending = cloud_sync::sync_status(&device_a).unwrap();
    assert_eq!(pending.pending_operations, 1, "离线操作没有进入 outbox");
    let pushed = cloud_sync::push_outbox(&device_a).unwrap();
    assert_eq!(pushed.pushed, 1, "outbox 恢复在线后没有提交云端");
    assert_eq!(pushed.pending, 0, "outbox 提交成功后仍有待同步操作");
    assert_eq!(pushed.conflicts, 0, "正常的 outbox 提交产生了冲突");
    cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: None,
        },
    )
    .unwrap();
    assert!(
        list_tasks(&device_b)
            .iter()
            .any(|task| task.id == queued_task.id),
        "设备 B 没有收到 outbox 提交的事项"
    );

    // Actual adjustment batches upload new tasks and their initial adjustment
    // as one remote transaction. A zero-minute draft creates only its task.
    let adjustment_date = chrono::Local::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();
    let quick_create_parent = create_task(&device_a, &subject_id, "Supabase E2E 快速新增父事项");
    cloud_sync::enqueue_entity(
        &device_a,
        "task_create",
        "task",
        Some(&quick_create_parent.id),
        None,
        None,
    )
    .unwrap();
    assert_eq!(push_all(&device_a).conflicts, 0);
    pull_all(&device_b);
    let quick_create_result = actual_time_adjustments::apply_batch(
        &device_a,
        ActualAdjustmentBatchRequest {
            subject_id: subject_id.clone(),
            work_date: adjustment_date.clone(),
            client_request_id: Uuid::now_v7().to_string(),
            batch_id: Uuid::now_v7().to_string(),
            note: Some("Supabase E2E 快速新增".to_string()),
            day_instance_date: None,
            items: vec![],
            new_tasks: vec![
                ActualAdjustmentNewTaskInput {
                    client_task_id: Uuid::now_v7().to_string(),
                    title: "Supabase E2E 带实际事项".to_string(),
                    parent_id: Some(quick_create_parent.id.clone()),
                    expected_parent_version: Some(quick_create_parent.version),
                    target_minutes: 30,
                },
                ActualAdjustmentNewTaskInput {
                    client_task_id: Uuid::now_v7().to_string(),
                    title: "Supabase E2E 零分钟事项".to_string(),
                    parent_id: None,
                    expected_parent_version: None,
                    target_minutes: 0,
                },
            ],
        },
    )
    .unwrap();
    assert_eq!(quick_create_result.created_task_count, 2);
    assert_eq!(quick_create_result.created_adjustment_count, 1);
    assert_eq!(push_all(&device_a).conflicts, 0);
    cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: None,
        },
    )
    .unwrap();
    let device_b_tasks = list_tasks(&device_b);
    let timed_task = device_b_tasks
        .iter()
        .find(|task| task.title == "Supabase E2E 带实际事项")
        .expect("设备 B 没有收到批次新增的带实际事项");
    assert_eq!(
        timed_task.parent_id.as_deref(),
        Some(quick_create_parent.id.as_str()),
        "设备 B 收到的新事项没有保留父子层级"
    );
    let zero_task = device_b_tasks
        .iter()
        .find(|task| task.title == "Supabase E2E 零分钟事项")
        .expect("设备 B 没有收到批次新增的零分钟事项");
    assert_eq!(
        actual_time_adjustments::direct_actual_minutes(
            &device_b.open().unwrap(),
            &workspace.id,
            &timed_task.id,
            Some(&adjustment_date),
        )
        .unwrap(),
        30
    );
    assert_eq!(
        actual_time_adjustments::direct_actual_minutes(
            &device_b.open().unwrap(),
            &workspace.id,
            &zero_task.id,
            Some(&adjustment_date),
        )
        .unwrap(),
        0
    );

    // A complete task+adjustment batch survives an interrupted upload. The
    // same outbox operation is retried after connectivity returns, and the
    // other device observes exactly one task and one initial adjustment.
    let interrupted_request_id = Uuid::now_v7().to_string();
    let interrupted_title = format!("Supabase E2E 中断续传 {}", Uuid::now_v7());
    let original_project_url = device_a
        .open()
        .unwrap()
        .query_row(
            "SELECT value_json FROM device_settings WHERE key='supabase_project_url'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap();
    actual_time_adjustments::apply_batch(
        &device_a,
        ActualAdjustmentBatchRequest {
            subject_id: subject_id.clone(),
            work_date: adjustment_date.clone(),
            client_request_id: interrupted_request_id.clone(),
            batch_id: Uuid::now_v7().to_string(),
            note: Some("Supabase E2E 中断续传".to_string()),
            day_instance_date: None,
            items: vec![],
            new_tasks: vec![ActualAdjustmentNewTaskInput {
                client_task_id: Uuid::now_v7().to_string(),
                title: interrupted_title.clone(),
                parent_id: None,
                expected_parent_version: None,
                target_minutes: 35,
            }],
        },
    )
    .unwrap();
    device_a
        .open()
        .unwrap()
        .execute(
            "UPDATE device_settings SET value_json=?1 WHERE key='supabase_project_url'",
            [json!("http://127.0.0.1:9").to_string()],
        )
        .unwrap();
    assert!(cloud_sync::push_outbox(&device_a).is_err());
    assert_eq!(
        cloud_sync::sync_status(&device_a)
            .unwrap()
            .pending_operations,
        1,
        "中断后的原子实际调整批次没有保留待重试"
    );
    device_a
        .open()
        .unwrap()
        .execute(
            "UPDATE device_settings SET value_json=?1 WHERE key='supabase_project_url'",
            [original_project_url],
        )
        .unwrap();
    assert_eq!(push_all(&device_a).conflicts, 0);
    pull_all(&device_b);
    let interrupted_tasks = list_tasks(&device_b)
        .into_iter()
        .filter(|task| task.title == interrupted_title)
        .collect::<Vec<_>>();
    assert_eq!(interrupted_tasks.len(), 1, "中断续传重复创建了事项");
    let interrupted_task = &interrupted_tasks[0];
    assert_eq!(
        actual_time_adjustments::direct_actual_minutes(
            &device_b.open().unwrap(),
            &workspace.id,
            &interrupted_task.id,
            Some(&adjustment_date),
        )
        .unwrap(),
        35
    );
    assert_eq!(
        device_b
            .open()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM actual_time_adjustments WHERE task_id=?1 AND deleted_at IS NULL",
                [&interrupted_task.id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1,
        "中断续传重复创建了初始调整"
    );

    // Two devices can submit from the same old adjustment fingerprint. The
    // first batch wins; using cloud on the losing device removes its optimistic
    // local record before the authoritative batch is pulled.
    let concurrent_task = create_task(&device_a, &subject_id, "Supabase E2E 并发调整事项");
    cloud_sync::enqueue_entity(
        &device_a,
        "task_create",
        "task",
        Some(&concurrent_task.id),
        None,
        None,
    )
    .unwrap();
    assert_eq!(push_all(&device_a).conflicts, 0);
    cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: None,
        },
    )
    .unwrap();
    let snapshot_a = actual_time_adjustments::snapshot(
        &device_a,
        ActualAdjustmentSnapshotRequest {
            subject_id: subject_id.clone(),
            work_date: adjustment_date.clone(),
            execution_only: None,
        },
    )
    .unwrap();
    let snapshot_b = actual_time_adjustments::snapshot(
        &device_b,
        ActualAdjustmentSnapshotRequest {
            subject_id: subject_id.clone(),
            work_date: adjustment_date.clone(),
            execution_only: None,
        },
    )
    .unwrap();
    let task_a = snapshot_a
        .tasks
        .iter()
        .find(|task| task.id == concurrent_task.id)
        .unwrap();
    let task_b = snapshot_b
        .tasks
        .iter()
        .find(|task| task.id == concurrent_task.id)
        .unwrap();
    for (database, task, target_minutes) in
        [(&device_a, task_a, 10_i64), (&device_b, task_b, 20_i64)]
    {
        actual_time_adjustments::apply_batch(
            database,
            ActualAdjustmentBatchRequest {
                subject_id: subject_id.clone(),
                work_date: adjustment_date.clone(),
                client_request_id: Uuid::now_v7().to_string(),
                batch_id: Uuid::now_v7().to_string(),
                note: Some("Supabase E2E 并发基线".to_string()),
                day_instance_date: None,
                items: vec![ActualAdjustmentBatchItem {
                    task_id: concurrent_task.id.clone(),
                    expected_task_version: task.version,
                    baseline_minutes: task.direct_minutes,
                    target_minutes,
                    delta_minutes: target_minutes - task.direct_minutes,
                    adjustment_fingerprint: task.adjustment_fingerprint.clone(),
                }],
                new_tasks: vec![],
            },
        )
        .unwrap();
    }
    assert_eq!(push_all(&device_a).conflicts, 0);
    assert_eq!(push_all(&device_b).conflicts, 1);
    let conflicts = cloud_sync::list_conflicts(&device_b).unwrap();
    let adjustment_conflict = conflicts
        .iter()
        .find(|conflict| conflict.entity_type == "actual_adjustment_batch")
        .expect("同一旧基线的第二个批次没有形成云端冲突");
    cloud_sync::resolve_conflict(
        &device_b,
        CloudConflictResolveRequest {
            operation_id: adjustment_conflict.operation_id.clone(),
            strategy: "use_cloud".to_string(),
        },
    )
    .unwrap();
    cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: None,
        },
    )
    .unwrap();
    assert_eq!(
        actual_time_adjustments::direct_actual_minutes(
            &device_b.open().unwrap(),
            &workspace.id,
            &concurrent_task.id,
            Some(&adjustment_date),
        )
        .unwrap(),
        10
    );

    // Both devices delete the same adjustment from the same version while
    // offline. The idempotent tombstone converges without restoring minutes.
    let delete_adjustment = device_a
        .open()
        .unwrap()
        .query_row(
            "SELECT id,version FROM actual_time_adjustments WHERE task_id=?1 AND deleted_at IS NULL ORDER BY created_at,id LIMIT 1",
            [&concurrent_task.id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .unwrap();
    let delete_adjustment_b = device_b
        .open()
        .unwrap()
        .query_row(
            "SELECT id,version FROM actual_time_adjustments WHERE task_id=?1 AND deleted_at IS NULL ORDER BY created_at,id LIMIT 1",
            [&concurrent_task.id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .unwrap();
    assert_eq!(delete_adjustment_b, delete_adjustment);
    for database in [&device_a, &device_b] {
        actual_time_adjustments::delete_adjustment(
            database,
            ActualAdjustmentDeleteRequest {
                adjustment_id: delete_adjustment.0.clone(),
                expected_version: delete_adjustment.1,
                client_request_id: Uuid::now_v7().to_string(),
            },
        )
        .unwrap();
    }
    assert_eq!(push_all(&device_a).conflicts, 0);
    let delete_push_b = push_all(&device_b);
    assert_eq!(delete_push_b.conflicts, 1, "并发删除没有形成预期的版本冲突");
    let delete_conflict = cloud_sync::list_conflicts(&device_b)
        .unwrap()
        .into_iter()
        .find(|item| item.entity_type == "actual_time_adjustment")
        .expect("并发删除冲突没有进入调整记录冲突列表");
    cloud_sync::resolve_conflict(
        &device_b,
        CloudConflictResolveRequest {
            operation_id: delete_conflict.operation_id,
            strategy: "use_cloud".to_string(),
        },
    )
    .unwrap();
    pull_all(&device_a);
    pull_all(&device_b);
    for database in [&device_a, &device_b] {
        let deleted = database
            .open()
            .unwrap()
            .query_row(
                "SELECT deleted_at,version FROM actual_time_adjustments WHERE id=?1",
                [&delete_adjustment.0],
                |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, i64>(1)?)),
            )
            .unwrap();
        assert!(deleted.0.is_some(), "并发删除后调整记录没有收敛为软删除");
        assert_eq!(
            actual_time_adjustments::direct_actual_minutes(
                &database.open().unwrap(),
                &workspace.id,
                &concurrent_task.id,
                Some(&adjustment_date),
            )
            .unwrap(),
            0,
            "并发删除后最终实际分钟没有收敛"
        );
    }

    // Planning parity: daily estimates and recurrence rules use the same
    // local-first outbox, propagate across devices, and conflict by their
    // planning identity rather than by the whole task.
    let planning_task = create_task(&device_a, &subject_id, "Supabase E2E 规划事项");
    cloud_sync::enqueue_entity(
        &device_a,
        "task_create",
        "task",
        Some(&planning_task.id),
        None,
        None,
    )
    .unwrap();
    cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: None,
        },
    )
    .unwrap();
    let today = chrono::Local::now().date_naive();
    let today_text = today.format("%Y-%m-%d").to_string();
    let tomorrow_text = today.succ_opt().unwrap().format("%Y-%m-%d").to_string();
    let next_text = today
        .succ_opt()
        .unwrap()
        .succ_opt()
        .unwrap()
        .format("%Y-%m-%d")
        .to_string();
    let state_a = supabase::storage_mode(&device_a).unwrap();
    let state_b = supabase::storage_mode(&device_b).unwrap();

    tasks::set_task_daily_estimate_for_cloud_test(
        &device_a,
        TaskDailyEstimateSetRequest {
            task_id: planning_task.id.clone(),
            work_date: today_text.clone(),
            estimate_minutes: Some(45),
        },
        &state_a,
    )
    .unwrap();
    assert_eq!(
        cloud_sync::sync_status(&device_a)
            .unwrap()
            .pending_operations,
        1,
        "离线优先按日预估没有进入 outbox"
    );
    push_all(&device_a);
    cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: None,
        },
    )
    .unwrap();
    assert_eq!(
        daily_estimate(&device_b, &planning_task.id, &today_text),
        Some(45),
        "设备 B 没有收到设备 A 的按日预估"
    );

    let rule_a = recurring::save_rule_for_cloud_test(
        &device_a,
        RecurrenceSaveRequest {
            task_id: planning_task.id.clone(),
            task_expected_version: task_version(&device_a, &planning_task.id),
            frequency: "daily".to_string(),
            weekdays_mask: None,
            effective_start: today_text.clone(),
            rule_expected_version: None,
        },
        &state_a,
    )
    .unwrap();
    push_all(&device_a);
    cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: None,
        },
    )
    .unwrap();
    let workspace_b_id: String = device_b
        .open()
        .unwrap()
        .query_row("SELECT id FROM workspaces LIMIT 1", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        recurring::get_active_rule(
            &device_b.open().unwrap(),
            &workspace_b_id,
            &planning_task.id
        )
        .unwrap()
        .map(|rule| rule.frequency),
        Some("daily".to_string()),
        "设备 B 没有收到重复规则"
    );

    // Synchronize the occurrence materialized by the first recurring estimate
    // before the estimate race, so the expected conflict is estimate-scoped.
    tasks::set_task_daily_estimate_for_cloud_test(
        &device_a,
        TaskDailyEstimateSetRequest {
            task_id: planning_task.id.clone(),
            work_date: today_text.clone(),
            estimate_minutes: Some(45),
        },
        &state_a,
    )
    .unwrap();
    push_all(&device_a);
    cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: None,
        },
    )
    .unwrap();

    // Both devices now share the same estimate version. Device A wins the next
    // version; the
    // stale device B update must become a conflict and remain resolvable.
    tasks::set_task_daily_estimate_for_cloud_test(
        &device_a,
        TaskDailyEstimateSetRequest {
            task_id: planning_task.id.clone(),
            work_date: today_text.clone(),
            estimate_minutes: Some(60),
        },
        &state_a,
    )
    .unwrap();
    push_all(&device_a);
    tasks::set_task_daily_estimate_for_cloud_test(
        &device_b,
        TaskDailyEstimateSetRequest {
            task_id: planning_task.id.clone(),
            work_date: today_text.clone(),
            estimate_minutes: Some(90),
        },
        &state_b,
    )
    .unwrap();
    assert_eq!(push_all(&device_b).conflicts, 1);
    let estimate_conflict = cloud_sync::list_conflicts(&device_b)
        .unwrap()
        .into_iter()
        .find(|conflict| conflict.entity_type == "task_daily_estimate")
        .expect("同日期按日预估冲突没有进入冲突列表");
    cloud_sync::resolve_conflict(
        &device_b,
        CloudConflictResolveRequest {
            operation_id: estimate_conflict.operation_id,
            strategy: "use_cloud".to_string(),
        },
    )
    .unwrap();
    cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            after_change_seq: Some(0),
            limit: None,
        },
    )
    .unwrap();
    assert_eq!(
        daily_estimate(&device_b, &planning_task.id, &today_text),
        Some(60)
    );

    // Different dates are independent identities and may upload in parallel.
    tasks::set_task_daily_estimate_for_cloud_test(
        &device_a,
        TaskDailyEstimateSetRequest {
            task_id: planning_task.id.clone(),
            work_date: tomorrow_text.clone(),
            estimate_minutes: Some(30),
        },
        &state_a,
    )
    .unwrap();
    tasks::set_task_daily_estimate_for_cloud_test(
        &device_b,
        TaskDailyEstimateSetRequest {
            task_id: planning_task.id.clone(),
            work_date: next_text.clone(),
            estimate_minutes: Some(20),
        },
        &state_b,
    )
    .unwrap();
    assert_eq!(push_all(&device_a).conflicts, 0);
    assert_eq!(push_all(&device_b).conflicts, 0);
    cloud_sync::pull(
        &device_a,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: None,
        },
    )
    .unwrap();
    cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: None,
        },
    )
    .unwrap();
    for database in [&device_a, &device_b] {
        assert_eq!(
            daily_estimate(database, &planning_task.id, &tomorrow_text),
            Some(30)
        );
        assert_eq!(
            daily_estimate(database, &planning_task.id, &next_text),
            Some(20)
        );
    }

    // Rule conflicts are task-scoped and atomic.
    let changed_rule_a = recurring::save_rule_for_cloud_test(
        &device_a,
        RecurrenceSaveRequest {
            task_id: planning_task.id.clone(),
            task_expected_version: task_version(&device_a, &planning_task.id),
            frequency: "weekdays".to_string(),
            weekdays_mask: None,
            effective_start: today_text.clone(),
            rule_expected_version: Some(rule_a.version),
        },
        &state_a,
    )
    .unwrap();
    push_all(&device_a);
    recurring::save_rule_for_cloud_test(
        &device_b,
        RecurrenceSaveRequest {
            task_id: planning_task.id.clone(),
            task_expected_version: task_version(&device_b, &planning_task.id),
            frequency: "weekly".to_string(),
            weekdays_mask: Some(21),
            effective_start: today_text.clone(),
            rule_expected_version: Some(rule_a.version),
        },
        &state_b,
    )
    .unwrap();
    assert_eq!(push_all(&device_b).conflicts, 1);
    let rule_conflict = cloud_sync::list_conflicts(&device_b)
        .unwrap()
        .into_iter()
        .find(|conflict| conflict.entity_type == "task_recurrence_rule")
        .expect("重复规则冲突没有进入冲突列表");
    cloud_sync::resolve_conflict(
        &device_b,
        CloudConflictResolveRequest {
            operation_id: rule_conflict.operation_id,
            strategy: "use_cloud".to_string(),
        },
    )
    .unwrap();
    cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            after_change_seq: Some(0),
            limit: None,
        },
    )
    .unwrap();
    assert_eq!(
        recurring::get_active_rule(
            &device_b.open().unwrap(),
            &workspace_b_id,
            &planning_task.id
        )
        .unwrap()
        .map(|rule| (rule.frequency, rule.version)),
        Some(("weekdays".to_string(), changed_rule_a.version))
    );

    // Explicit clear is a tombstone operation and must remove the other
    // device's cached value after incremental refresh.
    tasks::set_task_daily_estimate_for_cloud_test(
        &device_a,
        TaskDailyEstimateSetRequest {
            task_id: planning_task.id.clone(),
            work_date: today_text.clone(),
            estimate_minutes: None,
        },
        &state_a,
    )
    .unwrap();
    push_all(&device_a);
    cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: None,
        },
    )
    .unwrap();
    assert_eq!(
        daily_estimate(&device_b, &planning_task.id, &today_text),
        None,
        "清空按日预估没有传播到设备 B"
    );

    let stale_on_b = list_tasks(&device_b)
        .into_iter()
        .find(|task| task.id == synced_task.id)
        .unwrap();
    let updated_on_a = update_task_title(&device_a, &synced_task, "设备 A 更新后的标题");
    cloud_sync::enqueue_entity(
        &device_a,
        "task_update",
        "task",
        Some(&updated_on_a.id),
        Some(synced_task.version),
        None,
    )
    .unwrap();
    let stale_update = update_task_title(&device_b, &stale_on_b, "设备 B 的过期标题");
    cloud_sync::enqueue_entity_deferred(
        &device_b,
        "task_update",
        "task",
        Some(&stale_update.id),
        Some(stale_on_b.version),
        None,
    )
    .unwrap();
    let conflict = cloud_sync::push_outbox(&device_b).unwrap();
    assert_eq!(conflict.conflicts, 1, "过期版本没有进入冲突队列");
    let conflicts = cloud_sync::list_conflicts(&device_b).unwrap();
    assert_eq!(conflicts.len(), 1, "冲突列表没有返回过期更新");
    cloud_sync::resolve_conflict(
        &device_b,
        CloudConflictResolveRequest {
            operation_id: conflicts[0].operation_id.clone(),
            strategy: "use_cloud".to_string(),
        },
    )
    .unwrap();
    cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            after_change_seq: Some(0),
            limit: None,
        },
    )
    .unwrap();
    assert!(
        cloud_sync::list_conflicts(&device_b).unwrap().is_empty(),
        "选择云端版本后冲突仍未清除"
    );
    assert_eq!(
        list_tasks(&device_b)
            .into_iter()
            .find(|task| task.id == synced_task.id)
            .map(|task| task.title),
        Some("设备 A 更新后的标题".to_string()),
        "冲突选择云端版本后本地缓存没有恢复云端内容"
    );

    let timer = cloud_sync::legacy_cloud_timer_start_local_first(
        &device_a,
        CloudTimerStartRequest {
            task_id: Some(synced_task.id.clone()),
            note: Some("设备 A 启动".to_string()),
            operation_id: Uuid::now_v7().to_string(),
        },
    )
    .unwrap();
    push_all(&device_a);
    cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: None,
        },
    )
    .unwrap();
    let parallel = cloud_sync::legacy_cloud_timer_start_local_first(
        &device_b,
        CloudTimerStartRequest {
            task_id: Some(synced_task.id.clone()),
            note: Some("设备 B 重复启动".to_string()),
            operation_id: Uuid::now_v7().to_string(),
        },
    )
    .unwrap_err();
    assert!(parallel.contains("ACTIVE_TIMER_EXISTS"));

    let entry_id = timer.id.clone();
    let paused = cloud_sync::legacy_cloud_timer_pause_local_first(
        &device_b,
        CloudTimerVersionRequest {
            entry_id: entry_id.clone(),
            expected_version: timer.version,
            operation_id: Uuid::now_v7().to_string(),
        },
    )
    .unwrap();
    push_all(&device_b);
    cloud_sync::pull(
        &device_a,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: None,
        },
    )
    .unwrap();
    let resumed = cloud_sync::legacy_cloud_timer_resume_local_first(
        &device_a,
        CloudTimerVersionRequest {
            entry_id: entry_id.clone(),
            expected_version: paused.version,
            operation_id: Uuid::now_v7().to_string(),
        },
    )
    .unwrap();
    push_all(&device_a);
    cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: None,
        },
    )
    .unwrap();
    let stopped = cloud_sync::legacy_cloud_timer_stop_local_first(
        &device_b,
        CloudTimerVersionRequest {
            entry_id,
            expected_version: resumed.version,
            operation_id: Uuid::now_v7().to_string(),
        },
    )
    .unwrap();
    push_all(&device_b);
    assert_eq!(stopped.state, "ended");
    cloud_sync::pull(
        &device_a,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: None,
        },
    )
    .unwrap();
    assert!(
        crate::time_tracking::get_timer_state(&device_a)
            .unwrap()
            .is_none(),
        "全局计时器停止后仍然存在活动计时"
    );

    let unassigned_task = create_task(&device_a, &subject_id, "Supabase E2E 未归属事项");
    cloud_sync::enqueue_entity(
        &device_a,
        "task_create",
        "task",
        Some(&unassigned_task.id),
        None,
        None,
    )
    .unwrap();
    cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: None,
        },
    )
    .unwrap();
    let reset =
        reset_shared_unassigned_anchor(&device_a, chrono::Utc::now().timestamp_millis() - 120_000);
    cloud_sync::apply_remote_unassigned_session_for_test(&device_b, &reset["session"]).unwrap();
    let unassigned_state = unassigned::get_state(&device_a).unwrap().unwrap();
    let unassigned_result = unassigned::resolve_work_for_sync(
        &device_a,
        ResolveWorkRequest {
            session_id: unassigned_state.session_id.clone(),
            expected_version: unassigned_state.version,
            operation_id: Uuid::now_v7().to_string(),
            allocations: vec![UnassignedAllocationInput {
                task_id: unassigned_task.id.clone(),
                minutes: unassigned_state.required_minutes,
                complete_task: false,
                task_expected_version: None,
            }],
        },
    )
    .unwrap();
    let generated_entry_id = unassigned_result
        .generated_entry_id
        .as_deref()
        .expect("未归属处理没有生成时间记录");
    push_all(&device_a);
    cloud_sync::pull(
        &device_b,
        CloudSyncPullRequest {
            after_change_seq: None,
            limit: None,
        },
    )
    .unwrap();
    let synced_unassigned_entry = today_entries(&device_b)
        .entries
        .into_iter()
        .find(|entry| entry.id == generated_entry_id)
        .expect("设备 B 没有同步到未归属处理生成的记录");
    assert_eq!(synced_unassigned_entry.kind, "work");
    assert_eq!(synced_unassigned_entry.source_type, "unassigned");
    assert_eq!(
        synced_unassigned_entry.allocated_minutes, unassigned_state.required_minutes,
        "未归属时间同步后分配分钟不正确"
    );

    let lease_a = cloud_sync::acquire_lease(&device_a).unwrap();
    assert_eq!(lease_a["acquired"], true);
    let lease_b_blocked = cloud_sync::acquire_lease(&device_b).unwrap();
    assert_eq!(lease_b_blocked["acquired"], false);
    cloud_sync::release_lease(
        &device_a,
        TrackingLeaseRequest {
            lease_token: lease_a["leaseToken"].as_str().map(ToOwned::to_owned),
        },
    )
    .unwrap();
    let lease_b = cloud_sync::acquire_lease(&device_b).unwrap();
    assert_eq!(lease_b["acquired"], true);
    let device_b_id = supabase::storage_mode(&device_b).unwrap().device_id;
    let lease_rows = cloud_sync::get_lease(&device_a).unwrap();
    assert_eq!(
        lease_rows
            .as_array()
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("holder_device_id"))
            .and_then(Value::as_str),
        Some(device_b_id.as_str()),
        "租约获取后云端持有设备不是设备 B"
    );
    cloud_sync::release_lease(
        &device_b,
        TrackingLeaseRequest {
            lease_token: lease_b["leaseToken"].as_str().map(ToOwned::to_owned),
        },
    )
    .unwrap();

    let device_a_id = supabase::storage_mode(&device_a).unwrap().device_id;
    let device_b_id = supabase::storage_mode(&device_b).unwrap().device_id;
    let retained_task = create_task(&device_b, &subject_id, "撤销后保留的设备 B 本地事项");
    cloud_sync::enqueue_entity_deferred(
        &device_b,
        "task_create",
        "task",
        Some(&retained_task.id),
        None,
        Some(&Uuid::now_v7().to_string()),
    )
    .unwrap();
    supabase::revoke_device(&device_a, &device_b_id).unwrap();
    assert!(
        supabase::current_session(&device_a).is_ok(),
        "撤销设备 B 不应影响设备 A 的登录状态"
    );
    let revoked_push = cloud_sync::push_outbox(&device_b).unwrap_err();
    assert!(revoked_push.contains("DEVICE_REVOKED"));
    assert!(
        list_tasks(&device_b)
            .iter()
            .any(|task| task.id == retained_task.id),
        "设备 B 被撤销后本地事项丢失"
    );
    assert!(
        cloud_sync::sync_status(&device_b)
            .unwrap()
            .pending_operations
            > 0,
        "设备 B 被撤销后 outbox 被错误删除"
    );
    assert_eq!(
        supabase::session_snapshot_for_database(&device_b)
            .unwrap()
            .status,
        "reauth_required"
    );

    configure_and_sign_in(&device_b, &config);
    register_device(&device_b, "Supabase E2E 设备 B");
    let blocked_push = cloud_sync::push_outbox(&device_b).unwrap_err();
    assert!(blocked_push.contains("AUTH_RESUME_REQUIRED"));
    supabase::resume_pending_sync(&device_b).unwrap();
    assert!(cloud_sync::push_outbox(&device_b).is_ok());

    supabase::sign_out_all(&device_a).unwrap();
    supabase::force_cached_session_expiry_for_test(&device_b).unwrap();
    assert!(
        supabase::current_session(&device_b).is_err(),
        "退出所有设备后设备 B 的 refresh token 仍可用"
    );

    configure_and_sign_in(&device_a, &config);
    register_device(&device_a, "Supabase E2E 设备 A");
    configure_and_sign_in(&device_b, &config);
    register_device(&device_b, "Supabase E2E 设备 B");
    let externally_revoked_session = supabase::current_session(&device_b).unwrap();
    let external_logout = reqwest::blocking::Client::new()
        .post(format!(
            "{}/auth/v1/logout?scope=global",
            config.project_url
        ))
        .header("apikey", &config.anon_key)
        .bearer_auth(&externally_revoked_session.access_token)
        .send()
        .unwrap();
    assert!(
        external_logout.status().is_success(),
        "模拟后台 Auth 撤销失败"
    );
    supabase::force_cached_session_expiry_for_test(&device_b).unwrap();
    assert!(
        supabase::current_session(&device_b).is_err(),
        "后台 Auth 撤销后客户端刷新仍被视为有效"
    );
    assert_eq!(
        supabase::storage_mode(&device_a).unwrap().device_id,
        device_a_id
    );

    configure_and_sign_in(&device_a, &config);
    reset_owned_workspaces(&device_a);
    let _ = supabase::sign_out(&device_a);
    let _ = supabase::sign_out(&device_b);
}
