# Design

## Context

见 `proposal.md` 与 `specs/direct-cloud-interaction-performance/spec.md`。当前直连路径的普通数据库操作并非主要瓶颈：生产统计中事项更新平均约 5.85ms、事项创建约 5.99ms、事项列表约 8.61ms、今日概览约 4.71ms。客户端却把一次保存扩展成多组权威查询：`loadWorkspaceData()` 同时加载主体、全部事项和今日事项，`loadTimeData()` 同时加载时间记录和当前计时，许多 mutation 随后还加载报告、今日计划和今日概览。

此外，前端 mutation 适配器成功后主动发出 `work-data-changed`，Supabase `workspace_changes` 又通过 Realtime 向同一进程发布领域失效；页面调度器虽然已有 80ms 合并窗口，但不知道 mutation 权威响应已经覆盖哪些数据，因此仍可能再次重查。`startClock()` 还每 5 秒串行执行计时加载与今日概览，和保存及 Realtime 刷新互相叠加。

现有约束是 Supabase 必须继续作为云端模式唯一业务权威；Vue 状态可以作为当前进程内展示与查询缓存，但不得落成 SQLite 云端业务副本，也不得支持离线 mutation。

## Goals / Non-Goals

**Goals:**

- 让 mutation 成功边界只等待云端写入和当前视图所需的权威结果，不等待无关派生查询。
- 让来自 mutation、前端窗口、Realtime、焦点恢复和兜底校准的刷新经过同一个领域调度器并可证明去重。
- 复用进行中的等价查询，并通过失效序号避免旧响应覆盖新状态。
- 在不高频查询云端的情况下保持活动计时秒数连续显示和跨设备及时收敛。
- 建立操作级请求次数与耗时观测，使性能回归可以自动验证。

**Non-Goals:**

- 不重新引入 SQLite 云端业务缓存、outbox、离线编辑或双向同步。
- 不取消 expected version、workspace epoch、设备授权或 Realtime。
- 不要求首期重写全部 Supabase RPC；优先消费现有权威响应。
- 不承诺所有统计值在 mutation 返回的同一帧同步完成；允许短暂显示旧统计并后台校准。
- 不用“取消所有刷新”换取速度；响应未覆盖或确有远端新变化时仍必须查询。

## Decisions

### 1. 使用领域查询协调器统一缓存、失效和单航班请求

在前端状态层建立统一的领域查询状态，建议按 `workspace`、`todayPlan:<subjectId>:<date>`、`time:<date>`、`timeHistory:<page>`、`reports:<filter>`、`unassigned`、`todayOverview:<subjectId>:<date>` 等查询键管理：

```text
data/loading metadata
loadedAt
appliedChangeSeq
staleSeq
inFlight promise
refreshAgain
```

加载入口不再直接无条件创建请求：未失效且满足新鲜度要求时返回当前内存结果；已有相同 `inFlight` 时复用；请求期间发生更高序号失效时设置 `refreshAgain`，请求完成后至多追加一次查询。只有较新的请求序号、workspace epoch 和变化序号可以应用结果。

不采用为每个 store 方法各自添加 debounce，因为它无法跨 Realtime、焦点恢复和 mutation 后刷新合并，也容易产生不同页面各自维护的新鲜度规则。协调器只管理请求生命周期和领域元数据，现有业务数组和 ref 仍是界面状态来源，避免一次性替换整个 store。

### 2. mutation 返回“权威补丁 + 失效领域”，保存路径不再主动全量重查

已有 mutation 多数返回实体或复合操作结果。前端为每类 mutation 定义应用器：更新/插入/删除对应实体、修复临时标识引用、更新版本和已直接返回的关联状态。应用成功后记录本机操作上下文，包括 operation/client request ID（若有）、响应 change sequence、覆盖的查询键/实体以及仍需失效的派生领域。

保存 Promise 在权威响应完成并应用当前视图后即结束。今日概览、报告计数、其他页面列表等未被响应覆盖的数据只标记 stale，并交给协调器根据可见性后台刷新。对于响应暂不含 change sequence 的 RPC，可短期使用实体版本、设备 ID、完成时间窗口和领域作为保守抑制条件；中期优先统一 mutation envelope，而不是把本机 Realtime 全部忽略。

