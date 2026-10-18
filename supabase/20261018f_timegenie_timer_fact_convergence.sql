-- Multi-device timer facts converge by server acceptance order. Timer writes
-- no longer require a tracking lease; the lease remains an internal helper for
-- the shared unassigned-day boundary only. Safe to reapply.

begin;

create or replace function timegenie.timegenie_clamp_timer_stop_payload(
  p_payload jsonb,p_observed_at timestamptz default clock_timestamp()
) returns jsonb language plpgsql stable security definer set search_path=timegenie,extensions,pg_catalog
as $$
declare row_data jsonb:=coalesce(p_payload,'{}'::jsonb); observed_ms bigint; started_ms bigint; ended_ms bigint; sanitized_segments jsonb; total_seconds bigint;
begin
  if coalesce(row_data->>'state','')<>'ended' or row_data->'ended_at' is null then return row_data; end if;
  observed_ms:=floor(extract(epoch from p_observed_at)*1000)::bigint;
  started_ms:=coalesce((row_data->>'started_at')::bigint,observed_ms);
  ended_ms:=greatest(started_ms,least((row_data->>'ended_at')::bigint,observed_ms));
  select coalesce(jsonb_agg(
    item||jsonb_build_object(
      'ended_at',case when item->'ended_at' is null then null else greatest((item->>'started_at')::bigint,least((item->>'ended_at')::bigint,observed_ms)) end,
      'duration_seconds',case when item->'ended_at' is null then coalesce((item->>'duration_seconds')::bigint,0) else greatest(0,(greatest((item->>'started_at')::bigint,least((item->>'ended_at')::bigint,observed_ms))-(item->>'started_at')::bigint)/1000) end
    ) order by ordinal
  ),'[]'::jsonb),coalesce(sum(
    case when item->'ended_at' is null then coalesce((item->>'duration_seconds')::bigint,0) else greatest(0,(greatest((item->>'started_at')::bigint,least((item->>'ended_at')::bigint,observed_ms))-(item->>'started_at')::bigint)/1000) end
  ),0)
  into sanitized_segments,total_seconds
  from jsonb_array_elements(coalesce(row_data->'segments','[]'::jsonb)) with ordinality segment(item,ordinal);
  return row_data||jsonb_build_object(
    'ended_at',ended_ms,
    'updated_at',least(coalesce((row_data->>'updated_at')::bigint,ended_ms),observed_ms),
    'last_continuous_at',least(coalesce((row_data->>'last_continuous_at')::bigint,ended_ms),observed_ms),
    'duration_seconds',total_seconds,
    'segments',sanitized_segments
  );
end $$;

