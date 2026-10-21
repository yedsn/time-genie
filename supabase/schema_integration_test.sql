\set ON_ERROR_STOP on

-- Run in a disposable PostgreSQL database. The harness must provide
-- auth.users, auth.uid(), and the supabase_realtime publication. Supabase
-- already has anon/authenticated roles; plain PostgreSQL test databases do not.

do $$
begin
  if not exists(select 1 from pg_roles where rolname = 'anon') then
    create role anon nologin;
  end if;
  if not exists(select 1 from pg_roles where rolname = 'authenticated') then
    create role authenticated nologin;
  end if;
end $$;

create schema if not exists auth;
create table if not exists auth.users(id uuid primary key);
create or replace function auth.uid()
returns uuid language sql stable as $$
  select nullif(current_setting('request.jwt.claim.sub', true), '')::uuid;
$$;

do $$
begin
  if not exists(select 1 from pg_publication where pubname = 'supabase_realtime') then
    create publication supabase_realtime;
  end if;
end $$;

-- Reapply the idempotent schema after creating the Data API roles so this test
-- exercises the same explicit grants and revokes used by a real Supabase project.
\ir schema.sql
\ir 20261004_timegenie_cloud_sync_patch.sql
\ir 20261004b_timegenie_migration_target_guard_patch.sql
\ir 20261004c_timegenie_work_day_version_guard_patch.sql
\ir 20261004d_timegenie_session_revocation_patch.sql
\ir 20261004_verify_timegenie_cloud_guards.sql
\ir 20261005_timegenie_timer_sync_coordination_patch.sql
\ir 20261005_timegenie_cloud_task_planning_patch.sql
\ir 20261005b_timegenie_event_driven_sync_patch.sql
\ir 20261005b_timegenie_event_driven_sync_patch.sql
\ir 20261006_timegenie_shared_unassigned_patch.sql
\ir 20261006_timegenie_shared_unassigned_patch.sql
\ir 20261006b_timegenie_calendar_day_accounting_patch.sql
\ir 20261006b_timegenie_calendar_day_accounting_patch.sql
\ir 20261007_timegenie_unassigned_segment_boundary_fix.sql
\ir 20261007_timegenie_unassigned_segment_boundary_fix.sql
\ir 20261008_timegenie_simplified_unassigned_anchor.sql
\ir 20261008_timegenie_simplified_unassigned_anchor.sql
\ir 20261013_timegenie_unassigned_lease_continuity_fix.sql
\ir 20261013_timegenie_unassigned_lease_continuity_fix.sql
\ir 20261014_timegenie_timer_sync_authority.sql
\ir 20261014_timegenie_timer_sync_authority.sql
\ir 20261018_timegenie_timer_write_lease_takeover.sql
\ir 20261018_timegenie_timer_write_lease_takeover.sql
\ir 20261018f_timegenie_timer_fact_convergence.sql
\ir 20261018f_timegenie_timer_fact_convergence.sql
\ir 20261015_timegenie_actual_adjustment_batch.sql
\ir 20261015_timegenie_actual_adjustment_batch.sql
\ir 20261016_timegenie_named_template_bootstrap_fix.sql
\ir 20261016_timegenie_named_template_bootstrap_fix.sql
\ir 20261016b_timegenie_timer_transition.sql
\ir 20261016b_timegenie_timer_transition.sql
\ir 20261018b_timegenie_operation_history.sql
\ir 20261018b_timegenie_operation_history.sql
\ir 20261018c_timegenie_workspace_snapshots.sql
\ir 20261018c_timegenie_workspace_snapshots.sql
\ir 20261018d_timegenie_workspace_epoch.sql
\ir 20261018d_timegenie_workspace_epoch.sql
\ir 20261018e_timegenie_workspace_restore.sql
\ir 20261018e_timegenie_workspace_restore.sql
\ir 20261018g_timegenie_automatic_snapshot_schedule.sql
\ir 20261018g_timegenie_automatic_snapshot_schedule.sql
\ir 20261018h_timegenie_operation_history_page_size.sql
\ir 20261018h_timegenie_operation_history_page_size.sql
\ir 20261018i_timegenie_history_detail_and_snapshot_timeout.sql
\ir 20261018i_timegenie_history_detail_and_snapshot_timeout.sql
\ir 20261018j_timegenie_snapshot_retention.sql
\ir 20261018j_timegenie_snapshot_retention.sql
\ir 20261018k_timegenie_fixed_daily_snapshot.sql
\ir 20261018k_timegenie_fixed_daily_snapshot.sql
\ir 20261018l_timegenie_snapshot_metadata_detail.sql
\ir 20261018l_timegenie_snapshot_metadata_detail.sql
\ir 20261018m_timegenie_restore_ignore_unassigned.sql
\ir 20261018m_timegenie_restore_ignore_unassigned.sql
\ir 20261018n_timegenie_restore_timeout.sql
\ir 20261018n_timegenie_restore_timeout.sql
\ir 20261018o_timegenie_optional_pre_restore_snapshot.sql
\ir 20261018o_timegenie_optional_pre_restore_snapshot.sql
\ir 20261018p_timegenie_restore_unassigned_state.sql
\ir 20261018p_timegenie_restore_unassigned_state.sql
\ir 20261019_timegenie_timer_recurring_occurrence.sql
\ir 20261019_timegenie_timer_recurring_occurrence.sql
\ir 20261020_timegenie_actual_adjustment_target_rebase.sql
\ir 20261020_timegenie_actual_adjustment_target_rebase.sql
\ir 20261020b_timegenie_actual_adjustment_snapshot.sql
\ir 20261020b_timegenie_actual_adjustment_snapshot.sql
\ir 20261021_timegenie_today_task_instances.sql
\ir 20261021_timegenie_today_task_instances.sql
\ir 20261021b_timegenie_client_write_generation.sql
\ir 20261021b_timegenie_client_write_generation.sql

do $$
begin
  if has_table_privilege('anon', 'timegenie.workspaces', 'select') then
    raise exception 'anon can read business tables';
  end if;
  if has_function_privilege('anon', 'timegenie.cloud_snapshot_get(uuid)', 'execute') then
    raise exception 'anon can execute protected RPCs';
  end if;
  if has_function_privilege('anon', 'timegenie.record_processed_operation(uuid, uuid, uuid, text, jsonb, jsonb)', 'execute') then
    raise exception 'anon can execute internal idempotency helper';
  end if;
  if not has_table_privilege('authenticated', 'timegenie.workspaces', 'select,insert') then
    raise exception 'authenticated is missing workspace bootstrap privileges';
  end if;
  if has_table_privilege('authenticated', 'timegenie.tasks', 'insert,update,delete') then
    raise exception 'authenticated can directly write synced business tables';
  end if;
  if has_table_privilege('authenticated', 'timegenie.tasks', 'select') then
    raise exception 'authenticated can directly read synced business tables instead of using snapshot RPC';
  end if;
  if has_table_privilege('authenticated', 'timegenie.devices', 'select,insert,update,delete') then
    raise exception 'authenticated can bypass device lifecycle RPCs with direct table access';
  end if;
  if not has_table_privilege('authenticated', 'timegenie.workspace_changes', 'select') then
    raise exception 'authenticated is missing change feed read access';
  end if;
  if not has_table_privilege('authenticated', 'timegenie.tracking_leases', 'select') then
    raise exception 'authenticated is missing tracking lease read access';
  end if;
  if not has_table_privilege('authenticated', 'timegenie.time_entries', 'select') then
    raise exception 'authenticated is missing timer read access';
  end if;
  if not has_function_privilege('authenticated', 'timegenie.cloud_snapshot_get(uuid)', 'execute') then
    raise exception 'authenticated is missing RPC execute privilege';
  end if;
  if not has_function_privilege('authenticated', 'timegenie.cloud_incremental_pull(uuid, uuid, bigint, integer)', 'execute') then
    raise exception 'authenticated is missing incremental pull RPC execute privilege';
  end if;
  if has_function_privilege('anon', 'timegenie.cloud_incremental_pull(uuid, uuid, bigint, integer)', 'execute') then
    raise exception 'anon can execute incremental pull RPC';
  end if;
  if has_function_privilege('authenticated', 'timegenie.cloud_incremental_entity_get(uuid, text, text)', 'execute') then
    raise exception 'authenticated can execute internal incremental entity helper';
  end if;
  if not has_function_privilege('authenticated', 'timegenie.cloud_apply_patch(uuid, uuid, uuid, text, text, text, bigint, jsonb)', 'execute') then
    raise exception 'authenticated is missing cloud apply RPC execute privilege';
  end if;
  if not has_function_privilege('authenticated', 'timegenie.cloud_apply_patch(uuid, uuid, uuid, text, text, text, bigint, bigint, bigint, jsonb)', 'execute') then
    raise exception 'authenticated is missing coalesced timer apply RPC execute privilege';
  end if;
  if not has_function_privilege('authenticated', 'timegenie.timer_sync_context_get(uuid, uuid, uuid)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.timer_sync_context_get(uuid, uuid, uuid, text, jsonb)', 'execute') then
    raise exception 'authenticated is missing timer sync context privileges';
  end if;
  if not has_function_privilege('authenticated', 'timegenie.tracking_lease_acquire(uuid, uuid, uuid)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.tracking_lease_renew(uuid, uuid, uuid)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.tracking_lease_takeover_for_timer_write(uuid, uuid, uuid)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.tracking_lease_release(uuid, uuid, uuid)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.unassigned_tick(uuid, uuid, uuid)', 'execute') then
    raise exception 'authenticated is missing tracking lease RPC privileges';
  end if;
  if has_function_privilege('authenticated', 'timegenie.apply_calendar_time_entry(uuid, uuid, uuid, text, text, bigint, jsonb, jsonb)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.apply_calendar_time_entry_pre_timer_authority(uuid, uuid, uuid, text, text, bigint, jsonb, jsonb)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.timer_write_lease_context(uuid, uuid)', 'execute') then
    raise exception 'authenticated can execute an internal timer authority helper';
  end if;
  if not has_function_privilege('authenticated', 'timegenie.device_authorize(uuid, uuid, text, text, text)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.device_authorization_get(uuid, uuid)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.device_list(uuid)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.device_revoke(uuid, uuid)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.device_revoke_all(uuid)', 'execute') then
    raise exception 'authenticated is missing device lifecycle RPC privileges';
  end if;
  if has_function_privilege('anon', 'timegenie.device_revoke(uuid, uuid)', 'execute') then
    raise exception 'anon can revoke devices';
  end if;
  if has_function_privilege('authenticated', 'timegenie.is_workspace_owner(uuid)', 'execute') then
    raise exception 'authenticated can execute internal ownership helper';
  end if;
  if has_function_privilege('authenticated', 'timegenie.is_registered_device(uuid, uuid)', 'execute') then
    raise exception 'authenticated can execute internal device helper';
  end if;
  if has_function_privilege('authenticated', 'timegenie.current_auth_session_id()', 'execute') then
    raise exception 'authenticated can execute internal auth session helper';
  end if;
  if has_function_privilege('authenticated', 'timegenie.touch_workspace_change()', 'execute') then
    raise exception 'authenticated can execute internal trigger helper';
  end if;
  if has_function_privilege('authenticated', 'timegenie.operation_request_hash(text, jsonb)', 'execute') then
    raise exception 'authenticated can execute internal idempotency hash helper';
  end if;
  if has_function_privilege('authenticated', 'timegenie.processed_operation_result(uuid, uuid, text, jsonb)', 'execute') then
    raise exception 'authenticated can execute internal idempotency read helper';
  end if;
  if has_function_privilege('authenticated', 'timegenie.record_processed_operation(uuid, uuid, uuid, text, jsonb, jsonb)', 'execute') then
    raise exception 'authenticated can execute internal idempotency helper';
  end if;
  if not has_function_privilege('authenticated', 'timegenie.actual_adjustment_batch_apply(uuid, uuid, uuid, jsonb)', 'execute') then
    raise exception 'authenticated is missing actual adjustment batch RPC privileges';
  end if;
  if has_function_privilege('anon', 'timegenie.actual_adjustment_batch_apply(uuid, uuid, uuid, jsonb)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.actual_adjustment_fingerprint(uuid, uuid, date)', 'execute') then
    raise exception 'actual adjustment internal privileges are too broad';
  end if;
  if has_function_privilege('authenticated', 'timegenie.unassigned_resolve_non_work(uuid, uuid, uuid, bigint, uuid, text)', 'execute') then
    raise exception 'authenticated can execute internal unassigned helper';
  end if;
end $$;

begin;
select set_config('request.headers', '{"x-timegenie-client-schema-generation":"1"}', true);

insert into auth.users(id) values
  ('10000000-0000-0000-0000-000000000001'),
  ('10000000-0000-0000-0000-000000000002');

set local role authenticated;
select set_config('request.jwt.claim.sub', '10000000-0000-0000-0000-000000000001', true);
select set_config('request.jwt.claim.session_id', '80000000-0000-0000-0000-000000000001', true);

select set_config('request.jwt.claim.sub', '10000000-0000-0000-0000-000000000002', true);

insert into timegenie.workspaces(id, owner_user_id, name) values
  ('20000000-0000-0000-0000-000000000002', '10000000-0000-0000-0000-000000000002', '账号 B 未归属测试工作空间');

select timegenie.device_authorize('20000000-0000-0000-0000-000000000002', '30000000-0000-0000-0000-000000000003', '设备 C', 'windows', '0.1.0');

select set_config('request.headers', '{}', true);
do $$
begin
  begin
    perform timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-000000000002',
      '30000000-0000-0000-0000-000000000003',
      '40000000-0000-0000-0000-000000000098',
      'app_setting.update','app_setting','old_client_write_probe',null,
      jsonb_build_object('key','old_client_write_probe','value_json','true'::jsonb,'updated_at',1792271999000,'version',1)
    );
    raise exception 'old client write was accepted';
  exception when others then
    if sqlerrm not like '%CLIENT_UPGRADE_REQUIRED%' then raise; end if;
  end;
end $$;
select set_config('request.headers', '{"x-timegenie-client-schema-generation":"1"}', true);

select timegenie.cloud_apply_patch(
  '20000000-0000-0000-0000-000000000002',
  '30000000-0000-0000-0000-000000000003',
  '40000000-0000-0000-0000-000000000099',
  'unassigned_session.update',
  'unassigned_session',
  '71000000-0000-0000-0000-000000000099',
  null,
  jsonb_build_object(
    'id', '71000000-0000-0000-0000-000000000099',
    'work_date', '2026-09-25',
    'state', 'collecting',
    'threshold_seconds', 300,
    'duration_seconds', 60,
    'first_started_at', 1789779600000,
    'created_at', 1789779600000,
    'updated_at', 1789779660000,
    'version', 1,
    'segments', jsonb_build_array()
  )
);

reset role;
do $$
begin
  begin
    perform timegenie.migration_import_snapshot(
      '20000000-0000-0000-0000-000000000002',
      '40000000-0000-0000-0000-000000000100',
      '30000000-0000-0000-0000-000000000003',
      '{}'::jsonb
    );
    raise exception 'migration accepted a target that only had unassigned cloud data';
  exception
    when others then
      if sqlerrm not like '%MIGRATION_TARGET_NOT_EMPTY%' then raise; end if;
  end;
end $$;

select set_config('request.jwt.claim.sub', '10000000-0000-0000-0000-000000000001', true);

insert into timegenie.workspaces(id, owner_user_id, name) values
  ('20000000-0000-0000-0000-000000000001', '10000000-0000-0000-0000-000000000001', '账号 A 工作空间');

select timegenie.workspace_initialize_defaults('20000000-0000-0000-0000-000000000001');
select timegenie.workspace_initialize_defaults('20000000-0000-0000-0000-000000000001');

do $$
begin
  if (select count(*) from timegenie.report_templates where workspace_id='20000000-0000-0000-0000-000000000001' and deleted_at is null)<>3
     or exists(select 1 from timegenie.report_templates where workspace_id='20000000-0000-0000-0000-000000000001' and nullif(btrim(name),'') is null)
     or (select count(*) from timegenie.report_templates where workspace_id='20000000-0000-0000-0000-000000000001' and is_default and deleted_at is null)<>3 then
    raise exception 'workspace bootstrap did not create exactly three named default report templates';
  end if;
  if (select count(*) from timegenie.subject_work_schedules where workspace_id='20000000-0000-0000-0000-000000000001')<>7
     or (select count(*) from timegenie.subject_work_schedule_segments where workspace_id='20000000-0000-0000-0000-000000000001')<>10 then
    raise exception 'workspace bootstrap did not create an idempotent default work schedule';
  end if;
end $$;

select timegenie.device_authorize('20000000-0000-0000-0000-000000000001', '30000000-0000-0000-0000-000000000001', '设备 A', 'windows', '0.2.1');
reset role;
insert into timegenie.tracking_leases(workspace_id, holder_device_id, lease_token, expires_at)
values('20000000-0000-0000-0000-000000000001', '30000000-0000-0000-0000-000000000001', '50000000-0000-0000-0000-000000000001', now() + interval '45 seconds')
on conflict(workspace_id) do update set holder_device_id = excluded.holder_device_id, lease_token = excluded.lease_token, expires_at = excluded.expires_at;
set local role authenticated;
select timegenie.device_revoke('20000000-0000-0000-0000-000000000001', '30000000-0000-0000-0000-000000000001');
select timegenie.device_revoke('20000000-0000-0000-0000-000000000001', '30000000-0000-0000-0000-000000000001');
do $$
begin
  if (select holder_device_id is not null from timegenie.tracking_leases where workspace_id = '20000000-0000-0000-0000-000000000001') then
    raise exception 'device revoke did not release tracking lease';
  end if;
  if (timegenie.device_authorization_get('20000000-0000-0000-0000-000000000001', '30000000-0000-0000-0000-000000000001')->>'authorized')::boolean then
    raise exception 'revoked device remains registered';
  end if;
end $$;
do $$
begin
  begin
    perform timegenie.device_authorize('20000000-0000-0000-0000-000000000001', '30000000-0000-0000-0000-000000000001', '设备 A', 'windows', '0.2.1');
    raise exception 'revoked device was reauthorized by the old auth session';
  exception
    when others then
      if sqlerrm not like '%PASSWORD_REAUTH_REQUIRED%' then raise; end if;
  end;
end $$;
select set_config('request.jwt.claim.session_id', '80000000-0000-0000-0000-000000000002', true);
select timegenie.device_authorize('20000000-0000-0000-0000-000000000001', '30000000-0000-0000-0000-000000000001', '设备 A', 'windows', '0.2.1');
do $$
begin
  if not (timegenie.device_authorization_get('20000000-0000-0000-0000-000000000001', '30000000-0000-0000-0000-000000000001')->>'authorized')::boolean then
    raise exception 'explicit device authorization did not restore access';
  end if;
end $$;
reset role;
do $$
begin
  if (select reauthorized_at is null from timegenie.devices where id = '30000000-0000-0000-0000-000000000001') then
    raise exception 'device reauthorization audit time is missing';
  end if;
end $$;
set local role authenticated;

do $$
declare snapshot jsonb;
begin
  snapshot := timegenie.cloud_snapshot_get('20000000-0000-0000-0000-000000000001');
  if jsonb_array_length(snapshot->'subjects') <> 1 then
    raise exception 'workspace defaults created duplicate subjects';
  end if;
  if jsonb_array_length(snapshot->'app_settings') <> 5 then
    raise exception 'workspace defaults created duplicate settings';
  end if;
  if jsonb_array_length(snapshot->'report_templates') <> 3 then
    raise exception 'workspace defaults created duplicate templates';
  end if;
end $$;

select timegenie.device_authorize('20000000-0000-0000-0000-000000000001', '30000000-0000-0000-0000-000000000002', '设备 B', 'windows', '0.1.0');

-- Security-definer RPCs must reject unknown or revoked device identities even
-- when the authenticated user owns the workspace.
reset role;
insert into timegenie.devices(id, workspace_id, device_name, platform, app_version, revoked_at) values
  ('30000000-0000-0000-0000-000000000098', '20000000-0000-0000-0000-000000000001', '已撤销设备', 'windows', '0.1.0', now());
set local role authenticated;

do $$
begin
  begin
    perform timegenie.tracking_lease_acquire(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000098',
      '90000000-0000-0000-0000-000000000098'
    );
    raise exception 'revoked device acquired a tracking lease';
  exception
    when others then
      if sqlerrm not like '%DEVICE_NOT_REGISTERED%' then raise; end if;
  end;

  begin
    perform timegenie.timer_start(
      '20000000-0000-0000-0000-000000000001',
      null,
      '30000000-0000-0000-0000-000000000098',
      '40000000-0000-0000-0000-000000000098',
      '已撤销设备启动'
    );
    raise exception 'revoked device started a timer';
  exception
    when others then
      if sqlerrm not like '%DEVICE_NOT_REGISTERED%' then raise; end if;
  end;

  begin
    perform timegenie.unassigned_resolve_work(
      '20000000-0000-0000-0000-000000000001',
      '91000000-0000-0000-0000-000000000098',
      '30000000-0000-0000-0000-000000000098',
      1,
      '[]'::jsonb,
      '40000000-0000-0000-0000-000000000099'
    );
    raise exception 'revoked device resolved unassigned time';
  exception
    when others then
      if sqlerrm not like '%DEVICE_NOT_REGISTERED%' then raise; end if;
  end;

  begin
    perform timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000098',
      '40000000-0000-0000-0000-000000000098',
      'app_setting.update',
      'app_setting',
      'revoked_device_upload_probe',
      null,
      jsonb_build_object(
        'key', 'revoked_device_upload_probe',
        'value_json', 'true'::jsonb,
        'updated_at', 1791100800000,
        'version', 1
      )
    );
    raise exception 'revoked device uploaded an outbox operation';
  exception
    when others then
      if sqlerrm not like '%DEVICE_NOT_REGISTERED%' then raise; end if;
  end;

  begin
    perform timegenie.cloud_incremental_pull(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000098',
      0,
      10
    );
    raise exception 'revoked device read incremental changes';
  exception
    when others then
      if sqlerrm not like '%DEVICE_REVOKED%' then raise; end if;
  end;
