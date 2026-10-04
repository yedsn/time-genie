-- TimeGenie Supabase cloud guard verification.
-- This script is read-only. Run it in Supabase SQL Editor after applying the
-- 20261004b and 20261004c patches to verify the deployed RPC definitions.

do $$
declare
  migration_import_definition text;
  cloud_apply_definition text;
begin
  if to_regclass('timegenie.workspaces') is null
     or to_regclass('timegenie.work_days') is null
     or to_regclass('timegenie.unassigned_sessions') is null
     or to_regclass('timegenie.processed_operations') is null then
    raise exception 'TIMEGENIE_SCHEMA_CHECK_FAILED: timegenie schema or required tables are missing';
  end if;

  if to_regprocedure('timegenie.migration_import_snapshot(uuid, uuid, uuid, jsonb)') is null then
    raise exception 'TIMEGENIE_GUARD_MISSING: migration_import_snapshot RPC is missing';
  end if;

  if to_regprocedure('timegenie.cloud_apply_patch(uuid, uuid, uuid, text, text, text, bigint, jsonb)') is null then
    raise exception 'TIMEGENIE_GUARD_MISSING: cloud_apply_patch RPC is missing';
  end if;

  migration_import_definition := pg_get_functiondef(
    'timegenie.migration_import_snapshot(uuid, uuid, uuid, jsonb)'::regprocedure
  );
  cloud_apply_definition := pg_get_functiondef(
    'timegenie.cloud_apply_patch(uuid, uuid, uuid, text, text, text, bigint, jsonb)'::regprocedure
  );

  if position('exists(select 1 from timegenie.work_days where workspace_id = p_workspace_id)' in migration_import_definition) = 0 then
    raise exception 'TIMEGENIE_GUARD_MISSING: migration_import_snapshot does not treat work_days as existing business data';
  end if;

  if position('exists(select 1 from timegenie.unassigned_sessions where workspace_id = p_workspace_id)' in migration_import_definition) = 0 then
    raise exception 'TIMEGENIE_GUARD_MISSING: migration_import_snapshot does not treat unassigned_sessions as existing business data';
  end if;

  if position('where timegenie.work_days.version <= excluded.version' in cloud_apply_definition) = 0 then
    raise exception 'TIMEGENIE_GUARD_MISSING: cloud_apply_patch allows stale embedded work_day payloads to overwrite newer cloud work_days';
  end if;

  if position('p_entity_type = ''unassigned_session''' in cloud_apply_definition) = 0 then
    raise exception 'TIMEGENIE_GUARD_MISSING: cloud_apply_patch does not accept unassigned_session payloads';
  end if;

  if position('delete from timegenie.unassigned_segments where session_id = p_entity_id::uuid' in cloud_apply_definition) = 0 then
    raise exception 'TIMEGENIE_GUARD_MISSING: cloud_apply_patch does not replace unassigned session segments from complete payloads';
  end if;

  if not has_function_privilege(
    'authenticated',
    'timegenie.cloud_apply_patch(uuid, uuid, uuid, text, text, text, bigint, jsonb)',
    'execute'
  ) then
    raise exception 'TIMEGENIE_GRANT_MISSING: authenticated cannot execute cloud_apply_patch';
  end if;

  if not has_function_privilege(
    'authenticated',
    'timegenie.migration_import_snapshot(uuid, uuid, uuid, jsonb)',
    'execute'
  ) then
    raise exception 'TIMEGENIE_GRANT_MISSING: authenticated cannot execute migration_import_snapshot';
  end if;

  raise notice 'TimeGenie cloud guard verification passed: migration target guard and work_day version guard are active.';
end $$;

