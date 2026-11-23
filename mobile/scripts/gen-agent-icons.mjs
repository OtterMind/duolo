// 把 Agent 插件自带的图标（crates/duolo-agent/agents/<id>/icon.svg）变成两个平台的图标资源。
// 插件有增减、换了图标时运行：node mobile/scripts/gen-agent-icons.mjs
//
// 图标是单色的，颜色由电脑随数据发来（插件的 icon_color）。这里只认 path、line、circle、ellipse
// 和 rotate()，别的写法直接报错，免得新插件的图标在手机上悄悄画错。
import { mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync, existsSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const repo = fileURLToPath(new URL("../../", import.meta.url));
const plugins = join(repo, "crates/duolo-agent/agents");
const assets = join(repo, "mobile/ios/Duolo/Assets.xcassets/Agents");
const drawables = join(repo, "mobile/android/app/src/main/res/drawable");
const kotlin = join(repo, "mobile/android/app/src/main/kotlin/ai/ottermind/duolo/AgentIcons.kt");

const attrs = (tag) => Object.fromEntries([...tag.matchAll(/([\w-]+)="([^"]*)"/g)].map((m) => [m[1], m[2]]));
const num = (v) => Number(v ?? 0);
const fmt = (n) => Number(n.toFixed(3)).toString();

/** rotate(a cx cy) 作用在点上。 */
function rotate(transform, x, y) {
  if (!transform) return [x, y];
  const m = transform.match(/^rotate\(\s*([-\d.]+)(?:[\s,]+([-\d.]+)[\s,]+([-\d.]+))?\s*\)$/);
  if (!m) throw new Error(`unsupported transform: ${transform}`);
  const [a, cx, cy] = [num(m[1]) * (Math.PI / 180), num(m[2]), num(m[3])];
  return [cx + (x - cx) * Math.cos(a) - (y - cy) * Math.sin(a), cy + (x - cx) * Math.sin(a) + (y - cy) * Math.cos(a)];
}
const angle = (transform) => num(transform?.match(/rotate\(\s*([-\d.]+)/)?.[1]);

/** 一个元素变成一段路径，带上它（或外层 g）的描边、填充。 */
function shape(name, a, inherited) {
  const style = { ...inherited, ...a };
  let d;
  if (name === "path") {
    if (a.transform) throw new Error("transform on <path> is not supported");
    d = a.d;
  } else if (name === "line") {
    const [x1, y1] = rotate(a.transform, num(a.x1), num(a.y1));
    const [x2, y2] = rotate(a.transform, num(a.x2), num(a.y2));
    d = `M${fmt(x1)} ${fmt(y1)}L${fmt(x2)} ${fmt(y2)}`;
  } else if (name === "circle" || name === "ellipse") {
    const [cx, cy] = [num(a.cx), num(a.cy)];
    const [rx, ry] = name === "circle" ? [num(a.r), num(a.r)] : [num(a.rx), num(a.ry)];
    const turn = angle(a.transform);
    const [ux, uy] = [Math.cos((turn * Math.PI) / 180) * rx, Math.sin((turn * Math.PI) / 180) * rx];
    const [p1, p2] = [`${fmt(cx + ux)} ${fmt(cy + uy)}`, `${fmt(cx - ux)} ${fmt(cy - uy)}`];
    d = `M${p1}A${fmt(rx)} ${fmt(ry)} ${fmt(turn)} 1 0 ${p2}A${fmt(rx)} ${fmt(ry)} ${fmt(turn)} 1 0 ${p1}Z`;
  } else {
    throw new Error(`unsupported element <${name}>`);
  }
  const stroke = style.stroke && style.stroke !== "none";
  return {
    d,
    stroke: stroke ? num(style["stroke-width"] ?? 1) : null,
    cap: style["stroke-linecap"] ?? "butt",
    fill: !stroke || (style.fill && style.fill !== "none"),
    evenOdd: style["fill-rule"] === "evenodd",
  };
}

function parse(svg) {
  const view = attrs(svg.match(/<svg[^>]*>/)[0]).viewBox?.split(/\s+/).map(Number) ?? [0, 0, 16, 16];
  const shapes = [];
  const groups = [{}];
  for (const [, close, name, rest] of svg.matchAll(/<(\/?)([\w]+)([^>]*)>/g)) {
    if (name === "svg") continue;
    if (name === "g") {
      if (close) groups.pop();
      else groups.push({ ...groups.at(-1), ...attrs(rest) });
      continue;
    }
    if (!close) shapes.push(shape(name, attrs(rest), groups.at(-1)));
  }
  return { size: view[2], shapes };
}

const ios = ({ size, shapes }) =>
  `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${size} ${size}" width="${size}" height="${size}">` +
  shapes
    .map((s) =>
      s.stroke != null
        ? `<path d="${s.d}" fill="none" stroke="#000" stroke-width="${s.stroke}" stroke-linecap="${s.cap}"/>`
        : `<path d="${s.d}" fill="#000"${s.evenOdd ? ' fill-rule="evenodd"' : ""}/>`,
    )
    .join("") +
  "</svg>\n";

const android = ({ size, shapes }) => `<?xml version="1.0" encoding="utf-8"?>
<!-- 由 mobile/scripts/gen-agent-icons.mjs 从插件的 icon.svg 生成 -->
<vector xmlns:android="http://schemas.android.com/apk/res/android" android:width="${size}dp" android:height="${size}dp" android:viewportWidth="${size}" android:viewportHeight="${size}">
${shapes
  .map((s) =>
    s.stroke != null
      ? `    <path android:strokeColor="#FF000000" android:strokeWidth="${s.stroke}" android:strokeLineCap="${s.cap}" android:pathData="${s.d}" />`
      : `    <path android:fillColor="#FF000000"${s.evenOdd ? ' android:fillType="evenOdd"' : ""} android:pathData="${s.d}" />`,
  )
  .join("\n")}
</vector>
`;

const ids = readdirSync(plugins).filter((id) => existsSync(join(plugins, id, "icon.svg"))).sort();
rmSync(assets, { recursive: true, force: true });
mkdirSync(assets, { recursive: true });
writeFileSync(join(assets, "Contents.json"), JSON.stringify({ info: { author: "xcode", version: 1 } }, null, 2) + "\n");
for (const file of readdirSync(drawables).filter((f) => f.startsWith("ic_agent_"))) rmSync(join(drawables, file));
const names = [];
for (const id of ids) {
  const icon = parse(readFileSync(join(plugins, id, "icon.svg"), "utf8"));
  const res = id.toLowerCase().replace(/[^a-z0-9]/g, "_");
  const dir = join(assets, `agent-${id}.imageset`);
  mkdirSync(dir);
  writeFileSync(join(dir, "icon.svg"), ios(icon));
  const image = {
    images: [{ filename: "icon.svg", idiom: "universal" }],
    info: { author: "xcode", version: 1 },
    properties: { "preserves-vector-representation": true, "template-rendering-intent": "template" },
  };
  writeFileSync(join(dir, "Contents.json"), JSON.stringify(image, null, 2) + "\n");
  writeFileSync(join(drawables, `ic_agent_${res}.xml`), android(icon));
  names.push(`    ${JSON.stringify(id)} to R.drawable.ic_agent_${res},`);
}
writeFileSync(
  kotlin,
  `package ai.ottermind.duolo

// 由 mobile/scripts/gen-agent-icons.mjs 生成，不要手改。

/** 插件自带的 Agent 图标，按 Agent 的种类找；没有的显示首字母。 */
val agentIcons: Map<String, Int> = mapOf(
${names.join("\n")}
)
`,
);
console.log(`${ids.length} agent icons: ${ids.join(", ")}`);
