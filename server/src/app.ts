// HTTP 接口。返回给客户端的错误是错误码（`{ error: "invalidPairingCode" }`），客户端按自己的语言显示。
//
//   /v1/auth/*                  better-auth：邮箱验证码登录等（见 auth.ts）
//   POST   /v1/devices          登记当前设备（名字、平台、加密公钥）
//   GET    /v1/devices          我的设备和是否在线
//   DELETE /v1/devices/:id      解除一台设备（它的登录会话一起失效）
//   PUT    /v1/devices/me/push  上报推送令牌
//   POST   /v1/pairings         电脑端生成配对码（放进二维码）
//   POST   /v1/pairings/claim   手机用配对码登录同一个账号，并拿到电脑端的公钥
//   GET    /v1/connect          WebSocket 转发（见 relay.ts）
//   GET    /v1/voice            WebSocket 语音识别（见 voice.ts），匿名登录的也能用

import Fastify, { type FastifyReply, type FastifyRequest } from "fastify";
import websocket from "@fastify/websocket";
import { fromNodeHeaders } from "better-auth/node";
import { z } from "zod";

import type { Auth } from "./auth.js";
import type { Config } from "./config.js";
import { Hub } from "./relay.js";
import { DESKTOP_PLATFORMS, Store, type Device } from "./store.js";
import { bufferMessages, serveVoice } from "./voice.js";
import type { Pool } from "pg";

/** 配对码有效期。 */
const PAIRING_TTL_MS = 10 * 60 * 1000;

const DeviceInput = z.object({
  name: z.string().trim().min(1).max(80),
  platform: z.enum(["mac", "windows", "linux", "ios", "android"]),
  publicKey: z.string().min(16).max(200),
});

export interface AppDeps {
  config: Config;
  pool: Pool;
  auth: Auth;
}

