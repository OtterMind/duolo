# Duolo 服务端

[English](README.md) · 简体中文

所有用户共用的一套服务：账号登录、设备配对，以及电脑端（Mac / Windows / Linux）和手机之间的消息转发。
代码和执行都留在用户自己的电脑上。服务端只转发消息内容，不解析；内容由客户端做端到端加密，服务端看不到代码和对话。

- Node 22 + TypeScript，Fastify（HTTP）+ `ws`（WebSocket）
- PostgreSQL
- 登录用开源的 [better-auth](https://www.better-auth.com)：邮箱验证码登录，原生客户端用 `Authorization: Bearer <令牌>`

## 现状

还在开发，不能用于生产：

- 手机应用已经接入，带端到端加密（见 [docs/mobile.md](../docs/mobile.md)）。桌面应用还没有接入；在那之前，`true mobile-sandbox` 启动服务端和一台模拟电脑。
- 没有接邮件服务，任何环境下验证码都只打印在日志里。要发真实邮件，实现 `src/mailer.ts` 里的 `Mailer` 接口，并在 `src/main.ts` 里换上。
- 会保存推送令牌，但还不会发推送。

## 本地运行

```sh
docker compose up -d      # PostgreSQL，端口 54329
pnpm install
pnpm dev                  # http://localhost:8787，启动时自动建表
pnpm test                 # 端到端测试（每次建一个临时数据库，跑完删掉）
```

## 环境变量

| 变量 | 说明 | 默认 |
|---|---|---|
| `DATABASE_URL` | PostgreSQL 连接串 | `postgres://duolo:duolo@localhost:54329/duolo` |
| `BETTER_AUTH_SECRET` | 会话签名密钥，**生产环境（`NODE_ENV=production`）必须设置** | 开发用的固定值 |
| `PUBLIC_URL` | 对外地址 | `http://localhost:8787` |
| `PORT` / `HOST` | 监听地址 | `8787` / `0.0.0.0` |
| `MAILBOX_TTL_DAYS` | 离线消息最多存几天 | `7` |
| `MAX_MESSAGE_BYTES` | 单条消息上限 | `262144` |
| `DOUBAO_ASR_API_KEY` | 语音输入用的豆包语音 API Key。为空时不在服务器上识别，客户端用系统自带的识别 | 空 |
| `DOUBAO_ASR_RESOURCE_ID` / `DOUBAO_ASR_URL` | 豆包流式语音识别的资源和地址 | `volc.seedasr.sauc.duration` / `wss://openspeech.bytedance.com/api/v3/sauc/bigmodel_async` |
| `VOICE_DAILY_MINUTES` / `VOICE_ANONYMOUS_DAILY_MINUTES` | 每个用户每天能用多少分钟语音输入（登录的 / 匿名的） | `120` / `30` |

## 接口

| 接口 | 说明 |
|---|---|
| `POST /v1/auth/email-otp/send-verification-otp` | 发验证码：`{ email, type: "sign-in" }`，邮件按 `Accept-Language` 选语言 |
| `POST /v1/auth/sign-in/email-otp` | 用验证码登录：`{ email, otp }` → `{ token, user }`，第一次登录自动注册 |
| `POST /v1/auth/sign-in/anonymous` | 不注册也能拿到一个匿名会话 → `{ token, user }`。电脑端用它做语音输入；以后用邮箱登录时合并成正式账号 |
| `POST /v1/devices` | 登记当前设备：`{ name, platform: "mac" \| "windows" \| "linux" \| "ios" \| "android", publicKey }` |
| `GET /v1/devices` | 我的设备、公钥、是否在线 |
| `DELETE /v1/devices/:id` | 解除一台设备：它的登录会话失效，连接断开 |
| `PUT /v1/devices/me/push` | 上报推送令牌：`{ kind: "apns" \| "fcm", token }` |
| `POST /v1/pairings` | 电脑端生成一次性配对码（10 分钟有效），放进二维码；手机调用返回 `desktopOnly` |
| `POST /v1/pairings/claim` | 手机用配对码登录同一个账号：`{ code, device }` → `{ token, device, desktop }`（含电脑端的公钥） |
| `GET /v1/connect` | WebSocket 转发，协议见 `src/relay.ts` |
| `GET /v1/voice` | WebSocket 语音识别，匿名会话也能用。声音连同任务的热词和最近的对话一起交给豆包识别，协议见 `src/voice.ts` |

错误统一返回错误码，例如 `{ "error": "invalidPairingCode" }`，客户端按自己的语言显示。

## 转发怎么保证送达

每条消息先存进离线信箱（`mailbox` 表），对方在线就立刻推，收到对方的 `ack` 才删除；
对方上线时补发没确认的；同一个消息编号重复发送只存一次。超过 7 天（`MAILBOX_TTL_DAYS`）的消息自动清理。

现在是单台服务器。以后多台时，用 PostgreSQL 的 `LISTEN/NOTIFY` 把消息转到对方连着的那台。
