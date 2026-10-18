-- TimeGenie operation history defaults to the UI's fixed 50-row page size.
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

revoke execute on function timegenie.cloud_operation_history_list(uuid,uuid,integer,timestamptz,uuid,timestamptz,timestamptz,uuid,text,text,text) from public,anon;
grant execute on function timegenie.cloud_operation_history_list(uuid,uuid,integer,timestamptz,uuid,timestamptz,timestamptz,uuid,text,text,text) to authenticated;
notify pgrst,'reload schema';
