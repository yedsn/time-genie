begin;

alter table timegenie.report_templates add column if not exists name text;
alter table timegenie.report_templates add column if not exists description text;
alter table timegenie.report_templates add column if not exists is_default boolean not null default false;
alter table timegenie.report_templates add column if not exists sort_order integer not null default 0;

drop index if exists timegenie.uq_report_templates_scope;
drop index if exists timegenie.uq_report_templates_active_name;
drop index if exists timegenie.uq_report_templates_active_default;

update timegenie.report_templates
set name = case report_type when 'daily' then '标准日报' when 'weekly' then '标准周报' else '标准月报' end
where subject_id is null and nullif(btrim(name),'') is null;

with named as (
  select template.id,
         subject.name || ' - ' || case template.report_type when 'daily' then '日报' when 'weekly' then '周报' else '月报' end as base_name,
         row_number() over (
           partition by template.workspace_id, template.report_type, lower(btrim(subject.name))
           order by template.id
         ) as duplicate_no
  from timegenie.report_templates template
  join timegenie.subjects subject on subject.id=template.subject_id
  where template.subject_id is not null
)
update timegenie.report_templates template
set name = named.base_name || case when named.duplicate_no > 1 then ' (' || named.duplicate_no || ')' else '' end
from named where named.id=template.id and nullif(btrim(template.name),'') is null;

update timegenie.report_templates
set name = case report_type when 'daily' then '迁移日报' when 'weekly' then '迁移周报' else '迁移月报' end || ' - ' || left(id::text,8)
where nullif(btrim(name),'') is null;

update timegenie.report_templates set subject_id=null;

with ranked as (
  select id, row_number() over (
    partition by workspace_id, report_type
    order by case when is_builtin then 0 else 1 end, created_at, id
  ) as default_rank,
  row_number() over (partition by workspace_id, report_type order by sort_order, created_at, id) as order_rank
  from timegenie.report_templates where deleted_at is null
)
update timegenie.report_templates template
set is_default=(ranked.default_rank=1),
    sort_order=case when template.sort_order=0 then ranked.order_rank * 10 else template.sort_order end
from ranked where ranked.id=template.id;

alter table timegenie.report_templates alter column name set not null;
alter table timegenie.report_templates drop constraint if exists report_templates_name_not_blank;
alter table timegenie.report_templates add constraint report_templates_name_not_blank check (length(btrim(name)) > 0);

create unique index if not exists uq_report_templates_active_name
  on timegenie.report_templates(workspace_id, report_type, lower(btrim(name))) where deleted_at is null;
create unique index if not exists uq_report_templates_active_default
  on timegenie.report_templates(workspace_id, report_type) where is_default and deleted_at is null;
create index if not exists idx_report_templates_active_order
  on timegenie.report_templates(workspace_id, report_type, sort_order, name) where deleted_at is null;

