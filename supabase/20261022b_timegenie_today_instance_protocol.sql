-- Preserve today-task instance identity across every cloud write/import path.
begin;

create or replace function timegenie.validate_task_day_instance_source() returns trigger
language plpgsql set search_path=timegenie,pg_catalog as $$
begin
  if new.day_instance_date is not null and (
    new.day_instance_source_task_id is null or not exists(
      select 1 from timegenie.tasks source
      where source.id=new.day_instance_source_task_id
        and source.workspace_id=new.workspace_id
        and source.subject_id=new.subject_id
        and source.day_instance_date is null
    )
  ) then
    raise exception 'day instance source must be an ordinary task in the same workspace and subject';
  end if;
  return new;
end $$;

drop trigger if exists trg_tasks_day_instance_source on timegenie.tasks;
create trigger trg_tasks_day_instance_source
before insert or update of workspace_id,subject_id,day_instance_date,day_instance_source_task_id
on timegenie.tasks for each row execute function timegenie.validate_task_day_instance_source();

create or replace function timegenie.validate_task_day_plan_instance() returns trigger
language plpgsql set search_path=timegenie,pg_catalog as $$
begin
  if new.instance_task_id is not null and (
    new.state<>'included' or not exists(
      select 1 from timegenie.tasks instance
      where instance.id=new.instance_task_id
        and instance.workspace_id=new.workspace_id
        and instance.day_instance_source_task_id=new.task_id
        and instance.day_instance_date=new.work_date
        and instance.day_instance_kind='member'
        and instance.deleted_at is null
    )
  ) then
    raise exception 'task day plan instance must match its source task and work date';
  end if;
  return new;
end $$;

drop trigger if exists trg_task_day_plans_instance on timegenie.task_day_plans;
create trigger trg_task_day_plans_instance
before insert or update of workspace_id,task_id,work_date,state,instance_task_id
on timegenie.task_day_plans for each row execute function timegenie.validate_task_day_plan_instance();

