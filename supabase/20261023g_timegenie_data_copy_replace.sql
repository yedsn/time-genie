-- Atomic replacement of a direct-v2 workspace from a validated data-copy
-- staging package. The existing restore write barrier is shared so ordinary
-- business mutations cannot interleave with a replacement.

begin;

alter table timegenie.workspace_snapshots
  drop constraint if exists workspace_snapshots_snapshot_type_check;
alter table timegenie.workspace_snapshots
  add constraint workspace_snapshots_snapshot_type_check
  check(snapshot_type in ('manual','automatic','pre_restore','pre_replace'));

alter table timegenie.workspace_restore_locks
  add column if not exists target_package_id uuid
  references timegenie.data_copy_staging_packages(id);
alter table timegenie.workspace_restore_locks
  alter column target_snapshot_id drop not null;

do $$
begin
  if not exists(
    select 1 from pg_constraint
    where conrelid='timegenie.workspace_restore_locks'::regclass
      and conname='workspace_restore_locks_target_check'
  ) then
    alter table timegenie.workspace_restore_locks
      add constraint workspace_restore_locks_target_check
      check((target_snapshot_id is not null)::integer+(target_package_id is not null)::integer=1);
  end if;
end $$;

create or replace function timegenie.data_copy_stage_snapshot(p_package_id uuid)
returns jsonb language sql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
  select coalesce(
    jsonb_object_agg(catalog.entity_name,timegenie.data_copy_stage_rows(p_package_id,catalog.entity_name)),
    '{}'::jsonb
  )
  from timegenie.data_copy_entity_catalog() catalog;
$$;

