import { pluginOf } from "../lib/plugins";

/** Agent 的图标：用图形区分模型，不写名字。图标来自插件（单色 SVG），没有的显示首字母。 */
export function AgentIcon({ kind, size = 14, title }: { kind: string; size?: number; title?: string }) {
  const plugin = pluginOf(kind);
  if (!plugin?.icon) {
    return (
      <span className="agent-icon agent-letter" style={{ width: size, height: size }} title={title}>
        {kind.slice(0, 1).toUpperCase()}
      </span>
    );
  }
  // 当作遮罩而不是插进页面：颜色能跟随文字，SVG 里的脚本也不会运行。
  const mask = `url("data:image/svg+xml,${encodeURIComponent(plugin.icon)}") center / contain no-repeat`;
  return (
    <span
      className="agent-icon agent-mask"
      role="img"
      aria-label={title}
      title={title}
      style={{ width: size, height: size, mask, WebkitMask: mask, backgroundColor: plugin.color ?? "currentColor" }}
    />
  );
}
