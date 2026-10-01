# 时序数据结构设计

> 日期：2026-09-24  
> 对应 OpenSpec change：`create-work-report-assistant-desktop-app`  
> 阶段：UI 确认后的正式实现设计，不执行建库和数据写入

## 1. 设计结论

- 正式版本支持两种数据模式：`local` 本地模式和 `cloud` Supabase 云端模式。用户可以只使用本地模式，也可以登录 Supabase 在多台机器间共享同一份数据。
- 本地模式以 SQLite 为唯一主数据源；云端模式以 Supabase PostgreSQL 为权威主数据源，SQLite 只作为本机缓存、离线读取和待同步操作队列。
- 同一个数据空间在任一时刻只能选择一种权威模式，不允许 SQLite 与 Supabase 同时各自接受无约束写入。
- Vue 当前使用的 `localStorage`、页面内 mock 数组和 `BroadcastChannel` 仅用于 UI 阶段，正式实现后不得继续作为业务事实来源。
- “主体”是普通可编辑实体。名称为“默认”的主体与其他主体数据结构相同，默认选择由设置项引用，不在 UI 中显示特殊标识。
- 待办事项只持久化 `open`、`done` 两种完成状态。进行中、已暂停、已开始和未开始由计时记录实时推导，不再作为待办状态保存。
- 事项支持任意层级、同级排序、整棵子树拖动。拖动只改变顺序，不改变层级；层级只能通过增加缩进、减少缩进或对应快捷键改变。
- 父事项不能直接选择为计时或工时分配目标。父事项实际用时等于全部后代叶子事项的分配用时总和。
- 原始计时记录与最终工时分配分开保存。一条工作时间可以拆分给多个具体事项。
- 后台未归属时间使用可恢复的持久化会话记录。累计超过 5 分钟后进入必须处理状态，允许拆分给多个事项、记为休息或丢弃。
- 日报、周报、月报统一保存到报告库。报告名称不单独保存，始终由日期或日期范围派生。
- 云端模式使用 Supabase Auth、PostgreSQL、Row Level Security 和 Realtime。桌面应用不得包含 Supabase `service_role` key。
- Obsidian 和 SeaTable 是外部导入/输出目标，不是主数据源。所有外部写入先生成预览记录，再确认执行并保留逐项结果。

## 2. 当前 UI 与早期 OpenSpec 的差异

正式实现以当前已确认 UI 为准，以下差异需要在进入开发前同步回 OpenSpec：

| 领域 | 早期规格 | 当前 UI 事实 | 数据设计处理 |
| --- | --- | --- | --- |
| 导航 | 计划、结算、日报、周报、同步分开 | 待办、计时、报告、设置；同步页已删除 | 不为页面建表；同步能力保留为报告/设置中的动作 |
| 事项状态 | 计划中、进行中、已完成、暂不处理 | 未完成、已完成 | `tasks.status` 仅保存 `open/done` |
| 主体 | 默认主体配置 | 动态主体列表，可新增和重命名 | 新增 `subjects`，默认主体使用设置引用 |
| 计时 | 手动开启的时间片 | 手动计时加后台未归属时间 | 新增 `unassigned_sessions` 和分段记录 |
| 报告 | 日报、周报页面 | 日报/周报/月报统一列表 | `reports.type` 支持三种类型，日期范围唯一 |
| 报告编辑 | 生成预览 | 可保存 Markdown、调整范围、重复生成、删除 | 保存当前 Markdown、范围、生成次数和输入摘要 |
| 同步 | 独立同步模块 | 页面已删除 | 只保留底层预览、执行、重试数据模型 |
| 存储 | 第一阶段只考虑本地 SQLite | 可选 Supabase，多台机器同时操作 | 增加工作空间、设备、RLS、Realtime 和离线同步模型 |

## 3. 实体关系

```text
Supabase Auth users 1 ── n workspaces 1 ── n devices
                                  │
                                  ├── 1 tracking_leases
                                  ├── n workspace_changes
                                  └── n subjects 1 ── n tasks 1 ── n tasks(parent_id)
                     │
                     ├── n task_status_events
                     │
                     ├── n time_allocations n ── 1 time_entries 1 ── n time_segments
                     │
                     ├── n report_tasks n ── 1 reports
                     │
                     └── n external_bindings

work_days 1 ── n time_entries
work_days 1 ── n unassigned_sessions 1 ── n unassigned_segments
unassigned_sessions 0..1 ── 1 time_entries(origin)

subjects 1 ── n reports
report_templates 1 ── n reports(generator reference, logical)

plan_import_batches 1 ── n plan_import_items 0..1 ── 1 tasks
sync_runs 1 ── n sync_items

本地 SQLite（仅云端模式缓存）：
local_sync_state 1 ── n sync_outbox
```

## 4. 通用约定

### 4.1 标识和时间

- 业务主键统一使用应用生成的 UUID v7。SQLite 类型为 `TEXT`，Supabase PostgreSQL 类型为 `uuid`。
- 日期使用本地日期字符串 `YYYY-MM-DD`。
- 时间点的接口 DTO 使用 UTC Unix 毫秒；SQLite 使用 `INTEGER`，Supabase 使用 `timestamptz`，展示时按工作空间时区转换。
- 时间段不得跨工作日期。跨午夜的记录由服务层拆成两条时间记录。
- 时长事实以秒保存，报告与分配以整数分钟展示和结算。
- 时间片有有效秒数时，结算分钟数为 `max(1, ceil(duration_seconds / 60))`；零秒记录为 0 分钟，休息和丢弃时间不参与工时。

### 4.2 审计和并发

除纯明细表外，可同步业务表统一包含：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `created_at` | INTEGER | 创建时间，UTC 毫秒 |
| `updated_at` | INTEGER | 最后更新时间，UTC 毫秒 |
| `version` | INTEGER | 乐观锁版本，初始为 1，每次更新加 1 |
| `deleted_at` | INTEGER NULL | 需要保留历史引用的实体使用软删除 |
| `workspace_id` | UUID/TEXT | 所属数据空间，所有查询和唯一约束均以此隔离 |
| `created_by_device_id` | UUID/TEXT NULL | 创建数据的设备 |
| `updated_by_device_id` | UUID/TEXT NULL | 最近修改数据的设备 |

