# Design

## Context

见 `proposal.md` 的 Why 和 `specs/automation-hooks/spec.md`。当前本地计时和事项写入分别分散在 `time_tracking.rs`、`tasks.rs`、`unassigned.rs` 与托盘入口；云端模式通过 Supabase RPC 写入后拉取快照。设置目前以简单键值保存到 `device_settings` 或 `app_settings`，不适合表达多条可排序规则和执行历史。

Hook 需要运行本机 URI 或子进程，属于设备能力而不是工作空间业务数据。它既不能在数据库事务中同步等待，也不能因脚本错误改变事务结果。Windows GUI 应用启动子进程时还必须避免弹出终端窗口。

## Goals / Non-Goals

**Goals:**

- 为所有本机和云端操作入口提供统一的提交后业务事件出口。
- 用结构化配置安全地支持自定义 URI 与本地程序，不默认引入任意 Shell 解释。
- 让 Hook 可测试、可去重、可观察，并让失败与核心业务完全隔离。
- 保持设备级数据不进入 Supabase 快照、迁移和冲突处理。

**Non-Goals:**

- 不提供可阻止、修改或审批业务操作的前置 Hook。
- 不实现条件表达式语言、事件转换脚本、依赖编排或自动重试。
- 不远程执行另一台设备上的 Hook，也不补放同步得到的历史事件。
- 不增加完整 Shell 命令动作；如未来需要管道、重定向或内建命令，应单独设计高风险高级模式。

## Decisions

### Decision: 使用独立设备表保存规则和执行记录

新增本地表：

```text
device_hooks
  id                    TEXT PRIMARY KEY
  name                  TEXT
  event_type            timer.started | timer.stopped | task.completed
  action_type           uri | process
  action_config_json    URI 模板或程序/参数/工作目录
  timeout_seconds       1..60
  enabled               0 | 1
  sort_order
  created_at / updated_at

device_hook_runs
  id                    TEXT PRIMARY KEY
  hook_id               可空，规则删除后保留名称快照
  hook_name_snapshot
  event_id
  event_type
  is_test
  status                queued | running | succeeded | failed | timed_out
  exit_code
  duration_ms
  stdout_tail / stderr_tail / error_message
  started_at / finished_at / created_at
  UNIQUE(hook_id, event_id) WHERE is_test = 0
```

两张表不带 `workspace_id`，不加入云端实体列表和快照导入导出。复杂规则若塞入 `device_settings` JSON，会使逐条编辑、排序、唯一执行和运行历史都依赖整份 JSON 覆盖，因此不采用。

规则删除时保留运行记录并将 `hook_id` 置空或使用可空外键；运行记录保存 Hook 名称快照，保证历史可读。启动或定期清理时仅保留最近 100 条或 30 天内记录，并对 stdout/stderr 各保留末尾 8 KiB。

### Decision: 定义统一的提交后事件信封

后端新增内部事件结构，不直接把数据库 DTO 当作脚本协议：

```json
{
  "schemaVersion": 1,
  "event": "timer.stopped",
  "eventId": "timer.stopped:<operation-or-entry-id>",
  "occurredAt": "2026-10-01T10:30:00+08:00",
  "test": false,
  "device": { "id": "..." },
  "subject": { "id": "...", "name": "默认" },
  "task": { "id": "...", "title": "事项", "path": "父项 / 事项", "occurrenceDate": null },
  "taskSnapshot": { "title": "事项" },
  "timer": {
    "id": "...",
    "startedAt": 0,
    "endedAt": 0,
    "durationSeconds": 1800,
    "settlementMinutes": 30,
    "allocatedMinutes": 0,
    "pendingMinutes": 30
  }
}
```

事件载荷只包含已定义字段，不透传设置、凭据、备注中的敏感集成配置或数据库整行。`schemaVersion` 允许以后增字段而不破坏脚本。不存在的当前实体使用 `null`，名称快照独立保留。

事件 ID 使用可重试操作的 operation ID；没有 operation ID 的本地事务使用稳定业务实体与状态版本组合，例如 `task.completed:<task-id>:<occurrence-date-or-global>:<new-version>`。测试执行使用随机事件 ID 并标记 `test=true`，不参与实际事件唯一约束。

### Decision: 业务函数返回提交结果，命令边界统一发布事件

数据库事务只负责业务写入和形成事件所需结果，不在事务内启动进程。Tauri 命令、托盘动作和云端命令在确认提交成功后调用统一调度器：

```text
业务事务 / Supabase RPC 成功
          |
          v
构造 HookEvent（0..N 个）
          |
          v
写入 queued 运行记录并异步调度
          |
          +--> 立即返回业务结果
          |
          +--> 后台执行并更新状态
```

直接完成事项产生一个事件；工时分配和未归属分配可能产生多个 `task.completed` 事件。为避免前端、托盘、本地和云端入口各自实现不同逻辑，事件构造应放在 Rust 领域边界，前端只刷新执行结果。

云端请求必须沿用 operation ID 作为事件去重依据。拉取快照、应用同步结果和应用启动恢复不得发布事件。备选方案是在前端 watch 状态变化，但无法区分本机操作与同步重放，也覆盖不了托盘，因此不采用。

### Decision: 使用持久化队列实现进程内异步执行

匹配规则后，调度器先在 `device_hook_runs` 插入 `queued` 记录，再通过 Tauri 异步运行时执行。这样 UI 不等待命令，进程崩溃时也能识别未完成记录。应用启动时把遗留的 `queued` 或 `running` 标记为失败/中断，不自动重放，以避免重复外部副作用。

