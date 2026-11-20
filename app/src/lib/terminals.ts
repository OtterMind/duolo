// 每个 Agent 一个常驻的 xterm 实例。切换任务时只是把它挂到另一个位置，
// 回滚历史和光标状态都保留，后台的 Agent 输出也会继续写进去。

import { Terminal, type ITheme } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebglAddon } from "@xterm/addon-webgl";
import "@xterm/xterm/css/xterm.css";

import { agentKey, api, type AgentRef } from "./api";

const THEME: ITheme = {
  background: "#161616",
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

interface Entry {
  term: Terminal;
  fit: FitAddon;
  el: HTMLDivElement;
  /** xterm 要在元素挂到页面上之后才能初始化渲染（需要测量字符尺寸）。 */
  opened: boolean;
  /** 已订阅的进程编号。Agent 重新启动后编号会变，要重新订阅。 */
  attachedSession: number | null;
}

const pool = new Map<string, Entry>();

/** 用户在终端里的输入（发给 Agent 之前），App 用它给新任务起名。 */
let inputListener: ((ref: AgentRef, data: string) => void) | null = null;
export function setInputListener(listener: typeof inputListener) {
  inputListener = listener;
}

/** 应用自己处理的按键，不交给 Agent。 */
function isAppShortcut(e: KeyboardEvent) {
  if (e.metaKey) return true; // ⌘ 组合键都是应用菜单的
  if (e.ctrlKey && (e.code === "Space" || e.key === "Tab")) return true;
  return false;
}

function create(ref: AgentRef): Entry {
  const term = new Terminal({
    fontFamily: '"SF Mono", Menlo, Monaco, "PingFang SC", monospace',
    fontSize: 13,
    lineHeight: 1.2,
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

  term.attachCustomKeyEventHandler((e) => {
    if (isAppShortcut(e)) return false;
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

  return { term, fit, el, opened: false, attachedSession: null };
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

/** 任务关闭后释放它的终端。 */
export function disposeTask(task: number) {
  for (const [key, entry] of pool) {
    if (key.startsWith(`${task}:`)) {
      entry.term.dispose();
      pool.delete(key);
    }
  }
}
