# Design

## Context

参见 `proposal.md` 的动机。当前桌面端使用 `timegenie.sqlite3` 作为本地模式主库或云端模式缓存；云端写入先提交 SQLite 并进入 `sync_outbox`，再调用 Supabase RPC。`workspace_changes` 仅保存实体、版本和变更序号，`processed_operations` 负责操作幂等，两者都没有稳定的业务摘要与修改前后值。

现有 `cloud_incremental_pull` 按 `last_change_seq` 拉取当前实体镜像，客户端也已有清空可重建业务缓存并全量下载云端状态的能力。这些能力可以复用于回滚后的设备重建，但必须先解决旧离线设备继续上传的问题。

实现受以下约束：

- 云端模式以 Supabase 为最终权威，客户端只能使用 anon key 和用户会话，不能引入 `service_role`。
- 同一账号当前只有一个活跃云端工作空间，不存在 SeaTable 那样“恢复为另一个 Base”后切换的工作空间管理界面。
- 凭据保存在系统凭据库；备份、操作日志和镜像均不得复制这些秘密。
- 用户工作树中的 `MainWorkbench.vue` 已有未提交修改，实现时必须增量集成，不能覆盖无关改动。

## Goals / Non-Goals

**Goals:**

- 定义一个独立于 SQLite 物理布局、可跨版本校验和迁移的逻辑备份格式。
- 把同步用的实体变更与用户可读的业务操作历史分离。
- 在同一云端工作空间内原子恢复历史镜像，并通过世代号阻断回滚前的离线写入。
- 复用现有全量拉取和缓存重建路径，避免维护第二套云端数据应用逻辑。
- 让备份、日志与镜像使用同一份实体清单、脱敏策略和规范化序列化规则。

**Non-Goals:**

- 不把备份文件用作持续同步协议，也不直接导入为当前云端工作空间。
- 不恢复 Supabase 账号、设备授权、系统凭据、outbox、冲突、租约或活动采集状态。
- 不实现单条操作反向执行、单个事项局部恢复或任意两个镜像之间的字段级合并。
- 不提供服务端数据库灾备、Supabase 项目级 PITR 或管理员跨用户恢复。

## Decisions

### 1. 使用版本化逻辑归档作为本地备份格式

备份文件扩展名使用 `.timegenie-backup`，物理格式为 ZIP。生产依赖中启用 `zip` 的 deflate 支持。归档内容按固定名称组织：

```text
manifest.json
workspace.json
data/planning.json
data/time-tracking.json
data/reports.json
data/shared-settings.json
checksums.json
```

`manifest.json` 包含 `formatVersion`、应用版本、创建时间、来源模式、来源工作空间的非授权性标识、实体计数、冻结时间和导出种类。业务文件使用规范化 JSON：对象字段固定排序、集合按稳定主键排序、时间统一 UTC 毫秒。`checksums.json` 对每个成员文件保存 SHA-256。

普通备份和设备救援备份使用相同容器；救援备份额外标记 `backupKind=stale_epoch_rescue`、来源世代和旧 outbox 摘要，但不携带可重放的设备身份或凭据。

导入使用“两阶段、单事务”流程：先解压到受限临时目录并完成全部校验，再创建导入前安全备份，最后打开 SQLite 事务按外键依赖顺序替换业务数据。恢复后创建新的本地工作空间和设备关联，业务实体 ID 保留，所有 `workspace_id` 重映射到新工作空间。活动计时在导出冻结时间结算到该时刻并恢复为暂停；未归属采集不自动继续。

选择逻辑归档而不是直接复制 SQLite，是因为 SQLite 文件包含 schema 版本、本机设备、同步队列和设备配置，跨版本恢复也无法安全选择性排除秘密。未选择 SQL dump，是为了避免执行归档内任意 SQL并保持格式可验证。

### 2. 建立共享的实体目录与脱敏规则

Rust 侧新增数据保护模块，集中声明每个可备份/可镜像实体的：

- 导出查询和稳定排序键；
- 导入依赖顺序与工作空间重映射规则；
- 可进入操作日志的显示名称和字段白名单；
- 明确排除的字段与设备专属表。

云端 SQL 维护语义等价的镜像实体组和字段白名单。实现测试必须比较两侧实体组清单，避免新增业务表后只进入同步、不进入备份。

`integration_configs.secret_ref`、Supabase 配置、`device_settings`、`devices`、`local_sync_state`、`sync_outbox`、`cloud_deferred_entities`、凭据库内容、`device_hooks` 及执行记录默认排除。首期不导出自动化规则，因为其本地程序、参数和 URI 可能携带秘密或在其他设备上产生危险行为。

### 3. 新建语义化操作历史表，不扩充 `workspace_changes`

云端新增：

```text
cloud_operation_logs
  id, workspace_id, operation_id, operation_type, source
  actor_device_id, actor_device_name_snapshot
  summary, affected_entity_count, occurred_at, expires_at
  related_snapshot_id, related_safety_snapshot_id

cloud_operation_log_items
  id, log_id, entity_type, entity_id
  entity_label_snapshot, action
  before_json, after_json, diff_json
```

