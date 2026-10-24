use std::env;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;
use uuid::Uuid;

use crate::database::Database;
use crate::supabase::{
    self, CloudConfigureRequest, CloudDeviceRegisterRequest, CloudSignInRequest,
    CloudWorkspaceRestoreCommitRequest, CloudWorkspaceRestorePrepareRequest,
    CloudWorkspaceSnapshotCreateRequest,
};
use crate::tasks::{TaskCreateRequest, TaskListRequest, TaskUpdateRequest};

struct TestConfig {
    project_url: String,
    anon_key: String,
    email: Option<String>,
    password: Option<String>,
    session_json: Option<String>,
}

impl TestConfig {
    fn from_environment() -> Self {
        assert_eq!(
            env::var("TG_SUPABASE_E2E_ALLOW_RESET").as_deref(),
            Ok("1"),
            "该测试会删除专用测试账号的工作空间；请显式设置 TG_SUPABASE_E2E_ALLOW_RESET=1"
        );
        let session_json = env::var("TG_SUPABASE_SESSION_JSON")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let (email, password) = if session_json.is_some() {
            (None, None)
        } else {
            (
                Some(required_env("TG_SUPABASE_EMAIL")),
                Some(required_env("TG_SUPABASE_PASSWORD")),
            )
        };
        Self {
            project_url: required_env("TG_SUPABASE_URL")
                .trim_end_matches('/')
                .to_string(),
            anon_key: required_env("TG_SUPABASE_ANON_KEY"),
            email,
            password,
            session_json,
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
    if let Some(session_json) = config.session_json.as_deref() {
        supabase::install_serialized_test_session_for_database(database, session_json).unwrap();
    } else {
        supabase::sign_in_password(
            database,
            CloudSignInRequest {
                email: config.email.clone().expect("缺少 Supabase 测试邮箱"),
                password: config.password.clone().expect("缺少 Supabase 测试密码"),
            },
        )
        .unwrap();
    }
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
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!(
            "无法清理专用 Supabase 测试工作空间: {}",
            response.text().unwrap_or_default()
        ))
    }
}

