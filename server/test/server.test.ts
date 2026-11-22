// 端到端测试：真的 PostgreSQL（docker compose up -d），每次测试建一个临时数据库，跑完删掉。
// 走一遍完整流程：电脑端（Mac）用邮箱验证码登录 → 登记设备 → 生成配对码 → 手机扫码登录 → 互相转发 → 离线补发 → 解除设备。

import { randomUUID } from "node:crypto";
import type { AddressInfo } from "node:net";

import pg from "pg";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import WebSocket from "ws";

import { authOptions, createAuth } from "../src/auth.js";
import { buildApp } from "../src/app.js";
import { loadConfig } from "../src/config.js";
import { migrate } from "../src/db.js";
import { MemoryMailer } from "../src/mailer.js";

process.env.NODE_ENV = "test";
const ADMIN_URL = process.env.TEST_DATABASE_URL ?? "postgres://duolo:duolo@localhost:54329/duolo";
const dbName = `duolo_test_${randomUUID().replace(/-/g, "").slice(0, 12)}`;

let base = "";
let wsBase = "";
const mailer = new MemoryMailer();
let close: () => Promise<void>;

beforeAll(async () => {
  const admin = new pg.Client({ connectionString: ADMIN_URL });
  await admin.connect();
  await admin.query(`create database ${dbName}`);
  await admin.end();

  const databaseUrl = ADMIN_URL.replace(/\/[^/]*$/, `/${dbName}`);
  const pool = new pg.Pool({ connectionString: databaseUrl });
  const config = { ...loadConfig({ NODE_ENV: "test" }), databaseUrl, publicUrl: "http://localhost" };
  await migrate(pool, authOptions(config, pool, mailer));
  const app = await buildApp({ config, pool, auth: createAuth(config, pool, mailer) });
  await app.listen({ port: 0, host: "127.0.0.1" });
  const { port } = app.server.address() as AddressInfo;
  base = `http://127.0.0.1:${port}`;
  wsBase = `ws://127.0.0.1:${port}`;
  close = async () => {
    await app.close();
    await pool.end();
    const admin = new pg.Client({ connectionString: ADMIN_URL });
    await admin.connect();
    await admin.query(`drop database if exists ${dbName} with (force)`);
    await admin.end();
  };
});

afterAll(async () => close?.());

async function call(path: string, init: { method?: string; body?: unknown; token?: string; lang?: string } = {}) {
  const headers: Record<string, string> = { "content-type": "application/json" };
  if (init.token) headers.authorization = `Bearer ${init.token}`;
  if (init.lang) headers["accept-language"] = init.lang;
  const res = await fetch(base + path, {
    method: init.method ?? (init.body ? "POST" : "GET"),
    headers,
    body: init.body ? JSON.stringify(init.body) : undefined,
  });
  const text = await res.text();
  return { status: res.status, json: text ? JSON.parse(text) : null };
}

/** 邮箱验证码登录，返回令牌。 */
async function signIn(email: string, lang = "en") {
  const sent = await call("/v1/auth/email-otp/send-verification-otp", { body: { email, type: "sign-in" }, lang });
  expect(sent.status).toBe(200);
  await new Promise((r) => setTimeout(r, 20));
  const mail = mailer.sent.filter((m) => m.to === email).at(-1)!;
  const otp = mail.text.match(/\b\d{6}\b/)![0];
  const res = await call("/v1/auth/sign-in/email-otp", { body: { email, otp } });
  expect(res.status, JSON.stringify(res.json)).toBe(200);
  return { token: res.json.token as string, mail };
}

/** 一条 WebSocket 连接，收到的消息按顺序存起来。 */
class Client {
  readonly received: Record<string, unknown>[] = [];
  readonly socket: WebSocket;
  closed: number | null = null;
  constructor(token: string) {
    this.socket = new WebSocket(`${wsBase}/v1/connect`, { headers: { authorization: `Bearer ${token}` } });
    this.socket.on("message", (data) => this.received.push(JSON.parse(data.toString())));
    this.socket.on("close", (code) => (this.closed = code));
  }
  send(message: object) {
    this.socket.send(JSON.stringify(message));
  }
  async waitFor(match: (m: Record<string, unknown>) => boolean, what: string) {
    for (let i = 0; i < 100; i++) {
      const hit = this.received.find(match);
      if (hit) return hit;
      await new Promise((r) => setTimeout(r, 30));
    }
    throw new Error(`timed out waiting for ${what}: ${JSON.stringify(this.received)}`);
  }
  async closeAndWait() {
    this.socket.close();
    for (let i = 0; i < 100 && this.closed === null; i++) await new Promise((r) => setTimeout(r, 20));
  }
}

const key = (c: string) => Buffer.alloc(32, c).toString("base64");

