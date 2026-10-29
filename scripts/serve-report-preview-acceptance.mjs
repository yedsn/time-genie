import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import net from "node:net";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createServer } from "vite";

// Isolated UI acceptance: the real ReportWorkspace and Pinia actions call the
// real PostgreSQL report functions. No user database or cloud account is used.
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const temporary = mkdtempSync(path.join(root, ".tmp-report-preview-acceptance-"));
const pgBin = process.env.PG_BIN_DIR ?? "C:\\Program Files\\PostgreSQL\\17\\bin";
const binary = (name) => path.join(pgBin, name + (process.platform === "win32" ? ".exe" : ""));
const freePort = () => new Promise((resolve, reject) => {
  const server = net.createServer();
  server.on("error", reject);
  server.listen(0, "127.0.0.1", () => {
    const port = server.address().port;
    server.close(() => resolve(port));
  });
});
const pgPort = await freePort();
const uiPort = Number(process.env.TG_REPORT_ACCEPTANCE_PORT ?? 5217);
const ids = {
  user: "11000000-0000-0000-0000-000000000001",
  workspace: "21000000-0000-0000-0000-000000000001",
  device: "31000000-0000-0000-0000-000000000001",
  subject: "51000000-0000-0000-0000-000000000001",
  task: "61000000-0000-0000-0000-000000000001",
  instance: "61000000-0000-0000-0000-000000000002",
  unrelated: "61000000-0000-0000-0000-000000000003",
  template: "a1000000-0000-0000-0000-000000000001",
};
const quote = (value) => "'" + String(value).replaceAll("'", "''") + "'";
const optionalUuid = (value) => value ? quote(value) + "::uuid" : "null::uuid";
function run(name, args, input) {
  const result = spawnSync(binary(name), args, { cwd: root, input, encoding: "utf8", stdio: name === "pg_ctl" ? "ignore" : "pipe", env: { ...process.env, PGCLIENTENCODING: "UTF8" } });
  if (result.status !== 0) throw new Error(result.stderr || result.stdout || name + " failed");
  return result.stdout;
}
const pgArgs = ["-h", "127.0.0.1", "-p", String(pgPort), "-U", "postgres", "-d", "postgres", "-w", "-v", "ON_ERROR_STOP=1", "-qAt"];
const sql = (query) => run("psql", pgArgs, "set client_min_messages=warning;\n" + query);
const queryJson = (expression) => JSON.parse(sql(`begin; set local role authenticated; set local request.jwt.claim.sub=${quote(ids.user)}; select ${expression}; commit;`).trim());
const scope = `${quote(ids.workspace)},${quote(ids.device)}`;
let started = false;
let vite;
let closing = false;
async function cleanup() {
  if (closing) return;
  closing = true;
  await vite?.close();
  if (started) run("pg_ctl", ["-D", path.join(temporary, "data"), "stop", "-m", "fast"]);
  const relative = path.relative(root, temporary);
  if (!relative.startsWith("..") && !path.isAbsolute(relative)) rmSync(temporary, { recursive: true, force: true });
}
process.once("SIGINT", () => cleanup().finally(() => process.exit()));
process.once("SIGTERM", () => cleanup().finally(() => process.exit()));

