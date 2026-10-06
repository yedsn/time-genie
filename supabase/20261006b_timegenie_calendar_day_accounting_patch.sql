\set ON_ERROR_STOP on

begin;

alter table timegenie.time_entries add column if not exists timer_chain_id uuid;
alter table timegenie.time_entries add column if not exists previous_entry_id uuid references timegenie.time_entries(id);
alter table timegenie.time_entries add column if not exists split_boundary_at timestamptz;
alter table timegenie.time_entries add column if not exists last_continuous_at timestamptz;
alter table timegenie.unassigned_sessions add column if not exists last_continuous_at timestamptz;
alter table timegenie.tracking_leases add column if not exists last_confirmed_at timestamptz;

update timegenie.time_entries
set timer_chain_id = id
where source_type = 'timer' and timer_chain_id is null;
update timegenie.time_entries
set last_continuous_at = coalesce(ended_at, updated_at, started_at)
where source_type = 'timer' and last_continuous_at is null;
update timegenie.unassigned_sessions
set last_continuous_at = coalesce(last_ended_at, updated_at, first_started_at)
where last_continuous_at is null;
update timegenie.tracking_leases
set last_confirmed_at = coalesce(last_confirmed_at, updated_at, expires_at - interval '45 seconds')
where last_confirmed_at is null;

create unique index if not exists uq_time_entries_timer_chain_date
  on timegenie.time_entries(workspace_id, timer_chain_id, work_date)
  where source_type = 'timer' and timer_chain_id is not null and deleted_at is null;
create index if not exists idx_time_entries_previous_entry
  on timegenie.time_entries(previous_entry_id);
drop index if exists timegenie.uq_unassigned_active;
create unique index if not exists uq_unassigned_sessions_active_date
  on timegenie.unassigned_sessions(workspace_id, work_date)
  where state in ('collecting', 'awaiting_resolution');

create or replace function timegenie.workspace_timezone_name(p_workspace_id uuid)
returns text language plpgsql stable security definer set search_path = timegenie, pg_catalog
as $$
declare timezone_name text;
begin
  select timezone into timezone_name from timegenie.workspaces
  where id = p_workspace_id and deleted_at is null;
  if timezone_name is null then raise exception 'NOT_FOUND: workspace'; end if;
  if not exists(select 1 from pg_timezone_names where name = timezone_name) then
    raise exception 'VALIDATION_ERROR: invalid workspace timezone %', timezone_name;
  end if;
  return timezone_name;
end $$;

create or replace function timegenie.workspace_work_date(
  p_workspace_id uuid, p_at timestamptz default clock_timestamp()
) returns date language sql stable security definer set search_path = timegenie, pg_catalog
as $$ select (p_at at time zone timegenie.workspace_timezone_name(p_workspace_id))::date $$;

create or replace function timegenie.workspace_day_start(p_workspace_id uuid, p_work_date date)
returns timestamptz language sql stable security definer set search_path = timegenie, pg_catalog
as $$ select p_work_date::timestamp at time zone timegenie.workspace_timezone_name(p_workspace_id) $$;

create or replace function timegenie.workspace_day_end(p_workspace_id uuid, p_work_date date)
returns timestamptz language sql stable security definer set search_path = timegenie, pg_catalog
as $$ select (p_work_date + 1)::timestamp at time zone timegenie.workspace_timezone_name(p_workspace_id) $$;

create or replace function timegenie.calendar_stable_uuid(p_name text)
returns uuid language sql immutable set search_path = pg_catalog
as $$
  select (substr(md5(p_name),1,8)||'-'||substr(md5(p_name),9,4)||'-5'||substr(md5(p_name),14,3)||'-a'||substr(md5(p_name),18,3)||'-'||substr(md5(p_name),21,12))::uuid
$$;

create or replace function timegenie.sync_workspace_timezone_setting()
returns trigger language plpgsql security definer set search_path = timegenie, pg_catalog
as $$
declare timezone_name text;
begin
  if new.key <> 'timezone' then return new; end if;
  timezone_name := new.value_json #>> '{}';
  if timezone_name is null or not exists(select 1 from pg_timezone_names where name = timezone_name) then
    raise exception 'VALIDATION_ERROR: invalid workspace timezone';
  end if;
  update timegenie.workspaces
  set timezone = timezone_name, updated_at = greatest(updated_at, new.updated_at), version = version + 1
  where id = new.workspace_id and timezone is distinct from timezone_name;
  return new;
end $$;
drop trigger if exists sync_workspace_timezone_setting on timegenie.app_settings;
create trigger sync_workspace_timezone_setting after insert or update of value_json
on timegenie.app_settings for each row execute function timegenie.sync_workspace_timezone_setting();

create or replace function timegenie.fill_calendar_time_entry_fields()
returns trigger language plpgsql set search_path = timegenie, pg_catalog
as $$ begin
  if new.source_type = 'timer' then
    new.timer_chain_id := coalesce(new.timer_chain_id, new.id);
    new.last_continuous_at := coalesce(new.last_continuous_at, new.ended_at, new.updated_at, new.started_at);
  end if;
  return new;
