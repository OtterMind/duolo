// duolo.io 的几个网页：首页（介绍和下载）、隐私政策、使用条款、支持。App Store、Google Play 上架要用到后三个。
//
// 页面按 Accept-Language 选语言，`?lang=zh-CN` 可以指定。首页和支持页的文案在 locales/*.json（site.*，14 种语言）；
// 隐私政策和使用条款是法律文本，只有英文和简体中文两份（site/*.html），别的语言显示英文版并说明。

import { readFileSync } from "node:fs";
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

function page(locale: Locale, title: string, body: string, wide = false): string {
  const tr = (key: string) => escape(t(locale, key));
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
<link rel="icon" type="image/png" href="/favicon.png">
<style>${STYLE}</style>
</head>
<body>
<header class="top"><div class="wrap bar">
<a class="brand" href="/"><img src="/icon.png" alt="" width="26" height="26">Duolo</a>
<nav class="links"><a href="/#features">${tr("site.navFeatures")}</a><a href="/#faq">${tr("site.navFaq")}</a><a href="${REPO}#readme">${tr("site.navDocs")}</a><a href="${REPO}/releases">${tr("site.changelog")}</a></nav>
<nav class="end"><a class="ghost" href="${REPO}" aria-label="GitHub">${icon("github", 20)}</a><a class="button primary small" href="${DOWNLOAD}">${tr("site.navDownload")}</a></nav>
</div></header>
<main class="${wide ? "home" : "wrap doc"}">${body}</main>
<footer class="foot"><div class="wrap">
<div class="foot-grid">
<div class="foot-brand"><a class="brand" href="/"><img src="/icon.png" alt="" width="24" height="24">Duolo</a><p>${tr("site.footerTagline")}</p></div>
<div><h4>${tr("site.footerProduct")}</h4><a href="${DOWNLOAD}">${tr("site.navDownload")}</a><a href="${REPO}/releases">${tr("site.changelog")}</a><a href="${REPO}#readme">${tr("site.navDocs")}</a></div>
<div><h4>${tr("site.footerCommunity")}</h4><a href="${REPO}">GitHub</a><a href="${REPO}/issues">${tr("site.issues")}</a><a href="/support">${tr("site.support")}</a></div>
<div><h4>${tr("site.footerLegal")}</h4><a href="/privacy">${tr("site.privacy")}</a><a href="/terms">${tr("site.terms")}</a></div>
</div>
<p class="copy">© 2026 Aita Technology (Hangzhou) Co., Ltd.</p>
</div></footer>
</body>
</html>`;
}

/** 首屏的应用截图：用 HTML 画的 Duolo 窗口（侧边栏、Claude Code 终端、审阅结果），和真实界面的布局、配色一致。 */
const MOCKUP = `<div class="shot" aria-hidden="true">
<div class="win-bar"><span></span><span></span><span></span></div>
<div class="win">
<aside class="side">
<div class="search">Search tasks <kbd>⌘K</kbd></div>
<div class="proj">▾ shop</div>
<div class="row on">${agentIcon("claude", "#d97757", 14)}<div><b>Export orders as CSV</b><small>feat/order-csv <i class="ins">+48</i> <i class="del">−6</i></small></div><span class="st ok">✓</span></div>
<div class="row">${agentIcon("claude", "#d97757", 14)}<div><b>Stripe billing</b><small>feat/stripe-billing</small></div><span class="st run"></span></div>
<div class="row">${agentIcon("codex", "#cfcfcf", 14)}<div><b>Fix login styles</b><small>fix/login-styles</small></div><span class="st wait">!</span></div>
<div class="row">${agentIcon("opencode", "#cfcfcf", 14)}<div><b>Paginate the API</b><small>feat/cursor-paging</small></div><span class="st run"></span></div>
<div class="proj">▾ docs</div>
<div class="row">${agentIcon("pi", "#cfcfcf", 14)}<div><b>Update the README</b><small>docs/readme</small></div><span class="st done"></span></div>
</aside>
<div class="pane">
<div class="pane-head"><b>Export orders as CSV</b><small>feat/order-csv <i class="ins">+48</i> <i class="del">−6</i></small><span class="pill">Review ✓</span></div>
<div class="term">
<p><i class="c">⏺</i> I added an <i class="k">Export</i> button to the orders page and a <i class="k">GET /orders.csv</i> endpoint.</p>
<p class="dim">  ⎿ Edited src/routes/orders.ts (+31 −2)</p>
<p class="dim">  ⎿ Edited src/pages/Orders.tsx (+17 −4)</p>
<p><i class="c">⏺</i> Ran <i class="k">pnpm test</i>: 42 passed.</p>
<div class="review"><div class="review-title">${agentIcon("codex", "#cfcfcf", 13)} Codex review · no issues found</div><p>Amounts are written in integer cents and the date filter is applied on the server. Looks good to merge.</p></div>
</div>
<div class="input">❯ Also let me filter by customer<span class="mic">${icon("voice", 14)}</span></div>
</div>
</div>
</div>`;

function home(locale: Locale): string {
  const tr = (key: string) => escape(t(locale, key));
  const feature = (name: keyof typeof ICON, title: string, body: string) =>
    `<div class="card">${icon(name, 22)}<h3>${tr(title)}</h3><p>${tr(body)}</p></div>`;
  const step = (n: number, title: string, body: string) => `<div class="step"><span class="num">${n}</span><h3>${tr(title)}</h3><p>${tr(body)}</p></div>`;
  const faq = (q: string, a: string) => `<details><summary>${tr(q)}</summary><p>${tr(a)}</p></details>`;
  const download = `<div class="actions"><a class="button primary" href="${DOWNLOAD}">${icon("apple", 18)}${tr("site.download")}</a><a class="button" href="${REPO}">${icon("github", 18)}${tr("site.github")}</a></div>
<p class="note">${tr("site.downloadNote")}</p>`;
  return page(
    locale,
    `Duolo — ${t(locale, "site.tagline")}`,
    `<section class="hero wrap">
<a class="badge" href="${DOWNLOAD}">${tr("site.badge")} →</a>
<h1>${tr("site.tagline")}</h1>
<p class="lead">${tr("site.intro")}</p>
${download}
${MOCKUP}
</section>
<section class="agents wrap">
<p class="eyebrow">${tr("site.agentsTitle")}</p>
<div class="agent-list">${AGENTS.map((a) => `<span class="agent-chip">${agentIcon(a.id, a.color)}${a.name}</span>`).join("")}</div>
<p class="note">${tr("site.agentsMore")}</p>
</section>
<section class="features wrap" id="features">
<h2>${tr("site.featuresTitle")}</h2>
<p class="sub">${tr("site.featuresIntro")}</p>
<div class="cards">
${feature("parallel", "site.parallelTitle", "site.parallelBody")}
${feature("review", "site.reviewTitle", "site.reviewBody")}
${feature("chat", "site.chatTitle", "site.chatBody")}
${feature("handoff", "site.handoffTitle", "site.handoffBody")}
${feature("grid", "site.gridTitle", "site.gridBody")}
${feature("voice", "site.voiceTitle", "site.voiceBody")}
${feature("clock", "site.autoTitle", "site.autoBody")}
${feature("accounts", "site.accountsTitle", "site.accountsBody")}
${feature("terminal", "site.cliTitle", "site.cliBody")}
</div>
</section>
<section class="steps-wrap wrap">
<h2>${tr("site.stepsTitle")}</h2>
<div class="steps">${step(1, "site.step1Title", "site.step1Body")}${step(2, "site.step2Title", "site.step2Body")}${step(3, "site.step3Title", "site.step3Body")}</div>
</section>
<section class="mobile wrap"><div class="mobile-card">${icon("phone", 28)}<div><h2>${tr("site.mobileTitle")}</h2><p>${tr("site.mobileBody")}</p></div></div></section>
<section class="faq wrap" id="faq">
<h2>${tr("site.faqTitle")}</h2>
${faq("site.q1", "site.a1")}${faq("site.q2", "site.a2")}${faq("site.q3", "site.a3")}${faq("site.q4", "site.a4")}${faq("site.q5", "site.a5")}${faq("site.q6", "site.a6")}
</section>
<section class="cta wrap"><h2>${tr("site.ctaTitle")}</h2><p class="sub">${tr("site.ctaBody")}</p>${download}</section>`,
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
  for (const name of ["icon.png", "favicon.png"]) {
    app.get(`/${name}`, (_request, reply) =>
      reply.header("content-type", "image/png").header("cache-control", "public, max-age=86400").send(readFileSync(`${dir}/${name}`)),
    );
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
:root{--bg:#0f0f10;--bg2:#141415;--fg:#ececec;--muted:#a1a1a6;--dim:#6e6e73;--line:#26262a;--card:#18181a;--accent:#d97757;--accent-fg:#fff;--ins:#8fc286;--del:#e0716c;--need:#e0a84a;--run:#7aa2d6}
@media (prefers-color-scheme:light){:root{--bg:#fff;--bg2:#f7f7f8;--fg:#1d1d1f;--muted:#5f5f66;--dim:#8a8a90;--line:#e6e6e9;--card:#f7f7f9}}
*{box-sizing:border-box}
html{scroll-behavior:smooth}
body{margin:0;background:var(--bg);color:var(--fg);font:16px/1.6 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,"PingFang SC","Hiragino Sans","Microsoft YaHei",sans-serif;-webkit-font-smoothing:antialiased;display:flex;flex-direction:column;min-height:100vh}
a{color:var(--accent);text-decoration:none}a:hover{text-decoration:underline}
.wrap{max-width:1120px;width:100%;margin:0 auto;padding-left:24px;padding-right:24px}
.i{flex:none}
.top{position:sticky;top:0;z-index:10;background:color-mix(in srgb,var(--bg) 82%,transparent);backdrop-filter:saturate(180%) blur(14px);-webkit-backdrop-filter:saturate(180%) blur(14px);border-bottom:1px solid var(--line)}
.bar{display:flex;align-items:center;gap:28px;height:60px}
.brand{display:flex;align-items:center;gap:9px;color:var(--fg);font-weight:650;font-size:17px}.brand:hover{text-decoration:none}.brand img{border-radius:6px}
.links{display:flex;gap:22px}.links a,.ghost{color:var(--muted);font-size:14.5px}.links a:hover,.ghost:hover{color:var(--fg);text-decoration:none}
.end{margin-left:auto;display:flex;align-items:center;gap:16px}.ghost{display:flex}
.button{display:inline-flex;align-items:center;gap:9px;padding:12px 22px;border-radius:11px;border:1px solid var(--line);background:var(--card);color:var(--fg);font-weight:550;font-size:15.5px;transition:transform .15s,filter .15s}
.button:hover{text-decoration:none;transform:translateY(-1px)}
.button.primary{background:var(--accent);border-color:var(--accent);color:var(--accent-fg)}.button.primary:hover{filter:brightness(1.07)}
.button.small{padding:7px 14px;font-size:14px;border-radius:9px}
main{flex:1}
.doc{max-width:760px;padding-top:56px;padding-bottom:80px}
.doc h1{font-size:34px;line-height:1.25;margin:0 0 8px;letter-spacing:-.01em}
.doc h2{font-size:21px;margin:36px 0 10px}.doc h3{font-size:17px;margin:24px 0 6px}.doc li{margin:4px 0}
.meta,.note{color:var(--dim);font-size:14px}
h2{font-size:clamp(26px,3.6vw,38px);line-height:1.2;letter-spacing:-.02em;margin:0 0 12px;text-align:center}
.sub{color:var(--muted);text-align:center;max-width:640px;margin:0 auto 40px;font-size:17px}
main>section{padding-top:88px}
.hero{text-align:center;padding-top:72px;position:relative}
.hero::before{content:"";position:absolute;inset:-40px 0 auto;height:520px;background:radial-gradient(60% 60% at 50% 30%,color-mix(in srgb,var(--accent) 22%,transparent),transparent 70%);z-index:-1;pointer-events:none}
.badge{display:inline-block;padding:6px 14px;border:1px solid var(--line);border-radius:999px;color:var(--muted);font-size:13.5px;background:var(--card)}.badge:hover{text-decoration:none;color:var(--fg)}
.hero h1{font-size:clamp(40px,7vw,76px);line-height:1.05;letter-spacing:-.035em;margin:22px 0 20px;font-weight:750}
.lead{max-width:700px;margin:0 auto;color:var(--muted);font-size:clamp(17px,2vw,20px)}
.actions{display:flex;justify-content:center;gap:12px;flex-wrap:wrap;margin:34px 0 12px}
.shot{margin:56px auto 0;max-width:1040px;border:1px solid #2c2c30;border-radius:14px;overflow:hidden;background:#161616;box-shadow:0 30px 80px -20px rgba(0,0,0,.55),0 0 0 1px rgba(255,255,255,.03);text-align:left;color:#d6d6d6;font-size:13px}
.win-bar{height:34px;display:flex;align-items:center;gap:8px;padding:0 14px;background:#1b1b1b;border-bottom:1px solid #2a2a2a}.win-bar span{width:12px;height:12px;border-radius:50%;background:#3a3a3a}.win-bar span:nth-child(1){background:#ff5f57}.win-bar span:nth-child(2){background:#febc2e}.win-bar span:nth-child(3){background:#28c840}
.win{display:grid;grid-template-columns:250px 1fr;min-height:400px}
.side{background:#1b1b1b;border-right:1px solid #2a2a2a;padding:12px 10px}
.search{display:flex;justify-content:space-between;padding:7px 10px;border-radius:8px;background:#232323;color:#767676;margin-bottom:12px}.search kbd{font:inherit;font-size:11px}
.proj{color:#8a8a8a;font-size:12px;padding:8px 8px 4px}
.row{display:flex;align-items:center;gap:9px;padding:7px 8px;border-radius:8px}.row.on{background:#2a2a2a}
.row div{flex:1;min-width:0}.row b{display:block;font-weight:500;color:#e4e4e4;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}.row small{display:block;color:#767676;font-size:11.5px}
.ins{color:var(--ins);font-style:normal}.del{color:var(--del);font-style:normal}
.st{width:16px;height:16px;border-radius:50%;flex:none;display:grid;place-items:center;font-size:10px;color:#fff;font-weight:700}
.st.ok{background:#3f8f4f}.st.wait{background:var(--need)}.st.done{background:#555}
.st.run{border:2px solid #2d3d55;border-top-color:var(--run);animation:spin 1s linear infinite}
@keyframes spin{to{transform:rotate(360deg)}}
.pane{display:flex;flex-direction:column;min-width:0}
.pane-head{display:flex;align-items:center;flex-wrap:wrap;gap:4px 10px;padding:10px 16px;border-bottom:1px solid #2a2a2a}.pane-head b{font-weight:550;color:#e4e4e4}.pane-head small{color:#767676}
.pill{margin-left:auto;font-size:11.5px;padding:3px 9px;border-radius:999px;background:#22372a;color:#8fc286}
.term{flex:1;padding:16px 20px;font:12.5px/1.75 Menlo,"SF Mono",Consolas,monospace}.term p{margin:0}
.term .c{color:#d97757;font-style:normal}.term .k{color:#e8c08a;font-style:normal}.term .dim{color:#767676}
.review{margin:14px 0 4px;padding:10px 14px;border:1px solid #2f3b31;background:#18201a;border-radius:10px;font-family:-apple-system,BlinkMacSystemFont,sans-serif;font-size:13px}
.review-title{display:flex;align-items:center;gap:7px;color:#8fc286;font-weight:550;margin-bottom:4px}.review p{color:#bdbdbd}
.input{margin:0 16px 16px;padding:11px 14px;border-radius:10px;background:#232323;font:12.5px Menlo,"SF Mono",monospace;display:flex;align-items:center;color:#d6d6d6}.mic{margin-left:auto;color:#767676;display:flex}
.agent{display:inline-block;flex:none}
.agents{text-align:center}.eyebrow{color:var(--dim);font-size:14px;text-transform:uppercase;letter-spacing:.08em;margin:0 0 18px}
.agent-list{display:flex;justify-content:center;flex-wrap:wrap;gap:12px}
.agent-chip{display:inline-flex;align-items:center;gap:9px;padding:10px 18px;border:1px solid var(--line);border-radius:12px;background:var(--card);font-weight:550}
.agents .note{margin-top:14px}
.cards{display:grid;grid-template-columns:repeat(3,1fr);gap:14px}
.card{background:var(--card);border:1px solid var(--line);border-radius:16px;padding:22px 22px 20px;transition:border-color .2s}
.card:hover{border-color:color-mix(in srgb,var(--accent) 45%,var(--line))}
.card .i{color:var(--accent)}.card h3{margin:14px 0 6px;font-size:16.5px}.card p{margin:0;color:var(--muted);font-size:14.5px}
.steps{display:grid;grid-template-columns:repeat(auto-fit,minmax(240px,1fr));gap:28px;margin-top:36px}
.step{text-align:center}.num{display:inline-grid;place-items:center;width:38px;height:38px;border-radius:50%;background:color-mix(in srgb,var(--accent) 16%,transparent);color:var(--accent);font-weight:700}
.step h3{margin:14px 0 6px;font-size:18px}.step p{margin:0;color:var(--muted);font-size:15px}
.mobile-card{display:flex;gap:22px;align-items:center;padding:30px 34px;border:1px dashed var(--line);border-radius:18px;background:var(--bg2)}
.mobile-card .i{color:var(--accent)}.mobile-card h2{text-align:left;font-size:24px;margin:0 0 6px}.mobile-card p{margin:0;color:var(--muted)}
.faq{max-width:800px}.faq h2{margin-bottom:28px}
details{border-bottom:1px solid var(--line);padding:18px 4px}
summary{cursor:pointer;font-weight:600;font-size:17px;list-style:none;display:flex;justify-content:space-between;gap:16px}summary::-webkit-details-marker{display:none}
summary::after{content:"+";color:var(--dim);font-weight:400;font-size:22px;line-height:1}details[open] summary::after{content:"−"}
details p{margin:10px 0 0;color:var(--muted)}
.cta{text-align:center;padding-bottom:96px}.cta .sub{margin-bottom:0}
.foot{border-top:1px solid var(--line);padding:48px 0 32px;font-size:14px}
.foot-grid{display:grid;grid-template-columns:2fr 1fr 1fr 1fr;gap:28px}
.foot-brand p{color:var(--muted);margin:12px 0 0;max-width:280px}
.foot h4{margin:0 0 12px;font-size:13px;color:var(--dim);font-weight:600;text-transform:uppercase;letter-spacing:.06em}
.foot-grid a:not(.brand){display:block;color:var(--muted);margin:7px 0}.foot-grid a:hover{color:var(--fg)}
.copy{color:var(--dim);margin:36px 0 0;font-size:13px}
@media (max-width:980px){.cards{grid-template-columns:repeat(2,1fr)}}
@media (max-width:820px){.cards{grid-template-columns:1fr}.links{display:none}.win{grid-template-columns:1fr}.side{display:none}.foot-grid{grid-template-columns:1fr 1fr}.foot-brand{grid-column:1/-1}.mobile-card{flex-direction:column;align-items:flex-start}}
@media (prefers-reduced-motion:reduce){.st.run{animation:none}.button{transition:none}}
`;
