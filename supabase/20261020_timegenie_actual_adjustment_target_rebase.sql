-- Rebase actual-time adjustments to the user's target against the latest
-- cloud actual. Run after 20261015_timegenie_actual_adjustment_batch.sql.

begin;

create or replace function timegenie.actual_adjustment_batch_apply(
  p_workspace_id uuid, p_device_id uuid, p_operation_id uuid, p_payload jsonb
) returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare
  cached jsonb; request_value jsonb; item jsonb; draft jsonb; created_wrapper jsonb;
  task_image jsonb; adjustment_image jsonb; current_task timegenie.tasks;
  current_actual bigint; effective_delta integer; result_value jsonb;
  applied_adjustments jsonb := '[]'::jsonb; discarded_adjustment_ids jsonb := '[]'::jsonb;
  expected_adjustment_count integer := 0; matching_count integer;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_REVOKED'; end if;
  request_value:=jsonb_build_object('deviceId',p_device_id,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text || ':actual_adjustment_batch_apply:' || p_operation_id::text,0));
  cached:=timegenie.processed_operation_result(p_workspace_id,p_operation_id,'actual_adjustment_batch_apply',request_value);
  if cached is not null then return cached; end if;
  if jsonb_typeof(p_payload) is distinct from 'object'
     or jsonb_typeof(coalesce(p_payload->'items','[]'::jsonb)) is distinct from 'array'
     or jsonb_typeof(coalesce(p_payload->'newTasks','[]'::jsonb)) is distinct from 'array'
     or jsonb_typeof(coalesce(p_payload->'createdTasks','[]'::jsonb)) is distinct from 'array'
     or jsonb_typeof(coalesce(p_payload->'adjustments','[]'::jsonb)) is distinct from 'array'
     or p_payload->>'clientRequestId' is distinct from p_operation_id::text
     or nullif(p_payload->>'subjectId','') is null or nullif(p_payload->>'workDate','') is null
     or nullif(p_payload->>'batchId','') is null then
    raise exception 'VALIDATION_ERROR: invalid actual adjustment batch identity';
  end if;
  if jsonb_array_length(coalesce(p_payload->'items','[]'::jsonb))=0
     and jsonb_array_length(coalesce(p_payload->'newTasks','[]'::jsonb))=0 then
    raise exception 'VALIDATION_ERROR: actual adjustment batch cannot be empty';
  end if;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text || ':' || (p_payload->>'subjectId') || ':' || (p_payload->>'workDate'),0));
  if not exists(select 1 from timegenie.subjects where workspace_id=p_workspace_id and id=(p_payload->>'subjectId')::uuid and deleted_at is null) then
    raise exception 'NOT_FOUND: subject does not exist';
  end if;
  if exists(select 1 from jsonb_array_elements(coalesce(p_payload->'items','[]'::jsonb)) value group by value->>'taskId' having count(*)>1)
     or exists(select 1 from jsonb_array_elements(coalesce(p_payload->'newTasks','[]'::jsonb)) value group by value->>'clientTaskId' having nullif(value->>'clientTaskId','') is null or count(*)>1) then
    raise exception 'VALIDATION_ERROR: duplicate actual adjustment item identity';
  end if;

  for item in select value from jsonb_array_elements(coalesce(p_payload->'items','[]'::jsonb)) loop
    select * into current_task from timegenie.tasks where workspace_id=p_workspace_id and id=(item->>'taskId')::uuid for update;
    if not found or current_task.deleted_at is not null or current_task.subject_id<>(p_payload->>'subjectId')::uuid
       or exists(select 1 from timegenie.tasks child where child.parent_id=current_task.id and child.deleted_at is null) then
      raise exception 'VERSION_CONFLICT: actual adjustment task structure changed';
    end if;
    if current_task.version is distinct from (item->>'expectedTaskVersion')::bigint then raise exception 'VERSION_CONFLICT: actual adjustment task version changed'; end if;
    if (item->>'targetMinutes')::integer not between 0 and 1440 or (item->>'baselineMinutes')::integer<0
       or (item->>'targetMinutes')::integer-(item->>'baselineMinutes')::integer<>(item->>'deltaMinutes')::integer then
      raise exception 'VALIDATION_ERROR: actual adjustment minutes are inconsistent';
    end if;
    if (item->>'deltaMinutes')::integer<>0 then
      expected_adjustment_count:=expected_adjustment_count+1;
      select count(*) into matching_count from jsonb_array_elements(coalesce(p_payload->'adjustments','[]'::jsonb)) image
      where image->>'task_id'=item->>'taskId' and (image->>'delta_minutes')::integer=(item->>'deltaMinutes')::integer
        and (image->>'baseline_minutes')::integer=(item->>'baselineMinutes')::integer and (image->>'target_minutes')::integer=(item->>'targetMinutes')::integer;
      if matching_count<>1 then raise exception 'VALIDATION_ERROR: existing task adjustment image does not match request'; end if;
    end if;
  end loop;

  for draft in select value from jsonb_array_elements(coalesce(p_payload->'newTasks','[]'::jsonb)) loop
    if nullif(draft->>'clientTaskId','') is null or nullif(btrim(draft->>'title'),'') is null or (draft->>'targetMinutes')::integer not between 0 and 1440 then
      raise exception 'VALIDATION_ERROR: invalid new task draft';
    end if;
    if nullif(draft->>'parentId','') is not null then
      select * into current_task from timegenie.tasks where workspace_id=p_workspace_id and id=(draft->>'parentId')::uuid for update;
      if not found or current_task.deleted_at is not null or current_task.subject_id<>(p_payload->>'subjectId')::uuid or current_task.parent_id is not null or current_task.status<>'open'
         or exists(select 1 from timegenie.task_recurrence_rules rule where rule.task_id=current_task.id and rule.effective_end is null)
         or exists(select 1 from timegenie.time_allocations allocation join timegenie.time_entries entry on entry.id=allocation.entry_id where allocation.task_id=current_task.id and entry.deleted_at is null)
         or exists(select 1 from timegenie.actual_time_adjustments existing where existing.task_id=current_task.id and existing.deleted_at is null) then
        raise exception 'VERSION_CONFLICT: new task parent is no longer eligible';
      end if;
      if current_task.version is distinct from (draft->>'expectedParentVersion')::bigint then raise exception 'VERSION_CONFLICT: new task parent version changed'; end if;
    elsif draft->'expectedParentVersion' is not null and draft->'expectedParentVersion'<>'null'::jsonb then
      raise exception 'VALIDATION_ERROR: root task draft must not contain parent version';
    end if;
    if (draft->>'targetMinutes')::integer>0 then expected_adjustment_count:=expected_adjustment_count+1; end if;
  end loop;

  if jsonb_array_length(coalesce(p_payload->'createdTasks','[]'::jsonb))<>jsonb_array_length(coalesce(p_payload->'newTasks','[]'::jsonb))
     or jsonb_array_length(coalesce(p_payload->'adjustments','[]'::jsonb))<>expected_adjustment_count
     or exists(select 1 from jsonb_array_elements(coalesce(p_payload->'createdTasks','[]'::jsonb)) value group by value->>'clientTaskId' having nullif(value->>'clientTaskId','') is null or count(*)>1)
     or exists(select 1 from jsonb_array_elements(coalesce(p_payload->'createdTasks','[]'::jsonb)) value group by value->'task'->>'id' having nullif(value->'task'->>'id','') is null or count(*)>1)
     or exists(select 1 from jsonb_array_elements(coalesce(p_payload->'adjustments','[]'::jsonb)) value group by value->>'id' having nullif(value->>'id','') is null or count(*)>1) then
    raise exception 'VALIDATION_ERROR: actual adjustment batch images do not match request';
  end if;

  for draft in select value from jsonb_array_elements(coalesce(p_payload->'newTasks','[]'::jsonb)) loop
    select value into created_wrapper from jsonb_array_elements(coalesce(p_payload->'createdTasks','[]'::jsonb)) value where value->>'clientTaskId'=draft->>'clientTaskId';
    if created_wrapper is null then raise exception 'VALIDATION_ERROR: new task image is missing'; end if;
    task_image:=created_wrapper->'task';
    if jsonb_typeof(task_image) is distinct from 'object' or (task_image->>'subject_id')::uuid<>(p_payload->>'subjectId')::uuid
       or nullif(task_image->>'parent_id','')::uuid is distinct from nullif(draft->>'parentId','')::uuid or task_image->>'title' is distinct from btrim(draft->>'title')
       or task_image->>'status' is distinct from 'open' or nullif(task_image->>'planned_date','')::date is distinct from (p_payload->>'workDate')::date
       or nullif(task_image->>'planned_time','') is not null or nullif(task_image->>'estimate_minutes','') is not null
       or task_image->>'source_type' is distinct from 'manual' or task_image->>'source_ref' is distinct from 'actual_adjustment_quick_create'
       or coalesce((task_image->>'version')::bigint,0)<>1 or task_image->>'deleted_at' is not null then
      raise exception 'VALIDATION_ERROR: new task image does not match draft';
    end if;
    if (draft->>'targetMinutes')::integer>0 then
      select count(*) into matching_count from jsonb_array_elements(coalesce(p_payload->'adjustments','[]'::jsonb)) image
      where image->>'task_id'=task_image->>'id' and (image->>'delta_minutes')::integer=(draft->>'targetMinutes')::integer
        and (image->>'baseline_minutes')::integer=0 and (image->>'target_minutes')::integer=(draft->>'targetMinutes')::integer;
      if matching_count<>1 then raise exception 'VALIDATION_ERROR: new task adjustment image does not match draft'; end if;
    end if;
    insert into timegenie.tasks(id,workspace_id,subject_id,parent_id,title,status,planned_date,planned_time,estimate_minutes,note,project_name,solution_name,source_type,source_ref,sort_order,completed_at,created_at,updated_at,version,created_by_device_id,updated_by_device_id,deleted_at)
    values((task_image->>'id')::uuid,p_workspace_id,(task_image->>'subject_id')::uuid,nullif(task_image->>'parent_id','')::uuid,task_image->>'title',task_image->>'status',nullif(task_image->>'planned_date','')::date,nullif(task_image->>'planned_time','')::time,nullif(task_image->>'estimate_minutes','')::integer,task_image->>'note',task_image->>'project_name',task_image->>'solution_name','manual',task_image->>'source_ref',coalesce((task_image->>'sort_order')::integer,0),null,to_timestamp((task_image->>'created_at')::double precision/1000),to_timestamp((task_image->>'updated_at')::double precision/1000),1,p_device_id,p_device_id,null);
  end loop;

  for adjustment_image in select value from jsonb_array_elements(coalesce(p_payload->'adjustments','[]'::jsonb)) loop
    if (adjustment_image->>'subject_id')::uuid<>(p_payload->>'subjectId')::uuid or (adjustment_image->>'work_date')::date<>(p_payload->>'workDate')::date
       or (adjustment_image->>'batch_id')::uuid<>(p_payload->>'batchId')::uuid or adjustment_image->>'source_type' is distinct from 'quick_actual_adjustment'
       or coalesce((adjustment_image->>'version')::bigint,0)<>1 or adjustment_image->>'deleted_at' is not null
       or coalesce(nullif(adjustment_image->>'note',''),'') is distinct from coalesce(nullif(btrim(p_payload->>'note'),''),'') then
      raise exception 'VALIDATION_ERROR: actual adjustment image has invalid batch metadata';
    end if;
    select count(*) into matching_count from jsonb_array_elements(coalesce(p_payload->'items','[]'::jsonb)) expected
    where (expected->>'deltaMinutes')::integer<>0 and expected->>'taskId'=adjustment_image->>'task_id'
      and (expected->>'deltaMinutes')::integer=(adjustment_image->>'delta_minutes')::integer
      and (expected->>'baselineMinutes')::integer=(adjustment_image->>'baseline_minutes')::integer
      and (expected->>'targetMinutes')::integer=(adjustment_image->>'target_minutes')::integer;
    select matching_count+count(*) into matching_count
    from jsonb_array_elements(coalesce(p_payload->'newTasks','[]'::jsonb)) expected
    join lateral (select value->'task' task from jsonb_array_elements(coalesce(p_payload->'createdTasks','[]'::jsonb)) value where value->>'clientTaskId'=expected->>'clientTaskId') created on true
    where (expected->>'targetMinutes')::integer>0 and created.task->>'id'=adjustment_image->>'task_id'
      and (adjustment_image->>'delta_minutes')::integer=(expected->>'targetMinutes')::integer and (adjustment_image->>'baseline_minutes')::integer=0
      and (adjustment_image->>'target_minutes')::integer=(expected->>'targetMinutes')::integer;
    if matching_count<>1 then raise exception 'VALIDATION_ERROR: actual adjustment image does not match request'; end if;
    item:=null;
    select value into item from jsonb_array_elements(coalesce(p_payload->'items','[]'::jsonb)) value
    where value->>'taskId'=adjustment_image->>'task_id';
    if item is not null then
      select coalesce(sum(a.minutes),0)+coalesce((select sum(existing.delta_minutes) from timegenie.actual_time_adjustments existing where existing.workspace_id=p_workspace_id and existing.task_id=(adjustment_image->>'task_id')::uuid and existing.work_date=(p_payload->>'workDate')::date and existing.deleted_at is null),0)
      into current_actual from timegenie.time_allocations a join timegenie.time_entries e on e.id=a.entry_id
      where a.workspace_id=p_workspace_id and a.task_id=(adjustment_image->>'task_id')::uuid and e.work_date=(p_payload->>'workDate')::date and e.deleted_at is null;
      effective_delta:=(item->>'targetMinutes')::integer-current_actual;
      if effective_delta=0 then
        discarded_adjustment_ids:=discarded_adjustment_ids||jsonb_build_array(adjustment_image->>'id');
        continue;
      end if;
      adjustment_image:=adjustment_image||jsonb_build_object('delta_minutes',effective_delta,'baseline_minutes',current_actual,'target_minutes',(item->>'targetMinutes')::integer);
    end if;
    insert into timegenie.actual_time_adjustments(id,workspace_id,subject_id,task_id,work_date,delta_minutes,baseline_minutes,target_minutes,note,source_type,batch_id,created_at,updated_at,version,created_by_device_id,updated_by_device_id,deleted_at)
    values((adjustment_image->>'id')::uuid,p_workspace_id,(adjustment_image->>'subject_id')::uuid,(adjustment_image->>'task_id')::uuid,(adjustment_image->>'work_date')::date,(adjustment_image->>'delta_minutes')::integer,(adjustment_image->>'baseline_minutes')::integer,(adjustment_image->>'target_minutes')::integer,nullif(adjustment_image->>'note',''),'quick_actual_adjustment',(adjustment_image->>'batch_id')::uuid,to_timestamp((adjustment_image->>'created_at')::double precision/1000),to_timestamp((adjustment_image->>'updated_at')::double precision/1000),1,p_device_id,p_device_id,null);
    applied_adjustments:=applied_adjustments||jsonb_build_array(adjustment_image);
  end loop;
  result_value:=jsonb_build_object('batchId',p_payload->>'batchId','createdTaskCount',jsonb_array_length(coalesce(p_payload->'createdTasks','[]'::jsonb)),'createdAdjustmentCount',jsonb_array_length(applied_adjustments),'adjustments',applied_adjustments,'discardedAdjustmentIds',discarded_adjustment_ids);
  perform timegenie.record_processed_operation(p_workspace_id,p_operation_id,p_device_id,'actual_adjustment_batch_apply',request_value,result_value);
  return result_value;
end $$;

revoke execute on function timegenie.actual_adjustment_batch_apply(uuid,uuid,uuid,jsonb) from public,anon;
grant execute on function timegenie.actual_adjustment_batch_apply(uuid,uuid,uuid,jsonb) to authenticated;

commit;

notify pgrst, 'reload schema';