create or replace function timegenie.data_copy_replace_commit(
  p_workspace_id uuid,p_device_id uuid,p_expected_epoch bigint,p_package_id uuid,
  p_expected_content_hash text,p_operation_id uuid,p_confirmed boolean
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare
  package_row timegenie.data_copy_staging_packages;
  lock_token_value uuid:=gen_random_uuid();
  safety_result jsonb;
  safety_snapshot_id_value uuid;
  staged_snapshot jsonb;
  authoritative_snapshot jsonb;
  import_operation_id uuid:=gen_random_uuid();
  catalog_row record;
  expected_count integer;
  actual_count integer;
  current_epoch bigint;
  new_epoch bigint;
  reset_seq bigint;
  cached jsonb;
  result_value jsonb;
  request_value jsonb:=jsonb_build_object(
    'entityType','workspace','entityId',p_workspace_id,'packageId',p_package_id,
    'contentHash',p_expected_content_hash,'expectedEpoch',p_expected_epoch
  );
begin
  if not p_confirmed then raise exception 'DATA_COPY_REPLACE_CONFIRMATION_REQUIRED'; end if;
  -- A successful replacement advances the workspace epoch and deletes its
  -- staging package. Check an already-processed operation before enforcing the
  -- caller's old epoch so a lost response can be retried idempotently. Keep the
  -- owner, device, protocol, and schema checks in front of the cached result.
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,null);
  cached:=timegenie.processed_operation_result(
    p_workspace_id,p_operation_id,'data_copy.replace',request_value
  );
  if cached is not null then return cached; end if;
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);

  select package.* into package_row
  from timegenie.data_copy_staging_packages package
  where package.id=p_package_id and package.workspace_id=p_workspace_id
    and package.device_id=p_device_id and package.status='validated'
    and package.expires_at>now()
  for update;
  if not found then raise exception 'DATA_COPY_STAGE_NOT_VALIDATED'; end if;
  if package_row.package_version<>1 then raise exception 'DATA_PACKAGE_VERSION_UNSUPPORTED'; end if;
  if package_row.schema_generation<>1 then raise exception 'CLIENT_UPGRADE_REQUIRED: data package schema generation mismatch'; end if;
  if package_row.expected_content_hash<>coalesce(p_expected_content_hash,'') then
    raise exception 'DATA_PACKAGE_CONTENT_HASH_MISMATCH';
  end if;

  select workspace_epoch into current_epoch
  from timegenie.workspaces where id=p_workspace_id for update;
  if current_epoch<>p_expected_epoch then
    raise exception 'WORKSPACE_EPOCH_STALE: client=%, server=%',p_expected_epoch,current_epoch;
  end if;
  if exists(
    select 1 from timegenie.time_entries
    where workspace_id=p_workspace_id and state in('running','paused') and deleted_at is null
  ) then raise exception 'DATA_COPY_TARGET_ACTIVE_TIMER'; end if;
  if exists(
    select 1 from timegenie.unassigned_sessions
    where workspace_id=p_workspace_id and state in('collecting','paused','awaiting_resolution')
  ) then raise exception 'DATA_COPY_TARGET_ACTIVE_UNASSIGNED'; end if;

  delete from timegenie.workspace_restore_locks
  where workspace_id=p_workspace_id and expires_at<=now();
  if exists(select 1 from timegenie.workspace_restore_locks where workspace_id=p_workspace_id) then
    raise exception 'WORKSPACE_RESTORE_IN_PROGRESS';
  end if;
  insert into timegenie.workspace_restore_locks(
    workspace_id,lock_token,holder_device_id,target_snapshot_id,target_package_id,
    target_content_hash,prepared_epoch,status,expires_at
  ) values(
    p_workspace_id,lock_token_value,p_device_id,null,p_package_id,
    p_expected_content_hash,current_epoch,'preparing',now()+interval '10 minutes'
  );

  perform set_config('timegenie.restore_lock_token',lock_token_value::text,true);
  perform set_config(
    'request.headers',
    jsonb_build_object(
      'x-timegenie-workspace-epoch',current_epoch::text,
      'x-timegenie-client-schema-generation','1'
    )::text,
    true
  );
  safety_result:=timegenie.workspace_snapshot_create(
    p_workspace_id,p_device_id,gen_random_uuid(),'pre_restore',
    '数据复制替换前安全镜像','系统在本地数据完整替换云端前自动创建',false
  );
  safety_snapshot_id_value:=(safety_result->>'id')::uuid;
  if safety_snapshot_id_value is null or safety_result->>'status'<>'ready' then
    raise exception 'DATA_COPY_SAFETY_SNAPSHOT_FAILED: %',
      coalesce(safety_result->>'failureReason','unknown');
  end if;
  update timegenie.workspace_snapshots
  set snapshot_type='pre_replace' where id=safety_snapshot_id_value;
  update timegenie.workspace_restore_locks
  set safety_snapshot_id=safety_snapshot_id_value,status='prepared',updated_at=now()
  where workspace_id=p_workspace_id and lock_token=lock_token_value;

  staged_snapshot:=timegenie.data_copy_stage_snapshot(p_package_id);
  update timegenie.workspace_restore_locks
  set status='committing',updated_at=now()
  where workspace_id=p_workspace_id and lock_token=lock_token_value;

  delete from timegenie.report_tasks where workspace_id=p_workspace_id;
  delete from timegenie.reports where workspace_id=p_workspace_id;
  delete from timegenie.actual_time_adjustments where workspace_id=p_workspace_id;
  delete from timegenie.time_allocations where workspace_id=p_workspace_id;
  delete from timegenie.time_segments where workspace_id=p_workspace_id;
  update timegenie.unassigned_sessions set generated_entry_id=null where workspace_id=p_workspace_id;
  delete from timegenie.unassigned_segments where workspace_id=p_workspace_id;
  delete from timegenie.unassigned_sessions where workspace_id=p_workspace_id;
  delete from timegenie.time_entries where workspace_id=p_workspace_id;
  delete from timegenie.task_status_events where workspace_id=p_workspace_id;
  delete from timegenie.task_day_plans where workspace_id=p_workspace_id;
  delete from timegenie.task_daily_estimates where workspace_id=p_workspace_id;
  delete from timegenie.task_occurrences where workspace_id=p_workspace_id;
  delete from timegenie.task_recurrence_rules where workspace_id=p_workspace_id;
  delete from timegenie.report_templates where workspace_id=p_workspace_id;
  delete from timegenie.tasks where workspace_id=p_workspace_id;
  delete from timegenie.subject_work_schedule_segments where workspace_id=p_workspace_id;
  delete from timegenie.subject_work_schedules where workspace_id=p_workspace_id;
  delete from timegenie.work_days where workspace_id=p_workspace_id;
  delete from timegenie.external_bindings where workspace_id=p_workspace_id;
  delete from timegenie.integration_configs where workspace_id=p_workspace_id;
  delete from timegenie.app_settings where workspace_id=p_workspace_id;
  delete from timegenie.subjects where workspace_id=p_workspace_id;

  perform timegenie.migration_import_snapshot_pre_direct_v2(
    p_workspace_id,import_operation_id,p_device_id,staged_snapshot
  );
  delete from timegenie.processed_operations
  where workspace_id=p_workspace_id and operation_id=import_operation_id;

  authoritative_snapshot:=timegenie.cloud_snapshot_get(p_workspace_id);
  for catalog_row in select * from timegenie.data_copy_entity_catalog() order by sort_order loop
    expected_count:=coalesce((package_row.entity_counts->>catalog_row.entity_name)::integer,0);
    actual_count:=jsonb_array_length(coalesce(authoritative_snapshot->catalog_row.entity_name,'[]'::jsonb));
    if actual_count<>expected_count then
      raise exception 'DATA_COPY_REPLACE_COUNT_MISMATCH: % expected %, actual %',
        catalog_row.entity_name,expected_count,actual_count;
    end if;
  end loop;

  new_epoch:=current_epoch+1;
  update timegenie.workspaces
  set workspace_epoch=new_epoch,updated_at=now() where id=p_workspace_id;
  insert into timegenie.workspace_changes(
    workspace_id,entity_type,entity_id,operation,entity_version,changed_by_device_id
  ) values(
    p_workspace_id,'workspace_reset',p_workspace_id::text,'update',new_epoch,p_device_id
  ) returning change_seq into reset_seq;

  result_value:=jsonb_build_object(
    'replaced',true,'packageId',p_package_id,'contentHash',p_expected_content_hash,
    'safetySnapshotId',safety_snapshot_id_value,'previousEpoch',current_epoch,
    'workspaceEpoch',new_epoch,'resetChangeSeq',reset_seq,
    'entityCounts',package_row.entity_counts
  );
  perform timegenie.record_processed_operation(
    p_workspace_id,p_operation_id,p_device_id,'data_copy.replace',request_value,result_value
  );
  delete from timegenie.workspace_restore_locks
  where workspace_id=p_workspace_id and lock_token=lock_token_value;
  delete from timegenie.data_copy_staging_packages
  where id=p_package_id and workspace_id=p_workspace_id;
  perform set_config('timegenie.restore_lock_token','',true);
  return result_value;
end $$;

revoke execute on function timegenie.data_copy_stage_snapshot(uuid) from public,anon,authenticated;
revoke execute on function timegenie.data_copy_replace_commit(uuid,uuid,bigint,uuid,text,uuid,boolean) from public,anon;
grant execute on function timegenie.data_copy_replace_commit(uuid,uuid,bigint,uuid,text,uuid,boolean) to authenticated;

commit;
notify pgrst,'reload schema';
