import assert from "node:assert/strict";
import { readFileSync, rmSync } from "node:fs";

const store = readFileSync(new URL("../src-ui/src/store.ts", import.meta.url), "utf8");
const app = readFileSync(new URL("../src-ui/src/App.vue", import.meta.url), "utf8");
const metrics = readFileSync(new URL("../src-tauri/src/direct_cloud_observability.rs", import.meta.url), "utf8");

assert.doesNotMatch(store, /setInterval(() => void loadTimeData().then(() => loadTodayOverview()), 5_000)/, "不得恢复 5 秒计时+概览全量轮询");
assert.match(store, /DirectCloudQueryCoordinator/);
assert.match(app, /invalidateCloudDomains/);
assert.match(app, /store\.loadUnassignedState\(false, true\)/, "云端恢复和唤醒校准必须补拉未归属计时");
assert.match(metrics, /mutation_latency_total_ms/);
assert.match(metrics, /refresh_reused_count/);
assert.doesNotMatch(metrics, /access_token|password|markdown|task_title/i, "性能指标不得包含凭据或业务正文");

rmSync(".tmp-direct-cloud-performance-test", { recursive: true, force: true });
console.log("direct cloud interaction performance boundary verification passed");
