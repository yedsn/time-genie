\set ON_ERROR_STOP on

-- Smoke-test the current source-of-truth schema without applying any upgrade
-- patches. This runs in a separate disposable database from the upgrade-path
-- integration test.
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

\ir schema.sql

begin;

insert into auth.users(id) values ('11000000-0000-0000-0000-000000000001');
insert into timegenie.workspaces(id, owner_user_id, name) values
  ('21000000-0000-0000-0000-000000000001', '11000000-0000-0000-0000-000000000001', '全新结构规划测试');
insert into timegenie.devices(id, workspace_id, device_name, platform, app_version) values
  ('31000000-0000-0000-0000-000000000001', '21000000-0000-0000-0000-000000000001', '全新结构设备', 'windows', '0.2.1');
insert into timegenie.subjects(id, workspace_id, name, sort_order) values
  ('51000000-0000-0000-0000-000000000001', '21000000-0000-0000-0000-000000000001', '规划', 1);
insert into timegenie.tasks(id, workspace_id, subject_id, title, status, source_type, sort_order, version) values
  ('61000000-0000-0000-0000-000000000001', '21000000-0000-0000-0000-000000000001', '51000000-0000-0000-0000-000000000001', '全新结构事项', 'open', 'manual', 1, 1);

set local role authenticated;
select set_config('request.jwt.claim.sub', '11000000-0000-0000-0000-000000000001', true);

select timegenie.cloud_apply_patch(
  '21000000-0000-0000-0000-000000000001',
  '31000000-0000-0000-0000-000000000001',
  '41000000-0000-0000-0000-000000000001',
  'task_daily_estimate.set',
  'task_daily_estimate',
  '61000000-0000-0000-0000-000000000001|2026-10-05',
  null,
  jsonb_build_object(
    'task_id', '61000000-0000-0000-0000-000000000001',
    'work_date', '2026-10-05',
    'estimate_minutes', 45,
    'created_at', 1791158400000,
    'updated_at', 1791158400000,
    'version', 1
  )
);

select timegenie.cloud_apply_patch(
  '21000000-0000-0000-0000-000000000001',
  '31000000-0000-0000-0000-000000000001',
  '41000000-0000-0000-0000-000000000002',
  'task_recurrence.save',
  'task_recurrence_rule',
  '61000000-0000-0000-0000-000000000001',
  null,
  jsonb_build_object(
    'action', 'save',
    'task_id', '61000000-0000-0000-0000-000000000001',
    'task_expected_version', 1,
    'task_version', 2,
    'rule_id', '91000000-0000-0000-0000-000000000001',
    'frequency', 'weekly',
    'weekdays_mask', 21,
    'effective_start', '2026-10-05',
    'version', 1,
    'rules', jsonb_build_array(jsonb_build_object(
      'id', '91000000-0000-0000-0000-000000000001',
      'task_id', '61000000-0000-0000-0000-000000000001',
      'frequency', 'weekly',
      'weekdays_mask', 21,
      'effective_start', '2026-10-05',
      'effective_end', null,
      'created_at', 1791158400000,
      'updated_at', 1791158400000,
      'version', 1
    ))
  )
);

reset role;

do $$
begin
  if (select estimate_minutes from timegenie.task_daily_estimates
      where task_id = '61000000-0000-0000-0000-000000000001'
        and work_date = '2026-10-05') <> 45 then
    raise exception 'fresh schema did not apply daily estimate';
  end if;
  if not exists(
    select 1 from timegenie.task_recurrence_rules
    where id = '91000000-0000-0000-0000-000000000001'
      and frequency = 'weekly' and weekdays_mask = 21 and effective_end is null
  ) then
    raise exception 'fresh schema did not apply recurrence rule';
  end if;
  if not exists(
    select 1 from timegenie.workspace_changes
    where workspace_id = '21000000-0000-0000-0000-000000000001'
      and entity_type = 'task_daily_estimates'
      and entity_id = '61000000-0000-0000-0000-000000000001|2026-10-05'
  ) then
    raise exception 'fresh schema did not preserve daily estimate composite change identity';
  end if;
  if not exists(
    select 1 from timegenie.workspace_changes
    where workspace_id = '21000000-0000-0000-0000-000000000001'
      and entity_type = 'task_recurrence_rules'
      and entity_id = '61000000-0000-0000-0000-000000000001'
  ) then
    raise exception 'fresh schema did not preserve recurrence task change identity';
  end if;
end $$;

rollback;
