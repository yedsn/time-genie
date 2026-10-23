-- Direct-v2 report mutations. Report scope snapshots, generated content and
-- idempotency records are committed in one authoritative cloud transaction.

begin;

create or replace function timegenie.direct_cloud_report_period(
  p_report_type text,p_reference_date date
) returns jsonb language plpgsql immutable
set search_path=timegenie,extensions,pg_catalog as $$
declare period_start_value date; period_end_value date;
begin
  if p_report_type='daily' then
    period_start_value:=p_reference_date; period_end_value:=p_reference_date;
  elsif p_report_type='weekly' then
    period_start_value:=p_reference_date-(extract(isodow from p_reference_date)::integer-1);
    period_end_value:=period_start_value+6;
  elsif p_report_type='monthly' then
    period_start_value:=date_trunc('month',p_reference_date)::date;
    period_end_value:=(date_trunc('month',p_reference_date)+interval '1 month-1 day')::date;
  else
    raise exception 'VALIDATION_ERROR: 非法报告类型';
  end if;
  return jsonb_build_object('periodStart',period_start_value,'periodEnd',period_end_value);
end $$;

create or replace function timegenie.direct_cloud_report_task_path(
  p_workspace_id uuid,p_task_id uuid
) returns text language sql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
  with recursive ancestry as (
    select task.id,task.parent_id,task.title,0 as depth
    from timegenie.tasks task
    where task.workspace_id=p_workspace_id and task.id=p_task_id
    union all
    select parent.id,parent.parent_id,parent.title,child.depth+1
    from timegenie.tasks parent join ancestry child on child.parent_id=parent.id
    where parent.workspace_id=p_workspace_id and child.depth<64
  )
  select string_agg(title,' / ' order by depth desc) from ancestry;
