-- Isolated staging for explicit local-to-cloud copies. No function in this
-- patch writes authoritative business tables.

begin;

create table if not exists timegenie.data_copy_staging_packages (
  id uuid primary key default gen_random_uuid(),
  workspace_id uuid not null references timegenie.workspaces(id) on delete cascade,
  device_id uuid not null references timegenie.devices(id) on delete cascade,
  package_version integer not null,
  schema_generation bigint not null,
  expected_chunk_count integer not null check(expected_chunk_count between 1 and 256),
  expected_content_hash text not null check(expected_content_hash ~ '^[0-9a-f]{64}$'),
  status text not null default 'uploading' check(status in ('uploading','validated','invalid')),
  entity_counts jsonb not null default '{}'::jsonb,
  failure_code text,
  created_at timestamptz not null default now(),
  validated_at timestamptz,
  expires_at timestamptz not null default (now()+interval '1 day')
);

create table if not exists timegenie.data_copy_staging_chunks (
  package_id uuid not null references timegenie.data_copy_staging_packages(id) on delete cascade,
  chunk_index integer not null check(chunk_index between 0 and 255),
  entity_name text not null,
  content_text text not null,
  content_hash text not null check(content_hash ~ '^[0-9a-f]{64}$'),
  row_count integer not null check(row_count>=0),
  created_at timestamptz not null default now(),
  primary key(package_id,chunk_index)
);

create index if not exists idx_data_copy_staging_expiry on timegenie.data_copy_staging_packages(expires_at);
create index if not exists idx_data_copy_staging_entity on timegenie.data_copy_staging_chunks(package_id,entity_name,chunk_index);

alter table timegenie.data_copy_staging_packages enable row level security;
alter table timegenie.data_copy_staging_chunks enable row level security;
drop policy if exists workspace_owner on timegenie.data_copy_staging_packages;
create policy workspace_owner on timegenie.data_copy_staging_packages
  using(timegenie.is_workspace_owner(workspace_id)) with check(timegenie.is_workspace_owner(workspace_id));
drop policy if exists workspace_owner on timegenie.data_copy_staging_chunks;
create policy workspace_owner on timegenie.data_copy_staging_chunks
  using(exists(select 1 from timegenie.data_copy_staging_packages package where package.id=package_id and timegenie.is_workspace_owner(package.workspace_id)))
  with check(exists(select 1 from timegenie.data_copy_staging_packages package where package.id=package_id and timegenie.is_workspace_owner(package.workspace_id)));

create or replace function timegenie.data_copy_entity_catalog()
returns table(entity_name text,sort_order integer) language sql immutable as $$ values
  ('subjects',10),('subject_work_schedules',20),('subject_work_schedule_segments',30),('work_days',40),
  ('tasks',50),('task_status_events',60),('task_day_plans',70),('task_daily_estimates',80),
  ('task_recurrence_rules',90),('task_occurrences',100),('time_entries',110),('time_segments',120),
  ('time_allocations',130),('unassigned_sessions',140),('unassigned_segments',150),
  ('actual_time_adjustments',160),('report_templates',170),('reports',180),('report_tasks',190),
  ('app_settings',200),('integration_configs',210),('external_bindings',220);
$$;

create or replace function timegenie.data_copy_stage_rows(p_package_id uuid,p_entity_name text)
returns jsonb language sql stable as $$
  select coalesce(jsonb_agg(item order by chunk.chunk_index,item_order),'[]'::jsonb)
  from timegenie.data_copy_staging_chunks chunk
  cross join lateral jsonb_array_elements(chunk.content_text::jsonb) with ordinality expanded(item,item_order)
  where chunk.package_id=p_package_id and chunk.entity_name=p_entity_name;
$$;

