-- Limit batch plan edits/deletions to the editor baseline. Preserve tasks added
-- later and allow completed day instances without relaxing task version checks.

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
    and task.deleted_at is null and (day_instance_date is not null or task.status='open')
    and task.day_instance_date is not distinct from day_instance_date;
  perform 1 from timegenie.tasks task where task.id=any(current_ids) for update;
  for item in select value from jsonb_array_elements(coalesce(p_payload->'baselineTasks','[]'::jsonb)) loop
    task_id:=(item->>'taskId')::uuid; expected_version:=(item->>'expectedVersion')::bigint;
    if not task_id=any(current_ids) then raise exception 'VERSION_CONFLICT: 基线事项已不存在'; end if;
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
      if not task_id=any(baseline_ids) then raise exception 'VERSION_CONFLICT: 最终事项不属于打开时的主体'; end if;
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
  from unnest(baseline_ids) id where not id=any(final_existing_ids);
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
  if exists(select 1 from timegenie.tasks child
    where child.workspace_id=p_workspace_id and child.parent_id=any(deleted_ids)
      and child.deleted_at is null and not child.id=any(deleted_ids)) then
    raise exception 'VERSION_CONFLICT: 待删除父事项存在未确认删除的子事项，请重新预览';
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

revoke all on function timegenie.direct_cloud_task_plan_batch_apply(uuid,uuid,uuid,bigint,jsonb) from public,anon;
grant execute on function timegenie.direct_cloud_task_plan_batch_apply(uuid,uuid,uuid,bigint,jsonb) to authenticated;
notify pgrst, 'reload schema';
