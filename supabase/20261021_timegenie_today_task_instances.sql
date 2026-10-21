begin;

alter table timegenie.tasks add column if not exists day_instance_date date;
alter table timegenie.tasks add column if not exists day_instance_kind text;
alter table timegenie.tasks add column if not exists day_instance_source_task_id uuid;
alter table timegenie.task_day_plans add column if not exists instance_task_id uuid;

do $$ begin
  alter table timegenie.tasks add constraint tasks_day_instance_kind_check
    check (day_instance_kind is null or day_instance_kind in ('member', 'context'));
exception when duplicate_object then null; end $$;

do $$ begin
  alter table timegenie.tasks add constraint tasks_day_instance_pair_check
    check ((day_instance_date is null) = (day_instance_kind is null));
exception when duplicate_object then null; end $$;

do $$ begin
  alter table timegenie.tasks add constraint tasks_day_instance_source_check
    check (day_instance_source_task_id is null or day_instance_date is not null);
exception when duplicate_object then null; end $$;

do $$ begin
  alter table timegenie.tasks add constraint tasks_day_instance_source_task_fk
    foreign key (day_instance_source_task_id) references timegenie.tasks(id);
exception when duplicate_object then null; end $$;

do $$ begin
  alter table timegenie.task_day_plans add constraint task_day_plans_instance_task_fk
    foreign key (instance_task_id) references timegenie.tasks(id);
exception when duplicate_object then null; end $$;

create index if not exists idx_tasks_day_instances
  on timegenie.tasks(workspace_id, subject_id, day_instance_date, parent_id, sort_order);
create unique index if not exists uq_tasks_day_instance_source
  on timegenie.tasks(workspace_id, day_instance_source_task_id, day_instance_date)
  where deleted_at is null and day_instance_source_task_id is not null and day_instance_date is not null;

create or replace function timegenie.validate_task_day_instance_scope() returns trigger
language plpgsql set search_path = timegenie, pg_catalog as $$
begin
  if new.parent_id is not null and not exists (
    select 1 from timegenie.tasks parent
    where parent.id = new.parent_id
      and parent.workspace_id = new.workspace_id
      and parent.subject_id = new.subject_id
      and parent.deleted_at is null
      and parent.day_instance_date is not distinct from new.day_instance_date
  ) then
    raise exception 'task parent must be in the same day instance scope';
  end if;
  return new;
end $$;

drop trigger if exists trg_tasks_day_instance_scope on timegenie.tasks;
create trigger trg_tasks_day_instance_scope
before insert or update of parent_id, day_instance_date, day_instance_kind, subject_id
on timegenie.tasks for each row execute function timegenie.validate_task_day_instance_scope();

commit;
