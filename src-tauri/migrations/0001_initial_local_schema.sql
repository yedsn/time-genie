CREATE TABLE workspaces (
    id TEXT PRIMARY KEY,
    owner_user_id TEXT,
    name TEXT NOT NULL,
    timezone TEXT NOT NULL DEFAULT 'Asia/Shanghai',
    storage_mode TEXT NOT NULL CHECK (storage_mode IN ('local', 'cloud')),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    deleted_at INTEGER
);

CREATE TABLE devices (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    device_name TEXT NOT NULL,
    platform TEXT NOT NULL,
    app_version TEXT NOT NULL,
    last_seen_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    revoked_at INTEGER
);
CREATE INDEX idx_devices_workspace ON devices(workspace_id, revoked_at);

CREATE TABLE tracking_leases (
    workspace_id TEXT PRIMARY KEY REFERENCES workspaces(id),
    holder_device_id TEXT REFERENCES devices(id),
    lease_token TEXT,
    expires_at INTEGER,
    updated_at INTEGER NOT NULL,
    version INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE workspace_changes (
    change_seq INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    operation TEXT NOT NULL CHECK (operation IN ('insert', 'update', 'delete')),
    entity_version INTEGER NOT NULL,
    changed_by_device_id TEXT REFERENCES devices(id),
    changed_at INTEGER NOT NULL
);
CREATE INDEX idx_workspace_changes_pull ON workspace_changes(workspace_id, change_seq);

CREATE TABLE processed_operations (
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    operation_id TEXT NOT NULL,
    device_id TEXT NOT NULL REFERENCES devices(id),
    operation_type TEXT NOT NULL,
    result_json TEXT NOT NULL,
    processed_at INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, operation_id)
);

CREATE TABLE subjects (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    name TEXT NOT NULL CHECK (length(trim(name)) > 0),
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    created_by_device_id TEXT REFERENCES devices(id),
    updated_by_device_id TEXT REFERENCES devices(id),
    deleted_at INTEGER
);
CREATE UNIQUE INDEX uq_subjects_active_name ON subjects(workspace_id, lower(name)) WHERE deleted_at IS NULL;
CREATE INDEX idx_subjects_order ON subjects(workspace_id, sort_order);

CREATE TABLE work_days (
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    work_date TEXT NOT NULL,
    timezone TEXT NOT NULL,
    work_period_text TEXT,
    note TEXT,
    settled_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (workspace_id, work_date)
);

CREATE TABLE tasks (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    subject_id TEXT NOT NULL REFERENCES subjects(id),
    parent_id TEXT REFERENCES tasks(id),
    title TEXT NOT NULL CHECK (length(trim(title)) > 0),
    status TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'done')),
    planned_date TEXT,
    planned_time TEXT,
    estimate_minutes INTEGER CHECK (estimate_minutes IS NULL OR estimate_minutes > 0),
    note TEXT,
    project_name TEXT,
    solution_name TEXT,
    source_type TEXT NOT NULL CHECK (source_type IN ('manual', 'obsidian_import', 'timer_quick_create')),
    source_ref TEXT,
    sort_order INTEGER NOT NULL,
    completed_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    created_by_device_id TEXT REFERENCES devices(id),
    updated_by_device_id TEXT REFERENCES devices(id),
    deleted_at INTEGER,
    CHECK ((status = 'done' AND completed_at IS NOT NULL) OR status = 'open')
);
CREATE INDEX idx_tasks_subject_parent_order ON tasks(workspace_id, subject_id, parent_id, sort_order);
CREATE INDEX idx_tasks_planned_date ON tasks(workspace_id, subject_id, planned_date, status);
CREATE INDEX idx_tasks_completed_at ON tasks(workspace_id, subject_id, completed_at);
CREATE INDEX idx_tasks_source ON tasks(workspace_id, source_type, source_ref);

CREATE TABLE task_status_events (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    task_id TEXT NOT NULL REFERENCES tasks(id),
    status TEXT NOT NULL CHECK (status IN ('open', 'done')),
    occurred_at INTEGER NOT NULL,
    source_type TEXT NOT NULL CHECK (source_type IN ('user', 'import', 'migration')),
    created_at INTEGER NOT NULL
);
CREATE INDEX idx_task_status_events_task_time ON task_status_events(workspace_id, task_id, occurred_at);

