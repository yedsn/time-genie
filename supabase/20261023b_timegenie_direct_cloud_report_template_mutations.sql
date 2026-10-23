-- Direct-v2 report-template lifecycle with unique defaults, expected versions
-- and idempotent operation results.

begin;

create or replace function timegenie.direct_cloud_report_template_validate(
  p_report_type text,p_name text,p_content text
) returns void language plpgsql immutable
set search_path=timegenie,extensions,pg_catalog as $$
declare token_value text; allowed_tokens text[];
begin
  if p_report_type not in('daily','weekly','monthly') then raise exception 'VALIDATION_ERROR: 非法报告类型'; end if;
  if btrim(coalesce(p_name,''))='' then raise exception 'VALIDATION_ERROR: 模板名称不能为空'; end if;
  if length(btrim(p_name))>80 then raise exception 'VALIDATION_ERROR: 模板名称不能超过 80 个字符'; end if;
  if btrim(coalesce(p_content,''))='' then raise exception 'VALIDATION_ERROR: 报告模板不能为空'; end if;
  allowed_tokens:=case p_report_type
    when 'daily' then array['{{姓名}}','{{日期}}','{{星期}}','{{工作时段}}','{{总工时}}','{{今日事项}}','{{明日计划}}']
    else array['{{姓名}}','{{日期范围}}','{{总工时}}','{{每日情况}}','{{日报合集}}','{{统计信息}}','{{总结}}'] end;
  if regexp_replace(p_content,'\{\{[^{}]*\}\}','','g') like '%{{%' then
    raise exception 'VALIDATION_ERROR: 模板占位符未闭合';
  end if;
  for token_value in select (regexp_matches(p_content,'\{\{[^{}]*\}\}','g'))[1] loop
    if token_value<>all(allowed_tokens) then
      if token_value=any(array['{{姓名}}','{{日期}}','{{星期}}','{{日期范围}}','{{工作时段}}','{{总工时}}','{{今日事项}}','{{明日计划}}','{{每日情况}}','{{日报合集}}','{{统计信息}}','{{总结}}']) then
        raise exception 'VALIDATION_ERROR: 占位符 % 不适用于当前报告类型',token_value;
      end if;
      raise exception 'VALIDATION_ERROR: 未知占位符 %',token_value;
    end if;
  end loop;
end $$;

create or replace function timegenie.direct_cloud_report_template_record(
  p_workspace_id uuid,p_template_id uuid,p_include_deleted boolean default false
) returns jsonb language sql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
  select jsonb_build_object(
    'id',template.id,'reportType',template.report_type,'name',template.name,'description',template.description,
    'content',template.content,'isDefault',template.is_default,'isBuiltin',template.is_builtin,
    'sortOrder',template.sort_order,'version',template.version,'deletedAt',template.deleted_at
  ) from timegenie.report_templates template
  where template.workspace_id=p_workspace_id and template.id=p_template_id
    and (p_include_deleted or template.deleted_at is null);
$$;

create or replace function timegenie.direct_cloud_report_template_create(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; after_value jsonb;
declare template_id uuid; report_type_value text; name_value text; description_value text; content_value text; sort_value integer;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_report_template_create',request_value); if cached is not null then return cached; end if;
  report_type_value:=coalesce(p_payload->>'reportType',''); name_value:=btrim(coalesce(p_payload->>'name',''));
  description_value:=nullif(btrim(p_payload->>'description'),''); content_value:=coalesce(p_payload->>'content','');
  perform timegenie.direct_cloud_report_template_validate(report_type_value,name_value,content_value);
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':report-template:'||report_type_value,0));
  if exists(select 1 from timegenie.report_templates template where template.workspace_id=p_workspace_id
    and template.report_type=report_type_value and template.deleted_at is null and lower(btrim(template.name))=lower(name_value)) then
    raise exception 'VALIDATION_ERROR: 同类型模板名称已存在';
  end if;
  template_id:=coalesce(nullif(p_payload->>'templateId','')::uuid,timegenie.calendar_stable_uuid('direct-report-template:'||p_operation_id::text));
  if exists(select 1 from timegenie.report_templates template where template.workspace_id=p_workspace_id and template.id=template_id) then raise exception 'VERSION_CONFLICT: 模板标识已存在'; end if;
  select coalesce(max(template.sort_order),0)+10 into sort_value from timegenie.report_templates template
  where template.workspace_id=p_workspace_id and template.report_type=report_type_value and template.deleted_at is null;
  insert into timegenie.report_templates(id,workspace_id,report_type,subject_id,name,description,content,is_default,is_builtin,sort_order,created_at,updated_at,version)
  values(template_id,p_workspace_id,report_type_value,null,name_value,description_value,content_value,false,false,sort_value,now(),now(),1);
  after_value:=(select to_jsonb(template) from timegenie.report_templates template where template.id=template_id);
  result_value:=timegenie.direct_cloud_mutation_response(p_workspace_id,p_device_id,p_operation_id,
    jsonb_build_object('template',timegenie.direct_cloud_report_template_record(p_workspace_id,template_id,false)),array['reports'],p_expected_epoch);
  perform timegenie.record_direct_cloud_operation(p_workspace_id,p_operation_id,p_device_id,'direct_report_template_create',request_value,result_value);
  perform timegenie.operation_history_replace_item(p_workspace_id,p_operation_id,'report_template',template_id::text,null,after_value);
  return result_value;
