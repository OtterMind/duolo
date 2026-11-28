// duolo.io 的几个网页：首页（介绍和下载）、隐私政策、使用条款、支持。App Store、Google Play 上架要用到后三个。
//
// 页面按 Accept-Language 选语言，`?lang=zh-CN` 可以指定。首页和支持页的文案在 locales/*.json（site.*，14 种语言）；
// 隐私政策和使用条款是法律文本，只有英文和简体中文两份（site/*.html），别的语言显示英文版并说明。

import { readdirSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import type { FastifyInstance, FastifyReply, FastifyRequest } from "fastify";

import { matchLocale, t, type Locale } from "./i18n.js";

const dir = process.env.DUOLO_SITE_DIR ?? fileURLToPath(new URL("../site/", import.meta.url));
const REPO = "https://github.com/OtterMind/duolo";
const DOWNLOAD = `${REPO}/releases/latest`;
const SITE = "https://duolo.io";
/** 有译本的法律文本语言；其他语言用英文版。 */
const LEGAL_LOCALES = ["en", "zh-CN"] as const;

const files = new Map<string, string>();
function file(name: string): string {
  let text = files.get(name);
  if (text === undefined) {
    text = readFileSync(`${dir}/${name}`, "utf8");
    files.set(name, text);
  }
  return text;
}

const escape = (text: string) =>
  text.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);

function localeOf(request: FastifyRequest): Locale {
  const lang = (request.query as Record<string, unknown> | undefined)?.lang;
  const accept = request.headers["accept-language"] ?? "";
  return matchLocale(typeof lang === "string" && lang ? `${lang},${accept}` : accept);
}

const ICON = {
  github: '<path d="M12 .5a11.5 11.5 0 0 0-3.64 22.41c.58.1.79-.25.79-.56v-2c-3.2.7-3.88-1.37-3.88-1.37-.52-1.33-1.28-1.69-1.28-1.69-1.05-.71.08-.7.08-.7 1.16.08 1.77 1.19 1.77 1.19 1.03 1.77 2.7 1.26 3.36.96.1-.75.4-1.26.73-1.55-2.55-.29-5.24-1.28-5.24-5.69 0-1.26.45-2.28 1.19-3.09-.12-.29-.52-1.46.11-3.05 0 0 .97-.31 3.17 1.18a11 11 0 0 1 5.77 0c2.2-1.49 3.17-1.18 3.17-1.18.63 1.59.23 2.76.11 3.05.74.81 1.19 1.83 1.19 3.09 0 4.42-2.7 5.39-5.26 5.68.41.36.78 1.06.78 2.14v3.17c0 .31.21.67.8.56A11.5 11.5 0 0 0 12 .5Z" fill="currentColor"/>',
  apple: '<path d="M16.37 12.6c-.02-2.3 1.88-3.4 1.97-3.46-1.07-1.57-2.74-1.78-3.33-1.8-1.42-.15-2.77.83-3.49.83-.72 0-1.83-.81-3.01-.79-1.55.02-2.98.9-3.78 2.29-1.61 2.8-.41 6.94 1.16 9.21.77 1.11 1.68 2.36 2.88 2.31 1.16-.05 1.6-.75 3-.75s1.79.75 3.02.73c1.25-.02 2.04-1.13 2.8-2.25.88-1.29 1.25-2.54 1.27-2.6-.03-.01-2.43-.93-2.49-3.72ZM14.07 5.85c.64-.78 1.07-1.85.95-2.92-.92.04-2.03.61-2.69 1.39-.59.68-1.11 1.77-.97 2.82 1.03.08 2.07-.52 2.71-1.29Z" fill="currentColor"/>',
  parallel: '<rect x="3" y="4" width="7" height="16" rx="2"/><rect x="14" y="4" width="7" height="16" rx="2"/>',
  review: '<path d="M9 12l2 2 4-4"/><path d="M12 3l7 3v6c0 4.5-3 7.7-7 9-4-1.3-7-4.5-7-9V6l7-3z"/>',
  chat: '<path d="M4 5h16v11H9l-5 4V5z"/>',
  handoff: '<path d="M4 12h13"/><path d="M13 7l5 5-5 5"/><path d="M20 5v14"/>',
  grid: '<rect x="3" y="3" width="8" height="8" rx="1.5"/><rect x="13" y="3" width="8" height="8" rx="1.5"/><rect x="3" y="13" width="8" height="8" rx="1.5"/><rect x="13" y="13" width="8" height="8" rx="1.5"/>',
  voice: '<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5 11a7 7 0 0 0 14 0"/><path d="M12 18v3"/>',
  clock: '<circle cx="12" cy="13" r="8"/><path d="M12 9v4l2.5 2.5"/><path d="M5 3 2 6M19 3l3 3"/>',
  accounts: '<circle cx="9" cy="8" r="3.5"/><path d="M2.5 20c.8-3.5 3.4-5.5 6.5-5.5s5.7 2 6.5 5.5"/><path d="M16 4.5a3.5 3.5 0 0 1 0 7M18.5 14.8c1.6.8 2.6 2.6 3 5.2"/>',
  terminal: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 9l3 3-3 3M13 15h4"/>',
  local: '<rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8 20h8M12 16v4"/>',
  phone: '<rect x="7" y="2.5" width="10" height="19" rx="2.5"/><path d="M11 18.5h2"/>',
};
const icon = (name: keyof typeof ICON, size = 20) =>
  `<svg class="i" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${ICON[name]}</svg>`;

