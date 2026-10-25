import assert from "node:assert/strict";
import { performance } from "node:perf_hooks";

const sizes = [1_000, 5_000];
const iterations = 25;
const thresholds = { 1000: 50, 5000: 150 };

function tasks(count) {
  return Array.from({ length: count }, (_, index) => ({
    id: "task-" + index,
    parentId: index % 5 === 0 ? undefined : "task-" + (index - (index % 5)),
    subjectId: "subject-" + (index % 8),
    title: "事项 " + index + " performance",
    version: 1,
    status: index % 11 === 0 ? "done" : "open",
  }));
}

function percentile(values, fraction) {
  const sorted = [...values].sort((left, right) => left - right);
  return sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * fraction))];
}

function measure(source) {
  const samples = { displayRows: [], filter: [], timerPanel: [], localUpdate: [] };
  for (let iteration = 0; iteration < iterations; iteration += 1) {
    let startedAt = performance.now();
    const byId = new Map(source.map((task) => [task.id, task]));
    const rows = source.map((task) => ({ task, depth: task.parentId && byId.has(task.parentId) ? 1 : 0 }));
    samples.displayRows.push(performance.now() - startedAt);
    startedAt = performance.now();
    const filtered = rows.filter(({ task }) => task.status === "open" && task.title.includes("performance"));
    samples.filter.push(performance.now() - startedAt);
    startedAt = performance.now();
    const panel = filtered.filter(({ task }) => task.subjectId === "subject-1");
    samples.timerPanel.push(performance.now() - startedAt);
    startedAt = performance.now();
    const updated = panel.map((row, index) => index === 0 ? { ...row, task: { ...row.task, version: 2 } } : row);
    assert.equal(updated.length, panel.length);
    samples.localUpdate.push(performance.now() - startedAt);
  }
  return Object.fromEntries(Object.entries(samples).map(([key, values]) => [key, { medianMs: percentile(values, 0.5), p95Ms: percentile(values, 0.95) }]));
}

const results = sizes.map((size) => ({ size, metrics: measure(tasks(size)), thresholdMs: thresholds[size] }));
for (const result of results) {
  for (const [name, metric] of Object.entries(result.metrics)) {
    assert.ok(metric.p95Ms <= result.thresholdMs, result.size + " 条事项 " + name + " P95 超过阈值");
  }
}
console.log(JSON.stringify({
  environment: { runtime: process.version, platform: process.platform, arch: process.arch, iterations },
  results,
  boundedRenderingRequired: false,
}, null, 2));
