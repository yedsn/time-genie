ALTER TABLE unassigned_sessions ADD COLUMN shared_source TEXT NOT NULL DEFAULT 'legacy_local'
    CHECK (shared_source IN ('legacy_local', 'local', 'cloud'));
ALTER TABLE unassigned_sessions ADD COLUMN predecessor_session_id TEXT;
ALTER TABLE unassigned_sessions ADD COLUMN migration_state TEXT NOT NULL DEFAULT 'ready'
    CHECK (migration_state IN ('ready', 'candidate', 'adopted', 'superseded'));
ALTER TABLE unassigned_sessions ADD COLUMN resolution_operation_id TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS uq_unassigned_resolution_operation
    ON unassigned_sessions(workspace_id, resolution_operation_id)
    WHERE resolution_operation_id IS NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS uq_unassigned_next_session
    ON unassigned_sessions(workspace_id, predecessor_session_id)
    WHERE predecessor_session_id IS NOT NULL;

-- Version 7 intentionally made these rows device-local. Version 9 restores the
-- entity as a cloud aggregate without touching historical terminal sessions,
-- generated time entries, or unrelated outbox work. Existing active rows are
-- candidates until the first cloud convergence chooses one authoritative root.
UPDATE unassigned_sessions
SET migration_state = 'candidate',
    shared_source = 'legacy_local'
WHERE state IN ('collecting', 'awaiting_resolution');

UPDATE unassigned_sessions
SET shared_source = 'local'
WHERE state IN ('resolved', 'discarded');
