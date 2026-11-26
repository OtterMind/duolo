// 每个 Agent 一个常驻的 xterm 实例。切换任务时只是把它挂到另一个位置，
// 回滚历史和光标状态都保留，后台的 Agent 输出也会继续写进去。

import { Terminal, type IDecoration, type ITheme } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebglAddon } from "@xterm/addon-webgl";
import { WebLinksAddon } from "@xterm/addon-web-links";
import "@xterm/xterm/css/xterm.css";

import { agentKey, api, type AgentRef } from "./api";
import { isMac, modKey } from "./platform";
import { onTerminalFontSizeChange, terminalFontSize } from "./terminalFont";

export const THEME: ITheme = {
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

/** 终端字体：和 Warp 一样默认用 Hack；中文等字形回退到系统字体。 */
export const FONT_FAMILY =
  'Hack, "SF Mono", Menlo, "Cascadia Mono", Consolas, "DejaVu Sans Mono", "PingFang SC", "Hiragino Sans", "Apple SD Gothic Neo", "Microsoft YaHei", "Noto Sans CJK SC", monospace';

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
  /** 输入块位置变化时通知（录音条跟着它）。 */
  blockListeners: Set<() => void>;
  /** 画面往下挪了多少像素，见 `alignBottom`。 */
  shift: number;
  /** 输入框空着（连 Agent 的灰字提示都没有）时，提示文字放在哪；不该显示时为 null。 */
  hint: { left: number; top: number; height: number } | null;
  /** 最近发给 Agent 的几段输入，见 `recoverDroppedInput`。 */
  sent: { data: string; at: number }[];
}

const pool = new Map<string, Entry>();

// 改了字号：所有终端跟着换，输入块重画。格子数也变了，界面会重新量（见 TaskView）。
onTerminalFontSizeChange(() => {
  for (const entry of pool.values()) {
    entry.term.options.fontSize = terminalFontSize();
    entry.input = { key: "", decorations: entry.input.decorations };
    paintInputBlock(entry);
  }
});

/**
 * macOS 的 WebView 里开着中文输入法时，要按 Shift 的标点（？！：）第一下会被 xterm 丢掉，要按两次。
 *
 * Safari 内核先发输入法交来的字（input 事件），后发这个键的 keydown。xterm 看到之前按下的 Shift
 * 还没松开，以为这个字已经由按键处理过，就不发；随后 keydown 的 keyCode 是 229，xterm 去比对输入框
 * 前后的内容，可字已经在里面了，比不出差别，也不发（xterm.js #5374，修复 #5614 还没合并）。
 *
 * 办法：输入法交来字以后稍等一下，xterm 各条路（按键、输入事件、输入法组字、比对输入框）都没把它发出去，
 * 就由我们补发。xterm 发过的不会重复发。
 */
function recoverDroppedInput(entry: Entry, textarea: HTMLTextAreaElement) {
  textarea.addEventListener("input", (e) => {
    const ev = e as InputEvent;
    if (ev.inputType !== "insertText" || !ev.data || ev.isComposing) return;
    const data = ev.data;
    const at = performance.now();
    // xterm 自己的补发也是 setTimeout(0)，等它之后再看。
    window.setTimeout(() => {
      if (entry.sent.some((s) => s.at >= at - 50 && s.data.includes(data))) return;
      entry.term.input(data, true);
    }, 20);
  });
}

/**
 * xterm 按下鼠标开始拖选，等 document 收到 mouseup 才结束。macOS 的 WebView 有时收不到这个 mouseup
 * （比如从别的应用点回来、在窗口外松开），xterm 就一直以为还在拖：之后鼠标一动、内容一滚，选区就跟着变大，
 * 鼠标移到终端上下边外面时还会自动滚动着往下选。这里在鼠标移动或滚动时看一眼：键其实已经松开了，就补一个 mouseup。
 */
let pressedInTerminal = false;
const releaseIfUp = (e: MouseEvent) => {
  if (!pressedInTerminal || e.buttons !== 0) return;
  pressedInTerminal = false;
  document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true, clientX: e.clientX, clientY: e.clientY }));
};
window.addEventListener("mousemove", releaseIfUp, true);
window.addEventListener("wheel", releaseIfUp, { capture: true, passive: true });
window.addEventListener("mouseup", () => (pressedInTerminal = false), true);

