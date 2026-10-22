import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import net from "node:net";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, "..");
const dataDir = path.join(repoRoot, ".tmp-supabase-schema-pg-data");
const logFile = path.join(repoRoot, ".tmp-supabase-schema-pg.log");
const schemaFile = path.join(repoRoot, "supabase", "schema.sql");
const migrationPatchFile = path.join(repoRoot, "supabase", "20261003_timegenie_recurring_migration_patch.sql");
const cloudSyncPatchFile = path.join(repoRoot, "supabase", "20261004_timegenie_cloud_sync_patch.sql");
const sessionRevocationPatchFile = path.join(repoRoot, "supabase", "20261004d_timegenie_session_revocation_patch.sql");
const timerSyncPatchFile = path.join(repoRoot, "supabase", "20261005_timegenie_timer_sync_coordination_patch.sql");
const taskPlanningPatchFile = path.join(repoRoot, "supabase", "20261005_timegenie_cloud_task_planning_patch.sql");
const eventDrivenSyncPatchFile = path.join(repoRoot, "supabase", "20261005b_timegenie_event_driven_sync_patch.sql");
const sharedUnassignedPatchFile = path.join(repoRoot, "supabase", "20261006_timegenie_shared_unassigned_patch.sql");
const freshPlanningTestFile = path.join(repoRoot, "supabase", "schema_fresh_planning_test.sql");
const schemaTestFile = path.join(repoRoot, "supabase", "schema_integration_test.sql");
const calendarDayPatchFile = path.join(repoRoot, "supabase", "20261006b_timegenie_calendar_day_accounting_patch.sql");
const unassignedSegmentBoundaryPatchFile = path.join(repoRoot, "supabase", "20261007_timegenie_unassigned_segment_boundary_fix.sql");
const simplifiedUnassignedAnchorPatchFile = path.join(repoRoot, "supabase", "20261008_timegenie_simplified_unassigned_anchor.sql");
const unassignedLeaseContinuityPatchFile = path.join(repoRoot, "supabase", "20261013_timegenie_unassigned_lease_continuity_fix.sql");
const timerSyncAuthorityPatchFile = path.join(repoRoot, "supabase", "20261014_timegenie_timer_sync_authority.sql");
const timerWriteLeaseTakeoverPatchFile = path.join(repoRoot, "supabase", "20261018_timegenie_timer_write_lease_takeover.sql");
const automaticSnapshotSchedulePatchFile = path.join(repoRoot, "supabase", "20261018g_timegenie_automatic_snapshot_schedule.sql");
const operationHistoryPageSizePatchFile = path.join(repoRoot, "supabase", "20261018h_timegenie_operation_history_page_size.sql");
const historyDetailAndSnapshotTimeoutPatchFile = path.join(repoRoot, "supabase", "20261018i_timegenie_history_detail_and_snapshot_timeout.sql");
const snapshotRetentionPatchFile = path.join(repoRoot, "supabase", "20261018j_timegenie_snapshot_retention.sql");
const fixedDailySnapshotPatchFile = path.join(repoRoot, "supabase", "20261018k_timegenie_fixed_daily_snapshot.sql");
const snapshotMetadataDetailPatchFile = path.join(repoRoot, "supabase", "20261018l_timegenie_snapshot_metadata_detail.sql");
const restoreIgnoreUnassignedPatchFile = path.join(repoRoot, "supabase", "20261018m_timegenie_restore_ignore_unassigned.sql");
const restoreTimeoutPatchFile = path.join(repoRoot, "supabase", "20261018n_timegenie_restore_timeout.sql");
const optionalPreRestoreSnapshotPatchFile = path.join(repoRoot, "supabase", "20261018o_timegenie_optional_pre_restore_snapshot.sql");
const restoreUnassignedStatePatchFile = path.join(repoRoot, "supabase", "20261018p_timegenie_restore_unassigned_state.sql");
const timerFactConvergencePatchFile = path.join(repoRoot, "supabase", "20261018f_timegenie_timer_fact_convergence.sql");
const actualAdjustmentBatchPatchFile = path.join(repoRoot, "supabase", "20261015_timegenie_actual_adjustment_batch.sql");
const actualAdjustmentTargetRebasePatchFile = path.join(repoRoot, "supabase", "20261020_timegenie_actual_adjustment_target_rebase.sql");
const actualAdjustmentSnapshotPatchFile = path.join(repoRoot, "supabase", "20261020b_timegenie_actual_adjustment_snapshot.sql");
const namedTemplateBootstrapFixFile = path.join(repoRoot, "supabase", "20261016_timegenie_named_template_bootstrap_fix.sql");
const timerTransitionPatchFile = path.join(repoRoot, "supabase", "20261016b_timegenie_timer_transition.sql");
const timerRecurringOccurrencePatchFile = path.join(repoRoot, "supabase", "20261019_timegenie_timer_recurring_occurrence.sql");
const todayTaskInstancesPatchFile = path.join(repoRoot, "supabase", "20261021_timegenie_today_task_instances.sql");
const clientWriteGenerationPatchFile = path.join(repoRoot, "supabase", "20261021b_timegenie_client_write_generation.sql");
const todayInstanceProtocolPatchFile = path.join(repoRoot, "supabase", "20261022b_timegenie_today_instance_protocol.sql");
const snapshotTaskOrderPatchFile = path.join(repoRoot, "supabase", "20261022d_timegenie_snapshot_task_order.sql");
const dailyReportCollectionTemplatesPatchFile = path.join(repoRoot, "supabase", "20261021c_timegenie_daily_report_collection_templates.sql");

const binaryNames = process.platform === "win32"
  ? {
      initdb: "initdb.exe",
      pgCtl: "pg_ctl.exe",
      psql: "psql.exe",
    }
  : {
      initdb: "initdb",
      pgCtl: "pg_ctl",
      psql: "psql",
    };

const candidateBinDirs = [
  process.env.PG_BIN_DIR,
  process.platform === "win32" ? "C:\\Program Files\\PostgreSQL\\17\\bin" : undefined,
  process.platform === "win32" ? "C:\\Program Files\\PostgreSQL\\16\\bin" : undefined,
  process.platform === "win32" ? "C:\\Program Files\\PostgreSQL\\15\\bin" : undefined,
].filter(Boolean);

function commandExists(command) {
  const probe = process.platform === "win32" ? "where" : "command";
  const args = process.platform === "win32" ? [command] : ["-v", command];
  return spawnSync(probe, args, { stdio: "ignore", shell: process.platform !== "win32" }).status === 0;
}

function resolveBinary(name) {
  for (const dir of candidateBinDirs) {
    const fullPath = path.join(dir, name);
    if (fs.existsSync(fullPath)) return fullPath;
  }
  if (commandExists(name)) return name;
  throw new Error(`PostgreSQL binary not found: ${name}. Set PG_BIN_DIR to the PostgreSQL bin directory.`);
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: repoRoot,
    env: { ...process.env, PGCLIENTENCODING: "UTF8", ...options.env },
    encoding: "utf8",
    stdio: options.capture ? "pipe" : "inherit",
  });
  if (result.status !== 0) {
    const detail = options.capture ? `\n${result.stdout ?? ""}${result.stderr ?? ""}` : "";
    throw new Error(`${path.basename(command)} failed with exit code ${result.status}.${detail}`);
  }
  return result;
}