end $$;

select timegenie.cloud_apply_patch(
  '20000000-0000-0000-0000-000000000001',
  '30000000-0000-0000-0000-000000000002',
  '40000000-0000-0000-0000-000000000097',
  'app_setting.update',
  'app_setting',
  'authorized_device_upload_probe',
  null,
  jsonb_build_object(
    'key', 'authorized_device_upload_probe',
    'value_json', 'true'::jsonb,
    'updated_at', 1791100800000,
    'version', 1
  )
);

-- A different authenticated user cannot read or write account A's workspace.
select set_config('request.jwt.claim.sub', '10000000-0000-0000-0000-000000000002', true);
do $$
begin
  if (select count(*) from timegenie.workspaces where id = '20000000-0000-0000-0000-000000000001') <> 0 then
    raise exception 'RLS exposed another user workspace';
  end if;
  begin
    perform timegenie.cloud_snapshot_get('20000000-0000-0000-0000-000000000001');
    raise exception 'RLS exposed another user snapshot';
  exception
    when others then
      if sqlerrm not like '%AUTH_REQUIRED%' then raise; end if;
  end;
  begin
    perform timegenie.cloud_incremental_pull(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000001',
      0,
      10
    );
    raise exception 'incremental RPC exposed another user workspace';
  exception
    when others then
      if sqlerrm not like '%AUTH_REQUIRED%' then raise; end if;
  end;
  begin
    insert into timegenie.devices(id, workspace_id, device_name, platform, app_version) values
      ('30000000-0000-0000-0000-000000000099', '20000000-0000-0000-0000-000000000001', '越权设备', 'windows', '0.1.0');
    raise exception 'RLS allowed a device in another user workspace';
  exception
    when insufficient_privilege then null;
  end;
end $$;

select set_config('request.jwt.claim.sub', '10000000-0000-0000-0000-000000000001', true);

-- Replace generated defaults with a complete local snapshot containing a task
-- tree, one time entry, one report, and their relationships.
select timegenie.migration_import_snapshot(
  '20000000-0000-0000-0000-000000000001',
  '40000000-0000-0000-0000-000000000001',
  '30000000-0000-0000-0000-000000000001',
  jsonb_build_object(
    'subjects', jsonb_build_array(jsonb_build_object(
      'id', '50000000-0000-0000-0000-000000000001',
      'name', '默认',
      'sort_order', 10,
      'created_at', 1789689600000,
      'updated_at', 1789689600000,
      'version', 1
    )),
    'app_settings', jsonb_build_array(
      jsonb_build_object(
        'key', 'salary_hourly_rate',
        'value_json', 88.5,
        'updated_at', 1789689600000,
        'version', 1
      ),
      jsonb_build_object(
        'key', 'timezone',
        'value_json', 'Asia/Shanghai',
        'updated_at', 1789689600000,
        'version', 1
      )
    ),
    'integration_configs', jsonb_build_array(jsonb_build_object(
      'provider', 'seatable',
      'enabled', 1,
      'config_json', jsonb_build_object('serverUrl', 'https://example.test', 'batchSize', 100),
      'updated_at', 1789689600000,
      'version', 1
    )),
    'tasks', jsonb_build_array(
      jsonb_build_object(
        'id', '60000000-0000-0000-0000-000000000001',
        'subject_id', '50000000-0000-0000-0000-000000000001',
        'title', '运维相关',
        'status', 'open',
        'planned_date', '2026-09-24',
        'source_type', 'manual',
        'sort_order', 10,
        'created_at', 1789689600000,
        'updated_at', 1789689600000,
        'version', 1
      ),
      jsonb_build_object(
        'id', '60000000-0000-0000-0000-000000000002',
        'subject_id', '50000000-0000-0000-0000-000000000001',
        'parent_id', '60000000-0000-0000-0000-000000000001',
        'title', '验证云端同步',
        'status', 'open',
        'planned_date', '2026-09-24',
        'estimate_minutes', 30,
        'source_type', 'manual',
        'sort_order', 20,
        'created_at', 1789689600000,
        'updated_at', 1789689600000,
        'version', 1
      )
    ),
    'task_daily_estimates', jsonb_build_array(jsonb_build_object(
      'task_id', '60000000-0000-0000-0000-000000000002',
      'work_date', '2026-09-24',
      'estimate_minutes', 45,
      'created_at', 1789689600000,
      'updated_at', 1789689600000,
      'version', 1
    )),
    'task_recurrence_rules', jsonb_build_array(jsonb_build_object(
      'id', '90000000-0000-0000-0000-000000000001',
      'task_id', '60000000-0000-0000-0000-000000000002',
      'frequency', 'daily',
      'effective_start', '2026-09-24',
      'created_at', 1789689600000,
      'updated_at', 1789689600000,
      'version', 1
    )),
    'task_occurrences', jsonb_build_array(jsonb_build_object(
      'task_id', '60000000-0000-0000-0000-000000000002',
      'occurrence_date', '2026-09-24',
      'origin', 'scheduled',
      'status', 'open',
      'created_at', 1789689600000,
      'updated_at', 1789689600000,
      'version', 1
    )),
    'time_entries', jsonb_build_array(jsonb_build_object(
      'id', '70000000-0000-0000-0000-000000000001',
      'work_date', '2026-09-24',
      'kind', 'work',
      'source_type', 'manual',
      'state', 'ended',
      'default_task_id', '60000000-0000-0000-0000-000000000002',
      'label_snapshot', '运维相关 / 验证云端同步',
      'started_at', 1789693200000,
      'ended_at', 1789695000000,
      'duration_seconds', 1800,
      'origin_unassigned_session_id', '71000000-0000-0000-0000-000000000001',
      'created_at', 1789695000000,
      'updated_at', 1789695000000,
      'version', 1
    )),
    'unassigned_sessions', jsonb_build_array(jsonb_build_object(
      'id', '71000000-0000-0000-0000-000000000001',
      'work_date', '2026-09-24',
      'state', 'resolved',
      'threshold_seconds', 300,
      'duration_seconds', 1800,
      'first_started_at', 1789693200000,
      'last_ended_at', 1789695000000,
      'resolution_type', 'work',
      'generated_entry_id', '70000000-0000-0000-0000-000000000001',
      'resolved_at', 1789695000000,
      'created_at', 1789693200000,
      'updated_at', 1789695000000,
      'version', 1
    )),
    'unassigned_segments', jsonb_build_array(jsonb_build_object(
      'id', '72000000-0000-0000-0000-000000000001',
      'session_id', '71000000-0000-0000-0000-000000000001',
      'sequence_no', 1,
      'started_at', 1789693200000,
      'ended_at', 1789695000000,
      'duration_seconds', 1800
    )),
    'reports', jsonb_build_array(jsonb_build_object(
      'id', '80000000-0000-0000-0000-000000000001',
      'report_type', 'daily',
      'subject_id', '50000000-0000-0000-0000-000000000001',
      'period_start', '2026-09-24',
      'period_end', '2026-09-24',
      'reference_date', '2026-09-24',
      'markdown_content', '# 2026-09-24',
      'content_source', 'generated',
      'generation_count', 1,
      'created_at', 1789695000000,
      'updated_at', 1789695000000,
      'version', 1
    )),
    'report_tasks', jsonb_build_array(jsonb_build_object(
      'report_id', '80000000-0000-0000-0000-000000000001',
      'task_id', '60000000-0000-0000-0000-000000000002',
      'sort_order', 10,
      'title_snapshot', '验证云端同步',
      'path_snapshot', '运维相关 / 验证云端同步'
    ))
  )
);

-- Reusing an operation id with different content must be rejected. A new
-- migration operation must also be rejected because the target is no longer empty.
do $$
begin
  begin
    perform timegenie.migration_import_snapshot(
      '20000000-0000-0000-0000-000000000001',
      '40000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000001',
      '{}'::jsonb
    );
    raise exception 'migration accepted a reused operation id with different content';
  exception
    when others then
      if sqlerrm not like '%IDEMPOTENCY_CONFLICT%' then raise; end if;
  end;
end $$;

-- Incremental reads preserve ordered cursor coverage, page without gaps,
-- collapse repeated changes for one entity to its final image, and return an
-- explicit tombstone after deletion.
reset role;
do $$
declare before_seq bigint;
begin
  select coalesce(max(change_seq), 0) into before_seq
  from timegenie.workspace_changes
  where workspace_id = '20000000-0000-0000-0000-000000000001';
  perform set_config('timegenie.incremental_test_before_seq', before_seq::text, true);

  insert into timegenie.app_settings(workspace_id, key, value_json, version)
  values('20000000-0000-0000-0000-000000000001', 'incremental_contract_probe', '{"step":1}'::jsonb, 1);
  update timegenie.app_settings
  set value_json = '{"step":2}'::jsonb, version = 2, updated_at = now()
  where workspace_id = '20000000-0000-0000-0000-000000000001'
    and key = 'incremental_contract_probe';
  delete from timegenie.app_settings
  where workspace_id = '20000000-0000-0000-0000-000000000001'
    and key = 'incremental_contract_probe';
end $$;
set local role authenticated;
select set_config('request.jwt.claim.sub', '10000000-0000-0000-0000-000000000001', true);

do $$
declare before_seq bigint := current_setting('timegenie.incremental_test_before_seq')::bigint;
declare first_page jsonb;
declare folded jsonb;
declare first_change_seq bigint;
begin
  first_page := timegenie.cloud_incremental_pull(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    before_seq,
    1
  );
  if jsonb_array_length(first_page->'changes') <> 1
     or not (first_page->>'has_more')::boolean
     or (first_page->>'reset_required')::boolean then
    raise exception 'incremental first page metadata is invalid: %', first_page;
  end if;
  first_change_seq := (first_page->'changes'->0->>'change_seq')::bigint;
  if first_change_seq <> before_seq + 1
     or (first_page->>'covered_change_seq')::bigint <> first_change_seq
     or (first_page->>'latest_change_seq')::bigint < first_change_seq + 2 then
    raise exception 'incremental first page cursor coverage is invalid: %', first_page;
  end if;

  folded := timegenie.cloud_incremental_pull(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    before_seq,
    100
  );
  if jsonb_array_length(folded->'changes') <> 3
     or jsonb_array_length(folded->'entities') <> 1
     or (folded->>'has_more')::boolean then
    raise exception 'incremental repeated changes were not folded: %', folded;
  end if;
  if folded->'entities'->0->>'source_entity_type' <> 'app_settings'
     or folded->'entities'->0->>'entity_id' <> 'incremental_contract_probe'
     or (folded->'entities'->0->>'change_seq')::bigint <> (folded->>'covered_change_seq')::bigint
     or not (folded->'entities'->0->>'deleted')::boolean
     or folded->'entities'->0->'data' is distinct from 'null'::jsonb then
    raise exception 'incremental delete did not return an explicit tombstone: %', folded;
  end if;
end $$;

do $$
declare snapshot jsonb;
begin
  snapshot := timegenie.cloud_snapshot_get('20000000-0000-0000-0000-000000000001');
  if jsonb_array_length(snapshot->'tasks') <> 2 then
    raise exception 'migration did not preserve the task tree';
  end if;
  if not exists(
    select 1
    from jsonb_array_elements(snapshot->'tasks') task
    where task->>'id' = '60000000-0000-0000-0000-000000000002'
      and task->>'parent_id' = '60000000-0000-0000-0000-000000000001'
  ) then
    raise exception 'migration did not restore the task parent';
  end if;
  if not exists(
    select 1
    from jsonb_array_elements(snapshot->'time_entries') entry
    where entry->>'id' = '70000000-0000-0000-0000-000000000001'
      and entry->>'origin_unassigned_session_id' = '71000000-0000-0000-0000-000000000001'
  ) then
    raise exception 'migration did not import the time entry with its unassigned origin';
  end if;
  if not exists(
    select 1
    from jsonb_array_elements(snapshot->'unassigned_sessions') session
    where session->>'id' = '71000000-0000-0000-0000-000000000001'
      and session->>'generated_entry_id' = '70000000-0000-0000-0000-000000000001'
  ) then
    raise exception 'migration did not preserve the unassigned session generated entry';
  end if;
  if not exists(
    select 1
    from jsonb_array_elements(snapshot->'unassigned_segments') segment
    where segment->>'session_id' = '71000000-0000-0000-0000-000000000001'
  ) then
    raise exception 'migration did not preserve the unassigned segment';
  end if;
  if jsonb_array_length(snapshot->'task_daily_estimates') <> 1
     or jsonb_array_length(snapshot->'task_recurrence_rules') <> 1
     or jsonb_array_length(snapshot->'task_occurrences') <> 1 then
    raise exception 'migration did not preserve recurring task data';
  end if;
  if not exists(
    select 1
    from jsonb_array_elements(snapshot->'task_daily_estimates') estimate
    where estimate->>'task_id' = '60000000-0000-0000-0000-000000000002'
      and (estimate->>'estimate_minutes')::integer = 45
  ) then
    raise exception 'migration did not import the daily estimate';
  end if;
  if not exists(
    select 1
    from jsonb_array_elements(snapshot->'task_recurrence_rules') rule
    where rule->>'id' = '90000000-0000-0000-0000-000000000001'
      and rule->>'frequency' = 'daily'
  ) then
    raise exception 'migration did not import the recurrence rule';
  end if;
  if not exists(
    select 1
    from jsonb_array_elements(snapshot->'task_occurrences') occurrence
    where occurrence->>'task_id' = '60000000-0000-0000-0000-000000000002'
      and occurrence->>'occurrence_date' = '2026-09-24'
  ) then
    raise exception 'migration did not import the task occurrence';
  end if;
  if not exists(
    select 1
    from jsonb_array_elements(snapshot->'reports') report
    where report->>'id' = '80000000-0000-0000-0000-000000000001'
  ) then
    raise exception 'migration did not import the report';
  end if;
  if not exists(
    select 1
    from jsonb_array_elements(snapshot->'app_settings') setting
    where setting->>'key' = 'salary_hourly_rate'
      and jsonb_typeof(setting->'value_json') = 'number'
  ) then
    raise exception 'migration changed numeric setting type';
  end if;
  if not exists(
    select 1
    from jsonb_array_elements(snapshot->'app_settings') setting
    where setting->>'key' = 'timezone'
      and setting->>'value_json' = 'Asia/Shanghai'
  ) then
    raise exception 'migration changed string setting value';
  end if;
  if not exists(
    select 1
    from jsonb_array_elements(snapshot->'integration_configs') config
    where config->>'provider' = 'seatable'
      and jsonb_typeof(config->'config_json') = 'object'
  ) then
    raise exception 'migration changed integration config type';
  end if;
  begin
    perform timegenie.migration_import_snapshot(
      '20000000-0000-0000-0000-000000000001',
      '40000000-0000-0000-0000-000000000002',
      '30000000-0000-0000-0000-000000000001',
      '{}'::jsonb
    );
    raise exception 'migration accepted a non-empty target';
  exception
    when others then
      if sqlerrm not like '%MIGRATION_TARGET_NOT_EMPTY%' then raise; end if;
  end;
end $$;