describe("Duolo server", () => {
  let macToken = "";
  let phoneToken = "";
  let macId = "";
  let phoneId = "";

  it("signs in with an email code, in the user's language", async () => {
    const { token, mail } = await signIn("ada@example.com", "zh-CN,zh;q=0.9");
    expect(mail.subject).toMatch(/^你的 Duolo 登录验证码：\d{6}$/);
    expect(mail.text).toContain("10 分钟内有效");
    macToken = token;
    const unauthorized = await call("/v1/devices");
    expect(unauthorized).toEqual({ status: 401, json: { error: "unauthorized" } });
  });

  it("registers the Mac and pairs a phone with a one-time code", async () => {
    const mac = await call("/v1/devices", {
      token: macToken,
      body: { name: "MacBook Pro", platform: "mac", publicKey: key("m") },
    });
    expect(mac.status).toBe(200);
    macId = mac.json.device.id;

    const pairing = await call("/v1/pairings", { token: macToken, method: "POST" });
    expect(pairing.json.code).toMatch(/^[2-9A-HJKMNP-Z]{8}$/);

    const claim = await call("/v1/pairings/claim", {
      body: { code: pairing.json.code.toLowerCase(), device: { name: "iPhone", platform: "ios", publicKey: key("p") } },
    });
    expect(claim.status, JSON.stringify(claim.json)).toBe(200);
    expect(claim.json.desktop).toMatchObject({ id: macId, publicKey: key("m") });
    phoneToken = claim.json.token;
    phoneId = claim.json.device.id;

    const again = await call("/v1/pairings/claim", {
      body: { code: pairing.json.code, device: { name: "iPhone 2", platform: "ios", publicKey: key("q") } },
    });
    expect(again).toEqual({ status: 404, json: { error: "invalidPairingCode" } });

    const list = await call("/v1/devices", { token: phoneToken });
    expect(list.json.devices.map((d: { platform: string }) => d.platform)).toEqual(["mac", "ios"]);
  });

  it("relays messages between the user's devices and reports presence", async () => {
    const mac = new Client(macToken);
    const hello = await mac.waitFor((m) => m.type === "hello", "mac hello");
    expect(hello).toMatchObject({ device: { id: macId }, peers: [{ id: phoneId, online: false }] });

    const phone = new Client(phoneToken);
    await phone.waitFor((m) => m.type === "hello", "phone hello");
    await mac.waitFor((m) => m.type === "presence" && m.online === true, "phone online");

    mac.send({ type: "send", to: phoneId, id: "e1", body: "ciphertext-1" });
    await mac.waitFor((m) => m.type === "sent" && m.id === "e1", "sent");
    const got = await phone.waitFor((m) => m.type === "message" && m.id === "e1", "message");
    expect(got).toMatchObject({ from: macId, body: "ciphertext-1" });
    phone.send({ type: "ack", ids: ["e1"] });

    // 手机离线时发的，上线后补发；确认过的不再发。
    await phone.closeAndWait();
    await mac.waitFor((m) => m.type === "presence" && m.online === false, "phone offline");
    mac.send({ type: "send", to: phoneId, id: "e2", body: "ciphertext-2" });
    await mac.waitFor((m) => m.type === "sent" && m.id === "e2", "sent e2");
    const back = new Client(phoneToken);
    await back.waitFor((m) => m.type === "message" && m.id === "e2", "queued message");
    expect(back.received.some((m) => m.id === "e1")).toBe(false);
    back.send({ type: "ack", ids: ["e2"] });
    await back.closeAndWait();
    await mac.closeAndWait();
  });

  it("lets Windows and Linux desktops pair too, but not phones", async () => {
    const denied = await call("/v1/pairings", { token: phoneToken, method: "POST" });
    expect(denied).toEqual({ status: 403, json: { error: "desktopOnly" } });
    for (const platform of ["windows", "linux"]) {
      const { token } = await signIn(`${platform}@example.com`);
      await call("/v1/devices", { token, body: { name: `${platform} PC`, platform, publicKey: key("w") } });
      const pairing = await call("/v1/pairings", { token, method: "POST" });
      expect(pairing.status, platform).toBe(200);
    }
  });

  it("never relays to another user's devices", async () => {
    const { token } = await signIn("eve@example.com");
    await call("/v1/devices", { token, body: { name: "Eve's PC", platform: "windows", publicKey: key("e") } });
    const eve = new Client(token);
    await eve.waitFor((m) => m.type === "hello", "eve hello");
    eve.send({ type: "send", to: phoneId, id: "x", body: "hi" });
    await eve.waitFor((m) => m.type === "error" && m.code === "unknownDevice", "rejected");
    await eve.closeAndWait();
  });

  it("unlinking a device signs it out and disconnects it", async () => {
    const phone = new Client(phoneToken);
    await phone.waitFor((m) => m.type === "hello", "phone hello");
    const res = await call(`/v1/devices/${phoneId}`, { token: macToken, method: "DELETE" });
    expect(res.json).toEqual({ ok: true });
    for (let i = 0; i < 100 && phone.closed === null; i++) await new Promise((r) => setTimeout(r, 20));
    expect(phone.closed).toBe(4401);
    expect((await call("/v1/devices", { token: phoneToken })).status).toBe(401);
  });
});
