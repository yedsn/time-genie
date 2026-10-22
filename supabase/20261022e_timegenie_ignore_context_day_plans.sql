-- Older clients materialized a day-plan row for hierarchy-only context parents.
-- The context task is valid, but it is not an executable today-plan member.
begin;

do $$ begin
  if to_regprocedure('timegenie.apply_task_day_plan_pre_context_guard(uuid,uuid,uuid,text,text,bigint,bigint,bigint,jsonb,jsonb)') is null then
    alter function timegenie.apply_task_day_plan(uuid,uuid,uuid,text,text,bigint,bigint,bigint,jsonb,jsonb)
      rename to apply_task_day_plan_pre_context_guard;
  end if;
end $$;

create or replace function timegenie.apply_task_day_plan(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_operation_type text,p_entity_id text,
  p_base_version bigint,p_payload_version bigint,p_coalesced_count bigint,p_payload jsonb,p_request jsonb
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare row_data jsonb:=coalesce(p_payload,'{}'::jsonb); cached jsonb; result_value jsonb;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  cached:=timegenie.processed_operation_result(p_workspace_id,p_operation_id,p_operation_type,p_request);
  if cached is not null then return cached; end if;
  if p_entity_id is null or p_entity_id is distinct from (row_data->>'task_id')||'|'||(row_data->>'work_date') then
    raise exception 'VALIDATION_ERROR: entity id does not match task day plan';
  end if;
  if row_data->>'state'='included' and nullif(row_data->>'instance_task_id','') is not null and exists(
    select 1 from timegenie.tasks instance
    where instance.id=(row_data->>'instance_task_id')::uuid
      and instance.workspace_id=p_workspace_id
      and instance.day_instance_source_task_id=(row_data->>'task_id')::uuid
      and instance.day_instance_date=(row_data->>'work_date')::date
      and instance.day_instance_kind='context'
      and instance.deleted_at is null
  ) then
    result_value:=jsonb_build_object(
      'entityType','task_day_plan','entityId',p_entity_id,'version',p_payload_version,
      'authoritative',null,'ignoredContextPlan',true
    );
    perform timegenie.record_processed_operation(
      p_workspace_id,p_operation_id,p_device_id,p_operation_type,p_request,result_value
    );
    return result_value;
  end if;
  return timegenie.apply_task_day_plan_pre_context_guard(
    p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_id,
    p_base_version,p_payload_version,p_coalesced_count,p_payload,p_request
  );
end $$;

revoke execute on function timegenie.apply_task_day_plan_pre_context_guard(uuid,uuid,uuid,text,text,bigint,bigint,bigint,jsonb,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.apply_task_day_plan(uuid,uuid,uuid,text,text,bigint,bigint,bigint,jsonb,jsonb) from public,anon,authenticated;

do $$ begin
  if to_regprocedure('timegenie.cloud_apply_patch_pre_context_day_plan_guard(uuid,uuid,uuid,text,text,text,bigint,jsonb)') is null then
    alter function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,jsonb)
      rename to cloud_apply_patch_pre_context_day_plan_guard;
  end if;
  if to_regprocedure('timegenie.cloud_apply_patch_pre_context_day_plan_guard(uuid,uuid,uuid,text,text,text,bigint,bigint,bigint,jsonb)') is null then
    alter function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,bigint,bigint,jsonb)
      rename to cloud_apply_patch_pre_context_day_plan_guard;
  end if;
end $$;

create or replace function timegenie.cloud_apply_patch(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_operation_type text,
  p_entity_type text,p_entity_id text,p_base_version bigint,p_payload jsonb
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare request_value jsonb; cached jsonb; result_value jsonb;
begin
  request_value:=jsonb_build_object('deviceId',p_device_id,'entityType',p_entity_type,'entityId',p_entity_id,
    'baseVersion',p_base_version,'payload',coalesce(p_payload,'{}'::jsonb));
  if p_entity_type='task_day_plan' and p_payload->>'state'='included'
     and nullif(p_payload->>'instance_task_id','') is not null and exists(
       select 1 from timegenie.tasks instance
       where instance.id=(p_payload->>'instance_task_id')::uuid and instance.workspace_id=p_workspace_id
         and instance.day_instance_source_task_id=(p_payload->>'task_id')::uuid
         and instance.day_instance_date=(p_payload->>'work_date')::date
         and instance.day_instance_kind='context' and instance.deleted_at is null
     ) then
    if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
    if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
    cached:=timegenie.processed_operation_result(p_workspace_id,p_operation_id,p_operation_type,request_value);
    if cached is not null then return cached; end if;
    result_value:=jsonb_build_object('entityType','task_day_plan','entityId',p_entity_id,
      'version',(p_payload->>'version')::bigint,'authoritative',null,'ignoredContextPlan',true);
    perform timegenie.record_processed_operation(p_workspace_id,p_operation_id,p_device_id,p_operation_type,request_value,result_value);
    return result_value;
  end if;
  return timegenie.cloud_apply_patch_pre_context_day_plan_guard(
    p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_type,p_entity_id,p_base_version,p_payload
  );
end $$;

create or replace function timegenie.cloud_apply_patch(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_operation_type text,
  p_entity_type text,p_entity_id text,p_base_version bigint,p_payload_version bigint,
  p_coalesced_count bigint,p_payload jsonb
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare request_value jsonb; cached jsonb; result_value jsonb;
begin
  request_value:=jsonb_build_object('deviceId',p_device_id,'entityType',p_entity_type,'entityId',p_entity_id,
    'baseVersion',p_base_version,'payloadVersion',p_payload_version,'coalescedCount',p_coalesced_count,
    'payload',coalesce(p_payload,'{}'::jsonb));
  if p_entity_type='task_day_plan' and p_payload->>'state'='included'
     and nullif(p_payload->>'instance_task_id','') is not null and exists(
       select 1 from timegenie.tasks instance
       where instance.id=(p_payload->>'instance_task_id')::uuid and instance.workspace_id=p_workspace_id
         and instance.day_instance_source_task_id=(p_payload->>'task_id')::uuid
         and instance.day_instance_date=(p_payload->>'work_date')::date
         and instance.day_instance_kind='context' and instance.deleted_at is null
     ) then
    if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
    if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
    cached:=timegenie.processed_operation_result(p_workspace_id,p_operation_id,p_operation_type,request_value);
    if cached is not null then return cached; end if;
    result_value:=jsonb_build_object('entityType','task_day_plan','entityId',p_entity_id,
      'version',p_payload_version,'authoritative',null,'ignoredContextPlan',true);
    perform timegenie.record_processed_operation(p_workspace_id,p_operation_id,p_device_id,p_operation_type,request_value,result_value);
    return result_value;
  end if;
  return timegenie.cloud_apply_patch_pre_context_day_plan_guard(
    p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_type,p_entity_id,p_base_version,
    p_payload_version,p_coalesced_count,p_payload
  );
end $$;

revoke execute on function timegenie.cloud_apply_patch_pre_context_day_plan_guard(uuid,uuid,uuid,text,text,text,bigint,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.cloud_apply_patch_pre_context_day_plan_guard(uuid,uuid,uuid,text,text,text,bigint,bigint,bigint,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,jsonb) from public,anon;
revoke execute on function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,bigint,bigint,jsonb) from public,anon;
grant execute on function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,jsonb) to authenticated;
grant execute on function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,bigint,bigint,jsonb) to authenticated;

commit;
notify pgrst,'reload schema';
