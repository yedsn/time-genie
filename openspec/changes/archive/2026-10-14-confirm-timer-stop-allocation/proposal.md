# Proposal

## Why

当前“结束计时”会立即按默认事项保存归属，用户在确认停止后没有机会先检查本段时间应归属到哪里，也无法在结束入口直接勾选“同时完成”。这会让右上角、计时页、托盘悬浮面板和系统托盘菜单的结束动作过于仓促，尤其是计时事项需要拆分、改归属或顺手完成事项时。

## What Changes

- 将“结束计时”和“确认归属”拆为两个用户可理解的步骤：先停止本段计时，再确认或调整本段归属。
- 在所有用户可见的结束入口提供一致行为，包括右上角快速计时、计时页、托盘悬浮面板、系统托盘菜单和原生兜底托盘菜单。
- 结束后默认生成一份待确认归属草稿，默认值沿用当前默认事项和本段全部分钟；用户不修改时可一键按默认值保存。
- 在结束后的快速确认路径支持“同时完成”勾选；进入计时页面调整归属时继续支持每条分配行的“同时完成”。
- 用户选择调整归属时，主窗口打开计时页面并定位、展开刚结束的那条时间记录。
- 云端模式与本地模式保持一致，不因 Supabase RPC 仍自动创建归属而绕过确认流程。

## Capabilities

### New Capabilities

- `timer-stop-allocation-confirmation`: 定义结束计时后确认归属、调整归属、同时完成事项以及各结束入口一致性的用户可见行为。

### Modified Capabilities

- 无。

## Impact

- 前端状态与界面：`src-ui/src/store.ts`、`GlobalTimerBar.vue`、`TimeTrackingWorkspace.vue`、`HoverPanel.vue`、`MainWorkbench.vue` 以及相关样式。
- 本地后端：`src-tauri/src/time_tracking.rs` 的 `timer_stop` 默认归属行为，以及 `src-tauri/src/lib.rs` / `native_tray.rs` 的托盘结束入口。
- 云端同步：`src-ui/src/services/tauri.ts`、`src-tauri/src/cloud_sync.rs`、`supabase/schema.sql` 中 `timer_stop` 是否自动创建默认归属的行为。
- 自动化 Hook：`timer.stopped` 仍应在计时成功停止后发布；`task.completed` 只在用户确认归属并勾选“同时完成”后发布。
- 测试：需要覆盖本地计时、云端计时、托盘入口、调整归属定位、同时完成事项和既有自动化事件语义。
