begin;

-- Subject weekly work schedules. A schedule row is the conflict and change-log
-- unit; its child segments are always replaced as one complete image.
create table if not exists timegenie.subject_work_schedules (
  id uuid primary key default gen_random_uuid(),
  workspace_id uuid not null references timegenie.workspaces(id) on delete cascade,
  subject_id uuid not null references timegenie.subjects(id) on delete cascade,
  weekday smallint not null check (weekday between 0 and 6),
  is_workday boolean not null,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  version bigint not null default 1,
  unique(workspace_id,subject_id,weekday)
);
create index if not exists idx_subject_work_schedules_subject
  on timegenie.subject_work_schedules(workspace_id,subject_id,weekday);

create table if not exists timegenie.subject_work_schedule_segments (
  id uuid primary key,
  workspace_id uuid not null references timegenie.workspaces(id) on delete cascade,
  schedule_id uuid not null references timegenie.subject_work_schedules(id) on delete cascade,
  sequence_no integer not null check (sequence_no > 0),
  start_minute smallint not null check (start_minute between 0 and 1439),
  end_minute smallint not null check (end_minute between 1 and 1440),
  check (start_minute < end_minute),
  unique(schedule_id,sequence_no)
);
create index if not exists idx_subject_work_schedule_segments_schedule
  on timegenie.subject_work_schedule_segments(workspace_id,schedule_id,sequence_no);

alter table timegenie.subject_work_schedules enable row level security;
alter table timegenie.subject_work_schedule_segments enable row level security;
drop policy if exists workspace_owner on timegenie.subject_work_schedules;
create policy workspace_owner on timegenie.subject_work_schedules for all
using(timegenie.is_workspace_owner(workspace_id)) with check(timegenie.is_workspace_owner(workspace_id));
drop policy if exists workspace_owner on timegenie.subject_work_schedule_segments;
create policy workspace_owner on timegenie.subject_work_schedule_segments for all
using(timegenie.is_workspace_owner(workspace_id)) with check(timegenie.is_workspace_owner(workspace_id));

insert into timegenie.subject_work_schedules(workspace_id,subject_id,weekday,is_workday,created_at,updated_at,version)
select subject.workspace_id,subject.id,weekday.value,weekday.value<5,subject.created_at,subject.updated_at,1
from timegenie.subjects subject cross join generate_series(0,6) weekday(value)
where subject.deleted_at is null
on conflict(workspace_id,subject_id,weekday) do nothing;
insert into timegenie.subject_work_schedule_segments(id,workspace_id,schedule_id,sequence_no,start_minute,end_minute)
select gen_random_uuid(),schedule.workspace_id,schedule.id,segment.sequence_no,segment.start_minute,segment.end_minute
from timegenie.subject_work_schedules schedule
cross join (values (1,600,720),(2,810,1050)) segment(sequence_no,start_minute,end_minute)
where schedule.weekday<5 and schedule.is_workday
  and not exists(select 1 from timegenie.subject_work_schedule_segments existing where existing.schedule_id=schedule.id)
on conflict(schedule_id,sequence_no) do nothing;

drop trigger if exists workspace_change_trigger on timegenie.subject_work_schedules;
create trigger workspace_change_trigger after insert or update or delete on timegenie.subject_work_schedules
for each row execute function timegenie.touch_workspace_change();

do $$ begin
  if to_regprocedure('timegenie.workspace_initialize_defaults_pre_subject_work_schedules(uuid)') is null then
    alter function timegenie.workspace_initialize_defaults(uuid) rename to workspace_initialize_defaults_pre_subject_work_schedules;
  end if;
  if to_regprocedure('timegenie.cloud_snapshot_get_pre_subject_work_schedules(uuid)') is null then
    alter function timegenie.cloud_snapshot_get(uuid) rename to cloud_snapshot_get_pre_subject_work_schedules;
  end if;
  if to_regprocedure('timegenie.cloud_incremental_entity_get_pre_subject_work_schedules(uuid,text,text)') is null then
    alter function timegenie.cloud_incremental_entity_get(uuid,text,text) rename to cloud_incremental_entity_get_pre_subject_work_schedules;
  end if;
  if to_regprocedure('timegenie.migration_import_snapshot_pre_subject_work_schedules(uuid,uuid,uuid,jsonb)') is null then
    alter function timegenie.migration_import_snapshot(uuid,uuid,uuid,jsonb) rename to migration_import_snapshot_pre_subject_work_schedules;
  end if;
  if to_regprocedure('timegenie.cloud_apply_patch_pre_subject_work_schedules(uuid,uuid,uuid,text,text,text,bigint,jsonb)') is null then
    alter function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,jsonb) rename to cloud_apply_patch_pre_subject_work_schedules;
  end if;
  if to_regprocedure('timegenie.cloud_apply_patch_pre_subject_work_schedules(uuid,uuid,uuid,text,text,text,bigint,bigint,bigint,jsonb)') is null then
    alter function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,bigint,bigint,jsonb) rename to cloud_apply_patch_pre_subject_work_schedules;
  end if;
