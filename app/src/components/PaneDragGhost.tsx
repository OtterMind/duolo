import { usePaneDrag } from "../lib/paneDrag";

/** 拖任务时跟着指针的名字。 */
export function PaneDragGhost() {
  const drag = usePaneDrag();
  if (!drag) return null;
  return (
    <div className={`pane-ghost ${drag.over ? "" : "is-away"}`} style={{ left: drag.x + 12, top: drag.y + 10 }}>
      {drag.label}
    </div>
  );
}
