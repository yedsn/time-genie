-- TimeGenie incremental patch for coalesced local-first timer snapshots.
-- Run after 20261004d_timegenie_session_revocation_patch.sql.

begin;

do $$
begin
  if to_regprocedure('timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,jsonb)') is null then
    raise exception 'TIMEGENIE_CLOUD_PATCH_MISSING: deploy the earlier cloud sync patches first';
  end if;
end $$;

create or replace function timegenie.cloud_apply_patch(
  p_workspace_id uuid,
  p_device_id uuid,
  p_operation_id uuid,
  p_operation_type text,
  p_entity_type text,
  p_entity_id text,
  p_base_version bigint,
  p_payload_version bigint,
  p_coalesced_count bigint,
  p_payload jsonb
) returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare current_version bigint; final_version bigint; cached jsonb; result_value jsonb; row_data jsonb; request_value jsonb;
begin
  if p_entity_type <> 'time_entry' then
    return timegenie.cloud_apply_patch(
      p_workspace_id, p_device_id, p_operation_id, p_operation_type, p_entity_type,
      p_entity_id, p_base_version, p_payload
    );
  end if;

  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not exists(select 1 from timegenie.devices where id = p_device_id and workspace_id = p_workspace_id and revoked_at is null) then
    raise exception 'DEVICE_NOT_REGISTERED';
  end if;

  row_data := coalesce(p_payload, '{}'::jsonb);
  request_value := jsonb_build_object(
    'deviceId', p_device_id,
    'entityType', p_entity_type,
    'entityId', p_entity_id,
    'baseVersion', p_base_version,
    'payloadVersion', p_payload_version,
    'coalescedCount', p_coalesced_count,
    'payload', row_data
  );
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text || ':time_entry:' || p_entity_id, 0));
  cached := timegenie.processed_operation_result(
    p_workspace_id, p_operation_id, p_operation_type, request_value
  );
  if cached is not null then return cached; end if;

  if p_entity_id is null or row_data->>'id' is distinct from p_entity_id then
    raise exception 'VALIDATION_ERROR: entity id does not match payload id';
  end if;
  final_version := (row_data->>'version')::bigint;
  if p_payload_version is null or final_version is distinct from p_payload_version then
    raise exception 'VERSION_CONFLICT: declared payload version % does not match payload version %', p_payload_version, row_data->>'version';
  end if;
  if p_coalesced_count is null or p_coalesced_count < 1 then
    raise exception 'VERSION_CONFLICT: coalesced timer operation count must be positive';
  end if;
  if p_base_version is null and final_version <> p_coalesced_count then
    raise exception 'VERSION_CONFLICT: new timer payload version % does not match local operation count %', final_version, p_coalesced_count;
  elsif p_base_version is not null and final_version <> p_base_version + p_coalesced_count then
    raise exception 'VERSION_CONFLICT: timer payload version % does not match base version % plus local operation count %', final_version, p_base_version, p_coalesced_count;
  end if;

  select version into current_version
  from timegenie.time_entries
  where workspace_id = p_workspace_id and id = p_entity_id::uuid;
  if p_base_version is null and current_version is not null then
    raise exception 'SYNC_CONFLICT: expected new entity, current version %', current_version;
  elsif p_base_version is not null and current_version is distinct from p_base_version then
    raise exception 'SYNC_CONFLICT: expected version %, current version %', p_base_version, current_version;
  end if;

  insert into timegenie.time_entries(id, workspace_id, work_date, kind, source_type, state, default_task_id, label_snapshot, started_at, ended_at, duration_seconds, note, origin_unassigned_session_id, created_at, updated_at, version, created_by_device_id, updated_by_device_id, deleted_at)
  values((row_data->>'id')::uuid, p_workspace_id, (row_data->>'work_date')::date, row_data->>'kind', row_data->>'source_type', row_data->>'state', nullif(row_data->>'default_task_id','')::uuid, row_data->>'label_snapshot', to_timestamp((row_data->>'started_at')::double precision / 1000), case when row_data->>'ended_at' is null then null else to_timestamp((row_data->>'ended_at')::double precision / 1000) end, coalesce((row_data->>'duration_seconds')::bigint,0), row_data->>'note', nullif(row_data->>'origin_unassigned_session_id','')::uuid, to_timestamp(coalesce((row_data->>'created_at')::double precision, extract(epoch from now()) * 1000) / 1000), to_timestamp(coalesce((row_data->>'updated_at')::double precision, extract(epoch from now()) * 1000) / 1000), final_version, p_device_id, p_device_id, case when row_data->>'deleted_at' is null then null else to_timestamp((row_data->>'deleted_at')::double precision / 1000) end)
  on conflict(id) do update set work_date = excluded.work_date, kind = excluded.kind, source_type = excluded.source_type, state = excluded.state, default_task_id = excluded.default_task_id, label_snapshot = excluded.label_snapshot, started_at = excluded.started_at, ended_at = excluded.ended_at, duration_seconds = excluded.duration_seconds, note = excluded.note, origin_unassigned_session_id = excluded.origin_unassigned_session_id, updated_at = excluded.updated_at, version = excluded.version, updated_by_device_id = p_device_id, deleted_at = excluded.deleted_at;
  delete from timegenie.time_segments where entry_id = p_entity_id::uuid;
  insert into timegenie.time_segments(id, workspace_id, entry_id, sequence_no, started_at, ended_at, duration_seconds)
  select (item->>'id')::uuid, p_workspace_id, p_entity_id::uuid, (item->>'sequence_no')::integer, to_timestamp((item->>'started_at')::double precision / 1000), case when item->>'ended_at' is null then null else to_timestamp((item->>'ended_at')::double precision / 1000) end, coalesce((item->>'duration_seconds')::bigint,0)
  from jsonb_array_elements(coalesce(row_data->'segments','[]'::jsonb)) item;
  delete from timegenie.time_allocations where entry_id = p_entity_id::uuid;
  insert into timegenie.time_allocations(id, workspace_id, entry_id, task_id, minutes, note, created_at, updated_at, version)
  select (item->>'id')::uuid, p_workspace_id, p_entity_id::uuid, (item->>'task_id')::uuid, (item->>'minutes')::integer, item->>'note', to_timestamp((item->>'created_at')::double precision / 1000), to_timestamp((item->>'updated_at')::double precision / 1000), coalesce((item->>'version')::bigint,1)
  from jsonb_array_elements(coalesce(row_data->'allocations','[]'::jsonb)) item;
  if row_data->'work_day' is not null and row_data->'work_day' <> 'null'::jsonb then
    insert into timegenie.work_days(workspace_id, work_date, timezone, work_period_text, note, settled_at, created_at, updated_at, version)
    values(p_workspace_id, (row_data->'work_day'->>'work_date')::date, row_data->'work_day'->>'timezone', row_data->'work_day'->>'work_period_text', row_data->'work_day'->>'note', case when row_data->'work_day'->>'settled_at' is null then null else to_timestamp((row_data->'work_day'->>'settled_at')::double precision / 1000) end, to_timestamp((row_data->'work_day'->>'created_at')::double precision / 1000), to_timestamp((row_data->'work_day'->>'updated_at')::double precision / 1000), coalesce((row_data->'work_day'->>'version')::bigint,1))
    on conflict(workspace_id, work_date) do update set timezone = excluded.timezone, work_period_text = excluded.work_period_text, note = excluded.note, settled_at = excluded.settled_at, updated_at = excluded.updated_at, version = excluded.version
    where timegenie.work_days.version <= excluded.version;
  end if;

  result_value := jsonb_build_object('entityType','time_entry','entityId',p_entity_id,'version',final_version);
  perform timegenie.record_processed_operation(
    p_workspace_id, p_operation_id, p_device_id, p_operation_type, request_value, result_value
  );
  return result_value;
end $$;

grant execute on function timegenie.cloud_apply_patch(uuid, uuid, uuid, text, text, text, bigint, bigint, bigint, jsonb) to authenticated;

commit;
