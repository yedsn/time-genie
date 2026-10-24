# SQLite 迁移与旧同步数据退役

TimeGenie 的本地数据库使用按版本顺序执行的 SQLite migration。已经发布的 migration 必须继续保留，不能因为当前代码不再使用旧同步表就删除或改写历史文件；旧安装需要先按原顺序升级，最后再由清理 migration 收敛到当前结构。

## 最终清理 migration

`0032_remove_legacy_sync_runtime.sql` 是旧同步运行时的最终清理 migration。它执行两类操作：

- 将仍有独立本机用途的数据迁到职责明确的新表：`processed_operations` 迁到 `local_request_results`，`sync_runs` 迁到 `integration_runs`，`sync_items` 迁到 `integration_run_items`。这些表属于设备级请求结果和集成运行记录，不是云端业务副本，也不参与持续同步。
- 删除 `sync_outbox`、`local_sync_state`、`cloud_deferred_entities`、`legacy_abandoned_outbox`、`processed_operations`、`sync_items`、`sync_runs` 及关联索引。主体、事项、计时、未归属时间、报告、共享设置等本地业务表不会被删除。

全新数据库也会执行完整 migration 链，再执行 migration 32 删除过渡结构。后续只有建立新的数据库基线时才可以压缩历史 migration；在此之前，历史文件是旧数据库可升级性的组成部分。

## 升级前救援备份

应用在执行 migration 32 前检查旧 `sync_outbox`。如果仍有等待、发送中、失败或冲突操作，会先生成一次脱敏救援备份，然后继续升级，不会抛出 `LEGACY_QUEUE_BLOCKED`，也不会尝试把旧操作上传到 Supabase。

救援包默认保存在应用数据目录的 `backups` 子目录，文件名格式为：

```text
timegenie-before-legacy-sync-retirement-YYYYMMDD-HHMMSS.timegenie-backup
```

应用会在设备设置中记录救援包路径，避免同一旧队列重复导出。救援包包含可恢复的本地业务快照和脱敏诊断摘要，不包含访问令牌、刷新令牌、认证头、设备凭据或可重放的原始队列 payload。它只用于人工核对或恢复到独立本地空间，不会自动合并到 Supabase。

## 升级后的存储边界

- 本地模式继续以主 SQLite 的业务表为唯一权威。
- 云端模式只以 Supabase 为业务权威；主 SQLite 只保存设备元数据、本机备份历史、集成运行记录等本机状态。
- 云端只读缓存使用独立的 `cloud-read-cache.sqlite3`，可以安全清除，不参与 migration 32，也不会恢复任何旧同步表。

恢复普通 `.timegenie-backup` 时，应用只重建目录允许的业务实体，不会重建旧同步运行时表。
