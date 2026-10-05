-- TimeGenie incremental patch for cloud daily estimates and task-scoped recurrence changes.
-- Run after 20261005_timegenie_timer_sync_coordination_patch.sql.

begin;

do $$
begin
  if to_regprocedure('timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,jsonb)') is null then
    raise exception 'TIMEGENIE_CLOUD_PATCH_MISSING: deploy the earlier cloud sync patches first';
  end if;
  if to_regclass('timegenie.task_daily_estimates') is null
     or to_regclass('timegenie.task_recurrence_rules') is null then
    raise exception 'TIMEGENIE_PLANNING_SCHEMA_MISSING: deploy the recurring schema before this patch';
  end if;
  if to_regprocedure('timegenie.cloud_apply_patch_legacy(uuid,uuid,uuid,text,text,text,bigint,jsonb)') is null then
    alter function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,jsonb)
      rename to cloud_apply_patch_legacy;
  end if;
end $$;

create or replace function timegenie.touch_workspace_change()
returns trigger language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare row_image jsonb;
declare changed_entity_id text;
begin
  row_image := to_jsonb(coalesce(new, old));
  changed_entity_id := case tg_table_name
    when 'task_daily_estimates' then (row_image->>'task_id') || '|' || (row_image->>'work_date')
    when 'task_recurrence_rules' then row_image->>'task_id'
    when 'task_occurrences' then (row_image->>'task_id') || '|' || (row_image->>'occurrence_date')
    else coalesce(row_image->>'id', row_image->>'task_id')
  end;
  insert into timegenie.workspace_changes(workspace_id, entity_type, entity_id, operation, entity_version, changed_at)
  values (
    (row_image->>'workspace_id')::uuid,
    tg_table_name,
    changed_entity_id,
    lower(tg_op),
    coalesce((row_image->>'version')::bigint, 1),
    now()
  );
  return coalesce(new, old);
end $$;

