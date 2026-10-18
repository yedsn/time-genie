-- TimeGenie semantic cloud operation history. Idempotent upgrade patch.

create table if not exists timegenie.workspace_history_settings (
  workspace_id uuid primary key references timegenie.workspaces(id) on delete cascade,
  operation_log_retention_days integer not null default 90 check(operation_log_retention_days between 7 and 365),
  updated_at timestamptz not null default now()
);

create table if not exists timegenie.cloud_operation_logs (
  id uuid primary key default gen_random_uuid(),
  workspace_id uuid not null references timegenie.workspaces(id) on delete cascade,
  operation_id uuid not null,
  business_type text not null,
  source text not null check(source in ('user','automation','import','migration','restore','system')),
  device_id uuid references timegenie.devices(id) on delete set null,
  device_name_snapshot text not null,
  summary text not null,
  affected_entity_count integer not null default 0 check(affected_entity_count >= 0),
  metadata jsonb not null default '{}'::jsonb,
  occurred_at timestamptz not null default now(),
  expires_at timestamptz not null,
  unique(workspace_id, operation_id)
);
create index if not exists idx_cloud_operation_logs_page
  on timegenie.cloud_operation_logs(workspace_id, occurred_at desc, id desc);
create index if not exists idx_cloud_operation_logs_filters
  on timegenie.cloud_operation_logs(workspace_id, source, business_type, device_id, occurred_at desc);
create index if not exists idx_cloud_operation_logs_expiry
  on timegenie.cloud_operation_logs(expires_at);

create table if not exists timegenie.cloud_operation_log_items (
  id bigint generated always as identity primary key,
  operation_log_id uuid not null references timegenie.cloud_operation_logs(id) on delete cascade,
  workspace_id uuid not null references timegenie.workspaces(id) on delete cascade,
  entity_type text not null,
  entity_id text not null,
  entity_label_snapshot text not null,
  action text not null check(action in ('create','update','delete','restore')),
  before_json jsonb,
  after_json jsonb,
  changed_fields text[] not null default '{}',
  search_text text not null default ''
);
create index if not exists idx_cloud_operation_log_items_log
  on timegenie.cloud_operation_log_items(operation_log_id, id);
create index if not exists idx_cloud_operation_log_items_search
  on timegenie.cloud_operation_log_items using gin(to_tsvector('simple', search_text));

alter table timegenie.workspace_history_settings enable row level security;
alter table timegenie.cloud_operation_logs enable row level security;
alter table timegenie.cloud_operation_log_items enable row level security;

drop policy if exists workspace_owner on timegenie.workspace_history_settings;
create policy workspace_owner on timegenie.workspace_history_settings for all
using(timegenie.is_workspace_owner(workspace_id))
with check(timegenie.is_workspace_owner(workspace_id));
drop policy if exists workspace_owner on timegenie.cloud_operation_logs;
create policy workspace_owner on timegenie.cloud_operation_logs for select
using(timegenie.is_workspace_owner(workspace_id));
drop policy if exists workspace_owner on timegenie.cloud_operation_log_items;
create policy workspace_owner on timegenie.cloud_operation_log_items for select
using(timegenie.is_workspace_owner(workspace_id));

create or replace function timegenie.operation_history_source(p_operation_type text)
returns text language sql immutable as $$
  select case
    when coalesce(p_operation_type,'') ~* '(restore|rollback)' then 'restore'
    when coalesce(p_operation_type,'') ~* '(migration)' then 'migration'
    when coalesce(p_operation_type,'') ~* '(import|obsidian)' then 'import'
    when coalesce(p_operation_type,'') ~* '(automation|hook)' then 'automation'
    when coalesce(p_operation_type,'') ~* '(snapshot|cleanup)' then 'system'
    else 'user'
  end;
$$;

