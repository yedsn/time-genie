# Tasks

## 1. 工程与构建标识

- [x] 1.1 把 `package.json` 的 `name` 改为 `timegenie`，并同步 `package-lock.json` 的包名（优先 `npm install --package-lock-only`；若离线不可用，则按其顶层与 `packages[""]` 两处 `name` 等价修改），验证：`npm pkg get name` 输出 `"timegenie"`，且 `package-lock.json` 中检索旧包名零命中
- [x] 1.2 把 `src-tauri/Cargo.toml` 的 `[package] name` 改为 `timegenie`、`[lib] name` 改为 `timegenie_lib`，并更新 `description` 与 `authors`；同步 `src-tauri/src/main.rs` 中的 lib 引用，验证：`cargo metadata --manifest-path src-tauri/Cargo.toml --no-deps --format-version 1` 中包名为 `timegenie`，且 `cargo check --manifest-path src-tauri/Cargo.toml` 通过
- [x] 1.3 把 `src-tauri/tauri.conf.json` 的 `productName` 与主窗口 `title` 改为「时序」，`identifier` 改为 `com.timegenie.desktop`，验证：读取该文件确认三处取值正确，且 `identifier` 只含字母数字、连字符与句点（Tauri schema 要求）
- [x] 1.4 更新 `src-tauri/capabilities/default.json` 的 `description`，随后执行一次构建让生成物随之刷新，验证：`src-tauri/gen/schemas/capabilities.json` 中不再出现旧描述，且该改动已可被 git 察觉（生成物受版本管理，不得手工编辑）
- [x] 1.5 把 `supabase/config.toml` 的 `project_id` 与 `supabase/schema.sql` 首行注释改为 `timegenie`，验证：`Select-String` 在这两个文件中检索旧标识零命中，且 `schema.sql` 的表结构与 RLS 策略未被改动（`git diff` 只显示首行注释变化）
- [x] 1.6 执行 `npm run tauri:build` 验证 exe 名确实来自 Cargo 包名而非 `productName`，验证：`src-tauri/target/release/timegenie.exe` 存在,且未设置 `mainBinaryName`

## 2. Rust 运行时标识、数据与凭据位置

- [x] 2.1 把 `src-tauri/src/settings.rs` 的 `KEYRING_SERVICE` 常量改为 `timegenie`，验证：读取该常量确认新值，且 `src-tauri/` 下检索 `work-report-assistant` 零命中
- [x] 2.2 把 `src-tauri/src/database.rs` 的 `DATABASE_FILE_NAME` 改为 `timegenie.sqlite3`，验证：常量值为新名，且该名字与第 5 组迁移步骤中实际重命名的文件名一致
- [x] 2.3 把 `src-tauri/src/supabase.rs` 中三处客户端标识字符串改为 `timegenie`，验证：`src-tauri/src/supabase.rs` 中检索旧标识零命中
- [x] 2.4 更新 `src-tauri/src/lib.rs` 的托盘 id **3 处引用**（`TrayIconBuilder::with_id` 创建 1 处，`tray_by_id` 查询 2 处——三者必须同时改，否则 id 不匹配会让查询落空，进而误判"托盘不可见"并启动原生兜底托盘）、托盘 tooltip（改为「时序」）与 `expect` 启动失败文案，验证：`cargo check` 通过，且运行后托盘 tooltip 显示「时序」
- [x] 2.5 把环境变量前缀 `WRA_` 改为 `TG_`：`src-tauri/src/cloud_e2e.rs` 的 5 个变量名（6 处引用）与 `src-tauri/src/lib.rs` 的 `WRA_USE_APP_DATA_DIR`，验证：`cargo check` 通过，且 `src-tauri/` 下检索 `WRA_` 零命中；特别确认 `TG_USE_APP_DATA_DIR` 的读取点已同步，否则 debug 构建会退回真实数据目录
- [x] 2.6 更新 `src-tauri/src/native_tray.rs`：托盘 id 查询（与 2.4 的创建 id 保持一致）、Win32 窗口类名 `work_report_assistant_native_tray` 改为 `timegenie_native_tray`、原生托盘 tooltip 改为「时序」，验证：`cargo check` 通过，且强制走原生兜底托盘路径后，悬停托盘图标显示「时序」而非旧名

## 3. 前端显示名与本地缓存

- [x] 3.1 把 `src-ui/index.html` 的 `<title>` 改为「时序」，验证：`npm run dev` 后浏览器标签页标题为「时序」，且 `dist/index.html` 构建产物中同为「时序」
- [x] 3.2 把 `src-ui/src/views/MainWorkbench.vue` 的窗口标题栏应用名与侧栏品牌块改为「时序」（侧栏副标题由"本地优先工作台"改为"计时 · 工时 · 日报"：原副标题的「本地优先」是架构术语、对目标用户没有信息量，且「工作台」与页面标题 `{{page.label}}工作台` 重复），验证：`npm run typecheck` 通过，界面显示「时序」，且侧栏副标题显示"计时 · 工时 · 日报"
- [x] 3.3 把 `src-ui/src/services/theme.ts` 的缓存 key 改为 `timegenie:app-theme`，验证：切换主题后 localStorage 中只出现新 key 且刷新后主题保持；清除 localStorage 后界面以默认主题正常渲染、不报错，设置页仍可重新选择外观

