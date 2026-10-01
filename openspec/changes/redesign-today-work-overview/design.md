# Design

## Context

今日页当前直接组合 `store.todayMinutes`、`store.allocatedMinutes`、`store.pendingMinutes`、`store.todayTasks` 和 `TaskTable`。这些数据来自今日时间记录和今日待办，适合结算提示，但缺少日期范围聚合、主体过滤后的历史趋势、未归属时间段和当天时间轴。

现有数据已经覆盖本变更需要的来源：主体、任务、任务每日预计、时间记录、时间归属、未归属会话。实现应复用现有 SQLite/Tauri 架构和 Pinia store，不新增持久化表。

## Goals / Non-Goals

**Goals:**
- 为今日页提供一个按主体和日期范围聚合的稳定数据源。
- 用一个主工作台视图展示今日摘要、近 30 天点阵、近 14 天趋势、今日时间轴和今日事项摘要。
- 保持页面可扫读，减少与待办页、计时页重复的编辑功能。
- 在全部主体和单主体范围之间保持一致的数据口径。

**Non-Goals:**
- 不重写待办页的层级编辑、拖拽、重复任务设置等管理能力。
- 不重写计时页的开始、暂停、结算和分配流程。
- 不引入新的图表库作为第一版依赖。
- 不新增持久化数据表；如果已有数据不足，以派生查询解决。

## Decisions

### Decision 1: 新增聚合查询接口驱动今日页

新增 Tauri 命令，例如 `today_work_overview_get`，输入 `subjectId?`、`todayDate`、`historyDays`、`trendDays`，输出今日摘要、近期日汇总、今日时间段和今日事项摘要。

原因：前端逐日调用 `time_entry_list` 和 `task_list` 会产生多次查询、状态拼接复杂，并容易出现今日页不同区域口径不一致。后端聚合可以统一主体过滤、日期范围和未归属时间计算。

备选方案：前端循环拉取已有接口。该方案改动少，但会在 30 天点阵和 14 天趋势中产生重复请求，也难以获取未归属会话与事项预计的统一视图。

### Decision 2: 今日页使用当前主体选择作为默认范围，并提供“全部主体”切换

今日页打开时默认使用 `store.selectedSubjectId`，同时提供“全部主体”选项。切换今日页主体时不强制改变待办页当前主体，除非用户明确进入待办页并选择主体。

原因：用户希望按某一主体查看每日工作情况，但今日页是概览视角，不应意外改变待办页正在编辑的主体上下文。

备选方案：完全复用侧边栏主体选择。该方案简单，但现在侧边栏主体只在待办页下展示，且会让概览浏览和待办编辑互相干扰。

### Decision 3: 点阵和曲线用轻量 CSS/SVG 实现

近 30 天点阵用 CSS grid；近 14 天趋势用内联 SVG polyline 或纯 CSS 条线。颜色等级由每日实际投入分钟计算：0、低、正常、较高、高投入。

原因：图形表达简单，使用图表库会增加体积和样式适配成本。当前应用已有自定义 UI 风格，轻量实现更容易匹配。

备选方案：引入 ECharts 或 Chart.js。只有在后续需要缩放、复杂 tooltip、多序列分析时再考虑。

### Decision 4: 今日时间轴优先展示可理解的近似分布

时间轴基于 `time_entries.started_at`、`ended_at`、`duration_seconds`、`kind`、`state` 和归属事项生成。未归属时间使用 `unassigned_sessions` 的 `first_started_at`、`last_ended_at`、`duration_seconds` 补充展示。正在运行的时间段以当前时间作为结束点。

原因：时间轴用于快速识别时间花在哪里，不要求作为精确排班工具。过度精确会增加重叠、跨日、暂停片段等复杂度。

备选方案：按 time_segments 精确绘制所有暂停/恢复片段。该方案更准确，但第一版会增加后端 DTO 和视觉复杂度。

### Decision 5: 今日事项摘要只保留执行相关操作

今日页事项摘要展示事项名、路径、状态、今日预计/整体预计、实际用时、最近记录时间和快捷开始/继续计时。编辑标题、层级、重复规则、拖拽排序继续留在待办页。

原因：今日页定位是态势概览，完整编辑表格会与待办页重复，并稀释点阵、曲线和时间轴的价值。

备选方案：继续复用 `TaskTable`。该方案实现快，但用户已经反馈当前内容作用不大，且 TaskTable 更像管理列表。

## Data Shape

建议后端返回结构：

```ts
type TodayWorkOverview = {
  scope: { subjectId?: string; subjectName: string; todayDate: string };
  summary: {
    actualMinutes: number;
    estimatedMinutes: number;
    completedCount: number;
    activeCount: number;
    notStartedCount: number;
    unassignedMinutes: number;
    completionRate: number;
  };
  days: Array<{
    date: string;
    actualMinutes: number;
    estimatedMinutes: number;
    completedCount: number;
    totalCount: number;
    unassignedMinutes: number;
  }>;
  timeline: Array<{
    id: string;
    type: "work" | "break" | "unassigned";
    taskId?: string;
    taskTitle: string;
    subjectId?: string;
    startedAt: number;
    endedAt?: number;
    minutes: number;
    state?: "running" | "paused" | "ended";
  }>;
  tasks: Array<{
    taskId: string;
    title: string;
    pathLabel: string;
    subjectId: string;
    status: "done" | "active" | "not_started" | "started";
    actualMinutes: number;
    todayEstimateMinutes?: number;
    estimateMinutes?: number;
    lastEntryAt?: number;
    selectable: boolean;
  }>;
};
```

## Risks / Trade-offs

- [Risk] 聚合接口口径与报告生成口径不一致 -> 复用同一批基础表，并补充后端测试覆盖任务实际用时和主体过滤。
- [Risk] 全部主体视图中数据过多导致页面拥挤 -> 默认展示摘要和可视化，事项摘要只展示今日相关事项，并提供空状态/折叠策略。
- [Risk] 未归属时间没有主体归属，单主体视图展示会产生歧义 -> 单主体视图只展示与该主体事项相关的时间段；未归属时间在摘要中可显示全局提示或按全部主体视图展示，具体 UI 文案需说明“未归属时间未绑定主体”。
- [Risk] 轻量 SVG 曲线在极端数据下可读性不足 -> 第一版限制为 14 天趋势，并显示数值 tooltip/标题，后续再评估图表库。

## Migration Plan

1. 新增后端只读聚合接口和前端服务类型，不改变现有计时、待办、报告接口。
2. 新增 store 加载状态和今日页组件，先与现有今日页并行开发。
3. 替换今日页主体内容，保留顶部全局计时条、未归属时间入口和待办/计时跳转。
4. 若出现问题，可回退到旧今日页布局，因为数据表和核心业务接口未迁移。
