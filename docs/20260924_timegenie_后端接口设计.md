# 时序后端接口设计

> 日期：2026-09-24  
> 对应 OpenSpec change：`create-work-report-assistant-desktop-app`  
> 接口形态：Tauri 2 Rust commands + Supabase Auth/PostgREST/RPC/Realtime + 应用事件，不设计自建本地 HTTP 服务

## 1. 设计结论

- 后端支持 `local` 本地模式和 `cloud` Supabase 云端模式。前端始终通过 `invoke(command, payload)` 调用 Rust 服务层，不直接在 Vue 组件中区分存储模式。
- 本地模式由 Rust 服务层执行 SQLite 事务；云端模式由 Rust 服务层先提交本机 SQLite 事务并写入同步队列，再通过 Supabase Auth、PostgREST、PostgreSQL RPC 和 Realtime 推送、拉取和解决冲突。
- 云端模式以 Supabase PostgreSQL 为最终权威数据源，SQLite 保存本机缓存、待同步队列和同步状态；有待同步或冲突操作时不得用云端快照覆盖本地编辑，不允许形成双主。
- 主窗口、托盘窗口和悬浮窗口使用同一套命令，不允许各自维护业务副本。
- Rust 服务层每次本地或云端写入确认后广播领域事件。其他设备通过 Supabase Realtime 收到变更序号，再做增量拉取。
- 前端每秒只更新显示时钟，不每秒写数据库。运行时长以服务端保存的开放分段开始时间计算。
- 所有外部写入先创建预览，再使用预览 ID 确认执行，防止预览内容与实际写入内容不一致。
- 客户端只使用 Supabase `anon` key 和用户会话，绝不保存 `service_role` key。
- 本文只定义接口，不实现 Rust command、Supabase schema、数据库或真实外部调用。

## 2. 通用协议

### 2.1 请求和响应

命令参数在 TypeScript 使用 `camelCase`，Rust DTO 通过 serde 映射。成功响应统一为：

```json
{
  "data": {},
  "revision": 42
}
```

`revision` 是当前工作空间的数据修订号。本地模式使用 SQLite 维护的修订号；云端模式使用已应用的最大 `workspace_changes.change_seq`。列表查询可额外返回 `nextCursor`，首版数据量较小时默认不分页。

### 2.2 错误

Rust command 返回结构化错误：

```json
{
  "code": "ALLOCATION_EXCEEDS_DURATION",
  "message": "分配时间超出原始时间 15 分钟",
  "details": {
    "entryId": "...",
    "durationMinutes": 60,
    "allocatedMinutes": 75,
    "exceededMinutes": 15
  }
}
```

核心错误码：

| 错误码 | 使用场景 |
| --- | --- |
| `VALIDATION_ERROR` | 空标题、非法日期、非法分钟等 |
| `NOT_FOUND` | 实体不存在或已删除 |
| `VERSION_CONFLICT` | `expectedVersion` 与数据库不一致 |
| `ACTIVE_TIMER_EXISTS` | 已有运行或暂停的计时器 |
| `TASK_NOT_SELECTABLE` | 选择了父事项作为计时/分配目标 |
| `TASK_HAS_DIRECT_TIME` | 有直接工时的事项试图新增子项或成为父项 |
| `TASK_TREE_CYCLE` | 缩进或移动导致循环 |
| `ALLOCATION_EXCEEDS_DURATION` | 分配超过原始分钟 |
| `ALLOCATION_NOT_COMPLETE` | 未归属时间没有全部分配 |
| `REPORT_ALREADY_EXISTS` | 同主体、类型和周期已有报告 |
| `REPORT_INPUT_CHANGED` | 执行生成/写入时输入版本已经变化 |
| `EXTERNAL_FILE_CHANGED` | Obsidian 文件在预览后被外部修改 |
| `INTEGRATION_NOT_CONFIGURED` | 缺少路径、地址或 Token |
| `AUTH_FAILED` | SeaTable 认证失败 |
| `NETWORK_ERROR` | SeaTable 网络错误 |
| `SCHEMA_MISMATCH` | SeaTable 表或字段不匹配 |
| `AUTH_REQUIRED` | 云端模式会话不存在或已失效 |
| `DEVICE_REVOKED` | 当前设备已被撤销 |
| `OFFLINE_RESTRICTED` | 离线状态不允许执行全局计时或租约操作 |
| `SYNC_CONFLICT` | 离线操作的基础版本已经过期 |
| `TRACKING_LEASE_HELD` | 后台采集租约由其他设备持有 |
| `REMOTE_TIMER_ACTIVE` | 其他设备已经有活动计时器 |

### 2.3 并发规则

- 更新、删除、排序、分配和保存报告必须携带 `expectedVersion`。
- 本地模式多表操作必须在同一 SQLite 事务内完成；云端模式使用 PostgreSQL RPC 在单个数据库事务中完成。
- 计时开始使用数据库唯一约束兜底，不能只在前端判断 `runningEntry`。
- 树排序接口一次提交完整同级顺序或明确的相邻定位，避免逐行更新产生中间错序。
- `VERSION_CONFLICT` 时前端保留当前编辑内容，重新加载后让用户再次确认。
- 普通离线写入进入 `sync_outbox`；开始/暂停/继续计时、未归属处理和后台采集租约必须在线确认。
- 每个写命令携带 `operationId`，Supabase 通过 `processed_operations` 幂等。

