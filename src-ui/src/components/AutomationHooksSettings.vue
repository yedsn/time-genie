<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { ArrowDown, ArrowUp, Beaker, Code2, History, Link2, Pencil, Plus, Trash2, X } from "lucide-vue-next";
import { ElMessage, ElMessageBox } from "element-plus";
import {
  createAutomationHook,
  deleteAutomationHook,
  listAutomationHookRuns,
  listAutomationHooks,
  reorderAutomationHooks,
  setAutomationHookEnabled,
  testAutomationHook,
  updateAutomationHook,
  type AutomationHookAction,
  type AutomationHookDraft,
  type AutomationHookEvent,
  type AutomationHookRule,
  type AutomationHookRun,
} from "../services/tauri";

const hooks = ref<AutomationHookRule[]>([]);
const loading = ref(false);
const saving = ref(false);
const testing = ref(false);
const editorOpen = ref(false);
const runsOpen = ref(false);
const editingId = ref("");
const runs = ref<AutomationHookRun[]>([]);
const selectedRun = ref<AutomationHookRun>();

const events: Array<{ value: AutomationHookEvent; label: string }> = [
  { value: "timer.started", label: "计时开始" },
  { value: "timer.stopped", label: "计时结束" },
  { value: "task.completed", label: "事项完成" },
];
const templateVariables = [
  "{{event.type}}", "{{event.id}}", "{{task.id}}", "{{task.title}}", "{{task.path}}",
  "{{task.occurrenceDate}}", "{{subject.id}}", "{{subject.name}}", "{{timer.id}}",
  "{{timer.durationSeconds}}", "{{timer.durationMinutes}}", "{{timer.pendingMinutes}}",
];
const draft = reactive({
  name: "",
  eventType: "task.completed" as AutomationHookEvent,
  actionType: "uri" as AutomationHookAction,
  uriTemplate: "motioncue://play/task-complete",
  program: "",
  args: [""] as string[],
  workingDirectory: "",
  timeoutSeconds: 10,
  enabled: true,
});
const dialogTitle = computed(() => editingId.value ? "编辑自动化 Hook" : "添加自动化 Hook");

function resetDraft() {
  editingId.value = "";
  Object.assign(draft, {
    name: "", eventType: "task.completed", actionType: "uri",
    uriTemplate: "motioncue://play/task-complete", program: "", args: [""],
    workingDirectory: "", timeoutSeconds: 10, enabled: true,
  });
}

function currentRequest(): AutomationHookDraft {
  return {
    name: draft.name.trim(),
    eventType: draft.eventType,
    actionType: draft.actionType,
    actionConfig: draft.actionType === "uri"
      ? { uriTemplate: draft.uriTemplate.trim() }
      : { program: draft.program.trim(), args: draft.args, workingDirectory: draft.workingDirectory.trim() || undefined },
    timeoutSeconds: Number(draft.timeoutSeconds),
    enabled: draft.enabled,
  };
}

async function loadHooks() {
  loading.value = true;
  try { hooks.value = await listAutomationHooks(); }
  catch (error) { ElMessage.error(error instanceof Error ? error.message : String(error)); }
  finally { loading.value = false; }
}

function openCreate() { resetDraft(); editorOpen.value = true; }

function openEdit(hook: AutomationHookRule) {
  editingId.value = hook.id;
  Object.assign(draft, {
    name: hook.name,
    eventType: hook.eventType,
    actionType: hook.actionType,
    uriTemplate: String(hook.actionConfig.uriTemplate ?? ""),
    program: String(hook.actionConfig.program ?? ""),
    args: [...(hook.actionConfig.args ?? [""])],
    workingDirectory: String(hook.actionConfig.workingDirectory ?? ""),
    timeoutSeconds: hook.timeoutSeconds,
    enabled: hook.enabled,
  });
  if (!draft.args.length) draft.args.push("");
  editorOpen.value = true;
}

async function saveDraft() {
  if (saving.value) return;
  saving.value = true;
  try {
    const isEditing = Boolean(editingId.value);
    if (isEditing) await updateAutomationHook(editingId.value, currentRequest());
    else await createAutomationHook(currentRequest());
    editorOpen.value = false;
    await loadHooks();
    ElMessage.success(isEditing ? "Hook 已更新" : "Hook 已添加");
  } catch (error) { ElMessage.error(error instanceof Error ? error.message : String(error)); }
  finally { saving.value = false; }
}

