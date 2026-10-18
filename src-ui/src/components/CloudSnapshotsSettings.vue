<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { Archive, CircleHelp, Eye, History, LoaderCircle, Plus, RefreshCw, Shield, ShieldOff, Trash2, TriangleAlert } from "lucide-vue-next";
import { ElMessage, ElMessageBox } from "element-plus";
import {
  createCloudWorkspaceSnapshot, deleteCloudWorkspaceSnapshot, listCloudWorkspaceSnapshots,
  previewCloudWorkspaceSnapshot, setCloudWorkspaceSnapshotProtected, prepareCloudWorkspaceRestore,
  commitCloudWorkspaceRestore, cancelCloudWorkspaceRestore,
  getCloudWorkspaceSnapshotSettings, updateCloudWorkspaceSnapshotSettings,
  type CloudWorkspaceSnapshot, type CloudWorkspaceSnapshotPreview, type CloudWorkspaceSnapshotSettings,
  type CloudWorkspaceRestorePrepareResult,
} from "../services/tauri";

const props = defineProps<{ enabled: boolean; afterRestore?: () => Promise<void> | void }>();
const snapshots = ref<CloudWorkspaceSnapshot[]>([]);
const loading = ref(false);
const workingId = ref("");
const createOpen = ref(false);
const creating = ref(false);
const name = ref("");
const note = ref("");
const selected = ref<CloudWorkspaceSnapshot>();
const preview = ref<CloudWorkspaceSnapshotPreview>();
const previewLoading = ref(false);
const previewOpen = ref(false);
const restoreOpen = ref(false);
const restorePreparing = ref(false);
const restoreCommitting = ref(false);
const restorePrepared = ref<CloudWorkspaceRestorePrepareResult>();
const restoreNameConfirmation = ref("");
const createPreRestoreSnapshot = ref(false);
const automaticEnabled = ref(true);
const automaticTime = ref("03:00");
const automaticTimezone = ref("Asia/Shanghai");
const schedulerActive = ref(true);
const nextAutomaticAt = ref<string | null>(null);
const lastAutomaticAt = ref<string | null>(null);
const settingsLoading = ref(false);
const settingsSaving = ref(false);
const retentionDays = ref(30);

const readyCount = computed(() => snapshots.value.filter((item) => item.status === "ready").length);
const totalSize = computed(() => snapshots.value.reduce((sum, item) => sum + item.sizeBytes, 0));
const typeLabels: Record<string, string> = { manual: "手动", automatic: "自动", pre_restore: "恢复前安全" };
const statusLabels: Record<string, string> = { creating: "创建中", ready: "可用", corrupt: "已损坏", failed: "创建失败" };
const groupLabels: Record<string, string> = { planning: "计划与事项", time_tracking: "时间记录", reports: "报告", shared_settings: "共享设置" };
const entityLabels: Record<string, string> = {
  subjects: "主体", subject_work_schedules: "工作安排", subject_work_schedule_segments: "工作时段",
  work_days: "工作日", tasks: "事项", task_status_events: "状态记录", task_daily_estimates: "每日预估",
  task_recurrence_rules: "重复规则", task_occurrences: "事项实例", time_entries: "时间记录", time_segments: "计时分段",
  time_allocations: "时间分配", actual_time_adjustments: "实际工时调整", unassigned_sessions: "未归属会话",
  unassigned_segments: "未归属分段", report_templates: "报告模板", reports: "报告", report_tasks: "报告事项",
  app_settings: "共享设置", integration_configs: "集成配置", external_bindings: "外部绑定",
};
const previewEntities = computed(() => Object.entries(preview.value?.entityCounts ?? {}).filter(([, count]) => count > 0));
const scheduleStatusText = computed(() => {
  if (!automaticEnabled.value) return lastAutomaticAt.value ? `已关闭 · 上次自动快照 ${formatDate(lastAutomaticAt.value)}` : "已关闭";
  if (!schedulerActive.value) return "云端定时任务未启用，自动快照不会执行";
  const last = lastAutomaticAt.value ? `上次 ${formatDate(lastAutomaticAt.value)}` : "尚未创建";
  const next = nextAutomaticAt.value ? ` · 下次 ${formatDate(nextAutomaticAt.value)}` : "";
  return last + next;
});

