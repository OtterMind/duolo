import { Fragment, useEffect, useRef, useState, type ReactNode } from "react";

import { gridColumns, paneRows, resizePair, sizesFor, type PaneSizes } from "../lib/layout";
import { usePaneDrag, type DropZone } from "../lib/paneDrag";
import { PaneDivider } from "./PaneDivider";

/** 拖线时一格最窄、最矮多少像素。 */
const MIN_WIDTH = 240;
const MIN_HEIGHT = 140;

/**
 * 右侧的格子，按行排：每行几格由窗口宽度定（四格是两行两列）。格子之间、行与行之间的线都能拖，
 * 每行的宽度各自调；排法变了（多了一格、窗口变窄）就回到等大。拖任务经过时，指针下面那一格标出会落在哪里。
 */
export function PaneGrid({
  tasks,
  active,
  sizes: saved,
  onSizes,
  onActivate,
  render,
}: {
  tasks: number[];
  active: number;
  sizes: PaneSizes | null;
  onSizes(sizes: PaneSizes): void;
  onActivate(task: number): void;
  render(task: number): ReactNode;
}) {
  const host = useRef<HTMLDivElement>(null);
  const [width, height] = useSize(host);
  const drag = usePaneDrag();
  const rows = paneRows(tasks, gridColumns(tasks.length, width));
  const sizes = sizesFor(
    rows.map((r) => r.length),
    saved,
  );
  // 拖线时用最新的大小（一次拖动里会连着调用很多次）。
  const latest = useRef(sizes);
  latest.current = sizes;
  const update = (next: PaneSizes) => {
    latest.current = next;
    onSizes(next);
  };

  return (
    <div ref={host} className={`panes ${tasks.length > 1 ? "is-split" : ""}`}>
      {rows.map((row, r) => (
        <Fragment key={r}>
          {r > 0 && (
            <PaneDivider
              vertical={false}
              onDrag={(d) => {
                const s = latest.current;
                update({ ...s, rows: resizePair(s.rows, r - 1, d, MIN_HEIGHT / Math.max(height, 1)) });
              }}
              onEven={() => {
                const s = latest.current;
                update({ ...s, rows: resizePair(s.rows, r - 1, (s.rows[r] - s.rows[r - 1]) / 2, 0) });
              }}
            />
          )}
          <div className="pane-row" style={{ flex: `${sizes.rows[r]} 1 0` }}>
            {row.map((task, c) => (
              <Fragment key={task}>
                {c > 0 && (
                  <PaneDivider
                    vertical
                    onDrag={(d) => {
                      const s = latest.current;
                      const cols = s.cols.map((line, i) =>
                        i === r ? resizePair(line, c - 1, d, MIN_WIDTH / Math.max(width, 1)) : line,
                      );
                      update({ ...s, cols });
                    }}
                    onEven={() => {
                      const s = latest.current;
                      const line = s.cols[r];
                      const cols = s.cols.map((l, i) => (i === r ? resizePair(line, c - 1, (line[c] - line[c - 1]) / 2, 0) : l));
                      update({ ...s, cols });
                    }}
                  />
                )}
                <div
                  className={`pane ${tasks.length > 1 && task === active ? "is-active" : ""}`}
                  data-pane-task={task}
                  style={{ flex: `${sizes.cols[r][c]} 1 0` }}
                  // 点旁边那一格（包括里面的按钮、终端）就换成操作它。
                  onMouseDownCapture={() => task !== active && onActivate(task)}
                >
                  {render(task)}
                  {drag?.target === task && drag.task !== task && <DropMark zone={drag.zone} />}
                </div>
              </Fragment>
            ))}
          </div>
        </Fragment>
      ))}
    </div>
  );
}

/** 落点：边上是一条蓝色的边（插到这一格旁边），中间是整格蒙一层（换掉这一格）。 */
function DropMark({ zone }: { zone: DropZone }) {
  return <div className={`pane-drop is-${zone}`} aria-hidden="true" />;
}

/** 元素的宽和高，跟着窗口变。 */
function useSize(ref: React.RefObject<HTMLElement | null>): [number, number] {
  const [size, setSize] = useState<[number, number]>(() => [window.innerWidth, window.innerHeight]);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const measure = () => setSize([el.clientWidth, el.clientHeight]);
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    measure();
    return () => observer.disconnect();
  }, [ref]);
  return size;
}
