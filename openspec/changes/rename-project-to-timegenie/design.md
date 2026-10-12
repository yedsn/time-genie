# Design

## Context

TimeGenie 当前已经发布到 v0.2.8。工程底层标识已统一为英文：npm/Cargo 包名为 timegenie，identifier 为 com.timegenie.desktop，数据库文件为 timegenie.sqlite3，发布镜像资产也使用 TimeGenie 前缀。

剩余不一致集中在系统可见名称：src-tauri/tauri.conf.json 的 productName 和主窗口标题仍为「时序」，两套托盘实现的 tooltip 仍为「时序」，发布工作流和本地发布脚本也保留中文应用名。Tauri 的 productName 会参与安装包、安装目录、开始菜单、卸载项和 macOS .app 名称，因此它应承担系统与分发层的英文产品名，而不是应用内容区的中文显示名。

本机核实到旧版本卸载项名称为「时序」，安装目录为 %LOCALAPPDATA%\时序；用户数据已经位于 %APPDATA%\com.timegenie.desktop\timegenie.sqlite3。这说明系统安装名称可以通过卸载重装调整，而无需迁移业务数据。

## Goals / Non-Goals

**Goals:**

- 系统与分发层统一显示 TimeGenie。
- 内部技术标识继续统一为 timegenie。
- 应用内容界面的品牌名称以 TimeGenie 为主，「时序」仅作辅助名称。
- 保持已发布应用的数据、凭据和更新身份连续。
- 为唯一使用者提供简单、可验证、可回滚的一次性卸载重装步骤。

**Non-Goals:**

- 不改变 identifier、数据目录、数据库文件名、凭据 service、Supabase schema 或更新端点。
- 不编写旧安装目录探测、自动卸载、自动迁移或兼容清理代码。
- 不修改业务功能、数据结构或领域术语。
- 不为多个外部用户设计静默升级或批量迁移方案。
- 不在本变更中执行本机卸载、安装或删除备份。

## Naming Model

| 层级 | 规范名称 | 典型位置 |
|---|---|---|
| 系统与分发层 | TimeGenie | 安装包、主程序、安装目录、开始菜单、卸载项、系统窗口标题、托盘提示、macOS .app、Release 名称 |
| 内部技术标识 | timegenie | npm/Cargo 包名、Rust lib、数据库文件、凭据 service、Supabase schema、缓存 key |
| 稳定应用身份 | com.timegenie.desktop | Tauri identifier、应用数据目录、会话凭据 target 前缀 |
| 应用品牌界面 | TimeGenie（主）/「时序」（辅） | 浏览器标题、自定义标题栏、侧栏品牌块 |
| 业务操作界面 | 中文 | 页面名称、按钮、提示、托盘菜单与领域术语 |

## Decisions

### D1：系统与品牌界面以英文为主

TimeGenie 是系统、分发和应用品牌界面的主名称；「时序」只作为中文辅助名称。业务操作界面继续使用中文。

这样可让文件系统、命令行、发布资产和操作系统入口保持 ASCII、一致且易引用，同时保留中文界面的识别度。

### D2：productName 使用 TimeGenie

将 Tauri productName 从「时序」改为 TimeGenie。它负责驱动安装包、安装目录、系统快捷方式、卸载项和 macOS 应用包等系统名称。

主窗口配置中的系统 title 同步改为 TimeGenie，供任务切换器、辅助功能和系统级窗口识别使用。前端自绘标题栏和侧栏品牌主标题也显示 TimeGenie，侧栏辅助行可显示「时序」。悬浮计时窗口标题「今日计时」是功能标题，不承担应用身份，可保持中文。

### D3：显式设置 mainBinaryName 为 TimeGenie

Cargo 包名继续保持小写 timegenie，以符合 Rust 包命名习惯；通过 Tauri mainBinaryName 将最终主程序文件显式命名为 TimeGenie.exe 或对应平台的 TimeGenie。

不把 Cargo 包名改成大小写混合形式，也不依赖文件系统对大小写不敏感的偶然行为。

### D4：稳定身份和数据位置全部保持不变

以下内容不得随系统显示名修改：

- identifier: com.timegenie.desktop
- %APPDATA%\com.timegenie.desktop
- timegenie.sqlite3
- SeaTable 和 Supabase 凭据 service/target
- timegenie Supabase schema
- updater endpoints 与签名密钥

因此新安装可直接读取现有数据库和配置，不需要复制或重命名数据。

### D5：托盘属于系统层，使用英文名称

Tauri 托盘与 Windows 原生兜底托盘的 tooltip 都改为 TimeGenie。托盘菜单命令仍是应用操作界面的一部分，继续使用中文。

### D6：发布元数据以英文为主

GitHub/Gitee Release 名称、自动生成的 Release 说明、本地发布脚本输出和 macOS 应用路径示例统一使用 TimeGenie。README 和文档站采用 TimeGenie（时序），英文名称在前。

### D7：采用卸载重装，不实现自动迁移

当前只有维护者本人使用。迁移顺序为：

1. 完全退出旧程序。
2. 备份 %APPDATA%\com.timegenie.desktop。
3. 卸载系统中的「时序」。
4. 确认用户数据目录与 timegenie.sqlite3 仍存在。
5. 安装新的 TimeGenie。
6. 检查数据、设置、凭据和更新功能。
7. 验证正常后再删除备份。

如果旧卸载程序意外删除用户数据，可从备份原路径恢复。无需将数据搬到新的目录，因为 identifier 没有变化。

## Risks / Trade-offs

- **旧安装和新安装可能并存**：直接运行新安装包可能生成新的 TimeGenie 卸载项，而旧「时序」仍保留。通过先卸载旧程序规避。
- **卸载程序可能影响用户数据**：虽然应用数据通常独立于安装目录，仍要求卸载前完整备份并在卸载后确认。
- **Windows 快捷方式残留**：卸载后检查开始菜单和桌面，手工删除残留的「时序」快捷方式即可，不在程序中加入清理逻辑。
- **自动更新链路跨名称**：旧安装理论上可能通过 updater 安装新名称，但本变更不依赖这条路径；唯一用户采用卸载重装，降低重复安装风险。
- **文件名大小写验证困难**：Windows 文件系统通常大小写不敏感，因此验证应读取实际产物名称，而不能只验证文件可打开。

## Migration Plan

1. 更新 OpenSpec 规划，使命名规则和当前已发布状态一致。
2. 修改 Tauri 系统名称、主程序名称、两套托盘提示与发布元数据。
3. 将应用品牌界面改为英文优先，并检查未误改业务中文文案。
4. 执行类型检查、Rust 检查和全量 Tauri 打包，核对 Windows 与 macOS 预期产物名称。
5. 备份现有数据并卸载「时序」。
6. 安装 TimeGenie，验证它读取原有数据目录。
7. 检查系统入口只剩 TimeGenie，验证完成后删除备份。

**回滚策略：**卸载 TimeGenie，恢复数据目录备份，再安装最后一个中文系统名版本。由于 identifier 与数据库格式不变，不需要执行数据格式回滚。

## Open Questions

无。当前范围、命名边界和迁移方式均已确认。
