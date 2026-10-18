ALTER TABLE local_sync_state
    ADD COLUMN workspace_epoch INTEGER NOT NULL DEFAULT 1 CHECK(workspace_epoch >= 1);
ALTER TABLE local_sync_state
    ADD COLUMN epoch_blocked INTEGER NOT NULL DEFAULT 0 CHECK(epoch_blocked IN (0, 1));
ALTER TABLE local_sync_state ADD COLUMN epoch_blocked_reason TEXT;
ALTER TABLE local_sync_state ADD COLUMN epoch_blocked_at INTEGER;

ALTER TABLE sync_outbox
    ADD COLUMN workspace_epoch INTEGER NOT NULL DEFAULT 1 CHECK(workspace_epoch >= 1);

CREATE INDEX idx_sync_outbox_workspace_epoch_state
    ON sync_outbox(workspace_id, workspace_epoch, state, created_at);
CREATE INDEX idx_local_sync_state_epoch_blocked
    ON local_sync_state(epoch_blocked, workspace_id);
