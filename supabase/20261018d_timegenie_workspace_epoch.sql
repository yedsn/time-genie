-- TimeGenie workspace epoch write barrier and reset-aware sync protocol.
alter table timegenie.workspaces add column if not exists workspace_epoch bigint not null default 1 check(workspace_epoch>0);

create or replace function timegenie.workspace_epoch_current(p_workspace_id uuid) returns bigint language sql stable security definer set search_path=timegenie,extensions,pg_catalog as $$
  select workspace_epoch from timegenie.workspaces where id=p_workspace_id and timegenie.is_workspace_owner(p_workspace_id);
$$;

create or replace function timegenie.workspace_epoch_assert(p_workspace_id uuid,p_client_epoch bigint default null) returns bigint language plpgsql stable security definer set search_path=timegenie,extensions,pg_catalog as $$
declare server_epoch bigint;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  select workspace_epoch into server_epoch from timegenie.workspaces where id=p_workspace_id;
  if server_epoch is null then raise exception 'WORKSPACE_NOT_FOUND'; end if;
  if server_epoch=1 and (p_client_epoch is null or p_client_epoch=1) then return server_epoch; end if;
  if p_client_epoch is null or p_client_epoch<>server_epoch then
    raise exception 'WORKSPACE_EPOCH_STALE: client=%, server=%',coalesce(p_client_epoch,0),server_epoch;
  end if;
  return server_epoch;
end $$;

create or replace function timegenie.workspace_epoch_before_business_write() returns trigger language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare request_epoch_text text; request_epoch bigint; server_epoch bigint; target_workspace_id uuid;
begin
  request_epoch_text:=nullif(current_setting('request.headers',true),'')::jsonb->>'x-timegenie-workspace-epoch';
  request_epoch:=nullif(request_epoch_text,'')::bigint;
  target_workspace_id:=case when tg_op='DELETE' then old.workspace_id else new.workspace_id end;
  select workspace_epoch into server_epoch from timegenie.workspaces where id=target_workspace_id;
  if not (server_epoch=1 and (request_epoch is null or request_epoch=1)) and request_epoch is distinct from server_epoch then
    raise exception 'WORKSPACE_EPOCH_STALE: client=%, server=%',coalesce(request_epoch,0),server_epoch;
  end if;
  return coalesce(new,old);
end $$;

do $$
declare table_name text;
begin
  foreach table_name in array array[
    'subjects','subject_work_schedules','subject_work_schedule_segments','work_days','tasks','task_status_events','task_day_plans','task_daily_estimates',
    'task_recurrence_rules','task_occurrences','time_entries','time_segments','time_allocations','actual_time_adjustments','unassigned_sessions',
    'unassigned_segments','report_templates','reports','report_tasks','app_settings','integration_configs','external_bindings'
  ] loop
    execute format('drop trigger if exists workspace_epoch_write_barrier on timegenie.%I',table_name);
    execute format('create trigger workspace_epoch_write_barrier before insert or update or delete on timegenie.%I for each row execute function timegenie.workspace_epoch_before_business_write()',table_name);
  end loop;
end $$;

do $$ begin
  if to_regprocedure('timegenie.cloud_snapshot_get_pre_workspace_epoch(uuid)') is null then
    alter function timegenie.cloud_snapshot_get(uuid) rename to cloud_snapshot_get_pre_workspace_epoch;
  end if;
  if to_regprocedure('timegenie.cloud_incremental_pull_pre_workspace_epoch(uuid,uuid,bigint,integer)') is null then
    alter function timegenie.cloud_incremental_pull(uuid,uuid,bigint,integer) rename to cloud_incremental_pull_pre_workspace_epoch;
  end if;
end $$;

create or replace function timegenie.cloud_snapshot_get(p_workspace_id uuid) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare result_value jsonb; epoch_value bigint;
begin
  epoch_value:=timegenie.workspace_epoch_assert(p_workspace_id,nullif(nullif(current_setting('request.headers',true),'')::jsonb->>'x-timegenie-workspace-epoch','')::bigint);
  result_value:=timegenie.cloud_snapshot_get_pre_workspace_epoch(p_workspace_id);
  return result_value||jsonb_build_object('workspace_epoch',epoch_value);
end $$;

create or replace function timegenie.cloud_incremental_pull(p_workspace_id uuid,p_device_id uuid,p_after_change_seq bigint default 0,p_limit integer default 200) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare result_value jsonb; epoch_value bigint; client_epoch bigint;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_REVOKED'; end if;
  select workspace_epoch into epoch_value from timegenie.workspaces where id=p_workspace_id and timegenie.is_workspace_owner(p_workspace_id);
  client_epoch:=nullif(nullif(current_setting('request.headers',true),'')::jsonb->>'x-timegenie-workspace-epoch','')::bigint;
  if not (epoch_value=1 and (client_epoch is null or client_epoch=1)) and client_epoch is distinct from epoch_value then
    return jsonb_build_object('changes','[]'::jsonb,'entities','[]'::jsonb,'covered_change_seq',greatest(coalesce(p_after_change_seq,0),0),
      'latest_change_seq',coalesce((select max(change_seq) from timegenie.workspace_changes where workspace_id=p_workspace_id),0),
      'has_more',false,'reset_required',true,'workspace_epoch',epoch_value);
  end if;
  result_value:=timegenie.cloud_incremental_pull_pre_workspace_epoch(p_workspace_id,p_device_id,p_after_change_seq,p_limit);
  if exists(select 1 from jsonb_array_elements(coalesce(result_value->'changes','[]'::jsonb)) change where change->>'entity_type'='workspace_reset') then
    result_value:=jsonb_set(result_value,'{reset_required}','true'::jsonb);
  end if;
  return result_value||jsonb_build_object('workspace_epoch',epoch_value);
end $$;

revoke execute on function timegenie.workspace_epoch_current(uuid),timegenie.workspace_epoch_assert(uuid,bigint),timegenie.workspace_epoch_before_business_write(),timegenie.cloud_snapshot_get_pre_workspace_epoch(uuid),timegenie.cloud_incremental_pull_pre_workspace_epoch(uuid,uuid,bigint,integer) from public,anon,authenticated;
revoke execute on function timegenie.cloud_snapshot_get(uuid),timegenie.cloud_incremental_pull(uuid,uuid,bigint,integer) from public,anon;
grant execute on function timegenie.cloud_snapshot_get(uuid),timegenie.cloud_incremental_pull(uuid,uuid,bigint,integer) to authenticated;
notify pgrst,'reload schema';