exception when invalid_text_representation then raise exception 'VALIDATION_ERROR: 模板参数格式无效';
end $$;

create or replace function timegenie.direct_cloud_report_template_duplicate(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; after_value jsonb; source_row timegenie.report_templates%rowtype;
declare source_id uuid; template_id uuid; source_report_type text; name_value text; description_value text; sort_value integer;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_report_template_duplicate',request_value); if cached is not null then return cached; end if;
  source_id:=nullif(p_payload->>'templateId','')::uuid; name_value:=btrim(coalesce(p_payload->>'name',''));
  select template.report_type into source_report_type from timegenie.report_templates template
  where template.workspace_id=p_workspace_id and template.id=source_id;
  if not found then raise exception 'NOT_FOUND: 报告模板不存在'; end if;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':report-template:'||source_report_type,0));
  select template.* into source_row from timegenie.report_templates template where template.workspace_id=p_workspace_id and template.id=source_id for update;
  if not found then raise exception 'NOT_FOUND: 报告模板不存在'; end if;
  if source_row.deleted_at is not null then raise exception 'ENTITY_DELETED: 报告模板已删除'; end if;
  description_value:=case when p_payload ? 'description' then nullif(btrim(p_payload->>'description'),'') else source_row.description end;
  perform timegenie.direct_cloud_report_template_validate(source_row.report_type,name_value,source_row.content);
  if exists(select 1 from timegenie.report_templates template where template.workspace_id=p_workspace_id and template.report_type=source_row.report_type and template.deleted_at is null and lower(btrim(template.name))=lower(name_value)) then raise exception 'VALIDATION_ERROR: 同类型模板名称已存在'; end if;
  template_id:=coalesce(nullif(p_payload->>'newTemplateId','')::uuid,timegenie.calendar_stable_uuid('direct-report-template-duplicate:'||p_operation_id::text));
  select coalesce(max(template.sort_order),0)+10 into sort_value from timegenie.report_templates template where template.workspace_id=p_workspace_id and template.report_type=source_row.report_type and template.deleted_at is null;
  insert into timegenie.report_templates(id,workspace_id,report_type,subject_id,name,description,content,is_default,is_builtin,sort_order,created_at,updated_at,version)
  values(template_id,p_workspace_id,source_row.report_type,null,name_value,description_value,source_row.content,false,false,sort_value,now(),now(),1);
  after_value:=(select to_jsonb(template) from timegenie.report_templates template where template.id=template_id);
  result_value:=timegenie.direct_cloud_mutation_response(p_workspace_id,p_device_id,p_operation_id,jsonb_build_object(
    'template',timegenie.direct_cloud_report_template_record(p_workspace_id,template_id,false),'sourceTemplateId',source_id),array['reports'],p_expected_epoch);
  perform timegenie.record_direct_cloud_operation(p_workspace_id,p_operation_id,p_device_id,'direct_report_template_duplicate',request_value,result_value);
  perform timegenie.operation_history_replace_item(p_workspace_id,p_operation_id,'report_template',template_id::text,null,after_value);
  return result_value;
