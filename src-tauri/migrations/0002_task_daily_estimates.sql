CREATE TABLE task_daily_estimates (
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    task_id TEXT NOT NULL REFERENCES tasks(id),
    work_date TEXT NOT NULL,
    estimate_minutes INTEGER NOT NULL CHECK (estimate_minutes > 0),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (task_id, work_date)
);
CREATE INDEX idx_task_daily_estimates_date
    ON task_daily_estimates(workspace_id, work_date, task_id);

UPDATE tasks SET planned_time = NULL WHERE planned_time IS NOT NULL;
