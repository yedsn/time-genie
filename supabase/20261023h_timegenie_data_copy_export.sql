-- Consistent direct-v2 cloud exports for explicit cloud-to-local copies.

begin;

create table if not exists timegenie.data_copy_export_packages (
  id uuid primary key default gen_random_uuid(),
  workspace_id uuid not null references timegenie.workspaces(id) on delete cascade,
  device_id uuid not null references timegenie.devices(id) on delete cascade,
  package_version integer not null,
  schema_generation bigint not null,
  workspace_epoch bigint not null,
  change_seq bigint not null,
  content_hash text not null check(content_hash ~ '^[0-9a-f]{64}$'),
  entity_counts jsonb not null,
  chunk_count integer not null check(chunk_count between 1 and 256),
  created_at timestamptz not null default now(),
  expires_at timestamptz not null default (now()+interval '1 day')
);

create table if not exists timegenie.data_copy_export_chunks (
  package_id uuid not null references timegenie.data_copy_export_packages(id) on delete cascade,
  chunk_index integer not null check(chunk_index between 0 and 255),
  entity_name text not null,
  content_text text not null,
  content_hash text not null check(content_hash ~ '^[0-9a-f]{64}$'),
  row_count integer not null check(row_count>=0),
  primary key(package_id,chunk_index)
);

create index if not exists idx_data_copy_export_expiry on timegenie.data_copy_export_packages(expires_at);
create index if not exists idx_data_copy_export_entity on timegenie.data_copy_export_chunks(package_id,entity_name,chunk_index);

alter table timegenie.data_copy_export_packages enable row level security;
alter table timegenie.data_copy_export_chunks enable row level security;
drop policy if exists workspace_owner on timegenie.data_copy_export_packages;
create policy workspace_owner on timegenie.data_copy_export_packages using(timegenie.is_workspace_owner(workspace_id));
drop policy if exists workspace_owner on timegenie.data_copy_export_chunks;
create policy workspace_owner on timegenie.data_copy_export_chunks using(exists(
  select 1 from timegenie.data_copy_export_packages package
  where package.id=package_id and timegenie.is_workspace_owner(package.workspace_id)
));

create or replace function timegenie.data_copy_export_sanitize(p_value jsonb)
returns jsonb language plpgsql immutable as $$
declare result_value jsonb:='{}'::jsonb; item jsonb; pair record; normalized_key text;
begin
  if p_value is null then return null; end if;
  if jsonb_typeof(p_value)='array' then
    result_value:='[]'::jsonb;
    for item in select value from jsonb_array_elements(p_value) loop
      result_value:=result_value||jsonb_build_array(timegenie.data_copy_export_sanitize(item));
    end loop;
    return result_value;
  elsif jsonb_typeof(p_value)<>'object' then return p_value; end if;
  for pair in select key,value from jsonb_each(p_value) order by key loop
    normalized_key:=regexp_replace(lower(pair.key),'[^a-z0-9]+','','g');
    if normalized_key ~ '(token|password|secret|authorization|apikey|managementkey|servicerole)'
       or normalized_key=any(array[
         'anonkey','leasetoken','authsessionid','createdbydeviceid','updatedbydeviceid',
         'actionconfig','actionconfigjson','command','localpath','directory','directorypath',
         'filepath','sourcepath','outputpath','vaultpath','reportdirectory','dailyreportdirectory'
       ]) then continue; end if;
    result_value:=result_value||jsonb_build_object(pair.key,timegenie.data_copy_export_sanitize(pair.value));
  end loop;
  return result_value;
end $$;

create or replace function timegenie.data_copy_export_entity_rows(p_snapshot jsonb,p_entity_name text)
returns jsonb language sql immutable as $$
  select coalesce(jsonb_agg(timegenie.data_copy_export_sanitize(value) order by timegenie.data_copy_export_sanitize(value)::text),'[]'::jsonb)
  from jsonb_array_elements(case when jsonb_typeof(p_snapshot->p_entity_name)='array' then p_snapshot->p_entity_name else '[]'::jsonb end)
  where p_entity_name<>'app_settings' or regexp_replace(lower(coalesce(value->>'key','')),'[^a-z0-9]+','','g') !~
    '(token|password|secret|authorization|apikey|managementkey|servicerole|localpath|directory|filepath|sourcepath|outputpath|vaultpath)';