exception when invalid_text_representation then raise exception 'VALIDATION_ERROR: 模板参数格式无效';
end $$;

create or replace function timegenie.direct_cloud_report_template_update(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; before_value jsonb; after_value jsonb; template_row timegenie.report_templates%rowtype;
declare template_id uuid; expected_version bigint; report_type_value text; name_value text; description_value text; content_value text; sort_value integer;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_report_template_update',request_value); if cached is not null then return cached; end if;
  template_id:=nullif(p_payload->>'templateId','')::uuid; expected_version:=nullif(p_payload->>'expectedVersion','')::bigint;
  name_value:=btrim(coalesce(p_payload->>'name','')); description_value:=nullif(btrim(p_payload->>'description'),'');
  content_value:=coalesce(p_payload->>'content',''); sort_value:=nullif(p_payload->>'sortOrder','')::integer;
  if expected_version is null then raise exception 'VALIDATION_ERROR: 模板版本不能为空'; end if;
  select template.report_type into report_type_value from timegenie.report_templates template
  where template.workspace_id=p_workspace_id and template.id=template_id;
  if not found then raise exception 'NOT_FOUND: 报告模板不存在'; end if;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':report-template:'||report_type_value,0));
  select template.* into template_row from timegenie.report_templates template where template.workspace_id=p_workspace_id and template.id=template_id for update;
  if not found then raise exception 'NOT_FOUND: 报告模板不存在'; end if;
  if template_row.deleted_at is not null then raise exception 'ENTITY_DELETED: 报告模板已删除'; end if;
  if template_row.version<>expected_version then raise exception 'VERSION_CONFLICT: 报告模板已变化'; end if;
  if sort_value is null then raise exception 'VALIDATION_ERROR: 模板排序值不能为空'; end if;
  perform timegenie.direct_cloud_report_template_validate(template_row.report_type,name_value,content_value);
  if exists(select 1 from timegenie.report_templates template where template.workspace_id=p_workspace_id and template.report_type=template_row.report_type and template.id<>template_id and template.deleted_at is null and lower(btrim(template.name))=lower(name_value)) then raise exception 'VALIDATION_ERROR: 同类型模板名称已存在'; end if;
  before_value:=to_jsonb(template_row);
  update timegenie.report_templates template set name=name_value,description=description_value,content=content_value,sort_order=sort_value,updated_at=now(),version=template.version+1 where template.id=template_id;
  after_value:=(select to_jsonb(template) from timegenie.report_templates template where template.id=template_id);
  result_value:=timegenie.direct_cloud_mutation_response(p_workspace_id,p_device_id,p_operation_id,jsonb_build_object('template',timegenie.direct_cloud_report_template_record(p_workspace_id,template_id,false)),array['reports'],p_expected_epoch);
  perform timegenie.record_direct_cloud_operation(p_workspace_id,p_operation_id,p_device_id,'direct_report_template_update',request_value,result_value);
  perform timegenie.operation_history_replace_item(p_workspace_id,p_operation_id,'report_template',template_id::text,before_value,after_value);
  return result_value;
exception when invalid_text_representation then raise exception 'VALIDATION_ERROR: 模板参数格式无效';
end $$;