-- Changes are monotonic, and stale complete-entity updates are rejected.
do $$
declare before_seq bigint; snapshot jsonb;
begin
  select max(change_seq) into before_seq from timegenie.workspace_changes
  where workspace_id = '20000000-0000-0000-0000-000000000001';

  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000010',
    'task.update',
    'task',
    '60000000-0000-0000-0000-000000000002',
    1,
    jsonb_build_object(
      'id', '60000000-0000-0000-0000-000000000002',
      'subject_id', '50000000-0000-0000-0000-000000000001',
      'parent_id', '60000000-0000-0000-0000-000000000001',
      'title', '验证云端同步（设备 A）',
      'status', 'open',
      'planned_date', '2026-09-24',
      'estimate_minutes', 30,
      'source_type', 'manual',
      'sort_order', 20,
      'created_at', 1789689600000,
      'updated_at', 1789696800000,
      'version', 2
    )
  );

  if (select max(change_seq) from timegenie.workspace_changes where workspace_id = '20000000-0000-0000-0000-000000000001') <= before_seq then
    raise exception 'workspace change sequence did not advance';
  end if;

  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000015',
    'time_entry.update',
    'time_entry',
    '70000000-0000-0000-0000-000000000001',
    1,
    jsonb_build_object(
      'id', '70000000-0000-0000-0000-000000000001',
      'work_date', '2026-09-24',
      'kind', 'work',
      'source_type', 'manual',
      'state', 'ended',
      'default_task_id', '60000000-0000-0000-0000-000000000002',
      'label_snapshot', '运维相关 / 验证云端同步',
      'started_at', 1789693200000,
      'ended_at', 1789695600000,
      'duration_seconds', 2400,
      'note', '修正后仍保留未归属来源',
      'origin_unassigned_session_id', '71000000-0000-0000-0000-000000000001',
      'created_at', 1789695000000,
      'updated_at', 1789695600000,
      'version', 2,
      'segments', jsonb_build_array(jsonb_build_object(
        'id', '73000000-0000-0000-0000-000000000001',
        'sequence_no', 1,
        'started_at', 1789693200000,
        'ended_at', 1789695600000,
        'duration_seconds', 2400
      )),
      'allocations', jsonb_build_array(jsonb_build_object(
        'id', '74000000-0000-0000-0000-000000000001',
        'task_id', '60000000-0000-0000-0000-000000000002',
        'minutes', 40,
        'note', '修正归属',
        'created_at', 1789695600000,
        'updated_at', 1789695600000,
        'version', 1
      ))
    )
  );

  if not exists(
    select 1
    from timegenie.time_entries
    where id = '70000000-0000-0000-0000-000000000001'
      and origin_unassigned_session_id = '71000000-0000-0000-0000-000000000001'
      and duration_seconds = 2400
      and version = 2
  ) then
    raise exception 'time entry patch did not preserve unassigned origin';
  end if;

  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000040',
    'time_entry.update.with-newer-work-day',
    'time_entry',
    '70000000-0000-0000-0000-000000000001',
    2,
    jsonb_build_object(
      'id', '70000000-0000-0000-0000-000000000001',
      'work_date', '2026-09-24',
      'kind', 'work',
      'source_type', 'manual',
      'state', 'ended',
      'default_task_id', '60000000-0000-0000-0000-000000000002',
      'label_snapshot', '运维相关 / 验证云端同步',
      'started_at', 1789693200000,
      'ended_at', 1789695600000,
      'duration_seconds', 2400,
      'note', '先带入新版 work_day',
      'origin_unassigned_session_id', '71000000-0000-0000-0000-000000000001',
      'created_at', 1789695000000,
      'updated_at', 1789696200000,
      'version', 3,
      'segments', jsonb_build_array(jsonb_build_object(
        'id', '73000000-0000-0000-0000-000000000001',
        'sequence_no', 1,
        'started_at', 1789693200000,
        'ended_at', 1789695600000,
        'duration_seconds', 2400
      )),
      'allocations', jsonb_build_array(),
      'work_day', jsonb_build_object(
        'work_date', '2026-09-24',
        'timezone', 'Asia/Shanghai',
        'work_period_text', null,
        'note', '新版工作日',
        'settled_at', 1789696800000,
        'created_at', 1789689600000,
        'updated_at', 1789696800000,
        'version', 5
      )
    )
  );

  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000041',
    'time_entry.update.after-newer-work-day',
    'time_entry',
    '70000000-0000-0000-0000-000000000001',
    3,
    jsonb_build_object(
      'id', '70000000-0000-0000-0000-000000000001',
      'work_date', '2026-09-24',
      'kind', 'work',
      'source_type', 'manual',
      'state', 'ended',
      'default_task_id', '60000000-0000-0000-0000-000000000002',
      'label_snapshot', '运维相关 / 验证云端同步',
      'started_at', 1789693200000,
      'ended_at', 1789695600000,
      'duration_seconds', 2400,
      'note', '旧 work_day 不应覆盖新版',
      'origin_unassigned_session_id', '71000000-0000-0000-0000-000000000001',
      'created_at', 1789695000000,
      'updated_at', 1789696500000,
      'version', 4,
      'segments', jsonb_build_array(jsonb_build_object(
        'id', '73000000-0000-0000-0000-000000000001',
        'sequence_no', 1,
        'started_at', 1789693200000,
        'ended_at', 1789695600000,
        'duration_seconds', 2400
      )),
      'allocations', jsonb_build_array(),
      'work_day', jsonb_build_object(
        'work_date', '2026-09-24',
        'timezone', 'Asia/Shanghai',
        'work_period_text', null,
        'note', '旧版本工作日',
        'settled_at', null,
        'created_at', 1789689600000,
        'updated_at', 1789695000000,
        'version', 2
      )
    )
  );

  snapshot := timegenie.cloud_snapshot_get('20000000-0000-0000-0000-000000000001');
  if not exists(
    select 1
    from jsonb_array_elements(snapshot->'work_days') as work_day
    where work_day->>'work_date' = '2026-09-24'
      and (work_day->>'version')::bigint = 5
      and work_day->>'settled_at' is not null
  ) then
    raise exception 'stale embedded work_day overwrote a newer cloud work_day';
  end if;

  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000019',
    'unassigned_session.update',
    'unassigned_session',
    '71000000-0000-0000-0000-000000000001',
    1,
    jsonb_build_object(
      'id', '71000000-0000-0000-0000-000000000001',
      'work_date', '2026-09-24',
      'state', 'resolved',
      'threshold_seconds', 300,
      'duration_seconds', 2400,
      'first_started_at', 1789693200000,
      'last_ended_at', 1789695600000,
      'prompted_at', 1789695300000,
      'resolution_type', 'work',
      'generated_entry_id', '70000000-0000-0000-0000-000000000001',
      'resolved_at', 1789695600000,
      'created_at', 1789693200000,
      'updated_at', 1789695600000,
      'version', 3,
      'segments', jsonb_build_array(jsonb_build_object(
        'id', '72000000-0000-0000-0000-000000000001',
        'sequence_no', 1,
        'started_at', 1789693200000,
        'ended_at', 1789695600000,
        'duration_seconds', 2400,
        'lease_token', null
      ))
    )
  );
  snapshot := timegenie.cloud_snapshot_get('20000000-0000-0000-0000-000000000001');
  if not exists(
    select 1
    from jsonb_array_elements(snapshot->'unassigned_sessions') session
    where session->>'id' = '71000000-0000-0000-0000-000000000001'
      and session->>'generated_entry_id' = '70000000-0000-0000-0000-000000000001'
      and session->>'state' = 'resolved'
      and (session->>'version')::bigint = 3
  ) or not exists(
    select 1
    from jsonb_array_elements(snapshot->'unassigned_segments') segment
    where segment->>'session_id' = '71000000-0000-0000-0000-000000000001'
      and segment->>'id' = '72000000-0000-0000-0000-000000000001'
      and (segment->>'duration_seconds')::bigint = 2400
  ) then
    raise exception 'unassigned session patch did not preserve resolved source session and segments';
  end if;
  if not exists(
    select 1 from timegenie.workspace_changes
    where workspace_id = '20000000-0000-0000-0000-000000000001'
      and entity_type = 'unassigned_sessions'
      and entity_id = '71000000-0000-0000-0000-000000000001'
      and entity_version = 3
  ) then
    raise exception 'unassigned session patch did not emit workspace change';
  end if;

  begin
    perform timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000002',
      '40000000-0000-0000-0000-000000000020',
      'unassigned_session.update',
      'unassigned_session',
      '71000000-0000-0000-0000-000000000001',
      1,
      jsonb_build_object(
        'id', '71000000-0000-0000-0000-000000000001',
        'work_date', '2026-09-24',
        'state', 'discarded',
        'threshold_seconds', 300,
        'duration_seconds', 2400,
        'first_started_at', 1789693200000,
        'last_ended_at', 1789695600000,
        'prompted_at', 1789695300000,
        'resolution_type', 'discard',
        'generated_entry_id', '70000000-0000-0000-0000-000000000001',
        'resolved_at', 1789695700000,
        'created_at', 1789693200000,
        'updated_at', 1789695700000,
        'version', 4,
        'segments', jsonb_build_array()
      )
    );
    raise exception 'stale unassigned session patch was accepted';
  exception
    when others then
      if sqlerrm not like '%SYNC_CONFLICT%' then raise; end if;
  end;

  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000016',
    'app_setting.update',
    'app_setting',
    'identity_probe_setting',
    null,
    jsonb_build_object(
      'key', 'identity_probe_setting',
      'value_json', 120,
      'updated_at', 1789695600000,
      'version', 1
    )
  );
  if not exists(
    select 1 from timegenie.workspace_changes
    where workspace_id = '20000000-0000-0000-0000-000000000001'
      and entity_type = 'app_settings'
      and entity_id = 'identity_probe_setting'
  ) then
    raise exception 'app setting workspace change did not preserve setting key identity';
  end if;

  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000017',
    'integration_config.update',
    'integration_config',
    'identity_probe_provider',
    null,
    jsonb_build_object(
      'provider', 'identity_probe_provider',
      'enabled', true,
      'config_json', jsonb_build_object('syncEnabled', true),
      'updated_at', 1789695600000,
      'version', 1
    )
  );
  if not exists(
    select 1 from timegenie.workspace_changes
    where workspace_id = '20000000-0000-0000-0000-000000000001'
      and entity_type = 'integration_configs'
      and entity_id = 'identity_probe_provider'
  ) then
    raise exception 'integration config workspace change did not preserve provider identity';
  end if;

  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000018',
    'external_binding.upsert',
    'external_binding',
    'seatable|task|60000000-0000-0000-0000-000000000002',
    null,
    jsonb_build_object(
      'id', '76000000-0000-0000-0000-000000000001',
      'provider', 'seatable',
      'entity_type', 'task',
      'entity_id', '60000000-0000-0000-0000-000000000002',
      'external_id', 'sea-row-1',
      'external_revision', 'rev-1',
      'last_synced_hash', 'hash-1',
      'last_synced_at', 1789695600000
    )
  );
  if not exists(
    select 1 from timegenie.workspace_changes
    where workspace_id = '20000000-0000-0000-0000-000000000001'
      and entity_type = 'external_bindings'
      and entity_id = 'seatable|task|60000000-0000-0000-0000-000000000002'
  ) then
    raise exception 'external binding workspace change did not preserve bound entity identity';
  end if;

  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000010',
    'task.update',
    'task',
    '60000000-0000-0000-0000-000000000002',
    1,
    jsonb_build_object(
      'id', '60000000-0000-0000-0000-000000000002',
      'subject_id', '50000000-0000-0000-0000-000000000001',
      'parent_id', '60000000-0000-0000-0000-000000000001',
      'title', '验证云端同步（设备 A）',
      'status', 'open',
      'planned_date', '2026-09-24',
      'estimate_minutes', 30,
      'source_type', 'manual',
      'sort_order', 20,
      'created_at', 1789689600000,
      'updated_at', 1789696800000,
      'version', 2
    )
  );

  begin
    perform timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000001',
      '40000000-0000-0000-0000-000000000010',
      'task.update',
      'task',
      '60000000-0000-0000-0000-000000000002',
      1,
      jsonb_build_object(
        'id', '60000000-0000-0000-0000-000000000002',
        'subject_id', '50000000-0000-0000-0000-000000000001',
        'parent_id', '60000000-0000-0000-0000-000000000001',
        'title', '同 ID 不同内容不应当被当作重试',
        'status', 'open',
        'planned_date', '2026-09-24',
        'estimate_minutes', 30,
        'source_type', 'manual',
        'sort_order', 20,
        'created_at', 1789689600000,
        'updated_at', 1789696800000,
        'version', 2
      )
    );
    raise exception 'operation id accepted different payload';
  exception
    when others then
      if sqlerrm not like '%IDEMPOTENCY_CONFLICT%' then raise; end if;
  end;

  begin
    perform timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000002',
      '40000000-0000-0000-0000-000000000011',
      'task.update',
      'task',
      '60000000-0000-0000-0000-000000000002',
      1,
      jsonb_build_object('id', '60000000-0000-0000-0000-000000000002', 'version', 2)
    );
    raise exception 'stale patch was accepted';
  exception
    when others then
      if sqlerrm not like '%SYNC_CONFLICT%' then raise; end if;
  end;

  begin
    perform timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000002',
      '40000000-0000-0000-0000-000000000012',
      'task.create',
      'task',
      '60000000-0000-0000-0000-000000000002',
      null,
      jsonb_build_object(
        'id', '60000000-0000-0000-0000-000000000002',
        'subject_id', '50000000-0000-0000-0000-000000000001',
        'title', '创建不应覆盖已有事项',
        'status', 'open',
        'source_type', 'manual',
        'sort_order', 20,
        'created_at', 1789689600000,
        'updated_at', 1789696800000,
        'version', 1
      )
    );
    raise exception 'create patch overwrote an existing entity';
  exception
    when others then
      if sqlerrm not like '%SYNC_CONFLICT%' then raise; end if;
  end;

  begin
    perform timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000002',
      '40000000-0000-0000-0000-000000000013',
      'task.update',
      'task',
      '60000000-0000-0000-0000-000000000002',
      2,
      jsonb_build_object(
        'id', '60000000-0000-0000-0000-000000000002',
        'subject_id', '50000000-0000-0000-0000-000000000001',
        'title', '版本不能跳号',
        'status', 'open',
        'source_type', 'manual',
        'sort_order', 20,
        'created_at', 1789689600000,
        'updated_at', 1789696800000,
        'version', 4
      )
    );
    raise exception 'version jump patch was accepted';
  exception
    when others then
      if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if;
  end;

  begin
    perform timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000002',
      '40000000-0000-0000-0000-000000000014',
      'task.update',
      'task',
      '60000000-0000-0000-0000-000000000002',
      2,
      jsonb_build_object(
        'id', '60000000-0000-0000-0000-000000000001',
        'subject_id', '50000000-0000-0000-0000-000000000001',
        'title', 'payload id 不一致',
        'status', 'open',
        'source_type', 'manual',
        'sort_order', 20,
        'created_at', 1789689600000,
        'updated_at', 1789696800000,
        'version', 3
      )
    );
    raise exception 'mismatched payload id patch was accepted';
  exception
    when others then
      if sqlerrm not like '%VALIDATION_ERROR%' then raise; end if;
  end;
end $$;

-- Recurring task occurrences are complete entity patches with the composite
-- task/date identity used by the desktop outbox.
do $$
declare snapshot jsonb; occurrence_change_count bigint;
begin
  select count(*) into occurrence_change_count
  from timegenie.workspace_changes
  where workspace_id = '20000000-0000-0000-0000-000000000001'
    and entity_type = 'task_occurrences'
    and entity_id = '60000000-0000-0000-0000-000000000002|2026-09-29';

  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000030',
    'task_occurrence.set_completed',
    'task_occurrence',
    '60000000-0000-0000-0000-000000000002|2026-09-29',
    null,
    jsonb_build_object(
      'task_id', '60000000-0000-0000-0000-000000000002',
      'occurrence_date', '2026-09-29',
      'origin', 'manual',
      'status', 'done',
      'completed_at', 1789696800000,
      'created_at', 1789689600000,
      'updated_at', 1789696800000,
      'version', 2
    )
  );

  snapshot := timegenie.cloud_snapshot_get('20000000-0000-0000-0000-000000000001');
  if not exists(
    select 1
    from jsonb_array_elements(snapshot->'task_occurrences') as occurrence
    where occurrence->>'task_id' = '60000000-0000-0000-0000-000000000002'
      and occurrence->>'occurrence_date' = '2026-09-29'
      and occurrence->>'status' = 'done'
      and (occurrence->>'version')::bigint = 2
  ) then
    raise exception 'task occurrence patch was not applied';
  end if;
  if (
    select count(*)
    from timegenie.workspace_changes
    where workspace_id = '20000000-0000-0000-0000-000000000001'
      and entity_type = 'task_occurrences'
      and entity_id = '60000000-0000-0000-0000-000000000002|2026-09-29'
  ) <> occurrence_change_count + 1 then
    raise exception 'task occurrence patch produced duplicate workspace changes';
  end if;

  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000031',
    'task_occurrence.reopen',
    'task_occurrence',
    '60000000-0000-0000-0000-000000000002|2026-09-29',
    2,
    jsonb_build_object(
      'task_id', '60000000-0000-0000-0000-000000000002',
      'occurrence_date', '2026-09-29',
      'origin', 'manual',
      'status', 'open',
      'completed_at', null,
      'created_at', 1789689600000,
      'updated_at', 1789696900000,
      'version', 3
    )
  );

  begin
    perform timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000002',
      '40000000-0000-0000-0000-000000000032',
      'task_occurrence.set_completed',
      'task_occurrence',
      '60000000-0000-0000-0000-000000000002|2026-09-29',
      2,
      jsonb_build_object(
        'task_id', '60000000-0000-0000-0000-000000000002',
        'occurrence_date', '2026-09-29',
        'origin', 'manual',
        'status', 'done',
        'completed_at', 1789697000000,
        'created_at', 1789689600000,
        'updated_at', 1789697000000,
        'version', 3
      )
    );
    raise exception 'stale task occurrence patch was accepted';
  exception
    when others then
      if sqlerrm not like '%SYNC_CONFLICT%' then raise; end if;
  end;
end $$;

-- Daily estimates support composite identity, explicit clears, idempotent retries,
-- and optimistic concurrency without coupling different dates.
reset role;
insert into timegenie.subjects(id, workspace_id, name, sort_order)
values('51000000-0000-0000-0000-000000000090','20000000-0000-0000-0000-000000000001','规划同步测试',90);
insert into timegenie.tasks(id, workspace_id, subject_id, title, status, source_type, sort_order, version)
values('61000000-0000-0000-0000-000000000090','20000000-0000-0000-0000-000000000001','51000000-0000-0000-0000-000000000090','每日计划','open','manual',90,1);
set role authenticated;
do $$
declare first_result jsonb; retry_result jsonb;
begin
  first_result := timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000150','task_daily_estimate.set','task_daily_estimate',
    '61000000-0000-0000-0000-000000000090|2026-10-05',null,
    jsonb_build_object('task_id','61000000-0000-0000-0000-000000000090','work_date','2026-10-05','estimate_minutes',45,'created_at',1791158400000,'updated_at',1791158400000,'version',1)
  );
  retry_result := timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000150','task_daily_estimate.set','task_daily_estimate',
    '61000000-0000-0000-0000-000000000090|2026-10-05',null,
    jsonb_build_object('task_id','61000000-0000-0000-0000-000000000090','work_date','2026-10-05','estimate_minutes',45,'created_at',1791158400000,'updated_at',1791158400000,'version',1)
  );
  if first_result is distinct from retry_result then raise exception 'daily estimate retry changed result'; end if;

  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000151','task_daily_estimate.set','task_daily_estimate',
    '61000000-0000-0000-0000-000000000090|2026-10-05',1,
    jsonb_build_object('task_id','61000000-0000-0000-0000-000000000090','work_date','2026-10-05','estimate_minutes',60,'created_at',1791158400000,'updated_at',1791158460000,'version',2)
  );
  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000152','task_daily_estimate.set','task_daily_estimate',
    '61000000-0000-0000-0000-000000000090|2026-10-06',null,
    jsonb_build_object('task_id','61000000-0000-0000-0000-000000000090','work_date','2026-10-06','estimate_minutes',30,'created_at',1791244800000,'updated_at',1791244800000,'version',1)
  );
  reset role;
  if (select estimate_minutes from timegenie.task_daily_estimates where task_id='61000000-0000-0000-0000-000000000090' and work_date='2026-10-05') <> 60
     or (select estimate_minutes from timegenie.task_daily_estimates where task_id='61000000-0000-0000-0000-000000000090' and work_date='2026-10-06') <> 30 then
    raise exception 'daily estimate dates were not independent';
  end if;
  set role authenticated;

  begin
    perform timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002',
      '40000000-0000-0000-0000-000000000153','task_daily_estimate.set','task_daily_estimate',
      '61000000-0000-0000-0000-000000000090|2026-10-05',1,
      jsonb_build_object('task_id','61000000-0000-0000-0000-000000000090','work_date','2026-10-05','estimate_minutes',90,'created_at',1791158400000,'updated_at',1791158520000,'version',2)
    );
    raise exception 'stale daily estimate was accepted';
  exception when others then if sqlerrm not like '%SYNC_CONFLICT%' then raise; end if; end;

  first_result := timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000154','task_daily_estimate.clear','task_daily_estimate',
    '61000000-0000-0000-0000-000000000090|2026-10-05',2,
    jsonb_build_object('task_id','61000000-0000-0000-0000-000000000090','work_date','2026-10-05','deleted',true,'updated_at',1791158580000,'version',3)
  );
  retry_result := timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000154','task_daily_estimate.clear','task_daily_estimate',
    '61000000-0000-0000-0000-000000000090|2026-10-05',2,
    jsonb_build_object('task_id','61000000-0000-0000-0000-000000000090','work_date','2026-10-05','deleted',true,'updated_at',1791158580000,'version',3)
  );
  reset role;
  if first_result is distinct from retry_result or exists(select 1 from timegenie.task_daily_estimates where task_id='61000000-0000-0000-0000-000000000090' and work_date='2026-10-05') then
    raise exception 'daily estimate clear was not idempotent';
  end if;
  if not exists(
    select 1 from timegenie.workspace_changes
    where workspace_id='20000000-0000-0000-0000-000000000001'
      and entity_type='task_daily_estimates'
      and entity_id='61000000-0000-0000-0000-000000000090|2026-10-05'
      and operation='delete'
  ) then
    raise exception 'daily estimate clear did not publish its composite delete identity';
  end if;
  set role authenticated;
end $$;

-- Recurrence changes are one task-scoped atomic operation: rule history and the
-- task version advance together, while stale devices leave no partial rows.
do $$
declare before_rules jsonb; first_result jsonb; retry_result jsonb;
begin
  first_result := timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000160','task_recurrence.save','task_recurrence_rule',
    '61000000-0000-0000-0000-000000000090',null,
    jsonb_build_object('action','save','task_id','61000000-0000-0000-0000-000000000090','task_expected_version',1,'task_version',2,'rule_id','91000000-0000-0000-0000-000000000090','frequency','daily','weekdays_mask',null,'effective_start','2026-10-05','version',1,'rules',jsonb_build_array(jsonb_build_object('id','91000000-0000-0000-0000-000000000090','task_id','61000000-0000-0000-0000-000000000090','frequency','daily','weekdays_mask',null,'effective_start','2026-10-05','effective_end',null,'created_at',1791158400000,'updated_at',1791158400000,'version',1)))
  );
  retry_result := timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000160','task_recurrence.save','task_recurrence_rule',
    '61000000-0000-0000-0000-000000000090',null,
    jsonb_build_object('action','save','task_id','61000000-0000-0000-0000-000000000090','task_expected_version',1,'task_version',2,'rule_id','91000000-0000-0000-0000-000000000090','frequency','daily','weekdays_mask',null,'effective_start','2026-10-05','version',1,'rules',jsonb_build_array(jsonb_build_object('id','91000000-0000-0000-0000-000000000090','task_id','61000000-0000-0000-0000-000000000090','frequency','daily','weekdays_mask',null,'effective_start','2026-10-05','effective_end',null,'created_at',1791158400000,'updated_at',1791158400000,'version',1)))
  );
  if first_result is distinct from retry_result then raise exception 'recurrence retry changed result'; end if;

  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000161','task_recurrence.save','task_recurrence_rule',
    '61000000-0000-0000-0000-000000000090',1,
    jsonb_build_object('action','save','task_id','61000000-0000-0000-0000-000000000090','task_expected_version',2,'task_version',3,'rule_expected_version',1,'rule_id','91000000-0000-0000-0000-000000000090','frequency','weekdays','weekdays_mask',null,'effective_start','2026-10-05','version',2,'rules',jsonb_build_array(jsonb_build_object('id','91000000-0000-0000-0000-000000000090','task_id','61000000-0000-0000-0000-000000000090','frequency','weekdays','weekdays_mask',null,'effective_start','2026-10-05','effective_end',null,'created_at',1791158400000,'updated_at',1791158460000,'version',2)))
  );
  reset role;
  before_rules := (select jsonb_agg(to_jsonb(r) order by effective_start,id) from timegenie.task_recurrence_rules r where task_id='61000000-0000-0000-0000-000000000090');
  set role authenticated;
  begin
    perform timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002',
      '40000000-0000-0000-0000-000000000162','task_recurrence.save','task_recurrence_rule',
      '61000000-0000-0000-0000-000000000090',1,
      jsonb_build_object('action','save','task_id','61000000-0000-0000-0000-000000000090','task_expected_version',2,'task_version',3,'rule_expected_version',1,'rule_id','91000000-0000-0000-0000-000000000091','frequency','weekly','weekdays_mask',5,'effective_start','2026-10-06','version',1,'rules',jsonb_build_array())
    );
    raise exception 'stale recurrence update was accepted';
  exception when others then if sqlerrm not like '%SYNC_CONFLICT%' then raise; end if; end;
  reset role;
  if before_rules is distinct from (select jsonb_agg(to_jsonb(r) order by effective_start,id) from timegenie.task_recurrence_rules r where task_id='61000000-0000-0000-0000-000000000090') then
    raise exception 'stale recurrence changed rule rows';
  end if;
  set role authenticated;

  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000163','task_recurrence.close','task_recurrence_rule',
    '61000000-0000-0000-0000-000000000090',2,
    jsonb_build_object('action','close','task_id','61000000-0000-0000-0000-000000000090','task_expected_version',3,'task_version',4,'rule_expected_version',2,'rule_id','91000000-0000-0000-0000-000000000090','effective_end','2026-10-05','version',3,'rules',jsonb_build_array(jsonb_build_object('id','91000000-0000-0000-0000-000000000090','task_id','61000000-0000-0000-0000-000000000090','frequency','weekdays','weekdays_mask',null,'effective_start','2026-10-05','effective_end','2026-10-05','created_at',1791158400000,'updated_at',1791158520000,'version',3)))
  );
  reset role;
  if exists(select 1 from timegenie.task_recurrence_rules where task_id='61000000-0000-0000-0000-000000000090' and effective_end is null)
     or (select version from timegenie.tasks where id='61000000-0000-0000-0000-000000000090') <> 4 then
    raise exception 'recurrence close did not atomically update task and rule';
  end if;
  set role authenticated;
end $$;

-- Cross-day replacements create a new rule id while preserving the old
-- interval. Closing a rule before its start removes it, and both update/delete
-- changes stay keyed by task id for incremental refresh.
reset role;
insert into timegenie.tasks(id, workspace_id, subject_id, title, status, source_type, sort_order, version) values
  ('61000000-0000-0000-0000-000000000091','20000000-0000-0000-0000-000000000001','51000000-0000-0000-0000-000000000090','跨日规则','open','manual',91,1),
  ('61000000-0000-0000-0000-000000000092','20000000-0000-0000-0000-000000000001','51000000-0000-0000-0000-000000000090','未来规则','open','manual',92,1);
