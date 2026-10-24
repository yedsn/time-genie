# 验证记录

- `cargo test --manifest-path src-tauri/Cargo.toml`：388 passed，0 failed，1 ignored。
- `npm run build`：通过。
- `npm run typecheck`：通过。
- `npm run test:supabase:schema`：通过，输出 `Supabase schema integration verification passed`。
- `npm run test:storage-boundaries`：通过，157 个命令、42 张 SQLite 表边界验证成功。
- `npm run test:direct-cloud-command-boundaries`：通过，97 个业务命令验证成功。
- `npm run test:direct-cloud-realtime-boundary`、`npm run test:direct-cloud-storage-ui`、`npm run test:direct-cloud-interaction-performance`：通过。

真实 Supabase 双设备 E2E 保持为 ignored。运行 `npm run test:supabase:e2e` 前必须准备专用可丢弃测试账号，并设置 `TG_SUPABASE_URL`、`TG_SUPABASE_ANON_KEY`、`TG_SUPABASE_E2E_ALLOW_RESET=1`，以及 `TG_SUPABASE_SESSION_JSON` 或 `TG_SUPABASE_EMAIL` / `TG_SUPABASE_PASSWORD`。该测试会清理测试账号工作空间。
