-- Direct-v2 Obsidian plan import. The desktop client parses the local markdown
-- and stages a preview; confirming it creates every selected task atomically in
-- Supabase with the stable preview identifiers as idempotency inputs.

begin;

create or replace function timegenie.direct_cloud_plan_import_confirm(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; subject_id uuid; target_date date;
declare item_value jsonb; item_id uuid; parent_value uuid; title_value text; estimate_value integer;
declare sort_value integer; created_ids uuid[]:='{}'; task_list jsonb; inserted_task jsonb;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_plan_import_confirm',request_value);
  if cached is not null then return cached; end if;
  subject_id:=nullif(p_payload->>'subjectId','')::uuid;
  target_date:=nullif(p_payload->>'targetDate','')::date;
  if subject_id is null or target_date is null then raise exception 'VALIDATION_ERROR: 导入主体和目标日期不能为空'; end if;
  if jsonb_typeof(coalesce(p_payload->'items','null'::jsonb))<>'array' or jsonb_array_length(p_payload->'items')=0 then
    raise exception 'VALIDATION_ERROR: 导入事项不能为空';
  end if;
  perform 1 from timegenie.subjects where workspace_id=p_workspace_id and id=subject_id and deleted_at is null for update;
  if not found then raise exception 'NOT_FOUND: 主体不存在或已归档'; end if;
  for item_value in select value from jsonb_array_elements(p_payload->'items') loop
    item_id:=nullif(item_value->>'id','')::uuid;
    title_value:=btrim(coalesce(item_value->>'title',''));
    if item_id is null or title_value='' then raise exception 'VALIDATION_ERROR: 导入事项标识和名称不能为空'; end if;
    if exists(select 1 from timegenie.tasks where workspace_id=p_workspace_id and id=item_id) then
      raise exception 'VERSION_CONFLICT: 事项标识已存在';
    end if;
    if item_value ? 'estimateMinutes' and item_value->'estimateMinutes'<>'null'::jsonb then
      estimate_value:=(item_value->>'estimateMinutes')::integer;
      if estimate_value<=0 then raise exception 'VALIDATION_ERROR: 预计用时必须大于 0 分钟'; end if;
    end if;
  end loop;
  for item_value in select value from jsonb_array_elements(p_payload->'items') loop
    item_id:=nullif(item_value->>'id','')::uuid;
    title_value:=btrim(coalesce(item_value->>'title',''));
    estimate_value:=case when item_value ? 'estimateMinutes' and item_value->'estimateMinutes'<>'null'::jsonb
      then (item_value->>'estimateMinutes')::integer else null end;
    sort_value:=case when item_value ? 'sortOrder' and item_value->'sortOrder'<>'null'::jsonb
      then (item_value->>'sortOrder')::integer else 0 end;
    insert into timegenie.tasks(id,workspace_id,subject_id,parent_id,title,status,planned_date,estimate_minutes,source_type,sort_order,created_by_device_id,updated_by_device_id)
    values(item_id,p_workspace_id,subject_id,null,title_value,'open',target_date,estimate_value,'obsidian_import',sort_value,p_device_id,p_device_id);
    created_ids:=array_append(created_ids,item_id);
  end loop;
  for item_value in select value from jsonb_array_elements(p_payload->'items') loop
    item_id:=nullif(item_value->>'id','')::uuid;
    parent_value:=nullif(item_value->>'parentId','')::uuid;
    if parent_value is not null then
      if parent_value=item_id then raise exception 'VALIDATION_ERROR: 导入事项不能以自身为父事项'; end if;
      if not parent_value=any(created_ids) and not exists(select 1 from timegenie.tasks parent
          where parent.workspace_id=p_workspace_id and parent.id=parent_value
            and parent.subject_id=subject_id and parent.deleted_at is null) then
        raise exception 'VALIDATION_ERROR: 导入事项的父事项无效';
      end if;
      update timegenie.tasks task set parent_id=parent_value
      where task.workspace_id=p_workspace_id and task.id=item_id;
    end if;
  end loop;
  if exists(select 1 from timegenie.tasks child join timegenie.tasks parent on parent.id=child.parent_id
      where child.workspace_id=p_workspace_id and child.id=any(created_ids) and parent.parent_id is not null) then
    raise exception 'TASK_DEPTH_LIMIT: 导入事项最多支持两级';
  end if;
  select coalesce(jsonb_agg(timegenie.direct_cloud_task_project(p_workspace_id,created_item.task_id) order by created_item.position),'[]'::jsonb),
    coalesce(jsonb_object_agg(created_item.task_id::text,to_jsonb(task)),'{}'::jsonb)
    into task_list,inserted_task
    from unnest(created_ids) with ordinality as created_item(task_id,position)
    join timegenie.tasks task on task.id=created_item.task_id;
  result_value:=timegenie.direct_cloud_mutation_response(p_workspace_id,p_device_id,p_operation_id,
    jsonb_build_object('tasks',task_list,'count',array_length(created_ids,1)),array['tasks','planning'],p_expected_epoch);
  perform timegenie.record_direct_cloud_operation(p_workspace_id,p_operation_id,p_device_id,'direct_plan_import_confirm',request_value,result_value);
  for item_value in select value from jsonb_array_elements(p_payload->'items') loop
    item_id:=nullif(item_value->>'id','')::uuid;
    perform timegenie.operation_history_replace_item(p_workspace_id,p_operation_id,'task',item_id::text,null,inserted_task->(item_id::text));
  end loop;
  return result_value;
exception when invalid_text_representation then raise exception 'VALIDATION_ERROR: 导入参数格式无效'; end $$;

revoke all on function timegenie.direct_cloud_plan_import_confirm(uuid,uuid,uuid,bigint,jsonb) from public,anon;
grant execute on function timegenie.direct_cloud_plan_import_confirm(uuid,uuid,uuid,bigint,jsonb) to authenticated;

commit;

notify pgrst,'reload schema';
