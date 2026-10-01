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

-- Reapply the idempotent schema after creating the Data API roles so this test
-- exercises the same explicit grants and revokes used by a real Supabase project.
\ir schema.sql

do $$
begin
  if has_table_privilege('anon', 'public.workspaces', 'select') then
    raise exception 'anon can read business tables';
  end if;
  if has_function_privilege('anon', 'public.cloud_snapshot_get(uuid)', 'execute') then
    raise exception 'anon can execute protected RPCs';
  end if;
  if not has_table_privilege('authenticated', 'public.workspaces', 'select,insert,update,delete') then
    raise exception 'authenticated is missing business table privileges';
  end if;
  if not has_function_privilege('authenticated', 'public.cloud_snapshot_get(uuid)', 'execute') then
    raise exception 'authenticated is missing RPC execute privilege';
  end if;
end $$;

begin;

insert into auth.users(id) values
  ('10000000-0000-0000-0000-000000000001'),
  ('10000000-0000-0000-0000-000000000002');

set local role authenticated;
select set_config('request.jwt.claim.sub', '10000000-0000-0000-0000-000000000001', true);

insert into public.workspaces(id, owner_user_id, name) values
  ('20000000-0000-0000-0000-000000000001', '10000000-0000-0000-0000-000000000001', '账号 A 工作空间');

select public.workspace_initialize_defaults('20000000-0000-0000-0000-000000000001');
select public.workspace_initialize_defaults('20000000-0000-0000-0000-000000000001');

do $$
begin
  if (select count(*) from public.subjects where workspace_id = '20000000-0000-0000-0000-000000000001') <> 1 then
    raise exception 'workspace defaults created duplicate subjects';
  end if;
  if (select count(*) from public.app_settings where workspace_id = '20000000-0000-0000-0000-000000000001') <> 5 then
    raise exception 'workspace defaults created duplicate settings';
  end if;
  if (select count(*) from public.report_templates where workspace_id = '20000000-0000-0000-0000-000000000001') <> 3 then
    raise exception 'workspace defaults created duplicate templates';
  end if;
end $$;

insert into public.devices(id, workspace_id, device_name, platform, app_version) values
  ('30000000-0000-0000-0000-000000000001', '20000000-0000-0000-0000-000000000001', '设备 A', 'windows', '0.1.0'),
  ('30000000-0000-0000-0000-000000000002', '20000000-0000-0000-0000-000000000001', '设备 B', 'windows', '0.1.0');

-- Security-definer RPCs must reject unknown or revoked device identities even
-- when the authenticated user owns the workspace.
insert into public.devices(id, workspace_id, device_name, platform, app_version, revoked_at) values
  ('30000000-0000-0000-0000-000000000098', '20000000-0000-0000-0000-000000000001', '已撤销设备', 'windows', '0.1.0', now());