async function runTest(request: AutomationHookDraft) {
  if (testing.value) return;
  try {
    await ElMessageBox.confirm(
      request.actionType === "uri" ? "测试会立即打开当前 URI。" : "测试会立即运行当前本地程序。",
      "执行 Hook 测试",
      { confirmButtonText: "立即测试", cancelButtonText: "取消", type: "warning" },
    );
    testing.value = true;
    const result = await testAutomationHook(request);
    runs.value = [result];
    selectedRun.value = result;
    runsOpen.value = true;
    await loadHooks();
  } catch (error) {
    if (error !== "cancel" && error !== "close") ElMessage.error(error instanceof Error ? error.message : String(error));
  } finally { testing.value = false; }
}

async function toggleEnabled(hook: AutomationHookRule, value: string | number | boolean) {
  try { await setAutomationHookEnabled(hook.id, Boolean(value)); await loadHooks(); }
  catch (error) { ElMessage.error(error instanceof Error ? error.message : String(error)); }
}
function handleEnabledChange(hook: AutomationHookRule, value: unknown) {
  void toggleEnabled(hook, Boolean(value));
}

async function moveHook(index: number, direction: -1 | 1) {
  const target = index + direction;
  if (target < 0 || target >= hooks.value.length) return;
  const reordered = [...hooks.value];
  const current = reordered[index];
  reordered[index] = reordered[target];
  reordered[target] = current;
  try { hooks.value = await reorderAutomationHooks(reordered.map((hook) => hook.id)); }
  catch (error) { await loadHooks(); ElMessage.error(error instanceof Error ? error.message : String(error)); }
}

async function removeHook(hook: AutomationHookRule) {
  try {
    await ElMessageBox.confirm("删除“" + hook.name + "”后不会再触发，历史记录仍会保留。", "删除 Hook", {
      confirmButtonText: "删除", cancelButtonText: "取消", type: "warning",
    });
    await deleteAutomationHook(hook.id);
    await loadHooks();
    ElMessage.success("Hook 已删除");
  } catch (error) {
    if (error !== "cancel" && error !== "close") ElMessage.error(error instanceof Error ? error.message : String(error));
  }
}

async function openRuns(hook?: AutomationHookRule) {
  try {
    runs.value = await listAutomationHookRuns(hook?.id, 50);
    selectedRun.value = runs.value[0];
    runsOpen.value = true;
  } catch (error) { ElMessage.error(error instanceof Error ? error.message : String(error)); }
}

function eventLabel(value: AutomationHookEvent) { return events.find((item) => item.value === value)?.label ?? value; }
function statusLabel(value?: AutomationHookRun["status"]) {
  return ({ queued: "等待执行", running: "执行中", succeeded: "成功", failed: "失败", timed_out: "超时" } as Record<string, string>)[value ?? ""] ?? "尚未执行";
}
function actionSummary(hook: AutomationHookRule) {
  if (hook.actionType === "uri") return String(hook.actionConfig.uriTemplate ?? "URI");
  const args = (hook.actionConfig.args ?? []).filter(Boolean).join(" ");
  return String(hook.actionConfig.program ?? "本地程序") + (args ? " " + args : "");
}
function formatTime(value?: number) { return value ? new Date(value).toLocaleString("zh-CN", { hour12: false }) : "-"; }
function removeArgument(index: number) { draft.args.length === 1 ? draft.args.splice(0, 1, "") : draft.args.splice(index, 1); }

onMounted(loadHooks);
</script>

