import { Check, X } from "lucide-react";

import type { Status, Task, TaskState } from "../lib/api";
import { statusKey } from "../lib/format";
import { useT, type MessageKey } from "../i18n";

/**
 * 一个 Agent 的状态（任务页顶部）：转圈 = 运行中，琥珀色圆点 = 等你确认，
 * 灰色对勾 = 做完了、等你下一句，空心圆 = 已结束，红叉 = 异常退出，未启动不显示。
 */
export function StatusIcon({ status }: { status: Status }) {
  const t = useT();
  const label = t(statusKey(status));
  if (status === "not_started") return null;
  if (status === "idle")
    return <Check className="status-glyph" size={13} strokeWidth={2} aria-label={label} role="img" />;
  if (status === "failed")
    return <X className="status-glyph is-failed" size={13} strokeWidth={2} aria-label={label} role="img" />;
  return <span className={`status status-${status}`} title={label} aria-label={label} role="img" />;
}

/** 任务状态的文案，和手机端同一组键（`state.*`）。 */
export function useStateLabel() {
  const t = useT();
  return (task: Task) => t(`state.${task.state}` as MessageKey, { count: task.review?.issues ?? 0 });
}

// GitHub Octicons（MIT），16 × 16；手机端用的是同一组（mobile/scripts/gen-icons.mjs）。
const GLYPHS = {
  check:
    "M13.78 4.22a.75.75 0 0 1 0 1.06l-7.25 7.25a.75.75 0 0 1-1.06 0L2.22 9.28a.751.751 0 0 1 .018-1.042.751.751 0 0 1 1.042-.018L6 10.94l6.72-6.72a.75.75 0 0 1 1.06 0Z",
  x: "M3.72 3.72a.75.75 0 0 1 1.06 0L8 6.94l3.22-3.22a.749.749 0 0 1 1.275.326.749.749 0 0 1-.215.734L9.06 8l3.22 3.22a.749.749 0 0 1-.326 1.275.749.749 0 0 1-.734-.215L8 9.06l-3.22 3.22a.751.751 0 0 1-1.042-.018.751.751 0 0 1-.018-1.042L6.94 8 3.72 4.78a.75.75 0 0 1 0-1.06Z",
  pencil:
    "M11.013 1.427a1.75 1.75 0 0 1 2.474 0l1.086 1.086a1.75 1.75 0 0 1 0 2.474l-8.61 8.61c-.21.21-.47.364-.756.445l-3.251.93a.75.75 0 0 1-.927-.928l.929-3.25c.081-.286.235-.547.445-.758l8.61-8.61Zm.176 4.823L9.75 4.81l-6.286 6.287a.253.253 0 0 0-.064.108l-.558 1.953 1.953-.558a.253.253 0 0 0 .108-.064Zm1.238-3.763a.25.25 0 0 0-.354 0L10.811 3.75l1.439 1.44 1.263-1.263a.25.25 0 0 0 0-.354Z",
  eye: "M8 2c1.981 0 3.671.992 4.933 2.078 1.27 1.091 2.187 2.345 2.637 3.023a1.62 1.62 0 0 1 0 1.798c-.45.678-1.367 1.932-2.637 3.023C11.67 13.008 9.981 14 8 14c-1.981 0-3.671-.992-4.933-2.078C1.797 10.83.88 9.576.43 8.898a1.62 1.62 0 0 1 0-1.798c.45-.677 1.367-1.931 2.637-3.022C4.33 2.992 6.019 2 8 2ZM1.679 7.932a.12.12 0 0 0 0 .136c.411.622 1.241 1.75 2.366 2.717C5.176 11.758 6.527 12.5 8 12.5c1.473 0 2.825-.742 3.955-1.715 1.124-.967 1.954-2.096 2.366-2.717a.12.12 0 0 0 0-.136c-.412-.621-1.242-1.75-2.366-2.717C10.824 4.242 9.473 3.5 8 3.5c-1.473 0-2.825.742-3.955 1.715-1.124.967-1.954 2.096-2.366 2.717ZM8 10a2 2 0 1 1-.001-3.999A2 2 0 0 1 8 10Z",
  alert:
    "M6.457 1.047c.659-1.234 2.427-1.234 3.086 0l6.082 11.378A1.75 1.75 0 0 1 14.082 15H1.918a1.75 1.75 0 0 1-1.543-2.575Zm1.763.707a.25.25 0 0 0-.44 0L1.698 13.132a.25.25 0 0 0 .22.368h12.164a.25.25 0 0 0 .22-.368Zm.53 3.996v2.5a.75.75 0 0 1-1.5 0v-2.5a.75.75 0 0 1 1.5 0ZM9 11a1 1 0 1 1-2 0 1 1 0 0 1 2 0Z",
  dash: "M2 7.75A.75.75 0 0 1 2.75 7h10a.75.75 0 0 1 0 1.5h-10A.75.75 0 0 1 2 7.75Z",
} as const;
/** 圆里的问号不要外圈，自己画的线。 */
const ASK = "M5.6 6.1a2.4 2.4 0 1 1 3.4 2.2c-.65.32-1 .82-1 1.5v.45M8 12.6v.01";

