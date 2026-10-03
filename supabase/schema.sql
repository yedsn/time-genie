-- TimeGenie Supabase schema.
-- Run this file in a Supabase project with the anon key only in the desktop client.
-- The Supabase Data API/PostgREST exposed schemas must include `timegenie`.

create schema if not exists extensions;
create schema if not exists timegenie;
create extension if not exists pgcrypto with schema extensions;
alter extension pgcrypto set schema extensions;

create table if not exists timegenie.workspaces (
  id uuid primary key default gen_random_uuid(),
  owner_user_id uuid not null references auth.users(id) on delete cascade,
  name text not null default '我的工作台',
  timezone text not null default 'Asia/Shanghai',
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  version bigint not null default 1,
  deleted_at timestamptz
);
create unique index if not exists uq_workspaces_owner_active
  on timegenie.workspaces(owner_user_id) where deleted_at is null;

create table if not exists timegenie.devices (
  id uuid primary key,
  workspace_id uuid not null references timegenie.workspaces(id) on delete cascade,
  device_name text not null,
  platform text not null,
  app_version text not null,
  last_seen_at timestamptz not null default now(),
  created_at timestamptz not null default now(),
  revoked_at timestamptz
);
create index if not exists idx_devices_workspace on timegenie.devices(workspace_id, revoked_at);

create table if not exists timegenie.tracking_leases (
  workspace_id uuid primary key references timegenie.workspaces(id) on delete cascade,
  holder_device_id uuid references timegenie.devices(id),
  lease_token uuid,
  expires_at timestamptz,
  updated_at timestamptz not null default now(),
  version bigint not null default 1
);

create table if not exists timegenie.workspace_changes (
  change_seq bigint generated always as identity primary key,
  workspace_id uuid not null references timegenie.workspaces(id) on delete cascade,
  entity_type text not null,
  entity_id uuid not null,
  operation text not null check (operation in ('insert', 'update', 'delete')),
  entity_version bigint not null,
  changed_by_device_id uuid references timegenie.devices(id),
  changed_at timestamptz not null default now()
);
create index if not exists idx_workspace_changes_pull on timegenie.workspace_changes(workspace_id, change_seq);

create table if not exists timegenie.processed_operations (
  workspace_id uuid not null references timegenie.workspaces(id) on delete cascade,
  operation_id uuid not null,
  device_id uuid not null references timegenie.devices(id),
  operation_type text not null,
  request_hash text,
  result_json jsonb not null,
  processed_at timestamptz not null default now(),
  primary key (workspace_id, operation_id)
);
alter table timegenie.processed_operations add column if not exists request_hash text;

create table if not exists timegenie.subjects (
  id uuid primary key default gen_random_uuid(),
  workspace_id uuid not null references timegenie.workspaces(id) on delete cascade,
  name text not null check (length(trim(name)) > 0),
  sort_order integer not null default 0,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  version bigint not null default 1,
  created_by_device_id uuid references timegenie.devices(id),
  updated_by_device_id uuid references timegenie.devices(id),
  deleted_at timestamptz
);
create unique index if not exists uq_subjects_active_name on timegenie.subjects(workspace_id, lower(name)) where deleted_at is null;

create table if not exists timegenie.work_days (
  workspace_id uuid not null references timegenie.workspaces(id) on delete cascade,
  work_date date not null,
  timezone text not null default 'Asia/Shanghai',
  work_period_text text,
  note text,
  settled_at timestamptz,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  version bigint not null default 1,
  primary key(workspace_id, work_date)
);

create table if not exists timegenie.app_settings (
  workspace_id uuid not null references timegenie.workspaces(id) on delete cascade,
  key text not null,
  value_json jsonb not null,
  updated_at timestamptz not null default now(),
  version bigint not null default 1,
  primary key(workspace_id, key)
);

create table if not exists timegenie.tasks (
  id uuid primary key default gen_random_uuid(),
  workspace_id uuid not null references timegenie.workspaces(id) on delete cascade,
  subject_id uuid not null references timegenie.subjects(id),
  parent_id uuid references timegenie.tasks(id),
  title text not null check (length(trim(title)) > 0),
  status text not null default 'open' check (status in ('open', 'done')),
  planned_date date,
  planned_time time,
  estimate_minutes integer check (estimate_minutes is null or estimate_minutes > 0),
  note text,
  project_name text,
  solution_name text,
  source_type text not null check (source_type in ('manual', 'obsidian_import', 'timer_quick_create', 'migration')),
  source_ref text,
  sort_order integer not null,
  completed_at timestamptz,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  version bigint not null default 1,
  created_by_device_id uuid references timegenie.devices(id),
  updated_by_device_id uuid references timegenie.devices(id),
  deleted_at timestamptz,
  check ((status = 'done' and completed_at is not null) or status = 'open')
);
create index if not exists idx_tasks_tree on timegenie.tasks(workspace_id, subject_id, parent_id, sort_order);

create table if not exists timegenie.task_status_events (
  id uuid primary key default gen_random_uuid(),
  workspace_id uuid not null references timegenie.workspaces(id) on delete cascade,
  task_id uuid not null references timegenie.tasks(id),
  status text not null check (status in ('open', 'done')),
  occurred_at timestamptz not null default now(),
  source_type text not null check (source_type in ('user', 'import', 'migration')),
  created_at timestamptz not null default now()
);

create table if not exists timegenie.task_daily_estimates (
  workspace_id uuid not null references timegenie.workspaces(id) on delete cascade,
  task_id uuid not null references timegenie.tasks(id) on delete cascade,
  work_date date not null,
  estimate_minutes integer not null check (estimate_minutes > 0),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  version bigint not null default 1,
  primary key(task_id, work_date)
);
create index if not exists idx_task_daily_estimates_date on timegenie.task_daily_estimates(workspace_id, work_date, task_id);

create table if not exists timegenie.task_recurrence_rules (
  id uuid primary key default gen_random_uuid(),
  workspace_id uuid not null references timegenie.workspaces(id) on delete cascade,
  task_id uuid not null references timegenie.tasks(id) on delete cascade,
  frequency text not null check (frequency in ('daily', 'weekdays', 'weekly')),
  weekdays_mask integer check (weekdays_mask is null or weekdays_mask between 1 and 127),
  effective_start date not null,
  effective_end date,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  version bigint not null default 1,
  check (frequency = 'weekly' or weekdays_mask is null),
  check (frequency <> 'weekly' or weekdays_mask is not null),
  check (effective_end is null or effective_end >= effective_start),
  unique(task_id, effective_start)
);
create unique index if not exists uq_task_recurrence_rules_open on timegenie.task_recurrence_rules(task_id) where effective_end is null;
create index if not exists idx_task_recurrence_rules_workspace_dates on timegenie.task_recurrence_rules(workspace_id, effective_start, effective_end, task_id);
create index if not exists idx_task_recurrence_rules_task_dates on timegenie.task_recurrence_rules(task_id, effective_start, effective_end);

create table if not exists timegenie.task_occurrences (
  workspace_id uuid not null references timegenie.workspaces(id) on delete cascade,
  task_id uuid not null references timegenie.tasks(id) on delete cascade,
  occurrence_date date not null,
  origin text not null check (origin in ('scheduled', 'manual')),
  status text not null default 'open' check (status in ('open', 'done')),
  completed_at timestamptz,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  version bigint not null default 1,
  primary key(task_id, occurrence_date),
  check ((status = 'done' and completed_at is not null) or status = 'open')
);
create index if not exists idx_task_occurrences_workspace_date on timegenie.task_occurrences(workspace_id, occurrence_date, status, task_id);

create table if not exists timegenie.time_entries (
  id uuid primary key default gen_random_uuid(),
  workspace_id uuid not null references timegenie.workspaces(id) on delete cascade,
  work_date date not null,
  kind text not null check (kind in ('work', 'break')),
  source_type text not null check (source_type in ('timer', 'manual', 'unassigned')),
  state text not null check (state in ('running', 'paused', 'ended')),
  default_task_id uuid references timegenie.tasks(id),
  label_snapshot text not null,
  started_at timestamptz not null,
  ended_at timestamptz,
  duration_seconds bigint not null default 0 check (duration_seconds >= 0),
  note text,
  origin_unassigned_session_id uuid unique,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  version bigint not null default 1,
  created_by_device_id uuid references timegenie.devices(id),
  updated_by_device_id uuid references timegenie.devices(id),
  deleted_at timestamptz,
  check ((state = 'ended' and ended_at is not null) or (state in ('running', 'paused') and ended_at is null))
);
create unique index if not exists uq_time_entries_active on timegenie.time_entries(workspace_id) where state in ('running', 'paused') and deleted_at is null;

create table if not exists timegenie.time_segments (
  id uuid primary key default gen_random_uuid(),
  workspace_id uuid not null references timegenie.workspaces(id) on delete cascade,
  entry_id uuid not null references timegenie.time_entries(id) on delete cascade,
  sequence_no integer not null,
  started_at timestamptz not null,
  ended_at timestamptz,
  duration_seconds bigint not null default 0,
  unique(entry_id, sequence_no)
);

create table if not exists timegenie.time_allocations (
  id uuid primary key default gen_random_uuid(),
  workspace_id uuid not null references timegenie.workspaces(id) on delete cascade,
  entry_id uuid not null references timegenie.time_entries(id) on delete cascade,
  task_id uuid not null references timegenie.tasks(id),
  minutes integer not null check (minutes > 0),
  note text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  version bigint not null default 1,
  unique(entry_id, task_id)
);

create table if not exists timegenie.unassigned_sessions (
  id uuid primary key default gen_random_uuid(),
  workspace_id uuid not null references timegenie.workspaces(id) on delete cascade,
  work_date date not null,
  state text not null check (state in ('collecting', 'awaiting_resolution', 'resolved', 'discarded')),
  threshold_seconds integer not null default 300,
  duration_seconds bigint not null default 0,
  first_started_at timestamptz not null,
  last_ended_at timestamptz,
  prompted_at timestamptz,
  resolution_type text,
  generated_entry_id uuid references timegenie.time_entries(id),
  resolved_at timestamptz,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  version bigint not null default 1
);
create unique index if not exists uq_unassigned_active on timegenie.unassigned_sessions(workspace_id) where state in ('collecting', 'awaiting_resolution');

create table if not exists timegenie.unassigned_segments (
  id uuid primary key default gen_random_uuid(),
  workspace_id uuid not null references timegenie.workspaces(id) on delete cascade,
  session_id uuid not null references timegenie.unassigned_sessions(id) on delete cascade,
  sequence_no integer not null,
  started_at timestamptz not null,
  ended_at timestamptz,
  duration_seconds bigint not null default 0,
  lease_token uuid,
  unique(session_id, sequence_no)
);

create table if not exists timegenie.report_templates (
  id uuid primary key default gen_random_uuid(),
  workspace_id uuid not null references timegenie.workspaces(id) on delete cascade,
  report_type text not null check (report_type in ('daily', 'weekly', 'monthly')),
  subject_id uuid references timegenie.subjects(id),
  content text not null,
  is_builtin boolean not null default false,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  version bigint not null default 1,
  deleted_at timestamptz
);
create unique index if not exists uq_report_templates_scope on timegenie.report_templates(workspace_id, report_type, coalesce(subject_id, '00000000-0000-0000-0000-000000000000'::uuid)) where deleted_at is null;

