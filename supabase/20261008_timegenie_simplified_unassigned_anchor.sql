-- TimeGenie simplified shared-unassigned anchor model.
-- Safe to reapply: all schema changes are IF EXISTS/IF NOT EXISTS and RPCs use
-- CREATE OR REPLACE. Natural clock growth never writes the shared aggregate.

create or replace function timegenie.touch_workspace_change()
returns trigger language plpgsql security definer set search_path=timegenie,extensions,pg_catalog
as $$
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
    when 'task_daily_estimates' then (row_image->>'task_id')||'|'||(row_image->>'work_date')
    when 'task_recurrence_rules' then row_image->>'task_id'
    when 'task_occurrences' then (row_image->>'task_id')||'|'||(row_image->>'occurrence_date')
    else coalesce(row_image->>'id',row_image->>'task_id') end;
  insert into timegenie.workspace_changes(workspace_id,entity_type,entity_id,operation,entity_version,changed_at)
  values((row_image->>'workspace_id')::uuid,tg_table_name,changed_entity_id,lower(tg_op),coalesce((row_image->>'version')::bigint,1),now());
  return coalesce(new,old);
end $$;

create unique index if not exists uq_unassigned_sessions_active_date
  on timegenie.unassigned_sessions(workspace_id, work_date)
  where state in ('collecting','awaiting_resolution');

-- Active shared sessions contain exactly one open, zero-duration anchor. Keep
-- terminal and historical closed segments intact for compatibility.
delete from timegenie.unassigned_segments segment
where segment.session_id in (
  select session.id from timegenie.unassigned_sessions session
  where session.state in ('collecting','awaiting_resolution')
    and exists(select 1 from timegenie.unassigned_segments open_segment
               where open_segment.session_id=session.id and open_segment.ended_at is null)
)
and segment.ended_at is not null;

update timegenie.unassigned_segments segment
set sequence_no=1,duration_seconds=0,lease_token=null
where segment.ended_at is null
  and segment.session_id in (select id from timegenie.unassigned_sessions where state in ('collecting','awaiting_resolution'));

update timegenie.unassigned_sessions session
set state='collecting',duration_seconds=0,first_started_at=(
      select segment.started_at from timegenie.unassigned_segments segment
      where segment.session_id=session.id and segment.ended_at is null limit 1
    ),
    last_ended_at=null,prompted_at=null,
    updated_at=greatest(session.updated_at,(
      select segment.started_at from timegenie.unassigned_segments segment
      where segment.session_id=session.id and segment.ended_at is null limit 1
    )),
    last_continuous_at=(
      select segment.started_at from timegenie.unassigned_segments segment
      where segment.session_id=session.id and segment.ended_at is null limit 1
    )
where session.state in ('collecting','awaiting_resolution')
  and exists(select 1 from timegenie.unassigned_segments segment
             where segment.session_id=session.id and segment.ended_at is null);

update timegenie.unassigned_segments segment
set ended_at=coalesce(segment.ended_at,segment.started_at),duration_seconds=0,lease_token=null
where segment.session_id in (
  select older.id from timegenie.unassigned_sessions older
  where older.state in ('collecting','awaiting_resolution')
    and exists(select 1 from timegenie.unassigned_sessions newer
      where newer.workspace_id=older.workspace_id and newer.state in ('collecting','awaiting_resolution')
        and (newer.work_date>older.work_date
          or (newer.work_date=older.work_date and newer.first_started_at>older.first_started_at)
          or (newer.work_date=older.work_date and newer.first_started_at=older.first_started_at and newer.id>older.id)))
);

update timegenie.unassigned_sessions older
set state='discarded',resolution_type='discard',generated_entry_id=null,duration_seconds=0,prompted_at=null,
    last_ended_at=greatest(older.first_started_at,coalesce(older.last_ended_at,older.updated_at,older.first_started_at)),
    resolved_at=greatest(older.first_started_at,coalesce(older.last_ended_at,older.updated_at,older.first_started_at)),
    migration_state='superseded',version=older.version+1
where older.state in ('collecting','awaiting_resolution')
  and exists(select 1 from timegenie.unassigned_sessions newer
    where newer.workspace_id=older.workspace_id and newer.state in ('collecting','awaiting_resolution')
      and (newer.work_date>older.work_date
        or (newer.work_date=older.work_date and newer.first_started_at>older.first_started_at)
        or (newer.work_date=older.work_date and newer.first_started_at=older.first_started_at and newer.id>older.id)));

