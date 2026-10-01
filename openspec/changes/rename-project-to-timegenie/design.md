# Design

## Context

动机见 `proposal.md` 的 Why 一节，此处只列影响实现方式的现状事实（均为本次实地核实，非推测）：

1. **品牌标识分散在 51 行、22 个文件中。** 口径：在实施开始时用 `git grep -i` 对连字符形式、下划线形式、带空格的英文名、反向域名、`WRA_` 前缀、驼峰形式与中文显示名七类模式清点，排除 `openspec/` 后命中 51 行 / 22 文件；其中 2 行是必须保留的归档 change id，另有 4 行位于自动生成或锁定文件。清点经多轮才收敛，而**遗漏形态本身就是本变更要防的主要风险**：
   - 第一轮只搜连字符形式，漏掉 `src-ui/index.html` 的 `<title>Work Report Assistant</title>`、`capabilities/default.json` 的描述、`supabase/schema.sql` 首行注释、`Cargo.toml` 的 `description` 与 `authors`、`lib.rs` 的启动失败文案。
   - 随后补搜带空格形式，再查出 `lib.rs` 的 `WRA_USE_APP_DATA_DIR`——它不是文案，而是控制 debug 构建是否改用 `target/dev-data` 而非真实数据目录的开关，漏改会让开发运行直接写到生产数据目录。
   - **清点是快照而非常量。** 初稿清点为 43 行 / 19 文件；从规划到实施之间代码库前进了 3 个提交（`6d877fd`、`0619d36`、`a8af468`，跨越约 2 小时），期间新增了 `src-tauri/src/native_tray.rs` 且 `lib.rs` 增长，实施开始时的真实数字变为 51 行 / 22 文件。**结论：全量替换类任务必须在实施开始时重新清点，不能沿用规划期数字**，本次实施即按此执行。
   - **`native_tray.rs` 是 Tauri 托盘之外的第二套托盘实现，规划期完全遗漏。** 它在 `start_if_tauri_tray_missing` 中等待 8 秒，若 `tray_by_id` 判定 Tauri 托盘不可见，就注册一个原生 Win32 托盘窗口：窗口类名 `work_report_assistant_native_tray`、tooltip 为独立的 `工作日报助手`。**后果**是只改 `lib.rs` 会让 Tauri 托盘失效的机器仍显示旧名，并残留旧 Win32 类名。因此托盘 id 需在 5 处保持一致（`lib.rs` 3 处、`native_tray.rs` 2 处），tooltip 需在 2 处同时改为「时序」。
2. **标识符决定数据位置。** Tauri 2.11 配置 schema 对 `identifier` 的说明为 "used in system configurations like the bundle ID and path to the webview data directory"；本机 `app_data_dir()` 实际解析到 `%APPDATA%\com.workreportassistant.desktop`，其中有一个 606 KB 的 `work-report-assistant.sqlite3`（末次写入 2026-09-29）。
3. **exe 名来自 Cargo 包名，不是 `productName`。** `src-tauri/target/release/` 下是 `work-report-assistant.exe`，而 `productName` 已经是中文「工作日报助手」，两者不一致正好证明来源不同。
4. **生成物已入库。** `git ls-files src-tauri/gen` 确认 `acl-manifests.json`、`capabilities.json`、`desktop-schema.json`、`windows-schema.json` 均受版本管理，且 `capabilities.json` 由 `src-tauri/capabilities/default.json` 生成。
5. **主 spec 为空。** `openspec list --specs` 返回空数组，`openspec/specs/` 只有 `.gitkeep`；归档中存在 6 个能力目录（`daily-workbench`、`time-tracking-and-allocation`、`report-generation`、`external-sync`、`task-daily-planning`、`task-completion-feedback`），但均未同步为主 spec，且其功能需求不因改名变化。
6. **领域词与品牌词的密度差异极大。** 代码内（`src-ui` + `src-tauri`）`日报` 共出现 49 次，其中 6 处是品牌名「工作日报助手」（`lib.rs`、`native_tray.rs`、`tauri.conf.json` ×2、`MainWorkbench.vue` ×2），其余都是报告类型词与 Obsidian 默认目录 `工作日报/{date}.md`。改名后 `日报` 计数由 49 降为 43，**减幅正好等于这 6 处品牌名**，可逐处归因。另需注意「原始工时」「已分配」「待结算」是 README 中的口径定义（第 61–63 行）而非界面指标名，界面上的对应文案是「已分配」「待结算」，今日页概览的四个卡片则是「今日投入/今日预计/完成率/未归属」。这个比例与用词分布决定了不能对 `日报` 做任何形式的批量替换。
7. **本机数据目录只有一处。** `%APPDATA%` 下匹配 `report|assistant|work` 的目录只有一个，即上述应用数据目录；应用尚无外部用户。

