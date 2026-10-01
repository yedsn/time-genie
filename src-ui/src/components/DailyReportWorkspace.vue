<script setup lang="ts">
import { computed, nextTick, ref } from "vue";
import { ElMessage, ElMessageBox } from "element-plus";
import { Braces, ClipboardCopy, FilePenLine, RefreshCcw, Sparkles } from "lucide-vue-next";
import { MdEditor, MdPreview } from "md-editor-v3";
import "md-editor-v3/lib/style.css";
import { useWorkdayStore, type Task } from "../store";

const store = useWorkdayStore();
const reportPath = "D:/Obsidian/工作日报/2026-09-15.md";
const defaultTemplate = `【{{姓名}}】{{日期}} {{星期}} {{工作时段}} {{总工时}}

{{今日事项}}

## 明日计划：

{{明日计划}}
`;
const originalMarkdown = `# 2026-09-15 工作日报

## 今日完成

- 整理昨日遗留事项并确认今日安排
- 检查运维环境与同步任务状态

## 工作记录

- 原日报内容尚未根据当前计划重新生成。

## 明日计划

- 继续推进江湖数据同步 v3
- 整理运维自动化调研结果
`;

const markdown = ref(originalMarkdown);
const sourceState = ref<"original" | "generated">("original");
const editorMode = ref<"edit" | "preview">("edit");
const reportTemplate = ref(defaultTemplate);
const templateDraft = ref(defaultTemplate);
const templateDialogOpen = ref(false);
const templateTextarea = ref<HTMLTextAreaElement>();

const templateVariables = [
  { token: "{{姓名}}", label: "姓名", description: "日报署名", example: "晓健" },
  { token: "{{日期}}", label: "日期", description: "日报日期", example: "2026-09-15" },
  { token: "{{星期}}", label: "星期", description: "中文星期", example: "周二" },
  { token: "{{工作时段}}", label: "工作时段", description: "当天工作时间段", example: "10:00-12:00 13:30-17:30" },
  { token: "{{总工时}}", label: "总工时", description: "工作时段合计", example: "6h" },
  { token: "{{今日事项}}", label: "今日事项", description: "仅包含已完成或已开始的事项", example: "1. 🟢 已完成 / 🔴 未完成" },
  { token: "{{明日计划}}", label: "明日计划", description: "完整计划层级", example: "1. 今日计划..." }
] as const;

const toolbars = [
  "revoke",
  "next",
  "-",
  "bold",
  "italic",
  "strikeThrough",
  "title",
  "quote",
  "unorderedList",
  "orderedList",
  "task",
  "codeRow",
  "code",
  "link",
  "table",
  "=",
  "preview"
] as const;

const reportStateLabel = computed(() => sourceState.value === "generated" ? "已生成覆盖" : "原日报版本");
const templatePreview = computed(() => renderTemplate(templateDraft.value));

function taskMinutes(taskId: string) {
  let total = 0;
  for (const entry of store.entries) {
    if (entry.allocations?.length) {
      total += entry.allocations
        .filter((allocation) => allocation.taskId === taskId)
        .reduce((sum, allocation) => sum + allocation.minutes, 0);
      continue;
    }
    if (entry.defaultTask !== taskId) continue;
    total += Math.ceil(store.entryDurationSeconds(entry) / 60);
  }
  return total;
}

function formatHours(minutes: number) {
  return `${(minutes / 60).toFixed(1)}h`;
}

function getChildren(taskId: string) {
  return store.tasks.filter((task) => task.parentId === taskId);
}

function taskStatusIcon(task: Task) {
  return task.status === "done" ? "🟢" : "🔴";
}

function taskMetrics(task: Task) {
  const parts: string[] = [];
  const estimate = task.todayEstimate || task.estimate;
  if (estimate) parts.push(`预计：${formatHours(estimate)}`);
  const actual = taskMinutes(task.id);
  if (actual > 0) parts.push(`实际：${formatHours(actual)}`);
  return parts.length ? ` <${parts.join(" ")}>` : "";
}

function shouldIncludeToday(task: Task): boolean {
  const children = getChildren(task.id);
  if (children.length) return children.some(shouldIncludeToday);
  return task.status === "done" || taskMinutes(task.id) > 0;
}