create or replace function timegenie.direct_cloud_report_template_set_default(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; before_value jsonb; after_value jsonb; template_row timegenie.report_templates%rowtype;
declare template_id uuid; expected_version bigint; report_type_value text;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_report_template_set_default',request_value); if cached is not null then return cached; end if;
  template_id:=nullif(p_payload->>'templateId','')::uuid; expected_version:=nullif(p_payload->>'expectedVersion','')::bigint;
  if expected_version is null then raise exception 'VALIDATION_ERROR: 模板版本不能为空'; end if;
  select template.report_type into report_type_value from timegenie.report_templates template
  where template.workspace_id=p_workspace_id and template.id=template_id;
  if not found then raise exception 'NOT_FOUND: 报告模板不存在'; end if;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':report-template:'||report_type_value,0));
  select template.* into template_row from timegenie.report_templates template where template.workspace_id=p_workspace_id and template.id=template_id for update;
  if not found then raise exception 'NOT_FOUND: 报告模板不存在'; end if;
  if template_row.deleted_at is not null then raise exception 'ENTITY_DELETED: 报告模板已删除'; end if;
  if template_row.version<>expected_version then raise exception 'VERSION_CONFLICT: 报告模板已变化'; end if;
  before_value:=to_jsonb(template_row);
  update timegenie.report_templates template set is_default=false,updated_at=now(),version=template.version+1
  where template.workspace_id=p_workspace_id and template.report_type=template_row.report_type and template.is_default
    and template.id<>template_id and template.deleted_at is null;
  update timegenie.report_templates template set is_default=true,updated_at=now(),version=template.version+1 where template.id=template_id;
  after_value:=(select to_jsonb(template) from timegenie.report_templates template where template.id=template_id);
  result_value:=timegenie.direct_cloud_mutation_response(p_workspace_id,p_device_id,p_operation_id,jsonb_build_object('template',timegenie.direct_cloud_report_template_record(p_workspace_id,template_id,false)),array['reports'],p_expected_epoch);
  perform timegenie.record_direct_cloud_operation(p_workspace_id,p_operation_id,p_device_id,'direct_report_template_set_default',request_value,result_value);
  perform timegenie.operation_history_replace_item(p_workspace_id,p_operation_id,'report_template',template_id::text,before_value,after_value);
  return result_value;
exception when invalid_text_representation then raise exception 'VALIDATION_ERROR: 模板参数格式无效';
end $$;

create or replace function timegenie.direct_cloud_report_template_reorder(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; item jsonb; report_type_value text;
declare template_id uuid; expected_version bigint; item_count integer; active_count integer; seen_ids uuid[]:='{}'::uuid[]; index_value integer:=0;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_report_template_reorder',request_value); if cached is not null then return cached; end if;
  report_type_value:=coalesce(p_payload->>'reportType','');
  if report_type_value not in('daily','weekly','monthly') then raise exception 'VALIDATION_ERROR: 非法报告类型'; end if;
  if jsonb_typeof(coalesce(p_payload->'items','null'::jsonb))<>'array' then raise exception 'VALIDATION_ERROR: 模板排序列表无效'; end if;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':report-template:'||report_type_value,0));
  perform 1 from timegenie.report_templates template where template.workspace_id=p_workspace_id and template.report_type=report_type_value and template.deleted_at is null for update;
  select count(*) into active_count from timegenie.report_templates template where template.workspace_id=p_workspace_id and template.report_type=report_type_value and template.deleted_at is null;
  item_count:=jsonb_array_length(p_payload->'items');
  if item_count<>active_count then raise exception 'VALIDATION_ERROR: 模板排序列表不完整'; end if;
  for item in select value from jsonb_array_elements(p_payload->'items') loop
    index_value:=index_value+1; template_id:=nullif(item->>'templateId','')::uuid; expected_version:=nullif(item->>'expectedVersion','')::bigint;
    if template_id is null or expected_version is null or template_id=any(seen_ids) then raise exception 'VALIDATION_ERROR: 模板排序标识或版本无效'; end if;
    seen_ids:=array_append(seen_ids,template_id);
    if not exists(select 1 from timegenie.report_templates template where template.workspace_id=p_workspace_id and template.report_type=report_type_value and template.id=template_id and template.deleted_at is null) then raise exception 'VALIDATION_ERROR: 模板排序列表不完整'; end if;
    if (select template.version from timegenie.report_templates template where template.id=template_id)<>expected_version then raise exception 'VERSION_CONFLICT: 报告模板排序已变化'; end if;
    update timegenie.report_templates template set sort_order=index_value*10,updated_at=now(),version=template.version+1 where template.id=template_id;
  end loop;
  result_value:=timegenie.direct_cloud_mutation_response(p_workspace_id,p_device_id,p_operation_id,jsonb_build_object(
    'templates',coalesce((select jsonb_agg(timegenie.direct_cloud_report_template_record(p_workspace_id,template.id,false) order by template.sort_order,lower(template.name),template.id) from timegenie.report_templates template where template.workspace_id=p_workspace_id and template.report_type=report_type_value and template.deleted_at is null),'[]'::jsonb)
  ),array['reports'],p_expected_epoch);
  perform timegenie.record_direct_cloud_operation(p_workspace_id,p_operation_id,p_device_id,'direct_report_template_reorder',request_value,result_value);
  return result_value;
