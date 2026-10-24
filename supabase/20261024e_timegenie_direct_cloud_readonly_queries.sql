-- Direct-cloud query RPCs may run inside PostgREST read-only transactions.
-- Authorization only inspects the workspace and device, so it must not acquire
-- a row lock that PostgreSQL rejects in those transactions.

begin;

create or replace function timegenie.assert_direct_v2_workspace(
  p_workspace_id uuid,
  p_device_id uuid,
  p_expected_epoch bigint default null
) returns void language plpgsql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare workspace_row timegenie.workspaces;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  select workspace.* into workspace_row
  from timegenie.workspaces workspace
  where workspace.id=p_workspace_id and workspace.deleted_at is null;
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

revoke all on function timegenie.assert_direct_v2_workspace(uuid,uuid,bigint) from public,anon,authenticated;

notify pgrst, 'reload schema';
commit;
