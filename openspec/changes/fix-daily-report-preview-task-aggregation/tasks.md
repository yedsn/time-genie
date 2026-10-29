# Tasks

## 1. 固化报告聚合与预览契约

- [x] 1.1 扩展 Rust 报告测试数据，覆盖同一稳定事项的多条时间分配、实际用时调整和重复事项每日实例，并验证日报只输出一条事项且实际分钟为合计值
- [x] 1.2 扩展本地预览一致性测试，验证相同日期、主体、模板和未变化数据下，`preview_report` 的完整 Markdown 等于 `create_report` 的 Markdown，且预览不会写入报告记录
- [x] 1.3 在 Supabase schema 集成测试中增加无关进行中事项、多条分配和重复实例夹具，并先建立预览、创建、重新生成应满足的事项唯一性与分钟合计断言

## 2. 统一 Supabase 直连报告范围与渲染

- [x] 2.1 新增可增量部署的 Supabase 报告修复迁移，扩展 `direct_cloud_report_render` 按稳定事项汇总时间分配与实际用时调整，并验证同一事项只生成一行
- [x] 2.2 在直连报告渲染中读取 `report_duration_format` 并为日报叶子事项输出合计实际用时，验证分钟与一位小数小时两种格式以及父级不重复计时
- [x] 2.3 用正式的周期归一化、`direct_cloud_report_scope_task_ids` 和 `direct_cloud_report_render` 重写 `direct_cloud_report_preview`，验证无关进行中事项和每日实例不会进入自动日报
- [ ] 2.4 将新迁移接入 `supabase/schema.sql` 和 `supabase/schema_integration_test.sql`，重复执行迁移并验证函数部署幂等、权限授权和 schema 集成测试通过

## 3. 对齐应用请求与回归验证

- [x] 3.1 核对并在必要时调整 Rust 直连预览适配与前端 `previewReport` 请求，使预览和创建传递相同的日期、主体与模板语义，并通过 `npm run typecheck`
- [x] 3.2 运行报告相关 Rust 测试，验证本地模式的事项聚合、时长格式、预览不落库和预览创建一致性全部通过
- [x] 3.3 运行 Supabase schema 集成测试，验证直连模式下预览 Markdown 等于创建 Markdown、报告总工时等于各事项合计，并且重新生成保持相同口径
- [ ] 3.4 执行报告创建弹窗的人工验收：为同一事项准备多条当天时间记录，确认预览与保存结果均只显示一条事项及其合计实际用时
