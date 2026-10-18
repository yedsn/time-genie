import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const history = fs.readFileSync(path.join(repoRoot, "src-ui/src/components/CloudOperationHistorySettings.vue"), "utf8");
const snapshots = fs.readFileSync(path.join(repoRoot, "src-ui/src/components/CloudSnapshotsSettings.vue"), "utf8");
const backup = fs.readFileSync(path.join(repoRoot, "src-ui/src/components/DataBackupSettings.vue"), "utf8");
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
  "previewOpen", "snapshot-detail", "最长保留", "snapshotRetentionDays",
  "未保护快照最长保留", "不自动清理",
  'type="time"', "automaticSnapshotEnabled", "automaticSnapshotTime",
  "automaticSnapshotTimezone", "scheduleStatusText", "schedulerActive",
]) {
  if (!snapshots.includes(expected)) throw new Error(`云端快照界面缺少：${expected}`);
}
for (const forbidden of ["内容勾选", "局部恢复选项", "回滚到此镜像", "云端历史镜像", "snapshot-preview"]) {
  if (snapshots.includes(forbidden)) throw new Error(`云端快照界面仍包含旧行为或术语：${forbidden}`);
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
]) {
  if (!backup.includes(expected)) throw new Error(`本地备份界面缺少：${expected}`);
}

console.log("Cloud operation history and snapshot UI verification passed.");
