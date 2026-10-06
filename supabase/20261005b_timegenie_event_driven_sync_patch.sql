-- TimeGenie event-driven incremental cloud sync patch.
-- Run after 20261005_timegenie_cloud_task_planning_patch.sql.

begin;

create or replace function timegenie.touch_workspace_change()
returns trigger language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare row_image jsonb;
declare changed_entity_id text;
begin
  row_image := to_jsonb(case when tg_op = 'DELETE' then old else new end);
  changed_entity_id := case tg_table_name
    when 'work_days' then row_image->>'work_date'
    when 'app_settings' then row_image->>'key'
    when 'integration_configs' then row_image->>'provider'
    when 'external_bindings' then (row_image->>'provider') || '|' || (row_image->>'entity_type') || '|' || (row_image->>'entity_id')
    when 'time_segments' then row_image->>'entry_id'
    when 'time_allocations' then row_image->>'entry_id'
    when 'report_tasks' then row_image->>'report_id'
    when 'task_daily_estimates' then (row_image->>'task_id') || '|' || (row_image->>'work_date')
    when 'task_recurrence_rules' then row_image->>'task_id'
    when 'task_occurrences' then (row_image->>'task_id') || '|' || (row_image->>'occurrence_date')
    else coalesce(row_image->>'id', row_image->>'task_id')
  end;
  insert into timegenie.workspace_changes(workspace_id, entity_type, entity_id, operation, entity_version, changed_at)
  values((row_image->>'workspace_id')::uuid, tg_table_name, changed_entity_id, lower(tg_op), coalesce((row_image->>'version')::bigint, 1), now());
  return case when tg_op = 'DELETE' then old else new end;
end $$;

do $$
declare table_name text;
begin
  foreach table_name in array array['subjects','work_days','app_settings','tasks','task_status_events','task_daily_estimates','task_recurrence_rules','task_occurrences','time_entries','time_segments','time_allocations','unassigned_sessions','report_templates','reports','report_tasks','integration_configs','external_bindings'] loop
    execute format('drop trigger if exists workspace_change_trigger on timegenie.%I', table_name);
    execute format('create trigger workspace_change_trigger after insert or update or delete on timegenie.%I for each row execute function timegenie.touch_workspace_change()', table_name);
  end loop;
end $$;

create or replace function timegenie.cloud_incremental_entity_get(p_workspace_id uuid, p_entity_type text, p_entity_id text)
returns jsonb language plpgsql stable security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare entity_value jsonb; key_parts text[]; result_entity_type text := p_entity_type;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  key_parts := string_to_array(p_entity_id, '|');
  entity_value := case p_entity_type
    when 'subjects' then (select to_jsonb(r) from timegenie.subjects r where workspace_id = p_workspace_id and id::text = p_entity_id)
    when 'work_days' then (select to_jsonb(r) from timegenie.work_days r where workspace_id = p_workspace_id and work_date::text = p_entity_id)
    when 'app_settings' then (select to_jsonb(r) from timegenie.app_settings r where workspace_id = p_workspace_id and key = p_entity_id)
    when 'tasks' then (select to_jsonb(r) from timegenie.tasks r where workspace_id = p_workspace_id and id::text = p_entity_id)
    when 'task_status_events' then (select to_jsonb(r) from timegenie.task_status_events r where workspace_id = p_workspace_id and id::text = p_entity_id)
    when 'task_daily_estimates' then (select to_jsonb(r) from timegenie.task_daily_estimates r where workspace_id = p_workspace_id and task_id::text = key_parts[1] and work_date::text = key_parts[2])
    when 'task_recurrence_rules' then jsonb_build_object('task_id', p_entity_id, 'rules', coalesce((select jsonb_agg(to_jsonb(r) order by effective_start, id) from timegenie.task_recurrence_rules r where workspace_id = p_workspace_id and task_id::text = p_entity_id), '[]'::jsonb))
    when 'task_occurrences' then (select to_jsonb(r) from timegenie.task_occurrences r where workspace_id = p_workspace_id and task_id::text = key_parts[1] and occurrence_date::text = key_parts[2])
    when 'unassigned_sessions' then (select to_jsonb(r) from timegenie.unassigned_sessions r where workspace_id = p_workspace_id and id::text = p_entity_id)
    when 'time_entries' then (select to_jsonb(r) || jsonb_build_object('segments', coalesce((select jsonb_agg(to_jsonb(s) order by sequence_no, id) from timegenie.time_segments s where workspace_id = p_workspace_id and entry_id = r.id), '[]'::jsonb), 'allocations', coalesce((select jsonb_agg(to_jsonb(a) order by created_at, id) from timegenie.time_allocations a where workspace_id = p_workspace_id and entry_id = r.id), '[]'::jsonb)) from timegenie.time_entries r where workspace_id = p_workspace_id and id::text = p_entity_id)
    when 'time_segments' then (select to_jsonb(r) || jsonb_build_object('segments', coalesce((select jsonb_agg(to_jsonb(s) order by sequence_no, id) from timegenie.time_segments s where workspace_id = p_workspace_id and entry_id = r.id), '[]'::jsonb), 'allocations', coalesce((select jsonb_agg(to_jsonb(a) order by created_at, id) from timegenie.time_allocations a where workspace_id = p_workspace_id and entry_id = r.id), '[]'::jsonb)) from timegenie.time_entries r where workspace_id = p_workspace_id and id::text = p_entity_id)
    when 'time_allocations' then (select to_jsonb(r) || jsonb_build_object('segments', coalesce((select jsonb_agg(to_jsonb(s) order by sequence_no, id) from timegenie.time_segments s where workspace_id = p_workspace_id and entry_id = r.id), '[]'::jsonb), 'allocations', coalesce((select jsonb_agg(to_jsonb(a) order by created_at, id) from timegenie.time_allocations a where workspace_id = p_workspace_id and entry_id = r.id), '[]'::jsonb)) from timegenie.time_entries r where workspace_id = p_workspace_id and id::text = p_entity_id)
    when 'report_templates' then (select to_jsonb(r) from timegenie.report_templates r where workspace_id = p_workspace_id and id::text = p_entity_id)
    when 'reports' then (select to_jsonb(r) || jsonb_build_object('report_tasks', coalesce((select jsonb_agg(to_jsonb(t) order by sort_order, task_id) from timegenie.report_tasks t where workspace_id = p_workspace_id and report_id = r.id), '[]'::jsonb)) from timegenie.reports r where workspace_id = p_workspace_id and id::text = p_entity_id)
    when 'report_tasks' then (select to_jsonb(r) || jsonb_build_object('report_tasks', coalesce((select jsonb_agg(to_jsonb(t) order by sort_order, task_id) from timegenie.report_tasks t where workspace_id = p_workspace_id and report_id = r.id), '[]'::jsonb)) from timegenie.reports r where workspace_id = p_workspace_id and id::text = p_entity_id)
    when 'integration_configs' then (select to_jsonb(r) from timegenie.integration_configs r where workspace_id = p_workspace_id and provider = p_entity_id)
    when 'external_bindings' then (select to_jsonb(r) from timegenie.external_bindings r where workspace_id = p_workspace_id and provider = key_parts[1] and entity_type = key_parts[2] and entity_id::text = key_parts[3])
    else null end;
  if p_entity_type in ('time_segments','time_allocations') then result_entity_type := 'time_entries'; end if;
  if p_entity_type = 'report_tasks' then result_entity_type := 'reports'; end if;
  return jsonb_build_object('entity_type', result_entity_type, 'source_entity_type', p_entity_type, 'entity_id', p_entity_id, 'deleted', entity_value is null, 'data', entity_value);
