import { useT } from "../i18n";

/**
 * 格子之间的线：竖线拖左右宽度，横线拖上下高度，双击让两边一样大。
 * `onDrag` 收到的是指针移动了多少，占线所在那一行（或整个右侧）的比例。
 */
export function PaneDivider({
  vertical,
  onDrag,
  onEven,
}: {
  /** 竖线（左右两格之间）；否则是横线（上下两行之间）。 */
  vertical: boolean;
  onDrag(delta: number): void;
  onEven(): void;
}) {
  const t = useT();
  return (
    <div
      className={`pane-divider ${vertical ? "" : "is-row"}`}
      role="separator"
      aria-orientation={vertical ? "vertical" : "horizontal"}
      title={t("task.dividerHint")}
      onDoubleClick={onEven}
      onPointerDown={(e) => {
        const box = e.currentTarget.parentElement?.getBoundingClientRect();
        if (!box || e.button !== 0) return;
        e.preventDefault();
        const el = e.currentTarget;
        const total = vertical ? box.width : box.height;
        let last = vertical ? e.clientX : e.clientY;
        el.setPointerCapture(e.pointerId);
        el.classList.add("is-dragging");
        const move = (ev: PointerEvent) => {
          const at = vertical ? ev.clientX : ev.clientY;
          if (total > 0 && at !== last) onDrag((at - last) / total);
          last = at;
        };
        const up = () => {
          el.classList.remove("is-dragging");
          el.removeEventListener("pointermove", move);
          el.removeEventListener("pointerup", up);
          el.removeEventListener("pointercancel", up);
        };
        el.addEventListener("pointermove", move);
        el.addEventListener("pointerup", up);
        el.addEventListener("pointercancel", up);
      }}
    />
  );
}
