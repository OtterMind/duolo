// 记住窗口里看的是什么：正在操作的任务、分屏时旁边的任务和左右两格的宽度，重新打开应用时照着还原。
// 只是方便，记不住（隐私模式、清了数据）就按默认打开最近的任务。

const KEY = "duolo.layout";

export interface Layout {
  /** 正在操作的任务。 */
  task: number | null;
  /** 分屏时旁边的任务，和它在左边还是右边。 */
  beside: { task: number; left: boolean } | null;
  /** 左边一格占的比例。 */
  ratio: number;
}

/** 一格最窄占多少。 */
export const MIN_RATIO = 0.25;

export const clampRatio = (r: number) => Math.min(1 - MIN_RATIO, Math.max(MIN_RATIO, r));

export function loadLayout(): Layout {
  const fallback: Layout = { task: null, beside: null, ratio: 0.5 };
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? "null") as Partial<Layout> | null;
    if (!saved) return fallback;
    return {
      task: typeof saved.task === "number" ? saved.task : null,
      beside: saved.beside && typeof saved.beside.task === "number" ? saved.beside : null,
      ratio: typeof saved.ratio === "number" ? clampRatio(saved.ratio) : 0.5,
    };
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
