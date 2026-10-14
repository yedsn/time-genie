# Proposal

## Why

当前 Supabase 会话虽然已经保存 refresh token，但部分临时错误会被折叠为“未登录”或直接清除本机凭据，导致用户需要反复输入账号密码。TimeGenie 需要把“暂时无法联网验证”和“会话已被明确撤销”区分开，同时提供可控的单设备与全设备退出能力。

## What Changes

- 用户首次使用邮箱密码登录后，将会话安全保存在系统凭据库，并在应用重启、系统重启和 access token 到期后自动恢复或续期。
- 将云端账号状态区分为已登录、离线但保留登录、需要重新登录；断网、超时、限流、Supabase 暂时故障、Schema/RLS/业务错误不得误删会话。
- 只有用户手动退出、refresh token 被 Supabase 明确拒绝、账号被禁用或会话被明确撤销时，才清除本机保存的会话。
- 增加设备会话管理：查看账号下的设备，撤销指定设备，以及撤销全部设备；当前设备和其他设备均可成为撤销目标。
- 被撤销设备在下一次启动、会话检查或同步时停止云端访问、清除本机会话并提示重新登录；检测到撤销后可以继续查看本地缓存，但不得继续产生或上传待上云的业务修改。
- 手动退出默认只退出当前设备，并使当前设备的云端授权失效；“退出所有设备”必须二次确认并包括发起操作的当前设备。
- 支持管理员在 Supabase 后台撤销设备授权或 Auth 会话；客户端在下一次联网校验时识别撤销结果，不要求再次输入密码以外的恢复操作。

## Capabilities

### New Capabilities

- `cloud-session-lifecycle`: 定义 Supabase 长期登录、自动续期、离线保留、明确失效判定、设备列表以及单设备/全设备会话撤销行为。

### Modified Capabilities

无。当前项目尚无已发布的主规格。

## Impact

- Tauri 后端：`src-tauri/src/supabase.rs` 的会话存储、刷新、错误分类、登出与设备管理命令。
- 云端同步：启动恢复、同步前授权检查、离线写入限制及撤销后的本地状态处理。
- Vue 前端：`src-ui/src/views/MainWorkbench.vue`、`src-ui/src/services/tauri.ts` 中的账号状态展示、设备管理和确认交互。
- Supabase：`supabase/schema.sql` 及匹配的增量 SQL，增加受 RLS/RPC 保护的设备撤销与全设备撤销能力；桌面端仍只使用 anon key 和用户会话，不引入 service_role key。
- 验证：Rust 单元/集成测试、前端状态展示测试和 Supabase 双设备端到端场景。
