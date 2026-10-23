# Supabase 直连存储协议部署说明

本文说明 TimeGenie 的 `sync_v1` 与 `direct_v2` 工作空间存储协议。协议属于工作空间级数据边界：现有工作空间保持 `sync_v1`，不会因为部署 SQL 自动切换；只有用户完成旧同步队列处置后，才允许显式切换为 `direct_v2`。

## 使用边界

- `direct_v2` 云端模式必须联网，所有业务查询和 mutation 都直接访问 Supabase，不保存 SQLite 业务副本，也不创建 outbox。
- 网络不可达进入 `offline_readonly`；Supabase 5xx、请求超时或协议服务异常进入 `degraded_readonly`；明确会话失效或设备撤销进入 `reauth_required`。这些状态下所有后端 mutation 都会被统一拒绝。
- 当前进程已加载的数据可在只读状态继续查看，并标识可能不是最新；应用退出后不持久化页面业务缓存，离线重启不会显示 SQLite 历史云端数据。
- 网络或会话恢复后，客户端先根据 `change_seq` 与 workspace epoch 重新查询当前页面及活动计时，成功取得权威结果后才进入 `online_writable`。
- Realtime 只发送工作空间、变化序号、epoch 和领域失效信息。断线重连、休眠恢复和窗口重新激活都会进行补查，不要求断线期间每条通知都完整送达。
- 云端模式不存在“离线保存”“稍后同步”或本地版本覆盖云端。真实并发在提交时通过 expected version 即时拒绝并重新加载最新状态。

## 部署顺序

1. 在 Supabase 项目备份并确认当前 schema 版本。
2. 新项目直接执行 `supabase/schema.sql`；已有项目按顺序执行以下增量文件：
   - `supabase/20261022f_timegenie_direct_cloud_storage_protocol.sql`
   - `supabase/20261022g_timegenie_direct_cloud_subject_task_mutations.sql`
   - `supabase/20261022h_timegenie_direct_cloud_task_planning_mutations.sql`
   - `supabase/20261022i_timegenie_direct_cloud_recurrence_mutations.sql`
   - `supabase/20261022j_timegenie_direct_cloud_task_domain_routing.sql`
3. 执行后确认 `timegenie.workspaces.storage_protocol` 列为 `text not null default 'sync_v1'`，且约束只允许 `sync_v1` 和 `direct_v2`。
4. 在仓库根目录运行 `npm run test:supabase:schema`。该命令会启动临时 PostgreSQL，部署 schema、重复执行协议补丁并运行权限及协议屏障集成测试。

增量补丁可重复执行。函数包装器会先把旧实现重命名为 `*_pre_direct_v2`，再恢复原公共签名；重复执行时不会再次重命名。

## 客户端与角色权限

- 桌面端继续只使用 `anon`/publishable key 登录后的 `authenticated` 角色，不需要、也不应配置 `service_role`。
- `authenticated` 可调用 `storage_protocol_get`、`storage_protocol_upgrade_preflight`、`direct_cloud_context`、`direct_cloud_workbench_bootstrap`、`direct_cloud_operation_get` 和协议升级 RPC。
- `anon` 不得执行上述协议 RPC。`assert_sync_v1_workspace`、`assert_direct_v2_workspace`、`direct_cloud_operation_result` 和 `record_direct_cloud_operation` 只供服务端内部调用。
- 直连查询和业务 mutation RPC 必须通过带设备 ID 的公共鉴权前置逻辑，校验 Auth 用户、工作空间归属、设备授权、协议、workspace epoch 和客户端 schema generation。

## 工作台查询

`direct_cloud_workbench_bootstrap(workspace_id, device_id, expected_epoch)` 返回统一 `{ metadata, data }` 响应。`data` 包含工作空间时区、按服务器时间计算的工作日与起止毫秒、可见主体及开放事项数、事项计数摘要和当前设备授权上下文。查询直接读取 Supabase 业务表，不读取或写入 SQLite。主体按 `sort_order, created_at, id` 稳定排序；空工作空间返回 `subjects: []` 和全零摘要。

