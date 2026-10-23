-- Direct-v2 integration metadata reads and writes.
-- Credentials and device-local paths never enter these cloud contracts.

begin;

alter table timegenie.external_bindings
  add column if not exists version bigint not null default 1;

create or replace function timegenie.direct_cloud_integration_reject_sensitive(
  p_value jsonb,p_path text default '$'
) returns void language plpgsql immutable
set search_path=timegenie,extensions,pg_catalog as $$
declare pair record; item jsonb; index_value integer:=0; normalized text;
begin
  if p_value is null then return; end if;
  if jsonb_typeof(p_value)='object' then
    for pair in select key,value from jsonb_each(p_value) loop
      normalized:=regexp_replace(lower(pair.key),'[^a-z0-9]+','','g');
      if normalized ~ '(token|password|secret|authorization|apikey|managementkey|servicerole|privatekey)'
         or normalized=any(array['anonkey','leasetoken','authsessionid','lastsyncedhash']) then
        raise exception 'VALIDATION_ERROR: 集成凭据字段 % 必须保存在系统凭据库',p_path||'.'||pair.key;
      end if;
      if normalized=any(array['vaultpath','vaultroot','vaultrootpath','rootpath','localpath','filepath','absolutepath']) then
        raise exception 'VALIDATION_ERROR: 本机路径 % 必须保存在设备设置',p_path||'.'||pair.key;
      end if;
      perform timegenie.direct_cloud_integration_reject_sensitive(pair.value,p_path||'.'||pair.key);
    end loop;
  elsif jsonb_typeof(p_value)='array' then
    for item in select value from jsonb_array_elements(p_value) loop
      index_value:=index_value+1;
      perform timegenie.direct_cloud_integration_reject_sensitive(item,p_path||'['||index_value||']');
    end loop;
  end if;
end $$;

create or replace function timegenie.direct_cloud_integration_provider_assert(p_provider text)
returns text language plpgsql immutable
set search_path=timegenie,extensions,pg_catalog as $$
declare provider_value text:=lower(btrim(coalesce(p_provider,'')));
begin
  if provider_value not in('seatable','obsidian') then
    raise exception 'VALIDATION_ERROR: 不支持的共享集成 %',coalesce(p_provider,'');
  end if;
  return provider_value;
end $$;

create or replace function timegenie.direct_cloud_integration_record(
  p_workspace_id uuid,p_provider text
) returns jsonb language sql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
  select jsonb_build_object(
    'provider',config.provider,'enabled',config.enabled,
    'config',timegenie.direct_cloud_sanitize_shared_json(config.config_json),
    'version',config.version,'updatedAt',config.updated_at,
    'requiresDeviceCredential',config.provider='seatable'
  )
  from timegenie.integration_configs config
  where config.workspace_id=p_workspace_id and config.provider=p_provider;
$$;

