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
    source_type TEXT NOT NULL DEFAULT 'quick_actual_adjustment' CHECK (source_type = 'quick_actual_adjustment'),
    batch_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    created_by_device_id TEXT REFERENCES devices(id),
    updated_by_device_id TEXT REFERENCES devices(id),
    deleted_at INTEGER
);
CREATE INDEX idx_actual_time_adjustments_task_date
    ON actual_time_adjustments(workspace_id, task_id, work_date, deleted_at);
CREATE INDEX idx_actual_time_adjustments_date
    ON actual_time_adjustments(workspace_id, work_date, created_at, deleted_at);
CREATE INDEX idx_actual_time_adjustments_batch
    ON actual_time_adjustments(workspace_id, batch_id);

CREATE TABLE actual_adjustment_batches (
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    client_request_id TEXT NOT NULL,
    batch_id TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    result_json TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, client_request_id)
);
CREATE UNIQUE INDEX uq_actual_adjustment_batches_batch
    ON actual_adjustment_batches(workspace_id, batch_id);
