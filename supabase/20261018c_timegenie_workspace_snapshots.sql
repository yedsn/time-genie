-- TimeGenie cloud workspace snapshots. Idempotent upgrade patch.
alter table timegenie.workspaces add column if not exists workspace_epoch bigint not null default 1 check(workspace_epoch > 0);

alter table timegenie.workspace_history_settings
  add column if not exists automatic_snapshot_retention_days integer not null default 30 check(automatic_snapshot_retention_days between 1 and 365),
  add column if not exists manual_snapshot_limit integer not null default 20 check(manual_snapshot_limit between 1 and 100),
  add column if not exists pre_restore_snapshot_retention_days integer not null default 30 check(pre_restore_snapshot_retention_days between 1 and 365),
  add column if not exists snapshot_max_bytes bigint not null default 104857600 check(snapshot_max_bytes between 1048576 and 104857600);

create table if not exists timegenie.workspace_snapshots (
  id uuid primary key default gen_random_uuid(), workspace_id uuid not null references timegenie.workspaces(id) on delete cascade,
  snapshot_type text not null check(snapshot_type in ('manual','automatic','pre_restore')), snapshot_day date,
  name text not null check(length(btrim(name)) between 1 and 120), note text,
  status text not null default 'creating' check(status in ('creating','ready','corrupt','failed')), format_version integer not null default 1,
  base_change_seq bigint not null default 0, workspace_epoch bigint not null default 1, entity_counts jsonb not null default '{}'::jsonb,
  group_hashes jsonb not null default '{}'::jsonb, content_hash text, size_bytes bigint not null default 0 check(size_bytes between 0 and 104857600),
  chunk_count integer not null default 0 check(chunk_count between 0 and 64), created_by_device_id uuid references timegenie.devices(id) on delete set null,
  created_by_device_name text not null, protected boolean not null default false, protected_at timestamptz, expires_at timestamptz,
  created_at timestamptz not null default now(), completed_at timestamptz, failure_reason text
);
alter table timegenie.workspace_snapshots add column if not exists group_hashes jsonb not null default '{}'::jsonb;
alter table timegenie.workspace_snapshots add column if not exists protected_at timestamptz;
create unique index if not exists uq_workspace_snapshots_auto_day on timegenie.workspace_snapshots(workspace_id,snapshot_type,snapshot_day) where snapshot_type='automatic' and status in('creating','ready');
create index if not exists idx_workspace_snapshots_list on timegenie.workspace_snapshots(workspace_id,created_at desc,id desc);
create index if not exists idx_workspace_snapshots_cleanup on timegenie.workspace_snapshots(workspace_id,protected,expires_at);

create table if not exists timegenie.workspace_snapshot_chunks (
  snapshot_id uuid not null references timegenie.workspace_snapshots(id) on delete cascade, workspace_id uuid not null references timegenie.workspaces(id) on delete cascade,
  chunk_index integer not null check(chunk_index between 0 and 63), group_name text not null check(group_name in ('planning','time_tracking','reports','shared_settings')),
  entity_name text not null, content_json jsonb not null, content_hash text not null, size_bytes integer not null check(size_bytes between 1 and 16777216),
  row_count integer not null default 0 check(row_count>=0), primary key(snapshot_id,chunk_index)
);
alter table timegenie.workspace_snapshot_chunks add column if not exists entity_name text;
alter table timegenie.workspace_snapshot_chunks add column if not exists row_count integer not null default 0 check(row_count>=0);
update timegenie.workspace_snapshot_chunks set entity_name=group_name where entity_name is null;
alter table timegenie.workspace_snapshot_chunks alter column entity_name set not null;
create index if not exists idx_workspace_snapshot_chunks_workspace on timegenie.workspace_snapshot_chunks(workspace_id,snapshot_id,chunk_index);

alter table timegenie.workspace_snapshots enable row level security;
alter table timegenie.workspace_snapshot_chunks enable row level security;
drop policy if exists workspace_owner on timegenie.workspace_snapshots;
create policy workspace_owner on timegenie.workspace_snapshots for select using(timegenie.is_workspace_owner(workspace_id));
drop policy if exists workspace_owner on timegenie.workspace_snapshot_chunks;
create policy workspace_owner on timegenie.workspace_snapshot_chunks for select using(timegenie.is_workspace_owner(workspace_id));

