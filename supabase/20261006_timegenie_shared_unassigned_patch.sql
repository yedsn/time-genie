-- TimeGenie shared unassigned-time coordination patch.
-- Run after 20261005b_timegenie_event_driven_sync_patch.sql.

begin;

alter table timegenie.unassigned_sessions
  add column if not exists shared_source text not null default 'cloud',
  add column if not exists predecessor_session_id uuid,
  add column if not exists migration_state text not null default 'ready',
  add column if not exists resolution_operation_id uuid;

create unique index if not exists uq_unassigned_active
  on timegenie.unassigned_sessions(workspace_id)
  where state in ('collecting','awaiting_resolution');
create unique index if not exists uq_unassigned_next_session
  on timegenie.unassigned_sessions(workspace_id,predecessor_session_id)
  where predecessor_session_id is not null;
create unique index if not exists uq_unassigned_resolution_operation
  on timegenie.unassigned_sessions(workspace_id,resolution_operation_id)
  where resolution_operation_id is not null;

create or replace function timegenie.unassigned_get_or_create_shared(
  p_workspace_id uuid,
  p_device_id uuid,
  p_candidate jsonb default null,
  p_predecessor_session_id uuid default null,
  p_started_at timestamptz default null
) returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare session_row timegenie.unassigned_sessions; boundary timestamptz; threshold integer; candidate_id uuid;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text || ':shared-unassigned',0));
  select * into session_row from timegenie.unassigned_sessions
  where workspace_id=p_workspace_id and state in ('collecting','awaiting_resolution')
  order by created_at limit 1 for update;
  if found then
    return timegenie.cloud_incremental_entity_get(p_workspace_id,'unassigned_sessions',session_row.id::text)->'data';
  end if;
  if exists(select 1 from timegenie.time_entries where workspace_id=p_workspace_id and state in ('running','paused') and deleted_at is null) then
    return null;
  end if;
  if p_predecessor_session_id is not null then
    select resolved_at into boundary from timegenie.unassigned_sessions
    where workspace_id=p_workspace_id and id=p_predecessor_session_id;
  end if;
  boundary := coalesce(boundary,p_started_at,case when p_candidate is null then null else to_timestamp((p_candidate->>'first_started_at')::double precision/1000) end,clock_timestamp());
  threshold := coalesce((p_candidate->>'threshold_seconds')::integer,300);
  candidate_id := coalesce(nullif(p_candidate->>'id','')::uuid,gen_random_uuid());
  insert into timegenie.unassigned_sessions(
    id,workspace_id,work_date,state,threshold_seconds,duration_seconds,first_started_at,
    last_ended_at,prompted_at,resolution_type,generated_entry_id,resolved_at,
    shared_source,predecessor_session_id,migration_state,created_at,updated_at,version
  ) values(
    candidate_id,p_workspace_id,(boundary at time zone 'Asia/Shanghai')::date,
    case when p_candidate->>'state'='awaiting_resolution' then 'awaiting_resolution' else 'collecting' end,
    threshold,coalesce((p_candidate->>'duration_seconds')::bigint,0),boundary,
    null,null,null,null,null,'cloud',p_predecessor_session_id,'adopted',boundary,clock_timestamp(),1
  ) returning * into session_row;
  insert into timegenie.unassigned_segments(
    id,workspace_id,session_id,sequence_no,started_at,ended_at,duration_seconds,lease_token
  ) values(gen_random_uuid(),p_workspace_id,session_row.id,1,boundary,null,0,null);
  return timegenie.cloud_incremental_entity_get(p_workspace_id,'unassigned_sessions',session_row.id::text)->'data';
end $$;