end $$;
drop trigger if exists fill_calendar_time_entry_fields on timegenie.time_entries;
create trigger fill_calendar_time_entry_fields before insert or update on timegenie.time_entries
for each row execute function timegenie.fill_calendar_time_entry_fields();

create or replace function timegenie.ensure_calendar_occurrence(
  p_workspace_id uuid, p_task_id uuid, p_work_date date, p_at timestamptz
) returns void language plpgsql security definer set search_path = timegenie, pg_catalog
as $$
declare rule_row timegenie.task_recurrence_rules; weekday_bit integer; matches_rule boolean := false;
begin
  if p_task_id is null then return; end if;
  select * into rule_row from timegenie.task_recurrence_rules
  where workspace_id = p_workspace_id and task_id = p_task_id
    and effective_start <= p_work_date and (effective_end is null or effective_end >= p_work_date)
  order by effective_start desc limit 1;
  if not found then return; end if;
  weekday_bit := 1 << (extract(isodow from p_work_date)::integer - 1);
  matches_rule := rule_row.frequency = 'daily'
    or (rule_row.frequency = 'weekdays' and extract(isodow from p_work_date)::integer between 1 and 5)
    or (rule_row.frequency = 'weekly' and (rule_row.weekdays_mask & weekday_bit) <> 0);
  if matches_rule then
    insert into timegenie.task_occurrences(
      workspace_id, task_id, occurrence_date, origin, status, created_at, updated_at, version
    ) values(p_workspace_id, p_task_id, p_work_date, 'scheduled', 'open', p_at, p_at, 1)
    on conflict(task_id, occurrence_date) do nothing;
  end if;
end $$;

