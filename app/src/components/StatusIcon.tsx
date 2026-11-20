import type { Status } from "../lib/api";
import { STATUS_LABEL } from "../lib/format";

/** 状态图标：转圈 = 运行中，琥珀色实心 = 等你确认，空心 = 等待输入。 */
export function StatusIcon({ status }: { status: Status }) {
  return <span className={`status status-${status}`} title={STATUS_LABEL[status]} aria-label={STATUS_LABEL[status]} />;
}
