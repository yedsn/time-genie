-- Workspace-level storage protocol and the server-side barrier between the
-- legacy SQLite sync protocol and direct-v2 Supabase storage.

alter table timegenie.workspaces
  add column if not exists storage_protocol text not null default 'sync_v1';
alter table timegenie.workspaces
  drop constraint if exists workspaces_storage_protocol_check;
alter table timegenie.workspaces
  add constraint workspaces_storage_protocol_check
  check (storage_protocol in ('sync_v1','direct_v2'));

create or replace function timegenie.storage_protocol_get(p_workspace_id uuid)
returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare result_value jsonb;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then
    raise exception 'AUTH_REQUIRED';
  end if;
  select jsonb_build_object(
    'workspaceId',workspace.id,
    'storageProtocol',workspace.storage_protocol,
    'workspaceEpoch',workspace.workspace_epoch,
    'schemaGeneration',workspace.minimum_client_schema_generation,
    'serverTime',now()
  ) into result_value
  from timegenie.workspaces workspace
  where workspace.id=p_workspace_id and workspace.deleted_at is null;
  if result_value is null then raise exception 'WORKSPACE_NOT_FOUND'; end if;
  return result_value;
end $$;

create or replace function timegenie.assert_sync_v1_workspace(p_workspace_id uuid)
returns void language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare protocol_value text;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  select workspace.storage_protocol into protocol_value
  from timegenie.workspaces workspace
  where workspace.id=p_workspace_id
    and workspace.deleted_at is null;
  if protocol_value is null then raise exception 'WORKSPACE_NOT_FOUND'; end if;
  if protocol_value='direct_v2' then
    raise exception 'CLIENT_UPGRADE_REQUIRED: 云端已切换为直连存储，请升级 TimeGenie 后继续';
  end if;
end $$;

create or replace function timegenie.assert_direct_v2_workspace(
  p_workspace_id uuid,
  p_device_id uuid,
  p_expected_epoch bigint default null
) returns void language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare workspace_row timegenie.workspaces;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  perform timegenie.client_schema_generation_assert(p_workspace_id);
  select workspace.* into workspace_row
  from timegenie.workspaces workspace
  where workspace.id=p_workspace_id
    and workspace.deleted_at is null
  for share;
  if not found then raise exception 'WORKSPACE_NOT_FOUND'; end if;
  if workspace_row.storage_protocol<>'direct_v2' then
    raise exception 'CLIENT_UPGRADE_REQUIRED: 云端尚未切换到直连存储协议';
  end if;
  if not exists(
    select 1 from timegenie.devices device
    where device.id=p_device_id and device.workspace_id=p_workspace_id and device.revoked_at is null
  ) then
    raise exception 'DEVICE_REVOKED';
  end if;
  if p_expected_epoch is not null and p_expected_epoch<>workspace_row.workspace_epoch then
    raise exception 'WORKSPACE_EPOCH_STALE: expected %, current %',p_expected_epoch,workspace_row.workspace_epoch;
  end if;
end $$;

