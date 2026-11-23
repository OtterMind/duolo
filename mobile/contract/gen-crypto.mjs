// 生成 crypto.json：端到端加密的固定样例，Swift 和 Kotlin 的实现都要算出一样的结果。
// 算法见 docs/mobile.md。改了算法才需要重新生成：node mobile/contract/gen-crypto.mjs
import { createCipheriv, createPrivateKey, createPublicKey, diffieHellman, hkdfSync } from "node:crypto";
import { writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const bytes = (start, n = 32) => Buffer.from(Array.from({ length: n }, (_, i) => (start + i * 7) % 256));
const PKCS8 = Buffer.from("302e020100300506032b656e04220420", "hex");
const priv = (raw) => createPrivateKey({ key: Buffer.concat([PKCS8, raw]), format: "der", type: "pkcs8" });
const pub = (raw) => createPublicKey(priv(raw)).export({ format: "der", type: "spki" }).subarray(-32);
const b64 = (b) => Buffer.from(b).toString("base64");

const desktop = bytes(1);
const phone = bytes(90);
const secret = bytes(200);
const shared = diffieHellman({ privateKey: priv(desktop), publicKey: createPublicKey(priv(phone)) });
const derive = (direction) => {
  const info = Buffer.concat([Buffer.from(`duolo/v1 ${direction}`), pub(desktop), pub(phone)]);
  return Buffer.from(hkdfSync("sha256", shared, secret, info, 32));
};
const keys = { desktopToPhone: derive("desktop->phone"), phoneToDesktop: derive("phone->desktop") };

const seal = (k, nonce, plaintext, id, from, to) => {
  const cipher = createCipheriv("chacha20-poly1305", k, nonce, { authTagLength: 16 });
  cipher.setAAD(Buffer.from(`duolo/v1\n${id}\n${from}\n${to}`), { plaintextLength: plaintext.length });
  const sealed = Buffer.concat([cipher.update(plaintext), cipher.final()]);
  return b64(Buffer.concat([Buffer.from([1]), nonce, sealed, cipher.getAuthTag()]));
};
const message = (direction, nonceStart, plaintext, id, from, to) => {
  const nonce = bytes(nonceStart, 12);
  const body = seal(keys[direction], nonce, Buffer.from(plaintext), id, from, to);
  return { direction, id, from, to, nonce: b64(nonce), plaintext, body };
};

const out = {
  comment: "由 gen-crypto.mjs 生成",
  desktop: { privateKey: b64(desktop), publicKey: b64(pub(desktop)) },
  phone: { privateKey: b64(phone), publicKey: b64(pub(phone)) },
  secret: b64(secret),
  keys: { desktopToPhone: b64(keys.desktopToPhone), phoneToDesktop: b64(keys.phoneToDesktop) },
  messages: [
    message("phoneToDesktop", 3, '{"v":1,"type":"sync"}', "m-1", "phone-1", "desktop-1"),
    message("desktopToPhone", 50, '{"v":1,"type":"result","id":"c-1","result":{"ok":true}}', "m-2", "desktop-1", "phone-1"),
    message("desktopToPhone", 77, "多字节：你好 👋", "m-3", "desktop-1", "phone-1"),
  ],
};
writeFileSync(fileURLToPath(new URL("crypto.json", import.meta.url)), JSON.stringify(out, null, 2) + "\n");
