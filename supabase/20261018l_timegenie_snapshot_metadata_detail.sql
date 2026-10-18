-- TimeGenie lightweight snapshot details and restore-only integrity checks.
-- Opening the details dialog must never reconstruct the snapshot or compare it
-- row-by-row with the current workspace. The immutable summary saved during
-- snapshot creation is sufficient for browsing.

begin;

create or replace function timegenie.workspace_snapshot_preview(
  p_workspace_id uuid,p_device_id uuid,p_snapshot_id uuid
) returns jsonb language plpgsql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare snapshot_value timegenie.workspace_snapshots; group_counts jsonb:='{}'::jsonb; group_name_value text; entity_name_value text; entity_count integer;
begin
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  select * into snapshot_value from timegenie.workspace_snapshots
  where workspace_id=p_workspace_id and id=p_snapshot_id;
  if not found then raise exception 'SNAPSHOT_NOT_FOUND'; end if;
  if snapshot_value.status<>'ready' or snapshot_value.content_hash is null then
    return jsonb_build_object(
      'snapshotId',p_snapshot_id,'restorable',false,
      'errorCode',case when snapshot_value.status='corrupt' then 'SNAPSHOT_CORRUPT' else 'SNAPSHOT_NOT_RESTORABLE' end,
      'entityCounts',snapshot_value.entity_counts,'contentHash',snapshot_value.content_hash);
  end if;
  for group_name_value,entity_name_value in
    select group_name,entity_name from timegenie.workspace_snapshot_entity_catalog() order by sort_order
  loop
    entity_count:=coalesce((snapshot_value.entity_counts->>entity_name_value)::integer,0);
    group_counts:=jsonb_set(
      group_counts,array[group_name_value],
      to_jsonb(coalesce((group_counts->>group_name_value)::integer,0)+entity_count),true
    );
  end loop;
  return jsonb_build_object(
    'snapshotId',snapshot_value.id,
    'restorable',true,
    'entityCounts',snapshot_value.entity_counts,
    'groupCounts',group_counts,
    'groupHashes',snapshot_value.group_hashes,
    'contentHash',snapshot_value.content_hash,
    'sizeBytes',snapshot_value.size_bytes,
    'chunkCount',snapshot_value.chunk_count,
    'baseChangeSeq',snapshot_value.base_change_seq,
    'workspaceEpoch',snapshot_value.workspace_epoch,
    'createdAt',snapshot_value.created_at,
    'completedAt',snapshot_value.completed_at
  );
end $$;