本地模式不要求登录，初始化一个本地工作空间和本地设备。云端模式使用 Supabase Auth 用户拥有的工作空间；首版只支持同一账号的多设备，不实现多人共同编辑和成员权限。主窗口、托盘窗口、悬浮窗口及不同设备均可能并发操作，因此写操作必须检查 `version`。

SQLite 中 `created_at/updated_at/deleted_at` 使用 UTC 毫秒；Supabase 中对应字段使用 `timestamptz`。本文后续字段表不再重复列出 `workspace_id`、设备和通用审计字段。

### 4.3 双模式与离线规则

- `storage_mode='local'`：全部业务读写直接进入 SQLite，不需要网络或账号。
- `storage_mode='cloud'`：Supabase 是权威数据源。在线写操作优先提交 Supabase；断网时写入本机 `sync_outbox`，界面明确标识“等待同步”。
- 云端模式的 SQLite 缓存可以被删除并从 Supabase 重新构建，不得保存云端不存在的隐藏业务事实。
- 所有离线操作携带 `operation_id`、`device_id`、`base_version` 和完整业务参数。Supabase 以 `operation_id` 幂等，避免重试造成重复数据。
- Realtime 只作为变更通知，不作为唯一数据传输渠道。客户端收到通知后按 `workspace_changes.change_seq` 增量拉取。
- 云端删除使用软删除并写入变更日志，离线设备才能收到删除墓碑。
- 本地切换云端必须走显式迁移预览；云端切换本地会创建一份独立快照，之后不再自动同步，避免形成双主。

### 4.4 派生字段

以下字段不得作为独立业务事实重复保存：

- 事项实际用时：从 `time_allocations.minutes` 汇总。
- 父事项实际用时：递归汇总全部后代叶子事项。
- 已开始/进行中/已暂停：从时间记录和当前活动计时器推导。
- 原始工时：工作类型时间记录的结算分钟数合计。
- 已分配工时：工时分配分钟数合计。
- 待结算工时：原始工时减去已分配工时。
- 报告名称：日报使用日期，周报/月报使用日期范围。
- 完成列表中的层级路径：通过任务父链递归生成。

## 5. 表结构

### 5.1 云端工作空间与同步基础表

以下表在 Supabase PostgreSQL 中是正式业务表；本地模式在 SQLite 中创建语义等价的最小记录，以便上层 DTO 和服务逻辑保持一致。

#### `workspaces` 数据空间

用途：隔离一个用户的整套待办、计时和报告数据。

| 字段 | PostgreSQL 类型 | 约束 | 说明 |
| --- | --- | --- | --- |
| `id` | uuid | PK | 工作空间 ID |
| `owner_user_id` | uuid | NOT NULL, FK `auth.users` | 所有者账号 |
| `name` | text | NOT NULL DEFAULT `我的工作台` | 空间名称 |
| `timezone` | text | NOT NULL DEFAULT `Asia/Shanghai` | 业务时区 |
| `created_at` | timestamptz | NOT NULL | 创建时间 |
| `updated_at` | timestamptz | NOT NULL | 更新时间 |
| `version` | bigint | NOT NULL DEFAULT 1 | 乐观锁 |
| `deleted_at` | timestamptz | NULL | 软删除 |

首版一个账号只创建一个活跃工作空间，唯一索引约束 `owner_user_id WHERE deleted_at IS NULL`。该结构用于同一用户多设备，不提供工作空间成员和多人邀请。

#### `devices` 设备

| 字段 | PostgreSQL 类型 | 约束 | 说明 |
| --- | --- | --- | --- |
| `id` | uuid | PK | 安装实例生成并持久化的设备 ID |
| `workspace_id` | uuid | NOT NULL, FK | 所属工作空间 |
| `device_name` | text | NOT NULL | 用户可识别名称，例如“办公室电脑” |
| `platform` | text | NOT NULL | `windows`，预留其他平台 |
| `app_version` | text | NOT NULL | 最近连接的应用版本 |
| `last_seen_at` | timestamptz | NOT NULL | 最近心跳 |
| `created_at` | timestamptz | NOT NULL | 注册时间 |
| `revoked_at` | timestamptz | NULL | 设备撤销时间 |

设备撤销后，关联会话失效，未同步离线数据需要在撤销前处理。

#### `tracking_leases` 后台采集租约

用途：保证多个设备同时在线时，只有一个设备累计后台未归属时间。

| 字段 | PostgreSQL 类型 | 约束 | 说明 |
| --- | --- | --- | --- |
| `workspace_id` | uuid | PK, FK | 每个工作空间一条租约 |
| `holder_device_id` | uuid | NULL, FK | 当前采集设备 |
| `lease_token` | uuid | NULL | 每次获取租约生成的新令牌 |
| `expires_at` | timestamptz | NULL | 租约过期时间 |
| `updated_at` | timestamptz | NOT NULL | 最近续租时间 |
| `version` | bigint | NOT NULL DEFAULT 1 | 乐观锁 |

规则：

- 设备在主窗口或托盘后台服务运行时，每 15 秒续租一次，租期 45 秒。
- 只有持有有效 `lease_token` 的设备可以打开或续写 `unassigned_segments`。
- 其他设备显示“后台时间由设备 X 记录”，但仍可编辑待办、报告和查看计时。
- 租约过期后其他在线设备可以原子接管，旧设备的后续续租必须因 token 不匹配而失败。

#### `workspace_changes` 云端变更序列

用途：为 Realtime 和断线重连提供可补拉的单调变更序列。

| 字段 | PostgreSQL 类型 | 约束 | 说明 |
| --- | --- | --- | --- |
| `change_seq` | bigint identity | PK | 全局递增序号 |
| `workspace_id` | uuid | NOT NULL, FK | 数据空间 |
| `entity_type` | text | NOT NULL | 例如 `task`、`time_entry`、`report` |
| `entity_id` | uuid | NOT NULL | 实体 ID |
| `operation` | text | NOT NULL | `insert`、`update`、`delete` |
| `entity_version` | bigint | NOT NULL | 变更后的实体版本 |
| `changed_by_device_id` | uuid | NULL | 发起设备 |
| `changed_at` | timestamptz | NOT NULL | 变更时间 |

