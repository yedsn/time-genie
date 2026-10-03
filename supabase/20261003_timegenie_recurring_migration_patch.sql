-- TimeGenie Supabase incremental patch for recurring task migration support.
-- Use this only when an older TimeGenie Supabase schema already exists.
-- For a fresh Supabase project, run supabase/schema.sql instead.

begin;

do $$
begin
  if to_regclass('timegenie.workspaces') is null
     or to_regclass('timegenie.tasks') is null then
    raise exception 'TIMEGENIE_SCHEMA_MISSING: run supabase/schema.sql first';
  end if;
end $$;

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

alter table timegenie.task_daily_estimates enable row level security;
alter table timegenie.task_recurrence_rules enable row level security;
alter table timegenie.task_occurrences enable row level security;

do $$
declare table_name text;
begin
  foreach table_name in array array['task_daily_estimates','task_recurrence_rules','task_occurrences'] loop
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

do $$
declare table_name text;
begin
  foreach table_name in array array['subjects','tasks','task_daily_estimates','task_recurrence_rules','task_occurrences','time_entries','time_allocations','unassigned_sessions','report_templates','reports'] loop
    execute format('drop trigger if exists workspace_change_trigger on timegenie.%I', table_name);
    execute format('create trigger workspace_change_trigger after insert or update or delete on timegenie.%I for each row execute function timegenie.touch_workspace_change()', table_name);
  end loop;
end $$;

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

do $$
begin
  if exists(select 1 from pg_roles where rolname = 'anon') then
    grant usage on schema timegenie to anon;
    revoke all on timegenie.task_daily_estimates from anon;
    revoke all on timegenie.task_recurrence_rules from anon;
    revoke all on timegenie.task_occurrences from anon;
    revoke execute on function timegenie.touch_workspace_change() from anon;
    revoke execute on function timegenie.cloud_snapshot_get(uuid) from anon;
    revoke execute on function timegenie.migration_import_snapshot(uuid, uuid, uuid, jsonb) from anon;
  end if;

  if exists(select 1 from pg_roles where rolname = 'authenticated') then
    grant usage on schema timegenie to authenticated;
    revoke all on timegenie.task_daily_estimates from authenticated;
    revoke all on timegenie.task_recurrence_rules from authenticated;
    revoke all on timegenie.task_occurrences from authenticated;
    grant execute on function timegenie.cloud_snapshot_get(uuid) to authenticated;
    grant execute on function timegenie.migration_import_snapshot(uuid, uuid, uuid, jsonb) to authenticated;
    revoke execute on function timegenie.touch_workspace_change() from authenticated;
  end if;

  if exists(select 1 from pg_roles where rolname = 'service_role') then
    grant usage on schema timegenie to service_role;
    grant all privileges on timegenie.task_daily_estimates to service_role;
    grant all privileges on timegenie.task_recurrence_rules to service_role;
    grant all privileges on timegenie.task_occurrences to service_role;
    grant execute on function timegenie.touch_workspace_change() to service_role;
    grant execute on function timegenie.cloud_snapshot_get(uuid) to service_role;
    grant execute on function timegenie.migration_import_snapshot(uuid, uuid, uuid, jsonb) to service_role;
  end if;
end $$;

do $$
begin
  if to_regclass('timegenie.task_daily_estimates') is null
     or to_regclass('timegenie.task_recurrence_rules') is null
     or to_regclass('timegenie.task_occurrences') is null then
    raise exception 'TIMEGENIE_PATCH_FAILED: recurring migration tables were not created';
  end if;
end $$;

commit;

notify pgrst, 'reload schema';
