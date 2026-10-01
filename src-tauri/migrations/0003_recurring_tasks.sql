CREATE TABLE task_recurrence_rules (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    task_id TEXT NOT NULL REFERENCES tasks(id),
    frequency TEXT NOT NULL CHECK (frequency IN ('daily', 'weekdays', 'weekly')),
    weekdays_mask INTEGER CHECK (weekdays_mask IS NULL OR (weekdays_mask BETWEEN 1 AND 127)),
    effective_start TEXT NOT NULL,
    effective_end TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    CHECK (frequency = 'weekly' OR weekdays_mask IS NULL),
    CHECK (frequency != 'weekly' OR weekdays_mask IS NOT NULL),
    CHECK (effective_end IS NULL OR effective_end >= effective_start),
    UNIQUE (task_id, effective_start)
);

CREATE UNIQUE INDEX uq_task_recurrence_rules_open
    ON task_recurrence_rules(task_id)
    WHERE effective_end IS NULL;
CREATE INDEX idx_task_recurrence_rules_workspace_dates
    ON task_recurrence_rules(workspace_id, effective_start, effective_end, task_id);
CREATE INDEX idx_task_recurrence_rules_task_dates
    ON task_recurrence_rules(task_id, effective_start, effective_end);

CREATE TABLE task_occurrences (
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    task_id TEXT NOT NULL REFERENCES tasks(id),
    occurrence_date TEXT NOT NULL,
    origin TEXT NOT NULL CHECK (origin IN ('scheduled', 'manual')),
    status TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'done')),
    completed_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (task_id, occurrence_date),
    CHECK ((status = 'done' AND completed_at IS NOT NULL) OR status = 'open')
);

CREATE INDEX idx_task_occurrences_workspace_date
    ON task_occurrences(workspace_id, occurrence_date, status, task_id);
