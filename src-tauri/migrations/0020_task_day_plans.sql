CREATE TABLE task_day_plans (
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    task_id TEXT NOT NULL REFERENCES tasks(id),
    work_date TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('included', 'excluded')),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (task_id, work_date)
);

CREATE INDEX idx_task_day_plans_date
    ON task_day_plans(workspace_id, work_date, state, task_id);

INSERT OR IGNORE INTO task_day_plans(
    workspace_id, task_id, work_date, state, created_at, updated_at, version
)
SELECT workspace_id, task_id, work_date, 'included', created_at, updated_at, version
FROM task_daily_estimates;
