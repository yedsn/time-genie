-- Allow day instances created directly in Today to have no long-running source.
begin;

create or replace function timegenie.validate_task_day_instance_source() returns trigger
language plpgsql set search_path=timegenie,pg_catalog as $$
begin
  if new.day_instance_source_task_id is not null and not exists(
    select 1 from timegenie.tasks source
    where source.id=new.day_instance_source_task_id
      and source.workspace_id=new.workspace_id
      and source.subject_id=new.subject_id
      and source.day_instance_date is null
  ) then
    raise exception 'day instance source must be an ordinary task in the same workspace and subject';
  end if;
  return new;
end $$;

drop trigger if exists trg_tasks_day_instance_source on timegenie.tasks;
create trigger trg_tasks_day_instance_source
before insert or update of workspace_id,subject_id,day_instance_date,day_instance_source_task_id
on timegenie.tasks for each row execute function timegenie.validate_task_day_instance_source();

revoke execute on function timegenie.validate_task_day_instance_source() from public,anon,authenticated;

commit;
notify pgrst,'reload schema';
