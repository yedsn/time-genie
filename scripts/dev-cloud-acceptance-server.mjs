import crypto from "node:crypto";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";

const port = Number(process.env.TG_DEV_CLOUD_PORT || 55431);
const workspaceId = process.env.TG_DEV_CLOUD_WORKSPACE_ID || "20000000-0000-0000-0000-000000000001";
const subjectId = process.env.TG_DEV_CLOUD_SUBJECT_ID || "60000000-0000-0000-0000-000000000001";
const statePath = path.resolve(process.env.TG_DEV_CLOUD_STATE || "target/dev-cloud-acceptance.json");
const templatePrefix = "91000000-0000-0000-0000-00000000000";
const now = Date.now();

function freshState() {
  return {
    latestChangeSeq: 0,
    changes: [],
    entities: {
      subjects: {
        [subjectId]: { id: subjectId, name: "验收主体", sort_order: 10, created_at: now, updated_at: now, version: 1, deleted_at: null },
      },
      tasks: {}, task_status_events: {}, task_daily_estimates: {}, task_recurrence_rules: {}, task_occurrences: {},
      work_days: {}, time_entries: {}, unassigned_sessions: {}, report_templates: {
        [`${templatePrefix}1`]: { id: `${templatePrefix}1`, report_type: "daily", subject_id: null, content: "# {{日期}}\n\n{{今日事项}}\n", is_builtin: true, created_at: now, updated_at: now, version: 1, deleted_at: null },
        [`${templatePrefix}2`]: { id: `${templatePrefix}2`, report_type: "weekly", subject_id: null, content: "# {{日期范围}}\n\n{{每日情况}}\n", is_builtin: true, created_at: now, updated_at: now, version: 1, deleted_at: null },
        [`${templatePrefix}3`]: { id: `${templatePrefix}3`, report_type: "monthly", subject_id: null, content: "# {{日期范围}}\n\n{{每日情况}}\n", is_builtin: true, created_at: now, updated_at: now, version: 1, deleted_at: null },
      },
      reports: {}, app_settings: {}, integration_configs: {}, external_bindings: {},
    },
    processedOperations: {},
    control: { delayMs: 0, failNext: 0 },
  };
}

let state = fs.existsSync(statePath) ? JSON.parse(fs.readFileSync(statePath, "utf8")) : freshState();
state.control ||= { delayMs: 0, failNext: 0 };
state.processedOperations ||= {};
state.entities.unassigned_sessions ||= {};
const sockets = new Set();

function save() {
  fs.mkdirSync(path.dirname(statePath), { recursive: true });
  fs.writeFileSync(statePath, JSON.stringify(state, null, 2));
}

function json(response, status, value) {
  const body = JSON.stringify(value);
  response.writeHead(status, { "content-type": "application/json", "content-length": Buffer.byteLength(body), connection: "close" });
  response.end(body);
}

async function bodyJson(request) {
  const chunks = [];
  for await (const chunk of request) chunks.push(chunk);
  return chunks.length ? JSON.parse(Buffer.concat(chunks).toString("utf8")) : {};
}

function sourceType(entityType) {
  return ({
    subject: "subjects", task: "tasks", time_entry: "time_entries", report: "reports",
    report_template: "report_templates", app_setting: "app_settings", integration_config: "integration_configs",
    external_binding: "external_bindings", task_occurrence: "task_occurrences",
    task_daily_estimate: "task_daily_estimates", task_recurrence_rule: "task_recurrence_rules",
    unassigned_session: "unassigned_sessions",
  })[entityType];
}

function aggregateType(source) {
  return ({ time_segments: "time_entries", time_allocations: "time_entries", report_tasks: "reports" })[source] || source;
}

function recordChange(source, entityId, operation, version) {
  state.latestChangeSeq += 1;
  const change = { change_seq: state.latestChangeSeq, workspace_id: workspaceId, entity_type: source, entity_id: entityId, operation, entity_version: version || 0, changed_at: new Date().toISOString() };
  state.changes.push(change);
  save();
  for (const socket of sockets) sendText(socket, JSON.stringify({
    topic: `realtime:timegenie:workspace_changes:${workspaceId}`, event: "postgres_changes",
    payload: { data: { record: change } }, ref: null, join_ref: "1",
  }));
}

function currentVersion(source, entityId) {
  const value = state.entities[source]?.[entityId];
  if (!value) return null;
  if (source === "task_recurrence_rules") return Number(value.version || Math.max(0, ...(value.rules || []).map((rule) => Number(rule.version || 0))));
  return Number(value.version || 0);
}

