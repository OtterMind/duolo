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

function page(locale: Locale, title: string, body: string, wide = false): string {
  const tr = (key: string) => escape(t(locale, key));
  return `<!doctype html>
<html lang="${locale}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${escape(title)}</title>
<meta name="description" content="${tr("site.description")}">
<link rel="icon" type="image/png" href="/favicon.png">
<style>${STYLE}</style>
</head>
<body>
<header class="bar"><a class="brand" href="/"><img src="/icon.png" alt="" width="28" height="28">Duolo</a>
<nav><a href="${REPO}">GitHub</a></nav></header>
<main class="${wide ? "home" : "doc"}">${body}</main>
<footer class="bar">
<span>© 2026 Aita Technology (Hangzhou) Co., Ltd.</span>
<nav><a href="/privacy">${tr("site.privacy")}</a><a href="/terms">${tr("site.terms")}</a><a href="/support">${tr("site.support")}</a></nav>
</footer>
</body>
</html>`;
}

function home(locale: Locale): string {
  const tr = (key: string) => escape(t(locale, key));
  const feature = (title: string, body: string) => `<div class="card"><h3>${tr(title)}</h3><p>${tr(body)}</p></div>`;
  return page(
    locale,
    `Duolo — ${t(locale, "site.tagline")}`,
    `<section class="hero">
<img src="/icon.png" alt="" width="96" height="96">
<h1>${tr("site.tagline")}</h1>
<p class="lead">${tr("site.intro")}</p>
<div class="actions"><a class="button primary" href="${DOWNLOAD}">${tr("site.download")}</a><a class="button" href="${REPO}">${tr("site.github")}</a></div>
<p class="note">${tr("site.downloadNote")}</p>
</section>
<section class="cards">${feature("site.parallelTitle", "site.parallelBody")}${feature("site.reviewTitle", "site.reviewBody")}${feature("site.localTitle", "site.localBody")}</section>`,
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
:root{--bg:#fff;--fg:#1d1d1f;--muted:#6e6e73;--line:#e5e5e7;--card:#f6f6f8;--accent:#d97757;--accent-fg:#fff}
@media (prefers-color-scheme:dark){:root{--bg:#161616;--fg:#e8e8e8;--muted:#a0a0a0;--line:#2c2c2c;--card:#1f1f1f}}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--fg);font:16px/1.65 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,"PingFang SC","Hiragino Sans","Microsoft YaHei",sans-serif;-webkit-font-smoothing:antialiased;display:flex;flex-direction:column;min-height:100vh}
a{color:var(--accent);text-decoration:none}a:hover{text-decoration:underline}
.bar{display:flex;align-items:center;justify-content:space-between;gap:16px;flex-wrap:wrap;max-width:1040px;width:100%;margin:0 auto;padding:18px 24px}
.bar nav{display:flex;gap:20px;flex-wrap:wrap}.bar nav a{color:var(--muted)}
header.bar{border-bottom:1px solid var(--line)}
footer.bar{border-top:1px solid var(--line);color:var(--muted);font-size:14px;margin-top:auto}
.brand{display:flex;align-items:center;gap:10px;color:var(--fg);font-weight:600;font-size:18px}.brand:hover{text-decoration:none}
main{width:100%;margin:0 auto;padding:48px 24px 72px}
.doc{max-width:760px}.home{max-width:1040px}
.doc h1{font-size:32px;line-height:1.25;margin:0 0 8px}
.doc h2{font-size:21px;margin:36px 0 10px}.doc h3{font-size:17px;margin:24px 0 6px}
.doc li{margin:4px 0}
.meta,.note{color:var(--muted);font-size:14px}
.hero{text-align:center;padding:32px 0 56px}
.hero img{border-radius:22px}
.hero h1{font-size:clamp(32px,6vw,52px);line-height:1.15;letter-spacing:-.02em;margin:24px 0 16px}
.lead{max-width:640px;margin:0 auto;color:var(--muted);font-size:18px}
.actions{display:flex;justify-content:center;gap:12px;flex-wrap:wrap;margin:32px 0 12px}
.button{display:inline-block;padding:11px 22px;border-radius:10px;border:1px solid var(--line);color:var(--fg);font-weight:500}
.button:hover{text-decoration:none;background:var(--card)}
.button.primary{background:var(--accent);border-color:var(--accent);color:var(--accent-fg)}
.button.primary:hover{filter:brightness(1.05);background:var(--accent)}
.cards{display:grid;grid-template-columns:repeat(auto-fit,minmax(240px,1fr));gap:16px}
.card{background:var(--card);border:1px solid var(--line);border-radius:14px;padding:22px 24px}
.card h3{margin:0 0 8px;font-size:17px}.card p{margin:0;color:var(--muted);font-size:15px}
`;
