import assert from "node:assert/strict";
import { existsSync, readFileSync, rmSync, statSync } from "node:fs";
import { gzipSync } from "node:zlib";
import { versionedEntityArrayChanged } from "../.tmp-application-responsiveness-test/entityArrayChangeCore.js";

const app = readFileSync(new URL("../src-ui/src/App.vue", import.meta.url), "utf8");
const main = readFileSync(new URL("../src-ui/src/views/MainWorkbench.vue", import.meta.url), "utf8");
const store = readFileSync(new URL("../src-ui/src/store.ts", import.meta.url), "utf8");
const hover = readFileSync(new URL("../src-ui/src/views/HoverPanel.vue", import.meta.url), "utf8");
const supabase = readFileSync(new URL("../src-tauri/src/supabase/mod.rs", import.meta.url), "utf8");
const executor = readFileSync(new URL("../src-tauri/src/cloud_executor.rs", import.meta.url), "utf8");

assert.match(app, /defineAsyncComponent/);
assert.doesNotMatch(app, /import MainWorkbench from|import HoverPanel from/);
for (const component of ["ReportWorkspace", "DataBackupSettings", "CloudOperationHistorySettings", "CloudSnapshotsSettings", "AutomationHooksSettings"]) assert.match(main, new RegExp("const " + component + " = defineAsyncComponent"));
assert.match(store, /await loadWorkbenchBootstrap\(\)/);
assert.match(store, /loadWorkspaceData\(false, false\)/);
assert.match(store, /async function initializeHoverData/);
assert.doesNotMatch(hover, /loadWorkspaceData|loadReports|loadTodayOverview/);
assert.match(supabase, /SUPABASE_CLIENT_CACHE_CAPACITY/);
assert.match(supabase, /client_creation_count/);
assert.match(executor, /MAX_CONCURRENT_CLOUD_JOBS: usize = 6/);
const metricsDefinition = executor.slice(executor.indexOf("pub struct CloudExecutorMetrics"), executor.indexOf("pub async fn run"));
assert.doesNotMatch(metricsDefinition, /url|credential|anon|token|title|markdown|body/i);
assert.equal(versionedEntityArrayChanged([{ id: "a", version: 1 }], [{ id: "a", version: 1 }]), false);
assert.equal(versionedEntityArrayChanged([{ id: "a", version: 1 }], [{ id: "a", version: 2 }]), true);
assert.equal(versionedEntityArrayChanged([{ id: "a", version: 1 }, { id: "b", version: 1 }], [{ id: "b", version: 1 }, { id: "a", version: 1 }]), true);
assert.equal(versionedEntityArrayChanged([{ id: "draft-a", version: 0 }], [{ id: "draft-a", version: 0 }]), true);

const manifestPath = new URL("../dist/.vite/manifest.json", import.meta.url);
if (existsSync(manifestPath)) {
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  const appEntry = Object.values(manifest).find((entry) => entry.isEntry);
  assert.ok(appEntry, "构建产物缺少入口 chunk");
  const hover = Object.values(manifest).find((entry) => entry.src === "src/views/HoverPanel.vue" || entry.name === "HoverPanel");
  const main = Object.values(manifest).find((entry) => entry.src === "src/views/MainWorkbench.vue" || entry.name === "MainWorkbench");
  assert.ok(hover?.isDynamicEntry, "HoverPanel 未形成动态 chunk");
  assert.ok(main?.isDynamicEntry, "MainWorkbench 未形成动态 chunk");
  assert.notEqual(hover.file, main.file);
  const hoverCode = readFileSync(new URL("../dist/" + hover.file, import.meta.url), "utf8");
  assert.doesNotMatch(hoverCode, /MainWorkbench|md-editor-v3|DataBackupSettings|CloudOperationHistorySettings|CloudSnapshotsSettings|AutomationHooksSettings/);
  const initialFiles = [appEntry.file, ...(appEntry.css ?? [])];
  const initialGzipBytes = initialFiles.reduce((sum, file) => sum + gzipSync(readFileSync(new URL("../dist/" + file, import.meta.url))).byteLength, 0);
  console.log(JSON.stringify({ buildBoundary: { hoverChunk: hover.file, mainChunk: main.file, initialFiles, initialGzipBytes, initialRawBytes: initialFiles.reduce((sum, file) => sum + statSync(new URL("../dist/" + file, import.meta.url)).size, 0) } }, null, 2));
}
rmSync(".tmp-application-responsiveness-test", { recursive: true, force: true });
console.log("application responsiveness verification passed");