function todayTaskLine(task: Task, prefix: string) {
  const children = getChildren(task.id).filter(shouldIncludeToday);
  if (children.length) return `${prefix}${task.title}`;
  return `${prefix}${taskStatusIcon(task)} ${task.title}${taskMetrics(task)}`;
}

function appendTodayChildren(lines: string[], parent: Task, depth = 1) {
  const children = getChildren(parent.id).filter(shouldIncludeToday);
  for (const child of children) {
    lines.push(todayTaskLine(child, `${"   ".repeat(depth)}- `));
    if (getChildren(child.id).some(shouldIncludeToday)) appendTodayChildren(lines, child, depth + 1);
  }
}

function buildTodayLines() {
  const roots = store.tasks.filter((task) => !task.parentId).filter(shouldIncludeToday);
  if (!roots.length) return ["暂无今日工作记录"];
  const lines: string[] = [];
  roots.forEach((root, index) => {
    lines.push(todayTaskLine(root, `${index + 1}. `));
    appendTodayChildren(lines, root);
  });
  return lines;
}

function appendTomorrowChildren(lines: string[], parent: Task, depth = 1) {
  for (const child of getChildren(parent.id)) {
    lines.push(`${"   ".repeat(depth)}- ${child.title}`);
    appendTomorrowChildren(lines, child, depth + 1);
  }
}

function buildTomorrowLines() {
  const roots = store.tasks.filter((task) => !task.parentId);
  if (!roots.length) return ["暂无明日计划"];
  const lines: string[] = [];
  roots.forEach((root, index) => {
    lines.push(`${index + 1}. ${root.title}`);
    appendTomorrowChildren(lines, root);
  });
  return lines;
}

function templateContext() {
  return {
    "{{姓名}}": "晓健",
    "{{日期}}": "2026-09-15",
    "{{星期}}": "周二",
    "{{工作时段}}": "10:00-12:00 13:30-17:30",
    "{{总工时}}": "6h",
    "{{今日事项}}": buildTodayLines().join("\n"),
    "{{明日计划}}": buildTomorrowLines().join("\n")
  };
}

function renderTemplate(template: string) {
  let result = template;
  for (const [token, value] of Object.entries(templateContext())) {
    result = result.split(token).join(value);
  }
  return result.trimEnd() + "\n";
}

function generateMarkdown() {
  return renderTemplate(reportTemplate.value);
}

function openTemplateSettings() {
  templateDraft.value = reportTemplate.value;
  templateDialogOpen.value = true;
}

function insertTemplateVariable(token: string) {
  const textarea = templateTextarea.value;
  if (!textarea) {
    templateDraft.value += token;
    return;
  }
  const start = textarea.selectionStart;
  const end = textarea.selectionEnd;
  templateDraft.value = `${templateDraft.value.slice(0, start)}${token}${templateDraft.value.slice(end)}`;
  void nextTick(() => {
    textarea.focus();
    textarea.setSelectionRange(start + token.length, start + token.length);
  });
}

function restoreDefaultTemplate() {
  templateDraft.value = defaultTemplate;
  ElMessage.success("已恢复默认模板");
}

function saveTemplate() {
  if (!templateDraft.value.trim()) {
    ElMessage.warning("日报模板不能为空");
    return;
  }
  reportTemplate.value = templateDraft.value;
  templateDialogOpen.value = false;
  ElMessage.success("日报模板已更新，将在下次生成时使用");
}

async function generateReport() {
  try {
    await ElMessageBox.confirm(
      "生成后会用当前计划与计时数据覆盖编辑区中的日报内容。",
      "生成并覆盖日报",
      {
        type: "warning",
        confirmButtonText: "生成并覆盖",
        cancelButtonText: "取消",
        customClass: "work-confirm-dialog",
        confirmButtonClass: "work-confirm-danger",
        closeOnClickModal: false
      }
    );
  } catch {
    return;
  }
  markdown.value = generateMarkdown();
  sourceState.value = "generated";
  editorMode.value = "edit";
  ElMessage.success("日报已根据当前数据生成");
}

