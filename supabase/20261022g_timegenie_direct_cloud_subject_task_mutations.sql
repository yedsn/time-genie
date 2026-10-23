-- Direct-v2 subject and task mutations. Every public mutation is an
-- authenticated, device-authorized, epoch-checked, idempotent transaction.

create or replace function timegenie.direct_cloud_subject_record(
  p_workspace_id uuid,p_subject_id uuid,p_include_deleted boolean default false
) returns jsonb language sql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
  select jsonb_build_object(
    'id',subject.id,'name',subject.name,'sortOrder',subject.sort_order,
    'openTaskCount',(select count(*) from timegenie.tasks task
      where task.workspace_id=p_workspace_id and task.subject_id=subject.id
        and task.status='open' and task.deleted_at is null),
    'version',subject.version,'deletedAt',subject.deleted_at
  )
  from timegenie.subjects subject
  where subject.workspace_id=p_workspace_id and subject.id=p_subject_id
    and (p_include_deleted or subject.deleted_at is null);
$$;

create or replace function timegenie.direct_cloud_task_record(
  p_workspace_id uuid,p_task_id uuid,p_include_deleted boolean default false
) returns jsonb language sql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
  select jsonb_build_object(
    'id',task.id,'subjectId',task.subject_id,'parentId',task.parent_id,
    'dayInstanceDate',task.day_instance_date,'dayInstanceKind',task.day_instance_kind,
    'dayInstanceSourceTaskId',task.day_instance_source_task_id,
    'title',task.title,'status',task.status,'plannedDate',task.planned_date,
    'estimateMinutes',task.estimate_minutes,'note',task.note,
    'projectName',task.project_name,'solutionName',task.solution_name,
    'sourceType',task.source_type,'sortOrder',task.sort_order,
    'completedAt',case when task.completed_at is null then null else (extract(epoch from task.completed_at)*1000)::bigint end,
    'version',task.version,'deletedAt',task.deleted_at
  )
  from timegenie.tasks task
  where task.workspace_id=p_workspace_id and task.id=p_task_id
    and (p_include_deleted or task.deleted_at is null);
$$;

