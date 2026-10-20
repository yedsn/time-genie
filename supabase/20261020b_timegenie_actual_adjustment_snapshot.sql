-- The deployed snapshot wrapper predates actual_time_adjustments. Without this
-- collection the desktop cache would replace its local adjustments with [].
do $$ begin
  if to_regprocedure('timegenie.cloud_snapshot_get_pre_actual_adjustments(uuid)') is null then
    alter function timegenie.cloud_snapshot_get(uuid) rename to cloud_snapshot_get_pre_actual_adjustments;
  end if;
end $$;

create or replace function timegenie.cloud_snapshot_get(p_workspace_id uuid)
returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare result_value jsonb;
begin
  result_value:=timegenie.cloud_snapshot_get_pre_actual_adjustments(p_workspace_id);
  return result_value||jsonb_build_object(
    'actual_time_adjustments',coalesce((select jsonb_agg(to_jsonb(row_value) order by work_date,task_id,id)
      from timegenie.actual_time_adjustments row_value where workspace_id=p_workspace_id),'[]'::jsonb)
  );
end $$;

do $$ begin
  if to_regprocedure('timegenie.cloud_incremental_entity_get_pre_actual_adjustments(uuid,text,text)') is null then
    alter function timegenie.cloud_incremental_entity_get(uuid,text,text) rename to cloud_incremental_entity_get_pre_actual_adjustments;
  end if;
end $$;

create or replace function timegenie.cloud_incremental_entity_get(
  p_workspace_id uuid,p_entity_type text,p_entity_id text
) returns jsonb language plpgsql stable security definer set search_path=timegenie,extensions,pg_catalog as $$
declare entity_value jsonb;
begin
  if p_entity_type<>'actual_time_adjustments' then
    return timegenie.cloud_incremental_entity_get_pre_actual_adjustments(p_workspace_id,p_entity_type,p_entity_id);
  end if;
  if not timegenie.is_workspace_owner(p_workspace_id) then raise exception 'AUTH_REQUIRED'; end if;
  select to_jsonb(row_value) into entity_value from timegenie.actual_time_adjustments row_value
  where row_value.workspace_id=p_workspace_id and row_value.id::text=p_entity_id;
  return jsonb_build_object(
    'entity_type','actual_time_adjustments','source_entity_type','actual_time_adjustments',
    'entity_id',p_entity_id,'deleted',entity_value is null,'data',entity_value
  );
end $$;

do $$ begin
  if to_regprocedure('timegenie.migration_import_snapshot_pre_actual_adjustments(uuid,uuid,uuid,jsonb)') is null then
    alter function timegenie.migration_import_snapshot(uuid,uuid,uuid,jsonb) rename to migration_import_snapshot_pre_actual_adjustments;
  end if;
end $$;

create or replace function timegenie.migration_import_snapshot(
  p_workspace_id uuid,p_operation_id uuid,p_device_id uuid,p_snapshot jsonb
) returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare result_value jsonb; row_data jsonb;
begin
  result_value:=timegenie.migration_import_snapshot_pre_actual_adjustments(
    p_workspace_id,p_operation_id,p_device_id,p_snapshot
  );
  for row_data in select value from jsonb_array_elements(coalesce(p_snapshot->'actual_time_adjustments','[]'::jsonb)) loop
    insert into timegenie.actual_time_adjustments(
      id,workspace_id,subject_id,task_id,work_date,delta_minutes,baseline_minutes,target_minutes,note,
      source_type,batch_id,created_at,updated_at,version,created_by_device_id,updated_by_device_id,deleted_at
    ) values(
      (row_data->>'id')::uuid,p_workspace_id,(row_data->>'subject_id')::uuid,(row_data->>'task_id')::uuid,
      (row_data->>'work_date')::date,(row_data->>'delta_minutes')::integer,(row_data->>'baseline_minutes')::integer,
      (row_data->>'target_minutes')::integer,nullif(row_data->>'note',''),'quick_actual_adjustment',
      (row_data->>'batch_id')::uuid,to_timestamp((row_data->>'created_at')::double precision/1000),
      to_timestamp((row_data->>'updated_at')::double precision/1000),coalesce((row_data->>'version')::bigint,1),
      p_device_id,p_device_id,
      case when row_data->>'deleted_at' is null then null else to_timestamp((row_data->>'deleted_at')::double precision/1000) end
    ) on conflict(id) do update set
      subject_id=excluded.subject_id,task_id=excluded.task_id,work_date=excluded.work_date,
      delta_minutes=excluded.delta_minutes,baseline_minutes=excluded.baseline_minutes,
      target_minutes=excluded.target_minutes,note=excluded.note,batch_id=excluded.batch_id,
      updated_at=excluded.updated_at,version=excluded.version,deleted_at=excluded.deleted_at;
  end loop;
  return result_value;
end $$;

revoke execute on function timegenie.cloud_snapshot_get_pre_actual_adjustments(uuid) from public,anon,authenticated;
revoke execute on function timegenie.cloud_incremental_entity_get_pre_actual_adjustments(uuid,text,text) from public,anon,authenticated;
revoke execute on function timegenie.migration_import_snapshot_pre_actual_adjustments(uuid,uuid,uuid,jsonb) from public,anon,authenticated;
revoke execute on function timegenie.cloud_snapshot_get(uuid) from public,anon;
revoke execute on function timegenie.cloud_incremental_entity_get(uuid,text,text) from public,anon,authenticated;
revoke execute on function timegenie.migration_import_snapshot(uuid,uuid,uuid,jsonb) from public,anon;
grant execute on function timegenie.cloud_snapshot_get(uuid) to authenticated;
grant execute on function timegenie.migration_import_snapshot(uuid,uuid,uuid,jsonb) to authenticated;

notify pgrst,'reload schema';
