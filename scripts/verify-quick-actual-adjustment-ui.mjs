import assert from "node:assert/strict";
import fs from "node:fs";

const component = fs.readFileSync(new URL("../src-ui/src/components/PlanListEditor.vue", import.meta.url), "utf8");
const executionPanel = fs.readFileSync(new URL("../src-ui/src/components/TimerTaskDialog.vue", import.meta.url), "utf8");
const styles = fs.readFileSync(new URL("../src-ui/src/styles.css", import.meta.url), "utf8");

assert(!component.includes(">调整今日实际</button>"), "待办页不应保留独立调整今日实际入口");
assert(executionPanel.includes("今日实际") && executionPanel.includes("openActualAdjustment"), "执行事项面板缺少今日实际调整入口");
assert(component.includes("actualAdjustmentBatchId.value = crypto.randomUUID()"));
assert(component.includes("batchId: actualAdjustmentBatchId.value"), "重试必须复用同一批次 ID");
assert(component.includes("actualAdjustmentSubjectId.value !== props.subjectId"), "主体切换后不能把旧草稿保存到新主体");
assert(component.includes("新增事项") && component.includes("仅新增事项，无实际调整"));
assert(component.includes("dangerouslyUseHTMLString: true"), "保存前摘要应逐项展示变化");
assert(component.includes('errors[draft.clientTaskId] === "事项名称不能为空"'), "补全草稿名称后应清除旧的空名称错误");
assert(component.includes("updateActualNewTaskTitle(draft"), "草稿名称输入应即时更新并恢复可提交状态");
assert(styles.includes(".actual-adjustment-dialog { font-family: inherit; color: var(--text); }"));
assert(styles.includes(".actual-adjustment-confirm-list"));

console.log("Quick actual adjustment UI verification passed.");