create table if not exists timegenie.reports (
  id uuid primary key default gen_random_uuid(),
  workspace_id uuid not null references timegenie.workspaces(id) on delete cascade,
  report_type text not null check (report_type in ('daily', 'weekly', 'monthly')),
  subject_id uuid not null references timegenie.subjects(id),
  period_start date not null,
  period_end date not null,
  reference_date date not null,
  template_id uuid references timegenie.report_templates(id),
  markdown_content text not null,
  content_source text not null check (content_source in ('generated', 'edited')),
  generation_count integer not null default 1,
  input_revision_hash text,
  generated_at timestamptz,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  version bigint not null default 1,
  created_by_device_id uuid references timegenie.devices(id),
  updated_by_device_id uuid references timegenie.devices(id),
  deleted_at timestamptz,
  check(period_start <= period_end)
);
create unique index if not exists uq_reports_active_period on timegenie.reports(workspace_id, report_type, subject_id, period_start, period_end) where deleted_at is null;

create table if not exists timegenie.report_tasks (
  report_id uuid not null references timegenie.reports(id) on delete cascade,
  task_id uuid not null references timegenie.tasks(id),
  workspace_id uuid not null references timegenie.workspaces(id) on delete cascade,
  sort_order integer not null,
  title_snapshot text not null,
  path_snapshot text not null,
  primary key(report_id, task_id)
);

create table if not exists timegenie.integration_configs (
  workspace_id uuid not null references timegenie.workspaces(id) on delete cascade,
  provider text not null,
  enabled boolean not null default false,
  config_json jsonb not null default '{}'::jsonb,
  updated_at timestamptz not null default now(),
  version bigint not null default 1,
  primary key(workspace_id, provider)
);

create table if not exists timegenie.external_bindings (
  id uuid primary key default gen_random_uuid(),
  workspace_id uuid not null references timegenie.workspaces(id) on delete cascade,
  provider text not null,
  entity_type text not null,
  entity_id uuid not null,
  external_id text not null,
  external_revision text,
  last_synced_hash text,
  last_synced_at timestamptz,
  unique(workspace_id, provider, entity_type, entity_id),
  unique(workspace_id, provider, external_id)
);

create or replace function timegenie.is_workspace_owner(target_workspace_id uuid)
returns boolean language sql stable security definer set search_path = timegenie, extensions, pg_catalog
as $$ select exists(select 1 from timegenie.workspaces where id = target_workspace_id and owner_user_id = auth.uid() and deleted_at is null); $$;

create or replace function timegenie.is_registered_device(target_workspace_id uuid, target_device_id uuid)
returns boolean language sql stable security definer set search_path = timegenie, extensions, pg_catalog
as $$
  select timegenie.is_workspace_owner(target_workspace_id)
    and exists(
      select 1
      from timegenie.devices
      where id = target_device_id
        and workspace_id = target_workspace_id
        and revoked_at is null
    );
$$;

alter table timegenie.workspaces enable row level security;
alter table timegenie.devices enable row level security;
alter table timegenie.tracking_leases enable row level security;
alter table timegenie.workspace_changes enable row level security;
alter table timegenie.processed_operations enable row level security;
alter table timegenie.subjects enable row level security;
alter table timegenie.work_days enable row level security;
alter table timegenie.app_settings enable row level security;
alter table timegenie.tasks enable row level security;
alter table timegenie.task_status_events enable row level security;
alter table timegenie.task_daily_estimates enable row level security;
alter table timegenie.task_recurrence_rules enable row level security;
alter table timegenie.task_occurrences enable row level security;
alter table timegenie.time_entries enable row level security;
alter table timegenie.time_segments enable row level security;
alter table timegenie.time_allocations enable row level security;
alter table timegenie.unassigned_sessions enable row level security;
alter table timegenie.unassigned_segments enable row level security;
alter table timegenie.report_templates enable row level security;
alter table timegenie.reports enable row level security;
alter table timegenie.report_tasks enable row level security;
alter table timegenie.integration_configs enable row level security;
alter table timegenie.external_bindings enable row level security;

drop policy if exists workspaces_owner on timegenie.workspaces;
create policy workspaces_owner on timegenie.workspaces for all using(owner_user_id = auth.uid()) with check(owner_user_id = auth.uid());

do $$
declare table_name text;
begin
  foreach table_name in array array['devices','tracking_leases','workspace_changes','processed_operations','subjects','work_days','app_settings','tasks','task_status_events','task_daily_estimates','task_recurrence_rules','task_occurrences','time_entries','time_segments','time_allocations','unassigned_sessions','unassigned_segments','report_templates','reports','report_tasks','integration_configs','external_bindings'] loop
    execute format('drop policy if exists workspace_owner on timegenie.%I', table_name);
    execute format(
      'create policy workspace_owner on timegenie.%I for all using(
        exists(select 1 from timegenie.workspaces owner_workspace where owner_workspace.id = workspace_id and owner_workspace.owner_user_id = auth.uid() and owner_workspace.deleted_at is null)
      ) with check(
        exists(select 1 from timegenie.workspaces owner_workspace where owner_workspace.id = workspace_id and owner_workspace.owner_user_id = auth.uid() and owner_workspace.deleted_at is null)
      )',
      table_name
    );
  end loop;
end $$;

create or replace function timegenie.workspace_initialize_defaults(p_workspace_id uuid)
returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare default_subject_id uuid;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  select id into default_subject_id from timegenie.subjects
  where workspace_id = p_workspace_id and deleted_at is null order by sort_order, created_at limit 1;
  if default_subject_id is null then
    insert into timegenie.subjects(workspace_id, name, sort_order)
    values(p_workspace_id, '默认', 10) returning id into default_subject_id;
  end if;
  insert into timegenie.app_settings(workspace_id, key, value_json) values
    (p_workspace_id, 'default_subject_id', to_jsonb(default_subject_id::text)),
    (p_workspace_id, 'timezone', '"Asia/Shanghai"'::jsonb),
    (p_workspace_id, 'unassigned_prompt_seconds', '300'::jsonb),
    (p_workspace_id, 'default_work_period_text', '""'::jsonb),
    (p_workspace_id, 'salary_hourly_rate', '0'::jsonb)
  on conflict(workspace_id, key) do nothing;
  insert into timegenie.report_templates(workspace_id, report_type, content, is_builtin) values
    (p_workspace_id, 'daily', E'【{{姓名}}】{{日期}} {{星期}} {{工作时段}} {{总工时}}\n\n{{今日事项}}\n\n## 明日计划：\n\n{{明日计划}}\n', true),
    (p_workspace_id, 'weekly', E'# {{日期范围}}\n\n{{统计信息}}\n\n## 每日情况\n\n{{每日情况}}\n\n## 总结\n\n{{总结}}\n', true),
    (p_workspace_id, 'monthly', E'# {{日期范围}}\n\n{{统计信息}}\n\n## 本月事项\n\n{{每日情况}}\n\n## 总结\n\n{{总结}}\n', true)
  on conflict do nothing;
  return jsonb_build_object('workspaceId', p_workspace_id, 'defaultSubjectId', default_subject_id);
end $$;

create or replace function timegenie.touch_workspace_change()
returns trigger language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare row_image jsonb;
begin
  row_image := to_jsonb(coalesce(new, old));
  insert into timegenie.workspace_changes(workspace_id, entity_type, entity_id, operation, entity_version, changed_at)
  values (
    (row_image->>'workspace_id')::uuid,
    tg_table_name,
    coalesce(nullif(row_image->>'id', '')::uuid, nullif(row_image->>'task_id', '')::uuid),
    lower(tg_op),
    coalesce((row_image->>'version')::bigint, 1),
    now()
  );
  return coalesce(new, old);
end $$;

create or replace function timegenie.operation_request_hash(p_operation_type text, p_request jsonb)
returns text language sql immutable as $$
  select encode(extensions.digest(jsonb_build_object('operationType', p_operation_type, 'request', coalesce(p_request, '{}'::jsonb))::text, 'sha256'), 'hex');
$$;

create or replace function timegenie.processed_operation_result(
  p_workspace_id uuid,
  p_operation_id uuid,
  p_operation_type text,
  p_request jsonb
) returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare existing timegenie.processed_operations; expected_hash text;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text || ':' || p_operation_id::text, 0));
  select * into existing
  from timegenie.processed_operations
  where workspace_id = p_workspace_id and operation_id = p_operation_id;
  if not found then
    return null;
  end if;
  expected_hash := timegenie.operation_request_hash(p_operation_type, p_request);
  if existing.operation_type <> p_operation_type
     or (existing.request_hash is not null and existing.request_hash <> expected_hash) then
    raise exception 'IDEMPOTENCY_CONFLICT: operation_id was already used for different content';
  end if;
  return existing.result_json;
end $$;

create or replace function timegenie.record_processed_operation(
  p_workspace_id uuid,
  p_operation_id uuid,
  p_device_id uuid,
  p_operation_type text,
  p_request jsonb,
  p_result_json jsonb
) returns void language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare expected_hash text; existing timegenie.processed_operations;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id, p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  expected_hash := timegenie.operation_request_hash(p_operation_type, p_request);
  insert into timegenie.processed_operations(workspace_id, operation_id, device_id, operation_type, request_hash, result_json)
  values(p_workspace_id, p_operation_id, p_device_id, p_operation_type, expected_hash, p_result_json)
  on conflict(workspace_id, operation_id) do nothing;
  if found then
    return;
  end if;

  select * into existing
  from timegenie.processed_operations
  where workspace_id = p_workspace_id and operation_id = p_operation_id;
  if existing.operation_type <> p_operation_type
     or (existing.request_hash is not null and existing.request_hash <> expected_hash) then
    raise exception 'IDEMPOTENCY_CONFLICT: operation_id was already used for different content';
  end if;
end $$;

do $$
declare table_name text;
begin
  foreach table_name in array array['subjects','tasks','task_daily_estimates','task_recurrence_rules','task_occurrences','time_entries','time_allocations','unassigned_sessions','report_templates','reports'] loop
    execute format('drop trigger if exists workspace_change_trigger on timegenie.%I', table_name);
    execute format('create trigger workspace_change_trigger after insert or update or delete on timegenie.%I for each row execute function timegenie.touch_workspace_change()', table_name);
  end loop;
end $$;

create or replace function timegenie.unassigned_tick(
  p_workspace_id uuid, p_device_id uuid, p_lease_token uuid
) returns void language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$ begin return; end $$;

create or replace function timegenie.tracking_lease_acquire(
  p_workspace_id uuid, p_device_id uuid, p_lease_token uuid
) returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare current_lease timegenie.tracking_leases;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id, p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  insert into timegenie.tracking_leases(workspace_id, holder_device_id, lease_token, expires_at, updated_at, version)
  values(p_workspace_id, p_device_id, p_lease_token, now() + interval '45 seconds', now(), 1)
  on conflict(workspace_id) do update set holder_device_id = excluded.holder_device_id, lease_token = excluded.lease_token, expires_at = excluded.expires_at, updated_at = excluded.updated_at, version = timegenie.tracking_leases.version + 1
  where timegenie.tracking_leases.expires_at is null or timegenie.tracking_leases.expires_at < now() or timegenie.tracking_leases.holder_device_id = p_device_id;
  select * into current_lease from timegenie.tracking_leases where workspace_id = p_workspace_id;
  if current_lease.holder_device_id <> p_device_id or current_lease.lease_token <> p_lease_token then
    return jsonb_build_object('acquired', false, 'holderDeviceId', current_lease.holder_device_id, 'expiresAt', current_lease.expires_at);
  end if;
  perform timegenie.unassigned_tick(p_workspace_id, p_device_id, p_lease_token);
  return jsonb_build_object('acquired', true, 'leaseToken', current_lease.lease_token, 'expiresAt', current_lease.expires_at, 'version', current_lease.version);
end $$;