create or replace function timegenie.workspace_snapshot_hash(p_value jsonb) returns text language sql immutable as $$
  select encode(extensions.digest(coalesce(p_value,'null'::jsonb)::text,'sha256'),'hex');
$$;

create or replace function timegenie.workspace_snapshot_sanitize(p_value jsonb) returns jsonb language plpgsql immutable as $$
declare result_value jsonb:='{}'::jsonb; item jsonb; pair record;
begin
  if p_value is null then return null; end if;
  if jsonb_typeof(p_value)='array' then
    result_value:='[]'::jsonb;
    for item in select value from jsonb_array_elements(p_value) loop result_value:=result_value||jsonb_build_array(timegenie.workspace_snapshot_sanitize(item)); end loop;
    return result_value;
  elsif jsonb_typeof(p_value)<>'object' then return p_value; end if;
  for pair in select key,value from jsonb_each(p_value) order by key loop
    if lower(pair.key)=any(array['secret_ref','access_token','refresh_token','token','password','api_key','apikey','anon_key','service_role','service_role_key','lease_token','auth_session_id','created_by_device_id','updated_by_device_id','action_config','action_config_json','command']) then continue; end if;
    result_value:=result_value||jsonb_build_object(pair.key,timegenie.workspace_snapshot_sanitize(pair.value));
  end loop;
  return result_value;
end $$;

create or replace function timegenie.workspace_snapshot_entity_catalog()
returns table(entity_name text,group_name text,sort_order integer) language sql immutable as $$ values
  ('subjects','planning',10),('subject_work_schedules','planning',20),('subject_work_schedule_segments','planning',30),('work_days','planning',40),
  ('tasks','planning',50),('task_status_events','planning',60),('task_daily_estimates','planning',70),('task_recurrence_rules','planning',80),('task_occurrences','planning',90),
  ('time_entries','time_tracking',100),('time_segments','time_tracking',110),('time_allocations','time_tracking',120),('unassigned_sessions','time_tracking',130),
  ('unassigned_segments','time_tracking',140),('actual_time_adjustments','time_tracking',150),('report_templates','reports',160),('reports','reports',170),
  ('report_tasks','reports',180),('app_settings','shared_settings',190),('integration_configs','shared_settings',200),('external_bindings','shared_settings',210);
$$;

create or replace function timegenie.workspace_snapshot_normalized_entity(p_snapshot jsonb,p_entity_name text) returns jsonb language sql immutable as $$
  select coalesce(jsonb_agg(timegenie.workspace_snapshot_sanitize(value) order by timegenie.workspace_snapshot_sanitize(value)::text),'[]'::jsonb)
  from jsonb_array_elements(case when jsonb_typeof(p_snapshot->p_entity_name)='array' then p_snapshot->p_entity_name else '[]'::jsonb end);
$$;

create or replace function timegenie.workspace_snapshot_counts(p_snapshot jsonb) returns jsonb language sql immutable as $$
  select coalesce(jsonb_object_agg(c.entity_name,jsonb_array_length(timegenie.workspace_snapshot_normalized_entity(p_snapshot,c.entity_name)) order by c.sort_order),'{}'::jsonb)
  from timegenie.workspace_snapshot_entity_catalog() c;
$$;

create or replace function timegenie.workspace_snapshot_row_identity(p_entity_name text,p_row jsonb) returns text language sql immutable as $$
  select case p_entity_name when 'work_days' then p_row->>'work_date'
    when 'task_daily_estimates' then concat_ws('|',p_row->>'task_id',p_row->>'work_date')
    when 'task_occurrences' then concat_ws('|',p_row->>'task_id',p_row->>'occurrence_date')
    when 'report_tasks' then concat_ws('|',p_row->>'report_id',p_row->>'task_id')
    when 'app_settings' then p_row->>'key' when 'integration_configs' then p_row->>'provider'
    else coalesce(p_row->>'id',p_row->>'entity_id',p_row::text) end;
$$;

create or replace function timegenie.workspace_snapshot_grouped(p_snapshot jsonb) returns jsonb language plpgsql immutable as $$
declare result_value jsonb:=jsonb_build_object('planning','{}'::jsonb,'time_tracking','{}'::jsonb,'reports','{}'::jsonb,'shared_settings','{}'::jsonb); c record; group_value jsonb;
begin
  for c in select * from timegenie.workspace_snapshot_entity_catalog() order by sort_order loop
    group_value:=result_value->c.group_name;
    result_value:=jsonb_set(result_value,array[c.group_name],group_value||jsonb_build_object(c.entity_name,timegenie.workspace_snapshot_normalized_entity(p_snapshot,c.entity_name)));
  end loop;
  return result_value;