$$;

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
  ), candidates as (
    select task.id,task.sort_order,task.created_at
    from timegenie.tasks task left join actuals on actuals.task_id=task.id
    where task.workspace_id=p_workspace_id and task.subject_id=p_subject_id and task.deleted_at is null
      and (task.day_instance_date is null or task.day_instance_source_task_id is null)
      and (coalesce(actuals.minutes,0)>0
        or (task.status='done' and task.completed_at>=timegenie.workspace_day_start(p_workspace_id,p_period_start)
          and task.completed_at<timegenie.workspace_day_end(p_workspace_id,p_period_end))
        or exists(select 1 from timegenie.task_occurrences occurrence
          where occurrence.workspace_id=p_workspace_id and occurrence.task_id=task.id
            and occurrence.occurrence_date between p_period_start and p_period_end and occurrence.status='done'))
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
declare task_lines text; tomorrow_plan text; daily_collection text; total_minutes_value bigint;
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
  with selected as (
    select item.task_id,item.ordinality,task.title,timegenie.direct_cloud_report_task_path(p_workspace_id,item.task_id) as path_label
    from unnest(coalesce(p_task_ids,'{}'::uuid[])) with ordinality item(task_id,ordinality)
    join timegenie.tasks task on task.workspace_id=p_workspace_id and task.id=item.task_id
  )
  select string_agg(ordinality::text||'. '||coalesce(path_label,title),E'\n' order by ordinality) into task_lines from selected;

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
  statistics_value:='- 累计用时：'||greatest(total_minutes_value,0)::text||'min'||E'\n- 选中事项：'||cardinality(coalesce(p_task_ids,'{}'::uuid[]))::text||' 项';
  markdown_value:=template_row.content;
  markdown_value:=replace(markdown_value,'{{姓名}}',user_name_value);
  markdown_value:=replace(markdown_value,'{{日期}}',p_period_start::text);
  markdown_value:=replace(markdown_value,'{{星期}}',weekday_value);
  markdown_value:=replace(markdown_value,'{{日期范围}}',case when p_period_start=p_period_end then p_period_start::text else p_period_start::text||' 至 '||p_period_end::text end);
  markdown_value:=replace(markdown_value,'{{工作时段}}','');
  markdown_value:=replace(markdown_value,'{{总工时}}',greatest(total_minutes_value,0)::text||'min');
  markdown_value:=replace(markdown_value,'{{今日事项}}',coalesce(task_lines,'暂无事项'));
  markdown_value:=replace(markdown_value,'{{明日计划}}',coalesce(tomorrow_plan,'暂无计划'));
  markdown_value:=replace(markdown_value,'{{每日情况}}',coalesce(daily_collection,'暂无已保存日报'));
  markdown_value:=replace(markdown_value,'{{日报合集}}',coalesce(daily_collection,'暂无已保存日报'));
  markdown_value:=replace(markdown_value,'{{统计信息}}',statistics_value);
  markdown_value:=replace(markdown_value,'{{总结}}','- 在这里补充本期总结。');

  input_value:=jsonb_build_object(
    'reportType',p_report_type,'subjectId',p_subject_id,'periodStart',p_period_start,'periodEnd',p_period_end,
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

create or replace function timegenie.direct_cloud_report_record(
  p_workspace_id uuid,p_report_id uuid,p_include_deleted boolean default false
) returns jsonb language sql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
  select jsonb_build_object(
    'id',report.id,'reportType',report.report_type,'subjectId',report.subject_id,'subjectName',subject.name,
    'periodStart',report.period_start,'periodEnd',report.period_end,'referenceDate',report.reference_date,
    'taskIds',coalesce((select jsonb_agg(item.task_id order by item.sort_order,item.task_id) from timegenie.report_tasks item where item.workspace_id=p_workspace_id and item.report_id=report.id),'[]'::jsonb),
    'templateId',report.template_id,'templateName',template.name,'templateArchived',coalesce(template.deleted_at is not null,false),
    'markdown',report.markdown_content,'contentSource',report.content_source,'generatedCount',report.generation_count,
    'inputRevisionHash',report.input_revision_hash,'generatedAt',report.generated_at,
    'updatedAt',(extract(epoch from report.updated_at)*1000)::bigint,'version',report.version,'deletedAt',report.deleted_at
  ) from timegenie.reports report join timegenie.subjects subject on subject.id=report.subject_id
  left join timegenie.report_templates template on template.id=report.template_id
  where report.workspace_id=p_workspace_id and report.id=p_report_id and (p_include_deleted or report.deleted_at is null);
$$;

create or replace function timegenie.direct_cloud_report_replace_tasks(
  p_workspace_id uuid,p_report_id uuid,p_subject_id uuid,p_task_ids uuid[]
) returns void language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
begin
  delete from timegenie.report_tasks item where item.workspace_id=p_workspace_id and item.report_id=p_report_id;
  insert into timegenie.report_tasks(report_id,task_id,workspace_id,sort_order,title_snapshot,path_snapshot)
  select p_report_id,task.id,p_workspace_id,item.ordinality::integer*10,task.title,
    timegenie.direct_cloud_report_task_path(p_workspace_id,task.id)
  from unnest(coalesce(p_task_ids,'{}'::uuid[])) with ordinality item(task_id,ordinality)
  join timegenie.tasks task on task.workspace_id=p_workspace_id and task.subject_id=p_subject_id
    and task.id=item.task_id and task.deleted_at is null
  order by item.ordinality;
end $$;

create or replace function timegenie.direct_cloud_report_create(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; after_value jsonb; period_value jsonb; rendered jsonb;
declare report_id uuid; report_type_value text; reference_date_value date; subject_id uuid; template_id uuid;
declare period_start_value date; period_end_value date; task_ids uuid[];
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_report_create',request_value);
  if cached is not null then return cached; end if;
  report_type_value:=coalesce(nullif(p_payload->>'reportType',''),'');
  reference_date_value:=nullif(p_payload->>'referenceDate','')::date;
  subject_id:=nullif(p_payload->>'subjectId','')::uuid; template_id:=nullif(p_payload->>'templateId','')::uuid;
  if reference_date_value is null or subject_id is null then raise exception 'VALIDATION_ERROR: 报告类型、日期和主体不能为空'; end if;
  period_value:=timegenie.direct_cloud_report_period(report_type_value,reference_date_value);
  period_start_value:=(period_value->>'periodStart')::date; period_end_value:=(period_value->>'periodEnd')::date;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':report:'||report_type_value||':'||subject_id::text||':'||period_start_value::text,0));
  if exists(select 1 from timegenie.reports report where report.workspace_id=p_workspace_id and report.report_type=report_type_value
    and report.subject_id=subject_id and report.period_start=period_start_value and report.period_end=period_end_value and report.deleted_at is null) then
    raise exception 'REPORT_ALREADY_EXISTS: 当前日期或周期已有报告';
  end if;
  task_ids:=timegenie.direct_cloud_report_scope_task_ids(p_workspace_id,subject_id,report_type_value,period_start_value,period_end_value,p_payload->'taskIds');
  rendered:=timegenie.direct_cloud_report_render(p_workspace_id,report_type_value,subject_id,period_start_value,period_end_value,template_id,task_ids,false);
  report_id:=coalesce(nullif(p_payload->>'reportId','')::uuid,timegenie.calendar_stable_uuid('direct-report:'||p_operation_id::text));
  if exists(select 1 from timegenie.reports report where report.workspace_id=p_workspace_id and report.id=report_id) then raise exception 'VERSION_CONFLICT: 报告标识已存在'; end if;
  insert into timegenie.reports(
    id,workspace_id,report_type,subject_id,period_start,period_end,reference_date,template_id,
    markdown_content,content_source,generation_count,input_revision_hash,generated_at,created_at,updated_at,
    version,created_by_device_id,updated_by_device_id
  ) values(
    report_id,p_workspace_id,report_type_value,subject_id,period_start_value,period_end_value,reference_date_value,
    (rendered->>'templateId')::uuid,rendered->>'markdown','generated',1,rendered->>'inputRevisionHash',now(),now(),now(),1,p_device_id,p_device_id
  );
  perform timegenie.direct_cloud_report_replace_tasks(p_workspace_id,report_id,subject_id,task_ids);
  after_value:=(select to_jsonb(report)||jsonb_build_object('report_tasks',coalesce((select jsonb_agg(to_jsonb(item) order by item.sort_order,item.task_id) from timegenie.report_tasks item where item.report_id=report_id),'[]'::jsonb)) from timegenie.reports report where report.id=report_id);
  result_value:=timegenie.direct_cloud_mutation_response(p_workspace_id,p_device_id,p_operation_id,
    jsonb_build_object('report',timegenie.direct_cloud_report_record(p_workspace_id,report_id,false)),array['reports'],p_expected_epoch);
  perform timegenie.record_direct_cloud_operation(p_workspace_id,p_operation_id,p_device_id,'direct_report_create',request_value,result_value);
  perform timegenie.operation_history_replace_item(p_workspace_id,p_operation_id,'report',report_id::text,null,after_value);
  return result_value;
exception when invalid_text_representation then raise exception 'VALIDATION_ERROR: 报告参数格式无效';
end $$;

create or replace function timegenie.direct_cloud_report_scope_update(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; before_value jsonb; after_value jsonb; period_value jsonb;
declare report_id uuid; report_type_value text; reference_date_value date; subject_id uuid; template_id uuid; expected_version bigint;
declare period_start_value date; period_end_value date; task_ids uuid[]; report_row timegenie.reports%rowtype;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_report_scope_update',request_value); if cached is not null then return cached; end if;
  report_id:=nullif(p_payload->>'reportId','')::uuid; expected_version:=nullif(p_payload->>'expectedVersion','')::bigint;
  report_type_value:=coalesce(nullif(p_payload->>'reportType',''),''); reference_date_value:=nullif(p_payload->>'referenceDate','')::date;
  subject_id:=nullif(p_payload->>'subjectId','')::uuid; template_id:=nullif(p_payload->>'templateId','')::uuid;
  if report_id is null or expected_version is null or reference_date_value is null or subject_id is null then raise exception 'VALIDATION_ERROR: 报告范围更新参数不完整'; end if;
  select report.* into report_row from timegenie.reports report where report.workspace_id=p_workspace_id and report.id=report_id for update;
  if not found then raise exception 'NOT_FOUND: 报告不存在'; end if;
  if report_row.deleted_at is not null then raise exception 'ENTITY_DELETED: 报告已删除'; end if;
  if report_row.version<>expected_version then raise exception 'VERSION_CONFLICT: 报告范围已变化'; end if;
  period_value:=timegenie.direct_cloud_report_period(report_type_value,reference_date_value);
  period_start_value:=(period_value->>'periodStart')::date; period_end_value:=(period_value->>'periodEnd')::date;
  if exists(select 1 from timegenie.reports report where report.workspace_id=p_workspace_id and report.id<>report_id
    and report.report_type=report_type_value and report.subject_id=subject_id and report.period_start=period_start_value
    and report.period_end=period_end_value and report.deleted_at is null) then raise exception 'REPORT_ALREADY_EXISTS: 当前日期或周期已有报告'; end if;
  if template_id is null then
    if report_row.report_type<>report_type_value then raise exception 'VALIDATION_ERROR: 切换报告类型时请选择对应类型的模板'; end if;
    template_id:=report_row.template_id;
  else
    perform 1 from timegenie.report_templates template where template.workspace_id=p_workspace_id and template.id=template_id
      and template.report_type=report_type_value and template.deleted_at is null;
    if not found then raise exception 'NOT_FOUND: 报告模板不存在'; end if;
  end if;
  perform 1 from timegenie.subjects subject where subject.workspace_id=p_workspace_id and subject.id=subject_id and subject.deleted_at is null;
  if not found then raise exception 'NOT_FOUND: 主体不存在'; end if;
  task_ids:=timegenie.direct_cloud_report_scope_task_ids(p_workspace_id,subject_id,report_type_value,period_start_value,period_end_value,p_payload->'taskIds');
  before_value:=to_jsonb(report_row)||jsonb_build_object('report_tasks',coalesce((select jsonb_agg(to_jsonb(item) order by item.sort_order,item.task_id) from timegenie.report_tasks item where item.report_id=report_id),'[]'::jsonb));
  update timegenie.reports report set report_type=report_type_value,subject_id=subject_id,period_start=period_start_value,
    period_end=period_end_value,reference_date=reference_date_value,template_id=template_id,updated_at=now(),
    updated_by_device_id=p_device_id,version=report.version+1 where report.id=report_id;
  perform timegenie.direct_cloud_report_replace_tasks(p_workspace_id,report_id,subject_id,task_ids);
  after_value:=(select to_jsonb(report)||jsonb_build_object('report_tasks',coalesce((select jsonb_agg(to_jsonb(item) order by item.sort_order,item.task_id) from timegenie.report_tasks item where item.report_id=report_id),'[]'::jsonb)) from timegenie.reports report where report.id=report_id);
  result_value:=timegenie.direct_cloud_mutation_response(p_workspace_id,p_device_id,p_operation_id,jsonb_build_object(
    'report',timegenie.direct_cloud_report_record(p_workspace_id,report_id,false),'needsRegeneration',true),array['reports'],p_expected_epoch);
  perform timegenie.record_direct_cloud_operation(p_workspace_id,p_operation_id,p_device_id,'direct_report_scope_update',request_value,result_value);
  perform timegenie.operation_history_replace_item(p_workspace_id,p_operation_id,'report',report_id::text,before_value,after_value);
  return result_value;
exception when invalid_text_representation then raise exception 'VALIDATION_ERROR: 报告参数格式无效';
end $$;

create or replace function timegenie.direct_cloud_report_content_save(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; before_value jsonb; after_value jsonb;
declare report_id uuid; expected_version bigint; markdown_value text; report_row timegenie.reports%rowtype;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_report_content_save',request_value); if cached is not null then return cached; end if;
  report_id:=nullif(p_payload->>'reportId','')::uuid; expected_version:=nullif(p_payload->>'expectedVersion','')::bigint; markdown_value:=coalesce(p_payload->>'markdown','');
  if report_id is null or expected_version is null or btrim(markdown_value)='' then raise exception 'VALIDATION_ERROR: 报告内容不能为空'; end if;
  select report.* into report_row from timegenie.reports report where report.workspace_id=p_workspace_id and report.id=report_id for update;
  if not found then raise exception 'NOT_FOUND: 报告不存在'; end if;
  if report_row.deleted_at is not null then raise exception 'ENTITY_DELETED: 报告已删除'; end if;
  if report_row.version<>expected_version then raise exception 'VERSION_CONFLICT: 报告内容已变化'; end if;
  before_value:=to_jsonb(report_row);
  update timegenie.reports report set markdown_content=markdown_value,content_source='edited',updated_at=now(),
    updated_by_device_id=p_device_id,version=report.version+1 where report.id=report_id;
  after_value:=(select to_jsonb(report) from timegenie.reports report where report.id=report_id);
  result_value:=timegenie.direct_cloud_mutation_response(p_workspace_id,p_device_id,p_operation_id,
    jsonb_build_object('report',timegenie.direct_cloud_report_record(p_workspace_id,report_id,false)),array['reports'],p_expected_epoch);
  perform timegenie.record_direct_cloud_operation(p_workspace_id,p_operation_id,p_device_id,'direct_report_content_save',request_value,result_value);
  perform timegenie.operation_history_replace_item(p_workspace_id,p_operation_id,'report',report_id::text,before_value,after_value);
  return result_value;
exception when invalid_text_representation then raise exception 'VALIDATION_ERROR: 报告参数格式无效';
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

create or replace function timegenie.direct_cloud_report_delete(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; before_value jsonb; after_value jsonb;
declare report_id uuid; expected_version bigint; report_row timegenie.reports%rowtype;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_report_delete',request_value); if cached is not null then return cached; end if;
  report_id:=nullif(p_payload->>'reportId','')::uuid; expected_version:=nullif(p_payload->>'expectedVersion','')::bigint;
  select report.* into report_row from timegenie.reports report where report.workspace_id=p_workspace_id and report.id=report_id for update;
  if not found then raise exception 'NOT_FOUND: 报告不存在'; end if;
  if report_row.deleted_at is not null then raise exception 'ENTITY_DELETED: 报告已删除'; end if;
  if report_row.version<>expected_version then raise exception 'VERSION_CONFLICT: 报告已变化或已删除'; end if;
  before_value:=to_jsonb(report_row);
  update timegenie.reports report set deleted_at=now(),updated_at=now(),updated_by_device_id=p_device_id,version=report.version+1 where report.id=report_id;
  after_value:=(select to_jsonb(report) from timegenie.reports report where report.id=report_id);
  result_value:=timegenie.direct_cloud_mutation_response(p_workspace_id,p_device_id,p_operation_id,
    jsonb_build_object('report',timegenie.direct_cloud_report_record(p_workspace_id,report_id,true)),array['reports'],p_expected_epoch);
  perform timegenie.record_direct_cloud_operation(p_workspace_id,p_operation_id,p_device_id,'direct_report_delete',request_value,result_value);
  perform timegenie.operation_history_replace_item(p_workspace_id,p_operation_id,'report',report_id::text,before_value,after_value);
  return result_value;
exception when invalid_text_representation then raise exception 'VALIDATION_ERROR: 报告参数格式无效';
end $$;

revoke all on function timegenie.direct_cloud_report_period(text,date) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_report_task_path(uuid,uuid) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_report_scope_task_ids(uuid,uuid,text,date,date,jsonb) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_report_render(uuid,text,uuid,date,date,uuid,uuid[],boolean) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_report_record(uuid,uuid,boolean) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_report_replace_tasks(uuid,uuid,uuid,uuid[]) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_report_create(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_report_scope_update(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_report_content_save(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_report_regenerate(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_report_delete(uuid,uuid,uuid,bigint,jsonb) from public,anon;
grant execute on function timegenie.direct_cloud_report_create(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_report_scope_update(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_report_content_save(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_report_regenerate(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_report_delete(uuid,uuid,uuid,bigint,jsonb) to authenticated;

commit;

notify pgrst,'reload schema';
