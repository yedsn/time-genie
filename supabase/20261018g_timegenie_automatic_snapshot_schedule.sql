-- TimeGenie configurable automatic full-workspace snapshots. Idempotent upgrade patch.

alter table timegenie.workspace_history_settings
  add column if not exists automatic_snapshot_frequency text not null default '1d';

do $$ begin
  if not exists(
    select 1 from pg_constraint
    where conrelid='timegenie.workspace_history_settings'::regclass
      and conname='workspace_history_settings_snapshot_frequency_check'
  ) then
    alter table timegenie.workspace_history_settings
      add constraint workspace_history_settings_snapshot_frequency_check
      check(automatic_snapshot_frequency in('off','6h','12h','1d','3d','7d'));
  end if;
end $$;

alter table timegenie.workspace_snapshots
  add column if not exists schedule_slot_key text;

create unique index if not exists uq_workspace_snapshots_automatic_slot
  on timegenie.workspace_snapshots(workspace_id,schedule_slot_key)
  where snapshot_type='automatic' and schedule_slot_key is not null;

create or replace function timegenie.workspace_snapshot_slot_key(p_frequency text,p_at timestamptz default clock_timestamp())
returns text language plpgsql immutable as $$
declare seconds_per_slot bigint; slot_start bigint;
begin
  seconds_per_slot:=case p_frequency when '6h' then 21600 when '12h' then 43200 when '1d' then 86400 when '3d' then 259200 when '7d' then 604800 else null end;
  if seconds_per_slot is null then raise exception 'VALIDATION_ERROR: invalid automatic snapshot frequency'; end if;
  slot_start:=floor(extract(epoch from p_at)/seconds_per_slot)::bigint*seconds_per_slot;
  return p_frequency||':'||to_char(to_timestamp(slot_start) at time zone 'UTC','YYYY-MM-DD"T"HH24:MI:SS"Z"');
end $$;

