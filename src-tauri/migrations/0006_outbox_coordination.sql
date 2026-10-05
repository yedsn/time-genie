ALTER TABLE sync_outbox ADD COLUMN payload_version INTEGER;
ALTER TABLE sync_outbox ADD COLUMN depends_on_operation_id TEXT;
ALTER TABLE sync_outbox ADD COLUMN coalesced_count INTEGER NOT NULL DEFAULT 1;

UPDATE sync_outbox
SET payload_version = CAST(json_extract(payload_json, '$.version') AS INTEGER)
WHERE json_valid(payload_json) AND json_type(payload_json, '$.version') = 'integer';

CREATE INDEX idx_sync_outbox_dependency
    ON sync_outbox(workspace_id, depends_on_operation_id, state);
