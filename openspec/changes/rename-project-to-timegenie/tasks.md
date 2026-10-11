# Tasks

## 1. 已完成的技术标识统一

- [x] 1.1 npm 包名、Cargo 包名与 Rust lib 名统一为 timegenie / timegenie_lib
- [x] 1.2 Tauri identifier 统一为 com.timegenie.desktop
- [x] 1.3 数据库文件、系统凭据、Supabase 客户端与 schema 标识统一为 timegenie
- [x] 1.4 环境变量前缀统一为 TG_，主题缓存 key 统一为 timegenie:app-theme
- [x] 1.5 仓库、发布镜像和更新资产已使用 TimeGenie/time-genie 命名

## 2. 系统与打包名称改为英文

- [x] 2.1 将 src-tauri/tauri.conf.json 的 productName 改为 TimeGenie，验证配置读取结果为英文产品名
- [x] 2.2 在 src-tauri/tauri.conf.json 显式设置 mainBinaryName 为 TimeGenie，同时保持 Cargo 包名为 timegenie
- [x] 2.3 将主窗口的系统 title 改为 TimeGenie；保留悬浮计时窗口的功能标题「今日计时」
- [x] 2.4 将 src-tauri/src/lib.rs 和 src-tauri/src/native_tray.rs 的托盘 tooltip 同步改为 TimeGenie，托盘菜单命令继续使用中文
- [ ] 2.5 执行 Windows release 打包，验证实际产物为 TimeGenie.exe、安装包使用 TimeGenie 前缀，并检查安装目录、开始菜单和卸载项的预期名称
- [ ] 2.6 在可用的 macOS 构建环境或 CI 中验证应用包为 TimeGenie.app，DMG 与更新归档使用英文名称

## 3. 中文应用界面保持不变

- [x] 3.1 确认 src-ui/src/views/MainWorkbench.vue 的自定义标题栏和侧栏品牌块继续显示「时序」
- [ ] 3.2 确认系统名称调整没有改动计划、计时、报告、设置和托盘菜单中的中文操作文案
- [ ] 3.3 回归检查「日报/周报/月报」「工作日报/{date}.md」「已分配/待结算」等领域术语与默认值

## 4. 发布流程与文档名称

- [x] 4.1 将 .github/workflows/release.yml 的 Release 名称、说明和 macOS 路径示例改为 TimeGenie
- [x] 4.2 将 scripts/release/release.ps1 的 $AppName 改为 TimeGenie
- [ ] 4.3 确认 GitHub/Gitee 安装包、更新归档和签名文件继续统一使用 TimeGenie 前缀，且 latest.json 能引用新产物名称
- [x] 4.4 将 README 和文档站的产品标题调整为英文优先的 TimeGenie（时序），正文可继续使用中文说明

## 5. 稳定身份与数据连续性检查

- [ ] 5.1 确认 identifier 仍为 com.timegenie.desktop，没有新增或修改应用数据目录配置
- [ ] 5.2 确认数据库文件名仍为 timegenie.sqlite3，凭据 service/target、Supabase schema 与 updater endpoints 均无变化
- [ ] 5.3 执行差异检查，确保本次实现不包含数据库迁移、凭据迁移、云端 schema 变更或自动卸载逻辑

## 6. 构建与回归验证

- [ ] 6.1 执行 npm run typecheck
- [ ] 6.2 执行 cargo fmt --manifest-path src-tauri/Cargo.toml -- --check 与 cargo check --manifest-path src-tauri/Cargo.toml
- [ ] 6.3 执行 npm run tauri:build，记录实际安装包、主程序和 updater artifact 名称
- [ ] 6.4 启动构建结果，验证应用内显示「时序」、系统窗口与托盘显示 TimeGenie
- [ ] 6.5 检查工作区中与本变更无关的在途功能改动未被覆盖或混入命名修改

## 7. 单用户人工迁移（代码验证完成后执行）

- [ ] 7.1 完全退出旧版「时序」，备份 %APPDATA%\com.timegenie.desktop，记录 timegenie.sqlite3 的大小与修改时间
- [ ] 7.2 从 Windows 卸载列表卸载旧版「时序」，确认旧进程、安装目录和卸载项已移除
- [ ] 7.3 确认 %APPDATA%\com.timegenie.desktop\timegenie.sqlite3 仍存在；若被删除，则从备份恢复到原路径
- [ ] 7.4 安装新构建的 TimeGenie，确认默认安装目录、开始菜单和卸载项均使用英文名称
- [ ] 7.5 启动应用并核对主体、事项、计时记录、工时分配、报告、设备设置、SeaTable 和 Supabase 登录状态
- [ ] 7.6 确认系统中只剩 TimeGenie，清理残留的「时序」快捷方式或空安装目录
- [ ] 7.7 完成至少一次正常启动、退出和更新检查后，再删除迁移备份