function applyPatch(body) {
  const type = body.p_entity_type;
  const source = sourceType(type);
  if (!source) throw new Error(`UNSUPPORTED_OPERATION: ${type}`);
  const id = body.p_entity_id;
  const payload = body.p_payload || {};
  const base = body.p_base_version ?? null;
  const current = currentVersion(source, id);
  const deleting = payload.deleted === true || payload.deleted_at != null;
  if (current !== base && !(deleting && current === null)) {
    const currentValue = state.entities[source]?.[id] || null;
    const lifecycleWasSuperseded = type === "unassigned_session"
      && ["unassigned_session_pause", "unassigned_session_resume", "unassigned_session_awaiting_resolution"].includes(body.p_operation_type)
      && currentValue != null;
    if (lifecycleWasSuperseded) return { superseded: true, session: currentValue };
    throw new Error(`SYNC_CONFLICT: expected version ${base}, current version ${current}`);
  }
  if (deleting) delete state.entities[source][id];
  else state.entities[source][id] = payload;
  if (type === "task_recurrence_rule") {
    state.entities[source][id] = payload;
    const task = state.entities.tasks[id];
    if (task && payload.task_version) task.version = payload.task_version;
  }
  const version = Number(payload.version || payload.task_version || 1);
  recordChange(source, id, deleting ? "delete" : "update", version);
  if (type === "task_recurrence_rule" && state.entities.tasks[id]) recordChange("tasks", id, "update", Number(state.entities.tasks[id].version || 1));
  return { entityType: type, entityId: id, version };
}

function activeUnassignedSession() {
  return Object.values(state.entities.unassigned_sessions)
    .find((session) => session.state === "collecting" || session.state === "awaiting_resolution") || null;
}

function normalizeUnassignedSession(candidate, predecessorSessionId, startedAt) {
  const boundary = Number(startedAt || candidate?.first_started_at || Date.now());
  const id = candidate?.id || crypto.randomUUID();
  const segments = Array.isArray(candidate?.segments) && candidate.segments.length
    ? candidate.segments
    : [{ id: crypto.randomUUID(), session_id: id, sequence_no: 1, started_at: boundary, ended_at: null, duration_seconds: 0 }];
  return {
    id,
    work_date: candidate?.work_date || new Date(boundary).toISOString().slice(0, 10),
    state: candidate?.state === "awaiting_resolution" ? "awaiting_resolution" : "collecting",
    threshold_seconds: Number(candidate?.threshold_seconds || 300),
    duration_seconds: Number(candidate?.duration_seconds || 0),
    first_started_at: boundary,
    last_ended_at: candidate?.last_ended_at ?? null,
    prompted_at: candidate?.prompted_at ?? null,
    resolution_type: null,
    generated_entry_id: null,
    resolved_at: null,
    created_at: Number(candidate?.created_at || boundary),
    updated_at: Date.now(),
    version: Number(candidate?.version || 1),
    shared_source: "cloud",
    predecessor_session_id: predecessorSessionId || candidate?.predecessor_session_id || null,
    migration_state: "adopted",
    resolution_operation_id: null,
    segments: segments.map((segment, index) => ({
      ...segment,
      id: segment.id || crypto.randomUUID(),
      session_id: id,
      sequence_no: Number(segment.sequence_no || index + 1),
      started_at: Number(segment.started_at || boundary),
      ended_at: segment.ended_at == null ? null : Number(segment.ended_at),
      duration_seconds: Number(segment.duration_seconds || 0),
    })),
  };
}

function getOrCreateSharedUnassigned(body) {
  const existing = activeUnassignedSession();
  if (existing) return existing;
  const session = normalizeUnassignedSession(
    body.p_candidate || null,
    body.p_predecessor_session_id || null,
    body.p_started_at || null,
  );
  state.entities.unassigned_sessions[session.id] = session;
  recordChange("unassigned_sessions", session.id, "update", session.version);
  return session;
}

