# Verification

## 2026-10-21 本地验证

- `npm run typecheck`：通过。
- `npm run build`：通过。
- `npm run test:today-plan:local-acceptance`：通过。
- `npm run test:today-plan-ui`：通过。
- `npm run test:timer-task-dialog`：通过。
- `npm run test:execution-task-panel`：通过。
- `npm run test:today-plan-request`：通过。
- `npm run test:supabase:schema`：通过，包含本地临时 PostgreSQL 中的 Schema 重复执行与集成验证。
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`：通过。
- `cargo test --manifest-path src-tauri/Cargo.toml`：通过，451 passed、0 failed、2 ignored；两个 ignored 用例均要求真实 Supabase 专用测试账号与显式清理授权。首次与其他构建并行运行时，一个慢网上传时序用例偶发未观察到请求；该用例单独复跑通过，随后串行全量复跑通过。
- `openspec validate add-today-plan-tab-and-filter-execution-dialog --strict`：通过。
- `git diff --check`：通过，仅有工作区换行符提示。

本地隔离桌面走查已覆盖：从进行中加入、今日与来源双向改名/整体预计/今日预计/完成/实际/删除隔离、今日树拖动、配置弹框新增与排序、重复实例“仅修改今天 / 修改今后全部”、移出后再次加入新实例、执行已完成项、跨工作日候选隔离和未归属时间完整分配范围。走查使用临时 SQLite 与 WebView 目录，结束前已把工作空间时区恢复为 `Asia/Shanghai`。

## 真实 Supabase 双设备验证阻塞

任务 5.4 保持未完成。2026-10-21 再次检查时，以下环境变量均未配置：

- `TG_SUPABASE_URL`
- `TG_SUPABASE_ANON_KEY`
- `TG_SUPABASE_EMAIL`
- `TG_SUPABASE_PASSWORD`
- `TG_SUPABASE_E2E_ALLOW_RESET`

缺少前四项时无法登录专用测试项目；缺少 `TG_SUPABASE_E2E_ALLOW_RESET=true` 时，不具备清理一次性测试工作空间的明确授权。不得使用用户正式工作空间替代该破坏性验收。环境可用后运行 `npm run test:supabase:e2e`，并确认双窗口/双设备实例同步、并发加入、来源日期映射、离线恢复和快照还原全部通过，再勾选任务 5.4。
