import { Fragment, useEffect, useRef, useState, type ReactNode } from "react";

import { gridColumns } from "../lib/layout";
import { usePaneDrag, type DropZone } from "../lib/paneDrag";
import { PaneDivider } from "./PaneDivider";

/**
 * 右侧的格子。一格占满；两格左右排，中间的线可以拖；三格以上排成等大的网格，列数按宽度算，
 * 最后一行不满时最后一格拉宽补齐。拖任务经过时，指针下面那一格标出会落在哪里。
 */
export function PaneGrid({
  tasks,
  active,
  ratio,
  onRatio,
  onActivate,
  render,
}: {
  tasks: number[];
  active: number;
  ratio: number;
  onRatio(ratio: number): void;
  onActivate(task: number): void;
  render(task: number): ReactNode;
}) {
  const host = useRef<HTMLDivElement>(null);
  const width = useWidth(host);
  const drag = usePaneDrag();
  const count = tasks.length;
  const twoUp = count === 2 && gridColumns(2, width) === 2;
  const cols = count >= 3 ? gridColumns(count, width) : 1;
  const rows = Math.ceil(count / cols);
  const rest = count % cols;

  const pane = (task: number, i: number) => {
    const isActive = count > 1 && task === active;
    const span = count >= 3 && rest !== 0 && i === count - 1 ? cols - rest + 1 : 1;
    return (
      <div
        className={`pane ${isActive ? "is-active" : ""}`}
        data-pane-task={task}
        style={
          twoUp
            ? { flex: `${i === 0 ? ratio : 1 - ratio} 1 0` }
            : span > 1
              ? { gridColumn: `span ${span}` }
              : undefined
        }
        // 点旁边那一格（包括里面的按钮、终端）就换成操作它。
        onMouseDownCapture={() => task !== active && onActivate(task)}
      >
        {render(task)}
        {drag?.target === task && drag.task !== task && <DropMark zone={drag.zone} />}
      </div>
    );
  };

  if (count <= 2) {
    return (
      <div ref={host} className={`panes ${count === 2 && !twoUp ? "is-stacked" : ""}`}>
        {tasks.map((task, i) => (
          <Fragment key={task}>
            {twoUp && i > 0 && <PaneDivider ratio={ratio} onChange={onRatio} />}
            {pane(task, i)}
          </Fragment>
        ))}
      </div>
    );
  }
  return (
    <div
      ref={host}
      className="panes is-grid"
      style={{ gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))`, gridTemplateRows: `repeat(${rows}, minmax(0, 1fr))` }}
    >
      {tasks.map((task, i) => (
        <Fragment key={task}>{pane(task, i)}</Fragment>
      ))}
    </div>
  );
}

/** 落点：边上是一条蓝色的边（插到这一格旁边），中间是整格蒙一层（换掉这一格）。 */
function DropMark({ zone }: { zone: DropZone }) {
  return <div className={`pane-drop is-${zone}`} aria-hidden="true" />;
}

/** 元素的宽度，跟着窗口变。 */
function useWidth(ref: React.RefObject<HTMLElement | null>) {
  const [width, setWidth] = useState(() => window.innerWidth);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const observer = new ResizeObserver(() => setWidth(el.clientWidth));
    observer.observe(el);
    setWidth(el.clientWidth);
    return () => observer.disconnect();
  }, [ref]);
  return width;
}
