-- TimeGenie in-place workspace restore, lock lease, and epoch reset protocol.
create table if not exists timegenie.workspace_restore_locks (
  workspace_id uuid primary key references timegenie.workspaces(id) on delete cascade,
  lock_token uuid not null,
  holder_device_id uuid not null references timegenie.devices(id),
  target_snapshot_id uuid not null references timegenie.workspace_snapshots(id),
  target_content_hash text not null,
  prepared_epoch bigint not null,
  safety_snapshot_id uuid references timegenie.workspace_snapshots(id),
  status text not null default 'preparing' check(status in ('preparing','prepared','committing')),
  expires_at timestamptz not null,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);
alter table timegenie.workspace_restore_locks enable row level security;
drop policy if exists workspace_owner on timegenie.workspace_restore_locks;
create policy workspace_owner on timegenie.workspace_restore_locks for select using(timegenie.is_workspace_owner(workspace_id));
revoke all on timegenie.workspace_restore_locks from public,anon,authenticated;

create or replace function timegenie.workspace_restore_before_business_write() returns trigger language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare workspace_value uuid; bypass_token text; active_token uuid;
begin
  workspace_value:=coalesce(new.workspace_id,old.workspace_id);
  delete from timegenie.workspace_restore_locks where workspace_id=workspace_value and expires_at<=now();
  select lock_token into active_token from timegenie.workspace_restore_locks where workspace_id=workspace_value and expires_at>now();
  if active_token is null then return coalesce(new,old); end if;
  bypass_token:=nullif(current_setting('timegenie.restore_lock_token',true),'');
  if bypass_token is null or bypass_token::uuid<>active_token then raise exception 'WORKSPACE_RESTORE_IN_PROGRESS'; end if;
  return coalesce(new,old);
end $$;

do $$ declare table_name text; begin
  foreach table_name in array array['subjects','subject_work_schedules','subject_work_schedule_segments','work_days','tasks','task_status_events','task_day_plans','task_daily_estimates','task_recurrence_rules','task_occurrences','time_entries','time_segments','time_allocations','actual_time_adjustments','unassigned_sessions','unassigned_segments','report_templates','reports','report_tasks','app_settings','integration_configs','external_bindings'] loop
    execute format('drop trigger if exists workspace_restore_write_barrier on timegenie.%I',table_name);
    execute format('create trigger workspace_restore_write_barrier before insert or update or delete on timegenie.%I for each row execute function timegenie.workspace_restore_before_business_write()',table_name);
  end loop;
end $$;

create or replace function timegenie.workspace_restore_prepare(p_workspace_id uuid,p_device_id uuid,p_target_snapshot_id uuid,p_expected_epoch bigint) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare target_hash text; current_epoch bigint; token_value uuid:=gen_random_uuid(); safety_id uuid; preview_value jsonb; safety_result jsonb;
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
  preview_value:=timegenie.workspace_snapshot_preview(p_workspace_id,p_device_id,p_target_snapshot_id);
  if not coalesce((preview_value->>'restorable')::boolean,false) then raise exception 'SNAPSHOT_NOT_RESTORABLE'; end if;
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

