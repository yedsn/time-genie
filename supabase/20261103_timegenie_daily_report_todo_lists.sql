-- Daily reports mirror the Today instance tree and the in-progress source tree.
-- Queries remain read-only: no task materialization or report writes in preview.
begin;

-- Keep the current standalone-instance guard when upgrading older databases.
create or replace function timegenie.validate_task_day_instance_source() returns trigger
language plpgsql set search_path=timegenie,pg_catalog as $$
begin
  if new.day_instance_source_task_id is not null and not exists(
    select 1 from timegenie.tasks source where source.id=new.day_instance_source_task_id
      and source.workspace_id=new.workspace_id and source.subject_id=new.subject_id
      and source.day_instance_date is null
  ) then raise exception 'day instance source must be an ordinary task in the same workspace and subject'; end if;
  return new;
end $$;
revoke execute on function timegenie.validate_task_day_instance_source() from public,anon,authenticated;

create or replace function timegenie.direct_cloud_report_scope_task_ids(
  p_workspace_id uuid,p_subject_id uuid,p_report_type text,p_period_start date,p_period_end date,p_requested jsonb
) returns uuid[] language plpgsql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare result_value uuid[]; requested_count integer; matched_count integer;
begin
  if p_report_type='daily' then
    select coalesce(array_agg(task.id order by task.sort_order,task.created_at,task.id),'{}'::uuid[])
      into result_value from timegenie.tasks task
    where task.workspace_id=p_workspace_id and task.subject_id=p_subject_id
      and task.day_instance_date=p_period_start and task.day_instance_kind='member' and task.deleted_at is null;
    return result_value;
  end if;
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

  if p_report_type='daily' then
    p_task_ids:=timegenie.direct_cloud_report_scope_task_ids(p_workspace_id,p_subject_id,p_report_type,p_period_start,p_period_end,'[]'::jsonb);
  end if;

  with actuals as (
    select case when p_report_type='daily' then coalesce(
      (select instance.id from timegenie.tasks instance where instance.workspace_id=p_workspace_id
        and instance.subject_id=p_subject_id and instance.day_instance_source_task_id=task.id
        and instance.day_instance_date=p_period_start and instance.deleted_at is null),task.id)
      else coalesce(task.day_instance_source_task_id,task.id) end as task_id,sum(value.minutes)::bigint as minutes
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
    group by case when p_report_type='daily' then coalesce(
      (select instance.id from timegenie.tasks instance where instance.workspace_id=p_workspace_id
        and instance.subject_id=p_subject_id and instance.day_instance_source_task_id=task.id
        and instance.day_instance_date=p_period_start and instance.deleted_at is null),task.id)
      else coalesce(task.day_instance_source_task_id,task.id) end
  ), completed as (
    select task.id as task_id from timegenie.tasks task
    where task.workspace_id=p_workspace_id and task.subject_id=p_subject_id and task.deleted_at is null
      and (p_report_type='daily' or task.day_instance_date is null or task.day_instance_source_task_id is null)
      and task.status='done'
      and (p_report_type='daily' and task.day_instance_date=p_period_start or task.completed_at>=timegenie.workspace_day_start(p_workspace_id,p_period_start)
      and task.completed_at<timegenie.workspace_day_end(p_workspace_id,p_period_end))
    union
    select occurrence.task_id from timegenie.task_occurrences occurrence
    join timegenie.tasks task on task.id=occurrence.task_id and task.workspace_id=occurrence.workspace_id
    where occurrence.workspace_id=p_workspace_id and task.subject_id=p_subject_id and task.deleted_at is null
      and occurrence.occurrence_date between p_period_start and p_period_end and occurrence.status='done'
    union
    select case when p_report_type='daily' then instance.id else instance.day_instance_source_task_id end from timegenie.tasks instance
    where instance.workspace_id=p_workspace_id and instance.subject_id=p_subject_id and instance.deleted_at is null
      and instance.day_instance_source_task_id is not null and instance.status='done'
      and instance.day_instance_date between p_period_start and p_period_end
  ), selected as (
    select item.task_id,
      coalesce(actuals.minutes,0) as actual_minutes,
      exists(select 1 from completed where completed.task_id=item.task_id) as period_completed
    from unnest(coalesce(p_task_ids,'{}'::uuid[])) with ordinality item(task_id,ordinality)
    join timegenie.tasks task on task.workspace_id=p_workspace_id and task.id=item.task_id
    left join actuals on actuals.task_id=item.task_id
  )
  -- Selected items keep the task hierarchy: roots use an ordered list and
  -- children use indented dashes, the same shape as the plan section.
  select string_agg(
    case when line.depth=0 then line.current_root::text||'. ' else repeat('   ',line.depth)||'- ' end||
    case when line.is_leaf_selected then (case when line.period_completed then '🟢' else '🔴' end)||' ' else '' end||
    line.title||
    case when line.is_leaf_selected and line.actual_minutes>0
         then ' <实际：'||timegenie.direct_cloud_report_duration(line.actual_minutes,duration_format_value)||'>' else '' end,
    E'\n' order by line.order_path
  ) into task_lines
  from (
    with recursive included_tasks as (
      select item.task_id as id from unnest(coalesce(p_task_ids,'{}'::uuid[])) item(task_id)
      union
      select parent.id from included_tasks child
      join timegenie.tasks child_task on child_task.id=child.id
      join timegenie.tasks parent on parent.id=child_task.parent_id
      where parent.workspace_id=p_workspace_id and parent.deleted_at is null
    ), item_tree as (
      select task.id,task.title,task.parent_id,0 as depth,
             array[lpad(task.sort_order::text,20,'0')||':'||to_char(task.created_at,'YYYYMMDDHH24MISSUS')||':'||task.id::text] as order_path
      from timegenie.tasks task join included_tasks included on included.id=task.id
      where task.parent_id is null
         or not exists(select 1 from included_tasks parent where parent.id=task.parent_id)
      union all
      select child.id,child.title,child.parent_id,parent.depth+1,
             parent.order_path||(lpad(child.sort_order::text,20,'0')||':'||to_char(child.created_at,'YYYYMMDDHH24MISSUS')||':'||child.id::text)
      from item_tree parent
      join timegenie.tasks child on child.parent_id=parent.id
      join included_tasks included on included.id=child.id
      where child.workspace_id=p_workspace_id and child.deleted_at is null
    ), item_roots as (
      select order_path,row_number() over (order by order_path) as root_index
      from item_tree where depth=0
    ), item_lines as (
      select tree.*,
             (select roots.root_index from item_roots roots
              where tree.order_path[1:array_length(roots.order_path,1)]=roots.order_path) as current_root
      from item_tree tree
    )
    select line.title,line.depth,line.order_path,line.current_root,
           coalesce(selected.actual_minutes,0) as actual_minutes,
           coalesce(selected.period_completed,false) as period_completed,
           selected.task_id is not null
             and not exists(select 1 from item_tree child where child.parent_id=line.id) as is_leaf_selected
    from item_lines line left join selected on selected.task_id=line.id
  ) line;

  select coalesce(sum(value.minutes),0)::bigint into total_minutes_value from (
    select allocation.minutes::bigint as minutes
    from timegenie.time_allocations allocation join timegenie.time_entries entry on entry.id=allocation.entry_id
    join timegenie.tasks task on task.id=allocation.task_id
    where allocation.workspace_id=p_workspace_id and entry.work_date between p_period_start and p_period_end
      and entry.deleted_at is null and case when p_report_type='daily' then coalesce(
      (select instance.id from timegenie.tasks instance where instance.workspace_id=p_workspace_id
        and instance.subject_id=p_subject_id and instance.day_instance_source_task_id=task.id
        and instance.day_instance_date=p_period_start and instance.deleted_at is null),task.id)
      else coalesce(task.day_instance_source_task_id,task.id) end=any(coalesce(p_task_ids,'{}'::uuid[]))
    union all
    select adjustment.delta_minutes::bigint from timegenie.actual_time_adjustments adjustment
    join timegenie.tasks task on task.id=adjustment.task_id
    where adjustment.workspace_id=p_workspace_id and adjustment.work_date between p_period_start and p_period_end
      and adjustment.deleted_at is null and case when p_report_type='daily' then coalesce(
      (select instance.id from timegenie.tasks instance where instance.workspace_id=p_workspace_id
        and instance.subject_id=p_subject_id and instance.day_instance_source_task_id=task.id
        and instance.day_instance_date=p_period_start and instance.deleted_at is null),task.id)
      else coalesce(task.day_instance_source_task_id,task.id) end=any(coalesce(p_task_ids,'{}'::uuid[]))
  ) value;
  -- The plan section starts from every in-progress task, not only items that
  -- were explicitly planned for the next work day. Roots stay an ordered list
  -- and children use indented dashes, matching the report item hierarchy.
  select string_agg(
    case when line.depth=0 then line.root_index::text||'. '||line.title
         else repeat('   ',line.depth)||'- '||line.title end,
    E'\n' order by line.order_path
  ) into tomorrow_plan
  from (
    with recursive open_tasks as (
      select task.id,task.parent_id,task.title,task.sort_order,task.created_at
      from timegenie.tasks task
      where task.workspace_id=p_workspace_id and task.subject_id=p_subject_id
        and task.deleted_at is null
        and task.day_instance_date is null and task.status='open'
    ), included_tasks as (
      with recursive ancestors as (
        select task.id,task.parent_id from open_tasks task
        union
        select parent.id,parent.parent_id from timegenie.tasks parent
        join ancestors child on child.parent_id=parent.id
        where parent.workspace_id=p_workspace_id and parent.subject_id=p_subject_id
          and parent.deleted_at is null and parent.status='open' and parent.day_instance_date is null
      ) select id from ancestors
    ), plan_tree as (
      select task.id,task.title,0 as depth,
             array[lpad(task.sort_order::text,20,'0')||':'||to_char(task.created_at,'YYYYMMDDHH24MISSUS')||':'||task.id::text] as order_path
      from timegenie.tasks task join included_tasks included on included.id=task.id
      where task.parent_id is null
         or not exists(select 1 from included_tasks parent where parent.id=task.parent_id)
      union all
      select child.id,child.title,parent.depth+1,
             parent.order_path||(lpad(child.sort_order::text,20,'0')||':'||to_char(child.created_at,'YYYYMMDDHH24MISSUS')||':'||child.id::text)
      from plan_tree parent
      join timegenie.tasks child on child.parent_id=parent.id
      join included_tasks included on included.id=child.id
      where child.workspace_id=p_workspace_id and child.subject_id=p_subject_id and child.deleted_at is null
    ), plan_roots as (
      select id,row_number() over (order by order_path) as root_index
      from plan_tree where depth=0
    )
    select tree.title,tree.depth,tree.order_path,roots.root_index
    from plan_tree tree left join plan_roots roots on roots.id=tree.id
  ) line;
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
        and case when p_report_type='daily' then coalesce(
      (select instance.id from timegenie.tasks instance where instance.workspace_id=p_workspace_id
        and instance.subject_id=p_subject_id and instance.day_instance_source_task_id=task.id
        and instance.day_instance_date=p_period_start and instance.deleted_at is null),task.id)
      else coalesce(task.day_instance_source_task_id,task.id) end=any(coalesce(p_task_ids,'{}'::uuid[]))),'[]'::jsonb),
    'adjustments',coalesce((select jsonb_agg(jsonb_build_object('id',adjustment.id,'version',adjustment.version,'deltaMinutes',adjustment.delta_minutes,'deletedAt',adjustment.deleted_at) order by adjustment.id)
      from timegenie.actual_time_adjustments adjustment join timegenie.tasks task on task.id=adjustment.task_id
      where adjustment.workspace_id=p_workspace_id and adjustment.work_date between p_period_start and p_period_end
        and case when p_report_type='daily' then coalesce(
      (select instance.id from timegenie.tasks instance where instance.workspace_id=p_workspace_id
        and instance.subject_id=p_subject_id and instance.day_instance_source_task_id=task.id
        and instance.day_instance_date=p_period_start and instance.deleted_at is null),task.id)
      else coalesce(task.day_instance_source_task_id,task.id) end=any(coalesce(p_task_ids,'{}'::uuid[]))),'[]'::jsonb),
    'dailyReports',coalesce((select jsonb_agg(jsonb_build_object('id',report.id,'version',report.version,'content',report.markdown_content) order by report.reference_date,report.id)
      from timegenie.reports report where report.workspace_id=p_workspace_id and report.subject_id=p_subject_id
        and report.report_type='daily' and report.reference_date between p_period_start and p_period_end and report.deleted_at is null),'[]'::jsonb)
  );
  input_hash:=encode(extensions.digest(input_value::text,'sha256'),'hex');
  return jsonb_build_object('templateId',template_row.id,'markdown',rtrim(markdown_value)||E'\n','inputRevisionHash',input_hash,'totalMinutes',total_minutes_value);
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
      'totalMinutes',(rendered->>'totalMinutes')::bigint,
      'dailyCollection',coalesce((select string_agg(report.markdown_content,E'\n\n' order by report.reference_date,report.id)
        from timegenie.reports report where report.workspace_id=p_workspace_id and report.subject_id=p_subject_id
          and report.report_type='daily' and report.reference_date between period_start_value and period_end_value
          and report.deleted_at is null),'')
    ),
    'markdown',rendered->>'markdown'
  );
  return timegenie.direct_cloud_query_response(p_workspace_id,p_device_id,data_value,array['reports','tasks','time'],p_expected_epoch);
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

  if p_report_type='daily' then
    with recursive members as (
      select task.id from timegenie.tasks task
      where task.workspace_id=p_workspace_id and task.subject_id=p_subject_id
        and task.day_instance_date=period_start_value and task.day_instance_kind='member' and task.deleted_at is null
    ), included as (
      select id from members
      union
      select parent.id from included child join timegenie.tasks task on task.id=child.id
      join timegenie.tasks parent on parent.id=task.parent_id
      where parent.workspace_id=p_workspace_id and parent.subject_id=p_subject_id
        and parent.day_instance_date=period_start_value and parent.deleted_at is null
    ), actuals as (
      select coalesce(instance.id,value.task_id) as task_id,sum(value.minutes)::bigint as minutes
      from (
        select allocation.task_id,allocation.minutes from timegenie.time_allocations allocation
        join timegenie.time_entries entry on entry.id=allocation.entry_id
        where allocation.workspace_id=p_workspace_id and entry.work_date=period_start_value and entry.deleted_at is null
        union all
        select adjustment.task_id,adjustment.delta_minutes from timegenie.actual_time_adjustments adjustment
        where adjustment.workspace_id=p_workspace_id and adjustment.work_date=period_start_value and adjustment.deleted_at is null
      ) value join timegenie.tasks task on task.id=value.task_id
      left join timegenie.tasks instance on instance.workspace_id=p_workspace_id and instance.subject_id=p_subject_id
        and instance.day_instance_source_task_id=task.id and instance.day_instance_date=period_start_value and instance.deleted_at is null
      where task.workspace_id=p_workspace_id and task.subject_id=p_subject_id
      group by coalesce(instance.id,value.task_id)
    ), tree as (
      select task.*,array[lpad(task.sort_order::text,20,'0')||':'||task.created_at::text||':'||task.id::text] as order_path
      from included join timegenie.tasks task on task.id=included.id
      where task.parent_id is null or not exists(select 1 from included parent where parent.id=task.parent_id)
      union all
      select child.*,parent.order_path||(lpad(child.sort_order::text,20,'0')||':'||child.created_at::text||':'||child.id::text)
      from tree parent join timegenie.tasks child on child.parent_id=parent.id join included on included.id=child.id
    )
    select jsonb_build_object('tasks',coalesce(jsonb_agg(jsonb_build_object(
      'taskId',task.id,'parentId',task.parent_id,'title',task.title,'status',task.status,
      'estimateMinutes',task.estimate_minutes,'actualMinutes',coalesce(actuals.minutes,0),
      'dailyEstimateMinutes',estimate.estimate_minutes,'sortOrder',task.sort_order,
      'periodStatus',case when task.day_instance_kind='context' then 'context' when task.status='done' then 'completed' when coalesce(actuals.minutes,0)>0 then 'active' else 'planned' end,
      'defaultSelected',task.day_instance_kind='member','candidateRole',case when task.day_instance_kind='member' then 'selectable' else 'context' end
    ) order by task.order_path),'[]'::jsonb)) into data_value
    from tree task left join actuals on actuals.task_id=task.id
    left join timegenie.task_daily_estimates estimate on estimate.workspace_id=p_workspace_id and estimate.task_id=task.id and estimate.work_date=period_start_value;
    return timegenie.direct_cloud_query_response(p_workspace_id,p_device_id,data_value,array['reports','tasks','time'],p_expected_epoch);
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