$$;

create or replace function timegenie.data_copy_export_preview(
  p_workspace_id uuid,p_device_id uuid,p_expected_epoch bigint
) returns jsonb language plpgsql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare snapshot_value jsonb; counts_value jsonb:='{}'::jsonb; catalog_row record; change_seq_value bigint;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  select timegenie.cloud_snapshot_get(p_workspace_id),coalesce((
    select max(change_seq) from timegenie.workspace_changes where workspace_id=p_workspace_id
  ),0) into snapshot_value,change_seq_value;
  for catalog_row in select * from timegenie.data_copy_entity_catalog() order by sort_order loop
    counts_value:=counts_value||jsonb_build_object(
      catalog_row.entity_name,jsonb_array_length(timegenie.data_copy_export_entity_rows(snapshot_value,catalog_row.entity_name))
    );
  end loop;
  return jsonb_build_object('workspaceId',p_workspace_id,'packageVersion',1,'schemaGeneration',1,
    'workspaceEpoch',p_expected_epoch,'changeSeq',change_seq_value,'entityCounts',counts_value);
end $$;

create or replace function timegenie.data_copy_export_begin(
  p_workspace_id uuid,p_device_id uuid,p_expected_epoch bigint
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare package_id_value uuid:=gen_random_uuid(); workspace_epoch_value bigint; change_seq_value bigint;
declare snapshot_value jsonb; catalog_row record; rows_value jsonb; row_value jsonb;
declare pending_rows jsonb; candidate_rows jsonb; content_text_value text; content_hash_value text;
declare entity_counts_value jsonb:='{}'::jsonb; chunk_index_value integer:=0;
declare expires_at_value timestamptz:=now()+interval '1 day';
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  select workspace.workspace_epoch into workspace_epoch_value from timegenie.workspaces workspace
  where workspace.id=p_workspace_id and workspace.deleted_at is null for share;
  if workspace_epoch_value<>p_expected_epoch then raise exception 'WORKSPACE_EPOCH_STALE: expected %, current %',p_expected_epoch,workspace_epoch_value; end if;

  -- Both values are read by one statement and therefore one authoritative snapshot.
  select timegenie.cloud_snapshot_get(p_workspace_id),coalesce((
    select max(change_seq) from timegenie.workspace_changes where workspace_id=p_workspace_id
  ),0) into snapshot_value,change_seq_value;

  delete from timegenie.data_copy_export_packages where expires_at<=now();
  insert into timegenie.data_copy_export_packages(id,workspace_id,device_id,package_version,schema_generation,workspace_epoch,change_seq,content_hash,entity_counts,chunk_count,expires_at)
  values(package_id_value,p_workspace_id,p_device_id,1,1,workspace_epoch_value,change_seq_value,repeat('0',64),'{}'::jsonb,1,expires_at_value);
  for catalog_row in select * from timegenie.data_copy_entity_catalog() order by sort_order loop
    rows_value:=timegenie.data_copy_export_entity_rows(snapshot_value,catalog_row.entity_name);
    entity_counts_value:=entity_counts_value||jsonb_build_object(catalog_row.entity_name,jsonb_array_length(rows_value));
    pending_rows:='[]'::jsonb;
    for row_value in select value from jsonb_array_elements(rows_value) loop
      candidate_rows:=pending_rows||jsonb_build_array(row_value);
      if octet_length(convert_to(candidate_rows::text,'utf8'))>8388608 then
        if jsonb_array_length(pending_rows)=0 then raise exception 'DATA_PACKAGE_ROW_TOO_LARGE: %',catalog_row.entity_name; end if;
        if chunk_index_value>=256 then raise exception 'DATA_PACKAGE_TOO_MANY_CHUNKS'; end if;
        content_text_value:=pending_rows::text;
        insert into timegenie.data_copy_export_chunks(package_id,chunk_index,entity_name,content_text,content_hash,row_count)
        values(package_id_value,chunk_index_value,catalog_row.entity_name,content_text_value,encode(digest(convert_to(content_text_value,'utf8'),'sha256'),'hex'),jsonb_array_length(pending_rows));
        chunk_index_value:=chunk_index_value+1; pending_rows:=jsonb_build_array(row_value);
      else pending_rows:=candidate_rows; end if;
    end loop;
    if chunk_index_value>=256 then raise exception 'DATA_PACKAGE_TOO_MANY_CHUNKS'; end if;
    content_text_value:=pending_rows::text;
    insert into timegenie.data_copy_export_chunks(package_id,chunk_index,entity_name,content_text,content_hash,row_count)
    values(package_id_value,chunk_index_value,catalog_row.entity_name,content_text_value,encode(digest(convert_to(content_text_value,'utf8'),'sha256'),'hex'),jsonb_array_length(pending_rows));
    chunk_index_value:=chunk_index_value+1;
  end loop;

  select encode(digest(convert_to(string_agg(chunk.content_hash,'' order by chunk.chunk_index),'utf8'),'sha256'),'hex')
  into content_hash_value from timegenie.data_copy_export_chunks chunk where chunk.package_id=package_id_value;
  update timegenie.data_copy_export_packages
  set content_hash=content_hash_value,entity_counts=entity_counts_value,chunk_count=chunk_index_value
  where id=package_id_value;
  return jsonb_build_object('packageId',package_id_value,'packageVersion',1,'schemaGeneration',1,'workspaceEpoch',workspace_epoch_value,'changeSeq',change_seq_value,'entityCounts',entity_counts_value,'contentHash',content_hash_value,'chunkCount',chunk_index_value,'expiresAt',expires_at_value);
end $$;

create or replace function timegenie.data_copy_export_chunk_get(
  p_workspace_id uuid,p_device_id uuid,p_package_id uuid,p_chunk_index integer
) returns jsonb language plpgsql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare package_row timegenie.data_copy_export_packages; chunk_row timegenie.data_copy_export_chunks;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,null);
  select package.* into package_row from timegenie.data_copy_export_packages package
  where package.id=p_package_id and package.workspace_id=p_workspace_id and package.device_id=p_device_id and package.expires_at>now();
  if not found then raise exception 'DATA_COPY_EXPORT_NOT_FOUND'; end if;
  select chunk.* into chunk_row from timegenie.data_copy_export_chunks chunk
  where chunk.package_id=p_package_id and chunk.chunk_index=p_chunk_index;
  if not found then raise exception 'DATA_COPY_EXPORT_CHUNK_NOT_FOUND'; end if;
  return jsonb_build_object('packageId',p_package_id,'chunkIndex',chunk_row.chunk_index,'entityName',chunk_row.entity_name,'contentText',chunk_row.content_text,'contentHash',chunk_row.content_hash,'rowCount',chunk_row.row_count);