struct CleanupGuard {
    database: Database,
    sign_out: bool,
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        if supabase::current_session(&self.database).is_ok() {
            let _ = cleanup_owned_workspaces(&self.database);
            if self.sign_out {
                let _ = supabase::sign_out(&self.database);
            }
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

fn set_device_setting(database: &Database, key: &str, value: Value) {
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

fn enter_direct_cloud_mode(database: &Database, workspace_id: &str, workspace_epoch: i64) {
    set_device_setting(database, "cloud_workspace_id", json!(workspace_id));
    set_device_setting(database, "storage_mode", json!("cloud"));
    let device_id: String = database
        .open()
        .unwrap()
        .query_row(
            "SELECT json_extract(value_json, '$') FROM device_settings WHERE key='device_id'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let checkpoint = json!({
        "workspaceId": workspace_id,
        "deviceId": device_id,
        "changeSeq": 0,
        "workspaceEpoch": workspace_epoch,
        "fetchedAt": null,
        "lastError": null,
        "authBlockedReason": null
    });
    database
        .open()
        .unwrap()
        .execute(
            "INSERT INTO device_settings(key,value_json,updated_at) VALUES('direct_cloud_checkpoint',?1,0)
             ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json,updated_at=excluded.updated_at",
            [checkpoint.to_string()],
        )
        .unwrap();
    crate::cloud_sync::enter_direct_cloud_mode();
    supabase::direct_cloud_workbench_bootstrap(database).unwrap();
}

fn provision_direct_workspace(database: &Database, workspace_id: &str) {
    enter_direct_cloud_mode(database, workspace_id, 1);
}

fn assert_legacy_sync_tables_absent(database: &Database) {
    const TABLES: &[&str] = &[
        "sync_outbox",
        "local_sync_state",
        "cloud_deferred_entities",
        "legacy_abandoned_outbox",
        "processed_operations",
        "sync_items",
        "sync_runs",
    ];
    let connection = database.open().unwrap();
    for table in TABLES {
        let exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!exists, "直连数据库不应保留旧同步表 {table}");
    }
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
    url.set_scheme(if url.scheme() == "https" { "wss" } else { "ws" })
        .unwrap();
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
            if let Message::Text(text) = message.unwrap() {
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
    assert!(joined, "Supabase Realtime 拒绝 workspace_changes 订阅");
    socket
}

async fn wait_for_postgres_change(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) {
    let received = tokio::time::timeout(Duration::from_secs(15), async {
        while let Some(message) = socket.next().await {
            if let Message::Text(text) = message.unwrap() {
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
    assert!(received, "没有收到 workspace_changes Realtime 通知");
}

#[test]
#[ignore = "需要专用 Supabase 测试账号和显式的工作空间清理授权"]
fn real_supabase_direct_v2_two_device_flow() {
    let config = TestConfig::from_environment();
    let directory = tempfile::tempdir().unwrap();
    let device_a =
        Database::initialize_at(directory.path().join("direct-device-a.sqlite3")).unwrap();
    let device_b =
        Database::initialize_at(directory.path().join("direct-device-b.sqlite3")).unwrap();

    configure_and_sign_in(&device_a, &config);
    let _cleanup = CleanupGuard {
        database: device_a.clone(),
        sign_out: config.session_json.is_none(),
    };
    cleanup_owned_workspaces(&device_a).unwrap();
    let workspace = supabase::workspace_bootstrap(&device_a).unwrap();
    register_device(&device_a, "Direct V2 E2E 设备 A");

    configure_and_sign_in(&device_b, &config);
    assert_eq!(
        supabase::workspace_bootstrap(&device_b).unwrap().id,
        workspace.id
    );
    register_device(&device_b, "Direct V2 E2E 设备 B");
    provision_direct_workspace(&device_a, &workspace.id);
    enter_direct_cloud_mode(&device_b, &workspace.id, 1);

    assert_legacy_sync_tables_absent(&device_a);
    assert_legacy_sync_tables_absent(&device_b);
    let before_a = crate::storage_contract::business_data_fingerprint(&device_a).unwrap();
    let before_b = crate::storage_contract::business_data_fingerprint(&device_b).unwrap();

    let subject = supabase::direct_cloud_subject_create(
        &device_a,
        crate::subjects::SubjectCreateRequest {
            name: "Direct V2 验收主体".to_string(),
        },
    )
    .unwrap()
    .subject;
    let task = supabase::direct_cloud_task_create_for_command(
        &device_a,
        TaskCreateRequest {
            subject_id: subject.id.clone(),
            day_instance_date: None,
            parent_id: None,
            title: "Direct V2 双设备事项".to_string(),
            planned_date: None,
            estimate_minutes: Some(25),
            note: None,
            project_name: None,
            solution_name: None,
        },
        "manual",
    )
    .unwrap();
    let tasks_b = supabase::direct_cloud_task_list_for_command(
        &device_b,
        TaskListRequest {
            subject_id: Some(subject.id.clone()),
            include_completed: Some(true),
            planned_date: None,
            today_date: None,
            today_only: Some(false),
            daily_estimate_date: None,
            query: None,
        },
    )
    .unwrap();
    assert!(tasks_b.tasks.iter().any(|item| item.id == task.id));

    let stale = supabase::direct_cloud_task_update_for_command(
        &device_b,
        TaskUpdateRequest {
            id: task.id.clone(),
            expected_version: task.version - 1,
            title: Some("陈旧提交".to_string()),
            parent_id: None,
            planned_date: None,
            estimate_minutes: None,
            note: None,
            project_name: None,
            solution_name: None,
        },
    )
    .unwrap_err();
    assert!(stale.starts_with("VERSION_CONFLICT:"), "{stale}");

    let operation_id = Uuid::now_v7().to_string();
    let first = supabase::direct_cloud_mutation::<Value>(
        &device_a,
        "direct_cloud_task_day_plan_set",
        Some(operation_id.clone()),
        json!({"taskId": task.id, "workDate": "2026-10-23", "state": "included", "expectedVersion": null}),
    )
    .unwrap();
    let retry = supabase::direct_cloud_mutation::<Value>(
        &device_a,
        "direct_cloud_task_day_plan_set",
        Some(operation_id),
        json!({"taskId": task.id, "workDate": "2026-10-23", "state": "included", "expectedVersion": null}),
    )
    .unwrap();
    assert_eq!(first.data, retry.data, "幂等重试没有返回相同权威结果");

    let realtime_config = realtime_test_config(&device_b);
    let realtime_workspace_id = workspace.id.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(0);
    let listener = std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async move {
                let mut socket =
                    subscribe_to_workspace_changes(realtime_config, &realtime_workspace_id).await;
                ready_tx.send(()).unwrap();
                wait_for_postgres_change(&mut socket).await;
            });
    });
    ready_rx
        .recv_timeout(Duration::from_secs(20))
        .expect("Realtime 监听器未就绪");
    supabase::direct_cloud_task_update_for_command(
        &device_a,
        TaskUpdateRequest {
            id: task.id.clone(),
            expected_version: task.version,
            title: Some("Realtime 已更新".to_string()),
            parent_id: None,
            planned_date: None,
            estimate_minutes: None,
            note: None,
            project_name: None,
            solution_name: None,
        },
    )
    .unwrap();
    listener.join().expect("Realtime 未收到变更通知");

    let snapshot = supabase::workspace_snapshot_create(
        &device_a,
        CloudWorkspaceSnapshotCreateRequest {
            name: "Direct V2 E2E 恢复点".to_string(),
            note: None,
            snapshot_type: Some("manual".to_string()),
            lazy: Some(false),
        },
    )
    .unwrap();
    supabase::direct_cloud_subject_create(
        &device_a,
        crate::subjects::SubjectCreateRequest {
            name: "恢复后应消失".to_string(),
        },
    )
    .unwrap();
    let prepared = supabase::workspace_restore_prepare(
        &device_a,
        CloudWorkspaceRestorePrepareRequest {
            snapshot_id: snapshot.id,
            create_safety_snapshot: true,
        },
    )
    .unwrap();
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
    assert!(restored.workspace_epoch > restored.previous_epoch);
    enter_direct_cloud_mode(&device_b, &workspace.id, restored.workspace_epoch);
    assert!(supabase::direct_cloud_workbench_bootstrap(&device_b).is_ok());

    assert_eq!(
        before_a,
        crate::storage_contract::business_data_fingerprint(&device_a).unwrap(),
        "云端流程修改了设备 A 的本地业务表"
    );
    assert_eq!(
        before_b,
        crate::storage_contract::business_data_fingerprint(&device_b).unwrap(),
        "云端流程修改了设备 B 的本地业务表"
    );
    assert_legacy_sync_tables_absent(&device_a);
    assert_legacy_sync_tables_absent(&device_b);
}
