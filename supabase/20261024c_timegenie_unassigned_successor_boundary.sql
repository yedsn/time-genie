-- TimeGenie direct-cloud unassigned successor boundary fix.
--
-- Resolving or discarding an anchor during the same work date must restart
-- unassigned tracking at the authoritative resolution boundary. Only the
-- first anchor after a previous work date starts at the new day's boundary.

begin;

create or replace function timegenie.direct_cloud_unassigned_anchor_ensure_locked(
  p_workspace_id uuid,p_device_id uuid,p_observed_at timestamptz
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare
  target_date date;
  session_row timegenie.unassigned_sessions%rowtype;
  open_session timegenie.unassigned_sessions%rowtype;
  close_at timestamptz;
  elapsed bigint;
  threshold integer;
  previous_session_id uuid;
  anchor_started timestamptz;
  new_session_id uuid;
  active_timer boolean;
  rolled_over_previous_date boolean:=false;
begin
  target_date:=timegenie.workspace_work_date(p_workspace_id,p_observed_at);

  for open_session in
    select session.* from timegenie.unassigned_sessions session
    where session.workspace_id=p_workspace_id
      and session.work_date<target_date
      and session.state in ('collecting','awaiting_resolution')
      and exists(select 1 from timegenie.unassigned_segments segment
        where segment.session_id=session.id and segment.ended_at is null)
    order by session.work_date,session.created_at
    for update
  loop
    rolled_over_previous_date:=true;
    close_at:=least(timegenie.workspace_day_end(p_workspace_id,open_session.work_date),p_observed_at);
    update timegenie.unassigned_segments segment set
      ended_at=greatest(segment.started_at,close_at),
      duration_seconds=greatest(0,extract(epoch from(
        greatest(segment.started_at,close_at)-segment.started_at
      ))::bigint),
      lease_token=null
    where segment.session_id=open_session.id and segment.ended_at is null;
    select coalesce(sum(segment.duration_seconds),0) into elapsed
    from timegenie.unassigned_segments segment where segment.session_id=open_session.id;
    update timegenie.unassigned_sessions session set
      duration_seconds=elapsed,
      state=case when elapsed>0 then 'awaiting_resolution' else 'discarded' end,
      resolution_type=case when elapsed>0 then session.resolution_type else 'discard' end,
      resolved_at=case when elapsed>0 then session.resolved_at else close_at end,
      last_ended_at=close_at,
      last_continuous_at=close_at,
      prompted_at=case when elapsed>session.threshold_seconds
        then coalesce(session.prompted_at,close_at) else session.prompted_at end,
      updated_at=p_observed_at,
      version=session.version+1
    where session.id=open_session.id;
  end loop;

  select session.* into session_row from timegenie.unassigned_sessions session
  where session.workspace_id=p_workspace_id
    and session.work_date=target_date
    and session.state in ('collecting','awaiting_resolution')
  order by session.created_at limit 1
  for update;
  if found then
    return jsonb_build_object('sessionId',session_row.id,'created',false);
  end if;

  select exists(select 1 from timegenie.time_entries entry
    where entry.workspace_id=p_workspace_id
      and entry.state in ('running','paused') and entry.deleted_at is null)
  into active_timer;
  if active_timer then
    return jsonb_build_object('sessionId',null,'created',false);
  end if;

  select session.id into previous_session_id
  from timegenie.unassigned_sessions session
  where session.workspace_id=p_workspace_id
    and (session.work_date<target_date
      or (session.work_date=target_date
        and session.state in ('resolved','discarded')))
  order by session.work_date desc,
    coalesce(session.resolved_at,session.last_ended_at,session.updated_at,session.created_at) desc,
    session.created_at desc,session.id desc
  limit 1
  for update;

  if previous_session_id is not null then
    select session.* into session_row from timegenie.unassigned_sessions session
    where session.workspace_id=p_workspace_id
      and session.predecessor_session_id=previous_session_id
    order by session.created_at desc,session.id desc
    limit 1 for update;
    if found and session_row.work_date=target_date
       and session_row.state in ('collecting','awaiting_resolution') then
      return jsonb_build_object('sessionId',session_row.id,'created',false);
    end if;
  end if;

  anchor_started:=case
    when previous_session_id is null then p_observed_at
    when rolled_over_previous_date then timegenie.workspace_day_start(p_workspace_id,target_date)
    when exists(select 1 from timegenie.unassigned_sessions session
      where session.id=previous_session_id and session.work_date=target_date)
      then p_observed_at
    else timegenie.workspace_day_start(p_workspace_id,target_date)
  end;

  select (setting.value_json#>>'{}')::integer into threshold
  from timegenie.app_settings setting
  where setting.workspace_id=p_workspace_id and setting.key='unassigned_prompt_seconds';
  threshold:=coalesce(threshold,300);

  new_session_id:=gen_random_uuid();
  insert into timegenie.unassigned_sessions(
    id,workspace_id,work_date,state,threshold_seconds,duration_seconds,
    first_started_at,last_ended_at,prompted_at,resolution_type,generated_entry_id,
    resolved_at,shared_source,predecessor_session_id,migration_state,created_at,
    updated_at,version,last_continuous_at
  ) values(
    new_session_id,p_workspace_id,target_date,'collecting',threshold,0,
    anchor_started,null,null,null,null,null,'cloud',previous_session_id,'adopted',
    p_observed_at,p_observed_at,1,anchor_started
  );
  insert into timegenie.unassigned_segments(
    id,workspace_id,session_id,sequence_no,started_at,ended_at,duration_seconds,lease_token
  ) values(
    gen_random_uuid(),p_workspace_id,new_session_id,1,anchor_started,null,0,null
  );
  return jsonb_build_object('sessionId',new_session_id,'created',true);
end $$;

revoke all on function timegenie.direct_cloud_unassigned_anchor_ensure_locked(uuid,uuid,timestamptz)
  from public,anon,authenticated;

notify pgrst, 'reload schema';

commit;
