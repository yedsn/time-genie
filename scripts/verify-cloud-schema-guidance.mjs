import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const rust = fs.readFileSync(path.join(repoRoot, "src-tauri/src/supabase.rs"), "utf8");
const lib = fs.readFileSync(path.join(repoRoot, "src-tauri/src/lib.rs"), "utf8");
const service = fs.readFileSync(path.join(repoRoot, "src-ui/src/services/tauri.ts"), "utf8");
const workbench = fs.readFileSync(path.join(repoRoot, "src-ui/src/views/MainWorkbench.vue"), "utf8");
const styles = fs.readFileSync(path.join(repoRoot, "src-ui/src/styles.css"), "utf8");

for (const expected of [
  "pub fn probe_cloud_project",
  "classify_probe_response",
  "CLOUD_PROJECT_SCHEMA_MISSING",
  "CLOUD_PROJECT_SCHEMA_NOT_EXPOSED",
  "CLOUD_SCHEMA_NOT_EXPOSED",
  "PGRST106",
]) {
  if (!rust.includes(expected)) throw new Error("服务端缺少未初始化检测：" + expected);
}
if (!rust.includes("supabase/schema.sql")) throw new Error("未初始化提示必须指向 supabase/schema.sql");
if (!rust.includes("SQL Editor")) throw new Error("未初始化提示必须说明在 SQL Editor 执行脚本");
if (!rust.includes("PGRST_DB_SCHEMAS")) throw new Error("schema 未暴露时必须提示 PGRST_DB_SCHEMAS");
if (!lib.includes("supabase::cloud_project_probe")) throw new Error("未注册 cloud_project_probe 命令");

for (const expected of ["probeCloudProject", "cloud_project_probe", "CloudProjectProbeState"]) {
  if (!service.includes(expected)) throw new Error("前端服务层缺少：" + expected);
}
for (const expected of [
  "cloudProbeAlert",
  "probeSupabaseProject",
  "cloudSchemaFailure",
  "cloud-schema-guidance",
  "重新检测",
]) {
  if (!workbench.includes(expected)) throw new Error("连接流程缺少未初始化提示：" + expected);
}
if (!workbench.includes("cloudProbe.value = await probeCloudProject()")) {
  throw new Error("检测结果必须回写到连接提示状态");
}
if (!styles.includes(".cloud-schema-guidance")) throw new Error("未初始化提示缺少样式");
if (!fs.existsSync(path.join(repoRoot, "supabase/schema.sql"))) {
  throw new Error("初始化脚本 supabase/schema.sql 不存在");
}

console.log("Cloud schema guidance verification passed.");