## Goals / Non-Goals

**Goals:**

- 把 51 行、22 个文件中的品牌标识收敛为一条明确映射：用户可见位置用中文显示名「时序」，工程与网络标识用 `timegenie`。
- 在应用尚未发布、不存在外部用户的窗口内一次性完成，交付物为改名后的代码与配置，外加一套可验证的人工迁移步骤。
- 保证领域术语与用户既有的 Obsidian 目录约定零改动（`日报/周报/月报`、`工作日报/{date}.md`、`原始工时/已分配/待结算`）。
- 让改名后的应用在操作系统与外部服务层面表现为**一个完整且连续的身份**：产物名、identifier、凭据 service、数据库文件名、Supabase 客户端标识互相一致。

**Non-Goals:**

- 不实现旧数据目录的自动探测、搬迁或提示逻辑（见决策 D3）。
- 不改写归档 change 的 id 与内容（见决策 D4）。
- 不做版本号变更，`0.1.0` 保持不变（尚未首次发布，无版本语义需求）。
- 不改动 `supabase/schema.sql` 的业务表结构、字段与 RLS 策略，只改首行注释中的产品名。
- 不在本变更内注册域名或创建独立的 GitHub 组织（属仓库外动作，仅在 tasks 中记录为可选人工步骤）。
- 不引入任何新的运行时依赖。

## Decisions

### D1：采用"中英双名并行"，而非单一名称

用户可见位置统一用中文「时序」，工程与网络标识统一用 `timegenie`（英文产品名 `TimeGenie`）。

- **理由**：已确认目标用户是普通职场人，他们认不出也不易记住英文名；而公开发布又需要 ASCII、可输入、唯一性好的仓库名与包名。两个目标冲突，故由两个名字分别承担——与微信/WeChat、飞书/Lark 同构。
- **备选一：只用英文名。** 对普通职场人构成认知门槛，安装包与开始菜单显示英文会降低识别度。
- **备选二：只用中文名。** 仓库名、crate 名、identifier、域名无法统一到同一个可输入标识上，且中文 exe 名会给脚本与 CI 带来麻烦。

### D2：exe 名依靠 Cargo 包名，不使用 `mainBinaryName`

只把 `Cargo.toml` 的包名改为 `timegenie`，不设置 `mainBinaryName`。

- **理由**：现状已证明 exe 名来自 Cargo 包名（Context 第 3 条）。Tauri 2.11 schema 对 `mainBinaryName` 的说明是 "By default, Tauri uses the output binary from `cargo`"，即它只在需要覆盖时使用。包名与期望的 exe 名一致时，该配置是多余的。
- **备选：设置 `mainBinaryName: "timegenie"` 而保留旧包名。** 会留下"包名叫 `work-report-assistant`、产物叫 `timegenie`"的不一致，并新增一处需要长期维护的配置项。

### D3：交付一次性人工迁移步骤，不编写自动迁移代码

