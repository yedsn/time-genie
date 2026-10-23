-- Direct-v2 recurrence mutations. Rule history, daily occurrence generation,
-- today-instance materialization and completion transitions share one cloud
-- transaction and never fall back to a local outbox.

create or replace function timegenie.direct_cloud_recurrence_rule_record(
  p_workspace_id uuid,p_rule_id uuid
) returns jsonb language sql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
  select jsonb_build_object(
    'id',rule.id,'taskId',rule.task_id,'frequency',rule.frequency,
    'weekdaysMask',rule.weekdays_mask,'effectiveStart',rule.effective_start,
    'effectiveEnd',rule.effective_end,
    'summary',case rule.frequency when 'daily' then '每天' when 'weekdays' then '工作日'
      else '每周' end,'version',rule.version
  ) from timegenie.task_recurrence_rules rule
  where rule.workspace_id=p_workspace_id and rule.id=p_rule_id;
$$;

create or replace function timegenie.direct_cloud_occurrence_record(
  p_workspace_id uuid,p_task_id uuid,p_occurrence_date date
) returns jsonb language sql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
  select jsonb_build_object(
    'taskId',occurrence.task_id,'occurrenceDate',occurrence.occurrence_date,
    'origin',occurrence.origin,'status',occurrence.status,
    'completedAt',case when occurrence.completed_at is null then null
      else (extract(epoch from occurrence.completed_at)*1000)::bigint end,
    'version',occurrence.version
  ) from timegenie.task_occurrences occurrence
  where occurrence.workspace_id=p_workspace_id and occurrence.task_id=p_task_id
    and occurrence.occurrence_date=p_occurrence_date;
$$;

