# Tasks

## 1. 主窗口承接事项选择与处理流程

- [x] 1.1 增加从托盘打开主窗口计时区域的命令或事件，切换到时间记录页面并聚焦现有事项选择控件但不自动开始；补充前端聚焦验证并运行 `npm run typecheck`
- [x] 1.2 对齐托盘“结束并处理”与现有停止确认流程，确保停止后主窗口显示对应确认或处理入口；运行相关停止确认测试并手动验证不会创建第二套分配流程
- [x] 1.3 继续复用现有未归属处理导航并验证状态已变化时不打开空弹框；运行未归属入口相关测试

## 2. 简化托盘状态面板

- [x] 2.1 重构 `HoverPanel`，移除主体/事项选择、搜索、临时事项创建和选中事项状态，只保留计时状态、事项名称、未归属摘要及今日统计；运行 `npm run typecheck` 并检查空闲态和计时态渲染
- [x] 2.2 在空闲态加入“选择事项并开始”主窗口入口，在计时态保留暂停/继续和“结束并处理”，并维持手动刷新、处理未归属时间和打开主窗口动作；运行 `npm run build` 并实际点击验证各动作目的地
- [x] 2.3 删除托盘专用 autocomplete、候选列表和扩展态样式，调整固定紧凑布局，检查长事项名称省略提示、按钮可见性和 420px 面板无溢出
- [x] 2.4 更新 `trayHoverCore` 与托盘前端验证，移除事项选择有效性逻辑，覆盖活动计时、未归属和空闲三种展示状态；运行 `npm run test:tray-hover`

## 3. 恢复固定原生悬浮窗口

- [x] 3.1 删除事项选择窗口模式命令、扩展尺寸常量、完整表面/紧凑裁剪状态和 Windows GDI 区域切换，让悬浮窗口只按固定逻辑尺寸定位显示；运行 `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` 与 `cargo check --manifest-path src-tauri/Cargo.toml`
- [x] 3.2 简化 `HoverState` 和显示/隐藏恢复路径，使窗口物理边界成为唯一交互边界，并保留 DPI、工作区及负坐标显示器定位；更新 Rust 窗口测试并运行 `cargo test --manifest-path src-tauri/Cargo.toml hover_window_ --lib`
- [x] 3.3 移除前端 `setHoverTaskSelectionMode` 服务接口及所有调用，确认全仓不存在扩展模式、选择模式和裁剪 API 残留；运行全仓搜索与 `git diff --check`

## 4. 集成验收

- [ ] 4.1 使用实际 Windows 开发程序验证悬停约 300ms 显示、窗口位置和尺寸固定、显示时刷新、暂停/继续、结束并处理、选择事项主窗口定位、未归属入口、移出三秒关闭、外部点击立即关闭及右键菜单抑制
- [x] 4.2 运行 `npm run typecheck`、`npm run build`、`npm run test:tray-hover`、`npm run test:work-data-refresh`、相关 Rust 测试与 `openspec validate simplify-tray-hover-panel --strict`，确认简化后无回归
