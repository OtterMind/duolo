// 手机端联调沙箱：真的服务端（临时数据库）加一台模拟电脑，手机可以真的配对、收发、回答。
//
//   docker compose up -d        # PostgreSQL
//   pnpm mobile-sandbox         # 打印配对链接；要在真机上用：pnpm mobile-sandbox -- --host 192.168.x.x
//
// GET http://localhost:8788/link   换一个新的配对链接（配对码只能用一次）
// GET http://localhost:8788/state  模拟电脑现在的状态和收到过的命令（自动化测试用）
// POST http://localhost:8788/offline、/online  让模拟电脑离线、上线
//
// 模拟电脑按 docs/mobile.md 的约定收发，Rust 的实现接上之前由它当参照。

import { createCipheriv, createDecipheriv, createPublicKey, diffieHellman, generateKeyPairSync, hkdfSync, randomBytes, randomUUID } from "node:crypto";
import { readFileSync } from "node:fs";
import { createServer } from "node:http";

import pg from "pg";
import WebSocket from "ws";

import { buildApp } from "../src/app.js";
import { authOptions, createAuth } from "../src/auth.js";
import { loadConfig } from "../src/config.js";
import { migrate } from "../src/db.js";
import { MemoryMailer } from "../src/mailer.js";

process.env.NODE_ENV = "test";
const arg = (name: string, fallback: string) => {
  const i = process.argv.indexOf(`--${name}`);
  return i > 0 ? process.argv[i + 1] : fallback;
};
const host = arg("host", "localhost");
const port = Number(arg("port", "8787"));
const controlPort = Number(arg("control", "8788"));

// ---- 服务端 ----

const ADMIN_URL = process.env.TEST_DATABASE_URL ?? "postgres://duolo:duolo@localhost:54329/duolo";
const dbName = `duolo_sandbox_${randomUUID().replace(/-/g, "").slice(0, 12)}`;
const admin = new pg.Client({ connectionString: ADMIN_URL });
await admin.connect();
await admin.query(`create database ${dbName}`);

// 中途出错也要删掉临时库。
async function dropDatabase() {
  await admin.query(`drop database if exists ${dbName} with (force)`);
  await admin.end();
}
process.on("uncaughtException", (err) => {
  console.error(err);
  void dropDatabase().finally(() => process.exit(1));
});

const databaseUrl = ADMIN_URL.replace(/\/[^/]*$/, `/${dbName}`);
const pool = new pg.Pool({ connectionString: databaseUrl });
const mailer = new MemoryMailer();
const config = { ...loadConfig({ NODE_ENV: "test" }), databaseUrl, publicUrl: `http://${host}:${port}` };
await migrate(pool, authOptions(config, pool, mailer));
const app = await buildApp({ config, pool, auth: createAuth(config, pool, mailer) });
await app.listen({ port, host: "0.0.0.0" });
const base = `http://127.0.0.1:${port}`;

async function call(path: string, init: { method?: string; body?: unknown; token?: string } = {}) {
  // Node 的 fetch 会带 sec-fetch-*，better-auth 当成浏览器请求，要求有 Origin。
  const headers: Record<string, string> = { "content-type": "application/json", origin: config.publicUrl };
  if (init.token) headers.authorization = `Bearer ${init.token}`;
  const res = await fetch(base + path, {
    method: init.method ?? (init.body ? "POST" : "GET"),
    headers,
    body: init.body ? JSON.stringify(init.body) : undefined,
  });
  if (!res.ok) throw new Error(`${path}: ${res.status} ${await res.text()}`);
  return res.json();
}

// ---- 模拟电脑：登录、密钥、配对 ----

const email = "sandbox@example.com";
await call("/v1/auth/email-otp/send-verification-otp", { body: { email, type: "sign-in" } });
await new Promise((r) => setTimeout(r, 50));
const otp = mailer.sent.at(-1)!.text.match(/\b\d{6}\b/)![0];
const { token } = await call("/v1/auth/sign-in/email-otp", { body: { email, otp } });