export async function buildApp({ config, pool, auth }: AppDeps) {
  const app = Fastify({ logger: process.env.NODE_ENV !== "test", bodyLimit: 1024 * 1024 });
  const store = new Store(pool);
  const hub = new Hub(store, config.maxMessageBytes);
  await app.register(websocket, { options: { maxPayload: config.maxMessageBytes + 4096 } });

  // 没有请求体的 POST（例如生成配对码）也可能带着 JSON 的 content-type：当作没有内容，不报错。
  app.removeContentTypeParser("application/json");
  app.addContentTypeParser("application/json", { parseAs: "string" }, (_request, body, done) => {
    if (body === "") return done(null, undefined);
    try {
      done(null, JSON.parse(body as string));
    } catch (err) {
      done(Object.assign(err as Error, { statusCode: 400 }), undefined);
    }
  });

  // ---- better-auth ----
  app.route({
    method: ["GET", "POST"],
    url: "/v1/auth/*",
    async handler(request, reply) {
      const url = new URL(request.url, config.publicUrl);
      const body = request.body === undefined ? undefined : JSON.stringify(request.body);
      const response = await auth.handler(
        new Request(url, { method: request.method, headers: fromNodeHeaders(request.headers), body }),
      );
      reply.status(response.status);
      response.headers.forEach((value, key) => reply.header(key, value));
      return reply.send(response.body ? await response.text() : null);
    },
  });

  /** 当前请求的登录会话；没有登录时为 null。 */
  async function sessionOf(headers: FastifyRequest["headers"]) {
    return auth.api.getSession({ headers: fromNodeHeaders(headers) });
  }

  /** 要求已登录并且登记过设备。 */
  async function requireDevice(request: FastifyRequest, reply: FastifyReply): Promise<Device | null> {
    const session = await sessionOf(request.headers);
    if (!session) {
      reply.status(401).send({ error: "unauthorized" });
      return null;
    }
    const device = await store.deviceBySession(session.session.id);
    if (!device) {
      reply.status(409).send({ error: "deviceNotRegistered" });
      return null;
    }
    return device;
  }

  app.get("/health", async () => ({ ok: true }));

  // ---- 设备 ----

  app.post("/v1/devices", async (request, reply) => {
    const session = await sessionOf(request.headers);
    if (!session) return reply.status(401).send({ error: "unauthorized" });
    const input = DeviceInput.safeParse(request.body);
    if (!input.success) return reply.status(400).send({ error: "badRequest" });
    const device = await store.upsertDevice({ userId: session.user.id, sessionId: session.session.id, ...input.data });
    return { device: hub.peerView(device), user: { id: session.user.id, email: session.user.email } };
  });

  app.get("/v1/devices", async (request, reply) => {
    const me = await requireDevice(request, reply);
    if (!me) return;
    const devices = await store.devicesOf(me.userId);
    return { me: me.id, devices: devices.map((d) => ({ ...hub.peerView(d), publicKey: d.publicKey })) };
  });

  app.delete<{ Params: { id: string } }>("/v1/devices/:id", async (request, reply) => {
    const me = await requireDevice(request, reply);
    if (!me) return;
    const target = await store.device(request.params.id);
    if (!target || target.userId !== me.userId) return reply.status(404).send({ error: "notFound" });
    const ctx = await auth.$context;
    await ctx.internalAdapter.deleteSession(await sessionToken(pool, target.sessionId));
    await store.deleteDevice(target.id);
    hub.disconnect(target.id);
    return { ok: true };
  });

  app.put("/v1/devices/me/push", async (request, reply) => {
    const me = await requireDevice(request, reply);
    if (!me) return;
    const input = z.object({ kind: z.enum(["apns", "fcm"]), token: z.string().min(1).max(4096) }).safeParse(request.body);
    if (!input.success) return reply.status(400).send({ error: "badRequest" });
    await store.setPushToken(me.id, input.data.kind, input.data.token);
    return { ok: true };
  });

  // ---- 配对 ----

  app.post("/v1/pairings", async (request, reply) => {
    const me = await requireDevice(request, reply);
    if (!me) return;
    if (!DESKTOP_PLATFORMS.includes(me.platform)) return reply.status(403).send({ error: "desktopOnly" });
    const { code, expiresAt } = await store.createPairing(me.userId, me.id, PAIRING_TTL_MS);
    return { code, expiresAt: expiresAt.toISOString(), server: config.publicUrl };
  });

  app.post("/v1/pairings/claim", async (request, reply) => {
    const input = z.object({ code: z.string().min(4).max(32), device: DeviceInput }).safeParse(request.body);
    if (!input.success) return reply.status(400).send({ error: "badRequest" });
    if (DESKTOP_PLATFORMS.includes(input.data.device.platform)) return reply.status(400).send({ error: "badRequest" });
    const pairing = await store.consumePairing(input.data.code);
    if (!pairing) return reply.status(404).send({ error: "invalidPairingCode" });
    const desktop = await store.device(pairing.desktopDeviceId);
    if (!desktop) return reply.status(404).send({ error: "invalidPairingCode" });
    // 手机不用单独登录：直接给同一个用户建一个会话。
    const ctx = await auth.$context;
    const session = await ctx.internalAdapter.createSession(pairing.userId, false);
    if (!session) return reply.status(500).send({ error: "internal" });
    const device = await store.upsertDevice({ userId: pairing.userId, sessionId: session.id, ...input.data.device });
    return {
      token: session.token,
      device: hub.peerView(device),
      desktop: { ...hub.peerView(desktop), publicKey: desktop.publicKey },
    };
  });

  // ---- 转发 ----

  app.get("/v1/connect", { websocket: true }, async (socket, request) => {
    // 原生客户端用 Authorization 头；不方便设头的环境可以用 ?token=。
    const token = (request.query as { token?: string }).token;
    const headers = token ? { ...request.headers, authorization: `Bearer ${token}` } : request.headers;
    const session = await sessionOf(headers);
    const device = session ? await store.deviceBySession(session.session.id) : null;
    if (!device) return socket.close(4401, session ? "deviceNotRegistered" : "unauthorized");
    await hub.connect(socket, device);
  });

  // ---- 语音识别 ----

  app.get("/v1/voice", { websocket: true }, async (socket, request) => {
    const take = bufferMessages(socket);
    const token = (request.query as { token?: string }).token;
    const headers = token ? { ...request.headers, authorization: `Bearer ${token}` } : request.headers;
    const session = await sessionOf(headers);
    if (!session) return socket.close(4401, "unauthorized");
    const user = session.user as { id: string; isAnonymous?: boolean | null };
    await serveVoice(socket, { id: user.id, anonymous: user.isAnonymous === true }, config.voice, pool, take());
  });

  // 定时清理过期的离线消息和配对码。
  const purge = setInterval(() => void store.purge(config.mailboxTtlMs).catch(() => {}), 60 * 60 * 1000);
  app.addHook("onClose", async () => {
    clearInterval(purge);
    hub.close();
  });

  return app;
}

/** 会话编号对应的令牌（better-auth 按令牌删除会话）。 */
async function sessionToken(pool: Pool, sessionId: string) {
  const { rows } = await pool.query('select token from "session" where id = $1', [sessionId]);
  return (rows[0]?.token as string | undefined) ?? "";
}
