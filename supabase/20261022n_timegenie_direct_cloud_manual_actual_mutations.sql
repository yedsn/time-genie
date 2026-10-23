-- Direct-v2 manual time and actual-time adjustment operations.
-- Page snapshots and all compound writes use authoritative Supabase data.

begin;

create or replace function timegenie.direct_cloud_actual_adjustment_project(
  p_workspace_id uuid,p_adjustment_id uuid
) returns jsonb language sql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
  select jsonb_build_object(
    'id',adjustment.id,'subjectId',adjustment.subject_id,'taskId',adjustment.task_id,
    'taskTitle',coalesce(task.title,'已删除事项'),'workDate',adjustment.work_date,
    'deltaMinutes',adjustment.delta_minutes,'baselineMinutes',adjustment.baseline_minutes,
    'targetMinutes',adjustment.target_minutes,'note',adjustment.note,
    'batchId',adjustment.batch_id,
    'createdAt',(extract(epoch from adjustment.created_at)*1000)::bigint,
    'version',adjustment.version,
    'deletedAt',case when adjustment.deleted_at is null then null
      else (extract(epoch from adjustment.deleted_at)*1000)::bigint end
  )
  from timegenie.actual_time_adjustments adjustment
  left join timegenie.tasks task on task.id=adjustment.task_id
  where adjustment.workspace_id=p_workspace_id and adjustment.id=p_adjustment_id;
$$;

create or replace function timegenie.direct_cloud_actual_minutes(
  p_workspace_id uuid,p_task_id uuid,p_work_date date
) returns bigint language sql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
  select greatest(
    coalesce((
      select sum(allocation.minutes) from timegenie.time_allocations allocation
      join timegenie.time_entries entry on entry.id=allocation.entry_id
      where allocation.workspace_id=p_workspace_id and allocation.task_id=p_task_id
        and entry.work_date=p_work_date and entry.deleted_at is null
    ),0)+coalesce((
      select sum(adjustment.delta_minutes)
      from timegenie.actual_time_adjustments adjustment
      where adjustment.workspace_id=p_workspace_id and adjustment.task_id=p_task_id
        and adjustment.work_date=p_work_date and adjustment.deleted_at is null
    ),0),0
  );
$$;

