-- Direct-v2 timer recovery, continuity and device-takeover transitions.
-- Reads and lease heartbeats are deliberately not allowed to mutate timer
-- business rows. A workspace member must submit an explicit, versioned
-- mutation before calendar rollover or an observed continuity gap is applied.

create or replace function timegenie.direct_cloud_timer_recover_locked(
  p_workspace_id uuid,
  p_device_id uuid,
  p_entry_id uuid,
  p_observed_at timestamptz
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare entry_row timegenie.time_entries%rowtype;
declare target_date date; cursor_date date; day_end timestamptz;
declare open_segment timegenie.time_segments%rowtype;
declare confirmed_end timestamptz; elapsed bigint;
declare next_id uuid; previous_id uuid; next_state text; next_started timestamptz;
declare interrupted boolean:=false; calendar_rollover boolean:=false;
declare created_slice_count integer:=0; chain_id uuid; target_state text;
begin
  select entry.* into entry_row from timegenie.time_entries entry
  where entry.workspace_id=p_workspace_id and entry.id=p_entry_id
    and entry.deleted_at is null
  for update;
  if not found then raise exception 'NOT_FOUND: 计时记录不存在'; end if;
  if entry_row.source_type<>'timer' then
    raise exception 'VALIDATION_ERROR: 只有计时记录可以执行恢复转换';
  end if;
  if entry_row.state not in('running','paused') then
    raise exception 'VERSION_CONFLICT: 计时已经结束，请重新加载';
  end if;

  chain_id:=coalesce(entry_row.timer_chain_id,entry_row.id);
  if entry_row.timer_chain_id is null then
    update timegenie.time_entries set timer_chain_id=chain_id
    where id=entry_row.id returning * into entry_row;
  end if;
  if entry_row.last_continuous_at is null then
    update timegenie.time_entries set last_continuous_at=entry_row.started_at
    where id=entry_row.id returning * into entry_row;
  end if;

  interrupted:=entry_row.state='running'
    and p_observed_at-entry_row.last_continuous_at>interval '90 seconds';
  target_state:=entry_row.state;
  target_date:=timegenie.workspace_work_date(p_workspace_id,p_observed_at);

  if entry_row.work_date=target_date then
    if interrupted then
      select segment.* into open_segment from timegenie.time_segments segment
      where segment.workspace_id=p_workspace_id and segment.entry_id=entry_row.id
        and segment.ended_at is null
      order by segment.sequence_no desc limit 1 for update;
      if not found then
        raise exception 'VALIDATION_ERROR: 运行中的计时缺少开放时间段';
      end if;
      confirmed_end:=greatest(
        open_segment.started_at,
        least(entry_row.last_continuous_at,p_observed_at)
      );
      update timegenie.time_segments set ended_at=confirmed_end,
        duration_seconds=greatest(extract(epoch from(confirmed_end-started_at))::bigint,0)
      where id=open_segment.id;
      insert into timegenie.time_segments(
        id,workspace_id,entry_id,sequence_no,started_at,duration_seconds
      ) values(
        timegenie.calendar_stable_uuid(
          'direct-timer-resume:'||entry_row.id||':'||extract(epoch from p_observed_at)::bigint
        ),p_workspace_id,entry_row.id,
        (select coalesce(max(segment.sequence_no),0)+1
          from timegenie.time_segments segment where segment.entry_id=entry_row.id),
        p_observed_at,0
      );
      select coalesce(sum(segment.duration_seconds),0) into elapsed
      from timegenie.time_segments segment where segment.entry_id=entry_row.id;
      update timegenie.time_entries set duration_seconds=elapsed,
        last_continuous_at=p_observed_at,updated_at=p_observed_at,
        updated_by_device_id=p_device_id,version=version+1
      where id=entry_row.id returning * into entry_row;
    else
      update timegenie.time_entries set
        last_continuous_at=greatest(coalesce(last_continuous_at,started_at),p_observed_at)
      where id=entry_row.id returning * into entry_row;
    end if;
    return jsonb_build_object(
      'activeEntryId',entry_row.id,
      'calendarRollover',false,
      'continuityInterrupted',interrupted,
      'createdSliceCount',0,
      'recovered',interrupted
    );
  end if;

  calendar_rollover:=true;
  day_end:=timegenie.workspace_day_end(p_workspace_id,entry_row.work_date);
  if entry_row.state='running' then
    select segment.* into open_segment from timegenie.time_segments segment
    where segment.workspace_id=p_workspace_id and segment.entry_id=entry_row.id
      and segment.ended_at is null
    order by segment.sequence_no desc limit 1 for update;
    if not found then
      raise exception 'VALIDATION_ERROR: 运行中的计时缺少开放时间段';
    end if;
    if interrupted then
      confirmed_end:=greatest(
        open_segment.started_at,
        least(entry_row.last_continuous_at,p_observed_at)
      );
      update timegenie.time_segments set ended_at=confirmed_end,
        duration_seconds=greatest(extract(epoch from(confirmed_end-started_at))::bigint,0)
      where id=open_segment.id;
      select coalesce(sum(segment.duration_seconds),0) into elapsed
      from timegenie.time_segments segment where segment.entry_id=entry_row.id;
      update timegenie.time_entries set state='ended',ended_at=confirmed_end,
        duration_seconds=elapsed,split_boundary_at=confirmed_end,
        last_continuous_at=confirmed_end,updated_at=p_observed_at,
        updated_by_device_id=p_device_id,version=version+1
      where id=entry_row.id returning * into entry_row;
      cursor_date:=target_date;
      next_started:=p_observed_at;
    else
      update timegenie.time_segments set ended_at=day_end,
        duration_seconds=greatest(extract(epoch from(day_end-started_at))::bigint,0)
      where id=open_segment.id;
      select coalesce(sum(segment.duration_seconds),0) into elapsed
      from timegenie.time_segments segment where segment.entry_id=entry_row.id;
      update timegenie.time_entries set state='ended',ended_at=day_end,
        duration_seconds=elapsed,split_boundary_at=day_end,
        last_continuous_at=day_end,updated_at=p_observed_at,
        updated_by_device_id=p_device_id,version=version+1
      where id=entry_row.id returning * into entry_row;
      cursor_date:=entry_row.work_date+1;
      next_started:=day_end;
    end if;
  else
    update timegenie.time_entries set state='ended',ended_at=day_end,
      split_boundary_at=day_end,last_continuous_at=day_end,
      updated_at=p_observed_at,updated_by_device_id=p_device_id,version=version+1
    where id=entry_row.id returning * into entry_row;
    cursor_date:=target_date;
    next_started:=p_observed_at;
  end if;

  previous_id:=entry_row.id;
  while cursor_date<=target_date loop
    next_id:=timegenie.calendar_stable_uuid(
      'timer-slice:'||chain_id||':'||cursor_date
    );
    next_state:=case when cursor_date=target_date then target_state else 'ended' end;
    day_end:=timegenie.workspace_day_end(p_workspace_id,cursor_date);
    insert into timegenie.time_entries(
      id,workspace_id,work_date,kind,source_type,state,default_task_id,
      label_snapshot,started_at,ended_at,duration_seconds,note,created_at,
      updated_at,version,created_by_device_id,updated_by_device_id,timer_chain_id,
      previous_entry_id,split_boundary_at,last_continuous_at
    ) values(
      next_id,p_workspace_id,cursor_date,entry_row.kind,'timer',next_state,
      entry_row.default_task_id,entry_row.label_snapshot,next_started,
      case when next_state='ended' then day_end else null end,
      case when next_state='ended'
        then greatest(extract(epoch from(day_end-next_started))::bigint,0)
        else 0 end,
      entry_row.note,p_observed_at,p_observed_at,1,p_device_id,p_device_id,
      chain_id,previous_id,next_started,
      case when next_state='ended' then day_end else p_observed_at end
    ) on conflict(workspace_id,timer_chain_id,work_date)
      where source_type='timer' and timer_chain_id is not null and deleted_at is null
      do update set updated_at=greatest(timegenie.time_entries.updated_at,excluded.updated_at)
      returning id into next_id;
    if next_state='running' then
      insert into timegenie.time_segments(
        id,workspace_id,entry_id,sequence_no,started_at,duration_seconds
      ) values(
        timegenie.calendar_stable_uuid('timer-segment:'||next_id),
        p_workspace_id,next_id,1,next_started,0
      ) on conflict do nothing;
    elsif next_state='ended' then
      insert into timegenie.time_segments(
        id,workspace_id,entry_id,sequence_no,started_at,ended_at,duration_seconds
      ) values(
        timegenie.calendar_stable_uuid('timer-segment:'||next_id),
        p_workspace_id,next_id,1,next_started,day_end,
        greatest(extract(epoch from(day_end-next_started))::bigint,0)
      ) on conflict do nothing;
    end if;
    perform timegenie.ensure_calendar_occurrence(
      p_workspace_id,entry_row.default_task_id,cursor_date,p_observed_at
    );
    created_slice_count:=created_slice_count+1;
    previous_id:=next_id;
    cursor_date:=cursor_date+1;
    next_started:=day_end;
  end loop;

  return jsonb_build_object(
    'activeEntryId',previous_id,
    'calendarRollover',calendar_rollover,
    'continuityInterrupted',interrupted,
    'createdSliceCount',created_slice_count,
    'recovered',true
  );
end $$;

revoke execute on function timegenie.direct_cloud_timer_recover_locked(uuid,uuid,uuid,timestamptz)
  from public,anon,authenticated;

-- Direct-v2 timer writes claim the workspace tracking lease inside the same
-- transaction as the business mutation. The rejection carries the latest active
-- timer so a losing device can refresh without creating a local recovery
-- operation. An expired lease (or a token rotated by the same device) may be
-- claimed here; an unexpired lease held by another device may not.
create or replace function timegenie.direct_cloud_tracking_claim_lease(
  p_workspace_id uuid,p_device_id uuid,p_lease_token uuid
) returns timegenie.tracking_leases
language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare current_lease timegenie.tracking_leases%rowtype;
declare observed_at timestamptz:=clock_timestamp();
declare active_entry jsonb;
begin
  if p_lease_token is null then
    raise exception 'LEASE_LOST: 当前计时操作缺少设备租约，请刷新后重试';
  end if;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':tracking-lease',0));
  select lease.* into current_lease from timegenie.tracking_leases lease
  where lease.workspace_id=p_workspace_id for update;
  if found
     and current_lease.holder_device_id is not null
     and current_lease.holder_device_id<>p_device_id
     and current_lease.expires_at is not null
     and current_lease.expires_at>observed_at then
    select timegenie.direct_cloud_time_entry_project(
      p_workspace_id,entry.id,observed_at
    ) into active_entry
    from timegenie.time_entries entry
    where entry.workspace_id=p_workspace_id
      and entry.state in('running','paused') and entry.deleted_at is null
    order by entry.started_at desc,entry.id limit 1;
    raise exception 'LEASE_LOST: 当前计时由其他设备持有，请刷新后重试'
      using detail=jsonb_build_object(
        'code','LEASE_LOST',
        'holderDeviceId',current_lease.holder_device_id,
        'expiresAt',current_lease.expires_at,
        'activeEntry',active_entry
      )::text;
  end if;
  insert into timegenie.tracking_leases(
    workspace_id,holder_device_id,lease_token,expires_at,last_confirmed_at,updated_at,version
  ) values(
    p_workspace_id,p_device_id,p_lease_token,observed_at+interval '45 seconds',
    observed_at,observed_at,1
  ) on conflict(workspace_id) do update set
    holder_device_id=excluded.holder_device_id,lease_token=excluded.lease_token,
    expires_at=excluded.expires_at,last_confirmed_at=excluded.last_confirmed_at,
    updated_at=excluded.updated_at,version=timegenie.tracking_leases.version+1
  returning * into current_lease;
  return current_lease;