/** 内置的 Agent：图标和插件里的一样（site/agents/*.svg 复制自 crates/duolo-agent/agents/<id>/icon.svg，测试核对）。 */
const AGENTS = [
  { id: "claude", name: "Claude Code", color: "#d97757" },
  { id: "codex", name: "Codex" },
  { id: "opencode", name: "OpenCode" },
  { id: "pi", name: "pi" },
  { id: "grok", name: "Grok" },
];
const agentIcon = (id: string, color = "currentColor", size = 18) =>
  `<span class="agent" style="width:${size}px;height:${size}px;background:${color};-webkit-mask:url(/agents/${id}.svg) center/contain no-repeat;mask:url(/agents/${id}.svg) center/contain no-repeat"></span>`;

/** 页脚的语言切换：语言名用各自的写法，不翻译。 */
const LANGUAGES: [Locale, string][] = [
  ["en", "English"], ["zh-CN", "简体中文"], ["zh-TW", "繁體中文"], ["ja", "日本語"], ["ko", "한국어"], ["es", "Español"], ["fr", "Français"],
  ["de", "Deutsch"], ["pt-BR", "Português (Brasil)"], ["ru", "Русский"], ["it", "Italiano"], ["tr", "Türkçe"], ["vi", "Tiếng Việt"], ["id", "Bahasa Indonesia"],
];

function page(locale: Locale, title: string, body: string, wide = false): string {
  const tr = (key: string) => escape(t(locale, key));
  const current = LANGUAGES.find(([code]) => code === locale)?.[1] ?? "English";
  return `<!doctype html>
<html lang="${locale}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${escape(title)}</title>
<meta name="description" content="${tr("site.description")}">
<meta property="og:title" content="Duolo — ${tr("site.tagline")}">
<meta property="og:description" content="${tr("site.description")}">
<meta property="og:image" content="${SITE}/icon.png">
<meta name="theme-color" content="#08090a">
<link rel="icon" type="image/png" href="/favicon.png">
<style>${STYLE}</style>
</head>
<body>
<header class="top"><div class="wrap bar">
<a class="brand" href="/"><img src="/icon.png" alt="" width="28" height="28">Duolo</a>
<nav class="links"><a href="/#review">${tr("site.navReview")}</a><a href="/#voice">${tr("site.navVoice")}</a><a href="/#phone">${tr("site.navPhone")}</a><a href="/#compare">${tr("site.navCompare")}</a><a href="${REPO}#readme">${tr("site.navDocs")}</a></nav>
<nav class="end"><a class="ghost" href="${REPO}" aria-label="GitHub">${icon("github", 20)}</a><a class="button primary small" href="${DOWNLOAD}">${tr("site.navDownload")}</a></nav>
</div></header>
<main class="${wide ? "home" : "wrap doc"}">${body}</main>
<footer class="foot"><div class="wrap foot-bar">
<span class="copy">© 2026 Aita Technology (Hangzhou) Co., Ltd. · ${tr("site.footerNote")}</span>
<a href="/privacy">${tr("site.privacy")}</a><a href="/terms">${tr("site.terms")}</a><a href="/support">${tr("site.support")}</a><a href="${REPO}/releases">${tr("site.changelog")}</a><a href="${REPO}">GitHub</a>
<details class="lang"><summary>${escape(current)} ▾</summary><div>${LANGUAGES.map(([code, name]) => `<a href="?lang=${code}" lang="${code}"${code === locale ? ' aria-current="true"' : ""}>${escape(name)}</a>`).join("")}</div></details>
</div></footer>
</body>
</html>`;
}