function errorText(error: unknown, fallback: string) { return error instanceof Error ? error.message : typeof error === "string" ? error : fallback; }
function formatDate(value?: string) { return value ? new Date(value).toLocaleString("zh-CN") : "长期保留"; }
function formatBytes(bytes: number) {
  if (bytes < 1024) return bytes + " B";
  if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + " KB";
  return (bytes / 1024 / 1024).toFixed(1) + " MB";
}
function totalEntities(item: CloudWorkspaceSnapshot) { return Object.values(item.entityCounts).reduce((sum, count) => sum + count, 0); }

async function loadSnapshots() {
  if (!props.enabled || loading.value) return;
  loading.value = true;
  try { snapshots.value = await listCloudWorkspaceSnapshots(); }
  catch (error) { ElMessage.error(errorText(error, "云端快照加载失败")); }
  finally { loading.value = false; }
}

async function loadSettings() {
  if (!props.enabled || settingsLoading.value) return;
  settingsLoading.value = true;
  try {
    applySettings(await getCloudWorkspaceSnapshotSettings());
  } catch (error) { ElMessage.error(errorText(error, "自动快照设置加载失败")); }
  finally { settingsLoading.value = false; }
}

function applySettings(settings: CloudWorkspaceSnapshotSettings) {
  automaticEnabled.value = settings.automaticSnapshotEnabled;
  automaticTime.value = settings.automaticSnapshotTime;
  automaticTimezone.value = settings.automaticSnapshotTimezone;
  retentionDays.value = settings.snapshotRetentionDays;
  nextAutomaticAt.value = settings.nextAutomaticSnapshotAt;
  lastAutomaticAt.value = settings.lastAutomaticSnapshotAt;
  schedulerActive.value = settings.schedulerActive;
}

async function saveSettings() {
  if (!props.enabled || settingsSaving.value) return;
  const days = Math.trunc(Number(retentionDays.value));
  if (!Number.isFinite(days) || days < 1 || days > 365) {
    ElMessage.error("最长保留天数需要在 1 到 365 天之间");
    return;
  }
  const time = automaticTime.value.trim();
  if (!/^([01]\d|2[0-3]):[0-5]\d$/.test(time)) {
    ElMessage.error("自动快照时间格式应为 HH:MM");
    return;
  }
  settingsSaving.value = true;
  try {
    const settings = await updateCloudWorkspaceSnapshotSettings(automaticEnabled.value, time, days);
    applySettings(settings);
    await loadSnapshots();
    ElMessage.success(settings.automaticSnapshotEnabled ? `已保存，每天 ${settings.automaticSnapshotTime} 自动快照` : "已关闭自动快照");
  } catch (error) { ElMessage.error(errorText(error, "快照设置保存失败")); }
  finally { settingsSaving.value = false; }
}

