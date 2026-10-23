-- Direct-v2 daily-estimate and today-plan mutations. Planning writes are
-- serialized per workspace so instance materialization and batch validation
-- remain atomic across devices.

create or replace function timegenie.direct_cloud_daily_estimate_record(
  p_workspace_id uuid,p_task_id uuid,p_work_date date
) returns jsonb language sql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
  select jsonb_build_object(
    'taskId',estimate.task_id,'workDate',estimate.work_date,
    'estimateMinutes',estimate.estimate_minutes,'version',estimate.version
  )
  from timegenie.task_daily_estimates estimate
  where estimate.workspace_id=p_workspace_id and estimate.task_id=p_task_id
    and estimate.work_date=p_work_date;
$$;

create or replace function timegenie.direct_cloud_day_plan_record(
  p_workspace_id uuid,p_task_id uuid,p_work_date date
) returns jsonb language sql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
  select jsonb_build_object(
    'taskId',plan.task_id,'workDate',plan.work_date,'state',plan.state,
    'instanceTaskId',plan.instance_task_id,'version',plan.version
  )
  from timegenie.task_day_plans plan
  where plan.workspace_id=p_workspace_id and plan.task_id=p_task_id
    and plan.work_date=p_work_date;
$$;

create or replace function timegenie.direct_cloud_recurrence_matches_date(
  p_workspace_id uuid,p_task_id uuid,p_work_date date
) returns boolean language sql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
  select exists(
    select 1 from timegenie.task_recurrence_rules rule
    where rule.workspace_id=p_workspace_id and rule.task_id=p_task_id
      and rule.effective_start<=p_work_date
      and (rule.effective_end is null or rule.effective_end>=p_work_date)
      and (
        rule.frequency='daily'
        or (rule.frequency='weekdays' and extract(isodow from p_work_date)<6)
        or (rule.frequency='weekly'
          and (rule.weekdays_mask & (1 << (extract(isodow from p_work_date)::integer-1)))<>0)
      )
  );
$$;

create or replace function timegenie.direct_cloud_daily_estimate_move_to_instance(
  p_workspace_id uuid,p_source_task_id uuid,p_instance_task_id uuid,p_work_date date
) returns void language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare estimate_row timegenie.task_daily_estimates%rowtype;
begin
  select estimate.* into estimate_row
  from timegenie.task_daily_estimates estimate
  where estimate.workspace_id=p_workspace_id and estimate.task_id=p_source_task_id
    and estimate.work_date=p_work_date for update;
  if not found then return; end if;
  delete from timegenie.task_daily_estimates
  where workspace_id=p_workspace_id and task_id=p_source_task_id and work_date=p_work_date;
  insert into timegenie.task_daily_estimates(
    workspace_id,task_id,work_date,estimate_minutes,created_at,updated_at,version
  ) values(
    p_workspace_id,p_instance_task_id,p_work_date,estimate_row.estimate_minutes,
    now(),now(),1
  ) on conflict(task_id,work_date) do update set
    estimate_minutes=excluded.estimate_minutes,updated_at=excluded.updated_at,
    version=timegenie.task_daily_estimates.version+1;
end $$;