function ensureInsideRepo(targetPath) {
  const relative = path.relative(repoRoot, targetPath);
  if (relative.startsWith("..") || path.isAbsolute(relative)) {
    throw new Error(`Refusing to use path outside repo: ${targetPath}`);
  }
}

function verifyStaticSchemaInvariants() {
  const schema = fs.readFileSync(schemaFile, "utf8");
  const migrationPatch = fs.readFileSync(migrationPatchFile, "utf8");
  const cloudSyncPatch = fs.readFileSync(cloudSyncPatchFile, "utf8");
  const sessionRevocationPatch = fs.readFileSync(sessionRevocationPatchFile, "utf8");
  const timerSyncPatch = fs.readFileSync(timerSyncPatchFile, "utf8");
  const taskPlanningPatch = fs.readFileSync(taskPlanningPatchFile, "utf8");
  const eventDrivenSyncPatch = fs.readFileSync(eventDrivenSyncPatchFile, "utf8");
  const sharedUnassignedPatch = fs.readFileSync(sharedUnassignedPatchFile, "utf8");
  const calendarDayPatch = fs.readFileSync(calendarDayPatchFile, "utf8");
  const unassignedSegmentBoundaryPatch = fs.readFileSync(unassignedSegmentBoundaryPatchFile, "utf8");
  const simplifiedUnassignedAnchorPatch = fs.readFileSync(simplifiedUnassignedAnchorPatchFile, "utf8");
  const unassignedLeaseContinuityPatch = fs.readFileSync(unassignedLeaseContinuityPatchFile, "utf8");
  const timerSyncAuthorityPatch = fs.readFileSync(timerSyncAuthorityPatchFile, "utf8");
  const timerWriteLeaseTakeoverPatch = fs.readFileSync(timerWriteLeaseTakeoverPatchFile, "utf8");
  const automaticSnapshotSchedulePatch = fs.readFileSync(automaticSnapshotSchedulePatchFile, "utf8");
  const operationHistoryPageSizePatch = fs.readFileSync(operationHistoryPageSizePatchFile, "utf8");
  const historyDetailAndSnapshotTimeoutPatch = fs.readFileSync(historyDetailAndSnapshotTimeoutPatchFile, "utf8");
  const snapshotRetentionPatch = fs.readFileSync(snapshotRetentionPatchFile, "utf8");
  const fixedDailySnapshotPatch = fs.readFileSync(fixedDailySnapshotPatchFile, "utf8");
  const snapshotMetadataDetailPatch = fs.readFileSync(snapshotMetadataDetailPatchFile, "utf8");
  const restoreIgnoreUnassignedPatch = fs.readFileSync(restoreIgnoreUnassignedPatchFile, "utf8");
  const restoreTimeoutPatch = fs.readFileSync(restoreTimeoutPatchFile, "utf8");
  const optionalPreRestoreSnapshotPatch = fs.readFileSync(optionalPreRestoreSnapshotPatchFile, "utf8");
  const restoreUnassignedStatePatch = fs.readFileSync(restoreUnassignedStatePatchFile, "utf8");
  const timerFactConvergencePatch = fs.readFileSync(timerFactConvergencePatchFile, "utf8");
  const actualAdjustmentBatchPatch = fs.readFileSync(actualAdjustmentBatchPatchFile, "utf8");
  const actualAdjustmentTargetRebasePatch = fs.readFileSync(actualAdjustmentTargetRebasePatchFile, "utf8");
  const actualAdjustmentSnapshotPatch = fs.readFileSync(actualAdjustmentSnapshotPatchFile, "utf8");
  const namedTemplateBootstrapFix = fs.readFileSync(namedTemplateBootstrapFixFile, "utf8");
  const timerTransitionPatch = fs.readFileSync(timerTransitionPatchFile, "utf8");
  const timerRecurringOccurrencePatch = fs.readFileSync(timerRecurringOccurrencePatchFile, "utf8");
  const todayTaskInstancesPatch = fs.readFileSync(todayTaskInstancesPatchFile, "utf8");
  const clientWriteGenerationPatch = fs.readFileSync(clientWriteGenerationPatchFile, "utf8");
  const todayInstanceProtocolPatch = fs.readFileSync(todayInstanceProtocolPatchFile, "utf8");
  const snapshotTaskOrderPatch = fs.readFileSync(snapshotTaskOrderPatchFile, "utf8");
  const dailyReportCollectionTemplatesPatch = fs.readFileSync(dailyReportCollectionTemplatesPatchFile, "utf8");
  const schemaIntegrationTest = fs.readFileSync(schemaTestFile, "utf8");
  for (const expected of [
    "day_instance_date",
    "day_instance_kind",
    "day_instance_source_task_id",
    "instance_task_id",
    "validate_task_day_instance_scope",
  ]) {
    if (!todayTaskInstancesPatch.includes(expected)) {
      throw new Error(`The today-task instance patch must include: ${expected}`);
    }
  }
  for (const expected of [
    "minimum_client_schema_generation",
    "x-timegenie-client-schema-generation",
    "CLIENT_UPGRADE_REQUIRED",
    "client_schema_generation_write_barrier",
    "workspace_restore_commit_pre_client_generation",
  ]) {
    if (!clientWriteGenerationPatch.includes(expected)) {
      throw new Error(`The client write-generation patch must include: ${expected}`);
    }
  }
  if (!schema.includes("\\ir 20261021_timegenie_today_task_instances.sql")
      || !schema.includes("\\ir 20261021b_timegenie_client_write_generation.sql")) {
    throw new Error("The source schema must apply today-task instances and the compatible-client write barrier.");
  }
  for (const expected of [
    "apply_today_task_patch",
    "day_instance_source_task_id",
    "instance_task_id=excluded.instance_task_id",
    "migration_import_snapshot_pre_today_instance_protocol",
    "validate_task_day_plan_instance",
  ]) {
    if (!todayInstanceProtocolPatch.includes(expected)) {
      throw new Error(`The today-instance protocol patch must include: ${expected}`);
    }
  }
  if (!schema.includes("\\ir 20261022b_timegenie_today_instance_protocol.sql")) {
    throw new Error("The source schema must finish with the complete today-instance protocol patch.");
  }
  for (const expected of ["cloud_snapshot_get_pre_task_order", "with recursive ordered_tasks", "tree_depth"]) {
    if (!snapshotTaskOrderPatch.includes(expected)) {
      throw new Error(`The snapshot task-order patch must include: ${expected}`);
    }
  }
  if (!schema.includes("\\ir 20261022d_timegenie_snapshot_task_order.sql")) {
    throw new Error("The source schema must finish with stable snapshot task ordering.");
  }
  if ((schemaIntegrationTest.match(/\\ir 20261022d_timegenie_snapshot_task_order\.sql/g) ?? []).length < 2) {
    throw new Error("The schema integration test must prove the snapshot task-order patch is idempotent.");
  }
  if ((schemaIntegrationTest.match(/\\ir 20261022b_timegenie_today_instance_protocol\.sql/g) ?? []).length < 2) {
    throw new Error("The schema integration test must prove the today-instance protocol patch is idempotent.");
  }
  if ((schemaIntegrationTest.match(/\\ir 20261021_timegenie_today_task_instances\.sql/g) ?? []).length < 2
      || (schemaIntegrationTest.match(/\\ir 20261021b_timegenie_client_write_generation\.sql/g) ?? []).length < 2) {
    throw new Error("The schema integration test must prove the today-instance and client-generation patches are idempotent.");
  }
  if (!schemaIntegrationTest.includes("old client write was accepted")
      || !schemaIntegrationTest.includes("CLIENT_UPGRADE_REQUIRED")) {
    throw new Error("The schema integration test must reject a write without the compatible-client handshake.");
  }
  for (const expected of [
    "occurrence_row:=p_payload->'occurrence'",
    "timer occurrence identity mismatch",
    "'occurrence',case when occurrence_row is null",
    "day_plan_row:=p_payload->'day_plan'",
    "timer day plan must be an included initial version",
    "'dayPlan',case when day_plan_row is null",
  ]) {
    if (!timerRecurringOccurrencePatch.includes(expected)) {
      throw new Error(`The recurring timer occurrence patch must include: ${expected}`);
    }
  }
  if (!schema.includes("\\ir 20261019_timegenie_timer_recurring_occurrence.sql")) {
    throw new Error("The source schema must apply the recurring timer occurrence patch.");
  }
  if ((schemaIntegrationTest.match(/\\ir 20261019_timegenie_timer_recurring_occurrence\.sql/g) ?? []).length < 2) {
    throw new Error("The schema integration test must prove the recurring timer occurrence patch is idempotent.");
  }
  for (const [fileName, contents, expected] of [
    ["workspace snapshots", fs.readFileSync(path.join(repoRoot, "supabase", "20261018c_timegenie_workspace_snapshots.sql"), "utf8"), "('task_day_plans','planning'"],
    ["workspace epoch", fs.readFileSync(path.join(repoRoot, "supabase", "20261018d_timegenie_workspace_epoch.sql"), "utf8"), "'task_day_plans'"],
    ["workspace restore", fs.readFileSync(path.join(repoRoot, "supabase", "20261018e_timegenie_workspace_restore.sql"), "utf8"), "delete from timegenie.task_day_plans"],
    ["final workspace restore", restoreUnassignedStatePatch, "delete from timegenie.task_day_plans"],
  ]) {
    if (!contents.includes(expected)) {
      throw new Error(`The ${fileName} patch must preserve task_day_plans support.`);
    }
  }
  for (const expected of [
    "automatic_snapshot_frequency",
    "schedule_slot_key",
    "create or replace function timegenie.workspace_snapshot_settings_get",
    "create or replace function timegenie.workspace_snapshot_settings_update",
    "create or replace function timegenie.workspace_snapshot_ensure_automatic",
    "uq_workspace_snapshots_automatic_slot",
  ]) {
    if (!automaticSnapshotSchedulePatch.replaceAll(" ", "").toLowerCase().includes(expected.replaceAll(" ", "").toLowerCase())) {
      throw new Error(`The automatic snapshot schedule patch must include: ${expected}`);
    }
  }
  if (!schema.includes("\\ir 20261018g_timegenie_automatic_snapshot_schedule.sql")) {
    throw new Error("The source schema must apply the automatic snapshot schedule patch.");
  }
  if ((schemaIntegrationTest.match(/\\ir 20261018g_timegenie_automatic_snapshot_schedule\.sql/g) ?? []).length < 2) {
    throw new Error("The schema integration test must prove the automatic snapshot schedule patch is idempotent.");
  }
  if (!operationHistoryPageSizePatch.includes("p_limit integer default 50")
      || !operationHistoryPageSizePatch.includes("coalesce(p_limit,50)")) {
    throw new Error("The operation history page-size patch must default to 50 rows.");
  }
  if (!schema.includes("\\ir 20261018h_timegenie_operation_history_page_size.sql")) {
    throw new Error("The source schema must apply the operation history 50-row page-size patch.");
  }
  if ((schemaIntegrationTest.match(/\\ir 20261018h_timegenie_operation_history_page_size\.sql/g) ?? []).length < 2) {
    throw new Error("The schema integration test must prove the operation history page-size patch is idempotent.");
  }
  for (const expected of [
    "'operationId',log.operation_id",
    "'deviceNameSnapshot',log.device_name_snapshot",
    "alter function timegenie.workspace_snapshot_create",
    "alter function timegenie.workspace_snapshot_ensure_automatic_at",
    "alter function timegenie.workspace_snapshot_ensure_automatic",
    "set statement_timeout='120s'",
  ]) {
    if (!historyDetailAndSnapshotTimeoutPatch.includes(expected)) {
      throw new Error(`The history detail and snapshot timeout patch must include: ${expected}`);
    }
  }
  if (!schema.includes("\\ir 20261018i_timegenie_history_detail_and_snapshot_timeout.sql")) {
    throw new Error("The source schema must apply the history detail and snapshot timeout patch.");
  }
  if ((schemaIntegrationTest.match(/\\ir 20261018i_timegenie_history_detail_and_snapshot_timeout\.sql/g) ?? []).length < 2) {
    throw new Error("The schema integration test must prove the history detail and snapshot timeout patch is idempotent.");
  }
  for (const expected of [
    "snapshot_retention_days",
    "workspace_snapshot_apply_retention",
    "before insert or update of protected,created_at",
    "workspace_snapshot_settings_update_with_retention",
    "snapshotRetentionDays",
    "expires_at",
    "where protected and expires_at is not null",
    "set statement_timeout='120s'",
  ]) {
    if (!snapshotRetentionPatch.includes(expected)) {
      throw new Error(`The snapshot retention patch must include: ${expected}`);
    }
  }
  if (!schema.includes("\\ir 20261018j_timegenie_snapshot_retention.sql")) {
    throw new Error("The source schema must apply the snapshot retention patch.");
  }
  if ((schemaIntegrationTest.match(/\\ir 20261018j_timegenie_snapshot_retention\.sql/g) ?? []).length < 2) {
    throw new Error("The schema integration test must prove the snapshot retention patch is idempotent.");
  }
  for (const expected of [
    "automatic_snapshot_enabled",
    "automatic_snapshot_time",
    "drop column automatic_snapshot_frequency",
    "workspace_snapshot_run_scheduled",
    "workspace_snapshot_next_scheduled_at",
    "cron.schedule",
    "timegenie-daily-automatic-snapshot",
    "workspace_snapshot_settings_update(uuid,uuid,uuid,boolean,time,integer)",
    "drop function if exists timegenie.workspace_snapshot_ensure_automatic",
    "set statement_timeout='600s'",
  ]) {
    if (!fixedDailySnapshotPatch.replaceAll(" ", "").toLowerCase().includes(expected.replaceAll(" ", "").toLowerCase())) {
      throw new Error(`The fixed daily snapshot patch must include: ${expected}`);
    }
  }
  if (!schema.includes("\\ir 20261018k_timegenie_fixed_daily_snapshot.sql")) {
    throw new Error("The source schema must apply the fixed daily snapshot patch.");
  }
  if ((schemaIntegrationTest.match(/\\ir 20261018k_timegenie_fixed_daily_snapshot\.sql/g) ?? []).length < 2) {
    throw new Error("The schema integration test must prove the fixed daily snapshot patch is idempotent.");
  }
  for (const expected of [
    "workspace_snapshot_verify_integrity",
    "groupCounts",
    "CHUNK_CHECKSUM_MISMATCH",
    "verification_value:=timegenie.workspace_snapshot_verify_integrity",
  ]) {
    if (!snapshotMetadataDetailPatch.replaceAll(" ", "").toLowerCase().includes(expected.replaceAll(" ", "").toLowerCase())) {
      throw new Error(`The snapshot metadata detail patch must include: ${expected}`);
    }
  }
  if (snapshotMetadataDetailPatch.includes("cloud_snapshot_get(p_workspace_id)")
      || snapshotMetadataDetailPatch.includes("workspace_snapshot_rehydrate(p_snapshot_id)")) {
    throw new Error("Opening snapshot details must not reconstruct or compare full workspace data.");
  }
  if (!schema.includes("\\ir 20261018l_timegenie_snapshot_metadata_detail.sql")) {
    throw new Error("The source schema must apply the snapshot metadata detail patch.");
  }
  if ((schemaIntegrationTest.match(/\\ir 20261018l_timegenie_snapshot_metadata_detail\.sql/g) ?? []).length < 2) {
    throw new Error("The schema integration test must prove the snapshot metadata detail patch is idempotent.");
  }
  if (!restoreIgnoreUnassignedPatch.includes("create or replace function timegenie.workspace_restore_prepare")
      || restoreIgnoreUnassignedPatch.includes("RESTORE_ACTIVE_UNASSIGNED")) {
    throw new Error("Workspace restore must replace active unassigned time without blocking.");
  }
  if (!schema.includes("\\ir 20261018m_timegenie_restore_ignore_unassigned.sql")) {
    throw new Error("The source schema must apply the restore-ignore-unassigned patch.");
  }
  if ((schemaIntegrationTest.match(/\\ir 20261018m_timegenie_restore_ignore_unassigned\.sql/g) ?? []).length < 2) {
    throw new Error("The schema integration test must prove the restore-ignore-unassigned patch is idempotent.");
  }
  for (const signature of [
    "workspace_restore_prepare(uuid,uuid,uuid,bigint)",
    "workspace_restore_commit(uuid,uuid,uuid,uuid,text,bigint,uuid,boolean)",
  ]) {
    if (!restoreTimeoutPatch.replace(/\s/g, "").includes(`alterfunctiontimegenie.${signature.replace(/\s/g, "")}`)
        || !restoreTimeoutPatch.includes("statement_timeout='600s'")) {
      throw new Error(`Workspace restore RPC is missing the extended timeout: ${signature}`);
    }
  }
  if (!schema.includes("\\ir 20261018n_timegenie_restore_timeout.sql")) {
    throw new Error("The source schema must apply the workspace restore timeout patch.");
  }
  if ((schemaIntegrationTest.match(/\\ir 20261018n_timegenie_restore_timeout\.sql/g) ?? []).length < 2) {
    throw new Error("The schema integration test must prove the restore-timeout patch is idempotent.");
  }
  for (const expected of [
    "p_create_safety_snapshot boolean",
    "if coalesce(p_create_safety_snapshot,false) then",
    "workspace_restore_prepare($1,$2,$3,$4,false)",
    "safetySnapshotId",
  ]) {
    if (!optionalPreRestoreSnapshotPatch.replace(/\s/g, "").toLowerCase().includes(expected.replace(/\s/g, "").toLowerCase())) {
      throw new Error(`Optional pre-restore snapshot patch must include: ${expected}`);
    }
  }
  if (!schema.includes("\\ir 20261018o_timegenie_optional_pre_restore_snapshot.sql")) {
    throw new Error("The source schema must apply the optional pre-restore snapshot patch.");
  }
  if ((schemaIntegrationTest.match(/\\ir 20261018o_timegenie_optional_pre_restore_snapshot\.sql/g) ?? []).length < 2) {
    throw new Error("The schema integration test must prove the optional pre-restore snapshot patch is idempotent.");
  }
  for (const expected of [
    "state='awaiting_resolution'",
    "update timegenie.unassigned_segments",
    "lease_token=null",
  ]) {
    if (!restoreUnassignedStatePatch.replace(/\s/g, "").toLowerCase().includes(expected.replace(/\s/g, "").toLowerCase())) {
      throw new Error(`Restore unassigned-state patch must include: ${expected}`);
    }
  }
  if (restoreUnassignedStatePatch.includes("unassigned_sessions set state='paused'")) {
    throw new Error("Workspace restore must not write the invalid paused unassigned-session state.");
  }
  if (!schema.includes("\\ir 20261018p_timegenie_restore_unassigned_state.sql")) {
    throw new Error("The source schema must apply the restore unassigned-state patch.");
  }
  if ((schemaIntegrationTest.match(/\\ir 20261018p_timegenie_restore_unassigned_state\.sql/g) ?? []).length < 2) {
    throw new Error("The schema integration test must prove the restore unassigned-state patch is idempotent.");
  }
  for (const expected of [
    "create or replace function timegenie.timer_transition_apply",
    "expected_parent_version",
    "VERSION_CONFLICT: remote timer version changed",
    "grant execute on function timegenie.timer_transition_apply(uuid,uuid,uuid,jsonb) to authenticated",
  ]) {
    if (!schema.replaceAll(" ", "").toLowerCase().includes(expected.replaceAll(" ", "").toLowerCase())
        || !timerTransitionPatch.replaceAll(" ", "").toLowerCase().includes(expected.replaceAll(" ", "").toLowerCase())) {
      throw new Error(`The schema and timer transition patch must include: ${expected}`);
    }
  }
  if ((schemaIntegrationTest.match(/\\ir 20261016b_timegenie_timer_transition\.sql/g) ?? []).length < 2) {
    throw new Error("The schema integration test must prove the timer transition patch is idempotent.");
  }
  const importStart = schema.indexOf("create or replace function timegenie.migration_import_snapshot");
  const applyPatchStart = schema.indexOf("create or replace function timegenie.cloud_apply_patch");
  if (importStart < 0 || applyPatchStart < 0 || applyPatchStart <= importStart) {
    throw new Error("Could not locate migration_import_snapshot in supabase/schema.sql.");
  }
  const migrationImport = schema.slice(importStart, applyPatchStart);
  const cloudApplyPatch = schema.slice(applyPatchStart);
  if (!migrationImport.includes("nullif(row_data->>'origin_unassigned_session_id','')::uuid")) {
    throw new Error("migration_import_snapshot must preserve time_entries.origin_unassigned_session_id from local snapshots.");
  }
  if (!migrationImport.includes("exists(select 1 from timegenie.work_days where workspace_id = p_workspace_id)")
      || !migrationImport.includes("exists(select 1 from timegenie.unassigned_sessions where workspace_id = p_workspace_id)")) {
    throw new Error("migration_import_snapshot must treat work_days and unassigned_sessions as business data when checking for an empty target.");
  }
  if (!cloudApplyPatch.includes("nullif(row_data->>'origin_unassigned_session_id','')::uuid")) {
    throw new Error("cloud_apply_patch must preserve time_entries.origin_unassigned_session_id from local entity payloads.");
  }
  if (!cloudApplyPatch.includes("origin_unassigned_session_id = excluded.origin_unassigned_session_id")) {
    throw new Error("cloud_apply_patch conflict updates must refresh time_entries.origin_unassigned_session_id.");
  }
  if (!cloudApplyPatch.includes("where timegenie.work_days.version <= excluded.version")) {
    throw new Error("cloud_apply_patch must not let an older time-entry payload overwrite a newer work-day version.");
  }
  if (!cloudApplyPatch.includes("p_entity_type = 'unassigned_session'")) {
    throw new Error("cloud_apply_patch must accept unassigned_session entity payloads.");
  }
  for (const expected of [
    "p_coalesced_count bigint",
    "final_version <> p_base_version + p_coalesced_count",
    "pg_advisory_xact_lock",
    "cloud_apply_patch(uuid, uuid, uuid, text, text, text, bigint, bigint, bigint, jsonb)",
  ]) {
    if (!schema.includes(expected) || !timerSyncPatch.includes(expected)) {
      throw new Error(`The schema and timer sync patch must include: ${expected}`);
    }
  }
  if (timerSyncPatch.includes("while current_step <= final_version")) {
    throw new Error("The timer sync patch must apply the final image atomically instead of replaying intermediate versions.");
  }
  for (const expected of [
    "task_daily_estimate",
    "task_recurrence_rule",
    "when 'task_daily_estimates' then (row_image->>'task_id') || '|' || (row_image->>'work_date')",
    "when 'task_recurrence_rules' then row_image->>'task_id'",
  ]) {
    if (!schema.includes(expected) || !taskPlanningPatch.includes(expected)) {
      throw new Error(`The schema and cloud task planning patch must include: ${expected}`);
    }
  }
  if (!taskPlanningPatch.includes("cloud_apply_task_planning_patch")) {
    throw new Error("The cloud task planning patch must define its planning apply helper.");
  }
  for (const expected of [
    "create or replace function timegenie.cloud_incremental_entity_get",
    "create or replace function timegenie.cloud_incremental_pull",
    "'covered_change_seq'",
    "'latest_change_seq'",
    "'has_more'",
    "'reset_required'",
    "jsonb_build_object('change_seq'",
    "when 'unassigned_sessions' then",
    "grant execute on function timegenie.cloud_incremental_pull(uuid,uuid,bigint,integer) to authenticated",
  ]) {
    const compactExpected = expected.replaceAll(" ", "").toLowerCase();
    if (!schema.replaceAll(" ", "").toLowerCase().includes(compactExpected)
        || !eventDrivenSyncPatch.replaceAll(" ", "").toLowerCase().includes(compactExpected)) {
      throw new Error("The schema and event-driven sync patch must include: " + expected);
    }
  }
  if (!schema.includes("revoke execute on all functions in schema timegenie from anon")
      || !eventDrivenSyncPatch.replaceAll(" ", "").toLowerCase().includes("revokeexecuteonfunctiontimegenie.cloud_incremental_pull(uuid,uuid,bigint,integer)fromanon")) {
    throw new Error("Anonymous clients must not execute cloud_incremental_pull.");
  }
  if (!schema.replaceAll(" ", "").toLowerCase().includes("revokeexecuteonfunctiontimegenie.cloud_incremental_entity_get(uuid,text,text)fromauthenticated")
      || !eventDrivenSyncPatch.replaceAll(" ", "").toLowerCase().includes("revokeexecuteonfunctiontimegenie.cloud_incremental_entity_get(uuid,text,text)frompublic,anon,authenticated")) {
    throw new Error("Clients must not execute the internal incremental entity helper.");
  }
  if ((schemaIntegrationTest.match(/\\ir 20261005b_timegenie_event_driven_sync_patch\.sql/g) ?? []).length < 2) {
    throw new Error("The schema integration test must prove the event-driven sync patch is idempotent.");
  }
  if ((schemaIntegrationTest.match(/\\ir 20261006_timegenie_shared_unassigned_patch\.sql/g) ?? []).length < 2) {
    throw new Error("The schema integration test must prove the shared unassigned patch is idempotent.");
  }
  if ((schemaIntegrationTest.match(/\\ir 20261006b_timegenie_calendar_day_accounting_patch\.sql/g) ?? []).length < 2) {
    throw new Error("The schema integration test must prove the calendar-day patch is idempotent.");
  }
  if ((schemaIntegrationTest.match(/\\ir 20261007_timegenie_unassigned_segment_boundary_fix\.sql/g) ?? []).length < 2) {
    throw new Error("The schema integration test must prove the unassigned segment boundary patch is idempotent.");
  }
  if ((schemaIntegrationTest.match(/\\ir 20261008_timegenie_simplified_unassigned_anchor\.sql/g) ?? []).length < 2) {
    throw new Error("The schema integration test must prove the simplified unassigned anchor patch is idempotent.");
  }
  if ((schemaIntegrationTest.match(/\\ir 20261013_timegenie_unassigned_lease_continuity_fix\.sql/g) ?? []).length < 2) {
    throw new Error("The schema integration test must prove the unassigned lease continuity patch is idempotent.");
  }
  if ((schemaIntegrationTest.match(/\\ir 20261014_timegenie_timer_sync_authority\.sql/g) ?? []).length < 2) {
    throw new Error("The schema integration test must prove the timer sync authority patch is idempotent.");
  }
  if ((schemaIntegrationTest.match(/\\ir 20261018f_timegenie_timer_fact_convergence\.sql/g) ?? []).length < 2) {
    throw new Error("The schema integration test must prove the timer-fact convergence patch is idempotent.");
  }
  if ((schemaIntegrationTest.match(/\\ir 20261015_timegenie_actual_adjustment_batch\.sql/g) ?? []).length < 2) {
    throw new Error("The schema integration test must prove the actual adjustment batch patch is idempotent.");
  }
  if ((schemaIntegrationTest.match(/\\ir 20261020_timegenie_actual_adjustment_target_rebase\.sql/g) ?? []).length < 2) {
    throw new Error("The schema integration test must prove the actual adjustment target-rebase patch is idempotent.");
  }
  if ((schemaIntegrationTest.match(/\\ir 20261016_timegenie_named_template_bootstrap_fix\.sql/g) ?? []).length < 2) {
    throw new Error("The schema integration test must prove the named-template bootstrap fix is idempotent.");
  }
  for (const expected of [
    "create or replace function timegenie.workspace_initialize_defaults",
    "'标准日报'",
    "'标准周报'",
    "'标准月报'",
    "subject_work_schedules",
  ]) {
    if (!namedTemplateBootstrapFix.includes(expected)) {
      throw new Error(`The named-template bootstrap fix must include: ${expected}`);
    }
  }
  if (!schema.includes("\\ir 20261016_timegenie_named_template_bootstrap_fix.sql")) {
    throw new Error("The source schema must finish with the named-template bootstrap fix.");
  }
  for (const expected of ["{{日报合集}}", "weekly", "monthly"]) {
    if (!dailyReportCollectionTemplatesPatch.includes(expected)) {
      throw new Error(`The daily-report collection template patch must include: ${expected}`);
    }
  }
  if (!schema.includes("\\ir 20261021c_timegenie_daily_report_collection_templates.sql")) {
    throw new Error("The source schema must apply the daily-report collection template patch.");
  }
  if (!schema.includes("create or replace function timegenie.actual_adjustment_fingerprint")
      || !actualAdjustmentBatchPatch.includes("create or replace function timegenie.actual_adjustment_fingerprint")) {
    throw new Error("The schema and original actual adjustment patch must retain the adjustment fingerprint helper.");
  }
  for (const expected of [
    "create or replace function timegenie.actual_adjustment_batch_apply",
    "effective_delta := (item->>'targetMinutes')::integer - current_actual",
    "discardedAdjustmentIds",
    "'adjustments',applied_adjustments",
    "existing task adjustment image does not match request",
    "new task adjustment image does not match draft",
    "grant execute on function timegenie.actual_adjustment_batch_apply(uuid,uuid,uuid,jsonb) to authenticated",
  ]) {
    const compactExpected = expected.replaceAll(" ", "").toLowerCase();
    if (!schema.replaceAll(" ", "").toLowerCase().includes(compactExpected)
        || !actualAdjustmentTargetRebasePatch.replaceAll(" ", "").toLowerCase().includes(compactExpected)) {
      throw new Error(`The schema and actual adjustment target-rebase patch must include: ${expected}`);
    }
  }
  if (!schema.includes("\\ir 20261020_timegenie_actual_adjustment_target_rebase.sql")) {
    throw new Error("The source schema must finish with the actual adjustment target-rebase patch.");
  }
  if (!schema.includes("\\ir 20261020b_timegenie_actual_adjustment_snapshot.sql")) {
    throw new Error("The source schema must include the actual-adjustment snapshot patch.");
  }
  for (const expected of [
    "cloud_snapshot_get_pre_actual_adjustments",
    "'actual_time_adjustments',coalesce((select jsonb_agg(to_jsonb(row_value) order by work_date,task_id,id)",
    "cloud_incremental_entity_get_pre_actual_adjustments",
    "migration_import_snapshot_pre_actual_adjustments",
    "source_entity_type','actual_time_adjustments'",
  ]) {
    if (!actualAdjustmentSnapshotPatch.includes(expected)) {
      throw new Error(`The actual-adjustment snapshot patch must include: ${expected}`);
    }
  }
  if (!schemaIntegrationTest.includes("failed actual adjustment batch left partial remote data")
      || !schemaIntegrationTest.includes("stale adjustment target was not rebased")) {
    throw new Error("The schema integration test must prove failed batches stay atomic and stale baselines rebase to the requested target.");
  }
  const authorityTickStart = timerSyncAuthorityPatch.indexOf("create or replace function timegenie.unassigned_tick");
  const authorityTickEnd = timerSyncAuthorityPatch.indexOf("end $$;", authorityTickStart);
  const authorityTick = timerSyncAuthorityPatch.slice(authorityTickStart, authorityTickEnd);
  if (authorityTickStart < 0 || authorityTick.includes("coordinate_timer_calendar_day")) {
    throw new Error("Tracking lease heartbeats must not coordinate ordinary timers.");
  }
  for (const expected of [
    "create or replace function timegenie.timer_sync_context_get",
    "matchesRequest",
    "'code','IDEMPOTENCY_CONFLICT'",
    "'code','SYNC_CONFLICT'",
    "create or replace function timegenie.timegenie_clamp_timer_stop_payload",
    "grant execute on function timegenie.timer_sync_context_get(uuid,uuid,uuid) to authenticated",
    "grant execute on function timegenie.timer_sync_context_get(uuid,uuid,uuid,text,jsonb) to authenticated",
    "revoke execute on function timegenie.apply_calendar_time_entry(uuid,uuid,uuid,text,text,bigint,jsonb,jsonb) from public,anon,authenticated",
    "grant execute on function timegenie.unassigned_tick(uuid,uuid,uuid) to authenticated",
    "grant execute on function timegenie.tracking_lease_acquire(uuid,uuid,uuid) to authenticated",
    "grant execute on function timegenie.tracking_lease_renew(uuid,uuid,uuid) to authenticated",
    "grant execute on function timegenie.tracking_lease_takeover_for_timer_write(uuid,uuid,uuid) to authenticated",
    "grant execute on function timegenie.tracking_lease_release(uuid,uuid,uuid) to authenticated",
  ]) {
    const timerLeaseSources = `${timerSyncAuthorityPatch}\n${timerWriteLeaseTakeoverPatch}\n${timerFactConvergencePatch}`;
    if (!timerLeaseSources.replaceAll(" ", "").toLowerCase().includes(expected.replaceAll(" ", "").toLowerCase())) {
      throw new Error(`The timer lease patches must include: ${expected}`);
    }
  }
  if (timerFactConvergencePatch.includes("LEASE_NOT_HELD")
      || timerFactConvergencePatch.includes("timer_write_lease_context(p_workspace_id,p_device_id)")) {
    throw new Error("Timer-fact writes must not require a tracking lease.");
  }
  if (!timerFactConvergencePatch.includes("'compatibilityOnly',true")
      || !timerFactConvergencePatch.includes("'takeover',false")) {
    throw new Error("The legacy timer-write takeover RPC must be a non-mutating compatibility response.");
  }
  if (!schema.includes("\\ir 20261018f_timegenie_timer_fact_convergence.sql")) {
    throw new Error("The source schema must finish with the timer-fact convergence patch.");
  }
  for (const expected of [
    "create or replace function timegenie.unassigned_anchor_reset",
    "create or replace function timegenie.timegenie_unassigned_system_clear",
    "create or replace function timegenie.timegenie_unassigned_create_anchor",
    "create or replace function timegenie.unassigned_resolve_shared",
    "create or replace function timegenie.apply_calendar_time_entry",
    "create or replace function timegenie.cloud_apply_simplified_unassigned",
    "duration_seconds=0",
    "p_operation_type='timer_start'",
    "p_operation_type='timer_stop'",
  ]) {
    if (!schema.toLowerCase().includes(expected.toLowerCase())
        || !simplifiedUnassignedAnchorPatch.toLowerCase().includes(expected.toLowerCase())) {
      throw new Error(`The schema and simplified anchor patch must include: ${expected}`);
    }
  }
  const simplifiedTickStart = simplifiedUnassignedAnchorPatch.indexOf("create or replace function timegenie.unassigned_tick");
  const simplifiedTickEnd = simplifiedUnassignedAnchorPatch.indexOf("end $$;", simplifiedTickStart);
  const simplifiedTick = simplifiedUnassignedAnchorPatch.slice(simplifiedTickStart, simplifiedTickEnd);
  if (simplifiedTick.toLowerCase().includes("update timegenie.unassigned_sessions")) {
    throw new Error("unassigned_tick must not persist natural unassigned-time growth.");
  }
  for (const source of [schema, simplifiedUnassignedAnchorPatch, unassignedLeaseContinuityPatch]) {
    const closeStart = source.lastIndexOf("create or replace function timegenie.close_expired_unassigned_lease");
    const closeEnd = source.indexOf("end $$;", closeStart);
    const closeFunction = source.slice(closeStart, closeEnd);
    if (closeStart < 0 || closeFunction.includes("timegenie_unassigned_system_clear")
        || closeFunction.includes("update timegenie.unassigned_")) {
      throw new Error("Tracking lease lifecycle must not clear or mutate the shared unassigned anchor.");
    }
  }
  for (const expected of [
    "lease takeover mutated the shared unassigned anchor",
    "lease release mutated the shared unassigned anchor",
    "lease lifecycle generated an unassigned result",
  ]) {
    if (!schemaIntegrationTest.includes(expected)) {
      throw new Error(`The schema integration test is missing lease continuity coverage: ${expected}`);
    }
  }
  for (const expected of [
    "greatest(started_at,least(day_end,p_observed_at))",
    "where ended_at is not null and ended_at<started_at",
  ]) {
    if (!unassignedSegmentBoundaryPatch.replaceAll(" ", "").toLowerCase().includes(expected.replaceAll(" ", "").toLowerCase())) {
      throw new Error(`The unassigned segment boundary patch must include: ${expected}`);
    }
  }
  for (const expected of [
    "timer_chain_id uuid",
    "last_continuous_at timestamptz",
    "last_confirmed_at timestamptz",
    "uq_time_entries_timer_chain_date",
    "uq_unassigned_sessions_active_date",
    "create or replace function timegenie.workspace_work_date",
    "create or replace function timegenie.workspace_day_start",
    "create or replace function timegenie.workspace_day_end",
    "create or replace function timegenie.coordinate_timer_calendar_day",
    "create or replace function timegenie.coordinate_timer_calendar_day_at",
    "create or replace function timegenie.coordinate_unassigned_calendar_day",
    "create or replace function timegenie.coordinate_unassigned_calendar_day_at",
    "create or replace function timegenie.close_expired_unassigned_lease",
    "create or replace function timegenie.apply_calendar_time_entry",
  ]) {
    if (!calendarDayPatch.toLowerCase().includes(expected.toLowerCase())) {
      throw new Error(`The calendar-day patch must include: ${expected}`);
    }
  }
  if (calendarDayPatch.includes("at time zone 'Asia/Shanghai'")) {
    throw new Error("The calendar-day patch must derive dates from the workspace timezone, not a database or hard-coded timezone.");
  }
  for (const signature of [
    "timegenie.coordinate_timer_calendar_day(uuid,uuid)",
    "timegenie.coordinate_unassigned_calendar_day(uuid,uuid,uuid)",
  ]) {
    const compact = `grant execute on function ${signature} to authenticated`.replaceAll(" ", "").toLowerCase();
    if (!calendarDayPatch.replaceAll(" ", "").toLowerCase().includes(compact)) {
      throw new Error(`Authenticated clients must execute ${signature}.`);
    }
  }
  for (const signature of [
    "timegenie.coordinate_timer_calendar_day_at(uuid,uuid,timestamptz)",
    "timegenie.coordinate_unassigned_calendar_day_at(uuid,uuid,uuid,timestamptz)",
  ]) {
    const compact = `revoke execute on function ${signature} from public,anon,authenticated`.replaceAll(" ", "").toLowerCase();
    if (!calendarDayPatch.replaceAll(" ", "").toLowerCase().includes(compact)) {
      throw new Error(`Clients must not execute internal calendar coordinator ${signature}.`);
    }
  }
  for (const expected of [
    "create or replace function timegenie.unassigned_get_or_create_shared",
    "create or replace function timegenie.unassigned_resolve_shared",
    "uq_unassigned_next_session",
    "resolution_operation_id",
    "when 'unassigned_sessions' then",
  ]) {
    if (!schema.toLowerCase().includes(expected.toLowerCase())
        || !sharedUnassignedPatch.toLowerCase().includes(expected.toLowerCase())) {
      throw new Error(`The schema and shared unassigned patch must include: ${expected}`);
    }
  }
  if (!sharedUnassignedPatch.includes("cloud_apply_shared_unassigned_patch")
      || !sharedUnassignedPatch.replaceAll(" ", "").replaceAll("\n", "").includes("jsonb_build_object('segments'")) {
    throw new Error("The shared unassigned patch must deploy aggregate apply and segment reads.");
  }
  for (const expected of [
    "incremental first page metadata is invalid",
    "incremental repeated changes were not folded",
    "incremental delete did not return an explicit tombstone",
    "revoked device read incremental changes",
    "incremental RPC exposed another user workspace",
    "incremental RPC has no applicable representation",
  ]) {
    if (!schemaIntegrationTest.includes(expected)) {
      throw new Error("The schema integration test is missing incremental coverage: " + expected);
    }
  }
  if (!taskPlanningPatch.replaceAll(" ", "").includes("grantexecuteonfunctiontimegenie.cloud_apply_patch(uuid,uuid,uuid,text,text,text,bigint,jsonb)toauthenticated")) {
    throw new Error("The cloud task planning patch must grant the compatibility apply RPC to authenticated clients.");
  }
  if (!taskPlanningPatch.includes("rename to cloud_apply_patch_legacy")
      || !taskPlanningPatch.includes("return timegenie.cloud_apply_patch_legacy")) {
    throw new Error("The cloud task planning patch must preserve all legacy cloud_apply_patch branches.");
  }
  if (!cloudApplyPatch.includes("delete from timegenie.unassigned_segments where session_id = p_entity_id::uuid")) {
    throw new Error("cloud_apply_patch must replace unassigned session segments with the complete payload image.");
  }
  if (!cloudApplyPatch.includes("expected unassigned session version %, current version %")) {
    throw new Error("cloud_apply_patch must reject unassigned session updates with stale base versions.");
  }
  if (!schema.includes("'unassigned_sessions','report_templates'")) {
    throw new Error("unassigned_sessions must keep the workspace change trigger enabled.");
  }
  if (!schema.includes("alter table timegenie.workspace_changes alter column entity_id type text using entity_id::text")) {
    throw new Error("workspace_changes.entity_id must be text so composite and named entity ids are preserved.");
  }
  if (!schema.includes("when 'task_occurrences' then (row_image->>'task_id') || '|' || (row_image->>'occurrence_date')")) {
    throw new Error("touch_workspace_change must preserve task occurrence task/date identity.");
  }
  if (!cloudApplyPatch.includes("values(p_workspace_id, 'app_settings', row_data->>'key'")) {
    throw new Error("cloud_apply_patch workspace changes must identify app settings by key.");
  }
  if (!cloudApplyPatch.includes("values(p_workspace_id, 'integration_configs', row_data->>'provider'")) {
    throw new Error("cloud_apply_patch workspace changes must identify integration configs by provider.");
  }
  if (!cloudApplyPatch.includes("p_entity_id is distinct from (row_data->>'provider') || '|' || (row_data->>'entity_type') || '|' || (row_data->>'entity_id')")) {
    throw new Error("cloud_apply_patch must validate the complete external binding identity.");
  }

  const patchImportStart = migrationPatch.indexOf("create or replace function timegenie.migration_import_snapshot");
  if (patchImportStart < 0) {
    throw new Error("Could not locate migration_import_snapshot in the recurring migration patch.");
  }
  const patchMigrationImport = migrationPatch.slice(patchImportStart);
  if (!patchMigrationImport.includes("nullif(row_data->>'origin_unassigned_session_id','')::uuid")) {
    throw new Error("The recurring migration patch must preserve time_entries.origin_unassigned_session_id from local snapshots.");
  }
  if (!patchMigrationImport.includes("exists(select 1 from timegenie.work_days where workspace_id = p_workspace_id)")
      || !patchMigrationImport.includes("exists(select 1 from timegenie.unassigned_sessions where workspace_id = p_workspace_id)")) {
    throw new Error("The recurring migration patch must treat work_days and unassigned_sessions as business data when checking for an empty target.");
  }
  if (!migrationPatch.includes("alter table timegenie.workspace_changes alter column entity_id type text using entity_id::text")) {
    throw new Error("The recurring migration patch must migrate workspace_changes.entity_id to text.");
  }
  if (!migrationPatch.includes("when 'task_occurrences' then (row_image->>'task_id') || '|' || (row_image->>'occurrence_date')")) {
    throw new Error("The recurring migration patch must preserve task occurrence task/date identity in workspace changes.");
  }

  if (!cloudSyncPatch.includes("create or replace function timegenie.cloud_apply_patch")) {
    throw new Error("The cloud sync patch must redeploy cloud_apply_patch for existing Supabase projects.");
  }
  if (!cloudSyncPatch.includes("p_entity_type = 'unassigned_session'")) {
    throw new Error("The cloud sync patch must accept unassigned_session entity payloads.");
  }
  if (!cloudSyncPatch.includes("delete from timegenie.unassigned_segments where session_id = p_entity_id::uuid")) {
    throw new Error("The cloud sync patch must replace unassigned session segments with the complete payload image.");
  }
  if (!cloudSyncPatch.includes("expected unassigned session version %, current version %")) {
    throw new Error("The cloud sync patch must reject unassigned session updates with stale base versions.");
  }
  if (!cloudSyncPatch.includes("origin_unassigned_session_id = excluded.origin_unassigned_session_id")) {
    throw new Error("The cloud sync patch must preserve time entry unassigned origins on conflict updates.");
  }
  if (!cloudSyncPatch.includes("where timegenie.work_days.version <= excluded.version")) {
    throw new Error("The cloud sync patch must guard work-day updates by version.");
  }
  if (!cloudSyncPatch.includes("grant execute on function timegenie.cloud_apply_patch(uuid, uuid, uuid, text, text, text, bigint, jsonb) to authenticated")) {
    throw new Error("The cloud sync patch must grant authenticated clients access to cloud_apply_patch.");
  }
  for (const expected of [
    "create or replace function timegenie.device_authorize",
    "create or replace function timegenie.current_auth_session_id",
    "create or replace function timegenie.device_authorization_get",
    "create or replace function timegenie.device_list",
    "create or replace function timegenie.device_revoke",
    "create or replace function timegenie.device_revoke_all",
    "revoke all on timegenie.devices from authenticated",
    "PASSWORD_REAUTH_REQUIRED",
    "holder_device_id = null, lease_token = null, expires_at = null",
    "grant execute on function timegenie.device_revoke(uuid, uuid) to authenticated",
  ]) {
    if (!schema.includes(expected) || !sessionRevocationPatch.includes(expected)) {
      throw new Error(`The schema and session revocation patch must include: ${expected}`);
    }
  }

  const targetGuardPatchFile = path.join(repoRoot, "supabase", "20261004b_timegenie_migration_target_guard_patch.sql");
  const targetGuardPatch = fs.readFileSync(targetGuardPatchFile, "utf8");
  if (!targetGuardPatch.includes("create or replace function timegenie.migration_import_snapshot")) {
    throw new Error("The migration target guard patch must redeploy migration_import_snapshot for existing Supabase projects.");
  }
  if (!targetGuardPatch.includes("exists(select 1 from timegenie.work_days where workspace_id = p_workspace_id)")
      || !targetGuardPatch.includes("exists(select 1 from timegenie.unassigned_sessions where workspace_id = p_workspace_id)")) {
    throw new Error("The migration target guard patch must block migration into cloud workspaces that only contain work_days or unassigned_sessions.");
  }
  if (!targetGuardPatch.includes("grant execute on function timegenie.migration_import_snapshot(uuid, uuid, uuid, jsonb) to authenticated")) {
    throw new Error("The migration target guard patch must grant authenticated clients access to migration_import_snapshot.");
  }

  const workDayGuardPatchFile = path.join(repoRoot, "supabase", "20261004c_timegenie_work_day_version_guard_patch.sql");
  const workDayGuardPatch = fs.readFileSync(workDayGuardPatchFile, "utf8");
  if (!workDayGuardPatch.includes("where timegenie.work_days.version <= excluded.version")) {
    throw new Error("The work-day guard patch must reject stale embedded work-day images.");
  }
  if (!workDayGuardPatch.includes("create or replace function timegenie.cloud_apply_patch")) {
    throw new Error("The work-day guard patch must redeploy cloud_apply_patch for existing Supabase projects.");
  }

  const cloudGuardVerifierFile = path.join(repoRoot, "supabase", "20261004_verify_timegenie_cloud_guards.sql");
  const cloudGuardVerifier = fs.readFileSync(cloudGuardVerifierFile, "utf8");
  for (const expected of [
    "exists(select 1 from timegenie.work_days where workspace_id = p_workspace_id)",
    "exists(select 1 from timegenie.unassigned_sessions where workspace_id = p_workspace_id)",
    "where timegenie.work_days.version <= excluded.version",
    "p_entity_type = ''unassigned_session''",
    "delete from timegenie.unassigned_segments where session_id = p_entity_id::uuid",
    "has_function_privilege(\n    'authenticated',\n    'timegenie.cloud_apply_patch(uuid, uuid, uuid, text, text, text, bigint, jsonb)'",
    "has_function_privilege(\n    'authenticated',\n    'timegenie.migration_import_snapshot(uuid, uuid, uuid, jsonb)'",
  ]) {
    if (!cloudGuardVerifier.includes(expected)) {
      throw new Error(`The cloud guard verifier is missing invariant: ${expected}`);
    }
  }
}