所有可同步业务表通过数据库 trigger 在事务内追加变更记录。Supabase Realtime 只订阅本表；客户端收到 `change_seq` 后通过增量接口拉取实体，避免直接依赖可能丢失的广播载荷。

#### `processed_operations` 幂等操作

| 字段 | PostgreSQL 类型 | 约束 | 说明 |
| --- | --- | --- | --- |
| `workspace_id` | uuid | PK part | 数据空间 |
| `operation_id` | uuid | PK part | 客户端操作 ID |
| `device_id` | uuid | NOT NULL | 发起设备 |
| `operation_type` | text | NOT NULL | 操作类型 |
| `result_json` | jsonb | NOT NULL | 首次执行结果 |
| `processed_at` | timestamptz | NOT NULL | 执行时间 |

相同 `operation_id` 重试时直接返回首次结果。记录保留 30 天后可由定时任务清理。

#### Supabase RLS

`workspaces` 策略：`owner_user_id = auth.uid()`。

其他所有云端业务表策略：

```sql
exists (
  select 1
  from workspaces w
  where w.id = <table>.workspace_id
    and w.owner_user_id = auth.uid()
    and w.deleted_at is null
)
```

- 对 `select/insert/update/delete` 分别启用 RLS，`insert/update` 同时配置 `WITH CHECK`。
- 客户端只使用 Supabase `anon` key 和用户会话 JWT。
- `service_role` 只允许出现在受控服务端环境或 Edge Function Secret 中，绝不打包进 Tauri 应用。
- 高约束写操作通过 PostgreSQL RPC 函数执行，函数内部再次校验 `auth.uid()` 与工作空间所有权。

### 5.2 `subjects` 主体

用途：承载彼此独立的待办列表、计时选择范围和报告归属。

| 字段 | 类型 | 约束 | 说明 |
| --- | --- | --- | --- |
| `id` | TEXT | PK | 主体 ID |
| `name` | TEXT | NOT NULL | 主体名称，去除首尾空格后不能为空 |
| `sort_order` | INTEGER | NOT NULL DEFAULT 0 | 侧栏顺序 |
| `created_at` | INTEGER | NOT NULL | 创建时间 |
| `updated_at` | INTEGER | NOT NULL | 更新时间 |
| `version` | INTEGER | NOT NULL DEFAULT 1 | 乐观锁 |

约束和规则：

- 建立 `UNIQUE INDEX lower(name)`，主体名称大小写不敏感且不可重复。
- “默认”只是普通名称，不增加 `is_default` 字段。
- 当前 UI 只支持新增和重命名，不设计删除接口。
- 默认选中的主体保存到 `app_settings.default_subject_id`。

### 5.3 `work_days` 工作日

用途：保存指定日期的工作时段、结算确认和日报级备注。

| 字段 | 类型 | 约束 | 说明 |
| --- | --- | --- | --- |
| `workspace_id` | UUID/TEXT | PK part, FK | 所属数据空间 |
| `work_date` | TEXT | PK part | 本地工作日期 |
| `timezone` | TEXT | NOT NULL | 例如 `Asia/Shanghai` |
| `work_period_text` | TEXT | NULL | 例如 `10:00-12:00 13:30-17:30` |
| `note` | TEXT | NULL | 当日备注 |
| `settled_at` | INTEGER | NULL | 用户确认当天工时已结算的时间 |
| `created_at` | INTEGER | NOT NULL | 创建时间 |
| `updated_at` | INTEGER | NOT NULL | 更新时间 |
| `version` | INTEGER | NOT NULL DEFAULT 1 | 乐观锁 |

唯一键为 `(workspace_id, work_date)`。`settled_at` 是用户确认，不等同于“待结算分钟为 0”；重新分配工时后应自动清空该字段。

### 5.4 `tasks` 待办事项

用途：保存主体下的层级待办、预计开始日期和整体预计用时。

| 字段 | 类型 | 约束 | 说明 |
| --- | --- | --- | --- |
| `id` | TEXT | PK | 稳定事项 ID |
| `subject_id` | TEXT | NOT NULL, FK | 所属主体 |
| `parent_id` | TEXT | NULL, FK self | 上级事项 |
| `title` | TEXT | NOT NULL | 事项名称 |
| `status` | TEXT | NOT NULL DEFAULT `open` | `open`、`done` |
| `planned_date` | TEXT | NULL | 可选预计开始日期 |
| `planned_time` | TEXT | NULL | 兼容旧数据库和云快照，业务层不再读写 |
| `estimate_minutes` | INTEGER | NULL | 可选整体预计用时，必须大于 0 |
| `note` | TEXT | NULL | 备注 |
| `project_name` | TEXT | NULL | SeaTable 兼容字段，当前 UI 不展示 |
| `solution_name` | TEXT | NULL | SeaTable 兼容字段，当前 UI 不展示 |
| `source_type` | TEXT | NOT NULL | `manual`、`obsidian_import`、`timer_quick_create` |
| `source_ref` | TEXT | NULL | 导入批次或外部来源标识 |
| `sort_order` | INTEGER | NOT NULL | 同一父级内顺序 |
| `completed_at` | INTEGER | NULL | 完成时间 |
| `created_at` | INTEGER | NOT NULL | 创建时间 |
| `updated_at` | INTEGER | NOT NULL | 更新时间 |
| `version` | INTEGER | NOT NULL DEFAULT 1 | 乐观锁 |
| `deleted_at` | INTEGER | NULL | 软删除时间 |

索引：

- `idx_tasks_subject_parent_order(subject_id, parent_id, sort_order)`。
- `idx_tasks_planned_date(subject_id, planned_date, status)`。
- `idx_tasks_completed_at(subject_id, completed_at)`。
- `idx_tasks_source(source_type, source_ref)`。

树结构规则：