create or replace function timegenie.direct_cloud_actual_adjustment_snapshot(
  p_workspace_id uuid,p_device_id uuid,p_subject_id uuid,p_work_date date,
  p_execution_only boolean default false,p_expected_epoch bigint default null
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare day_start timestamptz; day_end timestamptz; data_value jsonb;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  if not exists(select 1 from timegenie.subjects subject where subject.workspace_id=p_workspace_id and subject.id=p_subject_id and subject.deleted_at is null) then
    raise exception 'NOT_FOUND: 主体不存在';
  end if;
  day_start:=timegenie.workspace_day_start(p_workspace_id,p_work_date);
  day_end:=timegenie.workspace_day_end(p_workspace_id,p_work_date);

  with recursive visible as (
    select task.* from timegenie.tasks task
    where task.workspace_id=p_workspace_id and task.subject_id=p_subject_id
      and task.deleted_at is null
      and (task.day_instance_date is null or task.day_instance_date=p_work_date)
      and (not p_execution_only or task.day_instance_date=p_work_date)
      and (task.status='open'
        or (task.completed_at>=day_start and task.completed_at<day_end)
        or exists(select 1 from timegenie.time_allocations allocation join timegenie.time_entries entry on entry.id=allocation.entry_id
          where allocation.task_id=task.id and entry.work_date=p_work_date and entry.deleted_at is null)
        or exists(select 1 from timegenie.actual_time_adjustments adjustment
          where adjustment.task_id=task.id and adjustment.work_date=p_work_date and adjustment.deleted_at is null)
        or exists(select 1 from timegenie.tasks child where child.parent_id=task.id and child.deleted_at is null
          and (child.status='open' or (child.completed_at>=day_start and child.completed_at<day_end)
            or exists(select 1 from timegenie.time_allocations child_allocation join timegenie.time_entries child_entry on child_entry.id=child_allocation.entry_id
              where child_allocation.task_id=child.id and child_entry.work_date=p_work_date and child_entry.deleted_at is null)
            or exists(select 1 from timegenie.actual_time_adjustments child_adjustment
              where child_adjustment.task_id=child.id and child_adjustment.work_date=p_work_date and child_adjustment.deleted_at is null))))
  ), direct_minutes as (
    select visible.id,timegenie.direct_cloud_actual_minutes(p_workspace_id,visible.id,p_work_date) as minutes
    from visible
  ), totals as (
    select visible.id,coalesce((
      select sum(child_minutes.minutes) from direct_minutes child_minutes
      join visible child on child.id=child_minutes.id
      where child.id=visible.id or child.parent_id=visible.id
    ),0)::bigint as minutes from visible
  ), projected as (
    select visible.*,direct.minutes as direct_value,totals.minutes as total_value,
      not exists(select 1 from timegenie.tasks child where child.parent_id=visible.id and child.deleted_at is null) as is_leaf,
      exists(select 1 from timegenie.time_entries entry where entry.workspace_id=p_workspace_id
        and entry.default_task_id=visible.id and entry.deleted_at is null and entry.state in('running','paused')) as has_timer,
      (select entry.state from timegenie.time_entries entry where entry.workspace_id=p_workspace_id
        and entry.default_task_id=visible.id and entry.deleted_at is null and entry.state in('running','paused') limit 1) as timer_state,
      visible.parent_id is null and visible.status='open'
        and not exists(select 1 from timegenie.task_recurrence_rules rule where rule.task_id=visible.id and rule.effective_end is null)
        and not exists(select 1 from timegenie.time_allocations allocation join timegenie.time_entries entry on entry.id=allocation.entry_id
          where allocation.task_id=visible.id and entry.deleted_at is null)
        and not exists(select 1 from timegenie.actual_time_adjustments adjustment
          where adjustment.task_id=visible.id and adjustment.deleted_at is null) as can_accept_children
    from visible join direct_minutes direct on direct.id=visible.id join totals on totals.id=visible.id
  )
  select jsonb_build_object(
    'subjectId',p_subject_id,'workDate',p_work_date,
    'tasks',coalesce((select jsonb_agg(jsonb_build_object(
      'id',task.id,'subjectId',task.subject_id,'parentId',task.parent_id,
      'dayInstanceDate',task.day_instance_date,'dayInstanceKind',task.day_instance_kind,
      'dayInstanceSourceTaskId',task.day_instance_source_task_id,
      'title',task.title,'note',task.note,'status',task.status,
      'completedAt',case when task.completed_at is null then null else (extract(epoch from task.completed_at)*1000)::bigint end,
      'completedToday',coalesce(task.completed_at>=day_start and task.completed_at<day_end,false),
      'plannedDate',task.planned_date,'estimateMinutes',task.estimate_minutes,
      'todayEstimateMinutes',estimate.estimate_minutes,'dailyEstimateVersion',estimate.version,
      'sortOrder',task.sort_order,'version',task.version,'isLeaf',task.is_leaf,
      'selectable',task.is_leaf,'directMinutes',task.direct_value,'totalMinutes',task.total_value,
      'adjustmentFingerprint',timegenie.actual_adjustment_fingerprint(p_workspace_id,task.id,p_work_date),
      'timerState',task.timer_state,'canAcceptChildren',task.can_accept_children
    ) order by task.sort_order,task.created_at,task.id) from projected task
      left join timegenie.task_daily_estimates estimate on estimate.workspace_id=p_workspace_id
        and estimate.task_id=task.id and estimate.work_date=p_work_date),'[]'::jsonb),
    'adjustments',coalesce((select jsonb_agg(timegenie.direct_cloud_actual_adjustment_project(p_workspace_id,adjustment.id)
      order by adjustment.created_at desc,adjustment.id desc)
      from timegenie.actual_time_adjustments adjustment
      where adjustment.workspace_id=p_workspace_id and adjustment.subject_id=p_subject_id
        and adjustment.work_date=p_work_date and adjustment.deleted_at is null),'[]'::jsonb)
  ) into data_value;
  return timegenie.direct_cloud_query_response(
    p_workspace_id,p_device_id,data_value,array['time','tasks'],p_expected_epoch
  );
end $$;

create or replace function timegenie.direct_cloud_manual_time_create(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; task_row timegenie.tasks%rowtype;
declare entry_id uuid; task_id uuid; work_date_value date; started_at_value timestamptz; ended_at_value timestamptz;
declare duration_seconds bigint; settlement_minutes integer; label_value text; complete_value boolean;
declare expected_task_version bigint; recurring_value boolean; occurred_at timestamptz:=now();
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_manual_time_create',request_value);
  if cached is not null then return cached; end if;
  work_date_value:=nullif(p_payload->>'workDate','')::date;
  task_id:=nullif(p_payload->>'taskId','')::uuid;
  complete_value:=coalesce((p_payload->>'completeTask')::boolean,false);
  expected_task_version:=nullif(p_payload->>'taskExpectedVersion','')::bigint;
  if work_date_value is null then raise exception 'VALIDATION_ERROR: 手工时间缺少工作日期'; end if;
  if nullif(p_payload->>'minutes','') is not null
     and p_payload->>'startedAt' is null and p_payload->>'endedAt' is null
     and (p_payload->>'minutes')::bigint>0 then
    ended_at_value:=occurred_at;
    started_at_value:=ended_at_value-((p_payload->>'minutes')::bigint*interval '1 minute');
  elsif p_payload->>'minutes' is null
     and nullif(p_payload->>'startedAt','') is not null
     and nullif(p_payload->>'endedAt','') is not null then
    started_at_value:=to_timestamp((p_payload->>'startedAt')::double precision/1000);
    ended_at_value:=to_timestamp((p_payload->>'endedAt')::double precision/1000);
    if ended_at_value<=started_at_value then
      raise exception 'VALIDATION_ERROR: 手工时间结束时间必须晚于开始时间';
    end if;
  else
    raise exception 'VALIDATION_ERROR: 请填写正整数分钟，或同时填写有效的开始与结束时间';
  end if;
  duration_seconds:=floor(extract(epoch from(ended_at_value-started_at_value)))::bigint;
  settlement_minutes:=case when duration_seconds<=0 then 0 else ((duration_seconds+59)/60)::integer end;

  if task_id is not null then
    select task.* into task_row from timegenie.tasks task
    where task.workspace_id=p_workspace_id and task.id=task_id for update;
    if not found then raise exception 'NOT_FOUND: 事项不存在'; end if;
    if task_row.deleted_at is not null then raise exception 'ENTITY_DELETED: 事项已删除'; end if;
    if exists(select 1 from timegenie.tasks child where child.workspace_id=p_workspace_id
      and child.parent_id=task_id and child.deleted_at is null) then
      raise exception 'TASK_NOT_SELECTABLE: 手工时间只能关联叶子事项';
    end if;
    label_value:=task_row.title;
    recurring_value:=exists(select 1 from timegenie.task_recurrence_rules rule
      where rule.workspace_id=p_workspace_id and rule.task_id=task_id
        and (rule.effective_end is null or (rule.effective_start<=work_date_value and rule.effective_end>=work_date_value)));
    if settlement_minutes>0 and recurring_value then
      perform timegenie.direct_cloud_occurrence_ensure(p_workspace_id,task_id,work_date_value);
    end if;
    if settlement_minutes>0 and complete_value then
      if expected_task_version is null then raise exception 'VALIDATION_ERROR: 完成事项时缺少事项版本'; end if;
      if task_row.version<>expected_task_version then
        raise exception 'VERSION_CONFLICT: 事项状态已变化，请刷新后重试'
          using detail=jsonb_build_object('code','VERSION_CONFLICT','taskId',task_id)::text;
      end if;
      if recurring_value then
        update timegenie.task_occurrences occurrence set status='done',completed_at=occurred_at,
          updated_at=occurred_at,version=occurrence.version+1
        where occurrence.workspace_id=p_workspace_id and occurrence.task_id=task_id
          and occurrence.occurrence_date=work_date_value and occurrence.status<>'done';
      elsif task_row.status<>'done' then
        update timegenie.tasks task set status='done',completed_at=occurred_at,updated_at=occurred_at,
          updated_by_device_id=p_device_id,version=task.version+1
        where task.workspace_id=p_workspace_id and task.id=task_id;
        insert into timegenie.task_status_events(id,workspace_id,task_id,status,occurred_at,source_type,created_at)
        values(gen_random_uuid(),p_workspace_id,task_id,'done',occurred_at,'user',occurred_at);
      end if;
    end if;
  else
    if complete_value then raise exception 'VALIDATION_ERROR: 未关联事项不能标记完成'; end if;
    label_value:='未关联事项';
  end if;

  entry_id:=timegenie.calendar_stable_uuid('direct-manual-entry:'||p_operation_id::text);
  insert into timegenie.time_entries(
    id,workspace_id,work_date,kind,source_type,state,default_task_id,label_snapshot,
    started_at,ended_at,duration_seconds,note,created_at,updated_at,version,
    created_by_device_id,updated_by_device_id
  ) values(
    entry_id,p_workspace_id,work_date_value,'work','manual','ended',task_id,label_value,
    started_at_value,ended_at_value,duration_seconds,nullif(p_payload->>'note',''),
    occurred_at,occurred_at,1,p_device_id,p_device_id
  );
  insert into timegenie.time_segments(
    id,workspace_id,entry_id,sequence_no,started_at,ended_at,duration_seconds
  ) values(
    timegenie.calendar_stable_uuid('direct-manual-segment:'||p_operation_id::text),
    p_workspace_id,entry_id,1,started_at_value,ended_at_value,duration_seconds
  );
  if task_id is not null and settlement_minutes>0 then
    insert into timegenie.time_allocations(
      id,workspace_id,entry_id,task_id,minutes,created_at,updated_at,version
    ) values(
      timegenie.calendar_stable_uuid('direct-manual-allocation:'||p_operation_id::text),
      p_workspace_id,entry_id,task_id,settlement_minutes,occurred_at,occurred_at,1
    );
  end if;
  result_value:=timegenie.direct_cloud_mutation_response(
    p_workspace_id,p_device_id,p_operation_id,
    jsonb_build_object('entry',timegenie.direct_cloud_time_entry_project(p_workspace_id,entry_id,occurred_at)),
    array['time','tasks','recurrence'],p_expected_epoch
  );
  perform timegenie.record_direct_cloud_operation(
    p_workspace_id,p_operation_id,p_device_id,'direct_manual_time_create',request_value,result_value
  );
  return result_value;
end $$;

create or replace function timegenie.direct_cloud_actual_adjustment_batch_apply(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; item jsonb; draft jsonb;
declare task_row timegenie.tasks%rowtype; parent_row timegenie.tasks%rowtype; task_id uuid; adjustment_id uuid;
declare subject_id uuid; work_date_value date; batch_id uuid; day_instance_date date;
declare current_date_value date; current_actual bigint; effective_delta integer; note_value text;
declare created_tasks jsonb:='[]'::jsonb; adjustments jsonb:='[]'::jsonb;
declare seen_item_ids text[]:='{}'::text[]; seen_draft_ids text[]:='{}'::text[];
declare sort_value integer; now_value timestamptz:=now(); task_title text; request_hash text;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_actual_adjustment_batch_apply',request_value);
  if cached is not null then return cached; end if;
  if jsonb_typeof(coalesce(p_payload->'items','[]'::jsonb))<>'array'
     or jsonb_typeof(coalesce(p_payload->'newTasks','[]'::jsonb))<>'array' then
    raise exception 'VALIDATION_ERROR: 实际工时调整列表无效';
  end if;
  subject_id:=nullif(p_payload->>'subjectId','')::uuid;
  work_date_value:=nullif(p_payload->>'workDate','')::date;
  batch_id:=nullif(p_payload->>'batchId','')::uuid;
  day_instance_date:=nullif(p_payload->>'dayInstanceDate','')::date;
  note_value:=nullif(btrim(p_payload->>'note'),'');
  if subject_id is null or work_date_value is null or batch_id is null
     or p_payload->>'clientRequestId' is distinct from p_operation_id::text then
    raise exception 'VALIDATION_ERROR: 实际工时调整批次标识无效';
  end if;
  if jsonb_array_length(coalesce(p_payload->'items','[]'::jsonb))=0
     and jsonb_array_length(coalesce(p_payload->'newTasks','[]'::jsonb))=0 then
    raise exception 'VALIDATION_ERROR: 调整批次不能为空';
  end if;
  select (now_value at time zone workspace.timezone)::date into current_date_value
  from timegenie.workspaces workspace where workspace.id=p_workspace_id;
  if work_date_value<>current_date_value then
    raise exception 'HISTORICAL_RECORD_READ_ONLY: % 的历史记录不可修改（当前工作日期为 %）',
      work_date_value,current_date_value;
  end if;
  if day_instance_date is not null and day_instance_date<>work_date_value then
    raise exception 'VALIDATION_ERROR: 今日实例日期必须等于调整日期';
  end if;
  if not exists(select 1 from timegenie.subjects subject where subject.workspace_id=p_workspace_id
    and subject.id=subject_id and subject.deleted_at is null) then raise exception 'NOT_FOUND: 主体不存在'; end if;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||subject_id::text||':'||work_date_value::text,0));
  request_hash:=encode(extensions.digest(request_value::text,'sha256'),'hex');
  if exists(select 1 from timegenie.actual_adjustment_batches batch
    where batch.workspace_id=p_workspace_id and batch.batch_id=batch_id) then
    raise exception 'IDEMPOTENCY_CONFLICT: 调整批次标识已被使用';
  end if;

  for item in select value from jsonb_array_elements(coalesce(p_payload->'items','[]'::jsonb)) loop
    if nullif(item->>'taskId','') is null or item->>'taskId'=any(seen_item_ids) then
      raise exception 'VALIDATION_ERROR: 调整事项标识无效或重复';
    end if;
    seen_item_ids:=array_append(seen_item_ids,item->>'taskId');
    if coalesce((item->>'targetMinutes')::integer,-1) not between 0 and 1440
       or coalesce((item->>'baselineMinutes')::integer,-1)<0
       or (item->>'targetMinutes')::integer-(item->>'baselineMinutes')::integer<>(item->>'deltaMinutes')::integer then
      raise exception 'VALIDATION_ERROR: 目标分钟或固定差额无效';
    end if;
    if (item->>'deltaMinutes')::integer=0 then continue; end if;
    task_id:=(item->>'taskId')::uuid;
    select task.* into task_row from timegenie.tasks task
    where task.workspace_id=p_workspace_id and task.id=task_id for update;
    if not found or task_row.deleted_at is not null or task_row.subject_id<>subject_id
       or exists(select 1 from timegenie.tasks child where child.parent_id=task_id and child.deleted_at is null) then
      raise exception 'VERSION_CONFLICT: 实际工时事项结构已变化';
    end if;
    if task_row.version<>nullif(item->>'expectedTaskVersion','')::bigint then
      raise exception 'VERSION_CONFLICT: 事项已变化，请重新加载';
    end if;
    if timegenie.actual_adjustment_fingerprint(p_workspace_id,task_id,work_date_value)
       is distinct from coalesce(item->>'adjustmentFingerprint','') then
      raise exception 'ADJUSTMENT_BASELINE_CONFLICT: 实际调整记录已变化，请重新加载';
    end if;
    current_actual:=timegenie.direct_cloud_actual_minutes(p_workspace_id,task_id,work_date_value);
    effective_delta:=(item->>'targetMinutes')::integer-current_actual;
    if effective_delta=0 then continue; end if;
    if current_actual+effective_delta<0 then raise exception 'VALIDATION_ERROR: 调整后实际时间不能小于 0'; end if;
    adjustment_id:=timegenie.calendar_stable_uuid('direct-actual-adjustment:'||p_operation_id::text||':'||task_id::text);
    insert into timegenie.actual_time_adjustments(
      id,workspace_id,subject_id,task_id,work_date,delta_minutes,baseline_minutes,target_minutes,
      note,source_type,batch_id,created_at,updated_at,version,created_by_device_id,updated_by_device_id
    ) values(
      adjustment_id,p_workspace_id,subject_id,task_id,work_date_value,effective_delta,current_actual,
      (item->>'targetMinutes')::integer,note_value,'quick_actual_adjustment',batch_id,
      now_value,now_value,1,p_device_id,p_device_id
    );
    adjustments:=adjustments||jsonb_build_array(
      timegenie.direct_cloud_actual_adjustment_project(p_workspace_id,adjustment_id)
    );
  end loop;

  for draft in select value from jsonb_array_elements(coalesce(p_payload->'newTasks','[]'::jsonb)) loop
    if nullif(btrim(draft->>'clientTaskId'),'') is null
       or draft->>'clientTaskId'=any(seen_draft_ids) then
      raise exception 'VALIDATION_ERROR: 新事项草稿标识无效或重复';
    end if;
    seen_draft_ids:=array_append(seen_draft_ids,draft->>'clientTaskId');
    task_title:=btrim(coalesce(draft->>'title',''));
    if task_title='' or coalesce((draft->>'targetMinutes')::integer,-1) not between 0 and 1440 then
      raise exception 'VALIDATION_ERROR: 新事项名称或目标分钟无效';
    end if;
    if nullif(draft->>'parentId','') is not null then
      select parent.* into parent_row from timegenie.tasks parent
      where parent.workspace_id=p_workspace_id and parent.id=(draft->>'parentId')::uuid for update;
      if not found or parent_row.deleted_at is not null or parent_row.subject_id<>subject_id
         or parent_row.parent_id is not null or parent_row.status<>'open'
         or (day_instance_date is not null and parent_row.day_instance_date is distinct from day_instance_date)
         or (day_instance_date is null and parent_row.day_instance_date is not null)
         or exists(select 1 from timegenie.task_recurrence_rules rule where rule.task_id=parent_row.id and rule.effective_end is null)
         or exists(select 1 from timegenie.time_allocations allocation join timegenie.time_entries entry on entry.id=allocation.entry_id
           where allocation.task_id=parent_row.id and entry.deleted_at is null)
         or exists(select 1 from timegenie.actual_time_adjustments adjustment
           where adjustment.task_id=parent_row.id and adjustment.deleted_at is null) then
        raise exception 'VERSION_CONFLICT: 新事项父项已不再可用';
      end if;
      if parent_row.version<>nullif(draft->>'expectedParentVersion','')::bigint then
        raise exception 'VERSION_CONFLICT: 父事项已变化，请重新加载';
      end if;
    elsif draft->'expectedParentVersion' is not null and draft->'expectedParentVersion'<>'null'::jsonb then
      raise exception 'VALIDATION_ERROR: 顶层事项不应携带父项版本';
    end if;
    task_id:=timegenie.calendar_stable_uuid(
      'direct-actual-task:'||p_operation_id::text||':'||(draft->>'clientTaskId')
    );
    select coalesce(max(task.sort_order),0)+10 into sort_value from timegenie.tasks task
    where task.workspace_id=p_workspace_id and task.subject_id=subject_id
      and task.parent_id is not distinct from nullif(draft->>'parentId','')::uuid
      and task.day_instance_date is not distinct from day_instance_date and task.deleted_at is null;
    insert into timegenie.tasks(
      id,workspace_id,subject_id,parent_id,title,status,planned_date,source_type,source_ref,
      sort_order,created_at,updated_at,version,created_by_device_id,updated_by_device_id,
      day_instance_date,day_instance_kind
    ) values(
      task_id,p_workspace_id,subject_id,nullif(draft->>'parentId','')::uuid,task_title,'open',
      case when day_instance_date is null then work_date_value else null end,'manual',
      'actual_adjustment_quick_create',sort_value,now_value,now_value,1,p_device_id,p_device_id,
      day_instance_date,case when day_instance_date is null then null else 'member' end
    );
    created_tasks:=created_tasks||jsonb_build_array(jsonb_build_object(
      'clientTaskId',draft->>'clientTaskId','taskId',task_id,
      'task',timegenie.direct_cloud_task_project(p_workspace_id,task_id)
    ));
    if (draft->>'targetMinutes')::integer>0 then
      adjustment_id:=timegenie.calendar_stable_uuid('direct-actual-adjustment:'||p_operation_id::text||':'||task_id::text);
      insert into timegenie.actual_time_adjustments(
        id,workspace_id,subject_id,task_id,work_date,delta_minutes,baseline_minutes,target_minutes,
        note,source_type,batch_id,created_at,updated_at,version,created_by_device_id,updated_by_device_id
      ) values(
        adjustment_id,p_workspace_id,subject_id,task_id,work_date_value,
        (draft->>'targetMinutes')::integer,0,(draft->>'targetMinutes')::integer,note_value,
        'quick_actual_adjustment',batch_id,now_value,now_value,1,p_device_id,p_device_id
      );
      adjustments:=adjustments||jsonb_build_array(
        timegenie.direct_cloud_actual_adjustment_project(p_workspace_id,adjustment_id)
      );
    end if;
  end loop;

  result_value:=timegenie.direct_cloud_mutation_response(
    p_workspace_id,p_device_id,p_operation_id,jsonb_build_object(
      'batchId',batch_id,'createdAdjustmentCount',jsonb_array_length(adjustments),
      'createdTaskCount',jsonb_array_length(created_tasks),
      'createdTasks',created_tasks,'adjustments',adjustments
    ),array['time','tasks','planning'],p_expected_epoch
  );
  insert into timegenie.actual_adjustment_batches(
    workspace_id,client_request_id,batch_id,request_hash,result_json,created_at
  ) values(p_workspace_id,p_operation_id,batch_id,request_hash,result_value,now_value);
  perform timegenie.record_direct_cloud_operation(
    p_workspace_id,p_operation_id,p_device_id,'direct_actual_adjustment_batch_apply',request_value,result_value
  );
  return result_value;
end $$;

create or replace function timegenie.direct_cloud_actual_adjustment_delete(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb;
declare adjustment_id uuid; expected_version bigint; adjustment_row timegenie.actual_time_adjustments%rowtype;
declare current_date_value date;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_actual_adjustment_delete',request_value);
  if cached is not null then return cached; end if;
  adjustment_id:=nullif(p_payload->>'adjustmentId','')::uuid;
  expected_version:=nullif(p_payload->>'expectedVersion','')::bigint;
  if adjustment_id is null or expected_version is null then
    raise exception 'VALIDATION_ERROR: 删除实际工时调整缺少标识或版本';
  end if;
  select adjustment.* into adjustment_row from timegenie.actual_time_adjustments adjustment
  where adjustment.workspace_id=p_workspace_id and adjustment.id=adjustment_id for update;
  if not found then raise exception 'NOT_FOUND: 实际调整记录不存在'; end if;
  select (now() at time zone workspace.timezone)::date into current_date_value
  from timegenie.workspaces workspace where workspace.id=p_workspace_id;
  if adjustment_row.work_date<>current_date_value then
    raise exception 'HISTORICAL_RECORD_READ_ONLY: % 的历史记录不可修改（当前工作日期为 %）',
      adjustment_row.work_date,current_date_value;
  end if;
  if adjustment_row.deleted_at is null then
    if adjustment_row.version<>expected_version then
      raise exception 'VERSION_CONFLICT: 实际调整记录已变化';
    end if;
    update timegenie.actual_time_adjustments adjustment set
      deleted_at=now(),updated_at=now(),updated_by_device_id=p_device_id,version=adjustment.version+1
    where adjustment.workspace_id=p_workspace_id and adjustment.id=adjustment_id;
  end if;
  result_value:=timegenie.direct_cloud_mutation_response(
    p_workspace_id,p_device_id,p_operation_id,
    jsonb_build_object('adjustment',timegenie.direct_cloud_actual_adjustment_project(p_workspace_id,adjustment_id)),
    array['time','tasks'],p_expected_epoch
  );
  perform timegenie.record_direct_cloud_operation(
    p_workspace_id,p_operation_id,p_device_id,'direct_actual_adjustment_delete',request_value,result_value
  );
  return result_value;
end $$;

revoke all on function timegenie.direct_cloud_actual_adjustment_project(uuid,uuid) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_actual_minutes(uuid,uuid,date) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_actual_adjustment_snapshot(uuid,uuid,uuid,date,boolean,bigint) from public,anon;
revoke all on function timegenie.direct_cloud_manual_time_create(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_actual_adjustment_batch_apply(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_actual_adjustment_delete(uuid,uuid,uuid,bigint,jsonb) from public,anon;
grant execute on function timegenie.direct_cloud_actual_adjustment_snapshot(uuid,uuid,uuid,date,boolean,bigint) to authenticated;
grant execute on function timegenie.direct_cloud_manual_time_create(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_actual_adjustment_batch_apply(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_actual_adjustment_delete(uuid,uuid,uuid,bigint,jsonb) to authenticated;

commit;

notify pgrst,'reload schema';
