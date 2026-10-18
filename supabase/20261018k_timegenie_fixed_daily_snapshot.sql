-- TimeGenie fixed-time daily automatic snapshots. Idempotent upgrade patch.
--
-- Automatic recovery points are created by a server-side pg_cron job at a fixed
-- workspace-local time instead of after a successful desktop sync, so the sync
-- path never pays the snapshot cost. Each workspace gets at most one automatic
-- snapshot per workspace-local calendar day.

begin;

alter table timegenie.workspace_history_settings
  add column if not exists automatic_snapshot_enabled boolean not null default true;

alter table timegenie.workspace_history_settings
  add column if not exists automatic_snapshot_time time not null default '03:00';

do $$ begin
  if exists(
    select 1 from information_schema.columns
    where table_schema='timegenie' and table_name='workspace_history_settings'
      and column_name='automatic_snapshot_frequency'
  ) then
    execute $migrate$update timegenie.workspace_history_settings
      set automatic_snapshot_enabled=(automatic_snapshot_frequency<>'off')$migrate$;
    execute 'alter table timegenie.workspace_history_settings drop column automatic_snapshot_frequency';
  end if;
end $$;

create or replace function timegenie.workspace_snapshot_next_scheduled_at(
  p_workspace_id uuid,p_at timestamptz
) returns timestamptz language plpgsql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare enabled_value boolean; scheduled_time time; timezone_name text; local_now timestamp;
begin
  select automatic_snapshot_enabled,automatic_snapshot_time
    into enabled_value,scheduled_time
  from timegenie.workspace_history_settings where workspace_id=p_workspace_id;
  if not coalesce(enabled_value,true) or scheduled_time is null then return null; end if;
  timezone_name:=timegenie.workspace_timezone_name(p_workspace_id);
  local_now:=p_at at time zone timezone_name;
  if local_now::time<scheduled_time then
    return (local_now::date+scheduled_time) at time zone timezone_name;
  end if;
  return (local_now::date+scheduled_time+interval '1 day') at time zone timezone_name;
end $$;

-- The scheduler runs inside the database, so it has to assume the workspace
-- owner for the duration of each snapshot: every cloud RPC proves ownership from
-- auth.uid(), which is empty in a pg_cron worker session.
create or replace function timegenie.workspace_snapshot_run_scheduled(p_at timestamptz default clock_timestamp())
returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog
set statement_timeout='600s' as $$
declare target record; timezone_name text; local_now timestamp; slot_key_value text;
  device_id_value uuid; created jsonb; results jsonb:='[]'::jsonb; previous_claim text;
begin
  previous_claim:=current_setting('request.jwt.claim.sub',true);
  if not pg_try_advisory_xact_lock(hashtextextended('timegenie:automatic_snapshot_runner',0)) then
    return jsonb_build_object('skipped',true,'reason','RUNNER_BUSY');
  end if;
  for target in
    select s.workspace_id,s.automatic_snapshot_time,w.owner_user_id
    from timegenie.workspace_history_settings s
    join timegenie.workspaces w on w.id=s.workspace_id and w.deleted_at is null
    where s.automatic_snapshot_enabled
    order by s.workspace_id
  loop
    begin
      timezone_name:=timegenie.workspace_timezone_name(target.workspace_id);
      local_now:=p_at at time zone timezone_name;
      if local_now::time<target.automatic_snapshot_time then continue; end if;
      slot_key_value:='daily:'||to_char(local_now::date,'YYYY-MM-DD');
      if exists(
        select 1 from timegenie.workspace_snapshots
        where workspace_id=target.workspace_id and schedule_slot_key=slot_key_value
      ) then continue; end if;
      select device.id into device_id_value
      from timegenie.devices device
      where device.workspace_id=target.workspace_id and device.revoked_at is null
      order by device.last_seen_at desc,device.created_at desc limit 1;
      if device_id_value is null then
        results:=results||jsonb_build_array(jsonb_build_object(
          'workspaceId',target.workspace_id,'slotKey',slot_key_value,'skipped','NO_ACTIVE_DEVICE'));
        continue;
      end if;
      perform set_config('request.jwt.claim.sub',target.owner_user_id::text,true);
      perform set_config('timegenie.snapshot_schedule_slot_key',slot_key_value,true);
      created:=timegenie.workspace_snapshot_create(
        target.workspace_id,device_id_value,gen_random_uuid(),'automatic',
        '自动快照 '||to_char(local_now,'YYYY-MM-DD HH24:MI'),
        '每天 '||to_char(target.automatic_snapshot_time,'HH24:MI')||'（'||timezone_name||'）自动创建的完整恢复点',
        false);
      perform set_config('timegenie.snapshot_schedule_slot_key','',true);
      results:=results||jsonb_build_array(jsonb_build_object(
        'workspaceId',target.workspace_id,'slotKey',slot_key_value,
        'snapshotId',created->>'id','status',created->>'status'));
    exception when others then
      perform set_config('timegenie.snapshot_schedule_slot_key','',true);
      perform set_config('request.jwt.claim.sub',coalesce(previous_claim,''),true);
      results:=results||jsonb_build_array(jsonb_build_object(
        'workspaceId',target.workspace_id,'error',left(sqlerrm,300)));
    end;
  end loop;
  perform set_config('request.jwt.claim.sub',coalesce(previous_claim,''),true);
  return jsonb_build_object('checkedAt',p_at,'results',results);
end $$;

drop function if exists timegenie.workspace_snapshot_settings_update(uuid,uuid,uuid,text);
drop function if exists timegenie.workspace_snapshot_settings_update_with_retention(uuid,uuid,uuid,text,integer);
drop function if exists timegenie.workspace_snapshot_ensure_automatic(uuid,uuid,uuid);
drop function if exists timegenie.workspace_snapshot_ensure_automatic_at(uuid,uuid,uuid,timestamptz);
drop function if exists timegenie.workspace_snapshot_slot_key(text,timestamptz);

