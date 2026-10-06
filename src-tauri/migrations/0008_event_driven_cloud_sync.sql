CREATE TABLE cloud_deferred_entities (
    workspace_id TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    observed_version INTEGER NOT NULL DEFAULT 0,
    observed_change_seq INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, entity_type, entity_id)
);

CREATE INDEX idx_cloud_deferred_entities_seq
    ON cloud_deferred_entities(workspace_id, observed_change_seq);
