# Spec Delta

## Purpose

定义应用的对外显示名、工程与构建产物标识、本机数据与凭据的存放位置，以及在品牌改名过程中必须保持不变的领域术语与用户既有约定，使改名后的应用对用户、操作系统与外部服务都呈现为一个完整且连续的身份。

## ADDED Requirements

### Requirement: 应用显示名一致性

所有用户可见的应用名称位置 SHALL 使用中文显示名「时序」，包括主窗口标题、窗口标题栏、侧栏品牌块、系统托盘提示、安装包名称、开始菜单条目与桌面快捷方式名称。

英文产品名 `TimeGenie` SHALL 仅用于工程标识与网络标识；它 MAY 出现在 README、关于页等允许双语并列的位置，但 MUST NOT 替代用户可见的应用名称。

#### Scenario: 主窗口与标题栏展示中文名

- **WHEN** 用户启动应用并显示主窗口
- **THEN** 主窗口标题为「时序」
- **AND** 窗口标题栏中的应用名文字为「时序」

#### Scenario: 托盘提示展示中文名

- **WHEN** 用户把鼠标悬停在系统托盘图标上
- **THEN** 提示文字为「时序」

#### Scenario: 安装后系统入口展示中文名

- **WHEN** 用户完成安装并在开始菜单中查找该应用
- **THEN** 开始菜单条目名称为「时序」
- **AND** 安装包文件名与安装目录名称体现「时序」

### Requirement: 工程与构建产物标识统一

应用的工程标识 SHALL 统一为 `timegenie`，覆盖 npm 包名、Cargo 包名与 lib 名、可执行文件名、bundle identifier、Supabase `project_id` 与 Supabase 客户端标识字符串。

端到端验证所使用的环境变量前缀 SHALL 统一为 `TG_`。代码库中 MUST NOT 残留 `work-report-assistant`、`work_report_assistant`、`Work Report Assistant`、`com.workreportassistant` 或 `WRA_` 作为应用标识出现。

#### Scenario: 构建产物使用新标识

- **WHEN** 执行 Windows release 构建
- **THEN** `src-tauri/target/release/` 下的可执行文件名为 `timegenie.exe`

#### Scenario: bundle identifier 变更生效

- **WHEN** 检查 `src-tauri/tauri.conf.json`
- **THEN** `identifier` 为 `com.timegenie.desktop`
- **AND** `productName` 体现中文显示名「时序」

#### Scenario: 端到端验证环境变量使用新前缀

- **WHEN** 用户按 README 说明配置双设备端到端验证
- **THEN** README 中列出的变量名为 `TG_SUPABASE_URL`、`TG_SUPABASE_ANON_KEY`、`TG_SUPABASE_EMAIL`、`TG_SUPABASE_PASSWORD`、`TG_SUPABASE_E2E_ALLOW_RESET`
- **AND** 未设置 `TG_SUPABASE_E2E_ALLOW_RESET=1` 时测试拒绝执行并给出中文提示

### Requirement: 本机数据与凭据的迁移

本次变更 SHALL 交付一套可执行且可验证的一次性迁移步骤，使改名后的应用能在新位置继续使用已有的本机工作区数据。

迁移范围 SHALL 覆盖 SQLite 数据库文件，以及设置中的设备级配置；系统凭据库中的 SeaTable Base API Token 与 Supabase 用户会话因 service 名变更无法自动沿用，SHALL 被明确标注为需要重新录入或重新登录。

#### Scenario: 迁移后工作区数据完整

- **WHEN** 按迁移步骤把旧应用数据目录搬迁到新目录后启动应用
- **THEN** 主体的数量与名称、事项数量、时间记录与工时分配与迁移前一致
- **AND** 已生成的报告仍可在报告页中打开

#### Scenario: 迁移后设备级配置保留

- **WHEN** 迁移完成并进入设置页
- **THEN** Obsidian 根目录与日报路径规则仍为迁移前配置的值
- **AND** SeaTable 服务地址、事项表、事项视图与报销表配置仍可读

#### Scenario: 凭据需重新建立

- **WHEN** 迁移完成后首次使用 SeaTable 同步或 Supabase 云端模式
- **THEN** 应用提示需要重新录入 Base API Token 或重新登录，而不是静默使用失效凭据

#### Scenario: 未迁移时的行为被显式说明

- **WHEN** 用户未执行迁移即启动改名后的应用
- **THEN** 应用以空白工作区正常启动
- **AND** 该现象在迁移说明中被写清，避免被误判为数据丢失

### Requirement: 领域术语与既有用户约定保持不变

品牌改名 MUST NOT 改变报告类型词「日报」「周报」「月报」，MUST NOT 改变 Obsidian 日报路径规则的默认值 `工作日报/{date}.md`，也 MUST NOT 改变工时结算术语「原始工时」「已分配」「待结算」及其口径定义。

用户既有的 Obsidian 目录结构与默认路径约定 MUST NOT 因改名被改写。

#### Scenario: 报告类型词不变

- **WHEN** 用户在报告页查看类型筛选
- **THEN** 选项仍为「日报」「周报」「月报」

#### Scenario: Obsidian 默认路径规则不变

- **WHEN** 用户在全新环境中打开设置页的日报路径规则
- **THEN** 默认值仍为 `工作日报/{date}.md`

#### Scenario: 工时结算术语不变

- **WHEN** 用户查看 README 中的工时口径说明
- **THEN** 「原始工时」「已分配」「待结算」三个术语及其定义（待结算工时等于原始工时减去已分配工时）保持不变
- **AND** 界面上「已分配」与「待结算」的文案保持不变

#### Scenario: 领域词计数减幅可完全归因

- **WHEN** 对改名前后在 `src-ui` 与 `src-tauri` 内检索 `日报`
- **THEN** 计数由 49 降为 43
- **AND** 减少的 6 处恰好是被移除的含「日报」的品牌名，不含任何报告类型词或路径规则

### Requirement: 本地外观缓存可重建

主题偏好等外观相关本地缓存在存储键变更后 SHALL 回落为默认外观，MUST NOT 导致界面无法渲染或报错；用户 SHALL 能在设置页重新选择外观。

#### Scenario: 升级后界面正常渲染

- **WHEN** 存储键变更后用户首次启动应用
- **THEN** 应用以默认外观正常渲染全部页面
- **AND** 设置页的外观选项可正常切换并持久化
