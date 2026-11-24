// 把 GitHub Octicons（MIT）变成两个平台的图标资源。图标有增减时才需要重新运行：
//
//   npm pack @primer/octicons@19.15.0 && tar xzf primer-octicons-*.tgz
//   node mobile/scripts/gen-icons.mjs package/build/svg
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const source = process.argv[2];
const root = fileURLToPath(new URL("..", import.meta.url));
const ICONS = [
  "three-bars", "search", "plus", "repo", "device-desktop", "gear", "issue-opened", "pencil", "eye", "chevron-right", "chevron-left", "chevron-down", "kebab-horizontal", "x", "arrow-up", "arrow-left",
  "square-fill", "clock", "check", "file-directory", "alert", "dash",
];
// Octicons 里没有麦克风，用一组声波代替；状态圆里的问号不要外圈，自己画。
const CUSTOM = {
  mic: { viewport: 24, width: 2, stroke: "M12 4v16M8 8v8M16 8v8M4 11v2M20 11v2" },
  ask: { viewport: 16, width: 1.8, stroke: "M5.6 6.1a2.4 2.4 0 1 1 3.4 2.2c-.65.32-1 .82-1 1.5v.45M8 12.6v.01" },
};

const paths = (name) => {
  const svg = readFileSync(join(source, `${name}-16.svg`), "utf8");
  return [...svg.matchAll(/\sd="([^"]+)"/g)].map((m) => m[1]);
};

const assets = join(root, "ios/Duolo/Assets.xcassets/Icons");
const drawables = join(root, "android/app/src/main/res/drawable");
mkdirSync(drawables, { recursive: true });
const contents = { info: { author: "xcode", version: 1 } };

function ios(name, svg) {
  const dir = join(assets, `${name}.imageset`);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, `${name}.svg`), svg);
  const image = { images: [{ filename: `${name}.svg`, idiom: "universal" }], ...contents, properties: { "preserves-vector-representation": true, "template-rendering-intent": "template" } };
  writeFileSync(join(dir, "Contents.json"), JSON.stringify(image, null, 2) + "\n");
}
function android(name, size, body) {
  const xml = `<?xml version="1.0" encoding="utf-8"?>
<vector xmlns:android="http://schemas.android.com/apk/res/android" android:width="${size}dp" android:height="${size}dp" android:viewportWidth="${size}" android:viewportHeight="${size}">
${body}
</vector>
`;
  writeFileSync(join(drawables, `ic_${name.replaceAll("-", "_")}.xml`), xml);
}

mkdirSync(assets, { recursive: true });
writeFileSync(join(assets, "Contents.json"), JSON.stringify(contents, null, 2) + "\n");
for (const name of ICONS) {
  const d = paths(name);
  ios(name, `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" width="16" height="16">${d.map((p) => `<path d="${p}"/>`).join("")}</svg>\n`);
  android(name, 16, d.map((p) => `    <path android:fillColor="#FF000000" android:pathData="${p}" />`).join("\n"));
}
for (const [name, { viewport, width, stroke }] of Object.entries(CUSTOM)) {
  ios(name, `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${viewport} ${viewport}" width="${viewport}" height="${viewport}"><path d="${stroke}" fill="none" stroke="#000" stroke-width="${width}" stroke-linecap="round" stroke-linejoin="round"/></svg>\n`);
  android(name, viewport, `    <path android:strokeColor="#FF000000" android:strokeWidth="${width}" android:strokeLineCap="round" android:strokeLineJoin="round" android:pathData="${stroke}" />`);
}
console.log(`${ICONS.length + Object.keys(CUSTOM).length} icons`);