create or replace function timegenie.tracking_lease_renew(
  p_workspace_id uuid, p_device_id uuid, p_lease_token uuid
) returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare updated_row timegenie.tracking_leases;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id, p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  update timegenie.tracking_leases set expires_at = now() + interval '45 seconds', updated_at = now(), version = version + 1
  where workspace_id = p_workspace_id and holder_device_id = p_device_id and lease_token = p_lease_token and expires_at > now()
  returning * into updated_row;
  if not found then raise exception 'LEASE_EXPIRED'; end if;
  perform timegenie.unassigned_tick(p_workspace_id, p_device_id, p_lease_token);
  return jsonb_build_object('renewed', true, 'expiresAt', updated_row.expires_at, 'version', updated_row.version);
end $$;

create or replace function timegenie.tracking_lease_release(
  p_workspace_id uuid, p_device_id uuid, p_lease_token uuid
) returns boolean language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id, p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  update timegenie.tracking_leases set holder_device_id = null, lease_token = null, expires_at = null, updated_at = now(), version = version + 1 where workspace_id = p_workspace_id and holder_device_id = p_device_id and lease_token = p_lease_token;
  return found;
end $$;

create or replace function timegenie.unassigned_tick(
  p_workspace_id uuid, p_device_id uuid, p_lease_token uuid
) returns void language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare session_row timegenie.unassigned_sessions; current_segment timegenie.unassigned_segments; active_timer boolean; elapsed bigint; next_sequence integer;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id, p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  if not exists(select 1 from timegenie.tracking_leases where workspace_id = p_workspace_id and holder_device_id = p_device_id and lease_token = p_lease_token and expires_at > now()) then
    raise exception 'LEASE_EXPIRED';
  end if;
  select exists(select 1 from timegenie.time_entries where workspace_id = p_workspace_id and state in ('running','paused') and deleted_at is null) into active_timer;
  select * into session_row from timegenie.unassigned_sessions where workspace_id = p_workspace_id and state in ('collecting','awaiting_resolution') limit 1 for update;
  if active_timer then
    if found then
      update timegenie.unassigned_segments set ended_at = now(), duration_seconds = greatest(0, extract(epoch from (now() - started_at))::bigint)
      where session_id = session_row.id and ended_at is null;
      select coalesce(sum(duration_seconds),0) into elapsed from timegenie.unassigned_segments where session_id = session_row.id;
      update timegenie.unassigned_sessions set duration_seconds = elapsed, last_ended_at = now(), updated_at = now(), version = version + 1 where id = session_row.id;
    end if;
    return;
  end if;
  if session_row.id is null then
    insert into timegenie.unassigned_sessions(workspace_id, work_date, state, threshold_seconds, first_started_at)
    values(p_workspace_id, current_date, 'collecting', 300, now()) returning * into session_row;
  end if;
  select * into current_segment from timegenie.unassigned_segments where session_id = session_row.id and ended_at is null limit 1;
  if current_segment.id is null then
    select coalesce(max(sequence_no),0) + 1 into next_sequence from timegenie.unassigned_segments where session_id = session_row.id;
    insert into timegenie.unassigned_segments(workspace_id, session_id, sequence_no, started_at, lease_token)
    values(p_workspace_id, session_row.id, next_sequence, now(), p_lease_token);
  elsif current_segment.lease_token <> p_lease_token then
    update timegenie.unassigned_segments set ended_at = now(), duration_seconds = greatest(0, extract(epoch from (now() - started_at))::bigint) where id = current_segment.id;
    select coalesce(max(sequence_no),0) + 1 into next_sequence from timegenie.unassigned_segments where session_id = session_row.id;
    insert into timegenie.unassigned_segments(workspace_id, session_id, sequence_no, started_at, lease_token)
    values(p_workspace_id, session_row.id, next_sequence, now(), p_lease_token);
  end if;
  select coalesce(sum(duration_seconds + case when ended_at is null then greatest(0, extract(epoch from (now() - started_at))::bigint) else 0 end),0)
  into elapsed from timegenie.unassigned_segments where session_id = session_row.id;
  update timegenie.unassigned_sessions set duration_seconds = elapsed, state = case when elapsed > threshold_seconds then 'awaiting_resolution' else state end, prompted_at = case when elapsed > threshold_seconds then coalesce(prompted_at, now()) else prompted_at end, updated_at = now(), version = version + 1 where id = session_row.id;
end $$;

create or replace function timegenie.timer_start(
  p_workspace_id uuid, p_task_id uuid, p_device_id uuid, p_operation_id uuid, p_note text default null
) returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare existing timegenie.time_entries; created timegenie.time_entries; cached jsonb; task_label text;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id, p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  cached := timegenie.processed_operation_result(
    p_workspace_id,
    p_operation_id,
    'timer_start',
    jsonb_build_object('deviceId', p_device_id, 'taskId', p_task_id, 'note', p_note)
  );
  if cached is not null then return cached; end if;
  select * into existing from timegenie.time_entries where workspace_id = p_workspace_id and state in ('running','paused') and deleted_at is null limit 1;
  if found then raise exception 'REMOTE_TIMER_ACTIVE'; end if;
  if p_task_id is null then
    task_label := '未命名事项';
  else
    select title into task_label from timegenie.tasks t
    where t.id = p_task_id and t.workspace_id = p_workspace_id and t.deleted_at is null
      and not exists(select 1 from timegenie.tasks child where child.parent_id = t.id and child.deleted_at is null);
    if task_label is null then raise exception 'INVALID_TASK'; end if;
  end if;
  insert into timegenie.time_entries(workspace_id, work_date, kind, source_type, state, default_task_id, label_snapshot, started_at, note, created_by_device_id, updated_by_device_id)
  values(p_workspace_id, current_date, 'work', 'timer', 'running', p_task_id, task_label, now(), p_note, p_device_id, p_device_id)
  returning * into created;
  insert into timegenie.time_segments(workspace_id, entry_id, sequence_no, started_at)
  values(p_workspace_id, created.id, 1, created.started_at);
  perform timegenie.record_processed_operation(
    p_workspace_id,
    p_operation_id,
    p_device_id,
    'timer_start',
    jsonb_build_object('deviceId', p_device_id, 'taskId', p_task_id, 'note', p_note),
    to_jsonb(created)
  );
  if exists(select 1 from timegenie.tracking_leases where workspace_id = p_workspace_id and holder_device_id = p_device_id and expires_at > now()) then
    perform timegenie.unassigned_tick(p_workspace_id, p_device_id, (select lease_token from timegenie.tracking_leases where workspace_id = p_workspace_id));
  end if;
  return to_jsonb(created);
end $$;

create or replace function timegenie.timer_pause(
  p_workspace_id uuid, p_entry_id uuid, p_device_id uuid, p_expected_version bigint, p_operation_id uuid
) returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare entry_row timegenie.time_entries; cached jsonb; segment_seconds bigint;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id, p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  cached := timegenie.processed_operation_result(
    p_workspace_id,
    p_operation_id,
    'timer_pause',
    jsonb_build_object('deviceId', p_device_id, 'entryId', p_entry_id, 'expectedVersion', p_expected_version)
  );
  if cached is not null then return cached; end if;
  select * into entry_row from timegenie.time_entries where id = p_entry_id and workspace_id = p_workspace_id for update;
  if not found or entry_row.state <> 'running' then raise exception 'TIMER_NOT_RUNNING'; end if;
  if entry_row.version <> p_expected_version then raise exception 'VERSION_CONFLICT'; end if;
  update timegenie.time_segments set ended_at = now(), duration_seconds = greatest(0, extract(epoch from (now() - started_at))::bigint)
  where entry_id = p_entry_id and ended_at is null returning duration_seconds into segment_seconds;
  update timegenie.time_entries set state = 'paused', duration_seconds = duration_seconds + coalesce(segment_seconds, 0), updated_at = now(), updated_by_device_id = p_device_id, version = version + 1
  where id = p_entry_id returning * into entry_row;
  perform timegenie.record_processed_operation(
    p_workspace_id,
    p_operation_id,
    p_device_id,
    'timer_pause',
    jsonb_build_object('deviceId', p_device_id, 'entryId', p_entry_id, 'expectedVersion', p_expected_version),
    to_jsonb(entry_row)
  );
  return to_jsonb(entry_row);
end $$;

create or replace function timegenie.timer_resume(
  p_workspace_id uuid, p_entry_id uuid, p_device_id uuid, p_expected_version bigint, p_operation_id uuid
) returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare entry_row timegenie.time_entries; cached jsonb; next_sequence integer;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id, p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  cached := timegenie.processed_operation_result(
    p_workspace_id,
    p_operation_id,
    'timer_resume',
    jsonb_build_object('deviceId', p_device_id, 'entryId', p_entry_id, 'expectedVersion', p_expected_version)
  );
  if cached is not null then return cached; end if;
  select * into entry_row from timegenie.time_entries where id = p_entry_id and workspace_id = p_workspace_id for update;
  if not found or entry_row.state <> 'paused' then raise exception 'TIMER_NOT_PAUSED'; end if;
  if entry_row.version <> p_expected_version then raise exception 'VERSION_CONFLICT'; end if;
  select coalesce(max(sequence_no), 0) + 1 into next_sequence from timegenie.time_segments where entry_id = p_entry_id;
  insert into timegenie.time_segments(workspace_id, entry_id, sequence_no, started_at) values(p_workspace_id, p_entry_id, next_sequence, now());
  update timegenie.time_entries set state = 'running', updated_at = now(), updated_by_device_id = p_device_id, version = version + 1 where id = p_entry_id returning * into entry_row;
  perform timegenie.record_processed_operation(
    p_workspace_id,
    p_operation_id,
    p_device_id,
    'timer_resume',
    jsonb_build_object('deviceId', p_device_id, 'entryId', p_entry_id, 'expectedVersion', p_expected_version),
    to_jsonb(entry_row)
  );
  return to_jsonb(entry_row);
end $$;

create or replace function timegenie.timer_stop(
  p_workspace_id uuid, p_entry_id uuid, p_device_id uuid, p_expected_version bigint, p_operation_id uuid
) returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare entry_row timegenie.time_entries; cached jsonb; segment_seconds bigint;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id, p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  cached := timegenie.processed_operation_result(
    p_workspace_id,
    p_operation_id,
    'timer_stop',
    jsonb_build_object('deviceId', p_device_id, 'entryId', p_entry_id, 'expectedVersion', p_expected_version)
  );
  if cached is not null then return cached; end if;
  select * into entry_row from timegenie.time_entries where id = p_entry_id and workspace_id = p_workspace_id for update;
  if not found or entry_row.state not in ('running','paused') then raise exception 'TIMER_NOT_ACTIVE'; end if;
  if entry_row.version <> p_expected_version then raise exception 'VERSION_CONFLICT'; end if;
  if entry_row.state = 'running' then
    update timegenie.time_segments set ended_at = now(), duration_seconds = greatest(0, extract(epoch from (now() - started_at))::bigint)
    where entry_id = p_entry_id and ended_at is null returning duration_seconds into segment_seconds;
  end if;
  update timegenie.time_entries set state = 'ended', ended_at = now(), duration_seconds = duration_seconds + coalesce(segment_seconds, 0), updated_at = now(), updated_by_device_id = p_device_id, version = version + 1
  where id = p_entry_id returning * into entry_row;
  if entry_row.default_task_id is not null and entry_row.duration_seconds > 0 and exists(
    select 1 from timegenie.tasks task
    where task.id = entry_row.default_task_id
      and task.workspace_id = p_workspace_id
      and task.deleted_at is null
      and not exists(
        select 1 from timegenie.tasks child
        where child.parent_id = task.id and child.deleted_at is null
      )
  ) then
    insert into timegenie.time_allocations(workspace_id, entry_id, task_id, minutes)
    values(p_workspace_id, entry_row.id, entry_row.default_task_id, greatest(1, ceil(entry_row.duration_seconds / 60.0)::integer))
    on conflict(entry_id, task_id) do update set minutes = excluded.minutes, updated_at = now(), version = timegenie.time_allocations.version + 1;
  elsif entry_row.default_task_id is not null then
    update timegenie.time_entries set default_task_id = null where id = entry_row.id returning * into entry_row;
  end if;
  perform timegenie.record_processed_operation(
    p_workspace_id,
    p_operation_id,
    p_device_id,
    'timer_stop',
    jsonb_build_object('deviceId', p_device_id, 'entryId', p_entry_id, 'expectedVersion', p_expected_version),
    to_jsonb(entry_row)
  );
  if exists(select 1 from timegenie.tracking_leases where workspace_id = p_workspace_id and holder_device_id = p_device_id and expires_at > now()) then
    perform timegenie.unassigned_tick(p_workspace_id, p_device_id, (select lease_token from timegenie.tracking_leases where workspace_id = p_workspace_id));
  end if;
  return to_jsonb(entry_row);