const keys = generateKeyPairSync("x25519");
const rawPublic = keys.publicKey.export({ format: "der", type: "spki" }).subarray(-32);
const SPKI = Buffer.from("302a300506032b656e032100", "hex");
const me = (await call("/v1/devices", { token, body: { name: "MacBook Pro", platform: "mac", publicKey: rawPublic.toString("base64") } })).device;

/** 给出去、还没有手机用上的密码。 */
const openSecrets: Buffer[] = [];
async function pairingLink() {
  const { code } = await call("/v1/pairings", { token, method: "POST" });
  const secret = randomBytes(32);
  openSecrets.push(secret);
  const q = new URLSearchParams({ v: "1", server: config.publicUrl, code, key: rawPublic.toString("base64url"), secret: secret.toString("base64url") });
  return `duolo://pair?${q}`;
}

interface Phone {
  id: string;
  toPhone: Buffer;
  fromPhone: Buffer;
}
const phones = new Map<string, Phone>();

function channel(id: string, phoneKey: Buffer, secret: Buffer): Phone {
  const peer = createPublicKey({ key: Buffer.concat([SPKI, phoneKey]), format: "der", type: "spki" });
  const shared = diffieHellman({ privateKey: keys.privateKey, publicKey: peer });
  const derive = (direction: string) =>
    Buffer.from(hkdfSync("sha256", shared, secret, Buffer.concat([Buffer.from(`duolo/v1 ${direction}`), rawPublic, phoneKey]), 32));
  return { id, toPhone: derive("desktop->phone"), fromPhone: derive("phone->desktop") };
}
const header = (id: string, from: string, to: string) => Buffer.from(`duolo/v1\n${id}\n${from}\n${to}`);

function open(key: Buffer, body: string, aad: Buffer) {
  const data = Buffer.from(body, "base64");
  if (data[0] !== 1) throw new Error("unknown version");
  const decipher = createDecipheriv("chacha20-poly1305", key, data.subarray(1, 13), { authTagLength: 16 });
  decipher.setAAD(aad, { plaintextLength: data.length - 29 });
  decipher.setAuthTag(data.subarray(-16));
  return Buffer.concat([decipher.update(data.subarray(13, -16)), decipher.final()]).toString();
}

/** 认出是哪台手机：第一次来的，用没用过的密码逐个试，能解开的就是它的。 */
async function identify(from: string, body: string, aad: Buffer): Promise<{ phone: Phone; text: string } | null> {
  const known = phones.get(from);
  if (known) return { phone: known, text: open(known.fromPhone, body, aad) };
  const { devices } = await call("/v1/devices", { token });
  const device = devices.find((d: { id: string }) => d.id === from);
  for (const secret of device ? openSecrets : []) {
    try {
      const phone = channel(from, Buffer.from(device.publicKey, "base64"), secret);
      const text = open(phone.fromPhone, body, aad);
      phones.set(from, phone);
      openSecrets.splice(openSecrets.indexOf(secret), 1);
      console.log(`[sandbox] paired ${device.name}`);
      return { phone, text };
    } catch {
      // 不是这个密码。
    }
  }
  return null;
}

// ---- 模拟电脑：任务 ----

type Event = { task: number; seq: number; at: number; type: string } & Record<string, unknown>;
interface Task {
  id: number;
  project: string;
  /** 项目里的哪个仓库；项目本身就是仓库时为空。 */
  repo?: string;
  title: string;
  branch: string;
  status: string;
  writer: string;
  reviewer: string;
  since: number;
  updatedAt: number;
  prompt?: { seq: number; question: string; detail: string; options: string[] };
  reviewingSince?: number;
  review?: { finishedAt: number; ok: boolean; verdict: string; issues: number; summary: string; output: string };
  diff?: { files: number; insertions: number; deletions: number };
  events: Event[];
}

