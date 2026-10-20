-- Add cloud support for explicit per-day task inclusion/exclusion.
-- Idempotent upgrade patch for deployments created before task_day_plans.
begin;

create table if not exists timegenie.task_day_plans (
  workspace_id uuid not null references timegenie.workspaces(id) on delete cascade,
  task_id uuid not null references timegenie.tasks(id) on delete cascade,
  work_date date not null,
  state text not null check (state in ('included','excluded')),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  version bigint not null default 1,
  primary key(task_id,work_date)
);
create index if not exists idx_task_day_plans_date
  on timegenie.task_day_plans(workspace_id,work_date,state,task_id);

alter table timegenie.task_day_plans enable row level security;
drop policy if exists workspace_owner on timegenie.task_day_plans;
create policy workspace_owner on timegenie.task_day_plans for all
using(timegenie.is_workspace_owner(workspace_id))
with check(timegenie.is_workspace_owner(workspace_id));

revoke all on timegenie.task_day_plans from public,anon,authenticated;
grant all on timegenie.task_day_plans to service_role;

create or replace function timegenie.touch_workspace_change()
returns trigger language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare row_image jsonb; changed_entity_id text;
begin
  row_image:=to_jsonb(case when tg_op='DELETE' then old else new end);
  if not exists(select 1 from timegenie.workspaces where id=(row_image->>'workspace_id')::uuid) then
    return coalesce(new,old);
  end if;
  changed_entity_id:=case tg_table_name
    when 'work_days' then row_image->>'work_date'
    when 'app_settings' then row_image->>'key'
    when 'integration_configs' then row_image->>'provider'
    when 'external_bindings' then (row_image->>'provider')||'|'||(row_image->>'entity_type')||'|'||(row_image->>'entity_id')
    when 'time_segments' then row_image->>'entry_id'
    when 'time_allocations' then row_image->>'entry_id'
    when 'report_tasks' then row_image->>'report_id'
    when 'task_day_plans' then (row_image->>'task_id')||'|'||(row_image->>'work_date')
    when 'task_daily_estimates' then (row_image->>'task_id')||'|'||(row_image->>'work_date')
    when 'task_recurrence_rules' then row_image->>'task_id'
    when 'task_occurrences' then (row_image->>'task_id')||'|'||(row_image->>'occurrence_date')
    else coalesce(row_image->>'id',row_image->>'task_id') end;
  insert into timegenie.workspace_changes(workspace_id,entity_type,entity_id,operation,entity_version,changed_at)
  values((row_image->>'workspace_id')::uuid,tg_table_name,changed_entity_id,lower(tg_op),coalesce((row_image->>'version')::bigint,1),now());
  return coalesce(new,old);
end $$;

drop trigger if exists workspace_change_trigger on timegenie.task_day_plans;
create trigger workspace_change_trigger after insert or update or delete on timegenie.task_day_plans
for each row execute function timegenie.touch_workspace_change();
drop trigger if exists workspace_epoch_write_barrier on timegenie.task_day_plans;
create trigger workspace_epoch_write_barrier before insert or update or delete on timegenie.task_day_plans
for each row execute function timegenie.workspace_epoch_before_business_write();
drop trigger if exists workspace_restore_write_barrier on timegenie.task_day_plans;
create trigger workspace_restore_write_barrier before insert or update or delete on timegenie.task_day_plans
for each row execute function timegenie.workspace_restore_before_business_write();

do $$ begin
  if to_regprocedure('timegenie.cloud_snapshot_get_pre_task_day_plans(uuid)') is null then
    alter function timegenie.cloud_snapshot_get(uuid) rename to cloud_snapshot_get_pre_task_day_plans;
  end if;
  if to_regprocedure('timegenie.cloud_incremental_entity_get_pre_task_day_plans(uuid,text,text)') is null then
    alter function timegenie.cloud_incremental_entity_get(uuid,text,text) rename to cloud_incremental_entity_get_pre_task_day_plans;
  end if;
  if to_regprocedure('timegenie.migration_import_snapshot_pre_task_day_plans(uuid,uuid,uuid,jsonb)') is null then
    alter function timegenie.migration_import_snapshot(uuid,uuid,uuid,jsonb) rename to migration_import_snapshot_pre_task_day_plans;
  end if;
