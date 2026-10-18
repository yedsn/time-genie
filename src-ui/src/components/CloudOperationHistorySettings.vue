<script setup lang="ts">
import { ref, watch } from "vue";
import { CheckCircle2, ChevronLeft, ChevronRight, CircleHelp, Clock3, Database, FileText, History, LoaderCircle, Pencil, Play, RefreshCw, RotateCcw, Settings2, Trash2 } from "lucide-vue-next";
import { ElMessage } from "element-plus";
import {
  getCloudOperationHistoryDetail,
  listCloudOperationHistory,
  type CloudOperationHistoryDetail,
  type CloudOperationHistoryItem,
  type CloudOperationHistoryPage,
} from "../services/tauri";

const props = defineProps<{ enabled: boolean }>();
const loading = ref(false);
const detailLoading = ref(false);
const detailOpen = ref(false);
const detailError = ref("");
const page = ref<CloudOperationHistoryPage>({ items: [] });
const selected = ref<CloudOperationHistoryDetail>();
const pageCursors = ref<Array<CloudOperationHistoryPage["nextCursor"]>>([undefined]);
const pageCache = ref<CloudOperationHistoryPage[]>([]);
const pageIndex = ref(0);

const sourceLabels: Record<string, string> = { user: "用户", automation: "自动化", import: "导入", migration: "迁移", restore: "回滚", system: "系统" };
const actionLabels: Record<string, string> = { create: "创建", update: "修改", delete: "删除", restore: "恢复" };
const fieldLabels: Record<string, string> = { title: "标题", status: "状态", planned_date: "计划日期", planned_time: "计划时间", estimate_minutes: "预估分钟", note: "备注", project_name: "项目", solution_name: "方案", completed_at: "完成时间", duration_seconds: "时长（秒）", minutes: "分钟", state: "状态", name: "名称", value_json: "设置值", deleted_at: "删除时间" };

function operationPresentation(item: Pick<CloudOperationHistoryItem, "businessType" | "summary" | "source">) {
  const value = `${item.businessType} ${item.summary}`.toLowerCase();
  if (/restore|rollback|回滚|恢复/.test(value)) return { tone: "restore", label: "恢复", icon: RotateCcw };
  if (/delete|删除/.test(value)) return { tone: "delete", label: "删除", icon: Trash2 };
  if (/complete|完成/.test(value)) return { tone: "complete", label: "完成", icon: CheckCircle2 };
  if (/timer.*start|开始计时/.test(value)) return { tone: "timer", label: "计时", icon: Play };
  if (/timer|time|allocation|adjustment|工时|计时/.test(value)) return { tone: "time", label: "时间", icon: Clock3 };
  if (/report|报告/.test(value)) return { tone: "report", label: "报告", icon: FileText };
  if (/setting|schedule|设置/.test(value)) return { tone: "setting", label: "设置", icon: Settings2 };
  if (/migration|import|snapshot|迁移|导入|快照/.test(value) || item.source === "system") return { tone: "system", label: sourceLabels[item.source] || "系统", icon: Database };
  return { tone: "change", label: "变更", icon: Pencil };
}

function errorText(error: unknown) { return error instanceof Error ? error.message : typeof error === "string" ? error : "操作记录加载失败"; }
function formatDate(value: string) { return new Date(value).toLocaleString("zh-CN"); }
function displayValue(value: unknown) {
  if (value === null || value === undefined || value === "") return "无";
  if (typeof value === "boolean") return value ? "是" : "否";
  if (typeof value === "object") return JSON.stringify(value);
  return String(value);
}

async function loadHistory(reset = true) {
  if (!props.enabled || loading.value) return;
  loading.value = true;
  try {
    if (reset) { pageIndex.value = 0; pageCursors.value = [undefined]; pageCache.value = []; }
    const result = await listCloudOperationHistory({
      limit: 50, cursor: pageCursors.value[pageIndex.value],
    });
    page.value = result;
    pageCache.value[pageIndex.value] = result;
    selected.value = undefined;
  } catch (error) { ElMessage.error(errorText(error)); } finally { loading.value = false; }
}
async function nextPage() {
  if (!page.value.nextCursor || loading.value) return;
  pageCursors.value = [...pageCursors.value.slice(0, pageIndex.value + 1), page.value.nextCursor];
  pageIndex.value += 1;
  const cached = pageCache.value[pageIndex.value];
  if (cached) { page.value = cached; selected.value = undefined; }
  else await loadHistory(false);
}
async function previousPage() {
  if (pageIndex.value === 0 || loading.value) return;
  pageIndex.value -= 1;
  const cached = pageCache.value[pageIndex.value];
  if (cached) { page.value = cached; selected.value = undefined; }
  else await loadHistory(false);
}