不采用纯乐观“先返回、云端失败再说”作为通用策略，因为当前云端模式明确要求 mutation 成功提交后才视为成功。输入控件可以立即反映用户编辑，但保存成功提示和正式版本仍以云端返回为准。

### 3. 区分同窗口局部应用、跨窗口通知和 Supabase Realtime

`notifyWorkDataChanged` 不再被当前窗口当成“自己也必须重查”的广播。通知 payload 增加来源窗口/进程上下文、operation ID、change sequence、covered domains/query keys 等非敏感元数据；发送窗口将 mutation 结果应用到本地 store，其他窗口只对自己可见且未覆盖的领域执行刷新。

Supabase Realtime 的 `workspace_changes` 继续携带 change sequence、workspace epoch、领域和 `changed_by_device_id`。协调器按下列顺序判断：

1. epoch 不同：清空进程内业务状态并重新加载；
2. `change_seq <= appliedChangeSeq`：忽略已覆盖通知；
3. 本机 operation 已知且响应覆盖当前查询键：推进序号但不重查；
4. 更高序号或未知远端操作：标记对应领域 stale，并按可见性刷新。

不按 `changed_by_device_id === currentDevice` 直接忽略所有通知，因为同一设备可有多个窗口，且 mutation 响应可能只覆盖部分派生结果。operation/sequence 覆盖判断比设备级忽略更安全。

### 4. 收窄领域依赖图，移除“事项变化就加载一切”

建立显式的 mutation 影响映射，而不是在各函数中手写 `Promise.all`：

| 操作 | 直接应用 | 后台失效 | 默认不刷新 |
|---|---|---|---|
| 事项标题/备注/普通预计 | 当前事项及可见镜像 | 必要的事项列表/今日计划 | 计时、报告 |
| 今日预计/计划成员 | 当前事项或计划 | 今日概览 | 报告、计时历史 |
| 完成/重开事项 | 当前事项状态 | 完成列表、今日概览；报告仅在可见/依赖时 | 无关设置 |
| 计时开始/暂停/继续 | 当前活动计时 | 必要的事项执行状态、概览 | 报告列表 |
| 计时停止/分配 | 停止结果与确认上下文 | 事项、未归属、概览、可见历史 | 无关报告/设置 |
| 报告编辑 | 当前报告 | 报告列表摘要 | 工作台、计时 |

影响映射由服务层返回的 domains 和前端产品依赖共同确定。不可见领域保持 stale，不在后台无限累积具体事件；重新进入页面时以一次权威查询收敛。

### 5. 活动计时采用本地显示时钟 + 事件驱动校准

保留现有每秒更新 `now` 的显示时钟，使用服务器返回的 `durationSeconds`、`syncedAt/serverTime`、状态和版本计算当前显示。删除每 5 秒执行 `loadTimeData().then(loadTodayOverview())` 的业务轮询。

权威校准触发点包括：

- 计时 mutation 成功；
- 收到更高 change sequence 的 time 领域 Realtime；
- 主窗口/悬浮窗从后台或系统休眠恢复，且超过新鲜度阈值；
- 跨工作日边界；
- Realtime 不健康或长时间无权威确认时的低频兜底（建议 30～60 秒，最终值可配置并通过测试使用虚拟时钟验证）。

窗口焦点、visibility 和 5 秒唤醒心跳共用同一 catch-up single-flight，避免恢复时同时执行多次校准。今日概览不跟随显示秒数轮询，只在影响其汇总的事实变化或页面显式刷新时更新。

### 6. 保留精确失败恢复，不让后台刷新反向阻塞操作

mutation 明确失败时，应用器恢复该操作前的局部快照，或对受影响的最小查询键执行权威加载。`VERSION_CONFLICT`、`WORKSPACE_EPOCH_STALE`、`DEVICE_REVOKED` 等继续走结构化处理：并发错误保留可复制输入并重载记录，epoch 错误清空旧世代内存，授权错误进入只读。

mutation 已成功但后续后台刷新失败时，不把本次保存显示为失败；保留权威 mutation 结果、标记派生领域 stale，并依据现有云端状态机展示连接/刷新提示。这样可以区分“数据没有保存”和“附加统计暂时没有刷新”。

### 7. 扩展可观测性到操作级，但不记录业务内容