async function findOpenPort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      const port = typeof address === "object" && address ? address.port : undefined;
      server.close(() => (port ? resolve(port) : reject(new Error("Could not allocate a local port"))));
    });
  });
}

async function waitForPostgres(psql, port) {
  const deadline = Date.now() + 15_000;
  while (Date.now() < deadline) {
    const result = spawnSync(psql, [
      "-h", "127.0.0.1",
      "-p", String(port),
      "-U", "postgres",
      "-d", "postgres",
      "-w",
      "-c", "select 1;",
    ], { cwd: repoRoot, stdio: "ignore" });
    if (result.status === 0) return;
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error("Timed out waiting for temporary PostgreSQL to start.");
}

async function main() {
  verifyStaticSchemaInvariants();
  const initdb = resolveBinary(binaryNames.initdb);
  const pgCtl = resolveBinary(binaryNames.pgCtl);
  const psql = resolveBinary(binaryNames.psql);
  const port = await findOpenPort();

  ensureInsideRepo(dataDir);
  ensureInsideRepo(logFile);
  fs.rmSync(dataDir, { recursive: true, force: true });
  fs.rmSync(logFile, { force: true });

  let started = false;
  try {
    run(initdb, ["-D", dataDir, "-U", "postgres", "-A", "trust", "--no-locale", "--encoding=UTF8"]);
    run(pgCtl, ["-D", dataDir, "-o", `-p ${port}`, "-l", logFile, "start"]);
    started = true;
    await waitForPostgres(psql, port);
    run(psql, [
      "-h", "127.0.0.1",
      "-p", String(port),
      "-U", "postgres",
      "-d", "postgres",
      "-w",
      "-v", "ON_ERROR_STOP=1",
      "-c", "create database timegenie_fresh_planning_test;",
    ]);
    run(psql, [
      "-h", "127.0.0.1",
      "-p", String(port),
      "-U", "postgres",
      "-d", "timegenie_fresh_planning_test",
      "-w",
      "-v", "ON_ERROR_STOP=1",
      "-f", freshPlanningTestFile,
    ]);
    run(psql, [
      "-h", "127.0.0.1",
      "-p", String(port),
      "-U", "postgres",
      "-d", "postgres",
      "-w",
      "-v", "ON_ERROR_STOP=1",
      "-f", schemaTestFile,
    ]);
    console.log("Supabase schema integration verification passed.");
  } finally {
    if (started) {
      spawnSync(pgCtl, ["-D", dataDir, "stop", "-m", "fast"], { cwd: repoRoot, stdio: "inherit" });
    }
    if (process.env.TG_KEEP_SUPABASE_SCHEMA_PG !== "1") {
      fs.rmSync(dataDir, { recursive: true, force: true });
      fs.rmSync(logFile, { force: true });
    }
  }
}

main().catch((error) => {
  console.error(error.message);
  process.exit(1);
});
