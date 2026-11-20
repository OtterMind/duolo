// 检查各语言包：键和英文一致，且每条译文的占位符（{name}）与英文相同。
// 键的完整性 TypeScript 也会检查；占位符只能靠这里。由 `pnpm build` 调用。
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

const dir = new URL("../src/i18n/locales/", import.meta.url).pathname;
const entry = /^\s*"([\w.]+)":\s*\n?\s*("(?:[^"\\]|\\.)*")/gm;

function load(file) {
  const text = readFileSync(join(dir, file), "utf8");
  const map = new Map();
  for (const [, key, value] of text.matchAll(entry)) map.set(key, JSON.parse(value));
  return map;
}
const placeholders = (s) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort().join(",");

const base = load("en.ts");
let problems = 0;
for (const file of readdirSync(dir).filter((f) => f.endsWith(".ts") && f !== "en.ts")) {
  const msgs = load(file);
  for (const [key, value] of base) {
    if (!msgs.has(key)) {
      console.error(`${file}: missing ${key}`);
      problems++;
    } else if (placeholders(msgs.get(key)) !== placeholders(value)) {
      console.error(`${file}: ${key} placeholders {${placeholders(msgs.get(key))}} ≠ en {${placeholders(value)}}`);
      problems++;
    }
  }
  for (const key of msgs.keys()) {
    if (!base.has(key)) {
      console.error(`${file}: unknown key ${key}`);
      problems++;
    }
  }
}
if (problems) {
  console.error(`i18n: ${problems} problem(s)`);
  process.exit(1);
}
console.log(`i18n: ${base.size} keys × ${readdirSync(dir).length} locales OK`);