create or replace function timegenie.direct_cloud_today_instance_ensure(
  p_workspace_id uuid,p_device_id uuid,p_source_task_id uuid,p_work_date date,
  p_expected_kind text
) returns uuid language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare source_row timegenie.tasks%rowtype; instance_row timegenie.tasks%rowtype;
declare parent_instance_id uuid; instance_id uuid; occurrence_row timegenie.task_occurrences%rowtype;
declare completed_value timestamptz;
begin
  if p_expected_kind not in('member','context') then
    raise exception 'VALIDATION_ERROR: 今日实例类型无效';
  end if;
  select task.* into source_row from timegenie.tasks task
  where task.workspace_id=p_workspace_id and task.id=p_source_task_id for update;
  if not found then raise exception 'NOT_FOUND: 事项不存在'; end if;
  if source_row.deleted_at is not null then raise exception 'ENTITY_DELETED: 事项已删除'; end if;

  if source_row.day_instance_date is not null then
    if source_row.day_instance_date is distinct from p_work_date
       or source_row.day_instance_source_task_id is null then
      raise exception 'VALIDATION_ERROR: 今日实例日期或来源无效';
    end if;
    if p_expected_kind='member' and source_row.day_instance_kind='context' then
      select task.* into instance_row from timegenie.tasks task
      where task.workspace_id=p_workspace_id and task.id=source_row.day_instance_source_task_id;
      update timegenie.tasks set
        day_instance_kind='member',title=instance_row.title,
        planned_date=instance_row.planned_date,estimate_minutes=instance_row.estimate_minutes,
        note=instance_row.note,project_name=instance_row.project_name,
        solution_name=instance_row.solution_name,updated_at=now(),
        updated_by_device_id=p_device_id,version=version+1
      where id=source_row.id;
    end if;
    return source_row.id;
  end if;

  select task.* into instance_row from timegenie.tasks task
  where task.workspace_id=p_workspace_id
    and task.day_instance_source_task_id=p_source_task_id
    and task.day_instance_date=p_work_date and task.deleted_at is null
  order by task.created_at,task.id limit 1 for update;
  if found then
    if p_expected_kind='member' and instance_row.day_instance_kind='context' then
      update timegenie.tasks set
        day_instance_kind='member',title=source_row.title,
        planned_date=source_row.planned_date,estimate_minutes=source_row.estimate_minutes,
        note=source_row.note,project_name=source_row.project_name,
        solution_name=source_row.solution_name,updated_at=now(),
        updated_by_device_id=p_device_id,version=version+1
      where id=instance_row.id;
    end if;
    if p_expected_kind='member' then
      perform timegenie.direct_cloud_daily_estimate_move_to_instance(
        p_workspace_id,p_source_task_id,instance_row.id,p_work_date
      );
      if source_row.estimate_minutes is not null
         and timegenie.direct_cloud_recurrence_matches_date(
           p_workspace_id,p_source_task_id,p_work_date
         ) and not exists(
           select 1 from timegenie.task_daily_estimates estimate
           where estimate.workspace_id=p_workspace_id and estimate.task_id=instance_row.id
             and estimate.work_date=p_work_date
         ) then
        insert into timegenie.task_daily_estimates(
          workspace_id,task_id,work_date,estimate_minutes
        ) values(
          p_workspace_id,instance_row.id,p_work_date,source_row.estimate_minutes
        );
      end if;
    end if;
    return instance_row.id;
  end if;

  if source_row.parent_id is not null then
    parent_instance_id:=timegenie.direct_cloud_today_instance_ensure(
      p_workspace_id,p_device_id,source_row.parent_id,p_work_date,'context'
    );
  end if;
  instance_id:=gen_random_uuid();
  if p_expected_kind='member' then
    select occurrence.* into occurrence_row from timegenie.task_occurrences occurrence
    where occurrence.workspace_id=p_workspace_id and occurrence.task_id=p_source_task_id
      and occurrence.occurrence_date=p_work_date;
    if found and occurrence_row.status='done' then
      completed_value:=coalesce(occurrence_row.completed_at,now());
    end if;
  end if;
  insert into timegenie.tasks(
    id,workspace_id,subject_id,parent_id,title,status,planned_date,estimate_minutes,
    note,project_name,solution_name,source_type,sort_order,completed_at,
    created_by_device_id,updated_by_device_id,day_instance_date,
    day_instance_kind,day_instance_source_task_id
  ) values(
    instance_id,p_workspace_id,source_row.subject_id,parent_instance_id,source_row.title,
    case when completed_value is null then 'open' else 'done' end,
    source_row.planned_date,source_row.estimate_minutes,source_row.note,
    source_row.project_name,source_row.solution_name,source_row.source_type,
    source_row.sort_order,completed_value,p_device_id,p_device_id,p_work_date,
    p_expected_kind,p_source_task_id
  );
  if p_expected_kind='member' then
    perform timegenie.direct_cloud_daily_estimate_move_to_instance(
      p_workspace_id,p_source_task_id,instance_id,p_work_date
    );
    if source_row.estimate_minutes is not null
       and timegenie.direct_cloud_recurrence_matches_date(
         p_workspace_id,p_source_task_id,p_work_date
       ) and not exists(
         select 1 from timegenie.task_daily_estimates estimate
         where estimate.workspace_id=p_workspace_id and estimate.task_id=instance_id
           and estimate.work_date=p_work_date
       ) then
      insert into timegenie.task_daily_estimates(
        workspace_id,task_id,work_date,estimate_minutes
      ) values(p_workspace_id,instance_id,p_work_date,source_row.estimate_minutes);
    end if;
  end if;
  return instance_id;
