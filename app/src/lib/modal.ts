import { useEffect, type RefObject } from "react";

/**
 * 弹窗（`.scrim` 里的）开着时，键盘留在弹窗里：打开时焦点放到 `target`，别处抢走（终端刚启动好、按 Tab 走出去了）
 * 就拿回来，不然按键会发给后面的 Agent。关闭后焦点还给原来的地方；原来没有的，
 * 给中途第一个想要的（启动时弹出提示，终端随后才准备好）。
 */
export function useModalFocus(target: RefObject<HTMLElement | null>) {
  useEffect(() => {
    let back: Element | null = document.activeElement;
    target.current?.focus();
    const keep = (e: FocusEvent) => {
      // 弹窗上面还可以再开一个弹窗，在哪个里面都行。
      if (!(e.target instanceof HTMLElement) || e.target.closest(".scrim")) return;
      if (!back || back === document.body) back = e.target;
      target.current?.focus();
    };
    document.addEventListener("focusin", keep);
    return () => {
      document.removeEventListener("focusin", keep);
      if (back instanceof HTMLElement && back.isConnected) back.focus();
    };
  }, [target]);
}
