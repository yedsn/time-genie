-- Direct-v2 unassigned-time creation, calendar-day boundary handling and
-- work/break/discard resolution. A workspace keeps one shared anchor per
-- work date; concurrent devices converge on the same anchor instead of
-- creating competing local recovery operations.

create or replace function timegenie.direct_cloud_unassigned_session_snapshot(
  p_workspace_id uuid,p_session_id uuid
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare
  session_row timegenie.unassigned_sessions%rowtype;
  segment_started timestamptz;
  elapsed bigint;
  server_time_value timestamptz:=clock_timestamp();
begin
  select session.* into session_row from timegenie.unassigned_sessions session
  where session.workspace_id=p_workspace_id and session.id=p_session_id;
  if not found then return null; end if;
  select segment.started_at into segment_started
  from timegenie.unassigned_segments segment
  where segment.session_id=p_session_id and segment.ended_at is null
  order by segment.sequence_no desc limit 1;
  select coalesce(sum(case
    when segment.ended_at is null
      then greatest(extract(epoch from(server_time_value-segment.started_at))::bigint,0)
    else segment.duration_seconds end),0)
  into elapsed
  from timegenie.unassigned_segments segment
  where segment.session_id=p_session_id;
  return jsonb_build_object(
    'sessionId',session_row.id,
    'workDate',session_row.work_date,
    'state',case when session_row.state='collecting' and elapsed>session_row.threshold_seconds
      then 'awaiting_resolution' else session_row.state end,
    'firstStartedAt',(extract(epoch from session_row.first_started_at)*1000)::bigint,
    'lastEndedAt',case when session_row.last_ended_at is null then null
      else (extract(epoch from session_row.last_ended_at)*1000)::bigint end,
    'currentSegmentStartedAt',case when segment_started is null then null
      else (extract(epoch from segment_started)*1000)::bigint end,
    'elapsedSeconds',elapsed,
    'requiredMinutes',case when elapsed>0 then greatest(1,((elapsed+59)/60)::integer) else 0 end,
    'thresholdSeconds',session_row.threshold_seconds,
    'mustResolve',elapsed>session_row.threshold_seconds,
    'version',session_row.version,
    'resolutionType',session_row.resolution_type,
    'generatedEntryId',session_row.generated_entry_id
  );
end $$;

create or replace function timegenie.direct_cloud_day_time_summary(
  p_workspace_id uuid,p_work_date date
) returns jsonb language sql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
  select jsonb_build_object(
    'workDate',p_work_date,
    'workSeconds',coalesce(sum(case when entry.kind='work' then entry.duration_seconds else 0 end),0),
    'breakSeconds',coalesce(sum(case when entry.kind='break' then entry.duration_seconds else 0 end),0),
    'totalSeconds',coalesce(sum(entry.duration_seconds),0),
    'unassignedWorkSeconds',coalesce(sum(case
      when entry.kind='work' and entry.source_type='unassigned' then entry.duration_seconds
      else 0 end),0),
    'entryCount',count(entry.id)
  )
  from timegenie.time_entries entry
  where entry.workspace_id=p_workspace_id
    and entry.work_date=p_work_date
    and entry.deleted_at is null;
$$;

-- Caller must hold the shared-unassigned advisory lock. This helper never
-- records an operation; it only materializes the current anchor after closing
-- older calendar-day anchors at their own day boundary.
create or replace function timegenie.direct_cloud_unassigned_anchor_ensure_locked(
  p_workspace_id uuid,p_device_id uuid,p_observed_at timestamptz
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare
  target_date date;
  session_row timegenie.unassigned_sessions%rowtype;
  open_session timegenie.unassigned_sessions%rowtype;
  close_at timestamptz;
  elapsed bigint;
  threshold integer;
  previous_session_id uuid;
  anchor_started timestamptz;
  new_session_id uuid;
  active_timer boolean;
  rolled_over_previous_date boolean:=false;
begin
  target_date:=timegenie.workspace_work_date(p_workspace_id,p_observed_at);
  select session.id into previous_session_id
  from timegenie.unassigned_sessions session
  where session.workspace_id=p_workspace_id
    and session.work_date<target_date
    and session.state in ('collecting','awaiting_resolution')
  order by session.work_date desc,session.first_started_at desc,session.id desc
  limit 1;

  -- Preserve each older day's unassigned time up to that day's end. Keeping a
  -- non-empty session awaiting resolution lets the user classify it later
  -- without losing the calendar-day total.
  for open_session in
    select session.* from timegenie.unassigned_sessions session
    where session.workspace_id=p_workspace_id
      and session.work_date<target_date
      and session.state in ('collecting','awaiting_resolution')
      and exists(select 1 from timegenie.unassigned_segments segment
        where segment.session_id=session.id and segment.ended_at is null)
    order by session.work_date,session.created_at
    for update
  loop
    rolled_over_previous_date:=true;
    close_at:=least(timegenie.workspace_day_end(p_workspace_id,open_session.work_date),p_observed_at);
    update timegenie.unassigned_segments segment set
      ended_at=greatest(segment.started_at,close_at),
      duration_seconds=greatest(0,extract(epoch from(
        greatest(segment.started_at,close_at)-segment.started_at
      ))::bigint),
      lease_token=null
    where segment.session_id=open_session.id and segment.ended_at is null;
    select coalesce(sum(segment.duration_seconds),0) into elapsed
    from timegenie.unassigned_segments segment where segment.session_id=open_session.id;
    update timegenie.unassigned_sessions session set
      duration_seconds=elapsed,
      state=case when elapsed>0 then 'awaiting_resolution' else 'discarded' end,
      resolution_type=case when elapsed>0 then session.resolution_type else 'discard' end,
      resolved_at=case when elapsed>0 then session.resolved_at else close_at end,
      last_ended_at=close_at,
      last_continuous_at=close_at,
      prompted_at=case when elapsed>session.threshold_seconds
        then coalesce(session.prompted_at,close_at) else session.prompted_at end,
      updated_at=p_observed_at,
      version=session.version+1
    where session.id=open_session.id;
  end loop;

  select session.* into session_row from timegenie.unassigned_sessions session
  where session.workspace_id=p_workspace_id
    and session.work_date=target_date
    and session.state in ('collecting','awaiting_resolution')
  order by session.created_at limit 1
  for update;
  if found then
    return jsonb_build_object('sessionId',session_row.id,'created',false);
  end if;

  select exists(select 1 from timegenie.time_entries entry
    where entry.workspace_id=p_workspace_id
      and entry.state in ('running','paused') and entry.deleted_at is null)
  into active_timer;
  if active_timer then
    return jsonb_build_object('sessionId',null,'created',false);
  end if;

  anchor_started:=case
    when previous_session_id is null then p_observed_at
    when rolled_over_previous_date then timegenie.workspace_day_start(p_workspace_id,target_date)
    when exists(select 1 from timegenie.unassigned_sessions session
      where session.id=previous_session_id and session.work_date=target_date)
      then p_observed_at
    else timegenie.workspace_day_start(p_workspace_id,target_date)
  end;

  select (setting.value_json#>>'{}')::integer into threshold
  from timegenie.app_settings setting
  where setting.workspace_id=p_workspace_id and setting.key='unassigned_prompt_seconds';
  threshold:=coalesce(threshold,300);

  new_session_id:=gen_random_uuid();
  insert into timegenie.unassigned_sessions(
    id,workspace_id,work_date,state,threshold_seconds,duration_seconds,
    first_started_at,last_ended_at,prompted_at,resolution_type,generated_entry_id,
    resolved_at,shared_source,predecessor_session_id,migration_state,created_at,
    updated_at,version,last_continuous_at
  ) values(
    new_session_id,p_workspace_id,target_date,'collecting',threshold,0,
    anchor_started,null,null,null,null,null,'cloud',previous_session_id,'adopted',
    p_observed_at,p_observed_at,1,anchor_started
  );
  insert into timegenie.unassigned_segments(
    id,workspace_id,session_id,sequence_no,started_at,ended_at,duration_seconds,lease_token
  ) values(
    gen_random_uuid(),p_workspace_id,new_session_id,1,anchor_started,null,0,null
  );
  return jsonb_build_object('sessionId',new_session_id,'created',true);
end $$;

create or replace function timegenie.direct_cloud_unassigned_ensure(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,
  p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare
  request_value jsonb;
  cached jsonb;
  observed_at timestamptz:=clock_timestamp();
  target_date date;
  anchor_result jsonb;
  session_id uuid;
  result_value jsonb;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object(
    'expectedEpoch',p_expected_epoch,
    'payload',coalesce(p_payload,'{}'::jsonb)
  );
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':shared-unassigned',0));
  cached:=timegenie.direct_cloud_operation_result(
    p_workspace_id,p_operation_id,'direct_unassigned_ensure',request_value
  );
  if cached is not null then return cached; end if;

  target_date:=timegenie.workspace_work_date(p_workspace_id,observed_at);
  anchor_result:=timegenie.direct_cloud_unassigned_anchor_ensure_locked(
    p_workspace_id,p_device_id,observed_at
  );
  session_id:=nullif(anchor_result->>'sessionId','')::uuid;
  result_value:=timegenie.direct_cloud_mutation_response(
    p_workspace_id,p_device_id,p_operation_id,
    jsonb_build_object(
      'created',coalesce((anchor_result->>'created')::boolean,false),
      'anchor',timegenie.direct_cloud_unassigned_session_snapshot(p_workspace_id,session_id),
      'daySummary',timegenie.direct_cloud_day_time_summary(p_workspace_id,target_date),
      'serverTime',(extract(epoch from observed_at)*1000)::bigint
    ),
    array['time','unassigned'],p_expected_epoch
  );
  perform timegenie.record_direct_cloud_operation(
    p_workspace_id,p_operation_id,p_device_id,'direct_unassigned_ensure',request_value,result_value
  );
  return result_value;
end $$;

create or replace function timegenie.direct_cloud_unassigned_resolve(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,
  p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare
  request_value jsonb;
  cached jsonb;
  result_value jsonb;
  session_row timegenie.unassigned_sessions%rowtype;
  session_id uuid;
  resolution_value text;
  boundary timestamptz:=clock_timestamp();
  anchor_started timestamptz;
  settled_end timestamptz;
  elapsed bigint;
  required_minutes bigint;
  groups jsonb:='{}'::jsonb;
  item jsonb;
  group_entry record;
  task_id uuid;
  minutes_value integer;
  complete_value boolean;
  expected_task_version bigint;
  previous_version bigint;
  group_value jsonb;
  task_row timegenie.tasks%rowtype;
  total_minutes integer:=0;
  allocation_task_count integer:=0;
  first_task_title text;
  recurring_value boolean;
  entry_id uuid;
  entry_kind text;
  entry_label text;
  entry_note text;
  next_result jsonb;
  next_session_id uuid;
  next_anchor jsonb;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object(
    'expectedEpoch',p_expected_epoch,
    'payload',coalesce(p_payload,'{}'::jsonb)
  );
  resolution_value:=coalesce(p_payload->>'resolutionType','');
  if resolution_value not in ('work','break','discard') then
    raise exception 'VALIDATION_ERROR: 未归属处理类型必须是 work、break 或 discard';
  end if;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':shared-unassigned',0));
  cached:=timegenie.direct_cloud_operation_result(
    p_workspace_id,p_operation_id,'direct_unassigned_resolve',request_value
  );
  if cached is not null then return cached; end if;

  session_id:=nullif(p_payload->>'sessionId','')::uuid;
  if session_id is null or nullif(p_payload->>'expectedVersion','') is null then
    raise exception 'VALIDATION_ERROR: 未归属处理缺少会话或版本';
  end if;
  select session.* into session_row from timegenie.unassigned_sessions session
  where session.workspace_id=p_workspace_id and session.id=session_id
  for update;
  if not found then raise exception 'NOT_FOUND: 未归属会话不存在'; end if;
  if session_row.state not in ('collecting','awaiting_resolution') then
    raise exception 'VERSION_CONFLICT: 未归属时间已被处理，请刷新后重试'
      using detail=jsonb_build_object(
        'code','VERSION_CONFLICT',
        'session',timegenie.direct_cloud_unassigned_session_snapshot(p_workspace_id,session_id)
      )::text;
  end if;
  if session_row.version<>(p_payload->>'expectedVersion')::bigint then
    raise exception 'VERSION_CONFLICT: 未归属时间已在其他设备变化，请刷新后重试'
      using detail=jsonb_build_object(
        'code','VERSION_CONFLICT',
        'session',timegenie.direct_cloud_unassigned_session_snapshot(p_workspace_id,session_id)
      )::text;
  end if;

  select segment.started_at into anchor_started
  from timegenie.unassigned_segments segment
  where segment.session_id=session_id and segment.ended_at is null
  order by segment.sequence_no desc limit 1;
  anchor_started:=coalesce(anchor_started,session_row.first_started_at);
  update timegenie.unassigned_segments segment set
    ended_at=greatest(segment.started_at,boundary),
    duration_seconds=greatest(0,extract(epoch from(
      greatest(segment.started_at,boundary)-segment.started_at
    ))::bigint),
    lease_token=null
  where segment.session_id=session_id and segment.ended_at is null;
  select coalesce(sum(segment.duration_seconds),0),max(segment.ended_at)
  into elapsed,settled_end
  from timegenie.unassigned_segments segment where segment.session_id=session_id;
  settled_end:=coalesce(settled_end,boundary);
  required_minutes:=case when elapsed>0 then ((elapsed+59)/60) else 0 end;

  if resolution_value='work' then
    if jsonb_typeof(coalesce(p_payload->'allocations','null'::jsonb))<>'array' then
      raise exception 'VALIDATION_ERROR: 未归属分配参数必须是数组';
    end if;
    for item in select value from jsonb_array_elements(p_payload->'allocations') loop
      task_id:=nullif(item->>'taskId','')::uuid;
      minutes_value:=coalesce((item->>'minutes')::integer,0);
      complete_value:=coalesce((item->>'completeTask')::boolean,false);
      if task_id is null or minutes_value<0 then
        raise exception 'VALIDATION_ERROR: 未归属分配事项或分钟无效';
      end if;
      if minutes_value=0 and not complete_value then continue; end if;
      perform 1 from timegenie.tasks task
      where task.workspace_id=p_workspace_id and task.id=task_id
        and task.deleted_at is null
        and not exists(select 1 from timegenie.tasks child
          where child.workspace_id=p_workspace_id and child.parent_id=task_id
            and child.deleted_at is null)
      for update;
      if not found then
        raise exception 'TASK_NOT_SELECTABLE: 未归属时间只能分配到叶子事项';
      end if;
      expected_task_version:=nullif(item->>'taskExpectedVersion','')::bigint;
      group_value:=coalesce(groups->task_id::text,'{"minutes":0,"completeTask":false}'::jsonb);
      previous_version:=nullif(group_value->>'taskExpectedVersion','')::bigint;
      if previous_version is not null and expected_task_version is not null
         and previous_version<>expected_task_version then
        raise exception 'VERSION_CONFLICT: 同一事项的版本信息不一致';
      end if;
      groups:=jsonb_set(groups,array[task_id::text],jsonb_build_object(
        'minutes',(group_value->>'minutes')::integer+minutes_value,
        'completeTask',coalesce((group_value->>'completeTask')::boolean,false) or complete_value,
        'note',coalesce(nullif(item->>'note',''),group_value->>'note'),
        'taskExpectedVersion',coalesce(expected_task_version,previous_version)
      ),true);
      total_minutes:=total_minutes+minutes_value;
    end loop;
    if total_minutes<=0 then
      raise exception 'VALIDATION_ERROR: 请至少分配 1 分钟到事项';
    end if;
    if total_minutes>required_minutes then
      raise exception 'ALLOCATION_EXCEEDS_DURATION: required=% supplied=%',
        required_minutes,total_minutes;
    end if;
  elsif coalesce(jsonb_array_length(p_payload->'allocations'),0)<>0 then
    raise exception 'VALIDATION_ERROR: 休息或丢弃处理不能包含事项分配';
  end if;

  entry_id:=timegenie.calendar_stable_uuid('direct-unassigned-entry:'||p_operation_id::text);
  entry_kind:=case when resolution_value='work' then 'work' else 'break' end;
  entry_label:=case when resolution_value='work' then '未归属时间分配'
    when resolution_value='break' then '休息时间' else '无效时间' end;
  entry_note:=case when resolution_value='work' then '未归属时间补分配'
    when resolution_value='break' then '未执行事项期间记录为休息'
    else '未执行事项期间记为无效' end;

  for group_entry in select key,value from jsonb_each(groups) loop
    task_id:=group_entry.key::uuid;
    minutes_value:=coalesce((group_entry.value->>'minutes')::integer,0);
    complete_value:=coalesce((group_entry.value->>'completeTask')::boolean,false);
    expected_task_version:=nullif(group_entry.value->>'taskExpectedVersion','')::bigint;
    select task.* into task_row from timegenie.tasks task
    where task.workspace_id=p_workspace_id and task.id=task_id for update;
    if not found then raise exception 'NOT_FOUND: 分配事项不存在'; end if;
    if task_row.deleted_at is not null then raise exception 'ENTITY_DELETED: 分配事项已删除'; end if;
    recurring_value:=exists(
      select 1 from timegenie.task_recurrence_rules rule
      where rule.workspace_id=p_workspace_id and rule.task_id=task_id
        and rule.effective_start<=session_row.work_date
        and (rule.effective_end is null or rule.effective_end>=session_row.work_date)
    );
    if recurring_value then
      perform timegenie.direct_cloud_occurrence_ensure(
        p_workspace_id,task_id,session_row.work_date
      );
    end if;
    if complete_value then
      if expected_task_version is null then
        raise exception 'VALIDATION_ERROR: 完成事项时缺少事项版本';
      end if;
      if task_row.version<>expected_task_version then
        raise exception 'VERSION_CONFLICT: 分配事项状态已变化，请刷新后重试'
          using detail=jsonb_build_object('code','VERSION_CONFLICT','taskId',task_id)::text;
      end if;
      if recurring_value then
        update timegenie.task_occurrences occurrence set
          status='done',completed_at=boundary,updated_at=boundary,version=occurrence.version+1
        where occurrence.workspace_id=p_workspace_id
          and occurrence.task_id=task_id
          and occurrence.occurrence_date=session_row.work_date
          and occurrence.status<>'done';
      elsif task_row.status<>'done' then
        update timegenie.tasks task set
          status='done',completed_at=boundary,updated_at=boundary,
          updated_by_device_id=p_device_id,version=task.version+1
        where task.workspace_id=p_workspace_id and task.id=task_id;
        insert into timegenie.task_status_events(
          id,workspace_id,task_id,status,occurred_at,source_type,created_at
        ) values(gen_random_uuid(),p_workspace_id,task_id,'done',boundary,'user',boundary);
      end if;
    end if;
    if minutes_value>0 then
      allocation_task_count:=allocation_task_count+1;
      if first_task_title is null then first_task_title:=task_row.title; end if;
    end if;
  end loop;

  if resolution_value='work' then
    entry_label:=case when allocation_task_count=1 then coalesce(first_task_title,'未归属工作时间')
      else '未归属时间分配到 '||allocation_task_count||' 个事项' end;
  end if;
  insert into timegenie.time_entries(
    id,workspace_id,work_date,kind,source_type,state,default_task_id,label_snapshot,
    started_at,ended_at,duration_seconds,note,origin_unassigned_session_id,
    created_at,updated_at,version,created_by_device_id,updated_by_device_id
  ) values(
    entry_id,p_workspace_id,session_row.work_date,entry_kind,'unassigned','ended',null,
    entry_label,anchor_started,settled_end,elapsed,entry_note,session_id,
    boundary,boundary,1,p_device_id,p_device_id
  );
  for group_entry in select key,value from jsonb_each(groups) loop
    minutes_value:=coalesce((group_entry.value->>'minutes')::integer,0);
    if minutes_value>0 then
      task_id:=group_entry.key::uuid;
      insert into timegenie.time_allocations(
        id,workspace_id,entry_id,task_id,minutes,note,created_at,updated_at,version
      ) values(
        timegenie.calendar_stable_uuid(
          'direct-unassigned-allocation:'||p_operation_id::text||':'||task_id::text
        ),p_workspace_id,entry_id,task_id,minutes_value,
        nullif(group_entry.value->>'note',''),boundary,boundary,1
      );
    end if;
  end loop;
  insert into timegenie.time_segments(
    id,workspace_id,entry_id,sequence_no,started_at,ended_at,duration_seconds
  )
  select timegenie.calendar_stable_uuid(
      'direct-unassigned-segment:'||p_operation_id::text||':'||segment.sequence_no::text
    ),p_workspace_id,entry_id,segment.sequence_no,segment.started_at,
    segment.ended_at,segment.duration_seconds
  from timegenie.unassigned_segments segment
  where segment.session_id=session_id
  order by segment.sequence_no;
  update timegenie.unassigned_sessions session set
    state=case when resolution_value='discard' then 'discarded' else 'resolved' end,
    duration_seconds=elapsed,
    resolution_type=resolution_value,
    generated_entry_id=entry_id,
    resolved_at=boundary,
    last_ended_at=settled_end,
    last_continuous_at=settled_end,
    updated_at=boundary,
    resolution_operation_id=p_operation_id,
    shared_source='cloud',
    migration_state='adopted',
    version=session.version+1
  where session.id=session_id;

  next_result:=timegenie.direct_cloud_unassigned_anchor_ensure_locked(
    p_workspace_id,p_device_id,boundary
  );
  next_session_id:=nullif(next_result->>'sessionId','')::uuid;
  next_anchor:=timegenie.direct_cloud_unassigned_session_snapshot(
    p_workspace_id,next_session_id
  );
  result_value:=timegenie.direct_cloud_mutation_response(
    p_workspace_id,p_device_id,p_operation_id,
    jsonb_build_object(
      'accepted',true,
      'session',timegenie.direct_cloud_unassigned_session_snapshot(p_workspace_id,session_id),
      'generatedEntry',timegenie.direct_cloud_time_entry_project(p_workspace_id,entry_id,boundary),
      'nextSession',next_anchor,
      'daySummary',timegenie.direct_cloud_day_time_summary(p_workspace_id,session_row.work_date),
      'serverTime',(extract(epoch from boundary)*1000)::bigint
    ),
    array['time','unassigned','tasks','recurrence'],p_expected_epoch
  );
  perform timegenie.record_direct_cloud_operation(
    p_workspace_id,p_operation_id,p_device_id,'direct_unassigned_resolve',request_value,result_value
  );
  return result_value;
end $$;

create or replace function timegenie.direct_cloud_unassigned_state(
  p_workspace_id uuid,p_device_id uuid,p_session_id uuid default null,
  p_expected_epoch bigint default null
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare
  target_date date:=timegenie.workspace_work_date(p_workspace_id,clock_timestamp());
  session_id uuid:=p_session_id;
  anchor jsonb;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  if session_id is null then
    select session.id into session_id from timegenie.unassigned_sessions session
    where session.workspace_id=p_workspace_id and session.work_date=target_date
      and session.state in ('collecting','awaiting_resolution')
    order by session.created_at limit 1;
  end if;
  if session_id is not null then
    anchor:=timegenie.direct_cloud_unassigned_session_snapshot(p_workspace_id,session_id);
  end if;
  return timegenie.direct_cloud_query_response(
    p_workspace_id,p_device_id,
    jsonb_build_object(
      'anchor',anchor,
      'workDate',target_date,
      'activeTimer',exists(select 1 from timegenie.time_entries entry
        where entry.workspace_id=p_workspace_id and entry.state in ('running','paused')
          and entry.deleted_at is null),
      'daySummary',timegenie.direct_cloud_day_time_summary(p_workspace_id,target_date)
    ),
    array['time','unassigned'],p_expected_epoch
  );
end $$;

revoke all on function timegenie.direct_cloud_unassigned_session_snapshot(uuid,uuid)
  from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_day_time_summary(uuid,date)
  from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_unassigned_anchor_ensure_locked(uuid,uuid,timestamptz)
  from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_unassigned_ensure(uuid,uuid,uuid,bigint,jsonb)
  from public,anon;
revoke all on function timegenie.direct_cloud_unassigned_resolve(uuid,uuid,uuid,bigint,jsonb)
  from public,anon;
revoke all on function timegenie.direct_cloud_unassigned_state(uuid,uuid,uuid,bigint)
  from public,anon;
grant execute on function timegenie.direct_cloud_unassigned_ensure(uuid,uuid,uuid,bigint,jsonb)
  to authenticated;
grant execute on function timegenie.direct_cloud_unassigned_resolve(uuid,uuid,uuid,bigint,jsonb)
  to authenticated;
grant execute on function timegenie.direct_cloud_unassigned_state(uuid,uuid,uuid,bigint)
  to authenticated;

notify pgrst,'reload schema';
