// duolo.io 的网页：按语言显示、法律文本只有英文和简体中文、接口的 404 还是 JSON。

import { readFileSync } from "node:fs";

import Fastify from "fastify";
import { describe, expect, it } from "vitest";

import { registerSite } from "../src/site.js";

const app = Fastify();
registerSite(app);

const get = (url: string, headers: Record<string, string> = {}) => app.inject({ method: "GET", url, headers });

describe("site", () => {
  it("shows the home page in the visitor's language", async () => {
    const en = await get("/");
    expect(en.statusCode).toBe(200);
    expect(en.headers["content-type"]).toContain("text/html");
    expect(en.body).toContain("Download for Mac");
    expect((await get("/", { "accept-language": "ja-JP,ja;q=0.9" })).body).toContain("Mac 版をダウンロード");
    expect((await get("/?lang=zh-CN", { "accept-language": "en" })).body).toContain("下载 Mac 版");
  });

  it("serves the privacy policy and terms in English and Simplified Chinese", async () => {
    expect((await get("/privacy")).body).toContain("<h1>Privacy Policy</h1>");
    expect((await get("/privacy", { "accept-language": "zh-CN" })).body).toContain("<h1>隐私政策</h1>");
    expect((await get("/terms", { "accept-language": "zh-TW" })).body).toContain("<h1>使用条款</h1>");
    // 没有译本的语言：英文版，并说明有哪些语言。
    const fr = (await get("/privacy", { "accept-language": "fr" })).body;
    expect(fr).toContain("<h1>Privacy Policy</h1>");
    expect(fr).toContain("anglais et en chinois simplifié");
  });

  it("serves the support page and icons", async () => {
    expect((await get("/support")).body).toContain("https://github.com/OtterMind/duolo/issues");
    const icon = await get("/icon.png");
    expect(icon.statusCode).toBe(200);
    expect(icon.headers["content-type"]).toBe("image/png");
  });

  it("uses the same agent icons as the plugins", async () => {
    for (const id of ["claude", "codex", "opencode", "pi", "grok"]) {
      const res = await get(`/agents/${id}.svg`);
      expect(res.headers["content-type"]).toBe("image/svg+xml");
      expect(res.body).toBe(readFileSync(new URL(`../../crates/duolo-agent/agents/${id}/icon.svg`, import.meta.url), "utf8"));
    }
  });

  it("keeps JSON 404s for the API and shows a page to browsers", async () => {
    const api = await get("/v1/nothing", { accept: "text/html" });
    expect(api.statusCode).toBe(404);
    expect(api.json()).toEqual({ error: "notFound" });
    const page = await get("/nothing", { accept: "text/html" });
    expect(page.statusCode).toBe(404);
    expect(page.body).toContain("Page not found.");
  });
});
