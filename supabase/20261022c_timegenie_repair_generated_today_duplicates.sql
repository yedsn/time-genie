-- One-time repair for the production records generated while the cloud RPC
-- discarded today-instance identity. Every target id is asserted explicitly.
begin;

do $$
declare source_id constant uuid:='01a0fc2d-5b01-75f5-a86c-8db02090afd2';
declare instance_20261021 constant uuid:='01a15d40-77d6-731f-bb01-fed83d324677';
declare instance_20261022 constant uuid:='01a16068-2051-7387-8f25-709b5b366085';
declare duplicate_ids uuid[]:=array[
  '01a15a23-9913-70d5-92b5-78bf98929edb','01a15f30-dee6-752c-9659-d3c2199d8b53',
  '01a16068-2051-7387-8f25-709fdf3c300f','01a16068-2807-7490-beb2-e89e33e3b3c9',
  '01a16068-2808-74f2-a559-f44f542f211c','01a16068-b5b0-70c7-8fd0-440f760a5c92',
  '01a16068-b5b1-719c-9bb7-b13462b476da','01a16068-2630-701c-8115-fe29739e2ab5',
  '01a16068-2633-72ff-a2fc-5d5e19a5e280','01a16068-2ead-7616-847c-8111b2d8ed1c',
  '01a16068-2eb3-760b-8f7a-fdf63999772a','01a16153-cc00-72a0-b75a-d287447caf62',
  '01a16153-cc02-7006-911d-8c5d75f7454a','01a16153-cc03-72fd-a7be-09c1f2e0bd8b',
  '01a16153-cc04-70c1-9bd3-fc0ae8269ed8','01a16153-cc07-77ee-9690-28b1b247c8bb',
  '01a16153-cc09-7392-a690-20772eb89299','01a16153-cc0b-727f-a835-afad756e43bb',
  '01a16153-cf09-7482-81ad-2cbd1df1315a','01a16153-d279-722f-9756-b4b53656d057',
  '01a16153-d27a-7585-b5f3-d39ac8b857b4','01a16153-d27a-7585-b5f3-d39f0e28dd25',
  '01a16153-d27a-7585-b5f3-d3a40dd3f7d8','01a16153-d473-7135-ba4e-6b76b7102ae2',
  '01a16177-d978-7321-8fd4-8768c171bb37','01a16177-d99a-74fb-ba41-8f800d8271b5',
  '01a16177-d99c-7339-9f94-2299b3a2c966','01a16177-d99f-7017-be98-18eab559ee2b',
  '01a16177-d9a1-7037-be7f-535e0d23c371','01a16177-df29-761e-856d-d1064cd00be5',
  '01a16177-df29-761e-856d-d10bd23bec64','01a16177-df29-761e-856d-d1102f20d5b3',
  '01a16177-df2a-75b7-9741-e93a69deda6d','01a16177-e134-7772-8ff6-f0d01aa79732',
  '01a16177-e882-772f-9b61-b73943d31b2a','01a16177-e885-752b-a930-0742b3e9ab95',
  '01a16177-e887-7444-84a3-fb2688fbbb07','01a16177-e889-75de-868e-222cd602ed34',
  '01a16177-e934-708f-9a49-09a956f59d22'
]::uuid[];
declare workspace_id_value uuid; subject_id_value uuid; workspace_epoch_value bigint; repaired_at timestamptz:=clock_timestamp();
begin
  select workspace_id,subject_id into strict workspace_id_value,subject_id_value
  from timegenie.tasks where id=source_id and deleted_at is null and day_instance_date is null
    and title='今日计划梳理与方案预备';
  if not exists(select 1 from timegenie.task_recurrence_rules where task_id=source_id and effective_end is null) then
    raise exception 'repair source is no longer the active recurring task';
  end if;
  select workspace_epoch into strict workspace_epoch_value from timegenie.workspaces
  where id=workspace_id_value for update;
  perform set_config('request.headers',jsonb_build_object(
    'x-timegenie-workspace-epoch',workspace_epoch_value::text
  )::text,true);
  if (select count(*) from timegenie.tasks where id=instance_20261021 or id=instance_20261022 or id=any(duplicate_ids))<>cardinality(duplicate_ids)+2 then
    raise exception 'repair target set is incomplete';
  end if;
  if exists(
    select 1 from timegenie.tasks
    where (id=instance_20261021 or id=instance_20261022 or id=any(duplicate_ids))
      and (workspace_id<>workspace_id_value or subject_id<>subject_id_value
        or title<>'今日计划梳理与方案预备' or day_instance_date is not null)
  ) then raise exception 'repair target identity changed'; end if;
  if exists(select 1 from timegenie.time_entries where default_task_id=any(duplicate_ids))
     or exists(select 1 from timegenie.time_allocations where task_id=any(duplicate_ids))
     or exists(select 1 from timegenie.actual_time_adjustments where task_id=any(duplicate_ids))
     or exists(select 1 from timegenie.task_status_events where task_id=any(duplicate_ids))
     or exists(select 1 from timegenie.task_occurrences where task_id=any(duplicate_ids))
     or exists(select 1 from timegenie.task_recurrence_rules where task_id=any(duplicate_ids))
     or exists(select 1 from timegenie.report_tasks where task_id=any(duplicate_ids))
     or exists(select 1 from timegenie.external_bindings where entity_id=any(duplicate_ids))
     or exists(select 1 from timegenie.tasks where parent_id=any(duplicate_ids)) then
    raise exception 'repair targets gained unsupported references';
  end if;

  insert into timegenie.task_daily_estimates(
    workspace_id,task_id,work_date,estimate_minutes,created_at,updated_at,version
  ) select workspace_id_value,instance_20261022,'2026-10-22',max(estimate_minutes),min(created_at),max(updated_at),max(version)
    from timegenie.task_daily_estimates where task_id=any(duplicate_ids) and work_date='2026-10-22'
  having count(*)>0
  on conflict(task_id,work_date) do update set estimate_minutes=excluded.estimate_minutes,
    updated_at=excluded.updated_at,version=greatest(timegenie.task_daily_estimates.version,excluded.version);
  delete from timegenie.task_daily_estimates where task_id=any(duplicate_ids);
  delete from timegenie.task_day_plans where task_id=instance_20261021 or task_id=instance_20261022 or task_id=any(duplicate_ids);

  update timegenie.tasks set deleted_at=coalesce(deleted_at,repaired_at),updated_at=repaired_at,version=version+1,
    day_instance_date=case when id='01a15a23-9913-70d5-92b5-78bf98929edb' then '2026-10-21'::date else '2026-10-22'::date end,
    day_instance_kind='member',day_instance_source_task_id=source_id
  where id=any(duplicate_ids);
  update timegenie.tasks set day_instance_date='2026-10-21',day_instance_kind='member',
    day_instance_source_task_id=source_id,updated_at=repaired_at,version=version+1
  where id=instance_20261021;
  update timegenie.tasks set day_instance_date='2026-10-22',day_instance_kind='member',
    day_instance_source_task_id=source_id,updated_at=repaired_at,version=version+1
  where id=instance_20261022;

  update timegenie.task_day_plans set state='included',instance_task_id=instance_20261021,
    updated_at=repaired_at,version=version+1
  where task_id=source_id and work_date='2026-10-21';
  update timegenie.task_day_plans set state='included',instance_task_id=instance_20261022,
    updated_at=repaired_at,version=version+1
  where task_id=source_id and work_date='2026-10-22';

  if (select count(*) from timegenie.tasks where workspace_id=workspace_id_value and subject_id=subject_id_value
      and title='今日计划梳理与方案预备' and deleted_at is null and day_instance_date is null)<>1 then
    raise exception 'repair did not leave exactly one ordinary source task';
  end if;
  if (select count(*) from timegenie.tasks where workspace_id=workspace_id_value
      and day_instance_source_task_id=source_id and deleted_at is null)<>2 then
    raise exception 'repair did not leave exactly two active dated instances';
  end if;
  if (select count(*) from timegenie.task_day_plans where task_id=source_id
      and ((work_date='2026-10-21' and state='included' and instance_task_id=instance_20261021)
        or (work_date='2026-10-22' and state='included' and instance_task_id=instance_20261022)))<>2 then
    raise exception 'repair did not map both source day plans to their instances';
  end if;
end $$;

commit;
notify pgrst,'reload schema';