create or replace function timegenie.apply_today_task_patch(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_operation_type text,
  p_entity_id text,p_base_version bigint,p_payload_version bigint,p_coalesced_count bigint,
  p_payload jsonb,p_request jsonb
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare row_data jsonb:=coalesce(p_payload,'{}'::jsonb); current_version bigint; cached jsonb; result_value jsonb;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  cached:=timegenie.processed_operation_result(p_workspace_id,p_operation_id,p_operation_type,p_request);
  if cached is not null then return cached; end if;
  if p_entity_id is null or row_data->>'id' is distinct from p_entity_id then
    raise exception 'VALIDATION_ERROR: entity id does not match payload id';
  end if;
  if p_payload_version is null or (row_data->>'version')::bigint is distinct from p_payload_version then
    raise exception 'VERSION_CONFLICT: declared payload version mismatch';
  end if;
  if p_coalesced_count is null or p_coalesced_count<1 then
    raise exception 'VERSION_CONFLICT: coalesced operation count must be positive';
  end if;
  if (p_base_version is null and p_payload_version<>p_coalesced_count)
     or (p_base_version is not null and p_payload_version<>p_base_version+p_coalesced_count) then
    raise exception 'VERSION_CONFLICT: task payload version does not match operation span';
  end if;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':task:'||p_entity_id,0));
  select version into current_version from timegenie.tasks where workspace_id=p_workspace_id and id=p_entity_id::uuid;
  if p_base_version is null and current_version is not null then
    raise exception 'SYNC_CONFLICT: expected new entity, current version %',current_version;
  elsif p_base_version is not null and current_version is distinct from p_base_version then
    raise exception 'SYNC_CONFLICT: expected version %, current version %',p_base_version,current_version;
  end if;
  insert into timegenie.tasks(
    id,workspace_id,subject_id,parent_id,title,status,planned_date,planned_time,estimate_minutes,note,
    project_name,solution_name,source_type,source_ref,sort_order,completed_at,created_at,updated_at,version,
    created_by_device_id,updated_by_device_id,deleted_at,day_instance_date,day_instance_kind,day_instance_source_task_id
  ) values(
    p_entity_id::uuid,p_workspace_id,(row_data->>'subject_id')::uuid,nullif(row_data->>'parent_id','')::uuid,
    row_data->>'title',row_data->>'status',nullif(row_data->>'planned_date','')::date,nullif(row_data->>'planned_time','')::time,
    nullif(row_data->>'estimate_minutes','')::integer,row_data->>'note',row_data->>'project_name',row_data->>'solution_name',
    case when row_data->>'source_type' in('manual','obsidian_import','timer_quick_create','migration') then row_data->>'source_type' else 'manual' end,
    row_data->>'source_ref',coalesce((row_data->>'sort_order')::integer,0),
    case when row_data->>'completed_at' is null then null else to_timestamp((row_data->>'completed_at')::double precision/1000) end,
    to_timestamp(coalesce((row_data->>'created_at')::double precision,extract(epoch from clock_timestamp())*1000)/1000),
    to_timestamp(coalesce((row_data->>'updated_at')::double precision,extract(epoch from clock_timestamp())*1000)/1000),p_payload_version,
    p_device_id,p_device_id,case when row_data->>'deleted_at' is null then null else to_timestamp((row_data->>'deleted_at')::double precision/1000) end,
    nullif(row_data->>'day_instance_date','')::date,nullif(row_data->>'day_instance_kind',''),
    nullif(row_data->>'day_instance_source_task_id','')::uuid
  ) on conflict(id) do update set
    subject_id=excluded.subject_id,parent_id=excluded.parent_id,title=excluded.title,status=excluded.status,
    planned_date=excluded.planned_date,planned_time=excluded.planned_time,estimate_minutes=excluded.estimate_minutes,
    note=excluded.note,project_name=excluded.project_name,solution_name=excluded.solution_name,source_ref=excluded.source_ref,
    sort_order=excluded.sort_order,completed_at=excluded.completed_at,updated_at=excluded.updated_at,version=excluded.version,
    updated_by_device_id=p_device_id,deleted_at=excluded.deleted_at,day_instance_date=excluded.day_instance_date,
    day_instance_kind=excluded.day_instance_kind,day_instance_source_task_id=excluded.day_instance_source_task_id;
  insert into timegenie.task_status_events(id,workspace_id,task_id,status,occurred_at,source_type,created_at)
  select (item->>'id')::uuid,p_workspace_id,p_entity_id::uuid,item->>'status',
    to_timestamp((item->>'occurred_at')::double precision/1000),item->>'source_type',
    to_timestamp((item->>'created_at')::double precision/1000)
  from jsonb_array_elements(coalesce(row_data->'status_events','[]'::jsonb)) item
  on conflict(id) do nothing;
  result_value:=jsonb_build_object(
    'entityType','task','entityId',p_entity_id,'version',p_payload_version,
    'authoritative',timegenie.cloud_incremental_entity_get(p_workspace_id,'tasks',p_entity_id)->'data'
  );
  perform timegenie.record_processed_operation(p_workspace_id,p_operation_id,p_device_id,p_operation_type,p_request,result_value);
  return result_value;
end $$;