/** 首页演示面板里的任务：界面截图一样的示意，内容（任务名、命令、代码）不翻译。 */
const LANES = `<div class="lanes" aria-hidden="true">
<div class="win-bar"><span></span><span></span><span></span><em>~/code/shop · 3 worktrees</em><em class="end">⌥↓</em></div>
<div class="lane-grid">
<div class="lane"><div class="lane-head">${agentIcon("claude", "#d97757", 15)}<b>Export orders as CSV</b><span class="pill wait">needs you</span></div>
<p class="dim">feat/order-csv-export · <i class="ins">+48</i> <i class="del">−6</i></p><p>● Edited src/routes/orders.ts</p><p>● Ran pnpm test — 42 passed</p>
<div class="ask">Run the migration on staging?<br><span>› 1. Yes</span>   2. No</div></div>
<div class="lane"><div class="lane-head">${agentIcon("codex", "#c9cdd3", 15)}<b>Stripe billing</b><span class="pill run">working</span></div>
<p class="dim">feat/stripe-billing · <i class="ins">+212</i> <i class="del">−19</i></p><p>● Read src/billing/webhook.ts</p><p>● Writing tests for invoice.paid…</p><p class="dim cursor">▍</p></div>
<div class="lane"><div class="lane-head">${agentIcon("claude", "#d97757", 15)}<b>Fix login redirect</b><span class="pill ok">reviewed</span></div>
<p class="dim">fix/login-redirect · <i class="ins">+9</i> <i class="del">−3</i></p>
<div class="okbox"><span>Codex review · no issues</span><br>The redirect keeps the query string; tests cover both paths.</div><p class="dim">ready to merge</p></div>
</div></div>`;

