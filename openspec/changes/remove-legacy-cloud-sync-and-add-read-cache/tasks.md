# Tasks

## 1. 锁定双模式边界

- [x] 1.1 扩展存储边界验证脚本，断言业务后端只允许 `LocalSqlite` 与 `DirectSupabase`，并断言云端业务模块不得引用 outbox、增量拉取或本地云端副本 API；运行 `npm run test:storage-boundaries` 和 `npm run test:direct-cloud-command-boundaries` 验证。
- [x] 1.2 为所有业务领域补充路由测试，证明本地模式不会调用 Supabase、云端模式不会修改本地业务表或旧同步表；运行对应 Rust 存储契约测试验证。
- [x] 1.3 将 `StorageBackend` 删除 `LegacySyncSqlite` 分支并统一未知/旧协议的升级处理，更新相关测试后运行 `cargo test --manifest-path src-tauri/Cargo.toml storage_contract` 验证。

## 2. 停止产生旧同步数据

- [x] 2.1 从 subjects、tasks、today plans、recurring、reports、settings、integrations 等领域模块移除 `enqueue_entity`、deferred enqueue 和 `flush_if_online` 调用，逐域运行 Rust 测试并用 `rg` 验证生产代码不再引用这些 hook。
- [x] 2.2 将计时、未归属时间和实际时间调整的本地路径限定为 SQLite 事务、云端路径限定为 direct mutation，补充成功、失败、幂等重试均不创建 outbox 的测试并运行相关 Rust 测试。
- [x] 2.3 删除前端对旧 `cloud_sync_save`、catch-up、conflict resolve、cache reset 和 protocol upgrade 命令的调用，更新 TypeScript 类型与验证脚本并运行 `npm run typecheck`。

## 3. 安全退役本地旧同步数据

- [x] 3.1 实现升级前旧队列检测和一次性脱敏救援备份，确保有待处理数据时不会抛出 `LEGACY_QUEUE_BLOCKED`、不会上传旧操作；使用含 pending/conflict 队列的迁移测试验证备份生成和继续升级。
- [x] 3.2 新增最终 SQLite 清理 migration，删除 `sync_outbox`、`local_sync_state`、`cloud_deferred_entities`、`legacy_abandoned_outbox`、`processed_operations`、`sync_items`、`sync_runs` 及关联索引，保留本地业务表和设备元数据；运行空库、旧库和本地模式数据保留测试。
- [x] 3.3 从 `database.rs`、`data_protection.rs` 和存储边界清单中删除旧同步运行时表、cloud-consistent 队列判断、epoch-blocked 救援与队列恢复逻辑，保留普通本地备份和显式完整复制；运行数据保护测试验证备份导入导出不回建旧表。
- [x] 3.4 更新数据库迁移文档，说明历史 migration 继续保留、最终清理 migration 的数据处置与救援备份位置，并通过文档验证脚本或人工核对确认。

## 4. 删除旧同步协调器

- [x] 4.1 从 `cloud_sync.rs` 提取 Supabase Realtime 连接、重连和业务域失效通知到独立模块，补充连接重试、change sequence 和 epoch 失效测试并运行相关 Rust/前端验证。
- [x] 4.2 从 `lib.rs` 删除旧后台同步协调器启动和所有旧 sync、conflict、lease、legacy timer/unassigned Tauri 命令注册，运行 `cargo check --manifest-path src-tauri/Cargo.toml` 验证命令表完整。
- [x] 4.3 删除 outbox push/pull、增量 apply、冲突解决、本地云端副本重建、旧租约和旧本地优先计时实现；用 `rg` 验证生产代码不再包含旧入口，并运行 `cargo test --manifest-path src-tauri/Cargo.toml`。
- [x] 4.4 删除或重写 `cloud_e2e.rs` 中依赖离线重试、冲突收敛、旧 epoch 阻塞和 snapshot reset 的测试，保留并通过 direct-v2 双设备、恢复、幂等与 Realtime 测试。

## 5. 精简 Supabase 客户端职责

- [x] 5.1 删除 `supabase.rs` 中旧队列统计、升级预检、drain、quarantine、rescue、auth-resume queue 和 sync-state 初始化逻辑，补充旧本地状态不再阻塞登录或进入云端的测试。
- [x] 5.2 将保留代码按认证与设备、直连查询与 mutation、显式完整复制、快照与恢复拆分，保持公开命令行为不变并运行相关单元测试和 `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`。
- [x] 5.3 验证明示本地→云端和云端→本地仍为带预览与确认的一次性替换，补充复制后两端后续修改互不自动传播的验收测试。

