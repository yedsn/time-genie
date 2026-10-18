-- Preparing a restore only locks and verifies the target by default. A
-- pre-restore safety snapshot is created only when the user explicitly opts in
-- from the final confirmation dialog.

begin;

create or replace function timegenie.workspace_restore_prepare(
  p_workspace_id uuid,
  p_device_id uuid,
  p_target_snapshot_id uuid,
  p_expected_epoch bigint,
  p_create_safety_snapshot boolean
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog
set statement_timeout='600s' as $$
declare target_hash text; current_epoch bigint; token_value uuid:=gen_random_uuid(); safety_id uuid; preview_value jsonb; verification_value jsonb; safety_result jsonb;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  select workspace_epoch into current_epoch from timegenie.workspaces where id=p_workspace_id for update;
  if p_expected_epoch is distinct from current_epoch then raise exception 'WORKSPACE_EPOCH_STALE: client=%, server=%',coalesce(p_expected_epoch,0),current_epoch; end if;
  if exists(select 1 from timegenie.time_entries where workspace_id=p_workspace_id and state in('running','paused') and deleted_at is null) then raise exception 'RESTORE_ACTIVE_TIMER'; end if;
  delete from timegenie.workspace_restore_locks where workspace_id=p_workspace_id and expires_at<=now();
  if exists(select 1 from timegenie.workspace_restore_locks where workspace_id=p_workspace_id) then raise exception 'WORKSPACE_RESTORE_IN_PROGRESS'; end if;
  select content_hash into target_hash from timegenie.workspace_snapshots where workspace_id=p_workspace_id and id=p_target_snapshot_id and status='ready';
  if target_hash is null then raise exception 'SNAPSHOT_NOT_RESTORABLE'; end if;
  verification_value:=timegenie.workspace_snapshot_verify_integrity(p_workspace_id,p_target_snapshot_id);
  if not coalesce((verification_value->>'restorable')::boolean,false) then raise exception 'SNAPSHOT_NOT_RESTORABLE'; end if;
  preview_value:=timegenie.workspace_snapshot_preview(p_workspace_id,p_device_id,p_target_snapshot_id);
  insert into timegenie.workspace_restore_locks(workspace_id,lock_token,holder_device_id,target_snapshot_id,target_content_hash,prepared_epoch,status,expires_at)
  values(p_workspace_id,token_value,p_device_id,p_target_snapshot_id,target_hash,current_epoch,'preparing',now()+interval '10 minutes');
  begin
    perform set_config('timegenie.restore_lock_token',token_value::text,true);
    if coalesce(p_create_safety_snapshot,false) then
      safety_result:=timegenie.workspace_snapshot_create(p_workspace_id,p_device_id,gen_random_uuid(),'pre_restore','恢复前备份','用户在恢复确认时选择创建',false);
      safety_id:=(safety_result->>'id')::uuid;
      if safety_result->>'status'<>'ready' then raise exception 'RESTORE_SAFETY_SNAPSHOT_FAILED: %',coalesce(safety_result->>'failureReason','unknown'); end if;
    end if;
    update timegenie.workspace_restore_locks set safety_snapshot_id=safety_id,status='prepared',updated_at=now() where workspace_id=p_workspace_id and lock_token=token_value;
    perform set_config('timegenie.restore_lock_token','',true);
  exception when others then
    perform set_config('timegenie.restore_lock_token','',true);
    delete from timegenie.workspace_restore_locks where workspace_id=p_workspace_id and lock_token=token_value;
    raise;
  end;
  return jsonb_build_object('lockToken',token_value,'targetSnapshotId',p_target_snapshot_id,'targetContentHash',target_hash,'preparedEpoch',current_epoch,'safetySnapshotId',safety_id,'expiresAt',now()+interval '10 minutes','preview',preview_value);
end $$;

-- Keep older clients compatible, but do not create a safety snapshot unless
-- the request explicitly carries the new opt-in flag.
create or replace function timegenie.workspace_restore_prepare(
  p_workspace_id uuid,p_device_id uuid,p_target_snapshot_id uuid,p_expected_epoch bigint
) returns jsonb language sql security definer
set search_path=timegenie,extensions,pg_catalog
set statement_timeout='600s' as $$
  select timegenie.workspace_restore_prepare($1,$2,$3,$4,false);
$$;

revoke execute on function timegenie.workspace_restore_prepare(uuid,uuid,uuid,bigint,boolean) from public,anon;
revoke execute on function timegenie.workspace_restore_prepare(uuid,uuid,uuid,bigint) from public,anon;
grant execute on function timegenie.workspace_restore_prepare(uuid,uuid,uuid,bigint,boolean) to authenticated;
grant execute on function timegenie.workspace_restore_prepare(uuid,uuid,uuid,bigint) to authenticated;

notify pgrst,'reload schema';

commit;