end $$;

revoke execute on function timegenie.direct_cloud_tracking_claim_lease(uuid,uuid,uuid)
  from public,anon,authenticated;

create or replace function timegenie.direct_cloud_active_timer(
  p_workspace_id uuid,p_device_id uuid,p_expected_epoch bigint
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare server_time_value timestamptz:=now(); active_entry jsonb;
begin
  perform timegenie.assert_direct_v2_workspace(
    p_workspace_id,p_device_id,p_expected_epoch
  );
  select timegenie.direct_cloud_time_entry_project(
    p_workspace_id,entry.id,server_time_value
  ) into active_entry
  from timegenie.time_entries entry
  where entry.workspace_id=p_workspace_id
    and entry.state in('running','paused') and entry.deleted_at is null
  order by entry.started_at desc,entry.id limit 1;
  return timegenie.direct_cloud_query_response(
    p_workspace_id,p_device_id,
    jsonb_build_object('activeEntry',active_entry),
    array['time'],p_expected_epoch
  );
end $$;

revoke all on function timegenie.direct_cloud_active_timer(uuid,uuid,bigint)
  from public,anon;
grant execute on function timegenie.direct_cloud_active_timer(uuid,uuid,bigint)
  to authenticated;

create or replace function timegenie.direct_cloud_timer_recover(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,
  p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb;
declare entry_row timegenie.time_entries%rowtype; recovery jsonb;
declare entry_id uuid; expected_version bigint; observed_at timestamptz:=now();
declare active_id uuid; active_entry jsonb; slices_value jsonb; lease_token uuid;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object(
    'expectedEpoch',p_expected_epoch,
    'payload',coalesce(p_payload,'{}'::jsonb)-'leaseToken'
  );
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':active-timer',0));
  cached:=timegenie.direct_cloud_operation_result(
    p_workspace_id,p_operation_id,'direct_timer_recover',request_value
  );
  if cached is not null then return cached; end if;
  lease_token:=nullif(p_payload->>'leaseToken','')::uuid;
  perform timegenie.direct_cloud_tracking_claim_lease(
    p_workspace_id,p_device_id,lease_token
  );
  entry_id:=nullif(p_payload->>'entryId','')::uuid;
  expected_version:=nullif(p_payload->>'expectedVersion','')::bigint;
  if entry_id is null or expected_version is null then
    raise exception 'VALIDATION_ERROR: 计时恢复参数不完整';
  end if;
  select entry.* into entry_row from timegenie.time_entries entry
  where entry.workspace_id=p_workspace_id and entry.id=entry_id
    and entry.deleted_at is null
  for update;
  if not found then raise exception 'NOT_FOUND: 计时记录不存在'; end if;
  if entry_row.version<>expected_version then
    raise exception 'VERSION_CONFLICT: 计时版本已变化，请重新加载';
  end if;
  if entry_row.state not in('running','paused') then
    raise exception 'VERSION_CONFLICT: 计时已经结束，请重新加载';
  end if;

  recovery:=timegenie.direct_cloud_timer_recover_locked(
    p_workspace_id,p_device_id,entry_id,observed_at
  );
  active_id:=(recovery->>'activeEntryId')::uuid;
  active_entry:=timegenie.direct_cloud_time_entry_project(
    p_workspace_id,active_id,observed_at
  );
  select coalesce(jsonb_agg(timegenie.direct_cloud_time_entry_project(
    p_workspace_id,entry.id,observed_at)
    order by entry.work_date,entry.started_at,entry.id),'[]'::jsonb)
  into slices_value
  from timegenie.time_entries entry
  where entry.workspace_id=p_workspace_id and entry.deleted_at is null
    and coalesce(entry.timer_chain_id,entry.id)=coalesce(
      (select chain.timer_chain_id from timegenie.time_entries chain where chain.id=active_id),
      active_id
    );
  result_value:=timegenie.direct_cloud_mutation_response(
    p_workspace_id,p_device_id,p_operation_id,jsonb_build_object(
      'activeEntry',active_entry,'slices',slices_value,
      'recovered',(recovery->>'recovered')::boolean,
      'calendarRollover',(recovery->>'calendarRollover')::boolean,
      'continuityInterrupted',(recovery->>'continuityInterrupted')::boolean,
      'createdSliceCount',(recovery->>'createdSliceCount')::integer,
      'observedAt',(extract(epoch from observed_at)*1000)::bigint,
      'noOp',not(recovery->>'recovered')::boolean
    ),array['time'],p_expected_epoch
  );
  perform timegenie.record_direct_cloud_operation(
    p_workspace_id,p_operation_id,p_device_id,'direct_timer_recover',
    request_value,result_value
  );
  return result_value;
