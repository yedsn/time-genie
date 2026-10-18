-- Let an explicit user timer write preempt the background tracking collector.
-- Ordinary lease acquisition remains non-preemptive.

begin;

create or replace function timegenie.tracking_lease_takeover_for_timer_write(
  p_workspace_id uuid,p_device_id uuid,p_lease_token uuid
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog
as $$
declare current_lease timegenie.tracking_leases%rowtype; observed_at timestamptz:=clock_timestamp();
declare previous_holder uuid; authoritative_timer jsonb; holder_changed boolean:=false;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':tracking-lease',0));
  select * into current_lease from timegenie.tracking_leases where workspace_id=p_workspace_id for update;
  if found then
    previous_holder:=current_lease.holder_device_id;
    holder_changed:=current_lease.holder_device_id is not null and current_lease.holder_device_id<>p_device_id;
  end if;
  insert into timegenie.tracking_leases(
    workspace_id,holder_device_id,lease_token,expires_at,last_confirmed_at,updated_at,version
  ) values(
    p_workspace_id,p_device_id,p_lease_token,observed_at+interval '45 seconds',observed_at,observed_at,1
  ) on conflict(workspace_id) do update set
    holder_device_id=excluded.holder_device_id,lease_token=excluded.lease_token,
    expires_at=excluded.expires_at,last_confirmed_at=excluded.last_confirmed_at,
    updated_at=excluded.updated_at,version=timegenie.tracking_leases.version+1
  returning * into current_lease;
  if holder_changed then
    select timegenie.cloud_incremental_entity_get(p_workspace_id,'time_entries',entry.id::text)->'data'
      into authoritative_timer
    from timegenie.time_entries entry
    where entry.workspace_id=p_workspace_id and entry.source_type='timer'
      and entry.state in ('running','paused') and entry.deleted_at is null
    order by entry.started_at desc limit 1;
  end if;
  return jsonb_build_object(
    'acquired',true,'leaseToken',p_lease_token,'expiresAt',current_lease.expires_at,
    'version',current_lease.version,'takeover',holder_changed,
    'previousHolderDeviceId',previous_holder,'authoritativeTimer',authoritative_timer
  );
end $$;

revoke execute on function timegenie.tracking_lease_takeover_for_timer_write(uuid,uuid,uuid) from public,anon;
grant execute on function timegenie.tracking_lease_takeover_for_timer_write(uuid,uuid,uuid) to authenticated;

commit;
notify pgrst, 'reload schema';