顶层表对 `(workspace_id, operation_id)` 建唯一约束。业务 RPC 在同一数据库事务中完成数据修改、`processed_operations` 幂等记录和操作日志写入。日志辅助函数接收业务动作类型、来源和受影响对象，在服务端按照字段白名单生成差异；因此操作失败不会留下“成功”历史，相同操作 ID 重试也不会重复。

通用 `cloud_apply_patch` 在更新前读取允许记录的旧实体，更新后读取新实体。计时切换、批量计划、实际工时调整、迁移和恢复等复合 RPC 显式提供聚合项，确保一次业务动作只产生一个顶层日志。设备名称在写入时保存快照，设备撤销后仍可识别。

`workspace_changes` 保持精简并继续只服务同步；不在其中保存 before/after，避免 Realtime 表快速膨胀，也不让日志保留策略影响同步补拉。

初始保留策略设为 90 天，可通过共享工作空间设置调整；清理函数仅删除日志及其明细。界面明确显示保留期限。

### 4. 云端镜像保存在 PostgreSQL 分块表中

新增：

```text
workspace_snapshots
  id, workspace_id, snapshot_type, name, description
  snapshot_day, status, format_version, base_change_seq
  workspace_epoch, entity_counts_json, content_hash
  created_by_device_id, created_at, expires_at, protected_at

workspace_snapshot_chunks
  snapshot_id, entity_group, chunk_index
  row_count, payload_json, content_hash
```

镜像内容分为 `planning`、`time_tracking`、`reports`、`shared_settings`，每块按稳定主键排序并限制行数与序列化大小。创建 RPC 在同一 PostgreSQL 事务与一致性快照中读取所有实体、写入 chunks、计算组级和整体 SHA-256，最后把元数据从 `creating` 标记为 `ready`；异常事务会整体回滚，不暴露半成品。

首期使用 PostgreSQL JSONB 分块而不是 Supabase Storage：数据仍受工作空间 RLS 和事务保护，不需要管理 bucket、签名 URL及孤儿对象。代价是数据库容量增加，因此实施时增加镜像大小统计和单镜像上限；超过上限时创建失败并提示用户先导出本地备份。未来数据规模证明 PostgreSQL 成本不可接受时，可在保持逻辑格式不变的前提下迁移 chunk 存储。

每日自动镜像采用“有修改时惰性触发”：业务操作成功后，后台请求当日自动镜像；数据库使用 `(workspace_id, snapshot_type, snapshot_day)` 唯一约束保证多设备只创建一份。它比依赖未必启用的 `pg_cron` 更适合现有部署，也满足“发生修改的工作空间每天一份”。手动镜像由用户触发；回滚前安全镜像由恢复 RPC强制创建。

初始保留默认值：自动镜像 30 天；未保护手动镜像最多 20 份；回滚前安全镜像 30 天；用户保护的手动或安全镜像不自动删除。这些值存入共享设置并由服务端限制在合理范围。

### 5. 使用工作空间世代号作为回滚后的写屏障

在 `workspaces` 增加 `workspace_epoch bigint not null default 1`，在本地 `local_sync_state` 保存最近确认的 `workspace_epoch`，在 `sync_outbox` 保存操作创建时的 `workspace_epoch`。所有新客户端云端写 RPC 都携带 `p_workspace_epoch`，服务端在读取或写入业务数据前验证它等于当前值。

为平滑部署，工作空间仍处于 epoch 1 时，旧客户端缺少世代参数可以暂时兼容；第一次回滚把 epoch 增至 2 后，缺少参数和旧值一律返回 `WORKSPACE_EPOCH_STALE`。这样不会在数据库补丁发布时立刻阻断所有旧版本，却能保证任何发生过回滚的工作空间不接受旧客户端写入。界面在用户执行首次回滚前提示其他设备需升级到支持世代隔离的版本。

`cloud_incremental_pull` 增加客户端世代参数和服务端世代返回值。若不匹配，响应 `reset_required=true`，不返回普通增量实体。Realtime 可继续用 `workspace_changes` 作为唤醒信号；恢复事务末尾插入一条专用 `workspace_reset` 变更，客户端看到该类型直接进入世代检查，不把它当成普通实体应用。

### 6. 回滚由服务端恢复锁和单事务控制

新增 `workspace_restore_locks`，以工作空间为唯一键，保存锁 token、持有设备、目标镜像和过期时间。所有云端写 RPC 在事务开始时检查有效恢复锁；只有持锁恢复 RPC 可以继续。锁有短期租约，失败客户端不会永久阻塞工作空间。

回滚分两步：

