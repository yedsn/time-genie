-- Disposable database regression: the two report sections mirror the two lists.
begin;
reset role;
insert into auth.users(id) values('da000000-0000-0000-0000-000000000001');
insert into timegenie.workspaces(id,owner_user_id,name) values(
  'db000000-0000-0000-0000-000000000001','da000000-0000-0000-0000-000000000001','日报列表回归'
);
insert into timegenie.devices(id,workspace_id,device_name,platform,app_version) values(
  'dc000000-0000-0000-0000-000000000001','db000000-0000-0000-0000-000000000001','隔离测试','windows','0.5.9'
);
insert into timegenie.subjects(id,workspace_id,name,sort_order) values(
  'dd000000-0000-0000-0000-000000000001','db000000-0000-0000-0000-000000000001','报告主体',10
);
insert into timegenie.tasks(id,workspace_id,subject_id,parent_id,title,status,source_type,sort_order) values
  ('de000000-0000-0000-0000-000000000001','db000000-0000-0000-0000-000000000001','dd000000-0000-0000-0000-000000000001',null,'进行中父项','open','manual',10),
  ('de000000-0000-0000-0000-000000000002','db000000-0000-0000-0000-000000000001','dd000000-0000-0000-0000-000000000001','de000000-0000-0000-0000-000000000001','仅工作日来源','open','manual',10),
  ('de000000-0000-0000-0000-000000000003','db000000-0000-0000-0000-000000000001','dd000000-0000-0000-0000-000000000001',null,'仅有工时不在今日','open','manual',20);
insert into timegenie.task_recurrence_rules(id,workspace_id,task_id,frequency,effective_start) values(
  'df000000-0000-0000-0000-000000000001','db000000-0000-0000-0000-000000000001','de000000-0000-0000-0000-000000000002','weekdays','2026-01-01'
);
insert into timegenie.tasks(id,workspace_id,subject_id,parent_id,title,status,completed_at,source_type,sort_order,day_instance_date,day_instance_kind,day_instance_source_task_id) values
  ('de000000-0000-0000-0000-000000000004','db000000-0000-0000-0000-000000000001','dd000000-0000-0000-0000-000000000001',null,'今日独立父项','open',null,'manual',10,'2026-10-02','context','de000000-0000-0000-0000-000000000001'),
  ('de000000-0000-0000-0000-000000000005','db000000-0000-0000-0000-000000000001','dd000000-0000-0000-0000-000000000001','de000000-0000-0000-0000-000000000004','今日独立子项','done','2026-10-02 10:00:00+08','manual',10,'2026-10-02','member','de000000-0000-0000-0000-000000000002'),
  ('de000000-0000-0000-0000-000000000006','db000000-0000-0000-0000-000000000001','dd000000-0000-0000-0000-000000000001',null,'今日未执行','open',null,'manual',20,'2026-10-02','member',null);
insert into timegenie.actual_time_adjustments(id,workspace_id,subject_id,task_id,work_date,delta_minutes,baseline_minutes,target_minutes,source_type,batch_id) values
  ('df000000-0000-0000-0000-000000000002','db000000-0000-0000-0000-000000000001','dd000000-0000-0000-0000-000000000001','de000000-0000-0000-0000-000000000002','2026-10-02',15,0,15,'quick_actual_adjustment','df000000-0000-0000-0000-000000000004'),
  ('df000000-0000-0000-0000-000000000003','db000000-0000-0000-0000-000000000001','dd000000-0000-0000-0000-000000000001','de000000-0000-0000-0000-000000000003','2026-10-02',40,0,40,'quick_actual_adjustment','df000000-0000-0000-0000-000000000004');
insert into timegenie.report_templates(id,workspace_id,report_type,name,content,is_default,is_builtin,sort_order) values(
  'df000000-0000-0000-0000-000000000005','db000000-0000-0000-0000-000000000001','daily','列表映射模板',E'{{今日事项}}\n\n## 明日计划：\n{{明日计划}}\n合计 {{总工时}}',false,false,10
);
commit;