end $$;

create or replace function timegenie.workspace_snapshot_rehydrate(p_snapshot_id uuid) returns jsonb language plpgsql stable security definer set search_path=timegenie,extensions,pg_catalog as $$
declare result_value jsonb:=jsonb_build_object('planning','{}'::jsonb,'time_tracking','{}'::jsonb,'reports','{}'::jsonb,'shared_settings','{}'::jsonb); chunk record; group_value jsonb; rows_value jsonb;
begin
  for chunk in select * from timegenie.workspace_snapshot_chunks where snapshot_id=p_snapshot_id order by chunk_index loop
    if timegenie.workspace_snapshot_hash(chunk.content_json)<>chunk.content_hash then raise exception 'SNAPSHOT_CHUNK_CHECKSUM_MISMATCH'; end if;
    group_value:=result_value->chunk.group_name;
    rows_value:=coalesce(group_value->chunk.entity_name,'[]'::jsonb)||coalesce(chunk.content_json->chunk.entity_name,'[]'::jsonb);
    result_value:=jsonb_set(result_value,array[chunk.group_name],group_value||jsonb_build_object(chunk.entity_name,rows_value));
  end loop;
  return result_value;
end $$;

create or replace function timegenie.workspace_snapshot_detail(p_workspace_id uuid,p_device_id uuid,p_snapshot_id uuid) returns jsonb language plpgsql stable security definer set search_path=timegenie,extensions,pg_catalog as $$
declare result_value jsonb;
begin
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  select jsonb_build_object('id',s.id,'snapshotType',s.snapshot_type,'name',s.name,'note',s.note,'status',s.status,'formatVersion',s.format_version,'baseChangeSeq',s.base_change_seq,'workspaceEpoch',s.workspace_epoch,'entityCounts',s.entity_counts,'groupHashes',s.group_hashes,'contentHash',s.content_hash,'sizeBytes',s.size_bytes,'chunkCount',s.chunk_count,'deviceName',s.created_by_device_name,'protected',s.protected,'protectedAt',s.protected_at,'expiresAt',s.expires_at,'createdAt',s.created_at,'completedAt',s.completed_at,'failureReason',s.failure_reason) into result_value from timegenie.workspace_snapshots s where s.workspace_id=p_workspace_id and s.id=p_snapshot_id;
  if result_value is null then raise exception 'SNAPSHOT_NOT_FOUND'; end if; return result_value;
end $$;

create or replace function timegenie.workspace_snapshot_cleanup(p_workspace_id uuid,p_device_id uuid) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare affected integer:=0; deleted_now integer; manual_limit integer;
begin
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  insert into timegenie.workspace_history_settings(workspace_id) values(p_workspace_id) on conflict(workspace_id) do nothing;
  select manual_snapshot_limit into manual_limit from timegenie.workspace_history_settings where workspace_id=p_workspace_id;
  delete from timegenie.workspace_snapshots where workspace_id=p_workspace_id and not protected and expires_at is not null and expires_at<=now();
  get diagnostics deleted_now=row_count; affected:=affected+deleted_now;
  delete from timegenie.workspace_snapshots where id in(select id from timegenie.workspace_snapshots where workspace_id=p_workspace_id and snapshot_type='manual' and not protected and status='ready' order by created_at desc,id desc offset coalesce(manual_limit,20));
  get diagnostics deleted_now=row_count; affected:=affected+deleted_now;
  return jsonb_build_object('deletedCount',affected);
end $$;

