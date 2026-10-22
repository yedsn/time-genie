-- Keep full snapshots compatible with clients that restore task parent links
-- while iterating the task array. Sources and roots must precede dependents.
begin;

do $$ begin
  if to_regprocedure('timegenie.cloud_snapshot_get_pre_task_order(uuid)') is null then
    alter function timegenie.cloud_snapshot_get(uuid) rename to cloud_snapshot_get_pre_task_order;
  end if;
end $$;

create or replace function timegenie.cloud_snapshot_get(p_workspace_id uuid)
returns jsonb language plpgsql security definer set search_path=timegenie,extensions,pg_catalog as $$
declare result_value jsonb;
begin
  result_value:=timegenie.cloud_snapshot_get_pre_task_order(p_workspace_id);
  return result_value||jsonb_build_object(
    'tasks',coalesce((
      with recursive ordered_tasks as (
        select task.*,0 as tree_depth
        from timegenie.tasks task
        where task.workspace_id=p_workspace_id and task.parent_id is null
        union all
        select child.*,parent.tree_depth+1
        from timegenie.tasks child
        join ordered_tasks parent on parent.id=child.parent_id
        where child.workspace_id=p_workspace_id
      )
      select jsonb_agg(to_jsonb(task) - 'tree_depth' order by
        case when task.day_instance_date is null then 0 else 1 end,
        task.tree_depth,task.parent_id nulls first,task.sort_order,task.created_at,task.id
      ) from ordered_tasks task
    ),'[]'::jsonb)
  );
end $$;

revoke execute on function timegenie.cloud_snapshot_get_pre_task_order(uuid) from public,anon,authenticated;
revoke execute on function timegenie.cloud_snapshot_get(uuid) from public,anon;
grant execute on function timegenie.cloud_snapshot_get(uuid) to authenticated;

commit;
notify pgrst,'reload schema';
