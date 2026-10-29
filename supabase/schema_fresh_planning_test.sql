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
select set_config('request.headers', '{"x-timegenie-client-schema-generation":"1"}', true);

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

select timegenie.direct_cloud_task_daily_estimate_set(
  '21000000-0000-0000-0000-000000000001',
  '31000000-0000-0000-0000-000000000001',
  '41000000-0000-0000-0000-000000000001',
  1,
  jsonb_build_object(
    'taskId', '61000000-0000-0000-0000-000000000001',
    'workDate', current_date,
    'estimateMinutes', 45,
    'expectedVersion', null
  )
);

select timegenie.direct_cloud_recurrence_save(
  '21000000-0000-0000-0000-000000000001',
  '31000000-0000-0000-0000-000000000001',
  '41000000-0000-0000-0000-000000000002',
  1,
  jsonb_build_object(
    'taskId', '61000000-0000-0000-0000-000000000001',
    'taskExpectedVersion', 1,
    'frequency', 'weekly',
    'weekdaysMask', 21,
    'effectiveStart', current_date,
    'ruleExpectedVersion', null
  )
);

reset role;

do $$
begin
  if (select estimate_minutes from timegenie.task_daily_estimates
      where task_id = '61000000-0000-0000-0000-000000000001'
        and work_date = current_date) <> 45 then
    raise exception 'fresh schema did not apply daily estimate';
  end if;
  if not exists(
    select 1 from timegenie.task_recurrence_rules
    where task_id = '61000000-0000-0000-0000-000000000001'
      and frequency = 'weekly' and weekdays_mask = 21
      and effective_start=current_date and effective_end is null
  ) then
    raise exception 'fresh schema did not apply recurrence rule';
  end if;
  if not exists(
    select 1 from timegenie.workspace_changes
    where workspace_id = '21000000-0000-0000-0000-000000000001'
      and entity_type = 'task_daily_estimates'
      and entity_id = '61000000-0000-0000-0000-000000000001|'||current_date::text
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
