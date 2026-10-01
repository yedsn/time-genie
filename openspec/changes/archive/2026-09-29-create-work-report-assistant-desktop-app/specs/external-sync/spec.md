## Purpose

让桌面工作台在本地结构化数据与现有 Obsidian、SeaTable 工作资料之间保持可控同步，使用户可以集中操作，同时继续获得 Markdown 归档、事项表管理和报销汇总能力。

## ADDED Requirements

### Requirement: 写入 Obsidian 日报和计划

系统 SHALL 使用用户配置的 Obsidian 根目录和日报目录写入指定日期的日报、计划或周报文件，并在写入前显示目标路径和内容预览；写入失败时 SHALL 保留本地数据并提供错误原因。

#### Scenario: 创建今日 Obsidian 日报
- **WHEN** 用户确认生成今日 Obsidian 日报且目标文件不存在
- **THEN** 系统按照配置的 Markdown 模板创建目标文件，并报告实际写入路径

#### Scenario: 目标文件已存在
- **WHEN** 用户生成日报但目标 Markdown 文件已经存在
- **THEN** 系统展示差异或覆盖提示，用户确认后才覆盖或合并，不得静默丢弃原文件内容

### Requirement: 从 Obsidian 读取计划

系统 SHALL 兼容现有日报中以日期标题、编号事项、缩进子事项和“明日计划/下周一计划”组织的 Markdown 内容，并将无法识别的内容保留为可查看的导入提示。

#### Scenario: 读取带父子事项的计划
- **WHEN** 用户导入包含编号父事项和缩进子事项的明日计划
- **THEN** 系统建立对应层级的候选事项，并保留原始文本供用户确认

#### Scenario: Markdown 格式不完整
- **WHEN** 日报中存在无法解析的计划行或非标准格式
- **THEN** 系统导入可识别部分，同时列出未识别行，不得静默删除或伪造事项

### Requirement: 同步今日事项到 SeaTable

系统 SHALL 支持将今日计划、事项状态、主体、方案、项目和最终分配工时同步到用户配置的 SeaTable 表；同步操作 SHALL 可预览、可重复执行，并避免按事项名称产生重复 TODO。

#### Scenario: 首次同步今日事项
- **WHEN** 用户确认将今日计划同步到 SeaTable
- **THEN** 系统创建缺失事项并更新对应日期、状态和最终用时，且显示新增、更新和跳过的数量

#### Scenario: 重复同步同一事项
- **WHEN** 用户对同一天重复执行 SeaTable 同步
- **THEN** 系统根据稳定事项身份或可确认的匹配规则更新已有记录，不得为同一事项无提示地创建重复记录

### Requirement: 处理 SeaTable 不可用

系统 SHALL 将 SeaTable 连接失败、认证失败、表结构不匹配和单条记录更新失败分别显示，并确保同步失败不会回滚或删除本地计划、计时和工时分配数据。

#### Scenario: SeaTable 网络或认证失败
- **WHEN** 用户同步时 SeaTable 不可访问或 API Token 无效
- **THEN** 系统显示失败原因，将本地同步状态标记为待重试，并允许用户稍后重新同步

#### Scenario: 部分事项同步失败
- **WHEN** SeaTable 批量同步中部分事项成功而部分事项失败
- **THEN** 系统展示每条事项的结果和失败原因，并允许只重试失败事项

### Requirement: 保护外部服务凭据

系统 SHALL 将 SeaTable API Token 等敏感配置保存在本地受保护的应用配置中；导出普通工作数据、日报和周报时不得包含完整 Token。

#### Scenario: 导出工作数据
- **WHEN** 用户导出本地数据备份或日报周报内容
- **THEN** 导出文件不包含 SeaTable API Token 或其他完整敏感凭据
