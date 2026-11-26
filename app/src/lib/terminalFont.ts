// 终端字号：只是这台电脑上的偏好，存在本机（localStorage）。

export const FONT_SIZES = [11, 12, 13, 14, 15, 16, 18];

const KEY = "duolo.terminalFontSize";
/** 默认和 Warp 一样。 */
const DEFAULT = 13;

let current = load();
const listeners = new Set<() => void>();

function load(): number {
  try {
    const saved = Number(localStorage.getItem(KEY));
    return FONT_SIZES.includes(saved) ? saved : DEFAULT;
  } catch {
    return DEFAULT;
  }
}

export const terminalFontSize = () => current;

/** 改字号：保存，通知所有终端。 */
export function setTerminalFontSize(size: number) {
  current = size;
  try {
    localStorage.setItem(KEY, String(size));
  } catch {
    // 记不住就下次用默认的。
  }
  listeners.forEach((l) => l());
}

/** 字号变了（终端要重新量格子）。 */
export function onTerminalFontSizeChange(listener: () => void) {
  listeners.add(listener);
  return () => void listeners.delete(listener);
}
