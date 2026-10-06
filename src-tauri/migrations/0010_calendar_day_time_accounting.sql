ALTER TABLE time_entries ADD COLUMN timer_chain_id TEXT;
ALTER TABLE time_entries ADD COLUMN previous_entry_id TEXT REFERENCES time_entries(id);
ALTER TABLE time_entries ADD COLUMN split_boundary_at INTEGER;
ALTER TABLE time_entries ADD COLUMN last_continuous_at INTEGER;

UPDATE time_entries
SET timer_chain_id = id
WHERE source_type = 'timer' AND timer_chain_id IS NULL;

UPDATE time_entries
SET last_continuous_at = COALESCE(ended_at, updated_at, started_at)
WHERE source_type = 'timer' AND last_continuous_at IS NULL;

CREATE UNIQUE INDEX uq_time_entries_timer_chain_date
    ON time_entries(workspace_id, timer_chain_id, work_date)
    WHERE source_type = 'timer' AND timer_chain_id IS NOT NULL AND deleted_at IS NULL;
CREATE INDEX idx_time_entries_previous_entry ON time_entries(previous_entry_id);

ALTER TABLE unassigned_sessions ADD COLUMN last_continuous_at INTEGER;
UPDATE unassigned_sessions
SET last_continuous_at = COALESCE(last_ended_at, updated_at, first_started_at)
WHERE last_continuous_at IS NULL;

DROP INDEX IF EXISTS uq_unassigned_sessions_active;
CREATE UNIQUE INDEX uq_unassigned_sessions_active_date
    ON unassigned_sessions(workspace_id, work_date)
    WHERE state IN ('collecting', 'awaiting_resolution');

CREATE TABLE IF NOT EXISTS tracking_runtime_state (
    workspace_id TEXT PRIMARY KEY REFERENCES workspaces(id),
    last_heartbeat_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

UPDATE workspaces SET timezone = 'Asia/Shanghai' WHERE TRIM(COALESCE(timezone, '')) = '';
