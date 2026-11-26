// 任务下方的终端面板：快捷键（⌃`）、菜单里的「在这里打开终端」要让某个任务的面板打开或收起。
// 任务可能还没显示出来（从侧边栏菜单打开别的任务），请求先记下来，任务页挂上后再处理。

import { useEffect } from "react";

/** `toggle`：开关面板（没有终端时开一个）；`new`：打开面板并新开一个终端；`{ show }`：打开面板，选中这个终端。 */
export type ShellRequest = "toggle" | "new" | { show: number };

const EVENT = "duolo:shell";
const pending = new Map<number, ShellRequest>();

export function requestShell(task: number, what: ShellRequest) {
  pending.set(task, what);
  window.dispatchEvent(new CustomEvent(EVENT, { detail: task }));
}

/** 任务页里调用：处理给这个任务的请求（包括挂上之前发来的）。 */
export function useShellRequests(task: number, handle: (what: ShellRequest) => void) {
  useEffect(() => {
    const take = () => {
      const what = pending.get(task);
      if (what === undefined) return;
      pending.delete(task);
      handle(what);
    };
    take();
    const onEvent = (e: Event) => (e as CustomEvent<number>).detail === task && take();
    window.addEventListener(EVENT, onEvent);
    return () => window.removeEventListener(EVENT, onEvent);
  });
}
