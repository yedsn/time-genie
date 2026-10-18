begin;

create or replace function timegenie.cloud_operation_history_detail(
  p_workspace_id uuid,
  p_device_id uuid,
  p_log_id uuid
)
returns jsonb
language plpgsql
stable
security definer
set search_path=timegenie,extensions,pg_catalog
as $$
declare result_value jsonb;
begin
  if not timegenie.is_registered_device(p_workspace_id,p_device_id) then
    raise exception 'DEVICE_NOT_REGISTERED';
  end if;

  select jsonb_build_object(
    'id',log.id,
    'operationId',log.operation_id,
    'businessType',log.business_type,
    'source',log.source,
    'deviceId',log.device_id,
    'deviceNameSnapshot',log.device_name_snapshot,
    'summary',log.summary,
    'affectedEntityCount',log.affected_entity_count,
    'metadata',log.metadata,
    'occurredAt',log.occurred_at,
    'expiresAt',log.expires_at,
    'items',coalesce((
      select jsonb_agg(jsonb_build_object(
        'id',item.id,
        'entityType',item.entity_type,
        'entityId',item.entity_id,
        'entityLabel',item.entity_label_snapshot,
        'action',item.action,
        'before',item.before_json,
        'after',item.after_json,
        'changedFields',item.changed_fields
      ) order by item.id)
      from timegenie.cloud_operation_log_items item
      where item.operation_log_id=log.id
    ),'[]'::jsonb)
  ) into result_value
  from timegenie.cloud_operation_logs log
  where log.workspace_id=p_workspace_id and log.id=p_log_id;

  if result_value is null then raise exception 'OPERATION_LOG_NOT_FOUND'; end if;
  return result_value;
end $$;

revoke execute on function timegenie.cloud_operation_history_detail(uuid,uuid,uuid) from public,anon;
grant execute on function timegenie.cloud_operation_history_detail(uuid,uuid,uuid) to authenticated;

alter function timegenie.workspace_snapshot_create(uuid,uuid,uuid,text,text,text,boolean)
  set statement_timeout='120s';
alter function timegenie.workspace_snapshot_ensure_automatic_at(uuid,uuid,uuid,timestamptz)
  set statement_timeout='120s';
alter function timegenie.workspace_snapshot_ensure_automatic(uuid,uuid,uuid)
  set statement_timeout='120s';

notify pgrst,'reload schema';

commit;