set role authenticated;
do $$
begin
  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000164','task_recurrence.save','task_recurrence_rule',
    '61000000-0000-0000-0000-000000000091',null,
    jsonb_build_object('action','save','task_id','61000000-0000-0000-0000-000000000091','task_expected_version',1,'task_version',2,'rule_id','91000000-0000-0000-0000-000000000091','frequency','daily','weekdays_mask',null,'effective_start','2026-10-05','version',1,'rules',jsonb_build_array(jsonb_build_object('id','91000000-0000-0000-0000-000000000091','task_id','61000000-0000-0000-0000-000000000091','frequency','daily','weekdays_mask',null,'effective_start','2026-10-05','effective_end',null,'created_at',1791158400000,'updated_at',1791158400000,'version',1)))
  );
  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000165','task_recurrence.save','task_recurrence_rule',
    '61000000-0000-0000-0000-000000000091',1,
    jsonb_build_object('action','save','task_id','61000000-0000-0000-0000-000000000091','task_expected_version',2,'task_version',3,'rule_expected_version',1,'rule_id','91000000-0000-0000-0000-000000000092','frequency','weekly','weekdays_mask',21,'effective_start','2026-10-06','version',1,'rules',jsonb_build_array(
      jsonb_build_object('id','91000000-0000-0000-0000-000000000091','task_id','61000000-0000-0000-0000-000000000091','frequency','daily','weekdays_mask',null,'effective_start','2026-10-05','effective_end','2026-10-05','created_at',1791158400000,'updated_at',1791244800000,'version',2),
      jsonb_build_object('id','91000000-0000-0000-0000-000000000092','task_id','61000000-0000-0000-0000-000000000091','frequency','weekly','weekdays_mask',21,'effective_start','2026-10-06','effective_end',null,'created_at',1791244800000,'updated_at',1791244800000,'version',1)
    ))
  );

  reset role;
  if (select count(*) from timegenie.task_recurrence_rules where task_id='61000000-0000-0000-0000-000000000091') <> 2
     or (select effective_end from timegenie.task_recurrence_rules where id='91000000-0000-0000-0000-000000000091') <> '2026-10-05'::date
     or not exists(select 1 from timegenie.task_recurrence_rules where id='91000000-0000-0000-0000-000000000092' and frequency='weekly' and weekdays_mask=21 and effective_end is null) then
    raise exception 'cross-day recurrence replacement did not preserve history';
  end if;
  if not exists(
    select 1 from timegenie.workspace_changes
    where entity_type='task_recurrence_rules'
      and entity_id='61000000-0000-0000-0000-000000000091'
      and operation in ('insert','update')
  ) then
    raise exception 'recurrence replacement did not publish task-scoped changes';
  end if;
  set role authenticated;

  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000166','task_recurrence.save','task_recurrence_rule',
    '61000000-0000-0000-0000-000000000092',null,
    jsonb_build_object('action','save','task_id','61000000-0000-0000-0000-000000000092','task_expected_version',1,'task_version',2,'rule_id','91000000-0000-0000-0000-000000000093','frequency','weekdays','weekdays_mask',null,'effective_start','2026-10-10','version',1,'rules',jsonb_build_array(jsonb_build_object('id','91000000-0000-0000-0000-000000000093','task_id','61000000-0000-0000-0000-000000000092','frequency','weekdays','weekdays_mask',null,'effective_start','2026-10-10','effective_end',null,'created_at',1791590400000,'updated_at',1791590400000,'version',1)))
  );
  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000167','task_recurrence.close','task_recurrence_rule',
    '61000000-0000-0000-0000-000000000092',1,
    jsonb_build_object('action','close','task_id','61000000-0000-0000-0000-000000000092','task_expected_version',2,'task_version',3,'rule_expected_version',1,'rule_id','91000000-0000-0000-0000-000000000093','effective_end','2026-10-09','version',2,'rules',jsonb_build_array())
  );
  reset role;
  if exists(select 1 from timegenie.task_recurrence_rules where id='91000000-0000-0000-0000-000000000093') then
    raise exception 'closing a future recurrence did not delete the rule';
  end if;
  if not exists(
    select 1 from timegenie.workspace_changes
    where entity_type='task_recurrence_rules'
      and entity_id='61000000-0000-0000-0000-000000000092'
      and operation='delete'
  ) then
    raise exception 'future recurrence delete did not publish its task identity';
  end if;
  set role authenticated;
end $$;

-- Coalesced timer snapshots may jump only across the exact number of local
-- operations declared by the client. The final image is applied once and the
-- original operation id remains the sole idempotency boundary.
reset role;
update timegenie.tracking_leases set expires_at=clock_timestamp()-interval '1 second'
where workspace_id='20000000-0000-0000-0000-000000000001';
set local role authenticated;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
do $$
declare timer_payload jsonb; first_result jsonb; retry_result jsonb; timer_change_count bigint;
declare sync_context jsonb; rejected jsonb;
begin
  -- Device 2 holds the background lease; device 1 timer writes must still be accepted.
  perform timegenie.tracking_lease_acquire(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000002',
    '49000000-0000-0000-0000-000000000001'
  );
  timer_payload := jsonb_build_object(
    'id', '70000000-0000-0000-0000-000000000099',
    'work_date', '2026-09-30',
    'kind', 'work',
    'source_type', 'timer',
    'state', 'ended',
    'default_task_id', '60000000-0000-0000-0000-000000000002',
    'label_snapshot', '运维相关 / 合并计时验证',
    'started_at', 1790730000000,
    'ended_at', 1790731800000,
    'duration_seconds', 1800,
    'note', '开始、暂停并结束后的最终镜像',
    'created_at', 1790730000000,
    'updated_at', 1790731800000,
    'version', 3,
    'segments', jsonb_build_array(jsonb_build_object(
      'id', '73000000-0000-0000-0000-000000000099',
      'sequence_no', 1,
      'started_at', 1790730000000,
      'ended_at', 1790731800000,
      'duration_seconds', 1800
    )),
    'allocations', jsonb_build_array()
  );
  select count(*) into timer_change_count
  from timegenie.workspace_changes
  where workspace_id = '20000000-0000-0000-0000-000000000001'
    and entity_type = 'time_entries'
    and entity_id = '70000000-0000-0000-0000-000000000099';
  first_result := timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000090',
    'timer_stop',
    'time_entry',
    '70000000-0000-0000-0000-000000000099',
    null,
    3,
    3,
    timer_payload
  );
  retry_result := timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000090',
    'timer_stop',
    'time_entry',
    '70000000-0000-0000-0000-000000000099',
    null,
    3,
    3,
    timer_payload
  );
  if first_result is distinct from retry_result then
    raise exception 'coalesced timer retry returned a different result';
  end if;
  sync_context:=timegenie.timer_sync_context_get(
    '20000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000090',
    '70000000-0000-0000-0000-000000000099',
    'timer_stop',
    jsonb_build_object(
      'deviceId','30000000-0000-0000-0000-000000000001',
      'entityType','time_entry',
      'entityId','70000000-0000-0000-0000-000000000099',
      'baseVersion',null,
      'payloadVersion',3,
      'coalescedCount',3,
      'payload',timer_payload
    )
  );
  if sync_context->'processedOperation'->>'operationType'<>'timer_stop'
     or not coalesce((sync_context->'processedOperation'->>'matchesRequest')::boolean,false)
     or (sync_context->>'currentVersion')::bigint<>3 then
    raise exception 'timer sync context did not confirm the processed operation: %',sync_context;
  end if;
  sync_context:=timegenie.timer_sync_context_get(
    '20000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000090',
    '70000000-0000-0000-0000-000000000099',
    'timer_stop',
    jsonb_build_object(
      'deviceId','30000000-0000-0000-0000-000000000001',
      'entityType','time_entry',
      'entityId','70000000-0000-0000-0000-000000000099',
      'baseVersion',null,
      'payloadVersion',3,
      'coalescedCount',3,
      'payload',jsonb_set(timer_payload,'{note}','"different request"'::jsonb,true)
    )
  );
  if coalesce((sync_context->'processedOperation'->>'matchesRequest')::boolean,true) then
    raise exception 'timer sync context accepted different content for an existing operation id';
  end if;
  if (select version from timegenie.time_entries where id = '70000000-0000-0000-0000-000000000099') <> 3 then
    raise exception 'coalesced timer final image was not applied at version 3';
  end if;
  if (
    select count(*) from timegenie.workspace_changes
    where workspace_id = '20000000-0000-0000-0000-000000000001'
      and entity_type = 'time_entries'
      and entity_id = '70000000-0000-0000-0000-000000000099'
  ) <> timer_change_count + 1 then
    raise exception 'coalesced timer retry created duplicate cloud writes: before %, after %, first %, retry %',
      timer_change_count,
      (select count(*) from timegenie.workspace_changes where workspace_id='20000000-0000-0000-0000-000000000001' and entity_type='time_entries' and entity_id='70000000-0000-0000-0000-000000000099'),
      first_result,retry_result;
  end if;

  begin
    perform timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000001',
      '40000000-0000-0000-0000-000000000094',
      'time_allocation_replace',
      'time_entry',
      '70000000-0000-0000-0000-000000000099',
      3,
      4,
      null,
      jsonb_set(timer_payload, '{version}', '4'::jsonb, true)
    );
    raise exception 'coalesced timer request without chain proof was accepted';
  exception
    when others then
      if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if;
  end;

  begin
    perform timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000001',
      '40000000-0000-0000-0000-000000000091',
      'time_allocation_replace',
      'time_entry',
      '70000000-0000-0000-0000-000000000099',
      3,
      4,
      2,
      jsonb_set(timer_payload, '{version}', '4'::jsonb, true)
    );
    raise exception 'mismatched coalesced timer span was accepted';
  exception
    when others then
      if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if;
  end;

  rejected:=timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000002',
      '40000000-0000-0000-0000-000000000092',
      'timer_stop',
      'time_entry',
      '70000000-0000-0000-0000-000000000099',
      2,
      4,
      2,
      jsonb_set(timer_payload, '{version}', '4'::jsonb, true)
    );
  if coalesce((rejected->>'accepted')::boolean,true)
     or rejected->'error'->>'code'<>'SYNC_CONFLICT'
     or (rejected->>'currentVersion')::bigint<>3
     or rejected->'authoritative'->>'id'<>'70000000-0000-0000-0000-000000000099' then
    raise exception 'competing timer write did not return the authoritative first-arrival result: %',rejected;
  end if;

  begin
    perform timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000001',
      '40000000-0000-0000-0000-000000000093',
      'subject.create',
      'subject',
      '61000000-0000-0000-0000-000000000099',
      null,
      2,
      2,
      jsonb_build_object(
        'id', '61000000-0000-0000-0000-000000000099',
        'name', '普通实体不可跳版本',
        'sort_order', 99,
        'created_at', 1790730000000,
        'updated_at', 1790730000000,
        'version', 2
      )
    );
    raise exception 'non-timer entity used coalesced proof to jump versions';
  exception
    when others then
      if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if;
  end;

  if not timegenie.tracking_lease_release(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000002',
    '49000000-0000-0000-0000-000000000001'
  ) then
    raise exception 'non-holder timer write test failed to release its fixture lease';
  end if;
end $$;

-- Lease acquire/renew/release and unassigned ticks must never mutate an
-- ordinary timer. The holder can heartbeat repeatedly without changing the
-- timer image, while a live lease still blocks another device.
reset role;
do $$
declare timer_id uuid:='70000000-0000-0000-0000-000000000098'; timer_version bigint;
declare timer_updated timestamptz; segment_image jsonb; lease_result jsonb;
begin
  delete from timegenie.time_segments where entry_id=timer_id;
  delete from timegenie.time_entries where id=timer_id;
  insert into timegenie.time_entries(
    id,workspace_id,work_date,kind,source_type,state,default_task_id,label_snapshot,
    started_at,duration_seconds,created_at,updated_at,version,created_by_device_id,
    updated_by_device_id,timer_chain_id,last_continuous_at
  ) values(
    timer_id,'20000000-0000-0000-0000-000000000001',timegenie.workspace_work_date(
      '20000000-0000-0000-0000-000000000001',clock_timestamp()
    ),'work','timer','running','60000000-0000-0000-0000-000000000002',
    '租约心跳不修改计时',clock_timestamp()-interval '10 minutes',0,clock_timestamp()-interval '10 minutes',
    clock_timestamp()-interval '2 minutes',7,'30000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',timer_id,clock_timestamp()-interval '2 minutes'
  );
  insert into timegenie.time_segments(
    id,workspace_id,entry_id,sequence_no,started_at,duration_seconds
  ) values(
    '73000000-0000-0000-0000-000000000098',
    '20000000-0000-0000-0000-000000000001',timer_id,1,clock_timestamp()-interval '10 minutes',0
  );
  select version,updated_at into timer_version,timer_updated from timegenie.time_entries where id=timer_id;
  select jsonb_agg(to_jsonb(segment) order by sequence_no) into segment_image
  from timegenie.time_segments segment where entry_id=timer_id;
  perform set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
  set local role authenticated;
  lease_result:=timegenie.tracking_lease_acquire(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '49000000-0000-0000-0000-000000000098'
  );
  if not coalesce((lease_result->>'acquired')::boolean,false) then
    raise exception 'timer holder failed to acquire lease for heartbeat isolation test';
  end if;
  perform timegenie.tracking_lease_renew(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '49000000-0000-0000-0000-000000000098'
  );
  perform timegenie.unassigned_tick(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '49000000-0000-0000-0000-000000000098'
  );
  reset role;
  if not exists(
    select 1 from timegenie.time_entries where id=timer_id
      and version=timer_version and updated_at=timer_updated and state='running'
  ) or (
    select jsonb_agg(to_jsonb(segment) order by sequence_no)
    from timegenie.time_segments segment where entry_id=timer_id
  ) is distinct from segment_image then
    raise exception 'lease heartbeat mutated the ordinary timer image';
  end if;
  set local role authenticated;
  if coalesce((timegenie.tracking_lease_acquire(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000002',
    '49000000-0000-0000-0000-000000000099'
  )->>'acquired')::boolean,false) then
    raise exception 'live timer lease was stolen by another device';
  end if;
  perform timegenie.tracking_lease_release(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '49000000-0000-0000-0000-000000000098'
  );
  reset role;
  if not exists(select 1 from timegenie.time_entries where id=timer_id and version=timer_version) then
    raise exception 'lease release mutated the ordinary timer version';
  end if;
  delete from timegenie.time_segments where entry_id=timer_id;
  delete from timegenie.time_entries where id=timer_id;
end $$;

-- Only one global timer may exist. Either registered device can control the
-- shared timer when it supplies the current optimistic version.
do $$
declare timer_row jsonb;
begin
  timer_row := timegenie.timer_start(
    '20000000-0000-0000-0000-000000000001',
    '60000000-0000-0000-0000-000000000002',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000020',
    '设备 A 启动'
  );

  begin
    perform timegenie.timer_start(
      '20000000-0000-0000-0000-000000000001',
      '60000000-0000-0000-0000-000000000002',
      '30000000-0000-0000-0000-000000000002',
      '40000000-0000-0000-0000-000000000021',
      '设备 B 重复启动'
    );
    raise exception 'parallel timer was accepted';
  exception
    when others then
      if sqlerrm not like '%REMOTE_TIMER_ACTIVE%' then raise; end if;
  end;

  begin
    perform timegenie.timer_pause(
      '20000000-0000-0000-0000-000000000001',
      (timer_row->>'id')::uuid,
      '30000000-0000-0000-0000-000000000002',
      99,
      '40000000-0000-0000-0000-000000000022'
    );
    raise exception 'stale timer action was accepted';
  exception
    when others then
      if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if;
  end;

  timer_row := timegenie.timer_pause(
    '20000000-0000-0000-0000-000000000001',
    (timer_row->>'id')::uuid,
    '30000000-0000-0000-0000-000000000002',
    (timer_row->>'version')::bigint,
    '40000000-0000-0000-0000-000000000023'
  );
  timer_row := timegenie.timer_resume(
    '20000000-0000-0000-0000-000000000001',
    (timer_row->>'id')::uuid,
    '30000000-0000-0000-0000-000000000002',
    (timer_row->>'version')::bigint,
    '40000000-0000-0000-0000-000000000024'
  );
  timer_row := timegenie.timer_stop(
    '20000000-0000-0000-0000-000000000001',
    (timer_row->>'id')::uuid,
    '30000000-0000-0000-0000-000000000001',
    (timer_row->>'version')::bigint,
    '40000000-0000-0000-0000-000000000025'
  );

  if timer_row->>'state' <> 'ended' then
    raise exception 'global timer did not stop';
  end if;
end $$;

-- The background collector has one lease holder and one open segment. A
-- takeover after expiry closes the old segment and opens exactly one new one.
do $$
declare lease_result jsonb;
begin
  lease_result := timegenie.tracking_lease_acquire(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '90000000-0000-0000-0000-000000000001'
  );
  if not (lease_result->>'acquired')::boolean then raise exception 'device A did not acquire lease'; end if;

  lease_result := timegenie.tracking_lease_acquire(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000002',
    '90000000-0000-0000-0000-000000000002'
  );
  if (lease_result->>'acquired')::boolean then raise exception 'device B stole a live lease'; end if;

  begin
    perform timegenie.tracking_lease_renew(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000001',
      '90000000-0000-0000-0000-000000000099'
    );
    raise exception 'wrong lease token was renewed';
  exception
    when others then
      if sqlerrm not like '%LEASE_EXPIRED%' then raise; end if;
  end;

end $$;

reset role;
update timegenie.tracking_leases set expires_at = now() - interval '1 second'
where workspace_id = '20000000-0000-0000-0000-000000000001';

set local role authenticated;
select set_config('request.jwt.claim.sub', '10000000-0000-0000-0000-000000000001', true);

do $$
declare lease_result jsonb;
begin
  lease_result := timegenie.tracking_lease_acquire(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000002',
    '90000000-0000-0000-0000-000000000002'
  );
  if not (lease_result->>'acquired')::boolean then raise exception 'device B did not acquire expired lease'; end if;

  begin
    perform timegenie.unassigned_tick(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000001',
      '90000000-0000-0000-0000-000000000001'
    );
    raise exception 'expired holder advanced unassigned time';
  exception
    when others then
      if sqlerrm not like '%LEASE_EXPIRED%' then raise; end if;
  end;

  perform timegenie.unassigned_tick(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000002',
    '90000000-0000-0000-0000-000000000002'
  );
end $$;

reset role;

-- Calendar-day helpers use the workspace IANA timezone, including DST days.
do $$
begin
  update timegenie.workspaces set timezone='America/New_York'
  where id='20000000-0000-0000-0000-000000000001';
  if timegenie.workspace_work_date('20000000-0000-0000-0000-000000000001','2026-03-08 04:30:00+00') <> date '2026-03-07' then
    raise exception 'workspace date ignored America/New_York timezone';
  end if;
  if extract(epoch from (
    timegenie.workspace_day_end('20000000-0000-0000-0000-000000000001',date '2026-03-08') -
    timegenie.workspace_day_start('20000000-0000-0000-0000-000000000001',date '2026-03-08')
  ))::bigint <> 82800 then
    raise exception 'DST spring day was treated as fixed 24 hours';
  end if;
  update timegenie.workspaces set timezone='Asia/Shanghai'
  where id='20000000-0000-0000-0000-000000000001';
end $$;

-- The same timer chain/date candidate converges to one authoritative row.
reset role;
update timegenie.tracking_leases set expires_at=clock_timestamp()-interval '1 second'
where workspace_id='20000000-0000-0000-0000-000000000001';
set local role authenticated;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
do $$
declare first_result jsonb; second_result jsonb; chain_id uuid:='74000000-0000-0000-0000-000000000001';
begin
  perform timegenie.tracking_lease_acquire(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '49000000-0000-0000-0000-000000000097'
  );
  first_result:=timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '44000000-0000-0000-0000-000000000001','timer_rollover_create','time_entry',
    '74100000-0000-0000-0000-000000000001',null,
    jsonb_build_object('id','74100000-0000-0000-0000-000000000001','work_date','2026-10-06','kind','work','source_type','timer','state','ended',
      'label_snapshot','日切测试','started_at',1791216000000,'ended_at',1791216060000,'duration_seconds',60,'created_at',1791216000000,
      'updated_at',1791216060000,'version',1,'timer_chain_id',chain_id::text,'last_continuous_at',1791216060000,'segments','[]'::jsonb,'allocations','[]'::jsonb)
  );
  second_result:=timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '44000000-0000-0000-0000-000000000002','timer_rollover_create','time_entry',
    '74100000-0000-0000-0000-000000000002',null,
    jsonb_build_object('id','74100000-0000-0000-0000-000000000002','work_date','2026-10-06','kind','work','source_type','timer','state','ended',
      'label_snapshot','竞争候选','started_at',1791216000000,'ended_at',1791216060000,'duration_seconds',60,'created_at',1791216000000,
      'updated_at',1791216060000,'version',1,'timer_chain_id',chain_id::text,'last_continuous_at',1791216060000,'segments','[]'::jsonb,'allocations','[]'::jsonb)
  );
  if not coalesce((second_result->>'superseded')::boolean,false)
     or second_result->'authoritative'->>'id'<>'74100000-0000-0000-0000-000000000001' then
    raise exception 'timer chain/date candidates did not converge to the first authoritative row: first %, second %',first_result,second_result;
  end if;
  if (select count(*) from timegenie.time_entries where timer_chain_id=chain_id and work_date='2026-10-06')<>1 then
    raise exception 'timer chain/date uniqueness was not preserved';
  end if;
end $$;
reset role;

-- A stale session date must never close a later-started segment before its start.
reset role;
delete from timegenie.unassigned_segments
where session_id in (select id from timegenie.unassigned_sessions where workspace_id='20000000-0000-0000-0000-000000000001' and state in ('collecting','awaiting_resolution'));
delete from timegenie.unassigned_sessions
where workspace_id='20000000-0000-0000-0000-000000000001' and state in ('collecting','awaiting_resolution');
insert into timegenie.unassigned_sessions(
  id,workspace_id,work_date,state,threshold_seconds,duration_seconds,first_started_at,shared_source,migration_state,created_at,updated_at,version,last_continuous_at
) values(
  '95200000-0000-0000-0000-000000000001','20000000-0000-0000-0000-000000000001','2026-10-04','collecting',300,0,
  '2026-10-06 14:25:42+00','cloud','adopted','2026-10-06 14:25:42+00','2026-10-06 14:25:42+00',1,'2026-10-06 14:25:42+00'
);
insert into timegenie.unassigned_segments(id,workspace_id,session_id,sequence_no,started_at,duration_seconds,lease_token)
values('95210000-0000-0000-0000-000000000001','20000000-0000-0000-0000-000000000001','95200000-0000-0000-0000-000000000001',1,'2026-10-06 14:25:42+00',0,'50000000-0000-0000-0000-000000000001');
update timegenie.tracking_leases set holder_device_id='30000000-0000-0000-0000-000000000001',
  lease_token='50000000-0000-0000-0000-000000000001',expires_at='2026-10-07 01:00:00+00',last_confirmed_at='2026-10-07 00:00:00+00'