end $$;

create or replace function timegenie.unassigned_get_state(p_workspace_id uuid)
returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare session_row timegenie.unassigned_sessions; segment_started timestamptz; elapsed bigint;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  select * into session_row from timegenie.unassigned_sessions where workspace_id = p_workspace_id and state in ('collecting','awaiting_resolution') limit 1;
  if not found then return null; end if;
  select started_at into segment_started from timegenie.unassigned_segments where session_id = session_row.id and ended_at is null limit 1;
  select coalesce(sum(duration_seconds + case when ended_at is null then greatest(0, extract(epoch from (now() - started_at))::bigint) else 0 end),0)
  into elapsed from timegenie.unassigned_segments where session_id = session_row.id;
  return jsonb_build_object(
    'session_id', session_row.id,
    'state', case when elapsed > session_row.threshold_seconds then 'awaiting_resolution' else session_row.state end,
    'first_started_at', session_row.first_started_at,
    'last_ended_at', session_row.last_ended_at,
    'current_segment_started_at', segment_started,
    'elapsed_seconds', elapsed,
    'required_minutes', case when elapsed > 0 then greatest(1, ceil(elapsed / 60.0)::integer) else 0 end,
    'threshold_seconds', session_row.threshold_seconds,
    'must_resolve', elapsed > session_row.threshold_seconds,
    'version', session_row.version
  );
end $$;

create or replace function timegenie.unassigned_resolve_work(
  p_workspace_id uuid, p_session_id uuid, p_device_id uuid, p_expected_version bigint, p_allocations jsonb, p_operation_id uuid
) returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare session_row timegenie.unassigned_sessions; cached jsonb; elapsed bigint; required_minutes integer; supplied_minutes integer; entry_id uuid; allocation jsonb;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id, p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  cached := timegenie.processed_operation_result(
    p_workspace_id,
    p_operation_id,
    'unassigned_resolve_work',
    jsonb_build_object('deviceId', p_device_id, 'sessionId', p_session_id, 'expectedVersion', p_expected_version, 'allocations', coalesce(p_allocations, '[]'::jsonb))
  );
  if cached is not null then return cached; end if;
  select * into session_row from timegenie.unassigned_sessions where id = p_session_id and workspace_id = p_workspace_id for update;
  if not found or session_row.state not in ('collecting','awaiting_resolution') then raise exception 'SESSION_ALREADY_RESOLVED'; end if;
  if session_row.version <> p_expected_version then raise exception 'VERSION_CONFLICT'; end if;
  update timegenie.unassigned_segments set ended_at = now(), duration_seconds = greatest(0, extract(epoch from (now() - started_at))::bigint) where session_id = p_session_id and ended_at is null;
  select coalesce(sum(duration_seconds),0) into elapsed from timegenie.unassigned_segments where session_id = p_session_id;
  required_minutes := case when elapsed > 0 then greatest(1, ceil(elapsed / 60.0)::integer) else 0 end;
  if exists(select 1 from jsonb_array_elements(coalesce(p_allocations,'[]'::jsonb)) where (value->>'minutes')::integer < 0) then raise exception 'VALIDATION_ERROR: allocation minutes must be non-negative'; end if;
  select coalesce(sum((value->>'minutes')::integer),0) into supplied_minutes from jsonb_array_elements(coalesce(p_allocations,'[]'::jsonb));
  if supplied_minutes <= 0 then raise exception 'VALIDATION_ERROR: allocation required'; end if;
  if supplied_minutes > required_minutes then raise exception 'ALLOCATION_EXCEEDS_DURATION: required=% supplied=%', required_minutes, supplied_minutes; end if;
  insert into timegenie.time_entries(workspace_id, work_date, kind, source_type, state, label_snapshot, started_at, ended_at, duration_seconds, origin_unassigned_session_id, created_by_device_id, updated_by_device_id)
  values(p_workspace_id, session_row.work_date, 'work', 'unassigned', 'ended', '未归属工作时间', session_row.first_started_at, now(), elapsed, session_row.id, p_device_id, p_device_id) returning id into entry_id;
  for allocation in select value from jsonb_array_elements(p_allocations) loop
    if (allocation->>'minutes')::integer <= 0 then continue; end if;
    if not exists(select 1 from timegenie.tasks t where t.id = (allocation->>'taskId')::uuid and t.workspace_id = p_workspace_id and t.deleted_at is null and not exists(select 1 from timegenie.tasks c where c.parent_id = t.id and c.deleted_at is null)) then raise exception 'INVALID_TASK'; end if;
    insert into timegenie.time_allocations(workspace_id, entry_id, task_id, minutes) values(p_workspace_id, entry_id, (allocation->>'taskId')::uuid, (allocation->>'minutes')::integer);
  end loop;
  update timegenie.unassigned_sessions set state = 'resolved', resolution_type = 'work', generated_entry_id = entry_id, duration_seconds = elapsed, last_ended_at = now(), resolved_at = now(), updated_at = now(), version = version + 1 where id = p_session_id returning * into session_row;
  cached := jsonb_build_object('sessionId', p_session_id, 'generatedEntryId', entry_id, 'resolutionType', 'work', 'elapsedSeconds', elapsed, 'requiredMinutes', required_minutes);
  perform timegenie.record_processed_operation(
    p_workspace_id,
    p_operation_id,
    p_device_id,
    'unassigned_resolve_work',
    jsonb_build_object('deviceId', p_device_id, 'sessionId', p_session_id, 'expectedVersion', p_expected_version, 'allocations', coalesce(p_allocations, '[]'::jsonb)),
    cached
  );
  return cached;
end $$;

create or replace function timegenie.unassigned_resolve_non_work(
  p_workspace_id uuid, p_session_id uuid, p_device_id uuid, p_expected_version bigint, p_operation_id uuid, p_resolution_type text
) returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare session_row timegenie.unassigned_sessions; cached jsonb; elapsed bigint; entry_id uuid;
begin
  if p_resolution_type not in ('break','discard') then raise exception 'INVALID_RESOLUTION'; end if;
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id, p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  cached := timegenie.processed_operation_result(
    p_workspace_id,
    p_operation_id,
    'unassigned_resolve_' || p_resolution_type,
    jsonb_build_object('deviceId', p_device_id, 'sessionId', p_session_id, 'expectedVersion', p_expected_version)
  );
  if cached is not null then return cached; end if;
  select * into session_row from timegenie.unassigned_sessions where id = p_session_id and workspace_id = p_workspace_id for update;
  if not found or session_row.state not in ('collecting','awaiting_resolution') then raise exception 'SESSION_ALREADY_RESOLVED'; end if;
  if session_row.version <> p_expected_version then raise exception 'VERSION_CONFLICT'; end if;
  update timegenie.unassigned_segments set ended_at = now(), duration_seconds = greatest(0, extract(epoch from (now() - started_at))::bigint) where session_id = p_session_id and ended_at is null;
  select coalesce(sum(duration_seconds),0) into elapsed from timegenie.unassigned_segments where session_id = p_session_id;
  if p_resolution_type in ('break','discard') then
    insert into timegenie.time_entries(workspace_id, work_date, kind, source_type, state, label_snapshot, started_at, ended_at, duration_seconds, origin_unassigned_session_id, created_by_device_id, updated_by_device_id)
    values(p_workspace_id, session_row.work_date, 'break', 'unassigned', 'ended', case when p_resolution_type = 'break' then '休息时间' else '无效时间' end, session_row.first_started_at, now(), elapsed, session_row.id, p_device_id, p_device_id) returning id into entry_id;
  end if;
  update timegenie.unassigned_sessions set state = case when p_resolution_type = 'discard' then 'discarded' else 'resolved' end, resolution_type = p_resolution_type, generated_entry_id = entry_id, duration_seconds = elapsed, last_ended_at = now(), resolved_at = now(), updated_at = now(), version = version + 1 where id = p_session_id;
  cached := jsonb_build_object('sessionId', p_session_id, 'generatedEntryId', entry_id, 'resolutionType', p_resolution_type, 'elapsedSeconds', elapsed, 'requiredMinutes', case when elapsed > 0 then greatest(1, ceil(elapsed / 60.0)::integer) else 0 end);
  perform timegenie.record_processed_operation(
    p_workspace_id,
    p_operation_id,
    p_device_id,
    'unassigned_resolve_' || p_resolution_type,
    jsonb_build_object('deviceId', p_device_id, 'sessionId', p_session_id, 'expectedVersion', p_expected_version),
    cached
  );
  return cached;
end $$;

create or replace function timegenie.unassigned_resolve_break(p_workspace_id uuid, p_session_id uuid, p_device_id uuid, p_expected_version bigint, p_allocations jsonb, p_operation_id uuid)
returns jsonb language sql security definer set search_path = timegenie, extensions, pg_catalog as $$ select timegenie.unassigned_resolve_non_work(p_workspace_id, p_session_id, p_device_id, p_expected_version, p_operation_id, 'break'); $$;

create or replace function timegenie.unassigned_discard(p_workspace_id uuid, p_session_id uuid, p_device_id uuid, p_expected_version bigint, p_allocations jsonb, p_operation_id uuid)
returns jsonb language sql security definer set search_path = timegenie, extensions, pg_catalog as $$ select timegenie.unassigned_resolve_non_work(p_workspace_id, p_session_id, p_device_id, p_expected_version, p_operation_id, 'discard'); $$;