1. `workspace_restore_prepare` 校验权限、镜像完整性、云端活动计时/未归属采集、当前 epoch 和锁状态，获取恢复锁，在同一锁上下文中创建并校验 `pre_restore` 安全镜像，返回差异摘要、锁 token 和安全镜像 ID。
2. 用户二次确认后调用 `workspace_restore_commit`，携带锁 token、目标镜像 ID、目标镜像哈希、准备阶段 epoch 和明确确认标记。RPC 再次检查全部前置条件，在一个事务内按外键顺序删除/写回可恢复业务表、递增 epoch、清理当前租约、写入恢复操作日志和 reset 变更，最后释放锁。

客户端在 prepare 前检查本机活动计时、outbox 的 pending/sending/failed/conflict；服务器再次检查云端权威状态和当前设备授权。其他离线设备是否持有未上传操作无法提前获知，由 epoch 屏障在恢复后拦截。

选择原地事务恢复而不是创建第二工作空间，是为了保持现有单工作空间产品模型。选择 prepare/commit 而不是一个长交互 RPC，是为了让用户看到目标差异和安全镜像后再确认，同时用有期限锁防止确认期间数据继续变化。

### 7. 复用现有缓存重建路径处理回滚事件

客户端收到 `WORKSPACE_EPOCH_STALE`、`workspace_reset` 或 pull 的 `reset_required` 后：

- 若本机没有旧 epoch outbox，停止协调器，保存新 epoch，清理可重建业务缓存并调用现有全量快照拉取；设备设置、凭据和本地集成输出不删除。
- 若存在旧 epoch outbox，设置 `epoch_blocked` 状态，停止领取和上传队列，不自动覆盖本机业务数据。界面提供“导出设备救援备份”和“放弃旧修改并重新下载”两个显式动作。
- 放弃操作只删除已解析为旧 epoch 的 outbox、冲突和 deferred 状态，随后全量重建；执行前再次生成本机安全备份。

现有“重置本地数据”是用户主动丢弃 pending/conflict 后重建，不能直接替代世代阻塞流程；新状态必须使用更明确的说明，避免用户误以为旧修改已经上传。

### 8. 设置页采用独立的数据保护分区

在“数据存储”附近新增“数据保护”设置分区：

- 本地备份：导出、选择文件预览、导入、安全备份结果。
- 云端操作记录：分页筛选列表与只读详情。
- 历史镜像：镜像列表、保护状态、创建、差异预览和回滚。
- 世代阻塞横幅：救援备份、放弃旧修改并重新下载。

破坏性动作使用独立对话框展示影响数量、不会恢复的内容、安全镜像 ID 和当前同步门槛。回滚确认要求用户输入目标镜像名称，避免误点。导入和回滚期间禁用相关按钮，但不冻结普通只读页面。

## Risks / Trade-offs

- [镜像占用 PostgreSQL 空间并增加写入负载] -> 分组分块、设置单镜像大小上限、只在当日发生修改时自动创建，并提供保留清理与容量统计。
- [新增业务表后遗漏备份或镜像] -> 共享实体目录、清单一致性测试，并把新同步实体加入清单检查作为迁移验收项。
- [before/after 日志可能记录敏感内容] -> 服务端字段白名单优先于通用 JSON 差异，集成配置只记录安全摘要，增加敏感关键词与凭据值泄露测试。
- [恢复大量数据的单事务耗时和锁竞争] -> 创建分块、恢复前校验大小、限制镜像规模、使用工作空间级恢复锁并对普通写快速返回可识别错误。
- [旧客户端不知道 epoch] -> epoch 1 兼容缺省参数；首次回滚前提示升级；epoch 大于 1 后服务端强制拒绝缺省或旧值。
- [回滚时其他设备刚好提交写入] -> prepare 后所有写 RPC 检查恢复锁；commit 再校验锁 token、epoch 和目标哈希。
- [设备救援备份不能自动合并回滚后的云端] -> 明确标记为非云端一致文件，首期只支持离线查看/本地恢复后人工迁移，不承诺冲突合并。
- [运行计时的备份语义不清] -> 用备份冻结时间截断并恢复为暂停，禁止恢复后按墙钟补算。

## Migration Plan

1. 先部署 Supabase 追加式 schema：操作日志、镜像、恢复锁、epoch 字段、RLS 和只读查询 RPC；现有工作空间 epoch 初始化为 1。该阶段不开放回滚入口。
2. 发布支持 epoch、备份和新云端 API 的桌面端；新建 outbox 记录开始携带 epoch，pull 保存服务器 epoch。
3. 验证新旧客户端在 epoch 1 的兼容性、RLS、日志脱敏、镜像大小和备份往返。
4. 再开放历史镜像与回滚 UI；首次回滚前检查当前账号设备版本并显示兼容提示。
5. 回滚桌面版本时保留新增表与 epoch 数据；旧客户端在未回滚工作空间可继续有限使用，在 epoch 大于 1 的工作空间会被服务端拒绝写入，防止数据破坏。
6. 回滚 Supabase 补丁时不得降低已经大于 1 的 epoch 或删除镜像/日志数据；应先关闭新入口，再部署只读兼容函数。