同一事件内按 `sort_order` 调度规则，但规则之间不互相依赖；单条失败不停止后续规则。第一版可以串行执行同一事件的规则以保持可预测顺序，不要求不同事件间严格排序。

备选方案是只用内存任务、不落 queued 记录，代码较少但应用退出时无法区分“未执行”和“执行中断”，也无法可靠去重，因此不采用。

### Decision: URI 与本地程序使用不同执行器

`uri` 动作：

- 先解析模板并验证 scheme；只允许 `http`、`https` 和合法自定义 scheme，拒绝 `file`、`javascript`、空 scheme 及控制字符。
- 动态模板变量作为 URI 组件值进行百分号编码，不允许变量注入新的 scheme、authority、query 参数边界或命令。
- Windows 使用 `ShellExecuteW`/等效系统协议 API，macOS 和 Linux 使用对应默认打开机制。
- 系统无法找到协议处理器时记录明确错误。

`process` 动作：

- 配置保存 `program`、字符串参数数组、可选 `workingDirectory`。
- 使用 `std::process::Command` 直接执行，不经过 `cmd /c`、PowerShell 字符串或 POSIX shell。
- 事件 JSON 通过 stdin 写入，stdout/stderr 使用管道读取并截断。
- Windows 统一应用 `CREATE_NO_WINDOW`，并设置隐藏窗口，避免终端闪现。
- 超时后终止子进程；如果子进程派生后代，第一版只保证终止直接进程，并在风险中说明。

这两个执行器的风险和错误模型不同，不能用一个自由文本命令框统一表达。

### Decision: 模板变量采用固定白名单

URI 和程序参数可引用固定变量，例如：

```text
{{event.type}}
{{event.id}}
{{task.id}}
{{task.title}}
{{task.path}}
{{task.occurrenceDate}}
{{subject.id}}
{{subject.name}}
{{timer.id}}
{{timer.durationSeconds}}
{{timer.durationMinutes}}
{{timer.pendingMinutes}}
```

缺失变量渲染为空字符串，并在测试预览中提示；未知变量拒绝保存。URI 使用百分号编码值，程序参数按单个参数替换但不做 Shell 转义。第一版不支持表达式、默认值、循环或访问任意 JSON 路径，以控制复杂度和安全面。

### Decision: 测试执行复用真实执行器但使用示例事件

设置弹框提供“测试”按钮，将未保存草稿发送给后端。后端校验配置后构造 `test=true` 的示例载荷并运行同一 URI/process 执行器，记录为测试运行。测试不保存规则、不触发计时或事项事务，也不使用真实事项标题，避免误泄露数据。

测试结果返回状态、耗时、退出码和截断输出；测试 URI 仍会真实打开目标协议，因此按钮旁需明确“将立即执行当前动作”。

### Decision: 设置页使用列表与编辑弹框

设置页新增独立“自动化 Hook”面板：

- 顶部显示用途和“添加 Hook”；
- 列表行显示启用开关、名称、事件、动作摘要、最近状态；
- 行操作提供测试、编辑、删除；删除使用 Element Plus 确认；
- 编辑弹框按动作类型显示 URI，或程序、参数列表、工作目录和超时；
- 参数使用可增减的逐行输入，避免用户手写 JSON 或处理引号；
- 最近执行详情使用轻量抽屉或弹框展示，不把大量日志直接铺在设置页。

Hook 面板使用独立保存，不并入当前页面底部的“保存设置”，避免新增、测试和排序依赖整页表单状态。

## Risks / Trade-offs

- [本地程序拥有当前用户权限，可执行危险操作] → 设置页明确标注风险，只允许用户主动配置；不从云端导入规则，不提供远程创建入口，不使用 Shell 解释。
- [业务已成功但应用在写入 Hook 队列前崩溃] → 事件调度在命令返回前快速写入本地 queued 记录；仍接受极小的提交后崩溃窗口，不为此把外部副作用放入业务事务。
- [同一操作通过网络重试触发重复动作] → 使用稳定 event ID 和 `(hook_id, event_id)` 唯一约束，在调度前做数据库幂等插入。
- [脚本阻塞或输出无限增长] → 限制 1 至 60 秒超时、管道读取上限和执行记录保留量。
- [终止直接进程后派生子进程仍存活] → 第一版文档化限制；若实际脚本需要可靠进程树终止，后续在 Windows 使用 Job Object、其他平台使用进程组。
- [URI 模板编码粒度错误导致不可用或注入] → 仅允许固定变量替换，动态值按组件编码；保存和测试时展示最终 URI 预览。
- [云端完成事件缺少完整路径或新状态] → RPC 成功后拉取或使用返回结果构造事件；只有当前设备发起的命令边界发布，不从快照 diff 推断。
- [设置页继续膨胀] → Hook 使用单个概览面板和专用编辑弹框，运行详情按需展开。

## Migration Plan

1. 新增设备 Hook 和运行记录表，现有设备默认没有规则，因此升级后行为不变。
2. 先上线规则 CRUD、测试执行和设置 UI，再接入事件发布；未配置规则时事件调度为零开销快速返回。
3. 接入本地计时、托盘和事项完成路径，再接入云端操作命令边界及多事项完成路径。
4. 增加运行记录清理和启动中断恢复，完成 Windows 隐藏进程与 URI 协议验收。

回滚时先停止发布新事件并隐藏设置入口，保留本地表和历史记录；旧版本忽略新增表，不影响计时、待办和报告数据。
