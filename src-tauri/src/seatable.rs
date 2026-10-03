use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::blocking::{Client, Response};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use url::Url;
use uuid::Uuid;

use crate::database::Database;
use crate::settings::integration_secret;
use crate::tasks::{list_tasks, TaskListRequest};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SeaTableConfig {
    server_url: String,
    #[serde(default = "default_task_table")]
    task_table: String,
    #[serde(default = "default_task_view")]
    task_view: String,
    #[serde(default = "default_reimbursement_table")]
    reimbursement_table: String,
    #[serde(default = "default_local_id_field")]
    local_id_field: String,
    #[serde(default = "default_item_field")]
    item_field: String,
    #[serde(default = "default_subject_field")]
    subject_field: String,
    #[serde(default = "default_project_field")]
    project_field: String,
    #[serde(default = "default_solution_field")]
    solution_field: String,
    #[serde(default = "default_status_field")]
    status_field: String,
    #[serde(default = "default_date_field")]
    date_field: String,
    #[serde(default = "default_hours_field")]
    hours_field: String,
    #[serde(default = "default_reimbursement_status")]
    reimbursement_pending_status: String,
    #[serde(default = "default_reimbursement_amount_field")]
    reimbursement_amount_field: String,
    #[serde(default = "default_reimbursement_item_field")]
    reimbursement_item_field: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSyncPreviewRequest {
    pub subject_id: String,
    pub work_date: String,
    #[serde(default)]
    pub task_ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncExecuteRequest {
    pub run_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReimbursementsRequest {
    pub subject_id: String,
    pub period_start: String,
    pub period_end: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SyncPreviewItem {
    pub item_id: String,
    pub task_id: String,
    pub title: String,
    pub action: String,
    pub reason: String,
    pub external_id: Option<String>,
    pub row: Value,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSyncPreview {
    pub run_id: String,
    pub table_name: String,
    pub work_date: String,
    pub subject_name: String,
    pub items: Vec<SyncPreviewItem>,
    pub create_count: usize,
    pub update_count: usize,
    pub skip_count: usize,
    pub conflict_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTestResult {
    pub base_name: String,
    pub task_table: String,
    pub reimbursement_table: String,
    pub reimbursement_available: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncExecutionResult {
    pub run_id: String,
    pub state: String,
    pub success_count: i64,
    pub failed_count: i64,
    pub skipped_count: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReimbursementItem {
    pub description: String,
    pub amount: f64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReimbursementResult {
    pub subject_id: String,
    pub subject_name: String,
    pub period_start: String,
    pub period_end: String,
    pub total: f64,
    pub items: Vec<ReimbursementItem>,
}

#[derive(Debug, Deserialize)]
struct AuthResponse {
    access_token: String,
    dtable_server: String,
    dtable_uuid: String,
    dtable_name: Option<String>,
    #[serde(default)]
    use_api_gateway: bool,
}

struct SeaTableClient {
    client: Client,
    headers: HeaderMap,
    rows_url: String,
    metadata_url: String,
    use_api_gateway: bool,
    base_name: String,
}

trait SeaTableApi {
    fn base_name(&self) -> &str;
    fn metadata(&self) -> Result<Value, String>;
    fn list_rows(&self, table: &str, view: Option<&str>) -> Result<Vec<Value>, String>;
    fn create_row(&self, table: &str, row: &Value) -> Result<String, String>;
    fn update_row(&self, table: &str, external_id: &str, row: &Value) -> Result<(), String>;
}

impl SeaTableClient {
    fn connect(config: &SeaTableConfig, token: &str) -> Result<Self, String> {
        let server = validate_server_url(&config.server_url)?;
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .map_err(|error| format!("NETWORK_ERROR: 无法创建 SeaTable 客户端: {error}"))?;
        let response = client
            .get(format!("{server}/api/v2.1/dtable/app-access-token/"))
            .header(AUTHORIZATION, format!("Token {token}"))
            .send()
            .map_err(network_error)?;
        let auth: AuthResponse = decode_response(response)?;
        let access = HeaderValue::from_str(&format!("Token {}", auth.access_token))
            .map_err(|error| format!("AUTH_FAILED: SeaTable 返回了无效访问令牌: {error}"))?;
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, access);
        let root = if auth.use_api_gateway {
            format!("{server}/api-gateway/api/v2/dtables/{}", auth.dtable_uuid)
        } else {
            format!(
                "{}/api/v1/dtables/{}",
                auth.dtable_server.trim_end_matches('/'),
                auth.dtable_uuid
            )
        };
        Ok(Self {
            client,
            headers,
            rows_url: format!("{root}/rows/"),
            metadata_url: format!("{root}/metadata/"),
            use_api_gateway: auth.use_api_gateway,
            base_name: auth.dtable_name.unwrap_or_default(),
        })
    }

    fn metadata(&self) -> Result<Value, String> {
        let response = self
            .client
            .get(&self.metadata_url)
            .headers(self.headers.clone())
            .send()
            .map_err(network_error)?;
        let value: Value = decode_response(response)?;
        Ok(value.get("metadata").cloned().unwrap_or(value))
    }

    fn list_rows(&self, table: &str, view: Option<&str>) -> Result<Vec<Value>, String> {
        let mut rows = Vec::new();
        let mut start = 0;
        loop {
            let mut request = self
                .client
                .get(&self.rows_url)
                .headers(self.headers.clone())
                .query(&[
                    ("table_name", table),
                    ("start", &start.to_string()),
                    ("limit", "1000"),
                ]);
            if self.use_api_gateway {
                request = request.query(&[("convert_keys", "true")]);
            }
            if let Some(view) = view.filter(|value| !value.trim().is_empty()) {
                request = request.query(&[("view_name", view)]);
            }
            let response = request.send().map_err(network_error)?;
            let value: Value = decode_response(response)?;
            let page = value
                .get("rows")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let page_len = page.len();
            rows.extend(page);
            if page_len < 1000 {
                break;
            }
            start += 1000;
        }
        Ok(rows)
    }

    fn create_row(&self, table: &str, row: &Value) -> Result<String, String> {
        let body = if self.use_api_gateway {
            json!({"table_name": table, "rows": [row]})
        } else {
            json!({"table_name": table, "row": row})
        };
        let response = self
            .client
            .post(&self.rows_url)
            .headers(self.headers.clone())
            .json(&body)
            .send()
            .map_err(network_error)?;
        let value: Value = decode_response(response)?;
        row_id(value.get("first_row").unwrap_or(&value))
            .ok_or_else(|| "SCHEMA_MISMATCH: SeaTable 新增成功但未返回行 ID".to_string())
    }

    fn update_row(&self, table: &str, external_id: &str, row: &Value) -> Result<(), String> {
        let body = if self.use_api_gateway {
            json!({"table_name": table, "updates": [{"row_id": external_id, "row": row}]})
        } else {
            json!({"table_name": table, "row_id": external_id, "row": row})
        };
        let response = self
            .client
            .put(&self.rows_url)
            .headers(self.headers.clone())
            .json(&body)
            .send()
            .map_err(network_error)?;
        let _: Value = decode_response(response)?;
        Ok(())
    }
}

impl SeaTableApi for SeaTableClient {
    fn base_name(&self) -> &str {
        &self.base_name
    }

    fn metadata(&self) -> Result<Value, String> {
        SeaTableClient::metadata(self)
    }

    fn list_rows(&self, table: &str, view: Option<&str>) -> Result<Vec<Value>, String> {
        SeaTableClient::list_rows(self, table, view)
    }

    fn create_row(&self, table: &str, row: &Value) -> Result<String, String> {
        SeaTableClient::create_row(self, table, row)
    }

    fn update_row(&self, table: &str, external_id: &str, row: &Value) -> Result<(), String> {
        SeaTableClient::update_row(self, table, external_id, row)
    }
}

pub fn test_connection(database: &Database) -> Result<ConnectionTestResult, String> {
    let connection = database.open()?;
    let (workspace_id, config) = load_config(&connection)?;
    let token = integration_secret("seatable", &workspace_id)?;
    let client = SeaTableClient::connect(&config, &token)?;
    test_connection_with(&config, &client)
}

fn test_connection_with(
    config: &SeaTableConfig,
    client: &impl SeaTableApi,
) -> Result<ConnectionTestResult, String> {
    let metadata = client.metadata()?;
    let tables = metadata
        .get("tables")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let task_columns = table_columns(&tables, &config.task_table)
        .ok_or_else(|| format!("SCHEMA_MISMATCH: 找不到事项表“{}”", config.task_table))?;
    let required = [
        &config.local_id_field,
        &config.item_field,
        &config.subject_field,
        &config.project_field,
        &config.solution_field,
        &config.status_field,
        &config.date_field,
        &config.hours_field,
    ];
    let missing: Vec<_> = required
        .into_iter()
        .filter(|field| !task_columns.contains(field.as_str()))
        .cloned()
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "SCHEMA_MISMATCH: 事项表缺少字段：{}",
            missing.join("、")
        ));
    }
    let reimbursement_available = table_columns(&tables, &config.reimbursement_table).is_some();
    let warnings = if reimbursement_available {
        Vec::new()
    } else {
        vec![format!(
            "未找到报销表“{}”，事项同步仍可使用",
            config.reimbursement_table
        )]
    };
    Ok(ConnectionTestResult {
        base_name: client.base_name().to_string(),
        task_table: config.task_table.clone(),
        reimbursement_table: config.reimbursement_table.clone(),
        reimbursement_available,
        warnings,
    })
}

pub fn preview_task_sync(
    database: &Database,
    request: TaskSyncPreviewRequest,
) -> Result<TaskSyncPreview, String> {
    let connection = database.open()?;
    let (workspace_id, config) = load_config(&connection)?;
    let token = integration_secret("seatable", &workspace_id)?;
    let client = SeaTableClient::connect(&config, &token)?;
    preview_task_sync_with(database, request, &config, &client)
}

fn preview_task_sync_with(
    database: &Database,
    request: TaskSyncPreviewRequest,
    config: &SeaTableConfig,
    client: &impl SeaTableApi,
) -> Result<TaskSyncPreview, String> {
    let connection = database.open()?;
    let workspace_id: String = connection
        .query_row(
            "SELECT id FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let remote_rows = client.list_rows(&config.task_table, Some(&config.task_view))?;
    let subject_name: String = connection
        .query_row(
            "SELECT name FROM subjects WHERE id = ?1 AND workspace_id = ?2 AND deleted_at IS NULL",
            params![request.subject_id, workspace_id],
            |row| row.get(0),
        )
        .map_err(|_| "NOT_FOUND: 主体不存在".to_string())?;
    let selected: HashSet<_> = request.task_ids.iter().cloned().collect();
    let tasks = list_tasks(
        database,
        TaskListRequest {
            subject_id: Some(request.subject_id.clone()),
            include_completed: Some(true),
            planned_date: None,
            today_date: None,
            today_only: Some(false),
            daily_estimate_date: None,
            query: None,
        },
    )?
    .tasks;
    let mut items = Vec::new();
    for task in tasks {
        let task_id = task.id;
        if !selected.is_empty() && !selected.contains(&task_id) {
            continue;
        }
        let title = task.title;
        let row = json!({
            config.local_id_field.clone(): task_id,
            config.item_field.clone(): title,
            config.subject_field.clone(): subject_name,
            config.project_field.clone(): task.project_name.unwrap_or_else(|| "-".to_string()),
            config.solution_field.clone(): task.solution_name.unwrap_or_default(),
            config.status_field.clone(): if task.status == "done" { "DONE" } else { "TODO" },
            config.date_field.clone(): request.work_date,
            config.hours_field.clone(): task.total_minutes as f64 / 60.0,
        });
        let binding: Option<String> = connection
            .query_row(
                "SELECT external_id FROM external_bindings WHERE workspace_id = ?1 AND provider = 'seatable' AND entity_type = 'task' AND entity_id = ?2",
                params![workspace_id, task_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        let by_local_id = remote_rows.iter().find(|remote| {
            string_cell(remote, &config.local_id_field).as_deref() == Some(task_id.as_str())
        });
        let matched = binding
            .as_ref()
            .and_then(|id| {
                remote_rows
                    .iter()
                    .find(|remote| row_id(remote).as_deref() == Some(id))
            })
            .or(by_local_id);
        let duplicate = remote_rows.iter().any(|remote| {
            string_cell(remote, &config.item_field).as_deref() == Some(title.as_str())
                && string_cell(remote, &config.subject_field).as_deref()
                    == Some(subject_name.as_str())
                && string_cell(remote, &config.date_field).as_deref()
                    == Some(request.work_date.as_str())
        });
        let (action, reason, external_id) = if let Some(remote) = matched {
            let id = row_id(remote);
            if mapped_row_matches(remote, &row) {
                ("skip", "外部记录已是最新内容", id)
            } else {
                ("update", "已通过稳定事项 ID 匹配外部记录", id)
            }
        } else if duplicate {
            (
                "conflict",
                "发现同日期、主体和标题的未绑定记录，需要人工处理",
                None,
            )
        } else {
            ("create", "未找到外部绑定，将新增记录", None)
        };
        items.push(SyncPreviewItem {
            item_id: Uuid::now_v7().to_string(),
            task_id,
            title,
            action: action.to_string(),
            reason: reason.to_string(),
            external_id,
            row,
        });
    }
    let run_id = Uuid::now_v7().to_string();
    let preview = summarize_preview(
        run_id.clone(),
        config.task_table.clone(),
        request.work_date.clone(),
        subject_name,
        items,
    );
    let now = now_millis();
    connection
        .execute(
            "INSERT INTO sync_runs(id, workspace_id, provider, operation_type, state, request_json, preview_json, created_at)
             VALUES (?1, ?2, 'seatable', 'task_sync', 'preview', ?3, ?4, ?5)",
            params![
                run_id,
                workspace_id,
                serde_json::to_string(&request).map_err(|error| error.to_string())?,
                serde_json::to_string(&preview).map_err(|error| error.to_string())?,
                now
            ],
        )
        .map_err(|error| error.to_string())?;
    for item in &preview.items {
        connection
            .execute(
                "INSERT INTO sync_items(id, workspace_id, run_id, entity_type, entity_id, action, state, external_id, after_json)
                 VALUES (?1, ?2, ?3, 'task', ?4, ?5, 'pending', ?6, ?7)",
                params![item.item_id, workspace_id, run_id, item.task_id, item.action, item.external_id, item.row.to_string()],
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(preview)
}

fn execute_task_sync_with(
    database: &Database,
    request: SyncExecuteRequest,
    config: &SeaTableConfig,
    client: &impl SeaTableApi,
    cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str)>,
) -> Result<SyncExecutionResult, String> {
    let mut connection = database.open()?;
    let workspace_id: String = connection
        .query_row(
            "SELECT id FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let (run_state, preview_json, prior_success, prior_failed, prior_skipped): (
        String,
        String,
        i64,
        i64,
        i64,
    ) = connection
        .query_row(
            "SELECT state, preview_json, success_count, failed_count, skipped_count
             FROM sync_runs WHERE id = ?1 AND workspace_id = ?2 AND provider = 'seatable' AND operation_type = 'task_sync'",
            params![request.run_id, workspace_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .map_err(|_| "NOT_FOUND: SeaTable 同步预览不存在".to_string())?;
    if matches!(run_state.as_str(), "succeeded" | "partial" | "failed") {
        return Ok(SyncExecutionResult {
            run_id: request.run_id,
            state: run_state,
            success_count: prior_success,
            failed_count: prior_failed,
            skipped_count: prior_skipped,
        });
    }
    let preview: TaskSyncPreview =
        serde_json::from_str(&preview_json).map_err(|error| error.to_string())?;
    if preview.conflict_count > 0 {
        return Err("SYNC_CONFLICT: 预览中存在可能重复的事项，请先处理冲突".to_string());
    }
    let now = now_millis();
    connection
        .execute(
            "UPDATE sync_runs SET state = 'running', started_at = ?1, error_code = NULL, error_message = NULL WHERE id = ?2",
            params![now, request.run_id],
        )
        .map_err(|error| error.to_string())?;
    let mut success = 0;
    let mut failed = 0;
    let mut skipped = 0;
    for item in preview.items {
        if item.action == "skip" {
            skipped += 1;
            connection
                .execute(
                    "UPDATE sync_items SET state = 'succeeded' WHERE id = ?1",
                    [&item.item_id],
                )
                .map_err(|error| error.to_string())?;
            continue;
        }
        let result = match item.action.as_str() {
            "create" => client.create_row(&config.task_table, &item.row).map(Some),
            "update" => item
                .external_id
                .as_deref()
                .ok_or_else(|| "SCHEMA_MISMATCH: 更新项缺少外部行 ID".to_string())
                .and_then(|id| {
                    client
                        .update_row(&config.task_table, id, &item.row)
                        .map(|_| Some(id.to_string()))
                }),
            _ => Err(format!("SYNC_CONFLICT: 不支持执行动作 {}", item.action)),
        };
        match result {
            Ok(external_id) => {
                success += 1;
                let external_id = external_id.or(item.external_id).unwrap_or_default();
                let transaction = connection
                    .transaction()
                    .map_err(|error| error.to_string())?;
                transaction.execute(
                    "UPDATE sync_items SET state = 'succeeded', external_id = ?1, error_code = NULL, error_message = NULL WHERE id = ?2",
                    params![external_id, item.item_id],
                ).map_err(|error| error.to_string())?;
                transaction.execute(
                    "INSERT INTO external_bindings(id, workspace_id, provider, entity_type, entity_id, external_id, last_synced_at)
                     VALUES (?1, ?2, 'seatable', 'task', ?3, ?4, ?5)
                     ON CONFLICT(workspace_id, provider, entity_type, entity_id) DO UPDATE SET external_id = excluded.external_id, last_synced_at = excluded.last_synced_at",
                    params![Uuid::now_v7().to_string(), workspace_id, item.task_id, external_id, now],
                ).map_err(|error| error.to_string())?;
                if let Some((state, operation_type)) = cloud_operation {
                    crate::cloud_sync::enqueue_entity_in_transaction(
                        &transaction,
                        state,
                        operation_type,
                        "external_binding",
                        Some(&item.task_id),
                        None,
                        None,
                    )?;
                }
                transaction.commit().map_err(|error| error.to_string())?;
            }
            Err(error) => {
                failed += 1;
                connection.execute(
                    "UPDATE sync_items SET state = 'failed', error_code = 'SEATABLE_WRITE_FAILED', error_message = ?1 WHERE id = ?2",
                    params![error, item.item_id],
                ).map_err(|db_error| db_error.to_string())?;
            }
        }
    }
    let state = if failed == 0 {
        "succeeded"
    } else if success > 0 || skipped > 0 {
        "partial"
    } else {
        "failed"
    };
    connection.execute(
        "UPDATE sync_runs SET state = ?1, success_count = ?2, failed_count = ?3, skipped_count = ?4, completed_at = ?5 WHERE id = ?6",
        params![state, success, failed, skipped, now_millis(), request.run_id],
    ).map_err(|error| error.to_string())?;
    Ok(SyncExecutionResult {
        run_id: request.run_id,
        state: state.to_string(),
        success_count: success,
        failed_count: failed,
        skipped_count: skipped,
    })
}

fn retry_failed_with(
    database: &Database,
    request: SyncExecuteRequest,
    config: &SeaTableConfig,
    client: &impl SeaTableApi,
    cloud_operation: Option<(&crate::supabase::StorageModeSnapshot, &str)>,
) -> Result<SyncExecutionResult, String> {
    let connection = database.open()?;
    let preview_json: String = connection
        .query_row(
            "SELECT preview_json FROM sync_runs WHERE id = ?1",
            [&request.run_id],
            |row| row.get(0),
        )
        .map_err(|_| "NOT_FOUND: 原同步记录不存在".to_string())?;
    let original: TaskSyncPreview =
        serde_json::from_str(&preview_json).map_err(|error| error.to_string())?;
    let mut failed_ids = HashSet::new();
    let mut statement = connection
        .prepare("SELECT id FROM sync_items WHERE run_id = ?1 AND state = 'failed'")
        .map_err(|error| error.to_string())?;
    for id in statement
        .query_map([&request.run_id], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?
    {
        failed_ids.insert(id.map_err(|error| error.to_string())?);
    }
    if failed_ids.is_empty() {
        return Err("VALIDATION_ERROR: 没有可重试的失败事项".to_string());
    }
    let new_id = Uuid::now_v7().to_string();
    let preview = summarize_preview(
        new_id.clone(),
        original.table_name,
        original.work_date,
        original.subject_name,
        original
            .items
            .into_iter()
            .filter(|item| failed_ids.contains(&item.item_id))
            .map(|mut item| {
                item.item_id = Uuid::now_v7().to_string();
                item
            })
            .collect(),
    );
    let workspace_id: String = connection
        .query_row(
            "SELECT workspace_id FROM sync_runs WHERE id = ?1",
            [&request.run_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    connection.execute(
        "INSERT INTO sync_runs(id, workspace_id, provider, operation_type, state, request_json, preview_json, created_at) VALUES (?1, ?2, 'seatable', 'task_sync', 'preview', '{}', ?3, ?4)",
        params![new_id, workspace_id, serde_json::to_string(&preview).map_err(|error| error.to_string())?, now_millis()],
    ).map_err(|error| error.to_string())?;
    for item in &preview.items {
        connection.execute(
            "INSERT INTO sync_items(id, workspace_id, run_id, entity_type, entity_id, action, state, external_id, after_json) VALUES (?1, ?2, ?3, 'task', ?4, ?5, 'pending', ?6, ?7)",
            params![item.item_id, workspace_id, new_id, item.task_id, item.action, item.external_id, item.row.to_string()],
        ).map_err(|error| error.to_string())?;
    }
    drop(statement);
    drop(connection);
    execute_task_sync_with(
        database,
        SyncExecuteRequest { run_id: new_id },
        config,
        client,
        cloud_operation,
    )
}

pub fn query_reimbursements(
    database: &Database,
    request: ReimbursementsRequest,
) -> Result<ReimbursementResult, String> {
    let connection = database.open()?;
    let (workspace_id, config) = load_config(&connection)?;
    let token = integration_secret("seatable", &workspace_id)?;
    let client = SeaTableClient::connect(&config, &token)?;
    query_reimbursements_with(database, request, &config, &client)
}

fn query_reimbursements_with(
    database: &Database,
    request: ReimbursementsRequest,
    config: &SeaTableConfig,
    client: &impl SeaTableApi,
) -> Result<ReimbursementResult, String> {
    let connection = database.open()?;
    let workspace_id: String = connection
        .query_row(
            "SELECT id FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let subject_name: String = connection
        .query_row(
            "SELECT name FROM subjects WHERE id = ?1 AND workspace_id = ?2 AND deleted_at IS NULL",
            params![request.subject_id, workspace_id],
            |row| row.get(0),
        )
        .map_err(|_| "NOT_FOUND: 主体不存在".to_string())?;
    let rows = client.list_rows(&config.reimbursement_table, None)?;
    let items = rows
        .into_iter()
        .filter_map(|row| {
            if string_cell(&row, &config.subject_field).as_deref() != Some(subject_name.as_str())
                || string_cell(&row, &config.status_field).as_deref()
                    != Some(config.reimbursement_pending_status.as_str())
            {
                return None;
            }
            let amount = number_cell(&row, &config.reimbursement_amount_field)?;
            Some(ReimbursementItem {
                description: string_cell(&row, &config.reimbursement_item_field)
                    .unwrap_or_else(|| "未命名报销项".to_string()),
                amount,
            })
        })
        .collect::<Vec<_>>();
    let result = ReimbursementResult {
        subject_id: request.subject_id.clone(),
        subject_name,
        period_start: request.period_start.clone(),
        period_end: request.period_end.clone(),
        total: items.iter().map(|item| item.amount).sum(),
        items,
    };
    connection.execute(
        "INSERT INTO sync_runs(id, workspace_id, provider, operation_type, state, request_json, preview_json, success_count, created_at, started_at, completed_at)
         VALUES (?1, ?2, 'seatable', 'reimbursements_query', 'succeeded', ?3, ?4, ?5, ?6, ?6, ?6)",
        params![Uuid::now_v7().to_string(), workspace_id, serde_json::to_string(&request).map_err(|error| error.to_string())?, serde_json::to_string(&result).map_err(|error| error.to_string())?, result.items.len() as i64, now_millis()],
    ).map_err(|error| error.to_string())?;
    Ok(result)
}

fn load_config(connection: &rusqlite::Connection) -> Result<(String, SeaTableConfig), String> {
    let workspace_id: String = connection
        .query_row(
            "SELECT id FROM workspaces WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let value: Option<(i64, String)> = connection.query_row(
        "SELECT enabled, config_json FROM integration_configs WHERE workspace_id = ?1 AND provider = 'seatable'",
        [&workspace_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional().map_err(|error| error.to_string())?;
    let (enabled, raw) =
        value.ok_or_else(|| "INTEGRATION_NOT_CONFIGURED: SeaTable 尚未配置".to_string())?;
    if enabled == 0 {
        return Err("INTEGRATION_NOT_CONFIGURED: SeaTable 集成未启用".to_string());
    }
    let config: SeaTableConfig = serde_json::from_str(&raw)
        .map_err(|error| format!("VALIDATION_ERROR: SeaTable 配置无效: {error}"))?;
    validate_config(&config)?;
    Ok((workspace_id, config))
}

fn validate_config(config: &SeaTableConfig) -> Result<(), String> {
    validate_server_url(&config.server_url)?;
    for (label, value) in [
        ("事项表", &config.task_table),
        ("本地事项 ID 字段", &config.local_id_field),
        ("事项字段", &config.item_field),
        ("主体字段", &config.subject_field),
        ("状态字段", &config.status_field),
    ] {
        if value.trim().is_empty() {
            return Err(format!("INTEGRATION_NOT_CONFIGURED: {label}不能为空"));
        }
    }
    Ok(())
}

fn validate_server_url(value: &str) -> Result<String, String> {
    let url = Url::parse(value.trim())
        .map_err(|_| "INTEGRATION_NOT_CONFIGURED: SeaTable 服务地址无效".to_string())?;
    if !matches!(url.scheme(), "https" | "http") || url.host_str().is_none() {
        return Err("INTEGRATION_NOT_CONFIGURED: SeaTable 服务地址必须是 HTTP(S) URL".to_string());
    }
    Ok(value.trim().trim_end_matches('/').to_string())
}

fn decode_response<T: serde::de::DeserializeOwned>(response: Response) -> Result<T, String> {
    let status = response.status();
    let body = response.text().map_err(network_error)?;
    if !status.is_success() {
        let code = if matches!(status.as_u16(), 401 | 403) {
            "AUTH_FAILED"
        } else if status.as_u16() == 404 {
            "SCHEMA_MISMATCH"
        } else {
            "NETWORK_ERROR"
        };
        return Err(format!(
            "{code}: SeaTable 请求失败（HTTP {}）：{}",
            status.as_u16(),
            truncate(&body, 240)
        ));
    }
    serde_json::from_str(&body)
        .map_err(|error| format!("SCHEMA_MISMATCH: SeaTable 返回格式无法识别: {error}"))
}

fn network_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        format!("NETWORK_ERROR: SeaTable 请求超时: {error}")
    } else {
        format!("NETWORK_ERROR: SeaTable 网络请求失败: {error}")
    }
}

fn table_columns(tables: &[Value], table_name: &str) -> Option<HashSet<String>> {
    let table = tables
        .iter()
        .find(|table| table.get("name").and_then(Value::as_str) == Some(table_name))?;
    Some(
        table
            .get("columns")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|column| {
                column
                    .get("name")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .collect(),
    )
}

fn summarize_preview(
    run_id: String,
    table_name: String,
    work_date: String,
    subject_name: String,
    items: Vec<SyncPreviewItem>,
) -> TaskSyncPreview {
    TaskSyncPreview {
        run_id,
        table_name,
        work_date,
        subject_name,
        create_count: items.iter().filter(|item| item.action == "create").count(),
        update_count: items.iter().filter(|item| item.action == "update").count(),
        skip_count: items.iter().filter(|item| item.action == "skip").count(),
        conflict_count: items
            .iter()
            .filter(|item| item.action == "conflict")
            .count(),
        items,
    }
}

fn mapped_row_matches(remote: &Value, expected: &Value) -> bool {
    expected.as_object().is_some_and(|expected| {
        expected.iter().all(|(key, value)| {
            remote
                .get(key)
                .is_some_and(|remote_value| values_equal(remote_value, value))
        })
    })
}

fn values_equal(left: &Value, right: &Value) -> bool {
    match (left.as_f64(), right.as_f64()) {
        (Some(left), Some(right)) => (left - right).abs() < 0.0001,
        _ => left.as_str().unwrap_or_default().trim() == right.as_str().unwrap_or_default().trim(),
    }
}

fn row_id(row: &Value) -> Option<String> {
    ["_id", "row_id", "id"]
        .into_iter()
        .find_map(|key| row.get(key).and_then(Value::as_str).map(ToOwned::to_owned))
}

fn string_cell(row: &Value, field: &str) -> Option<String> {
    let value = row.get(field)?;
    value
        .as_str()
        .map(|value| value.trim().to_string())
        .or_else(|| {
            if value.is_null() {
                None
            } else {
                Some(value.to_string().trim_matches('"').to_string())
            }
        })
}

fn number_cell(row: &Value, field: &str) -> Option<f64> {
    row.get(field)?
        .as_f64()
        .or_else(|| row.get(field)?.as_str()?.replace(',', "").parse().ok())
}

fn truncate(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn default_task_table() -> String {
    "事项计划".to_string()
}
fn default_task_view() -> String {
    "默认视图".to_string()
}
fn default_reimbursement_table() -> String {
    "报销".to_string()
}
fn default_local_id_field() -> String {
    "本地事项ID".to_string()
}
fn default_item_field() -> String {
    "事项".to_string()
}
fn default_subject_field() -> String {
    "主体".to_string()
}
fn default_project_field() -> String {
    "项目".to_string()
}
fn default_solution_field() -> String {
    "方案".to_string()
}
fn default_status_field() -> String {
    "状态".to_string()
}
fn default_date_field() -> String {
    "日期".to_string()
}
fn default_hours_field() -> String {
    "用时(h)".to_string()
}
fn default_reimbursement_status() -> String {
    "待结算".to_string()
}
fn default_reimbursement_amount_field() -> String {
    "报销金额".to_string()
}
fn default_reimbursement_item_field() -> String {
    "报销项".to_string()
}

#[tauri::command]
pub fn seatable_connection_test(
    database: tauri::State<'_, Database>,
) -> Result<ConnectionTestResult, String> {
    test_connection(&database)
}

#[tauri::command]
pub fn seatable_task_sync_preview(
    database: tauri::State<'_, Database>,
    request: TaskSyncPreviewRequest,
) -> Result<TaskSyncPreview, String> {
    preview_task_sync(&database, request)
}

#[tauri::command]
pub fn seatable_task_sync_execute(
    database: tauri::State<'_, Database>,
    request: SyncExecuteRequest,
) -> Result<SyncExecutionResult, String> {
    let state = crate::supabase::storage_mode(&database)?;
    let connection = database.open()?;
    let (_, config) = load_config(&connection)?;
    let workspace_id: String = connection
        .query_row(
            "SELECT workspace_id FROM sync_runs WHERE id = ?1",
            [&request.run_id],
            |row| row.get(0),
        )
        .map_err(|_| "NOT_FOUND: SeaTable 同步预览不存在".to_string())?;
    let token = integration_secret("seatable", &workspace_id)?;
    let client = SeaTableClient::connect(&config, &token)?;
    drop(connection);
    let result = execute_task_sync_with(
        &database,
        request,
        &config,
        &client,
        Some((&state, "external_binding_upsert")),
    )?;
    crate::cloud_sync::flush_if_online(&database)?;
    Ok(result)
}

#[tauri::command]
pub fn seatable_sync_retry_failed(
    database: tauri::State<'_, Database>,
    request: SyncExecuteRequest,
) -> Result<SyncExecutionResult, String> {
    let state = crate::supabase::storage_mode(&database)?;
    let connection = database.open()?;
    let (_, config) = load_config(&connection)?;
    let workspace_id: String = connection
        .query_row(
            "SELECT workspace_id FROM sync_runs WHERE id = ?1",
            [&request.run_id],
            |row| row.get(0),
        )
        .map_err(|_| "NOT_FOUND: 原同步记录不存在".to_string())?;
    let token = integration_secret("seatable", &workspace_id)?;
    let client = SeaTableClient::connect(&config, &token)?;
    drop(connection);
    let result = retry_failed_with(
        &database,
        request,
        &config,
        &client,
        Some((&state, "external_binding_upsert")),
    )?;
    crate::cloud_sync::flush_if_online(&database)?;
    Ok(result)
}

#[tauri::command]
pub fn seatable_reimbursements_query(
    database: tauri::State<'_, Database>,
    request: ReimbursementsRequest,
) -> Result<ReimbursementResult, String> {
    query_reimbursements(&database, request)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;
    use crate::settings::{self, SettingsScope, SettingsUpdate};
    use tempfile::tempdir;

    struct MockSeaTable {
        rows: Mutex<HashMap<String, Vec<Value>>>,
        create_calls: Mutex<HashMap<String, usize>>,
        fail_create_once: Mutex<HashSet<String>>,
    }

    struct ErrorSeaTable(&'static str);

    impl SeaTableApi for ErrorSeaTable {
        fn base_name(&self) -> &str {
            ""
        }

        fn metadata(&self) -> Result<Value, String> {
            Err(self.0.to_string())
        }

        fn list_rows(&self, _table: &str, _view: Option<&str>) -> Result<Vec<Value>, String> {
            Err(self.0.to_string())
        }

        fn create_row(&self, _table: &str, _row: &Value) -> Result<String, String> {
            Err(self.0.to_string())
        }

        fn update_row(&self, _table: &str, _external_id: &str, _row: &Value) -> Result<(), String> {
            Err(self.0.to_string())
        }
    }

    impl MockSeaTable {
        fn new() -> Self {
            Self {
                rows: Mutex::new(HashMap::new()),
                create_calls: Mutex::new(HashMap::new()),
                fail_create_once: Mutex::new(HashSet::new()),
            }
        }

        fn with_rows(self, table: &str, rows: Vec<Value>) -> Self {
            self.rows.lock().unwrap().insert(table.to_string(), rows);
            self
        }
    }

    impl SeaTableApi for MockSeaTable {
        fn base_name(&self) -> &str {
            "测试 Base"
        }

        fn metadata(&self) -> Result<Value, String> {
            Ok(json!({"tables": [
                {"name": "事项计划", "columns": [
                    {"name": "本地事项ID"}, {"name": "事项"}, {"name": "主体"},
                    {"name": "项目"}, {"name": "方案"}, {"name": "状态"},
                    {"name": "日期"}, {"name": "用时(h)"}
                ]},
                {"name": "报销", "columns": [
                    {"name": "主体"}, {"name": "状态"}, {"name": "报销金额"}, {"name": "报销项"}
                ]}
            ]}))
        }

        fn list_rows(&self, table: &str, _view: Option<&str>) -> Result<Vec<Value>, String> {
            Ok(self
                .rows
                .lock()
                .unwrap()
                .get(table)
                .cloned()
                .unwrap_or_default())
        }

        fn create_row(&self, table: &str, row: &Value) -> Result<String, String> {
            let title = string_cell(row, "事项").unwrap_or_default();
            *self
                .create_calls
                .lock()
                .unwrap()
                .entry(title.clone())
                .or_default() += 1;
            if self.fail_create_once.lock().unwrap().remove(&title) {
                return Err("NETWORK_ERROR: 模拟单条写入失败".to_string());
            }
            let id = Uuid::now_v7().to_string();
            let mut stored = row.clone();
            stored
                .as_object_mut()
                .unwrap()
                .insert("_id".to_string(), Value::String(id.clone()));
            self.rows
                .lock()
                .unwrap()
                .entry(table.to_string())
                .or_default()
                .push(stored);
            Ok(id)
        }

        fn update_row(&self, table: &str, external_id: &str, row: &Value) -> Result<(), String> {
            let mut rows = self.rows.lock().unwrap();
            let stored = rows
                .entry(table.to_string())
                .or_default()
                .iter_mut()
                .find(|stored| row_id(stored).as_deref() == Some(external_id))
                .ok_or_else(|| "NOT_FOUND: 模拟行不存在".to_string())?;
            let id = row_id(stored).unwrap();
            *stored = row.clone();
            stored
                .as_object_mut()
                .unwrap()
                .insert("_id".to_string(), Value::String(id));
            Ok(())
        }
    }

    fn config() -> SeaTableConfig {
        serde_json::from_value(json!({"serverUrl": "https://example.invalid"})).unwrap()
    }

    fn database_and_tasks() -> (Database, String, Vec<String>) {
        let directory = tempdir().unwrap().keep();
        let database = Database::initialize_at(directory.join("test.sqlite3")).unwrap();
        let connection = database.open().unwrap();
        let subject_id: String = connection
            .query_row(
                "SELECT id FROM subjects ORDER BY sort_order LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let mut statement = connection
            .prepare("SELECT id FROM tasks WHERE subject_id = ?1 ORDER BY sort_order LIMIT 2")
            .unwrap();
        let task_ids = statement
            .query_map([&subject_id], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<String>, _>>()
            .unwrap();
        drop(statement);
        drop(connection);
        (database, subject_id, task_ids)
    }

    #[test]
    fn preview_summary_counts_all_actions() {
        let items = ["create", "update", "skip", "conflict"]
            .into_iter()
            .map(|action| SyncPreviewItem {
                item_id: Uuid::now_v7().to_string(),
                task_id: action.to_string(),
                title: action.to_string(),
                action: action.to_string(),
                reason: String::new(),
                external_id: None,
                row: json!({}),
            })
            .collect();
        let preview = summarize_preview(
            "run".to_string(),
            "事项计划".to_string(),
            "2026-09-24".to_string(),
            "默认".to_string(),
            items,
        );
        assert_eq!(
            (
                preview.create_count,
                preview.update_count,
                preview.skip_count,
                preview.conflict_count
            ),
            (1, 1, 1, 1)
        );
    }

    #[test]
    fn connection_test_surfaces_auth_and_network_failures() {
        let auth =
            test_connection_with(&config(), &ErrorSeaTable("AUTH_FAILED: Token 无效")).unwrap_err();
        let network = test_connection_with(
            &config(),
            &ErrorSeaTable("NETWORK_ERROR: SeaTable 网络不可用"),
        )
        .unwrap_err();
        assert!(auth.starts_with("AUTH_FAILED:"));
        assert!(network.starts_with("NETWORK_ERROR:"));
    }

    #[test]
    fn connection_test_requires_configuration() {
        let directory = tempdir().unwrap();
        let database = Database::initialize_at(directory.path().join("test.sqlite3")).unwrap();
        let error = test_connection(&database).unwrap_err();
        assert!(error.starts_with("INTEGRATION_NOT_CONFIGURED:"));
    }

    #[test]
    fn mapped_rows_compare_numbers_and_text() {
        assert!(mapped_row_matches(
            &json!({"事项": "测试", "用时(h)": 0.5}),
            &json!({"事项": "测试", "用时(h)": 0.5})
        ));
        assert!(!mapped_row_matches(
            &json!({"事项": "旧名称"}),
            &json!({"事项": "新名称"})
        ));
    }

    #[test]
    fn repeated_task_sync_is_idempotent_and_next_preview_skips() {
        let (database, subject_id, task_ids) = database_and_tasks();
        let client = MockSeaTable::new();
        let request = TaskSyncPreviewRequest {
            subject_id: subject_id.clone(),
            work_date: "2026-09-24".to_string(),
            task_ids: vec![task_ids[0].clone()],
        };
        let preview = preview_task_sync_with(&database, request, &config(), &client).unwrap();
        assert_eq!(preview.create_count, 1);
        let first = execute_task_sync_with(
            &database,
            SyncExecuteRequest {
                run_id: preview.run_id.clone(),
            },
            &config(),
            &client,
            None,
        )
        .unwrap();
        let repeated = execute_task_sync_with(
            &database,
            SyncExecuteRequest {
                run_id: preview.run_id,
            },
            &config(),
            &client,
            None,
        )
        .unwrap();
        assert_eq!(first.success_count, 1);
        assert_eq!(repeated.success_count, 1);
        assert_eq!(
            client.create_calls.lock().unwrap().values().sum::<usize>(),
            1
        );

        let next = preview_task_sync_with(
            &database,
            TaskSyncPreviewRequest {
                subject_id,
                work_date: "2026-09-24".to_string(),
                task_ids: vec![task_ids[0].clone()],
            },
            &config(),
            &client,
        )
        .unwrap();
        assert_eq!(next.skip_count, 1);
        assert_eq!(next.create_count, 0);
    }

    #[test]
    fn cloud_task_sync_persists_external_binding_outbox_transactionally() {
        let (database, subject_id, task_ids) = database_and_tasks();
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "cloud_workspace_id".to_string(),
                value: json!(Uuid::now_v7().to_string()),
            },
        )
        .unwrap();
        settings::update_setting(
            &database,
            SettingsUpdate {
                scope: SettingsScope::Device,
                key: "storage_mode".to_string(),
                value: json!("cloud"),
            },
        )
        .unwrap();
        let state = crate::supabase::storage_mode(&database).unwrap();
        let client = MockSeaTable::new();
        let preview = preview_task_sync_with(
            &database,
            TaskSyncPreviewRequest {
                subject_id,
                work_date: "2026-09-24".to_string(),
                task_ids: vec![task_ids[0].clone()],
            },
            &config(),
            &client,
        )
        .unwrap();

        let result = execute_task_sync_with(
            &database,
            SyncExecuteRequest {
                run_id: preview.run_id,
            },
            &config(),
            &client,
            Some((&state, "external_binding_upsert")),
        )
        .unwrap();

        let connection = database.open().unwrap();
        let external_id: String = connection
            .query_row(
                "SELECT external_id FROM external_bindings WHERE provider = 'seatable' AND entity_type = 'task' AND entity_id = ?1",
                [&task_ids[0]],
                |row| row.get(0),
            )
            .unwrap();
        let (operation_type, entity_type, payload_json): (String, String, String) = connection
            .query_row(
                "SELECT operation_type, entity_type, payload_json FROM sync_outbox WHERE entity_id = ?1",
                [&task_ids[0]],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let payload: Value = serde_json::from_str(&payload_json).unwrap();

        assert_eq!(result.success_count, 1);
        assert!(!external_id.is_empty());
        assert_eq!(operation_type, "external_binding_upsert");
        assert_eq!(entity_type, "external_binding");
        assert_eq!(payload["provider"], "seatable");
        assert_eq!(payload["entity_id"], task_ids[0]);
        assert_eq!(payload["external_id"], external_id);
    }

    #[test]
    fn retry_only_replays_failed_sync_items() {
        let (database, subject_id, task_ids) = database_and_tasks();
        let connection = database.open().unwrap();
        let failing_title: String = connection
            .query_row(
                "SELECT title FROM tasks WHERE id = ?1",
                [&task_ids[1]],
                |row| row.get(0),
            )
            .unwrap();
        drop(connection);
        let client = MockSeaTable::new();
        client
            .fail_create_once
            .lock()
            .unwrap()
            .insert(failing_title.clone());
        let preview = preview_task_sync_with(
            &database,
            TaskSyncPreviewRequest {
                subject_id,
                work_date: "2026-09-24".to_string(),
                task_ids: task_ids.clone(),
            },
            &config(),
            &client,
        )
        .unwrap();
        let first = execute_task_sync_with(
            &database,
            SyncExecuteRequest {
                run_id: preview.run_id.clone(),
            },
            &config(),
            &client,
            None,
        )
        .unwrap();
        assert_eq!((first.success_count, first.failed_count), (1, 1));
        let retried = retry_failed_with(
            &database,
            SyncExecuteRequest {
                run_id: preview.run_id,
            },
            &config(),
            &client,
            None,
        )
        .unwrap();
        assert_eq!((retried.success_count, retried.failed_count), (1, 0));
        let calls = client.create_calls.lock().unwrap();
        assert_eq!(calls.get(&failing_title), Some(&2));
        assert_eq!(calls.values().sum::<usize>(), 3);
    }

    #[test]
    fn reimbursement_query_filters_subject_and_pending_status() {
        let (database, subject_id, _) = database_and_tasks();
        let subject_name: String = database
            .open()
            .unwrap()
            .query_row(
                "SELECT name FROM subjects WHERE id = ?1",
                [&subject_id],
                |row| row.get(0),
            )
            .unwrap();
        let client = MockSeaTable::new().with_rows(
            "报销",
            vec![
                json!({"主体": subject_name, "状态": "待结算", "报销金额": 28.5, "报销项": "交通"}),
                json!({"主体": subject_name, "状态": "已结算", "报销金额": 50, "报销项": "已处理"}),
                json!({"主体": "其他", "状态": "待结算", "报销金额": 100, "报销项": "其他主体"}),
            ],
        );
        let result = query_reimbursements_with(
            &database,
            ReimbursementsRequest {
                subject_id,
                period_start: "2026-09-21".to_string(),
                period_end: "2026-09-27".to_string(),
            },
            &config(),
            &client,
        )
        .unwrap();
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.total, 28.5);
    }
}
