# Proposal

## Why

项目即将首次公开发布，但当前名称只覆盖了产品的五个模块中的一个。`work-report-assistant` / 「工作日报助手」把产品说成了"日报工具"，而它实际上是一个本地优先的工作台：计划、计时、工时结算、日报/周报/月报生成与外部同步。产品自己已经在用别的定位词描述自己——侧栏副标题（本变更中由"本地优先工作台"改为"计时 · 工时 · 日报"），顶部标题是"{{页面}}工作台"——说明现有名称已经落后于产品本身。

更关键的是时间窗口。`identifier` 决定 Tauri 的应用数据目录、安装包身份与开机启动项，发布后变更会让新旧版本在系统层面成为两个互不相干的应用，用户已存的本机数据与凭据不会自动跟随，届时必须编写迁移逻辑并承担升级路径断裂的风险。当前项目尚无任何外部用户，`openspec/specs/` 也尚无已同步的主 spec，这是定名的唯一零成本窗口。

## What Changes

- 应用对外身份改为**中文显示名「时序」+ 英文产品名 `TimeGenie`**，两者并行：用户可见位置用中文名，工程与网络标识用 `timegenie`。
- 工程标识统一为 `timegenie`：本地目录、GitHub 仓库、npm 包名、Cargo 包名与 lib 名、bundle identifier、SQLite 文件名、系统凭据库 service 名、主题缓存 key、Supabase 客户端标识字符串与 `supabase/config.toml` 的 `project_id`。
- 环境变量前缀由 `WRA_` 改为 `TG_`：共 5 个变量名、12 处引用，分布在 `README.md`（5 处）、`src-tauri/src/cloud_e2e.rs`（6 处，其中 `WRA_SUPABASE_E2E_ALLOW_RESET` 出现 2 次）与 `src-tauri/src/lib.rs` 的开发期变量 `WRA_USE_APP_DATA_DIR`（1 处）。其中 `WRA_USE_APP_DATA_DIR` 带行为含义，控制 debug 构建是否改用 `target/dev-data` 而非真实数据目录。
- **BREAKING**：`identifier` 由 `com.workreportassistant.desktop` 改为 `com.timegenie.desktop`，导致 Tauri 应用数据目录从 `%APPDATA%\com.workreportassistant.desktop` 变为 `%APPDATA%\com.timegenie.desktop`，已有本机 SQLite 数据不再被读取。
- **BREAKING**：系统凭据库 service 名变更，已保存的 SeaTable Base API Token 与 Supabase 用户会话不再被读取，需要重新录入或重新登录。
- 主题缓存 key 由 `work-report-assistant:app-theme` 改为 `timegenie:app-theme`，用户主题偏好重置（可接受，仅影响外观）。
- 修正一处既有不一致：`src-ui/index.html` 的 `<title>` 目前是英文 `Work Report Assistant`，与应用内中文界面不一致，本次统一。
- 明确**不得改动**的部分：报告类型词 `日报/周报/月报`、Obsidian 默认路径规则 `工作日报/{date}.md`、结算术语 `原始工时/已分配/待结算`、`supabase/schema.sql` 的表结构与 RLS 策略、归档 change 的 id 与内容。

## Capabilities

### New Capabilities

- `app-identity`: 应用的对外显示名、工程与构建产物标识、本机数据与凭据的存放位置，以及在品牌改名过程中必须保持不变的领域术语。本次为新能力，因为项目尚无已同步的主 spec；选择新建而非复用 `daily-workbench` 等归档能力，是因为改名不影响这些能力的功能需求，而"应用身份"本身是一份会被后续变更（图标、版本、关于页、自动更新与代码签名）持续扩展的独立契约。

### Modified Capabilities

无。归档 change 中的 `daily-workbench`、`time-tracking-and-allocation`、`report-generation`、`external-sync`、`task-daily-planning`、`task-completion-feedback` 均未同步到 `openspec/specs/`（当前主 spec 为空），且本次改名不改变其中任何功能需求。

## Impact

全量清点口径：在实施开始时对 `HEAD` 用 `git grep -i` 对连字符形式、下划线形式、带空格的英文名、反向域名、`WRA_` 前缀、驼峰形式与中文显示名七类模式清点，排除 `openspec/` 后命中 **51 行、22 个文件**；其中 2 行是必须保留的归档 change id，另有 4 行位于自动生成或锁定文件（`Cargo.lock`、`package-lock.json`、`gen/schemas/capabilities.json`）。以下按层列出。

**应用配置与前端**

- `src-tauri/tauri.conf.json`：`productName`、主窗口 `title`、`identifier`。
- `src-ui/index.html`：`<title>`。
- `src-ui/src/views/MainWorkbench.vue`：标题栏应用名、侧栏品牌块。
- `src-ui/src/services/theme.ts`：主题缓存 key。

**Rust 侧**

- `src-tauri/Cargo.toml`：包名、`description`、`authors`、`[lib] name`。
- `src-tauri/src/main.rs`：lib 引用路径。
- `src-tauri/src/lib.rs`：托盘 id 的 3 处引用、托盘 tooltip（改为「时序」）、启动失败提示文案（`expect` 消息）、开发期环境变量 `WRA_USE_APP_DATA_DIR` 的读取点。
- `src-tauri/src/native_tray.rs`：**Tauri 托盘之外的原生 Win32 兜底托盘**，含托盘 id 查询、Win32 窗口类名 `work_report_assistant_native_tray`，以及第二处用户可见 tooltip。该文件在规划期不存在，实施开始时才出现在代码库中。
- `src-tauri/src/settings.rs`：凭据库 service 常量。
- `src-tauri/src/database.rs`：SQLite 文件名常量。
- `src-tauri/src/supabase.rs`：客户端标识字符串（3 处）。
- `src-tauri/src/cloud_e2e.rs`：6 处环境变量引用（5 个变量名）。
- `src-tauri/capabilities/default.json` 与已纳入版本管理的 `src-tauri/gen/schemas/capabilities.json`（生成物，需随源文件重新生成）。

**仓库与文档**

- `package.json`、`README.md`、`supabase/config.toml`、`supabase/schema.sql` 首行注释。
- `docs/20260924_work-report-assistant_数据结构设计.md`、`docs/20260924_work-report-assistant_后端接口设计.md`：文件名与文档标题。两份文档正文中"对应 OpenSpec change：`create-work-report-assistant-desktop-app`"的引用**必须保留**，因为归档 change 的 id 不改写。
- `.vscode/launch.json`、`.vscode/tasks.json`：仅使用 `${workspaceFolder}` 相对路径，不含品牌名，无需改动（已核实）。

**构建产物与锁定文件**

- `src-tauri/target/release/work-report-assistant.exe` 将重建为 `timegenie.exe`（exe 名来自 Cargo 包名，已核实）。`src-tauri/Cargo.lock`、`package-lock.json` 随包名重新生成。

**本机运行环境（一次性人工操作）**

- `%APPDATA%\com.workreportassistant.desktop\work-report-assistant.sqlite3`（606 KB，末次写入 2026-09-29）需要搬迁到新目录。
- Windows 凭据管理器中的 SeaTable Token 与 Supabase 会话需要重新建立。

**仓库外动作（不在代码变更范围内，作为实施步骤记录）**

- GitHub 仓库改名、本地目录改名、`git remote set-url`。