create or replace function timegenie.timer_sync_context_get(
  p_workspace_id uuid,p_operation_id uuid default null,p_entity_id uuid default null
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog
as $$
declare processed timegenie.processed_operations%rowtype; authoritative jsonb;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if p_operation_id is not null then
    select * into processed from timegenie.processed_operations where workspace_id=p_workspace_id and operation_id=p_operation_id;
  end if;
  if p_entity_id is not null then
    authoritative:=timegenie.cloud_incremental_entity_get(p_workspace_id,'time_entries',p_entity_id::text)->'data';
  end if;
  if authoritative is null or authoritative='null'::jsonb then
    select timegenie.cloud_incremental_entity_get(p_workspace_id,'time_entries',entry.id::text)->'data' into authoritative
    from timegenie.time_entries entry
    where entry.workspace_id=p_workspace_id and entry.source_type='timer' and entry.state in('running','paused') and entry.deleted_at is null
    order by entry.started_at desc,entry.id desc limit 1;
  end if;
  return jsonb_build_object(
    'processedOperation',case when processed.operation_id is null then null else jsonb_build_object(
      'operationId',processed.operation_id,'deviceId',processed.device_id,'operationType',processed.operation_type,
      'requestHash',processed.request_hash,'result',processed.result_json,'processedAt',processed.processed_at
    ) end,
    'entityId',p_entity_id,
    'currentVersion',case when authoritative is null then null else (authoritative->>'version')::bigint end,
    'authoritative',authoritative
  );
end $$;

create or replace function timegenie.apply_calendar_time_entry(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_operation_type text,
  p_entity_id text,p_base_version bigint,p_payload jsonb,p_request jsonb
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog
as $$
declare processed timegenie.processed_operations%rowtype; expected_hash text; context_value jsonb; authoritative jsonb; result_value jsonb; payload_value jsonb:=coalesce(p_payload,'{}'::jsonb);
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  expected_hash:=timegenie.operation_request_hash(p_operation_type,p_request);
  select * into processed from timegenie.processed_operations where workspace_id=p_workspace_id and operation_id=p_operation_id;
  if found then
    if processed.operation_type=p_operation_type and (processed.request_hash is null or processed.request_hash=expected_hash) then return processed.result_json; end if;
    context_value:=timegenie.timer_sync_context_get(p_workspace_id,p_operation_id,nullif(p_entity_id,'')::uuid);
    context_value:=jsonb_set(context_value,'{processedOperation,matchesRequest}','false'::jsonb,true);
    return jsonb_build_object('accepted',false,'error',jsonb_build_object('code','IDEMPOTENCY_CONFLICT','message','operation_id was already used for different content'))||context_value;
  end if;

  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':active-timer',0));
  authoritative:=timegenie.cloud_incremental_entity_get(p_workspace_id,'time_entries',p_entity_id)->'data';
  if authoritative='null'::jsonb then authoritative:=null; end if;
  if p_operation_type='timer_start' and p_base_version is null and authoritative is null then
    select timegenie.cloud_incremental_entity_get(p_workspace_id,'time_entries',entry.id::text)->'data' into authoritative
    from timegenie.time_entries entry where entry.workspace_id=p_workspace_id and entry.source_type='timer' and entry.state in('running','paused') and entry.deleted_at is null
    order by entry.started_at desc,entry.id desc limit 1;
  end if;
  if (p_base_version is null and authoritative is not null)
     or (p_base_version is not null and (authoritative is null or (authoritative->>'version')::bigint<>p_base_version)) then
    return jsonb_build_object(
      'accepted',false,'error',jsonb_build_object('code','SYNC_CONFLICT','message','timer fact already changed'),
      'currentVersion',case when authoritative is null then null else (authoritative->>'version')::bigint end,
      'authoritative',authoritative
    );
  end if;
  if p_operation_type='timer_stop' then payload_value:=timegenie.timegenie_clamp_timer_stop_payload(payload_value,clock_timestamp()); end if;
  result_value:=timegenie.apply_calendar_time_entry_pre_timer_authority(
    p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_id,p_base_version,payload_value,p_request
  );
  result_value:=result_value||jsonb_build_object('accepted',true,'alreadyProcessed',false);
  update timegenie.processed_operations set result_json=result_value where workspace_id=p_workspace_id and operation_id=p_operation_id;
  return result_value;
end $$;

-- Compatibility for clients released with the temporary preemption patch. It
-- validates the caller and returns the current lease without changing holder,
-- token, expiry or version. Timer writes no longer consume the returned token.
create or replace function timegenie.tracking_lease_takeover_for_timer_write(
  p_workspace_id uuid,p_device_id uuid,p_lease_token uuid
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog
as $$
declare current_lease timegenie.tracking_leases%rowtype;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  select * into current_lease from timegenie.tracking_leases where workspace_id=p_workspace_id;
  return jsonb_build_object(
    'acquired',true,'compatibilityOnly',true,'takeover',false,
    'requestedLeaseToken',p_lease_token,'holderDeviceId',current_lease.holder_device_id,
    'expiresAt',current_lease.expires_at,'version',coalesce(current_lease.version,0)
  );
end $$;

revoke execute on function timegenie.timegenie_clamp_timer_stop_payload(jsonb,timestamptz) from public,anon,authenticated;
revoke execute on function timegenie.apply_calendar_time_entry(uuid,uuid,uuid,text,text,bigint,jsonb,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.tracking_lease_takeover_for_timer_write(uuid,uuid,uuid) from public,anon;
grant execute on function timegenie.tracking_lease_takeover_for_timer_write(uuid,uuid,uuid) to authenticated;

commit;
notify pgrst, 'reload schema';
