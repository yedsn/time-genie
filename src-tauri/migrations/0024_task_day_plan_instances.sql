ALTER TABLE task_day_plans ADD COLUMN instance_task_id TEXT REFERENCES tasks(id);

CREATE INDEX idx_task_day_plans_instance
    ON task_day_plans(workspace_id, work_date, instance_task_id);