create or replace function timegenie.cloud_snapshot_get(p_workspace_id uuid)
returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  return jsonb_build_object(
    'subjects', coalesce((select jsonb_agg(to_jsonb(row_value)) from (select * from timegenie.subjects where workspace_id = p_workspace_id) row_value), '[]'::jsonb),
    'work_days', coalesce((select jsonb_agg(to_jsonb(row_value)) from (select * from timegenie.work_days where workspace_id = p_workspace_id) row_value), '[]'::jsonb),
    'app_settings', coalesce((select jsonb_agg(to_jsonb(row_value)) from (select * from timegenie.app_settings where workspace_id = p_workspace_id) row_value), '[]'::jsonb),
    'integration_configs', coalesce((select jsonb_agg(to_jsonb(row_value)) from (select * from timegenie.integration_configs where workspace_id = p_workspace_id) row_value), '[]'::jsonb),
    'external_bindings', coalesce((select jsonb_agg(to_jsonb(row_value)) from (select * from timegenie.external_bindings where workspace_id = p_workspace_id) row_value), '[]'::jsonb),
    'tasks', coalesce((select jsonb_agg(to_jsonb(row_value)) from (select * from timegenie.tasks where workspace_id = p_workspace_id) row_value), '[]'::jsonb),
    'task_status_events', coalesce((select jsonb_agg(to_jsonb(row_value)) from (select * from timegenie.task_status_events where workspace_id = p_workspace_id) row_value), '[]'::jsonb),
    'task_daily_estimates', coalesce((select jsonb_agg(to_jsonb(row_value)) from (select * from timegenie.task_daily_estimates where workspace_id = p_workspace_id) row_value), '[]'::jsonb),
    'task_recurrence_rules', coalesce((select jsonb_agg(to_jsonb(row_value)) from (select * from timegenie.task_recurrence_rules where workspace_id = p_workspace_id) row_value), '[]'::jsonb),
    'task_occurrences', coalesce((select jsonb_agg(to_jsonb(row_value)) from (select * from timegenie.task_occurrences where workspace_id = p_workspace_id) row_value), '[]'::jsonb),
    'time_entries', coalesce((select jsonb_agg(to_jsonb(row_value)) from (select * from timegenie.time_entries where workspace_id = p_workspace_id) row_value), '[]'::jsonb),
    'time_segments', coalesce((select jsonb_agg(to_jsonb(row_value)) from (select * from timegenie.time_segments where workspace_id = p_workspace_id) row_value), '[]'::jsonb),
    'time_allocations', coalesce((select jsonb_agg(to_jsonb(row_value)) from (select * from timegenie.time_allocations where workspace_id = p_workspace_id) row_value), '[]'::jsonb),
    'unassigned_sessions', coalesce((select jsonb_agg(to_jsonb(row_value)) from (select * from timegenie.unassigned_sessions where workspace_id = p_workspace_id) row_value), '[]'::jsonb),
    'unassigned_segments', coalesce((select jsonb_agg(to_jsonb(row_value)) from (select * from timegenie.unassigned_segments where workspace_id = p_workspace_id) row_value), '[]'::jsonb),
    'report_templates', coalesce((select jsonb_agg(to_jsonb(row_value)) from (select * from timegenie.report_templates where workspace_id = p_workspace_id) row_value), '[]'::jsonb),
    'reports', coalesce((select jsonb_agg(to_jsonb(row_value)) from (select * from timegenie.reports where workspace_id = p_workspace_id) row_value), '[]'::jsonb),
    'report_tasks', coalesce((select jsonb_agg(to_jsonb(row_value)) from (select * from timegenie.report_tasks where workspace_id = p_workspace_id) row_value), '[]'::jsonb),
    'latest_change_seq', coalesce((select max(change_seq) from timegenie.workspace_changes where workspace_id = p_workspace_id), 0)
  );
end $$;

