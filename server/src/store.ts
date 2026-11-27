// 设备、配对码、离线信箱的读写。全部是普通 SQL，没有 ORM。

import { createHash, randomBytes, randomUUID } from "node:crypto";
import type { Pool } from "pg";

export type Platform = "mac" | "windows" | "linux" | "ios" | "android";

/** 电脑端（运行 Agent、显示配对码的一端）。 */
export const DESKTOP_PLATFORMS: readonly Platform[] = ["mac", "windows", "linux"];

export interface Device {
  id: string;
  userId: string;
  sessionId: string;
  name: string;
  platform: Platform;
  publicKey: string;
  lastSeen: Date;
}

export interface Envelope {
  id: string;
  from: string;
  to: string;
  body: string;
  createdAt: Date;
}

const sha256 = (s: string) => createHash("sha256").update(s).digest("hex");

/** 配对码用的字母表：去掉了容易看错的 0/O、1/I/L。 */
const CODE_ALPHABET = "23456789ABCDEFGHJKMNPQRSTUVWXYZ";

type Row = Record<string, unknown>;
const toDevice = (r: Row): Device => ({
  id: r.id as string,
  userId: r.user_id as string,
  sessionId: r.session_id as string,
  name: r.name as string,
  platform: r.platform as Platform,
  publicKey: r.public_key as string,
  lastSeen: r.last_seen as Date,
});

export class Store {
  constructor(private readonly pool: Pool) {}

  // ---- 设备 ----

  /** 登记当前会话对应的设备；这个会话已经登记过时更新名字和公钥。 */
  async upsertDevice(input: { userId: string; sessionId: string; name: string; platform: Platform; publicKey: string }) {
    const { rows } = await this.pool.query(
      `insert into device (id, user_id, session_id, name, platform, public_key)
       values ($1, $2, $3, $4, $5, $6)
       on conflict (session_id) do update set name = excluded.name, platform = excluded.platform,
         public_key = excluded.public_key, last_seen = now()
       returning *`,
      [randomUUID(), input.userId, input.sessionId, input.name, input.platform, input.publicKey],
    );
    return toDevice(rows[0]);
  }

  async deviceBySession(sessionId: string) {
    const { rows } = await this.pool.query("select * from device where session_id = $1", [sessionId]);
    return rows[0] ? toDevice(rows[0]) : null;
  }

  async devicesOf(userId: string) {
    const { rows } = await this.pool.query("select * from device where user_id = $1 order by created_at", [userId]);
    return rows.map(toDevice);
  }

  async device(id: string) {
    const { rows } = await this.pool.query("select * from device where id = $1", [id]);
    return rows[0] ? toDevice(rows[0]) : null;
  }

  async deleteDevice(id: string) {
    await this.pool.query("delete from device where id = $1", [id]);
  }

  async touch(id: string) {
    await this.pool.query("update device set last_seen = now() where id = $1", [id]);
  }

  async setPushToken(id: string, kind: "apns" | "fcm", token: string) {
    await this.pool.query("update device set push_kind = $2, push_token = $3 where id = $1", [id, kind, token]);
  }

  // ---- 配对码 ----

  /** 给电脑端生成一次性配对码（8 位，`ttlMs` 内有效）。数据库里只存哈希。 */
  async createPairing(userId: string, desktopDeviceId: string, ttlMs: number) {
    const bytes = randomBytes(8);
    const code = Array.from(bytes, (b) => CODE_ALPHABET[b % CODE_ALPHABET.length]).join("");
    const expiresAt = new Date(Date.now() + ttlMs);
    await this.pool.query("delete from pairing where desktop_device_id = $1", [desktopDeviceId]);
    await this.pool.query("insert into pairing (code_hash, user_id, desktop_device_id, expires_at) values ($1, $2, $3, $4)", [
      sha256(code),
      userId,
      desktopDeviceId,
      expiresAt,
    ]);
    return { code, expiresAt };
  }

  /** 用掉配对码：有效时返回它属于的用户和电脑端，并删除（只能用一次）。 */
  async consumePairing(code: string) {
    const { rows } = await this.pool.query(
      "delete from pairing where code_hash = $1 and expires_at > now() returning user_id, desktop_device_id",
      [sha256(code.trim().toUpperCase())],
    );
    return rows[0] ? { userId: rows[0].user_id as string, desktopDeviceId: rows[0].desktop_device_id as string } : null;
  }

  // ---- 离线信箱 ----

  /** 存一条消息；同一个编号重复发送时只存一次。 */
  async putMessage(e: Omit<Envelope, "createdAt">) {
    const { rows } = await this.pool.query(
      `insert into mailbox (to_device, id, from_device, body) values ($1, $2, $3, $4)
       on conflict (to_device, id) do nothing returning created_at`,
      [e.to, e.id, e.from, e.body],
    );
    return rows[0] ? (rows[0].created_at as Date) : null;
  }

  /** 发给 `device` 还没确认的消息，按时间先后。 */
  async pendingFor(device: string, limit = 500): Promise<Envelope[]> {
    const { rows } = await this.pool.query(
      "select * from mailbox where to_device = $1 order by created_at, id limit $2",
      [device, limit],
    );
    return rows.map((r) => ({
      id: r.id,
      from: r.from_device,
      to: r.to_device,
      body: r.body,
      createdAt: r.created_at,
    }));
  }

  async ack(device: string, ids: string[]) {
    if (ids.length === 0) return;
    await this.pool.query("delete from mailbox where to_device = $1 and id = any($2)", [device, ids]);
  }

  /** 清理过期的消息和配对码。 */
  async purge(mailboxTtlMs: number) {
    await this.pool.query("delete from mailbox where created_at < $1", [new Date(Date.now() - mailboxTtlMs)]);
    await this.pool.query("delete from pairing where expires_at < now()");
    // 语音用量只用来算每天的额度，留 90 天（隐私政策里写明的期限）。
    await this.pool.query("delete from voice_usage where day < current_date - 90");
  }
}
