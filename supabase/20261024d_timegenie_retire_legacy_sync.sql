-- Final direct-cloud schema. Supabase is the only cloud authority; the old
-- SQLite synchronization protocol and its workspace generation field no
-- longer exist after this migration.

begin;

-- Direct authorization depends only on identity, device authorization and the
-- workspace epoch. There is no longer a sync-v1/direct-v2 workspace state.
create or replace function timegenie.assert_direct_v2_workspace(
  p_workspace_id uuid,
  p_device_id uuid,
  p_expected_epoch bigint default null
) returns void language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare workspace_row timegenie.workspaces;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  select workspace.* into workspace_row
  from timegenie.workspaces workspace
  where workspace.id=p_workspace_id and workspace.deleted_at is null
  for share;
  if not found then raise exception 'WORKSPACE_NOT_FOUND'; end if;
  if not exists(
    select 1 from timegenie.devices device
    where device.id=p_device_id and device.workspace_id=p_workspace_id and device.revoked_at is null
  ) then
    raise exception 'DEVICE_REVOKED';
  end if;
  if p_expected_epoch is not null and p_expected_epoch<>workspace_row.workspace_epoch then
    raise exception 'WORKSPACE_EPOCH_STALE: expected %, current %',p_expected_epoch,workspace_row.workspace_epoch;
  end if;
end $$;

create or replace function timegenie.direct_cloud_context(
  p_workspace_id uuid,
  p_device_id uuid,
  p_expected_epoch bigint default null
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare workspace_row timegenie.workspaces; latest_change_seq bigint;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  select workspace.* into workspace_row from timegenie.workspaces workspace
  where workspace.id=p_workspace_id and workspace.deleted_at is null;
  select coalesce(max(change_seq),0) into latest_change_seq
  from timegenie.workspace_changes where workspace_id=p_workspace_id;
  return jsonb_build_object(
    'workspaceId',workspace_row.id,
    'deviceId',p_device_id,
    'storageProtocol','direct_v2',
    'workspaceEpoch',workspace_row.workspace_epoch,
    'changeSeq',latest_change_seq,
    'serverTime',now(),
    'writable',not workspace_row.emergency_read_only,
    'readOnlyReason',case when workspace_row.emergency_read_only then
      coalesce(nullif(workspace_row.emergency_read_only_reason,''),'Supabase 紧急停写已启用')
      else null end
  );
end $$;

-- Remove public legacy sync and protocol-upgrade entry points. Old clients now
-- receive a normal PostgREST function-not-found response and cannot mutate
-- authoritative data through the retired protocol.
drop function if exists timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,jsonb);
drop function if exists timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,bigint,bigint,jsonb);
drop function if exists timegenie.cloud_incremental_pull(uuid,uuid,bigint,integer);
drop function if exists timegenie.storage_protocol_get(uuid);
drop function if exists timegenie.storage_protocol_upgrade_preflight(uuid,uuid);
drop function if exists timegenie.upgrade_storage_protocol(uuid,uuid,uuid,bigint,text,boolean);
drop function if exists timegenie.provision_direct_v2_workspace(uuid,uuid,uuid);
drop function if exists timegenie.assert_sync_v1_workspace(uuid);

-- These private implementations served only cloud_apply_patch/pull. Other
-- private pre-direct functions are retained because current direct timer,
-- explicit full-copy and snapshot restore RPCs use them as internal helpers.
drop function if exists timegenie.cloud_apply_patch_pre_direct_v2(uuid,uuid,uuid,text,text,text,bigint,jsonb);
drop function if exists timegenie.cloud_apply_patch_pre_direct_v2(uuid,uuid,uuid,text,text,text,bigint,bigint,bigint,jsonb);
drop function if exists timegenie.cloud_incremental_pull_pre_direct_v2(uuid,uuid,bigint,integer);

alter table timegenie.workspaces
  drop constraint if exists workspaces_storage_protocol_check;
alter table timegenie.workspaces
  drop column if exists storage_protocol;

revoke all on function timegenie.assert_direct_v2_workspace(uuid,uuid,bigint) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_context(uuid,uuid,bigint) from public,anon;
grant execute on function timegenie.direct_cloud_context(uuid,uuid,bigint) to authenticated;

notify pgrst, 'reload schema';
commit;
