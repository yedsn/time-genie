# Supabase 旧同步协议退役

TimeGenie 云端模式现在只使用 Supabase 直连。Supabase 是唯一业务权威，客户端不会再上传 SQLite outbox、执行增量拉取或建立本地云端业务副本。

## 发布顺序

1. 先部署 `supabase/20261024d_timegenie_retire_legacy_sync.sql`。该 SQL 将既有 workspace 收敛到直连语义，并让旧 patch、增量拉取和协议升级 RPC 只返回 `LEGACY_SYNC_UNSUPPORTED`，不得修改业务数据。
2. 再发布本次桌面客户端。新客户端不读取 `sync_v1/direct_v2` 升级状态，云端模式始终直接查询和修改 Supabase。
3. 发布后运行 `npm run test:supabase:schema`，再使用专用可丢弃账号运行 `npm run test:supabase:e2e`。

## 旧客户端策略

旧客户端不再受支持。旧同步 RPC 可以在短暂诊断窗口保留为拒绝型 tombstone，但不能恢复真实写入权限；兼容窗口结束后可以直接删除这些函数、grant 和仅旧同步使用的协议字段。

## 回滚约束

客户端可以回滚到“无持久缓存的 Supabase 直连版本”，并安全删除 `cloud-read-cache.sqlite3`。服务端回滚只能恢复直连 RPC、快照和恢复能力，禁止重新授予 `cloud_apply_patch`、`cloud_incremental_pull`、`upgrade_storage_protocol` 等旧同步写权限，也禁止恢复 outbox 上传。

若用户需要找回升级前未上传的本地修改，应使用 SQLite 退役迁移生成的本机救援备份进行人工恢复，不得重新启用持续同步。
