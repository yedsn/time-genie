-- Require clients that understand independent today-task instances before any
-- upgraded workspace business data can be changed. The numeric generation is
-- a data-protocol capability, not a lexical application-version comparison.
begin;

alter table timegenie.workspaces
  add column if not exists minimum_client_schema_generation bigint not null default 0
  check(minimum_client_schema_generation >= 0);

-- Generation 1 is the first protocol that treats today tasks as independent
-- instances. Applying this migration deliberately upgrades every workspace in
-- the project and prevents older clients from continuing to write into it.
update timegenie.workspaces
set minimum_client_schema_generation = greatest(minimum_client_schema_generation, 1);

alter table timegenie.workspaces
  alter column minimum_client_schema_generation set default 1;

create or replace function timegenie.client_schema_generation_current()
returns bigint language plpgsql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare headers jsonb; generation_text text;
begin
  headers:=coalesce(nullif(current_setting('request.headers',true),'')::jsonb,'{}'::jsonb);
  generation_text:=headers->>'x-timegenie-client-schema-generation';
  if generation_text is null or generation_text !~ '^[0-9]+$' then
    return 0;
  end if;
  return generation_text::bigint;
exception when others then
  return 0;
end $$;

create or replace function timegenie.client_schema_generation_assert(p_workspace_id uuid)
returns bigint language plpgsql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare client_generation bigint; minimum_generation bigint;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  select minimum_client_schema_generation into minimum_generation
  from timegenie.workspaces where id=p_workspace_id and deleted_at is null;
  if minimum_generation is null then raise exception 'WORKSPACE_NOT_FOUND'; end if;
  client_generation:=timegenie.client_schema_generation_current();
  if client_generation < minimum_generation then
    raise exception 'CLIENT_UPGRADE_REQUIRED: client schema generation %, minimum %; upgrade all TimeGenie devices before editing this workspace',
      client_generation,minimum_generation;
  end if;
  return client_generation;
end $$;

create or replace function timegenie.client_schema_generation_before_business_write()
returns trigger language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare target_workspace_id uuid; client_generation bigint; minimum_generation bigint;
begin
  target_workspace_id:=case when tg_op='DELETE' then old.workspace_id else new.workspace_id end;
  -- A restore commit already passed its RPC-level generation assertion and
  -- uses this private transaction marker while replacing business rows.
  if nullif(current_setting('timegenie.restore_lock_token',true),'') is not null then
    return coalesce(new,old);
  end if;
  select minimum_client_schema_generation into minimum_generation
  from timegenie.workspaces where id=target_workspace_id and deleted_at is null;
  if minimum_generation is null then raise exception 'WORKSPACE_NOT_FOUND'; end if;
  client_generation:=timegenie.client_schema_generation_current();
  if client_generation < minimum_generation then
    raise exception 'CLIENT_UPGRADE_REQUIRED: client schema generation %, minimum %; upgrade all TimeGenie devices before editing this workspace',
      client_generation,minimum_generation;
  end if;
  return coalesce(new,old);
end $$;

do $$ begin
  if to_regprocedure('timegenie.workspace_restore_commit_pre_client_generation(uuid,uuid,uuid,uuid,text,bigint,uuid,boolean)') is null then
    alter function timegenie.workspace_restore_commit(uuid,uuid,uuid,uuid,text,bigint,uuid,boolean)
      rename to workspace_restore_commit_pre_client_generation;
  end if;
end $$;

create or replace function timegenie.workspace_restore_commit(
  p_workspace_id uuid,p_device_id uuid,p_lock_token uuid,p_target_snapshot_id uuid,
  p_target_content_hash text,p_prepared_epoch bigint,p_operation_id uuid,p_confirmed boolean
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog set statement_timeout='600s' as $$
begin
  perform timegenie.client_schema_generation_assert(p_workspace_id);
  return timegenie.workspace_restore_commit_pre_client_generation(
    p_workspace_id,p_device_id,p_lock_token,p_target_snapshot_id,p_target_content_hash,
    p_prepared_epoch,p_operation_id,p_confirmed
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
    execute format('drop trigger if exists client_schema_generation_write_barrier on timegenie.%I',table_name);
    execute format('create trigger client_schema_generation_write_barrier before insert or update or delete on timegenie.%I for each row execute function timegenie.client_schema_generation_before_business_write()',table_name);
  end loop;
end $$;

revoke execute on function timegenie.client_schema_generation_current(),
  timegenie.client_schema_generation_assert(uuid),
  timegenie.client_schema_generation_before_business_write(),
  timegenie.workspace_restore_commit_pre_client_generation(uuid,uuid,uuid,uuid,text,bigint,uuid,boolean)
from public,anon,authenticated;
revoke execute on function timegenie.workspace_restore_commit(uuid,uuid,uuid,uuid,text,bigint,uuid,boolean) from public,anon;
grant execute on function timegenie.workspace_restore_commit(uuid,uuid,uuid,uuid,text,bigint,uuid,boolean) to authenticated;

notify pgrst,'reload schema';
commit;