where workspace_id='20000000-0000-0000-0000-000000000001';
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
select timegenie.coordinate_unassigned_calendar_day_at(
  '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
  '50000000-0000-0000-0000-000000000001','2026-10-07 00:00:00+00'
);
do $$
begin
  if exists(select 1 from timegenie.unassigned_segments where id='95210000-0000-0000-0000-000000000001' and ended_at<started_at) then
    raise exception 'calendar rollover closed an unassigned segment before it started';
  end if;
  if not exists(select 1 from timegenie.unassigned_segments where id='95210000-0000-0000-0000-000000000001' and ended_at=started_at and duration_seconds=0) then
    raise exception 'stale-date unassigned segment was not closed safely at zero duration';
  end if;
  if not exists(select 1 from timegenie.unassigned_sessions where id='95200000-0000-0000-0000-000000000001' and state='discarded' and resolution_type='discard' and resolved_at is not null) then
    raise exception 'zero-duration stale session was not finalized with terminal metadata';
  end if;
end $$;

-- Lease expiry, takeover, and release must not mutate the shared unassigned
-- anchor. Only ordinary item timers use lease continuity boundaries.
set local role authenticated;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
do $$
declare first_lease jsonb; second_lease jsonb; anchor_after_takeover jsonb; anchor_after_release jsonb;
declare original_started timestamptz:=clock_timestamp()-interval '120 seconds'; original_version bigint; generated_count bigint;
begin
  first_lease:=timegenie.tracking_lease_acquire('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001','94000000-0000-0000-0000-000000000001');
  reset role;
  delete from timegenie.time_segments where entry_id in (select id from timegenie.time_entries where workspace_id='20000000-0000-0000-0000-000000000001' and state in ('running','paused'));
  delete from timegenie.time_entries where workspace_id='20000000-0000-0000-0000-000000000001' and state in ('running','paused');
  delete from timegenie.unassigned_segments where session_id in (select id from timegenie.unassigned_sessions where workspace_id='20000000-0000-0000-0000-000000000001' and state in ('collecting','awaiting_resolution'));
  delete from timegenie.unassigned_sessions where workspace_id='20000000-0000-0000-0000-000000000001' and state in ('collecting','awaiting_resolution');
  insert into timegenie.unassigned_sessions(id,workspace_id,work_date,state,threshold_seconds,duration_seconds,first_started_at,shared_source,migration_state,created_at,updated_at,version,last_continuous_at)
  values('95000000-0000-0000-0000-000000000001','20000000-0000-0000-0000-000000000001',timegenie.workspace_work_date('20000000-0000-0000-0000-000000000001',clock_timestamp()),
    'collecting',300,0,original_started,'cloud','adopted',original_started,clock_timestamp(),1,original_started)
  on conflict do nothing;
  insert into timegenie.unassigned_segments(id,workspace_id,session_id,sequence_no,started_at,duration_seconds,lease_token)
  values('95100000-0000-0000-0000-000000000001','20000000-0000-0000-0000-000000000001','95000000-0000-0000-0000-000000000001',1,original_started,0,'94000000-0000-0000-0000-000000000001')
  on conflict do nothing;
  update timegenie.unassigned_segments set started_at=original_started
  where lease_token='94000000-0000-0000-0000-000000000001' and ended_at is null;
  select version into original_version from timegenie.unassigned_sessions
  where id='95000000-0000-0000-0000-000000000001';
  update timegenie.tracking_leases set last_confirmed_at=clock_timestamp()-interval '90 seconds',expires_at=clock_timestamp()-interval '45 seconds'
  where workspace_id='20000000-0000-0000-0000-000000000001';
  set local role authenticated;
  perform set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
  second_lease:=timegenie.tracking_lease_acquire('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002','94000000-0000-0000-0000-000000000002');
  reset role;
  if not coalesce((second_lease->>'acquired')::boolean,false) then
    raise exception 'replacement device did not acquire the expired lease';
  end if;
  anchor_after_takeover:=timegenie.timegenie_unassigned_active_anchor('20000000-0000-0000-0000-000000000001',null);
  if anchor_after_takeover is null
     or anchor_after_takeover->>'id'<>'95000000-0000-0000-0000-000000000001'
     or (anchor_after_takeover->>'first_started_at')::timestamptz<>original_started
     or (anchor_after_takeover->>'version')::bigint<>original_version
     or jsonb_array_length(anchor_after_takeover->'segments')<>1
     or anchor_after_takeover->'segments'->0->>'ended_at' is not null
     or (anchor_after_takeover->'segments'->0->>'started_at')::timestamptz<>original_started then
    raise exception 'lease takeover mutated the shared unassigned anchor: %',anchor_after_takeover;
  end if;
  set local role authenticated;
  perform set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
  if not timegenie.tracking_lease_release('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002','94000000-0000-0000-0000-000000000002') then
    raise exception 'replacement device did not release its lease';
  end if;
  reset role;
  anchor_after_release:=timegenie.timegenie_unassigned_active_anchor('20000000-0000-0000-0000-000000000001',null);
  if anchor_after_release is null
     or anchor_after_release->>'id'<>anchor_after_takeover->>'id'
     or (anchor_after_release->>'first_started_at')::timestamptz<>original_started
     or (anchor_after_release->>'version')::bigint<>original_version
     or anchor_after_release->'segments'->0->>'ended_at' is not null then
    raise exception 'lease release mutated the shared unassigned anchor: %',anchor_after_release;
  end if;
  select count(*) into generated_count from timegenie.time_entries
  where origin_unassigned_session_id='95000000-0000-0000-0000-000000000001';
  if generated_count<>0 then
    raise exception 'lease lifecycle generated an unassigned result';
  end if;
end $$;
reset role;

-- Shared unassigned time has one root per workspace. Repeated create calls
-- adopt the same root, only the first concurrent resolution wins, and retrying
-- the winning operation is idempotent.
reset role;
insert into timegenie.subjects(id,workspace_id,name,sort_order)
values('52000000-0000-0000-0000-000000000001','20000000-0000-0000-0000-000000000002','未归属处理',1)
on conflict do nothing;
insert into timegenie.tasks(id,workspace_id,subject_id,title,status,source_type,sort_order,version)
values('62000000-0000-0000-0000-000000000001','20000000-0000-0000-0000-000000000002','52000000-0000-0000-0000-000000000001','未归属处理事项','open','manual',1,1)
on conflict do nothing;
set local role authenticated;
select set_config('request.jwt.claim.sub', '10000000-0000-0000-0000-000000000002', true);

do $$
declare first_session jsonb; second_session jsonb; next_session jsonb; first_result jsonb; retry_result jsonb; loser_result jsonb;
declare reset_result jsonb; reset_retry jsonb; reset_loser jsonb; reset_millis bigint;
declare session_id uuid; session_version bigint; original_started timestamptz; original_updated timestamptz;
begin
  first_session := timegenie.unassigned_get_or_create_shared(
    '20000000-0000-0000-0000-000000000002',
    '30000000-0000-0000-0000-000000000003',
    jsonb_build_object(
      'id','72000000-0000-0000-0000-000000000001',
      'state','collecting','threshold_seconds',300,'duration_seconds',0,
      'first_started_at',1791250000000
    ), null, null
  );
  second_session := timegenie.unassigned_get_or_create_shared(
    '20000000-0000-0000-0000-000000000002',
    '30000000-0000-0000-0000-000000000003',
    jsonb_build_object(
      'id','72000000-0000-0000-0000-000000000002',
      'state','collecting','threshold_seconds',300,'duration_seconds',999,
      'first_started_at',1791250999000
    ), null, null
  );
  if first_session->>'id' is distinct from second_session->>'id' then
    raise exception 'parallel shared unassigned create returned different roots';
  end if;
  if first_session->>'state'<>'collecting'
     or (first_session->>'duration_seconds')::bigint<>0
     or jsonb_array_length(first_session->'segments')<>1
     or first_session->'segments'->0->>'ended_at' is not null
     or second_session->>'first_started_at' is distinct from first_session->>'first_started_at' then
    raise exception 'shared create did not preserve one immutable zero-duration anchor: %, %',first_session,second_session;
  end if;
  session_id := (first_session->>'id')::uuid;
  session_version := (first_session->>'version')::bigint;
  original_started := (first_session->>'first_started_at')::timestamptz;
  original_updated := (first_session->>'updated_at')::timestamptz;
  retry_result := timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000002',
    '30000000-0000-0000-0000-000000000003',
    '42000000-0000-0000-0000-000000000004',
    'unassigned_session_awaiting_resolution',
    'unassigned_session',
    session_id::text,
    session_version,
    jsonb_build_object(
      'id',session_id::text,'work_date','2026-10-06','state','awaiting_resolution',
      'threshold_seconds',300,'duration_seconds',300,'first_started_at',1791250000000,
      'created_at',1791250000000,'updated_at',1791250300000,'version',session_version+1,
      'segments','[]'::jsonb
    )
  );
  if not coalesce((retry_result->>'superseded')::boolean,false)
     or retry_result->'session'->>'state'<>'collecting'
     or (retry_result->'session'->>'version')::bigint<>session_version
     or (retry_result->'session'->>'duration_seconds')::bigint<>0
     or (retry_result->'session'->>'first_started_at')::timestamptz<>original_started then
    raise exception 'legacy awaiting-resolution update changed the simplified anchor: %',retry_result;
  end if;
  retry_result := timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000002',
    '30000000-0000-0000-0000-000000000003',
    '42000000-0000-0000-0000-000000000005',
    'unassigned_session_awaiting_resolution',
    'unassigned_session',
    session_id::text,
    session_version,
    jsonb_build_object(
      'id',session_id::text,'work_date','2026-10-06','state','awaiting_resolution',
      'threshold_seconds',300,'duration_seconds',300,'first_started_at',1791250000000,
      'created_at',1791250000000,'updated_at',1791250300001,'version',session_version+1,
      'segments','[]'::jsonb
    )
  );
  if not coalesce((retry_result->>'superseded')::boolean,false)
     or retry_result->'session'->>'state'<>'collecting'
     or (retry_result->'session'->>'version')::bigint<>session_version
     or (retry_result->'session'->>'updated_at')::timestamptz<>original_updated then
    raise exception 'duplicate legacy lifecycle update mutated the current anchor';
  end if;
  reset_millis:=floor(extract(epoch from clock_timestamp())*1000)::bigint-60000;
  reset_result:=timegenie.unassigned_anchor_reset(
    '20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000003',
    '42000000-0000-0000-0000-000000000006',session_id,session_version,reset_millis
  );
  reset_retry:=timegenie.unassigned_anchor_reset(
    '20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000003',
    '42000000-0000-0000-0000-000000000006',session_id,session_version,reset_millis
  );
  reset_loser:=timegenie.unassigned_anchor_reset(
    '20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000003',
    '42000000-0000-0000-0000-000000000007',session_id,session_version,reset_millis-60000
  );
  if not coalesce((reset_result->>'accepted')::boolean,false)
     or reset_retry is distinct from reset_result
     or coalesce((reset_loser->>'accepted')::boolean,false)
     or reset_loser->'session'->>'id'<>session_id::text
     or (reset_loser->'session'->>'first_started_at')::timestamptz<>(reset_result->'session'->>'first_started_at')::timestamptz then
    raise exception 'anchor reset retry or stale reset did not converge: %, %, %',reset_result,reset_retry,reset_loser;
  end if;
  session_version:=(reset_result->'session'->>'version')::bigint;
  first_result := timegenie.unassigned_resolve_shared(
    '20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000003',
    '42000000-0000-0000-0000-000000000001',session_id,session_version,'work',
    jsonb_build_object('generated_entry_id','73000000-0000-0000-0000-000000000001','generated_entry',jsonb_build_object(
      'id','73000000-0000-0000-0000-000000000001','origin_unassigned_session_id',session_id::text,
      'label_snapshot','未归属处理事项','note',null,'allocations',jsonb_build_array(jsonb_build_object(
        'id','73100000-0000-0000-0000-000000000001','task_id','62000000-0000-0000-0000-000000000001','minutes',1,'note',null
      ))
    ))
  );
  retry_result := timegenie.unassigned_resolve_shared(
    '20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000003',
    '42000000-0000-0000-0000-000000000001',session_id,session_version,'work',
    jsonb_build_object('generated_entry_id','73000000-0000-0000-0000-000000000001','generated_entry',jsonb_build_object(
      'id','73000000-0000-0000-0000-000000000001','origin_unassigned_session_id',session_id::text,
      'label_snapshot','未归属处理事项','note',null,'allocations',jsonb_build_array(jsonb_build_object(
        'id','73100000-0000-0000-0000-000000000001','task_id','62000000-0000-0000-0000-000000000001','minutes',1,'note',null
      ))
    ))
  );
  loser_result := timegenie.unassigned_resolve_shared(
    '20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000003',
    '42000000-0000-0000-0000-000000000002',session_id,session_version,'break','{}'::jsonb
  );
  if not (first_result->>'accepted')::boolean or retry_result <> first_result then
    raise exception 'shared unassigned resolution retry was not idempotent';
  end if;
  if (loser_result->>'accepted')::boolean or loser_result->'session'->>'resolution_type' <> 'work' then
    raise exception 'concurrent shared unassigned loser did not adopt winner';
  end if;
  if first_result->'session'->>'resolved_at' is null then
    raise exception 'shared unassigned resolution did not use server resolved_at';
  end if;
  if first_result->'generatedEntry'->>'id'<>'73000000-0000-0000-0000-000000000001'
     or first_result->'session'->>'generated_entry_id'<>'73000000-0000-0000-0000-000000000001'
     or (first_result->'generatedEntry'->>'started_at')::timestamptz<>(first_result->'session'->>'first_started_at')::timestamptz
     or (first_result->'generatedEntry'->>'ended_at')::timestamptz<>(first_result->'session'->>'resolved_at')::timestamptz
     or (first_result->'generatedEntry'->>'duration_seconds')::bigint<>(first_result->'session'->>'duration_seconds')::bigint then
    raise exception 'shared resolution did not atomically create the authoritative final entry: %',first_result;
  end if;
  next_session:=timegenie.unassigned_get_or_create_shared(
    '20000000-0000-0000-0000-000000000002',
    '30000000-0000-0000-0000-000000000003',null,null,null
  );
  if first_result->'nextSession' is null
     or first_result->'nextSession'->>'predecessor_session_id'<>session_id::text
     or (first_result->'nextSession'->>'first_started_at')::timestamptz
        <> (first_result->'session'->>'resolved_at')::timestamptz
     or next_session->>'id'<>first_result->'nextSession'->>'id' then
    raise exception 'resolution did not establish exactly one next anchor at the authoritative boundary: %',first_result;
  end if;
  retry_result := timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000002',
    '30000000-0000-0000-0000-000000000003',
    '42000000-0000-0000-0000-000000000003',
    'unassigned_session_awaiting_resolution',
    'unassigned_session',
    session_id::text,
    session_version,
    jsonb_build_object(
      'id',session_id::text,'work_date','2026-10-06','state','awaiting_resolution',
      'threshold_seconds',300,'duration_seconds',300,'first_started_at',1791250999000,
      'created_at',1791250999000,'updated_at',1791251299000,'version',session_version+1,
      'segments','[]'::jsonb
    )
  );
  if not coalesce((retry_result->>'superseded')::boolean,false)
     or retry_result->'session'->>'resolution_type' <> 'work' then
    raise exception 'superseded shared unassigned lifecycle did not adopt terminal session';
  end if;
end $$;

-- Timer start clears the current anchor in the same cloud operation; timer
-- stop creates exactly one new anchor at its authoritative ended_at boundary.
do $$
declare start_payload jsonb; stop_payload jsonb; first_stop jsonb; retry_stop jsonb; active_anchor jsonb; start_millis bigint; stop_millis bigint;
begin
  perform timegenie.tracking_lease_acquire(
    '20000000-0000-0000-0000-000000000002',
    '30000000-0000-0000-0000-000000000003',
    '49000000-0000-0000-0000-000000000096'
  );
  start_millis:=floor(extract(epoch from clock_timestamp())*1000)::bigint;
  stop_millis:=start_millis+60000;
  start_payload:=jsonb_build_object(
    'id','74000000-0000-0000-0000-000000000001','work_date',timegenie.workspace_work_date('20000000-0000-0000-0000-000000000002',to_timestamp(start_millis::double precision/1000)),
    'kind','work','source_type','timer','state','running','default_task_id','62000000-0000-0000-0000-000000000001',
    'label_snapshot','原子计时验证','started_at',start_millis,'ended_at',null,'duration_seconds',0,'note',null,
    'created_at',start_millis,'updated_at',start_millis,'version',1,
    'segments',jsonb_build_array(jsonb_build_object('id','74100000-0000-0000-0000-000000000001','sequence_no',1,'started_at',start_millis,'ended_at',null,'duration_seconds',0)),
    'allocations',jsonb_build_array()
  );
  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000003',
    '42000000-0000-0000-0000-000000000008','timer_start','time_entry','74000000-0000-0000-0000-000000000001',
    null,1,1,start_payload
  );
  if timegenie.unassigned_get_state('20000000-0000-0000-0000-000000000002') is not null then
    raise exception 'timer start did not atomically clear the active anchor';
  end if;
  stop_payload:=start_payload||jsonb_build_object(
    'state','ended','ended_at',stop_millis,'duration_seconds',60,'updated_at',stop_millis,'version',2,
    'segments',jsonb_build_array(jsonb_build_object('id','74100000-0000-0000-0000-000000000001','sequence_no',1,'started_at',start_millis,'ended_at',stop_millis,'duration_seconds',60))
  );
  first_stop:=timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000003',
    '42000000-0000-0000-0000-000000000009','timer_stop','time_entry','74000000-0000-0000-0000-000000000001',
    1,2,1,stop_payload
  );
  retry_stop:=timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000003',
    '42000000-0000-0000-0000-000000000009','timer_stop','time_entry','74000000-0000-0000-0000-000000000001',
    1,2,1,stop_payload
  );
  active_anchor:=timegenie.unassigned_get_state('20000000-0000-0000-0000-000000000002');
  if first_stop is distinct from retry_stop
     or active_anchor is null
     or (active_anchor->>'first_started_at')::timestamptz<>(
       select ended_at from timegenie.time_entries
       where id='74000000-0000-0000-0000-000000000001'
     )
     or active_anchor->>'session_id' is null then
    raise exception 'timer stop did not create one idempotent authoritative anchor: %',active_anchor;
  end if;
end $$;

