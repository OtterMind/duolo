// 语音识别转发：电脑和手机把麦克风的声音发到这里，服务器转给豆包的流式语音识别，把识别出的文字发回去。
// 识别服务的密钥只放在服务器上，客户端只用自己的登录凭据（匿名的也行），按用户每天限额。
//
// 和客户端之间（`GET /v1/voice`，WebSocket）：
//   客户端 → {"type":"start","hotwords":[...],"context":[...],"organize":true}
//                                                              热词（文件名、函数名……）、对话上下文（新的在前）、要不要整理，都可省
//   客户端 → 二进制：16 kHz、16 位、单声道 PCM（小端），每块 100~200 ms
//   客户端 → {"type":"stop"}                                   说完了
//   服务器 → {"type":"ready"}                                  登录和额度没问题，可以识别（客户端可以先发 start 和声音，不用等它）
//   服务器 → {"type":"partial","text":"..."}                   识别中，整段到目前为止的文字
//   服务器 → {"type":"final","text":"...","organized":"..."}   最终结果（原话；要整理且整理出来了时带 organized），之后服务器关闭连接
//   服务器 → {"type":"error","error":"voiceUnavailable"}       错误码，客户端按自己的语言显示，并退回本地识别
//
// 和豆包之间的二进制帧格式见 https://www.volcengine.com/docs/6561/1354869（实现参考 ottermind 的 voice-ws-proxy）。

import { randomUUID } from "node:crypto";
import { gunzipSync, gzipSync } from "node:zlib";
import WebSocket, { type RawData } from "ws";
import type { Pool } from "pg";

import { organize, type OrganizeConfig } from "./organize.js";

/** 一秒钟的 PCM 有多少字节（16 kHz × 16 位 × 单声道）。 */
const BYTES_PER_SECOND = 16000 * 2;
/** 热词：豆包双向流式最多 100 个 token，这里按词数和长度先截一下。 */
const MAX_HOTWORDS = 40;
const MAX_HOTWORD_CHARS = 40;
/** 对话上下文：豆包限 800 token、20 轮，超出会自己截，这里先按字数截短，免得请求太大。 */
const MAX_CONTEXT_ITEMS = 20;
const MAX_CONTEXT_CHARS = 1500;
/** 一次最长录多久，超过就当说完了。 */
const MAX_SESSION_SECONDS = 5 * 60;
/** 说完后最多等多久的最终结果。 */
const FINAL_TIMEOUT_MS = 15_000;

export interface VoiceConfig {
  /** 豆包流式识别的地址（双向流式优化版）。 */
  url: string;
  /** 豆包语音的 API Key；为空时语音识别不可用，客户端退回本地识别。 */
  apiKey: string;
  resourceId: string;
  /** 每个用户每天最多识别多少秒；匿名用户单独一个额度。 */
  dailySeconds: number;
  anonymousDailySeconds: number;
  /** 把原话整理成指令用的模型，见 organize.ts。 */
  organize: OrganizeConfig;
}

/** 客户端看到的错误码（文案在客户端）。 */
export type VoiceError = "voiceUnavailable" | "voiceQuotaExceeded" | "voiceFailed" | "badRequest";

export interface VoiceStart {
  hotwords: string[];
  context: string[];
  /** 识别完后整理成指令。 */
  organize?: boolean;
}

/** 把客户端给的热词和上下文清理、截短，拼成豆包的 `corpus.context`（一个 JSON 字符串）。 */
export function corpusContext({ hotwords, context }: VoiceStart): string | undefined {
  const words = [...new Set(hotwords.map((w) => w.trim()).filter((w) => w && w.length <= MAX_HOTWORD_CHARS))].slice(
    0,
    MAX_HOTWORDS,
  );
  const dialog: { text: string }[] = [];
  let chars = 0;
  for (const item of context.slice(0, MAX_CONTEXT_ITEMS)) {
    const text = item.trim().slice(0, MAX_CONTEXT_CHARS - chars);
    if (!text) continue;
    dialog.push({ text });
    chars += text.length;
    if (chars >= MAX_CONTEXT_CHARS) break;
  }
  if (words.length === 0 && dialog.length === 0) return undefined;
  return JSON.stringify({
    ...(words.length ? { hotwords: words.map((word) => ({ word })) } : {}),
    ...(dialog.length ? { context_type: "dialog_ctx", context_data: dialog } : {}),
  });
}

// ---- 豆包的二进制帧 ----

const MESSAGE_FULL_CLIENT = 0x1;
const MESSAGE_AUDIO = 0x2;
const MESSAGE_ERROR = 0xf;
const SEQUENCE_POSITIVE = 0x1;
const LAST_WITHOUT_SEQUENCE = 0x2;
const LAST_WITH_SEQUENCE = 0x3;
const SERIALIZATION_JSON = 0x1;
const COMPRESSION_GZIP = 0x1;