create or replace function timegenie.direct_cloud_integration_config_save(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; provider_value text; enabled_value boolean;
declare config_value jsonb; expected_version bigint; before_value jsonb; after_value jsonb; changed_value boolean:=false;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_integration_config_save',request_value);
  if cached is not null then return cached; end if;
  provider_value:=timegenie.direct_cloud_integration_provider_assert(p_payload->>'provider');
  if jsonb_typeof(coalesce(p_payload->'enabled','null'::jsonb))<>'boolean' then
    raise exception 'VALIDATION_ERROR: 集成启用状态无效';
  end if;
  enabled_value:=(p_payload->>'enabled')::boolean;
  config_value:=coalesce(p_payload->'config','{}'::jsonb);
  if jsonb_typeof(config_value)<>'object' then raise exception 'VALIDATION_ERROR: 集成配置必须是对象'; end if;
  if octet_length(config_value::text)>32768 then raise exception 'VALIDATION_ERROR: 集成配置过大'; end if;
  perform timegenie.direct_cloud_integration_reject_sensitive(config_value,'$');
  expected_version:=case when p_payload ? 'expectedVersion' then nullif(p_payload->>'expectedVersion','')::bigint else null end;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':integration-config:'||provider_value,0));
  select to_jsonb(config) into before_value from timegenie.integration_configs config
  where config.workspace_id=p_workspace_id and config.provider=provider_value for update;
  if before_value is null then
    if expected_version is not null and expected_version<>0 then raise exception 'VERSION_CONFLICT: 集成配置已变化'; end if;
    insert into timegenie.integration_configs(workspace_id,provider,enabled,config_json,updated_at,version)
    values(p_workspace_id,provider_value,enabled_value,config_value,now(),1);
    changed_value:=true;
  else
    if expected_version is null or (before_value->>'version')::bigint<>expected_version then
      raise exception 'VERSION_CONFLICT: 集成配置已变化';
    end if;
    if (before_value->>'enabled')::boolean=enabled_value and before_value->'config_json'=config_value then
      return timegenie.direct_cloud_mutation_response(p_workspace_id,p_device_id,p_operation_id,
        jsonb_build_object('changed',false,'integration',timegenie.direct_cloud_integration_record(p_workspace_id,provider_value)),
        array['settings','integrations'],p_expected_epoch);
    end if;
    update timegenie.integration_configs config
    set enabled=enabled_value,config_json=config_value,updated_at=now(),version=config.version+1
    where config.workspace_id=p_workspace_id and config.provider=provider_value and config.version=expected_version;
    if not found then raise exception 'VERSION_CONFLICT: 集成配置已变化'; end if;
    changed_value:=true;
  end if;
  select to_jsonb(config) into after_value from timegenie.integration_configs config
  where config.workspace_id=p_workspace_id and config.provider=provider_value;
  result_value:=timegenie.direct_cloud_mutation_response(p_workspace_id,p_device_id,p_operation_id,
    jsonb_build_object('changed',changed_value,'integration',timegenie.direct_cloud_integration_record(p_workspace_id,provider_value)),
    array['settings','integrations'],p_expected_epoch);
  perform timegenie.record_direct_cloud_operation(p_workspace_id,p_operation_id,p_device_id,'direct_integration_config_save',request_value,result_value);
  perform timegenie.operation_history_replace_item(p_workspace_id,p_operation_id,'integration_config',provider_value,before_value,after_value);
  return result_value;
exception when invalid_text_representation then raise exception 'VALIDATION_ERROR: 集成配置参数格式无效'; end $$;

create or replace function timegenie.direct_cloud_external_binding_record(
  p_workspace_id uuid,p_provider text,p_entity_type text,p_entity_id uuid
) returns jsonb language sql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
  select jsonb_strip_nulls(jsonb_build_object(
    'id',binding.id,'provider',binding.provider,'entityType',binding.entity_type,'entityId',binding.entity_id,
    'externalId',binding.external_id,'externalRevision',binding.external_revision,
    'lastSyncedAt',case when binding.last_synced_at is null then null else (extract(epoch from binding.last_synced_at)*1000)::bigint end,
    'version',binding.version
  ))
  from timegenie.external_bindings binding
  where binding.workspace_id=p_workspace_id and binding.provider=p_provider
    and binding.entity_type=p_entity_type and binding.entity_id=p_entity_id;
$$;