事项页面使用 `direct_cloud_task_list`、`direct_cloud_completed_tasks` 和 `direct_cloud_task_detail`。活动列表保持父子树顺序并投影路径、层级、可选择状态及直接/子树工时；日实例和软删除事项不进入普通事项树。已完成列表使用 `(completed_at, id)` 稳定游标，单页 1–100 条。筛选、详情和分页均通过同一设备与 workspace epoch 鉴权边界。

`direct_cloud_today_plan` 按显式工作日读取今日成员、父级上下文、按日估时、计划状态、重复规则、已存在轮次和已存在日实例。该查询只计算应显示的成员，不创建轮次、计划映射或日实例；跨日结果分别根据目标日期和重复规则计算。

## 事项与计划接口

### 调用约定

直连接口都是一个 Supabase RPC。查询使用具名参数；写入统一使用以下公共参数，并把业务字段放在 `p_payload`：

```json
{
  "p_workspace_id": "20000000-0000-0000-0000-00000000000a",
  "p_device_id": "30000000-0000-0000-0000-00000000000a",
  "p_operation_id": "4f000000-0000-0000-0000-000000000602",
  "p_expected_epoch": 1,
  "p_payload": {}
}
```

- `p_operation_id` 是稳定幂等键。响应丢失时，使用相同 operation ID、相同 `p_expected_epoch` 和完全相同的 `p_payload` 重试；同一 operation ID 对应不同请求时返回 `IDEMPOTENCY_CONFLICT`。
- 写入响应为 `{ operationId, metadata, data }`。`metadata.domains` 是需要重新查询的领域，`metadata.changeSeq`、`metadata.serverTime` 和 `metadata.workspaceEpoch` 用于判断是否刷新。
- 桌面端仍需发送 `Authorization`、`apikey`、`x-timegenie-workspace-epoch` 和 `x-timegenie-client-schema-generation`。业务请求不直接写表，也不使用 `service_role`。
- Tauri 命令在本地模式走 SQLite，在 `direct_v2` 模式走下表 RPC；同一命令不会同时写两边。

### 查询

| RPC | 对应 Tauri 命令 | 主要参数 | 结果与条件 |
| --- | --- | --- | --- |
| `direct_cloud_task_list` | `task_list` | `p_subject_id`、`p_include_completed`、`p_query`、`p_planned_date` | 返回活动事项树；`p_planned_date` 按 `planned_date` 精确过滤。日实例和软删除事项不进入结果。 |
| `direct_cloud_completed_tasks` | `task_list_completed_page` | `p_subject_id`、`p_cursor_completed_at`、`p_cursor_id`、`p_limit` | 按 `(completed_at, id)` 稳定倒序分页，单页 1–100 条。 |
| `direct_cloud_task_detail` | 无独立命令 | `p_task_id` | 返回事项、父级路径和直接子项，用于云端契约测试及内部详情读取。 |
| `direct_cloud_today_plan` | `task_list`（`todayOnly`）、`task_day_plan_snapshot` | `p_subject_id`、`p_work_date` | 返回今日成员、层级上下文、按日估时、计划状态和重复轮次。查询不生成轮次、实例或计划行。 |

`task_list` 带 `todayOnly` 或 `dailyEstimateDate` 时，Rust 适配器会组合 `direct_cloud_today_plan` 取得对应日期的权威状态；这仍是只读组合，不产生业务写入。

### 写入与版本条件