end $$;

create or replace function timegenie.workspace_initialize_defaults(p_workspace_id uuid)
returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare result_value jsonb; default_subject_id uuid;
begin
  result_value:=timegenie.workspace_initialize_defaults_pre_subject_work_schedules(p_workspace_id);
  default_subject_id:=(result_value->>'defaultSubjectId')::uuid;
  insert into timegenie.subject_work_schedules(workspace_id,subject_id,weekday,is_workday)
  select p_workspace_id,default_subject_id,weekday.value,weekday.value<5 from generate_series(0,6) weekday(value)
  on conflict(workspace_id,subject_id,weekday) do nothing;
  insert into timegenie.subject_work_schedule_segments(id,workspace_id,schedule_id,sequence_no,start_minute,end_minute)
  select gen_random_uuid(),schedule.workspace_id,schedule.id,segment.sequence_no,segment.start_minute,segment.end_minute
  from timegenie.subject_work_schedules schedule
  cross join (values (1,600,720),(2,810,1050)) segment(sequence_no,start_minute,end_minute)
  where schedule.workspace_id=p_workspace_id and schedule.subject_id=default_subject_id
    and schedule.weekday<5 and schedule.is_workday
    and not exists(select 1 from timegenie.subject_work_schedule_segments existing where existing.schedule_id=schedule.id)
  on conflict(schedule_id,sequence_no) do nothing;
  return result_value;
end $$;

create or replace function timegenie.cloud_snapshot_get(p_workspace_id uuid)
returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare result_value jsonb;
begin
  result_value:=timegenie.cloud_snapshot_get_pre_subject_work_schedules(p_workspace_id);
  return result_value || jsonb_build_object(
    'subject_work_schedules',coalesce((select jsonb_agg(to_jsonb(schedule) || jsonb_build_object(
      'segments',coalesce((select jsonb_agg(to_jsonb(segment) order by sequence_no,id) from timegenie.subject_work_schedule_segments segment where segment.workspace_id=p_workspace_id and segment.schedule_id=schedule.id),'[]'::jsonb)
    ) order by subject_id,weekday) from timegenie.subject_work_schedules schedule where workspace_id=p_workspace_id),'[]'::jsonb),
    'subject_work_schedule_segments',coalesce((select jsonb_agg(to_jsonb(row_value) order by schedule_id,sequence_no) from timegenie.subject_work_schedule_segments row_value where workspace_id=p_workspace_id),'[]'::jsonb)
  );
end $$;

create or replace function timegenie.cloud_incremental_entity_get(p_workspace_id uuid,p_entity_type text,p_entity_id text)
returns jsonb language plpgsql stable security definer set search_path=timegenie,extensions,pg_catalog as $$
declare entity_value jsonb;
begin
  if p_entity_type<>'subject_work_schedules' then
    return timegenie.cloud_incremental_entity_get_pre_subject_work_schedules(p_workspace_id,p_entity_type,p_entity_id);
  end if;
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  select to_jsonb(schedule) || jsonb_build_object(
    'segments',coalesce((select jsonb_agg(to_jsonb(segment) order by sequence_no,id) from timegenie.subject_work_schedule_segments segment where segment.workspace_id=p_workspace_id and segment.schedule_id=schedule.id),'[]'::jsonb)
  ) into entity_value from timegenie.subject_work_schedules schedule
  where schedule.workspace_id=p_workspace_id and schedule.id::text=p_entity_id;
  return jsonb_build_object('entity_type','subject_work_schedules','source_entity_type','subject_work_schedules','entity_id',p_entity_id,'deleted',entity_value is null,'data',entity_value);
