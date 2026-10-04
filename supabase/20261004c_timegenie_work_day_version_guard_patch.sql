-- TimeGenie Supabase incremental patch for local-first work-day version guards.
-- Use this for existing Supabase projects after 20261004b_timegenie_migration_target_guard_patch.sql.
-- For a fresh Supabase project, run supabase/schema.sql instead.

begin;

do $$
begin
  if to_regclass('timegenie.workspaces') is null
     or to_regclass('timegenie.processed_operations') is null
     or to_regclass('timegenie.sync_outbox') is not null then
    raise exception 'TIMEGENIE_SCHEMA_CHECK_FAILED: run this in the Supabase timegenie schema, not the local SQLite schema';
  end if;
  if to_regclass('timegenie.task_occurrences') is null then
    raise exception 'TIMEGENIE_RECURRING_PATCH_MISSING: run 20261003_timegenie_recurring_migration_patch.sql first';
  end if;
end $$;

-- Apply one complete entity image from the desktop outbox. This version keeps
-- operation idempotency, validates entity identity, preserves unassigned source
-- links, and supports unassigned_session payloads with stale-version rejection.
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
declare current_version bigint; cached jsonb; result_value jsonb; row_data jsonb; payload_version bigint;
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

  row_data := coalesce(p_payload, '{}'::jsonb);

  if p_entity_type in ('subject','task','time_entry','unassigned_session','report','report_template') then
    if p_entity_id is null or row_data->>'id' is distinct from p_entity_id then
      raise exception 'VALIDATION_ERROR: entity id does not match payload id';
    end if;
  elsif p_entity_type = 'app_setting' then
    if p_entity_id is null or row_data->>'key' is distinct from p_entity_id then
      raise exception 'VALIDATION_ERROR: entity id does not match setting key';
    end if;
  elsif p_entity_type = 'integration_config' then
    if p_entity_id is null or row_data->>'provider' is distinct from p_entity_id then
      raise exception 'VALIDATION_ERROR: entity id does not match integration provider';
    end if;
  elsif p_entity_type = 'task_occurrence' then
    if p_entity_id is null or p_entity_id is distinct from (row_data->>'task_id') || '|' || (row_data->>'occurrence_date') then
      raise exception 'VALIDATION_ERROR: entity id does not match task occurrence';
    end if;
  elsif p_entity_type = 'external_binding' then
    if p_entity_id is null or p_entity_id is distinct from (row_data->>'provider') || '|' || (row_data->>'entity_type') || '|' || (row_data->>'entity_id') then
      raise exception 'VALIDATION_ERROR: entity id does not match external binding identity';
    end if;
  end if;

  if p_entity_type in ('subject','task','time_entry','report','report_template','app_setting','integration_config','task_occurrence') then
    case p_entity_type
      when 'subject' then select version into current_version from timegenie.subjects where workspace_id = p_workspace_id and id = p_entity_id::uuid;
      when 'task' then select version into current_version from timegenie.tasks where workspace_id = p_workspace_id and id = p_entity_id::uuid;
      when 'time_entry' then select version into current_version from timegenie.time_entries where workspace_id = p_workspace_id and id = p_entity_id::uuid;
      when 'report' then select version into current_version from timegenie.reports where workspace_id = p_workspace_id and id = p_entity_id::uuid;
      when 'report_template' then select version into current_version from timegenie.report_templates where workspace_id = p_workspace_id and id = p_entity_id::uuid;
      when 'app_setting' then select version into current_version from timegenie.app_settings where workspace_id = p_workspace_id and key = row_data->>'key';
      when 'integration_config' then select version into current_version from timegenie.integration_configs where workspace_id = p_workspace_id and provider = row_data->>'provider';
      when 'task_occurrence' then select version into current_version from timegenie.task_occurrences where workspace_id = p_workspace_id and task_id = (row_data->>'task_id')::uuid and occurrence_date = (row_data->>'occurrence_date')::date;
      else current_version := p_base_version;
    end case;

    payload_version := coalesce((row_data->>'version')::bigint, 1);
    if p_base_version is null and current_version is not null then
      raise exception 'SYNC_CONFLICT: expected new entity, current version %', current_version;
    elsif p_base_version is not null and current_version is distinct from p_base_version then
      raise exception 'SYNC_CONFLICT: expected version %, current version %', p_base_version, current_version;
    elsif p_base_version is not null and payload_version <> p_base_version + 1 then
      raise exception 'VERSION_CONFLICT: payload version % must follow base version %', payload_version, p_base_version;
    elsif p_base_version is null and p_entity_type <> 'task_occurrence' and payload_version <> 1 then
      raise exception 'VERSION_CONFLICT: new entity payload version must be 1';
    end if;
  elsif p_entity_type = 'unassigned_session' then
    select version into current_version from timegenie.unassigned_sessions where workspace_id = p_workspace_id and id = p_entity_id::uuid;
    payload_version := coalesce((row_data->>'version')::bigint, 1);
    if p_base_version is null and current_version is not null then
      raise exception 'SYNC_CONFLICT: expected new unassigned session, current version %', current_version;
    elsif p_base_version is not null and current_version is distinct from p_base_version then
      raise exception 'SYNC_CONFLICT: expected unassigned session version %, current version %', p_base_version, current_version;
    elsif p_base_version is not null and payload_version <= p_base_version then
      raise exception 'VERSION_CONFLICT: unassigned session payload version % must be above base version %', payload_version, p_base_version;
    elsif current_version is not null and payload_version <= current_version then
      raise exception 'SYNC_CONFLICT: expected unassigned session version above %, payload version %', current_version, payload_version;
    end if;
  end if;

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
    on conflict(id) do update set work_date = excluded.work_date, kind = excluded.kind, source_type = excluded.source_type, state = excluded.state, default_task_id = excluded.default_task_id, label_snapshot = excluded.label_snapshot, started_at = excluded.started_at, ended_at = excluded.ended_at, duration_seconds = excluded.duration_seconds, note = excluded.note, origin_unassigned_session_id = excluded.origin_unassigned_session_id, updated_at = excluded.updated_at, version = excluded.version, updated_by_device_id = p_device_id, deleted_at = excluded.deleted_at;
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
      on conflict(workspace_id, work_date) do update set timezone = excluded.timezone, work_period_text = excluded.work_period_text, note = excluded.note, settled_at = excluded.settled_at, updated_at = excluded.updated_at, version = excluded.version
      where timegenie.work_days.version <= excluded.version;
    end if;
    result_value := jsonb_build_object('entityType','time_entry','entityId',p_entity_id,'version',(select version from timegenie.time_entries where id = p_entity_id::uuid));
  elsif p_entity_type = 'unassigned_session' then
    insert into timegenie.unassigned_sessions(id, workspace_id, work_date, state, threshold_seconds, duration_seconds, first_started_at, last_ended_at, prompted_at, resolution_type, generated_entry_id, resolved_at, created_at, updated_at, version)
    values((row_data->>'id')::uuid, p_workspace_id, (row_data->>'work_date')::date, row_data->>'state', coalesce((row_data->>'threshold_seconds')::integer,300), coalesce((row_data->>'duration_seconds')::bigint,0), to_timestamp((row_data->>'first_started_at')::double precision / 1000), case when row_data->>'last_ended_at' is null then null else to_timestamp((row_data->>'last_ended_at')::double precision / 1000) end, case when row_data->>'prompted_at' is null then null else to_timestamp((row_data->>'prompted_at')::double precision / 1000) end, row_data->>'resolution_type', nullif(row_data->>'generated_entry_id','')::uuid, case when row_data->>'resolved_at' is null then null else to_timestamp((row_data->>'resolved_at')::double precision / 1000) end, to_timestamp(coalesce((row_data->>'created_at')::double precision, extract(epoch from now()) * 1000) / 1000), to_timestamp(coalesce((row_data->>'updated_at')::double precision, extract(epoch from now()) * 1000) / 1000), coalesce((row_data->>'version')::bigint,1))
    on conflict(id) do update set work_date = excluded.work_date, state = excluded.state, threshold_seconds = excluded.threshold_seconds, duration_seconds = excluded.duration_seconds, first_started_at = excluded.first_started_at, last_ended_at = excluded.last_ended_at, prompted_at = excluded.prompted_at, resolution_type = excluded.resolution_type, generated_entry_id = excluded.generated_entry_id, resolved_at = excluded.resolved_at, updated_at = excluded.updated_at, version = excluded.version;
    delete from timegenie.unassigned_segments where session_id = p_entity_id::uuid;
    insert into timegenie.unassigned_segments(id, workspace_id, session_id, sequence_no, started_at, ended_at, duration_seconds, lease_token)
    select (item->>'id')::uuid, p_workspace_id, p_entity_id::uuid, (item->>'sequence_no')::integer, to_timestamp((item->>'started_at')::double precision / 1000), case when item->>'ended_at' is null then null else to_timestamp((item->>'ended_at')::double precision / 1000) end, coalesce((item->>'duration_seconds')::bigint,0), nullif(item->>'lease_token','')::uuid
    from jsonb_array_elements(coalesce(row_data->'segments','[]'::jsonb)) item;
    result_value := jsonb_build_object('entityType','unassigned_session','entityId',p_entity_id,'version',(select version from timegenie.unassigned_sessions where id = p_entity_id::uuid));
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
    values(p_workspace_id, 'app_settings', row_data->>'key', 'update', (select version from timegenie.app_settings where workspace_id = p_workspace_id and key = row_data->>'key'), p_device_id);
    result_value := jsonb_build_object('entityType','app_setting','entityId',row_data->>'key','version',(select version from timegenie.app_settings where workspace_id = p_workspace_id and key = row_data->>'key'));
  elsif p_entity_type = 'integration_config' then
    insert into timegenie.integration_configs(workspace_id, provider, enabled, config_json, updated_at, version)
    values(p_workspace_id, row_data->>'provider', coalesce((row_data->>'enabled')::boolean,false), coalesce(row_data->'config_json','{}'::jsonb), to_timestamp(coalesce((row_data->>'updated_at')::double precision, extract(epoch from now()) * 1000) / 1000), coalesce((row_data->>'version')::bigint,1))
    on conflict(workspace_id,provider) do update set enabled = excluded.enabled, config_json = excluded.config_json, updated_at = excluded.updated_at, version = excluded.version;
    insert into timegenie.workspace_changes(workspace_id, entity_type, entity_id, operation, entity_version, changed_by_device_id)
    values(p_workspace_id, 'integration_configs', row_data->>'provider', 'update', (select version from timegenie.integration_configs where workspace_id = p_workspace_id and provider = row_data->>'provider'), p_device_id);
    result_value := jsonb_build_object('entityType','integration_config','entityId',row_data->>'provider','version',(select version from timegenie.integration_configs where workspace_id = p_workspace_id and provider = row_data->>'provider'));
  elsif p_entity_type = 'task_occurrence' then
    insert into timegenie.task_occurrences(workspace_id, task_id, occurrence_date, origin, status, completed_at, created_at, updated_at, version)
    values(p_workspace_id, (row_data->>'task_id')::uuid, (row_data->>'occurrence_date')::date, row_data->>'origin', row_data->>'status', case when row_data->>'completed_at' is null then null else to_timestamp((row_data->>'completed_at')::double precision / 1000) end, to_timestamp(coalesce((row_data->>'created_at')::double precision, extract(epoch from now()) * 1000) / 1000), to_timestamp(coalesce((row_data->>'updated_at')::double precision, extract(epoch from now()) * 1000) / 1000), coalesce((row_data->>'version')::bigint,1))
    on conflict(task_id, occurrence_date) do update set origin = excluded.origin, status = excluded.status, completed_at = excluded.completed_at, updated_at = excluded.updated_at, version = excluded.version;
    result_value := jsonb_build_object('entityType','task_occurrence','entityId',p_entity_id,'version',(select version from timegenie.task_occurrences where workspace_id = p_workspace_id and task_id = (row_data->>'task_id')::uuid and occurrence_date = (row_data->>'occurrence_date')::date));
  elsif p_entity_type = 'external_binding' then
    insert into timegenie.external_bindings(id, workspace_id, provider, entity_type, entity_id, external_id, external_revision, last_synced_hash, last_synced_at)
    values((row_data->>'id')::uuid, p_workspace_id, row_data->>'provider', row_data->>'entity_type', (row_data->>'entity_id')::uuid, row_data->>'external_id', row_data->>'external_revision', row_data->>'last_synced_hash', case when row_data->>'last_synced_at' is null then null else to_timestamp((row_data->>'last_synced_at')::double precision / 1000) end)
    on conflict(workspace_id, provider, entity_type, entity_id) do update set external_id = excluded.external_id, external_revision = excluded.external_revision, last_synced_hash = excluded.last_synced_hash, last_synced_at = excluded.last_synced_at;
    insert into timegenie.workspace_changes(workspace_id, entity_type, entity_id, operation, entity_version, changed_by_device_id)
    values(p_workspace_id, 'external_bindings', p_entity_id, 'update', 1, p_device_id);
    result_value := jsonb_build_object('entityType','external_binding','entityId',p_entity_id,'version',1);
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

do $$
begin
  if exists(select 1 from pg_roles where rolname = 'anon') then
    revoke execute on function timegenie.cloud_apply_patch(uuid, uuid, uuid, text, text, text, bigint, jsonb) from anon;
  end if;
  if exists(select 1 from pg_roles where rolname = 'authenticated') then
    grant execute on function timegenie.cloud_apply_patch(uuid, uuid, uuid, text, text, text, bigint, jsonb) to authenticated;
  end if;
  if exists(select 1 from pg_roles where rolname = 'service_role') then
    grant execute on function timegenie.cloud_apply_patch(uuid, uuid, uuid, text, text, text, bigint, jsonb) to service_role;
  end if;
end $$;

commit;

notify pgrst, 'reload schema';