end $$;

create or replace function timegenie.direct_cloud_tracking_acquire(
  p_workspace_id uuid,p_device_id uuid,p_expected_epoch bigint,p_lease_token uuid
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare current_lease timegenie.tracking_leases%rowtype;
declare observed_at timestamptz:=now(); previous_holder uuid;
declare active_entry jsonb;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  if p_lease_token is null then raise exception 'VALIDATION_ERROR: 缺少租约令牌'; end if;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':tracking-lease',0));
  select lease.* into current_lease from timegenie.tracking_leases lease
  where lease.workspace_id=p_workspace_id for update;
  if found then
    previous_holder:=current_lease.holder_device_id;
    if current_lease.holder_device_id is not null
       and current_lease.holder_device_id<>p_device_id
       and current_lease.expires_at>observed_at then
      select timegenie.direct_cloud_time_entry_project(
        p_workspace_id,entry.id,observed_at
      ) into active_entry
      from timegenie.time_entries entry
      where entry.workspace_id=p_workspace_id
        and entry.state in('running','paused') and entry.deleted_at is null
      order by entry.started_at desc,entry.id limit 1;
      return jsonb_build_object(
        'acquired',false,'holderDeviceId',current_lease.holder_device_id,
        'expiresAt',current_lease.expires_at,'activeEntry',active_entry
      );
    end if;
  end if;
  insert into timegenie.tracking_leases(
    workspace_id,holder_device_id,lease_token,expires_at,last_confirmed_at,updated_at,version
  ) values(
    p_workspace_id,p_device_id,p_lease_token,observed_at+interval '45 seconds',
    observed_at,observed_at,1
  ) on conflict(workspace_id) do update set
    holder_device_id=excluded.holder_device_id,lease_token=excluded.lease_token,
    expires_at=excluded.expires_at,last_confirmed_at=excluded.last_confirmed_at,
    updated_at=excluded.updated_at,version=timegenie.tracking_leases.version+1
  returning * into current_lease;
  select timegenie.direct_cloud_time_entry_project(
    p_workspace_id,entry.id,observed_at
  ) into active_entry
    from timegenie.time_entries entry
    where entry.workspace_id=p_workspace_id
      and entry.state in('running','paused') and entry.deleted_at is null
    order by entry.started_at desc,entry.id limit 1;
  return jsonb_build_object(
    'acquired',true,'leaseToken',p_lease_token,'expiresAt',current_lease.expires_at,
    'version',current_lease.version,'tookOverFrom',previous_holder,
    'activeEntry',active_entry
  );