- **理由**：本机只有一个数据目录、一个开发机、零外部用户。为单一使用者编写"探测旧目录 → 搬迁 → 校验"的自动逻辑，会让一段一次性代码永久留在产品里，并在发布后成为需要兼容的路径。人工迁移一次即可，且步骤完全可验证。
- **备选：启动时探测旧数据目录并提示。** 需要新增 Rust 侧探测逻辑、用户可见文案与对应测试，收益仅覆盖一个已经拿到迁移说明的使用者。若将来出现真实外部用户，应以独立变更引入，而不是现在预埋。
- **已接受的后果**：未迁移时应用以空白工作区启动（不报错、不提示）。该行为已作为 spec 场景显式记录，并要求在迁移说明中写清，避免被误判为数据丢失。

### D4：归档 change 不改写，历史文档只改文件名与标题

- **做法**：`openspec/changes/archive/2026-09-29-create-work-report-assistant-desktop-app/` 的目录 id 与内容保持原样；`docs/20260924_work-report-assistant_*.md` 两份文档改为按新标识命名并更新文档标题，但正文中 `> 对应 OpenSpec change：create-work-report-assistant-desktop-app` 的引用**必须保留**。
- **理由**：归档记录是历史事实，改写会破坏可追溯性，且会让归档目录 id 与其中的 spec 引用自相矛盾；而文档文件名是对外可见的组织形式，长期保留旧品牌名会造成困惑。两类对象按不同标准处理，取舍边界是"历史记录不改写，对外组织可更新"。
- **风险控制**：改写文档标题时若连带改掉 change id 引用，会指向一个不存在的 change，因此该引用列为实施时的显式检查项。

### D5：`src-tauri/gen/schemas/*` 以重新生成为准，不手工编辑

- **做法**：只修改源文件 `src-tauri/capabilities/default.json`，随后执行一次构建让 `gen/schemas/capabilities.json` 重新生成，并确认变更已入库。
- **理由**：这些文件受版本管理（Context 第 4 条），必须与源文件一致；手工编辑生成物会造成下一次构建覆盖掉人工改动。
- **备选：把 `gen/` 移出版本管理。** 属于额外的基础设施改动，与本变更目标无关，会扩大影响面。
- **实施期实测**：编辑 `capabilities/default.json` 后，在尚未手工执行构建的情况下，`gen/schemas/capabilities.json` 与 `Cargo.lock` 已被一个 cargo 构建监听自动重生成，且内容与源文件一致。这既验证了"以重新生成为准"可行，也说明锁定文件的包名更新可完全交给工具完成，无需手工编辑。

### D6：主题缓存 key 直接更换，不做读取回退

- **做法**：`work-report-assistant:app-theme` → `timegenie:app-theme`，不保留对旧 key 的兼容读取。
- **理由**：主题偏好是一次性外观状态，用户重新选择一次即可；为它保留双 key 读取会引入一段永久兼容代码，收益低于成本。
- **已接受的后果**：升级后外观回落为默认。spec 要求界面必须能正常渲染而不是报错，并允许在设置页重新选择。

### D7：环境变量前缀 `WRA_` → `TG_`

- **范围**：5 个变量名、12 处引用——`README.md` 5 处、`cloud_e2e.rs` 6 处（`WRA_SUPABASE_E2E_ALLOW_RESET` 出现 2 次）、`lib.rs` 1 处。
- **理由**：`TG` 与 `timegenie` 对应，比 `WRA` 短且与项目标识一致；这些变量只服务本地端到端验证与开发期数据目录切换，没有外部兼容负担。
- **注意：`WRA_USE_APP_DATA_DIR` 是其中唯一带行为含义的变量**（`lib.rs` 是它全仓唯一读取点，README 未提及）。改名时必须同步该读取处，否则开发运行会退回真实数据目录，进而在验证阶段污染生产数据。
- **备选：保留 `WRA_`。** 会留下一个无法从新名称推导出来的缩写，成为后续所有读者的疑问点。

## Risks / Trade-offs

