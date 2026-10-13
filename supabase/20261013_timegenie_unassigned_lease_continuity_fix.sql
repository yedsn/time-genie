-- TimeGenie unassigned-anchor lease continuity fix.
-- Safe to reapply: the legacy helper remains callable but no longer mutates the
-- shared unassigned anchor when a tracking lease expires or is released.

begin;

create or replace function timegenie.close_expired_unassigned_lease(
  p_workspace_id uuid,p_close_at timestamptz
) returns void language plpgsql security definer set search_path=timegenie,pg_catalog
as $$ begin
  -- Tracking leases belong to ordinary item-timer continuity. Unassigned time
  -- is derived from its persisted anchor and must survive lease lifecycle events.
  perform p_workspace_id,p_close_at;
end $$;

revoke execute on function timegenie.close_expired_unassigned_lease(uuid,timestamptz) from public,anon,authenticated;

commit;
notify pgrst, 'reload schema';
