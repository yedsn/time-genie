# 性能验收结果

## 环境

- 日期：2026-10-25
- 平台：Windows x64
- Node.js：v22.19.0
- 浏览器：Playwright Chromium 153.0.8010.12
- 视口：1360 × 900
- 构建模式：Vite production preview

## 页面切换

- 已缓存页面切换样本：25 次
- P95：23.5ms
- 验收阈值：不超过 50ms
- 今日、待办、时间记录、报告、设置五个主页面的挂载次数均为 1
- 同一渲染帧快速扫过待办、时间记录、报告时，只创建最终报告页，中间页面没有挂载
- 点击会取消待处理 hover，旧目标不会覆盖点击结果

## 状态与查询

- 时间记录页面离开再返回后，手动补录表单值与页面滚动位置保持
- 待办页面离开再返回后，当前标签保持
- 报告页面离开再返回后，所选报告、类型筛选、编辑模式和未保存草稿保持
- 报告领域 Realtime 失效后发起后台权威查询，未保存草稿没有被覆盖
- 未失效的今日计划、时间历史再次进入均未新增查询
- 首次验收请求次数：task_day_plan_snapshot=1、time_entry_history_list=1

## 代码预加载与设置懒加载

- 报告代码在空闲期或 pointer intent 时通过稳定 Promise 预加载
- 仅预加载代码时报告页面未挂载，report_list=0
- 首次进入设置没有创建备份、操作记录、历史镜像和自动化设置组件
- 接近相应设置入口后才创建组件；再次返回设置时复用已有实例
- 重复进入设置后 data_backup_history_list=1、automation_hook_list=1

## 资源稳定性

- 压力操作：今日与报告之间往返切换 100 次
- DOM 节点：1086 → 1081
- 活动事件监听估算：909 → 908
- 页面实例数：压力操作前后不变
- Chromium usedJSHeapSize 记录值：35,100,000 bytes
- 结论：未观察到随切换轮次线性增长的页面实例、DOM 或事件监听

## 构建边界与动态效果

- HoverPanel 与 MainWorkbench 继续保持独立动态 chunk
- HoverPanel 初始依赖链不包含 MainWorkbench、md-editor-v3、报告工作区或四个重量设置模块
- 生产构建只有既有的大 chunk 提示，没有新增循环 chunk 警告
- 首次页面反馈为 120ms opacity 动画；缓存页面再次激活不重播
- prefers-reduced-motion: reduce 下页面和 skeleton 动画均为 none

## 已通过命令

- npm run typecheck
- npm run build
- npm run test:workbench-navigation
- npm run test:application-responsiveness
- npm run test:direct-cloud-interaction-performance
- npm run test:timer-followed-ui
- npm run test:tray-hover
- npm run test:direct-cloud-realtime-boundary
- npm run test:work-data-refresh

补充运行的旧 UI 脚本 test:today-plan-ui 与 test:task-status-tabs-ui 仍依赖旧的启动 mock，未提供当前必需的 workbench_bootstrap_get 返回值，因此分别在旧测试前置状态和空 bootstrap 上失败；本变更要求的导航缓存、请求预算、Realtime、计时 UI 与构建边界回归均已通过。
