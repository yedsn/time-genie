<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { CircleHelp, Download, ExternalLink, FileArchive, FileSearch, FolderOpen, LoaderCircle, ShieldCheck, Upload } from "lucide-vue-next";
import { open, save } from "@tauri-apps/plugin-dialog";
import { ElMessage, ElMessageBox } from "element-plus";
import {
  exportDataBackup,
  importDataBackup,
  listDataBackupHistory,
  openDataBackup,
  previewDataBackup,
  type DataBackupExportResult,
  type DataBackupHistoryItem,
  type DataBackupImportResult,
  type DataBackupPreview,
} from "../services/tauri";

const emit = defineEmits<{ imported: [] }>();
const working = ref<"export" | "preview" | "import" | "">("");
const exportResult = ref<DataBackupExportResult>();
const preview = ref<DataBackupPreview>();
const restoreOpen = ref(false);
const importResult = ref<DataBackupImportResult>();
const history = ref<DataBackupHistoryItem[]>([]);
const historyLoading = ref(false);
const openingPath = ref("");

const totalEntities = computed(() =>
  Object.values(preview.value?.entityCounts ?? {}).reduce((sum, count) => sum + count, 0),
);
const replacedEntities = computed(() =>
  Object.values(preview.value?.replacedEntityCounts ?? {}).reduce((sum, count) => sum + count, 0),
);

function errorText(error: unknown, fallback: string) {
  return error instanceof Error ? error.message : typeof error === "string" ? error : fallback;
}
function formatBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

async function loadHistory() {
  if (historyLoading.value) return;
  historyLoading.value = true;
  try { history.value = await listDataBackupHistory(); }
  catch (error) { ElMessage.error(errorText(error, "备份历史加载失败")); }
  finally { historyLoading.value = false; }
}

async function exportBackup() {
  const path = await save({
    title: "导出 TimeGenie 本地备份",
    defaultPath: `timegenie-${new Date().toISOString().slice(0, 10)}.timegenie-backup`,
    filters: [{ name: "TimeGenie 备份", extensions: ["timegenie-backup"] }],
  });
  if (!path) return;
  working.value = "export";
  try {
    exportResult.value = await exportDataBackup(path);
    await loadHistory();
    ElMessage.success("本地备份已导出并校验通过");
  } catch (error) {
    ElMessage.error(errorText(error, "备份导出失败"));
  } finally {
    working.value = "";
  }
}

async function previewHistory(item: DataBackupHistoryItem) {
  if (!item.fileExists) { ElMessage.warning("备份文件已被移动或删除"); return; }
  restoreOpen.value = true;
  working.value = "preview";
  importResult.value = undefined;
  preview.value = undefined;
  try { preview.value = await previewDataBackup(item.path); }
  catch (error) { restoreOpen.value = false; ElMessage.error(errorText(error, "备份文件无法读取")); }
  finally { working.value = ""; }
}

async function openHistory(item: DataBackupHistoryItem, reveal: boolean) {
  openingPath.value = item.path;
  try { await openDataBackup(item.path, reveal); }
  catch (error) { ElMessage.error(errorText(error, reveal ? "无法打开备份所在位置" : "无法打开备份文件")); await loadHistory(); }
  finally { openingPath.value = ""; }
}

async function selectBackup() {
  const path = await open({
    title: "选择 TimeGenie 备份",
    multiple: false,
    directory: false,
    filters: [{ name: "TimeGenie 备份", extensions: ["timegenie-backup"] }],
  });
  if (!path || Array.isArray(path)) return;
  restoreOpen.value = true;
  working.value = "preview";
  importResult.value = undefined;
  preview.value = undefined;
  try {
    preview.value = await previewDataBackup(path);
  } catch (error) {
    restoreOpen.value = false;
    preview.value = undefined;
    ElMessage.error(errorText(error, "备份文件无法读取"));
  } finally {
    working.value = "";
  }
}

async function restoreBackup() {
  if (!preview.value || working.value) return;
  try {
    await ElMessageBox.confirm(
      `将用备份中的 ${totalEntities.value} 条业务数据替换当前 ${replacedEntities.value} 条数据。账号、设备授权、同步队列和凭据不会恢复；完成后将切换为本地模式。`,
      "确认恢复本地备份",
      {
        type: "warning",
        confirmButtonText: "创建安全备份并恢复",
        cancelButtonText: "取消",
        closeOnClickModal: false,
        customClass: "work-confirm-dialog",
      },
    );
  } catch {
    return;
  }
  working.value = "import";
  try {
    importResult.value = await importDataBackup(preview.value.path);
    restoreOpen.value = false;
    await loadHistory();
    ElMessage.success("数据已恢复为独立的本地工作空间");
    emit("imported");
  } catch (error) {
    ElMessage.error(errorText(error, "备份恢复失败，当前数据未修改"));
  } finally {
    working.value = "";
  }
}

onMounted(() => { void loadHistory(); });
</script>