const NAMES: Record<string, string> = { claude: "Claude", codex: "Codex" };
/** 图标颜色和电脑一样取插件的 icon_color。 */
const colorOf = (kind: string) => {
  try {
    const toml = readFileSync(new URL(`../../crates/duolo-agent/agents/${kind}/agent.toml`, import.meta.url), "utf8");
    return toml.match(/^icon_color\s*=\s*"([^"]+)"/m)?.[1];
  } catch {
    return undefined;
  }
};

/** 列表状态，和 crates/duolo-core/src/wire.rs 的 TaskState::of 同样的规则（沙箱代替电脑算）。 */
function stateOf(t: Task): string {
  if (t.status === "needs_input") return "needs_you";
  if (t.reviewingSince != null) return "reviewing";
  if (t.status === "running") return "writing";
  if (t.status === "failed") return "crashed";
  const r = t.review;
  if (r && !(t.status === "idle" && t.since > r.finishedAt)) {
    if (!r.ok) return "review_failed";
    return r.verdict === "issues" ? "issues" : r.verdict === "pass" ? "passed" : "reviewed";
  }
  return t.status === "idle" ? "written" : t.status === "exited" ? "ended" : "not_started";
}
const minute = 60_000;
const tasks: Task[] = [];
let rev = Date.now();
let nextId = 1;

function addTask(project: string, title: string, branch: string, ago: number, writer = "claude", reviewer = "codex"): Task {
  const at = Date.now() - ago;
  const task: Task = { id: nextId++, project, repo: "", title, branch, status: "not_started", writer, reviewer, since: at, updatedAt: at, events: [] };
  tasks.push(task);
  return task;
}

function record(task: Task, type: string, fields: Record<string, unknown> = {}, at = Date.now()) {
  const event: Event = { task: task.id, seq: task.events.length + 1, at, type, ...fields };
  task.events.push(event);
  task.updatedAt = at;
  if (type === "status") {
    task.status = fields.status as string;
    task.since = at;
    if (task.status !== "needs_input") task.prompt = undefined;
  }
  if (type === "diff") task.diff = fields as Task["diff"];
  return event;
}
const say = (task: Task, speaker: "user" | "agent", text: string, at?: number) => record(task, "message", { speaker, agent: 0, text }, at);
const tool = (task: Task, name: string, target: string, at?: number) => record(task, "tool", { agent: 0, tool: name, target }, at);
const status = (task: Task, value: string, at?: number) => record(task, "status", { agent: 0, status: value }, at);

function ask(task: Task, question: string, detail: string, options: string[], at?: number) {
  status(task, "needs_input", at);
  const event = record(task, "prompt", { agent: 0, question, detail, options }, at);
  task.prompt = { seq: event.seq, question, detail, options };
}

