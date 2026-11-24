// 数据库：PostgreSQL。
//
// - better-auth 的表（user、session、verification…）用它自带的迁移建；
// - 我们自己的表：设备、配对码、离线信箱，在下面的 SCHEMA 里，可以重复执行；语音用量在 voice.ts。

import { getMigrations } from "better-auth/db/migration";
import type { BetterAuthOptions } from "better-auth";
import pg from "pg";

import { VOICE_SCHEMA } from "./voice.js";

export function createPool(databaseUrl: string) {
  return new pg.Pool({ connectionString: databaseUrl, max: 10 });
}

const SCHEMA = `
-- 一台设备（Mac、Windows、Linux 电脑，iPhone、Android 手机），绑定一个登录会话。
create table if not exists device (
  id text primary key,
  user_id text not null,
  session_id text not null unique,
  name text not null,
  platform text not null check (platform in ('mac', 'windows', 'linux', 'ios', 'android')),
  -- 端到端加密的公钥（X25519，base64）。服务器只保存公钥，解不开内容。
  public_key text not null,
  push_kind text check (push_kind in ('apns', 'fcm')),
  push_token text,
  created_at timestamptz not null default now(),
  last_seen timestamptz not null default now()
);
create index if not exists device_user on device (user_id);

-- 手机扫码配对用的一次性配对码（只存哈希）。
create table if not exists pairing (
  code_hash text primary key,
  user_id text not null,
  desktop_device_id text not null references device (id) on delete cascade,
  expires_at timestamptz not null
);

-- 离线信箱：每条消息先存这里，收件设备确认收到后删除。
create table if not exists mailbox (
  to_device text not null references device (id) on delete cascade,
  id text not null,
  from_device text not null,
  body text not null,
  created_at timestamptz not null default now(),
  primary key (to_device, id)
);
create index if not exists mailbox_created on mailbox (created_at);
`;

/** 建表：better-auth 的表和我们自己的表。 */
export async function migrate(pool: pg.Pool, authOptions: BetterAuthOptions) {
  const { runMigrations } = await getMigrations(authOptions);
  await runMigrations();
  await pool.query(SCHEMA);
  await pool.query(VOICE_SCHEMA);
}