/** 用户在终端里的输入（发给 Agent 之前），App 用它给新任务起名。 */
/** 鼠标停在链接上时的提示（当前语言，由界面设置）。 */
let linkHint = "";
export function setLinkHint(text: string) {
  linkHint = text;
}

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

/**
 * 剪贴板里是文件或图片时，把路径粘贴进终端（复制的图片先存成临时 PNG），返回粘贴了没有。
 * 路径里的空格等字符照访达拖文件进终端的写法转义，Agent 能认出这是一个路径。
 */
async function pasteFiles(term: Terminal): Promise<boolean> {
  const paths = await api.clipboardPaths().catch(() => [] as string[]);
  if (paths.length === 0) return false;
  term.paste(paths.map(quotePath).join(" ") + " ");
  return true;
}

function quotePath(path: string): string {
  if (!isMac && path.includes("\\")) return /\s/.test(path) ? `"${path}"` : path;
  return path.replace(/([\s'"()&;|<>$`!*?[\]{}#~\\])/g, "\\$1");
}

function create(ref: AgentRef): Entry {
  const term = new Terminal({
    // 和 Warp 一样默认用 Hack；中文等字形回退到系统字体。
    // Windows 回退到 Cascadia Mono / Consolas，Linux 回退到 DejaVu Sans Mono。
    fontFamily: FONT_FAMILY,
    // 字号默认和 Warp 一样（13），可以在设置里改；行高和 Warp 一样。
    fontSize: terminalFontSize(),
    lineHeight: 1.2,
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
  // 输出里的网址：按住 ⌘（Windows / Linux 是 Ctrl）点击用默认浏览器打开。不按住时照常点终端、选文字。
  // 两种链接：文字里看起来像网址的（WebLinksAddon），和程序用终端超链接（OSC 8）标出来的（Claude Code
  // 就这样输出链接）。后一种 xterm 默认先弹浏览器的确认框再 window.open，在桌面应用里打不开，换成一样的处理。
  const follow = (event: MouseEvent, uri: string) => {
    if (modKey(event)) void api.openUrl(uri).catch(() => {});
  };
  const hover = () => {
    if (linkHint) el.title = linkHint;
  };
  const leave = () => el.removeAttribute("title");
  term.loadAddon(new WebLinksAddon(follow, { hover, leave }));
  term.options.linkHandler = { activate: follow, hover, leave };
  term.unicode.activeVersion = "11";

  const el = document.createElement("div");
  el.className = "xterm-host";
  // 指针离开终端时暂时拿走它的焦点，见 lib/focus.ts。
  el.dataset.focusHome = "";
  el.addEventListener("mousedown", () => (pressedInTerminal = true), true);
  const block = document.createElement("div");
  block.className = "input-block";
  el.appendChild(block);

  // 粘贴（⌘V，Windows / Linux 上 Ctrl+V）：剪贴板里是文件或图片时，粘贴它们的路径，Agent 会当成附件；
  // 只有文字时照常粘贴。见 `pasteFiles`。
  el.addEventListener(
    "paste",
    (e) => {
      // 不看网页给的类型：从访达复制文件时 WebKit 可能只给出文件名文字。问系统的剪贴板（不到 0.1 秒）。
      e.preventDefault();
      e.stopPropagation();
      const text = e.clipboardData?.getData("text/plain") ?? "";
      void pasteFiles(term).then((pasted) => {
        if (!pasted && text) term.paste(text);
      });
    },
    true,
  );
  term.attachCustomKeyEventHandler((e) => {
    // macOS 上 ⌃V 是 Claude Code 这些 Agent 自己读剪贴板里的图片。复制的是文件时它读到的是文件图标，
    // 所以先看剪贴板：有文件或图片就粘贴路径，没有再把 ⌃V 交给 Agent。
    if (isMac && e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey && e.code === "KeyV") {
      if (e.type === "keydown") {
        void pasteFiles(term).then((pasted) => {
          if (!pasted) void api.write(ref, "\x16").catch(() => {});
        });
      }
      return false;
    }
    if (pcClipboard(term, e) || isAppShortcut(e)) return false;
    // ⇧↵ 换行：各个 Agent 的按键不同，由插件决定。
    if (e.type === "keydown" && e.key === "Enter" && e.shiftKey) {
      void api.newline(ref).catch(() => {});
      return false;
    }
    return true;
  });
  const entry: Entry = { term, fit, el, input: { key: "", decorations: [] }, block, opened: false, attachedSession: null, blockListeners: new Set(), shift: 0, hint: null, sent: [] };
  term.onData((data) => {
    entry.sent.push({ data, at: performance.now() });
    if (entry.sent.length > 8) entry.sent.shift();
    inputListener?.(ref, data);
    void api.write(ref, data).catch(() => {});
  });
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
    if (entry.term.textarea) recoverDroppedInput(entry, entry.term.textarea);
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
  const before = `${block.style.display}:${block.style.top}:${block.style.height}:${JSON.stringify(entry.hint)}`;
  const rowHeight = screen && term.rows > 0 ? screen.offsetHeight / term.rows : 0;
  if (screen && rowHeight) alignBottom(entry, screen, rowHeight);
  if (found && screen && rowHeight) {
    const first = top + 1 - buf.viewportY;
    const rows = bottom - top - 1;
    block.style.display = "block";
    block.style.top = `${screen.offsetTop + entry.shift + first * rowHeight - INPUT_PAD_Y}px`;
    block.style.height = `${rows * rowHeight + INPUT_PAD_Y * 2}px`;
    entry.hint = emptyInput(term, top + 1, bottom - 1, screen, rowHeight, entry.shift);
  } else {
    block.style.display = "none";
    entry.hint = null;
  }
  if (`${block.style.display}:${block.style.top}:${block.style.height}:${JSON.stringify(entry.hint)}` !== before) {
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

/**
 * 输入框里除了开头的提示符（❯ > ›）什么都没有时，返回提示文字的位置（提示符后面，第一行）。
 * 用户打的字、Agent 灰着写的提示或建议，都算有内容，这时不显示，免得叠在一起。
 */
function emptyInput(term: Terminal, first: number, last: number, screen: HTMLElement, rowHeight: number, shift: number) {
  const buf = term.buffer.active;
  const lines: string[] = [];
  for (let y = first; y <= last; y++) lines.push(buf.getLine(y)?.translateToString(true) ?? "");
  const prompt = /^(\s*)([❯>›])\s?/.exec(lines[0] ?? "");
  const rest = [prompt ? lines[0].slice(prompt[0].length) : lines[0], ...lines.slice(1)];
  if (!prompt || rest.some((line) => line.trim())) return null;
  const cellWidth = screen.offsetWidth / term.cols;
  return {
    left: screen.offsetLeft + (prompt[1].length + 2) * cellWidth,
    top: screen.offsetTop + shift + (first - buf.viewportY) * rowHeight,
    height: rowHeight,
  };
}

/**
 * 输入框始终在底部（参考 Warp）：内容还没占满一屏时，把画面整体往下挪，让最后一行内容（或光标）贴着底边。
 * 只挪画面，终端的行数不变，Agent 看到的尺寸也不变；点击和选中按挪过的位置算，xterm 自己会换算。
 * 有回滚历史时不挪（上面空出来的地方没法显示历史），这时内容一般已经占满一屏。
 */
function alignBottom(entry: Entry, screen: HTMLElement, rowHeight: number) {
  const { term } = entry;
  const buf = term.buffer.active;
  let rows = 0;
  if (buf.baseY === 0) {
    let last = buf.cursorY;
    for (let y = term.rows - 1; y > last; y--) {
      if (buf.getLine(y)?.translateToString(true).trim()) {
        last = y;
        break;
      }
    }
    rows = term.rows - 1 - last;
  }
  const shift = Math.round(rows * rowHeight);
  if (shift === entry.shift) return;
  entry.shift = shift;
  screen.style.transform = shift ? `translateY(${shift}px)` : "";
}

/** 输入块相对终端元素的位置，`hint` 见 `emptyInput`；Agent 没有显示输入框时为 null。 */
export function inputBlock(
  ref: AgentRef,
): { top: number; height: number; hint: { left: number; top: number; height: number } | null } | null {
  const entry = pool.get(agentKey(ref));
  if (!entry || entry.block.style.display !== "block") return null;
  return { top: entry.block.offsetTop, height: entry.block.offsetHeight, hint: entry.hint };
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
