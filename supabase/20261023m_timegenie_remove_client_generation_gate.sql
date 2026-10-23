-- Direct-v2 reads and writes are authorized by identity, device, storage
-- protocol and workspace epoch. They no longer depend on a numeric client
-- schema generation header. Historical generation objects are removed or kept
-- behind private compatibility wrappers only.

begin;

create or replace function timegenie.storage_protocol_get(p_workspace_id uuid)
returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare result_value jsonb;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then
    raise exception 'AUTH_REQUIRED';
  end if;
  select jsonb_build_object(
    'workspaceId',workspace.id,
    'storageProtocol',workspace.storage_protocol,
    'workspaceEpoch',workspace.workspace_epoch,
    'serverTime',now()
  ) into result_value
  from timegenie.workspaces workspace
  where workspace.id=p_workspace_id and workspace.deleted_at is null;
  if result_value is null then raise exception 'WORKSPACE_NOT_FOUND'; end if;
  return result_value;
end $$;

create or replace function timegenie.assert_sync_v1_workspace(p_workspace_id uuid)
returns void language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare protocol_value text;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  select workspace.storage_protocol into protocol_value
  from timegenie.workspaces workspace
  where workspace.id=p_workspace_id and workspace.deleted_at is null;
  if protocol_value is null then raise exception 'WORKSPACE_NOT_FOUND'; end if;
  if protocol_value='direct_v2' then
    raise exception 'CLOUD_PROTOCOL_UNSUPPORTED: direct_v2 工作空间不接受旧同步入口';
  end if;
end $$;

create or replace function timegenie.assert_direct_v2_workspace(
  p_workspace_id uuid,
  p_device_id uuid,
  p_expected_epoch bigint default null
) returns void language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare workspace_row timegenie.workspaces;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  select workspace.* into workspace_row
  from timegenie.workspaces workspace
  where workspace.id=p_workspace_id and workspace.deleted_at is null
  for share;
  if not found then raise exception 'WORKSPACE_NOT_FOUND'; end if;
  if workspace_row.storage_protocol<>'direct_v2' then
    raise exception 'CLOUD_PROTOCOL_UNSUPPORTED: 工作空间尚未启用 direct_v2';
  end if;
  if not exists(
    select 1 from timegenie.devices device
    where device.id=p_device_id and device.workspace_id=p_workspace_id and device.revoked_at is null
  ) then
    raise exception 'DEVICE_REVOKED';
  end if;
  if p_expected_epoch is not null and p_expected_epoch<>workspace_row.workspace_epoch then
    raise exception 'WORKSPACE_EPOCH_STALE: expected %, current %',p_expected_epoch,workspace_row.workspace_epoch;
  end if;
end $$;