function resolveSharedUnassigned(body) {
  const operationId = body.p_operation_id;
  if (state.processedOperations[operationId]) return state.processedOperations[operationId];
  const session = state.entities.unassigned_sessions[body.p_session_id];
  if (!session) throw new Error("VALIDATION_ERROR: shared unassigned session not found");
  const expectedVersion = Number(body.p_expected_version);
  if ((session.state !== "collecting" && session.state !== "awaiting_resolution") || Number(session.version) !== expectedVersion) {
    return { accepted: false, session };
  }
  const resolvedAt = Date.now();
  const resolutionType = body.p_resolution_type;
  for (const segment of session.segments || []) {
    if (segment.ended_at == null) {
      segment.ended_at = resolvedAt;
      segment.duration_seconds = Math.max(0, Math.floor((resolvedAt - Number(segment.started_at)) / 1000));
    }
  }
  session.state = resolutionType === "discard" ? "discarded" : "resolved";
  session.resolution_type = resolutionType;
  session.generated_entry_id = body.p_payload?.generated_entry_id || null;
  session.resolved_at = resolvedAt;
  session.last_ended_at = resolvedAt;
  session.updated_at = resolvedAt;
  session.duration_seconds = (session.segments || []).reduce((sum, segment) => sum + Number(segment.duration_seconds || 0), 0);
  session.version = expectedVersion + 1;
  session.resolution_operation_id = operationId;
  const result = { accepted: true, session };
  state.processedOperations[operationId] = result;
  recordChange("unassigned_sessions", session.id, "update", session.version);
  return result;
}

function incremental(body) {
  const after = Math.max(0, Number(body.p_after_change_seq || 0));
  const limit = Math.max(1, Math.min(1000, Number(body.p_limit || 200)));
  const changes = state.changes.filter((change) => change.change_seq > after).slice(0, limit);
  const covered = changes.at(-1)?.change_seq ?? after;
  const latest = new Map();
  for (const change of changes) latest.set(`${change.entity_type}|${change.entity_id}`, change);
  const entities = [...latest.values()].sort((a, b) => a.change_seq - b.change_seq).map((change) => {
    const entityType = aggregateType(change.entity_type);
    const data = state.entities[entityType]?.[change.entity_id] ?? null;
    return { entity_type: entityType, source_entity_type: change.entity_type, entity_id: change.entity_id, change_seq: change.change_seq, deleted: data === null, data };
  });
  return { changes, entities, covered_change_seq: covered, latest_change_seq: state.latestChangeSeq, has_more: covered < state.latestChangeSeq, reset_required: false };
}

function snapshot() {
  const result = { latest_change_seq: state.latestChangeSeq, time_segments: [], time_allocations: [], unassigned_sessions: [], unassigned_segments: [], report_tasks: [] };
  for (const [key, rows] of Object.entries(state.entities)) {
    result[key] = key === "task_recurrence_rules"
      ? Object.values(rows).flatMap((value) => value.rules || [])
      : Object.values(rows);
  }
  for (const entry of result.time_entries || []) { result.time_segments.push(...(entry.segments || [])); result.time_allocations.push(...(entry.allocations || [])); }
  for (const report of result.reports || []) result.report_tasks.push(...(report.report_tasks || []));
  return result;
}

const server = http.createServer(async (request, response) => {
  const url = new URL(request.url, `http://127.0.0.1:${port}`);
  if (url.pathname === "/control/state") return json(response, 200, state);
  if (url.pathname === "/control/delay") { state.control.delayMs = Number(url.searchParams.get("ms") || 0); save(); return json(response, 200, state.control); }
  if (url.pathname === "/control/fail-next") { state.control.failNext = Number(url.searchParams.get("count") || 1); save(); return json(response, 200, state.control); }
  if (url.pathname === "/control/reset") { state = freshState(); save(); return json(response, 200, { reset: true }); }
  if (url.pathname === "/control/unassigned/create") {
    const secondsAgo = Math.max(0, Number(url.searchParams.get("secondsAgo") || 0));
    const session = getOrCreateSharedUnassigned({ p_started_at: Date.now() - secondsAgo * 1000 });
    return json(response, 200, session);
  }
  if (url.pathname === "/control/unassigned/age") {
    const session = activeUnassignedSession();
    if (!session) return json(response, 404, { message: "no active unassigned session" });
    const seconds = Math.max(0, Number(url.searchParams.get("seconds") || 0));
    const boundary = Date.now() - seconds * 1000;
    session.first_started_at = boundary;
    session.created_at = Math.min(Number(session.created_at || boundary), boundary);
    session.updated_at = Date.now();
    session.version = Number(session.version || 0) + 1;
    for (const segment of session.segments || []) {
      if (segment.ended_at == null) segment.started_at = boundary;
    }
    recordChange("unassigned_sessions", session.id, "update", session.version);
    return json(response, 200, session);
  }
  if (url.pathname === "/control/unassigned/awaiting") {
    const session = activeUnassignedSession();
    if (!session) return json(response, 404, { message: "no active unassigned session" });
    const seconds = Math.max(0, Number(url.searchParams.get("seconds") || 360));
    const boundary = Date.now() - seconds * 1000;
    session.state = "awaiting_resolution";
    session.first_started_at = boundary;
    session.duration_seconds = seconds;
    session.prompted_at = Date.now();
    session.updated_at = session.prompted_at;
    session.version = Number(session.version || 0) + 1;
    recordChange("unassigned_sessions", session.id, "update", session.version);
    return json(response, 200, session);
  }
  if (url.pathname === "/auth/v1/settings") return json(response, 200, { external: {}, disable_signup: false });
  const body = await bodyJson(request);
  if (url.pathname.endsWith("/device_authorization_get")) return json(response, 200, { authorized: true });
  if (url.pathname.endsWith("/cloud_incremental_pull")) return json(response, 200, incremental(body));
  if (url.pathname.endsWith("/cloud_snapshot_get")) return json(response, 200, snapshot());
  if (url.pathname.endsWith("/unassigned_get_or_create_shared")) return json(response, 200, getOrCreateSharedUnassigned(body));
  if (url.pathname.endsWith("/unassigned_resolve_shared")) return json(response, 200, resolveSharedUnassigned(body));
  if (url.pathname.includes("tracking_lease")) return json(response, 200, { lease_token: "dev-lease", expires_at: new Date(Date.now() + 90000).toISOString() });
  if (url.pathname.endsWith("/cloud_apply_patch")) {
    if (state.control.delayMs > 0) await new Promise((resolve) => setTimeout(resolve, state.control.delayMs));
    if (state.control.failNext > 0) { state.control.failNext -= 1; save(); return json(response, 503, { message: "temporary acceptance failure" }); }
    try { return json(response, 200, applyPatch(body)); } catch (error) { return json(response, 409, { message: error.message }); }
  }
  json(response, 404, { message: `unexpected request ${request.method} ${url.pathname}` });
});