create or replace function timegenie.coordinate_timer_calendar_day_at(
  p_workspace_id uuid, p_device_id uuid, p_observed_at timestamptz default clock_timestamp()
) returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare entry_row timegenie.time_entries; target_date date; cursor_date date; day_end timestamptz;
declare open_segment timegenie.time_segments; confirmed_end timestamptz; elapsed bigint;
declare next_id uuid; previous_id uuid; next_state text; next_started timestamptz; interrupted boolean;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text || ':calendar-timer',0));
  select * into entry_row from timegenie.time_entries
  where workspace_id=p_workspace_id and state in ('running','paused') and deleted_at is null
  order by started_at desc limit 1 for update;
  if not found then return null; end if;
  update timegenie.time_entries set timer_chain_id=coalesce(timer_chain_id,id),
    last_continuous_at=coalesce(last_continuous_at,updated_at,started_at) where id=entry_row.id returning * into entry_row;
  target_date := timegenie.workspace_work_date(p_workspace_id,p_observed_at);
  interrupted := p_observed_at - entry_row.last_continuous_at > interval '90 seconds';

  if entry_row.work_date = target_date then
    if entry_row.state='running' and interrupted then
      select * into open_segment from timegenie.time_segments where entry_id=entry_row.id and ended_at is null limit 1 for update;
      if found then
        confirmed_end := greatest(open_segment.started_at, least(entry_row.last_continuous_at,p_observed_at));
        update timegenie.time_segments set ended_at=confirmed_end,
          duration_seconds=greatest(0,extract(epoch from (confirmed_end-started_at))::bigint) where id=open_segment.id;
        select coalesce(sum(duration_seconds),0) into elapsed from timegenie.time_segments where entry_id=entry_row.id;
        insert into timegenie.time_segments(id,workspace_id,entry_id,sequence_no,started_at,duration_seconds)
        values(timegenie.calendar_stable_uuid('timer-resume:'||entry_row.id||':'||extract(epoch from p_observed_at)::bigint),
          p_workspace_id,entry_row.id,(select coalesce(max(sequence_no),0)+1 from timegenie.time_segments where entry_id=entry_row.id),p_observed_at,0);
        update timegenie.time_entries set duration_seconds=elapsed,last_continuous_at=p_observed_at,updated_at=p_observed_at,version=version+1
        where id=entry_row.id returning * into entry_row;
      end if;
    else
      update timegenie.time_entries set last_continuous_at=p_observed_at where id=entry_row.id returning * into entry_row;
    end if;
    return timegenie.cloud_incremental_entity_get(p_workspace_id,'time_entries',entry_row.id::text)->'data';
  end if;

  if entry_row.state='running' then
    select * into open_segment from timegenie.time_segments where entry_id=entry_row.id and ended_at is null limit 1 for update;
    if not found then raise exception 'VALIDATION_ERROR: running timer has no open segment'; end if;
    if interrupted then
      confirmed_end := greatest(open_segment.started_at,least(entry_row.last_continuous_at,p_observed_at));
      update timegenie.time_segments set ended_at=confirmed_end,
        duration_seconds=greatest(0,extract(epoch from (confirmed_end-started_at))::bigint) where id=open_segment.id;
      select coalesce(sum(duration_seconds),0) into elapsed from timegenie.time_segments where entry_id=entry_row.id;
      update timegenie.time_entries set state='ended',ended_at=confirmed_end,duration_seconds=elapsed,
        split_boundary_at=confirmed_end,last_continuous_at=confirmed_end,updated_at=p_observed_at,version=version+1
      where id=entry_row.id;
      cursor_date := target_date;
      next_started := p_observed_at;
    else
      day_end := timegenie.workspace_day_end(p_workspace_id,entry_row.work_date);
      update timegenie.time_segments set ended_at=day_end,
        duration_seconds=greatest(0,extract(epoch from (day_end-started_at))::bigint) where id=open_segment.id;
      select coalesce(sum(duration_seconds),0) into elapsed from timegenie.time_segments where entry_id=entry_row.id;
      update timegenie.time_entries set state='ended',ended_at=day_end,duration_seconds=elapsed,
        split_boundary_at=day_end,last_continuous_at=day_end,updated_at=p_observed_at,version=version+1
      where id=entry_row.id;
      cursor_date := entry_row.work_date + 1;
      next_started := day_end;
    end if;
  else
    day_end := timegenie.workspace_day_end(p_workspace_id,entry_row.work_date);
    update timegenie.time_entries set state='ended',ended_at=day_end,split_boundary_at=day_end,
      last_continuous_at=day_end,updated_at=p_observed_at,version=version+1 where id=entry_row.id;
    cursor_date := target_date;
    next_started := p_observed_at;
  end if;

  previous_id := entry_row.id;
  while cursor_date <= target_date loop
    next_id := timegenie.calendar_stable_uuid('timer-slice:'||entry_row.timer_chain_id||':'||cursor_date);
    next_state := case when cursor_date=target_date then entry_row.state else 'ended' end;
    day_end := timegenie.workspace_day_end(p_workspace_id,cursor_date);
    insert into timegenie.time_entries(
      id,workspace_id,work_date,kind,source_type,state,default_task_id,label_snapshot,started_at,ended_at,
      duration_seconds,note,created_at,updated_at,version,created_by_device_id,updated_by_device_id,
      timer_chain_id,previous_entry_id,split_boundary_at,last_continuous_at
    ) values(
      next_id,p_workspace_id,cursor_date,entry_row.kind,'timer',next_state,entry_row.default_task_id,entry_row.label_snapshot,
      next_started,case when next_state='ended' then day_end else null end,
      case when next_state='ended' then greatest(0,extract(epoch from (day_end-next_started))::bigint) else 0 end,
      entry_row.note,next_started,p_observed_at,1,p_device_id,p_device_id,entry_row.timer_chain_id,previous_id,next_started,
      case when next_state='ended' then day_end else p_observed_at end
    ) on conflict(workspace_id,timer_chain_id,work_date) where source_type='timer' and timer_chain_id is not null and deleted_at is null
      do update set updated_at=greatest(timegenie.time_entries.updated_at,excluded.updated_at) returning id into next_id;
    if next_state='running' then
      insert into timegenie.time_segments(id,workspace_id,entry_id,sequence_no,started_at,duration_seconds)
      values(timegenie.calendar_stable_uuid('timer-segment:'||next_id),p_workspace_id,next_id,1,next_started,0) on conflict do nothing;
    elsif next_state='ended' then
      insert into timegenie.time_segments(id,workspace_id,entry_id,sequence_no,started_at,ended_at,duration_seconds)
      values(timegenie.calendar_stable_uuid('timer-segment:'||next_id),p_workspace_id,next_id,1,next_started,day_end,
        greatest(0,extract(epoch from (day_end-next_started))::bigint)) on conflict do nothing;
    end if;
    perform timegenie.ensure_calendar_occurrence(p_workspace_id,entry_row.default_task_id,cursor_date,p_observed_at);
    previous_id := next_id;
    cursor_date := cursor_date + 1;
    next_started := day_end;
  end loop;
  return timegenie.cloud_incremental_entity_get(p_workspace_id,'time_entries',previous_id::text)->'data';
end $$;

create or replace function timegenie.coordinate_timer_calendar_day(
  p_workspace_id uuid, p_device_id uuid
) returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$ begin
  return timegenie.coordinate_timer_calendar_day_at(p_workspace_id,p_device_id,clock_timestamp());
end $$;

