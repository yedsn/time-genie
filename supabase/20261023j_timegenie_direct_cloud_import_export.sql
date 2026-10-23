-- Direct-v2 daily-report import and time-allocation export. Local files stay
-- device-local, while duplicate detection, mutations and exported rows are
-- derived exclusively from authoritative Supabase business data.

begin;

create or replace function timegenie.direct_cloud_daily_report_source_ref(
  p_source_path_hash text,p_source_date date,p_target_kind text,p_source_fingerprint text
) returns text language sql immutable set search_path=pg_catalog as $$
  select 'daily-report:'||p_source_path_hash||':'||p_source_date::text||':'||p_target_kind||':'||p_source_fingerprint
$$;

create or replace function timegenie.direct_cloud_daily_report_imported_fingerprints(
  p_workspace_id uuid,p_device_id uuid,p_subject_id uuid,p_source_date date,p_target_kind text,
  p_source_path_hash text,p_fingerprints text[],p_expected_epoch bigint default null
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare data_value jsonb;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  if p_target_kind not in('backlog','day_instance') then raise exception 'VALIDATION_ERROR: 导入目标无效'; end if;
  if not exists(select 1 from timegenie.subjects where workspace_id=p_workspace_id and id=p_subject_id and deleted_at is null) then
    raise exception 'NOT_FOUND: 主体不存在';
  end if;
  select jsonb_build_object('fingerprints',coalesce(jsonb_agg(source.fingerprint order by source.ordinality),'[]'::jsonb))
  into data_value
  from unnest(coalesce(p_fingerprints,'{}'::text[])) with ordinality source(fingerprint,ordinality)
  where exists(
    select 1 from timegenie.tasks source_task
    where source_task.workspace_id=p_workspace_id and source_task.subject_id=p_subject_id
      and source_task.source_ref=timegenie.direct_cloud_daily_report_source_ref(
        p_source_path_hash,p_source_date,p_target_kind,source.fingerprint)
      and (
        (p_target_kind='backlog' and source_task.deleted_at is null)
        or (p_target_kind='day_instance' and exists(
          select 1 from timegenie.tasks instance
          where instance.workspace_id=p_workspace_id
            and instance.day_instance_source_task_id=source_task.id
            and instance.day_instance_date=p_source_date and instance.deleted_at is null
        ))
      )
  );
  return timegenie.direct_cloud_query_response(
    p_workspace_id,p_device_id,coalesce(data_value,jsonb_build_object('fingerprints','[]'::jsonb)),
    array['tasks','reports'],p_expected_epoch);
end $$;

create or replace function timegenie.direct_cloud_daily_report_import_confirm(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; subject_id uuid; source_date date;
declare target_kind text; source_path_hash text; batch_id uuid; item_value jsonb; item_id uuid;
declare parent_item_id uuid; parent_task_id uuid; source_task_id uuid; effective_task_id uuid;
declare source_fingerprint text; source_ref_value text;
declare title_value text; status_value text; estimate_value integer; target_minutes integer; baseline_minutes integer;
declare delta_minutes integer; is_selected boolean; is_context boolean; created_ids uuid[]:='{}';
declare created_source_ids uuid[]:='{}';
declare created_count integer:=0; skipped_count integer:=0; adjustment_count integer:=0; task_list jsonb;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_daily_report_import_confirm',request_value);
  if cached is not null then return cached; end if;
  subject_id:=nullif(p_payload->>'subjectId','')::uuid;
  source_date:=nullif(p_payload->>'sourceDate','')::date;
  target_kind:=nullif(p_payload->>'targetKind','');
  source_path_hash:=nullif(p_payload->>'sourcePathHash','');
  batch_id:=nullif(p_payload->>'batchId','')::uuid;
  if subject_id is null or source_date is null or source_path_hash is null or batch_id is null then
    raise exception 'VALIDATION_ERROR: 日报导入参数不完整';
  end if;
  if target_kind not in('backlog','day_instance') then raise exception 'VALIDATION_ERROR: 导入目标无效'; end if;
  if jsonb_typeof(coalesce(p_payload->'items','null'::jsonb))<>'array' or jsonb_array_length(p_payload->'items')=0 then
    raise exception 'VALIDATION_ERROR: 导入事项不能为空';
  end if;
  perform 1 from timegenie.subjects where workspace_id=p_workspace_id and id=subject_id and deleted_at is null for update;
  if not found then raise exception 'NOT_FOUND: 主体不存在或已归档'; end if;

  for item_value in select value from jsonb_array_elements(p_payload->'items') loop
    item_id:=nullif(item_value->>'id','')::uuid;
    title_value:=btrim(coalesce(item_value->>'title',''));
    source_fingerprint:=nullif(item_value->>'sourceFingerprint','');
    if item_id is null or title_value='' or source_fingerprint is null then
      raise exception 'VALIDATION_ERROR: 导入事项标识、名称和来源指纹不能为空';
    end if;
    if item_value ? 'estimateMinutes' and item_value->'estimateMinutes'<>'null'::jsonb
       and (item_value->>'estimateMinutes')::integer<=0 then
      raise exception 'VALIDATION_ERROR: 预计用时必须大于 0 分钟';
    end if;
  end loop;

  -- Parents are inserted before children because the preview preserves source order.
  for item_value in select value from jsonb_array_elements(p_payload->'items') loop
    item_id:=nullif(item_value->>'id','')::uuid;
    parent_item_id:=nullif(item_value->>'parentId','')::uuid;
    source_fingerprint:=nullif(item_value->>'sourceFingerprint','');
    source_ref_value:=timegenie.direct_cloud_daily_report_source_ref(
      source_path_hash,source_date,target_kind,source_fingerprint);
    select task.id into source_task_id from timegenie.tasks task
      where task.workspace_id=p_workspace_id and task.subject_id=subject_id
        and task.source_ref=source_ref_value limit 1;
    if found then skipped_count:=skipped_count+1; continue; end if;
    if exists(select 1 from timegenie.tasks where workspace_id=p_workspace_id and id=item_id) then
      raise exception 'VERSION_CONFLICT: 事项标识已存在';
    end if;
    parent_task_id:=null;
    if parent_item_id is not null and target_kind='backlog' then
      select task.id into parent_task_id
      from jsonb_array_elements(p_payload->'items') parent_item
      join timegenie.tasks task on task.workspace_id=p_workspace_id and task.subject_id=subject_id
        and task.source_ref=timegenie.direct_cloud_daily_report_source_ref(
          source_path_hash,source_date,target_kind,parent_item.value->>'sourceFingerprint')
        and task.deleted_at is null
      where nullif(parent_item.value->>'id','')::uuid=parent_item_id limit 1;
      if parent_task_id is null then raise exception 'VALIDATION_ERROR: 导入父事项无效'; end if;
    end if;
    title_value:=btrim(item_value->>'title');
    status_value:=coalesce(nullif(item_value->>'status',''),'open');
    is_selected:=coalesce((item_value->>'selected')::boolean,false);
    is_context:=target_kind='day_instance' and not is_selected;
    estimate_value:=case when is_selected and item_value ? 'estimateMinutes' and item_value->'estimateMinutes'<>'null'::jsonb
      then (item_value->>'estimateMinutes')::integer else null end;
    insert into timegenie.tasks(
      id,workspace_id,subject_id,parent_id,title,status,planned_date,estimate_minutes,source_type,source_ref,
      sort_order,completed_at,created_by_device_id,updated_by_device_id
    ) values(
      item_id,p_workspace_id,subject_id,case when target_kind='day_instance' then parent_item_id else parent_task_id end,title_value,
      case when target_kind='backlog' and is_selected and status_value='done' then 'done' else 'open' end,
      null,case when target_kind='backlog' then estimate_value else null end,'obsidian_import',source_ref_value,
      coalesce((item_value->>'sortOrder')::integer,0),
      case when target_kind='backlog' and is_selected and status_value='done' then timegenie.workspace_day_start(p_workspace_id,source_date)+interval '12 hours' else null end,
      p_device_id,p_device_id
    );
    source_task_id:=item_id; effective_task_id:=item_id;
    if target_kind='day_instance' then
      effective_task_id:=timegenie.direct_cloud_today_instance_ensure(
        p_workspace_id,p_device_id,source_task_id,source_date,case when is_context then 'context' else 'member' end);
      created_source_ids:=array_append(created_source_ids,source_task_id);
      if is_selected and status_value='done' and not is_context then
        update timegenie.tasks set status='done',
          completed_at=timegenie.workspace_day_start(p_workspace_id,source_date)+interval '12 hours',
          updated_at=now(),updated_by_device_id=p_device_id,version=version+1
        where id=effective_task_id;
      end if;
    end if;
    if target_kind='day_instance' and estimate_value is not null and not is_context then
      insert into timegenie.task_daily_estimates(workspace_id,task_id,work_date,estimate_minutes)
      values(p_workspace_id,effective_task_id,source_date,estimate_value);
    end if;
    if is_selected and status_value='done' and not is_context then
      insert into timegenie.task_status_events(id,workspace_id,task_id,status,occurred_at,source_type)
      values(gen_random_uuid(),p_workspace_id,effective_task_id,'done',timegenie.workspace_day_start(p_workspace_id,source_date)+interval '12 hours','import');
    end if;
    if is_selected and status_value='done' and item_value ? 'importActualMinutes' and item_value->'importActualMinutes'<>'null'::jsonb then
      target_minutes:=(item_value->>'importActualMinutes')::integer;
      baseline_minutes:=timegenie.direct_cloud_actual_minutes(p_workspace_id,effective_task_id,source_date);
      delta_minutes:=target_minutes-baseline_minutes;
      if delta_minutes<>0 then
        insert into timegenie.actual_time_adjustments(
          id,workspace_id,subject_id,task_id,work_date,delta_minutes,baseline_minutes,target_minutes,note,
          source_type,batch_id,created_by_device_id,updated_by_device_id
        ) values(
          gen_random_uuid(),p_workspace_id,subject_id,effective_task_id,source_date,delta_minutes,baseline_minutes,target_minutes,
          '从 Obsidian 日报导入','daily_report_import',batch_id,p_device_id,p_device_id
        );
        adjustment_count:=adjustment_count+1;
      end if;
    end if;
    created_ids:=array_append(created_ids,effective_task_id); created_count:=created_count+1;
  end loop;
  if target_kind='day_instance' and cardinality(created_source_ids)>0 then
    update timegenie.tasks set deleted_at=now(),updated_at=now(),updated_by_device_id=p_device_id,version=version+1
    where workspace_id=p_workspace_id and id=any(created_source_ids);
  end if;
  select coalesce(jsonb_agg(timegenie.direct_cloud_task_project(p_workspace_id,created.task_id) order by created.position),'[]'::jsonb)
    into task_list from unnest(created_ids) with ordinality created(task_id,position);
  result_value:=timegenie.direct_cloud_mutation_response(
    p_workspace_id,p_device_id,p_operation_id,jsonb_build_object(
      'tasks',task_list,'createdCount',created_count,'skippedCount',skipped_count,'adjustmentCount',adjustment_count),
    array['tasks','planning','time','reports'],p_expected_epoch);
  perform timegenie.record_direct_cloud_operation(
    p_workspace_id,p_operation_id,p_device_id,'direct_daily_report_import_confirm',request_value,result_value);
  return result_value;
exception when invalid_text_representation or numeric_value_out_of_range then
  raise exception 'VALIDATION_ERROR: 日报导入参数格式无效';
end $$;

create or replace function timegenie.direct_cloud_time_allocation_export(
  p_workspace_id uuid,p_device_id uuid,p_start_date date,p_end_date date,
  p_include_special_time boolean default false,p_expected_epoch bigint default null
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare data_value jsonb; server_time_value timestamptz:=now(); timezone_value text;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  if p_start_date is null or p_end_date is null or p_start_date>p_end_date then
    raise exception 'VALIDATION_ERROR: 导出日期范围无效';
  end if;
  select workspace.timezone into timezone_value from timegenie.workspaces workspace where workspace.id=p_workspace_id;
  with entry_rows as (
    select entry.*,
      case when entry.state='running' then greatest(entry.duration_seconds+coalesce((select sum(greatest(extract(epoch from(server_time_value-segment.started_at))::bigint,0)) from timegenie.time_segments segment where segment.entry_id=entry.id and segment.ended_at is null),0),0) else entry.duration_seconds end as effective_seconds,
      case when entry.state='running' then server_time_value
        when entry.state='paused' then coalesce((select max(segment.ended_at) from timegenie.time_segments segment where segment.entry_id=entry.id and segment.ended_at is not null),entry.started_at+make_interval(secs=>entry.duration_seconds::double precision))
        else coalesce(entry.ended_at,entry.started_at+make_interval(secs=>entry.duration_seconds::double precision)) end as display_end
    from timegenie.time_entries entry
    where entry.workspace_id=p_workspace_id and entry.work_date between p_start_date and p_end_date and entry.deleted_at is null
  ), rows as (
    select entry.work_date,coalesce(subject.name,'已删除主体') as subject,
      case when task.deleted_at is not null then '已删除事项' when parent.id is null then task.title else parent.title||' / '||task.title end as task,
      allocation.minutes,
      to_char(entry.started_at at time zone timezone_value,'HH24:MI')||'-'||to_char(entry.display_end at time zone timezone_value,'HH24:MI') as time_range,
      concat_ws('；',case when nullif(btrim(entry.note),'') is not null then '计时：'||btrim(entry.note) end,case when nullif(btrim(allocation.note),'') is not null then '归属：'||btrim(allocation.note) end) as note,
      entry.started_at as order_time,allocation.created_at as item_time,allocation.id as item_id
    from entry_rows entry join timegenie.time_allocations allocation on allocation.entry_id=entry.id and allocation.minutes>0
    left join timegenie.tasks task on task.id=allocation.task_id left join timegenie.tasks parent on parent.id=task.parent_id
    left join timegenie.subjects subject on subject.id=task.subject_id
    where entry.kind<>'break' and entry.state='ended'
    union all
    select entry.work_date,'特殊时间','休息',case when entry.effective_seconds<=0 then 0 else (entry.effective_seconds+59)/60 end,
      to_char(entry.started_at at time zone timezone_value,'HH24:MI')||'-'||to_char(entry.display_end at time zone timezone_value,'HH24:MI'),
      concat_ws('；',case when nullif(btrim(entry.note),'') is not null then '计时：'||btrim(entry.note) end,'休息',case entry.state when 'running' then '进行中' when 'paused' then '已暂停' end),
      entry.started_at,entry.created_at,entry.id
    from entry_rows entry where p_include_special_time and entry.kind='break' and entry.effective_seconds>0
    union all
    select entry.work_date,'特殊时间','未归属',
      greatest((case when entry.effective_seconds<=0 then 0 else (entry.effective_seconds+59)/60 end)-case when entry.state='ended' then coalesce((select sum(allocation.minutes) from timegenie.time_allocations allocation where allocation.entry_id=entry.id and allocation.minutes>0),0) else 0 end,0),
      to_char(entry.started_at at time zone timezone_value,'HH24:MI')||'-'||to_char(entry.display_end at time zone timezone_value,'HH24:MI'),
      concat_ws('；',case when nullif(btrim(entry.note),'') is not null then '计时：'||btrim(entry.note) end,'未归属',case entry.state when 'running' then '进行中' when 'paused' then '已暂停' end),
      entry.started_at,entry.created_at,entry.id
    from entry_rows entry where p_include_special_time and entry.kind<>'break'
      and greatest((case when entry.effective_seconds<=0 then 0 else (entry.effective_seconds+59)/60 end)-case when entry.state='ended' then coalesce((select sum(allocation.minutes) from timegenie.time_allocations allocation where allocation.entry_id=entry.id and allocation.minutes>0),0) else 0 end,0)>0
    union all
    select adjustment.work_date,coalesce(subject.name,'已删除主体'),
      case when task.deleted_at is not null then '已删除事项' when parent.id is null then task.title else parent.title||' / '||task.title end,
      adjustment.delta_minutes,'',case when nullif(btrim(adjustment.note),'') is null then '实际调整' else '实际调整：'||btrim(adjustment.note) end,
      adjustment.created_at,adjustment.created_at,adjustment.id
    from timegenie.actual_time_adjustments adjustment
    left join timegenie.tasks task on task.id=adjustment.task_id left join timegenie.tasks parent on parent.id=task.parent_id
    left join timegenie.subjects subject on subject.id=task.subject_id
    where adjustment.workspace_id=p_workspace_id and adjustment.work_date between p_start_date and p_end_date and adjustment.deleted_at is null
  )
  select jsonb_build_object('rows',coalesce(jsonb_agg(jsonb_build_object(
    'workDate',row.work_date,'subject',row.subject,'task',row.task,'minutes',row.minutes,
    'timeRange',row.time_range,'note',row.note) order by row.work_date,row.order_time,row.item_time,row.item_id),'[]'::jsonb))
  into data_value from rows row where row.minutes<>0;
  return timegenie.direct_cloud_query_response(
    p_workspace_id,p_device_id,data_value,array['tasks','time'],p_expected_epoch);
end $$;

revoke all on function timegenie.direct_cloud_daily_report_source_ref(text,date,text,text) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_daily_report_imported_fingerprints(uuid,uuid,uuid,date,text,text,text[],bigint) from public,anon;
revoke all on function timegenie.direct_cloud_daily_report_import_confirm(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_time_allocation_export(uuid,uuid,date,date,boolean,bigint) from public,anon;
grant execute on function timegenie.direct_cloud_daily_report_imported_fingerprints(uuid,uuid,uuid,date,text,text,text[],bigint) to authenticated;
grant execute on function timegenie.direct_cloud_daily_report_import_confirm(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_time_allocation_export(uuid,uuid,date,date,boolean,bigint) to authenticated;

commit;

notify pgrst,'reload schema';