1. 父子事项必须属于同一主体。
2. 禁止将事项移动到自身或后代下面，避免循环。
3. 不设置业务层级上限；服务层遍历时必须检测循环并设置安全保护。
4. 拖动排序移动事项和全部后代，只更新同级 `sort_order`，不得修改 `parent_id`。
5. 增加/减少缩进单独修改 `parent_id` 并重新计算相关同级顺序。
6. 删除上级事项时，UI 确认后整棵子树软删除。
7. 空白占位行不是事项；只有用户提交非空标题时才插入数据库。
8. 复制上级事项时复制整棵子树，为所有副本生成新 ID，根标题增加“ - 副本”。

### 5.5 `task_daily_estimates` 事项按日预计

用途：保存某个事项在指定日期预计投入的分钟数，不表示事项被安排到该日期。

| 字段 | 类型 | 约束 | 说明 |
| --- | --- | --- | --- |
| `workspace_id` | TEXT | NOT NULL, FK | 所属数据空间 |
| `task_id` | TEXT | PK part, FK | 事项 ID |
| `work_date` | TEXT | PK part | 预计投入日期 |
| `estimate_minutes` | INTEGER | NOT NULL | 今日预计用时，必须大于 0 |
| `created_at` | INTEGER | NOT NULL | 创建时间 |
| `updated_at` | INTEGER | NOT NULL | 更新时间 |
| `version` | INTEGER | NOT NULL DEFAULT 1 | 版本号 |

唯一键为 `(task_id, work_date)`。清空今日预计时删除对应记录；不得覆盖 `tasks.estimate_minutes`。

完成规则：

- 完成和取消完成只影响当前事项，不自动级联父项或子项。
- 已完成列表以平铺方式显示，但通过父链展示完整路径。
- `status='done'` 时写入 `completed_at`；恢复未完成时清空。
- 每次完成或取消完成同时追加 `task_status_events`，用于历史报告按周期结束时间还原事项状态。
- 旧 mock 数据中的 `planned`、`active`、`deferred` 迁移为 `open`，`done` 迁移为 `done`。

可分配性规则：

- 只有没有未删除子项的叶子事项可以作为计时默认事项或工时分配目标。
- 已存在直接工时分配的事项不得新增第一个子项，也不得通过缩进操作变成其他事项的父项。
- 如需把已有叶子事项变成父项，必须先把它的直接分配迁移到具体子项。

#### `task_status_events` 事项状态历史

用途：支持对历史日报、周报和月报重复生成，不因事项后来取消完成而丢失周期结束时的状态。

| 字段 | 类型 | 约束 | 说明 |
| --- | --- | --- | --- |
| `id` | TEXT | PK | 事件 ID |
| `task_id` | TEXT | NOT NULL, FK | 事项 ID |
| `status` | TEXT | NOT NULL | `open`、`done` |
| `occurred_at` | INTEGER | NOT NULL | 用户执行状态变更的时间 |
| `source_type` | TEXT | NOT NULL | `user`、`import`、`migration` |
| `created_at` | INTEGER | NOT NULL | 事件写入时间 |

索引 `idx_task_status_events_task_time(task_id, occurred_at)`。`tasks.status` 是当前状态缓存；指定历史时间点的状态取该时间点之前最后一条事件，没有事件时按事项创建时的 `open` 处理。

### 5.5 `time_entries` 原始时间记录

用途：保存计时器、手动补录和未归属时间处理后形成的客观时间片。

| 字段 | 类型 | 约束 | 说明 |
| --- | --- | --- | --- |
| `id` | TEXT | PK | 时间记录 ID |
| `work_date` | TEXT | NOT NULL, FK | 归属工作日期 |
| `kind` | TEXT | NOT NULL | `work`、`break` |
| `source_type` | TEXT | NOT NULL | `timer`、`manual`、`unassigned` |
| `state` | TEXT | NOT NULL | `running`、`paused`、`ended` |
| `default_task_id` | TEXT | NULL, FK | 开始时意图事项，不代表最终归属 |
| `label_snapshot` | TEXT | NOT NULL | 时间记录列表展示名称 |
| `started_at` | INTEGER | NOT NULL | 最早开始时间 |
| `ended_at` | INTEGER | NULL | 结束时间 |
| `duration_seconds` | INTEGER | NOT NULL DEFAULT 0 | 有效工作/休息秒数缓存 |
| `note` | TEXT | NULL | 计时备注 |
| `origin_unassigned_session_id` | TEXT | NULL, UNIQUE | 来源未归属会话 |
| `created_at` | INTEGER | NOT NULL | 创建时间 |
| `updated_at` | INTEGER | NOT NULL | 更新时间 |
| `version` | INTEGER | NOT NULL DEFAULT 1 | 乐观锁 |
| `deleted_at` | INTEGER | NULL | 修正时保留历史引用 |

约束和索引：

- 同一工作空间只允许一条 `running` 或 `paused` 的未删除记录。本地模式使用 SQLite 部分唯一索引；云端模式使用 PostgreSQL 部分唯一索引并通过原子 RPC 操作。
- `ended` 必须有 `ended_at`，`running/paused` 不得有 `ended_at`。
- `kind='break'` 不得存在工时分配。
- `default_task_id` 必须指向叶子事项。
- `duration_seconds` 由时间分段汇总，前端不得直接修改。
- 索引 `idx_time_entries_date(work_date, started_at)`、`idx_time_entries_default_task(default_task_id)`。

查询 DTO 可以额外返回 `settlement_minutes = duration_seconds > 0 ? max(1, ceil(duration_seconds / 60)) : 0`，该字段是派生值，不在表中重复保存。

### 5.6 `time_segments` 计时有效分段

用途：表达暂停和继续，不把暂停期间计入时长。

| 字段 | 类型 | 约束 | 说明 |
| --- | --- | --- | --- |
| `id` | TEXT | PK | 分段 ID |
| `entry_id` | TEXT | NOT NULL, FK | 原始时间记录 |
| `sequence_no` | INTEGER | NOT NULL | 分段顺序 |
| `started_at` | INTEGER | NOT NULL | 分段开始 |
| `ended_at` | INTEGER | NULL | 分段结束；运行中分段为空 |
| `duration_seconds` | INTEGER | NOT NULL DEFAULT 0 | 已结束分段秒数 |