/** 一帧：4 字节头、4 字节序号（负数表示最后一帧）、4 字节长度、内容（gzip）。 */
export function encodeFrame(kind: "start" | "audio", sequence: number, payload: Uint8Array): Buffer {
  const body = payload.byteLength > 0 ? gzipSync(payload) : Buffer.alloc(0);
  const head = Buffer.alloc(12);
  head[0] = (0x1 << 4) | 0x1;
  head[1] = ((kind === "start" ? MESSAGE_FULL_CLIENT : MESSAGE_AUDIO) << 4) | (sequence < 0 ? LAST_WITH_SEQUENCE : SEQUENCE_POSITIVE);
  head[2] = ((kind === "start" ? SERIALIZATION_JSON : 0) << 4) | (body.byteLength > 0 ? COMPRESSION_GZIP : 0);
  head.writeInt32BE(sequence, 4);
  head.writeUInt32BE(body.byteLength, 8);
  return Buffer.concat([head, body]);
}

export interface ServerFrame {
  /** 最后一帧（之后不会再有结果）。 */
  last: boolean;
  /** 整段到目前为止的文字；出错时没有。 */
  text?: string;
  /** 出错时的错误码（豆包的）。 */
  errorCode?: number;
}

export function decodeFrame(data: Buffer): ServerFrame | null {
  if (data.byteLength < 8) return null;
  const headerSize = (data[0] & 0x0f) * 4;
  const type = data[1] >> 4;
  const flags = data[1] & 0x0f;
  if (type === MESSAGE_ERROR) {
    return { last: true, errorCode: data.byteLength >= headerSize + 4 ? data.readUInt32BE(headerSize) : 0 };
  }
  let offset = headerSize;
  if (flags === SEQUENCE_POSITIVE || flags === LAST_WITH_SEQUENCE) offset += 4;
  if (data.byteLength < offset + 4) return null;
  const size = data.readUInt32BE(offset);
  offset += 4;
  let payload = data.subarray(offset, offset + size);
  if ((data[2] & 0x0f) === COMPRESSION_GZIP && payload.byteLength > 0) payload = gunzipSync(payload);
  const last = flags === LAST_WITHOUT_SEQUENCE || flags === LAST_WITH_SEQUENCE;
  if (payload.byteLength === 0) return { last };
  const event = JSON.parse(payload.toString("utf8")) as {
    result?: { text?: string; utterances?: { text?: string }[] };
    header?: { status_code?: number };
  };
  const status = event.header?.status_code;
  if (typeof status === "number" && status >= 400) return { last: true, errorCode: status };
  const result = event.result;
  const text = result?.text?.trim() || result?.utterances?.map((u) => u.text?.trim() ?? "").join("") || "";
  return { last, text };
}

// ---- 额度 ----

export const VOICE_SCHEMA = `
-- 语音识别用量：每个用户每天识别了多少秒。
create table if not exists voice_usage (
  user_id text not null,
  day date not null,
  seconds integer not null default 0,
  primary key (user_id, day)
);
`;

async function usedToday(pool: Pool, userId: string) {
  const { rows } = await pool.query("select seconds from voice_usage where user_id = $1 and day = current_date", [userId]);
  return (rows[0]?.seconds as number | undefined) ?? 0;
}

async function addUsage(pool: Pool, userId: string, seconds: number) {
  if (seconds <= 0) return;
  await pool.query(
    `insert into voice_usage (user_id, day, seconds) values ($1, current_date, $2)
     on conflict (user_id, day) do update set seconds = voice_usage.seconds + excluded.seconds`,
    [userId, seconds],
  );
}

// ---- 一次识别 ----

export interface VoiceUser {
  id: string;
  anonymous: boolean;
}

/** 连接一建立就开始收消息：校验登录、查额度要等数据库，这期间客户端发来的 start 和声音不能丢。 */
export function bufferMessages(client: WebSocket) {
  const early: [RawData, boolean][] = [];
  const collect = (data: RawData, binary: boolean) => early.push([data, binary]);
  client.on("message", collect);
  return () => {
    client.off("message", collect);
    return early;
  };
}