try {
  run("initdb", ["-D", path.join(temporary, "data"), "-U", "postgres", "-A", "trust", "--no-locale", "--encoding=UTF8"]);
  run("pg_ctl", ["-D", path.join(temporary, "data"), "-o", `-p ${pgPort}`, "-l", path.join(temporary, "postgres.log"), "start"]);
  started = true;
  run("psql", [...pgArgs, "-f", path.join(root, "supabase/schema_fresh_planning_test.sql")]);
  sql(`
    insert into auth.users(id) values('${ids.user}');
    insert into timegenie.workspaces(id,owner_user_id,name) values('${ids.workspace}','${ids.user}','报告验收');
    insert into timegenie.devices(id,workspace_id,device_name,platform,app_version) values('${ids.device}','${ids.workspace}','报告验收设备','windows','0.5.7');
    insert into timegenie.subjects(id,workspace_id,name,sort_order) values('${ids.subject}','${ids.workspace}','报告验收',1);
    insert into timegenie.tasks(id,workspace_id,subject_id,title,status,source_type,sort_order) values
      ('${ids.task}','${ids.workspace}','${ids.subject}','合并事项','open','manual',1),
      ('${ids.unrelated}','${ids.workspace}','${ids.subject}','无关进行中事项','open','manual',3);
    insert into timegenie.tasks(id,workspace_id,subject_id,title,status,source_type,sort_order,day_instance_date,day_instance_kind,day_instance_source_task_id)
      values('${ids.instance}','${ids.workspace}','${ids.subject}','每日实例不应单独出现','open','manual',2,current_date,'member','${ids.task}');
    with entries as (
      insert into timegenie.time_entries(id,workspace_id,work_date,kind,source_type,state,default_task_id,label_snapshot,started_at,ended_at,duration_seconds)
      select gen_random_uuid(),'${ids.workspace}',current_date,'work','manual','ended',task_id,'验收记录',now(),now()+minutes*interval '1 minute',minutes*60
      from (values('${ids.task}'::uuid,20),('${ids.task}'::uuid,25),('${ids.instance}'::uuid,15)) item(task_id,minutes)
      returning id,default_task_id,duration_seconds
    ) insert into timegenie.time_allocations(id,workspace_id,entry_id,task_id,minutes)
      select gen_random_uuid(),'${ids.workspace}',id,default_task_id,duration_seconds/60 from entries;
    insert into timegenie.actual_time_adjustments(id,workspace_id,subject_id,task_id,work_date,delta_minutes,baseline_minutes,target_minutes,source_type,batch_id)
      values(gen_random_uuid(),'${ids.workspace}','${ids.subject}','${ids.task}',current_date,5,60,65,'quick_actual_adjustment',gen_random_uuid());
    insert into timegenie.report_templates(id,workspace_id,report_type,name,content,is_default,is_builtin,sort_order)
      values('${ids.template}','${ids.workspace}','daily','验收日报模板',E'# {{日期}} 日报\n\n{{今日事项}}\n\n合计 {{总工时}}',true,false,1);
  `);
  const calls = [];
  const previews = [];
  let created;
  function invoke(command, args = {}) {
    const request = args.request ?? {};
    calls.push({ command, request });
    switch (command) {
      case "settings_get": return { shared: { report_duration_format: "minutes" }, device: {}, integrations: {}, secrets: {} };
      case "direct_cloud_client_metric_record": return null;
      case "subject_work_schedule_get": return [];
      case "report_work_period_preview": return "";
      case "report_list": return { reports: created ? [created] : [], page: 1, pageSize: 20, total: created ? 1 : 0, hasMore: false, typeCounts: { all: created ? 1 : 0, daily: created ? 1 : 0, weekly: 0, monthly: 0 }, revision: 1 };
      case "report_template_list": return queryJson(`timegenie.direct_cloud_report_templates(${scope},'daily',1)` ).data.templates;
      case "report_preview": {
        const result = queryJson(`timegenie.direct_cloud_report_preview(${scope},${quote(request.reportType)},${quote(request.referenceDate)}::date,${quote(request.subjectId)},${optionalUuid(request.templateId)},1)`);
        previews.push(result.data.markdown);
        return result.data.markdown;
      }
      case "report_create": {
        const result = queryJson(`timegenie.direct_cloud_report_create(${scope},${quote(request.clientRequestId)}::uuid,1,${quote(JSON.stringify(request))}::jsonb)`);
        created = { ...result.data.report, period: result.data.report.periodStart };
        if (created.markdown !== previews.at(-1)) throw new Error("预览与保存 Markdown 不一致");
        if (created.taskIds.length !== 1 || created.taskIds[0] !== ids.task
          || created.markdown.split("合并事项 <实际：1h5min>").length !== 2
          || !created.markdown.includes("合计 1h5min")
          || created.markdown.includes("每日实例不应单独出现")
          || created.markdown.includes("无关进行中事项")) throw new Error("事项唯一性或实际用时合计不正确");
        return created;
      }
      default: throw new Error("未预期的验收调用：" + command);
    }
  }
  vite = await createServer({
    configFile: path.join(root, "vite.config.ts"),
    root: path.join(root, "src-ui"),
    server: { host: "127.0.0.1", port: uiPort, strictPort: true },
    plugins: [{
      name: "report-preview-acceptance",
      configureServer(server) {
        server.middlewares.use(async (req, res, next) => {
          if (req.url === "/__report_acceptance/cleanup" && req.method === "POST") {
            res.end("验收环境已关闭");
            setTimeout(() => cleanup().finally(() => process.exit()), 50);
            return;
          }
          if (req.url === "/__report_acceptance/rpc") {
            try {
              let body = "";
              for await (const chunk of req) body += chunk;
              const { command, args } = JSON.parse(body);
              res.setHeader("Content-Type", "application/json");
              res.end(JSON.stringify({ result: invoke(command, args) }));
            } catch (error) { res.statusCode = 500; res.end(JSON.stringify({ error: error.message })); }
            return;
          }
          if (req.url === "/__report_acceptance/evidence") {
            res.setHeader("Content-Type", "application/json");
            res.end(JSON.stringify({ calls, previews, created, saved: JSON.parse(sql(`select coalesce(jsonb_agg(jsonb_build_object('markdown',markdown_content,'taskCount',(select count(*) from timegenie.report_tasks where report_id=report.id))),'[]') from timegenie.reports report;`).trim()) }));
            return;
          }
          if (req.url !== "/__report_acceptance.html") return next();
          const html = `<!doctype html><html><head><meta charset="utf-8"><title>报告创建预览验收</title></head><body><div id="app"></div><script type="module">
            window.__TAURI_INTERNALS__={metadata:{currentWindow:{label:'main'},currentWebview:{label:'main'}},invoke:async(command,args)=>{const response=await fetch('/__report_acceptance/rpc',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({command,args})});const data=await response.json();if(!response.ok)throw new Error(data.error);return data.result;}};
            const [{createApp},{createPinia},{default:ElementPlus},{default:ReportWorkspace},{useWorkdayStore},{hoverSwitchDirective}]=await Promise.all([import('vue'),import('pinia'),import('element-plus'),import('/src/components/ReportWorkspace.vue'),import('/src/store.ts'),import('/src/services/hoverNavigation.ts')]);
            await import('element-plus/dist/index.css');await import('/src/styles.css');
            const pinia=createPinia();const app=createApp(ReportWorkspace);app.use(pinia).use(ElementPlus).directive('hover-switch',hoverSwitchDirective);const store=useWorkdayStore(pinia);store.subjects=[{id:'${ids.subject}',name:'报告验收',sortOrder:1,openTaskCount:2,version:1}];store.selectedSubjectId='${ids.subject}';app.mount('#app');
          </script></body></html>`;
          res.setHeader("Content-Type", "text/html");
          res.end(await server.transformIndexHtml(req.url, html));
        });
      },
    }],
  });
  await vite.listen();
  console.log(`报告弹窗验收已就绪：http://127.0.0.1:${uiPort}/__report_acceptance.html`);
  console.log("夹具：20 + 25 + 实例 15 + 调整 5 = 65 分钟；预览和保存均应仅有一条合并事项。");
} catch (error) {
  console.error(error);
  await cleanup();
  process.exitCode = 1;
}