create or replace function timegenie.migration_import_snapshot(
  p_workspace_id uuid, p_operation_id uuid, p_device_id uuid, p_snapshot jsonb
) returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare row_data jsonb; cached jsonb; imported_subjects integer := 0; imported_tasks integer := 0; imported_entries integer := 0; imported_reports integer := 0;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  cached := timegenie.processed_operation_result(
    p_workspace_id,
    p_operation_id,
    'migration_import_snapshot',
    jsonb_build_object('deviceId', p_device_id, 'snapshot', coalesce(p_snapshot, '{}'::jsonb))
  );
  if cached is not null then return cached; end if;
  if not exists(select 1 from timegenie.devices where id = p_device_id and workspace_id = p_workspace_id and revoked_at is null) then
    raise exception 'DEVICE_NOT_REGISTERED';
  end if;
  if exists(select 1 from timegenie.tasks where workspace_id = p_workspace_id)
     or exists(select 1 from timegenie.task_daily_estimates where workspace_id = p_workspace_id)
     or exists(select 1 from timegenie.task_recurrence_rules where workspace_id = p_workspace_id)
     or exists(select 1 from timegenie.task_occurrences where workspace_id = p_workspace_id)
     or exists(select 1 from timegenie.time_entries where workspace_id = p_workspace_id)
     or exists(select 1 from timegenie.reports where workspace_id = p_workspace_id) then
    raise exception 'MIGRATION_TARGET_NOT_EMPTY';
  end if;
  delete from timegenie.report_templates where workspace_id = p_workspace_id;
  delete from timegenie.app_settings where workspace_id = p_workspace_id;
  delete from timegenie.subjects where workspace_id = p_workspace_id;
  for row_data in select value from jsonb_array_elements(coalesce(p_snapshot->'subjects', '[]'::jsonb)) loop
    insert into timegenie.subjects(id, workspace_id, name, sort_order, created_at, updated_at, version, created_by_device_id, updated_by_device_id, deleted_at)
    values((row_data->>'id')::uuid, p_workspace_id, row_data->>'name', coalesce((row_data->>'sort_order')::integer,0), to_timestamp((row_data->>'created_at')::double precision / 1000), to_timestamp((row_data->>'updated_at')::double precision / 1000), coalesce((row_data->>'version')::bigint,1), p_device_id, p_device_id, case when row_data->>'deleted_at' is null then null else to_timestamp((row_data->>'deleted_at')::double precision / 1000) end)
    on conflict(id) do update set name = excluded.name, sort_order = excluded.sort_order, updated_at = excluded.updated_at, version = greatest(timegenie.subjects.version, excluded.version), deleted_at = excluded.deleted_at;
    imported_subjects := imported_subjects + 1;
  end loop;
  for row_data in select value from jsonb_array_elements(coalesce(p_snapshot->'tasks', '[]'::jsonb)) loop
    insert into timegenie.tasks(id, workspace_id, subject_id, parent_id, title, status, planned_date, planned_time, estimate_minutes, note, project_name, solution_name, source_type, source_ref, sort_order, completed_at, created_at, updated_at, version, created_by_device_id, updated_by_device_id, deleted_at)
    values((row_data->>'id')::uuid, p_workspace_id, (row_data->>'subject_id')::uuid, null, row_data->>'title', row_data->>'status', nullif(row_data->>'planned_date','')::date, nullif(row_data->>'planned_time','')::time, nullif(row_data->>'estimate_minutes','')::integer, row_data->>'note', row_data->>'project_name', row_data->>'solution_name', case when row_data->>'source_type' in ('manual','obsidian_import','timer_quick_create') then row_data->>'source_type' else 'migration' end, row_data->>'source_ref', coalesce((row_data->>'sort_order')::integer,0), case when row_data->>'completed_at' is null then null else to_timestamp((row_data->>'completed_at')::double precision / 1000) end, to_timestamp((row_data->>'created_at')::double precision / 1000), to_timestamp((row_data->>'updated_at')::double precision / 1000), coalesce((row_data->>'version')::bigint,1), p_device_id, p_device_id, case when row_data->>'deleted_at' is null then null else to_timestamp((row_data->>'deleted_at')::double precision / 1000) end)
    on conflict(id) do update set parent_id = null, title = excluded.title, status = excluded.status, planned_date = excluded.planned_date, planned_time = excluded.planned_time, estimate_minutes = excluded.estimate_minutes, note = excluded.note, project_name = excluded.project_name, solution_name = excluded.solution_name, sort_order = excluded.sort_order, completed_at = excluded.completed_at, updated_at = excluded.updated_at, version = greatest(timegenie.tasks.version, excluded.version), deleted_at = excluded.deleted_at;
    imported_tasks := imported_tasks + 1;
  end loop;
  for row_data in select value from jsonb_array_elements(coalesce(p_snapshot->'tasks', '[]'::jsonb)) loop
    if row_data->>'parent_id' is not null and row_data->>'parent_id' <> '' then
      update timegenie.tasks set parent_id = (row_data->>'parent_id')::uuid where id = (row_data->>'id')::uuid and workspace_id = p_workspace_id;
    end if;
  end loop;
  for row_data in select value from jsonb_array_elements(coalesce(p_snapshot->'work_days', '[]'::jsonb)) loop
    insert into timegenie.work_days(workspace_id, work_date, timezone, work_period_text, note, settled_at, created_at, updated_at, version)
    values(p_workspace_id, (row_data->>'work_date')::date, row_data->>'timezone', row_data->>'work_period_text', row_data->>'note', case when row_data->>'settled_at' is null then null else to_timestamp((row_data->>'settled_at')::double precision / 1000) end, to_timestamp((row_data->>'created_at')::double precision / 1000), to_timestamp((row_data->>'updated_at')::double precision / 1000), coalesce((row_data->>'version')::bigint,1))
    on conflict(workspace_id, work_date) do update set timezone = excluded.timezone, work_period_text = excluded.work_period_text, note = excluded.note, settled_at = excluded.settled_at, updated_at = excluded.updated_at, version = greatest(timegenie.work_days.version, excluded.version);
  end loop;
  for row_data in select value from jsonb_array_elements(coalesce(p_snapshot->'app_settings', '[]'::jsonb)) loop
    insert into timegenie.app_settings(workspace_id, key, value_json, updated_at, version)
    values(p_workspace_id, row_data->>'key', coalesce(row_data->'value_json', 'null'::jsonb), to_timestamp((row_data->>'updated_at')::double precision / 1000), coalesce((row_data->>'version')::bigint,1))
    on conflict(workspace_id, key) do update set value_json = excluded.value_json, updated_at = excluded.updated_at, version = greatest(timegenie.app_settings.version, excluded.version);
  end loop;
  for row_data in select value from jsonb_array_elements(coalesce(p_snapshot->'integration_configs', '[]'::jsonb)) loop
    insert into timegenie.integration_configs(workspace_id, provider, enabled, config_json, updated_at, version)
    values(p_workspace_id, row_data->>'provider', coalesce((row_data->>'enabled')::integer,0) <> 0, coalesce(row_data->'config_json', '{}'::jsonb), to_timestamp((row_data->>'updated_at')::double precision / 1000), coalesce((row_data->>'version')::bigint,1))
    on conflict(workspace_id, provider) do update set enabled = excluded.enabled, config_json = excluded.config_json, updated_at = excluded.updated_at, version = greatest(timegenie.integration_configs.version, excluded.version);
  end loop;
  for row_data in select value from jsonb_array_elements(coalesce(p_snapshot->'external_bindings', '[]'::jsonb)) loop
    insert into timegenie.external_bindings(id, workspace_id, provider, entity_type, entity_id, external_id, external_revision, last_synced_hash, last_synced_at)
    values((row_data->>'id')::uuid, p_workspace_id, row_data->>'provider', row_data->>'entity_type', (row_data->>'entity_id')::uuid, row_data->>'external_id', row_data->>'external_revision', row_data->>'last_synced_hash', case when row_data->>'last_synced_at' is null then null else to_timestamp((row_data->>'last_synced_at')::double precision / 1000) end)
    on conflict(workspace_id, provider, entity_type, entity_id) do update set external_id = excluded.external_id, external_revision = excluded.external_revision, last_synced_hash = excluded.last_synced_hash, last_synced_at = excluded.last_synced_at;
  end loop;
  for row_data in select value from jsonb_array_elements(coalesce(p_snapshot->'task_status_events', '[]'::jsonb)) loop
    insert into timegenie.task_status_events(id, workspace_id, task_id, status, occurred_at, source_type, created_at)
    values((row_data->>'id')::uuid, p_workspace_id, (row_data->>'task_id')::uuid, row_data->>'status', to_timestamp((row_data->>'occurred_at')::double precision / 1000), row_data->>'source_type', to_timestamp((row_data->>'created_at')::double precision / 1000))
    on conflict(id) do nothing;
  end loop;
  for row_data in select value from jsonb_array_elements(coalesce(p_snapshot->'task_daily_estimates', '[]'::jsonb)) loop
    insert into timegenie.task_daily_estimates(workspace_id, task_id, work_date, estimate_minutes, created_at, updated_at, version)
    values(p_workspace_id, (row_data->>'task_id')::uuid, (row_data->>'work_date')::date, (row_data->>'estimate_minutes')::integer, to_timestamp((row_data->>'created_at')::double precision / 1000), to_timestamp((row_data->>'updated_at')::double precision / 1000), coalesce((row_data->>'version')::bigint,1))
    on conflict(task_id, work_date) do update set estimate_minutes = excluded.estimate_minutes, updated_at = excluded.updated_at, version = greatest(timegenie.task_daily_estimates.version, excluded.version);
  end loop;
  for row_data in select value from jsonb_array_elements(coalesce(p_snapshot->'task_recurrence_rules', '[]'::jsonb)) loop
    insert into timegenie.task_recurrence_rules(id, workspace_id, task_id, frequency, weekdays_mask, effective_start, effective_end, created_at, updated_at, version)
    values((row_data->>'id')::uuid, p_workspace_id, (row_data->>'task_id')::uuid, row_data->>'frequency', nullif(row_data->>'weekdays_mask','')::integer, (row_data->>'effective_start')::date, nullif(row_data->>'effective_end','')::date, to_timestamp((row_data->>'created_at')::double precision / 1000), to_timestamp((row_data->>'updated_at')::double precision / 1000), coalesce((row_data->>'version')::bigint,1))
    on conflict(id) do update set frequency = excluded.frequency, weekdays_mask = excluded.weekdays_mask, effective_start = excluded.effective_start, effective_end = excluded.effective_end, updated_at = excluded.updated_at, version = greatest(timegenie.task_recurrence_rules.version, excluded.version);
  end loop;
  for row_data in select value from jsonb_array_elements(coalesce(p_snapshot->'task_occurrences', '[]'::jsonb)) loop
    insert into timegenie.task_occurrences(workspace_id, task_id, occurrence_date, origin, status, completed_at, created_at, updated_at, version)
    values(p_workspace_id, (row_data->>'task_id')::uuid, (row_data->>'occurrence_date')::date, row_data->>'origin', row_data->>'status', case when row_data->>'completed_at' is null then null else to_timestamp((row_data->>'completed_at')::double precision / 1000) end, to_timestamp((row_data->>'created_at')::double precision / 1000), to_timestamp((row_data->>'updated_at')::double precision / 1000), coalesce((row_data->>'version')::bigint,1))
    on conflict(task_id, occurrence_date) do update set origin = excluded.origin, status = excluded.status, completed_at = excluded.completed_at, updated_at = excluded.updated_at, version = greatest(timegenie.task_occurrences.version, excluded.version);
  end loop;
  for row_data in select value from jsonb_array_elements(coalesce(p_snapshot->'time_entries', '[]'::jsonb)) loop
    insert into timegenie.time_entries(id, workspace_id, work_date, kind, source_type, state, default_task_id, label_snapshot, started_at, ended_at, duration_seconds, note, origin_unassigned_session_id, created_at, updated_at, version, created_by_device_id, updated_by_device_id, deleted_at)
    values((row_data->>'id')::uuid, p_workspace_id, (row_data->>'work_date')::date, row_data->>'kind', row_data->>'source_type', row_data->>'state', nullif(row_data->>'default_task_id','')::uuid, row_data->>'label_snapshot', to_timestamp((row_data->>'started_at')::double precision / 1000), case when row_data->>'ended_at' is null then null else to_timestamp((row_data->>'ended_at')::double precision / 1000) end, coalesce((row_data->>'duration_seconds')::bigint,0), row_data->>'note', null, to_timestamp((row_data->>'created_at')::double precision / 1000), to_timestamp((row_data->>'updated_at')::double precision / 1000), coalesce((row_data->>'version')::bigint,1), p_device_id, p_device_id, case when row_data->>'deleted_at' is null then null else to_timestamp((row_data->>'deleted_at')::double precision / 1000) end)
    on conflict(id) do update set state = excluded.state, ended_at = excluded.ended_at, duration_seconds = excluded.duration_seconds, note = excluded.note, updated_at = excluded.updated_at, version = greatest(timegenie.time_entries.version, excluded.version), deleted_at = excluded.deleted_at;
    imported_entries := imported_entries + 1;
  end loop;
  for row_data in select value from jsonb_array_elements(coalesce(p_snapshot->'time_segments', '[]'::jsonb)) loop
    insert into timegenie.time_segments(id, workspace_id, entry_id, sequence_no, started_at, ended_at, duration_seconds)
    values((row_data->>'id')::uuid, p_workspace_id, (row_data->>'entry_id')::uuid, (row_data->>'sequence_no')::integer, to_timestamp((row_data->>'started_at')::double precision / 1000), case when row_data->>'ended_at' is null then null else to_timestamp((row_data->>'ended_at')::double precision / 1000) end, coalesce((row_data->>'duration_seconds')::bigint,0))
    on conflict(id) do update set ended_at = excluded.ended_at, duration_seconds = excluded.duration_seconds;
  end loop;
  for row_data in select value from jsonb_array_elements(coalesce(p_snapshot->'time_allocations', '[]'::jsonb)) loop
    insert into timegenie.time_allocations(id, workspace_id, entry_id, task_id, minutes, note, created_at, updated_at, version)
    values((row_data->>'id')::uuid, p_workspace_id, (row_data->>'entry_id')::uuid, (row_data->>'task_id')::uuid, (row_data->>'minutes')::integer, row_data->>'note', to_timestamp((row_data->>'created_at')::double precision / 1000), to_timestamp((row_data->>'updated_at')::double precision / 1000), coalesce((row_data->>'version')::bigint,1))
    on conflict(id) do update set task_id = excluded.task_id, minutes = excluded.minutes, note = excluded.note, updated_at = excluded.updated_at, version = greatest(timegenie.time_allocations.version, excluded.version);
  end loop;
  for row_data in select value from jsonb_array_elements(coalesce(p_snapshot->'unassigned_sessions', '[]'::jsonb)) loop
    insert into timegenie.unassigned_sessions(id, workspace_id, work_date, state, threshold_seconds, duration_seconds, first_started_at, last_ended_at, prompted_at, resolution_type, generated_entry_id, resolved_at, created_at, updated_at, version)
    values((row_data->>'id')::uuid, p_workspace_id, (row_data->>'work_date')::date, row_data->>'state', coalesce((row_data->>'threshold_seconds')::integer,300), coalesce((row_data->>'duration_seconds')::bigint,0), to_timestamp((row_data->>'first_started_at')::double precision / 1000), case when row_data->>'last_ended_at' is null then null else to_timestamp((row_data->>'last_ended_at')::double precision / 1000) end, case when row_data->>'prompted_at' is null then null else to_timestamp((row_data->>'prompted_at')::double precision / 1000) end, row_data->>'resolution_type', nullif(row_data->>'generated_entry_id','')::uuid, case when row_data->>'resolved_at' is null then null else to_timestamp((row_data->>'resolved_at')::double precision / 1000) end, to_timestamp((row_data->>'created_at')::double precision / 1000), to_timestamp((row_data->>'updated_at')::double precision / 1000), coalesce((row_data->>'version')::bigint,1))
    on conflict(id) do update set state = excluded.state, duration_seconds = excluded.duration_seconds, last_ended_at = excluded.last_ended_at, prompted_at = excluded.prompted_at, resolution_type = excluded.resolution_type, generated_entry_id = excluded.generated_entry_id, resolved_at = excluded.resolved_at, updated_at = excluded.updated_at, version = greatest(timegenie.unassigned_sessions.version, excluded.version);
  end loop;
  for row_data in select value from jsonb_array_elements(coalesce(p_snapshot->'unassigned_segments', '[]'::jsonb)) loop
    insert into timegenie.unassigned_segments(id, workspace_id, session_id, sequence_no, started_at, ended_at, duration_seconds, lease_token)
    values((row_data->>'id')::uuid, p_workspace_id, (row_data->>'session_id')::uuid, (row_data->>'sequence_no')::integer, to_timestamp((row_data->>'started_at')::double precision / 1000), case when row_data->>'ended_at' is null then null else to_timestamp((row_data->>'ended_at')::double precision / 1000) end, coalesce((row_data->>'duration_seconds')::bigint,0), nullif(row_data->>'lease_token','')::uuid)
    on conflict(id) do update set ended_at = excluded.ended_at, duration_seconds = excluded.duration_seconds, lease_token = excluded.lease_token;
  end loop;
  for row_data in select value from jsonb_array_elements(coalesce(p_snapshot->'report_templates', '[]'::jsonb)) loop
    insert into timegenie.report_templates(id, workspace_id, report_type, subject_id, content, is_builtin, created_at, updated_at, version, deleted_at)
    values((row_data->>'id')::uuid, p_workspace_id, row_data->>'report_type', nullif(row_data->>'subject_id','')::uuid, row_data->>'content', coalesce((row_data->>'is_builtin')::integer,0) <> 0, to_timestamp((row_data->>'created_at')::double precision / 1000), to_timestamp((row_data->>'updated_at')::double precision / 1000), coalesce((row_data->>'version')::bigint,1), case when row_data->>'deleted_at' is null then null else to_timestamp((row_data->>'deleted_at')::double precision / 1000) end)
    on conflict(id) do update set content = excluded.content, updated_at = excluded.updated_at, version = greatest(timegenie.report_templates.version, excluded.version), deleted_at = excluded.deleted_at;
  end loop;
  for row_data in select value from jsonb_array_elements(coalesce(p_snapshot->'reports', '[]'::jsonb)) loop
    insert into timegenie.reports(id, workspace_id, report_type, subject_id, period_start, period_end, reference_date, template_id, markdown_content, content_source, generation_count, input_revision_hash, generated_at, created_at, updated_at, version, created_by_device_id, updated_by_device_id, deleted_at)
    values((row_data->>'id')::uuid, p_workspace_id, row_data->>'report_type', (row_data->>'subject_id')::uuid, (row_data->>'period_start')::date, (row_data->>'period_end')::date, (row_data->>'reference_date')::date, nullif(row_data->>'template_id','')::uuid, row_data->>'markdown_content', row_data->>'content_source', coalesce((row_data->>'generation_count')::integer,1), row_data->>'input_revision_hash', case when row_data->>'generated_at' is null then null else to_timestamp((row_data->>'generated_at')::double precision / 1000) end, to_timestamp((row_data->>'created_at')::double precision / 1000), to_timestamp((row_data->>'updated_at')::double precision / 1000), coalesce((row_data->>'version')::bigint,1), p_device_id, p_device_id, case when row_data->>'deleted_at' is null then null else to_timestamp((row_data->>'deleted_at')::double precision / 1000) end)
    on conflict(id) do update set markdown_content = excluded.markdown_content, content_source = excluded.content_source, generation_count = excluded.generation_count, input_revision_hash = excluded.input_revision_hash, generated_at = excluded.generated_at, updated_at = excluded.updated_at, version = greatest(timegenie.reports.version, excluded.version), deleted_at = excluded.deleted_at;
    imported_reports := imported_reports + 1;
  end loop;
  for row_data in select value from jsonb_array_elements(coalesce(p_snapshot->'report_tasks', '[]'::jsonb)) loop
    insert into timegenie.report_tasks(report_id, task_id, workspace_id, sort_order, title_snapshot, path_snapshot)
    values((row_data->>'report_id')::uuid, (row_data->>'task_id')::uuid, p_workspace_id, (row_data->>'sort_order')::integer, row_data->>'title_snapshot', row_data->>'path_snapshot')
    on conflict(report_id, task_id) do update set sort_order = excluded.sort_order, title_snapshot = excluded.title_snapshot, path_snapshot = excluded.path_snapshot;
  end loop;
  perform timegenie.record_processed_operation(
    p_workspace_id,
    p_operation_id,
    p_device_id,
    'migration_import_snapshot',
    jsonb_build_object('deviceId', p_device_id, 'snapshot', coalesce(p_snapshot, '{}'::jsonb)),
    jsonb_build_object('accepted', true, 'subjects', imported_subjects, 'tasks', imported_tasks, 'timeEntries', imported_entries, 'reports', imported_reports)
  );
  return jsonb_build_object('accepted', true, 'subjects', imported_subjects, 'tasks', imported_tasks, 'timeEntries', imported_entries, 'reports', imported_reports);