## 4. 文档与仓库元数据

- [x] 4.1 更新 `README.md` 的标题、首段简介，并把 5 处 `WRA_` 变量示例改为 `TG_`，验证：README 中检索旧标识零命中，且示例中的变量名与 `cloud_e2e.rs` 实际读取的变量名逐一对应
- [x] 4.2 把 `docs/20260924_work-report-assistant_数据结构设计.md` 与 `docs/20260924_work-report-assistant_后端接口设计.md` 改名并更新文档标题，验证：新文件名为 `20260924_timegenie_数据结构设计.md` 与 `20260924_timegenie_后端接口设计.md`，且两份正文中的 `> 对应 OpenSpec change：create-work-report-assistant-desktop-app` **原样保留**，该 id 在 `openspec/changes/archive/` 下真实存在

## 5. 本机数据迁移与凭据重建（人工，须在会话外执行）

- [ ] 5.1 退出旧应用及任何占用应用数据目录的进程，验证：`%APPDATA%\com.workreportassistant.desktop` 可被正常复制（无句柄占用报错）
- [ ] 5.2 复制数据目录并重命名库文件：`%APPDATA%\com.workreportassistant.desktop` → `%APPDATA%\com.timegenie.desktop`，目录内 `work-report-assistant.sqlite3` → `timegenie.sqlite3`，验证：新目录内存在 `timegenie.sqlite3`，大小与旧库一致（迁移前为 606 KB 量级）
- [ ] 5.3 启动改名后的应用核对数据完整性，验证：主体数量与名称、事项数量、时间记录与工时分配、既有报告与迁移前逐项一致；设置中 Obsidian 根目录与日报路径规则、SeaTable 服务地址与各表配置仍可读
- [ ] 5.4 重建系统凭据：重新录入 SeaTable Base API Token、重新登录 Supabase，验证：SeaTable 连接检查通过、云端模式状态正常；若未重建，同步或登录必须给出明确提示而不是静默使用失效凭据
- [ ] 5.5 删除旧安装与旧数据目录，避免两个 `identifier` 长期并存，验证：开始菜单中只剩「时序」一个条目

## 6. 仓库与目录改名（人工，须在会话外执行，最后一步）

- [ ] 6.1 在 GitHub 上把仓库改名为 `timegenie`，验证：访问新地址可用，旧地址按 GitHub 重定向规则仍可访问
- [ ] 6.2 把本地目录 `D:\Workspaces\git\my\work-report-assistant` 改名为 `timegenie`，并执行 `git remote set-url origin <新地址>`，验证：`git remote -v` 指向新地址，`git status` 正常、无 `.git` 损坏
- [ ] 6.3 在新目录下重开会话与编辑器，验证：项目内相对路径（`.agents/skills`、`openspec/`、`src-tauri/`）均可正常读写，`.vscode` 任务可运行
- [ ] 6.4（可选）决定并执行 `timegenie` 相关域名注册与 GitHub 组织创建，验证：如决定注册则域名解析可用，如不注册则该项明确记录为"不执行"，不影响其余任务

## 7. 集成验证

- [x] 7.1 残留检查：用 `git grep -n -i` 对七类品牌模式检索全仓，验证：命中只剩三类白名单——`openspec/` 下本 change 自身的产物文字、`docs/` 两份文档中指向归档 change id 的那 2 行引用、`openspec/changes/archive/` 下的历史内容；源码、配置、以及重新生成后的锁定与生成物中零命中
- [x] 7.2 领域术语回归，验证：设置页日报路径规则默认值仍为 `工作日报/{date}.md`；报告类型筛选仍为「日报」「周报」「月报」；README 第 61–63 行的「原始工时/已分配/待结算」口径定义保持不变；且代码内 `日报` 计数由基线 49 降为 43，**减幅 6 恰好等于被移除的 6 处含「日报」的品牌名**（`lib.rs` 1、`native_tray.rs` 1、`tauri.conf.json` 2、`MainWorkbench.vue` 2），即无一处领域词受损
- [x] 7.3 构建与产物终验：执行 `npm run typecheck` 与 `npm run tauri:build`，验证：两者均通过，产物为 `timegenie.exe`，`tauri.conf.json` 的 `identifier` 已生效，`gen/schemas/capabilities.json` 与 `capabilities/default.json` 内容一致
- [ ] 7.4 端到端验证脚本终验：按 README 用 `TG_` 变量运行 `npm run test:supabase:e2e`（需可丢弃测试账号与已部署 schema），验证：脚本正常执行或跳过；若未设置 `TG_SUPABASE_E2E_ALLOW_RESET=1`，脚本必须给出中文拒绝提示
- [x] 7.5 开发期数据目录终验：以 debug 构建运行应用，验证：未设置 `TG_USE_APP_DATA_DIR` 时使用 `target/dev-data`，且 `%APPDATA%\com.timegenie.desktop` 未被开发运行写入