create or replace function timegenie.data_copy_stage_begin(
  p_workspace_id uuid,p_device_id uuid,p_expected_epoch bigint,p_package_version integer,
  p_schema_generation bigint,p_expected_chunk_count integer,p_expected_content_hash text
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare package_id_value uuid;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  if p_package_version<>1 then raise exception 'DATA_PACKAGE_VERSION_UNSUPPORTED'; end if;
  if p_schema_generation<>1 then raise exception 'CLIENT_UPGRADE_REQUIRED: data package schema generation mismatch'; end if;
  if p_expected_chunk_count<22 or p_expected_chunk_count>256 then raise exception 'DATA_PACKAGE_CHUNK_COUNT_INVALID'; end if;
  if coalesce(p_expected_content_hash,'')!~'^[0-9a-f]{64}$' then raise exception 'DATA_PACKAGE_HASH_INVALID'; end if;
  delete from timegenie.data_copy_staging_packages where expires_at<=now();
  insert into timegenie.data_copy_staging_packages(workspace_id,device_id,package_version,schema_generation,expected_chunk_count,expected_content_hash)
    values(p_workspace_id,p_device_id,p_package_version,p_schema_generation,p_expected_chunk_count,p_expected_content_hash)
    returning id into package_id_value;
  return jsonb_build_object('packageId',package_id_value,'status','uploading','expiresAt',now()+interval '1 day');
end $$;

create or replace function timegenie.data_copy_stage_chunk_put(
  p_workspace_id uuid,p_device_id uuid,p_expected_epoch bigint,p_package_id uuid,
  p_chunk_index integer,p_entity_name text,p_content_text text,p_content_hash text,p_row_count integer
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare package_row timegenie.data_copy_staging_packages; parsed_value jsonb; actual_hash text;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  select package.* into package_row from timegenie.data_copy_staging_packages package
    where package.id=p_package_id and package.workspace_id=p_workspace_id and package.device_id=p_device_id
      and package.status='uploading' and package.expires_at>now() for update;
  if not found then raise exception 'DATA_COPY_STAGE_NOT_FOUND'; end if;
  if p_chunk_index<0 or p_chunk_index>=package_row.expected_chunk_count then raise exception 'DATA_PACKAGE_CHUNK_INDEX_INVALID'; end if;
  if not exists(select 1 from timegenie.data_copy_entity_catalog() where entity_name=p_entity_name) then raise exception 'DATA_PACKAGE_ENTITY_UNKNOWN'; end if;
  if octet_length(coalesce(p_content_text,''))>16777216 then raise exception 'DATA_PACKAGE_CHUNK_TOO_LARGE'; end if;
  actual_hash:=encode(digest(convert_to(coalesce(p_content_text,''),'utf8'),'sha256'),'hex');
  if actual_hash<>coalesce(p_content_hash,'') then raise exception 'DATA_PACKAGE_CHUNK_HASH_MISMATCH'; end if;
  begin parsed_value:=p_content_text::jsonb; exception when others then raise exception 'DATA_PACKAGE_CHUNK_JSON_INVALID'; end;
  if jsonb_typeof(parsed_value)<>'array' or jsonb_array_length(parsed_value)<>p_row_count then raise exception 'DATA_PACKAGE_ROW_COUNT_MISMATCH'; end if;
  insert into timegenie.data_copy_staging_chunks(package_id,chunk_index,entity_name,content_text,content_hash,row_count)
    values(p_package_id,p_chunk_index,p_entity_name,p_content_text,p_content_hash,p_row_count)
    on conflict(package_id,chunk_index) do update set entity_name=excluded.entity_name,content_text=excluded.content_text,content_hash=excluded.content_hash,row_count=excluded.row_count,created_at=now();
  return jsonb_build_object('packageId',p_package_id,'chunkIndex',p_chunk_index,'accepted',true);
end $$;

create or replace function timegenie.data_copy_stage_validate(
  p_workspace_id uuid,p_device_id uuid,p_expected_epoch bigint,p_package_id uuid
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare package_row timegenie.data_copy_staging_packages; actual_count integer; actual_hash text; counts_value jsonb;
declare subjects_value jsonb; schedules_value jsonb; tasks_value jsonb;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  select package.* into package_row from timegenie.data_copy_staging_packages package
    where package.id=p_package_id and package.workspace_id=p_workspace_id and package.device_id=p_device_id and package.expires_at>now() for update;
  if not found then raise exception 'DATA_COPY_STAGE_NOT_FOUND'; end if;
  if package_row.status='validated' then return jsonb_build_object('packageId',p_package_id,'status','validated','entityCounts',package_row.entity_counts,'contentHash',package_row.expected_content_hash); end if;
  select count(*),encode(digest(convert_to(coalesce(string_agg(content_hash,'' order by chunk_index),''),'utf8'),'sha256'),'hex')
    into actual_count,actual_hash from timegenie.data_copy_staging_chunks where package_id=p_package_id;
  if actual_count<>package_row.expected_chunk_count then raise exception 'DATA_PACKAGE_CHUNK_MISSING'; end if;
  if exists(select 1 from generate_series(0,package_row.expected_chunk_count-1) expected
    left join timegenie.data_copy_staging_chunks chunk on chunk.package_id=p_package_id and chunk.chunk_index=expected where chunk.package_id is null) then raise exception 'DATA_PACKAGE_CHUNK_MISSING'; end if;
  if actual_hash<>package_row.expected_content_hash then raise exception 'DATA_PACKAGE_CONTENT_HASH_MISMATCH'; end if;
  if exists(select 1 from timegenie.data_copy_entity_catalog() catalog where not exists(
    select 1 from timegenie.data_copy_staging_chunks chunk where chunk.package_id=p_package_id and chunk.entity_name=catalog.entity_name)) then raise exception 'DATA_PACKAGE_ENTITY_MISSING'; end if;
  select coalesce(jsonb_object_agg(entity_name,row_count),'{}'::jsonb) into counts_value from (
    select entity_name,sum(row_count)::integer row_count from timegenie.data_copy_staging_chunks where package_id=p_package_id group by entity_name
  ) counts;
  subjects_value:=timegenie.data_copy_stage_rows(p_package_id,'subjects');
  schedules_value:=timegenie.data_copy_stage_rows(p_package_id,'subject_work_schedules');
  tasks_value:=timegenie.data_copy_stage_rows(p_package_id,'tasks');
  if exists(select 1 from jsonb_array_elements(schedules_value) row_value where not exists(select 1 from jsonb_array_elements(subjects_value) subject where subject->>'id'=row_value->>'subject_id')) then raise exception 'DATA_PACKAGE_FOREIGN_KEY_INVALID: subject_work_schedules.subject_id'; end if;
  if exists(select 1 from jsonb_array_elements(timegenie.data_copy_stage_rows(p_package_id,'subject_work_schedule_segments')) row_value where not exists(select 1 from jsonb_array_elements(schedules_value) schedule where schedule->>'id'=row_value->>'schedule_id')) then raise exception 'DATA_PACKAGE_FOREIGN_KEY_INVALID: subject_work_schedule_segments.schedule_id'; end if;
  if exists(select 1 from jsonb_array_elements(tasks_value) row_value where not exists(select 1 from jsonb_array_elements(subjects_value) subject where subject->>'id'=row_value->>'subject_id') or (nullif(row_value->>'parent_id','') is not null and not exists(select 1 from jsonb_array_elements(tasks_value) parent where parent->>'id'=row_value->>'parent_id'))) then raise exception 'DATA_PACKAGE_FOREIGN_KEY_INVALID: tasks'; end if;
  if exists(select 1 from jsonb_array_elements(timegenie.data_copy_stage_rows(p_package_id,'time_segments')) row_value where not exists(select 1 from jsonb_array_elements(timegenie.data_copy_stage_rows(p_package_id,'time_entries')) entry where entry->>'id'=row_value->>'entry_id')) then raise exception 'DATA_PACKAGE_FOREIGN_KEY_INVALID: time_segments.entry_id'; end if;
  if exists(select 1 from jsonb_array_elements(timegenie.data_copy_stage_rows(p_package_id,'time_allocations')) row_value where not exists(select 1 from jsonb_array_elements(timegenie.data_copy_stage_rows(p_package_id,'time_entries')) entry where entry->>'id'=row_value->>'entry_id') or not exists(select 1 from jsonb_array_elements(tasks_value) task where task->>'id'=row_value->>'task_id')) then raise exception 'DATA_PACKAGE_FOREIGN_KEY_INVALID: time_allocations'; end if;
  if exists(select 1 from jsonb_array_elements(timegenie.data_copy_stage_rows(p_package_id,'report_tasks')) row_value where not exists(select 1 from jsonb_array_elements(timegenie.data_copy_stage_rows(p_package_id,'reports')) report where report->>'id'=row_value->>'report_id') or not exists(select 1 from jsonb_array_elements(tasks_value) task where task->>'id'=row_value->>'task_id')) then raise exception 'DATA_PACKAGE_FOREIGN_KEY_INVALID: report_tasks'; end if;
  update timegenie.data_copy_staging_packages set status='validated',entity_counts=counts_value,validated_at=now(),failure_code=null where id=p_package_id;
  return jsonb_build_object('packageId',p_package_id,'status','validated','entityCounts',counts_value,'contentHash',actual_hash);
exception when others then
  update timegenie.data_copy_staging_packages set status='invalid',failure_code=left(sqlerrm,240) where id=p_package_id;
  raise;
end $$;

create or replace function timegenie.data_copy_stage_discard(p_workspace_id uuid,p_device_id uuid,p_expected_epoch bigint,p_package_id uuid)
returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare deleted_count integer;
begin
  perform timegenie.assert_direct_v2_workspace(p_workspace_id,p_device_id,p_expected_epoch);
  delete from timegenie.data_copy_staging_packages where id=p_package_id and workspace_id=p_workspace_id and device_id=p_device_id;
  get diagnostics deleted_count=row_count;
  return jsonb_build_object('packageId',p_package_id,'discarded',deleted_count=1);
end $$;

revoke all on timegenie.data_copy_staging_packages,timegenie.data_copy_staging_chunks from public,anon,authenticated;
revoke execute on function timegenie.data_copy_entity_catalog(),timegenie.data_copy_stage_rows(uuid,text) from public,anon,authenticated;
revoke execute on function timegenie.data_copy_stage_begin(uuid,uuid,bigint,integer,bigint,integer,text),timegenie.data_copy_stage_chunk_put(uuid,uuid,bigint,uuid,integer,text,text,text,integer),timegenie.data_copy_stage_validate(uuid,uuid,bigint,uuid),timegenie.data_copy_stage_discard(uuid,uuid,bigint,uuid) from public,anon;
grant execute on function timegenie.data_copy_stage_begin(uuid,uuid,bigint,integer,bigint,integer,text),timegenie.data_copy_stage_chunk_put(uuid,uuid,bigint,uuid,integer,text,text,text,integer),timegenie.data_copy_stage_validate(uuid,uuid,bigint,uuid),timegenie.data_copy_stage_discard(uuid,uuid,bigint,uuid) to authenticated;

commit;
notify pgrst,'reload schema';
