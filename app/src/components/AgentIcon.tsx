/** Agent 的图标：用图形区分模型，不写名字。未知的 Agent 显示首字母。 */
export function AgentIcon({ kind, size = 14, title }: { kind: string; size?: number; title?: string }) {
  if (kind === "claude") {
    // 放射状星芒，Claude 的标志色。
    const rays = Array.from({ length: 12 }, (_, i) => i * 30);
    return (
      <svg className="agent-icon" width={size} height={size} viewBox="0 0 16 16" aria-label={title}>
        {title && <title>{title}</title>}
        {rays.map((deg, i) => (
          <line
            key={deg}
            x1="8"
            y1="8"
            x2="8"
            y2={i % 2 ? 2.6 : 1.2}
            stroke="#d97757"
            strokeWidth="1.7"
            strokeLinecap="round"
            transform={`rotate(${deg} 8 8)`}
          />
        ))}
      </svg>
    );
  }
  if (kind === "codex") {
    // 三个交叠的椭圆，类似花结。
    return (
      <svg className="agent-icon" width={size} height={size} viewBox="0 0 16 16" aria-label={title}>
        {title && <title>{title}</title>}
        {[0, 60, 120].map((deg) => (
          <ellipse
            key={deg}
            cx="8"
            cy="8"
            rx="6.2"
            ry="2.9"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.3"
            transform={`rotate(${deg} 8 8)`}
          />
        ))}
      </svg>
    );
  }
  return (
    <span className="agent-icon agent-letter" style={{ width: size, height: size }} title={title}>
      {kind.slice(0, 1).toUpperCase()}
    </span>
  );
}
