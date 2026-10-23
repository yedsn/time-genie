# Proposal

## Why

TimeGenie 切换到 Supabase 直连后，普通事项保存虽然在数据库中通常只需数毫秒，但客户端会在写入成功后重复加载工作台、计时、报告、今日计划和今日概览，并可能再被前端事件、Realtime 通知及 5 秒轮询触发同类查询，导致一次轻量修改被放大为多轮网络往返并阻塞交互。需要在保持 Supabase 为云端唯一业务权威的前提下，通过进程内局部更新、失效合并和后台校准恢复接近本地操作的响应速度，而不是重新引入 SQLite 业务副本与双向同步。

## What Changes

- 云端 mutation 成功后优先使用其权威返回值局部更新 Vue 进程内状态，用户操作完成状态 MUST NOT 等待无关领域的全量重查。
- 建立按业务领域管理的进程内查询状态，记录已加载数据、加载时间、失效状态和进行中的请求；该状态不持久化、不支持离线编辑，也不形成新的同步来源。
- 将前端主动变更事件、Supabase Realtime 通知、窗口恢复和后台校准产生的刷新请求统一合并、去重，并避免同一笔本机 mutation 立即触发重复权威查询。
- 将工作台、今日计划、计时、报告和今日概览的刷新范围收敛到实际受影响领域；当前不可见领域只标记失效，进入相关页面时再加载。
- 移除活动计时期间每 5 秒执行“计时数据 + 今日概览”全量查询的依赖，改为使用云端返回的计时基准在本地推导显示时间，并通过 Realtime、窗口激活及低频兜底进行权威校准。
- 为保存链路、领域刷新和云端 RPC 增加可验证的请求次数与分段耗时观测，确保能够区分数据库执行、网络往返和客户端状态更新成本。
- 保持断网只读、实体 expected version、workspace epoch、Realtime 失效通知和 Supabase 单一业务权威语义不变；不新增离线写入、SQLite 云端业务缓存、待同步队列或冲突合并。

## Capabilities

### New Capabilities

- `direct-cloud-interaction-performance`: 定义直连云端模式下的 mutation 局部应用、进程内查询缓存、刷新去重、Realtime 本机事件抑制、计时低频校准及性能可观测行为。

### Modified Capabilities

无。项目当前没有已归档主规格；本变更新增独立能力，并保持进行中的 `direct-cloud-storage` 所定义的 Supabase 单一权威和非持久业务缓存边界。

## Impact

- Vue 状态与启动协调：`src-ui/src/store.ts`、`src-ui/src/App.vue`、`src-ui/src/services/workDataRefresh.ts` 需要统一领域失效、请求合并和后台刷新规则。
- 前端 Tauri 服务：`src-ui/src/services/tauri.ts` 需要避免 mutation 成功后同时发出本机刷新事件和依赖 Realtime 再次刷新，并为本机操作关联稳定标识或等价抑制上下文。
- 工作台与组件：`src-ui/src/views/MainWorkbench.vue` 及事项、计时相关组件需要将“保存成功”与“附加统计刷新完成”分离，保留局部失败回滚和并发错误重载。
- Rust 云端适配器：`src-tauri/src/supabase.rs`、`src-tauri/src/cloud_sync.rs`、`src-tauri/src/direct_cloud_realtime.rs` 和 `src-tauri/src/direct_cloud_observability.rs` 需要补足 mutation/查询耗时、来源识别及必要的响应元数据，但不改变 Supabase 权威存储模型。
- Supabase：优先复用现有 RPC 返回的权威实体和变化领域；只有在现有响应不足以支持局部校准或本机通知识别时才追加兼容字段或轻量 RPC，不引入本地同步结构。
- 验证：需要覆盖典型事项保存、备注与预计修改、今日计划、计时开始/暂停/继续/停止、双设备远端变化、Realtime 重复/乱序、窗口恢复和断网只读，并对一次用户操作的请求数量设置回归断言。