create or replace function timegenie.operation_history_summary(p_operation_type text)
returns text language sql immutable as $$
  select case
    when p_operation_type ~* 'task.*complete' then '完成事项'
    when p_operation_type ~* 'task.*delete' then '删除事项'
    when p_operation_type ~* 'task.*create' then '创建事项'
    when p_operation_type ~* 'task' then '修改事项'
    when p_operation_type ~* 'timer.*start' then '开始计时'
    when p_operation_type ~* 'timer.*pause' then '暂停计时'
    when p_operation_type ~* 'timer.*resume' then '继续计时'
    when p_operation_type ~* 'timer.*stop' then '结束计时'
    when p_operation_type ~* 'time|allocation|adjustment' then '调整工时'
    when p_operation_type ~* 'report' then '修改报告'
    when p_operation_type ~* 'setting|schedule' then '修改设置'
    when p_operation_type ~* 'migration' then '迁移工作空间数据'
    when p_operation_type ~* 'restore|rollback' then '回滚工作空间'
    when p_operation_type ~* 'snapshot' then '创建历史镜像'
    else replace(initcap(replace(coalesce(p_operation_type,'业务操作'),'_',' ')),'.',' · ')
  end;
$$;

create or replace function timegenie.operation_history_safe_json(p_entity_type text, p_value jsonb)
returns jsonb language plpgsql immutable as $$
declare allowed text[]; result_value jsonb := '{}'::jsonb; field_name text;
begin
  if p_value is null or jsonb_typeof(p_value) <> 'object' then return null; end if;
  allowed := case regexp_replace(coalesce(p_entity_type,''), 's$', '')
    when 'subject' then array['id','name','sort_order','deleted_at']
    when 'subject_work_schedule' then array['id','subject_id','weekday','is_workday','segments','version']
    when 'work_day' then array['work_date','timezone','work_period_text','note','settled_at','version']
    when 'task' then array['id','subject_id','parent_id','title','status','planned_date','planned_time','estimate_minutes','note','project_name','solution_name','sort_order','completed_at','deleted_at','version']
    when 'task_status_event' then array['id','task_id','status','occurred_at','source_type']
    when 'task_daily_estimate' then array['task_id','work_date','estimate_minutes','version']
    when 'task_recurrence_rule' then array['id','task_id','frequency','weekdays_mask','effective_start','effective_end','version']
    when 'task_occurrence' then array['task_id','occurrence_date','origin','status','completed_at','version']
    when 'time_entry' then array['id','work_date','kind','source_type','state','default_task_id','label_snapshot','started_at','ended_at','duration_seconds','note','deleted_at','version']
    when 'time_segment' then array['id','entry_id','sequence_no','started_at','ended_at','duration_seconds']
    when 'time_allocation' then array['id','entry_id','task_id','minutes','note','version']
    when 'actual_time_adjustment' then array['id','subject_id','task_id','work_date','delta_minutes','baseline_minutes','target_minutes','note','deleted_at','version']
    when 'unassigned_session' then array['id','work_date','state','duration_seconds','resolution_type','resolved_at','version']
    when 'report_template' then array['id','report_type','subject_id','name','description','is_default','sort_order','deleted_at','version']
    when 'report' then array['id','report_type','subject_id','period_start','period_end','reference_date','template_id','markdown_content','content_source','deleted_at','version']
    when 'report_task' then array['report_id','task_id','sort_order','title_snapshot','path_snapshot']
    when 'app_setting' then array['key','value_json','version']
    when 'integration_config' then array['provider','enabled','version']
    when 'external_binding' then array['id','provider','entity_type','entity_id','external_id','external_revision','last_synced_at']
    else array['id','entity_id','title','name','status','version','deleted_at']
  end;
  foreach field_name in array allowed loop
    if p_value ? field_name then result_value := result_value || jsonb_build_object(field_name, p_value->field_name); end if;
  end loop;
  return result_value;
end $$;

create or replace function timegenie.operation_history_label(p_entity_type text, p_value jsonb, p_entity_id text)
returns text language sql immutable as $$
  select coalesce(nullif(p_value->>'title',''), nullif(p_value->>'name',''), nullif(p_value->>'label_snapshot',''),
    nullif(p_value->>'reference_date',''), nullif(p_value->>'work_date',''), nullif(p_value->>'provider',''),
    nullif(p_value->>'key',''), nullif(p_entity_id,''), '未知对象');
