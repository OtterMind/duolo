import { useEffect, useRef } from "react";

import { errorMessage, useT } from "../i18n";

/** 启动时发现的问题（数据文件损坏后恢复了、配置写错了）：说明情况，看过才关。 */
export function Notice({ problems, onClose }: { problems: unknown[]; onClose(): void }) {
  const t = useT();
  const button = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    button.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      onClose();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose]);

  return (
    <div className="scrim">
      <div className="dialog" role="alertdialog">
        <h2>{t("notice.title")}</h2>
        {problems.map((problem, i) => (
          <p key={i} className="selectable">
            {errorMessage(t, problem)}
          </p>
        ))}
        <div className="dialog-actions">
          <button ref={button} className="btn primary" onClick={onClose}>
            {t("notice.ok")}
          </button>
        </div>
      </div>
    </div>
  );
}
