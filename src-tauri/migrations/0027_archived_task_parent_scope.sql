DROP TRIGGER IF EXISTS trg_tasks_day_instance_parent_insert;
DROP TRIGGER IF EXISTS trg_tasks_day_instance_parent_update;

CREATE TRIGGER trg_tasks_day_instance_parent_insert
BEFORE INSERT ON tasks
WHEN NEW.parent_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM tasks parent
    WHERE parent.id = NEW.parent_id
      AND parent.workspace_id = NEW.workspace_id
      AND parent.subject_id = NEW.subject_id
      AND (NEW.deleted_at IS NOT NULL OR parent.deleted_at IS NULL)
      AND (
        (parent.day_instance_date IS NULL AND NEW.day_instance_date IS NULL)
        OR parent.day_instance_date = NEW.day_instance_date
      )
)
BEGIN
    SELECT RAISE(ABORT, 'task parent must be in the same day instance scope');
END;

CREATE TRIGGER trg_tasks_day_instance_parent_update
BEFORE UPDATE OF parent_id, day_instance_date, day_instance_kind, subject_id, deleted_at ON tasks
WHEN NEW.parent_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM tasks parent
    WHERE parent.id = NEW.parent_id
      AND parent.workspace_id = NEW.workspace_id
      AND parent.subject_id = NEW.subject_id
      AND (NEW.deleted_at IS NOT NULL OR parent.deleted_at IS NULL)
      AND (
        (parent.day_instance_date IS NULL AND NEW.day_instance_date IS NULL)
        OR parent.day_instance_date = NEW.day_instance_date
      )
)
BEGIN
    SELECT RAISE(ABORT, 'task parent must be in the same day instance scope');
END;
