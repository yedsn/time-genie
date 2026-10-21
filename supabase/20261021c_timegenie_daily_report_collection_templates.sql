begin;

do $$
declare workspace_record record; client_generation bigint;
begin
  for workspace_record in
    select id, workspace_epoch
    from timegenie.workspaces
    where deleted_at is null
  loop
    client_generation := 0;
    if exists(
      select 1 from pg_attribute
      where attrelid = 'timegenie.workspaces'::regclass
        and attname = 'minimum_client_schema_generation' and not attisdropped
    ) then
      execute 'select minimum_client_schema_generation from timegenie.workspaces where id = $1'
        into client_generation using workspace_record.id;
    end if;
    perform set_config(
      'request.headers',
      jsonb_build_object(
        'x-timegenie-workspace-epoch', workspace_record.workspace_epoch::text,
        'x-timegenie-client-schema-generation', client_generation::text
      )::text,
      true
    );
    update timegenie.report_templates
    set content = E'{{统计信息}}\n\n{{日报合集}}\n',
        description = '合并周期内已保存日报的最终内容',
        updated_at = now(),
        version = version + 1
    where workspace_id = workspace_record.id
      and is_builtin
      and report_type in ('weekly', 'monthly')
      and deleted_at is null
      and (content is distinct from E'{{统计信息}}\n\n{{日报合集}}\n'
           or description is distinct from '合并周期内已保存日报的最终内容');
  end loop;
end $$;

notify pgrst, 'reload schema';
commit;