end $$;

create or replace function timegenie.direct_cloud_tracking_heartbeat(
  p_workspace_id uuid,p_device_id uuid,p_expected_epoch bigint,p_lease_token uuid
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare updated_row timegenie.tracking_leases%rowtype;
declare observed_at timestamptz:=now();
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  if p_lease_token is null then raise exception 'VALIDATION_ERROR: 缺少租约令牌'; end if;
  update timegenie.tracking_leases set
    expires_at=observed_at+interval '45 seconds',last_confirmed_at=observed_at,
    updated_at=observed_at,version=version+1
  where workspace_id=p_workspace_id and holder_device_id=p_device_id
    and lease_token=p_lease_token and expires_at>observed_at
  returning * into updated_row;
  if not found then raise exception 'LEASE_LOST: 当前设备不再持有计时协调租约'; end if;
  return jsonb_build_object(
    'renewed',true,'leaseToken',p_lease_token,
    'expiresAt',updated_row.expires_at,'version',updated_row.version
  );
end $$;

create or replace function timegenie.direct_cloud_timer_takeover(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,
  p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb;
declare entry_row timegenie.time_entries%rowtype; recovery jsonb;
declare entry_id uuid; expected_version bigint; lease_token uuid;
declare current_lease timegenie.tracking_leases%rowtype; previous_holder uuid;
declare observed_at timestamptz:=now(); active_id uuid; active_entry jsonb;
declare slices_value jsonb;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object(
    'expectedEpoch',p_expected_epoch,
    'payload',coalesce(p_payload,'{}'::jsonb)-'leaseToken'
  );
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':active-timer',0));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':tracking-lease',0));
  cached:=timegenie.direct_cloud_operation_result(
    p_workspace_id,p_operation_id,'direct_timer_takeover',request_value
  );
  if cached is not null then return cached; end if;
  entry_id:=nullif(p_payload->>'entryId','')::uuid;
  expected_version:=nullif(p_payload->>'expectedVersion','')::bigint;
  lease_token:=nullif(p_payload->>'leaseToken','')::uuid;
  if entry_id is null or expected_version is null or lease_token is null then
    raise exception 'VALIDATION_ERROR: 计时接管参数不完整';
  end if;
  select lease.* into current_lease from timegenie.tracking_leases lease
  where lease.workspace_id=p_workspace_id for update;
  if found then
    previous_holder:=current_lease.holder_device_id;
    if current_lease.holder_device_id is not null
       and current_lease.holder_device_id<>p_device_id
       and current_lease.expires_at>observed_at then
      select timegenie.direct_cloud_time_entry_project(
        p_workspace_id,entry.id,observed_at
      ) into active_entry
      from timegenie.time_entries entry
      where entry.workspace_id=p_workspace_id
        and entry.state in('running','paused') and entry.deleted_at is null
      order by entry.started_at desc,entry.id limit 1;
      raise exception 'LEASE_LOST: 计时协调租约仍由其他设备持有'
        using detail=jsonb_build_object(
          'code','LEASE_LOST',
          'holderDeviceId',current_lease.holder_device_id,
          'expiresAt',current_lease.expires_at,
          'activeEntry',active_entry
        )::text;
    end if;
  end if;
  select entry.* into entry_row from timegenie.time_entries entry
  where entry.workspace_id=p_workspace_id and entry.id=entry_id
    and entry.deleted_at is null
  for update;
  if not found then raise exception 'NOT_FOUND: 计时记录不存在'; end if;
  if entry_row.version<>expected_version then
    raise exception 'VERSION_CONFLICT: 计时版本已变化，请重新加载';
  end if;
  if entry_row.state not in('running','paused') then
    raise exception 'VERSION_CONFLICT: 计时已经结束，请重新加载';
  end if;

  insert into timegenie.tracking_leases(
    workspace_id,holder_device_id,lease_token,expires_at,last_confirmed_at,updated_at,version
  ) values(
    p_workspace_id,p_device_id,lease_token,observed_at+interval '45 seconds',
    observed_at,observed_at,1
  ) on conflict(workspace_id) do update set
    holder_device_id=excluded.holder_device_id,lease_token=excluded.lease_token,
    expires_at=excluded.expires_at,last_confirmed_at=excluded.last_confirmed_at,
    updated_at=excluded.updated_at,version=timegenie.tracking_leases.version+1
  returning * into current_lease;

  recovery:=timegenie.direct_cloud_timer_recover_locked(
    p_workspace_id,p_device_id,entry_id,observed_at
  );
  active_id:=(recovery->>'activeEntryId')::uuid;
  active_entry:=timegenie.direct_cloud_time_entry_project(
    p_workspace_id,active_id,observed_at
  );
  select coalesce(jsonb_agg(timegenie.direct_cloud_time_entry_project(
    p_workspace_id,entry.id,observed_at)
    order by entry.work_date,entry.started_at,entry.id),'[]'::jsonb)
  into slices_value
  from timegenie.time_entries entry
  where entry.workspace_id=p_workspace_id and entry.deleted_at is null
    and coalesce(entry.timer_chain_id,entry.id)=coalesce(
      (select chain.timer_chain_id from timegenie.time_entries chain where chain.id=active_id),
      active_id
    );
  result_value:=timegenie.direct_cloud_mutation_response(
    p_workspace_id,p_device_id,p_operation_id,jsonb_build_object(
      'activeEntry',active_entry,'slices',slices_value,
      'recovered',(recovery->>'recovered')::boolean,
      'calendarRollover',(recovery->>'calendarRollover')::boolean,
      'continuityInterrupted',(recovery->>'continuityInterrupted')::boolean,
      'createdSliceCount',(recovery->>'createdSliceCount')::integer,
      'observedAt',(extract(epoch from observed_at)*1000)::bigint,
      'noOp',not(recovery->>'recovered')::boolean,
      'lease',jsonb_build_object(
        'holderDeviceId',p_device_id,'leaseToken',lease_token,
        'expiresAt',current_lease.expires_at,'version',current_lease.version,
        'tookOverFrom',previous_holder
      )
    ),array['time'],p_expected_epoch
  );
  perform timegenie.record_direct_cloud_operation(
    p_workspace_id,p_operation_id,p_device_id,'direct_timer_takeover',
    request_value,result_value
  );
  return result_value;
end $$;

revoke all on function timegenie.direct_cloud_timer_recover(uuid,uuid,uuid,bigint,jsonb)
  from public,anon;
revoke all on function timegenie.direct_cloud_timer_takeover(uuid,uuid,uuid,bigint,jsonb)
  from public,anon;
revoke all on function timegenie.direct_cloud_tracking_acquire(uuid,uuid,bigint,uuid)
  from public,anon;
revoke all on function timegenie.direct_cloud_tracking_heartbeat(uuid,uuid,bigint,uuid)
  from public,anon;
grant execute on function timegenie.direct_cloud_timer_recover(uuid,uuid,uuid,bigint,jsonb)
  to authenticated;
grant execute on function timegenie.direct_cloud_timer_takeover(uuid,uuid,uuid,bigint,jsonb)
  to authenticated;
grant execute on function timegenie.direct_cloud_tracking_acquire(uuid,uuid,bigint,uuid)
  to authenticated;
grant execute on function timegenie.direct_cloud_tracking_heartbeat(uuid,uuid,bigint,uuid)
  to authenticated;

notify pgrst,'reload schema';
