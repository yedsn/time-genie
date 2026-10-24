#[cfg(windows)]
fn main() {
    use rusqlite::Connection;
    use serde_json::{json, Value};
    use std::io::Write;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};
    use uuid::Uuid;

    let app_data = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .expect("找不到 Windows APPDATA");
    let database_path = app_data
        .join("com.timegenie.desktop")
        .join("timegenie.sqlite3");
    let connection =
        Connection::open_with_flags(&database_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap_or_else(|error| panic!("无法只读打开 TimeGenie 配置数据库: {error}"));
    let setting = |key: &str| -> String {
        connection
            .query_row(
                "SELECT json_extract(value_json, '$') FROM device_settings WHERE key=?1",
                [key],
                |row| row.get(0),
            )
            .unwrap_or_else(|error| panic!("TimeGenie 缺少 {key} 配置: {error}"))
    };
    let project_url = setting("supabase_project_url");
    let anon_key = setting("supabase_anon_key");
    let suffix = Uuid::now_v7().simple().to_string();
    let email = format!("timegenie-e2e-{suffix}@example.invalid");
    let password = format!("TgE2e!{}", Uuid::now_v7().simple());
    let create_body = json!({
        "email": email,
        "password": password,
        "email_confirm": true
    })
    .to_string();
    let admin_create = r#"set -eu
cd /www/wwwroot/supabase
key=$(sed -n 's/^SERVICE_ROLE_KEY=//p' .env | head -n 1)
test -n "$key"
curl -fsS -X POST http://127.0.0.1:8000/auth/v1/admin/users \
  -H "apikey: $key" \
  -H "Authorization: Bearer $key" \
  -H "Content-Type: application/json" \
  --data-binary @-"#;
    let mut create = Command::new("ssh")
        .args(["-p", "22", "root@10.0.7.33", admin_create])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("无法启动临时 Supabase 测试账号创建命令");
    create
        .stdin
        .take()
        .expect("无法写入临时账号创建请求")
        .write_all(create_body.as_bytes())
        .expect("无法发送临时账号创建请求");
    let created = create.wait_with_output().expect("无法等待临时账号创建结果");
    assert!(created.status.success(), "创建临时 Supabase 测试账号失败");
    let created: Value =
        serde_json::from_slice(&created.stdout).expect("临时 Supabase 测试账号创建响应无效");
    let user_id = created
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .expect("临时 Supabase 测试账号响应缺少用户标识")
        .to_string();
    Uuid::parse_str(&user_id).expect("临时 Supabase 测试账号用户标识不是 UUID");

    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("找不到 TimeGenie 仓库根目录")
        .to_path_buf();
    let test_status = Command::new("node")
        .arg("scripts/run-supabase-e2e.mjs")
        .current_dir(repo_root)
        .env("TG_SUPABASE_URL", project_url)
        .env("TG_SUPABASE_ANON_KEY", anon_key)
        .env("TG_SUPABASE_EMAIL", &email)
        .env("TG_SUPABASE_PASSWORD", &password)
        .env("TG_SUPABASE_E2E_ALLOW_RESET", "1")
        .env("TG_SUPABASE_E2E_PERFORMANCE_ONLY", "1")
        .status();

    let delete_script = format!(
        r#"set -eu
cd /www/wwwroot/supabase
docker exec supabase-db psql -X -U supabase_admin -d postgres -v ON_ERROR_STOP=1 \
  -c "delete from timegenie.workspace_restore_locks where workspace_id in (select id from timegenie.workspaces where owner_user_id='{user_id}'::uuid); delete from timegenie.workspaces where owner_user_id='{user_id}'::uuid;" >/dev/null
key=$(sed -n 's/^SERVICE_ROLE_KEY=//p' .env | head -n 1)
test -n "$key"
curl -fsS -X DELETE http://127.0.0.1:8000/auth/v1/admin/users/{user_id} \
  -H "apikey: $key" \
  -H "Authorization: Bearer $key" >/dev/null"#
    );
    let deleted = Command::new("ssh")
        .args(["-p", "22", "root@10.0.7.33", &delete_script])
        .status()
        .expect("无法启动临时 Supabase 测试账号清理命令");
    assert!(deleted.success(), "临时 Supabase 测试账号清理失败");

    match test_status {
        Ok(status) => std::process::exit(status.code().unwrap_or(1)),
        Err(error) => panic!("无法启动 Supabase 双设备端到端测试: {error}"),
    }
}

#[cfg(not(windows))]
fn main() {
    panic!("临时账号 Supabase E2E 启动器只用于 Windows TimeGenie 安装");
}