create or replace function timegenie.timegenie_unassigned_active_anchor(
  p_workspace_id uuid,p_work_date date default null
) returns jsonb language sql stable security definer set search_path=timegenie,extensions,pg_catalog
as $$
  select timegenie.cloud_incremental_entity_get(p_workspace_id,'unassigned_sessions',session.id::text)->'data'
  from timegenie.unassigned_sessions session
  where session.workspace_id=p_workspace_id
    and session.work_date=coalesce(p_work_date,timegenie.workspace_work_date(p_workspace_id,clock_timestamp()))
    and session.state in ('collecting','awaiting_resolution')
    and exists(select 1 from timegenie.unassigned_segments segment
               where segment.session_id=session.id and segment.ended_at is null)
  order by session.created_at limit 1;
$$;

create or replace function timegenie.timegenie_unassigned_discard_superseded(
  p_workspace_id uuid,p_keep_session_id uuid,p_boundary timestamptz
) returns void language plpgsql security definer set search_path=timegenie,extensions,pg_catalog
as $$
begin
  update timegenie.unassigned_segments segment set
    ended_at=coalesce(segment.ended_at,segment.started_at),duration_seconds=0,lease_token=null
  where segment.workspace_id=p_workspace_id and segment.session_id in (
    select session.id from timegenie.unassigned_sessions session
    where session.workspace_id=p_workspace_id and session.state in ('collecting','awaiting_resolution')
      and (p_keep_session_id is null or session.id<>p_keep_session_id)
  );
  update timegenie.unassigned_sessions session set
    state='discarded',resolution_type='discard',generated_entry_id=null,duration_seconds=0,prompted_at=null,
    last_ended_at=greatest(session.first_started_at,p_boundary),resolved_at=greatest(session.first_started_at,p_boundary),
    updated_at=greatest(session.updated_at,p_boundary),last_continuous_at=greatest(session.first_started_at,p_boundary),
    migration_state='superseded',version=session.version+1
  where session.workspace_id=p_workspace_id and session.state in ('collecting','awaiting_resolution')
    and (p_keep_session_id is null or session.id<>p_keep_session_id);
end $$;

