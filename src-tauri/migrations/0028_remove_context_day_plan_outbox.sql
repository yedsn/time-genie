DELETE FROM sync_outbox
WHERE entity_type = 'task_day_plan'
  AND operation_type = 'task_day_plan_materialize'
  AND json_valid(payload_json)
  AND json_extract(payload_json, '$.state') = 'included'
  AND EXISTS (
    SELECT 1 FROM tasks instance
    WHERE instance.workspace_id = sync_outbox.workspace_id
      AND instance.id = json_extract(sync_outbox.payload_json, '$.instance_task_id')
      AND instance.day_instance_kind = 'context'
      AND instance.day_instance_date = json_extract(sync_outbox.payload_json, '$.work_date')
      AND instance.day_instance_source_task_id = json_extract(sync_outbox.payload_json, '$.task_id')
  );

DELETE FROM task_day_plans
WHERE state = 'included'
  AND EXISTS (
    SELECT 1 FROM tasks instance
    WHERE instance.workspace_id = task_day_plans.workspace_id
      AND instance.id = task_day_plans.instance_task_id
      AND instance.day_instance_kind = 'context'
      AND instance.day_instance_date = task_day_plans.work_date
      AND instance.day_instance_source_task_id = task_day_plans.task_id
  );