约束：`UNIQUE(entry_id, sequence_no)`；一条记录最多一个未结束分段。

### 5.7 `time_allocations` 工时分配

用途：将工作时间记录的结算分钟拆分给一个或多个具体事项。

| 字段 | 类型 | 约束 | 说明 |
| --- | --- | --- | --- |
| `id` | TEXT | PK | 分配 ID |
| `entry_id` | TEXT | NOT NULL, FK | 原始时间记录 |
| `task_id` | TEXT | NOT NULL, FK | 叶子事项 |
| `minutes` | INTEGER | NOT NULL | 分配分钟，必须大于 0 |
| `note` | TEXT | NULL | 分配说明 |
| `created_at` | INTEGER | NOT NULL | 创建时间 |
| `updated_at` | INTEGER | NOT NULL | 更新时间 |
| `version` | INTEGER | NOT NULL DEFAULT 1 | 乐观锁 |

约束和规则：

- `UNIQUE(entry_id, task_id)`；同一时间记录分给同一事项的多行在保存时合并。
- `sum(minutes) <= time_entry.settlement_minutes`。
- 允许小于原始分钟，差额作为待结算工时；未归属时间弹框选择“分配给事项”时必须恰好相等。
- 修改时间范围导致可分配分钟减少时，如果现有分配超出，事务拒绝并返回超出分钟数。
- 重新分配只更新本表，不修改原始时间范围和分段。

### 5.8 `unassigned_sessions` 后台未归属时间会话

用途：在没有手动计时事项时累计后台时间，并保证应用重启或窗口切换后可恢复。

| 字段 | 类型 | 约束 | 说明 |
| --- | --- | --- | --- |
| `id` | TEXT | PK | 会话 ID |
| `work_date` | TEXT | NOT NULL | 工作日期 |
| `state` | TEXT | NOT NULL | `collecting`、`awaiting_resolution`、`resolved`、`discarded` |
| `threshold_seconds` | INTEGER | NOT NULL DEFAULT 300 | 自动弹框阈值 |
| `duration_seconds` | INTEGER | NOT NULL DEFAULT 0 | 分段累计秒数缓存 |
| `first_started_at` | INTEGER | NOT NULL | 首次开始累计时间 |
| `last_ended_at` | INTEGER | NULL | 最近结束分段时间 |
| `prompted_at` | INTEGER | NULL | 首次达到阈值时间 |
| `resolution_type` | TEXT | NULL | `work`、`break`、`discard` |
| `generated_entry_id` | TEXT | NULL, UNIQUE | 处理后生成的时间记录 |
| `resolved_at` | INTEGER | NULL | 处理完成时间 |
| `created_at` | INTEGER | NOT NULL | 创建时间 |
| `updated_at` | INTEGER | NOT NULL | 更新时间 |
| `version` | INTEGER | NOT NULL DEFAULT 1 | 乐观锁 |

规则：

- 全局最多一条 `collecting/awaiting_resolution` 会话。
- 云端模式只有后台采集租约持有设备可以累计分段；任意在线设备都可以处理已经进入 `awaiting_resolution` 的会话。
- 没有活动计时器时打开一个未归属分段；开始手动计时时关闭该分段；结束计时后继续同一未处理会话。
- 累计大于 300 秒时切换为 `awaiting_resolution`。主窗口打开时必须展示不可关闭弹框，同时右上角指示器持续显示实时秒数。
- 处理时在同一事务中封口当前分段，并执行以下一种动作：
  - `work`：创建 `kind='work'` 的时间记录和一到多条分配，总分钟必须相等。
  - `break`：创建 `kind='break'` 的时间记录，不创建分配。
  - `discard`：不创建时间记录，仅保留已丢弃审计信息。
- 处理完成且当前无计时器时立即创建下一条未归属会话。

### 5.9 `unassigned_segments` 未归属时间分段

字段与 `time_segments` 相同，外键改为 `session_id`。它用于排除手动计时期间，不允许通过简单的首尾时间差计算未归属时长。

### 5.10 `report_templates` 报告模板

用途：保存日报、周报和月报的 Markdown 模板与占位符设置。

| 字段 | 类型 | 约束 | 说明 |
| --- | --- | --- | --- |
| `id` | TEXT | PK | 模板 ID |
| `report_type` | TEXT | NOT NULL | `daily`、`weekly`、`monthly` |
| `subject_id` | TEXT | NULL, FK | 为空表示全局模板 |
| `content` | TEXT | NOT NULL | Markdown 模板 |
| `is_builtin` | INTEGER | NOT NULL DEFAULT 0 | 是否内置默认模板 |
| `created_at` | INTEGER | NOT NULL | 创建时间 |
| `updated_at` | INTEGER | NOT NULL | 更新时间 |
| `version` | INTEGER | NOT NULL DEFAULT 1 | 乐观锁 |

唯一规则：每种报告类型最多一个全局模板；每个主体和报告类型最多一个覆盖模板。

首版占位符：

- `{{姓名}}`、`{{日期}}`、`{{星期}}`、`{{日期范围}}`
- `{{工作时段}}`、`{{总工时}}`
- `{{今日事项}}`、`{{明日计划}}`
- `{{每日情况}}`、`{{统计信息}}`、`{{总结}}`

保存模板时必须校验未知占位符，但允许普通 Markdown 文本。

### 5.11 `reports` 报告记录

用途：统一保存日报、周报和月报的范围、当前 Markdown 和生成状态。