## 6. 退役 Supabase 旧同步协议

- [x] 6.1 新增 Supabase 退役 SQL，撤销或删除旧 patch、增量拉取和协议升级 RPC 的客户端权限，并确保旧调用返回 `LEGACY_SYNC_UNSUPPORTED` 或函数不存在且不会修改业务数据；运行 schema 集成测试验证。
- [x] 6.2 将现有和新建 workspace 收敛为直连语义，移除客户端对 `sync_v1/direct_v2` 升级状态的依赖；运行 fresh schema 和既有 schema 升级测试验证。
- [ ] 6.3 从 `schema.sql`、schema 验证脚本和最终兼容阶段中删除旧函数、grant、仅旧同步使用的触发器与协议字段，同时保留 direct mutation、幂等、workspace epoch、change sequence、快照和恢复能力；运行 `npm run test:supabase:schema`。
- [x] 6.4 更新 Supabase 部署说明，记录 SQL 先行/同步发布顺序、旧客户端不再受支持以及禁止恢复旧写权限的回滚约束，并核对文档中的执行命令。

## 7. 上线隔离的持久只读缓存

- [x] 7.1 新建独立 `cloud-read-cache.sqlite3` 管理模块，实现按账号、workspace、domain 和 query key 读取/保存查询快照及 schema version 校验；用单元测试验证它不访问主业务数据库且无实体 mutation/outbox API。
- [x] 7.2 实现缓存 payload 脱敏、64 MiB/30 天保留限制、最近使用清理和损坏自动重建，测试确认 token、认证头、集成密钥不会落盘，损坏文件不会阻止应用启动。
- [ ] 7.3 将直连查询接入 stale-while-revalidate：启动先读匹配快照、在线立即查询权威数据、成功后异步更新快照，补充冷启动、在线替换、缓存写失败不影响查询成功的测试。
- [x] 7.4 将 mutation 成功、Realtime domain 通知、workspace epoch 变化和云端恢复接入缓存失效，测试确认只刷新受影响域且 epoch 变化会清空整个 workspace 缓存。
- [x] 7.5 在退出登录、设备撤销、切换账号、切换 workspace 和切换本地模式时清理内存数据并隔离或删除对应持久缓存，测试确认任何身份都不能读取另一身份的快照。
- [x] 7.6 增加“清除云端缓存”操作，测试确认它只删除云端内存/磁盘缓存，不修改本地模式业务数据或 Supabase 数据。

## 8. 精简存储状态与界面

- [x] 8.1 精简 `StorageModeSnapshot`，移除 pending、conflict、sync state、epoch blocked 和 configured protocol 字段，新增或保留连接、认证、可写性、只读原因、缓存来源与缓存时间；更新 Rust 与 TypeScript 契约测试。
- [x] 8.2 删除主界面旧协议升级、冲突处理、手动同步、重新下载本地云端副本和右下角同步提示；运行 UI 验证脚本确认云端正常时不再出现“待同步/冲突/版本升级”。
- [x] 8.3 为离线缓存状态增加明确的“只读缓存”和最后更新时间展示，并在所有 mutation 入口同时以前端状态和后端守卫拒绝保存；运行离线 UI 与后端拒绝测试。
- [x] 8.4 保留 SeaTable 导入的业务重复检测并更新相关文案/选择器，运行 SeaTable 预览测试确认其不会被误删为同步冲突。

## 9. 清理依赖并完成集成验收

- [x] 9.1 删除未使用的 `timegenie: file:` npm 自依赖和旧 cloud-sync presentation/legacy E2E 脚本，运行 `npm install`、`npm run typecheck` 和相关前端测试确认依赖锁一致。
- [x] 9.2 运行本地模式验收，验证应用启动、CRUD、计时、未归属时间、备份恢复在完全无 Supabase 请求时正常。
- [x] 9.3 运行 Supabase 直连验收，验证 CRUD、计时、未归属时间、双设备 Realtime、幂等重试、快照恢复和显式完整复制均不产生本地业务写入或同步队列。
- [x] 9.4 运行缓存验收，验证在线请求预算、冷启动快照、离线只读、账号/workspace 隔离、epoch 恢复失效和清缓存安全性。
- [x] 9.5 运行 `npm run build`、`cargo test --manifest-path src-tauri/Cargo.toml`、`npm run test:supabase:schema` 及所有存储边界验证，并记录任何需要准备测试 Supabase 账号才能执行的外部 E2E。
