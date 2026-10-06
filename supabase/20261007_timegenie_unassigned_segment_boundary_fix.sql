-- Prevent calendar-day rollover from closing an unassigned segment before it started.
begin;

create or replace function timegenie.coordinate_unassigned_calendar_day_at(
  p_workspace_id uuid, p_device_id uuid, p_lease_token uuid, p_observed_at timestamptz default clock_timestamp()
) returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare session_row timegenie.unassigned_sessions; target_date date; day_end timestamptz; close_at timestamptz; elapsed bigint; new_id uuid; active_timer boolean;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  if not exists(select 1 from timegenie.tracking_leases where workspace_id=p_workspace_id and holder_device_id=p_device_id and lease_token=p_lease_token and expires_at>p_observed_at) then raise exception 'LEASE_EXPIRED'; end if;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text || ':calendar-unassigned',0));
  target_date := timegenie.workspace_work_date(p_workspace_id,p_observed_at);
  for session_row in select * from timegenie.unassigned_sessions
    where workspace_id=p_workspace_id and state in ('collecting','awaiting_resolution') and work_date<>target_date
    order by work_date for update
  loop
    day_end := timegenie.workspace_day_end(p_workspace_id,session_row.work_date);
    update timegenie.unassigned_segments set ended_at=greatest(started_at,least(day_end,p_observed_at)),
      duration_seconds=greatest(0,extract(epoch from (greatest(started_at,least(day_end,p_observed_at))-started_at))::bigint)
    where session_id=session_row.id and ended_at is null;
    select coalesce(sum(duration_seconds),0),max(ended_at) into elapsed,close_at
    from timegenie.unassigned_segments where session_id=session_row.id;
    close_at := coalesce(close_at,greatest(session_row.first_started_at,least(day_end,p_observed_at)));
    update timegenie.unassigned_sessions set duration_seconds=elapsed,last_ended_at=close_at,
      last_continuous_at=close_at,updated_at=p_observed_at,
      state=case when elapsed>0 then 'awaiting_resolution' else 'discarded' end,
      resolution_type=case when elapsed>0 then resolution_type else 'discard' end,
      resolved_at=case when elapsed>0 then resolved_at else close_at end,
      prompted_at=case when elapsed>threshold_seconds then coalesce(prompted_at,p_observed_at) else prompted_at end,version=version+1
    where id=session_row.id;
  end loop;
  select exists(select 1 from timegenie.time_entries where workspace_id=p_workspace_id and state in ('running','paused') and deleted_at is null) into active_timer;
  select * into session_row from timegenie.unassigned_sessions
  where workspace_id=p_workspace_id and work_date=target_date and state in ('collecting','awaiting_resolution')
  order by created_at limit 1 for update;
  if active_timer then
    if found then perform timegenie.close_expired_unassigned_lease(p_workspace_id,p_observed_at); end if;
    return case when session_row.id is null then null else timegenie.cloud_incremental_entity_get(p_workspace_id,'unassigned_sessions',session_row.id::text)->'data' end;
  end if;
  if session_row.id is null then
    new_id := timegenie.calendar_stable_uuid('unassigned-session:'||p_workspace_id||':'||target_date);
    insert into timegenie.unassigned_sessions(
      id,workspace_id,work_date,state,threshold_seconds,duration_seconds,first_started_at,shared_source,migration_state,created_at,updated_at,version,last_continuous_at
    ) values(new_id,p_workspace_id,target_date,'collecting',300,0,p_observed_at,'cloud','adopted',p_observed_at,p_observed_at,1,p_observed_at)
    on conflict(workspace_id,work_date) where state in ('collecting','awaiting_resolution') do update set updated_at=excluded.updated_at
    returning * into session_row;
  end if;
  if not exists(select 1 from timegenie.unassigned_segments where session_id=session_row.id and ended_at is null) then
    insert into timegenie.unassigned_segments(id,workspace_id,session_id,sequence_no,started_at,duration_seconds,lease_token)
    values(timegenie.calendar_stable_uuid('unassigned-segment:'||session_row.id||':'||p_lease_token),p_workspace_id,session_row.id,
      (select coalesce(max(sequence_no),0)+1 from timegenie.unassigned_segments where session_id=session_row.id),p_observed_at,0,p_lease_token)
    on conflict do nothing;
  end if;
  return timegenie.cloud_incremental_entity_get(p_workspace_id,'unassigned_sessions',session_row.id::text)->'data';
end $$;

do $$
declare affected_session_id uuid; repaired_duration bigint; repaired_end timestamptz;
begin
  for affected_session_id in
    select distinct session_id from timegenie.unassigned_segments
    where ended_at is not null and ended_at<started_at
  loop
    update timegenie.unassigned_segments
    set ended_at=started_at,duration_seconds=0
    where session_id=affected_session_id and ended_at is not null and ended_at<started_at;
    select coalesce(sum(duration_seconds),0),max(ended_at) into repaired_duration,repaired_end
    from timegenie.unassigned_segments where session_id=affected_session_id;
    update timegenie.unassigned_sessions
    set duration_seconds=repaired_duration,
        last_ended_at=coalesce(repaired_end,last_ended_at,first_started_at),
        last_continuous_at=greatest(coalesce(last_continuous_at,first_started_at),coalesce(repaired_end,first_started_at)),
        state=case when repaired_duration=0 and state in ('collecting','awaiting_resolution') then 'discarded' else state end,
        resolution_type=case when repaired_duration=0 and state in ('collecting','awaiting_resolution') then 'discard' else resolution_type end,
        resolved_at=case when repaired_duration=0 and state in ('collecting','awaiting_resolution') then coalesce(repaired_end,first_started_at) else resolved_at end,
        updated_at=clock_timestamp(),version=version+1
    where id=affected_session_id;
  end loop;
end $$;

revoke execute on function timegenie.coordinate_unassigned_calendar_day_at(uuid,uuid,uuid,timestamptz) from public,anon,authenticated;

commit;

notify pgrst, 'reload schema';
