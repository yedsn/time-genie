import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const history = fs.readFileSync(path.join(repoRoot, "src-ui/src/components/CloudOperationHistorySettings.vue"), "utf8");
const snapshots = fs.readFileSync(path.join(repoRoot, "src-ui/src/components/CloudSnapshotsSettings.vue"), "utf8");
const backup = fs.readFileSync(path.join(repoRoot, "src-ui/src/components/DataBackupSettings.vue"), "utf8");
const workbench = fs.readFileSync(path.join(repoRoot, "src-ui/src/views/MainWorkbench.vue"), "utf8");
const service = fs.readFileSync(path.join(repoRoot, "src-ui/src/services/tauri.ts"), "utf8");
const styles = fs.readFileSync(path.join(repoRoot, "src-ui/src/styles.css"), "utf8");

for (const forbidden of ["搜索操作", "筛选设备", "筛选来源", "加载更多"]) {
  if (history.includes(forbidden)) throw new Error(`操作记录界面仍包含旧入口：${forbidden}`);
}
for (const expected of [
  "limit: 50", "pageCache", "上一页", "下一页", "第 {{ pageIndex + 1 }} 页",
  "暂无操作记录", "detailOpen", "<el-dialog", "{{ item.affectedEntityCount }} 项",
  "operationPresentation", "history-type-mark",
  "changedFields", "before?.[field]", "after?.[field]",
]) {
  if (!history.includes(expected)) throw new Error(`操作记录界面缺少：${expected}`);
}
if (history.includes("history-layout") || history.includes("history-toolbar")) {
  throw new Error("操作记录仍使用旧的分栏或独立刷新工具栏");
}
if (!styles.includes(".settings-snapshots-panel") || !styles.includes("grid-column: 1 / -1")) {
  throw new Error("云端快照面板必须独占设置页的一整行");
}
for (const expected of [
  "创建手动快照", "恢复到此时点", "sizeBytes", "chunkCount", "entityCounts",
  "创建中", "创建失败", "登录并切换到 Supabase 云端模式",
  "暂无云端快照", "white-space: nowrap",
  "snapshot-toolbar-actions", "snapshot-schedule-actions", "snapshot-action danger",
  "grid-template-columns: minmax(0,1fr) auto", "background: transparent !important",
  "height: 324px", "grid-auto-rows: 60px",
  "previewOpen", "snapshot-detail", "最长保留", "snapshotRetentionDays",
  "未保护快照最长保留", "不自动清理",
  'type="time"', "automaticSnapshotEnabled", "automaticSnapshotTime",
  "automaticSnapshotTimezone", "scheduleStatusText", "schedulerActive",
  '@click="beginRestore(item)"', "当前设备数据已重新下载",
  "创建时已完成完整性校验", "preview.groupCounts", "previewEntities",
  "createPreRestoreSnapshot", "恢复前创建一份备份快照",
  "直接恢复，不额外创建快照", "prepareCloudWorkspaceRestore(selected.value.id, createPreRestoreSnapshot.value)",
  'v-if="selected" class="snapshot-form restore-form"', ':disabled="restoreCommitting || Boolean(restorePrepared)"',
  'LoaderCircle v-if="restoreCommitting"',
  "afterRestore", "await props.afterRestore?.()",
]) {
  if (!snapshots.includes(expected)) throw new Error(`云端快照界面缺少：${expected}`);
}
for (const expected of [
  'function handleCloudSnapshotRestored()', ':after-restore="handleCloudSnapshotRestored"',
  "store.loadWorkspaceData()", "store.loadTimeData()", "store.loadReports()",
  "store.loadUnassignedState()", "store.loadTodayOverview()",
]) {
  if (!workbench.includes(expected)) throw new Error(`快照恢复后缺少工作台刷新：${expected}`);
}
for (const forbidden of ["内容勾选", "局部恢复选项", "回滚到此镜像", "云端历史镜像", "snapshot-preview"]) {
  if (snapshots.includes(forbidden)) throw new Error(`云端快照界面仍包含旧行为或术语：${forbidden}`);
}
for (const forbidden of ["preview.groups", "preview.entities", "新增 {{ diff.added }}", "实体级差异"]) {
  if (snapshots.includes(forbidden)) throw new Error(`快照详情仍在展示打开时计算的差异：${forbidden}`);
}
if (snapshots.includes('v-if="restorePreparing" class="snapshot-empty"')) {
  throw new Error("恢复确认弹框不应在准备阶段替换整个正文");
}
if (!service.includes("updateCloudWorkspaceSnapshotSettings") || !service.includes("automaticSnapshotTime")) {
  throw new Error("前端服务层缺少每日固定时间的自动快照设置接口");
}
for (const forbidden of [
  "getAutomaticSnapshotCheckStatus", "AutomaticSnapshotFrequency", "automaticSnapshotFrequency",
]) {
  if (service.includes(forbidden) || snapshots.includes(forbidden)) {
    throw new Error("自动快照仍保留同步后触发的频率接口：" + forbidden);
  }
}
for (const [name, source, removedClass] of [
  ["本地备份", backup, "backup-intro"],
  ["操作记录", history, "history-intro"],
  ["云端快照", snapshots, "snapshot-intro"],
]) {
  if (!source.includes('class="help-tooltip"')) throw new Error(name + "缺少按需说明入口");
  if (source.includes(removedClass)) throw new Error(name + "仍展示常驻介绍卡片：" + removedClass);
}
for (const expected of [
  "listDataBackupHistory", "openDataBackup", "备份记录", "暂无本地备份",
  "打开", "位置", "item.path", "item.createdAt", "item.sizeBytes",
  "恢复本地备份", "从备份文件恢复", "创建安全备份并恢复",
  "restoreOpen", '@click="previewHistory(item)"', "importDataBackup",
  "backup-action danger", "background: transparent !important",
  "height: 324px", "height: 58px",
]) {
  if (!backup.includes(expected)) throw new Error(`本地备份界面缺少：${expected}`);
}
for (const expected of ["history-scroll", "height: 324px", "height: 56px"]) {
  if (!history.includes(expected)) throw new Error(`操作记录界面缺少固定五条可视区域：${expected}`);
}

console.log("Cloud operation history and snapshot UI verification passed.");
