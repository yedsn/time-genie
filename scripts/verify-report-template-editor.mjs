import assert from "node:assert/strict";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";
import { readFileSync, rmSync } from "node:fs";

const modulePath = resolve(".tmp-report-template-editor-test/reportTemplateEditorCore.js");
const core = await import(pathToFileURL(modulePath));

const templates = [
  { id: "standard", name: "标准日报", isDefault: true },
  { id: "customer", name: "客户日报", isDefault: false },
];
assert.equal(core.validateReportTemplateDraft({ type: "daily", name: "精简日报", content: "# {{日期}}\n{{今日事项}}", templates }), undefined);
assert.equal(core.validateReportTemplateDraft({ type: "daily", name: "  ", content: "正文", templates }), "请输入模板名称");
assert.equal(core.validateReportTemplateDraft({ type: "daily", name: "客户日报", content: "正文", templates }), "同类型模板名称已存在");
assert.equal(core.validateReportTemplateDraft({ type: "daily", name: "新模板", content: "{{不存在字段}}", templates }), "未知占位符：{{不存在字段}}");
assert.equal(core.validateReportTemplateDraft({ type: "weekly", name: "新模板", content: "{{今日事项}}", templates: [] }), "占位符 {{今日事项}} 不适用于当前报告类型");
assert.equal(core.validateReportTemplateDraft({ type: "daily", name: "新模板", content: "{{日期", templates: [] }), "报告模板包含未闭合占位符");

assert.deepEqual(core.insertTemplateToken("前后", 1, 1, "{{日期}}"), { content: "前{{日期}}后", selectionStart: 7, selectionEnd: 7 });
assert.equal(core.renderTemplatePreview("# {{日期}} {{姓名}}", { "{{日期}}": "2026-10-11", "{{姓名}}": "晓健" }), "# 2026-10-11 晓健");
assert.deepEqual(core.placeholdersForReportType("weekly").map((item) => item.token), ["{{姓名}}", "{{日期范围}}", "{{总工时}}", "{{统计信息}}", "{{每日情况}}", "{{日报合集}}", "{{总结}}"]);
assert.equal(core.validateReportTemplateDraft({ type: "weekly", name: "日报合集周报", content: "{{统计信息}}\n\n{{日报合集}}", templates: [] }), undefined);

assert.equal(core.selectReportTemplate(templates, { selectDefault: true })?.id, "standard");
assert.equal(core.selectReportTemplate(templates, { preferredId: "customer", selectDefault: true })?.id, "customer");
assert.equal(core.selectReportTemplate(templates, { currentId: "customer", selectDefault: false })?.id, "customer");
assert.equal(core.planTemplateDeletion(templates[0], [templates[0]]).error, "每种报告类型至少需要保留一份模板");
assert.equal(core.planTemplateDeletion(templates[0], templates).replacement?.id, "customer");
assert.equal(core.planTemplateDeletion(templates[1], templates).replacement, undefined);
assert.deepEqual(core.reorderTemplateIds(templates, "customer", -1), ["customer", "standard"]);
assert.equal(core.reorderTemplateIds(templates, "standard", -1), undefined);

const reportWorkspace = readFileSync(resolve("src-ui/src/components/ReportWorkspace.vue"), "utf8");
const globalStyles = readFileSync(resolve("src-ui/src/styles.css"), "utf8");
assert.match(reportWorkspace, /<section class="report-create-section report-template-section">[\s\S]*?class="report-template-trigger"[\s\S]*?openTemplatePicker/);
assert.match(reportWorkspace, /class="report-template-picker-dialog"[\s\S]*?class="report-template-picker-list"[\s\S]*?class="report-template-picker-preview"/);
assert.match(reportWorkspace, /{{日报合集}}/);
assert.match(reportWorkspace, /{{统计信息}}\\n\\n{{日报合集}}/);
assert.doesNotMatch(reportWorkspace.match(/<section class="report-create-section report-template-section">[\s\S]*?<\/section>/)?.[0] ?? "", /MdPreview|el-select/);
assert.match(globalStyles, /\.report-create-form\s*\{[^}]*grid-template-rows:\s*auto auto minmax\(170px, 1fr\)/);
assert.match(globalStyles, /\.report-create-dialog \.el-dialog__body\s*\{[^}]*overflow:\s*hidden/);

const temporaryOutput = resolve(".tmp-report-template-editor-test");
if (temporaryOutput.startsWith(resolve("."))) rmSync(temporaryOutput, { recursive: true, force: true });
console.log("report template editor verification passed");