do $$
begin
  if public.is_registered_device(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000098'
  ) then
    raise exception 'revoked device was considered registered';
  end if;

  begin
    perform public.tracking_lease_acquire(
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
    perform public.timer_start(
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
    perform public.unassigned_resolve_work(
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
end $$;

-- A different authenticated user cannot read or write account A's workspace.
select set_config('request.jwt.claim.sub', '10000000-0000-0000-0000-000000000002', true);
do $$
begin
  if (select count(*) from public.workspaces where id = '20000000-0000-0000-0000-000000000001') <> 0 then
    raise exception 'RLS exposed another user workspace';
  end if;
  if (select count(*) from public.subjects where workspace_id = '20000000-0000-0000-0000-000000000001') <> 0 then
    raise exception 'RLS exposed another user subjects';
  end if;
  begin
    insert into public.devices(id, workspace_id, device_name, platform, app_version) values
      ('30000000-0000-0000-0000-000000000099', '20000000-0000-0000-0000-000000000001', '越权设备', 'windows', '0.1.0');
    raise exception 'RLS allowed a device in another user workspace';
  exception
    when insufficient_privilege then null;
  end;
end $$;

select set_config('request.jwt.claim.sub', '10000000-0000-0000-0000-000000000001', true);

-- Replace generated defaults with a complete local snapshot containing a task
-- tree, one time entry, one report, and their relationships.
select public.migration_import_snapshot(
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
      'created_at', 1789695000000,
      'updated_at', 1789695000000,
      'version', 1
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

-- Retrying the same operation is idempotent even though the target is no
-- longer empty. A new migration operation must be rejected.
select public.migration_import_snapshot(
  '20000000-0000-0000-0000-000000000001',
  '40000000-0000-0000-0000-000000000001',
  '30000000-0000-0000-0000-000000000001',
  '{}'::jsonb
);

do $$
begin
  if (select count(*) from public.tasks where workspace_id = '20000000-0000-0000-0000-000000000001') <> 2 then
    raise exception 'migration did not preserve the task tree';
  end if;
  if (select parent_id from public.tasks where id = '60000000-0000-0000-0000-000000000002') <> '60000000-0000-0000-0000-000000000001' then
    raise exception 'migration did not restore the task parent';
  end if;
  if (select count(*) from public.time_entries where id = '70000000-0000-0000-0000-000000000001') <> 1 then
    raise exception 'migration did not import the time entry';
  end if;
  if (select count(*) from public.reports where id = '80000000-0000-0000-0000-000000000001') <> 1 then
    raise exception 'migration did not import the report';
  end if;
  if jsonb_typeof((select value_json from public.app_settings where workspace_id = '20000000-0000-0000-0000-000000000001' and key = 'salary_hourly_rate')) <> 'number' then
    raise exception 'migration changed numeric setting type';
  end if;
  if (select value_json #>> '{}' from public.app_settings where workspace_id = '20000000-0000-0000-0000-000000000001' and key = 'timezone') <> 'Asia/Shanghai' then
    raise exception 'migration changed string setting value';
  end if;
  if jsonb_typeof((select config_json from public.integration_configs where workspace_id = '20000000-0000-0000-0000-000000000001' and provider = 'seatable')) <> 'object' then
    raise exception 'migration changed integration config type';
  end if;
  begin
    perform public.migration_import_snapshot(
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
declare before_seq bigint;
begin
  select max(change_seq) into before_seq from public.workspace_changes
  where workspace_id = '20000000-0000-0000-0000-000000000001';

  perform public.cloud_apply_patch(
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

  if (select max(change_seq) from public.workspace_changes where workspace_id = '20000000-0000-0000-0000-000000000001') <= before_seq then
    raise exception 'workspace change sequence did not advance';
  end if;

  begin
    perform public.cloud_apply_patch(
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
end $$;

-- Only one global timer may exist. Either registered device can control the
-- shared timer when it supplies the current optimistic version.
do $$
declare timer_row jsonb;
begin
  timer_row := public.timer_start(
    '20000000-0000-0000-0000-000000000001',
    '60000000-0000-0000-0000-000000000002',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000020',
    '设备 A 启动'
  );

  begin
    perform public.timer_start(
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
    perform public.timer_pause(
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

  timer_row := public.timer_pause(
    '20000000-0000-0000-0000-000000000001',
    (timer_row->>'id')::uuid,
    '30000000-0000-0000-0000-000000000002',
    (timer_row->>'version')::bigint,
    '40000000-0000-0000-0000-000000000023'
  );
  timer_row := public.timer_resume(
    '20000000-0000-0000-0000-000000000001',
    (timer_row->>'id')::uuid,
    '30000000-0000-0000-0000-000000000002',
    (timer_row->>'version')::bigint,
    '40000000-0000-0000-0000-000000000024'
  );
  timer_row := public.timer_stop(
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
  lease_result := public.tracking_lease_acquire(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '90000000-0000-0000-0000-000000000001'
  );
  if not (lease_result->>'acquired')::boolean then raise exception 'device A did not acquire lease'; end if;

  lease_result := public.tracking_lease_acquire(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000002',
    '90000000-0000-0000-0000-000000000002'
  );
  if (lease_result->>'acquired')::boolean then raise exception 'device B stole a live lease'; end if;

  begin
    perform public.tracking_lease_renew(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000001',
      '90000000-0000-0000-0000-000000000099'
    );
    raise exception 'wrong lease token was renewed';
  exception
    when others then
      if sqlerrm not like '%LEASE_EXPIRED%' then raise; end if;
  end;

  update public.tracking_leases set expires_at = now() - interval '1 second'
  where workspace_id = '20000000-0000-0000-0000-000000000001';

  lease_result := public.tracking_lease_acquire(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000002',
    '90000000-0000-0000-0000-000000000002'
  );
  if not (lease_result->>'acquired')::boolean then raise exception 'device B did not acquire expired lease'; end if;

  begin
    perform public.unassigned_tick(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000001',
      '90000000-0000-0000-0000-000000000001'
    );
    raise exception 'expired holder advanced unassigned time';
  exception
    when others then
      if sqlerrm not like '%LEASE_EXPIRED%' then raise; end if;
  end;

  perform public.unassigned_tick(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000002',
    '90000000-0000-0000-0000-000000000002'
  );

  if (
    select count(*)
    from public.unassigned_segments segment
    join public.unassigned_sessions session on session.id = segment.session_id
    where session.workspace_id = '20000000-0000-0000-0000-000000000001'
      and segment.ended_at is null
  ) <> 1 then
    raise exception 'background collector has duplicate open segments';
  end if;
end $$;

reset role;

do $$
begin
  if not exists(
    select 1
    from pg_publication_tables
    where pubname = 'supabase_realtime'
      and schemaname = 'public'
      and tablename = 'workspace_changes'
  ) then
    raise exception 'workspace_changes is not in the realtime publication';
  end if;
end $$;

select 'Supabase schema integration tests passed' as result;

rollback;
