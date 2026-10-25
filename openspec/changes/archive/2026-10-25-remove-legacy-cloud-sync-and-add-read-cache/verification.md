# 验证记录

- `cargo test --manifest-path src-tauri/Cargo.toml`：393 passed，0 failed，1 ignored；持久缓存、后台权威替换、缓存不可写容错、工作区 epoch 自动恢复，以及强制刷新权威 epoch 和清理 workspace 缓存的测试均通过。
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`：通过。
- `npm run build`：通过。
- `npm run test:supabase:schema`：通过，输出 `Supabase schema integration verification passed`；最终结构不存在 `workspaces.storage_protocol`，旧 patch、增量拉取和协议升级 RPC 均不存在。
- `supabase/20261024e_timegenie_direct_cloud_readonly_queries.sql`：已部署到当前 Supabase。使用当前账号在 `BEGIN READ ONLY` 中调用 `direct_cloud_context` 和 `direct_cloud_task_list` 均通过，不再触发 `SELECT FOR SHARE` 错误；PostgREST schema cache 已成功刷新。
- `WORKSPACE_EPOCH_STALE` 自动恢复：查询与修改遇到本机 epoch 落后时，会先无副作用读取云端权威 epoch、清除旧只读缓存、更新本机 checkpoint，再自动重试一次；修改重试沿用同一 operation ID，避免重复副作用。当前设备 checkpoint 已从缺失时的默认值 1 修复为云端值 7。
- “强制刷新云端数据”：设置页与右下角入口会先调用 `direct_cloud_force_refresh_prepare`，忽略本机旧 epoch 获取服务端权威上下文，删除当前 workspace 持久缓存并更新 checkpoint；随后清空前端进程内数据，重新读取事项、计时、报告、未归属时间、时间历史、今日计划和今日概览。测试确认该准备操作不修改本地业务数据。
- `npm run test:storage-boundaries`：通过，157 个命令、42 张 SQLite 表边界验证成功。
- `npm run test:direct-cloud-command-boundaries`：通过，97 个业务命令验证成功。
- `npm run test:direct-cloud-realtime-boundary`、`npm run test:direct-cloud-storage-ui`、`npm run test:direct-cloud-interaction-performance`：通过。
- `openspec validate remove-legacy-cloud-sync-and-add-read-cache --strict`：通过。
- `npm run test:supabase:e2e:current-session`：真实 Supabase 双设备直连 E2E 通过，覆盖 CRUD、并发拒绝、幂等重试、Realtime 和快照恢复。测试前已备份当前账号工作区；测试后已恢复原工作区及事项关联数据，并核对事项总数 138、有效事项 73、主体 2、计时记录 272、未归属记录 336。

服务器仍保留测试前备份：结构备份 `/www/wwwroot/supabase/backups/timegenie-before-direct-final-20261024-211712-schema.sql`，当前账号行级备份 schema `timegenie_user_backup_20261024_211712`；只读查询修复前结构备份 `/www/wwwroot/supabase/backups/timegenie-before-readonly-query-fix-20261024-224743.sql`，SHA-256 为 `a4c189396202b57bdb0bab56ba2506c8d56fac0d5dc15c9dad7a2cac8c8dd047`。

当前设备 epoch 修复前的本机数据库备份为 `C:\Users\yedsn\AppData\Roaming\com.timegenie.desktop\timegenie-before-epoch-repair-20261024-233208.sqlite3`。最终复核：本机 cloud checkpoint epoch 为 7，云端 workspace epoch 为 7；事项总数 138、有效事项 73、主体 2，未改变当前账号事项数据。