begin read only;
select set_config('request.jwt.claim.sub','da000000-0000-0000-0000-000000000001',true);
set local role authenticated;
do $$
declare preview_value jsonb; suggestions jsonb; markdown_value text;
begin
  preview_value:=timegenie.direct_cloud_report_preview(
    'db000000-0000-0000-0000-000000000001','dc000000-0000-0000-0000-000000000001','daily','2026-10-02',
    'dd000000-0000-0000-0000-000000000001','df000000-0000-0000-0000-000000000005',1
  );
  markdown_value:=preview_value->'data'->>'markdown';
  if markdown_value not like E'1. 今日独立父项\n   - 🟢 今日独立子项 <实际：15min>\n2. 🔴 今日未执行%'
     or split_part(markdown_value,'## 明日计划：',1) like '%仅有工时不在今日%'
     or split_part(markdown_value,'## 明日计划：',2) not like '%仅工作日来源%'
     or split_part(markdown_value,'## 明日计划：',2) like '%今日独立子项%'
     or (preview_value->'data'->'context'->>'totalMinutes')::integer<>15
     or jsonb_array_length(preview_value->'data'->'context'->'taskIds')<>2 then
    raise exception 'daily todo mapping or read-only preview failed: %',preview_value;
  end if;
  suggestions:=timegenie.direct_cloud_report_task_suggestions(
    'db000000-0000-0000-0000-000000000001','dc000000-0000-0000-0000-000000000001','daily','2026-10-02',
    'dd000000-0000-0000-0000-000000000001','{}'::uuid[],1
  );
  if jsonb_array_length(suggestions->'data'->'tasks')<>3 or not exists(
    select 1 from jsonb_array_elements(suggestions->'data'->'tasks') task
    where task->>'title'='今日未执行' and (task->>'defaultSelected')::boolean
  ) then raise exception 'unexecuted daily candidate missing: %',suggestions; end if;
end $$;
rollback;

begin;
select set_config('request.jwt.claim.sub','da000000-0000-0000-0000-000000000001',true);
set local role authenticated;
do $$
declare preview_value jsonb; created jsonb;
begin
  preview_value:=timegenie.direct_cloud_report_preview(
    'db000000-0000-0000-0000-000000000001','dc000000-0000-0000-0000-000000000001','daily','2026-10-02',
    'dd000000-0000-0000-0000-000000000001','df000000-0000-0000-0000-000000000005',1
  );
  created:=timegenie.direct_cloud_report_create(
    'db000000-0000-0000-0000-000000000001','dc000000-0000-0000-0000-000000000001','df000000-0000-0000-0000-000000000006',1,
    '{"reportId":"df000000-0000-0000-0000-000000000007","reportType":"daily","referenceDate":"2026-10-02","subjectId":"dd000000-0000-0000-0000-000000000001","templateId":"df000000-0000-0000-0000-000000000005","taskIds":[]}'::jsonb
  );
  if created->'data'->'report'->>'markdown' is distinct from preview_value->'data'->>'markdown' then
    raise exception 'daily create differs from preview';
  end if;
end $$;
reset role;
update timegenie.tasks set deleted_at=now() where id='de000000-0000-0000-0000-000000000006';
insert into timegenie.tasks(id,workspace_id,subject_id,title,status,source_type,sort_order,day_instance_date,day_instance_kind) values(
  'de000000-0000-0000-0000-000000000007','db000000-0000-0000-0000-000000000001','dd000000-0000-0000-0000-000000000001','后来加入今日','open','manual',30,'2026-10-02','member'
);
set local role authenticated;
do $$
declare regenerated jsonb; retry_value jsonb;
begin
  regenerated:=timegenie.direct_cloud_report_regenerate(
    'db000000-0000-0000-0000-000000000001','dc000000-0000-0000-0000-000000000001','df000000-0000-0000-0000-000000000008',1,
    '{"reportId":"df000000-0000-0000-0000-000000000007","expectedVersion":1}'::jsonb
  );
  retry_value:=timegenie.direct_cloud_report_regenerate(
    'db000000-0000-0000-0000-000000000001','dc000000-0000-0000-0000-000000000001','df000000-0000-0000-0000-000000000008',1,
    '{"reportId":"df000000-0000-0000-0000-000000000007","expectedVersion":1}'::jsonb
  );
  if regenerated is distinct from retry_value
     or regenerated->'data'->'report'->>'markdown' not like '%后来加入今日%'
     or regenerated->'data'->'report'->>'markdown' like '%今日未执行%'
     or not (regenerated->'data'->'report'->'taskIds' @> '["de000000-0000-0000-0000-000000000007"]'::jsonb)
     or regenerated->'data'->'report'->'taskIds' @> '["de000000-0000-0000-0000-000000000006"]'::jsonb then
    raise exception 'daily regeneration failed to refresh membership/idempotency: %',regenerated;
  end if;
end $$;
rollback;
