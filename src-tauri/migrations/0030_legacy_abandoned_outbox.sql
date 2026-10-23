CREATE TABLE legacy_abandoned_outbox (
    operation_id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    device_id TEXT NOT NULL,
    operation_type TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    entity_id TEXT,
    base_version INTEGER,
    payload_json TEXT NOT NULL,
    state TEXT NOT NULL,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    last_attempt_at INTEGER,
    error_json TEXT,
    payload_version INTEGER,
    depends_on_operation_id TEXT,
    coalesced_count INTEGER NOT NULL DEFAULT 1,
    recovery_kind TEXT,
    recovery_context_json TEXT,
    recovery_updated_at INTEGER,
    workspace_epoch INTEGER NOT NULL DEFAULT 1,
    quarantine_batch_id TEXT NOT NULL,
    quarantine_reason TEXT NOT NULL,
    rescue_backup_checksum TEXT NOT NULL,
    quarantined_at INTEGER NOT NULL
);

CREATE INDEX idx_legacy_abandoned_outbox_workspace
    ON legacy_abandoned_outbox(workspace_id, quarantine_batch_id, quarantined_at);
