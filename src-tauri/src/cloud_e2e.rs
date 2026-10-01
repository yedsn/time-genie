use std::env;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;
use uuid::Uuid;

use crate::cloud_sync::{
    self, CloudConflictResolveRequest, CloudSyncPullRequest, CloudTimerStartRequest,
    CloudTimerVersionRequest, TrackingLeaseRequest,
};
use crate::database::Database;
use crate::subjects;
use crate::supabase::{
    self, CloudConfigureRequest, CloudDeviceRegisterRequest, CloudSignInRequest,
    MigrationExecuteRequest,
};
use crate::tasks::{self, TaskCreateRequest, TaskListRequest, TaskUpdateRequest};

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

async fn subscribe_to_workspace_changes(
    database: &Database,
    workspace_id: &str,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let session = supabase::current_session(database).unwrap();
    let api = supabase::client(database).unwrap();
    let mut url = Url::parse(&api.project_url).unwrap();
    let websocket_scheme = if url.scheme() == "https" { "wss" } else { "ws" };
    url.set_scheme(websocket_scheme).unwrap();
    url.set_path("/realtime/v1/websocket");
    url.set_query(None);
    url.query_pairs_mut()
        .append_pair("apikey", api.anon_key())
        .append_pair("vsn", "1.0.0");
    let (mut socket, _) = connect_async(url.as_str()).await.unwrap();
    let topic = format!("realtime:workspace_changes:{workspace_id}");
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
                            "schema": "public",
                            "table": "workspace_changes",
                            "filter": format!("workspace_id=eq.{workspace_id}")
                        }],
                        "private": false
                    },
                    "access_token": session.access_token
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

#[test]
#[ignore = "需要专用 Supabase 测试账号和显式的工作空间清理授权"]
fn real_supabase_two_device_flow() {
    let config = TestConfig::from_environment();
    let directory = tempfile::tempdir().unwrap();
    let device_a = Database::initialize_at(directory.path().join("device-a.sqlite3")).unwrap();
    let device_b = Database::initialize_at(directory.path().join("device-b.sqlite3")).unwrap();

    configure_and_sign_in(&device_a, &config);
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

    let api = supabase::client(&device_a).unwrap();
    let session = supabase::current_session(&device_a).unwrap();
    let devices: Vec<Value> = api
        .request(
            api.get(format!(
                "{}/rest/v1/devices?workspace_id=eq.{}&revoked_at=is.null&select=id,device_name",
                api.project_url, workspace.id
            )),
            &session,
        )
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(devices.len(), 2, "云端没有注册两台独立设备");

    let subject_id = subjects::list_subjects(&device_a).unwrap()[0].id.clone();
    let realtime_database = device_b.clone();
    let realtime_workspace_id = workspace.id.clone();
    let (realtime_ready_tx, realtime_ready_rx) = std::sync::mpsc::sync_channel(0);
    let realtime_listener = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let mut socket =
                subscribe_to_workspace_changes(&realtime_database, &realtime_workspace_id).await;
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

    let timer = cloud_sync::timer_start(
        &device_a,
        CloudTimerStartRequest {
            task_id: Some(synced_task.id.clone()),
            note: Some("设备 A 启动".to_string()),
            operation_id: Uuid::now_v7().to_string(),
        },
    )
    .unwrap();
    let parallel = cloud_sync::timer_start(
        &device_b,
        CloudTimerStartRequest {
            task_id: Some(synced_task.id.clone()),
            note: Some("设备 B 重复启动".to_string()),
            operation_id: Uuid::now_v7().to_string(),
        },
    )
    .unwrap_err();
    assert!(parallel.contains("REMOTE_TIMER_ACTIVE"));

    let entry_id = timer["id"].as_str().unwrap().to_string();
    let paused = cloud_sync::timer_action(
        &device_b,
        "timer_pause",
        CloudTimerVersionRequest {
            entry_id: entry_id.clone(),
            expected_version: timer["version"].as_i64().unwrap(),
            operation_id: Uuid::now_v7().to_string(),
        },
    )
    .unwrap();
    let resumed = cloud_sync::timer_action(
        &device_a,
        "timer_resume",
        CloudTimerVersionRequest {
            entry_id: entry_id.clone(),
            expected_version: paused["version"].as_i64().unwrap(),
            operation_id: Uuid::now_v7().to_string(),
        },
    )
    .unwrap();
    let stopped = cloud_sync::timer_action(
        &device_b,
        "timer_stop",
        CloudTimerVersionRequest {
            entry_id,
            expected_version: resumed["version"].as_i64().unwrap(),
            operation_id: Uuid::now_v7().to_string(),
        },
    )
    .unwrap();
    assert_eq!(stopped["state"], "ended");
    assert!(
        cloud_sync::timer_get_state(&device_a).unwrap().is_none(),
        "全局计时器停止后仍然存在活动计时"
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

    reset_owned_workspaces(&device_a);
    let _ = supabase::sign_out(&device_a);
    let _ = supabase::sign_out(&device_b);
}
