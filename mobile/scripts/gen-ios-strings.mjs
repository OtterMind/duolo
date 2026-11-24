// iOS 系统授权弹窗的说明按语言放在 <lang>.lproj/InfoPlist.strings，从 locales/*.json 的 mobile.perm.* 生成。
//
//   node mobile/scripts/gen-ios-strings.mjs          生成
//   node mobile/scripts/gen-ios-strings.mjs --check  检查有没有忘了生成（app 的 true build 会调用）
import { mkdirSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const repo = fileURLToPath(new URL("../../", import.meta.url));
const check = process.argv.includes("--check");
const LPROJ = { "zh-CN": "zh-Hans", "zh-TW": "zh-Hant" };
const CODES = ["en", "zh-CN", "zh-TW", "ja", "ko", "es", "fr", "de", "pt-BR", "ru", "it", "tr", "vi", "id"];
const KEYS = {
  NSCameraUsageDescription: "mobile.perm.camera",
  NSMicrophoneUsageDescription: "mobile.perm.microphone",
};
const quote = (s) => `"${s.replaceAll("\\", "\\\\").replaceAll('"', '\\"')}"`;

let stale = 0;
for (const code of CODES) {
  const messages = JSON.parse(readFileSync(join(repo, "locales", `${code}.json`), "utf8"));
  const text = `/* 由 mobile/scripts/gen-ios-strings.mjs 生成 */\n${Object.entries(KEYS)
    .map(([plist, key]) => `${plist} = ${quote(messages[key])};`)
    .join("\n")}\n`;
  const dir = join(repo, "mobile/ios/Duolo", `${LPROJ[code] ?? code}.lproj`);
  const file = join(dir, "InfoPlist.strings");
  if (check) {
    if (!existsSync(file) || readFileSync(file, "utf8") !== text) {
      console.error(`${file.slice(repo.length)} is stale: run node mobile/scripts/gen-ios-strings.mjs`);
      stale++;
    }
  } else {
    mkdirSync(dir, { recursive: true });
    writeFileSync(file, text);
  }
}
if (stale) process.exit(1);