function seed() {
  const now = Date.now();
  const csv = addTask("shop", "Export orders as CSV", "feat/order-csv-export", 9 * minute);
  say(csv, "user", "Add CSV export to the order list, with a date filter", now - 9 * minute);
  tool(csv, "Read", "src/orders/list.tsx", now - 8 * minute);
  tool(csv, "Edit", "src/orders/list.tsx", now - 5 * minute);
  tool(csv, "Write", "src/orders/export.ts", now - 4 * minute);
  say(csv, "agent", "The export works and can be filtered by date. I'd like to run the tests to make sure nothing else broke.", now - 2 * minute);
  record(csv, "diff", { files: 3, insertions: 96, deletions: 4 }, now - 2 * minute);
  ask(csv, "Do you want to proceed?", "pnpm test --filter orders", ["Yes", "Yes, and don't ask again for this task", "No, and tell Claude what to do differently"], now - minute);

  const deps = addTask("shop", "Upgrade dependencies", "chore/deps", 26 * 60 * minute, "codex", "claude");
  say(deps, "user", "Upgrade everything to the latest version", now - 26 * 60 * minute);
  tool(deps, "shell", "pnpm update --latest", now - 25 * 60 * minute);
  say(deps, "agent", "Everything is on the latest version. Build and tests pass.", now - 24 * 60 * minute);
  record(deps, "diff", { files: 3, insertions: 128, deletions: 94 }, now - 24 * 60 * minute);
  status(deps, "idle", now - 24 * 60 * minute);
  finishReview(deps, "issues", "CI still uses Node 18, so the build fails after merging. One more below.", "[ISSUES 2] CI still uses Node 18, so the build fails after merging.\n\n- **CI uses Node 18.** The new Vite needs 20.19 or later.\n- **Old esbuild in the lockfile.** Two copies get installed.", now - 23 * 60 * minute);

  const login = addTask("shop", "Fix the login redirect loop", "fix/login-redirect-loop", 6 * minute);
  say(login, "user", "After signing in it bounces between the login page and the home page. Please fix it.", now - 6 * minute);
  for (const f of ["router/guard.ts", "session/restore.ts", "pages/Login.tsx"]) tool(login, "Read", f, now - 5 * minute);
  say(login, "agent", "The route guard runs before the session is restored: at that moment there is no signed-in state, so it goes to the login page; once restored, the login page sends you back home.", now - 4 * minute);
  for (const f of ["router/guard.ts", "session/restore.ts", "pages/Login.tsx", "tests/login.spec.ts"]) tool(login, "Edit", f, now - 3 * minute);
  record(login, "diff", { files: 4, insertions: 42, deletions: 8 }, now - 3 * minute);
  say(login, "agent", "Changed it. Running the related tests now.", now - 2 * minute);
  status(login, "running", now - 2 * minute);
  tool(login, "Bash", "pnpm test login", now - 2 * minute);

  const sidebar = addTask("shop", "Rework the sidebar", "feat/sidebar", 40 * minute);
  sidebar.repo = "web";
  say(sidebar, "agent", "Drag to reorder and collapse both work now.", now - 2 * minute);
  status(sidebar, "idle", now - 2 * minute);
  sidebar.reviewingSince = now - minute;
  record(sidebar, "review_started", { reviewer: "codex" }, now - minute);

  const pay = addTask("shop", "Refactor payment callbacks", "refactor/payment", 180 * minute);
  say(pay, "agent", "Repeated notifications no longer post twice.", now - 130 * minute);
  status(pay, "idle", now - 130 * minute);
  finishReview(pay, "pass", "No issues found", "[PASS] No issues found", now - 120 * minute);

  const images = addTask("shop", "Move image storage", "feat/images", 200 * minute);
  say(images, "agent", "Copying the second batch of images.", now - 190 * minute);
  status(images, "failed", now - 180 * minute);

  const search = addTask("shop", "Highlight search results", "feat/highlight", 30 * minute);
  say(search, "agent", "Matching words are highlighted.", now - 20 * minute);
  status(search, "idle", now - 20 * minute);

  const tracking = addTask("shop", "Remove old tracking events", "chore/tracking", 30 * 60 * minute);
  say(tracking, "agent", "Removed the events nobody reports any more.", now - 29 * 60 * minute);
  status(tracking, "exited", now - 29 * 60 * minute);
}

function finishReview(task: Task, verdict: string, summary: string, output: string, at = Date.now()) {
  const reviewer = task.reviewer;
  if (!task.events.some((e) => e.type === "review_started")) record(task, "review_started", { reviewer }, at - minute);
  const issues = verdict === "issues" ? 2 : 0;
  task.reviewingSince = undefined;
  task.review = { finishedAt: at, ok: true, verdict, issues, summary, output };
  record(task, "review_finished", { ok: true, verdict, issues, summary }, at);
}

