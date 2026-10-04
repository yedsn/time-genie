import { spawnSync } from "node:child_process";

const required = [
  "TG_SUPABASE_URL",
  "TG_SUPABASE_ANON_KEY",
  "TG_SUPABASE_EMAIL",
  "TG_SUPABASE_PASSWORD",
  "TG_SUPABASE_E2E_ALLOW_RESET",
];

const missing = required.filter((name) => !process.env[name]);
if (missing.length) {
  console.error(`缺少 Supabase 双设备端到端测试环境变量：${missing.join(", ")}`);
  console.error("该测试会删除专用测试账号的工作空间。请只使用可丢弃测试账号，并设置 TG_SUPABASE_E2E_ALLOW_RESET=1 后再运行。");
  process.exit(2);
}

if (process.env.TG_SUPABASE_E2E_ALLOW_RESET !== "1") {
  console.error("拒绝运行：TG_SUPABASE_E2E_ALLOW_RESET 必须显式设置为 1。该测试会清理专用测试账号的工作空间。");
  process.exit(2);
}

const result = spawnSync(
  "cargo",
  [
    "test",
    "--manifest-path",
    "src-tauri/Cargo.toml",
    "cloud_e2e::real_supabase_two_device_flow",
    "--",
    "--ignored",
    "--nocapture",
  ],
  { stdio: "inherit", shell: process.platform === "win32" },
);

if (result.error) {
  console.error(`无法启动 Supabase 双设备端到端测试：${result.error.message}`);
  process.exit(1);
}

process.exit(result.status ?? 1);