end $$;

create or replace function timegenie.cloud_snapshot_get(p_workspace_id uuid)
returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare result_value jsonb;
begin
  result_value:=timegenie.cloud_snapshot_get_pre_task_day_plans(p_workspace_id);
  return result_value||jsonb_build_object(
    'task_day_plans',coalesce((select jsonb_agg(to_jsonb(row_value) order by work_date,task_id)
      from timegenie.task_day_plans row_value where workspace_id=p_workspace_id),'[]'::jsonb)
  );
end $$;

create or replace function timegenie.cloud_incremental_entity_get(
  p_workspace_id uuid,p_entity_type text,p_entity_id text
) returns jsonb language plpgsql stable security definer set search_path=timegenie,extensions,pg_catalog as $$
declare entity_value jsonb; key_parts text[];
begin
  if p_entity_type<>'task_day_plans' then
    return timegenie.cloud_incremental_entity_get_pre_task_day_plans(p_workspace_id,p_entity_type,p_entity_id);
  end if;
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  key_parts:=string_to_array(p_entity_id,'|');
  if array_length(key_parts,1)<>2 then raise exception 'VALIDATION_ERROR: invalid task day plan identity'; end if;
  select to_jsonb(row_value) into entity_value from timegenie.task_day_plans row_value
  where workspace_id=p_workspace_id and task_id::text=key_parts[1] and work_date::text=key_parts[2];
  return jsonb_build_object('entity_type','task_day_plans','source_entity_type','task_day_plans',
    'entity_id',p_entity_id,'deleted',entity_value is null,'data',entity_value);
end $$;

create or replace function timegenie.migration_import_snapshot(
  p_workspace_id uuid,p_operation_id uuid,p_device_id uuid,p_snapshot jsonb
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare result_value jsonb; row_data jsonb; sanitized jsonb; cached jsonb; request_value jsonb;
begin
  request_value:=jsonb_build_object('deviceId',p_device_id,'snapshot',coalesce(p_snapshot,'{}'::jsonb));
  cached:=timegenie.processed_operation_result(p_workspace_id,p_operation_id,'migration_import_snapshot',request_value);
  if cached is not null then return cached; end if;
  sanitized:=jsonb_set(coalesce(p_snapshot,'{}'::jsonb),'{task_day_plans}','[]'::jsonb,true);
  result_value:=timegenie.migration_import_snapshot_pre_task_day_plans(p_workspace_id,p_operation_id,p_device_id,sanitized);
  delete from timegenie.task_day_plans where workspace_id=p_workspace_id;
  for row_data in select value from jsonb_array_elements(coalesce(p_snapshot->'task_day_plans','[]'::jsonb)) loop
    insert into timegenie.task_day_plans(workspace_id,task_id,work_date,state,created_at,updated_at,version)
    values(p_workspace_id,(row_data->>'task_id')::uuid,(row_data->>'work_date')::date,row_data->>'state',
      to_timestamp((row_data->>'created_at')::double precision/1000),
      to_timestamp((row_data->>'updated_at')::double precision/1000),coalesce((row_data->>'version')::bigint,1))
    on conflict(task_id,work_date) do update set state=excluded.state,updated_at=excluded.updated_at,
      version=greatest(timegenie.task_day_plans.version,excluded.version);
  end loop;
  update timegenie.processed_operations
  set request_hash=timegenie.operation_request_hash('migration_import_snapshot',request_value)
  where workspace_id=p_workspace_id and operation_id=p_operation_id;
  return result_value;
end $$;

create or replace function timegenie.apply_task_day_plan(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_operation_type text,p_entity_id text,
  p_base_version bigint,p_payload_version bigint,p_coalesced_count bigint,p_payload jsonb,p_request jsonb
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare row_data jsonb:=coalesce(p_payload,'{}'::jsonb); current_version bigint; cached jsonb; result_value jsonb;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  cached:=timegenie.processed_operation_result(p_workspace_id,p_operation_id,p_operation_type,p_request);
  if cached is not null then return cached; end if;
  if p_entity_id is null or p_entity_id is distinct from (row_data->>'task_id')||'|'||(row_data->>'work_date') then
    raise exception 'VALIDATION_ERROR: entity id does not match task day plan';
  end if;
  if not exists(select 1 from timegenie.tasks where workspace_id=p_workspace_id
    and id=(row_data->>'task_id')::uuid and deleted_at is null) then raise exception 'NOT_FOUND: task does not exist'; end if;
  if coalesce(row_data->>'state','') not in('included','excluded') then
    raise exception 'VALIDATION_ERROR: invalid task day plan state';
  end if;
  if p_payload_version is null or (row_data->>'version')::bigint is distinct from p_payload_version then
    raise exception 'VERSION_CONFLICT: declared payload version mismatch';
  end if;
  if p_coalesced_count is null or p_coalesced_count<1 then
    raise exception 'VERSION_CONFLICT: coalesced operation count must be positive';
  end if;
  if (p_base_version is null and p_payload_version<>p_coalesced_count)
     or (p_base_version is not null and p_payload_version<>p_base_version+p_coalesced_count) then
    raise exception 'VERSION_CONFLICT: task day plan payload version does not match operation span';
  end if;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':task-day-plan:'||p_entity_id,0));
  select version into current_version from timegenie.task_day_plans
  where workspace_id=p_workspace_id and task_id=(row_data->>'task_id')::uuid and work_date=(row_data->>'work_date')::date;
  if p_base_version is null and current_version is not null then
    raise exception 'SYNC_CONFLICT: expected new entity, current version %',current_version;
  elsif p_base_version is not null and current_version is distinct from p_base_version then
    raise exception 'SYNC_CONFLICT: expected version %, current version %',p_base_version,current_version;
  end if;
  insert into timegenie.task_day_plans(workspace_id,task_id,work_date,state,created_at,updated_at,version)
  values(p_workspace_id,(row_data->>'task_id')::uuid,(row_data->>'work_date')::date,row_data->>'state',
    to_timestamp(coalesce((row_data->>'created_at')::double precision,extract(epoch from clock_timestamp())*1000)/1000),
    to_timestamp(coalesce((row_data->>'updated_at')::double precision,extract(epoch from clock_timestamp())*1000)/1000),p_payload_version)
  on conflict(task_id,work_date) do update set state=excluded.state,updated_at=excluded.updated_at,version=excluded.version;
  result_value:=jsonb_build_object('entityType','task_day_plan','entityId',p_entity_id,'version',p_payload_version);
  perform timegenie.record_processed_operation(p_workspace_id,p_operation_id,p_device_id,p_operation_type,p_request,result_value);
  return result_value;
