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
const freshPlanningTestFile = path.join(repoRoot, "supabase", "schema_fresh_planning_test.sql");
const schemaTestFile = path.join(repoRoot, "supabase", "schema_integration_test.sql");

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
    env: { ...process.env, ...options.env },
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
  const schemaIntegrationTest = fs.readFileSync(schemaTestFile, "utf8");
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
    run(initdb, ["-D", dataDir, "-U", "postgres", "-A", "trust", "--no-locale"]);
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