create or replace function timegenie.workspace_restore_commit(p_workspace_id uuid,p_device_id uuid,p_lock_token uuid,p_target_snapshot_id uuid,p_target_content_hash text,p_prepared_epoch bigint,p_operation_id uuid,p_confirmed boolean) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare lock_value timegenie.workspace_restore_locks; target jsonb; group_name_value text; c record; rows_value jsonb; row_value jsonb; current_epoch bigint; new_epoch bigint; result_value jsonb; cached jsonb; reset_seq bigint;
begin
  if not p_confirmed then raise exception 'RESTORE_CONFIRMATION_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  cached:=timegenie.processed_operation_result(p_workspace_id,p_operation_id,'workspace.restore',jsonb_build_object('entityType','workspace','entityId',p_workspace_id,'snapshotId',p_target_snapshot_id,'targetHash',p_target_content_hash,'preparedEpoch',p_prepared_epoch));
  if cached is not null then return cached; end if;
  select * into lock_value from timegenie.workspace_restore_locks where workspace_id=p_workspace_id and lock_token=p_lock_token and holder_device_id=p_device_id and status='prepared' and expires_at>now() for update;
  if not found then raise exception 'RESTORE_LOCK_INVALID_OR_EXPIRED'; end if;
  if lock_value.target_snapshot_id<>p_target_snapshot_id or lock_value.target_content_hash<>p_target_content_hash or lock_value.prepared_epoch<>p_prepared_epoch then raise exception 'RESTORE_PREPARE_MISMATCH'; end if;
  select workspace_epoch into current_epoch from timegenie.workspaces where id=p_workspace_id for update;
  if current_epoch<>p_prepared_epoch then raise exception 'WORKSPACE_EPOCH_STALE: client=%, server=%',p_prepared_epoch,current_epoch; end if;
  target:=timegenie.workspace_snapshot_rehydrate(p_target_snapshot_id);
  if timegenie.workspace_snapshot_hash(target)<>p_target_content_hash then raise exception 'SNAPSHOT_CHECKSUM_FAILED'; end if;
  update timegenie.workspace_restore_locks set status='committing',updated_at=now() where workspace_id=p_workspace_id;
  perform set_config('timegenie.restore_lock_token',p_lock_token::text,true);
  perform set_config('request.headers',jsonb_build_object('x-timegenie-workspace-epoch',current_epoch::text)::text,true);
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
  foreach group_name_value in array array['planning','time_tracking','reports','shared_settings'] loop
    for c in select * from timegenie.workspace_snapshot_entity_catalog() catalog where catalog.group_name=group_name_value order by sort_order loop
      rows_value:=coalesce(target->group_name_value->c.entity_name,'[]'::jsonb);
      for row_value in select value from jsonb_array_elements(rows_value) loop
        execute format('insert into timegenie.%I select * from jsonb_populate_record(null::timegenie.%I,$1)',c.entity_name,c.entity_name) using row_value;
      end loop;
    end loop;
  end loop;
  update timegenie.time_entries set state='paused',ended_at=null where workspace_id=p_workspace_id and state='running';
  update timegenie.time_segments set ended_at=coalesce(ended_at,started_at),duration_seconds=coalesce(duration_seconds,0) where workspace_id=p_workspace_id and ended_at is null;
  update timegenie.unassigned_sessions set state='paused' where workspace_id=p_workspace_id and state='collecting';
  new_epoch:=current_epoch+1;
  update timegenie.workspaces set workspace_epoch=new_epoch,updated_at=now() where id=p_workspace_id;
  insert into timegenie.workspace_changes(workspace_id,entity_type,entity_id,operation,entity_version,changed_by_device_id) values(p_workspace_id,'workspace_reset',p_workspace_id::text,'update',new_epoch,p_device_id) returning change_seq into reset_seq;
  result_value:=jsonb_build_object('restored',true,'targetSnapshotId',p_target_snapshot_id,'safetySnapshotId',lock_value.safety_snapshot_id,'previousEpoch',current_epoch,'workspaceEpoch',new_epoch,'resetChangeSeq',reset_seq);
  perform timegenie.record_processed_operation(p_workspace_id,p_operation_id,p_device_id,'workspace.restore',jsonb_build_object('entityType','workspace','entityId',p_workspace_id,'snapshotId',p_target_snapshot_id,'targetHash',p_target_content_hash,'preparedEpoch',p_prepared_epoch),result_value);
  delete from timegenie.workspace_restore_locks where workspace_id=p_workspace_id and lock_token=p_lock_token;
  return result_value;
end $$;

create or replace function timegenie.workspace_restore_cancel(p_workspace_id uuid,p_device_id uuid,p_lock_token uuid) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare safety_id uuid;
begin
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  select safety_snapshot_id into safety_id from timegenie.workspace_restore_locks where workspace_id=p_workspace_id and lock_token=p_lock_token and holder_device_id=p_device_id;
  delete from timegenie.workspace_restore_locks where workspace_id=p_workspace_id and lock_token=p_lock_token and holder_device_id=p_device_id;
  return jsonb_build_object('cancelled',true,'safetySnapshotId',safety_id);
end $$;

revoke execute on function timegenie.workspace_restore_before_business_write() from public,anon,authenticated;
revoke execute on function timegenie.workspace_restore_prepare(uuid,uuid,uuid,bigint),timegenie.workspace_restore_commit(uuid,uuid,uuid,uuid,text,bigint,uuid,boolean),timegenie.workspace_restore_cancel(uuid,uuid,uuid) from public,anon;
grant execute on function timegenie.workspace_restore_prepare(uuid,uuid,uuid,bigint),timegenie.workspace_restore_commit(uuid,uuid,uuid,uuid,text,bigint,uuid,boolean),timegenie.workspace_restore_cancel(uuid,uuid,uuid) to authenticated;
notify pgrst,'reload schema';
