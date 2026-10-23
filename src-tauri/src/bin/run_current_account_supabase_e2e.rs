#[cfg(windows)]
fn main() {
    use rusqlite::Connection;
    use serde_json::Value;
    use std::path::PathBuf;
    use std::process::Command;

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
    let entry = keyring::Entry::new_with_target(
        "com.timegenie.desktop:supabase:session",
        "timegenie",
        "supabase:session",
    )
    .expect("无法访问 TimeGenie Windows 凭据项");
    let session_json = entry
        .get_password()
        .expect("当前账号没有可用的 TimeGenie Supabase 保存会话");
    let session: Value =
        serde_json::from_str(&session_json).expect("当前 TimeGenie Supabase 保存会话不是有效 JSON");
    for field in ["userId", "accessToken", "refreshToken"] {
        assert!(
            session
                .get(field)
                .and_then(Value::as_str)
                .is_some_and(|value| !value.is_empty()),
            "当前 TimeGenie Supabase 保存会话缺少 {field}"
        );
    }
    let expires_at = session
        .get("expiresAt")
        .and_then(Value::as_i64)
        .expect("当前 TimeGenie Supabase 保存会话缺少 expiresAt");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("系统时间早于 Unix epoch")
        .as_secs() as i64;
    assert!(
        expires_at > now + 15 * 60,
        "当前 TimeGenie Supabase 登录有效期不足 15 分钟，请先在应用中重新登录"
    );
    if std::env::args().any(|argument| argument == "--check-only") {
        println!(
            "CURRENT_SESSION_READY expires_in_seconds={}",
            expires_at - now
        );
        return;
    }

    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("找不到 TimeGenie 仓库根目录")
        .to_path_buf();
    let status = Command::new("node")
        .arg("scripts/run-supabase-e2e.mjs")
        .current_dir(repo_root)
        .env("TG_SUPABASE_URL", project_url)
        .env("TG_SUPABASE_ANON_KEY", anon_key)
        .env("TG_SUPABASE_SESSION_JSON", session_json)
        .env("TG_SUPABASE_E2E_ALLOW_RESET", "1")
        .status()
        .expect("无法启动 Supabase 双设备端到端测试");
    std::process::exit(status.code().unwrap_or(1));
}

#[cfg(not(windows))]
fn main() {
    panic!("当前账号 Supabase E2E 启动器只用于 Windows TimeGenie 安装");
}
