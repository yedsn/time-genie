import { spawnSync } from "node:child_process";

const commands = [
  {
    label: "今日默认标签、无预计加入、预计自动加入/清除、移出、完成/恢复、父子上下文、重复轮次与跨日刷新",
    command: "npm",
    args: ["run", "test:today-plan-ui"],
  },
  {
    label: "今日 / 进行中 / 已完成标签切换与默认标签",
    command: "npm",
    args: ["run", "test:task-status-tabs-ui"],
  },
  {
    label: "执行弹框只显示今日、新建并开始、加入失败与跨日当前计时",
    command: "npm",
    args: ["run", "test:execution-dialog-ui"],
  },
  {
    label: "非执行分配保持完整事项范围",
    command: "npm",
    args: ["run", "test:execution-task-panel"],
  },
  {
    label: "本地 SQLite 今日计划成员、预计、重复轮次、事务回滚与完成态",
    command: "cargo",
    args: ["test", "--manifest-path", "src-tauri/Cargo.toml", "today_plans::tests", "--", "--nocapture"],
  },
  {
    label: "新建并开始的本地原子事务与跨日工作日语义",
    command: "cargo",
    args: ["test", "--manifest-path", "src-tauri/Cargo.toml", "time_tracking::tests::transition_", "--", "--nocapture"],
  },
];

const shell = process.platform === "win32";
for (const item of commands) {
  console.log("\n[验收] " + item.label);
  const result = spawnSync(item.command, item.args, {
    cwd: process.cwd(),
    env: process.env,
    stdio: "inherit",
    shell,
  });
  if (result.status !== 0) process.exit(result.status ?? 1);
  console.log("[通过] " + item.label);
}

console.log("\n今日计划本地模式验收矩阵全部通过");
