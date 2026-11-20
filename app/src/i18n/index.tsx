// 多语言：界面上的所有文字都通过 t() 取，不在组件里写死。
// 规范见 docs/i18n.md。新增语言：在 locales/ 加一个文件，在 LOCALES 里登记，
// 并在 crates/duolo-app/src/locale.rs 补上菜单文案。

import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";

import { en, type MessageKey, type Messages } from "./locales/en";
import { zhCN } from "./locales/zh-CN";
import { zhTW } from "./locales/zh-TW";
import { ja } from "./locales/ja";
import { ko } from "./locales/ko";
import { es } from "./locales/es";
import { fr } from "./locales/fr";
import { de } from "./locales/de";
import { ptBR } from "./locales/pt-BR";
import { ru } from "./locales/ru";
import { it } from "./locales/it";
import { tr } from "./locales/tr";
import { vi } from "./locales/vi";
import { id } from "./locales/id";

export type { MessageKey } from "./locales/en";

/** 支持的语言。`name` 用该语言自己的写法，在语言菜单里显示。 */
export const LOCALES = [
  { code: "en", name: "English", messages: en },
  { code: "zh-CN", name: "简体中文", messages: zhCN },
  { code: "zh-TW", name: "繁體中文", messages: zhTW },
  { code: "ja", name: "日本語", messages: ja },
  { code: "ko", name: "한국어", messages: ko },
  { code: "es", name: "Español", messages: es },
  { code: "fr", name: "Français", messages: fr },
  { code: "de", name: "Deutsch", messages: de },
  { code: "pt-BR", name: "Português (Brasil)", messages: ptBR },
  { code: "ru", name: "Русский", messages: ru },
  { code: "it", name: "Italiano", messages: it },
  { code: "tr", name: "Türkçe", messages: tr },
  { code: "vi", name: "Tiếng Việt", messages: vi },
  { code: "id", name: "Bahasa Indonesia", messages: id },
] as const satisfies readonly { code: string; name: string; messages: Messages }[];

export type Locale = (typeof LOCALES)[number]["code"];
/** 用户的选择：跟随系统，或者某种语言。 */
export type LanguagePref = "auto" | Locale;

const isLocale = (code: string): code is Locale => LOCALES.some((l) => l.code === code);

/** 把系统语言（如 `zh-Hans-CN`、`pt-PT`、`en-GB`）对应到支持的语言，找不到时用英文。 */
export function matchLocale(tags: readonly string[]): Locale {
  for (const raw of tags) {
    const tag = raw.toLowerCase();
    if (tag.startsWith("zh")) {
      return /hant|tw|hk|mo/.test(tag) ? "zh-TW" : "zh-CN";
    }
    if (tag.startsWith("pt")) return "pt-BR";
    const base = tag.split("-")[0];
    const hit = LOCALES.find((l) => l.code.toLowerCase() === base);
    if (hit) return hit.code;
  }
  return "en";
}

const browserLanguages = () => (navigator.languages?.length ? navigator.languages : [navigator.language]);

export type Translate = (key: MessageKey, vars?: Record<string, string | number>) => string;

function translator(locale: Locale): Translate {
  const messages = LOCALES.find((l) => l.code === locale)?.messages ?? en;
  return (key, vars) => {
    const text = messages[key] ?? en[key] ?? key;
    return vars ? text.replace(/\{(\w+)\}/g, (m, name: string) => (name in vars ? String(vars[name]) : m)) : text;
  };
}

interface I18n {
  locale: Locale;
  pref: LanguagePref;
  t: Translate;
  setPref(pref: LanguagePref): void;
}

const Context = createContext<I18n>({ locale: "en", pref: "auto", t: translator("en"), setPref: () => {} });

export function I18nProvider({ children }: { children: ReactNode }) {
  const [pref, setPrefState] = useState<LanguagePref>("auto");
  const [system, setSystem] = useState<Locale>(() => matchLocale(browserLanguages()));

  // 读取保存的选择（~/.duolo/settings.json）和系统语言。
  useEffect(() => {
    invoke<string>("get_language")
      .then((saved) => setPrefState(saved === "auto" || isLocale(saved) ? (saved as LanguagePref) : "auto"))
      .catch(() => {});
    invoke<string[]>("system_languages")
      .then((tags) => tags.length > 0 && setSystem(matchLocale(tags)))
      .catch(() => {});
  }, []);

  const locale: Locale = pref === "auto" ? system : pref;

  // 菜单和审阅结论跟着界面语言走。
  useEffect(() => {
    document.documentElement.lang = locale;
    invoke("apply_locale", { locale }).catch(() => {});
  }, [locale]);

  const setPref = useCallback((next: LanguagePref) => {
    setPrefState(next);
    invoke("set_language", { language: next }).catch(() => {});
  }, []);

  const value = useMemo(() => ({ locale, pref, t: translator(locale), setPref }), [locale, pref, setPref]);
  return <Context.Provider value={value}>{children}</Context.Provider>;
}

export const useI18n = () => useContext(Context);
export const useT = () => useContext(Context).t;

/** 后端命令的错误：有错误码时按当前语言显示，否则显示通用文案和英文详情。 */
export function errorMessage(t: Translate, err: unknown): string {
  if (err && typeof err === "object" && "code" in err) {
    const { code, params, message } = err as { code: string; params?: Record<string, string>; message?: string };
    const key = `error.${code}` as MessageKey;
    if (code !== "internal" && key in en) return t(key, params);
    return t("error.internal", { message: message ?? "" });
  }
  // 前端自己给出的提示已经是当前语言的句子，原样显示。
  if (typeof err === "string") return err;
  return t("error.internal", { message: String(err) });
}
