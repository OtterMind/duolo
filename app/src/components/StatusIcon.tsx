import { Check, CircleAlert, X } from "lucide-react";

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

/**
 * 侧边栏里任务的状态（写代码的 Agent 和审阅合在一起，后端算好）。颜色和手机端一样：
 * 琥珀色只表示等你确认，红色是要你处理的问题，绿色只表示审阅通过，蓝色在写，紫色在审，灰色是没有下一步。
 */
export function StateIcon({ task }: { task: Task }) {
  const label = useStateLabel()(task);
  const glyph = { size: 13, strokeWidth: 2, "aria-label": label, role: "img" } as const;
  const state: TaskState = task.state;
  switch (state) {
    case "needs_you":
      return <span className="status status-needs_input" title={label} aria-label={label} role="img" />;
    case "issues":
      return (
        <span className="state-issues" title={label}>
          <CircleAlert className="status-glyph is-failed" {...glyph} />
          {task.review?.issues ?? 0}
        </span>
      );
    case "review_failed":
      return <CircleAlert className="status-glyph is-failed" {...glyph} />;
    case "crashed":
      return <X className="status-glyph is-failed" {...glyph} />;
    case "writing":
    case "reviewing":
      return <span className={`status status-running is-${state}`} title={label} aria-label={label} role="img" />;
    case "passed":
      return <Check className="status-glyph is-passed" {...glyph} />;
    case "reviewed":
    case "written":
      return <Check className="status-glyph" {...glyph} />;
    case "ended":
      return <span className="status status-exited" title={label} aria-label={label} role="img" />;
    case "not_started":
      return null;
  }
}
