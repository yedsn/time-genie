# Tasks

## 1. 性能基线与可观测性

- [x] 1.1 扩展直连性能验证，记录 main/hover 初始化的 bootstrap、查询和 Client 创建次数，并用现有 mock 调用断言当前基线可被稳定复现
- [x] 1.2 为云端后台执行增加排队、运行、完成和失败的非敏感计数或耗时指标，并用 Rust 单元测试验证指标不包含 URL 凭据、anon key、access token 或业务正文
- [x] 1.3 新增 1,000/5,000 条两级事项固定数据基准，覆盖展示行构造、筛选、计时面板数据准备和局部更新，并验证脚本输出运行环境、中位数或 P95 及样本规模

## 2. 云端阻塞工作隔离

- [x] 2.1 实现有界云端 blocking 执行器和存储路由辅助方法，验证并发上限生效且任务 panic 或 join 失败能转换为结构化错误
- [x] 2.2 将高频事项、计划、计时、未归属、概览和报告查询 command 改为 async 云端执行路径，同时保留 SQLite 本地快路径，并通过路由测试验证两种模式调用正确适配器
- [x] 2.3 将对应 mutation 及启动所需认证/配置 command 接入受控执行器，验证成功、版本冲突、epoch 过期、超时与重试结果和改造前一致
- [x] 2.4 增加慢云端 mock 的响应性测试，在业务请求挂起时调用窗口或轻量 command，并验证轻量调用无需等待慢请求完成

## 3. HTTP Client 生命周期复用

- [x] 3.1 实现以规范化 Project URL 和 anon key 指纹为键的有界 SupabaseClient 缓存，验证连续请求只创建一次底层 HTTP Client
- [x] 3.2 在云端配置更新、退出登录、切换账户和测试隔离边界清理对应缓存，并用双配置与并行测试验证不会跨项目或跨用例复用
- [x] 3.3 将查询、mutation、认证、快照和数据复制路径统一使用缓存客户端，运行相关 Rust 测试并验证请求重试与 token 刷新仍复用正确配置的连接池

## 4. 启动上下文与窗口请求预算

- [x] 4.1 暴露前端可消费的 workbench bootstrap command 与类型，覆盖日历、主体和必要工作区元数据，并用本地/直连路由测试验证返回语义一致
- [x] 4.2 重构 main 初始化为一次 bootstrap 加并行事项查询，移除同轮 refreshWorkspaceCalendar 和 listSubjects 对 bootstrap 的重复调用，并用请求预算测试验证冷启动最多一次 bootstrap
- [x] 4.3 将 store 时钟启动与视图数据初始化解耦，使 hover 仅加载计时和未归属最小数据；用悬浮窗测试验证不会加载报告、设置、完整事项树或工作台 bootstrap
- [x] 4.4 保留独立日历与主体 command 的兼容行为，并验证跨日、云端恢复、workspace epoch 变化及跨窗口失效仍能重新收敛

## 5. 前端按需加载与构建分包

- [x] 5.1 在 App.vue 中按 view 异步加载 MainWorkbench 和 HoverPanel，提供轻量 loading fallback，并用组件测试验证两个入口均可独立渲染
- [x] 5.2 将报告编辑器、备份、云端历史、快照及其他设置重量组件按页面或分区异步加载，验证首次进入有加载状态且后续交互保持原行为
- [x] 5.3 增加构建产物验证脚本，断言 hover 初始依赖链不包含 MainWorkbench、Markdown 编辑器和主窗口设置模块，并记录初始 JS/CSS gzip 大小基线
- [x] 5.4 运行生产构建并按验证结果决定是否加入最小 manualChunks 配置，确保构建无循环 chunk 且主窗口、悬浮窗和重量页面产物边界稳定

## 6. 大数据热路径收敛

- [x] 6.1 用实体 id、version、顺序和 change sequence 摘要替代 workspace/time 启动热路径的完整 JSON.stringify 数组比较，并用单元测试覆盖相同、版本变化、顺序变化和草稿混入场景
- [x] 6.2 运行 1,000/5,000 条事项基准并将浏览器首次稳定、筛选响应和活动 DOM 行数阈值写入基准配置及结果文档
- [x] 6.3 若 5,000 条基准超过阈值，为 PlanListEditor 和 TimerTaskDialog 实现虚拟列表、分段渲染或等价有界渲染；验证层级、搜索、选择、滚动定位和拖拽语义不回归；若未超过阈值，记录不引入虚拟化的基准证据

## 7. 相关复杂度清理与集成验收

- [x] 7.1 删除本变更触及路径中已无作用的 cloud_operation 参数、未使用辅助函数及 package.json 自身 file 依赖，验证 cargo check 不新增警告且 npm install/build 成功
- [x] 7.2 运行 npm run typecheck、npm run build、npm run test:direct-cloud-interaction-performance 和相关新增前端基准/分包测试，验证请求预算、动态加载与现有计时校准全部通过
- [x] 7.3 运行 cargo fmt、cargo check 及受影响 Rust 测试，验证本地 SQLite、直连云端、认证刷新、配置切换和数据保护路径均通过
- [x] 7.4 使用主窗口与悬浮窗执行弱网、并发打开、计时开始/暂停/停止、跨日和 Realtime 变化的集成验收，记录用户操作期间窗口控制面可响应且数据最终收敛
