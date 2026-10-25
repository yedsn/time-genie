# Proposal

## Why

TimeGenie 的直连云端请求仍由同步 Tauri 命令执行阻塞网络 I/O，并且每次请求重新创建 HTTP 客户端、首次加载重复调用工作台 bootstrap；与此同时，主窗口和悬浮窗共享同一套静态前端依赖。弱网、冷启动或事项数据增长时，这些路径可能造成操作长时间无响应、悬浮窗首次展示变慢和不必要的 CPU、网络及内存开销。

## What Changes

- 云端查询与 mutation MUST NOT 阻塞 Tauri 窗口事件和其他轻量命令的处理；阻塞式网络工作需要异步化或隔离到受控的后台执行资源。
- Supabase HTTP 客户端按有效配置复用连接池，并在配置变化、退出登录或测试隔离边界发生变化时安全失效。
- 工作台初始化合并日历、主体和工作区上下文读取，避免同一轮启动重复调用 `direct_cloud_workbench_bootstrap`，并为主窗口与悬浮窗定义请求预算。
- 主窗口、悬浮窗以及报告、设置、备份等重量页面按实际入口和可见页面延迟加载，悬浮窗不再为未展示功能承担完整主工作台加载成本。
- 建立大事项集的可重复性能基准，验证待办页和计时事项面板在目标数据规模下的加载、筛选与交互响应；只有基准超过阈值时才引入虚拟列表或分段渲染。
- 将与上述路径直接相关的无效比较、历史兼容参数和死代码纳入收敛，但不进行无关的全仓库重构。

## Capabilities

### New Capabilities

- `application-responsiveness`: 定义云端操作期间的应用响应性、启动请求预算、窗口按需加载和大事项集性能验证要求。

### Modified Capabilities

无。项目当前没有已归档主规格，本变更新增独立的应用响应性能力，并保持现有直连云端权威、查询协调和数据一致性语义不变。

## Impact

- Rust/Tauri 云端执行层：`src-tauri/src/supabase/`、业务 command 路由及可能新增的异步/阻塞任务调度边界。
- 云端客户端生命周期：Supabase URL、anon key、会话和测试客户端的缓存与失效逻辑。
- 前端启动协调：`src-ui/src/App.vue`、`src-ui/src/store.ts`、`src-ui/src/views/MainWorkbench.vue` 与悬浮窗入口。
- 前端构建：Vite 分包、异步组件和重量页面加载边界。
- 验证：直连请求预算、并发响应性、HTTP Client 复用、冷启动分包以及 1,000/5,000 条事项基准。
- 不改变业务数据模型、Supabase RPC 业务语义、SQLite 本地模式或用户可见的数据一致性规则。
