-- TimeGenie ordinary-timer sync authority.
-- Safe to reapply: lease traffic never mutates ordinary timers, timer writes
-- expose structured authoritative context, and processed operations can be
-- inspected only by the owning workspace.

begin;

create or replace function timegenie.unassigned_tick(
  p_workspace_id uuid,p_device_id uuid,p_lease_token uuid
) returns void language plpgsql security definer set search_path=timegenie,extensions,pg_catalog
as $$ begin
  -- Lease heartbeats may coordinate the persisted unassigned-time day anchor,
  -- but ordinary timer transitions are produced only by a client outbox row.
  perform timegenie.coordinate_unassigned_calendar_day_at(
    p_workspace_id,p_device_id,p_lease_token,clock_timestamp()
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
    select * into processed from timegenie.processed_operations
    where workspace_id=p_workspace_id and operation_id=p_operation_id;
  end if;
  if p_entity_id is not null then
    authoritative:=timegenie.cloud_incremental_entity_get(
      p_workspace_id,'time_entries',p_entity_id::text
    )->'data';
  end if;
  return jsonb_build_object(
    'processedOperation',case when processed.operation_id is null then null else jsonb_build_object(
      'operationId',processed.operation_id,
      'deviceId',processed.device_id,
      'operationType',processed.operation_type,
      'requestHash',processed.request_hash,
      'result',processed.result_json,
      'processedAt',processed.processed_at
    ) end,
    'entityId',p_entity_id,
    'currentVersion',case when authoritative is null then null else (authoritative->>'version')::bigint end,
    'authoritative',authoritative
  );
end $$;

create or replace function timegenie.timer_sync_context_get(
  p_workspace_id uuid,p_operation_id uuid,p_entity_id uuid,
  p_operation_type text,p_request jsonb
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog
as $$
declare context_value jsonb; expected_hash text; stored_hash text;
begin
  context_value:=timegenie.timer_sync_context_get(
    p_workspace_id,p_operation_id,p_entity_id
  );
  if context_value->'processedOperation' is null
     or context_value->'processedOperation'='null'::jsonb then
    return context_value;
  end if;
  expected_hash:=timegenie.operation_request_hash(p_operation_type,p_request);
  stored_hash:=context_value->'processedOperation'->>'requestHash';
  return jsonb_set(
    context_value,
    '{processedOperation,matchesRequest}',
    to_jsonb(
      context_value->'processedOperation'->>'operationType'=p_operation_type
      and (stored_hash is null or stored_hash=expected_hash)
    ),
    true
  );
end $$;

create or replace function timegenie.timer_write_lease_context(
  p_workspace_id uuid,p_device_id uuid
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog
as $$
declare lease_row timegenie.tracking_leases%rowtype;
begin
  select * into lease_row from timegenie.tracking_leases where workspace_id=p_workspace_id;
  return jsonb_build_object(
    'held',lease_row.holder_device_id=p_device_id and lease_row.expires_at>clock_timestamp(),
    'holderDeviceId',lease_row.holder_device_id,
    'expiresAt',lease_row.expires_at,
    'leaseVersion',lease_row.version
  );
end $$;

do $$ begin
  if to_regprocedure('timegenie.apply_calendar_time_entry_pre_timer_authority(uuid,uuid,uuid,text,text,bigint,jsonb,jsonb)') is null then
    alter function timegenie.apply_calendar_time_entry(uuid,uuid,uuid,text,text,bigint,jsonb,jsonb)
      rename to apply_calendar_time_entry_pre_timer_authority;
  end if;
end $$;

create or replace function timegenie.apply_calendar_time_entry(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_operation_type text,
  p_entity_id text,p_base_version bigint,p_payload jsonb,p_request jsonb
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog
as $$
declare processed timegenie.processed_operations%rowtype; expected_hash text; context_value jsonb;
declare authoritative jsonb; lease_context jsonb; result_value jsonb;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  expected_hash:=timegenie.operation_request_hash(p_operation_type,p_request);
  select * into processed from timegenie.processed_operations
  where workspace_id=p_workspace_id and operation_id=p_operation_id;
  if found then
    if processed.operation_type=p_operation_type
       and (processed.request_hash is null or processed.request_hash=expected_hash) then
      return processed.result_json;
    end if;
    context_value:=timegenie.timer_sync_context_get(
      p_workspace_id,p_operation_id,nullif(p_entity_id,'')::uuid
    );
    context_value:=jsonb_set(
      context_value,'{processedOperation,matchesRequest}','false'::jsonb,true
    );
    return jsonb_build_object(
      'accepted',false,
      'error',jsonb_build_object(
        'code','IDEMPOTENCY_CONFLICT',
        'message','operation_id was already used for different content'
      )
    )||context_value;
  end if;

  authoritative:=timegenie.cloud_incremental_entity_get(
    p_workspace_id,'time_entries',p_entity_id
  )->'data';
  if authoritative='null'::jsonb then authoritative:=null; end if;
  lease_context:=timegenie.timer_write_lease_context(p_workspace_id,p_device_id);
  if not coalesce((lease_context->>'held')::boolean,false) then
    return jsonb_build_object(
      'accepted',false,
      'error',jsonb_build_object('code','LEASE_NOT_HELD','message','tracking lease is not held by this device'),
      'currentVersion',case when authoritative is null then null else (authoritative->>'version')::bigint end,
      'authoritative',authoritative,
      'lease',lease_context
    );
  end if;
  if (p_base_version is null and authoritative is not null)
     or (p_base_version is not null and (authoritative is null or (authoritative->>'version')::bigint<>p_base_version)) then
    return jsonb_build_object(
      'accepted',false,
      'error',jsonb_build_object('code','SYNC_CONFLICT','message','timer version changed'),
      'currentVersion',case when authoritative is null then null else (authoritative->>'version')::bigint end,
      'authoritative',authoritative,
      'lease',lease_context
    );
  end if;
  result_value:=timegenie.apply_calendar_time_entry_pre_timer_authority(
    p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_id,
    p_base_version,p_payload,p_request
  );
  result_value:=result_value||jsonb_build_object('accepted',true,'alreadyProcessed',false);
  update timegenie.processed_operations set result_json=result_value
  where workspace_id=p_workspace_id and operation_id=p_operation_id;
  return result_value;
end $$;

create or replace function timegenie.tracking_lease_acquire(
  p_workspace_id uuid,p_device_id uuid,p_lease_token uuid
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog
as $$
declare current_lease timegenie.tracking_leases%rowtype; observed_at timestamptz:=clock_timestamp();
declare previous_holder uuid; expired_takeover boolean:=false; authoritative_timer jsonb;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':tracking-lease',0));
  select * into current_lease from timegenie.tracking_leases where workspace_id=p_workspace_id for update;
  if found then
    previous_holder:=current_lease.holder_device_id;
    expired_takeover:=current_lease.holder_device_id is not null
      and current_lease.holder_device_id<>p_device_id
      and current_lease.expires_at is not null and current_lease.expires_at<=observed_at;
    if current_lease.expires_at>observed_at and current_lease.holder_device_id<>p_device_id then
      return jsonb_build_object(
        'acquired',false,'holderDeviceId',current_lease.holder_device_id,
        'expiresAt',current_lease.expires_at,'takeover',false
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
  perform timegenie.unassigned_tick(p_workspace_id,p_device_id,p_lease_token);
  if expired_takeover then
    select timegenie.cloud_incremental_entity_get(p_workspace_id,'time_entries',entry.id::text)->'data'
      into authoritative_timer
    from timegenie.time_entries entry
    where entry.workspace_id=p_workspace_id and entry.source_type='timer'
      and entry.state in ('running','paused') and entry.deleted_at is null
    order by entry.started_at desc limit 1;
  end if;
  return jsonb_build_object(
    'acquired',true,'leaseToken',p_lease_token,'expiresAt',current_lease.expires_at,
    'version',current_lease.version,'takeover',expired_takeover,
    'previousHolderDeviceId',previous_holder,'authoritativeTimer',authoritative_timer
  );
end $$;

create or replace function timegenie.tracking_lease_renew(
  p_workspace_id uuid,p_device_id uuid,p_lease_token uuid
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog
as $$
declare updated_row timegenie.tracking_leases%rowtype; observed_at timestamptz:=clock_timestamp();
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  update timegenie.tracking_leases set
    expires_at=observed_at+interval '45 seconds',last_confirmed_at=observed_at,
    updated_at=observed_at,version=version+1
  where workspace_id=p_workspace_id and holder_device_id=p_device_id
    and lease_token=p_lease_token and expires_at>observed_at
  returning * into updated_row;
  if not found then raise exception 'LEASE_EXPIRED'; end if;
  perform timegenie.unassigned_tick(p_workspace_id,p_device_id,p_lease_token);
  return jsonb_build_object(
    'renewed',true,'leaseToken',p_lease_token,'expiresAt',updated_row.expires_at,
    'version',updated_row.version,'takeover',false
  );
end $$;

create or replace function timegenie.tracking_lease_release(
  p_workspace_id uuid,p_device_id uuid,p_lease_token uuid
) returns boolean language plpgsql security definer set search_path=timegenie,extensions,pg_catalog
as $$ begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  update timegenie.tracking_leases set holder_device_id=null,lease_token=null,
    expires_at=null,last_confirmed_at=clock_timestamp(),updated_at=clock_timestamp(),
    version=version+1
  where workspace_id=p_workspace_id and holder_device_id=p_device_id and lease_token=p_lease_token;
  return found;
end $$;

revoke execute on function timegenie.timer_write_lease_context(uuid,uuid) from public,anon,authenticated;
revoke execute on function timegenie.apply_calendar_time_entry_pre_timer_authority(uuid,uuid,uuid,text,text,bigint,jsonb,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.apply_calendar_time_entry(uuid,uuid,uuid,text,text,bigint,jsonb,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.unassigned_tick(uuid,uuid,uuid) from public,anon;
revoke execute on function timegenie.timer_sync_context_get(uuid,uuid,uuid) from public,anon;
revoke execute on function timegenie.timer_sync_context_get(uuid,uuid,uuid,text,jsonb) from public,anon;
revoke execute on function timegenie.tracking_lease_acquire(uuid,uuid,uuid) from public,anon;
revoke execute on function timegenie.tracking_lease_renew(uuid,uuid,uuid) from public,anon;
revoke execute on function timegenie.tracking_lease_release(uuid,uuid,uuid) from public,anon;
grant execute on function timegenie.timer_sync_context_get(uuid,uuid,uuid) to authenticated;
grant execute on function timegenie.timer_sync_context_get(uuid,uuid,uuid,text,jsonb) to authenticated;
grant execute on function timegenie.unassigned_tick(uuid,uuid,uuid) to authenticated;
grant execute on function timegenie.tracking_lease_acquire(uuid,uuid,uuid) to authenticated;
grant execute on function timegenie.tracking_lease_renew(uuid,uuid,uuid) to authenticated;
grant execute on function timegenie.tracking_lease_release(uuid,uuid,uuid) to authenticated;

commit;
notify pgrst, 'reload schema';