create or replace function timegenie.close_expired_unassigned_lease(
  p_workspace_id uuid, p_close_at timestamptz
) returns void language plpgsql security definer set search_path = timegenie, pg_catalog
as $$
declare segment_row timegenie.unassigned_segments; session_row timegenie.unassigned_sessions; elapsed bigint; close_at timestamptz;
begin
  select segment.* into segment_row from timegenie.unassigned_segments segment
  join timegenie.unassigned_sessions session on session.id=segment.session_id
  where session.workspace_id=p_workspace_id and session.state in ('collecting','awaiting_resolution') and segment.ended_at is null
  order by segment.started_at desc limit 1 for update of segment;
  if not found then return; end if;
  close_at := greatest(segment_row.started_at,p_close_at);
  update timegenie.unassigned_segments set ended_at=close_at,
    duration_seconds=greatest(0,extract(epoch from (close_at-started_at))::bigint) where id=segment_row.id;
  select * into session_row from timegenie.unassigned_sessions where id=segment_row.session_id for update;
  select coalesce(sum(duration_seconds),0) into elapsed from timegenie.unassigned_segments where session_id=session_row.id;
  update timegenie.unassigned_sessions set duration_seconds=elapsed,last_ended_at=close_at,
    last_continuous_at=close_at,updated_at=close_at,
    state=case when elapsed>threshold_seconds then 'awaiting_resolution' else state end,version=version+1 where id=session_row.id;
end $$;

create or replace function timegenie.coordinate_unassigned_calendar_day_at(
  p_workspace_id uuid, p_device_id uuid, p_lease_token uuid, p_observed_at timestamptz default clock_timestamp()
) returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare session_row timegenie.unassigned_sessions; target_date date; day_end timestamptz; elapsed bigint; new_id uuid; active_timer boolean;
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
    update timegenie.unassigned_segments set ended_at=least(day_end,p_observed_at),
      duration_seconds=greatest(0,extract(epoch from (least(day_end,p_observed_at)-started_at))::bigint)
    where session_id=session_row.id and ended_at is null;
    select coalesce(sum(duration_seconds),0) into elapsed from timegenie.unassigned_segments where session_id=session_row.id;
    update timegenie.unassigned_sessions set duration_seconds=elapsed,last_ended_at=least(day_end,p_observed_at),
      last_continuous_at=least(day_end,p_observed_at),updated_at=p_observed_at,
      state=case when elapsed>0 then 'awaiting_resolution' else 'discarded' end,
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

create or replace function timegenie.coordinate_unassigned_calendar_day(
  p_workspace_id uuid, p_device_id uuid, p_lease_token uuid
) returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$ begin
  return timegenie.coordinate_unassigned_calendar_day_at(p_workspace_id,p_device_id,p_lease_token,clock_timestamp());
end $$;

create or replace function timegenie.unassigned_tick(p_workspace_id uuid,p_device_id uuid,p_lease_token uuid)
returns void language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare observed_at timestamptz:=clock_timestamp(); session_row timegenie.unassigned_sessions; elapsed bigint;
begin
  perform timegenie.coordinate_timer_calendar_day_at(p_workspace_id,p_device_id,observed_at);
  perform timegenie.coordinate_unassigned_calendar_day_at(p_workspace_id,p_device_id,p_lease_token,observed_at);
  select * into session_row from timegenie.unassigned_sessions where workspace_id=p_workspace_id
    and work_date=timegenie.workspace_work_date(p_workspace_id,observed_at) and state in ('collecting','awaiting_resolution') limit 1 for update;
  if not found then return; end if;
  select coalesce(sum(duration_seconds+case when ended_at is null then greatest(0,extract(epoch from (observed_at-started_at))::bigint) else 0 end),0)
  into elapsed from timegenie.unassigned_segments where session_id=session_row.id;
  update timegenie.unassigned_sessions set duration_seconds=elapsed,last_continuous_at=observed_at,updated_at=observed_at,
    state=case when elapsed>threshold_seconds then 'awaiting_resolution' else state end,
    prompted_at=case when elapsed>threshold_seconds then coalesce(prompted_at,observed_at) else prompted_at end,version=version+1 where id=session_row.id;
end $$;

create or replace function timegenie.tracking_lease_acquire(p_workspace_id uuid,p_device_id uuid,p_lease_token uuid)
returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare current_lease timegenie.tracking_leases; observed_at timestamptz:=clock_timestamp(); close_at timestamptz;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text || ':tracking-lease',0));
  select * into current_lease from timegenie.tracking_leases where workspace_id=p_workspace_id for update;
  if found and current_lease.expires_at is not null and current_lease.expires_at<=observed_at then
    close_at:=least(
      coalesce(current_lease.last_confirmed_at,current_lease.updated_at,current_lease.expires_at),
      coalesce(current_lease.expires_at,observed_at),
      observed_at
    );
    perform timegenie.close_expired_unassigned_lease(p_workspace_id,close_at);
  elsif found and current_lease.expires_at>observed_at and current_lease.holder_device_id<>p_device_id then
    return jsonb_build_object('acquired',false,'holderDeviceId',current_lease.holder_device_id,'expiresAt',current_lease.expires_at);
  end if;
  insert into timegenie.tracking_leases(workspace_id,holder_device_id,lease_token,expires_at,last_confirmed_at,updated_at,version)
  values(p_workspace_id,p_device_id,p_lease_token,observed_at+interval '45 seconds',observed_at,observed_at,1)
  on conflict(workspace_id) do update set holder_device_id=excluded.holder_device_id,lease_token=excluded.lease_token,
    expires_at=excluded.expires_at,last_confirmed_at=excluded.last_confirmed_at,updated_at=excluded.updated_at,version=timegenie.tracking_leases.version+1
  returning * into current_lease;
  perform timegenie.unassigned_tick(p_workspace_id,p_device_id,p_lease_token);
  return jsonb_build_object('acquired',true,'leaseToken',p_lease_token,'expiresAt',current_lease.expires_at,'version',current_lease.version);
