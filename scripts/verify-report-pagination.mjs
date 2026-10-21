import assert from "node:assert/strict";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";
import { readFileSync, rmSync } from "node:fs";

const core = await import(pathToFileURL(resolve(".tmp-report-pagination-test/reportPaginationCore.js")));
const report = (id) => ({ id });
const page = (reports, currentPage, total, hasMore) => ({
  reports,
  page: currentPage,
  pageSize: 20,
  total,
  hasMore,
  typeCounts: { all: total, daily: total, weekly: 0, monthly: 0 },
});

const state = core.createReportPaginationState();
const firstToken = core.beginReportReset(state, { subjectId: "subject-a", reportType: "daily", query: " 10-20 " });
assert.deepEqual(state.filters, { subjectId: "subject-a", reportType: "daily", query: "10-20" });
assert.equal(core.applyReportPage(state, firstToken, page([report("a"), report("b")], 1, 45, true), true), true);
const secondToken = core.beginReportLoadMore(state);
assert.ok(secondToken);
assert.equal(core.beginReportLoadMore(state), undefined, "同一页加载期间必须互斥");
assert.equal(core.applyReportPage(state, secondToken, page([report("b"), report("c")], 2, 45, true), false), true);
assert.deepEqual(state.items.map((item) => item.id), ["a", "b", "c"], "跨页重复 ID 必须去重");
const thirdToken = core.beginReportLoadMore(state);
assert.ok(thirdToken);
core.applyReportPage(state, thirdToken, page([report("d")], 3, 45, false), false);
assert.deepEqual(state.items.map((item) => item.id), ["a", "b", "c", "d"]);
assert.equal(state.loadedPageDepth, 3);
assert.equal(core.beginReportRefresh(state).page, 3, "刷新必须恢复到已经加载的页深");
state.loading = false;

state.hasMore = true;
const failedToken = core.beginReportLoadMore(state);
assert.ok(failedToken);
core.failReportPage(state, failedToken, "网络异常");
assert.deepEqual(state.items.map((item) => item.id), ["a", "b", "c", "d"], "续载失败应保留已有结果");
assert.equal(state.error, "网络异常");

const staleToken = core.beginReportReset(state, { subjectId: "subject-old", reportType: "all", query: "old" });
const currentToken = core.beginReportReset(state, { subjectId: "subject-new", reportType: "weekly", query: "new" });
assert.equal(core.applyReportPage(state, staleToken, page([report("old")], 1, 1, false), true), false);
assert.equal(core.applyReportPage(state, currentToken, page([report("new")], 1, 1, false), true), true);
assert.deepEqual(state.items.map((item) => item.id), ["new"], "旧筛选响应不得覆盖新筛选");

const refreshToken = core.beginReportRefresh(state);
core.applyReportPage(state, refreshToken, page([report("new"), report("newer")], 1, 2, false), true);
assert.deepEqual(state.items.map((item) => item.id), ["new", "newer"]);
assert.equal(core.resolveReportSelection(state.items, "newer"), "newer", "可见选择应保持");
assert.equal(core.resolveReportSelection(state.items, "deleted"), "new", "删除或筛选不可见时应选择第一份");
assert.equal(core.resolveReportSelection([], "new"), "", "空列表应清空选择");

const yearState = core.createReportPaginationState();
const yearItems = Array.from({ length: 365 }, (_, index) => report(`day-${String(365 - index).padStart(3, "0")}`));
let yearToken = core.beginReportReset(yearState, { subjectId: "year-subject", reportType: "daily" });
core.applyReportPage(yearState, yearToken, page(yearItems.slice(0, 20), 1, 365, true), true);
assert.equal(yearState.items.length, 20, "一年日报首次只加载 20 条");
for (let pageNumber = 2; pageNumber <= Math.ceil(yearItems.length / 20); pageNumber += 1) {
  yearToken = core.beginReportLoadMore(yearState);
  assert.ok(yearToken);
  const start = (pageNumber - 1) * 20;
  core.applyReportPage(
    yearState,
    yearToken,
    page(yearItems.slice(start, start + 20), pageNumber, 365, start + 20 < yearItems.length),
    false,
  );
}
assert.equal(yearState.items.length, 365);
assert.equal(new Set(yearState.items.map((item) => item.id)).size, 365, "连续滚动一年数据不得重复遗漏");
assert.equal(yearState.hasMore, false);

const tauriSource = readFileSync(resolve("src-ui/src/services/tauri.ts"), "utf8");
assert.match(tauriSource, /export type ReportListResultDto[\s\S]*?pageSize: number;[\s\S]*?total: number;[\s\S]*?hasMore: boolean;[\s\S]*?typeCounts:/);
assert.match(tauriSource, /page: request\.page \?\? 1,[\s\S]*?pageSize: request\.pageSize \?\? 20/);

const workspaceSource = readFileSync(resolve("src-ui/src/components/ReportWorkspace.vue"), "utf8");
assert.match(workspaceSource, /250/);
assert.match(workspaceSource, /scrollHeight - target\.scrollTop - target\.clientHeight <= 120/);
assert.doesNotMatch(workspaceSource, /class="report-pagination"/);

const temporaryOutput = resolve(".tmp-report-pagination-test");
if (temporaryOutput.startsWith(resolve("."))) rmSync(temporaryOutput, { recursive: true, force: true });
console.log("report pagination verification passed");