end $$;

create or replace function timegenie.apply_subject_work_schedule(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_operation_type text,p_entity_id text,p_base_version bigint,p_payload jsonb,p_request jsonb
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare existing timegenie.subject_work_schedules%rowtype; cached jsonb; result_value jsonb; payload_version bigint; workday boolean; segment_count integer;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  cached:=timegenie.processed_operation_result(p_workspace_id,p_operation_id,p_operation_type,p_request); if cached is not null then return cached; end if;
  if p_entity_id is null or p_payload->>'id' is distinct from p_entity_id then raise exception 'VALIDATION_ERROR: entity id does not match payload id'; end if;
  if not exists(select 1 from timegenie.subjects where workspace_id=p_workspace_id and id=nullif(p_payload->>'subject_id','')::uuid and deleted_at is null) then raise exception 'VALIDATION_ERROR: subject does not belong to workspace'; end if;
  if nullif(p_payload->>'weekday','')::integer not between 0 and 6 then raise exception 'VALIDATION_ERROR: invalid weekday'; end if;
  if jsonb_typeof(p_payload->'is_workday')='boolean' then workday:=(p_payload->>'is_workday')::boolean; else workday:=coalesce((p_payload->>'is_workday')::integer,0)<>0; end if;
  if jsonb_typeof(coalesce(p_payload->'segments','null'::jsonb))<>'array' then raise exception 'VALIDATION_ERROR: schedule segments must be an array'; end if;
  segment_count:=jsonb_array_length(p_payload->'segments');
  if (workday and segment_count=0) or (not workday and segment_count<>0) then raise exception 'VALIDATION_ERROR: workday and segments do not match'; end if;
  if exists(select 1 from jsonb_array_elements(p_payload->'segments') with ordinality item(value,ordinality)
    where nullif(value->>'id','')::uuid is null or (value->>'start_minute')::integer not between 0 and 1439
      or (value->>'end_minute')::integer not between 1 and 1440 or (value->>'start_minute')::integer >= (value->>'end_minute')::integer
      or coalesce(nullif(value->>'sequence_no','')::integer,ordinality::integer)<>ordinality::integer
  ) then raise exception 'VALIDATION_ERROR: invalid schedule segment'; end if;
  if exists(select 1 from (select (value->>'start_minute')::integer start_minute,lag((value->>'end_minute')::integer) over(order by ordinality) previous_end from jsonb_array_elements(p_payload->'segments') with ordinality item(value,ordinality)) ordered where previous_end>start_minute) then raise exception 'VALIDATION_ERROR: overlapping schedule segments'; end if;
  select * into existing from timegenie.subject_work_schedules where workspace_id=p_workspace_id and id=p_entity_id::uuid for update;
  payload_version:=coalesce((p_payload->>'version')::bigint,1);
  if p_base_version is null and found then raise exception 'SYNC_CONFLICT: expected new entity'; end if;
  if p_base_version is not null and (not found or existing.version<>p_base_version) then raise exception 'SYNC_CONFLICT: schedule version changed'; end if;
  if p_base_version is null and payload_version<>1 then raise exception 'VERSION_CONFLICT: new schedule version must be 1'; end if;
  if p_base_version is not null and payload_version<>p_base_version+1 then raise exception 'VERSION_CONFLICT: schedule version must advance once'; end if;
  insert into timegenie.subject_work_schedules(id,workspace_id,subject_id,weekday,is_workday,created_at,updated_at,version)
  values(p_entity_id::uuid,p_workspace_id,(p_payload->>'subject_id')::uuid,(p_payload->>'weekday')::smallint,workday,to_timestamp(coalesce((p_payload->>'created_at')::double precision,extract(epoch from clock_timestamp())*1000)/1000),to_timestamp(coalesce((p_payload->>'updated_at')::double precision,extract(epoch from clock_timestamp())*1000)/1000),payload_version)
  on conflict(id) do update set subject_id=excluded.subject_id,weekday=excluded.weekday,is_workday=excluded.is_workday,updated_at=excluded.updated_at,version=excluded.version;
  delete from timegenie.subject_work_schedule_segments where schedule_id=p_entity_id::uuid;
  insert into timegenie.subject_work_schedule_segments(id,workspace_id,schedule_id,sequence_no,start_minute,end_minute)
  select (value->>'id')::uuid,p_workspace_id,p_entity_id::uuid,ordinality::integer,(value->>'start_minute')::smallint,(value->>'end_minute')::smallint
  from jsonb_array_elements(p_payload->'segments') with ordinality item(value,ordinality);
  result_value:=jsonb_build_object('entityType','subject_work_schedule','entityId',p_entity_id,'version',payload_version,'authoritative',timegenie.cloud_incremental_entity_get(p_workspace_id,'subject_work_schedules',p_entity_id)->'data');
  perform timegenie.record_processed_operation(p_workspace_id,p_operation_id,p_device_id,p_operation_type,p_request,result_value); return result_value;
end $$;

create or replace function timegenie.cloud_apply_patch(p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_operation_type text,p_entity_type text,p_entity_id text,p_base_version bigint,p_payload jsonb)
returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$ begin
  if p_entity_type='subject_work_schedule' then return timegenie.apply_subject_work_schedule(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_id,p_base_version,p_payload,jsonb_build_object('deviceId',p_device_id,'entityType',p_entity_type,'entityId',p_entity_id,'baseVersion',p_base_version,'payload',coalesce(p_payload,'{}'::jsonb))); end if;
  return timegenie.cloud_apply_patch_pre_subject_work_schedules(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_type,p_entity_id,p_base_version,p_payload);
end $$;

create or replace function timegenie.cloud_apply_patch(p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_operation_type text,p_entity_type text,p_entity_id text,p_base_version bigint,p_payload_version bigint,p_coalesced_count bigint,p_payload jsonb)
returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$ begin
  if p_payload_version is null or (p_payload->>'version')::bigint<>p_payload_version or p_coalesced_count is null or p_coalesced_count<1 then raise exception 'VERSION_CONFLICT: invalid coalesced payload'; end if;
  if p_entity_type='subject_work_schedule' then
    if p_coalesced_count<>1 or (p_base_version is null and p_payload_version<>1) or (p_base_version is not null and p_payload_version<>p_base_version+1) then raise exception 'VERSION_CONFLICT: invalid schedule operation span'; end if;
    return timegenie.apply_subject_work_schedule(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_id,p_base_version,p_payload,jsonb_build_object('deviceId',p_device_id,'entityType',p_entity_type,'entityId',p_entity_id,'baseVersion',p_base_version,'payloadVersion',p_payload_version,'coalescedCount',p_coalesced_count,'payload',coalesce(p_payload,'{}'::jsonb)));
  end if;
  return timegenie.cloud_apply_patch_pre_subject_work_schedules(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_type,p_entity_id,p_base_version,p_payload_version,p_coalesced_count,p_payload);
end $$;

create or replace function timegenie.migration_import_snapshot(p_workspace_id uuid,p_operation_id uuid,p_device_id uuid,p_snapshot jsonb)
returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare result_value jsonb; row_data jsonb; sanitized jsonb; cached jsonb;
begin
  sanitized:=jsonb_set(jsonb_set(coalesce(p_snapshot,'{}'::jsonb),'{subject_work_schedules}','[]'::jsonb,true),'{subject_work_schedule_segments}','[]'::jsonb,true);
  cached:=timegenie.processed_operation_result(p_workspace_id,p_operation_id,'migration_import_snapshot',jsonb_build_object('deviceId',p_device_id,'snapshot',coalesce(p_snapshot,'{}'::jsonb)));
  if cached is not null then return cached; end if;
  result_value:=timegenie.migration_import_snapshot_pre_subject_work_schedules(p_workspace_id,p_operation_id,p_device_id,sanitized);
  delete from timegenie.subject_work_schedules where workspace_id=p_workspace_id;
  for row_data in select value from jsonb_array_elements(coalesce(p_snapshot->'subject_work_schedules','[]'::jsonb)) loop
    insert into timegenie.subject_work_schedules(id,workspace_id,subject_id,weekday,is_workday,created_at,updated_at,version)
    values((row_data->>'id')::uuid,p_workspace_id,(row_data->>'subject_id')::uuid,(row_data->>'weekday')::smallint,case when jsonb_typeof(row_data->'is_workday')='boolean' then (row_data->>'is_workday')::boolean else coalesce((row_data->>'is_workday')::integer,0)<>0 end,to_timestamp((row_data->>'created_at')::double precision/1000),to_timestamp((row_data->>'updated_at')::double precision/1000),coalesce((row_data->>'version')::bigint,1));
  end loop;
  for row_data in select value from jsonb_array_elements(coalesce(p_snapshot->'subject_work_schedule_segments','[]'::jsonb)) loop
    insert into timegenie.subject_work_schedule_segments(id,workspace_id,schedule_id,sequence_no,start_minute,end_minute)
    values((row_data->>'id')::uuid,p_workspace_id,(row_data->>'schedule_id')::uuid,(row_data->>'sequence_no')::integer,(row_data->>'start_minute')::smallint,(row_data->>'end_minute')::smallint);
  end loop;
  insert into timegenie.subject_work_schedules(workspace_id,subject_id,weekday,is_workday)
  select p_workspace_id,subject.id,weekday.value,weekday.value<5
  from timegenie.subjects subject cross join generate_series(0,6) weekday(value)
  where subject.workspace_id=p_workspace_id and subject.deleted_at is null
  on conflict(workspace_id,subject_id,weekday) do nothing;
  insert into timegenie.subject_work_schedule_segments(id,workspace_id,schedule_id,sequence_no,start_minute,end_minute)
  select gen_random_uuid(),schedule.workspace_id,schedule.id,segment.sequence_no,segment.start_minute,segment.end_minute
  from timegenie.subject_work_schedules schedule
  cross join (values (1,600,720),(2,810,1050)) segment(sequence_no,start_minute,end_minute)
  where schedule.workspace_id=p_workspace_id and schedule.weekday<5 and schedule.is_workday
    and not exists(select 1 from timegenie.subject_work_schedule_segments existing where existing.schedule_id=schedule.id)
  on conflict(schedule_id,sequence_no) do nothing;
  update timegenie.processed_operations
  set request_hash=timegenie.operation_request_hash('migration_import_snapshot',jsonb_build_object('deviceId',p_device_id,'snapshot',coalesce(p_snapshot,'{}'::jsonb)))
  where workspace_id=p_workspace_id and operation_id=p_operation_id;
  return result_value;
end $$;

revoke all on timegenie.subject_work_schedules,timegenie.subject_work_schedule_segments from public,anon,authenticated;
revoke execute on function timegenie.workspace_initialize_defaults_pre_subject_work_schedules(uuid) from public,anon,authenticated;
revoke execute on function timegenie.cloud_snapshot_get_pre_subject_work_schedules(uuid) from public,anon,authenticated;
revoke execute on function timegenie.cloud_incremental_entity_get_pre_subject_work_schedules(uuid,text,text) from public,anon,authenticated;
revoke execute on function timegenie.migration_import_snapshot_pre_subject_work_schedules(uuid,uuid,uuid,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.cloud_apply_patch_pre_subject_work_schedules(uuid,uuid,uuid,text,text,text,bigint,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.cloud_apply_patch_pre_subject_work_schedules(uuid,uuid,uuid,text,text,text,bigint,bigint,bigint,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.apply_subject_work_schedule(uuid,uuid,uuid,text,text,bigint,jsonb,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.workspace_initialize_defaults(uuid) from public,anon;
revoke execute on function timegenie.cloud_snapshot_get(uuid) from public,anon;
revoke execute on function timegenie.cloud_incremental_entity_get(uuid,text,text) from public,anon,authenticated;
revoke execute on function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,jsonb) from public,anon;
revoke execute on function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,bigint,bigint,jsonb) from public,anon;
revoke execute on function timegenie.migration_import_snapshot(uuid,uuid,uuid,jsonb) from public,anon;
grant execute on function timegenie.workspace_initialize_defaults(uuid) to authenticated;
grant execute on function timegenie.cloud_snapshot_get(uuid) to authenticated;
grant execute on function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,jsonb) to authenticated;
grant execute on function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,bigint,bigint,jsonb) to authenticated;
grant execute on function timegenie.migration_import_snapshot(uuid,uuid,uuid,jsonb) to authenticated;

commit;
notify pgrst, 'reload schema';
