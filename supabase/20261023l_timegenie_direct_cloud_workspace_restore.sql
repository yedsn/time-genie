-- Allow workspace snapshot restore for direct_v2 while preserving the legacy
-- sync protocol barrier for old synchronization writes.
begin;

create or replace function timegenie.direct_cloud_workspace_restore_prepare(
  p_workspace_id uuid,p_device_id uuid,p_target_snapshot_id uuid,
  p_expected_epoch bigint,p_create_safety_snapshot boolean
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog set statement_timeout='600s' as $$
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  return timegenie.workspace_restore_prepare_pre_direct_v2(
    p_workspace_id,p_device_id,p_target_snapshot_id,p_expected_epoch,p_create_safety_snapshot
  );
end $$;

create or replace function timegenie.direct_cloud_workspace_restore_commit(
  p_workspace_id uuid,p_device_id uuid,p_lock_token uuid,p_target_snapshot_id uuid,
  p_target_content_hash text,p_prepared_epoch bigint,p_operation_id uuid,p_confirmed boolean
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog set statement_timeout='600s' as $$
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_prepared_epoch);
  return timegenie.workspace_restore_commit_pre_direct_v2(
    p_workspace_id,p_device_id,p_lock_token,p_target_snapshot_id,
    p_target_content_hash,p_prepared_epoch,p_operation_id,p_confirmed
  );
end $$;

-- Keep the legacy RPC names behind the sync_v1 barrier. Direct clients call
-- the dedicated functions above, so old clients cannot invoke restore writes.
create or replace function timegenie.workspace_restore_prepare(
  p_workspace_id uuid,p_device_id uuid,p_target_snapshot_id uuid,p_expected_epoch bigint
) returns jsonb language sql security definer set search_path=timegenie,extensions,pg_catalog as $$
  with checked as (select timegenie.assert_sync_v1_workspace(p_workspace_id))
  select timegenie.workspace_restore_prepare_pre_direct_v2(
    p_workspace_id,p_device_id,p_target_snapshot_id,p_expected_epoch
  ) from checked;
$$;

create or replace function timegenie.workspace_restore_prepare(
  p_workspace_id uuid,p_device_id uuid,p_target_snapshot_id uuid,
  p_expected_epoch bigint,p_create_safety_snapshot boolean
) returns jsonb language sql security definer set search_path=timegenie,extensions,pg_catalog as $$
  with checked as (select timegenie.assert_sync_v1_workspace(p_workspace_id))
  select timegenie.workspace_restore_prepare_pre_direct_v2(
    p_workspace_id,p_device_id,p_target_snapshot_id,p_expected_epoch,p_create_safety_snapshot
  ) from checked;
$$;

create or replace function timegenie.workspace_restore_commit(
  p_workspace_id uuid,p_device_id uuid,p_lock_token uuid,p_target_snapshot_id uuid,
  p_target_content_hash text,p_prepared_epoch bigint,p_operation_id uuid,p_confirmed boolean
) returns jsonb language sql security definer
set search_path=timegenie,extensions,pg_catalog set statement_timeout='600s' as $$
  with checked as (select timegenie.assert_sync_v1_workspace(p_workspace_id))
  select timegenie.workspace_restore_commit_pre_direct_v2(
    p_workspace_id,p_device_id,p_lock_token,p_target_snapshot_id,
    p_target_content_hash,p_prepared_epoch,p_operation_id,p_confirmed
  ) from checked;
$$;

revoke execute on function timegenie.direct_cloud_workspace_restore_prepare(uuid,uuid,uuid,bigint,boolean) from public,anon;
revoke execute on function timegenie.direct_cloud_workspace_restore_commit(uuid,uuid,uuid,uuid,text,bigint,uuid,boolean) from public,anon;
grant execute on function timegenie.direct_cloud_workspace_restore_prepare(uuid,uuid,uuid,bigint,boolean) to authenticated;
grant execute on function timegenie.direct_cloud_workspace_restore_commit(uuid,uuid,uuid,uuid,text,bigint,uuid,boolean) to authenticated;

notify pgrst,'reload schema';
commit;