end $$;

create or replace function timegenie.direct_cloud_today_instance_archive(
  p_workspace_id uuid,p_device_id uuid,p_instance_task_id uuid,p_work_date date
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare archive_time timestamptz:=now(); affected_ids jsonb; root_parent_id uuid;
declare context_row timegenie.tasks%rowtype;
begin
  select task.parent_id into root_parent_id from timegenie.tasks task
  where task.workspace_id=p_workspace_id and task.id=p_instance_task_id
    and task.day_instance_date=p_work_date and task.deleted_at is null;
  if not found then return '[]'::jsonb; end if;
  with recursive subtree as (
    select task.id,task.day_instance_source_task_id from timegenie.tasks task
    where task.workspace_id=p_workspace_id and task.id=p_instance_task_id
      and task.day_instance_date=p_work_date and task.deleted_at is null
    union all
    select child.id,child.day_instance_source_task_id
    from timegenie.tasks child join subtree parent on child.parent_id=parent.id
    where child.workspace_id=p_workspace_id and child.day_instance_date=p_work_date
      and child.deleted_at is null
  ), updated as (
    update timegenie.tasks task set deleted_at=archive_time,updated_at=archive_time,
      updated_by_device_id=p_device_id,version=task.version+1
    where task.id in(select id from subtree) returning task.id,task.day_instance_source_task_id
  ) select coalesce(jsonb_agg(id order by id),'[]'::jsonb) into affected_ids from updated;
  delete from timegenie.task_daily_estimates estimate
  where estimate.workspace_id=p_workspace_id and estimate.work_date=p_work_date
    and estimate.task_id in(
      select value::text::uuid from jsonb_array_elements_text(affected_ids)
    );
  insert into timegenie.task_day_plans(
    workspace_id,task_id,work_date,state,instance_task_id,created_at,updated_at,version
  )
  select p_workspace_id,task.day_instance_source_task_id,p_work_date,'excluded',null,
    now(),now(),1
  from timegenie.tasks task
  where task.id in(select value::text::uuid from jsonb_array_elements_text(affected_ids))
    and task.day_instance_kind='member' and task.day_instance_source_task_id is not null
  on conflict(task_id,work_date) do update set
    state='excluded',instance_task_id=null,updated_at=excluded.updated_at,
    version=case
      when timegenie.task_day_plans.state='excluded'
       and timegenie.task_day_plans.instance_task_id is null
      then timegenie.task_day_plans.version
      else timegenie.task_day_plans.version+1 end;

  while root_parent_id is not null loop
    select task.* into context_row from timegenie.tasks task
    where task.workspace_id=p_workspace_id and task.id=root_parent_id
      and task.day_instance_date=p_work_date and task.deleted_at is null for update;
    if not found or context_row.day_instance_kind<>'context' then exit; end if;
    if exists(select 1 from timegenie.tasks child
      where child.workspace_id=p_workspace_id and child.parent_id=context_row.id
        and child.day_instance_date=p_work_date and child.deleted_at is null) then exit; end if;
    update timegenie.tasks set deleted_at=archive_time,updated_at=archive_time,
      updated_by_device_id=p_device_id,version=version+1 where id=context_row.id;
    affected_ids:=affected_ids||jsonb_build_array(context_row.id);
    delete from timegenie.task_daily_estimates
    where workspace_id=p_workspace_id and task_id=context_row.id and work_date=p_work_date;
    root_parent_id:=context_row.parent_id;
  end loop;
  return affected_ids;
end $$;

create or replace function timegenie.direct_cloud_daily_estimate_apply(
  p_workspace_id uuid,p_device_id uuid,p_task_id uuid,p_work_date date,
  p_estimate_minutes integer,p_expected_version bigint,
  p_expected_task_version bigint default null,p_update_recurring_estimate boolean default false
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare task_row timegenie.tasks%rowtype; current_row timegenie.task_daily_estimates%rowtype;
declare current_version bigint; recurring_match boolean;
begin
  select task.* into task_row from timegenie.tasks task
  where task.workspace_id=p_workspace_id and task.id=p_task_id for update;
  if not found then raise exception 'NOT_FOUND: 事项不存在'; end if;
  if task_row.deleted_at is not null then raise exception 'ENTITY_DELETED: 事项已删除'; end if;
  if task_row.day_instance_kind='context' then
    raise exception 'VALIDATION_ERROR: 今日上下文事项不能设置预计';
  end if;
  if task_row.day_instance_date is not null
     and task_row.day_instance_date is distinct from p_work_date then
    raise exception 'VALIDATION_ERROR: 今日预计必须属于实例日期';
  end if;
  if p_expected_task_version is not null
     and task_row.version is distinct from p_expected_task_version then
    raise exception 'VERSION_CONFLICT: 事项 expected %, current %',
      p_expected_task_version,task_row.version;
  end if;
  if p_estimate_minutes is not null and (p_estimate_minutes<=0 or p_estimate_minutes>1440) then
    raise exception 'VALIDATION_ERROR: 单项预计必须在 1 到 1440 分钟之间';
  end if;
  select estimate.* into current_row from timegenie.task_daily_estimates estimate
  where estimate.workspace_id=p_workspace_id and estimate.task_id=p_task_id
    and estimate.work_date=p_work_date for update;
  current_version:=case when found then current_row.version else null end;
  if current_version is distinct from p_expected_version then
    raise exception 'VERSION_CONFLICT: 今日预计 expected %, current %',
      p_expected_version,current_version;
  end if;
  recurring_match:=task_row.day_instance_date is null
    and timegenie.direct_cloud_recurrence_matches_date(
      p_workspace_id,p_task_id,p_work_date
    );
  if p_update_recurring_estimate and recurring_match then
    if p_estimate_minutes is null and task_row.estimate_minutes is not null then
      raise exception 'VALIDATION_ERROR: 重复事项仍有每次预计，请在单项编辑中调整';
    end if;
    if p_estimate_minutes is not null then
      update timegenie.tasks set estimate_minutes=p_estimate_minutes,updated_at=now(),
        updated_by_device_id=p_device_id,version=version+1 where id=p_task_id;
    end if;
  elsif recurring_match and p_estimate_minutes is not null then
    insert into timegenie.task_occurrences(
      workspace_id,task_id,occurrence_date,origin,status
    ) values(p_workspace_id,p_task_id,p_work_date,'scheduled','open')
    on conflict(task_id,occurrence_date) do nothing;
  end if;
  if p_estimate_minutes is null then
    delete from timegenie.task_daily_estimates
    where workspace_id=p_workspace_id and task_id=p_task_id and work_date=p_work_date;
  elsif current_version is null then
    insert into timegenie.task_daily_estimates(
      workspace_id,task_id,work_date,estimate_minutes
    ) values(p_workspace_id,p_task_id,p_work_date,p_estimate_minutes);
  else
    update timegenie.task_daily_estimates set estimate_minutes=p_estimate_minutes,
      updated_at=now(),version=version+1
    where workspace_id=p_workspace_id and task_id=p_task_id and work_date=p_work_date;
  end if;
  return jsonb_build_object(
    'taskId',p_task_id,'workDate',p_work_date,
    'dailyEstimate',timegenie.direct_cloud_daily_estimate_record(
      p_workspace_id,p_task_id,p_work_date
    ),
    'task',timegenie.direct_cloud_task_project(p_workspace_id,p_task_id)
  );
end $$;

create or replace function timegenie.direct_cloud_day_plan_apply(
  p_workspace_id uuid,p_device_id uuid,p_task_id uuid,p_work_date date,
  p_state text,p_expected_version bigint
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare input_row timegenie.tasks%rowtype; source_row timegenie.tasks%rowtype;
declare current_row timegenie.task_day_plans%rowtype; current_version bigint;
declare source_task_id uuid; instance_id uuid; affected_ids jsonb:='[]'::jsonb;
declare plan_value jsonb; instance_value jsonb;
begin
  if p_state not in('included','excluded') then
    raise exception 'VALIDATION_ERROR: 今日计划状态无效';
  end if;
  select task.* into input_row from timegenie.tasks task
  where task.workspace_id=p_workspace_id and task.id=p_task_id for update;
  if not found then raise exception 'NOT_FOUND: 事项不存在'; end if;
  if input_row.deleted_at is not null then raise exception 'ENTITY_DELETED: 事项已删除'; end if;
  if input_row.day_instance_date is not null then
    if input_row.day_instance_date is distinct from p_work_date
       or input_row.day_instance_kind<>'member'
       or input_row.day_instance_source_task_id is null then
      raise exception 'VALIDATION_ERROR: 今日实例不属于目标日期或不可作为成员';
    end if;
    source_task_id:=input_row.day_instance_source_task_id;
    instance_id:=input_row.id;
  else
    source_task_id:=input_row.id;
  end if;
  select task.* into source_row from timegenie.tasks task
  where task.workspace_id=p_workspace_id and task.id=source_task_id for update;
  if not found then raise exception 'NOT_FOUND: 来源事项不存在'; end if;
  if source_row.deleted_at is not null then raise exception 'ENTITY_DELETED: 来源事项已删除'; end if;
  select plan.* into current_row from timegenie.task_day_plans plan
  where plan.workspace_id=p_workspace_id and plan.task_id=source_task_id
    and plan.work_date=p_work_date for update;
  current_version:=case when found then current_row.version else null end;
  if current_version is distinct from p_expected_version then
    raise exception 'VERSION_CONFLICT: 今日计划 expected %, current %',
      p_expected_version,current_version;
  end if;

  if p_state='included' then
    if instance_id is null and current_version is not null and current_row.state='included'
       and current_row.instance_task_id is not null and exists(
         select 1 from timegenie.tasks task
         where task.workspace_id=p_workspace_id and task.id=current_row.instance_task_id
           and task.deleted_at is null and task.day_instance_date=p_work_date
           and task.day_instance_kind='member'
           and task.day_instance_source_task_id=source_task_id
       ) then
      instance_id:=current_row.instance_task_id;
    end if;
    if instance_id is null then
      instance_id:=timegenie.direct_cloud_today_instance_ensure(
        p_workspace_id,p_device_id,source_task_id,p_work_date,'member'
      );
    end if;
    if current_version is null then
      insert into timegenie.task_day_plans(
        workspace_id,task_id,work_date,state,instance_task_id
      ) values(p_workspace_id,source_task_id,p_work_date,'included',instance_id);
    elsif current_row.state is distinct from 'included'
       or current_row.instance_task_id is distinct from instance_id then
      update timegenie.task_day_plans set state='included',instance_task_id=instance_id,
        updated_at=now(),version=version+1
      where workspace_id=p_workspace_id and task_id=source_task_id and work_date=p_work_date;
    end if;
    instance_value:=timegenie.direct_cloud_task_project(p_workspace_id,instance_id);
  else
    if instance_id is null and current_version is not null and current_row.state='included' then
      instance_id:=current_row.instance_task_id;
    end if;
    if instance_id is null then
      select task.id into instance_id from timegenie.tasks task
      where task.workspace_id=p_workspace_id
        and task.day_instance_source_task_id=source_task_id
        and task.day_instance_date=p_work_date and task.deleted_at is null
        and task.day_instance_kind='member'
      order by task.created_at,task.id limit 1;
    end if;
    if instance_id is not null then
      affected_ids:=timegenie.direct_cloud_today_instance_archive(
        p_workspace_id,p_device_id,instance_id,p_work_date
      );
      instance_value:=timegenie.direct_cloud_task_record(
        p_workspace_id,instance_id,true
      );
    end if;
    delete from timegenie.task_daily_estimates
    where workspace_id=p_workspace_id and task_id=source_task_id and work_date=p_work_date;
    select plan.* into current_row from timegenie.task_day_plans plan
    where plan.workspace_id=p_workspace_id and plan.task_id=source_task_id
      and plan.work_date=p_work_date for update;
    if not found then
      insert into timegenie.task_day_plans(
        workspace_id,task_id,work_date,state,instance_task_id
      ) values(p_workspace_id,source_task_id,p_work_date,'excluded',null);
    elsif current_row.state is distinct from 'excluded'
       or current_row.instance_task_id is not null then
      update timegenie.task_day_plans set state='excluded',instance_task_id=null,
        updated_at=now(),version=version+1
      where workspace_id=p_workspace_id and task_id=source_task_id and work_date=p_work_date;
    end if;
  end if;
  plan_value:=timegenie.direct_cloud_day_plan_record(
    p_workspace_id,source_task_id,p_work_date
  );
  return jsonb_build_object(
    'taskId',source_task_id,'workDate',p_work_date,'state',p_state,
    'plan',plan_value,'instance',instance_value,'affectedTaskIds',affected_ids
  );
end $$;

create or replace function timegenie.direct_cloud_task_daily_estimate_set(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,
  p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; item_value jsonb;
declare task_id uuid; work_date date; estimate_minutes integer; expected_version bigint;
declare expected_task_version bigint; update_recurring_estimate boolean;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':planning',0));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(
    p_workspace_id,p_operation_id,'direct_task_daily_estimate_set',request_value
  );
  if cached is not null then return cached; end if;
  task_id:=nullif(p_payload->>'taskId','')::uuid;
  work_date:=nullif(p_payload->>'workDate','')::date;
  estimate_minutes:=nullif(p_payload->>'estimateMinutes','')::integer;
  expected_version:=nullif(p_payload->>'expectedVersion','')::bigint;
  expected_task_version:=nullif(p_payload->>'expectedTaskVersion','')::bigint;
  update_recurring_estimate:=coalesce((p_payload->>'updateRecurringEstimate')::boolean,false);
  if task_id is null or work_date is null then
    raise exception 'VALIDATION_ERROR: 事项和日期不能为空';
  end if;
  item_value:=timegenie.direct_cloud_daily_estimate_apply(
    p_workspace_id,p_device_id,task_id,work_date,estimate_minutes,
    expected_version,expected_task_version,update_recurring_estimate
  );
  result_value:=timegenie.direct_cloud_mutation_response(
    p_workspace_id,p_device_id,p_operation_id,item_value,
    array['planning','tasks','recurrence'],p_expected_epoch
  );
  perform timegenie.record_direct_cloud_operation(
    p_workspace_id,p_operation_id,p_device_id,'direct_task_daily_estimate_set',
    request_value,result_value
  );
  return result_value;
end $$;

create or replace function timegenie.direct_cloud_task_daily_estimate_batch_set(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,
  p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; items_value jsonb;
declare subject_id uuid; work_date date; item jsonb; item_result jsonb;
declare task_row timegenie.tasks%rowtype;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':planning',0));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(
    p_workspace_id,p_operation_id,'direct_task_daily_estimate_batch_set',request_value
  );
  if cached is not null then return cached; end if;
  subject_id:=nullif(p_payload->>'subjectId','')::uuid;
  work_date:=nullif(p_payload->>'workDate','')::date;
  if subject_id is null or work_date is null
     or jsonb_typeof(coalesce(p_payload->'items','null'::jsonb))<>'array' then
    raise exception 'VALIDATION_ERROR: 批量预计参数不完整';
  end if;
  if not exists(select 1 from timegenie.subjects subject
    where subject.workspace_id=p_workspace_id and subject.id=subject_id
      and subject.deleted_at is null) then
    raise exception 'NOT_FOUND: 主体不存在';
  end if;
  if exists(
    select 1 from jsonb_array_elements(p_payload->'items') value
    group by value->>'taskId' having count(*)>1
  ) then raise exception 'VALIDATION_ERROR: 批量预计不能包含重复事项'; end if;
  items_value:='[]'::jsonb;
  for item in select value from jsonb_array_elements(p_payload->'items') loop
    select task.* into task_row from timegenie.tasks task
    where task.workspace_id=p_workspace_id and task.id=nullif(item->>'taskId','')::uuid;
    if not found then raise exception 'NOT_FOUND: 事项不存在'; end if;
    if task_row.subject_id is distinct from subject_id then
      raise exception 'VALIDATION_ERROR: 事项不属于当前主体';
    end if;
    item_result:=timegenie.direct_cloud_daily_estimate_apply(
      p_workspace_id,p_device_id,task_row.id,work_date,
      nullif(item->>'estimateMinutes','')::integer,
      nullif(item->>'expectedDailyEstimateVersion','')::bigint,
      nullif(item->>'expectedTaskVersion','')::bigint,true
    );
    items_value:=items_value||jsonb_build_array(item_result);
  end loop;
  result_value:=timegenie.direct_cloud_mutation_response(
    p_workspace_id,p_device_id,p_operation_id,
    jsonb_build_object('changedCount',jsonb_array_length(items_value),'items',items_value),
    array['planning','tasks','recurrence'],p_expected_epoch
  );
  perform timegenie.record_direct_cloud_operation(
    p_workspace_id,p_operation_id,p_device_id,'direct_task_daily_estimate_batch_set',
    request_value,result_value
  );
  return result_value;
end $$;

create or replace function timegenie.direct_cloud_task_day_plan_set(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,
  p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; item_value jsonb;
declare task_id uuid; work_date date; state_value text; expected_version bigint;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':planning',0));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(
    p_workspace_id,p_operation_id,'direct_task_day_plan_set',request_value
  );
  if cached is not null then return cached; end if;
  task_id:=nullif(p_payload->>'taskId','')::uuid;
  work_date:=nullif(p_payload->>'workDate','')::date;
  state_value:=nullif(p_payload->>'state','');
  expected_version:=nullif(p_payload->>'expectedVersion','')::bigint;
  if task_id is null or work_date is null or state_value is null then
    raise exception 'VALIDATION_ERROR: 今日计划参数不完整';
  end if;
  item_value:=timegenie.direct_cloud_day_plan_apply(
    p_workspace_id,p_device_id,task_id,work_date,state_value,expected_version
  );
  result_value:=timegenie.direct_cloud_mutation_response(
    p_workspace_id,p_device_id,p_operation_id,item_value,
    array['planning','tasks'],p_expected_epoch
  );
  perform timegenie.record_direct_cloud_operation(
    p_workspace_id,p_operation_id,p_device_id,'direct_task_day_plan_set',
    request_value,result_value
  );
  return result_value;
