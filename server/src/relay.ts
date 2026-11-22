// 转发：每台设备连一条 WebSocket，服务器只在同一个用户的设备之间转发消息。
//
// 协议（JSON，一条 WebSocket 消息一个对象）：
//
// 设备 → 服务器
//   { type: "send", to, id, body }   发给自己账号下的另一台设备。body 是加密后的内容（base64），服务器不解析。
//   { type: "ack", ids: [...] }      确认收到这些消息，服务器从离线信箱删除。
//   { type: "ping" }
// 服务器 → 设备
//   { type: "hello", device, peers }  连上后先发：自己是谁，账号下其他设备和是否在线。
//   { type: "message", id, from, body, at }
//   { type: "sent", id }              服务器已经收下（对方可能还离线，上线后补发）。
//   { type: "presence", device, online }
//   { type: "pong" } / { type: "error", code, id? }
//
// 可靠送达：每条消息都先存进离线信箱，对方在线就立刻推，收到 ack 才删；对方上线时补发没确认的。
// 现在是单台服务器；以后多台时，用 PostgreSQL 的 LISTEN/NOTIFY 把消息转到对方连着的那台。

import type { WebSocket } from "ws";
import { z } from "zod";

import type { Device, Store } from "./store.js";

const Incoming = z.discriminatedUnion("type", [
  z.object({ type: z.literal("send"), to: z.string().min(1), id: z.string().min(1).max(100), body: z.string() }),
  z.object({ type: z.literal("ack"), ids: z.array(z.string()).max(1000) }),
  z.object({ type: z.literal("ping") }),
]);

export interface PeerView {
  id: string;
  name: string;
  platform: Device["platform"];
  online: boolean;
  lastSeen: string;
}

export class Hub {
  /** 在线设备的连接，每台设备一条（新连接顶掉旧的）。 */
  private readonly sockets = new Map<string, { socket: WebSocket; device: Device }>();

  constructor(
    private readonly store: Store,
    private readonly maxMessageBytes: number,
  ) {}

  isOnline(deviceId: string) {
    return this.sockets.has(deviceId);
  }

  peerView(d: Device): PeerView {
    return { id: d.id, name: d.name, platform: d.platform, online: this.isOnline(d.id), lastSeen: d.lastSeen.toISOString() };
  }

  /** 设备连上来了（身份已经验证过）。 */
  async connect(socket: WebSocket, device: Device) {
    this.sockets.get(device.id)?.socket.close(4000, "replaced");
    this.sockets.set(device.id, { socket, device });
    await this.store.touch(device.id);

    socket.on("message", (data) => void this.onMessage(device, data.toString()).catch((err) => this.fail(socket, err)));
    socket.on("close", () => {
      if (this.sockets.get(device.id)?.socket !== socket) return;
      this.sockets.delete(device.id);
      void this.store.touch(device.id);
      void this.broadcastPresence(device, false);
    });

    const peers = (await this.store.devicesOf(device.userId)).filter((d) => d.id !== device.id);
    send(socket, { type: "hello", device: this.peerView(device), peers: peers.map((p) => this.peerView(p)) });
    await this.broadcastPresence(device, true);
    // 补发离线期间没确认的消息。
    for (const e of await this.store.pendingFor(device.id)) {
      send(socket, { type: "message", id: e.id, from: e.from, body: e.body, at: e.createdAt.toISOString() });
    }
  }

  /** 设备被解除（或退出登录）：断开它的连接。 */
  disconnect(deviceId: string) {
    this.sockets.get(deviceId)?.socket.close(4401, "unlinked");
  }

  private async onMessage(device: Device, raw: string) {
    const socket = this.sockets.get(device.id)?.socket;
    if (!socket) return;
    let parsed: z.infer<typeof Incoming>;
    try {
      parsed = Incoming.parse(JSON.parse(raw));
    } catch {
      return send(socket, { type: "error", code: "badRequest" });
    }
    switch (parsed.type) {
      case "ping":
        return send(socket, { type: "pong" });
      case "ack":
        return this.store.ack(device.id, parsed.ids);
      case "send": {
        const { to, id, body } = parsed;
        if (Buffer.byteLength(body) > this.maxMessageBytes) return send(socket, { type: "error", code: "tooLarge", id });
        const target = await this.store.device(to);
        if (!target || target.userId !== device.userId) return send(socket, { type: "error", code: "unknownDevice", id });
        const at = await this.store.putMessage({ id, from: device.id, to, body });
        send(socket, { type: "sent", id });
        // 重复发送的同一条消息不再推第二次。
        const online = this.sockets.get(to);
        if (at && online) send(online.socket, { type: "message", id, from: device.id, body, at: at.toISOString() });
        return;
      }
    }
  }

  private async broadcastPresence(device: Device, online: boolean) {
    for (const peer of await this.store.devicesOf(device.userId)) {
      const conn = this.sockets.get(peer.id);
      if (conn && peer.id !== device.id) send(conn.socket, { type: "presence", device: device.id, online });
    }
  }

  private fail(socket: WebSocket, err: unknown) {
    console.error("[relay]", err);
    send(socket, { type: "error", code: "internal" });
  }

  close() {
    for (const { socket } of this.sockets.values()) socket.close(1001, "server shutting down");
    this.sockets.clear();
  }
}

function send(socket: WebSocket, message: object) {
  if (socket.readyState === socket.OPEN) socket.send(JSON.stringify(message));
}
