import { Check as CheckIcon, FolderOpen, Plus, X } from "lucide-react";

import { useT } from "../i18n";
import type { Check, Snapshot } from "../lib/api";

interface Props {
  snap: Snapshot | null;
  env: Check[] | null;
  creating: boolean;
  onNewTask(): void;
  onAddProject(): void;
}

/** 右侧没有任务可显示时：还没有项目，或者任务都关掉了。 */
export function Empty({ snap, env, creating, onNewTask, onAddProject }: Props) {
  const t = useT();
  const noProject = snap !== null && snap.projects.length === 0;
  return (
    <div className="empty">
      <div className="topbar" data-tauri-drag-region />
      <div className="empty-body" data-tauri-drag-region>
        {!snap || creating ? (
          <p className="empty-note">{creating ? t("empty.preparing") : ""}</p>
        ) : noProject ? (
          <>
            <h1>{t("empty.addProjectTitle")}</h1>
            <p className="empty-note">{t("empty.addProjectBody")}</p>
            <button className="btn primary" onClick={onAddProject}>
              <FolderOpen size={14} strokeWidth={1.75} />
              {t("empty.chooseFolder")}
            </button>
          </>
        ) : (
          <>
            <p className="empty-note">{t("empty.noOpenTask")}</p>
            <button className="btn" onClick={onNewTask}>
              <Plus size={14} strokeWidth={1.75} />
              {t("empty.newTask")} <kbd>⌘N</kbd>
            </button>
          </>
        )}
        <EnvLine env={env} />
      </div>
    </div>
  );
}

/** 环境检查：git、Agent CLI、语音。缺少 Agent 时提示安装。 */
function EnvLine({ env }: { env: Check[] | null }) {
  const t = useT();
  if (!env || env.length === 0) return null;
  const missing = env.filter((c) => !c.ok && c.kind !== "voice");
  return (
    <p className="env">
      {env.map((c) => {
        const name = c.kind === "voice" ? t("env.voice") : c.name;
        const icon = c.ok ? <CheckIcon size={12} /> : c.kind === "voice" ? null : <X size={12} />;
        return (
          <span key={c.kind + c.name} className={c.ok ? "ok" : c.kind === "voice" ? "" : "bad"}>
            {icon}
            {name}
            {c.ok && c.version ? ` ${c.version}` : ""}
          </span>
        );
      })}
      {missing.length > 0 && (
        <span className="bad block">
          {missing.map((c) => t("env.missing", { command: c.command })).join(" · ")} — {t("env.installHint")}
        </span>
      )}
    </p>
  );
}