CREATE TABLE time_entries (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    work_date TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('work', 'break')),
    source_type TEXT NOT NULL CHECK (source_type IN ('timer', 'manual', 'unassigned')),
    state TEXT NOT NULL CHECK (state IN ('running', 'paused', 'ended')),
    default_task_id TEXT REFERENCES tasks(id),
    label_snapshot TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    ended_at INTEGER,
    duration_seconds INTEGER NOT NULL DEFAULT 0 CHECK (duration_seconds >= 0),
    note TEXT,
    origin_unassigned_session_id TEXT UNIQUE,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    created_by_device_id TEXT REFERENCES devices(id),
    updated_by_device_id TEXT REFERENCES devices(id),
    deleted_at INTEGER,
    CHECK ((state = 'ended' AND ended_at IS NOT NULL) OR (state IN ('running', 'paused') AND ended_at IS NULL))
);
CREATE UNIQUE INDEX uq_time_entries_active ON time_entries(workspace_id) WHERE state IN ('running', 'paused') AND deleted_at IS NULL;
CREATE INDEX idx_time_entries_date ON time_entries(workspace_id, work_date, started_at);
CREATE INDEX idx_time_entries_default_task ON time_entries(workspace_id, default_task_id);

CREATE TABLE time_segments (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    entry_id TEXT NOT NULL REFERENCES time_entries(id),
    sequence_no INTEGER NOT NULL,
    started_at INTEGER NOT NULL,
    ended_at INTEGER,
    duration_seconds INTEGER NOT NULL DEFAULT 0 CHECK (duration_seconds >= 0),
    UNIQUE (entry_id, sequence_no)
);
CREATE UNIQUE INDEX uq_time_segments_open ON time_segments(entry_id) WHERE ended_at IS NULL;

CREATE TABLE time_allocations (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    entry_id TEXT NOT NULL REFERENCES time_entries(id),
    task_id TEXT NOT NULL REFERENCES tasks(id),
    minutes INTEGER NOT NULL CHECK (minutes > 0),
    note TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    UNIQUE (entry_id, task_id)
);
CREATE INDEX idx_time_allocations_task ON time_allocations(workspace_id, task_id);

CREATE TABLE unassigned_sessions (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    work_date TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('collecting', 'awaiting_resolution', 'resolved', 'discarded')),
    threshold_seconds INTEGER NOT NULL DEFAULT 300 CHECK (threshold_seconds > 0),
    duration_seconds INTEGER NOT NULL DEFAULT 0 CHECK (duration_seconds >= 0),
    first_started_at INTEGER NOT NULL,
    last_ended_at INTEGER,
    prompted_at INTEGER,
    resolution_type TEXT CHECK (resolution_type IS NULL OR resolution_type IN ('work', 'break', 'discard')),
    generated_entry_id TEXT UNIQUE REFERENCES time_entries(id),
    resolved_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    version INTEGER NOT NULL DEFAULT 1
);
CREATE UNIQUE INDEX uq_unassigned_sessions_active ON unassigned_sessions(workspace_id) WHERE state IN ('collecting', 'awaiting_resolution');

CREATE TABLE unassigned_segments (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    session_id TEXT NOT NULL REFERENCES unassigned_sessions(id),
    sequence_no INTEGER NOT NULL,
    started_at INTEGER NOT NULL,
    ended_at INTEGER,
    duration_seconds INTEGER NOT NULL DEFAULT 0 CHECK (duration_seconds >= 0),
    lease_token TEXT,
    UNIQUE (session_id, sequence_no)
);
CREATE UNIQUE INDEX uq_unassigned_segments_open ON unassigned_segments(session_id) WHERE ended_at IS NULL;