| RPC | 对应 Tauri 命令 | 必填与可选字段 | 版本、原子性与返回 |
| --- | --- | --- | --- |
| `direct_cloud_subject_create` | `subject_create` | `id`、`name` | 无实体版本；客户端生成 UUID，重复 operation ID 返回同一主体。返回 `subject`。 |
| `direct_cloud_subject_update` | `subject_rename` | `id`、`expectedVersion`、`name` | `expectedVersion` 必须等于 `subjects.version`。 |
| `direct_cloud_subject_reorder` | 无独立命令 | `id`、`expectedVersion`、`previousSubjectId`/`nextSubjectId` | 锚点必须属于同一工作空间且顺序有效。 |
| `direct_cloud_subject_archive` / `direct_cloud_subject_restore` | 无独立命令 | `id`、`expectedVersion` | 软归档或恢复；恢复前检查同工作空间所有权。 |
| `direct_cloud_task_create` | `task_create`、`task_create_positioned`、`task_create_quick` | `id`、`subjectId`、`title`；可选 `parentId`、`dayInstanceDate`、`plannedDate`、`estimateMinutes`、`note`、`projectName`、`solutionName`、`sourceType`、`previousSiblingId`、`nextSiblingId` | 创建无 `expectedVersion`。层级最多两级；父事项必须同主体、同实例日期、无直接工时且不是重复模板。返回 `task`。 |
| `direct_cloud_task_update` | `task_update` | `id`、`expectedVersion`；可按字段更新 `title`、`parentId`、`plannedDate`、`estimateMinutes`、`note`、`projectName`、`solutionName` | `expectedVersion` 必须等于 `tasks.version`。显式 `null` 清空可空字段，省略字段保持不变。 |
| `direct_cloud_task_move` | `task_move`、`task_reorder_subtree`、`task_change_parent` | `id`、`expectedVersion`、`parentId`；排序使用 `previousSiblingId`/`nextSiblingId` 或 `sortOrder` | 一次提交一个层级位置；锚点必须属于目标层级。版本或层级冲突整体失败。 |
| `direct_cloud_task_archive_subtree` | `task_delete_subtree` | `id`、`expectedVersion` | 在同一事务软删除根及全部未删除后代，返回 `affectedTaskIds`。陈旧版本返回 `VERSION_CONFLICT`。 |
| `direct_cloud_task_restore_subtree` | 无独立命令 | `id`、`expectedVersion` | 仅恢复同一次删除的子树；父事项或主体仍删除时返回 `ENTITY_DELETED`。 |
| `direct_cloud_task_duplicate_subtree` | `task_duplicate_subtree` | `id`、`expectedVersion` | 为新子树生成新 ID，根标题追加“ - 副本”，保持父子关系；重复 operation ID 不重复复制。 |
| `direct_cloud_task_set_status` | `task_set_completed` | `id`、`expectedVersion`、`done` | 只处理普通事项；状态实际变化时才增加 `tasks.version` 并追加状态事件。重复模板必须带轮次日期，否则返回 `VALIDATION_ERROR`。 |
| `direct_cloud_recurrence_occurrence_set_status` | `task_set_completed`（带 `occurrenceDate`） | `taskId`、`occurrenceDate`、`done`、`taskExpectedVersion`；可选 `occurrenceExpectedVersion`、`instanceExpectedVersion` | 模板、轮次和今日实例在同一事务更新。`occurrenceExpectedVersion` 为 `0` 表示首次创建轮次时不校验轮次版本。 |
| `direct_cloud_today_recurring_update` | `task_today_recurring_update` | `instanceId`、`instanceExpectedVersion`、`sourceExpectedVersion`；可选 `title`、`plannedDate`、`estimateMinutes`、`note`、`projectName` | 今日实例与来源模板必须同时匹配版本，任一过期则两者都不写入。返回 `instance` 和 `source`。 |
| `direct_cloud_recurrence_save` | `task_recurrence_save` | `taskId`、`taskExpectedVersion`、`frequency`、`effectiveStart`；`weekdaysMask` 仅周重复使用；`ruleExpectedVersion` 新建时为空 | 同时校验事项版本和当前规则版本。只有叶子、未完成且无完成祖先的事项可以启用重复。 |
| `direct_cloud_recurrence_close` | `task_recurrence_close` | `taskId`、`taskExpectedVersion`、`effectiveEnd`、`ruleExpectedVersion` | 同时校验事项与活动规则版本；保留历史规则，结束日期早于开始时删除当前规则。 |
| `direct_cloud_recurrence_day_generate` | 无独立命令 | `subjectId`、`workDate` | 对匹配的重复模板幂等生成轮次和今日计划；不依赖 `expectedVersion`，以 operation ID 和业务唯一键去重。 |
| `direct_cloud_task_daily_estimate_set` | `task_daily_estimate_set` | `taskId`、`workDate`、`estimateMinutes`；`expectedVersion` 为按日估时版本；可选 `expectedTaskVersion`、`updateRecurringEstimate` | `estimateMinutes=null` 删除该日记录。`expectedVersion` 用于 `task_daily_estimates.version`，不是事项版本。 |
| `direct_cloud_task_daily_estimate_batch_set` | `task_daily_estimate_batch_set` | `subjectId`、`workDate`、`items[]`：`taskId`、`expectedTaskVersion`、`expectedDailyEstimateVersion`、`estimateMinutes` | 所有事项必须属于同一主体；任一项版本失败则整批回滚。返回 `changedCount` 和按输入顺序排列的 `items`。 |
| `direct_cloud_task_day_plan_set` | `task_day_plan_set` | `taskId`、`workDate`、`state`（`included`/`excluded`）、`expectedVersion` | `expectedVersion` 是 `task_day_plans.version`；首次创建时为空。同状态重试不增加业务版本。 |
| `direct_cloud_task_day_plan_batch_set` | `task_day_plan_batch_set` | `subjectId`、`workDate`、`items[]`：`taskId`、`state`、`expectedVersion` | 批量计划整体事务提交；重复事项或跨主体事项直接拒绝。 |
| `direct_cloud_task_plan_batch_apply` | `task_plan_batch_apply` | `subjectId`、`baselineTasks[]`、`finalItems[]`、`confirmedDeleteTaskIds[]`；可选 `dayInstanceDate`、`todayPlanChange` | `baselineTasks` 必须精确覆盖打开时主体（或指定今日实例日期）的未完成事项及版本；`confirmedDeleteTaskIds` 必须精确匹配最终项中缺少的事项。新增、改名、父级、删除和今日计划变更在同一事务提交。 |