| 字段 | 类型 | 约束 | 说明 |
| --- | --- | --- | --- |
| `id` | TEXT | PK | 报告 ID |
| `report_type` | TEXT | NOT NULL | `daily`、`weekly`、`monthly` |
| `subject_id` | TEXT | NOT NULL, FK | 报告主体 |
| `period_start` | TEXT | NOT NULL | 范围开始日期 |
| `period_end` | TEXT | NOT NULL | 范围结束日期 |
| `reference_date` | TEXT | NOT NULL | UI 日期控件参考日期 |
| `template_id` | TEXT | NULL, FK | 最近生成使用的模板 |
| `markdown_content` | TEXT | NOT NULL | 当前可编辑内容 |
| `content_source` | TEXT | NOT NULL | `generated`、`edited` |
| `generation_count` | INTEGER | NOT NULL DEFAULT 1 | 生成次数 |
| `input_revision_hash` | TEXT | NULL | 最近生成输入摘要 |
| `generated_at` | INTEGER | NULL | 最近生成时间 |
| `created_at` | INTEGER | NOT NULL | 创建时间 |
| `updated_at` | INTEGER | NOT NULL | 更新时间 |
| `version` | INTEGER | NOT NULL DEFAULT 1 | 乐观锁 |
| `deleted_at` | INTEGER | NULL | 删除时间 |

约束：

- 活跃报告唯一键为 `(report_type, subject_id, period_start, period_end)`。
- 报告名称由范围派生，不增加 `name` 字段。
- 日报必须满足 `period_start=period_end`；周报固定周一至周日；月报固定自然月首日至末日。
- 保存编辑内容时 `content_source='edited'`。
- 重新生成前必须确认，生成后覆盖 `markdown_content`、增加 `generation_count` 并改为 `generated`。
- `input_revision_hash` 由范围、任务版本、分配版本和模板版本计算，用于提示数据已经变化。
- Obsidian 本地路径和文件修改时间不保存在云端 `reports`，由每台设备的 `local_report_outputs` 记录。

### 5.12 `report_tasks` 报告事项范围

用途：保存创建/调整报告时选择的事项，以及事项被删除后的回退展示信息。

| 字段 | 类型 | 约束 | 说明 |
| --- | --- | --- | --- |
| `report_id` | TEXT | PK part, FK | 报告 ID |
| `task_id` | TEXT | PK part, FK | 事项 ID |
| `sort_order` | INTEGER | NOT NULL | 生成顺序 |
| `title_snapshot` | TEXT | NOT NULL | 选择时标题 |
| `path_snapshot` | TEXT | NOT NULL | 选择时完整层级路径 |

生成规则：

- 日报主事项只自动选择已完成或范围内实际用时大于 0 的事项；完全未开始事项不进入今日事项。
- 历史报告中的完成状态按 `min(period_end 当日结束时间, 生成时间)` 之前最后一条 `task_status_events` 判断，不直接使用事项当前状态。
- 有子项的父事项只作为结构标题，不显示红绿状态图标，也不直接显示工时。
- 叶子事项已完成显示绿色，已投入时间但未完成显示红色。
- 明日计划从该主体仍未完成的待办按层级和顺序生成，不计入“今日事项”。
- 周报/月报按天列出可用数据，并明确列出缺少记录或未结算日期。

### 5.13 `plan_import_batches` 与 `plan_import_items`

用途：支持从 Obsidian 昨日日报预览并确认导入今日候选计划，保留无法识别的原文。

`plan_import_batches` 主要字段：

- `id`、`target_date`、`subject_id`
- `source_path`、`source_mtime`、`source_content_hash`
- `state`: `preview`、`confirmed`、`cancelled`、`failed`
- `raw_markdown`、`warnings_json`
- `created_at`、`updated_at`、`version`

`plan_import_items` 主要字段：

- `id`、`batch_id`、`parent_item_id`
- `title`、`estimate_minutes`、`sort_order`
- `source_line_no`、`source_text`
- `parse_status`: `recognized`、`unrecognized`
- `confirmed_task_id`

确认导入时，只把用户保留且标题非空的识别项转换为 `tasks`；未识别行继续保存在导入批次中供查看。

### 5.14 `external_bindings`

用途：保存本地事项与 SeaTable 行等外部实体的稳定绑定，避免按标题重复创建。

| 字段 | 类型 | 约束 | 说明 |
| --- | --- | --- | --- |
| `id` | TEXT | PK | 绑定 ID |
| `provider` | TEXT | NOT NULL | 首版为 `seatable` |
| `entity_type` | TEXT | NOT NULL | 首版为 `task` |
| `entity_id` | TEXT | NOT NULL | 本地实体 ID |
| `external_id` | TEXT | NOT NULL | SeaTable row ID |
| `external_revision` | TEXT | NULL | 外部版本标识 |
| `last_synced_hash` | TEXT | NULL | 最近同步内容摘要 |
| `last_synced_at` | INTEGER | NULL | 最近成功同步时间 |

唯一约束：`(provider, entity_type, entity_id)` 和 `(provider, external_id)`。

### 5.15 `sync_runs` 与 `sync_items`

用途：记录 Obsidian/SeaTable 的预览、执行和失败重试，不依赖独立同步页面。

`sync_runs`：

- `id`、`provider`、`operation_type`
- `state`: `preview`、`running`、`partial`、`succeeded`、`failed`
- `request_json`、`preview_json`
- `success_count`、`failed_count`、`skipped_count`
- `error_code`、`error_message`
- `created_at`、`started_at`、`completed_at`

`sync_items`：

- `id`、`run_id`、`entity_type`、`entity_id`
- `action`: `create`、`update`、`skip`、`conflict`、`write_file`
- `state`: `pending`、`succeeded`、`failed`
- `external_id`、`before_json`、`after_json`
- `error_code`、`error_message`

预览记录在确认执行前不得产生外部写入。部分失败时重试只复制失败项到新的同步运行。

### 5.16 `app_settings`、`device_settings` 与 `integration_configs`

`app_settings` 保存应在多设备间共享的工作空间设置，使用受控键值结构：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `workspace_id` | UUID/TEXT | PK part，云端工作空间；本地模式使用本地工作空间 ID |
| `key` | TEXT | PK part，设置键 |
| `value_json` | TEXT | JSON 值 |
| `updated_at` | INTEGER | 更新时间 |

首版设置键：

- `user_name`
- `timezone`
- `default_subject_id`
- `unassigned_prompt_seconds`，默认 300
- `default_work_period_text`
- `salary_hourly_rate`