create or replace function timegenie.direct_cloud_occurrence_ensure(
  p_workspace_id uuid,p_task_id uuid,p_occurrence_date date
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare task_row timegenie.tasks%rowtype; rule_row timegenie.task_recurrence_rules%rowtype;
declare origin_value text; rule_found boolean;
begin
  select task.* into task_row from timegenie.tasks task
  where task.workspace_id=p_workspace_id and task.id=p_task_id for update;
  if not found then raise exception 'NOT_FOUND: 事项不存在'; end if;
  if task_row.deleted_at is not null then raise exception 'ENTITY_DELETED: 事项已删除'; end if;
  if task_row.day_instance_date is not null then
    raise exception 'VALIDATION_ERROR: 今日实例不能作为重复模板';
  end if;
  select rule.* into rule_row from timegenie.task_recurrence_rules rule
  where rule.workspace_id=p_workspace_id and rule.task_id=p_task_id
    and rule.effective_start<=p_occurrence_date
    and (rule.effective_end is null or rule.effective_end>=p_occurrence_date)
  order by rule.effective_start desc,rule.id desc limit 1;
  rule_found:=found;
  if not rule_found and not exists(
    select 1 from timegenie.task_recurrence_rules rule
    where rule.workspace_id=p_workspace_id and rule.task_id=p_task_id
      and rule.effective_end is null
  ) then raise exception 'TASK_NOT_RECURRING: 事项未启用重复'; end if;
  origin_value:=case
    when rule_found and (
      rule_row.frequency='daily'
      or (rule_row.frequency='weekdays' and extract(isodow from p_occurrence_date)<6)
      or (rule_row.frequency='weekly'
        and (rule_row.weekdays_mask & (1 << (extract(isodow from p_occurrence_date)::integer-1)))<>0)
    ) then 'scheduled' else 'manual' end;
  insert into timegenie.task_occurrences(
    workspace_id,task_id,occurrence_date,origin,status
  ) values(p_workspace_id,p_task_id,p_occurrence_date,origin_value,'open')
  on conflict(task_id,occurrence_date) do nothing;
  return timegenie.direct_cloud_occurrence_record(
    p_workspace_id,p_task_id,p_occurrence_date
  );
end $$;

create or replace function timegenie.direct_cloud_recurrence_save(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,
  p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb;
declare task_id uuid; task_expected_version bigint; frequency_value text;
declare weekdays_mask_value integer; effective_start_value date; rule_expected_version bigint;
declare task_row timegenie.tasks%rowtype; current_rule timegenie.task_recurrence_rules%rowtype;
declare rule_id uuid; current_work_date date; workspace_timezone text;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':planning',0));
  cached:=timegenie.direct_cloud_operation_result(
    p_workspace_id,p_operation_id,'direct_recurrence_save',request_value
  ); if cached is not null then return cached; end if;
  task_id:=nullif(p_payload->>'taskId','')::uuid;
  task_expected_version:=nullif(p_payload->>'taskExpectedVersion','')::bigint;
  frequency_value:=nullif(p_payload->>'frequency','');
  weekdays_mask_value:=nullif(p_payload->>'weekdaysMask','')::integer;
  effective_start_value:=nullif(p_payload->>'effectiveStart','')::date;
  rule_expected_version:=nullif(p_payload->>'ruleExpectedVersion','')::bigint;
  if task_id is null or task_expected_version is null or frequency_value is null
     or effective_start_value is null then raise exception 'VALIDATION_ERROR: 重复规则参数不完整'; end if;
  if not ((frequency_value in('daily','weekdays') and weekdays_mask_value is null)
    or (frequency_value='weekly' and weekdays_mask_value between 1 and 127)) then
    raise exception 'VALIDATION_ERROR: 重复频率或星期选择无效';
  end if;
  select workspace.timezone into workspace_timezone from timegenie.workspaces workspace
  where workspace.id=p_workspace_id;
  current_work_date:=(now() at time zone workspace_timezone)::date;
  if effective_start_value<current_work_date then
    raise exception 'VALIDATION_ERROR: 重复规则开始日期不能早于今天';
  end if;
  select task.* into task_row from timegenie.tasks task
  where task.workspace_id=p_workspace_id and task.id=task_id for update;
  if not found then raise exception 'NOT_FOUND: 事项不存在'; end if;
  if task_row.deleted_at is not null then raise exception 'ENTITY_DELETED: 事项已删除'; end if;
  if task_row.version<>task_expected_version then
    raise exception 'VERSION_CONFLICT: 事项 expected %, current %',task_expected_version,task_row.version;
  end if;
  if task_row.day_instance_date is not null then raise exception 'VALIDATION_ERROR: 今日实例不能启用重复'; end if;
  if task_row.status='done' then raise exception 'TASK_COMPLETED: 请先将事项恢复为未完成再启用重复'; end if;
  if exists(select 1 from timegenie.tasks child where child.workspace_id=p_workspace_id
    and child.parent_id=task_id and child.deleted_at is null) then
    raise exception 'TASK_NOT_LEAF: 只有无子事项可以启用重复';
  end if;
  if exists(
    with recursive ancestors as (
      select task.parent_id as id from timegenie.tasks task where task.id=task_id
      union all select parent.parent_id from timegenie.tasks parent join ancestors child on parent.id=child.id
      where parent.workspace_id=p_workspace_id and parent.deleted_at is null
    ) select 1 from ancestors join timegenie.tasks parent on parent.id=ancestors.id
      where parent.status='done'
  ) then raise exception 'TASK_COMPLETED_ANCESTOR: 请先恢复已完成的上级事项'; end if;
  select rule.* into current_rule from timegenie.task_recurrence_rules rule
  where rule.workspace_id=p_workspace_id and rule.task_id=task_id and rule.effective_end is null
  order by rule.effective_start desc,rule.id desc limit 1 for update;
  if found then
    if rule_expected_version is distinct from current_rule.version then
      raise exception 'VERSION_CONFLICT: 重复规则已变化';
    end if;
    if current_rule.effective_start=effective_start_value
       or current_rule.effective_start>current_work_date then
      update timegenie.task_recurrence_rules set frequency=frequency_value,
        weekdays_mask=weekdays_mask_value,effective_start=effective_start_value,
        updated_at=now(),version=version+1 where id=current_rule.id;
      rule_id:=current_rule.id;
    else
      update timegenie.task_recurrence_rules set effective_end=effective_start_value-1,
        updated_at=now(),version=version+1 where id=current_rule.id;
      rule_id:=gen_random_uuid();
      insert into timegenie.task_recurrence_rules(
        id,workspace_id,task_id,frequency,weekdays_mask,effective_start
      ) values(rule_id,p_workspace_id,task_id,frequency_value,weekdays_mask_value,effective_start_value);
    end if;
  else
    if rule_expected_version is not null then raise exception 'VERSION_CONFLICT: 重复规则已关闭'; end if;
    rule_id:=gen_random_uuid();
    insert into timegenie.task_recurrence_rules(
      id,workspace_id,task_id,frequency,weekdays_mask,effective_start
    ) values(rule_id,p_workspace_id,task_id,frequency_value,weekdays_mask_value,effective_start_value);
  end if;
  update timegenie.tasks set status='open',completed_at=null,updated_at=now(),
    updated_by_device_id=p_device_id,version=version+1 where id=task_id;
  result_value:=timegenie.direct_cloud_mutation_response(
    p_workspace_id,p_device_id,p_operation_id,jsonb_build_object(
      'rule',timegenie.direct_cloud_recurrence_rule_record(p_workspace_id,rule_id),
      'rules',coalesce((select jsonb_agg(timegenie.direct_cloud_recurrence_rule_record(p_workspace_id,rule.id)
        order by rule.effective_start,rule.id) from timegenie.task_recurrence_rules rule
        where rule.workspace_id=p_workspace_id and rule.task_id=task_id),'[]'::jsonb),
      'task',timegenie.direct_cloud_task_project(p_workspace_id,task_id)
    ),array['recurrence','tasks','planning'],p_expected_epoch
  );
  perform timegenie.record_direct_cloud_operation(
    p_workspace_id,p_operation_id,p_device_id,'direct_recurrence_save',request_value,result_value
  ); return result_value;
