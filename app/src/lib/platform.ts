// 各个系统的差别：快捷键怎么按、怎么写。
//
// 快捷键统一按 macOS 的写法定义（⌘⇧R、⌥↓），在 Windows / Linux 上换算：
// - ⌘ 换成 Ctrl；
// - ⌘ 加字母的应用快捷键再加上 Shift（⌘N → Ctrl+Shift+N）：Ctrl+字母要留给终端里的 Claude / Codex，
//   和 Windows Terminal、VS Code 终端的习惯一样。⌘Z 不加；数字、方向键、↵ 也不加。
// - ⌥ 换成 Alt，⇧ 换成 Shift，↵ 换成 Enter。

export const isMac = navigator.userAgent.includes("Mac");

/** 按住说话的键：macOS 是 Fn，Windows / Linux 是全局快捷键（见 crates/duolo-app/src/fnkey.rs）。 */
export const TALK_KEY = isMac ? "Fn" : "Ctrl+Shift+Space";

/** ⌘ 加字母时不再加 Shift 的编辑键（撤销）。 */
const EDIT_KEYS = new Set(["Z"]);

interface Combo {
  cmd: boolean;
  alt: boolean;
  shift: boolean;
  /** 大写字母、数字、↵、↑、↓ 或 Space；只有修饰键时为空。 */
  key: string;
}

function parse(mac: string): Combo {
  const key = mac.replace(/[⌘⌥⇧⌃]/g, "");
  return { cmd: /[⌘⌃]/.test(mac), alt: mac.includes("⌥"), shift: mac.includes("⇧"), key };
}

/** 在 Windows / Linux 上实际要按的组合。 */
function pcCombo(mac: string): Combo {
  const c = parse(mac);
  return { ...c, shift: c.shift || (c.cmd && /^[A-Z]$/.test(c.key) && !EDIT_KEYS.has(c.key)) };
}

function pcLabel(mac: string): string {
  const c = pcCombo(mac);
  const key = c.key === "↵" ? "Enter" : c.key;
  return [c.cmd && "Ctrl", c.alt && "Alt", c.shift && "Shift", key].filter(Boolean).join("+");
}

/** 快捷键的写法：传 macOS 的写法，在 Windows / Linux 上换算。 */
export function kbd(mac: string): string {
  return isMac ? mac : pcLabel(mac);
}

/** 把一段文字里的 macOS 快捷键（"显示审阅（⌘⇧R）"）换成当前系统的写法。 */
export function localizeKeys(text: string): string {
  return isMac ? text : text.replace(/[⌘⌥⇧⌃]+(?:Space|[A-Z0-9↵↑↓])?/g, pcLabel);
}

const KEY_NAMES: Record<string, string> = { "↵": "Enter", "↑": "ArrowUp", "↓": "ArrowDown", Space: " " };

/** 按键事件是不是这个快捷键（按 macOS 的写法传）。 */
export function matches(e: KeyboardEvent, mac: string): boolean {
  const c = isMac ? parse(mac) : pcCombo(mac);
  const cmd = isMac ? e.metaKey : e.ctrlKey;
  const other = isMac ? e.ctrlKey : e.metaKey;
  if (cmd !== c.cmd || e.altKey !== c.alt || e.shiftKey !== c.shift || other) return false;
  const want = KEY_NAMES[c.key] ?? c.key;
  // ⌥ 会改变 e.key（⌥K → ˚），字母和数字按物理键比较。
  if (/^[A-Z]$/.test(want)) return e.code === `Key${want}`;
  if (/^[0-9]$/.test(want)) return e.code === `Digit${want}`;
  return e.key === want;
}

/** ⌘（macOS）或 Ctrl（Windows / Linux）是否按下。 */
export const modKey = (e: KeyboardEvent | MouseEvent) => (isMac ? e.metaKey : e.ctrlKey);