function home(locale: Locale): string {
  const tr = (key: string) => escape(t(locale, key));
  // 标题里的换行（\n）在宽屏上断开。
  const lines = (key: string) => tr(key).replace(/\n/g, "<br>");
  const label = (n: string, key: string) => `<span class="label">[${n}] ${tr(key)}</span>`;
  const stat = (value: string, key: string) => `<div><b>${value}</b><span>${tr(key)}</span></div>`;
  const yes = (key?: string) => `<td class="yes">✓${key ? ` ${tr(key)}` : ""}</td>`;
  const cell = (key: string, cls = "") => `<td${cls ? ` class="${cls}"` : ""}>${tr(key)}</td>`;
  const row = (key: string, ...cells: string[]) => `<tr><td>${tr(key)}</td>${cells.join("")}</tr>`;
  const bars = [12, 24, 34, 18, 28, 40, 22, 14, 30, 20, 10].map((h) => `<i style="height:${h}px"></i>`).join("");
  return page(
    locale,
    `Duolo — ${t(locale, "site.tagline")}`,
    `<section class="hero wrap" id="top">
<p class="status"><span class="dot"></span>${tr("site.heroStatus")}</p>
<h1>${tr("site.heroLine1")}<br><span class="accent">${tr("site.heroLine2")}</span></h1>
<p class="lead">${tr("site.intro")}</p>
<div class="actions"><a class="button primary" href="${DOWNLOAD}">${icon("apple", 18)}${tr("site.download")}</a><a class="button code" href="${REPO}"><span class="dim">$</span> git clone OtterMind/duolo</a></div>
<p class="note mono">${tr("site.downloadNote")}</p>
${LANES}
<div class="stats">${stat("6", "site.statPanes")}${stat(`5<small> ${tr("site.statPlugins")}</small>`, "site.statAgents")}${stat("15 MB", "site.statSize")}${stat("E2E", "site.statE2E")}${stat("14", "site.statLanguages")}</div>
</section>

<section class="split wrap" id="review">
<div class="copy">${label("01", "site.reviewLabel")}<h2>${lines("site.reviewTitle")}</h2><p>${tr("site.reviewBody1")}</p><p>${tr("site.reviewBody2")}</p></div>
<div class="panel mono" aria-hidden="true"><div class="panel-head">${agentIcon("codex", "#c9cdd3", 14)} Codex · reviewing feat/stripe-billing<em class="end">read-only</em></div>
<div class="panel-body"><p class="warn"><b>[ISSUES 2]</b></p>
<p class="finding"><span class="p1">P1</span>webhook.ts:41 — the signature is checked after the body is parsed; verify the raw body first.</p>
<p class="finding"><span class="p2">P2</span>invoice.paid is handled twice when Stripe retries; store the event id.</p>
<div class="panel-actions"><span class="fake primary">Send to Claude ↵</span><span class="fake">Hand off to Codex</span></div></div></div>
</section>

<section class="split flip wrap">
<div class="copy">${label("02", "site.handoffLabel")}<h2>${lines("site.handoffTitle")}</h2><p>${tr("site.handoffBody")}</p></div>
<div class="panel mono pad" aria-hidden="true">
<p class="who"><i style="background:#d97757"></i><b>Claude</b><span class="dim">feat/cursor-paging</span></p>
<div class="branch"><p class="dim">↳ duolo handoff 7 --to codex --fork "try keyset paging"</p>
<p class="who"><i style="background:#c9cdd3"></i><b>Codex</b><span class="dim">feat/keyset-paging</span></p>
<p class="dim">${tr("site.handoffMockBoth")}</p></div>
<p class="who"><i class="you"></i><b>${tr("site.handoffMockYou")}</b><span class="dim">${tr("site.handoffMockKeep")}</span></p>
</div>
</section>

<section class="split wrap">
<div class="copy">${label("03", "site.statusLabel")}<h2>${lines("site.statusTitle")}</h2><p>${tr("site.statusBody")}</p></div>
<div class="panel side" aria-hidden="true">
<p class="proj mono">▾ shop</p>
<p class="trow on">${agentIcon("claude", "#d97757", 14)}<b>Export orders as CSV</b><span class="st wait">● needs you</span></p>
<p class="trow">${agentIcon("codex", "#c9cdd3", 14)}<b>Stripe billing</b><span class="st run">◐ working</span></p>
<p class="trow">${agentIcon("claude", "#d97757", 14)}<b>Fix login redirect</b><span class="st ok">✓ reviewed</span></p>
<p class="trow">${agentIcon("pi", "#c9cdd3", 14)}<b>Paginate the API</b><span class="st bad">✕ failed</span></p>
<p class="side-note mono">${tr("site.statusMockNote")}</p>
</div>
</section>

<section class="split flip wrap" id="voice">
<div class="copy">${label("04", "site.voiceLabel")}<h2>${lines("site.voiceTitle")}</h2><p>${tr("site.voiceBody1")}</p><p>${tr("site.voiceBody2")}</p></div>
<div class="panel pad voice" aria-hidden="true">
<div class="wave">${bars}<span class="mono dim">${tr("site.voiceHolding")} · 0:04</span></div>
<p class="mono dim small">${tr("site.voiceHeard")}</p>
<p class="mono heard">把 use state 那个 hook 加上，export CSV 也挪到 orders ts 里</p>
<p class="chips mono"><span>useState</span><span>exportCsv</span><span>orders.ts</span><em>← ${tr("site.voiceHints")}</em></p>
<p class="mono okbox big">把 useState 那个 hook 加上，exportCsv 也挪到 orders.ts 里</p>
</div>
</section>

<section class="pair wrap">
<div class="panel pad">${label("05", "site.limitsLabel")}<h3>${tr("site.limitsTitle")}</h3><p>${tr("site.limitsBody")}</p>
<div class="meters mono" aria-hidden="true"><p><span>personal</span><i class="full"></i><em class="bad">100%</em></p><p><span>work</span><i><b style="width:18%"></b></i><em>18%</em></p><p class="dim">→ ${tr("site.limitsSwitched")}</p></div></div>
<div class="panel pad">${label("06", "site.lostLabel")}<h3>${tr("site.lostTitle")}</h3><p>${tr("site.lostBody")}</p>
<div class="log mono" aria-hidden="true"><p><span class="dim">close</span> Export orders as CSV</p><p><span class="okc">✓</span> 3 files committed to feat/order-csv-export</p><p><span class="okc">✓</span> worktree removed · branch kept</p></div></div>
</section>

<section class="split wrap" id="phone">
<div class="copy">${label("07", "site.phoneLabel")}<h2>${lines("site.phoneTitle")}</h2><p>${tr("site.phoneBody")}</p>
<div class="relay mono"><span>${tr("site.phonePhone")}</span><em>⇄</em><span class="dash">${tr("site.phoneRelay")}</span><em>⇄</em><span>${tr("site.phoneComputer")}</span><small>X25519 · HKDF-SHA256 · ChaCha20-Poly1305 · ${tr("site.phoneCrypto")}</small></div></div>
<div class="phone"><img src="/shots/phone.png" width="900" height="1956" alt="${tr("site.phoneAlt")}" loading="lazy"></div>
</section>

<section class="wrap real">
${label("08", "site.realLabel")}
<div class="frame"><img src="/shots/desktop.png" width="2080" height="874" alt="${tr("site.realAlt")}" loading="lazy"></div>
<p class="mono dim small">${tr("site.realCaption")}</p>
</section>

<section class="wrap compare" id="compare">
${label("09", "site.compareLabel")}
<h2>${tr("site.compareTitle")}</h2>
<p class="intro">${tr("site.compareIntro")}</p>
<div class="table"><table>
<thead><tr><th></th><th class="us">Duolo</th><th>Orca</th><th>T3 Code</th></tr></thead>
<tbody>
${row("site.cmpRowReview", yes("site.cmpReadOnly"), cell("site.cmpOrcaReview"), cell("site.cmpT3Review"))}
${row("site.cmpRowE2E", yes(), cell("site.cmpNotStated", "na"), cell("site.cmpNotStated", "na"))}
${row("site.cmpRowVoice", yes("site.cmpAuto"), cell("site.cmpOrcaVoice"), `<td class="na">–</td>`)}
${row("site.cmpRowUsage", yes("site.cmpAuto"), cell("site.cmpOrcaUsage"), `<td class="na">–</td>`)}
${row("site.cmpRowSave", yes(), cell("site.cmpNotStated", "na"), cell("site.cmpNotStated", "na"))}
${row("site.cmpRowSsh", yes(), cell("site.cmpOrcaRemote"), cell("site.cmpT3Remote"))}
${row("site.cmpRowPr", cell("site.cmpNotYet"), `<td class="them">✓</td>`, `<td class="them">✓</td>`)}
${row("site.cmpRowAgents", cell("site.cmpPlugins"), `<td class="them">27</td>`, `<td class="them">6+</td>`)}
${row("site.cmpRowExtras", `<td class="na">–</td>`, `<td class="them">✓</td>`, `<td class="na">–</td>`)}
</tbody></table></div>
<p class="mono dim small">${tr("site.compareNote")}</p>
</section>

<section class="split wrap">
<div class="copy">${label("10", "site.cliLabel")}<h2>${lines("site.cliTitle")}</h2><p>${tr("site.cliBody")}</p></div>
<div class="panel pad term mono" aria-hidden="true">
<p><span class="accentc">❯</span> duolo new "Export orders as CSV"</p><p class="dim">  task 12 · feat/order-csv-export · claude</p>
<p><span class="accentc">❯</span> duolo handoff 12 --to codex --review</p>
<p><span class="accentc">❯</span> duolo automation add "Fix the flakiest test" --cron "0 9 * * 1-5"</p>
<p><span class="accentc">❯</span> duolo tui <span class="dim"># ${tr("site.cliComment")}</span></p>
</div>
</section>

<section class="cta wrap" id="download">
<img src="/icon.png" alt="" width="72" height="72">
<h2>${tr("site.ctaTitle")}</h2>
<p class="intro">${tr("site.ctaBody")}</p>
<div class="actions"><a class="button primary" href="${DOWNLOAD}">${icon("apple", 18)}${tr("site.download")}</a><a class="button" href="${REPO}#install">${tr("site.ctaOther")}</a><a class="button" href="/#phone">${tr("site.ctaPhone")}</a></div>
</section>`,
    true,
  );
}