create or replace function timegenie.direct_cloud_mutation_response(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_data jsonb,
  p_domains text[],p_expected_epoch bigint
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
begin
  return jsonb_build_object(
    'operationId',p_operation_id,
    'metadata',timegenie.direct_cloud_query_metadata(
      p_workspace_id,p_device_id,p_expected_epoch,p_domains
    ),
    'data',coalesce(p_data,'{}'::jsonb)
  );
end $$;

create or replace function timegenie.direct_cloud_task_parent_assert(
  p_workspace_id uuid,p_subject_id uuid,p_parent_id uuid,p_day_instance_date date,
  p_moving_task_id uuid default null
) returns void language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare parent_row timegenie.tasks%rowtype; direct_minutes bigint;
begin
  select parent.* into parent_row from timegenie.tasks parent
  where parent.workspace_id=p_workspace_id and parent.id=p_parent_id
  for update;
  if not found then raise exception 'NOT_FOUND: 父事项不存在'; end if;
  if parent_row.deleted_at is not null then raise exception 'ENTITY_DELETED: 父事项已删除'; end if;
  if parent_row.subject_id<>p_subject_id then raise exception 'VALIDATION_ERROR: 父事项必须属于同一主体'; end if;
  if parent_row.parent_id is not null then raise exception 'TASK_DEPTH_LIMIT: 待办事项最多支持两级'; end if;
  if parent_row.day_instance_date is distinct from p_day_instance_date then
    raise exception 'VALIDATION_ERROR: 父事项必须属于同一今日实例日期';
  end if;
  if p_moving_task_id is not null then
    if p_parent_id=p_moving_task_id or exists(
      with recursive descendants as (
        select child.id from timegenie.tasks child
        where child.workspace_id=p_workspace_id and child.parent_id=p_moving_task_id and child.deleted_at is null
        union all
        select child.id from timegenie.tasks child join descendants parent on child.parent_id=parent.id
        where child.workspace_id=p_workspace_id and child.deleted_at is null
      ) select 1 from descendants where id=p_parent_id
    ) then raise exception 'TASK_TREE_CYCLE: 事项层级不能形成循环'; end if;
  end if;
  select coalesce((select sum(allocation.minutes)::bigint
    from timegenie.time_allocations allocation
    join timegenie.time_entries entry on entry.id=allocation.entry_id
    where allocation.workspace_id=p_workspace_id and allocation.task_id=p_parent_id
      and entry.deleted_at is null),0)
    +coalesce((select sum(adjustment.delta_minutes)::bigint
      from timegenie.actual_time_adjustments adjustment
      where adjustment.workspace_id=p_workspace_id and adjustment.task_id=p_parent_id
        and adjustment.deleted_at is null),0)
    into direct_minutes;
  if direct_minutes>0 then raise exception 'TASK_HAS_DIRECT_TIME: 有直接工时的事项不能成为父事项'; end if;
  if exists(select 1 from timegenie.task_recurrence_rules rule
    where rule.workspace_id=p_workspace_id and rule.task_id=p_parent_id and rule.effective_end is null) then
    raise exception 'TASK_RECURRING: 重复事项新增子项前必须先关闭重复';
  end if;
end $$;

-- A restore is a first-class history action instead of an ordinary update.
create or replace function timegenie.operation_history_replace_item(
  p_workspace_id uuid,p_operation_id uuid,p_entity_type text,p_entity_id text,p_before jsonb,p_after jsonb
) returns void language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare log_id uuid; safe_before jsonb; safe_after jsonb; label_value text; action_value text;
begin
  select id into log_id from timegenie.cloud_operation_logs where workspace_id=p_workspace_id and operation_id=p_operation_id;
  if log_id is null then return; end if;
  safe_before:=timegenie.operation_history_safe_json(p_entity_type,p_before);
  safe_after:=timegenie.operation_history_safe_json(p_entity_type,p_after);
  label_value:=timegenie.operation_history_label(p_entity_type,coalesce(safe_after,safe_before),p_entity_id);
  action_value:=case
    when p_before is null then 'create'
    when p_after is null or p_after->>'deleted_at' is not null then 'delete'
    when p_before->>'deleted_at' is not null and p_after->>'deleted_at' is null then 'restore'
    else 'update' end;
  delete from timegenie.cloud_operation_log_items where operation_log_id=log_id;
  insert into timegenie.cloud_operation_log_items(operation_log_id,workspace_id,entity_type,entity_id,entity_label_snapshot,action,before_json,after_json,changed_fields,search_text)
  values(log_id,p_workspace_id,p_entity_type,p_entity_id,label_value,action_value,safe_before,safe_after,timegenie.operation_history_changed_fields(safe_before,safe_after),concat_ws(' ',p_entity_type,p_entity_id,label_value));
  update timegenie.cloud_operation_logs set affected_entity_count=1 where id=log_id;
end $$;

create or replace function timegenie.direct_cloud_subject_create(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; after_value jsonb;
declare subject_id uuid; subject_name text; subject_sort integer;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_subject_create',request_value);
  if cached is not null then return cached; end if;
  subject_id:=nullif(p_payload->>'id','')::uuid;
  subject_name:=btrim(coalesce(p_payload->>'name',''));
  if subject_id is null or subject_name='' then raise exception 'VALIDATION_ERROR: 主体标识和名称不能为空'; end if;
  if exists(select 1 from timegenie.subjects where workspace_id=p_workspace_id and id=subject_id) then
    raise exception 'VERSION_CONFLICT: 主体标识已存在';
  end if;
  if exists(select 1 from timegenie.subjects where workspace_id=p_workspace_id and deleted_at is null and lower(name)=lower(subject_name)) then
    raise exception 'VALIDATION_ERROR: 主体名称已存在';
  end if;
  select coalesce(max(sort_order),0)+10 into subject_sort from timegenie.subjects
  where workspace_id=p_workspace_id and deleted_at is null;
  insert into timegenie.subjects(id,workspace_id,name,sort_order,created_by_device_id,updated_by_device_id)
  values(subject_id,p_workspace_id,subject_name,subject_sort,p_device_id,p_device_id);
  insert into timegenie.subject_work_schedules(workspace_id,subject_id,weekday,is_workday)
  select p_workspace_id,subject_id,weekday.value,weekday.value<5 from generate_series(0,6) weekday(value);
  insert into timegenie.subject_work_schedule_segments(id,workspace_id,schedule_id,sequence_no,start_minute,end_minute)
  select gen_random_uuid(),p_workspace_id,schedule.id,segment.sequence_no,segment.start_minute,segment.end_minute
  from timegenie.subject_work_schedules schedule
  cross join (values(1,600,720),(2,810,1050)) segment(sequence_no,start_minute,end_minute)
  where schedule.workspace_id=p_workspace_id and schedule.subject_id=subject_id and schedule.weekday<5;
  after_value:=(select to_jsonb(subject) from timegenie.subjects subject where subject.id=subject_id);
  result_value:=timegenie.direct_cloud_mutation_response(p_workspace_id,p_device_id,p_operation_id,
    jsonb_build_object('subject',timegenie.direct_cloud_subject_record(p_workspace_id,subject_id,true),'alreadyExists',false),
    array['subjects','calendar'],p_expected_epoch);
  perform timegenie.record_direct_cloud_operation(p_workspace_id,p_operation_id,p_device_id,'direct_subject_create',request_value,result_value);
  perform timegenie.operation_history_replace_item(p_workspace_id,p_operation_id,'subject',subject_id::text,null,after_value);
  return result_value;
end $$;

create or replace function timegenie.direct_cloud_subject_update(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; before_value jsonb; after_value jsonb;
declare subject_id uuid; expected_version bigint; subject_name text; subject_row timegenie.subjects%rowtype;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_subject_update',request_value);
  if cached is not null then return cached; end if;
  subject_id:=nullif(p_payload->>'id','')::uuid; expected_version:=nullif(p_payload->>'expectedVersion','')::bigint;
  subject_name:=btrim(coalesce(p_payload->>'name',''));
  if subject_id is null or expected_version is null or subject_name='' then raise exception 'VALIDATION_ERROR: 主体更新参数不完整'; end if;
  select subject.* into subject_row from timegenie.subjects subject where subject.workspace_id=p_workspace_id and subject.id=subject_id for update;
  if not found then raise exception 'NOT_FOUND: 主体不存在'; end if;
  if subject_row.deleted_at is not null then raise exception 'ENTITY_DELETED: 主体已归档'; end if;
  if subject_row.version<>expected_version then raise exception 'VERSION_CONFLICT: expected %, current %',expected_version,subject_row.version; end if;
  if exists(select 1 from timegenie.subjects where workspace_id=p_workspace_id and id<>subject_id and deleted_at is null and lower(name)=lower(subject_name)) then
    raise exception 'VALIDATION_ERROR: 主体名称已存在';
  end if;
  before_value:=to_jsonb(subject_row);
  update timegenie.subjects set name=subject_name,updated_at=now(),updated_by_device_id=p_device_id,version=version+1 where id=subject_id;
  after_value:=(select to_jsonb(subject) from timegenie.subjects subject where subject.id=subject_id);
  result_value:=timegenie.direct_cloud_mutation_response(p_workspace_id,p_device_id,p_operation_id,
    jsonb_build_object('subject',timegenie.direct_cloud_subject_record(p_workspace_id,subject_id,true)),array['subjects'],p_expected_epoch);
  perform timegenie.record_direct_cloud_operation(p_workspace_id,p_operation_id,p_device_id,'direct_subject_update',request_value,result_value);
  perform timegenie.operation_history_replace_item(p_workspace_id,p_operation_id,'subject',subject_id::text,before_value,after_value);
  return result_value;
end $$;

create or replace function timegenie.direct_cloud_subject_reorder(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; before_value jsonb; after_value jsonb;
declare subject_id uuid; expected_version bigint; previous_id uuid; next_id uuid;
declare previous_sort integer; next_sort integer; target_sort integer; subject_row timegenie.subjects%rowtype;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':subject-order',0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_subject_reorder',request_value);
  if cached is not null then return cached; end if;
  subject_id:=nullif(p_payload->>'id','')::uuid; expected_version:=nullif(p_payload->>'expectedVersion','')::bigint;
  previous_id:=nullif(p_payload->>'previousSubjectId','')::uuid; next_id:=nullif(p_payload->>'nextSubjectId','')::uuid;
  if subject_id is null or expected_version is null then raise exception 'VALIDATION_ERROR: 主体排序参数不完整'; end if;
  if previous_id=subject_id or next_id=subject_id or (previous_id is not null and previous_id=next_id) then raise exception 'VALIDATION_ERROR: 主体排序锚点无效'; end if;
  select subject.* into subject_row from timegenie.subjects subject where subject.workspace_id=p_workspace_id and subject.id=subject_id for update;
  if not found then raise exception 'NOT_FOUND: 主体不存在'; end if;
  if subject_row.deleted_at is not null then raise exception 'ENTITY_DELETED: 主体已归档'; end if;
  if subject_row.version<>expected_version then raise exception 'VERSION_CONFLICT: expected %, current %',expected_version,subject_row.version; end if;
  if previous_id is not null then select sort_order into previous_sort from timegenie.subjects where workspace_id=p_workspace_id and id=previous_id and deleted_at is null; if not found then raise exception 'VALIDATION_ERROR: 前序主体不存在'; end if; end if;
  if next_id is not null then select sort_order into next_sort from timegenie.subjects where workspace_id=p_workspace_id and id=next_id and deleted_at is null; if not found then raise exception 'VALIDATION_ERROR: 后序主体不存在'; end if; end if;
  if previous_sort is not null and next_sort is not null and previous_sort>=next_sort then raise exception 'VALIDATION_ERROR: 主体排序锚点顺序无效'; end if;
  if previous_sort is not null and next_sort is not null and next_sort-previous_sort<=1 then
    with ordered as (select id,row_number() over(order by sort_order,created_at,id) as position from timegenie.subjects where workspace_id=p_workspace_id and deleted_at is null and id<>subject_id)
    update timegenie.subjects subject set sort_order=ordered.position*10,updated_at=now(),version=subject.version+1 from ordered where subject.id=ordered.id;
    if previous_id is not null then select sort_order into previous_sort from timegenie.subjects where id=previous_id; end if;
    if next_id is not null then select sort_order into next_sort from timegenie.subjects where id=next_id; end if;
  end if;
  target_sort:=case when previous_sort is not null and next_sort is not null then previous_sort+(next_sort-previous_sort)/2 when previous_sort is not null then previous_sort+10 when next_sort is not null then next_sort-10 else coalesce((select max(sort_order)+10 from timegenie.subjects where workspace_id=p_workspace_id and deleted_at is null and id<>subject_id),10) end;
  before_value:=to_jsonb(subject_row);
  update timegenie.subjects set sort_order=target_sort,updated_at=now(),updated_by_device_id=p_device_id,version=version+1 where id=subject_id;
  after_value:=(select to_jsonb(subject) from timegenie.subjects subject where subject.id=subject_id);
  result_value:=timegenie.direct_cloud_mutation_response(p_workspace_id,p_device_id,p_operation_id,jsonb_build_object('subject',timegenie.direct_cloud_subject_record(p_workspace_id,subject_id,true)),array['subjects'],p_expected_epoch);
  perform timegenie.record_direct_cloud_operation(p_workspace_id,p_operation_id,p_device_id,'direct_subject_reorder',request_value,result_value);
  perform timegenie.operation_history_replace_item(p_workspace_id,p_operation_id,'subject',subject_id::text,before_value,after_value);
  return result_value;
end $$;

create or replace function timegenie.direct_cloud_subject_archive(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; before_value jsonb; after_value jsonb;
declare subject_id uuid; expected_version bigint; subject_row timegenie.subjects%rowtype;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_subject_archive',request_value); if cached is not null then return cached; end if;
  subject_id:=nullif(p_payload->>'id','')::uuid; expected_version:=nullif(p_payload->>'expectedVersion','')::bigint;
  select subject.* into subject_row from timegenie.subjects subject where subject.workspace_id=p_workspace_id and subject.id=subject_id for update;
  if not found then raise exception 'NOT_FOUND: 主体不存在'; end if;
  if subject_row.deleted_at is not null then raise exception 'ENTITY_DELETED: 主体已归档'; end if;
  if subject_row.version<>expected_version then raise exception 'VERSION_CONFLICT: expected %, current %',expected_version,subject_row.version; end if;
  before_value:=to_jsonb(subject_row);
  update timegenie.subjects set deleted_at=now(),updated_at=now(),updated_by_device_id=p_device_id,version=version+1 where id=subject_id;
  after_value:=(select to_jsonb(subject) from timegenie.subjects subject where subject.id=subject_id);
  result_value:=timegenie.direct_cloud_mutation_response(p_workspace_id,p_device_id,p_operation_id,jsonb_build_object('subject',timegenie.direct_cloud_subject_record(p_workspace_id,subject_id,true)),array['subjects','tasks'],p_expected_epoch);
  perform timegenie.record_direct_cloud_operation(p_workspace_id,p_operation_id,p_device_id,'direct_subject_archive',request_value,result_value);
  perform timegenie.operation_history_replace_item(p_workspace_id,p_operation_id,'subject',subject_id::text,before_value,after_value);
  return result_value;
end $$;

create or replace function timegenie.direct_cloud_subject_restore(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; before_value jsonb; after_value jsonb;
declare subject_id uuid; expected_version bigint; subject_row timegenie.subjects%rowtype;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_subject_restore',request_value); if cached is not null then return cached; end if;
  subject_id:=nullif(p_payload->>'id','')::uuid; expected_version:=nullif(p_payload->>'expectedVersion','')::bigint;
  select subject.* into subject_row from timegenie.subjects subject where subject.workspace_id=p_workspace_id and subject.id=subject_id for update;
  if not found then raise exception 'NOT_FOUND: 主体不存在'; end if;
  if subject_row.deleted_at is null then raise exception 'VERSION_CONFLICT: 主体当前未归档'; end if;
  if subject_row.version<>expected_version then raise exception 'VERSION_CONFLICT: expected %, current %',expected_version,subject_row.version; end if;
  if exists(select 1 from timegenie.subjects where workspace_id=p_workspace_id and id<>subject_id and deleted_at is null and lower(name)=lower(subject_row.name)) then raise exception 'VALIDATION_ERROR: 同名主体已存在，无法恢复'; end if;
  before_value:=to_jsonb(subject_row);
  update timegenie.subjects set deleted_at=null,updated_at=now(),updated_by_device_id=p_device_id,version=version+1 where id=subject_id;
  after_value:=(select to_jsonb(subject) from timegenie.subjects subject where subject.id=subject_id);
  result_value:=timegenie.direct_cloud_mutation_response(p_workspace_id,p_device_id,p_operation_id,jsonb_build_object('subject',timegenie.direct_cloud_subject_record(p_workspace_id,subject_id,true)),array['subjects','tasks'],p_expected_epoch);
  perform timegenie.record_direct_cloud_operation(p_workspace_id,p_operation_id,p_device_id,'direct_subject_restore',request_value,result_value);
  perform timegenie.operation_history_replace_item(p_workspace_id,p_operation_id,'subject',subject_id::text,before_value,after_value);
  return result_value;
end $$;

create or replace function timegenie.direct_cloud_task_create(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; after_value jsonb;
declare task_id uuid; subject_id uuid; parent_id uuid; day_date date; parent_date date;
declare task_title text; estimate_value integer; sort_value integer; source_value text;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_task_create',request_value); if cached is not null then return cached; end if;
  task_id:=nullif(p_payload->>'id','')::uuid; subject_id:=nullif(p_payload->>'subjectId','')::uuid; parent_id:=nullif(p_payload->>'parentId','')::uuid;
  task_title:=btrim(coalesce(p_payload->>'title','')); day_date:=nullif(p_payload->>'dayInstanceDate','')::date;
  estimate_value:=nullif(p_payload->>'estimateMinutes','')::integer; source_value:=coalesce(nullif(p_payload->>'sourceType',''),'manual');
  if task_id is null or subject_id is null or task_title='' then raise exception 'VALIDATION_ERROR: 事项标识、主体和名称不能为空'; end if;
  if estimate_value is not null and estimate_value<=0 then raise exception 'VALIDATION_ERROR: 预计用时必须大于 0 分钟'; end if;
  if source_value not in('manual','obsidian_import','timer_quick_create','migration') then raise exception 'VALIDATION_ERROR: 事项来源无效'; end if;
  if exists(select 1 from timegenie.tasks where workspace_id=p_workspace_id and id=task_id) then raise exception 'VERSION_CONFLICT: 事项标识已存在'; end if;
  perform 1 from timegenie.subjects where workspace_id=p_workspace_id and id=subject_id and deleted_at is null for update;
  if not found then raise exception 'NOT_FOUND: 主体不存在或已归档'; end if;
  if parent_id is not null then
    select task.day_instance_date into parent_date from timegenie.tasks task where task.workspace_id=p_workspace_id and task.id=parent_id;
    if day_date is null then day_date:=parent_date; elsif day_date is distinct from parent_date then raise exception 'VALIDATION_ERROR: 今日子事项必须属于同一实例日期'; end if;
    perform timegenie.direct_cloud_task_parent_assert(p_workspace_id,subject_id,parent_id,day_date,null);
  end if;
  if p_payload ? 'previousSiblingId' or p_payload ? 'nextSiblingId' then
    if nullif(p_payload->>'previousSiblingId','') is not null then select anchor.sort_order into sort_value from timegenie.tasks anchor where anchor.workspace_id=p_workspace_id and anchor.id=(p_payload->>'previousSiblingId')::uuid and anchor.subject_id=subject_id and anchor.parent_id is not distinct from parent_id and anchor.day_instance_date is not distinct from day_date and anchor.deleted_at is null; if not found then raise exception 'VALIDATION_ERROR: 前序事项不属于目标层级'; end if; sort_value:=sort_value+10;
    elsif nullif(p_payload->>'nextSiblingId','') is not null then select anchor.sort_order-10 into sort_value from timegenie.tasks anchor where anchor.workspace_id=p_workspace_id and anchor.id=(p_payload->>'nextSiblingId')::uuid and anchor.subject_id=subject_id and anchor.parent_id is not distinct from parent_id and anchor.day_instance_date is not distinct from day_date and anchor.deleted_at is null; if not found then raise exception 'VALIDATION_ERROR: 后序事项不属于目标层级'; end if;
    end if;
  end if;
  if sort_value is null then select coalesce(max(task.sort_order),0)+10 into sort_value from timegenie.tasks task where task.workspace_id=p_workspace_id and task.subject_id=subject_id and task.parent_id is not distinct from parent_id and task.day_instance_date is not distinct from day_date and task.deleted_at is null; end if;
  insert into timegenie.tasks(id,workspace_id,subject_id,parent_id,title,status,planned_date,estimate_minutes,note,project_name,solution_name,source_type,sort_order,created_by_device_id,updated_by_device_id,day_instance_date,day_instance_kind)
  values(task_id,p_workspace_id,subject_id,parent_id,task_title,'open',nullif(p_payload->>'plannedDate','')::date,estimate_value,nullif(btrim(p_payload->>'note'),''),nullif(btrim(p_payload->>'projectName'),''),nullif(btrim(p_payload->>'solutionName'),''),source_value,sort_value,p_device_id,p_device_id,day_date,case when day_date is null then null else 'member' end);
  after_value:=(select to_jsonb(task) from timegenie.tasks task where task.id=task_id);
  result_value:=timegenie.direct_cloud_mutation_response(p_workspace_id,p_device_id,p_operation_id,jsonb_build_object('task',timegenie.direct_cloud_task_project(p_workspace_id,task_id)),array['tasks'],p_expected_epoch);
  perform timegenie.record_direct_cloud_operation(p_workspace_id,p_operation_id,p_device_id,'direct_task_create',request_value,result_value);
  perform timegenie.operation_history_replace_item(p_workspace_id,p_operation_id,'task',task_id::text,null,after_value);
  return result_value;
end $$;

create or replace function timegenie.direct_cloud_task_update(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; before_value jsonb; after_value jsonb;
declare task_id uuid; expected_version bigint; task_row timegenie.tasks%rowtype; title_value text; estimate_value integer; parent_value uuid;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_task_update',request_value); if cached is not null then return cached; end if;
  task_id:=nullif(p_payload->>'id','')::uuid; expected_version:=nullif(p_payload->>'expectedVersion','')::bigint;
  select task.* into task_row from timegenie.tasks task where task.workspace_id=p_workspace_id and task.id=task_id for update;
  if not found then raise exception 'NOT_FOUND: 事项不存在'; end if;
  if task_row.deleted_at is not null then raise exception 'ENTITY_DELETED: 事项已删除'; end if;
  if task_row.version<>expected_version then raise exception 'VERSION_CONFLICT: expected %, current %',expected_version,task_row.version; end if;
  if p_payload ? 'title' then title_value:=btrim(coalesce(p_payload->>'title','')); if title_value='' then raise exception 'VALIDATION_ERROR: 事项名称不能为空'; end if; end if;
  if p_payload ? 'estimateMinutes' and p_payload->'estimateMinutes'<>'null'::jsonb then estimate_value:=(p_payload->>'estimateMinutes')::integer; if estimate_value<=0 then raise exception 'VALIDATION_ERROR: 预计用时必须大于 0 分钟'; end if; end if;
  if p_payload ? 'parentId' then
    parent_value:=nullif(p_payload->>'parentId','')::uuid;
    if parent_value is not null then
      if exists(select 1 from timegenie.tasks child where child.workspace_id=p_workspace_id and child.parent_id=task_id and child.deleted_at is null) then raise exception 'TASK_DEPTH_LIMIT: 包含子事项的事项不能变为二级'; end if;
      perform timegenie.direct_cloud_task_parent_assert(p_workspace_id,task_row.subject_id,parent_value,task_row.day_instance_date,task_id);
    end if;
  end if;
  before_value:=to_jsonb(task_row);
  update timegenie.tasks set
    title=case when p_payload ? 'title' then title_value else title end,
    planned_date=case when p_payload ? 'plannedDate' then nullif(p_payload->>'plannedDate','')::date else planned_date end,
    estimate_minutes=case when p_payload ? 'estimateMinutes' then estimate_value else estimate_minutes end,
    note=case when p_payload ? 'note' then nullif(btrim(p_payload->>'note'),'') else note end,
    project_name=case when p_payload ? 'projectName' then nullif(btrim(p_payload->>'projectName'),'') else project_name end,
    solution_name=case when p_payload ? 'solutionName' then nullif(btrim(p_payload->>'solutionName'),'') else solution_name end,
    parent_id=case when p_payload ? 'parentId' then parent_value else parent_id end,
    updated_at=now(),updated_by_device_id=p_device_id,version=version+1 where id=task_id;
  after_value:=(select to_jsonb(task) from timegenie.tasks task where task.id=task_id);
  result_value:=timegenie.direct_cloud_mutation_response(p_workspace_id,p_device_id,p_operation_id,jsonb_build_object('task',timegenie.direct_cloud_task_project(p_workspace_id,task_id)),array['tasks'],p_expected_epoch);
  perform timegenie.record_direct_cloud_operation(p_workspace_id,p_operation_id,p_device_id,'direct_task_update',request_value,result_value);
  perform timegenie.operation_history_replace_item(p_workspace_id,p_operation_id,'task',task_id::text,before_value,after_value);
  return result_value;
end $$;

create or replace function timegenie.direct_cloud_task_move(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; before_value jsonb; after_value jsonb;
declare task_id uuid; expected_version bigint; parent_id uuid; previous_id uuid; next_id uuid;
declare previous_sort integer; next_sort integer; target_sort integer; task_row timegenie.tasks%rowtype;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':task-order',0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_task_move',request_value); if cached is not null then return cached; end if;
  task_id:=nullif(p_payload->>'id','')::uuid; expected_version:=nullif(p_payload->>'expectedVersion','')::bigint;
  parent_id:=nullif(p_payload->>'parentId','')::uuid; previous_id:=nullif(p_payload->>'previousSiblingId','')::uuid; next_id:=nullif(p_payload->>'nextSiblingId','')::uuid;
  if task_id is null or expected_version is null then raise exception 'VALIDATION_ERROR: 事项移动参数不完整'; end if;
  if previous_id=task_id or next_id=task_id or (previous_id is not null and previous_id=next_id) then raise exception 'VALIDATION_ERROR: 排序锚点无效'; end if;
  select task.* into task_row from timegenie.tasks task where task.workspace_id=p_workspace_id and task.id=task_id for update;
  if not found then raise exception 'NOT_FOUND: 事项不存在'; end if;
  if task_row.deleted_at is not null then raise exception 'ENTITY_DELETED: 事项已删除'; end if;
  if task_row.version<>expected_version then raise exception 'VERSION_CONFLICT: expected %, current %',expected_version,task_row.version; end if;
  if parent_id is not null then
    if exists(select 1 from timegenie.tasks where workspace_id=p_workspace_id and parent_id=task_id and deleted_at is null) then raise exception 'TASK_DEPTH_LIMIT: 包含子事项的事项不能变为二级'; end if;
    perform timegenie.direct_cloud_task_parent_assert(p_workspace_id,task_row.subject_id,parent_id,task_row.day_instance_date,task_id);
  end if;
  if p_payload ? 'sortOrder' and previous_id is null and next_id is null then
    target_sort:=nullif(p_payload->>'sortOrder','')::integer;
  end if;
  if previous_id is not null then select anchor.sort_order into previous_sort from timegenie.tasks anchor where anchor.workspace_id=p_workspace_id and anchor.id=previous_id and anchor.subject_id=task_row.subject_id and anchor.parent_id is not distinct from parent_id and anchor.day_instance_date is not distinct from task_row.day_instance_date and anchor.deleted_at is null; if not found then raise exception 'VALIDATION_ERROR: 前序事项不属于目标层级'; end if; end if;
  if next_id is not null then select anchor.sort_order into next_sort from timegenie.tasks anchor where anchor.workspace_id=p_workspace_id and anchor.id=next_id and anchor.subject_id=task_row.subject_id and anchor.parent_id is not distinct from parent_id and anchor.day_instance_date is not distinct from task_row.day_instance_date and anchor.deleted_at is null; if not found then raise exception 'VALIDATION_ERROR: 后序事项不属于目标层级'; end if; end if;
  if previous_sort is not null and next_sort is not null and previous_sort>=next_sort then raise exception 'VALIDATION_ERROR: 排序锚点顺序无效'; end if;
  if previous_sort is not null and next_sort is not null and next_sort-previous_sort<=1 then
    with ordered as (select sibling.id,row_number() over(order by sibling.sort_order,sibling.created_at,sibling.id) as position from timegenie.tasks sibling where sibling.workspace_id=p_workspace_id and sibling.subject_id=task_row.subject_id and sibling.parent_id is not distinct from parent_id and sibling.day_instance_date is not distinct from task_row.day_instance_date and sibling.deleted_at is null and sibling.id<>task_id)
    update timegenie.tasks task set sort_order=ordered.position*10,updated_at=now(),version=task.version+1 from ordered where task.id=ordered.id;
    if previous_id is not null then select sort_order into previous_sort from timegenie.tasks where id=previous_id; end if;
    if next_id is not null then select sort_order into next_sort from timegenie.tasks where id=next_id; end if;
  end if;
  if target_sort is null then target_sort:=case when previous_sort is not null and next_sort is not null then previous_sort+(next_sort-previous_sort)/2 when previous_sort is not null then previous_sort+10 when next_sort is not null then next_sort-10 else coalesce((select max(sibling.sort_order)+10 from timegenie.tasks sibling where sibling.workspace_id=p_workspace_id and sibling.subject_id=task_row.subject_id and sibling.parent_id is not distinct from parent_id and sibling.day_instance_date is not distinct from task_row.day_instance_date and sibling.deleted_at is null and sibling.id<>task_id),10) end; end if;
  before_value:=to_jsonb(task_row);
  update timegenie.tasks set parent_id=parent_id,sort_order=target_sort,updated_at=now(),updated_by_device_id=p_device_id,version=version+1 where id=task_id;
  after_value:=(select to_jsonb(task) from timegenie.tasks task where task.id=task_id);
  result_value:=timegenie.direct_cloud_mutation_response(p_workspace_id,p_device_id,p_operation_id,jsonb_build_object('task',timegenie.direct_cloud_task_project(p_workspace_id,task_id)),array['tasks'],p_expected_epoch);
  perform timegenie.record_direct_cloud_operation(p_workspace_id,p_operation_id,p_device_id,'direct_task_move',request_value,result_value);
  perform timegenie.operation_history_replace_item(p_workspace_id,p_operation_id,'task',task_id::text,before_value,after_value);
  return result_value;
end $$;

create or replace function timegenie.direct_cloud_task_archive_subtree(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; before_value jsonb; after_value jsonb;
declare task_id uuid; expected_version bigint; task_row timegenie.tasks%rowtype; archive_time timestamptz:=now(); affected_ids jsonb;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_task_archive_subtree',request_value); if cached is not null then return cached; end if;
  task_id:=nullif(p_payload->>'id','')::uuid; expected_version:=nullif(p_payload->>'expectedVersion','')::bigint;
  select task.* into task_row from timegenie.tasks task where task.workspace_id=p_workspace_id and task.id=task_id for update;
  if not found then raise exception 'NOT_FOUND: 事项不存在'; end if;
  if task_row.deleted_at is not null then raise exception 'ENTITY_DELETED: 事项已删除'; end if;
  if task_row.version<>expected_version then raise exception 'VERSION_CONFLICT: expected %, current %',expected_version,task_row.version; end if;
  before_value:=to_jsonb(task_row);
  with recursive subtree as (
    select id from timegenie.tasks where workspace_id=p_workspace_id and id=task_id and deleted_at is null
    union all select child.id from timegenie.tasks child join subtree parent on child.parent_id=parent.id where child.workspace_id=p_workspace_id and child.deleted_at is null
  ), updated as (
    update timegenie.tasks task set deleted_at=archive_time,updated_at=archive_time,updated_by_device_id=p_device_id,version=task.version+1 where task.id in(select id from subtree) returning task.id
  ) select coalesce(jsonb_agg(id order by id),'[]'::jsonb) into affected_ids from updated;
  after_value:=(select to_jsonb(task) from timegenie.tasks task where task.id=task_id);
  result_value:=timegenie.direct_cloud_mutation_response(p_workspace_id,p_device_id,p_operation_id,jsonb_build_object('task',timegenie.direct_cloud_task_record(p_workspace_id,task_id,true),'affectedTaskIds',affected_ids),array['tasks','plans'],p_expected_epoch);
  perform timegenie.record_direct_cloud_operation(p_workspace_id,p_operation_id,p_device_id,'direct_task_archive_subtree',request_value,result_value);
  perform timegenie.operation_history_replace_item(p_workspace_id,p_operation_id,'task',task_id::text,before_value,after_value);
  return result_value;
end $$;

create or replace function timegenie.direct_cloud_task_restore_subtree(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; before_value jsonb; after_value jsonb;
declare task_id uuid; expected_version bigint; task_row timegenie.tasks%rowtype; affected_ids jsonb; archived_at timestamptz;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_task_restore_subtree',request_value); if cached is not null then return cached; end if;
  task_id:=nullif(p_payload->>'id','')::uuid; expected_version:=nullif(p_payload->>'expectedVersion','')::bigint;
  select task.* into task_row from timegenie.tasks task where task.workspace_id=p_workspace_id and task.id=task_id for update;
  if not found then raise exception 'NOT_FOUND: 事项不存在'; end if;
  if task_row.deleted_at is null then raise exception 'VERSION_CONFLICT: 事项当前未删除'; end if;
  if task_row.version<>expected_version then raise exception 'VERSION_CONFLICT: expected %, current %',expected_version,task_row.version; end if;
  if not exists(select 1 from timegenie.subjects where workspace_id=p_workspace_id and id=task_row.subject_id and deleted_at is null) then raise exception 'ENTITY_DELETED: 所属主体已归档'; end if;
  if task_row.parent_id is not null and not exists(select 1 from timegenie.tasks where workspace_id=p_workspace_id and id=task_row.parent_id and deleted_at is null) then raise exception 'ENTITY_DELETED: 父事项仍处于删除状态'; end if;
  before_value:=to_jsonb(task_row); archived_at:=task_row.deleted_at;
  update timegenie.tasks set deleted_at=null,updated_at=now(),updated_by_device_id=p_device_id,version=version+1 where id=task_id;
  with recursive subtree as (
    select child.id from timegenie.tasks child where child.workspace_id=p_workspace_id and child.parent_id=task_id and child.deleted_at=archived_at
    union all select child.id from timegenie.tasks child join subtree parent on child.parent_id=parent.id where child.workspace_id=p_workspace_id and child.deleted_at=archived_at
  ), updated as (
    update timegenie.tasks task set deleted_at=null,updated_at=now(),updated_by_device_id=p_device_id,version=task.version+1 where task.id in(select id from subtree) returning task.id
  ) select coalesce(jsonb_build_array(task_id)||coalesce(jsonb_agg(id order by id),'[]'::jsonb),jsonb_build_array(task_id)) into affected_ids from updated;
  after_value:=(select to_jsonb(task) from timegenie.tasks task where task.id=task_id);
  result_value:=timegenie.direct_cloud_mutation_response(p_workspace_id,p_device_id,p_operation_id,jsonb_build_object('task',timegenie.direct_cloud_task_project(p_workspace_id,task_id),'affectedTaskIds',affected_ids),array['tasks','plans'],p_expected_epoch);
  perform timegenie.record_direct_cloud_operation(p_workspace_id,p_operation_id,p_device_id,'direct_task_restore_subtree',request_value,result_value);
  perform timegenie.operation_history_replace_item(p_workspace_id,p_operation_id,'task',task_id::text,before_value,after_value);
  return result_value;
end $$;

-- Archived subjects are not part of page-level task lists.
create or replace function timegenie.direct_cloud_task_list(
  p_workspace_id uuid,p_device_id uuid,p_expected_epoch bigint default null,
  p_subject_id uuid default null,p_include_completed boolean default true,p_query text default null
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare data_value jsonb;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  if p_subject_id is not null and not exists(select 1 from timegenie.subjects where workspace_id=p_workspace_id and id=p_subject_id and deleted_at is null) then raise exception 'NOT_FOUND: 主体不存在'; end if;
  select jsonb_build_object('tasks',coalesce(jsonb_agg(projected order by subject_sort,tree_path),'[]'::jsonb)) into data_value from (
    with recursive tree as (
      select task.id,subject.sort_order as subject_sort,lpad((task.sort_order+1000000000)::text,11,'0')||'|'||task.created_at::text||'|'||task.id::text as tree_path
      from timegenie.tasks task join timegenie.subjects subject on subject.id=task.subject_id and subject.deleted_at is null
      where task.workspace_id=p_workspace_id and task.deleted_at is null and task.day_instance_date is null and (p_subject_id is null or task.subject_id=p_subject_id)
        and (task.parent_id is null or not exists(select 1 from timegenie.tasks parent where parent.workspace_id=p_workspace_id and parent.id=task.parent_id and parent.deleted_at is null and parent.day_instance_date is null))
      union all
      select child.id,parent.subject_sort,parent.tree_path||'/'||lpad((child.sort_order+1000000000)::text,11,'0')||'|'||child.created_at::text||'|'||child.id::text
      from timegenie.tasks child join tree parent on child.parent_id=parent.id where child.workspace_id=p_workspace_id and child.deleted_at is null and child.day_instance_date is null
    ) select tree.subject_sort,tree.tree_path,timegenie.direct_cloud_task_project(p_workspace_id,tree.id) as projected from tree join timegenie.tasks task on task.id=tree.id
    where (p_include_completed or task.status='open') and (nullif(btrim(p_query),'') is null or task.title ilike '%'||btrim(p_query)||'%')
  ) rows_value;
  return timegenie.direct_cloud_query_response(p_workspace_id,p_device_id,data_value,array['tasks'],p_expected_epoch);
end $$;

revoke all on function timegenie.direct_cloud_subject_record(uuid,uuid,boolean) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_task_record(uuid,uuid,boolean) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_mutation_response(uuid,uuid,uuid,jsonb,text[],bigint) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_task_parent_assert(uuid,uuid,uuid,date,uuid) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_subject_create(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_subject_update(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_subject_reorder(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_subject_archive(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_subject_restore(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_task_create(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_task_update(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_task_move(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_task_archive_subtree(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_task_restore_subtree(uuid,uuid,uuid,bigint,jsonb) from public,anon;
grant execute on function timegenie.direct_cloud_subject_create(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_subject_update(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_subject_reorder(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_subject_archive(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_subject_restore(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_task_create(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_task_update(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_task_move(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_task_archive_subtree(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_task_restore_subtree(uuid,uuid,uuid,bigint,jsonb) to authenticated;