function sendText(socket, text) {
  const payload = Buffer.from(text);
  let header;
  if (payload.length < 126) header = Buffer.from([0x81, payload.length]);
  else { header = Buffer.alloc(4); header[0] = 0x81; header[1] = 126; header.writeUInt16BE(payload.length, 2); }
  socket.write(Buffer.concat([header, payload]));
}

function decodeFrames(buffer) {
  const messages = [];
  let offset = 0;
  while (buffer.length - offset >= 2) {
    const second = buffer[offset + 1];
    const masked = (second & 0x80) !== 0;
    let length = second & 0x7f;
    let header = 2;
    if (length === 126) { if (buffer.length - offset < 4) break; length = buffer.readUInt16BE(offset + 2); header = 4; }
    const total = header + (masked ? 4 : 0) + length;
    if (buffer.length - offset < total) break;
    let payload = Buffer.from(buffer.subarray(offset + header + (masked ? 4 : 0), offset + total));
    if (masked) { const mask = buffer.subarray(offset + header, offset + header + 4); payload = payload.map((byte, index) => byte ^ mask[index % 4]); }
    messages.push({ opcode: buffer[offset] & 0x0f, text: payload.toString("utf8") });
    offset += total;
  }
  return { messages, rest: buffer.subarray(offset) };
}

server.on("upgrade", (request, socket) => {
  const accept = crypto.createHash("sha1").update(`${request.headers["sec-websocket-key"]}258EAFA5-E914-47DA-95CA-C5AB0DC85B11`).digest("base64");
  socket.write(`HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${accept}\r\n\r\n`);
  sockets.add(socket);
  let pending = Buffer.alloc(0);
  socket.on("data", (chunk) => {
    pending = Buffer.concat([pending, chunk]);
    const decoded = decodeFrames(pending); pending = decoded.rest;
    for (const message of decoded.messages) {
      if (message.opcode === 8) return socket.end();
      if (message.opcode !== 1) continue;
      let value; try { value = JSON.parse(message.text); } catch { continue; }
      if (value.event === "phx_join") sendText(socket, JSON.stringify({ topic: value.topic, event: "phx_reply", payload: { status: "ok", response: {} }, ref: value.ref, join_ref: value.join_ref }));
      else if (value.event === "heartbeat") sendText(socket, JSON.stringify({ topic: "phoenix", event: "phx_reply", payload: { status: "ok", response: {} }, ref: value.ref }));
    }
  });
  socket.on("close", () => sockets.delete(socket));
  socket.on("error", () => sockets.delete(socket));
});

server.listen(port, "127.0.0.1", () => {
  save();
  console.log(JSON.stringify({ ready: true, url: `http://127.0.0.1:${port}`, workspaceId, subjectId, statePath }));
});