### 2.4 模式路由

Rust 对页面暴露统一 Repository/Service 接口：

```text
Vue -> Tauri Command -> Domain Service
                         ├── LocalRepository(SQLite)
                         └── CloudRepository(Supabase + SQLite cache/outbox)
```

- 查询优先读取本机缓存，以便界面快速启动；云端同步完成后广播刷新事件。
- 云端模式的普通业务写操作先在本机 SQLite 事务中完成并写入 `sync_outbox`；即时推送失败、断网或遇到冲突时，本地结果保留，界面通过 `syncState`、待同步数量和冲突数量提示用户。
- 只有当 outbox 清空且无冲突时，才允许用云端快照刷新本机缓存；待同步、失败或冲突状态不能伪装成已云端保存。
- 云端 RPC 必须根据 JWT 内的 `auth.uid()` 校验工作空间所有权，不接受客户端传入任意用户 ID。

## 3. 账号、工作空间和设备

### 3.1 接口总览

| Command | 说明 |
| --- | --- |
| `storage_mode_get` | 获取当前本地/云端模式和同步状态 |
| `storage_mode_set_local` | 初始化或切换到独立本地模式 |
| `cloud_configure` | 保存 Supabase Project URL 和 anon key |
| `cloud_sign_in_password` | 邮箱密码登录 |
| `cloud_sign_in_magic_link` | 发送 Magic Link，可作为后续登录方式 |
| `cloud_sign_out` | 退出当前云端账号 |
| `cloud_sign_out_all` | 退出当前账号的全部设备 |
| `cloud_session_get` | 获取脱敏会话和账号状态 |
| `cloud_workspace_bootstrap` | 创建或加载当前用户的工作空间 |
| `cloud_device_register` | 注册当前安装实例 |
| `cloud_device_list` | 查看已登录设备 |
| `cloud_device_revoke` | 撤销其他设备 |
| `cloud_sync_resume_after_reauth` | 重新登录后确认恢复此前暂停的待同步修改 |
| `storage_migration_preview` | 预览本地上传云端或云端导出本地 |
| `storage_migration_execute` | 确认执行一次性迁移 |

### 3.2 `cloud_configure`

请求：

```json
{
  "projectUrl": "https://example.supabase.co",
  "anonKey": "ey..."
}
```

规则：校验 HTTPS URL 和项目连通性；`anonKey` 可以保存在设备配置中，`service_role` 格式或管理密钥必须拒绝保存。

### 3.3 `cloud_sign_in_password`

请求：

```json
{
  "email": "user@example.com",
  "password": "********"
}
```

密码只用于本次 Supabase Auth 请求，不落盘、不写日志。access token 和 refresh token 写入 Windows Credential Manager。返回脱敏账号信息，不返回 refresh token。

`cloud_session_get` 返回结构化状态：`authenticated` 表示在线校验成功；`offline_saved` 表示凭据仍安全保存在本机，但因断网、限流或 Supabase 临时故障无法校验；`reauth_required` 表示 refresh token、账号、Auth 会话或设备授权已明确失效。只有 `reauth_required` 才展示密码登录。单次业务请求的 `401/403` 不直接删除 refresh token；可安全重放的请求先刷新再重试，不可安全重放的请求只刷新认证并提示用户重试。Schema、RLS、版本冲突和普通业务错误不改变登录状态。

### 3.4 `cloud_workspace_bootstrap`

登录后调用。没有活跃工作空间时创建一个，已有时直接返回：

```json
{
  "workspaceId": "0199...",
  "name": "我的工作台",
  "timezone": "Asia/Shanghai",
  "latestChangeSeq": 128
}
```

### 3.5 `cloud_device_register`

请求：

```json
{
  "deviceId": "0199...",
  "deviceName": "办公室电脑",
  "platform": "windows",
  "appVersion": "0.1.0"
}
```

`deviceId` 在首次安装时生成并保存到设备配置。重新登录同一安装实例更新原记录，不重复创建。

设备授权通过 `device_authorize` 显式建立，并绑定当前 Supabase Auth `session_id`；该标识只用于区分新旧 Auth 会话，不是 access/refresh token。`device_authorization_get` 在启动、会话刷新和同步上传前检查授权。`device_revoke` 撤销指定设备并释放其采集租约；`device_revoke_all` 撤销账号下全部设备并释放全部租约。已撤销设备只有在新的密码登录产生不同 Auth 会话后才能重新授权，旧会话、普通心跳或直接表更新都不得静默清除撤销状态。