end $$;

create or replace function timegenie.tracking_lease_renew(p_workspace_id uuid,p_device_id uuid,p_lease_token uuid)
returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare updated_row timegenie.tracking_leases; observed_at timestamptz:=clock_timestamp();
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  update timegenie.tracking_leases set expires_at=observed_at+interval '45 seconds',last_confirmed_at=observed_at,updated_at=observed_at,version=version+1
  where workspace_id=p_workspace_id and holder_device_id=p_device_id and lease_token=p_lease_token and expires_at>observed_at returning * into updated_row;
  if not found then raise exception 'LEASE_EXPIRED'; end if;
  perform timegenie.unassigned_tick(p_workspace_id,p_device_id,p_lease_token);
  return jsonb_build_object('renewed',true,'expiresAt',updated_row.expires_at,'version',updated_row.version);
end $$;

create or replace function timegenie.tracking_lease_release(p_workspace_id uuid,p_device_id uuid,p_lease_token uuid)
returns boolean language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$ declare observed_at timestamptz:=clock_timestamp(); begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  if exists(select 1 from timegenie.tracking_leases where workspace_id=p_workspace_id and holder_device_id=p_device_id and lease_token=p_lease_token) then
    perform timegenie.close_expired_unassigned_lease(p_workspace_id,observed_at);
    update timegenie.tracking_leases set holder_device_id=null,lease_token=null,expires_at=null,last_confirmed_at=observed_at,updated_at=observed_at,version=version+1
    where workspace_id=p_workspace_id and holder_device_id=p_device_id and lease_token=p_lease_token;
    return true;
  end if;
  return false;
end $$;

create or replace function timegenie.unassigned_get_state(p_workspace_id uuid)
returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare session_row timegenie.unassigned_sessions; segment_started timestamptz; elapsed bigint; target_date date;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  target_date:=timegenie.workspace_work_date(p_workspace_id,clock_timestamp());
  select * into session_row from timegenie.unassigned_sessions where workspace_id=p_workspace_id and work_date=target_date
    and state in ('collecting','awaiting_resolution') order by created_at limit 1;
  if not found then return null; end if;
  select started_at into segment_started from timegenie.unassigned_segments where session_id=session_row.id and ended_at is null limit 1;
  select coalesce(sum(duration_seconds+case when ended_at is null then greatest(0,extract(epoch from (clock_timestamp()-started_at))::bigint) else 0 end),0)
    into elapsed from timegenie.unassigned_segments where session_id=session_row.id;
  return jsonb_build_object('session_id',session_row.id,'work_date',session_row.work_date,'state',case when elapsed>session_row.threshold_seconds then 'awaiting_resolution' else session_row.state end,
    'first_started_at',session_row.first_started_at,'last_ended_at',session_row.last_ended_at,'current_segment_started_at',segment_started,
    'elapsed_seconds',elapsed,'required_minutes',case when elapsed>0 then greatest(1,ceil(elapsed/60.0)::integer) else 0 end,
    'threshold_seconds',session_row.threshold_seconds,'must_resolve',elapsed>session_row.threshold_seconds,'version',session_row.version);
end $$;

create or replace function timegenie.unassigned_get_or_create_shared(
  p_workspace_id uuid,p_device_id uuid,p_candidate jsonb default null,p_predecessor_session_id uuid default null,p_started_at timestamptz default null
) returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare session_row timegenie.unassigned_sessions; boundary timestamptz; threshold integer; candidate_id uuid; target_date date;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text || ':shared-unassigned',0));
  boundary:=coalesce(p_started_at,case when p_candidate is null then null else to_timestamp((p_candidate->>'first_started_at')::double precision/1000) end,clock_timestamp());
  target_date:=timegenie.workspace_work_date(p_workspace_id,boundary);
  select * into session_row from timegenie.unassigned_sessions where workspace_id=p_workspace_id and work_date=target_date
    and state in ('collecting','awaiting_resolution') order by created_at limit 1 for update;
  if found then return timegenie.cloud_incremental_entity_get(p_workspace_id,'unassigned_sessions',session_row.id::text)->'data'; end if;
  if exists(select 1 from timegenie.time_entries where workspace_id=p_workspace_id and state in ('running','paused') and deleted_at is null) then return null; end if;
  threshold:=coalesce((p_candidate->>'threshold_seconds')::integer,300);
  candidate_id:=coalesce(nullif(p_candidate->>'id','')::uuid,timegenie.calendar_stable_uuid('unassigned-session:'||p_workspace_id||':'||target_date));
  insert into timegenie.unassigned_sessions(id,workspace_id,work_date,state,threshold_seconds,duration_seconds,first_started_at,last_ended_at,prompted_at,
    resolution_type,generated_entry_id,resolved_at,shared_source,predecessor_session_id,migration_state,created_at,updated_at,version,last_continuous_at)
  values(candidate_id,p_workspace_id,target_date,case when p_candidate->>'state'='awaiting_resolution' then 'awaiting_resolution' else 'collecting' end,
    threshold,coalesce((p_candidate->>'duration_seconds')::bigint,0),boundary,null,null,null,null,null,'cloud',p_predecessor_session_id,'adopted',boundary,clock_timestamp(),1,boundary)
  returning * into session_row;
  insert into timegenie.unassigned_segments(id,workspace_id,session_id,sequence_no,started_at,ended_at,duration_seconds,lease_token)
  values(timegenie.calendar_stable_uuid('unassigned-segment:'||session_row.id),p_workspace_id,session_row.id,1,boundary,null,0,null);
  return timegenie.cloud_incremental_entity_get(p_workspace_id,'unassigned_sessions',session_row.id::text)->'data';