end $$;

create or replace function timegenie.direct_cloud_recurrence_close(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,
  p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb;
declare task_id uuid; task_expected_version bigint; effective_end_value date;
declare rule_expected_version bigint; task_row timegenie.tasks%rowtype;
declare current_rule timegenie.task_recurrence_rules%rowtype;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':planning',0));
  cached:=timegenie.direct_cloud_operation_result(
    p_workspace_id,p_operation_id,'direct_recurrence_close',request_value
  ); if cached is not null then return cached; end if;
  task_id:=nullif(p_payload->>'taskId','')::uuid;
  task_expected_version:=nullif(p_payload->>'taskExpectedVersion','')::bigint;
  effective_end_value:=nullif(p_payload->>'effectiveEnd','')::date;
  rule_expected_version:=nullif(p_payload->>'ruleExpectedVersion','')::bigint;
  if task_id is null or task_expected_version is null or effective_end_value is null
     or rule_expected_version is null then raise exception 'VALIDATION_ERROR: 关闭重复参数不完整'; end if;
  select task.* into task_row from timegenie.tasks task
  where task.workspace_id=p_workspace_id and task.id=task_id for update;
  if not found then raise exception 'NOT_FOUND: 事项不存在'; end if;
  if task_row.deleted_at is not null then raise exception 'ENTITY_DELETED: 事项已删除'; end if;
  if task_row.version<>task_expected_version then raise exception 'VERSION_CONFLICT: 事项已变化'; end if;
  select rule.* into current_rule from timegenie.task_recurrence_rules rule
  where rule.workspace_id=p_workspace_id and rule.task_id=task_id and rule.effective_end is null
  order by rule.effective_start desc,rule.id desc limit 1 for update;
  if not found then raise exception 'NOT_FOUND: 事项没有启用重复'; end if;
  if current_rule.version<>rule_expected_version then raise exception 'VERSION_CONFLICT: 重复规则已变化'; end if;
  if effective_end_value<current_rule.effective_start then
    delete from timegenie.task_recurrence_rules where id=current_rule.id;
  else
    update timegenie.task_recurrence_rules set effective_end=effective_end_value,
      updated_at=now(),version=version+1 where id=current_rule.id;
  end if;
  update timegenie.tasks set status='open',completed_at=null,updated_at=now(),
    updated_by_device_id=p_device_id,version=version+1 where id=task_id;
  result_value:=timegenie.direct_cloud_mutation_response(
    p_workspace_id,p_device_id,p_operation_id,jsonb_build_object(
      'closedRuleId',current_rule.id,
      'rules',coalesce((select jsonb_agg(timegenie.direct_cloud_recurrence_rule_record(p_workspace_id,rule.id)
        order by rule.effective_start,rule.id) from timegenie.task_recurrence_rules rule
        where rule.workspace_id=p_workspace_id and rule.task_id=task_id),'[]'::jsonb),
      'task',timegenie.direct_cloud_task_project(p_workspace_id,task_id)
    ),array['recurrence','tasks','planning'],p_expected_epoch
  );
  perform timegenie.record_direct_cloud_operation(
    p_workspace_id,p_operation_id,p_device_id,'direct_recurrence_close',request_value,result_value
  ); return result_value;