普通退出按“撤销当前设备授权 → Supabase Auth local logout → 清除本机 token”执行；退出所有设备按“撤销全部设备授权 → Supabase Auth global logout → 清除当前设备 token”执行，并要求前端二次确认。退出或撤销不会删除 SQLite 业务数据和 `sync_outbox`；重新登录后必须通过 `cloud_sync_resume_after_reauth` 明确恢复上传。

后台撤销指定设备时，数据库 owner/迁移管理员可在受控环境调用 `timegenie.device_revoke(workspace_id, device_id)`；撤销账号全部 Auth 会话应使用 Supabase Auth 管理后台或受保护的 Admin API。`service_role` 只能存在于受控服务端，禁止配置到桌面客户端。

### 3.6 存储模式迁移

`storage_migration_preview` 返回各实体数量、冲突项和迁移方向：

- `local_to_cloud`：把当前本地工作空间一次性上传到新建或空的云端工作空间。
- `cloud_to_local_snapshot`：下载当前云端数据生成一份新的独立本地工作空间。

迁移执行期间锁定业务写入。执行完成后必须明确选择新的权威模式；不提供持续的 SQLite 和 Supabase 双向双主。

## 4. 页面初始化和事件

### 4.1 `app_bootstrap`

服务页面：所有页面首次加载、主窗口从托盘恢复。

请求：

```json
{ "workDate": "2026-09-24" }
```

返回：

```json
{
  "data": {
    "settings": {},
    "storage": {
      "mode": "cloud",
      "online": true,
      "syncState": "synced",
      "pendingOperations": 0,
      "workspaceId": "...",
      "deviceId": "..."
    },
    "subjects": [],
    "selectedSubjectId": "...",
    "activeTimer": null,
    "unassigned": {
      "sessionId": "...",
      "state": "collecting",
      "elapsedSeconds": 182,
      "mustResolve": false
    },
    "todaySummary": {
      "rawMinutes": 116,
      "allocatedMinutes": 104,
      "pendingMinutes": 12,
      "breakMinutes": 20
    }
  },
  "revision": 42
}
```

处理：

1. 执行本机 SQLite 迁移检查。
2. 云端模式恢复 Supabase 会话、校验设备状态并执行增量同步。
3. 恢复运行/暂停计时器。
4. 尝试获取或续租后台采集租约；没有租约时只展示其他设备采集状态。
5. 恢复或创建后台未归属会话。
6. 如果未归属累计超过阈值，返回 `mustResolve=true`，前端立即打开不可关闭弹框。

### 4.2 应用事件

Rust 向所有窗口广播：

| 事件 | 载荷 | 用途 |
| --- | --- | --- |
| `work-data-changed` | `{revision, domains[]}` | 主体、待办、时间、报告或设置变化 |
| `timer-state-changed` | 活动计时 DTO 或 `null` | 同步主窗口、托盘和悬浮窗 |
| `unassigned-state-changed` | 未归属状态 DTO | 显示右上角入口或强制弹框 |
| `sync-run-changed` | 同步运行摘要 | 更新外部写入进度和结果 |
| `external-file-conflict` | 路径、修改时间 | Obsidian 文件冲突提示 |
| `cloud-sync-state-changed` | 在线状态、待同步数量、冲突数量 | 显示同步状态 |
| `remote-device-changed` | 设备、计时器或租约摘要 | 多设备实时同步 |

同一设备多窗口由 Rust 事件同步；不同设备由 Supabase Realtime 订阅 `workspace_changes`。前端收到比本地更新的 `revision/changeSeq` 后按 `domains` 增量查询，正式版本不再使用 `BroadcastChannel` 传递业务快照。

### 4.3 云端增量同步

| Command | 说明 |
| --- | --- |
| `cloud_sync_status` | 获取在线状态、最后序号、待处理和冲突数量 |
| `cloud_sync_pull` | 从 `lastChangeSeq` 增量拉取变化并更新本机缓存 |
| `cloud_sync_push` | 顺序提交 outbox 操作 |
| `cloud_sync_retry` | 重试失败操作 |
| `cloud_sync_conflicts` | 列出需要人工处理的冲突 |
| `cloud_sync_resolve_conflict` | 选择云端版本、本地版本或合并内容 |

`cloud_sync_pull` 请求：

```json
{
  "workspaceId": "...",
  "afterChangeSeq": 120,
  "limit": 500
}
```

服务端返回变更元数据和对应实体当前版本。客户端必须按 `changeSeq` 顺序应用，并在同一 SQLite 事务中更新缓存和 `last_change_seq`。

冲突规则：

- 实体版本未变化时直接应用离线操作。
- 标题、日期、预计时长等同一实体字段被不同设备修改时标记 `SYNC_CONFLICT`，不做静默最后写入覆盖。
- 不同实体或同一实体互不重叠字段可以由服务端受控合并。
- 报告 Markdown 双端都修改时必须人工选择或使用三方文本合并。
- 删除与编辑冲突默认保留删除，允许用户在冲突界面复制本地内容新建事项。

## 5. 主体接口

### 5.1 接口总览

