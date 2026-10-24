-- Retire the SQLite-to-Supabase synchronization protocol. Supabase is the
-- only cloud authority after this migration; the remaining legacy signatures
-- are rejection-only tombstones for older clients.

update timegenie.workspaces
set storage_protocol='direct_v2',updated_at=now()
where storage_protocol is distinct from 'direct_v2';

alter table timegenie.workspaces
  alter column storage_protocol set default 'direct_v2';
alter table timegenie.workspaces
  drop constraint if exists workspaces_storage_protocol_check;
alter table timegenie.workspaces
  add constraint workspaces_storage_protocol_check
  check (storage_protocol='direct_v2');

create or replace function timegenie.assert_sync_v1_workspace(p_workspace_id uuid)
returns void language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then
    raise exception 'AUTH_REQUIRED';
  end if;
  if not exists(
    select 1 from timegenie.workspaces workspace
    where workspace.id=p_workspace_id and workspace.deleted_at is null
  ) then
    raise exception 'WORKSPACE_NOT_FOUND';
  end if;
  raise exception 'LEGACY_SYNC_UNSUPPORTED: 旧同步客户端已停止支持，请升级 TimeGenie';
end $$;

create or replace function timegenie.cloud_apply_patch(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_operation_type text,
  p_entity_type text,p_entity_id text,p_base_version bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
begin
  perform timegenie.assert_sync_v1_workspace(p_workspace_id);
  raise exception 'LEGACY_SYNC_UNSUPPORTED: 旧同步客户端已停止支持，请升级 TimeGenie';
end $$;

create or replace function timegenie.cloud_apply_patch(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_operation_type text,
  p_entity_type text,p_entity_id text,p_base_version bigint,p_payload_version bigint,
  p_coalesced_count bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
begin
  perform timegenie.assert_sync_v1_workspace(p_workspace_id);
  raise exception 'LEGACY_SYNC_UNSUPPORTED: 旧同步客户端已停止支持，请升级 TimeGenie';
end $$;

create or replace function timegenie.cloud_incremental_pull(
  p_workspace_id uuid,p_device_id uuid,p_after_change_seq bigint default 0,
  p_limit integer default 200
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
begin
  perform timegenie.assert_sync_v1_workspace(p_workspace_id);
  raise exception 'LEGACY_SYNC_UNSUPPORTED: 旧同步客户端已停止支持，请升级 TimeGenie';
end $$;

create or replace function timegenie.storage_protocol_upgrade_preflight(
  p_workspace_id uuid,p_device_id uuid
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  raise exception 'LEGACY_SYNC_UNSUPPORTED: 云端工作区已统一使用直连存储';
end $$;

create or replace function timegenie.upgrade_storage_protocol(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,
  p_old_sync_disposition text,p_create_safety_snapshot boolean default true
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  raise exception 'LEGACY_SYNC_UNSUPPORTED: 云端工作区已统一使用直连存储';
end $$;

-- Older direct-v2 clients may still call this after creating a workspace. It
-- is now an idempotent compatibility read and never changes workspace data.
create or replace function timegenie.provision_direct_v2_workspace(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare workspace_row timegenie.workspaces;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,null);
  select workspace.* into workspace_row
  from timegenie.workspaces workspace
  where workspace.id=p_workspace_id and workspace.deleted_at is null;
  return jsonb_build_object(
    'provisioned',false,'alreadyDirect',true,
    'workspaceEpoch',workspace_row.workspace_epoch,
    'storageProtocol','direct_v2','serverTime',now()
  );
end $$;

revoke all on function timegenie.assert_sync_v1_workspace(uuid) from public,anon,authenticated;
revoke all on function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,jsonb) from public,anon,authenticated;
revoke all on function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,bigint,bigint,jsonb) from public,anon,authenticated;
revoke all on function timegenie.cloud_incremental_pull(uuid,uuid,bigint,integer) from public,anon,authenticated;
revoke all on function timegenie.storage_protocol_upgrade_preflight(uuid,uuid) from public,anon,authenticated;
revoke all on function timegenie.upgrade_storage_protocol(uuid,uuid,uuid,bigint,text,boolean) from public,anon,authenticated;
revoke all on function timegenie.provision_direct_v2_workspace(uuid,uuid,uuid) from public,anon,authenticated;

-- Authenticated callers retain only the rejection-only compatibility
-- signatures. The real pre-direct implementations remain private.
grant execute on function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,jsonb) to authenticated;
grant execute on function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,bigint,bigint,jsonb) to authenticated;
grant execute on function timegenie.cloud_incremental_pull(uuid,uuid,bigint,integer) to authenticated;
grant execute on function timegenie.storage_protocol_upgrade_preflight(uuid,uuid) to authenticated;
grant execute on function timegenie.upgrade_storage_protocol(uuid,uuid,uuid,bigint,text,boolean) to authenticated;
grant execute on function timegenie.provision_direct_v2_workspace(uuid,uuid,uuid) to authenticated;

revoke execute on function timegenie.cloud_apply_patch_pre_direct_v2(uuid,uuid,uuid,text,text,text,bigint,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.cloud_apply_patch_pre_direct_v2(uuid,uuid,uuid,text,text,text,bigint,bigint,bigint,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.cloud_incremental_pull_pre_direct_v2(uuid,uuid,bigint,integer) from public,anon,authenticated;

notify pgrst, 'reload schema';