end $$;

create or replace function timegenie.direct_cloud_recurrence_day_generate(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,
  p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; items_value jsonb:='[]'::jsonb;
declare subject_id uuid; work_date date; candidate record; plan_row timegenie.task_day_plans%rowtype;
declare plan_result jsonb; occurrence_value jsonb; plan_found boolean;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':planning',0));
  cached:=timegenie.direct_cloud_operation_result(
    p_workspace_id,p_operation_id,'direct_recurrence_day_generate',request_value
  ); if cached is not null then return cached; end if;
  subject_id:=nullif(p_payload->>'subjectId','')::uuid;
  work_date:=nullif(p_payload->>'workDate','')::date;
  if subject_id is null or work_date is null then raise exception 'VALIDATION_ERROR: 主体和日期不能为空'; end if;
  if not exists(select 1 from timegenie.subjects subject where subject.workspace_id=p_workspace_id
    and subject.id=subject_id and subject.deleted_at is null) then raise exception 'NOT_FOUND: 主体不存在'; end if;
  for candidate in
    select task.id,task.sort_order from timegenie.tasks task
    where task.workspace_id=p_workspace_id and task.subject_id=subject_id
      and task.deleted_at is null and task.day_instance_date is null
      and timegenie.direct_cloud_recurrence_matches_date(p_workspace_id,task.id,work_date)
    order by task.sort_order,task.created_at,task.id
  loop
    select plan.* into plan_row from timegenie.task_day_plans plan
    where plan.workspace_id=p_workspace_id and plan.task_id=candidate.id and plan.work_date=work_date for update;
    plan_found:=found;
    if plan_found and plan_row.state='excluded' then continue; end if;
    occurrence_value:=timegenie.direct_cloud_occurrence_ensure(p_workspace_id,candidate.id,work_date);
    plan_result:=timegenie.direct_cloud_day_plan_apply(
      p_workspace_id,p_device_id,candidate.id,work_date,'included',
      case when plan_found then plan_row.version else null end
    );
    items_value:=items_value||jsonb_build_array(jsonb_build_object(
      'taskId',candidate.id,'occurrence',occurrence_value,
      'plan',plan_result->'plan','instance',plan_result->'instance'
    ));
  end loop;
  result_value:=timegenie.direct_cloud_mutation_response(
    p_workspace_id,p_device_id,p_operation_id,
    jsonb_build_object('subjectId',subject_id,'workDate',work_date,
      'generatedCount',jsonb_array_length(items_value),'items',items_value),
    array['recurrence','planning','tasks'],p_expected_epoch
  );
  perform timegenie.record_direct_cloud_operation(
    p_workspace_id,p_operation_id,p_device_id,'direct_recurrence_day_generate',request_value,result_value
  ); return result_value;
end $$;