| Command | 页面操作 | 主要返回 |
| --- | --- | --- |
| `subject_list` | 展示侧栏主体 | 排序后的主体列表和事项数量 |
| `subject_create` | 添加主体 | 新主体 |
| `subject_rename` | 编辑主体名称 | 更新后的主体 |
| `subject_select_default` | 设置默认进入主体 | 设置值 |

### 5.2 `subject_create`

请求：

```json
{ "name": "研发支持" }
```

规则：去除首尾空格后不能为空；名称重复时返回现有主体和 `alreadyExists=true`，前端可以直接切换，不重复创建。

### 5.3 `subject_rename`

请求：

```json
{
  "subjectId": "...",
  "name": "运维工作",
  "expectedVersion": 2
}
```

重命名不改历史报告 Markdown；报告列表实时显示当前主体名，已生成正文保持原内容，除非用户重新生成。

## 6. 待办接口

### 6.1 `task_list`

服务页面：待办列表、今日工作台、计时选择器、报告范围选择器。

请求：

```json
{
  "subjectId": "...",
  "includeCompleted": true,
  "plannedDate": null,
  "todayDate": null,
  "dailyEstimateDate": "2026-09-29",
  "query": null
}
```

返回每项：

```json
{
  "id": "...",
  "subjectId": "...",
  "parentId": null,
  "title": "运维相关",
  "status": "open",
  "plannedDate": null,
  "estimateMinutes": null,
  "todayEstimateMinutes": 60,
  "sortOrder": 20,
  "depth": 0,
  "pathLabel": "运维相关",
  "selectable": false,
  "directMinutes": 0,
  "totalMinutes": 132,
  "executionState": "started",
  "version": 3
}
```

`executionState` 可取 `not_started`、`started`、`running`、`paused`、`done`，由查询层计算。

### 6.2 `task_create`

服务操作：点击列表末尾空白行、点击某个一级组末尾的子项占位行、计时选择器输入临时事项。

请求：

```json
{
  "subjectId": "...",
  "parentId": "...",
  "title": "临时处理 NAS 空间",
  "plannedDate": null,
  "estimateMinutes": null,
  "sourceType": "timer_quick_create",
  "insertAfterTaskId": null
}
```

规则：

- 空标题不创建记录。
- `parentId` 存在时继承主体，不接受前端传入不同主体。
- 上级已有直接工时分配时返回 `TASK_HAS_DIRECT_TIME`。
- 快速创建成功后可直接作为计时/分配目标。

### 6.3 `task_update`

服务操作：行内编辑标题、预计开始日期、整体预计分钟和备注。

请求只提交变更字段：

```json
{
  "taskId": "...",
  "patch": {
    "title": "开发江湖数据同步 v3 功能",
    "plannedDate": "2026-09-24",
    "estimateMinutes": 120
  },
  "expectedVersion": 4
}
```

### 6.4 `task_daily_estimate_set`

服务操作：在事项行内设置或清空指定日期的今日预计用时。

```json
{
  "taskId": "...",
  "workDate": "2026-09-29",
  "estimateMinutes": 60
}
```

`estimateMinutes` 为 `null` 时删除该日期的预计记录。该接口不得修改事项的整体预计用时。

### 6.5 `task_set_completed`

请求：

```json
{ "taskId": "...", "completed": true, "expectedVersion": 4 }
```

只修改当前事项，不级联。在同一事务中更新当前状态并追加状态历史事件。取消完成后事项回到上方未完成列表，并恢复原树位置。

### 6.6 `task_reorder_subtree`

服务操作：拖动整棵事项子树排序。

请求：

```json
{
  "taskId": "...",
  "parentId": "...",
  "beforeTaskId": null,
  "afterTaskId": "...",
  "expectedVersion": 4
}
```

规则：

- `parentId` 必须等于事项当前父 ID；不允许拖动改变层级。
- 服务端找出整棵子树后移动，并重新编号受影响同级的 `sort_order`。
- 返回新的扁平树顺序，前端拖动结束后用该结果重建占位行。

### 6.6 `task_change_parent`

服务操作：增加缩进、减少缩进、`Tab`、`Shift+Tab`。

请求：

```json
{
  "taskId": "...",
  "newParentId": "...",
  "insertAfterTaskId": null,
  "expectedVersion": 4
}
```

规则：同主体、无循环；变成父项的事项不能已有直接分配；返回变更后的路径、深度和同级顺序。

### 6.7 `task_duplicate_subtree`

请求：`{taskId, expectedVersion}`。服务端复制整棵子树并返回根副本 ID 与新建节点列表。

### 6.8 `task_delete_subtree`

请求：

```json
{ "taskId": "...", "expectedVersion": 4 }
```

服务端返回预检查信息时应包含后代数量和直接/间接工时引用。确认删除后软删除整棵子树；已有报告和时间分配仍可通过软删除记录展示历史名称。

## 7. 计时接口

### 7.1 接口总览

| Command | 页面操作 |
| --- | --- |
| `timer_get_state` | 主窗口、托盘和悬浮窗读取当前计时 |
| `timer_start` | 从右上角、计时页或事项行开始计时 |
| `timer_pause` | 暂停 |
| `timer_resume` | 继续 |
| `timer_stop` | 结束本段并进入归属确认 |