create or replace function timegenie.direct_cloud_context(
  p_workspace_id uuid,
  p_device_id uuid,
  p_expected_epoch bigint default null
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare workspace_row timegenie.workspaces; latest_change_seq bigint;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  select workspace.* into workspace_row from timegenie.workspaces workspace
  where workspace.id=p_workspace_id and workspace.deleted_at is null;
  select coalesce(max(change_seq),0) into latest_change_seq
  from timegenie.workspace_changes where workspace_id=p_workspace_id;
  return jsonb_build_object(
    'workspaceId',workspace_row.id,
    'deviceId',p_device_id,
    'storageProtocol',workspace_row.storage_protocol,
    'workspaceEpoch',workspace_row.workspace_epoch,
    'changeSeq',latest_change_seq,
    'serverTime',now(),
    'writable',not workspace_row.emergency_read_only,
    'readOnlyReason',case when workspace_row.emergency_read_only then
      coalesce(nullif(workspace_row.emergency_read_only_reason,''),'Supabase 紧急停写已启用')
      else null end
  );
end $$;

create or replace function timegenie.storage_protocol_upgrade_preflight(
  p_workspace_id uuid,
  p_device_id uuid
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare workspace_row timegenie.workspaces;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then
    raise exception 'DEVICE_REVOKED';
  end if;
  select workspace.* into workspace_row
  from timegenie.workspaces workspace
  where workspace.id=p_workspace_id and workspace.deleted_at is null;
  if not found then raise exception 'WORKSPACE_NOT_FOUND'; end if;
  return jsonb_build_object(
    'workspaceId',workspace_row.id,
    'deviceId',p_device_id,
    'storageProtocol',workspace_row.storage_protocol,
    'workspaceEpoch',workspace_row.workspace_epoch,
    'serverUpgradeAllowed',workspace_row.storage_protocol='sync_v1',
    'alreadyDirect',workspace_row.storage_protocol='direct_v2',
    'requiresLocalQueueDisposition',true,
    'allowedQueueDispositions',jsonb_build_array('queue_drained','abandoned_with_rescue'),
    'serverTime',now()
  );
end $$;

do $$
declare table_name text;
begin
  foreach table_name in array array[
    'subjects','subject_work_schedules','subject_work_schedule_segments','work_days','tasks','task_status_events',
    'task_day_plans','task_daily_estimates','task_recurrence_rules','task_occurrences','time_entries','time_segments',
    'time_allocations','actual_time_adjustments','actual_adjustment_batches','unassigned_sessions','unassigned_segments',
    'report_templates','reports','report_tasks','app_settings','integration_configs','external_bindings'
  ] loop
    if to_regclass('timegenie.'||table_name) is not null then
      execute format('drop trigger if exists client_schema_generation_write_barrier on timegenie.%I',table_name);
    end if;
  end loop;
end $$;

-- The direct-v2 protocol patch renamed the generation wrapper to this private
-- function. Rebind it to the original restore implementation before dropping
-- the generation assertion helper.
create or replace function timegenie.workspace_restore_commit_pre_direct_v2(
  p_workspace_id uuid,p_device_id uuid,p_lock_token uuid,p_target_snapshot_id uuid,
  p_target_content_hash text,p_prepared_epoch bigint,p_operation_id uuid,p_confirmed boolean
) returns jsonb language sql security definer
set search_path=timegenie,extensions,pg_catalog set statement_timeout='600s' as $$
  select timegenie.workspace_restore_commit_pre_client_generation(
    p_workspace_id,p_device_id,p_lock_token,p_target_snapshot_id,
    p_target_content_hash,p_prepared_epoch,p_operation_id,p_confirmed
  );
$$;

drop function if exists timegenie.client_schema_generation_before_business_write();
drop function if exists timegenie.client_schema_generation_assert(uuid);
drop function if exists timegenie.client_schema_generation_current();

alter table timegenie.workspaces
  drop column if exists minimum_client_schema_generation;

-- New clients send only the actual data-package format version. The old
-- seven-argument function remains private so already staged packages can still
-- be completed safely during a rolling deployment.
create or replace function timegenie.data_copy_stage_begin(
  p_workspace_id uuid,p_device_id uuid,p_expected_epoch bigint,p_package_version integer,
  p_expected_chunk_count integer,p_expected_content_hash text
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
begin
  if p_package_version<>1 then raise exception 'DATA_PACKAGE_VERSION_UNSUPPORTED'; end if;
  return timegenie.data_copy_stage_begin(
    p_workspace_id,p_device_id,p_expected_epoch,p_package_version,1,
    p_expected_chunk_count,p_expected_content_hash
  );
end $$;

revoke execute on function timegenie.data_copy_stage_begin(uuid,uuid,bigint,integer,bigint,integer,text)
from public,anon,authenticated;
revoke execute on function timegenie.data_copy_stage_begin(uuid,uuid,bigint,integer,integer,text)
from public,anon;
grant execute on function timegenie.data_copy_stage_begin(uuid,uuid,bigint,integer,integer,text)
to authenticated;

do $$
begin
  if to_regprocedure('timegenie.data_copy_export_preview_with_client_generation(uuid,uuid,bigint)') is null
     and to_regprocedure('timegenie.data_copy_export_preview(uuid,uuid,bigint)') is not null then
    alter function timegenie.data_copy_export_preview(uuid,uuid,bigint)
      rename to data_copy_export_preview_with_client_generation;
  end if;
  if to_regprocedure('timegenie.data_copy_export_begin_with_client_generation(uuid,uuid,bigint)') is null
     and to_regprocedure('timegenie.data_copy_export_begin(uuid,uuid,bigint)') is not null then
    alter function timegenie.data_copy_export_begin(uuid,uuid,bigint)
      rename to data_copy_export_begin_with_client_generation;
  end if;
end $$;

create or replace function timegenie.data_copy_export_preview(
  p_workspace_id uuid,p_device_id uuid,p_expected_epoch bigint
) returns jsonb language sql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
  select timegenie.data_copy_export_preview_with_client_generation(
    p_workspace_id,p_device_id,p_expected_epoch
  ) - 'schemaGeneration';
$$;

create or replace function timegenie.data_copy_export_begin(
  p_workspace_id uuid,p_device_id uuid,p_expected_epoch bigint
) returns jsonb language sql security definer
set search_path=timegenie,extensions,pg_catalog as $$
  select timegenie.data_copy_export_begin_with_client_generation(
    p_workspace_id,p_device_id,p_expected_epoch
  ) - 'schemaGeneration';
$$;

revoke execute on function timegenie.data_copy_export_preview_with_client_generation(uuid,uuid,bigint),
  timegenie.data_copy_export_begin_with_client_generation(uuid,uuid,bigint)
from public,anon,authenticated;
revoke execute on function timegenie.data_copy_export_preview(uuid,uuid,bigint),
  timegenie.data_copy_export_begin(uuid,uuid,bigint)
from public,anon;
grant execute on function timegenie.data_copy_export_preview(uuid,uuid,bigint),
  timegenie.data_copy_export_begin(uuid,uuid,bigint)
to authenticated;

notify pgrst,'reload schema';
commit;