create or replace function timegenie.cloud_apply_task_planning_patch(
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
declare
  cached jsonb;
  result_value jsonb;
  row_data jsonb := coalesce(p_payload, '{}'::jsonb);
  payload_version bigint;
  current_version bigint;
  current_task_version bigint;
  current_rule_id uuid;
  current_rule_version bigint;
  current_rule_start date;
  target_rule jsonb;
  expected_rule_count integer;
  actual_rule_count integer;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not exists(
    select 1 from timegenie.devices
    where id = p_device_id and workspace_id = p_workspace_id and revoked_at is null
  ) then
    raise exception 'DEVICE_NOT_REGISTERED';
  end if;
  if p_entity_type not in ('task_daily_estimate', 'task_recurrence_rule') then
    raise exception 'UNSUPPORTED_OPERATION: %', p_entity_type;
  end if;

  perform pg_advisory_xact_lock(hashtextextended(
    p_workspace_id::text || ':' || p_entity_type || ':' || coalesce(p_entity_id, ''), 0
  ));
  cached := timegenie.processed_operation_result(
    p_workspace_id,
    p_operation_id,
    p_operation_type,
    jsonb_build_object(
      'deviceId', p_device_id,
      'entityType', p_entity_type,
      'entityId', p_entity_id,
      'baseVersion', p_base_version,
      'payload', row_data
    )
  );
  if cached is not null then return cached; end if;

  if p_entity_type = 'task_daily_estimate' then
    if p_entity_id is null
       or row_data->>'task_id' is null
       or row_data->>'work_date' is null
       or p_entity_id is distinct from (row_data->>'task_id') || '|' || (row_data->>'work_date') then
      raise exception 'VALIDATION_ERROR: entity id does not match task daily estimate';
    end if;
    if not exists(
      select 1 from timegenie.tasks
      where workspace_id = p_workspace_id and id = (row_data->>'task_id')::uuid and deleted_at is null
    ) then
      raise exception 'NOT_FOUND: task does not exist';
    end if;
    if not coalesce((row_data->>'deleted')::boolean, false)
       and coalesce((row_data->>'estimate_minutes')::integer, 0) <= 0 then
      raise exception 'VALIDATION_ERROR: daily estimate minutes must be positive';
    end if;
    select version into current_version
    from timegenie.task_daily_estimates
    where workspace_id = p_workspace_id
      and task_id = (row_data->>'task_id')::uuid
      and work_date = (row_data->>'work_date')::date;
    payload_version := coalesce((row_data->>'version')::bigint, 1);
    if coalesce((row_data->>'deleted')::boolean, false) and current_version is null then
      null;
    elsif p_base_version is null and current_version is not null then
      raise exception 'SYNC_CONFLICT: expected new entity, current version %', current_version;
    elsif p_base_version is not null and current_version is distinct from p_base_version then
      raise exception 'SYNC_CONFLICT: expected version %, current version %', p_base_version, current_version;
    elsif p_base_version is not null and payload_version <> p_base_version + 1 then
      raise exception 'VERSION_CONFLICT: payload version % must follow base version %', payload_version, p_base_version;
    elsif p_base_version is null and payload_version <> 1 then
      raise exception 'VERSION_CONFLICT: new entity payload version must be 1';
    end if;

    if coalesce((row_data->>'deleted')::boolean, false) then
      delete from timegenie.task_daily_estimates
      where workspace_id = p_workspace_id
        and task_id = (row_data->>'task_id')::uuid
        and work_date = (row_data->>'work_date')::date;
      result_value := jsonb_build_object(
        'entityType', p_entity_type,
        'entityId', p_entity_id,
        'version', payload_version,
        'deleted', true
      );
    else
      insert into timegenie.task_daily_estimates(
        workspace_id, task_id, work_date, estimate_minutes, created_at, updated_at, version
      ) values (
        p_workspace_id,
        (row_data->>'task_id')::uuid,
        (row_data->>'work_date')::date,
        (row_data->>'estimate_minutes')::integer,
        to_timestamp(coalesce((row_data->>'created_at')::double precision, extract(epoch from now()) * 1000) / 1000),
        to_timestamp(coalesce((row_data->>'updated_at')::double precision, extract(epoch from now()) * 1000) / 1000),
        payload_version
      )
      on conflict(task_id, work_date) do update
      set estimate_minutes = excluded.estimate_minutes,
          updated_at = excluded.updated_at,
          version = excluded.version;
      result_value := jsonb_build_object(
        'entityType', p_entity_type,
        'entityId', p_entity_id,
        'version', payload_version
      );
    end if;
  else
    if p_entity_id is null
       or row_data->>'task_id' is distinct from p_entity_id
       or coalesce(row_data->>'action','') not in ('save', 'close')
       or jsonb_typeof(row_data->'rules') is distinct from 'array' then
      raise exception 'VALIDATION_ERROR: invalid task recurrence rule operation';
    end if;
    select version into current_task_version
    from timegenie.tasks
    where workspace_id = p_workspace_id
      and id = (row_data->>'task_id')::uuid
      and deleted_at is null;
    if current_task_version is null then raise exception 'NOT_FOUND: task does not exist'; end if;
    if current_task_version is distinct from (row_data->>'task_expected_version')::bigint then
      raise exception 'SYNC_CONFLICT: expected task version %, current version %', row_data->>'task_expected_version', current_task_version;
    end if;
    if exists(
      select 1 from timegenie.tasks
      where workspace_id = p_workspace_id
        and parent_id = (row_data->>'task_id')::uuid
        and deleted_at is null
    ) then
      raise exception 'TASK_NOT_LEAF: only leaf tasks can recur';
    end if;
    if exists(
      select 1 from timegenie.tasks
      where workspace_id = p_workspace_id
        and id = (row_data->>'task_id')::uuid
        and status = 'done'
    ) then
      raise exception 'TASK_COMPLETED: completed tasks cannot recur';
    end if;
    select id, version, effective_start
      into current_rule_id, current_rule_version, current_rule_start
    from timegenie.task_recurrence_rules
    where workspace_id = p_workspace_id
      and task_id = (row_data->>'task_id')::uuid
      and effective_end is null
    order by effective_start desc limit 1;
    if p_base_version is null and current_rule_version is not null then
      raise exception 'SYNC_CONFLICT: expected no active recurrence rule, current version %', current_rule_version;
    elsif p_base_version is not null and current_rule_version is distinct from p_base_version then
      raise exception 'SYNC_CONFLICT: expected recurrence version %, current version %', p_base_version, current_rule_version;
    end if;
    if (row_data->>'task_version')::bigint <> current_task_version + 1 then
      raise exception 'VERSION_CONFLICT: recurrence task version must follow current task version';
    end if;

    if row_data->>'action' = 'save' then
      if coalesce(row_data->>'frequency','') not in ('daily','weekdays','weekly') then
        raise exception 'VALIDATION_ERROR: invalid recurrence frequency';
      end if;
      if row_data->>'frequency' = 'weekly' then
        if coalesce((row_data->>'weekdays_mask')::integer, 0) not between 1 and 127 then
          raise exception 'VALIDATION_ERROR: weekly recurrence requires weekdays mask';
        end if;
      elsif row_data->'weekdays_mask' is not null and row_data->'weekdays_mask' <> 'null'::jsonb then
        raise exception 'VALIDATION_ERROR: non-weekly recurrence must not include weekdays mask';
      end if;
      target_rule := (
        select value from jsonb_array_elements(row_data->'rules') value
        where value->>'id' = row_data->>'rule_id' limit 1
      );
      if target_rule is null
         or target_rule->>'task_id' is distinct from row_data->>'task_id'
         or target_rule->>'frequency' is distinct from row_data->>'frequency'
         or target_rule->>'effective_start' is distinct from row_data->>'effective_start'
         or target_rule->>'version' is distinct from row_data->>'version'
         or coalesce(target_rule->>'weekdays_mask','') is distinct from coalesce(row_data->>'weekdays_mask','')
         or target_rule->>'effective_end' is not null then
        raise exception 'VALIDATION_ERROR: recurrence save result does not match rule snapshot';
      end if;
      if current_rule_id is null and (row_data->>'version')::bigint <> 1 then
        raise exception 'VERSION_CONFLICT: new recurrence rule version must be 1';
      elsif current_rule_id is not null and current_rule_id::text = row_data->>'rule_id'
            and (row_data->>'version')::bigint <> current_rule_version + 1 then
        raise exception 'VERSION_CONFLICT: updated recurrence rule version must follow current version';
      elsif current_rule_id is not null and current_rule_id::text <> row_data->>'rule_id'
            and (row_data->>'version')::bigint <> 1 then
        raise exception 'VERSION_CONFLICT: replacement recurrence rule version must be 1';
      end if;
    else
      if current_rule_id is null or row_data->>'rule_id' is distinct from current_rule_id::text then
        raise exception 'SYNC_CONFLICT: active recurrence rule changed';
      end if;
      if (row_data->>'version')::bigint <> current_rule_version + 1 then
        raise exception 'VERSION_CONFLICT: closed recurrence rule version must follow current version';
      end if;
      if exists(
        select 1 from jsonb_array_elements(row_data->'rules') value
        where value->>'id' = current_rule_id::text
          and (value->>'effective_end' is distinct from row_data->>'effective_end'
               or (value->>'version')::bigint <> (row_data->>'version')::bigint)
      ) then
        raise exception 'VALIDATION_ERROR: recurrence close result does not match rule snapshot';
      end if;
    end if;
    if exists(
      select 1 from jsonb_array_elements(row_data->'rules') value
      where value->>'task_id' is distinct from row_data->>'task_id'
    ) then
      raise exception 'VALIDATION_ERROR: recurrence rule task id mismatch';
    end if;

    if row_data->>'action' = 'save' then
      if current_rule_id is null then
        insert into timegenie.task_recurrence_rules(
          id, workspace_id, task_id, frequency, weekdays_mask, effective_start,
          effective_end, created_at, updated_at, version
        ) values (
          (target_rule->>'id')::uuid, p_workspace_id, (target_rule->>'task_id')::uuid,
          target_rule->>'frequency', nullif(target_rule->>'weekdays_mask','')::integer,
          (target_rule->>'effective_start')::date, null,
          to_timestamp(coalesce((target_rule->>'created_at')::double precision, extract(epoch from now()) * 1000) / 1000),
          to_timestamp(coalesce((target_rule->>'updated_at')::double precision, extract(epoch from now()) * 1000) / 1000),
          (target_rule->>'version')::bigint
        );
      elsif current_rule_id::text = row_data->>'rule_id' then
        update timegenie.task_recurrence_rules
        set frequency = target_rule->>'frequency',
            weekdays_mask = nullif(target_rule->>'weekdays_mask','')::integer,
            effective_start = (target_rule->>'effective_start')::date,
            updated_at = to_timestamp((target_rule->>'updated_at')::double precision / 1000),
            version = (target_rule->>'version')::bigint
        where workspace_id = p_workspace_id
          and id = current_rule_id
          and version = current_rule_version
          and effective_end is null;
        if not found then raise exception 'SYNC_CONFLICT: active recurrence rule changed while applying save'; end if;
      else
        if not exists(
          select 1 from jsonb_array_elements(row_data->'rules') value
          where value->>'id' = current_rule_id::text
            and (value->>'version')::bigint = current_rule_version + 1
            and (value->>'effective_end')::date = (target_rule->>'effective_start')::date - 1
        ) then
          raise exception 'VALIDATION_ERROR: replacement recurrence history does not close the active rule';
        end if;
        update timegenie.task_recurrence_rules
        set effective_end = (
              select (value->>'effective_end')::date from jsonb_array_elements(row_data->'rules') value
              where value->>'id' = current_rule_id::text
            ),
            updated_at = to_timestamp((
              select (value->>'updated_at')::double precision from jsonb_array_elements(row_data->'rules') value
              where value->>'id' = current_rule_id::text
            ) / 1000),
            version = current_rule_version + 1
        where workspace_id = p_workspace_id
          and id = current_rule_id
          and version = current_rule_version
          and effective_end is null;
        if not found then raise exception 'SYNC_CONFLICT: active recurrence rule changed while applying replacement'; end if;
        insert into timegenie.task_recurrence_rules(
          id, workspace_id, task_id, frequency, weekdays_mask, effective_start,
          effective_end, created_at, updated_at, version
        ) values (
          (target_rule->>'id')::uuid, p_workspace_id, (target_rule->>'task_id')::uuid,
          target_rule->>'frequency', nullif(target_rule->>'weekdays_mask','')::integer,
          (target_rule->>'effective_start')::date, null,
          to_timestamp(coalesce((target_rule->>'created_at')::double precision, extract(epoch from now()) * 1000) / 1000),
          to_timestamp(coalesce((target_rule->>'updated_at')::double precision, extract(epoch from now()) * 1000) / 1000),
          (target_rule->>'version')::bigint
        );
      end if;
    elsif (row_data->>'effective_end')::date < current_rule_start then
      delete from timegenie.task_recurrence_rules
      where workspace_id = p_workspace_id
        and id = current_rule_id
        and version = current_rule_version
        and effective_end is null;
      if not found then raise exception 'SYNC_CONFLICT: active recurrence rule changed while applying close'; end if;
      if exists(
        select 1 from jsonb_array_elements(row_data->'rules') value
        where value->>'id' = current_rule_id::text
      ) then
        raise exception 'VALIDATION_ERROR: future recurrence close must remove the rule';
      end if;
    else
      update timegenie.task_recurrence_rules
      set effective_end = (row_data->>'effective_end')::date,
          updated_at = now(),
          version = (row_data->>'version')::bigint
      where workspace_id = p_workspace_id
        and id = current_rule_id
        and version = current_rule_version
        and effective_end is null;
      if not found then raise exception 'SYNC_CONFLICT: active recurrence rule changed while applying close'; end if;
    end if;

    update timegenie.tasks
    set status = 'open', completed_at = null, updated_at = now(),
        version = (row_data->>'task_version')::bigint, updated_by_device_id = p_device_id
    where workspace_id = p_workspace_id
      and id = (row_data->>'task_id')::uuid
      and version = current_task_version;
    if not found then raise exception 'SYNC_CONFLICT: task changed while applying recurrence rule'; end if;

    select count(*) into expected_rule_count from jsonb_array_elements(row_data->'rules');
    select count(*) into actual_rule_count
    from timegenie.task_recurrence_rules
    where workspace_id = p_workspace_id and task_id = (row_data->>'task_id')::uuid;
    if expected_rule_count <> actual_rule_count or exists(
      select 1
      from jsonb_array_elements(row_data->'rules') expected
      left join timegenie.task_recurrence_rules actual
        on actual.workspace_id = p_workspace_id and actual.id = (expected->>'id')::uuid
      where actual.id is null
         or actual.task_id::text is distinct from expected->>'task_id'
         or actual.frequency is distinct from expected->>'frequency'
         or coalesce(actual.weekdays_mask::text,'') is distinct from coalesce(expected->>'weekdays_mask','')
         or actual.effective_start::text is distinct from expected->>'effective_start'
         or coalesce(actual.effective_end::text,'') is distinct from coalesce(expected->>'effective_end','')
         or actual.version <> (expected->>'version')::bigint
    ) then
      raise exception 'VALIDATION_ERROR: recurrence rule snapshot does not match applied result';
    end if;
    result_value := jsonb_build_object(
      'entityType', p_entity_type,
      'entityId', p_entity_id,
      'version', coalesce((row_data->>'version')::bigint, 0),
      'taskVersion', (row_data->>'task_version')::bigint,
      'rules', row_data->'rules'
    );
  end if;

  perform timegenie.record_processed_operation(
    p_workspace_id,
    p_operation_id,
    p_device_id,
    p_operation_type,
    jsonb_build_object(
      'deviceId', p_device_id,
      'entityType', p_entity_type,
      'entityId', p_entity_id,
      'baseVersion', p_base_version,
      'payload', row_data
    ),
    result_value
  );
  return result_value;
end $$;

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
begin
  if p_entity_type in ('task_daily_estimate', 'task_recurrence_rule') then
    return timegenie.cloud_apply_task_planning_patch(
      p_workspace_id, p_device_id, p_operation_id, p_operation_type,
      p_entity_type, p_entity_id, p_base_version, p_payload
    );
  end if;
  return timegenie.cloud_apply_patch_legacy(
    p_workspace_id, p_device_id, p_operation_id, p_operation_type,
    p_entity_type, p_entity_id, p_base_version, p_payload
  );
end $$;

revoke all on function timegenie.cloud_apply_task_planning_patch(uuid,uuid,uuid,text,text,text,bigint,jsonb) from public, anon, authenticated;
grant execute on function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,jsonb) to authenticated;

do $$
begin
  if exists(select 1 from pg_roles where rolname = 'service_role') then
    grant execute on function timegenie.cloud_apply_task_planning_patch(uuid,uuid,uuid,text,text,text,bigint,jsonb) to service_role;
  end if;
end $$;

commit;