$$;

create or replace function timegenie.operation_history_changed_fields(p_before jsonb, p_after jsonb)
returns text[] language sql immutable as $$
  select coalesce(array_agg(key order by key) filter(where coalesce(p_before->key,'null'::jsonb) is distinct from coalesce(p_after->key,'null'::jsonb)), '{}'::text[])
  from (select key from jsonb_object_keys(coalesce(p_before,'{}'::jsonb)) key union select key from jsonb_object_keys(coalesce(p_after,'{}'::jsonb)) key) keys;
$$;

create or replace function timegenie.operation_history_upsert(
  p_workspace_id uuid, p_operation_id uuid, p_device_id uuid, p_operation_type text, p_request jsonb, p_result jsonb
) returns uuid language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare log_id uuid; retention_days integer; device_name_value text; entity_type text; entity_id text; safe_after jsonb; action_value text;
begin
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  insert into timegenie.workspace_history_settings(workspace_id) values(p_workspace_id) on conflict(workspace_id) do nothing;
  select operation_log_retention_days into retention_days from timegenie.workspace_history_settings where workspace_id=p_workspace_id;
  select coalesce(device.device_name,'未知设备') into device_name_value from timegenie.devices device where device.workspace_id=p_workspace_id and device.id=p_device_id;
  insert into timegenie.cloud_operation_logs(workspace_id,operation_id,business_type,source,device_id,device_name_snapshot,summary,metadata,expires_at)
  values(p_workspace_id,p_operation_id,p_operation_type,timegenie.operation_history_source(p_operation_type),p_device_id,coalesce(device_name_value,'未知设备'),timegenie.operation_history_summary(p_operation_type),
    jsonb_strip_nulls(jsonb_build_object('resultVersion',p_result->'version','snapshotId',p_result->'snapshotId','safetySnapshotId',p_result->'safetySnapshotId')),
    now()+make_interval(days=>coalesce(retention_days,90)))
  on conflict(workspace_id,operation_id) do update set operation_id=excluded.operation_id
  returning id into log_id;
  if exists(select 1 from timegenie.cloud_operation_log_items where operation_log_id=log_id) then return log_id; end if;
  entity_type:=coalesce(p_request->>'entityType',p_result->>'entityType',case when p_operation_type~*'migration' then 'workspace' end,'operation');
  entity_id:=coalesce(p_request->>'entityId',p_result->>'entityId',p_workspace_id::text);
  safe_after:=timegenie.operation_history_safe_json(entity_type,coalesce(p_result->'authoritative',p_request->'payload',p_result));
  action_value:=case when p_operation_type~*'(delete|discard)' then 'delete' when p_operation_type~*'(create|start|import)' then 'create' when p_operation_type~*'(restore|rollback)' then 'restore' else 'update' end;
  insert into timegenie.cloud_operation_log_items(operation_log_id,workspace_id,entity_type,entity_id,entity_label_snapshot,action,before_json,after_json,changed_fields,search_text)
  values(log_id,p_workspace_id,entity_type,entity_id,timegenie.operation_history_label(entity_type,safe_after,entity_id),action_value,null,safe_after,timegenie.operation_history_changed_fields(null,safe_after),
    concat_ws(' ',entity_type,entity_id,timegenie.operation_history_label(entity_type,safe_after,entity_id)));
  update timegenie.cloud_operation_logs set affected_entity_count=1 where id=log_id;
  return log_id;
end $$;

