-- Direct-v2 timer and allocation mutations. The legacy timer implementation is
-- reused behind a direct-only protocol guard so old sync clients cannot reach
-- these writes, while each desktop transition remains one database transaction.

create or replace function timegenie.direct_cloud_timer_start(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,
  p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; raw_value jsonb; entry_id uuid; result_value jsonb;
declare lease_token uuid;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object(
    'expectedEpoch',p_expected_epoch,
    'payload',coalesce(p_payload,'{}'::jsonb)-'leaseToken'
  );
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':active-timer',0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_timer_start',request_value);
  if cached is not null then return cached; end if;
  lease_token:=nullif(p_payload->>'leaseToken','')::uuid;
  perform timegenie.direct_cloud_tracking_claim_lease(
    p_workspace_id,p_device_id,lease_token
  );
  raw_value:=timegenie.timer_start_pre_direct_v2(
    p_workspace_id,nullif(p_payload->>'taskId','')::uuid,p_device_id,
    timegenie.calendar_stable_uuid('direct-timer-start:'||p_operation_id::text),
    nullif(btrim(coalesce(p_payload->>'note','')),'')
  );
  entry_id:=(raw_value->>'id')::uuid;
  result_value:=timegenie.direct_cloud_mutation_response(
    p_workspace_id,p_device_id,p_operation_id,
    jsonb_build_object('entry',timegenie.direct_cloud_time_entry_project(p_workspace_id,entry_id)),
    array['time','unassigned'],p_expected_epoch
  );
  perform timegenie.record_direct_cloud_operation(
    p_workspace_id,p_operation_id,p_device_id,'direct_timer_start',request_value,result_value
  );
  return result_value;
end $$;

create or replace function timegenie.direct_cloud_timer_pause(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,
  p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; raw_value jsonb; entry_id uuid; result_value jsonb;
declare lease_token uuid;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object(
    'expectedEpoch',p_expected_epoch,
    'payload',coalesce(p_payload,'{}'::jsonb)-'leaseToken'
  );
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':active-timer',0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_timer_pause',request_value);
  if cached is not null then return cached; end if;
  lease_token:=nullif(p_payload->>'leaseToken','')::uuid;
  perform timegenie.direct_cloud_tracking_claim_lease(
    p_workspace_id,p_device_id,lease_token
  );
  entry_id:=nullif(p_payload->>'entryId','')::uuid;
  if entry_id is null or nullif(p_payload->>'expectedVersion','') is null then
    raise exception 'VALIDATION_ERROR: 计时暂停参数不完整';
  end if;
  raw_value:=timegenie.timer_pause_pre_direct_v2(
    p_workspace_id,entry_id,p_device_id,(p_payload->>'expectedVersion')::bigint,
    timegenie.calendar_stable_uuid('direct-timer-pause:'||p_operation_id::text)
  );
  result_value:=timegenie.direct_cloud_mutation_response(
    p_workspace_id,p_device_id,p_operation_id,
    jsonb_build_object('entry',timegenie.direct_cloud_time_entry_project(p_workspace_id,entry_id)),
    array['time'],p_expected_epoch
  );
  perform timegenie.record_direct_cloud_operation(
    p_workspace_id,p_operation_id,p_device_id,'direct_timer_pause',request_value,result_value
  );
  return result_value;
end $$;

create or replace function timegenie.direct_cloud_timer_resume(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,
  p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; raw_value jsonb; entry_id uuid; result_value jsonb;
declare lease_token uuid;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object(
    'expectedEpoch',p_expected_epoch,
    'payload',coalesce(p_payload,'{}'::jsonb)-'leaseToken'
  );
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':active-timer',0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_timer_resume',request_value);
  if cached is not null then return cached; end if;
  lease_token:=nullif(p_payload->>'leaseToken','')::uuid;
  perform timegenie.direct_cloud_tracking_claim_lease(
    p_workspace_id,p_device_id,lease_token
  );
  entry_id:=nullif(p_payload->>'entryId','')::uuid;
  if entry_id is null or nullif(p_payload->>'expectedVersion','') is null then
    raise exception 'VALIDATION_ERROR: 计时继续参数不完整';
  end if;
  raw_value:=timegenie.timer_resume_pre_direct_v2(
    p_workspace_id,entry_id,p_device_id,(p_payload->>'expectedVersion')::bigint,
    timegenie.calendar_stable_uuid('direct-timer-resume:'||p_operation_id::text)
  );
  result_value:=timegenie.direct_cloud_mutation_response(
    p_workspace_id,p_device_id,p_operation_id,
    jsonb_build_object('entry',timegenie.direct_cloud_time_entry_project(p_workspace_id,entry_id)),
    array['time'],p_expected_epoch
  );
  perform timegenie.record_direct_cloud_operation(
    p_workspace_id,p_operation_id,p_device_id,'direct_timer_resume',request_value,result_value
  );
  return result_value;
end $$;

create or replace function timegenie.direct_cloud_timer_stop(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,
  p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; raw_value jsonb; entry_id uuid; result_value jsonb;
declare stopped_row timegenie.time_entries%rowtype; slices_value jsonb; total_duration bigint;
declare total_settlement bigint; current_value jsonb; lease_token uuid;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object(
    'expectedEpoch',p_expected_epoch,
    'payload',coalesce(p_payload,'{}'::jsonb)-'leaseToken'
  );
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':active-timer',0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_timer_stop',request_value);
  if cached is not null then return cached; end if;
  lease_token:=nullif(p_payload->>'leaseToken','')::uuid;
  perform timegenie.direct_cloud_tracking_claim_lease(
    p_workspace_id,p_device_id,lease_token
  );
  entry_id:=nullif(p_payload->>'entryId','')::uuid;
  if entry_id is null or nullif(p_payload->>'expectedVersion','') is null then
    raise exception 'VALIDATION_ERROR: 计时停止参数不完整';
  end if;
  raw_value:=timegenie.timer_stop_pre_direct_v2(
    p_workspace_id,entry_id,p_device_id,(p_payload->>'expectedVersion')::bigint,
    timegenie.calendar_stable_uuid('direct-timer-stop:'||p_operation_id::text)
  );
  select entry.* into stopped_row from timegenie.time_entries entry
  where entry.workspace_id=p_workspace_id and entry.id=entry_id and entry.deleted_at is null;
  select coalesce(jsonb_agg(timegenie.direct_cloud_time_entry_project(p_workspace_id,entry.id)
    order by entry.work_date,entry.started_at,entry.id),'[]'::jsonb) into slices_value
  from timegenie.time_entries entry
  where entry.workspace_id=p_workspace_id and entry.deleted_at is null
    and coalesce(entry.timer_chain_id,entry.id)=coalesce(stopped_row.timer_chain_id,stopped_row.id);
  select coalesce(sum((item->>'durationSeconds')::bigint),0),
         coalesce(sum((item->>'settlementMinutes')::bigint),0)
  into total_duration,total_settlement
  from jsonb_array_elements(slices_value) item;
  current_value:=timegenie.direct_cloud_time_entry_project(p_workspace_id,entry_id);
  result_value:=timegenie.direct_cloud_mutation_response(
    p_workspace_id,p_device_id,p_operation_id,jsonb_build_object(
      'currentEntry',current_value,'slices',slices_value,
      'totalDurationSeconds',total_duration,'totalSettlementMinutes',total_settlement
    ),array['time','unassigned'],p_expected_epoch
  );
  perform timegenie.record_direct_cloud_operation(
    p_workspace_id,p_operation_id,p_device_id,'direct_timer_stop',request_value,result_value
  );
  return result_value;
end $$;

create or replace function timegenie.direct_cloud_time_allocation_replace(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,
  p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; entry_row timegenie.time_entries%rowtype;
declare groups jsonb:='{}'::jsonb; item jsonb; group_value jsonb; task_id uuid; minutes_value integer;
declare complete_value boolean; note_value text; expected_task_version bigint; previous_version bigint;
declare task_row timegenie.tasks%rowtype; total_minutes integer:=0; recurring_value boolean;
declare group_entry record;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_time_allocation_replace',request_value);
  if cached is not null then return cached; end if;
  select entry.* into entry_row from timegenie.time_entries entry
  where entry.workspace_id=p_workspace_id and entry.id=nullif(p_payload->>'entryId','')::uuid
    and entry.deleted_at is null for update;
  if not found then raise exception 'NOT_FOUND: 时间记录不存在'; end if;
  if entry_row.version<>coalesce((p_payload->>'expectedVersion')::bigint,-1) then
    raise exception 'VERSION_CONFLICT: 时间记录已被更新';
  end if;
  if entry_row.state<>'ended' then raise exception 'VALIDATION_ERROR: 只能分配已结束的时间记录'; end if;
  if entry_row.kind<>'work' and not(entry_row.kind='break' and entry_row.source_type='unassigned') then
    raise exception 'VALIDATION_ERROR: 只能分配工作记录或未归属处理记录';
  end if;
  if jsonb_typeof(coalesce(p_payload->'allocations','null'::jsonb))<>'array' then
    raise exception 'VALIDATION_ERROR: 分配参数必须是数组';
  end if;
  for item in select value from jsonb_array_elements(p_payload->'allocations') loop
    task_id:=nullif(item->>'taskId','')::uuid;
    minutes_value:=coalesce((item->>'minutes')::integer,0);
    complete_value:=coalesce((item->>'completeTask')::boolean,false);
    if task_id is null or minutes_value<0 then raise exception 'VALIDATION_ERROR: 分配事项或分钟无效'; end if;
    if minutes_value=0 and not complete_value then continue; end if;
    perform 1 from timegenie.tasks task
    where task.workspace_id=p_workspace_id and task.id=task_id and task.deleted_at is null
      and not exists(select 1 from timegenie.tasks child where child.workspace_id=p_workspace_id
        and child.parent_id=task_id and child.deleted_at is null)
    for update;
    if not found then raise exception 'TASK_NOT_SELECTABLE: 只允许分配叶子事项'; end if;
    group_value:=coalesce(groups->task_id::text,'{"minutes":0,"completeTask":false}'::jsonb);
    expected_task_version:=nullif(item->>'taskExpectedVersion','')::bigint;
    if group_value ? 'taskExpectedVersion' and expected_task_version is not null
       and (group_value->>'taskExpectedVersion')::bigint<>expected_task_version then
      raise exception 'VERSION_CONFLICT: 同一事项的版本信息不一致';
    end if;
    previous_version:=nullif(group_value->>'taskExpectedVersion','')::bigint;
    group_value:=jsonb_build_object(
      'minutes',(group_value->>'minutes')::integer+minutes_value,
      'completeTask',coalesce((group_value->>'completeTask')::boolean,false) or complete_value,
      'note',coalesce(nullif(item->>'note',''),group_value->>'note'),
      'taskExpectedVersion',coalesce(expected_task_version,previous_version)
    );
    groups:=jsonb_set(groups,array[task_id::text],group_value,true);
    total_minutes:=total_minutes+minutes_value;
  end loop;
  for group_entry in select key,value from jsonb_each(groups) loop
    task_id:=group_entry.key::uuid;
    complete_value:=coalesce((group_entry.value->>'completeTask')::boolean,false);
    expected_task_version:=nullif(group_entry.value->>'taskExpectedVersion','')::bigint;
    select task.* into task_row from timegenie.tasks task
    where task.workspace_id=p_workspace_id and task.id=task_id for update;
    if not found then raise exception 'NOT_FOUND: 事项不存在'; end if;
    if task_row.deleted_at is not null then raise exception 'ENTITY_DELETED: 事项已删除'; end if;
    recurring_value:=exists(
      select 1 from timegenie.task_recurrence_rules rule
      where rule.workspace_id=p_workspace_id and rule.task_id=task_id
        and (rule.effective_end is null
          or (rule.effective_start<=entry_row.work_date
            and (rule.effective_end is null or rule.effective_end>=entry_row.work_date)))
    );
    if recurring_value then
      perform timegenie.direct_cloud_occurrence_ensure(
        p_workspace_id,task_id,entry_row.work_date
      );
    end if;
    if not complete_value then continue; end if;
    if expected_task_version is null then
      raise exception 'VALIDATION_ERROR: 完成事项时缺少事项版本';
    end if;
    if task_row.version<>expected_task_version then
      raise exception 'VERSION_CONFLICT: 事项状态已变化，请刷新后重试';
    end if;
    if recurring_value then
      update timegenie.task_occurrences
      set status='done',completed_at=now(),updated_at=now(),version=version+1
      where workspace_id=p_workspace_id and task_id=task_id
        and occurrence_date=entry_row.work_date and status<>'done';
    elsif task_row.status<>'done' then
      update timegenie.tasks set status='done',completed_at=now(),updated_at=now(),
        updated_by_device_id=p_device_id,version=version+1 where id=task_id;
      insert into timegenie.task_status_events(
        id,workspace_id,task_id,status,occurred_at,source_type,created_at
      ) values(gen_random_uuid(),p_workspace_id,task_id,'done',now(),'user',now());
    end if;
  end loop;
  delete from timegenie.time_allocations where workspace_id=p_workspace_id and entry_id=entry_row.id;
  for group_entry in select key,value from jsonb_each(groups) loop
    minutes_value:=(group_entry.value->>'minutes')::integer;
    if minutes_value>0 then
      insert into timegenie.time_allocations(id,workspace_id,entry_id,task_id,minutes,note,version)
      values(gen_random_uuid(),p_workspace_id,entry_row.id,group_entry.key::uuid,minutes_value,
        nullif(group_entry.value->>'note',''),1);
    end if;
  end loop;
  update timegenie.time_entries set
    kind=case when total_minutes>0 then 'work' else kind end,
    label_snapshot=case when entry_row.kind='break' and total_minutes>0 then '未归属时间重新分配' else label_snapshot end,
    note=case when entry_row.kind='break' and total_minutes>0 then '从休息或无效时间重新分配' else note end,
    updated_at=now(),updated_by_device_id=p_device_id,version=version+1
  where id=entry_row.id;
  result_value:=timegenie.direct_cloud_mutation_response(
    p_workspace_id,p_device_id,p_operation_id,
    jsonb_build_object('entry',timegenie.direct_cloud_time_entry_project(p_workspace_id,entry_row.id)),
    array['time','tasks','planning','recurrence'],p_expected_epoch
  );
  perform timegenie.record_direct_cloud_operation(
    p_workspace_id,p_operation_id,p_device_id,'direct_time_allocation_replace',request_value,result_value
  );
  return result_value;
end $$;

create or replace function timegenie.direct_cloud_timer_transition(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,
  p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb;
declare active_row timegenie.time_entries%rowtype; active_found boolean:=false;
declare target_row timegenie.tasks%rowtype; target_task_id uuid; created_task_id uuid;
declare parent_row timegenie.tasks%rowtype; parent_id uuid; expected_parent_version bigint;
declare subject_id uuid; work_date date; transition_at timestamptz:=now();
declare current_entry_id uuid; current_expected_version bigint; reopen_version bigint;
declare new_task jsonb; task_title text; day_date date; sort_value integer;
declare previous_stop_result jsonb; slices_value jsonb; total_duration bigint;
declare total_settlement bigint; active_entry jsonb; created_task jsonb;
declare recurring_value boolean:=false; occurrence_value jsonb; plan_value jsonb;
declare lease_token uuid;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object(
    'expectedEpoch',p_expected_epoch,
    'payload',coalesce(p_payload,'{}'::jsonb)-'leaseToken'
  );
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':active-timer',0));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':planning',0));
  cached:=timegenie.direct_cloud_operation_result(
    p_workspace_id,p_operation_id,'direct_timer_transition',request_value
  );
  if cached is not null then return cached; end if;
  lease_token:=nullif(p_payload->>'leaseToken','')::uuid;
  perform timegenie.direct_cloud_tracking_claim_lease(
    p_workspace_id,p_device_id,lease_token
  );

  if (nullif(p_payload->>'taskId','') is null) = (nullif(p_payload->'newTask','null'::jsonb) is null) then
    raise exception 'VALIDATION_ERROR: 必须且只能选择一个目标事项';
  end if;
  current_entry_id:=nullif(p_payload->>'currentEntryId','')::uuid;
  current_expected_version:=nullif(p_payload->>'currentEntryExpectedVersion','')::bigint;
  if (current_entry_id is null)<>(current_expected_version is null) then
    raise exception 'VALIDATION_ERROR: 当前计时 ID 和版本必须同时提供';
  end if;
  reopen_version:=nullif(p_payload->>'reopenCompletedTaskVersion','')::bigint;
  work_date:=timegenie.workspace_work_date(p_workspace_id,transition_at);

  if current_entry_id is not null then
    select entry.* into active_row from timegenie.time_entries entry
    where entry.workspace_id=p_workspace_id and entry.id=current_entry_id
      and entry.deleted_at is null for update;
    if not found then raise exception 'NOT_FOUND: 当前计时不存在'; end if;
    if active_row.state not in('running','paused') then
      raise exception 'VERSION_CONFLICT: 当前计时状态已变化，请重新加载';
    end if;
    if active_row.version<>current_expected_version then
      raise exception 'VERSION_CONFLICT: 当前计时版本已变化，请重新加载';
    end if;
    active_found:=true;
  else
    select entry.* into active_row from timegenie.time_entries entry
    where entry.workspace_id=p_workspace_id and entry.state in('running','paused')
      and entry.deleted_at is null for update;
    active_found:=found;
    if active_found then
      raise exception 'VERSION_CONFLICT: 当前计时状态已变化，请重新加载';
    end if;
  end if;

  if nullif(p_payload->>'taskId','') is not null then
    target_task_id:=(p_payload->>'taskId')::uuid;
    select task.* into target_row from timegenie.tasks task
    where task.workspace_id=p_workspace_id and task.id=target_task_id for update;
    if not found then raise exception 'NOT_FOUND: 目标事项不存在'; end if;
    if target_row.deleted_at is not null then raise exception 'ENTITY_DELETED: 目标事项已删除'; end if;
    if exists(select 1 from timegenie.tasks child
      where child.workspace_id=p_workspace_id and child.parent_id=target_task_id
        and child.deleted_at is null) then
      raise exception 'TASK_NOT_SELECTABLE: 父事项不能直接计时';
    end if;
    if reopen_version is not null then
      if target_row.status<>'done' or target_row.version<>reopen_version then
        raise exception 'VERSION_CONFLICT: 已完成事项状态已变化，请重新加载';
      end if;
      if target_row.completed_at is null
         or timegenie.workspace_work_date(p_workspace_id,target_row.completed_at)<>work_date then
        raise exception 'VALIDATION_ERROR: 只能继续今天完成的事项';
      end if;
      if exists(select 1 from timegenie.task_recurrence_rules rule
        where rule.workspace_id=p_workspace_id and rule.task_id=target_task_id
          and rule.effective_end is null) then
        raise exception 'VALIDATION_ERROR: 重复事项不能通过普通事项继续动作重新打开';
      end if;
    elsif target_row.status<>'open' then
      raise exception 'TASK_NOT_SELECTABLE: 已完成事项需使用继续动作';
    end if;
  else
    new_task:=p_payload->'newTask';
    subject_id:=nullif(new_task->>'subjectId','')::uuid;
    parent_id:=nullif(new_task->>'parentId','')::uuid;
    expected_parent_version:=nullif(new_task->>'expectedParentVersion','')::bigint;
    task_title:=btrim(coalesce(new_task->>'title',''));
    day_date:=nullif(new_task->>'dayInstanceDate','')::date;
    if subject_id is null or task_title='' then
      raise exception 'VALIDATION_ERROR: 快速事项的主体和名称不能为空';
    end if;
    perform 1 from timegenie.subjects subject
    where subject.workspace_id=p_workspace_id and subject.id=subject_id
      and subject.deleted_at is null for update;
    if not found then raise exception 'NOT_FOUND: 主体不存在或已归档'; end if;
    if parent_id is not null then
      select task.* into parent_row from timegenie.tasks task
      where task.workspace_id=p_workspace_id and task.id=parent_id for update;
      if not found then raise exception 'NOT_FOUND: 父事项不存在'; end if;
      if expected_parent_version is not null and parent_row.version<>expected_parent_version then
        raise exception 'VERSION_CONFLICT: 父事项已变化，请重新加载';
      end if;
      if day_date is null then day_date:=parent_row.day_instance_date;
      elsif day_date is distinct from parent_row.day_instance_date then
        raise exception 'VALIDATION_ERROR: 今日子事项必须属于同一实例日期';
      end if;
      perform timegenie.direct_cloud_task_parent_assert(
        p_workspace_id,subject_id,parent_id,day_date,null
      );
    end if;
    select coalesce(max(task.sort_order),0)+10 into sort_value
    from timegenie.tasks task
    where task.workspace_id=p_workspace_id and task.subject_id=subject_id
      and task.parent_id is not distinct from parent_id
      and task.day_instance_date is not distinct from day_date
      and task.deleted_at is null;
    created_task_id:=gen_random_uuid();
    insert into timegenie.tasks(
      id,workspace_id,subject_id,parent_id,title,status,planned_date,estimate_minutes,
      note,project_name,solution_name,source_type,sort_order,created_by_device_id,
      updated_by_device_id,day_instance_date,day_instance_kind
    ) values(
      created_task_id,p_workspace_id,subject_id,parent_id,task_title,'open',
      case when day_date is null then work_date else null end,null,null,null,null,
      'timer_quick_create',sort_value,p_device_id,p_device_id,day_date,
      case when day_date is null then null else 'member' end
    );
    target_task_id:=created_task_id;
    created_task:=timegenie.direct_cloud_task_project(p_workspace_id,created_task_id);
    if day_date is null then
      plan_value:=timegenie.direct_cloud_day_plan_apply(
        p_workspace_id,p_device_id,target_task_id,work_date,'included',null
      );
    end if;
  end if;

  if active_found and created_task_id is null and reopen_version is null
     and active_row.default_task_id=target_task_id then
    active_entry:=timegenie.direct_cloud_time_entry_project(
      p_workspace_id,active_row.id,transition_at
    );
    result_value:=timegenie.direct_cloud_mutation_response(
      p_workspace_id,p_device_id,p_operation_id,jsonb_build_object(
        'createdTask',null,'previousStopResult',null,'activeEntry',active_entry,
        'transitionAt',(extract(epoch from transition_at)*1000)::bigint,'noOp',true
      ),array['time'],p_expected_epoch
    );
    perform timegenie.record_direct_cloud_operation(
      p_workspace_id,p_operation_id,p_device_id,'direct_timer_transition',
      request_value,result_value
    );
    return result_value;
  end if;

  if active_found then
    if active_row.state='running' then
      update timegenie.time_segments set ended_at=transition_at,
        duration_seconds=greatest(extract(epoch from(transition_at-started_at))::bigint,0)
      where workspace_id=p_workspace_id and entry_id=active_row.id and ended_at is null;
    end if;
    update timegenie.time_entries set state='ended',ended_at=transition_at,
      duration_seconds=coalesce((select sum(segment.duration_seconds)
        from timegenie.time_segments segment
        where segment.workspace_id=p_workspace_id and segment.entry_id=active_row.id),0),
      updated_at=transition_at,updated_by_device_id=p_device_id,version=version+1
    where id=active_row.id;
    select coalesce(jsonb_agg(timegenie.direct_cloud_time_entry_project(
      p_workspace_id,entry.id,transition_at) order by entry.work_date,entry.started_at,entry.id),
      '[]'::jsonb) into slices_value
    from timegenie.time_entries entry
    where entry.workspace_id=p_workspace_id and entry.deleted_at is null
      and coalesce(entry.timer_chain_id,entry.id)=coalesce(active_row.timer_chain_id,active_row.id);
    select coalesce(sum((item->>'durationSeconds')::bigint),0),
           coalesce(sum((item->>'settlementMinutes')::bigint),0)
    into total_duration,total_settlement
    from jsonb_array_elements(slices_value) item;
    previous_stop_result:=jsonb_build_object(
      'currentEntry',(select value from jsonb_array_elements(slices_value) value
        where value->>'id'=active_row.id::text limit 1),
      'slices',slices_value,'totalDurationSeconds',total_duration,
      'totalSettlementMinutes',total_settlement
    );
  end if;

  if reopen_version is not null then
    update timegenie.tasks set status='open',completed_at=null,updated_at=transition_at,
      updated_by_device_id=p_device_id,version=version+1
    where workspace_id=p_workspace_id and id=target_task_id;
    insert into timegenie.task_status_events(
      id,workspace_id,task_id,status,occurred_at,source_type,created_at
    ) values(gen_random_uuid(),p_workspace_id,target_task_id,'open',transition_at,'user',transition_at);
  end if;

  select task.title into task_title from timegenie.tasks task
  where task.workspace_id=p_workspace_id and task.id=target_task_id;
  if task_title is null then raise exception 'NOT_FOUND: 目标事项不存在'; end if;
  recurring_value:=exists(
    select 1 from timegenie.task_recurrence_rules rule
    where rule.workspace_id=p_workspace_id and rule.task_id=target_task_id
      and (rule.effective_end is null
        or (rule.effective_start<=work_date
          and (rule.effective_end is null or rule.effective_end>=work_date)))
  );
  if recurring_value and not exists(
    select 1 from timegenie.tasks task
    where task.workspace_id=p_workspace_id and task.id=target_task_id
      and task.day_instance_date is not null
  ) then
    occurrence_value:=timegenie.direct_cloud_occurrence_ensure(
      p_workspace_id,target_task_id,work_date
    );
  end if;

  insert into timegenie.time_entries(
    id,workspace_id,work_date,kind,source_type,state,default_task_id,label_snapshot,
    started_at,duration_seconds,note,created_at,updated_at,version,created_by_device_id,
    updated_by_device_id,last_continuous_at
  ) values(
    gen_random_uuid(),p_workspace_id,work_date,'work','timer','running',target_task_id,
    task_title,transition_at,0,nullif(btrim(coalesce(p_payload->>'note','')),''),
    transition_at,transition_at,1,p_device_id,p_device_id,transition_at
  ) returning id into active_row.id;
  insert into timegenie.time_segments(
    id,workspace_id,entry_id,sequence_no,started_at,duration_seconds
  ) values(gen_random_uuid(),p_workspace_id,active_row.id,1,transition_at,0);
  perform timegenie.timegenie_unassigned_system_clear(p_workspace_id,transition_at);
  active_entry:=timegenie.direct_cloud_time_entry_project(
    p_workspace_id,active_row.id,transition_at
  );
  result_value:=timegenie.direct_cloud_mutation_response(
    p_workspace_id,p_device_id,p_operation_id,jsonb_build_object(
      'createdTask',created_task,'previousStopResult',previous_stop_result,
      'activeEntry',active_entry,
      'transitionAt',(extract(epoch from transition_at)*1000)::bigint,'noOp',false
    ),array['time','tasks','planning','recurrence','unassigned'],p_expected_epoch
  );
  perform timegenie.record_direct_cloud_operation(
    p_workspace_id,p_operation_id,p_device_id,'direct_timer_transition',
    request_value,result_value
  );
  return result_value;
end $$;

revoke all on function timegenie.direct_cloud_timer_start(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_timer_pause(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_timer_resume(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_timer_stop(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_time_allocation_replace(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_timer_transition(uuid,uuid,uuid,bigint,jsonb) from public,anon;
grant execute on function timegenie.direct_cloud_timer_start(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_timer_pause(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_timer_resume(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_timer_stop(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_time_allocation_replace(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_timer_transition(uuid,uuid,uuid,bigint,jsonb) to authenticated;

notify pgrst,'reload schema';
