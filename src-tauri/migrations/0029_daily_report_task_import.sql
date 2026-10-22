ALTER TABLE actual_time_adjustments RENAME TO actual_time_adjustments_legacy;

CREATE TABLE actual_time_adjustments (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    subject_id TEXT NOT NULL REFERENCES subjects(id),
    task_id TEXT NOT NULL REFERENCES tasks(id),
    work_date TEXT NOT NULL,
    delta_minutes INTEGER NOT NULL CHECK (delta_minutes <> 0),
    baseline_minutes INTEGER NOT NULL CHECK (baseline_minutes >= 0),
    target_minutes INTEGER NOT NULL CHECK (target_minutes >= 0 AND target_minutes <= 1440),
    note TEXT,
    source_type TEXT NOT NULL DEFAULT 'quick_actual_adjustment'
        CHECK (source_type IN ('quick_actual_adjustment', 'daily_report_import')),
    batch_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    created_by_device_id TEXT REFERENCES devices(id),
    updated_by_device_id TEXT REFERENCES devices(id),
    deleted_at INTEGER
);

INSERT INTO actual_time_adjustments(
    id, workspace_id, subject_id, task_id, work_date, delta_minutes, baseline_minutes,
    target_minutes, note, source_type, batch_id, created_at, updated_at, version,
    created_by_device_id, updated_by_device_id, deleted_at
)
SELECT id, workspace_id, subject_id, task_id, work_date, delta_minutes, baseline_minutes,
       target_minutes, note, source_type, batch_id, created_at, updated_at, version,
       created_by_device_id, updated_by_device_id, deleted_at
FROM actual_time_adjustments_legacy;

DROP TABLE actual_time_adjustments_legacy;

CREATE INDEX idx_actual_time_adjustments_task_date
    ON actual_time_adjustments(workspace_id, task_id, work_date, deleted_at);
CREATE INDEX idx_actual_time_adjustments_date
    ON actual_time_adjustments(workspace_id, work_date, created_at, deleted_at);
CREATE INDEX idx_actual_time_adjustments_batch
    ON actual_time_adjustments(workspace_id, batch_id);

CREATE TABLE daily_report_import_batches (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    device_id TEXT NOT NULL REFERENCES devices(id),
    subject_id TEXT NOT NULL REFERENCES subjects(id),
    source_date TEXT NOT NULL,
    target_kind TEXT NOT NULL CHECK (target_kind IN ('backlog', 'day_instance')),
    source_path TEXT NOT NULL,
    source_path_hash TEXT NOT NULL,
    source_mtime INTEGER NOT NULL,
    source_content_hash TEXT NOT NULL,
    raw_markdown TEXT NOT NULL,
    warnings_json TEXT NOT NULL DEFAULT '[]',
    state TEXT NOT NULL DEFAULT 'preview' CHECK (state IN ('preview', 'confirmed', 'failed')),
    confirm_client_request_id TEXT,
    result_json TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    version INTEGER NOT NULL DEFAULT 1
);
CREATE UNIQUE INDEX uq_daily_report_import_request
    ON daily_report_import_batches(workspace_id, confirm_client_request_id)
    WHERE confirm_client_request_id IS NOT NULL;

CREATE TABLE daily_report_import_items (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    batch_id TEXT NOT NULL REFERENCES daily_report_import_batches(id) ON DELETE CASCADE,
    parent_item_id TEXT REFERENCES daily_report_import_items(id),
    title TEXT NOT NULL,
    item_status TEXT NOT NULL CHECK (item_status IN ('done', 'open')),
    estimate_minutes INTEGER,
    raw_actual_minutes INTEGER,
    import_actual_minutes INTEGER,
    sort_order INTEGER NOT NULL,
    source_line_no INTEGER NOT NULL,
    source_text TEXT NOT NULL,
    source_fingerprint TEXT NOT NULL,
    warning TEXT,
    already_imported INTEGER NOT NULL DEFAULT 0,
    confirmed_task_id TEXT REFERENCES tasks(id)
);
CREATE INDEX idx_daily_report_import_items_batch
    ON daily_report_import_items(batch_id, source_line_no, sort_order);

CREATE TABLE daily_report_import_sources (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    subject_id TEXT NOT NULL REFERENCES subjects(id),
    source_device_id TEXT,
    source_path_hash TEXT NOT NULL,
    source_date TEXT NOT NULL,
    target_kind TEXT NOT NULL CHECK (target_kind IN ('backlog', 'day_instance')),
    source_fingerprint TEXT NOT NULL,
    task_id TEXT NOT NULL REFERENCES tasks(id),
    batch_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE(workspace_id, subject_id, source_device_id, source_path_hash, source_date, target_kind, source_fingerprint)
);
CREATE INDEX idx_daily_report_import_sources_task
    ON daily_report_import_sources(workspace_id, task_id);