create or replace function timegenie.set_report_template_default(
  p_workspace_id uuid, p_template_id uuid, p_expected_version bigint default null
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare target timegenie.report_templates%rowtype;
begin
  if not timegenie.has_workspace_access(p_workspace_id) then raise exception 'workspace access denied'; end if;
  select * into target from timegenie.report_templates
  where id=p_template_id and workspace_id=p_workspace_id and deleted_at is null for update;
  if not found then raise exception 'report template not found'; end if;
  if p_expected_version is not null and target.version<>p_expected_version then raise exception 'version conflict'; end if;
  update timegenie.report_templates set is_default=false,updated_at=now(),version=version+1
  where workspace_id=p_workspace_id and report_type=target.report_type and is_default and id<>p_template_id and deleted_at is null;
  update timegenie.report_templates set is_default=true,updated_at=now(),version=version+1 where id=p_template_id;
  return (select to_jsonb(row_value) from timegenie.report_templates row_value where id=p_template_id);
end $$;

create or replace function timegenie.apply_named_report_template(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_operation_type text,p_entity_id text,p_base_version bigint,p_payload jsonb,p_request jsonb
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare existing timegenie.report_templates%rowtype; payload_version bigint; cached jsonb; result_value jsonb; target_type text; target_name text; deleting boolean; replacement_id uuid;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  cached:=timegenie.processed_operation_result(p_workspace_id,p_operation_id,p_operation_type,p_request);
  if cached is not null then return cached; end if;
  if p_entity_id is null or p_payload->>'id' is distinct from p_entity_id then raise exception 'VALIDATION_ERROR: entity id does not match payload id'; end if;
  target_type:=p_payload->>'report_type'; target_name:=btrim(p_payload->>'name');
  if target_type not in ('daily','weekly','monthly') then raise exception 'VALIDATION_ERROR: invalid report type'; end if;
  if target_name is null or target_name='' then raise exception 'VALIDATION_ERROR: report template name is required'; end if;
  if coalesce(btrim(p_payload->>'content'),'')='' then raise exception 'VALIDATION_ERROR: report template content is required'; end if;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':report-template:'||target_type,0));
  select * into existing from timegenie.report_templates where workspace_id=p_workspace_id and id=p_entity_id::uuid for update;
  payload_version:=coalesce((p_payload->>'version')::bigint,1);
  if p_base_version is null and found then raise exception 'SYNC_CONFLICT: expected new entity, current version %',existing.version; end if;
  if p_base_version is not null and (not found or existing.version<>p_base_version) then raise exception 'SYNC_CONFLICT: expected version %, current version %',p_base_version,existing.version; end if;
  if p_base_version is null and payload_version<>1 then raise exception 'VERSION_CONFLICT: new entity payload version must be 1'; end if;
  if p_base_version is not null and payload_version<=p_base_version then raise exception 'VERSION_CONFLICT: template payload version must advance'; end if;
  deleting:=p_payload->>'deleted_at' is not null;
  if deleting and (select count(*) from timegenie.report_templates where workspace_id=p_workspace_id and report_type=target_type and deleted_at is null and id<>p_entity_id::uuid)=0 then
    raise exception 'VALIDATION_ERROR: each report type requires one template';
  end if;
  if deleting and existing.is_default then
    replacement_id:=nullif(p_payload->>'replacement_template_id','')::uuid;
    if replacement_id is null or not exists(
      select 1 from timegenie.report_templates
      where id=replacement_id and workspace_id=p_workspace_id and report_type=target_type
        and id<>p_entity_id::uuid and deleted_at is null
    ) then
      raise exception 'VALIDATION_ERROR: deleting the default template requires a compatible replacement';
    end if;
  end if;
  if coalesce((p_payload->>'is_default')::boolean,false) and not deleting then
    update timegenie.report_templates set is_default=false,updated_at=clock_timestamp(),version=version+1
    where workspace_id=p_workspace_id and report_type=target_type and id<>p_entity_id::uuid and is_default and deleted_at is null;
  end if;
  insert into timegenie.report_templates(id,workspace_id,report_type,subject_id,name,description,content,is_default,is_builtin,sort_order,created_at,updated_at,version,deleted_at)
  values(p_entity_id::uuid,p_workspace_id,target_type,null,target_name,nullif(p_payload->>'description',''),p_payload->>'content',coalesce((p_payload->>'is_default')::boolean,false) and not deleting,coalesce((p_payload->>'is_builtin')::boolean,false),coalesce((p_payload->>'sort_order')::integer,0),to_timestamp(coalesce((p_payload->>'created_at')::double precision,extract(epoch from clock_timestamp())*1000)/1000),to_timestamp(coalesce((p_payload->>'updated_at')::double precision,extract(epoch from clock_timestamp())*1000)/1000),payload_version,case when deleting then to_timestamp((p_payload->>'deleted_at')::double precision/1000) else null end)
  on conflict(id) do update set report_type=excluded.report_type,subject_id=null,name=excluded.name,description=excluded.description,content=excluded.content,is_default=excluded.is_default,is_builtin=excluded.is_builtin,sort_order=excluded.sort_order,updated_at=excluded.updated_at,version=excluded.version,deleted_at=excluded.deleted_at;
  if replacement_id is not null then
    update timegenie.report_templates set is_default=false,updated_at=clock_timestamp(),version=version+1
    where workspace_id=p_workspace_id and report_type=target_type and id<>replacement_id and is_default and deleted_at is null;
    update timegenie.report_templates set is_default=true,updated_at=clock_timestamp(),version=version+1
    where id=replacement_id;
  end if;
  if not exists(select 1 from timegenie.report_templates where workspace_id=p_workspace_id and report_type=target_type and is_default and deleted_at is null) then
    update timegenie.report_templates set is_default=true,updated_at=clock_timestamp(),version=version+1
    where id=(select id from timegenie.report_templates where workspace_id=p_workspace_id and report_type=target_type and deleted_at is null order by sort_order,created_at,id limit 1);
  end if;
  result_value:=jsonb_build_object('entityType','report_template','entityId',p_entity_id,'version',(select version from timegenie.report_templates where id=p_entity_id::uuid),'authoritative',timegenie.cloud_incremental_entity_get(p_workspace_id,'report_templates',p_entity_id)->'data');
  perform timegenie.record_processed_operation(p_workspace_id,p_operation_id,p_device_id,p_operation_type,p_request,result_value);
  return result_value;