其中托盘和启动行为属于设备级设置，正式实现时从 `app_settings` 移入本地 SQLite 的 `device_settings`。`device_settings` 不上传 Supabase，至少包含：

- `tray_hover_enabled`
- `tray_menu_suppress_hover`
- `start_minimized`
- `obsidian_root_path`
- `obsidian_daily_path_pattern`
- `device_name`
- `storage_mode`
- `supabase_project_url`

Supabase `anon` key 可以随应用配置或连接配置保存，不属于管理密钥；用户 access/refresh token 必须保存到系统凭据库。

`integration_configs` 保存非敏感的共享集成配置。SeaTable 地址、Base 和字段映射可以按工作空间同步；Obsidian 路径和写入规则只保存在 `device_settings`：

- `workspace_id`、`provider` 联合主键；首版共享 provider 为 `seatable`
- `enabled`
- `config_json`：路径、服务地址、表名和字段映射
- `secret_ref`：系统凭据库中的引用，不保存 Token 原文；Supabase refresh token 使用单独的 `supabase` 凭据引用
- `updated_at`、`version`

SeaTable Token、Supabase refresh token 等敏感值应使用 Windows Credential Manager 或等价系统凭据存储。普通数据库导出、报告和日志不得包含完整凭据。

### 5.17 云端模式本地同步表

这些表只存在于设备 SQLite，不上传 Supabase。

#### `local_sync_state`

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `workspace_id` | TEXT PK | 当前云端工作空间 |
| `device_id` | TEXT NOT NULL | 当前设备 |
| `last_change_seq` | INTEGER NOT NULL DEFAULT 0 | 已应用的最大云端变更序号 |
| `last_full_sync_at` | INTEGER NULL | 最近全量同步时间 |
| `last_error` | TEXT NULL | 最近同步错误摘要 |

#### `sync_outbox`

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `operation_id` | TEXT PK | 幂等操作 ID |
| `workspace_id` | TEXT NOT NULL | 工作空间 |
| `device_id` | TEXT NOT NULL | 发起设备 |
| `operation_type` | TEXT NOT NULL | 业务操作类型 |
| `entity_type` | TEXT NOT NULL | 实体类型 |
| `entity_id` | TEXT NULL | 实体 ID |
| `base_version` | INTEGER NULL | 离线编辑前版本 |
| `payload_json` | TEXT NOT NULL | 完整命令参数 |
| `state` | TEXT NOT NULL | `pending`、`sending`、`conflict`、`failed` |
| `attempt_count` | INTEGER NOT NULL DEFAULT 0 | 重试次数 |
| `created_at` | INTEGER NOT NULL | 本地创建时间 |
| `last_attempt_at` | INTEGER NULL | 最近尝试时间 |
| `error_json` | TEXT NULL | 失败或冲突详情 |

普通待办、报告正文和设置编辑可以进入 outbox。以下全局单例操作在云端模式离线时不允许新发起：开始/暂停/继续/结束计时、处理未归属时间、获取后台采集租约。正在运行的计时器断网后仍可按本地时间显示；“结束”可记录为本地待提交请求，但在同步完成前禁止开始下一条计时。

#### `local_report_outputs`

用途：记录某台设备把共享报告写入哪个本地 Obsidian 文件，避免不同机器的磁盘路径互相覆盖。

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `report_id` | TEXT PK part | 共享报告 ID |
| `device_id` | TEXT PK part | 写入设备 |
| `output_path` | TEXT NOT NULL | 本机实际路径 |
| `output_content_hash` | TEXT NOT NULL | 最近写入内容摘要 |
| `output_file_mtime` | INTEGER NOT NULL | 写入后文件修改时间 |
| `written_at` | INTEGER NOT NULL | 最近写入时间 |

Obsidian 导入批次也默认保存在设备 SQLite；确认后的事项才进入共享业务表。只有需要跨设备审计导入来源时，才把脱敏后的批次摘要上传云端。

## 6. 状态流转

### 6.1 待办

```text
open ──完成──> done
done ──取消完成──> open
```

“未开始、已开始、进行中、已暂停”均是查询结果，不参与流转。

### 6.2 时间记录

```text
running ──暂停──> paused ──继续──> running
running ──结束──> ended
paused  ──结束──> ended
```

不允许 `ended` 恢复为运行；修正通过编辑已结束记录完成。

### 6.3 未归属时间

```text
collecting ──累计超过阈值──> awaiting_resolution
collecting/awaiting_resolution ──分配工作──> resolved(work)
collecting/awaiting_resolution ──标记休息──> resolved(break)
collecting/awaiting_resolution ──丢弃──> discarded
```

### 6.4 外部同步

```text
preview ──确认──> running ──全部成功──> succeeded
                         ├──部分失败──> partial
                         └──全部失败──> failed
```

## 7. 关键查询和汇总

### 7.1 事项选择器

- 按当前主体、树顺序返回未完成事项，再返回已完成事项。
- 父事项返回 `selectable=false`，用时为所有后代叶子事项汇总。
- 叶子事项返回直接分配用时。
- 进行中/已暂停优先于已完成和已开始状态展示。

### 7.2 今日概览

- 原始工时：当天 `kind='work'` 的时间记录结算分钟合计。
- 已分配：当天分配分钟合计。
- 待结算：`max(0, 原始工时 - 已分配)`。
- 休息时间单独统计，不进入上述三个数字。
- 正在运行的时间记录使用当前开放分段实时计算，但不每秒写数据库。

### 7.3 报告范围

- 日报：指定日期。
- 周报：参考日期所在周的周一至周日。
- 月报：参考日期所在自然月。
- 事项候选按主体过滤，并保持待办树顺序。
- 已完成或有实际分配的事项默认选中；用户可以手动调整范围。

## 8. 示例数据

以下为评审示例，不是待执行 SQL。

### 8.1 主体和事项