type Tone = "wait" | "bad" | "write" | "review" | "pass" | "gray";
const LOOK: Record<TaskState, [Tone, keyof typeof GLYPHS | "ask" | "count" | null]> = {
  needs_you: ["wait", "ask"],
  issues: ["bad", "count"],
  review_failed: ["bad", "alert"],
  crashed: ["bad", "x"],
  writing: ["write", "pencil"],
  reviewing: ["review", "eye"],
  passed: ["pass", "check"],
  reviewed: ["gray", "eye"],
  written: ["gray", "check"],
  ended: ["gray", "dash"],
  not_started: ["gray", null],
};

/**
 * 侧边栏里任务的状态（写代码的 Agent 和审阅合在一起，后端算好），和手机端同一套：纯色圆底加白色图标。
 * 琥珀色只表示等你确认，红色是要你处理的问题，绿色只表示审阅通过，蓝色在写，紫色在审，灰色是没有下一步。
 * 进行中的圆小一圈，外面一段弧线在转；未启动只有一个虚线圈。
 */
export function StateIcon({ task, size = 16 }: { task: Task; size?: number }) {
  const label = useStateLabel()(task);
  const [tone, glyph] = LOOK[task.state];
  const active = task.state === "writing" || task.state === "reviewing";
  const issues = task.review?.issues ?? 0;
  // 侧边栏里只有 16px，图标放大一些才看得清；进行中的圆小一圈，图标跟着小。
  const scale = (size < 24 ? 1.375 : 1) * (active ? 0.8 : 1);
  const at = 16 - 8 * scale;
  return (
    <svg className={`state-badge tone-${tone}`} width={size} height={size} viewBox="0 0 32 32" role="img" aria-label={label}>
      <title>{label}</title>
      {glyph === null ? (
        <circle className="state-dashed" cx="16" cy="16" r="14" />
      ) : (
        <>
          {active && (
            <>
              <circle className="state-track" cx="16" cy="16" r="14.5" />
              <path className="state-arc" d="M16 1.5a14.5 14.5 0 0 1 14.5 14.5" />
            </>
          )}
          <circle className="state-fill" cx="16" cy="16" r={active ? 11.5 : 16} />
          {glyph === "count" ? (
            <text className="state-count" x="16" y="16" dy=".36em" textAnchor="middle" fontSize={issues > 9 ? 15 : 19}>
              {issues > 9 ? "9+" : issues}
            </text>
          ) : glyph === "ask" ? (
            <path className="state-ask" transform={`translate(${at} ${at}) scale(${scale})`} d={ASK} />
          ) : (
            <path className="state-glyph" transform={`translate(${at} ${at}) scale(${scale})`} d={GLYPHS[glyph]} />
          )}
        </>
      )}
    </svg>
  );
}