async function openDetail(item: CloudOperationHistoryItem) {
  detailOpen.value = true;
  detailError.value = "";
  selected.value = undefined;
  detailLoading.value = true;
  try { selected.value = await getCloudOperationHistoryDetail(item.id); }
  catch (error) { detailError.value = errorText(error); ElMessage.error(detailError.value); }
  finally { detailLoading.value = false; }
}

watch(() => props.enabled, (enabled) => { if (enabled && !page.value.items.length) void loadHistory(); }, { immediate: true });
</script>

<template>
  <div class="operation-history-settings">
    <div class="panel-title history-title">
      <div class="panel-title-heading">
        <h2>云端操作记录</h2>
        <button class="help-tooltip" type="button" aria-label="查看操作记录说明">
          <CircleHelp :size="15" />
          <span role="tooltip">按时间倒序展示成功提交的云端数据变更，默认保留 90 天。记录只读，不能撤销单条修改，过期后也不能用于恢复。</span>
        </button>
      </div>
      <button v-if="enabled" class="secondary-button compact" type="button" :disabled="loading" @click="loadHistory()"><RefreshCw :class="{ 'sync-spin': loading }" :size="13" />刷新</button>
    </div>
    <div v-if="!enabled" class="history-empty"><History :size="24" /><strong>操作记录暂不可用</strong><span>切换到 Supabase 云端模式后即可查看</span></div>
    <template v-else>
      <div v-if="loading && !page.items.length" class="history-empty"><LoaderCircle class="sync-spin" :size="22" /><strong>正在加载操作记录</strong></div>
      <div v-else-if="!page.items.length" class="history-empty"><History :size="24" /><strong>暂无操作记录</strong><span>同步产生的数据变更会显示在这里</span></div>
      <div v-else class="history-list">
        <div class="history-scroll">
          <button v-for="item in page.items" :key="item.id" type="button" @click="openDetail(item)">
            <span :class="['history-type-mark', operationPresentation(item).tone]"><component :is="operationPresentation(item).icon" :size="14" /><small>{{ operationPresentation(item).label }}</small></span>
            <div class="history-list-main"><strong>{{ item.summary }}</strong><div class="history-list-meta"><span><i>来源</i>{{ sourceLabels[item.source] || item.source }}</span><span><i>设备</i>{{ item.deviceName }}</span><span><i>时间</i>{{ formatDate(item.occurredAt) }}</span><span><i>影响</i>{{ item.affectedEntityCount }} 项</span></div></div><ChevronRight class="history-row-arrow" :size="14" />
          </button>
        </div>
        <div class="history-pagination"><button class="secondary-button compact" type="button" :disabled="loading || pageIndex === 0" @click="previousPage"><ChevronLeft :size="14" />上一页</button><span>第 {{ pageIndex + 1 }} 页</span><button class="secondary-button compact" type="button" :disabled="loading || !page.nextCursor" @click="nextPage">下一页<ChevronRight :size="14" /></button></div>
      </div>
    </template>

    <el-dialog v-model="detailOpen" title="操作记录详情" width="min(720px, calc(100vw - 32px))">
      <div class="history-detail">
          <div v-if="detailLoading" class="history-empty"><LoaderCircle class="sync-spin" :size="22" /><strong>正在加载详情</strong></div>
          <div v-else-if="detailError" class="history-empty error"><History :size="24" /><strong>详情加载失败</strong><span>{{ detailError }}</span></div>
          <template v-else-if="selected">
            <header><div><strong>{{ selected.summary }}</strong><span>{{ selected.deviceNameSnapshot }} · {{ formatDate(selected.occurredAt) }}</span></div><span>{{ sourceLabels[selected.source] || selected.source }}</span></header>
            <article v-for="item in selected.items" :key="item.id" class="history-entity">
              <div class="history-entity-title"><strong>{{ item.entityLabel }}</strong><span>{{ actionLabels[item.action] || item.action }} · {{ item.entityType }}</span></div>
              <div v-if="item.changedFields.length" class="history-diff">
                <div v-for="field in item.changedFields" :key="field"><span>{{ fieldLabels[field] || field }}</span><del>{{ displayValue(item.before?.[field]) }}</del><ins>{{ displayValue(item.after?.[field]) }}</ins></div>
              </div>
              <p v-else>该操作没有可展示的字段变化。</p>
            </article>
            <small class="history-expiry">保留至 {{ formatDate(selected.expiresAt) }}。到期清理后，此记录不可再查看或恢复。</small>
          </template>
      </div>
    </el-dialog>
  </div>
</template>