function showCreate() {
  name.value = "手动快照 " + new Date().toLocaleString("zh-CN", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" });
  note.value = "";
  createOpen.value = true;
}

async function createSnapshot() {
  if (!name.value.trim() || creating.value) return;
  creating.value = true;
  try {
    const result = await createCloudWorkspaceSnapshot({ name: name.value.trim(), note: note.value.trim() || undefined });
    createOpen.value = false;
    await loadSnapshots();
    if (result.status === "ready") ElMessage.success("云端快照已创建并校验通过");
    else ElMessage.error("快照创建失败：" + (result.failureReason || "请稍后重试"));
  } catch (error) { ElMessage.error(errorText(error, "快照创建失败")); }
  finally { creating.value = false; }
}

async function toggleProtected(item: CloudWorkspaceSnapshot) {
  workingId.value = item.id;
  try {
    await setCloudWorkspaceSnapshotProtected(item.id, !item.protected);
    ElMessage.success(item.protected ? "已解除快照保护" : "快照已保护，不会被自动清理");
    await loadSnapshots();
  } catch (error) { ElMessage.error(errorText(error, "保护状态更新失败")); }
  finally { workingId.value = ""; }
}

async function removeSnapshot(item: CloudWorkspaceSnapshot) {
  try {
    await ElMessageBox.confirm("确定删除“" + item.name + "”吗？快照内容将永久删除，当前业务数据和操作记录不受影响。", "删除云端快照", { type: "warning", confirmButtonText: "删除快照", cancelButtonText: "取消", customClass: "work-confirm-dialog" });
  } catch { return; }
  workingId.value = item.id;
  try {
    await deleteCloudWorkspaceSnapshot(item.id);
    if (selected.value?.id === item.id) { previewOpen.value = false; selected.value = undefined; preview.value = undefined; }
    ElMessage.success("快照已删除");
    await loadSnapshots();
  } catch (error) { ElMessage.error(errorText(error, "快照删除失败")); }
  finally { workingId.value = ""; }
}

async function showPreview(item: CloudWorkspaceSnapshot) {
  selected.value = item;
  preview.value = undefined;
  previewOpen.value = true;
  previewLoading.value = true;
  try {
    preview.value = await previewCloudWorkspaceSnapshot(item.id);
    if (!preview.value.restorable) ElMessage.error("这份快照当前不可恢复");
  } catch (error) { ElMessage.error(errorText(error, "差异预览失败")); }
  finally { previewLoading.value = false; }
}

async function beginRestore(item?: CloudWorkspaceSnapshot) {
  if (item && selected.value?.id !== item.id) {
    selected.value = item;
    preview.value = undefined;
    previewLoading.value = true;
    try { preview.value = await previewCloudWorkspaceSnapshot(item.id); }
    catch (error) { ElMessage.error(errorText(error, "快照校验失败")); return; }
    finally { previewLoading.value = false; }
  }
  if (!selected.value || !preview.value?.restorable || restorePreparing.value) {
    if (preview.value && !preview.value.restorable) ElMessage.error("快照完整性校验失败，不能恢复");
    return;
  }
  restoreOpen.value = true;
  restoreNameConfirmation.value = "";
  createPreRestoreSnapshot.value = false;
  restorePrepared.value = undefined;
}

async function closeRestore() {
  if (restoreCommitting.value) return;
  if (restorePrepared.value?.lockToken) {
    try { await cancelCloudWorkspaceRestore(restorePrepared.value.lockToken); } catch { /* 锁会自动过期 */ }
  }
  restorePrepared.value = undefined;
  restoreOpen.value = false;
}

async function commitRestore() {
  if (!selected.value || restoreNameConfirmation.value !== selected.value.name || restoreCommitting.value) return;
  restoreCommitting.value = true;
  let restoreCommitted = false;
  try {
    if (!restorePrepared.value) {
      restorePreparing.value = true;
      restorePrepared.value = await prepareCloudWorkspaceRestore(selected.value.id, createPreRestoreSnapshot.value);
      restorePreparing.value = false;
    }
    const result = await commitCloudWorkspaceRestore({
      lockToken: restorePrepared.value.lockToken,
      targetSnapshotId: restorePrepared.value.targetSnapshotId,
      targetContentHash: restorePrepared.value.targetContentHash,
      preparedEpoch: restorePrepared.value.preparedEpoch,
    });
    restoreCommitted = result.restored && result.localCacheRebuilt;
    restorePrepared.value = undefined;
    restoreOpen.value = false;
    previewOpen.value = false;
    await props.afterRestore?.();
    ElMessage.success(`已恢复到“${selected.value.name}”时点，当前设备数据已重新下载`);
  } catch (error) {
    const message = errorText(error, "云端恢复失败，当前数据未改变");
    if (restoreCommitted) ElMessage.warning("云端和本机数据已经恢复，但当前页面重新载入失败，请重新打开应用查看恢复后的数据");
    else if (message.includes("RESTORE_LOCAL_REBUILD_FAILED")) ElMessage.error(message.replace("RESTORE_LOCAL_REBUILD_FAILED: ", ""));
    else ElMessage.error(message);
  }
  finally { restorePreparing.value = false; restoreCommitting.value = false; await loadSnapshots(); }
}

watch(() => props.enabled, (enabled) => {
  if (enabled) { void loadSnapshots(); void loadSettings(); }
  else { snapshots.value = []; previewOpen.value = false; selected.value = undefined; preview.value = undefined; }
}, { immediate: true });
</script>

<template>
  <div class="snapshot-settings">
    <div class="panel-title">
      <div class="panel-title-heading">
        <h2>云端快照</h2>
        <button class="help-tooltip" type="button" aria-label="查看云端快照说明">
          <CircleHelp :size="15" />
          <span role="tooltip">快照是完整恢复点，包含全部共享业务数据和非敏感设置，不支持局部快照或局部恢复。账号、凭据、设备授权、同步队列和操作记录不会写入快照。自动快照由云端每天在设定时间创建一次，按工作区时区计算，不占用本机同步；每天最多一份。</span>
        </button>
      </div>
    </div>
    <div v-if="!enabled" class="snapshot-empty"><Archive :size="24" /><strong>云端快照暂不可用</strong><span>登录并切换到 Supabase 云端模式后即可使用</span></div>
    <template v-else>
      <div class="snapshot-schedule">
        <div class="snapshot-schedule-copy">
          <strong>自动快照</strong>
          <span v-if="settingsLoading">读取中...</span>
          <span v-else>{{ automaticEnabled ? `每天 ${automaticTime}（${automaticTimezone}）` : '已关闭' }} · 未保护快照最长保留 {{ retentionDays }} 天</span>
        </div>
        <div class="snapshot-schedule-actions">
          <label class="snapshot-toggle"><input v-model="automaticEnabled" type="checkbox" :disabled="settingsLoading || settingsSaving" /><span>启用</span></label>
          <label class="snapshot-retention-field"><span>每天</span><input v-model="automaticTime" type="time" step="60" :disabled="settingsLoading || settingsSaving || !automaticEnabled" /><span>执行</span></label>
          <label class="snapshot-retention-field"><span>最长保留</span><input v-model.number="retentionDays" type="number" min="1" max="365" step="1" :disabled="settingsLoading || settingsSaving" /><span>天</span></label>
          <button class="primary-button compact" type="button" :disabled="settingsLoading || settingsSaving" @click="saveSettings"><LoaderCircle v-if="settingsSaving" class="sync-spin" :size="14" />{{ settingsSaving ? '保存中...' : '保存' }}</button>
        </div>
      </div>
      <div v-if="scheduleStatusText" :class="['automatic-status', { failed: !schedulerActive }]"><TriangleAlert v-if="!schedulerActive" :size="14" /><span>{{ scheduleStatusText }}</span></div>
      <div class="snapshot-toolbar">
        <div class="snapshot-toolbar-summary"><span><strong>{{ readyCount }}</strong> 份可用快照</span><span>当前占用 {{ formatBytes(totalSize) }}</span></div>
        <div class="snapshot-toolbar-actions"><button class="secondary-button compact" type="button" :disabled="loading" @click="loadSnapshots"><RefreshCw :class="{ 'sync-spin': loading }" :size="14" />刷新</button><button class="primary-button compact" type="button" :disabled="creating" @click="showCreate"><Plus :size="14" />创建手动快照</button></div>
      </div>
      <div v-if="loading && !snapshots.length" class="snapshot-empty"><LoaderCircle class="sync-spin" :size="22" /><strong>正在加载云端快照</strong></div>
      <div v-else-if="!snapshots.length" class="snapshot-empty"><Archive :size="24" /><strong>暂无云端快照</strong><span>创建一份手动快照，为重要修改保留恢复点</span></div>
      <div v-else class="snapshot-list">
        <article v-for="item in snapshots" :key="item.id" :class="['snapshot-card', item.status, { active: selected?.id === item.id }]">
          <button class="snapshot-main" type="button" :disabled="item.status !== 'ready'" @click="showPreview(item)">
            <div class="snapshot-card-title"><strong>{{ item.name }}</strong><span>{{ typeLabels[item.snapshotType] || item.snapshotType }} · {{ statusLabels[item.status] || item.status }}</span></div>
            <p v-if="item.note">{{ item.note }}</p>
            <div class="snapshot-meta"><span>{{ formatDate(item.createdAt) }}</span><span>{{ formatBytes(item.sizeBytes) }} · {{ totalEntities(item) }} 条 · {{ item.chunkCount }} 块</span></div>
            <div class="snapshot-state"><span :class="item.status"><TriangleAlert v-if="item.status === 'corrupt' || item.status === 'failed'" :size="12" />{{ item.protected ? '已保护 · 不自动清理' : (item.expiresAt ? formatDate(item.expiresAt) + ' 到期' : '长期保留') }}</span></div>
            <small v-if="item.failureReason">{{ item.failureReason }}</small>
          </button>
          <div class="snapshot-actions">
            <button :class="['snapshot-action', { active: selected?.id === item.id }]" type="button" :disabled="workingId === item.id || item.status !== 'ready'" @click="showPreview(item)"><Eye :size="13" />详情</button>
            <button class="snapshot-action danger" type="button" :disabled="workingId === item.id || restorePreparing || restoreCommitting || item.status !== 'ready'" @click="beginRestore(item)"><History :size="13" />恢复</button>
            <button :class="['snapshot-action', { active: item.protected }]" type="button" :disabled="workingId === item.id || item.status !== 'ready'" @click="toggleProtected(item)"><ShieldOff v-if="item.protected" :size="13" /><Shield v-else :size="13" />{{ item.protected ? '解除' : '保护' }}</button>
            <button class="snapshot-action danger" type="button" :disabled="workingId === item.id || item.protected" @click="removeSnapshot(item)"><Trash2 :size="14" />删除</button>
          </div>
        </article>
      </div>
    </template>

    <el-dialog v-model="previewOpen" :title="selected ? '快照详情 · ' + selected.name : '快照详情'" width="min(760px, calc(100vw - 32px))" :close-on-click-modal="!restorePreparing">
      <div v-if="selected" class="snapshot-detail">
        <div class="snapshot-detail-meta">
          <span>{{ typeLabels[selected.snapshotType] || selected.snapshotType }}</span>
          <span>{{ formatDate(selected.createdAt) }}</span>
          <span>{{ formatBytes(selected.sizeBytes) }}</span>
          <span>{{ totalEntities(selected) }} 条数据 · {{ selected.chunkCount }} 块</span>
          <span>{{ selected.protected ? '已保护，不自动清理' : (selected.expiresAt ? formatDate(selected.expiresAt) + ' 到期' : '长期保留') }}</span>
        </div>
        <div v-if="previewLoading" class="snapshot-empty"><LoaderCircle class="sync-spin" :size="18" />正在校验并计算差异...</div>
        <template v-else-if="preview?.restorable">
          <div class="snapshot-detail-status"><span>创建时已完成完整性校验</span><small>内容校验：{{ preview.contentHash }}</small></div>
          <div class="snapshot-diff-grid">
            <div v-for="(count, group) in preview.groupCounts" :key="group"><strong>{{ groupLabels[group] || group }}</strong><span>{{ count }} 条数据</span></div>
          </div>
          <details><summary>查看快照数据组成</summary><div class="entity-diffs"><div v-for="([entity, count]) in previewEntities" :key="entity"><span>{{ entityLabels[entity] || entity }}</span><small>{{ count }} 条</small></div></div></details>
          <p class="snapshot-detail-note">详情来自快照创建时保存的摘要，不会在打开详情时重新展开数据或计算与当前工作区的差异。恢复前会单独校验所有分块，提交时还会再次校验整体内容。</p>
          <div class="snapshot-detail-footer"><span>恢复会替换当前云端业务数据；是否保留恢复前备份，可在确认框中选择。</span><button class="danger-button snapshot-restore-button" type="button" :disabled="restorePreparing" @click="beginRestore()"><History :size="14" />恢复到此时点</button></div>
        </template>
        <div v-else-if="preview" class="snapshot-corrupt"><TriangleAlert :size="20" /><strong>快照不可恢复</strong><p>内容分块缺失或校验和不一致，系统已阻止继续使用此快照。</p></div>
      </div>
    </el-dialog>

    <el-dialog v-model="createOpen" title="创建手动快照" width="min(480px, calc(100vw - 32px))" :close-on-click-modal="!creating">
      <div class="snapshot-form"><label><span>快照名称</span><input v-model="name" maxlength="120" placeholder="例如：批量调整计划前" /></label><label><span>备注（可选）</span><textarea v-model="note" rows="3" placeholder="说明创建原因，便于后续选择" /></label><p>创建时将从同一个云端一致性边界读取全部受保护数据，并完成分块和 SHA-256 校验。单份快照上限为 100 MiB。</p></div>
      <template #footer><button class="secondary-button" type="button" :disabled="creating" @click="createOpen = false">取消</button><button class="primary-button" type="button" :disabled="creating || !name.trim()" @click="createSnapshot"><LoaderCircle v-if="creating" class="sync-spin" :size="14" />{{ creating ? '正在创建...' : '创建并校验' }}</button></template>
    </el-dialog>
    <el-dialog v-model="restoreOpen" title="确认恢复到此时点" width="min(560px, calc(100vw - 32px))" :close-on-click-modal="false" :show-close="!restoreCommitting" @closed="closeRestore">
      <div v-if="selected" class="snapshot-form restore-form">
        <div class="snapshot-corrupt restore-warning"><TriangleAlert :size="20" /><strong>这会替换同一个云端工作空间的业务数据</strong><p>设备授权、登录凭据、同步队列、操作记录和运行时租约不会从快照恢复。恢复完成后其他设备必须按新世代重建；有旧修改的设备会暂停同步。</p></div>
        <div class="restore-summary"><span>目标快照</span><strong>{{ selected.name }}</strong><span>恢复方式</span><strong>{{ createPreRestoreSnapshot ? '先创建恢复前备份，再执行恢复' : '直接恢复，不额外创建快照' }}</strong></div>
        <label class="restore-safety-option"><input v-model="createPreRestoreSnapshot" type="checkbox" :disabled="restoreCommitting || Boolean(restorePrepared)" /><span><strong>恢复前创建一份备份快照</strong><small>仅在确认恢复后创建；未勾选时不会额外生成快照。</small></span></label>
        <label><span>请输入快照名称以确认：{{ selected.name }}</span><input v-model="restoreNameConfirmation" :disabled="restoreCommitting" autocomplete="off" /></label>
      </div>
      <template #footer><button class="secondary-button" type="button" :disabled="restoreCommitting" @click="closeRestore">取消</button><button class="danger-button" type="button" :disabled="restoreCommitting || restoreNameConfirmation !== selected?.name" @click="commitRestore"><LoaderCircle v-if="restoreCommitting" class="sync-spin" :size="14" />{{ restoreCommitting ? (createPreRestoreSnapshot ? '正在备份并恢复...' : '正在恢复...') : '确认恢复' }}</button></template>
    </el-dialog>
  </div>
</template>

<style scoped>
.snapshot-settings { display: grid; gap: 9px; }
.snapshot-schedule { display: flex; align-items: center; justify-content: space-between; gap: 10px; padding: 8px 10px; border: 1px solid var(--line-soft); border-radius: 9px; }.snapshot-schedule-copy { display: grid; flex: 1 1 auto; min-width: 110px; gap: 1px; }.snapshot-schedule-actions { display: flex; flex: none; align-items: center; justify-content: flex-end; gap: 8px; margin-left: auto; }.snapshot-schedule strong,.snapshot-schedule span,.snapshot-schedule select,.snapshot-schedule button { white-space: nowrap; }.snapshot-schedule strong { font-size: 12px; }.snapshot-schedule span { overflow: hidden; color: var(--text-secondary); font-size: 10px; text-overflow: ellipsis; }.snapshot-schedule select { width: 104px; min-width: 104px; min-height: 30px; padding-right: 24px; font-size: 11px; }.snapshot-schedule button { display: inline-flex; flex: none; align-items: center; gap: 5px; }.snapshot-retention-field { display: inline-flex; flex: none; align-items: center; gap: 5px; }.snapshot-retention-field input { width: 58px; min-height: 30px; padding: 0 6px; border: 1px solid var(--line-soft); border-radius: 6px; background: var(--control-bg); color: var(--text-primary); font-size: 11px; }
.automatic-status { display: flex; min-width: 0; align-items: center; gap: 7px; margin-top: -7px; color: var(--text-secondary); font-size: 12px; }.automatic-status span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }.automatic-status.failed { color: #a84949; }
.snapshot-toolbar { display: flex; align-items: center; justify-content: space-between; gap: 10px; }.snapshot-toolbar-summary { display: flex; min-width: 0; gap: 10px; color: var(--text-secondary); font-size: 11px; }.snapshot-toolbar-actions { display: flex; flex: none; align-items: center; justify-content: flex-end; gap: 7px; margin-left: auto; }.snapshot-toolbar span,.snapshot-toolbar button { white-space: nowrap; }
.snapshot-toolbar button,.snapshot-actions button { display: inline-flex; align-items: center; gap: 5px; }
.snapshot-list { display: grid; grid-auto-rows: 60px; width: 100%; height: 324px; gap: 6px; align-content: start; overflow-y: auto; }
.snapshot-card { display: grid; grid-template-columns: minmax(0,1fr) auto; align-items: stretch; width: 100%; height: 60px; border: 1px solid var(--line-soft); border-radius: 9px; overflow: hidden; }.snapshot-card.active { border-color: var(--brand); box-shadow: 0 0 0 1px color-mix(in srgb,var(--brand) 35%,transparent); }
.snapshot-card.corrupt,.snapshot-card.failed { border-color: rgba(190,70,70,.35); }.snapshot-main { display: grid; gap: 2px; width: 100%; padding: 6px 9px; border: 0; text-align: left; background: transparent; color: inherit; cursor: pointer; }.snapshot-main:disabled { cursor: default; }
.snapshot-card-title,.snapshot-state { display: flex; min-width: 0; justify-content: space-between; gap: 8px; align-items: flex-start; }.snapshot-card-title strong,.snapshot-card-title span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }.snapshot-card-title strong { min-width: 0; font-size: 12px; font-weight: 600; }.snapshot-card-title span { flex: none; max-width: 45%; }.snapshot-card-title span,.snapshot-meta,.snapshot-state,.snapshot-main small { color: var(--text-secondary); font-size: 10px; }.snapshot-main small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.snapshot-main p { display: none; }.snapshot-meta { display: flex; min-width: 0; justify-content: space-between; gap: 8px; }.snapshot-meta span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }.snapshot-meta span:last-child { flex: none; }.snapshot-state span { display: inline-flex; min-width: 0; gap: 4px; align-items: center; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }.snapshot-state .corrupt,.snapshot-state .failed { color: #a84949; }
.snapshot-actions { display: flex; align-items: center; justify-content: flex-end; gap: 3px; padding: 7px 9px; border-left: 1px solid var(--line-soft); font-size: 11px; }.snapshot-action { padding: 4px 6px; border: 0; border-radius: 5px; background: transparent !important; box-shadow: none !important; color: var(--text-secondary); cursor: pointer; white-space: nowrap; }.snapshot-action:hover:not(:disabled),.snapshot-action.active { color: var(--brand); }.snapshot-action.danger,.snapshot-action.danger:hover:not(:disabled),.snapshot-action.danger.active { color: #b44b4b; }.snapshot-action:disabled { background: transparent !important; color: color-mix(in srgb,var(--text-secondary) 45%,transparent); cursor: not-allowed; opacity: 1; }.snapshot-action.danger:disabled { color: color-mix(in srgb,#b44b4b 42%,transparent); }
.snapshot-detail { display: grid; gap: 12px; font-size: 12px; }.snapshot-detail-meta { display: flex; flex-wrap: wrap; gap: 5px 14px; color: var(--text-secondary); font-size: 11px; }.snapshot-detail-meta span { white-space: nowrap; }.snapshot-detail-status { display: flex; align-items: center; justify-content: space-between; gap: 12px; }.snapshot-detail-status > span { color: #237451; font-weight: 600; white-space: nowrap; }.snapshot-detail-status small { overflow-wrap: anywhere; color: var(--text-secondary); font-size: 10px; }
.snapshot-diff-grid { display: grid; grid-template-columns: repeat(2,minmax(0,1fr)); gap: 6px; margin: 10px 0; }.snapshot-diff-grid > div { display: grid; grid-template-columns: 1fr auto; gap: 3px 8px; padding: 7px 8px; border-radius: 7px; background: var(--surface-soft); }.snapshot-diff-grid strong { grid-column: 1/-1; font-size: 11px; }.snapshot-diff-grid span { font-size: 10px; }.snapshot-diff-grid .add { color: #237451; }.snapshot-diff-grid .change { color: #8c651d; }.snapshot-diff-grid .remove { color: #a84949; }
.snapshot-detail details { margin-top: 2px; }.snapshot-detail summary { cursor: pointer; }.entity-diffs { display: grid; gap: 5px; margin-top: 8px; }.entity-diffs > div { display: flex; justify-content: space-between; gap: 10px; font-size: 12px; }.entity-diffs small { color: var(--text-secondary); }.snapshot-detail-footer { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding-top: 12px; border-top: 1px solid var(--line-soft); }.snapshot-detail-footer > span { color: var(--text-secondary); font-size: 11px; line-height: 1.5; }
.snapshot-detail-note { margin: 0; padding: 8px 9px; border-radius: 7px; background: var(--surface-soft); color: var(--text-secondary); font-size: 10px; line-height: 1.6; }
.snapshot-empty { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 5px; min-height: 150px; padding: 20px; text-align: center; color: var(--text-secondary); }.snapshot-settings > .snapshot-empty { box-sizing: border-box; height: 324px; min-height: 0; }.snapshot-empty strong { color: var(--text-primary); font-size: 12px; font-weight: 600; }.snapshot-empty span { font-size: 11px; }.snapshot-corrupt { display: grid; place-items: center; gap: 8px; min-height: 260px; text-align: center; color: #a84949; }.snapshot-corrupt p { max-width: 320px; }
.snapshot-form { display: grid; gap: 14px; }.snapshot-form label { display: grid; gap: 6px; }.snapshot-form input,.snapshot-form textarea { width: 100%; }.snapshot-form p { margin: 0; color: var(--text-secondary); font-size: 12px; line-height: 1.6; }
.snapshot-restore-button { display: inline-flex; flex: none; align-items: center; justify-content: center; gap: 6px; }.restore-warning { min-height: 0; padding: 14px; border-radius: 10px; background: color-mix(in srgb,#a84949 8%,transparent); }.restore-summary { display: grid; grid-template-columns: auto 1fr; gap: 8px 14px; padding: 12px; border-radius: 10px; background: var(--surface-soft); }.restore-summary span { color: var(--text-secondary); }.restore-summary code { overflow-wrap: anywhere; }
.restore-safety-option { display: grid !important; grid-template-columns: 16px minmax(0,1fr); align-items: start; gap: 9px !important; padding: 10px 11px; border: 1px solid var(--line-soft); border-radius: 8px; cursor: pointer; }.restore-safety-option input { width: 14px; height: 14px; margin: 2px 0 0; accent-color: var(--brand); }.restore-safety-option > span { display: grid; gap: 2px; }.restore-safety-option strong { color: var(--text-primary); font-size: 11px; }.restore-safety-option small { color: var(--text-secondary); font-size: 10px; line-height: 1.5; }
.snapshot-toggle { display: inline-flex; flex: none; align-items: center; gap: 4px; cursor: pointer; }.snapshot-toggle input { width: 13px; height: 13px; margin: 0; accent-color: var(--brand); }.snapshot-retention-field input[type=time] { width: 92px; }
@media (max-width: 640px) { .snapshot-schedule,.snapshot-toolbar { align-items: flex-start; flex-wrap: wrap; }.snapshot-schedule-copy,.snapshot-toolbar-summary { width: 100%; }.snapshot-schedule-actions,.snapshot-toolbar-actions { width: 100%; margin-left: 0; }.snapshot-list { grid-auto-rows: auto; }.snapshot-card { grid-template-columns: 1fr; height: auto; min-height: 94px; }.snapshot-actions { border-top: 1px solid var(--line-soft); border-left: 0; }.snapshot-diff-grid { grid-template-columns: 1fr; }.snapshot-detail-footer { align-items: stretch; flex-direction: column; }.snapshot-detail-footer .snapshot-restore-button { width: 100%; } }
</style>
