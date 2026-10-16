begin;

-- Named report templates made report_templates.name mandatory. Some upgraded
-- projects still retained an older workspace bootstrap wrapper that inserted
-- the three built-in templates without a name. Keep this function complete
-- and self-contained so later wrappers cannot re-enter that legacy function.
create or replace function timegenie.workspace_initialize_defaults(p_workspace_id uuid)
returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare default_subject_id uuid; target_type text; target_template_id uuid;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;

  select id into default_subject_id from timegenie.subjects
  where workspace_id=p_workspace_id and deleted_at is null
  order by sort_order,created_at,id limit 1;
  if default_subject_id is null then
    insert into timegenie.subjects(workspace_id,name,sort_order)
    values(p_workspace_id,'默认',10) returning id into default_subject_id;
  end if;

  insert into timegenie.app_settings(workspace_id,key,value_json) values
    (p_workspace_id,'default_subject_id',to_jsonb(default_subject_id::text)),
    (p_workspace_id,'timezone','"Asia/Shanghai"'::jsonb),
    (p_workspace_id,'unassigned_prompt_seconds','300'::jsonb),
    (p_workspace_id,'default_work_period_text','""'::jsonb),
    (p_workspace_id,'salary_hourly_rate','0'::jsonb)
  on conflict(workspace_id,key) do nothing;

  insert into timegenie.report_templates(
    workspace_id,report_type,name,description,content,is_default,is_builtin,sort_order
  ) values
    (p_workspace_id,'daily','标准日报','系统内置模板',E'【{{姓名}}】{{日期}} {{星期}} {{工作时段}} {{总工时}}\n\n{{今日事项}}\n\n## 明日计划：\n\n{{明日计划}}\n',false,true,10),
    (p_workspace_id,'weekly','标准周报','系统内置模板',E'# {{日期范围}}\n\n{{统计信息}}\n\n## 每日情况\n\n{{每日情况}}\n\n## 总结\n\n{{总结}}\n',false,true,10),
    (p_workspace_id,'monthly','标准月报','系统内置模板',E'# {{日期范围}}\n\n{{统计信息}}\n\n## 本月事项\n\n{{每日情况}}\n\n## 总结\n\n{{总结}}\n',false,true,10)
  on conflict do nothing;

  foreach target_type in array array['daily','weekly','monthly'] loop
    if not exists(
      select 1 from timegenie.report_templates
      where workspace_id=p_workspace_id and report_type=target_type
        and is_default and deleted_at is null
    ) then
      select id into target_template_id from timegenie.report_templates
      where workspace_id=p_workspace_id and report_type=target_type and deleted_at is null
      order by case when is_builtin then 0 else 1 end,sort_order,created_at,id limit 1;
      update timegenie.report_templates set is_default=true
      where id=target_template_id;
    end if;
    target_template_id:=null;
  end loop;

  insert into timegenie.subject_work_schedules(workspace_id,subject_id,weekday,is_workday)
  select p_workspace_id,default_subject_id,weekday.value,weekday.value<5
  from generate_series(0,6) weekday(value)
  on conflict(workspace_id,subject_id,weekday) do nothing;

  insert into timegenie.subject_work_schedule_segments(
    id,workspace_id,schedule_id,sequence_no,start_minute,end_minute
  )
  select gen_random_uuid(),schedule.workspace_id,schedule.id,segment.sequence_no,segment.start_minute,segment.end_minute
  from timegenie.subject_work_schedules schedule
  cross join (values (1,600,720),(2,810,1050)) segment(sequence_no,start_minute,end_minute)
  where schedule.workspace_id=p_workspace_id and schedule.subject_id=default_subject_id
    and schedule.weekday<5 and schedule.is_workday
    and not exists(
      select 1 from timegenie.subject_work_schedule_segments existing
      where existing.schedule_id=schedule.id
    )
  on conflict(schedule_id,sequence_no) do nothing;

  return jsonb_build_object('workspaceId',p_workspace_id,'defaultSubjectId',default_subject_id);
end $$;

revoke execute on function timegenie.workspace_initialize_defaults(uuid) from public,anon;
grant execute on function timegenie.workspace_initialize_defaults(uuid) to authenticated;

commit;
notify pgrst, 'reload schema';
