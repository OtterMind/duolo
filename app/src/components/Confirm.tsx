import { useEffect, useRef, useState } from "react";

import { useT } from "../i18n";
import { useModalFocus } from "../lib/modal";

export interface ConfirmSpec {
  title: string;
  body: string;
  action: string;
  danger?: boolean;
  run(): Promise<void>;
}

/** 确认框：↵ 确认，Esc 取消。 */
export function Confirm({ spec, onDone, onError }: { spec: ConfirmSpec; onDone(): void; onError(err: unknown): void }) {
  const t = useT();
  const [running, setRunning] = useState(false);
  const button = useRef<HTMLButtonElement>(null);

  useModalFocus(button);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onDone();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onDone]);

  const run = async () => {
    setRunning(true);
    try {
      await spec.run();
    } catch (err) {
      onError(err);
    }
    onDone();
  };

  return (
    <div className="scrim" onMouseDown={onDone}>
      <div className="dialog" role="dialog" onMouseDown={(e) => e.stopPropagation()}>
        <h2>{spec.title}</h2>
        <p>{spec.body}</p>
        <div className="dialog-actions">
          <button className="btn" onClick={onDone}>
            {t("common.cancel")}
          </button>
          <button ref={button} className={`btn primary ${spec.danger ? "danger" : ""}`} disabled={running} onClick={run}>
            {running ? t("common.pleaseWait") : spec.action}
          </button>
        </div>
      </div>
    </div>
  );
}