<template>
  <div class="backup-settings">
    <div class="panel-title">
      <div class="panel-title-heading">
        <h2>本地数据备份</h2>
        <button class="help-tooltip" type="button" aria-label="查看本地备份说明">
          <CircleHelp :size="15" />
          <span role="tooltip">备份包含事项、工时、报告和共享设置，不包含设备身份、账号凭据、Supabase 配置、自动化命令或待同步队列。</span>
        </button>
      </div>
    </div>
    <div class="backup-actions">
      <button class="primary-button compact" type="button" :disabled="Boolean(working)" @click="exportBackup">
        <LoaderCircle v-if="working === 'export'" class="sync-spin" :size="14" /><Download v-else :size="14" />
        {{ working === 'export' ? '正在创建...' : '导出本地备份' }}
      </button>
      <button class="secondary-button compact" type="button" :disabled="Boolean(working)" @click="selectBackup">
        <LoaderCircle v-if="working === 'preview'" class="sync-spin" :size="14" /><FileSearch v-else :size="14" />
        {{ working === 'preview' ? '正在校验...' : '从备份文件恢复' }}
      </button>
    </div>
    <div class="backup-history">
      <div class="backup-history-title"><strong>备份记录</strong><span>仅保存在当前设备</span></div>
      <div v-if="historyLoading && !history.length" class="backup-empty"><LoaderCircle class="sync-spin" :size="20" /><span>正在读取备份记录</span></div>
      <div v-else-if="!history.length" class="backup-empty"><FileArchive :size="22" /><strong>暂无本地备份</strong><span>导出后的备份会显示在这里</span></div>
      <div v-else class="backup-list">
        <article v-for="item in history" :key="item.id" :class="['backup-row', { missing: !item.fileExists }]">
          <div class="backup-file-icon"><FileArchive :size="16" /></div>
          <button class="backup-row-main" type="button" :disabled="!item.fileExists || Boolean(working)" @click="previewHistory(item)">
            <strong>{{ item.fileName }}</strong>
            <span>{{ new Date(item.createdAt).toLocaleString('zh-CN') }} · {{ formatBytes(item.sizeBytes) }} · {{ item.entityCount }} 条</span>
            <small>{{ item.path }}</small>
          </button>
          <span class="backup-kind">{{ !item.fileExists ? '文件不存在' : item.origin === 'safety' ? '安全备份' : '手动备份' }}</span>
          <div class="backup-row-actions">
            <button class="backup-action danger" type="button" :disabled="!item.fileExists || Boolean(working)" title="校验并恢复这份备份" @click="previewHistory(item)"><Upload :size="13" />恢复</button>
            <button class="backup-action" type="button" :disabled="!item.fileExists || openingPath === item.path" title="使用系统打开备份文件" @click="openHistory(item, false)"><ExternalLink :size="13" />打开</button>
            <button class="backup-action" type="button" :disabled="!item.fileExists || openingPath === item.path" title="在文件夹中显示" @click="openHistory(item, true)"><FolderOpen :size="13" />位置</button>
          </div>
        </article>
      </div>
    </div>
    <div v-if="importResult" class="backup-result success">
      <ShieldCheck :size="17" /><div><strong>恢复完成，导入前安全备份已保留</strong><span>{{ importResult.safetyBackupPath }}</span></div>
    </div>

    <el-dialog v-model="restoreOpen" title="恢复本地备份" width="min(620px, calc(100vw - 32px))" :close-on-click-modal="!working" :show-close="!working">
      <div v-if="working === 'preview'" class="backup-empty restore-loading"><LoaderCircle class="sync-spin" :size="20" /><strong>正在校验备份文件</strong><span>正在检查文件完整性和数据引用</span></div>
      <div v-else-if="preview" class="backup-preview">
        <header><div><strong>{{ preview.workspaceName }}</strong><span>{{ new Date(preview.createdAt).toLocaleString('zh-CN') }} · v{{ preview.appVersion }}</span></div><span>{{ preview.backupKind === 'stale_epoch_rescue' ? '设备救援备份' : '普通备份' }}</span></header>
        <div class="backup-counts"><span><strong>{{ totalEntities }}</strong><small>将恢复的数据</small></span><span><strong>{{ replacedEntities }}</strong><small>当前将被替换</small></span><span><strong>{{ preview.timezone }}</strong><small>工作空间时区</small></span></div>
        <p class="restore-impact">恢复会先自动备份当前数据，再用这份备份替换事项、时间记录、报告和共享设置。完成后将切换为独立的本地模式。</p>
        <details><summary>不会恢复的内容</summary><ul><li v-for="item in preview.excludedCategories" :key="item">{{ item }}</li></ul></details>
      </div>
      <template #footer>
        <button class="secondary-button" type="button" :disabled="Boolean(working)" @click="restoreOpen = false">取消</button>
        <button class="danger-button backup-restore-button" type="button" :disabled="Boolean(working) || !preview" @click="restoreBackup">
          <LoaderCircle v-if="working === 'import'" class="sync-spin" :size="14" /><Upload v-else :size="14" />
          {{ working === 'import' ? '正在恢复...' : '创建安全备份并恢复' }}
        </button>
      </template>
    </el-dialog>
  </div>