end $$;

create or replace function timegenie.cloud_apply_patch(p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_operation_type text,p_entity_type text,p_entity_id text,p_base_version bigint,p_payload jsonb)
returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$ begin
  if p_entity_type='report_template' then
    return timegenie.apply_named_report_template(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_id,p_base_version,p_payload,jsonb_build_object('deviceId',p_device_id,'entityType',p_entity_type,'entityId',p_entity_id,'baseVersion',p_base_version,'payload',coalesce(p_payload,'{}'::jsonb)));
  end if;
  if p_entity_type='unassigned_session' and p_operation_type in ('unassigned_session_pause','unassigned_session_resume','unassigned_session_awaiting_resolution') then return timegenie.cloud_apply_simplified_unassigned(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_id,p_base_version,p_payload); end if;
  if p_entity_type='time_entry' and coalesce(p_payload->>'source_type','')='timer' then return timegenie.apply_calendar_time_entry(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_id,p_base_version,p_payload,jsonb_build_object('deviceId',p_device_id,'entityType',p_entity_type,'entityId',p_entity_id,'baseVersion',p_base_version,'payload',coalesce(p_payload,'{}'::jsonb))); end if;
  return timegenie.cloud_apply_patch_pre_calendar(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_type,p_entity_id,p_base_version,p_payload);
end $$;

create or replace function timegenie.cloud_apply_patch(p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_operation_type text,p_entity_type text,p_entity_id text,p_base_version bigint,p_payload_version bigint,p_coalesced_count bigint,p_payload jsonb)
returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$ begin
  if p_payload_version is null or (p_payload->>'version')::bigint<>p_payload_version then raise exception 'VERSION_CONFLICT: declared payload version mismatch'; end if;
  if p_coalesced_count is null or p_coalesced_count<1 then raise exception 'VERSION_CONFLICT: coalesced operation count must be positive'; end if;
  if p_entity_type='report_template' then
    return timegenie.apply_named_report_template(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_id,p_base_version,p_payload,jsonb_build_object('deviceId',p_device_id,'entityType',p_entity_type,'entityId',p_entity_id,'baseVersion',p_base_version,'payloadVersion',p_payload_version,'coalescedCount',p_coalesced_count,'payload',coalesce(p_payload,'{}'::jsonb)));
  end if;
  if p_entity_type='unassigned_session' and p_operation_type in ('unassigned_session_pause','unassigned_session_resume','unassigned_session_awaiting_resolution') then return timegenie.cloud_apply_simplified_unassigned(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_id,p_base_version,p_payload); end if;
  if p_entity_type='time_entry' and coalesce(p_payload->>'source_type','')='timer' then return timegenie.apply_calendar_time_entry(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_id,p_base_version,p_payload,jsonb_build_object('deviceId',p_device_id,'entityType',p_entity_type,'entityId',p_entity_id,'baseVersion',p_base_version,'payloadVersion',p_payload_version,'coalescedCount',p_coalesced_count,'payload',coalesce(p_payload,'{}'::jsonb))); end if;
  return timegenie.cloud_apply_patch_pre_calendar(p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_type,p_entity_id,p_base_version,p_payload_version,p_coalesced_count,p_payload);