版本对象必须分开处理：

- 事项编辑、移动、删除和状态使用 `tasks.version`，载荷字段为 `expectedVersion`。
- 按日估时使用 `task_daily_estimates.version`，载荷字段为 `expectedVersion`；批量估时还要求每项的 `expectedTaskVersion`。
- 今日计划使用 `task_day_plans.version`，载荷字段为 `expectedVersion`。
- 重复规则使用 `task_recurrence_rules.version`，载荷字段为 `ruleExpectedVersion`；事项自身仍使用 `taskExpectedVersion`。
- 今日重复实例编辑同时校验实例和来源模板版本。
- 批量计划使用集合级 `baselineTasks` 加逐项 `expectedVersion`，不是逐条请求。

### 可执行示例

以下示例使用测试夹具的固定工作空间和设备；“文档示例”的实体 ID 与 operation ID 也用于 `supabase/schema_integration_test.sql`。

1. 创建文档示例主体：

```sql
select timegenie.direct_cloud_subject_create(
  '20000000-0000-0000-0000-00000000000a',
  '30000000-0000-0000-0000-00000000000a',
  '4f000000-0000-0000-0000-000000000601',
  1,
  '{"id":"53000000-0000-0000-0000-0000000000c1","name":"文档示例主体"}'::jsonb
);
```

2. 创建事项，`p_payload` 与客户端提交相同：

```json
{
  "id": "64000000-0000-0000-0000-0000000000d1",
  "subjectId": "53000000-0000-0000-0000-0000000000c1",
  "title": "文档示例事项",
  "plannedDate": "2026-11-02",
  "estimateMinutes": 30
}
```

3. 按计划日期查询同一主体：

```sql
select timegenie.direct_cloud_task_list(
  '20000000-0000-0000-0000-00000000000a',
  '30000000-0000-0000-0000-00000000000a',
  1,
  '53000000-0000-0000-0000-0000000000c1',
  false,
  null,
  '2026-11-02'
);
```

4. 使用创建返回的版本 `1` 更新标题，再用旧版本 `1` 提交一次：

```json
{
  "id": "64000000-0000-0000-0000-0000000000d1",
  "expectedVersion": 1,
  "title": "文档示例事项（已更新）"
}
```

```json
{
  "id": "64000000-0000-0000-0000-0000000000d1",
  "expectedVersion": 1,
  "title": "不应写入"
}
```

