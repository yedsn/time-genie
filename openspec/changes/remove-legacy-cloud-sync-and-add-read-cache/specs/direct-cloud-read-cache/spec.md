# Spec Delta

## Purpose

定义 Supabase 直连模式的快速读取和离线只读体验，使缓存能够降低重复请求与启动等待，同时保证缓存永远不会演变为可写业务副本或重新引入双向同步。

## ADDED Requirements

### Requirement: 云端读取必须优先复用进程内缓存
系统 SHALL 对直连云端查询进行请求合并、新鲜度复用和按业务域失效，在缓存仍有效时避免重复请求 Supabase。

#### Scenario: 相同查询并发发生
- **WHEN** 同一缓存键的多个读取在首个请求完成前发生
- **THEN** 系统复用同一个进行中的云端请求，而不是重复发送等价请求

#### Scenario: 查询结果仍处于新鲜期
- **WHEN** 页面再次请求尚未失效且未超过新鲜期的数据
- **THEN** 系统直接使用进程内结果并跳过重复云端查询

### Requirement: 持久缓存必须是隔离的只读快照
系统 SHALL 将可跨进程保留的云端缓存与本地模式业务表隔离，并以 workspace、账号、业务域、查询键、缓存结构版本、workspace epoch、change sequence 和获取时间标识每个快照。该缓存 MUST 不提供通用业务写入接口。

#### Scenario: 成功读取云端数据
- **WHEN** Supabase 权威查询成功返回适合缓存的数据
- **THEN** 系统可以异步更新对应只读快照，且缓存写入失败不得改变云端查询的成功结果

#### Scenario: 切换到本地模式
- **WHEN** 用户从云端模式切换为本地模式
- **THEN** 本地业务查询只读取本地业务 SQLite，不得读取云端只读缓存作为本地数据

### Requirement: 缓存不得参与 mutation 或离线写入
所有云端 mutation MUST 直接提交 Supabase；系统不得将失败或离线 mutation 写入持久缓存、不得生成 outbox，也不得把缓存结果当作 mutation 已成功的证明。

#### Scenario: 离线时尝试编辑
- **WHEN** 用户只持有云端只读缓存且 Supabase 当前不可访问
- **THEN** 系统禁止或拒绝需要保存的操作，并清楚说明云端数据当前只读

#### Scenario: mutation 成功
- **WHEN** Supabase mutation 返回成功结果
- **THEN** 系统更新相关进程内状态，并根据返回数据更新或失效持久缓存；缓存维护失败不得将成功操作改报为失败

### Requirement: 启动和离线时可以展示标记过的缓存
云端模式启动时，系统 MAY 先展示匹配当前账号和 workspace 的持久缓存，同时立即尝试获取权威数据；无法访问 Supabase 时，系统 SHALL 仅以只读方式展示缓存，并显示缓存来源和最后更新时间。

#### Scenario: 启动时缓存可用且网络正常
- **WHEN** 应用启动时存在身份、workspace 和缓存结构版本均匹配的快照
- **THEN** 系统可以先展示快照，再以 Supabase 查询结果替换或确认该数据

#### Scenario: 启动时网络不可用
- **WHEN** 应用无法访问 Supabase但存在有效身份范围内的快照
- **THEN** 系统展示只读缓存及最后更新时间，不允许用户误认为数据已实时连接或可保存

#### Scenario: 无缓存且网络不可用
- **WHEN** 应用无法访问 Supabase且没有匹配缓存
- **THEN** 系统显示无可用云端数据的离线状态，不得回退读取旧本地业务副本

### Requirement: 缓存必须及时失效并防止跨身份泄漏
系统 MUST 在 Realtime 变更通知、本机 mutation、workspace epoch 变化、云端完整恢复、退出登录、切换账号或切换 workspace 时使受影响缓存失效；不同账号与 workspace 的缓存不得交叉返回。

#### Scenario: 收到业务域变更通知
- **WHEN** Realtime 通知某业务域在更新后的 change sequence 发生变化
- **THEN** 系统将该业务域相关缓存标记为过期，并在需要展示时重新查询权威数据

#### Scenario: 退出登录或切换账号
- **WHEN** 用户退出登录或改用另一个账号
- **THEN** 系统立即清空进程内业务数据，且新会话不得读取上一账号的持久缓存

#### Scenario: workspace epoch 改变
- **WHEN** 客户端发现缓存记录的 workspace epoch 与服务端不一致
- **THEN** 系统丢弃该 workspace 的旧缓存并执行权威重载

### Requirement: 缓存内容必须受控且可安全丢弃
持久缓存 MUST 排除访问令牌、刷新令牌、集成密钥及其他认证秘密，并 MUST 具有容量或保留期限制、结构版本校验和损坏恢复能力。删除缓存不得删除本地模式数据或云端权威数据。

#### Scenario: 缓存结构版本不兼容或内容损坏
- **WHEN** 系统无法验证或解析持久缓存
- **THEN** 系统安全删除或忽略该缓存并重新查询 Supabase，不阻止应用启动

#### Scenario: 用户重置云端缓存
- **WHEN** 用户执行“清除云端缓存”
- **THEN** 系统只删除进程内和持久化云端快照，不删除 Supabase 数据或本地模式业务数据

