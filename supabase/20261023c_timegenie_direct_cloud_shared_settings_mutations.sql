-- Direct-v2 shared settings, work calendar and subject work-schedule writes.
-- Device-only preferences and integration credentials are deliberately absent.

begin;

create or replace function timegenie.direct_cloud_shared_setting_reject_sensitive(
  p_value jsonb,p_path text default '$'
) returns void language plpgsql immutable
set search_path=timegenie,extensions,pg_catalog as $$
declare pair record; item jsonb; index_value integer:=0; normalized text;
begin
  if p_value is null then return; end if;
  if jsonb_typeof(p_value)='object' then
    for pair in select key,value from jsonb_each(p_value) loop
      normalized:=lower(pair.key);
      if normalized like '%token%' or normalized like '%password%' or normalized like '%secret%'
         or normalized like '%authorization%' or normalized like '%apikey%' or normalized like '%api_key%' then
        raise exception 'VALIDATION_ERROR: 敏感字段 % 必须保存在系统凭据库',p_path||'.'||pair.key;
      end if;
      perform timegenie.direct_cloud_shared_setting_reject_sensitive(pair.value,p_path||'.'||pair.key);
    end loop;
  elsif jsonb_typeof(p_value)='array' then
    for item in select value from jsonb_array_elements(p_value) loop
      index_value:=index_value+1;
      perform timegenie.direct_cloud_shared_setting_reject_sensitive(item,p_path||'['||index_value||']');
    end loop;
  end if;
end $$;