end $$;

create or replace function timegenie.direct_cloud_task_day_plan_batch_set(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,
  p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; items_value jsonb;
declare subject_id uuid; work_date date; item jsonb; item_result jsonb;
declare task_row timegenie.tasks%rowtype;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':planning',0));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(
    p_workspace_id,p_operation_id,'direct_task_day_plan_batch_set',request_value
  );
  if cached is not null then return cached; end if;
  subject_id:=nullif(p_payload->>'subjectId','')::uuid;
  work_date:=nullif(p_payload->>'workDate','')::date;
  if subject_id is null or work_date is null
     or jsonb_typeof(coalesce(p_payload->'items','null'::jsonb))<>'array' then
    raise exception 'VALIDATION_ERROR: 批量今日计划参数不完整';
  end if;
  if not exists(select 1 from timegenie.subjects subject
    where subject.workspace_id=p_workspace_id and subject.id=subject_id
      and subject.deleted_at is null) then
    raise exception 'NOT_FOUND: 主体不存在';
  end if;
  if exists(
    select 1 from jsonb_array_elements(p_payload->'items') value
    group by value->>'taskId' having count(*)>1
  ) then raise exception 'VALIDATION_ERROR: 批量今日计划不能包含重复事项'; end if;
  items_value:='[]'::jsonb;
  for item in select value from jsonb_array_elements(p_payload->'items') loop
    select task.* into task_row from timegenie.tasks task
    where task.workspace_id=p_workspace_id and task.id=nullif(item->>'taskId','')::uuid;
    if not found then raise exception 'NOT_FOUND: 事项不存在'; end if;
    if task_row.subject_id is distinct from subject_id then
      raise exception 'VALIDATION_ERROR: 事项不属于当前主体';
    end if;
    item_result:=timegenie.direct_cloud_day_plan_apply(
      p_workspace_id,p_device_id,task_row.id,work_date,item->>'state',
      nullif(item->>'expectedVersion','')::bigint
    );
    items_value:=items_value||jsonb_build_array(item_result);
  end loop;
  result_value:=timegenie.direct_cloud_mutation_response(
    p_workspace_id,p_device_id,p_operation_id,
    jsonb_build_object('changedCount',jsonb_array_length(items_value),'items',items_value),
    array['planning','tasks'],p_expected_epoch
  );
  perform timegenie.record_direct_cloud_operation(
    p_workspace_id,p_operation_id,p_device_id,'direct_task_day_plan_batch_set',
    request_value,result_value
  );
  return result_value;
end $$;

revoke all on function timegenie.direct_cloud_daily_estimate_record(uuid,uuid,date) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_day_plan_record(uuid,uuid,date) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_recurrence_matches_date(uuid,uuid,date) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_daily_estimate_move_to_instance(uuid,uuid,uuid,date) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_today_instance_ensure(uuid,uuid,uuid,date,text) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_today_instance_archive(uuid,uuid,uuid,date) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_daily_estimate_apply(uuid,uuid,uuid,date,integer,bigint,bigint,boolean) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_day_plan_apply(uuid,uuid,uuid,date,text,bigint) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_task_daily_estimate_set(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_task_daily_estimate_batch_set(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_task_day_plan_set(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_task_day_plan_batch_set(uuid,uuid,uuid,bigint,jsonb) from public,anon;

grant execute on function timegenie.direct_cloud_task_daily_estimate_set(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_task_daily_estimate_batch_set(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_task_day_plan_set(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_task_day_plan_batch_set(uuid,uuid,uuid,bigint,jsonb) to authenticated;

notify pgrst,'reload schema';