create or replace function timegenie.operation_history_replace_item(
  p_workspace_id uuid,p_operation_id uuid,p_entity_type text,p_entity_id text,p_before jsonb,p_after jsonb
) returns void language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare log_id uuid; safe_before jsonb; safe_after jsonb; label_value text; action_value text;
begin
  select id into log_id from timegenie.cloud_operation_logs where workspace_id=p_workspace_id and operation_id=p_operation_id;
  if log_id is null then return; end if;
  safe_before:=timegenie.operation_history_safe_json(p_entity_type,p_before);
  safe_after:=timegenie.operation_history_safe_json(p_entity_type,p_after);
  label_value:=timegenie.operation_history_label(p_entity_type,coalesce(safe_after,safe_before),p_entity_id);
  action_value:=case when p_before is null then 'create' when p_after is null or p_after->>'deleted_at' is not null then 'delete' else 'update' end;
  delete from timegenie.cloud_operation_log_items where operation_log_id=log_id;
  insert into timegenie.cloud_operation_log_items(operation_log_id,workspace_id,entity_type,entity_id,entity_label_snapshot,action,before_json,after_json,changed_fields,search_text)
  values(log_id,p_workspace_id,p_entity_type,p_entity_id,label_value,action_value,safe_before,safe_after,timegenie.operation_history_changed_fields(safe_before,safe_after),concat_ws(' ',p_entity_type,p_entity_id,label_value));
  update timegenie.cloud_operation_logs set affected_entity_count=1 where id=log_id;
end $$;

do $$ begin
  if to_regprocedure('timegenie.record_processed_operation_pre_operation_history(uuid,uuid,uuid,text,jsonb,jsonb)') is null then
    alter function timegenie.record_processed_operation(uuid,uuid,uuid,text,jsonb,jsonb) rename to record_processed_operation_pre_operation_history;
  end if;
end $$;

create or replace function timegenie.record_processed_operation(
  p_workspace_id uuid,p_operation_id uuid,p_device_id uuid,p_operation_type text,p_request jsonb,p_result_json jsonb
) returns void language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
begin
  perform timegenie.record_processed_operation_pre_operation_history(p_workspace_id,p_operation_id,p_device_id,p_operation_type,p_request,p_result_json);
  perform timegenie.operation_history_upsert(p_workspace_id,p_operation_id,p_device_id,p_operation_type,p_request,p_result_json);
end $$;

do $$ begin
  if pg_get_functiondef('timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,jsonb)'::regprocedure) not like '%operation_history_replace_item%' then
    drop function if exists timegenie.cloud_apply_patch_pre_operation_history(uuid,uuid,uuid,text,text,text,bigint,jsonb);
    alter function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,jsonb) rename to cloud_apply_patch_pre_operation_history;
  end if;
  if pg_get_functiondef('timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,bigint,bigint,jsonb)'::regprocedure) not like '%operation_history_replace_item%' then
    drop function if exists timegenie.cloud_apply_patch_pre_operation_history(uuid,uuid,uuid,text,text,text,bigint,bigint,bigint,jsonb);
    alter function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,bigint,bigint,jsonb) rename to cloud_apply_patch_pre_operation_history;
  end if;
end $$;

create or replace function timegenie.cloud_apply_patch(p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_operation_type text,p_entity_type text,p_entity_id text,p_base_version bigint,p_payload jsonb)
returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare before_value jsonb; after_value jsonb; result_value jsonb; source_type text;
begin
  source_type:=case p_entity_type when 'subject_work_schedule' then 'subject_work_schedules' when 'app_setting' then 'app_settings' else p_entity_type||case when right(p_entity_type,1)='s' then '' else 's' end end;
  begin before_value:=timegenie.cloud_incremental_entity_get(p_workspace_id,source_type,p_entity_id)->'data'; exception when others then before_value:=null; end;
  result_value:=timegenie.cloud_apply_patch_pre_operation_history(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_type,p_entity_id,p_base_version,p_payload);
  begin after_value:=timegenie.cloud_incremental_entity_get(p_workspace_id,source_type,p_entity_id)->'data'; exception when others then after_value:=coalesce(result_value->'authoritative',p_payload); end;
  perform timegenie.operation_history_replace_item(p_workspace_id,p_operation_id,p_entity_type,p_entity_id,before_value,after_value);
  return result_value;
