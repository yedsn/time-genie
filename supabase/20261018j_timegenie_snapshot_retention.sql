begin;

alter table timegenie.workspace_history_settings
  add column if not exists snapshot_retention_days integer not null default 30
  check(snapshot_retention_days between 1 and 365);

update timegenie.workspace_history_settings
set snapshot_retention_days=automatic_snapshot_retention_days
where snapshot_retention_days=30 and automatic_snapshot_retention_days<>30;

update timegenie.workspace_snapshots
set expires_at=null
where protected and expires_at is not null;

create or replace function timegenie.workspace_snapshot_retention_days(p_workspace_id uuid)
returns integer language sql stable security definer set search_path=timegenie,extensions,pg_catalog as $$
  select coalesce(
    (select snapshot_retention_days from timegenie.workspace_history_settings where workspace_id=p_workspace_id),
    30
  );
$$;

create or replace function timegenie.workspace_snapshot_apply_retention()
returns trigger language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
begin
  if new.protected then
    new.expires_at:=null;
  else
    new.expires_at:=coalesce(new.created_at,now())
      + make_interval(days=>timegenie.workspace_snapshot_retention_days(new.workspace_id));
  end if;
  return new;
end $$;

drop trigger if exists workspace_snapshot_apply_retention on timegenie.workspace_snapshots;
create trigger workspace_snapshot_apply_retention
before insert or update of protected,created_at on timegenie.workspace_snapshots
for each row execute function timegenie.workspace_snapshot_apply_retention();

create or replace function timegenie.workspace_snapshot_cleanup(p_workspace_id uuid,p_device_id uuid)
returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare affected integer:=0; deleted_now integer; manual_limit integer; retention_days integer;
begin
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  insert into timegenie.workspace_history_settings(workspace_id) values(p_workspace_id) on conflict(workspace_id) do nothing;
  delete from timegenie.workspace_restore_locks where workspace_id=p_workspace_id and expires_at<=now();
  select manual_snapshot_limit,snapshot_retention_days into manual_limit,retention_days
  from timegenie.workspace_history_settings where workspace_id=p_workspace_id;

  delete from timegenie.workspace_snapshots
  where workspace_id=p_workspace_id and not protected
    and ((expires_at is not null and expires_at<=now())
      or created_at<now()-make_interval(days=>coalesce(retention_days,30)))
    and id not in(
      select target_snapshot_id from timegenie.workspace_restore_locks where workspace_id=p_workspace_id
      union
      select safety_snapshot_id from timegenie.workspace_restore_locks
      where workspace_id=p_workspace_id and safety_snapshot_id is not null
    );
  get diagnostics deleted_now=row_count; affected:=affected+deleted_now;

  delete from timegenie.workspace_snapshots
  where id in(
    select id from timegenie.workspace_snapshots
    where workspace_id=p_workspace_id and snapshot_type='manual' and not protected and status='ready'
      and id not in(select target_snapshot_id from timegenie.workspace_restore_locks where workspace_id=p_workspace_id)
    order by created_at desc,id desc offset coalesce(manual_limit,20)
  );
  get diagnostics deleted_now=row_count; affected:=affected+deleted_now;
  return jsonb_build_object('deletedCount',affected);
end $$;

create or replace function timegenie.workspace_snapshot_list(p_workspace_id uuid,p_device_id uuid)
returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
begin
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  perform timegenie.workspace_snapshot_cleanup(p_workspace_id,p_device_id);
  return coalesce((select jsonb_agg(jsonb_build_object('id',id,'snapshotType',snapshot_type,'name',name,'note',note,'status',status,'formatVersion',format_version,'baseChangeSeq',base_change_seq,'workspaceEpoch',workspace_epoch,'entityCounts',entity_counts,'groupHashes',group_hashes,'contentHash',content_hash,'sizeBytes',size_bytes,'chunkCount',chunk_count,'deviceName',created_by_device_name,'protected',protected,'protectedAt',protected_at,'expiresAt',expires_at,'createdAt',created_at,'completedAt',completed_at,'failureReason',failure_reason) order by created_at desc,id desc) from timegenie.workspace_snapshots where workspace_id=p_workspace_id),'[]'::jsonb);
end $$;

create or replace function timegenie.workspace_snapshot_settings_get(p_workspace_id uuid,p_device_id uuid)
returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare frequency_value text; automatic_days integer; retention_days integer;
begin
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  insert into timegenie.workspace_history_settings(workspace_id) values(p_workspace_id) on conflict(workspace_id) do nothing;
  select automatic_snapshot_frequency,automatic_snapshot_retention_days,snapshot_retention_days
    into frequency_value,automatic_days,retention_days
  from timegenie.workspace_history_settings where workspace_id=p_workspace_id;
  return jsonb_build_object(
    'automaticSnapshotFrequency',frequency_value,
    'automaticSnapshotRetentionDays',automatic_days,
    'snapshotRetentionDays',retention_days
  );