</template>

<style scoped>
.backup-settings { display: grid; gap: 10px; }
.backup-actions { display: flex; flex-wrap: wrap; gap: 8px; }
.backup-actions button, .backup-restore-button { display: inline-flex; align-items: center; gap: 6px; }
.backup-history { display: grid; grid-template-rows: auto minmax(0,1fr); height: 324px; overflow: hidden; border: 1px solid var(--line-soft); border-radius: 9px; }.backup-history-title { display: flex; justify-content: space-between; align-items: center; min-height: 34px; padding: 0 10px; border-bottom: 1px solid var(--line-soft); }.backup-history-title strong { font-size: 11px; }.backup-history-title span { color: var(--text-secondary); font-size: 10px; }.backup-list { display: grid; min-height: 0; overflow-y: auto; }.backup-row { display: grid; grid-template-columns: 30px minmax(0,1fr) auto auto; align-items: center; gap: 8px; height: 58px; padding: 7px 9px; border-bottom: 1px solid var(--line-soft); }.backup-row:last-child { border-bottom: 0; }.backup-row.missing { opacity: .58; }.backup-file-icon { display: grid; width: 28px; height: 28px; place-items: center; border-radius: 7px; background: color-mix(in srgb,var(--brand) 10%,transparent); color: var(--brand); }.backup-row-main { display: grid; min-width: 0; gap: 2px; padding: 0; border: 0; background: transparent; color: inherit; text-align: left; cursor: pointer; }.backup-row-main:disabled { cursor: default; }.backup-row-main strong,.backup-row-main span,.backup-row-main small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }.backup-row-main strong { font-size: 11px; font-weight: 600; }.backup-row-main span { color: var(--text-secondary); font-size: 10px; }.backup-row-main small { color: var(--text-secondary); font-size: 9px; opacity: .75; }.backup-kind { padding: 2px 6px; border-radius: 5px; background: var(--surface-soft); color: var(--text-secondary); font-size: 9px; white-space: nowrap; }.backup-row-actions { display: flex; align-items: center; gap: 5px; }.backup-action { display: inline-flex; align-items: center; gap: 3px; padding: 4px 6px; border: 0; border-radius: 5px; background: transparent !important; box-shadow: none !important; color: var(--text-secondary); cursor: pointer; white-space: nowrap; }.backup-action:hover:not(:disabled) { color: var(--brand); }.backup-action.danger,.backup-action.danger:hover:not(:disabled) { color: #b44b4b; }.backup-action:disabled { background: transparent !important; color: color-mix(in srgb,var(--text-secondary) 45%,transparent); cursor: not-allowed; opacity: 1; }.backup-action.danger:disabled { color: color-mix(in srgb,#b44b4b 42%,transparent); }.backup-empty { display: flex; flex-direction: column; justify-content: center; align-items: center; gap: 4px; min-height: 110px; padding: 18px; color: var(--text-secondary); font-size: 11px; text-align: center; }.backup-history > .backup-empty { min-height: 0; }.backup-empty strong { color: var(--text-primary); font-size: 12px; }
.backup-result { display: flex; gap: 8px; align-items: flex-start; padding: 9px 11px; border-radius: 8px; font-size: 12px; }
.backup-result.success { color: var(--success-text, #237451); background: rgba(50, 160, 105, .1); }
.backup-result div { min-width: 0; display: grid; gap: 4px; }
.backup-result span { overflow-wrap: anywhere; font-size: 12px; }
.backup-preview { display: grid; gap: 10px; padding: 11px; border: 1px solid var(--line-soft); border-radius: 9px; }
.backup-preview header { display: flex; align-items: flex-start; justify-content: space-between; gap: 12px; }
.backup-preview header div { display: grid; gap: 3px; }
.backup-preview header strong { font-size: 12px; }.backup-preview header span, .backup-preview small { color: var(--text-secondary); font-size: 10px; }
.backup-counts { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 8px; }
.backup-counts span { display: grid; gap: 2px; padding: 7px 8px; border-radius: 7px; background: var(--surface-soft); font-size: 11px; }
.backup-counts strong { overflow-wrap: anywhere; }
.backup-preview details { color: var(--text-secondary); font-size: 11px; }
.backup-preview summary { cursor: pointer; color: var(--text-primary); }
.backup-preview ul { margin: 8px 0 0; padding-left: 20px; line-height: 1.7; }
.restore-impact { margin: 0; padding: 9px 10px; border-radius: 7px; background: color-mix(in srgb,#a84949 7%,var(--surface-soft)); color: var(--text-secondary); font-size: 11px; line-height: 1.6; }
.restore-loading { min-height: 220px; }
.backup-restore-button { justify-self: start; }
@media (max-width: 720px) { .backup-counts { grid-template-columns: 1fr; }.backup-row { grid-template-columns: 30px minmax(0,1fr) auto; }.backup-kind { display: none; }.backup-row-actions { grid-column: 2 / 4; } }
</style>