function legal(locale: Locale, name: "privacy" | "terms"): string {
  const version = locale.startsWith("zh") ? "zh-CN" : LEGAL_LOCALES.find((l) => l === locale) ?? "en";
  const note = version === locale || (locale === "zh-TW" && version === "zh-CN") ? "" : `<p class="note">${escape(t(locale, "site.legalLanguage"))}</p>`;
  const body = file(`${name}.${version}.html`);
  const title = /<h1>(.*?)<\/h1>/.exec(body)?.[1] ?? "Duolo";
  return page(locale, `${title} — Duolo`, note + body);
}

function support(locale: Locale): string {
  const tr = (key: string) => escape(t(locale, key));
  return page(
    locale,
    `${t(locale, "site.support")} — Duolo`,
    `<h1>${tr("site.support")}</h1>
<p>${tr("site.supportBody")}</p>
<p><a class="button primary" href="${REPO}/issues">${tr("site.supportIssues")}</a></p>`,
  );
}

function html(reply: FastifyReply, text: string, status = 200) {
  return reply
    .status(status)
    .header("content-type", "text/html; charset=utf-8")
    .header("cache-control", "public, max-age=300")
    .header("content-security-policy", "default-src 'none'; img-src 'self'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'")
    .header("x-content-type-options", "nosniff")
    .header("vary", "accept-language")
    .send(text);
}

export function registerSite(app: FastifyInstance) {
  app.get("/", (request, reply) => html(reply, home(localeOf(request))));
  app.get("/privacy", (request, reply) => html(reply, legal(localeOf(request), "privacy")));
  app.get("/terms", (request, reply) => html(reply, legal(localeOf(request), "terms")));
  app.get("/support", (request, reply) => html(reply, support(localeOf(request))));
  for (const name of ["icon.png", "favicon.png", "shots/desktop.png", "shots/phone.png"]) {
    app.get(`/${name}`, (_request, reply) =>
      reply.header("content-type", "image/png").header("cache-control", "public, max-age=86400").send(readFileSync(`${dir}/${name}`)),
    );
  }
  // Google Search Console 的所有权验证文件（Google Play 开发者账号要验证组织网站），不要删。
  for (const name of readdirSync(dir).filter((f) => /^google[0-9a-f]+\.html$/.test(f))) {
    app.get(`/${name}`, (_request, reply) => reply.header("content-type", "text/html; charset=utf-8").send(file(name)));
  }
  for (const { id } of AGENTS) {
    app.get(`/agents/${id}.svg`, (_request, reply) =>
      reply.header("content-type", "image/svg+xml").header("cache-control", "public, max-age=86400").send(file(`agents/${id}.svg`)),
    );
  }
  // 别的地址：接口（/v1/…）照常回 JSON 的 404；浏览器来的给一个网页。
  app.setNotFoundHandler((request, reply) => {
    if (request.url.startsWith("/v1/") || !request.headers.accept?.includes("text/html")) {
      return reply.status(404).send({ error: "notFound" });
    }
    const locale = localeOf(request);
    return html(reply, page(locale, "Duolo", `<h1>404</h1><p>${escape(t(locale, "site.notFound"))}</p><p><a href="/">duolo.io</a></p>`), 404);
  });
}

