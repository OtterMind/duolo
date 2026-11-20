import { useEffect, useRef, useState } from "react";
import { Check, Folder, FolderPlus, Globe, Plus, SquarePen, X } from "lucide-react";

import { LOCALES, useI18n, useT } from "../i18n";
import type { Snapshot, Task } from "../lib/api";
import { statusKey, writerOf } from "../lib/format";
import type { View } from "../App";
import { AgentIcon } from "./AgentIcon";
import { StatusIcon } from "./StatusIcon";

interface Props {
  snap: Snapshot | null;
  view: View;
  onNewTask(project?: string): void;
  onOpenTask(task: number): void;
  onOpenReview(task: number): void;
  onCloseTask(task: number): void;
  onAddProject(): void;
}

const MIN_WIDTH = 200;
const MAX_WIDTH = 440;
const WIDTH_KEY = "duolo.sidebarWidth";

export function Sidebar({ snap, view, onNewTask, onOpenTask, onOpenReview, onCloseTask, onAddProject }: Props) {
  const t = useT();
  const [width, startResize] = useResizableWidth();
  const projects = snap?.projects ?? [];

  return (
    <aside className="sidebar" style={{ width }}>
      <div className="sidebar-top" data-tauri-drag-region />
      <button className="side-row new-task" onClick={() => onNewTask()}>
        <SquarePen className="side-icon" size={14} strokeWidth={1.75} />
        <span className="grow">{t("sidebar.newTask")}</span>
        <kbd>⌘N</kbd>
      </button>

      <div className="side-scroll">
        {projects.map((project) => (
          <section key={project.id} className="side-project">
            <div className="side-project-head">
              <Folder size={12} strokeWidth={1.75} />
              <span className="grow ellipsis" title={project.root}>
                {project.name}
              </span>
              <button
                className="icon-btn"
                title={t("sidebar.newTaskIn", { project: project.name })}
                onClick={() => onNewTask(project.id)}
              >
                <Plus size={13} strokeWidth={1.75} />
              </button>
            </div>
            {project.tasks.length === 0 && <div className="side-empty">{t("sidebar.noTasks")}</div>}
            {project.tasks.map((task) => (
              <TaskRows
                key={task.id}
                task={task}
                current={view.kind === "task" && view.task === task.id}
                onOpen={() => onOpenTask(task.id)}
                onOpenReview={() => onOpenReview(task.id)}
                onClose={() => onCloseTask(task.id)}
              />
            ))}
          </section>
        ))}
        {snap && projects.length === 0 && <div className="side-empty">{t("sidebar.noProjects")}</div>}
        <button className="side-row add-project" onClick={onAddProject}>
          <FolderPlus className="side-icon" size={14} strokeWidth={1.75} />
          <span className="grow">{t("sidebar.addProject")}</span>
        </button>
      </div>

      <footer className="sidebar-foot">
        <span className="grow foot-stats">
          {snap && snap.running > 0 && (
            <span title={t("sidebar.running", { count: snap.running })}>
              <span className="status status-running" />
              {snap.running}
            </span>
          )}
          {snap && snap.needsInput > 0 && (
            <span className="need" title={t("sidebar.needsYou", { count: snap.needsInput })}>
              <span className="status status-needs_input" />
              {snap.needsInput}
            </span>
          )}
        </span>
        <LanguageMenu />
      </footer>
      <div className="sidebar-resizer" onMouseDown={startResize} onDoubleClick={() => startResize(null)} />
    </aside>
  );
}