create or replace function timegenie.cloud_apply_patch_pre_subject_work_schedules(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_operation_type text,
  p_entity_type text,p_entity_id text,p_base_version bigint,p_payload jsonb
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare request_value jsonb;
begin
  request_value:=jsonb_build_object('deviceId',p_device_id,'entityType',p_entity_type,'entityId',p_entity_id,
    'baseVersion',p_base_version,'payload',coalesce(p_payload,'{}'::jsonb));
  if p_entity_type='task' then
    return timegenie.apply_today_task_patch(p_workspace_id,p_device_id,p_operation_id,p_operation_type,
      p_entity_id,p_base_version,(p_payload->>'version')::bigint,1,p_payload,request_value);
  end if;
  if p_entity_type='task_day_plan' then
    return timegenie.apply_task_day_plan(p_workspace_id,p_device_id,p_operation_id,p_operation_type,
      p_entity_id,p_base_version,(p_payload->>'version')::bigint,1,p_payload,request_value);
  end if;
  if p_entity_type='report_template' then
    return timegenie.apply_named_report_template(p_workspace_id,p_device_id,p_operation_id,p_operation_type,
      p_entity_id,p_base_version,p_payload,request_value);
  end if;
  if p_entity_type='unassigned_session'
     and p_operation_type in('unassigned_session_pause','unassigned_session_resume','unassigned_session_awaiting_resolution') then
    return timegenie.cloud_apply_simplified_unassigned(p_workspace_id,p_device_id,p_operation_id,p_operation_type,
      p_entity_id,p_base_version,p_payload);
  end if;
  if p_entity_type='time_entry' and coalesce(p_payload->>'source_type','')='timer' then
    return timegenie.apply_calendar_time_entry(p_workspace_id,p_device_id,p_operation_id,p_operation_type,
      p_entity_id,p_base_version,p_payload,request_value);
  end if;
  return timegenie.cloud_apply_patch_pre_calendar(p_workspace_id,p_device_id,p_operation_id,p_operation_type,
    p_entity_type,p_entity_id,p_base_version,p_payload);
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
    and id=(row_data->>'task_id')::uuid and deleted_at is null and day_instance_date is null) then
    raise exception 'NOT_FOUND: source task does not exist';
  end if;
  if coalesce(row_data->>'state','') not in('included','excluded') then raise exception 'VALIDATION_ERROR: invalid task day plan state'; end if;
  if row_data->>'state'='excluded' and nullif(row_data->>'instance_task_id','') is not null then
    raise exception 'VALIDATION_ERROR: excluded task day plan cannot reference an instance';
  end if;
  if p_payload_version is null or (row_data->>'version')::bigint is distinct from p_payload_version then
    raise exception 'VERSION_CONFLICT: declared payload version mismatch';
  end if;
  if p_coalesced_count is null or p_coalesced_count<1 then raise exception 'VERSION_CONFLICT: coalesced operation count must be positive'; end if;
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
  insert into timegenie.task_day_plans(workspace_id,task_id,work_date,state,instance_task_id,created_at,updated_at,version)
  values(p_workspace_id,(row_data->>'task_id')::uuid,(row_data->>'work_date')::date,row_data->>'state',
    nullif(row_data->>'instance_task_id','')::uuid,
    to_timestamp(coalesce((row_data->>'created_at')::double precision,extract(epoch from clock_timestamp())*1000)/1000),
    to_timestamp(coalesce((row_data->>'updated_at')::double precision,extract(epoch from clock_timestamp())*1000)/1000),p_payload_version)
  on conflict(task_id,work_date) do update set state=excluded.state,instance_task_id=excluded.instance_task_id,
    updated_at=excluded.updated_at,version=excluded.version;
  result_value:=jsonb_build_object(
    'entityType','task_day_plan','entityId',p_entity_id,'version',p_payload_version,
    'authoritative',timegenie.cloud_incremental_entity_get(p_workspace_id,'task_day_plans',p_entity_id)->'data'
  );
  perform timegenie.record_processed_operation(p_workspace_id,p_operation_id,p_device_id,p_operation_type,p_request,result_value);
  return result_value;
end $$;

