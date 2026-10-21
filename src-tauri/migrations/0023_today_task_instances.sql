ALTER TABLE tasks ADD COLUMN day_instance_date TEXT;
ALTER TABLE tasks ADD COLUMN day_instance_kind TEXT CHECK (day_instance_kind IN ('member', 'context'));
ALTER TABLE tasks ADD COLUMN day_instance_source_task_id TEXT REFERENCES tasks(id);

CREATE INDEX idx_tasks_day_instances
    ON tasks(workspace_id, subject_id, day_instance_date, parent_id, sort_order);

CREATE UNIQUE INDEX idx_tasks_day_instance_source
    ON tasks(workspace_id, day_instance_source_task_id, day_instance_date)
    WHERE deleted_at IS NULL
      AND day_instance_source_task_id IS NOT NULL
      AND day_instance_date IS NOT NULL;

CREATE TRIGGER trg_tasks_day_instance_fields_insert
BEFORE INSERT ON tasks
WHEN (
    (NEW.day_instance_date IS NULL) <> (NEW.day_instance_kind IS NULL)
    OR (NEW.day_instance_source_task_id IS NOT NULL AND NEW.day_instance_date IS NULL)
)
BEGIN
    SELECT RAISE(ABORT, 'day instance date and kind must be set together');
END;

CREATE TRIGGER trg_tasks_day_instance_fields_update
BEFORE UPDATE OF day_instance_date, day_instance_kind, day_instance_source_task_id ON tasks
WHEN (
    (NEW.day_instance_date IS NULL) <> (NEW.day_instance_kind IS NULL)
    OR (NEW.day_instance_source_task_id IS NOT NULL AND NEW.day_instance_date IS NULL)
)
BEGIN
    SELECT RAISE(ABORT, 'day instance date and kind must be set together');
END;

CREATE TRIGGER trg_tasks_day_instance_parent_insert
BEFORE INSERT ON tasks
WHEN NEW.parent_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM tasks parent
    WHERE parent.id = NEW.parent_id
      AND parent.workspace_id = NEW.workspace_id
      AND parent.subject_id = NEW.subject_id
      AND parent.deleted_at IS NULL
      AND (
        (parent.day_instance_date IS NULL AND NEW.day_instance_date IS NULL)
        OR parent.day_instance_date = NEW.day_instance_date
      )
)
BEGIN
    SELECT RAISE(ABORT, 'task parent must be in the same day instance scope');
END;

CREATE TRIGGER trg_tasks_day_instance_parent_update
BEFORE UPDATE OF parent_id, day_instance_date, day_instance_kind, subject_id ON tasks
WHEN NEW.parent_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM tasks parent
    WHERE parent.id = NEW.parent_id
      AND parent.workspace_id = NEW.workspace_id
      AND parent.subject_id = NEW.subject_id
      AND parent.deleted_at IS NULL
      AND (
        (parent.day_instance_date IS NULL AND NEW.day_instance_date IS NULL)
        OR parent.day_instance_date = NEW.day_instance_date
      )
)
BEGIN
    SELECT RAISE(ABORT, 'task parent must be in the same day instance scope');
END;
