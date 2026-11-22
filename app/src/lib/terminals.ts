// 每个 Agent 一个常驻的 xterm 实例。切换任务时只是把它挂到另一个位置，
// 回滚历史和光标状态都保留，后台的 Agent 输出也会继续写进去。

import { Terminal, type IDecoration, type ITheme } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebglAddon } from "@xterm/addon-webgl";
import "@xterm/xterm/css/xterm.css";

import { agentKey, api, type AgentRef } from "./api";
import { isMac } from "./platform";

const THEME: ITheme = {
  // 背景透明，由外层元素提供底色，这样输入块可以垫在文字下面。
  background: "rgba(0, 0, 0, 0)",
  foreground: "#d6d6d6",
  cursor: "#d6d6d6",
  cursorAccent: "#161616",
  selectionBackground: "#3a3a3a",
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

/** 输入框底色：比终端背景浅一档，参考 Warp 的输入区。 */
const INPUT_BG = "#232323";
/** 输入块在输入行上下留出的内边距（像素），参考 Warp。要大于半行，才能盖住被隐藏的横线。 */
const INPUT_PAD_Y = 12;
/** 只在光标上下这么多行里找输入框的分隔线。 */
const INPUT_SEARCH_ROWS = 24;

interface Entry {
  term: Terminal;
  /** 当前输入框的装饰和它覆盖的行范围，范围不变时不重画。 */
  input: { key: string; decorations: IDecoration[] };
  /** 垫在终端文字下面的输入块（终端背景透明，所以能透出来）。 */
  block: HTMLDivElement;
  fit: FitAddon;
  el: HTMLDivElement;
  /** xterm 要在元素挂到页面上之后才能初始化渲染（需要测量字符尺寸）。 */
  opened: boolean;
  /** 已订阅的进程编号。Agent 重新启动后编号会变，要重新订阅。 */
  attachedSession: number | null;
  /** 输入块位置变化时通知（麦克风和录音条跟着它）。 */
  blockListeners: Set<() => void>;
}

const pool = new Map<string, Entry>();

/** 用户在终端里的输入（发给 Agent 之前），App 用它给新任务起名。 */
let inputListener: ((ref: AgentRef, data: string) => void) | null = null;
export function setInputListener(listener: typeof inputListener) {
  inputListener = listener;
}

/** 应用自己处理的按键，不交给 Agent。 */
function isAppShortcut(e: KeyboardEvent) {
  // macOS：⌘ 组合键都是应用菜单的。Windows / Linux：Ctrl+Shift+字母是应用的（见 lib/platform.ts），
  // Ctrl+字母留给 Agent。
  if (isMac ? e.metaKey : e.ctrlKey && e.shiftKey && /^Key[A-Z]$/.test(e.code)) return true;
  if (e.ctrlKey && e.key === "Tab") return true;
  return false;
}

/**
 * Windows / Linux 上的复制粘贴，和 Windows Terminal 一样：有选中文字时 Ctrl+C 复制，否则照常发给 Agent（中断）；
 * Ctrl+V 粘贴。返回 true 表示已经处理，不再交给终端。
 */
function pcClipboard(term: Terminal, e: KeyboardEvent) {
  if (isMac || e.type !== "keydown" || !e.ctrlKey || e.altKey || e.metaKey) return false;
  if (e.code === "KeyC" && (e.shiftKey || term.hasSelection())) {
    if (term.hasSelection()) void navigator.clipboard.writeText(term.getSelection());
    term.clearSelection();
    e.preventDefault();
    return true;
  }
  // 交给浏览器的默认行为：触发 paste 事件，由 xterm 当作粘贴发给 Agent。
  return e.code === "KeyV";
}

function create(ref: AgentRef): Entry {
  const term = new Terminal({
    // 和 Warp 一样默认用 Hack；中文等字形回退到系统字体。
    // Windows 回退到 Cascadia Mono / Consolas，Linux 回退到 DejaVu Sans Mono。
    fontFamily:
      'Hack, "SF Mono", Menlo, "Cascadia Mono", Consolas, "DejaVu Sans Mono", "PingFang SC", "Hiragino Sans", "Apple SD Gothic Neo", "Microsoft YaHei", "Noto Sans CJK SC", monospace',
    fontSize: 13,
    lineHeight: 1.3,
    cursorStyle: "bar",
    allowTransparency: true,
    cursorBlink: true,
    scrollback: 10000,
    allowProposedApi: true,
    macOptionClickForcesSelection: true,
    theme: THEME,
  });
  const fit = new FitAddon();
  term.loadAddon(fit);
  term.loadAddon(new Unicode11Addon());
  term.unicode.activeVersion = "11";

  const el = document.createElement("div");
  el.className = "xterm-host";
  const block = document.createElement("div");
  block.className = "input-block";
  el.appendChild(block);

  term.attachCustomKeyEventHandler((e) => {
    if (pcClipboard(term, e) || isAppShortcut(e)) return false;
    // ⇧↵ 换行：发送 Esc+CR，Claude Code 和 Codex 都把它当作输入框里的换行。
    if (e.type === "keydown" && e.key === "Enter" && e.shiftKey) {
      void api.write(ref, "\x1b\r");
      return false;
    }
    return true;
  });
  term.onData((data) => {
    inputListener?.(ref, data);
    void api.write(ref, data).catch(() => {});
  });

  const entry: Entry = { term, fit, el, input: { key: "", decorations: [] }, block, opened: false, attachedSession: null, blockListeners: new Set() };
  let frame = 0;
  const schedule = () => {
    if (!frame) frame = requestAnimationFrame(() => ((frame = 0), paintInputBlock(entry)));
  };
  term.onWriteParsed(schedule);
  term.onResize(schedule);
  return entry;
}

/** 把 Agent 的终端挂到 `container` 里，返回卸载函数。 */
export function mount(ref: AgentRef, container: HTMLElement): () => void {
  const entry = terminalFor(ref);
  container.appendChild(entry.el);
  if (!entry.opened) {
    entry.opened = true;
    entry.term.open(entry.el);
    try {
      const webgl = new WebglAddon();
      webgl.onContextLoss(() => webgl.dispose());
      entry.term.loadAddon(webgl);
    } catch {
      // 不支持 WebGL 时退回默认渲染。
    }
  }
  return () => entry.el.remove();
}

export function terminalFor(ref: AgentRef): Entry {
  const key = agentKey(ref);
  let entry = pool.get(key);
  if (!entry) {
    entry = create(ref);
    pool.set(key, entry);
  }
  return entry;
}

/** 订阅 `session` 号进程的输出；已订阅时什么也不做。 */
export async function attach(ref: AgentRef, session: number) {
  const entry = terminalFor(ref);
  if (entry.attachedSession === session) return;
  entry.attachedSession = session;
  entry.term.reset();
  try {
    await api.attach(ref, (bytes) => entry.term.write(bytes));
  } catch (err) {
    entry.attachedSession = null;
    throw err;
  }
}

/** 按容器大小调整列数和行数，返回新的尺寸。 */
export function fit(ref: AgentRef): { rows: number; cols: number } | null {
  const entry = pool.get(agentKey(ref));
  if (!entry || !entry.opened || !entry.el.isConnected) return null;
  entry.fit.fit();
  return { rows: entry.term.rows, cols: entry.term.cols };
}

/**
 * 把 Agent 的输入框画成一块圆角浅色底（参考 Warp）。
 *
 * Claude Code 用上下两条横线（─）围出输入框。这里在光标上下找到这两条线：输入块覆盖中间的输入行，
 * 上下再留出内边距；横线染成输入块的颜色，正好落在内边距里看不出来。找不到横线时隐藏输入块。
 */
function paintInputBlock(entry: Entry) {
  const { term, block } = entry;
  const buf = term.buffer.active;
  const cursor = buf.baseY + buf.cursorY;
  const isRule = (y: number) => {
    const text = buf.getLine(y)?.translateToString(true).trim() ?? "";
    return text.length >= Math.min(20, term.cols / 2) && /^[─━]+$/.test(text);
  };
  let top = -1;
  let bottom = -1;
  // 普通屏幕和备用屏幕（新版 Claude Code 用备用屏幕）都适用。
  for (let y = cursor; y >= Math.max(0, cursor - INPUT_SEARCH_ROWS); y--) {
    if (isRule(y)) {
      top = y;
      break;
    }
  }
  for (let y = cursor; y <= Math.min(buf.length - 1, cursor + INPUT_SEARCH_ROWS); y++) {
    if (isRule(y)) {
      bottom = y;
      break;
    }
  }
  const found = top >= 0 && bottom > top + 1;

  // 输入块的位置每次都更新（滚动或改变窗口大小时行的位置会变）。
  const screen = entry.el.querySelector<HTMLElement>(".xterm-screen");
  const before = `${block.style.display}:${block.style.top}:${block.style.height}`;
  if (found && screen && term.rows > 0) {
    const rowHeight = screen.getBoundingClientRect().height / term.rows;
    const first = top + 1 - buf.viewportY;
    const rows = bottom - top - 1;
    block.style.display = "block";
    block.style.top = `${screen.offsetTop + first * rowHeight - INPUT_PAD_Y}px`;
    block.style.height = `${rows * rowHeight + INPUT_PAD_Y * 2}px`;
  } else {
    block.style.display = "none";
  }
  if (`${block.style.display}:${block.style.top}:${block.style.height}` !== before) {
    entry.blockListeners.forEach((listener) => listener());
  }

  const key = found ? `${top}:${bottom}:${term.cols}` : "";
  if (key === entry.input.key) return;
  entry.input.decorations.forEach((d) => d.dispose());
  entry.input = { key, decorations: [] };
  if (!found) return;
  for (let y = top; y <= bottom; y++) {
    const marker = term.registerMarker(y - cursor);
    const rule = y === top || y === bottom;
    // 横线染成输入块的颜色藏起来；输入行铺上同样的底色，透明背景上的字形渲染才不会带出黑底。
    const decoration =
      marker &&
      term.registerDecoration(
        rule
          ? { marker, width: term.cols, foregroundColor: INPUT_BG }
          : { marker, width: term.cols, backgroundColor: INPUT_BG },
      );
    if (decoration) entry.input.decorations.push(decoration);
  }
}

/** 输入块相对终端元素的位置；Agent 没有显示输入框时为 null。 */
export function inputBlock(ref: AgentRef): { top: number; height: number } | null {
  const entry = pool.get(agentKey(ref));
  if (!entry || entry.block.style.display !== "block") return null;
  return { top: entry.block.offsetTop, height: entry.block.offsetHeight };
}

/** 输入块出现、消失或移动时回调，返回取消函数。 */
export function onInputBlockChange(ref: AgentRef, callback: () => void): () => void {
  const listeners = terminalFor(ref).blockListeners;
  listeners.add(callback);
  return () => listeners.delete(callback);
}

/** 任务关闭后释放它的终端。 */
export function disposeTask(task: number) {
  for (const [key, entry] of pool) {
    if (key.startsWith(`${task}:`)) {
      entry.term.dispose();
      pool.delete(key);
    }
  }
}
