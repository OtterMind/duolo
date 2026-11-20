import { Check, X } from "lucide-react";

import type { Status } from "../lib/api";
import { statusKey } from "../lib/format";
import { useT } from "../i18n";

/**
 * 状态用小图标表示，悬停显示文字：转圈 = 运行中，琥珀色圆点 = 等你确认，
 * 空心圆 = 等待输入，对勾 = 已结束，红叉 = 异常退出，未启动不显示。
 */
export function StatusIcon({ status }: { status: Status }) {
  const t = useT();
  const label = t(statusKey(status));
  if (status === "exited") return <Check className="status-glyph" size={12} strokeWidth={2} aria-label={label} />;
  if (status === "failed") return <X className="status-glyph is-failed" size={12} strokeWidth={2} aria-label={label} />;
  return <span className={`status status-${status}`} title={label} aria-label={label} />;
}
