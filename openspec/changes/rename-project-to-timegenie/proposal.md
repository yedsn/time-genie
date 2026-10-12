# Proposal

## Why

项目已经完成从旧工程名到 TimeGenie 技术标识的主体改名，但 Tauri 的 productName、系统托盘提示和发布流程仍以中文「时序」作为系统级名称。这导致 Windows 安装目录、开始菜单、卸载项、macOS 应用包和部分 Release 信息以中文命名，与可执行文件、仓库、更新资产和内部技术标识不一致。

当前应用已经发布到 v0.2.8，不能再把改名视为首次发布前的零成本身份替换。后续调整必须保持 com.timegenie.desktop、用户数据目录、数据库文件、凭据服务和更新地址不变，只改变系统与分发层的名称。当前仅有项目维护者本人使用已安装版本，可以采用“备份用户数据、卸载旧程序、安装新程序”的一次性迁移，不需要在产品中保留自动迁移代码。

## What Changes

- 建立三层命名规则：系统、分发与应用品牌界面统一以英文产品名 TimeGenie 为主；内部技术标识继续使用小写 timegenie；中文名「时序」仅作为界面和中文文档中的辅助名称。
- 将 Tauri productName 改为 TimeGenie，并显式设置 mainBinaryName 为 TimeGenie，使安装包、应用包、安装目录、系统入口与主程序文件统一使用英文名称和一致大小写。
- 将主窗口的系统标题、Tauri 托盘提示、原生 Windows 兜底托盘提示、自定义标题栏和侧栏品牌主标题改为 TimeGenie；侧栏可保留「时序」作为辅助名称。
- 将 GitHub Release 名称、Release 说明、macOS 应用路径示例和本地发布脚本中的应用名统一为 TimeGenie。
- README 与文档站中的产品标题采用英文优先的双语写法 TimeGenie（时序）；中文产品名只作为面向中文用户的界面名称和辅助说明。
- 保持应用身份和数据位置不变：com.timegenie.desktop、%APPDATA%\com.timegenie.desktop、timegenie.sqlite3、系统凭据 service、Supabase schema、更新端点均不修改。
- 采用一次性人工迁移：退出并备份数据，卸载旧的「时序」，确认数据目录仍存在，再安装 TimeGenie 并核对数据。

## Capabilities

### New Capabilities

- app-identity：定义 TimeGenie 在系统、分发、技术标识和应用界面四类位置的命名契约，以及已发布版本调整系统名称时的数据连续性要求。

### Modified Capabilities

无。此次调整只改变应用身份呈现，不改变计划、计时、工时结算、报告生成、外部同步或云端存储等业务能力。

## Impact

**应用配置与运行时**

- src-tauri/tauri.conf.json：productName、mainBinaryName、主窗口系统标题。
- src-tauri/src/lib.rs：Tauri 托盘提示。
- src-tauri/src/native_tray.rs：Windows 原生兜底托盘提示。
- src-ui/index.html、src-ui/src/views/MainWorkbench.vue：应用品牌名称改为英文优先，中文「时序」仅作辅助显示。

**发布与文档**

- .github/workflows/release.yml：Release 名称、说明和 macOS 应用路径。
- scripts/release/release.ps1：发布脚本显示的应用名。
- README.md、docs/index.md、docs/.vitepress/config.mts 及相关设计文档标题：改为英文优先的双语品牌表达。
- 安装包、更新归档和签名文件继续使用既有 TimeGenie 英文前缀。

**明确不改动**

- Tauri identifier com.timegenie.desktop。
- 本机数据目录与数据库文件名。
- 系统凭据、Supabase schema、环境变量前缀和更新 URL。
- 应用业务界面的中文文案以及「日报/周报/月报」等领域术语。

**人工迁移**

- 当前 Windows 安装项「时序」位于 %LOCALAPPDATA%\时序；用户数据位于独立的 %APPDATA%\com.timegenie.desktop。
- 先备份用户数据，再卸载旧程序并安装 TimeGenie。不搬迁、不重命名用户数据目录。