### 7.2 `timer_start`

请求：

```json
{
  "taskId": "...",
  "note": "",
  "clientRequestId": "uuid"
}
```

处理：

1. 校验任务是叶子事项。
2. 事务内确认没有活动计时器。
3. 暂停当前未归属时间分段。
4. 创建 `time_entry` 和首个开放 `time_segment`。
5. 使用 `clientRequestId` 幂等，避免双击创建两条记录。
6. 广播 `timer-state-changed`。

本地模式在 SQLite 事务中完成；云端模式调用 PostgreSQL RPC `timer_start(workspace_id, task_id, device_id, operation_id)`，由部分唯一索引保证整个工作空间只有一条活动计时器。若其他设备已经开始，返回 `REMOTE_TIMER_ACTIVE` 和当前计时摘要，前端直接展示该计时而不是新建。

返回包含服务端 `startedAt` 和 `startedByDevice`，各窗口用该时间计算显示秒数。任意在线设备都可以查看、暂停、继续或结束当前计时，所有动作通过 Realtime 同步。

### 7.3 `timer_pause`

请求：`{entryId, expectedVersion}`。

处理：关闭当前开放分段，汇总 `durationSeconds`，状态改为 `paused`。暂停期间不启动未归属时间，因为它仍属于当前计时过程的主动暂停。云端模式通过 RPC 原子检查 `expectedVersion`，并记录执行设备。

### 7.4 `timer_resume`

请求：`{entryId, expectedVersion, operationId}`。新增开放分段并改为 `running`。云端离线时禁止继续，避免其他设备已经结束或切换该计时。

### 7.5 `timer_stop`

请求：

```json
{
  "entryId": "...",
  "expectedVersion": 3,
  "createDefaultAllocation": false
}
```

处理：

- 运行中则关闭开放分段；暂停中直接结束。
- 计算结算分钟数。
- 用户结束入口默认不创建分配，保留默认事项作为归属确认草稿来源。
- 仅内部兼容路径显式传入 `createDefaultAllocation: true` 时才立即创建覆盖全部分钟的默认分配。
- 启动/恢复后台未归属会话。
- 返回结束后的时间记录，前端随后通过归属确认或 `time_allocation_replace` 保存分配。

云端断网时允许先记录“结束待提交”，锁定本机计时操作并继续显示等待同步；在结束请求成功前不得开始下一条计时。若云端记录已被其他设备结束，按服务端结果关闭本地计时并提示实际结束设备和时间。

## 8. 时间记录与工时分配接口

### 8.1 `time_entry_list`

请求：

```json
{ "workDate": "2026-09-24", "includeBreaks": true }
```

按结束/开始时间倒序返回范围、时长、备注、分配合计、状态和分配明细。

### 8.2 `time_entry_create_manual`

服务操作：手动补录。

请求：

```json
{
  "taskId": "...",
  "workDate": "2026-09-24",
  "startedAt": 0,
  "endedAt": 0,
  "minutes": 30,
  "note": "补录讨论时间",
  "clientRequestId": "uuid"
}
```

首版 UI 只输入分钟时，以当前时间为结束时间反推开始时间。请求必须二选一：提供正整数 `minutes`，或同时提供有效的 `startedAt/endedAt`；两种方式同时提供时拒绝保存。使用时间范围时，分钟由服务端计算。任务必须是叶子事项；有任务时默认全部分配，无任务时允许创建待结算记录。

### 8.3 `time_entry_update`

服务操作：修正开始、结束、分钟或备注。

请求：

```json
{
  "entryId": "...",
  "startedAt": 0,
  "endedAt": 0,
  "note": "修正结束时间",
  "expectedVersion": 2
}
```

只允许修改已结束记录。修改后重新计算分钟；现有分配超出时拒绝保存并返回超出数量。

### 8.4 `time_allocation_replace`

服务操作：时间记录展开后的“确认归属”。

请求：

```json
{
  "entryId": "...",
  "allocations": [
    { "taskId": "task-a", "minutes": 60, "note": null },
    { "taskId": "task-b", "minutes": 30, "note": null }
  ],
  "expectedVersion": 3
}
```

处理：

- 合并重复任务、移除 0 分钟项。
- 全部目标必须是未删除叶子事项。
- 总和不得超过可分配分钟，但普通时间记录允许保留未分配差额。
- 删除旧分配并写入新分配必须在同一事务中完成。
- 更新后清空对应工作日 `settled_at`，广播时间和报告输入已变化。

### 8.5 `workday_settle`

请求：`{workDate, expectedVersion}`。仅当待结算分钟为 0 且没有运行/暂停计时时允许确认。

## 9. 未归属时间接口

云端模式先通过后台采集租约确定负责设备：

| Command | 说明 |
| --- | --- |
| `tracking_lease_acquire` | 没有有效持有者时原子获取 45 秒租约 |
| `tracking_lease_renew` | 每 15 秒携带 token 续租 |
| `tracking_lease_release` | 退出应用或切换本地模式时释放 |
| `tracking_lease_get` | 查看当前持有设备和过期时间 |