create or replace function timegenie.timegenie_unassigned_system_clear(
  p_workspace_id uuid,p_boundary timestamptz
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog
as $$
declare session_row timegenie.unassigned_sessions;
begin
  select * into session_row from timegenie.unassigned_sessions
  where workspace_id=p_workspace_id and state in ('collecting','awaiting_resolution')
    and exists(select 1 from timegenie.unassigned_segments segment where segment.session_id=unassigned_sessions.id and segment.ended_at is null)
  order by work_date desc,created_at desc limit 1 for update;
  if not found then return null; end if;
  update timegenie.unassigned_segments set ended_at=greatest(started_at,p_boundary),duration_seconds=0,lease_token=null
  where session_id=session_row.id and ended_at is null;
  update timegenie.unassigned_sessions set state='discarded',resolution_type='discard',duration_seconds=0,
    last_ended_at=p_boundary,resolved_at=p_boundary,prompted_at=null,updated_at=p_boundary,
    last_continuous_at=p_boundary,migration_state='superseded',version=version+1
  where id=session_row.id;
  return timegenie.cloud_incremental_entity_get(p_workspace_id,'unassigned_sessions',session_row.id::text)->'data';
end $$;

create or replace function timegenie.timegenie_unassigned_create_anchor(
  p_workspace_id uuid,p_device_id uuid,p_boundary timestamptz,p_candidate_id uuid default null,p_predecessor_session_id uuid default null
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog
as $$
declare session_row timegenie.unassigned_sessions; target_date date; threshold integer; session_id uuid;
begin
  if exists(select 1 from timegenie.time_entries where workspace_id=p_workspace_id and state in ('running','paused') and deleted_at is null) then
    return null;
  end if;
  target_date:=timegenie.workspace_work_date(p_workspace_id,p_boundary);
  select * into session_row from timegenie.unassigned_sessions where workspace_id=p_workspace_id and work_date=target_date
    and state in ('collecting','awaiting_resolution') order by first_started_at desc,id desc limit 1 for update;
  if found then
    perform timegenie.timegenie_unassigned_discard_superseded(p_workspace_id,session_row.id,p_boundary);
    return timegenie.cloud_incremental_entity_get(p_workspace_id,'unassigned_sessions',session_row.id::text)->'data';
  end if;
  perform timegenie.timegenie_unassigned_discard_superseded(p_workspace_id,null,p_boundary);
  select coalesce((value_json#>>'{}')::integer,300) into threshold from timegenie.app_settings
  where workspace_id=p_workspace_id and key='unassigned_prompt_seconds';
  threshold:=coalesce(threshold,300);
  session_id:=case when p_candidate_id is not null and not exists(select 1 from timegenie.unassigned_sessions where id=p_candidate_id) then p_candidate_id else timegenie.calendar_stable_uuid(
    'unassigned-anchor:'||p_workspace_id||':'||target_date||':'||coalesce(p_predecessor_session_id::text,p_boundary::text)
  ) end;
  insert into timegenie.unassigned_sessions(id,workspace_id,work_date,state,threshold_seconds,duration_seconds,first_started_at,
    last_ended_at,prompted_at,resolution_type,generated_entry_id,resolved_at,shared_source,predecessor_session_id,migration_state,
    created_at,updated_at,version,last_continuous_at)
  values(session_id,p_workspace_id,target_date,'collecting',threshold,0,p_boundary,null,null,null,null,null,'cloud',
    p_predecessor_session_id,'adopted',p_boundary,p_boundary,1,p_boundary) returning * into session_row;
  insert into timegenie.unassigned_segments(id,workspace_id,session_id,sequence_no,started_at,ended_at,duration_seconds,lease_token)
  values(timegenie.calendar_stable_uuid('unassigned-anchor-segment:'||session_id),p_workspace_id,session_id,1,p_boundary,null,0,null);
  return timegenie.cloud_incremental_entity_get(p_workspace_id,'unassigned_sessions',session_id::text)->'data';
end $$;

create or replace function timegenie.unassigned_get_state(p_workspace_id uuid)
returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog
as $$
declare session_row timegenie.unassigned_sessions; anchor_started timestamptz; elapsed bigint;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  select * into session_row from timegenie.unassigned_sessions where workspace_id=p_workspace_id
    and work_date=timegenie.workspace_work_date(p_workspace_id,clock_timestamp())
    and state in ('collecting','awaiting_resolution') order by created_at limit 1;
  if not found then return null; end if;
  select started_at into anchor_started from timegenie.unassigned_segments where session_id=session_row.id and ended_at is null limit 1;
  if anchor_started is null then return null; end if;
  elapsed:=greatest(0,extract(epoch from (clock_timestamp()-anchor_started))::bigint);
  return jsonb_build_object('session_id',session_row.id,'work_date',session_row.work_date,'state','collecting',
    'first_started_at',anchor_started,'last_ended_at',null,'current_segment_started_at',anchor_started,
    'elapsed_seconds',elapsed,'required_minutes',case when elapsed>0 then greatest(1,ceil(elapsed/60.0)::integer) else 0 end,
    'threshold_seconds',session_row.threshold_seconds,'must_resolve',elapsed>session_row.threshold_seconds,'version',session_row.version);
end $$;

create or replace function timegenie.unassigned_get_or_create_shared(
  p_workspace_id uuid,p_device_id uuid,p_candidate jsonb default null,p_predecessor_session_id uuid default null,p_started_at timestamptz default null
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog
as $$
declare boundary timestamptz; candidate_id uuid;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':shared-unassigned',0));
  boundary:=coalesce(p_started_at,case when p_candidate is null then null else
    case when jsonb_typeof(p_candidate->'first_started_at')='number' then to_timestamp((p_candidate->>'first_started_at')::double precision/1000)
         else (p_candidate->>'first_started_at')::timestamptz end end,clock_timestamp());
  candidate_id:=case when p_candidate is null then null else nullif(p_candidate->>'id','')::uuid end;
  return timegenie.timegenie_unassigned_create_anchor(p_workspace_id,p_device_id,boundary,candidate_id,p_predecessor_session_id);
end $$;

create or replace function timegenie.unassigned_anchor_reset(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_session_id uuid,p_expected_version bigint,p_resumed_at_millis bigint default null
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog
as $$
declare session_row timegenie.unassigned_sessions; boundary timestamptz:=coalesce(to_timestamp(p_resumed_at_millis::double precision/1000),clock_timestamp()); request_value jsonb; cached jsonb; result_value jsonb;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  request_value:=jsonb_build_object('deviceId',p_device_id,'sessionId',p_session_id,'expectedVersion',p_expected_version,'resumedAt',boundary);
  cached:=timegenie.processed_operation_result(p_workspace_id,p_operation_id,'unassigned_anchor_reset',request_value);
  if cached is not null then return cached; end if;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':shared-unassigned',0));
  select * into session_row from timegenie.unassigned_sessions where workspace_id=p_workspace_id and id=p_session_id for update;
  if not found or session_row.state not in ('collecting','awaiting_resolution') or session_row.version<>p_expected_version then
    result_value:=jsonb_build_object('accepted',false,'session',timegenie.timegenie_unassigned_active_anchor(p_workspace_id,null));
  else
    if exists(select 1 from timegenie.unassigned_sessions other where other.workspace_id=p_workspace_id
      and other.id<>p_session_id and other.state in ('collecting','awaiting_resolution')
      and (other.work_date>session_row.work_date or (other.work_date=session_row.work_date and other.first_started_at>session_row.first_started_at))) then
      perform timegenie.timegenie_unassigned_discard_superseded(p_workspace_id,(select other.id from timegenie.unassigned_sessions other
        where other.workspace_id=p_workspace_id and other.state in ('collecting','awaiting_resolution')
        order by other.work_date desc,other.first_started_at desc,other.id desc limit 1),boundary);
      result_value:=jsonb_build_object('accepted',false,'session',timegenie.timegenie_unassigned_active_anchor(p_workspace_id,null));
      perform timegenie.record_processed_operation(p_workspace_id,p_operation_id,p_device_id,'unassigned_anchor_reset',request_value,result_value);
      return result_value;
    end if;
    perform timegenie.timegenie_unassigned_discard_superseded(p_workspace_id,p_session_id,boundary);
    delete from timegenie.unassigned_segments where session_id=p_session_id;
    insert into timegenie.unassigned_segments(id,workspace_id,session_id,sequence_no,started_at,duration_seconds)
    values(timegenie.calendar_stable_uuid('unassigned-anchor-reset:'||p_operation_id),p_workspace_id,p_session_id,1,boundary,0);
    update timegenie.unassigned_sessions set work_date=timegenie.workspace_work_date(p_workspace_id,boundary),state='collecting',duration_seconds=0,
      first_started_at=boundary,last_ended_at=null,prompted_at=null,resolution_type=null,generated_entry_id=null,resolved_at=null,
      updated_at=boundary,last_continuous_at=boundary,shared_source='cloud',migration_state='adopted',version=version+1 where id=p_session_id;
    result_value:=jsonb_build_object('accepted',true,'session',timegenie.cloud_incremental_entity_get(p_workspace_id,'unassigned_sessions',p_session_id::text)->'data');
  end if;
  perform timegenie.record_processed_operation(p_workspace_id,p_operation_id,p_device_id,'unassigned_anchor_reset',request_value,result_value);
  return result_value;
end $$;

create or replace function timegenie.unassigned_resolve_shared(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_session_id uuid,p_expected_version bigint,p_resolution_type text,p_payload jsonb
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog
as $$
declare session_row timegenie.unassigned_sessions; cached jsonb; boundary timestamptz; anchor_started timestamptz; elapsed bigint; required_minutes bigint; supplied_minutes bigint; result_value jsonb; next_anchor jsonb; request_value jsonb; entry_value jsonb; generated_entry jsonb; allocation_value jsonb; resolved_entry_id uuid;
begin
  if p_resolution_type not in ('work','break','discard') then raise exception 'VALIDATION_ERROR: invalid unassigned resolution'; end if;
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  request_value:=jsonb_build_object('deviceId',p_device_id,'sessionId',p_session_id,'expectedVersion',p_expected_version,'resolutionType',p_resolution_type,'payload',coalesce(p_payload,'{}'::jsonb));
  cached:=timegenie.processed_operation_result(p_workspace_id,p_operation_id,'unassigned_resolve_shared',request_value);
  if cached is not null then return cached; end if;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':shared-unassigned',0));
  select * into session_row from timegenie.unassigned_sessions where workspace_id=p_workspace_id and id=p_session_id for update;
  if not found then raise exception 'NOT_FOUND: shared unassigned session'; end if;
  if session_row.state in ('resolved','discarded') or session_row.version<>p_expected_version then
    result_value:=jsonb_build_object('accepted',false,'session',timegenie.cloud_incremental_entity_get(p_workspace_id,'unassigned_sessions',p_session_id::text)->'data',
      'generatedEntry',case when session_row.generated_entry_id is null then null else timegenie.cloud_incremental_entity_get(p_workspace_id,'time_entries',session_row.generated_entry_id::text)->'data' end,
      'nextSession',timegenie.timegenie_unassigned_active_anchor(p_workspace_id,null));
    perform timegenie.record_processed_operation(p_workspace_id,p_operation_id,p_device_id,'unassigned_resolve_shared',request_value,result_value);
    return result_value;
  end if;
  if exists(select 1 from timegenie.unassigned_sessions other where other.workspace_id=p_workspace_id
    and other.id<>p_session_id and other.state in ('collecting','awaiting_resolution')
    and (other.work_date>session_row.work_date or (other.work_date=session_row.work_date and other.first_started_at>session_row.first_started_at))) then
    perform timegenie.timegenie_unassigned_discard_superseded(p_workspace_id,(select other.id from timegenie.unassigned_sessions other
      where other.workspace_id=p_workspace_id and other.state in ('collecting','awaiting_resolution')
      order by other.work_date desc,other.first_started_at desc,other.id desc limit 1),clock_timestamp());
    result_value:=jsonb_build_object('accepted',false,'session',timegenie.cloud_incremental_entity_get(p_workspace_id,'unassigned_sessions',p_session_id::text)->'data',
      'generatedEntry',null,'nextSession',timegenie.timegenie_unassigned_active_anchor(p_workspace_id,null));
    perform timegenie.record_processed_operation(p_workspace_id,p_operation_id,p_device_id,'unassigned_resolve_shared',request_value,result_value);
    return result_value;
  end if;
  boundary:=clock_timestamp();
  select started_at into anchor_started from timegenie.unassigned_segments where session_id=p_session_id and ended_at is null limit 1 for update;
  anchor_started:=coalesce(anchor_started,session_row.first_started_at);
  elapsed:=greatest(0,extract(epoch from (boundary-anchor_started))::bigint);
  required_minutes:=case when elapsed>0 then greatest(1,ceil(elapsed/60.0)::bigint) else 0 end;
  generated_entry:=p_payload->'generated_entry';
  if p_resolution_type in ('work','break','discard') then
    if generated_entry is null or jsonb_typeof(generated_entry)<>'object' then raise exception 'VALIDATION_ERROR: missing generated unassigned entry'; end if;
    resolved_entry_id:=nullif(generated_entry->>'id','')::uuid;
    if resolved_entry_id is null then raise exception 'VALIDATION_ERROR: missing generated unassigned entry id'; end if;
    if coalesce(generated_entry->>'origin_unassigned_session_id','')<>p_session_id::text then raise exception 'VALIDATION_ERROR: generated entry session mismatch'; end if;
    if p_resolution_type='work' then
      if jsonb_typeof(generated_entry->'allocations')<>'array' or jsonb_array_length(generated_entry->'allocations')=0 then raise exception 'VALIDATION_ERROR: work resolution requires allocations'; end if;
      select coalesce(sum((value->>'minutes')::bigint),0) into supplied_minutes from jsonb_array_elements(generated_entry->'allocations');
      if supplied_minutes<=0 or supplied_minutes>required_minutes then raise exception 'ALLOCATION_EXCEEDS_DURATION: invalid unassigned allocation total'; end if;
      if exists(
        select 1 from jsonb_array_elements(generated_entry->'allocations') allocation
        left join timegenie.tasks task on task.workspace_id=p_workspace_id and task.id=nullif(allocation->>'task_id','')::uuid and task.deleted_at is null
        where task.id is null or exists(select 1 from timegenie.tasks child where child.parent_id=task.id and child.deleted_at is null)
      ) then raise exception 'TASK_NOT_SELECTABLE: invalid unassigned allocation task'; end if;
    elsif coalesce(jsonb_array_length(generated_entry->'allocations'),0)<>0 then
      raise exception 'VALIDATION_ERROR: non-work resolution cannot contain allocations';
    end if;
    insert into timegenie.time_entries(id,workspace_id,work_date,kind,source_type,state,default_task_id,label_snapshot,started_at,ended_at,duration_seconds,note,origin_unassigned_session_id,created_at,updated_at,version,created_by_device_id,updated_by_device_id)
    values(resolved_entry_id,p_workspace_id,session_row.work_date,case when p_resolution_type='work' then 'work' else 'break' end,'unassigned','ended',null,
      coalesce(nullif(generated_entry->>'label_snapshot',''),case when p_resolution_type='work' then '未归属时间分配' when p_resolution_type='break' then '未归属时间记为休息' else '未归属时间记为无效' end),
      anchor_started,boundary,elapsed,nullif(generated_entry->>'note',''),p_session_id,boundary,boundary,1,p_device_id,p_device_id);
    insert into timegenie.time_segments(id,workspace_id,entry_id,sequence_no,started_at,ended_at,duration_seconds)
    values(timegenie.calendar_stable_uuid('unassigned-entry-segment:'||p_operation_id),p_workspace_id,resolved_entry_id,1,anchor_started,boundary,elapsed);
    if p_resolution_type='work' then
      for allocation_value in select value from jsonb_array_elements(generated_entry->'allocations') loop
        insert into timegenie.time_allocations(id,workspace_id,entry_id,task_id,minutes,note,created_at,updated_at,version)
        values(coalesce(nullif(allocation_value->>'id','')::uuid,gen_random_uuid()),p_workspace_id,resolved_entry_id,
          (allocation_value->>'task_id')::uuid,(allocation_value->>'minutes')::integer,nullif(allocation_value->>'note',''),boundary,boundary,1);
      end loop;
    end if;
    entry_value:=timegenie.cloud_incremental_entity_get(p_workspace_id,'time_entries',resolved_entry_id::text)->'data';
  else
    resolved_entry_id:=null;
    entry_value:=null;
  end if;
  update timegenie.unassigned_segments set ended_at=greatest(started_at,boundary),duration_seconds=greatest(0,extract(epoch from (boundary-started_at))::bigint),lease_token=null
  where session_id=p_session_id and ended_at is null;
  update timegenie.unassigned_sessions set state=case when p_resolution_type='discard' then 'discarded' else 'resolved' end,
    duration_seconds=elapsed,resolution_type=p_resolution_type,generated_entry_id=resolved_entry_id,resolved_at=boundary,last_ended_at=boundary,
    updated_at=boundary,last_continuous_at=boundary,resolution_operation_id=p_operation_id,shared_source='cloud',migration_state='adopted',version=version+1
  where id=p_session_id;
  next_anchor:=timegenie.timegenie_unassigned_create_anchor(p_workspace_id,p_device_id,boundary,null,p_session_id);
  result_value:=jsonb_build_object('accepted',true,'session',timegenie.cloud_incremental_entity_get(p_workspace_id,'unassigned_sessions',p_session_id::text)->'data','generatedEntry',entry_value,'nextSession',next_anchor);
  perform timegenie.record_processed_operation(p_workspace_id,p_operation_id,p_device_id,'unassigned_resolve_shared',request_value,result_value);
  return result_value;
end $$;

-- Lease traffic only coordinates calendar boundaries. It must not advance the
-- anchor, duration, threshold state, updated_at, or version.
create or replace function timegenie.unassigned_tick(p_workspace_id uuid,p_device_id uuid,p_lease_token uuid)
returns void language plpgsql security definer set search_path=timegenie,extensions,pg_catalog
as $$ begin
  perform timegenie.coordinate_timer_calendar_day_at(p_workspace_id,p_device_id,clock_timestamp());
  perform timegenie.coordinate_unassigned_calendar_day_at(p_workspace_id,p_device_id,p_lease_token,clock_timestamp());
end $$;

-- Simplified day coordination creates one anchor only when absent. Natural
-- growth stays local and does not append segments or touch an existing row.
create or replace function timegenie.coordinate_unassigned_calendar_day_at(
  p_workspace_id uuid,p_device_id uuid,p_lease_token uuid,p_observed_at timestamptz default clock_timestamp()
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog
as $$
declare current_anchor jsonb; target_date date; next_boundary timestamptz; rolled_over boolean:=false;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  if not exists(select 1 from timegenie.tracking_leases where workspace_id=p_workspace_id and holder_device_id=p_device_id and lease_token=p_lease_token and expires_at>p_observed_at) then raise exception 'LEASE_EXPIRED'; end if;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':shared-unassigned',0));
  target_date:=timegenie.workspace_work_date(p_workspace_id,p_observed_at);
  rolled_over:=exists(select 1 from timegenie.unassigned_sessions session where session.workspace_id=p_workspace_id
    and session.work_date<target_date and session.state in ('collecting','awaiting_resolution'));
  current_anchor:=timegenie.timegenie_unassigned_active_anchor(p_workspace_id,target_date);
  if current_anchor is not null then
    perform timegenie.timegenie_unassigned_discard_superseded(p_workspace_id,(current_anchor->>'id')::uuid,p_observed_at);
    return current_anchor;
  end if;
  perform timegenie.timegenie_unassigned_discard_superseded(p_workspace_id,null,p_observed_at);
  -- Continuous rollover starts the new day's anchor exactly at the workspace
  -- day boundary. A delayed tick must not silently drop the time between
  -- midnight and the observation. When there is no prior day to continue,
  -- start at the current observation as usual.
  if rolled_over then
    next_boundary:=timegenie.workspace_day_start(p_workspace_id,target_date);
  else
    next_boundary:=p_observed_at;
  end if;
  return timegenie.timegenie_unassigned_create_anchor(p_workspace_id,p_device_id,next_boundary,null,null);
