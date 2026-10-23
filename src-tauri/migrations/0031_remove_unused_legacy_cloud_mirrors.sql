CREATE TABLE legacy_sync_cleanup_diagnostics (
    table_name TEXT PRIMARY KEY,
    row_count INTEGER NOT NULL,
    recorded_at INTEGER NOT NULL,
    cleanup_reason TEXT NOT NULL
);

INSERT INTO legacy_sync_cleanup_diagnostics(table_name, row_count, recorded_at, cleanup_reason)
SELECT 'tracking_leases', COUNT(*), CAST(strftime('%s', 'now') AS INTEGER) * 1000,
       'direct_v2 不再在 SQLite 保存云端计时租约镜像'
FROM tracking_leases;

INSERT INTO legacy_sync_cleanup_diagnostics(table_name, row_count, recorded_at, cleanup_reason)
SELECT 'workspace_changes', COUNT(*), CAST(strftime('%s', 'now') AS INTEGER) * 1000,
       'direct_v2 Realtime 只传递失效通知，不再落地云端变更实体'
FROM workspace_changes;

DROP INDEX IF EXISTS idx_workspace_changes_pull;
DROP TABLE tracking_leases;
DROP TABLE workspace_changes;