<template>
  <div class="hooks-panel">
    <div class="hooks-header">
      <div><h2>自动化 Hook</h2><span>规则和执行记录只保存在当前设备</span></div>
      <div class="hooks-header-actions">
        <el-button text :icon="History" @click="openRuns()">最近执行</el-button>
        <el-button type="primary" :icon="Plus" @click="openCreate">添加 Hook</el-button>
      </div>
    </div>

    <div v-loading="loading" class="hooks-list">
      <div v-for="(hook, index) in hooks" :key="hook.id" class="hook-row" :class="{ disabled: !hook.enabled }">
        <el-switch :model-value="hook.enabled" aria-label="启用 Hook" @change="handleEnabledChange(hook, $event)" />
        <div class="hook-main">
          <div class="hook-title-line"><strong>{{ hook.name }}</strong><span>{{ eventLabel(hook.eventType) }}</span><span><Link2 v-if="hook.actionType === 'uri'" :size="13" /><Code2 v-else :size="13" />{{ hook.actionType === 'uri' ? 'URI' : '本地程序' }}</span></div>
          <small>{{ actionSummary(hook) }}</small>
        </div>
        <button class="hook-status" type="button" :class="hook.recentRun?.status" @click="openRuns(hook)"><i></i><span>{{ statusLabel(hook.recentRun?.status) }}</span><small v-if="hook.recentRun?.durationMs !== undefined">{{ hook.recentRun.durationMs }}ms</small></button>
        <div class="hook-actions">
          <el-tooltip content="上移"><el-button text circle :icon="ArrowUp" :disabled="index === 0" @click="moveHook(index, -1)" /></el-tooltip>
          <el-tooltip content="下移"><el-button text circle :icon="ArrowDown" :disabled="index === hooks.length - 1" @click="moveHook(index, 1)" /></el-tooltip>
          <el-tooltip content="测试"><el-button text circle :icon="Beaker" @click="runTest({ name: hook.name, eventType: hook.eventType, actionType: hook.actionType, actionConfig: hook.actionConfig, timeoutSeconds: hook.timeoutSeconds, enabled: hook.enabled })" /></el-tooltip>
          <el-tooltip content="编辑"><el-button text circle :icon="Pencil" @click="openEdit(hook)" /></el-tooltip>
          <el-tooltip content="删除"><el-button text circle type="danger" :icon="Trash2" @click="removeHook(hook)" /></el-tooltip>
        </div>
      </div>
      <button v-if="!hooks.length && !loading" class="hook-empty" type="button" @click="openCreate"><Plus :size="18" /><span>添加第一条 Hook</span></button>
    </div>
  </div>

  <el-dialog v-model="editorOpen" class="hook-editor-dialog" :title="dialogTitle" width="min(760px, 94vw)" append-to-body destroy-on-close :close-on-click-modal="!saving && !testing">
    <el-form label-position="top" class="hook-form" @submit.prevent="saveDraft">
      <div class="hook-form-grid">
        <el-form-item label="名称"><el-input v-model="draft.name" maxlength="80" placeholder="例如：事项完成反馈" /></el-form-item>
        <el-form-item label="触发事件"><el-select v-model="draft.eventType"><el-option v-for="item in events" :key="item.value" :label="item.label" :value="item.value" /></el-select></el-form-item>
      </div>
      <el-form-item label="动作类型"><el-segmented v-model="draft.actionType" :options="[{ label: '打开 URI', value: 'uri' }, { label: '运行本地程序', value: 'process' }]" /></el-form-item>
      <el-form-item v-if="draft.actionType === 'uri'" label="URI 模板"><el-input v-model="draft.uriTemplate" placeholder="motioncue://play/{{task.title}}" /></el-form-item>
      <template v-else>
        <el-form-item label="程序"><el-input v-model="draft.program" placeholder="powershell.exe" /></el-form-item>
        <el-form-item label="参数"><div class="hook-args"><div v-for="(_, index) in draft.args" :key="index" class="hook-arg-row"><el-input v-model="draft.args[index]" :placeholder="index === 0 ? '-NoProfile' : '独立参数，可使用模板变量'" /><el-button text circle :icon="X" aria-label="删除参数" @click="removeArgument(index)" /></div><el-button text :icon="Plus" @click="draft.args.push('')">添加参数</el-button></div></el-form-item>
        <el-form-item label="工作目录（可选）"><el-input v-model="draft.workingDirectory" placeholder="D:/Scripts" /></el-form-item>
      </template>
      <div class="hook-form-grid compact-grid">
        <el-form-item label="超时"><el-input-number v-model="draft.timeoutSeconds" :min="1" :max="60" controls-position="right" /><span class="field-unit">秒</span></el-form-item>
        <el-form-item label="状态"><el-switch v-model="draft.enabled" active-text="启用" inactive-text="停用" /></el-form-item>
      </div>
      <div class="hook-template-vars"><span>可用变量</span><code v-for="variable in templateVariables" :key="variable">{{ variable }}</code></div>
    </el-form>
    <template #footer><div class="hook-dialog-footer"><el-button :icon="Beaker" :loading="testing" @click="runTest(currentRequest())">测试当前配置</el-button><span></span><el-button @click="editorOpen = false">取消</el-button><el-button type="primary" :loading="saving" @click="saveDraft">保存</el-button></div></template>
  </el-dialog>

  <el-dialog v-model="runsOpen" class="hook-runs-dialog" title="Hook 执行记录" width="min(900px, 94vw)" append-to-body>
    <div class="hook-runs-layout">
      <div class="hook-run-list"><button v-for="run in runs" :key="run.id" type="button" :class="{ active: selectedRun?.id === run.id }" @click="selectedRun = run"><i :class="run.status"></i><span><strong>{{ run.hookNameSnapshot }}</strong><small>{{ eventLabel(run.eventType) }} · {{ formatTime(run.createdAt) }}</small></span></button><p v-if="!runs.length">暂无执行记录</p></div>
      <div v-if="selectedRun" class="hook-run-detail">
        <div class="run-summary"><span :class="['run-status', selectedRun.status]">{{ statusLabel(selectedRun.status) }}</span><strong>{{ selectedRun.durationMs ?? 0 }}ms</strong><small v-if="selectedRun.exitCode !== undefined">退出码 {{ selectedRun.exitCode }}</small></div>
        <dl><dt>事件 ID</dt><dd>{{ selectedRun.eventId }}</dd><dt>开始时间</dt><dd>{{ formatTime(selectedRun.startedAt) }}</dd><dt>结束时间</dt><dd>{{ formatTime(selectedRun.finishedAt) }}</dd></dl>
        <div v-if="selectedRun.errorMessage" class="run-output error"><strong>错误</strong><pre>{{ selectedRun.errorMessage }}</pre></div>
        <div v-if="selectedRun.stdoutTail" class="run-output"><strong>标准输出</strong><pre>{{ selectedRun.stdoutTail }}</pre></div>
        <div v-if="selectedRun.stderrTail" class="run-output error"><strong>标准错误</strong><pre>{{ selectedRun.stderrTail }}</pre></div>
      </div>
    </div>
  </el-dialog>
