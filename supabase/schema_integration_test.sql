\set ON_ERROR_STOP on

-- Run in a disposable PostgreSQL database. The harness must provide
-- auth.users, auth.uid(), and the supabase_realtime publication. Supabase
-- already has anon/authenticated roles; plain PostgreSQL test databases do not.

do $$
begin
  if not exists(select 1 from pg_roles where rolname = 'anon') then
    create role anon nologin;
  end if;
  if not exists(select 1 from pg_roles where rolname = 'authenticated') then
    create role authenticated nologin;
  end if;
end $$;

create schema if not exists auth;
create table if not exists auth.users(id uuid primary key);
create or replace function auth.uid()
returns uuid language sql stable as $$
  select nullif(current_setting('request.jwt.claim.sub', true), '')::uuid;
$$;

do $$
begin
  if not exists(select 1 from pg_publication where pubname = 'supabase_realtime') then
    create publication supabase_realtime;
  end if;
end $$;

-- Reapply the idempotent schema after creating the Data API roles so this test
-- exercises the same explicit grants and revokes used by a real Supabase project.
\ir schema.sql
\ir 20261004_timegenie_cloud_sync_patch.sql
\ir 20261004b_timegenie_migration_target_guard_patch.sql
\ir 20261004c_timegenie_work_day_version_guard_patch.sql
\ir 20261004d_timegenie_session_revocation_patch.sql
\ir 20261004_verify_timegenie_cloud_guards.sql
\ir 20261005_timegenie_timer_sync_coordination_patch.sql
\ir 20261005_timegenie_cloud_task_planning_patch.sql
\ir 20261005b_timegenie_event_driven_sync_patch.sql
\ir 20261005b_timegenie_event_driven_sync_patch.sql
\ir 20261006_timegenie_shared_unassigned_patch.sql
\ir 20261006_timegenie_shared_unassigned_patch.sql
\ir 20261006b_timegenie_calendar_day_accounting_patch.sql
\ir 20261006b_timegenie_calendar_day_accounting_patch.sql
\ir 20261007_timegenie_unassigned_segment_boundary_fix.sql
\ir 20261007_timegenie_unassigned_segment_boundary_fix.sql
\ir 20261008_timegenie_simplified_unassigned_anchor.sql
\ir 20261008_timegenie_simplified_unassigned_anchor.sql
\ir 20261013_timegenie_unassigned_lease_continuity_fix.sql
\ir 20261013_timegenie_unassigned_lease_continuity_fix.sql
\ir 20261014_timegenie_timer_sync_authority.sql
\ir 20261014_timegenie_timer_sync_authority.sql
\ir 20261018_timegenie_timer_write_lease_takeover.sql
\ir 20261018_timegenie_timer_write_lease_takeover.sql
\ir 20261018f_timegenie_timer_fact_convergence.sql
\ir 20261018f_timegenie_timer_fact_convergence.sql
\ir 20261015_timegenie_actual_adjustment_batch.sql
\ir 20261015_timegenie_actual_adjustment_batch.sql
\ir 20261016_timegenie_named_template_bootstrap_fix.sql
\ir 20261016_timegenie_named_template_bootstrap_fix.sql
\ir 20261016b_timegenie_timer_transition.sql
\ir 20261016b_timegenie_timer_transition.sql
\ir 20261018b_timegenie_operation_history.sql
\ir 20261018b_timegenie_operation_history.sql
\ir 20261018c_timegenie_workspace_snapshots.sql
\ir 20261018c_timegenie_workspace_snapshots.sql
\ir 20261018d_timegenie_workspace_epoch.sql
\ir 20261018d_timegenie_workspace_epoch.sql
\ir 20261018e_timegenie_workspace_restore.sql
\ir 20261018e_timegenie_workspace_restore.sql
\ir 20261018g_timegenie_automatic_snapshot_schedule.sql
\ir 20261018g_timegenie_automatic_snapshot_schedule.sql
\ir 20261018h_timegenie_operation_history_page_size.sql
\ir 20261018h_timegenie_operation_history_page_size.sql
\ir 20261018i_timegenie_history_detail_and_snapshot_timeout.sql
\ir 20261018i_timegenie_history_detail_and_snapshot_timeout.sql
\ir 20261018j_timegenie_snapshot_retention.sql
\ir 20261018j_timegenie_snapshot_retention.sql
\ir 20261018k_timegenie_fixed_daily_snapshot.sql
\ir 20261018k_timegenie_fixed_daily_snapshot.sql
\ir 20261018l_timegenie_snapshot_metadata_detail.sql
\ir 20261018l_timegenie_snapshot_metadata_detail.sql
\ir 20261018m_timegenie_restore_ignore_unassigned.sql
\ir 20261018m_timegenie_restore_ignore_unassigned.sql
\ir 20261018n_timegenie_restore_timeout.sql
\ir 20261018n_timegenie_restore_timeout.sql
\ir 20261018o_timegenie_optional_pre_restore_snapshot.sql
\ir 20261018o_timegenie_optional_pre_restore_snapshot.sql
\ir 20261018p_timegenie_restore_unassigned_state.sql
\ir 20261018p_timegenie_restore_unassigned_state.sql
\ir 20261019_timegenie_timer_recurring_occurrence.sql
\ir 20261019_timegenie_timer_recurring_occurrence.sql
\ir 20261020_timegenie_actual_adjustment_target_rebase.sql
\ir 20261020_timegenie_actual_adjustment_target_rebase.sql
\ir 20261020b_timegenie_actual_adjustment_snapshot.sql
\ir 20261020b_timegenie_actual_adjustment_snapshot.sql
\ir 20261021_timegenie_today_task_instances.sql
\ir 20261021_timegenie_today_task_instances.sql
\ir 20261021b_timegenie_client_write_generation.sql
\ir 20261021b_timegenie_client_write_generation.sql
\ir 20261022b_timegenie_today_instance_protocol.sql
\ir 20261022b_timegenie_today_instance_protocol.sql
\ir 20261022d_timegenie_snapshot_task_order.sql
\ir 20261022d_timegenie_snapshot_task_order.sql
\ir 20261022e_timegenie_ignore_context_day_plans.sql
\ir 20261022e_timegenie_ignore_context_day_plans.sql
\ir 20261022f_timegenie_direct_cloud_storage_protocol.sql
\ir 20261022f_timegenie_direct_cloud_storage_protocol.sql
\ir 20261022g_timegenie_direct_cloud_subject_task_mutations.sql
\ir 20261022g_timegenie_direct_cloud_subject_task_mutations.sql
\ir 20261022h_timegenie_direct_cloud_task_planning_mutations.sql
\ir 20261022h_timegenie_direct_cloud_task_planning_mutations.sql
\ir 20261022i_timegenie_direct_cloud_recurrence_mutations.sql
\ir 20261022j_timegenie_direct_cloud_task_domain_routing.sql
\ir 20261022i_timegenie_direct_cloud_recurrence_mutations.sql
\ir 20261022j_timegenie_direct_cloud_task_domain_routing.sql
\ir 20261022k_timegenie_direct_cloud_timer_mutations.sql
\ir 20261022k_timegenie_direct_cloud_timer_mutations.sql
\ir 20261022l_timegenie_direct_cloud_timer_recovery.sql
\ir 20261022l_timegenie_direct_cloud_timer_recovery.sql
\ir 20261022m_timegenie_direct_cloud_unassigned_mutations.sql
\ir 20261022m_timegenie_direct_cloud_unassigned_mutations.sql
\ir 20261022n_timegenie_direct_cloud_manual_actual_mutations.sql
\ir 20261022n_timegenie_direct_cloud_manual_actual_mutations.sql
\ir 20261022o_timegenie_direct_cloud_time_entry_mutations.sql
\ir 20261022o_timegenie_direct_cloud_time_entry_mutations.sql
\ir 20261023a_timegenie_direct_cloud_report_mutations.sql
\ir 20261023a_timegenie_direct_cloud_report_mutations.sql
\ir 20261023b_timegenie_direct_cloud_report_template_mutations.sql
\ir 20261023b_timegenie_direct_cloud_report_template_mutations.sql
\ir 20261023c_timegenie_direct_cloud_shared_settings_mutations.sql
\ir 20261023c_timegenie_direct_cloud_shared_settings_mutations.sql
\ir 20261023d_timegenie_direct_cloud_integration_mutations.sql
\ir 20261023d_timegenie_direct_cloud_integration_mutations.sql
\ir 20261023i_timegenie_direct_cloud_today_overview.sql
\ir 20261023i_timegenie_direct_cloud_today_overview.sql
\ir 20261023j_timegenie_direct_cloud_import_export.sql
\ir 20261023j_timegenie_direct_cloud_import_export.sql
\ir 20261023k_timegenie_direct_cloud_release_guard.sql
\ir 20261023l_timegenie_direct_cloud_workspace_restore.sql
\ir 20261023l_timegenie_direct_cloud_workspace_restore.sql
\ir 20261023k_timegenie_direct_cloud_release_guard.sql
\ir 20261023m_timegenie_remove_client_generation_gate.sql
\ir 20261023m_timegenie_remove_client_generation_gate.sql

do $$
begin
  if has_table_privilege('anon', 'timegenie.workspaces', 'select') then
    raise exception 'anon can read business tables';
  end if;
  if has_function_privilege('anon', 'timegenie.cloud_snapshot_get(uuid)', 'execute') then
    raise exception 'anon can execute protected RPCs';
  end if;
  if has_function_privilege('anon', 'timegenie.record_processed_operation(uuid, uuid, uuid, text, jsonb, jsonb)', 'execute') then
    raise exception 'anon can execute internal idempotency helper';
  end if;
  if has_function_privilege('anon', 'timegenie.storage_protocol_get(uuid)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_context(uuid, uuid, bigint)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_workbench_bootstrap(uuid, uuid, bigint)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_task_list(uuid, uuid, bigint, uuid, boolean, text, date)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_completed_tasks(uuid, uuid, bigint, uuid, timestamptz, uuid, integer)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_task_detail(uuid, uuid, uuid, bigint)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_today_plan(uuid, uuid, uuid, date, bigint)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_today_overview(uuid, uuid, uuid, date, integer, integer, bigint)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_time_state(uuid, uuid, date, bigint)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_time_history(uuid, uuid, bigint, date, integer, boolean)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_time_history_page(uuid, uuid, integer, boolean, bigint)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_time_entry_update(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_time_entry_disposition(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_timer_start(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_timer_pause(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_timer_resume(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_timer_stop(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_timer_transition(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_time_allocation_replace(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_timer_recover(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_timer_takeover(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_tracking_acquire(uuid, uuid, bigint, uuid)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_tracking_heartbeat(uuid, uuid, bigint, uuid)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_timer_recover_locked(uuid, uuid, uuid, timestamptz)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_tracking_claim_lease(uuid, uuid, uuid)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_active_timer(uuid, uuid, bigint)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_unassigned_ensure(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_unassigned_resolve(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_unassigned_state(uuid, uuid, uuid, bigint)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_actual_adjustment_snapshot(uuid, uuid, uuid, date, boolean, bigint)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_manual_time_create(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_actual_adjustment_batch_apply(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_actual_adjustment_delete(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_report_list(uuid, uuid, bigint, text, uuid, text, timestamptz, uuid, integer)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_report_detail(uuid, uuid, uuid, bigint)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_report_templates(uuid, uuid, text, bigint)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_report_preview(uuid, uuid, text, date, uuid, uuid, bigint)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_report_create(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_report_scope_update(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_report_content_save(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_report_regenerate(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_report_delete(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_report_template_create(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_report_template_duplicate(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_report_template_update(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_report_template_set_default(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_report_template_reorder(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_report_template_delete(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_shared_setting_update(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_shared_settings_batch_update(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_work_day_set(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_subject_schedule_save(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_integration_config_save(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_integration_state(uuid, uuid, bigint)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_external_binding_upsert(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_external_binding_delete(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_shared_configuration(uuid, uuid, bigint)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_subject_create(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_subject_update(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_subject_reorder(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_subject_archive(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_subject_restore(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_task_create(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_task_update(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_task_move(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_task_archive_subtree(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_task_restore_subtree(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_task_daily_estimate_set(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_task_daily_estimate_batch_set(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_task_day_plan_set(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_task_day_plan_batch_set(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_recurrence_save(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_recurrence_close(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_recurrence_day_generate(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_recurrence_occurrence_set_status(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_task_set_status(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_today_recurring_update(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_task_duplicate_subtree(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.direct_cloud_task_plan_batch_apply(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or has_function_privilege('anon', 'timegenie.storage_protocol_upgrade_preflight(uuid, uuid)', 'execute')
     or has_function_privilege('anon', 'timegenie.upgrade_storage_protocol(uuid, uuid, uuid, bigint, text, boolean)', 'execute') then
    raise exception 'anon can execute direct storage protocol RPCs';
  end if;
  if not has_function_privilege('authenticated', 'timegenie.storage_protocol_get(uuid)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_context(uuid, uuid, bigint)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_workbench_bootstrap(uuid, uuid, bigint)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_task_list(uuid, uuid, bigint, uuid, boolean, text, date)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_completed_tasks(uuid, uuid, bigint, uuid, timestamptz, uuid, integer)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_task_detail(uuid, uuid, uuid, bigint)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_today_plan(uuid, uuid, uuid, date, bigint)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_today_overview(uuid, uuid, uuid, date, integer, integer, bigint)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_time_state(uuid, uuid, date, bigint)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_time_history(uuid, uuid, bigint, date, integer, boolean)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_time_history_page(uuid, uuid, integer, boolean, bigint)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_time_entry_update(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_time_entry_disposition(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_timer_start(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_timer_pause(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_timer_resume(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_timer_stop(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_timer_transition(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_time_allocation_replace(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_timer_recover(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_timer_takeover(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_tracking_acquire(uuid, uuid, bigint, uuid)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_tracking_heartbeat(uuid, uuid, bigint, uuid)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_timer_recover_locked(uuid, uuid, uuid, timestamptz)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_tracking_claim_lease(uuid, uuid, uuid)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_active_timer(uuid, uuid, bigint)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_unassigned_ensure(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_unassigned_resolve(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_unassigned_state(uuid, uuid, uuid, bigint)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_actual_adjustment_snapshot(uuid, uuid, uuid, date, boolean, bigint)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_manual_time_create(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_actual_adjustment_batch_apply(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_actual_adjustment_delete(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_report_list(uuid, uuid, bigint, text, uuid, text, timestamptz, uuid, integer)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_report_detail(uuid, uuid, uuid, bigint)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_report_templates(uuid, uuid, text, bigint)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_report_preview(uuid, uuid, text, date, uuid, uuid, bigint)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_report_create(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_report_scope_update(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_report_content_save(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_report_regenerate(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_report_delete(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_report_template_create(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_report_template_duplicate(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_report_template_update(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_report_template_set_default(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_report_template_reorder(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_report_template_delete(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_shared_setting_update(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_shared_settings_batch_update(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_work_day_set(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_subject_schedule_save(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_integration_config_save(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_integration_state(uuid, uuid, bigint)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_external_binding_upsert(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_external_binding_delete(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_shared_configuration(uuid, uuid, bigint)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_subject_create(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_subject_update(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_subject_reorder(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_subject_archive(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_subject_restore(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_task_create(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_task_update(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_task_move(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_task_archive_subtree(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_task_restore_subtree(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_task_daily_estimate_set(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_task_daily_estimate_batch_set(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_task_day_plan_set(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_task_day_plan_batch_set(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_recurrence_save(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_recurrence_close(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_recurrence_day_generate(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_recurrence_occurrence_set_status(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_task_set_status(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_today_recurring_update(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_task_duplicate_subtree(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_task_plan_batch_apply(uuid, uuid, uuid, bigint, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.storage_protocol_upgrade_preflight(uuid, uuid)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.direct_cloud_operation_get(uuid, uuid, uuid, text, jsonb)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.upgrade_storage_protocol(uuid, uuid, uuid, bigint, text, boolean)', 'execute') then
    raise exception 'authenticated is missing direct storage protocol RPC privileges';
  end if;
  if has_function_privilege('authenticated', 'timegenie.assert_sync_v1_workspace(uuid)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.assert_direct_v2_workspace(uuid, uuid, bigint)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_task_project(uuid, uuid)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_time_entry_project(uuid, uuid, timestamptz)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_report_project(uuid, uuid)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_sanitize_shared_json(jsonb)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_subject_record(uuid, uuid, boolean)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_task_record(uuid, uuid, boolean)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_mutation_response(uuid, uuid, uuid, jsonb, text[], bigint)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_task_parent_assert(uuid, uuid, uuid, date, uuid)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_daily_estimate_record(uuid, uuid, date)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_day_plan_record(uuid, uuid, date)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_recurrence_matches_date(uuid, uuid, date)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_daily_estimate_move_to_instance(uuid, uuid, uuid, date)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_today_instance_ensure(uuid, uuid, uuid, date, text)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_today_instance_archive(uuid, uuid, uuid, date)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_daily_estimate_apply(uuid, uuid, uuid, date, integer, bigint, bigint, boolean)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_day_plan_apply(uuid, uuid, uuid, date, text, bigint)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_recurrence_rule_record(uuid, uuid)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_occurrence_record(uuid, uuid, date)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_occurrence_ensure(uuid, uuid, date)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_operation_result(uuid, uuid, text, jsonb)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.record_direct_cloud_operation(uuid, uuid, uuid, text, jsonb, jsonb)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_unassigned_session_snapshot(uuid, uuid)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_actual_adjustment_project(uuid, uuid)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_actual_minutes(uuid, uuid, date)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_report_period(text, date)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_report_task_path(uuid, uuid)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_report_scope_task_ids(uuid, uuid, text, date, date, jsonb)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_report_render(uuid, text, uuid, date, date, uuid, uuid[], boolean)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_report_record(uuid, uuid, boolean)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_report_replace_tasks(uuid, uuid, uuid, uuid[])', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_report_template_validate(text, text, text)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_report_template_record(uuid, uuid, boolean)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_shared_setting_validate(text, jsonb)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_shared_setting_apply(uuid, uuid, uuid, text, jsonb, bigint)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_work_day_record(uuid, date)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_integration_reject_sensitive(jsonb, text)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_integration_provider_assert(text)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_integration_record(uuid, text)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_external_binding_record(uuid, text, text, uuid)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_subject_schedule_apply_day(uuid, uuid, jsonb)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_day_time_summary(uuid, date)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.direct_cloud_unassigned_anchor_ensure_locked(uuid, uuid, timestamptz)', 'execute') then
    raise exception 'authenticated can execute internal direct storage helpers';
  end if;
  if has_function_privilege('authenticated', 'timegenie.cloud_apply_patch_pre_direct_v2(uuid, uuid, uuid, text, text, text, bigint, jsonb)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.workspace_restore_commit_pre_direct_v2(uuid, uuid, uuid, uuid, text, bigint, uuid, boolean)', 'execute') then
    raise exception 'authenticated can execute a pre-protocol implementation directly';
  end if;
  if not has_table_privilege('authenticated', 'timegenie.workspaces', 'select,insert') then
    raise exception 'authenticated is missing workspace bootstrap privileges';
  end if;
  if has_table_privilege('authenticated', 'timegenie.tasks', 'insert,update,delete') then
    raise exception 'authenticated can directly write synced business tables';
  end if;
  if has_table_privilege('authenticated', 'timegenie.tasks', 'select') then
    raise exception 'authenticated can directly read synced business tables instead of using snapshot RPC';
  end if;
  if has_table_privilege('authenticated', 'timegenie.devices', 'select,insert,update,delete') then
    raise exception 'authenticated can bypass device lifecycle RPCs with direct table access';
  end if;
  if not has_table_privilege('authenticated', 'timegenie.workspace_changes', 'select') then
    raise exception 'authenticated is missing change feed read access';
  end if;
  if not has_table_privilege('authenticated', 'timegenie.tracking_leases', 'select') then
    raise exception 'authenticated is missing tracking lease read access';
  end if;
  if not has_table_privilege('authenticated', 'timegenie.time_entries', 'select') then
    raise exception 'authenticated is missing timer read access';
  end if;
  if not has_function_privilege('authenticated', 'timegenie.cloud_snapshot_get(uuid)', 'execute') then
    raise exception 'authenticated is missing RPC execute privilege';
  end if;
  if not has_function_privilege('authenticated', 'timegenie.cloud_incremental_pull(uuid, uuid, bigint, integer)', 'execute') then
    raise exception 'authenticated is missing incremental pull RPC execute privilege';
  end if;
  if has_function_privilege('anon', 'timegenie.cloud_incremental_pull(uuid, uuid, bigint, integer)', 'execute') then
    raise exception 'anon can execute incremental pull RPC';
  end if;
  if has_function_privilege('authenticated', 'timegenie.cloud_incremental_entity_get(uuid, text, text)', 'execute') then
    raise exception 'authenticated can execute internal incremental entity helper';
  end if;
  if not has_function_privilege('authenticated', 'timegenie.cloud_apply_patch(uuid, uuid, uuid, text, text, text, bigint, jsonb)', 'execute') then
    raise exception 'authenticated is missing cloud apply RPC execute privilege';
  end if;
  if not has_function_privilege('authenticated', 'timegenie.cloud_apply_patch(uuid, uuid, uuid, text, text, text, bigint, bigint, bigint, jsonb)', 'execute') then
    raise exception 'authenticated is missing coalesced timer apply RPC execute privilege';
  end if;
  if not has_function_privilege('authenticated', 'timegenie.timer_sync_context_get(uuid, uuid, uuid)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.timer_sync_context_get(uuid, uuid, uuid, text, jsonb)', 'execute') then
    raise exception 'authenticated is missing timer sync context privileges';
  end if;
  if not has_function_privilege('authenticated', 'timegenie.tracking_lease_acquire(uuid, uuid, uuid)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.tracking_lease_renew(uuid, uuid, uuid)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.tracking_lease_takeover_for_timer_write(uuid, uuid, uuid)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.tracking_lease_release(uuid, uuid, uuid)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.unassigned_tick(uuid, uuid, uuid)', 'execute') then
    raise exception 'authenticated is missing tracking lease RPC privileges';
  end if;
  if has_function_privilege('authenticated', 'timegenie.apply_calendar_time_entry(uuid, uuid, uuid, text, text, bigint, jsonb, jsonb)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.apply_calendar_time_entry_pre_timer_authority(uuid, uuid, uuid, text, text, bigint, jsonb, jsonb)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.timer_write_lease_context(uuid, uuid)', 'execute') then
    raise exception 'authenticated can execute an internal timer authority helper';
  end if;
  if not has_function_privilege('authenticated', 'timegenie.device_authorize(uuid, uuid, text, text, text)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.device_authorization_get(uuid, uuid)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.device_list(uuid)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.device_revoke(uuid, uuid)', 'execute')
     or not has_function_privilege('authenticated', 'timegenie.device_revoke_all(uuid)', 'execute') then
    raise exception 'authenticated is missing device lifecycle RPC privileges';
  end if;
  if has_function_privilege('anon', 'timegenie.device_revoke(uuid, uuid)', 'execute') then
    raise exception 'anon can revoke devices';
  end if;
  if has_function_privilege('authenticated', 'timegenie.is_workspace_owner(uuid)', 'execute') then
    raise exception 'authenticated can execute internal ownership helper';
  end if;
  if has_function_privilege('authenticated', 'timegenie.is_registered_device(uuid, uuid)', 'execute') then
    raise exception 'authenticated can execute internal device helper';
  end if;
  if has_function_privilege('authenticated', 'timegenie.current_auth_session_id()', 'execute') then
    raise exception 'authenticated can execute internal auth session helper';
  end if;
  if has_function_privilege('authenticated', 'timegenie.touch_workspace_change()', 'execute') then
    raise exception 'authenticated can execute internal trigger helper';
  end if;
  if has_function_privilege('authenticated', 'timegenie.operation_request_hash(text, jsonb)', 'execute') then
    raise exception 'authenticated can execute internal idempotency hash helper';
  end if;
  if has_function_privilege('authenticated', 'timegenie.processed_operation_result(uuid, uuid, text, jsonb)', 'execute') then
    raise exception 'authenticated can execute internal idempotency read helper';
  end if;
  if has_function_privilege('authenticated', 'timegenie.record_processed_operation(uuid, uuid, uuid, text, jsonb, jsonb)', 'execute') then
    raise exception 'authenticated can execute internal idempotency helper';
  end if;
  if not has_function_privilege('authenticated', 'timegenie.actual_adjustment_batch_apply(uuid, uuid, uuid, jsonb)', 'execute') then
    raise exception 'authenticated is missing actual adjustment batch RPC privileges';
  end if;
  if has_function_privilege('anon', 'timegenie.actual_adjustment_batch_apply(uuid, uuid, uuid, jsonb)', 'execute')
     or has_function_privilege('authenticated', 'timegenie.actual_adjustment_fingerprint(uuid, uuid, date)', 'execute') then
    raise exception 'actual adjustment internal privileges are too broad';
  end if;
  if has_function_privilege('authenticated', 'timegenie.unassigned_resolve_non_work(uuid, uuid, uuid, bigint, uuid, text)', 'execute') then
    raise exception 'authenticated can execute internal unassigned helper';
  end if;
end $$;

begin;
select set_config('request.headers', '{}', true);

insert into auth.users(id) values
  ('10000000-0000-0000-0000-000000000001'),
  ('10000000-0000-0000-0000-000000000002');

set role authenticated;
select set_config('request.jwt.claim.sub', '10000000-0000-0000-0000-000000000001', true);
select set_config('request.jwt.claim.session_id', '80000000-0000-0000-0000-000000000001', true);

select set_config('request.jwt.claim.sub', '10000000-0000-0000-0000-000000000002', true);

insert into timegenie.workspaces(id, owner_user_id, name) values
  ('20000000-0000-0000-0000-000000000002', '10000000-0000-0000-0000-000000000002', '账号 B 未归属测试工作空间');

select timegenie.device_authorize('20000000-0000-0000-0000-000000000002', '30000000-0000-0000-0000-000000000003', '设备 C', 'windows', '0.1.0');

select timegenie.cloud_apply_patch(
  '20000000-0000-0000-0000-000000000002',
  '30000000-0000-0000-0000-000000000003',
  '40000000-0000-0000-0000-000000000098',
  'app_setting.update','app_setting','generation_free_write_probe',null,
  jsonb_build_object('key','generation_free_write_probe','value_json','true'::jsonb,'updated_at',1792271999000,'version',1)
);
do $$ begin
  if not exists(
    select 1
    from jsonb_array_elements(
      timegenie.cloud_snapshot_get('20000000-0000-0000-0000-000000000002')->'app_settings'
    ) item
    where item->>'key'='generation_free_write_probe'
  ) then raise exception 'write without client generation header was not persisted'; end if;
end $$;

select timegenie.cloud_apply_patch(
  '20000000-0000-0000-0000-000000000002',
  '30000000-0000-0000-0000-000000000003',
  '40000000-0000-0000-0000-000000000099',
  'unassigned_session.update',
  'unassigned_session',
  '71000000-0000-0000-0000-000000000099',
  null,
  jsonb_build_object(
    'id', '71000000-0000-0000-0000-000000000099',
    'work_date', '2026-09-25',
    'state', 'collecting',
    'threshold_seconds', 300,
    'duration_seconds', 60,
    'first_started_at', 1789779600000,
    'created_at', 1789779600000,
    'updated_at', 1789779660000,
    'version', 1,
    'segments', jsonb_build_array()
  )
);

reset role;
do $$
begin
  begin
    perform timegenie.migration_import_snapshot(
      '20000000-0000-0000-0000-000000000002',
      '40000000-0000-0000-0000-000000000100',
      '30000000-0000-0000-0000-000000000003',
      '{}'::jsonb
    );
    raise exception 'migration accepted a target that only had unassigned cloud data';
  exception
    when others then
      if sqlerrm not like '%MIGRATION_TARGET_NOT_EMPTY%' then raise; end if;
  end;
end $$;

select set_config('request.jwt.claim.sub', '10000000-0000-0000-0000-000000000001', true);

insert into timegenie.workspaces(id, owner_user_id, name) values
  ('20000000-0000-0000-0000-000000000001', '10000000-0000-0000-0000-000000000001', '账号 A 工作空间');

select timegenie.workspace_initialize_defaults('20000000-0000-0000-0000-000000000001');
select timegenie.workspace_initialize_defaults('20000000-0000-0000-0000-000000000001');

do $$
begin
  if (select count(*) from timegenie.report_templates where workspace_id='20000000-0000-0000-0000-000000000001' and deleted_at is null)<>3
     or exists(select 1 from timegenie.report_templates where workspace_id='20000000-0000-0000-0000-000000000001' and nullif(btrim(name),'') is null)
     or (select count(*) from timegenie.report_templates where workspace_id='20000000-0000-0000-0000-000000000001' and is_default and deleted_at is null)<>3 then
    raise exception 'workspace bootstrap did not create exactly three named default report templates';
  end if;
  if (select count(*) from timegenie.subject_work_schedules where workspace_id='20000000-0000-0000-0000-000000000001')<>7
     or (select count(*) from timegenie.subject_work_schedule_segments where workspace_id='20000000-0000-0000-0000-000000000001')<>10 then
    raise exception 'workspace bootstrap did not create an idempotent default work schedule';
  end if;
end $$;

select timegenie.device_authorize('20000000-0000-0000-0000-000000000001', '30000000-0000-0000-0000-000000000001', '设备 A', 'windows', '0.2.1');
reset role;
insert into timegenie.tracking_leases(workspace_id, holder_device_id, lease_token, expires_at)
values('20000000-0000-0000-0000-000000000001', '30000000-0000-0000-0000-000000000001', '50000000-0000-0000-0000-000000000001', now() + interval '45 seconds')
on conflict(workspace_id) do update set holder_device_id = excluded.holder_device_id, lease_token = excluded.lease_token, expires_at = excluded.expires_at;
set local role authenticated;
select timegenie.device_revoke('20000000-0000-0000-0000-000000000001', '30000000-0000-0000-0000-000000000001');
select timegenie.device_revoke('20000000-0000-0000-0000-000000000001', '30000000-0000-0000-0000-000000000001');
do $$
begin
  if (select holder_device_id is not null from timegenie.tracking_leases where workspace_id = '20000000-0000-0000-0000-000000000001') then
    raise exception 'device revoke did not release tracking lease';
  end if;
  if (timegenie.device_authorization_get('20000000-0000-0000-0000-000000000001', '30000000-0000-0000-0000-000000000001')->>'authorized')::boolean then
    raise exception 'revoked device remains registered';
  end if;
end $$;
do $$
begin
  begin
    perform timegenie.device_authorize('20000000-0000-0000-0000-000000000001', '30000000-0000-0000-0000-000000000001', '设备 A', 'windows', '0.2.1');
    raise exception 'revoked device was reauthorized by the old auth session';
  exception
    when others then
      if sqlerrm not like '%PASSWORD_REAUTH_REQUIRED%' then raise; end if;
  end;
end $$;
select set_config('request.jwt.claim.session_id', '80000000-0000-0000-0000-000000000002', true);
select timegenie.device_authorize('20000000-0000-0000-0000-000000000001', '30000000-0000-0000-0000-000000000001', '设备 A', 'windows', '0.2.1');
do $$
begin
  if not (timegenie.device_authorization_get('20000000-0000-0000-0000-000000000001', '30000000-0000-0000-0000-000000000001')->>'authorized')::boolean then
    raise exception 'explicit device authorization did not restore access';
  end if;
end $$;

reset role;
do $$
begin
  if (select reauthorized_at is null from timegenie.devices where id = '30000000-0000-0000-0000-000000000001') then
    raise exception 'device reauthorization audit time is missing';
  end if;
end $$;
set local role authenticated;

do $$
declare snapshot jsonb;
begin
  snapshot := timegenie.cloud_snapshot_get('20000000-0000-0000-0000-000000000001');
  if jsonb_array_length(snapshot->'subjects') <> 1 then
    raise exception 'workspace defaults created duplicate subjects';
  end if;
  if jsonb_array_length(snapshot->'app_settings') <> 5 then
    raise exception 'workspace defaults created duplicate settings';
  end if;
  if jsonb_array_length(snapshot->'report_templates') <> 3 then
    raise exception 'workspace defaults created duplicate templates';
  end if;
end $$;

select timegenie.device_authorize('20000000-0000-0000-0000-000000000001', '30000000-0000-0000-0000-000000000002', '设备 B', 'windows', '0.1.0');

-- Security-definer RPCs must reject unknown or revoked device identities even
-- when the authenticated user owns the workspace.
reset role;
insert into timegenie.devices(id, workspace_id, device_name, platform, app_version, revoked_at) values
  ('30000000-0000-0000-0000-000000000098', '20000000-0000-0000-0000-000000000001', '已撤销设备', 'windows', '0.1.0', now());
set local role authenticated;

do $$
begin
  begin
    perform timegenie.tracking_lease_acquire(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000098',
      '90000000-0000-0000-0000-000000000098'
    );
    raise exception 'revoked device acquired a tracking lease';
  exception
    when others then
      if sqlerrm not like '%DEVICE_NOT_REGISTERED%' then raise; end if;
  end;

  begin
    perform timegenie.timer_start(
      '20000000-0000-0000-0000-000000000001',
      null,
      '30000000-0000-0000-0000-000000000098',
      '40000000-0000-0000-0000-000000000098',
      '已撤销设备启动'
    );
    raise exception 'revoked device started a timer';
  exception
    when others then
      if sqlerrm not like '%DEVICE_NOT_REGISTERED%' then raise; end if;
  end;

  begin
    perform timegenie.unassigned_resolve_work(
      '20000000-0000-0000-0000-000000000001',
      '91000000-0000-0000-0000-000000000098',
      '30000000-0000-0000-0000-000000000098',
      1,
      '[]'::jsonb,
      '40000000-0000-0000-0000-000000000099'
    );
    raise exception 'revoked device resolved unassigned time';
  exception
    when others then
      if sqlerrm not like '%DEVICE_NOT_REGISTERED%' then raise; end if;
  end;

  begin
    perform timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000098',
      '40000000-0000-0000-0000-000000000098',
      'app_setting.update',
      'app_setting',
      'revoked_device_upload_probe',
      null,
      jsonb_build_object(
        'key', 'revoked_device_upload_probe',
        'value_json', 'true'::jsonb,
        'updated_at', 1791100800000,
        'version', 1
      )
    );
    raise exception 'revoked device uploaded an outbox operation';
  exception
    when others then
      if sqlerrm not like '%DEVICE_NOT_REGISTERED%' then raise; end if;
  end;

  begin
    perform timegenie.cloud_incremental_pull(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000098',
      0,
      10
    );
    raise exception 'revoked device read incremental changes';
  exception
    when others then
      if sqlerrm not like '%DEVICE_REVOKED%' then raise; end if;
  end;
end $$;

select timegenie.cloud_apply_patch(
  '20000000-0000-0000-0000-000000000001',
  '30000000-0000-0000-0000-000000000002',
  '40000000-0000-0000-0000-000000000097',
  'app_setting.update',
  'app_setting',
  'authorized_device_upload_probe',
  null,
  jsonb_build_object(
    'key', 'authorized_device_upload_probe',
    'value_json', 'true'::jsonb,
    'updated_at', 1791100800000,
    'version', 1
  )
);

-- A different authenticated user cannot read or write account A's workspace.
select set_config('request.jwt.claim.sub', '10000000-0000-0000-0000-000000000002', true);
do $$
begin
  if (select count(*) from timegenie.workspaces where id = '20000000-0000-0000-0000-000000000001') <> 0 then
    raise exception 'RLS exposed another user workspace';
  end if;
  begin
    perform timegenie.cloud_snapshot_get('20000000-0000-0000-0000-000000000001');
    raise exception 'RLS exposed another user snapshot';
  exception
    when others then
      if sqlerrm not like '%AUTH_REQUIRED%' then raise; end if;
  end;
  begin
    perform timegenie.cloud_incremental_pull(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000001',
      0,
      10
    );
    raise exception 'incremental RPC exposed another user workspace';
  exception
    when others then
      if sqlerrm not like '%AUTH_REQUIRED%' then raise; end if;
  end;
  begin
    insert into timegenie.devices(id, workspace_id, device_name, platform, app_version) values
      ('30000000-0000-0000-0000-000000000099', '20000000-0000-0000-0000-000000000001', '越权设备', 'windows', '0.1.0');
    raise exception 'RLS allowed a device in another user workspace';
  exception
    when insufficient_privilege then null;
  end;
end $$;

select set_config('request.jwt.claim.sub', '10000000-0000-0000-0000-000000000001', true);

-- Replace generated defaults with a complete local snapshot containing a task
-- tree, one time entry, one report, and their relationships.
select timegenie.migration_import_snapshot(
  '20000000-0000-0000-0000-000000000001',
  '40000000-0000-0000-0000-000000000001',
  '30000000-0000-0000-0000-000000000001',
  jsonb_build_object(
    'subjects', jsonb_build_array(jsonb_build_object(
      'id', '50000000-0000-0000-0000-000000000001',
      'name', '默认',
      'sort_order', 10,
      'created_at', 1789689600000,
      'updated_at', 1789689600000,
      'version', 1
    )),
    'app_settings', jsonb_build_array(
      jsonb_build_object(
        'key', 'salary_hourly_rate',
        'value_json', 88.5,
        'updated_at', 1789689600000,
        'version', 1
      ),
      jsonb_build_object(
        'key', 'timezone',
        'value_json', 'Asia/Shanghai',
        'updated_at', 1789689600000,
        'version', 1
      )
    ),
    'integration_configs', jsonb_build_array(jsonb_build_object(
      'provider', 'seatable',
      'enabled', 1,
      'config_json', jsonb_build_object('serverUrl', 'https://example.test', 'batchSize', 100),
      'updated_at', 1789689600000,
      'version', 1
    )),
    'tasks', jsonb_build_array(
      jsonb_build_object(
        'id', '60000000-0000-0000-0000-000000000001',
        'subject_id', '50000000-0000-0000-0000-000000000001',
        'title', '运维相关',
        'status', 'open',
        'planned_date', '2026-09-24',
        'source_type', 'manual',
        'sort_order', 10,
        'created_at', 1789689600000,
        'updated_at', 1789689600000,
        'version', 1
      ),
      jsonb_build_object(
        'id', '60000000-0000-0000-0000-000000000002',
        'subject_id', '50000000-0000-0000-0000-000000000001',
        'parent_id', '60000000-0000-0000-0000-000000000001',
        'title', '验证云端同步',
        'status', 'open',
        'planned_date', '2026-09-24',
        'estimate_minutes', 30,
        'source_type', 'manual',
        'sort_order', 20,
        'created_at', 1789689600000,
        'updated_at', 1789689600000,
        'version', 1
      )
    ),
    'task_daily_estimates', jsonb_build_array(jsonb_build_object(
      'task_id', '60000000-0000-0000-0000-000000000002',
      'work_date', '2026-09-24',
      'estimate_minutes', 45,
      'created_at', 1789689600000,
      'updated_at', 1789689600000,
      'version', 1
    )),
    'task_recurrence_rules', jsonb_build_array(jsonb_build_object(
      'id', '90000000-0000-0000-0000-000000000001',
      'task_id', '60000000-0000-0000-0000-000000000002',
      'frequency', 'daily',
      'effective_start', '2026-09-24',
      'created_at', 1789689600000,
      'updated_at', 1789689600000,
      'version', 1
    )),
    'task_occurrences', jsonb_build_array(jsonb_build_object(
      'task_id', '60000000-0000-0000-0000-000000000002',
      'occurrence_date', '2026-09-24',
      'origin', 'scheduled',
      'status', 'open',
      'created_at', 1789689600000,
      'updated_at', 1789689600000,
      'version', 1
    )),
    'time_entries', jsonb_build_array(jsonb_build_object(
      'id', '70000000-0000-0000-0000-000000000001',
      'work_date', '2026-09-24',
      'kind', 'work',
      'source_type', 'manual',
      'state', 'ended',
      'default_task_id', '60000000-0000-0000-0000-000000000002',
      'label_snapshot', '运维相关 / 验证云端同步',
      'started_at', 1789693200000,
      'ended_at', 1789695000000,
      'duration_seconds', 1800,
      'origin_unassigned_session_id', '71000000-0000-0000-0000-000000000001',
      'created_at', 1789695000000,
      'updated_at', 1789695000000,
      'version', 1
    )),
    'unassigned_sessions', jsonb_build_array(jsonb_build_object(
      'id', '71000000-0000-0000-0000-000000000001',
      'work_date', '2026-09-24',
      'state', 'resolved',
      'threshold_seconds', 300,
      'duration_seconds', 1800,
      'first_started_at', 1789693200000,
      'last_ended_at', 1789695000000,
      'resolution_type', 'work',
      'generated_entry_id', '70000000-0000-0000-0000-000000000001',
      'resolved_at', 1789695000000,
      'created_at', 1789693200000,
      'updated_at', 1789695000000,
      'version', 1
    )),
    'unassigned_segments', jsonb_build_array(jsonb_build_object(
      'id', '72000000-0000-0000-0000-000000000001',
      'session_id', '71000000-0000-0000-0000-000000000001',
      'sequence_no', 1,
      'started_at', 1789693200000,
      'ended_at', 1789695000000,
      'duration_seconds', 1800
    )),
    'reports', jsonb_build_array(jsonb_build_object(
      'id', '80000000-0000-0000-0000-000000000001',
      'report_type', 'daily',
      'subject_id', '50000000-0000-0000-0000-000000000001',
      'period_start', '2026-09-24',
      'period_end', '2026-09-24',
      'reference_date', '2026-09-24',
      'markdown_content', '# 2026-09-24',
      'content_source', 'generated',
      'generation_count', 1,
      'created_at', 1789695000000,
      'updated_at', 1789695000000,
      'version', 1
    )),
    'report_tasks', jsonb_build_array(jsonb_build_object(
      'report_id', '80000000-0000-0000-0000-000000000001',
      'task_id', '60000000-0000-0000-0000-000000000002',
      'sort_order', 10,
      'title_snapshot', '验证云端同步',
      'path_snapshot', '运维相关 / 验证云端同步'
    ))
  )
);

-- Reusing an operation id with different content must be rejected. A new
-- migration operation must also be rejected because the target is no longer empty.
do $$
begin
  begin
    perform timegenie.migration_import_snapshot(
      '20000000-0000-0000-0000-000000000001',
      '40000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000001',
      '{}'::jsonb
    );
    raise exception 'migration accepted a reused operation id with different content';
  exception
    when others then
      if sqlerrm not like '%IDEMPOTENCY_CONFLICT%' then raise; end if;
  end;
end $$;

-- Incremental reads preserve ordered cursor coverage, page without gaps,
-- collapse repeated changes for one entity to its final image, and return an
-- explicit tombstone after deletion.
reset role;
do $$
declare before_seq bigint;
begin
  select coalesce(max(change_seq), 0) into before_seq
  from timegenie.workspace_changes
  where workspace_id = '20000000-0000-0000-0000-000000000001';
  perform set_config('timegenie.incremental_test_before_seq', before_seq::text, true);

  insert into timegenie.app_settings(workspace_id, key, value_json, version)
  values('20000000-0000-0000-0000-000000000001', 'incremental_contract_probe', '{"step":1}'::jsonb, 1);
  update timegenie.app_settings
  set value_json = '{"step":2}'::jsonb, version = 2, updated_at = now()
  where workspace_id = '20000000-0000-0000-0000-000000000001'
    and key = 'incremental_contract_probe';
  delete from timegenie.app_settings
  where workspace_id = '20000000-0000-0000-0000-000000000001'
    and key = 'incremental_contract_probe';
end $$;
set local role authenticated;
select set_config('request.jwt.claim.sub', '10000000-0000-0000-0000-000000000001', true);

do $$
declare before_seq bigint := current_setting('timegenie.incremental_test_before_seq')::bigint;
declare first_page jsonb;
declare folded jsonb;
declare first_change_seq bigint;
begin
  first_page := timegenie.cloud_incremental_pull(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    before_seq,
    1
  );
  if jsonb_array_length(first_page->'changes') <> 1
     or not (first_page->>'has_more')::boolean
     or (first_page->>'reset_required')::boolean then
    raise exception 'incremental first page metadata is invalid: %', first_page;
  end if;
  first_change_seq := (first_page->'changes'->0->>'change_seq')::bigint;
  if first_change_seq <> before_seq + 1
     or (first_page->>'covered_change_seq')::bigint <> first_change_seq
     or (first_page->>'latest_change_seq')::bigint < first_change_seq + 2 then
    raise exception 'incremental first page cursor coverage is invalid: %', first_page;
  end if;

  folded := timegenie.cloud_incremental_pull(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    before_seq,
    100
  );
  if jsonb_array_length(folded->'changes') <> 3
     or jsonb_array_length(folded->'entities') <> 1
     or (folded->>'has_more')::boolean then
    raise exception 'incremental repeated changes were not folded: %', folded;
  end if;
  if folded->'entities'->0->>'source_entity_type' <> 'app_settings'
     or folded->'entities'->0->>'entity_id' <> 'incremental_contract_probe'
     or (folded->'entities'->0->>'change_seq')::bigint <> (folded->>'covered_change_seq')::bigint
     or not (folded->'entities'->0->>'deleted')::boolean
     or folded->'entities'->0->'data' is distinct from 'null'::jsonb then
    raise exception 'incremental delete did not return an explicit tombstone: %', folded;
  end if;
end $$;

do $$
declare snapshot jsonb;
begin
  snapshot := timegenie.cloud_snapshot_get('20000000-0000-0000-0000-000000000001');
  if jsonb_array_length(snapshot->'tasks') <> 2 then
    raise exception 'migration did not preserve the task tree';
  end if;
  if not exists(
    select 1
    from jsonb_array_elements(snapshot->'tasks') task
    where task->>'id' = '60000000-0000-0000-0000-000000000002'
      and task->>'parent_id' = '60000000-0000-0000-0000-000000000001'
  ) then
    raise exception 'migration did not restore the task parent';
  end if;
  if not exists(
    select 1
    from jsonb_array_elements(snapshot->'time_entries') entry
    where entry->>'id' = '70000000-0000-0000-0000-000000000001'
      and entry->>'origin_unassigned_session_id' = '71000000-0000-0000-0000-000000000001'
  ) then
    raise exception 'migration did not import the time entry with its unassigned origin';
  end if;
  if not exists(
    select 1
    from jsonb_array_elements(snapshot->'unassigned_sessions') session
    where session->>'id' = '71000000-0000-0000-0000-000000000001'
      and session->>'generated_entry_id' = '70000000-0000-0000-0000-000000000001'
  ) then
    raise exception 'migration did not preserve the unassigned session generated entry';
  end if;
  if not exists(
    select 1
    from jsonb_array_elements(snapshot->'unassigned_segments') segment
    where segment->>'session_id' = '71000000-0000-0000-0000-000000000001'
  ) then
    raise exception 'migration did not preserve the unassigned segment';
  end if;
  if jsonb_array_length(snapshot->'task_daily_estimates') <> 1
     or jsonb_array_length(snapshot->'task_recurrence_rules') <> 1
     or jsonb_array_length(snapshot->'task_occurrences') <> 1 then
    raise exception 'migration did not preserve recurring task data';
  end if;
  if not exists(
    select 1
    from jsonb_array_elements(snapshot->'task_daily_estimates') estimate
    where estimate->>'task_id' = '60000000-0000-0000-0000-000000000002'
      and (estimate->>'estimate_minutes')::integer = 45
  ) then
    raise exception 'migration did not import the daily estimate';
  end if;
  if not exists(
    select 1
    from jsonb_array_elements(snapshot->'task_recurrence_rules') rule
    where rule->>'id' = '90000000-0000-0000-0000-000000000001'
      and rule->>'frequency' = 'daily'
  ) then
    raise exception 'migration did not import the recurrence rule';
  end if;
  if not exists(
    select 1
    from jsonb_array_elements(snapshot->'task_occurrences') occurrence
    where occurrence->>'task_id' = '60000000-0000-0000-0000-000000000002'
      and occurrence->>'occurrence_date' = '2026-09-24'
  ) then
    raise exception 'migration did not import the task occurrence';
  end if;
  if not exists(
    select 1
    from jsonb_array_elements(snapshot->'reports') report
    where report->>'id' = '80000000-0000-0000-0000-000000000001'
  ) then
    raise exception 'migration did not import the report';
  end if;
  if not exists(
    select 1
    from jsonb_array_elements(snapshot->'app_settings') setting
    where setting->>'key' = 'salary_hourly_rate'
      and jsonb_typeof(setting->'value_json') = 'number'
  ) then
    raise exception 'migration changed numeric setting type';
  end if;
  if not exists(
    select 1
    from jsonb_array_elements(snapshot->'app_settings') setting
    where setting->>'key' = 'timezone'
      and setting->>'value_json' = 'Asia/Shanghai'
  ) then
    raise exception 'migration changed string setting value';
  end if;
  if not exists(
    select 1
    from jsonb_array_elements(snapshot->'integration_configs') config
    where config->>'provider' = 'seatable'
      and jsonb_typeof(config->'config_json') = 'object'
  ) then
    raise exception 'migration changed integration config type';
  end if;
  begin
    perform timegenie.migration_import_snapshot(
      '20000000-0000-0000-0000-000000000001',
      '40000000-0000-0000-0000-000000000002',
      '30000000-0000-0000-0000-000000000001',
      '{}'::jsonb
    );
    raise exception 'migration accepted a non-empty target';
  exception
    when others then
      if sqlerrm not like '%MIGRATION_TARGET_NOT_EMPTY%' then raise; end if;
  end;
end $$;

-- Changes are monotonic, and stale complete-entity updates are rejected.
do $$
declare before_seq bigint; snapshot jsonb;
begin
  select max(change_seq) into before_seq from timegenie.workspace_changes
  where workspace_id = '20000000-0000-0000-0000-000000000001';

  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000010',
    'task.update',
    'task',
    '60000000-0000-0000-0000-000000000002',
    1,
    jsonb_build_object(
      'id', '60000000-0000-0000-0000-000000000002',
      'subject_id', '50000000-0000-0000-0000-000000000001',
      'parent_id', '60000000-0000-0000-0000-000000000001',
      'title', '验证云端同步（设备 A）',
      'status', 'open',
      'planned_date', '2026-09-24',
      'estimate_minutes', 30,
      'source_type', 'manual',
      'sort_order', 20,
      'created_at', 1789689600000,
      'updated_at', 1789696800000,
      'version', 2
    )
  );

  if (select max(change_seq) from timegenie.workspace_changes where workspace_id = '20000000-0000-0000-0000-000000000001') <= before_seq then
    raise exception 'workspace change sequence did not advance';
  end if;

  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000015',
    'time_entry.update',
    'time_entry',
    '70000000-0000-0000-0000-000000000001',
    1,
    jsonb_build_object(
      'id', '70000000-0000-0000-0000-000000000001',
      'work_date', '2026-09-24',
      'kind', 'work',
      'source_type', 'manual',
      'state', 'ended',
      'default_task_id', '60000000-0000-0000-0000-000000000002',
      'label_snapshot', '运维相关 / 验证云端同步',
      'started_at', 1789693200000,
      'ended_at', 1789695600000,
      'duration_seconds', 2400,
      'note', '修正后仍保留未归属来源',
      'origin_unassigned_session_id', '71000000-0000-0000-0000-000000000001',
      'created_at', 1789695000000,
      'updated_at', 1789695600000,
      'version', 2,
      'segments', jsonb_build_array(jsonb_build_object(
        'id', '73000000-0000-0000-0000-000000000001',
        'sequence_no', 1,
        'started_at', 1789693200000,
        'ended_at', 1789695600000,
        'duration_seconds', 2400
      )),
      'allocations', jsonb_build_array(jsonb_build_object(
        'id', '74000000-0000-0000-0000-000000000001',
        'task_id', '60000000-0000-0000-0000-000000000002',
        'minutes', 40,
        'note', '修正归属',
        'created_at', 1789695600000,
        'updated_at', 1789695600000,
        'version', 1
      ))
    )
  );

  if not exists(
    select 1
    from timegenie.time_entries
    where id = '70000000-0000-0000-0000-000000000001'
      and origin_unassigned_session_id = '71000000-0000-0000-0000-000000000001'
      and duration_seconds = 2400
      and version = 2
  ) then
    raise exception 'time entry patch did not preserve unassigned origin';
  end if;

  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000040',
    'time_entry.update.with-newer-work-day',
    'time_entry',
    '70000000-0000-0000-0000-000000000001',
    2,
    jsonb_build_object(
      'id', '70000000-0000-0000-0000-000000000001',
      'work_date', '2026-09-24',
      'kind', 'work',
      'source_type', 'manual',
      'state', 'ended',
      'default_task_id', '60000000-0000-0000-0000-000000000002',
      'label_snapshot', '运维相关 / 验证云端同步',
      'started_at', 1789693200000,
      'ended_at', 1789695600000,
      'duration_seconds', 2400,
      'note', '先带入新版 work_day',
      'origin_unassigned_session_id', '71000000-0000-0000-0000-000000000001',
      'created_at', 1789695000000,
      'updated_at', 1789696200000,
      'version', 3,
      'segments', jsonb_build_array(jsonb_build_object(
        'id', '73000000-0000-0000-0000-000000000001',
        'sequence_no', 1,
        'started_at', 1789693200000,
        'ended_at', 1789695600000,
        'duration_seconds', 2400
      )),
      'allocations', jsonb_build_array(),
      'work_day', jsonb_build_object(
        'work_date', '2026-09-24',
        'timezone', 'Asia/Shanghai',
        'work_period_text', null,
        'note', '新版工作日',
        'settled_at', 1789696800000,
        'created_at', 1789689600000,
        'updated_at', 1789696800000,
        'version', 5
      )
    )
  );

  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000041',
    'time_entry.update.after-newer-work-day',
    'time_entry',
    '70000000-0000-0000-0000-000000000001',
    3,
    jsonb_build_object(
      'id', '70000000-0000-0000-0000-000000000001',
      'work_date', '2026-09-24',
      'kind', 'work',
      'source_type', 'manual',
      'state', 'ended',
      'default_task_id', '60000000-0000-0000-0000-000000000002',
      'label_snapshot', '运维相关 / 验证云端同步',
      'started_at', 1789693200000,
      'ended_at', 1789695600000,
      'duration_seconds', 2400,
      'note', '旧 work_day 不应覆盖新版',
      'origin_unassigned_session_id', '71000000-0000-0000-0000-000000000001',
      'created_at', 1789695000000,
      'updated_at', 1789696500000,
      'version', 4,
      'segments', jsonb_build_array(jsonb_build_object(
        'id', '73000000-0000-0000-0000-000000000001',
        'sequence_no', 1,
        'started_at', 1789693200000,
        'ended_at', 1789695600000,
        'duration_seconds', 2400
      )),
      'allocations', jsonb_build_array(),
      'work_day', jsonb_build_object(
        'work_date', '2026-09-24',
        'timezone', 'Asia/Shanghai',
        'work_period_text', null,
        'note', '旧版本工作日',
        'settled_at', null,
        'created_at', 1789689600000,
        'updated_at', 1789695000000,
        'version', 2
      )
    )
  );

  snapshot := timegenie.cloud_snapshot_get('20000000-0000-0000-0000-000000000001');
  if not exists(
    select 1
    from jsonb_array_elements(snapshot->'work_days') as work_day
    where work_day->>'work_date' = '2026-09-24'
      and (work_day->>'version')::bigint = 5
      and work_day->>'settled_at' is not null
  ) then
    raise exception 'stale embedded work_day overwrote a newer cloud work_day';
  end if;

  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000019',
    'unassigned_session.update',
    'unassigned_session',
    '71000000-0000-0000-0000-000000000001',
    1,
    jsonb_build_object(
      'id', '71000000-0000-0000-0000-000000000001',
      'work_date', '2026-09-24',
      'state', 'resolved',
      'threshold_seconds', 300,
      'duration_seconds', 2400,
      'first_started_at', 1789693200000,
      'last_ended_at', 1789695600000,
      'prompted_at', 1789695300000,
      'resolution_type', 'work',
      'generated_entry_id', '70000000-0000-0000-0000-000000000001',
      'resolved_at', 1789695600000,
      'created_at', 1789693200000,
      'updated_at', 1789695600000,
      'version', 3,
      'segments', jsonb_build_array(jsonb_build_object(
        'id', '72000000-0000-0000-0000-000000000001',
        'sequence_no', 1,
        'started_at', 1789693200000,
        'ended_at', 1789695600000,
        'duration_seconds', 2400,
        'lease_token', null
      ))
    )
  );
  snapshot := timegenie.cloud_snapshot_get('20000000-0000-0000-0000-000000000001');
  if not exists(
    select 1
    from jsonb_array_elements(snapshot->'unassigned_sessions') session
    where session->>'id' = '71000000-0000-0000-0000-000000000001'
      and session->>'generated_entry_id' = '70000000-0000-0000-0000-000000000001'
      and session->>'state' = 'resolved'
      and (session->>'version')::bigint = 3
  ) or not exists(
    select 1
    from jsonb_array_elements(snapshot->'unassigned_segments') segment
    where segment->>'session_id' = '71000000-0000-0000-0000-000000000001'
      and segment->>'id' = '72000000-0000-0000-0000-000000000001'
      and (segment->>'duration_seconds')::bigint = 2400
  ) then
    raise exception 'unassigned session patch did not preserve resolved source session and segments';
  end if;
  if not exists(
    select 1 from timegenie.workspace_changes
    where workspace_id = '20000000-0000-0000-0000-000000000001'
      and entity_type = 'unassigned_sessions'
      and entity_id = '71000000-0000-0000-0000-000000000001'
      and entity_version = 3
  ) then
    raise exception 'unassigned session patch did not emit workspace change';
  end if;

  begin
    perform timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000002',
      '40000000-0000-0000-0000-000000000020',
      'unassigned_session.update',
      'unassigned_session',
      '71000000-0000-0000-0000-000000000001',
      1,
      jsonb_build_object(
        'id', '71000000-0000-0000-0000-000000000001',
        'work_date', '2026-09-24',
        'state', 'discarded',
        'threshold_seconds', 300,
        'duration_seconds', 2400,
        'first_started_at', 1789693200000,
        'last_ended_at', 1789695600000,
        'prompted_at', 1789695300000,
        'resolution_type', 'discard',
        'generated_entry_id', '70000000-0000-0000-0000-000000000001',
        'resolved_at', 1789695700000,
        'created_at', 1789693200000,
        'updated_at', 1789695700000,
        'version', 4,
        'segments', jsonb_build_array()
      )
    );
    raise exception 'stale unassigned session patch was accepted';
  exception
    when others then
      if sqlerrm not like '%SYNC_CONFLICT%' then raise; end if;
  end;

  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000016',
    'app_setting.update',
    'app_setting',
    'identity_probe_setting',
    null,
    jsonb_build_object(
      'key', 'identity_probe_setting',
      'value_json', 120,
      'updated_at', 1789695600000,
      'version', 1
    )
  );
  if not exists(
    select 1 from timegenie.workspace_changes
    where workspace_id = '20000000-0000-0000-0000-000000000001'
      and entity_type = 'app_settings'
      and entity_id = 'identity_probe_setting'
  ) then
    raise exception 'app setting workspace change did not preserve setting key identity';
  end if;

  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000017',
    'integration_config.update',
    'integration_config',
    'identity_probe_provider',
    null,
    jsonb_build_object(
      'provider', 'identity_probe_provider',
      'enabled', true,
      'config_json', jsonb_build_object('syncEnabled', true),
      'updated_at', 1789695600000,
      'version', 1
    )
  );
  if not exists(
    select 1 from timegenie.workspace_changes
    where workspace_id = '20000000-0000-0000-0000-000000000001'
      and entity_type = 'integration_configs'
      and entity_id = 'identity_probe_provider'
  ) then
    raise exception 'integration config workspace change did not preserve provider identity';
  end if;

  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000018',
    'external_binding.upsert',
    'external_binding',
    'seatable|task|60000000-0000-0000-0000-000000000002',
    null,
    jsonb_build_object(
      'id', '76000000-0000-0000-0000-000000000001',
      'provider', 'seatable',
      'entity_type', 'task',
      'entity_id', '60000000-0000-0000-0000-000000000002',
      'external_id', 'sea-row-1',
      'external_revision', 'rev-1',
      'last_synced_hash', 'hash-1',
      'last_synced_at', 1789695600000
    )
  );
  if not exists(
    select 1 from timegenie.workspace_changes
    where workspace_id = '20000000-0000-0000-0000-000000000001'
      and entity_type = 'external_bindings'
      and entity_id = 'seatable|task|60000000-0000-0000-0000-000000000002'
  ) then
    raise exception 'external binding workspace change did not preserve bound entity identity';
  end if;

  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000010',
    'task.update',
    'task',
    '60000000-0000-0000-0000-000000000002',
    1,
    jsonb_build_object(
      'id', '60000000-0000-0000-0000-000000000002',
      'subject_id', '50000000-0000-0000-0000-000000000001',
      'parent_id', '60000000-0000-0000-0000-000000000001',
      'title', '验证云端同步（设备 A）',
      'status', 'open',
      'planned_date', '2026-09-24',
      'estimate_minutes', 30,
      'source_type', 'manual',
      'sort_order', 20,
      'created_at', 1789689600000,
      'updated_at', 1789696800000,
      'version', 2
    )
  );

  begin
    perform timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000001',
      '40000000-0000-0000-0000-000000000010',
      'task.update',
      'task',
      '60000000-0000-0000-0000-000000000002',
      1,
      jsonb_build_object(
        'id', '60000000-0000-0000-0000-000000000002',
        'subject_id', '50000000-0000-0000-0000-000000000001',
        'parent_id', '60000000-0000-0000-0000-000000000001',
        'title', '同 ID 不同内容不应当被当作重试',
        'status', 'open',
        'planned_date', '2026-09-24',
        'estimate_minutes', 30,
        'source_type', 'manual',
        'sort_order', 20,
        'created_at', 1789689600000,
        'updated_at', 1789696800000,
        'version', 2
      )
    );
    raise exception 'operation id accepted different payload';
  exception
    when others then
      if sqlerrm not like '%IDEMPOTENCY_CONFLICT%' then raise; end if;
  end;

  begin
    perform timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000002',
      '40000000-0000-0000-0000-000000000011',
      'task.update',
      'task',
      '60000000-0000-0000-0000-000000000002',
      1,
      jsonb_build_object('id', '60000000-0000-0000-0000-000000000002', 'version', 2)
    );
    raise exception 'stale patch was accepted';
  exception
    when others then
      if sqlerrm not like '%SYNC_CONFLICT%' then raise; end if;
  end;

  begin
    perform timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000002',
      '40000000-0000-0000-0000-000000000012',
      'task.create',
      'task',
      '60000000-0000-0000-0000-000000000002',
      null,
      jsonb_build_object(
        'id', '60000000-0000-0000-0000-000000000002',
        'subject_id', '50000000-0000-0000-0000-000000000001',
        'title', '创建不应覆盖已有事项',
        'status', 'open',
        'source_type', 'manual',
        'sort_order', 20,
        'created_at', 1789689600000,
        'updated_at', 1789696800000,
        'version', 1
      )
    );
    raise exception 'create patch overwrote an existing entity';
  exception
    when others then
      if sqlerrm not like '%SYNC_CONFLICT%' then raise; end if;
  end;

  begin
    perform timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000002',
      '40000000-0000-0000-0000-000000000013',
      'task.update',
      'task',
      '60000000-0000-0000-0000-000000000002',
      2,
      jsonb_build_object(
        'id', '60000000-0000-0000-0000-000000000002',
        'subject_id', '50000000-0000-0000-0000-000000000001',
        'title', '版本不能跳号',
        'status', 'open',
        'source_type', 'manual',
        'sort_order', 20,
        'created_at', 1789689600000,
        'updated_at', 1789696800000,
        'version', 4
      )
    );
    raise exception 'version jump patch was accepted';
  exception
    when others then
      if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if;
  end;

  begin
    perform timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000002',
      '40000000-0000-0000-0000-000000000014',
      'task.update',
      'task',
      '60000000-0000-0000-0000-000000000002',
      2,
      jsonb_build_object(
        'id', '60000000-0000-0000-0000-000000000001',
        'subject_id', '50000000-0000-0000-0000-000000000001',
        'title', 'payload id 不一致',
        'status', 'open',
        'source_type', 'manual',
        'sort_order', 20,
        'created_at', 1789689600000,
        'updated_at', 1789696800000,
        'version', 3
      )
    );
    raise exception 'mismatched payload id patch was accepted';
  exception
    when others then
      if sqlerrm not like '%VALIDATION_ERROR%' then raise; end if;
  end;
end $$;

-- Recurring task occurrences are complete entity patches with the composite
-- task/date identity used by the desktop outbox.
do $$
declare snapshot jsonb; occurrence_change_count bigint;
begin
  select count(*) into occurrence_change_count
  from timegenie.workspace_changes
  where workspace_id = '20000000-0000-0000-0000-000000000001'
    and entity_type = 'task_occurrences'
    and entity_id = '60000000-0000-0000-0000-000000000002|2026-09-29';

  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000030',
    'task_occurrence.set_completed',
    'task_occurrence',
    '60000000-0000-0000-0000-000000000002|2026-09-29',
    null,
    jsonb_build_object(
      'task_id', '60000000-0000-0000-0000-000000000002',
      'occurrence_date', '2026-09-29',
      'origin', 'manual',
      'status', 'done',
      'completed_at', 1789696800000,
      'created_at', 1789689600000,
      'updated_at', 1789696800000,
      'version', 2
    )
  );

  snapshot := timegenie.cloud_snapshot_get('20000000-0000-0000-0000-000000000001');
  if not exists(
    select 1
    from jsonb_array_elements(snapshot->'task_occurrences') as occurrence
    where occurrence->>'task_id' = '60000000-0000-0000-0000-000000000002'
      and occurrence->>'occurrence_date' = '2026-09-29'
      and occurrence->>'status' = 'done'
      and (occurrence->>'version')::bigint = 2
  ) then
    raise exception 'task occurrence patch was not applied';
  end if;
  if (
    select count(*)
    from timegenie.workspace_changes
    where workspace_id = '20000000-0000-0000-0000-000000000001'
      and entity_type = 'task_occurrences'
      and entity_id = '60000000-0000-0000-0000-000000000002|2026-09-29'
  ) <> occurrence_change_count + 1 then
    raise exception 'task occurrence patch produced duplicate workspace changes';
  end if;

  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000031',
    'task_occurrence.reopen',
    'task_occurrence',
    '60000000-0000-0000-0000-000000000002|2026-09-29',
    2,
    jsonb_build_object(
      'task_id', '60000000-0000-0000-0000-000000000002',
      'occurrence_date', '2026-09-29',
      'origin', 'manual',
      'status', 'open',
      'completed_at', null,
      'created_at', 1789689600000,
      'updated_at', 1789696900000,
      'version', 3
    )
  );

  begin
    perform timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000002',
      '40000000-0000-0000-0000-000000000032',
      'task_occurrence.set_completed',
      'task_occurrence',
      '60000000-0000-0000-0000-000000000002|2026-09-29',
      2,
      jsonb_build_object(
        'task_id', '60000000-0000-0000-0000-000000000002',
        'occurrence_date', '2026-09-29',
        'origin', 'manual',
        'status', 'done',
        'completed_at', 1789697000000,
        'created_at', 1789689600000,
        'updated_at', 1789697000000,
        'version', 3
      )
    );
    raise exception 'stale task occurrence patch was accepted';
  exception
    when others then
      if sqlerrm not like '%SYNC_CONFLICT%' then raise; end if;
  end;
end $$;

-- Daily estimates support composite identity, explicit clears, idempotent retries,
-- and optimistic concurrency without coupling different dates.
reset role;
insert into timegenie.subjects(id, workspace_id, name, sort_order)
values('51000000-0000-0000-0000-000000000090','20000000-0000-0000-0000-000000000001','规划同步测试',90);
insert into timegenie.tasks(id, workspace_id, subject_id, title, status, source_type, sort_order, version)
values('61000000-0000-0000-0000-000000000090','20000000-0000-0000-0000-000000000001','51000000-0000-0000-0000-000000000090','每日计划','open','manual',90,1);
set role authenticated;
do $$
declare first_result jsonb; retry_result jsonb;
begin
  first_result := timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000150','task_daily_estimate.set','task_daily_estimate',
    '61000000-0000-0000-0000-000000000090|2026-10-05',null,
    jsonb_build_object('task_id','61000000-0000-0000-0000-000000000090','work_date','2026-10-05','estimate_minutes',45,'created_at',1791158400000,'updated_at',1791158400000,'version',1)
  );
  retry_result := timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000150','task_daily_estimate.set','task_daily_estimate',
    '61000000-0000-0000-0000-000000000090|2026-10-05',null,
    jsonb_build_object('task_id','61000000-0000-0000-0000-000000000090','work_date','2026-10-05','estimate_minutes',45,'created_at',1791158400000,'updated_at',1791158400000,'version',1)
  );
  if first_result is distinct from retry_result then raise exception 'daily estimate retry changed result'; end if;

  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000151','task_daily_estimate.set','task_daily_estimate',
    '61000000-0000-0000-0000-000000000090|2026-10-05',1,
    jsonb_build_object('task_id','61000000-0000-0000-0000-000000000090','work_date','2026-10-05','estimate_minutes',60,'created_at',1791158400000,'updated_at',1791158460000,'version',2)
  );
  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000152','task_daily_estimate.set','task_daily_estimate',
    '61000000-0000-0000-0000-000000000090|2026-10-06',null,
    jsonb_build_object('task_id','61000000-0000-0000-0000-000000000090','work_date','2026-10-06','estimate_minutes',30,'created_at',1791244800000,'updated_at',1791244800000,'version',1)
  );
  reset role;
  if (select estimate_minutes from timegenie.task_daily_estimates where task_id='61000000-0000-0000-0000-000000000090' and work_date='2026-10-05') <> 60
     or (select estimate_minutes from timegenie.task_daily_estimates where task_id='61000000-0000-0000-0000-000000000090' and work_date='2026-10-06') <> 30 then
    raise exception 'daily estimate dates were not independent';
  end if;
  set role authenticated;

  begin
    perform timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002',
      '40000000-0000-0000-0000-000000000153','task_daily_estimate.set','task_daily_estimate',
      '61000000-0000-0000-0000-000000000090|2026-10-05',1,
      jsonb_build_object('task_id','61000000-0000-0000-0000-000000000090','work_date','2026-10-05','estimate_minutes',90,'created_at',1791158400000,'updated_at',1791158520000,'version',2)
    );
    raise exception 'stale daily estimate was accepted';
  exception when others then if sqlerrm not like '%SYNC_CONFLICT%' then raise; end if; end;

  first_result := timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000154','task_daily_estimate.clear','task_daily_estimate',
    '61000000-0000-0000-0000-000000000090|2026-10-05',2,
    jsonb_build_object('task_id','61000000-0000-0000-0000-000000000090','work_date','2026-10-05','deleted',true,'updated_at',1791158580000,'version',3)
  );
  retry_result := timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000154','task_daily_estimate.clear','task_daily_estimate',
    '61000000-0000-0000-0000-000000000090|2026-10-05',2,
    jsonb_build_object('task_id','61000000-0000-0000-0000-000000000090','work_date','2026-10-05','deleted',true,'updated_at',1791158580000,'version',3)
  );
  reset role;
  if first_result is distinct from retry_result or exists(select 1 from timegenie.task_daily_estimates where task_id='61000000-0000-0000-0000-000000000090' and work_date='2026-10-05') then
    raise exception 'daily estimate clear was not idempotent';
  end if;
  if not exists(
    select 1 from timegenie.workspace_changes
    where workspace_id='20000000-0000-0000-0000-000000000001'
      and entity_type='task_daily_estimates'
      and entity_id='61000000-0000-0000-0000-000000000090|2026-10-05'
      and operation='delete'
  ) then
    raise exception 'daily estimate clear did not publish its composite delete identity';
  end if;
  set role authenticated;
end $$;

-- Recurrence changes are one task-scoped atomic operation: rule history and the
-- task version advance together, while stale devices leave no partial rows.
do $$
declare before_rules jsonb; first_result jsonb; retry_result jsonb;
begin
  first_result := timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000160','task_recurrence.save','task_recurrence_rule',
    '61000000-0000-0000-0000-000000000090',null,
    jsonb_build_object('action','save','task_id','61000000-0000-0000-0000-000000000090','task_expected_version',1,'task_version',2,'rule_id','91000000-0000-0000-0000-000000000090','frequency','daily','weekdays_mask',null,'effective_start','2026-10-05','version',1,'rules',jsonb_build_array(jsonb_build_object('id','91000000-0000-0000-0000-000000000090','task_id','61000000-0000-0000-0000-000000000090','frequency','daily','weekdays_mask',null,'effective_start','2026-10-05','effective_end',null,'created_at',1791158400000,'updated_at',1791158400000,'version',1)))
  );
  retry_result := timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000160','task_recurrence.save','task_recurrence_rule',
    '61000000-0000-0000-0000-000000000090',null,
    jsonb_build_object('action','save','task_id','61000000-0000-0000-0000-000000000090','task_expected_version',1,'task_version',2,'rule_id','91000000-0000-0000-0000-000000000090','frequency','daily','weekdays_mask',null,'effective_start','2026-10-05','version',1,'rules',jsonb_build_array(jsonb_build_object('id','91000000-0000-0000-0000-000000000090','task_id','61000000-0000-0000-0000-000000000090','frequency','daily','weekdays_mask',null,'effective_start','2026-10-05','effective_end',null,'created_at',1791158400000,'updated_at',1791158400000,'version',1)))
  );
  if first_result is distinct from retry_result then raise exception 'recurrence retry changed result'; end if;

  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000161','task_recurrence.save','task_recurrence_rule',
    '61000000-0000-0000-0000-000000000090',1,
    jsonb_build_object('action','save','task_id','61000000-0000-0000-0000-000000000090','task_expected_version',2,'task_version',3,'rule_expected_version',1,'rule_id','91000000-0000-0000-0000-000000000090','frequency','weekdays','weekdays_mask',null,'effective_start','2026-10-05','version',2,'rules',jsonb_build_array(jsonb_build_object('id','91000000-0000-0000-0000-000000000090','task_id','61000000-0000-0000-0000-000000000090','frequency','weekdays','weekdays_mask',null,'effective_start','2026-10-05','effective_end',null,'created_at',1791158400000,'updated_at',1791158460000,'version',2)))
  );
  reset role;
  before_rules := (select jsonb_agg(to_jsonb(r) order by effective_start,id) from timegenie.task_recurrence_rules r where task_id='61000000-0000-0000-0000-000000000090');
  set role authenticated;
  begin
    perform timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002',
      '40000000-0000-0000-0000-000000000162','task_recurrence.save','task_recurrence_rule',
      '61000000-0000-0000-0000-000000000090',1,
      jsonb_build_object('action','save','task_id','61000000-0000-0000-0000-000000000090','task_expected_version',2,'task_version',3,'rule_expected_version',1,'rule_id','91000000-0000-0000-0000-000000000091','frequency','weekly','weekdays_mask',5,'effective_start','2026-10-06','version',1,'rules',jsonb_build_array())
    );
    raise exception 'stale recurrence update was accepted';
  exception when others then if sqlerrm not like '%SYNC_CONFLICT%' then raise; end if; end;
  reset role;
  if before_rules is distinct from (select jsonb_agg(to_jsonb(r) order by effective_start,id) from timegenie.task_recurrence_rules r where task_id='61000000-0000-0000-0000-000000000090') then
    raise exception 'stale recurrence changed rule rows';
  end if;
  set role authenticated;

  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000163','task_recurrence.close','task_recurrence_rule',
    '61000000-0000-0000-0000-000000000090',2,
    jsonb_build_object('action','close','task_id','61000000-0000-0000-0000-000000000090','task_expected_version',3,'task_version',4,'rule_expected_version',2,'rule_id','91000000-0000-0000-0000-000000000090','effective_end','2026-10-05','version',3,'rules',jsonb_build_array(jsonb_build_object('id','91000000-0000-0000-0000-000000000090','task_id','61000000-0000-0000-0000-000000000090','frequency','weekdays','weekdays_mask',null,'effective_start','2026-10-05','effective_end','2026-10-05','created_at',1791158400000,'updated_at',1791158520000,'version',3)))
  );
  reset role;
  if exists(select 1 from timegenie.task_recurrence_rules where task_id='61000000-0000-0000-0000-000000000090' and effective_end is null)
     or (select version from timegenie.tasks where id='61000000-0000-0000-0000-000000000090') <> 4 then
    raise exception 'recurrence close did not atomically update task and rule';
  end if;
  set role authenticated;
end $$;

-- Cross-day replacements create a new rule id while preserving the old
-- interval. Closing a rule before its start removes it, and both update/delete
-- changes stay keyed by task id for incremental refresh.
reset role;
insert into timegenie.tasks(id, workspace_id, subject_id, title, status, source_type, sort_order, version) values
  ('61000000-0000-0000-0000-000000000091','20000000-0000-0000-0000-000000000001','51000000-0000-0000-0000-000000000090','跨日规则','open','manual',91,1),
  ('61000000-0000-0000-0000-000000000092','20000000-0000-0000-0000-000000000001','51000000-0000-0000-0000-000000000090','未来规则','open','manual',92,1);
set role authenticated;
do $$
begin
  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000164','task_recurrence.save','task_recurrence_rule',
    '61000000-0000-0000-0000-000000000091',null,
    jsonb_build_object('action','save','task_id','61000000-0000-0000-0000-000000000091','task_expected_version',1,'task_version',2,'rule_id','91000000-0000-0000-0000-000000000091','frequency','daily','weekdays_mask',null,'effective_start','2026-10-05','version',1,'rules',jsonb_build_array(jsonb_build_object('id','91000000-0000-0000-0000-000000000091','task_id','61000000-0000-0000-0000-000000000091','frequency','daily','weekdays_mask',null,'effective_start','2026-10-05','effective_end',null,'created_at',1791158400000,'updated_at',1791158400000,'version',1)))
  );
  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000165','task_recurrence.save','task_recurrence_rule',
    '61000000-0000-0000-0000-000000000091',1,
    jsonb_build_object('action','save','task_id','61000000-0000-0000-0000-000000000091','task_expected_version',2,'task_version',3,'rule_expected_version',1,'rule_id','91000000-0000-0000-0000-000000000092','frequency','weekly','weekdays_mask',21,'effective_start','2026-10-06','version',1,'rules',jsonb_build_array(
      jsonb_build_object('id','91000000-0000-0000-0000-000000000091','task_id','61000000-0000-0000-0000-000000000091','frequency','daily','weekdays_mask',null,'effective_start','2026-10-05','effective_end','2026-10-05','created_at',1791158400000,'updated_at',1791244800000,'version',2),
      jsonb_build_object('id','91000000-0000-0000-0000-000000000092','task_id','61000000-0000-0000-0000-000000000091','frequency','weekly','weekdays_mask',21,'effective_start','2026-10-06','effective_end',null,'created_at',1791244800000,'updated_at',1791244800000,'version',1)
    ))
  );

  reset role;
  if (select count(*) from timegenie.task_recurrence_rules where task_id='61000000-0000-0000-0000-000000000091') <> 2
     or (select effective_end from timegenie.task_recurrence_rules where id='91000000-0000-0000-0000-000000000091') <> '2026-10-05'::date
     or not exists(select 1 from timegenie.task_recurrence_rules where id='91000000-0000-0000-0000-000000000092' and frequency='weekly' and weekdays_mask=21 and effective_end is null) then
    raise exception 'cross-day recurrence replacement did not preserve history';
  end if;
  if not exists(
    select 1 from timegenie.workspace_changes
    where entity_type='task_recurrence_rules'
      and entity_id='61000000-0000-0000-0000-000000000091'
      and operation in ('insert','update')
  ) then
    raise exception 'recurrence replacement did not publish task-scoped changes';
  end if;
  set role authenticated;

  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000166','task_recurrence.save','task_recurrence_rule',
    '61000000-0000-0000-0000-000000000092',null,
    jsonb_build_object('action','save','task_id','61000000-0000-0000-0000-000000000092','task_expected_version',1,'task_version',2,'rule_id','91000000-0000-0000-0000-000000000093','frequency','weekdays','weekdays_mask',null,'effective_start','2026-10-10','version',1,'rules',jsonb_build_array(jsonb_build_object('id','91000000-0000-0000-0000-000000000093','task_id','61000000-0000-0000-0000-000000000092','frequency','weekdays','weekdays_mask',null,'effective_start','2026-10-10','effective_end',null,'created_at',1791590400000,'updated_at',1791590400000,'version',1)))
  );
  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000167','task_recurrence.close','task_recurrence_rule',
    '61000000-0000-0000-0000-000000000092',1,
    jsonb_build_object('action','close','task_id','61000000-0000-0000-0000-000000000092','task_expected_version',2,'task_version',3,'rule_expected_version',1,'rule_id','91000000-0000-0000-0000-000000000093','effective_end','2026-10-09','version',2,'rules',jsonb_build_array())
  );
  reset role;
  if exists(select 1 from timegenie.task_recurrence_rules where id='91000000-0000-0000-0000-000000000093') then
    raise exception 'closing a future recurrence did not delete the rule';
  end if;
  if not exists(
    select 1 from timegenie.workspace_changes
    where entity_type='task_recurrence_rules'
      and entity_id='61000000-0000-0000-0000-000000000092'
      and operation='delete'
  ) then
    raise exception 'future recurrence delete did not publish its task identity';
  end if;
  set role authenticated;
end $$;

-- Coalesced timer snapshots may jump only across the exact number of local
-- operations declared by the client. The final image is applied once and the
-- original operation id remains the sole idempotency boundary.
reset role;
update timegenie.tracking_leases set expires_at=clock_timestamp()-interval '1 second'
where workspace_id='20000000-0000-0000-0000-000000000001';
set local role authenticated;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
do $$
declare timer_payload jsonb; first_result jsonb; retry_result jsonb; timer_change_count bigint;
declare sync_context jsonb; rejected jsonb;
begin
  -- Device 2 holds the background lease; device 1 timer writes must still be accepted.
  perform timegenie.tracking_lease_acquire(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000002',
    '49000000-0000-0000-0000-000000000001'
  );
  timer_payload := jsonb_build_object(
    'id', '70000000-0000-0000-0000-000000000099',
    'work_date', '2026-09-30',
    'kind', 'work',
    'source_type', 'timer',
    'state', 'ended',
    'default_task_id', '60000000-0000-0000-0000-000000000002',
    'label_snapshot', '运维相关 / 合并计时验证',
    'started_at', 1790730000000,
    'ended_at', 1790731800000,
    'duration_seconds', 1800,
    'note', '开始、暂停并结束后的最终镜像',
    'created_at', 1790730000000,
    'updated_at', 1790731800000,
    'version', 3,
    'segments', jsonb_build_array(jsonb_build_object(
      'id', '73000000-0000-0000-0000-000000000099',
      'sequence_no', 1,
      'started_at', 1790730000000,
      'ended_at', 1790731800000,
      'duration_seconds', 1800
    )),
    'allocations', jsonb_build_array()
  );
  select count(*) into timer_change_count
  from timegenie.workspace_changes
  where workspace_id = '20000000-0000-0000-0000-000000000001'
    and entity_type = 'time_entries'
    and entity_id = '70000000-0000-0000-0000-000000000099';
  first_result := timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000090',
    'timer_stop',
    'time_entry',
    '70000000-0000-0000-0000-000000000099',
    null,
    3,
    3,
    timer_payload
  );
  retry_result := timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000090',
    'timer_stop',
    'time_entry',
    '70000000-0000-0000-0000-000000000099',
    null,
    3,
    3,
    timer_payload
  );
  if first_result is distinct from retry_result then
    raise exception 'coalesced timer retry returned a different result';
  end if;
  sync_context:=timegenie.timer_sync_context_get(
    '20000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000090',
    '70000000-0000-0000-0000-000000000099',
    'timer_stop',
    jsonb_build_object(
      'deviceId','30000000-0000-0000-0000-000000000001',
      'entityType','time_entry',
      'entityId','70000000-0000-0000-0000-000000000099',
      'baseVersion',null,
      'payloadVersion',3,
      'coalescedCount',3,
      'payload',timer_payload
    )
  );
  if sync_context->'processedOperation'->>'operationType'<>'timer_stop'
     or not coalesce((sync_context->'processedOperation'->>'matchesRequest')::boolean,false)
     or (sync_context->>'currentVersion')::bigint<>3 then
    raise exception 'timer sync context did not confirm the processed operation: %',sync_context;
  end if;
  sync_context:=timegenie.timer_sync_context_get(
    '20000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000090',
    '70000000-0000-0000-0000-000000000099',
    'timer_stop',
    jsonb_build_object(
      'deviceId','30000000-0000-0000-0000-000000000001',
      'entityType','time_entry',
      'entityId','70000000-0000-0000-0000-000000000099',
      'baseVersion',null,
      'payloadVersion',3,
      'coalescedCount',3,
      'payload',jsonb_set(timer_payload,'{note}','"different request"'::jsonb,true)
    )
  );
  if coalesce((sync_context->'processedOperation'->>'matchesRequest')::boolean,true) then
    raise exception 'timer sync context accepted different content for an existing operation id';
  end if;
  if (select version from timegenie.time_entries where id = '70000000-0000-0000-0000-000000000099') <> 3 then
    raise exception 'coalesced timer final image was not applied at version 3';
  end if;
  if (
    select count(*) from timegenie.workspace_changes
    where workspace_id = '20000000-0000-0000-0000-000000000001'
      and entity_type = 'time_entries'
      and entity_id = '70000000-0000-0000-0000-000000000099'
  ) <> timer_change_count + 1 then
    raise exception 'coalesced timer retry created duplicate cloud writes: before %, after %, first %, retry %',
      timer_change_count,
      (select count(*) from timegenie.workspace_changes where workspace_id='20000000-0000-0000-0000-000000000001' and entity_type='time_entries' and entity_id='70000000-0000-0000-0000-000000000099'),
      first_result,retry_result;
  end if;

  begin
    perform timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000001',
      '40000000-0000-0000-0000-000000000094',
      'time_allocation_replace',
      'time_entry',
      '70000000-0000-0000-0000-000000000099',
      3,
      4,
      null,
      jsonb_set(timer_payload, '{version}', '4'::jsonb, true)
    );
    raise exception 'coalesced timer request without chain proof was accepted';
  exception
    when others then
      if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if;
  end;

  begin
    perform timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000001',
      '40000000-0000-0000-0000-000000000091',
      'time_allocation_replace',
      'time_entry',
      '70000000-0000-0000-0000-000000000099',
      3,
      4,
      2,
      jsonb_set(timer_payload, '{version}', '4'::jsonb, true)
    );
    raise exception 'mismatched coalesced timer span was accepted';
  exception
    when others then
      if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if;
  end;

  rejected:=timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000002',
      '40000000-0000-0000-0000-000000000092',
      'timer_stop',
      'time_entry',
      '70000000-0000-0000-0000-000000000099',
      2,
      4,
      2,
      jsonb_set(timer_payload, '{version}', '4'::jsonb, true)
    );
  if coalesce((rejected->>'accepted')::boolean,true)
     or rejected->'error'->>'code'<>'SYNC_CONFLICT'
     or (rejected->>'currentVersion')::bigint<>3
     or rejected->'authoritative'->>'id'<>'70000000-0000-0000-0000-000000000099' then
    raise exception 'competing timer write did not return the authoritative first-arrival result: %',rejected;
  end if;

  begin
    perform timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000001',
      '40000000-0000-0000-0000-000000000093',
      'subject.create',
      'subject',
      '61000000-0000-0000-0000-000000000099',
      null,
      2,
      2,
      jsonb_build_object(
        'id', '61000000-0000-0000-0000-000000000099',
        'name', '普通实体不可跳版本',
        'sort_order', 99,
        'created_at', 1790730000000,
        'updated_at', 1790730000000,
        'version', 2
      )
    );
    raise exception 'non-timer entity used coalesced proof to jump versions';
  exception
    when others then
      if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if;
  end;

  if not timegenie.tracking_lease_release(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000002',
    '49000000-0000-0000-0000-000000000001'
  ) then
    raise exception 'non-holder timer write test failed to release its fixture lease';
  end if;
end $$;

-- Lease acquire/renew/release and unassigned ticks must never mutate an
-- ordinary timer. The holder can heartbeat repeatedly without changing the
-- timer image, while a live lease still blocks another device.
reset role;
do $$
declare timer_id uuid:='70000000-0000-0000-0000-000000000098'; timer_version bigint;
declare timer_updated timestamptz; segment_image jsonb; lease_result jsonb;
begin
  delete from timegenie.time_segments where entry_id=timer_id;
  delete from timegenie.time_entries where id=timer_id;
  insert into timegenie.time_entries(
    id,workspace_id,work_date,kind,source_type,state,default_task_id,label_snapshot,
    started_at,duration_seconds,created_at,updated_at,version,created_by_device_id,
    updated_by_device_id,timer_chain_id,last_continuous_at
  ) values(
    timer_id,'20000000-0000-0000-0000-000000000001',timegenie.workspace_work_date(
      '20000000-0000-0000-0000-000000000001',clock_timestamp()
    ),'work','timer','running','60000000-0000-0000-0000-000000000002',
    '租约心跳不修改计时',clock_timestamp()-interval '10 minutes',0,clock_timestamp()-interval '10 minutes',
    clock_timestamp()-interval '2 minutes',7,'30000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',timer_id,clock_timestamp()-interval '2 minutes'
  );
  insert into timegenie.time_segments(
    id,workspace_id,entry_id,sequence_no,started_at,duration_seconds
  ) values(
    '73000000-0000-0000-0000-000000000098',
    '20000000-0000-0000-0000-000000000001',timer_id,1,clock_timestamp()-interval '10 minutes',0
  );
  select version,updated_at into timer_version,timer_updated from timegenie.time_entries where id=timer_id;
  select jsonb_agg(to_jsonb(segment) order by sequence_no) into segment_image
  from timegenie.time_segments segment where entry_id=timer_id;
  perform set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
  set local role authenticated;
  lease_result:=timegenie.tracking_lease_acquire(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '49000000-0000-0000-0000-000000000098'
  );
  if not coalesce((lease_result->>'acquired')::boolean,false) then
    raise exception 'timer holder failed to acquire lease for heartbeat isolation test';
  end if;
  perform timegenie.tracking_lease_renew(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '49000000-0000-0000-0000-000000000098'
  );
  perform timegenie.unassigned_tick(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '49000000-0000-0000-0000-000000000098'
  );
  reset role;
  if not exists(
    select 1 from timegenie.time_entries where id=timer_id
      and version=timer_version and updated_at=timer_updated and state='running'
  ) or (
    select jsonb_agg(to_jsonb(segment) order by sequence_no)
    from timegenie.time_segments segment where entry_id=timer_id
  ) is distinct from segment_image then
    raise exception 'lease heartbeat mutated the ordinary timer image';
  end if;
  set local role authenticated;
  if coalesce((timegenie.tracking_lease_acquire(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000002',
    '49000000-0000-0000-0000-000000000099'
  )->>'acquired')::boolean,false) then
    raise exception 'live timer lease was stolen by another device';
  end if;
  perform timegenie.tracking_lease_release(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '49000000-0000-0000-0000-000000000098'
  );
  reset role;
  if not exists(select 1 from timegenie.time_entries where id=timer_id and version=timer_version) then
    raise exception 'lease release mutated the ordinary timer version';
  end if;
  delete from timegenie.time_segments where entry_id=timer_id;
  delete from timegenie.time_entries where id=timer_id;
end $$;

-- Only one global timer may exist. Either registered device can control the
-- shared timer when it supplies the current optimistic version.
do $$
declare timer_row jsonb;
begin
  timer_row := timegenie.timer_start(
    '20000000-0000-0000-0000-000000000001',
    '60000000-0000-0000-0000-000000000002',
    '30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000020',
    '设备 A 启动'
  );

  begin
    perform timegenie.timer_start(
      '20000000-0000-0000-0000-000000000001',
      '60000000-0000-0000-0000-000000000002',
      '30000000-0000-0000-0000-000000000002',
      '40000000-0000-0000-0000-000000000021',
      '设备 B 重复启动'
    );
    raise exception 'parallel timer was accepted';
  exception
    when others then
      if sqlerrm not like '%REMOTE_TIMER_ACTIVE%' then raise; end if;
  end;

  begin
    perform timegenie.timer_pause(
      '20000000-0000-0000-0000-000000000001',
      (timer_row->>'id')::uuid,
      '30000000-0000-0000-0000-000000000002',
      99,
      '40000000-0000-0000-0000-000000000022'
    );
    raise exception 'stale timer action was accepted';
  exception
    when others then
      if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if;
  end;

  timer_row := timegenie.timer_pause(
    '20000000-0000-0000-0000-000000000001',
    (timer_row->>'id')::uuid,
    '30000000-0000-0000-0000-000000000002',
    (timer_row->>'version')::bigint,
    '40000000-0000-0000-0000-000000000023'
  );
  timer_row := timegenie.timer_resume(
    '20000000-0000-0000-0000-000000000001',
    (timer_row->>'id')::uuid,
    '30000000-0000-0000-0000-000000000002',
    (timer_row->>'version')::bigint,
    '40000000-0000-0000-0000-000000000024'
  );
  timer_row := timegenie.timer_stop(
    '20000000-0000-0000-0000-000000000001',
    (timer_row->>'id')::uuid,
    '30000000-0000-0000-0000-000000000001',
    (timer_row->>'version')::bigint,
    '40000000-0000-0000-0000-000000000025'
  );

  if timer_row->>'state' <> 'ended' then
    raise exception 'global timer did not stop';
  end if;
end $$;

-- The background collector has one lease holder and one open segment. A
-- takeover after expiry closes the old segment and opens exactly one new one.
do $$
declare lease_result jsonb;
begin
  lease_result := timegenie.tracking_lease_acquire(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '90000000-0000-0000-0000-000000000001'
  );
  if not (lease_result->>'acquired')::boolean then raise exception 'device A did not acquire lease'; end if;

  lease_result := timegenie.tracking_lease_acquire(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000002',
    '90000000-0000-0000-0000-000000000002'
  );
  if (lease_result->>'acquired')::boolean then raise exception 'device B stole a live lease'; end if;

  begin
    perform timegenie.tracking_lease_renew(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000001',
      '90000000-0000-0000-0000-000000000099'
    );
    raise exception 'wrong lease token was renewed';
  exception
    when others then
      if sqlerrm not like '%LEASE_EXPIRED%' then raise; end if;
  end;

end $$;

reset role;
update timegenie.tracking_leases set expires_at = now() - interval '1 second'
where workspace_id = '20000000-0000-0000-0000-000000000001';

set local role authenticated;
select set_config('request.jwt.claim.sub', '10000000-0000-0000-0000-000000000001', true);

do $$
declare lease_result jsonb;
begin
  lease_result := timegenie.tracking_lease_acquire(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000002',
    '90000000-0000-0000-0000-000000000002'
  );
  if not (lease_result->>'acquired')::boolean then raise exception 'device B did not acquire expired lease'; end if;

  begin
    perform timegenie.unassigned_tick(
      '20000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000001',
      '90000000-0000-0000-0000-000000000001'
    );
    raise exception 'expired holder advanced unassigned time';
  exception
    when others then
      if sqlerrm not like '%LEASE_EXPIRED%' then raise; end if;
  end;

  perform timegenie.unassigned_tick(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000002',
    '90000000-0000-0000-0000-000000000002'
  );
end $$;

reset role;

-- Calendar-day helpers use the workspace IANA timezone, including DST days.
do $$
begin
  update timegenie.workspaces set timezone='America/New_York'
  where id='20000000-0000-0000-0000-000000000001';
  if timegenie.workspace_work_date('20000000-0000-0000-0000-000000000001','2026-03-08 04:30:00+00') <> date '2026-03-07' then
    raise exception 'workspace date ignored America/New_York timezone';
  end if;
  if extract(epoch from (
    timegenie.workspace_day_end('20000000-0000-0000-0000-000000000001',date '2026-03-08') -
    timegenie.workspace_day_start('20000000-0000-0000-0000-000000000001',date '2026-03-08')
  ))::bigint <> 82800 then
    raise exception 'DST spring day was treated as fixed 24 hours';
  end if;
  update timegenie.workspaces set timezone='Asia/Shanghai'
  where id='20000000-0000-0000-0000-000000000001';
end $$;

-- The same timer chain/date candidate converges to one authoritative row.
reset role;
update timegenie.tracking_leases set expires_at=clock_timestamp()-interval '1 second'
where workspace_id='20000000-0000-0000-0000-000000000001';
set local role authenticated;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
do $$
declare first_result jsonb; second_result jsonb; chain_id uuid:='74000000-0000-0000-0000-000000000001';
begin
  perform timegenie.tracking_lease_acquire(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '49000000-0000-0000-0000-000000000097'
  );
  first_result:=timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '44000000-0000-0000-0000-000000000001','timer_rollover_create','time_entry',
    '74100000-0000-0000-0000-000000000001',null,
    jsonb_build_object('id','74100000-0000-0000-0000-000000000001','work_date','2026-10-06','kind','work','source_type','timer','state','ended',
      'label_snapshot','日切测试','started_at',1791216000000,'ended_at',1791216060000,'duration_seconds',60,'created_at',1791216000000,
      'updated_at',1791216060000,'version',1,'timer_chain_id',chain_id::text,'last_continuous_at',1791216060000,'segments','[]'::jsonb,'allocations','[]'::jsonb)
  );
  second_result:=timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '44000000-0000-0000-0000-000000000002','timer_rollover_create','time_entry',
    '74100000-0000-0000-0000-000000000002',null,
    jsonb_build_object('id','74100000-0000-0000-0000-000000000002','work_date','2026-10-06','kind','work','source_type','timer','state','ended',
      'label_snapshot','竞争候选','started_at',1791216000000,'ended_at',1791216060000,'duration_seconds',60,'created_at',1791216000000,
      'updated_at',1791216060000,'version',1,'timer_chain_id',chain_id::text,'last_continuous_at',1791216060000,'segments','[]'::jsonb,'allocations','[]'::jsonb)
  );
  if not coalesce((second_result->>'superseded')::boolean,false)
     or second_result->'authoritative'->>'id'<>'74100000-0000-0000-0000-000000000001' then
    raise exception 'timer chain/date candidates did not converge to the first authoritative row: first %, second %',first_result,second_result;
  end if;
  if (select count(*) from timegenie.time_entries where timer_chain_id=chain_id and work_date='2026-10-06')<>1 then
    raise exception 'timer chain/date uniqueness was not preserved';
  end if;
end $$;
reset role;

-- A stale session date must never close a later-started segment before its start.
reset role;
delete from timegenie.unassigned_segments
where session_id in (select id from timegenie.unassigned_sessions where workspace_id='20000000-0000-0000-0000-000000000001' and state in ('collecting','awaiting_resolution'));
delete from timegenie.unassigned_sessions
where workspace_id='20000000-0000-0000-0000-000000000001' and state in ('collecting','awaiting_resolution');
insert into timegenie.unassigned_sessions(
  id,workspace_id,work_date,state,threshold_seconds,duration_seconds,first_started_at,shared_source,migration_state,created_at,updated_at,version,last_continuous_at
) values(
  '95200000-0000-0000-0000-000000000001','20000000-0000-0000-0000-000000000001','2026-10-04','collecting',300,0,
  '2026-10-06 14:25:42+00','cloud','adopted','2026-10-06 14:25:42+00','2026-10-06 14:25:42+00',1,'2026-10-06 14:25:42+00'
);
insert into timegenie.unassigned_segments(id,workspace_id,session_id,sequence_no,started_at,duration_seconds,lease_token)
values('95210000-0000-0000-0000-000000000001','20000000-0000-0000-0000-000000000001','95200000-0000-0000-0000-000000000001',1,'2026-10-06 14:25:42+00',0,'50000000-0000-0000-0000-000000000001');
update timegenie.tracking_leases set holder_device_id='30000000-0000-0000-0000-000000000001',
  lease_token='50000000-0000-0000-0000-000000000001',expires_at='2026-10-07 01:00:00+00',last_confirmed_at='2026-10-07 00:00:00+00'
where workspace_id='20000000-0000-0000-0000-000000000001';
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
select timegenie.coordinate_unassigned_calendar_day_at(
  '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
  '50000000-0000-0000-0000-000000000001','2026-10-07 00:00:00+00'
);
do $$
begin
  if exists(select 1 from timegenie.unassigned_segments where id='95210000-0000-0000-0000-000000000001' and ended_at<started_at) then
    raise exception 'calendar rollover closed an unassigned segment before it started';
  end if;
  if not exists(select 1 from timegenie.unassigned_segments where id='95210000-0000-0000-0000-000000000001' and ended_at=started_at and duration_seconds=0) then
    raise exception 'stale-date unassigned segment was not closed safely at zero duration';
  end if;
  if not exists(select 1 from timegenie.unassigned_sessions where id='95200000-0000-0000-0000-000000000001' and state='discarded' and resolution_type='discard' and resolved_at is not null) then
    raise exception 'zero-duration stale session was not finalized with terminal metadata';
  end if;
end $$;

-- Lease expiry, takeover, and release must not mutate the shared unassigned
-- anchor. Only ordinary item timers use lease continuity boundaries.
set local role authenticated;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
do $$
declare first_lease jsonb; second_lease jsonb; anchor_after_takeover jsonb; anchor_after_release jsonb;
declare original_started timestamptz:=clock_timestamp()-interval '120 seconds'; original_version bigint; generated_count bigint;
begin
  first_lease:=timegenie.tracking_lease_acquire('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001','94000000-0000-0000-0000-000000000001');
  reset role;
  delete from timegenie.time_segments where entry_id in (select id from timegenie.time_entries where workspace_id='20000000-0000-0000-0000-000000000001' and state in ('running','paused'));
  delete from timegenie.time_entries where workspace_id='20000000-0000-0000-0000-000000000001' and state in ('running','paused');
  delete from timegenie.unassigned_segments where session_id in (select id from timegenie.unassigned_sessions where workspace_id='20000000-0000-0000-0000-000000000001' and state in ('collecting','awaiting_resolution'));
  delete from timegenie.unassigned_sessions where workspace_id='20000000-0000-0000-0000-000000000001' and state in ('collecting','awaiting_resolution');
  insert into timegenie.unassigned_sessions(id,workspace_id,work_date,state,threshold_seconds,duration_seconds,first_started_at,shared_source,migration_state,created_at,updated_at,version,last_continuous_at)
  values('95000000-0000-0000-0000-000000000001','20000000-0000-0000-0000-000000000001',timegenie.workspace_work_date('20000000-0000-0000-0000-000000000001',clock_timestamp()),
    'collecting',300,0,original_started,'cloud','adopted',original_started,clock_timestamp(),1,original_started)
  on conflict do nothing;
  insert into timegenie.unassigned_segments(id,workspace_id,session_id,sequence_no,started_at,duration_seconds,lease_token)
  values('95100000-0000-0000-0000-000000000001','20000000-0000-0000-0000-000000000001','95000000-0000-0000-0000-000000000001',1,original_started,0,'94000000-0000-0000-0000-000000000001')
  on conflict do nothing;
  update timegenie.unassigned_segments set started_at=original_started
  where lease_token='94000000-0000-0000-0000-000000000001' and ended_at is null;
  select version into original_version from timegenie.unassigned_sessions
  where id='95000000-0000-0000-0000-000000000001';
  update timegenie.tracking_leases set last_confirmed_at=clock_timestamp()-interval '90 seconds',expires_at=clock_timestamp()-interval '45 seconds'
  where workspace_id='20000000-0000-0000-0000-000000000001';
  set local role authenticated;
  perform set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
  second_lease:=timegenie.tracking_lease_acquire('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002','94000000-0000-0000-0000-000000000002');
  reset role;
  if not coalesce((second_lease->>'acquired')::boolean,false) then
    raise exception 'replacement device did not acquire the expired lease';
  end if;
  anchor_after_takeover:=timegenie.timegenie_unassigned_active_anchor('20000000-0000-0000-0000-000000000001',null);
  if anchor_after_takeover is null
     or anchor_after_takeover->>'id'<>'95000000-0000-0000-0000-000000000001'
     or (anchor_after_takeover->>'first_started_at')::timestamptz<>original_started
     or (anchor_after_takeover->>'version')::bigint<>original_version
     or jsonb_array_length(anchor_after_takeover->'segments')<>1
     or anchor_after_takeover->'segments'->0->>'ended_at' is not null
     or (anchor_after_takeover->'segments'->0->>'started_at')::timestamptz<>original_started then
    raise exception 'lease takeover mutated the shared unassigned anchor: %',anchor_after_takeover;
  end if;
  set local role authenticated;
  perform set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
  if not timegenie.tracking_lease_release('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002','94000000-0000-0000-0000-000000000002') then
    raise exception 'replacement device did not release its lease';
  end if;
  reset role;
  anchor_after_release:=timegenie.timegenie_unassigned_active_anchor('20000000-0000-0000-0000-000000000001',null);
  if anchor_after_release is null
     or anchor_after_release->>'id'<>anchor_after_takeover->>'id'
     or (anchor_after_release->>'first_started_at')::timestamptz<>original_started
     or (anchor_after_release->>'version')::bigint<>original_version
     or anchor_after_release->'segments'->0->>'ended_at' is not null then
    raise exception 'lease release mutated the shared unassigned anchor: %',anchor_after_release;
  end if;
  select count(*) into generated_count from timegenie.time_entries
  where origin_unassigned_session_id='95000000-0000-0000-0000-000000000001';
  if generated_count<>0 then
    raise exception 'lease lifecycle generated an unassigned result';
  end if;
end $$;
reset role;

-- Shared unassigned time has one root per workspace. Repeated create calls
-- adopt the same root, only the first concurrent resolution wins, and retrying
-- the winning operation is idempotent.
reset role;
insert into timegenie.subjects(id,workspace_id,name,sort_order)
values('52000000-0000-0000-0000-000000000001','20000000-0000-0000-0000-000000000002','未归属处理',1)
on conflict do nothing;
insert into timegenie.tasks(id,workspace_id,subject_id,title,status,source_type,sort_order,version)
values('62000000-0000-0000-0000-000000000001','20000000-0000-0000-0000-000000000002','52000000-0000-0000-0000-000000000001','未归属处理事项','open','manual',1,1)
on conflict do nothing;
set local role authenticated;
select set_config('request.jwt.claim.sub', '10000000-0000-0000-0000-000000000002', true);

do $$
declare first_session jsonb; second_session jsonb; next_session jsonb; first_result jsonb; retry_result jsonb; loser_result jsonb;
declare reset_result jsonb; reset_retry jsonb; reset_loser jsonb; reset_millis bigint;
declare session_id uuid; session_version bigint; original_started timestamptz; original_updated timestamptz;
begin
  first_session := timegenie.unassigned_get_or_create_shared(
    '20000000-0000-0000-0000-000000000002',
    '30000000-0000-0000-0000-000000000003',
    jsonb_build_object(
      'id','72000000-0000-0000-0000-000000000001',
      'state','collecting','threshold_seconds',300,'duration_seconds',0,
      'first_started_at',1791250000000
    ), null, null
  );
  second_session := timegenie.unassigned_get_or_create_shared(
    '20000000-0000-0000-0000-000000000002',
    '30000000-0000-0000-0000-000000000003',
    jsonb_build_object(
      'id','72000000-0000-0000-0000-000000000002',
      'state','collecting','threshold_seconds',300,'duration_seconds',999,
      'first_started_at',1791250999000
    ), null, null
  );
  if first_session->>'id' is distinct from second_session->>'id' then
    raise exception 'parallel shared unassigned create returned different roots';
  end if;
  if first_session->>'state'<>'collecting'
     or (first_session->>'duration_seconds')::bigint<>0
     or jsonb_array_length(first_session->'segments')<>1
     or first_session->'segments'->0->>'ended_at' is not null
     or second_session->>'first_started_at' is distinct from first_session->>'first_started_at' then
    raise exception 'shared create did not preserve one immutable zero-duration anchor: %, %',first_session,second_session;
  end if;
  session_id := (first_session->>'id')::uuid;
  session_version := (first_session->>'version')::bigint;
  original_started := (first_session->>'first_started_at')::timestamptz;
  original_updated := (first_session->>'updated_at')::timestamptz;
  retry_result := timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000002',
    '30000000-0000-0000-0000-000000000003',
    '42000000-0000-0000-0000-000000000004',
    'unassigned_session_awaiting_resolution',
    'unassigned_session',
    session_id::text,
    session_version,
    jsonb_build_object(
      'id',session_id::text,'work_date','2026-10-06','state','awaiting_resolution',
      'threshold_seconds',300,'duration_seconds',300,'first_started_at',1791250000000,
      'created_at',1791250000000,'updated_at',1791250300000,'version',session_version+1,
      'segments','[]'::jsonb
    )
  );
  if not coalesce((retry_result->>'superseded')::boolean,false)
     or retry_result->'session'->>'state'<>'collecting'
     or (retry_result->'session'->>'version')::bigint<>session_version
     or (retry_result->'session'->>'duration_seconds')::bigint<>0
     or (retry_result->'session'->>'first_started_at')::timestamptz<>original_started then
    raise exception 'legacy awaiting-resolution update changed the simplified anchor: %',retry_result;
  end if;
  retry_result := timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000002',
    '30000000-0000-0000-0000-000000000003',
    '42000000-0000-0000-0000-000000000005',
    'unassigned_session_awaiting_resolution',
    'unassigned_session',
    session_id::text,
    session_version,
    jsonb_build_object(
      'id',session_id::text,'work_date','2026-10-06','state','awaiting_resolution',
      'threshold_seconds',300,'duration_seconds',300,'first_started_at',1791250000000,
      'created_at',1791250000000,'updated_at',1791250300001,'version',session_version+1,
      'segments','[]'::jsonb
    )
  );
  if not coalesce((retry_result->>'superseded')::boolean,false)
     or retry_result->'session'->>'state'<>'collecting'
     or (retry_result->'session'->>'version')::bigint<>session_version
     or (retry_result->'session'->>'updated_at')::timestamptz<>original_updated then
    raise exception 'duplicate legacy lifecycle update mutated the current anchor';
  end if;
  reset_millis:=floor(extract(epoch from clock_timestamp())*1000)::bigint-60000;
  reset_result:=timegenie.unassigned_anchor_reset(
    '20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000003',
    '42000000-0000-0000-0000-000000000006',session_id,session_version,reset_millis
  );
  reset_retry:=timegenie.unassigned_anchor_reset(
    '20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000003',
    '42000000-0000-0000-0000-000000000006',session_id,session_version,reset_millis
  );
  reset_loser:=timegenie.unassigned_anchor_reset(
    '20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000003',
    '42000000-0000-0000-0000-000000000007',session_id,session_version,reset_millis-60000
  );
  if not coalesce((reset_result->>'accepted')::boolean,false)
     or reset_retry is distinct from reset_result
     or coalesce((reset_loser->>'accepted')::boolean,false)
     or reset_loser->'session'->>'id'<>session_id::text
     or (reset_loser->'session'->>'first_started_at')::timestamptz<>(reset_result->'session'->>'first_started_at')::timestamptz then
    raise exception 'anchor reset retry or stale reset did not converge: %, %, %',reset_result,reset_retry,reset_loser;
  end if;
  session_version:=(reset_result->'session'->>'version')::bigint;
  first_result := timegenie.unassigned_resolve_shared(
    '20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000003',
    '42000000-0000-0000-0000-000000000001',session_id,session_version,'work',
    jsonb_build_object('generated_entry_id','73000000-0000-0000-0000-000000000001','generated_entry',jsonb_build_object(
      'id','73000000-0000-0000-0000-000000000001','origin_unassigned_session_id',session_id::text,
      'label_snapshot','未归属处理事项','note',null,'allocations',jsonb_build_array(jsonb_build_object(
        'id','73100000-0000-0000-0000-000000000001','task_id','62000000-0000-0000-0000-000000000001','minutes',1,'note',null
      ))
    ))
  );
  retry_result := timegenie.unassigned_resolve_shared(
    '20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000003',
    '42000000-0000-0000-0000-000000000001',session_id,session_version,'work',
    jsonb_build_object('generated_entry_id','73000000-0000-0000-0000-000000000001','generated_entry',jsonb_build_object(
      'id','73000000-0000-0000-0000-000000000001','origin_unassigned_session_id',session_id::text,
      'label_snapshot','未归属处理事项','note',null,'allocations',jsonb_build_array(jsonb_build_object(
        'id','73100000-0000-0000-0000-000000000001','task_id','62000000-0000-0000-0000-000000000001','minutes',1,'note',null
      ))
    ))
  );
  loser_result := timegenie.unassigned_resolve_shared(
    '20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000003',
    '42000000-0000-0000-0000-000000000002',session_id,session_version,'break','{}'::jsonb
  );
  if not (first_result->>'accepted')::boolean or retry_result <> first_result then
    raise exception 'shared unassigned resolution retry was not idempotent';
  end if;
  if (loser_result->>'accepted')::boolean or loser_result->'session'->>'resolution_type' <> 'work' then
    raise exception 'concurrent shared unassigned loser did not adopt winner';
  end if;
  if first_result->'session'->>'resolved_at' is null then
    raise exception 'shared unassigned resolution did not use server resolved_at';
  end if;
  if first_result->'generatedEntry'->>'id'<>'73000000-0000-0000-0000-000000000001'
     or first_result->'session'->>'generated_entry_id'<>'73000000-0000-0000-0000-000000000001'
     or (first_result->'generatedEntry'->>'started_at')::timestamptz<>(first_result->'session'->>'first_started_at')::timestamptz
     or (first_result->'generatedEntry'->>'ended_at')::timestamptz<>(first_result->'session'->>'resolved_at')::timestamptz
     or (first_result->'generatedEntry'->>'duration_seconds')::bigint<>(first_result->'session'->>'duration_seconds')::bigint then
    raise exception 'shared resolution did not atomically create the authoritative final entry: %',first_result;
  end if;
  next_session:=timegenie.unassigned_get_or_create_shared(
    '20000000-0000-0000-0000-000000000002',
    '30000000-0000-0000-0000-000000000003',null,null,null
  );
  if first_result->'nextSession' is null
     or first_result->'nextSession'->>'predecessor_session_id'<>session_id::text
     or (first_result->'nextSession'->>'first_started_at')::timestamptz
        <> (first_result->'session'->>'resolved_at')::timestamptz
     or next_session->>'id'<>first_result->'nextSession'->>'id' then
    raise exception 'resolution did not establish exactly one next anchor at the authoritative boundary: %',first_result;
  end if;
  retry_result := timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000002',
    '30000000-0000-0000-0000-000000000003',
    '42000000-0000-0000-0000-000000000003',
    'unassigned_session_awaiting_resolution',
    'unassigned_session',
    session_id::text,
    session_version,
    jsonb_build_object(
      'id',session_id::text,'work_date','2026-10-06','state','awaiting_resolution',
      'threshold_seconds',300,'duration_seconds',300,'first_started_at',1791250999000,
      'created_at',1791250999000,'updated_at',1791251299000,'version',session_version+1,
      'segments','[]'::jsonb
    )
  );
  if not coalesce((retry_result->>'superseded')::boolean,false)
     or retry_result->'session'->>'resolution_type' <> 'work' then
    raise exception 'superseded shared unassigned lifecycle did not adopt terminal session';
  end if;
end $$;

-- Timer start clears the current anchor in the same cloud operation; timer
-- stop creates exactly one new anchor at its authoritative ended_at boundary.
do $$
declare start_payload jsonb; stop_payload jsonb; first_stop jsonb; retry_stop jsonb; active_anchor jsonb; start_millis bigint; stop_millis bigint;
begin
  perform timegenie.tracking_lease_acquire(
    '20000000-0000-0000-0000-000000000002',
    '30000000-0000-0000-0000-000000000003',
    '49000000-0000-0000-0000-000000000096'
  );
  start_millis:=floor(extract(epoch from clock_timestamp())*1000)::bigint;
  stop_millis:=start_millis+60000;
  start_payload:=jsonb_build_object(
    'id','74000000-0000-0000-0000-000000000001','work_date',timegenie.workspace_work_date('20000000-0000-0000-0000-000000000002',to_timestamp(start_millis::double precision/1000)),
    'kind','work','source_type','timer','state','running','default_task_id','62000000-0000-0000-0000-000000000001',
    'label_snapshot','原子计时验证','started_at',start_millis,'ended_at',null,'duration_seconds',0,'note',null,
    'created_at',start_millis,'updated_at',start_millis,'version',1,
    'segments',jsonb_build_array(jsonb_build_object('id','74100000-0000-0000-0000-000000000001','sequence_no',1,'started_at',start_millis,'ended_at',null,'duration_seconds',0)),
    'allocations',jsonb_build_array()
  );
  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000003',
    '42000000-0000-0000-0000-000000000008','timer_start','time_entry','74000000-0000-0000-0000-000000000001',
    null,1,1,start_payload
  );
  if timegenie.unassigned_get_state('20000000-0000-0000-0000-000000000002') is not null then
    raise exception 'timer start did not atomically clear the active anchor';
  end if;
  stop_payload:=start_payload||jsonb_build_object(
    'state','ended','ended_at',stop_millis,'duration_seconds',60,'updated_at',stop_millis,'version',2,
    'segments',jsonb_build_array(jsonb_build_object('id','74100000-0000-0000-0000-000000000001','sequence_no',1,'started_at',start_millis,'ended_at',stop_millis,'duration_seconds',60))
  );
  first_stop:=timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000003',
    '42000000-0000-0000-0000-000000000009','timer_stop','time_entry','74000000-0000-0000-0000-000000000001',
    1,2,1,stop_payload
  );
  retry_stop:=timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000003',
    '42000000-0000-0000-0000-000000000009','timer_stop','time_entry','74000000-0000-0000-0000-000000000001',
    1,2,1,stop_payload
  );
  active_anchor:=timegenie.unassigned_get_state('20000000-0000-0000-0000-000000000002');
  if first_stop is distinct from retry_stop
     or active_anchor is null
     or (active_anchor->>'first_started_at')::timestamptz<>(
       select ended_at from timegenie.time_entries
       where id='74000000-0000-0000-0000-000000000001'
     )
     or active_anchor->>'session_id' is null then
    raise exception 'timer stop did not create one idempotent authoritative anchor: %',active_anchor;
  end if;
end $$;

-- Timer transitions create/switch the target and timer images in one remote
-- transaction. Retries reuse the exact result and conflicts leave no partial
-- task or timer behind.
do $$
declare start_millis bigint; switch_millis bigint; start_payload jsonb; invalid_occurrence_payload jsonb; switch_payload jsonb; stale_payload jsonb; first_result jsonb; retry_result jsonb;
begin
  start_millis:=floor(extract(epoch from clock_timestamp())*1000)::bigint;
  switch_millis:=start_millis+60000;
  start_payload:=jsonb_build_object(
    'previous_entry',null,'previous_base_version',null,'task',jsonb_build_object(
      'id','62100000-0000-0000-0000-000000000001','subject_id','52000000-0000-0000-0000-000000000001','parent_id',null,
      'title','原子转换一级事项','status','open','planned_date',timegenie.workspace_work_date('20000000-0000-0000-0000-000000000002',to_timestamp(start_millis::double precision/1000)),
      'planned_time',null,'estimate_minutes',null,'note',null,'project_name',null,'solution_name',null,'source_type','timer_quick_create',
      'source_ref',null,'sort_order',10,'completed_at',null,'created_at',start_millis,'updated_at',start_millis,'version',1,'deleted_at',null
    ),'expected_parent_version',null,'occurrence',jsonb_build_object(
      'task_id','62100000-0000-0000-0000-000000000001',
      'occurrence_date',timegenie.workspace_work_date('20000000-0000-0000-0000-000000000002',to_timestamp(start_millis::double precision/1000)),
      'origin','scheduled','status','open','completed_at',null,'created_at',start_millis,'updated_at',start_millis,'version',1
    ),'active_entry',jsonb_build_object(
      'id','74100000-0000-0000-0000-000000000011','work_date',timegenie.workspace_work_date('20000000-0000-0000-0000-000000000002',to_timestamp(start_millis::double precision/1000)),
      'kind','work','source_type','timer','state','running','default_task_id','62100000-0000-0000-0000-000000000001','label_snapshot','原子转换一级事项',
      'started_at',start_millis,'ended_at',null,'duration_seconds',0,'note',null,'created_at',start_millis,'updated_at',start_millis,'version',1,
      'segments',jsonb_build_array(jsonb_build_object('id','74200000-0000-0000-0000-000000000011','sequence_no',1,'started_at',start_millis,'ended_at',null,'duration_seconds',0)),
      'allocations','[]'::jsonb
    )
  );
  invalid_occurrence_payload:=jsonb_set(
    start_payload,
    '{occurrence,task_id}',
    to_jsonb('62100000-0000-0000-0000-000000000099'::text)
  );
  begin
    perform timegenie.timer_transition_apply(
      '20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000003','42000000-0000-0000-0000-000000000010',invalid_occurrence_payload
    );
    raise exception 'mismatched timer occurrence was accepted';
  exception when others then if sqlerrm not like '%timer occurrence identity mismatch%' then raise; end if; end;
  reset role;
  if exists(select 1 from timegenie.tasks where id='62100000-0000-0000-0000-000000000001')
     or exists(select 1 from timegenie.time_entries where id='74100000-0000-0000-0000-000000000011')
     or exists(select 1 from timegenie.task_occurrences where task_id='62100000-0000-0000-0000-000000000001') then
    raise exception 'invalid timer occurrence left partial remote data';
  end if;
  set role authenticated;
  first_result:=timegenie.timer_transition_apply(
    '20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000003','42000000-0000-0000-0000-000000000011',start_payload
  );
  retry_result:=timegenie.timer_transition_apply(
    '20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000003','42000000-0000-0000-0000-000000000011',start_payload
  );
  reset role;
  if first_result is distinct from retry_result
     or (select count(*) from timegenie.tasks where id='62100000-0000-0000-0000-000000000001')<>1
     or (select count(*) from timegenie.task_occurrences where task_id='62100000-0000-0000-0000-000000000001')<>1
     or first_result->'occurrence'->>'status' is distinct from 'open'
     or (select count(*) from timegenie.time_entries where id='74100000-0000-0000-0000-000000000011' and state='running')<>1 then
    raise exception 'timer transition create/start retry was not idempotent';
  end if;
  set role authenticated;

  switch_payload:=jsonb_build_object(
    'previous_entry',(start_payload->'active_entry')||jsonb_build_object(
      'state','ended','ended_at',switch_millis,'duration_seconds',60,'updated_at',switch_millis,'version',2,
      'segments',jsonb_build_array(jsonb_build_object('id','74200000-0000-0000-0000-000000000011','sequence_no',1,'started_at',start_millis,'ended_at',switch_millis,'duration_seconds',60))
    ),'previous_base_version',1,'task',jsonb_build_object(
      'id','62100000-0000-0000-0000-000000000002','subject_id','52000000-0000-0000-0000-000000000001','parent_id','62100000-0000-0000-0000-000000000001',
      'title','原子转换二级事项','status','open','planned_date',timegenie.workspace_work_date('20000000-0000-0000-0000-000000000002',to_timestamp(switch_millis::double precision/1000)),
      'planned_time',null,'estimate_minutes',null,'note',null,'project_name',null,'solution_name',null,'source_type','timer_quick_create',
      'source_ref',null,'sort_order',10,'completed_at',null,'created_at',switch_millis,'updated_at',switch_millis,'version',1,'deleted_at',null
    ),'expected_parent_version',1,'active_entry',jsonb_build_object(
      'id','74100000-0000-0000-0000-000000000012','work_date',timegenie.workspace_work_date('20000000-0000-0000-0000-000000000002',to_timestamp(switch_millis::double precision/1000)),
      'kind','work','source_type','timer','state','running','default_task_id','62100000-0000-0000-0000-000000000002','label_snapshot','原子转换二级事项',
      'started_at',switch_millis,'ended_at',null,'duration_seconds',0,'note',null,'created_at',switch_millis,'updated_at',switch_millis,'version',1,
      'segments',jsonb_build_array(jsonb_build_object('id','74200000-0000-0000-0000-000000000012','sequence_no',1,'started_at',switch_millis,'ended_at',null,'duration_seconds',0)),
      'allocations','[]'::jsonb
    )
  );
  perform timegenie.timer_transition_apply(
    '20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000003','42000000-0000-0000-0000-000000000012',switch_payload
  );
  reset role;
  if not exists(select 1 from timegenie.time_entries where id='74100000-0000-0000-0000-000000000011' and state='ended' and ended_at=to_timestamp(switch_millis::double precision/1000))
     or not exists(select 1 from timegenie.time_entries where id='74100000-0000-0000-0000-000000000012' and state='running' and started_at=to_timestamp(switch_millis::double precision/1000))
     or (select count(*) from timegenie.time_entries where workspace_id='20000000-0000-0000-0000-000000000002' and state in ('running','paused') and deleted_at is null)<>1 then
    raise exception 'timer transition switch did not preserve one boundary and one active timer';
  end if;
  set role authenticated;

  stale_payload:=jsonb_build_object(
    'previous_entry',(switch_payload->'active_entry')||jsonb_build_object('state','ended','ended_at',switch_millis+60000,'updated_at',switch_millis+60000,'version',2,
      'segments',jsonb_build_array(jsonb_build_object('id','74200000-0000-0000-0000-000000000012','sequence_no',1,'started_at',switch_millis,'ended_at',switch_millis+60000,'duration_seconds',60))),
    'previous_base_version',999,'task',jsonb_build_object(
      'id','62100000-0000-0000-0000-000000000003','subject_id','52000000-0000-0000-0000-000000000001','parent_id',null,'title','冲突时不应创建',
      'status','open','planned_date','2026-10-16','planned_time',null,'estimate_minutes',null,'note',null,'project_name',null,'solution_name',null,
      'source_type','timer_quick_create','source_ref',null,'sort_order',20,'completed_at',null,'created_at',switch_millis+60000,'updated_at',switch_millis+60000,'version',1,'deleted_at',null
    ),'expected_parent_version',null,'active_entry',jsonb_build_object(
      'id','74100000-0000-0000-0000-000000000013','work_date','2026-10-16','kind','work','source_type','timer','state','running',
      'default_task_id','62100000-0000-0000-0000-000000000003','label_snapshot','冲突时不应创建','started_at',switch_millis+60000,'ended_at',null,
      'duration_seconds',0,'note',null,'created_at',switch_millis+60000,'updated_at',switch_millis+60000,'version',1,
      'segments',jsonb_build_array(jsonb_build_object('id','74200000-0000-0000-0000-000000000013','sequence_no',1,'started_at',switch_millis+60000,'ended_at',null,'duration_seconds',0)),
      'allocations','[]'::jsonb
    )
  );
  begin
    perform timegenie.timer_transition_apply(
      '20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000003','42000000-0000-0000-0000-000000000013',stale_payload
    );
    raise exception 'stale timer transition was accepted';
  exception when others then if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if; end;
  reset role;
  if exists(select 1 from timegenie.tasks where id='62100000-0000-0000-0000-000000000003')
     or exists(select 1 from timegenie.time_entries where id='74100000-0000-0000-0000-000000000013')
     or exists(select 1 from timegenie.processed_operations where operation_id='42000000-0000-0000-0000-000000000013') then
    raise exception 'failed timer transition left partial remote data';
  end if;
  set role authenticated;
end $$;

-- Owner isolation and registered-device checks also apply to the shared RPCs.
select set_config('request.jwt.claim.sub', '10000000-0000-0000-0000-000000000001', true);
do $$ begin
  begin
    perform timegenie.unassigned_get_or_create_shared(
      '20000000-0000-0000-0000-000000000002',
      '30000000-0000-0000-0000-000000000003',null,null,null
    );
    raise exception 'another owner read shared unassigned state';
  exception when others then
    if sqlerrm not like '%AUTH_REQUIRED%' then raise; end if;
  end;
end $$;

reset role;

-- Every entity type published by the supported workspace change triggers has
-- a deterministic incremental representation. Child rows are intentionally
-- aggregated to their time-entry or report root.
do $$
declare mapping record;
declare entity jsonb;
begin
  for mapping in
    select * from (values
      ('subjects', '00000000-0000-0000-0000-000000000001', 'subjects'),
      ('work_days', '2099-01-01', 'work_days'),
      ('app_settings', 'missing-setting', 'app_settings'),
      ('tasks', '00000000-0000-0000-0000-000000000002', 'tasks'),
      ('task_status_events', '00000000-0000-0000-0000-000000000003', 'task_status_events'),
      ('task_daily_estimates', '00000000-0000-0000-0000-000000000002|2099-01-01', 'task_daily_estimates'),
      ('task_recurrence_rules', '00000000-0000-0000-0000-000000000002', 'task_recurrence_rules'),
      ('task_occurrences', '00000000-0000-0000-0000-000000000002|2099-01-01', 'task_occurrences'),
      ('unassigned_sessions', '00000000-0000-0000-0000-000000000007', 'unassigned_sessions'),
      ('time_entries', '00000000-0000-0000-0000-000000000004', 'time_entries'),
      ('time_segments', '00000000-0000-0000-0000-000000000004', 'time_entries'),
      ('time_allocations', '00000000-0000-0000-0000-000000000004', 'time_entries'),
      ('report_templates', '00000000-0000-0000-0000-000000000005', 'report_templates'),
      ('reports', '00000000-0000-0000-0000-000000000006', 'reports'),
      ('report_tasks', '00000000-0000-0000-0000-000000000006', 'reports'),
      ('integration_configs', 'missing-provider', 'integration_configs'),
      ('external_bindings', 'missing-provider|task|00000000-0000-0000-0000-000000000002', 'external_bindings')
    ) values_table(source_type, entity_id, result_type)
  loop
    entity := timegenie.cloud_incremental_entity_get(
      '20000000-0000-0000-0000-000000000001',
      mapping.source_type,
      mapping.entity_id
    );
    if entity->>'source_entity_type' <> mapping.source_type
       or entity->>'entity_type' <> mapping.result_type
       or entity->>'entity_id' <> mapping.entity_id
       or not (entity ? 'deleted')
       or not (entity ? 'data') then
      raise exception 'incremental RPC has no applicable representation for %: %', mapping.source_type, entity;
    end if;
  end loop;
end $$;

do $$
begin
  if (
    select count(*)
    from timegenie.unassigned_segments segment
    join timegenie.unassigned_sessions session on session.id = segment.session_id
    where session.workspace_id = '20000000-0000-0000-0000-000000000001'
      and segment.ended_at is null
  ) <> 1 then
    raise exception 'background collector has duplicate open segments';
  end if;
end $$;

do $$
begin
  if not exists(
    select 1
    from pg_publication_tables
    where pubname = 'supabase_realtime'
      and schemaname = 'timegenie'
      and tablename = 'workspace_changes'
  ) then
    raise exception 'workspace_changes is not in the realtime publication';
  end if;
end $$;

-- Actual adjustment batches validate their complete task/adjustment image and
-- commit atomically. Ordinary allocations do not change the adjustment
-- fingerprint, while an adjustment created from the same old snapshot does.
reset role;
insert into timegenie.subjects(id,workspace_id,name,sort_order)
values('51000000-0000-0000-0000-000000000095','20000000-0000-0000-0000-000000000001','实际调整批次',95);
insert into timegenie.tasks(id,workspace_id,subject_id,title,status,source_type,sort_order,version) values
  ('61000000-0000-0000-0000-000000000095','20000000-0000-0000-0000-000000000001','51000000-0000-0000-0000-000000000095','已有叶子','open','manual',10,1),
  ('61000000-0000-0000-0000-000000000096','20000000-0000-0000-0000-000000000001','51000000-0000-0000-0000-000000000095','父事项','open','manual',20,1);
insert into timegenie.time_entries(id,workspace_id,work_date,kind,source_type,state,label_snapshot,started_at,ended_at,duration_seconds,created_at,updated_at,version)
values('71000000-0000-0000-0000-000000000095','20000000-0000-0000-0000-000000000001','2026-10-15','work','manual','ended','已有叶子',to_timestamp(1791993600),to_timestamp(1791997200),3600,to_timestamp(1791993600),to_timestamp(1791997200),1);
insert into timegenie.time_allocations(id,workspace_id,entry_id,task_id,minutes,created_at,updated_at,version)
values('72000000-0000-0000-0000-000000000095','20000000-0000-0000-0000-000000000001','71000000-0000-0000-0000-000000000095','61000000-0000-0000-0000-000000000095',60,to_timestamp(1791997200),to_timestamp(1791997200),1);
set role authenticated;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
do $$
declare empty_fingerprint text; first_result jsonb; retry_result jsonb; payload jsonb;
begin
  empty_fingerprint:='e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855';
  if empty_fingerprint<>'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855' then
    raise exception 'empty adjustment fingerprint test vector changed';
  end if;

  payload:=jsonb_build_object(
    'subjectId','51000000-0000-0000-0000-000000000095','workDate','2026-10-15',
    'clientRequestId','40000000-0000-0000-0000-000000000195','batchId','41000000-0000-0000-0000-000000000195','note','批次核对',
    'items',jsonb_build_array(jsonb_build_object(
      'taskId','61000000-0000-0000-0000-000000000095','expectedTaskVersion',1,
      'baselineMinutes',60,'targetMinutes',75,'deltaMinutes',15,'adjustmentFingerprint',empty_fingerprint
    )),
    'newTasks',jsonb_build_array(
      jsonb_build_object('clientTaskId','draft-positive','title','新子项','parentId','61000000-0000-0000-0000-000000000096','expectedParentVersion',1,'targetMinutes',25),
      jsonb_build_object('clientTaskId','draft-zero','title','仅建事项','parentId',null,'expectedParentVersion',null,'targetMinutes',0)
    ),
    'createdTasks',jsonb_build_array(
      jsonb_build_object('clientTaskId','draft-positive','task',jsonb_build_object(
        'id','61100000-0000-0000-0000-000000000095','subject_id','51000000-0000-0000-0000-000000000095','parent_id','61000000-0000-0000-0000-000000000096','title','新子项','status','open','planned_date','2026-10-15','planned_time',null,'estimate_minutes',null,'note',null,'project_name',null,'solution_name',null,'source_type','manual','source_ref','actual_adjustment_quick_create','sort_order',10,'created_at',1791997200000,'updated_at',1791997200000,'version',1,'deleted_at',null
      )),
      jsonb_build_object('clientTaskId','draft-zero','task',jsonb_build_object(
        'id','61100000-0000-0000-0000-000000000096','subject_id','51000000-0000-0000-0000-000000000095','parent_id',null,'title','仅建事项','status','open','planned_date','2026-10-15','planned_time',null,'estimate_minutes',null,'note',null,'project_name',null,'solution_name',null,'source_type','manual','source_ref','actual_adjustment_quick_create','sort_order',30,'created_at',1791997200000,'updated_at',1791997200000,'version',1,'deleted_at',null
      ))
    ),
    'adjustments',jsonb_build_array(
      jsonb_build_object('id','81000000-0000-0000-0000-000000000095','subject_id','51000000-0000-0000-0000-000000000095','task_id','61000000-0000-0000-0000-000000000095','work_date','2026-10-15','delta_minutes',15,'baseline_minutes',60,'target_minutes',75,'note','批次核对','source_type','quick_actual_adjustment','batch_id','41000000-0000-0000-0000-000000000195','created_at',1791997200000,'updated_at',1791997200000,'version',1,'deleted_at',null),
      jsonb_build_object('id','81000000-0000-0000-0000-000000000096','subject_id','51000000-0000-0000-0000-000000000095','task_id','61100000-0000-0000-0000-000000000095','work_date','2026-10-15','delta_minutes',25,'baseline_minutes',0,'target_minutes',25,'note','批次核对','source_type','quick_actual_adjustment','batch_id','41000000-0000-0000-0000-000000000195','created_at',1791997200000,'updated_at',1791997200000,'version',1,'deleted_at',null)
    )
  );
  first_result:=timegenie.actual_adjustment_batch_apply('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002','40000000-0000-0000-0000-000000000195',payload);
  retry_result:=timegenie.actual_adjustment_batch_apply('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002','40000000-0000-0000-0000-000000000195',payload);
  if first_result is distinct from retry_result or (first_result->>'createdTaskCount')::integer<>2 or (first_result->>'createdAdjustmentCount')::integer<>2 then
    raise exception 'actual adjustment batch retry changed result: % / %',first_result,retry_result;
  end if;
  reset role;
  if (select count(*) from timegenie.tasks where id in('61100000-0000-0000-0000-000000000095','61100000-0000-0000-0000-000000000096'))<>2
     or (select count(*) from timegenie.actual_time_adjustments where batch_id='41000000-0000-0000-0000-000000000195')<>2
     or exists(select 1 from timegenie.actual_time_adjustments where task_id='61100000-0000-0000-0000-000000000096') then
    raise exception 'actual adjustment batch did not create exact task/adjustment set';
  end if;
  set role authenticated;

  -- A normal allocation changes current actual, but not the adjustment-set fingerprint.
  reset role;
  insert into timegenie.time_entries(id,workspace_id,work_date,kind,source_type,state,label_snapshot,started_at,ended_at,duration_seconds,created_at,updated_at,version)
  values('71000000-0000-0000-0000-000000000096','20000000-0000-0000-0000-000000000001','2026-10-15','work','manual','ended','新计时',to_timestamp(1791997200),to_timestamp(1791997800),600,to_timestamp(1791997200),to_timestamp(1791997800),1);
  insert into timegenie.time_allocations(id,workspace_id,entry_id,task_id,minutes,created_at,updated_at,version)
  values('72000000-0000-0000-0000-000000000096','20000000-0000-0000-0000-000000000001','71000000-0000-0000-0000-000000000096','61000000-0000-0000-0000-000000000096',10,to_timestamp(1791997800),to_timestamp(1791997800),1);
  set role authenticated;
  begin
    perform timegenie.actual_adjustment_batch_apply(
      '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002','40000000-0000-0000-0000-000000000196',
      jsonb_build_object('subjectId','51000000-0000-0000-0000-000000000095','workDate','2026-10-15','clientRequestId','40000000-0000-0000-0000-000000000196','batchId','41000000-0000-0000-0000-000000000196','items','[]'::jsonb,
        'newTasks',jsonb_build_array(jsonb_build_object('clientTaskId','stale-parent','title','应整体失败','parentId','61000000-0000-0000-0000-000000000096','expectedParentVersion',1,'targetMinutes',5)),
        'createdTasks',jsonb_build_array(jsonb_build_object('clientTaskId','stale-parent','task',jsonb_build_object('id','61100000-0000-0000-0000-000000000097','subject_id','51000000-0000-0000-0000-000000000095','parent_id','61000000-0000-0000-0000-000000000096','title','应整体失败','status','open','planned_date','2026-10-15','planned_time',null,'estimate_minutes',null,'note',null,'project_name',null,'solution_name',null,'source_type','manual','source_ref','actual_adjustment_quick_create','sort_order',20,'created_at',1791997800000,'updated_at',1791997800000,'version',1,'deleted_at',null))),
        'adjustments',jsonb_build_array(jsonb_build_object('id','81000000-0000-0000-0000-000000000097','subject_id','51000000-0000-0000-0000-000000000095','task_id','61100000-0000-0000-0000-000000000097','work_date','2026-10-15','delta_minutes',5,'baseline_minutes',0,'target_minutes',5,'note',null,'source_type','quick_actual_adjustment','batch_id','41000000-0000-0000-0000-000000000196','created_at',1791997800000,'updated_at',1791997800000,'version',1,'deleted_at',null))
      )
    );
    raise exception 'parent with direct actual accepted new child';
  exception when others then if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if; end;
  reset role;
  if exists(select 1 from timegenie.tasks where id='61100000-0000-0000-0000-000000000097')
     or exists(select 1 from timegenie.actual_time_adjustments where id='81000000-0000-0000-0000-000000000097')
     or exists(select 1 from timegenie.processed_operations where operation_id='40000000-0000-0000-0000-000000000196') then
    raise exception 'failed actual adjustment batch left partial remote data';
  end if;
  set role authenticated;

  -- A stale local baseline still carries the user's target. Rebase its fixed
  -- delta against the current cloud actual instead of creating a conflict.
  first_result:=timegenie.actual_adjustment_batch_apply(
      '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002','40000000-0000-0000-0000-000000000197',
      jsonb_build_object('subjectId','51000000-0000-0000-0000-000000000095','workDate','2026-10-15','clientRequestId','40000000-0000-0000-0000-000000000197','batchId','41000000-0000-0000-0000-000000000197',
        'items',jsonb_build_array(jsonb_build_object('taskId','61000000-0000-0000-0000-000000000095','expectedTaskVersion',1,'baselineMinutes',60,'targetMinutes',55,'deltaMinutes',-5,'adjustmentFingerprint',empty_fingerprint)),
        'newTasks','[]'::jsonb,'createdTasks','[]'::jsonb,
        'adjustments',jsonb_build_array(jsonb_build_object('id','81000000-0000-0000-0000-000000000098','subject_id','51000000-0000-0000-0000-000000000095','task_id','61000000-0000-0000-0000-000000000095','work_date','2026-10-15','delta_minutes',-5,'baseline_minutes',60,'target_minutes',55,'note',null,'source_type','quick_actual_adjustment','batch_id','41000000-0000-0000-0000-000000000197','created_at',1791997900000,'updated_at',1791997900000,'version',1,'deleted_at',null))
      )
    );
  reset role;
  if not exists(select 1 from timegenie.actual_time_adjustments where id='81000000-0000-0000-0000-000000000098' and baseline_minutes=75 and delta_minutes=-20 and target_minutes=55)
     or (select coalesce(sum(a.minutes),0) from timegenie.time_allocations a join timegenie.time_entries e on e.id=a.entry_id where a.task_id='61000000-0000-0000-0000-000000000095' and e.work_date='2026-10-15' and e.deleted_at is null)
        +(select coalesce(sum(delta_minutes),0) from timegenie.actual_time_adjustments where task_id='61000000-0000-0000-0000-000000000095' and work_date='2026-10-15' and deleted_at is null)<>55
     or (first_result->'adjustments'->0->>'delta_minutes')::integer<>-20 then
    raise exception 'stale adjustment target was not rebased: %',first_result;
  end if;
  set role authenticated;

  -- Ordinary time added after the snapshot is folded into the latest target,
  -- so the final actual still equals the user's requested minutes.
  reset role;
  insert into timegenie.time_entries(id,workspace_id,work_date,kind,source_type,state,label_snapshot,started_at,ended_at,duration_seconds,created_at,updated_at,version)
  values('71000000-0000-0000-0000-000000000097','20000000-0000-0000-0000-000000000001','2026-10-16','work','manual','ended','普通计时增长',to_timestamp(1792080000),to_timestamp(1792083600),3600,to_timestamp(1792080000),to_timestamp(1792083600),1);
  insert into timegenie.time_allocations(id,workspace_id,entry_id,task_id,minutes,created_at,updated_at,version)
  values('72000000-0000-0000-0000-000000000097','20000000-0000-0000-0000-000000000001','71000000-0000-0000-0000-000000000097','61000000-0000-0000-0000-000000000095',65,to_timestamp(1792083600),to_timestamp(1792083600),1);
  set role authenticated;
  perform timegenie.actual_adjustment_batch_apply(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002','40000000-0000-0000-0000-000000000198',
    jsonb_build_object('subjectId','51000000-0000-0000-0000-000000000095','workDate','2026-10-16','clientRequestId','40000000-0000-0000-0000-000000000198','batchId','41000000-0000-0000-0000-000000000198',
      'items',jsonb_build_array(jsonb_build_object('taskId','61000000-0000-0000-0000-000000000095','expectedTaskVersion',1,'baselineMinutes',60,'targetMinutes',75,'deltaMinutes',15,'adjustmentFingerprint',empty_fingerprint)),
      'newTasks','[]'::jsonb,'createdTasks','[]'::jsonb,
      'adjustments',jsonb_build_array(jsonb_build_object('id','81000000-0000-0000-0000-000000000099','subject_id','51000000-0000-0000-0000-000000000095','task_id','61000000-0000-0000-0000-000000000095','work_date','2026-10-16','delta_minutes',15,'baseline_minutes',60,'target_minutes',75,'note',null,'source_type','quick_actual_adjustment','batch_id','41000000-0000-0000-0000-000000000198','created_at',1792083600000,'updated_at',1792083600000,'version',1,'deleted_at',null))
    )
  );
  reset role;
  if (select coalesce(sum(a.minutes),0) from timegenie.time_allocations a join timegenie.time_entries e on e.id=a.entry_id where a.task_id='61000000-0000-0000-0000-000000000095' and e.work_date='2026-10-16' and e.deleted_at is null)
       +(select coalesce(sum(delta_minutes),0) from timegenie.actual_time_adjustments where task_id='61000000-0000-0000-0000-000000000095' and work_date='2026-10-16' and deleted_at is null)<>75 then
    raise exception 'ordinary timing growth did not preserve the requested actual target';
  end if;
  update timegenie.actual_time_adjustments set deleted_at=to_timestamp(1792083600.123),updated_at=to_timestamp(1792083600.123),version=2
  where id='81000000-0000-0000-0000-000000000099';
  if timegenie.actual_adjustment_fingerprint('20000000-0000-0000-0000-000000000001','61000000-0000-0000-0000-000000000095','2026-10-16')
     <> encode(extensions.digest('81000000-0000-0000-0000-000000000099:2:1792083600123','sha256'),'hex') then
    raise exception 'PostgreSQL deleted-at fingerprint differs from Rust millisecond canonical format';
  end if;
  set role authenticated;
end $$;
reset role;

-- Semantic operation history is idempotent, sanitized, read-only and isolated.
set local role authenticated;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
do $$
declare list_result jsonb; detail_result jsonb; before_task_count bigint; before_change_count bigint; before_log_count bigint;
begin
  reset role;
  if (select count(*) from timegenie.cloud_operation_logs where workspace_id='20000000-0000-0000-0000-000000000001')=0 then
    raise exception 'successful cloud writes did not produce operation history';
  end if;
  if exists(
    select 1 from timegenie.cloud_operation_log_items item
    where coalesce(item.before_json,'{}'::jsonb)::text||coalesce(item.after_json,'{}'::jsonb)::text
      ~* '(token|password|secret_ref|access_token|refresh_token|service_role|action_config)'
  ) then raise exception 'operation history leaked protected fields'; end if;
  if (select count(*) from timegenie.cloud_operation_logs where workspace_id='20000000-0000-0000-0000-000000000001' and operation_id='40000000-0000-0000-0000-000000000195')<>1 then
    raise exception 'idempotent batch produced duplicate operation history';
  end if;
  if exists(select 1 from timegenie.cloud_operation_logs where operation_id='40000000-0000-0000-0000-000000000196') then
    raise exception 'failed operation was recorded as successful history';
  end if;
  if not exists(select 1 from timegenie.cloud_operation_logs where operation_id='40000000-0000-0000-0000-000000000197') then
    raise exception 'rebased actual adjustment was not recorded as successful history';
  end if;
  before_task_count:=(select count(*) from timegenie.tasks where workspace_id='20000000-0000-0000-0000-000000000001');
  before_change_count:=(select count(*) from timegenie.workspace_changes where workspace_id='20000000-0000-0000-0000-000000000001');
  set role authenticated;
  list_result:=timegenie.cloud_operation_history_list('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002',2,null,null,null,null,null,null,null,null);
  if jsonb_array_length(list_result->'items')=0 then raise exception 'operation history list returned no rows'; end if;
  if pg_get_function_arguments('timegenie.cloud_operation_history_list(uuid,uuid,integer,timestamptz,uuid,timestamptz,timestamptz,uuid,text,text,text)'::regprocedure) not like '%p_limit integer DEFAULT 50%' then raise exception 'operation history RPC default page size is not 50'; end if;
  detail_result:=timegenie.cloud_operation_history_detail('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002',(list_result->'items'->0->>'id')::uuid);
  if jsonb_array_length(detail_result->'items')=0 then raise exception 'operation history detail returned no items'; end if;
  if detail_result->>'operationId' is null
     or detail_result->>'businessType' is null
     or detail_result->>'deviceNameSnapshot' is null
     or detail_result->>'occurredAt' is null then
    raise exception 'operation history detail did not return the camelCase API contract: %',detail_result;
  end if;
  reset role;
  if before_task_count<>(select count(*) from timegenie.tasks where workspace_id='20000000-0000-0000-0000-000000000001')
     or before_change_count<>(select count(*) from timegenie.workspace_changes where workspace_id='20000000-0000-0000-0000-000000000001') then
    raise exception 'operation history browsing changed business or sync data';
  end if;
  begin
    set role authenticated;
    perform timegenie.cloud_operation_history_list('20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000002',30,null,null,null,null,null,null,null,null);
    raise exception 'cross-workspace operation history read succeeded';
  exception when others then if sqlerrm not like '%DEVICE_NOT_REGISTERED%' then raise; end if; end;
  reset role;
  update timegenie.cloud_operation_logs set expires_at=now()-interval '1 second' where workspace_id='20000000-0000-0000-0000-000000000001';
  before_log_count:=(select count(*) from timegenie.cloud_operation_logs where workspace_id='20000000-0000-0000-0000-000000000001');
  set role authenticated;
  perform timegenie.cloud_operation_history_cleanup('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002');
  reset role;
  if before_log_count=0 or exists(select 1 from timegenie.cloud_operation_logs where workspace_id='20000000-0000-0000-0000-000000000001' and expires_at<=now()) then
    raise exception 'expired operation history was not cleaned';
  end if;
  if before_task_count<>(select count(*) from timegenie.tasks where workspace_id='20000000-0000-0000-0000-000000000001') then
    raise exception 'operation history cleanup changed business data';
  end if;
  set role authenticated;
end $$;
reset role;

-- Workspace snapshots are grouped, checksummed, isolated, previewable and retained safely.
set local role authenticated;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
do $$
declare manual_result jsonb; automatic_result jsonb; repeated_result jsonb; preview_result jsonb; delete_result jsonb;
  manual_id uuid; automatic_id uuid; delete_id uuid; corrupt_id uuid; protected_id uuid; failed_id uuid;
  before_task_count bigint; before_change_count bigint; before_log_count bigint; before_expiry timestamptz;
begin
  if not exists(
    select 1 from pg_proc p join pg_namespace n on n.oid=p.pronamespace
    where n.nspname='timegenie' and p.proname='workspace_snapshot_create'
      and 'statement_timeout=120s'=any(coalesce(p.proconfig,array[]::text[]))
  ) or exists(
    select 1 from pg_proc p join pg_namespace n on n.oid=p.pronamespace
    where n.nspname='timegenie'
      and p.proname in('workspace_snapshot_ensure_automatic','workspace_snapshot_ensure_automatic_at','workspace_snapshot_slot_key')
  ) then raise exception 'obsolete sync-triggered automatic snapshot RPC still exists'; end if;
  manual_result:=timegenie.workspace_snapshot_create(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002','40000000-0000-0000-0000-000000000201',
    'manual','集成测试手动镜像','验证四组分块、哈希和预览',false
  );
  manual_id:=(manual_result->>'id')::uuid;
  if manual_result->>'status'<>'ready' or (manual_result->>'chunkCount')::integer<21 then raise exception 'manual snapshot was not published with entity chunks: %',manual_result; end if;
  reset role;
  if (select count(distinct group_name) from timegenie.workspace_snapshot_chunks where snapshot_id=manual_id)<>4 then raise exception 'snapshot did not contain four protected entity groups'; end if;
  if exists(select 1 from timegenie.workspace_snapshot_chunks where snapshot_id=manual_id and content_json::text~*'(secret_ref|access_token|refresh_token|lease_token|action_config_json)') then raise exception 'snapshot chunk leaked protected fields'; end if;
  if (select count(*) from timegenie.workspace_snapshot_chunks where snapshot_id=manual_id)<>(select chunk_count from timegenie.workspace_snapshots where id=manual_id) then raise exception 'snapshot chunk count metadata is inconsistent'; end if;
  set role authenticated;
  reset role;
  before_task_count:=(select count(*) from timegenie.tasks where workspace_id='20000000-0000-0000-0000-000000000001');
  before_change_count:=(select count(*) from timegenie.workspace_changes where workspace_id='20000000-0000-0000-0000-000000000001');
  before_expiry:=(select expires_at from timegenie.workspace_snapshots where id=manual_id);
  set role authenticated;
  preview_result:=timegenie.workspace_snapshot_preview('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002',manual_id);
  if not coalesce((preview_result->>'restorable')::boolean,false)
     or preview_result->'entityCounts' is null
     or preview_result->'groupCounts' is null
     or preview_result->'groups' is not null
     or preview_result->'entities' is not null then
    raise exception 'lightweight snapshot details failed: %',preview_result;
  end if;
  reset role;
  if before_task_count<>(select count(*) from timegenie.tasks where workspace_id='20000000-0000-0000-0000-000000000001')
     or before_change_count<>(select count(*) from timegenie.workspace_changes where workspace_id='20000000-0000-0000-0000-000000000001')
     or before_expiry is distinct from (select expires_at from timegenie.workspace_snapshots where id=manual_id) then raise exception 'snapshot preview changed business data, sync state or retention'; end if;
  set role authenticated;

  begin
    perform timegenie.workspace_snapshot_list('20000000-0000-0000-0000-000000000002','30000000-0000-0000-0000-000000000002');
    raise exception 'cross-workspace snapshot access succeeded';
  exception when others then if sqlerrm not like '%DEVICE_NOT_REGISTERED%' then raise; end if; end;
  begin
    perform timegenie.workspace_snapshot_list('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000098');
    raise exception 'revoked device read snapshots';
  exception when others then if sqlerrm not like '%DEVICE_NOT_REGISTERED%' then raise; end if; end;

  delete_result:=timegenie.workspace_snapshot_create(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002','40000000-0000-0000-0000-000000000202',
    'manual','待删除镜像',null,false
  );
  delete_id:=(delete_result->>'id')::uuid;
  perform timegenie.workspace_snapshot_delete('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002',delete_id);
  reset role;
  if exists(select 1 from timegenie.workspace_snapshot_chunks where snapshot_id=delete_id) then raise exception 'snapshot delete did not cascade to chunks'; end if;
  set role authenticated;

  automatic_result:=timegenie.workspace_snapshot_create(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001','40000000-0000-0000-0000-000000000203',
    'automatic','每日自动镜像',null,true
  );
  automatic_id:=(automatic_result->>'id')::uuid;
  repeated_result:=timegenie.workspace_snapshot_create(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002','40000000-0000-0000-0000-000000000204',
    'automatic','并发自动镜像',null,true
  );
  if (repeated_result->>'id')::uuid<>automatic_id then raise exception 'same-day automatic snapshot was duplicated'; end if;
  reset role;
  if (select count(*) from timegenie.workspace_snapshots where workspace_id='20000000-0000-0000-0000-000000000001' and snapshot_type='automatic' and snapshot_day=current_date and status='ready')<>1 then raise exception 'automatic snapshot uniqueness failed'; end if;
  if (select expires_at-created_at from timegenie.workspace_snapshots where id=automatic_id) not between interval '29 days 23 hours' and interval '30 days 1 hour' then raise exception 'automatic snapshot retention is not 30 days'; end if;
  set role authenticated;

  protected_id:=(timegenie.workspace_snapshot_create(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002','40000000-0000-0000-0000-000000000205',
    'pre_restore','回滚前保护镜像',null,false
  )->>'id')::uuid;
  perform timegenie.workspace_snapshot_set_protected('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002',protected_id,true);
  reset role;
  update timegenie.workspace_snapshots set expires_at=now()-interval '1 second' where id in(automatic_id,protected_id);
  set role authenticated;
  perform timegenie.workspace_snapshot_cleanup('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002');
  reset role;
  if exists(select 1 from timegenie.workspace_snapshots where id=automatic_id) then raise exception 'expired automatic snapshot was not cleaned'; end if;
  if not exists(select 1 from timegenie.workspace_snapshots where id=protected_id and protected and protected_at is not null) then raise exception 'protected snapshot was removed by cleanup'; end if;
  set role authenticated;

  corrupt_id:=(timegenie.workspace_snapshot_create(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002','40000000-0000-0000-0000-000000000206',
    'manual','待篡改镜像',null,false
  )->>'id')::uuid;
  reset role;
  update timegenie.workspace_snapshot_chunks set content_json=content_json||jsonb_build_object('tampered',true) where snapshot_id=corrupt_id and chunk_index=(select min(chunk_index) from timegenie.workspace_snapshot_chunks where snapshot_id=corrupt_id);
  set role authenticated;
  preview_result:=timegenie.workspace_snapshot_preview('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002',corrupt_id);
  if not coalesce((preview_result->>'restorable')::boolean,false) then raise exception 'metadata details unexpectedly inspected chunk payloads'; end if;
  reset role;
  preview_result:=timegenie.workspace_snapshot_verify_integrity('20000000-0000-0000-0000-000000000001',corrupt_id);
  set role authenticated;
  if coalesce((preview_result->>'restorable')::boolean,true) or preview_result->>'errorCode'<>'SNAPSHOT_CORRUPT' then raise exception 'tampered snapshot remained restorable'; end if;
  reset role;
  if (select status from timegenie.workspace_snapshots where id=corrupt_id)<>'corrupt' then raise exception 'tampered snapshot was not marked corrupt'; end if;

  update timegenie.workspace_history_settings set snapshot_max_bytes=1048576,manual_snapshot_limit=2 where workspace_id='20000000-0000-0000-0000-000000000001';
  insert into timegenie.app_settings(workspace_id,key,value_json,updated_at,version) values('20000000-0000-0000-0000-000000000001','snapshot_failure_probe',jsonb_build_object('payload',repeat('x',1100000)),now(),1);
  set role authenticated;
  failed_id:=(timegenie.workspace_snapshot_create(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002','40000000-0000-0000-0000-000000000207',
    'manual','容量失败诊断',null,false
  )->>'id')::uuid;
  reset role;
  if (select status from timegenie.workspace_snapshots where id=failed_id)<>'failed' or exists(select 1 from timegenie.workspace_snapshot_chunks where snapshot_id=failed_id) then raise exception 'failed snapshot exposed a ready partial artifact'; end if;
  delete from timegenie.app_settings where workspace_id='20000000-0000-0000-0000-000000000001' and key='snapshot_failure_probe';
  update timegenie.workspace_history_settings set snapshot_max_bytes=104857600 where workspace_id='20000000-0000-0000-0000-000000000001';
  set role authenticated;
  perform timegenie.workspace_snapshot_create('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002','40000000-0000-0000-0000-000000000208','manual','保留边界一',null,false);
  perform timegenie.workspace_snapshot_create('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002','40000000-0000-0000-0000-000000000209','manual','保留边界二',null,false);
  perform timegenie.workspace_snapshot_create('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002','40000000-0000-0000-0000-000000000210','manual','保留边界三',null,false);
  reset role;
  if (select count(*) from timegenie.workspace_snapshots where workspace_id='20000000-0000-0000-0000-000000000001' and snapshot_type='manual' and status='ready' and not protected)>2 then raise exception 'manual snapshot count retention was not enforced'; end if;
  before_log_count:=(select count(*) from timegenie.cloud_operation_logs where workspace_id='20000000-0000-0000-0000-000000000001' and business_type='workspace_snapshot.create');
  if before_log_count=0 then raise exception 'snapshot creation did not produce operation history'; end if;
  if before_task_count<>(select count(*) from timegenie.tasks where workspace_id='20000000-0000-0000-0000-000000000001') then raise exception 'snapshot retention changed business data'; end if;
  set role authenticated;
end $$;
reset role;

-- Automatic full snapshots run once per workspace-local day at a fixed time.
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
do $$
declare settings_value jsonb; early_result jsonb; due_result jsonb; repeat_result jsonb;
  next_day_result jsonb; before_count bigint; first_id uuid; second_id uuid;
begin
  set role authenticated;
  settings_value:=timegenie.workspace_snapshot_settings_get(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001');
  if not coalesce((settings_value->>'automaticSnapshotEnabled')::boolean,false) then
    raise exception 'automatic snapshots are not enabled by default: %',settings_value;
  end if;
  if settings_value->>'automaticSnapshotTime'<>'03:00'
     or settings_value->>'automaticSnapshotTimezone'<>'Asia/Shanghai'
     or settings_value->>'snapshotRetentionDays'<>'30' then
    raise exception 'automatic snapshot defaults are wrong: %',settings_value;
  end if;
  if coalesce(settings_value->>'schedulerActive','false')<>'false' then
    raise exception 'plain PostgreSQL must report the cron scheduler as inactive: %',settings_value;
  end if;
  if not exists(
    select 1 from pg_proc p join pg_namespace n on n.oid=p.pronamespace
    where n.nspname='timegenie' and p.proname='workspace_snapshot_run_scheduled'
      and 'statement_timeout=600s'=any(coalesce(p.proconfig,array[]::text[]))
  ) then raise exception 'the scheduled snapshot runner must bound its statement timeout'; end if;
  if has_function_privilege('authenticated','timegenie.workspace_snapshot_run_scheduled(timestamp with time zone)','execute')
     or has_function_privilege('anon','timegenie.workspace_snapshot_run_scheduled(timestamp with time zone)','execute') then
    raise exception 'clients must not schedule automatic snapshots';
  end if;
  if to_char((settings_value->>'nextAutomaticSnapshotAt')::timestamptz at time zone 'Asia/Shanghai','HH24:MI')<>'03:00' then
    raise exception 'next automatic snapshot time is wrong: %',settings_value;
  end if;
  settings_value:=timegenie.workspace_snapshot_settings_update(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000211',true,'21:30'::time,30);
  if settings_value->>'automaticSnapshotTime'<>'21:30' then
    raise exception 'automatic snapshot time update failed: %',settings_value;
  end if;
  settings_value:=timegenie.workspace_snapshot_settings_get(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002');
  if settings_value->>'automaticSnapshotTime'<>'21:30' then
    raise exception 'automatic snapshot time was not shared across devices: %',settings_value;
  end if;
  begin
    perform timegenie.workspace_snapshot_settings_update(
      '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
      '40000000-0000-0000-0000-000000000212',true,'21:30'::time,0);
    raise exception 'zero snapshot retention was accepted';
  exception when others then if sqlerrm not like '%VALIDATION_ERROR%' then raise; end if; end;
  reset role;

  before_count:=(select count(*) from timegenie.workspace_snapshots
    where workspace_id='20000000-0000-0000-0000-000000000001' and snapshot_type='automatic');
  early_result:=timegenie.workspace_snapshot_run_scheduled('2026-10-18 12:00:00+00');
  if (select count(*) from timegenie.workspace_snapshots
      where workspace_id='20000000-0000-0000-0000-000000000001' and snapshot_type='automatic')<>before_count then
    raise exception 'scheduled snapshot ran before its workspace-local time: %',early_result;
  end if;
  due_result:=timegenie.workspace_snapshot_run_scheduled('2026-10-18 13:30:00+00');
  if (select count(*) from timegenie.workspace_snapshots
      where workspace_id='20000000-0000-0000-0000-000000000001' and snapshot_type='automatic')<>before_count+1 then
    raise exception 'scheduled snapshot did not create exactly one recovery point: %',due_result;
  end if;
  select id into first_id from timegenie.workspace_snapshots
  where workspace_id='20000000-0000-0000-0000-000000000001' and snapshot_type='automatic'
    and schedule_slot_key='daily:2026-10-18';
  if first_id is null or (select status from timegenie.workspace_snapshots where id=first_id)<>'ready' then
    raise exception 'scheduled snapshot was not a ready recovery point: %',due_result;
  end if;
  if (select expires_at-created_at from timegenie.workspace_snapshots where id=first_id)
     not between interval '29 days 23 hours' and interval '30 days 1 hour' then
    raise exception 'scheduled snapshot retention is not 30 days';
  end if;
  repeat_result:=timegenie.workspace_snapshot_run_scheduled('2026-10-18 15:59:00+00');
  if (select count(*) from timegenie.workspace_snapshots
      where workspace_id='20000000-0000-0000-0000-000000000001' and snapshot_type='automatic')<>before_count+1 then
    raise exception 'the same workspace-local day created a second recovery point: %',repeat_result;
  end if;
  next_day_result:=timegenie.workspace_snapshot_run_scheduled('2026-10-19 13:30:00+00');
  select id into second_id from timegenie.workspace_snapshots
  where workspace_id='20000000-0000-0000-0000-000000000001' and snapshot_type='automatic'
    and schedule_slot_key='daily:2026-10-19';
  if second_id is null or second_id=first_id then
    raise exception 'the next workspace-local day did not create a new recovery point: %',next_day_result;
  end if;
  if (select count(*) from timegenie.workspace_snapshots
      where workspace_id='20000000-0000-0000-0000-000000000001' and snapshot_type='automatic')<>before_count+2 then
    raise exception 'scheduled snapshots were duplicated';
  end if;

  set role authenticated;
  settings_value:=timegenie.workspace_snapshot_settings_update(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000215',false,'21:30'::time,30);
  reset role;
  if coalesce((settings_value->>'automaticSnapshotEnabled')::boolean,true) then
    raise exception 'automatic snapshots could not be disabled: %',settings_value;
  end if;
  early_result:=timegenie.workspace_snapshot_run_scheduled('2026-10-20 13:30:00+00');
  if (select count(*) from timegenie.workspace_snapshots
      where workspace_id='20000000-0000-0000-0000-000000000001' and snapshot_type='automatic')<>before_count+2 then
    raise exception 'disabled automatic snapshots still created: %',early_result;
  end if;
  if not exists(select 1 from timegenie.workspace_snapshots where id=first_id) then
    raise exception 'settings update removed an existing snapshot';
  end if;
end $$;
reset role;

-- Snapshot retention is shared across automatic and manual recovery points.
reset role;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
do $$
declare settings_value jsonb; manual_result jsonb; manual_id uuid; expired_id uuid; protected_id uuid;
  before_task_count bigint; before_change_count bigint;
begin
  before_task_count:=(select count(*) from timegenie.tasks where workspace_id='20000000-0000-0000-0000-000000000001');
  before_change_count:=(select count(*) from timegenie.workspace_changes where workspace_id='20000000-0000-0000-0000-000000000001');
  set role authenticated;
  settings_value:=timegenie.workspace_snapshot_settings_update(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000220',true,'03:00'::time,7
  );
  if settings_value->>'snapshotRetentionDays'<>'7' then raise exception 'snapshot retention update was not returned: %',settings_value; end if;
  settings_value:=timegenie.workspace_snapshot_settings_get('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002');
  if settings_value->>'snapshotRetentionDays'<>'7' then raise exception 'snapshot retention was not shared across devices: %',settings_value; end if;
  if settings_value->>'automaticSnapshotTime'<>'03:00' then raise exception 'retention update reset the automatic snapshot time: %',settings_value; end if;
  begin
    perform timegenie.workspace_snapshot_settings_update(
      '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
      '40000000-0000-0000-0000-000000000222',true,'03:00'::time,0
    );
    raise exception 'zero snapshot retention was accepted';
  exception when others then
    if sqlerrm not like '%VALIDATION_ERROR%' then raise; end if;
  end;
  begin
    perform timegenie.workspace_snapshot_settings_update(
      '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
      '40000000-0000-0000-0000-000000000223',true,'03:00'::time,366
    );
    raise exception 'snapshot retention above 365 days was accepted';
  exception when others then if sqlerrm not like '%VALIDATION_ERROR%' then raise; end if; end;
  manual_result:=timegenie.workspace_snapshot_create(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002',
    '40000000-0000-0000-0000-000000000224','manual','保留策略手动快照',null,false
  );
  manual_id:=(manual_result->>'id')::uuid;
  reset role;
  if (select expires_at-created_at from timegenie.workspace_snapshots where id=manual_id) not between interval '6 days 23 hours' and interval '7 days 1 hour' then raise exception 'manual snapshot did not use the shared retention window'; end if;
  insert into timegenie.workspace_snapshots(
    workspace_id,snapshot_type,name,status,created_by_device_name,protected,created_at
  ) values(
    '20000000-0000-0000-0000-000000000001','manual','过期未保护快照','ready','integration-test',false,now()-interval '8 days'
  ) returning id into expired_id;
  if (select expires_at from timegenie.workspace_snapshots where id=expired_id)>now() then raise exception 'expired snapshot did not receive an elapsed retention timestamp'; end if;
  set role authenticated;
  perform timegenie.workspace_snapshot_cleanup('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002');
  reset role;
  if exists(select 1 from timegenie.workspace_snapshots where id=expired_id) then raise exception 'unprotected snapshot past retention was not cleaned'; end if;
  protected_id:=(timegenie.workspace_snapshot_create(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002',
    '40000000-0000-0000-0000-000000000225','pre_restore','保护快照保留测试',null,false
  )->>'id')::uuid;
  set role authenticated;
  perform timegenie.workspace_snapshot_set_protected('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002',protected_id,true);
  reset role;
  if (select expires_at from timegenie.workspace_snapshots where id=protected_id) is not null then raise exception 'protected snapshot kept an automatic expiry'; end if;
  update timegenie.workspace_snapshots set created_at=now()-interval '30 days' where id=protected_id;
  set role authenticated;
  perform timegenie.workspace_snapshot_cleanup('20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000002');
  reset role;
  if not exists(select 1 from timegenie.workspace_snapshots where id=protected_id and protected) then raise exception 'protected snapshot was removed by retention cleanup'; end if;
  if before_task_count<>(select count(*) from timegenie.tasks where workspace_id='20000000-0000-0000-0000-000000000001')
     or before_change_count<>(select count(*) from timegenie.workspace_changes where workspace_id='20000000-0000-0000-0000-000000000001') then raise exception 'snapshot retention changed business or sync data'; end if;
  set role authenticated;
end $$;
reset role;

-- The old timer-write takeover RPC remains callable for compatibility but
-- cannot change a live background lease.
update timegenie.tracking_leases set holder_device_id=null,lease_token=null,expires_at=null
where workspace_id='20000000-0000-0000-0000-000000000001';
set local role authenticated;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
do $$
declare first_lease jsonb; blocked_lease jsonb; takeover_lease jsonb;
begin
  first_lease:=timegenie.tracking_lease_acquire(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '49000000-0000-0000-0000-000000000091'
  );
  blocked_lease:=timegenie.tracking_lease_acquire(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000002',
    '49000000-0000-0000-0000-000000000092'
  );
  if coalesce((blocked_lease->>'acquired')::boolean,false) then
    raise exception 'ordinary background lease acquisition preempted a live holder';
  end if;
  takeover_lease:=timegenie.tracking_lease_takeover_for_timer_write(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000002',
    '49000000-0000-0000-0000-000000000093'
  );
  if not coalesce((takeover_lease->>'acquired')::boolean,false)
     or coalesce((takeover_lease->>'takeover')::boolean,true)
     or not coalesce((takeover_lease->>'compatibilityOnly')::boolean,false)
     or takeover_lease->>'holderDeviceId'<>'30000000-0000-0000-0000-000000000001' then
    raise exception 'legacy timer write takeover was not a non-mutating compatibility response';
  end if;
  perform timegenie.tracking_lease_renew(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '49000000-0000-0000-0000-000000000091'
  );
  perform timegenie.tracking_lease_release(
    '20000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000001',
    '49000000-0000-0000-0000-000000000091'
  );
end $$;
reset role;

-- Timer stop accepts writes without a lease. A future client timestamp is
-- clamped to server receipt time, while an offline stop in the past is kept.
set local role authenticated;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
do $$
declare observed_at timestamptz:=clock_timestamp(); observed_ms bigint; start_ms bigint; future_payload jsonb; offline_payload jsonb; result_value jsonb; saved_end timestamptz;
begin
  observed_ms:=floor(extract(epoch from observed_at)*1000)::bigint;
  start_ms:=observed_ms-60000;
  future_payload:=jsonb_build_object(
    'id','70000000-0000-0000-0000-000000000196','work_date',timegenie.workspace_work_date('20000000-0000-0000-0000-000000000001',observed_at),
    'kind','work','source_type','timer','state','ended','default_task_id','60000000-0000-0000-0000-000000000002',
    'label_snapshot','未来时钟收敛','started_at',start_ms,'ended_at',observed_ms+300000,'duration_seconds',360,
    'created_at',start_ms,'updated_at',observed_ms+300000,'version',1,'segments',jsonb_build_array(jsonb_build_object(
      'id','73000000-0000-0000-0000-000000000196','sequence_no',1,'started_at',start_ms,'ended_at',observed_ms+300000,'duration_seconds',360
    )),'allocations','[]'::jsonb
  );
  result_value:=timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000196','timer_stop','time_entry','70000000-0000-0000-0000-000000000196',null,1,1,future_payload
  );
  if not coalesce((result_value->>'accepted')::boolean,false) then raise exception 'timer stop without a lease was rejected: %',result_value; end if;
  select ended_at into saved_end from timegenie.time_entries where id='70000000-0000-0000-0000-000000000196';
  if saved_end>clock_timestamp()+interval '1 second' then raise exception 'future timer stop was not clamped: %',saved_end; end if;

  offline_payload:=future_payload||jsonb_build_object(
    'id','70000000-0000-0000-0000-000000000197','label_snapshot','离线结束保留',
    'ended_at',observed_ms-30000,'updated_at',observed_ms-30000,'duration_seconds',30,
    'segments',jsonb_build_array(jsonb_build_object('id','73000000-0000-0000-0000-000000000197','sequence_no',1,'started_at',start_ms,'ended_at',observed_ms-30000,'duration_seconds',30))
  );
  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '40000000-0000-0000-0000-000000000197','timer_stop','time_entry','70000000-0000-0000-0000-000000000197',null,1,1,offline_payload
  );
  if abs(extract(epoch from ((select ended_at from timegenie.time_entries where id='70000000-0000-0000-0000-000000000197')-to_timestamp((observed_ms-30000)::double precision/1000))))>0.001 then
    raise exception 'offline timer stop boundary was changed';
  end if;
end $$;
reset role;

-- Workspace epoch 1 accepts legacy clients, but epoch > 1 requires an exact
-- request header for every business write and returns reset-only pull data.
set local role authenticated;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
select set_config('request.headers','{"x-timegenie-client-schema-generation":"1"}',true);
do $$
declare legacy_snapshot jsonb;
begin
  legacy_snapshot:=timegenie.cloud_snapshot_get('20000000-0000-0000-0000-000000000001');
  if (legacy_snapshot->>'workspace_epoch')::bigint<>1 then
    raise exception 'epoch 1 legacy snapshot did not report the server epoch: %',legacy_snapshot;
  end if;
end $$;
select set_config('request.headers','{"x-timegenie-workspace-epoch":"1","x-timegenie-client-schema-generation":"1"}',true);
select timegenie.cloud_apply_patch(
  '20000000-0000-0000-0000-000000000001',
  '30000000-0000-0000-0000-000000000001',
  '4e000000-0000-0000-0000-000000000001',
  'app_setting.update','app_setting','workspace_epoch_probe',null,
  jsonb_build_object('key','workspace_epoch_probe','value_json','1'::jsonb,'updated_at',1792272000000,'version',1)
);
reset role;
update timegenie.workspaces set workspace_epoch=2 where id='20000000-0000-0000-0000-000000000001';
set local role authenticated;
select set_config('request.headers','{"x-timegenie-client-schema-generation":"1"}',true);
do $$
begin
  begin
    perform timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
      '4e000000-0000-0000-0000-000000000002','app_setting.update','app_setting','workspace_epoch_probe',1,
      jsonb_build_object('key','workspace_epoch_probe','value_json','2'::jsonb,'updated_at',1792272001000,'version',2)
    );
    raise exception 'epoch > 1 accepted a missing header';
  exception when others then
    if sqlerrm not like '%WORKSPACE_EPOCH_STALE%' then raise; end if;
  end;
end $$;
select set_config('request.headers','{"x-timegenie-workspace-epoch":"1","x-timegenie-client-schema-generation":"1"}',true);
do $$
declare retry integer;
begin
  for retry in 1..2 loop
    begin
      perform timegenie.cloud_apply_patch(
        '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
        '4e000000-0000-0000-0000-000000000003','app_setting.update','app_setting','workspace_epoch_probe',1,
        jsonb_build_object('key','workspace_epoch_probe','value_json','3'::jsonb,'updated_at',1792272002000,'version',2)
      );
      raise exception 'old epoch retry % was accepted',retry;
    exception when others then
      if sqlerrm not like '%WORKSPACE_EPOCH_STALE%' then raise; end if;
    end;
  end loop;
end $$;
select set_config('request.headers','{"x-timegenie-workspace-epoch":"2","x-timegenie-client-schema-generation":"1"}',true);
select timegenie.cloud_apply_patch(
  '20000000-0000-0000-0000-000000000001',
  '30000000-0000-0000-0000-000000000001',
  '4e000000-0000-0000-0000-000000000004',
  'app_setting.update','app_setting','workspace_epoch_probe',1,
  jsonb_build_object('key','workspace_epoch_probe','value_json','4'::jsonb,'updated_at',1792272003000,'version',2)
);
do $$
declare snapshot jsonb; mismatch jsonb;
begin
  snapshot:=timegenie.cloud_snapshot_get('20000000-0000-0000-0000-000000000001');
  if (snapshot->>'workspace_epoch')::bigint<>2 then
    raise exception 'snapshot omitted the current workspace epoch: %',snapshot;
  end if;
  perform set_config('request.headers','{"x-timegenie-workspace-epoch":"1","x-timegenie-client-schema-generation":"1"}',true);
  mismatch:=timegenie.cloud_incremental_pull(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',0,100
  );
  if not coalesce((mismatch->>'reset_required')::boolean,false)
     or (mismatch->>'workspace_epoch')::bigint<>2
     or jsonb_array_length(mismatch->'changes')<>0
     or jsonb_array_length(mismatch->'entities')<>0 then
    raise exception 'epoch mismatch pull was not reset-only: %',mismatch;
  end if;
end $$;
reset role;

-- A restore lock excludes ordinary writes, creates a safety snapshot, restores
-- atomically in place, increments epoch, emits reset, and is idempotent.
select set_config('request.headers','{"x-timegenie-workspace-epoch":"2","x-timegenie-client-schema-generation":"1"}',true);
update timegenie.time_entries set state='ended',ended_at=coalesce(ended_at,updated_at),duration_seconds=greatest(duration_seconds,0)
where workspace_id='20000000-0000-0000-0000-000000000001' and state in('running','paused');
update timegenie.time_segments set ended_at=coalesce(ended_at,started_at),duration_seconds=greatest(duration_seconds,0)
where workspace_id='20000000-0000-0000-0000-000000000001' and ended_at is null;
update timegenie.unassigned_sessions set state='resolved',resolution_type='discard',resolved_at=coalesce(resolved_at,updated_at)
where workspace_id='20000000-0000-0000-0000-000000000001' and state in('collecting','paused','awaiting_resolution');
set local role authenticated;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
do $$
declare target_id uuid; prepared jsonb; direct_prepared jsonb; restored jsonb; repeated jsonb; before_task_count bigint; restored_snapshot jsonb; active_unassigned_id uuid; pre_restore_count bigint;
begin
  target_id:=(timegenie.workspace_snapshot_create(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '4f000000-0000-0000-0000-000000000001','manual','回滚协议目标',null,false
  )->>'id')::uuid;
  before_task_count:=jsonb_array_length(timegenie.cloud_snapshot_get('20000000-0000-0000-0000-000000000001')->'tasks');
  reset role;
  select id into active_unassigned_id from timegenie.unassigned_sessions
  where workspace_id='20000000-0000-0000-0000-000000000001' and state in('collecting','paused','awaiting_resolution') limit 1;
  if active_unassigned_id is null then
    insert into timegenie.unassigned_sessions(
      workspace_id,work_date,state,threshold_seconds,duration_seconds,first_started_at,shared_source,migration_state
    ) values(
      '20000000-0000-0000-0000-000000000001',current_date,'collecting',300,0,now(),'cloud','adopted'
    ) returning id into active_unassigned_id;
  end if;
  set local role authenticated;
  perform set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
  perform timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '4f000000-0000-0000-0000-000000000002','app_setting.update','app_setting','workspace_epoch_probe',2,
    jsonb_build_object('key','workspace_epoch_probe','value_json','99'::jsonb,'updated_at',1792272100000,'version',3)
  );
  reset role;
  select count(*) into pre_restore_count from timegenie.workspace_snapshots
  where workspace_id='20000000-0000-0000-0000-000000000001' and snapshot_type='pre_restore';
  set local role authenticated;
  perform set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
  direct_prepared:=timegenie.workspace_restore_prepare(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',target_id,2,false
  );
  if direct_prepared->>'safetySnapshotId' is not null then raise exception 'direct restore unexpectedly created a safety snapshot: %',direct_prepared; end if;
  reset role;
  if (select count(*) from timegenie.workspace_snapshots where workspace_id='20000000-0000-0000-0000-000000000001' and snapshot_type='pre_restore')<>pre_restore_count then raise exception 'direct restore changed safety snapshot count'; end if;
  set local role authenticated;
  perform set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
  perform timegenie.workspace_restore_cancel(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',(direct_prepared->>'lockToken')::uuid
  );
  prepared:=timegenie.workspace_restore_prepare(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',target_id,2,true
  );
  if prepared->>'safetySnapshotId' is null or prepared->>'lockToken' is null then raise exception 'restore prepare omitted safety or lock metadata: %',prepared; end if;
  begin
    perform timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
      '4f000000-0000-0000-0000-000000000003','app_setting.update','app_setting','restore_lock_probe',null,
      jsonb_build_object('key','restore_lock_probe','value_json','true'::jsonb,'updated_at',1792272101000,'version',1)
    );
    raise exception 'ordinary write bypassed restore lock';
  exception when others then
    if sqlerrm not like '%WORKSPACE_RESTORE_IN_PROGRESS%' then raise; end if;
  end;
  perform set_config('request.headers','{"x-timegenie-workspace-epoch":"2","x-timegenie-client-schema-generation":"1"}',true);
  restored:=timegenie.workspace_restore_commit(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    (prepared->>'lockToken')::uuid,target_id,prepared->>'targetContentHash',(prepared->>'preparedEpoch')::bigint,
    '4f000000-0000-0000-0000-000000000004',true
  );
  repeated:=timegenie.workspace_restore_commit(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    (prepared->>'lockToken')::uuid,target_id,prepared->>'targetContentHash',(prepared->>'preparedEpoch')::bigint,
    '4f000000-0000-0000-0000-000000000004',true
  );
  if restored is distinct from repeated or (restored->>'workspaceEpoch')::bigint<>3 then raise exception 'restore commit was not idempotent or did not advance epoch: % / %',restored,repeated; end if;
  perform set_config('request.headers','{"x-timegenie-workspace-epoch":"3","x-timegenie-client-schema-generation":"1"}',true);
  restored_snapshot:=timegenie.cloud_snapshot_get('20000000-0000-0000-0000-000000000001');
  if not exists(select 1 from jsonb_array_elements(restored_snapshot->'app_settings') value where value->>'key'='workspace_epoch_probe' and value->'value_json'='4'::jsonb) then raise exception 'restore did not recover target business value'; end if;
  if before_task_count<>jsonb_array_length(restored_snapshot->'tasks') then raise exception 'restore changed target task count'; end if;
  if exists(select 1 from jsonb_array_elements(restored_snapshot->'unassigned_sessions') value where value->>'state'='paused') then raise exception 'restore wrote invalid paused unassigned state'; end if;
  if exists(select 1 from jsonb_array_elements(restored_snapshot->'unassigned_sessions') value where value->>'state'='collecting') then raise exception 'restore resumed an active unassigned collection'; end if;
  perform set_config('timegenie.restore_test_safety_id',restored->>'safetySnapshotId',true);
end $$;
reset role;

insert into auth.users(id) values('10000000-0000-0000-0000-000000000004')
on conflict(id) do nothing;
insert into timegenie.workspaces(id,owner_user_id,name) values(
  '20000000-0000-0000-0000-00000000000b',
  '10000000-0000-0000-0000-000000000004',
  '直连计时验收工作空间'
) on conflict(id) do nothing;
set local role authenticated;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000004',true);
select set_config('request.jwt.claim.session_id','80000000-0000-0000-0000-00000000000b',true);
select set_config('request.headers','{"x-timegenie-client-schema-generation":"1"}',true);
select timegenie.workspace_initialize_defaults('20000000-0000-0000-0000-00000000000b');
select timegenie.device_authorize(
  '20000000-0000-0000-0000-00000000000b','30000000-0000-0000-0000-00000000000b',
  '直连计时验收设备','windows','0.4.6'
);
select timegenie.upgrade_storage_protocol(
  '20000000-0000-0000-0000-00000000000b','30000000-0000-0000-0000-00000000000b',
  '4e000000-0000-0000-0000-0000000000b0',1,'queue_drained',false
);

do $$
declare
  subject_id uuid;
  task_a uuid:='61000000-0000-0000-0000-0000000000c1';
  task_b uuid:='61000000-0000-0000-0000-0000000000c2';
  task_c uuid:='61000000-0000-0000-0000-0000000000c4';
  parent_id uuid:='61000000-0000-0000-0000-0000000000c3';
  started jsonb; start_retry jsonb; paused jsonb; paused_retry jsonb;
  resumed jsonb; resumed_retry jsonb; transitioned jsonb; transition_retry jsonb;
  no_op_transition jsonb; stopped jsonb; stopped_retry jsonb;
  allocated jsonb; allocated_retry jsonb; entry_id uuid; active_version bigint;
  stopped_version bigint; task_b_version bigint; allocation_count integer; allocation_minutes integer;
  task_a_status text; task_b_status text; entry_version bigint;
  bootstrap jsonb; work_date_value date; state_value jsonb; task_list_value jsonb;
  running jsonb; running_version bigint; task_a_version bigint;
  transition_entry_id uuid; transition_expected_version bigint; allocation_entry_id uuid; allocation_expected_version bigint;
  timer_lease uuid:='4e000000-0000-0000-0000-0000000000f0';
begin
  bootstrap:=timegenie.direct_cloud_workbench_bootstrap(
    '20000000-0000-0000-0000-00000000000b','30000000-0000-0000-0000-00000000000b',1
  );
  subject_id:=(bootstrap->'data'->'subjects'->0->>'id')::uuid;
  work_date_value:=(bootstrap->'data'->>'currentWorkDate')::date;
  if subject_id is null then raise exception 'direct timer fixture has no subject'; end if;

  perform timegenie.direct_cloud_task_create(
    '20000000-0000-0000-0000-00000000000b','30000000-0000-0000-0000-00000000000b',
    '4e000000-0000-0000-0000-0000000000c1',1,
    jsonb_build_object('id',task_a,'subjectId',subject_id,'title','直连计时事项 A')
  );
  perform timegenie.direct_cloud_task_create(
    '20000000-0000-0000-0000-00000000000b','30000000-0000-0000-0000-00000000000b',
    '4e000000-0000-0000-0000-0000000000c2',1,
    jsonb_build_object('id',task_b,'subjectId',subject_id,'title','直连计时事项 B')
  );
  perform timegenie.direct_cloud_task_create(
    '20000000-0000-0000-0000-00000000000b','30000000-0000-0000-0000-00000000000b',
    '4e000000-0000-0000-0000-0000000000c3',1,
    jsonb_build_object('id',parent_id,'subjectId',subject_id,'title','直连计时父事项')
  );
  perform timegenie.direct_cloud_task_create(
    '20000000-0000-0000-0000-00000000000b','30000000-0000-0000-0000-00000000000b',
    '4e000000-0000-0000-0000-0000000000c4',1,
    jsonb_build_object('id',task_c,'subjectId',subject_id,'title','直连计时事项 C')
  );

  started:=timegenie.direct_cloud_timer_start(
    '20000000-0000-0000-0000-00000000000b','30000000-0000-0000-0000-00000000000b',
    '4e000000-0000-0000-0000-0000000000d1',1,
    jsonb_build_object('taskId',task_a,'note','开始验收','leaseToken',timer_lease)
  );
  start_retry:=timegenie.direct_cloud_timer_start(
    '20000000-0000-0000-0000-00000000000b','30000000-0000-0000-0000-00000000000b',
    '4e000000-0000-0000-0000-0000000000d1',1,
    jsonb_build_object('taskId',task_a,'note','开始验收','leaseToken',timer_lease)
  );
  if started is distinct from start_retry
     or started->'data'->'entry'->>'state' is distinct from 'running'
     or started->'data'->'entry'->>'defaultTaskId' is distinct from task_a::text then
    raise exception 'direct timer start is not idempotent or authoritative: % / %',started,start_retry;
  end if;
  entry_id:=(started->'data'->'entry'->>'id')::uuid;
  active_version:=(started->'data'->'entry'->>'version')::bigint;

  paused:=timegenie.direct_cloud_timer_pause(
    '20000000-0000-0000-0000-00000000000b','30000000-0000-0000-0000-00000000000b',
    '4e000000-0000-0000-0000-0000000000d2',1,
    jsonb_build_object('entryId',entry_id,'expectedVersion',active_version,'leaseToken',timer_lease)
  );
  paused_retry:=timegenie.direct_cloud_timer_pause(
    '20000000-0000-0000-0000-00000000000b','30000000-0000-0000-0000-00000000000b',
    '4e000000-0000-0000-0000-0000000000d2',1,
    jsonb_build_object('entryId',entry_id,'expectedVersion',active_version,'leaseToken',timer_lease)
  );
  if paused is distinct from paused_retry
     or paused->'data'->'entry'->>'state' is distinct from 'paused' then
    raise exception 'direct timer pause is not idempotent: % / %',paused,paused_retry;
  end if;
  active_version:=(paused->'data'->'entry'->>'version')::bigint;

  resumed:=timegenie.direct_cloud_timer_resume(
    '20000000-0000-0000-0000-00000000000b','30000000-0000-0000-0000-00000000000b',
    '4e000000-0000-0000-0000-0000000000d3',1,
    jsonb_build_object('entryId',entry_id,'expectedVersion',active_version,'leaseToken',timer_lease)
  );
  resumed_retry:=timegenie.direct_cloud_timer_resume(
    '20000000-0000-0000-0000-00000000000b','30000000-0000-0000-0000-00000000000b',
    '4e000000-0000-0000-0000-0000000000d3',1,
    jsonb_build_object('entryId',entry_id,'expectedVersion',active_version,'leaseToken',timer_lease)
  );
  if resumed is distinct from resumed_retry
     or resumed->'data'->'entry'->>'state' is distinct from 'running' then
    raise exception 'direct timer resume is not idempotent: % / %',resumed,resumed_retry;
  end if;
  active_version:=(resumed->'data'->'entry'->>'version')::bigint;

  transition_entry_id:=entry_id;
  transition_expected_version:=active_version;
  transitioned:=timegenie.direct_cloud_timer_transition(
    '20000000-0000-0000-0000-00000000000b','30000000-0000-0000-0000-00000000000b',
    '4e000000-0000-0000-0000-0000000000d4',1,
    jsonb_build_object(
      'currentEntryId',entry_id,'currentEntryExpectedVersion',active_version,
      'taskId',task_b,'note','切换目标','leaseToken',timer_lease
    )
  );
  transition_retry:=timegenie.direct_cloud_timer_transition(
    '20000000-0000-0000-0000-00000000000b','30000000-0000-0000-0000-00000000000b',
    '4e000000-0000-0000-0000-0000000000d4',1,
    jsonb_build_object(
      'currentEntryId',entry_id,'currentEntryExpectedVersion',active_version,
      'taskId',task_b,'note','切换目标','leaseToken',timer_lease
    )
  );
  if transitioned is distinct from transition_retry
     or transitioned->'data'->>'noOp' is distinct from 'false'
     or transitioned->'data'->'previousStopResult'->'currentEntry'->>'id' is distinct from entry_id::text
     or transitioned->'data'->'previousStopResult'->'currentEntry'->>'state' is distinct from 'ended'
     or transitioned->'data'->'activeEntry'->>'defaultTaskId' is distinct from task_b::text
     or transitioned->'data'->'activeEntry'->>'state' is distinct from 'running' then
    raise exception 'direct timer transition is not atomic or idempotent: % / %',transitioned,transition_retry;
  end if;
  entry_id:=(transitioned->'data'->'activeEntry'->>'id')::uuid;
  active_version:=(transitioned->'data'->'activeEntry'->>'version')::bigint;

  no_op_transition:=timegenie.direct_cloud_timer_transition(
    '20000000-0000-0000-0000-00000000000b','30000000-0000-0000-0000-00000000000b',
    '4e000000-0000-0000-0000-0000000000d5',1,
    jsonb_build_object(
      'currentEntryId',entry_id,'currentEntryExpectedVersion',active_version,
      'taskId',task_b,'leaseToken',timer_lease
    )
  );
  if no_op_transition->'data'->>'noOp' is distinct from 'true'
     or no_op_transition->'data'->'activeEntry'->>'id' is distinct from entry_id::text then
    raise exception 'direct timer same-target transition must be a no-op: %',no_op_transition;
  end if;

  stopped:=timegenie.direct_cloud_timer_stop(
    '20000000-0000-0000-0000-00000000000b','30000000-0000-0000-0000-00000000000b',
    '4e000000-0000-0000-0000-0000000000d6',1,
    jsonb_build_object(
      'entryId',entry_id,'expectedVersion',active_version,
      'createDefaultAllocation',false,'leaseToken',timer_lease
    )
  );
  stopped_retry:=timegenie.direct_cloud_timer_stop(
    '20000000-0000-0000-0000-00000000000b','30000000-0000-0000-0000-00000000000b',
    '4e000000-0000-0000-0000-0000000000d6',1,
    jsonb_build_object(
      'entryId',entry_id,'expectedVersion',active_version,
      'createDefaultAllocation',false,'leaseToken',timer_lease
    )
  );
  if stopped is distinct from stopped_retry
     or stopped->'data'->'currentEntry'->>'state' is distinct from 'ended' then
    raise exception 'direct timer stop is not idempotent: % / %',stopped,stopped_retry;
  end if;
  stopped_version:=(stopped->'data'->'currentEntry'->>'version')::bigint;
  task_b_version:=(timegenie.direct_cloud_task_detail(
    '20000000-0000-0000-0000-00000000000b','30000000-0000-0000-0000-00000000000b',task_b,1
  )->'data'->'task'->>'version')::bigint;

  allocation_entry_id:=entry_id;
  allocation_expected_version:=stopped_version;
  allocated:=timegenie.direct_cloud_time_allocation_replace(
    '20000000-0000-0000-0000-00000000000b','30000000-0000-0000-0000-00000000000b',
    '4e000000-0000-0000-0000-0000000000d7',1,
    jsonb_build_object(
      'entryId',entry_id,'expectedVersion',stopped_version,'allocations',jsonb_build_array(
        jsonb_build_object('taskId',task_b,'minutes',2,'completeTask',true,'taskExpectedVersion',task_b_version),
        jsonb_build_object('taskId',task_b,'minutes',3,'completeTask',false,'taskExpectedVersion',task_b_version)
      )
    )
  );
  allocated_retry:=timegenie.direct_cloud_time_allocation_replace(
    '20000000-0000-0000-0000-00000000000b','30000000-0000-0000-0000-00000000000b',
    '4e000000-0000-0000-0000-0000000000d7',1,
    jsonb_build_object(
      'entryId',entry_id,'expectedVersion',stopped_version,'allocations',jsonb_build_array(
        jsonb_build_object('taskId',task_b,'minutes',2,'completeTask',true,'taskExpectedVersion',task_b_version),
        jsonb_build_object('taskId',task_b,'minutes',3,'completeTask',false,'taskExpectedVersion',task_b_version)
      )
    )
  );
  allocation_count:=jsonb_array_length(allocated->'data'->'entry'->'allocations');
  allocation_minutes:=coalesce((allocated->'data'->'entry'->>'allocatedMinutes')::integer,0);
  task_b_status:=timegenie.direct_cloud_task_detail(
    '20000000-0000-0000-0000-00000000000b','30000000-0000-0000-0000-00000000000b',task_b,1
  )->'data'->'task'->>'status';
  entry_version:=(allocated->'data'->'entry'->>'version')::bigint;
  if allocated is distinct from allocated_retry or allocation_count<>1 or allocation_minutes<>5
     or (allocated->'data'->'entry'->'allocations'->0->>'minutes')::integer<>5
     or task_b_status is distinct from 'done' then
    raise exception 'direct allocation did not merge duplicates and complete the task atomically: % / %',allocated,allocation_count;
  end if;

  begin
    task_a_version:=(timegenie.direct_cloud_task_detail(
      '20000000-0000-0000-0000-00000000000b','30000000-0000-0000-0000-00000000000b',task_a,1
    )->'data'->'task'->>'version')::bigint;
    perform timegenie.direct_cloud_time_allocation_replace(
      '20000000-0000-0000-0000-00000000000b','30000000-0000-0000-0000-00000000000b',
      '4e000000-0000-0000-0000-0000000000d8',1,
      jsonb_build_object(
        'entryId',entry_id,'expectedVersion',entry_version,'allocations',jsonb_build_array(
          jsonb_build_object('taskId',task_a,'minutes',1,'completeTask',true,'taskExpectedVersion',task_a_version),
          jsonb_build_object('taskId',task_c,'minutes',1,'completeTask',true,'taskExpectedVersion',99)
        )
      )
    );
    raise exception 'direct allocation accepted a stale task version';
  exception when others then
    if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if;
  end;
  state_value:=timegenie.direct_cloud_time_state(
    '20000000-0000-0000-0000-00000000000b','30000000-0000-0000-0000-00000000000b',work_date_value,1
  );
  select value into state_value from jsonb_array_elements(state_value->'data'->'entries') value
  where value->>'id'=entry_id::text;
  task_a_status:=timegenie.direct_cloud_task_detail(
    '20000000-0000-0000-0000-00000000000b','30000000-0000-0000-0000-00000000000b',task_a,1
  )->'data'->'task'->>'status';
  if jsonb_array_length(state_value->'allocations')<>1 or task_a_status is distinct from 'open'
     or (state_value->>'version')::bigint<>entry_version then
    raise exception 'failed direct allocation left partial state';
  end if;

  running:=timegenie.direct_cloud_timer_start(
    '20000000-0000-0000-0000-00000000000b','30000000-0000-0000-0000-00000000000b',
    '4e000000-0000-0000-0000-0000000000d9',1,
    jsonb_build_object('taskId',task_a,'note','回滚验收计时','leaseToken',timer_lease)
  );
  entry_id:=(running->'data'->'entry'->>'id')::uuid;
  running_version:=(running->'data'->'entry'->>'version')::bigint;
  begin
    perform timegenie.direct_cloud_timer_transition(
      '20000000-0000-0000-0000-00000000000b','30000000-0000-0000-0000-00000000000b',
      '4e000000-0000-0000-0000-0000000000da',1,
      jsonb_build_object(
        'currentEntryId',entry_id,'currentEntryExpectedVersion',running_version,
        'leaseToken',timer_lease,
        'newTask',jsonb_build_object(
          'subjectId',subject_id,'parentId',parent_id,'expectedParentVersion',99,
          'title','不应创建的快速事项'
        )
      )
    );
    raise exception 'direct transition accepted a stale parent version';
  exception when others then
    if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if;
  end;
  state_value:=timegenie.direct_cloud_time_state(
    '20000000-0000-0000-0000-00000000000b','30000000-0000-0000-0000-00000000000b',work_date_value,1
  );
  task_list_value:=timegenie.direct_cloud_task_list(
    '20000000-0000-0000-0000-00000000000b','30000000-0000-0000-0000-00000000000b',1,
    null,true,'不应创建的快速事项'
  );
  if jsonb_array_length(task_list_value->'data'->'tasks')<>0
     or state_value->'data'->'activeEntry'->>'id' is distinct from entry_id::text
     or state_value->'data'->'activeEntry'->>'state' is distinct from 'running'
     or (state_value->'data'->'activeEntry'->>'version')::bigint<>running_version then
    raise exception 'failed direct transition left a partial task or stopped the timer';
  end if;

  perform timegenie.direct_cloud_timer_stop(
    '20000000-0000-0000-0000-00000000000b','30000000-0000-0000-0000-00000000000b',
    '4e000000-0000-0000-0000-0000000000db',1,
    jsonb_build_object(
      'entryId',entry_id,'expectedVersion',running_version,
      'createDefaultAllocation',false,'leaseToken',timer_lease
    )
  );

  if (timegenie.direct_cloud_operation_get(
        '20000000-0000-0000-0000-00000000000b','30000000-0000-0000-0000-00000000000b',
        '4e000000-0000-0000-0000-0000000000d4','direct_timer_transition',
        jsonb_build_object(
          'expectedEpoch',1,
          'payload',jsonb_build_object(
            'currentEntryId',transition_entry_id,
            'currentEntryExpectedVersion',transition_expected_version,
            'taskId',task_b,'note','切换目标'
          )
        )
      )->>'found')<>'true'
     or (timegenie.direct_cloud_operation_get(
        '20000000-0000-0000-0000-00000000000b','30000000-0000-0000-0000-00000000000b',
        '4e000000-0000-0000-0000-0000000000d7','direct_time_allocation_replace',
        jsonb_build_object(
          'expectedEpoch',1,
          'payload',jsonb_build_object(
            'entryId',allocation_entry_id,'expectedVersion',allocation_expected_version,'allocations',jsonb_build_array(
              jsonb_build_object('taskId',task_b,'minutes',2,'completeTask',true,'taskExpectedVersion',task_b_version),
              jsonb_build_object('taskId',task_b,'minutes',3,'completeTask',false,'taskExpectedVersion',task_b_version)
            )
          )
        )
      )->>'found')<>'true' then
    raise exception 'repeated direct timer operation created more than one authoritative result';
  end if;
end $$;
reset role;

insert into auth.users(id) values('10000000-0000-0000-0000-000000000005')
on conflict(id) do nothing;
insert into timegenie.workspaces(id,owner_user_id,name) values(
  '20000000-0000-0000-0000-00000000000c',
  '10000000-0000-0000-0000-000000000005',
  '直连计时恢复验收工作空间'
) on conflict(id) do nothing;
set local role authenticated;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000005',true);
select set_config('request.jwt.claim.session_id','80000000-0000-0000-0000-00000000000c',true);
select set_config('request.headers','{"x-timegenie-client-schema-generation":"1"}',true);
select timegenie.workspace_initialize_defaults('20000000-0000-0000-0000-00000000000c');
select timegenie.device_authorize(
  '20000000-0000-0000-0000-00000000000c','30000000-0000-0000-0000-0000000000c1',
  '直连恢复设备 A','windows','0.4.6'
);
select timegenie.device_authorize(
  '20000000-0000-0000-0000-00000000000c','30000000-0000-0000-0000-0000000000c2',
  '直连恢复设备 B','windows','0.4.6'
);
select timegenie.upgrade_storage_protocol(
  '20000000-0000-0000-0000-00000000000c','30000000-0000-0000-0000-0000000000c1',
  '4e000000-0000-0000-0000-0000000000e0',1,'queue_drained',false
);
reset role;

update timegenie.app_settings set value_json='"UTC"'::jsonb
where workspace_id='20000000-0000-0000-0000-00000000000c' and key='timezone';

insert into timegenie.time_entries(
  id,workspace_id,work_date,kind,source_type,state,label_snapshot,started_at,
  duration_seconds,created_at,updated_at,version,created_by_device_id,
  updated_by_device_id,timer_chain_id,last_continuous_at
) values(
  '62000000-0000-0000-0000-0000000000e1',
  '20000000-0000-0000-0000-00000000000c',
  (now() at time zone 'UTC')::date,'work','timer','running','连续性读取测试',
  now()-interval '2 hours',0,now()-interval '2 hours',now()-interval '2 hours',
  7,'30000000-0000-0000-0000-0000000000c1',
  '30000000-0000-0000-0000-0000000000c1',
  '62000000-0000-0000-0000-0000000000e1',now()-interval '2 minutes'
);
insert into timegenie.time_segments(
  id,workspace_id,entry_id,sequence_no,started_at,duration_seconds
) values(
  '63000000-0000-0000-0000-0000000000e1',
  '20000000-0000-0000-0000-00000000000c',
  '62000000-0000-0000-0000-0000000000e1',1,now()-interval '2 hours',0
);

set local role authenticated;
do $$
declare
  state_value jsonb; recovery jsonb; recovery_retry jsonb; heartbeat jsonb;
  lease_value jsonb; entry_version bigint; segment_count integer;
  last_anchor timestamptz; observed_anchor bigint;
begin
  state_value:=timegenie.direct_cloud_time_state(
    '20000000-0000-0000-0000-00000000000c','30000000-0000-0000-0000-0000000000c1',
    (now() at time zone 'UTC')::date,1
  );
  if (state_value->'data'->'activeEntry'->>'version')::bigint<>7
     or state_value->'data'->'activeEntry'->>'state' is distinct from 'running'
     or jsonb_array_length(state_value->'data'->'activeEntry'->'segments')<>1 then
    raise exception 'direct time state read mutated a timer: %',state_value;
  end if;
  select version,last_continuous_at into entry_version,last_anchor
  from timegenie.time_entries where id='62000000-0000-0000-0000-0000000000e1';
  if entry_version<>7 or last_anchor is distinct from now()-interval '2 minutes' then
    raise exception 'direct time state read changed timer persistence';
  end if;

  recovery:=timegenie.direct_cloud_timer_recover(
    '20000000-0000-0000-0000-00000000000c','30000000-0000-0000-0000-0000000000c1',
    '4e000000-0000-0000-0000-0000000000e1',1,
    jsonb_build_object(
      'entryId','62000000-0000-0000-0000-0000000000e1','expectedVersion',7,
      'leaseToken','4e000000-0000-0000-0000-0000000000f1'
    )
  );
  recovery_retry:=timegenie.direct_cloud_timer_recover(
    '20000000-0000-0000-0000-00000000000c','30000000-0000-0000-0000-0000000000c1',
    '4e000000-0000-0000-0000-0000000000e1',1,
    jsonb_build_object(
      'entryId','62000000-0000-0000-0000-0000000000e1','expectedVersion',7,
      'leaseToken','4e000000-0000-0000-0000-0000000000f1'
    )
  );
  segment_count:=jsonb_array_length(recovery->'data'->'activeEntry'->'segments');
  if recovery is distinct from recovery_retry
     or recovery->'data'->>'continuityInterrupted' is distinct from 'true'
     or recovery->'data'->>'calendarRollover' is distinct from 'false'
     or (recovery->'data'->'activeEntry'->>'version')::bigint<>8
     or segment_count<>2 then
    raise exception 'direct continuity recovery did not split the interrupted segment: % / %',recovery,recovery_retry;
  end if;
  observed_anchor:=(recovery->'data'->'activeEntry'->'segments'->1->>'startedAt')::bigint;
  if observed_anchor<>((extract(epoch from now())*1000)::bigint) then
    raise exception 'direct continuity recovery did not use server time for the new segment';
  end if;

  lease_value:=timegenie.direct_cloud_tracking_acquire(
    '20000000-0000-0000-0000-00000000000c','30000000-0000-0000-0000-0000000000c1',1,
    '4e000000-0000-0000-0000-0000000000f1'
  );
  if lease_value->>'acquired' is distinct from 'true'
     or (lease_value->'activeEntry'->>'version')::bigint<>8 then
    raise exception 'direct tracking acquire changed the timer or failed: %',lease_value;
  end if;
  heartbeat:=timegenie.direct_cloud_tracking_heartbeat(
    '20000000-0000-0000-0000-00000000000c','30000000-0000-0000-0000-0000000000c1',1,
    '4e000000-0000-0000-0000-0000000000f1'
  );
  state_value:=timegenie.direct_cloud_time_state(
    '20000000-0000-0000-0000-00000000000c','30000000-0000-0000-0000-0000000000c1',
    (now() at time zone 'UTC')::date,1
  );
  if heartbeat->>'renewed' is distinct from 'true'
     or (state_value->'data'->'activeEntry'->>'version')::bigint<>8
     or jsonb_array_length(state_value->'data'->'activeEntry'->'segments')<>2 then
    raise exception 'direct lease heartbeat mutated a timer: % / %',heartbeat,state_value;
  end if;
  select version,last_continuous_at into entry_version,last_anchor
  from timegenie.time_entries where id='62000000-0000-0000-0000-0000000000e1';
  if entry_version<>8 or last_anchor is distinct from now() then
    raise exception 'direct lease heartbeat changed timer continuity fields';
  end if;

  recovery:=timegenie.direct_cloud_timer_recover(
    '20000000-0000-0000-0000-00000000000c','30000000-0000-0000-0000-0000000000c1',
    '4e000000-0000-0000-0000-0000000000e2',1,
    jsonb_build_object(
      'entryId','62000000-0000-0000-0000-0000000000e1','expectedVersion',8,
      'leaseToken','4e000000-0000-0000-0000-0000000000f1'
    )
  );
  if recovery->'data'->>'noOp' is distinct from 'true'
     or recovery->'data'->>'recovered' is distinct from 'false'
     or (recovery->'data'->'activeEntry'->>'version')::bigint<>8
     or jsonb_array_length(recovery->'data'->'activeEntry'->'segments')<>2 then
    raise exception 'repeated direct continuity recovery created duplicate segments: %',recovery;
  end if;
end $$;
reset role;

delete from timegenie.time_entries where id='62000000-0000-0000-0000-0000000000e1';

insert into timegenie.time_entries(
  id,workspace_id,work_date,kind,source_type,state,label_snapshot,started_at,
  duration_seconds,created_at,updated_at,version,created_by_device_id,
  updated_by_device_id,timer_chain_id,last_continuous_at
) values(
  '62000000-0000-0000-0000-0000000000e2',
  '20000000-0000-0000-0000-00000000000c',
  (now() at time zone 'UTC')::date,'work','timer','running','接管验收计时',
  now()-interval '1 hour',0,now()-interval '1 hour',now()-interval '1 hour',
  3,'30000000-0000-0000-0000-0000000000c1',
  '30000000-0000-0000-0000-0000000000c1',
  '62000000-0000-0000-0000-0000000000e2',now()-interval '2 minutes'
);
insert into timegenie.time_segments(
  id,workspace_id,entry_id,sequence_no,started_at,duration_seconds
) values(
  '63000000-0000-0000-0000-0000000000e2',
  '20000000-0000-0000-0000-00000000000c',
  '62000000-0000-0000-0000-0000000000e2',1,now()-interval '1 hour',0
);

set local role authenticated;
do $$
declare state_value jsonb;
begin
  begin
    perform timegenie.direct_cloud_timer_takeover(
      '20000000-0000-0000-0000-00000000000c','30000000-0000-0000-0000-0000000000c2',
      '4e000000-0000-0000-0000-0000000000e3',1,
      jsonb_build_object(
        'entryId','62000000-0000-0000-0000-0000000000e2','expectedVersion',3,
        'leaseToken','4e000000-0000-0000-0000-0000000000f2'
      )
    );
    raise exception 'direct timer takeover ignored an active lease';
  exception when others then
    if sqlerrm not like '%LEASE_LOST%' then raise; end if;
  end;
  state_value:=timegenie.direct_cloud_time_state(
    '20000000-0000-0000-0000-00000000000c','30000000-0000-0000-0000-0000000000c2',
    (now() at time zone 'UTC')::date,1
  );
  if (state_value->'data'->'activeEntry'->>'version')::bigint<>3
     or jsonb_array_length(state_value->'data'->'activeEntry'->'segments')<>1 then
    raise exception 'rejected direct timer takeover changed the timer';
  end if;
end $$;
reset role;

update timegenie.tracking_leases set expires_at=now()-interval '1 second'
where workspace_id='20000000-0000-0000-0000-00000000000c';

set local role authenticated;
do $$
declare takeover jsonb; takeover_retry jsonb; heartbeat jsonb; state_value jsonb;
begin
  takeover:=timegenie.direct_cloud_timer_takeover(
    '20000000-0000-0000-0000-00000000000c','30000000-0000-0000-0000-0000000000c2',
    '4e000000-0000-0000-0000-0000000000e3',1,
    jsonb_build_object(
      'entryId','62000000-0000-0000-0000-0000000000e2','expectedVersion',3,
      'leaseToken','4e000000-0000-0000-0000-0000000000f2'
    )
  );
  takeover_retry:=timegenie.direct_cloud_timer_takeover(
    '20000000-0000-0000-0000-00000000000c','30000000-0000-0000-0000-0000000000c2',
    '4e000000-0000-0000-0000-0000000000e3',1,
    jsonb_build_object(
      'entryId','62000000-0000-0000-0000-0000000000e2','expectedVersion',3,
      'leaseToken','4e000000-0000-0000-0000-0000000000f2'
    )
  );
  if takeover is distinct from takeover_retry
     or takeover->'data'->'lease'->>'holderDeviceId' is distinct from '30000000-0000-0000-0000-0000000000c2'
     or takeover->'data'->'lease'->>'tookOverFrom' is distinct from '30000000-0000-0000-0000-0000000000c1'
     or takeover->'data'->>'continuityInterrupted' is distinct from 'true'
     or (takeover->'data'->'activeEntry'->>'version')::bigint<>4 then
    raise exception 'direct timer takeover did not adopt an expired lease: % / %',takeover,takeover_retry;
  end if;
  heartbeat:=timegenie.direct_cloud_tracking_heartbeat(
    '20000000-0000-0000-0000-00000000000c','30000000-0000-0000-0000-0000000000c2',1,
    '4e000000-0000-0000-0000-0000000000f2'
  );
  state_value:=timegenie.direct_cloud_time_state(
    '20000000-0000-0000-0000-00000000000c','30000000-0000-0000-0000-0000000000c2',
    (now() at time zone 'UTC')::date,1
  );
  if heartbeat->>'renewed' is distinct from 'true'
     or (state_value->'data'->'activeEntry'->>'version')::bigint<>4
     or jsonb_array_length(state_value->'data'->'activeEntry'->'segments')<>2 then
    raise exception 'direct takeover lost the authoritative timer';
  end if;
  begin
    perform timegenie.direct_cloud_tracking_heartbeat(
      '20000000-0000-0000-0000-00000000000c','30000000-0000-0000-0000-0000000000c1',1,
      '4e000000-0000-0000-0000-0000000000f1'
    );
    raise exception 'direct lease heartbeat accepted a replaced holder';
  exception when others then
    if sqlerrm not like '%LEASE_LOST%' then raise; end if;
  end;
  begin
    perform timegenie.direct_cloud_timer_takeover(
      '20000000-0000-0000-0000-00000000000c','30000000-0000-0000-0000-0000000000c2',
      '4e000000-0000-0000-0000-0000000000e4',1,
      jsonb_build_object(
        'entryId','62000000-0000-0000-0000-0000000000e2','expectedVersion',3,
        'leaseToken','4e000000-0000-0000-0000-0000000000f3'
      )
    );
    raise exception 'direct timer takeover accepted a stale version';
  exception when others then
    if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if;
  end;
end $$;
reset role;

delete from timegenie.time_entries where id='62000000-0000-0000-0000-0000000000e2';
update timegenie.tracking_leases set holder_device_id=null,lease_token=null,expires_at=null
where workspace_id='20000000-0000-0000-0000-00000000000c';

insert into timegenie.time_entries(
  id,workspace_id,work_date,kind,source_type,state,label_snapshot,started_at,
  duration_seconds,created_at,updated_at,version,created_by_device_id,
  updated_by_device_id,timer_chain_id,last_continuous_at
) values(
  '62000000-0000-0000-0000-0000000000e3',
  '20000000-0000-0000-0000-00000000000c',
  (now() at time zone 'UTC')::date-2,'work','timer','running','跨日验收计时',
  ((now() at time zone 'UTC')::date-2)::timestamp at time zone 'UTC'+interval '22 hours',
  0,now(),now(),1,'30000000-0000-0000-0000-0000000000c1',
  '30000000-0000-0000-0000-0000000000c1',
  '62000000-0000-0000-0000-0000000000e3',
  now()-interval '1 second'
);
insert into timegenie.time_segments(
  id,workspace_id,entry_id,sequence_no,started_at,duration_seconds
) values(
  '63000000-0000-0000-0000-0000000000e3',
  '20000000-0000-0000-0000-00000000000c',
  '62000000-0000-0000-0000-0000000000e3',1,
  ((now() at time zone 'UTC')::date-2)::timestamp at time zone 'UTC'+interval '22 hours',0
);

set local role authenticated;
do $$
declare rollover jsonb; rollover_retry jsonb; slices_value jsonb; active_entry jsonb;
begin
  rollover:=timegenie.direct_cloud_timer_recover(
    '20000000-0000-0000-0000-00000000000c','30000000-0000-0000-0000-0000000000c1',
    '4e000000-0000-0000-0000-0000000000e5',1,
    jsonb_build_object(
      'entryId','62000000-0000-0000-0000-0000000000e3','expectedVersion',1,
      'leaseToken','4e000000-0000-0000-0000-0000000000f3'
    )
  );
  rollover_retry:=timegenie.direct_cloud_timer_recover(
    '20000000-0000-0000-0000-00000000000c','30000000-0000-0000-0000-0000000000c1',
    '4e000000-0000-0000-0000-0000000000e5',1,
    jsonb_build_object(
      'entryId','62000000-0000-0000-0000-0000000000e3','expectedVersion',1,
      'leaseToken','4e000000-0000-0000-0000-0000000000f3'
    )
  );
  slices_value:=rollover->'data'->'slices';
  active_entry:=rollover->'data'->'activeEntry';
  if rollover is distinct from rollover_retry
     or rollover->'data'->>'calendarRollover' is distinct from 'true'
     or rollover->'data'->>'continuityInterrupted' is distinct from 'false'
     or jsonb_array_length(slices_value)<>3 then
    raise exception 'direct calendar rollover did not split across days: % / %',rollover,rollover_retry;
  end if;
  if active_entry->>'workDate' is distinct from ((now() at time zone 'UTC')::date)::text
     or active_entry->>'state' is distinct from 'running'
     or (slices_value->0->>'durationSeconds')::bigint<>7200
     or slices_value->1->>'state' is distinct from 'ended'
     or (slices_value->1->>'durationSeconds')::bigint<>86400
     or slices_value->2->>'state' is distinct from 'running' then
    raise exception 'direct calendar rollover durations are invalid: %',rollover;
  end if;
  rollover:=timegenie.direct_cloud_timer_recover(
    '20000000-0000-0000-0000-00000000000c','30000000-0000-0000-0000-0000000000c1',
    '4e000000-0000-0000-0000-0000000000e6',1,
    jsonb_build_object(
      'entryId',active_entry->>'id','expectedVersion',1,
      'leaseToken','4e000000-0000-0000-0000-0000000000f3'
    )
  );
  if rollover->'data'->>'noOp' is distinct from 'true'
     or jsonb_array_length(rollover->'data'->'slices')<>3 then
    raise exception 'direct calendar rollover repeated and created duplicate slices: %',rollover;
  end if;
end $$;

do $$
declare
  workspace_id uuid:='20000000-0000-0000-0000-00000000000c';
  device_a uuid:='30000000-0000-0000-0000-0000000000c1';
  device_b uuid:='30000000-0000-0000-0000-0000000000c2';
  active_before jsonb; active_after jsonb; holder_pause jsonb;
  blocked_detail text; blocked_version bigint;
  b_operation uuid:='4e000000-0000-0000-0000-0000000000e7';
  a_operation uuid:='4e000000-0000-0000-0000-0000000000e8';
  b_operation_again uuid:='4e000000-0000-0000-0000-0000000000e9';
begin
  active_before:=timegenie.direct_cloud_active_timer(workspace_id,device_a,1)
    ->'data'->'activeEntry';
  if active_before is null or active_before->>'state' is distinct from 'running' then
    raise exception 'direct lease contention fixture has no running timer: %',active_before;
  end if;
  begin
    perform timegenie.direct_cloud_timer_pause(
      workspace_id,device_b,b_operation,1,
      jsonb_build_object(
        'entryId',active_before->>'id',
        'expectedVersion',(active_before->>'version')::bigint,
        'leaseToken','4e000000-0000-0000-0000-0000000000f4'
      )
    );
    raise exception 'non-holder direct timer write succeeded';
  exception when others then
    if sqlerrm not like '%LEASE_LOST%' then raise; end if;
    get stacked diagnostics blocked_detail=pg_exception_detail;
  end;
  if blocked_detail is null
     or position(active_before->>'id' in blocked_detail)=0
     or position('LEASE_LOST' in blocked_detail)=0 then
    raise exception 'LEASE_LOST did not return the authoritative active timer: %',blocked_detail;
  end if;
  if (timegenie.direct_cloud_operation_get(
        workspace_id,device_b,b_operation,'direct_timer_pause',
        jsonb_build_object(
          'expectedEpoch',1,
          'payload',jsonb_build_object(
            'entryId',active_before->>'id',
            'expectedVersion',(active_before->>'version')::bigint
          )
        )
      )->>'found')<>'false' then
    raise exception 'rejected non-holder timer write left an operation record';
  end if;
  active_after:=timegenie.direct_cloud_active_timer(workspace_id,device_b,1)
    ->'data'->'activeEntry';
  if active_after->>'id' is distinct from active_before->>'id'
     or (active_after->>'version')::bigint<>(active_before->>'version')::bigint
     or active_after->>'state' is distinct from active_before->>'state' then
    raise exception 'rejected non-holder timer write changed the active timer: % / %',
      active_before,active_after;
  end if;

  holder_pause:=timegenie.direct_cloud_timer_pause(
    workspace_id,device_a,a_operation,1,
    jsonb_build_object(
      'entryId',active_before->>'id',
      'expectedVersion',(active_before->>'version')::bigint,
      'leaseToken','4e000000-0000-0000-0000-0000000000f3'
    )
  );
  if holder_pause->'data'->'entry'->>'state' is distinct from 'paused' then
    raise exception 'direct lease holder could not pause the shared timer: %',holder_pause;
  end if;
  blocked_version:=(holder_pause->'data'->'entry'->>'version')::bigint;
  begin
    perform timegenie.direct_cloud_timer_resume(
      workspace_id,device_b,b_operation_again,1,
      jsonb_build_object(
        'entryId',active_before->>'id',
        'expectedVersion',blocked_version,
        'leaseToken','4e000000-0000-0000-0000-0000000000f4'
      )
    );
    raise exception 'non-holder direct timer write succeeded after holder update';
  exception when others then
    if sqlerrm not like '%LEASE_LOST%' then raise; end if;
  end;
  if (timegenie.direct_cloud_active_timer(workspace_id,device_b,1)
      ->'data'->'activeEntry'->>'state') is distinct from 'paused' then
    raise exception 'failed non-holder timer write changed the holder state';
  end if;
end $$;
reset role;

-- Direct unassigned time keeps one shared anchor across devices, rolls old
-- anchors at the workspace calendar boundary, and classifies work/break/
-- discard time atomically with stable operation IDs.
reset role;
insert into auth.users(id) values('10000000-0000-0000-0000-000000000006')
on conflict(id) do nothing;
insert into timegenie.workspaces(id,owner_user_id,name) values(
  '20000000-0000-0000-0000-00000000000d',
  '10000000-0000-0000-0000-000000000006',
  '直连未归属验收工作空间'
) on conflict(id) do nothing;
set local role authenticated;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000006',true);
select set_config('request.jwt.claim.session_id','80000000-0000-0000-0000-00000000000d',true);
select set_config('request.headers','{"x-timegenie-client-schema-generation":"1"}',true);
select timegenie.workspace_initialize_defaults('20000000-0000-0000-0000-00000000000d');
select timegenie.device_authorize(
  '20000000-0000-0000-0000-00000000000d','30000000-0000-0000-0000-0000000000d1',
  '直连未归属设备 A','windows','0.4.6'
);
select timegenie.device_authorize(
  '20000000-0000-0000-0000-00000000000d','30000000-0000-0000-0000-0000000000d2',
  '直连未归属设备 B','windows','0.4.6'
);
select timegenie.upgrade_storage_protocol(
  '20000000-0000-0000-0000-00000000000d','30000000-0000-0000-0000-0000000000d1',
  '4e000000-0000-0000-0000-000000000100',1,'queue_drained',false
);
reset role;

update timegenie.app_settings set value_json='"UTC"'::jsonb
where workspace_id='20000000-0000-0000-0000-00000000000d' and key='timezone';
update timegenie.app_settings set value_json='30'::jsonb
where workspace_id='20000000-0000-0000-0000-00000000000d'
  and key='unassigned_prompt_seconds';

do $$
declare
  ws_id uuid:='20000000-0000-0000-0000-00000000000d';
  device_a uuid:='30000000-0000-0000-0000-0000000000d1';
  device_b uuid:='30000000-0000-0000-0000-0000000000d2';
  first_result jsonb; second_result jsonb; work_result jsonb; work_retry jsonb;
  break_anchor jsonb; break_result jsonb; discard_anchor jsonb; discard_result jsonb;
  task_result jsonb; current_state jsonb; stale_state jsonb;
  subject_id uuid; task_id uuid:='61000000-0000-0000-0000-0000000000d1';
  current_session_id uuid; session_version bigint; yesterday_id uuid;
  today_date date:=(now() at time zone 'UTC')::date;
  anchor_count integer; generated_count integer; processed_count integer;
  yesterday_seconds bigint;
begin
  subject_id:=(timegenie.direct_cloud_workbench_bootstrap(
    ws_id,device_a,1
  )->'data'->'subjects'->0->>'id')::uuid;
  task_result:=timegenie.direct_cloud_task_create(
    ws_id,device_a,'4e000000-0000-0000-0000-000000000101',1,
    jsonb_build_object('id',task_id,'subjectId',subject_id,'title','未归属直连分配事项')
  );

  first_result:=timegenie.direct_cloud_unassigned_ensure(
    ws_id,device_a,'4e000000-0000-0000-0000-000000000102',1,'{}'::jsonb
  );
  second_result:=timegenie.direct_cloud_unassigned_ensure(
    ws_id,device_b,'4e000000-0000-0000-0000-000000000103',1,'{}'::jsonb
  );
  if first_result->'data'->'anchor'->>'sessionId' is distinct from
       second_result->'data'->'anchor'->>'sessionId'
     or first_result->'data'->>'created' is distinct from 'true'
     or second_result->'data'->>'created' is distinct from 'false' then
    raise exception 'direct devices did not converge on one unassigned anchor: % / %',
      first_result,second_result;
  end if;
  select count(*) into anchor_count from timegenie.unassigned_sessions
  where workspace_id=ws_id and work_date=today_date
    and state in ('collecting','awaiting_resolution');
  if anchor_count<>1 then raise exception 'direct unassigned created % active anchors',anchor_count; end if;

  current_session_id:=(first_result->'data'->'anchor'->>'sessionId')::uuid;
  update timegenie.unassigned_segments set started_at=now()-interval '2 minutes'
  where session_id=current_session_id and ended_at is null;
  update timegenie.unassigned_sessions set first_started_at=now()-interval '2 minutes',
    last_continuous_at=now()-interval '2 minutes' where id=current_session_id;
  current_state:=timegenie.direct_cloud_unassigned_state(ws_id,device_b,current_session_id,1);
  session_version:=(current_state->'data'->'anchor'->>'version')::bigint;
  work_result:=timegenie.direct_cloud_unassigned_resolve(
    ws_id,device_a,'4e000000-0000-0000-0000-000000000104',1,
    jsonb_build_object(
      'sessionId',current_session_id,'expectedVersion',session_version,'resolutionType','work',
      'allocations',jsonb_build_array(jsonb_build_object(
        'taskId',task_id,'minutes',1,'completeTask',true,
        'taskExpectedVersion',(task_result->'data'->'task'->>'version')::bigint
      ))
    )
  );
  work_retry:=timegenie.direct_cloud_unassigned_resolve(
    ws_id,device_a,'4e000000-0000-0000-0000-000000000104',1,
    jsonb_build_object(
      'sessionId',current_session_id,'expectedVersion',session_version,'resolutionType','work',
      'allocations',jsonb_build_array(jsonb_build_object(
        'taskId',task_id,'minutes',1,'completeTask',true,
        'taskExpectedVersion',(task_result->'data'->'task'->>'version')::bigint
      ))
    )
  );
  if work_result is distinct from work_retry
     or work_result->'data'->'session'->>'resolutionType' is distinct from 'work'
     or work_result->'data'->'generatedEntry'->>'kind' is distinct from 'work'
     or (work_result->'data'->'generatedEntry'->>'allocatedMinutes')::integer<>1
     or work_result->'data'->'daySummary'->>'workSeconds' is null
     or (timegenie.direct_cloud_task_detail(
          ws_id,device_b,task_id,1
        )->'data'->'task'->>'status') is distinct from 'done' then
    raise exception 'direct unassigned work resolution was not atomic/idempotent: % / %',
      work_result,work_retry;
  end if;
  select count(*) into generated_count from timegenie.time_entries
  where workspace_id=ws_id and origin_unassigned_session_id=current_session_id;
  select count(*) into processed_count from timegenie.processed_operations
  where workspace_id=ws_id and operation_id='4e000000-0000-0000-0000-000000000104';
  if generated_count<>1 or processed_count<>1 then
    raise exception 'direct unassigned retry duplicated entry/operation: % / %',generated_count,processed_count;
  end if;
  begin
    perform timegenie.direct_cloud_unassigned_resolve(
      ws_id,device_b,'4e000000-0000-0000-0000-000000000105',1,
      jsonb_build_object(
        'sessionId',current_session_id,'expectedVersion',session_version,'resolutionType','discard',
        'allocations','[]'::jsonb
      )
    );
    raise exception 'stale device resolved an already-classified anchor';
  exception when others then
    if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if;
  end;
  select count(*) into processed_count from timegenie.processed_operations
  where workspace_id=ws_id and operation_id='4e000000-0000-0000-0000-000000000105';
  if processed_count<>0 then raise exception 'rejected stale unassigned write recorded success'; end if;

  break_anchor:=work_result->'data'->'nextSession';
  update timegenie.unassigned_segments set started_at=now()-interval '90 seconds'
  where session_id=(break_anchor->>'sessionId')::uuid and ended_at is null;
  update timegenie.unassigned_sessions set first_started_at=now()-interval '90 seconds',
    last_continuous_at=now()-interval '90 seconds'
  where id=(break_anchor->>'sessionId')::uuid;
  stale_state:=timegenie.direct_cloud_unassigned_state(
    ws_id,device_b,(break_anchor->>'sessionId')::uuid,1
  );
  break_result:=timegenie.direct_cloud_unassigned_resolve(
    ws_id,device_b,'4e000000-0000-0000-0000-000000000106',1,
    jsonb_build_object(
      'sessionId',break_anchor->>'sessionId',
      'expectedVersion',(stale_state->'data'->'anchor'->>'version')::bigint,
      'resolutionType','break','allocations','[]'::jsonb
    )
  );
  if break_result->'data'->'generatedEntry'->>'kind' is distinct from 'break'
     or break_result->'data'->'session'->>'resolutionType' is distinct from 'break'
     or (break_result->'data'->'daySummary'->>'breakSeconds')::bigint<90 then
    raise exception 'direct unassigned break classification changed calendar totals: %',break_result;
  end if;

  discard_anchor:=break_result->'data'->'nextSession';
  update timegenie.unassigned_segments set started_at=now()-interval '60 seconds'
  where session_id=(discard_anchor->>'sessionId')::uuid and ended_at is null;
  update timegenie.unassigned_sessions set first_started_at=now()-interval '60 seconds',
    last_continuous_at=now()-interval '60 seconds'
  where id=(discard_anchor->>'sessionId')::uuid;
  stale_state:=timegenie.direct_cloud_unassigned_state(
    ws_id,device_a,(discard_anchor->>'sessionId')::uuid,1
  );
  discard_result:=timegenie.direct_cloud_unassigned_resolve(
    ws_id,device_a,'4e000000-0000-0000-0000-000000000107',1,
    jsonb_build_object(
      'sessionId',discard_anchor->>'sessionId',
      'expectedVersion',(stale_state->'data'->'anchor'->>'version')::bigint,
      'resolutionType','discard','allocations','[]'::jsonb
    )
  );
  if discard_result->'data'->'generatedEntry'->>'kind' is distinct from 'break'
     or discard_result->'data'->'session'->>'resolutionType' is distinct from 'discard'
     or discard_result->'data'->'session'->>'state' is distinct from 'discarded' then
    raise exception 'direct unassigned discard classification is inconsistent: %',discard_result;
  end if;

  -- Replace the current zero-duration anchor with a fixture that started on
  -- the previous day. Ensuring again must close it exactly at that day's end
  -- and create today's single anchor at today's boundary.
  delete from timegenie.unassigned_segments
  where session_id=(discard_result->'data'->'nextSession'->>'sessionId')::uuid;
  delete from timegenie.unassigned_sessions
  where id=(discard_result->'data'->'nextSession'->>'sessionId')::uuid;
  yesterday_id:='72000000-0000-0000-0000-0000000000d1';
  insert into timegenie.unassigned_sessions(
    id,workspace_id,work_date,state,threshold_seconds,duration_seconds,first_started_at,
    shared_source,migration_state,created_at,updated_at,version,last_continuous_at
  ) values(
    yesterday_id,ws_id,today_date-1,'collecting',30,0,
    (today_date-1)::timestamp at time zone 'UTC'+interval '23 hours 58 minutes',
    'cloud','adopted',now(),now(),1,
    (today_date-1)::timestamp at time zone 'UTC'+interval '23 hours 58 minutes'
  );
  insert into timegenie.unassigned_segments(
    id,workspace_id,session_id,sequence_no,started_at,duration_seconds
  ) values(
    '73000000-0000-0000-0000-0000000000d1',ws_id,yesterday_id,1,
    (today_date-1)::timestamp at time zone 'UTC'+interval '23 hours 58 minutes',0
  );
  first_result:=timegenie.direct_cloud_unassigned_ensure(
    ws_id,device_a,'4e000000-0000-0000-0000-000000000108',1,'{}'::jsonb
  );
  select duration_seconds into yesterday_seconds from timegenie.unassigned_sessions
  where id=yesterday_id;
  if yesterday_seconds<>120
     or first_result->'data'->'anchor'->>'workDate' is distinct from today_date::text
     or to_timestamp((first_result->'data'->'anchor'->>'firstStartedAt')::double precision/1000)
          is distinct from today_date::timestamp at time zone 'UTC' then
    raise exception 'direct unassigned calendar rollover is inconsistent: % / %',
      yesterday_seconds,first_result;
  end if;
  select count(*) into anchor_count from timegenie.unassigned_sessions
  where workspace_id=ws_id and work_date=today_date
    and state in ('collecting','awaiting_resolution');
  if anchor_count<>1 then raise exception 'calendar rollover created % current anchors',anchor_count; end if;
end $$;
reset role;

-- Manual time and actual-time edits are authoritative cloud transactions.
-- Stable operation IDs make response-loss retries return the original result.
reset role;
insert into auth.users(id) values('10000000-0000-0000-0000-000000000007')
on conflict(id) do nothing;
insert into timegenie.workspaces(id,owner_user_id,name,timezone) values(
  '20000000-0000-0000-0000-00000000000e',
  '10000000-0000-0000-0000-000000000007',
  '直连手工与实际工时验收工作空间','UTC'
) on conflict(id) do nothing;
set local role authenticated;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000007',true);
select set_config('request.jwt.claim.session_id','80000000-0000-0000-0000-00000000000e',true);
select set_config('request.headers','{"x-timegenie-client-schema-generation":"1"}',true);
select timegenie.workspace_initialize_defaults('20000000-0000-0000-0000-00000000000e');
select timegenie.device_authorize(
  '20000000-0000-0000-0000-00000000000e','30000000-0000-0000-0000-0000000000e1',
  '直连手工时间设备','windows','0.4.6'
);
select timegenie.upgrade_storage_protocol(
  '20000000-0000-0000-0000-00000000000e','30000000-0000-0000-0000-0000000000e1',
  '4e000000-0000-0000-0000-000000000200',1,'queue_drained',false
);
do $$
declare
  ws_id uuid:='20000000-0000-0000-0000-00000000000e';
  device_id uuid:='30000000-0000-0000-0000-0000000000e1';
  subject_id uuid; task_id uuid:='61000000-0000-0000-0000-0000000000e1';
  manual_result jsonb; manual_retry jsonb; snapshot_before jsonb; snapshot_after jsonb;
  batch_result jsonb; batch_retry jsonb; delete_result jsonb; delete_retry jsonb;
  bootstrap_value jsonb; task_value jsonb; adjustment_value jsonb; adjustment_id uuid; created_task_id uuid;
  current_date_value date;
  entry_count integer; operation_count integer; mapping_count integer;
  manual_entry_id uuid; corrected_result jsonb; corrected_retry jsonb;
  disposition_result jsonb; disposition_retry jsonb; history_page jsonb;
  correction_started_at bigint; correction_ended_at bigint;
begin
  bootstrap_value:=timegenie.direct_cloud_workbench_bootstrap(ws_id,device_id,1);
  subject_id:=(bootstrap_value->'data'->'subjects'->0->>'id')::uuid;
  current_date_value:=(bootstrap_value->'data'->>'currentWorkDate')::date;
  task_value:=timegenie.direct_cloud_task_create(
    ws_id,device_id,'4e000000-0000-0000-0000-000000000201',1,
    jsonb_build_object('id',task_id,'subjectId',subject_id,'title','手工时间映射事项')
  )->'data'->'task';
  manual_result:=timegenie.direct_cloud_manual_time_create(
    ws_id,device_id,'4e000000-0000-0000-0000-000000000202',1,
    jsonb_build_object(
      'taskId',task_id,'workDate',current_date_value,'minutes',5,'note','手工补录',
      'completeTask',true,'taskExpectedVersion',(task_value->>'version')::bigint
    )
  );
  manual_retry:=timegenie.direct_cloud_manual_time_create(
    ws_id,device_id,'4e000000-0000-0000-0000-000000000202',1,
    jsonb_build_object(
      'taskId',task_id,'workDate',current_date_value,'minutes',5,'note','手工补录',
      'completeTask',true,'taskExpectedVersion',(task_value->>'version')::bigint
    )
  );
  if manual_result is distinct from manual_retry
     or manual_result->'data'->'entry'->>'sourceType' is distinct from 'manual'
     or (manual_result->'data'->'entry'->>'settlementMinutes')::integer<>5
     or (manual_result->'data'->'entry'->>'allocatedMinutes')::integer<>5
     or (timegenie.direct_cloud_task_detail(ws_id,device_id,task_id,1)
       ->'data'->'task'->>'status') is distinct from 'done' then
    raise exception 'direct manual time was not atomic/idempotent: % / %',manual_result,manual_retry;
  end if;
  manual_entry_id:=(manual_result->'data'->'entry'->>'id')::uuid;
  correction_started_at:=(extract(epoch from(now()-interval '8 minutes'))*1000)::bigint;
  correction_ended_at:=(extract(epoch from(now()-interval '2 minutes'))*1000)::bigint;
  corrected_result:=timegenie.direct_cloud_time_entry_update(
    ws_id,device_id,'4e000000-0000-0000-0000-000000000207',1,
    jsonb_build_object(
      'entryId',manual_entry_id,
      'startedAt',correction_started_at,
      'endedAt',correction_ended_at,
      'note','云端修正','expectedVersion',1
    )
  );
  corrected_retry:=timegenie.direct_cloud_time_entry_update(
    ws_id,device_id,'4e000000-0000-0000-0000-000000000207',1,
    jsonb_build_object(
      'entryId',manual_entry_id,
      'startedAt',correction_started_at,
      'endedAt',correction_ended_at,
      'note','云端修正','expectedVersion',1
    )
  );
  if corrected_result is distinct from corrected_retry
     or (corrected_result->'data'->'entry'->>'version')::bigint<>2
     or corrected_result->'data'->'entry'->>'note' is distinct from '云端修正'
     or (corrected_result->'data'->'entry'->>'durationSeconds')::bigint not between 359 and 361 then
    raise exception 'direct time-entry correction was not authoritative/idempotent: % / %',
      corrected_result,corrected_retry;
  end if;
  disposition_result:=timegenie.direct_cloud_time_entry_disposition(
    ws_id,device_id,'4e000000-0000-0000-0000-000000000208',1,
    jsonb_build_object(
      'entryId',manual_entry_id,'expectedVersion',2,'disposition','break'
    )
  );
  disposition_retry:=timegenie.direct_cloud_time_entry_disposition(
    ws_id,device_id,'4e000000-0000-0000-0000-000000000208',1,
    jsonb_build_object(
      'entryId',manual_entry_id,'expectedVersion',2,'disposition','break'
    )
  );
  if disposition_result is distinct from disposition_retry
     or disposition_result->'data'->'entry'->>'kind' is distinct from 'break'
     or (disposition_result->'data'->'entry'->>'allocatedMinutes')::integer<>0
     or (disposition_result->'data'->'entry'->>'version')::bigint<>3 then
    raise exception 'direct time-entry disposition was not authoritative/idempotent: % / %',
      disposition_result,disposition_retry;
  end if;
  history_page:=timegenie.direct_cloud_time_history_page(ws_id,device_id,1,true,1);
  if (history_page->'data'->>'page')::integer<>1
     or (history_page->'data'->>'pageSize')::integer<>7
     or (history_page->'data'->>'totalDays')::integer<>1
     or not exists(
       select 1
       from jsonb_array_elements(history_page->'data'->'groups') history_group
       cross join lateral jsonb_array_elements(history_group->'entries') history_entry
       where history_entry->>'id'=manual_entry_id::text
     ) then
    raise exception 'direct time-entry history page did not use authoritative cloud data: %',history_page;
  end if;
  reset role;
  select count(*) into entry_count from timegenie.time_entries entry
  where entry.workspace_id=ws_id and entry.id=manual_entry_id;
  select count(*) into operation_count from timegenie.processed_operations operation
  where operation.workspace_id=ws_id and operation.operation_id='4e000000-0000-0000-0000-000000000202';
  if entry_count<>1 or operation_count<>1 then
    raise exception 'direct manual time retry duplicated entry/operation: % / %',entry_count,operation_count;
  end if;
  set role authenticated;

  snapshot_before:=timegenie.direct_cloud_actual_adjustment_snapshot(
    ws_id,device_id,subject_id,current_date_value,false,1
  );
  select value into task_value from jsonb_array_elements(snapshot_before->'data'->'tasks') value
  where value->>'id'=task_id::text;
  if task_value is null or (task_value->>'directMinutes')::integer<>0
     or (task_value->>'totalMinutes')::integer<>0
     or task_value->>'adjustmentFingerprint' is null then
    raise exception 'direct actual snapshot did not use authoritative cloud data: %',snapshot_before;
  end if;

  batch_result:=timegenie.direct_cloud_actual_adjustment_batch_apply(
    ws_id,device_id,'4e000000-0000-0000-0000-000000000203',1,
    jsonb_build_object(
      'subjectId',subject_id,'workDate',current_date_value,
      'clientRequestId','4e000000-0000-0000-0000-000000000203',
      'batchId','41000000-0000-0000-0000-0000000000e1','note','批量校准',
      'items',jsonb_build_array(jsonb_build_object(
        'taskId',task_id,'expectedTaskVersion',(task_value->>'version')::bigint,
        'baselineMinutes',0,'targetMinutes',3,'deltaMinutes',3,
        'adjustmentFingerprint',task_value->>'adjustmentFingerprint'
      )),
      'newTasks',jsonb_build_array(jsonb_build_object(
        'clientTaskId','draft-cloud-task','title','调整时新建事项',
        'parentId',null,'expectedParentVersion',null,'targetMinutes',6
      ))
    )
  );
  batch_retry:=timegenie.direct_cloud_actual_adjustment_batch_apply(
    ws_id,device_id,'4e000000-0000-0000-0000-000000000203',1,
    jsonb_build_object(
      'subjectId',subject_id,'workDate',current_date_value,
      'clientRequestId','4e000000-0000-0000-0000-000000000203',
      'batchId','41000000-0000-0000-0000-0000000000e1','note','批量校准',
      'items',jsonb_build_array(jsonb_build_object(
        'taskId',task_id,'expectedTaskVersion',(task_value->>'version')::bigint,
        'baselineMinutes',0,'targetMinutes',3,'deltaMinutes',3,
        'adjustmentFingerprint',task_value->>'adjustmentFingerprint'
      )),
      'newTasks',jsonb_build_array(jsonb_build_object(
        'clientTaskId','draft-cloud-task','title','调整时新建事项',
        'parentId',null,'expectedParentVersion',null,'targetMinutes',6
      ))
    )
  );
  created_task_id:=(batch_result->'data'->'createdTasks'->0->>'taskId')::uuid;
  reset role;
  if batch_result is distinct from batch_retry
     or (batch_result->'data'->>'createdTaskCount')::integer<>1
     or (batch_result->'data'->>'createdAdjustmentCount')::integer<>2
     or timegenie.direct_cloud_actual_minutes(ws_id,task_id,current_date_value)<>3
     or timegenie.direct_cloud_actual_minutes(ws_id,created_task_id,current_date_value)<>6 then
    raise exception 'direct actual batch did not atomically create task mapping and adjustments: %',batch_result;
  end if;
  select count(*) into mapping_count from timegenie.tasks task
  where task.workspace_id=ws_id and task.source_ref='actual_adjustment_quick_create';
  select count(*) into operation_count from timegenie.actual_time_adjustments adjustment
  where adjustment.workspace_id=ws_id and adjustment.batch_id='41000000-0000-0000-0000-0000000000e1';
  if mapping_count<>1 or operation_count<>2 then
    raise exception 'direct actual batch retry duplicated task or adjustment: % / %',mapping_count,operation_count;
  end if;
  set role authenticated;

  begin
    perform timegenie.direct_cloud_actual_adjustment_batch_apply(
      ws_id,device_id,'4e000000-0000-0000-0000-000000000204',1,
      jsonb_build_object(
        'subjectId',subject_id,'workDate',current_date_value,
        'clientRequestId','4e000000-0000-0000-0000-000000000204',
        'batchId','41000000-0000-0000-0000-0000000000e2',
        'items',jsonb_build_array(jsonb_build_object(
          'taskId',task_id,'expectedTaskVersion',(task_value->>'version')::bigint,
          'baselineMinutes',0,'targetMinutes',4,'deltaMinutes',4,
          'adjustmentFingerprint',task_value->>'adjustmentFingerprint'
        )),'newTasks','[]'::jsonb
      )
    );
    raise exception 'stale actual adjustment batch was accepted';
  exception when others then
    if sqlerrm not like '%ADJUSTMENT_BASELINE_CONFLICT%' then raise; end if;
  end;

  snapshot_after:=timegenie.direct_cloud_actual_adjustment_snapshot(
    ws_id,device_id,subject_id,current_date_value,false,1
  );
  select value into task_value from jsonb_array_elements(snapshot_after->'data'->'tasks') value
  where value->>'id'=task_id::text;
  begin
    perform timegenie.direct_cloud_actual_adjustment_batch_apply(
      ws_id,device_id,'4e000000-0000-0000-0000-000000000205',1,
      jsonb_build_object(
        'subjectId',subject_id,'workDate',current_date_value,
        'clientRequestId','4e000000-0000-0000-0000-000000000205',
        'batchId','41000000-0000-0000-0000-0000000000e3',
        'items',jsonb_build_array(jsonb_build_object(
          'taskId',task_id,'expectedTaskVersion',(task_value->>'version')::bigint,
          'baselineMinutes',3,'targetMinutes',5,'deltaMinutes',2,
          'adjustmentFingerprint',task_value->>'adjustmentFingerprint'
        )),
        'newTasks',jsonb_build_array(jsonb_build_object(
          'clientTaskId','must-roll-back','title','必须整体回滚',
          'parentId','61000000-0000-0000-0000-0000000000ff',
          'expectedParentVersion',1,'targetMinutes',4
        ))
      )
    );
    raise exception 'invalid actual adjustment compound batch was accepted';
  exception when others then
    if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if;
  end;
  reset role;
  if exists(select 1 from timegenie.actual_time_adjustments adjustment
       where adjustment.workspace_id=ws_id and adjustment.batch_id='41000000-0000-0000-0000-0000000000e3')
     or exists(select 1 from timegenie.tasks task
       where task.workspace_id=ws_id and task.title='必须整体回滚')
     or exists(select 1 from timegenie.processed_operations operation
       where operation.workspace_id=ws_id and operation.operation_id='4e000000-0000-0000-0000-000000000205') then
    raise exception 'failed actual adjustment batch left partial direct data';
  end if;
  set role authenticated;

  select value into adjustment_value from jsonb_array_elements(batch_result->'data'->'adjustments') value
  where value->>'taskId'=task_id::text;
  adjustment_id:=(adjustment_value->>'id')::uuid;
  delete_result:=timegenie.direct_cloud_actual_adjustment_delete(
    ws_id,device_id,'4e000000-0000-0000-0000-000000000206',1,
    jsonb_build_object('adjustmentId',adjustment_id,'expectedVersion',1)
  );
  delete_retry:=timegenie.direct_cloud_actual_adjustment_delete(
    ws_id,device_id,'4e000000-0000-0000-0000-000000000206',1,
    jsonb_build_object('adjustmentId',adjustment_id,'expectedVersion',1)
  );
  reset role;
  if delete_result is distinct from delete_retry
     or (delete_result->'data'->'adjustment'->>'version')::integer<>2
     or delete_result->'data'->'adjustment'->>'deletedAt' is null
     or timegenie.direct_cloud_actual_minutes(ws_id,task_id,current_date_value)<>0 then
    raise exception 'direct actual adjustment delete was not versioned/idempotent: % / %',
      delete_result,delete_retry;
  end if;
  set role authenticated;
end $$;
reset role;

do $$ begin
  if exists(select 1 from timegenie.workspace_restore_locks where workspace_id='20000000-0000-0000-0000-000000000001') then raise exception 'restore lock was not released'; end if;
  if not exists(select 1 from timegenie.workspace_snapshots where id=current_setting('timegenie.restore_test_safety_id')::uuid and snapshot_type='pre_restore' and status='ready') then raise exception 'restore safety snapshot missing'; end if;
  if not exists(select 1 from timegenie.workspace_changes where workspace_id='20000000-0000-0000-0000-000000000001' and entity_type='workspace_reset' and entity_version=3) then raise exception 'workspace reset event missing'; end if;
  if not exists(select 1 from timegenie.cloud_operation_logs where workspace_id='20000000-0000-0000-0000-000000000001' and operation_id='4f000000-0000-0000-0000-000000000004' and source='restore') then raise exception 'restore operation history missing'; end if;
end $$;

select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
select set_config('request.headers','{"x-timegenie-workspace-epoch":"3","x-timegenie-client-schema-generation":"1"}',true);
do $$
declare target_task uuid; target_subject uuid; snapshot jsonb; adjustment_id uuid := '4f000000-0000-0000-0000-000000000099';
begin
  select id, subject_id into target_task, target_subject from timegenie.tasks
  where workspace_id='20000000-0000-0000-0000-000000000001' and deleted_at is null order by id limit 1;
  if target_task is null then raise exception 'actual adjustment snapshot test has no task'; end if;
  insert into timegenie.actual_time_adjustments(
    id,workspace_id,subject_id,task_id,work_date,delta_minutes,baseline_minutes,target_minutes,source_type,batch_id,created_at,updated_at,version
  ) values(
    adjustment_id,'20000000-0000-0000-0000-000000000001',target_subject,target_task,'2026-10-21',15,0,15,'quick_actual_adjustment',gen_random_uuid(),now(),now(),1
  ) on conflict(id) do nothing;
  snapshot:=timegenie.cloud_snapshot_get('20000000-0000-0000-0000-000000000001');
  if not (snapshot ? 'actual_time_adjustments') then raise exception 'cloud snapshot omitted actual_time_adjustments'; end if;
  if not exists(select 1 from jsonb_array_elements(snapshot->'actual_time_adjustments') value where value->>'id'=adjustment_id::text and (value->>'target_minutes')::integer=15) then
    raise exception 'cloud snapshot did not retain the saved actual time adjustment';
  end if;
  if timegenie.cloud_incremental_entity_get('20000000-0000-0000-0000-000000000001','actual_time_adjustments',adjustment_id::text)->'data' is null then
    raise exception 'incremental entity get reported an existing actual time adjustment as deleted';
  end if;
end $$;
reset role;

-- Full snapshots remain consumable by older clients that restore parent links
-- while walking the task array: source tasks and parents must appear first.
do $$
declare target_subject uuid; snapshot jsonb; source_parent_pos bigint; source_child_pos bigint; instance_parent_pos bigint; instance_child_pos bigint;
begin
  select id into target_subject from timegenie.subjects
  where workspace_id='20000000-0000-0000-0000-000000000001' and deleted_at is null order by id limit 1;
  insert into timegenie.tasks(id,workspace_id,subject_id,parent_id,title,status,source_type,sort_order,day_instance_date,day_instance_kind,day_instance_source_task_id) values
    ('6d000000-0000-0000-0000-000000000001','20000000-0000-0000-0000-000000000001',target_subject,null,'快照排序源父事项','open','manual',901,null,null,null),
    ('6d000000-0000-0000-0000-000000000002','20000000-0000-0000-0000-000000000001',target_subject,'6d000000-0000-0000-0000-000000000001','快照排序源子事项','open','manual',902,null,null,null),
    ('6d000000-0000-0000-0000-000000000003','20000000-0000-0000-0000-000000000001',target_subject,null,'快照排序实例父事项','open','manual',903,'2026-10-22','context','6d000000-0000-0000-0000-000000000001'),
    ('6d000000-0000-0000-0000-000000000004','20000000-0000-0000-0000-000000000001',target_subject,'6d000000-0000-0000-0000-000000000003','快照排序实例子事项','open','manual',904,'2026-10-22','member','6d000000-0000-0000-0000-000000000002');
  perform set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
  perform set_config('request.headers','{"x-timegenie-workspace-epoch":"3","x-timegenie-client-schema-generation":"1"}',true);
  snapshot:=timegenie.cloud_snapshot_get('20000000-0000-0000-0000-000000000001');
  select ordinality into source_parent_pos from jsonb_array_elements(snapshot->'tasks') with ordinality item where item.value->>'id'='6d000000-0000-0000-0000-000000000001';
  select ordinality into source_child_pos from jsonb_array_elements(snapshot->'tasks') with ordinality item where item.value->>'id'='6d000000-0000-0000-0000-000000000002';
  select ordinality into instance_parent_pos from jsonb_array_elements(snapshot->'tasks') with ordinality item where item.value->>'id'='6d000000-0000-0000-0000-000000000003';
  select ordinality into instance_child_pos from jsonb_array_elements(snapshot->'tasks') with ordinality item where item.value->>'id'='6d000000-0000-0000-0000-000000000004';
  if source_parent_pos is null or source_child_pos is null or instance_parent_pos is null or instance_child_pos is null
     or source_parent_pos>=source_child_pos or instance_parent_pos>=instance_child_pos
     or source_parent_pos>=instance_parent_pos or source_child_pos>=instance_child_pos then
    raise exception 'cloud snapshot task dependency order is unsafe: source parent %, source child %, instance parent %, instance child %',
      source_parent_pos,source_child_pos,instance_parent_pos,instance_child_pos;
  end if;
end $$;

set local role authenticated;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
select set_config('request.headers','{"x-timegenie-workspace-epoch":"3","x-timegenie-client-schema-generation":"1"}',true);
do $$
declare ignored jsonb; repeated jsonb; accepted jsonb;
begin
  ignored:=timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '4d000000-0000-0000-0000-000000000001','task_day_plan_materialize','task_day_plan',
    '6d000000-0000-0000-0000-000000000001|2026-10-22',null,1,1,
    jsonb_build_object('task_id','6d000000-0000-0000-0000-000000000001','work_date','2026-10-22','state','included','instance_task_id','6d000000-0000-0000-0000-000000000003','created_at',1792622400000,'updated_at',1792622400000,'version',1)
  );
  repeated:=timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '4d000000-0000-0000-0000-000000000001','task_day_plan_materialize','task_day_plan',
    '6d000000-0000-0000-0000-000000000001|2026-10-22',null,1,1,
    jsonb_build_object('task_id','6d000000-0000-0000-0000-000000000001','work_date','2026-10-22','state','included','instance_task_id','6d000000-0000-0000-0000-000000000003','created_at',1792622400000,'updated_at',1792622400000,'version',1)
  );
  if not coalesce((ignored->>'ignoredContextPlan')::boolean,false) or ignored is distinct from repeated then
    raise exception 'legacy context day plan was not acknowledged idempotently: % / %',ignored,repeated;
  end if;
  if ignored->'authoritative' is distinct from 'null'::jsonb then
    raise exception 'legacy context day plan was persisted';
  end if;
  accepted:=timegenie.cloud_apply_patch(
    '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',
    '4d000000-0000-0000-0000-000000000002','task_day_plan_materialize','task_day_plan',
    '6d000000-0000-0000-0000-000000000002|2026-10-22',null,1,1,
    jsonb_build_object('task_id','6d000000-0000-0000-0000-000000000002','work_date','2026-10-22','state','included','instance_task_id','6d000000-0000-0000-0000-000000000004','created_at',1792622400000,'updated_at',1792622400000,'version',1)
  );
  if coalesce((accepted->>'ignoredContextPlan')::boolean,false)
     or accepted->>'entityId' is distinct from '6d000000-0000-0000-0000-000000000002|2026-10-22' then
    raise exception 'valid member day plan was not persisted: %',accepted;
  end if;
end $$;
reset role;

insert into auth.users(id) values('10000000-0000-0000-0000-000000000003')
on conflict(id) do nothing;
select set_config('request.headers','{"x-timegenie-workspace-epoch":"1","x-timegenie-client-schema-generation":"1"}',true);
insert into timegenie.workspaces(id,owner_user_id,name,timezone)
values('20000000-0000-0000-0000-00000000000a','10000000-0000-0000-0000-000000000003','直连协议测试工作空间','Pacific/Kiritimati');
insert into timegenie.subjects(id,workspace_id,name,sort_order) values
  ('51000000-0000-0000-0000-0000000000a1','20000000-0000-0000-0000-00000000000a','直连主体一',10),
  ('51000000-0000-0000-0000-0000000000a2','20000000-0000-0000-0000-00000000000a','直连主体二',20);
insert into timegenie.app_settings(workspace_id,key,value_json,version) values
  ('20000000-0000-0000-0000-00000000000a','user_name','"直连用户"'::jsonb,2),
  ('20000000-0000-0000-0000-00000000000a','shared_security_probe','{"visible":"保留","nested":{"managementKey":"management-credential-must-not-leak","theme":"ocean"}}'::jsonb,3);
insert into timegenie.work_days(workspace_id,work_date,timezone,work_period_text,note,settled_at,version) values
  ('20000000-0000-0000-0000-00000000000a',(now() at time zone 'Pacific/Kiritimati')::date,'Pacific/Kiritimati','10:00-12:00 13:30-17:30','直连工作日',null,2),
  ('20000000-0000-0000-0000-00000000000a',(now() at time zone 'Pacific/Kiritimati')::date+1,'Pacific/Kiritimati',null,'休息日',now(),1);
insert into timegenie.subject_work_schedules(workspace_id,subject_id,weekday,is_workday,version)
select '20000000-0000-0000-0000-00000000000a',subject.id,weekday.value,weekday.value<5,case when subject.sort_order=10 and weekday.value=0 then 4 else 1 end
from timegenie.subjects subject cross join generate_series(0,6) weekday(value)
where subject.workspace_id='20000000-0000-0000-0000-00000000000a';
insert into timegenie.subject_work_schedule_segments(id,workspace_id,schedule_id,sequence_no,start_minute,end_minute)
select gen_random_uuid(),schedule.workspace_id,schedule.id,segment.sequence_no,segment.start_minute,segment.end_minute
from timegenie.subject_work_schedules schedule
cross join (values (1,600,720),(2,810,1050)) segment(sequence_no,start_minute,end_minute)
where schedule.workspace_id='20000000-0000-0000-0000-00000000000a' and schedule.weekday<5;
insert into timegenie.integration_configs(workspace_id,provider,enabled,config_json,version) values
  ('20000000-0000-0000-0000-00000000000a','obsidian',true,'{"vaultName":"TimeGenie","nested":{"api_key":"nested-api-key-must-not-leak"}}'::jsonb,2),
  ('20000000-0000-0000-0000-00000000000a','seatable',true,'{"serverUrl":"https://cloud.seatable.cn","baseUuid":"base-public-id","baseApiToken":"base-token-must-not-leak","password":"password-must-not-leak"}'::jsonb,5);
insert into timegenie.external_bindings(id,workspace_id,provider,entity_type,entity_id,external_id,external_revision,last_synced_hash,last_synced_at) values
  ('e1000000-0000-0000-0000-0000000000a1','20000000-0000-0000-0000-00000000000a','seatable','task','61000000-0000-0000-0000-0000000000a1','row-public-id','revision-public-id','binding-hash-must-not-leak',now());
do $$
declare snapshot_probe jsonb; log_probe jsonb;
begin
  snapshot_probe:=timegenie.workspace_snapshot_sanitize(jsonb_build_object(
    'integration_configs',jsonb_build_array(jsonb_build_object(
      'provider','seatable','config_json',jsonb_build_object(
        'serverUrl','https://cloud.seatable.cn','baseApiToken','snapshot-token-must-not-leak','managementKey','snapshot-management-key-must-not-leak'
      )
    ))
  ));
  log_probe:=timegenie.operation_history_safe_json('integration_config',jsonb_build_object(
    'provider','seatable','enabled',true,'version',5,'config_json',jsonb_build_object('baseApiToken','log-token-must-not-leak')
  ));
  if snapshot_probe::text~*'(baseApiToken|managementKey|snapshot-token-must-not-leak|snapshot-management-key-must-not-leak)'
     or log_probe::text~*'(config_json|baseApiToken|log-token-must-not-leak)' then
    raise exception 'shared integration credentials leaked into snapshot or operation history: % / %',snapshot_probe,log_probe;
  end if;
end $$;
insert into timegenie.tasks(id,workspace_id,subject_id,title,status,source_type,sort_order)
values('61000000-0000-0000-0000-0000000000a1','20000000-0000-0000-0000-00000000000a','51000000-0000-0000-0000-0000000000a1','直连读取事项','open','manual',10);
insert into timegenie.tasks(id,workspace_id,subject_id,parent_id,title,status,source_type,sort_order) values
  ('61000000-0000-0000-0000-0000000000a2','20000000-0000-0000-0000-00000000000a','51000000-0000-0000-0000-0000000000a1',null,'汇总事项','open','manual',5),
  ('61000000-0000-0000-0000-0000000000a3','20000000-0000-0000-0000-00000000000a','51000000-0000-0000-0000-0000000000a1','61000000-0000-0000-0000-0000000000a2','叶子事项','open','manual',5),
  ('61000000-0000-0000-0000-0000000000a4','20000000-0000-0000-0000-00000000000a','51000000-0000-0000-0000-0000000000a1',null,'归档日实例','open','manual',20);
update timegenie.tasks set day_instance_date=(now() at time zone 'Pacific/Kiritimati')::date,
  day_instance_kind='member',day_instance_source_task_id='61000000-0000-0000-0000-0000000000a1'
where id='61000000-0000-0000-0000-0000000000a4';
insert into timegenie.tasks(id,workspace_id,subject_id,title,status,source_type,sort_order,deleted_at)
values('61000000-0000-0000-0000-0000000000a5','20000000-0000-0000-0000-00000000000a','51000000-0000-0000-0000-0000000000a1','已删除事项','open','manual',30,now());
insert into timegenie.tasks(id,workspace_id,subject_id,title,status,source_type,sort_order,completed_at)
select gen_random_uuid(),'20000000-0000-0000-0000-00000000000a','51000000-0000-0000-0000-0000000000a1',
  '历史完成 '||lpad(series::text,2,'0'),'done','manual',100+series,now()-(series||' minutes')::interval
from generate_series(1,25) series;
insert into timegenie.actual_time_adjustments(
  id,workspace_id,subject_id,task_id,work_date,delta_minutes,baseline_minutes,target_minutes,source_type,batch_id
) values(
  '81000000-0000-0000-0000-0000000000a1','20000000-0000-0000-0000-00000000000a',
  '51000000-0000-0000-0000-0000000000a1','61000000-0000-0000-0000-0000000000a3',
  (now() at time zone 'Pacific/Kiritimati')::date,30,0,30,'quick_actual_adjustment',
  '41000000-0000-0000-0000-0000000000a1'
);
insert into timegenie.task_daily_estimates(workspace_id,task_id,work_date,estimate_minutes) values
  ('20000000-0000-0000-0000-00000000000a','61000000-0000-0000-0000-0000000000a3',(now() at time zone 'Pacific/Kiritimati')::date,45);
insert into timegenie.task_day_plans(workspace_id,task_id,work_date,state) values
  ('20000000-0000-0000-0000-00000000000a','61000000-0000-0000-0000-0000000000a1',(now() at time zone 'Pacific/Kiritimati')::date+1,'included');
insert into timegenie.task_recurrence_rules(id,workspace_id,task_id,frequency,effective_start) values
  ('71000000-0000-0000-0000-0000000000a1','20000000-0000-0000-0000-00000000000a','61000000-0000-0000-0000-0000000000a3','daily',(now() at time zone 'Pacific/Kiritimati')::date-1);
insert into timegenie.task_occurrences(workspace_id,task_id,occurrence_date,origin,status,completed_at,version) values
  ('20000000-0000-0000-0000-00000000000a','61000000-0000-0000-0000-0000000000a3',(now() at time zone 'Pacific/Kiritimati')::date,'scheduled','done',now(),2);
insert into timegenie.time_entries(
  id,workspace_id,work_date,kind,source_type,state,default_task_id,label_snapshot,started_at,ended_at,duration_seconds,version,timer_chain_id,split_boundary_at,last_continuous_at
) values
  ('91000000-0000-0000-0000-0000000000a1','20000000-0000-0000-0000-00000000000a',(now() at time zone 'Pacific/Kiritimati')::date-1,'work','timer','ended','61000000-0000-0000-0000-0000000000a3','跨日前段',now()-interval '26 hours',now()-interval '25 hours',3600,1,'91000000-0000-0000-0000-0000000000aa',now()-interval '25 hours',now()-interval '25 hours'),
  ('91000000-0000-0000-0000-0000000000a2','20000000-0000-0000-0000-00000000000a',(now() at time zone 'Pacific/Kiritimati')::date,'work','timer','paused','61000000-0000-0000-0000-0000000000a3','跨日后段',now()-interval '2 hours',null,1800,2,'91000000-0000-0000-0000-0000000000aa','2026-10-22 10:00:00+00',now()-interval '30 minutes'),
  ('91000000-0000-0000-0000-0000000000a3','20000000-0000-0000-0000-00000000000a',(now() at time zone 'Pacific/Kiritimati')::date,'break','manual','ended',null,'休息时间',now()-interval '4 hours',now()-interval '3 hours 45 minutes',900,1,null,null,now()-interval '3 hours 45 minutes');
insert into timegenie.time_segments(id,workspace_id,entry_id,sequence_no,started_at,ended_at,duration_seconds) values
  ('92000000-0000-0000-0000-0000000000a1','20000000-0000-0000-0000-00000000000a','91000000-0000-0000-0000-0000000000a2',1,now()-interval '2 hours',now()-interval '90 minutes',1800),
  ('92000000-0000-0000-0000-0000000000a2','20000000-0000-0000-0000-00000000000a','91000000-0000-0000-0000-0000000000a2',2,now()-interval '1 hour',null,0);
insert into timegenie.time_allocations(id,workspace_id,entry_id,task_id,minutes)
values('93000000-0000-0000-0000-0000000000a1','20000000-0000-0000-0000-00000000000a','91000000-0000-0000-0000-0000000000a2','61000000-0000-0000-0000-0000000000a3',20);
insert into timegenie.unassigned_sessions(
  id,workspace_id,work_date,state,threshold_seconds,duration_seconds,first_started_at,shared_source,migration_state,last_continuous_at
) values('94000000-0000-0000-0000-0000000000a1','20000000-0000-0000-0000-00000000000a',(now() at time zone 'Pacific/Kiritimati')::date,'collecting',300,0,now()-interval '10 minutes','cloud','adopted',now());
insert into timegenie.unassigned_segments(id,workspace_id,session_id,sequence_no,started_at,duration_seconds)
values('95000000-0000-0000-0000-0000000000a1','20000000-0000-0000-0000-00000000000a','94000000-0000-0000-0000-0000000000a1',1,now()-interval '10 minutes',0);
insert into timegenie.actual_time_adjustments(
  id,workspace_id,subject_id,task_id,work_date,delta_minutes,baseline_minutes,target_minutes,source_type,batch_id
) select gen_random_uuid(),'20000000-0000-0000-0000-00000000000a','51000000-0000-0000-0000-0000000000a1','61000000-0000-0000-0000-0000000000a3',
  (now() at time zone 'Pacific/Kiritimati')::date-series,5,0,5,'quick_actual_adjustment',gen_random_uuid() from generate_series(2,9) series;
insert into timegenie.report_templates(
  id,workspace_id,report_type,name,content,is_default,is_builtin,sort_order
) values
  ('a1000000-0000-0000-0000-0000000000a1','20000000-0000-0000-0000-00000000000a','daily','直连日报模板','# {{姓名}} {{日期}}\n\n{{今日事项}}\n\n合计 {{总工时}}',true,false,10),
  ('a1000000-0000-0000-0000-0000000000a2','20000000-0000-0000-0000-00000000000a','weekly','直连周报模板','# {{日期范围}}\n\n{{日报合集}}',true,false,10),
  ('a1000000-0000-0000-0000-0000000000a3','20000000-0000-0000-0000-00000000000a','monthly','直连月报模板','# {{日期范围}}\n\n{{统计信息}}',true,false,10),
  ('a1000000-0000-0000-0000-0000000000a4','20000000-0000-0000-0000-00000000000a','daily','已归档日报模板','# 历史模板',false,false,20);
update timegenie.report_templates set deleted_at=now() where id='a1000000-0000-0000-0000-0000000000a4';
insert into timegenie.reports(
  id,workspace_id,report_type,subject_id,period_start,period_end,reference_date,template_id,markdown_content,content_source,generation_count,generated_at,updated_at
) values
  ('b1000000-0000-0000-0000-0000000000a1','20000000-0000-0000-0000-00000000000a','daily','51000000-0000-0000-0000-0000000000a1',(now() at time zone 'Pacific/Kiritimati')::date,(now() at time zone 'Pacific/Kiritimati')::date,(now() at time zone 'Pacific/Kiritimati')::date,'a1000000-0000-0000-0000-0000000000a4','# 已保存日报正文','edited',2,now(),now()),
  ('b1000000-0000-0000-0000-0000000000a2','20000000-0000-0000-0000-00000000000a','weekly','51000000-0000-0000-0000-0000000000a1',(now() at time zone 'Pacific/Kiritimati')::date-6,(now() at time zone 'Pacific/Kiritimati')::date,(now() at time zone 'Pacific/Kiritimati')::date,'a1000000-0000-0000-0000-0000000000a2','# 已保存周报','generated',1,now(),now()-interval '1 minute');
insert into timegenie.report_tasks(report_id,task_id,workspace_id,sort_order,title_snapshot,path_snapshot) values
  ('b1000000-0000-0000-0000-0000000000a1','61000000-0000-0000-0000-0000000000a5','20000000-0000-0000-0000-00000000000a',10,'已删除事项','已删除事项');
insert into timegenie.reports(
  id,workspace_id,report_type,subject_id,period_start,period_end,reference_date,template_id,markdown_content,content_source,generation_count,updated_at
) select gen_random_uuid(),'20000000-0000-0000-0000-00000000000a','daily','51000000-0000-0000-0000-0000000000a1',
  (now() at time zone 'Pacific/Kiritimati')::date-series,(now() at time zone 'Pacific/Kiritimati')::date-series,(now() at time zone 'Pacific/Kiritimati')::date-series,
  'a1000000-0000-0000-0000-0000000000a1','# 批量日报 '||series,'generated',1,now()-(series||' minutes')::interval from generate_series(1,24) series;

-- Integration-only probes inspect protected rows without weakening production
-- table privileges for authenticated clients.
create or replace function timegenie.integration_direct_planning_probe(
  p_workspace_id uuid,p_task_id uuid,p_work_date date
) returns jsonb language sql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
  select jsonb_build_object(
    'task',case when task.id is null then null else jsonb_build_object(
      'id',task.id,'parentId',task.parent_id,'title',task.title,'status',task.status,
      'dayInstanceKind',task.day_instance_kind,
      'dayInstanceSourceTaskId',task.day_instance_source_task_id,
      'deletedAt',task.deleted_at,'version',task.version
    ) end,
    'estimate',(select jsonb_build_object(
      'minutes',estimate.estimate_minutes,'version',estimate.version
    ) from timegenie.task_daily_estimates estimate
      where estimate.workspace_id=p_workspace_id and estimate.task_id=p_task_id
        and estimate.work_date=p_work_date),
    'plan',(select jsonb_build_object(
      'state',plan.state,'version',plan.version,'instanceTaskId',plan.instance_task_id
    ) from timegenie.task_day_plans plan
      where plan.workspace_id=p_workspace_id and plan.task_id=p_task_id
        and plan.work_date=p_work_date),
    'allocatedMinutes',coalesce((select sum(allocation.minutes)
      from timegenie.time_allocations allocation
      where allocation.workspace_id=p_workspace_id and allocation.task_id=p_task_id),0),
    'statusEventCount',(select count(*) from timegenie.task_status_events event
      where event.workspace_id=p_workspace_id and event.task_id=p_task_id)
  )
  from (select 1) singleton
  left join timegenie.tasks task on task.workspace_id=p_workspace_id and task.id=p_task_id;
$$;

create or replace function timegenie.integration_direct_planning_add_time(
  p_workspace_id uuid,p_task_id uuid,p_work_date date
) returns void language plpgsql security definer
set search_path=timegenie,extensions,pg_catalog as $$
declare entry_id uuid:=gen_random_uuid(); task_title text;
begin
  select title into task_title from timegenie.tasks
  where workspace_id=p_workspace_id and id=p_task_id;
  insert into timegenie.time_entries(
    id,workspace_id,work_date,kind,source_type,state,default_task_id,label_snapshot,
    started_at,ended_at,duration_seconds
  ) values(entry_id,p_workspace_id,p_work_date,'work','manual','ended',p_task_id,
    coalesce(task_title,'今日实例历史工时'),p_work_date::timestamptz,
    p_work_date::timestamptz+interval '30 minutes',1800);
  insert into timegenie.time_allocations(
    id,workspace_id,entry_id,task_id,minutes
  ) values(gen_random_uuid(),p_workspace_id,entry_id,p_task_id,30);
end $$;

create or replace function timegenie.integration_direct_report_probe(
  p_workspace_id uuid,p_report_id uuid,p_operation_id uuid
) returns jsonb language sql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
  select jsonb_build_object(
    'reportCount',(select count(*) from timegenie.reports report where report.workspace_id=p_workspace_id and report.id=p_report_id),
    'version',(select report.version from timegenie.reports report where report.workspace_id=p_workspace_id and report.id=p_report_id),
    'deleted',(select report.deleted_at is not null from timegenie.reports report where report.workspace_id=p_workspace_id and report.id=p_report_id),
    'taskCount',(select count(*) from timegenie.report_tasks item where item.workspace_id=p_workspace_id and item.report_id=p_report_id),
    'tasks',coalesce((select jsonb_agg(jsonb_build_object(
      'taskId',item.task_id,'title',item.title_snapshot,'pathLabel',item.path_snapshot,'sortOrder',item.sort_order
    ) order by item.sort_order,item.task_id) from timegenie.report_tasks item
      where item.workspace_id=p_workspace_id and item.report_id=p_report_id),'[]'::jsonb),
    'operationCount',(select count(*) from timegenie.processed_operations operation
      where operation.workspace_id=p_workspace_id and operation.operation_id=p_operation_id)
  );
$$;

create or replace function timegenie.integration_direct_report_template_probe(
  p_workspace_id uuid,p_template_id uuid,p_operation_id uuid
) returns jsonb language sql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
  select jsonb_build_object(
    'templateCount',(select count(*) from timegenie.report_templates template
      where template.workspace_id=p_workspace_id and template.id=p_template_id),
    'reportType',(select template.report_type from timegenie.report_templates template
      where template.workspace_id=p_workspace_id and template.id=p_template_id),
    'name',(select template.name from timegenie.report_templates template
      where template.workspace_id=p_workspace_id and template.id=p_template_id),
    'description',(select template.description from timegenie.report_templates template
      where template.workspace_id=p_workspace_id and template.id=p_template_id),
    'content',(select template.content from timegenie.report_templates template
      where template.workspace_id=p_workspace_id and template.id=p_template_id),
    'version',(select template.version from timegenie.report_templates template
      where template.workspace_id=p_workspace_id and template.id=p_template_id),
    'sortOrder',(select template.sort_order from timegenie.report_templates template
      where template.workspace_id=p_workspace_id and template.id=p_template_id),
    'isDefault',(select template.is_default from timegenie.report_templates template
      where template.workspace_id=p_workspace_id and template.id=p_template_id),
    'deleted',(select template.deleted_at is not null from timegenie.report_templates template
      where template.workspace_id=p_workspace_id and template.id=p_template_id),
    'activeTypeCount',(select count(*) from timegenie.report_templates candidate
      where candidate.workspace_id=p_workspace_id and candidate.deleted_at is null
        and candidate.report_type=(select template.report_type from timegenie.report_templates template
          where template.workspace_id=p_workspace_id and template.id=p_template_id)),
    'defaultTypeCount',(select count(*) from timegenie.report_templates candidate
      where candidate.workspace_id=p_workspace_id and candidate.deleted_at is null and candidate.is_default
        and candidate.report_type=(select template.report_type from timegenie.report_templates template
          where template.workspace_id=p_workspace_id and template.id=p_template_id)),
    'boundReportCount',(select count(*) from timegenie.reports report
      where report.workspace_id=p_workspace_id and report.template_id=p_template_id),
    'operationCount',(select count(*) from timegenie.processed_operations operation
      where operation.workspace_id=p_workspace_id and operation.operation_id=p_operation_id)
  );
$$;

create or replace function timegenie.integration_direct_settings_probe(
  p_workspace_id uuid,p_operation_id uuid,p_setting_key text,p_work_date date,p_subject_id uuid,p_weekday integer
) returns jsonb language sql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
  select jsonb_build_object(
    'settingVersion',(select setting.version from timegenie.app_settings setting
      where setting.workspace_id=p_workspace_id and setting.key=p_setting_key),
    'settingValue',(select setting.value_json from timegenie.app_settings setting
      where setting.workspace_id=p_workspace_id and setting.key=p_setting_key),
    'workspaceTimezone',(select workspace.timezone from timegenie.workspaces workspace where workspace.id=p_workspace_id),
    'workDay',case when day.work_date is null then null else jsonb_build_object(
      'timezone',day.timezone,'workPeriodText',day.work_period_text,'note',day.note,'version',day.version
    ) end,
    'schedule',timegenie.direct_cloud_work_schedule_day_record(p_workspace_id,p_subject_id,p_weekday),
    'subjectSchedule',timegenie.direct_cloud_subject_schedule_record(p_workspace_id,p_subject_id),
    'operationCount',(select count(*) from timegenie.processed_operations operation
      where operation.workspace_id=p_workspace_id and operation.operation_id=p_operation_id),
    'historyItemCount',(select count(*) from timegenie.cloud_operation_log_items item
      join timegenie.cloud_operation_logs log on log.id=item.operation_log_id
      where log.workspace_id=p_workspace_id and log.operation_id=p_operation_id)
  ) from (select 1) singleton
  left join timegenie.work_days day on day.workspace_id=p_workspace_id and day.work_date=p_work_date;
$$;

create or replace function timegenie.integration_direct_integration_probe(
  p_workspace_id uuid,p_operation_id uuid,p_provider text,p_entity_type text,p_entity_id uuid
) returns jsonb language sql stable security definer
set search_path=timegenie,extensions,pg_catalog as $$
  select jsonb_build_object(
    'config',timegenie.direct_cloud_integration_record(p_workspace_id,p_provider),
    'binding',timegenie.direct_cloud_external_binding_record(p_workspace_id,p_provider,p_entity_type,p_entity_id),
    'bindingCount',(select count(*) from timegenie.external_bindings binding
      where binding.workspace_id=p_workspace_id and binding.provider=p_provider
        and binding.entity_type=p_entity_type and binding.entity_id=p_entity_id),
    'operationCount',(select count(*) from timegenie.processed_operations operation
      where operation.workspace_id=p_workspace_id and operation.operation_id=p_operation_id),
    'historyItemCount',(select count(*) from timegenie.cloud_operation_log_items item
      join timegenie.cloud_operation_logs log on log.id=item.operation_log_id
      where log.workspace_id=p_workspace_id and log.operation_id=p_operation_id),
    'historyText',coalesce((select concat_ws(' ',item.before_json::text,item.after_json::text)
      from timegenie.cloud_operation_log_items item
      join timegenie.cloud_operation_logs log on log.id=item.operation_log_id
      where log.workspace_id=p_workspace_id and log.operation_id=p_operation_id),'')
  );
$$;

revoke all on function timegenie.integration_direct_planning_probe(uuid,uuid,date) from public,anon;
revoke all on function timegenie.integration_direct_planning_add_time(uuid,uuid,date) from public,anon;
revoke all on function timegenie.integration_direct_report_probe(uuid,uuid,uuid) from public,anon;
revoke all on function timegenie.integration_direct_report_template_probe(uuid,uuid,uuid) from public,anon;
revoke all on function timegenie.integration_direct_settings_probe(uuid,uuid,text,date,uuid,integer) from public,anon;
revoke all on function timegenie.integration_direct_integration_probe(uuid,uuid,text,text,uuid) from public,anon;
grant execute on function timegenie.integration_direct_planning_probe(uuid,uuid,date) to authenticated;
grant execute on function timegenie.integration_direct_planning_add_time(uuid,uuid,date) to authenticated;
grant execute on function timegenie.integration_direct_report_probe(uuid,uuid,uuid) to authenticated;
grant execute on function timegenie.integration_direct_report_template_probe(uuid,uuid,uuid) to authenticated;
grant execute on function timegenie.integration_direct_settings_probe(uuid,uuid,text,date,uuid,integer) to authenticated;
grant execute on function timegenie.integration_direct_integration_probe(uuid,uuid,text,text,uuid) to authenticated;
do $$ begin
  if (select storage_protocol from timegenie.workspaces where id='20000000-0000-0000-0000-000000000001') is distinct from 'sync_v1' then
    raise exception 'the direct protocol patch must not upgrade existing workspaces';
  end if;
end $$;
set local role authenticated;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000003',true);
select set_config('request.jwt.claim.session_id','80000000-0000-0000-0000-00000000000a',true);
select set_config('request.headers','{"x-timegenie-client-schema-generation":"1"}',true);
select timegenie.device_authorize('20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a','直连协议测试设备','windows','0.4.6');
do $$
declare protocol_value jsonb; context_value jsonb; upgraded jsonb; repeated jsonb;
  operation_value jsonb; operation_request jsonb; preflight_value jsonb; response_value jsonb; old_client_bootstrap jsonb;
  bootstrap_value jsonb; today_overview_value jsonb; expected_work_date text; task_list_value jsonb; task_detail_value jsonb;
  completed_page_one jsonb; completed_page_two jsonb; today_plan_value jsonb; tomorrow_plan_value jsonb;
  today_plan_repeat jsonb; tomorrow_plan_repeat jsonb; time_state_value jsonb; history_one jsonb; history_two jsonb;
  report_page_one jsonb; report_page_two jsonb; report_detail_value jsonb; report_templates_value jsonb; report_preview_value jsonb; weekly_preview_value jsonb;
  report_created jsonb; report_create_retry jsonb; report_scope_updated jsonb; report_saved jsonb; report_save_retry jsonb;
  report_regenerated jsonb; report_deleted jsonb; report_delete_retry jsonb; report_mutation_detail jsonb;
  report_mutation_id uuid; report_snapshot_count integer; report_operation_probe jsonb;
  report_database_probe jsonb;
  template_created jsonb; template_create_retry jsonb; template_operation_probe jsonb; template_probe jsonb;
  template_duplicated jsonb; template_updated jsonb; template_defaulted jsonb; template_reordered jsonb;
  template_deleted jsonb; template_replacement_deleted jsonb;
  template_created_id uuid; template_duplicate_id uuid; template_replacement_id uuid;
  settings_probe jsonb; setting_updated jsonb; settings_batch jsonb; settings_batch_retry jsonb;
  settings_concurrent jsonb; settings_history jsonb; work_day_updated jsonb; work_day_daily jsonb;
  work_day_created jsonb; work_day_failed jsonb; schedule_first jsonb; schedule_stale jsonb;
  schedule_granular jsonb; schedule_failed jsonb; schedule_batch_retry jsonb;
  schedule_day_monday jsonb; schedule_day_sunday jsonb;
  timezone_updated jsonb; timezone_restored jsonb; schedule_batch_payload jsonb; schedule_failed_payload jsonb;
  shared_configuration_value jsonb; shared_configuration_text text;
  integration_config_saved jsonb; integration_config_retry jsonb; integration_state jsonb; integration_state_text text;
  integration_probe jsonb; binding_saved jsonb; binding_retry jsonb; binding_created jsonb;
  binding_deleted jsonb; binding_delete_retry jsonb; plan_import jsonb; plan_import_retry jsonb; plan_import_probe jsonb;
  created_subject jsonb; repeated_subject jsonb; updated_subject jsonb; archived_subject jsonb; restored_subject jsonb;
  created_parent jsonb; created_child jsonb; moved_child jsonb; archived_tree jsonb; restored_tree jsonb;
  planning_parent jsonb; planning_child jsonb; planning_sibling jsonb; planning_other jsonb;
  estimate_first jsonb; estimate_retry jsonb; estimate_batch jsonb; estimate_batch_retry jsonb;
  included_plan jsonb; excluded_plan jsonb; excluded_retry jsonb; excluded_same_state jsonb;
  plan_batch jsonb; plan_batch_retry jsonb;
  recurrence_saved jsonb; recurrence_retry jsonb; recurrence_replaced jsonb;
  recurrence_generated jsonb; recurrence_generated_retry jsonb;
  occurrence_completed jsonb; occurrence_reopened jsonb; recurrence_closed jsonb;
  recurrence_task_version bigint; recurrence_rule_version bigint;
  planning_instance_id uuid; planning_context_id uuid; plan_version bigint;
  planning_probe jsonb; context_probe jsonb;
  status_task jsonb; status_done jsonb; status_retry jsonb; status_reopened jsonb; status_probe jsonb;
  duplicate_parent jsonb; duplicate_child jsonb; duplicate_result jsonb; duplicate_retry jsonb;
  batch_subject jsonb; batch_parent jsonb; batch_removed jsonb; batch_result jsonb; batch_retry jsonb;
  direct_manual_snapshot jsonb; direct_automatic_snapshot jsonb; direct_snapshot_payload jsonb;
  today_update jsonb; today_update_probe jsonb; today_instance_version bigint; today_source_version bigint;
  concurrency_task jsonb;
  mutation_shared_configuration jsonb; mutation_history jsonb; mutation_history_detail jsonb;
  mutation_schedule_count integer; mutation_segment_count integer;
  doc_example_subject jsonb; doc_example_created jsonb; doc_example_list jsonb;
  doc_example_updated jsonb; doc_example_estimate jsonb; doc_example_estimate_retry jsonb;
  doc_example_batch jsonb; doc_example_batch_retry jsonb; doc_example_child_id uuid;
begin
  if (select storage_protocol from timegenie.workspaces where id='20000000-0000-0000-0000-00000000000a') is distinct from 'sync_v1' then
    raise exception 'new workspaces must default to sync_v1';
  end if;
  protocol_value:=timegenie.storage_protocol_get('20000000-0000-0000-0000-00000000000a');
  if protocol_value->>'storageProtocol' is distinct from 'sync_v1'
     or protocol_value ? 'schemaGeneration' then
    raise exception 'storage protocol status is invalid or still exposes client generation: %',protocol_value;
  end if;
  preflight_value:=timegenie.storage_protocol_upgrade_preflight(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a'
  );
  if preflight_value->>'storageProtocol' is distinct from 'sync_v1'
     or not coalesce((preflight_value->>'serverUpgradeAllowed')::boolean,false)
     or not coalesce((preflight_value->>'requiresLocalQueueDisposition')::boolean,false)
     or (preflight_value->>'workspaceEpoch')::bigint<>1 then
    raise exception 'storage protocol upgrade preflight is incomplete: %',preflight_value;
  end if;
  begin
    perform timegenie.upgrade_storage_protocol(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4e000000-0000-0000-0000-000000000010',1,'queue_unknown',false
    );
    raise exception 'protocol upgrade accepted an unknown queue disposition';
  exception when others then
    if sqlerrm not like '%VALIDATION_ERROR%' then raise; end if;
  end;
  begin
    perform timegenie.upgrade_storage_protocol(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4e000000-0000-0000-0000-000000000011',99,'queue_drained',false
    );
    raise exception 'protocol upgrade accepted a stale workspace epoch';
  exception when others then
    if sqlerrm not like '%WORKSPACE_EPOCH_STALE%' then raise; end if;
  end;
  if (select storage_protocol from timegenie.workspaces where id='20000000-0000-0000-0000-00000000000a') is distinct from 'sync_v1' then
    raise exception 'rejected protocol upgrade changed the workspace';
  end if;
  perform timegenie.device_authorize(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-0000000000be',
    '仍运行旧同步版本的设备','windows','0.4.5'
  );
  upgraded:=timegenie.upgrade_storage_protocol(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4e000000-0000-0000-0000-000000000001',1,'queue_drained',false
  );
  repeated:=timegenie.upgrade_storage_protocol(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4e000000-0000-0000-0000-000000000001',1,'queue_drained',false
  );
  if upgraded->>'storageProtocol' is distinct from 'direct_v2' or upgraded is distinct from repeated then
    raise exception 'direct protocol upgrade was not idempotent: % / %',upgraded,repeated;
  end if;
  operation_request:=jsonb_build_object(
    'deviceId','30000000-0000-0000-0000-00000000000a','expectedEpoch',1,
    'oldSyncDisposition','queue_drained','createSafetySnapshot',false
  );
  operation_value:=timegenie.direct_cloud_operation_get(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4e000000-0000-0000-0000-000000000001','storage_protocol_upgrade',operation_request
  );
  if not coalesce((operation_value->>'found')::boolean,false)
     or operation_value->'result' is distinct from upgraded then
    raise exception 'processed operation result was not stable: % / %',operation_value,upgraded;
  end if;

  begin
  created_subject:=timegenie.direct_cloud_subject_create(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4e000000-0000-0000-0000-000000000101',1,
    '{"id":"51000000-0000-0000-0000-0000000000b1","name":"直连新增主体"}'::jsonb
  );
  repeated_subject:=timegenie.direct_cloud_subject_create(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4e000000-0000-0000-0000-000000000101',1,
    '{"id":"51000000-0000-0000-0000-0000000000b1","name":"直连新增主体"}'::jsonb
  );
  if created_subject is distinct from repeated_subject
     or created_subject->'data'->'subject'->>'name' is distinct from '直连新增主体'
     or jsonb_array_length(created_subject->'metadata'->'domains')<>2 then
    raise exception 'direct subject create/default schedules/idempotency are invalid: % / %',created_subject,repeated_subject;
  end if;
  mutation_shared_configuration:=timegenie.direct_cloud_shared_configuration(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',1
  );
  select coalesce(sum(jsonb_array_length(day_value->'segments')),0)
    into mutation_segment_count
  from jsonb_array_elements(mutation_shared_configuration->'data'->'subjectWorkSchedules') subject_schedule
  cross join lateral jsonb_array_elements(subject_schedule->'days') day_value
  where subject_schedule->>'subjectId'='51000000-0000-0000-0000-0000000000b1';
  select jsonb_array_length(subject_schedule->'days') into mutation_schedule_count
  from jsonb_array_elements(mutation_shared_configuration->'data'->'subjectWorkSchedules') subject_schedule
  where subject_schedule->>'subjectId'='51000000-0000-0000-0000-0000000000b1';
  if mutation_schedule_count<>7 or mutation_segment_count<>10 then
    raise exception 'direct subject create did not create the seven-day default schedule: % / %',mutation_schedule_count,mutation_segment_count;
  end if;
  begin
    perform timegenie.direct_cloud_subject_update(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4e000000-0000-0000-0000-000000000102',1,
      '{"id":"51000000-0000-0000-0000-0000000000b1","expectedVersion":99,"name":"陈旧名称"}'::jsonb
    );
    raise exception 'direct subject update accepted a stale version';
  exception when others then if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if; end;
  updated_subject:=timegenie.direct_cloud_subject_update(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4e000000-0000-0000-0000-000000000103',1,
    '{"id":"51000000-0000-0000-0000-0000000000b1","expectedVersion":1,"name":"直连主体改名"}'::jsonb
  );
  perform timegenie.direct_cloud_subject_reorder(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4e000000-0000-0000-0000-000000000104',1,
    jsonb_build_object('id','51000000-0000-0000-0000-0000000000b1','expectedVersion',2,'nextSubjectId','51000000-0000-0000-0000-0000000000a1')
  );
  archived_subject:=timegenie.direct_cloud_subject_archive(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4e000000-0000-0000-0000-000000000105',1,
    '{"id":"51000000-0000-0000-0000-0000000000b1","expectedVersion":3}'::jsonb
  );
  if archived_subject->'data'->'subject'->>'deletedAt' is null then raise exception 'direct subject archive did not soft-delete'; end if;
  restored_subject:=timegenie.direct_cloud_subject_restore(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4e000000-0000-0000-0000-000000000106',1,
    '{"id":"51000000-0000-0000-0000-0000000000b1","expectedVersion":4}'::jsonb
  );
  if restored_subject->'data'->'subject'->>'deletedAt' is not null then raise exception 'direct subject restore did not reactivate'; end if;

  created_parent:=timegenie.direct_cloud_task_create(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4e000000-0000-0000-0000-000000000111',1,
    '{"id":"61000000-0000-0000-0000-0000000000b1","subjectId":"51000000-0000-0000-0000-0000000000b1","title":"直连父事项","estimateMinutes":30}'::jsonb
  );
  created_child:=timegenie.direct_cloud_task_create(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4e000000-0000-0000-0000-000000000112',1,
    '{"id":"61000000-0000-0000-0000-0000000000b2","subjectId":"51000000-0000-0000-0000-0000000000b1","parentId":"61000000-0000-0000-0000-0000000000b1","title":"直连子事项"}'::jsonb
  );
  begin
    perform timegenie.direct_cloud_task_create(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4e000000-0000-0000-0000-000000000113',1,
      '{"id":"61000000-0000-0000-0000-0000000000b3","subjectId":"51000000-0000-0000-0000-0000000000b1","parentId":"61000000-0000-0000-0000-0000000000b2","title":"非法三级"}'::jsonb
    );
    raise exception 'direct task create accepted a third level';
  exception when others then if sqlerrm not like '%TASK_DEPTH_LIMIT%' then raise; end if; end;
  perform timegenie.direct_cloud_task_update(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4e000000-0000-0000-0000-000000000114',1,
    '{"id":"61000000-0000-0000-0000-0000000000b2","expectedVersion":1,"title":"直连子事项改名","note":"跨设备权威备注"}'::jsonb
  );
  moved_child:=timegenie.direct_cloud_task_move(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4e000000-0000-0000-0000-000000000115',1,
    '{"id":"61000000-0000-0000-0000-0000000000b2","expectedVersion":2,"parentId":null,"previousSiblingId":"61000000-0000-0000-0000-0000000000b1"}'::jsonb
  );
  if moved_child->'data'->'task'->>'parentId' is not null then raise exception 'direct task move did not promote child'; end if;
  perform timegenie.direct_cloud_task_move(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4e000000-0000-0000-0000-000000000116',1,
    '{"id":"61000000-0000-0000-0000-0000000000b2","expectedVersion":3,"parentId":"61000000-0000-0000-0000-0000000000b1"}'::jsonb
  );
  archived_tree:=timegenie.direct_cloud_task_archive_subtree(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4e000000-0000-0000-0000-000000000117',1,
    '{"id":"61000000-0000-0000-0000-0000000000b1","expectedVersion":1}'::jsonb
  );
  if jsonb_array_length(archived_tree->'data'->'affectedTaskIds')<>2
     or archived_tree->'data'->'task'->>'deletedAt' is null then
    raise exception 'direct task subtree archive was not atomic: %',archived_tree;
  end if;
  restored_tree:=timegenie.direct_cloud_task_restore_subtree(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4e000000-0000-0000-0000-000000000118',1,
    '{"id":"61000000-0000-0000-0000-0000000000b1","expectedVersion":2}'::jsonb
  );
  if jsonb_array_length(restored_tree->'data'->'affectedTaskIds')<>2
     or restored_tree->'data'->'task'->>'deletedAt' is not null then
    raise exception 'direct subject/task mutation history or subtree restore is invalid: %',restored_tree;
  end if;
  mutation_history:=timegenie.cloud_operation_history_list(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',50,
    null,null,null,null,null,null,'direct_task_restore_subtree',null
  );
  if jsonb_array_length(mutation_history->'items')<>1 then raise exception 'direct task restore history is missing: %',mutation_history; end if;
  mutation_history_detail:=timegenie.cloud_operation_history_detail(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    (mutation_history->'items'->0->>'id')::uuid
  );
  if mutation_history_detail->'items'->0->>'action' is distinct from 'restore' then
    raise exception 'direct task restore history action is invalid: %',mutation_history_detail;
  end if;
  if mutation_history_detail::text~*'(access_token|refresh_token|password|secret_ref|managementKey|baseApiToken)' then
    raise exception 'direct task restore history leaked protected fields: %',mutation_history_detail;
  end if;
  begin
    perform timegenie.direct_cloud_task_update(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4e000000-0000-0000-0000-000000000119',1,
      '{"id":"61000000-0000-0000-0000-0000000000b1","expectedVersion":1,"title":"失败请求不应记录成功历史"}'::jsonb
    );
    raise exception 'stale direct task update was accepted';
  exception when others then if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if; end;
  planning_parent:=timegenie.direct_cloud_task_create(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000101',1,
    '{"id":"62000000-0000-0000-0000-0000000000c1","subjectId":"51000000-0000-0000-0000-0000000000b1","title":"计划父事项"}'::jsonb
  );
  planning_child:=timegenie.direct_cloud_task_create(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000102',1,
    '{"id":"62000000-0000-0000-0000-0000000000c2","subjectId":"51000000-0000-0000-0000-0000000000b1","parentId":"62000000-0000-0000-0000-0000000000c1","title":"计划子事项"}'::jsonb
  );
  planning_sibling:=timegenie.direct_cloud_task_create(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000103',1,
    '{"id":"62000000-0000-0000-0000-0000000000c3","subjectId":"51000000-0000-0000-0000-0000000000b1","title":"计划同级事项"}'::jsonb
  );
  planning_other:=timegenie.direct_cloud_task_create(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000104',1,
    '{"id":"62000000-0000-0000-0000-0000000000c4","subjectId":"51000000-0000-0000-0000-0000000000a2","title":"其他主体事项"}'::jsonb
  );

  estimate_first:=timegenie.direct_cloud_task_daily_estimate_set(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000201',1,
    '{"taskId":"62000000-0000-0000-0000-0000000000c2","workDate":"2026-10-25","estimateMinutes":30,"expectedVersion":null}'::jsonb
  );
  estimate_retry:=timegenie.direct_cloud_task_daily_estimate_set(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000201',1,
    '{"taskId":"62000000-0000-0000-0000-0000000000c2","workDate":"2026-10-25","estimateMinutes":30,"expectedVersion":null}'::jsonb
  );
  if estimate_first is distinct from estimate_retry
     or (estimate_first->'data'->'dailyEstimate'->>'version')::bigint<>1 then
    raise exception 'direct daily estimate retry was not idempotent: % / %',estimate_first,estimate_retry;
  end if;
  perform timegenie.direct_cloud_task_daily_estimate_set(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000202',1,
    '{"taskId":"62000000-0000-0000-0000-0000000000c2","workDate":"2026-10-25","estimateMinutes":45,"expectedVersion":1}'::jsonb
  );
  perform timegenie.direct_cloud_task_daily_estimate_set(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000203',1,
    '{"taskId":"62000000-0000-0000-0000-0000000000c2","workDate":"2026-10-25","estimateMinutes":null,"expectedVersion":2}'::jsonb
  );
  planning_probe:=timegenie.integration_direct_planning_probe(
    '20000000-0000-0000-0000-00000000000a','62000000-0000-0000-0000-0000000000c2','2026-10-25'
  );
  if planning_probe->'estimate'<>'null'::jsonb then
    raise exception 'direct daily estimate clear did not delete the authoritative row';
  end if;
  estimate_batch:=timegenie.direct_cloud_task_daily_estimate_batch_set(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000204',1,
    '{"subjectId":"51000000-0000-0000-0000-0000000000b1","workDate":"2026-10-26","items":[{"taskId":"62000000-0000-0000-0000-0000000000c3","estimateMinutes":40,"expectedTaskVersion":1,"expectedDailyEstimateVersion":null},{"taskId":"62000000-0000-0000-0000-0000000000c2","estimateMinutes":20,"expectedTaskVersion":1,"expectedDailyEstimateVersion":null}]}'::jsonb
  );
  estimate_batch_retry:=timegenie.direct_cloud_task_daily_estimate_batch_set(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000204',1,
    '{"subjectId":"51000000-0000-0000-0000-0000000000b1","workDate":"2026-10-26","items":[{"taskId":"62000000-0000-0000-0000-0000000000c3","estimateMinutes":40,"expectedTaskVersion":1,"expectedDailyEstimateVersion":null},{"taskId":"62000000-0000-0000-0000-0000000000c2","estimateMinutes":20,"expectedTaskVersion":1,"expectedDailyEstimateVersion":null}]}'::jsonb
  );
  if estimate_batch is distinct from estimate_batch_retry
     or (estimate_batch->'data'->>'changedCount')::integer<>2
     or estimate_batch->'data'->'items'->0->>'taskId' is distinct from '62000000-0000-0000-0000-0000000000c3'
     or estimate_batch->'data'->'items'->1->>'taskId' is distinct from '62000000-0000-0000-0000-0000000000c2' then
    raise exception 'direct daily estimate batch order is unstable: %',estimate_batch;
  end if;
  begin
    perform timegenie.direct_cloud_task_daily_estimate_batch_set(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4f000000-0000-0000-0000-000000000205',1,
      '{"subjectId":"51000000-0000-0000-0000-0000000000b1","workDate":"2026-10-27","items":[{"taskId":"62000000-0000-0000-0000-0000000000c2","estimateMinutes":10,"expectedTaskVersion":1,"expectedDailyEstimateVersion":null},{"taskId":"62000000-0000-0000-0000-0000000000c3","estimateMinutes":15,"expectedTaskVersion":99,"expectedDailyEstimateVersion":null}]}'::jsonb
    );
    raise exception 'daily estimate batch accepted a stale later item';
  exception when others then if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if; end;
  planning_probe:=timegenie.integration_direct_planning_probe(
    '20000000-0000-0000-0000-00000000000a','62000000-0000-0000-0000-0000000000c2','2026-10-27'
  );
  if planning_probe->'estimate'<>'null'::jsonb then
    raise exception 'failed daily estimate batch did not roll back its first item';
  end if;
  begin
    perform timegenie.direct_cloud_task_daily_estimate_batch_set(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4f000000-0000-0000-0000-000000000206',1,
      '{"subjectId":"51000000-0000-0000-0000-0000000000b1","workDate":"2026-10-27","items":[{"taskId":"62000000-0000-0000-0000-0000000000c2","estimateMinutes":10,"expectedTaskVersion":1,"expectedDailyEstimateVersion":null},{"taskId":"62000000-0000-0000-0000-0000000000c2","estimateMinutes":15,"expectedTaskVersion":1,"expectedDailyEstimateVersion":null}]}'::jsonb
    );
    raise exception 'daily estimate batch accepted duplicate task ids';
  exception when others then if sqlerrm not like '%VALIDATION_ERROR%' then raise; end if; end;
  begin
    perform timegenie.direct_cloud_task_daily_estimate_batch_set(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4f000000-0000-0000-0000-000000000207',1,
      '{"subjectId":"51000000-0000-0000-0000-0000000000b1","workDate":"2026-10-27","items":[{"taskId":"62000000-0000-0000-0000-0000000000c4","estimateMinutes":10,"expectedTaskVersion":1,"expectedDailyEstimateVersion":null}]}'::jsonb
    );
    raise exception 'daily estimate batch accepted a task from another subject';
  exception when others then if sqlerrm not like '%VALIDATION_ERROR%' then raise; end if; end;

  perform timegenie.direct_cloud_task_daily_estimate_set(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000208',1,
    '{"taskId":"62000000-0000-0000-0000-0000000000c2","workDate":"2026-10-28","estimateMinutes":25,"expectedVersion":null}'::jsonb
  );
  included_plan:=timegenie.direct_cloud_task_day_plan_set(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000301',1,
    '{"taskId":"62000000-0000-0000-0000-0000000000c2","workDate":"2026-10-28","state":"included","expectedVersion":null}'::jsonb
  );
  planning_instance_id:=(included_plan->'data'->'plan'->>'instanceTaskId')::uuid;
  planning_probe:=timegenie.integration_direct_planning_probe(
    '20000000-0000-0000-0000-00000000000a',planning_instance_id,'2026-10-28'
  );
  planning_context_id:=(planning_probe->'task'->>'parentId')::uuid;
  context_probe:=timegenie.integration_direct_planning_probe(
    '20000000-0000-0000-0000-00000000000a',planning_context_id,'2026-10-28'
  );
  if planning_instance_id is null or planning_context_id is null
     or planning_probe->'task'->>'dayInstanceKind' is distinct from 'member'
     or context_probe->'task'->>'dayInstanceKind' is distinct from 'context'
     or (timegenie.integration_direct_planning_probe(
       '20000000-0000-0000-0000-00000000000a','62000000-0000-0000-0000-0000000000c2','2026-10-28'
     )->'estimate')<>'null'::jsonb
     or (planning_probe->'estimate'->>'minutes')::integer<>25 then
    raise exception 'direct included plan did not materialize member/context or move estimate: %',included_plan;
  end if;
  begin
    perform timegenie.direct_cloud_task_daily_estimate_set(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4f000000-0000-0000-0000-000000000209',1,
      jsonb_build_object('taskId',planning_instance_id,'workDate','2026-10-29','estimateMinutes',10,'expectedVersion',null)
    );
    raise exception 'today instance accepted an estimate for another date';
  exception when others then if sqlerrm not like '%VALIDATION_ERROR%' then raise; end if; end;
  perform timegenie.integration_direct_planning_add_time(
    '20000000-0000-0000-0000-00000000000a',planning_instance_id,'2026-10-28'
  );
  excluded_plan:=timegenie.direct_cloud_task_day_plan_set(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000302',1,
    jsonb_build_object('taskId','62000000-0000-0000-0000-0000000000c2','workDate','2026-10-28','state','excluded','expectedVersion',(included_plan->'data'->'plan'->>'version')::bigint)
  );
  excluded_retry:=timegenie.direct_cloud_task_day_plan_set(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000302',1,
    jsonb_build_object('taskId','62000000-0000-0000-0000-0000000000c2','workDate','2026-10-28','state','excluded','expectedVersion',(included_plan->'data'->'plan'->>'version')::bigint)
  );
  planning_probe:=timegenie.integration_direct_planning_probe(
    '20000000-0000-0000-0000-00000000000a',planning_instance_id,'2026-10-28'
  );
  context_probe:=timegenie.integration_direct_planning_probe(
    '20000000-0000-0000-0000-00000000000a',planning_context_id,'2026-10-28'
  );
  if excluded_plan is distinct from excluded_retry
     or planning_probe->'task'->>'deletedAt' is null
     or context_probe->'task'->>'deletedAt' is null
     or planning_probe->'estimate'<>'null'::jsonb
     or (planning_probe->>'allocatedMinutes')::integer<>30 then
    raise exception 'direct excluded plan did not atomically archive instances, clear estimate, preserve time, or retry idempotently: % / %',excluded_plan,excluded_retry;
  end if;
  plan_version:=(excluded_plan->'data'->'plan'->>'version')::bigint;
  excluded_same_state:=timegenie.direct_cloud_task_day_plan_set(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000306',1,
    jsonb_build_object('taskId','62000000-0000-0000-0000-0000000000c2','workDate','2026-10-28','state','excluded','expectedVersion',plan_version)
  );
  if (excluded_same_state->'data'->'plan'->>'version')::bigint<>plan_version then
    raise exception 'same-state day plan operation drifted the business version: %',excluded_same_state;
  end if;
  begin
    perform timegenie.direct_cloud_task_day_plan_set(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4f000000-0000-0000-0000-000000000303',1,
      jsonb_build_object('taskId','62000000-0000-0000-0000-0000000000c2','workDate','2026-10-28','state','included','expectedVersion',plan_version+1)
    );
    raise exception 'direct day plan accepted a stale expected version';
  exception when others then if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if; end;
  plan_batch:=timegenie.direct_cloud_task_day_plan_batch_set(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000304',1,
    jsonb_build_object('subjectId','51000000-0000-0000-0000-0000000000b1','workDate','2026-10-28','items',jsonb_build_array(
      jsonb_build_object('taskId','62000000-0000-0000-0000-0000000000c3','state','included','expectedVersion',null),
      jsonb_build_object('taskId','62000000-0000-0000-0000-0000000000c2','state','included','expectedVersion',plan_version)
    ))
  );
  plan_batch_retry:=timegenie.direct_cloud_task_day_plan_batch_set(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000304',1,
    jsonb_build_object('subjectId','51000000-0000-0000-0000-0000000000b1','workDate','2026-10-28','items',jsonb_build_array(
      jsonb_build_object('taskId','62000000-0000-0000-0000-0000000000c3','state','included','expectedVersion',null),
      jsonb_build_object('taskId','62000000-0000-0000-0000-0000000000c2','state','included','expectedVersion',plan_version)
    ))
  );
  if plan_batch is distinct from plan_batch_retry
     or (plan_batch->'data'->>'changedCount')::integer<>2
     or plan_batch->'data'->'items'->0->>'taskId' is distinct from '62000000-0000-0000-0000-0000000000c3'
     or plan_batch->'data'->'items'->1->>'taskId' is distinct from '62000000-0000-0000-0000-0000000000c2' then
    raise exception 'direct day-plan batch order is unstable: %',plan_batch;
  end if;
  plan_version:=(plan_batch->'data'->'items'->1->'plan'->>'version')::bigint;
  planning_instance_id:=(plan_batch->'data'->'items'->1->'plan'->>'instanceTaskId')::uuid;
  begin
    perform timegenie.direct_cloud_task_day_plan_batch_set(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4f000000-0000-0000-0000-000000000305',1,
      jsonb_build_object('subjectId','51000000-0000-0000-0000-0000000000b1','workDate','2026-10-28','items',jsonb_build_array(
        jsonb_build_object('taskId','62000000-0000-0000-0000-0000000000c2','state','excluded','expectedVersion',plan_version),
        jsonb_build_object('taskId','62000000-0000-0000-0000-0000000000c3','state','excluded','expectedVersion',99)
      ))
    );
    raise exception 'day-plan batch accepted a stale later item';
  exception when others then if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if; end;
  planning_probe:=timegenie.integration_direct_planning_probe(
    '20000000-0000-0000-0000-00000000000a','62000000-0000-0000-0000-0000000000c2','2026-10-28'
  );
  context_probe:=timegenie.integration_direct_planning_probe(
    '20000000-0000-0000-0000-00000000000a',planning_instance_id,'2026-10-28'
  );
  if planning_probe->'plan'->>'state' is distinct from 'included'
     or (planning_probe->'plan'->>'version')::bigint<>plan_version
     or context_probe->'task'->>'deletedAt' is not null then
    raise exception 'failed day-plan batch did not roll back its first item';
  end if;

  recurrence_task_version:=(planning_sibling->'data'->'task'->>'version')::bigint;
  recurrence_saved:=timegenie.direct_cloud_recurrence_save(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000401',1,
    jsonb_build_object('taskId','62000000-0000-0000-0000-0000000000c3',
      'taskExpectedVersion',recurrence_task_version,'frequency','daily',
      'weekdaysMask',null,'effectiveStart',(now() at time zone 'Pacific/Kiritimati')::date,
      'ruleExpectedVersion',null)
  );
  recurrence_retry:=timegenie.direct_cloud_recurrence_save(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000401',1,
    jsonb_build_object('taskId','62000000-0000-0000-0000-0000000000c3',
      'taskExpectedVersion',recurrence_task_version,'frequency','daily',
      'weekdaysMask',null,'effectiveStart',(now() at time zone 'Pacific/Kiritimati')::date,
      'ruleExpectedVersion',null)
  );
  if recurrence_saved is distinct from recurrence_retry
     or recurrence_saved->'data'->'rule'->>'frequency' is distinct from 'daily'
     or (recurrence_saved->'data'->'task'->>'version')::bigint<>recurrence_task_version+1 then
    raise exception 'direct recurrence save was not authoritative or idempotent: % / %',recurrence_saved,recurrence_retry;
  end if;
  recurrence_rule_version:=(recurrence_saved->'data'->'rule'->>'version')::bigint;
  recurrence_replaced:=timegenie.direct_cloud_recurrence_save(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000402',1,
    jsonb_build_object('taskId','62000000-0000-0000-0000-0000000000c3',
      'taskExpectedVersion',recurrence_task_version+1,'frequency','weekly',
      'weekdaysMask',1,'effectiveStart',(now() at time zone 'Pacific/Kiritimati')::date+1,
      'ruleExpectedVersion',recurrence_rule_version)
  );
  if jsonb_array_length(recurrence_replaced->'data'->'rules')<>2
     or recurrence_replaced->'data'->'rules'->0->>'effectiveEnd'
       is distinct from (now() at time zone 'Pacific/Kiritimati')::date::text
     or recurrence_replaced->'data'->'rule'->>'frequency' is distinct from 'weekly' then
    raise exception 'direct recurrence replacement did not preserve rule history: %',recurrence_replaced;
  end if;
  recurrence_task_version:=(recurrence_replaced->'data'->'task'->>'version')::bigint;
  recurrence_rule_version:=(recurrence_replaced->'data'->'rule'->>'version')::bigint;

  recurrence_generated:=timegenie.direct_cloud_recurrence_day_generate(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000403',1,
    jsonb_build_object('subjectId','51000000-0000-0000-0000-0000000000b1',
      'workDate',(now() at time zone 'Pacific/Kiritimati')::date)
  );
  recurrence_generated_retry:=timegenie.direct_cloud_recurrence_day_generate(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000403',1,
    jsonb_build_object('subjectId','51000000-0000-0000-0000-0000000000b1',
      'workDate',(now() at time zone 'Pacific/Kiritimati')::date)
  );
  if recurrence_generated is distinct from recurrence_generated_retry
     or (recurrence_generated->'data'->>'generatedCount')::integer<>1
     or recurrence_generated->'data'->'items'->0->'occurrence'->>'origin' is distinct from 'scheduled'
     or recurrence_generated->'data'->'items'->0->'instance'->>'dayInstanceKind' is distinct from 'member' then
    raise exception 'direct recurrence day generation is invalid or not idempotent: % / %',recurrence_generated,recurrence_generated_retry;
  end if;
  planning_instance_id:=(recurrence_generated->'data'->'items'->0->'plan'->>'instanceTaskId')::uuid;
  occurrence_completed:=timegenie.direct_cloud_recurrence_occurrence_set_status(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000404',1,
    jsonb_build_object('taskId','62000000-0000-0000-0000-0000000000c3',
      'occurrenceDate',(now() at time zone 'Pacific/Kiritimati')::date,'done',true,
      'taskExpectedVersion',recurrence_task_version,'occurrenceExpectedVersion',1,
      'instanceExpectedVersion',1)
  );
  if occurrence_completed->'data'->'occurrence'->>'status' is distinct from 'done'
     or occurrence_completed->'data'->'instance'->>'status' is distinct from 'done'
     or occurrence_completed->'data'->'task'->>'status' is distinct from 'open' then
    raise exception 'direct recurrence completion did not atomically advance only the daily occurrence: %',occurrence_completed;
  end if;
  occurrence_reopened:=timegenie.direct_cloud_recurrence_occurrence_set_status(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000405',1,
    jsonb_build_object('taskId','62000000-0000-0000-0000-0000000000c3',
      'occurrenceDate',(now() at time zone 'Pacific/Kiritimati')::date,'done',false,
      'taskExpectedVersion',recurrence_task_version,'occurrenceExpectedVersion',2,
      'instanceExpectedVersion',2)
  );
  if occurrence_reopened->'data'->'occurrence'->>'status' is distinct from 'open'
     or occurrence_reopened->'data'->'instance'->>'status' is distinct from 'open' then
    raise exception 'direct recurrence reopen did not restore the daily occurrence: %',occurrence_reopened;
  end if;
  begin
    perform timegenie.direct_cloud_recurrence_occurrence_set_status(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4f000000-0000-0000-0000-000000000406',1,
      jsonb_build_object('taskId','62000000-0000-0000-0000-0000000000c3',
        'occurrenceDate',(now() at time zone 'Pacific/Kiritimati')::date,'done',true,
        'taskExpectedVersion',recurrence_task_version,'occurrenceExpectedVersion',99,
        'instanceExpectedVersion',3)
    );
    raise exception 'direct recurrence completion accepted a stale occurrence version';
  exception when others then if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if; end;

  status_task:=timegenie.direct_cloud_task_create(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000501',1,
    '{"id":"63000000-0000-0000-0000-0000000000d1","subjectId":"51000000-0000-0000-0000-0000000000b1","title":"状态事项"}'::jsonb
  );
  status_done:=timegenie.direct_cloud_task_set_status(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000502',1,
    '{"id":"63000000-0000-0000-0000-0000000000d1","expectedVersion":1,"done":true}'::jsonb
  );
  status_retry:=timegenie.direct_cloud_task_set_status(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000502',1,
    '{"id":"63000000-0000-0000-0000-0000000000d1","expectedVersion":1,"done":true}'::jsonb
  );
  status_probe:=timegenie.integration_direct_planning_probe(
    '20000000-0000-0000-0000-00000000000a','63000000-0000-0000-0000-0000000000d1','2026-10-25'
  );
  if status_done is distinct from status_retry
     or status_done->'data'->'task'->>'status' is distinct from 'done'
     or (status_probe->'task'->>'version')::bigint<>2
     or (status_probe->>'statusEventCount')::integer<>1 then
    raise exception 'direct task completion was not atomic or idempotent: % / %',status_done,status_probe;
  end if;
  status_reopened:=timegenie.direct_cloud_task_set_status(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000503',1,
    '{"id":"63000000-0000-0000-0000-0000000000d1","expectedVersion":2,"done":false}'::jsonb
  );
  status_probe:=timegenie.integration_direct_planning_probe(
    '20000000-0000-0000-0000-00000000000a','63000000-0000-0000-0000-0000000000d1','2026-10-25'
  );
  if status_reopened->'data'->'task'->>'status' is distinct from 'open'
     or (status_probe->'task'->>'version')::bigint<>3
     or (status_probe->>'statusEventCount')::integer<>2 then
    raise exception 'direct task reopen did not append its status event: %',status_probe;
  end if;
  context_probe:=timegenie.integration_direct_planning_probe(
    '20000000-0000-0000-0000-00000000000a',planning_context_id,(now() at time zone 'Pacific/Kiritimati')::date
  );
  begin
    perform timegenie.direct_cloud_task_set_status(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4f000000-0000-0000-0000-000000000504',1,
      jsonb_build_object('id',planning_context_id,'expectedVersion',(context_probe->'task'->>'version')::bigint,'done',true)
    );
    raise exception 'direct context instance accepted completion';
  exception when others then
    if sqlerrm not like '%VALIDATION_ERROR%' and sqlerrm not like '%ENTITY_DELETED%' then raise; end if;
  end;
  begin
    perform timegenie.direct_cloud_task_set_status(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4f000000-0000-0000-0000-000000000505',1,
      '{"id":"63000000-0000-0000-0000-0000000000d1","expectedVersion":1,"done":true}'::jsonb
    );
    raise exception 'direct task completion accepted a stale version';
  exception when others then if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if; end;
  status_probe:=timegenie.integration_direct_planning_probe(
    '20000000-0000-0000-0000-00000000000a','63000000-0000-0000-0000-0000000000d1','2026-10-25'
  );
  if status_probe->'task'->>'status' is distinct from 'open'
     or (status_probe->'task'->>'version')::bigint<>3 then
    raise exception 'stale task completion changed the authoritative task: %',status_probe;
  end if;

  duplicate_parent:=timegenie.direct_cloud_task_create(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000511',1,
    '{"id":"63000000-0000-0000-0000-0000000000e1","subjectId":"51000000-0000-0000-0000-0000000000b1","title":"复制父事项"}'::jsonb
  );
  duplicate_child:=timegenie.direct_cloud_task_create(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000512',1,
    '{"id":"63000000-0000-0000-0000-0000000000e2","subjectId":"51000000-0000-0000-0000-0000000000b1","parentId":"63000000-0000-0000-0000-0000000000e1","title":"复制子事项"}'::jsonb
  );
  duplicate_result:=timegenie.direct_cloud_task_duplicate_subtree(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000513',1,
    '{"id":"63000000-0000-0000-0000-0000000000e1","expectedVersion":1}'::jsonb
  );
  duplicate_retry:=timegenie.direct_cloud_task_duplicate_subtree(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000513',1,
    '{"id":"63000000-0000-0000-0000-0000000000e1","expectedVersion":1}'::jsonb
  );
  if duplicate_result is distinct from duplicate_retry
     or jsonb_array_length(duplicate_result->'data'->'tasks')<>2
     or not exists(
       select 1 from jsonb_array_elements(duplicate_result->'data'->'tasks') task_value
       where task_value->>'id'=duplicate_result->'data'->>'rootId'
         and task_value->>'title'='复制父事项 - 副本'
     )
     or not exists(
       select 1 from jsonb_array_elements(duplicate_result->'data'->'tasks') task_value
       where task_value->>'parentId'=duplicate_result->'data'->>'rootId'
         and task_value->>'title'='复制子事项'
     ) then
    raise exception 'direct task subtree duplicate lost hierarchy or idempotency: % / %',duplicate_result,duplicate_retry;
  end if;

  today_update_probe:=timegenie.integration_direct_planning_probe(
    '20000000-0000-0000-0000-00000000000a','62000000-0000-0000-0000-0000000000c3',(now() at time zone 'Pacific/Kiritimati')::date
  );
  today_source_version:=(today_update_probe->'task'->>'version')::bigint;
  today_update_probe:=timegenie.integration_direct_planning_probe(
    '20000000-0000-0000-0000-00000000000a',planning_instance_id,(now() at time zone 'Pacific/Kiritimati')::date
  );
  today_instance_version:=(today_update_probe->'task'->>'version')::bigint;
  today_update:=timegenie.direct_cloud_today_recurring_update(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000521',1,
    jsonb_build_object('instanceId',planning_instance_id,
      'instanceExpectedVersion',today_instance_version,
      'sourceExpectedVersion',today_source_version,
      'title','今日重复事项改名','note','实例与来源原子更新')
  );
  if today_update->'data'->'instance'->>'title' is distinct from '今日重复事项改名'
     or today_update->'data'->'source'->>'title' is distinct from '今日重复事项改名'
     or (today_update->'data'->'instance'->>'version')::bigint<>today_instance_version+1
     or (today_update->'data'->'source'->>'version')::bigint<>today_source_version+1 then
    raise exception 'today recurring update did not commit instance and source atomically: %',today_update;
  end if;
  begin
    perform timegenie.direct_cloud_today_recurring_update(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4f000000-0000-0000-0000-000000000522',1,
      jsonb_build_object('instanceId',planning_instance_id,
        'instanceExpectedVersion',today_instance_version+1,
        'sourceExpectedVersion',today_source_version,
        'title','不应写入')
    );
    raise exception 'today recurring update accepted a stale source version';
  exception when others then if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if; end;
  today_update_probe:=timegenie.integration_direct_planning_probe(
    '20000000-0000-0000-0000-00000000000a',planning_instance_id,(now() at time zone 'Pacific/Kiritimati')::date
  );
  if today_update_probe->'task'->>'title' is distinct from '今日重复事项改名'
     or (today_update_probe->'task'->>'version')::bigint<>today_instance_version+1 then
    raise exception 'stale today recurring update changed the instance: %',today_update_probe;
  end if;
  recurrence_task_version:=today_source_version+1;

  batch_subject:=timegenie.direct_cloud_subject_create(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000531',1,
    '{"id":"52000000-0000-0000-0000-0000000000b2","name":"批量计划主体"}'::jsonb
  );
  batch_parent:=timegenie.direct_cloud_task_create(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000532',1,
    '{"id":"63000000-0000-0000-0000-0000000000f1","subjectId":"52000000-0000-0000-0000-0000000000b2","title":"批量父事项","plannedDate":"2026-10-30"}'::jsonb
  );
  batch_removed:=timegenie.direct_cloud_task_create(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000533',1,
    '{"id":"63000000-0000-0000-0000-0000000000f2","subjectId":"52000000-0000-0000-0000-0000000000b2","title":"批量删除事项","plannedDate":"2026-10-31"}'::jsonb
  );
  batch_result:=timegenie.direct_cloud_task_plan_batch_apply(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000534',1,
    jsonb_build_object(
      'subjectId','52000000-0000-0000-0000-0000000000b2',
      'baselineTasks',jsonb_build_array(
        jsonb_build_object('taskId','63000000-0000-0000-0000-0000000000f1','expectedVersion',1),
        jsonb_build_object('taskId','63000000-0000-0000-0000-0000000000f2','expectedVersion',1)
      ),
      'finalItems',jsonb_build_array(
        jsonb_build_object('lineKey','keep','taskId','63000000-0000-0000-0000-0000000000f1','title','批量父事项改名'),
        jsonb_build_object('lineKey','new','title','批量新增子事项','parentLineKey','keep')
      ),
      'confirmedDeleteTaskIds',jsonb_build_array('63000000-0000-0000-0000-0000000000f2'),
      'todayPlanChange',jsonb_build_object('workDate','2026-10-30','includeLineKeys',jsonb_build_array('keep'))
    )
  );
  batch_retry:=timegenie.direct_cloud_task_plan_batch_apply(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000534',1,
    jsonb_build_object(
      'subjectId','52000000-0000-0000-0000-0000000000b2',
      'baselineTasks',jsonb_build_array(
        jsonb_build_object('taskId','63000000-0000-0000-0000-0000000000f1','expectedVersion',1),
        jsonb_build_object('taskId','63000000-0000-0000-0000-0000000000f2','expectedVersion',1)
      ),
      'finalItems',jsonb_build_array(
        jsonb_build_object('lineKey','keep','taskId','63000000-0000-0000-0000-0000000000f1','title','批量父事项改名'),
        jsonb_build_object('lineKey','new','title','批量新增子事项','parentLineKey','keep')
      ),
      'confirmedDeleteTaskIds',jsonb_build_array('63000000-0000-0000-0000-0000000000f2'),
      'todayPlanChange',jsonb_build_object('workDate','2026-10-30','includeLineKeys',jsonb_build_array('keep'))
    )
  );
  if batch_result is distinct from batch_retry
     or (batch_result->'data'->>'createdCount')::integer<>1
     or (batch_result->'data'->>'updatedCount')::integer<>1
     or (batch_result->'data'->>'deletedCount')::integer<>1
     or (batch_result->'data'->>'todayPlanChangedCount')::integer<>1 then
    raise exception 'direct plan batch was not atomic or idempotent: % / %',batch_result,batch_retry;
  end if;
  task_list_value:=timegenie.direct_cloud_task_list(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    1,'52000000-0000-0000-0000-0000000000b2',true,null,'2026-10-30'
  );
  if jsonb_array_length(task_list_value->'data'->'tasks')<>1
     or task_list_value->'data'->'tasks'->0->>'id' is distinct from '63000000-0000-0000-0000-0000000000f1' then
    raise exception 'direct task list did not apply planned-date filtering: %',task_list_value;
  end if;
  begin
    perform timegenie.direct_cloud_task_plan_batch_apply(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4f000000-0000-0000-0000-000000000535',1,
      jsonb_build_object(
        'subjectId','52000000-0000-0000-0000-0000000000b2',
        'baselineTasks',jsonb_build_array(
          jsonb_build_object('taskId','63000000-0000-0000-0000-0000000000f1','expectedVersion',1),
          jsonb_build_object('taskId',batch_result->'data'->'createdItems'->0->>'taskId','expectedVersion',1)
        ),
        'finalItems',jsonb_build_array(
          jsonb_build_object('lineKey','keep','taskId','63000000-0000-0000-0000-0000000000f1','title','批量父事项改名')
        ),
        'confirmedDeleteTaskIds',jsonb_build_array(batch_result->'data'->'createdItems'->0->>'taskId')
      )
    );
    raise exception 'direct plan batch accepted a stale baseline version';
  exception when others then if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if; end;
  begin
    perform timegenie.direct_cloud_task_plan_batch_apply(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4f000000-0000-0000-0000-000000000536',1,
      jsonb_build_object(
        'subjectId','52000000-0000-0000-0000-0000000000b2',
        'baselineTasks',jsonb_build_array(
          jsonb_build_object('taskId','63000000-0000-0000-0000-0000000000f1','expectedVersion',2),
          jsonb_build_object('taskId',batch_result->'data'->'createdItems'->0->>'taskId','expectedVersion',1)
        ),
        'finalItems',jsonb_build_array(
          jsonb_build_object('lineKey','keep','taskId','63000000-0000-0000-0000-0000000000f1','title','不应写入')
        ),
        'confirmedDeleteTaskIds','[]'::jsonb
      )
    );
    raise exception 'direct plan batch accepted an incomplete delete confirmation';
  exception when others then if sqlerrm not like '%VALIDATION_ERROR%' then raise; end if; end;
  planning_probe:=timegenie.integration_direct_planning_probe(
    '20000000-0000-0000-0000-00000000000a','63000000-0000-0000-0000-0000000000f1','2026-10-30'
  );
  if planning_probe->'task'->>'title' is distinct from '批量父事项改名'
     or (planning_probe->'task'->>'version')::bigint<>2 then
    raise exception 'failed plan batch did not roll back its earlier update: %',planning_probe;
  end if;

  perform timegenie.device_authorize(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-0000000000bc',
    '直连并发设备 B','windows','0.4.6'
  );
  concurrency_task:=timegenie.direct_cloud_task_create(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000541',1,
    '{"id":"63000000-0000-0000-0000-000000000101","subjectId":"51000000-0000-0000-0000-0000000000b1","title":"双设备并发事项"}'::jsonb
  );
  perform timegenie.direct_cloud_task_update(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000542',1,
    '{"id":"63000000-0000-0000-0000-000000000101","expectedVersion":1,"title":"设备 A 已保存"}'::jsonb
  );
  begin
    perform timegenie.direct_cloud_task_update(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-0000000000bc',
      '4f000000-0000-0000-0000-000000000543',1,
      '{"id":"63000000-0000-0000-0000-000000000101","expectedVersion":1,"title":"设备 B 陈旧标题"}'::jsonb
    );
    raise exception 'second device accepted a stale task update';
  exception when others then if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if; end;
  begin
    perform timegenie.direct_cloud_task_move(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-0000000000bc',
      '4f000000-0000-0000-0000-000000000544',1,
      '{"id":"63000000-0000-0000-0000-000000000101","expectedVersion":1,"parentId":null,"sortOrder":99}'::jsonb
    );
    raise exception 'second device accepted a stale task reorder';
  exception when others then if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if; end;
  begin
    perform timegenie.direct_cloud_task_archive_subtree(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-0000000000bc',
      '4f000000-0000-0000-0000-000000000545',1,
      '{"id":"63000000-0000-0000-0000-000000000101","expectedVersion":1}'::jsonb
    );
    raise exception 'second device accepted a stale task delete';
  exception when others then if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if; end;
  task_list_value:=timegenie.direct_cloud_task_list(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-0000000000bc',
    1,'51000000-0000-0000-0000-0000000000b1',true,null,null
  );
  if not exists(
    select 1 from jsonb_array_elements(task_list_value->'data'->'tasks') task_value
    where task_value->>'id'='63000000-0000-0000-0000-000000000101'
      and task_value->>'title'='设备 A 已保存'
      and (task_value->>'version')::bigint=2
  ) then
    raise exception 'second device could not reload the latest authoritative task: %',task_list_value;
  end if;
  direct_manual_snapshot:=timegenie.workspace_snapshot_create(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000546','manual','直连权威手动快照',
    '刚完成的直连 mutation 无需设备同步即可进入快照',false
  );
  direct_automatic_snapshot:=timegenie.workspace_snapshot_create(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-0000000000bc',
    '4f000000-0000-0000-0000-000000000547','automatic','直连权威自动快照',
    '由服务端直接读取权威云端数据',false
  );
  reset role;
  direct_snapshot_payload:=timegenie.workspace_snapshot_rehydrate((direct_manual_snapshot->>'id')::uuid);
  if direct_manual_snapshot->>'status' is distinct from 'ready'
     or direct_automatic_snapshot->>'status' is distinct from 'ready'
     or not exists(
       select 1 from jsonb_array_elements(direct_snapshot_payload->'planning'->'tasks') task_value
       where task_value->>'id'='63000000-0000-0000-0000-000000000101'
         and task_value->>'title'='设备 A 已保存'
         and (task_value->>'version')::bigint=2
     ) then
    raise exception 'direct manual/automatic snapshot did not capture authoritative cloud data: % / %',
      direct_manual_snapshot,direct_automatic_snapshot;
  end if;
  set role authenticated;

  recurrence_closed:=timegenie.direct_cloud_recurrence_close(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000407',1,
    jsonb_build_object('taskId','62000000-0000-0000-0000-0000000000c3',
      'taskExpectedVersion',recurrence_task_version,
      'effectiveEnd',(now() at time zone 'Pacific/Kiritimati')::date,
      'ruleExpectedVersion',recurrence_rule_version)
  );
  if jsonb_array_length(recurrence_closed->'data'->'rules')<>1
     or recurrence_closed->'data'->'rules'->0->>'frequency' is distinct from 'daily' then
    raise exception 'closing a future recurrence rule did not remove it while keeping history: %',recurrence_closed;
  end if;

  -- 文档示例请求：这些 payload、实体 ID 和 operation ID 与
  -- docs/direct-cloud-storage-protocol.md 的“可执行示例”保持一致。
  doc_example_subject:=timegenie.direct_cloud_subject_create(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000601',1,
    '{"id":"53000000-0000-0000-0000-0000000000c1","name":"文档示例主体"}'::jsonb
  );
  doc_example_created:=timegenie.direct_cloud_task_create(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000602',1,
    '{"id":"64000000-0000-0000-0000-0000000000d1","subjectId":"53000000-0000-0000-0000-0000000000c1","title":"文档示例事项","plannedDate":"2026-11-02","estimateMinutes":30}'::jsonb
  );
  doc_example_list:=timegenie.direct_cloud_task_list(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    1,'53000000-0000-0000-0000-0000000000c1',false,null,'2026-11-02'
  );
  if doc_example_subject->'data'->'subject'->>'name' is distinct from '文档示例主体'
     or doc_example_created->'data'->'task'->>'title' is distinct from '文档示例事项'
     or (doc_example_created->'data'->'task'->>'version')::bigint<>1
     or jsonb_array_length(doc_example_list->'data'->'tasks')<>1
     or doc_example_list->'data'->'tasks'->0->>'id' is distinct from '64000000-0000-0000-0000-0000000000d1' then
    raise exception 'documented task create/query example is invalid: % / %',doc_example_created,doc_example_list;
  end if;
  doc_example_updated:=timegenie.direct_cloud_task_update(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000603',1,
    '{"id":"64000000-0000-0000-0000-0000000000d1","expectedVersion":1,"title":"文档示例事项（已更新）"}'::jsonb
  );
  begin
    perform timegenie.direct_cloud_task_update(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4f000000-0000-0000-0000-000000000604',1,
      '{"id":"64000000-0000-0000-0000-0000000000d1","expectedVersion":1,"title":"不应写入"}'::jsonb
    );
    raise exception 'documented stale update example was accepted';
  exception when others then
    if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if;
  end;
  doc_example_list:=timegenie.direct_cloud_task_list(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    1,'53000000-0000-0000-0000-0000000000c1',false,null,'2026-11-02'
  );
  if (doc_example_updated->'data'->'task'->>'version')::bigint<>2
     or doc_example_list->'data'->'tasks'->0->>'title' is distinct from '文档示例事项（已更新）'
     or (doc_example_list->'data'->'tasks'->0->>'version')::bigint<>2 then
    raise exception 'documented stale update did not preserve the authoritative version: % / %',doc_example_updated,doc_example_list;
  end if;
  doc_example_estimate:=timegenie.direct_cloud_task_daily_estimate_set(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000605',1,
    '{"taskId":"64000000-0000-0000-0000-0000000000d1","workDate":"2026-11-02","estimateMinutes":45,"expectedVersion":null}'::jsonb
  );
  doc_example_estimate_retry:=timegenie.direct_cloud_task_daily_estimate_set(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000605',1,
    '{"taskId":"64000000-0000-0000-0000-0000000000d1","workDate":"2026-11-02","estimateMinutes":45,"expectedVersion":null}'::jsonb
  );
  doc_example_batch:=timegenie.direct_cloud_task_plan_batch_apply(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000606',1,
    '{"subjectId":"53000000-0000-0000-0000-0000000000c1","baselineTasks":[{"taskId":"64000000-0000-0000-0000-0000000000d1","expectedVersion":2}],"finalItems":[{"lineKey":"keep","taskId":"64000000-0000-0000-0000-0000000000d1","title":"文档示例事项（已更新）"},{"lineKey":"child","title":"文档示例子事项","parentLineKey":"keep"}],"confirmedDeleteTaskIds":[],"todayPlanChange":{"workDate":"2026-11-02","includeLineKeys":["keep"],"excludeItems":[]}}'::jsonb
  );
  doc_example_batch_retry:=timegenie.direct_cloud_task_plan_batch_apply(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000606',1,
    '{"subjectId":"53000000-0000-0000-0000-0000000000c1","baselineTasks":[{"taskId":"64000000-0000-0000-0000-0000000000d1","expectedVersion":2}],"finalItems":[{"lineKey":"keep","taskId":"64000000-0000-0000-0000-0000000000d1","title":"文档示例事项（已更新）"},{"lineKey":"child","title":"文档示例子事项","parentLineKey":"keep"}],"confirmedDeleteTaskIds":[],"todayPlanChange":{"workDate":"2026-11-02","includeLineKeys":["keep"],"excludeItems":[]}}'::jsonb
  );
  doc_example_child_id:=(doc_example_batch->'data'->'createdItems'->0->>'taskId')::uuid;
  planning_probe:=timegenie.integration_direct_planning_probe(
    '20000000-0000-0000-0000-00000000000a',doc_example_child_id,'2026-11-02'
  );
  if doc_example_estimate is distinct from doc_example_estimate_retry
     or (doc_example_estimate->'data'->'dailyEstimate'->>'version')::bigint<>1
     or doc_example_batch is distinct from doc_example_batch_retry
     or (doc_example_batch->'data'->>'createdCount')::integer<>1
     or (doc_example_batch->'data'->>'updatedCount')::integer<>0
     or (doc_example_batch->'data'->>'deletedCount')::integer<>0
     or (doc_example_batch->'data'->>'todayPlanChangedCount')::integer<>1
     or planning_probe->'task'->>'parentId' is distinct from '64000000-0000-0000-0000-0000000000d1' then
    raise exception 'documented estimate/batch example is not atomic or idempotent: % / % / %',doc_example_estimate,doc_example_batch,planning_probe;
  end if;
  raise exception 'DIRECT_MUTATION_TEST_ROLLBACK';
  exception when others then
    if sqlerrm is distinct from 'DIRECT_MUTATION_TEST_ROLLBACK' then raise; end if;
  end;
  begin
    perform timegenie.direct_cloud_operation_get(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4e000000-0000-0000-0000-000000000001','storage_protocol_upgrade',
      operation_request||jsonb_build_object('expectedEpoch',2)
    );
    raise exception 'operation id accepted a different request';
  exception when others then
    if sqlerrm not like '%IDEMPOTENCY_CONFLICT%' then raise; end if;
  end;
  context_value:=timegenie.direct_cloud_context(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',1
  );
  if context_value->>'storageProtocol' is distinct from 'direct_v2'
     or context_value ? 'schemaGeneration' then
    raise exception 'direct cloud context is invalid: %',context_value;
  end if;
  response_value:=timegenie.direct_cloud_query_response(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    jsonb_build_object('probe','中文查询结果'),array['tasks','time'],1
  );
  if response_value->'metadata'->>'workspaceId' is distinct from '20000000-0000-0000-0000-00000000000a'
     or response_value->'metadata'->>'deviceId' is distinct from '30000000-0000-0000-0000-00000000000a'
     or response_value->'metadata'->>'storageProtocol' is distinct from 'direct_v2'
     or jsonb_typeof(response_value->'metadata'->'workspaceEpoch') is distinct from 'number'
     or jsonb_typeof(response_value->'metadata'->'changeSeq') is distinct from 'number'
     or response_value->'metadata' ? 'schemaGeneration'
     or jsonb_typeof(response_value->'metadata'->'serverTime') is distinct from 'string'
     or response_value->'metadata'->'domains' is distinct from '["tasks","time"]'::jsonb
     or (response_value->'metadata'->'diagnostics'->>'responseBytes')::integer<=0
     or (response_value->'metadata'->'diagnostics'->>'rpcCount')::integer<>1
     or (response_value->'metadata'->'diagnostics'->>'durationMs')::numeric<0
     or response_value->'data'->>'probe' is distinct from '中文查询结果' then
    raise exception 'direct query metadata contract is unstable: %',response_value;
  end if;
  bootstrap_value:=timegenie.direct_cloud_workbench_bootstrap(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',1
  );
  expected_work_date:=(now() at time zone 'Pacific/Kiritimati')::date::text;
  if bootstrap_value->'metadata'->>'storageProtocol' is distinct from 'direct_v2'
     or bootstrap_value->'data'->>'workspaceId' is distinct from '20000000-0000-0000-0000-00000000000a'
     or bootstrap_value->'data'->>'timezone' is distinct from 'Pacific/Kiritimati'
     or bootstrap_value->'data'->>'currentWorkDate' is distinct from expected_work_date
     or jsonb_typeof(bootstrap_value->'data'->'subjects') is distinct from 'array'
     or jsonb_array_length(bootstrap_value->'data'->'subjects')<>2
     or bootstrap_value->'data'->'subjects'->0->>'id' is distinct from '51000000-0000-0000-0000-0000000000a1'
     or bootstrap_value->'data'->'subjects'->0->>'openTaskCount' is distinct from '4'
     or (bootstrap_value->'data'->'summary'->>'subjectCount')::integer<>jsonb_array_length(bootstrap_value->'data'->'subjects')
     or (bootstrap_value->'data'->'summary'->>'openTaskCount')::integer<>4
     or (bootstrap_value->'data'->'summary'->>'completedTaskCount')::integer<>25
     or (bootstrap_value->'data'->'summary'->>'taskCount')::integer<>29
     or (bootstrap_value->'data'->'currentDayStartAt')::bigint>=(bootstrap_value->'data'->'currentDayEndAt')::bigint
     or bootstrap_value->'data'->'device'->>'id' is distinct from '30000000-0000-0000-0000-00000000000a'
     or not coalesce((bootstrap_value->'data'->'device'->>'writable')::boolean,false) then
    raise exception 'direct workbench bootstrap contract is invalid: %',bootstrap_value;
  end if;
  today_overview_value:=timegenie.direct_cloud_today_overview(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    null,expected_work_date::date,30,14,1
  );
  if today_overview_value->'metadata'->>'storageProtocol' is distinct from 'direct_v2'
     or today_overview_value->'data'->'scope'->>'todayDate' is distinct from expected_work_date
     or jsonb_typeof(today_overview_value->'data'->'revision') is distinct from 'number'
     or jsonb_typeof(today_overview_value->'data'->'days') is distinct from 'array'
     or jsonb_typeof(today_overview_value->'data'->'tasks') is distinct from 'array' then
    raise exception 'direct today overview contract is invalid: %',today_overview_value;
  end if;
  task_list_value:=timegenie.direct_cloud_task_list(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',1,
    '51000000-0000-0000-0000-0000000000a1',false,null
  );
  if jsonb_array_length(task_list_value->'data'->'tasks')<>3
     or task_list_value->'data'->'tasks'->0->>'id' is distinct from '61000000-0000-0000-0000-0000000000a2'
     or task_list_value->'data'->'tasks'->1->>'id' is distinct from '61000000-0000-0000-0000-0000000000a3'
     or task_list_value->'data'->'tasks'->1->>'pathLabel' is distinct from '汇总事项 / 叶子事项'
     or (task_list_value->'data'->'tasks'->1->>'depth')::integer<>1
     or (task_list_value->'data'->'tasks'->0->>'selectable')::boolean
     or not (task_list_value->'data'->'tasks'->1->>'selectable')::boolean
     or (task_list_value->'data'->'tasks'->0->>'totalMinutes')::integer<>90
     or (task_list_value->'data'->'tasks'->1->>'directMinutes')::integer<>90
     or exists(select 1 from jsonb_array_elements(task_list_value->'data'->'tasks') item where item->>'title' in('归档日实例','已删除事项')) then
    raise exception 'direct active task tree contract is invalid: %',task_list_value;
  end if;
  task_list_value:=timegenie.direct_cloud_task_list(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',1,
    null,true,'叶子'
  );
  if jsonb_array_length(task_list_value->'data'->'tasks')<>1
     or task_list_value->'data'->'tasks'->0->>'title' is distinct from '叶子事项' then
    raise exception 'direct task search filter is invalid: %',task_list_value;
  end if;
  task_detail_value:=timegenie.direct_cloud_task_detail(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '61000000-0000-0000-0000-0000000000a2',1
  );
  if task_detail_value->'data'->'task'->>'title' is distinct from '汇总事项'
     or jsonb_array_length(task_detail_value->'data'->'children')<>1
     or task_detail_value->'data'->'children'->0->>'title' is distinct from '叶子事项' then
    raise exception 'direct task detail contract is invalid: %',task_detail_value;
  end if;
  completed_page_one:=timegenie.direct_cloud_completed_tasks(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',1,
    '51000000-0000-0000-0000-0000000000a1',null,null,20
  );
  completed_page_two:=timegenie.direct_cloud_completed_tasks(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',1,
    '51000000-0000-0000-0000-0000000000a1',
    (completed_page_one->'data'->'nextCursor'->>'completedAt')::timestamptz,
    (completed_page_one->'data'->'nextCursor'->>'id')::uuid,20
  );
  if jsonb_array_length(completed_page_one->'data'->'tasks')<>20
     or not (completed_page_one->'data'->>'hasMore')::boolean
     or jsonb_array_length(completed_page_two->'data'->'tasks')<>5
     or (completed_page_two->'data'->>'hasMore')::boolean then
    raise exception 'direct completed task cursor pagination is invalid: % / %',completed_page_one,completed_page_two;
  end if;
  today_plan_value:=timegenie.direct_cloud_today_plan(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '51000000-0000-0000-0000-0000000000a1',expected_work_date::date,1
  );
  tomorrow_plan_value:=timegenie.direct_cloud_today_plan(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '51000000-0000-0000-0000-0000000000a1',expected_work_date::date+1,1
  );
  today_plan_repeat:=timegenie.direct_cloud_today_plan(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '51000000-0000-0000-0000-0000000000a1',expected_work_date::date,1
  );
  tomorrow_plan_repeat:=timegenie.direct_cloud_today_plan(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '51000000-0000-0000-0000-0000000000a1',expected_work_date::date+1,1
  );
  if today_plan_value->'data'->>'workDate' is distinct from expected_work_date
     or jsonb_array_length(today_plan_value->'data'->'members')<>1
     or today_plan_value->'data'->'members'->0->>'id' is distinct from '61000000-0000-0000-0000-0000000000a3'
     or today_plan_value->'data'->'members'->0->>'todayEstimateMinutes' is distinct from '45'
     or jsonb_array_length(today_plan_value->'data'->'contexts')<>1
     or today_plan_value->'data'->'contexts'->0->>'id' is distinct from '61000000-0000-0000-0000-0000000000a2'
     or jsonb_array_length(today_plan_value->'data'->'recurrenceRules')<>1
     or not (today_plan_value->'data'->'recurrenceRules'->0->>'matchesDate')::boolean
     or today_plan_value->'data'->'occurrences'->0->>'status' is distinct from 'done'
     or jsonb_array_length(today_plan_value->'data'->'instances')<>1
     or jsonb_array_length(tomorrow_plan_value->'data'->'members')<>2
     or jsonb_array_length(tomorrow_plan_value->'data'->'instances')<>0
     or not exists(select 1 from jsonb_array_elements(tomorrow_plan_value->'data'->'members') item where item->>'id'='61000000-0000-0000-0000-0000000000a1') then
    raise exception 'direct today-plan read contract is invalid: % / %',today_plan_value,tomorrow_plan_value;
  end if;
  if today_plan_value->'data' is distinct from today_plan_repeat->'data'
     or tomorrow_plan_value->'data' is distinct from tomorrow_plan_repeat->'data'
     or today_plan_value->'metadata'->'changeSeq' is distinct from today_plan_repeat->'metadata'->'changeSeq'
     or tomorrow_plan_value->'metadata'->'changeSeq' is distinct from tomorrow_plan_repeat->'metadata'->'changeSeq' then
    raise exception 'direct today-plan query performed an implicit business write';
  end if;
  time_state_value:=timegenie.direct_cloud_time_state(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',expected_work_date::date,1
  );
  if time_state_value->'data'->'activeEntry'->>'id' is distinct from '91000000-0000-0000-0000-0000000000a2'
     or time_state_value->'data'->'activeEntry'->>'state' is distinct from 'paused'
     or time_state_value->'data'->'activeEntry'->>'timerChainId' is distinct from '91000000-0000-0000-0000-0000000000aa'
     or time_state_value->'data'->'activeEntry'->>'previousEntryId' is not null
     or jsonb_array_length(time_state_value->'data'->'activeEntry'->'segments')<>2
     or (time_state_value->'data'->'activeEntry'->>'allocatedMinutes')::integer<>20
     or time_state_value->'data'->'unassigned'->>'sessionId' is distinct from '94000000-0000-0000-0000-0000000000a1'
     or (time_state_value->'data'->'unassigned'->>'elapsedSeconds')::integer<599
     or not (time_state_value->'data'->'unassigned'->>'mustResolve')::boolean then
    raise exception 'direct time state contract is invalid: %',time_state_value;
  end if;
  history_one:=timegenie.direct_cloud_time_history(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',1,null,7,true
  );
  history_two:=timegenie.direct_cloud_time_history(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',1,
    (history_one->'data'->>'nextCursor')::date,7,true
  );
  if jsonb_array_length(history_one->'data'->'groups')<>7
     or not (history_one->'data'->>'hasMore')::boolean
     or jsonb_array_length(history_two->'data'->'groups')<2
     or history_one->'data'->'groups'->0->'entries'->0->>'id' is null
     or not exists(select 1 from jsonb_array_elements(history_one->'data'->'groups') day where jsonb_array_length(day->'adjustments')>0) then
    raise exception 'direct time history cursor contract is invalid: % / %',history_one,history_two;
  end if;
  report_page_one:=timegenie.direct_cloud_report_list(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',1,
    null,'51000000-0000-0000-0000-0000000000a1',null,null,null,20
  );
  report_page_two:=timegenie.direct_cloud_report_list(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',1,
    null,'51000000-0000-0000-0000-0000000000a1',null,
    (report_page_one->'data'->'nextCursor'->>'updatedAt')::timestamptz,
    (report_page_one->'data'->'nextCursor'->>'id')::uuid,20
  );
  if jsonb_array_length(report_page_one->'data'->'reports')<>20
     or not (report_page_one->'data'->>'hasMore')::boolean
     or jsonb_array_length(report_page_two->'data'->'reports')<>6
     or (report_page_two->'data'->>'hasMore')::boolean
     or (report_page_one->'data'->'typeCounts'->>'daily')::integer<>25
     or (report_page_one->'data'->'typeCounts'->>'weekly')::integer<>1 then
    raise exception 'direct report cursor contract is invalid: % / %',report_page_one,report_page_two;
  end if;
  report_detail_value:=timegenie.direct_cloud_report_detail(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    'b1000000-0000-0000-0000-0000000000a1',1
  );
  if report_detail_value->'data'->'report'->>'templateName' is distinct from '已归档日报模板'
     or not (report_detail_value->'data'->'report'->>'templateArchived')::boolean
     or report_detail_value->'data'->'report'->>'markdown' is distinct from '# 已保存日报正文'
     or jsonb_array_length(report_detail_value->'data'->'tasks')<>1
     or not (report_detail_value->'data'->'tasks'->0->>'archived')::boolean then
    raise exception 'direct report detail contract is invalid: %',report_detail_value;
  end if;
  report_templates_value:=timegenie.direct_cloud_report_templates(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',null,1
  );
  if jsonb_array_length(report_templates_value->'data'->'templates')<>3
     or exists(select 1 from jsonb_array_elements(report_templates_value->'data'->'templates') item where item->>'name'='已归档日报模板') then
    raise exception 'direct report template library contract is invalid: %',report_templates_value;
  end if;
  report_preview_value:=timegenie.direct_cloud_report_preview(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a','daily',expected_work_date::date,
    '51000000-0000-0000-0000-0000000000a1','a1000000-0000-0000-0000-0000000000a1',1
  );
  weekly_preview_value:=timegenie.direct_cloud_report_preview(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a','weekly',expected_work_date::date,
    '51000000-0000-0000-0000-0000000000a1','a1000000-0000-0000-0000-0000000000a2',1
  );
  if report_preview_value->'data'->>'markdown' not like '%直连主体一%'
     or report_preview_value->'data'->>'markdown' not like '%叶子事项%'
     or report_preview_value->'data'->>'markdown' like '%{{%'
     or weekly_preview_value->'data'->'context'->>'dailyCollection' not like '%已保存日报正文%'
     or weekly_preview_value->'data'->>'markdown' like '%{{日报合集}}%' then
    raise exception 'direct report preview contract is invalid: % / %',report_preview_value,weekly_preview_value;
  end if;
  report_created:=timegenie.direct_cloud_report_create(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000701',1,
    jsonb_build_object(
      'reportType','daily','referenceDate',expected_work_date::date+40,
      'subjectId','51000000-0000-0000-0000-0000000000a1',
      'taskIds',jsonb_build_array('61000000-0000-0000-0000-0000000000a2','61000000-0000-0000-0000-0000000000a3'),
      'templateId','a1000000-0000-0000-0000-0000000000a1'
    )
  );
  report_create_retry:=timegenie.direct_cloud_report_create(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000701',1,
    jsonb_build_object(
      'reportType','daily','referenceDate',expected_work_date::date+40,
      'subjectId','51000000-0000-0000-0000-0000000000a1',
      'taskIds',jsonb_build_array('61000000-0000-0000-0000-0000000000a2','61000000-0000-0000-0000-0000000000a3'),
      'templateId','a1000000-0000-0000-0000-0000000000a1'
    )
  );
  report_mutation_id:=(report_created->'data'->'report'->>'id')::uuid;
  report_database_probe:=timegenie.integration_direct_report_probe(
    '20000000-0000-0000-0000-00000000000a',report_mutation_id,'4f000000-0000-0000-0000-000000000701'
  );
  report_operation_probe:=timegenie.direct_cloud_operation_get(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000701','direct_report_create',
    jsonb_build_object('expectedEpoch',1,'payload',jsonb_build_object(
      'reportType','daily','referenceDate',expected_work_date::date+40,
      'subjectId','51000000-0000-0000-0000-0000000000a1',
      'taskIds',jsonb_build_array('61000000-0000-0000-0000-0000000000a2','61000000-0000-0000-0000-0000000000a3'),
      'templateId','a1000000-0000-0000-0000-0000000000a1'
    ))
  );
  if report_created is distinct from report_create_retry
     or not coalesce((report_operation_probe->>'found')::boolean,false)
     or report_operation_probe->'result' is distinct from report_created
     or (report_created->'data'->'report'->>'version')::bigint<>1
     or report_created->'data'->'report'->>'contentSource' is distinct from 'generated'
     or nullif(report_created->'data'->'report'->>'inputRevisionHash','') is null
     or jsonb_array_length(report_created->'data'->'report'->'taskIds')<>2
     or (report_database_probe->>'reportCount')::integer<>1
     or (report_database_probe->>'taskCount')::integer<>2
     or (report_database_probe->>'operationCount')::integer<>1 then
    raise exception 'direct report create was not authoritative/idempotent: % / % / %',report_created,report_create_retry,report_operation_probe;
  end if;
  report_snapshot_count:=(report_database_probe->>'taskCount')::integer;
  begin
    perform timegenie.direct_cloud_report_scope_update(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4f000000-0000-0000-0000-000000000702',1,
      jsonb_build_object(
        'reportId',report_mutation_id,'expectedVersion',1,'reportType','weekly',
        'referenceDate',expected_work_date::date+40,'subjectId','51000000-0000-0000-0000-0000000000a1',
        'taskIds',jsonb_build_array('61000000-0000-0000-0000-0000000000a3','61000000-0000-0000-0000-000000000999'),
        'templateId','a1000000-0000-0000-0000-0000000000a2'
      )
    );
    raise exception 'failed report scope update was accepted';
  exception when others then if sqlerrm not like '%NOT_FOUND%' then raise; end if; end;
  report_database_probe:=timegenie.integration_direct_report_probe(
    '20000000-0000-0000-0000-00000000000a',report_mutation_id,'4f000000-0000-0000-0000-000000000702'
  );
  if (report_database_probe->>'version')::bigint<>1
     or (report_database_probe->>'taskCount')::integer<>report_snapshot_count
     or (report_database_probe->>'operationCount')::integer<>0 then
    raise exception 'failed report scope update left partial data';
  end if;
  report_scope_updated:=timegenie.direct_cloud_report_scope_update(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000703',1,
    jsonb_build_object(
      'reportId',report_mutation_id,'expectedVersion',1,'reportType','weekly',
      'referenceDate',expected_work_date::date+40,'subjectId','51000000-0000-0000-0000-0000000000a1',
      'taskIds',jsonb_build_array('61000000-0000-0000-0000-0000000000a3'),
      'templateId','a1000000-0000-0000-0000-0000000000a2'
    )
  );
  if not (report_scope_updated->'data'->>'needsRegeneration')::boolean
     or (report_scope_updated->'data'->'report'->>'version')::bigint<>2
     or report_scope_updated->'data'->'report'->>'markdown' is distinct from report_created->'data'->'report'->>'markdown'
     or jsonb_array_length(report_scope_updated->'data'->'report'->'taskIds')<>1 then
    raise exception 'direct report scope update did not preserve content or replace snapshots atomically: %',report_scope_updated;
  end if;
  perform timegenie.direct_cloud_task_update(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000704',1,
    '{"id":"61000000-0000-0000-0000-0000000000a3","expectedVersion":1,"title":"叶子事项已改名"}'::jsonb
  );
  report_mutation_detail:=timegenie.direct_cloud_report_detail(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',report_mutation_id,1
  );
  if report_mutation_detail->'data'->'tasks'->0->>'title' is distinct from '叶子事项'
     or report_mutation_detail->'data'->'tasks'->0->>'pathLabel' is distinct from '汇总事项 / 叶子事项' then
    raise exception 'historical report task snapshots changed after task rename: %',report_mutation_detail;
  end if;
  report_saved:=timegenie.direct_cloud_report_content_save(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000705',1,
    jsonb_build_object('reportId',report_mutation_id,'expectedVersion',2,'markdown','# 人工编辑报告')
  );
  report_save_retry:=timegenie.direct_cloud_report_content_save(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000705',1,
    jsonb_build_object('reportId',report_mutation_id,'expectedVersion',2,'markdown','# 人工编辑报告')
  );
  if report_saved is distinct from report_save_retry
     or report_saved->'data'->'report'->>'contentSource' is distinct from 'edited'
     or report_saved->'data'->'report'->>'markdown' is distinct from '# 人工编辑报告'
     or (report_saved->'data'->'report'->>'version')::bigint<>3 then
    raise exception 'direct report content save was not versioned/idempotent: % / %',report_saved,report_save_retry;
  end if;
  begin
    perform timegenie.direct_cloud_report_content_save(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4f000000-0000-0000-0000-000000000706',1,
      jsonb_build_object('reportId',report_mutation_id,'expectedVersion',2,'markdown','# 陈旧内容')
    );
    raise exception 'stale report content save was accepted';
  exception when others then if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if; end;
  report_regenerated:=timegenie.direct_cloud_report_regenerate(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000707',1,
    jsonb_build_object('reportId',report_mutation_id,'expectedVersion',3)
  );
  if report_regenerated->'data'->'report'->>'contentSource' is distinct from 'generated'
     or report_regenerated->'data'->'report'->>'markdown'='# 人工编辑报告'
     or (report_regenerated->'data'->'report'->>'generatedCount')::integer<>2
     or (report_regenerated->'data'->'report'->>'version')::bigint<>4
     or nullif(report_regenerated->'data'->'report'->>'inputRevisionHash','') is null then
    raise exception 'direct report regeneration did not restore generated content: %',report_regenerated;
  end if;
  report_mutation_detail:=timegenie.direct_cloud_report_detail(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',report_mutation_id,1
  );
  if report_mutation_detail->'data'->'tasks'->0->>'title' is distinct from '叶子事项' then
    raise exception 'historical report task snapshots changed after task rename: %',report_mutation_detail;
  end if;
  report_deleted:=timegenie.direct_cloud_report_delete(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000708',1,jsonb_build_object('reportId',report_mutation_id,'expectedVersion',4)
  );
  report_delete_retry:=timegenie.direct_cloud_report_delete(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000708',1,jsonb_build_object('reportId',report_mutation_id,'expectedVersion',4)
  );
  report_database_probe:=timegenie.integration_direct_report_probe(
    '20000000-0000-0000-0000-00000000000a',report_mutation_id,'4f000000-0000-0000-0000-000000000708'
  );
  if report_deleted is distinct from report_delete_retry
     or report_deleted->'data'->'report'->>'deletedAt' is null
     or (report_deleted->'data'->'report'->>'version')::bigint<>5
     or not (report_database_probe->>'deleted')::boolean
     or (report_database_probe->>'operationCount')::integer<>1 then
    raise exception 'direct report delete was not soft/idempotent: % / %',report_deleted,report_delete_retry;
  end if;
  perform timegenie.device_authorize(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-0000000000bd',
    '直连模板并发设备','windows','0.4.6'
  );
  template_created:=timegenie.direct_cloud_report_template_create(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000721',1,
    jsonb_build_object('reportType','daily','name','设备模板甲','description','复制来源说明',
      'content','# {{姓名}} {{日期}}'||E'\n\n'||'{{今日事项}}')
  );
  template_create_retry:=timegenie.direct_cloud_report_template_create(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000721',1,
    jsonb_build_object('reportType','daily','name','设备模板甲','description','复制来源说明',
      'content','# {{姓名}} {{日期}}'||E'\n\n'||'{{今日事项}}')
  );
  template_created_id:=(template_created->'data'->'template'->>'id')::uuid;
  template_operation_probe:=timegenie.direct_cloud_operation_get(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000721','direct_report_template_create',
    jsonb_build_object('expectedEpoch',1,'payload',jsonb_build_object(
      'reportType','daily','name','设备模板甲','description','复制来源说明',
      'content','# {{姓名}} {{日期}}'||E'\n\n'||'{{今日事项}}'))
  );
  template_probe:=timegenie.integration_direct_report_template_probe(
    '20000000-0000-0000-0000-00000000000a',template_created_id,'4f000000-0000-0000-0000-000000000721'
  );
  if template_created is distinct from template_create_retry
     or not coalesce((template_operation_probe->>'found')::boolean,false)
     or template_operation_probe->'result' is distinct from template_created
     or (template_probe->>'templateCount')::integer<>1
     or (template_probe->>'version')::bigint<>1
     or (template_probe->>'operationCount')::integer<>1 then
    raise exception 'direct report-template create was not authoritative/idempotent: % / % / %',template_created,template_create_retry,template_probe;
  end if;
  begin
    perform timegenie.direct_cloud_report_template_create(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4f000000-0000-0000-0000-000000000722',1,
      '{"reportType":"daily","name":"非法未知占位符","content":"{{未知}}"}'::jsonb
    );
    raise exception 'direct report-template validation accepted invalid placeholders';
  exception when others then if sqlerrm not like '%未知占位符%' then raise; end if; end;
  begin
    perform timegenie.direct_cloud_report_template_create(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4f000000-0000-0000-0000-000000000723',1,
      '{"reportType":"daily","name":"非法类型占位符","content":"{{日期范围}}"}'::jsonb
    );
    raise exception 'direct report-template validation accepted invalid placeholders';
  exception when others then if sqlerrm not like '%不适用于当前报告类型%' then raise; end if; end;
  begin
    perform timegenie.direct_cloud_report_template_create(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4f000000-0000-0000-0000-000000000724',1,
      '{"reportType":"daily","name":"非法未闭合占位符","content":"前缀 {{姓名"}'::jsonb
    );
    raise exception 'direct report-template validation accepted invalid placeholders';
  exception when others then if sqlerrm not like '%占位符未闭合%' then raise; end if; end;
  template_probe:=timegenie.integration_direct_report_template_probe(
    '20000000-0000-0000-0000-00000000000a',template_created_id,'4f000000-0000-0000-0000-000000000722'
  );
  if (template_probe->>'operationCount')::integer<>0 then
    raise exception 'direct report-template validation accepted invalid placeholders';
  end if;
  template_duplicated:=timegenie.direct_cloud_report_template_duplicate(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000725',1,
    jsonb_build_object('templateId',template_created_id,'name','设备模板乙')
  );
  template_duplicate_id:=(template_duplicated->'data'->'template'->>'id')::uuid;
  if template_duplicated->'data'->'template'->>'content' is distinct from template_created->'data'->'template'->>'content'
     or template_duplicated->'data'->'template'->>'description' is distinct from '复制来源说明' then
    raise exception 'direct report-template duplicate did not preserve source content: %',template_duplicated;
  end if;
  template_updated:=timegenie.direct_cloud_report_template_update(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000726',1,
    jsonb_build_object('templateId',template_created_id,'expectedVersion',1,'name','设备模板甲（更新）',
      'description','更新说明','content','# {{日期}}'||E'\n'||'{{明日计划}}','sortOrder',40)
  );
  begin
    perform timegenie.direct_cloud_report_template_update(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-0000000000bd',
      '4f000000-0000-0000-0000-000000000727',1,
      jsonb_build_object('templateId',template_created_id,'expectedVersion',1,'name','陈旧设备覆盖',
        'content','# {{日期}}','sortOrder',50)
    );
    raise exception 'stale report-template update was accepted';
  exception when others then if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if; end;
  template_probe:=timegenie.integration_direct_report_template_probe(
    '20000000-0000-0000-0000-00000000000a',template_created_id,'4f000000-0000-0000-0000-000000000727'
  );
  if (template_updated->'data'->'template'->>'version')::bigint<>2
     or template_probe->>'name' is distinct from '设备模板甲（更新）'
     or (template_probe->>'operationCount')::integer<>0 then
    raise exception 'stale report-template update was accepted';
  end if;
  template_defaulted:=timegenie.direct_cloud_report_template_set_default(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000728',1,
    jsonb_build_object('templateId',template_duplicate_id,'expectedVersion',1)
  );
  template_probe:=timegenie.integration_direct_report_template_probe(
    '20000000-0000-0000-0000-00000000000a',template_duplicate_id,'4f000000-0000-0000-0000-000000000728'
  );
  if not (template_probe->>'isDefault')::boolean or (template_probe->>'defaultTypeCount')::integer<>1 then
    raise exception 'report-template default switch did not keep one default: %',template_probe;
  end if;
  begin
    perform timegenie.direct_cloud_report_template_set_default(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-0000000000bd',
      '4f000000-0000-0000-0000-000000000729',1,
      jsonb_build_object('templateId','a1000000-0000-0000-0000-0000000000a1','expectedVersion',1)
    );
    raise exception 'stale report-template default competition was accepted';
  exception when others then if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if; end;
  template_reordered:=timegenie.direct_cloud_report_template_reorder(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000730',1,jsonb_build_object('reportType','daily','items',jsonb_build_array(
      jsonb_build_object('templateId',template_created_id,'expectedVersion',2),
      jsonb_build_object('templateId','a1000000-0000-0000-0000-0000000000a1','expectedVersion',2),
      jsonb_build_object('templateId',template_duplicate_id,'expectedVersion',2)
    ))
  );
  begin
    perform timegenie.direct_cloud_report_template_reorder(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-0000000000bd',
      '4f000000-0000-0000-0000-000000000731',1,jsonb_build_object('reportType','daily','items',jsonb_build_array(
        jsonb_build_object('templateId',template_duplicate_id,'expectedVersion',3),
        jsonb_build_object('templateId',template_created_id,'expectedVersion',2),
        jsonb_build_object('templateId','a1000000-0000-0000-0000-0000000000a1','expectedVersion',3)
      ))
    );
    raise exception 'failed report-template reorder left partial data';
  exception when others then if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if; end;
  template_probe:=timegenie.integration_direct_report_template_probe(
    '20000000-0000-0000-0000-00000000000a',template_created_id,'4f000000-0000-0000-0000-000000000731'
  );
  if (template_probe->>'version')::bigint<>3 or (template_probe->>'sortOrder')::integer<>10
     or (template_probe->>'operationCount')::integer<>0 then
    raise exception 'failed report-template reorder left partial data: %',template_probe;
  end if;
  template_deleted:=timegenie.direct_cloud_report_template_delete(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000732',1,
    jsonb_build_object('templateId',template_created_id,'expectedVersion',3)
  );
  begin
    perform timegenie.direct_cloud_report_template_delete(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4f000000-0000-0000-0000-000000000733',1,
      jsonb_build_object('templateId',template_duplicate_id,'expectedVersion',3)
    );
    raise exception 'default report-template deletion without replacement was accepted';
  exception when others then if sqlerrm not like '%替代默认模板%' then raise; end if; end;
  template_replacement_deleted:=timegenie.direct_cloud_report_template_delete(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000734',1,
    jsonb_build_object('templateId',template_duplicate_id,'expectedVersion',3,
      'replacementTemplateId','a1000000-0000-0000-0000-0000000000a1')
  );
  template_created:=timegenie.direct_cloud_report_template_create(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000735',1,
    jsonb_build_object('reportType','daily','name','历史模板替代项','content','# {{日期}}')
  );
  template_replacement_id:=(template_created->'data'->'template'->>'id')::uuid;
  template_deleted:=timegenie.direct_cloud_report_template_delete(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000736',1,
    jsonb_build_object('templateId','a1000000-0000-0000-0000-0000000000a1','expectedVersion',4,
      'replacementTemplateId',template_replacement_id)
  );
  template_probe:=timegenie.integration_direct_report_template_probe(
    '20000000-0000-0000-0000-00000000000a','a1000000-0000-0000-0000-0000000000a1','4f000000-0000-0000-0000-000000000736'
  );
  if not (template_probe->>'deleted')::boolean or (template_probe->>'boundReportCount')::integer<1
     or (template_probe->>'operationCount')::integer<>1 then
    raise exception 'report-template deletion changed historical report binding: %',template_probe;
  end if;
  template_probe:=timegenie.integration_direct_report_template_probe(
    '20000000-0000-0000-0000-00000000000a',template_replacement_id,'4f000000-0000-0000-0000-000000000737'
  );
  if not (template_probe->>'isDefault')::boolean or (template_probe->>'defaultTypeCount')::integer<>1 then
    raise exception 'report-template default replacement is invalid: %',template_probe;
  end if;
  begin
    perform timegenie.direct_cloud_report_template_delete(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-0000000000bd',
      '4f000000-0000-0000-0000-000000000737',1,
      jsonb_build_object('templateId',template_replacement_id,'expectedVersion',2)
    );
    raise exception 'last report-template deletion was accepted';
  exception when others then if sqlerrm not like '%至少需要保留一份模板%' then raise; end if; end;
  setting_updated:=timegenie.direct_cloud_shared_setting_update(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000741',1,
    '{"key":"user_name","value":"直连用户-云端","expectedVersion":2}'::jsonb
  );
  settings_batch:=timegenie.direct_cloud_shared_setting_update(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000741',1,
    '{"key":"user_name","value":"直连用户-云端","expectedVersion":2}'::jsonb
  );
  settings_probe:=timegenie.integration_direct_settings_probe(
    '20000000-0000-0000-0000-00000000000a','4f000000-0000-0000-0000-000000000741',
    'user_name',expected_work_date::date,'51000000-0000-0000-0000-0000000000a1',0
  );
  if setting_updated is distinct from settings_batch
     or (settings_probe->>'settingVersion')::bigint<>3
     or settings_probe->'settingValue' is distinct from '"直连用户-云端"'::jsonb
     or (settings_probe->>'operationCount')::integer<>1 then
    raise exception 'direct shared setting update was not authoritative/idempotent: % / %',setting_updated,settings_probe;
  end if;
  begin
    perform timegenie.direct_cloud_shared_setting_update(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-0000000000bd',
      '4f000000-0000-0000-0000-000000000742',1,
      '{"key":"user_name","value":{"token":"must-not-be-stored"},"expectedVersion":3}'::jsonb
    );
    raise exception 'sensitive shared setting was accepted';
  exception when others then if sqlerrm not like '%敏感字段%' then raise; end if; end;
  settings_probe:=timegenie.integration_direct_settings_probe(
    '20000000-0000-0000-0000-00000000000a','4f000000-0000-0000-0000-000000000742',
    'user_name',expected_work_date::date,'51000000-0000-0000-0000-0000000000a1',0
  );
  if (settings_probe->>'settingVersion')::bigint<>3 or (settings_probe->>'operationCount')::integer<>0 then
    raise exception 'sensitive shared setting changed cloud state';
  end if;
  settings_batch:=timegenie.direct_cloud_shared_setting_update(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000755',1,
    '{"key":"subject_salary_hourly_rate:51000000-0000-0000-0000-0000000000a1","value":120,"expectedVersion":0}'::jsonb
  );
  if (settings_batch->'data'->'setting'->>'version')::bigint<>1
     or settings_batch->'data'->'setting'->'value' is distinct from '120'::jsonb then
    raise exception 'subject salary shared setting was not stored authoritatively: %',settings_batch;
  end if;
  begin
    perform timegenie.direct_cloud_shared_setting_update(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4f000000-0000-0000-0000-000000000756',1,
      '{"key":"subject_salary_hourly_rate:51000000-0000-0000-0000-0000000000ff","value":80,"expectedVersion":0}'::jsonb
    );
    raise exception 'subject salary for a missing subject was accepted';
  exception when others then if sqlerrm not like '%主体不存在%' then raise; end if; end;
  begin
    perform timegenie.direct_cloud_shared_setting_update(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-0000000000bd',
      '4f000000-0000-0000-0000-000000000757',1,
      '{"key":"subject_salary_hourly_rate:51000000-0000-0000-0000-0000000000a1","value":90,"expectedVersion":0}'::jsonb
    );
    raise exception 'stale subject salary from another device was accepted';
  exception when others then if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if; end;
  settings_probe:=timegenie.integration_direct_settings_probe(
    '20000000-0000-0000-0000-00000000000a','4f000000-0000-0000-0000-000000000757',
    'subject_salary_hourly_rate:51000000-0000-0000-0000-0000000000a1',expected_work_date::date,
    '51000000-0000-0000-0000-0000000000a1',0
  );
  if (settings_probe->>'settingVersion')::bigint<>1
     or settings_probe->'settingValue' is distinct from '120'::jsonb
     or (settings_probe->>'operationCount')::integer<>0 then
    raise exception 'rejected subject salary changed cloud state: %',settings_probe;
  end if;
  settings_batch:=timegenie.direct_cloud_shared_settings_batch_update(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000743',1,
    jsonb_build_object('items',jsonb_build_array(
      jsonb_build_object('key','user_name','value','直连用户-批量','expectedVersion',3),
      jsonb_build_object('key','report_duration_format','value','hours')
    ))
  );
  settings_batch_retry:=timegenie.direct_cloud_shared_settings_batch_update(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000743',1,
    jsonb_build_object('items',jsonb_build_array(
      jsonb_build_object('key','user_name','value','直连用户-批量','expectedVersion',3),
      jsonb_build_object('key','report_duration_format','value','hours')
    ))
  );
  settings_probe:=timegenie.integration_direct_settings_probe(
    '20000000-0000-0000-0000-00000000000a','4f000000-0000-0000-0000-000000000743',
    'user_name',expected_work_date::date,'51000000-0000-0000-0000-0000000000a1',0
  );
  if settings_batch is distinct from settings_batch_retry
     or (settings_probe->>'settingVersion')::bigint<>4
     or settings_probe->'settingValue' is distinct from '"直连用户-批量"'::jsonb then
    raise exception 'direct shared settings batch was not atomic/idempotent: % / %',settings_batch,settings_batch_retry;
  end if;
  begin
    perform timegenie.direct_cloud_shared_settings_batch_update(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-0000000000bd',
      '4f000000-0000-0000-0000-000000000744',1,
      jsonb_build_object('items',jsonb_build_array(
        jsonb_build_object('key','user_name','value','陈旧设备覆盖','expectedVersion',3)
      ))
    );
    raise exception 'stale shared setting from another device was accepted';
  exception when others then if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if; end;
  timezone_updated:=timegenie.direct_cloud_shared_setting_update(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000745',1,
    '{"key":"timezone","value":"America/New_York","expectedVersion":0}'::jsonb
  );
  timezone_restored:=timegenie.direct_cloud_shared_setting_update(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000746',1,
    '{"key":"timezone","value":"Pacific/Kiritimati","expectedVersion":1}'::jsonb
  );
  settings_probe:=timegenie.integration_direct_settings_probe(
    '20000000-0000-0000-0000-00000000000a','4f000000-0000-0000-0000-000000000746',
    'timezone',expected_work_date::date,'51000000-0000-0000-0000-0000000000a1',0
  );
  if settings_probe->>'workspaceTimezone' is distinct from 'Pacific/Kiritimati'
     or (settings_probe->>'settingVersion')::bigint<>2
     or (settings_probe->>'historyItemCount')::integer<1 then
    raise exception 'direct workspace timezone setting was not synchronized safely: %',settings_probe;
  end if;
  begin
    perform timegenie.direct_cloud_shared_settings_batch_update(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4f000000-0000-0000-0000-000000000747',1,
      jsonb_build_object('items',jsonb_build_array(
        jsonb_build_object('key','user_name','value','不应保留的设置','expectedVersion',4),
        jsonb_build_object('key','timezone','value','Mars/Olympus','expectedVersion',2)
      ))
    );
    raise exception 'invalid shared settings batch was accepted';
  exception when others then if sqlerrm not like '%无效的工作空间时区%' then raise; end if; end;
  settings_probe:=timegenie.integration_direct_settings_probe(
    '20000000-0000-0000-0000-00000000000a','4f000000-0000-0000-0000-000000000747',
    'user_name',expected_work_date::date,'51000000-0000-0000-0000-0000000000a1',0
  );
  if (settings_probe->>'settingVersion')::bigint<>4
     or settings_probe->'settingValue' is distinct from '"直连用户-批量"'::jsonb
     or settings_probe->>'workspaceTimezone' is distinct from 'Pacific/Kiritimati'
     or (settings_probe->>'operationCount')::integer<>0 then
    raise exception 'failed shared settings batch left partial data: %',settings_probe;
  end if;
  work_day_updated:=timegenie.direct_cloud_work_day_set(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000748',1,
    jsonb_build_object('workDate',expected_work_date::date+1,'expectedVersion',1,
      'workPeriodText','09:00-11:00','note','批量日历')
  );
  begin
    perform timegenie.direct_cloud_work_day_set(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-0000000000bd',
      '4f000000-0000-0000-0000-000000000749',1,
      jsonb_build_object('workDate',expected_work_date::date+1,'expectedVersion',1,'note','陈旧日历')
    );
    raise exception 'stale work-day update was accepted';
  exception when others then if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if; end;
  work_day_failed:=timegenie.integration_direct_settings_probe(
    '20000000-0000-0000-0000-00000000000a','4f000000-0000-0000-0000-000000000749',
    'user_name',expected_work_date::date+1,'51000000-0000-0000-0000-0000000000a1',0
  );
  if (work_day_failed->'workDay'->>'version')::bigint<>2
     or work_day_failed->'workDay'->>'note' is distinct from '批量日历'
     or (work_day_failed->>'operationCount')::integer<>0 then
    raise exception 'stale work-day update or version check failed: %',work_day_failed;
  end if;
  work_day_created:=timegenie.direct_cloud_work_day_set(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000750',1,
    jsonb_build_object('workDate',expected_work_date::date+2,'expectedVersion',0,
      'workPeriodText','08:30-12:00','note','新建日历')
  );
  settings_probe:=timegenie.integration_direct_settings_probe(
    '20000000-0000-0000-0000-00000000000a','4f000000-0000-0000-0000-000000000750',
    'user_name',expected_work_date::date+2,'51000000-0000-0000-0000-0000000000a1',0
  );
  if not (work_day_created->'data'->>'created')::boolean
     or (settings_probe->'workDay'->>'version')::bigint<>1
     or settings_probe->'workDay'->>'timezone' is distinct from 'Pacific/Kiritimati' then
    raise exception 'direct work-day creation is not authoritative: %',settings_probe;
  end if;
  settings_probe:=timegenie.integration_direct_settings_probe(
    '20000000-0000-0000-0000-00000000000a','4f000000-0000-0000-0000-000000000750',
    'user_name',expected_work_date::date,'51000000-0000-0000-0000-0000000000a1',0
  );
  schedule_day_monday:=settings_probe->'schedule';
  schedule_granular:=timegenie.direct_cloud_subject_schedule_save(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000751',1,jsonb_build_object('subjectId','51000000-0000-0000-0000-0000000000a1','day',
      jsonb_build_object('id',schedule_day_monday->>'id','weekday',0,'expectedVersion',(schedule_day_monday->>'version')::bigint,
        'enabled',true,'segments',jsonb_build_array(jsonb_build_object('start','08:00','end','10:00'),jsonb_build_object('start','11:00','end','12:00'))))
  );
  begin
    perform timegenie.direct_cloud_subject_schedule_save(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-0000000000bd',
      '4f000000-0000-0000-0000-000000000752',1,jsonb_build_object('subjectId','51000000-0000-0000-0000-0000000000a1','day',
        jsonb_build_object('id',schedule_day_monday->>'id','weekday',0,'expectedVersion',(schedule_day_monday->>'version')::bigint,
          'enabled',true,'segments',jsonb_build_array(jsonb_build_object('start','07:00','end','09:00'))))
    );
    raise exception 'stale subject schedule update was accepted';
  exception when others then if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if; end;
  settings_probe:=timegenie.integration_direct_settings_probe(
    '20000000-0000-0000-0000-00000000000a','4f000000-0000-0000-0000-000000000752',
    'user_name',expected_work_date::date,'51000000-0000-0000-0000-0000000000a1',0
  );
  if (settings_probe->'schedule'->>'version')::bigint<>(schedule_day_monday->>'version')::bigint+1
     or jsonb_array_length(settings_probe->'schedule'->'segments')<>2 then
    raise exception 'stale subject schedule changed the authoritative row: %',settings_probe;
  end if;
  schedule_batch_payload:=jsonb_build_object('subjectId','51000000-0000-0000-0000-0000000000a1','days',
    (select jsonb_agg((day-'version')||jsonb_build_object('expectedVersion',(day->>'version')::bigint) order by (day->>'weekday')::integer)
     from jsonb_array_elements(settings_probe->'subjectSchedule'->'days') day));
  schedule_batch_payload:=jsonb_set(schedule_batch_payload,'{days,6}',
    jsonb_build_object('id',settings_probe->'subjectSchedule'->'days'->6->>'id','weekday',6,
      'expectedVersion',(settings_probe->'subjectSchedule'->'days'->6->>'version')::bigint,
      'enabled',true,'segments',jsonb_build_array(jsonb_build_object('start','09:00','end','10:00'))));
  settings_batch:=timegenie.direct_cloud_subject_schedule_save(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000753',1,schedule_batch_payload
  );
  settings_batch_retry:=timegenie.direct_cloud_subject_schedule_save(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000753',1,schedule_batch_payload
  );
  work_day_daily:=timegenie.integration_direct_settings_probe(
    '20000000-0000-0000-0000-00000000000a','4f000000-0000-0000-0000-000000000753',
    'user_name',expected_work_date::date,'51000000-0000-0000-0000-0000000000a1',6
  );
  if settings_batch is distinct from settings_batch_retry
     or not (work_day_daily->'schedule'->>'enabled')::boolean
     or jsonb_array_length(work_day_daily->'schedule'->'segments')<>1 then
    raise exception 'subject work-schedule batch was not idempotent: % / %',settings_batch,settings_batch_retry;
  end if;
  schedule_failed_payload:=jsonb_build_object('subjectId','51000000-0000-0000-0000-0000000000a1','days',
    (select jsonb_agg((day-'version')||jsonb_build_object('expectedVersion',(day->>'version')::bigint) order by (day->>'weekday')::integer)
     from jsonb_array_elements(work_day_daily->'subjectSchedule'->'days') day));
  schedule_failed_payload:=jsonb_set(schedule_failed_payload,'{days,0}',
    jsonb_build_object('id',work_day_daily->'subjectSchedule'->'days'->0->>'id','weekday',0,
      'expectedVersion',(work_day_daily->'subjectSchedule'->'days'->0->>'version')::bigint,
      'enabled',true,'segments',jsonb_build_array(jsonb_build_object('start','07:00','end','09:00'))));
  schedule_failed_payload:=jsonb_set(schedule_failed_payload,'{days,6,expectedVersion}',
    to_jsonb((work_day_daily->'subjectSchedule'->'days'->6->>'version')::bigint-1));
  begin
    perform timegenie.direct_cloud_subject_schedule_save(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4f000000-0000-0000-0000-000000000754',1,schedule_failed_payload
    );
    raise exception 'partial subject work-schedule batch was accepted';
  exception when others then if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if; end;
  work_day_failed:=timegenie.integration_direct_settings_probe(
    '20000000-0000-0000-0000-00000000000a','4f000000-0000-0000-0000-000000000754',
    'user_name',expected_work_date::date,'51000000-0000-0000-0000-0000000000a1',0
  );
  if (work_day_failed->'schedule'->>'version')::bigint<>(work_day_daily->'subjectSchedule'->'days'->0->>'version')::bigint
     or jsonb_array_length(work_day_failed->'schedule'->'segments')<>2
     or (work_day_failed->>'operationCount')::integer<>0 then
    raise exception 'failed subject work-schedule batch left partial data: %',work_day_failed;
  end if;
  integration_config_saved:=timegenie.direct_cloud_integration_config_save(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000760',1,
    jsonb_build_object('provider','seatable','enabled',true,
      'config',jsonb_build_object('serverUrl','https://cloud.seatable.cn','baseUuid','base-direct','taskTable','DirectTasks'),
      'expectedVersion',5)
  );
  integration_config_retry:=timegenie.direct_cloud_integration_config_save(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000760',1,
    jsonb_build_object('provider','seatable','enabled',true,
      'config',jsonb_build_object('serverUrl','https://cloud.seatable.cn','baseUuid','base-direct','taskTable','DirectTasks'),
      'expectedVersion',5)
  );
  if integration_config_saved is distinct from integration_config_retry
     or (integration_config_saved->'data'->'integration'->>'version')::bigint<>6
     or integration_config_saved->'data'->'integration'->'config'->>'baseUuid' is distinct from 'base-direct'
     or integration_config_saved->'data'->'integration'->'config' ? 'baseApiToken'
     or integration_config_saved->'data'->'integration'->'config' ? 'password' then
    raise exception 'direct integration config save was not authoritative/idempotent: %',integration_config_saved;
  end if;
  integration_state:=timegenie.direct_cloud_integration_state(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-0000000000bd',1
  );
  integration_state_text:=integration_state::text;
  if integration_state->'metadata'->'domains' is distinct from '["settings","integrations"]'::jsonb
     or not exists(select 1 from jsonb_array_elements(integration_state->'data'->'integrations') item
       where item->>'provider'='seatable' and (item->>'version')::bigint=6 and item->'config'->>'baseUuid'='base-direct')
     or not exists(select 1 from jsonb_array_elements(integration_state->'data'->'integrations') item
       where item->>'provider'='obsidian' and item->'config'->>'vaultName'='TimeGenie')
     or integration_state->'data'->'bindings'->0->>'externalId' is distinct from 'row-public-id'
     or (integration_state->'data'->'bindings'->0->>'version')::bigint<>1
     or integration_state_text~*'(baseApiToken|managementKey|api_key|password|lastSyncedHash)'
     or integration_state_text like '%base-token-must-not-leak%'
     or integration_state_text like '%password-must-not-leak%'
     or integration_state_text like '%nested-api-key-must-not-leak%'
     or integration_state_text like '%binding-hash-must-not-leak%' then
    raise exception 'direct integration state crossed the credential boundary: %',integration_state;
  end if;
  begin
    perform timegenie.direct_cloud_integration_config_save(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4f000000-0000-0000-0000-000000000761',1,
      jsonb_build_object('provider','seatable','enabled',true,
        'config',jsonb_build_object('serverUrl','https://cloud.seatable.cn','baseApiToken','new-cloud-token-must-not-leak'),
        'expectedVersion',6)
    );
    raise exception 'credential integration config was accepted';
  exception when others then if sqlerrm not like '%凭据库%' then raise; end if; end;
  integration_probe:=timegenie.integration_direct_integration_probe(
    '20000000-0000-0000-0000-00000000000a','4f000000-0000-0000-0000-000000000761',
    'seatable','task','61000000-0000-0000-0000-0000000000a1'
  );
  if (integration_probe->'config'->>'version')::bigint<>6 or (integration_probe->>'operationCount')::integer<>0 then
    raise exception 'rejected integration credential changed cloud state: %',integration_probe;
  end if;
  begin
    perform timegenie.direct_cloud_integration_config_save(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4f000000-0000-0000-0000-000000000762',1,
      jsonb_build_object('provider','obsidian','enabled',true,
        'config',jsonb_build_object('vaultName','Work','vaultPath','D:/private-vault'),'expectedVersion',2)
    );
    raise exception 'device-local integration path was accepted';
  exception when others then if sqlerrm not like '%本机路径%' then raise; end if; end;
  begin
    perform timegenie.direct_cloud_integration_config_save(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-0000000000bd',
      '4f000000-0000-0000-0000-000000000763',1,
      jsonb_build_object('provider','seatable','enabled',false,
        'config',jsonb_build_object('baseUuid','stale-device-write'),'expectedVersion',5)
    );
    raise exception 'stale integration config from another device was accepted';
  exception when others then if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if; end;
  settings_batch:=timegenie.direct_cloud_integration_config_save(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000764',1,
    jsonb_build_object('provider','obsidian','enabled',true,
      'config',jsonb_build_object('vaultName','WorkVault','nested',jsonb_build_object('theme','dark')),'expectedVersion',2)
  );
  if (settings_batch->'data'->'integration'->>'version')::bigint<>3
     or settings_batch->'data'->'integration'->'config' ? 'vaultPath' then
    raise exception 'direct Obsidian metadata save was not authoritative: %',settings_batch;
  end if;
  binding_saved:=timegenie.direct_cloud_external_binding_upsert(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000765',1,
    jsonb_build_object('provider','seatable','entityType','task','entityId','61000000-0000-0000-0000-0000000000a1',
      'externalId','row-direct-1','externalRevision','rev-direct-1','lastSyncedAt',1792622400000,'expectedVersion',1)
  );
  binding_retry:=timegenie.direct_cloud_external_binding_upsert(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000765',1,
    jsonb_build_object('provider','seatable','entityType','task','entityId','61000000-0000-0000-0000-0000000000a1',
      'externalId','row-direct-1','externalRevision','rev-direct-1','lastSyncedAt',1792622400000,'expectedVersion',1)
  );
  if binding_saved is distinct from binding_retry
     or (binding_saved->'data'->'binding'->>'version')::bigint<>2
     or binding_saved->'data'->'binding'->>'externalId' is distinct from 'row-direct-1' then
    raise exception 'direct external binding upsert was not authoritative/idempotent: %',binding_saved;
  end if;
  begin
    perform timegenie.direct_cloud_external_binding_upsert(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-0000000000bd',
      '4f000000-0000-0000-0000-000000000766',1,
      jsonb_build_object('provider','seatable','entityType','task','entityId','61000000-0000-0000-0000-0000000000a1',
        'externalId','row-stale-device','expectedVersion',1)
    );
    raise exception 'stale external binding from another device was accepted';
  exception when others then if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if; end;
  integration_probe:=timegenie.integration_direct_integration_probe(
    '20000000-0000-0000-0000-00000000000a','4f000000-0000-0000-0000-000000000766',
    'seatable','task','61000000-0000-0000-0000-0000000000a1'
  );
  if (integration_probe->'binding'->>'version')::bigint<>2
     or integration_probe->'binding'->>'externalId' is distinct from 'row-direct-1'
     or (integration_probe->>'operationCount')::integer<>0
     or integration_probe->>'historyText' like '%row-stale-device%' then
    raise exception 'rejected external binding changed cloud state: %',integration_probe;
  end if;
  binding_created:=timegenie.direct_cloud_external_binding_upsert(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000767',1,
    jsonb_build_object('provider','seatable','entityType','task','entityId','61000000-0000-0000-0000-0000000000a2',
      'externalId','row-direct-2','expectedVersion',0)
  );
  if (binding_created->'data'->'binding'->>'version')::bigint<>1 then
    raise exception 'direct external binding creation is invalid: %',binding_created;
  end if;
  binding_deleted:=timegenie.direct_cloud_external_binding_delete(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000768',1,
    jsonb_build_object('provider','seatable','entityType','task','entityId','61000000-0000-0000-0000-0000000000a2',
      'expectedVersion',1)
  );
  binding_delete_retry:=timegenie.direct_cloud_external_binding_delete(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-000000000768',1,
    jsonb_build_object('provider','seatable','entityType','task','entityId','61000000-0000-0000-0000-0000000000a2',
      'expectedVersion',1)
  );
  integration_probe:=timegenie.integration_direct_integration_probe(
    '20000000-0000-0000-0000-00000000000a','4f000000-0000-0000-0000-000000000768',
    'seatable','task','61000000-0000-0000-0000-0000000000a2'
  );
  if binding_deleted is distinct from binding_delete_retry
     or not coalesce((binding_deleted->'data'->>'changed')::boolean,false)
     or (integration_probe->>'bindingCount')::integer<>0
     or (integration_probe->>'operationCount')::integer<>1
     or (integration_probe->>'historyText') like '%binding-hash-must-not-leak%' then
    raise exception 'direct external binding delete was not idempotent: % / %',binding_deleted,integration_probe;
  end if;
  plan_import:=timegenie.direct_cloud_plan_import_confirm(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-0000000007a9',1,
    jsonb_build_object('subjectId','51000000-0000-0000-0000-0000000000a1','targetDate','2026-10-24',
      'items',jsonb_build_array(
        jsonb_build_object('id','61000000-0000-0000-0000-0000000000f1','title','导入父事项','estimateMinutes',45,'sortOrder',1),
        jsonb_build_object('id','61000000-0000-0000-0000-0000000000f2','parentId','61000000-0000-0000-0000-0000000000f1','title','导入子事项','sortOrder',1)
      ))
  );
  plan_import_retry:=timegenie.direct_cloud_plan_import_confirm(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
    '4f000000-0000-0000-0000-0000000007a9',1,
    jsonb_build_object('subjectId','51000000-0000-0000-0000-0000000000a1','targetDate','2026-10-24',
      'items',jsonb_build_array(
        jsonb_build_object('id','61000000-0000-0000-0000-0000000000f1','title','导入父事项','estimateMinutes',45,'sortOrder',1),
        jsonb_build_object('id','61000000-0000-0000-0000-0000000000f2','parentId','61000000-0000-0000-0000-0000000000f1','title','导入子事项','sortOrder',1)
      ))
  );
  plan_import_probe:=timegenie.integration_direct_planning_probe(
    '20000000-0000-0000-0000-00000000000a','61000000-0000-0000-0000-0000000000f2',null
  );
  if plan_import is distinct from plan_import_retry
     or (plan_import->'data'->>'count')::integer<>2
     or jsonb_array_length(plan_import->'data'->'tasks')<>2
     or plan_import_probe->'task'->>'parentId' is distinct from '61000000-0000-0000-0000-0000000000f1'
     or plan_import_probe->'task'->>'title' is distinct from '导入子事项'
     or plan_import->'data'->'tasks'->0->>'plannedDate' is distinct from '2026-10-24'
     or plan_import->'data'->'tasks'->0->>'title' is distinct from '导入父事项' then
    raise exception 'direct plan import confirmation was not atomic/idempotent: %',plan_import;
  end if;
  begin
    perform timegenie.direct_cloud_plan_import_confirm(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4f000000-0000-0000-0000-0000000007aa',1,
      jsonb_build_object('subjectId','51000000-0000-0000-0000-0000000000a1','targetDate','2026-10-24',
        'items',jsonb_build_array(jsonb_build_object('id','61000000-0000-0000-0000-0000000000f1','title','重复导入')))
    );
    raise exception 'duplicate plan import identifiers were accepted';
  exception when others then if sqlerrm not like '%VERSION_CONFLICT%' then raise; end if; end;
  shared_configuration_value:=timegenie.direct_cloud_shared_configuration(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',1
  );
  shared_configuration_text:=shared_configuration_value::text;
  if shared_configuration_value->'metadata'->'domains' is distinct from '["settings","calendar","integrations"]'::jsonb
     or shared_configuration_value->'data'->'workspace'->>'timezone' is distinct from 'Pacific/Kiritimati'
     or shared_configuration_value->'data'->'settings'->>'user_name' is distinct from '直连用户-批量'
     or shared_configuration_value->'data'->'settings'->>'subject_salary_hourly_rate:51000000-0000-0000-0000-0000000000a1' is distinct from '120'
     or shared_configuration_value->'data'->'settings'->'shared_security_probe'->>'visible' is distinct from '保留'
     or shared_configuration_value->'data'->'settings'->'shared_security_probe'->'nested'->>'theme' is distinct from 'ocean'
     or jsonb_array_length(shared_configuration_value->'data'->'workDays')<>3
     or shared_configuration_value->'data'->'workDays'->0->>'workPeriodText' is distinct from '10:00-12:00 13:30-17:30'
     or jsonb_array_length(shared_configuration_value->'data'->'subjectWorkSchedules')<>2
     or jsonb_array_length(shared_configuration_value->'data'->'subjectWorkSchedules'->0->'days')<>7
     or shared_configuration_value->'data'->'subjectWorkSchedules'->0->'days'->0->>'version' is distinct from '5'
     or jsonb_array_length(shared_configuration_value->'data'->'subjectWorkSchedules'->0->'days'->0->'segments')<>2
     or shared_configuration_value->'data'->'subjectWorkSchedules'->0->'days'->0->'segments'->0->>'start' is distinct from '08:00'
     or jsonb_array_length(shared_configuration_value->'data'->'integrations')<>2
     or not exists(select 1 from jsonb_array_elements(shared_configuration_value->'data'->'integrations') item
       where item->>'provider'='seatable' and (item->>'requiresDeviceCredential')::boolean and item->'config'->>'baseUuid'='base-direct')
     or shared_configuration_value->'data'->'bindings'->0->>'externalId' is distinct from 'row-direct-1'
     or shared_configuration_text~*'(baseApiToken|api_key|password|managementKey|secret_ref|last_synced_hash)'
     or shared_configuration_text like '%base-token-must-not-leak%'
     or shared_configuration_text like '%nested-api-key-must-not-leak%'
     or shared_configuration_text like '%password-must-not-leak%'
     or shared_configuration_text like '%management-credential-must-not-leak%'
     or shared_configuration_text like '%binding-hash-must-not-leak%' then
    raise exception 'direct shared configuration contract or credential boundary is invalid: %',shared_configuration_value;
  end if;
  if octet_length(bootstrap_value::text)>131072
     or octet_length(task_list_value::text)>262144
     or octet_length(today_plan_value::text)>262144
     or octet_length(time_state_value::text)>262144
     or octet_length(report_page_one::text)>524288
     or octet_length(shared_configuration_value::text)>262144 then
    raise exception 'representative direct query response exceeded its performance baseline';
  end if;
  begin
    perform timegenie.direct_cloud_context(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',999
    );
    raise exception 'stale direct workspace epoch was accepted';
  exception when others then
    if sqlerrm not like '%WORKSPACE_EPOCH_STALE%' then raise; end if;
  end;
  perform set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000002',true);
  begin
    perform timegenie.direct_cloud_context(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',1
    );
    raise exception 'direct context exposed another account workspace';
  exception when others then
    if sqlerrm not like '%AUTH_REQUIRED%' then raise; end if;
  end;
  begin
    perform timegenie.direct_cloud_workbench_bootstrap(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',1
    );
    raise exception 'direct workbench bootstrap exposed another account workspace';
  exception when others then
    if sqlerrm not like '%AUTH_REQUIRED%' then raise; end if;
  end;
  begin
    perform timegenie.direct_cloud_shared_configuration(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',1
    );
    raise exception 'direct shared configuration exposed another account workspace';
  exception when others then
    if sqlerrm not like '%AUTH_REQUIRED%' then raise; end if;
  end;
  perform set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
  begin
    perform timegenie.direct_cloud_context(
      '20000000-0000-0000-0000-000000000001','30000000-0000-0000-0000-000000000001',1
    );
    raise exception 'direct context accepted a sync_v1 workspace';
  exception when others then
    if sqlerrm not like '%CLOUD_PROTOCOL_UNSUPPORTED%' then raise; end if;
  end;
  perform set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000003',true);
  perform timegenie.device_authorize(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-0000000000bc',
    '已被撤销的直连设备','windows','0.4.6'
  );
  perform timegenie.device_revoke(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-0000000000bc'
  );
  begin
    perform timegenie.direct_cloud_context(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-0000000000bc',1
    );
    raise exception 'direct context accepted a revoked device';
  exception when others then
    if sqlerrm not like '%DEVICE_REVOKED%' then raise; end if;
  end;
  begin
    perform timegenie.direct_cloud_workbench_bootstrap(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-0000000000bc',1
    );
    raise exception 'direct workbench bootstrap accepted a revoked device';
  exception when others then
    if sqlerrm not like '%DEVICE_REVOKED%' then raise; end if;
  end;
  begin
    perform timegenie.direct_cloud_shared_configuration(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-0000000000bc',1
    );
    raise exception 'direct shared configuration accepted a revoked device';
  exception when others then
    if sqlerrm not like '%DEVICE_REVOKED%' then raise; end if;
  end;
  perform set_config('request.headers','{}',true);
  if timegenie.direct_cloud_context(
       '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',1
     ) ? 'schemaGeneration' then
    raise exception 'direct context still exposed client schema generation';
  end if;
  begin
    perform timegenie.cloud_apply_patch(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-0000000000be',
      '4e000000-0000-0000-0000-000000000002','app_setting.update','app_setting','direct_protocol_write_probe',null,
      jsonb_build_object('key','direct_protocol_write_probe','value_json','true'::jsonb,'updated_at',1792622400000,'version',1)
    );
    raise exception 'legacy sync write was accepted after direct protocol upgrade';
  exception when others then
    if sqlerrm not like '%CLOUD_PROTOCOL_UNSUPPORTED%' then raise; end if;
  end;
  begin
    perform timegenie.tracking_lease_acquire(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-0000000000be',
      '4e000000-0000-0000-0000-000000000003'
    );
    raise exception 'legacy tracking lease was accepted after direct protocol upgrade';
  exception when others then
    if sqlerrm not like '%CLOUD_PROTOCOL_UNSUPPORTED%' then raise; end if;
  end;
  begin
    perform timegenie.tracking_lease_takeover_for_timer_write(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4e000000-0000-0000-0000-000000000004'
    );
    raise exception 'legacy timer-write lease takeover was accepted after direct protocol upgrade';
  exception when others then
    if sqlerrm not like '%CLOUD_PROTOCOL_UNSUPPORTED%' then raise; end if;
  end;
  begin
    perform timegenie.unassigned_get_or_create_shared(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a'
    );
    raise exception 'legacy unassigned anchor creation was accepted after direct protocol upgrade';
  exception when others then
    if sqlerrm not like '%CLOUD_PROTOCOL_UNSUPPORTED%' then raise; end if;
  end;
  begin
    perform timegenie.unassigned_resolve_shared(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4e000000-0000-0000-0000-000000000005','4e000000-0000-0000-0000-000000000006',
      1,'work','{}'::jsonb
    );
    raise exception 'legacy unassigned resolution was accepted after direct protocol upgrade';
  exception when others then
    if sqlerrm not like '%CLOUD_PROTOCOL_UNSUPPORTED%' then raise; end if;
  end;
  begin
    perform timegenie.unassigned_anchor_reset(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4e000000-0000-0000-0000-000000000007','4e000000-0000-0000-0000-000000000008',
      1,null
    );
    raise exception 'legacy unassigned anchor reset was accepted after direct protocol upgrade';
  exception when others then
    if sqlerrm not like '%CLOUD_PROTOCOL_UNSUPPORTED%' then raise; end if;
  end;
  begin
    perform timegenie.actual_adjustment_batch_apply(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4e000000-0000-0000-0000-000000000009','{}'::jsonb
    );
    raise exception 'legacy actual adjustment batch was accepted after direct protocol upgrade';
  exception when others then
    if sqlerrm not like '%CLOUD_PROTOCOL_UNSUPPORTED%' then raise; end if;
  end;
  begin
    perform timegenie.timer_transition_apply(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4e000000-0000-0000-0000-00000000000a','{}'::jsonb
    );
    raise exception 'legacy timer transition was accepted after direct protocol upgrade';
  exception when others then
    if sqlerrm not like '%CLOUD_PROTOCOL_UNSUPPORTED%' then raise; end if;
  end;
  begin
    perform timegenie.migration_import_snapshot(
      '20000000-0000-0000-0000-00000000000a','4e000000-0000-0000-0000-00000000000b',
      '30000000-0000-0000-0000-00000000000a','{}'::jsonb
    );
    raise exception 'legacy migration import was accepted after direct protocol upgrade';
  exception when others then
    if sqlerrm not like '%CLOUD_PROTOCOL_UNSUPPORTED%' then raise; end if;
  end;
  begin
    perform timegenie.workspace_restore_prepare(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4e000000-0000-0000-0000-00000000000c',1
    );
    raise exception 'legacy restore prepare was accepted after direct protocol upgrade';
  exception when others then
    if sqlerrm not like '%CLOUD_PROTOCOL_UNSUPPORTED%' then raise; end if;
  end;
  begin
    perform timegenie.workspace_restore_prepare(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4e000000-0000-0000-0000-00000000000c',1,true
    );
    raise exception 'legacy restore prepare with safety snapshot was accepted after direct protocol upgrade';
  exception when others then
    if sqlerrm not like '%CLOUD_PROTOCOL_UNSUPPORTED%' then raise; end if;
  end;
  begin
    perform timegenie.workspace_restore_commit(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',
      '4e000000-0000-0000-0000-00000000000d','4e000000-0000-0000-0000-00000000000e',
      'not-a-real-hash',1,'4e000000-0000-0000-0000-00000000000f',true
    );
    raise exception 'legacy restore commit was accepted after direct protocol upgrade';
  exception when others then
    if sqlerrm not like '%CLOUD_PROTOCOL_UNSUPPORTED%' then raise; end if;
  end;
  begin
    perform timegenie.cloud_incremental_pull(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-0000000000be',0,10
    );
    raise exception 'legacy incremental pull was accepted after direct protocol upgrade';
  exception when others then
    if sqlerrm not like '%CLOUD_PROTOCOL_UNSUPPORTED%' then raise; end if;
  end;
  old_client_bootstrap:=timegenie.direct_cloud_workbench_bootstrap(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-0000000000be',1
  );
  if old_client_bootstrap->'metadata'->>'storageProtocol' is distinct from 'direct_v2'
     or (old_client_bootstrap->'data'->'summary'->>'subjectCount')::integer<1 then
    raise exception 'upgraded second device could not read the latest direct cloud data: %',old_client_bootstrap;
  end if;
end $$;
reset role;
do $$ begin
  if exists(
    select 1 from timegenie.cloud_operation_logs
    where workspace_id='20000000-0000-0000-0000-00000000000a'
      and operation_id='4e000000-0000-0000-0000-000000000119'
  ) then raise exception 'failed direct mutation recorded a successful operation history row'; end if;
  if (select count(*) from timegenie.processed_operations where workspace_id='20000000-0000-0000-0000-00000000000a' and operation_id='4e000000-0000-0000-0000-000000000001')<>1
     or (select count(*) from timegenie.workspace_changes where workspace_id='20000000-0000-0000-0000-00000000000a' and entity_type='workspace_reset')<>1
     or (select count(*) from timegenie.cloud_operation_logs where workspace_id='20000000-0000-0000-0000-00000000000a' and operation_id='4e000000-0000-0000-0000-000000000001')<>1 then
    raise exception 'repeated direct operation was applied more than once';
  end if;
  if exists(select 1 from timegenie.app_settings where workspace_id='20000000-0000-0000-0000-00000000000a' and key='direct_protocol_write_probe') then
    raise exception 'rejected legacy sync write changed business data';
  end if;
  if exists(select 1 from timegenie.tracking_leases where workspace_id='20000000-0000-0000-0000-00000000000a') then
    raise exception 'rejected legacy lease changed coordination state';
  end if;
  if (select count(*) from timegenie.unassigned_sessions where workspace_id='20000000-0000-0000-0000-00000000000a')<>1
     or (select count(*) from timegenie.actual_time_adjustments where workspace_id='20000000-0000-0000-0000-00000000000a')<>9
     or exists(select 1 from timegenie.workspace_restore_locks where workspace_id='20000000-0000-0000-0000-00000000000a')
     or exists(select 1 from timegenie.workspace_snapshots where workspace_id='20000000-0000-0000-0000-00000000000a') then
    raise exception 'rejected legacy write changed protected workspace state';
  end if;
end $$;

delete from timegenie.time_allocations where workspace_id='20000000-0000-0000-0000-00000000000a';
delete from timegenie.time_segments where workspace_id='20000000-0000-0000-0000-00000000000a';
delete from timegenie.time_entries where workspace_id='20000000-0000-0000-0000-00000000000a';
delete from timegenie.unassigned_segments where workspace_id='20000000-0000-0000-0000-00000000000a';
delete from timegenie.unassigned_sessions where workspace_id='20000000-0000-0000-0000-00000000000a';
delete from timegenie.actual_time_adjustments where workspace_id='20000000-0000-0000-0000-00000000000a';
delete from timegenie.task_occurrences where workspace_id='20000000-0000-0000-0000-00000000000a';
delete from timegenie.task_recurrence_rules where workspace_id='20000000-0000-0000-0000-00000000000a';
delete from timegenie.task_day_plans where workspace_id='20000000-0000-0000-0000-00000000000a';
delete from timegenie.task_daily_estimates where workspace_id='20000000-0000-0000-0000-00000000000a';
delete from timegenie.report_tasks where workspace_id='20000000-0000-0000-0000-00000000000a';
delete from timegenie.reports where workspace_id='20000000-0000-0000-0000-00000000000a';
delete from timegenie.report_templates where workspace_id='20000000-0000-0000-0000-00000000000a';
delete from timegenie.tasks where workspace_id='20000000-0000-0000-0000-00000000000a';
delete from timegenie.subjects where workspace_id='20000000-0000-0000-0000-00000000000a';
set local role authenticated;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000003',true);
select set_config('request.jwt.claim.session_id','80000000-0000-0000-0000-00000000000a',true);
select set_config('request.headers','{"x-timegenie-client-schema-generation":"1"}',true);
do $$
declare empty_value jsonb; expected_work_date text;
begin
  empty_value:=timegenie.direct_cloud_workbench_bootstrap(
    '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',1
  );
  expected_work_date:=(now() at time zone 'Pacific/Kiritimati')::date::text;
  if empty_value->'data'->>'currentWorkDate' is distinct from expected_work_date
     or empty_value->'data'->>'timezone' is distinct from 'Pacific/Kiritimati'
     or empty_value->'data'->'subjects' is distinct from '[]'::jsonb
     or (empty_value->'data'->'summary'->>'subjectCount')::integer<>0
     or (empty_value->'data'->'summary'->>'openTaskCount')::integer<>0
     or (empty_value->'data'->'summary'->>'completedTaskCount')::integer<>0
     or (empty_value->'data'->'summary'->>'taskCount')::integer<>0 then
    raise exception 'empty direct workbench bootstrap is invalid: %',empty_value;
  end if;
  perform set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000001',true);
  begin
    perform timegenie.direct_cloud_workbench_bootstrap(
      '20000000-0000-0000-0000-00000000000a','30000000-0000-0000-0000-00000000000a',1
    );
    raise exception 'empty workspace bootstrap bypassed row-level ownership';
  exception when others then
    if sqlerrm not like '%AUTH_REQUIRED%' then raise; end if;
  end;
end $$;
reset role;

\ir 20261021c_timegenie_daily_report_collection_templates.sql
\ir 20261021c_timegenie_daily_report_collection_templates.sql

begin;

do $$
begin
  if exists(
    select 1 from timegenie.report_templates
    where is_builtin and report_type in ('weekly','monthly') and deleted_at is null
      and content not like '%{{日报合集}}%'
  ) then raise exception 'weekly/monthly built-in templates must aggregate saved daily reports'; end if;
end $$;

create temporary table data_copy_authority_baseline on commit drop as
select
  (select count(*) from timegenie.subjects where workspace_id='20000000-0000-0000-0000-00000000000a') as subject_count,
  (select count(*) from timegenie.tasks where workspace_id='20000000-0000-0000-0000-00000000000a') as task_count;
create temporary table data_copy_test_catalog on commit drop as
select * from timegenie.data_copy_entity_catalog();
grant select on data_copy_test_catalog to authenticated;

set role authenticated;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000003',true);
select set_config('request.jwt.claim.session_id','80000000-0000-0000-0000-00000000000a',true);
select set_config('request.headers','{"x-timegenie-client-schema-generation":"1"}',true);
do $$
declare
  workspace_value uuid:='20000000-0000-0000-0000-00000000000a';
  device_value uuid:='30000000-0000-0000-0000-00000000000a';
  package_value uuid; begin_value jsonb; validated_value jsonb; catalog_row record;
  chunk_index_value integer; empty_hash text:='4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945';
  content_hash_value text; tasks_text text; tasks_hash text;
begin
  content_hash_value:='41d3dc79d2110e88cb8d7b4b6d3ca559e70c6ee0b7da4964a1284ba815878cd5';

  begin_value:=timegenie.data_copy_stage_begin(workspace_value,device_value,1,1,22,content_hash_value);
  package_value:=(begin_value->>'packageId')::uuid; chunk_index_value:=0;
  for catalog_row in select * from data_copy_test_catalog order by sort_order loop
    perform timegenie.data_copy_stage_chunk_put(workspace_value,device_value,1,package_value,chunk_index_value,catalog_row.entity_name,'[]',empty_hash,0);
    chunk_index_value:=chunk_index_value+1;
  end loop;
  validated_value:=timegenie.data_copy_stage_validate(workspace_value,device_value,1,package_value);
  if validated_value->>'status' is distinct from 'validated' then raise exception 'complete staging package was not validated'; end if;

  begin_value:=timegenie.data_copy_stage_begin(workspace_value,device_value,1,1,22,content_hash_value);
  package_value:=(begin_value->>'packageId')::uuid; chunk_index_value:=0;
  for catalog_row in select * from data_copy_test_catalog order by sort_order limit 21 loop
    perform timegenie.data_copy_stage_chunk_put(workspace_value,device_value,1,package_value,chunk_index_value,catalog_row.entity_name,'[]',empty_hash,0);
    chunk_index_value:=chunk_index_value+1;
  end loop;
  begin perform timegenie.data_copy_stage_validate(workspace_value,device_value,1,package_value); raise exception 'missing staging chunk was accepted';
  exception when others then if sqlerrm not like '%DATA_PACKAGE_CHUNK_MISSING%' then raise; end if; end;

  begin_value:=timegenie.data_copy_stage_begin(workspace_value,device_value,1,1,22,content_hash_value);
  package_value:=(begin_value->>'packageId')::uuid;
  begin perform timegenie.data_copy_stage_chunk_put(workspace_value,device_value,1,package_value,0,'subjects','[]',repeat('0',64),0); raise exception 'damaged staging chunk was accepted';
  exception when others then if sqlerrm not like '%DATA_PACKAGE_CHUNK_HASH_MISMATCH%' then raise; end if; end;

  begin perform timegenie.data_copy_stage_begin(workspace_value,device_value,1,2,22,content_hash_value); raise exception 'unsupported package version was accepted';
  exception when others then if sqlerrm not like '%DATA_PACKAGE_VERSION_UNSUPPORTED%' then raise; end if; end;

  tasks_text:='[{"id":"90000000-0000-0000-0000-000000000001","subject_id":"90000000-0000-0000-0000-000000000099"}]';
  tasks_hash:='5aed44403b0b0018cf3c1c577eeec3322df9864a56eef499665f77929dbe309b';
  content_hash_value:='2724e6aec79d3360343435c8807544d1bfb710e44cc6d4761a4d5a653b18ab67';
  begin_value:=timegenie.data_copy_stage_begin(workspace_value,device_value,1,1,22,content_hash_value);
  package_value:=(begin_value->>'packageId')::uuid; chunk_index_value:=0;
  for catalog_row in select * from data_copy_test_catalog order by sort_order loop
    if catalog_row.entity_name='tasks' then
      perform timegenie.data_copy_stage_chunk_put(workspace_value,device_value,1,package_value,chunk_index_value,catalog_row.entity_name,tasks_text,tasks_hash,1);
    else
      perform timegenie.data_copy_stage_chunk_put(workspace_value,device_value,1,package_value,chunk_index_value,catalog_row.entity_name,'[]',empty_hash,0);
    end if;
    chunk_index_value:=chunk_index_value+1;
  end loop;
  begin perform timegenie.data_copy_stage_validate(workspace_value,device_value,1,package_value); raise exception 'broken staging foreign key was accepted';
  exception when others then if sqlerrm not like '%DATA_PACKAGE_FOREIGN_KEY_INVALID: tasks%' then raise; end if; end;
end $$;
reset role;

do $$
declare baseline record;
begin
  select * into baseline from data_copy_authority_baseline;
  if baseline.subject_count<>(select count(*) from timegenie.subjects where workspace_id='20000000-0000-0000-0000-00000000000a')
     or baseline.task_count<>(select count(*) from timegenie.tasks where workspace_id='20000000-0000-0000-0000-00000000000a') then
    raise exception 'staging validation changed authoritative business data';
  end if;
end $$;

-- A validated package replaces the direct-v2 workspace under the shared
-- restore lock, keeps a pre_replace snapshot, advances the epoch, and emits
-- one reset. A later import failure must roll every authoritative effect back.
select set_config(
  'request.headers',
  '{"x-timegenie-workspace-epoch":"1","x-timegenie-client-schema-generation":"1"}',
  true
);
insert into timegenie.subjects(id,workspace_id,name,sort_order) values(
  '90000000-0000-0000-0000-000000000020',
  '20000000-0000-0000-0000-00000000000a',
  '替换前云端主体',20
);
set role authenticated;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000003',true);
select set_config('request.jwt.claim.session_id','80000000-0000-0000-0000-00000000000a',true);
do $$
declare
  workspace_value uuid:='20000000-0000-0000-0000-00000000000a';
  device_value uuid:='30000000-0000-0000-0000-00000000000a';
  package_value uuid; begin_value jsonb; replace_value jsonb; catalog_row record;
  chunk_index_value integer:=0;
  empty_hash text:='4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945';
  content_hash_value text:='41d3dc79d2110e88cb8d7b4b6d3ca559e70c6ee0b7da4964a1284ba815878cd5';
begin
  begin_value:=timegenie.data_copy_stage_begin(
    workspace_value,device_value,1,1,22,content_hash_value
  );
  package_value:=(begin_value->>'packageId')::uuid;
  for catalog_row in select * from data_copy_test_catalog order by sort_order loop
    perform timegenie.data_copy_stage_chunk_put(
      workspace_value,device_value,1,package_value,chunk_index_value,
      catalog_row.entity_name,'[]',empty_hash,0
    );
    chunk_index_value:=chunk_index_value+1;
  end loop;
  perform timegenie.data_copy_stage_validate(
    workspace_value,device_value,1,package_value
  );
  replace_value:=timegenie.data_copy_replace_commit(
    workspace_value,device_value,1,package_value,content_hash_value,
    '4e000000-0000-0000-0000-0000000000f1',true
  );
  if not coalesce((replace_value->>'replaced')::boolean,false)
     or (replace_value->>'previousEpoch')::bigint<>1
     or (replace_value->>'workspaceEpoch')::bigint<>2
     or replace_value->>'safetySnapshotId' is null then
    raise exception 'data copy replacement result is invalid: %',replace_value;
  end if;
  if timegenie.data_copy_replace_commit(
       workspace_value,device_value,1,package_value,content_hash_value,
       '4e000000-0000-0000-0000-0000000000f1',true
     ) is distinct from replace_value then
    raise exception 'data copy replacement retry did not return its cached result';
  end if;
end $$;
reset role;
do $$
declare safety_id uuid;
begin
  if exists(select 1 from timegenie.subjects where workspace_id='20000000-0000-0000-0000-00000000000a') then
    raise exception 'validated empty package did not replace authoritative subjects';
  end if;
  if (select workspace_epoch from timegenie.workspaces where id='20000000-0000-0000-0000-00000000000a')<>2 then
    raise exception 'data copy replacement did not advance workspace epoch';
  end if;
  select id into safety_id from timegenie.workspace_snapshots
  where workspace_id='20000000-0000-0000-0000-00000000000a'
    and snapshot_type='pre_replace' and status='ready'
  order by created_at desc limit 1;
  if safety_id is null
     or coalesce((select (entity_counts->>'subjects')::integer from timegenie.workspace_snapshots where id=safety_id),0)<>1 then
    raise exception 'pre_replace safety snapshot did not preserve the previous cloud subject';
  end if;
  if not exists(
    select 1 from timegenie.workspace_changes
    where workspace_id='20000000-0000-0000-0000-00000000000a'
      and entity_type='workspace_reset' and entity_version=2
  ) then raise exception 'data copy replacement did not publish a reset notification'; end if;
  if exists(select 1 from timegenie.workspace_restore_locks where workspace_id='20000000-0000-0000-0000-00000000000a') then
    raise exception 'data copy replacement left its restore lock behind';
  end if;
end $$;

select set_config(
  'request.headers',
  '{"x-timegenie-workspace-epoch":"2","x-timegenie-client-schema-generation":"1"}',
  true
);
insert into timegenie.subjects(id,workspace_id,name,sort_order) values(
  '90000000-0000-0000-0000-000000000021',
  '20000000-0000-0000-0000-00000000000a',
  '失败回滚基线主体',21
);
create temporary table data_copy_replace_failure_baseline on commit drop as
select
  (select jsonb_agg(to_jsonb(subject) order by id) from timegenie.subjects subject where workspace_id='20000000-0000-0000-0000-00000000000a') as subjects_value,
  (select workspace_epoch from timegenie.workspaces where id='20000000-0000-0000-0000-00000000000a') as epoch_value,
  (select count(*) from timegenie.workspace_snapshots where workspace_id='20000000-0000-0000-0000-00000000000a' and snapshot_type='pre_replace') as snapshot_count,
  (select count(*) from timegenie.workspace_changes where workspace_id='20000000-0000-0000-0000-00000000000a' and entity_type='workspace_reset') as reset_count;
set role authenticated;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000003',true);
select set_config('request.jwt.claim.session_id','80000000-0000-0000-0000-00000000000a',true);
do $$
declare
  workspace_value uuid:='20000000-0000-0000-0000-00000000000a';
  device_value uuid:='30000000-0000-0000-0000-00000000000a';
  package_value uuid; begin_value jsonb; catalog_row record; chunk_index_value integer:=0;
  empty_hash text:='4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945';
  invalid_text text:='[{"key":"invalid-copy-setting","value_json":true,"updated_at":"not-a-timestamp","version":1}]';
  invalid_hash text:='1253992f822d596f2d3e090bbb6da294178bee541ebf9d13d93ff4c215b5b3de';
  content_hash_value text:='3f5b6dec9932dd43a5074a9c7464591242282a8169131dec60fd31b0755cc0cf';
begin
  begin_value:=timegenie.data_copy_stage_begin(
    workspace_value,device_value,2,1,22,content_hash_value
  );
  package_value:=(begin_value->>'packageId')::uuid;
  for catalog_row in select * from data_copy_test_catalog order by sort_order loop
    if catalog_row.entity_name='app_settings' then
      perform timegenie.data_copy_stage_chunk_put(
        workspace_value,device_value,2,package_value,chunk_index_value,
        catalog_row.entity_name,invalid_text,invalid_hash,1
      );
    else
      perform timegenie.data_copy_stage_chunk_put(
        workspace_value,device_value,2,package_value,chunk_index_value,
        catalog_row.entity_name,'[]',empty_hash,0
      );
    end if;
    chunk_index_value:=chunk_index_value+1;
  end loop;
  perform timegenie.data_copy_stage_validate(
    workspace_value,device_value,2,package_value
  );
  begin
    perform timegenie.data_copy_replace_commit(
      workspace_value,device_value,2,package_value,content_hash_value,
      '4e000000-0000-0000-0000-0000000000f2',true
    );
    raise exception 'invalid staged package unexpectedly replaced cloud data';
  exception when others then
    if sqlerrm not like '%invalid input syntax%' then raise; end if;
  end;
end $$;
reset role;
do $$
declare baseline record;
begin
  select * into baseline from data_copy_replace_failure_baseline;
  if baseline.subjects_value is distinct from (
       select jsonb_agg(to_jsonb(subject) order by id)
       from timegenie.subjects subject
       where workspace_id='20000000-0000-0000-0000-00000000000a'
     )
     or baseline.epoch_value<>(select workspace_epoch from timegenie.workspaces where id='20000000-0000-0000-0000-00000000000a')
     or baseline.snapshot_count<>(select count(*) from timegenie.workspace_snapshots where workspace_id='20000000-0000-0000-0000-00000000000a' and snapshot_type='pre_replace')
     or baseline.reset_count<>(select count(*) from timegenie.workspace_changes where workspace_id='20000000-0000-0000-0000-00000000000a' and entity_type='workspace_reset')
     or exists(select 1 from timegenie.workspace_restore_locks where workspace_id='20000000-0000-0000-0000-00000000000a') then
    raise exception 'failed data copy replacement did not roll back completely';
  end if;
end $$;

-- Cloud-to-local copy preview and export are frozen from authoritative rows,
-- contain every catalog entity, and exclude credential/path/device metadata.
select set_config(
  'request.headers',
  '{"x-timegenie-workspace-epoch":"2","x-timegenie-client-schema-generation":"1"}',
  true
);
insert into timegenie.app_settings(workspace_id,key,value_json,version) values
  ('20000000-0000-0000-0000-00000000000a','copy_export_safe_setting','{"theme":"dark","apiToken":"must-not-export"}',1),
  ('20000000-0000-0000-0000-00000000000a','daily_report_directory','"C:/private/reports"',1)
on conflict(workspace_id,key) do update set value_json=excluded.value_json;
create or replace function timegenie.test_only_data_copy_sha256(p_value text)
returns text language sql immutable security definer
set search_path=extensions,pg_catalog as $$
  select encode(extensions.digest(convert_to(p_value,'utf8'),'sha256'),'hex');
$$;
revoke all on function timegenie.test_only_data_copy_sha256(text) from public,anon;
grant execute on function timegenie.test_only_data_copy_sha256(text) to authenticated;
set role authenticated;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000003',true);
select set_config('request.jwt.claim.session_id','80000000-0000-0000-0000-00000000000a',true);
do $$
declare
  workspace_value uuid:='20000000-0000-0000-0000-00000000000a';
  device_value uuid:='30000000-0000-0000-0000-00000000000a';
  preview_value jsonb; manifest_value jsonb; chunk_value jsonb; discard_value jsonb;
  package_value uuid; chunk_count_value integer; chunk_index_value integer;
  content_hash_value text; hash_chain_value text:=''; all_text text:='';
begin
  preview_value:=timegenie.data_copy_export_preview(workspace_value,device_value,2);
  if (select count(*) from jsonb_object_keys(preview_value->'entityCounts'))<>22
     or (preview_value->'entityCounts'->>'subjects')::integer<1 then
    raise exception 'cloud-to-local preview entity counts are incomplete: %',preview_value;
  end if;
  manifest_value:=timegenie.data_copy_export_begin(workspace_value,device_value,2);
  package_value:=(manifest_value->>'packageId')::uuid;
  chunk_count_value:=(manifest_value->>'chunkCount')::integer;
  if chunk_count_value<22 or (select count(*) from jsonb_object_keys(manifest_value->'entityCounts'))<>22 then
    raise exception 'cloud export manifest is incomplete: %',manifest_value;
  end if;
  for chunk_index_value in 0..chunk_count_value-1 loop
    chunk_value:=timegenie.data_copy_export_chunk_get(
      workspace_value,device_value,package_value,chunk_index_value
    );
    if timegenie.test_only_data_copy_sha256(chunk_value->>'contentText')<>chunk_value->>'contentHash' then
      raise exception 'cloud export chunk hash is invalid';
    end if;
    hash_chain_value:=hash_chain_value||(chunk_value->>'contentHash');
    all_text:=all_text||(chunk_value->>'contentText');
  end loop;
  content_hash_value:=timegenie.test_only_data_copy_sha256(hash_chain_value);
  if content_hash_value<>manifest_value->>'contentHash' then
    raise exception 'cloud export package hash is invalid';
  end if;
  if all_text ilike '%must-not-export%'
     or all_text ilike '%C:/private/reports%'
     or all_text ilike '%auth_session_id%'
     or all_text ilike '%access_token%'
     or all_text ilike '%operation_id%'
     or all_text ilike '%sync_outbox%' then
    raise exception 'cloud export leaked excluded metadata: %',all_text;
  end if;
  discard_value:=timegenie.data_copy_export_discard(workspace_value,device_value,package_value);
  if coalesce((discard_value->>'discarded')::boolean,false) is not true then
    raise exception 'cloud export package was not discarded: %',discard_value;
  end if;
end $$;
reset role;
drop function timegenie.test_only_data_copy_sha256(text);

-- Direct-v2 local-file import and spreadsheet export use cloud authority, are
-- idempotent, and keep duplicate-source detection in Supabase.
insert into timegenie.subjects(id,workspace_id,name,sort_order) values(
  '90000000-0000-0000-0000-000000000031',
  '20000000-0000-0000-0000-00000000000a',
  '导入导出验收主体',31
);
set role authenticated;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000003',true);
select set_config('request.jwt.claim.session_id','80000000-0000-0000-0000-00000000000a',true);
do $$
declare workspace_value uuid:='20000000-0000-0000-0000-00000000000a';
declare device_value uuid:='30000000-0000-0000-0000-00000000000a';
declare import_value jsonb; retry_value jsonb; duplicate_value jsonb; export_value jsonb;
declare payload_value jsonb:=jsonb_build_object(
  'batchId','90000000-0000-0000-0000-000000000032',
  'subjectId','90000000-0000-0000-0000-000000000031',
  'sourceDate','2026-10-23','targetKind','day_instance','sourcePathHash','path-hash',
  'items',jsonb_build_array(
    jsonb_build_object('id','90000000-0000-0000-0000-000000000033','parentId',null,'title','日报父项','status','open','estimateMinutes',null,'importActualMinutes',null,'sortOrder',0,'sourceFingerprint','parent-fingerprint','selected',false),
    jsonb_build_object('id','90000000-0000-0000-0000-000000000034','parentId','90000000-0000-0000-0000-000000000033','title','日报完成项','status','done','estimateMinutes',30,'importActualMinutes',25,'sortOrder',1,'sourceFingerprint','child-fingerprint','selected',true)
  )
);
begin
  import_value:=timegenie.direct_cloud_daily_report_import_confirm(
    workspace_value,device_value,'90000000-0000-0000-0000-000000000035',2,payload_value);
  retry_value:=timegenie.direct_cloud_daily_report_import_confirm(
    workspace_value,device_value,'90000000-0000-0000-0000-000000000035',2,payload_value);
  if import_value is distinct from retry_value
     or (import_value->'data'->>'createdCount')::integer<>2
     or (import_value->'data'->>'adjustmentCount')::integer<>1
     or import_value->'data'->'tasks'->1->>'status' is distinct from 'done' then
    raise exception 'direct daily-report import was not atomic/idempotent: %',import_value;
  end if;
  duplicate_value:=timegenie.direct_cloud_daily_report_imported_fingerprints(
    workspace_value,device_value,'90000000-0000-0000-0000-000000000031','2026-10-23','day_instance',
    'path-hash',array['missing','child-fingerprint'],2);
  if duplicate_value->'data'->'fingerprints' is distinct from '["child-fingerprint"]'::jsonb then
    raise exception 'direct daily-report duplicate query is invalid: %',duplicate_value;
  end if;
  export_value:=timegenie.direct_cloud_time_allocation_export(
    workspace_value,device_value,'2026-10-23','2026-10-23',false,2);
  if jsonb_array_length(export_value->'data'->'rows')<>1
     or (export_value->'data'->'rows'->0->>'minutes')::integer<>25
     or export_value->'data'->'rows'->0->>'task' is distinct from '日报父项 / 日报完成项' then
    raise exception 'direct time-allocation export did not use cloud authority: %',export_value;
  end if;
end $$;
reset role;

-- Emergency read-only keeps authoritative queries available while rejecting
-- direct mutations, legacy sync writes and lease updates at the database layer.
update timegenie.workspaces
set emergency_read_only=true,emergency_read_only_reason='发布保护演练'
where id='20000000-0000-0000-0000-00000000000a';
set role authenticated;
select set_config('request.jwt.claim.sub','10000000-0000-0000-0000-000000000003',true);
select set_config('request.jwt.claim.session_id','80000000-0000-0000-0000-00000000000a',true);
do $$
declare context_value jsonb; query_value jsonb; rejected boolean:=false;
begin
  context_value:=timegenie.direct_cloud_context(
    '20000000-0000-0000-0000-00000000000a',
    '30000000-0000-0000-0000-00000000000a',2
  );
  if coalesce((context_value->>'writable')::boolean,true)
     or context_value->>'readOnlyReason' is distinct from '发布保护演练' then
    raise exception 'emergency read-only was not exposed by direct context: %',context_value;
  end if;
  query_value:=timegenie.direct_cloud_task_list(
    '20000000-0000-0000-0000-00000000000a',
    '30000000-0000-0000-0000-00000000000a',2,null,false,null,null
  );
  if query_value->'metadata'->>'writable' is distinct from 'false' then
    raise exception 'emergency read-only blocked or mislabeled queries: %',query_value;
  end if;
  begin
    perform timegenie.direct_cloud_subject_create(
      '20000000-0000-0000-0000-00000000000a',
      '30000000-0000-0000-0000-00000000000a',
      '4e000000-0000-0000-0000-00000000f001',2,
      '{"id":"51000000-0000-0000-0000-00000000f001","name":"不应写入"}'::jsonb
    );
  exception when others then
    rejected:=sqlerrm like 'EMERGENCY_READ_ONLY:%';
  end;
  if not rejected then raise exception 'emergency read-only accepted a direct mutation'; end if;
end $$;
reset role;
update timegenie.workspaces
set emergency_read_only=false,emergency_read_only_reason=null
where id='20000000-0000-0000-0000-00000000000a';

select 'Supabase schema integration tests passed' as result;

rollback;