end $$;

create or replace function timegenie.data_copy_export_discard(
  p_workspace_id uuid,p_device_id uuid,p_package_id uuid
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare deleted_count integer;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,null);
  delete from timegenie.data_copy_export_packages where id=p_package_id and workspace_id=p_workspace_id and device_id=p_device_id;
  get diagnostics deleted_count=row_count;
  return jsonb_build_object('packageId',p_package_id,'discarded',deleted_count=1);
end $$;

revoke all on timegenie.data_copy_export_packages,timegenie.data_copy_export_chunks from public,anon,authenticated;
revoke execute on function timegenie.data_copy_export_sanitize(jsonb),timegenie.data_copy_export_entity_rows(jsonb,text) from public,anon,authenticated;
revoke execute on function timegenie.data_copy_export_preview(uuid,uuid,bigint),timegenie.data_copy_export_begin(uuid,uuid,bigint),timegenie.data_copy_export_chunk_get(uuid,uuid,uuid,integer),timegenie.data_copy_export_discard(uuid,uuid,uuid) from public,anon;
grant execute on function timegenie.data_copy_export_preview(uuid,uuid,bigint),timegenie.data_copy_export_begin(uuid,uuid,bigint),timegenie.data_copy_export_chunk_get(uuid,uuid,uuid,integer),timegenie.data_copy_export_discard(uuid,uuid,uuid) to authenticated;

commit;
notify pgrst,'reload schema';