/** 侧边栏底部的语言切换。 */
function LanguageMenu() {
  const { pref, setPref, t } = useI18n();
  const [open, setOpen] = useState(false);
  useEffect(() => {
    if (!open) return;
    const close = () => setOpen(false);
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [open]);
  const choose = (next: typeof pref) => {
    setPref(next);
    setOpen(false);
  };
  return (
    <div className="lang" onMouseDown={(e) => e.stopPropagation()}>
      <button className="icon-btn" title={t("sidebar.language")} onClick={() => setOpen((o) => !o)}>
        <Globe size={14} strokeWidth={1.75} />
      </button>
      {open && (
        <div className="menu lang-menu">
          <button className="menu-item" onClick={() => choose("auto")}>
            <span className="check">{pref === "auto" && <Check size={13} />}</span>
            <span className="grow">{t("sidebar.languageAuto")}</span>
          </button>
          <div className="menu-sep" />
          {LOCALES.map((l) => (
            <button key={l.code} className="menu-item" onClick={() => choose(l.code)}>
              <span className="check">{pref === l.code && <Check size={13} />}</span>
              <span className="grow">{l.name}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

/** 一个任务一行：模型图标、标题、审阅和进度的小图标。文字说明都放在悬停提示里。 */
function TaskRows({
  task,
  current,
  onOpen,
  onOpenReview,
  onClose,
}: {
  task: Task;
  current: boolean;
  onOpen(): void;
  onOpenReview(): void;
  onClose(): void;
}) {
  const t = useT();
  const writer = writerOf(task);
  const reviewing = task.reviewingSince != null;
  const status = writer?.status ?? "not_started";
  const title = task.prompt ? task.title : t("task.untitled");
  return (
    <div
      className={`side-row task-row ${current ? "is-current" : ""}`}
      role="button"
      tabIndex={0}
      onClick={onOpen}
      title={`${title}\n${writer ? `${writer.name} · ${t(statusKey(status))}` : ""}`}
    >
      <AgentIcon kind={writer?.kind ?? ""} title={writer?.name} />
      <span className={`grow ellipsis ${task.prompt ? "" : "untitled"}`}>{title}</span>
      {(reviewing || task.review) && (
        <button
          className={`review-chip ${reviewing ? "is-running" : task.review?.ok ? "" : "is-failed"}`}
          title={`${t("sidebar.review")} · ${
            reviewing ? t("sidebar.reviewRunning") : task.review!.ok ? task.review!.summary : t("sidebar.reviewFailed")
          }`}
          onClick={(e) => {
            e.stopPropagation();
            onOpenReview();
          }}
        >
          <AgentIcon kind="codex" size={12} />
        </button>
      )}
      <span className="row-status">
        <StatusIcon status={status} />
      </span>
      <button
        className="icon-btn close"
        title={t("sidebar.closeTask")}
        onClick={(e) => {
          e.stopPropagation();
          onClose();
        }}
      >
        <X size={13} strokeWidth={1.75} />
      </button>
    </div>
  );
}

/** 侧边栏宽度：拖右边缘调整，双击恢复默认，记在本机。 */
function useResizableWidth(): [number, (e: React.MouseEvent | null) => void] {
  const DEFAULT = 264;
  const [width, setWidth] = useState(() => {
    try {
      const saved = Number(localStorage.getItem(WIDTH_KEY));
      return saved >= MIN_WIDTH && saved <= MAX_WIDTH ? saved : DEFAULT;
    } catch {
      return DEFAULT;
    }
  });
  const widthRef = useRef(width);
  widthRef.current = width;

  const save = (w: number) => {
    try {
      localStorage.setItem(WIDTH_KEY, String(w));
    } catch {
      // 存不了就只在本次生效。
    }
  };

  const start = (e: React.MouseEvent | null) => {
    if (!e) {
      setWidth(DEFAULT);
      save(DEFAULT);
      return;
    }
    e.preventDefault();
    const startX = e.clientX;
    const startWidth = widthRef.current;
    document.body.classList.add("is-resizing");
    const move = (ev: MouseEvent) =>
      setWidth(Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, startWidth + ev.clientX - startX)));
    const up = () => {
      document.body.classList.remove("is-resizing");
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      save(widthRef.current);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };
  return [width, start];
}