create or replace function timegenie.unassigned_resolve_shared(
  p_workspace_id uuid,
  p_device_id uuid,
  p_operation_id uuid,
  p_session_id uuid,
  p_expected_version bigint,
  p_resolution_type text,
  p_payload jsonb
) returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare session_row timegenie.unassigned_sessions; cached jsonb; server_resolved_at timestamptz; result_value jsonb;
begin
  if p_resolution_type not in ('work','break','discard') then raise exception 'VALIDATION_ERROR: invalid unassigned resolution'; end if;
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  cached := timegenie.processed_operation_result(
    p_workspace_id,p_operation_id,'unassigned_resolve_shared',
    jsonb_build_object('deviceId',p_device_id,'sessionId',p_session_id,'expectedVersion',p_expected_version,'resolutionType',p_resolution_type,'payload',coalesce(p_payload,'{}'::jsonb))
  );
  if cached is not null then return cached; end if;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text || ':shared-unassigned:' || p_session_id::text,0));
  select * into session_row from timegenie.unassigned_sessions
  where workspace_id=p_workspace_id and id=p_session_id for update;
  if not found then raise exception 'NOT_FOUND: shared unassigned session'; end if;
  if session_row.state in ('resolved','discarded') then
    return jsonb_build_object('accepted',false,'session',timegenie.cloud_incremental_entity_get(p_workspace_id,'unassigned_sessions',p_session_id::text)->'data');
  end if;
  if session_row.version <> p_expected_version then raise exception 'VERSION_CONFLICT'; end if;
  server_resolved_at := clock_timestamp();
  update timegenie.unassigned_segments
  set ended_at=least(server_resolved_at,greatest(started_at,coalesce(ended_at,server_resolved_at))),
      duration_seconds=greatest(0,extract(epoch from (least(server_resolved_at,greatest(started_at,coalesce(ended_at,server_resolved_at)))-started_at))::bigint)
  where session_id=p_session_id;
  update timegenie.unassigned_sessions
  set state=case when p_resolution_type='discard' then 'discarded' else 'resolved' end,
      duration_seconds=(select coalesce(sum(duration_seconds),0) from timegenie.unassigned_segments where session_id=p_session_id),
      resolution_type=p_resolution_type,generated_entry_id=null,
      resolved_at=server_resolved_at,last_ended_at=server_resolved_at,updated_at=server_resolved_at,
      resolution_operation_id=p_operation_id,shared_source='cloud',migration_state='adopted',version=version+1
  where id=p_session_id;
  result_value := jsonb_build_object('accepted',true,'session',timegenie.cloud_incremental_entity_get(p_workspace_id,'unassigned_sessions',p_session_id::text)->'data');
  perform timegenie.record_processed_operation(
    p_workspace_id,p_operation_id,p_device_id,'unassigned_resolve_shared',
    jsonb_build_object('deviceId',p_device_id,'sessionId',p_session_id,'expectedVersion',p_expected_version,'resolutionType',p_resolution_type,'payload',coalesce(p_payload,'{}'::jsonb)),
    result_value
  );
  return result_value;
end $$;

