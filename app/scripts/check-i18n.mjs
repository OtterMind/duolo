// 检查语言包（仓库根目录的 locales/*.json）。由 `pnpm build` 和 `pnpm check` 调用。
//
// 1. 每种语言的键和英文完全一致；
// 2. 每条译文的占位符（{name}）和英文相同；
// 3. 代码里用到的键都存在：Rust 的 t("…") / tf("…")（命令行、终端界面、菜单），手机端 Swift / Kotlin 的 t("…")。
//    TypeScript 里的键由类型检查保证；
// 4. TypeScript 里有占位符的文案调用时传了参数；
// 5. 没有用不到的键：每个键都要在代码里出现（status.*、state.*、error.*、menu.*、macos.*、mobile.perm.*、automation.skip.* 是拼出来的，不检查）；
// 6. iOS 系统授权弹窗的说明是最新的（mobile/scripts/gen-ios-strings.mjs）。
import { spawnSync } from "node:child_process";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const repo = fileURLToPath(new URL("../../", import.meta.url));
const dir = join(repo, "locales");
const load = (file) => new Map(Object.entries(JSON.parse(readFileSync(join(dir, file), "utf8"))));
const placeholders = (s) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort().join(",");

const base = load("en.json");
const files = readdirSync(dir).filter((f) => f.endsWith(".json"));
let problems = 0;
const problem = (msg) => {
  console.error(msg);
  problems++;
};

for (const file of files.filter((f) => f !== "en.json")) {
  const msgs = load(file);
  for (const [key, value] of base) {
    if (!msgs.has(key)) problem(`${file}: missing ${key}`);
    else if (placeholders(msgs.get(key)) !== placeholders(value))
      problem(`${file}: ${key} placeholders {${placeholders(msgs.get(key))}} ≠ en {${placeholders(value)}}`);
    else if (!msgs.get(key).trim()) problem(`${file}: ${key} is empty`);
  }
  for (const key of msgs.keys()) if (!base.has(key)) problem(`${file}: unknown key ${key}`);
}

// Rust 代码里用到的键。
function* rustFiles(path) {
  for (const name of readdirSync(path)) {
    const full = join(path, name);
    if (statSync(full).isDirectory()) {
      if (name !== "target" && name !== "gen") yield* rustFiles(full);
    } else if (name.endsWith(".rs")) yield full;
  }
}
let used = 0;
let allSource = "";
function* sourceFiles(path, ext) {
  for (const name of readdirSync(path)) {
    const full = join(path, name);
    if (statSync(full).isDirectory()) {
      if (!["target", "gen", "node_modules", "dist", "build", ".build"].includes(name)) yield* sourceFiles(full, ext);
    } else if (ext.test(name)) yield full;
  }
}
for (const file of sourceFiles(join(repo, "app/src"), /\.tsx?$/)) allSource += readFileSync(file, "utf8");
// 服务端（server/）发给用户的文字（验证码邮件）也用这份语言包。
for (const file of sourceFiles(join(repo, "server/src"), /\.ts$/)) allSource += readFileSync(file, "utf8");
// 手机端（mobile/）：Swift 和 Kotlin 都是 t("key", …)，占位符要给值。
let mobile = 0;
for (const file of sourceFiles(join(repo, "mobile"), /\.(swift|kt)$/)) {
  if (/\/(Tests|test)\//.test(file)) continue;
  const text = readFileSync(file, "utf8");
  allSource += text;
  for (const [, key, next] of text.matchAll(/\bt\(\s*"([a-zA-Z]+\.[\w.]+)"\s*([,)])/g)) {
    mobile++;
    if (!base.has(key)) problem(`${file.slice(repo.length)}: unknown key ${key}`);
    else if (next === ")" && placeholders(base.get(key))) problem(`${file.slice(repo.length)}: t("${key}") needs {${placeholders(base.get(key))}}`);
  }
}
for (const file of rustFiles(join(repo, "crates"))) {
  const text = readFileSync(file, "utf8");
  allSource += text;
  for (const [, key] of text.matchAll(/\bt[f]?\(\s*"([a-zA-Z]+\.[\w.]+)"/g)) {
    used++;
    if (!base.has(key)) problem(`${file.slice(repo.length)}: unknown key ${key}`);
  }
}

// TypeScript 里调用 t("key") 却没传参数，而文案里有占位符：显示出来会是 {name} 原样。
for (const file of sourceFiles(join(repo, "app/src"), /\.tsx?$/)) {
  const text = readFileSync(file, "utf8");
  for (const [, key] of text.matchAll(/\bt\(\s*"([\w.]+)"\s*\)/g)) {
    if (base.has(key) && placeholders(base.get(key))) {
      problem(`${file.slice(repo.length)}: t("${key}") needs {${placeholders(base.get(key))}}`);
    }
  }
}

for (const key of base.keys()) {
  if (!/^(status|state|error|menu|macos|mobile\.perm|automation\.skip)\./.test(key) && !allSource.includes(`"${key}"`)) problem(`en.json: ${key} is not used anywhere`);
}

const strings = spawnSync(process.execPath, [join(repo, "mobile/scripts/gen-ios-strings.mjs"), "--check"], { stdio: "inherit" });
if (strings.status !== 0) problems++;

if (problems) {
  console.error(`i18n: ${problems} problem(s)`);
  process.exit(1);
}
console.log(`i18n: ${base.size} keys × ${files.length} locales OK (${used} uses in Rust, ${mobile} on mobile)`);