end $$;

-- Preserve the existing broad apply/import implementations and wrap only the
-- calendar-day entities. Reapplying the patch is safe because the preserved
-- function names are created once.
do $$ begin
  if to_regprocedure('timegenie.cloud_apply_patch_pre_calendar(uuid,uuid,uuid,text,text,text,bigint,jsonb)') is null then
    alter function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,jsonb) rename to cloud_apply_patch_pre_calendar;
  end if;
  if to_regprocedure('timegenie.cloud_apply_patch_pre_calendar(uuid,uuid,uuid,text,text,text,bigint,bigint,bigint,jsonb)') is null then
    alter function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,bigint,bigint,jsonb) rename to cloud_apply_patch_pre_calendar;
  end if;
  if to_regprocedure('timegenie.migration_import_snapshot_pre_calendar(uuid,uuid,uuid,jsonb)') is null then
    alter function timegenie.migration_import_snapshot(uuid,uuid,uuid,jsonb) rename to migration_import_snapshot_pre_calendar;
  end if;
end $$;

create or replace function timegenie.apply_calendar_time_entry(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_operation_type text,p_entity_id text,p_base_version bigint,p_payload jsonb,p_request jsonb
) returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare row_data jsonb:=coalesce(p_payload,'{}'::jsonb); existing timegenie.time_entries; authoritative timegenie.time_entries; result_value jsonb; cached jsonb;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  cached:=timegenie.processed_operation_result(p_workspace_id,p_operation_id,p_operation_type,p_request);
  if cached is not null then return cached; end if;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':timer-chain:'||coalesce(row_data->>'timer_chain_id',p_entity_id),0));
  select * into authoritative from timegenie.time_entries where workspace_id=p_workspace_id and source_type='timer' and deleted_at is null
    and timer_chain_id=coalesce(nullif(row_data->>'timer_chain_id','')::uuid,p_entity_id::uuid) and work_date=(row_data->>'work_date')::date limit 1 for update;
  if found and authoritative.id::text<>p_entity_id then
    return jsonb_build_object('superseded',true,'candidateEntityId',p_entity_id,'authoritative',
      timegenie.cloud_incremental_entity_get(p_workspace_id,'time_entries',authoritative.id::text)->'data');
  end if;
  select * into existing from timegenie.time_entries where workspace_id=p_workspace_id and id=p_entity_id::uuid for update;
  if p_base_version is null and found then raise exception 'SYNC_CONFLICT: expected new entity, current version %',existing.version; end if;
  if p_base_version is not null and (not found or existing.version<>p_base_version) then raise exception 'SYNC_CONFLICT: expected version %, current version %',p_base_version,existing.version; end if;
  insert into timegenie.time_entries(id,workspace_id,work_date,kind,source_type,state,default_task_id,label_snapshot,started_at,ended_at,duration_seconds,note,
    origin_unassigned_session_id,created_at,updated_at,version,created_by_device_id,updated_by_device_id,deleted_at,timer_chain_id,previous_entry_id,split_boundary_at,last_continuous_at)
  values((row_data->>'id')::uuid,p_workspace_id,(row_data->>'work_date')::date,row_data->>'kind',row_data->>'source_type',row_data->>'state',
    nullif(row_data->>'default_task_id','')::uuid,row_data->>'label_snapshot',to_timestamp((row_data->>'started_at')::double precision/1000),
    case when row_data->>'ended_at' is null then null else to_timestamp((row_data->>'ended_at')::double precision/1000) end,coalesce((row_data->>'duration_seconds')::bigint,0),row_data->>'note',
    nullif(row_data->>'origin_unassigned_session_id','')::uuid,to_timestamp(coalesce((row_data->>'created_at')::double precision,extract(epoch from clock_timestamp())*1000)/1000),
    to_timestamp(coalesce((row_data->>'updated_at')::double precision,extract(epoch from clock_timestamp())*1000)/1000),coalesce((row_data->>'version')::bigint,1),p_device_id,p_device_id,
    case when row_data->>'deleted_at' is null then null else to_timestamp((row_data->>'deleted_at')::double precision/1000) end,
    coalesce(nullif(row_data->>'timer_chain_id','')::uuid,(row_data->>'id')::uuid),nullif(row_data->>'previous_entry_id','')::uuid,
    case when row_data->>'split_boundary_at' is null then null else to_timestamp((row_data->>'split_boundary_at')::double precision/1000) end,
    case when row_data->>'last_continuous_at' is null then coalesce(case when row_data->>'ended_at' is null then null else to_timestamp((row_data->>'ended_at')::double precision/1000) end,to_timestamp((row_data->>'updated_at')::double precision/1000)) else to_timestamp((row_data->>'last_continuous_at')::double precision/1000) end)
  on conflict(id) do update set work_date=excluded.work_date,kind=excluded.kind,state=excluded.state,default_task_id=excluded.default_task_id,label_snapshot=excluded.label_snapshot,
    started_at=excluded.started_at,ended_at=excluded.ended_at,duration_seconds=excluded.duration_seconds,note=excluded.note,updated_at=excluded.updated_at,version=excluded.version,
    updated_by_device_id=p_device_id,deleted_at=excluded.deleted_at,timer_chain_id=excluded.timer_chain_id,previous_entry_id=excluded.previous_entry_id,
    split_boundary_at=excluded.split_boundary_at,last_continuous_at=excluded.last_continuous_at;
  delete from timegenie.time_segments where entry_id=p_entity_id::uuid;
  insert into timegenie.time_segments(id,workspace_id,entry_id,sequence_no,started_at,ended_at,duration_seconds)
    select (item->>'id')::uuid,p_workspace_id,p_entity_id::uuid,(item->>'sequence_no')::integer,to_timestamp((item->>'started_at')::double precision/1000),
      case when item->>'ended_at' is null then null else to_timestamp((item->>'ended_at')::double precision/1000) end,coalesce((item->>'duration_seconds')::bigint,0)
    from jsonb_array_elements(coalesce(row_data->'segments','[]'::jsonb)) item;
  delete from timegenie.time_allocations where entry_id=p_entity_id::uuid;
  insert into timegenie.time_allocations(id,workspace_id,entry_id,task_id,minutes,note,created_at,updated_at,version)
    select (item->>'id')::uuid,p_workspace_id,p_entity_id::uuid,(item->>'task_id')::uuid,(item->>'minutes')::integer,item->>'note',
      to_timestamp((item->>'created_at')::double precision/1000),to_timestamp((item->>'updated_at')::double precision/1000),coalesce((item->>'version')::bigint,1)
    from jsonb_array_elements(coalesce(row_data->'allocations','[]'::jsonb)) item;
  result_value:=jsonb_build_object('entityType','time_entry','entityId',p_entity_id,'version',(row_data->>'version')::bigint,
    'authoritative',timegenie.cloud_incremental_entity_get(p_workspace_id,'time_entries',p_entity_id)->'data');
  perform timegenie.record_processed_operation(p_workspace_id,p_operation_id,p_device_id,p_operation_type,p_request,result_value);
  return result_value;
