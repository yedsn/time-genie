create or replace function timegenie.touch_workspace_change()
returns trigger language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare row_image jsonb;
declare changed_entity_id text;
declare changed_device_id uuid;
begin
  row_image := to_jsonb(case when tg_op = 'DELETE' then old else new end);
  if not exists(
    select 1 from timegenie.workspaces
    where id=(row_image->>'workspace_id')::uuid
  ) then
    return coalesce(new, old);
  end if;
  changed_entity_id := case tg_table_name
    when 'work_days' then row_image->>'work_date'
    when 'app_settings' then row_image->>'key'
    when 'integration_configs' then row_image->>'provider'
    when 'external_bindings' then (row_image->>'provider') || '|' || (row_image->>'entity_type') || '|' || (row_image->>'entity_id')
    when 'time_segments' then row_image->>'entry_id'
    when 'time_allocations' then row_image->>'entry_id'
    when 'report_tasks' then row_image->>'report_id'
    when 'task_daily_estimates' then (row_image->>'task_id') || '|' || (row_image->>'work_date')
    when 'task_day_plans' then (row_image->>'task_id') || '|' || (row_image->>'work_date')
    when 'task_recurrence_rules' then row_image->>'task_id'
    when 'task_occurrences' then (row_image->>'task_id') || '|' || (row_image->>'occurrence_date')
    else coalesce(row_image->>'id', row_image->>'task_id')
  end;
  changed_device_id := nullif(coalesce(row_image->>'updated_by_device_id', row_image->>'created_by_device_id'), '')::uuid;
  insert into timegenie.workspace_changes(
    workspace_id, entity_type, entity_id, operation, entity_version, changed_by_device_id, changed_at
  ) values (
    (row_image->>'workspace_id')::uuid,
    tg_table_name,
    changed_entity_id,
    lower(tg_op),
    coalesce((row_image->>'version')::bigint, 1),
    changed_device_id,
    now()
  );
  return coalesce(new, old);
end $$;
