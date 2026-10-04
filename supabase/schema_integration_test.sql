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
  if not has_function_privilege('authenticated', 'timegenie.cloud_apply_patch(uuid, uuid, uuid, text, text, text, bigint, jsonb)', 'execute') then
    raise exception 'authenticated is missing cloud apply RPC execute privilege';
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
  if has_function_privilege('authenticated', 'timegenie.unassigned_resolve_non_work(uuid, uuid, uuid, bigint, uuid, text)', 'execute') then
    raise exception 'authenticated can execute internal unassigned helper';
  end if;
end $$;

begin;

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

select 'Supabase schema integration tests passed' as result;

rollback;
