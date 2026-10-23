-- Direct-v2 task-domain mutations required by the desktop storage router.
-- These operations keep compound desktop commands inside one cloud transaction.

drop function if exists timegenie.direct_cloud_task_list(uuid,uuid,bigint,uuid,boolean,text);

create or replace function timegenie.direct_cloud_task_list(
  p_workspace_id uuid,p_device_id uuid,p_expected_epoch bigint default null,
  p_subject_id uuid default null,p_include_completed boolean default true,
  p_query text default null,p_planned_date date default null
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare data_value jsonb;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  if p_subject_id is not null and not exists(
    select 1 from timegenie.subjects subject where subject.workspace_id=p_workspace_id and subject.id=p_subject_id and subject.deleted_at is null
  ) then raise exception 'NOT_FOUND: 主体不存在'; end if;
  select jsonb_build_object('tasks',coalesce(jsonb_agg(projected order by subject_sort,tree_path),'[]'::jsonb))
  into data_value from (
    with recursive tree as (
      select task.id,subject.sort_order as subject_sort,
        lpad((task.sort_order+1000000000)::text,11,'0')||'|'||task.created_at::text||'|'||task.id::text as tree_path
      from timegenie.tasks task join timegenie.subjects subject on subject.id=task.subject_id
      where task.workspace_id=p_workspace_id and task.deleted_at is null and task.day_instance_date is null
        and (p_subject_id is null or task.subject_id=p_subject_id)
        and (p_planned_date is null or task.planned_date=p_planned_date)
        and (task.parent_id is null or not exists(
          select 1 from timegenie.tasks parent where parent.workspace_id=p_workspace_id and parent.id=task.parent_id and parent.deleted_at is null and parent.day_instance_date is null
        ))
      union all
      select child.id,parent.subject_sort,parent.tree_path||'/'||lpad((child.sort_order+1000000000)::text,11,'0')||'|'||child.created_at::text||'|'||child.id::text
      from timegenie.tasks child join tree parent on child.parent_id=parent.id
      where child.workspace_id=p_workspace_id and child.deleted_at is null and child.day_instance_date is null
        and (p_planned_date is null or child.planned_date=p_planned_date)
    ) select tree.subject_sort,tree.tree_path,timegenie.direct_cloud_task_project(p_workspace_id,tree.id) as projected
    from tree join timegenie.tasks task on task.id=tree.id
    where (p_include_completed or task.status='open')
      and (nullif(btrim(p_query),'') is null or task.title ilike '%'||btrim(p_query)||'%')
  ) rows_value;
  return timegenie.direct_cloud_query_response(p_workspace_id,p_device_id,data_value,array['tasks'],p_expected_epoch);
end $$;

create or replace function timegenie.direct_cloud_task_set_status(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,
  p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb;
declare task_id uuid; expected_version bigint; done_value boolean; task_row timegenie.tasks%rowtype;
declare target_status text; completed_value timestamptz; before_value jsonb; after_value jsonb;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':tasks',0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_task_set_status',request_value);
  if cached is not null then return cached; end if;
  task_id:=nullif(p_payload->>'id','')::uuid;
  expected_version:=nullif(p_payload->>'expectedVersion','')::bigint;
  done_value:=coalesce((p_payload->>'done')::boolean,false);
  if task_id is null or expected_version is null then raise exception 'VALIDATION_ERROR: 事项状态参数不完整'; end if;
  select task.* into task_row from timegenie.tasks task
  where task.workspace_id=p_workspace_id and task.id=task_id for update;
  if not found then raise exception 'NOT_FOUND: 事项不存在'; end if;
  if task_row.deleted_at is not null then raise exception 'ENTITY_DELETED: 事项已删除'; end if;
  if task_row.version<>expected_version then raise exception 'VERSION_CONFLICT: 事项已变化'; end if;
  if task_row.day_instance_kind='context' then raise exception 'VALIDATION_ERROR: 今日上下文父事项不能直接完成'; end if;
  if task_row.day_instance_date is null and exists(
    select 1 from timegenie.task_recurrence_rules rule
    where rule.workspace_id=p_workspace_id and rule.task_id=task_id and rule.effective_end is null
  ) then raise exception 'VALIDATION_ERROR: 重复事项完成操作必须指定轮次日期'; end if;
  target_status:=case when done_value then 'done' else 'open' end;
  completed_value:=case when done_value then now() else null end;
  before_value:=to_jsonb(task_row);
  if task_row.status is distinct from target_status then
    update timegenie.tasks set status=target_status,completed_at=completed_value,updated_at=now(),
      updated_by_device_id=p_device_id,version=version+1 where id=task_id;
    insert into timegenie.task_status_events(id,workspace_id,task_id,status,occurred_at,source_type)
    values(gen_random_uuid(),p_workspace_id,task_id,target_status,now(),'user');
  end if;
  after_value:=(select to_jsonb(task) from timegenie.tasks task where task.id=task_id);
  result_value:=timegenie.direct_cloud_mutation_response(
    p_workspace_id,p_device_id,p_operation_id,
    jsonb_build_object('task',timegenie.direct_cloud_task_project(p_workspace_id,task_id)),
    array['tasks','planning'],p_expected_epoch
  );
  perform timegenie.record_direct_cloud_operation(
    p_workspace_id,p_operation_id,p_device_id,'direct_task_set_status',request_value,result_value
  );
  perform timegenie.operation_history_replace_item(
    p_workspace_id,p_operation_id,'task',task_id::text,before_value,after_value
  );
  return result_value;
end $$;

create or replace function timegenie.direct_cloud_today_recurring_update(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,
  p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb;
declare instance_id uuid; instance_expected_version bigint; source_expected_version bigint;
declare instance_row timegenie.tasks%rowtype; source_row timegenie.tasks%rowtype;
declare title_value text; estimate_value integer; note_value text; project_value text;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':planning',0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_today_recurring_update',request_value);
  if cached is not null then return cached; end if;
  instance_id:=nullif(p_payload->>'instanceId','')::uuid;
  instance_expected_version:=nullif(p_payload->>'instanceExpectedVersion','')::bigint;
  source_expected_version:=nullif(p_payload->>'sourceExpectedVersion','')::bigint;
  if instance_id is null or instance_expected_version is null or source_expected_version is null then
    raise exception 'VALIDATION_ERROR: 今日重复事项编辑参数不完整';
  end if;
  select task.* into instance_row from timegenie.tasks task
  where task.workspace_id=p_workspace_id and task.id=instance_id for update;
  if not found then raise exception 'NOT_FOUND: 今日事项不存在'; end if;
  if instance_row.deleted_at is not null then raise exception 'ENTITY_DELETED: 今日事项已删除'; end if;
  if instance_row.version<>instance_expected_version then raise exception 'VERSION_CONFLICT: 今日事项已变化'; end if;
  if instance_row.day_instance_kind<>'member' or instance_row.day_instance_source_task_id is null then
    raise exception 'VALIDATION_ERROR: 当前事项不是重复计划的今日成员';
  end if;
  select task.* into source_row from timegenie.tasks task
  where task.workspace_id=p_workspace_id and task.id=instance_row.day_instance_source_task_id for update;
  if not found then raise exception 'NOT_FOUND: 重复事项模板不存在'; end if;
  if source_row.deleted_at is not null then raise exception 'ENTITY_DELETED: 重复事项模板已删除'; end if;
  if source_row.version<>source_expected_version then raise exception 'VERSION_CONFLICT: 重复事项模板已变化'; end if;
  if not exists(select 1 from timegenie.task_recurrence_rules rule
    where rule.workspace_id=p_workspace_id and rule.task_id=source_row.id and rule.effective_end is null) then
    raise exception 'VALIDATION_ERROR: 来源事项不是有效的重复计划';
  end if;
  if p_payload ? 'title' then title_value:=btrim(coalesce(p_payload->>'title','')); if title_value='' then raise exception 'VALIDATION_ERROR: 事项名称不能为空'; end if; end if;
  if p_payload ? 'estimateMinutes' and p_payload->'estimateMinutes'<>'null'::jsonb then estimate_value:=(p_payload->>'estimateMinutes')::integer; if estimate_value<=0 then raise exception 'VALIDATION_ERROR: 预计用时必须大于 0 分钟'; end if; end if;
  if p_payload ? 'note' then note_value:=nullif(btrim(p_payload->>'note'),''); end if;
  if p_payload ? 'projectName' then project_value:=nullif(btrim(p_payload->>'projectName'),''); end if;
  update timegenie.tasks set
    title=case when p_payload ? 'title' then title_value else title end,
    planned_date=case when p_payload ? 'plannedDate' then nullif(p_payload->>'plannedDate','')::date else planned_date end,
    estimate_minutes=case when p_payload ? 'estimateMinutes' then estimate_value else estimate_minutes end,
    note=case when p_payload ? 'note' then note_value else note end,
    project_name=case when p_payload ? 'projectName' then project_value else project_name end,
    updated_at=now(),updated_by_device_id=p_device_id,version=version+1 where id=instance_id;
  update timegenie.tasks set
    title=case when p_payload ? 'title' then title_value else title end,
    estimate_minutes=case when p_payload ? 'estimateMinutes' then estimate_value else estimate_minutes end,
    note=case when p_payload ? 'note' then note_value else note end,
    updated_at=now(),updated_by_device_id=p_device_id,version=version+1 where id=source_row.id;
  result_value:=timegenie.direct_cloud_mutation_response(
    p_workspace_id,p_device_id,p_operation_id,jsonb_build_object(
      'instance',timegenie.direct_cloud_task_project(p_workspace_id,instance_id),
      'source',timegenie.direct_cloud_task_project(p_workspace_id,source_row.id)
    ),array['tasks','planning','recurrence'],p_expected_epoch
  );
  perform timegenie.record_direct_cloud_operation(
    p_workspace_id,p_operation_id,p_device_id,'direct_today_recurring_update',request_value,result_value
  );
  return result_value;
end $$;

create or replace function timegenie.direct_cloud_task_duplicate_subtree(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,
  p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb;
declare root_id uuid; expected_version bigint; root_row timegenie.tasks%rowtype; item record;
declare id_map jsonb:='{}'::jsonb; created_ids uuid[]:='{}'::uuid[]; new_id uuid; new_parent uuid; tasks_value jsonb;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':tasks',0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_task_duplicate_subtree',request_value);
  if cached is not null then return cached; end if;
  root_id:=nullif(p_payload->>'id','')::uuid; expected_version:=nullif(p_payload->>'expectedVersion','')::bigint;
  select task.* into root_row from timegenie.tasks task where task.workspace_id=p_workspace_id and task.id=root_id for update;
  if not found then raise exception 'NOT_FOUND: 事项不存在'; end if;
  if root_row.deleted_at is not null then raise exception 'ENTITY_DELETED: 事项已删除'; end if;
  if root_row.version<>expected_version then raise exception 'VERSION_CONFLICT: 事项已变化'; end if;
  for item in
    with recursive subtree as (
      select task.*,0 as depth_value from timegenie.tasks task where task.workspace_id=p_workspace_id and task.id=root_id
      union all select child.*,parent.depth_value+1 from timegenie.tasks child join subtree parent on child.parent_id=parent.id
      where child.workspace_id=p_workspace_id and child.deleted_at is null
    ) select * from subtree order by depth_value,sort_order,created_at,id
  loop
    new_id:=gen_random_uuid();
    new_parent:=case when item.id=root_id then item.parent_id else nullif(id_map->>item.parent_id::text,'')::uuid end;
    insert into timegenie.tasks(
      id,workspace_id,subject_id,parent_id,title,status,planned_date,estimate_minutes,note,
      project_name,solution_name,source_type,source_ref,sort_order,created_by_device_id,
      updated_by_device_id,day_instance_date,day_instance_kind
    ) values(
      new_id,p_workspace_id,item.subject_id,new_parent,
      case when item.id=root_id then item.title||' - 副本' else item.title end,'open',
      item.planned_date,item.estimate_minutes,item.note,item.project_name,item.solution_name,
      'manual',item.id::text,item.sort_order+1,p_device_id,p_device_id,
      item.day_instance_date,case when item.day_instance_date is null then null else 'member' end
    );
    id_map:=id_map||jsonb_build_object(item.id::text,new_id);
    created_ids:=array_append(created_ids,new_id);
  end loop;
  select coalesce(jsonb_agg(timegenie.direct_cloud_task_project(p_workspace_id,id) order by array_position(created_ids,id)),'[]'::jsonb)
    into tasks_value from unnest(created_ids) id;
  result_value:=timegenie.direct_cloud_mutation_response(
    p_workspace_id,p_device_id,p_operation_id,jsonb_build_object(
      'rootId',id_map->>root_id::text,'tasks',tasks_value
    ),array['tasks','planning'],p_expected_epoch
  );
  perform timegenie.record_direct_cloud_operation(
    p_workspace_id,p_operation_id,p_device_id,'direct_task_duplicate_subtree',request_value,result_value
  );
  return result_value;
end $$;

create or replace function timegenie.direct_cloud_task_plan_batch_apply(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,
  p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; item jsonb; exclude_item jsonb;
declare subject_id uuid; day_instance_date date; work_date date; task_id uuid; parent_id uuid;
declare line_key text; parent_line_key text; title_value text; parent_key text;
declare expected_version bigint; current_row timegenie.tasks%rowtype; before_value jsonb; after_value jsonb;
declare baseline_ids uuid[]:='{}'::uuid[]; current_ids uuid[]:='{}'::uuid[];
declare final_existing_ids uuid[]:='{}'::uuid[]; deleted_ids uuid[]:='{}'::uuid[]; confirmed_ids uuid[]:='{}'::uuid[];
declare created_ids uuid[]:='{}'::uuid[]; updated_ids uuid[]:='{}'::uuid[];
declare line_map jsonb:='{}'::jsonb; order_map jsonb:='{}'::jsonb; created_items jsonb:='[]'::jsonb;
declare order_value integer; created_count integer:=0; updated_count integer:=0;
declare deleted_count integer:=0; today_plan_changed_count integer:=0;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':tasks',0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_task_plan_batch_apply',request_value);
  if cached is not null then return cached; end if;

  subject_id:=nullif(p_payload->>'subjectId','')::uuid;
  day_instance_date:=nullif(p_payload->>'dayInstanceDate','')::date;
  if subject_id is null then raise exception 'VALIDATION_ERROR: 主体标识不能为空'; end if;
  if not exists(select 1 from timegenie.subjects subject
    where subject.workspace_id=p_workspace_id and subject.id=subject_id and subject.deleted_at is null) then
    raise exception 'NOT_FOUND: 主体不存在';
  end if;
  if day_instance_date is not null and p_payload->'todayPlanChange' is not null
     and p_payload->'todayPlanChange'<>'null'::jsonb then
    raise exception 'VALIDATION_ERROR: 今日实例编辑不能同时修改来源今日计划';
  end if;
  if jsonb_typeof(coalesce(p_payload->'baselineTasks','[]'::jsonb))<>'array'
     or jsonb_typeof(coalesce(p_payload->'finalItems','[]'::jsonb))<>'array'
     or jsonb_typeof(coalesce(p_payload->'confirmedDeleteTaskIds','[]'::jsonb))<>'array' then
    raise exception 'VALIDATION_ERROR: 批量计划参数必须是数组';
  end if;

  for item in select value from jsonb_array_elements(coalesce(p_payload->'baselineTasks','[]'::jsonb)) loop
    task_id:=nullif(item->>'taskId','')::uuid;
    expected_version:=nullif(item->>'expectedVersion','')::bigint;
    if task_id is null or expected_version is null then raise exception 'VALIDATION_ERROR: 基线事项参数不完整'; end if;
    if task_id=any(baseline_ids) then raise exception 'VALIDATION_ERROR: 基线事项不能重复'; end if;
    baseline_ids:=array_append(baseline_ids,task_id);
  end loop;

  select coalesce(array_agg(task.id order by task.id),'{}'::uuid[]) into current_ids
  from timegenie.tasks task
  where task.workspace_id=p_workspace_id and task.subject_id=subject_id
    and task.status='open' and task.deleted_at is null
    and task.day_instance_date is not distinct from day_instance_date;
  perform 1 from timegenie.tasks task where task.id=any(current_ids) for update;
  if (select coalesce(array_agg(id order by id),'{}'::uuid[]) from unnest(baseline_ids) id)
     is distinct from current_ids then
    raise exception 'VERSION_CONFLICT: 当前主体的进行中事项集合已变化';
  end if;
  for item in select value from jsonb_array_elements(coalesce(p_payload->'baselineTasks','[]'::jsonb)) loop
    task_id:=(item->>'taskId')::uuid; expected_version:=(item->>'expectedVersion')::bigint;
    if not exists(select 1 from timegenie.tasks task where task.id=task_id and task.version=expected_version) then
      raise exception 'VERSION_CONFLICT: 事项已被其他窗口或设备更新';
    end if;
  end loop;

  for item in select value from jsonb_array_elements(coalesce(p_payload->'finalItems','[]'::jsonb)) loop
    line_key:=btrim(coalesce(item->>'lineKey',''));
    title_value:=btrim(coalesce(item->>'title',''));
    parent_line_key:=nullif(btrim(item->>'parentLineKey'),'');
    if line_key='' or title_value='' then raise exception 'VALIDATION_ERROR: 事项行身份和名称不能为空'; end if;
    if line_map ? line_key then raise exception 'VALIDATION_ERROR: 事项行身份不能重复'; end if;
    if parent_line_key is not null and not (line_map ? parent_line_key) then
      raise exception 'VALIDATION_ERROR: 父事项行必须先于子事项出现';
    end if;
    parent_id:=case when parent_line_key is null then null else (line_map->>parent_line_key)::uuid end;
    task_id:=nullif(item->>'taskId','')::uuid;
    if task_id is not null then
      if not task_id=any(current_ids) then raise exception 'VERSION_CONFLICT: 最终事项不属于打开时的主体'; end if;
      if task_id=any(final_existing_ids) then raise exception 'VALIDATION_ERROR: 同一事项不能出现两次'; end if;
      final_existing_ids:=array_append(final_existing_ids,task_id);
      select task.* into current_row from timegenie.tasks task where task.id=task_id for update;
      if parent_id is not null then
        perform timegenie.direct_cloud_task_parent_assert(
          p_workspace_id,subject_id,parent_id,day_instance_date,task_id
        );
      end if;
    else
      task_id:=gen_random_uuid();
      if parent_id is not null then
        perform timegenie.direct_cloud_task_parent_assert(
          p_workspace_id,subject_id,parent_id,day_instance_date,null
        );
      end if;
    end if;
    parent_key:=coalesce(parent_id::text,'__root__');
    order_value:=coalesce((order_map->>parent_key)::integer,0)+10;
    order_map:=order_map||jsonb_build_object(parent_key,order_value);

    if nullif(item->>'taskId','') is null then
      insert into timegenie.tasks(
        id,workspace_id,subject_id,parent_id,title,status,source_type,sort_order,
        created_by_device_id,updated_by_device_id,day_instance_date,day_instance_kind
      ) values(
        task_id,p_workspace_id,subject_id,parent_id,title_value,'open','manual',order_value,
        p_device_id,p_device_id,day_instance_date,case when day_instance_date is null then null else 'member' end
      );
      created_ids:=array_append(created_ids,task_id); created_count:=created_count+1;
      created_items:=created_items||jsonb_build_array(jsonb_build_object('lineKey',line_key,'taskId',task_id));
    elsif current_row.title is distinct from title_value
       or current_row.parent_id is distinct from parent_id
       or current_row.sort_order is distinct from order_value then
      before_value:=to_jsonb(current_row);
      update timegenie.tasks set title=title_value,parent_id=parent_id,sort_order=order_value,
        updated_at=now(),updated_by_device_id=p_device_id,version=version+1 where id=task_id;
      updated_ids:=array_append(updated_ids,task_id); updated_count:=updated_count+1;
      after_value:=(select to_jsonb(task) from timegenie.tasks task where task.id=task_id);
    end if;
    line_map:=line_map||jsonb_build_object(line_key,task_id);
  end loop;

  select coalesce(array_agg(id order by id),'{}'::uuid[]) into deleted_ids
  from unnest(current_ids) id where not id=any(final_existing_ids);
  for item in select value from jsonb_array_elements(coalesce(p_payload->'confirmedDeleteTaskIds','[]'::jsonb)) loop
    task_id:=nullif(item#>>'{}','')::uuid;
    if task_id is null then raise exception 'VALIDATION_ERROR: 删除确认事项不能为空'; end if;
    if task_id=any(confirmed_ids) then raise exception 'VALIDATION_ERROR: 删除确认事项不能重复'; end if;
    confirmed_ids:=array_append(confirmed_ids,task_id);
  end loop;
  if (select coalesce(array_agg(id order by id),'{}'::uuid[]) from unnest(confirmed_ids) id)
     is distinct from deleted_ids then
    raise exception 'VALIDATION_ERROR: 删除确认范围已变化，请重新预览';
  end if;
  foreach task_id in array deleted_ids loop
    if day_instance_date is null then
      update timegenie.tasks set deleted_at=now(),updated_at=now(),updated_by_device_id=p_device_id,
        version=version+1 where id=task_id and deleted_at is null;
    else
      perform timegenie.direct_cloud_today_instance_archive(
        p_workspace_id,p_device_id,task_id,day_instance_date
      );
    end if;
    deleted_count:=deleted_count+1;
  end loop;

  if p_payload->'todayPlanChange' is not null and p_payload->'todayPlanChange'<>'null'::jsonb then
    work_date:=nullif(p_payload->'todayPlanChange'->>'workDate','')::date;
    if work_date is null then raise exception 'VALIDATION_ERROR: 今日计划日期不能为空'; end if;
    for item in select value from jsonb_array_elements(coalesce(p_payload->'todayPlanChange'->'includeLineKeys','[]'::jsonb)) loop
      line_key:=nullif(btrim(item#>>'{}'),'');
      if line_key is null or not (line_map ? line_key) then raise exception 'VALIDATION_ERROR: 今日计划新增行不存在'; end if;
      perform timegenie.direct_cloud_day_plan_apply(
        p_workspace_id,p_device_id,(line_map->>line_key)::uuid,work_date,'included',null
      );
      today_plan_changed_count:=today_plan_changed_count+1;
    end loop;
    for exclude_item in select value from jsonb_array_elements(coalesce(p_payload->'todayPlanChange'->'excludeItems','[]'::jsonb)) loop
      task_id:=nullif(exclude_item->>'taskId','')::uuid;
      expected_version:=nullif(exclude_item->>'expectedVersion','')::bigint;
      if task_id is null or not task_id=any(current_ids) then raise exception 'VERSION_CONFLICT: 移出今日事项已不存在'; end if;
      perform timegenie.direct_cloud_day_plan_apply(
        p_workspace_id,p_device_id,task_id,work_date,'excluded',expected_version
      );
      today_plan_changed_count:=today_plan_changed_count+1;
    end loop;
  end if;

  result_value:=timegenie.direct_cloud_mutation_response(
    p_workspace_id,p_device_id,p_operation_id,jsonb_build_object(
      'createdCount',created_count,'updatedCount',updated_count,'deletedCount',deleted_count,
      'todayPlanChangedCount',today_plan_changed_count,'createdItems',created_items
    ),array['tasks','planning','recurrence'],p_expected_epoch
  );
  perform timegenie.record_direct_cloud_operation(
    p_workspace_id,p_operation_id,p_device_id,'direct_task_plan_batch_apply',request_value,result_value
  );
  return result_value;
end $$;

revoke all on function timegenie.direct_cloud_task_set_status(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_task_list(uuid,uuid,bigint,uuid,boolean,text,date) from public,anon;
revoke all on function timegenie.direct_cloud_today_recurring_update(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_task_duplicate_subtree(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_task_plan_batch_apply(uuid,uuid,uuid,bigint,jsonb) from public,anon;
grant execute on function timegenie.direct_cloud_task_set_status(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_task_list(uuid,uuid,bigint,uuid,boolean,text,date) to authenticated;
grant execute on function timegenie.direct_cloud_today_recurring_update(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_task_duplicate_subtree(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_task_plan_batch_apply(uuid,uuid,uuid,bigint,jsonb) to authenticated;

notify pgrst,'reload schema';
