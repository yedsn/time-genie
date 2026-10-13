import assert from "node:assert/strict";
import {
  currentMonthRange,
  defaultExportDraft,
  ensureXlsxExtension,
  exportRangeError,
  LatestRequestGuard,
} from "../.tmp-time-allocation-export-test/timeAllocationExportCore.js";

assert.deepEqual(currentMonthRange("2026-02-15"), {
  startDate: "2026-02-01",
  endDate: "2026-02-28",
});
assert.deepEqual(currentMonthRange("2028-02-15"), {
  startDate: "2028-02-01",
  endDate: "2028-02-29",
});
assert.deepEqual(currentMonthRange("2026-12-31"), {
  startDate: "2026-12-01",
  endDate: "2026-12-31",
});
assert.deepEqual(currentMonthRange("invalid"), { startDate: "", endDate: "" });
assert.deepEqual(defaultExportDraft("2026-10-13"), {
  startDate: "2026-10-01",
  endDate: "2026-10-31",
  includeSpecialTime: false,
});

assert.equal(exportRangeError("2026-10-01", "2026-10-31"), "");
assert.equal(exportRangeError("2026-11-01", "2026-10-31"), "开始日期不能晚于结束日期");
assert.equal(exportRangeError("", "2026-10-31"), "请选择完整的开始和结束日期");
assert.equal(ensureXlsxExtension("D:/export/time"), "D:/export/time.xlsx");
assert.equal(ensureXlsxExtension("D:/export/time.XLSX"), "D:/export/time.XLSX");

const guard = new LatestRequestGuard();
const first = guard.next();
const second = guard.next();
assert.equal(guard.isCurrent(first), false);
assert.equal(guard.isCurrent(second), true);
guard.invalidate();
assert.equal(guard.isCurrent(second), false);

console.log("time allocation export core checks passed");