exception when invalid_text_representation then raise exception 'VALIDATION_ERROR: 模板参数格式无效';
end $$;

create or replace function timegenie.direct_cloud_report_template_delete(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; before_value jsonb; after_value jsonb; template_row timegenie.report_templates%rowtype; replacement_row timegenie.report_templates%rowtype;
declare template_id uuid; replacement_id uuid; expected_version bigint; report_type_value text; active_count integer;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_report_template_delete',request_value); if cached is not null then return cached; end if;
  template_id:=nullif(p_payload->>'templateId','')::uuid; replacement_id:=nullif(p_payload->>'replacementTemplateId','')::uuid; expected_version:=nullif(p_payload->>'expectedVersion','')::bigint;
  if expected_version is null then raise exception 'VALIDATION_ERROR: 模板版本不能为空'; end if;
  select template.report_type into report_type_value from timegenie.report_templates template
  where template.workspace_id=p_workspace_id and template.id=template_id;
  if not found then raise exception 'NOT_FOUND: 报告模板不存在'; end if;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':report-template:'||report_type_value,0));
  select template.* into template_row from timegenie.report_templates template where template.workspace_id=p_workspace_id and template.id=template_id for update;
  if not found then raise exception 'NOT_FOUND: 报告模板不存在'; end if;
  if template_row.deleted_at is not null then raise exception 'ENTITY_DELETED: 报告模板已删除'; end if;
  if template_row.version<>expected_version then raise exception 'VERSION_CONFLICT: 报告模板已变化'; end if;
  select count(*) into active_count from timegenie.report_templates template where template.workspace_id=p_workspace_id and template.report_type=template_row.report_type and template.deleted_at is null;
  if active_count<=1 then raise exception 'VALIDATION_ERROR: 每种报告类型至少需要保留一份模板'; end if;
  if template_row.is_default then
    if replacement_id is null or replacement_id=template_id then raise exception 'VALIDATION_ERROR: 删除默认模板时请选择替代默认模板'; end if;
    select template.* into replacement_row from timegenie.report_templates template where template.workspace_id=p_workspace_id and template.id=replacement_id for update;
    if not found or replacement_row.deleted_at is not null or replacement_row.report_type<>template_row.report_type then raise exception 'VALIDATION_ERROR: 替代默认模板无效'; end if;
  end if;
  before_value:=to_jsonb(template_row);
  update timegenie.report_templates template set is_default=false,deleted_at=now(),updated_at=now(),version=template.version+1 where template.id=template_id;
  if template_row.is_default then update timegenie.report_templates template set is_default=true,updated_at=now(),version=template.version+1 where template.id=replacement_id; end if;
  after_value:=(select to_jsonb(template) from timegenie.report_templates template where template.id=template_id);
  result_value:=timegenie.direct_cloud_mutation_response(p_workspace_id,p_device_id,p_operation_id,jsonb_build_object(
    'template',timegenie.direct_cloud_report_template_record(p_workspace_id,template_id,true),
    'replacementTemplate',case when replacement_id is null then null else timegenie.direct_cloud_report_template_record(p_workspace_id,replacement_id,false) end
  ),array['reports'],p_expected_epoch);
  perform timegenie.record_direct_cloud_operation(p_workspace_id,p_operation_id,p_device_id,'direct_report_template_delete',request_value,result_value);
  perform timegenie.operation_history_replace_item(p_workspace_id,p_operation_id,'report_template',template_id::text,before_value,after_value);
  return result_value;
exception when invalid_text_representation then raise exception 'VALIDATION_ERROR: 模板参数格式无效';
end $$;

revoke all on function timegenie.direct_cloud_report_template_validate(text,text,text) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_report_template_record(uuid,uuid,boolean) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_report_template_create(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_report_template_duplicate(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_report_template_update(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_report_template_set_default(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_report_template_reorder(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_report_template_delete(uuid,uuid,uuid,bigint,jsonb) from public,anon;
grant execute on function timegenie.direct_cloud_report_template_create(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_report_template_duplicate(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_report_template_update(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_report_template_set_default(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_report_template_reorder(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_report_template_delete(uuid,uuid,uuid,bigint,jsonb) to authenticated;

commit;

notify pgrst,'reload schema';