end $$;

create or replace function timegenie.cloud_apply_patch(p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_operation_type text,p_entity_type text,p_entity_id text,p_base_version bigint,p_payload_version bigint,p_coalesced_count bigint,p_payload jsonb)
returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare before_value jsonb; after_value jsonb; result_value jsonb; source_type text;
begin
  source_type:=case p_entity_type when 'subject_work_schedule' then 'subject_work_schedules' when 'app_setting' then 'app_settings' else p_entity_type||case when right(p_entity_type,1)='s' then '' else 's' end end;
  begin before_value:=timegenie.cloud_incremental_entity_get(p_workspace_id,source_type,p_entity_id)->'data'; exception when others then before_value:=null; end;
  result_value:=timegenie.cloud_apply_patch_pre_operation_history(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_type,p_entity_id,p_base_version,p_payload_version,p_coalesced_count,p_payload);
  begin after_value:=timegenie.cloud_incremental_entity_get(p_workspace_id,source_type,p_entity_id)->'data'; exception when others then after_value:=coalesce(result_value->'authoritative',p_payload); end;
  perform timegenie.operation_history_replace_item(p_workspace_id,p_operation_id,p_entity_type,p_entity_id,before_value,after_value);
  return result_value;
end $$;

create or replace function timegenie.cloud_operation_history_list(
  p_workspace_id uuid,p_device_id uuid,p_limit integer default 50,p_cursor_time timestamptz default null,p_cursor_id uuid default null,
  p_from timestamptz default null,p_to timestamptz default null,p_filter_device_id uuid default null,p_source text default null,p_business_type text default null,p_query text default null
) returns jsonb language plpgsql stable security definer set search_path=timegenie,extensions,pg_catalog as $$
declare result_value jsonb; next_row record; safe_limit integer:=least(greatest(coalesce(p_limit,50),1),100);
begin
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  with filtered as (
    select log.* from timegenie.cloud_operation_logs log
    where log.workspace_id=p_workspace_id
      and (p_cursor_time is null or (log.occurred_at,log.id)<(p_cursor_time,coalesce(p_cursor_id,'ffffffff-ffff-ffff-ffff-ffffffffffff'::uuid)))
      and (p_from is null or log.occurred_at>=p_from) and (p_to is null or log.occurred_at<=p_to)
      and (p_filter_device_id is null or log.device_id=p_filter_device_id)
      and (p_source is null or log.source=p_source) and (p_business_type is null or log.business_type=p_business_type)
      and (nullif(btrim(p_query),'') is null or log.summary ilike '%'||p_query||'%' or exists(select 1 from timegenie.cloud_operation_log_items item where item.operation_log_id=log.id and item.search_text ilike '%'||p_query||'%'))
    order by log.occurred_at desc,log.id desc limit safe_limit+1
  ), page as (select * from filtered order by occurred_at desc,id desc limit safe_limit)
  select jsonb_build_object('items',coalesce(jsonb_agg(jsonb_build_object(
    'id',id,'operationId',operation_id,'businessType',business_type,'source',source,'deviceId',device_id,'deviceName',device_name_snapshot,
    'summary',summary,'affectedEntityCount',affected_entity_count,'occurredAt',occurred_at,'expiresAt',expires_at,'metadata',metadata
  ) order by occurred_at desc,id desc),'[]'::jsonb)) into result_value from page;
  select log.occurred_at,log.id into next_row from timegenie.cloud_operation_logs log
    where log.workspace_id=p_workspace_id
      and (p_cursor_time is null or (log.occurred_at,log.id)<(p_cursor_time,coalesce(p_cursor_id,'ffffffff-ffff-ffff-ffff-ffffffffffff'::uuid)))
      and (p_from is null or log.occurred_at>=p_from) and (p_to is null or log.occurred_at<=p_to)
      and (p_filter_device_id is null or log.device_id=p_filter_device_id)
      and (p_source is null or log.source=p_source) and (p_business_type is null or log.business_type=p_business_type)
      and (nullif(btrim(p_query),'') is null or log.summary ilike '%'||p_query||'%' or exists(select 1 from timegenie.cloud_operation_log_items item where item.operation_log_id=log.id and item.search_text ilike '%'||p_query||'%'))
    order by log.occurred_at desc,log.id desc offset safe_limit limit 1;
  return result_value||jsonb_build_object('nextCursor',case when next_row.id is null then null else jsonb_build_object('occurredAt',next_row.occurred_at,'id',next_row.id) end);
