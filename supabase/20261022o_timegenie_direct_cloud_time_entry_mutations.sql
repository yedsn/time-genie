-- Direct-v2 corrections and disposition changes for ended time entries.
-- These mutations preserve the existing desktop command contract while
-- keeping the authoritative entry, segments, allocations and work-day
-- settlement state in one cloud transaction.

begin;

create or replace function timegenie.direct_cloud_time_history_page(
  p_workspace_id uuid,p_device_id uuid,p_page integer default 1,
  p_include_breaks boolean default true,p_expected_epoch bigint default null
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare total_days bigint; total_pages bigint; actual_page integer; before_date date;
declare response_value jsonb;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  if p_page<1 then raise exception 'VALIDATION_ERROR: 历史记录页码必须从 1 开始'; end if;
  select count(*) into total_days from (
    select entry.work_date from timegenie.time_entries entry
    where entry.workspace_id=p_workspace_id and entry.deleted_at is null
      and (p_include_breaks or entry.kind<>'break')
    union
    select adjustment.work_date from timegenie.actual_time_adjustments adjustment
    where adjustment.workspace_id=p_workspace_id and adjustment.deleted_at is null
  ) dates;
  total_pages:=case when total_days=0 then 0 else ((total_days+6)/7) end;
  actual_page:=case when total_pages=0 then 1 else least(p_page,total_pages::integer) end;
  if actual_page>1 then
    select work_date into before_date from (
      select entry.work_date from timegenie.time_entries entry
      where entry.workspace_id=p_workspace_id and entry.deleted_at is null
        and (p_include_breaks or entry.kind<>'break')
      union
      select adjustment.work_date from timegenie.actual_time_adjustments adjustment
      where adjustment.workspace_id=p_workspace_id and adjustment.deleted_at is null
    ) dates order by work_date desc offset ((actual_page-1)*7-1) limit 1;
  end if;
  response_value:=timegenie.direct_cloud_time_history(
    p_workspace_id,p_device_id,p_expected_epoch,before_date,7,p_include_breaks
  );
  response_value:=jsonb_set(response_value,'{data,page}',to_jsonb(actual_page),true);
  response_value:=jsonb_set(response_value,'{data,pageSize}','7'::jsonb,true);
  response_value:=jsonb_set(response_value,'{data,totalDays}',to_jsonb(total_days),true);
  response_value:=jsonb_set(response_value,'{data,totalPages}',to_jsonb(total_pages),true);
  return response_value;
end $$;

create or replace function timegenie.direct_cloud_time_entry_update(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; entry_row timegenie.time_entries%rowtype;
declare entry_id uuid; expected_version bigint; started_at_value timestamptz; ended_at_value timestamptz;
declare duration_seconds bigint; current_date_value date; now_value timestamptz:=clock_timestamp();
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(
    p_workspace_id,p_operation_id,'direct_time_entry_update',request_value
  );
  if cached is not null then return cached; end if;

  entry_id:=nullif(p_payload->>'entryId','')::uuid;
  expected_version:=nullif(p_payload->>'expectedVersion','')::bigint;
  started_at_value:=to_timestamp((p_payload->>'startedAt')::double precision/1000);
  ended_at_value:=to_timestamp((p_payload->>'endedAt')::double precision/1000);
  if entry_id is null or expected_version is null
     or p_payload->>'startedAt' is null or p_payload->>'endedAt' is null
     or ended_at_value<=started_at_value then
    raise exception 'VALIDATION_ERROR: 结束时间必须晚于开始时间';
  end if;

  select entry.* into entry_row from timegenie.time_entries entry
  where entry.workspace_id=p_workspace_id and entry.id=entry_id for update;
  if not found then raise exception 'NOT_FOUND: 时间记录不存在'; end if;
  if entry_row.deleted_at is not null then raise exception 'ENTITY_DELETED: 时间记录已删除'; end if;
  if entry_row.version<>expected_version then
    raise exception 'VERSION_CONFLICT: 时间记录已被更新'
      using detail=jsonb_build_object('code','VERSION_CONFLICT','entryId',entry_id)::text;
  end if;
  if entry_row.state<>'ended' then raise exception 'VALIDATION_ERROR: 只能修正已结束的时间记录'; end if;
  select (now_value at time zone workspace.timezone)::date into current_date_value
  from timegenie.workspaces workspace where workspace.id=p_workspace_id;
  if entry_row.work_date<>current_date_value then
    raise exception 'HISTORICAL_RECORD_READ_ONLY: % 的历史记录不可修改（当前工作日期为 %）',
      entry_row.work_date,current_date_value;
  end if;

  duration_seconds:=floor(extract(epoch from(ended_at_value-started_at_value)))::bigint;
  update timegenie.time_entries entry set
    started_at=started_at_value,ended_at=ended_at_value,duration_seconds=duration_seconds,
    note=nullif(p_payload->>'note',''),updated_at=now_value,updated_by_device_id=p_device_id,
    version=entry.version+1
  where entry.workspace_id=p_workspace_id and entry.id=entry_id;
  delete from timegenie.time_segments segment
  where segment.workspace_id=p_workspace_id and segment.entry_id=entry_id;
  insert into timegenie.time_segments(
    id,workspace_id,entry_id,sequence_no,started_at,ended_at,duration_seconds
  ) values(
    timegenie.calendar_stable_uuid('direct-time-entry-update-segment:'||p_operation_id::text),
    p_workspace_id,entry_id,1,started_at_value,ended_at_value,duration_seconds
  );
  update timegenie.work_days day set settled_at=null,updated_at=now_value,version=day.version+1
  where day.workspace_id=p_workspace_id and day.work_date=entry_row.work_date;

  result_value:=timegenie.direct_cloud_mutation_response(
    p_workspace_id,p_device_id,p_operation_id,
    jsonb_build_object('entry',timegenie.direct_cloud_time_entry_project(p_workspace_id,entry_id,now_value)),
    array['time'],p_expected_epoch
  );
  perform timegenie.record_direct_cloud_operation(
    p_workspace_id,p_operation_id,p_device_id,'direct_time_entry_update',request_value,result_value
  );
  return result_value;
end $$;

create or replace function timegenie.direct_cloud_time_entry_disposition(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; entry_result jsonb;
declare entry_row timegenie.time_entries%rowtype; entry_id uuid; expected_version bigint;
declare disposition_value text; current_date_value date; now_value timestamptz:=clock_timestamp();
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(
    p_workspace_id,p_operation_id,'direct_time_entry_disposition',request_value
  );
  if cached is not null then return cached; end if;

  entry_id:=nullif(p_payload->>'entryId','')::uuid;
  expected_version:=nullif(p_payload->>'expectedVersion','')::bigint;
  disposition_value:=coalesce(p_payload->>'disposition','');
  if entry_id is null or expected_version is null or disposition_value not in ('break','discard') then
    raise exception 'VALIDATION_ERROR: 处理方式必须是休息或无效';
  end if;
  select entry.* into entry_row from timegenie.time_entries entry
  where entry.workspace_id=p_workspace_id and entry.id=entry_id for update;
  if not found then raise exception 'NOT_FOUND: 时间记录不存在'; end if;
  if entry_row.deleted_at is not null then raise exception 'ENTITY_DELETED: 时间记录已删除'; end if;
  if entry_row.version<>expected_version then
    raise exception 'VERSION_CONFLICT: 时间记录已被更新'
      using detail=jsonb_build_object('code','VERSION_CONFLICT','entryId',entry_id)::text;
  end if;
  if entry_row.state<>'ended' then raise exception 'VALIDATION_ERROR: 只能处理已结束的时间记录'; end if;
  select (now_value at time zone workspace.timezone)::date into current_date_value
  from timegenie.workspaces workspace where workspace.id=p_workspace_id;
  if entry_row.work_date<>current_date_value then
    raise exception 'HISTORICAL_RECORD_READ_ONLY: % 的历史记录不可修改（当前工作日期为 %）',
      entry_row.work_date,current_date_value;
  end if;

  delete from timegenie.time_allocations allocation
  where allocation.workspace_id=p_workspace_id and allocation.entry_id=entry_id;
  if disposition_value='break' then
    update timegenie.time_entries entry set
      kind='break',default_task_id=null,label_snapshot='休息时间',note='已标记为休息时间',
      updated_at=now_value,updated_by_device_id=p_device_id,version=entry.version+1
    where entry.workspace_id=p_workspace_id and entry.id=entry_id;
    entry_result:=timegenie.direct_cloud_time_entry_project(p_workspace_id,entry_id,now_value);
  else
    update timegenie.time_entries entry set
      default_task_id=null,note='已标记为无效时间',updated_at=now_value,
      updated_by_device_id=p_device_id,version=entry.version+1
    where entry.workspace_id=p_workspace_id and entry.id=entry_id;
    entry_result:=timegenie.direct_cloud_time_entry_project(p_workspace_id,entry_id,now_value);
    update timegenie.time_entries entry set deleted_at=now_value
    where entry.workspace_id=p_workspace_id and entry.id=entry_id;
  end if;
  update timegenie.work_days day set settled_at=null,updated_at=now_value,version=day.version+1
  where day.workspace_id=p_workspace_id and day.work_date=entry_row.work_date;

  result_value:=timegenie.direct_cloud_mutation_response(
    p_workspace_id,p_device_id,p_operation_id,jsonb_build_object('entry',entry_result),
    array['time'],p_expected_epoch
  );
  perform timegenie.record_direct_cloud_operation(
    p_workspace_id,p_operation_id,p_device_id,'direct_time_entry_disposition',request_value,result_value
  );
  return result_value;
end $$;

revoke all on function timegenie.direct_cloud_time_entry_update(uuid,uuid,uuid,bigint,jsonb)
  from public,anon;
revoke all on function timegenie.direct_cloud_time_entry_disposition(uuid,uuid,uuid,bigint,jsonb)
  from public,anon;
revoke all on function timegenie.direct_cloud_time_history_page(uuid,uuid,integer,boolean,bigint)
  from public,anon;
grant execute on function timegenie.direct_cloud_time_history_page(uuid,uuid,integer,boolean,bigint)
  to authenticated;
grant execute on function timegenie.direct_cloud_time_entry_update(uuid,uuid,uuid,bigint,jsonb)
  to authenticated;
grant execute on function timegenie.direct_cloud_time_entry_disposition(uuid,uuid,uuid,bigint,jsonb)
  to authenticated;

commit;

notify pgrst,'reload schema';
