import { Check as CheckIcon, Mic, Plus, Upload, X } from "lucide-react";

import { useT } from "../i18n";
import type { Check, Snapshot } from "../lib/api";
import type { ProjectAdder } from "../lib/projects";
import { AddProject } from "./AddProject";
import { kbd } from "../lib/platform";

interface Props {
  snap: Snapshot | null;
  env: Check[] | null;
  creating: boolean;
  /** 正在把文件夹拖进窗口。 */
  dragging: boolean;
  adder: ProjectAdder;
  onNewTask(): void;
  /** 加好了项目，直接进它的命令行。 */
  onProjectAdded(id: string): void;
}

const ICON = { size: 14, strokeWidth: 1.75 } as const;

/** 右侧没有任务可显示时：第一次打开（还没有项目），或者任务都关掉了。 */
export function Empty({ snap, env, creating, dragging, adder, onNewTask, onProjectAdded }: Props) {
  const t = useT();
  const noProject = snap !== null && snap.projects.length === 0;
  return (
    <div className="empty">
      <div className="topbar" data-tauri-drag-region />
      {!snap || creating ? (
        <div className="empty-body" data-tauri-drag-region>
          <p className="empty-note">{creating ? t("empty.preparing") : ""}</p>
        </div>
      ) : noProject ? (
        <Welcome env={env} dragging={dragging} adder={adder} onProjectAdded={onProjectAdded} />
      ) : (
        <div className="empty-body" data-tauri-drag-region>
          <p className="empty-note">{t("empty.noOpenTask")}</p>
          <button className="btn" onClick={onNewTask}>
            <Plus {...ICON} />
            {t("empty.newTask")} <kbd>{kbd("⌘N")}</kbd>
          </button>
        </div>
      )}
    </div>
  );
}

/** 第一次打开：拖文件夹进来、选文件夹、克隆，或者点常见目录里找到的仓库。 */
function Welcome({
  env,
  dragging,
  adder,
  onProjectAdded,
}: {
  env: Check[] | null;
  dragging: boolean;
  adder: ProjectAdder;
  onProjectAdded(id: string): void;
}) {
  const t = useT();
  return (
    <div className="welcome">
      <h1>{t("welcome.title")}</h1>
      <p className="empty-note">{t("welcome.body")}</p>
      <div className={`dropzone ${dragging ? "is-over" : ""}`}>
        <Upload size={20} strokeWidth={1.75} />
        <span>{t("welcome.drop")}</span>
      </div>
      <AddProject adder={adder} onAdded={onProjectAdded} />
      <EnvLine env={env} />
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
        const voice = c.kind === "voice";
        const icon = voice ? <Mic size={12} /> : c.ok ? <CheckIcon size={12} /> : <X size={12} />;
        const label = voice ? (c.ok ? t("env.voice") : t("welcome.voiceLater")) : c.name;
        return (
          <span key={c.kind + c.name} className={c.ok ? "ok" : voice ? "" : "bad"}>
            {icon}
            {label}
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