<style scoped>
.operation-history-settings { display: grid; gap: 9px; }
.history-title { display: flex; justify-content: space-between; align-items: center; gap: 10px; }.history-title > button { display: inline-flex; align-items: center; gap: 5px; white-space: nowrap; }
.history-list { display: grid; grid-template-rows: minmax(0,1fr) auto; height: 324px; border: 1px solid var(--line-soft); border-radius: 11px; overflow: hidden; }
.history-scroll { min-height: 0; overflow-y: auto; }
.history-scroll > button { display: grid; grid-template-columns: 40px minmax(0,1fr) 14px; align-items: center; gap: 9px; width: 100%; height: 56px; padding: 7px 9px; text-align: left; border: 0; border-bottom: 1px solid var(--line-soft); background: transparent; color: inherit; cursor: pointer; }
.history-scroll > button:hover { background: var(--surface-soft); }.history-list-main { min-width: 0; display: grid; gap: 3px; }.history-list button strong { overflow: hidden; font-size: 12px; font-weight: 600; text-overflow: ellipsis; white-space: nowrap; }
.history-type-mark { display: grid; width: 38px; min-height: 36px; place-items: center; align-content: center; gap: 1px; border-radius: 7px; background: var(--surface-soft); color: var(--text-secondary); }.history-type-mark small { color: currentColor; font-size: 8px; line-height: 1; }.history-type-mark.complete { background: rgba(50,160,105,.1); color: #237451; }.history-type-mark.delete { background: rgba(190,70,70,.1); color: #a84949; }.history-type-mark.restore { background: rgba(118,88,180,.11); color: #7452a8; }.history-type-mark.timer,.history-type-mark.time { background: rgba(52,120,190,.1); color: #376f9f; }.history-type-mark.report { background: rgba(190,135,45,.11); color: #8c651d; }.history-type-mark.setting,.history-type-mark.system { background: rgba(100,110,125,.1); color: #64707d; }.history-type-mark.change { background: color-mix(in srgb,var(--brand) 10%,transparent); color: var(--brand); }
.history-list-meta { display: flex; min-width: 0; gap: 5px 13px; color: color-mix(in srgb,var(--text-secondary) 78%,transparent); font-size: 10px; white-space: nowrap; overflow: hidden; }.history-list-meta span { display: inline-flex; min-width: 0; gap: 4px; overflow: hidden; text-overflow: ellipsis; }.history-list-meta i { color: color-mix(in srgb,var(--text-secondary) 58%,transparent); font-size: 9px; font-style: normal; }.history-list-meta span:nth-child(2) { max-width: 180px; }.history-row-arrow { color: color-mix(in srgb,var(--text-secondary) 55%,transparent); }
.history-list span, .history-detail header span, .history-expiry { color: var(--text-secondary); font-size: 10px; }
.history-pagination { display: flex; justify-content: space-between; align-items: center; gap: 7px; padding: 7px 8px; color: var(--text-secondary); font-size: 10px; }
.history-pagination button { display: inline-flex; align-items: center; gap: 4px; }
.history-detail { min-height: 150px; max-height: min(620px, calc(100vh - 210px)); overflow-y: auto; font-size: 12px; }
.history-detail header, .history-entity-title { display: flex; justify-content: space-between; align-items: flex-start; gap: 10px; }
.history-detail header div { display: grid; gap: 4px; }
.history-entity { margin-top: 10px; padding-top: 9px; border-top: 1px solid var(--line-soft); }
.history-entity-title span { color: var(--text-secondary); font-size: 10px; }
.history-diff { display: grid; gap: 5px; margin-top: 7px; }
.history-diff > div { display: grid; grid-template-columns: minmax(70px,.45fr) minmax(0,1fr) minmax(0,1fr); gap: 5px; align-items: start; font-size: 11px; }
.history-diff del, .history-diff ins { padding: 4px 6px; border-radius: 5px; text-decoration: none; overflow-wrap: anywhere; }
.history-diff del { background: rgba(190,70,70,.1); color: #a84949; }
.history-diff ins { background: rgba(50,160,105,.1); color: #237451; }
.history-empty { display: flex; flex-direction: column; justify-content: center; align-items: center; gap: 5px; min-height: 150px; padding: 20px; color: var(--text-secondary); text-align: center; }.operation-history-settings > .history-empty { box-sizing: border-box; height: 324px; min-height: 0; }.history-empty strong { color: var(--text-primary); font-size: 12px; font-weight: 600; }.history-empty span { max-width: 520px; font-size: 11px; }.history-empty.error { color: #a84949; }.history-empty.error strong { color: #a84949; }
.history-expiry { display: block; margin-top: 11px; }
@media (max-width: 600px) { .history-list-meta span:nth-child(2) { display: none; }.history-scroll > button { grid-template-columns: 36px minmax(0,1fr) 14px; }.history-type-mark { width: 34px; } }
</style>