-- Timer transitions create/switch the target and timer images in one remote
-- transaction. Retries reuse the exact result and conflicts leave no partial
-- task or timer behind.
do $$
declare start_millis bigint; switch_millis bigint; start_payload jsonb; invalid_occurrence_payload jsonb; switch_payload jsonb; stale_payload jsonb; first_result jsonb; retry_result jsonb;
begin
  start_millis:=floor(extract(epoch from clock_timestamp())*1000)::bigint;
  switch_millis:=start_millis+60000;
  start_payload:=jsonb_build_object(
    'previous_entry',null,'previous_base_version',null,'task',jsonb_build_object(
      'id','62100000-0000-0000-0000-000000000001','subject_id','52000000-0000-0000-0000-000000000001','parent_id',null,
      'title','原子转换一级事项','status','open','planned_date',timegenie.workspace_work_date('20000000-0000-0000-0000-000000000002',to_timestamp(start_millis::double precision/1000)),
      'planned_time',null,'estimate_minutes',null,'note',null,'project_name',null,'solution_name',null,'source_type','timer_quick_create',
      'source_ref',null,'sort_order',10,'completed_at',null,'created_at',start_millis,'updated_at',start_millis,'version',1,'deleted_at',null
    ),'expected_parent_version',null,'occurrence',jsonb_build_object(
      'task_id','62100000-0000-0000-0000-000000000001',
      'occurrence_date',timegenie.workspace_work_date('20000000-0000-0000-0000-000000000002',to_timestamp(start_millis::double precision/1000)),
      'origin','scheduled','status','open','completed_at',null,'created_at',start_millis,'updated_at',start_millis,'version',1
    ),'active_entry',jsonb_build_object(
      'id','74100000-0000-0000-0000-000000000011','work_date',timegenie.workspace_work_date('20000000-0000-0000-0000-000000000002',to_timestamp(start_millis::double precision/1000)),
      'kind','work','source_type','timer','state','running','default_task_id','62100000-0000-0000-0000-000000000001','label_snapshot','原子转换一级事项',
      'started_at',start_millis,'ended_at',null,'duration_seconds',0,'note',null,'created_at',start_millis,'updated_at',start_millis,'version',1,
      'segments',jsonb_build_array(jsonb_build_object('id','74200000-0000-0000-0000-000000000011','sequence_no',1,'started_at',start_millis,'ended_at',null,'duration_seconds',0)),
      'allocations','[]'::jsonb
    )
  );
  invalid_occurrence_payload:=jsonb_set(
    start_payload,
    '{occurrence,task_id}',
    to_jsonb('62100000-0000-0000-0000-000000000099'::text)
  );
  begin
    perform timegenie.timer_transition_apply(
      '20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000003','42000000-0000-0000-0000-000000000010',invalid_occurrence_payload
    );
    raise exception 'mismatched timer occurrence was accepted';
  exception when others then if sqlerrm not like '%timer occurrence identity mismatch%' then raise; end if; end;
  reset role;
  if exists(select 1 from timegenie.tasks where id='62100000-0000-0000-0000-000000000001')
     or exists(select 1 from timegenie.time_entries where id='74100000-0000-0000-0000-000000000011')
     or exists(select 1 from timegenie.task_occurrences where task_id='62100000-0000-0000-0000-000000000001') then
    raise exception 'invalid timer occurrence left partial remote data';
  end if;
  set role authenticated;
  first_result:=timegenie.timer_transition_apply(
    '20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000003','42000000-0000-0000-0000-000000000011',start_payload
  );
  retry_result:=timegenie.timer_transition_apply(
    '20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000003','42000000-0000-0000-0000-000000000011',start_payload
  );
  reset role;
  if first_result is distinct from retry_result
     or (select count(*) from timegenie.tasks where id='62100000-0000-0000-0000-000000000001')<>1
     or (select count(*) from timegenie.task_occurrences where task_id='62100000-0000-0000-0000-000000000001')<>1
     or first_result->'occurrence'->>'status' is distinct from 'open'
     or (select count(*) from timegenie.time_entries where id='74100000-0000-0000-0000-000000000011' and state='running')<>1 then
    raise exception 'timer transition create/start retry was not idempotent';
  end if;
  set role authenticated;

  switch_payload:=jsonb_build_object(
    'previous_entry',(start_payload->'active_entry')||jsonb_build_object(
      'state','ended','ended_at',switch_millis,'duration_seconds',60,'updated_at',switch_millis,'version',2,
      'segments',jsonb_build_array(jsonb_build_object('id','74200000-0000-0000-0000-000000000011','sequence_no',1,'started_at',start_millis,'ended_at',switch_millis,'duration_seconds',60))
    ),'previous_base_version',1,'task',jsonb_build_object(
      'id','62100000-0000-0000-0000-000000000002','subject_id','52000000-0000-0000-0000-000000000001','parent_id','62100000-0000-0000-0000-000000000001',
      'title','原子转换二级事项','status','open','planned_date',timegenie.workspace_work_date('20000000-0000-0000-0000-000000000002',to_timestamp(switch_millis::double precision/1000)),
      'planned_time',null,'estimate_minutes',null,'note',null,'project_name',null,'solution_name',null,'source_type','timer_quick_create',
      'source_ref',null,'sort_order',10,'completed_at',null,'created_at',switch_millis,'updated_at',switch_millis,'version',1,'deleted_at',null
    ),'expected_parent_version',1,'active_entry',jsonb_build_object(
      'id','74100000-0000-0000-0000-000000000012','work_date',timegenie.workspace_work_date('20000000-0000-0000-0000-000000000002',to_timestamp(switch_millis::double precision/1000)),
      'kind','work','source_type','timer','state','running','default_task_id','62100000-0000-0000-0000-000000000002','label_snapshot','原子转换二级事项',
      'started_at',switch_millis,'ended_at',null,'duration_seconds',0,'note',null,'created_at',switch_millis,'updated_at',switch_millis,'version',1,
      'segments',jsonb_build_array(jsonb_build_object('id','74200000-0000-0000-0000-000000000012','sequence_no',1,'started_at',switch_millis,'ended_at',null,'duration_seconds',0)),
      'allocations','[]'::jsonb
    )
  );
  perform timegenie.timer_transition_apply(
    '20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000003','42000000-0000-0000-0000-000000000012',switch_payload
  );
  reset role;
  if not exists(select 1 from timegenie.time_entries where id='74100000-0000-0000-0000-000000000011' and state='ended' and ended_at=to_timestamp(switch_millis::double precision/1000))
     or not exists(select 1 from timegenie.time_entries where id='74100000-0000-0000-0000-000000000012' and state='running' and started_at=to_timestamp(switch_millis::double precision/1000))
     or (select count(*) from timegenie.time_entries where workspace_id='20000000-0000-0000-0000-000000000002' and state in ('running','paused') and deleted_at is null)<>1 then
    raise exception 'timer transition switch did not preserve one boundary and one active timer';
  end if;
  set role authenticated;

  stale_payload:=jsonb_build_object(
    'previous_entry',(switch_payload->'active_entry')||jsonb_build_object('state','ended','ended_at',switch_millis+60000,'updated_at',switch_millis+60000,'version',2,
      'segments',jsonb_build_array(jsonb_build_object('id','74200000-0000-0000-0000-000000000012','sequence_no',1,'started_at',switch_millis,'ended_at',switch_millis+60000,'duration_seconds',60))),
    'previous_base_version',999,'task',jsonb_build_object(
      'id','62100000-0000-0000-0000-000000000003','subject_id','52000000-0000-0000-0000-000000000001','parent_id',null,'title','冲突时不应创建',
      'status','open','planned_date','2026-10-16','planned_time',null,'estimate_minutes',null,'note',null,'project_name',null,'solution_name',null,
      'source_type','timer_quick_create','source_ref',null,'sort_order',20,'completed_at',null,'created_at',switch_millis+60000,'updated_at',switch_millis+60000,'version',1,'deleted_at',null
    ),'expected_parent_version',null,'active_entry',jsonb_build_object(
      'id','74100000-0000-0000-0000-000000000013','work_date','2026-10-16','kind','work','source_type','timer','state','running',
      'default_task_id','62100000-0000-0000-0000-000000000003','label_snapshot','冲突时不应创建','started_at',switch_millis+60000,'ended_at',null,
      'duration_seconds',0,'note',null,'created_at',switch_millis+60000,'updated_at',switch_millis+60000,'version',1,
      'segments',jsonb_build_array(jsonb_build_object('id','74200000-0000-0000-0000-000000000013','sequence_no',1,'started_at',switch_millis+60000,'ended_at',null,'duration_seconds',0)),
      'allocations','[]'::jsonb
    )
  );
  begin
    perform timegenie.timer_transition_apply(
      '20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000003','42000000-0000-0000-0000-000000000013',stale_payload
    );
    raise exception 'stale timer transition was accepted';
  exception when others then if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if; end;
  reset role;
  if exists(select 1 from timegenie.tasks where id='62100000-0000-0000-0000-000000000003')
     or exists(select 1 from timegenie.time_entries where id='74100000-0000-0000-0000-000000000013')
     or exists(select 1 from timegenie.processed_operations where operation_id='42000000-0000-0000-0000-000000000013') then
    raise exception 'failed timer transition left partial remote data';
  end if;
  set role authenticated;
end $$;

-- Owner isolation and registered-device checks also apply to the shared RPCs.
select set_config('request.jwt.claim.sub', '10000000-0000-0000-0000-000000000001', true);
do $$ begin
  begin
    perform timegenie.unassigned_get_or_create_shared(
      '20000000-0000-0000-0000-000000000002',
      '30000000-0000-0000-0000-000000000003',null,null,null
    );
    raise exception 'another owner read shared unassigned state';
  exception when others then
    if sqlerrm not like '%AUTH_REQUIRED%' then raise; end if;
  end;
end $$;

reset role;

-- Every entity type published by the supported workspace change triggers has
-- a deterministic incremental representation. Child rows are intentionally
-- aggregated to their time-entry or report root.
do $$
declare mapping record;
declare entity jsonb;
begin
  for mapping in
    select * from (values
      ('subjects', '00000000-0000-0000-0000-000000000001', 'subjects'),
      ('work_days', '2099-01-01', 'work_days'),
      ('app_settings', 'missing-setting', 'app_settings'),
      ('tasks', '00000000-0000-0000-0000-000000000002', 'tasks'),
      ('task_status_events', '00000000-0000-0000-0000-000000000003', 'task_status_events'),
      ('task_daily_estimates', '00000000-0000-0000-0000-000000000002|2099-01-01', 'task_daily_estimates'),
      ('task_recurrence_rules', '00000000-0000-0000-0000-000000000002', 'task_recurrence_rules'),
      ('task_occurrences', '00000000-0000-0000-0000-000000000002|2099-01-01', 'task_occurrences'),
      ('unassigned_sessions', '00000000-0000-0000-0000-000000000007', 'unassigned_sessions'),
      ('time_entries', '00000000-0000-0000-0000-000000000004', 'time_entries'),
      ('time_segments', '00000000-0000-0000-0000-000000000004', 'time_entries'),
      ('time_allocations', '00000000-0000-0000-0000-000000000004', 'time_entries'),
      ('report_templates', '00000000-0000-0000-0000-000000000005', 'report_templates'),
      ('reports', '00000000-0000-0000-0000-000000000006', 'reports'),
      ('report_tasks', '00000000-0000-0000-0000-000000000006', 'reports'),
      ('integration_configs', 'missing-provider', 'integration_configs'),
      ('external_bindings', 'missing-provider|task|00000000-0000-0000-0000-000000000002', 'external_bindings')
    ) values_table(source_type, entity_id, result_type)
  loop
    entity := timegenie.cloud_incremental_entity_get(
      '20000000-0000-0000-0000-000000000001',
      mapping.source_type,
      mapping.entity_id
    );
    if entity->>'source_entity_type' <> mapping.source_type
       or entity->>'entity_type' <> mapping.result_type
       or entity->>'entity_id' <> mapping.entity_id
       or not (entity ? 'deleted')
       or not (entity ? 'data') then
      raise exception 'incremental RPC has no applicable representation for %: %', mapping.source_type, entity;
    end if;
  end loop;
end $$;

do $$
begin
  if (
    select count(*)
    from timegenie.unassigned_segments segment
    join timegenie.unassigned_sessions session on session.id = segment.session_id
    where session.workspace_id = '20000000-0000-0000-0000-000000000001'
      and segment.ended_at is null
  ) <> 1 then
    raise exception 'background collector has duplicate open segments';
  end if;
end $$;

do $$
begin
  if not exists(
    select 1
    from pg_publication_tables
    where pubname = 'supabase_realtime'
      and schemaname = 'timegenie'
      and tablename = 'workspace_changes'
  ) then
    raise exception 'workspace_changes is not in the realtime publication';
  end if;
end $$;

-- Actual adjustment batches validate their complete task/adjustment image and
-- commit atomically. Ordinary allocations do not change the adjustment
-- fingerprint, while an adjustment created from the same old snapshot does.
reset role;
insert into timegenie.subjects(id,workspace_id,name,sort_order)
values('51000000-0000-0000-0000-000000000095','20000000-0000-0000-0000-000000000001','实际调整批次',95);
insert into timegenie.tasks(id,workspace_id,subject_id,title,status,source_type,sort_order,version) values
  ('61000000-0000-0000-0000-000000000095','20000000-0000-0000-0000-000000000001','51000000-0000-0000-0000-000000000095','已有叶子','open','manual',10,1),
  ('61000000-0000-0000-0000-000000000096','20000000-0000-0000-0000-000000000001','51000000-0000-0000-0000-000000000095','父事项','open','manual',20,1);
insert into timegenie.time_entries(id,workspace_id,work_date,kind,source_type,state,label_snapshot,started_at,ended_at,duration_seconds,created_at,updated_at,version)
values('71000000-0000-0000-0000-000000000095','20000000-0000-0000-0000-000000000001','2026-10-15','work','manual','ended','已有叶子',to_timestamp(1791993600),to_timestamp(1791997200),3600,to_timestamp(1791993600),to_timestamp(1791997200),1);
insert into timegenie.time_allocations(id,workspace_id,entry_id,task_id,minutes,created_at,updated_at,version)
values('72000000-0000-0000-0000-000000000095','20000000-0000-0000-0000-000000000001','71000000-0000-0000-0000-000000000095','61000000-0000-0000-0000-000000000095',60,to_timestamp(1791997200),to_timestamp(1791997200),1);
set role authenticated;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
do $$
declare empty_fingerprint text; first_result jsonb; retry_result jsonb; payload jsonb;
begin
  empty_fingerprint:='e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855';
  if empty_fingerprint<>'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855' then
    raise exception 'empty adjustment fingerprint test vector changed';
  end if;

  payload:=jsonb_build_object(
    'subjectId','51000000-0000-0000-0000-000000000095','workDate','2026-10-15',
    'clientRequestId','40000000-0000-0000-0000-000000000195','batchId','41000000-0000-0000-0000-000000000195','note','批次核对',
    'items',jsonb_build_array(jsonb_build_object(
      'taskId','61000000-0000-0000-0000-000000000095','expectedTaskVersion',1,
      'baselineMinutes',60,'targetMinutes',75,'deltaMinutes',15,'adjustmentFingerprint',empty_fingerprint
    )),
    'newTasks',jsonb_build_array(
      jsonb_build_object('clientTaskId','draft-positive','title','新子项','parentId','61000000-0000-0000-0000-000000000096','expectedParentVersion',1,'targetMinutes',25),
      jsonb_build_object('clientTaskId','draft-zero','title','仅建事项','parentId',null,'expectedParentVersion',null,'targetMinutes',0)
    ),
    'createdTasks',jsonb_build_array(
      jsonb_build_object('clientTaskId','draft-positive','task',jsonb_build_object(
        'id','61100000-0000-0000-0000-000000000095','subject_id','51000000-0000-0000-0000-000000000095','parent_id','61000000-0000-0000-0000-000000000096','title','新子项','status','open','planned_date','2026-10-15','planned_time',null,'estimate_minutes',null,'note',null,'project_name',null,'solution_name',null,'source_type','manual','source_ref','actual_adjustment_quick_create','sort_order',10,'created_at',1791997200000,'updated_at',1791997200000,'version',1,'deleted_at',null
      )),
      jsonb_build_object('clientTaskId','draft-zero','task',jsonb_build_object(
        'id','61100000-0000-0000-0000-000000000096','subject_id','51000000-0000-0000-0000-000000000095','parent_id',null,'title','仅建事项','status','open','planned_date','2026-10-15','planned_time',null,'estimate_minutes',null,'note',null,'project_name',null,'solution_name',null,'source_type','manual','source_ref','actual_adjustment_quick_create','sort_order',30,'created_at',1791997200000,'updated_at',1791997200000,'version',1,'deleted_at',null
      ))
    ),
    'adjustments',jsonb_build_array(
      jsonb_build_object('id','81000000-0000-0000-0000-000000000095','subject_id','51000000-0000-0000-0000-000000000095','task_id','61000000-0000-0000-0000-000000000095','work_date','2026-10-15','delta_minutes',15,'baseline_minutes',60,'target_minutes',75,'note','批次核对','source_type','quick_actual_adjustment','batch_id','41000000-0000-0000-0000-000000000195','created_at',1791997200000,'updated_at',1791997200000,'version',1,'deleted_at',null),
      jsonb_build_object('id','81000000-0000-0000-0000-000000000096','subject_id','51000000-0000-0000-0000-000000000095','task_id','61100000-0000-0000-0000-000000000095','work_date','2026-10-15','delta_minutes',25,'baseline_minutes',0,'target_minutes',25,'note','批次核对','source_type','quick_actual_adjustment','batch_id','41000000-0000-0000-0000-000000000195','created_at',1791997200000,'updated_at',1791997200000,'version',1,'deleted_at',null)
    )
  );
  first_result:=timegenie.actual_adjustment_batch_apply('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002','40000000-0000-0000-0000-000000000195',payload);
  retry_result:=timegenie.actual_adjustment_batch_apply('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002','40000000-0000-0000-0000-000000000195',payload);
  if first_result is distinct from retry_result or (first_result->>'createdTaskCount')::integer<>2 or (first_result->>'createdAdjustmentCount')::integer<>2 then
    raise exception 'actual adjustment batch retry changed result: % / %',first_result,retry_result;
  end if;
  reset role;
  if (select count(*) from timegenie.tasks where id in('61100000-0000-0000-0000-000000000095','61100000-0000-0000-0000-000000000096'))<>2
     or (select count(*) from timegenie.actual_time_adjustments where batch_id='41000000-0000-0000-0000-000000000195')<>2
     or exists(select 1 from timegenie.actual_time_adjustments where task_id='61100000-0000-0000-0000-000000000096') then
    raise exception 'actual adjustment batch did not create exact task/adjustment set';
  end if;
  set role authenticated;

  -- A normal allocation changes current actual, but not the adjustment-set fingerprint.
  reset role;
  insert into timegenie.time_entries(id,workspace_id,work_date,kind,source_type,state,label_snapshot,started_at,ended_at,duration_seconds,created_at,updated_at,version)
  values('71000000-0000-0000-0000-000000000096','20000000-0000-0000-0000-000000000001','2026-10-15','work','manual','ended','新计时',to_timestamp(1791997200),to_timestamp(1791997800),600,to_timestamp(1791997200),to_timestamp(1791997800),1);
  insert into timegenie.time_allocations(id,workspace_id,entry_id,task_id,minutes,created_at,updated_at,version)
  values('72000000-0000-0000-0000-000000000096','20000000-0000-0000-0000-000000000001','71000000-0000-0000-0000-000000000096','61000000-0000-0000-0000-000000000096',10,to_timestamp(1791997800),to_timestamp(1791997800),1);
  set role authenticated;
  begin
    perform timegenie.actual_adjustment_batch_apply(
      '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002','40000000-0000-0000-0000-000000000196',
      jsonb_build_object('subjectId','51000000-0000-0000-0000-000000000095','workDate','2026-10-15','clientRequestId','40000000-0000-0000-0000-000000000196','batchId','41000000-0000-0000-0000-000000000196','items','[]'::jsonb,
        'newTasks',jsonb_build_array(jsonb_build_object('clientTaskId','stale-parent','title','应整体失败','parentId','61000000-0000-0000-0000-000000000096','expectedParentVersion',1,'targetMinutes',5)),
        'createdTasks',jsonb_build_array(jsonb_build_object('clientTaskId','stale-parent','task',jsonb_build_object('id','61100000-0000-0000-0000-000000000097','subject_id','51000000-0000-0000-0000-000000000095','parent_id','61000000-0000-0000-0000-000000000096','title','应整体失败','status','open','planned_date','2026-10-15','planned_time',null,'estimate_minutes',null,'note',null,'project_name',null,'solution_name',null,'source_type','manual','source_ref','actual_adjustment_quick_create','sort_order',20,'created_at',1791997800000,'updated_at',1791997800000,'version',1,'deleted_at',null))),
        'adjustments',jsonb_build_array(jsonb_build_object('id','81000000-0000-0000-0000-000000000097','subject_id','51000000-0000-0000-0000-000000000095','task_id','61100000-0000-0000-0000-000000000097','work_date','2026-10-15','delta_minutes',5,'baseline_minutes',0,'target_minutes',5,'note',null,'source_type','quick_actual_adjustment','batch_id','41000000-0000-0000-0000-000000000196','created_at',1791997800000,'updated_at',1791997800000,'version',1,'deleted_at',null))
      )
    );
    raise exception 'parent with direct actual accepted new child';
  exception when others then if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if; end;
  reset role;
  if exists(select 1 from timegenie.tasks where id='61100000-0000-0000-0000-000000000097')
     or exists(select 1 from timegenie.actual_time_adjustments where id='81000000-0000-0000-0000-000000000097')
     or exists(select 1 from timegenie.processed_operations where operation_id='40000000-0000-0000-0000-000000000196') then
    raise exception 'failed actual adjustment batch left partial remote data';
  end if;
  set role authenticated;

  -- A stale local baseline still carries the user's target. Rebase its fixed
  -- delta against the current cloud actual instead of creating a conflict.
  first_result:=timegenie.actual_adjustment_batch_apply(
      '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002','40000000-0000-0000-0000-000000000197',
      jsonb_build_object('subjectId','51000000-0000-0000-0000-000000000095','workDate','2026-10-15','clientRequestId','40000000-0000-0000-0000-000000000197','batchId','41000000-0000-0000-0000-000000000197',
        'items',jsonb_build_array(jsonb_build_object('taskId','61000000-0000-0000-0000-000000000095','expectedTaskVersion',1,'baselineMinutes',60,'targetMinutes',55,'deltaMinutes',-5,'adjustmentFingerprint',empty_fingerprint)),
        'newTasks','[]'::jsonb,'createdTasks','[]'::jsonb,
        'adjustments',jsonb_build_array(jsonb_build_object('id','81000000-0000-0000-0000-000000000098','subject_id','51000000-0000-0000-0000-000000000095','task_id','61000000-0000-0000-0000-000000000095','work_date','2026-10-15','delta_minutes',-5,'baseline_minutes',60,'target_minutes',55,'note',null,'source_type','quick_actual_adjustment','batch_id','41000000-0000-0000-0000-000000000197','created_at',1791997900000,'updated_at',1791997900000,'version',1,'deleted_at',null))
      )
    );
  reset role;
  if not exists(select 1 from timegenie.actual_time_adjustments where id='81000000-0000-0000-0000-000000000098' and baseline_minutes=75 and delta_minutes=-20 and target_minutes=55)
     or (select coalesce(sum(a.minutes),0) from timegenie.time_allocations a join timegenie.time_entries e on e.id=a.entry_id where a.task_id='61000000-0000-0000-0000-000000000095' and e.work_date='2026-10-15' and e.deleted_at is null)
        +(select coalesce(sum(delta_minutes),0) from timegenie.actual_time_adjustments where task_id='61000000-0000-0000-0000-000000000095' and work_date='2026-10-15' and deleted_at is null)<>55
     or (first_result->'adjustments'->0->>'delta_minutes')::integer<>-20 then
    raise exception 'stale adjustment target was not rebased: %',first_result;
  end if;
  set role authenticated;

  -- Ordinary time added after the snapshot is folded into the latest target,
  -- so the final actual still equals the user's requested minutes.
  reset role;
  insert into timegenie.time_entries(id,workspace_id,work_date,kind,source_type,state,label_snapshot,started_at,ended_at,duration_seconds,created_at,updated_at,version)
  values('71000000-0000-0000-0000-000000000097','20000000-0000-0000-0000-000000000001','2026-10-16','work','manual','ended','普通计时增长',to_timestamp(1792080000),to_timestamp(1792083600),3600,to_timestamp(1792080000),to_timestamp(1792083600),1);
  insert into timegenie.time_allocations(id,workspace_id,entry_id,task_id,minutes,created_at,updated_at,version)
  values('72000000-0000-0000-0000-000000000097','20000000-0000-0000-0000-000000000001','71000000-0000-0000-0000-000000000097','61000000-0000-0000-0000-000000000095',65,to_timestamp(1792083600),to_timestamp(1792083600),1);
  set role authenticated;
  perform timegenie.actual_adjustment_batch_apply(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002','40000000-0000-0000-0000-000000000198',
    jsonb_build_object('subjectId','51000000-0000-0000-0000-000000000095','workDate','2026-10-16','clientRequestId','40000000-0000-0000-0000-000000000198','batchId','41000000-0000-0000-0000-000000000198',
      'items',jsonb_build_array(jsonb_build_object('taskId','61000000-0000-0000-0000-000000000095','expectedTaskVersion',1,'baselineMinutes',60,'targetMinutes',75,'deltaMinutes',15,'adjustmentFingerprint',empty_fingerprint)),
      'newTasks','[]'::jsonb,'createdTasks','[]'::jsonb,
      'adjustments',jsonb_build_array(jsonb_build_object('id','81000000-0000-0000-0000-000000000099','subject_id','51000000-0000-0000-0000-000000000095','task_id','61000000-0000-0000-0000-000000000095','work_date','2026-10-16','delta_minutes',15,'baseline_minutes',60,'target_minutes',75,'note',null,'source_type','quick_actual_adjustment','batch_id','41000000-0000-0000-0000-000000000198','created_at',1792083600000,'updated_at',1792083600000,'version',1,'deleted_at',null))
    )
  );
  reset role;
  if (select coalesce(sum(a.minutes),0) from timegenie.time_allocations a join timegenie.time_entries e on e.id=a.entry_id where a.task_id='61000000-0000-0000-0000-000000000095' and e.work_date='2026-10-16' and e.deleted_at is null)
       +(select coalesce(sum(delta_minutes),0) from timegenie.actual_time_adjustments where task_id='61000000-0000-0000-0000-000000000095' and work_date='2026-10-16' and deleted_at is null)<>75 then
    raise exception 'ordinary timing growth did not preserve the requested actual target';
  end if;
  update timegenie.actual_time_adjustments set deleted_at=to_timestamp(1792083600.123),updated_at=to_timestamp(1792083600.123),version=2
  where id='81000000-0000-0000-0000-000000000099';
  if timegenie.actual_adjustment_fingerprint('20000000-0000-0000-0000-000000000001','61000000-0000-0000-0000-000000000095','2026-10-16')
     <> encode(extensions.digest('81000000-0000-0000-0000-000000000099:2:1792083600123','sha256'),'hex') then
    raise exception 'PostgreSQL deleted-at fingerprint differs from Rust millisecond canonical format';
  end if;
  set role authenticated;
