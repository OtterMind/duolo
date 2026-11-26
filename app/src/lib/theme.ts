// 外观：主题、强调色、终端字号。只是这台电脑上的偏好，存在本机（localStorage），启动时在显示界面之前应用，免得闪一下。
//
// 颜色都在 styles.css 的变量里，这里只设置 <html> 的 data-theme、data-accent。
// 终端的配色（xterm）不能用 CSS 变量，按主题在这里给；浅色主题里终端保持深色：
// Agent 自己的配色（diff 的底色、强调色）是按深色背景设计的，浅色背景上会看不清。

import type { ITheme } from "@xterm/xterm";

export type ThemeId = "dark" | "light" | "midnight" | "mocha" | "system";
export type AccentId = "blue" | "orange" | "green" | "purple" | "pink";

export interface Appearance {
  theme: ThemeId;
  accent: AccentId;
  /** 终端字号（像素）。 */
  fontSize: number;
}

export const THEMES: ThemeId[] = ["dark", "light", "midnight", "mocha", "system"];
export const ACCENTS: { id: AccentId; color: string }[] = [
  { id: "blue", color: "rgb(110, 155, 245)" },
  { id: "orange", color: "rgb(217, 119, 87)" },
  { id: "green", color: "rgb(88, 170, 110)" },
  { id: "purple", color: "rgb(150, 118, 226)" },
  { id: "pink", color: "rgb(222, 104, 156)" },
];
export const FONT_SIZES = [11, 12, 13, 14, 15, 16, 18];

/** 设置里主题预览的颜色：底色、侧边栏、文字。 */
export const THEME_SWATCH: Record<Exclude<ThemeId, "system">, [string, string, string]> = {
  dark: ["#161616", "#1b1b1b", "#e4e4e4"],
  light: ["#fafafa", "#f2f2f2", "#1f1f1f"],
  midnight: ["#0f141c", "#121822", "#e3e8f0"],
  mocha: ["#1a1714", "#1e1a17", "#ece4da"],
};

const KEY = "duolo.appearance";
const DEFAULT: Appearance = { theme: "dark", accent: "blue", fontSize: 13 };

let current = load();
const listeners = new Set<() => void>();
const systemDark = window.matchMedia("(prefers-color-scheme: dark)");

function load(): Appearance {
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? "{}") as Partial<Appearance>;
    return {
      theme: THEMES.includes(saved.theme as ThemeId) ? (saved.theme as ThemeId) : DEFAULT.theme,
      accent: ACCENTS.some((a) => a.id === saved.accent) ? (saved.accent as AccentId) : DEFAULT.accent,
      fontSize: FONT_SIZES.includes(saved.fontSize as number) ? (saved.fontSize as number) : DEFAULT.fontSize,
    };
  } catch {
    return DEFAULT;
  }
}

export const appearance = () => current;

/** 实际用的主题："跟随系统"按系统的深浅色。 */
export const resolvedTheme = (): Exclude<ThemeId, "system"> =>
  current.theme === "system" ? (systemDark.matches ? "dark" : "light") : current.theme;

/** 改外观：保存，马上应用到界面和所有终端。 */
export function setAppearance(change: Partial<Appearance>) {
  current = { ...current, ...change };
  try {
    localStorage.setItem(KEY, JSON.stringify(current));
  } catch {
    // 记不住就下次用默认的。
  }
  apply();
}

/** 外观变了（终端要换配色、字号）。 */
export function onAppearanceChange(listener: () => void) {
  listeners.add(listener);
  return () => void listeners.delete(listener);
}

/** 把外观应用到 <html>。启动时、改设置时、系统深浅色变了时调用。 */
export function apply() {
  const root = document.documentElement;
  root.dataset.theme = resolvedTheme();
  root.dataset.accent = current.accent;
  listeners.forEach((l) => l());
}

systemDark.addEventListener("change", () => current.theme === "system" && apply());

const ANSI = {
  black: "#1e1e1e",
  red: "#e0716c",
  green: "#8fc286",
  yellow: "#d9b45f",
  blue: "#7aa2d6",
  magenta: "#c493d1",
  cyan: "#71bcbc",
  white: "#d6d6d6",
  brightBlack: "#6b6b6b",
  brightRed: "#ec8a85",
  brightGreen: "#a6d49d",
  brightYellow: "#e6c57a",
  brightBlue: "#94b8e6",
  brightMagenta: "#d4aade",
  brightCyan: "#8dd0d0",
  brightWhite: "#f2f2f2",
};

/** 终端的文字、光标、选区颜色（背景透明，用外层的 --term-bg）。 */
const TERMINAL: Record<Exclude<ThemeId, "system">, { fg: string; bg: string; selection: string }> = {
  dark: { fg: "#d6d6d6", bg: "#161616", selection: "#3a3a3a" },
  light: { fg: "#d6d6d6", bg: "#1c1c1c", selection: "#3a3a3a" },
  midnight: { fg: "#d6dde8", bg: "#0f141c", selection: "#2c3a50" },
  mocha: { fg: "#e6ddd2", bg: "#1a1714", selection: "#463b33" },
};

/** 现在的主题下终端（xterm）的配色。 */
export function terminalTheme(): ITheme {
  const t = TERMINAL[resolvedTheme()];
  return {
    // 背景透明，由外层元素提供底色，这样输入块可以垫在文字下面。
    background: "rgba(0, 0, 0, 0)",
    foreground: t.fg,
    cursor: t.fg,
    cursorAccent: t.bg,
    selectionBackground: t.selection,
    ...ANSI,
    white: t.fg,
  };
}

/** 终端输入块的底色（styles.css 的 --term-input）。 */
export const terminalInputColor = () =>
  getComputedStyle(document.documentElement).getPropertyValue("--term-input").trim() || "#232323";