async function restoreOriginal() {
  if (markdown.value === originalMarkdown) return;
  try {
    await ElMessageBox.confirm("将放弃当前编辑内容并恢复进入页面时的原日报。", "恢复原版本", {
      type: "warning",
      confirmButtonText: "恢复原版本",
      cancelButtonText: "取消",
      customClass: "work-confirm-dialog",
      closeOnClickModal: false
    });
  } catch {
    return;
  }
  markdown.value = originalMarkdown;
  sourceState.value = "original";
  ElMessage.success("已恢复原日报版本");
}

async function copyMarkdown() {
  await navigator.clipboard?.writeText(markdown.value);
  ElMessage.success("Markdown 已复制");
}
</script>

<template>
  <section class="daily-report-workspace">
    <header class="daily-report-head">
      <div>
        <div class="daily-report-title-line">
          <h2>日报 Markdown</h2>
          <span class="report-source-state" :class="sourceState">{{ reportStateLabel }}</span>
        </div>
        <p>{{ reportPath }}</p>
      </div>
      <div class="daily-report-actions">
        <button class="secondary-button compact" type="button" title="设置日报模板" @click="openTemplateSettings"><Braces :size="14" />模板设置</button>
        <button class="secondary-button compact" type="button" title="恢复原日报" @click="restoreOriginal"><RefreshCcw :size="14" />恢复原版本</button>
        <button class="secondary-button compact" type="button" title="复制 Markdown" @click="copyMarkdown"><ClipboardCopy :size="14" />复制</button>
        <button class="primary-button" type="button" @click="generateReport"><Sparkles :size="15" />生成日报</button>
      </div>
    </header>

    <div class="daily-editor-modebar">
      <div class="report-mode-tabs" role="tablist" aria-label="日报编辑模式">
        <button :class="{ active: editorMode === 'edit' }" type="button" @click="editorMode = 'edit'"><FilePenLine :size="14" />编辑</button>
        <button :class="{ active: editorMode === 'preview' }" type="button" @click="editorMode = 'preview'">预览</button>
      </div>
      <span>{{ markdown.length }} 字符 · Markdown</span>
    </div>

    <MdEditor
      v-model="markdown"
      class="daily-markdown-editor"
      editor-id="daily-report-editor"
      language="zh-CN"
      theme="dark"
      preview-theme="github"
      code-theme="github"
      :toolbars="[...toolbars]"
      :preview="editorMode === 'edit'"
      :preview-only="editorMode === 'preview'"
      :footers="['markdownTotal', '=', 'scrollSwitch']"
      :show-code-row-number="true"
    />

    <el-dialog
      v-model="templateDialogOpen"
      class="report-template-dialog"
      title="日报模板设置"
      width="min(920px, 92vw)"
      append-to-body
      destroy-on-close
    >
      <div class="report-template-layout">
        <section class="template-editor-pane">
          <div class="template-pane-head">
            <div><strong>模板内容</strong><span>使用 Markdown 和占位符设计生成格式</span></div>
            <button class="secondary-button compact" type="button" @click="restoreDefaultTemplate"><RefreshCcw :size="13" />恢复默认</button>
          </div>
          <textarea ref="templateTextarea" v-model="templateDraft" spellcheck="false" aria-label="日报模板内容"></textarea>
          <div class="template-variable-list">
            <button
              v-for="variable in templateVariables"
              :key="variable.token"
              type="button"
              :title="`${variable.description}，示例：${variable.example}`"
              @click="insertTemplateVariable(variable.token)"
            >
              <code>{{ variable.token }}</code>
              <span>{{ variable.description }}</span>
            </button>
          </div>
        </section>

        <section class="template-preview-pane">
          <div class="template-pane-head"><div><strong>实时预览</strong><span>使用当前模拟数据替换占位符</span></div></div>
          <MdPreview
            editor-id="daily-template-preview"
            :model-value="templatePreview"
            language="zh-CN"
            theme="dark"
            preview-theme="github"
            code-theme="github"
          />
        </section>
      </div>
      <template #footer>
        <button class="secondary-button" type="button" @click="templateDialogOpen = false">取消</button>
        <button class="primary-button" type="button" @click="saveTemplate">保存模板</button>
      </template>
    </el-dialog>
  </section>
</template>
