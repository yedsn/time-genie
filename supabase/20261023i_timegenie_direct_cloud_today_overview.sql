-- Direct-v2 today overview. The dashboard reads one authoritative cloud
-- projection and never rebuilds the view from SQLite business tables.

begin;

create or replace function timegenie.direct_cloud_today_overview(
  p_workspace_id uuid,p_device_id uuid,p_subject_id uuid default null,
  p_today_date date default null,p_history_days integer default 30,p_trend_days integer default 14,
  p_expected_epoch bigint default null
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare today_value date; start_value date; data_value jsonb; server_time_value timestamptz:=now();
declare subject_name_value text; timezone_name_value text;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  if p_history_days<1 or p_history_days>90 then raise exception 'VALIDATION_ERROR: 历史天数必须在 1 到 90 之间'; end if;
  if p_trend_days<1 or p_trend_days>p_history_days then raise exception 'VALIDATION_ERROR: 趋势天数超出范围'; end if;
  today_value:=coalesce(p_today_date,timegenie.workspace_work_date(p_workspace_id,server_time_value));
  start_value:=today_value-(p_history_days-1);
  select workspace.timezone into timezone_name_value from timegenie.workspaces workspace where workspace.id=p_workspace_id;
  if p_subject_id is not null then
    select subject.name into subject_name_value from timegenie.subjects subject
    where subject.workspace_id=p_workspace_id and subject.id=p_subject_id and subject.deleted_at is null;
    if not found then raise exception 'NOT_FOUND: 主体不存在'; end if;
  else subject_name_value:='全部主体'; end if;

  with recursive dates as (
    select generate_series(start_value,today_value,interval '1 day')::date as work_date
  ), task_paths as (
    with recursive paths as (
      select task.id,task.id as descendant_id,task.parent_id,task.title,0 as distance
      from timegenie.tasks task
      where task.workspace_id=p_workspace_id and task.deleted_at is null
      union all
      select parent.id,path.descendant_id,parent.parent_id,parent.title,path.distance+1
      from timegenie.tasks parent join paths path on path.parent_id=parent.id
      where parent.workspace_id=p_workspace_id and parent.deleted_at is null
    )
    select descendant_id,string_agg(title,' / ' order by distance desc) as path_label
    from paths group by descendant_id
  ), today_actuals as (
    select value.task_id,sum(value.minutes)::bigint as actual_minutes,max(value.started_at) as last_entry_at
    from (
      select allocation.task_id,allocation.minutes,entry.started_at
      from timegenie.time_allocations allocation
      join timegenie.time_entries entry on entry.id=allocation.entry_id
      where allocation.workspace_id=p_workspace_id and entry.work_date=today_value and entry.deleted_at is null
      union all
      select adjustment.task_id,adjustment.delta_minutes,adjustment.created_at
      from timegenie.actual_time_adjustments adjustment
      where adjustment.workspace_id=p_workspace_id and adjustment.work_date=today_value and adjustment.deleted_at is null
    ) value group by value.task_id
  ), daily_actuals as (
    select value.work_date,sum(value.minutes)::bigint as actual_minutes
    from (
      select entry.work_date,allocation.minutes
      from timegenie.time_allocations allocation
      join timegenie.time_entries entry on entry.id=allocation.entry_id
      join timegenie.tasks task on task.id=allocation.task_id
      where allocation.workspace_id=p_workspace_id and entry.work_date between start_value and today_value
        and entry.deleted_at is null and (p_subject_id is null or task.subject_id=p_subject_id)
      union all
      select adjustment.work_date,adjustment.delta_minutes
      from timegenie.actual_time_adjustments adjustment
      where adjustment.workspace_id=p_workspace_id and adjustment.work_date between start_value and today_value
        and adjustment.deleted_at is null and (p_subject_id is null or adjustment.subject_id=p_subject_id)
    ) value group by value.work_date
  ), daily_estimate_totals as (
    select estimate.work_date,sum(estimate.estimate_minutes)::bigint as estimated_minutes
    from timegenie.task_daily_estimates estimate join timegenie.tasks task on task.id=estimate.task_id
    where estimate.workspace_id=p_workspace_id and estimate.work_date between start_value and today_value
      and task.deleted_at is null and (p_subject_id is null or task.subject_id=p_subject_id)
    group by estimate.work_date
  ), daily_completions as (
    select day.work_date,count(task.id)::bigint as completed_count
    from dates day left join timegenie.tasks task on task.workspace_id=p_workspace_id and task.deleted_at is null
      and task.status='done' and task.completed_at>=day.work_date::timestamp at time zone timezone_name_value
      and task.completed_at<(day.work_date+1)::timestamp at time zone timezone_name_value
      and (p_subject_id is null or task.subject_id=p_subject_id)
    group by day.work_date
  ), daily_task_totals as (
    select day.work_date,count(task.id)::bigint as total_count
    from dates day left join timegenie.tasks task on task.workspace_id=p_workspace_id and task.deleted_at is null
      and (p_subject_id is null or task.subject_id=p_subject_id)
      and ((task.planned_date is not null and task.planned_date<=day.work_date)
        or exists(select 1 from timegenie.task_daily_estimates estimate where estimate.task_id=task.id and estimate.work_date=day.work_date))
    group by day.work_date
  ), daily_unassigned as (
    select session.work_date,sum(session.duration_seconds)::bigint as unassigned_seconds
    from timegenie.unassigned_sessions session
    where p_subject_id is null and session.workspace_id=p_workspace_id
      and session.work_date between start_value and today_value
      and session.state in('collecting','awaiting_resolution')
    group by session.work_date
  ), task_rows as (
    select task.*,subject.name as subject_name,
      coalesce(actual.actual_minutes,0) as actual_minutes,actual.last_entry_at,estimate.estimate_minutes as today_estimate,
      coalesce(path.path_label,task.title) as path_label,
      exists(select 1 from timegenie.tasks child where child.workspace_id=p_workspace_id and child.parent_id=task.id and child.deleted_at is null) as has_children,
      exists(select 1 from timegenie.task_day_plans plan where plan.workspace_id=p_workspace_id and plan.task_id=task.id and plan.work_date=today_value and plan.state='included')
      or exists(select 1 from timegenie.task_daily_estimates estimate where estimate.workspace_id=p_workspace_id and estimate.task_id=task.id and estimate.work_date=today_value)
      or exists(select 1 from timegenie.task_occurrences occurrence where occurrence.workspace_id=p_workspace_id and occurrence.task_id=task.id and occurrence.occurrence_date=today_value)
      or exists(select 1 from timegenie.task_recurrence_rules rule where rule.workspace_id=p_workspace_id and rule.task_id=task.id
        and rule.effective_start<=today_value and (rule.effective_end is null or rule.effective_end>=today_value)
        and (rule.frequency='daily' or (rule.frequency='weekdays' and extract(isodow from today_value)<6)
          or (rule.frequency='weekly' and (rule.weekdays_mask & (1 << (extract(isodow from today_value)::integer-1)))<>0)))
      or (task.planned_date is not null and task.planned_date<=today_value)
      or coalesce(actual.actual_minutes,0)<>0 as today_member
    from timegenie.tasks task join timegenie.subjects subject on subject.id=task.subject_id
    left join today_actuals actual on actual.task_id=task.id
    left join timegenie.task_daily_estimates estimate on estimate.workspace_id=p_workspace_id and estimate.task_id=task.id and estimate.work_date=today_value
    left join task_paths path on path.descendant_id=task.id
    where task.workspace_id=p_workspace_id and task.deleted_at is null
      and (p_subject_id is null or task.subject_id=p_subject_id)
  ), day_stats as (
    select day.work_date,
      coalesce(actual.actual_minutes,0) as actual_minutes,coalesce(estimate.estimated_minutes,0) as estimated_minutes,
      coalesce(completion.completed_count,0) as completed_count,coalesce(total.total_count,0) as total_count,
      coalesce(unassigned.unassigned_seconds,0) as unassigned_seconds
    from dates day left join daily_actuals actual on actual.work_date=day.work_date
    left join daily_estimate_totals estimate on estimate.work_date=day.work_date
    left join daily_completions completion on completion.work_date=day.work_date
    left join daily_task_totals total on total.work_date=day.work_date
    left join daily_unassigned unassigned on unassigned.work_date=day.work_date
  ), timeline_rows as (
    select entry.id,entry.kind as item_type,entry.default_task_id as task_id,entry.label_snapshot as task_title,
      task.subject_id,subject.name as subject_name,(extract(epoch from entry.started_at)*1000)::bigint as started_at,
      case when entry.ended_at is null then null else (extract(epoch from entry.ended_at)*1000)::bigint end as ended_at,
      case when entry.state='running' then greatest(extract(epoch from(server_time_value-entry.started_at))::bigint,0) else entry.duration_seconds end as duration_seconds,entry.state
    from timegenie.time_entries entry left join timegenie.tasks task on task.id=entry.default_task_id left join timegenie.subjects subject on subject.id=task.subject_id
    where entry.workspace_id=p_workspace_id and entry.work_date=today_value and entry.deleted_at is null and (p_subject_id is null or task.subject_id=p_subject_id)
    union all
    select adjustment.id,'actual_adjustment',adjustment.task_id,task.title,task.subject_id,subject.name,
      (extract(epoch from adjustment.created_at)*1000)::bigint,(extract(epoch from adjustment.created_at)*1000)::bigint,adjustment.delta_minutes*60,null
    from timegenie.actual_time_adjustments adjustment join timegenie.tasks task on task.id=adjustment.task_id join timegenie.subjects subject on subject.id=task.subject_id
    where adjustment.workspace_id=p_workspace_id and adjustment.work_date=today_value and adjustment.deleted_at is null and (p_subject_id is null or task.subject_id=p_subject_id)
  )
  select jsonb_build_object(
    'scope',jsonb_build_object('subjectId',p_subject_id,'subjectName',subject_name_value,'todayDate',today_value),
    'summary',(select jsonb_build_object(
      'actualMinutes',coalesce(sum(task.actual_minutes),0),
      'estimatedMinutes',coalesce(sum(case when not task.has_children then coalesce(task.today_estimate,task.estimate_minutes,0) else 0 end),0),
      'completedCount',count(*) filter(where task.today_member and task.status='done'),
      'activeCount',count(*) filter(where task.today_member and task.status<>'done' and task.actual_minutes>0),
      'notStartedCount',count(*) filter(where task.today_member and task.status<>'done' and task.actual_minutes=0),
      'unassignedMinutes',case when p_subject_id is null then coalesce((select (unassigned_seconds+59)/60 from day_stats where work_date=today_value),0) else 0 end,
      'completionRate',case when count(*) filter(where task.today_member)=0 then 0 else (count(*) filter(where task.today_member and task.status='done'))::double precision/(count(*) filter(where task.today_member)) end
    ) from task_rows task),
    'days',(select coalesce(jsonb_agg(jsonb_build_object('date',day.work_date,'actualMinutes',day.actual_minutes,'estimatedMinutes',day.estimated_minutes,'completedCount',day.completed_count,'totalCount',day.total_count,'unassignedMinutes',(day.unassigned_seconds+59)/60) order by day.work_date),'[]'::jsonb) from day_stats day),
    'timeline',(select coalesce(jsonb_agg(jsonb_build_object('id',item.id,'itemType',item.item_type,'taskId',item.task_id,'taskTitle',item.task_title,'subjectId',item.subject_id,'subjectName',item.subject_name,'startedAt',item.started_at,'endedAt',item.ended_at,'minutes',case when item.duration_seconds<=0 then 0 else (item.duration_seconds+59)/60 end,'state',item.state) order by item.started_at,item.id),'[]'::jsonb) from timeline_rows item),
    'tasks',(select coalesce(jsonb_agg(jsonb_build_object('taskId',task.id,'title',task.title,'pathLabel',task.path_label,'subjectId',task.subject_id,'subjectName',task.subject_name,'status',case when task.status='done' then 'done' when task.actual_minutes>0 then 'started' else 'not_started' end,'actualMinutes',task.actual_minutes,'todayEstimateMinutes',task.today_estimate,'estimateMinutes',task.estimate_minutes,'lastEntryAt',case when task.last_entry_at is null then null else (extract(epoch from task.last_entry_at)*1000)::bigint end,'selectable',not task.has_children) order by task.sort_order,task.created_at,task.id) filter(where task.today_member),'[]'::jsonb) from task_rows task),
    'revision',(select coalesce(max(change.change_seq),0) from timegenie.workspace_changes change where change.workspace_id=p_workspace_id)
  ) into data_value;
  return timegenie.direct_cloud_query_response(p_workspace_id,p_device_id,data_value,array['tasks','planning','time','unassigned'],p_expected_epoch);
end $$;

revoke all on function timegenie.direct_cloud_today_overview(uuid,uuid,uuid,date,integer,integer,bigint) from public,anon;
grant execute on function timegenie.direct_cloud_today_overview(uuid,uuid,uuid,date,integer,integer,bigint) to authenticated;

commit;

notify pgrst,'reload schema';