现有 Rust 指标只有聚合 query 延迟和 mutation 次数，mutation 没有耗时，前端也无法看到一次用户操作触发多少查询。新增非敏感性能事件/聚合计数：

- operation type 与匿名 correlation ID；
- mutation 总耗时、网络/RPC 耗时、客户端 apply 耗时；
- query key/domain、请求原因、是否复用、是否被合并/抑制；
- 单次操作关联的权威查询数量；
- Realtime source、change sequence 覆盖结果；
- 错误代码，不含请求正文、实体标题、备注、报告内容或 token。

发布指标继续以内存聚合为主；自动化测试通过可注入计数器或 mock invoke 断言请求预算，避免把性能正确性只留给人工体感。典型预算以“一个 mutation + 响应未覆盖的最小查询”为原则，而不是承诺不受网络环境影响的固定毫秒 SLA。

### 8. 优化顺序先客户端请求放大，再按证据处理 SQL

当前数据库统计显示普通 CRUD 和主要查询都在低毫秒级，因此首期不新增索引或重写 SQL。先建立基线、消除重复调用并加入操作级观测；若优化后某 RPC 的服务端 P95/P99 或响应体积仍异常，再以 `EXPLAIN (ANALYZE, BUFFERS)` 和真实数据量单独优化。

这避免为了客户端重复请求问题盲目增加数据库复杂度。计时 start/stop 的数据库时间显著高于普通事项操作，应在请求放大修复后作为独立热点复测，但仍不改变本变更的前端收敛架构。

## Risks / Trade-offs

- [局部应用漏掉派生字段，短时间显示不一致] → 为每类 mutation 明确直接覆盖与失效领域，未知影响采取最小权威刷新，并用双设备和复合操作测试覆盖。
- [错误抑制本机 Realtime 导致遗漏其他设备紧随其后的修改] → 以 operation/实体版本/change sequence 覆盖判断，不按设备 ID 粗暴忽略；更高序号始终进入校准。
- [进程内缓存让用户看到短暂旧统计] → 展示保存成功与统计刷新分离，当前可见关键统计后台刷新；不可见页面进入时强制检查 stale。
- [协调器状态复杂并造成刷新饥饿] → 每个查询键保留 `refreshAgain`，请求完成后最多追加一次；设置低频 catch-up 和可观测 stale 年龄。
- [多个窗口各自维护内存状态] → mutation 通过跨窗口通知传播失效/覆盖元数据；每个窗口独立 single-flight，不共享可变业务对象。
- [取消 5 秒轮询后 Realtime 故障时变化发现变慢] → Realtime 健康状态、窗口恢复和 30～60 秒低频兜底共同校准，不依赖单一通知渠道。
- [后台刷新失败被忽略，页面长期陈旧] → 保持 stale、记录错误和最近成功时间，连接恢复或页面进入时重试；关键权威 mutation 结果不回滚。

## Migration Plan

1. 先为现有典型保存和计时链路增加只读请求计数测试及操作级观测，记录优化前基线。
2. 引入领域查询协调器并让加载函数支持查询键 single-flight、失效序号和 `refreshAgain`，初期保持现有调用点以验证等价行为。
3. 逐类改造事项、今日计划、报告和计时 mutation：应用权威响应，删除保存路径内的无条件全量 `Promise.all`，改为声明式失效。
4. 扩充 mutation/Realtime 元数据和跨窗口通知，使已覆盖的本机事件可以推进序号而不重复查询；使用两个设备验证更高远端序号仍会收敛。
5. 删除 5 秒计时全量轮询，启用本地显示时钟、窗口恢复 single-flight 和低频兜底；验证休眠、跨日、Realtime 断线和计时接管。
6. 对工作台激活、复制、模式切换和各页面显式刷新做请求预算回归，确保用户明确刷新仍可绕过新鲜度并获取权威数据。
7. 发布后观察操作级查询数量、错误率、stale 年龄和 RPC 延迟；只有指标显示具体 RPC 仍慢时再追加 SQL/索引优化。

回滚时可暂时恢复旧的 mutation 后领域刷新和较高频校准，但 MUST 保持 Supabase 单一业务权威，不得恢复 SQLite 云端业务副本或旧同步队列。领域协调器和观测代码可以保留，因为它们不改变数据语义。