create or replace function timegenie.workspace_snapshot_create(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_snapshot_type text,p_name text,p_note text default null,p_lazy boolean default false
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare raw_snapshot jsonb; grouped_snapshot jsonb; snapshot_id_value uuid; existing_id uuid; change_seq_value bigint; epoch_value bigint; device_name_value text;
  counts jsonb; group_hashes_value jsonb:='{}'::jsonb; hash_value text; total_bytes bigint:=0; max_bytes bigint; expiry timestamptz; c record; row_value jsonb;
  rows_value jsonb; pending_rows jsonb; candidate jsonb; payload jsonb; payload_bytes integer; chunk_index_value integer:=0; retention_auto integer; retention_pre integer;
  verified jsonb; failure_message text;
begin
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  if p_snapshot_type not in('manual','automatic','pre_restore') or nullif(btrim(p_name),'') is null or length(btrim(p_name))>120 then raise exception 'VALIDATION_ERROR: invalid snapshot request'; end if;
  if p_snapshot_type='automatic' then
    select id into existing_id from timegenie.workspace_snapshots where workspace_id=p_workspace_id and snapshot_type='automatic' and snapshot_day=current_date and status in('creating','ready') order by status='ready' desc limit 1;
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
  if p_snapshot_type='automatic' then
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

create or replace function timegenie.workspace_snapshot_list(p_workspace_id uuid,p_device_id uuid) returns jsonb language plpgsql stable security definer set search_path=timegenie,extensions,pg_catalog as $$
begin
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  return coalesce((select jsonb_agg(jsonb_build_object('id',id,'snapshotType',snapshot_type,'name',name,'note',note,'status',status,'formatVersion',format_version,'baseChangeSeq',base_change_seq,'workspaceEpoch',workspace_epoch,'entityCounts',entity_counts,'groupHashes',group_hashes,'contentHash',content_hash,'sizeBytes',size_bytes,'chunkCount',chunk_count,'deviceName',created_by_device_name,'protected',protected,'protectedAt',protected_at,'expiresAt',expires_at,'createdAt',created_at,'completedAt',completed_at,'failureReason',failure_reason) order by created_at desc,id desc) from timegenie.workspace_snapshots where workspace_id=p_workspace_id),'[]'::jsonb);
end $$;

create or replace function timegenie.workspace_snapshot_set_protected(p_workspace_id uuid,p_device_id uuid,p_snapshot_id uuid,p_protected boolean) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
begin
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  update timegenie.workspace_snapshots set protected=p_protected,protected_at=case when p_protected then coalesce(protected_at,now()) else null end where workspace_id=p_workspace_id and id=p_snapshot_id and status='ready';
  if not found then raise exception 'SNAPSHOT_NOT_FOUND'; end if; return timegenie.workspace_snapshot_detail(p_workspace_id,p_device_id,p_snapshot_id);
end $$;

create or replace function timegenie.workspace_snapshot_delete(p_workspace_id uuid,p_device_id uuid,p_snapshot_id uuid) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
begin
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  if exists(select 1 from timegenie.workspace_snapshots where workspace_id=p_workspace_id and id=p_snapshot_id and protected) then raise exception 'SNAPSHOT_PROTECTED'; end if;
  delete from timegenie.workspace_snapshots where workspace_id=p_workspace_id and id=p_snapshot_id; if not found then raise exception 'SNAPSHOT_NOT_FOUND'; end if;
  return jsonb_build_object('deleted',true,'snapshotId',p_snapshot_id);
end $$;

create or replace function timegenie.workspace_snapshot_preview(p_workspace_id uuid,p_device_id uuid,p_snapshot_id uuid) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare target jsonb; current_value jsonb; target_hash text; target_group_hashes jsonb; target_counts jsonb; current_counts jsonb:='{}'::jsonb; calculated_hashes jsonb:='{}'::jsonb;
  group_value text; c record; target_rows jsonb; current_rows jsonb; added integer; modified integer; removed integer; entity_diffs jsonb:='{}'::jsonb;
  group_diffs jsonb:=jsonb_build_object('planning',jsonb_build_object('added',0,'modified',0,'deleted',0),'time_tracking',jsonb_build_object('added',0,'modified',0,'deleted',0),'reports',jsonb_build_object('added',0,'modified',0,'deleted',0),'shared_settings',jsonb_build_object('added',0,'modified',0,'deleted',0));
begin
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  select content_hash,group_hashes,entity_counts into target_hash,target_group_hashes,target_counts from timegenie.workspace_snapshots where workspace_id=p_workspace_id and id=p_snapshot_id and status='ready';
  if target_hash is null then raise exception 'SNAPSHOT_NOT_RESTORABLE'; end if;
  begin target:=timegenie.workspace_snapshot_rehydrate(p_snapshot_id); exception when others then target:=null; end;
  if target is not null then foreach group_value in array array['planning','time_tracking','reports','shared_settings'] loop calculated_hashes:=calculated_hashes||jsonb_build_object(group_value,timegenie.workspace_snapshot_hash(target->group_value)); end loop; end if;
  if target is null or timegenie.workspace_snapshot_hash(target)<>target_hash or calculated_hashes<>target_group_hashes then
    update timegenie.workspace_snapshots set status='corrupt',failure_reason='CHECKSUM_MISMATCH' where id=p_snapshot_id;
    return jsonb_build_object('snapshotId',p_snapshot_id,'restorable',false,'errorCode','SNAPSHOT_CORRUPT');
  end if;
  current_value:=timegenie.workspace_snapshot_grouped(timegenie.cloud_snapshot_get(p_workspace_id));
  for c in select * from timegenie.workspace_snapshot_entity_catalog() order by sort_order loop
    target_rows:=target->c.group_name->c.entity_name; current_rows:=current_value->c.group_name->c.entity_name;
    select count(*) into added from jsonb_array_elements(target_rows) t where not exists(select 1 from jsonb_array_elements(current_rows) x where timegenie.workspace_snapshot_row_identity(c.entity_name,x)=timegenie.workspace_snapshot_row_identity(c.entity_name,t));
    select count(*) into removed from jsonb_array_elements(current_rows) x where not exists(select 1 from jsonb_array_elements(target_rows) t where timegenie.workspace_snapshot_row_identity(c.entity_name,t)=timegenie.workspace_snapshot_row_identity(c.entity_name,x));
    select count(*) into modified from jsonb_array_elements(target_rows) t join lateral(select x from jsonb_array_elements(current_rows) x where timegenie.workspace_snapshot_row_identity(c.entity_name,x)=timegenie.workspace_snapshot_row_identity(c.entity_name,t) limit 1) matched on true where matched.x<>t;
    entity_diffs:=entity_diffs||jsonb_build_object(c.entity_name,jsonb_build_object('added',added,'modified',modified,'deleted',removed));
    group_diffs:=jsonb_set(group_diffs,array[c.group_name],jsonb_build_object('added',coalesce((group_diffs->c.group_name->>'added')::integer,0)+added,'modified',coalesce((group_diffs->c.group_name->>'modified')::integer,0)+modified,'deleted',coalesce((group_diffs->c.group_name->>'deleted')::integer,0)+removed));
    current_counts:=current_counts||jsonb_build_object(c.entity_name,jsonb_array_length(current_rows));
  end loop;
  return jsonb_build_object('snapshotId',p_snapshot_id,'restorable',true,'entityCounts',target_counts,'currentEntityCounts',current_counts,'groups',group_diffs,'entities',entity_diffs,'contentHash',target_hash);
end $$;

revoke all on timegenie.workspace_snapshots,timegenie.workspace_snapshot_chunks from public,anon,authenticated;
revoke execute on function timegenie.workspace_snapshot_hash(jsonb),timegenie.workspace_snapshot_sanitize(jsonb),timegenie.workspace_snapshot_entity_catalog(),timegenie.workspace_snapshot_normalized_entity(jsonb,text),timegenie.workspace_snapshot_counts(jsonb),timegenie.workspace_snapshot_row_identity(text,jsonb),timegenie.workspace_snapshot_grouped(jsonb),timegenie.workspace_snapshot_rehydrate(uuid) from public,anon,authenticated;
revoke execute on function timegenie.workspace_snapshot_create(uuid,uuid,uuid,text,text,text,boolean),timegenie.workspace_snapshot_list(uuid,uuid),timegenie.workspace_snapshot_detail(uuid,uuid,uuid),timegenie.workspace_snapshot_set_protected(uuid,uuid,uuid,boolean),timegenie.workspace_snapshot_delete(uuid,uuid,uuid),timegenie.workspace_snapshot_preview(uuid,uuid,uuid),timegenie.workspace_snapshot_cleanup(uuid,uuid) from public,anon;
grant execute on function timegenie.workspace_snapshot_create(uuid,uuid,uuid,text,text,text,boolean),timegenie.workspace_snapshot_list(uuid,uuid),timegenie.workspace_snapshot_detail(uuid,uuid,uuid),timegenie.workspace_snapshot_set_protected(uuid,uuid,uuid,boolean),timegenie.workspace_snapshot_delete(uuid,uuid,uuid),timegenie.workspace_snapshot_preview(uuid,uuid,uuid),timegenie.workspace_snapshot_cleanup(uuid,uuid) to authenticated;
notify pgrst,'reload schema';