end $$;

create or replace function timegenie.cloud_apply_patch_pre_subject_work_schedules(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_operation_type text,p_entity_type text,p_entity_id text,p_base_version bigint,p_payload jsonb
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$ begin
  if p_entity_type='task_day_plan' then
    return timegenie.apply_task_day_plan(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_id,p_base_version,
      coalesce((p_payload->>'version')::bigint,1),1,p_payload,
      jsonb_build_object('deviceId',p_device_id,'entityType',p_entity_type,'entityId',p_entity_id,'baseVersion',p_base_version,'payload',coalesce(p_payload,'{}'::jsonb)));
  end if;
  if p_entity_type='report_template' then
    return timegenie.apply_named_report_template(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_id,p_base_version,p_payload,jsonb_build_object('deviceId',p_device_id,'entityType',p_entity_type,'entityId',p_entity_id,'baseVersion',p_base_version,'payload',coalesce(p_payload,'{}'::jsonb)));
  end if;
  if p_entity_type='unassigned_session' and p_operation_type in('unassigned_session_pause','unassigned_session_resume','unassigned_session_awaiting_resolution') then return timegenie.cloud_apply_simplified_unassigned(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_id,p_base_version,p_payload); end if;
  if p_entity_type='time_entry' and coalesce(p_payload->>'source_type','')='timer' then return timegenie.apply_calendar_time_entry(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_id,p_base_version,p_payload,jsonb_build_object('deviceId',p_device_id,'entityType',p_entity_type,'entityId',p_entity_id,'baseVersion',p_base_version,'payload',coalesce(p_payload,'{}'::jsonb))); end if;
  return timegenie.cloud_apply_patch_pre_calendar(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_type,p_entity_id,p_base_version,p_payload);
end $$;

create or replace function timegenie.cloud_apply_patch_pre_subject_work_schedules(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_operation_type text,p_entity_type text,p_entity_id text,p_base_version bigint,p_payload_version bigint,p_coalesced_count bigint,p_payload jsonb
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$ begin
  if p_payload_version is null or (p_payload->>'version')::bigint<>p_payload_version then raise exception 'VERSION_CONFLICT: declared payload version mismatch'; end if;
  if p_coalesced_count is null or p_coalesced_count<1 then raise exception 'VERSION_CONFLICT: coalesced operation count must be positive'; end if;
  if p_entity_type='task_day_plan' then
    return timegenie.apply_task_day_plan(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_id,p_base_version,p_payload_version,p_coalesced_count,p_payload,
      jsonb_build_object('deviceId',p_device_id,'entityType',p_entity_type,'entityId',p_entity_id,'baseVersion',p_base_version,'payloadVersion',p_payload_version,'coalescedCount',p_coalesced_count,'payload',coalesce(p_payload,'{}'::jsonb)));
  end if;
  if p_entity_type='report_template' then return timegenie.apply_named_report_template(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_id,p_base_version,p_payload,jsonb_build_object('deviceId',p_device_id,'entityType',p_entity_type,'entityId',p_entity_id,'baseVersion',p_base_version,'payloadVersion',p_payload_version,'coalescedCount',p_coalesced_count,'payload',coalesce(p_payload,'{}'::jsonb))); end if;
  if p_entity_type='unassigned_session' and p_operation_type in('unassigned_session_pause','unassigned_session_resume','unassigned_session_awaiting_resolution') then return timegenie.cloud_apply_simplified_unassigned(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_id,p_base_version,p_payload); end if;
  if p_entity_type='time_entry' and coalesce(p_payload->>'source_type','')='timer' then return timegenie.apply_calendar_time_entry(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_id,p_base_version,p_payload,jsonb_build_object('deviceId',p_device_id,'entityType',p_entity_type,'entityId',p_entity_id,'baseVersion',p_base_version,'payloadVersion',p_payload_version,'coalescedCount',p_coalesced_count,'payload',coalesce(p_payload,'{}'::jsonb))); end if;
  return timegenie.cloud_apply_patch_pre_calendar(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_type,p_entity_id,p_base_version,p_payload_version,p_coalesced_count,p_payload);
end $$;

create or replace function timegenie.workspace_snapshot_entity_catalog()
returns table(entity_name text,group_name text,sort_order integer) language sql immutable as $$ values
  ('subjects','planning',10),('subject_work_schedules','planning',20),('subject_work_schedule_segments','planning',30),('work_days','planning',40),
  ('tasks','planning',50),('task_status_events','planning',60),('task_day_plans','planning',70),('task_daily_estimates','planning',80),('task_recurrence_rules','planning',90),('task_occurrences','planning',100),
  ('time_entries','time_tracking',110),('time_segments','time_tracking',120),('time_allocations','time_tracking',130),('unassigned_sessions','time_tracking',140),
  ('unassigned_segments','time_tracking',150),('actual_time_adjustments','time_tracking',160),('report_templates','reports',170),('reports','reports',180),
  ('report_tasks','reports',190),('app_settings','shared_settings',200),('integration_configs','shared_settings',210),('external_bindings','shared_settings',220);
$$;

create or replace function timegenie.workspace_snapshot_row_identity(p_entity_name text,p_row jsonb)
returns text language sql immutable as $$
  select case p_entity_name when 'work_days' then p_row->>'work_date'
    when 'task_day_plans' then concat_ws('|',p_row->>'task_id',p_row->>'work_date')
    when 'task_daily_estimates' then concat_ws('|',p_row->>'task_id',p_row->>'work_date')
    when 'task_occurrences' then concat_ws('|',p_row->>'task_id',p_row->>'occurrence_date')
    when 'report_tasks' then concat_ws('|',p_row->>'report_id',p_row->>'task_id')
    when 'app_settings' then p_row->>'key' when 'integration_configs' then p_row->>'provider'
    else coalesce(p_row->>'id',p_row->>'entity_id',p_row::text) end;
$$;

create or replace function timegenie.timer_transition_apply(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_payload jsonb
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare cached jsonb; request_value jsonb; previous_row jsonb; task_row jsonb; occurrence_row jsonb; day_plan_row jsonb; active_row jsonb; reopen_base_version bigint;
declare remote_active timegenie.time_entries; remote_occurrence timegenie.task_occurrences; transition_at timestamptz; result_value jsonb;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  request_value:=jsonb_build_object('deviceId',p_device_id,'payload',coalesce(p_payload,'{}'::jsonb));
  cached:=timegenie.processed_operation_result(p_workspace_id,p_operation_id,'timer_transition_apply',request_value);
  if cached is not null then return cached; end if;
  previous_row:=p_payload->'previous_entry'; task_row:=p_payload->'task'; occurrence_row:=p_payload->'occurrence'; day_plan_row:=p_payload->'day_plan'; active_row:=p_payload->'active_entry';
  reopen_base_version:=nullif(p_payload->>'reopen_task_base_version','')::bigint;
  if active_row is null or jsonb_typeof(active_row)<>'object' then raise exception 'VALIDATION_ERROR: missing active timer'; end if;
  transition_at:=to_timestamp((active_row->>'started_at')::double precision/1000);
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':active-timer',0));
  select * into remote_active from timegenie.time_entries where workspace_id=p_workspace_id and state in('running','paused') and deleted_at is null limit 1 for update;
  if previous_row is null or previous_row='null'::jsonb then
    if found then raise exception 'VERSION_CONFLICT: remote timer is active'; end if;
  else
    if not found or remote_active.id<>(previous_row->>'id')::uuid then raise exception 'VERSION_CONFLICT: remote timer changed'; end if;
    if remote_active.version<>coalesce((p_payload->>'previous_base_version')::bigint,-1) then raise exception 'VERSION_CONFLICT: remote timer version changed'; end if;
    if to_timestamp((previous_row->>'ended_at')::double precision/1000)<>transition_at then raise exception 'VALIDATION_ERROR: transition boundary mismatch'; end if;
    perform timegenie.apply_calendar_time_entry_pre_simplified_anchor(
      p_workspace_id,p_device_id,timegenie.calendar_stable_uuid('timer-transition-stop:'||p_operation_id),
      'timer_transition_stop',previous_row->>'id',(p_payload->>'previous_base_version')::bigint,previous_row,
      jsonb_build_object('deviceId',p_device_id,'transitionId',p_operation_id,'phase','stop','payload',previous_row));
  end if;
  if task_row is not null and task_row<>'null'::jsonb then
    if reopen_base_version is not null then
      if not exists(select 1 from timegenie.tasks target where target.workspace_id=p_workspace_id and target.id=(task_row->>'id')::uuid and target.status='done' and target.version=reopen_base_version and target.deleted_at is null) then
        raise exception 'VERSION_CONFLICT: completed task changed';
      end if;
    end if;
    if nullif(task_row->>'parent_id','') is not null and not exists(
      select 1 from timegenie.tasks parent where parent.workspace_id=p_workspace_id and parent.id=(task_row->>'parent_id')::uuid
        and parent.subject_id=(task_row->>'subject_id')::uuid and parent.deleted_at is null and parent.status='open' and parent.parent_id is null
        and parent.version=coalesce((p_payload->>'expected_parent_version')::bigint,-1)
        and not exists(select 1 from timegenie.task_recurrence_rules rule where rule.task_id=parent.id and rule.effective_end is null)
        and not exists(select 1 from timegenie.time_allocations allocation join timegenie.time_entries entry on entry.id=allocation.entry_id where allocation.task_id=parent.id and entry.workspace_id=p_workspace_id and entry.deleted_at is null)
        and not exists(select 1 from timegenie.actual_time_adjustments adjustment where adjustment.task_id=parent.id and adjustment.workspace_id=p_workspace_id and adjustment.deleted_at is null)
    ) then raise exception 'VERSION_CONFLICT: parent task changed'; end if;
    perform timegenie.cloud_apply_patch_pre_calendar(
      p_workspace_id,p_device_id,timegenie.calendar_stable_uuid('timer-transition-task:'||p_operation_id),
      case when reopen_base_version is null then 'task_create_from_timer_transition' else 'task_reopen_from_timer_transition' end,
      'task',task_row->>'id',reopen_base_version,task_row);
  end if;
  if not exists(select 1 from timegenie.tasks target where target.workspace_id=p_workspace_id and target.id=(active_row->>'default_task_id')::uuid
    and target.deleted_at is null and not exists(select 1 from timegenie.tasks child where child.parent_id=target.id and child.deleted_at is null)) then
    raise exception 'TASK_NOT_SELECTABLE';
  end if;
  if occurrence_row is not null and occurrence_row<>'null'::jsonb then
    if occurrence_row->>'task_id' is distinct from active_row->>'default_task_id' or occurrence_row->>'occurrence_date' is distinct from active_row->>'work_date' then
      raise exception 'VALIDATION_ERROR: timer occurrence identity mismatch';
    end if;
    if coalesce((occurrence_row->>'version')::bigint,0)<>1 or occurrence_row->>'status' is distinct from 'open' then
      raise exception 'VALIDATION_ERROR: timer occurrence must be an open initial version';
    end if;
    select * into remote_occurrence from timegenie.task_occurrences where workspace_id=p_workspace_id and task_id=(occurrence_row->>'task_id')::uuid and occurrence_date=(occurrence_row->>'occurrence_date')::date for update;
    if not found then
      insert into timegenie.task_occurrences(workspace_id,task_id,occurrence_date,origin,status,completed_at,created_at,updated_at,version)
      values(p_workspace_id,(occurrence_row->>'task_id')::uuid,(occurrence_row->>'occurrence_date')::date,occurrence_row->>'origin','open',null,
        to_timestamp(coalesce((occurrence_row->>'created_at')::double precision,extract(epoch from now())*1000)/1000),
        to_timestamp(coalesce((occurrence_row->>'updated_at')::double precision,extract(epoch from now())*1000)/1000),1);
    end if;
  end if;
  if day_plan_row is not null and day_plan_row<>'null'::jsonb then
    if day_plan_row->>'task_id' is distinct from active_row->>'default_task_id'
       or day_plan_row->>'work_date' is distinct from active_row->>'work_date'
       or day_plan_row->>'state' is distinct from 'included'
       or coalesce((day_plan_row->>'version')::bigint,0)<>1 then
      raise exception 'VALIDATION_ERROR: timer day plan must be an included initial version';
    end if;
    insert into timegenie.task_day_plans(workspace_id,task_id,work_date,state,created_at,updated_at,version)
    values(p_workspace_id,(day_plan_row->>'task_id')::uuid,(day_plan_row->>'work_date')::date,'included',
      to_timestamp(coalesce((day_plan_row->>'created_at')::double precision,extract(epoch from now())*1000)/1000),
      to_timestamp(coalesce((day_plan_row->>'updated_at')::double precision,extract(epoch from now())*1000)/1000),1)
    on conflict(task_id,work_date) do update set state='included',updated_at=excluded.updated_at,version=timegenie.task_day_plans.version+1;
  end if;
  perform timegenie.apply_calendar_time_entry_pre_simplified_anchor(
    p_workspace_id,p_device_id,timegenie.calendar_stable_uuid('timer-transition-start:'||p_operation_id),
    'timer_transition_start',active_row->>'id',null,active_row,
    jsonb_build_object('deviceId',p_device_id,'transitionId',p_operation_id,'phase','start','payload',active_row));
  perform timegenie.timegenie_unassigned_system_clear(p_workspace_id,transition_at);
  result_value:=jsonb_build_object('accepted',true,'activeEntry',timegenie.cloud_incremental_entity_get(p_workspace_id,'time_entries',active_row->>'id')->'data',
    'previousEntry',case when previous_row is null or previous_row='null'::jsonb then null else timegenie.cloud_incremental_entity_get(p_workspace_id,'time_entries',previous_row->>'id')->'data' end,
    'task',case when task_row is null or task_row='null'::jsonb then null else timegenie.cloud_incremental_entity_get(p_workspace_id,'tasks',task_row->>'id')->'data' end,
    'occurrence',case when occurrence_row is null or occurrence_row='null'::jsonb then null else timegenie.cloud_incremental_entity_get(p_workspace_id,'task_occurrences',(occurrence_row->>'task_id')||'|'||(occurrence_row->>'occurrence_date'))->'data' end,
    'dayPlan',case when day_plan_row is null or day_plan_row='null'::jsonb then null else timegenie.cloud_incremental_entity_get(p_workspace_id,'task_day_plans',(day_plan_row->>'task_id')||'|'||(day_plan_row->>'work_date'))->'data' end);
  perform timegenie.record_processed_operation(p_workspace_id,p_operation_id,p_device_id,'timer_transition_apply',request_value,result_value);
  return result_value;
end $$;

create or replace function timegenie.workspace_restore_commit(p_workspace_id uuid,p_device_id uuid,p_lock_token uuid,p_target_snapshot_id uuid,p_target_content_hash text,p_prepared_epoch bigint,p_operation_id uuid,p_confirmed boolean) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog set statement_timeout='600s' as $$
declare lock_value timegenie.workspace_restore_locks; target jsonb; group_name_value text; c record; rows_value jsonb; row_value jsonb; current_epoch bigint; new_epoch bigint; result_value jsonb; cached jsonb; reset_seq bigint;
begin
  if not p_confirmed then raise exception 'RESTORE_CONFIRMATION_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  cached:=timegenie.processed_operation_result(p_workspace_id,p_operation_id,'workspace.restore',jsonb_build_object('entityType','workspace','entityId',p_workspace_id,'snapshotId',p_target_snapshot_id,'targetHash',p_target_content_hash,'preparedEpoch',p_prepared_epoch));
  if cached is not null then return cached; end if;
  select * into lock_value from timegenie.workspace_restore_locks where workspace_id=p_workspace_id and lock_token=p_lock_token and holder_device_id=p_device_id and status='prepared' and expires_at>now() for update;
  if not found then raise exception 'RESTORE_LOCK_INVALID_OR_EXPIRED'; end if;
  if lock_value.target_snapshot_id<>p_target_snapshot_id or lock_value.target_content_hash<>p_target_content_hash or lock_value.prepared_epoch<>p_prepared_epoch then raise exception 'RESTORE_PREPARE_MISMATCH'; end if;
  select workspace_epoch into current_epoch from timegenie.workspaces where id=p_workspace_id for update;
  if current_epoch<>p_prepared_epoch then raise exception 'WORKSPACE_EPOCH_STALE: client=%, server=%',p_prepared_epoch,current_epoch; end if;
  target:=timegenie.workspace_snapshot_rehydrate(p_target_snapshot_id);
  if timegenie.workspace_snapshot_hash(target)<>p_target_content_hash then raise exception 'SNAPSHOT_CHECKSUM_FAILED'; end if;
  update timegenie.workspace_restore_locks set status='committing',updated_at=now() where workspace_id=p_workspace_id;
  perform set_config('timegenie.restore_lock_token',p_lock_token::text,true);
  perform set_config('request.headers',jsonb_build_object('x-timegenie-workspace-epoch',current_epoch::text)::text,true);
  delete from timegenie.report_tasks where workspace_id=p_workspace_id;
  delete from timegenie.reports where workspace_id=p_workspace_id;
  delete from timegenie.actual_time_adjustments where workspace_id=p_workspace_id;
  delete from timegenie.time_allocations where workspace_id=p_workspace_id;
  delete from timegenie.time_segments where workspace_id=p_workspace_id;
  update timegenie.unassigned_sessions set generated_entry_id=null where workspace_id=p_workspace_id;
  delete from timegenie.unassigned_segments where workspace_id=p_workspace_id;
  delete from timegenie.unassigned_sessions where workspace_id=p_workspace_id;
  delete from timegenie.time_entries where workspace_id=p_workspace_id;
  delete from timegenie.task_status_events where workspace_id=p_workspace_id;
  delete from timegenie.task_day_plans where workspace_id=p_workspace_id;
  delete from timegenie.task_daily_estimates where workspace_id=p_workspace_id;
  delete from timegenie.task_occurrences where workspace_id=p_workspace_id;
  delete from timegenie.task_recurrence_rules where workspace_id=p_workspace_id;
  delete from timegenie.report_templates where workspace_id=p_workspace_id;
  delete from timegenie.tasks where workspace_id=p_workspace_id;
  delete from timegenie.subject_work_schedule_segments where workspace_id=p_workspace_id;
  delete from timegenie.subject_work_schedules where workspace_id=p_workspace_id;
  delete from timegenie.work_days where workspace_id=p_workspace_id;
  delete from timegenie.external_bindings where workspace_id=p_workspace_id;
  delete from timegenie.integration_configs where workspace_id=p_workspace_id;
  delete from timegenie.app_settings where workspace_id=p_workspace_id;
  delete from timegenie.subjects where workspace_id=p_workspace_id;
  foreach group_name_value in array array['planning','time_tracking','reports','shared_settings'] loop
    for c in select * from timegenie.workspace_snapshot_entity_catalog() catalog where catalog.group_name=group_name_value order by sort_order loop
      rows_value:=coalesce(target->group_name_value->c.entity_name,'[]'::jsonb);
      for row_value in select value from jsonb_array_elements(rows_value) loop
        execute format('insert into timegenie.%I select * from jsonb_populate_record(null::timegenie.%I,$1)',c.entity_name,c.entity_name) using row_value;
      end loop;
    end loop;
  end loop;
  update timegenie.time_entries set state='paused',ended_at=null where workspace_id=p_workspace_id and state='running';
  update timegenie.time_segments set ended_at=coalesce(ended_at,started_at),duration_seconds=coalesce(duration_seconds,0) where workspace_id=p_workspace_id and ended_at is null;
  update timegenie.unassigned_segments set ended_at=coalesce(ended_at,started_at),duration_seconds=coalesce(duration_seconds,0),lease_token=null where workspace_id=p_workspace_id and ended_at is null;
  update timegenie.unassigned_sessions set state='awaiting_resolution',last_ended_at=coalesce(last_ended_at,last_continuous_at,first_started_at),prompted_at=coalesce(prompted_at,clock_timestamp()),updated_at=clock_timestamp() where workspace_id=p_workspace_id and state='collecting';
  new_epoch:=current_epoch+1;
  update timegenie.workspaces set workspace_epoch=new_epoch,updated_at=now() where id=p_workspace_id;
  insert into timegenie.workspace_changes(workspace_id,entity_type,entity_id,operation,entity_version,changed_by_device_id) values(p_workspace_id,'workspace_reset',p_workspace_id::text,'update',new_epoch,p_device_id) returning change_seq into reset_seq;
  result_value:=jsonb_build_object('restored',true,'targetSnapshotId',p_target_snapshot_id,'safetySnapshotId',lock_value.safety_snapshot_id,'previousEpoch',current_epoch,'workspaceEpoch',new_epoch,'resetChangeSeq',reset_seq);
  perform timegenie.record_processed_operation(p_workspace_id,p_operation_id,p_device_id,'workspace.restore',jsonb_build_object('entityType','workspace','entityId',p_workspace_id,'snapshotId',p_target_snapshot_id,'targetHash',p_target_content_hash,'preparedEpoch',p_prepared_epoch),result_value);
  delete from timegenie.workspace_restore_locks where workspace_id=p_workspace_id and lock_token=p_lock_token;
  return result_value;
end $$;

revoke execute on function timegenie.cloud_snapshot_get_pre_task_day_plans(uuid) from public,anon,authenticated;
revoke execute on function timegenie.cloud_incremental_entity_get_pre_task_day_plans(uuid,text,text) from public,anon,authenticated;
revoke execute on function timegenie.migration_import_snapshot_pre_task_day_plans(uuid,uuid,uuid,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.apply_task_day_plan(uuid,uuid,uuid,text,text,bigint,bigint,bigint,jsonb,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.cloud_snapshot_get(uuid) from public,anon;
revoke execute on function timegenie.cloud_incremental_entity_get(uuid,text,text) from public,anon,authenticated;
revoke execute on function timegenie.migration_import_snapshot(uuid,uuid,uuid,jsonb) from public,anon;
grant execute on function timegenie.cloud_snapshot_get(uuid) to authenticated;
grant execute on function timegenie.migration_import_snapshot(uuid,uuid,uuid,jsonb) to authenticated;
grant execute on function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,jsonb) to authenticated;
grant execute on function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,bigint,bigint,jsonb) to authenticated;
grant execute on function timegenie.timer_transition_apply(uuid,uuid,uuid,jsonb) to authenticated;
grant execute on function timegenie.workspace_restore_commit(uuid,uuid,uuid,uuid,text,bigint,uuid,boolean) to authenticated;

commit;
notify pgrst,'reload schema';
