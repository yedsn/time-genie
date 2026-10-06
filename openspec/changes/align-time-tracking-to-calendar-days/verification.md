# 验收记录

## 验收环境

- 两个独立 SQLite 客户端连接隔离的 Supabase 协议测试服务。
- 临时 PostgreSQL 执行完整 `schema.sql`、版本化补丁和集成 SQL，事务结束后回滚并清理。
- 正式自托管 Supabase 仅部署仓库中的版本化补丁；部署前创建非空 schema 备份，部署后执行只读元数据和服务健康复查。

## 验收结果

| 场景 | 证据 | 结果 |
|---|---|---|
| 跨零点按日切片 | Rust 自然日、连续跨日、暂停跨日和中断恢复测试；昨天与今天分别保存 30 秒和 60 秒 | 通过 |
| 两客户端不同候选 ID | `isolated_calendar_day_two_device_candidates_converge_to_one_authoritative_slice`：两端候选 ID 不同，服务端最终只有一个 `timer_chain_id + work_date` 实体，两端均采用同一权威 ID | 通过 |
| outbox 幂等与依赖 | `cloud_rollover_queues_deterministic_close_then_create_operations`：关闭操作在前，创建操作依赖关闭操作，重复协调不增加队列 | 通过 |
| 权威实体替换候选 | `authoritative_calendar_timer_replaces_conflicting_local_candidate_and_redirects_dependents`：候选被移除，分配、前序关系和后续 outbox 重定向到权威实体 | 通过 |
| 历史未归属与今天会话并存 | 未归属日期轮转和历史待处理列表测试：历史会话按日期保留，今日累计不包含历史分钟 | 通过 |
| 一端处理后另一端停止旧计时 | `isolated_unassigned_resolution_stops_the_old_clock_on_both_devices`：两端旧会话均无开放 segment，固定为已处理时长，并从相同 `resolved_at` 建立新会话 | 通过 |
| 失联与租约过期 | PostgreSQL 集成 SQL 和本地恢复测试：旧分段截止最后确认边界，新租约从重新获取时开始，失联区间不补算 | 通过 |
| 队列和冲突收敛 | 两个隔离客户端验收结束时 `pending_operations = 0`、`conflict_count = 0` | 通过 |
| Supabase schema 与权限 | 补丁连续执行两次成功；DST 23 小时日、并发权威实体、租约失效、RLS/执行权限检查通过 | 通过 |
| 正式环境部署后复查 | 6 个字段、2 个唯一索引、4 个协调函数签名和权限正确；公开 RPC 只使用服务端时间；PostgreSQL 与 REST 健康且 schema cache 重载成功 | 通过 |
| 前台不被后台上传阻塞 | `slow_cloud_upload_does_not_block_local_sqlite_reads_or_writes`：模拟慢上传期间本地读写在限定时间内完成 | 通过 |

## 数据清理和限制

- 临时 PostgreSQL 集成数据在外层事务中回滚，隔离客户端数据库由临时目录自动清理。
- 正式环境没有运行会创建业务测试数据或清理用户工作空间的 E2E；当前机器未配置专用可丢弃测试账号。正式环境验收范围为版本化 SQL 部署和只读结构、权限、健康复查。
- 客户端行为由两个隔离客户端测试覆盖，服务端原子行为由真实 PostgreSQL 集成 SQL 覆盖；两部分共同验证本变更的多端自然日收敛契约。