end $$;

-- Apply one complete entity image. The desktop client sends the image after
-- its local SQLite transaction commits. Keeping the operation idempotent and
-- version checked here prevents an offline retry from becoming last-write-wins.
create or replace function timegenie.cloud_apply_patch(
  p_workspace_id uuid,
  p_device_id uuid,
  p_operation_id uuid,
  p_operation_type text,
  p_entity_type text,
  p_entity_id text,
  p_base_version bigint,
  p_payload jsonb
) returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare current_version bigint; cached jsonb; result_value jsonb; row_data jsonb;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not exists(select 1 from timegenie.devices where id = p_device_id and workspace_id = p_workspace_id and revoked_at is null) then
    raise exception 'DEVICE_NOT_REGISTERED';
  end if;
  cached := timegenie.processed_operation_result(
    p_workspace_id,
    p_operation_id,
    p_operation_type,
    jsonb_build_object('deviceId', p_device_id, 'entityType', p_entity_type, 'entityId', p_entity_id, 'baseVersion', p_base_version, 'payload', coalesce(p_payload, '{}'::jsonb))
  );
  if cached is not null then return cached; end if;

  if p_base_version is not null and (p_entity_id is not null or p_entity_type = 'app_setting') then
    case p_entity_type
      when 'subject' then select version into current_version from timegenie.subjects where workspace_id = p_workspace_id and id = p_entity_id::uuid;
      when 'task' then select version into current_version from timegenie.tasks where workspace_id = p_workspace_id and id = p_entity_id::uuid;
      when 'time_entry' then select version into current_version from timegenie.time_entries where workspace_id = p_workspace_id and id = p_entity_id::uuid;
      when 'report' then select version into current_version from timegenie.reports where workspace_id = p_workspace_id and id = p_entity_id::uuid;
      when 'report_template' then select version into current_version from timegenie.report_templates where workspace_id = p_workspace_id and id = p_entity_id::uuid;
      when 'app_setting' then select version into current_version from timegenie.app_settings where workspace_id = p_workspace_id and key = p_payload->>'key';
      when 'integration_config' then select version into current_version from timegenie.integration_configs where workspace_id = p_workspace_id and provider = p_payload->>'provider';
      else current_version := p_base_version;
    end case;
    if current_version is distinct from p_base_version then
      raise exception 'SYNC_CONFLICT: expected version %, current version %', p_base_version, current_version;
    end if;
  end if;

  row_data := coalesce(p_payload, '{}'::jsonb);
  if p_entity_type = 'subject' then
    insert into timegenie.subjects(id, workspace_id, name, sort_order, created_at, updated_at, version, created_by_device_id, updated_by_device_id, deleted_at)
    values((row_data->>'id')::uuid, p_workspace_id, row_data->>'name', coalesce((row_data->>'sort_order')::integer,0), to_timestamp(coalesce((row_data->>'created_at')::double precision, extract(epoch from now()) * 1000) / 1000), to_timestamp(coalesce((row_data->>'updated_at')::double precision, extract(epoch from now()) * 1000) / 1000), coalesce((row_data->>'version')::bigint,1), p_device_id, p_device_id, case when row_data->>'deleted_at' is null then null else to_timestamp((row_data->>'deleted_at')::double precision / 1000) end)
    on conflict(id) do update set name = excluded.name, sort_order = excluded.sort_order, updated_at = excluded.updated_at, version = excluded.version, updated_by_device_id = p_device_id, deleted_at = excluded.deleted_at;
    result_value := jsonb_build_object('entityType','subject','entityId',p_entity_id,'version',(select version from timegenie.subjects where id = p_entity_id::uuid));
  elsif p_entity_type = 'task' then
    insert into timegenie.tasks(id, workspace_id, subject_id, parent_id, title, status, planned_date, planned_time, estimate_minutes, note, project_name, solution_name, source_type, source_ref, sort_order, completed_at, created_at, updated_at, version, created_by_device_id, updated_by_device_id, deleted_at)
    values((row_data->>'id')::uuid, p_workspace_id, (row_data->>'subject_id')::uuid, nullif(row_data->>'parent_id','')::uuid, row_data->>'title', row_data->>'status', nullif(row_data->>'planned_date','')::date, nullif(row_data->>'planned_time','')::time, nullif(row_data->>'estimate_minutes','')::integer, row_data->>'note', row_data->>'project_name', row_data->>'solution_name', case when row_data->>'source_type' in ('manual','obsidian_import','timer_quick_create','migration') then row_data->>'source_type' else 'manual' end, row_data->>'source_ref', coalesce((row_data->>'sort_order')::integer,0), case when row_data->>'completed_at' is null then null else to_timestamp((row_data->>'completed_at')::double precision / 1000) end, to_timestamp(coalesce((row_data->>'created_at')::double precision, extract(epoch from now()) * 1000) / 1000), to_timestamp(coalesce((row_data->>'updated_at')::double precision, extract(epoch from now()) * 1000) / 1000), coalesce((row_data->>'version')::bigint,1), p_device_id, p_device_id, case when row_data->>'deleted_at' is null then null else to_timestamp((row_data->>'deleted_at')::double precision / 1000) end)
    on conflict(id) do update set subject_id = excluded.subject_id, parent_id = excluded.parent_id, title = excluded.title, status = excluded.status, planned_date = excluded.planned_date, planned_time = excluded.planned_time, estimate_minutes = excluded.estimate_minutes, note = excluded.note, project_name = excluded.project_name, solution_name = excluded.solution_name, source_ref = excluded.source_ref, sort_order = excluded.sort_order, completed_at = excluded.completed_at, updated_at = excluded.updated_at, version = excluded.version, updated_by_device_id = p_device_id, deleted_at = excluded.deleted_at;
    insert into timegenie.task_status_events(id, workspace_id, task_id, status, occurred_at, source_type, created_at)
    select (item->>'id')::uuid, p_workspace_id, p_entity_id::uuid, item->>'status', to_timestamp((item->>'occurred_at')::double precision / 1000), item->>'source_type', to_timestamp((item->>'created_at')::double precision / 1000)
    from jsonb_array_elements(coalesce(row_data->'status_events','[]'::jsonb)) item
    on conflict(id) do nothing;
    result_value := jsonb_build_object('entityType','task','entityId',p_entity_id,'version',(select version from timegenie.tasks where id = p_entity_id::uuid));
  elsif p_entity_type = 'report_template' then
    insert into timegenie.report_templates(id, workspace_id, report_type, subject_id, content, is_builtin, created_at, updated_at, version, deleted_at)
    values((row_data->>'id')::uuid, p_workspace_id, row_data->>'report_type', nullif(row_data->>'subject_id','')::uuid, row_data->>'content', coalesce((row_data->>'is_builtin')::boolean,false), to_timestamp(coalesce((row_data->>'created_at')::double precision, extract(epoch from now()) * 1000) / 1000), to_timestamp(coalesce((row_data->>'updated_at')::double precision, extract(epoch from now()) * 1000) / 1000), coalesce((row_data->>'version')::bigint,1), case when row_data->>'deleted_at' is null then null else to_timestamp((row_data->>'deleted_at')::double precision / 1000) end)
    on conflict(id) do update set content = excluded.content, is_builtin = excluded.is_builtin, updated_at = excluded.updated_at, version = excluded.version, deleted_at = excluded.deleted_at;
    result_value := jsonb_build_object('entityType','report_template','entityId',p_entity_id,'version',(select version from timegenie.report_templates where id = p_entity_id::uuid));
  elsif p_entity_type = 'time_entry' then
    insert into timegenie.time_entries(id, workspace_id, work_date, kind, source_type, state, default_task_id, label_snapshot, started_at, ended_at, duration_seconds, note, origin_unassigned_session_id, created_at, updated_at, version, created_by_device_id, updated_by_device_id, deleted_at)
    values((row_data->>'id')::uuid, p_workspace_id, (row_data->>'work_date')::date, row_data->>'kind', row_data->>'source_type', row_data->>'state', nullif(row_data->>'default_task_id','')::uuid, row_data->>'label_snapshot', to_timestamp((row_data->>'started_at')::double precision / 1000), case when row_data->>'ended_at' is null then null else to_timestamp((row_data->>'ended_at')::double precision / 1000) end, coalesce((row_data->>'duration_seconds')::bigint,0), row_data->>'note', nullif(row_data->>'origin_unassigned_session_id','')::uuid, to_timestamp(coalesce((row_data->>'created_at')::double precision, extract(epoch from now()) * 1000) / 1000), to_timestamp(coalesce((row_data->>'updated_at')::double precision, extract(epoch from now()) * 1000) / 1000), coalesce((row_data->>'version')::bigint,1), p_device_id, p_device_id, case when row_data->>'deleted_at' is null then null else to_timestamp((row_data->>'deleted_at')::double precision / 1000) end)
    on conflict(id) do update set work_date = excluded.work_date, kind = excluded.kind, source_type = excluded.source_type, state = excluded.state, default_task_id = excluded.default_task_id, label_snapshot = excluded.label_snapshot, started_at = excluded.started_at, ended_at = excluded.ended_at, duration_seconds = excluded.duration_seconds, note = excluded.note, updated_at = excluded.updated_at, version = excluded.version, updated_by_device_id = p_device_id, deleted_at = excluded.deleted_at;
    delete from timegenie.time_segments where entry_id = p_entity_id::uuid;
    insert into timegenie.time_segments(id, workspace_id, entry_id, sequence_no, started_at, ended_at, duration_seconds)
    select (item->>'id')::uuid, p_workspace_id, p_entity_id::uuid, (item->>'sequence_no')::integer, to_timestamp((item->>'started_at')::double precision / 1000), case when item->>'ended_at' is null then null else to_timestamp((item->>'ended_at')::double precision / 1000) end, coalesce((item->>'duration_seconds')::bigint,0)
    from jsonb_array_elements(coalesce(row_data->'segments','[]'::jsonb)) item;
    delete from timegenie.time_allocations where entry_id = p_entity_id::uuid;
    insert into timegenie.time_allocations(id, workspace_id, entry_id, task_id, minutes, note, created_at, updated_at, version)
    select (item->>'id')::uuid, p_workspace_id, p_entity_id::uuid, (item->>'task_id')::uuid, (item->>'minutes')::integer, item->>'note', to_timestamp((item->>'created_at')::double precision / 1000), to_timestamp((item->>'updated_at')::double precision / 1000), coalesce((item->>'version')::bigint,1)
    from jsonb_array_elements(coalesce(row_data->'allocations','[]'::jsonb)) item;
    if row_data->'work_day' is not null and row_data->'work_day' <> 'null'::jsonb then
      insert into timegenie.work_days(workspace_id, work_date, timezone, work_period_text, note, settled_at, created_at, updated_at, version)
      values(p_workspace_id, (row_data->'work_day'->>'work_date')::date, row_data->'work_day'->>'timezone', row_data->'work_day'->>'work_period_text', row_data->'work_day'->>'note', case when row_data->'work_day'->>'settled_at' is null then null else to_timestamp((row_data->'work_day'->>'settled_at')::double precision / 1000) end, to_timestamp((row_data->'work_day'->>'created_at')::double precision / 1000), to_timestamp((row_data->'work_day'->>'updated_at')::double precision / 1000), coalesce((row_data->'work_day'->>'version')::bigint,1))
      on conflict(workspace_id, work_date) do update set timezone = excluded.timezone, work_period_text = excluded.work_period_text, note = excluded.note, settled_at = excluded.settled_at, updated_at = excluded.updated_at, version = excluded.version;
    end if;
    result_value := jsonb_build_object('entityType','time_entry','entityId',p_entity_id,'version',(select version from timegenie.time_entries where id = p_entity_id::uuid));
  elsif p_entity_type = 'report' then
    insert into timegenie.reports(id, workspace_id, report_type, subject_id, period_start, period_end, reference_date, template_id, markdown_content, content_source, generation_count, input_revision_hash, generated_at, created_at, updated_at, version, created_by_device_id, updated_by_device_id, deleted_at)
    values((row_data->>'id')::uuid, p_workspace_id, row_data->>'report_type', (row_data->>'subject_id')::uuid, (row_data->>'period_start')::date, (row_data->>'period_end')::date, (row_data->>'reference_date')::date, nullif(row_data->>'template_id','')::uuid, row_data->>'markdown_content', row_data->>'content_source', coalesce((row_data->>'generation_count')::integer,1), row_data->>'input_revision_hash', case when row_data->>'generated_at' is null then null else to_timestamp((row_data->>'generated_at')::double precision / 1000) end, to_timestamp(coalesce((row_data->>'created_at')::double precision, extract(epoch from now()) * 1000) / 1000), to_timestamp(coalesce((row_data->>'updated_at')::double precision, extract(epoch from now()) * 1000) / 1000), coalesce((row_data->>'version')::bigint,1), p_device_id, p_device_id, case when row_data->>'deleted_at' is null then null else to_timestamp((row_data->>'deleted_at')::double precision / 1000) end)
    on conflict(id) do update set report_type = excluded.report_type, subject_id = excluded.subject_id, period_start = excluded.period_start, period_end = excluded.period_end, reference_date = excluded.reference_date, template_id = excluded.template_id, markdown_content = excluded.markdown_content, content_source = excluded.content_source, generation_count = excluded.generation_count, input_revision_hash = excluded.input_revision_hash, generated_at = excluded.generated_at, updated_at = excluded.updated_at, version = excluded.version, updated_by_device_id = p_device_id, deleted_at = excluded.deleted_at;
    delete from timegenie.report_tasks where report_id = p_entity_id::uuid;
    insert into timegenie.report_tasks(report_id, task_id, workspace_id, sort_order, title_snapshot, path_snapshot)
    select p_entity_id::uuid, (item->>'task_id')::uuid, p_workspace_id, coalesce((item->>'sort_order')::integer,0), item->>'title_snapshot', item->>'path_snapshot'
    from jsonb_array_elements(coalesce(row_data->'report_tasks','[]'::jsonb)) item;
    result_value := jsonb_build_object('entityType','report','entityId',p_entity_id,'version',(select version from timegenie.reports where id = p_entity_id::uuid));
  elsif p_entity_type = 'app_setting' then
    insert into timegenie.app_settings(workspace_id, key, value_json, updated_at, version)
    values(p_workspace_id, row_data->>'key', coalesce(row_data->'value_json','null'::jsonb), to_timestamp(coalesce((row_data->>'updated_at')::double precision, extract(epoch from now()) * 1000) / 1000), coalesce((row_data->>'version')::bigint,1))
    on conflict(workspace_id,key) do update set value_json = excluded.value_json, updated_at = excluded.updated_at, version = excluded.version;
    insert into timegenie.workspace_changes(workspace_id, entity_type, entity_id, operation, entity_version, changed_by_device_id)
    values(p_workspace_id, 'app_settings', p_operation_id, 'update', (select version from timegenie.app_settings where workspace_id = p_workspace_id and key = row_data->>'key'), p_device_id);
    result_value := jsonb_build_object('entityType','app_setting','entityId',row_data->>'key','version',(select version from timegenie.app_settings where workspace_id = p_workspace_id and key = row_data->>'key'));
  elsif p_entity_type = 'integration_config' then
    insert into timegenie.integration_configs(workspace_id, provider, enabled, config_json, updated_at, version)
    values(p_workspace_id, row_data->>'provider', coalesce((row_data->>'enabled')::boolean,false), coalesce(row_data->'config_json','{}'::jsonb), to_timestamp(coalesce((row_data->>'updated_at')::double precision, extract(epoch from now()) * 1000) / 1000), coalesce((row_data->>'version')::bigint,1))
    on conflict(workspace_id,provider) do update set enabled = excluded.enabled, config_json = excluded.config_json, updated_at = excluded.updated_at, version = excluded.version;
    insert into timegenie.workspace_changes(workspace_id, entity_type, entity_id, operation, entity_version, changed_by_device_id)
    values(p_workspace_id, 'integration_configs', p_operation_id, 'update', (select version from timegenie.integration_configs where workspace_id = p_workspace_id and provider = row_data->>'provider'), p_device_id);
    result_value := jsonb_build_object('entityType','integration_config','entityId',row_data->>'provider','version',(select version from timegenie.integration_configs where workspace_id = p_workspace_id and provider = row_data->>'provider'));
  elsif p_entity_type = 'external_binding' then
    insert into timegenie.external_bindings(id, workspace_id, provider, entity_type, entity_id, external_id, external_revision, last_synced_hash, last_synced_at)
    values((row_data->>'id')::uuid, p_workspace_id, row_data->>'provider', row_data->>'entity_type', (row_data->>'entity_id')::uuid, row_data->>'external_id', row_data->>'external_revision', row_data->>'last_synced_hash', case when row_data->>'last_synced_at' is null then null else to_timestamp((row_data->>'last_synced_at')::double precision / 1000) end)
    on conflict(workspace_id, provider, entity_type, entity_id) do update set external_id = excluded.external_id, external_revision = excluded.external_revision, last_synced_hash = excluded.last_synced_hash, last_synced_at = excluded.last_synced_at;
    insert into timegenie.workspace_changes(workspace_id, entity_type, entity_id, operation, entity_version, changed_by_device_id)
    values(p_workspace_id, 'external_bindings', p_operation_id, 'update', 1, p_device_id);
    result_value := jsonb_build_object('entityType','external_binding','entityId',row_data->>'entity_id','version',1);
  else
    raise exception 'UNSUPPORTED_OPERATION: %', p_entity_type;
  end if;

  perform timegenie.record_processed_operation(
    p_workspace_id,
    p_operation_id,
    p_device_id,
    p_operation_type,
    jsonb_build_object('deviceId', p_device_id, 'entityType', p_entity_type, 'entityId', p_entity_id, 'baseVersion', p_base_version, 'payload', coalesce(p_payload, '{}'::jsonb)),
    result_value
  );
  return result_value;