const agent = (kind: string) => ({ kind, name: NAMES[kind] ?? kind, ...(colorOf(kind) ? { color: colorOf(kind) } : {}) });
function overview() {
  return {
    v: 1,
    type: "overview",
    rev: ++rev,
    projects: [
      { id: "shop", name: "shop", branch: "main", branches: ["main", "develop"] },
      { id: "docs", name: "docs", branch: "main", branches: ["main"] },
    ],
    tasks: tasks.map((t) => {
      const { review, events, writer, reviewer, ...rest } = t;
      const last = events.findLast((e) => e.type === "message")?.text ?? "";
      const { output: _, ...brief } = review ?? ({} as Partial<NonNullable<Task["review"]>>);
      const state = stateOf(t);
      return { ...rest, state, untitled: false, writer: agent(writer), reviewer: agent(reviewer), last, seq: events.length, ...(review ? { review: brief } : {}) };
    }),
    agents: Object.keys(NAMES).map(agent),
    writer: "claude",
    reviewer: "codex",
  };
}

// ---- 模拟电脑：收发 ----

let socket: WebSocket | null = null;
let wanted = true;
const received: unknown[] = [];
const done = new Map<string, unknown>();

function sendTo(phone: Phone, message: object) {
  if (socket?.readyState !== WebSocket.OPEN) return;
  const id = randomUUID();
  const nonce = randomBytes(12);
  const plaintext = Buffer.from(JSON.stringify(message));
  const cipher = createCipheriv("chacha20-poly1305", phone.toPhone, nonce, { authTagLength: 16 });
  cipher.setAAD(header(id, me.id, phone.id), { plaintextLength: plaintext.length });
  const body = Buffer.concat([Buffer.from([1]), nonce, cipher.update(plaintext), cipher.final(), cipher.getAuthTag()]);
  socket.send(JSON.stringify({ type: "send", to: phone.id, id, body: body.toString("base64") }));
}

const online = new Set<string>();
/** 只推给在线的手机：离线的上线后自己会要。 */
function broadcast(message: object) {
  for (const phone of phones.values()) if (online.has(phone.id)) sendTo(phone, message);
}
function changed(task: Task, from: number) {
  broadcast({ v: 1, type: "events", task: task.id, events: task.events.slice(from), latest: task.events.length });
  broadcast(overview());
}
/** 改任务，并把新的事件推出去。 */
function update(task: Task, body: () => void) {
  const from = task.events.length;
  body();
  changed(task, from);
}
const later = (ms: number, body: () => void) => setTimeout(body, ms);

function execute(command: Record<string, unknown>): { ok: boolean; task?: number; error?: string } {
  if (command.type === "create_task") {
    const prompt = String(command.prompt);
    const task = addTask(String(command.project), prompt.split("\n")[0].slice(0, 60), `duolo/${nextId}`, 0, String(command.writer ?? "claude"), String(command.reviewer ?? "codex"));
    update(task, () => {
      record(task, "task_created", { title: task.title, branch: task.branch });
      say(task, "user", prompt);
      status(task, "running");
      tool(task, "Read", "README.md");
    });
    later(6000, () => update(task, () => (say(task, "agent", "Done. Have a look."), status(task, "idle"))));
    return { ok: true, task: task.id };
  }
  const task = tasks.find((t) => t.id === command.task);
  if (!task) return { ok: false, error: "taskNotFound" };
  switch (command.type) {
    case "send_message":
      update(task, () => (say(task, "user", String(command.text)), status(task, "running"), tool(task, "Grep", "TODO")));
      later(5000, () => update(task, () => (say(task, "agent", `Got it: ${command.text}`), status(task, "idle"))));
      return { ok: true };
    case "answer": {
      const prompt = task.prompt;
      const option = Number(command.option);
      if (!prompt || prompt.seq !== command.prompt || option >= prompt.options.length) return { ok: false, error: "promptChanged" };
      update(task, () => {
        status(task, "running");
        if (option < 2) tool(task, "Bash", prompt.detail);
      });
      later(6000, () => update(task, () => (say(task, "agent", option < 2 ? "All tests pass." : "OK, I won't run it."), status(task, "idle"))));
      return { ok: true };
    }
    case "interrupt":
      update(task, () => status(task, "idle"));
      return { ok: true };
    case "start_review":
      update(task, () => ((task.reviewingSince = Date.now()), record(task, "review_started", { reviewer: task.reviewer })));
      later(5000, () => update(task, () => finishReview(task, "pass", "No issues found", "[PASS] No issues found")));
      return { ok: true };
    case "close_task":
      update(task, () => record(task, "task_closed"));
      tasks.splice(tasks.indexOf(task), 1);
      broadcast(overview());
      return { ok: true };
    default:
      return { ok: false, error: "internal" };
  }
}

