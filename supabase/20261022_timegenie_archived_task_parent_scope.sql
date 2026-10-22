begin;

create or replace function timegenie.validate_task_day_instance_scope() returns trigger
language plpgsql set search_path = timegenie, pg_catalog as $$
begin
  if new.parent_id is not null and not exists (
    select 1 from timegenie.tasks parent
    where parent.id = new.parent_id
      and parent.workspace_id = new.workspace_id
      and parent.subject_id = new.subject_id
      and (new.deleted_at is not null or parent.deleted_at is null)
      and parent.day_instance_date is not distinct from new.day_instance_date
  ) then
    raise exception 'task parent must be in the same day instance scope';
  end if;
  return new;
end $$;

drop trigger if exists trg_tasks_day_instance_scope on timegenie.tasks;
create trigger trg_tasks_day_instance_scope
before insert or update of parent_id, day_instance_date, day_instance_kind, subject_id, deleted_at
on timegenie.tasks for each row execute function timegenie.validate_task_day_instance_scope();

commit;
