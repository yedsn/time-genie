import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const manifest = JSON.parse(fs.readFileSync(path.join(repoRoot, "src-tauri", "storage-boundaries.json"), "utf8"));
const libSource = fs.readFileSync(path.join(repoRoot, "src-tauri", "src", "lib.rs"), "utf8");
const handlerStart = libSource.indexOf("tauri::generate_handler![");
const handlerEnd = libSource.indexOf("]),", handlerStart) >= 0
  ? libSource.indexOf("]),", handlerStart)
  : libSource.indexOf("])", handlerStart);
if (handlerStart < 0 || handlerEnd < 0) throw new Error("无法定位 Tauri handler 清单");
const handlers = libSource.slice(handlerStart, handlerEnd)
  .split(/[,\n]/)
  .map((item) => item.trim())
  .filter((item) => item && !item.startsWith("tauri::generate_handler"));

const moduleDomains = new Map();
for (const [domain, modules] of Object.entries(manifest.businessDomains)) {
  for (const moduleName of modules) {
    if (moduleDomains.has(moduleName)) throw new Error(`命令模块重复归类：${moduleName}`);
    moduleDomains.set(moduleName, domain);
  }
}
for (const moduleName of manifest.infrastructureModules) {
  if (moduleDomains.has(moduleName)) throw new Error(`基础模块与业务模块重复归类：${moduleName}`);
  moduleDomains.set(moduleName, "infrastructure");
}
const rootCommands = new Set(manifest.rootInfrastructureCommands);
const uncoveredHandlers = handlers.filter((handler) => {
  const parts = handler.split("::");
  return parts.length === 1 ? !rootCommands.has(handler) : !moduleDomains.has(parts[0]);
});
if (uncoveredHandlers.length) throw new Error(`存在未归类 Tauri 命令：${uncoveredHandlers.join(", ")}`);

const duplicateTables = new Set();
const classifiedTables = new Set();
for (const tables of Object.values(manifest.sqliteTables)) {
  for (const table of tables) {
    if (classifiedTables.has(table)) duplicateTables.add(table);
    classifiedTables.add(table);
  }
}
if (duplicateTables.size) throw new Error(`SQLite 表重复归类：${[...duplicateTables].join(", ")}`);
if (manifest.sqliteTables.cloudBusiness.length !== 0) {
  throw new Error("云端业务表不得在 SQLite 清单中出现");
}
const domainTableNames = new Set(Object.values(manifest.businessDomainTables).flat());
const uncoveredBusinessTables = manifest.sqliteTables.localBusiness.filter((table) => !domainTableNames.has(table));
if (uncoveredBusinessTables.length) {
  throw new Error(`存在未归入六个业务领域的本地业务表：${uncoveredBusinessTables.join(", ")}`);
}
for (const table of domainTableNames) {
  if (!manifest.sqliteTables.localBusiness.includes(table)) {
    throw new Error(`领域表不是已声明的本地业务表：${table}`);
  }
}
const migrationDir = path.join(repoRoot, "src-tauri", "migrations");
const migrationTables = new Set();
for (const fileName of fs.readdirSync(migrationDir).filter((name) => name.endsWith(".sql"))) {
  const sql = fs.readFileSync(path.join(migrationDir, fileName), "utf8");
  const pattern = /CREATE\s+(?:VIRTUAL\s+)?TABLE(?:\s+IF\s+NOT\s+EXISTS)?\s+["']?([a-zA-Z0-9_]+)/gim;
  for (const match of sql.matchAll(pattern)) migrationTables.add(match[1]);
}
migrationTables.add("schema_migrations");
const missingTables = [...migrationTables].filter((table) => !classifiedTables.has(table)).sort();
const staleTables = [...classifiedTables].filter((table) => !migrationTables.has(table)).sort();
if (missingTables.length) throw new Error(`存在未归类 SQLite 表：${missingTables.join(", ")}`);
if (staleTables.length) throw new Error(`存储边界清单包含不存在的 SQLite 表：${staleTables.join(", ")}`);

for (const docName of ["20260924_timegenie_后端接口设计.md", "20260924_timegenie_数据结构设计.md"]) {
  const documentation = fs.readFileSync(path.join(repoRoot, "docs", docName), "utf8");
  for (const marker of ["事项", "计划与重复", "计时与未归属", "报告与模板", "共享设置与集成", "数据保护"]) {
    if (!documentation.includes(marker)) throw new Error(`${docName} 缺少存储领域说明：${marker}`);
  }
  if (!documentation.includes("云端不落本地业务副本")) {
    throw new Error(`${docName} 缺少云端直连存储边界说明`);
  }
}

console.log(`Storage boundaries verified: ${handlers.length} commands, ${migrationTables.size} SQLite tables.`);
