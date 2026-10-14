# 部署核对记录

## 部署前备份

生产部署前 MUST 先保存当前 schema 和受本变更影响的业务表。建议使用 Supabase CLI 或 `pg_dump` 备份：

- schema：`timegenie`
- 表：`time_entries`、`time_segments`、`time_allocations`、`sync_outbox`、`processed_operations`、`tracking_leases`
- 备份文件 MUST 带部署时间，并在独立位置确认可读后再执行 `supabase/20261014_timegenie_timer_sync_authority.sql`。

本次实现未直接部署生产；本地临时 PostgreSQL 会完整加载主 schema、重复应用补丁并运行集成测试。

## 部署后权限检查

部署后 MUST 运行以下检查：

- `authenticated` 可以执行两个 `timer_sync_context_get` 重载。
- `authenticated` 可以执行 `tracking_lease_acquire`、`tracking_lease_renew`、`tracking_lease_release` 和兼容旧客户端的 `unassigned_tick`。
- `authenticated` 可以经两个 `cloud_apply_patch` 重载提交计时操作。
- `authenticated`、`anon` 和 `public` 不能直接执行 `apply_calendar_time_entry`、旧版 apply helper 或 `timer_write_lease_context`；`anon` 和 `public` 不能执行 `unassigned_tick`。
- `anon` 不能执行计时 context 或租约 RPC。

这些权限已写入 `supabase/schema_integration_test.sql`，并由 `node scripts/verify-supabase-schema.mjs` 在临时 PostgreSQL 中验证。生产部署后应再次执行同等的 `has_function_privilege` 查询并保存结果。
