import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const read = (relativePath) => readFileSync(path.join(root, relativePath), "utf8");
const main = read("src-ui/src/views/MainWorkbench.vue");
const editor = read("src-ui/src/components/PlanListEditor.vue");
const today = read("src-ui/src/components/TodayOverviewWorkspace.vue");

const requireText = (source, text, label) => {
  if (!source.includes(text)) throw new Error(`缺少${label}: ${text}`);
};

requireText(editor, "emit('importPlan', mode === 'today' ? 'day_instance' : 'backlog')", "待办两类入口目标映射");
requireText(today, "emit('importDailyReport', store.todayOverviewSubjectId || undefined)", "今日概览导入入口");
requireText(main, 'title="从日报导入"', "日报导入弹框");
requireText(main, '@change="refreshDailyReportPreview"', "切换日期重新预览");
requireText(main, ':disabled="item.alreadyImported"', "已导入候选禁用态");
requireText(main, "实际 {{ formatMinutes(item.importActualMinutes) }}", "已完成事项实际用时");
requireText(main, "选择子事项时，会自动补齐保持层级所需的父项。", "父级补齐提示");
requireText(main, "importItems.value = [];", "日期切换清空旧选择");
requireText(main, 'targetKind === "day_instance" ? listTodayTasks(subjectId, sourceDate)', "按所选日期读取今日列表");
requireText(main, 'existingTodayTitles.has(item.title)', "今日同名事项默认不勾选");
requireText(main, 'subject_daily_report_directory:', "主体日报目录设置");
if (main.includes("导入昨日计划") || editor.includes("导入昨日计划")) throw new Error("前端仍保留旧‘导入昨日计划’入口");
if (main.includes("切换明日计划") || editor.includes("切换明日计划")) throw new Error("前端不应提供明日计划切换");
if (main.includes("实际 {{ formatMinutes(item.rawActualMinutes) }} 将忽略") || main.includes("未完成事项的实际用时将忽略")) throw new Error("前端不应显示未完成事项的实际用时忽略提示");

console.log("daily report import UI contract checks passed");
