# Design

## Context

见 proposal.md 与 specs/application-responsiveness/spec.md。当前前端已经具备领域查询 single-flight、失效合并、本地计时时钟和 30～60 秒权威校准，因此本次不重做直连查询协调器。剩余风险主要在更底层和更早的加载阶段：业务 command 以同步函数执行 reqwest blocking 请求，client(database) 每次重新读取配置并创建 HTTP Client；工作区日历和主体查询分别复用完整 bootstrap，导致启动重复网络往返；所有窗口和页面又由同一个静态前端入口装载。

Tauri 同时创建 main 与 hover 两个 WebView，它们各自拥有前端 store，但调用同一 Rust 后端。设计必须保持两个窗口独立展示、跨窗口失效、Supabase 单一权威、断网只读和本地 SQLite 路由不变。

## Goals / Non-Goals

**Goals:**

- 将云端网络等待从同步 command 执行路径隔离，确保控制面和轻量命令保持可调度。
- 在相同配置下复用 HTTP 连接池，同时提供清晰、可测试的客户端失效边界。
- 用一个启动上下文结果满足日历和主体初始化，并为 main/hover 建立自动化请求预算。
- 让 hover、main 及重量页面形成真实构建分包，而不是只在模板中使用 v-if。
- 以可重复基准决定是否实施虚拟列表，避免没有证据地增加 UI 复杂度。

**Non-Goals:**

- 不修改 Supabase RPC 的业务模型、表结构或版本冲突语义。
- 不重新引入 SQLite 云端业务副本、离线 mutation 或双向同步。
- 不把所有 Tauri command 一次性迁移为 async；只覆盖可能执行云端网络 I/O 的业务路由和相关认证/数据保护入口。
- 不承诺固定网络毫秒 SLA；验收关注请求数量、调度隔离、资源复用和本地渲染成本。
- 不在没有基准失败证据时直接引入复杂的虚拟树形拖拽实现。

## Decisions

### 1. 在统一云端执行器中隔离现有 blocking 适配器

首期保留成熟的同步 Supabase 适配器与错误映射，但把可能进入 DirectSupabase 的 command 改为 async，并通过受控的 blocking 执行器调用现有同步函数。执行器设置全局并发上限，通过 Tauri/Tokio 提供的 blocking 调度运行，结果再异步返回前端。

本地 SQLite 快路径可以继续同步执行，或统一经过 async command 但不额外进入阻塞池。路由层需要先读取存储模式，再选择本地闭包或云端后台闭包，避免把所有本地操作无条件移入线程池。

不在首期把整个 Supabase 层改写成 async reqwest，因为当前调用面很大、认证刷新和重试逻辑复杂，全面迁移会显著扩大回归面。隔离层先解决应用控制面被拖住的风险，并为后续逐步异步化留下稳定边界。

### 2. 以配置指纹缓存 SupabaseClient

引入进程内客户端缓存，键至少包含规范化 project URL 与 anon key 指纹；缓存值持有可克隆的 reqwest blocking Client 和非敏感配置。业务请求继续单独读取或刷新会话，不能把 access token 固化到共享 Client。

配置写入成功、退出登录、测试注入上下文切换时显式清理相应缓存；生产缓存使用小而有界的容量，避免用户多次改配置后无限增长。测试使用独立构造器或清理 guard，保证并行测试不会跨用例污染。

不只缓存裸 HTTP Client 而每次继续读取 URL/key，因为那仍保留两次 SQLite 查询；也不把会话放入 Client 缓存，因为 token 刷新会使缓存生命周期和权限状态耦合。

### 3. 建立单一 workbench bootstrap 消费入口

前端初始化新增一个工作区 bootstrap 加载方法，一次取得工作日历、主体、摘要和必要设备上下文，并同时写入 workspaceToday、日期边界及 subjects。随后事项和今日事项查询并行执行，但不再分别通过日历 command 和 subject command 隐式触发 bootstrap。

work_calendar_get_context 与 subject_list 仍保留为独立公开 command，供非启动场景和兼容调用使用；在直连云端下它们可以读取短时进程缓存或轻量 RPC，但主启动路径必须直接消费 bootstrap。bootstrap 查询接入现有 query coordinator，main 窗口同一轮初始化最多一次；hover 不需要完整 bootstrap 时不调用它。

不依赖 1 秒新鲜度偶然去重，因为两个 Rust command 使用不同入口和查询键，无法保证稳定合并。

### 4. hover 使用最小初始化并复用跨窗口事件

store.startClock() 不再让所有视图调用同一个完整 initializeWorkspace。main 执行工作台初始化；hover 只加载计时状态、未归属状态，以及展示当前事项标签所需的最小数据。如果计时响应已经包含 label snapshot，hover 不加载完整事项树；需要切换事项时仍打开 main 的计时面板。

