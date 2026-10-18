-- TimeGenie workspace restore no longer requires users to resolve the current
-- unassigned-time session. The pre-restore safety snapshot preserves it, then
-- the target snapshot replaces it together with the rest of the business data.

begin;

create or replace function timegenie.workspace_restore_prepare(p_workspace_id uuid,p_device_id uuid,p_target_snapshot_id uuid,p_expected_epoch bigint) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
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
    safety_result:=timegenie.workspace_snapshot_create(p_workspace_id,p_device_id,gen_random_uuid(),'pre_restore','回滚前安全镜像','系统在云端回滚前自动创建',false);
    safety_id:=(safety_result->>'id')::uuid;
    if safety_result->>'status'<>'ready' then raise exception 'RESTORE_SAFETY_SNAPSHOT_FAILED: %',coalesce(safety_result->>'failureReason','unknown'); end if;
    update timegenie.workspace_restore_locks set safety_snapshot_id=safety_id,status='prepared',updated_at=now() where workspace_id=p_workspace_id and lock_token=token_value;
    perform set_config('timegenie.restore_lock_token','',true);
  exception when others then
    perform set_config('timegenie.restore_lock_token','',true);
    delete from timegenie.workspace_restore_locks where workspace_id=p_workspace_id and lock_token=token_value;
    raise;
  end;
  return jsonb_build_object('lockToken',token_value,'targetSnapshotId',p_target_snapshot_id,'targetContentHash',target_hash,'preparedEpoch',current_epoch,'safetySnapshotId',safety_id,'expiresAt',now()+interval '10 minutes','preview',preview_value);
end $$;

revoke execute on function timegenie.workspace_restore_prepare(uuid,uuid,uuid,bigint) from public,anon;
grant execute on function timegenie.workspace_restore_prepare(uuid,uuid,uuid,bigint) to authenticated;

notify pgrst,'reload schema';

commit;
