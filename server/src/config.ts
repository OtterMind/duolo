// 服务端配置，全部来自环境变量。开发时的默认值能直接配合 docker-compose.yml 里的数据库跑起来。

import type { VoiceConfig } from "./voice.js";

export interface Config {
  port: number;
  host: string;
  /** PostgreSQL 连接串。 */
  databaseUrl: string;
  /** 对外的地址（登录库生成链接、二维码里都用它）。 */
  publicUrl: string;
  /** 登录会话签名用的密钥，生产环境必须设置。 */
  authSecret: string;
  /** 离线信箱里的消息最多存多久。 */
  mailboxTtlMs: number;
  /** 一条消息最大多少字节（加密后的内容）。 */
  maxMessageBytes: number;
  /** 语音识别（豆包），见 voice.ts。 */
  voice: VoiceConfig;
}

export function loadConfig(env: NodeJS.ProcessEnv = process.env): Config {
  const production = env.NODE_ENV === "production";
  const authSecret = env.BETTER_AUTH_SECRET ?? (production ? "" : "dev-secret-change-me-dev-secret-change-me");
  if (!authSecret) throw new Error("BETTER_AUTH_SECRET must be set in production");
  const port = Number(env.PORT ?? 8787);
  return {
    port,
    host: env.HOST ?? "0.0.0.0",
    databaseUrl: env.DATABASE_URL ?? "postgres://duolo:duolo@localhost:54329/duolo",
    publicUrl: env.PUBLIC_URL ?? `http://localhost:${port}`,
    authSecret,
    mailboxTtlMs: Number(env.MAILBOX_TTL_DAYS ?? 7) * 24 * 60 * 60 * 1000,
    maxMessageBytes: Number(env.MAX_MESSAGE_BYTES ?? 256 * 1024),
    voice: {
      url: env.DOUBAO_ASR_URL ?? "wss://openspeech.bytedance.com/api/v3/sauc/bigmodel_async",
      apiKey: env.DOUBAO_ASR_API_KEY ?? "",
      resourceId: env.DOUBAO_ASR_RESOURCE_ID ?? "volc.seedasr.sauc.duration",
      dailySeconds: Number(env.VOICE_DAILY_MINUTES ?? 120) * 60,
      anonymousDailySeconds: Number(env.VOICE_ANONYMOUS_DAILY_MINUTES ?? 30) * 60,
      organize: {
        baseUrl: env.VOICE_ORGANIZE_BASE_URL ?? "",
        apiKey: env.VOICE_ORGANIZE_API_KEY ?? "",
        model: env.VOICE_ORGANIZE_MODEL ?? "",
        timeoutMs: Number(env.VOICE_ORGANIZE_TIMEOUT_MS ?? 4000),
      },
    },
  };
}
