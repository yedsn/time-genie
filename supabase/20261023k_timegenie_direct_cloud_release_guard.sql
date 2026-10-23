-- Release observability support and an emergency server-side write barrier.
-- Queries and exports remain available while authoritative business mutations,
-- legacy sync writes and lease writes are rejected at the database boundary.

alter table timegenie.workspaces
  add column if not exists emergency_read_only boolean not null default false;
alter table timegenie.workspaces
  add column if not exists emergency_read_only_reason text;

create or replace function timegenie.emergency_read_only_assert(p_workspace_id uuid)
returns void language plpgsql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare read_only_value boolean; reason_value text;
begin
  select workspace.emergency_read_only,workspace.emergency_read_only_reason
  into read_only_value,reason_value
  from timegenie.workspaces workspace
  where workspace.id=p_workspace_id and workspace.deleted_at is null;
  if coalesce(read_only_value,false) then
    raise exception 'EMERGENCY_READ_ONLY: %',coalesce(nullif(reason_value,''),'Supabase 紧急停写已启用');
  end if;
end $$;

create or replace function timegenie.emergency_read_only_before_business_write()
returns trigger language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare workspace_value uuid;
begin
  workspace_value:=coalesce(
    case when tg_op<>'DELETE' then new.workspace_id else null end,
    case when tg_op<>'INSERT' then old.workspace_id else null end
  );
  perform timegenie.emergency_read_only_assert(workspace_value);
  return case when tg_op='DELETE' then old else new end;
end $$;

do $$
declare table_name text;
begin
  foreach table_name in array array[
    'devices','tracking_leases','workspace_changes','processed_operations',
    'subjects','work_days','app_settings','tasks','task_status_events',
    'task_day_plans','task_daily_estimates','task_recurrence_rules','task_occurrences',
    'time_entries','time_segments','time_allocations','actual_time_adjustments',
    'actual_adjustment_batches','unassigned_sessions','unassigned_segments',
    'report_templates','reports','report_tasks','integration_configs','external_bindings',
    'subject_work_schedules','subject_work_schedule_segments'
  ] loop
    if to_regclass('timegenie.'||table_name) is not null then
      execute format('drop trigger if exists emergency_read_only_write_barrier on timegenie.%I',table_name);
      execute format(
        'create trigger emergency_read_only_write_barrier before insert or update or delete on timegenie.%I for each row execute function timegenie.emergency_read_only_before_business_write()',
        table_name
      );
    end if;
  end loop;
end $$;

create or replace function timegenie.direct_cloud_context(
  p_workspace_id uuid,
  p_device_id uuid,
  p_expected_epoch bigint default null
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare workspace_row timegenie.workspaces; latest_change_seq bigint;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  select workspace.* into workspace_row from timegenie.workspaces workspace
  where workspace.id=p_workspace_id and workspace.deleted_at is null;
  select coalesce(max(change_seq),0) into latest_change_seq
  from timegenie.workspace_changes where workspace_id=p_workspace_id;
  return jsonb_build_object(
    'workspaceId',workspace_row.id,
    'deviceId',p_device_id,
    'storageProtocol',workspace_row.storage_protocol,
    'workspaceEpoch',workspace_row.workspace_epoch,
    'changeSeq',latest_change_seq,
    'schemaGeneration',workspace_row.minimum_client_schema_generation,
    'serverTime',now(),
    'writable',not workspace_row.emergency_read_only,
    'readOnlyReason',case when workspace_row.emergency_read_only then
      coalesce(nullif(workspace_row.emergency_read_only_reason,''),'Supabase 紧急停写已启用')
      else null end
  );
end $$;

revoke execute on function timegenie.emergency_read_only_assert(uuid),
  timegenie.emergency_read_only_before_business_write()
from public,anon,authenticated;

notify pgrst, 'reload schema';