end $$;
reset role;

-- Semantic operation history is idempotent, sanitized, read-only and isolated.
set local role authenticated;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
do $$
declare list_result jsonb; detail_result jsonb; before_task_count bigint; before_change_count bigint; before_log_count bigint;
begin
  reset role;
  if (select count(*) from timegenie.cloud_operation_logs where workspace_id='20000000-0000-0000-0000-000000000001')=0 then
    raise exception 'successful cloud writes did not produce operation history';
  end if;
  if exists(
    select 1 from timegenie.cloud_operation_log_items item
    where coalesce(item.before_json,'{}'::jsonb)::text||coalesce(item.after_json,'{}'::jsonb)::text
      ~* '(token|password|secret_ref|access_token|refresh_token|service_role|action_config)'
  ) then raise exception 'operation history leaked protected fields'; end if;
  if (select count(*) from timegenie.cloud_operation_logs where workspace_id='20000000-0000-0000-0000-000000000001' and operation_id='40000000-0000-0000-0000-000000000195')<>1 then
    raise exception 'idempotent batch produced duplicate operation history';
  end if;
  if exists(select 1 from timegenie.cloud_operation_logs where operation_id='40000000-0000-0000-0000-000000000196') then
    raise exception 'failed operation was recorded as successful history';
  end if;
  if not exists(select 1 from timegenie.cloud_operation_logs where operation_id='40000000-0000-0000-0000-000000000197') then
    raise exception 'rebased actual adjustment was not recorded as successful history';
  end if;
  before_task_count:=(select count(*) from timegenie.tasks where workspace_id='20000000-0000-0000-0000-000000000001');
  before_change_count:=(select count(*) from timegenie.workspace_changes where workspace_id='20000000-0000-0000-0000-000000000001');
  set role authenticated;
  list_result:=timegenie.cloud_operation_history_list('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002',2,null,null,null,null,null,null,null,null);
  if jsonb_array_length(list_result->'items')=0 then raise exception 'operation history list returned no rows'; end if;
  if pg_get_function_arguments('timegenie.cloud_operation_history_list(uuid,uuid,integer,timestamptz,uuid,timestamptz,timestamptz,uuid,text,text,text)'::regprocedure) not like '%p_limit integer DEFAULT 50%' then raise exception 'operation history RPC default page size is not 50'; end if;
  detail_result:=timegenie.cloud_operation_history_detail('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002',(list_result->'items'->0->>'id')::uuid);
  if jsonb_array_length(detail_result->'items')=0 then raise exception 'operation history detail returned no items'; end if;
  if detail_result->>'operationId' is null
     or detail_result->>'businessType' is null
     or detail_result->>'deviceNameSnapshot' is null
     or detail_result->>'occurredAt' is null then
    raise exception 'operation history detail did not return the camelCase API contract: %',detail_result;
  end if;
  reset role;
  if before_task_count<>(select count(*) from timegenie.tasks where workspace_id='20000000-0000-0000-0000-000000000001')
     or before_change_count<>(select count(*) from timegenie.workspace_changes where workspace_id='20000000-0000-0000-0000-000000000001') then
    raise exception 'operation history browsing changed business or sync data';
  end if;
  begin
    set role authenticated;
    perform timegenie.cloud_operation_history_list('20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000002',30,null,null,null,null,null,null,null,null);
    raise exception 'cross-workspace operation history read succeeded';
  exception when others then if sqlerrm not like '%DEVICE_NOT_REGISTERED%' then raise; end if; end;
  reset role;
  update timegenie.cloud_operation_logs set expires_at=now()-interval '1 second' where workspace_id='20000000-0000-0000-0000-000000000001';
  before_log_count:=(select count(*) from timegenie.cloud_operation_logs where workspace_id='20000000-0000-0000-0000-000000000001');
  set role authenticated;
  perform timegenie.cloud_operation_history_cleanup('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002');
  reset role;
  if before_log_count=0 or exists(select 1 from timegenie.cloud_operation_logs where workspace_id='20000000-0000-0000-0000-000000000001' and expires_at<=now()) then
    raise exception 'expired operation history was not cleaned';
  end if;
  if before_task_count<>(select count(*) from timegenie.tasks where workspace_id='20000000-0000-0000-0000-000000000001') then
    raise exception 'operation history cleanup changed business data';
  end if;
  set role authenticated;
end $$;
reset role;

-- Workspace snapshots are grouped, checksummed, isolated, previewable and retained safely.
set local role authenticated;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
do $$
declare manual_result jsonb; automatic_result jsonb; repeated_result jsonb; preview_result jsonb; delete_result jsonb;
  manual_id uuid; automatic_id uuid; delete_id uuid; corrupt_id uuid; protected_id uuid; failed_id uuid;
  before_task_count bigint; before_change_count bigint; before_log_count bigint; before_expiry timestamptz;
begin
  if not exists(
    select 1 from pg_proc p join pg_namespace n on n.oid=p.pronamespace
    where n.nspname='timegenie' and p.proname='workspace_snapshot_create'
      and 'statement_timeout=120s'=any(coalesce(p.proconfig,array[]::text[]))
  ) or exists(
    select 1 from pg_proc p join pg_namespace n on n.oid=p.pronamespace
    where n.nspname='timegenie'
      and p.proname in('workspace_snapshot_ensure_automatic','workspace_snapshot_ensure_automatic_at','workspace_snapshot_slot_key')
  ) then raise exception 'obsolete sync-triggered automatic snapshot RPC still exists'; end if;
  manual_result:=timegenie.workspace_snapshot_create(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002','40000000-0000-0000-0000-000000000201',
    'manual','集成测试手动镜像','验证四组分块、哈希和预览',false
  );
  manual_id:=(manual_result->>'id')::uuid;
  if manual_result->>'status'<>'ready' or (manual_result->>'chunkCount')::integer<21 then raise exception 'manual snapshot was not published with entity chunks: %',manual_result; end if;
  reset role;
  if (select count(distinct group_name) from timegenie.workspace_snapshot_chunks where snapshot_id=manual_id)<>4 then raise exception 'snapshot did not contain four protected entity groups'; end if;
  if exists(select 1 from timegenie.workspace_snapshot_chunks where snapshot_id=manual_id and content_json::text~*'(secret_ref|access_token|refresh_token|lease_token|action_config_json)') then raise exception 'snapshot chunk leaked protected fields'; end if;
  if (select count(*) from timegenie.workspace_snapshot_chunks where snapshot_id=manual_id)<>(select chunk_count from timegenie.workspace_snapshots where id=manual_id) then raise exception 'snapshot chunk count metadata is inconsistent'; end if;
  set role authenticated;
  reset role;
  before_task_count:=(select count(*) from timegenie.tasks where workspace_id='20000000-0000-0000-0000-000000000001');
  before_change_count:=(select count(*) from timegenie.workspace_changes where workspace_id='20000000-0000-0000-0000-000000000001');
  before_expiry:=(select expires_at from timegenie.workspace_snapshots where id=manual_id);
  set role authenticated;
  preview_result:=timegenie.workspace_snapshot_preview('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002',manual_id);
  if not coalesce((preview_result->>'restorable')::boolean,false)
     or preview_result->'entityCounts' is null
     or preview_result->'groupCounts' is null
     or preview_result->'groups' is not null
     or preview_result->'entities' is not null then
    raise exception 'lightweight snapshot details failed: %',preview_result;
  end if;
  reset role;
  if before_task_count<>(select count(*) from timegenie.tasks where workspace_id='20000000-0000-0000-0000-000000000001')
     or before_change_count<>(select count(*) from timegenie.workspace_changes where workspace_id='20000000-0000-0000-0000-000000000001')
     or before_expiry is distinct from (select expires_at from timegenie.workspace_snapshots where id=manual_id) then raise exception 'snapshot preview changed business data, sync state or retention'; end if;
  set role authenticated;

  begin
    perform timegenie.workspace_snapshot_list('20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000002');
    raise exception 'cross-workspace snapshot access succeeded';
  exception when others then if sqlerrm not like '%DEVICE_NOT_REGISTERED%' then raise; end if; end;
  begin
    perform timegenie.workspace_snapshot_list('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000098');
    raise exception 'revoked device read snapshots';
  exception when others then if sqlerrm not like '%DEVICE_NOT_REGISTERED%' then raise; end if; end;

  delete_result:=timegenie.workspace_snapshot_create(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002','40000000-0000-0000-0000-000000000202',
    'manual','待删除镜像',null,false
  );
  delete_id:=(delete_result->>'id')::uuid;
  perform timegenie.workspace_snapshot_delete('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002',delete_id);
  reset role;
  if exists(select 1 from timegenie.workspace_snapshot_chunks where snapshot_id=delete_id) then raise exception 'snapshot delete did not cascade to chunks'; end if;
  set role authenticated;

  automatic_result:=timegenie.workspace_snapshot_create(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001','40000000-0000-0000-0000-000000000203',
    'automatic','每日自动镜像',null,true
  );
  automatic_id:=(automatic_result->>'id')::uuid;
  repeated_result:=timegenie.workspace_snapshot_create(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002','40000000-0000-0000-0000-000000000204',
    'automatic','并发自动镜像',null,true
  );
  if (repeated_result->>'id')::uuid<>automatic_id then raise exception 'same-day automatic snapshot was duplicated'; end if;
  reset role;
  if (select count(*) from timegenie.workspace_snapshots where workspace_id='20000000-0000-0000-0000-000000000001' and snapshot_type='automatic' and snapshot_day=current_date and status='ready')<>1 then raise exception 'automatic snapshot uniqueness failed'; end if;
  if (select expires_at-created_at from timegenie.workspace_snapshots where id=automatic_id) not between interval '29 days 23 hours' and interval '30 days 1 hour' then raise exception 'automatic snapshot retention is not 30 days'; end if;
  set role authenticated;

  protected_id:=(timegenie.workspace_snapshot_create(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002','40000000-0000-0000-0000-000000000205',
    'pre_restore','回滚前保护镜像',null,false
  )->>'id')::uuid;
  perform timegenie.workspace_snapshot_set_protected('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002',protected_id,true);
  reset role;
  update timegenie.workspace_snapshots set expires_at=now()-interval '1 second' where id in(automatic_id,protected_id);
  set role authenticated;
  perform timegenie.workspace_snapshot_cleanup('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002');
  reset role;
  if exists(select 1 from timegenie.workspace_snapshots where id=automatic_id) then raise exception 'expired automatic snapshot was not cleaned'; end if;
  if not exists(select 1 from timegenie.workspace_snapshots where id=protected_id and protected and protected_at is not null) then raise exception 'protected snapshot was removed by cleanup'; end if;
  set role authenticated;

  corrupt_id:=(timegenie.workspace_snapshot_create(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002','40000000-0000-0000-0000-000000000206',
    'manual','待篡改镜像',null,false
  )->>'id')::uuid;
  reset role;
  update timegenie.workspace_snapshot_chunks set content_json=content_json||jsonb_build_object('tampered',true) where snapshot_id=corrupt_id and chunk_index=(select min(chunk_index) from timegenie.workspace_snapshot_chunks where snapshot_id=corrupt_id);
  set role authenticated;
  preview_result:=timegenie.workspace_snapshot_preview('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002',corrupt_id);
  if not coalesce((preview_result->>'restorable')::boolean,false) then raise exception 'metadata details unexpectedly inspected chunk payloads'; end if;
  reset role;
  preview_result:=timegenie.workspace_snapshot_verify_integrity('20000000-0000-0000-0000-000000000001',corrupt_id);
  set role authenticated;
  if coalesce((preview_result->>'restorable')::boolean,true) or preview_result->>'errorCode'<>'SNAPSHOT_CORRUPT' then raise exception 'tampered snapshot remained restorable'; end if;
  reset role;
  if (select status from timegenie.workspace_snapshots where id=corrupt_id)<>'corrupt' then raise exception 'tampered snapshot was not marked corrupt'; end if;

  update timegenie.workspace_history_settings set snapshot_max_bytes=1048576,manual_snapshot_limit=2 where workspace_id='20000000-0000-0000-0000-000000000001';
  insert into timegenie.app_settings(workspace_id,key,value_json,updated_at,version) values('20000000-0000-0000-0000-000000000001','snapshot_failure_probe',jsonb_build_object('payload',repeat('x',1100000)),now(),1);
  set role authenticated;
  failed_id:=(timegenie.workspace_snapshot_create(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002','40000000-0000-0000-0000-000000000207',
    'manual','容量失败诊断',null,false
  )->>'id')::uuid;
  reset role;
  if (select status from timegenie.workspace_snapshots where id=failed_id)<>'failed' or exists(select 1 from timegenie.workspace_snapshot_chunks where snapshot_id=failed_id) then raise exception 'failed snapshot exposed a ready partial artifact'; end if;
  delete from timegenie.app_settings where workspace_id='20000000-0000-0000-0000-000000000001' and key='snapshot_failure_probe';
  update timegenie.workspace_history_settings set snapshot_max_bytes=104857600 where workspace_id='20000000-0000-0000-0000-000000000001';
  set role authenticated;
  perform timegenie.workspace_snapshot_create('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002','40000000-0000-0000-0000-000000000208','manual','保留边界一',null,false);
  perform timegenie.workspace_snapshot_create('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002','40000000-0000-0000-0000-000000000209','manual','保留边界二',null,false);
  perform timegenie.workspace_snapshot_create('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002','40000000-0000-0000-0000-000000000210','manual','保留边界三',null,false);
  reset role;
  if (select count(*) from timegenie.workspace_snapshots where workspace_id='20000000-0000-0000-0000-000000000001' and snapshot_type='manual' and status='ready' and not protected)>2 then raise exception 'manual snapshot count retention was not enforced'; end if;
  before_log_count:=(select count(*) from timegenie.cloud_operation_logs where workspace_id='20000000-0000-0000-0000-000000000001' and business_type='workspace_snapshot.create');
  if before_log_count=0 then raise exception 'snapshot creation did not produce operation history'; end if;
  if before_task_count<>(select count(*) from timegenie.tasks where workspace_id='20000000-0000-0000-0000-000000000001') then raise exception 'snapshot retention changed business data'; end if;
  set role authenticated;
end $$;
reset role;

-- Automatic full snapshots run once per workspace-local day at a fixed time.
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
do $$
declare settings_value jsonb; early_result jsonb; due_result jsonb; repeat_result jsonb;
  next_day_result jsonb; before_count bigint; first_id uuid; second_id uuid;