```json
{
  "subjects": [
    { "id": "0199-default-subject", "name": "默认", "sortOrder": 10 }
  ],
  "tasks": [
    {
      "id": "0199-task-plan-review",
      "subjectId": "0199-default-subject",
      "parentId": null,
      "title": "今日计划梳理与方案预备",
      "status": "done",
      "estimateMinutes": 12,
      "sortOrder": 10,
      "sourceType": "manual"
    },
    {
      "id": "0199-task-ops",
      "subjectId": "0199-default-subject",
      "parentId": null,
      "title": "运维相关",
      "status": "open",
      "sortOrder": 20,
      "sourceType": "manual"
    },
    {
      "id": "0199-task-sync-v3",
      "subjectId": "0199-default-subject",
      "parentId": "0199-task-ops",
      "title": "开发江湖数据同步v3功能",
      "status": "open",
      "sortOrder": 10,
      "sourceType": "manual"
    },
    {
      "id": "0199-task-pve-backup",
      "subjectId": "0199-default-subject",
      "parentId": "0199-task-ops",
      "title": "按照规则重新分配PVE虚拟机ID，开启虚拟机备份",
      "status": "done",
      "sortOrder": 20,
      "sourceType": "obsidian_import"
    }
  ]
}
```

该数据覆盖一级事项、父事项、未完成子项和只完成二级事项的场景。两个已完成事项在完成列表中平铺显示，其中二级完成项带路径“运维相关”。

### 8.2 计时和拆分分配

```json
{
  "entry": {
    "id": "0199-entry-001",
    "workDate": "2026-09-24",
    "kind": "work",
    "sourceType": "timer",
    "state": "ended",
    "defaultTaskId": "0199-task-sync-v3",
    "labelSnapshot": "运维相关 / 开发江湖数据同步v3功能",
    "startedAt": 1790215200000,
    "endedAt": 1790220600000,
    "durationSeconds": 5400,
    "settlementMinutes": 90
  },
  "allocations": [
    { "taskId": "0199-task-sync-v3", "minutes": 60 },
    { "taskId": "0199-task-pve-backup", "minutes": 30 }
  ]
}
```

该数据覆盖默认事项与最终归属不同、单条时间拆分给多个事项的场景。

### 8.3 未归属时间

```json
{
  "session": {
    "id": "0199-unassigned-001",
    "workDate": "2026-09-24",
    "state": "awaiting_resolution",
    "thresholdSeconds": 300,
    "durationSeconds": 487,
    "firstStartedAt": 1790226000000,
    "promptedAt": 1790226487000
  },
  "resolutionDraft": [
    { "taskId": "0199-task-sync-v3", "minutes": 5 },
    { "taskId": "0199-task-pve-backup", "minutes": 4 }
  ]
}
```

487 秒按规则结算为 9 分钟，弹框默认已有一行，并允许增加第二行。

### 8.4 报告

```json
{
  "id": "0199-report-daily-20260924",
  "reportType": "daily",
  "subjectId": "0199-default-subject",
  "periodStart": "2026-09-24",
  "periodEnd": "2026-09-24",
  "referenceDate": "2026-09-24",
  "contentSource": "edited",
  "generationCount": 2,
  "markdownContent": "【晓健】2026-09-24 周四 ..."
}
```

报告列表显示名称 `2026-09-24`，不保存另一份可编辑标题。

## 9. 初始化数据与迁移策略

本地模式正式初始化必需数据：

1. 创建本地工作空间和本地设备记录。
2. 创建名称为“默认”的普通主体。
3. 把该主体 ID 写入 `app_settings.default_subject_id`。
4. 写入日报、周报、月报三份内置模板。
5. 写入 `unassigned_prompt_seconds=300` 和当前时区。

云端模式首次登录初始化：

1. 使用 Supabase Auth 登录并创建/读取当前用户唯一工作空间。
2. 注册当前设备。
3. 工作空间为空时创建“默认”主体、内置模板和默认共享设置。
4. 全量下载云端数据到本机 SQLite 缓存，记录最大 `change_seq`。
5. 启动 Realtime 订阅和后台采集租约竞争。

本地数据切换云端时必须先显示迁移预览，包括实体数量、重复工作空间状态和冲突项。迁移通过幂等操作批次写入；全部成功后才把设备 `storage_mode` 切换为 `cloud`。云端导出本地时创建新的本地工作空间快照，之后不再与原云端空间同步。

UI mock 数据只用于开发演示，不应自动写入用户正式数据库。需要演示数据时使用显式的开发种子开关。

从当前 UI 快照迁移时：

- `Task.actual` 丢弃，实际用时统一由分配表计算。
- `Task.project` 仅在有值时迁移到 `project_name`。
- `planned/active/deferred` 迁移为 `open`，`done` 迁移为 `done`。
- 为迁移后的每个事项写入一条 `migration` 状态事件，事件时间取原数据更新时间或迁移时间。
- `TimeEntry.minutes` 转为 `duration_seconds = minutes * 60`。
- `TimeEntry.allocations` 拆到 `time_allocations`。
- 当前 `reports` 未持久化，正式迁移不读取页面内临时报告。
- `statusColors` 属于已取消的四状态 UI，不迁移。

## 10. 正式实现前必须确认

1. 将 OpenSpec 的四种事项状态改为完成/未完成，并把执行状态定义为计时派生状态。
2. 将主体、月报、统一报告库和未归属时间补充到规格。
3. 将原先“单用户、本地 SQLite”边界改为可选 Supabase 云端模式，并确认首版只支持同一账号多设备。
4. 确认本地模式和云端模式是否首版同时交付，以及本地数据迁移云端的入口。
5. 确认 Supabase 项目、Auth 登录方式、邮箱确认和密码重置策略。
6. 确认同步模块不恢复独立页面，但 Obsidian/SeaTable 底层能力仍属于后续实现范围。
7. 确认时间片统一采用向上取整到分钟；这会影响不足一分钟的计时和未归属时间。
8. 确认“已有直接工时的事项不能变成父事项”的约束和迁移提示文案。
9. 获取实际 SeaTable 表名、字段映射和稳定匹配字段后，再定稿 `integration_configs.config_json`。
10. 获取真实 Obsidian 日报样例后，再冻结解析规则和默认模板。

本阶段禁止创建 Supabase 项目或表、执行建表 SQL、写入 SQLite、调用 SeaTable 或覆盖 Obsidian 文件。