create or replace function timegenie.direct_cloud_report_regenerate(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; before_value jsonb; after_value jsonb; rendered jsonb;
declare report_id uuid; expected_version bigint; report_row timegenie.reports%rowtype; task_ids uuid[];
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_report_regenerate',request_value); if cached is not null then return cached; end if;
  report_id:=nullif(p_payload->>'reportId','')::uuid; expected_version:=nullif(p_payload->>'expectedVersion','')::bigint;
  select report.* into report_row from timegenie.reports report where report.workspace_id=p_workspace_id and report.id=report_id for update;
  if not found then raise exception 'NOT_FOUND: 报告不存在'; end if;
  if report_row.deleted_at is not null then raise exception 'ENTITY_DELETED: 报告已删除'; end if;
  if report_row.version<>expected_version then raise exception 'VERSION_CONFLICT: 报告已变化'; end if;
  select coalesce(array_agg(item.task_id order by item.sort_order,item.task_id),'{}'::uuid[]) into task_ids
  from timegenie.report_tasks item where item.workspace_id=p_workspace_id and item.report_id=report_id;
  if report_row.report_type='daily' then
    task_ids:=timegenie.direct_cloud_report_scope_task_ids(p_workspace_id,report_row.subject_id,'daily',report_row.period_start,report_row.period_end,'[]'::jsonb);
    perform timegenie.direct_cloud_report_replace_tasks(p_workspace_id,report_id,report_row.subject_id,task_ids);
  end if;
  rendered:=timegenie.direct_cloud_report_render(p_workspace_id,report_row.report_type,report_row.subject_id,report_row.period_start,report_row.period_end,report_row.template_id,task_ids,true);
  before_value:=to_jsonb(report_row);
  update timegenie.reports report set template_id=(rendered->>'templateId')::uuid,markdown_content=rendered->>'markdown',
    content_source='generated',generation_count=report.generation_count+1,input_revision_hash=rendered->>'inputRevisionHash',
    generated_at=now(),updated_at=now(),updated_by_device_id=p_device_id,version=report.version+1 where report.id=report_id;
  after_value:=(select to_jsonb(report) from timegenie.reports report where report.id=report_id);
  result_value:=timegenie.direct_cloud_mutation_response(p_workspace_id,p_device_id,p_operation_id,
    jsonb_build_object('report',timegenie.direct_cloud_report_record(p_workspace_id,report_id,false)),array['reports'],p_expected_epoch);
  perform timegenie.record_direct_cloud_operation(p_workspace_id,p_operation_id,p_device_id,'direct_report_regenerate',request_value,result_value);
  perform timegenie.operation_history_replace_item(p_workspace_id,p_operation_id,'report',report_id::text,before_value,after_value);
  return result_value;
exception when invalid_text_representation then raise exception 'VALIDATION_ERROR: 报告参数格式无效';
end $$;


revoke all on function timegenie.direct_cloud_report_scope_task_ids(uuid,uuid,text,date,date,jsonb) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_report_render(uuid,text,uuid,date,date,uuid,uuid[],boolean) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_report_preview(uuid,uuid,text,date,uuid,uuid,bigint) from public,anon;
revoke all on function timegenie.direct_cloud_report_task_suggestions(uuid,uuid,text,date,uuid,uuid[],bigint) from public,anon;
revoke all on function timegenie.direct_cloud_report_regenerate(uuid,uuid,uuid,bigint,jsonb) from public,anon;
grant execute on function timegenie.direct_cloud_report_preview(uuid,uuid,text,date,uuid,uuid,bigint) to authenticated;
grant execute on function timegenie.direct_cloud_report_task_suggestions(uuid,uuid,text,date,uuid,uuid[],bigint) to authenticated;
grant execute on function timegenie.direct_cloud_report_regenerate(uuid,uuid,uuid,bigint,jsonb) to authenticated;
notify pgrst, 'reload schema';
commit;