-- Re-deploy the existing full-snapshot builder with an internal schedule-slot
-- context. Public automatic calls keep the legacy daily behavior, while the
-- scheduler can create multiple UTC slots per day without first pretending the
-- snapshot is manual (which would affect manual retention and audit metadata).
create or replace function timegenie.workspace_snapshot_create(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_snapshot_type text,p_name text,p_note text default null,p_lazy boolean default false
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare raw_snapshot jsonb; grouped_snapshot jsonb; snapshot_id_value uuid; existing_id uuid; change_seq_value bigint; epoch_value bigint; device_name_value text;
  counts jsonb; group_hashes_value jsonb:='{}'::jsonb; hash_value text; total_bytes bigint:=0; max_bytes bigint; expiry timestamptz; c record; row_value jsonb;
  rows_value jsonb; pending_rows jsonb; candidate jsonb; payload jsonb; payload_bytes integer; chunk_index_value integer:=0; retention_auto integer; retention_pre integer;
  verified jsonb; failure_message text; scheduled_slot_key text:=nullif(current_setting('timegenie.snapshot_schedule_slot_key',true),'');
begin
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  if p_snapshot_type not in('manual','automatic','pre_restore') or nullif(btrim(p_name),'') is null or length(btrim(p_name))>120 then raise exception 'VALIDATION_ERROR: invalid snapshot request'; end if;
  if p_snapshot_type='automatic' then
    if scheduled_slot_key is not null then
      select id into existing_id from timegenie.workspace_snapshots where workspace_id=p_workspace_id and snapshot_type='automatic' and schedule_slot_key=scheduled_slot_key and status in('creating','ready') order by status='ready' desc limit 1;
    else
      select id into existing_id from timegenie.workspace_snapshots where workspace_id=p_workspace_id and snapshot_type='automatic' and snapshot_day=current_date and status in('creating','ready') order by status='ready' desc limit 1;
    end if;
    if existing_id is not null then return timegenie.workspace_snapshot_detail(p_workspace_id,p_device_id,existing_id); end if;
    select coalesce(max(base_change_seq),0) into change_seq_value from timegenie.workspace_snapshots where workspace_id=p_workspace_id and snapshot_type='automatic' and status='ready';
    if p_lazy and not exists(select 1 from timegenie.workspace_changes where workspace_id=p_workspace_id and change_seq>change_seq_value) then return jsonb_build_object('skipped',true,'reason','NO_CHANGES'); end if;
  end if;
  select coalesce(max(change_seq),0) into change_seq_value from timegenie.workspace_changes where workspace_id=p_workspace_id;
  select coalesce(workspace_epoch,1) into epoch_value from timegenie.workspaces where id=p_workspace_id;
  select coalesce(device_name,'未知设备') into device_name_value from timegenie.devices where workspace_id=p_workspace_id and id=p_device_id;
  insert into timegenie.workspace_history_settings(workspace_id) values(p_workspace_id) on conflict(workspace_id) do nothing;
  select automatic_snapshot_retention_days,pre_restore_snapshot_retention_days,snapshot_max_bytes into retention_auto,retention_pre,max_bytes from timegenie.workspace_history_settings where workspace_id=p_workspace_id;
  expiry:=case p_snapshot_type when 'automatic' then now()+make_interval(days=>coalesce(retention_auto,30)) when 'pre_restore' then now()+make_interval(days=>coalesce(retention_pre,30)) else null end;
  if p_snapshot_type='automatic' and scheduled_slot_key is not null then
    insert into timegenie.workspace_snapshots(workspace_id,snapshot_type,snapshot_day,schedule_slot_key,name,note,status,base_change_seq,workspace_epoch,created_by_device_id,created_by_device_name,expires_at,created_at)
      values(p_workspace_id,p_snapshot_type,null,scheduled_slot_key,btrim(p_name),nullif(btrim(p_note),''),'creating',change_seq_value,epoch_value,p_device_id,device_name_value,expiry,clock_timestamp())
      on conflict(workspace_id,schedule_slot_key) where snapshot_type='automatic' and schedule_slot_key is not null do nothing returning id into snapshot_id_value;
    if snapshot_id_value is null then select id into snapshot_id_value from timegenie.workspace_snapshots where workspace_id=p_workspace_id and snapshot_type='automatic' and schedule_slot_key=scheduled_slot_key and status in('creating','ready') limit 1; return timegenie.workspace_snapshot_detail(p_workspace_id,p_device_id,snapshot_id_value); end if;
  elsif p_snapshot_type='automatic' then
    insert into timegenie.workspace_snapshots(workspace_id,snapshot_type,snapshot_day,name,note,status,base_change_seq,workspace_epoch,created_by_device_id,created_by_device_name,expires_at,created_at)
      values(p_workspace_id,p_snapshot_type,current_date,btrim(p_name),nullif(btrim(p_note),''),'creating',change_seq_value,epoch_value,p_device_id,device_name_value,expiry,clock_timestamp())
      on conflict(workspace_id,snapshot_type,snapshot_day) where snapshot_type='automatic' and status in('creating','ready') do nothing returning id into snapshot_id_value;
    if snapshot_id_value is null then select id into snapshot_id_value from timegenie.workspace_snapshots where workspace_id=p_workspace_id and snapshot_type='automatic' and snapshot_day=current_date and status in('creating','ready') limit 1; return timegenie.workspace_snapshot_detail(p_workspace_id,p_device_id,snapshot_id_value); end if;
  else
    insert into timegenie.workspace_snapshots(workspace_id,snapshot_type,name,note,status,base_change_seq,workspace_epoch,created_by_device_id,created_by_device_name,expires_at,created_at)
      values(p_workspace_id,p_snapshot_type,btrim(p_name),nullif(btrim(p_note),''),'creating',change_seq_value,epoch_value,p_device_id,device_name_value,expiry,clock_timestamp()) returning id into snapshot_id_value;
  end if;
  begin
    raw_snapshot:=timegenie.cloud_snapshot_get(p_workspace_id); grouped_snapshot:=timegenie.workspace_snapshot_grouped(raw_snapshot);
    counts:=timegenie.workspace_snapshot_counts(raw_snapshot); hash_value:=timegenie.workspace_snapshot_hash(grouped_snapshot);
    for c in select * from timegenie.workspace_snapshot_entity_catalog() order by sort_order loop
      group_hashes_value:=group_hashes_value||jsonb_build_object(c.group_name,timegenie.workspace_snapshot_hash(grouped_snapshot->c.group_name));
      rows_value:=grouped_snapshot->c.group_name->c.entity_name; pending_rows:='[]'::jsonb;
      for row_value in select value from jsonb_array_elements(rows_value) loop
        candidate:=jsonb_build_object(c.entity_name,pending_rows||jsonb_build_array(row_value));
        if octet_length(candidate::text)>16777216 then
          if jsonb_array_length(pending_rows)=0 then raise exception 'SNAPSHOT_ROW_TOO_LARGE: %',c.entity_name; end if;
          payload:=jsonb_build_object(c.entity_name,pending_rows); payload_bytes:=octet_length(payload::text);
          if chunk_index_value>=64 then raise exception 'SNAPSHOT_TOO_MANY_CHUNKS'; end if;
          insert into timegenie.workspace_snapshot_chunks values(snapshot_id_value,p_workspace_id,chunk_index_value,c.group_name,c.entity_name,payload,timegenie.workspace_snapshot_hash(payload),payload_bytes,jsonb_array_length(pending_rows));
          total_bytes:=total_bytes+payload_bytes; chunk_index_value:=chunk_index_value+1; pending_rows:=jsonb_build_array(row_value);
        else pending_rows:=pending_rows||jsonb_build_array(row_value); end if;
      end loop;
      payload:=jsonb_build_object(c.entity_name,pending_rows); payload_bytes:=octet_length(payload::text);
      if chunk_index_value>=64 then raise exception 'SNAPSHOT_TOO_MANY_CHUNKS'; end if;
      insert into timegenie.workspace_snapshot_chunks values(snapshot_id_value,p_workspace_id,chunk_index_value,c.group_name,c.entity_name,payload,timegenie.workspace_snapshot_hash(payload),payload_bytes,jsonb_array_length(pending_rows));
      total_bytes:=total_bytes+payload_bytes; chunk_index_value:=chunk_index_value+1;
      if total_bytes>coalesce(max_bytes,104857600) then raise exception 'SNAPSHOT_TOO_LARGE'; end if;
    end loop;
    verified:=timegenie.workspace_snapshot_rehydrate(snapshot_id_value);
    if verified<>grouped_snapshot or timegenie.workspace_snapshot_hash(verified)<>hash_value then raise exception 'SNAPSHOT_CHECKSUM_FAILED'; end if;
    update timegenie.workspace_snapshots set status='ready',entity_counts=counts,group_hashes=group_hashes_value,content_hash=hash_value,size_bytes=total_bytes,chunk_count=chunk_index_value,completed_at=now(),failure_reason=null where id=snapshot_id_value;
  exception when others then
    failure_message:=left(sqlerrm,500); delete from timegenie.workspace_snapshot_chunks where snapshot_id=snapshot_id_value;
    update timegenie.workspace_snapshots set status='failed',failure_reason=failure_message,completed_at=now(),size_bytes=0,chunk_count=0 where id=snapshot_id_value;
    return timegenie.workspace_snapshot_detail(p_workspace_id,p_device_id,snapshot_id_value);
  end;
  perform timegenie.operation_history_upsert(p_workspace_id,p_operation_id,p_device_id,'workspace_snapshot.create',jsonb_build_object('entityType','workspace_snapshot','entityId',snapshot_id_value,'payload',jsonb_build_object('name',p_name,'snapshot_type',p_snapshot_type)),jsonb_build_object('entityType','workspace_snapshot','entityId',snapshot_id_value,'snapshotId',snapshot_id_value,'name',p_name,'status','ready'));
  perform timegenie.workspace_snapshot_cleanup(p_workspace_id,p_device_id);
  return timegenie.workspace_snapshot_detail(p_workspace_id,p_device_id,snapshot_id_value);
end $$;

create or replace function timegenie.workspace_snapshot_settings_get(p_workspace_id uuid,p_device_id uuid)
returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare frequency_value text; retention_days integer;
begin
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  insert into timegenie.workspace_history_settings(workspace_id) values(p_workspace_id) on conflict(workspace_id) do nothing;
  select automatic_snapshot_frequency,automatic_snapshot_retention_days into frequency_value,retention_days
  from timegenie.workspace_history_settings where workspace_id=p_workspace_id;
  return jsonb_build_object('automaticSnapshotFrequency',frequency_value,'automaticSnapshotRetentionDays',retention_days);
end $$;

create or replace function timegenie.workspace_snapshot_settings_update(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_automatic_snapshot_frequency text
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare result_value jsonb;
begin
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  if p_automatic_snapshot_frequency not in('off','6h','12h','1d','3d','7d') then raise exception 'VALIDATION_ERROR: invalid automatic snapshot frequency'; end if;
  insert into timegenie.workspace_history_settings(workspace_id,automatic_snapshot_frequency,updated_at)
  values(p_workspace_id,p_automatic_snapshot_frequency,now())
  on conflict(workspace_id) do update set automatic_snapshot_frequency=excluded.automatic_snapshot_frequency,updated_at=excluded.updated_at;
  result_value:=timegenie.workspace_snapshot_settings_get(p_workspace_id,p_device_id);
  perform timegenie.operation_history_upsert(p_workspace_id,p_operation_id,p_device_id,'workspace_snapshot_settings.update',
    jsonb_build_object('entityType','workspace_snapshot_settings','entityId',p_workspace_id,'payload',jsonb_build_object('automatic_snapshot_frequency',p_automatic_snapshot_frequency)),
    jsonb_build_object('entityType','workspace_snapshot_settings','entityId',p_workspace_id,'authoritative',jsonb_build_object('automatic_snapshot_frequency',p_automatic_snapshot_frequency)));
  return result_value;
end $$;

create or replace function timegenie.workspace_snapshot_ensure_automatic_at(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_at timestamptz
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare frequency_value text; slot_key_value text; existing_id uuid; created_result jsonb; created_id uuid; retention_days integer;
begin
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  insert into timegenie.workspace_history_settings(workspace_id) values(p_workspace_id) on conflict(workspace_id) do nothing;
  select automatic_snapshot_frequency,automatic_snapshot_retention_days into frequency_value,retention_days
  from timegenie.workspace_history_settings where workspace_id=p_workspace_id;
  if frequency_value='off' then
    return jsonb_build_object('created',false,'frequency',frequency_value,'reason','DISABLED');
  end if;
  slot_key_value:=timegenie.workspace_snapshot_slot_key(frequency_value,p_at);
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||slot_key_value,0));
  select id into existing_id from timegenie.workspace_snapshots
  where workspace_id=p_workspace_id and snapshot_type='automatic' and schedule_slot_key=slot_key_value
  order by created_at desc limit 1;
  if existing_id is not null then
    if exists(select 1 from timegenie.workspace_snapshots where id=existing_id and status in('failed','corrupt')) then
      delete from timegenie.workspace_snapshots where id=existing_id;
    else
      return jsonb_build_object('created',false,'frequency',frequency_value,'slotKey',slot_key_value,'snapshot',timegenie.workspace_snapshot_detail(p_workspace_id,p_device_id,existing_id));
    end if;
  end if;
  perform set_config('timegenie.snapshot_schedule_slot_key',slot_key_value,true);
  created_result:=timegenie.workspace_snapshot_create(p_workspace_id,p_device_id,p_operation_id,'automatic',
    '自动快照 '||to_char(p_at at time zone 'UTC','YYYY-MM-DD HH24:MI')||' UTC','按'||frequency_value||'频率自动创建的完整恢复点',false);
  perform set_config('timegenie.snapshot_schedule_slot_key','',true);
  created_id:=(created_result->>'id')::uuid;
  return jsonb_build_object('created',true,'frequency',frequency_value,'slotKey',slot_key_value,'snapshot',timegenie.workspace_snapshot_detail(p_workspace_id,p_device_id,created_id));
end $$;

create or replace function timegenie.workspace_snapshot_ensure_automatic(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid
) returns jsonb language sql security definer set search_path=timegenie,extensions,pg_catalog as $$
  select timegenie.workspace_snapshot_ensure_automatic_at(p_workspace_id,p_device_id,p_operation_id,clock_timestamp());
$$;

revoke execute on function timegenie.workspace_snapshot_slot_key(text,timestamptz) from public,anon,authenticated;
revoke execute on function timegenie.workspace_snapshot_ensure_automatic_at(uuid,uuid,uuid,timestamptz) from public,anon,authenticated;
revoke execute on function timegenie.workspace_snapshot_settings_get(uuid,uuid),timegenie.workspace_snapshot_settings_update(uuid,uuid,uuid,text),timegenie.workspace_snapshot_ensure_automatic(uuid,uuid,uuid) from public,anon;
grant execute on function timegenie.workspace_snapshot_settings_get(uuid,uuid),timegenie.workspace_snapshot_settings_update(uuid,uuid,uuid,text),timegenie.workspace_snapshot_ensure_automatic(uuid,uuid,uuid) to authenticated;

notify pgrst,'reload schema';