create or replace function timegenie.cloud_incremental_entity_get(p_workspace_id uuid,p_entity_type text,p_entity_id text)
returns jsonb language plpgsql stable security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare entity_value jsonb; key_parts text[]; result_entity_type text := p_entity_type;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  key_parts := string_to_array(p_entity_id,'|');
  entity_value := case p_entity_type
    when 'subjects' then (select to_jsonb(r) from timegenie.subjects r where workspace_id=p_workspace_id and id::text=p_entity_id)
    when 'work_days' then (select to_jsonb(r) from timegenie.work_days r where workspace_id=p_workspace_id and work_date::text=p_entity_id)
    when 'app_settings' then (select to_jsonb(r) from timegenie.app_settings r where workspace_id=p_workspace_id and key=p_entity_id)
    when 'tasks' then (select to_jsonb(r) from timegenie.tasks r where workspace_id=p_workspace_id and id::text=p_entity_id)
    when 'task_status_events' then (select to_jsonb(r) from timegenie.task_status_events r where workspace_id=p_workspace_id and id::text=p_entity_id)
    when 'task_daily_estimates' then (select to_jsonb(r) from timegenie.task_daily_estimates r where workspace_id=p_workspace_id and task_id::text=key_parts[1] and work_date::text=key_parts[2])
    when 'task_recurrence_rules' then jsonb_build_object('task_id',p_entity_id,'rules',coalesce((select jsonb_agg(to_jsonb(r) order by effective_start,id) from timegenie.task_recurrence_rules r where workspace_id=p_workspace_id and task_id::text=p_entity_id),'[]'::jsonb))
    when 'task_occurrences' then (select to_jsonb(r) from timegenie.task_occurrences r where workspace_id=p_workspace_id and task_id::text=key_parts[1] and occurrence_date::text=key_parts[2])
    when 'unassigned_sessions' then (select to_jsonb(r) || jsonb_build_object('segments',coalesce((select jsonb_agg(to_jsonb(s) order by sequence_no,id) from timegenie.unassigned_segments s where workspace_id=p_workspace_id and session_id=r.id),'[]'::jsonb)) from timegenie.unassigned_sessions r where workspace_id=p_workspace_id and id::text=p_entity_id)
    when 'time_entries' then (select to_jsonb(r) || jsonb_build_object('segments',coalesce((select jsonb_agg(to_jsonb(s) order by sequence_no,id) from timegenie.time_segments s where workspace_id=p_workspace_id and entry_id=r.id),'[]'::jsonb),'allocations',coalesce((select jsonb_agg(to_jsonb(a) order by created_at,id) from timegenie.time_allocations a where workspace_id=p_workspace_id and entry_id=r.id),'[]'::jsonb)) from timegenie.time_entries r where workspace_id=p_workspace_id and id::text=p_entity_id)
    when 'time_segments' then (select to_jsonb(r) || jsonb_build_object('segments',coalesce((select jsonb_agg(to_jsonb(s) order by sequence_no,id) from timegenie.time_segments s where workspace_id=p_workspace_id and entry_id=r.id),'[]'::jsonb),'allocations',coalesce((select jsonb_agg(to_jsonb(a) order by created_at,id) from timegenie.time_allocations a where workspace_id=p_workspace_id and entry_id=r.id),'[]'::jsonb)) from timegenie.time_entries r where workspace_id=p_workspace_id and id::text=p_entity_id)
    when 'time_allocations' then (select to_jsonb(r) || jsonb_build_object('segments',coalesce((select jsonb_agg(to_jsonb(s) order by sequence_no,id) from timegenie.time_segments s where workspace_id=p_workspace_id and entry_id=r.id),'[]'::jsonb),'allocations',coalesce((select jsonb_agg(to_jsonb(a) order by created_at,id) from timegenie.time_allocations a where workspace_id=p_workspace_id and entry_id=r.id),'[]'::jsonb)) from timegenie.time_entries r where workspace_id=p_workspace_id and id::text=p_entity_id)
    when 'report_templates' then (select to_jsonb(r) from timegenie.report_templates r where workspace_id=p_workspace_id and id::text=p_entity_id)
    when 'reports' then (select to_jsonb(r) || jsonb_build_object('report_tasks',coalesce((select jsonb_agg(to_jsonb(t) order by sort_order,task_id) from timegenie.report_tasks t where workspace_id=p_workspace_id and report_id=r.id),'[]'::jsonb)) from timegenie.reports r where workspace_id=p_workspace_id and id::text=p_entity_id)
    when 'report_tasks' then (select to_jsonb(r) || jsonb_build_object('report_tasks',coalesce((select jsonb_agg(to_jsonb(t) order by sort_order,task_id) from timegenie.report_tasks t where workspace_id=p_workspace_id and report_id=r.id),'[]'::jsonb)) from timegenie.reports r where workspace_id=p_workspace_id and id::text=p_entity_id)
    when 'integration_configs' then (select to_jsonb(r) from timegenie.integration_configs r where workspace_id=p_workspace_id and provider=p_entity_id)
    when 'external_bindings' then (select to_jsonb(r) from timegenie.external_bindings r where workspace_id=p_workspace_id and provider=key_parts[1] and entity_type=key_parts[2] and entity_id::text=key_parts[3])
    else null end;
  if p_entity_type in ('time_segments','time_allocations') then result_entity_type := 'time_entries'; end if;
  if p_entity_type='report_tasks' then result_entity_type := 'reports'; end if;
  return jsonb_build_object('entity_type',result_entity_type,'source_entity_type',p_entity_type,'entity_id',p_entity_id,'deleted',entity_value is null,'data',entity_value);
