CREATE TABLE local_request_results (
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    request_id TEXT NOT NULL,
    device_id TEXT NOT NULL REFERENCES devices(id),
    request_type TEXT NOT NULL,
    result_json TEXT NOT NULL,
    processed_at INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, request_id)
);

INSERT INTO local_request_results(
    workspace_id,request_id,device_id,request_type,result_json,processed_at
)
SELECT workspace_id,operation_id,device_id,operation_type,result_json,processed_at
FROM processed_operations;

CREATE TABLE integration_runs (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    provider TEXT NOT NULL,
    operation_type TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('preview', 'running', 'partial', 'succeeded', 'failed')),
    request_json TEXT NOT NULL,
    preview_json TEXT,
    success_count INTEGER NOT NULL DEFAULT 0,
    failed_count INTEGER NOT NULL DEFAULT 0,
    skipped_count INTEGER NOT NULL DEFAULT 0,
    error_code TEXT,
    error_message TEXT,
    created_at INTEGER NOT NULL,
    started_at INTEGER,
    completed_at INTEGER
);

INSERT INTO integration_runs SELECT * FROM sync_runs;

CREATE TABLE integration_run_items (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    run_id TEXT NOT NULL REFERENCES integration_runs(id),
    entity_type TEXT NOT NULL,
    entity_id TEXT,
    action TEXT NOT NULL CHECK (action IN ('create', 'update', 'skip', 'conflict', 'write_file')),
    state TEXT NOT NULL CHECK (state IN ('pending', 'succeeded', 'failed')),
    external_id TEXT,
    before_json TEXT,
    after_json TEXT,
    error_code TEXT,
    error_message TEXT
);

INSERT INTO integration_run_items SELECT * FROM sync_items;
CREATE INDEX idx_integration_run_items_retry
    ON integration_run_items(workspace_id, run_id, state);

DROP INDEX IF EXISTS idx_sync_outbox_state;
DROP INDEX IF EXISTS idx_sync_outbox_dependency;
DROP INDEX IF EXISTS idx_sync_outbox_timer_recovery;
DROP INDEX IF EXISTS idx_sync_outbox_workspace_epoch_state;
DROP INDEX IF EXISTS idx_local_sync_state_epoch_blocked;
DROP INDEX IF EXISTS idx_cloud_deferred_entities_seq;
DROP INDEX IF EXISTS idx_legacy_abandoned_outbox_workspace;
DROP INDEX IF EXISTS idx_sync_items_retry;

DROP TABLE IF EXISTS cloud_deferred_entities;
DROP TABLE IF EXISTS legacy_abandoned_outbox;
DROP TABLE IF EXISTS sync_outbox;
DROP TABLE IF EXISTS local_sync_state;
DROP TABLE IF EXISTS processed_operations;
DROP TABLE IF EXISTS sync_items;
DROP TABLE IF EXISTS sync_runs;
