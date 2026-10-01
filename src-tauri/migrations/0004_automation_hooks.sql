CREATE TABLE device_hooks (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL CHECK(length(trim(name)) > 0),
    event_type TEXT NOT NULL CHECK(event_type IN ('timer.started', 'timer.stopped', 'task.completed')),
    action_type TEXT NOT NULL CHECK(action_type IN ('uri', 'process')),
    action_config_json TEXT NOT NULL CHECK(json_valid(action_config_json)),
    timeout_seconds INTEGER NOT NULL DEFAULT 10 CHECK(timeout_seconds BETWEEN 1 AND 60),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0, 1)),
    sort_order INTEGER NOT NULL DEFAULT 10,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX idx_device_hooks_event_enabled_sort
    ON device_hooks(event_type, enabled, sort_order, created_at);

CREATE TABLE device_hook_runs (
    id TEXT PRIMARY KEY,
    hook_id TEXT,
    hook_name_snapshot TEXT NOT NULL,
    event_id TEXT NOT NULL,
    event_type TEXT NOT NULL CHECK(event_type IN ('timer.started', 'timer.stopped', 'task.completed')),
    is_test INTEGER NOT NULL DEFAULT 0 CHECK(is_test IN (0, 1)),
    status TEXT NOT NULL CHECK(status IN ('queued', 'running', 'succeeded', 'failed', 'timed_out')),
    exit_code INTEGER,
    duration_ms INTEGER,
    stdout_tail TEXT,
    stderr_tail TEXT,
    error_message TEXT,
    started_at INTEGER,
    finished_at INTEGER,
    created_at INTEGER NOT NULL,
    FOREIGN KEY(hook_id) REFERENCES device_hooks(id) ON DELETE SET NULL
);

CREATE UNIQUE INDEX idx_device_hook_runs_actual_event
    ON device_hook_runs(hook_id, event_id)
    WHERE is_test = 0 AND hook_id IS NOT NULL;

CREATE INDEX idx_device_hook_runs_created_at
    ON device_hook_runs(created_at DESC);

CREATE INDEX idx_device_hook_runs_hook_created_at
    ON device_hook_runs(hook_id, created_at DESC);
