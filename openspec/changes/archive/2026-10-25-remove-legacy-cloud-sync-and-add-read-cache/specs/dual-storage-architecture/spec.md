# Spec Delta

## Purpose

定义 TimeGenie 本地 SQLite 与 Supabase 直连两种互斥的数据存储模式，确保日常业务读写只有一个权威来源，并彻底移除持续同步、冲突队列和本地云端业务副本。

## ADDED Requirements

### Requirement: 系统只提供两种业务存储模式
系统 SHALL 只提供“本地存储”和“Supabase 云端存储”两种业务存储模式，不得保留第三种 SQLite 与 Supabase 持续同步模式。

#### Scenario: 选择本地存储
- **WHEN** 用户选择本地存储模式
- **THEN** 系统只将业务数据读写到本地 SQLite，且不因业务操作调用 Supabase 数据接口

#### Scenario: 选择云端存储
- **WHEN** 用户选择 Supabase 云端存储模式并完成登录
- **THEN** 系统将 Supabase 作为唯一业务权威，并通过云端查询和 mutation 直接读写数据

### Requirement: 本地与云端业务写入必须隔离
系统 MUST 根据当前存储模式将业务 mutation 路由到唯一后端；云端模式不得先写本地业务表、不得生成待同步操作，本地模式不得隐式上传业务数据。

#### Scenario: 云端 mutation 成功
- **WHEN** 云端模式下用户新增、修改或删除业务数据且 Supabase 返回成功
- **THEN** 系统以 Supabase 返回结果作为操作结果，且本地业务表与旧同步队列均不发生对应写入

#### Scenario: 云端 mutation 失败
- **WHEN** 云端模式下 Supabase mutation 因离线、认证失效或服务异常而失败
- **THEN** 系统明确报告未保存，并且不得将操作保存成本地待同步修改

#### Scenario: 本地 mutation 成功
- **WHEN** 本地模式下用户修改业务数据
- **THEN** 系统在本地 SQLite 事务中完成写入，且不得产生任何云端请求或同步队列记录

### Requirement: 云端模式不得暴露同步与冲突概念
系统 SHALL 从云端模式的用户界面和公开状态中移除待同步数量、同步冲突、手动同步、旧世代阻塞和存储协议升级；云端状态只表达连接、认证、可写性、缓存来源和请求失败。

#### Scenario: 云端服务正常
- **WHEN** 用户处于云端模式且认证和 Supabase 服务均可用
- **THEN** 界面不得显示“待同步”“同步冲突”或“处理队列”等状态与操作

#### Scenario: 云端写入并发失败
- **WHEN** Supabase 因唯一约束、幂等键或业务前置条件拒绝 mutation
- **THEN** 系统显示可理解的业务错误或采用服务端权威结果，不得创建本地冲突记录或要求用户选择冲突版本

### Requirement: 存储模式之间只允许显式完整复制
系统 SHALL 保留用户主动确认的“本地完整复制到云端”和“云端完整复制到本地”，复制操作 MUST 是有预览、有目标替换确认的一次性操作，完成后不得建立持续同步关系。

#### Scenario: 本地复制到云端
- **WHEN** 用户预览影响范围并确认用本地数据替换目标云端数据
- **THEN** 系统执行一次完整复制并报告结果，后续本地变化不会自动上传

#### Scenario: 云端复制到本地
- **WHEN** 用户预览影响范围并确认用云端数据替换本地数据
- **THEN** 系统在替换前创建可恢复的本地备份并执行一次完整复制，后续云端变化不会自动写入本地业务表

### Requirement: 旧同步客户端数据必须无阻塞退役
升级后的客户端 MUST 停止上传旧 outbox。若设备存在旧队列或旧云端 SQLite 副本，系统 SHALL 在不阻塞进入云端模式的前提下创建一次本机救援备份或诊断留档，然后以 Supabase 数据为权威并清理旧同步运行时数据。

#### Scenario: 旧队列仍有待处理操作
- **WHEN** 升级时检测到旧同步队列仍有待处理或冲突操作
- **THEN** 系统不得显示 `LEGACY_QUEUE_BLOCKED` 或要求先处理队列，而是留存可追溯备份后放弃队列并直接进入云端模式

#### Scenario: 本地模式用户升级
- **WHEN** 当前用户明确处于本地存储模式
- **THEN** 清理旧同步基础设施时系统保留其本地业务数据并继续使用本地 SQLite

### Requirement: 服务端旧同步接口必须安全退役
Supabase SHALL 拒绝旧客户端继续通过 sync RPC 上传或拉取业务数据；兼容屏障存续期间 MUST 返回明确的不支持错误，并在兼容窗口结束后删除旧同步函数与协议代际状态。

#### Scenario: 旧客户端调用同步 RPC
- **WHEN** 旧客户端尝试调用已退役的 patch、增量拉取或协议升级 RPC
- **THEN** Supabase 拒绝请求且不修改任何业务数据

#### Scenario: 新客户端创建云端工作区
- **WHEN** 新客户端初始化云端工作区
- **THEN** 工作区直接采用云端直连能力，不经历 `sync_v1` 升级流程

### Requirement: 恢复与缓存一致性标记不得成为同步版本
系统 MAY 保留 workspace epoch、change sequence、幂等操作 ID 和服务端记录版本用于恢复、防重复提交与缓存失效，但 MUST 不将其展示为需要用户处理的同步版本或本地冲突。

#### Scenario: 云端工作区完成恢复
- **WHEN** 云端快照恢复改变 workspace epoch 或权威 change sequence
- **THEN** 客户端清空不兼容缓存并重新查询云端，不要求用户解决本地版本冲突

