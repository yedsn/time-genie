-- Some databases recorded migration 10 before its legacy index cleanup was
-- added. Remove that stale workspace-wide constraint so active unassigned
-- sessions remain unique per workspace and work date.
DROP INDEX IF EXISTS uq_unassigned_sessions_active;

CREATE UNIQUE INDEX IF NOT EXISTS uq_unassigned_sessions_active_date
    ON unassigned_sessions(workspace_id, work_date)
    WHERE state IN ('collecting', 'awaiting_resolution');