第二次请求必须返回 `VERSION_CONFLICT`，随后重新查询应看到“文档示例事项（已更新）”和版本 `2`。

5. 设置按日估时，首次提交 `expectedVersion` 为 `null`：

```json
{
  "taskId": "64000000-0000-0000-0000-0000000000d1",
  "workDate": "2026-11-02",
  "estimateMinutes": 45,
  "expectedVersion": null
}
```

6. 用 operation ID `4f000000-0000-0000-0000-000000000606` 的批量计划在同一事务新增子项并把父项加入今日计划：

```json
{
  "subjectId": "53000000-0000-0000-0000-0000000000c1",
  "baselineTasks": [
    {
      "taskId": "64000000-0000-0000-0000-0000000000d1",
      "expectedVersion": 2
    }
  ],
  "finalItems": [
    {
      "lineKey": "keep",
      "taskId": "64000000-0000-0000-0000-0000000000d1",
      "title": "文档示例事项（已更新）"
    },
    {
      "lineKey": "child",
      "title": "文档示例子事项",
      "parentLineKey": "keep"
    }
  ],
  "confirmedDeleteTaskIds": [],
  "todayPlanChange": {
    "workDate": "2026-11-02",
    "includeLineKeys": ["keep"],
    "excludeItems": []
  }
}
```

该请求预期返回 `createdCount=1`、`updatedCount=0`、`deletedCount=0`、`todayPlanChangedCount=1`，并返回新子项的 `taskId`。相同 operation ID 原样重试必须返回完全相同的响应。

仓库的 `supabase/schema_integration_test.sql` 会在临时 PostgreSQL 测试项目中执行上述创建、查询、更新、陈旧冲突、按日估时和批量计划请求，并在事务结束时回滚。运行 `npm run test:supabase:schema` 即可验证文档示例、RPC 参数和响应语义仍然一致。

### 事项与计划错误码

除下方通用错误外，事项与计划写入还会返回以下可稳定判断的代码：

| 代码 | 含义 |
| --- | --- |
| `VALIDATION_ERROR` | 必填字段缺失、字段格式无效、批量参数重复或行键顺序不合法。 |
| `NOT_FOUND` | 主体、事项、父事项或重复规则不存在。 |
| `ENTITY_DELETED` | 目标事项、父事项或所属主体已经删除。 |
| `TASK_DEPTH_LIMIT` | 事项层级超过两级，或已有子项的事项不能变成二级事项。 |
| `TASK_TREE_CYCLE` | 移动父级会形成循环。 |
| `TASK_HAS_DIRECT_TIME` | 已有直接工时的事项不能成为父事项。 |
| `TASK_RECURRING` | 重复事项新增子项前必须先关闭重复。 |
| `TASK_NOT_LEAF` | 只有叶子事项可以启用重复。 |
| `TASK_COMPLETED` | 已完成事项不能直接启用重复。 |
| `TASK_COMPLETED_ANCESTOR` | 存在已完成的上级事项，需先恢复上级。 |
| `TASK_NOT_RECURRING` | 轮次操作的目标事项没有有效重复规则。 |

计时页面使用 `direct_cloud_time_state` 和 `direct_cloud_time_history`。前者以同一服务器时间计算活动计时与未归属时长，并返回暂停分段、跨日链标识和实际分配；后者按工作日游标返回计时、休息与实际工时调整。历史游标只按日期前移，不依赖本机时间或 SQLite 分页状态。

报告页面使用 `direct_cloud_report_list`、`direct_cloud_report_detail`、`direct_cloud_report_templates` 和 `direct_cloud_report_preview`。列表使用 `(updated_at, id)` 稳定游标；详情保留历史模板名称、归档状态和事项快照；预览在云端读取当前事项、工时和已保存日报并替换允许的模板占位符，不保存报告。

共享配置使用 `direct_cloud_shared_configuration(workspace_id, device_id, expected_epoch)` 一次返回工作空间名称与时区、共享 `app_settings`、工作日历、所有可见主体的七天工作计划、安全集成配置和外部绑定元数据。设备设置、系统凭据值、`secret_ref`、管理密钥和绑定内容哈希不进入响应；集成项只声明 `requiresDeviceCredential`，本机再从系统凭据库组合实际的 `hasSecret` 状态。响应和云端快照共用递归敏感键脱敏，操作日志对集成配置继续采用字段白名单。

