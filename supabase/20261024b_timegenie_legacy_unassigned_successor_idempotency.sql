-- TimeGenie legacy unassigned successor idempotency fix.
--
-- Older clients may resolve an earlier session after a newer successor has
-- already been created or resolved by direct-v2. Walk the existing successor
-- chain before creating another anchor so uq_unassigned_next_session remains
-- a data guard instead of surfacing as VERSION_CONFLICT.

begin;

create or replace function timegenie.timegenie_unassigned_create_anchor(
  p_workspace_id uuid,p_device_id uuid,p_boundary timestamptz,
  p_candidate_id uuid default null,p_predecessor_session_id uuid default null
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare
  session_row timegenie.unassigned_sessions%rowtype;
  successor_row timegenie.unassigned_sessions%rowtype;
  target_date date;
  threshold integer;
  session_id uuid;
  predecessor_id uuid:=p_predecessor_session_id;
begin
  if exists(select 1 from timegenie.time_entries entry
    where entry.workspace_id=p_workspace_id
      and entry.state in ('running','paused') and entry.deleted_at is null) then
    return null;
  end if;

  target_date:=timegenie.workspace_work_date(p_workspace_id,p_boundary);
  select session.* into session_row from timegenie.unassigned_sessions session
  where session.workspace_id=p_workspace_id and session.work_date=target_date
    and session.state in ('collecting','awaiting_resolution')
  order by session.first_started_at desc,session.id desc limit 1 for update;
  if found then
    perform timegenie.timegenie_unassigned_discard_superseded(
      p_workspace_id,session_row.id,p_boundary
    );
    return timegenie.cloud_incremental_entity_get(
      p_workspace_id,'unassigned_sessions',session_row.id::text
    )->'data';
  end if;

  -- A delayed legacy request can still name an older predecessor. Follow the
  -- already-materialized chain to its tail. This also makes retries from mixed
  -- direct-v2/legacy clients converge on one active anchor.
  while predecessor_id is not null loop
    select session.* into successor_row from timegenie.unassigned_sessions session
    where session.workspace_id=p_workspace_id
      and session.predecessor_session_id=predecessor_id
    order by session.created_at desc,session.id desc limit 1 for update;
    exit when not found;
    if successor_row.work_date=target_date
       and successor_row.state in ('collecting','awaiting_resolution') then
      perform timegenie.timegenie_unassigned_discard_superseded(
        p_workspace_id,successor_row.id,p_boundary
      );
      return timegenie.cloud_incremental_entity_get(
        p_workspace_id,'unassigned_sessions',successor_row.id::text
      )->'data';
    end if;
    predecessor_id:=successor_row.id;
  end loop;

  perform timegenie.timegenie_unassigned_discard_superseded(
    p_workspace_id,null,p_boundary
  );
  select coalesce((setting.value_json#>>'{}')::integer,300) into threshold
  from timegenie.app_settings setting
  where setting.workspace_id=p_workspace_id
    and setting.key='unassigned_prompt_seconds';
  threshold:=coalesce(threshold,300);
  session_id:=case
    when p_candidate_id is not null and not exists(
      select 1 from timegenie.unassigned_sessions session where session.id=p_candidate_id
    ) then p_candidate_id
    else timegenie.calendar_stable_uuid(
      'unassigned-anchor:'||p_workspace_id||':'||target_date||':'||
      coalesce(predecessor_id::text,p_boundary::text)
    )
  end;
  insert into timegenie.unassigned_sessions(
    id,workspace_id,work_date,state,threshold_seconds,duration_seconds,
    first_started_at,last_ended_at,prompted_at,resolution_type,
    generated_entry_id,resolved_at,shared_source,predecessor_session_id,
    migration_state,created_at,updated_at,version,last_continuous_at
  ) values(
    session_id,p_workspace_id,target_date,'collecting',threshold,0,p_boundary,
    null,null,null,null,null,'cloud',predecessor_id,'adopted',p_boundary,
    p_boundary,1,p_boundary
  ) returning * into session_row;
  insert into timegenie.unassigned_segments(
    id,workspace_id,session_id,sequence_no,started_at,ended_at,
    duration_seconds,lease_token
  ) values(
    timegenie.calendar_stable_uuid('unassigned-anchor-segment:'||session_id),
    p_workspace_id,session_id,1,p_boundary,null,0,null
  );
  return timegenie.cloud_incremental_entity_get(
    p_workspace_id,'unassigned_sessions',session_id::text
  )->'data';
end $$;

revoke all on function timegenie.timegenie_unassigned_create_anchor(
  uuid,uuid,timestamptz,uuid,uuid
) from public,anon,authenticated;

notify pgrst, 'reload schema';

commit;
