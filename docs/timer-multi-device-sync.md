# Supabase 直连模式的多设备计时

TimeGenie 的 `direct_v2` 云端模式把 Supabase 作为计时、未归属时间和实际工时的唯一权威数据源。主窗口、悬浮窗与托盘都通过同一套 Rust 存储路由读取和修改云端状态；客户端不会先写 SQLite，也不会创建 `sync_outbox`、离线待提交操作或后台同步冲突。

本地存储模式仍只使用 SQLite，可完全离线运行。旧 `sync_v1` 工作空间在协议升级前继续保持兼容行为。

## 在线要求与权威时间

- `direct_v2` 的开始、暂停、继续、结束、切换事项、恢复、接管、手工补录、时间修正、归类、工时分配、未归属处理和实际工时调整都必须在线提交成功。
- 云端不可达、会话无法确认或设备已撤销时，计时修改必须失败或进入只读状态；客户端不得把操作转成本地待同步修改。
- 界面使用 RPC 返回的 `startedAt`、`durationSeconds`、`transitionAt`、`observedAt`、`serverTime` 和权威版本，不使用本机业务表中的历史副本。
- 读取活动计时不会提升普通计时版本；跨日、休眠恢复和连续性中断只通过显式恢复 RPC 改变业务状态。

## 计时 RPC 与请求条件

所有 mutation 都携带公共参数 `p_workspace_id`、`p_device_id`、`p_operation_id`、`p_expected_epoch` 和 `p_payload`。普通查询携带工作空间、设备、当前 workspace epoch；写请求还必须通过设备授权、`direct_v2` 协议和客户端 schema generation 校验。

| 行为 | RPC | `p_payload` 关键字段 |
| --- | --- | --- |
| 读取活动计时 | `direct_cloud_active_timer` | 无业务 payload |
| 开始 | `direct_cloud_timer_start` | `taskId`、`note`、`leaseToken` |
| 暂停 | `direct_cloud_timer_pause` | `entryId`、`expectedVersion`、`leaseToken` |
| 继续 | `direct_cloud_timer_resume` | `entryId`、`expectedVersion`、`leaseToken` |
| 结束 | `direct_cloud_timer_stop` | `entryId`、`expectedVersion`、`createDefaultAllocation`、`leaseToken` |
| 切换事项 | `direct_cloud_timer_transition` | 当前计时版本、目标事项或新事项、`leaseToken` |
| 恢复/跨日 | `direct_cloud_timer_recover` | `entryId`、`expectedVersion`、`leaseToken` |
| 接管 | `direct_cloud_timer_takeover` | `entryId`、`expectedVersion`、新的 `leaseToken` |
| 租约心跳 | `direct_cloud_tracking_heartbeat` | RPC 参数直接携带 `p_lease_token` |

停止计时返回当前记录、同一计时链的全部跨日切片、总有效秒数和总结算分钟数。托盘动作使用相同 RPC 与同一权威返回值，不再走独立的本地计时实现。

## 租约职责与多设备接管

直连计时租约用于协调“哪个设备可以执行当前计时写入”，不是本地副本同步锁。

- 当前持有设备可以暂停、继续、结束、切换和恢复计时。
- 非持有设备提交计时写入时返回 `LEASE_LOST`，响应包含或指示重新查询权威活动计时；失败请求不会留下成功 operation 记录、本地恢复操作或同步冲突。
- 租约过期后，另一台设备可调用 `direct_cloud_timer_takeover`。接管在同一事务中更新租约、处理连续性中断并返回最新活动计时。
- 被替换设备后续心跳或写入继续使用旧 token 时返回 `LEASE_LOST`。
- 同一 operation ID 的接管重试返回第一次的完整结果，不重复切分计时或提升版本。

## 跨日、休眠和响应丢失

`direct_cloud_timer_recover` 以工作空间时区和服务器观察时间为边界：

- 跨越工作日时，云端原子关闭前一天切片并创建当天活动切片；跨越多天时逐日形成完整计时链。
- 同一恢复 operation ID 重试返回相同切片；再次恢复已收敛状态返回 `noOp=true`，不会重复创建跨日记录。
- 连续性中断时，云端关闭中断前分段并从服务器观察时间重新开始，明确标记 `continuityInterrupted=true`。
- 若请求已经提交但 HTTP 响应丢失，Rust 客户端使用完全相同的 operation ID 和 payload 自动重试。服务端从 processed operation 返回首次结果，因此只发生一次业务变化。

## 未归属时间与实际工时

- `direct_cloud_unassigned_state` 和 `direct_cloud_unassigned_ensure` 返回服务器计算的锚点、累计秒数和工作日；多设备共享同一个云端锚点。
- `direct_cloud_unassigned_resolve` 使用 `resolutionType=work|break|discard` 原子处理会话。相同 operation ID 重试不会重复生成时间记录。
- 手工时间、时间修正/归类和实际工时批量调整都由云端事务提交；实际工时批次中的任务映射与调整记录要么全部成功，要么全部回滚。

## 即时并发提示

云端模式不再提供“稍后同步”或人工选择本地/云端版本。客户端按结构化错误立即处理：

| 错误码 | 用户可见行为 |
| --- | --- |
| `VERSION_CONFLICT` | 提示计时或记录已在另一设备变化，并重新查询最新状态 |
| `LEASE_LOST` | 提示当前计时由另一设备持有或已被接管，并加载权威活动计时 |
| `ENTITY_DELETED` | 提示目标记录已删除，关闭当前编辑并刷新 |
| `WORKSPACE_EPOCH_STALE` | 清空当前进程内相关状态并按新 epoch 重查 |
| `DEVICE_REVOKED` | 进入重新认证/重新授权流程，禁止继续写入 |
| `CLIENT_UPGRADE_REQUIRED` | 阻止旧协议写入并提示升级客户端 |

网络错误或超时不能证明操作未提交。客户端会先以稳定 operation ID 幂等重试；仍无法确认时保持只读并通过后续权威查询收敛，不创建本地冲突。

## 验证范围

自动化测试覆盖以下行为：

- 首次响应断开后使用相同 operation ID 重试，请求只产生一次云端结果。
- 多日运行形成正确的逐日切片，重复恢复不增加切片。
- 两台设备争用租约时只有持有者成功，失败设备可重查同一权威计时；租约过期后第二设备可幂等接管。
- 主窗口与托盘使用同一存储路由和服务器时间。
- 上述 `direct_v2` 场景执行前后，本机 SQLite 业务表、`sync_outbox`、`local_sync_state` 和冲突状态不发生业务写入。