未持有租约的设备不得累计未归属分段，但可以查看并处理已经达到阈值的未归属会话。

### 9.1 `unassigned_get_state`

返回：

```json
{
  "sessionId": "...",
  "state": "awaiting_resolution",
  "firstStartedAt": 0,
  "lastEndedAt": null,
  "elapsedSeconds": 487,
  "requiredMinutes": 9,
  "thresholdSeconds": 300,
  "mustResolve": true,
  "version": 5
}
```

本地模式由 Rust 后台任务负责阈值判断；云端模式由租约持有设备续写分段并通过 RPC 检查阈值。主窗口进入前台或收到状态事件时，`mustResolve=true` 必须打开不可关闭弹框。

### 9.2 `unassigned_resolve_work`

请求：

```json
{
  "sessionId": "...",
  "allocations": [
    { "taskId": "task-a", "minutes": 5 },
    { "taskId": "task-b", "minutes": 4 }
  ],
  "expectedVersion": 5
}
```

总分配必须等于服务端在事务封口后重新计算的 `requiredMinutes`。由于弹框中的时间持续增长，若提交时分钟数增加，返回 `ALLOCATION_NOT_COMPLETE` 和新的所需分钟，前端把差额加到最后一行并保持弹框打开。云端使用 RPC 一次性封口会话、创建时间记录和分配，任意设备重复提交同一 `operationId` 只执行一次。

### 9.3 `unassigned_resolve_break`

请求：`{sessionId, expectedVersion}`。生成休息时间记录，不参与原始工时和报告。

### 9.4 `unassigned_discard`

请求：`{sessionId, expectedVersion}`。只保存丢弃审计，不生成时间记录。

## 10. 报告接口

### 10.1 `report_list`

服务页面：统一报告列表。

请求：

```json
{
  "type": "all",
  "subjectId": null,
  "query": "2026-09",
  "limit": 100
}
```

返回日期/日期范围、类型、主体、选中事项数、更新时间、生成次数和是否有未保存外部变化。日报、周报、月报颜色由前端按 `reportType` 映射。

### 10.2 `report_scope_suggest`

服务操作：打开创建报告弹框或切换类型、日期、主体。

请求：

```json
{
  "reportType": "daily",
  "referenceDate": "2026-09-24",
  "subjectId": "..."
}
```

返回：

- 标准化 `periodStart/periodEnd`。
- 按计划排序的事项候选。
- 每项层级、是否完成、范围内实际分钟和默认选中状态。
- 默认只选已完成或范围内有实际用时的事项；如果没有此类事项，可返回全部事项但标记 `fallback=true`，由 UI 明确显示。

### 10.3 `report_create`

请求：

```json
{
  "reportType": "daily",
  "referenceDate": "2026-09-24",
  "subjectId": "...",
  "taskIds": ["..."],
  "clientRequestId": "uuid"
}
```

处理：标准化范围、检查唯一性、保存报告范围、选取模板并生成 Markdown。若已存在，返回 `REPORT_ALREADY_EXISTS` 和已有报告 ID，前端切换到原报告。

### 10.4 `report_get`

返回完整 Markdown、范围、任务选择、输入摘要、外部输出路径和 `version`。

### 10.5 `report_update_scope`

请求：

```json
{
  "reportId": "...",
  "reportType": "weekly",
  "referenceDate": "2026-09-24",
  "subjectId": "...",
  "taskIds": ["..."],
  "expectedVersion": 2
}
```

只更新范围，不自动覆盖 Markdown。返回 `needsRegeneration=true`，用户点击重新生成后才替换正文。

### 10.6 `report_save_content`

请求：

```json
{
  "reportId": "...",
  "markdownContent": "...",
  "expectedVersion": 3
}
```

保存后标记 `contentSource='edited'`。支持 `Ctrl+S`，不自动改动任务或工时。

### 10.7 `report_regenerate`

请求：

```json
{
  "reportId": "...",
  "expectedVersion": 3,
  "confirmedOverwrite": true
}
```

处理：

1. 重新查询范围内事项和最终分配。
2. 叶子事项在报告截止时间已完成则显示绿色；有时间但当时未完成则显示红色；未开始事项不进入今日事项。历史周期截止时间为周期末，当前或未来周期截止时间不晚于本次生成时间。
3. 有选中后代的父事项只输出结构标题，不显示状态图标或直接工时。
4. 周报/月报生成统计信息和逐日情况，缺失日期明确列出。
5. 覆盖 Markdown、生成次数加 1，并保存新输入摘要。

### 10.8 `report_delete`

请求：`{reportId, expectedVersion}`。确认后软删除，不删除事项、时间记录或已写出的 Obsidian 文件。

## 11. 报告模板接口

| Command | 说明 |
| --- | --- |
| `report_template_get` | 按报告类型和主体获取最终生效模板 |
| `report_template_validate` | 返回未知占位符、缺失推荐占位符和实时预览 |
| `report_template_save` | 保存全局或主体模板 |
| `report_template_reset` | 删除主体覆盖或恢复内置默认内容 |