create or replace function timegenie.workspace_snapshot_verify_integrity(
  p_workspace_id uuid,p_snapshot_id uuid
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog
set statement_timeout='600s' as $$
declare snapshot_value timegenie.workspace_snapshots; chunk_value record; chunk_count_value integer:=0; size_bytes_value bigint:=0; entity_counts_value jsonb:='{}'::jsonb; expected_rows integer; verified_rows integer;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  select * into snapshot_value from timegenie.workspace_snapshots
  where workspace_id=p_workspace_id and id=p_snapshot_id and status='ready';
  if not found or snapshot_value.content_hash is null then raise exception 'SNAPSHOT_NOT_RESTORABLE'; end if;
  for chunk_value in
    select entity_name,content_json,content_hash,size_bytes,row_count
    from timegenie.workspace_snapshot_chunks
    where workspace_id=p_workspace_id and snapshot_id=p_snapshot_id
    order by chunk_index
  loop
    chunk_count_value:=chunk_count_value+1;
    size_bytes_value:=size_bytes_value+chunk_value.size_bytes;
    if timegenie.workspace_snapshot_hash(chunk_value.content_json)<>chunk_value.content_hash then
      update timegenie.workspace_snapshots set status='corrupt',failure_reason='CHUNK_CHECKSUM_MISMATCH' where id=p_snapshot_id;
      return jsonb_build_object('restorable',false,'errorCode','SNAPSHOT_CORRUPT');
    end if;
    verified_rows:=jsonb_array_length(coalesce(chunk_value.content_json->chunk_value.entity_name,'[]'::jsonb));
    if verified_rows<>chunk_value.row_count then
      update timegenie.workspace_snapshots set status='corrupt',failure_reason='CHUNK_ROW_COUNT_MISMATCH' where id=p_snapshot_id;
      return jsonb_build_object('restorable',false,'errorCode','SNAPSHOT_CORRUPT');
    end if;
    entity_counts_value:=jsonb_set(
      entity_counts_value,array[chunk_value.entity_name],
      to_jsonb(coalesce((entity_counts_value->>chunk_value.entity_name)::integer,0)+verified_rows),true
    );
  end loop;
  if chunk_count_value<>snapshot_value.chunk_count or size_bytes_value<>snapshot_value.size_bytes then
    update timegenie.workspace_snapshots set status='corrupt',failure_reason='SNAPSHOT_CHUNK_METADATA_MISMATCH' where id=p_snapshot_id;
    return jsonb_build_object('restorable',false,'errorCode','SNAPSHOT_CORRUPT');
  end if;
  for chunk_value in select key,value from jsonb_each(snapshot_value.entity_counts) loop
    expected_rows:=(chunk_value.value#>>'{}')::integer;
    if coalesce((entity_counts_value->>chunk_value.key)::integer,0)<>expected_rows then
      update timegenie.workspace_snapshots set status='corrupt',failure_reason='SNAPSHOT_ENTITY_COUNT_MISMATCH' where id=p_snapshot_id;
      return jsonb_build_object('restorable',false,'errorCode','SNAPSHOT_CORRUPT');
    end if;
  end loop;
  return jsonb_build_object('restorable',true,'contentHash',snapshot_value.content_hash);
end $$;

create or replace function timegenie.workspace_restore_prepare(p_workspace_id uuid,p_device_id uuid,p_target_snapshot_id uuid,p_expected_epoch bigint) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare target_hash text; current_epoch bigint; token_value uuid:=gen_random_uuid(); safety_id uuid; preview_value jsonb; verification_value jsonb; safety_result jsonb;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  select workspace_epoch into current_epoch from timegenie.workspaces where id=p_workspace_id for update;
  if p_expected_epoch is distinct from current_epoch then raise exception 'WORKSPACE_EPOCH_STALE: client=%, server=%',coalesce(p_expected_epoch,0),current_epoch; end if;
  if exists(select 1 from timegenie.time_entries where workspace_id=p_workspace_id and state in('running','paused') and deleted_at is null) then raise exception 'RESTORE_ACTIVE_TIMER'; end if;
  if exists(select 1 from timegenie.unassigned_sessions where workspace_id=p_workspace_id and state in('collecting','paused','awaiting_resolution')) then raise exception 'RESTORE_ACTIVE_UNASSIGNED'; end if;
  delete from timegenie.workspace_restore_locks where workspace_id=p_workspace_id and expires_at<=now();
  if exists(select 1 from timegenie.workspace_restore_locks where workspace_id=p_workspace_id) then raise exception 'WORKSPACE_RESTORE_IN_PROGRESS'; end if;
  select content_hash into target_hash from timegenie.workspace_snapshots where workspace_id=p_workspace_id and id=p_target_snapshot_id and status='ready';
  if target_hash is null then raise exception 'SNAPSHOT_NOT_RESTORABLE'; end if;
  verification_value:=timegenie.workspace_snapshot_verify_integrity(p_workspace_id,p_target_snapshot_id);
  if not coalesce((verification_value->>'restorable')::boolean,false) then raise exception 'SNAPSHOT_NOT_RESTORABLE'; end if;
  preview_value:=timegenie.workspace_snapshot_preview(p_workspace_id,p_device_id,p_target_snapshot_id);
  insert into timegenie.workspace_restore_locks(workspace_id,lock_token,holder_device_id,target_snapshot_id,target_content_hash,prepared_epoch,status,expires_at)
  values(p_workspace_id,token_value,p_device_id,p_target_snapshot_id,target_hash,current_epoch,'preparing',now()+interval '10 minutes');
  begin
    perform set_config('timegenie.restore_lock_token',token_value::text,true);
    safety_result:=timegenie.workspace_snapshot_create(p_workspace_id,p_device_id,gen_random_uuid(),'pre_restore','回滚前安全镜像','系统在云端回滚前自动创建',false);
    safety_id:=(safety_result->>'id')::uuid;
    if safety_result->>'status'<>'ready' then raise exception 'RESTORE_SAFETY_SNAPSHOT_FAILED: %',coalesce(safety_result->>'failureReason','unknown'); end if;
    update timegenie.workspace_restore_locks set safety_snapshot_id=safety_id,status='prepared',updated_at=now() where workspace_id=p_workspace_id and lock_token=token_value;
    perform set_config('timegenie.restore_lock_token','',true);
  exception when others then
    perform set_config('timegenie.restore_lock_token','',true);
    delete from timegenie.workspace_restore_locks where workspace_id=p_workspace_id and lock_token=token_value;
    raise;
  end;
  return jsonb_build_object('lockToken',token_value,'targetSnapshotId',p_target_snapshot_id,'targetContentHash',target_hash,'preparedEpoch',current_epoch,'safetySnapshotId',safety_id,'expiresAt',now()+interval '10 minutes','preview',preview_value);
end $$;

revoke execute on function timegenie.workspace_snapshot_verify_integrity(uuid,uuid) from public,anon,authenticated;
revoke execute on function timegenie.workspace_snapshot_preview(uuid,uuid,uuid) from public,anon;
grant execute on function timegenie.workspace_snapshot_preview(uuid,uuid,uuid) to authenticated;

notify pgrst,'reload schema';

commit;