end $$;

create or replace function timegenie.cloud_apply_patch(p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_operation_type text,p_entity_type text,p_entity_id text,p_base_version bigint,p_payload jsonb)
returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog as $$ begin
  if p_entity_type='time_entry' and coalesce(p_payload->>'source_type','')='timer' then
    return timegenie.apply_calendar_time_entry(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_id,p_base_version,p_payload,
      jsonb_build_object('deviceId',p_device_id,'entityType',p_entity_type,'entityId',p_entity_id,'baseVersion',p_base_version,'payload',coalesce(p_payload,'{}'::jsonb)));
  end if;
  return timegenie.cloud_apply_patch_pre_calendar(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_type,p_entity_id,p_base_version,p_payload);
end $$;

create or replace function timegenie.cloud_apply_patch(p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_operation_type text,p_entity_type text,p_entity_id text,p_base_version bigint,p_payload_version bigint,p_coalesced_count bigint,p_payload jsonb)
returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog as $$ begin
  if p_entity_type='time_entry' and coalesce(p_payload->>'source_type','')='timer' then
    if p_payload_version is null or (p_payload->>'version')::bigint<>p_payload_version then raise exception 'VERSION_CONFLICT: declared payload version mismatch'; end if;
    if p_coalesced_count is null or p_coalesced_count<1 then raise exception 'VERSION_CONFLICT: coalesced timer operation count must be positive'; end if;
    if (p_base_version is null and p_payload_version<>p_coalesced_count) or (p_base_version is not null and p_payload_version<>p_base_version+p_coalesced_count) then
      raise exception 'VERSION_CONFLICT: timer payload version does not match operation span';
    end if;
    return timegenie.apply_calendar_time_entry(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_id,p_base_version,p_payload,
      jsonb_build_object('deviceId',p_device_id,'entityType',p_entity_type,'entityId',p_entity_id,'baseVersion',p_base_version,
        'payloadVersion',p_payload_version,'coalescedCount',p_coalesced_count,'payload',coalesce(p_payload,'{}'::jsonb)));
  end if;
  return timegenie.cloud_apply_patch_pre_calendar(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_type,p_entity_id,p_base_version,p_payload_version,p_coalesced_count,p_payload);