function onPhoneMessage(phone: Phone, text: string) {
  const message = JSON.parse(text);
  received.push(message);
  switch (message.type) {
    case "sync":
      return sendTo(phone, overview());
    case "fetch": {
      const task = tasks.find((t) => t.id === message.task);
      if (!task) return;
      // 分页：一次最多 200 条。
      const events = task.events.filter((e) => e.seq > message.after).slice(0, 200);
      sendTo(phone, { v: 1, type: "events", task: task.id, events, latest: task.events.length });
      if (task.review) sendTo(phone, { v: 1, type: "review", task: task.id, finishedAt: task.review.finishedAt, output: task.review.output });
      return;
    }
    case "command": {
      const live = ["answer", "interrupt"].includes(message.command.type);
      const late = Date.now() - message.at > (live ? 5 * minute : 7 * 24 * 60 * minute);
      // 同一个编号只执行一次，再来就回同样的结果。
      const result = done.get(message.id) ?? (late ? { ok: false, error: "commandExpired" } : execute(message.command));
      done.set(message.id, result);
      return sendTo(phone, { v: 1, type: "result", id: message.id, result });
    }
  }
}

function connect() {
  if (!wanted) return;
  const ws = new WebSocket(`${base.replace("http", "ws")}/v1/connect`, { headers: { authorization: `Bearer ${token}` } });
  socket = ws;
  let queue = Promise.resolve();
  ws.on("message", (data) => {
    queue = queue.then(() => onRelay(ws, JSON.parse(data.toString()))).catch((err) => console.error("[sandbox]", err));
  });
  ws.on("close", () => wanted && setTimeout(connect, 500));
}

async function onRelay(ws: WebSocket, m: Record<string, any>) {
  if (m.type === "hello") for (const p of m.peers) p.online ? online.add(p.id) : online.delete(p.id);
  if (m.type === "presence") m.online ? online.add(m.device) : online.delete(m.device);
  if (m.type !== "message") return;
  try {
    const hit = await identify(m.from, m.body, header(m.id, m.from, me.id));
    if (hit) onPhoneMessage(hit.phone, hit.text);
    else console.log("[sandbox] message from an unpaired phone ignored");
  } catch (err) {
    console.error("[sandbox] bad message", err);
  }
  ws.send(JSON.stringify({ type: "ack", ids: [m.id] }));
}

seed();
connect();

// ---- 控制口 ----

createServer(async (req, res) => {
  const reply = (body: string) => res.writeHead(200, { "content-type": "text/plain" }).end(body);
  switch (req.url) {
    case "/link":
      return reply(await pairingLink());
    case "/state":
      return reply(JSON.stringify({ received, tasks: tasks.map(({ events, ...t }) => ({ ...t, seq: events.length })), phones: [...phones.keys()] }));
    case "/offline":
      wanted = false;
      socket?.close();
      return reply("offline");
    case "/online":
      wanted = true;
      connect();
      return reply("online");
    default:
      return res.writeHead(404).end();
  }
}).listen(controlPort, "127.0.0.1");

console.log(`[sandbox] relay ${config.publicUrl}\n[sandbox] pair with:\n${await pairingLink()}`);

let closing = false;
async function shutdown() {
  if (closing) return;
  closing = true;
  wanted = false;
  socket?.close();
  await app.close();
  await pool.end();
  await dropDatabase();
  process.exit(0);
}
process.on("SIGINT", () => void shutdown());
process.on("SIGTERM", () => void shutdown());
