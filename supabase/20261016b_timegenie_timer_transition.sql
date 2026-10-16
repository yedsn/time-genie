begin;

-- Apply an inline timer create/start or switch as one idempotent transaction.
create or replace function timegenie.timer_transition_apply(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_payload jsonb
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog
as $$
declare cached jsonb; request_value jsonb; previous_row jsonb; task_row jsonb; active_row jsonb;
declare remote_active timegenie.time_entries; transition_at timestamptz; result_value jsonb;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  request_value:=jsonb_build_object('deviceId',p_device_id,'payload',coalesce(p_payload,'{}'::jsonb));
  cached:=timegenie.processed_operation_result(p_workspace_id,p_operation_id,'timer_transition_apply',request_value);
  if cached is not null then return cached; end if;
  previous_row:=p_payload->'previous_entry'; task_row:=p_payload->'task'; active_row:=p_payload->'active_entry';
  if active_row is null or jsonb_typeof(active_row)<>'object' then raise exception 'VALIDATION_ERROR: missing active timer'; end if;
  transition_at:=to_timestamp((active_row->>'started_at')::double precision/1000);
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':active-timer',0));
  select * into remote_active from timegenie.time_entries where workspace_id=p_workspace_id and state in ('running','paused') and deleted_at is null limit 1 for update;
  if previous_row is null or previous_row='null'::jsonb then
    if found then raise exception 'VERSION_CONFLICT: remote timer is active'; end if;
  else
    if not found or remote_active.id<>(previous_row->>'id')::uuid then raise exception 'VERSION_CONFLICT: remote timer changed'; end if;
    if remote_active.version<>coalesce((p_payload->>'previous_base_version')::bigint,-1) then raise exception 'VERSION_CONFLICT: remote timer version changed'; end if;
    if to_timestamp((previous_row->>'ended_at')::double precision/1000)<>transition_at then raise exception 'VALIDATION_ERROR: transition boundary mismatch'; end if;
    perform timegenie.apply_calendar_time_entry_pre_simplified_anchor(
      p_workspace_id,p_device_id,timegenie.calendar_stable_uuid('timer-transition-stop:'||p_operation_id),
      'timer_transition_stop',previous_row->>'id',(p_payload->>'previous_base_version')::bigint,previous_row,
      jsonb_build_object('deviceId',p_device_id,'transitionId',p_operation_id,'phase','stop','payload',previous_row));
  end if;
  if task_row is not null and task_row<>'null'::jsonb then
    if nullif(task_row->>'parent_id','') is not null and not exists(
      select 1 from timegenie.tasks parent where parent.workspace_id=p_workspace_id and parent.id=(task_row->>'parent_id')::uuid
        and parent.subject_id=(task_row->>'subject_id')::uuid and parent.deleted_at is null and parent.status='open' and parent.parent_id is null
        and parent.version=coalesce((p_payload->>'expected_parent_version')::bigint,-1)
        and not exists(select 1 from timegenie.task_recurrence_rules rule where rule.task_id=parent.id and rule.effective_end is null)
        and not exists(
          select 1 from timegenie.time_allocations allocation
          join timegenie.time_entries entry on entry.id=allocation.entry_id
          where allocation.task_id=parent.id and entry.workspace_id=p_workspace_id and entry.deleted_at is null
        )
        and not exists(select 1 from timegenie.actual_time_adjustments adjustment where adjustment.task_id=parent.id and adjustment.workspace_id=p_workspace_id and adjustment.deleted_at is null)
    ) then raise exception 'VERSION_CONFLICT: parent task changed'; end if;
    perform timegenie.cloud_apply_patch_pre_calendar(
      p_workspace_id,p_device_id,timegenie.calendar_stable_uuid('timer-transition-task:'||p_operation_id),
      'task_create_from_timer_transition','task',task_row->>'id',null,task_row);
  end if;
  if not exists(select 1 from timegenie.tasks target where target.workspace_id=p_workspace_id and target.id=(active_row->>'default_task_id')::uuid
    and target.deleted_at is null and not exists(select 1 from timegenie.tasks child where child.parent_id=target.id and child.deleted_at is null)) then
    raise exception 'TASK_NOT_SELECTABLE';
  end if;
  perform timegenie.apply_calendar_time_entry_pre_simplified_anchor(
    p_workspace_id,p_device_id,timegenie.calendar_stable_uuid('timer-transition-start:'||p_operation_id),
    'timer_transition_start',active_row->>'id',null,active_row,
    jsonb_build_object('deviceId',p_device_id,'transitionId',p_operation_id,'phase','start','payload',active_row));
  perform timegenie.timegenie_unassigned_system_clear(p_workspace_id,transition_at);
  result_value:=jsonb_build_object('accepted',true,'activeEntry',timegenie.cloud_incremental_entity_get(p_workspace_id,'time_entries',active_row->>'id')->'data',
    'previousEntry',case when previous_row is null or previous_row='null'::jsonb then null else timegenie.cloud_incremental_entity_get(p_workspace_id,'time_entries',previous_row->>'id')->'data' end,
    'task',case when task_row is null or task_row='null'::jsonb then null else timegenie.cloud_incremental_entity_get(p_workspace_id,'tasks',task_row->>'id')->'data' end);
  perform timegenie.record_processed_operation(p_workspace_id,p_operation_id,p_device_id,'timer_transition_apply',request_value,result_value);
  return result_value;
end $$;

revoke execute on function timegenie.timer_transition_apply(uuid,uuid,uuid,jsonb) from public,anon;
grant execute on function timegenie.timer_transition_apply(uuid,uuid,uuid,jsonb) to authenticated;

commit;
notify pgrst, 'reload schema';