create or replace function timegenie.direct_cloud_context(
  p_workspace_id uuid,
  p_device_id uuid,
  p_expected_epoch bigint default null
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare workspace_row timegenie.workspaces; latest_change_seq bigint;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  select workspace.* into workspace_row from timegenie.workspaces workspace
  where workspace.id=p_workspace_id and workspace.deleted_at is null;
  select coalesce(max(change_seq),0) into latest_change_seq
  from timegenie.workspace_changes where workspace_id=p_workspace_id;
  return jsonb_build_object(
    'workspaceId',workspace_row.id,
    'deviceId',p_device_id,
    'storageProtocol',workspace_row.storage_protocol,
    'workspaceEpoch',workspace_row.workspace_epoch,
    'changeSeq',latest_change_seq,
    'schemaGeneration',workspace_row.minimum_client_schema_generation,
    'serverTime',now(),
    'writable',true
  );
end $$;

create or replace function timegenie.direct_cloud_query_metadata(
  p_workspace_id uuid,
  p_device_id uuid,
  p_expected_epoch bigint default null,
  p_domains text[] default '{}'::text[]
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare context_value jsonb;
begin
  context_value:=timegenie.direct_cloud_context(p_workspace_id,p_device_id,p_expected_epoch);
  return context_value||jsonb_build_object(
    'domains',to_jsonb(coalesce(p_domains,'{}'::text[]))
  );
end $$;

create or replace function timegenie.direct_cloud_query_response(
  p_workspace_id uuid,
  p_device_id uuid,
  p_data jsonb,
  p_domains text[] default '{}'::text[],
  p_expected_epoch bigint default null
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare response_value jsonb; started_at timestamptz:=clock_timestamp();
begin
  response_value:=jsonb_build_object(
    'metadata',timegenie.direct_cloud_query_metadata(
      p_workspace_id,p_device_id,p_expected_epoch,p_domains
    ),
    'data',coalesce(p_data,'null'::jsonb)
  );
  return jsonb_set(
    response_value,'{metadata,diagnostics}',jsonb_build_object(
      'responseBytes',octet_length(response_value::text),
      'rpcCount',1,
      'durationMs',greatest(round(extract(epoch from(clock_timestamp()-started_at))*1000,3),0)
    ),true
  );
end $$;

create or replace function timegenie.direct_cloud_sanitize_shared_json(p_value jsonb)
returns jsonb language plpgsql immutable as $$
declare result_value jsonb:='{}'::jsonb; item jsonb; pair record; normalized_key text;
begin
  if p_value is null then return null; end if;
  if jsonb_typeof(p_value)='array' then
    result_value:='[]'::jsonb;
    for item in select value from jsonb_array_elements(p_value) loop
      result_value:=result_value||jsonb_build_array(timegenie.direct_cloud_sanitize_shared_json(item));
    end loop;
    return result_value;
  elsif jsonb_typeof(p_value)<>'object' then
    return p_value;
  end if;
  for pair in select key,value from jsonb_each(p_value) order by key loop
    normalized_key:=regexp_replace(lower(pair.key),'[^a-z0-9]+','','g');
    if normalized_key ~ '(token|password|secret|authorization|apikey|managementkey|servicerole)'
       or normalized_key=any(array[
         'anonkey','leasetoken','authsessionid','createdbydeviceid','updatedbydeviceid',
         'actionconfig','actionconfigjson','command'
       ]) then
      continue;
    end if;
    result_value:=result_value||jsonb_build_object(
      pair.key,timegenie.direct_cloud_sanitize_shared_json(pair.value)
    );
  end loop;
  return result_value;
end $$;

-- Keep snapshots on the same recursive credential boundary as direct reads.
-- This closes camelCase variants such as baseApiToken and managementKey.
create or replace function timegenie.workspace_snapshot_sanitize(p_value jsonb)
returns jsonb language sql immutable as $$
  select timegenie.direct_cloud_sanitize_shared_json(p_value);
$$;

create or replace function timegenie.direct_cloud_workbench_bootstrap(
  p_workspace_id uuid,
  p_device_id uuid,
  p_expected_epoch bigint default null
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare workspace_row timegenie.workspaces%rowtype;
  current_work_date_value date;
  day_start_value timestamptz;
  day_end_value timestamptz;
  data_value jsonb;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  select workspace.* into workspace_row
  from timegenie.workspaces workspace
  where workspace.id=p_workspace_id and workspace.deleted_at is null;
  if not found then raise exception 'WORKSPACE_NOT_FOUND'; end if;

  current_work_date_value:=(now() at time zone workspace_row.timezone)::date;
  day_start_value:=current_work_date_value::timestamp at time zone workspace_row.timezone;
  day_end_value:=(current_work_date_value+1)::timestamp at time zone workspace_row.timezone;

  select jsonb_build_object(
    'workspaceId',workspace_row.id,
    'workspaceName',workspace_row.name,
    'timezone',workspace_row.timezone,
    'currentWorkDate',current_work_date_value,
    'currentDayStartAt',(extract(epoch from day_start_value)*1000)::bigint,
    'currentDayEndAt',(extract(epoch from day_end_value)*1000)::bigint,
    'subjects',coalesce((
      select jsonb_agg(jsonb_build_object(
        'id',subject.id,
        'name',subject.name,
        'sortOrder',subject.sort_order,
        'openTaskCount',(
          select count(*) from timegenie.tasks task
          where task.workspace_id=p_workspace_id
            and task.subject_id=subject.id
            and task.status='open'
            and task.deleted_at is null
        ),
        'version',subject.version
      ) order by subject.sort_order,subject.created_at,subject.id)
      from timegenie.subjects subject
      where subject.workspace_id=p_workspace_id and subject.deleted_at is null
    ),'[]'::jsonb),
    'summary',jsonb_build_object(
      'subjectCount',(select count(*) from timegenie.subjects subject where subject.workspace_id=p_workspace_id and subject.deleted_at is null),
      'openTaskCount',(select count(*) from timegenie.tasks task where task.workspace_id=p_workspace_id and task.status='open' and task.deleted_at is null),
      'completedTaskCount',(select count(*) from timegenie.tasks task where task.workspace_id=p_workspace_id and task.status='done' and task.deleted_at is null),
      'taskCount',(select count(*) from timegenie.tasks task where task.workspace_id=p_workspace_id and task.deleted_at is null)
    ),
    'device',(
      select jsonb_build_object(
        'id',device.id,
        'deviceName',device.device_name,
        'platform',device.platform,
        'appVersion',device.app_version,
        'lastSeenAt',device.last_seen_at,
        'authorizedAt',device.authorized_at,
        'reauthorizedAt',device.reauthorized_at,
        'writable',true
      )
      from timegenie.devices device
      where device.id=p_device_id and device.workspace_id=p_workspace_id and device.revoked_at is null
    )
  ) into data_value;

  return timegenie.direct_cloud_query_response(
    p_workspace_id,p_device_id,data_value,array['subjects','tasks'],p_expected_epoch
  );
end $$;

create or replace function timegenie.direct_cloud_task_project(
  p_workspace_id uuid,
  p_task_id uuid
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare task_row timegenie.tasks%rowtype;
  path_value text;
  depth_value integer;
  direct_minutes_value bigint;
  total_minutes_value bigint;
  has_children_value boolean;
  active_state_value text;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  select task.* into task_row from timegenie.tasks task
  where task.workspace_id=p_workspace_id and task.id=p_task_id and task.deleted_at is null;
  if not found then return null; end if;

  with recursive ancestors as (
    select task.id,task.parent_id,task.title,0 as distance
    from timegenie.tasks task where task.workspace_id=p_workspace_id and task.id=p_task_id
    union all
    select parent.id,parent.parent_id,parent.title,child.distance+1
    from timegenie.tasks parent join ancestors child on child.parent_id=parent.id
    where parent.workspace_id=p_workspace_id and parent.deleted_at is null
  ) select string_agg(title,' / ' order by distance desc),count(*)-1
    into path_value,depth_value from ancestors;

  with recursive subtree as (
    select task.id from timegenie.tasks task
    where task.workspace_id=p_workspace_id and task.id=p_task_id and task.deleted_at is null
    union all
    select child.id from timegenie.tasks child join subtree parent on child.parent_id=parent.id
    where child.workspace_id=p_workspace_id and child.deleted_at is null
  ), minutes as (
    select allocation.task_id,sum(allocation.minutes)::bigint as value
    from timegenie.time_allocations allocation
    join timegenie.time_entries entry on entry.id=allocation.entry_id
    where allocation.workspace_id=p_workspace_id and entry.deleted_at is null
      and allocation.task_id in(select id from subtree) group by allocation.task_id
    union all
    select adjustment.task_id,sum(adjustment.delta_minutes)::bigint
    from timegenie.actual_time_adjustments adjustment
    where adjustment.workspace_id=p_workspace_id and adjustment.deleted_at is null
      and adjustment.task_id in(select id from subtree) group by adjustment.task_id
  ) select greatest(coalesce(sum(value) filter(where task_id=p_task_id),0),0),
           greatest(coalesce(sum(value),0),0)
    into direct_minutes_value,total_minutes_value from minutes;

  select exists(select 1 from timegenie.tasks child where child.workspace_id=p_workspace_id and child.parent_id=p_task_id and child.deleted_at is null)
    into has_children_value;
  select entry.state into active_state_value from timegenie.time_entries entry
  where entry.workspace_id=p_workspace_id and entry.default_task_id=p_task_id
    and entry.state in('running','paused') and entry.deleted_at is null limit 1;

  return jsonb_build_object(
    'id',task_row.id,'subjectId',task_row.subject_id,'parentId',task_row.parent_id,
    'dayInstanceDate',task_row.day_instance_date,'dayInstanceKind',task_row.day_instance_kind,
    'dayInstanceSourceTaskId',task_row.day_instance_source_task_id,
    'title',task_row.title,'status',task_row.status,'plannedDate',task_row.planned_date,
    'estimateMinutes',task_row.estimate_minutes,'note',task_row.note,
    'projectName',task_row.project_name,'solutionName',task_row.solution_name,
    'sortOrder',task_row.sort_order,
    'completedAt',case when task_row.completed_at is null then null else (extract(epoch from task_row.completed_at)*1000)::bigint end,
    'depth',depth_value,'pathLabel',coalesce(path_value,task_row.title),
    'selectable',not has_children_value,'directMinutes',direct_minutes_value,
    'totalMinutes',total_minutes_value,
    'executionState',case when task_row.status='done' then 'done' when active_state_value is not null then active_state_value when total_minutes_value>0 then 'started' else 'not_started' end,
    'version',task_row.version
  );
end $$;

create or replace function timegenie.direct_cloud_task_list(
  p_workspace_id uuid,p_device_id uuid,p_expected_epoch bigint default null,
  p_subject_id uuid default null,p_include_completed boolean default true,
  p_query text default null
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare data_value jsonb;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  if p_subject_id is not null and not exists(
    select 1 from timegenie.subjects subject where subject.workspace_id=p_workspace_id and subject.id=p_subject_id and subject.deleted_at is null
  ) then raise exception 'NOT_FOUND: 主体不存在'; end if;
  select jsonb_build_object('tasks',coalesce(jsonb_agg(projected order by subject_sort,tree_path),'[]'::jsonb))
  into data_value from (
    with recursive tree as (
      select task.id,subject.sort_order as subject_sort,
        lpad((task.sort_order+1000000000)::text,11,'0')||'|'||task.created_at::text||'|'||task.id::text as tree_path
      from timegenie.tasks task join timegenie.subjects subject on subject.id=task.subject_id
      where task.workspace_id=p_workspace_id and task.deleted_at is null and task.day_instance_date is null
        and (p_subject_id is null or task.subject_id=p_subject_id)
        and (task.parent_id is null or not exists(
          select 1 from timegenie.tasks parent where parent.workspace_id=p_workspace_id and parent.id=task.parent_id and parent.deleted_at is null and parent.day_instance_date is null
        ))
      union all
      select child.id,parent.subject_sort,parent.tree_path||'/'||lpad((child.sort_order+1000000000)::text,11,'0')||'|'||child.created_at::text||'|'||child.id::text
      from timegenie.tasks child join tree parent on child.parent_id=parent.id
      where child.workspace_id=p_workspace_id and child.deleted_at is null and child.day_instance_date is null
    ) select tree.subject_sort,tree.tree_path,timegenie.direct_cloud_task_project(p_workspace_id,tree.id) as projected
    from tree join timegenie.tasks task on task.id=tree.id
    where (p_include_completed or task.status='open')
      and (nullif(btrim(p_query),'') is null or task.title ilike '%'||btrim(p_query)||'%')
  ) rows_value;
  return timegenie.direct_cloud_query_response(p_workspace_id,p_device_id,data_value,array['tasks'],p_expected_epoch);
end $$;

create or replace function timegenie.direct_cloud_completed_tasks(
  p_workspace_id uuid,p_device_id uuid,p_expected_epoch bigint default null,
  p_subject_id uuid default null,p_cursor_completed_at timestamptz default null,
  p_cursor_id uuid default null,p_limit integer default 20
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare data_value jsonb; limit_value integer;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  if p_limit<1 or p_limit>100 then raise exception 'VALIDATION_ERROR: 每页条数必须在 1 到 100 之间'; end if;
  if p_subject_id is not null and not exists(
    select 1 from timegenie.subjects subject where subject.workspace_id=p_workspace_id and subject.id=p_subject_id and subject.deleted_at is null
  ) then raise exception 'NOT_FOUND: 主体不存在'; end if;
  limit_value:=p_limit+1;
  with selected as (
    select task.id,task.completed_at from timegenie.tasks task
    where task.workspace_id=p_workspace_id and task.deleted_at is null and task.day_instance_date is null and task.status='done'
      and (p_subject_id is null or task.subject_id=p_subject_id)
      and (p_cursor_completed_at is null or (task.completed_at,task.id)<(p_cursor_completed_at,p_cursor_id))
    order by task.completed_at desc,task.id desc limit limit_value
  ), projected as (
    select selected.*,timegenie.direct_cloud_task_project(p_workspace_id,selected.id) as item,
      row_number() over(order by selected.completed_at desc,selected.id desc) as row_number
    from selected
  ) select jsonb_build_object(
    'tasks',coalesce(jsonb_agg(item order by completed_at desc,id desc) filter(where row_number<=p_limit),'[]'::jsonb),
    'total',(select count(*) from timegenie.tasks task
      where task.workspace_id=p_workspace_id and task.deleted_at is null
        and task.day_instance_date is null and task.status='done'
        and (p_subject_id is null or task.subject_id=p_subject_id)),
    'hasMore',count(*)>p_limit,
    'nextCursor',case when count(*)>p_limit then (
      select jsonb_build_object('completedAt',completed_at,'id',id) from projected where row_number=p_limit
    ) else null end
  ) into data_value from projected;
  return timegenie.direct_cloud_query_response(p_workspace_id,p_device_id,data_value,array['tasks'],p_expected_epoch);
end $$;

create or replace function timegenie.direct_cloud_task_detail(
  p_workspace_id uuid,p_device_id uuid,p_task_id uuid,p_expected_epoch bigint default null
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare task_value jsonb; data_value jsonb;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  task_value:=timegenie.direct_cloud_task_project(p_workspace_id,p_task_id);
  if task_value is null then raise exception 'NOT_FOUND: 事项不存在'; end if;
  data_value:=jsonb_build_object(
    'task',task_value,
    'children',coalesce((select jsonb_agg(timegenie.direct_cloud_task_project(p_workspace_id,child.id) order by child.sort_order,child.created_at,child.id)
      from timegenie.tasks child where child.workspace_id=p_workspace_id and child.parent_id=p_task_id and child.deleted_at is null),'[]'::jsonb)
  );
  return timegenie.direct_cloud_query_response(p_workspace_id,p_device_id,data_value,array['tasks'],p_expected_epoch);
end $$;

create or replace function timegenie.direct_cloud_today_plan(
  p_workspace_id uuid,p_device_id uuid,p_subject_id uuid,p_work_date date,
  p_expected_epoch bigint default null
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare data_value jsonb;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  if not exists(select 1 from timegenie.subjects subject where subject.workspace_id=p_workspace_id and subject.id=p_subject_id and subject.deleted_at is null) then
    raise exception 'NOT_FOUND: 主体不存在';
  end if;

  with valid_rules as (
    select rule.* from timegenie.task_recurrence_rules rule
    join timegenie.tasks task on task.id=rule.task_id
    where rule.workspace_id=p_workspace_id and task.subject_id=p_subject_id
      and task.deleted_at is null and task.day_instance_date is null
      and rule.effective_start<=p_work_date and (rule.effective_end is null or rule.effective_end>=p_work_date)
  ), matching_rules as (
    select rule.* from valid_rules rule where
      rule.frequency='daily'
      or (rule.frequency='weekdays' and extract(isodow from p_work_date)<6)
      or (rule.frequency='weekly' and (rule.weekdays_mask & (1 << (extract(isodow from p_work_date)::integer-1)))<>0)
  ), member_ids as (
    select task.id from timegenie.tasks task
    left join timegenie.task_day_plans plan on plan.workspace_id=task.workspace_id and plan.task_id=task.id and plan.work_date=p_work_date
    where task.workspace_id=p_workspace_id and task.subject_id=p_subject_id
      and task.deleted_at is null and task.day_instance_date is null
      and (
        plan.state='included'
        or (plan.task_id is null and exists(select 1 from timegenie.task_daily_estimates estimate where estimate.workspace_id=p_workspace_id and estimate.task_id=task.id and estimate.work_date=p_work_date))
        or (coalesce(plan.state,'')<>'excluded' and exists(select 1 from matching_rules rule where rule.task_id=task.id))
      )
  ), context_ids as (
    with recursive ancestors(id,parent_id) as (
      select task.id,task.parent_id from timegenie.tasks task where task.id in(select id from member_ids)
      union
      select parent.id,parent.parent_id from timegenie.tasks parent join ancestors child on child.parent_id=parent.id
      where parent.workspace_id=p_workspace_id and parent.subject_id=p_subject_id and parent.deleted_at is null and parent.day_instance_date is null
    ) select id from ancestors where id not in(select id from member_ids)
  )
  select jsonb_build_object(
    'subjectId',p_subject_id,'workDate',p_work_date,
    'members',coalesce((select jsonb_agg(
      timegenie.direct_cloud_task_project(p_workspace_id,task.id)||jsonb_build_object(
        'todayMember',true,'todayContext',false,
        'todayEstimateMinutes',estimate.estimate_minutes,
        'dailyEstimateVersion',estimate.version,
        'dayPlanState',plan.state,'dayPlanVersion',plan.version,
        'instanceTaskId',plan.instance_task_id
      ) order by task.sort_order,task.created_at,task.id
    ) from member_ids member join timegenie.tasks task on task.id=member.id
      left join timegenie.task_daily_estimates estimate on estimate.workspace_id=p_workspace_id and estimate.task_id=task.id and estimate.work_date=p_work_date
      left join timegenie.task_day_plans plan on plan.workspace_id=p_workspace_id and plan.task_id=task.id and plan.work_date=p_work_date),'[]'::jsonb),
    'contexts',coalesce((select jsonb_agg(
      timegenie.direct_cloud_task_project(p_workspace_id,task.id)||jsonb_build_object('todayMember',false,'todayContext',true)
      order by task.sort_order,task.created_at,task.id
    ) from context_ids context join timegenie.tasks task on task.id=context.id),'[]'::jsonb),
    'plans',coalesce((select jsonb_agg(jsonb_build_object(
      'taskId',plan.task_id,'workDate',plan.work_date,'state',plan.state,'instanceTaskId',plan.instance_task_id,'version',plan.version
    ) order by plan.task_id) from timegenie.task_day_plans plan join timegenie.tasks task on task.id=plan.task_id
      where plan.workspace_id=p_workspace_id and plan.work_date=p_work_date and task.subject_id=p_subject_id and task.deleted_at is null),'[]'::jsonb),
    'estimates',coalesce((select jsonb_agg(jsonb_build_object(
      'taskId',estimate.task_id,'estimateMinutes',estimate.estimate_minutes,'version',estimate.version
    ) order by estimate.task_id) from timegenie.task_daily_estimates estimate join timegenie.tasks task on task.id=estimate.task_id
      where estimate.workspace_id=p_workspace_id and estimate.work_date=p_work_date and task.subject_id=p_subject_id and task.deleted_at is null),'[]'::jsonb),
    'recurrenceRules',coalesce((select jsonb_agg(jsonb_build_object(
      'id',rule.id,'taskId',rule.task_id,'frequency',rule.frequency,'weekdaysMask',rule.weekdays_mask,
      'summary',case rule.frequency when 'daily' then '每天' when 'weekdays' then '工作日'
        else '每周'||array_to_string(array_remove(array[
          case when (rule.weekdays_mask & 1)<>0 then '一' end,case when (rule.weekdays_mask & 2)<>0 then '二' end,
          case when (rule.weekdays_mask & 4)<>0 then '三' end,case when (rule.weekdays_mask & 8)<>0 then '四' end,
          case when (rule.weekdays_mask & 16)<>0 then '五' end,case when (rule.weekdays_mask & 32)<>0 then '六' end,
          case when (rule.weekdays_mask & 64)<>0 then '日' end
        ],null),'、') end,
      'effectiveStart',rule.effective_start,'effectiveEnd',rule.effective_end,'version',rule.version,
      'matchesDate',exists(select 1 from matching_rules matching where matching.id=rule.id)
    ) order by rule.task_id,rule.effective_start,rule.id) from valid_rules rule),'[]'::jsonb),
    'occurrences',coalesce((select jsonb_agg(jsonb_build_object(
      'taskId',occurrence.task_id,'occurrenceDate',occurrence.occurrence_date,'origin',occurrence.origin,
      'status',occurrence.status,
      'completedAt',case when occurrence.completed_at is null then null else (extract(epoch from occurrence.completed_at)*1000)::bigint end,
      'version',occurrence.version
    ) order by occurrence.task_id) from timegenie.task_occurrences occurrence join timegenie.tasks task on task.id=occurrence.task_id
      where occurrence.workspace_id=p_workspace_id and occurrence.occurrence_date=p_work_date and task.subject_id=p_subject_id),'[]'::jsonb),
    'instances',coalesce((select jsonb_agg(timegenie.direct_cloud_task_project(p_workspace_id,instance.id) order by instance.sort_order,instance.created_at,instance.id)
      from timegenie.tasks instance where instance.workspace_id=p_workspace_id and instance.subject_id=p_subject_id
        and instance.day_instance_date=p_work_date and instance.deleted_at is null),'[]'::jsonb)
  ) into data_value;
  return timegenie.direct_cloud_query_response(p_workspace_id,p_device_id,data_value,array['planning','tasks'],p_expected_epoch);
end $$;

create or replace function timegenie.direct_cloud_time_entry_project(
  p_workspace_id uuid,p_entry_id uuid,p_server_time timestamptz default now()
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare entry_row timegenie.time_entries%rowtype; duration_value bigint; allocation_value bigint;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  select entry.* into entry_row from timegenie.time_entries entry
  where entry.workspace_id=p_workspace_id and entry.id=p_entry_id and entry.deleted_at is null;
  if not found then return null; end if;
  select coalesce(sum(case when segment.ended_at is null and entry_row.state='running'
    then greatest(extract(epoch from(p_server_time-segment.started_at))::bigint,0)
    else segment.duration_seconds end),entry_row.duration_seconds) into duration_value
  from timegenie.time_segments segment where segment.workspace_id=p_workspace_id and segment.entry_id=p_entry_id;
  select coalesce(sum(allocation.minutes),0) into allocation_value from timegenie.time_allocations allocation
  where allocation.workspace_id=p_workspace_id and allocation.entry_id=p_entry_id;
  return jsonb_build_object(
    'id',entry_row.id,'workDate',entry_row.work_date,'label',entry_row.label_snapshot,
    'startedAt',(extract(epoch from entry_row.started_at)*1000)::bigint,
    'endedAt',case when entry_row.ended_at is null then null else (extract(epoch from entry_row.ended_at)*1000)::bigint end,
    'durationSeconds',duration_value,'settlementMinutes',case when duration_value<=0 then 0 else (duration_value+59)/60 end,
    'allocatedMinutes',allocation_value,'defaultTaskId',entry_row.default_task_id,'note',entry_row.note,
    'kind',entry_row.kind,'sourceType',entry_row.source_type,'state',entry_row.state,'version',entry_row.version,
    'timerChainId',entry_row.timer_chain_id,'previousEntryId',entry_row.previous_entry_id,
    'splitBoundaryAt',case when entry_row.split_boundary_at is null then null else (extract(epoch from entry_row.split_boundary_at)*1000)::bigint end,
    'segments',coalesce((select jsonb_agg(jsonb_build_object(
      'id',segment.id,'sequenceNo',segment.sequence_no,
      'startedAt',(extract(epoch from segment.started_at)*1000)::bigint,
      'endedAt',case when segment.ended_at is null then null else (extract(epoch from segment.ended_at)*1000)::bigint end,
      'durationSeconds',segment.duration_seconds
    ) order by segment.sequence_no,segment.id) from timegenie.time_segments segment where segment.workspace_id=p_workspace_id and segment.entry_id=p_entry_id),'[]'::jsonb),
    'allocations',coalesce((select jsonb_agg(jsonb_build_object(
      'id',allocation.id,'taskId',allocation.task_id,'taskTitle',task.title,'subjectId',task.subject_id,
      'minutes',allocation.minutes,'note',allocation.note,'version',allocation.version
    ) order by allocation.created_at,allocation.id) from timegenie.time_allocations allocation
      join timegenie.tasks task on task.id=allocation.task_id where allocation.workspace_id=p_workspace_id and allocation.entry_id=p_entry_id),'[]'::jsonb)
  );
end $$;

create or replace function timegenie.direct_cloud_time_state(
  p_workspace_id uuid,p_device_id uuid,p_work_date date,p_expected_epoch bigint default null
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare data_value jsonb; server_time_value timestamptz:=now();
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  select jsonb_build_object(
    'workDate',p_work_date,
    'activeEntry',(select timegenie.direct_cloud_time_entry_project(p_workspace_id,entry.id,server_time_value)
      from timegenie.time_entries entry where entry.workspace_id=p_workspace_id and entry.state in('running','paused') and entry.deleted_at is null limit 1),
    'entries',coalesce((select jsonb_agg(timegenie.direct_cloud_time_entry_project(p_workspace_id,entry.id,server_time_value) order by entry.started_at desc,entry.id desc)
      from timegenie.time_entries entry where entry.workspace_id=p_workspace_id and entry.work_date=p_work_date and entry.deleted_at is null),'[]'::jsonb),
    'unassigned',(select jsonb_build_object(
      'sessionId',session.id,'workDate',session.work_date,'state',session.state,
      'firstStartedAt',(extract(epoch from session.first_started_at)*1000)::bigint,
      'lastEndedAt',case when session.last_ended_at is null then null else (extract(epoch from session.last_ended_at)*1000)::bigint end,
      'currentSegmentStartedAt',case when segment.started_at is null then null else (extract(epoch from segment.started_at)*1000)::bigint end,
      'elapsedSeconds',case when segment.started_at is not null then greatest(extract(epoch from(server_time_value-segment.started_at))::bigint,0) else session.duration_seconds end,
      'requiredMinutes',case when (case when segment.started_at is not null then greatest(extract(epoch from(server_time_value-segment.started_at))::bigint,0) else session.duration_seconds end)<=0 then 0 else ((case when segment.started_at is not null then greatest(extract(epoch from(server_time_value-segment.started_at))::bigint,0) else session.duration_seconds end)+59)/60 end,
      'thresholdSeconds',session.threshold_seconds,
      'mustResolve',(case when segment.started_at is not null then greatest(extract(epoch from(server_time_value-segment.started_at))::bigint,0) else session.duration_seconds end)>session.threshold_seconds,
      'version',session.version
    ) from timegenie.unassigned_sessions session
      left join lateral(select value.started_at from timegenie.unassigned_segments value where value.session_id=session.id and value.ended_at is null order by value.sequence_no desc limit 1) segment on true
      where session.workspace_id=p_workspace_id and session.state in('collecting','awaiting_resolution') order by session.work_date desc limit 1)
  ) into data_value;
  return timegenie.direct_cloud_query_response(p_workspace_id,p_device_id,data_value,array['time','unassigned'],p_expected_epoch);
end $$;

create or replace function timegenie.direct_cloud_time_history(
  p_workspace_id uuid,p_device_id uuid,p_expected_epoch bigint default null,
  p_before_work_date date default null,p_limit_days integer default 7,p_include_breaks boolean default true
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare data_value jsonb; server_time_value timestamptz:=now();
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  if p_limit_days<1 or p_limit_days>31 then raise exception 'VALIDATION_ERROR: 历史日期条数必须在 1 到 31 之间'; end if;
  with dates as (
    select work_date from (
      select entry.work_date from timegenie.time_entries entry where entry.workspace_id=p_workspace_id and entry.deleted_at is null and (p_include_breaks or entry.kind<>'break')
      union select adjustment.work_date from timegenie.actual_time_adjustments adjustment where adjustment.workspace_id=p_workspace_id and adjustment.deleted_at is null
    ) value where p_before_work_date is null or work_date<p_before_work_date order by work_date desc limit p_limit_days+1
  ), numbered as (select work_date,row_number() over(order by work_date desc) as row_number from dates), groups as (
    select day.work_date,day.row_number,
      coalesce((select jsonb_agg(timegenie.direct_cloud_time_entry_project(p_workspace_id,entry.id,server_time_value) order by entry.started_at desc,entry.id desc)
        from timegenie.time_entries entry where entry.workspace_id=p_workspace_id and entry.work_date=day.work_date and entry.deleted_at is null and (p_include_breaks or entry.kind<>'break')),'[]'::jsonb) as entries,
      coalesce((select jsonb_agg(jsonb_build_object(
        'id',adjustment.id,'subjectId',adjustment.subject_id,'taskId',adjustment.task_id,'taskTitle',task.title,
        'workDate',adjustment.work_date,'deltaMinutes',adjustment.delta_minutes,'baselineMinutes',adjustment.baseline_minutes,
        'targetMinutes',adjustment.target_minutes,'note',adjustment.note,'batchId',adjustment.batch_id,
        'createdAt',(extract(epoch from adjustment.created_at)*1000)::bigint,'version',adjustment.version
      ) order by adjustment.created_at desc,adjustment.id desc) from timegenie.actual_time_adjustments adjustment
        join timegenie.tasks task on task.id=adjustment.task_id where adjustment.workspace_id=p_workspace_id and adjustment.work_date=day.work_date and adjustment.deleted_at is null),'[]'::jsonb) as adjustments
    from numbered day where day.row_number<=p_limit_days
  ) select jsonb_build_object(
    'groups',coalesce(jsonb_agg(jsonb_build_object(
      'workDate',work_date,'entries',entries,'adjustments',adjustments
    ) order by work_date desc),'[]'::jsonb),
    'hasMore',(select count(*)>p_limit_days from numbered),
    'nextCursor',case when (select count(*)>p_limit_days from numbered) then (select work_date from numbered where row_number=p_limit_days) else null end
  ) into data_value from groups;
  return timegenie.direct_cloud_query_response(p_workspace_id,p_device_id,data_value,array['time','unassigned'],p_expected_epoch);
end $$;

create or replace function timegenie.direct_cloud_report_project(
  p_workspace_id uuid,p_report_id uuid
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare report_row timegenie.reports%rowtype; subject_name_value text; template_name_value text; template_archived_value boolean;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  select report.* into report_row from timegenie.reports report
  where report.workspace_id=p_workspace_id and report.id=p_report_id and report.deleted_at is null;
  if not found then return null; end if;
  select subject.name into subject_name_value from timegenie.subjects subject where subject.id=report_row.subject_id;
  select template.name,template.deleted_at is not null into template_name_value,template_archived_value
  from timegenie.report_templates template where template.id=report_row.template_id;
  return jsonb_build_object(
    'id',report_row.id,'reportType',report_row.report_type,'subjectId',report_row.subject_id,
    'subjectName',subject_name_value,'periodStart',report_row.period_start,'periodEnd',report_row.period_end,
    'referenceDate',report_row.reference_date,
    'taskIds',coalesce((select jsonb_agg(item.task_id order by item.sort_order,item.task_id) from timegenie.report_tasks item where item.workspace_id=p_workspace_id and item.report_id=p_report_id),'[]'::jsonb),
    'templateId',report_row.template_id,'templateName',template_name_value,
    'templateArchived',coalesce(template_archived_value,false),
    'markdown',report_row.markdown_content,'contentSource',report_row.content_source,
    'generatedCount',report_row.generation_count,
    'updatedAt',(extract(epoch from report_row.updated_at)*1000)::bigint,'version',report_row.version
  );
end $$;

create or replace function timegenie.direct_cloud_report_list(
  p_workspace_id uuid,p_device_id uuid,p_expected_epoch bigint default null,
  p_report_type text default null,p_subject_id uuid default null,p_query text default null,
  p_cursor_updated_at timestamptz default null,p_cursor_id uuid default null,p_limit integer default 20
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare data_value jsonb;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  if p_limit<1 or p_limit>100 then raise exception 'VALIDATION_ERROR: 每页条数必须在 1 到 100 之间'; end if;
  if p_report_type is not null and p_report_type not in('daily','weekly','monthly') then raise exception 'VALIDATION_ERROR: 非法报告类型'; end if;
  with selected as (
    select report.id,report.updated_at from timegenie.reports report join timegenie.subjects subject on subject.id=report.subject_id
    where report.workspace_id=p_workspace_id and report.deleted_at is null
      and (p_report_type is null or report.report_type=p_report_type)
      and (p_subject_id is null or report.subject_id=p_subject_id)
      and (nullif(btrim(p_query),'') is null or report.period_start::text ilike '%'||btrim(p_query)||'%' or report.period_end::text ilike '%'||btrim(p_query)||'%' or subject.name ilike '%'||btrim(p_query)||'%')
      and (p_cursor_updated_at is null or (report.updated_at,report.id)<(p_cursor_updated_at,p_cursor_id))
    order by report.updated_at desc,report.id desc limit p_limit+1
  ), numbered as(select *,row_number() over(order by updated_at desc,id desc) as row_number from selected)
  select jsonb_build_object(
    'reports',coalesce(jsonb_agg(timegenie.direct_cloud_report_project(p_workspace_id,id) order by updated_at desc,id desc) filter(where row_number<=p_limit),'[]'::jsonb),
    'hasMore',count(*)>p_limit,
    'nextCursor',case when count(*)>p_limit then (select jsonb_build_object('updatedAt',updated_at,'id',id) from numbered where row_number=p_limit) else null end,
    'typeCounts',jsonb_build_object(
      'all',(select count(*) from timegenie.reports report where report.workspace_id=p_workspace_id and report.deleted_at is null and (p_subject_id is null or report.subject_id=p_subject_id)),
      'daily',(select count(*) from timegenie.reports report where report.workspace_id=p_workspace_id and report.deleted_at is null and report.report_type='daily' and (p_subject_id is null or report.subject_id=p_subject_id)),
      'weekly',(select count(*) from timegenie.reports report where report.workspace_id=p_workspace_id and report.deleted_at is null and report.report_type='weekly' and (p_subject_id is null or report.subject_id=p_subject_id)),
      'monthly',(select count(*) from timegenie.reports report where report.workspace_id=p_workspace_id and report.deleted_at is null and report.report_type='monthly' and (p_subject_id is null or report.subject_id=p_subject_id))
    )
  ) into data_value from numbered;
  return timegenie.direct_cloud_query_response(p_workspace_id,p_device_id,data_value,array['reports'],p_expected_epoch);
end $$;

create or replace function timegenie.direct_cloud_report_detail(
  p_workspace_id uuid,p_device_id uuid,p_report_id uuid,p_expected_epoch bigint default null
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare report_value jsonb; data_value jsonb;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  report_value:=timegenie.direct_cloud_report_project(p_workspace_id,p_report_id);
  if report_value is null then raise exception 'NOT_FOUND: 报告不存在'; end if;
  data_value:=jsonb_build_object(
    'report',report_value,
    'tasks',coalesce((select jsonb_agg(jsonb_build_object(
      'taskId',item.task_id,'title',item.title_snapshot,'pathLabel',item.path_snapshot,'sortOrder',item.sort_order,
      'archived',task.deleted_at is not null
    ) order by item.sort_order,item.task_id) from timegenie.report_tasks item left join timegenie.tasks task on task.id=item.task_id
      where item.workspace_id=p_workspace_id and item.report_id=p_report_id),'[]'::jsonb)
  );
  return timegenie.direct_cloud_query_response(p_workspace_id,p_device_id,data_value,array['reports'],p_expected_epoch);
end $$;

create or replace function timegenie.direct_cloud_report_templates(
  p_workspace_id uuid,p_device_id uuid,p_report_type text default null,p_expected_epoch bigint default null
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare data_value jsonb;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  if p_report_type is not null and p_report_type not in('daily','weekly','monthly') then raise exception 'VALIDATION_ERROR: 非法报告类型'; end if;
  select jsonb_build_object('templates',coalesce(jsonb_agg(jsonb_build_object(
    'id',template.id,'reportType',template.report_type,'name',template.name,'description',template.description,
    'content',template.content,'isDefault',template.is_default,'isBuiltin',template.is_builtin,
    'sortOrder',template.sort_order,'version',template.version
  ) order by template.report_type,template.is_default desc,template.sort_order,template.name,template.id),'[]'::jsonb)) into data_value
  from timegenie.report_templates template where template.workspace_id=p_workspace_id and template.deleted_at is null
    and (p_report_type is null or template.report_type=p_report_type);
  return timegenie.direct_cloud_query_response(p_workspace_id,p_device_id,data_value,array['reports'],p_expected_epoch);
end $$;

create or replace function timegenie.direct_cloud_report_preview(
  p_workspace_id uuid,p_device_id uuid,p_report_type text,p_reference_date date,p_subject_id uuid,
  p_template_id uuid default null,p_expected_epoch bigint default null
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare period_start_value date; period_end_value date; template_row timegenie.report_templates%rowtype;
  subject_name_value text; task_lines text; total_minutes_value bigint; daily_collection text; markdown_value text; data_value jsonb;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  if p_report_type not in('daily','weekly','monthly') then raise exception 'VALIDATION_ERROR: 非法报告类型'; end if;
  select subject.name into subject_name_value from timegenie.subjects subject
    where subject.workspace_id=p_workspace_id and subject.id=p_subject_id and subject.deleted_at is null;
  if not found then raise exception 'NOT_FOUND: 主体不存在'; end if;
  if p_report_type='daily' then period_start_value:=p_reference_date; period_end_value:=p_reference_date;
  elsif p_report_type='weekly' then period_start_value:=p_reference_date-(extract(isodow from p_reference_date)::integer-1); period_end_value:=period_start_value+6;
  else period_start_value:=date_trunc('month',p_reference_date)::date; period_end_value:=(date_trunc('month',p_reference_date)+interval '1 month-1 day')::date; end if;
  if p_template_id is not null then
    select template.* into template_row from timegenie.report_templates template where template.workspace_id=p_workspace_id and template.id=p_template_id and template.report_type=p_report_type and template.deleted_at is null;
  else
    select template.* into template_row from timegenie.report_templates template where template.workspace_id=p_workspace_id and template.report_type=p_report_type and template.deleted_at is null order by template.is_default desc,template.sort_order,template.id limit 1;
  end if;
  if not found then raise exception 'NOT_FOUND: 报告模板不存在'; end if;
  with candidate as (
    select task.id,task.title,task.sort_order,task.created_at from timegenie.tasks task
    where task.workspace_id=p_workspace_id and task.subject_id=p_subject_id
      and (task.deleted_at is null or exists(select 1 from timegenie.actual_time_adjustments adjustment where adjustment.task_id=task.id and adjustment.work_date between period_start_value and period_end_value and adjustment.deleted_at is null))
      and (task.status='open' or task.completed_at::date between period_start_value and period_end_value
        or exists(select 1 from timegenie.time_allocations allocation join timegenie.time_entries entry on entry.id=allocation.entry_id where allocation.task_id=task.id and entry.work_date between period_start_value and period_end_value and entry.deleted_at is null)
        or exists(select 1 from timegenie.actual_time_adjustments adjustment where adjustment.task_id=task.id and adjustment.work_date between period_start_value and period_end_value and adjustment.deleted_at is null))
  ) select string_agg(row_number_value||'. '||title,E'\n' order by sort_order,created_at,id) into task_lines
    from(select candidate.*,row_number() over(order by sort_order,created_at,id) as row_number_value from candidate) value;
  select coalesce(sum(allocation.minutes),0)+coalesce((select sum(adjustment.delta_minutes) from timegenie.actual_time_adjustments adjustment where adjustment.workspace_id=p_workspace_id and adjustment.subject_id=p_subject_id and adjustment.work_date between period_start_value and period_end_value and adjustment.deleted_at is null),0) into total_minutes_value
  from timegenie.time_allocations allocation join timegenie.time_entries entry on entry.id=allocation.entry_id join timegenie.tasks task on task.id=allocation.task_id
  where allocation.workspace_id=p_workspace_id and task.subject_id=p_subject_id and entry.work_date between period_start_value and period_end_value and entry.deleted_at is null;
  select string_agg(report.markdown_content,E'\n\n' order by report.reference_date) into daily_collection from timegenie.reports report
    where report.workspace_id=p_workspace_id and report.subject_id=p_subject_id and report.report_type='daily' and report.reference_date between period_start_value and period_end_value and report.deleted_at is null;
  markdown_value:=template_row.content;
  markdown_value:=replace(markdown_value,'{{姓名}}',subject_name_value);
  markdown_value:=replace(markdown_value,'{{日期}}',period_start_value::text);
  markdown_value:=replace(markdown_value,'{{星期}}',to_char(period_start_value,'ID'));
  markdown_value:=replace(markdown_value,'{{日期范围}}',period_start_value::text||' 至 '||period_end_value::text);
  markdown_value:=replace(markdown_value,'{{总工时}}',greatest(total_minutes_value,0)::text||'min');
  markdown_value:=replace(markdown_value,'{{今日事项}}',coalesce(task_lines,'暂无事项'));
  markdown_value:=replace(markdown_value,'{{明日计划}}','暂无计划');
  markdown_value:=replace(markdown_value,'{{工作时段}}','');
  markdown_value:=replace(markdown_value,'{{每日情况}}',coalesce(daily_collection,'暂无已保存日报'));
  markdown_value:=replace(markdown_value,'{{日报合集}}',coalesce(daily_collection,'暂无已保存日报'));
  markdown_value:=replace(markdown_value,'{{统计信息}}','- 累计用时：'||greatest(total_minutes_value,0)::text||'min');
  markdown_value:=replace(markdown_value,'{{总结}}','- 本期总结');
  data_value:=jsonb_build_object(
    'reportType',p_report_type,'referenceDate',p_reference_date,'periodStart',period_start_value,'periodEnd',period_end_value,
    'subjectId',p_subject_id,'subjectName',subject_name_value,
    'template',jsonb_build_object('id',template_row.id,'name',template_row.name,'version',template_row.version),
    'context',jsonb_build_object('totalMinutes',greatest(total_minutes_value,0),'taskLines',coalesce(task_lines,''),'dailyCollection',coalesce(daily_collection,'')),
    'markdown',markdown_value
  );
  return timegenie.direct_cloud_query_response(p_workspace_id,p_device_id,data_value,array['reports','tasks','time'],p_expected_epoch);
end $$;

create or replace function timegenie.direct_cloud_shared_configuration(
  p_workspace_id uuid,p_device_id uuid,p_expected_epoch bigint default null
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare workspace_row timegenie.workspaces%rowtype; data_value jsonb;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  select workspace.* into workspace_row from timegenie.workspaces workspace
  where workspace.id=p_workspace_id and workspace.deleted_at is null;
  data_value:=jsonb_build_object(
    'workspace',jsonb_build_object(
      'id',workspace_row.id,'name',workspace_row.name,'timezone',workspace_row.timezone,
      'version',workspace_row.version,'updatedAt',workspace_row.updated_at
    ),
    'settings',coalesce((select jsonb_object_agg(
      setting.key,timegenie.direct_cloud_sanitize_shared_json(setting.value_json) order by setting.key
    ) from timegenie.app_settings setting where setting.workspace_id=p_workspace_id),'{}'::jsonb),
    'settingVersions',coalesce((select jsonb_object_agg(setting.key,setting.version order by setting.key)
      from timegenie.app_settings setting where setting.workspace_id=p_workspace_id),'{}'::jsonb),
    'workDays',coalesce((select jsonb_agg(jsonb_strip_nulls(jsonb_build_object(
      'workDate',day.work_date,'timezone',day.timezone,'workPeriodText',day.work_period_text,
      'note',day.note,'settledAt',day.settled_at,'version',day.version,'updatedAt',day.updated_at
    )) order by day.work_date) from timegenie.work_days day where day.workspace_id=p_workspace_id),'[]'::jsonb),
    'subjectWorkSchedules',coalesce((select jsonb_agg(jsonb_build_object(
      'subjectId',subject.id,'subjectName',subject.name,
      'days',coalesce((select jsonb_agg(jsonb_build_object(
        'id',schedule.id,'weekday',schedule.weekday,'enabled',schedule.is_workday,
        'segments',coalesce((select jsonb_agg(jsonb_build_object(
          'start',lpad((segment.start_minute/60)::text,2,'0')||':'||lpad((segment.start_minute%60)::text,2,'0'),
          'end',lpad((segment.end_minute/60)::text,2,'0')||':'||lpad((segment.end_minute%60)::text,2,'0'),
          'startMinute',segment.start_minute,'endMinute',segment.end_minute
        ) order by segment.sequence_no) from timegenie.subject_work_schedule_segments segment
          where segment.workspace_id=p_workspace_id and segment.schedule_id=schedule.id),'[]'::jsonb),
        'version',schedule.version,'updatedAt',schedule.updated_at
      ) order by schedule.weekday) from timegenie.subject_work_schedules schedule
        where schedule.workspace_id=p_workspace_id and schedule.subject_id=subject.id),'[]'::jsonb)
    ) order by subject.sort_order,subject.created_at,subject.id)
      from timegenie.subjects subject where subject.workspace_id=p_workspace_id and subject.deleted_at is null),'[]'::jsonb),
    'integrations',coalesce((select jsonb_agg(jsonb_build_object(
      'provider',config.provider,'enabled',config.enabled,
      'config',timegenie.direct_cloud_sanitize_shared_json(config.config_json),
      'version',config.version,'updatedAt',config.updated_at,
      'requiresDeviceCredential',(config.provider in ('seatable'))
    ) order by config.provider) from timegenie.integration_configs config
      where config.workspace_id=p_workspace_id),'[]'::jsonb),
    'bindings',coalesce((select jsonb_agg(jsonb_strip_nulls(jsonb_build_object(
      'id',binding.id,'provider',binding.provider,'entityType',binding.entity_type,
      'entityId',binding.entity_id,'externalId',binding.external_id,
      'externalRevision',binding.external_revision,'lastSyncedAt',binding.last_synced_at
    )) order by binding.provider,binding.entity_type,binding.entity_id,binding.id)
      from timegenie.external_bindings binding where binding.workspace_id=p_workspace_id),'[]'::jsonb)
  );
  return timegenie.direct_cloud_query_response(
    p_workspace_id,p_device_id,data_value,array['settings','calendar','integrations'],p_expected_epoch
  );
end $$;

create or replace function timegenie.direct_cloud_operation_result(
  p_workspace_id uuid,
  p_operation_id uuid,
  p_operation_type text,
  p_request jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare processed_row timegenie.processed_operations; request_value text;
begin
  select * into processed_row from timegenie.processed_operations
  where workspace_id=p_workspace_id and operation_id=p_operation_id;
  if not found then return null; end if;
  request_value:=timegenie.operation_request_hash(p_operation_type,coalesce(p_request,'{}'::jsonb));
  if processed_row.operation_type is distinct from p_operation_type
     or processed_row.request_hash is distinct from request_value then
    raise exception 'IDEMPOTENCY_CONFLICT: 操作标识已被用于不同请求';
  end if;
  return processed_row.result_json;
end $$;

create or replace function timegenie.record_direct_cloud_operation(
  p_workspace_id uuid,
  p_operation_id uuid,
  p_device_id uuid,
  p_operation_type text,
  p_request jsonb,
  p_result jsonb
) returns void language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
begin
  perform timegenie.record_processed_operation(
    p_workspace_id,p_operation_id,p_device_id,p_operation_type,
    coalesce(p_request,'{}'::jsonb),coalesce(p_result,'{}'::jsonb)
  );
end $$;

create or replace function timegenie.direct_cloud_operation_get(
  p_workspace_id uuid,
  p_device_id uuid,
  p_operation_id uuid,
  p_operation_type text,
  p_request jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare cached jsonb;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,null);
  cached:=timegenie.direct_cloud_operation_result(
    p_workspace_id,p_operation_id,p_operation_type,coalesce(p_request,'{}'::jsonb)
  );
  if cached is null then
    return jsonb_build_object('found',false,'operationId',p_operation_id);
  end if;
  return jsonb_build_object(
    'found',true,'operationId',p_operation_id,'operationType',p_operation_type,'result',cached
  );
end $$;

create or replace function timegenie.storage_protocol_upgrade_preflight(
  p_workspace_id uuid,
  p_device_id uuid
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare workspace_row timegenie.workspaces;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  perform timegenie.client_schema_generation_assert(p_workspace_id);
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then
    raise exception 'DEVICE_REVOKED';
  end if;
  select workspace.* into workspace_row
  from timegenie.workspaces workspace
  where workspace.id=p_workspace_id and workspace.deleted_at is null;
  if not found then raise exception 'WORKSPACE_NOT_FOUND'; end if;
  return jsonb_build_object(
    'workspaceId',workspace_row.id,
    'deviceId',p_device_id,
    'storageProtocol',workspace_row.storage_protocol,
    'workspaceEpoch',workspace_row.workspace_epoch,
    'schemaGeneration',workspace_row.minimum_client_schema_generation,
    'serverUpgradeAllowed',workspace_row.storage_protocol='sync_v1',
    'alreadyDirect',workspace_row.storage_protocol='direct_v2',
    'requiresLocalQueueDisposition',true,
    'allowedQueueDispositions',jsonb_build_array('queue_drained','abandoned_with_rescue'),
    'serverTime',now()
  );
end $$;

create or replace function timegenie.upgrade_storage_protocol(
  p_workspace_id uuid,
  p_device_id uuid,
  p_operation_id uuid,
  p_expected_epoch bigint,
  p_old_sync_disposition text,
  p_create_safety_snapshot boolean default true
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare workspace_row timegenie.workspaces; cached jsonb; request_value jsonb;
declare result_value jsonb; reset_seq bigint; safety_snapshot jsonb;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_REVOKED'; end if;
  if p_old_sync_disposition not in ('queue_drained','abandoned_with_rescue') then
    raise exception 'VALIDATION_ERROR: 必须明确旧同步队列已清空或已导出救援备份后放弃';
  end if;
  request_value:=jsonb_build_object(
    'deviceId',p_device_id,'expectedEpoch',p_expected_epoch,
    'oldSyncDisposition',p_old_sync_disposition,
    'createSafetySnapshot',p_create_safety_snapshot
  );
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':storage_protocol_upgrade',0));
  cached:=timegenie.direct_cloud_operation_result(
    p_workspace_id,p_operation_id,'storage_protocol_upgrade',request_value
  );
  if cached is not null then return cached; end if;
  select workspace.* into workspace_row from timegenie.workspaces workspace
  where workspace.id=p_workspace_id and workspace.deleted_at is null for update;
  if not found then raise exception 'WORKSPACE_NOT_FOUND'; end if;
  if workspace_row.storage_protocol='direct_v2' then
    return jsonb_build_object(
      'upgraded',false,'alreadyDirect',true,'workspaceEpoch',workspace_row.workspace_epoch
    );
  end if;
  if workspace_row.workspace_epoch<>p_expected_epoch then
    raise exception 'WORKSPACE_EPOCH_STALE: expected %, current %',
      p_expected_epoch,workspace_row.workspace_epoch;
  end if;
  if p_create_safety_snapshot then
    safety_snapshot:=timegenie.workspace_snapshot_create(
      p_workspace_id,p_device_id,gen_random_uuid(),'pre_restore',
      '直连协议升级安全快照','切换 direct_v2 前创建',false
    );
  end if;
  update timegenie.workspaces
  set storage_protocol='direct_v2',updated_at=now(),version=version+1
  where id=p_workspace_id;
  insert into timegenie.workspace_changes(
    workspace_id,entity_type,entity_id,operation,entity_version,changed_by_device_id
  ) values(
    p_workspace_id,'workspace_reset',p_workspace_id::text,'update',workspace_row.version+1,p_device_id
  ) returning change_seq into reset_seq;
  result_value:=jsonb_build_object(
    'upgraded',true,'alreadyDirect',false,
    'workspaceEpoch',workspace_row.workspace_epoch,
    'storageProtocol','direct_v2',
    'changeSeq',reset_seq,
    'safetySnapshotId',coalesce(safety_snapshot->>'id',null),
    'serverTime',now()
  );
  perform timegenie.record_direct_cloud_operation(
    p_workspace_id,p_operation_id,p_device_id,'storage_protocol_upgrade',request_value,result_value
  );
  return result_value;
end $$;

create or replace function timegenie.provision_direct_v2_workspace(
  p_workspace_id uuid,
  p_device_id uuid,
  p_operation_id uuid
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare workspace_row timegenie.workspaces; cached jsonb; request_value jsonb;
declare result_value jsonb; reset_seq bigint;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_REVOKED'; end if;
  request_value:=jsonb_build_object('deviceId',p_device_id);
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':storage_protocol_upgrade',0));
  cached:=timegenie.direct_cloud_operation_result(
    p_workspace_id,p_operation_id,'storage_protocol_provision_direct',request_value
  );
  if cached is not null then return cached; end if;
  select workspace.* into workspace_row from timegenie.workspaces workspace
  where workspace.id=p_workspace_id and workspace.deleted_at is null for update;
  if not found then raise exception 'WORKSPACE_NOT_FOUND'; end if;
  if workspace_row.storage_protocol='direct_v2' then
    return jsonb_build_object('provisioned',false,'alreadyDirect',true,'workspaceEpoch',workspace_row.workspace_epoch);
  end if;
  if exists(select 1 from timegenie.tasks where workspace_id=p_workspace_id)
     or exists(select 1 from timegenie.time_entries where workspace_id=p_workspace_id)
     or exists(select 1 from timegenie.reports where workspace_id=p_workspace_id) then
    raise exception 'DIRECT_PROVISION_REQUIRES_EMPTY_WORKSPACE';
  end if;
  update timegenie.workspaces
  set storage_protocol='direct_v2',updated_at=now(),version=version+1
  where id=p_workspace_id;
  insert into timegenie.workspace_changes(
    workspace_id,entity_type,entity_id,operation,entity_version,changed_by_device_id
  ) values(
    p_workspace_id,'workspace_reset',p_workspace_id::text,'update',workspace_row.version+1,p_device_id
  ) returning change_seq into reset_seq;
  result_value:=jsonb_build_object(
    'provisioned',true,'alreadyDirect',false,
    'workspaceEpoch',workspace_row.workspace_epoch,
    'storageProtocol','direct_v2','changeSeq',reset_seq,'serverTime',now()
  );
  perform timegenie.record_direct_cloud_operation(
    p_workspace_id,p_operation_id,p_device_id,'storage_protocol_provision_direct',request_value,result_value
  );
  return result_value;
end $$;

-- Rename the existing implementations once, then install guarded wrappers with
-- the original public signatures.
do $$ begin
  if to_regprocedure('timegenie.tracking_lease_acquire_pre_direct_v2(uuid,uuid,uuid)') is null then
    alter function timegenie.tracking_lease_acquire(uuid,uuid,uuid) rename to tracking_lease_acquire_pre_direct_v2;
  end if;
  if to_regprocedure('timegenie.tracking_lease_renew_pre_direct_v2(uuid,uuid,uuid)') is null then
    alter function timegenie.tracking_lease_renew(uuid,uuid,uuid) rename to tracking_lease_renew_pre_direct_v2;
  end if;
  if to_regprocedure('timegenie.tracking_lease_release_pre_direct_v2(uuid,uuid,uuid)') is null then
    alter function timegenie.tracking_lease_release(uuid,uuid,uuid) rename to tracking_lease_release_pre_direct_v2;
  end if;
  if to_regprocedure('timegenie.cloud_apply_patch_pre_direct_v2(uuid,uuid,uuid,text,text,text,bigint,jsonb)') is null then
    alter function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,jsonb) rename to cloud_apply_patch_pre_direct_v2;
  end if;
  if to_regprocedure('timegenie.cloud_apply_patch_pre_direct_v2(uuid,uuid,uuid,text,text,text,bigint,bigint,bigint,jsonb)') is null then
    alter function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,bigint,bigint,jsonb) rename to cloud_apply_patch_pre_direct_v2;
  end if;
  if to_regprocedure('timegenie.cloud_incremental_pull_pre_direct_v2(uuid,uuid,bigint,integer)') is null then
    alter function timegenie.cloud_incremental_pull(uuid,uuid,bigint,integer) rename to cloud_incremental_pull_pre_direct_v2;
  end if;
  if to_regprocedure('timegenie.tracking_lease_takeover_for_timer_write_pre_direct_v2(uuid,uuid,uuid)') is null then
    alter function timegenie.tracking_lease_takeover_for_timer_write(uuid,uuid,uuid)
      rename to tracking_lease_takeover_for_timer_write_pre_direct_v2;
  end if;
  if to_regprocedure('timegenie.unassigned_get_or_create_shared_pre_direct_v2(uuid,uuid,jsonb,uuid,timestamptz)') is null then
    alter function timegenie.unassigned_get_or_create_shared(uuid,uuid,jsonb,uuid,timestamptz)
      rename to unassigned_get_or_create_shared_pre_direct_v2;
  end if;
  if to_regprocedure('timegenie.unassigned_resolve_shared_pre_direct_v2(uuid,uuid,uuid,uuid,bigint,text,jsonb)') is null then
    alter function timegenie.unassigned_resolve_shared(uuid,uuid,uuid,uuid,bigint,text,jsonb)
      rename to unassigned_resolve_shared_pre_direct_v2;
  end if;
  if to_regprocedure('timegenie.unassigned_anchor_reset_pre_direct_v2(uuid,uuid,uuid,uuid,bigint,bigint)') is null then
    alter function timegenie.unassigned_anchor_reset(uuid,uuid,uuid,uuid,bigint,bigint)
      rename to unassigned_anchor_reset_pre_direct_v2;
  end if;
  if to_regprocedure('timegenie.actual_adjustment_batch_apply_pre_direct_v2(uuid,uuid,uuid,jsonb)') is null then
    alter function timegenie.actual_adjustment_batch_apply(uuid,uuid,uuid,jsonb)
      rename to actual_adjustment_batch_apply_pre_direct_v2;
  end if;
  if to_regprocedure('timegenie.timer_transition_apply_pre_direct_v2(uuid,uuid,uuid,jsonb)') is null then
    alter function timegenie.timer_transition_apply(uuid,uuid,uuid,jsonb)
      rename to timer_transition_apply_pre_direct_v2;
  end if;
  if to_regprocedure('timegenie.migration_import_snapshot_pre_direct_v2(uuid,uuid,uuid,jsonb)') is null then
    alter function timegenie.migration_import_snapshot(uuid,uuid,uuid,jsonb)
      rename to migration_import_snapshot_pre_direct_v2;
  end if;
  if to_regprocedure('timegenie.workspace_restore_prepare_pre_direct_v2(uuid,uuid,uuid,bigint)') is null then
    alter function timegenie.workspace_restore_prepare(uuid,uuid,uuid,bigint)
      rename to workspace_restore_prepare_pre_direct_v2;
  end if;
  if to_regprocedure('timegenie.workspace_restore_prepare_pre_direct_v2(uuid,uuid,uuid,bigint,boolean)') is null then
    alter function timegenie.workspace_restore_prepare(uuid,uuid,uuid,bigint,boolean)
      rename to workspace_restore_prepare_pre_direct_v2;
  end if;
  if to_regprocedure('timegenie.workspace_restore_commit_pre_direct_v2(uuid,uuid,uuid,uuid,text,bigint,uuid,boolean)') is null then
    alter function timegenie.workspace_restore_commit(uuid,uuid,uuid,uuid,text,bigint,uuid,boolean)
      rename to workspace_restore_commit_pre_direct_v2;
  end if;
  if to_regprocedure('timegenie.timer_start_pre_direct_v2(uuid,uuid,uuid,uuid,text)') is null then
    alter function timegenie.timer_start(uuid,uuid,uuid,uuid,text) rename to timer_start_pre_direct_v2;
  end if;
  if to_regprocedure('timegenie.timer_pause_pre_direct_v2(uuid,uuid,uuid,bigint,uuid)') is null then
    alter function timegenie.timer_pause(uuid,uuid,uuid,bigint,uuid) rename to timer_pause_pre_direct_v2;
  end if;
  if to_regprocedure('timegenie.timer_resume_pre_direct_v2(uuid,uuid,uuid,bigint,uuid)') is null then
    alter function timegenie.timer_resume(uuid,uuid,uuid,bigint,uuid) rename to timer_resume_pre_direct_v2;
  end if;
  if to_regprocedure('timegenie.timer_stop_pre_direct_v2(uuid,uuid,uuid,bigint,uuid)') is null then
    alter function timegenie.timer_stop(uuid,uuid,uuid,bigint,uuid) rename to timer_stop_pre_direct_v2;
  end if;
  if to_regprocedure('timegenie.unassigned_resolve_work_pre_direct_v2(uuid,uuid,uuid,bigint,jsonb,uuid)') is null then
    alter function timegenie.unassigned_resolve_work(uuid,uuid,uuid,bigint,jsonb,uuid)
      rename to unassigned_resolve_work_pre_direct_v2;
  end if;
  if to_regprocedure('timegenie.unassigned_resolve_break_pre_direct_v2(uuid,uuid,uuid,bigint,jsonb,uuid)') is null then
    alter function timegenie.unassigned_resolve_break(uuid,uuid,uuid,bigint,jsonb,uuid)
      rename to unassigned_resolve_break_pre_direct_v2;
  end if;
  if to_regprocedure('timegenie.unassigned_discard_pre_direct_v2(uuid,uuid,uuid,bigint,jsonb,uuid)') is null then
    alter function timegenie.unassigned_discard(uuid,uuid,uuid,bigint,jsonb,uuid)
      rename to unassigned_discard_pre_direct_v2;
  end if;
  if to_regprocedure('timegenie.coordinate_timer_calendar_day_pre_direct_v2(uuid,uuid)') is null then
    alter function timegenie.coordinate_timer_calendar_day(uuid,uuid)
      rename to coordinate_timer_calendar_day_pre_direct_v2;
  end if;
  if to_regprocedure('timegenie.coordinate_unassigned_calendar_day_pre_direct_v2(uuid,uuid,uuid)') is null then
    alter function timegenie.coordinate_unassigned_calendar_day(uuid,uuid,uuid)
      rename to coordinate_unassigned_calendar_day_pre_direct_v2;
  end if;
  if to_regprocedure('timegenie.set_report_template_default_pre_direct_v2(uuid,uuid,bigint)') is null then
    alter function timegenie.set_report_template_default(uuid,uuid,bigint)
      rename to set_report_template_default_pre_direct_v2;
  end if;
end $$;

-- Legacy timer lease and sync entry points must fail before changing any
-- protocol-owned state once a workspace has moved to direct_v2.
create or replace function timegenie.tracking_lease_acquire(p_workspace_id uuid,p_device_id uuid,p_lease_token uuid)
returns jsonb language sql security definer set search_path=timegenie,extensions,pg_catalog as $$
  with checked as (select timegenie.assert_sync_v1_workspace(p_workspace_id))
  select timegenie.tracking_lease_acquire_pre_direct_v2(p_workspace_id,p_device_id,p_lease_token)
  from checked;
$$;

create or replace function timegenie.tracking_lease_renew(p_workspace_id uuid,p_device_id uuid,p_lease_token uuid)
returns jsonb language sql security definer set search_path=timegenie,extensions,pg_catalog as $$
  with checked as (select timegenie.assert_sync_v1_workspace(p_workspace_id))
  select timegenie.tracking_lease_renew_pre_direct_v2(p_workspace_id,p_device_id,p_lease_token)
  from checked;
$$;

create or replace function timegenie.tracking_lease_release(p_workspace_id uuid,p_device_id uuid,p_lease_token uuid)
returns boolean language sql security definer set search_path=timegenie,extensions,pg_catalog as $$
  with checked as (select timegenie.assert_sync_v1_workspace(p_workspace_id))
  select timegenie.tracking_lease_release_pre_direct_v2(p_workspace_id,p_device_id,p_lease_token)
  from checked;
$$;

create or replace function timegenie.cloud_apply_patch(p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_operation_type text,p_entity_type text,p_entity_id text,p_base_version bigint,p_payload jsonb)
returns jsonb language sql security definer set search_path=timegenie,extensions,pg_catalog as $$
  with checked as (select timegenie.assert_sync_v1_workspace(p_workspace_id))
  select timegenie.cloud_apply_patch_pre_direct_v2(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_type,p_entity_id,p_base_version,p_payload)
  from checked;
$$;

create or replace function timegenie.cloud_apply_patch(p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_operation_type text,p_entity_type text,p_entity_id text,p_base_version bigint,p_payload_version bigint,p_coalesced_count bigint,p_payload jsonb)
returns jsonb language sql security definer set search_path=timegenie,extensions,pg_catalog as $$
  with checked as (select timegenie.assert_sync_v1_workspace(p_workspace_id))
  select timegenie.cloud_apply_patch_pre_direct_v2(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_type,p_entity_id,p_base_version,p_payload_version,p_coalesced_count,p_payload)
  from checked;
$$;

create or replace function timegenie.cloud_incremental_pull(p_workspace_id uuid,p_device_id uuid,p_after_change_seq bigint default 0,p_limit integer default 200)
returns jsonb language sql security definer set search_path=timegenie,extensions,pg_catalog as $$
  with checked as (select timegenie.assert_sync_v1_workspace(p_workspace_id))
  select timegenie.cloud_incremental_pull_pre_direct_v2(p_workspace_id,p_device_id,p_after_change_seq,p_limit)
  from checked;
$$;

create or replace function timegenie.tracking_lease_takeover_for_timer_write(p_workspace_id uuid,p_device_id uuid,p_lease_token uuid)
returns jsonb language sql security definer set search_path=timegenie,extensions,pg_catalog as $$
  with checked as (select timegenie.assert_sync_v1_workspace(p_workspace_id))
  select timegenie.tracking_lease_takeover_for_timer_write_pre_direct_v2(p_workspace_id,p_device_id,p_lease_token)
  from checked;
$$;

create or replace function timegenie.unassigned_get_or_create_shared(
  p_workspace_id uuid,p_device_id uuid,p_candidate jsonb default null,
  p_predecessor_session_id uuid default null,p_started_at timestamptz default null
) returns jsonb language sql security definer set search_path=timegenie,extensions,pg_catalog as $$
  with checked as (select timegenie.assert_sync_v1_workspace(p_workspace_id))
  select timegenie.unassigned_get_or_create_shared_pre_direct_v2(
    p_workspace_id,p_device_id,p_candidate,p_predecessor_session_id,p_started_at
  ) from checked;
$$;

create or replace function timegenie.unassigned_resolve_shared(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_session_id uuid,
  p_expected_version bigint,p_resolution_type text,p_payload jsonb
) returns jsonb language sql security definer set search_path=timegenie,extensions,pg_catalog as $$
  with checked as (select timegenie.assert_sync_v1_workspace(p_workspace_id))
  select timegenie.unassigned_resolve_shared_pre_direct_v2(
    p_workspace_id,p_device_id,p_operation_id,p_session_id,p_expected_version,p_resolution_type,p_payload
  ) from checked;
$$;

create or replace function timegenie.unassigned_anchor_reset(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_session_id uuid,
  p_expected_version bigint,p_resumed_at_millis bigint default null
) returns jsonb language sql security definer set search_path=timegenie,extensions,pg_catalog as $$
  with checked as (select timegenie.assert_sync_v1_workspace(p_workspace_id))
  select timegenie.unassigned_anchor_reset_pre_direct_v2(
    p_workspace_id,p_device_id,p_operation_id,p_session_id,p_expected_version,p_resumed_at_millis
  ) from checked;
$$;

create or replace function timegenie.actual_adjustment_batch_apply(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_payload jsonb
) returns jsonb language sql security definer set search_path=timegenie,extensions,pg_catalog as $$
  with checked as (select timegenie.assert_sync_v1_workspace(p_workspace_id))
  select timegenie.actual_adjustment_batch_apply_pre_direct_v2(p_workspace_id,p_device_id,p_operation_id,p_payload)
  from checked;
$$;

create or replace function timegenie.timer_transition_apply(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_payload jsonb
) returns jsonb language sql security definer set search_path=timegenie,extensions,pg_catalog as $$
  with checked as (select timegenie.assert_sync_v1_workspace(p_workspace_id))
  select timegenie.timer_transition_apply_pre_direct_v2(p_workspace_id,p_device_id,p_operation_id,p_payload)
  from checked;
$$;

create or replace function timegenie.migration_import_snapshot(
  p_workspace_id uuid,p_operation_id uuid,p_device_id uuid,p_snapshot jsonb
) returns jsonb language sql security definer set search_path=timegenie,extensions,pg_catalog as $$
  with checked as (select timegenie.assert_sync_v1_workspace(p_workspace_id))
  select timegenie.migration_import_snapshot_pre_direct_v2(p_workspace_id,p_operation_id,p_device_id,p_snapshot)
  from checked;
$$;

create or replace function timegenie.workspace_restore_prepare(
  p_workspace_id uuid,p_device_id uuid,p_target_snapshot_id uuid,p_expected_epoch bigint
) returns jsonb language sql security definer set search_path=timegenie,extensions,pg_catalog as $$
  with checked as (select timegenie.assert_sync_v1_workspace(p_workspace_id))
  select timegenie.workspace_restore_prepare_pre_direct_v2(
    p_workspace_id,p_device_id,p_target_snapshot_id,p_expected_epoch
  ) from checked;
$$;

create or replace function timegenie.workspace_restore_prepare(
  p_workspace_id uuid,p_device_id uuid,p_target_snapshot_id uuid,
  p_expected_epoch bigint,p_create_safety_snapshot boolean
) returns jsonb language sql security definer set search_path=timegenie,extensions,pg_catalog as $$
  with checked as (select timegenie.assert_sync_v1_workspace(p_workspace_id))
  select timegenie.workspace_restore_prepare_pre_direct_v2(
    p_workspace_id,p_device_id,p_target_snapshot_id,p_expected_epoch,p_create_safety_snapshot
  ) from checked;
$$;

create or replace function timegenie.workspace_restore_commit(
  p_workspace_id uuid,p_device_id uuid,p_lock_token uuid,p_target_snapshot_id uuid,
  p_target_content_hash text,p_prepared_epoch bigint,p_operation_id uuid,p_confirmed boolean
) returns jsonb language sql security definer set search_path=timegenie,extensions,pg_catalog set statement_timeout='600s' as $$
  with checked as (select timegenie.assert_sync_v1_workspace(p_workspace_id))
  select timegenie.workspace_restore_commit_pre_direct_v2(
    p_workspace_id,p_device_id,p_lock_token,p_target_snapshot_id,
    p_target_content_hash,p_prepared_epoch,p_operation_id,p_confirmed
  ) from checked;
$$;

create or replace function timegenie.timer_start(
  p_workspace_id uuid,p_task_id uuid,p_device_id uuid,p_operation_id uuid,p_note text default null
) returns jsonb language sql security definer set search_path=timegenie,extensions,pg_catalog as $$
  with checked as (select timegenie.assert_sync_v1_workspace(p_workspace_id))
  select timegenie.timer_start_pre_direct_v2(p_workspace_id,p_task_id,p_device_id,p_operation_id,p_note)
  from checked;
$$;

create or replace function timegenie.timer_pause(
  p_workspace_id uuid,p_entry_id uuid,p_device_id uuid,p_expected_version bigint,p_operation_id uuid
) returns jsonb language sql security definer set search_path=timegenie,extensions,pg_catalog as $$
  with checked as (select timegenie.assert_sync_v1_workspace(p_workspace_id))
  select timegenie.timer_pause_pre_direct_v2(p_workspace_id,p_entry_id,p_device_id,p_expected_version,p_operation_id)
  from checked;
$$;

create or replace function timegenie.timer_resume(
  p_workspace_id uuid,p_entry_id uuid,p_device_id uuid,p_expected_version bigint,p_operation_id uuid
) returns jsonb language sql security definer set search_path=timegenie,extensions,pg_catalog as $$
  with checked as (select timegenie.assert_sync_v1_workspace(p_workspace_id))
  select timegenie.timer_resume_pre_direct_v2(p_workspace_id,p_entry_id,p_device_id,p_expected_version,p_operation_id)
  from checked;
$$;

create or replace function timegenie.timer_stop(
  p_workspace_id uuid,p_entry_id uuid,p_device_id uuid,p_expected_version bigint,p_operation_id uuid
) returns jsonb language sql security definer set search_path=timegenie,extensions,pg_catalog as $$
  with checked as (select timegenie.assert_sync_v1_workspace(p_workspace_id))
  select timegenie.timer_stop_pre_direct_v2(p_workspace_id,p_entry_id,p_device_id,p_expected_version,p_operation_id)
  from checked;
$$;

create or replace function timegenie.unassigned_resolve_work(
  p_workspace_id uuid,p_session_id uuid,p_device_id uuid,p_expected_version bigint,
  p_allocations jsonb,p_operation_id uuid
) returns jsonb language sql security definer set search_path=timegenie,extensions,pg_catalog as $$
  with checked as (select timegenie.assert_sync_v1_workspace(p_workspace_id))
  select timegenie.unassigned_resolve_work_pre_direct_v2(
    p_workspace_id,p_session_id,p_device_id,p_expected_version,p_allocations,p_operation_id
  ) from checked;
$$;

create or replace function timegenie.unassigned_resolve_break(
  p_workspace_id uuid,p_session_id uuid,p_device_id uuid,p_expected_version bigint,
  p_allocations jsonb,p_operation_id uuid
) returns jsonb language sql security definer set search_path=timegenie,extensions,pg_catalog as $$
  with checked as (select timegenie.assert_sync_v1_workspace(p_workspace_id))
  select timegenie.unassigned_resolve_break_pre_direct_v2(
    p_workspace_id,p_session_id,p_device_id,p_expected_version,p_allocations,p_operation_id
  ) from checked;
$$;

create or replace function timegenie.unassigned_discard(
  p_workspace_id uuid,p_session_id uuid,p_device_id uuid,p_expected_version bigint,
  p_allocations jsonb,p_operation_id uuid
) returns jsonb language sql security definer set search_path=timegenie,extensions,pg_catalog as $$
  with checked as (select timegenie.assert_sync_v1_workspace(p_workspace_id))
  select timegenie.unassigned_discard_pre_direct_v2(
    p_workspace_id,p_session_id,p_device_id,p_expected_version,p_allocations,p_operation_id
  ) from checked;
$$;

create or replace function timegenie.coordinate_timer_calendar_day(p_workspace_id uuid,p_device_id uuid)
returns jsonb language sql security definer set search_path=timegenie,extensions,pg_catalog as $$
  with checked as (select timegenie.assert_sync_v1_workspace(p_workspace_id))
  select timegenie.coordinate_timer_calendar_day_pre_direct_v2(p_workspace_id,p_device_id)
  from checked;
$$;

create or replace function timegenie.coordinate_unassigned_calendar_day(
  p_workspace_id uuid,p_device_id uuid,p_lease_token uuid
) returns jsonb language sql security definer set search_path=timegenie,extensions,pg_catalog as $$
  with checked as (select timegenie.assert_sync_v1_workspace(p_workspace_id))
  select timegenie.coordinate_unassigned_calendar_day_pre_direct_v2(p_workspace_id,p_device_id,p_lease_token)
  from checked;
$$;

create or replace function timegenie.set_report_template_default(
  p_workspace_id uuid,p_template_id uuid,p_expected_version bigint default null
) returns jsonb language sql security definer set search_path=timegenie,extensions,pg_catalog as $$
  with checked as (select timegenie.assert_sync_v1_workspace(p_workspace_id))
  select timegenie.set_report_template_default_pre_direct_v2(p_workspace_id,p_template_id,p_expected_version)
  from checked;
$$;

revoke all on function timegenie.storage_protocol_get(uuid) from public,anon,authenticated;
revoke all on function timegenie.assert_sync_v1_workspace(uuid) from public,anon,authenticated;
revoke all on function timegenie.assert_direct_v2_workspace(uuid,uuid,bigint) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_context(uuid,uuid,bigint) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_query_metadata(uuid,uuid,bigint,text[]) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_query_response(uuid,uuid,jsonb,text[],bigint) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_sanitize_shared_json(jsonb) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_workbench_bootstrap(uuid,uuid,bigint) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_task_project(uuid,uuid) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_task_list(uuid,uuid,bigint,uuid,boolean,text) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_completed_tasks(uuid,uuid,bigint,uuid,timestamptz,uuid,integer) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_task_detail(uuid,uuid,uuid,bigint) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_today_plan(uuid,uuid,uuid,date,bigint) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_time_entry_project(uuid,uuid,timestamptz) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_time_state(uuid,uuid,date,bigint) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_time_history(uuid,uuid,bigint,date,integer,boolean) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_report_project(uuid,uuid) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_report_list(uuid,uuid,bigint,text,uuid,text,timestamptz,uuid,integer) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_report_detail(uuid,uuid,uuid,bigint) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_report_templates(uuid,uuid,text,bigint) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_report_preview(uuid,uuid,text,date,uuid,uuid,bigint) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_shared_configuration(uuid,uuid,bigint) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_operation_result(uuid,uuid,text,jsonb) from public,anon,authenticated;
revoke all on function timegenie.record_direct_cloud_operation(uuid,uuid,uuid,text,jsonb,jsonb) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_operation_get(uuid,uuid,uuid,text,jsonb) from public,anon,authenticated;
revoke all on function timegenie.storage_protocol_upgrade_preflight(uuid,uuid) from public,anon,authenticated;
revoke all on function timegenie.upgrade_storage_protocol(uuid,uuid,uuid,bigint,text,boolean) from public,anon;
revoke all on function timegenie.provision_direct_v2_workspace(uuid,uuid,uuid) from public,anon;
grant execute on function timegenie.storage_protocol_get(uuid) to authenticated;
grant execute on function timegenie.direct_cloud_context(uuid,uuid,bigint) to authenticated;
grant execute on function timegenie.direct_cloud_query_metadata(uuid,uuid,bigint,text[]) to authenticated;
grant execute on function timegenie.direct_cloud_query_response(uuid,uuid,jsonb,text[],bigint) to authenticated;
grant execute on function timegenie.direct_cloud_workbench_bootstrap(uuid,uuid,bigint) to authenticated;
grant execute on function timegenie.direct_cloud_task_list(uuid,uuid,bigint,uuid,boolean,text) to authenticated;
grant execute on function timegenie.direct_cloud_completed_tasks(uuid,uuid,bigint,uuid,timestamptz,uuid,integer) to authenticated;
grant execute on function timegenie.direct_cloud_task_detail(uuid,uuid,uuid,bigint) to authenticated;
grant execute on function timegenie.direct_cloud_today_plan(uuid,uuid,uuid,date,bigint) to authenticated;
grant execute on function timegenie.direct_cloud_time_state(uuid,uuid,date,bigint) to authenticated;
grant execute on function timegenie.direct_cloud_time_history(uuid,uuid,bigint,date,integer,boolean) to authenticated;
grant execute on function timegenie.direct_cloud_report_list(uuid,uuid,bigint,text,uuid,text,timestamptz,uuid,integer) to authenticated;
grant execute on function timegenie.direct_cloud_report_detail(uuid,uuid,uuid,bigint) to authenticated;
grant execute on function timegenie.direct_cloud_report_templates(uuid,uuid,text,bigint) to authenticated;
grant execute on function timegenie.direct_cloud_report_preview(uuid,uuid,text,date,uuid,uuid,bigint) to authenticated;
grant execute on function timegenie.direct_cloud_shared_configuration(uuid,uuid,bigint) to authenticated;
grant execute on function timegenie.direct_cloud_operation_get(uuid,uuid,uuid,text,jsonb) to authenticated;
grant execute on function timegenie.storage_protocol_upgrade_preflight(uuid,uuid) to authenticated;
grant execute on function timegenie.upgrade_storage_protocol(uuid,uuid,uuid,bigint,text,boolean) to authenticated;
grant execute on function timegenie.provision_direct_v2_workspace(uuid,uuid,uuid) to authenticated;

grant execute on function timegenie.tracking_lease_acquire(uuid,uuid,uuid) to authenticated;
grant execute on function timegenie.tracking_lease_renew(uuid,uuid,uuid) to authenticated;
grant execute on function timegenie.tracking_lease_release(uuid,uuid,uuid) to authenticated;
grant execute on function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,jsonb) to authenticated;
grant execute on function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,bigint,bigint,jsonb) to authenticated;
grant execute on function timegenie.cloud_incremental_pull(uuid,uuid,bigint,integer) to authenticated;

revoke execute on function timegenie.tracking_lease_acquire_pre_direct_v2(uuid,uuid,uuid) from public,anon,authenticated;
revoke execute on function timegenie.tracking_lease_renew_pre_direct_v2(uuid,uuid,uuid) from public,anon,authenticated;
revoke execute on function timegenie.tracking_lease_release_pre_direct_v2(uuid,uuid,uuid) from public,anon,authenticated;
revoke execute on function timegenie.cloud_apply_patch_pre_direct_v2(uuid,uuid,uuid,text,text,text,bigint,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.cloud_apply_patch_pre_direct_v2(uuid,uuid,uuid,text,text,text,bigint,bigint,bigint,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.cloud_incremental_pull_pre_direct_v2(uuid,uuid,bigint,integer) from public,anon,authenticated;

revoke execute on function timegenie.tracking_lease_takeover_for_timer_write(uuid,uuid,uuid) from public,anon;
revoke execute on function timegenie.unassigned_get_or_create_shared(uuid,uuid,jsonb,uuid,timestamptz) from public,anon;
revoke execute on function timegenie.unassigned_resolve_shared(uuid,uuid,uuid,uuid,bigint,text,jsonb) from public,anon;
revoke execute on function timegenie.unassigned_anchor_reset(uuid,uuid,uuid,uuid,bigint,bigint) from public,anon;
revoke execute on function timegenie.actual_adjustment_batch_apply(uuid,uuid,uuid,jsonb) from public,anon;
revoke execute on function timegenie.timer_transition_apply(uuid,uuid,uuid,jsonb) from public,anon;
revoke execute on function timegenie.migration_import_snapshot(uuid,uuid,uuid,jsonb) from public,anon;
revoke execute on function timegenie.workspace_restore_prepare(uuid,uuid,uuid,bigint) from public,anon;
revoke execute on function timegenie.workspace_restore_prepare(uuid,uuid,uuid,bigint,boolean) from public,anon;
revoke execute on function timegenie.workspace_restore_commit(uuid,uuid,uuid,uuid,text,bigint,uuid,boolean) from public,anon;
revoke execute on function timegenie.timer_start(uuid,uuid,uuid,uuid,text) from public,anon;
revoke execute on function timegenie.timer_pause(uuid,uuid,uuid,bigint,uuid) from public,anon;
revoke execute on function timegenie.timer_resume(uuid,uuid,uuid,bigint,uuid) from public,anon;
revoke execute on function timegenie.timer_stop(uuid,uuid,uuid,bigint,uuid) from public,anon;
revoke execute on function timegenie.unassigned_resolve_work(uuid,uuid,uuid,bigint,jsonb,uuid) from public,anon;
revoke execute on function timegenie.unassigned_resolve_break(uuid,uuid,uuid,bigint,jsonb,uuid) from public,anon;
revoke execute on function timegenie.unassigned_discard(uuid,uuid,uuid,bigint,jsonb,uuid) from public,anon;
revoke execute on function timegenie.coordinate_timer_calendar_day(uuid,uuid) from public,anon;
revoke execute on function timegenie.coordinate_unassigned_calendar_day(uuid,uuid,uuid) from public,anon;
revoke execute on function timegenie.set_report_template_default(uuid,uuid,bigint) from public,anon;

grant execute on function timegenie.tracking_lease_takeover_for_timer_write(uuid,uuid,uuid) to authenticated;
grant execute on function timegenie.unassigned_get_or_create_shared(uuid,uuid,jsonb,uuid,timestamptz) to authenticated;
grant execute on function timegenie.unassigned_resolve_shared(uuid,uuid,uuid,uuid,bigint,text,jsonb) to authenticated;
grant execute on function timegenie.unassigned_anchor_reset(uuid,uuid,uuid,uuid,bigint,bigint) to authenticated;
grant execute on function timegenie.actual_adjustment_batch_apply(uuid,uuid,uuid,jsonb) to authenticated;
grant execute on function timegenie.timer_transition_apply(uuid,uuid,uuid,jsonb) to authenticated;
grant execute on function timegenie.migration_import_snapshot(uuid,uuid,uuid,jsonb) to authenticated;
grant execute on function timegenie.workspace_restore_prepare(uuid,uuid,uuid,bigint) to authenticated;
grant execute on function timegenie.workspace_restore_prepare(uuid,uuid,uuid,bigint,boolean) to authenticated;
grant execute on function timegenie.workspace_restore_commit(uuid,uuid,uuid,uuid,text,bigint,uuid,boolean) to authenticated;
grant execute on function timegenie.timer_start(uuid,uuid,uuid,uuid,text) to authenticated;
grant execute on function timegenie.timer_pause(uuid,uuid,uuid,bigint,uuid) to authenticated;
grant execute on function timegenie.timer_resume(uuid,uuid,uuid,bigint,uuid) to authenticated;
grant execute on function timegenie.timer_stop(uuid,uuid,uuid,bigint,uuid) to authenticated;
grant execute on function timegenie.unassigned_resolve_work(uuid,uuid,uuid,bigint,jsonb,uuid) to authenticated;
grant execute on function timegenie.unassigned_resolve_break(uuid,uuid,uuid,bigint,jsonb,uuid) to authenticated;
grant execute on function timegenie.unassigned_discard(uuid,uuid,uuid,bigint,jsonb,uuid) to authenticated;
grant execute on function timegenie.coordinate_timer_calendar_day(uuid,uuid) to authenticated;
grant execute on function timegenie.coordinate_unassigned_calendar_day(uuid,uuid,uuid) to authenticated;
grant execute on function timegenie.set_report_template_default(uuid,uuid,bigint) to authenticated;

revoke execute on function timegenie.tracking_lease_takeover_for_timer_write_pre_direct_v2(uuid,uuid,uuid) from public,anon,authenticated;
revoke execute on function timegenie.unassigned_get_or_create_shared_pre_direct_v2(uuid,uuid,jsonb,uuid,timestamptz) from public,anon,authenticated;
revoke execute on function timegenie.unassigned_resolve_shared_pre_direct_v2(uuid,uuid,uuid,uuid,bigint,text,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.unassigned_anchor_reset_pre_direct_v2(uuid,uuid,uuid,uuid,bigint,bigint) from public,anon,authenticated;
revoke execute on function timegenie.actual_adjustment_batch_apply_pre_direct_v2(uuid,uuid,uuid,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.timer_transition_apply_pre_direct_v2(uuid,uuid,uuid,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.migration_import_snapshot_pre_direct_v2(uuid,uuid,uuid,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.workspace_restore_prepare_pre_direct_v2(uuid,uuid,uuid,bigint) from public,anon,authenticated;
revoke execute on function timegenie.workspace_restore_prepare_pre_direct_v2(uuid,uuid,uuid,bigint,boolean) from public,anon,authenticated;
revoke execute on function timegenie.workspace_restore_commit_pre_direct_v2(uuid,uuid,uuid,uuid,text,bigint,uuid,boolean) from public,anon,authenticated;
revoke execute on function timegenie.timer_start_pre_direct_v2(uuid,uuid,uuid,uuid,text) from public,anon,authenticated;
revoke execute on function timegenie.timer_pause_pre_direct_v2(uuid,uuid,uuid,bigint,uuid) from public,anon,authenticated;
revoke execute on function timegenie.timer_resume_pre_direct_v2(uuid,uuid,uuid,bigint,uuid) from public,anon,authenticated;
revoke execute on function timegenie.timer_stop_pre_direct_v2(uuid,uuid,uuid,bigint,uuid) from public,anon,authenticated;
revoke execute on function timegenie.unassigned_resolve_work_pre_direct_v2(uuid,uuid,uuid,bigint,jsonb,uuid) from public,anon,authenticated;
revoke execute on function timegenie.unassigned_resolve_break_pre_direct_v2(uuid,uuid,uuid,bigint,jsonb,uuid) from public,anon,authenticated;
revoke execute on function timegenie.unassigned_discard_pre_direct_v2(uuid,uuid,uuid,bigint,jsonb,uuid) from public,anon,authenticated;
revoke execute on function timegenie.coordinate_timer_calendar_day_pre_direct_v2(uuid,uuid) from public,anon,authenticated;
revoke execute on function timegenie.coordinate_unassigned_calendar_day_pre_direct_v2(uuid,uuid,uuid) from public,anon,authenticated;
revoke execute on function timegenie.set_report_template_default_pre_direct_v2(uuid,uuid,bigint) from public,anon,authenticated;

notify pgrst, 'reload schema';
