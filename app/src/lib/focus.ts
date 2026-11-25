// macOS 上，网页里的输入框有焦点、又开着第三方输入法（例如微信输入法）时，鼠标按下要先交给输入法转一圈，
// 比松开晚到：第一下点击就没了，要点两次。终端和聊天输入框几乎一直有焦点，点左侧的任务就会这样。
//
// 办法：指针离开这些输入区域（标了 `data-focus-home` 的元素）时先让它失去焦点，网页里没有输入框有焦点，
// 点击就不经过输入法；指针回来、或者开始打字时（期间没有点别处），把焦点还给它。
// 指针停在外面时按的第一个键已经发给了页面：字会跟着输入法进到输入框，回车、方向键这类会丢一次。

/** 暂时拿走焦点的输入框。 */
let parked: HTMLElement | null = null;

export function parkFocusOffInput() {
  document.addEventListener(
    "pointerover",
    (e) => {
      const target = e.target as Element | null;
      if (!target) return;
      if (parked) {
        // 回到原来的输入区域：焦点还没给别人时还回去。
        if (parked.closest("[data-focus-home]")?.contains(target)) {
          if (parked.isConnected && document.activeElement === document.body) parked.focus({ preventScroll: true });
          parked = null;
        }
        return;
      }
      const active = document.activeElement as HTMLElement | null;
      const home = active?.closest("[data-focus-home]");
      if (active && home && !home.contains(target)) {
        parked = active;
        active.blur();
      }
    },
    true,
  );
  // 指针还在外面就开始打字：先把焦点还回去。
  document.addEventListener(
    "keydown",
    () => {
      if (parked?.isConnected && document.activeElement === document.body) parked.focus({ preventScroll: true });
      parked = null;
    },
    true,
  );
  // 焦点给了别人（点了别的地方、快捷键），就不再还。
  document.addEventListener("focusin", (e) => {
    if (e.target !== parked) parked = null;
  }, true);
}
