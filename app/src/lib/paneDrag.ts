// 把任务拖到右侧并排看：从侧边栏拖任务，或者拖格子的把手换位置。
//
// 不用 HTML 的拖放：窗口接收拖进来的文件夹（Tauri 的拖放）时，Windows 上网页里的拖放事件收不到。
// 这里用指针事件：按下后移动超过几像素才算开始拖，松开时看指针下面是哪一格、落在格子的哪个位置。

import { useSyncExternalStore } from "react";

/** 落在格子的哪里：边上是插到它前面或后面，中间是换掉它。 */
export type DropZone = "left" | "right" | "top" | "bottom" | "center";

export interface PaneDrag {
  task: number;
  /** 跟着指针显示的名字。 */
  label: string;
  x: number;
  y: number;
  /** 指针下面的格子（它的任务）；在右侧但不在格子上（没有任务时）为 null。 */
  target: number | null;
  zone: DropZone;
  /** 指针在右侧（能放下）。 */
  over: boolean;
}

export interface PaneDrop {
  task: number;
  target: number | null;
  zone: DropZone;
}

/** 移动多少像素才算开始拖（之前松开就是普通的点击）。 */
const THRESHOLD = 6;

let current: PaneDrag | null = null;
const listeners = new Set<() => void>();
let onDrop: ((drop: PaneDrop) => void) | null = null;

const emit = () => listeners.forEach((l) => l());

/** 松开时调用（App 注册）。 */
export function setDropHandler(handler: ((drop: PaneDrop) => void) | null) {
  onDrop = handler;
}

/** 正在拖的任务，没有在拖时为 null。 */
export function usePaneDrag(): PaneDrag | null {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => current,
  );
}

/** 指针下面是哪一格、哪个位置。 */
function hit(x: number, y: number): Pick<PaneDrag, "target" | "zone" | "over"> {
  const el = document.elementFromPoint(x, y);
  const pane = el?.closest<HTMLElement>("[data-pane-task]");
  if (!pane) return { target: null, zone: "center", over: !!el?.closest(".main") };
  const r = pane.getBoundingClientRect();
  const [fx, fy] = [(x - r.left) / r.width, (y - r.top) / r.height];
  let zone: DropZone = "center";
  // 中间一半是换掉，外圈按离哪条边最近。
  if (fx < 0.25 || fx > 0.75 || fy < 0.25 || fy > 0.75) {
    const edges: [DropZone, number][] = [
      ["left", fx],
      ["right", 1 - fx],
      ["top", fy],
      ["bottom", 1 - fy],
    ];
    zone = edges.sort((a, b) => a[1] - b[1])[0][0];
  }
  return { target: Number(pane.dataset.paneTask), zone, over: true };
}

/** 在指针按下时调用：之后移动够远就开始拖 `task`。 */
export function beginPaneDrag(e: { button: number; clientX: number; clientY: number }, task: number, label: string) {
  if (e.button !== 0) return;
  const [sx, sy] = [e.clientX, e.clientY];
  let dragging = false;
  const move = (ev: PointerEvent) => {
    if (!dragging) {
      if (Math.hypot(ev.clientX - sx, ev.clientY - sy) < THRESHOLD) return;
      dragging = true;
      document.body.classList.add("is-dragging-pane");
    }
    ev.preventDefault();
    current = { task, label, x: ev.clientX, y: ev.clientY, ...hit(ev.clientX, ev.clientY) };
    emit();
  };
  const end = (ev: PointerEvent) => {
    window.removeEventListener("pointermove", move, true);
    window.removeEventListener("pointerup", end, true);
    window.removeEventListener("pointercancel", end, true);
    if (!dragging) return;
    document.body.classList.remove("is-dragging-pane");
    // 拖完松开在侧边栏的同一行上时，不要再当成点击。
    window.addEventListener("click", stop, true);
    window.setTimeout(() => window.removeEventListener("click", stop, true), 0);
    const drop = current;
    current = null;
    emit();
    if (ev.type === "pointerup" && drop?.over) onDrop?.({ task: drop.task, target: drop.target, zone: drop.zone });
  };
  const stop = (c: Event) => c.stopPropagation();
  window.addEventListener("pointermove", move, true);
  window.addEventListener("pointerup", end, true);
  window.addEventListener("pointercancel", end, true);
}