create or replace function timegenie.direct_cloud_shared_setting_validate(
  p_key text,p_value jsonb
) returns void language plpgsql immutable
set search_path=timegenie,extensions,pg_catalog as $$
declare subject_id_text text; number_value numeric;
begin
  if p_key like 'subject_salary_hourly_rate:%' then
    begin perform split_part(p_key,':',2)::uuid; exception when others then raise exception 'VALIDATION_ERROR: 不允许写入设置键 %',p_key; end;
  elsif p_key not in('user_name','timezone','default_subject_id','unassigned_prompt_seconds',
      'default_work_period_text','salary_hourly_rate','report_duration_format') then
    raise exception 'VALIDATION_ERROR: 不允许写入设置键 %',p_key;
  end if;
  perform timegenie.direct_cloud_shared_setting_reject_sensitive(p_value,'$');
  if p_value is null or jsonb_typeof(p_value)='null' then raise exception 'VALIDATION_ERROR: 设置值不能为空'; end if;
  if p_key='timezone' then
    if jsonb_typeof(p_value)<>'string' then raise exception 'VALIDATION_ERROR: 工作空间时区必须是 IANA 时区名称'; end if;
    begin perform timestamptz '2000-01-01 00:00:00+00' at time zone (p_value#>>'{}');
    exception when others then raise exception 'VALIDATION_ERROR: 无效的工作空间时区: %',p_value#>>'{}'; end;
  elsif p_key='report_duration_format' then
    if p_value#>>'{}' not in('minutes','hours') then raise exception 'VALIDATION_ERROR: 报告时长格式只能是 minutes 或 hours'; end if;
  elsif p_key='salary_hourly_rate' or p_key like 'subject_salary_hourly_rate:%' then
    if jsonb_typeof(p_value)<>'number' then raise exception 'VALIDATION_ERROR: 工资时薪必须是大于等于 0 的数字'; end if;
    number_value:=(p_value#>>'{}')::numeric;
    if number_value<0 then raise exception 'VALIDATION_ERROR: 工资时薪必须是大于等于 0 的数字'; end if;
  elsif p_key='unassigned_prompt_seconds' then
    if jsonb_typeof(p_value)<>'number' or (p_value#>>'{}')::numeric<0 then raise exception 'VALIDATION_ERROR: 未归属提示秒数无效'; end if;
  elsif p_key='default_subject_id' then
    if jsonb_typeof(p_value)<>'string' then raise exception 'VALIDATION_ERROR: 默认主体标识无效'; end if;
    subject_id_text:=nullif(btrim(p_value#>>'{}'),'');
    if subject_id_text is null then raise exception 'VALIDATION_ERROR: 默认主体标识无效'; end if;
    begin perform subject_id_text::uuid; exception when others then raise exception 'VALIDATION_ERROR: 默认主体标识无效'; end;
  elsif p_key='user_name' then
    if jsonb_typeof(p_value)<>'string' or length(btrim(p_value#>>'{}'))>80 then raise exception 'VALIDATION_ERROR: 用户姓名无效'; end if;
  elsif p_key in('default_work_period_text') and jsonb_typeof(p_value)<>'string' then
    raise exception 'VALIDATION_ERROR: 设置值类型无效';
  end if;
end $$;

create or replace function timegenie.direct_cloud_shared_setting_record(p_workspace_id uuid,p_key text)
returns jsonb language sql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
  select jsonb_build_object('key',setting.key,'value',setting.value_json,'version',setting.version,'updatedAt',setting.updated_at)
  from timegenie.app_settings setting where setting.workspace_id=p_workspace_id and setting.key=p_key;
$$;

create or replace function timegenie.direct_cloud_shared_setting_apply(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_key text,p_value jsonb,p_expected_version bigint
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare current_version bigint; current_value jsonb; value_changed boolean; workspace_before jsonb; workspace_after jsonb;
begin
  perform timegenie.direct_cloud_shared_setting_validate(p_key,p_value);
  if p_key='timezone' or p_key='default_subject_id' or p_key like 'subject_salary_hourly_rate:%' then
    perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':shared-setting:'||p_key,0));
  end if;
  select setting.version,setting.value_json into current_version,current_value from timegenie.app_settings setting
  where setting.workspace_id=p_workspace_id and setting.key=p_key for update;
  if found then
    if p_expected_version is null or current_version<>p_expected_version then raise exception 'VERSION_CONFLICT: 共享设置已变化'; end if;
    if current_value is not distinct from p_value then return jsonb_build_object('changed',false,'setting',timegenie.direct_cloud_shared_setting_record(p_workspace_id,p_key),'workspace',null); end if;
  elsif p_expected_version is not null and p_expected_version<>0 then
    raise exception 'VERSION_CONFLICT: 共享设置已变化';
  end if;
  if p_key='default_subject_id' then
    if not exists(select 1 from timegenie.subjects subject where subject.workspace_id=p_workspace_id
      and subject.id=(p_value#>>'{}')::uuid and subject.deleted_at is null) then raise exception 'NOT_FOUND: 默认主体不存在'; end if;
  end if;
  if p_key like 'subject_salary_hourly_rate:%' then
    if not exists(select 1 from timegenie.subjects subject where subject.workspace_id=p_workspace_id
      and subject.id=split_part(p_key,':',2)::uuid and subject.deleted_at is null) then raise exception 'NOT_FOUND: 主体不存在'; end if;
  end if;
  if p_key='timezone' then
    select to_jsonb(workspace) into workspace_before from timegenie.workspaces workspace where workspace.id=p_workspace_id for update;
  end if;
  insert into timegenie.app_settings(workspace_id,key,value_json,updated_at,version)
  values(p_workspace_id,p_key,p_value,now(),1)
  on conflict(workspace_id,key) do update set value_json=excluded.value_json,updated_at=excluded.updated_at,version=timegenie.app_settings.version+1;
  if p_key='timezone' then
    update timegenie.workspaces workspace set timezone=p_value#>>'{}',updated_at=now(),version=workspace.version+1 where workspace.id=p_workspace_id;
    select to_jsonb(workspace) into workspace_after from timegenie.workspaces workspace where workspace.id=p_workspace_id;
    perform timegenie.operation_history_replace_item(p_workspace_id,p_operation_id,'workspace',p_workspace_id::text,workspace_before,workspace_after);
  end if;
  return jsonb_build_object('changed',true,'setting',timegenie.direct_cloud_shared_setting_record(p_workspace_id,p_key),
    'workspace',case when p_key='timezone' then workspace_after else null end);
end $$;

create or replace function timegenie.direct_cloud_shared_setting_update(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; key_value text; apply_value jsonb; before_value jsonb; after_value jsonb;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_shared_setting_update',request_value); if cached is not null then return cached; end if;
  key_value:=btrim(coalesce(p_payload->>'key',''));
  select to_jsonb(setting) into before_value from timegenie.app_settings setting where setting.workspace_id=p_workspace_id and setting.key=key_value;
  apply_value:=timegenie.direct_cloud_shared_setting_apply(p_workspace_id,p_device_id,p_operation_id,key_value,p_payload->'value',
    case when p_payload ? 'expectedVersion' then nullif(p_payload->>'expectedVersion','')::bigint else null end);
  result_value:=timegenie.direct_cloud_mutation_response(p_workspace_id,p_device_id,p_operation_id,apply_value,
    array['settings','calendar'],p_expected_epoch);
  perform timegenie.record_direct_cloud_operation(p_workspace_id,p_operation_id,p_device_id,'direct_shared_setting_update',request_value,result_value);
  if coalesce((apply_value->>'changed')::boolean,false) then
    after_value:=(select to_jsonb(setting) from timegenie.app_settings setting where setting.workspace_id=p_workspace_id and setting.key=key_value);
    perform timegenie.operation_history_replace_item(p_workspace_id,p_operation_id,'app_setting',key_value,before_value,after_value);
  end if;
  return result_value;
exception when invalid_text_representation then raise exception 'VALIDATION_ERROR: 共享设置参数格式无效'; end $$;

create or replace function timegenie.direct_cloud_shared_settings_batch_update(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; item jsonb; applied jsonb; setting_values jsonb:='[]'::jsonb; key_value text;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_shared_settings_batch_update',request_value); if cached is not null then return cached; end if;
  if jsonb_typeof(coalesce(p_payload->'items','null'::jsonb))<>'array' then raise exception 'VALIDATION_ERROR: 共享设置批量列表无效'; end if;
  if jsonb_array_length(p_payload->'items')=0 then raise exception 'VALIDATION_ERROR: 共享设置批量列表不能为空'; end if;
  for item in select value from jsonb_array_elements(p_payload->'items') loop
    key_value:=btrim(coalesce(item->>'key',''));
    if key_value='' then raise exception 'VALIDATION_ERROR: 共享设置键不能为空'; end if;
    applied:=timegenie.direct_cloud_shared_setting_apply(p_workspace_id,p_device_id,p_operation_id,key_value,item->'value',
      case when item ? 'expectedVersion' then nullif(item->>'expectedVersion','')::bigint else null end);
    setting_values:=setting_values||jsonb_build_array(applied->'setting');
  end loop;
  result_value:=timegenie.direct_cloud_mutation_response(p_workspace_id,p_device_id,p_operation_id,
    jsonb_build_object('settings',setting_values),array['settings','calendar'],p_expected_epoch);
  perform timegenie.record_direct_cloud_operation(p_workspace_id,p_operation_id,p_device_id,'direct_shared_settings_batch_update',request_value,result_value);
  return result_value;
exception when invalid_text_representation then raise exception 'VALIDATION_ERROR: 共享设置参数格式无效'; end $$;

create or replace function timegenie.direct_cloud_work_day_record(p_workspace_id uuid,p_work_date date)
returns jsonb language sql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
  select jsonb_build_object('workDate',day.work_date,'timezone',day.timezone,'workPeriodText',day.work_period_text,
    'note',day.note,'settledAt',day.settled_at,'version',day.version,'updatedAt',day.updated_at)
  from timegenie.work_days day where day.workspace_id=p_workspace_id and day.work_date=p_work_date;
$$;

create or replace function timegenie.direct_cloud_work_day_set(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; work_date_value date; expected_version bigint;
declare timezone_value text; period_text text; note_text text; before_value jsonb; after_value jsonb; created_value boolean:=false;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_work_day_set',request_value); if cached is not null then return cached; end if;
  work_date_value:=nullif(p_payload->>'workDate','')::date;
  if work_date_value is null then raise exception 'VALIDATION_ERROR: 工作日日期不能为空'; end if;
  expected_version:=case when p_payload ? 'expectedVersion' then nullif(p_payload->>'expectedVersion','')::bigint else null end;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':work-day:'||work_date_value::text,0));
  select to_jsonb(day) into before_value from timegenie.work_days day
  where day.workspace_id=p_workspace_id and day.work_date=work_date_value for update;
  if before_value is null then
    if expected_version is not null and expected_version<>0 then raise exception 'VERSION_CONFLICT: 工作日已变化'; end if;
    if not (p_payload ? 'workPeriodText') then raise exception 'VALIDATION_ERROR: 工作日工作时段不能为空'; end if;
    period_text:=nullif(btrim(p_payload->>'workPeriodText'),'');
    note_text:=nullif(btrim(p_payload->>'note'),'');
    select workspace.timezone into timezone_value from timegenie.workspaces workspace where workspace.id=p_workspace_id;
    insert into timegenie.work_days(workspace_id,work_date,timezone,work_period_text,note,version)
    values(p_workspace_id,work_date_value,timezone_value,period_text,note_text,1);
    created_value:=true;
  else
    if expected_version is null or (before_value->>'version')::bigint<>expected_version then raise exception 'VERSION_CONFLICT: 工作日已变化'; end if;
    timezone_value:=coalesce(nullif(p_payload->>'timezone',''),before_value->>'timezone');
    period_text:=case when p_payload ? 'workPeriodText' then nullif(btrim(p_payload->>'workPeriodText'),'') else before_value->>'work_period_text' end;
    note_text:=case when p_payload ? 'note' then nullif(btrim(p_payload->>'note'),'') else before_value->>'note' end;
    update timegenie.work_days day set timezone=timezone_value,work_period_text=period_text,note=note_text,updated_at=now(),version=day.version+1
    where day.workspace_id=p_workspace_id and day.work_date=work_date_value and day.version=expected_version;
    if not found then raise exception 'VERSION_CONFLICT: 工作日已变化'; end if;
  end if;
  after_value:=(select to_jsonb(day) from timegenie.work_days day where day.workspace_id=p_workspace_id and day.work_date=work_date_value);
  result_value:=timegenie.direct_cloud_mutation_response(p_workspace_id,p_device_id,p_operation_id,
    jsonb_build_object('created',created_value,'workDay',timegenie.direct_cloud_work_day_record(p_workspace_id,work_date_value)),
    array['calendar','settings'],p_expected_epoch);
  perform timegenie.record_direct_cloud_operation(p_workspace_id,p_operation_id,p_device_id,'direct_work_day_set',request_value,result_value);
  perform timegenie.operation_history_replace_item(p_workspace_id,p_operation_id,'work_day',work_date_value::text,before_value,after_value);
  return result_value;
exception when invalid_text_representation then raise exception 'VALIDATION_ERROR: 工作日参数格式无效'; end $$;

create or replace function timegenie.direct_cloud_work_schedule_day_record(
  p_workspace_id uuid,p_subject_id uuid,p_weekday integer
) returns jsonb language sql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
  select jsonb_build_object('id',schedule.id,'weekday',schedule.weekday,'enabled',schedule.is_workday,
    'segments',coalesce((select jsonb_agg(jsonb_build_object(
      'start',lpad((segment.start_minute/60)::text,2,'0')||':'||lpad((segment.start_minute%60)::text,2,'0'),
      'end',lpad((segment.end_minute/60)::text,2,'0')||':'||lpad((segment.end_minute%60)::text,2,'0')) order by segment.sequence_no)
      from timegenie.subject_work_schedule_segments segment where segment.workspace_id=p_workspace_id and segment.schedule_id=schedule.id),'[]'::jsonb),
    'version',schedule.version)
  from timegenie.subject_work_schedules schedule
  where schedule.workspace_id=p_workspace_id and schedule.subject_id=p_subject_id and schedule.weekday=p_weekday;
$$;

create or replace function timegenie.direct_cloud_subject_schedule_record(p_workspace_id uuid,p_subject_id uuid)
returns jsonb language sql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
  select jsonb_build_object('subjectId',p_subject_id,
    'subjectName',(select subject.name from timegenie.subjects subject where subject.workspace_id=p_workspace_id and subject.id=p_subject_id),
    'days',coalesce((select jsonb_agg(timegenie.direct_cloud_work_schedule_day_record(p_workspace_id,p_subject_id,weekday.value) order by weekday.value)
      from generate_series(0,6) weekday(value)),'[]'::jsonb));
$$;

create or replace function timegenie.direct_cloud_subject_schedule_apply_day(
  p_workspace_id uuid,p_subject_id uuid,p_day jsonb
) returns void language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare weekday_value integer; enabled_value boolean; expected_version bigint; schedule_id uuid; schedule_table_id uuid;
declare segment_item jsonb; segment_index integer:=0; previous_end integer:=-1; start_value integer; end_value integer; start_match text[]; end_match text[];
begin
  weekday_value:=nullif(p_day->>'weekday','')::integer;
  if weekday_value is null or weekday_value not between 0 and 6 then raise exception 'VALIDATION_ERROR: 主体工作时段星期无效'; end if;
  expected_version:=nullif(p_day->>'expectedVersion','')::bigint;
  schedule_id:=nullif(p_day->>'id','')::uuid;
  if schedule_id is null or expected_version is null then raise exception 'VALIDATION_ERROR: 主体工作时段标识或版本无效'; end if;
  if jsonb_typeof(coalesce(p_day->'enabled','null'::jsonb))<>'boolean' then raise exception 'VALIDATION_ERROR: 主体工作时段启用状态无效'; end if;
  enabled_value:=(p_day->>'enabled')::boolean;
  if jsonb_typeof(coalesce(p_day->'segments','null'::jsonb))<>'array' then raise exception 'VALIDATION_ERROR: 主体工作时段分段无效'; end if;
  if enabled_value and jsonb_array_length(p_day->'segments')=0 then raise exception 'VALIDATION_ERROR: 周%至少需要一个工作时段',weekday_value+1; end if;
  if not enabled_value and jsonb_array_length(p_day->'segments')>0 then raise exception 'VALIDATION_ERROR: 周%休息日不能包含工作时段',weekday_value+1; end if;
  select schedule.id into schedule_table_id from timegenie.subject_work_schedules schedule
  where schedule.workspace_id=p_workspace_id and schedule.subject_id=p_subject_id and schedule.weekday=weekday_value for update;
  if schedule_table_id is null or schedule_table_id<>schedule_id then raise exception 'VERSION_CONFLICT: 主体工作时段已被其他设备更新'; end if;
  if (select schedule.version from timegenie.subject_work_schedules schedule where schedule.id=schedule_id)<>expected_version then
    raise exception 'VERSION_CONFLICT: 主体工作时段已被其他设备更新';
  end if;
  if (timegenie.direct_cloud_work_schedule_day_record(p_workspace_id,p_subject_id,weekday_value)->>'enabled')::boolean=enabled_value
     and timegenie.direct_cloud_work_schedule_day_record(p_workspace_id,p_subject_id,weekday_value)->'segments'=coalesce(p_day->'segments','[]'::jsonb) then
    return;
  end if;
  for segment_item in select value from jsonb_array_elements(p_day->'segments') loop
    segment_index:=segment_index+1;
    start_match:=regexp_match(coalesce(segment_item->>'start',''),'^([01][0-9]|2[0-3]):([0-5][0-9])$');
    end_match:=regexp_match(coalesce(segment_item->>'end',''),'^([01][0-9]|2[0-3]):([0-5][0-9])$');
    if start_match is null or end_match is null then raise exception 'VALIDATION_ERROR: 工作时段时间格式无效'; end if;
    start_value:=start_match[1]::integer*60+start_match[2]::integer;
    end_value:=end_match[1]::integer*60+end_match[2]::integer;
    if start_value>=end_value then raise exception 'VALIDATION_ERROR: 周%结束时间必须晚于开始时间',weekday_value+1; end if;
    if previous_end>start_value then raise exception 'VALIDATION_ERROR: 周%存在重叠的工作时段',weekday_value+1; end if;
    previous_end:=end_value;
  end loop;
  update timegenie.subject_work_schedules schedule set is_workday=enabled_value,updated_at=now(),version=schedule.version+1
  where schedule.id=schedule_id and schedule.version=expected_version;
  if not found then raise exception 'VERSION_CONFLICT: 主体工作时段已被其他设备更新'; end if;
  delete from timegenie.subject_work_schedule_segments segment where segment.schedule_id=schedule_id;
  segment_index:=0;
  for segment_item in select value from jsonb_array_elements(p_day->'segments') loop
    segment_index:=segment_index+1;
    start_match:=regexp_match(segment_item->>'start','^([01][0-9]|2[0-3]):([0-5][0-9])$');
    end_match:=regexp_match(segment_item->>'end','^([01][0-9]|2[0-3]):([0-5][0-9])$');
    insert into timegenie.subject_work_schedule_segments(id,workspace_id,schedule_id,sequence_no,start_minute,end_minute)
    values(gen_random_uuid(),p_workspace_id,schedule_id,segment_index,
      start_match[1]::integer*60+start_match[2]::integer,end_match[1]::integer*60+end_match[2]::integer);
  end loop;
end $$;

create or replace function timegenie.direct_cloud_subject_schedule_save(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_expected_epoch bigint,p_payload jsonb
) returns jsonb language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
#variable_conflict use_variable
declare request_value jsonb; cached jsonb; result_value jsonb; subject_id_value uuid; day_item jsonb; seen_weekdays integer[]:='{}'::integer[];
declare batch_value boolean; before_value jsonb:='[]'::jsonb; after_value jsonb;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  request_value:=jsonb_build_object('expectedEpoch',p_expected_epoch,'payload',coalesce(p_payload,'{}'::jsonb));
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':'||p_operation_id::text,0));
  cached:=timegenie.direct_cloud_operation_result(p_workspace_id,p_operation_id,'direct_subject_schedule_save',request_value); if cached is not null then return cached; end if;
  subject_id_value:=nullif(p_payload->>'subjectId','')::uuid;
  if subject_id_value is null then raise exception 'VALIDATION_ERROR: 主体标识不能为空'; end if;
  batch_value:=p_payload ? 'days';
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text||':subject-schedule:'||subject_id_value::text,0));
  perform 1 from timegenie.subjects subject where subject.workspace_id=p_workspace_id and subject.id=subject_id_value and subject.deleted_at is null for update;
  if not found then raise exception 'NOT_FOUND: 主体不存在'; end if;
  if batch_value then
    if jsonb_typeof(p_payload->'days')<>'array' or jsonb_array_length(p_payload->'days')<>7 then raise exception 'VALIDATION_ERROR: 主体工作时段必须包含周一至周日'; end if;
    for day_item in select value from jsonb_array_elements(p_payload->'days') loop
      if (day_item->>'weekday')::integer=any(seen_weekdays) then raise exception 'VALIDATION_ERROR: 主体工作时段星期无效'; end if;
      seen_weekdays:=array_append(seen_weekdays,(day_item->>'weekday')::integer);
    end loop;
    if (select count(*) from unnest(seen_weekdays) value where value between 0 and 6)<>7 then raise exception 'VALIDATION_ERROR: 主体工作时段星期无效'; end if;
  else
    day_item:=p_payload->'day';
    if day_item is null then raise exception 'VALIDATION_ERROR: 主体工作时段为空'; end if;
  end if;
  select coalesce(jsonb_agg(timegenie.direct_cloud_work_schedule_day_record(p_workspace_id,subject_id_value,weekday.value) order by weekday.value),'[]'::jsonb)
  into before_value from generate_series(0,6) weekday(value);
  if batch_value then
    for day_item in select value from jsonb_array_elements(p_payload->'days') loop
      perform timegenie.direct_cloud_subject_schedule_apply_day(p_workspace_id,subject_id_value,day_item);
    end loop;
  else
    perform timegenie.direct_cloud_subject_schedule_apply_day(p_workspace_id,subject_id_value,day_item);
  end if;
  after_value:=timegenie.direct_cloud_subject_schedule_record(p_workspace_id,subject_id_value);
  result_value:=timegenie.direct_cloud_mutation_response(p_workspace_id,p_device_id,p_operation_id,
    jsonb_build_object('schedule',after_value,'batch',batch_value),array['settings','calendar'],p_expected_epoch);
  perform timegenie.record_direct_cloud_operation(p_workspace_id,p_operation_id,p_device_id,'direct_subject_schedule_save',request_value,result_value);
  for day_item in select value from jsonb_array_elements(after_value->'days') loop
    perform timegenie.operation_history_replace_item(p_workspace_id,p_operation_id,'subject_work_schedule',day_item->>'id',
      (select value from jsonb_array_elements(before_value) previous where previous->>'id'=day_item->>'id'),day_item);
  end loop;
  return result_value;
exception when invalid_text_representation then raise exception 'VALIDATION_ERROR: 主体工作时段参数格式无效'; end $$;

revoke all on function timegenie.direct_cloud_shared_setting_reject_sensitive(jsonb,text) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_shared_setting_validate(text,jsonb) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_shared_setting_record(uuid,text) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_shared_setting_apply(uuid,uuid,uuid,text,jsonb,bigint) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_work_day_record(uuid,date) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_work_schedule_day_record(uuid,uuid,integer) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_subject_schedule_record(uuid,uuid) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_subject_schedule_apply_day(uuid,uuid,jsonb) from public,anon,authenticated;
revoke all on function timegenie.direct_cloud_shared_setting_update(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_shared_settings_batch_update(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_work_day_set(uuid,uuid,uuid,bigint,jsonb) from public,anon;
revoke all on function timegenie.direct_cloud_subject_schedule_save(uuid,uuid,uuid,bigint,jsonb) from public,anon;
grant execute on function timegenie.direct_cloud_shared_setting_update(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_shared_settings_batch_update(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_work_day_set(uuid,uuid,uuid,bigint,jsonb) to authenticated;
grant execute on function timegenie.direct_cloud_subject_schedule_save(uuid,uuid,uuid,bigint,jsonb) to authenticated;

commit;

notify pgrst,'reload schema';
