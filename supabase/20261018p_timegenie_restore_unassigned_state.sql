-- A restored collecting unassigned session must not resume collecting, but
-- `paused` is not a valid unassigned-session state. Move it to the existing
-- awaiting-resolution state and close any open segment at its start boundary.

begin;

create or replace function timegenie.workspace_restore_commit(p_workspace_id uuid,p_device_id uuid,p_lock_token uuid,p_target_snapshot_id uuid,p_target_content_hash text,p_prepared_epoch bigint,p_operation_id uuid,p_confirmed boolean) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog set statement_timeout='600s' as $$
declare lock_value timegenie.workspace_restore_locks; target jsonb; group_name_value text; c record; rows_value jsonb; row_value jsonb; current_epoch bigint; new_epoch bigint; result_value jsonb; cached jsonb; reset_seq bigint;
begin
  if not p_confirmed then raise exception 'RESTORE_CONFIRMATION_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  cached:=timegenie.processed_operation_result(p_workspace_id,p_operation_id,'workspace.restore',jsonb_build_object('entityType','workspace','entityId',p_workspace_id,'snapshotId',p_target_snapshot_id,'targetHash',p_target_content_hash,'preparedEpoch',p_prepared_epoch));
  if cached is not null then return cached; end if;
  select * into lock_value from timegenie.workspace_restore_locks where workspace_id=p_workspace_id and lock_token=p_lock_token and holder_device_id=p_device_id and status='prepared' and expires_at>now() for update;
  if not found then raise exception 'RESTORE_LOCK_INVALID_OR_EXPIRED'; end if;
  if lock_value.target_snapshot_id<>p_target_snapshot_id or lock_value.target_content_hash<>p_target_content_hash or lock_value.prepared_epoch<>p_prepared_epoch then raise exception 'RESTORE_PREPARE_MISMATCH'; end if;
  select workspace_epoch into current_epoch from timegenie.workspaces where id=p_workspace_id for update;
  if current_epoch<>p_prepared_epoch then raise exception 'WORKSPACE_EPOCH_STALE: client=%, server=%',p_prepared_epoch,current_epoch; end if;
  target:=timegenie.workspace_snapshot_rehydrate(p_target_snapshot_id);
  if timegenie.workspace_snapshot_hash(target)<>p_target_content_hash then raise exception 'SNAPSHOT_CHECKSUM_FAILED'; end if;
  update timegenie.workspace_restore_locks set status='committing',updated_at=now() where workspace_id=p_workspace_id;
  perform set_config('timegenie.restore_lock_token',p_lock_token::text,true);
  perform set_config(
    'request.headers',
    (coalesce(nullif(current_setting('request.headers',true),'')::jsonb,'{}'::jsonb)
      || jsonb_build_object('x-timegenie-workspace-epoch',current_epoch::text))::text,
    true
  );
  delete from timegenie.report_tasks where workspace_id=p_workspace_id;
  delete from timegenie.reports where workspace_id=p_workspace_id;
  delete from timegenie.actual_time_adjustments where workspace_id=p_workspace_id;
  delete from timegenie.time_allocations where workspace_id=p_workspace_id;
  delete from timegenie.time_segments where workspace_id=p_workspace_id;
  update timegenie.unassigned_sessions set generated_entry_id=null where workspace_id=p_workspace_id;
  delete from timegenie.unassigned_segments where workspace_id=p_workspace_id;
  delete from timegenie.unassigned_sessions where workspace_id=p_workspace_id;
  delete from timegenie.time_entries where workspace_id=p_workspace_id;
  delete from timegenie.task_status_events where workspace_id=p_workspace_id;
  delete from timegenie.task_day_plans where workspace_id=p_workspace_id;
  delete from timegenie.task_daily_estimates where workspace_id=p_workspace_id;
  delete from timegenie.task_occurrences where workspace_id=p_workspace_id;
  delete from timegenie.task_recurrence_rules where workspace_id=p_workspace_id;
  delete from timegenie.report_templates where workspace_id=p_workspace_id;
  delete from timegenie.tasks where workspace_id=p_workspace_id;
  delete from timegenie.subject_work_schedule_segments where workspace_id=p_workspace_id;
  delete from timegenie.subject_work_schedules where workspace_id=p_workspace_id;
  delete from timegenie.work_days where workspace_id=p_workspace_id;
  delete from timegenie.external_bindings where workspace_id=p_workspace_id;
  delete from timegenie.integration_configs where workspace_id=p_workspace_id;
  delete from timegenie.app_settings where workspace_id=p_workspace_id;
  delete from timegenie.subjects where workspace_id=p_workspace_id;
  foreach group_name_value in array array['planning','time_tracking','reports','shared_settings'] loop
    for c in select * from timegenie.workspace_snapshot_entity_catalog() catalog where catalog.group_name=group_name_value order by sort_order loop
      rows_value:=coalesce(target->group_name_value->c.entity_name,'[]'::jsonb);
      for row_value in select value from jsonb_array_elements(rows_value) loop
        execute format('insert into timegenie.%I select * from jsonb_populate_record(null::timegenie.%I,$1)',c.entity_name,c.entity_name) using row_value;
      end loop;
    end loop;
  end loop;
  update timegenie.time_entries set state='paused',ended_at=null where workspace_id=p_workspace_id and state='running';
  update timegenie.time_segments set ended_at=coalesce(ended_at,started_at),duration_seconds=coalesce(duration_seconds,0) where workspace_id=p_workspace_id and ended_at is null;
  update timegenie.unassigned_segments set ended_at=coalesce(ended_at,started_at),duration_seconds=coalesce(duration_seconds,0),lease_token=null where workspace_id=p_workspace_id and ended_at is null;
  update timegenie.unassigned_sessions set state='awaiting_resolution',last_ended_at=coalesce(last_ended_at,last_continuous_at,first_started_at),prompted_at=coalesce(prompted_at,clock_timestamp()),updated_at=clock_timestamp() where workspace_id=p_workspace_id and state='collecting';
  new_epoch:=current_epoch+1;
  update timegenie.workspaces set workspace_epoch=new_epoch,updated_at=now() where id=p_workspace_id;
  insert into timegenie.workspace_changes(workspace_id,entity_type,entity_id,operation,entity_version,changed_by_device_id) values(p_workspace_id,'workspace_reset',p_workspace_id::text,'update',new_epoch,p_device_id) returning change_seq into reset_seq;
  result_value:=jsonb_build_object('restored',true,'targetSnapshotId',p_target_snapshot_id,'safetySnapshotId',lock_value.safety_snapshot_id,'previousEpoch',current_epoch,'workspaceEpoch',new_epoch,'resetChangeSeq',reset_seq);
  perform timegenie.record_processed_operation(p_workspace_id,p_operation_id,p_device_id,'workspace.restore',jsonb_build_object('entityType','workspace','entityId',p_workspace_id,'snapshotId',p_target_snapshot_id,'targetHash',p_target_content_hash,'preparedEpoch',p_prepared_epoch),result_value);
  delete from timegenie.workspace_restore_locks where workspace_id=p_workspace_id and lock_token=p_lock_token;
  return result_value;
end $$;

revoke execute on function timegenie.workspace_restore_commit(uuid,uuid,uuid,uuid,text,bigint,uuid,boolean) from public,anon;
grant execute on function timegenie.workspace_restore_commit(uuid,uuid,uuid,uuid,text,bigint,uuid,boolean) to authenticated;

notify pgrst,'reload schema';

commit;