end $$;

-- Wrap the current calendar timer apply once. Timer start clears the active
-- anchor, while timer stop creates/returns the unique anchor at ended_at.
do $$ begin
  if to_regprocedure('timegenie.apply_calendar_time_entry_pre_simplified_anchor(uuid,uuid,uuid,text,text,bigint,jsonb,jsonb)') is null then
    alter function timegenie.apply_calendar_time_entry(uuid,uuid,uuid,text,text,bigint,jsonb,jsonb)
      rename to apply_calendar_time_entry_pre_simplified_anchor;
  end if;
end $$;

create or replace function timegenie.apply_calendar_time_entry(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_operation_type text,p_entity_id text,p_base_version bigint,p_payload jsonb,p_request jsonb
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog
as $$
declare result_value jsonb; boundary timestamptz; unassigned_value jsonb;
declare cached jsonb;
begin
  cached:=timegenie.processed_operation_result(p_workspace_id,p_operation_id,p_operation_type,p_request);
  if cached is not null then return cached; end if;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':shared-unassigned',0));
  if p_operation_type='timer_start' then
    boundary:=case when jsonb_typeof(p_payload->'started_at')='number' then to_timestamp((p_payload->>'started_at')::double precision/1000) else (p_payload->>'started_at')::timestamptz end;
    unassigned_value:=timegenie.timegenie_unassigned_system_clear(p_workspace_id,boundary);
  end if;
  result_value:=timegenie.apply_calendar_time_entry_pre_simplified_anchor(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_id,p_base_version,p_payload,p_request);
  if p_operation_type='timer_stop' then
    boundary:=case when jsonb_typeof(p_payload->'ended_at')='number' then to_timestamp((p_payload->>'ended_at')::double precision/1000) else (p_payload->>'ended_at')::timestamptz end;
    unassigned_value:=timegenie.timegenie_unassigned_create_anchor(p_workspace_id,p_device_id,boundary,null,null);
  end if;
  if unassigned_value is not null then
    result_value:=result_value||jsonb_build_object('unassignedSession',unassigned_value);
  end if;
  -- The preserved timer implementation records its base result before this
  -- wrapper adds the authoritative unassigned boundary. Keep retries byte-for-
  -- byte idempotent by replacing the cached response with the final envelope.
  update timegenie.processed_operations
  set result_json=result_value
  where workspace_id=p_workspace_id and operation_id=p_operation_id;
  return result_value;
