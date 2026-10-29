-- Keep report preview and saved report generation on the same stable-task scope
-- and render aggregated actual minutes for every selected report item.

begin;

create or replace function timegenie.direct_cloud_report_duration(
  p_minutes bigint,p_format text
) returns text language sql immutable
set search_path=timegenie,extensions,pg_catalog as $$
  select case
    when coalesce(p_format,'minutes')='hours' then round(greatest(coalesce(p_minutes,0),0)::numeric/60,1)::text||'h'
    when greatest(coalesce(p_minutes,0),0)<60 then greatest(coalesce(p_minutes,0),0)::text||'min'
    when greatest(coalesce(p_minutes,0),0)%60=0 then (greatest(coalesce(p_minutes,0),0)/60)::text||'h'
    else (greatest(coalesce(p_minutes,0),0)/60)::text||'h'||(greatest(coalesce(p_minutes,0),0)%60)::text||'min'
  end;
$$;

create or replace function timegenie.direct_cloud_report_render(
  p_workspace_id uuid,p_report_type text,p_subject_id uuid,p_period_start date,p_period_end date,
  p_template_id uuid,p_task_ids uuid[],p_allow_deleted_template boolean default false
) returns jsonb language plpgsql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare template_row timegenie.report_templates%rowtype; subject_name_value text; user_name_value text;
declare duration_format_value text; task_lines text; tomorrow_plan text; daily_collection text; total_minutes_value bigint;
declare markdown_value text; weekday_value text; statistics_value text; input_value jsonb; input_hash text;
begin
  select subject.name into subject_name_value from timegenie.subjects subject
  where subject.workspace_id=p_workspace_id and subject.id=p_subject_id and subject.deleted_at is null;
  if not found then raise exception 'NOT_FOUND: 主体不存在'; end if;
  if p_template_id is not null then
    select template.* into template_row from timegenie.report_templates template
    where template.workspace_id=p_workspace_id and template.id=p_template_id and template.report_type=p_report_type
      and (p_allow_deleted_template or template.deleted_at is null);
  else
    select template.* into template_row from timegenie.report_templates template
    where template.workspace_id=p_workspace_id and template.report_type=p_report_type
      and template.is_default and template.deleted_at is null
    order by template.sort_order,template.id limit 1;
  end if;
  if not found then raise exception 'NOT_FOUND: 报告模板不存在'; end if;

  select coalesce(value_json#>>'{}','晓健') into user_name_value
  from timegenie.app_settings where workspace_id=p_workspace_id and key='user_name';
  user_name_value:=coalesce(nullif(user_name_value,''),'晓健');
  select coalesce(value_json#>>'{}','minutes') into duration_format_value
  from timegenie.app_settings where workspace_id=p_workspace_id and key='report_duration_format';
  duration_format_value:=case when duration_format_value='hours' then 'hours' else 'minutes' end;

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
  ), selected as (
    select item.task_id,item.ordinality,task.title,
      timegenie.direct_cloud_report_task_path(p_workspace_id,item.task_id) as path_label,
      coalesce(actuals.minutes,0) as actual_minutes
    from unnest(coalesce(p_task_ids,'{}'::uuid[])) with ordinality item(task_id,ordinality)
    join timegenie.tasks task on task.workspace_id=p_workspace_id and task.id=item.task_id
    left join actuals on actuals.task_id=item.task_id
  )
  select string_agg(
    ordinality::text||'. '||coalesce(path_label,title)||
      case when actual_minutes>0 then ' <实际：'||timegenie.direct_cloud_report_duration(actual_minutes,duration_format_value)||'>' else '' end,
    E'\n' order by ordinality
  ) into task_lines from selected;

  select coalesce(sum(value.minutes),0)::bigint into total_minutes_value from (
    select allocation.minutes::bigint as minutes
    from timegenie.time_allocations allocation join timegenie.time_entries entry on entry.id=allocation.entry_id
    join timegenie.tasks task on task.id=allocation.task_id
    where allocation.workspace_id=p_workspace_id and entry.work_date between p_period_start and p_period_end
      and entry.deleted_at is null and coalesce(task.day_instance_source_task_id,task.id)=any(coalesce(p_task_ids,'{}'::uuid[]))
    union all
    select adjustment.delta_minutes::bigint from timegenie.actual_time_adjustments adjustment
    join timegenie.tasks task on task.id=adjustment.task_id
    where adjustment.workspace_id=p_workspace_id and adjustment.work_date between p_period_start and p_period_end
      and adjustment.deleted_at is null and coalesce(task.day_instance_source_task_id,task.id)=any(coalesce(p_task_ids,'{}'::uuid[]))
  ) value;
  select string_agg(timegenie.direct_cloud_report_task_path(p_workspace_id,task.id),E'\n' order by task.sort_order,task.created_at,task.id) into tomorrow_plan
  from timegenie.tasks task where task.workspace_id=p_workspace_id and task.subject_id=p_subject_id
    and task.deleted_at is null and task.status='open' and task.planned_date=p_period_start+1
    and (task.day_instance_date is null or task.day_instance_source_task_id is null);
  select string_agg(report.markdown_content,E'\n\n' order by report.reference_date,report.id) into daily_collection
  from timegenie.reports report where report.workspace_id=p_workspace_id and report.subject_id=p_subject_id
    and report.report_type='daily' and report.reference_date between p_period_start and p_period_end
    and report.deleted_at is null;
  weekday_value:=case extract(isodow from p_period_start)::integer
    when 1 then '周一' when 2 then '周二' when 3 then '周三' when 4 then '周四'
    when 5 then '周五' when 6 then '周六' else '周日' end;
  statistics_value:='- 累计用时：'||timegenie.direct_cloud_report_duration(total_minutes_value,duration_format_value)||E'\n- 选中事项：'||cardinality(coalesce(p_task_ids,'{}'::uuid[]))::text||' 项';
  markdown_value:=template_row.content;
  markdown_value:=replace(markdown_value,'{{姓名}}',user_name_value);
  markdown_value:=replace(markdown_value,'{{日期}}',p_period_start::text);
  markdown_value:=replace(markdown_value,'{{星期}}',weekday_value);
  markdown_value:=replace(markdown_value,'{{日期范围}}',case when p_period_start=p_period_end then p_period_start::text else p_period_start::text||' 至 '||p_period_end::text end);
  markdown_value:=replace(markdown_value,'{{工作时段}}','');
  markdown_value:=replace(markdown_value,'{{总工时}}',timegenie.direct_cloud_report_duration(total_minutes_value,duration_format_value));
  markdown_value:=replace(markdown_value,'{{今日事项}}',coalesce(task_lines,'暂无事项'));
  markdown_value:=replace(markdown_value,'{{明日计划}}',coalesce(tomorrow_plan,'暂无计划'));
  markdown_value:=replace(markdown_value,'{{每日情况}}',coalesce(daily_collection,'暂无已保存日报'));
  markdown_value:=replace(markdown_value,'{{日报合集}}',coalesce(daily_collection,'暂无已保存日报'));
  markdown_value:=replace(markdown_value,'{{统计信息}}',statistics_value);
  markdown_value:=replace(markdown_value,'{{总结}}','- 在这里补充本期总结。');

  input_value:=jsonb_build_object(
    'reportType',p_report_type,'subjectId',p_subject_id,'periodStart',p_period_start,'periodEnd',p_period_end,
    'durationFormat',duration_format_value,
    'template',jsonb_build_object('id',template_row.id,'version',template_row.version,'deletedAt',template_row.deleted_at),
    'tasks',coalesce((select jsonb_agg(jsonb_build_object('id',task.id,'version',task.version,'title',task.title,'parentId',task.parent_id) order by item.ordinality)
      from unnest(coalesce(p_task_ids,'{}'::uuid[])) with ordinality item(task_id,ordinality) join timegenie.tasks task on task.id=item.task_id),'[]'::jsonb),
    'timeAllocations',coalesce((select jsonb_agg(jsonb_build_object('id',allocation.id,'version',allocation.version,'minutes',allocation.minutes,'entryVersion',entry.version,'entryDeletedAt',entry.deleted_at) order by allocation.id)
      from timegenie.time_allocations allocation join timegenie.time_entries entry on entry.id=allocation.entry_id join timegenie.tasks task on task.id=allocation.task_id
      where allocation.workspace_id=p_workspace_id and entry.work_date between p_period_start and p_period_end
        and coalesce(task.day_instance_source_task_id,task.id)=any(coalesce(p_task_ids,'{}'::uuid[]))),'[]'::jsonb),
    'adjustments',coalesce((select jsonb_agg(jsonb_build_object('id',adjustment.id,'version',adjustment.version,'deltaMinutes',adjustment.delta_minutes,'deletedAt',adjustment.deleted_at) order by adjustment.id)
      from timegenie.actual_time_adjustments adjustment join timegenie.tasks task on task.id=adjustment.task_id
      where adjustment.workspace_id=p_workspace_id and adjustment.work_date between p_period_start and p_period_end
        and coalesce(task.day_instance_source_task_id,task.id)=any(coalesce(p_task_ids,'{}'::uuid[]))),'[]'::jsonb),
    'dailyReports',coalesce((select jsonb_agg(jsonb_build_object('id',report.id,'version',report.version,'content',report.markdown_content) order by report.reference_date,report.id)
      from timegenie.reports report where report.workspace_id=p_workspace_id and report.subject_id=p_subject_id
        and report.report_type='daily' and report.reference_date between p_period_start and p_period_end and report.deleted_at is null),'[]'::jsonb)
  );
  input_hash:=encode(extensions.digest(input_value::text,'sha256'),'hex');
  return jsonb_build_object('templateId',template_row.id,'markdown',rtrim(markdown_value)||E'\n','inputRevisionHash',input_hash);
end $$;

create or replace function timegenie.direct_cloud_report_preview(
  p_workspace_id uuid,p_device_id uuid,p_report_type text,p_reference_date date,p_subject_id uuid,
  p_template_id uuid default null,p_expected_epoch bigint default null
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare period_value jsonb; period_start_value date; period_end_value date; task_ids uuid[]; rendered jsonb;
declare subject_name_value text; template_row timegenie.report_templates%rowtype; data_value jsonb;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  period_value:=timegenie.direct_cloud_report_period(p_report_type,p_reference_date);
  period_start_value:=(period_value->>'periodStart')::date;
  period_end_value:=(period_value->>'periodEnd')::date;
  select subject.name into subject_name_value from timegenie.subjects subject
    where subject.workspace_id=p_workspace_id and subject.id=p_subject_id and subject.deleted_at is null;
  if not found then raise exception 'NOT_FOUND: 主体不存在'; end if;
  task_ids:=timegenie.direct_cloud_report_scope_task_ids(
    p_workspace_id,p_subject_id,p_report_type,period_start_value,period_end_value,'[]'::jsonb
  );
  rendered:=timegenie.direct_cloud_report_render(
    p_workspace_id,p_report_type,p_subject_id,period_start_value,period_end_value,p_template_id,task_ids,false
  );
  select template.* into template_row from timegenie.report_templates template
    where template.workspace_id=p_workspace_id and template.id=(rendered->>'templateId')::uuid;
  data_value:=jsonb_build_object(
    'reportType',p_report_type,'referenceDate',p_reference_date,'periodStart',period_start_value,'periodEnd',period_end_value,
    'subjectId',p_subject_id,'subjectName',subject_name_value,
    'template',jsonb_build_object('id',template_row.id,'name',template_row.name,'version',template_row.version),
    'context',jsonb_build_object(
      'taskIds',to_jsonb(task_ids),
      'totalMinutes',coalesce((select sum(value.minutes) from (
        select allocation.minutes::bigint as minutes from timegenie.time_allocations allocation
        join timegenie.time_entries entry on entry.id=allocation.entry_id join timegenie.tasks task on task.id=allocation.task_id
        where allocation.workspace_id=p_workspace_id and entry.work_date between period_start_value and period_end_value
          and entry.deleted_at is null and coalesce(task.day_instance_source_task_id,task.id)=any(task_ids)
        union all
        select adjustment.delta_minutes::bigint from timegenie.actual_time_adjustments adjustment
        join timegenie.tasks task on task.id=adjustment.task_id
        where adjustment.workspace_id=p_workspace_id and adjustment.work_date between period_start_value and period_end_value
          and adjustment.deleted_at is null and coalesce(task.day_instance_source_task_id,task.id)=any(task_ids)
      ) value),0),
      'dailyCollection',coalesce((select string_agg(report.markdown_content,E'\n\n' order by report.reference_date,report.id)
        from timegenie.reports report where report.workspace_id=p_workspace_id and report.subject_id=p_subject_id
          and report.report_type='daily' and report.reference_date between period_start_value and period_end_value
          and report.deleted_at is null),'')
    ),
    'markdown',rendered->>'markdown'
  );
  return timegenie.direct_cloud_query_response(p_workspace_id,p_device_id,data_value,array['reports','tasks','time'],p_expected_epoch);
end $$;

revoke all on function timegenie.direct_cloud_report_duration(bigint,text) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_report_render(uuid,text,uuid,date,date,uuid,uuid[],boolean) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_report_preview(uuid,uuid,text,date,uuid,uuid,bigint) from public,anon;
grant execute on function timegenie.direct_cloud_report_preview(uuid,uuid,text,date,uuid,uuid,bigint) to authenticated;

commit;
