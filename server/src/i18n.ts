// 发给用户看的文字（目前只有验证码邮件）按用户的语言显示。
// 文案和桌面端共用仓库根目录的 locales/*.json（规范见 docs/i18n.md），键是 email.*。

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const LOCALES = ["en", "zh-CN", "zh-TW", "ja", "ko", "es", "fr", "de", "pt-BR", "ru", "it", "tr", "vi", "id"] as const;
type Locale = (typeof LOCALES)[number];

const dir = process.env.DUOLO_LOCALES_DIR ?? fileURLToPath(new URL("../../locales/", import.meta.url));
const catalogs = new Map<Locale, Record<string, string>>();

function catalog(locale: Locale): Record<string, string> {
  let messages = catalogs.get(locale);
  if (!messages) {
    messages = JSON.parse(readFileSync(`${dir}/${locale}.json`, "utf8")) as Record<string, string>;
    catalogs.set(locale, messages);
  }
  return messages;
}

/** 从 Accept-Language 里挑一个支持的语言，规则和桌面端的 matchLocale 一致。 */
export function matchLocale(acceptLanguage: string | null | undefined): Locale {
  const tags = (acceptLanguage ?? "")
    .split(",")
    .map((part) => part.split(";")[0].trim().toLowerCase())
    .filter(Boolean);
  for (const tag of tags) {
    if (tag.startsWith("zh")) return /hant|tw|hk|mo/.test(tag) ? "zh-TW" : "zh-CN";
    if (tag.startsWith("pt")) return "pt-BR";
    const hit = LOCALES.find((l) => l.toLowerCase() === tag.split("-")[0]);
    if (hit) return hit;
  }
  return "en";
}

export function t(locale: Locale, key: string, vars: Record<string, string | number> = {}): string {
  const text = catalog(locale)[key] ?? catalog("en")[key] ?? key;
  return text.replace(/\{(\w+)\}/g, (m, name: string) => (name in vars ? String(vars[name]) : m));
}

export type { Locale };
