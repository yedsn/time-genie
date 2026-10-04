# Verification

## 结论

本变更的本地实现、Rust 单元与集成测试、前端类型与展示层测试、Supabase schema 集成测试均已通过。真实 Supabase 双设备端到端测试因当前环境未提供所需测试项目和一次性清理授权而未执行；未将其记录为通过。

## 已通过检查

- `cargo fmt --manifest-path src-tauri/Cargo.toml`
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`：177 passed，0 failed，1 ignored；忽略项为需要专用 Supabase 测试账号的 `real_supabase_two_device_flow`。
- `cargo test --manifest-path src-tauri/Cargo.toml cloud_e2e::real_supabase_two_device_flow --no-run`：真实 E2E 场景编译通过。
- `npm run typecheck`
- `npm run test:cloud-session-presentation`
- `npm run test:cloud-device-presentation`
- `npm run test:cloud-sync-presentation`
- `npm run test:supabase:schema`：完整 schema、版本化补丁及 SQL 集成测试通过。
- `git diff --check`：通过，仅有 Windows 换行转换提示。

## 关键覆盖

- 会话重启恢复、refresh token 轮换、凭据写入失败保留旧记录、凭据库暂不可用映射为 `offline_saved`。
- 断网、429、5xx、刷新响应解析失败、未知 401/403 不删除会话；`invalid_grant`、用户禁用、Auth 会话不存在和设备撤销进入重新登录。
- 可安全重放请求刷新后只重试一次；不可重放请求只刷新认证并返回 `AUTH_RETRY_REQUIRED`。
- token 刷新成功后立即复核当前设备授权，在业务请求恢复前识别 `DEVICE_REVOKED`。
- 设备 RPC 的账号归属、最小权限、重复撤销、租约释放、旧 Auth 会话不能重新授权以及错误脱敏。
- SQL 集成测试直接证明持有有效用户身份的已撤销设备不能调用 `cloud_apply_patch`，同时未撤销设备仍可上传。
- 当前设备退出与全设备退出的调用顺序、部分失败安全状态、本地 SQLite 与 outbox 保留；即使退出前已切到本地模式，仍会撤销已配置云工作空间中的当前设备授权并保留受阻提示。
- 重新登录后 outbox 仍受阻，只有显式调用恢复命令才允许上传。
- 前端三种会话状态、设备列表、单设备撤销、当前设备退出、全设备二次确认及取消确认不调用后端。
- 双设备 E2E 场景代码覆盖重启恢复、断网保留、token 自动刷新、撤销设备 B 不影响 A、B 保留本地数据/outbox、显式恢复、全设备退出和外部 Auth 撤销。

## 真实 Supabase E2E

执行 `npm run test:supabase:e2e` 后，脚本因缺少以下环境变量安全退出：

- `TG_SUPABASE_URL`
- `TG_SUPABASE_ANON_KEY`
- `TG_SUPABASE_EMAIL`
- `TG_SUPABASE_PASSWORD`
- `TG_SUPABASE_E2E_ALLOW_RESET`

状态：**未执行（环境未提供）**。测试会清理专用测试账号的工作空间，只有配置可丢弃账号并显式设置 `TG_SUPABASE_E2E_ALLOW_RESET=1` 后才会运行。

## 兼容性与敏感信息检查

- 新凭据统一使用 keyring service `timegenie` 和稳定账户键 `supabase:session`；旧 workspace 范围账户键可读取并回写稳定键。
- 旧产品名 keyring service 按既有改名方案不自动读取，因此升级后只需一次重新登录；新 `timegenie` service 登录后可以长期恢复。
- 仓库扫描未发现真实 Supabase 项目 URL、JWT、service-role key、账号密码或完整 access/refresh token。命中项仅为字段名、测试占位值、环境变量名、SQL 角色名和文档示例。
- 密码不持久化；完整 token 只保存于系统凭据库，不进入会话快照、SQLite 业务导出或普通日志。