create or replace function timegenie.cloud_apply_patch_pre_subject_work_schedules(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_operation_type text,
  p_entity_type text,p_entity_id text,p_base_version bigint,p_payload_version bigint,
  p_coalesced_count bigint,p_payload jsonb
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare request_value jsonb;
begin
  if p_payload_version is null or (p_payload->>'version')::bigint is distinct from p_payload_version
     or p_coalesced_count is null or p_coalesced_count<1 then
    raise exception 'VERSION_CONFLICT: invalid coalesced payload';
  end if;
  request_value:=jsonb_build_object('deviceId',p_device_id,'entityType',p_entity_type,'entityId',p_entity_id,
    'baseVersion',p_base_version,'payloadVersion',p_payload_version,'coalescedCount',p_coalesced_count,
    'payload',coalesce(p_payload,'{}'::jsonb));
  if p_entity_type='task' then
    return timegenie.apply_today_task_patch(p_workspace_id,p_device_id,p_operation_id,p_operation_type,
      p_entity_id,p_base_version,p_payload_version,p_coalesced_count,p_payload,request_value);
  end if;
  if p_entity_type='task_day_plan' then
    return timegenie.apply_task_day_plan(p_workspace_id,p_device_id,p_operation_id,p_operation_type,
      p_entity_id,p_base_version,p_payload_version,p_coalesced_count,p_payload,request_value);
  end if;
  if p_entity_type='report_template' then
    return timegenie.apply_named_report_template(p_workspace_id,p_device_id,p_operation_id,p_operation_type,
      p_entity_id,p_base_version,p_payload,request_value);
  end if;
  if p_entity_type='unassigned_session'
     and p_operation_type in('unassigned_session_pause','unassigned_session_resume','unassigned_session_awaiting_resolution') then
    return timegenie.cloud_apply_simplified_unassigned(p_workspace_id,p_device_id,p_operation_id,p_operation_type,
      p_entity_id,p_base_version,p_payload);
  end if;
  if p_entity_type='time_entry' and coalesce(p_payload->>'source_type','')='timer' then
    if (p_base_version is null and p_payload_version<>p_coalesced_count)
       or (p_base_version is not null and p_payload_version<>p_base_version+p_coalesced_count) then
      raise exception 'VERSION_CONFLICT: timer payload version does not match operation span';
    end if;
    return timegenie.apply_calendar_time_entry(p_workspace_id,p_device_id,p_operation_id,p_operation_type,
      p_entity_id,p_base_version,p_payload,request_value);
  end if;
  return timegenie.cloud_apply_patch_pre_calendar(p_workspace_id,p_device_id,p_operation_id,p_operation_type,
    p_entity_type,p_entity_id,p_base_version,p_payload_version,p_coalesced_count,p_payload);
end $$;

do $$ begin
  if to_regprocedure('timegenie.migration_import_snapshot_pre_today_instance_protocol(uuid,uuid,uuid,jsonb)') is null then
    alter function timegenie.migration_import_snapshot(uuid,uuid,uuid,jsonb)
      rename to migration_import_snapshot_pre_today_instance_protocol;
  end if;
end $$;

create or replace function timegenie.migration_import_snapshot(
  p_workspace_id uuid,p_operation_id uuid,p_device_id uuid,p_snapshot jsonb
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare result_value jsonb; row_data jsonb;
begin
  result_value:=timegenie.migration_import_snapshot_pre_today_instance_protocol(
    p_workspace_id,p_operation_id,p_device_id,p_snapshot
  );
  update timegenie.tasks target set parent_id=null
  where target.workspace_id=p_workspace_id and target.id in(
    select (value->>'id')::uuid from jsonb_array_elements(coalesce(p_snapshot->'tasks','[]'::jsonb)) value
    where nullif(value->>'day_instance_date','') is not null
  );
  for row_data in select value from jsonb_array_elements(coalesce(p_snapshot->'tasks','[]'::jsonb)) loop
    update timegenie.tasks set
      day_instance_date=nullif(row_data->>'day_instance_date','')::date,
      day_instance_kind=nullif(row_data->>'day_instance_kind',''),
      day_instance_source_task_id=nullif(row_data->>'day_instance_source_task_id','')::uuid
    where workspace_id=p_workspace_id and id=(row_data->>'id')::uuid;
  end loop;
  for row_data in select value from jsonb_array_elements(coalesce(p_snapshot->'tasks','[]'::jsonb)) loop
    update timegenie.tasks set parent_id=nullif(row_data->>'parent_id','')::uuid
    where workspace_id=p_workspace_id and id=(row_data->>'id')::uuid;
  end loop;
  for row_data in select value from jsonb_array_elements(coalesce(p_snapshot->'task_day_plans','[]'::jsonb)) loop
    update timegenie.task_day_plans set instance_task_id=nullif(row_data->>'instance_task_id','')::uuid
    where workspace_id=p_workspace_id and task_id=(row_data->>'task_id')::uuid
      and work_date=(row_data->>'work_date')::date;
  end loop;
  return result_value;
end $$;

revoke execute on function timegenie.validate_task_day_instance_source() from public,anon,authenticated;
revoke execute on function timegenie.validate_task_day_plan_instance() from public,anon,authenticated;
revoke execute on function timegenie.apply_today_task_patch(uuid,uuid,uuid,text,text,bigint,bigint,bigint,jsonb,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.cloud_apply_patch_pre_subject_work_schedules(uuid,uuid,uuid,text,text,text,bigint,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.cloud_apply_patch_pre_subject_work_schedules(uuid,uuid,uuid,text,text,text,bigint,bigint,bigint,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.migration_import_snapshot_pre_today_instance_protocol(uuid,uuid,uuid,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.apply_task_day_plan(uuid,uuid,uuid,text,text,bigint,bigint,bigint,jsonb,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.migration_import_snapshot(uuid,uuid,uuid,jsonb) from public,anon;
grant execute on function timegenie.migration_import_snapshot(uuid,uuid,uuid,jsonb) to authenticated;

commit;
notify pgrst,'reload schema';