</template>

<style scoped>
.hooks-panel{min-width:0}.hooks-header{display:flex;align-items:center;justify-content:space-between;gap:16px;margin-bottom:14px}.hooks-header h2{margin:0 0 3px;font-size:16px;color:var(--text)}.hooks-header span{font-size:12px;color:var(--muted)}.hooks-header-actions,.hook-actions{display:flex;align-items:center}.hooks-list{min-height:72px;border-top:1px solid var(--line-soft)}.hook-row{display:grid;grid-template-columns:auto minmax(220px,1fr) minmax(105px,auto) auto;align-items:center;gap:12px;min-height:66px;padding:9px 2px;border-bottom:1px solid var(--line-soft)}.hook-row.disabled{opacity:.6}.hook-main{min-width:0}.hook-title-line{display:flex;align-items:center;gap:7px;min-width:0}.hook-title-line strong{overflow:hidden;color:var(--text);font-size:13px;text-overflow:ellipsis;white-space:nowrap}.hook-title-line>span{display:inline-flex;align-items:center;gap:3px;padding:2px 6px;border-radius:4px;background:var(--panel-2);color:var(--text-soft);font-size:11px;white-space:nowrap}.hook-main>small{display:block;overflow:hidden;margin-top:6px;color:var(--muted);font:11px Consolas,monospace;text-overflow:ellipsis;white-space:nowrap}.hook-status{display:grid;grid-template-columns:auto auto;align-items:center;justify-content:start;gap:2px 6px;border:0;background:transparent;color:var(--text-soft);text-align:left;cursor:pointer}.hook-status i{width:7px;height:7px;border-radius:50%;background:var(--muted);grid-row:1/3}.hook-status.succeeded i{background:var(--green)}.hook-status.failed i,.hook-status.timed_out i{background:var(--red)}.hook-status.running i,.hook-status.queued i{background:var(--amber)}.hook-status span{font-size:12px}.hook-status small{color:var(--muted);font-size:10px}.hook-empty{display:flex;align-items:center;justify-content:center;gap:8px;width:100%;min-height:78px;border:0;background:transparent;color:var(--muted);cursor:pointer}.hook-empty:hover{color:var(--accent)}.hook-form-grid{display:grid;grid-template-columns:minmax(0,1.4fr) minmax(180px,.8fr);gap:14px}.compact-grid{grid-template-columns:minmax(180px,1fr) minmax(180px,1fr)}.field-unit{margin-left:8px;color:var(--muted);font-size:12px}.hook-args{display:grid;gap:8px;width:100%}.hook-arg-row{display:grid;grid-template-columns:minmax(0,1fr) 32px;gap:6px}.hook-template-vars{display:flex;flex-wrap:wrap;gap:6px;padding-top:4px}.hook-template-vars>span{width:100%;color:var(--muted);font-size:12px}.hook-template-vars code{padding:3px 6px;border-radius:4px;background:var(--panel-2);color:var(--text-soft);font-size:11px}.hook-dialog-footer{display:grid;grid-template-columns:auto 1fr auto auto;align-items:center;gap:8px}.hook-runs-layout{display:grid;grid-template-columns:minmax(220px,300px) minmax(0,1fr);min-height:360px;max-height:62vh}.hook-run-list{overflow:auto;padding-right:12px;border-right:1px solid var(--line-soft)}.hook-run-list button{display:grid;grid-template-columns:8px minmax(0,1fr);align-items:center;gap:9px;width:100%;padding:10px;border:0;border-radius:6px;background:transparent;color:var(--text);text-align:left;cursor:pointer}.hook-run-list button.active{background:var(--panel-2);font-weight:600}.hook-run-list button>i{width:7px;height:7px;border-radius:50%;background:var(--muted)}.hook-run-list button>i.succeeded{background:var(--green)}.hook-run-list button>i.failed,.hook-run-list button>i.timed_out{background:var(--red)}.hook-run-list button span{min-width:0}.hook-run-list strong,.hook-run-list small{display:block;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.hook-run-list small{margin-top:3px;color:var(--muted);font-size:11px;font-weight:400}.hook-run-detail{overflow:auto;min-width:0;padding-left:18px}.run-summary{display:flex;align-items:center;gap:10px;margin-bottom:16px}.run-summary small{color:var(--muted)}.run-status{padding:3px 7px;border-radius:4px;background:var(--panel-2)}.run-status.succeeded{color:var(--success-text)}.run-status.failed,.run-status.timed_out{color:var(--danger-text)}.hook-run-detail dl{display:grid;grid-template-columns:76px minmax(0,1fr);gap:8px 10px;margin:0 0 16px;font-size:12px}.hook-run-detail dt{color:var(--muted)}.hook-run-detail dd{overflow-wrap:anywhere;margin:0;color:var(--text-soft)}.run-output{margin-top:12px}.run-output strong{display:block;margin-bottom:6px;font-size:12px}.run-output pre{overflow:auto;max-height:180px;margin:0;padding:10px;border-radius:6px;background:var(--panel-2);color:var(--text);font:11px/1.55 Consolas,monospace;white-space:pre-wrap;overflow-wrap:anywhere}.run-output.error strong{color:var(--danger-text)}@media(max-width:760px){.hooks-header{align-items:flex-start}.hooks-header-actions :deep(.el-button span){display:none}.hook-row{grid-template-columns:auto minmax(0,1fr) auto}.hook-status{grid-column:2}.hook-actions{grid-column:3;grid-row:1/3;flex-wrap:wrap;width:80px}.hook-runs-layout{grid-template-columns:1fr}.hook-run-list{max-height:180px;padding:0 0 10px;border:0;border-bottom:1px solid var(--line-soft)}.hook-run-detail{padding:14px 0 0}.hook-form-grid,.compact-grid{grid-template-columns:1fr}}
</style>