hover 显示事件继续使用 single-flight 刷新计时与未归属状态。来自 main 或 Realtime 的 domain 事件只刷新 hover 可见领域，沿用现有 partitionWorkDataRefreshDomains 行为。

### 5. 使用动态 import 形成窗口和页面分包

App.vue 根据 view 使用异步组件加载 MainWorkbench 或 HoverPanel，确保 hover 初始 chunk 不静态引用 main。MainWorkbench 内的报告、设置相关大组件进一步按页面或设置分区异步加载，并提供轻量 loading fallback。

Vite 保持自动分包优先；仅在自动分包无法稳定隔离 Markdown 编辑器、Element Plus 重量模块时增加少量 manualChunks。构建验证解析 manifest 或产物依赖，断言 hover 入口初始链不包含主工作台或 Markdown 编辑器，并记录主初始 chunk gzip 大小基线。

不直接创建两套完全独立 HTML 应用，因为会复制启动、主题、事件监听和 store 逻辑；先通过路由级动态 import 达到大部分收益。

### 6. 大事项集先基准、后条件性虚拟化

新增可重复的前端性能脚本，生成固定的两级事项树，规模为 1,000 和 5,000 条，覆盖：构造展示行、关键词筛选、打开计时事项面板、DOM 首次稳定及一次局部更新。脚本记录运行环境、样本规模、中位数或 P95 和实际 DOM 行数。

验收阈值记录在基准配置中，而不是散落在实现代码。推荐初始阈值：1,000 条关键纯计算 P95 不超过 50ms，5,000 条不超过 150ms；浏览器首次可交互和 DOM 阈值根据本机基线在任务执行阶段固化。如果 5,000 条失败，则优先对展示列表实施窗口化或分段渲染，同时保持搜索、父子层级、选中状态和拖拽语义；否则只保留基准，不引入虚拟化依赖。

### 7. 用版本摘要替代热路径完整 JSON 比较

工作台大数组应用结果时，优先依据云端 change sequence、实体 id + version 摘要和数组顺序判断是否需要替换；仅对没有版本语义的小对象保留深比较。这样避免刷新时同时序列化新旧完整事项树。

该调整只处理本变更触及的 workspace、time 和相关启动数据，不抽象成通用深比较框架。

### 8. 扩展现有性能验证而不记录业务正文

在现有 directCloudRequestBudget 和 observability 上增加：command 后台执行等待或运行计数、HTTP Client 创建与复用计数、main/hover bootstrap 次数、初始 chunk 或动态 chunk 验证及大事项集基准。指标不得包含 URL 中的凭据、anon key、access token、事项标题或报告正文。

## Risks / Trade-offs

- [blocking 线程池过小导致云端请求排队] → 区分前台 mutation 与后台刷新优先级，设置合理并发上限并记录排队时间；不允许后台刷新无限占位。
- [blocking 线程池过大导致线程和连接激增] → 使用单一有界执行器和共享 HTTP Client，以压力测试确定上限。
- [客户端缓存未及时失效而访问错误项目] → 缓存键包含配置指纹，配置写入和测试 guard 显式清理，并用双配置测试验证。
- [bootstrap 合并后部分调用依赖旧副作用] → 逐项记录 bootstrap 写入的日历、主体和 access 状态，保留独立 command 兼容测试。
- [动态组件首次进入出现短暂空白] → 提供稳定骨架或加载状态，预加载用户高概率进入的下一页面，但不得让 hover 预加载 main 重量组件。
- [虚拟树形列表破坏拖拽和多选] → 只有基准失败才实施，并把层级、选择、滚动和拖拽列为独立验收场景。
- [性能测试受机器波动影响] → 纯算法测试使用固定数据和多轮统计；浏览器指标记录环境并采用相对回归阈值而非单次绝对值。

## Migration Plan

1. 先增加当前启动请求数、Client 创建数、构建产物和大事项集基线测试，锁定优化前行为。
2. 引入云端 blocking 执行器，迁移高频查询和 mutation command；验证慢请求期间窗口命令与本地模式操作仍可执行。
3. 引入配置指纹 Client 缓存及失效测试，再观察连接复用和错误率。
4. 增加一次性 bootstrap 消费入口，改造 main/hover 初始化并收紧请求预算。
5. 对窗口和重量页面实施动态 import，验证构建分包、窗口首次展示和页面 loading fallback。
6. 替换启动热路径的完整 JSON 比较，运行 1,000/5,000 条事项基准；仅在阈值失败时实施有界渲染。
7. 清理本次触及路径的历史参数与死代码，执行 TypeScript、Rust、请求预算、构建分包和双窗口回归。

回滚可按层进行：动态分包可恢复为静态导入；bootstrap 消费入口可恢复独立查询；Client 缓存可禁用而不改变请求语义；后台执行器若出现兼容问题可逐 command 回退。任何回滚都不得恢复高频全量轮询或改变 Supabase 单一权威边界。