begin
  set role authenticated;
  settings_value:=timegenie.workspace_snapshot_settings_get(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001');
  if not coalesce((settings_value->>'automaticSnapshotEnabled')::boolean,false) then
    raise exception 'automatic snapshots are not enabled by default: %',settings_value;
  end if;
  if settings_value->>'automaticSnapshotTime'<>'03:00'
     or settings_value->>'automaticSnapshotTimezone'<>'Asia/Shanghai'
     or settings_value->>'snapshotRetentionDays'<>'30' then
    raise exception 'automatic snapshot defaults are wrong: %',settings_value;
  end if;
  if coalesce(settings_value->>'schedulerActive','false')<>'false' then
    raise exception 'plain PostgreSQL must report the cron scheduler as inactive: %',settings_value;
  end if;
  if not exists(
    select 1 from pg_proc p join pg_namespace n on n.oid=p.pronamespace
    where n.nspname='timegenie' and p.proname='workspace_snapshot_run_scheduled'
      and 'statement_timeout=600s'=any(coalesce(p.proconfig,array[]::text[]))
  ) then raise exception 'the scheduled snapshot runner must bound its statement timeout'; end if;
  if has_function_privilege('authenticated','timegenie.workspace_snapshot_run_scheduled(timestamp with time zone)','execute')
     or has_function_privilege('anon','timegenie.workspace_snapshot_run_scheduled(timestamp with time zone)','execute') then
    raise exception 'clients must not schedule automatic snapshots';
  end if;
  if to_char((settings_value->>'nextAutomaticSnapshotAt')::timestamptz at time zone 'Asia/Shanghai','HH24:MI')<>'03:00' then
    raise exception 'next automatic snapshot time is wrong: %',settings_value;
  end if;
  settings_value:=timegenie.workspace_snapshot_settings_update(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000211',true,'21:30'::time,30);
  if settings_value->>'automaticSnapshotTime'<>'21:30' then
    raise exception 'automatic snapshot time update failed: %',settings_value;
  end if;
  settings_value:=timegenie.workspace_snapshot_settings_get(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002');
  if settings_value->>'automaticSnapshotTime'<>'21:30' then
    raise exception 'automatic snapshot time was not shared across devices: %',settings_value;
  end if;
  begin
    perform timegenie.workspace_snapshot_settings_update(
      '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
      '40000000-0000-0000-0000-000000000212',true,'21:30'::time,0);
    raise exception 'zero snapshot retention was accepted';
  exception when others then if sqlerrm not like '%VALIDATION_ERROR%' then raise; end if; end;
  reset role;

  before_count:=(select count(*) from timegenie.workspace_snapshots
    where workspace_id='20000000-0000-0000-0000-000000000001' and snapshot_type='automatic');
  early_result:=timegenie.workspace_snapshot_run_scheduled('2026-10-18 12:00:00+00');
  if (select count(*) from timegenie.workspace_snapshots
      where workspace_id='20000000-0000-0000-0000-000000000001' and snapshot_type='automatic')<>before_count then
    raise exception 'scheduled snapshot ran before its workspace-local time: %',early_result;
  end if;
  due_result:=timegenie.workspace_snapshot_run_scheduled('2026-10-18 13:30:00+00');
  if (select count(*) from timegenie.workspace_snapshots
      where workspace_id='20000000-0000-0000-0000-000000000001' and snapshot_type='automatic')<>before_count+1 then
    raise exception 'scheduled snapshot did not create exactly one recovery point: %',due_result;
  end if;
  select id into first_id from timegenie.workspace_snapshots
  where workspace_id='20000000-0000-0000-0000-000000000001' and snapshot_type='automatic'
    and schedule_slot_key='daily:2026-10-18';
  if first_id is null or (select status from timegenie.workspace_snapshots where id=first_id)<>'ready' then
    raise exception 'scheduled snapshot was not a ready recovery point: %',due_result;
  end if;
  if (select expires_at-created_at from timegenie.workspace_snapshots where id=first_id)
     not between interval '29 days 23 hours' and interval '30 days 1 hour' then
    raise exception 'scheduled snapshot retention is not 30 days';
  end if;
  repeat_result:=timegenie.workspace_snapshot_run_scheduled('2026-10-18 15:59:00+00');
  if (select count(*) from timegenie.workspace_snapshots
      where workspace_id='20000000-0000-0000-0000-000000000001' and snapshot_type='automatic')<>before_count+1 then
    raise exception 'the same workspace-local day created a second recovery point: %',repeat_result;
  end if;
  next_day_result:=timegenie.workspace_snapshot_run_scheduled('2026-10-19 13:30:00+00');
  select id into second_id from timegenie.workspace_snapshots
  where workspace_id='20000000-0000-0000-0000-000000000001' and snapshot_type='automatic'
    and schedule_slot_key='daily:2026-10-19';
  if second_id is null or second_id=first_id then
    raise exception 'the next workspace-local day did not create a new recovery point: %',next_day_result;
  end if;
  if (select count(*) from timegenie.workspace_snapshots
      where workspace_id='20000000-0000-0000-0000-000000000001' and snapshot_type='automatic')<>before_count+2 then
    raise exception 'scheduled snapshots were duplicated';
  end if;

  set role authenticated;
  settings_value:=timegenie.workspace_snapshot_settings_update(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000215',false,'21:30'::time,30);
  reset role;
  if coalesce((settings_value->>'automaticSnapshotEnabled')::boolean,true) then
    raise exception 'automatic snapshots could not be disabled: %',settings_value;
  end if;
  early_result:=timegenie.workspace_snapshot_run_scheduled('2026-10-20 13:30:00+00');
  if (select count(*) from timegenie.workspace_snapshots
      where workspace_id='20000000-0000-0000-0000-000000000001' and snapshot_type='automatic')<>before_count+2 then
    raise exception 'disabled automatic snapshots still created: %',early_result;
  end if;
  if not exists(select 1 from timegenie.workspace_snapshots where id=first_id) then
    raise exception 'settings update removed an existing snapshot';
  end if;
end $$;
reset role;

-- Snapshot retention is shared across automatic and manual recovery points.
reset role;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
do $$
declare settings_value jsonb; manual_result jsonb; manual_id uuid; expired_id uuid; protected_id uuid;
  before_task_count bigint; before_change_count bigint;
begin
  before_task_count:=(select count(*) from timegenie.tasks where workspace_id='20000000-0000-0000-0000-000000000001');
  before_change_count:=(select count(*) from timegenie.workspace_changes where workspace_id='20000000-0000-0000-0000-000000000001');
  set role authenticated;
  settings_value:=timegenie.workspace_snapshot_settings_update(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000220',true,'03:00'::time,7
  );
  if settings_value->>'snapshotRetentionDays'<>'7' then raise exception 'snapshot retention update was not returned: %',settings_value; end if;
  settings_value:=timegenie.workspace_snapshot_settings_get('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002');
  if settings_value->>'snapshotRetentionDays'<>'7' then raise exception 'snapshot retention was not shared across devices: %',settings_value; end if;
  if settings_value->>'automaticSnapshotTime'<>'03:00' then raise exception 'retention update reset the automatic snapshot time: %',settings_value; end if;
  begin
    perform timegenie.workspace_snapshot_settings_update(
      '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
      '40000000-0000-0000-0000-000000000222',true,'03:00'::time,0
    );
    raise exception 'zero snapshot retention was accepted';
  exception when others then if sqlerrm not like '%VALIDATION_ERROR%' then raise; end if; end;
  begin
    perform timegenie.workspace_snapshot_settings_update(
      '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
      '40000000-0000-0000-0000-000000000223',true,'03:00'::time,366
    );
    raise exception 'snapshot retention above 365 days was accepted';
  exception when others then if sqlerrm not like '%VALIDATION_ERROR%' then raise; end if; end;
  manual_result:=timegenie.workspace_snapshot_create(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002',
    '40000000-0000-0000-0000-000000000224','manual','保留策略手动快照',null,false
  );
  manual_id:=(manual_result->>'id')::uuid;
  reset role;
  if (select expires_at-created_at from timegenie.workspace_snapshots where id=manual_id) not between interval '6 days 23 hours' and interval '7 days 1 hour' then raise exception 'manual snapshot did not use the shared retention window'; end if;
  insert into timegenie.workspace_snapshots(
    workspace_id,snapshot_type,name,status,created_by_device_name,protected,created_at
  ) values(
    '20000000-0000-0000-0000-000000000001','manual','过期未保护快照','ready','integration-test',false,now()-interval '8 days'
  ) returning id into expired_id;
  if (select expires_at from timegenie.workspace_snapshots where id=expired_id)>now() then raise exception 'expired snapshot did not receive an elapsed retention timestamp'; end if;
  set role authenticated;
  perform timegenie.workspace_snapshot_cleanup('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002');
  reset role;
  if exists(select 1 from timegenie.workspace_snapshots where id=expired_id) then raise exception 'unprotected snapshot past retention was not cleaned'; end if;
  protected_id:=(timegenie.workspace_snapshot_create(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002',
    '40000000-0000-0000-0000-000000000225','pre_restore','保护快照保留测试',null,false
  )->>'id')::uuid;
  set role authenticated;
  perform timegenie.workspace_snapshot_set_protected('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002',protected_id,true);
  reset role;
  if (select expires_at from timegenie.workspace_snapshots where id=protected_id) is not null then raise exception 'protected snapshot kept an automatic expiry'; end if;
  update timegenie.workspace_snapshots set created_at=now()-interval '30 days' where id=protected_id;
  set role authenticated;
  perform timegenie.workspace_snapshot_cleanup('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002');
  reset role;
  if not exists(select 1 from timegenie.workspace_snapshots where id=protected_id and protected) then raise exception 'protected snapshot was removed by retention cleanup'; end if;
  if before_task_count<>(select count(*) from timegenie.tasks where workspace_id='20000000-0000-0000-0000-000000000001')
     or before_change_count<>(select count(*) from timegenie.workspace_changes where workspace_id='20000000-0000-0000-0000-000000000001') then raise exception 'snapshot retention changed business or sync data'; end if;
  set role authenticated;
end $$;
reset role;

-- The old timer-write takeover RPC remains callable for compatibility but
-- cannot change a live background lease.
update timegenie.tracking_leases set holder_device_id=null,lease_token=null,expires_at=null
where workspace_id='20000000-0000-0000-0000-000000000001';
set local role authenticated;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
do $$
declare first_lease jsonb; blocked_lease jsonb; takeover_lease jsonb;
begin
  first_lease:=timegenie.tracking_lease_acquire(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '49000000-0000-0000-0000-000000000091'
  );
  blocked_lease:=timegenie.tracking_lease_acquire(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000002',
    '49000000-0000-0000-0000-000000000092'
  );
  if coalesce((blocked_lease->>'acquired')::boolean,false) then
    raise exception 'ordinary background lease acquisition preempted a live holder';
  end if;
  takeover_lease:=timegenie.tracking_lease_takeover_for_timer_write(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000002',
    '49000000-0000-0000-0000-000000000093'
  );
  if not coalesce((takeover_lease->>'acquired')::boolean,false)
     or coalesce((takeover_lease->>'takeover')::boolean,true)
     or not coalesce((takeover_lease->>'compatibilityOnly')::boolean,false)
     or takeover_lease->>'holderDeviceId'<>'30000000-0000-0000-0000-000000000001' then
    raise exception 'legacy timer write takeover was not a non-mutating compatibility response';
  end if;
  perform timegenie.tracking_lease_renew(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '49000000-0000-0000-0000-000000000091'
  );
  perform timegenie.tracking_lease_release(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '49000000-0000-0000-0000-000000000091'
  );
end $$;
reset role;

-- Timer stop accepts writes without a lease. A future client timestamp is
-- clamped to server receipt time, while an offline stop in the past is kept.
set local role authenticated;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
do $$
declare observed_at timestamptz:=clock_timestamp(); observed_ms bigint; start_ms bigint; future_payload jsonb; offline_payload jsonb; result_value jsonb; saved_end timestamptz;
begin
  observed_ms:=floor(extract(epoch from observed_at)*1000)::bigint;
  start_ms:=observed_ms-60000;
  future_payload:=jsonb_build_object(
    'id','70000000-0000-0000-0000-000000000196','work_date',timegenie.workspace_work_date('20000000-0000-0000-0000-000000000001',observed_at),
    'kind','work','source_type','timer','state','ended','default_task_id','60000000-0000-0000-0000-000000000002',
    'label_snapshot','未来时钟收敛','started_at',start_ms,'ended_at',observed_ms+300000,'duration_seconds',360,
    'created_at',start_ms,'updated_at',observed_ms+300000,'version',1,'segments',jsonb_build_array(jsonb_build_object(
      'id','73000000-0000-0000-0000-000000000196','sequence_no',1,'started_at',start_ms,'ended_at',observed_ms+300000,'duration_seconds',360
    )),'allocations','[]'::jsonb
  );
  result_value:=timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000196','timer_stop','time_entry','70000000-0000-0000-0000-000000000196',null,1,1,future_payload
  );
  if not coalesce((result_value->>'accepted')::boolean,false) then raise exception 'timer stop without a lease was rejected: %',result_value; end if;
  select ended_at into saved_end from timegenie.time_entries where id='70000000-0000-0000-0000-000000000196';
  if saved_end>clock_timestamp()+interval '1 second' then raise exception 'future timer stop was not clamped: %',saved_end; end if;

  offline_payload:=future_payload||jsonb_build_object(
    'id','70000000-0000-0000-0000-000000000197','label_snapshot','离线结束保留',
    'ended_at',observed_ms-30000,'updated_at',observed_ms-30000,'duration_seconds',30,
    'segments',jsonb_build_array(jsonb_build_object('id','73000000-0000-0000-0000-000000000197','sequence_no',1,'started_at',start_ms,'ended_at',observed_ms-30000,'duration_seconds',30))
  );
  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000197','timer_stop','time_entry','70000000-0000-0000-0000-000000000197',null,1,1,offline_payload
  );
  if abs(extract(epoch from ((select ended_at from timegenie.time_entries where id='70000000-0000-0000-0000-000000000197')-to_timestamp((observed_ms-30000)::double precision/1000))))>0.001 then
    raise exception 'offline timer stop boundary was changed';
  end if;
end $$;
reset role;

-- Workspace epoch 1 accepts legacy clients, but epoch > 1 requires an exact
-- request header for every business write and returns reset-only pull data.
set local role authenticated;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
select set_config('request.headers','{"x-timegenie-client-schema-generation":"1"}',true);
do $$
declare legacy_snapshot jsonb;
begin
  legacy_snapshot:=timegenie.cloud_snapshot_get('20000000-0000-0000-0000-000000000001');
  if (legacy_snapshot->>'workspace_epoch')::bigint<>1 then
    raise exception 'epoch 1 legacy snapshot did not report the server epoch: %',legacy_snapshot;
  end if;
end $$;
select set_config('request.headers','{"x-timegenie-workspace-epoch":"1","x-timegenie-client-schema-generation":"1"}',true);
select timegenie.cloud_apply_patch(
  '20000000-0000-0000-0000-000000000001',
  '30000000-0000-0000-0000-000000000001',
  '4e000000-0000-0000-0000-000000000001',
  'app_setting.update','app_setting','workspace_epoch_probe',null,
  jsonb_build_object('key','workspace_epoch_probe','value_json','1'::jsonb,'updated_at',1792272000000,'version',1)
);
reset role;
update timegenie.workspaces set workspace_epoch=2 where id='20000000-0000-0000-0000-000000000001';
set local role authenticated;
select set_config('request.headers','{"x-timegenie-client-schema-generation":"1"}',true);
do $$
begin
  begin
    perform timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
      '4e000000-0000-0000-0000-000000000002','app_setting.update','app_setting','workspace_epoch_probe',1,
      jsonb_build_object('key','workspace_epoch_probe','value_json','2'::jsonb,'updated_at',1792272001000,'version',2)
    );
    raise exception 'epoch > 1 accepted a missing header';
  exception when others then
    if sqlerrm not like '%WORKSPACE_EPOCH_STALE%' then raise; end if;
  end;
end $$;
select set_config('request.headers','{"x-timegenie-workspace-epoch":"1","x-timegenie-client-schema-generation":"1"}',true);
do $$
declare retry integer;
begin
  for retry in 1..2 loop
    begin
      perform timegenie.cloud_apply_patch(
        '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
        '4e000000-0000-0000-0000-000000000003','app_setting.update','app_setting','workspace_epoch_probe',1,
        jsonb_build_object('key','workspace_epoch_probe','value_json','3'::jsonb,'updated_at',1792272002000,'version',2)
      );
      raise exception 'old epoch retry % was accepted',retry;
    exception when others then
      if sqlerrm not like '%WORKSPACE_EPOCH_STALE%' then raise; end if;
    end;
  end loop;
end $$;
select set_config('request.headers','{"x-timegenie-workspace-epoch":"2","x-timegenie-client-schema-generation":"1"}',true);
select timegenie.cloud_apply_patch(
  '20000000-0000-0000-0000-000000000001',
  '30000000-0000-0000-0000-000000000001',
  '4e000000-0000-0000-0000-000000000004',
  'app_setting.update','app_setting','workspace_epoch_probe',1,
  jsonb_build_object('key','workspace_epoch_probe','value_json','4'::jsonb,'updated_at',1792272003000,'version',2)
);
do $$
declare snapshot jsonb; mismatch jsonb;
begin
  snapshot:=timegenie.cloud_snapshot_get('20000000-0000-0000-0000-000000000001');
  if (snapshot->>'workspace_epoch')::bigint<>2 then
    raise exception 'snapshot omitted the current workspace epoch: %',snapshot;
  end if;
  perform set_config('request.headers','{"x-timegenie-workspace-epoch":"1","x-timegenie-client-schema-generation":"1"}',true);
  mismatch:=timegenie.cloud_incremental_pull(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',0,100
  );
  if not coalesce((mismatch->>'reset_required')::boolean,false)
     or (mismatch->>'workspace_epoch')::bigint<>2
     or jsonb_array_length(mismatch->'changes')<>0
     or jsonb_array_length(mismatch->'entities')<>0 then
    raise exception 'epoch mismatch pull was not reset-only: %',mismatch;
  end if;
end $$;
reset role;

-- A restore lock excludes ordinary writes, creates a safety snapshot, restores
-- atomically in place, increments epoch, emits reset, and is idempotent.
select set_config('request.headers','{"x-timegenie-workspace-epoch":"2","x-timegenie-client-schema-generation":"1"}',true);
update timegenie.time_entries set state='ended',ended_at=coalesce(ended_at,updated_at),duration_seconds=greatest(duration_seconds,0)
where workspace_id='20000000-0000-0000-0000-000000000001' and state in('running','paused');
update timegenie.time_segments set ended_at=coalesce(ended_at,started_at),duration_seconds=greatest(duration_seconds,0)
where workspace_id='20000000-0000-0000-0000-000000000001' and ended_at is null;
update timegenie.unassigned_sessions set state='resolved',resolution_type='discard',resolved_at=coalesce(resolved_at,updated_at)
where workspace_id='20000000-0000-0000-0000-000000000001' and state in('collecting','paused','awaiting_resolution');
set local role authenticated;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
do $$
declare target_id uuid; prepared jsonb; direct_prepared jsonb; restored jsonb; repeated jsonb; before_task_count bigint; restored_snapshot jsonb; active_unassigned_id uuid; pre_restore_count bigint;
begin
  target_id:=(timegenie.workspace_snapshot_create(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '4f000000-0000-0000-0000-000000000001','manual','回滚协议目标',null,false
  )->>'id')::uuid;
  before_task_count:=jsonb_array_length(timegenie.cloud_snapshot_get('20000000-0000-0000-0000-000000000001')->'tasks');
  reset role;
  select id into active_unassigned_id from timegenie.unassigned_sessions
  where workspace_id='20000000-0000-0000-0000-000000000001' and state in('collecting','paused','awaiting_resolution') limit 1;
  if active_unassigned_id is null then
    insert into timegenie.unassigned_sessions(
      workspace_id,work_date,state,threshold_seconds,duration_seconds,first_started_at,shared_source,migration_state
    ) values(
      '20000000-0000-0000-0000-000000000001',current_date,'collecting',300,0,now(),'cloud','adopted'
    ) returning id into active_unassigned_id;
  end if;
  set local role authenticated;
  perform set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '4f000000-0000-0000-0000-000000000002','app_setting.update','app_setting','workspace_epoch_probe',2,
    jsonb_build_object('key','workspace_epoch_probe','value_json','99'::jsonb,'updated_at',1792272100000,'version',3)
  );
  reset role;
  select count(*) into pre_restore_count from timegenie.workspace_snapshots
  where workspace_id='20000000-0000-0000-0000-000000000001' and snapshot_type='pre_restore';
  set local role authenticated;
  perform set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
  direct_prepared:=timegenie.workspace_restore_prepare(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',target_id,2,false
  );
  if direct_prepared->>'safetySnapshotId' is not null then raise exception 'direct restore unexpectedly created a safety snapshot: %',direct_prepared; end if;
  reset role;
  if (select count(*) from timegenie.workspace_snapshots where workspace_id='20000000-0000-0000-0000-000000000001' and snapshot_type='pre_restore')<>pre_restore_count then raise exception 'direct restore changed safety snapshot count'; end if;
  set local role authenticated;
  perform set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
  perform timegenie.workspace_restore_cancel(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',(direct_prepared->>'lockToken')::uuid
  );
  prepared:=timegenie.workspace_restore_prepare(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',target_id,2,true
  );
  if prepared->>'safetySnapshotId' is null or prepared->>'lockToken' is null then raise exception 'restore prepare omitted safety or lock metadata: %',prepared; end if;
  begin
    perform timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
      '4f000000-0000-0000-0000-000000000003','app_setting.update','app_setting','restore_lock_probe',null,
      jsonb_build_object('key','restore_lock_probe','value_json','true'::jsonb,'updated_at',1792272101000,'version',1)
    );
    raise exception 'ordinary write bypassed restore lock';
  exception when others then
    if sqlerrm not like '%WORKSPACE_RESTORE_IN_PROGRESS%' then raise; end if;
  end;
  perform set_config('request.headers','{"x-timegenie-workspace-epoch":"2","x-timegenie-client-schema-generation":"1"}',true);
  restored:=timegenie.workspace_restore_commit(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    (prepared->>'lockToken')::uuid,target_id,prepared->>'targetContentHash',(prepared->>'preparedEpoch')::bigint,
    '4f000000-0000-0000-0000-000000000004',true
  );
  repeated:=timegenie.workspace_restore_commit(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    (prepared->>'lockToken')::uuid,target_id,prepared->>'targetContentHash',(prepared->>'preparedEpoch')::bigint,
    '4f000000-0000-0000-0000-000000000004',true
  );
  if restored is distinct from repeated or (restored->>'workspaceEpoch')::bigint<>3 then raise exception 'restore commit was not idempotent or did not advance epoch: % / %',restored,repeated; end if;
  perform set_config('request.headers','{"x-timegenie-workspace-epoch":"3","x-timegenie-client-schema-generation":"1"}',true);
  restored_snapshot:=timegenie.cloud_snapshot_get('20000000-0000-0000-0000-000000000001');
  if not exists(select 1 from jsonb_array_elements(restored_snapshot->'app_settings') value where value->>'key'='workspace_epoch_probe' and value->'value_json'='4'::jsonb) then raise exception 'restore did not recover target business value'; end if;
  if before_task_count<>jsonb_array_length(restored_snapshot->'tasks') then raise exception 'restore changed target task count'; end if;
  if exists(select 1 from jsonb_array_elements(restored_snapshot->'unassigned_sessions') value where value->>'state'='paused') then raise exception 'restore wrote invalid paused unassigned state'; end if;
  if exists(select 1 from jsonb_array_elements(restored_snapshot->'unassigned_sessions') value where value->>'state'='collecting') then raise exception 'restore resumed an active unassigned collection'; end if;
  perform set_config('timegenie.restore_test_safety_id',restored->>'safetySnapshotId',true);
end $$;
reset role;
do $$ begin
  if exists(select 1 from timegenie.workspace_restore_locks where workspace_id='20000000-0000-0000-0000-000000000001') then raise exception 'restore lock was not released'; end if;
  if not exists(select 1 from timegenie.workspace_snapshots where id=current_setting('timegenie.restore_test_safety_id')::uuid and snapshot_type='pre_restore' and status='ready') then raise exception 'restore safety snapshot missing'; end if;
  if not exists(select 1 from timegenie.workspace_changes where workspace_id='20000000-0000-0000-0000-000000000001' and entity_type='workspace_reset' and entity_version=3) then raise exception 'workspace reset event missing'; end if;
  if not exists(select 1 from timegenie.cloud_operation_logs where workspace_id='20000000-0000-0000-0000-000000000001' and operation_id='4f000000-0000-0000-0000-000000000004' and source='restore') then raise exception 'restore operation history missing'; end if;
end $$;

select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
select set_config('request.headers','{"x-timegenie-workspace-epoch":"3","x-timegenie-client-schema-generation":"1"}',true);
do $$
declare target_task uuid; target_subject uuid; snapshot jsonb; adjustment_id uuid := '4f000000-0000-0000-0000-000000000099';
begin
  select id, subject_id into target_task, target_subject from timegenie.tasks
  where workspace_id='20000000-0000-0000-0000-000000000001' and deleted_at is null order by id limit 1;
  if target_task is null then raise exception 'actual adjustment snapshot test has no task'; end if;
  insert into timegenie.actual_time_adjustments(
    id,workspace_id,subject_id,task_id,work_date,delta_minutes,baseline_minutes,target_minutes,source_type,batch_id,created_at,updated_at,version
  ) values(
    adjustment_id,'20000000-0000-0000-0000-000000000001',target_subject,target_task,'2026-10-21',15,0,15,'quick_actual_adjustment',gen_random_uuid(),now(),now(),1
  ) on conflict(id) do nothing;
  snapshot:=timegenie.cloud_snapshot_get('20000000-0000-0000-0000-000000000001');
  if not (snapshot ? 'actual_time_adjustments') then raise exception 'cloud snapshot omitted actual_time_adjustments'; end if;
  if not exists(select 1 from jsonb_array_elements(snapshot->'actual_time_adjustments') value where value->>'id'=adjustment_id::text and (value->>'target_minutes')::integer=15) then
    raise exception 'cloud snapshot did not retain the saved actual time adjustment';
  end if;
  if timegenie.cloud_incremental_entity_get('20000000-0000-0000-0000-000000000001','actual_time_adjustments',adjustment_id::text)->'data' is null then
    raise exception 'incremental entity get reported an existing actual time adjustment as deleted';
  end if;
end $$;
reset role;

\ir 20261021c_timegenie_daily_report_collection_templates.sql
\ir 20261021c_timegenie_daily_report_collection_templates.sql

do $$
begin
  if exists(
    select 1 from timegenie.report_templates
    where is_builtin and report_type in ('weekly','monthly') and deleted_at is null
      and content not like '%{{日报合集}}%'
  ) then raise exception 'weekly/monthly built-in templates must aggregate saved daily reports'; end if;
end $$;

select 'Supabase schema integration tests passed' as result;

rollback;