end $$;

create or replace function timegenie.cloud_incremental_pull(p_workspace_id uuid, p_device_id uuid, p_after_change_seq bigint default 0, p_limit integer default 200)
returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare normalized_limit integer := greatest(1, least(coalesce(p_limit,200),1000)); latest_seq bigint; covered_seq bigint; change_values jsonb; entity_values jsonb;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id, p_device_id) then raise exception 'DEVICE_REVOKED'; end if;
  select coalesce(max(change_seq),0) into latest_seq from timegenie.workspace_changes where workspace_id = p_workspace_id;
  with selected_changes as (select change_seq, workspace_id, entity_type, entity_id, operation, entity_version, changed_by_device_id, changed_at from timegenie.workspace_changes where workspace_id = p_workspace_id and change_seq > greatest(coalesce(p_after_change_seq,0),0) order by change_seq limit normalized_limit)
  select coalesce(jsonb_agg(to_jsonb(r) order by change_seq),'[]'::jsonb), coalesce(max(change_seq),greatest(coalesce(p_after_change_seq,0),0)) into change_values, covered_seq from selected_changes r;
  with selected_changes as (select change_seq, entity_type, entity_id from timegenie.workspace_changes where workspace_id = p_workspace_id and change_seq > greatest(coalesce(p_after_change_seq,0),0) order by change_seq limit normalized_limit), latest_entities as (select distinct on (entity_type,entity_id) entity_type,entity_id,change_seq from selected_changes order by entity_type,entity_id,change_seq desc)
  select coalesce(jsonb_agg(timegenie.cloud_incremental_entity_get(p_workspace_id,entity_type,entity_id) || jsonb_build_object('change_seq',change_seq) order by change_seq),'[]'::jsonb) into entity_values from latest_entities;
  return jsonb_build_object('changes',change_values,'entities',entity_values,'covered_change_seq',covered_seq,'latest_change_seq',latest_seq,'has_more',covered_seq < latest_seq,'reset_required',false);
end $$;

revoke execute on function timegenie.cloud_incremental_entity_get(uuid,text,text) from public, anon, authenticated;
grant execute on function timegenie.cloud_incremental_pull(uuid,uuid,bigint,integer) to authenticated;
revoke execute on function timegenie.cloud_incremental_pull(uuid,uuid,bigint,integer) from anon;

commit;
notify pgrst, 'reload schema';