end $$;

create or replace function timegenie.migration_import_snapshot(p_workspace_id uuid,p_operation_id uuid,p_device_id uuid,p_snapshot jsonb)
returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$ declare result_value jsonb; row_data jsonb; begin
  result_value:=timegenie.migration_import_snapshot_pre_calendar(p_workspace_id,p_operation_id,p_device_id,p_snapshot);
  for row_data in select value from jsonb_array_elements(coalesce(p_snapshot->'time_entries','[]'::jsonb)) loop
    if row_data->>'source_type'='timer' then
      update timegenie.time_entries set timer_chain_id=coalesce(nullif(row_data->>'timer_chain_id','')::uuid,(row_data->>'id')::uuid),
        previous_entry_id=nullif(row_data->>'previous_entry_id','')::uuid,
        split_boundary_at=case when row_data->>'split_boundary_at' is null then null else to_timestamp((row_data->>'split_boundary_at')::double precision/1000) end,
        last_continuous_at=case when row_data->>'last_continuous_at' is null then coalesce(ended_at,updated_at,started_at) else to_timestamp((row_data->>'last_continuous_at')::double precision/1000) end
      where workspace_id=p_workspace_id and id=(row_data->>'id')::uuid;
    end if;
  end loop;
  for row_data in select value from jsonb_array_elements(coalesce(p_snapshot->'unassigned_sessions','[]'::jsonb)) loop
    update timegenie.unassigned_sessions set last_continuous_at=case when row_data->>'last_continuous_at' is null then coalesce(last_ended_at,updated_at,first_started_at)
      else to_timestamp((row_data->>'last_continuous_at')::double precision/1000) end where workspace_id=p_workspace_id and id=(row_data->>'id')::uuid;
  end loop;
  return result_value;
end $$;

revoke execute on function timegenie.workspace_timezone_name(uuid) from public,anon,authenticated;
revoke execute on function timegenie.calendar_stable_uuid(text) from public,anon,authenticated;
revoke execute on function timegenie.sync_workspace_timezone_setting() from public,anon,authenticated;
revoke execute on function timegenie.fill_calendar_time_entry_fields() from public,anon,authenticated;
revoke execute on function timegenie.ensure_calendar_occurrence(uuid,uuid,date,timestamptz) from public,anon,authenticated;
revoke execute on function timegenie.close_expired_unassigned_lease(uuid,timestamptz) from public,anon,authenticated;
revoke execute on function timegenie.coordinate_timer_calendar_day_at(uuid,uuid,timestamptz) from public,anon,authenticated;
revoke execute on function timegenie.coordinate_unassigned_calendar_day_at(uuid,uuid,uuid,timestamptz) from public,anon,authenticated;
revoke execute on function timegenie.apply_calendar_time_entry(uuid,uuid,uuid,text,text,bigint,jsonb,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.cloud_apply_patch_pre_calendar(uuid,uuid,uuid,text,text,text,bigint,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.cloud_apply_patch_pre_calendar(uuid,uuid,uuid,text,text,text,bigint,bigint,bigint,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.migration_import_snapshot_pre_calendar(uuid,uuid,uuid,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.workspace_work_date(uuid,timestamptz) from public,anon;
revoke execute on function timegenie.workspace_day_start(uuid,date) from public,anon;
revoke execute on function timegenie.workspace_day_end(uuid,date) from public,anon;
drop function if exists timegenie.coordinate_timer_calendar_day(uuid,uuid,timestamptz);
drop function if exists timegenie.coordinate_unassigned_calendar_day(uuid,uuid,uuid,timestamptz);
revoke execute on function timegenie.coordinate_timer_calendar_day(uuid,uuid) from public,anon;
revoke execute on function timegenie.coordinate_unassigned_calendar_day(uuid,uuid,uuid) from public,anon;
grant execute on function timegenie.workspace_work_date(uuid,timestamptz) to authenticated;
grant execute on function timegenie.workspace_day_start(uuid,date) to authenticated;
grant execute on function timegenie.workspace_day_end(uuid,date) to authenticated;
grant execute on function timegenie.coordinate_timer_calendar_day(uuid,uuid) to authenticated;
grant execute on function timegenie.coordinate_unassigned_calendar_day(uuid,uuid,uuid) to authenticated;
grant execute on function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,jsonb) to authenticated;
grant execute on function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,bigint,bigint,jsonb) to authenticated;
grant execute on function timegenie.migration_import_snapshot(uuid,uuid,uuid,jsonb) to authenticated;

commit;

notify pgrst, 'reload schema';
