import { useRef, useState } from "react";
import { Folder, FolderOpen, FolderPlus, Github, Plus } from "lucide-react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import { useT } from "../i18n";
import { useModalFocus } from "../lib/modal";
import type { CloneProgress } from "../lib/api";
import { cloneOverall, tildify, useProjectsHome, type ProjectAdder } from "../lib/projects";

const ICON = { size: 15, strokeWidth: 1.75 } as const;

/**
 * 添加项目：打开现有文件夹、新建文件夹、从 GitHub 克隆，下面列出这台 Mac 上找到的仓库。
 * 新建和克隆默认放在 ~/DuoloProjects。在侧边栏"添加项目"（⌘O）的弹窗里和首次打开页里共用。
 */
export function AddProject({ adder, onAdded }: { adder: ProjectAdder; onAdded(id: string): void }) {
  const t = useT();
  const [mode, setMode] = useState<"list" | "create" | "clone">("list");
  const done = (id: string | null) => id && onAdded(id);

  return (
    <div className="add-project">
      <div className="add-options">
        <button className="add-option" disabled={adder.busy} onClick={() => void adder.choose().then(done)}>
          <FolderOpen {...ICON} />
          <span>{t("addProject.open")}</span>
        </button>
        <button
          className={`add-option ${mode === "create" ? "is-active" : ""}`}
          onClick={() => setMode(mode === "create" ? "list" : "create")}
        >
          <FolderPlus {...ICON} />
          <span>{t("addProject.create")}</span>
        </button>
        <button
          className={`add-option ${mode === "clone" ? "is-active" : ""}`}
          onClick={() => setMode(mode === "clone" ? "list" : "clone")}
        >
          <Github {...ICON} />
          <span>{t("addProject.clone")}</span>
        </button>
      </div>

      {mode === "create" && <CreateFolder adder={adder} onDone={done} onCancel={() => setMode("list")} />}
      {mode === "clone" && <CloneRepo adder={adder} onDone={done} onCancel={() => setMode("list")} />}
      {mode === "list" && adder.repos.length > 0 && (
        <div className="repo-list">
          <div className="repo-list-head">{t("addProject.found")}</div>
          {adder.repos.slice(0, 6).map((r) => (
            <button key={r.path} className="repo" disabled={adder.busy} onClick={() => void adder.add(r.path).then(done)}>
              <Folder size={14} strokeWidth={1.75} className="dim" />
              <span className="repo-name">{r.name}</span>
              <span className="meta ellipsis">{tildify(r.path)}</span>
              <span className="grow" />
              <Plus size={14} strokeWidth={1.75} className="dim" />
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

/** 新建文件夹：输入名字，默认放在 ~/DuoloProjects，可以换位置。 */
function CreateFolder({
  adder,
  onDone,
  onCancel,
}: {
  adder: ProjectAdder;
  onDone(id: string | null): void;
  onCancel(): void;
}) {
  const t = useT();
  const home = useProjectsHome();
  const [name, setName] = useState("");
  const [parent, setParent] = useState<string | null>(null);
  const where = tildify(parent ?? home);
  return (
    <form
      className="add-form"
      onSubmit={(e) => {
        e.preventDefault();
        if (name.trim()) void adder.create(name.trim(), parent).then(onDone);
      }}
    >
      <input
        autoFocus
        className="text-input"
        value={name}
        placeholder={t("addProject.folderName")}
        aria-label={t("addProject.folderName")}
        onChange={(e) => setName(e.target.value)}
        onKeyDown={(e) => e.key === "Escape" && (e.stopPropagation(), onCancel())}
      />
      <div className="add-form-row">
        <span className="meta ellipsis grow">
          {t("addProject.location", { path: name.trim() ? `${where}/${name.trim()}` : where })}
        </span>
        <button
          type="button"
          className="link-btn"
          onClick={() =>
            void openDialog({ directory: true, title: t("addProject.chooseLocation") }).then(
              (dir) => typeof dir === "string" && setParent(dir),
            )
          }
        >
          {t("addProject.changeLocation")}
        </button>
        <button type="submit" className="btn primary small" disabled={!name.trim() || adder.busy}>
          {adder.busy ? <span className="status status-running" /> : t("addProject.createAction")}
        </button>
      </div>
    </form>
  );
}

/** 从 GitHub（或任何 git 地址）克隆到 ~/DuoloProjects。 */
function CloneRepo({
  adder,
  onDone,
  onCancel,
}: {
  adder: ProjectAdder;
  onDone(id: string | null): void;
  onCancel(): void;
}) {
  const t = useT();
  const home = useProjectsHome();
  const [url, setUrl] = useState("");
  return (
    <form
      className="add-form"
      onSubmit={(e) => {
        e.preventDefault();
        if (url.trim()) void adder.clone(url.trim()).then(onDone);
      }}
    >
      <input
        autoFocus
        className="text-input mono"
        value={url}
        placeholder="https://github.com/owner/repo.git"
        aria-label={t("addProject.clone")}
        onChange={(e) => setUrl(e.target.value)}
        onKeyDown={(e) => e.key === "Escape" && (e.stopPropagation(), onCancel())}
      />
      {adder.cloning ? (
        <CloneProgressRow progress={adder.cloning} onCancel={adder.cancelClone} />
      ) : (
        <div className="add-form-row">
          <span className="meta ellipsis grow">{t("addProject.cloneInto", { path: tildify(home) })}</span>
          <button type="submit" className="btn primary small" disabled={!url.trim() || adder.busy}>
            {t("addProject.cloneAction")}
          </button>
        </div>
      )}
    </form>
  );
}

/** 克隆每一步的说明。 */
const PHASE_LABEL = {
  connecting: "clone.connecting",
  preparing: "clone.preparing",
  receiving: "clone.receiving",
  resolving: "clone.resolving",
  checkingOut: "clone.checkingOut",
} as const satisfies Record<CloneProgress["phase"], string>;

/** 克隆中：哪一步、整体进度条、下载的大小和速度，和取消按钮。 */
function CloneProgressRow({ progress, onCancel }: { progress: CloneProgress; onCancel(): void }) {
  const t = useT();
  const overall = cloneOverall(progress);
  const step = t(PHASE_LABEL[progress.phase]);
  const label = progress.percent != null && progress.phase !== "connecting" ? `${step} ${progress.percent}%` : step;
  return (
    <div className="clone-progress" role="status" aria-live="polite">
      <div className="add-form-row">
        <span className="ellipsis grow">{label}</span>
        {progress.detail && <span className="meta mono">{progress.detail}</span>}
        <button type="button" className="btn small" onClick={onCancel}>
          {t("common.cancel")}
        </button>
      </div>
      <div
        className={`clone-bar ${progress.phase === "connecting" ? "is-waiting" : ""}`}
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={overall}
      >
        <span style={{ width: `${overall}%` }} />
      </div>
    </div>
  );
}

/** 侧边栏"添加项目"（⌘O）打开的弹窗。 */
export function AddProjectDialog({
  adder,
  onAdded,
  onClose,
}: {
  adder: ProjectAdder;
  onAdded(id: string): void;
  onClose(): void;
}) {
  const t = useT();
  const dialog = useRef<HTMLDivElement>(null);
  useModalFocus(dialog);
  return (
    <div
      className="scrim panel-scrim"
      onMouseDown={(e) => e.target === e.currentTarget && onClose()}
      onKeyDown={(e) => e.key === "Escape" && onClose()}
    >
      <div ref={dialog} tabIndex={-1} className="dialog add-project-dialog" role="dialog" aria-label={t("addProject.title")}>
        <h2>{t("addProject.title")}</h2>
        <AddProject adder={adder} onAdded={onAdded} />
      </div>
    </div>
  );
}