Rust 只读适配器分别对应工作台、事项列表/详情/已完成分页、今日计划、计时状态/历史、报告列表/详情/模板/预览和共享配置 RPC。适配器只从 SQLite 读取 `device_settings`、`devices`、会话凭据引用和 `local_sync_state.workspace_epoch`；不读取 `subjects`、`tasks`、`time_entries`、`reports`、`app_settings` 等业务表。事项投影可转换为现有 `TaskDto`，并以响应 `changeSeq` 作为列表修订号，使本地与云端固定夹具可执行同一业务断言。

所有直连查询响应的 `metadata.diagnostics` 记录 `responseBytes`、`rpcCount` 和 `durationMs`。页面级查询固定为一次 RPC，客户端测试验证一次适配器调用只发出一个 `/rpc/` 请求。当前代表性集成夹具的响应预算为：工作台 128 KiB、事项/今日计划/计时/共享配置各 256 KiB、20 条报告页 512 KiB；超过预算会使 schema 集成测试失败。`durationMs` 是服务端组装统一响应的诊断时间，端到端网络耗时仍由客户端观测指标补充。

## 协议切换

1. 客户端先调用 `storage_protocol_upgrade_preflight` 获取当前协议、epoch、schema generation、设备状态以及必需的队列处置选项。
2. 客户端检查本机旧队列。队列为空时使用 `queue_drained`；已完成救援备份并明确放弃旧修改时使用 `abandoned_with_rescue`。
3. 使用稳定 operation ID 调用 `upgrade_storage_protocol`。服务端在同一事务内校验设备与 epoch、按需创建安全快照、切换 `direct_v2`、写入 `workspace_reset` 变化记录和操作历史。
4. 切换后的工作空间拒绝旧补丁、增量拉取、租约、旧计时入口、旧未归属入口、旧实际工时批次、快照导入和旧恢复入口，并返回 `CLIENT_UPGRADE_REQUIRED`。

协议切换后不得降回 `sync_v1`。旧客户端只能升级后读取最新云端数据，或导出旧本地修改用于人工救援。

桌面端在“设置 → 数据存储”展示升级卡片。预检会列出当前旧队列、工作空间世代和其他有效设备，并明确提示切换对旧设备的影响。队列可收敛时选择“完成队列并升级”；队列存在失败或冲突且用户决定放弃时，必须先导出包含业务快照和脱敏队列诊断的救援备份，再输入完整文字“放弃旧修改”并完成第二次确认。任一步失败时保持 `sync_v1`，或保留可识别、可恢复的隔离队列。

## 错误与幂等

直连业务 mutation 使用稳定 operation ID。客户端丢失响应时，应使用同一 operation ID 和完全相同的请求重试，或调用 `direct_cloud_operation_get` 查询已处理结果；相同 operation ID 对应不同请求时返回 `IDEMPOTENCY_CONFLICT`。

客户端按稳定错误码处理，不解析英文提示：

- `VERSION_CONFLICT`：实体版本已变化，重新查询后再决定是否编辑。
- `ENTITY_DELETED`：目标实体已删除，不得覆盖。
- `LEASE_LOST`：当前设备已失去写入资格。
- `WORKSPACE_EPOCH_STALE`：工作空间世代已变化，必须刷新权威数据。
- `DEVICE_REVOKED`：当前设备授权失效，需要重新授权。
- `CLIENT_UPGRADE_REQUIRED`：客户端或存储协议版本不兼容。

## 发布检查

- 确认所有活跃工作空间仍为 `sync_v1`，除非已完成显式升级。
- 确认旧同步入口只授予 `authenticated`，内部实现函数不对客户端授权。
- 使用两个设备验证升级前队列受阻、空队列升级、升级后旧客户端写入被拒绝和幂等重试。
- 验证多设备升级后直接读取 Supabase，SQLite 业务表、`sync_outbox` 和 `local_sync_state` 不再增长。
