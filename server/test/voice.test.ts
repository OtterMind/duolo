// 语音识别转发：帧格式、热词和上下文，以及用一个假的豆包服务器走完整流程（匿名登录 → 识别 → 额度）。

import { randomUUID } from "node:crypto";
import { gunzipSync } from "node:zlib";
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";

import pg from "pg";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import WebSocket, { WebSocketServer } from "ws";

import { authOptions, createAuth } from "../src/auth.js";
import { buildApp } from "../src/app.js";
import { loadConfig } from "../src/config.js";
import { migrate } from "../src/db.js";
import { MemoryMailer } from "../src/mailer.js";
import { corpusContext, decodeFrame, encodeFrame } from "../src/voice.js";

process.env.NODE_ENV = "test";
const ADMIN_URL = process.env.TEST_DATABASE_URL ?? "postgres://duolo:duolo@localhost:54329/duolo";
const dbName = `duolo_test_${randomUUID().replace(/-/g, "").slice(0, 12)}`;

/** 豆包发回的一帧：头、序号、长度、JSON（不压缩）。 */
function serverFrame(sequence: number, body: object) {
  const payload = Buffer.from(JSON.stringify(body));
  const head = Buffer.alloc(12);
  head[0] = 0x11;
  head[1] = (0x9 << 4) | (sequence < 0 ? 0x3 : 0x1);
  head[2] = 0x10;
  head.writeInt32BE(sequence, 4);
  head.writeUInt32BE(payload.byteLength, 8);
  return Buffer.concat([head, payload]);
}

/** 假的豆包：记下收到的请求和声音，每收到一块声音回一次"识别中"，收到最后一帧回最终结果。 */
class FakeDoubao {
  readonly server = new WebSocketServer({ port: 0, host: "127.0.0.1" });
  headers: Record<string, unknown> = {};
  request: Record<string, any> = {};
  audioBytes = 0;
  constructor() {
    this.server.on("connection", (socket, req) => {
      this.headers = req.headers;
      socket.on("message", (data: Buffer) => {
        const type = data[1] >> 4;
        const sequence = data.readInt32BE(4);
        const body = data.subarray(12);
        if (type === 0x1) {
          this.request = JSON.parse(gunzipSync(body).toString());
        } else if (sequence > 0) {
          this.audioBytes += gunzipSync(body).byteLength;
          socket.send(serverFrame(sequence, { result: { text: "导出订单" } }));
        } else {
          socket.send(serverFrame(sequence, { result: { text: "导出订单为 CSV" } }));
        }
      });
    });
  }
  get url() {
    return `ws://127.0.0.1:${(this.server.address() as AddressInfo).port}`;
  }
}

const fake = new FakeDoubao();

/** 假的模型接口（OpenAI 兼容）：记下请求，把原话里的 "CSV" 换成 "CSV file" 作为整理结果。 */
const llmRequests: any[] = [];
const llm = createServer((req, res) => {
  let body = "";
  req.on("data", (c) => (body += c));
  req.on("end", () => {
    const request = JSON.parse(body);
    llmRequests.push({ url: req.url, auth: req.headers.authorization, request });
    const said = (request.messages[1].content as string).split("Transcript:\n")[1];
    res.setHeader("content-type", "application/json");
    res.end(JSON.stringify({ choices: [{ message: { content: said.replace("CSV", "CSV file") } }] }));
  });
});
let base = "";
let wsBase = "";
let close: () => Promise<void>;

beforeAll(async () => {
  await new Promise<void>((resolve) => llm.listen(0, "127.0.0.1", resolve));
  const llmUrl = `http://127.0.0.1:${(llm.address() as AddressInfo).port}/v1`;
  const admin = new pg.Client({ connectionString: ADMIN_URL });
  await admin.connect();
  await admin.query(`create database ${dbName}`);
  await admin.end();

  const databaseUrl = ADMIN_URL.replace(/\/[^/]*$/, `/${dbName}`);
  const pool = new pg.Pool({ connectionString: databaseUrl });
  const loaded = loadConfig({ NODE_ENV: "test" });
  const config = {
    ...loaded,
    databaseUrl,
    publicUrl: "http://localhost",
    // 匿名用户每天 2 秒，测得到超额。
    voice: {
      ...loaded.voice,
      url: fake.url,
      apiKey: "test-key",
      anonymousDailySeconds: 2,
      organize: { baseUrl: llmUrl, apiKey: "llm-key", model: "test-model", timeoutMs: 2000 },
    },
  };
  const mailer = new MemoryMailer();
  await migrate(pool, authOptions(config, pool, mailer));
  const app = await buildApp({ config, pool, auth: createAuth(config, pool, mailer) });
  await app.listen({ port: 0, host: "127.0.0.1" });
  const { port } = app.server.address() as AddressInfo;
  base = `http://127.0.0.1:${port}`;
  wsBase = `ws://127.0.0.1:${port}`;
  close = async () => {
    await app.close();
    await pool.end();
    fake.server.close();
    llm.close();
    const admin = new pg.Client({ connectionString: ADMIN_URL });
    await admin.connect();
    await admin.query(`drop database if exists ${dbName} with (force)`);
    await admin.end();
  };
});