create or replace function timegenie.direct_cloud_recurrence_occurrence_set_status(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,
  p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; occurrence_value jsonb;
declare task_id uuid; occurrence_date_value date; done_value boolean;
declare task_expected_version bigint; occurrence_expected_version bigint; instance_expected_version bigint;
declare task_row timegenie.tasks%rowtype; occurrence_row timegenie.task_occurrences%rowtype;
declare instance_row timegenie.tasks%rowtype; target_status text; completed_value timestamptz;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':planning',0));
  cached:=timegenie.direct_cloud_operation_result(
    p_workspace_id,p_operation_id,'direct_recurrence_occurrence_set_status',request_value
  ); if cached is not null then return cached; end if;
  task_id:=nullif(p_payload->>'taskId','')::uuid;
  occurrence_date_value:=nullif(p_payload->>'occurrenceDate','')::date;
  done_value:=coalesce((p_payload->>'done')::boolean,false);
  task_expected_version:=nullif(p_payload->>'taskExpectedVersion','')::bigint;
  occurrence_expected_version:=nullif(p_payload->>'occurrenceExpectedVersion','')::bigint;
  instance_expected_version:=nullif(p_payload->>'instanceExpectedVersion','')::bigint;
  if task_id is null or occurrence_date_value is null or task_expected_version is null then
    raise exception 'VALIDATION_ERROR: 重复轮次状态参数不完整';
  end if;
  select task.* into task_row from timegenie.tasks task
  where task.workspace_id=p_workspace_id and task.id=task_id for update;
  if not found then raise exception 'NOT_FOUND: 事项不存在'; end if;
  if task_row.deleted_at is not null then raise exception 'ENTITY_DELETED: 事项已删除'; end if;
  if task_row.version<>task_expected_version then raise exception 'VERSION_CONFLICT: 事项已变化'; end if;
  occurrence_value:=timegenie.direct_cloud_occurrence_ensure(p_workspace_id,task_id,occurrence_date_value);
  select occurrence.* into occurrence_row from timegenie.task_occurrences occurrence
  where occurrence.workspace_id=p_workspace_id and occurrence.task_id=task_id
    and occurrence.occurrence_date=occurrence_date_value for update;
  if occurrence_expected_version is not null and occurrence_expected_version<>0
     and occurrence_row.version<>occurrence_expected_version then
    raise exception 'VERSION_CONFLICT: 当日重复事项状态已变化';
  end if;
  target_status:=case when done_value then 'done' else 'open' end;
  completed_value:=case when done_value then now() else null end;
  if occurrence_row.status is distinct from target_status then
    update timegenie.task_occurrences occurrence set
      status=target_status,completed_at=completed_value,updated_at=now(),version=occurrence.version+1
    where occurrence.workspace_id=p_workspace_id and occurrence.task_id=task_id
      and occurrence.occurrence_date=occurrence_date_value;
  end if;
  select task.* into instance_row from timegenie.tasks task
  where task.workspace_id=p_workspace_id and task.day_instance_source_task_id=task_id
    and task.day_instance_date=occurrence_date_value and task.day_instance_kind='member'
    and task.deleted_at is null order by task.created_at,task.id limit 1 for update;
  if found then
    if instance_expected_version is not null and instance_row.version<>instance_expected_version then
      raise exception 'VERSION_CONFLICT: 今日实例已变化';
    end if;
    if instance_row.status is distinct from target_status then
      update timegenie.tasks set status=target_status,completed_at=completed_value,
        updated_at=now(),updated_by_device_id=p_device_id,version=version+1 where id=instance_row.id;
    end if;
  end if;
  occurrence_value:=timegenie.direct_cloud_occurrence_record(p_workspace_id,task_id,occurrence_date_value);
  result_value:=timegenie.direct_cloud_mutation_response(
    p_workspace_id,p_device_id,p_operation_id,jsonb_build_object(
      'occurrence',occurrence_value,'task',timegenie.direct_cloud_task_project(p_workspace_id,task_id),
      'instance',case when instance_row.id is null then null else timegenie.direct_cloud_task_project(p_workspace_id,instance_row.id) end
    ),array['recurrence','planning','tasks'],p_expected_epoch
  );
  perform timegenie.record_direct_cloud_operation(
    p_workspace_id,p_operation_id,p_device_id,'direct_recurrence_occurrence_set_status',request_value,result_value
  ); return result_value;
end $$;

revoke all on function timegenie.direct_cloud_recurrence_rule_record(uuid,uuid) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_occurrence_record(uuid,uuid,date) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_occurrence_ensure(uuid,uuid,date) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_recurrence_save(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_recurrence_close(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_recurrence_day_generate(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_recurrence_occurrence_set_status(uuid,uuid,uuid,bigint,jsonb) from public,anon;
grant execute on function timegenie.direct_cloud_recurrence_save(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_recurrence_close(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_recurrence_day_generate(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_recurrence_occurrence_set_status(uuid,uuid,uuid,bigint,jsonb) to authenticated;

notify pgrst,'reload schema';
