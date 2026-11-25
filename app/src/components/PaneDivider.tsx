import { useT } from "../i18n";
import { clampRatio } from "../lib/layout";

/** 分屏中间的线：拖动调整左右两格的宽度，双击回到一半一半。 */
export function PaneDivider({ ratio, onChange }: { ratio: number; onChange(ratio: number): void }) {
  const t = useT();
  return (
    <div
      className="pane-divider"
      role="separator"
      aria-orientation="vertical"
      aria-valuenow={Math.round(ratio * 100)}
      title={t("task.dividerHint")}
      onDoubleClick={() => onChange(0.5)}
      onPointerDown={(e) => {
        const panes = e.currentTarget.parentElement?.getBoundingClientRect();
        if (!panes || e.button !== 0) return;
        e.preventDefault();
        const el = e.currentTarget;
        el.setPointerCapture(e.pointerId);
        el.classList.add("is-dragging");
        const move = (ev: PointerEvent) => onChange(clampRatio((ev.clientX - panes.left) / panes.width));
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