const STYLE = `
:root{color-scheme:dark;--bg:#08090a;--panel:#0c0d0f;--line:#1c1f23;--line2:#2a2e33;--fg:#eceef1;--muted:#a6acb4;--dim:#8a9099;--accent:#2bd27a;--accent-fg:#03130a;--ins:#2bd27a;--del:#ff7a7a;--wait:#f5b041;--run:#7cb2ff;--bad:#ff7a7a;
--sans:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,"PingFang SC","Hiragino Sans","Microsoft YaHei",sans-serif;--mono:ui-monospace,"SF Mono",Menlo,Consolas,"Liberation Mono",monospace}
*{box-sizing:border-box}
html{scroll-behavior:smooth}
body{margin:0;background-color:var(--bg);background-image:linear-gradient(rgba(255,255,255,.035) 1px,transparent 1px),linear-gradient(90deg,rgba(255,255,255,.035) 1px,transparent 1px);background-size:48px 48px;color:var(--fg);font:16px/1.6 var(--sans);-webkit-font-smoothing:antialiased;display:flex;flex-direction:column;min-height:100vh}
a{color:var(--accent);text-decoration:none}a:hover{color:#5be39a}
p{margin:0}
.wrap{max-width:1240px;width:100%;margin:0 auto;padding-left:24px;padding-right:24px}
.i{flex:none}.mono{font-family:var(--mono)}.dim{color:var(--dim)}.small{font-size:13px}
.ins{color:var(--ins);font-style:normal}.del{color:var(--del);font-style:normal}
.top{position:sticky;top:0;z-index:10;background:rgba(8,9,10,.86);backdrop-filter:saturate(160%) blur(12px);-webkit-backdrop-filter:saturate(160%) blur(12px);border-bottom:1px solid var(--line)}
.bar{display:flex;align-items:center;gap:28px;height:64px}
.brand{display:flex;align-items:center;gap:10px;color:var(--fg);font-weight:700;font-size:19px;letter-spacing:-.01em}.brand:hover{color:var(--fg)}.brand img{border-radius:7px}
.links{display:flex;gap:22px;font:13px var(--mono);text-transform:uppercase;letter-spacing:.06em}.links a,.ghost{color:var(--dim)}.links a:hover,.ghost:hover{color:var(--fg)}
.end{margin-left:auto;display:flex;align-items:center;gap:16px}.ghost{display:flex}
.button{display:inline-flex;align-items:center;gap:10px;min-height:52px;padding:0 22px;border-radius:10px;border:1px solid var(--line2);background:#0e1012;color:var(--fg);font-weight:550;font-size:16px;transition:transform .15s,filter .15s}
.button:hover{color:var(--fg);transform:translateY(-1px)}
.button.primary{background:var(--accent);border-color:var(--accent);color:var(--accent-fg);font-weight:650;box-shadow:0 8px 32px rgba(43,210,122,.22)}.button.primary:hover{color:var(--accent-fg);filter:brightness(1.06)}
.button.small{min-height:44px;padding:0 18px;font-size:15px;box-shadow:none}
.button.code{font:15px var(--mono);font-weight:400}
main{flex:1}
.doc{max-width:760px;padding-top:56px;padding-bottom:80px}.doc p{margin:0 0 14px}
.doc h1{font-size:34px;line-height:1.25;margin:0 0 12px;letter-spacing:-.01em}
.doc h2{font-size:21px;margin:36px 0 10px}.doc h3{font-size:17px;margin:24px 0 6px}.doc li{margin:4px 0}.doc .button{margin-top:8px}
.meta,.note{color:var(--dim);font-size:14px}
.hero{display:flex;flex-direction:column;align-items:center;text-align:center;gap:22px;padding-top:88px}
.status{display:inline-flex;align-items:center;gap:10px;padding:6px 14px;border:1px solid var(--line2);border-radius:6px;background:#0e1012;font:13px var(--mono);color:var(--muted)}
.dot{width:7px;height:7px;border-radius:50%;background:var(--accent);box-shadow:0 0 10px var(--accent)}
.hero h1{margin:0;max-width:1000px;font-size:clamp(44px,7.4vw,92px);line-height:1;letter-spacing:-.04em;font-weight:760}
.accent,.accentc{color:var(--accent)}
.lead{max-width:720px;color:var(--muted);font-size:clamp(17px,2vw,20px)}
.actions{display:flex;justify-content:center;gap:12px;flex-wrap:wrap}
.note.mono{font-size:13px}
.lanes{width:100%;margin-top:34px;border:1px solid #23272c;border-radius:14px;background:var(--panel);box-shadow:0 40px 120px rgba(0,0,0,.6);overflow:hidden;text-align:left}
.win-bar{display:flex;align-items:center;gap:8px;padding:12px 16px;border-bottom:1px solid var(--line);font:12px var(--mono);color:var(--dim)}
.win-bar span{width:11px;height:11px;border-radius:50%;background:#3a3f45}.win-bar em{font-style:normal;margin-left:12px}.win-bar .end,.panel-head .end{margin-left:auto}
.lane-grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(300px,1fr));gap:1px;background:var(--line)}
.lane{background:var(--panel);padding:18px 20px;display:flex;flex-direction:column;gap:8px;font:13px/1.7 var(--mono);color:#c9cdd3}
.lane-head{display:flex;align-items:center;gap:9px;font:14px var(--sans);color:var(--fg)}.lane-head b{flex:1;font-weight:600}
.pill{padding:2px 8px;border-radius:4px;font:12px var(--mono)}.pill.wait{background:rgba(245,176,65,.12);color:var(--wait)}.pill.run{background:rgba(124,178,255,.12);color:var(--run)}.pill.ok{background:rgba(43,210,122,.12);color:var(--accent)}
.ask{padding:10px 12px;border:1px solid #4a3a1c;border-radius:6px;background:#14110a;color:#e8d5b0}.ask span{color:var(--wait)}
.okbox{padding:10px 12px;border:1px solid #1e4d33;border-radius:6px;background:#0a1a11;color:#cfebdb}.okbox span{color:var(--accent)}
.cursor{animation:blink 1s steps(1) infinite}@keyframes blink{50%{opacity:0}}
.stats{width:100%;display:grid;grid-template-columns:repeat(auto-fit,minmax(170px,1fr));border:1px solid var(--line);border-radius:12px;background:#0b0c0e;text-align:left}
.stats div{padding:20px 24px;border-right:1px solid var(--line)}.stats div:last-child{border-right:0}
.stats b{display:block;font-size:34px;font-weight:750;letter-spacing:-.02em}.stats b small{font-size:20px;color:var(--dim);font-weight:500}
.stats span{font:12px var(--mono);color:var(--dim);text-transform:uppercase;letter-spacing:.06em}
.label{display:block;font:13px var(--mono);color:var(--accent);letter-spacing:.08em;text-transform:uppercase}
h2{margin:0;font-size:clamp(32px,4.6vw,54px);line-height:1.05;letter-spacing:-.03em;font-weight:750}
h3{margin:0;font-size:28px;line-height:1.15;letter-spacing:-.02em}
main>section{padding-top:110px}
.split{display:flex;flex-wrap:wrap;gap:56px;align-items:center}.split.flip{flex-direction:row-reverse}
.copy{flex:1 1 380px;display:flex;flex-direction:column;gap:18px}.copy p{color:var(--muted);font-size:18px}
.panel{flex:1 1 520px;border:1px solid #23272c;border-radius:14px;background:var(--panel);overflow:hidden;font-size:13px;line-height:1.6}
.panel.pad{padding:24px;display:flex;flex-direction:column;gap:14px}
.panel-head{display:flex;align-items:center;gap:10px;padding:12px 16px;border-bottom:1px solid var(--line);color:var(--dim)}
.panel-body{padding:16px;display:flex;flex-direction:column;gap:12px;color:#c9cdd3}
.warn{color:var(--wait)}.finding{display:flex;gap:12px}.p1{color:var(--bad)}.p2{color:var(--wait)}
.panel-actions{display:flex;gap:10px;flex-wrap:wrap;border-top:1px solid var(--line);padding-top:12px}
.fake{display:inline-flex;align-items:center;min-height:40px;padding:0 16px;border-radius:8px;border:1px solid var(--line2);font:500 14px var(--sans);color:var(--fg)}.fake.primary{background:var(--accent);border-color:var(--accent);color:var(--accent-fg);font-weight:600}
.who{display:flex;align-items:center;gap:12px}.who i{width:10px;height:10px;border-radius:50%}.who .you{background:var(--accent)}.who b{font-weight:500;color:var(--fg)}
.branch{margin-left:4px;padding-left:18px;border-left:1px dashed #3a3f45;display:flex;flex-direction:column;gap:14px}
.side{padding:12px;display:flex;flex-direction:column;gap:2px;font-size:14px}
.proj{padding:8px 12px;font-size:12px;color:var(--dim)}
.trow{display:flex;align-items:center;gap:10px;padding:9px 12px 9px 32px;border-radius:8px;color:#c9cdd3}.trow.on{background:#16191c;color:var(--fg)}.trow b{flex:1;font-weight:500}
.st{font:12px var(--mono)}.st.wait{color:var(--wait)}.st.run{color:var(--run)}.st.ok{color:var(--accent)}.st.bad{color:var(--bad)}
.side-note{margin-top:8px;padding:10px 12px;border-top:1px solid var(--line);font-size:12px;color:var(--dim)}
.wave{display:flex;align-items:center;gap:4px;height:40px}.wave i{width:4px;border-radius:2px;background:var(--accent)}.wave span{margin-left:14px;font-size:12px}
.heard{font-size:15px;color:var(--muted)}
.chips{display:flex;flex-wrap:wrap;gap:8px;font-size:12px}.chips span{padding:3px 9px;border-radius:4px;border:1px solid var(--line2);color:#c9cdd3}.chips em{font-style:normal;color:var(--dim);padding:3px 0}
.okbox.big{font-size:15px;color:var(--fg);padding:14px 16px}
.pair{display:grid;grid-template-columns:repeat(auto-fit,minmax(340px,1fr));gap:20px}.pair .panel p{color:var(--muted);font-size:16px}
.meters{margin-top:auto;display:flex;flex-direction:column;gap:10px;font-size:12px}.meters p{display:flex;align-items:center;gap:10px;font-size:12px}
.meters span{width:78px;color:var(--muted)}.meters i{flex:1;height:6px;border-radius:3px;background:var(--line);position:relative;overflow:hidden}.meters i.full{background:var(--bad)}
.meters i b{position:absolute;inset:0 auto 0 0;background:var(--accent);border-radius:3px}.meters em{width:44px;text-align:right;font-style:normal;color:var(--fg)}.meters em.bad{color:var(--bad)}
.log{margin-top:auto;padding:14px 16px;border-radius:8px;background:var(--bg);border:1px solid var(--line);font-size:12px;line-height:1.8}.log p{color:var(--muted);font-size:12px}.okc{color:var(--accent)}
.relay{display:flex;flex-wrap:wrap;align-items:center;gap:10px;padding:16px;border:1px solid #23272c;border-radius:10px;background:var(--panel);font-size:12px;color:var(--muted)}
.relay span{padding:6px 10px;border:1px solid var(--line2);border-radius:6px;color:var(--fg)}.relay .dash{border-style:dashed;color:var(--muted)}.relay em{font-style:normal;color:var(--accent)}.relay small{flex-basis:100%;color:var(--dim);font-size:12px}
.phone{flex:1 1 320px;display:flex;justify-content:center}.phone img{width:290px;height:auto;border:9px solid #1c1f23;border-radius:46px;background:#fff;box-shadow:0 40px 100px rgba(0,0,0,.6)}
.real{display:flex;flex-direction:column;gap:20px}.frame{border:1px solid #23272c;border-radius:14px;padding:8px;background:var(--panel)}.frame img{display:block;width:100%;height:auto;border-radius:8px}
.compare{display:flex;flex-direction:column;gap:24px}.intro{max-width:780px;color:var(--muted);font-size:18px}
.table{overflow-x:auto;border:1px solid #23272c;border-radius:14px;background:var(--panel)}
table{width:100%;min-width:820px;border-collapse:collapse;font-size:15px}
th{padding:16px 20px;text-align:left;font:600 13px var(--mono);text-transform:uppercase;letter-spacing:.06em;color:var(--fg)}th.us{color:var(--accent)}
td{padding:14px 20px;border-top:1px solid var(--line);color:var(--muted)}td:first-child{color:var(--fg)}td.yes{color:var(--accent)}td.them{color:var(--fg)}td.na{color:var(--dim)}
.term{font-size:14px;line-height:2;overflow-x:auto;white-space:nowrap;background:#060708}
.cta{display:flex;flex-direction:column;align-items:center;text-align:center;gap:24px;padding-bottom:120px}.cta img{border-radius:16px;box-shadow:0 0 60px rgba(43,210,122,.25)}
.foot{border-top:1px solid var(--line);background:var(--bg)}
.foot-bar{display:flex;flex-wrap:wrap;gap:22px;align-items:center;padding-top:28px;padding-bottom:28px;font:12px var(--mono);color:var(--dim)}
.foot-bar .copy{flex:1;display:block;font-size:12px}.foot-bar>a{color:var(--muted)}.foot-bar>a:hover{color:var(--fg)}
.lang{position:relative}.lang summary{list-style:none;cursor:pointer;padding:6px 10px;border:1px solid #23272c;border-radius:6px;color:var(--muted)}.lang summary::-webkit-details-marker{display:none}
.lang div{position:absolute;right:0;bottom:calc(100% + 6px);display:grid;grid-template-columns:1fr 1fr;gap:2px;padding:8px;min-width:320px;border:1px solid var(--line2);border-radius:10px;background:#0e1012;box-shadow:0 20px 60px rgba(0,0,0,.6)}
.lang a{padding:7px 10px;border-radius:6px;color:var(--muted);font-family:var(--sans);font-size:13px}.lang a:hover,.lang a[aria-current]{background:#16191c;color:var(--fg)}
@media (max-width:820px){.links{display:none}.split.flip{flex-direction:row}.hero{padding-top:56px}main>section{padding-top:80px}.stats div{border-right:0;border-bottom:1px solid var(--line)}.lang div{right:auto;left:0;min-width:280px}}
@media (prefers-reduced-motion:reduce){.cursor{animation:none}.button{transition:none}}
`;
