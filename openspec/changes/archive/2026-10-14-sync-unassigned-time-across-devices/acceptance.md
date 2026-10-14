# 双桌面实例验收记录

## 环境

- 日期：2026-10-06（Asia/Shanghai）
- 客户端：两个真实 Tauri debug 桌面实例，隔离目录 `device-a5`、`device-b5`
- 云端：本机验收服务复现 Supabase RPC、增量游标和 Realtime 提醒语义
- 最终状态：A/B `last_change_seq=31`，`sync_outbox=0`，冲突为 0，界面均显示“所有更改均已保存”
- 真实 Supabase：补丁已部署到 `timegenie` Schema，数据库与 PostgREST 健康，Schema cache 已重载

## 场景与证据

### 1. 共享累计与阈值竞争

- A/B 首次采用同一个 session：`01a1104f-eef2-7118-aa48-814daa68410f`。
- 两端初始 version 均为 `1`，`first_started_at=1791275036402`。
- 两端同时观察到阈值后，最终共同采用 `awaiting_resolution` version `3`，没有留下生命周期冲突或待保存项。

### 2. 分配到事项

- session：`01a1104f-eef2-7118-aa48-814daa68410f`，终态 version `4`。
- `resolution_type=work`，`resolved_at=1791275208559`。
- 唯一最终工时：`01a11052-8ef4-762b-84b3-1bb86bd193be`。
- A/B 均看到相同事项、相同工时及同一条 8 分钟 allocation，没有重复记录。
- 下一 session 从 `1791275208559` 这一服务端边界继续。

### 3. 休息时间传播

- session：`01a11052-8ef5-7221-a7da-3c71a92a82e2`，终态 version `4`。
- `resolution_type=break`，`resolved_at=1791275460421`。
- 唯一最终记录：`01a11056-6698-76d5-b5d5-dc9b69aaec9e`，标签为“休息时间”。
- A/B 都停止旧累计，并采用同一后续会话边界。

### 4. 无效时间传播

- session：`01a11057-73d4-7760-b047-abf38933983b`，终态 version `4`。
- `resolution_type=discard`，`resolved_at=1791275584269`。
- 唯一最终记录：`01a11058-4a61-76aa-a00c-e3cb2bc5dd73`，标签为“无效时间”。
- A/B 均采用相同终态与后续会话边界。

### 5. 另一设备正在事项计时

- B 启动事项计时：`01a11053-ad6b-7370-b642-8c8238a8aa70`；A/B 均显示同一 running 记录。
- A 处理旧未归属会话后，两端都只清除旧会话，没有建立下一未归属会话。
- 事项结束于 `1791275529171` 后，两端才共同创建 session `01a11057-73d4-7760-b047-abf38933983b`，其 `first_started_at` 精确等于事项结束边界。

### 6. 退出期间处理与恢复

- B 退出时，session `01a11057-73d4-7760-b047-abf38933983b` 仍为 version `3`、待处理。
- A 离线期间将其处理为 discard version `4`，并创建下一 session `01a11058-4a62-7167-9e55-2d7bef58f790`。
- B 重启后自动补齐到相同终态和 `last_change_seq=20`，旧会话没有复活；A/B 采用同一下一 session，`first_started_at=1791275584269`。

### 7. 并发处理唯一结果

- A、B 基于同一 session `01a1105a-8fb8-7289-b25a-ba31240ec674` version `3`，通过 UI 屏障同时提交“休息时间”和“无效时间”。
- 云端唯一接受 operation：`6ce71276-7144-4ce7-861c-d1316c1a04cd`。
- 最终统一为 discard version `4`，`resolved_at=1791275858209`。
- 唯一最终记录：`01a1105c-78f4-77ff-af05-91357bd44068`，标签为“无效时间”；输家的休息记录被回滚，没有第二条最终记录。
- 下一共享 session：`01a1105c-78f5-7154-9466-01859a42af35`，开始边界与 `resolved_at` 相同。

## 验收过程中发现并修复的问题

- 远端终态早于最终 time entry 到达时，外键会阻止终态应用：改为先落终态，time entry 到达后补关联。
- 本地生命周期动作与远端更高版本竞争会形成伪冲突：普通 pause/resume/awaiting-resolution 现在采用云端当前会话；真正 work/break/discard 仍保持唯一胜者。
- 远端权威下一会话与本地候选共享前序 ID 时触发唯一约束：现在封存候选、清理其 outbox，再采用权威会话。
- 并发处理输家可能留下乐观生成记录或事项完成副作用：现在随依赖链一并回滚。

## 自动验证

- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`：通过。
- `cargo test --manifest-path src-tauri/Cargo.toml`：234 passed，0 failed，1 ignored（需要专用真实 Supabase 测试账号）。
- `npm run typecheck`：通过。
- `npm run build`：通过。
- `npm run test:unassigned-resolution-guard`：通过。
- `npm run test:cloud-sync-presentation`：通过。
- `npm run test:work-data-refresh`：通过。
- `node scripts/verify-supabase-schema.mjs`：通过。
- `openspec validate sync-unassigned-time-across-devices --strict`：通过。
- `git diff --check`：通过。

## Supabase 部署

- 部署补丁：`supabase/20261006_timegenie_shared_unassigned_patch.sql`。
- 本地补丁 SHA-256：`883C1F1BF10343421C45A5F33857310E1D9DAC481CA0DFDFA98B7445CBDF774C`。
- 本次部署前备份：`/www/wwwroot/supabase/backups/timegenie-before-shared-unassigned-followup-20261006-164157.sql`。
- 备份 SHA-256：`8a3f382d801dbb9c776e1f02d7a07d8d634d0a7ad56b829f489b905c521f1949`。
- 权限复查：`authenticated` 可调用公开同步 RPC，`anon` 不可调用内部共享未归属补丁函数。
