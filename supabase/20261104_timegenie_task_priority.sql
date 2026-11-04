begin;

alter table timegenie.tasks
  add column if not exists priority text not null default 'normal';

update timegenie.tasks
set priority = 'normal'
where priority is null or priority not in ('urgent','high','normal','low');

alter table timegenie.tasks
  drop constraint if exists tasks_priority_check;
alter table timegenie.tasks
  add constraint tasks_priority_check check (priority in ('urgent','high','normal','low'));

commit;