`report_template_validate` 请求示例：

```json
{
  "reportType": "daily",
  "subjectId": null,
  "content": "【{{姓名}}】{{日期}} {{今日事项}}"
}
```

预览使用当前选定日期的只读数据，不产生报告记录。

## 12. Obsidian 计划导入接口

### 12.1 `obsidian_plan_import_preview`

请求：

```json
{
  "sourceDate": "2026-09-23",
  "targetDate": "2026-09-24",
  "subjectId": "..."
}
```

处理：读取配置路径下的日报，解析“明日计划/下周一计划”、编号、缩进和预计时长，创建 `preview` 导入批次。返回识别树、未识别行、源路径和文件摘要，不创建事项。

### 12.2 `obsidian_plan_import_confirm`

请求：

```json
{
  "batchId": "...",
  "items": [
    { "importItemId": "...", "selected": true, "title": "...", "estimateMinutes": 30 }
  ],
  "expectedVersion": 1
}
```

在一个事务中创建事项并标记批次已确认。重复提交相同批次返回已创建事项，不重复插入。

## 13. 外部写入和 SeaTable 接口

当前没有独立同步页面，以下命令由报告操作、设置连接检测或未来入口调用。

### 13.1 Obsidian

| Command | 说明 |
| --- | --- |
| `obsidian_report_write_preview` | 返回目标路径、创建/覆盖动作、内容和差异摘要 |
| `obsidian_report_write_execute` | 使用预览运行 ID 确认写入 |

执行前重新检查云端报告版本、当前设备的 `local_report_outputs` 目标文件修改时间和内容摘要。文件已变化时返回 `EXTERNAL_FILE_CHANGED`，不得静默覆盖。报告正文同步到云端，文件路径和写入状态只写本机 SQLite。

### 13.2 SeaTable

| Command | 说明 |
| --- | --- |
| `seatable_connection_test` | 检查地址、Token、Base 和表结构 |
| `seatable_task_sync_preview` | 生成新增、更新、跳过和冲突清单 |
| `seatable_task_sync_execute` | 执行已确认预览 |
| `seatable_sync_retry_failed` | 只重试上次失败项 |
| `seatable_reimbursements_query` | 按主体和日期范围读取待报销数据 |

同步预览请求：

```json
{
  "subjectId": "...",
  "workDate": "2026-09-24",
  "taskIds": ["..."]
}
```

稳定匹配顺序：

1. `external_bindings` 中已有 SeaTable row ID。
2. SeaTable 中保存的本地事项 ID。
3. 配置的复合业务键，需要用户确认后才能绑定。
4. 不允许只按事项标题静默匹配并覆盖。

部分失败不回滚本地任务、计时和分配，也不回滚已经成功的外部记录；结果逐项保存，允许只重试失败项。

## 14. 设置和凭据接口

| Command | 说明 |
| --- | --- |
| `settings_get` | 获取非敏感设置和集成配置状态 |
| `settings_update` | 更新基础、托盘和阈值设置 |
| `integration_config_update` | 保存 Obsidian 路径或 SeaTable 非敏感配置 |
| `integration_secret_set` | 将 Token 写入系统凭据库 |
| `integration_secret_clear` | 删除系统凭据 |

`settings_get` 同时返回 `storage.mode`、登录账号、工作空间、当前设备和同步状态。共享设置进入 Supabase；托盘行为、Obsidian 本地路径、设备名称和文件输出状态只进入本机 SQLite。只返回 `hasSecret: true/false`，不得返回完整 Token。日志中请求 DTO 必须对 `token`、`authorization`、`password` 和 `secret` 字段脱敏。

## 15. Rust 服务结构建议

```text
src-tauri/src/
  commands/
    app.rs
    auth.rs
    cloud_sync.rs
    storage_mode.rs
    subject.rs
    task.rs
    timer.rs
    time_entry.rs
    unassigned.rs
    report.rs
    settings.rs
    integration.rs
  services/
    auth_service.rs
    cloud_sync_service.rs
    storage_migration_service.rs
    tracking_lease_service.rs
    task_service.rs
    timer_service.rs
    allocation_service.rs
    unassigned_service.rs
    report_service.rs
    markdown_import_service.rs
    obsidian_service.rs
    seatable_service.rs
    credential_service.rs
  repositories/
    mod.rs
    local/
      subject_repository.rs
      task_repository.rs
      time_repository.rs
      report_repository.rs
      sync_repository.rs
    cloud/
      subject_repository.rs
      task_repository.rs
      time_repository.rs
      report_repository.rs
      sync_repository.rs
      rpc_client.rs
  supabase/
    auth_client.rs
    postgrest_client.rs
    realtime_client.rs
    session_store.rs
  db/
    connection.rs
    migrations.rs
  events.rs
  error.rs
  dto.rs
```

边界：

