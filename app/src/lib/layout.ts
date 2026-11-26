// 记住窗口里看的是什么：正在操作的任务、并排的任务和两格时的宽度，重新打开应用时照着还原。
// 只是方便，记不住（隐私模式、清了数据）就按默认打开最近的任务。

const KEY = "duolo.layout";

export interface Layout {
  /** 正在操作的任务。 */
  task: number | null;
  /** 并排看的任务，按排列顺序；不并排时为空。 */
  grid: number[];
  /** 旧版两格时左边一格占的比例，只用来换算成 `sizes`。 */
  ratio: number;
  /** 格子的大小（拖分隔线调过的）；格子的排法变了就不再用。 */
  sizes: PaneSizes | null;
}

/**
 * 格子按行排：每行的高度占比 `rows`，每行里每格的宽度占比 `cols[行]`，各自加起来是 1。
 * `key` 是排法（每行几格，例如 "3-2"），排法变了就回到等大。
 */
export interface PaneSizes {
  key: string;
  rows: number[];
  cols: number[][];
}

/** 把任务排成行：每行 `cols` 个。 */
export function paneRows<T>(items: T[], cols: number): T[][] {
  const rows: T[][] = [];
  for (let i = 0; i < items.length; i += cols) rows.push(items.slice(i, i + cols));
  return rows;
}

/** 这种排法的大小：记下的排法一样就用记下的，否则等大。 */
export function sizesFor(shape: number[], saved: PaneSizes | null): PaneSizes {
  const key = shape.join("-");
  if (saved && saved.key === key) return saved;
  const even = (n: number) => Array.from({ length: n }, () => 1 / n);
  return { key, rows: even(shape.length), cols: shape.map(even) };
}

/**
 * 拖第 `i` 和 `i + 1` 格之间的线，移动了总长的 `delta`（比例）。两格都不小于 `min`，总和不变。
 */
export function resizePair(sizes: number[], i: number, delta: number, min: number): number[] {
  const next = [...sizes];
  const pair = next[i] + next[i + 1];
  const lo = Math.min(min, pair / 2);
  next[i] = Math.min(pair - lo, Math.max(lo, next[i] + delta));
  next[i + 1] = pair - next[i];
  return next;
}

/** 两格时一格最窄占多少。 */
export const MIN_RATIO = 0.25;

/** 最多并排几格。每格一个终端，再多就太窄，WebView 能同时开的 WebGL 画布也有限。 */
export const MAX_PANES = 6;

/** 一格至少多宽（像素），终端大约 60 列。放不下就多排一行。 */
const PANE_MIN_WIDTH = 520;

export const clampRatio = (r: number) => Math.min(1 - MIN_RATIO, Math.max(MIN_RATIO, r));

/** `count` 格在 `width` 宽的地方排几列。 */
export function gridColumns(count: number, width: number): number {
  const fit = Math.max(1, Math.floor(width / PANE_MIN_WIDTH));
  // 四格排成两行两列，比一行四格或三加一好看。
  if (count === 4 && fit >= 2) return 2;
  return Math.max(1, Math.min(count, fit, 3));
}

export function loadLayout(): Layout {
  const fallback: Layout = { task: null, grid: [], ratio: 0.5, sizes: null };
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? "null") as
      | (Partial<Layout> & { beside?: { task: number; left: boolean } | null })
      | null;
    if (!saved) return fallback;
    const task = typeof saved.task === "number" ? saved.task : null;
    let grid = Array.isArray(saved.grid) ? saved.grid.filter((id): id is number => typeof id === "number") : [];
    // 旧版只有「旁边一格」。
    if (grid.length === 0 && task !== null && saved.beside && typeof saved.beside.task === "number") {
      grid = saved.beside.left ? [saved.beside.task, task] : [task, saved.beside.task];
    }
    const ratio = typeof saved.ratio === "number" ? clampRatio(saved.ratio) : 0.5;
    const ok = (s: unknown): s is PaneSizes =>
      !!s && typeof (s as PaneSizes).key === "string" && Array.isArray((s as PaneSizes).rows) && Array.isArray((s as PaneSizes).cols);
    // 旧版只记了两格的比例。
    const sizes = ok(saved.sizes) ? saved.sizes : ratio !== 0.5 ? { key: "2", rows: [1], cols: [[ratio, 1 - ratio]] } : null;
    return { task, grid: grid.slice(0, MAX_PANES), ratio, sizes };
  } catch {
    return fallback;
  }
}

export function saveLayout(layout: Layout) {
  try {
    localStorage.setItem(KEY, JSON.stringify(layout));
  } catch {
    // 记不住就下次打开最近的任务。
  }
}