- **改名后新旧版本在系统层面并存** → 两个 `identifier` 会被 Windows 视作两个独立应用，旧安装不会自动升级。缓解：迁移完成后删除旧安装；该步骤写入 tasks，并在迁移说明中提示。
- **系统凭据无法自动沿用** → SeaTable Base API Token 与 Supabase 会话存在旧 service 名下，改名后读不到。缓解：spec 要求应用在同步或登录时提示重新录入，而不是静默使用失效凭据；tasks 中列出重新录入步骤。
- **生成物与源文件不一致** → 只改 `capabilities/default.json` 而不构建，会导致入库的 `gen/schemas/capabilities.json` 仍含旧描述。缓解：把"构建后确认生成物已更新"列为独立验证项。
- **领域词被误伤** → 代码内 `日报` 出现 49 次而其中品牌名只有 5 处，批量替换极易破坏用户 Obsidian 目录约定。缓解：实施时以白名单为准（报告类型词、路径规则默认值、结算术语三类一律不动），spec 中已为三类各设场景；残留检查以 `git grep` 逐处对照，不做正则批量替换。
- **本地目录改名中断当前工作会话** → 改名会使活跃会话的工作目录失效，进而影响编辑器工作区与项目内相对路径。缓解：该步骤排在全部代码改动与验证之后，在会话外执行，并在完成后重开会话。
- **`productName` 为中文导致安装包文件名为中文** → 对中文用户可接受，但不利于脚本引用。缓解：这是有意取舍（用户可见位置用中文），exe 名保持 ASCII 已由 D2 保证，脚本应引用 exe 名而非安装包名。
- **回滚是对称的** → 由于应用尚未发布，回滚只需把标识改回旧值并恢复旧数据目录，不涉及外部用户与版本兼容。这是选择在发布前执行本次变更的核心收益。

## Migration Plan

按依赖顺序执行，前四步在代码库内完成，后两步涉及本机环境与仓库外操作：

1. **代码与配置改名**：按 proposal 的 Impact 清单逐文件替换五类模式，`日报` 等三类领域词保持不动。
2. **构建与静态验证**：`npm run typecheck`、`npm run tauri:build`；确认 `target/release/timegenie.exe` 存在、`tauri.conf.json` 的 `identifier` 生效、`gen/schemas/capabilities.json` 已随源文件更新。
3. **领域词回归检查**：确认设置页日报路径规则默认值仍为 `工作日报/{date}.md`、报告类型筛选仍为日报/周报/月报、今日页指标仍为原始工时/已分配/待结算。
4. **文档与引用检查**：确认两份 docs 文件名与标题已更新，且正文中 `create-work-report-assistant-desktop-app` 的引用仍指向归档中真实存在的 change。
5. **本机数据迁移**：退出旧应用 → 复制 `%APPDATA%\com.workreportassistant.desktop` 为 `%APPDATA%\com.timegenie.desktop` → 把库文件重命名为 `timegenie.sqlite3` → 启动新版本，核对主体、事项、时间记录、分配、报告的完整性，以及设置中的 Obsidian 与 SeaTable 配置。
6. **凭据重建与收尾**：重新录入 SeaTable Base API Token、重新登录 Supabase 并核对云端模式；随后删除旧安装；最后执行仓库外动作（GitHub 仓库改名 → 本地目录改名 → `git remote set-url` → 重开会话）。

**回滚策略**：因应用尚未发布，回滚为对称操作——把标识改回 `work-report-assistant` / 「工作日报助手」、恢复旧数据目录、`git remote set-url` 指回旧仓库名。不涉及外部用户通知、版本降级或数据格式转换。

## Open Questions

以下两项均可在实施后单独决定，不会改变 spec、方案或任务拆分：

- 是否注册 `timegenie` 相关域名，以及是否为其创建独立的 GitHub 组织（当前沿用个人账号 `yedsn` 亦可）。tasks 中仅记录为可选步骤。
- 公开发布渠道与首发版本号（当前保持 `0.1.0`）。属于发布决策，不属于本次改名范围。