- `commands` 只做 DTO 校验、调用服务和返回结果。
- `services` 承担事务、状态迁移、树规则、报告生成和外部同步编排。
- `repositories` 通过统一 trait 暴露业务读写；`local` 使用 SQLite，`cloud` 使用 Supabase 并更新本地缓存，不包含 UI 文案。
- 树移动、全局计时、工时分配、未归属处理和租约等跨表操作必须调用 Supabase RPC，不在客户端组合多次 PostgREST 写入。
- `cloud_sync_service` 负责 Realtime 订阅、变更序号补拉、outbox 推送、幂等重试和冲突记录，业务 Service 不直接处理同步循环。
- 托盘菜单调用服务层，不得绕过事务直接修改内存状态。

## 16. 测试重点

### 16.1 数据与树

- 拖动父项时整棵子树移动且层级不变。
- `Tab/Shift+Tab` 正确更新父链并拒绝循环。
- 新建占位行失焦且标题为空时不产生数据。
- 已有直接工时的事项不能新增子项或变成父项。
- 完成子项后完成列表平铺显示完整路径，父项不被自动完成。
- 事项完成后再取消完成，历史报告仍能按周期结束时间还原当时状态。

### 16.2 计时

- 主窗口和托盘同时点击开始时只创建一条活动记录。
- 两台设备同时点击开始时只有一个请求成功，另一台收到 `REMOTE_TIMER_ACTIVE`。
- 暂停期间不计入时长，继续后新增分段。
- 应用窗口隐藏后计时仍由时间戳正确计算。
- 重启后恢复运行/暂停状态。
- 后台未归属时间同一时间只有租约持有设备累计，租约过期后可以被其他设备接管。
- 修正时间后分配超出时保存失败且编辑内容可恢复。

### 16.3 未归属时间

- 累计等于 5 分钟时不弹，大于 5 分钟后进入必须处理状态。
- 两台设备同时处理未归属会话时只有一个 RPC 成功，另一台收到版本冲突并刷新。
- 弹框打开期间时长继续增加。
- 默认存在一行分配，可增加和删除其他行，但不能删除最后一行。
- 提交瞬间分钟增加时返回新差额，不丢失用户输入。
- 分配、休息、丢弃三种操作均只能执行一次。

### 16.4 报告

- 同主体和周期只能有一份当前报告。
- 报告名称始终由日期或范围生成。
- 未开始事项不进入日报今日事项；有用时未完成项显示红色；完成项显示绿色。
- 父事项只作结构标题，工时由后代汇总。
- 人工编辑后保存，重新生成必须二次确认并覆盖。
- 周报/月报能列出每日情况和缺失日期。

### 16.5 外部集成

- Obsidian 文件在预览后变化时阻止覆盖。
- SeaTable 重复同步使用稳定绑定，不产生重复 TODO。
- 两台设备同时编辑同一事项时，旧版本写入被拒绝并进入冲突队列。
- Realtime 通知丢失后，按 `lastChangeSeq` 增量拉取仍能恢复完整状态。
- refresh token 和 SeaTable Token 不进入 Supabase 表、普通导出或日志。
- 部分失败能只重试失败项。
- 数据导出和日志不包含完整 Token。

## 17. 实现顺序建议

以下仅为后续 `openspec-apply-change` 的依赖顺序，本轮不修改 `tasks.md`：

1. 本地 SQLite 迁移、Repository 基础和本地全局修订号。
2. Supabase 项目、Auth、PostgreSQL schema、RLS、RPC、trigger 和 Realtime publication。
3. 工作空间/设备注册、模式设置和本地凭据存储。
4. 主体、待办树、排序、缩进和完成状态的双模式 Repository。
5. 计时记录、暂停分段、工时分配和多窗口事件。
6. Supabase 全局计时 RPC、后台采集租约和未归属时间处理。
7. 本地缓存、outbox、增量拉取、幂等推送和冲突界面。
8. 报告库、模板、生成和编辑保存。
9. Obsidian 导入与设备本地写入预览。
10. SeaTable 配置、预览、执行和失败重试。
11. 从 mock store 切换到 commands，删除业务 `localStorage` 持久化。
12. 双机并发、断网恢复、数据库备份和升级测试。

## 18. 正式实现前必须确认

1. OpenSpec 与当前 UI 的差异已经同步并通过评审。
2. 本地模式和 Supabase 云端模式的首版是否同时交付，还是先本地后云端。
3. Supabase 项目归属、Auth 登录方式、是否启用邮箱确认和密码重置。
4. 是否只支持同一账号多设备，还是首版就需要多用户工作空间和成员权限。
5. 时间不足一分钟和有零散秒数时采用向上取整。
6. 父事项不可直接计时/分配，以及已有直接工时不能转为父事项。
7. SeaTable 的实际 Base、表名、字段映射和本地 ID 保存字段。
8. Obsidian 的真实目录、文件命名和至少三份历史日报样例。
9. 周报/月报工资与报销内容是否进入首个正式版本；当前 UI 报告库尚未展示这两个字段。

本阶段禁止实现命令、创建数据库、执行迁移、写 Obsidian 或调用 SeaTable。
