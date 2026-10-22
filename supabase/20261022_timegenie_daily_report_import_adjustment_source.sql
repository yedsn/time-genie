begin;
alter table timegenie.actual_time_adjustments drop constraint if exists actual_time_adjustments_source_type_check;
alter table timegenie.actual_time_adjustments add constraint actual_time_adjustments_source_type_check
  check (source_type in ('quick_actual_adjustment','daily_report_import'));
commit;
notify pgrst, 'reload schema';
