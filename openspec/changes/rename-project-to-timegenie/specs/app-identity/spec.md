# Spec Delta

## Purpose

定义 TimeGenie 在系统与分发层、内部技术标识、稳定应用身份和中文应用界面中的名称边界，并保证已发布版本从中文系统名称切换为英文系统名称时继续使用原有数据、凭据和更新配置。

## ADDED Requirements

### Requirement: 系统与分发名称统一使用英文

应用在操作系统和发布渠道中的主名称 SHALL 为 TimeGenie。

该规则 SHALL 覆盖安装包、主程序文件、默认安装目录、开始菜单条目、桌面快捷方式、卸载项、主窗口系统标题、系统托盘提示、macOS .app 名称及 Release 名称。

#### Scenario: Windows 打包产物使用英文名称

- **WHEN** 执行 Windows NSIS release 构建
- **THEN** 安装包文件名使用 TimeGenie 前缀
- **AND** 安装后的主程序文件名为 TimeGenie.exe
- **AND** 默认安装目录、开始菜单条目和卸载项名称均为 TimeGenie

#### Scenario: macOS 打包产物使用英文名称

- **WHEN** 执行 macOS app 或 DMG release 构建
- **THEN** 应用包名称为 TimeGenie.app
- **AND** DMG 与更新归档使用 TimeGenie 英文名称

#### Scenario: 系统窗口和托盘使用英文名称

- **WHEN** 用户启动应用并在系统任务切换器或托盘中查看应用
- **THEN** 主窗口的系统标题为 TimeGenie
- **AND** Tauri 托盘和 Windows 原生兜底托盘的提示均为 TimeGenie

### Requirement: 应用内容界面保留中文显示名

应用内容区 SHALL 继续使用中文显示名「时序」，包括前端自定义标题栏和侧栏品牌块。

系统层英文名称 MUST NOT 导致应用业务文案、托盘菜单命令或领域术语被批量改为英文。

#### Scenario: 自定义界面继续显示中文品牌

- **WHEN** 用户打开主工作台
- **THEN** 自定义标题栏中的应用名为「时序」
- **AND** 侧栏品牌块中的应用名为「时序」

#### Scenario: 中文业务界面不受影响

- **WHEN** 用户使用计划、计时、工时结算、报告或设置功能
- **THEN** 相关操作文案继续使用中文
- **AND** 「日报」「周报」「月报」「已分配」「待结算」等领域术语保持不变

### Requirement: 内部技术标识保持统一且稳定

内部技术标识 SHALL 继续使用小写 timegenie，稳定应用身份 SHALL 继续使用 com.timegenie.desktop。

修改系统产品名时 MUST NOT 改变 npm/Cargo 包名、Rust lib 名、数据库文件名、凭据 service、Supabase schema、缓存 key、环境变量前缀或 updater endpoints。

#### Scenario: identifier 与数据位置保持不变

- **WHEN** 系统产品名从「时序」改为 TimeGenie
- **THEN** Tauri identifier 仍为 com.timegenie.desktop
- **AND** 应用仍从 %APPDATA%\com.timegenie.desktop\timegenie.sqlite3 读取本机数据

#### Scenario: 凭据与云端标识保持不变

- **WHEN** 用户安装英文系统名版本并启动应用
- **THEN** SeaTable 与 Supabase 使用原有 timegenie 凭据标识
- **AND** Supabase schema 仍为 timegenie
- **AND** updater endpoints 保持原值

### Requirement: 发布元数据使用英文主名称

GitHub/Gitee Release 名称、本地发布脚本中的应用名称及发布说明 SHALL 使用 TimeGenie。

README 和文档站 MAY 保留中文名称，但 SHALL 使用英文优先的 TimeGenie（时序）表达。

#### Scenario: 自动发布使用英文品牌

- **WHEN** release workflow 为版本标签创建 Release
- **THEN** Release 名称为 TimeGenie v<version>
- **AND** Release 说明中的应用名和 macOS 安装路径示例使用 TimeGenie

### Requirement: 单用户迁移采用备份后卸载重装

本次系统名称调整 SHALL 通过一次性人工卸载重装完成，MUST NOT 为此引入运行时自动迁移代码。

迁移步骤 SHALL 要求在卸载前备份 %APPDATA%\com.timegenie.desktop，在安装新版本前确认原数据目录仍存在，并在新版本启动后核对核心数据。

#### Scenario: 卸载旧程序后保留用户数据

- **WHEN** 用户备份数据并卸载旧的「时序」
- **THEN** 用户确认 %APPDATA%\com.timegenie.desktop\timegenie.sqlite3 仍存在
- **AND** 若该目录被意外删除，用户可从备份恢复到相同路径

#### Scenario: 新安装继续读取原有数据

- **WHEN** 用户安装并启动 TimeGenie
- **THEN** 主体、事项、计时记录、工时分配和报告与卸载前一致
- **AND** 原有设备设置与可复用凭据继续生效

#### Scenario: 系统入口完成清理

- **WHEN** 新版本验证完成
- **THEN** 开始菜单和卸载列表中只保留 TimeGenie
- **AND** 不再保留旧的「时序」安装目录或快捷方式
