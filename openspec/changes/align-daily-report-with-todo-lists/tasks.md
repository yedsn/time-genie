# Tasks

## 1. 本地列表映射

- [x] 1.1 日报取今日实例树，保留未执行成员、独立标题层级及工时；以 Rust 回归测试验证范围、状态、顺序和无重复
- [x] 1.2 明日计划取全部进行中来源事项，日报重新生成刷新关联；以重复规则、增删今日成员和预览创建一致性测试验证

## 2. 云端对齐

- [x] 2.1 新增幂等迁移同步日报范围、实例渲染、进行中计划和重新生成，接入 schema；隔离 PostgreSQL 测试验证与本地相同的边界
- [x] 2.2 更新冲突的报告规范测试和界面说明，不改变周月报合集；运行报告相关回归及前端类型检查

## 3. 集成检查

- [x] 3.1 执行 Rust 报告测试、Supabase schema 集成测试、类型检查和 OpenSpec 严格校验，记录命令、结果和部署提示

## 验证结果（2026-11-03）

- `cargo test --offline --manifest-path src-tauri/Cargo.toml --lib reports::tests -- --test-threads=4`：36 项通过。
- `cargo test --offline --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=4`：最终全量 416 项通过，1 项真实 Supabase 端到端测试按既有要求跳过。
- `npm run test:supabase:schema`：隔离 PostgreSQL schema 集成通过，包含新增迁移重复执行、只读预览、今日未执行及独立标题、工时补充、进行中重复事项和重新生成幂等回归。
- `npm run typecheck`、`npm run build`：通过；构建仅有既有大分包提示。
- `npm run test:report-template-editor`、`npm run test:report-task-candidates`、`npm run test:report-pagination`：通过。
- `node scripts/verify-daily-report-import-ui.mjs`、`node scripts/verify-direct-cloud-command-boundaries.mjs`：通过，98 个业务命令边界检查通过。
- `openspec validate align-daily-report-with-todo-lists --strict`、`git diff --check`：通过。
- 未操作真实云端账户、未部署增量 SQL，未进行桌面手工交互验收。云端使用需部署 `supabase/20261103_timegenie_daily_report_todo_lists.sql`；已保存日报需主动重新生成才更新。