create or replace function timegenie.direct_cloud_integration_state(
  p_workspace_id uuid,p_device_id uuid,p_expected_epoch bigint default null
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare data_value jsonb;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  data_value:=jsonb_build_object(
    'integrations',coalesce((select jsonb_agg(timegenie.direct_cloud_integration_record(p_workspace_id,config.provider) order by config.provider)
      from timegenie.integration_configs config where config.workspace_id=p_workspace_id),'[]'::jsonb),
    'bindings',coalesce((select jsonb_agg(timegenie.direct_cloud_external_binding_record(p_workspace_id,binding.provider,binding.entity_type,binding.entity_id)
      order by binding.provider,binding.entity_type,binding.entity_id,binding.id)
      from timegenie.external_bindings binding where binding.workspace_id=p_workspace_id),'[]'::jsonb)
  );
  return timegenie.direct_cloud_query_response(p_workspace_id,p_device_id,data_value,array['settings','integrations'],p_expected_epoch);
end $$;

create or replace function timegenie.direct_cloud_external_binding_upsert(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; provider_value text; entity_type_value text; entity_id_value uuid;
declare external_id_value text; revision_value text; synced_at_value timestamptz; expected_version bigint; binding_id uuid;
declare before_value jsonb; after_value jsonb; changed_value boolean:=false;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_external_binding_upsert',request_value);
  if cached is not null then return cached; end if;
  provider_value:=timegenie.direct_cloud_integration_provider_assert(p_payload->>'provider');
  entity_type_value:=lower(btrim(coalesce(p_payload->>'entityType','')));
  if entity_type_value not in('task','subject','report') then raise exception 'VALIDATION_ERROR: 不支持的外部绑定对象类型 %',coalesce(entity_type_value,''); end if;
  entity_id_value:=nullif(p_payload->>'entityId','')::uuid;
  if entity_id_value is null then raise exception 'VALIDATION_ERROR: 外部绑定对象不能为空'; end if;
  external_id_value:=btrim(coalesce(p_payload->>'externalId',''));
  if external_id_value='' or length(external_id_value)>512 then raise exception 'VALIDATION_ERROR: 外部记录标识无效'; end if;
  revision_value:=nullif(btrim(coalesce(p_payload->>'externalRevision','')),'');
  if revision_value is not null and length(revision_value)>512 then raise exception 'VALIDATION_ERROR: 外部版本标识无效'; end if;
  if p_payload ? 'lastSyncedAt' and p_payload->'lastSyncedAt'<>'null'::jsonb then
    synced_at_value:=to_timestamp((p_payload->>'lastSyncedAt')::double precision/1000);
    if synced_at_value<'2000-01-01'::timestamptz or synced_at_value>'2100-01-01'::timestamptz then
      raise exception 'VALIDATION_ERROR: 外部同步时间无效';
    end if;
  end if;
  if entity_type_value='task' and not exists(select 1 from timegenie.tasks target
      where target.workspace_id=p_workspace_id and target.id=entity_id_value and target.deleted_at is null) then
    raise exception 'NOT_FOUND: 外部绑定事项不存在';
  elsif entity_type_value='subject' and not exists(select 1 from timegenie.subjects target
      where target.workspace_id=p_workspace_id and target.id=entity_id_value and target.deleted_at is null) then
    raise exception 'NOT_FOUND: 外部绑定主体不存在';
  elsif entity_type_value='report' and not exists(select 1 from timegenie.reports target
      where target.workspace_id=p_workspace_id and target.id=entity_id_value and target.deleted_at is null) then
    raise exception 'NOT_FOUND: 外部绑定报告不存在';
  end if;
  expected_version:=case when p_payload ? 'expectedVersion' then nullif(p_payload->>'expectedVersion','')::bigint else null end;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':external-binding:'||provider_value||':'||entity_type_value||':'||entity_id_value::text,0));
  select to_jsonb(binding) into before_value from timegenie.external_bindings binding
  where binding.workspace_id=p_workspace_id and binding.provider=provider_value
    and binding.entity_type=entity_type_value and binding.entity_id=entity_id_value for update;
  if exists(select 1 from timegenie.external_bindings binding where binding.workspace_id=p_workspace_id
      and binding.provider=provider_value and binding.external_id=external_id_value
      and not(binding.entity_type=entity_type_value and binding.entity_id=entity_id_value)) then
    raise exception 'VALIDATION_ERROR: 外部记录已绑定到其他对象';
  end if;
  if before_value is null then
    if expected_version is not null and expected_version<>0 then raise exception 'VERSION_CONFLICT: 外部绑定已变化'; end if;
    binding_id:=coalesce(nullif(p_payload->>'id','')::uuid,gen_random_uuid());
    if exists(select 1 from timegenie.external_bindings binding where binding.id=binding_id) then
      raise exception 'VERSION_CONFLICT: 外部绑定标识已存在';
    end if;
    insert into timegenie.external_bindings(id,workspace_id,provider,entity_type,entity_id,external_id,external_revision,last_synced_hash,last_synced_at,version)
    values(binding_id,p_workspace_id,provider_value,entity_type_value,entity_id_value,external_id_value,revision_value,null,synced_at_value,1);
    changed_value:=true;
  else
    if expected_version is null or (before_value->>'version')::bigint<>expected_version then raise exception 'VERSION_CONFLICT: 外部绑定已变化'; end if;
    if before_value->>'external_id'=external_id_value and coalesce(before_value->>'external_revision','')=coalesce(revision_value,'')
       and coalesce(before_value->>'last_synced_at','')=coalesce(synced_at_value::text,'') then
      return timegenie.direct_cloud_mutation_response(p_workspace_id,p_device_id,p_operation_id,
        jsonb_build_object('changed',false,'binding',timegenie.direct_cloud_external_binding_record(p_workspace_id,provider_value,entity_type_value,entity_id_value)),
        array['integrations'],p_expected_epoch);
    end if;
    binding_id:=(before_value->>'id')::uuid;
    update timegenie.external_bindings binding
    set external_id=external_id_value,external_revision=revision_value,last_synced_hash=null,
      last_synced_at=synced_at_value,version=binding.version+1
    where binding.id=binding_id and binding.version=expected_version;
    if not found then raise exception 'VERSION_CONFLICT: 外部绑定已变化'; end if;
    changed_value:=true;
  end if;
  select to_jsonb(binding) into after_value from timegenie.external_bindings binding where binding.id=binding_id;
  result_value:=timegenie.direct_cloud_mutation_response(p_workspace_id,p_device_id,p_operation_id,
    jsonb_build_object('changed',changed_value,'binding',timegenie.direct_cloud_external_binding_record(p_workspace_id,provider_value,entity_type_value,entity_id_value)),
    array['integrations'],p_expected_epoch);
  perform timegenie.record_direct_cloud_operation(p_workspace_id,p_operation_id,p_device_id,'direct_external_binding_upsert',request_value,result_value);
  perform timegenie.operation_history_replace_item(p_workspace_id,p_operation_id,'external_binding',
    provider_value||'|'||entity_type_value||'|'||entity_id_value::text,before_value,after_value);
  return result_value;
