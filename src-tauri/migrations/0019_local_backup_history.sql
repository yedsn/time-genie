CREATE TABLE local_backup_history (
    id TEXT PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    file_name TEXT NOT NULL,
    created_at TEXT NOT NULL,
    recorded_at INTEGER NOT NULL,
    backup_kind TEXT NOT NULL,
    source_mode TEXT NOT NULL,
    cloud_consistent INTEGER NOT NULL CHECK(cloud_consistent IN (0, 1)),
    entity_count INTEGER NOT NULL DEFAULT 0,
    size_bytes INTEGER NOT NULL DEFAULT 0,
    checksum TEXT NOT NULL,
    origin TEXT NOT NULL CHECK(origin IN ('manual', 'safety'))
);

CREATE INDEX idx_local_backup_history_created
    ON local_backup_history(recorded_at DESC, id DESC);