end $$;

do $$
begin
  if to_regprocedure('timegenie.cloud_apply_patch_pre_shared(uuid,uuid,uuid,text,text,text,bigint,jsonb)') is null then
    alter function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,jsonb)
      rename to cloud_apply_patch_pre_shared;
  end if;
end $$;

create or replace function timegenie.cloud_apply_shared_unassigned_patch(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_operation_type text,
  p_entity_id text,p_base_version bigint,p_payload jsonb
) returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare row_data jsonb := coalesce(p_payload,'{}'::jsonb); current_version bigint; current_state text; cached jsonb; result_value jsonb;
begin
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then raise exception 'DEVICE_NOT_REGISTERED'; end if;
  if p_entity_id is null or row_data->>'id' is distinct from p_entity_id then
    raise exception 'VALIDATION_ERROR: entity id does not match payload id';
  end if;
  cached := timegenie.processed_operation_result(
    p_workspace_id,p_operation_id,p_operation_type,
    jsonb_build_object('deviceId',p_device_id,'entityType','unassigned_session','entityId',p_entity_id,'baseVersion',p_base_version,'payload',row_data)
  );
  if cached is not null then return cached; end if;
  perform pg_advisory_xact_lock(hashtextextended(p_workspace_id::text || ':shared-unassigned:' || p_entity_id,0));
  select version,state into current_version,current_state from timegenie.unassigned_sessions
  where workspace_id=p_workspace_id and id=p_entity_id::uuid for update;
  if p_base_version is null then
    if current_version is not null then raise exception 'SYNC_CONFLICT: unassigned session already exists'; end if;
  elsif current_version is distinct from p_base_version then
    if p_operation_type in ('unassigned_session_pause','unassigned_session_resume','unassigned_session_awaiting_resolution')
       and current_state is not null then
      result_value := jsonb_build_object(
        'superseded',true,
        'session',timegenie.cloud_incremental_entity_get(p_workspace_id,'unassigned_sessions',p_entity_id)->'data'
      );
      perform timegenie.record_processed_operation(
        p_workspace_id,p_operation_id,p_device_id,p_operation_type,
        jsonb_build_object('deviceId',p_device_id,'entityType','unassigned_session','entityId',p_entity_id,'baseVersion',p_base_version,'payload',row_data),
        result_value
      );
      return result_value;
    end if;
    raise exception 'SYNC_CONFLICT: expected unassigned session version %, current version %',p_base_version,current_version;
  end if;
  insert into timegenie.unassigned_sessions(
    id,workspace_id,work_date,state,threshold_seconds,duration_seconds,first_started_at,
    last_ended_at,prompted_at,resolution_type,generated_entry_id,resolved_at,
    shared_source,predecessor_session_id,migration_state,resolution_operation_id,
    created_at,updated_at,version
  ) values(
    (row_data->>'id')::uuid,p_workspace_id,(row_data->>'work_date')::date,row_data->>'state',
    coalesce((row_data->>'threshold_seconds')::integer,300),coalesce((row_data->>'duration_seconds')::bigint,0),
    to_timestamp((row_data->>'first_started_at')::double precision/1000),
    case when row_data->>'last_ended_at' is null then null else to_timestamp((row_data->>'last_ended_at')::double precision/1000) end,
    case when row_data->>'prompted_at' is null then null else to_timestamp((row_data->>'prompted_at')::double precision/1000) end,
    row_data->>'resolution_type',nullif(row_data->>'generated_entry_id','')::uuid,
    case when row_data->>'resolved_at' is null then null else to_timestamp((row_data->>'resolved_at')::double precision/1000) end,
    'cloud',nullif(row_data->>'predecessor_session_id','')::uuid,'adopted',nullif(row_data->>'resolution_operation_id','')::uuid,
    to_timestamp(coalesce((row_data->>'created_at')::double precision,extract(epoch from now())*1000)/1000),
    to_timestamp(coalesce((row_data->>'updated_at')::double precision,extract(epoch from now())*1000)/1000),
    coalesce((row_data->>'version')::bigint,1)
  ) on conflict(id) do update set
    work_date=excluded.work_date,state=excluded.state,threshold_seconds=excluded.threshold_seconds,
    duration_seconds=excluded.duration_seconds,first_started_at=excluded.first_started_at,
    last_ended_at=excluded.last_ended_at,prompted_at=excluded.prompted_at,
    resolution_type=excluded.resolution_type,generated_entry_id=excluded.generated_entry_id,
    resolved_at=excluded.resolved_at,shared_source='cloud',
    predecessor_session_id=excluded.predecessor_session_id,migration_state='adopted',
    resolution_operation_id=excluded.resolution_operation_id,updated_at=excluded.updated_at,version=excluded.version;
  delete from timegenie.unassigned_segments where session_id=p_entity_id::uuid;
  insert into timegenie.unassigned_segments(id,workspace_id,session_id,sequence_no,started_at,ended_at,duration_seconds,lease_token)
  select (item->>'id')::uuid,p_workspace_id,p_entity_id::uuid,(item->>'sequence_no')::integer,
         to_timestamp((item->>'started_at')::double precision/1000),
         case when item->>'ended_at' is null then null else to_timestamp((item->>'ended_at')::double precision/1000) end,
         coalesce((item->>'duration_seconds')::bigint,0),nullif(item->>'lease_token','')::uuid
  from jsonb_array_elements(coalesce(row_data->'segments','[]'::jsonb)) item;
  result_value := jsonb_build_object('entityType','unassigned_session','entityId',p_entity_id,'version',(row_data->>'version')::bigint);
  perform timegenie.record_processed_operation(
    p_workspace_id,p_operation_id,p_device_id,p_operation_type,
    jsonb_build_object('deviceId',p_device_id,'entityType','unassigned_session','entityId',p_entity_id,'baseVersion',p_base_version,'payload',row_data),
    result_value
  );
  return result_value;