exception when invalid_text_representation then raise exception 'VALIDATION_ERROR: 外部绑定参数格式无效'; end $$;

create or replace function timegenie.direct_cloud_external_binding_delete(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; provider_value text; entity_type_value text; entity_id_value uuid;
declare expected_version bigint; before_value jsonb;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_external_binding_delete',request_value);
  if cached is not null then return cached; end if;
  provider_value:=timegenie.direct_cloud_integration_provider_assert(p_payload->>'provider');
  entity_type_value:=lower(btrim(coalesce(p_payload->>'entityType','')));
  if entity_type_value not in('task','subject','report') then raise exception 'VALIDATION_ERROR: 不支持的外部绑定对象类型 %',coalesce(entity_type_value,''); end if;
  entity_id_value:=nullif(p_payload->>'entityId','')::uuid;
  if entity_id_value is null then raise exception 'VALIDATION_ERROR: 外部绑定对象不能为空'; end if;
  expected_version:=case when p_payload ? 'expectedVersion' then nullif(p_payload->>'expectedVersion','')::bigint else null end;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':external-binding:'||provider_value||':'||entity_type_value||':'||entity_id_value::text,0));
  select to_jsonb(binding) into before_value from timegenie.external_bindings binding
  where binding.workspace_id=p_workspace_id and binding.provider=provider_value
    and binding.entity_type=entity_type_value and binding.entity_id=entity_id_value for update;
  if before_value is not null then
    if expected_version is null or (before_value->>'version')::bigint<>expected_version then raise exception 'VERSION_CONFLICT: 外部绑定已变化'; end if;
    delete from timegenie.external_bindings binding where binding.id=(before_value->>'id')::uuid;
  elsif expected_version is not null and expected_version<>0 then
    raise exception 'VERSION_CONFLICT: 外部绑定已变化';
  end if;
  result_value:=timegenie.direct_cloud_mutation_response(p_workspace_id,p_device_id,p_operation_id,
    jsonb_build_object('changed',before_value is not null,'binding',null),array['integrations'],p_expected_epoch);
  perform timegenie.record_direct_cloud_operation(p_workspace_id,p_operation_id,p_device_id,'direct_external_binding_delete',request_value,result_value);
  perform timegenie.operation_history_replace_item(p_workspace_id,p_operation_id,'external_binding',
    provider_value||'|'||entity_type_value||'|'||entity_id_value::text,before_value,null);
  return result_value;
exception when invalid_text_representation then raise exception 'VALIDATION_ERROR: 外部绑定参数格式无效'; end $$;

revoke all on function timegenie.direct_cloud_integration_reject_sensitive(jsonb,text) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_integration_provider_assert(text) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_integration_record(uuid,text) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_external_binding_record(uuid,text,text,uuid) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_integration_config_save(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_integration_state(uuid,uuid,bigint) from public,anon;
revoke all on function timegenie.direct_cloud_external_binding_upsert(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_external_binding_delete(uuid,uuid,uuid,bigint,jsonb) from public,anon;
grant execute on function timegenie.direct_cloud_integration_config_save(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_integration_state(uuid,uuid,bigint) to authenticated;
grant execute on function timegenie.direct_cloud_external_binding_upsert(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_external_binding_delete(uuid,uuid,uuid,bigint,jsonb) to authenticated;

commit;

notify pgrst,'reload schema';
