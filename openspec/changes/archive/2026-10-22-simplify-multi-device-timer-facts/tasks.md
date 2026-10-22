# Tasks

## 1. 服务端：计时写入取消写入权门槛

- [x] 1.1 新增版本化补丁 `supabase/20261018f_timegenie_timer_fact_convergence.sql`，移除 `apply_calendar_time_entry` 的租约校验，使无租约设备也走既有并发校验路径；验证：`npm run test:supabase:schema` 通过，且用例覆盖“未持有租约的写入被接受”
- [x] 1.2 把 `tracking_lease_takeover_for_timer_write` 降级为无副作用兼容实现，保留签名与 `authenticated` 权限但不再切换租约持有者；验证：schema 测试断言旧调用返回成功且租约持有者不变
- [x] 1.3 结束事实的边界在服务端收敛到 `[started_at, 服务端接收时刻]`，并按收敛后的边界重算时长、分段与新的未归属起点；验证：集成用例分别覆盖“客户端时钟超前”与“离线后补交”两种场景
- [x] 1.4 同步 `supabase/schema.sql` 与 `supabase/schema_integration_test.sql`，使权威 schema 与补丁一致；验证：`npm run test:supabase:schema` 全绿
- [x] 1.5 更新 `scripts/verify-supabase-schema.mjs` 的断言（新补丁存在、权限正确、不再要求写入前持有租约）；验证：脚本运行后无缺失或多余断言
- [x] 1.6 更新 `docs/` 中计时同步权威相关说明，写明取消写入权门槛与租约收窄后的职责；验证：文档描述与补丁实际行为逐条一致

## 2. 客户端：移除等待与抢占

- [x] 2.1 移除推送计时操作前的 `take_over_tracking_lease_for_timer_write` 调用；验证：`rg -n "take_over_tracking_lease_for_timer_write" src-tauri/src` 无残留引用，且 `cargo test --manifest-path src-tauri/Cargo.toml` 通过
- [x] 2.2 删除 `reconcile_timer_sync_context` 的 `LEASE_NOT_HELD`/`lease_wait` 分支，并清理 `src-ui/src/services/timerSyncRecoveryPresentation.ts` 中的等待写入权文案；验证：`npm run typecheck` 通过且 `rg -n "lease_wait|等待计时写入权" src-tauri/src src-ui/src` 无结果
- [x] 2.3 收窄租约用途：`timer_write_lease_context` 与租约获取返回的 `authoritativeTimer` 退出计时路径，仅保留未归属跨日协调；验证：双设备用例中不持有租约的设备可直接完成开始与结束写入
- [x] 2.4 在 `src-tauri/src/cloud_e2e.rs` 补充“另一台设备持有租约时写入立即被接受”的用例；验证：新增用例单独运行通过

## 3. 客户端：跟随收敛与提示

- [x] 3.1 在计时恢复分类中实现按意图判定的跟随逻辑（开始、暂停、继续、结束、连续性关闭、跨日切片）；验证：四类意图各有对应用例，覆盖“云端已生效”与“云端缺失实体”两种结果
- [x] 3.2 跟随时删除本机该实体操作链及其依赖操作，并采用云端权威镜像覆盖本地记录、分段与分配；验证：用例断言本地无残留待同步操作且字段与云端一致
- [x] 3.3 同步工作线程在跟随时发出跟随事件，主窗口显示可自动消失的非阻塞提示并提供跳转入口；验证：双设备手测出现提示且不阻断操作，`npm run typecheck` 通过
- [x] 3.4 确认非计时对象仍走既有冲突流程；验证：现有冲突用例保持通过，`cargo test --manifest-path src-tauri/Cargo.toml` 无回归
- [x] 3.5 实现历史计时冲突的一次性重分类，仅作用于计时实体且可重复执行；验证：构造 `competing_edit` 计时冲突后触发同步，冲突数归零且本地记录等于云端权威结果
- [x] 3.6 更新计时同步相关文档，说明“先到云端生效 + 后到跟随 + 轻提示”与可修正入口；验证：文档步骤与实际界面行为一致

## 4. 前端：未归属时间显示基准

- [x] 4.1 将 `src-ui/src/store.ts` 的未归属显示改为“权威时长 + 本机经过时间”推进；验证：显示值每秒平滑增长，重新拉取时校正不超过一秒
- [x] 4.2 阈值判断与展示的处理分钟改用同一显示基准；验证：跨越阈值时提示与显示秒数一致，无自相矛盾状态
- [x] 4.3 在系统时钟被人为调整数分钟的设备上验证显示一致性；验证：两台设备同时显示同一未归属值，差值不超过拉取延迟，并记录实测数据
- [x] 4.4 运行前端既有专项校验确认无回归；验证：`npm run typecheck`、`npm run test:allocation-math`、`npm run test:completion-feedback` 全部通过

## 5. 端到端验收

- [x] 5.1 双设备计时端到端：A 在线续租期间 B 开始与结束计时均立即同步，界面不出现等待写入权；验证：`cargo test --manifest-path src-tauri/Cargo.toml cloud_e2e` 通过并记录队列状态
- [x] 5.2 双设备竞争收敛端到端：两台设备分别结束同一段计时，先到结果生效、另一台跟随并提示，冲突数为零；验证：记录双方结束时间、最终时长、队列与冲突计数
- [x] 5.3 服务端部署与验证：先备份数据库，部署 `20261018f` 补丁，复查函数权限与 PostgREST 健康；验证：记录备份路径、校验和与健康检查结果
- [x] 5.4 变更自检：验证：`openspec validate simplify-multi-device-timer-facts --strict` 通过
