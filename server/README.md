# Duolo server

English · [简体中文](README.zh-CN.md)

One service shared by all users: accounts, device pairing, and relaying messages between desktops (macOS, Windows, Linux) and phones. Code and agents stay on the user's own computer. The server passes message bodies through without parsing them; clients are meant to encrypt them end to end, so the server never sees code or conversations.

- Node 22 + TypeScript, Fastify (HTTP) + `ws` (WebSocket)
- PostgreSQL
- Sign-in uses the open-source [better-auth](https://www.better-auth.com): one-time codes by email, and native clients send `Authorization: Bearer <token>`

## Status

In development, not ready for production:

- The mobile apps connect to it with end-to-end encryption (see [docs/mobile.md](../docs/mobile.md)). The desktop app does not connect yet; until it does, `true mobile-sandbox` runs the server with a simulated desktop.
- No email service is wired up. Sign-in codes are only printed to the log, in every environment. To send real email, implement the `Mailer` interface from `src/mailer.ts` and use it in `src/main.ts`.
- Push tokens are stored, but no push notifications are sent.

## Run locally

```sh
docker compose up -d      # PostgreSQL on port 54329
pnpm install
pnpm dev                  # http://localhost:8787, creates tables on startup
pnpm test                 # end-to-end tests, each run uses a temporary database
```

## Environment variables

| Variable | Description | Default |
|---|---|---|
| `DATABASE_URL` | PostgreSQL connection string | `postgres://duolo:duolo@localhost:54329/duolo` |
| `BETTER_AUTH_SECRET` | Key that signs sessions. **Required in production** (`NODE_ENV=production`) | A fixed development value |
| `PUBLIC_URL` | Public address | `http://localhost:8787` |
| `PORT` / `HOST` | Address to listen on | `8787` / `0.0.0.0` |
| `MAILBOX_TTL_DAYS` | How many days undelivered messages are kept | `7` |
| `MAX_MESSAGE_BYTES` | Maximum size of one message | `262144` |
| `DOUBAO_ASR_API_KEY` | Doubao speech API key for voice input. Empty turns voice input off. Keep keys in the environment, never in the repository | empty |
| `DOUBAO_ASR_RESOURCE_ID` / `DOUBAO_ASR_URL` | Doubao streaming recognition resource and address | `volc.seedasr.sauc.duration` / `wss://openspeech.bytedance.com/api/v3/sauc/bigmodel_async` |
| `VOICE_DAILY_MINUTES` / `VOICE_ANONYMOUS_DAILY_MINUTES` | Minutes of voice input per user per day, for signed-in and anonymous users | `120` / `30` |
| `VOICE_ORGANIZE_BASE_URL` / `VOICE_ORGANIZE_API_KEY` / `VOICE_ORGANIZE_MODEL` | The model that turns what was said into a clean instruction (an OpenAI-compatible API; the address ends at `/v1`). If any is empty, nothing is cleaned up | empty |
| `VOICE_ORGANIZE_TIMEOUT_MS` | How long to wait for the clean-up before using what was said | `4000` |

## API

| Endpoint | Description |
|---|---|
| `POST /v1/auth/email-otp/send-verification-otp` | Send a sign-in code: `{ email, type: "sign-in" }`. The email language follows `Accept-Language` |
| `POST /v1/auth/sign-in/email-otp` | Sign in with the code: `{ email, otp }` → `{ token, user }`. The first sign-in creates the account |
| `POST /v1/auth/sign-in/anonymous` | Get an anonymous session without signing up → `{ token, user }`. Desktops use it for voice input; signing in with email later merges it into a real account |
| `POST /v1/devices` | Register the current device: `{ name, platform: "mac" \| "windows" \| "linux" \| "ios" \| "android", publicKey }` |
| `GET /v1/devices` | My devices, their public keys, and whether they are online |
| `DELETE /v1/devices/:id` | Remove a device: its session is revoked and its connection closed |
| `PUT /v1/devices/me/push` | Report a push token: `{ kind: "apns" \| "fcm", token }` |
| `POST /v1/pairings` | Desktop creates a one-time pairing code (valid for 10 minutes) to show as a QR code. Phones get `desktopOnly` |
| `POST /v1/pairings/claim` | A phone signs in to the same account with the pairing code: `{ code, device }` → `{ token, device, desktop }` (includes the desktop's public key) |
| `GET /v1/connect` | WebSocket relay. The protocol is described in `src/relay.ts` |
| `GET /v1/voice` | WebSocket voice recognition, anonymous sessions allowed. Audio goes to Doubao with the task's hot words and recent conversation. The protocol is described in `src/voice.ts` |

Errors are returned as codes, for example `{ "error": "invalidPairingCode" }`, and clients show them in their own language.

## How delivery is guaranteed

Every message is first stored in an offline mailbox (the `mailbox` table). If the recipient is online it is pushed right away, and it is deleted only after the recipient sends an `ack`. When a recipient comes online, unacknowledged messages are sent again. Sending the same message ID twice stores it once. Messages older than 7 days (`MAILBOX_TTL_DAYS`) are removed automatically.

It runs as a single server today. With several servers, PostgreSQL `LISTEN/NOTIFY` would pass messages to the one the recipient is connected to.