CREATE TABLE report_templates (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    report_type TEXT NOT NULL CHECK (report_type IN ('daily', 'weekly', 'monthly')),
    subject_id TEXT REFERENCES subjects(id),
    content TEXT NOT NULL,
    is_builtin INTEGER NOT NULL DEFAULT 0 CHECK (is_builtin IN (0, 1)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    deleted_at INTEGER
);
CREATE UNIQUE INDEX uq_report_templates_global ON report_templates(workspace_id, report_type) WHERE subject_id IS NULL AND deleted_at IS NULL;
CREATE UNIQUE INDEX uq_report_templates_subject ON report_templates(workspace_id, report_type, subject_id) WHERE subject_id IS NOT NULL AND deleted_at IS NULL;

CREATE TABLE reports (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    report_type TEXT NOT NULL CHECK (report_type IN ('daily', 'weekly', 'monthly')),
    subject_id TEXT NOT NULL REFERENCES subjects(id),
    period_start TEXT NOT NULL,
    period_end TEXT NOT NULL,
    reference_date TEXT NOT NULL,
    template_id TEXT REFERENCES report_templates(id),
    markdown_content TEXT NOT NULL,
    content_source TEXT NOT NULL CHECK (content_source IN ('generated', 'edited')),
    generation_count INTEGER NOT NULL DEFAULT 1 CHECK (generation_count > 0),
    input_revision_hash TEXT,
    generated_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    created_by_device_id TEXT REFERENCES devices(id),
    updated_by_device_id TEXT REFERENCES devices(id),
    deleted_at INTEGER,
    CHECK (period_start <= period_end),
    CHECK (report_type != 'daily' OR period_start = period_end)
);
CREATE UNIQUE INDEX uq_reports_active_period ON reports(workspace_id, report_type, subject_id, period_start, period_end) WHERE deleted_at IS NULL;
CREATE INDEX idx_reports_updated ON reports(workspace_id, updated_at DESC);

CREATE TABLE report_tasks (
    report_id TEXT NOT NULL REFERENCES reports(id),
    task_id TEXT NOT NULL REFERENCES tasks(id),
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    sort_order INTEGER NOT NULL,
    title_snapshot TEXT NOT NULL,
    path_snapshot TEXT NOT NULL,
    PRIMARY KEY (report_id, task_id)
);

CREATE TABLE plan_import_batches (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    target_date TEXT NOT NULL,
    subject_id TEXT NOT NULL REFERENCES subjects(id),
    source_path TEXT NOT NULL,
    source_mtime INTEGER,
    source_content_hash TEXT,
    state TEXT NOT NULL CHECK (state IN ('preview', 'confirmed', 'cancelled', 'failed')),
    raw_markdown TEXT NOT NULL,
    warnings_json TEXT NOT NULL DEFAULT '[]',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    version INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE plan_import_items (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    batch_id TEXT NOT NULL REFERENCES plan_import_batches(id),
    parent_item_id TEXT REFERENCES plan_import_items(id),
    title TEXT NOT NULL,
    estimate_minutes INTEGER,
    sort_order INTEGER NOT NULL,
    source_line_no INTEGER,
    source_text TEXT NOT NULL,
    parse_status TEXT NOT NULL CHECK (parse_status IN ('recognized', 'unrecognized')),
    confirmed_task_id TEXT REFERENCES tasks(id)
);

CREATE TABLE external_bindings (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    provider TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    external_id TEXT NOT NULL,
    external_revision TEXT,
    last_synced_hash TEXT,
    last_synced_at INTEGER,
    UNIQUE (workspace_id, provider, entity_type, entity_id),
    UNIQUE (workspace_id, provider, external_id)
);

CREATE TABLE sync_runs (
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

CREATE TABLE sync_items (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    run_id TEXT NOT NULL REFERENCES sync_runs(id),
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
CREATE INDEX idx_sync_items_retry ON sync_items(workspace_id, run_id, state);

CREATE TABLE app_settings (
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    key TEXT NOT NULL,
    value_json TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (workspace_id, key)
);

CREATE TABLE device_settings (
    key TEXT PRIMARY KEY,
    value_json TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE integration_configs (
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    provider TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
    config_json TEXT NOT NULL DEFAULT '{}',
    secret_ref TEXT,
    updated_at INTEGER NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (workspace_id, provider)
);

CREATE TABLE local_sync_state (
    workspace_id TEXT PRIMARY KEY,
    device_id TEXT NOT NULL,
    last_change_seq INTEGER NOT NULL DEFAULT 0,
    last_full_sync_at INTEGER,
    last_error TEXT
);

CREATE TABLE sync_outbox (
    operation_id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    device_id TEXT NOT NULL,
    operation_type TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    entity_id TEXT,
    base_version INTEGER,
    payload_json TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('pending', 'sending', 'conflict', 'failed')),
    attempt_count INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    last_attempt_at INTEGER,
    error_json TEXT
);
CREATE INDEX idx_sync_outbox_state ON sync_outbox(state, created_at);

CREATE TABLE local_report_outputs (
    report_id TEXT NOT NULL,
    device_id TEXT NOT NULL,
    output_path TEXT NOT NULL,
    output_content_hash TEXT NOT NULL,
    output_file_mtime INTEGER NOT NULL,
    written_at INTEGER NOT NULL,
    PRIMARY KEY (report_id, device_id)
);

CREATE TABLE app_metadata (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
INSERT INTO app_metadata(key, value) VALUES ('global_revision', '0');