end $$;

do $$ begin
  alter publication supabase_realtime add table timegenie.workspace_changes;
exception when duplicate_object then null;
end $$;

-- Supabase projects normally expose new timegenie-schema objects to Data API
-- roles. Keep this schema self-contained so an anonymous client cannot read
-- business data or invoke security-definer RPCs even when project defaults
-- change. RLS remains the per-account boundary for authenticated clients.
revoke all on all tables in schema timegenie from public;
revoke all on all sequences in schema timegenie from public;
revoke execute on all functions in schema timegenie from public;

do $$
begin
  if exists(select 1 from pg_roles where rolname = 'anon') then
    grant usage on schema timegenie to anon;
    revoke all on all tables in schema timegenie from anon;
    revoke all on all sequences in schema timegenie from anon;
    revoke execute on all functions in schema timegenie from anon;
    revoke execute on function timegenie.operation_request_hash(text, jsonb) from anon;
    revoke execute on function timegenie.processed_operation_result(uuid, uuid, text, jsonb) from anon;
    revoke execute on function timegenie.record_processed_operation(uuid, uuid, uuid, text, jsonb, jsonb) from anon;
  end if;

  if exists(select 1 from pg_roles where rolname = 'authenticated') then
    grant usage on schema timegenie to authenticated;
    revoke all on all tables in schema timegenie from authenticated;
    grant select, insert on timegenie.workspaces to authenticated;
    grant select, insert, update on timegenie.devices to authenticated;
    grant select on timegenie.workspace_changes to authenticated;
    grant select on timegenie.tracking_leases to authenticated;
    grant select on timegenie.time_entries to authenticated;
    grant usage, select on all sequences in schema timegenie to authenticated;
    revoke execute on all functions in schema timegenie from authenticated;
    grant execute on function timegenie.workspace_initialize_defaults(uuid) to authenticated;
    grant execute on function timegenie.tracking_lease_acquire(uuid, uuid, uuid) to authenticated;
    grant execute on function timegenie.tracking_lease_renew(uuid, uuid, uuid) to authenticated;
    grant execute on function timegenie.tracking_lease_release(uuid, uuid, uuid) to authenticated;
    grant execute on function timegenie.unassigned_tick(uuid, uuid, uuid) to authenticated;
    grant execute on function timegenie.timer_start(uuid, uuid, uuid, uuid, text) to authenticated;
    grant execute on function timegenie.timer_pause(uuid, uuid, uuid, bigint, uuid) to authenticated;
    grant execute on function timegenie.timer_resume(uuid, uuid, uuid, bigint, uuid) to authenticated;
    grant execute on function timegenie.timer_stop(uuid, uuid, uuid, bigint, uuid) to authenticated;
    grant execute on function timegenie.unassigned_get_state(uuid) to authenticated;
    grant execute on function timegenie.unassigned_resolve_work(uuid, uuid, uuid, bigint, jsonb, uuid) to authenticated;
    grant execute on function timegenie.unassigned_resolve_break(uuid, uuid, uuid, bigint, jsonb, uuid) to authenticated;
    grant execute on function timegenie.unassigned_discard(uuid, uuid, uuid, bigint, jsonb, uuid) to authenticated;
    grant execute on function timegenie.cloud_snapshot_get(uuid) to authenticated;
    grant execute on function timegenie.migration_import_snapshot(uuid, uuid, uuid, jsonb) to authenticated;
    grant execute on function timegenie.cloud_apply_patch(uuid, uuid, uuid, text, text, text, bigint, jsonb) to authenticated;
    revoke execute on function timegenie.operation_request_hash(text, jsonb) from authenticated;
    revoke execute on function timegenie.is_workspace_owner(uuid) from authenticated;
    revoke execute on function timegenie.is_registered_device(uuid, uuid) from authenticated;
    revoke execute on function timegenie.touch_workspace_change() from authenticated;
    revoke execute on function timegenie.processed_operation_result(uuid, uuid, text, jsonb) from authenticated;
    revoke execute on function timegenie.record_processed_operation(uuid, uuid, uuid, text, jsonb, jsonb) from authenticated;
    revoke execute on function timegenie.unassigned_resolve_non_work(uuid, uuid, uuid, bigint, uuid, text) from authenticated;
  end if;

  if exists(select 1 from pg_roles where rolname = 'service_role') then
    grant usage on schema timegenie to service_role;
    grant all privileges on all tables in schema timegenie to service_role;
    grant all privileges on all sequences in schema timegenie to service_role;
    grant execute on all functions in schema timegenie to service_role;
  end if;
end $$;