afterAll(async () => close?.());

/** 匿名登录，返回令牌。 */
async function anonymousToken() {
  const res = await fetch(`${base}/v1/auth/sign-in/anonymous`, { method: "POST", headers: { "content-type": "application/json" }, body: "{}" });
  expect(res.status).toBe(200);
  return ((await res.json()) as { token: string }).token;
}

/** 说一段话：发 start、几块声音、stop，收齐服务器发回的消息。 */
async function dictate(token: string, start: object, chunks: number) {
  const ws = new WebSocket(`${wsBase}/v1/voice`, { headers: { authorization: `Bearer ${token}` } });
  const received: { type: string; text?: string; error?: string }[] = [];
  ws.on("message", (data) => received.push(JSON.parse(data.toString())));
  await new Promise((resolve) => ws.once("open", resolve));
  ws.send(JSON.stringify({ type: "start", ...start }));
  for (let i = 0; i < chunks; i++) ws.send(Buffer.alloc(6400)); // 200 ms
  ws.send(JSON.stringify({ type: "stop" }));
  await new Promise((resolve) => ws.once("close", resolve));
  return received;
}

describe("voice frames", () => {
  it("encodes a gzip frame with a negative sequence for the last one", () => {
    const frame = encodeFrame("audio", -3, Buffer.from("pcm"));
    expect(frame[1]).toBe((0x2 << 4) | 0x3);
    expect(frame.readInt32BE(4)).toBe(-3);
    expect(gunzipSync(frame.subarray(12)).toString()).toBe("pcm");
  });

  it("decodes results, the last frame and errors", () => {
    expect(decodeFrame(serverFrame(2, { result: { text: " hi " } }))).toEqual({ last: false, text: "hi" });
    expect(decodeFrame(serverFrame(-3, { result: { utterances: [{ text: "a" }, { text: "b" }] } }))).toEqual({ last: true, text: "ab" });
    const error = Buffer.alloc(12);
    error[0] = 0x11;
    error[1] = 0xf0;
    error.writeUInt32BE(45000001, 4);
    expect(decodeFrame(error)).toEqual({ last: true, errorCode: 45000001 });
  });

  it("puts hotwords and recent dialog into corpus.context, deduplicated and trimmed", () => {
    const context = corpusContext({ hotwords: ["useState", " useState ", "", "x".repeat(41)], context: ["Run pnpm test?", ""] });
    expect(JSON.parse(context!)).toEqual({
      hotwords: [{ word: "useState" }],
      context_type: "dialog_ctx",
      context_data: [{ text: "Run pnpm test?" }],
    });
    expect(corpusContext({ hotwords: [], context: [] })).toBeUndefined();
  });
});

describe("voice relay", () => {
  it("rejects connections without a session", async () => {
    const ws = new WebSocket(`${wsBase}/v1/voice`);
    const code = await new Promise((resolve) => ws.once("close", resolve));
    expect(code).toBe(4401);
  });

  it("lets an anonymous user dictate with hotwords and context, then enforces the daily quota", async () => {
    const token = await anonymousToken();
    const received = await dictate(token, { hotwords: ["CSV"], context: ["Which format?"] }, 5);

    expect(fake.headers["x-api-key"]).toBe("test-key");
    expect(fake.request.audio).toMatchObject({ format: "pcm", rate: 16000 });
    expect(JSON.parse(fake.request.request.corpus.context)).toMatchObject({ hotwords: [{ word: "CSV" }] });
    expect(fake.audioBytes).toBe(5 * 6400);
    expect(received[0]).toEqual({ type: "ready" });
    expect(received[1]).toEqual({ type: "partial", text: "导出订单" });
    expect(received.at(-1)).toEqual({ type: "final", text: "导出订单为 CSV" });

    // 每天 2 秒的额度已经用完（刚才 1 秒，再说 1 秒以上）。
    await dictate(token, {}, 10);
    const over = await dictate(token, {}, 1);
    expect(over).toEqual([{ type: "error", error: "voiceQuotaExceeded" }]);
  });

  it("organizes the final text with the project's names when asked", async () => {
    const plain = await dictate(await anonymousToken(), { hotwords: ["exportCsv"] }, 1);
    expect(plain.at(-1)).toEqual({ type: "final", text: "导出订单为 CSV" });
    expect(llmRequests).toHaveLength(0);

    const asked = await dictate(await anonymousToken(), { hotwords: ["exportCsv"], context: ["Which format?"], organize: true }, 1);
    expect(asked.at(-1)).toEqual({ type: "final", text: "导出订单为 CSV", organized: "导出订单为 CSV file" });
    expect(llmRequests).toHaveLength(1);
    expect(llmRequests[0]).toMatchObject({ url: "/v1/chat/completions", auth: "Bearer llm-key", request: { model: "test-model" } });
    expect(llmRequests[0].request.messages[1].content).toContain("exportCsv");
    expect(llmRequests[0].request.messages[1].content).toContain("Which format?");
  });
});
