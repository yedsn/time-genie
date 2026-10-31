-- Daily report candidates and item status are derived strictly from activity
-- inside the requested report period.

\ir 20261029_timegenie_report_preview_task_aggregation.sql

begin;

create or replace function timegenie.direct_cloud_report_scope_task_ids(
  p_workspace_id uuid,p_subject_id uuid,p_report_type text,p_period_start date,p_period_end date,p_requested jsonb
) returns uuid[] language plpgsql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare result_value uuid[]; requested_count integer; matched_count integer;
begin
  if p_requested is not null and jsonb_typeof(p_requested)<>'array' then
    raise exception 'VALIDATION_ERROR: 报告事项范围必须是数组';
  end if;
  if coalesce(jsonb_array_length(coalesce(p_requested,'[]'::jsonb)),0)>0 then
    begin
      with requested as (
        select (value#>>'{}')::uuid as id,min(ordinality) as ordinality
        from jsonb_array_elements(p_requested) with ordinality item(value,ordinality)
        group by (value#>>'{}')::uuid
      )
      select array_agg(requested.id order by requested.ordinality),count(*)
        into result_value,requested_count from requested;
    exception when invalid_text_representation then
      raise exception 'VALIDATION_ERROR: 报告事项标识无效';
    end;
    select count(*) into matched_count from timegenie.tasks task
    where task.workspace_id=p_workspace_id and task.subject_id=p_subject_id
      and task.id=any(result_value) and task.deleted_at is null
      and (task.day_instance_date is null or task.day_instance_source_task_id is null);
    if matched_count<>requested_count then raise exception 'NOT_FOUND: 报告事项不存在或不属于当前主体'; end if;
    return result_value;
  end if;

  with actuals as (
    select coalesce(task.day_instance_source_task_id,task.id) as task_id,sum(value.minutes)::bigint as minutes
    from (
      select allocation.task_id,allocation.minutes::bigint as minutes
      from timegenie.time_allocations allocation
      join timegenie.time_entries entry on entry.id=allocation.entry_id
      where allocation.workspace_id=p_workspace_id and entry.work_date between p_period_start and p_period_end
        and entry.deleted_at is null
      union all
      select adjustment.task_id,adjustment.delta_minutes::bigint
      from timegenie.actual_time_adjustments adjustment
      where adjustment.workspace_id=p_workspace_id and adjustment.work_date between p_period_start and p_period_end
        and adjustment.deleted_at is null
    ) value join timegenie.tasks task on task.id=value.task_id
    group by coalesce(task.day_instance_source_task_id,task.id)
  ), completed as (
    select task.id as task_id from timegenie.tasks task
    where task.workspace_id=p_workspace_id and task.subject_id=p_subject_id and task.deleted_at is null
      and (task.day_instance_date is null or task.day_instance_source_task_id is null)
      and task.status='done'
      and task.completed_at>=timegenie.workspace_day_start(p_workspace_id,p_period_start)
      and task.completed_at<timegenie.workspace_day_end(p_workspace_id,p_period_end)
    union
    select occurrence.task_id from timegenie.task_occurrences occurrence
    join timegenie.tasks task on task.id=occurrence.task_id and task.workspace_id=occurrence.workspace_id
    where occurrence.workspace_id=p_workspace_id and task.subject_id=p_subject_id and task.deleted_at is null
      and occurrence.occurrence_date between p_period_start and p_period_end and occurrence.status='done'
    union
    select instance.day_instance_source_task_id from timegenie.tasks instance
    where instance.workspace_id=p_workspace_id and instance.subject_id=p_subject_id and instance.deleted_at is null
      and instance.day_instance_source_task_id is not null and instance.status='done'
      and instance.day_instance_date between p_period_start and p_period_end
  ), candidates as (
    select task.id,task.sort_order,task.created_at
    from timegenie.tasks task left join actuals on actuals.task_id=task.id
    where task.workspace_id=p_workspace_id and task.subject_id=p_subject_id and task.deleted_at is null
      and (task.day_instance_date is null or task.day_instance_source_task_id is null)
      and (coalesce(actuals.minutes,0)>0 or exists(select 1 from completed where completed.task_id=task.id))
  )
  select coalesce(array_agg(id order by sort_order,created_at,id),'{}'::uuid[]) into result_value from candidates;
  return result_value;
end $$;

create or replace function timegenie.direct_cloud_report_task_suggestions(
  p_workspace_id uuid,p_device_id uuid,p_report_type text,p_reference_date date,p_subject_id uuid,
  p_include_task_ids uuid[] default '{}'::uuid[],p_expected_epoch bigint default null
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare period_value jsonb; period_start_value date; period_end_value date; data_value jsonb;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  period_value:=timegenie.direct_cloud_report_period(p_report_type,p_reference_date);
  period_start_value:=(period_value->>'periodStart')::date;
  period_end_value:=(period_value->>'periodEnd')::date;
  if not exists(select 1 from timegenie.subjects subject where subject.workspace_id=p_workspace_id and subject.id=p_subject_id and subject.deleted_at is null) then
    raise exception 'NOT_FOUND: 主体不存在';
  end if;

  with recursive actuals as (
    select coalesce(task.day_instance_source_task_id,task.id) as task_id,sum(value.minutes)::bigint as minutes
    from (
      select allocation.task_id,allocation.minutes::bigint as minutes
      from timegenie.time_allocations allocation join timegenie.time_entries entry on entry.id=allocation.entry_id
      where allocation.workspace_id=p_workspace_id and entry.work_date between period_start_value and period_end_value and entry.deleted_at is null
      union all
      select adjustment.task_id,adjustment.delta_minutes::bigint
      from timegenie.actual_time_adjustments adjustment
      where adjustment.workspace_id=p_workspace_id and adjustment.work_date between period_start_value and period_end_value and adjustment.deleted_at is null
    ) value join timegenie.tasks task on task.id=value.task_id
    group by coalesce(task.day_instance_source_task_id,task.id)
  ), completed as (
    select task.id as task_id from timegenie.tasks task
    where task.workspace_id=p_workspace_id and task.subject_id=p_subject_id and task.deleted_at is null
      and (task.day_instance_date is null or task.day_instance_source_task_id is null) and task.status='done'
      and task.completed_at>=timegenie.workspace_day_start(p_workspace_id,period_start_value)
      and task.completed_at<timegenie.workspace_day_end(p_workspace_id,period_end_value)
    union
    select occurrence.task_id from timegenie.task_occurrences occurrence
    join timegenie.tasks task on task.id=occurrence.task_id and task.workspace_id=occurrence.workspace_id
    where occurrence.workspace_id=p_workspace_id and task.subject_id=p_subject_id and task.deleted_at is null
      and occurrence.occurrence_date between period_start_value and period_end_value and occurrence.status='done'
    union
    select instance.day_instance_source_task_id from timegenie.tasks instance
    where instance.workspace_id=p_workspace_id and instance.subject_id=p_subject_id and instance.deleted_at is null
      and instance.day_instance_source_task_id is not null and instance.status='done'
      and instance.day_instance_date between period_start_value and period_end_value
  ), selectable as (
    select task.id from timegenie.tasks task left join actuals on actuals.task_id=task.id
    where task.workspace_id=p_workspace_id and task.subject_id=p_subject_id and task.deleted_at is null
      and (task.day_instance_date is null or task.day_instance_source_task_id is null)
      and (coalesce(actuals.minutes,0)>0 or exists(select 1 from completed where completed.task_id=task.id)
        or task.id=any(coalesce(p_include_task_ids,'{}'::uuid[])))
  ), included as (
    select id from selectable
    union
    select parent.id from included child
    join timegenie.tasks child_task on child_task.id=child.id
    join timegenie.tasks parent on parent.id=child_task.parent_id
    where parent.workspace_id=p_workspace_id and parent.subject_id=p_subject_id and parent.deleted_at is null
  ), tree as (
    select task.id,array[(lpad(task.sort_order::text,20,'0')||':'||task.id::text)] as order_path
    from timegenie.tasks task
    where task.workspace_id=p_workspace_id and task.subject_id=p_subject_id and task.parent_id is null
      and task.deleted_at is null and (task.day_instance_date is null or task.day_instance_source_task_id is null)
    union all
    select child.id,parent.order_path||(lpad(child.sort_order::text,20,'0')||':'||child.id::text)
    from tree parent join timegenie.tasks child on child.parent_id=parent.id
    where child.workspace_id=p_workspace_id and child.subject_id=p_subject_id and child.deleted_at is null
      and (child.day_instance_date is null or child.day_instance_source_task_id is null)
  ), rows as (
    select task.*,tree.order_path,coalesce(actuals.minutes,0) as actual_minutes,
      exists(select 1 from completed where completed.task_id=task.id) as period_completed,
      exists(select 1 from selectable where selectable.id=task.id) as is_selectable,
      coalesce((select instance_estimate.estimate_minutes from timegenie.tasks instance
         join timegenie.task_daily_estimates instance_estimate on instance_estimate.workspace_id=instance.workspace_id and instance_estimate.task_id=instance.id and instance_estimate.work_date=period_start_value
         where instance.workspace_id=p_workspace_id and instance.day_instance_source_task_id=task.id and instance.day_instance_date=period_start_value and instance.deleted_at is null limit 1),
        (select estimate.estimate_minutes from timegenie.task_daily_estimates estimate
         where estimate.workspace_id=p_workspace_id and estimate.task_id=task.id and estimate.work_date=period_start_value)) as daily_estimate_minutes
    from included join timegenie.tasks task on task.id=included.id
    join tree on tree.id=task.id left join actuals on actuals.task_id=task.id
  )
  select jsonb_build_object('tasks',coalesce(jsonb_agg(jsonb_build_object(
    'taskId',row.id,'parentId',row.parent_id,'title',row.title,'status',row.status,
    'estimateMinutes',row.estimate_minutes,'actualMinutes',row.actual_minutes,
    'dailyEstimateMinutes',row.daily_estimate_minutes,'sortOrder',row.sort_order,
    'periodStatus',case when not row.is_selectable then 'context' when row.period_completed then 'completed' when row.actual_minutes>0 then 'active' else 'included' end,
    'defaultSelected',row.is_selectable and (row.period_completed or row.actual_minutes>0),
    'candidateRole',case when row.is_selectable then 'selectable' else 'context' end
  ) order by row.order_path),'[]'::jsonb)) into data_value from rows row;
  return timegenie.direct_cloud_query_response(p_workspace_id,p_device_id,data_value,array['reports','tasks','time'],p_expected_epoch);
end $$;

revoke all on function timegenie.direct_cloud_report_scope_task_ids(uuid,uuid,text,date,date,jsonb) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_report_task_suggestions(uuid,uuid,text,date,uuid,uuid[],bigint) from public,anon;
grant execute on function timegenie.direct_cloud_report_task_suggestions(uuid,uuid,text,date,uuid,uuid[],bigint) to authenticated;

commit;