end $$;

create or replace function timegenie.cloud_apply_patch(
  p_workspace_id uuid,p_device_id uuid,p_operation_id uuid,p_operation_type text,
  p_entity_type text,p_entity_id text,p_base_version bigint,p_payload jsonb
) returns jsonb language plpgsql security definer set search_path = timegenie, extensions, pg_catalog
as $$
declare result_value jsonb; row_data jsonb := coalesce(p_payload,'{}'::jsonb);
begin
  if p_entity_type='unassigned_session' then
    return timegenie.cloud_apply_shared_unassigned_patch(
      p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_id,p_base_version,row_data
    );
  end if;
  result_value := timegenie.cloud_apply_patch_pre_shared(
    p_workspace_id,p_device_id,p_operation_id,p_operation_type,p_entity_type,p_entity_id,p_base_version,row_data
  );
  if p_entity_type='time_entry' and nullif(row_data->>'origin_unassigned_session_id','') is not null then
    update timegenie.unassigned_sessions
    set generated_entry_id=(row_data->>'id')::uuid,updated_at=greatest(updated_at,now())
    where workspace_id=p_workspace_id
      and id=nullif(row_data->>'origin_unassigned_session_id','')::uuid
      and state in ('resolved','discarded');
  end if;
  return result_value;
end $$;

revoke execute on function timegenie.cloud_incremental_entity_get(uuid,text,text) from public,anon,authenticated;
revoke execute on function timegenie.cloud_apply_shared_unassigned_patch(uuid,uuid,uuid,text,text,bigint,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.unassigned_get_or_create_shared(uuid,uuid,jsonb,uuid,timestamptz) from public,anon;
revoke execute on function timegenie.unassigned_resolve_shared(uuid,uuid,uuid,uuid,bigint,text,jsonb) from public,anon;
grant execute on function timegenie.unassigned_get_or_create_shared(uuid,uuid,jsonb,uuid,timestamptz) to authenticated;
grant execute on function timegenie.unassigned_resolve_shared(uuid,uuid,uuid,uuid,bigint,text,jsonb) to authenticated;
grant execute on function timegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,jsonb) to authenticated;

commit;