/** 处理一个客户端连接：等 start，连豆包，转发声音，把结果发回去。`early` 是挂上处理函数之前收到的消息。 */
export async function serveVoice(client: WebSocket, user: VoiceUser, config: VoiceConfig, pool: Pool, early: [RawData, boolean][]) {
  const send = (message: object) => client.readyState === WebSocket.OPEN && client.send(JSON.stringify(message));
  const fail = (error: VoiceError) => {
    send({ type: "error", error });
    client.close(1000);
  };
  if (!config.apiKey) return fail("voiceUnavailable");
  const limit = user.anonymous ? config.anonymousDailySeconds : config.dailySeconds;
  if ((await usedToday(pool, user.id)) >= limit) return fail("voiceQuotaExceeded");
  send({ type: "ready" });

  let upstream: WebSocket | null = null;
  let sequence = 1;
  let audioBytes = 0;
  let text = "";
  let stopped = false;
  let done = false;
  let finalTimer: NodeJS.Timeout | undefined;
  let started: VoiceStart | undefined;
  const pending: Buffer[] = [];

  const finish = (error?: VoiceError) => {
    if (done) return;
    done = true;
    clearTimeout(finalTimer);
    void addUsage(pool, user.id, Math.ceil(audioBytes / BYTES_PER_SECOND)).catch(() => {});
    upstream?.close();
    if (error && !text) {
      send({ type: "error", error });
      client.close(1000);
      return;
    }
    void (async () => {
      const organized = started?.organize
        ? await organize(config.organize, { text, hotwords: started.hotwords, context: started.context })
        : undefined;
      send({ type: "final", text, ...(organized && organized !== text ? { organized } : {}) });
      client.close(1000);
    })();
  };
  const stop = () => {
    if (stopped) return;
    stopped = true;
    finalTimer = setTimeout(() => finish(), FINAL_TIMEOUT_MS);
    if (upstream?.readyState === WebSocket.OPEN) upstream.send(encodeFrame("audio", -(sequence + 1), new Uint8Array()));
  };
  const sendAudio = (chunk: Buffer) => {
    if (stopped || done) return;
    audioBytes += chunk.byteLength;
    if (upstream?.readyState !== WebSocket.OPEN) {
      pending.push(chunk);
      return;
    }
    sequence += 1;
    upstream.send(encodeFrame("audio", sequence, chunk));
    if (audioBytes >= MAX_SESSION_SECONDS * BYTES_PER_SECOND) stop();
  };

  const open = (start: VoiceStart) => {
    started = start;
    upstream = new WebSocket(config.url, {
      headers: { "X-Api-Key": config.apiKey, "X-Api-Resource-Id": config.resourceId, "X-Api-Connect-Id": randomUUID() },
    });
    upstream.on("open", () => {
      const context = corpusContext(start);
      const request = {
        user: { uid: user.id },
        audio: { format: "pcm", codec: "raw", rate: 16000, bits: 16, channel: 1 },
        request: {
          model_name: "bigmodel",
          // 二遍识别：边说边出字，一句说完再精识别一遍。
          enable_nonstream: true,
          enable_itn: true,
          enable_punc: true,
          ...(context ? { corpus: { context } } : {}),
        },
      };
      upstream!.send(encodeFrame("start", 1, Buffer.from(JSON.stringify(request))));
      for (const chunk of pending.splice(0)) {
        sequence += 1;
        upstream!.send(encodeFrame("audio", sequence, chunk));
      }
      if (stopped) upstream!.send(encodeFrame("audio", -(sequence + 1), new Uint8Array()));
    });
    upstream.on("message", (data: RawData) => {
      let frame: ServerFrame | null;
      try {
        frame = decodeFrame(Buffer.isBuffer(data) ? data : Buffer.from(data as ArrayBuffer));
      } catch {
        frame = null;
      }
      if (!frame) return;
      if (frame.errorCode !== undefined) {
        console.error(`[voice] upstream error ${frame.errorCode}`);
        return finish("voiceFailed");
      }
      if (frame.text !== undefined && frame.text !== text) {
        text = frame.text;
        if (!frame.last) send({ type: "partial", text });
      }
      if (frame.last) finish();
    });
    upstream.on("unexpected-response", (_req, res) => {
      console.error(`[voice] upstream rejected: ${res.statusCode} ${res.headers["x-tt-logid"] ?? ""}`);
      finish("voiceFailed");
    });
    upstream.on("error", (err) => {
      console.error("[voice] upstream", err.message);
      finish("voiceFailed");
    });
    upstream.on("close", () => finish(stopped ? undefined : "voiceFailed"));
  };

  const onMessage = (data: RawData, binary: boolean) => {
    if (done) return;
    if (binary) return sendAudio(Buffer.isBuffer(data) ? data : Buffer.from(data as ArrayBuffer));
    let message: { type?: string; hotwords?: unknown; context?: unknown; organize?: unknown };
    try {
      message = JSON.parse(data.toString());
    } catch {
      return fail("badRequest");
    }
    if (message.type === "start" && !upstream) {
      const strings = (v: unknown) => (Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : []);
      open({ hotwords: strings(message.hotwords), context: strings(message.context), organize: message.organize === true });
    } else if (message.type === "stop") {
      stop();
    }
  };
  client.on("message", onMessage);
  for (const [data, binary] of early) onMessage(data, binary);
  // 客户端断开：不再等结果，只记用量。
  client.on("close", () => {
    if (done) return;
    done = true;
    clearTimeout(finalTimer);
    void addUsage(pool, user.id, Math.ceil(audioBytes / BYTES_PER_SECOND)).catch(() => {});
    upstream?.close();
  });
}