end $$;

create or replace function timegenie.close_expired_unassigned_lease(
  p_workspace_id uuid,p_close_at timestamptz
) returns void language plpgsql security definer set search_path=timegenie,pg_catalog
as $$ begin
  perform timegenie.timegenie_unassigned_system_clear(p_workspace_id,p_close_at);
end $$;

-- New clients route resets to the dedicated RPC. Legacy lifecycle payloads are
-- acknowledged as superseded and can never restore accumulated/multi-segment state.
create or replace function timegenie.cloud_apply_simplified_unassigned(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_operation_type text,p_entity_id text,p_base_version bigint,p_payload jsonb
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog
as $$
declare authoritative jsonb;
begin
  if p_operation_type in ('unassigned_session_pause','unassigned_session_resume','unassigned_session_awaiting_resolution') then
    authoritative:=timegenie.cloud_incremental_entity_get(p_workspace_id,'unassigned_sessions',p_entity_id)->'data';
    if authoritative is null then authoritative:=timegenie.timegenie_unassigned_active_anchor(p_workspace_id,null); end if;
    return jsonb_build_object('superseded',true,'session',authoritative);
  end if;
  raise exception 'VALIDATION_ERROR: unsupported simplified unassigned operation %',p_operation_type;
end $$;

create or replace function timegenie.cloud_apply_patch(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_operation_type text,p_entity_type text,p_entity_id text,p_base_version bigint,p_payload jsonb
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$ begin
  if p_entity_type='unassigned_session' and p_operation_type in ('unassigned_session_pause','unassigned_session_resume','unassigned_session_awaiting_resolution') then
    return timegenie.cloud_apply_simplified_unassigned(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_id,p_base_version,p_payload);
  end if;
  if p_entity_type='time_entry' and coalesce(p_payload->>'source_type','')='timer' then
    return timegenie.apply_calendar_time_entry(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_id,p_base_version,p_payload,
      jsonb_build_object('deviceId',p_device_id,'entityType',p_entity_type,'entityId',p_entity_id,'baseVersion',p_base_version,'payload',coalesce(p_payload,'{}'::jsonb)));
  end if;
  return timegenie.cloud_apply_patch_pre_calendar(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_type,p_entity_id,p_base_version,p_payload);
end $$;

create or replace function timegenie.cloud_apply_patch(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_operation_type text,p_entity_type text,p_entity_id text,p_base_version bigint,p_payload_version bigint,p_coalesced_count bigint,p_payload jsonb
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$ begin
  if p_entity_type='unassigned_session' and p_operation_type in ('unassigned_session_pause','unassigned_session_resume','unassigned_session_awaiting_resolution') then
    return timegenie.cloud_apply_simplified_unassigned(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_id,p_base_version,p_payload);
  end if;
  if p_entity_type='time_entry' and coalesce(p_payload->>'source_type','')='timer' then
    if p_payload_version is null or (p_payload->>'version')::bigint<>p_payload_version then raise exception 'VERSION_CONFLICT: declared payload version mismatch'; end if;
    if p_coalesced_count is null or p_coalesced_count<1 then raise exception 'VERSION_CONFLICT: coalesced timer operation count must be positive'; end if;
    if (p_base_version is null and p_payload_version<>p_coalesced_count) or (p_base_version is not null and p_payload_version<>p_base_version+p_coalesced_count) then
      raise exception 'VERSION_CONFLICT: timer payload version does not match operation span';
    end if;
    return timegenie.apply_calendar_time_entry(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_id,p_base_version,p_payload,
      jsonb_build_object('deviceId',p_device_id,'entityType',p_entity_type,'entityId',p_entity_id,'baseVersion',p_base_version,
        'payloadVersion',p_payload_version,'coalescedCount',p_coalesced_count,'payload',coalesce(p_payload,'{}'::jsonb)));
  end if;
  return timegenie.cloud_apply_patch_pre_calendar(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_type,p_entity_id,p_base_version,p_payload_version,p_coalesced_count,p_payload);
end $$;

revoke execute on function timegenie.timegenie_unassigned_active_anchor(uuid,date) from public,anon,authenticated;
revoke execute on function timegenie.timegenie_unassigned_system_clear(uuid,timestamptz) from public,anon,authenticated;
revoke execute on function timegenie.timegenie_unassigned_create_anchor(uuid,uuid,timestamptz,uuid,uuid) from public,anon,authenticated;
revoke execute on function timegenie.cloud_apply_simplified_unassigned(uuid,uuid,uuid,text,text,bigint,jsonb) from public,anon,authenticated;
grant execute on function timegenie.unassigned_get_state(uuid) to authenticated;
grant execute on function timegenie.unassigned_get_or_create_shared(uuid,uuid,jsonb,uuid,timestamptz) to authenticated;
grant execute on function timegenie.unassigned_resolve_shared(uuid,uuid,uuid,uuid,bigint,text,jsonb) to authenticated;
grant execute on function timegenie.unassigned_anchor_reset(uuid,uuid,uuid,uuid,bigint,bigint) to authenticated;
-- Workspace deletion remains protected by the existing owner-only RLS policy.
-- The grant lets disposable accounts clean up their own cascaded test data.
grant delete on timegenie.workspaces to authenticated;
