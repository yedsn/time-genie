import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const read = (relativePath) => fs.readFileSync(path.join(root, relativePath), "utf8");

const observability = read("src-tauri/src/direct_cloud_observability.rs");
const supabase = read("src-tauri/src/supabase.rs");
const cloudSync = read("src-tauri/src/cloud_sync.rs");
const lib = read("src-tauri/src/lib.rs");
const frontendApi = read("src-ui/src/services/tauri.ts");
const schema = read("supabase/schema.sql");
const integration = read("supabase/schema_integration_test.sql");
const guard = read("supabase/20261023k_timegenie_direct_cloud_release_guard.sql");
const protocolDocs = read("docs/direct-cloud-storage-protocol.md");

for (const field of [
  "query_count",
  "query_error_count",
  "query_latency_total_ms",
  "query_latency_max_ms",
  "mutation_count",
  "mutation_error_count",
  "protocol_rejection_count",
  "realtime_reconnect_count",
  "copy_failure_count",
  "last_error_code",
]) {
  if (!observability.includes(field)) {
    throw new Error("Release metrics are missing the field: " + field);
  }
}

for (const hook of [
  "record_query(",
  "record_query_error(",
  "record_mutation(",
  "record_mutation_error(",
  "record_copy_failure(",
  "direct_cloud_release_metrics_get",
]) {
  if (!supabase.includes(hook)) {
    throw new Error("The direct-cloud release path does not record: " + hook);
  }
}
if (!cloudSync.includes("record_realtime_reconnect")) {
  throw new Error("Realtime reconnect attempts are not observable.");
}
if (!lib.includes("supabase::direct_cloud_release_metrics_get")) {
  throw new Error("The release metrics Tauri command is not registered.");
}
if (!frontendApi.includes("getDirectCloudReleaseMetrics")) {
  throw new Error("The frontend release metrics API is missing.");
}

const mutationStart = supabase.indexOf("fn direct_cloud_mutation_with_context");
const mutationEnd = supabase.indexOf("fn record_direct_cloud_checkpoint", mutationStart);
if (mutationStart < 0 || mutationEnd <= mutationStart) {
  throw new Error("The direct-cloud mutation boundary could not be inspected.");
}
const mutationPath = supabase.slice(mutationStart, mutationEnd);
for (const forbidden of ["sync_outbox", "cloud_incremental_pull", "apply_incremental_entities"]) {
  if (mutationPath.includes(forbidden)) {
    throw new Error("The direct-cloud mutation path still references legacy sync: " + forbidden);
  }
}

for (const expected of [
  "emergency_read_only boolean not null default false",
  "emergency_read_only_write_barrier",
  "processed_operations",
  "tracking_leases",
  "raise exception 'EMERGENCY_READ_ONLY",
  "'writable',not workspace_row.emergency_read_only",
  "'readOnlyReason'",
]) {
  if (!guard.toLowerCase().includes(expected.toLowerCase())) {
    throw new Error("The emergency read-only guard is missing: " + expected);
  }
}
if (/grant\s+execute\s+on\s+function[^;]*\bto\s+(anon|service_role)/i.test(guard)) {
  throw new Error("Emergency guard internals must not be executable by anon or service_role.");
}
if (!schema.includes("\\ir 20261023k_timegenie_direct_cloud_release_guard.sql")) {
  throw new Error("The source schema does not include the release guard patch.");
}
if (!integration.includes("EMERGENCY_READ_ONLY")) {
  throw new Error("Schema integration tests do not exercise the emergency write barrier.");
}

for (const expected of [
  "direct_cloud_release_metrics_get",
  "getDirectCloudReleaseMetrics",
  "emergency_read_only = true",
  "emergency_read_only = false",
  "supabase/20261023k_timegenie_direct_cloud_release_guard.sql",
  "不会转入 SQLite outbox",
  "writable: true",
]) {
  if (!protocolDocs.includes(expected)) {
    throw new Error("Release operations documentation is missing: " + expected);
  }
}

console.log("Direct cloud release guard and observability verified.");
