import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const tauriRoot = path.join(repoRoot, "src-tauri");
const sourceRoot = path.join(tauriRoot, "src");
const manifest = JSON.parse(fs.readFileSync(path.join(tauriRoot, "storage-boundaries.json"), "utf8"));
const boundary = manifest.directCloudCommandBoundaries;
if (!boundary) throw new Error("存储边界清单缺少 directCloudCommandBoundaries");

const libSource = fs.readFileSync(path.join(sourceRoot, "lib.rs"), "utf8");
const handlerStart = libSource.indexOf("tauri::generate_handler![");
const handlerEnd = libSource.indexOf("]),", handlerStart) >= 0
  ? libSource.indexOf("]),", handlerStart)
  : libSource.indexOf("] )", handlerStart) >= 0
    ? libSource.indexOf("] )", handlerStart)
    : libSource.indexOf("])", handlerStart);
if (handlerStart < 0 || handlerEnd < 0) throw new Error("无法定位 Tauri handler 清单");
const handlers = libSource.slice(handlerStart, handlerEnd)
  .split(/[,\n]/)
  .map((item) => item.trim())
  .filter((item) => item.includes("::"));

const businessModules = new Set(Object.values(manifest.businessDomains).flat());
const runtimeModules = new Set(boundary.runtimeModules);
const exceptions = new Set(Object.values(boundary.explicitCommandExceptions ?? {}).flat());
const routedStorageHelpers = new Set(boundary.explicitCommandExceptions?.routedStorageHelpers ?? []);
for (const moduleName of businessModules) {
  if (moduleName !== "data_protection" && !runtimeModules.has(moduleName)) {
    throw new Error(`生产业务模块未声明 direct_v2 运行边界：${moduleName}`);
  }
}
for (const moduleName of runtimeModules) {
  if (!businessModules.has(moduleName)) throw new Error(`direct_v2 运行模块不在业务领域清单中：${moduleName}`);
}

function functionBody(source, functionName) {
  const patterns = [
    `pub fn ${functionName}(`,
    `pub async fn ${functionName}(`,
    `pub(crate) fn ${functionName}(`,
    `pub(crate) async fn ${functionName}(`,
  ];
  const start = patterns.map((pattern) => source.indexOf(pattern)).find((index) => index >= 0);
  if (start === undefined) throw new Error(`无法定位命令函数：${functionName}`);
  const nextCommand = source.indexOf("#[tauri::command]", start);
  return source.slice(start, nextCommand < 0 ? source.length : nextCommand);
}

const uncovered = [];
const forbidden = [];
for (const handler of handlers) {
  const [moduleName, functionName] = handler.split("::");
  if (!businessModules.has(moduleName)) continue;
  if (exceptions.has(handler)) {
    if (routedStorageHelpers.has(handler)) {
      const source = fs.readFileSync(path.join(sourceRoot, `${moduleName}.rs`), "utf8");
      const body = functionBody(source, functionName);
      const helper = [...body.matchAll(/([a-zA-Z0-9_]+_for_storage)\s*\(/g)]
        .map((match) => match[1])
        .find((name) => name !== "run_for_storage");
      if (!helper || !functionBody(source, helper).includes("storage_contract::route")) {
        uncovered.push(`${handler}（路由辅助函数无效）`);
      }
    }
    continue;
  }
  if (!runtimeModules.has(moduleName)) { uncovered.push(handler); continue; }
  const source = fs.readFileSync(path.join(sourceRoot, `${moduleName}.rs`), "utf8");
  const body = functionBody(source, functionName);
  if (!body.includes("storage_contract::route")) uncovered.push(handler);
  for (const reference of boundary.forbiddenDirectV2References) {
    if (body.includes(reference)) forbidden.push(`${handler} -> ${reference}`);
  }
}
if (uncovered.length) throw new Error(`生产业务命令未接入存储路由：${uncovered.join(", ")}`);
if (forbidden.length) throw new Error(`direct_v2 命令重新引用旧同步 API：${forbidden.join(", ")}`);
for (const exception of exceptions) {
  if (!handlers.includes(exception)) throw new Error(`直连例外清单包含不存在的命令：${exception}`);
}

console.log(`Direct cloud command boundaries verified: ${handlers.filter((handler) => businessModules.has(handler.split("::")[0])).length} business commands.`);
