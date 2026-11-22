import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { kbd } from "../lib/platform";

export type MenuEntry = { label: string; kbd?: string; danger?: boolean; run(): void } | { sep: true };

/** 右键菜单：出现在鼠标位置（不超出窗口），点别处或按 Esc 关闭。 */
export function ContextMenu({
  at,
  entries,
  onDismiss,
}: {
  at: { x: number; y: number };
  entries: MenuEntry[];
  onDismiss(): void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState(at);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    setPos({
      x: Math.min(at.x, window.innerWidth - width - 8),
      y: at.y + height > window.innerHeight - 8 ? Math.max(8, at.y - height) : at.y,
    });
  }, [at.x, at.y]);

  useEffect(() => {
    const close = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && onDismiss();
    const key = (e: KeyboardEvent) => e.key === "Escape" && onDismiss();
    window.addEventListener("mousedown", close, true);
    window.addEventListener("keydown", key, true);
    window.addEventListener("blur", onDismiss);
    return () => {
      window.removeEventListener("mousedown", close, true);
      window.removeEventListener("keydown", key, true);
      window.removeEventListener("blur", onDismiss);
    };
  }, [onDismiss]);

  return createPortal(
    <div
      ref={ref}
      className="menu context-menu"
      role="menu"
      style={{ left: pos.x, top: pos.y }}
      onClick={(e) => e.stopPropagation()}
      onContextMenu={(e) => e.preventDefault()}
    >
      {entries.map((entry, i) =>
        "sep" in entry ? (
          <div key={i} className="menu-sep" />
        ) : (
          <button
            key={i}
            role="menuitem"
            className={`menu-item ${entry.danger ? "danger" : ""}`}
            onClick={() => {
              onDismiss();
              entry.run();
            }}
          >
            <span className="grow">{entry.label}</span>
            {entry.kbd && <kbd>{kbd(entry.kbd)}</kbd>}
          </button>
        ),
      )}
    </div>,
    document.body,
  );
}
