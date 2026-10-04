-- TimeGenie persistent Supabase session and device authorization patch.
-- Run after the existing TimeGenie schema. Fresh projects should run supabase/schema.sql.

alter table timegenie.devices add column if not exists authorized_at timestamptz not null default now();
alter table timegenie.devices add column if not exists reauthorized_at timestamptz;
alter table timegenie.devices add column if not exists auth_session_id text;
revoke all on timegenie.devices from authenticated;

create or replace function timegenie.current_auth_session_id() returns text
language sql stable security definer set search_path = timegenie, extensions, pg_catalog as $$
  select coalesce((nullif(current_setting('request.jwt.claims', true), '')::jsonb)->>'session_id', nullif(current_setting('request.jwt.claim.session_id', true), ''));
$$;

create or replace function timegenie.device_authorize(
  p_workspace_id uuid, p_device_id uuid, p_device_name text, p_platform text, p_app_version text
) returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare existing_device timegenie.devices; authorized_device timegenie.devices; current_session_id text;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  current_session_id := timegenie.current_auth_session_id();
  if current_session_id is null then raise exception 'AUTH_SESSION_REQUIRED'; end if;
  select * into existing_device from timegenie.devices where id = p_device_id for update;
  if found and existing_device.workspace_id <> p_workspace_id then raise exception 'AUTH_REQUIRED'; end if;
  if found and existing_device.revoked_at is not null and existing_device.auth_session_id = current_session_id then raise exception 'PASSWORD_REAUTH_REQUIRED'; end if;
  insert into timegenie.devices(id, workspace_id, device_name, platform, app_version, last_seen_at, authorized_at, reauthorized_at, auth_session_id, revoked_at)
  values(p_device_id, p_workspace_id, nullif(trim(p_device_name), ''), p_platform, p_app_version, now(), now(), null, current_session_id, null)
  on conflict(id) do update set device_name = excluded.device_name, platform = excluded.platform, app_version = excluded.app_version, last_seen_at = now(),
    reauthorized_at = case when timegenie.devices.revoked_at is not null then now() else timegenie.devices.reauthorized_at end, auth_session_id = current_session_id, revoked_at = null
  returning * into authorized_device;
  return jsonb_build_object('id', authorized_device.id, 'workspace_id', authorized_device.workspace_id, 'device_name', authorized_device.device_name, 'platform', authorized_device.platform, 'app_version', authorized_device.app_version, 'last_seen_at', authorized_device.last_seen_at, 'authorized_at', authorized_device.authorized_at, 'reauthorized_at', authorized_device.reauthorized_at, 'revoked_at', authorized_device.revoked_at);
end $$;

create or replace function timegenie.device_authorization_get(p_workspace_id uuid, p_device_id uuid) returns jsonb
language plpgsql security definer set search_path = timegenie, extensions, pg_catalog as $$
declare device_row timegenie.devices;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  select * into device_row from timegenie.devices where workspace_id = p_workspace_id and id = p_device_id;
  if not found then return jsonb_build_object('authorized', false, 'reason', 'DEVICE_NOT_REGISTERED'); end if;
  return jsonb_build_object('authorized', device_row.revoked_at is null, 'reason', case when device_row.revoked_at is null then null else 'DEVICE_REVOKED' end, 'revoked_at', device_row.revoked_at);
end $$;

create or replace function timegenie.device_list(p_workspace_id uuid) returns jsonb
language plpgsql security definer set search_path = timegenie, extensions, pg_catalog as $$
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  return coalesce((select jsonb_agg(jsonb_build_object('id', d.id, 'workspace_id', d.workspace_id, 'device_name', d.device_name, 'platform', d.platform, 'app_version', d.app_version, 'last_seen_at', d.last_seen_at, 'authorized_at', d.authorized_at, 'reauthorized_at', d.reauthorized_at, 'revoked_at', d.revoked_at) order by d.revoked_at is not null, d.last_seen_at desc) from timegenie.devices d where d.workspace_id = p_workspace_id), '[]'::jsonb);
end $$;

create or replace function timegenie.device_revoke(p_workspace_id uuid, p_device_id uuid) returns jsonb
language plpgsql security definer set search_path = timegenie, extensions, pg_catalog as $$
declare affected integer;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  update timegenie.devices set revoked_at = coalesce(revoked_at, now()) where workspace_id = p_workspace_id and id = p_device_id;
  get diagnostics affected = row_count;
  if affected = 0 then raise exception 'DEVICE_NOT_FOUND'; end if;
  update timegenie.tracking_leases set holder_device_id = null, lease_token = null, expires_at = null, updated_at = now(), version = version + 1 where workspace_id = p_workspace_id and holder_device_id = p_device_id;
  return jsonb_build_object('deviceId', p_device_id, 'revoked', true);
end $$;

create or replace function timegenie.device_revoke_all(p_workspace_id uuid) returns jsonb
language plpgsql security definer set search_path = timegenie, extensions, pg_catalog as $$
declare affected integer;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  update timegenie.devices set revoked_at = coalesce(revoked_at, now()) where workspace_id = p_workspace_id;
  get diagnostics affected = row_count;
  update timegenie.tracking_leases set holder_device_id = null, lease_token = null, expires_at = null, updated_at = now(), version = version + 1 where workspace_id = p_workspace_id;
  return jsonb_build_object('revokedCount', affected);
end $$;

revoke execute on function timegenie.device_authorize(uuid, uuid, text, text, text) from public, anon;
revoke execute on function timegenie.device_authorization_get(uuid, uuid) from public, anon;
revoke execute on function timegenie.device_list(uuid) from public, anon;
revoke execute on function timegenie.device_revoke(uuid, uuid) from public, anon;
revoke execute on function timegenie.device_revoke_all(uuid) from public, anon;
grant execute on function timegenie.device_authorize(uuid, uuid, text, text, text) to authenticated;
grant execute on function timegenie.device_authorization_get(uuid, uuid) to authenticated;
grant execute on function timegenie.device_list(uuid) to authenticated;
grant execute on function timegenie.device_revoke(uuid, uuid) to authenticated;
grant execute on function timegenie.device_revoke_all(uuid) to authenticated;
revoke execute on function timegenie.current_auth_session_id() from public, anon, authenticated;