end $$;

create or replace function timegenie.migration_import_snapshot(p_workspace_id uuid,p_operation_id uuid,p_device_id uuid,p_snapshot jsonb)
returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare result_value jsonb; row_data jsonb; target_id uuid; target_type text; sanitized jsonb;
begin
  sanitized:=jsonb_set(coalesce(p_snapshot,'{}'::jsonb),'{report_templates}','[]'::jsonb,true);
  result_value:=timegenie.migration_import_snapshot_pre_calendar(p_workspace_id,p_operation_id,p_device_id,sanitized);
  for row_data in select value from jsonb_array_elements(coalesce(p_snapshot->'report_templates','[]'::jsonb)) loop
    insert into timegenie.report_templates(id,workspace_id,report_type,subject_id,name,description,content,is_default,is_builtin,sort_order,created_at,updated_at,version,deleted_at)
    values((row_data->>'id')::uuid,p_workspace_id,row_data->>'report_type',null,coalesce(nullif(btrim(row_data->>'name'),''),'迁移模板 - '||left(row_data->>'id',8)),nullif(row_data->>'description',''),row_data->>'content',false,coalesce((row_data->>'is_builtin')::boolean,false),coalesce((row_data->>'sort_order')::integer,0),to_timestamp((row_data->>'created_at')::double precision/1000),to_timestamp((row_data->>'updated_at')::double precision/1000),coalesce((row_data->>'version')::bigint,1),case when row_data->>'deleted_at' is null then null else to_timestamp((row_data->>'deleted_at')::double precision/1000) end)
    on conflict(id) do update set report_type=excluded.report_type,subject_id=null,name=excluded.name,description=excluded.description,content=excluded.content,is_default=false,is_builtin=excluded.is_builtin,sort_order=excluded.sort_order,updated_at=excluded.updated_at,version=greatest(timegenie.report_templates.version,excluded.version),deleted_at=excluded.deleted_at;
  end loop;
  for target_type in select unnest(array['daily','weekly','monthly']) loop
    select (row_data->>'id')::uuid into target_id from jsonb_array_elements(coalesce(p_snapshot->'report_templates','[]'::jsonb)) row_data where row_data->>'report_type'=target_type and coalesce((row_data->>'is_default')::boolean,false) and row_data->>'deleted_at' is null order by row_data->>'id' limit 1;
    if target_id is null then select id into target_id from timegenie.report_templates where workspace_id=p_workspace_id and report_type=target_type and deleted_at is null order by sort_order,created_at,id limit 1; end if;
    update timegenie.report_templates set is_default=(id=target_id) where workspace_id=p_workspace_id and report_type=target_type and deleted_at is null;
    target_id:=null;
  end loop;
  return result_value;
end $$;

revoke execute on function timegenie.apply_named_report_template(uuid,uuid,uuid,text,text,bigint,jsonb,jsonb) from public,anon,authenticated;
grant execute on function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,jsonb) to authenticated;
grant execute on function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,bigint,bigint,jsonb) to authenticated;
grant execute on function timegenie.migration_import_snapshot(uuid,uuid,uuid,jsonb) to authenticated;
grant execute on function timegenie.set_report_template_default(uuid,uuid,bigint) to authenticated;

commit;
notify pgrst, 'reload schema';
