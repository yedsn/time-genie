import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import net from "node:net";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, "..");
const dataDir = path.join(repoRoot, ".tmp-supabase-schema-pg-data");
const logFile = path.join(repoRoot, ".tmp-supabase-schema-pg.log");
const schemaTestFile = path.join(repoRoot, "supabase", "schema_integration_test.sql");

const binaryNames = process.platform === "win32"
  ? {
      initdb: "initdb.exe",
      pgCtl: "pg_ctl.exe",
      psql: "psql.exe",
    }
  : {
      initdb: "initdb",
      pgCtl: "pg_ctl",
      psql: "psql",
    };

const candidateBinDirs = [
  process.env.PG_BIN_DIR,
  process.platform === "win32" ? "C:\\Program Files\\PostgreSQL\\17\\bin" : undefined,
  process.platform === "win32" ? "C:\\Program Files\\PostgreSQL\\16\\bin" : undefined,
  process.platform === "win32" ? "C:\\Program Files\\PostgreSQL\\15\\bin" : undefined,
].filter(Boolean);

function commandExists(command) {
  const probe = process.platform === "win32" ? "where" : "command";
  const args = process.platform === "win32" ? [command] : ["-v", command];
  return spawnSync(probe, args, { stdio: "ignore", shell: process.platform !== "win32" }).status === 0;
}

function resolveBinary(name) {
  for (const dir of candidateBinDirs) {
    const fullPath = path.join(dir, name);
    if (fs.existsSync(fullPath)) return fullPath;
  }
  if (commandExists(name)) return name;
  throw new Error(`PostgreSQL binary not found: ${name}. Set PG_BIN_DIR to the PostgreSQL bin directory.`);
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: repoRoot,
    env: { ...process.env, ...options.env },
    encoding: "utf8",
    stdio: options.capture ? "pipe" : "inherit",
  });
  if (result.status !== 0) {
    const detail = options.capture ? `\n${result.stdout ?? ""}${result.stderr ?? ""}` : "";
    throw new Error(`${path.basename(command)} failed with exit code ${result.status}.${detail}`);
  }
  return result;
}

function ensureInsideRepo(targetPath) {
  const relative = path.relative(repoRoot, targetPath);
  if (relative.startsWith("..") || path.isAbsolute(relative)) {
    throw new Error(`Refusing to use path outside repo: ${targetPath}`);
  }
}

async function findOpenPort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      const port = typeof address === "object" && address ? address.port : undefined;
      server.close(() => (port ? resolve(port) : reject(new Error("Could not allocate a local port"))));
    });
  });
}

async function waitForPostgres(psql, port) {
  const deadline = Date.now() + 15_000;
  while (Date.now() < deadline) {
    const result = spawnSync(psql, [
      "-h", "127.0.0.1",
      "-p", String(port),
      "-U", "postgres",
      "-d", "postgres",
      "-w",
      "-c", "select 1;",
    ], { cwd: repoRoot, stdio: "ignore" });
    if (result.status === 0) return;
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error("Timed out waiting for temporary PostgreSQL to start.");
}

async function main() {
  const initdb = resolveBinary(binaryNames.initdb);
  const pgCtl = resolveBinary(binaryNames.pgCtl);
  const psql = resolveBinary(binaryNames.psql);
  const port = await findOpenPort();

  ensureInsideRepo(dataDir);
  ensureInsideRepo(logFile);
  fs.rmSync(dataDir, { recursive: true, force: true });
  fs.rmSync(logFile, { force: true });

  let started = false;
  try {
    run(initdb, ["-D", dataDir, "-U", "postgres", "-A", "trust", "--no-locale"]);
    run(pgCtl, ["-D", dataDir, "-o", `-p ${port}`, "-l", logFile, "start"]);
    started = true;
    await waitForPostgres(psql, port);
    run(psql, [
      "-h", "127.0.0.1",
      "-p", String(port),
      "-U", "postgres",
      "-d", "postgres",
      "-w",
      "-v", "ON_ERROR_STOP=1",
      "-f", schemaTestFile,
    ]);
    console.log("Supabase schema integration verification passed.");
  } finally {
    if (started) {
      spawnSync(pgCtl, ["-D", dataDir, "stop", "-m", "fast"], { cwd: repoRoot, stdio: "inherit" });
    }
    if (process.env.TG_KEEP_SUPABASE_SCHEMA_PG !== "1") {
      fs.rmSync(dataDir, { recursive: true, force: true });
      fs.rmSync(logFile, { force: true });
    }
  }
}

main().catch((error) => {
  console.error(error.message);
  process.exit(1);
});