create or replace function timegenie.workspace_snapshot_settings_get(p_workspace_id uuid,p_device_id uuid)
returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare enabled_value boolean; scheduled_time time; timezone_name text; retention_days integer;
  last_at timestamptz; next_at timestamptz; scheduler_installed boolean; job_active boolean:=false;
begin
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  insert into timegenie.workspace_history_settings(workspace_id) values(p_workspace_id) on conflict(workspace_id) do nothing;
  select automatic_snapshot_enabled,automatic_snapshot_time,snapshot_retention_days
    into enabled_value,scheduled_time,retention_days
  from timegenie.workspace_history_settings where workspace_id=p_workspace_id;
  timezone_name:=timegenie.workspace_timezone_name(p_workspace_id);
  select max(created_at) into last_at from timegenie.workspace_snapshots
  where workspace_id=p_workspace_id and snapshot_type='automatic';
  next_at:=timegenie.workspace_snapshot_next_scheduled_at(p_workspace_id,clock_timestamp());
  scheduler_installed:=exists(select 1 from pg_extension where extname='pg_cron');
  if scheduler_installed then
    execute 'select exists(select 1 from cron.job where jobname=$1 and active)'
      into job_active using 'timegenie-daily-automatic-snapshot';
  end if;
  return jsonb_build_object(
    'automaticSnapshotEnabled',coalesce(enabled_value,true),
    'automaticSnapshotTime',to_char(scheduled_time,'HH24:MI'),
    'automaticSnapshotTimezone',timezone_name,
    'snapshotRetentionDays',coalesce(retention_days,30),
    'nextAutomaticSnapshotAt',to_jsonb(next_at),
    'lastAutomaticSnapshotAt',to_jsonb(last_at),
    'schedulerActive',coalesce(job_active,false)
  );
end $$;

create or replace function timegenie.workspace_snapshot_settings_update(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,
  p_automatic_snapshot_enabled boolean,p_automatic_snapshot_time time,p_snapshot_retention_days integer
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare result_value jsonb;
begin
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  if p_automatic_snapshot_enabled is null or p_automatic_snapshot_time is null then
    raise exception 'VALIDATION_ERROR: invalid automatic snapshot schedule';
  end if;
  if p_snapshot_retention_days not between 1 and 365 then
    raise exception 'VALIDATION_ERROR: snapshot retention must be between 1 and 365 days';
  end if;
  insert into timegenie.workspace_history_settings(
    workspace_id,automatic_snapshot_enabled,automatic_snapshot_time,
    automatic_snapshot_retention_days,pre_restore_snapshot_retention_days,
    snapshot_retention_days,updated_at
  ) values(
    p_workspace_id,p_automatic_snapshot_enabled,p_automatic_snapshot_time,
    p_snapshot_retention_days,p_snapshot_retention_days,p_snapshot_retention_days,now()
  )
  on conflict(workspace_id) do update set
    automatic_snapshot_enabled=excluded.automatic_snapshot_enabled,
    automatic_snapshot_time=excluded.automatic_snapshot_time,
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
        'automatic_snapshot_enabled',p_automatic_snapshot_enabled,
        'automatic_snapshot_time',to_char(p_automatic_snapshot_time,'HH24:MI'),
        'snapshot_retention_days',p_snapshot_retention_days)),
    jsonb_build_object('entityType','workspace_snapshot_settings','entityId',p_workspace_id,
      'authoritative',jsonb_build_object(
        'automatic_snapshot_enabled',p_automatic_snapshot_enabled,
        'automatic_snapshot_time',to_char(p_automatic_snapshot_time,'HH24:MI'),
        'snapshot_retention_days',p_snapshot_retention_days))
  );
  perform timegenie.workspace_snapshot_cleanup(p_workspace_id,p_device_id);
  return result_value;
end $$;

revoke execute on function timegenie.workspace_snapshot_next_scheduled_at(uuid,timestamptz) from public,anon,authenticated;
revoke execute on function timegenie.workspace_snapshot_run_scheduled(timestamptz) from public,anon,authenticated;
revoke execute on function timegenie.workspace_snapshot_settings_get(uuid,uuid) from public,anon;
revoke execute on function timegenie.workspace_snapshot_settings_update(uuid,uuid,uuid,boolean,time,integer) from public,anon;
grant execute on function timegenie.workspace_snapshot_settings_get(uuid,uuid) to authenticated;
grant execute on function timegenie.workspace_snapshot_settings_update(uuid,uuid,uuid,boolean,time,integer) to authenticated;

-- pg_cron is preloaded by Supabase images; keep this patch usable in plain
-- PostgreSQL test databases where the extension cannot be installed.
do $$
declare scheduler_ready boolean:=false;
begin
  begin
    execute 'create extension if not exists pg_cron';
  exception when others then
    raise notice 'pg_cron extension unavailable: %',sqlerrm;
  end;
  scheduler_ready:=exists(select 1 from pg_extension where extname='pg_cron');
  if scheduler_ready then
    execute 'select cron.schedule($1,$2,$3)'
      using 'timegenie-daily-automatic-snapshot','* * * * *',
        'select timegenie.workspace_snapshot_run_scheduled()';
    execute 'select cron.schedule($1,$2,$3)'
      using 'timegenie-cron-maintenance','15 4 * * *',
        'delete from cron.job_run_details where end_time<now()-interval ''14 days''::interval';
  else
    raise notice 'pg_cron is unavailable; automatic snapshots stay idle until it is installed';
  end if;
end $$;

notify pgrst,'reload schema';

commit;
