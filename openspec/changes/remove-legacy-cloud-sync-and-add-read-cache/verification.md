# 验证记录

- `cargo test --manifest-path src-tauri/Cargo.toml`：390 passed，0 failed，1 ignored；新增的持久缓存优先返回、后台权威替换和缓存不可写容错测试均通过。
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`：通过。
- `npm run build`：通过。
- `npm run test:supabase:schema`：通过，输出 `Supabase schema integration verification passed`；最终结构不存在 `workspaces.storage_protocol`，旧 patch、增量拉取和协议升级 RPC 均不存在。
- `npm run test:storage-boundaries`：通过，157 个命令、42 张 SQLite 表边界验证成功。
- `npm run test:direct-cloud-command-boundaries`：通过，97 个业务命令验证成功。
- `npm run test:direct-cloud-realtime-boundary`、`npm run test:direct-cloud-storage-ui`、`npm run test:direct-cloud-interaction-performance`：通过。
- `openspec validate remove-legacy-cloud-sync-and-add-read-cache --strict`：通过。
- `npm run test:supabase:e2e:current-session`：真实 Supabase 双设备直连 E2E 通过，覆盖 CRUD、并发拒绝、幂等重试、Realtime 和快照恢复。测试前已备份当前账号工作区；测试后已恢复原工作区及事项关联数据，并核对事项总数 138、有效事项 73、主体 2、计时记录 272、未归属记录 336。

服务器仍保留测试前备份：结构备份 `/www/wwwroot/supabase/backups/timegenie-before-direct-final-20261024-211712-schema.sql`，当前账号行级备份 schema `timegenie_user_backup_20261024_211712`。