end $$;

create or replace function timegenie.workspace_snapshot_settings_update_with_retention(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,
  p_automatic_snapshot_frequency text,p_snapshot_retention_days integer
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare result_value jsonb;
begin
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  if p_automatic_snapshot_frequency not in('off','6h','12h','1d','3d','7d') then
    raise exception 'VALIDATION_ERROR: invalid automatic snapshot frequency';
  end if;
  if p_snapshot_retention_days not between 1 and 365 then
    raise exception 'VALIDATION_ERROR: snapshot retention must be between 1 and 365 days';
  end if;
  insert into timegenie.workspace_history_settings(
    workspace_id,automatic_snapshot_frequency,automatic_snapshot_retention_days,
    pre_restore_snapshot_retention_days,snapshot_retention_days,updated_at
  ) values(
    p_workspace_id,p_automatic_snapshot_frequency,p_snapshot_retention_days,
    p_snapshot_retention_days,p_snapshot_retention_days,now()
  )
  on conflict(workspace_id) do update set
    automatic_snapshot_frequency=excluded.automatic_snapshot_frequency,
    automatic_snapshot_retention_days=excluded.automatic_snapshot_retention_days,
    pre_restore_snapshot_retention_days=excluded.pre_restore_snapshot_retention_days,
    snapshot_retention_days=excluded.snapshot_retention_days,
    updated_at=excluded.updated_at;

  update timegenie.workspace_snapshots
  set expires_at=created_at+make_interval(days=>p_snapshot_retention_days)
  where workspace_id=p_workspace_id and not protected;

  result_value:=timegenie.workspace_snapshot_settings_get(p_workspace_id,p_device_id);
  perform timegenie.operation_history_upsert(
    p_workspace_id,p_operation_id,p_device_id,'workspace_snapshot_settings.update',
    jsonb_build_object('entityType','workspace_snapshot_settings','entityId',p_workspace_id,
      'payload',jsonb_build_object(
        'automatic_snapshot_frequency',p_automatic_snapshot_frequency,
        'snapshot_retention_days',p_snapshot_retention_days
      )),
    jsonb_build_object('entityType','workspace_snapshot_settings','entityId',p_workspace_id,
      'authoritative',jsonb_build_object(
        'automatic_snapshot_frequency',p_automatic_snapshot_frequency,
        'snapshot_retention_days',p_snapshot_retention_days
      ))
  );
  perform timegenie.workspace_snapshot_cleanup(p_workspace_id,p_device_id);
  return result_value;
end $$;

create or replace function timegenie.workspace_snapshot_settings_update(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_automatic_snapshot_frequency text
) returns jsonb language sql security definer set search_path=timegenie,extensions,pg_catalog as $$
  select timegenie.workspace_snapshot_settings_update_with_retention(
    p_workspace_id,p_device_id,p_operation_id,p_automatic_snapshot_frequency,
    timegenie.workspace_snapshot_retention_days(p_workspace_id)
  );
$$;

create or replace function timegenie.workspace_snapshot_ensure_automatic_at(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_at timestamptz
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare frequency_value text; slot_key_value text; existing_id uuid; created_result jsonb; created_id uuid; retention_days integer;
begin
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  insert into timegenie.workspace_history_settings(workspace_id) values(p_workspace_id) on conflict(workspace_id) do nothing;
  perform timegenie.workspace_snapshot_cleanup(p_workspace_id,p_device_id);
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

revoke execute on function timegenie.workspace_snapshot_retention_days(uuid) from public,anon,authenticated;
revoke execute on function timegenie.workspace_snapshot_apply_retention() from public,anon,authenticated;
revoke execute on function timegenie.workspace_snapshot_settings_update_with_retention(uuid,uuid,uuid,text,integer) from public,anon;
grant execute on function timegenie.workspace_snapshot_settings_update_with_retention(uuid,uuid,uuid,text,integer) to authenticated;

alter function timegenie.workspace_snapshot_create(uuid,uuid,uuid,text,text,text,boolean)
  set statement_timeout='120s';
alter function timegenie.workspace_snapshot_ensure_automatic_at(uuid,uuid,uuid,timestamptz)
  set statement_timeout='120s';
alter function timegenie.workspace_snapshot_ensure_automatic(uuid,uuid,uuid)
  set statement_timeout='120s';

notify pgrst,'reload schema';

commit;