end $$;

create or replace function timegenie.cloud_operation_history_detail(p_workspace_id uuid,p_device_id uuid,p_log_id uuid)
returns jsonb language plpgsql stable security definer set search_path=timegenie,extensions,pg_catalog as $$
declare result_value jsonb;
begin
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  select to_jsonb(log)||jsonb_build_object('items',coalesce((select jsonb_agg(jsonb_build_object(
    'id',item.id,'entityType',item.entity_type,'entityId',item.entity_id,'entityLabel',item.entity_label_snapshot,'action',item.action,
    'before',item.before_json,'after',item.after_json,'changedFields',item.changed_fields
  ) order by item.id) from timegenie.cloud_operation_log_items item where item.operation_log_id=log.id),'[]'::jsonb)) into result_value
  from timegenie.cloud_operation_logs log where log.workspace_id=p_workspace_id and log.id=p_log_id;
  if result_value is null then raise exception 'OPERATION_LOG_NOT_FOUND'; end if;
  return result_value;
end $$;

create or replace function timegenie.cloud_operation_history_cleanup(p_workspace_id uuid,p_device_id uuid)
returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare affected integer;
begin
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  delete from timegenie.cloud_operation_logs where workspace_id=p_workspace_id and expires_at<=now();
  get diagnostics affected=row_count;
  return jsonb_build_object('deletedCount',affected);
end $$;

revoke all on timegenie.workspace_history_settings,timegenie.cloud_operation_logs,timegenie.cloud_operation_log_items from public,anon,authenticated;
revoke execute on function timegenie.operation_history_source(text) from public,anon,authenticated;
revoke execute on function timegenie.operation_history_summary(text) from public,anon,authenticated;
revoke execute on function timegenie.operation_history_safe_json(text,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.operation_history_label(text,jsonb,text) from public,anon,authenticated;
revoke execute on function timegenie.operation_history_changed_fields(jsonb,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.operation_history_upsert(uuid,uuid,uuid,text,jsonb,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.operation_history_replace_item(uuid,uuid,text,text,jsonb,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.record_processed_operation_pre_operation_history(uuid,uuid,uuid,text,jsonb,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.record_processed_operation(uuid,uuid,uuid,text,jsonb,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.cloud_apply_patch_pre_operation_history(uuid,uuid,uuid,text,text,text,bigint,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.cloud_apply_patch_pre_operation_history(uuid,uuid,uuid,text,text,text,bigint,bigint,bigint,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,jsonb) from public,anon;
revoke execute on function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,bigint,bigint,jsonb) from public,anon;
revoke execute on function timegenie.cloud_operation_history_list(uuid,uuid,integer,timestamptz,uuid,timestamptz,timestamptz,uuid,text,text,text) from public,anon;
revoke execute on function timegenie.cloud_operation_history_detail(uuid,uuid,uuid) from public,anon;
revoke execute on function timegenie.cloud_operation_history_cleanup(uuid,uuid) from public,anon;
grant execute on function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,jsonb) to authenticated;
grant execute on function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,bigint,bigint,jsonb) to authenticated;
grant execute on function timegenie.cloud_operation_history_list(uuid,uuid,integer,timestamptz,uuid,timestamptz,timestamptz,uuid,text,text,text) to authenticated;
grant execute on function timegenie.cloud_operation_history_detail(uuid,uuid,uuid) to authenticated;
grant execute on function timegenie.cloud_operation_history_cleanup(uuid,uuid) to authenticated;

notify pgrst,'reload schema';
